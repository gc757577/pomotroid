//! Save only settled normal timer geometry. Coordinates and sizes are physical
//! outer-window pixels, so restoration never mixes client and frame dimensions.
use crate::{db::DbState, settings};
use serde::{Deserialize, Serialize};

const KEY: &str = "timer_window_state"; // Previous single-mode geometry.
const LAYOUT_KEY: &str = "timer_window_layout";
const MINI_WIDTH: f64 = 200.0;
const MINI_HEIGHT: f64 = 88.0;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
struct Anchor {
    right: i64,
    bottom: i64,
}

fn anchor(geometry: Geometry) -> Anchor {
    Anchor {
        right: geometry.x as i64 + geometry.width as i64,
        bottom: geometry.y as i64 + geometry.height as i64,
    }
}

fn anchored(mut geometry: Geometry, corner: Anchor) -> Geometry {
    geometry.x =
        (corner.right - geometry.width as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    geometry.y =
        (corner.bottom - geometry.height as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    geometry
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
struct Layout {
    horizontal: bool,
    normal: Geometry,
    mini: Option<Geometry>,
    #[serde(default)]
    corner: Option<Anchor>,
}
impl Layout {
    fn geometry(self) -> Geometry {
        let geometry = if self.horizontal {
            self.mini.unwrap_or(self.normal)
        } else {
            self.normal
        };
        self.corner
            .map(|corner| anchored(geometry, corner))
            .unwrap_or(geometry)
    }
    fn record(&mut self, geometry: Geometry) {
        self.corner = Some(anchor(geometry));
        if self.horizontal {
            self.mini = Some(geometry);
        } else {
            self.normal = geometry;
        }
    }
    fn toggled(mut self) -> Self {
        self.horizontal = !self.horizontal;
        self
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
struct Geometry {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    scale: f64,
}

impl Geometry {
    fn valid(self) -> bool {
        self.width > 0 && self.height > 0 && self.scale.is_finite() && self.scale > 0.0
    }
}

#[derive(Clone, Copy)]
struct Monitor {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    scale: f64,
}

fn normalize(saved: Geometry, monitors: &[Monitor]) -> Option<Geometry> {
    normalize_size(saved, monitors, 240.0, 240.0)
}

fn normalize_size(
    saved: Geometry,
    monitors: &[Monitor],
    min_width: f64,
    min_height: f64,
) -> Option<Geometry> {
    if !saved.valid() {
        return None;
    }
    // Choose the nearest work area, including monitors left of the primary.
    let center_x = saved.x as f64 + saved.width as f64 / 2.0;
    let center_y = saved.y as f64 + saved.height as f64 / 2.0;
    let distance = |m: &&Monitor| {
        let dx =
            (m.x as f64 - center_x).max(0.0) + (center_x - (m.x as f64 + m.width as f64)).max(0.0);
        let dy =
            (m.y as f64 - center_y).max(0.0) + (center_y - (m.y as f64 + m.height as f64)).max(0.0);
        dx * dx + dy * dy
    };
    let monitor = monitors
        .iter()
        .filter(|m| m.width > 0 && m.height > 0 && m.scale.is_finite() && m.scale > 0.0)
        .min_by(|a, b| distance(a).total_cmp(&distance(b)))?;
    let ratio = monitor.scale / saved.scale;
    let min_size = (min_width * monitor.scale).round() as u32;
    let width = ((saved.width as f64 * ratio).round() as u32)
        .max(min_size)
        .min(monitor.width);
    let height = ((saved.height as f64 * ratio).round() as u32)
        .max((min_height * monitor.scale).round() as u32)
        .min(monitor.height);
    Some(Geometry {
        x: (saved.x as i64).clamp(
            monitor.x as i64,
            monitor.x as i64 + (monitor.width - width) as i64,
        ) as i32,
        y: (saved.y as i64).clamp(
            monitor.y as i64,
            monitor.y as i64 + (monitor.height - height) as i64,
        ) as i32,
        width,
        height,
        scale: monitor.scale,
    })
}

#[cfg(test)]
fn save(db: &DbState, geometry: Geometry) -> Result<(), String> {
    let value = serde_json::to_string(&geometry).map_err(|e| e.to_string())?;
    let conn = db.lock().map_err(|e| e.to_string())?;
    // A single SQLite statement saves the rectangle and DPI atomically.
    settings::save_setting(&conn, KEY, &value).map_err(|e| e.to_string())
}

fn save_layout(db: &DbState, layout: Layout) -> Result<(), String> {
    let value = serde_json::to_string(&layout).map_err(|e| e.to_string())?;
    let conn = db.lock().map_err(|e| e.to_string())?;
    settings::save_setting(&conn, LAYOUT_KEY, &value).map_err(|e| e.to_string())
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use std::cell::RefCell;
    use windows_sys::Win32::{
        Foundation::{GetLastError, HWND, RECT},
        Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromRect, RedrawWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
            RDW_ALLCHILDREN, RDW_FRAME, RDW_INVALIDATE,
        },
        UI::{
            HiDpi::GetDpiForWindow,
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    };

    const SUBCLASS: usize = 0x504f4d53;
    const SAVE_TIMER: usize = 0x504f4d50;
    #[derive(Clone, Copy, Serialize)]
    pub struct DisplayPlan {
        id: u64,
        horizontal: bool,
        width: f64,
        height: f64,
    }
    struct PendingSwitch {
        plan: DisplayPlan,
        previous: Layout,
        next: Layout,
        from: Geometry,
        monitor: Monitor,
    }

    fn take_pending(pending: &mut Option<PendingSwitch>, id: u64) -> Option<PendingSwitch> {
        if pending.as_ref().is_some_and(|p| p.plan.id == id) {
            pending.take()
        } else {
            None
        }
    }

    struct State {
        db: DbState,
        current: Geometry,
        layout: Layout,
        saved: Option<Layout>,
        dragging: bool,
        switching: bool,
        destroying: bool,
        pending: Option<PendingSwitch>,
        next_id: u64,
    }
    impl State {
        fn snapshot(&self) -> Layout {
            let mut layout = self.layout;
            layout.record(self.current);
            layout
        }
    }
    thread_local! { static STATE: RefCell<Option<State>> = const { RefCell::new(None) }; }

    unsafe fn rectangle(hwnd: HWND) -> Option<Geometry> {
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return None;
        }
        let geometry = Geometry {
            x: rect.left,
            y: rect.top,
            width: (rect.right as i64 - rect.left as i64).max(0) as u32,
            height: (rect.bottom as i64 - rect.top as i64).max(0) as u32,
            scale: GetDpiForWindow(hwnd) as f64 / 96.0,
        };
        geometry.valid().then_some(geometry)
    }

    fn capture_allowed(
        visible: bool,
        minimized: bool,
        maximized: bool,
        allowed_style: bool,
    ) -> bool {
        visible && !minimized && !maximized && allowed_style
    }

    unsafe fn capture(hwnd: HWND) {
        // Tao removes WS_THICKFRAME for fullscreen, but retains it for the
        // resizable undecorated timer. Hidden/tray windows keep the cached value.
        let mode = STATE.with(|state| {
            state
                .borrow()
                .as_ref()
                .filter(|s| !s.switching && !s.destroying)
                .map(|s| s.layout.horizontal)
        });
        let Some(horizontal) = mode else {
            return;
        };
        if !capture_allowed(
            IsWindowVisible(hwnd) != 0,
            IsIconic(hwnd) != 0,
            IsZoomed(hwnd) != 0,
            horizontal || GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_THICKFRAME != 0,
        ) {
            return;
        }
        if let Some(geometry) = rectangle(hwnd) {
            STATE.with(|state| {
                if let Some(state) = state.borrow_mut().as_mut() {
                    state.current = geometry;
                }
            });
        }
    }

    fn flush() {
        // No RefCell borrow is held while calling Windows or SQLite.
        let pending = STATE.with(|state| {
            state
                .borrow()
                .as_ref()
                .filter(|s| !s.switching && s.saved != Some(s.snapshot()))
                .map(|s| (s.db.clone(), s.snapshot()))
        });
        if let Some((db, layout)) = pending {
            match save_layout(&db, layout) {
                Ok(()) => {
                    STATE.with(|state| {
                        if let Some(state) = state.borrow_mut().as_mut() {
                            state.saved = Some(layout);
                        }
                    });
                    log::debug!("[window-state] saved {layout:?}");
                }
                Err(error) => log::error!("[window-state] save failed: {error}"),
            }
        }
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: usize,
        lparam: isize,
        subclass: usize,
        _data: usize,
    ) -> isize {
        if matches!(message, WM_DESTROY | WM_NCDESTROY) {
            STATE.with(|state| {
                if let Some(state) = state.borrow_mut().as_mut() {
                    state.destroying = true;
                }
            });
        }
        if matches!(
            message,
            WM_CLOSE
                | WM_DESTROY
                | WM_NCDESTROY
                | WM_ENTERSIZEMOVE
                | WM_DPICHANGED
                | WM_DISPLAYCHANGE
        ) || (message == WM_SHOWWINDOW && wparam == 0)
            || (message == WM_SIZE && wparam as u32 == SIZE_MINIMIZED)
        {
            STATE.with(|state| {
                if let Some(state) = state.borrow_mut().as_mut() {
                    state.pending = None;
                }
            });
        }
        let switching = STATE.with(|state| state.borrow().as_ref().is_some_and(|s| s.switching));
        if switching
            && (matches!(message, WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK)
                || (message == WM_SYSCOMMAND
                    && matches!(wparam as u32 & 0xfff0, SC_MOVE | SC_SIZE | SC_MAXIMIZE)))
        {
            return 0;
        }
        let horizontal =
            STATE.with(|state| state.borrow().as_ref().is_some_and(|s| s.layout.horizontal));
        if horizontal
            && message == WM_SYSCOMMAND
            && matches!(wparam as u32 & 0xfff0, SC_MAXIMIZE | SC_SIZE)
        {
            return 0;
        }
        // Capture before hide/close destroys access to the normal rectangle.
        if message == WM_CLOSE || message == WM_DESTROY || (message == WM_SHOWWINDOW && wparam == 0)
        {
            capture(hwnd);
            flush();
        }
        if message == WM_NCDESTROY {
            KillTimer(hwnd, SAVE_TIMER);
            flush();
            RemoveWindowSubclass(hwnd, Some(window_proc), subclass);
            let _ = STATE.try_with(|state| state.borrow_mut().take());
            return DefSubclassProc(hwnd, message, wparam, lparam);
        }
        let result = DefSubclassProc(hwnd, message, wparam, lparam);
        match message {
            WM_GETMINMAXINFO if lparam != 0 => {
                let limits = STATE.with(|state| {
                    state
                        .borrow()
                        .as_ref()
                        .map(|s| (s.switching, s.layout.horizontal, s.current))
                });
                if let Some((switching, horizontal, geometry)) = limits {
                    let info = &mut *(lparam as *mut MINMAXINFO);
                    if switching {
                        info.ptMinTrackSize.x = 1;
                        info.ptMinTrackSize.y = 1;
                    } else if horizontal {
                        let ratio = GetDpiForWindow(hwnd) as f64 / 96.0 / geometry.scale;
                        info.ptMinTrackSize.x = (geometry.width as f64 * ratio).round() as i32;
                        info.ptMinTrackSize.y = (geometry.height as f64 * ratio).round() as i32;
                        info.ptMaxTrackSize = info.ptMinTrackSize;
                    } else {
                        let minimum = (240.0 * GetDpiForWindow(hwnd) as f64 / 96.0).round() as i32;
                        info.ptMinTrackSize.x = minimum;
                        info.ptMinTrackSize.y = minimum;
                    }
                }
            }
            WM_ENTERSIZEMOVE => STATE.with(|state| {
                if let Some(state) = state.borrow_mut().as_mut() {
                    state.dragging = true;
                }
            }),
            WM_EXITSIZEMOVE => {
                STATE.with(|state| {
                    if let Some(state) = state.borrow_mut().as_mut() {
                        state.dragging = false;
                    }
                });
                KillTimer(hwnd, SAVE_TIMER);
                capture(hwnd);
                flush();
            }
            WM_WINDOWPOSCHANGED | WM_DPICHANGED => {
                capture(hwnd);
                // Covers keyboard placement, Snap and DPI changes outside a drag.
                if SetTimer(hwnd, SAVE_TIMER, 400, None) == 0 {
                    log::error!("[window-state] could not schedule save: {}", GetLastError());
                    flush();
                }
            }
            WM_TIMER if wparam == SAVE_TIMER => {
                KillTimer(hwnd, SAVE_TIMER);
                let dragging =
                    STATE.with(|state| state.borrow().as_ref().is_some_and(|s| s.dragging));
                if !dragging {
                    capture(hwnd);
                    flush();
                }
            }
            _ => {}
        }
        result
    }

    fn monitors(window: &tauri::WebviewWindow) -> Result<Vec<Monitor>, String> {
        Ok(window
            .available_monitors()
            .map_err(|e| e.to_string())?
            .iter()
            .map(|m| {
                let area = m.work_area();
                Monitor {
                    x: area.position.x,
                    y: area.position.y,
                    width: area.size.width,
                    height: area.size.height,
                    scale: m.scale_factor(),
                }
            })
            .collect())
    }

    fn apply_layout(window: &tauri::WebviewWindow, layout: Layout) -> Result<Geometry, String> {
        window.set_fullscreen(false).map_err(|e| e.to_string())?;
        window.unmaximize().map_err(|e| e.to_string())?;
        window
            .set_max_size(None::<tauri::LogicalSize<f64>>)
            .map_err(|e| e.to_string())?;
        window
            .set_min_size(Some(tauri::LogicalSize::new(
                if layout.horizontal { MINI_WIDTH } else { 240.0 },
                if layout.horizontal {
                    MINI_HEIGHT
                } else {
                    240.0
                },
            )))
            .map_err(|e| e.to_string())?;
        window
            .set_resizable(!layout.horizontal)
            .map_err(|e| e.to_string())?;
        window
            .set_maximizable(!layout.horizontal)
            .map_err(|e| e.to_string())?;
        let hwnd = window.hwnd().map_err(|e| e.to_string())?.0;
        let monitors = monitors(window)?;
        let saved = layout.geometry();
        let restored = if layout.horizontal {
            let target = normalize_size(saved, &monitors, MINI_WIDTH, MINI_HEIGHT).unwrap_or(saved);
            // Constraints and dimensions are logical client pixels. Persisted
            // geometry remains outer pixels, including any native frame offsets.
            let frame = unsafe { rectangle(hwnd) }.ok_or("Cannot read timer frame")?;
            let mut client = RECT::default();
            if unsafe { GetClientRect(hwnd, &mut client) } == 0 {
                return Err("Cannot read timer client".into());
            }
            let mini = Geometry {
                width: (MINI_WIDTH * target.scale).round() as u32
                    + frame.width.saturating_sub(client.right as u32),
                height: (MINI_HEIGHT * target.scale).round() as u32
                    + frame.height.saturating_sub(client.bottom as u32),
                ..target
            };
            window
                .set_max_size(Some(tauri::LogicalSize::new(MINI_WIDTH, MINI_HEIGHT)))
                .map_err(|e| e.to_string())?;
            normalize_size(mini, &monitors, MINI_WIDTH, MINI_HEIGHT).unwrap_or(mini)
        } else {
            normalize(saved, &monitors).unwrap_or(saved)
        };
        let restored = if let Some(corner) = layout.corner {
            let positioned = anchored(restored, corner);
            normalize_size(
                positioned,
                &monitors,
                if layout.horizontal { MINI_WIDTH } else { 240.0 },
                if layout.horizontal {
                    MINI_HEIGHT
                } else {
                    240.0
                },
            )
            .unwrap_or(positioned)
        } else {
            restored
        };
        unsafe {
            if SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                restored.x,
                restored.y,
                restored.width as i32,
                restored.height as i32,
                SWP_NOACTIVATE | SWP_NOZORDER,
            ) == 0
            {
                return Err(format!("Cannot restore timer geometry: {}", GetLastError()));
            }
        }
        unsafe { rectangle(hwnd) }.ok_or_else(|| "Cannot read restored timer geometry".into())
    }

    unsafe fn place(hwnd: HWND, geometry: Geometry) -> Result<(), String> {
        if IsIconic(hwnd) != 0 {
            // WINDOWPLACEMENT uses workspace coordinates for an ordinary app.
            // Update only its restore rectangle, preserving the minimized state.
            let mut placement: WINDOWPLACEMENT = std::mem::zeroed();
            placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
            if GetWindowPlacement(hwnd, &mut placement) == 0 {
                return Err(format!("GetWindowPlacement: {}", GetLastError()));
            }
            let mut rect = RECT {
                left: geometry.x,
                top: geometry.y,
                right: anchor(geometry).right as i32,
                bottom: anchor(geometry).bottom as i32,
            };
            let mut info: MONITORINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            if GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW == 0 {
                if GetMonitorInfoW(MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST), &mut info) == 0
                {
                    return Err(format!("GetMonitorInfo: {}", GetLastError()));
                }
                let dx = info.rcWork.left - info.rcMonitor.left;
                let dy = info.rcWork.top - info.rcMonitor.top;
                rect.left -= dx;
                rect.right -= dx;
                rect.top -= dy;
                rect.bottom -= dy;
            }
            placement.rcNormalPosition = rect;
            placement.flags &= !WPF_RESTORETOMAXIMIZED;
            if SetWindowPlacement(hwnd, &placement) == 0 {
                return Err(format!("SetWindowPlacement: {}", GetLastError()));
            }
        } else if SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            geometry.x,
            geometry.y,
            geometry.width as i32,
            geometry.height as i32,
            SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOREDRAW,
        ) == 0
        {
            return Err(format!("SetWindowPos: {}", GetLastError()));
        }
        Ok(())
    }

    fn target_geometry(
        window: &tauri::WebviewWindow,
        next: Layout,
        from: Geometry,
        monitor: Monitor,
    ) -> Result<Geometry, String> {
        let hwnd = window.hwnd().map_err(|e| e.to_string())?.0;
        let frame = unsafe { rectangle(hwnd) }.ok_or("Cannot read timer frame")?;
        let mut client = RECT::default();
        if unsafe { GetClientRect(hwnd, &mut client) } == 0 {
            return Err("Cannot read timer client".into());
        }
        let size = if next.horizontal {
            Geometry {
                width: (MINI_WIDTH * monitor.scale).round() as u32
                    + (frame.width.saturating_sub(client.right as u32) as f64 * monitor.scale
                        / frame.scale)
                        .round() as u32,
                height: (MINI_HEIGHT * monitor.scale).round() as u32
                    + (frame.height.saturating_sub(client.bottom as u32) as f64 * monitor.scale
                        / frame.scale)
                        .round() as u32,
                scale: monitor.scale,
                ..from
            }
        } else {
            Geometry {
                width: (next.normal.width as f64 * monitor.scale / next.normal.scale).round()
                    as u32,
                height: (next.normal.height as f64 * monitor.scale / next.normal.scale).round()
                    as u32,
                scale: monitor.scale,
                ..from
            }
        };
        let target = anchored(size, anchor(from));
        let target = normalize_size(
            target,
            &[monitor],
            if next.horizontal { MINI_WIDTH } else { 240.0 },
            if next.horizontal { MINI_HEIGHT } else { 240.0 },
        )
        .ok_or("Cannot fit timer to work area")?;
        Ok(target)
    }

    fn configure_mode(window: &tauri::WebviewWindow, horizontal: bool) -> Result<(), String> {
        window.set_fullscreen(false).map_err(|e| e.to_string())?;
        window.unmaximize().map_err(|e| e.to_string())?;
        window
            .set_max_size(None::<tauri::LogicalSize<f64>>)
            .map_err(|e| e.to_string())?;
        window
            .set_min_size(Some(tauri::LogicalSize::new(1.0, 1.0)))
            .map_err(|e| e.to_string())?;
        window
            .set_resizable(!horizontal)
            .map_err(|e| e.to_string())?;
        window
            .set_maximizable(!horizontal)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn prepare_display_mode(window: &tauri::WebviewWindow) -> Result<DisplayPlan, String> {
        let hwnd = window.hwnd().map_err(|e| e.to_string())?.0;
        if unsafe { IsIconic(hwnd) != 0 || IsWindowVisible(hwnd) == 0 } {
            return Err("Timer is not visible".into());
        }
        if let Some(plan) = STATE.with(|state| {
            state
                .borrow()
                .as_ref()
                .and_then(|s| s.pending.as_ref())
                .map(|p| p.plan)
        }) {
            return Ok(plan);
        }
        unsafe {
            capture(hwnd);
        }
        let previous = STATE.with(|state| {
            state
                .borrow()
                .as_ref()
                .map(|s| s.snapshot())
                .ok_or("Timer geometry is not ready")
        })?;
        let from = unsafe { rectangle(hwnd) }.ok_or("Cannot read timer geometry")?;
        let next = previous.toggled();
        // Hold the source monitor through a style change or unmaximize.
        let current = window
            .current_monitor()
            .map_err(|e| e.to_string())?
            .ok_or("No current timer monitor")?;
        let area = current.work_area();
        let monitor = Monitor {
            x: area.position.x,
            y: area.position.y,
            width: area.size.width,
            height: area.size.height,
            scale: current.scale_factor(),
        };
        let target = target_geometry(window, next, from, monitor)?;
        let mut client = RECT::default();
        if unsafe { GetClientRect(hwnd, &mut client) } == 0 {
            return Err("Cannot read timer client".into());
        }
        let frame_width = from.width.saturating_sub(client.right as u32) as f64 / from.scale;
        let frame_height = from.height.saturating_sub(client.bottom as u32) as f64 / from.scale;
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            let state = state.as_mut().ok_or("Timer geometry is not ready")?;
            let plan = DisplayPlan {
                id: state.next_id,
                horizontal: next.horizontal,
                width: target.width as f64 / target.scale - frame_width,
                height: target.height as f64 / target.scale - frame_height,
            };
            state.next_id += 1;
            state.pending = Some(PendingSwitch {
                plan,
                previous,
                next,
                from,
                monitor,
            });
            Ok(plan)
        })
    }

    pub fn display_mode(
        window: &tauri::WebviewWindow,
        toggle: bool,
        prepared: Option<u64>,
    ) -> Result<bool, String> {
        if !toggle {
            return STATE.with(|state| {
                state
                    .borrow()
                    .as_ref()
                    .map(|s| s.layout.horizontal)
                    .ok_or_else(|| "Timer geometry is not ready".into())
            });
        }
        let id = prepared.ok_or("Display mode must be prepared before commit")?;
        let request = STATE.with(|state| {
            state
                .borrow_mut()
                .as_mut()
                .and_then(|s| take_pending(&mut s.pending, id))
        });
        // Duplicate or cancelled commits never toggle a second time.
        let Some(request) = request else {
            return display_mode(window, false, None);
        };
        let hwnd = window.hwnd().map_err(|e| e.to_string())?.0;
        if unsafe {
            IsIconic(hwnd) != 0
                || IsWindowVisible(hwnd) == 0
                || rectangle(hwnd) != Some(request.from)
        } {
            return display_mode(window, false, None);
        }
        let db = STATE.with(|state| {
            state
                .borrow()
                .as_ref()
                .map(|s| s.db.clone())
                .ok_or("Timer geometry is not ready")
        })?;
        STATE.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.switching = true;
            }
        });
        let result = (|| {
            configure_mode(window, request.next.horizontal)?;
            let target = target_geometry(window, request.next, request.from, request.monitor)?;
            unsafe {
                place(hwnd, target)?;
            }
            let actual = unsafe { rectangle(hwnd) }.ok_or("Cannot read final timer geometry")?;
            let mut next = request.next;
            next.record(actual);
            save_layout(&db, next)?;
            Ok((next, actual))
        })();
        let (layout, geometry, answer) = match result {
            Ok((layout, geometry)) => (layout, geometry, Ok(layout.horizontal)),
            Err(error) => {
                log::error!("[window-state] instant switch failed: {error}");
                if let Err(rollback) = apply_layout(window, request.previous) {
                    log::error!("[window-state] rollback failed: {rollback}");
                }
                (request.previous, request.previous.geometry(), Err(error))
            }
        };
        STATE.with(|state| {
            if let Some(state) = state.borrow_mut().as_mut() {
                state.layout = layout;
                state.current = geometry;
                state.switching = false;
                if answer.is_ok() {
                    state.saved = Some(layout);
                }
            }
        });
        unsafe {
            RedrawWindow(
                hwnd,
                std::ptr::null(),
                std::ptr::null_mut(),
                RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_FRAME,
            );
        }
        log::info!(
            "[window-state] instant switch horizontal={} {geometry:?}",
            layout.horizontal
        );
        answer
    }

    pub fn install(
        window: &tauri::WebviewWindow,
        db: DbState,
        settings: &settings::Settings,
    ) -> Result<(), String> {
        let hwnd = window.hwnd().map_err(|e| e.to_string())?.0;
        let initial = unsafe { rectangle(hwnd) }.ok_or("Cannot read initial timer geometry")?;
        let stored = {
            let conn = db.lock().map_err(|e| e.to_string())?;
            let json: Option<String> = conn
                .query_row("SELECT value FROM settings WHERE key=?1", [KEY], |row| {
                    row.get(0)
                })
                .optional()
                .map_err(|e| e.to_string())?;
            json.and_then(|value| serde_json::from_str::<Geometry>(&value).ok())
                .filter(|g| g.valid())
        };
        // Legacy sizes are client pixels. Convert once using the actual hidden
        // frame, then all new state uses a consistent outer-window rectangle.
        let legacy = match (
            settings.window_x,
            settings.window_y,
            settings.window_width,
            settings.window_height,
        ) {
            (Some(x), Some(y), Some(width), Some(height)) if width > 0 && height > 0 => {
                let mut client = RECT::default();
                unsafe {
                    GetClientRect(hwnd, &mut client);
                }
                Some(Geometry {
                    x,
                    y,
                    width: width.saturating_add(initial.width.saturating_sub(client.right as u32)),
                    height: height
                        .saturating_add(initial.height.saturating_sub(client.bottom as u32)),
                    scale: initial.scale,
                })
            }
            _ => None,
        };
        let mut layout = {
            let conn = db.lock().map_err(|e| e.to_string())?;
            let json: Option<String> = conn
                .query_row(
                    "SELECT value FROM settings WHERE key=?1",
                    [LAYOUT_KEY],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            json.and_then(|value| serde_json::from_str::<Layout>(&value).ok())
                .filter(|l| l.normal.valid() && l.mini.is_none_or(|g| g.valid()))
                .unwrap_or(Layout {
                    horizontal: false,
                    normal: stored.or(legacy).unwrap_or(initial),
                    mini: None,
                    corner: None,
                })
        };
        // Older mode-specific positions migrate from the last active rectangle.
        layout.record(layout.geometry());
        let actual = apply_layout(window, layout)?;
        layout.record(actual);
        STATE.with(|state| {
            *state.borrow_mut() = Some(State {
                db,
                current: actual,
                layout,
                saved: None,
                dragging: false,
                switching: false,
                destroying: false,
                pending: None,
                next_id: 1,
            })
        });
        unsafe {
            if SetWindowSubclass(hwnd, Some(window_proc), SUBCLASS, 0) == 0 {
                STATE.with(|state| state.borrow_mut().take());
                return Err(format!(
                    "Cannot install timer geometry handler: {}",
                    GetLastError()
                ));
            }
        }
        flush();
        log::info!(
            "[window-state] restored horizontal={} {actual:?}",
            layout.horizontal
        );
        Ok(())
    }
    #[cfg(test)]
    #[test]
    fn duplicate_stale_and_cancelled_commits_do_not_toggle_again() {
        let geometry = Geometry {
            x: 100,
            y: 100,
            width: 240,
            height: 240,
            scale: 1.0,
        };
        let previous = Layout {
            horizontal: false,
            normal: geometry,
            mini: None,
            corner: Some(anchor(geometry)),
        };
        let make_request = |id| PendingSwitch {
            plan: DisplayPlan {
                id,
                horizontal: true,
                width: MINI_WIDTH,
                height: MINI_HEIGHT,
            },
            previous,
            next: previous.toggled(),
            from: geometry,
            monitor: Monitor {
                x: 0,
                y: 0,
                width: 1920,
                height: 1040,
                scale: 1.0,
            },
        };
        let mut pending = Some(make_request(1));
        assert!(take_pending(&mut pending, 2).is_none());
        let committed = take_pending(&mut pending, 1).unwrap();
        assert!(committed.next.horizontal);
        for _ in 0..100 {
            assert!(take_pending(&mut pending, 1).is_none());
        }
        pending = Some(make_request(2));
        assert!(take_pending(&mut pending, 1).is_none());
        assert_eq!(pending.as_ref().unwrap().plan.id, 2);
        pending = None; // Hide/minimize/close cancels a prepared request.
        assert!(take_pending(&mut pending, 2).is_none());
    }

    #[cfg(test)]
    #[test]
    fn only_normal_visible_window_can_replace_saved_geometry() {
        assert!(capture_allowed(true, false, false, true));
        assert!(!capture_allowed(true, true, false, true));
        assert!(!capture_allowed(true, false, true, true));
        assert!(!capture_allowed(false, false, false, true));
        assert!(!capture_allowed(true, false, false, false));
    }
    use rusqlite::OptionalExtension;
}

