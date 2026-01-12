use anyhow::{anyhow, Result};
use std::fs;
use std::path::Path;
use tuxedo_common::types::*;

pub fn get_system_info() -> Result<SystemInfo> {
    Ok(SystemInfo {
        product_name: read_dmi("product_name")?,
        manufacturer: read_dmi("sys_vendor")?,
        bios_version: read_dmi("bios_version")?,
    })
}

fn read_dmi(file: &str) -> Result<String> {
    let path = format!("/sys/devices/virtual/dmi/id/{}", file);
    fs::read_to_string(&path)
        .map(|s| s.trim().to_string())
        .map_err(|e| anyhow!("Failed to read {}: {}", path, e))
}

pub fn get_cpu_info() -> Result<CpuInfo> {
    let name = get_cpu_name()?;
    let core_count = get_cpu_count()?;
    
    let mut cores = Vec::new();
    let mut frequencies = Vec::new();
    
    for i in 0..core_count {
        let freq = read_cpu_frequency(i)?;
        let load = read_cpu_load(i).unwrap_or(0.0);
        frequencies.push(freq);
        
        cores.push(CoreInfo {
            id: i,
            frequency: freq,
            load,
            temperature: 0.0,
        });
    }
    
    let median_frequency = calculate_median(&frequencies);
    let package_temp = get_package_temp_k10temp().unwrap_or(0.0);
    let all_power_sources = get_all_power_sources();
    
    let power_source = all_power_sources.iter()
        .find(|s| s.name == "RAPL" || s.name == "amdgpu" || s.name == "zenpower")
        .cloned();
    
    let package_power = power_source.as_ref().map(|s| s.value);
    let power_source_name = power_source.as_ref().map(|s| s.name.clone());
    
    let governor = read_governor()?;
    let available_governors = read_available_governors()?;
    let boost_enabled = is_boost_enabled().unwrap_or(false);
    let smt_enabled = is_smt_enabled().unwrap_or(false);
    let scaling_driver = read_scaling_driver()?;
    let amd_pstate_status = read_amd_pstate_status().ok();
    
    let (min_freq, max_freq) = read_frequency_limits();
    let (hw_min_freq, hw_max_freq) = read_hw_frequency_limits()?;
    
    let available_pstate_controls = get_available_pstate_controls(&amd_pstate_status);
    
    let energy_performance_preference = read_epp_preference().ok();
    let available_epp_preferences = read_available_epp_preferences().unwrap_or_default();
    
    let total_load: f32 = cores.iter().map(|c| c.load).sum();
    let median_load = total_load / cores.len() as f32;

    Ok(CpuInfo {
        name,
        median_frequency,
        median_load,
        package_temp,
        package_power,
        cores,
        governor,
        available_governors,
        boost_enabled,
        smt_enabled,
        scaling_driver,
        amd_pstate_status,
        min_freq,
        max_freq,
        hw_min_freq,
        hw_max_freq,
        all_power_sources,
        power_source: power_source_name,
        available_pstate_controls,
        energy_performance_preference,
        available_epp_preferences,
    })
}

// Helper functions for CPU info

fn get_cpu_count() -> Result<u32> {
    let mut count = 0;
    for i in 0..1024 {
        let path = format!("/sys/devices/system/cpu/cpu{}", i);
        if !Path::new(&path).exists() {
            break;
        }
        count += 1;
    }
    if count == 0 {
        return Err(anyhow!("No CPUs found"));
    }
    Ok(count)
}

fn get_cpu_name() -> Result<String> {
    let cpuinfo = fs::read_to_string("/proc/cpuinfo")?;
    for line in cpuinfo.lines() {
        if line.starts_with("model name") {
            if let Some(name) = line.split(':').nth(1) {
                return Ok(name.trim().to_string());
            }
        }
    }
    Err(anyhow!("CPU name not found"))
}

fn read_cpu_frequency(cpu_id: u32) -> Result<u64> {
    let path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq", cpu_id);
    fs::read_to_string(&path)?
        .trim()
        .parse()
        .map_err(|e| anyhow!("Failed to parse frequency: {}", e))
}

fn read_cpu_load(cpu_id: u32) -> Result<f32> {
    // Read from /proc/stat
    let stat = fs::read_to_string("/proc/stat")?;
    let cpu_line = format!("cpu{} ", cpu_id);
    
    for line in stat.lines() {
        if line.starts_with(&cpu_line) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 5 {
                let user: u64 = parts[1].parse().unwrap_or(0);
                let nice: u64 = parts[2].parse().unwrap_or(0);
                let system: u64 = parts[3].parse().unwrap_or(0);
                let idle: u64 = parts[4].parse().unwrap_or(0);
                
                let total = user + nice + system + idle;
                let active = user + nice + system;
                
                if total > 0 {
                    return Ok((active as f32 / total as f32) * 100.0);
                }
            }
        }
    }
    Ok(0.0)
}

