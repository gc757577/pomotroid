//! Persistent Tauri owner + replaceable native desktop surface and child WebView.
//! The Tauri Window is never parented to Explorer; recovery reuses fixed labels.
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    ptr::{null, null_mut},
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, Webview, Window};
use windows_sys::Win32::{
    Foundation::{GetLastError, SetLastError, HWND, POINT, RECT},
    Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromRect, ScreenToClient, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    },
    UI::{
        HiDpi::{AreDpiAwarenessContextsEqual, GetWindowDpiAwarenessContext},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};
const OWNER: &str = "stats-widget-owner";
pub const VIEW: &str = "stats-widget";
const ID: usize = 0x53545747;
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
struct Widget {
    app: tauri::AppHandle,
    main: isize,
    owner: Option<Window>,
    view: Option<Webview>,
    surface: isize,
    enabled: bool,
    creating: bool,
    ready: bool,
    bound: bool,
    attached: bool,
    generation: u64,
    geometry: Geometry,
    last_tick: Instant,
}
thread_local! { static WIDGET:RefCell<Option<Widget>> = const { RefCell::new(None) }; }
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn with_widget<T>(f: impl FnOnce(&mut Widget) -> Result<T, String>) -> Result<T, String> {
    let mut w = WIDGET
        .with(|s| s.borrow_mut().take())
        .ok_or("위젯을 준비 중입니다.")?;
    let result = f(&mut w);
    WIDGET.with(|s| *s.borrow_mut() = Some(w));
    result
}
unsafe fn set_style(hwnd: HWND, index: i32, value: isize) -> Result<(), String> {
    SetLastError(0);
    if SetWindowLongPtrW(hwnd, index, value) == 0 && GetLastError() != 0 {
        return Err(format!("창 스타일: {}", GetLastError()));
    }
    Ok(())
}
unsafe fn host() -> HWND {
    FindWindowW(wide("Progman").as_ptr(), null())
}
pub fn normalize(mut g: Geometry, bounds: RECT) -> Geometry {
    let max_w = (bounds.right - bounds.left).max(1) as u32;
    let max_h = (bounds.bottom - bounds.top).max(1) as u32;
    g.width = g.width.clamp(360, 2000).min(max_w);
    g.height = g.height.clamp(240, 1400).min(max_h);
    g.x = g.x.clamp(bounds.left, bounds.right - g.width as i32);
    g.y = g.y.clamp(bounds.top, bounds.bottom - g.height as i32);
    g
}
unsafe fn fit(g: Geometry) -> Geometry {
    let rect = RECT {
        left: g.x,
        top: g.y,
        right: g.x.saturating_add(g.width as i32),
        bottom: g.y.saturating_add(g.height as i32),
    };
    let monitor = MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST);
    let mut info: MONITORINFO = std::mem::zeroed();
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if GetMonitorInfoW(monitor, &mut info) != 0 {
        normalize(g, info.rcWork)
    } else {
        g
    }
}
impl Widget {
    fn counts(&self, reason: &str) {
        let owners = self
            .app
            .windows()
            .keys()
            .filter(|s| s.as_str() == OWNER)
            .count();
        let views = self
            .app
            .webviews()
            .keys()
            .filter(|s| s.as_str() == VIEW)
            .count();
        log::info!(
            "[widget] {reason} generation={} owners={} views={} total_windows={} total_views={}",
            self.generation,
            owners,
            views,
            self.app.windows().len(),
            self.app.webviews().len()
        );
    }
    unsafe fn clear(&mut self) -> Result<(), String> {
        self.ready = false;
        self.bound = false;
        self.attached = false;
        // close() removes the WebView from both runtime and Tauri registries.
        // The persistent owner Window is still alive, so it never becomes stale.
        if let Some(view) = self.view.take().or_else(|| self.app.get_webview(VIEW)) {
            view.close().map_err(|e| e.to_string())?;
        }
        let surface = self.surface as HWND;
        self.surface = 0;
        if IsWindow(surface) != 0 {
            DestroyWindow(surface);
        }
        self.counts("closed child");
        Ok(())
    }
    unsafe fn position(&mut self) -> Result<(), String> {
        let surface = self.surface as HWND;
        if IsWindow(surface) == 0 {
            return Err("위젯 창이 없습니다.".into());
        }
        let g = fit(self.geometry);
        self.geometry = g;
        let mut pt = POINT { x: g.x, y: g.y };
        if self.attached {
            let parent = GetParent(surface);
            if parent.is_null() || ScreenToClient(parent, &mut pt) == 0 {
                return Err("바탕화면 좌표 변환 실패".into());
            }
        }
        if SetWindowPos(
            surface,
            if self.attached {
                HWND_TOP
            } else {
                HWND_NOTOPMOST
            },
            pt.x,
            pt.y,
            g.width as i32,
            g.height as i32,
            SWP_NOACTIVATE | SWP_FRAMECHANGED,
        ) == 0
        {
            return Err(format!("위젯 위치 변경: {}", GetLastError()));
        }
        self.resize_view();
        Ok(())
    }
    fn resize_view(&self) {
        if let Some(view) = &self.view {
            let raw = self.surface;
            let _ = view.with_webview(move |platform| unsafe {
                let controller = platform.controller();
                let mut rect = Default::default();
                if controller.Bounds(&mut rect).is_ok() {
                    let mut client = RECT::default();
                    GetClientRect(raw as HWND, &mut client);
                    rect.left = 0;
                    rect.top = 0;
                    rect.right = client.right;
                    rect.bottom = client.bottom;
                    let _ = controller.SetBounds(rect);
                    let _ = controller.NotifyParentWindowPositionChanged();
                }
            });
        }
    }
    unsafe fn show_ready(&mut self) -> Result<(), String> {
        if !self.enabled || !self.ready || !self.bound {
            return Ok(());
        }
        let surface = self.surface as HWND;
        let desktop = host();
        let result = (|| -> Result<(), String> {
            if desktop.is_null()
                || FindWindowExW(
                    desktop,
                    null_mut(),
                    wide("SHELLDLL_DefView").as_ptr(),
                    null(),
                )
                .is_null()
            {
                return Err("Windows 바탕화면이 준비되지 않았습니다.".into());
            }
            if AreDpiAwarenessContextsEqual(
                GetWindowDpiAwarenessContext(surface),
                GetWindowDpiAwarenessContext(desktop),
            ) == 0
            {
                return Err("바탕화면과 위젯의 DPI 모드가 다릅니다.".into());
            }
            set_style(surface, GWL_STYLE, (WS_CHILD | WS_CLIPCHILDREN) as isize)?;
            SetLastError(0);
            if SetParent(surface, desktop).is_null() && GetLastError() != 0 {
                return Err(format!("바탕화면 연결: {}", GetLastError()));
            }
            if GetParent(surface) != desktop {
                return Err("바탕화면 연결 확인 실패".into());
            }
            self.attached = true;
            self.position()?;
            Ok(())
        })();
        if let Err(error) = result {
            self.attached = false;
            SetParent(surface, null_mut());
            set_style(
                surface,
                GWL_STYLE,
                (WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN) as isize,
            )?;
            self.position()?;
            log::error!("[widget] attachment failed, ordinary-window fallback: {error}");
            let _ = self.app.emit(
                "widget:error",
                "바탕화면에 위젯을 연결하지 못했습니다. 일반 창으로 복구했습니다.",
            );
        }
        ShowWindow(surface, SW_SHOWNOACTIVATE);
        self.resize_view();
        self.counts(if self.attached {
            "attached"
        } else {
            "fallback"
        });
        Ok(())
    }
    unsafe fn create(&mut self) -> Result<(), String> {
        if self.creating || !self.enabled {
            return Ok(());
        }
        self.clear()?;
        if self.owner.is_none() {
            self.owner = Some(
                tauri::window::WindowBuilder::new(&self.app, OWNER)
                    .title("Pomotroid widget owner")
                    .inner_size(760.0, 320.0)
                    .visible(false)
                    .skip_taskbar(true)
                    .build()
                    .map_err(|e| e.to_string())?,
            );
        }
        self.geometry = fit(self.geometry);
        let g = self.geometry;
        let surface = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            wide("STATIC").as_ptr(),
            wide("Pomotroid Statistics").as_ptr(),
            WS_POPUP | WS_CLIPCHILDREN,
            g.x,
            g.y,
            g.width as i32,
            g.height as i32,
            null_mut(),
            null_mut(),
            null_mut(),
            null(),
        );
        if surface.is_null() {
            return Err(format!("위젯 표면 생성: {}", GetLastError()));
        }
        self.surface = surface as isize;
        if SetLayeredWindowAttributes(surface, 0, 255, LWA_ALPHA) == 0 {
            return Err(format!("위젯 표면: {}", GetLastError()));
        }
        SetWindowSubclass(surface, Some(surface_proc), ID, 0);
        self.creating = true;
        self.generation += 1;
        let generation = self.generation;
        let native = self.surface;
        let owner = self.owner.as_ref().unwrap().clone();
        let app = self.app.clone();
        std::thread::spawn(move || {
            let result = owner.add_child(
                tauri::webview::WebviewBuilder::new(
                    VIEW,
                    tauri::WebviewUrl::App(format!("desktop-probe?generation={generation}").into()),
                ),
                tauri::PhysicalPosition::new(0, 0),
                tauri::PhysicalSize::new(g.width, g.height),
            );
            match result {
                Err(error) => {
                    let _ = app.run_on_main_thread(move || {
                        let _ = with_widget(|w| {
                            w.creating = false;
                            log::error!("[widget] create WebView: {error}");
                            Ok(())
                        });
                    });
                }
                Ok(view) => {
                    let app2 = app.clone();
                    let view2 = view.clone();
                    let dispatch = view.with_webview(move |platform| {
                        let result = unsafe {
                            let controller = platform.controller();
                            let mut parent = Default::default();
                            controller
                                .ParentWindow(&mut parent)
                                .and_then(|_| {
                                    parent.0 = native as HWND;
                                    controller.SetParentWindow(parent)
                                })
                                .and_then(|_| controller.SetIsVisible(true))
                        };
                        let _ = app2.run_on_main_thread(move || {
                            let _ = with_widget(|w| unsafe {
                                w.creating = false;
                                if !w.enabled
                                    || w.generation != generation
                                    || w.surface != native
                                    || result.is_err()
                                {
                                    let _ = view2.close();
                                    log::warn!("[widget] cancelled/reparent failed: {result:?}");
                                    if w.enabled {
                                        w.create()?;
                                    }
                                    return Ok(());
                                }
                                w.view = Some(view2);
                                w.bound = true;
                                w.show_ready()?;
                                Ok(())
                            });
                        });
                    });
                    if let Err(error) = dispatch {
                        log::error!("[widget] controller dispatch: {error}");
                    }
                }
            }
        });
        Ok(())
    }
    unsafe fn tick(&mut self) {
        if !self.enabled {
            return;
        }
        if self.last_tick.elapsed() > Duration::from_secs(5) {
            let _ = self.app.emit("widget:refresh", ());
        }
        self.last_tick = Instant::now();
        if !self.creating && (self.surface == 0 || IsWindow(self.surface as HWND) == 0) {
            log::warn!("[widget] desktop surface lost; recreating child with fixed labels");
            if let Err(error) = self.create() {
                log::error!("[widget] recovery: {error}");
            }
        } else if self.ready
            && self.bound
            && self.attached
            && GetParent(self.surface as HWND) != host()
        {
            if let Err(error) = self.show_ready() {
                log::error!("[widget] reattach: {error}");
            }
        }
    }
}
unsafe extern "system" fn surface_proc(
    hwnd: HWND,
    msg: u32,
    w: usize,
    l: isize,
    id: usize,
    _: usize,
) -> isize {
    if msg == WM_NCDESTROY {
        RemoveWindowSubclass(hwnd, Some(surface_proc), id);
    }
    if msg == WM_SIZE || msg == WM_MOVE {
        let _ = with_widget(|widget| {
            widget.resize_view();
            Ok(())
        });
    }
    DefSubclassProc(hwnd, msg, w, l)
}
unsafe extern "system" fn supervisor(
    hwnd: HWND,
    msg: u32,
    w: usize,
    l: isize,
    id: usize,
    _: usize,
) -> isize {
    if msg == WM_TIMER && w == ID {
        let _ = with_widget(|widget| {
            widget.tick();
            Ok(())
        });
    } else if msg == WM_POWERBROADCAST && (w == 0x12 || w == 0x7) {
        let _ = with_widget(|widget| {
            let _ = widget.app.emit("widget:refresh", ());
            Ok(())
        });
    } else if msg == WM_DISPLAYCHANGE {
        let _ = with_widget(|widget| {
            if widget.enabled {
                widget.position()?;
            }
            Ok(())
        });
    } else if msg == WM_NCDESTROY {
        KillTimer(hwnd, ID);
        let _ = with_widget(|widget| {
            widget.enabled = false;
            widget.clear()?;
            if let Some(owner) = widget.owner.take() {
                let _ = owner.destroy();
            }
            Ok(())
        });
        RemoveWindowSubclass(hwnd, Some(supervisor), id);
    }
    DefSubclassProc(hwnd, msg, w, l)
}
pub fn start(
    main: &tauri::WebviewWindow,
    settings: &crate::settings::Settings,
) -> Result<(), String> {
    unsafe {
        let hwnd = main.hwnd().map_err(|e| e.to_string())?.0;
        if SetWindowSubclass(hwnd, Some(supervisor), ID, 0) == 0 {
            return Err("위젯 감시 설치 실패".into());
        }
        WIDGET.with(|s| {
            *s.borrow_mut() = Some(Widget {
                app: main.app_handle().clone(),
                main: hwnd as isize,
                owner: None,
                view: None,
                surface: 0,
                enabled: false,
                creating: false,
                ready: false,
                bound: false,
                attached: false,
                generation: 0,
                last_tick: Instant::now(),
                geometry: Geometry {
                    x: 80,
                    y: 80,
                    width: 760,
                    height: 320,
                },
            })
        });
        configure(settings)
    }
}
pub fn configure(s: &crate::settings::Settings) -> Result<(), String> {
    with_widget(|w| unsafe {
        w.geometry = Geometry {
            x: s.widget_x.unwrap_or(80),
            y: s.widget_y.unwrap_or(80),
            width: s.widget_width,
            height: s.widget_height,
        };
        w.enabled = s.widget_enabled;
        if w.enabled {
            SetTimer(w.main as HWND, ID, 1000, None);
            if w.surface == 0 && !w.creating {
                w.create()?;
            }
        } else {
            KillTimer(w.main as HWND, ID);
            w.clear()?;
        }
        Ok(())
    })
}
pub fn action(
    caller: &Webview,
    generation: u64,
    action: &str,
    geometry: Option<Geometry>,
    save: bool,
) -> Result<serde_json::Value, String> {
    with_widget(|w| unsafe {
        if caller.label() != VIEW || generation != w.generation || !w.enabled {
            return Err("이전 위젯 요청은 처리하지 않습니다.".into());
        }
        match action {
            "ready" => {
                w.ready = true;
                w.show_ready()?;
            }
            "geometry" => {
                if let Some(g) = geometry {
                    w.geometry = fit(g);
                    w.position()?;
                    if save {
                        let db = w.app.state::<crate::db::DbState>();
                        let mut conn = db.lock().map_err(|e| e.to_string())?;
                        let tx = conn.transaction().map_err(|e| e.to_string())?;
                        for (key, value) in [
                            ("widget_x", w.geometry.x.to_string()),
                            ("widget_y", w.geometry.y.to_string()),
                            ("widget_width", w.geometry.width.to_string()),
                            ("widget_height", w.geometry.height.to_string()),
                        ] {
                            crate::settings::save_setting(&tx, key, &value)
                                .map_err(|e| e.to_string())?;
                        }
                        tx.commit().map_err(|e| e.to_string())?;
                    }
                }
            }
            _ => return Err("위젯 요청이 올바르지 않습니다.".into()),
        }
        Ok(serde_json::to_value(w.geometry).unwrap())
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn geometry_clamps_negative_monitor_and_disconnect() {
        let bounds = RECT {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1080,
        };
        let g = normalize(
            Geometry {
                x: -3000,
                y: 2000,
                width: 760,
                height: 320,
            },
            bounds,
        );
        assert_eq!(
            g,
            Geometry {
                x: -1920,
                y: 760,
                width: 760,
                height: 320
            }
        );
        let g = normalize(
            g,
            RECT {
                left: 0,
                top: 0,
                right: 1280,
                bottom: 720,
            },
        );
        assert_eq!(g.x, 0);
        assert_eq!(g.y, 400);
    }
}
