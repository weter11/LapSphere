use anyhow::{anyhow, Result};
use nvml_wrapper::Nvml;
use once_cell::sync::Lazy;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use lapsphere_common::types::*;
use crate::tuxedo_io::{TuxedoIo, HardwareInterface};

static CPU_LIMITS_MODIFIED: AtomicBool = AtomicBool::new(false);
static LAST_APPLIED_PROFILE: Lazy<Mutex<Option<Profile>>> = Lazy::new(|| Mutex::new(None));

/// Lock a shared daemon mutex, recovering from poisoning instead of panicking.
///
/// A `.lock().unwrap()` on a poisoned mutex (i.e. a thread panicked while
/// holding the guard) propagates the poison as a panic *in this thread*,
/// taking the whole daemon down. The daemon outlives any single hardware
/// call, so every global lock site must instead extract the inner value,
/// clear the poison, log, and carry on with the last-known-good contents.
fn lock_or_recover<'a, T>(mutex: &'a Mutex<T>, label: &str) -> std::sync::MutexGuard<'a, T> {
    match mutex.lock() {
        Ok(g) => g,
        Err(e) => {
            log::error!(target: "hw.lock", "{} mutex poisoned — clearing poison and recovering", label);
            let g = e.into_inner();
            mutex.clear_poison();
            g
        }
    }
}

/// ---------------------------------------------------------------------------
/// sysfs write safety guard (task-safety-sysfs-writes)
///
/// Every daemon write to /sysfs is routed through `guard_sysfs_write`, which
/// enforces two independent protections *before* any bytes hit disk:
///   1. PATH ALLOWLIST - only a fixed set of control attributes may ever be
///      written; anything else is refused outright.
///   2. VALUE RANGE CHECK - the payload must match the allowed vocabulary for
///      that attribute (sanctioned governor string, boolean, status word, or a
///      numeric within its hardware bounds). Out-of-range values are rejected,
///      never clamped silently.
/// ---------------------------------------------------------------------------

/// Governor names we will ever permit writing to scaling_governor. Unknown
/// tokens are refused so a bad/untrusted value cannot hang CPUs.
const GOVERNOR_ALLOWLIST: &[&str] = &[
    "performance", "powersave", "balanced", "ondemand", "user", "schedutil",
    "userspace", "conservative", "interactive", "scheyonic", "energy_perf",
];

/// cpufreq frequency attributes guarded against out-of-hardware-range values.
fn is_freq_attribute(path: &str) -> bool {
    path.ends_with("scaling_min_freq")
        || path.ends_with("scaling_max_freq")
        || path.ends_with("cpuinfo_min_freq")
        || path.ends_with("cpuinfo_max_freq")
}

/// Power-management boolean toggles written as "0"/"1".
fn is_bool_toggle(path: &str) -> bool {
    path.ends_with("cpufreq/boost")
        || path.ends_with("intel_pstate/no_turbo")
        || path.ends_with("amd_pstate/cpb_boost")
}

/// Intel p-state status words ("passive"/"active").
fn is_intel_pstate_status(path: &str) -> bool {
    path.ends_with("intel_pstate/status")
}

/// AMD p-state status words ("passive"/"active"/"guided").
fn is_amd_pstate_status(path: &str) -> bool {
    path.ends_with("amd_pstate/status")
}

/// SMT control ("on"/"off").
fn is_smt_control(path: &str) -> bool {
    path.ends_with("smt/control")
}

/// Numeric backlight brightness attrs; bounded by sibling max_brightness.
fn is_brightness_attr(path: &str) -> bool {
    path.ends_with("/brightness") || path.ends_with("/actual_brightness")
}

/// LED mode/speed attrs; small non-negative byte integers.
fn is_led_mode_or_speed(path: &str) -> bool {
    path.ends_with("/mode") || path.ends_with("/speed")
}

/// Whole-path allowlist ('*' wildcard spans exactly one path segment).
const APPROVED_SYSFS_PATHS: &[&str] = &[
    "/sys/devices/system/cpu/*/cpufreq/scaling_governor",
    "/sys/devices/system/cpu/*/cpufreq/scaling_min_freq",
    "/sys/devices/system/cpu/*/cpufreq/scaling_max_freq",
    "/sys/devices/system/cpu/*/cpufreq/cpuinfo_min_freq",
    "/sys/devices/system/cpu/*/cpufreq/cpuinfo_max_freq",
    "/sys/devices/system/cpu/*/cpufreq/energy_performance_preference",
    "/sys/devices/system/cpu/*/cpufreq/energy_performance_available_preferences",
    "/sys/devices/system/cpu/cpufreq/boost",
    "/sys/devices/system/cpu/intel_pstate/no_turbo",
    "/sys/devices/system/cpu/amd_pstate/cpb_boost",
    "/sys/devices/system/cpu/smt/control",
    "/sys/devices/system/cpu/amd_pstate/status",
    "/sys/devices/system/cpu/intel_pstate/status",
    "/sys/class/backlight/*/brightness",
    "/sys/class/backlight/*/actual_brightness",
    "/sys/class/leds/*/multi_intensity",
    "/sys/class/leds/*/brightness",
    "/sys/class/leds/*/mode",
    "/sys/class/leds/*/speed",
];

/// Split on '/', dropping the empty leading segment that an absolute path
/// produces ("/sys/..." -> ["", "sys", ...]) so it cannot poison the wildcard
/// comparison below.
fn split_path(p: &str) -> Vec<&str> {
    p.split('/').filter(|s| !s.is_empty()).collect()
}

/// Does `path` match an allowlist template whose '*' wildcard spans one segment?
fn matches_template(path: &str, tmpl: &str) -> bool {
    let mut a = split_path(path);
    let mut b = split_path(tmpl);
    if a.len() != b.len() {
        return false;
    }
    for (x, y) in a.iter().zip(b.iter()) {
        if *y == "*" {
            continue; // wildcard matches any single non-empty segment
        }
        if x.is_empty() || *x != *y {
            return false;
        }
    }
    true
}

