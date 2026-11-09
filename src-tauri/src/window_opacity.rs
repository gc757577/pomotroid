//! Native whole-window opacity. Timer hover policy is separate from the shared HWND applicator.

#[cfg(any(target_os = "windows", test))]
#[derive(Clone)]
struct OpacityState {
    normal: u32,
    hover_enabled: bool,
    hover: u32,
    applied: Option<u32>,
    target: Option<u32>,
    preview: Option<u32>,
    animation: Option<OpacityTransition>,
    motion_enabled: bool,
    hover_timer: bool,
    frame_timer: bool,
    applying: bool,
}

#[cfg(any(target_os = "windows", test))]
#[derive(Clone)]
struct OpacityTransition {
    from: u32,
    to: u32,
    started: std::time::Instant,
}

#[cfg(any(target_os = "windows", test))]
const TRANSITION_DURATION: std::time::Duration = std::time::Duration::from_millis(180);

#[cfg(any(target_os = "windows", test))]
impl OpacityState {
    fn from_settings(settings: &crate::settings::Settings) -> Self {
        Self {
            normal: settings.timer_opacity_percent,
            hover_enabled: settings.hover_opacity_enabled,
            hover: settings.hover_opacity_percent,
            applied: None,
            target: None,
            preview: None,
            animation: None,
            motion_enabled: false,
            hover_timer: false,
            frame_timer: false,
            applying: false,
        }
    }

    fn update_preferences(&mut self, settings: &crate::settings::Settings) {
        self.normal = settings.timer_opacity_percent;
        self.hover_enabled = settings.hover_opacity_enabled;
        self.hover = settings.hover_opacity_percent;
        self.preview = None;
    }

    fn frame(&mut self, over_timer: bool, animate: bool, now: std::time::Instant) -> Option<u32> {
        let target = self.preview.unwrap_or(if self.hover_enabled && over_timer { self.hover } else { self.normal });
        let animate = animate && self.hover_enabled && self.preview.is_none() && self.normal != self.hover;
        if self.target != Some(target) {
            // Start from the last value actually presented, never an old target
            // or a time sample that has not been written to the native window.
            self.animation = self.applied.filter(|from| animate && *from != target)
                .map(|from| OpacityTransition { from, to: target, started: now });
            self.target = Some(target);
        }
        if !animate { self.animation = None; }
        let percent = if let Some(animation) = &self.animation {
            let t = (now.saturating_duration_since(animation.started).as_secs_f64()
                / TRANSITION_DURATION.as_secs_f64()).clamp(0.0, 1.0);
            let eased = 1.0 - (1.0 - t).powi(3);
            let value = (animation.from as f64 + (animation.to as f64 - animation.from as f64) * eased).round() as u32;
            if t >= 1.0 { self.animation = None; }
            value
        } else { target };
        if self.applied == Some(percent) { return None; }
        self.applied = Some(percent);
        Some(percent)
    }

    #[cfg(test)]
    fn transition(&mut self, over_timer: bool) -> Option<u32> {
        self.frame(over_timer, false, std::time::Instant::now())
    }
}

#[cfg(target_os = "windows")]
type NativeState = std::sync::Arc<std::sync::Mutex<OpacityState>>;

#[cfg(target_os = "windows")]
thread_local! {
    // The window subclass, timer, and synchronous commands share the UI thread.
    // Keep ownership in Rust instead of exposing a raw state pointer to Windows.
    static OPACITY_STATES: std::cell::RefCell<std::collections::HashMap<isize, NativeState>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

#[cfg(target_os = "windows")]
const OPACITY_SUBCLASS: usize = 0x504f4d4f;
#[cfg(target_os = "windows")]
const HOVER_TIMER: usize = 0x504f4d48;
#[cfg(target_os = "windows")]
const FRAME_TIMER: usize = 0x504f4d41;

#[cfg(target_os = "windows")]
unsafe fn motion_enabled() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION};
    let mut enabled: i32 = 0;
    SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, (&mut enabled as *mut i32).cast(), 0) != 0 && enabled != 0
}

