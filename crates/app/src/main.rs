//! MeterOscope — mesurer sur des photos ou des captures caméra.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod camera_view;
mod clipboard;
mod history;
mod language;
mod prefs;
mod profile_view;
mod settings;

/// Icône de la fenêtre et de la barre des tâches.
fn window_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon/meteroscope-256.png"))
        .expect("icône PNG intégrée valide")
}

fn main() -> eframe::Result {
    language::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MeterOscope")
            .with_app_id("meteroscope")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([720.0, 480.0])
            .with_drag_and_drop(true)
            .with_icon(window_icon()),
        // Taille et position de la fenêtre, restaurées au lancement suivant.
        persistence_path: settings::window_file(),
        ..Default::default()
    };
    let initial = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    eframe::run_native("MeterOscope", options, Box::new(move |cc| Ok(Box::new(app::MeterOscopeApp::new(cc, initial)))))
}