#[cfg(target_os = "windows")]
pub use windows::{display_mode, install, prepare_display_mode};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_corner_tracks_moves_and_remembers_only_normal_size() {
        let normal = Geometry {
            width: 900,
            height: 700,
            ..geometry()
        };
        let mini = anchored(
            Geometry {
                width: 216,
                height: 97,
                ..normal
            },
            anchor(normal),
        );
        let mut layout = Layout {
            horizontal: false,
            normal,
            mini: None,
            corner: None,
        };
        layout.record(normal);
        layout = layout.toggled();
        layout.record(mini);
        assert_eq!(anchor(layout.geometry()), anchor(normal));
        let moved = Geometry {
            x: 500,
            y: 300,
            ..mini
        };
        layout.record(moved);
        let expanded = layout.toggled().geometry();
        assert_eq!(anchor(expanded), anchor(moved));
        assert_eq!((expanded.width, expanded.height), (900, 700));
        let fitted = normalize(expanded, &[monitor()]).unwrap();
        assert!(fitted.x >= 0 && fitted.y >= 0);
        assert!(anchor(fitted).right <= 1920 && anchor(fitted).bottom <= 1040);
    }

    #[test]
    fn legacy_positions_migrate_from_last_active_mode() {
        for horizontal in [false, true] {
            let json = format!(
                r#"{{"horizontal":{horizontal},"normal":{{"x":50,"y":60,"width":900,"height":700,"scale":1.0}},"mini":{{"x":1200,"y":700,"width":216,"height":97,"scale":1.0}}}}"#
            );
            let mut layout: Layout = serde_json::from_str(&json).unwrap();
            assert_eq!(layout.corner, None);
            let active = layout.geometry();
            layout.record(active);
            assert_eq!(layout.corner, Some(anchor(active)));
            assert_eq!(anchor(layout.toggled().geometry()), anchor(active));
        }
    }

    #[test]
    fn mode_sizes_and_shared_corner_survive_database_reopen() {
        let normal = Geometry {
            x: 90,
            y: 100,
            width: 900,
            height: 700,
            scale: 1.0,
        };
        let mini = Geometry {
            x: 600,
            y: 300,
            width: 200,
            height: 88,
            scale: 1.0,
        };
        let mut layout = Layout {
            horizontal: false,
            normal,
            mini: None,
            corner: None,
        }
        .toggled();
        layout.record(mini);
        assert_eq!(layout.normal, normal);
        let path =
            std::env::temp_dir().join(format!("pomotroid-window-modes-{}.db", std::process::id()));
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            crate::db::migrations::run(&conn).unwrap();
            save_layout(&std::sync::Arc::new(std::sync::Mutex::new(conn)), layout).unwrap();
        }
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            let json: String = conn
                .query_row(
                    "SELECT value FROM settings WHERE key=?1",
                    [LAYOUT_KEY],
                    |r| r.get(0),
                )
                .unwrap();
            let restored: Layout = serde_json::from_str(&json).unwrap();
            assert!(restored.horizontal);
            assert_eq!(restored.geometry(), mini);
            assert_eq!(
                restored.toggled().geometry(),
                anchored(normal, anchor(mini))
            );
            assert_eq!(restored.toggled().toggled().geometry(), mini);
            let bounded = normalize_size(mini, &[monitor()], MINI_WIDTH, MINI_HEIGHT).unwrap();
            assert_eq!((bounded.width, bounded.height), (200, 88));
        }
        std::fs::remove_file(path).unwrap();
    }

    fn monitor() -> Monitor {
        Monitor {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
            scale: 1.0,
        }
    }
    fn geometry() -> Geometry {
        Geometry {
            x: 100,
            y: 200,
            width: 240,
            height: 240,
            scale: 1.0,
        }
    }

    #[test]
    fn small_and_large_geometry_survive_database_reopen() {
        // File-backed SQLite tests the same storage boundary as a full restart.
        let path =
            std::env::temp_dir().join(format!("pomotroid-window-state-{}.db", std::process::id()));
        for geometry in [
            geometry(),
            Geometry {
                x: 900,
                y: 20,
                width: 900,
                height: 950,
                scale: 1.0,
            },
        ] {
            {
                let conn = rusqlite::Connection::open(&path).unwrap();
                crate::db::migrations::run(&conn).unwrap();
                let db = std::sync::Arc::new(std::sync::Mutex::new(conn));
                save(&db, geometry).unwrap();
            }
            let conn = rusqlite::Connection::open(&path).unwrap();
            let value: String = conn
                .query_row("SELECT value FROM settings WHERE key=?1", [KEY], |r| {
                    r.get(0)
                })
                .unwrap();
            let restored = normalize(serde_json::from_str(&value).unwrap(), &[monitor()]).unwrap();
            assert_eq!(restored, geometry);
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn disconnected_monitor_and_taskbar_are_clamped() {
        let saved = Geometry {
            x: -2500,
            y: 1200,
            width: 5000,
            height: 3000,
            ..geometry()
        };
        assert_eq!(
            normalize(saved, &[monitor()]),
            Some(Geometry {
                x: 0,
                y: 0,
                width: 1920,
                height: 1040,
                scale: 1.0
            })
        );
        let left = Monitor {
            x: -1920,
            ..monitor()
        };
        let saved = Geometry {
            x: -1000,
            y: 100,
            width: 360,
            height: 478,
            ..geometry()
        };
        assert_eq!(normalize(saved, &[monitor(), left]), Some(saved));
    }

    #[test]
    fn dpi_change_scales_size_and_keeps_controls_on_screen() {
        let high_dpi = Monitor {
            scale: 2.0,
            ..monitor()
        };
        let restored = normalize(geometry(), &[high_dpi]).unwrap();
        assert_eq!(
            (restored.width, restored.height, restored.scale),
            (480, 480, 2.0)
        );
        assert_eq!(normalize(restored, &[monitor()]), Some(geometry()));
        let tiny = Monitor {
            width: 200,
            height: 150,
            ..high_dpi
        };
        let restored = normalize(geometry(), &[tiny]).unwrap();
        assert_eq!(
            (restored.x, restored.y, restored.width, restored.height),
            (0, 0, 200, 150)
        );
        assert!(normalize(
            Geometry {
                width: 0,
                ..geometry()
            },
            &[monitor()]
        )
        .is_none());
        assert!(normalize(
            Geometry {
                scale: f64::NAN,
                ..geometry()
            },
            &[monitor()]
        )
        .is_none());
    }
}