/// Install on the owning UI thread before showing the timer.
pub fn install(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        return Err("불투명도는 타이머 창에만 적용할 수 있습니다.".into());
    }
    #[cfg(target_os = "windows")]
    {
        let hwnd = window.hwnd().map_err(|e| e.to_string())?.0;
        // SAFETY: setup runs on the thread owning this live Tauri window.
        unsafe { install_native(hwnd)?; }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
unsafe fn install_native(hwnd: windows_sys::Win32::Foundation::HWND) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::SetWindowSubclass;
    if native_state(hwnd).is_ok() { return Ok(()); }
    let state = std::sync::Arc::new(std::sync::Mutex::new(
        OpacityState::from_settings(&crate::settings::Settings::default())
    ));
    let enabled = motion_enabled();
    state.lock().map_err(|e| e.to_string())?.motion_enabled = enabled;
    OPACITY_STATES.with(|states| states.borrow_mut().insert(hwnd as isize, state));
    if SetWindowSubclass(hwnd, Some(preserve_layered_style), OPACITY_SUBCLASS, 0) == 0 {
        OPACITY_STATES.with(|states| states.borrow_mut().remove(&(hwnd as isize)));
        return Err("창 불투명도 유지 처리를 설치할 수 없습니다.".into());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn preserve_layered_style(
    hwnd: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: usize,
    lparam: isize,
    subclass: usize,
    _data: usize,
) -> isize {
    use windows_sys::Win32::UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass},
        WindowsAndMessaging::{GWL_EXSTYLE, STYLESTRUCT, WM_NCDESTROY, WM_STYLECHANGING, WS_EX_LAYERED,
            KillTimer, WM_TIMER, WM_SIZE, WM_SHOWWINDOW, WM_WINDOWPOSCHANGED, WM_EXITSIZEMOVE,
            WM_ACTIVATE, WM_SETFOCUS, WM_SETTINGCHANGE},
    };
    // Hold a local reference across DefSubclassProc, which can reenter and
    // destroy the window. No state lock is held across any Windows calls.
    let state = native_state(hwnd).ok();
    if message == WM_NCDESTROY {
        KillTimer(hwnd, HOVER_TIMER);
        KillTimer(hwnd, FRAME_TIMER);
        RemoveWindowSubclass(hwnd, Some(preserve_layered_style), subclass);
        let _ = OPACITY_STATES.try_with(|states| states.borrow_mut().remove(&(hwnd as isize)));
        return DefSubclassProc(hwnd, message, wparam, lparam);
    }
    let result = DefSubclassProc(hwnd, message, wparam, lparam);
    if message == WM_STYLECHANGING && wparam as i32 == GWL_EXSTYLE && lparam != 0 {
        // SAFETY: Windows supplies a writable STYLESTRUCT for this synchronous
        // message. Tao rebuilds styles on show/topmost/restore; retain only our
        // layered bit and leave its other style decisions intact.
        (*(lparam as *mut STYLESTRUCT)).styleNew |= WS_EX_LAYERED;
    }
    if message == WM_SETTINGCHANGE {
        let enabled = motion_enabled();
        if let Some(state) = &state {
            if let Ok(mut cached) = state.lock() { cached.motion_enabled = enabled; }
        }
    }
    if (message == WM_TIMER && (wparam == HOVER_TIMER || wparam == FRAME_TIMER))
        || matches!(message, WM_SIZE | WM_SHOWWINDOW | WM_WINDOWPOSCHANGED | WM_EXITSIZEMOVE | WM_ACTIVATE | WM_SETFOCUS | WM_SETTINGCHANGE)
    {
        // A reentrant destroy can have detached the subclass while forwarding.
        if let (Some(state), Ok(current)) = (state, native_state(hwnd)) {
            if std::sync::Arc::ptr_eq(&state, &current) {
                if let Err(error) = refresh_native(hwnd, &state) {
                    log::error!("[window-opacity] hover refresh failed: {error}");
                }
            }
        }
    }
    result
}

#[cfg(target_os = "windows")]
fn native_state(hwnd: windows_sys::Win32::Foundation::HWND) -> Result<NativeState, String> {
    OPACITY_STATES.try_with(|states| states.borrow().get(&(hwnd as isize)).cloned())
        .ok().flatten().ok_or_else(|| "창 불투명도 상태를 찾을 수 없습니다.".into())
}

