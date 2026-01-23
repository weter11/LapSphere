mod app;
mod dbus_client;
mod theme;
mod pages;
mod keyboard_shortcuts;
mod widgets;
mod polling_scheduler;
mod system_tray;

use app::LapSphereApp;

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
        "LapSphere",
        options,
        Box::new(|cc| Ok(Box::new(LapSphereApp::new(cc)))),
    )
}

fn load_icon() -> egui::IconData {
    let width = 32;
    let height = 32;
    let mut rgba = vec![0u8; (width * height * 4) as usize];

    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;

            // Gaming-themed "L" icon: neon green on dark background
            let is_l_vertical = x >= 10 && x <= 14 && y >= 6 && y <= 26;
            let is_l_horizontal = x >= 10 && x <= 22 && y >= 22 && y <= 26;

            if is_l_vertical || is_l_horizontal {
                rgba[idx] = 0;     // R
                rgba[idx + 1] = 255; // G
                rgba[idx + 2] = 0;   // B
                rgba[idx + 3] = 255; // A
            } else {
                rgba[idx] = 26;
                rgba[idx + 1] = 26;
                rgba[idx + 2] = 26;
                rgba[idx + 3] = 255;
            }
        }
    }

    egui::IconData {
        rgba,
        width,
        height,
    }
}
