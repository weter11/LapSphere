use anyhow::{anyhow, Result};
use std::fs;
use std::path::Path;
use tuxedo_common::types::*;

pub fn set_cpu_governor(governor: &str) -> Result<()> {
    let cpu_count = get_cpu_count()?;
    
    for i in 0..cpu_count {
        let path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_governor", i);
        fs::write(&path, governor)
            .map_err(|e| anyhow!("Failed to set governor for CPU {}: {}", i, e))?;
    }
    
    log::info!("Set CPU governor to: {}", governor);
    Ok(())
}

pub fn set_cpu_frequency_limits(min_freq: u64, max_freq: u64) -> Result<()> {
    let min_path = "/sys/devices/system/cpu/cpu0/cpufreq/scaling_min_freq";
    let max_path = "/sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq";
    
    if !Path::new(min_path).exists() || !Path::new(max_path).exists() {
        return Err(anyhow!("Frequency control not available (check AMD pstate status)"));
    }
    
    let cpu_count = get_cpu_count()?;
    
    for i in 0..cpu_count {
        let min_path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_min_freq", i);
        let max_path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_max_freq", i);
        
        fs::write(&min_path, min_freq.to_string())
            .map_err(|e| anyhow!("Failed to set min frequency for CPU {}: {}", i, e))?;
        
        fs::write(&max_path, max_freq.to_string())
            .map_err(|e| anyhow!("Failed to set max frequency for CPU {}: {}", i, e))?;
    }
    
    log::info!("Set CPU frequency limits: {} - {} kHz", min_freq, max_freq);
    Ok(())
}

pub fn set_cpu_boost(enabled: bool) -> Result<()> {
    // Try AMD boost
    let amd_path = "/sys/devices/system/cpu/cpufreq/boost";
    if Path::new(amd_path).exists() {
        fs::write(amd_path, if enabled { "1" } else { "0" })?;
        log::info!("Set AMD CPU boost to: {}", enabled);
        return Ok(());
    }
    
    // Try Intel turbo (inverted logic)
    let intel_path = "/sys/devices/system/cpu/intel_pstate/no_turbo";
    if Path::new(intel_path).exists() {
        fs::write(intel_path, if enabled { "0" } else { "1" })?;
        log::info!("Set Intel CPU turbo to: {}", enabled);
        return Ok(());
    }
    
    Err(anyhow!("Boost control not available"))
}

pub fn set_smt(enabled: bool) -> Result<()> {
    let path = "/sys/devices/system/cpu/smt/control";
    if !Path::new(path).exists() {
        return Err(anyhow!("SMT control not available"));
    }
    
    fs::write(path, if enabled { "on" } else { "off" })?;
    log::info!("Set SMT to: {}", if enabled { "on" } else { "off" });
    Ok(())
}

pub fn set_amd_pstate_status(status: &str) -> Result<()> {
    let path = "/sys/devices/system/cpu/amd_pstate/status";
    if !Path::new(path).exists() {
        return Err(anyhow!("AMD pstate not available"));
    }
    
    // Validate status
    if !["passive", "active", "guided"].contains(&status) {
        return Err(anyhow!("Invalid AMD pstate status: {}", status));
    }
    
    fs::write(path, status)?;
    log::info!("Set AMD pstate status to: {}", status);
    Ok(())
}

pub fn set_energy_performance_preference(preference: &str) -> Result<()> {
    let cpu_count = get_cpu_count()?;
    
    // Validate that EPP is available
    let test_path = "/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference";
    if !Path::new(test_path).exists() {
        return Err(anyhow!("Energy Performance Preference not available (requires AMD pstate active/passive mode)"));
    }
    
    for i in 0..cpu_count {
        let path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/energy_performance_preference", i);
        fs::write(&path, preference)
            .map_err(|e| anyhow!("Failed to set EPP for CPU {}: {}", i, e))?;
    }
    
    log::info!("Set Energy Performance Preference to: {}", preference);
    Ok(())
}

pub fn apply_profile(profile: &Profile) -> Result<()> {
    log::info!("Applying profile: {}", profile.name);
    
    // Apply AMD pstate first (if changed, it affects available controls)
    if let Some(ref amd_status) = profile.cpu_settings.amd_pstate_status {
        set_amd_pstate_status(amd_status)?;
    }
    
    // Apply CPU settings
    if let Some(ref governor) = profile.cpu_settings.governor {
        set_cpu_governor(governor)?;
    }
    
    if let (Some(min), Some(max)) = (profile.cpu_settings.min_frequency, profile.cpu_settings.max_frequency) {
        set_cpu_frequency_limits(min, max)?;
    }
    
    if let Some(boost) = profile.cpu_settings.boost {
        set_cpu_boost(boost)?;
    }
    
    if let Some(smt) = profile.cpu_settings.smt {
        set_smt(smt)?;
    }
    
    if let Some(ref epp) = profile.cpu_settings.energy_performance_preference {
        set_energy_performance_preference(epp)?;
    }
    
    // Apply keyboard settings
    apply_keyboard_settings(&profile.keyboard_settings)?;
    
    // Apply screen settings
    apply_screen_settings(&profile.screen_settings)?;
    
    // Apply fan settings
    apply_fan_settings(&profile.fan_settings)?;
    
    log::info!("Profile '{}' applied successfully", profile.name);
    Ok(())
}

