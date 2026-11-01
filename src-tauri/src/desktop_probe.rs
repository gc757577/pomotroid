//! Opt-in desktop *WebView* experiment; no database/statistics access.
//! Implements Win32 contracts independently. Research: DEVELOPMENT-WINDOWS.md.
use std::{
    cell::{Cell, RefCell},
    ptr::{null, null_mut},
};
use tauri::{Emitter, Manager, WebviewWindow};
use windows_sys::Win32::{
    Foundation::{GetLastError, SetLastError, HWND, LPARAM, POINT, RECT},
    UI::{
        HiDpi::{AreDpiAwarenessContextsEqual, GetDpiForWindow, GetWindowDpiAwarenessContext},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};
const LABEL_PREFIX: &str = "desktop-probe-";
const SUBCLASS: usize = 0x50445042;
const TIMER: usize = 0x50445042;
const STOP: u32 = WM_APP + 42;
const WIDTH: i32 = 440;
const HEIGHT: i32 = 270;
thread_local! {
    static PROBE: RefCell<Option<Probe>> = const { RefCell::new(None) };
    static ATTACHED: Cell<isize> = const { Cell::new(0) };
}
struct Probe {
    app: tauri::AppHandle,
    main: isize,
    window: Option<WebviewWindow>,
    hwnd: isize,
    host: isize,
    label: String,
    retired: u32,
    style: isize,
    exstyle: isize,
    requested: bool,
    creating: bool,
    ready: bool,
    retries: u32,
    generation: u32,
    clicks: u32,
    mode: String,
    message: String,
    last_hit: isize,
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn with_probe<T>(f: impl FnOnce(&mut Probe) -> Result<T, String>) -> Result<T, String> {
    let mut probe = PROBE
        .with(|slot| slot.borrow_mut().take())
        .ok_or("시험 창 작업 중")?;
    let result = f(&mut probe);
    PROBE.with(|slot| *slot.borrow_mut() = Some(probe));
    result
}
unsafe extern "system" fn find_host(hwnd: HWND, out: LPARAM) -> i32 {
    let mut class = [0u16; 80];
    let n = GetClassNameW(hwnd, class.as_mut_ptr(), 80).max(0) as usize;
    if String::from_utf16_lossy(&class[..n]) == "Progman"
        && !FindWindowExW(hwnd, null_mut(), wide("SHELLDLL_DefView").as_ptr(), null()).is_null()
    {
        *(out as *mut HWND) = hwnd;
        return 0;
    }
    1
}
unsafe fn desktop_host() -> HWND {
    let mut host = null_mut();
    EnumWindows(Some(find_host), &mut host as *mut HWND as isize);
    host
}
unsafe fn set_long(hwnd: HWND, index: i32, value: isize, name: &str) -> Result<(), String> {
    SetLastError(0);
    if SetWindowLongPtrW(hwnd, index, value) == 0 && GetLastError() != 0 {
        return Err(format!("{name}: {}", GetLastError()));
    }
    Ok(())
}

impl Probe {
    fn report(&self) -> serde_json::Value {
        serde_json::json!({"mode":self.mode,"message":self.message,"clicks":self.clicks,"generation":self.generation,"ready":self.ready,"retired":self.retired})
    }
    fn update(&mut self, mode: &str, message: impl Into<String>) {
        self.mode = mode.into();
        self.message = message.into();
        log::info!(
            "[desktop-webview-probe] {} generation={} hwnd=0x{:x} host=0x{:x} clicks={} {}",
            self.mode,
            self.generation,
            self.hwnd,
            self.host,
            self.clicks,
            self.message
        );
        let _ = self
            .app
            .emit_to(self.label.as_str(), "desktop-probe:status", self.report());
    }
    unsafe fn adopt(&mut self, window: &WebviewWindow) -> Result<(), String> {
        let hwnd = window.hwnd().map_err(|e| e.to_string())?.0;
        if self.hwnd == hwnd as isize {
            return Ok(());
        }
        self.hwnd = hwnd as isize;
        self.host = 0;
        self.ready = false;
        self.last_hit = 0;
        self.style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        self.exstyle = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if SetWindowSubclass(hwnd, Some(widget_proc), SUBCLASS, self.main as usize) == 0 {
            return Err("WebView subclass failed".into());
        }
        self.window = Some(window.clone());
        self.creating = false;
        self.update("normal", "일반 Tauri WebView 창 · 통계 연결 없음");
        Ok(())
    }
    unsafe fn detach(&mut self, reason: &str) -> Result<(), String> {
        let hwnd = self.hwnd as HWND;
        ATTACHED.with(|slot| slot.set(0));
        if IsWindow(hwnd) == 0 {
            return Err("복구할 HWND가 없습니다".into());
        }
        ShowWindow(hwnd, SW_HIDE);
        SetLastError(0);
        if SetParent(hwnd, null_mut()).is_null() && GetLastError() != 0 {
            return Err(format!("detach SetParent: {}", GetLastError()));
        }
        set_long(hwnd, GWL_STYLE, self.style, "restore style")?;
        set_long(hwnd, GWL_EXSTYLE, self.exstyle, "restore exstyle")?;
        if SetWindowPos(
            hwnd,
            HWND_NOTOPMOST,
            ((GetSystemMetrics(SM_CXSCREEN) - WIDTH) / 2).max(0),
            ((GetSystemMetrics(SM_CYSCREEN) - HEIGHT) / 2).max(0),
            WIDTH,
            HEIGHT,
            SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOACTIVATE,
        ) == 0
        {
            return Err(format!("restore position: {}", GetLastError()));
        }
        self.host = 0;
        SetWindowTextW(
            hwnd,
            wide("Pomotroid Desktop WebView Probe - NORMAL").as_ptr(),
        );
        self.update("normal", reason);
        Ok(())
    }
    unsafe fn attach(&mut self) -> Result<(), String> {
        if !self.ready {
            return Err("WebView ready IPC를 먼저 확인해야 합니다".into());
        }
        let hwnd = self.hwnd as HWND;
        let host = desktop_host();
        if self.host != 0 && self.host == host as isize && GetParent(hwnd) == host {
            return Ok(());
        }
        if host.is_null() {
            return Err("Explorer 바탕화면이 아직 준비되지 않았습니다".into());
        }
        let host_ex = GetWindowLongPtrW(host, GWL_EXSTYLE) as u32;
        if host_ex & WS_EX_NOREDIRECTIONBITMAP == 0 {
            return Err("이번 검증은 최신 Progman 구성만 대상으로 합니다".into());
        }
        if AreDpiAwarenessContextsEqual(
            GetWindowDpiAwarenessContext(hwnd),
            GetWindowDpiAwarenessContext(host),
        ) == 0
        {
            return Err("DPI awareness 불일치: 타이머 DPI 보호를 위해 부착 중단".into());
        }
        log::info!(
            "[desktop-webview-probe] prepare host={host:?} exstyle=0x{host_ex:x} dpi={}",
            GetDpiForWindow(hwnd)
        );
        ShowWindow(hwnd, SW_HIDE);
        // Supported-OS manifest is required. Prepare the rendering surface
        // BEFORE SetParent, as in the raised-desktop strategy, not afterward.
        ATTACHED.with(|slot| slot.set(self.hwnd));
        set_long(
            hwnd,
            GWL_STYLE,
            ((self.style as u32
                & !(WS_POPUP | WS_CAPTION | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX))
                | WS_CHILD
                | WS_CLIPCHILDREN) as isize,
            "child style",
        )?;
        set_long(
            hwnd,
            GWL_EXSTYLE,
            ((self.exstyle as u32 & !(WS_EX_APPWINDOW | WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST))
                | WS_EX_LAYERED
                | WS_EX_TOOLWINDOW) as isize,
            "layered surface",
        )?;
        SetLastError(0);
        if SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA) == 0 {
            return Err(format!(
                "SetLayeredWindowAttributes before SetParent: {}",
                GetLastError()
            ));
        }
        let mut alpha = 0;
        if GetLayeredWindowAttributes(hwnd, null_mut(), &mut alpha, null_mut()) == 0 || alpha != 255
        {
            return Err("opaque surface readback failed".into());
        }
        SetLastError(0);
        if SetParent(hwnd, host).is_null() && GetLastError() != 0 {
            return Err(format!("attach SetParent: {}", GetLastError()));
        }
        if GetParent(hwnd) != host {
            return Err("parent readback mismatch".into());
        }
        let mut rect = RECT::default();
        if GetClientRect(host, &mut rect) == 0 {
            return Err("desktop bounds unavailable".into());
        }
        // Unlike wallpaper, this *small interactive widget* sits above the
        // icon view inside Progman. No full-screen overlay or raw-input sink.
        if SetWindowPos(
            hwnd,
            HWND_TOP,
            (rect.right - WIDTH - 80).max(0),
            140,
            WIDTH,
            HEIGHT,
            SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOACTIVATE,
        ) == 0
        {
            return Err(format!("desktop position: {}", GetLastError()));
        }
        self.host = host as isize;
        self.last_hit = 0;
        SetWindowTextW(
            hwnd,
            wide("Pomotroid Desktop WebView Probe - ATTACHED CANDIDATE").as_ptr(),
        );
        self.update(
            "attached-candidate",
            "부모 연결과 alpha=255 확인 · 실제 표시·클릭은 별도 확인",
        );
        Ok(())
    }
    unsafe fn attempt(&mut self) {
        if let Err(error) = self.attach() {
            self.requested = false;
            log::warn!("[desktop-webview-probe] attach failed: {error}");
            if let Err(fallback) = self.detach(&error) {
                log::error!("[desktop-webview-probe] fallback failed: {fallback}; recreating normal WebView");
                if let Some(window) = self.window.take() {
                    let _ = window.destroy();
                }
                self.hwnd = 0;
                self.spawn_window();
            }
        }
    }
    fn spawn_window(&mut self) {
        if self.creating {
            return;
        }
        self.creating = true;
        self.generation += 1;
        self.label = format!("{LABEL_PREFIX}{}", self.generation);
        let generation = self.generation;
        let label = self.label.clone();
        let focus = !self.requested;
        let app = self.app.clone();
        std::thread::spawn(move || {
            // Creating WebViews in synchronous callbacks deadlocks WebView2.
            let result = tauri::WebviewWindowBuilder::new(
                &app,
                label,
                tauri::WebviewUrl::App("desktop-probe".into()),
            )
            .title("Pomotroid Desktop WebView Probe - NORMAL")
            .inner_size(WIDTH as f64, HEIGHT as f64)
            .resizable(false)
            .decorations(true)
            .center()
            .visible(true)
            .focused(focus)
            .build();
            let _ = app.run_on_main_thread(move || {
                let _ = with_probe(|probe| {
                    if generation != probe.generation {
                        if let Ok(window) = result {
                            let _ = window.destroy();
                        }
                        return Ok(());
                    }
                    match result {
                        Ok(window) => unsafe {
                            probe.adopt(&window)?;
                        },
                        Err(error) => {
                            probe.creating = false;
                            probe.update("failed", format!("WebView 생성 실패: {error}"));
                        }
                    }
                    Ok(())
                });
            });
        });
    }
    unsafe fn tick(&mut self) {
        if !self.requested {
            return;
        }
        if self.hwnd == 0 || IsWindow(self.hwnd as HWND) == 0 {
            if self.creating {
                return;
            }
            self.ready = false;
            self.host = 0;
            if let Some(stale) = self.window.take() {
                let webview: &tauri::Webview = stale.as_ref();
                let closed = webview.close();
                let _ = stale.destroy();
                self.retired += 1;
                log::warn!("[desktop-webview-probe] lost HWND; retired label={} webview_close={closed:?} retired_count={}; window metadata may remain", stale.label(), self.retired);
            }
            self.hwnd = 0;
            self.spawn_window();
            return;
        }
        let host = desktop_host();
        if self.host != 0
            && (host as isize != self.host || GetParent(self.hwnd as HWND) as isize != self.host)
        {
            self.retries = 0;
            if let Err(error) = self.detach("Explorer 교체 감지 · 바탕화면 준비 대기")
            {
                log::warn!("[desktop-webview-probe] {error}");
            }
        }
        if self.host == 0 && self.ready {
            self.retries += 1;
            if !host.is_null() {
                self.attempt();
            } else if self.retries >= 15 {
                self.requested = false;
                let _ = self.detach("Explorer 복구 시간 초과 · 일반 창 유지");
            }
        }
        if self.host != 0 {
            let mut rect = RECT::default();
            GetWindowRect(self.hwnd as HWND, &mut rect);
            let hit = WindowFromPoint(POINT {
                x: rect.left + 60,
                y: rect.top + 120,
            });
            if hit as isize != self.last_hit {
                self.last_hit = hit as isize;
                let mut class = [0u16; 100];
                let n = GetClassNameW(hit, class.as_mut_ptr(), 100).max(0) as usize;
                log::info!(
                    "[desktop-webview-probe] hit-test class={} own={} root={:?}",
                    String::from_utf16_lossy(&class[..n]),
                    hit as isize == self.hwnd || IsChild(self.hwnd as HWND, hit) != 0,
                    GetAncestor(hit, GA_ROOT)
                );
            }
        }
    }
}
unsafe extern "system" fn widget_proc(
    hwnd: HWND,
    msg: u32,
    w: usize,
    l: isize,
    id: usize,
    main: usize,
) -> isize {
    // Explorer can send WM_CLOSE while tearing down foreign child windows.
    // Only the user's system close command disables automatic recovery.
    if msg == WM_SYSCOMMAND && w & 0xfff0 == SC_CLOSE as usize {
        PostMessageW(main as HWND, STOP, 0, 0);
        return 0;
    }
    if msg == WM_CLOSE {
        log::info!("[desktop-webview-probe] external WM_CLOSE; preserving recovery intent");
    }
    let result = DefSubclassProc(hwnd, msg, w, l);
    if msg == WM_NCDESTROY {
        ATTACHED.with(|slot| {
            if slot.get() == hwnd as isize {
                slot.set(0);
            }
        });
        RemoveWindowSubclass(hwnd, Some(widget_proc), id);
    } else if msg == WM_STYLECHANGING && l != 0 && ATTACHED.with(|slot| slot.get() == hwnd as isize)
    {
        let styles = &mut *(l as *mut STYLESTRUCT);
        if w as i32 == GWL_EXSTYLE {
            styles.styleNew |= WS_EX_LAYERED;
            styles.styleNew &= !WS_EX_TOPMOST;
        } else if w as i32 == GWL_STYLE {
            styles.styleNew =
                (styles.styleNew | WS_CHILD) & !(WS_POPUP | WS_CAPTION | WS_THICKFRAME);
        }
    }
    result
}
unsafe extern "system" fn supervisor(
    hwnd: HWND,
    msg: u32,
    w: usize,
    l: isize,
    id: usize,
    _: usize,
) -> isize {
    if msg == WM_TIMER && w == TIMER {
        let _ = with_probe(|probe| {
            probe.tick();
            Ok(())
        });
    } else if msg == STOP || msg == WM_NCDESTROY {
        log::info!("[desktop-webview-probe] supervisor stop message=0x{msg:x}");
        let _ = with_probe(|probe| {
            probe.requested = false;
            if probe.hwnd != 0 && IsWindow(probe.hwnd as HWND) != 0 {
                let _ = probe.detach("시험 종료");
            }
            if let Some(window) = probe.window.take() {
                let _ = window.destroy();
            }
            Ok(())
        });
        if msg == WM_NCDESTROY {
            let _ = with_probe(|probe| {
                probe.app.exit(0);
                Ok(())
            });
            KillTimer(hwnd, TIMER);
            RemoveWindowSubclass(hwnd, Some(supervisor), id);
        }
    }
    DefSubclassProc(hwnd, msg, w, l)
}
pub fn action(window: &WebviewWindow, action: &str) -> Result<serde_json::Value, String> {
    with_probe(|probe| unsafe {
        if window.label() != probe.label {
            return Err("wrong caller".into());
        }
        probe.adopt(window)?;
        match action {
            "ready" => {
                probe.ready = true;
                let mode = if probe.host == 0 {
                    "normal"
                } else {
                    "attached-candidate"
                };
                probe.update(
                    mode,
                    "WebView 표시 및 IPC 준비 완료 · Click test로 입력 확인",
                );
                if probe.requested {
                    probe.tick();
                }
            }
            "click" => {
                probe.clicks += 1;
                let mode = probe.mode.clone();
                probe.update(&mode, "WebView 클릭과 Rust IPC 수신 확인");
            }
            "attach" => {
                probe.requested = true;
                probe.retries = 0;
                probe.attempt();
            }
            "detach" => {
                probe.requested = false;
                probe.detach("사용자 요청으로 일반 창 복구")?;
            }
            _ => return Err("unknown probe action".into()),
        }
        Ok(probe.report())
    })
}
pub fn start(main: &WebviewWindow) -> Result<(), String> {
    unsafe {
        let hwnd = main.hwnd().map_err(|e| e.to_string())?.0;
        if SetWindowSubclass(hwnd, Some(supervisor), SUBCLASS, 0) == 0
            || SetTimer(hwnd, TIMER, 1000, None) == 0
        {
            RemoveWindowSubclass(hwnd, Some(supervisor), SUBCLASS);
            return Err("probe supervisor failed".into());
        }
        PROBE.with(|slot| {
            *slot.borrow_mut() = Some(Probe {
                app: main.app_handle().clone(),
                main: hwnd as isize,
                window: None,
                hwnd: 0,
                host: 0,
                label: String::new(),
                retired: 0,
                style: 0,
                exstyle: 0,
                requested: false,
                creating: false,
                ready: false,
                retries: 0,
                generation: 0,
                clicks: 0,
                mode: "loading".into(),
                message: String::new(),
                last_hit: 0,
            })
        });
        with_probe(|probe| {
            probe.spawn_window();
            Ok(())
        })?;
    }
    Ok(())
}
