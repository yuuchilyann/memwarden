//! memwarden - Windows memory dashboard (read-only in this version).

// Hide the console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(windows))]
compile_error!("memwarden currently supports Windows only");

use eframe::egui;

rust_i18n::i18n!("locales", fallback = "en");

mod activity;
mod app;
mod classify;
mod i18n;
mod priority;
mod process;
mod sampler;
mod system;
mod theme;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("memwarden")
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
