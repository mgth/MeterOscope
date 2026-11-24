//! MeterOscope — mesurer sur des photos ou des captures caméra.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod camera_view;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MeterOscope")
            .with_app_id("meteroscope")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([720.0, 480.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    let initial = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    eframe::run_native("MeterOscope", options, Box::new(move |cc| Ok(Box::new(app::MeterOscopeApp::new(cc, initial)))))
}
