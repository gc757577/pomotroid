//! Login launch policy, single-instance handoff and guarded current-user registration.
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager};

pub const LOGIN_ARG: &str = "--from-autostart";
static EARLY_MANUAL_LAUNCH: AtomicBool = AtomicBool::new(false);

pub fn is_login(args: &[String]) -> bool { args.iter().any(|arg| arg == LOGIN_ARG) }

pub struct LaunchState {
    show_requested: AtomicBool,
    ready: AtomicBool,
    force_tray: bool,
}

impl LaunchState {
    pub fn new(login: bool, hidden: bool) -> Self {
        Self { show_requested: AtomicBool::new(!login || !hidden), ready: AtomicBool::new(false), force_tray: login && hidden }
    }
}

pub fn initialize(app: &AppHandle, settings: &crate::settings::Settings) {
    let args: Vec<_> = std::env::args().collect();
    let hidden = if cfg!(debug_assertions) && is_login(&args) && args.iter().any(|arg| arg == "--autostart-show-window") {
        false // Debug-only launch-policy smoke test; never registered or saved.
    } else { settings.autostart_start_hidden };
    let state = LaunchState::new(is_login(&args), hidden);
    if EARLY_MANUAL_LAUNCH.swap(false, Ordering::SeqCst) { state.show_requested.store(true, Ordering::SeqCst); }
    log::info!("[startup] login={} start_hidden={} timer_initial=idle", is_login(&args), state.force_tray);
    app.manage(state);
}

pub fn tray_required(app: &AppHandle) -> bool {
    app.try_state::<LaunchState>().is_some_and(|state| state.force_tray)
}

pub fn second_instance(app: &AppHandle, args: &[String]) {
    // A duplicate login launch must not interrupt a session already running.
    if is_login(args) { log::info!("[startup] duplicate login ignored"); return; }
    if let Some(state) = app.try_state::<LaunchState>() {
        state.show_requested.store(true, Ordering::SeqCst);
        if !state.ready.load(Ordering::SeqCst) { return; }
    } else { EARLY_MANUAL_LAUNCH.store(true, Ordering::SeqCst); return; }
    if let Some(window) = app.get_webview_window("main") {
        let result = window.show().and_then(|_| window.unminimize()).and_then(|_| window.set_focus());
        log::info!("[startup] manual second launch restored={} visible={:?}", result.is_ok(), window.is_visible());
    }
}

/// The frontend calls this only after locale, theme and mode are prepared.
#[tauri::command]
pub fn startup_ready(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != "main" { return Err("Only the timer can finish startup".into()); }
    let app = window.app_handle();
    let Some(state) = app.try_state::<LaunchState>() else { return window.show().map_err(|e| e.to_string()); };
    if state.ready.swap(true, Ordering::SeqCst) { return Ok(()); }
    let tray_ok = app.try_state::<std::sync::Arc<crate::tray::TrayState>>()
        .is_some_and(|tray| tray.icon.lock().is_ok_and(|icon| icon.is_some()));
    let show = state.show_requested.load(Ordering::SeqCst) || !tray_ok;
    if show { window.show().map_err(|e| e.to_string())?; }
    let running = app.try_state::<crate::timer::TimerController>().is_some_and(|timer| timer.get_snapshot().is_running);
    log::info!("[startup] ready show={show} tray={tray_ok} visible={:?} timer_running={running}", window.is_visible());
    Ok(())
}

#[derive(serde::Serialize)]
pub struct AutostartStatus {
    pub available: bool,
    pub enabled: bool,
    pub registered: bool,
    pub reason: String,
    pub executable: String,
    pub start_hidden: bool,
}