#[cfg(target_os = "windows")]
unsafe fn cursor_over_timer(hwnd: windows_sys::Win32::Foundation::HWND) -> bool {
    use windows_sys::Win32::{Foundation::{POINT, RECT}, UI::WindowsAndMessaging::{
        IsWindowVisible, IsIconic, GetCursorPos, GetWindowRect, WindowFromPoint,
        GetAncestor, GA_ROOT, GA_ROOTOWNER,
    }};
    if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 { return false; }
    let mut point = POINT::default();
    let mut rect = RECT::default();
    if GetCursorPos(&mut point) == 0 || GetWindowRect(hwnd, &mut rect) == 0
        || point.x < rect.left || point.x >= rect.right || point.y < rect.top || point.y >= rect.bottom
    { return false; }
    let hit = WindowFromPoint(point);
    // Includes the webview children and Tauri's owned native resize helper,
    // but excludes unrelated windows covering the timer.
    !hit.is_null() && (GetAncestor(hit, GA_ROOT) == hwnd || GetAncestor(hit, GA_ROOTOWNER) == hwnd)
}

#[cfg(target_os = "windows")]
unsafe fn refresh_native(hwnd: windows_sys::Win32::Foundation::HWND, state: &NativeState) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindowVisible, IsIconic};
    let visible = IsWindowVisible(hwnd) != 0 && IsIconic(hwnd) == 0;
    let over = cursor_over_timer(hwnd);
    let percent = {
        let mut cached = state.lock().map_err(|e| e.to_string())?;
        if cached.applying { return Ok(()); }
        let animate = visible && cached.motion_enabled;
        let percent = cached.frame(over, animate, std::time::Instant::now());
        cached.applying = percent.is_some();
        percent
    };
    if let Some(percent) = percent {
        let result = apply_native(hwnd, percent);
        let mut cached = state.lock().map_err(|e| e.to_string())?;
        cached.applying = false;
        if result.is_err() {
            cached.applied = None;
            cached.target = None;
            cached.animation = None;
        }
        drop(cached);
        sync_timers(hwnd, state, visible)?;
        result?;
    } else {
        sync_timers(hwnd, state, visible)?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
unsafe fn sync_timers(hwnd: windows_sys::Win32::Foundation::HWND, state: &NativeState, visible: bool) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetTimer, KillTimer};
    let changes = {
        let cached = state.lock().map_err(|e| e.to_string())?;
        let hover = visible && cached.hover_enabled && cached.preview.is_none() && cached.normal != cached.hover;
        let frame = visible && cached.animation.is_some();
        [(HOVER_TIMER, 50, cached.hover_timer, hover), (FRAME_TIMER, 16, cached.frame_timer, frame)]
    };
    for (id, interval, was_running, should_run) in changes {
        if was_running == should_run { continue; }
        if should_run {
            if SetTimer(hwnd, id, interval, None) == 0 {
                return Err("불투명도 타이머를 시작할 수 없습니다.".into());
            }
        } else { KillTimer(hwnd, id); }
        let mut cached = state.lock().map_err(|e| e.to_string())?;
        if id == HOVER_TIMER { cached.hover_timer = should_run; } else { cached.frame_timer = should_run; }
    }
    Ok(())
}

/// Update cached preferences and retarget from the displayed opacity.
/// Called on the UI thread; hover transitions never access settings storage.
pub fn configure(window: &tauri::WebviewWindow, settings: &crate::settings::Settings) -> Result<(), String> {
    validate(settings.timer_opacity_percent)?;
    validate(settings.hover_opacity_percent)?;
    if window.label() != "main" { return Err("불투명도는 타이머 창에만 적용할 수 있습니다.".into()); }
    #[cfg(target_os = "windows")]
    unsafe {
        let hwnd = window.hwnd().map_err(|e| e.to_string())?.0;
        let state = native_state(hwnd)?;
        let enabled = motion_enabled();
        {
            let mut cached = state.lock().map_err(|e| e.to_string())?;
            cached.update_preferences(settings);
            cached.motion_enabled = enabled;
        }
        refresh_native(hwnd, &state)?;
    }
    #[cfg(not(target_os = "windows"))]
    apply(window, settings.timer_opacity_percent)?;
    Ok(())
}

pub fn validate(percent: u32) -> Result<u8, String> {
    if !(20..=100).contains(&percent) {
        return Err("불투명도는 20~100% 사이여야 합니다.".into());
    }
    Ok(((percent * 255 + 50) / 100) as u8)
}

#[cfg(target_os = "windows")]
pub fn apply(window: &tauri::WebviewWindow, percent: u32) -> Result<(), String> {
    preview(window, Some(percent))
}

