mod app;
mod dbus_client;
mod theme;
mod pages;
mod keyboard_shortcuts;
mod widgets;
mod scheduler;
mod system_tray;

use app::TuxedoApp;
use tokio::sync::mpsc;
use system_tray::{run_tray_service, TrayCommand, TrayEvent};

fn main() -> Result<(), eframe::Error> {
    env_logger::init();

    // Check for --tray argument
    let args: Vec<String> = std::env::args().collect();
    let start_in_tray = args.contains(&"--tray".to_string());

    // Create and enter a Tokio runtime context.
    let rt = tokio::runtime::Runtime::new().expect("Unable to create a Tokio runtime");
    let _enter = rt.enter();

    // Create channels for tray communication
    let (tray_command_tx, tray_command_rx) = mpsc::unbounded_channel::<TrayCommand>();
    let (tray_event_tx, tray_event_rx) = mpsc::unbounded_channel::<TrayEvent>();

    // We need a separate sender for the tray service itself
    let tray_event_tx_for_service = tray_event_tx.clone();

    tokio::spawn(async move {
        // These are placeholder values. The app will send an update.
        let initial_profiles = Vec::new();
        let initial_profile_name = String::new();

        if let Err(e) = run_tray_service(
            tray_command_rx,
            tray_event_tx_for_service,
            initial_profiles,
            initial_profile_name,
        )
        .await
        {
            log::error!("Tray service failed: {}", e);
        }
    });

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([733.0, 500.0])
            .with_min_inner_size([500.0, 350.0])
            .with_icon(load_icon())
            .with_visible(!start_in_tray),
        ..Default::default()
    };

    eframe::run_native(
        "TUXEDO Control Center",
        options,
        Box::new(move |cc| {
            let app = TuxedoApp::new(cc, tray_event_rx, tray_command_tx);
            Ok(Box::new(app))
        }),
    )
}

fn load_icon() -> egui::IconData {
    egui::IconData::default()
}