#[cfg(target_os = "windows")]
mod windows {
    use super::*;
    use std::{io, path::Path};
    use tauri_plugin_autostart::ManagerExt;
    use winreg::{enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE}, RegKey, RegValue};

    pub const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    pub const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Pomotroid Fork Preview";

    pub fn command(exe: &Path) -> String { format!("\"{}\" {LOGIN_ARG}", exe.display()) }

    pub fn installed_path(debug: bool, exe: &Path, location: Option<&str>) -> bool {
        if debug || !exe.is_absolute() || exe.file_name().is_none_or(|name| name != "pomotroid-fork-preview.exe") { return false; }
        let Some(location) = location else { return false; };
        let installed = Path::new(location.trim_matches('"'));
        match (installed.canonicalize(), exe.parent().and_then(|p| p.canonicalize().ok())) {
            (Ok(installed), Some(parent)) => installed == parent && exe.is_file(),
            _ => false,
        }
    }

    pub fn read(path: &str, name: &str) -> io::Result<Option<RegValue>> {
        let result = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(path, KEY_READ)
            .and_then(|key| key.get_raw_value(name));
        match result { Ok(value) => Ok(Some(value)), Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None), Err(e) => Err(e) }
    }

    pub fn write(path: &str, name: &str, value: Option<&RegValue>) -> io::Result<()> {
        if let Some(value) = value {
            RegKey::predef(HKEY_CURRENT_USER).create_subkey(path)?.0.set_raw_value(name, value)
        } else {
            let result = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(path, KEY_WRITE)
                .and_then(|key| key.delete_value(name));
            match result { Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()), other => other }
        }
    }

    // Supplement auto-launch 0.5's timestamp-only check. Unknown states stay off.
    // This format is a Windows implementation detail; preserve it verbatim.
    pub fn approved(value: Option<&RegValue>) -> bool {
        value.is_none_or(|value| value.vtype == winreg::enums::REG_BINARY
            && value.bytes.len() >= 12 && matches!(value.bytes[0..4], [2, 0, 0, 0] | [6, 0, 0, 0]))
    }

    pub fn status(app: &AppHandle, conn: &rusqlite::Connection) -> Result<AutostartStatus, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let location: Option<String> = RegKey::predef(HKEY_CURRENT_USER).open_subkey(UNINSTALL).ok()
            .and_then(|key| key.get_value("InstallLocation").ok());
        let available = installed_path(cfg!(debug_assertions), &exe, location.as_deref())
            && app.config().identifier == "io.github.gc75.pomotroidfork.preview";
        let name = &app.config().identifier;
        let run = read(RUN, name).map_err(|e| e.to_string())?;
        let approval = read(APPROVED, name).map_err(|e| e.to_string())?;
        let registered = run.is_some();
        let matches_command = run.as_ref().and_then(|value| <String as winreg::types::FromRegValue>::from_reg_value(value).ok())
            .is_some_and(|actual| actual == command(&exe));
        let blocked = !approved(approval.as_ref());
        let enabled = available && matches_command && !blocked;
        // Reconcile by reading, never by enabling registration on application start.
        crate::settings::save_setting(conn, "autostart_enabled", if enabled { "true" } else { "false" }).map_err(|e| e.to_string())?;
        let settings = crate::settings::load(conn).map_err(|e| e.to_string())?;
        Ok(AutostartStatus { available, enabled, registered, executable: exe.display().to_string(), start_hidden: settings.autostart_start_hidden,
            reason: if cfg!(debug_assertions) { "development" } else if !available { "not_installed" }
                else if blocked { "windows_disabled" } else if registered && !matches_command { "registration_mismatch" } else { "" }.into() })
    }

    pub fn set(app: &AppHandle, conn: &mut rusqlite::Connection, enabled: bool, hidden: bool) -> Result<AutostartStatus, String> {
        let before = status(app, conn)?;
        if enabled && !before.available { return Err("autostart_unavailable".into()); }
        let name = &app.config().identifier;
        let old_run = read(RUN, name).map_err(|e| e.to_string())?;
        let old_approval = read(APPROVED, name).map_err(|e| e.to_string())?;
        let operation = (|| -> Result<(), String> {
            if enabled {
                RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN).map_err(|e| e.to_string())?;
                app.autolaunch().enable().map_err(|e| e.to_string())?;
                // The pinned backend doesn't quote paths with spaces. Normalize
                // only our own Run value, then verify the effective Windows state.
                let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN, KEY_WRITE).map_err(|e| e.to_string())?;
                key.set_value(name, &command(Path::new(&before.executable))).map_err(|e| e.to_string())?;
            } else if old_run.is_some() { app.autolaunch().disable().map_err(|e| e.to_string())?; }
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            if status(app, &tx)?.enabled != enabled { return Err("autostart_verification_failed".into()); }
            crate::settings::save_setting(&tx, "autostart_start_hidden", if hidden { "true" } else { "false" }).map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            Ok(())
        })();
        if let Err(error) = operation {
            let rollback = write(RUN, name, old_run.as_ref()).and_then(|_| write(APPROVED, name, old_approval.as_ref()));
            log::error!("[startup] registration failed: {error}; rollback={rollback:?}");
            return Err(if rollback.is_ok() { "autostart_failed" } else { "autostart_rollback_failed" }.into());
        }
        status(app, conn)
    }
}

#[tauri::command]
pub fn autostart_status(app: AppHandle, db: tauri::State<'_, crate::db::DbState>) -> Result<AutostartStatus, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    { windows::status(&app, &conn) }
    #[cfg(not(target_os = "windows"))]
    { let _ = (app, conn); Err("autostart_unavailable".into()) }
}

#[tauri::command]
pub fn autostart_set(app: AppHandle, db: tauri::State<'_, crate::db::DbState>, enabled: bool, start_hidden: bool) -> Result<AutostartStatus, String> {
    use tauri::Emitter;
    let mut conn = db.lock().map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    {
        let status = windows::set(&app, &mut conn, enabled, start_hidden)?;
        let settings = crate::settings::load(&conn).map_err(|e| e.to_string())?;
        drop(conn);
        app.emit("settings:changed", settings).ok();
        Ok(status)
    }
    #[cfg(not(target_os = "windows"))]
    { let _ = (app, conn, enabled, start_hidden); Err("autostart_unavailable".into()) }
}

