//! `firecraft` — the desktop shell entry point. The wasm face of this
//! crate is the library (consumed by `firecraft-web`); the binary is
//! native-only.

// A release build is a windowed app: no console window behind it.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

/// The window icon, decoded from the same art the installer uses.
#[cfg(not(target_arch = "wasm32"))]
fn window_icon() -> egui::IconData {
    const PNG: &[u8] = include_bytes!("../../../packaging/windows/firecraft-512.png");
    io::raster::decode(PNG).map_or_else(
        |_| egui::IconData::default(),
        |image| egui::IconData {
            rgba: image.rgba,
            width: image.width,
            height: image.height,
        },
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 600.0])
            .with_icon(window_icon())
            .with_title("FireCraft"),
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        "FireCraft",
        options,
        Box::new(|_cc| Ok(Box::new(app::shell::Shell::new(_cc)))),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {}
