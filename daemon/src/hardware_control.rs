use anyhow::{anyhow, Result};
use nvml_wrapper::Nvml;
use once_cell::sync::Lazy;
use std::fs;
use std::path::Path;
use tuxedo_common::types::*;
use crate::tuxedo_io::TuxedoIo;

/// Keyboard effect modes for Clevo hardware
#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum ClevoEffectMode {
    Custom = 0x00000000,        // Static color
    Breathe = 0x1002A000,       // Breathing effect
    Cycle = 0x33010000,         // Color cycle through spectrum
    Dance = 0x80000000,         // Dance effect
    Flash = 0xA0000000,         // Flash effect
    RandomColor = 0x70000000,   // Random color
    Tempo = 0x90000000,         // Tempo effect
    Wave = 0xB0000000,          // Wave effect
}

impl ClevoEffectMode {
    pub fn from_keyboard_mode(mode: &KeyboardMode) -> (Self, Option<u8>, Option<u8>) {
        match mode {
            KeyboardMode::SingleColor { .. } => (Self::Custom, None, None),
            KeyboardMode::Breathe { speed, .. } => (Self::Breathe, Some(*speed), None),
            KeyboardMode::Cycle { speed, .. } => (Self::Cycle, Some(*speed), None),
            KeyboardMode::Dance { speed, .. } => (Self::Dance, Some(*speed), None),
            KeyboardMode::Flash { speed, .. } => (Self::Flash, Some(*speed), None),
            KeyboardMode::RandomColor { speed, .. } => (Self::RandomColor, Some(*speed), None),
            KeyboardMode::Tempo { speed, .. } => (Self::Tempo, Some(*speed), None),
            KeyboardMode::Wave { speed, .. } => (Self::Wave, Some(*speed), None),
        }
    }
}

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

/// Apply keyboard settings (main entry point)
pub fn apply_keyboard_settings(settings: &KeyboardSettings) -> Result<()> {
    // Detect hardware type and apply appropriate settings
    if is_clevo_hardware() {
        apply_clevo_keyboard_settings(settings)
    } else if is_uniwill_hardware() {
        apply_uniwill_keyboard_settings(settings)
    } else {
        // Fallback to sysfs-based control
        apply_keyboard_settings_sysfs(settings)
    }
}

pub fn preview_keyboard_settings(settings: &KeyboardSettings) -> Result<()> {
    // For preview, we use the same hardware-specific logic
    if is_clevo_hardware() {
        apply_clevo_keyboard_settings(settings)
    } else if is_uniwill_hardware() {
        apply_uniwill_keyboard_settings(settings)
    } else {
        preview_keyboard_settings_sysfs(settings)
    }
}

/// Apply keyboard settings for Clevo hardware
fn apply_clevo_keyboard_settings(settings: &KeyboardSettings) -> Result<()> {
    if !settings.control_enabled {
        log::info!("Clevo keyboard control disabled, skipping");
        return Ok(());
    }

    match &settings.mode {
        KeyboardMode::SingleColor { r, g, b, brightness } => {
            log::info!("Applying Clevo static color: RGB({}, {}, {}) brightness {}%", r, g, b, brightness);

            // Apply color
            apply_clevo_static_color(*r, *g, *b)?;

            // Apply brightness
            apply_clevo_brightness(*brightness)?;
        }
        mode => {
            // For effect modes, we need to use the Clevo interface
            let (effect_mode, speed, _) = ClevoEffectMode::from_keyboard_mode(mode);

            log::info!("Applying Clevo effect mode: {:?}", effect_mode);

            // First set the color if the mode uses it
            if let Some((r, g, b)) = get_mode_color(mode) {
                apply_clevo_static_color(r, g, b)?;
            }

            // Apply the effect mode
            apply_clevo_effect_mode(effect_mode, speed)?;

            // Apply brightness
            if let Some(brightness) = get_mode_brightness(mode) {
                apply_clevo_brightness(brightness)?;
            }
        }
    }

    Ok(())
}

/// Apply static color for Clevo keyboards
fn apply_clevo_static_color(r: u8, g: u8, b: u8) -> Result<()> {
    // Pack RGB into 0x00RRGGBB format
    let color_value = ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);

    // For 3-zone RGB keyboards, we need to set all zones
    // Zone 0 (left)
    let zone0_cmd = 0xF0000000 | color_value;
    clevo_evaluate_method(0x67, zone0_cmd)?;

    // Zone 1 (center)
    let zone1_cmd = 0xF1000000 | color_value;
    clevo_evaluate_method(0x67, zone1_cmd)?;

    // Zone 2 (right)
    let zone2_cmd = 0xF2000000 | color_value;
    clevo_evaluate_method(0x67, zone2_cmd)?;

    log::debug!("Applied Clevo static color: RGB({}, {}, {})", r, g, b);
    Ok(())
}

