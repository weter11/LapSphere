use anyhow::{anyhow, Result};
use std::fs;
use std::path::Path;
use tuxedo_common::types::*;
use crate::tuxedo_io::TuxedoIo;

fn get_cpu_count() -> Result<u32> {
    let cpuinfo = fs::read_to_string("/proc/cpuinfo")?;
    let count = cpuinfo.lines()
        .filter(|line| line.starts_with("processor"))
        .count();
    Ok(count as u32)
}

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
    let cpu_count = get_cpu_count()?;
    
    // IMPORTANT: Set max first, then min to avoid conflicts
    // If current min > new max, setting max first will fail
    // If current max < new min, setting min first will fail
    
    // First, read current values
    let current_min = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_min_freq")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(min_freq);
    
    let current_max = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(max_freq);
    
    for i in 0..cpu_count {
        let min_path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_min_freq", i);
        let max_path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_max_freq", i);
        
        // Determine order based on current vs new values
        if max_freq < current_max || min_freq > current_min {
            // Set max first
            fs::write(&max_path, max_freq.to_string())
                .map_err(|e| anyhow!("Failed to set max frequency for CPU {}: {}", i, e))?;
            fs::write(&min_path, min_freq.to_string())
                .map_err(|e| anyhow!("Failed to set min frequency for CPU {}: {}", i, e))?;
        } else {
            // Set min first
            fs::write(&min_path, min_freq.to_string())
                .map_err(|e| anyhow!("Failed to set min frequency for CPU {}: {}", i, e))?;
            fs::write(&max_path, max_freq.to_string())
                .map_err(|e| anyhow!("Failed to set max frequency for CPU {}: {}", i, e))?;
        }
    }
    
    log::info!("Set CPU frequency limits: {} - {} kHz", min_freq, max_freq);
    Ok(())
}