fn is_allowed_sysfs_path(path: &str) -> bool {
    APPROVED_SYSFS_PATHS.iter().any(|t| matches_template(path, t))
}

/// Parse a u64 payload, used by both frequency and brightness range checks.
fn parse_u64(contents: &str) -> Result<u64> {
    contents.trim().parse::<u64>().map_err(|_| {
        anyhow!("non-numeric value \"{}\"", contents)
    })
}

/// Range-check a frequency attribute against the given hardware window.
/// Returns Ok(()) when the value is in-range; Err when out-of-range or
/// unparseable. Callers decide whether an unreadable window is fatal.
fn within_hw_freq(contents: &str, path: &str, hw_min: u64, hw_max: u64) -> Result<()> {
    let raw = parse_u64(contents)?;
    if raw < hw_min || raw > hw_max {
        return Err(anyhow!(
            "freq {} out of [{},{}] for {}",
            raw,
            hw_min,
            hw_max,
            path
        ));
    }
    Ok(())
}

/// Range-check a backlight brightness value against the reported maximum.
fn within_brightness(contents: &str, path: &str, max_b: u32) -> Result<()> {
    let val: u32 = contents
        .trim()
        .parse()
        .map_err(|_| anyhow!("non-numeric brightness \"{}\" for {}", contents, path))?;
    if val > max_b {
        return Err(anyhow!(
            "brightness {} exceeds max {} for {}",
            val,
            max_b,
            path
        ));
    }
    Ok(())
}

/// Read CPU0 hardware frequency bounds once for range checking.
fn read_hw_freq_bounds() -> Option<(u64, u64)> {
    let min_s = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_min_freq").ok()?;
    let max_s = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq").ok()?;
    let min: u64 = min_s.trim().parse().ok()?;
    let max: u64 = max_s.trim().parse().ok()?;
    Some((min, max))
}

/// Maximum brightness the hardware reports (sibling of the writable attr).
fn read_max_brightness(base: &str) -> Option<u32> {
    let s = fs::read_to_string(format!("{base}/max_brightness")).ok()?;
    s.trim().parse().ok()
}

/// Central gate: refuse anything not on the allowlist, then enforce value rules.
fn guard_sysfs_write(path: &str, contents: &str) -> Result<()> {
    if !is_allowed_sysfs_path(path) {
        return Err(anyhow!(
            "blocked non-allowlisted sysfs write to {}",
            path
        ));
    }

    // Governor string must be an explicitly sanctioned value.
    if path.ends_with("scaling_governor") {
        if !GOVERNOR_ALLOWLIST.contains(&contents) {
            return Err(anyhow!(
                "governor \"{}\" rejected for {}; allowed: {:?}",
                contents,
                path,
                GOVERNOR_ALLOWLIST
            ));
        }
    }

    // Frequency attributes must sit within the hardware's [min,max] window.
    if is_freq_attribute(path) {
        match read_hw_freq_bounds() {
            Some((hw_min, hw_max)) => {
                within_hw_freq(contents, path, hw_min, hw_max)?;
            }
            None => {
                log::warn!(target: "hw.cpu",
                    "skipping freq range-check at {} (cpuinfo limits unreadable)", path);
            }
        }
    }

    // Backlight brightness bounded by reported max_brightness.
    if is_brightness_attr(path) {
        let trimmed = path.strip_suffix("/actual_brightness").unwrap();
        let trimmed = trimmed.strip_suffix("/brightness").unwrap();
        match read_max_brightness(trimmed) {
            Some(max_b) => {
                within_brightness(contents, path, max_b)?;
            }
            None => {
                log::warn!(target: "hw.screen",
                    "skipping brightness range-check at {} (max_brightness unreadable)", path);
            }
        }
    }

    // Boolean toggles are strictly 0/1.
    if is_bool_toggle(path) {
        if contents != "0" && contents != "1" {
            return Err(anyhow!(
                "bool toggle {} expects \"0\" or \"1\", got \"{}\"",
                path,
                contents
            ));
        }
    }

    // Intel p-state status word.
    if is_intel_pstate_status(path) {
        if !["passive", "active"].contains(&contents) {
            return Err(anyhow!(
                "intel pstate status \"{}\" invalid for {}",
                contents,
                path
            ));
        }
    }

    // AMD p-state status word.
    if is_amd_pstate_status(path) {
        if !["passive", "active", "guided"].contains(&contents) {
            return Err(anyhow!(
                "amd pstate status \"{}\" invalid for {}",
                contents,
                path
            ));
        }
    }

    // SMT control.
    if is_smt_control(path) {
        if contents != "on" && contents != "off" {
            return Err(anyhow!(
                "smt/control expects \"on\" or \"off\", got \"{}\"",
                contents
            ));
        }
    }

    // LED mode/speed small byte integers.
    if is_led_mode_or_speed(path) {
        if !contents.parse::<u8>().is_ok() {
            return Err(anyhow!(
                "led mode/speed must be a byte integer, got \"{}\"",
                contents
            ));
        }
    }

    // All checks passed: perform the write. The guard was originally
    // validate-only and the call sites validated without ever writing, which
    // made every sysfs-control method a no-op (profile apply returned success
    // and no hardware state changed). See the docblock above: the intent was
    // always "before any bytes hit disk".
    fs::write(path, contents)?;
    Ok(())
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

    guard_sysfs_write("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor", governor)?;

    for i in 0..cpu_count {
        let path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_governor", i);
        guard_sysfs_write(&path, governor)?;
    }
    
    log::info!(target: "hw.cpu", "set_governor profile=\"{}\"", governor);
    crate::refresh_hardware_cache();
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
            guard_sysfs_write(&max_path, &max_freq.to_string())?;
            guard_sysfs_write(&min_path, &min_freq.to_string())?;
        } else {
            // Set min first
            guard_sysfs_write(&min_path, &min_freq.to_string())?;
            guard_sysfs_write(&max_path, &max_freq.to_string())?;
        }
    }
    
    CPU_LIMITS_MODIFIED.store(true, Ordering::SeqCst);
    log::info!(target: "hw.cpu", "set_freq_limits min={} max={}", min_freq, max_freq);
    Ok(())
}

