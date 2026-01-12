mod dbus_interface;
mod fan_daemon;
mod hardware_control;
mod hardware_detection;

use anyhow::Result;
use tokio::signal;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::time::Duration;
use tokio::time;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    log::info!("Starting TUXEDO Control Center Daemon");

    // Check if running as root
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("Error: Daemon must run as root");
        std::process::exit(1);
    }

    // Start DBus service
    let connection = zbus::Connection::system().await?;
    let _service = dbus_interface::start_service(connection.clone()).await?;

    log::info!("DBus service started");
    
    // Start process monitoring if enabled
    // Note: Would load config from file in production
    let app_monitoring_enabled = false; // Load from config
    
    if app_monitoring_enabled {
        log::info!("Starting process monitor");
        let connection_clone = connection.clone();
        tokio::spawn(async move {
            if let Err(e) = run_process_monitor(connection_clone).await {
                log::error!("Process monitor error: {}", e);
            }
        });
    }

    // Wait for shutdown signal
    signal::ctrl_c().await?;
    log::info!("Shutting down daemon");

    Ok(())
}

// Process monitoring implementation (integrated directly)
async fn run_process_monitor(connection: zbus::Connection) -> Result<()> {
    let mut interval = time::interval(Duration::from_secs(5));
    let mut current_profile = "Default".to_string();
    let mut process_cache: HashSet<String> = HashSet::new();
    
    // Profile -> app mapping (would load from config)
    let mut profile_apps: HashMap<String, Vec<String>> = HashMap::new();
    profile_apps.insert(
        "Gaming".to_string(),
        vec!["steam".to_string(), "wine".to_string(), "lutris".to_string()],
    );
    profile_apps.insert(
        "Performance".to_string(),
        vec!["blender".to_string(), "gimp".to_string(), "kdenlive".to_string()],
    );
    
    log::info!("Process monitor started with {} profiles", profile_apps.len());
    
    loop {
        interval.tick().await;
        
        // Get current processes
        match get_running_processes() {
            Ok(processes) => {
                let process_names: HashSet<String> = processes
                    .iter()
                    .map(|p| p.clone())
                    .collect();
                
                // Only check if cache changed
                if process_names != process_cache {
                    process_cache = process_names.clone();
                    
                    // Determine target profile
                    if let Some(target_profile) = determine_target_profile(&process_names, &profile_apps) {
                        if target_profile != current_profile {
                            log::info!(
                                "Auto-switching profile: '{}' -> '{}'", 
                                current_profile, 
                                target_profile
                            );
                            current_profile = target_profile.clone();
                            
                            // Apply profile via DBus (would need to call the interface)
                            // This is a simplified example
                        }
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to get processes: {}", e);
            }
        }
    }
}

fn get_running_processes() -> Result<Vec<String>> {
    let mut process_names = Vec::new();
    
    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        let path = entry.path();
        
        if let Some(filename) = path.file_name() {
            if let Some(filename_str) = filename.to_str() {
                if filename_str.parse::<u32>().is_ok() {
                    let comm_path = path.join("comm");
                    if let Ok(name) = fs::read_to_string(&comm_path) {
                        process_names.push(name.trim().to_string());
                    }
                }
            }
        }
    }
    
    Ok(process_names)
}

fn determine_target_profile(
    running_processes: &HashSet<String>,
    profile_apps: &HashMap<String, Vec<String>>,
) -> Option<String> {
    // Priority order
    let priority_profiles = vec![
        ("Gaming", 100),
        ("Performance", 90),
        ("Balanced", 50),
        ("Power Save", 10),
    ];
    
    let mut best_match: Option<(String, i32)> = None;
    
    for (profile_name, app_list) in profile_apps {
        for app_name in app_list {
            if running_processes.contains(app_name) 
                || fuzzy_match_process(app_name, running_processes) {
                
                let priority = priority_profiles
                    .iter()
                    .find(|(name, _)| name == profile_name)
                    .map(|(_, p)| *p)
                    .unwrap_or(50);
                
                if let Some((_, current_priority)) = best_match {
                    if priority > current_priority {
                        best_match = Some((profile_name.clone(), priority));
                    }
                } else {
                    best_match = Some((profile_name.clone(), priority));
                }
                
                log::debug!("Detected '{}' for profile '{}'", app_name, profile_name);
            }
        }
    }
    
    best_match.map(|(profile, _)| profile)
}

fn fuzzy_match_process(app_name: &str, running_processes: &HashSet<String>) -> bool {
    let app_lower = app_name.to_lowercase();
    
    for process in running_processes {
        let process_lower = process.to_lowercase();
        
        if process_lower == app_lower {
            return true;
        }
        
        if process_lower.contains(&app_lower) || app_lower.contains(&process_lower) {
            return true;
        }
    }
    
    false
}