/// Preview is explicit and takes precedence over hover, without altering either
/// stored preference. None releases the preview and resumes the hover policy.
#[cfg(target_os = "windows")]
pub fn preview(window: &tauri::WebviewWindow, percent: Option<u32>) -> Result<(), String> {
    if let Some(percent) = percent { validate(percent)?; }
    if window.label() != "main" { return Err("불투명도는 타이머 창에만 적용할 수 있습니다.".into()); }
    let hwnd = window.hwnd().map_err(|e| e.to_string())?.0;
    unsafe {
        let state = native_state(hwnd)?;
        {
            let mut cached = state.lock().map_err(|e| e.to_string())?;
            cached.preview = percent;
        }
        refresh_native(hwnd, &state)
    }
}

#[cfg(target_os = "windows")]
pub(crate) unsafe fn apply_native(hwnd: windows_sys::Win32::Foundation::HWND, percent: u32) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::{GetLastError, SetLastError},
        UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW,
            GWL_EXSTYLE, LWA_ALPHA, WS_EX_LAYERED,
        },
    };

    let alpha = validate(percent)?;
    // SAFETY: The caller owns this live HWND. Preserve every existing extended
    // style bit; no click-through or topmost flags are changed.
    unsafe {
        SetLastError(0);
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let error = GetLastError();
        if style == 0 && error != 0 {
            return Err(format!("창 스타일 읽기 실패: {error}"));
        }
        let layered = style | WS_EX_LAYERED as isize;
        if layered != style {
            SetLastError(0);
            if SetWindowLongPtrW(hwnd, GWL_EXSTYLE, layered) == 0 && GetLastError() != 0 {
                return Err(format!("창 스타일 변경 실패: {}", GetLastError()));
            }
        }
        if SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA) == 0 {
            let error = GetLastError();
            if layered != style {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style);
            }
            return Err(format!("창 불투명도 적용 실패: {error}"));
        }
        #[cfg(debug_assertions)]
        {
            use windows_sys::Win32::UI::WindowsAndMessaging::GetLayeredWindowAttributes;
            let mut observed = 0;
            if GetLayeredWindowAttributes(hwnd, std::ptr::null_mut(), &mut observed, std::ptr::null_mut()) == 0
                || observed != alpha
            {
                return Err("창 불투명도 적용 결과를 확인할 수 없습니다.".into());
            }
            log::debug!("[window-opacity] hwnd={hwnd:p} percent={percent} alpha={observed}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hover_state(normal: u32, hover: u32) -> OpacityState {
        OpacityState::from_settings(&crate::settings::Settings {
            timer_opacity_percent: normal, hover_opacity_percent: hover,
            hover_opacity_enabled: true, ..Default::default()
        })
    }

    #[test]
    fn animated_hover_moves_both_directions_and_finishes_at_180ms() {
        for (normal, hover) in [(70, 100), (100, 70), (100, 20), (20, 100)] {
            let mut state = hover_state(normal, hover);
            let start = std::time::Instant::now();
            assert_eq!(state.frame(false, true, start), Some(normal));
            assert_eq!(state.frame(true, true, start), None);
            let mut previous = normal;
            for ms in (15..180).step_by(15) {
                state.frame(true, true, start + std::time::Duration::from_millis(ms));
                let current = state.applied.unwrap();
                assert!((normal.min(hover)..=normal.max(hover)).contains(&current));
                assert!(if normal < hover { current >= previous } else { current <= previous });
                previous = current;
                assert!(state.animation.is_some());
            }
            state.frame(true, true, start + TRANSITION_DURATION);
            assert_eq!(state.applied, Some(hover));
            assert!(state.animation.is_none());
            for ms in 181..400 { assert_eq!(state.frame(true, true, start + std::time::Duration::from_millis(ms)), None); }
        }
        let mut state = hover_state(20, 100);
        let start = std::time::Instant::now();
        state.frame(false, true, start); state.frame(true, true, start);
        state.frame(true, true, start + std::time::Duration::from_millis(90));
        assert_eq!(state.applied, Some(90), "cubic ease-out covers 87.5% at half time");
    }

    #[test]
    fn rapid_reversal_starts_at_the_presented_value_and_never_restarts_same_target() {
        let mut state = hover_state(100, 20);
        let start = std::time::Instant::now();
        state.frame(false, true, start);
        let mut over = true;
        for ms in (0..600).step_by(30) {
            let now = start + std::time::Duration::from_millis(ms);
            let presented = state.applied;
            assert_eq!(state.frame(over, true, now), None, "retarget must not jump");
            assert_eq!(state.applied, presented);
            assert_eq!(state.animation.as_ref().unwrap().from, presented.unwrap());
            state.frame(over, true, now + std::time::Duration::from_millis(15));
            let began = state.animation.as_ref().unwrap().started;
            state.frame(over, true, now + std::time::Duration::from_millis(20));
            assert_eq!(state.animation.as_ref().unwrap().started, began, "child/button events must not restart a transition");
            over = !over;
        }
        state.frame(!over, true, start + std::time::Duration::from_millis(800));
        assert_eq!(state.applied, state.target);
        assert!(state.animation.is_none());
    }

    #[test]
    fn disabling_equal_values_hidden_and_reduced_motion_cancel_animation() {
        let start = std::time::Instant::now();
        for reason in ["disabled", "equal", "hidden-or-reduced-motion"] {
            let mut state = hover_state(70, 100);
            state.frame(false, true, start); state.frame(true, true, start);
            state.frame(true, true, start + std::time::Duration::from_millis(45));
            assert!(state.animation.is_some());
            match reason {
                "disabled" => state.hover_enabled = false,
                "equal" => state.hover = 70,
                _ => {}
            }
            let animate = reason != "hidden-or-reduced-motion";
            state.frame(animate, animate, start + std::time::Duration::from_millis(50));
            assert_eq!(state.applied, Some(70));
            assert!(state.animation.is_none());
            assert_eq!(state.frame(false, animate, start + TRANSITION_DURATION), None);
        }
        let mut state = hover_state(70, 100);
        state.frame(false, true, start);
        assert_eq!(state.frame(true, false, start), Some(100), "reduced motion snaps to the hover target");
        assert!(state.animation.is_none());
    }

    #[test]
    fn preview_has_priority_and_preference_edits_retarget_without_resetting_display() {
        let mut state = hover_state(70, 100);
        let start = std::time::Instant::now();
        state.frame(false, true, start); state.frame(true, true, start);
        state.frame(true, true, start + std::time::Duration::from_millis(45));
        state.preview = Some(40);
        state.frame(true, true, start + std::time::Duration::from_millis(50));
        assert_eq!(state.applied, Some(40));
        assert!(state.animation.is_none());
        for over in [false, true, false] {
            assert_eq!(state.frame(over, true, start + std::time::Duration::from_millis(60)), None);
        }
        assert_eq!((state.normal, state.hover), (70, 100));
        state.preview = None;
        state.frame(true, true, start + std::time::Duration::from_millis(70));
        assert_eq!(state.applied, Some(40));
        state.frame(true, true, start + std::time::Duration::from_millis(100));
        let displayed = state.applied;
        let mut settings = crate::settings::Settings {
            timer_opacity_percent: 80, hover_opacity_enabled: true, hover_opacity_percent: 20,
            ..Default::default()
        };
        state.update_preferences(&settings);
        state.frame(true, true, start + std::time::Duration::from_millis(110));
        assert_eq!(state.applied, displayed);
        assert_eq!(state.animation.as_ref().unwrap().from, displayed.unwrap());
        state.frame(true, true, start + std::time::Duration::from_millis(290));
        assert_eq!(state.applied, Some(20));
        settings.hover_opacity_enabled = false;
        state.update_preferences(&settings);
        assert_eq!(state.frame(true, true, start + std::time::Duration::from_millis(300)), Some(80));
        assert!(state.animation.is_none());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn native_frames_keep_geometry_and_stop_frame_timer_when_idle() {
        use windows_sys::Win32::{Foundation::RECT, UI::WindowsAndMessaging::*};
        unsafe {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            let hwnd = CreateWindowExW(0, class.as_ptr(), std::ptr::null(), WS_POPUP,
                0, 0, 200, 88, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null());
            assert!(!hwnd.is_null());
            install_native(hwnd).unwrap();
            let state = native_state(hwnd).unwrap();
            *state.lock().unwrap() = hover_state(100, 20);
            let start = std::time::Instant::now();
            let mut before = RECT::default(); GetWindowRect(hwnd, &mut before);
            // Simulated timestamps, real HWND alpha writes and timer ownership.
            for ms in (0..=180).step_by(15) {
                let percent = {
                    let mut cached = state.lock().unwrap();
                    if ms == 0 { cached.frame(false, true, start); }
                    cached.frame(true, true, start + std::time::Duration::from_millis(ms));
                    cached.applied.unwrap()
                };
                apply_native(hwnd, percent).unwrap();
                sync_timers(hwnd, &state, true).unwrap();
                let cached = state.lock().unwrap();
                assert_eq!(cached.frame_timer, ms < 180);
                assert!(cached.hover_timer);
                let mut alpha = 0;
                assert_ne!(GetLayeredWindowAttributes(hwnd, std::ptr::null_mut(), &mut alpha, std::ptr::null_mut()), 0);
                assert_eq!(alpha, validate(percent).unwrap());
            }
            {
                let mut cached = state.lock().unwrap();
                assert_eq!(cached.frame(true, true, start + std::time::Duration::from_secs(1)), None);
                assert!(cached.animation.is_none());
                cached.hover = cached.normal;
                cached.frame(true, true, start + std::time::Duration::from_secs(1));
            }
            sync_timers(hwnd, &state, true).unwrap();
            assert!(!state.lock().unwrap().hover_timer, "equal values need no hover polling");
            assert!(!state.lock().unwrap().frame_timer);
            let mut after = RECT::default(); GetWindowRect(hwnd, &mut after);
            assert_eq!((before.left, before.top, before.right, before.bottom),
                (after.left, after.top, after.right, after.bottom));
            DestroyWindow(hwnd);
            assert!(native_state(hwnd).is_err());
        }
    }

    // Hidden, app-owned native windows only: this checks HWND attributes, not
    // desktop/WebView visual composition or interaction with the user's UI.
    #[cfg(target_os = "windows")]
    #[test]
    fn native_child_opacity_preserves_parent_geometry_and_other_window_alpha() {
        use windows_sys::Win32::{Foundation::RECT, UI::WindowsAndMessaging::*};
        unsafe {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            let parent = CreateWindowExW(0, class.as_ptr(), std::ptr::null(), WS_POPUP,
                0, 0, 400, 300, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null());
            assert!(!parent.is_null());
            let child = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_LAYERED, class.as_ptr(), std::ptr::null(),
                WS_POPUP | WS_CLIPCHILDREN, 10, 10, 200, 100, std::ptr::null_mut(),
                std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null());
            assert!(!child.is_null());
            // Match the widget: create a layered popup, then attach it as a child.
            SetWindowLongPtrW(child, GWL_STYLE, (WS_CHILD | WS_CLIPCHILDREN) as isize);
            SetParent(child, parent);
            let result = (|| -> Result<(), String> {
                apply_native(parent, 40)?;
                let mut before = RECT::default();
                GetWindowRect(child, &mut before);
                for percent in [20, 70, 100] {
                    apply_native(child, percent)?;
                    let mut alpha = 0;
                    assert_ne!(GetLayeredWindowAttributes(child, std::ptr::null_mut(), &mut alpha, std::ptr::null_mut()), 0);
                    assert_eq!(alpha, validate(percent)?);
                    GetLayeredWindowAttributes(parent, std::ptr::null_mut(), &mut alpha, std::ptr::null_mut());
                    assert_eq!(alpha, validate(40)?);
                    assert_eq!(GetParent(child), parent);
                    let style = GetWindowLongPtrW(child, GWL_EXSTYLE) as u32;
                    assert_eq!(style & (WS_EX_TRANSPARENT | WS_EX_TOPMOST), 0);
                    assert_ne!(GetWindowLongPtrW(child, GWL_STYLE) as u32 & WS_CHILD, 0);
                    let mut after = RECT::default();
                    GetWindowRect(child, &mut after);
                    assert_eq!((before.left, before.top, before.right, before.bottom),
                        (after.left, after.top, after.right, after.bottom));
                }
                Ok(())
            })();
            DestroyWindow(child);
            DestroyWindow(parent);
            result.unwrap();
        }
    }

    #[test]
    fn accepts_limits_and_rounds_alpha() {
        assert_eq!(validate(20), Ok(51));
        assert_eq!(validate(50), Ok(128));
        assert_eq!(validate(100), Ok(255));
    }

    #[test]
    fn rejects_out_of_range_without_wrapping() {
        for percent in [0, 19, 101, u32::MAX] {
            assert!(validate(percent).is_err());
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn tao_style_rewrites_preserve_alpha_and_other_flags() {
        use windows_sys::Win32::{
            Foundation::HWND,
            UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, GetLayeredWindowAttributes, GetWindowLongPtrW,
                SetLayeredWindowAttributes, SetWindowLongPtrW, GWL_EXSTYLE, LWA_ALPHA,
                WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW,
            },
        };
        struct TestWindow(HWND);
        impl Drop for TestWindow {
            fn drop(&mut self) { unsafe { DestroyWindow(self.0); } }
        }
        // A hidden, test-owned HWND on this thread; no user app is touched.
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        unsafe {
            let hwnd = CreateWindowExW(0, class.as_ptr(), std::ptr::null(), WS_OVERLAPPEDWINDOW,
                0, 0, 100, 100, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null());
            assert!(!hwnd.is_null());
            let _owned = TestWindow(hwnd);
            install_native(hwnd).unwrap();
            *native_state(hwnd).unwrap().lock().unwrap() = OpacityState {
                normal: 70, hover_enabled: false, hover: 100, applied: Some(70),
                ..OpacityState::from_settings(&crate::settings::Settings::default())
            };
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, WS_EX_LAYERED as isize);
            assert_ne!(SetLayeredWindowAttributes(hwnd, 0, 179, LWA_ALPHA), 0);
            // Simulate Tao replacing the entire extended style without knowing
            // about our layered flag (show/topmost/restore use this path).
            for style in [WS_EX_TOOLWINDOW, 0, WS_EX_TOOLWINDOW] {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style as isize);
                // Windows can normalize WS_EX_WINDOWEDGE for overlapped
                // windows. Check both bits owned by this regression scenario.
                let flags = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
                assert_eq!(flags & (WS_EX_TOOLWINDOW | WS_EX_LAYERED), style | WS_EX_LAYERED);
                let mut alpha = 0;
                assert_ne!(GetLayeredWindowAttributes(hwnd, std::ptr::null_mut(), &mut alpha, std::ptr::null_mut()), 0);
                assert_eq!(alpha, 179);
            }
        }
    }

    #[test]
    fn hover_uses_either_value_without_ordering_assumptions() {
        for (normal, hover) in [(20, 100), (100, 20), (70, 70)] {
            let settings = crate::settings::Settings { timer_opacity_percent: normal,
                hover_opacity_enabled: true, hover_opacity_percent: hover, ..Default::default() };
            let mut state = OpacityState::from_settings(&settings);
            assert_eq!(state.transition(false), Some(normal));
            assert_eq!(state.transition(true), if normal == hover { None } else { Some(hover) });
            // Moving among descendants must not cause redundant applications.
            for _ in 0..20 { assert_eq!(state.transition(true), None); }
            assert_eq!(state.transition(false), if normal == hover { None } else { Some(normal) });
            state.transition(true);
            state.hover_enabled = false;
            assert_eq!(state.transition(true), if normal == hover { None } else { Some(normal) });
            assert_eq!(state.normal, normal);
            assert_eq!(state.hover, hover);
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn hidden_window_discards_hover_opacity() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, GetLayeredWindowAttributes, WS_OVERLAPPEDWINDOW};
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        unsafe {
            let hwnd = CreateWindowExW(0, class.as_ptr(), std::ptr::null(), WS_OVERLAPPEDWINDOW,
                0, 0, 100, 100, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null());
            assert!(!hwnd.is_null());
            install_native(hwnd).unwrap();
            let state = native_state(hwnd).unwrap();
            *state.lock().unwrap() = OpacityState { normal: 70, hover_enabled: true, hover: 20, applied: Some(20),
                ..OpacityState::from_settings(&crate::settings::Settings::default()) };
            apply_native(hwnd, 20).unwrap();
            assert!(!cursor_over_timer(hwnd));
            refresh_native(hwnd, &state).unwrap();
            let mut alpha = 0;
            assert_ne!(GetLayeredWindowAttributes(hwnd, std::ptr::null_mut(), &mut alpha, std::ptr::null_mut()), 0);
            assert_eq!(alpha, 179);
            DestroyWindow(hwnd);
            assert_eq!(std::sync::Arc::strong_count(&state), 1, "destroy must release the subclass state");
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn apply(_window: &tauri::WebviewWindow, percent: u32) -> Result<(), String> {
    validate(percent)?;
    if percent == 100 {
        Ok(())
    } else {
        Err("창 불투명도는 Windows에서만 지원됩니다.".into())
    }
}