fn calculate_median(values: &[u64]) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len() / 2]
}

fn get_package_temp_k10temp() -> Result<f32> {
    for entry in fs::read_dir("/sys/class/hwmon")? {
        let entry = entry?;
        let name_path = entry.path().join("name");
        if let Ok(name) = fs::read_to_string(&name_path) {
            let name = name.trim();
            if name == "coretemp" || name == "k10temp" {
                let temp_path = entry.path().join("temp1_input");
                if let Ok(temp_str) = fs::read_to_string(&temp_path) {
                    if let Ok(temp) = temp_str.trim().parse::<f32>() {
                        return Ok(temp / 1000.0);
                    }
                }
            }
        }
    }
    Err(anyhow!("Package temperature not found"))
}

fn get_all_power_sources() -> Vec<PowerSource> {
    let mut sources = Vec::new();
    
    // Try RAPL
    if let Ok(entries) = fs::read_dir("/sys/class/powercap") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.join("name").exists() {
                if let Ok(name) = fs::read_to_string(path.join("name")) {
                    if name.trim().contains("package") {
                        if let Ok(energy) = fs::read_to_string(path.join("energy_uj")) {
                            if let Ok(energy_val) = energy.trim().parse::<f32>() {
                                sources.push(PowerSource {
                                    name: "RAPL".to_string(),
                                    value: energy_val / 1_000_000.0, // Convert to watts
                                    description: "Intel RAPL".to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Try AMD GPU power
    if let Ok(entries) = fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let power_path = entry.path().join("device/hwmon").join("hwmon0/power1_average");
            if power_path.exists() {
                if let Ok(power_str) = fs::read_to_string(&power_path) {
                    if let Ok(power_val) = power_str.trim().parse::<f32>() {
                        sources.push(PowerSource {
                            name: "amdgpu".to_string(),
                            value: power_val / 1_000_000.0,
                            description: "AMD GPU Power".to_string(),
                        });
                    }
                }
            }
        }
    }
    
    sources
}

fn read_governor() -> Result<String> {
    fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
        .map(|s| s.trim().to_string())
        .map_err(|e| anyhow!("Failed to read governor: {}", e))
}

fn read_available_governors() -> Result<Vec<String>> {
    let governors = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_available_governors")?;
    Ok(governors.split_whitespace().map(String::from).collect())
}

fn is_boost_enabled() -> Result<bool> {
    // Try AMD
    let amd_path = "/sys/devices/system/cpu/cpufreq/boost";
    if Path::new(amd_path).exists() {
        let val = fs::read_to_string(amd_path)?.trim().to_string();
        return Ok(val == "1");
    }
    
    // Try Intel (inverted)
    let intel_path = "/sys/devices/system/cpu/intel_pstate/no_turbo";
    if Path::new(intel_path).exists() {
        let val = fs::read_to_string(intel_path)?.trim().to_string();
        return Ok(val == "0");
    }
    
    Err(anyhow!("Boost control not available"))
}

fn is_smt_enabled() -> Result<bool> {
    let path = "/sys/devices/system/cpu/smt/active";
    if Path::new(path).exists() {
        let val = fs::read_to_string(path)?.trim().to_string();
        return Ok(val == "1");
    }
    Err(anyhow!("SMT control not available"))
}

fn read_scaling_driver() -> Result<String> {
    fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_driver")
        .map(|s| s.trim().to_string())
        .map_err(|e| anyhow!("Failed to read scaling driver: {}", e))
}

fn read_amd_pstate_status() -> Result<String> {
    fs::read_to_string("/sys/devices/system/cpu/amd_pstate/status")
        .map(|s| s.trim().to_string())
        .map_err(|e| anyhow!("Failed to read AMD pstate status: {}", e))
}

fn read_frequency_limits() -> (Option<u64>, Option<u64>) {
    let min = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_min_freq")
        .ok()
        .and_then(|s| s.trim().parse().ok());
    
    let max = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq")
        .ok()
        .and_then(|s| s.trim().parse().ok());
    
    (min, max)
}

fn read_hw_frequency_limits() -> Result<(u64, u64)> {
    let min = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_min_freq")?
        .trim()
        .parse()?;
    
    let max = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq")?
        .trim()
        .parse()?;
    
    Ok((min, max))
}

fn get_available_pstate_controls(pstate_status: &Option<String>) -> Vec<String> {
    let mut controls = vec![
        "boost".to_string(),
        "cpuinfo_max_freq".to_string(),
        "cpuinfo_min_freq".to_string(),
        "scaling_driver".to_string(),
        "scaling_governor".to_string(),
        "smt".to_string(),
    ];
    
    if let Some(status) = pstate_status {
        match status.as_str() {
            "active" | "passive" => {
                controls.push("energy_performance_preference".to_string());
            }
            _ => {}
        }
    }
    
    controls
}

fn read_epp_preference() -> Result<String> {
    fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference")
        .map(|s| s.trim().to_string())
        .map_err(|e| anyhow!("Failed to read EPP: {}", e))
}

fn read_available_epp_preferences() -> Result<Vec<String>> {
    let prefs = fs::read_to_string(
        "/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_available_preferences"
    )?;
    Ok(prefs.split_whitespace().map(String::from).collect())
}

// Battery detection
pub fn get_battery_info() -> Result<BatteryInfo> {
    let battery_path = find_battery_path()?;
    
    let voltage_mv = read_battery_value(&battery_path, "voltage_now")?;
    let current_ma = read_battery_value_signed(&battery_path, "current_now")?;
    let charge_percent = read_battery_value(&battery_path, "capacity")?;
    let capacity_mah = read_battery_value(&battery_path, "charge_full")?;
    
    let manufacturer = read_battery_string(&battery_path, "manufacturer")
        .unwrap_or_else(|_| "Unknown".to_string());
    let model = read_battery_string(&battery_path, "model_name")
        .unwrap_or_else(|_| "Unknown".to_string());
    
    let status_str = read_battery_string(&battery_path, "status")
        .unwrap_or_else(|_| "Unknown".to_string());
    
    let status = match status_str.as_str() {
        "Charging" => BatteryStatus::Charging,
        "Discharging" => BatteryStatus::Discharging,
        "Full" => BatteryStatus::Full,
        "Not charging" => BatteryStatus::NotCharging,
        _ => BatteryStatus::Unknown,
    };
    
    let on_ac_power = is_on_ac_power();
    
    let power_draw_w = if on_ac_power {
        None
    } else {
        Some((voltage_mv as f32 * current_ma.abs() as f32) / 1_000_000_000_000.0)
    };
    
    let charge_start_threshold = read_battery_value(&battery_path, "charge_control_start_threshold")
        .ok()
        .map(|v| v as u8);
    let charge_end_threshold = read_battery_value(&battery_path, "charge_control_end_threshold")
        .ok()
        .map(|v| v as u8);
    
    Ok(BatteryInfo {
        voltage_mv,
        current_ma,
        charge_percent,
        capacity_mah,
        manufacturer,
        model,
        charge_start_threshold,
        charge_end_threshold,
        status,
        power_draw_w,
        on_ac_power,
    })
}

fn find_battery_path() -> Result<String> {
    for i in 0..10 {
        let path = format!("/sys/class/power_supply/BAT{}", i);
        if Path::new(&path).exists() {
            return Ok(path);
        }
    }
    Err(anyhow!("No battery found"))
}

fn read_battery_value(battery_path: &str, file: &str) -> Result<u64> {
    let path = format!("{}/{}", battery_path, file);
    fs::read_to_string(&path)?
        .trim()
        .parse()
        .map_err(|e| anyhow!("Failed to parse {}: {}", file, e))
}

fn read_battery_value_signed(battery_path: &str, file: &str) -> Result<i64> {
    let path = format!("{}/{}", battery_path, file);
    fs::read_to_string(&path)?
        .trim()
        .parse()
        .map_err(|e| anyhow!("Failed to parse {}: {}", file, e))
}

fn read_battery_string(battery_path: &str, file: &str) -> Result<String> {
    let path = format!("{}/{}", battery_path, file);
    fs::read_to_string(&path)
        .map(|s| s.trim().to_string())
        .map_err(|e| anyhow!("Failed to read {}: {}", file, e))
}

fn is_on_ac_power() -> bool {
    for i in 0..10 {
        let path = format!("/sys/class/power_supply/AC{}/online", i);
        if let Ok(online) = fs::read_to_string(&path) {
            if online.trim() == "1" {
                return true;
            }
        }
        
        let path_alt = "/sys/class/power_supply/ACAD/online";
        if let Ok(online) = fs::read_to_string(path_alt) {
            if online.trim() == "1" {
                return true;
            }
        }
    }
    false
}

// WiFi detection
pub fn get_wifi_info() -> Result<Vec<WiFiInfo>> {
    let mut wifi_interfaces = Vec::new();
    
    for entry in fs::read_dir("/sys/class/net")? {
        let entry = entry?;
        let iface_name = entry.file_name().to_string_lossy().to_string();
        
        let wireless_path = entry.path().join("wireless");
        if !wireless_path.exists() {
            continue;
        }
        
        let driver = read_device_driver(&entry.path()).unwrap_or_else(|_| "Unknown".to_string());
        let chip_model = read_wifi_chip_model(&entry.path()).unwrap_or_else(|_| "Unknown".to_string());
        
        let link_speed_mbps = read_wifi_link_speed(&iface_name).ok();
        let signal_strength = read_wifi_signal(&iface_name).ok();
        let ssid = read_wifi_ssid(&iface_name).ok();
        let temperature = read_wifi_temperature(&driver).ok();
        
        wifi_interfaces.push(WiFiInfo {
            interface: iface_name,
            chip_model,
            driver,
            link_speed_mbps,
            signal_strength,
            ssid,
            temperature,
        });
    }
    
    Ok(wifi_interfaces)
}

fn read_device_driver(device_path: &Path) -> Result<String> {
    let driver_link = device_path.join("device/driver");
    let driver_path = fs::read_link(&driver_link)?;
    let driver_name = driver_path.file_name()
        .ok_or_else(|| anyhow!("Invalid driver path"))?
        .to_string_lossy()
        .to_string();
    Ok(driver_name)
}

fn read_wifi_chip_model(device_path: &Path) -> Result<String> {
    let modalias_path = device_path.join("device/modalias");
    if let Ok(modalias) = fs::read_to_string(&modalias_path) {
        return Ok(modalias.trim().to_string());
    }
    
    let vendor_path = device_path.join("device/vendor");
    let device_id_path = device_path.join("device/device");
    
    if let (Ok(vendor), Ok(device)) = (
        fs::read_to_string(&vendor_path),
        fs::read_to_string(&device_id_path)
    ) {
        return Ok(format!("{}:{}", vendor.trim(), device.trim()));
    }
    
    Err(anyhow!("Could not read chip model"))
}

fn read_wifi_link_speed(iface: &str) -> Result<u32> {
    let output = std::process::Command::new("iw")
        .args(&["dev", iface, "link"])
        .output()?;
    
    let output_str = String::from_utf8_lossy(&output.stdout);
    for line in output_str.lines() {
        if line.contains("tx bitrate:") {
            if let Some(speed_str) = line.split_whitespace().nth(2) {
                if let Ok(speed) = speed_str.parse::<f32>() {
                    return Ok(speed as u32);
                }
            }
        }
    }
    
    Err(anyhow!("Could not read link speed"))
}

fn read_wifi_signal(iface: &str) -> Result<i32> {
    let output = std::process::Command::new("iw")
        .args(&["dev", iface, "link"])
        .output()?;
    
    let output_str = String::from_utf8_lossy(&output.stdout);
    for line in output_str.lines() {
        if line.contains("signal:") {
            if let Some(signal_str) = line.split_whitespace().nth(1) {
                if let Ok(signal) = signal_str.parse::<i32>() {
                    return Ok(signal);
                }
            }
        }
    }
    
    Err(anyhow!("Could not read signal strength"))
}

fn read_wifi_ssid(iface: &str) -> Result<String> {
    let output = std::process::Command::new("iw")
        .args(&["dev", iface, "link"])
        .output()?;
    
    let output_str = String::from_utf8_lossy(&output.stdout);
    for line in output_str.lines() {
        if line.contains("SSID:") {
            if let Some(ssid) = line.split("SSID:").nth(1) {
                return Ok(ssid.trim().to_string());
            }
        }
    }
    
    Err(anyhow!("Not connected"))
}

fn read_wifi_temperature(driver: &str) -> Result<f32> {
    for entry in fs::read_dir("/sys/class/hwmon")? {
        let entry = entry?;
        let name_path = entry.path().join("name");
        if let Ok(name) = fs::read_to_string(&name_path) {
            if name.trim().to_lowercase().contains(driver) {
                let temp_path = entry.path().join("temp1_input");
                if let Ok(temp_str) = fs::read_to_string(&temp_path) {
                    if let Ok(temp) = temp_str.trim().parse::<f32>() {
                        return Ok(temp / 1000.0);
                    }
                }
            }
        }
    }
    Err(anyhow!("Temperature not available"))
}

// GPU detection
pub fn get_gpu_info() -> Result<Vec<GpuInfo>> {
    let mut gpus = Vec::new();
    
    for i in 0..4 {
        let card_path = format!("/sys/class/drm/card{}", i);
        if Path::new(&card_path).exists() {
            if let Ok(gpu) = detect_gpu_enhanced(&card_path, i) {
                gpus.push(gpu);
            }
        }
    }
    
    Ok(gpus)
}

fn detect_gpu_enhanced(path: &str, id: u32) -> Result<GpuInfo> {
    let device_path = format!("{}/device", path);
    
    let gpu_type = if id == 0 {
        GpuType::Integrated
    } else {
        GpuType::Discrete
    };
    
    let status_path = format!("{}/power/runtime_status", device_path);
    let status = if let Ok(status_str) = fs::read_to_string(&status_path) {
        match status_str.trim() {
            "active" => GpuStatus::Active,
            "suspended" => GpuStatus::Suspended,
            _ => GpuStatus::Unknown,
        }
    } else {
        GpuStatus::Unknown
    };
    
    let driver = read_device_driver(&Path::new(path)).ok();
    
    let vendor_path = format!("{}/vendor", device_path);
    let device_id_path = format!("{}/device", device_path);
    let pci_id = if let (Ok(vendor), Ok(device)) = (
        fs::read_to_string(&vendor_path),
        fs::read_to_string(&device_id_path)
    ) {
        Some(format!("{}:{}", vendor.trim(), device.trim()))
    } else {
        None
    };
    
    let name = format!("GPU {} ({})", id, match gpu_type {
        GpuType::Integrated => "iGPU",
        GpuType::Discrete => "dGPU",
    });
    
    Ok(GpuInfo {
        name,
        gpu_type,
        status,
        frequency: None,
        temperature: None,
        load: None,
        power: None,
        voltage: None,
        driver,
        pci_id,
    })
}

// Fan detection
pub fn get_fan_info() -> Result<Vec<FanInfo>> {
    let mut fans = Vec::new();
    
    if Path::new("/sys/devices/platform/tuxedo_io").exists() {
        fans = get_tuxedo_fans()?;
    }
    
    if fans.is_empty() {
        fans = get_hwmon_fans()?;
    }
    
    Ok(fans)
}

fn get_tuxedo_fans() -> Result<Vec<FanInfo>> {
    let mut fans = Vec::new();
    let base_path = "/sys/devices/platform/tuxedo_io";
    
    for i in 0..4 {
        let rpm_path = format!("{}/fan{}_rpm", base_path, i);
        let speed_path = format!("{}/fan{}_speed", base_path, i);
        
        if !Path::new(&rpm_path).exists() {
            break;
        }
        
        let rpm = fs::read_to_string(&rpm_path)
            .ok()
            .and_then(|s| s.trim().parse().ok());
        
        let duty_percent = fs::read_to_string(&speed_path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        
        fans.push(FanInfo {
            id: i,
            name: format!("Fan {}", i),
            rpm,
            duty_percent,
            temperature: None,
        });
    }
    
    Ok(fans)
}

fn get_hwmon_fans() -> Result<Vec<FanInfo>> {
    let mut fans = Vec::new();
    let mut fan_id = 0;
    
    for entry in fs::read_dir("/sys/class/hwmon")? {
        let entry = entry?;
        
        for i in 1..10 {
            let rpm_path = entry.path().join(format!("fan{}_input", i));
            if !rpm_path.exists() {
                break;
            }
            
            let rpm = fs::read_to_string(&rpm_path)
                .ok()
                .and_then(|s| s.trim().parse().ok());
            
            fans.push(FanInfo {
                id: fan_id,
                name: format!("Fan {}", fan_id),
                rpm,
                duty_percent: 0,
                temperature: None,
            });
            
            fan_id += 1;
        }
    }
    
    Ok(fans)
}

// Storage info (placeholder)
pub fn get_storage_info() -> Result<Vec<StorageInfo>> {
    let mut storage_devices = Vec::new();
    
    // Basic implementation - would need to be expanded
    for entry in fs::read_dir("/sys/block")? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        
        if name.starts_with("sd") || name.starts_with("nvme") {
            let size_path = entry.path().join("size");
            if let Ok(size_str) = fs::read_to_string(&size_path) {
                if let Ok(size_blocks) = size_str.trim().parse::<u64>() {
                    let size_gb = (size_blocks * 512) / (1024 * 1024 * 1024);
                    
                    storage_devices.push(StorageInfo {
                        device: format!("/dev/{}", name),
                        model: "Unknown".to_string(),
                        size_gb,
                        temperature: None,
                    });
                }
            }
        }
    }
    
    Ok(storage_devices)
}
