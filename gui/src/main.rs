mod app;
mod dbus_client;
mod theme;
mod pages;
mod keyboard_shortcuts;
mod widgets;

use app::TuxedoApp;

fn main() -> Result<(), eframe::Error> {
    env_logger::init();

    // Create and enter a Tokio runtime context.
    // This is required for `tokio::spawn` to work in the `DbusClient`.
    let rt = tokio::runtime::Runtime::new().expect("Unable to create a Tokio runtime");
    let _enter = rt.enter();
    
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([733.0, 500.0])
            .with_min_inner_size([500.0, 350.0])
            .with_icon(load_icon()),
        ..Default::default()
    };
    
    eframe::run_native(
        "TUXEDO Control Center",
        options,
        Box::new(|cc| Ok(Box::new(TuxedoApp::new(cc)))),
    )
}

fn load_icon() -> egui::IconData {
    egui::IconData::default()
}