/// Apply effect mode for Clevo keyboards
fn apply_clevo_effect_mode(mode: ClevoEffectMode, speed: Option<u8>) -> Result<()> {
    let mut mode_value = mode as u32;

    // Some modes support speed parameter (encoded in lower bits)
    if let Some(speed) = speed {
        // Speed is typically 0-255, map to appropriate range for the effect
        // Different effects may use different speed encodings
        match mode {
            ClevoEffectMode::Breathe | ClevoEffectMode::Flash => {
                // These modes encode speed in bits 8-15
                mode_value = (mode_value & 0xFFFF0000) | ((speed as u32) << 8);
            }
            _ => {
                // Other modes use default encoding
            }
        }
    }

    log::debug!("Applying Clevo effect mode: 0x{:08X}", mode_value);
    clevo_evaluate_method(0x67, mode_value)?;

    Ok(())
}

/// Apply brightness for Clevo keyboards
fn apply_clevo_brightness(brightness: u8) -> Result<()> {
    let brightness_cmd = 0xF4000000 | (brightness as u32);
    clevo_evaluate_method(0x67, brightness_cmd)?;

    log::debug!("Applied Clevo brightness: {}%", brightness);
    Ok(())
}

/// Helper to evaluate Clevo methods
fn clevo_evaluate_method(cmd: u8, arg: u32) -> Result<()> {
    let io = TuxedoIo::new()?;
    io.evaluate_clevo_method(cmd, arg)
}

/// Get color from keyboard mode
fn get_mode_color(mode: &KeyboardMode) -> Option<(u8, u8, u8)> {
    match mode {
        KeyboardMode::SingleColor { r, g, b, .. } => Some((*r, *g, *b)),
        KeyboardMode::Breathe { r, g, b, .. } => Some((*r, *g, *b)),
        KeyboardMode::Flash { r, g, b, .. } => Some((*r, *g, *b)),
        _ => None,
    }
}

/// Get brightness from keyboard mode
fn get_mode_brightness(mode: &KeyboardMode) -> Option<u8> {
    match mode {
        KeyboardMode::SingleColor { brightness, .. } |
        KeyboardMode::Breathe { brightness, .. } |
        KeyboardMode::Cycle { brightness, .. } |
        KeyboardMode::Dance { brightness, .. } |
        KeyboardMode::Flash { brightness, .. } |
        KeyboardMode::RandomColor { brightness, .. } |
        KeyboardMode::Tempo { brightness, .. } |
        KeyboardMode::Wave { brightness, .. } => Some(*brightness),
    }
}

/// Check if this is Clevo hardware
fn is_clevo_hardware() -> bool {
    TuxedoIo::is_available() && TuxedoIo::new()
        .map(|io| matches!(io.get_interface(), crate::tuxedo_io::HardwareInterface::Clevo))
        .unwrap_or(false)
}

/// Check if this is Uniwill hardware
fn is_uniwill_hardware() -> bool {
    TuxedoIo::is_available() && TuxedoIo::new()
        .map(|io| matches!(io.get_interface(), crate::tuxedo_io::HardwareInterface::Uniwill))
        .unwrap_or(false)
}

/// Apply keyboard settings for Uniwill hardware
fn apply_uniwill_keyboard_settings(settings: &KeyboardSettings) -> Result<()> {
    if !settings.control_enabled {
        log::info!("Uniwill keyboard control disabled, skipping");
        return Ok(());
    }

    log::info!("Uniwill keyboard effects not yet implemented");
    Ok(())
}

/// Fallback sysfs-based keyboard settings
fn apply_keyboard_settings_sysfs(settings: &KeyboardSettings) -> Result<()> {
    if !settings.control_enabled {
        log::info!("Keyboard control disabled, skipping");
        return Ok(());
    }
    
    let base_path = match find_keyboard_backlight_path() {
        Some(path) => path,
        None => {
            log::warn!("Keyboard backlight not found for sysfs control");
            return Ok(());
        }
    };
    
    match &settings.mode {
        KeyboardMode::SingleColor { r, g, b, brightness } => {
            log::info!("Applying keyboard: RGB({}, {}, {}) brightness {}%", r, g, b, brightness);
            
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
            
            log::info!("✅ Keyboard backlight applied successfully");
        }
        _ => {
            if let Ok(kbd) = RgbKeyboardControl::new() {
                kbd.set_mode(&settings.mode)?;
                log::info!("✅ Keyboard effect mode applied successfully");
            } else {
                log::warn!("RGB keyboard control not available for effect modes via sysfs");
            }
        }
    }
    
    Ok(())
}

fn preview_keyboard_settings_sysfs(settings: &KeyboardSettings) -> Result<()> {
    let base_path = match find_keyboard_backlight_path() {
        Some(path) => path,
        None => return Ok(()),
    };
    
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

use nvml_wrapper::enum_wrappers::device::{Clock, PerformanceState};
use nvml_wrapper::enums::device::GpuLockedClocksSetting;

static NVML: Lazy<Result<Nvml, nvml_wrapper::error::NvmlError>> = Lazy::new(|| Nvml::init());

pub fn get_nvml() -> Result<&'static Nvml> {
    match &*NVML {
        Ok(nvml) => Ok(nvml),
        Err(e) => Err(anyhow!("Failed to initialize NVML: {}", e)),
    }
}