pub fn restore_cpu_frequency_limits() -> Result<()> {
    if !CPU_LIMITS_MODIFIED.load(Ordering::SeqCst) {
        return Ok(());
    }

    log::info!(target: "hw.cpu", "Restoring CPU frequency limits to hardware defaults");
    let (hw_min, hw_max) = crate::hardware_detection::read_hw_frequency_limits()?;

    if let (Some(min), Some(max)) = (hw_min, hw_max) {
        set_cpu_frequency_limits(min, max)?;
    }

    Ok(())
}

pub fn set_cpu_boost(enabled: bool) -> Result<()> {
    // AMD cpufreq boost
    let amd_path = "/sys/devices/system/cpu/cpufreq/boost";
    if Path::new(amd_path).exists() {
        guard_sysfs_write(amd_path, if enabled { "1" } else { "0" })?;
        log::info!(target: "hw.cpu", "set_amd_boost enabled={}", enabled);
        return Ok(());
    }
    
    // Intel turbo
    let intel_path = "/sys/devices/system/cpu/intel_pstate/no_turbo";
        if Path::new(intel_path).exists() {
            guard_sysfs_write(intel_path, if enabled { "0" } else { "1" })?;
        log::info!(target: "hw.cpu", "set_intel_turbo enabled={}", enabled);
        return Ok(());
    }
    
    // AMD P-State boost (if using amd-pstate driver)
    let amd_pstate_boost = "/sys/devices/system/cpu/amd_pstate/cpb_boost";
    if Path::new(amd_pstate_boost).exists() {
        guard_sysfs_write(amd_pstate_boost, if enabled { "1" } else { "0" })?;
        log::info!(target: "hw.cpu", "set_amd_pstate_boost enabled={}", enabled);
        return Ok(());
    }
    
    Err(anyhow!("Boost control not available"))
}

pub fn set_smt(enabled: bool) -> Result<()> {
    let path = "/sys/devices/system/cpu/smt/control";
    if !Path::new(path).exists() {
        return Err(anyhow!("SMT control not available"));
    }
    
    guard_sysfs_write(path, if enabled { "on" } else { "off" })?;
    log::info!(target: "hw.cpu", "set_smt enabled={}", enabled);
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
    
    guard_sysfs_write(path, status)?;
    log::info!(target: "hw.cpu", "set_amd_pstate_status status=\"{}\"", status);
    crate::refresh_hardware_cache();
    Ok(())
}

pub fn set_intel_pstate_status(status: &str) -> Result<()> {
    let path = "/sys/devices/system/cpu/intel_pstate/status";
    if !Path::new(path).exists() {
        return Err(anyhow!("Intel pstate not available"));
    }

    if !["passive", "active"].contains(&status) {
        return Err(anyhow!("Invalid Intel pstate status: {}", status));
    }

    guard_sysfs_write(path, status)?;
    log::info!(target: "hw.cpu", "set_intel_pstate_status status=\"{}\"", status);
    crate::refresh_hardware_cache();
    Ok(())
}

pub fn apply_profile(profile: &Profile) -> Result<()> {
    // Check if this profile is already applied to avoid redundant hardware calls.
    //
    // IMPORTANT: the "already applied" marker is only written *after* every
    // hardware step below succeeds. Previously it was recorded up-front, so a
    // failure on any late step (keyboard/screen/fan...) left the profile
    // marked as applied and every subsequent identical apply was silently
    // skipped, leaving the machine in a partial hardware state. See task
    // fix-hwc-stale-applied-profile.
    {
        let last_profile = lock_or_recover(&LAST_APPLIED_PROFILE, "LAST_APPLIED_PROFILE");
        if let Some(ref last) = *last_profile {
            if last == profile {
                log::info!(target: "hw.detect", "Profile '{}' is already applied, skipping", profile.name);
                return Ok(());
            }
        }
    }

    log::info!(target: "hw.detect", "Applying profile: {}", profile.name);
    let result = apply_profile_inner(profile);
    match result {
        Ok(()) => {
            // Record the profile only now that every hardware step succeeded.
            let mut last_profile = lock_or_recover(&LAST_APPLIED_PROFILE, "LAST_APPLIED_PROFILE");
            *last_profile = Some(profile.clone());
            log::info!(target: "hw.detect", "Profile '{}' applied successfully", profile.name);
            Ok(())
        }
        Err(e) => {
            // A partial apply must NOT be remembered as "applied": otherwise the
            // next identical apply would hit the early-return above and leave the
            // machine in the partial state forever.
            let mut last_profile = lock_or_recover(&LAST_APPLIED_PROFILE, "LAST_APPLIED_PROFILE");
            *last_profile = None;
            log::error!(target: "hw.detect", "Profile '{}' apply failed: {} — cleared last-applied marker", profile.name, e);
            Err(e)
        }
    }
}

