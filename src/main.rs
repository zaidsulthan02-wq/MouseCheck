// Hide the console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod analysis;
mod capture;
mod findings;
mod hid;
mod session;
mod ui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MouseCheck: honest mouse diagnostics")
            .with_inner_size([1360.0, 880.0])
            .with_min_inner_size([1000.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "MouseCheck",
        options,
        Box::new(|cc| {
            ui::widgets::apply_theme(&cc.egui_ctx);
            // Start capture after the window exists: the UI toolkit registers its own raw
            // input when its event loop is created, and the last registration wins.
            let cap = capture::start();
            Ok(Box::new(ui::App::new(cap)))
        }),
    )
}
