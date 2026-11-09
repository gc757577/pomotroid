use std::process::Command;

fn main() {
    // Rerun when git HEAD changes (new commit, checkout) or when tags move.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs/");
    // Rerun if the canonical version source changes.
    println!("cargo:rerun-if-changed=tauri.conf.json");

    let base_version = read_base_version();

    // Full 40-char SHA for the log line.
    let full_sha = run_git(&["rev-parse", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let short_sha = full_sha.get(..8).unwrap_or(&full_sha);
    let dirty = run_git(&["status", "--porcelain"])
        .is_some_and(|status| !status.trim().is_empty());
    let suffix = if dirty { ".dirty" } else { "" };
    let build_version = format!("{base_version}+{short_sha}{suffix}");

    println!("cargo:rustc-env=APP_BUILD_VERSION={build_version}");
    println!("cargo:rustc-env=APP_BUILD_SHA={full_sha}");

    println!("cargo:rerun-if-changed=desktop-probe.manifest.xml");
    #[cfg(target_os = "windows")]
    {
        let windows = tauri_build::WindowsAttributes::new()
            .app_manifest(include_str!("desktop-probe.manifest.xml"));
        tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
            .expect("failed to build Windows desktop resources");
    }
    #[cfg(not(target_os = "windows"))]
    tauri_build::build()
}

// ---------------------------------------------------------------------------
// Git helpers
// ---------------------------------------------------------------------------

fn run_git(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
}

// ---------------------------------------------------------------------------
// Version parsing
// ---------------------------------------------------------------------------

/// Read "version" from tauri.conf.json (same directory as build.rs = src-tauri/).
/// Falls back to "1.0.0" if the file is unreadable or the field is missing.
fn read_base_version() -> String {
    std::fs::read_to_string("tauri.conf.json")
        .ok()
        .and_then(|content| extract_json_str_field(&content, "version"))
        .unwrap_or_else(|| "1.0.0".to_string())
}

/// Minimal JSON string-field extractor — avoids a serde_json build-dependency.
fn extract_json_str_field(json: &str, field: &str) -> Option<String> {
    let key = format!("\"{}\"", field);
    let pos = json.find(&key)?;
    let rest = json[pos + key.len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}
