//! memwarden - Windows memory dashboard (read-only in this version).

// Hide the console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(windows))]
compile_error!("memwarden currently supports Windows only");

use eframe::egui;

rust_i18n::i18n!("locales", fallback = "en");

mod activity;
mod app;
mod audio;
mod bridge;
mod classify;
mod close;
mod growth;
mod i18n;
mod integration;
mod priority;
mod pagefile;
mod process;
mod sampler;
mod system;
mod theme;
mod trim;

fn main() -> eframe::Result {
    // The same picture as the executable's icon (embedded by build.rs).
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png")).expect("icon is a valid PNG");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("memwarden")
            .with_icon(icon)
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([1140.0, 680.0]),
        ..Default::default()
    };
    eframe::run_native(
        "memwarden",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
