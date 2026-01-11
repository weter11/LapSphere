mod dbus_interface;
mod fan_daemon;
mod hardware_control;
mod hardware_detection;
mod tuxedo_io;
mod battery_control;

use anyhow::Result;
use tokio::signal;
use std::sync::{Arc, Mutex};
use tuxedo_common::types::FanSettings;

// Global fan daemon state
pub static FAN_DAEMON_STATE: once_cell::sync::Lazy<Arc<Mutex<Option<FanSettings>>>> = 
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(None)));

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    log::info!("Starting TUXEDO Control Center Daemon");

    // Check if running as root
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("Error: Daemon must run as root");
        std::process::exit(1);
    }

    // Initialize hardware interfaces
    let tuxedo_io = if tuxedo_io::TuxedoIo::is_available() {
        match tuxedo_io::TuxedoIo::new() {
            Ok(io) => {
                let interface = match io.get_interface() {
                    tuxedo_io::HardwareInterface::Clevo => "Clevo",
                    tuxedo_io::HardwareInterface::Uniwill => "Uniwill",
                    tuxedo_io::HardwareInterface::None => "None",
                };
                log::info!("Detected hardware interface: {}", interface);
                log::info!("Number of fans: {}", io.get_fan_count());
                Some(io)
            }
            Err(e) => {
                log::warn!("Failed to initialize tuxedo_io: {}", e);
                None
            }
        }
    } else {
        log::warn!("/dev/tuxedo_io not available - some features will be disabled");
        None
    };

    // Check battery charge control
    if battery_control::BatteryControl::is_available() {
        log::info!("Battery charge control (flexicharger) is available");
    } else {
        log::info!("Battery charge control not available");
    }

    // Start fan daemon in background
    if let Some(io) = tuxedo_io {
        let fan_io = Arc::new(io);
        tokio::spawn(async move {
            fan_daemon_task(fan_io).await;
        });
    }

    // Start DBus service
    let connection = zbus::Connection::system().await?;
    let _service = dbus_interface::start_service(connection.clone()).await?;

    log::info!("DBus service started");

    // Wait for shutdown signal
    signal::ctrl_c().await?;
    log::info!("Shutting down daemon");

    Ok(())
}

async fn fan_daemon_task(io: Arc<tuxedo_io::TuxedoIo>) {
    log::info!("Starting fan control daemon");
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(2));
    let mut last_settings: Option<FanSettings> = None;
    let mut sorted_curves: Vec<Vec<(u8, u8)>> = Vec::new();

    loop {
        interval.tick().await;

        let settings = {
            let state = FAN_DAEMON_STATE.lock().unwrap();
            state.clone()
        };

        if settings != last_settings {
            if let Some(ref s) = settings {
                sorted_curves = s.curves.iter().map(|c| {
                    let mut points = c.points.clone();
                    points.sort_by_key(|p| p.0);
                    points
                }).collect();
            }
            last_settings = settings;
        }

        if let Some(ref fan_settings) = last_settings {
            if fan_settings.control_enabled {
                if let Err(e) = apply_fan_curves(&io, fan_settings, &sorted_curves) {
                    log::error!("Failed to apply fan curves: {}", e);
                }
            }
        }
    }
}

fn apply_fan_curves(io: &tuxedo_io::TuxedoIo, settings: &FanSettings, sorted_curves: &[Vec<(u8, u8)>]) -> Result<()> {
    for (i, curve) in settings.curves.iter().enumerate() {
        if curve.fan_id >= io.get_fan_count() {
            continue;
        }
        
        let temp = match io.get_fan_temperature(curve.fan_id) {
            Ok(t) => t as f32,
            Err(e) => {
                log::warn!("Failed to read fan {} temperature: {}", curve.fan_id, e);
                continue;
            }
        };
        
        let speed = calculate_fan_speed(&sorted_curves[i], temp);
        
        if let Err(e) = io.set_fan_speed(curve.fan_id, speed as u32) {
            log::error!("Failed to set fan {} speed: {}", curve.fan_id, e);
        } else {
            log::debug!("Fan {}: temp={}°C, speed={}%", curve.fan_id, temp, speed);
        }
    }
    
    Ok(())
}

fn calculate_fan_speed(sorted_points: &[(u8, u8)], temp: f32) -> u8 {
    if sorted_points.is_empty() {
        return 50; // Default fallback
    }
    
    if sorted_points.len() == 1 {
        return sorted_points[0].1;
    }
    
    if temp <= sorted_points[0].0 as f32 {
        return sorted_points[0].1;
    }
    
    if temp >= sorted_points[sorted_points.len() - 1].0 as f32 {
        return sorted_points[sorted_points.len() - 1].1;
    }
    
    for i in 0..sorted_points.len() - 1 {
        let (temp1, speed1) = sorted_points[i];
        let (temp2, speed2) = sorted_points[i + 1];
        
        if temp >= temp1 as f32 && temp <= temp2 as f32 {
            let ratio = (temp - temp1 as f32) / (temp2 as f32 - temp1 as f32);
            let speed = speed1 as f32 + ratio * (speed2 as f32 - speed1 as f32);
            return speed.round() as u8;
        }
    }
    
    50 // Fallback
}
