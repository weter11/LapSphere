mod dbus_interface;
mod hardware_control;
mod hardware_detection;
mod tuxedo_io;
mod battery_control;
mod polling_scheduler;

use anyhow::Result;
use tokio::signal;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use lapsphere_common::types::FanSettings;
use polling_scheduler::{PollingScheduler, PollJob};

// Global fan daemon state
pub static FAN_DAEMON_STATE: once_cell::sync::Lazy<Arc<Mutex<Option<FanSettings>>>> = 
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(None)));

// Global polling scheduler handle
pub static SCHEDULER_HANDLE: once_cell::sync::OnceCell<polling_scheduler::SchedulerHandle> = 
    once_cell::sync::OnceCell::new();

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    log::info!("Starting LapSphere Daemon");

    let args: Vec<String> = std::env::args().collect();

    if args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        println!("LapSphere Daemon - Hardware Control for Laptops");
        println!("\nUsage: lapsphere-daemon [OPTIONS]");
        println!("\nOptions:");
        println!("  --gui       Launch the graphical user interface");
        println!("  --tray      Start minimized to system tray");
        println!("  --help, -h  Show this help message");
        println!("  --debug     Enable debug logging");
        println!("\nConfiguration:");
        println!("  Settings are stored in ~/.config/lapsphere/settings.json");
        println!("  Profiles are stored in ~/.config/lapsphere/profiles.json");
        println!("\nTo configure via CLI, you can edit these JSON files directly.");
        return Ok(());
    }

    let launch_gui = args.contains(&"--gui".to_string());
    let launch_tray = args.contains(&"--tray".to_string());

    // Collect other arguments to pass to the GUI
    let gui_args: Vec<String> = args.iter()
        .skip(1)
        .filter(|&a| a != "--gui")
        .cloned()
        .collect();

    // Check if running as root
    if unsafe { libc::geteuid() } != 0 {
        if !launch_gui && !launch_tray {
            println!("--- LapSphere Hardware Statistics (Limited - non-root) ---");
            match hardware_detection::get_cpu_info() {
                Ok(cpu) => {
                    println!("CPU: {}", cpu.name);
                    println!("  Load: {:.1}%", cpu.median_load);
                    println!("  Temp: {:.1}°C", cpu.package_temp);
                }
                Err(e) => println!("Error getting CPU info: {}", e),
            }
            match hardware_detection::get_memory_info() {
                Ok(mem) => {
                    println!("Memory: {:.1} / {:.1} GiB ({:.1}%)",
                        mem.used_gib, mem.total_gib, mem.used_percent);
                }
                Err(e) => println!("Error getting memory info: {}", e),
            }
            println!("\nError: Full daemon functionality requires root privileges.");
        } else {
            eprintln!("Error: Daemon must run as root to support GUI or tray modes.");
        }
        std::process::exit(1);
    }

    if !launch_gui && !launch_tray {
        println!("--- LapSphere Hardware Statistics ---");
        match hardware_detection::get_cpu_info() {
            Ok(cpu) => {
                println!("CPU: {}", cpu.name);
                println!("  Load: {:.1}%", cpu.median_load);
                println!("  Temp: {:.1}°C", cpu.package_temp);
                if let Some(power) = cpu.package_power {
                    println!("  Power: {:.1}W", power);
                }
            }
            Err(e) => println!("Error getting CPU info: {}", e),
        }

        match hardware_detection::get_memory_info() {
            Ok(mem) => {
                println!("Memory: {:.1} / {:.1} GiB ({:.1}%)",
                    mem.used_gib, mem.total_gib, mem.used_percent);
            }
            Err(e) => println!("Error getting memory info: {}", e),
        }

        if let Ok(gpus) = hardware_detection::get_gpu_info() {
            for gpu in gpus {
                println!("GPU: {}", gpu.name);
                if let Some(load) = gpu.load { println!("  Load: {:.1}%", load); }
                if let Some(temp) = gpu.temperature { println!("  Temp: {:.1}°C", temp); }
            }
        }

        println!("\nDaemon is running. Press Ctrl+C to exit.");
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

    // Create and start polling scheduler
    let scheduler = PollingScheduler::new();
    let scheduler_handle = scheduler.get_handle();
    
    // Store handle globally for DBus interface to use
    SCHEDULER_HANDLE.set(scheduler_handle.clone()).ok();
    
    // Start scheduler in background
    tokio::spawn(async move {
        scheduler.run().await;
    });

    // Add fan control polling job if hardware is available
    if let Some(io) = tuxedo_io {
        let fan_io = Arc::new(io);
        let poll_fn = {
            let fan_io = fan_io.clone();
            move || {
                let settings = {
                    let state = FAN_DAEMON_STATE.lock().unwrap();
                    state.clone()
                };

                if let Some(ref fan_settings) = settings {
                    if fan_settings.control_enabled {
                        // Sort curves for each fan
                        let sorted_curves: Vec<Vec<(u8, u8)>> = fan_settings.curves.iter().map(|c| {
                            let mut points = c.points.clone();
                            points.sort_by_key(|p| p.0);
                            points
                        }).collect();

                        apply_fan_curves(&fan_io, fan_settings, &sorted_curves)?;
                    }
                }
                Ok(())
            }
        };

        let fan_job = PollJob::new(
            "fan_control".to_string(),
            Duration::from_secs(2),
            poll_fn,
        );

        if let Err(e) = scheduler_handle.add_job(fan_job) {
            log::error!("Failed to add fan control job: {}", e);
        } else {
            log::info!("Fan control polling job added");
        }
    }

    // Start DBus service
    let connection = zbus::Connection::system().await?;
    let connection_clone = connection.clone();
    tokio::spawn(async move {
        if let Err(e) = dbus_interface::start_service(connection_clone).await {
            log::error!("Failed to start DBus service: {}", e);
        }
    });

    log::info!("DBus service started");

    // Launch GUI if requested
    if launch_gui {
        use tokio::process::Command;

        let target_uid = std::env::var("SUDO_UID")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .or_else(|| std::env::var("PKEXEC_UID").ok().and_then(|v| v.parse::<u32>().ok()));

        if let Some(uid) = target_uid {
            log::info!("Launching GUI as user UID {}", uid);

            // Try to find the binary in the same directory as the daemon or in PATH
            let current_exe = std::env::current_exe().ok();
            let gui_bin_path = current_exe.and_then(|p| p.parent().map(|parent| parent.join("lapsphere")));

            let mut gui_cmd = if let Some(ref path) = gui_bin_path.filter(|p| p.exists()) {
                Command::new(path)
            } else {
                Command::new("lapsphere")
            };
            gui_cmd.args(&gui_args);
            gui_cmd.uid(uid);

            // Inherit environment variables that might be needed for X11/Wayland
            for var in &[
                "DISPLAY",
                "XAUTHORITY",
                "WAYLAND_DISPLAY",
                "DBUS_SESSION_BUS_ADDRESS",
                "XDG_RUNTIME_DIR",
                "XDG_SESSION_TYPE",
                "XDG_CURRENT_DESKTOP",
                "GDK_BACKEND",
                "QT_QPA_PLATFORM",
            ] {
                if let Ok(val) = std::env::var(var) {
                    gui_cmd.env(var, val);
                }
            }

            match gui_cmd.spawn() {
                Ok(mut child) => {
                    tokio::spawn(async move {
                        match child.wait().await {
                            Ok(status) => {
                                log::info!("GUI exited with status: {}, shutting down daemon", status);
                            }
                            Err(e) => {
                                log::error!("Error waiting for GUI: {}, shutting down daemon", e);
                            }
                        }
                        let _ = nix::sys::signal::raise(nix::sys::signal::Signal::SIGINT);
                    });
                }
                Err(e) => {
                    log::error!("Failed to launch GUI: {}", e);
                    // If we failed to launch requested GUI, should we exit?
                    // User said "if user exit gui, then stop daemon", so if it never starts...
                    let _ = nix::sys::signal::raise(nix::sys::signal::Signal::SIGINT);
                }
            }
        } else {
            log::warn!("--gui flag passed but couldn't determine target UID (SUDO_UID or PKEXEC_UID not set)");
            // If we are root but not via sudo/pkexec, we probably shouldn't launch GUI as root
        }
    }

    // Wait for shutdown signal
    signal::ctrl_c().await?;
    log::info!("Shutting down daemon");

    Ok(())
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