pub fn reconcile(app: &AppHandle, conn: &rusqlite::Connection) {
    #[cfg(target_os = "windows")]
    if let Err(error) = windows::status(app, conn) { log::warn!("[startup] registration status unavailable: {error}"); }
    #[cfg(not(target_os = "windows"))]
    let _ = (app, conn);
}

pub fn disable_for_reset(app: &AppHandle, conn: &mut rusqlite::Connection) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    { windows::set(app, conn, false, true)?; }
    #[cfg(not(target_os = "windows"))]
    let _ = (app, conn);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn direct_launch_always_shows_and_login_defaults_to_tray() {
        for hidden in [false, true] {
            assert!(LaunchState::new(false, hidden).show_requested.load(Ordering::SeqCst));
            assert!(!LaunchState::new(false, hidden).force_tray);
            let login = LaunchState::new(true, hidden);
            assert_eq!(login.force_tray, hidden);
            assert_eq!(login.show_requested.load(Ordering::SeqCst), !hidden);
        }
        assert!(is_login(&["app.exe".into(), LOGIN_ARG.into()]));
        assert!(!is_login(&["app.exe".into(), "--from-autostart-no".into()]));
    }
    #[cfg(target_os = "windows")]
    #[test]
    fn development_path_and_windows_disabled_registration_are_not_enabled() {
        use winreg::{RegValue, enums::REG_BINARY};
        assert!(!windows::installed_path(true, std::path::Path::new(r"C:\dev\pomotroid.exe"), Some(r"C:\dev")));
        assert!(!windows::installed_path(false, std::path::Path::new(r"C:\dev\pomotroid-fork-preview.exe"), None));
        assert_eq!(windows::command(std::path::Path::new(r"C:\App With Spaces\pomotroid-fork-preview.exe")),
            "\"C:\\App With Spaces\\pomotroid-fork-preview.exe\" --from-autostart");
        assert!(windows::approved(None));
        for first in [0, 1, 2, 3, 6, 7, 255] {
            let mut bytes = vec![0; 12]; bytes[0] = first;
            assert_eq!(windows::approved(Some(&RegValue { vtype: REG_BINARY, bytes })), matches!(first, 2 | 6));
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "explicit standalone release fixture; temporarily writes only a uniquely named HKCU test entry"]
    fn windows_run_registration_enable_disable_and_disabled_state() {
        use winreg::{RegValue, enums::{REG_BINARY, HKEY_CURRENT_USER}, RegKey};
        let exe = std::path::PathBuf::from(std::env::var_os("POMOTROID_AUTOSTART_TEST_EXE").expect("set explicit standalone fixture"));
        assert!(exe.is_absolute() && exe.is_file());
        assert_eq!(exe.file_name().unwrap(), "pomotroid-fork-preview.exe");
        let name = format!("io.github.gc75.pomotroidfork.dev.autostart-test-{}", std::process::id());
        assert!(windows::read(windows::RUN, &name).unwrap().is_none());
        assert!(windows::read(windows::APPROVED, &name).unwrap().is_none());
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                windows::write(windows::RUN, &self.0, None).expect("remove own test Run entry");
                windows::write(windows::APPROVED, &self.0, None).expect("remove own test approval entry");
            }
        }
        let _cleanup = Cleanup(name.clone());
        RegKey::predef(HKEY_CURRENT_USER).create_subkey(windows::RUN).unwrap();
        let engine = auto_launch::AutoLaunchBuilder::new().set_app_name(&name)
            .set_app_path(&exe.display().to_string()).set_args(&[LOGIN_ARG]).build().unwrap();
        engine.enable().unwrap();
        RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(windows::RUN, winreg::enums::KEY_WRITE).unwrap()
            .set_value(&name, &windows::command(&exe)).unwrap();
        assert!(engine.is_enabled().unwrap());
        let raw = windows::read(windows::RUN, &name).unwrap().unwrap();
        assert_eq!(<String as winreg::types::FromRegValue>::from_reg_value(&raw).unwrap(), windows::command(&exe));
        // Windows-disabled format with a zero timestamp exposes the pinned
        // backend's timestamp-only edge case; our stricter check stays off.
        let mut disabled = vec![0; 12]; disabled[0] = 3;
        let value = RegValue { vtype: REG_BINARY, bytes: disabled };
        windows::write(windows::APPROVED, &name, Some(&value)).unwrap();
        let observed = windows::read(windows::APPROVED, &name).unwrap();
        assert!(!windows::approved(observed.as_ref()));
        assert_eq!(observed.unwrap().bytes, value.bytes, "inspection must never re-enable Windows startup");
        engine.disable().unwrap();
        assert!(windows::read(windows::RUN, &name).unwrap().is_none());
    }
}
