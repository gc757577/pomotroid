//! Native whole-window opacity. Only the timer window may be changed.

#[cfg(any(target_os = "windows", test))]
#[derive(Clone)]
struct OpacityState {
    normal: u32,
    hover_enabled: bool,
    hover: u32,
    applied: Option<u32>,
    applying: bool,
}

#[cfg(any(target_os = "windows", test))]
impl OpacityState {
    fn from_settings(settings: &crate::settings::Settings) -> Self {
        Self {
            normal: settings.timer_opacity_percent,
            hover_enabled: settings.hover_opacity_enabled,
            hover: settings.hover_opacity_percent,
            applied: None,
            applying: false,
        }
    }

    fn transition(&mut self, over_timer: bool) -> Option<u32> {
        let percent = if self.hover_enabled && over_timer { self.hover } else { self.normal };
        if self.applied == Some(percent) { return None; }
        self.applied = Some(percent);
        Some(percent)
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
            WM_ACTIVATE, WM_SETFOCUS},
    };
    // Hold a local reference across DefSubclassProc, which can reenter and
    // destroy the window. No state lock is held across any Windows calls.
    let state = native_state(hwnd).ok();
    if message == WM_NCDESTROY {
        KillTimer(hwnd, HOVER_TIMER);
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
    if (message == WM_TIMER && wparam == HOVER_TIMER)
        || matches!(message, WM_SIZE | WM_SHOWWINDOW | WM_WINDOWPOSCHANGED | WM_EXITSIZEMOVE | WM_ACTIVATE | WM_SETFOCUS)
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
    let over = cursor_over_timer(hwnd);
    let percent = {
        let mut cached = state.lock().map_err(|e| e.to_string())?;
        if cached.applying { return Ok(()); }
        let percent = cached.transition(over);
        cached.applying = percent.is_some();
        percent
    };
    if let Some(percent) = percent {
        let result = apply_native(hwnd, percent);
        let mut cached = state.lock().map_err(|e| e.to_string())?;
        cached.applying = false;
        if result.is_err() { cached.applied = None; }
        result?;
    }
    Ok(())
}

/// Update cached preferences and immediately resolve the effective opacity.
/// Called on the UI thread; hover transitions never access settings storage.
pub fn configure(window: &tauri::WebviewWindow, settings: &crate::settings::Settings) -> Result<(), String> {
    validate(settings.timer_opacity_percent)?;
    validate(settings.hover_opacity_percent)?;
    if window.label() != "main" { return Err("불투명도는 타이머 창에만 적용할 수 있습니다.".into()); }
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SetTimer, KillTimer};
        let hwnd = window.hwnd().map_err(|e| e.to_string())?.0;
        let state = native_state(hwnd)?;
        if settings.hover_opacity_enabled {
            if SetTimer(hwnd, HOVER_TIMER, 50, None) == 0 { return Err("호버 감지를 시작할 수 없습니다.".into()); }
        } else {
            KillTimer(hwnd, HOVER_TIMER);
        }
        *state.lock().map_err(|e| e.to_string())? = OpacityState::from_settings(settings);
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
    validate(percent)?;
    if window.label() != "main" { return Err("불투명도는 타이머 창에만 적용할 수 있습니다.".into()); }
    let hwnd = window.hwnd().map_err(|e| e.to_string())?.0;
    unsafe {
        let state = native_state(hwnd)?;
        {
            let mut cached = state.lock().map_err(|e| e.to_string())?;
            cached.normal = percent;
            cached.applied = None;
        }
        refresh_native(hwnd, &state)
    }
}

#[cfg(target_os = "windows")]
unsafe fn apply_native(hwnd: windows_sys::Win32::Foundation::HWND, percent: u32) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::{GetLastError, SetLastError},
        UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW,
            GWL_EXSTYLE, LWA_ALPHA, WS_EX_LAYERED,
        },
    };

    let alpha = validate(percent)?;
    // SAFETY: Tauri owns this live HWND. Preserve every existing extended
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
            log::debug!("[window-opacity] main percent={percent} alpha={observed}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
                normal: 70, hover_enabled: false, hover: 100, applied: Some(70), applying: false,
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
            *state.lock().unwrap() = OpacityState { normal: 70, hover_enabled: true, hover: 20, applied: Some(20), applying: false };
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
