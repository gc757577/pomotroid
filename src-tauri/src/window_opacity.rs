//! Native whole-window opacity. Only the timer window may be changed.

#[cfg(target_os = "windows")]
const OPACITY_SUBCLASS: usize = 0x504f4d4f;

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
    if SetWindowSubclass(hwnd, Some(preserve_layered_style), OPACITY_SUBCLASS, 0) == 0 {
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
        WindowsAndMessaging::{GWL_EXSTYLE, STYLESTRUCT, WM_NCDESTROY, WM_STYLECHANGING, WS_EX_LAYERED},
    };
    if message == WM_NCDESTROY {
        RemoveWindowSubclass(hwnd, Some(preserve_layered_style), subclass);
    }
    let result = DefSubclassProc(hwnd, message, wparam, lparam);
    if message == WM_STYLECHANGING && wparam as i32 == GWL_EXSTYLE && lparam != 0 {
        // SAFETY: Windows supplies a writable STYLESTRUCT for this synchronous
        // message. Tao rebuilds styles on show/topmost/restore; retain only our
        // layered bit and leave its other style decisions intact.
        (*(lparam as *mut STYLESTRUCT)).styleNew |= WS_EX_LAYERED;
    }
    result
}

pub fn validate(percent: u32) -> Result<u8, String> {
    if !(20..=100).contains(&percent) {
        return Err("불투명도는 20~100% 사이여야 합니다.".into());
    }
    Ok(((percent * 255 + 50) / 100) as u8)
}

#[cfg(target_os = "windows")]
pub fn apply(window: &tauri::WebviewWindow, percent: u32) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::{GetLastError, SetLastError},
        UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW,
            GWL_EXSTYLE, LWA_ALPHA, WS_EX_LAYERED,
        },
    };

    let alpha = validate(percent)?;
    if window.label() != "main" {
        return Err("불투명도는 타이머 창에만 적용할 수 있습니다.".into());
    }
    let hwnd = window.hwnd().map_err(|e| e.to_string())?.0;
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