/// All hardware steps of a profile apply, in order. The caller owns the
/// last-applied marker so a failure never leaves a stale "already applied" state.
fn apply_profile_inner(profile: &Profile) -> Result<()> {
    
    // Apply CPU settings
    if let Some(ref governor) = profile.cpu_settings.governor {
        set_cpu_governor(governor)?;
    }
    
    if let Some(ref tdp_profile) = profile.cpu_settings.tdp_profile {
        set_tdp_profile(tdp_profile)?;
    }

    if let Some(io) = TuxedoIo::shared() {
        if io.get_interface() == HardwareInterface::Uniwill {
            if let Some(val) = profile.cpu_settings.tdp0 {
                let _ = io.set_tdp(0, val);
            }
            if let Some(val) = profile.cpu_settings.tdp1 {
                let _ = io.set_tdp(1, val);
            }
            if let Some(val) = profile.cpu_settings.tdp2 {
                let _ = io.set_tdp(2, val);
            }
        }
    }
    
    if let Some(ref amd_status) = profile.cpu_settings.amd_pstate_status {
        set_amd_pstate_status(amd_status)?;
    }

    if let Some(ref intel_status) = profile.cpu_settings.intel_pstate_status {
        set_intel_pstate_status(intel_status)?;
    }
    
    if let Some(ref epp) = profile.cpu_settings.energy_performance_preference {
        set_energy_performance_preference(epp)?;
    }
    
    if let (Some(min), Some(max)) = (profile.cpu_settings.min_frequency, profile.cpu_settings.max_frequency) {
        set_cpu_frequency_limits(min, max)?;
    }

    // Apply GPU settings
    let nvidia_gpu_idx = {
        let cache = match crate::HARDWARE_CACHE.lock() {
            Ok(g) => g,
            Err(e) => {
                log::error!(target: "hw.cache", "HARDWARE_CACHE poisoned — clearing poison and recovering");
                let g = e.into_inner();
                crate::HARDWARE_CACHE.clear_poison();
                g
            }
        };
        cache.gpu_info.iter()
            .find(|g| g.name.to_lowercase().contains("nvidia"))
            .and_then(|g| g.nvml_index)
            .unwrap_or(0)
    };

    if let Some(limit) = profile.gpu_settings.power_limit {
        let _ = set_gpu_power_limit(nvidia_gpu_idx, limit);
    }

    if let Some(core_offset) = profile.gpu_settings.core_offset {
        let _ = set_gpu_core_offset(nvidia_gpu_idx, core_offset as f32);
    }

    if let Some(memory_offset) = profile.gpu_settings.memory_offset {
        let _ = set_gpu_memory_offset(nvidia_gpu_idx, memory_offset as f32);
    }

    if let (Some(min_clock), Some(max_clock)) = (profile.gpu_settings.min_gpu_clock, profile.gpu_settings.max_gpu_clock) {
        let _ = set_gpu_locked_clocks(nvidia_gpu_idx, min_clock, max_clock);
    } else {
        let _ = reset_gpu_clocks(nvidia_gpu_idx);
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

    // Apply NVIDIA fan settings
    for fan_setting in &profile.gpu_settings.nvidia_fans {
        if fan_setting.manual {
            let _ = set_gpu_fan_speed(fan_setting.device_index, fan_setting.fan_id, fan_setting.speed);
        } else {
            let _ = set_gpu_fan_auto(fan_setting.device_index, fan_setting.fan_id);
        }
    }

    Ok(())
}

pub fn apply_battery_settings(settings: &BatterySettings) -> Result<()> {
    if !crate::battery_control::BatteryControl::is_available() {
        log::info!(target: "hw.battery", "Battery control not available, skipping");
        return Ok(());
    }

    let battery = crate::battery_control::BatteryControl::new()?;

    if settings.control_enabled {
        battery.set_charge_type("Custom")?;
        battery.set_charge_control_start_threshold(settings.charge_start_threshold)?;
        battery.set_charge_control_end_threshold(settings.charge_end_threshold)?;
        log::info!(target: "hw.battery",
            "set_thresholds enabled=true start={} end={}",
            settings.charge_start_threshold,
            settings.charge_end_threshold
        );
    } else {
        battery.set_charge_type("Standard")?;
        log::info!(target: "hw.battery", "set_thresholds enabled=false mode=\"Standard\"");
    }

    Ok(())
}

fn apply_keyboard_settings(settings: &KeyboardSettings) -> Result<()> {
    if !settings.control_enabled {
        log::info!(target: "hw.kbd", "keyboard_control enabled=false");
        if let Ok(kbd) = RgbKeyboardControl::new() {
            let white_mode = KeyboardMode::SingleColor {
                r: 255,
                g: 255,
                b: 255,
                brightness: 50,
            };
            let _ = kbd.set_mode(&white_mode);
        }
        return Ok(());
    }
    
    if let Ok(kbd) = RgbKeyboardControl::new() {
        kbd.set_mode(&settings.mode)?;
        log::info!(target: "hw.kbd", "keyboard_settings applied=true");
        Ok(())
    } else {
        log::warn!(target: "hw.kbd", "Keyboard control not available");
        Err(anyhow!("Keyboard control not available"))
    }
}

pub fn preview_keyboard_settings(settings: &KeyboardSettings) -> Result<()> {
    if let Ok(kbd) = RgbKeyboardControl::new() {
        kbd.set_mode(&settings.mode)?;
        Ok(())
    } else {
        Err(anyhow!("Keyboard control not available"))
    }
}

fn apply_screen_settings(settings: &ScreenSettings) -> Result<()> {
    if settings.system_control {
        log::info!(target: "hw.screen", "Using system screen brightness control");
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
                let _ = guard_sysfs_write(&actual_path, &actual_brightness.to_string());
            }
            
            // Then write to brightness
            guard_sysfs_write(&brightness_path, &actual_brightness.to_string())?;
            log::info!(target: "hw.screen", "set_brightness level={}% path=\"{}\"", settings.brightness, base_path);
            return Ok(());
        }
    }
    
    Err(anyhow!("No writable backlight control found"))
}

