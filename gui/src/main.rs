mod app;
mod dbus_client;
mod theme;
mod pages;
mod keyboard_shortcuts;
mod widgets;
mod system_tray;

use app::TuxedoApp;
use std::sync::mpsc;

fn main() -> Result<(), eframe::Error> {
    env_logger::init();

    // Create and enter a Tokio runtime context.
    // This is required for `tokio::spawn` to work in the `DbusClient`.
    let rt = tokio::runtime::Runtime::new().expect("Unable to create a Tokio runtime");
    let _enter = rt.enter();
    
    let args: Vec<String> = std::env::args().collect();
    let start_in_tray = args.contains(&"--tray".to_string());

    let (tray_tx, tray_rx) = mpsc::channel();

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
        Box::new(|cc| {
            let app = TuxedoApp::new(cc, tray_rx);
            let mut tray = system_tray::SystemTray::new(&app.state.config.lock().profiles, &app.state.current_profile_name()).unwrap();
            let egui_ctx = cc.egui_ctx.clone();

            std::thread::spawn(move || {
                loop {
                    match tray.rx.recv() {
                        Ok(system_tray::TrayEvent::ShowWindow) => {
                            tray_tx.send(system_tray::TrayEvent::ShowWindow).unwrap();
                            egui_ctx.request_repaint();
                        }
                        Ok(system_tray::TrayEvent::Quit) => {
                            tray_tx.send(system_tray::TrayEvent::Quit).unwrap();
                            egui_ctx.request_repaint();
                        }
                        Ok(system_tray::TrayEvent::SwitchProfile(p)) => {
                            tray_tx.send(system_tray::TrayEvent::SwitchProfile(p)).unwrap();
                            egui_ctx.request_repaint();
                        }
                        Err(_) => {}
                    }
                }
            });

            Ok(Box::new(app))
        }),
    )
}

fn load_icon() -> egui::IconData {
    egui::IconData::default()
}