pub fn set_cpu_boost(enabled: bool) -> Result<()> {
    // AMD cpufreq boost
    let amd_path = "/sys/devices/system/cpu/cpufreq/boost";
    if Path::new(amd_path).exists() {
        fs::write(amd_path, if enabled { "1" } else { "0" })?;
        log::info!("Set AMD CPU boost to: {}", enabled);
        return Ok(());
    }
    
    // Intel turbo
    let intel_path = "/sys/devices/system/cpu/intel_pstate/no_turbo";
    if Path::new(intel_path).exists() {
        fs::write(intel_path, if enabled { "0" } else { "1" })?;
        log::info!("Set Intel CPU turbo to: {}", enabled);
        return Ok(());
    }
    
    // AMD P-State boost (if using amd-pstate driver)
    let amd_pstate_boost = "/sys/devices/system/cpu/amd_pstate/cpb_boost";
    if Path::new(amd_pstate_boost).exists() {
        fs::write(amd_pstate_boost, if enabled { "1" } else { "0" })?;
        log::info!("Set AMD P-State boost to: {}", enabled);
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
    
    if !["passive", "active", "guided"].contains(&status) {
        return Err(anyhow!("Invalid AMD pstate status: {}", status));
    }
    
    fs::write(path, status)?;
    log::info!("Set AMD pstate status to: {}", status);
    Ok(())
}

pub fn apply_profile(profile: &Profile) -> Result<()> {
    log::info!("Applying profile: {}", profile.name);
    
    // Apply CPU settings
    if let Some(ref governor) = profile.cpu_settings.governor {
        set_cpu_governor(governor)?;
    }
    
    if let Some(ref tdp_profile) = profile.cpu_settings.tdp_profile {
        set_tdp_profile(tdp_profile)?;
    }
    
    if let Some(ref amd_status) = profile.cpu_settings.amd_pstate_status {
        set_amd_pstate_status(amd_status)?;
    }
    
    if let Some(ref epp) = profile.cpu_settings.energy_performance_preference {
        set_energy_performance_preference(epp)?;
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
    
    // Apply keyboard settings
    apply_keyboard_settings(&profile.keyboard_settings)?;
    
    // Apply screen settings
    apply_screen_settings(&profile.screen_settings)?;
    
    // Apply fan settings - update daemon state
    apply_fan_settings(&profile.fan_settings)?;
    
    log::info!("Profile '{}' applied successfully", profile.name);
    Ok(())
}

pub fn apply_battery_settings(settings: &BatterySettings) -> Result<()> {
    if !crate::battery_control::BatteryControl::is_available() {
        log::info!("Battery control not available, skipping");
        return Ok(());
    }

    let battery = crate::battery_control::BatteryControl::new()?;

    if settings.control_enabled {
        battery.set_charge_type("Custom")?;
        battery.set_charge_control_start_threshold(settings.charge_start_threshold)?;
        battery.set_charge_control_end_threshold(settings.charge_end_threshold)?;
        log::info!(
            "Set battery thresholds: start={}, end={}",
            settings.charge_start_threshold,
            settings.charge_end_threshold
        );
    } else {
        battery.set_charge_type("Standard")?;
        log::info!("Set battery charge type to Standard");
    }

    Ok(())
}

fn apply_keyboard_settings(settings: &KeyboardSettings) -> Result<()> {
    if !settings.control_enabled {
        log::info!("Keyboard control disabled, skipping");
        return Ok(());
    }
    
    let base_path = find_keyboard_backlight_path()
        .ok_or_else(|| anyhow!("Keyboard backlight not found"))?;
    
    use tuxedo_common::types::KeyboardMode;
    match &settings.mode {
        KeyboardMode::SingleColor { r, g, b, brightness } => {
            log::info!("Applying keyboard: RGB({}, {}, {}) brightness {}%", r, g, b, brightness);
            
            let color_path = format!("{}/multi_intensity", base_path);
            if Path::new(&color_path).exists() {
                let color_str = format!("{} {} {}", r, g, b);
                log::info!("Writing to {}: {}", color_path, color_str);
                fs::write(&color_path, color_str)?;
            } else {
                log::warn!("multi_intensity not found at {}", color_path);
            }
            
            let brightness_path = format!("{}/brightness", base_path);
            if Path::new(&brightness_path).exists() {
                let max_brightness_path = format!("{}/max_brightness", base_path);
                let max_brightness: u32 = if let Ok(max_str) = fs::read_to_string(&max_brightness_path) {
                    max_str.trim().parse().unwrap_or(255)
                } else {
                    255
                };
                
                let actual_brightness = ((*brightness as u32) * max_brightness) / 100;
                
                log::info!("Writing to {}: {} ({}% of {} max)", 
                    brightness_path, actual_brightness, brightness, max_brightness);
                
                fs::write(&brightness_path, actual_brightness.to_string())?;
            } else {
                log::warn!("brightness not found at {}", brightness_path);
            }
            
            log::info!("✅ Keyboard backlight applied successfully");
        }
        _ => {
            if let Ok(kbd) = RgbKeyboardControl::new() {
                kbd.set_mode(&settings.mode)?;
                log::info!("✅ Keyboard effect mode applied successfully");
            } else {
                log::warn!("RGB keyboard control not available for effect modes");
            }
        }
    }
    
    Ok(())
}

pub fn preview_keyboard_settings(settings: &KeyboardSettings) -> Result<()> {
    let base_path = find_keyboard_backlight_path()
        .ok_or_else(|| anyhow!("Keyboard backlight not found"))?;
    
    use tuxedo_common::types::KeyboardMode;
    match &settings.mode {
        KeyboardMode::SingleColor { r, g, b, brightness } => {
            let color_path = format!("{}/multi_intensity", base_path);
            if Path::new(&color_path).exists() {
                let color_str = format!("{} {} {}", r, g, b);
                fs::write(&color_path, color_str)?;
            }
            
            let brightness_path = format!("{}/brightness", base_path);
            if Path::new(&brightness_path).exists() {
                let max_brightness_path = format!("{}/max_brightness", base_path);
                let max_brightness: u32 = if let Ok(max_str) = fs::read_to_string(&max_brightness_path) {
                    max_str.trim().parse().unwrap_or(255)
                } else {
                    255
                };
                
                let actual_brightness = ((*brightness as u32) * max_brightness) / 100;
                fs::write(&brightness_path, actual_brightness.to_string())?;
            }
        }
        _ => {
            if let Ok(kbd) = RgbKeyboardControl::new() {
                kbd.set_mode(&settings.mode)?;
            }
        }
    }
    
    Ok(())
}

fn apply_screen_settings(settings: &ScreenSettings) -> Result<()> {
    if settings.system_control {
        log::info!("Using system screen brightness control");
        return Ok(());
    }
    
    let backlight_paths = [
        "/sys/class/backlight/intel_backlight",
        "/sys/class/backlight/amdgpu_bl0",
        "/sys/class/backlight/amdgpu_bl1",
        "/sys/class/backlight/acpi_video0",
    ];
    
    for base_path in &backlight_paths {
        let brightness_path = format!("{}/brightness", base_path);
        let max_brightness_path = format!("{}/max_brightness", base_path);
        
        if Path::new(&brightness_path).exists() {
            let max_brightness: u32 = fs::read_to_string(&max_brightness_path)
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(255);
            
            let actual_brightness = ((settings.brightness as u32) * max_brightness) / 100;
            
            // Write to actual_brightness first (this is writable)
            let actual_path = format!("{}/actual_brightness", base_path);
            if Path::new(&actual_path).exists() {
                if let Err(e) = fs::write(&actual_path, actual_brightness.to_string()) {
                    log::warn!("Could not write to actual_brightness: {}", e);
                }
            }
            
            // Then write to brightness
            match fs::write(&brightness_path, actual_brightness.to_string()) {
                Ok(_) => {
                    log::info!("Set screen brightness to {}% at {}", settings.brightness, base_path);
                    return Ok(());
                }
                Err(e) => {
                    log::warn!("Failed to set brightness at {}: {}", base_path, e);
                    continue;
                }
            }
        }
    }
    
    Err(anyhow!("No writable backlight control found"))
}

pub fn set_tdp_profile(profile_name: &str) -> Result<()> {
    if !TuxedoIo::is_available() {
        return Err(anyhow!("TDP profiles not available"));
    }
    
    let io = TuxedoIo::new()?;
    let profiles = io.get_available_profiles()?;
    
    if let Some(profile_id) = profiles.iter().position(|p| p == profile_name) {
        io.set_performance_profile(profile_id as u32)?;
        log::info!("Set TDP profile to: {} (id: {})", profile_name, profile_id);
        Ok(())
    } else {
        Err(anyhow!("Profile '{}' not found. Available: {:?}", profile_name, profiles))
    }
}

pub fn set_fan_speed(fan_id: u32, speed_percent: u32) -> Result<()> {
    if !TuxedoIo::is_available() {
        return Err(anyhow!("Fan control not available"));
    }
    
    let speed = speed_percent.min(100);
    log::info!("DBus request: set fan {} to {}%", fan_id, speed);
    let io = TuxedoIo::new()?;
    io.set_fan_speed(fan_id, speed)?;
    
    log::info!("Set fan {} to {}%", fan_id, speed);
    Ok(())
}

pub fn set_fan_auto(fan_id: u32) -> Result<()> {
    if !TuxedoIo::is_available() {
        return Err(anyhow!("Fan control not available"));
    }
    
    let io = TuxedoIo::new()?;
    io.set_fan_auto()?;
    
    log::info!("Set all fans to auto mode");
    Ok(())
}

fn apply_fan_settings(settings: &FanSettings) -> Result<()> {
    if !TuxedoIo::is_available() {
        log::info!("Fan control not available (/dev/tuxedo_io not present)");
        return Ok(());
    }
    
    log::info!("Applying fan settings: enabled={}", settings.control_enabled);
    
    // Update the global fan daemon state
    {
        let mut state = crate::FAN_DAEMON_STATE.lock().unwrap();
        if settings.control_enabled {
            *state = Some(settings.clone());
            log::info!("Fan daemon: enabled with {} curves", settings.curves.len());
        } else {
            *state = None;
            log::info!("Fan daemon: disabled");
        }
    }
    
    if !settings.control_enabled {
        set_fan_auto(0)?;
        log::info!("Set all fans to auto mode");
    }
    
    Ok(())
}

pub fn set_webcam_state(enabled: bool) -> Result<()> {
    if !TuxedoIo::is_available() {
        return Err(anyhow!("Webcam control not available"));
    }
    
    let io = TuxedoIo::new()?;
    io.set_webcam_state(enabled)?;
    
    log::info!("Set webcam to: {}", if enabled { "enabled" } else { "disabled" });
    Ok(())
}

pub fn get_webcam_state() -> Result<bool> {
    if !TuxedoIo::is_available() {
        return Err(anyhow!("Webcam state not available"));
    }
    
    let io = TuxedoIo::new()?;
    io.get_webcam_state()
}

fn find_keyboard_backlight_path() -> Option<String> {
    let possible_paths = vec![
        "/sys/class/leds/rgb:kbd_backlight",
        "/sys/class/leds/tuxedo::kbd_backlight",
        "/sys/devices/platform/tuxedo_keyboard/leds/rgb:kbd_backlight",
        "/sys/class/leds/asus::kbd_backlight",
    ];
    
    for path in possible_paths {
        let brightness_path = format!("{}/brightness", path);
        if Path::new(&brightness_path).exists() {
            log::info!("Found keyboard backlight at: {}", path);
            return Some(path.to_string());
        }
    }
    
    log::warn!("No keyboard backlight found");
    None
}

pub fn set_energy_performance_preference(epp: &str) -> Result<()> {
    let cpu_count = get_cpu_count()?;
    
    let valid_values = ["performance", "balance_performance", "balance_power", "power", 
                       "default", "balance-performance", "balance-power"];
    if !valid_values.contains(&epp) {
        return Err(anyhow!("Invalid EPP value: {}", epp));
    }
    
    for i in 0..cpu_count {
        let path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/energy_performance_preference", i);
        if Path::new(&path).exists() {
            fs::write(&path, epp)
                .map_err(|e| anyhow!("Failed to set EPP for CPU {}: {}", i, e))?;
        }
    }
    
    log::info!("Set energy performance preference to: {}", epp);
    Ok(())
}

#[derive(Debug, Clone)]
pub struct RgbKeyboardControl {
    base_path: String,
}

impl RgbKeyboardControl {
    pub fn new() -> Result<Self> {
        let base_path = Self::find_keyboard_backlight_path()?;
        Ok(Self { base_path })
    }
    
    pub fn is_available() -> bool {
        Self::find_keyboard_backlight_path().is_ok()
    }
    
    fn find_keyboard_backlight_path() -> Result<String> {
        let possible_paths = vec![
            "/sys/class/leds/rgb:kbd_backlight",
            "/sys/class/leds/tuxedo::kbd_backlight",
            "/sys/devices/platform/tuxedo_keyboard/leds/rgb:kbd_backlight",
            "/sys/class/leds/asus::kbd_backlight",
        ];
        
        for path in possible_paths {
            let brightness_path = format!("{}/brightness", path);
            if Path::new(&brightness_path).exists() {
                log::info!("Found keyboard backlight at: {}", path);
                return Ok(path.to_string());
            }
        }
        
        Err(anyhow!("No RGB keyboard backlight found"))
    }
    
    pub fn set_color(&self, red: u8, green: u8, blue: u8) -> Result<()> {
        let color_path = format!("{}/multi_intensity", self.base_path);
        if !Path::new(&color_path).exists() {
            return Err(anyhow!("RGB control not available"));
        }
        
        let color_str = format!("{} {} {}", red, green, blue);
        fs::write(&color_path, color_str)?;
        
        log::info!("Set keyboard RGB color: ({}, {}, {})", red, green, blue);
        Ok(())
    }
    
    pub fn set_brightness(&self, brightness: u8) -> Result<()> {
        let brightness_path = format!("{}/brightness", self.base_path);
        let max_brightness_path = format!("{}/max_brightness", self.base_path);
        
        let max_brightness: u32 = if let Ok(max_str) = fs::read_to_string(&max_brightness_path) {
            max_str.trim().parse().unwrap_or(255)
        } else {
            255
        };
        
        let actual_brightness = ((brightness as u32) * max_brightness) / 100;
        fs::write(&brightness_path, actual_brightness.to_string())?;
        
        log::info!("Set keyboard brightness to {}%", brightness);
        Ok(())
    }
    
    pub fn get_brightness(&self) -> Result<u8> {
        let brightness_path = format!("{}/brightness", self.base_path);
        let max_brightness_path = format!("{}/max_brightness", self.base_path);
        
        let current: u32 = fs::read_to_string(&brightness_path)?
            .trim()
            .parse()?;
        
        let max: u32 = fs::read_to_string(&max_brightness_path)?
            .trim()
            .parse()
            .unwrap_or(255);
        
        let percent = ((current * 100) / max) as u8;
        Ok(percent)
    }
    
    pub fn set_mode(&self, mode: &tuxedo_common::types::KeyboardMode) -> Result<()> {
        use tuxedo_common::types::KeyboardMode;
        match mode {
            KeyboardMode::SingleColor { r, g, b, brightness } => {
                self.set_color(*r, *g, *b)?;
                self.set_brightness(*brightness)?;
            }
            KeyboardMode::Breathe { r, g, b, brightness, speed } => {
                let mode_path = format!("{}/mode", self.base_path);
                if Path::new(&mode_path).exists() {
                    fs::write(&mode_path, "breathing")?;
                }
                self.set_color(*r, *g, *b)?;
                self.set_brightness(*brightness)?;
                log::info!("Set breathing mode with speed {}", speed);
            }
            KeyboardMode::Wave { brightness, speed } => {
                let mode_path = format!("{}/mode", self.base_path);
                if Path::new(&mode_path).exists() {
                    fs::write(&mode_path, "wave")?;
                    self.set_brightness(*brightness)?;
                    log::info!("Set wave mode with speed {}", speed);
                } else {
                    return Err(anyhow!("Wave mode not supported"));
                }
            }
            KeyboardMode::Cycle { brightness, speed } => {
                let mode_path = format!("{}/mode", self.base_path);
                if Path::new(&mode_path).exists() {
                    fs::write(&mode_path, "cycle")?;
                    self.set_brightness(*brightness)?;
                    log::info!("Set cycle mode with speed {}", speed);
                } else {
                    return Err(anyhow!("Cycle mode not supported"));
                }
            }
            KeyboardMode::Dance { brightness, speed } => {
                let mode_path = format!("{}/mode", self.base_path);
                if Path::new(&mode_path).exists() {
                    fs::write(&mode_path, "dance")?;
                    self.set_brightness(*brightness)?;
                    log::info!("Set dance mode with speed {}", speed);
                } else {
                    return Err(anyhow!("Dance mode not supported"));
                }
            }
            KeyboardMode::Flash { r, g, b, brightness, speed } => {
                let mode_path = format!("{}/mode", self.base_path);
                if Path::new(&mode_path).exists() {
                    fs::write(&mode_path, "flash")?;
                }
                self.set_color(*r, *g, *b)?;
                self.set_brightness(*brightness)?;
                log::info!("Set flash mode with speed {}", speed);
            }
            KeyboardMode::RandomColor { brightness, speed } => {
                let mode_path = format!("{}/mode", self.base_path);
                if Path::new(&mode_path).exists() {
                    fs::write(&mode_path, "random")?;
                    self.set_brightness(*brightness)?;
                    log::info!("Set random color mode with speed {}", speed);
                } else {
                    return Err(anyhow!("Random color mode not supported"));
                }
            }
            KeyboardMode::Tempo { brightness, speed } => {
                let mode_path = format!("{}/mode", self.base_path);
                if Path::new(&mode_path).exists() {
                    fs::write(&mode_path, "tempo")?;
                    self.set_brightness(*brightness)?;
                    log::info!("Set tempo mode with speed {}", speed);
                } else {
                    return Err(anyhow!("Tempo mode not supported"));
                }
            }
        }
        Ok(())
    }
}