pub fn set_tdp_profile(profile_name: &str) -> Result<()> {
    if !TuxedoIo::is_available() {
        return Err(anyhow!("TDP profiles not available"));
    }
    
    let io = TuxedoIo::shared().ok_or_else(|| anyhow!("tuxedo_io not available"))?;
    let profiles = io.get_available_profiles()?;
    
    if let Some(profile_id) = profiles.iter().position(|p| p == profile_name) {
        io.set_performance_profile(profile_id as u32)?;
        log::info!(target: "hw.cpu", "set_tdp_profile name=\"{}\" id={}", profile_name, profile_id);
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
    log::info!(target: "hw.fan", "DBus request: set fan {} to {}%", fan_id, speed);
    let io = TuxedoIo::shared().ok_or_else(|| anyhow!("tuxedo_io not available"))?;
    io.set_fan_speed(fan_id, speed)?;
    
    log::info!(target: "hw.fan", "set_fan id={} speed={}%", fan_id, speed);
    Ok(())
}

pub fn set_fan_auto(_fan_id: u32) -> Result<()> {
    if !TuxedoIo::is_available() {
        return Err(anyhow!("Fan control not available"));
    }
    
    let io = TuxedoIo::shared().ok_or_else(|| anyhow!("tuxedo_io not available"))?;
    io.set_fan_auto()?;
    
    log::info!(target: "hw.fan", "set_fans_auto");
    Ok(())
}

fn apply_fan_settings(settings: &FanSettings) -> Result<()> {
    if !TuxedoIo::is_available() {
        log::info!(target: "hw.fan", "Fan control not available (/dev/tuxedo_io not present)");
        return Ok(());
    }
    
    log::info!(target: "hw.fan", "Applying fan settings: enabled={}", settings.control_enabled);
    
    // Update the global fan daemon state
    {
        let mut state = lock_or_recover(&crate::FAN_DAEMON_STATE, "FAN_DAEMON_STATE");
        if settings.control_enabled {
            *state = Some(settings.clone());
            log::info!(target: "hw.fan", "fan_daemon enabled=true curves={}", settings.curves.len());
        } else {
            *state = None;
            log::info!(target: "hw.fan", "fan_daemon enabled=false");
        }
    }
    
    if !settings.control_enabled {
        set_fan_auto(0)?;
    }
    
    Ok(())
}

pub fn set_webcam_state(enabled: bool) -> Result<()> {
    if !TuxedoIo::is_available() {
        return Err(anyhow!("Webcam control not available"));
    }
    
    let io = TuxedoIo::shared().ok_or_else(|| anyhow!("tuxedo_io not available"))?;
    io.set_webcam_state(enabled)?;
    
    log::info!(target: "hw.detect", "set_webcam enabled={}", enabled);
    Ok(())
}

pub fn get_webcam_state() -> Result<bool> {
    if !TuxedoIo::is_available() {
        // Return true as default if driver not present
        return Ok(true);
    }
    
    let io = TuxedoIo::shared().ok_or_else(|| anyhow!("tuxedo_io not available"))?;
    if io.get_interface() != HardwareInterface::Clevo {
        // Return true for non-Clevo hardware (standard state)
        return Ok(true);
    }

    match io.get_webcam_state() {
        Ok(state) => Ok(state),
        Err(_) => Ok(true), // Fallback to true on error
    }
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


pub fn reset_gpu_clocks(device_index: u32) -> Result<()> {
    let nvml = get_nvml()?;
    let mut device = nvml.device_by_index(device_index)?;
    device.reset_gpu_locked_clocks()?;
    Ok(())
}

pub fn set_gpu_core_offset(device_index: u32, offset: f32) -> Result<()> {
    let nvml = get_nvml()?;
    let mut device = nvml.device_by_index(device_index)?;
    device.set_clock_offset(Clock::Graphics, PerformanceState::Zero, offset.round() as i32)?;
    {
        let mut map = lock_or_recover(&crate::MANUAL_GPU_OFFSETS, "MANUAL_GPU_OFFSETS");
        let entry = map.entry(device_index).or_insert((0.0, 0.0));
        entry.0 = offset;
    }
    log::info!(target: "hw.gpu", "set_core_offset gpu={} offset={} offset_rounded={}", device_index, offset, offset.round());
    Ok(())
}

pub fn set_gpu_memory_offset(device_index: u32, offset: f32) -> Result<()> {
    let nvml = get_nvml()?;
    let mut device = nvml.device_by_index(device_index)?;
    device.set_clock_offset(Clock::Memory, PerformanceState::Zero, offset.round() as i32)?;
    {
        let mut map = lock_or_recover(&crate::MANUAL_GPU_OFFSETS, "MANUAL_GPU_OFFSETS");
        let entry = map.entry(device_index).or_insert((0.0, 0.0));
        entry.1 = offset;
    }
    log::info!(target: "hw.gpu", "set_mem_offset gpu={} offset={} offset_rounded={}", device_index, offset, offset.round());
    Ok(())
}

pub fn set_gpu_power_limit(device_index: u32, limit_watts: u32) -> Result<()> {
    let nvml = get_nvml()?;
    let mut device = nvml.device_by_index(device_index)?;
    device.set_power_management_limit(limit_watts * 1000)?; // Watts to mW
    log::info!(target: "hw.gpu", "set_power_limit gpu={} limit={}W", device_index, limit_watts);
    Ok(())
}

pub fn set_gpu_fan_speed(device_index: u32, fan_index: u32, speed_percent: u32) -> Result<()> {
    let nvml = get_nvml()?;
    let mut device = nvml.device_by_index(device_index)?;
    device.set_fan_speed(fan_index, speed_percent)?;
    log::info!(target: "hw.gpu", "set_fan_speed gpu={} fan={} speed={}%", device_index, fan_index, speed_percent);
    Ok(())
}

pub fn set_gpu_fan_auto(device_index: u32, fan_index: u32) -> Result<()> {
    let nvml = get_nvml()?;
    let mut device = nvml.device_by_index(device_index)?;
    device.set_default_fan_speed(fan_index)?;
    log::info!(target: "hw.gpu", "set_fan_auto gpu={} fan={}", device_index, fan_index);
    Ok(())
}

fn find_binary(cmd: &str) -> Option<String> {
    let paths = ["/usr/bin", "/usr/sbin", "/usr/local/bin", "/usr/local/sbin", "/sbin", "/bin"];
    for path in paths {
        let full_path = format!("{}/{}", path, cmd);
        if Path::new(&full_path).exists() {
            return Some(full_path);
        }
    }
    None
}

pub fn set_prime_profile(profile: &str) -> Result<()> {
    let valid_profiles = ["on-demand", "nvidia", "intel"];
    if !valid_profiles.contains(&profile) {
        return Err(anyhow!("Invalid prime profile: {}", profile));
    }

    // Check for optimus-manager first (common on Arch)
    if let Some(path) = find_binary("optimus-manager") {
        let opt_mode = match profile {
            "on-demand" => "hybrid",
            "intel" => "integrated",
            "nvidia" => "nvidia",
            _ => profile,
        };

        log::info!(target: "hw.gpu", "set_prime_profile mode=\"{}\" tool=\"optimus-manager\"", opt_mode);
        let output = std::process::Command::new(path)
            .arg("--switch")
            .arg(opt_mode)
            .arg("--no-confirm")
            .output()?;

        if !output.status.success() {
            return Err(anyhow!("optimus-manager command failed: {}", String::from_utf8_lossy(&output.stderr)));
        }
        return Ok(());
    }

    // Fallback to prime-select (Ubuntu/Debian)
    let output = std::process::Command::new("prime-select")
        .arg(profile)
        .output()?;
    if !output.status.success() {
        return Err(anyhow!("prime-select command failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    log::info!(target: "hw.gpu", "set_prime_profile mode=\"{}\" tool=\"prime-select\"", profile);
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
            guard_sysfs_write(&path, epp)?;
        }
    }
    
    log::info!(target: "hw.cpu", "set_epp preference=\"{}\"", epp);
    Ok(())
}

pub fn set_all_cpu_epp(epp: &str) -> Result<()> {
    set_energy_performance_preference(epp)?;

    let policy_path = "/sys/devices/system/cpu/cpufreq/energy_performance_preference";
    if Path::new(policy_path).exists() {
        guard_sysfs_write(policy_path, epp)?;
    }

    log::info!(target: "hw.cpu", "set_all_cpu_epp preference={}", epp);
    Ok(())
}

#[derive(Debug, Clone)]
pub struct RgbKeyboardControl {
    paths: Vec<String>,
    tuxedo_io: Option<Arc<TuxedoIo>>,
}

impl RgbKeyboardControl {
    pub fn new() -> Result<Self> {
        let paths = Self::find_all_keyboard_backlight_paths();
        let tuxedo_io = TuxedoIo::shared();

        if paths.is_empty() && tuxedo_io.is_none() {
            return Err(anyhow!("No keyboard backlight control available"));
        }

        Ok(Self { paths, tuxedo_io })
    }
    
    
    fn find_all_keyboard_backlight_paths() -> Vec<String> {
        let paths = Vec::new();
        
        // Priority 1: tuxedo_keyboard platform device
        let platform_base = "/sys/devices/platform/tuxedo_keyboard/leds";
        if Path::new(platform_base).exists() {
            // Check for 3-zone
            let zones = ["left", "center", "right"];
            let mut found_zones = Vec::new();
            for zone in zones {
                let path = format!("{}/{}:kbd_backlight", platform_base, zone);
                if Path::new(&path).exists() {
                    found_zones.push(path);
                }
            }

            if !found_zones.is_empty() {
                return found_zones;
            }

            // Check for single zone
            let single = format!("{}/rgb:kbd_backlight", platform_base);
            if Path::new(&single).exists() {
                return vec![single];
            }
        }

        // Priority 2: Standard /sys/class/leds
        if let Ok(entries) = fs::read_dir("/sys/class/leds") {
            let mut kbd_entries: Vec<String> = entries.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.contains("kbd_backlight"))
                .map(|n| format!("/sys/class/leds/{}", n))
                .collect();

            // Sort to have a consistent order (e.g. left, center, right if they are named so)
            kbd_entries.sort();

            if !kbd_entries.is_empty() {
                return kbd_entries;
            }
        }
        
        paths
    }
    
    pub fn set_zone_color(&self, zone_idx: usize, red: u8, green: u8, blue: u8) -> Result<()> {
        if let Some(ref io) = self.tuxedo_io {
            if io.get_interface() == HardwareInterface::Clevo {
                if let Ok(_) = io.set_clevo_keyboard_color(zone_idx as u8, red, green, blue) {
                    // Also try to set via sysfs for consistency, but ignore errors if it fails
                    // as ioctl already succeeded.
                }
            }
        }

        let path = self.paths.get(zone_idx)
            .ok_or_else(|| anyhow!("Invalid zone index: {}", zone_idx))?;

        let color_path = format!("{}/multi_intensity", path);
        if !Path::new(&color_path).exists() {
            return Err(anyhow!("RGB control not available for zone {}", zone_idx));
        }
        
        let color_str = format!("{} {} {}", red, green, blue);
        guard_sysfs_write(&color_path, &color_str)?;
        
        log::info!(target: "hw.kbd", "set_zone_color zone={} r={} g={} b={}", zone_idx, red, green, blue);
        Ok(())
    }
    
    pub fn set_brightness(&self, brightness: u8) -> Result<()> {
        if let Some(ref io) = self.tuxedo_io {
            if io.get_interface() == HardwareInterface::Clevo {
                let _ = io.set_clevo_keyboard_brightness(brightness);
            }
        }

        for path in &self.paths {
            let brightness_path = format!("{}/brightness", path);
            let max_brightness_path = format!("{}/max_brightness", path);

            let max_brightness: u32 = if let Ok(max_str) = fs::read_to_string(&max_brightness_path) {
                max_str.trim().parse().unwrap_or(255)
            } else {
                255
            };

            let actual_brightness = ((brightness as u32) * max_brightness) / 100;
            guard_sysfs_write(&brightness_path, &actual_brightness.to_string())?;
        }
        
        log::info!(target: "hw.kbd", "set_brightness level={}%", brightness);
        Ok(())
    }
    
    pub fn set_mode(&self, mode: &lapsphere_common::types::KeyboardMode) -> Result<()> {
        use lapsphere_common::types::KeyboardMode;
        match mode {
            KeyboardMode::SingleColor { r, g, b, brightness } => {
                // For Clevo, explicitly set mode 0 (Custom/Static)
                if let Some(ref io) = self.tuxedo_io {
                    if io.get_interface() == HardwareInterface::Clevo {
                        let _ = io.set_clevo_keyboard_mode(0x00000000);
                    }
                }

                let num_zones = self.paths.len().max(if self.tuxedo_io.is_some() { 3 } else { 0 });
                for i in 0..num_zones {
                    let _ = self.set_zone_color(i, *r, *g, *b);
                }
                self.set_brightness(*brightness)?;
            }
            KeyboardMode::PerKeyRGB { keys, brightness } => {
                if let Some(ref io) = self.tuxedo_io {
                    if io.get_interface() == HardwareInterface::Clevo {
                        let _ = io.set_clevo_keyboard_mode(0x00000000);
                    }
                }

                for (i, color) in keys.iter().enumerate() {
                    let _ = self.set_zone_color(i, color.r, color.g, color.b);
                }
                self.set_brightness(*brightness)?;
            }
            KeyboardMode::MultipleZones { zones, brightness } => {
                if let Some(ref io) = self.tuxedo_io {
                    if io.get_interface() == HardwareInterface::Clevo {
                        let _ = io.set_clevo_keyboard_mode(0x00000000);
                    }
                }

                for (i, zone) in zones.iter().enumerate() {
                    let _ = self.set_zone_color(i, zone.r, zone.g, zone.b);
                }
                self.set_brightness(*brightness)?;
            }
            KeyboardMode::Breathe { r, g, b, brightness, speed } => {
                if let Some(ref io) = self.tuxedo_io {
                    if io.get_interface() == HardwareInterface::Clevo {
                        let _ = io.set_clevo_keyboard_mode(0x1002a000); // BREATHE
                    }
                }
                self.write_effect_mode(1)?;
                self.write_effect_speed(*speed)?;
                for i in 0..self.paths.len() {
                    let _ = self.set_zone_color(i, *r, *g, *b);
                }
                self.set_brightness(*brightness)?;
                log::info!(target: "hw.kbd", "set_mode mode=\"breathing\" speed={}", speed);
            }
            KeyboardMode::Wave { brightness, speed } => {
                if let Some(ref io) = self.tuxedo_io {
                    if io.get_interface() == HardwareInterface::Clevo {
                        let _ = io.set_clevo_keyboard_mode(0xB0000000); // WAVE
                    }
                }
                self.write_effect_mode(7)?;
                self.write_effect_speed(*speed)?;
                self.set_brightness(*brightness)?;
                log::info!(target: "hw.kbd", "set_mode mode=\"wave\" speed={}", speed);
            }
            KeyboardMode::Cycle { brightness, speed } => {
                if let Some(ref io) = self.tuxedo_io {
                    if io.get_interface() == HardwareInterface::Clevo {
                        let _ = io.set_clevo_keyboard_mode(0x33010000); // CYCLE
                    }
                }
                self.write_effect_mode(2)?;
                self.write_effect_speed(*speed)?;
                self.set_brightness(*brightness)?;
                log::info!(target: "hw.kbd", "set_mode mode=\"cycle\" speed={}", speed);
            }
            KeyboardMode::Dance { brightness, speed } => {
                if let Some(ref io) = self.tuxedo_io {
                    if io.get_interface() == HardwareInterface::Clevo {
                        let _ = io.set_clevo_keyboard_mode(0x80000000); // DANCE
                    }
                }
                self.write_effect_mode(3)?;
                self.write_effect_speed(*speed)?;
                self.set_brightness(*brightness)?;
                log::info!(target: "hw.kbd", "set_mode mode=\"dance\" speed={}", speed);
            }
            KeyboardMode::Flash { r, g, b, brightness, speed } => {
                if let Some(ref io) = self.tuxedo_io {
                    if io.get_interface() == HardwareInterface::Clevo {
                        let _ = io.set_clevo_keyboard_mode(0xA0000000); // FLASH
                    }
                }
                self.write_effect_mode(4)?;
                self.write_effect_speed(*speed)?;
                for i in 0..self.paths.len() {
                    let _ = self.set_zone_color(i, *r, *g, *b);
                }
                self.set_brightness(*brightness)?;
                log::info!(target: "hw.kbd", "set_mode mode=\"flash\" speed={}", speed);
            }
            KeyboardMode::RandomColor { brightness, speed } => {
                if let Some(ref io) = self.tuxedo_io {
                    if io.get_interface() == HardwareInterface::Clevo {
                        let _ = io.set_clevo_keyboard_mode(0x70000000); // RANDOM_COLOR
                    }
                }
                self.write_effect_mode(5)?;
                self.write_effect_speed(*speed)?;
                self.set_brightness(*brightness)?;
                log::info!(target: "hw.kbd", "set_mode mode=\"random\" speed={}", speed);
            }
            KeyboardMode::Tempo { brightness, speed } => {
                if let Some(ref io) = self.tuxedo_io {
                    if io.get_interface() == HardwareInterface::Clevo {
                        let _ = io.set_clevo_keyboard_mode(0x90000000); // TEMPO
                    }
                }
                self.write_effect_mode(6)?;
                self.write_effect_speed(*speed)?;
                self.set_brightness(*brightness)?;
                log::info!(target: "hw.kbd", "set_mode mode=\"tempo\" speed={}", speed);
            }
        }
        Ok(())
    }

    /// Writes the effect mode integer to every LED `mode` attribute.
    ///
    /// Routed through `guard_sysfs_write`: the path must be on
    /// APPROVED_SYSFS_PATHS and the payload must parse as a byte integer.
    /// The old call sites passed a string fallback token ("breathing") which
    /// the guard's LED check can never accept, so the fallback is gone.
    fn write_effect_mode(&self, mode_value: u8) -> Result<()> {
        for path in &self.paths {
            let mode_path = format!("{}/mode", path);
            if Path::new(&mode_path).exists() {
                guard_sysfs_write(&mode_path, &mode_value.to_string())?;
            }
        }
        Ok(())
    }

    /// Writes the effect speed integer to every LED `speed` attribute,
    /// validated by `guard_sysfs_write` the same way as `mode`.
    fn write_effect_speed(&self, speed: u8) -> Result<()> {
        for path in &self.paths {
            let speed_path = format!("{}/speed", path);
            if Path::new(&speed_path).exists() {
                guard_sysfs_write(&speed_path, &speed.to_string())?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: absolute sysfs paths must match the allowlist templates.
    ///
    /// `split_path` previously kept the empty leading segment an absolute path
    /// produces ("/sys/..." -> ["", "sys", ...]). `matches_template` rejects an
    /// empty non-wildcard segment, so every real path was refused with
    /// "blocked non-allowlisted sysfs write" and profile application never
    /// reached the hardware at all. Dropping empty segments restores matching.
    #[test]
    fn absolute_paths_match_sysfs_allowlist() {
        // The exact path the live daemon rejected 2026-09-16, plus a sibling
        // wildcard position and a non-wildcard entry for coverage.
        assert!(is_allowed_sysfs_path(
            "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"
        ));
        assert!(is_allowed_sysfs_path(
            "/sys/devices/system/cpu/cpu7/cpufreq/scaling_max_freq"
        ));
        assert!(is_allowed_sysfs_path("/sys/devices/system/cpu/cpufreq/boost"));
        assert!(is_allowed_sysfs_path("/sys/class/leds/kbd_backlight/mode"));

        // Still rejects unlisted attributes: the guard is not weakened.
        assert!(!is_allowed_sysfs_path(
            "/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq"
        ));
        assert!(!is_allowed_sysfs_path("/sys/class/hwmon/hwmon0/temp1_input"));
    }


    /// A profile whose earliest hardware step fails: `set_cpu_governor` runs
    /// first in `apply_profile_inner` and "invalid-governor-token" is not on
    /// GOVERNOR_ALLOWLIST, so `guard_sysfs_write` rejects it on any host.
    fn failing_profile() -> Profile {
        Profile {
            name: "regression-early-fail".to_string(),
            is_default: false,
            cpu_settings: CpuSettings {
                governor: Some("invalid-governor-token".to_string()),
                ..CpuSettings::default()
            },
            ..Profile::default()
        }
    }

    #[test]
    fn failing_apply_propagates_err_and_is_repeatable() {
        // Pre-fix: LAST_APPLIED_PROFILE was written before any hardware step
        // ran, so the FIRST apply failed but the SECOND identical apply hit the
        // "already applied" early return and returned Ok without touching
        // hardware. Post-fix both must run the hardware steps and fail.
        let p = failing_profile();

        let first = apply_profile(&p);
        assert!(first.is_err(), "first apply with a bad governor must fail");

        // Re-apply must NOT be suppressed: it re-executes the hardware steps.
        let second = apply_profile(&p);
        assert!(second.is_err(), "re-apply of the same profile after a failure must not be skipped");

        // Same step fails both times, proving re-execution rather than a
        // short-circuit via a stale marker.
        assert_eq!(
            format!("{}", first.unwrap_err()),
            format!("{}", second.unwrap_err()),
            "re-apply must fail at the same step, not be suppressed"
        );
    }

    #[test]
    fn failing_apply_clears_last_applied_marker() {
        // The invariant the bug broke: a failed apply must leave the marker
        // empty so the next identical apply is never suppressed.
        let p = failing_profile();
        assert!(apply_profile(&p).is_err());

        {
            let marker = lock_or_recover(&LAST_APPLIED_PROFILE, "LAST_APPLIED_PROFILE");
            assert!(marker.is_none(), "LAST_APPLIED_PROFILE must be None after a failed apply");
        }
    }

    #[test]
    fn profile_equality_keyes_the_skip_branch() {
        // The skip branch is keyed on Profile PartialEq; guard against a
        // future derive change silently breaking the contract.
        assert_eq!(&failing_profile(), &failing_profile());
    }

    /// Regression: poisoning LAST_APPLIED_PROFILE must not panic the daemon.
    ///
    /// A `.lock().unwrap()` here used to propagate the poison as a panic in
    /// the calling thread, killing the daemon. The recovery contract is
    /// into_inner() + clear_poison() + error log + carry on.
    #[test]
    fn poisoned_last_applied_profile_is_recovered_not_panic() {
        // Poison the mutex the real way: a thread that panics while holding
        // the guard. Unwinding drops the guard (so the mutex is unlocked) but
        // marks it poisoned — exactly the state a `.lock().unwrap()` used to
        // propagate as a daemon-killing panic.
        let marker_value = failing_profile();
        let handle = std::thread::spawn(move || {
            let mut g = LAST_APPLIED_PROFILE.lock().unwrap();
            *g = Some(marker_value);
            panic!("simulated holder panic (intentional)");
        });
        let _ = handle.join();

        // lock_or_recover must NOT panic: it takes the inner value, clears the
        // poison, logs, and hands back a usable guard.
        {
            let g = lock_or_recover(&LAST_APPLIED_PROFILE, "LAST_APPLIED_PROFILE");
            assert!(g.is_some(), "recovery must return the pre-poison value");
        }

        // The next plain lock must succeed again — poison cleared.
        {
            let g = LAST_APPLIED_PROFILE.lock().unwrap();
            assert!(g.is_some());
        }
    }
}
