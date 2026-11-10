//! Render the production tray function for review; does not start the app/UI.
use pomotroid_lib::tray::{render_tray_icon_rgba, TrayColors};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::path::PathBuf::from(std::env::args_os().nth(1).ok_or("output directory required")?);
    std::fs::create_dir_all(&output)?;
    for (name, progress, paused, round) in [
        ("full", 0.0, false, "work"), ("half", 0.5, false, "work"),
        ("quarter", 0.75, false, "work"), ("empty", 1.0, false, "work"),
        ("paused", 0.5, true, "work"), ("short-break", 0.5, false, "short-break"),
    ] {
        let mut rgba = render_tray_icon_rgba(&TrayColors::default(), paused, progress, round, true);
        for pixel in rgba.as_chunks_mut::<4>().0 {
            let alpha = pixel[3] as u16;
            for color in &mut pixel[..3] { *color = ((*color as u16 * alpha + 127) / 255) as u8; }
        }
        let pixmap = tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(64, 64).unwrap()).ok_or("invalid RGBA")?;
        pixmap.save_png(output.join(format!("tray-{name}.png")))?;
    }
    Ok(())
}