fn apply_keyboard_settings(settings: &KeyboardSettings) -> Result<()> {
    if !settings.control_enabled {
        return Ok(());
    }
    
    match &settings.mode {
        KeyboardMode::SingleColor { r, g, b, brightness } => {
            // Try both possible paths
            let paths = [
                "/sys/class/leds/rgb:kbd_backlight",
                "/sys/devices/platform/tuxedo_keyboard/leds/rgb:kbd_backlight",
            ];
            
            let mut success = false;
            for base_path in &paths {
                let color_path = format!("{}/multi_intensity", base_path);
                let brightness_path = format!("{}/brightness", base_path);
                
                if Path::new(&color_path).exists() {
                    fs::write(&color_path, format!("{} {} {}", r, g, b))?;
                    
                    // Read max brightness
                    let max_brightness_path = format!("{}/max_brightness", base_path);
                    let max_brightness: u32 = fs::read_to_string(&max_brightness_path)?
                        .trim()
                        .parse()
                        .unwrap_or(255);
                    
                    let actual_brightness = ((*brightness as u32) * max_brightness) / 100;
                    fs::write(&brightness_path, actual_brightness.to_string())?;
                    
                    success = true;
                    log::info!("Set keyboard: RGB({}, {}, {}), brightness {}%", r, g, b, brightness);
                    break;
                }
            }
            
            if !success {
                log::warn!("No keyboard backlight control found");
            }
        }
        KeyboardMode::Effect { effect, speed } => {
            log::info!("Keyboard effect mode not yet implemented: {} at speed {}", effect, speed);
        }
    }
    
    Ok(())
}

fn apply_screen_settings(settings: &ScreenSettings) -> Result<()> {
    if settings.system_control {
        return Ok(());
    }
    
    let backlight_paths = [
        "/sys/class/backlight/intel_backlight",
        "/sys/class/backlight/amdgpu_bl0",
        "/sys/class/backlight/acpi_video0",
    ];
    
    for base_path in &backlight_paths {
        let brightness_path = format!("{}/brightness", base_path);
        let max_brightness_path = format!("{}/max_brightness", base_path);
        
        if Path::new(&brightness_path).exists() {
            let max_brightness: u32 = fs::read_to_string(&max_brightness_path)?
                .trim()
                .parse()
                .unwrap_or(255);
            
            let actual_brightness = ((settings.brightness as u32) * max_brightness) / 100;
            fs::write(&brightness_path, actual_brightness.to_string())?;
            
            log::info!("Set screen brightness to {}%", settings.brightness);
            return Ok(());
        }
    }
    
    log::warn!("No backlight control found");
    Ok(())
}

fn apply_fan_settings(settings: &FanSettings) -> Result<()> {
    let tuxedo_io_path = "/sys/devices/platform/tuxedo_io";
    
    if !Path::new(tuxedo_io_path).exists() {
        log::warn!("tuxedo_io not available, skipping fan settings");
        return Ok(());
    }
    
    if !settings.control_enabled {
        // Set to auto mode
        let mode_path = format!("{}/fan_mode", tuxedo_io_path);
        if Path::new(&mode_path).exists() {
            fs::write(&mode_path, "auto")?;
            log::info!("Set fans to auto mode");
        }
        return Ok(());
    }
    
    // Set to manual mode
    let mode_path = format!("{}/fan_mode", tuxedo_io_path);
    if Path::new(&mode_path).exists() {
        fs::write(&mode_path, "manual")?;
    }
    
    // Apply fan curves
    for curve in &settings.curves {
        for (idx, (temp, speed)) in curve.points.iter().enumerate() {
            let temp_path = format!("{}/fan{}_temp{}", tuxedo_io_path, curve.fan_id, idx);
            let speed_path = format!("{}/fan{}_speed{}", tuxedo_io_path, curve.fan_id, idx);
            
            if Path::new(&temp_path).exists() {
                fs::write(&temp_path, temp.to_string())?;
            }
            
            if Path::new(&speed_path).exists() {
                fs::write(&speed_path, speed.to_string())?;
            }
        }
        
        log::info!("Applied fan curve for fan {}", curve.fan_id);
    }
    
    Ok(())
}

fn get_cpu_count() -> Result<u32> {
    let mut count = 0;
    for i in 0..1024 {
        let path = format!("/sys/devices/system/cpu/cpu{}", i);
        if !Path::new(&path).exists() {
            break;
        }
        count += 1;
    }
    Ok(count)
}
