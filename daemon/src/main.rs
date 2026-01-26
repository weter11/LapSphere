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

// Global GPU daemon state
pub static GPU_DAEMON_STATE: once_cell::sync::Lazy<Arc<Mutex<Option<lapsphere_common::types::GpuSettings>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(None)));

pub struct GpuOverclockStats {
    pub freq_offset: i32,
    pub drain_offset: i32,
    pub power_offset: i32,
    pub total_offset: i32,
}

pub static CURRENT_GPU_OVERCLOCK_STATS: once_cell::sync::Lazy<Arc<Mutex<Option<GpuOverclockStats>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(None)));

pub static NVIDIA_SMI_LEGACY_PATH: once_cell::sync::Lazy<Arc<Mutex<Option<String>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(None)));

pub static LAST_APPLIED_OFFSET: once_cell::sync::Lazy<Arc<Mutex<Option<i32>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(None)));

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    log::info!("Starting LapSphere Daemon");

    let args: Vec<String> = std::env::args().collect();

    if args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        println!("LapSphere Daemon - Hardware Control for Clevo/Uniwill Laptops");
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
            println!("--- Hardware Statistics for Clevo/Uniwill Laptops (Limited - non-root) ---");
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
        println!("--- Hardware Statistics for Clevo/Uniwill Laptops ---");
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

    // Add GPU overclocking polling job
    let gpu_poll_fn = || {
        let settings = {
            let state = GPU_DAEMON_STATE.lock().unwrap();
            state.clone()
        };

        if let Some(ref gpu_settings) = settings {
            apply_gpu_overclocking(gpu_settings)?;
        } else {
            {
                let mut stats = CURRENT_GPU_OVERCLOCK_STATS.lock().unwrap();
                *stats = None;
            }
            {
                let mut last = LAST_APPLIED_OFFSET.lock().unwrap();
                *last = None;
            }
        }
        Ok(())
    };

    let gpu_job = PollJob::new(
        "gpu_overclock".to_string(),
        Duration::from_millis(1000), // Default 1s
        gpu_poll_fn,
    );

    if let Err(e) = scheduler_handle.add_job(gpu_job) {
        log::error!("Failed to add GPU overclocking job: {}", e);
    } else {
        log::info!("GPU overclocking polling job added");
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

fn apply_gpu_overclocking(gpu_settings: &lapsphere_common::types::GpuSettings) -> Result<()> {
     // Clear stats and last offset if advanced control or manual clocks are disabled
     if !gpu_settings.advanced_control || !gpu_settings.manual_clocks {
         {
             let mut stats = CURRENT_GPU_OVERCLOCK_STATS.lock().unwrap();
             *stats = None;
         }
         {
             let mut last = LAST_APPLIED_OFFSET.lock().unwrap();
             *last = None;
         }
         if !gpu_settings.manual_clocks {
             let _ = crate::hardware_control::set_gpu_core_offset(0, 0);
         }
         return Ok(());
     }

    // 1. Get current GPU stats (temperature, power, frequency)
    let gpus = crate::hardware_detection::get_gpu_info()?;
    let nvidia_gpu = gpus.iter().find(|g| g.name.to_lowercase().contains("nvidia"));

        if let Some(gpu) = nvidia_gpu {
            let status_lower = gpu.status.to_lowercase();
            let is_suspended = status_lower.contains("suspended");
            let is_pstate = status_lower.starts_with('p');
            // Check if GPU is suspended or not active
            if is_suspended || !is_pstate {
                return Ok(());
            }


        let temp = gpu.temperature.unwrap_or(0.0);
        let power = gpu.power.unwrap_or(0.0);
        let freq = gpu.frequency.unwrap_or(0) as f32;

        let adv = &gpu_settings.advanced;

        // Freq Offset calculation
        let freq_offset = if freq <= adv.frequency_min as f32 {
            adv.freq_offset_max as f32
        } else if freq >= adv.frequency_max as f32 {
            adv.freq_offset_min as f32
        } else {
            let ratio = (freq - adv.frequency_min as f32) / (adv.frequency_max - adv.frequency_min) as f32;
            adv.freq_offset_max as f32 - ratio * (adv.freq_offset_max - adv.freq_offset_min) as f32
        };

        // Drain Offset calculation
        let mut drain_offset = 0.0;
        if adv.drain_offset_control {
            let is_high_freq = freq >= adv.high_freq_min as f32 && freq <= adv.high_freq_max as f32;
            let is_low_freq = freq >= adv.low_freq_min as f32 && freq <= adv.low_freq_max as f32;

            if adv.critical_temp_range_control && temp >= adv.critical_temp_min as f32 && temp <= adv.critical_temp_max as f32 {
                if is_low_freq {
                    drain_offset = adv.drain_offset_lmin as f32;
                } else if is_high_freq {
                    drain_offset = adv.drain_offset_hmin as f32;
                }
            } else if temp > adv.temperature_max as f32 {
                if is_low_freq {
                    drain_offset = adv.drain_offset_lmax as f32;
                } else if is_high_freq {
                    drain_offset = adv.drain_offset_hmin as f32;
                }
            } else {
                let temp_range = (adv.temperature_max - adv.temperature_min) as f32;
                let temp_ratio = if temp_range > 0.0 {
                    ((temp - adv.temperature_min as f32) / temp_range).clamp(0.0, 1.0)
                } else {
                    0.0
                };

                if is_high_freq {
                    // linearly decreased from 'drain_offset_hmax' to 'drain_offset_hmin'
                    drain_offset = adv.drain_offset_hmax as f32 - temp_ratio * (adv.drain_offset_hmax - adv.drain_offset_hmin) as f32;
                } else {
                    // linearly increased from 'drain_offset_lmin' to 'drain_offset_lmax'
                    drain_offset = adv.drain_offset_lmin as f32 + temp_ratio * (adv.drain_offset_lmax - adv.drain_offset_lmin) as f32;
                }
            }
        }

        // Power Offset calculation
        let mut power_offset = 0.0;
        if adv.power_offset_control {
            if power <= adv.plimit_min as f32 {
                power_offset = adv.power_offset_max as f32;
            } else if power >= adv.plimit_max as f32 {
                power_offset = adv.power_offset_min as f32;
            } else {
                let p_range = (adv.plimit_max - adv.plimit_min) as f32;
                let p_ratio = if p_range > 0.0 {
                    (power - adv.plimit_min as f32) / p_range
                } else {
                    0.0
                };
                power_offset = adv.power_offset_max as f32 - p_ratio * (adv.power_offset_max - adv.power_offset_min) as f32;
            }
        }

        // Total Offset
        let total_offset = freq_offset + drain_offset + power_offset;

        if status_lower != "p0" {
            let mut last = LAST_APPLIED_OFFSET.lock().unwrap();
            if *last != Some(0) {
                crate::hardware_control::set_gpu_core_offset(0, 0)?;
                *last = Some(0);
                log::info!("Cleared dynamic GPU offset (P-state not 0)");
            }
            drop(last);
            let mut stats = CURRENT_GPU_OVERCLOCK_STATS.lock().unwrap();
            *stats = None;
            return Ok(());
        }

        let final_offset = {
             // SMART ROUNDING
             let threshold = adv.smart_rounding_threshold as f32;
             if threshold > 0.0 {
                 let multiples = (total_offset / threshold).floor();
                 let remainder = total_offset - (multiples * threshold);
                 if remainder >= (2.0/3.0) * threshold {
                     (multiples + 1.0) * threshold
                 } else {
                     multiples * threshold
                 }
             } else {
                 total_offset
             }
        };

        let final_offset_i32 = final_offset as i32;

        // ONLY APPLY IF CHANGED (fix stuttering)
        {
            let mut last = LAST_APPLIED_OFFSET.lock().unwrap();
            if *last != Some(final_offset_i32) {
                crate::hardware_control::set_gpu_core_offset(0, final_offset_i32)?;
                *last = Some(final_offset_i32);
                if final_offset_i32 == 0 {
                    log::info!("Cleared dynamic GPU offset (P-state not 0)");
                } else {
                    log::info!("Applied new dynamic GPU offset: {} MHz", final_offset_i32);
                }
            }
        }

        // Update global stats for UI
        let mut stats = CURRENT_GPU_OVERCLOCK_STATS.lock().unwrap();
        *stats = Some(GpuOverclockStats {
            freq_offset: freq_offset as i32,
            drain_offset: drain_offset as i32,
            power_offset: power_offset as i32,
            total_offset: final_offset as i32,
        });
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