pub fn set_gpu_locked_clocks(device_index: u32, min_clock: u32, max_clock: u32) -> Result<()> {
    let nvml = get_nvml()?;
    let mut device = nvml.device_by_index(device_index)?;
    device.set_gpu_locked_clocks(GpuLockedClocksSetting::Numeric {
        min_clock_mhz: min_clock,
        max_clock_mhz: max_clock,
    })?;
    Ok(())
}

pub fn set_memory_locked_clocks(device_index: u32, min_clock: u32, max_clock: u32) -> Result<()> {
    let nvml = get_nvml()?;
    let mut device = nvml.device_by_index(device_index)?;
    device.set_mem_locked_clocks(min_clock, max_clock)?;
    Ok(())
}

pub fn reset_memory_locked_clocks(device_index: u32) -> Result<()> {
    let nvml = get_nvml()?;
    let mut device = nvml.device_by_index(device_index)?;
    device.reset_mem_locked_clocks()?;
    Ok(())
}

pub fn reset_gpu_clocks(device_index: u32) -> Result<()> {
    let nvml = get_nvml()?;
    let mut device = nvml.device_by_index(device_index)?;
    device.reset_gpu_locked_clocks()?;
    Ok(())
}

pub fn set_gpu_core_offset(device_index: u32, offset: i32) -> Result<()> {
    let nvml = get_nvml()?;
    let mut device = nvml.device_by_index(device_index)?;
    device.set_clock_offset(Clock::Graphics, PerformanceState::Zero, offset)?;
    log::info!("Set GPU core offset to {} MHz for device {}", offset, device_index);
    Ok(())
}

pub fn set_gpu_memory_offset(device_index: u32, offset: i32) -> Result<()> {
    let nvml = get_nvml()?;
    let mut device = nvml.device_by_index(device_index)?;
    device.set_clock_offset(Clock::Memory, PerformanceState::Zero, offset)?;
    log::info!("Set GPU memory offset to {} MHz for device {}", offset, device_index);
    Ok(())
}

pub fn set_prime_profile(profile: &str) -> Result<()> {
    let valid_profiles = ["on-demand", "nvidia", "intel"];
    if !valid_profiles.contains(&profile) {
        return Err(anyhow!("Invalid prime profile: {}", profile));
    }

    let output = std::process::Command::new("prime-select")
        .arg(profile)
        .output()?;
    if !output.status.success() {
        return Err(anyhow!("prime-select command failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    log::info!("Set prime profile to: {}", profile);
    Ok(())
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
                self.write_effect_mode(1, "breathing")?;
                self.write_effect_speed(*speed)?;
                self.set_color(*r, *g, *b)?;
                self.set_brightness(*brightness)?;
                log::info!("Set breathing mode with speed {}", speed);
            }
            KeyboardMode::Wave { brightness, speed } => {
                self.write_effect_mode(7, "wave")?;
                self.write_effect_speed(*speed)?;
                self.set_brightness(*brightness)?;
                log::info!("Set wave mode with speed {}", speed);
            }
            KeyboardMode::Cycle { brightness, speed } => {
                self.write_effect_mode(2, "cycle")?;
                self.write_effect_speed(*speed)?;
                self.set_brightness(*brightness)?;
                log::info!("Set cycle mode with speed {}", speed);
            }
            KeyboardMode::Dance { brightness, speed } => {
                self.write_effect_mode(3, "dance")?;
                self.write_effect_speed(*speed)?;
                self.set_brightness(*brightness)?;
                log::info!("Set dance mode with speed {}", speed);
            }
            KeyboardMode::Flash { r, g, b, brightness, speed } => {
                self.write_effect_mode(4, "flash")?;
                self.write_effect_speed(*speed)?;
                self.set_color(*r, *g, *b)?;
                self.set_brightness(*brightness)?;
                log::info!("Set flash mode with speed {}", speed);
            }
            KeyboardMode::RandomColor { brightness, speed } => {
                self.write_effect_mode(5, "random")?;
                self.write_effect_speed(*speed)?;
                self.set_brightness(*brightness)?;
                log::info!("Set random color mode with speed {}", speed);
            }
            KeyboardMode::Tempo { brightness, speed } => {
                self.write_effect_mode(6, "tempo")?;
                self.write_effect_speed(*speed)?;
                self.set_brightness(*brightness)?;
                log::info!("Set tempo mode with speed {}", speed);
            }
        }
        Ok(())
    }

    fn write_effect_mode(&self, mode_value: u8, fallback: &str) -> Result<()> {
        let mode_path = format!("{}/mode", self.base_path);
        if !Path::new(&mode_path).exists() {
            return Err(anyhow!("Keyboard effect modes not supported"));
        }

        let value = mode_value.to_string();
        if fs::write(&mode_path, &value).is_err() {
            fs::write(&mode_path, fallback)?;
        }

        Ok(())
    }

    fn write_effect_speed(&self, speed: u8) -> Result<()> {
        let speed_path = format!("{}/speed", self.base_path);
        if Path::new(&speed_path).exists() {
            fs::write(&speed_path, speed.to_string())?;
        }
        Ok(())
    }
}
