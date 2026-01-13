use anyhow::{anyhow, Result};
use nvml_wrapper::Nvml;
use std::fs;
use std::path::Path;
use std::collections::HashMap;
use std::sync::Mutex;
use crate::tuxedo_io::TuxedoIo;
use systemstat::{System, Platform, saturating_sub_bytes};
// use tuxedo_io::TuxedoIo;
use tuxedo_common::types::*;

// Thread-safe storage for previous CPU stats
static PREVIOUS_CPU_STATS: Mutex<Option<HashMap<u32, CpuStats>>> = Mutex::new(None);

#[derive(Debug, Clone)]
struct CpuStats {
    user: u64,
    nice: u64,
    system: u64,
    idle: u64,
    iowait: u64,
    irq: u64,
    softirq: u64,
}

impl CpuStats {
    fn total(&self) -> u64 {
        self.user + self.nice + self.system + self.idle + self.iowait + self.irq + self.softirq
    }
    
    fn work(&self) -> u64 {
        self.user + self.nice + self.system + self.irq + self.softirq
    }
}

fn read_cpu_stats() -> Result<HashMap<u32, CpuStats>> {
    let stat = fs::read_to_string("/proc/stat")?;
    let mut stats = HashMap::new();
    
    for line in stat.lines() {
        if line.starts_with("cpu") && !line.starts_with("cpu ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 8 {
                continue;
            }
            
            let cpu_id: u32 = parts[0].trim_start_matches("cpu").parse()?;
            let user: u64 = parts[1].parse()?;
            let nice: u64 = parts[2].parse()?;
            let system: u64 = parts[3].parse()?;
            let idle: u64 = parts[4].parse()?;
            let iowait: u64 = parts[5].parse()?;
            let irq: u64 = parts[6].parse()?;
            let softirq: u64 = parts[7].parse()?;
            
            stats.insert(cpu_id, CpuStats {
                user, nice, system, idle, iowait, irq, softirq,
            });
        }
    }
    
    Ok(stats)
}

fn calculate_cpu_load() -> Result<HashMap<u32, f32>> {
    let current_stats = read_cpu_stats()?;
    
    // Get previous stats from thread-safe storage
    let mut prev_stats_lock = PREVIOUS_CPU_STATS.lock().unwrap();
    
    let loads = if let Some(ref prev_stats) = *prev_stats_lock {
        // Calculate load based on delta from previous call
        let mut loads = HashMap::new();
        
        for (cpu_id, current) in current_stats.iter() {
            if let Some(prev) = prev_stats.get(cpu_id) {
                let total_diff = current.total().saturating_sub(prev.total());
                let work_diff = current.work().saturating_sub(prev.work());
                
                let load = if total_diff > 0 {
                    (work_diff as f32 / total_diff as f32) * 100.0
                } else {
                    0.0
                };
                
                loads.insert(*cpu_id, load);
            } else {
                // New CPU appeared, assume 0% load
                loads.insert(*cpu_id, 0.0);
            }
        }
        
        loads
    } else {
        // First call - no previous stats available, return 0% for all CPUs
        current_stats.keys().map(|&id| (id, 0.0)).collect()
    };
    
    // Store current stats for next call
    *prev_stats_lock = Some(current_stats);
    
    Ok(loads)
}

// Scheduler detection
fn get_scheduler_info() -> (String, Vec<String>) {
    let scheduler = fs::read_to_string("/sys/kernel/debug/sched/features")
        .or_else(|_| fs::read_to_string("/proc/sys/kernel/sched_features"))
        .ok()
        .and_then(|content| {
            if content.contains("EEVDF") {
                Some("EEVDF".to_string())
            } else {
                Some("CFS".to_string())
            }
        })
        .unwrap_or_else(|| "CFS".to_string());
    
    let available = vec!["CFS".to_string(), "EEVDF".to_string()];
    (scheduler, available)
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

fn get_cpu_count() -> Result<u32> {
    let cpuinfo = fs::read_to_string("/proc/cpuinfo")?;
    let count = cpuinfo.lines()
        .filter(|line| line.starts_with("processor"))
        .count();
    Ok(count as u32)
}

fn read_cpu_frequency(cpu: u32) -> Result<u64> {
    let path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq", cpu);
    if let Ok(s) = fs::read_to_string(&path) {
        if let Ok(freq) = s.trim().parse() {
            return Ok(freq);
        }
    }
    
    let path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/cpuinfo_cur_freq", cpu);
    if let Ok(s) = fs::read_to_string(&path) {
        if let Ok(freq) = s.trim().parse() {
            return Ok(freq);
        }
    }
    
    let cpuinfo = fs::read_to_string("/proc/cpuinfo")?;
    for line in cpuinfo.lines().skip((cpu * 30) as usize).take(30) {
        if line.starts_with("cpu MHz") {
            if let Some(mhz) = line.split(':').nth(1) {
                if let Ok(mhz_val) = mhz.trim().parse::<f64>() {
                    return Ok((mhz_val * 1000.0) as u64);
                }
            }
        }
    }
    
    Ok(2000000)
}

fn calculate_median(values: &[u64]) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len() / 2]
}

fn get_core_temp(cpu: u32) -> Result<f32> {
    for entry in fs::read_dir("/sys/class/hwmon")? {
        let entry = entry?;
        let name_path = entry.path().join("name");
        if let Ok(name) = fs::read_to_string(&name_path) {
            let name = name.trim();
            if name == "k10temp" {
                return get_package_temp();
            } else if name == "coretemp" {
                let temp_path = entry.path().join(format!("temp{}_input", cpu + 2));
                if let Ok(temp_str) = fs::read_to_string(&temp_path) {
                    if let Ok(temp) = temp_str.trim().parse::<f32>() {
                        return Ok(temp / 1000.0);
                    }
                }
            }
        }
    }
    Err(anyhow!("Core temperature not found"))
}

fn get_package_temp() -> Result<f32> {
    for entry in fs::read_dir("/sys/class/hwmon")? {
        let entry = entry?;
        let name_path = entry.path().join("name");
        if let Ok(name) = fs::read_to_string(&name_path) {
            let name = name.trim();
            if name == "k10temp" {
                let temp_path = entry.path().join("temp1_input");
                if let Ok(temp_str) = fs::read_to_string(&temp_path) {
                    if let Ok(temp) = temp_str.trim().parse::<f32>() {
                        return Ok(temp / 1000.0);
                    }
                }
            } else if name == "coretemp" {
                let temp_path = entry.path().join("temp1_input");
                if let Ok(temp_str) = fs::read_to_string(&temp_path) {
                    if let Ok(temp) = temp_str.trim().parse::<f32>() {
                        return Ok(temp / 1000.0);
                    }
                }
            } else if name == "zenpower" {
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

fn read_hwmon_power(hwmon_path: &Path) -> Result<f32> {
    let power_input_path = hwmon_path.join("power1_input");
    if let Ok(power_str) = fs::read_to_string(&power_input_path) {
        if let Ok(microwatts) = power_str.trim().parse::<f32>() {
            return Ok(microwatts / 1_000_000.0);
        }
    }
    
    let power_avg_path = hwmon_path.join("power1_average");
    if let Ok(power_str) = fs::read_to_string(&power_avg_path) {
        if let Ok(microwatts) = power_str.trim().parse::<f32>() {
            return Ok(microwatts / 1_000_000.0);
        }
    }
    
    Err(anyhow!("No power reading available"))
}

fn try_rapl() -> Result<f32> {
    for entry in fs::read_dir("/sys/class/powercap")? {
        let entry = entry?;
        let path = entry.path();
        
        if let Ok(name) = fs::read_to_string(path.join("name")) {
            if name.trim() == "package-0" {
                if let Ok(energy_str) = fs::read_to_string(path.join("energy_uj")) {
                    if let Ok(energy) = energy_str.trim().parse::<f64>() {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        if let Ok(energy2_str) = fs::read_to_string(path.join("energy_uj")) {
                            if let Ok(energy2) = energy2_str.trim().parse::<f64>() {
                                let diff = energy2 - energy;
                                let power = (diff / 100000.0) as f32;
                                return Ok(power);
                            }
                        }
                    }
                }
            }
        }
    }
    Err(anyhow!("RAPL not available"))
}

fn is_amd_cpu() -> bool {
    if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
        for line in cpuinfo.lines() {
            if line.starts_with("vendor_id") {
                return line.contains("AuthenticAMD");
            }
        }
    }
    false
}

fn get_amd_dgpu_count() -> u32 {
    let mut count = 0;
    if let Ok(entries) = fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name() {
                let name_str = name.to_string_lossy();
                if name_str.starts_with("card") && !name_str.contains("-") {
                    let device_path = path.join("device/vendor");
                    if let Ok(vendor) = fs::read_to_string(&device_path) {
                        if vendor.trim() == "0x1002" {
                            count += 1;
                        }
                    }
                }
            }
        }
    }
    if count > 1 { count - 1 } else { 0 }
}

fn get_all_power_sources() -> Vec<PowerSource> {
    let mut sources = Vec::new();
    
    if let Ok(power) = try_rapl() {
        sources.push(PowerSource {
            name: "RAPL".to_string(),
            value: power,
            description: "Intel/AMD RAPL (Running Average Power Limit)".to_string(),
        });
    }
    
    if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
        for entry in entries.flatten() {
            let name_path = entry.path().join("name");
            if let Ok(name) = fs::read_to_string(&name_path) {
                let name = name.trim();
                
                match name {
                    "amdgpu" => {
                        let power_input = entry.path().join("power1_input");
                        let power_avg = entry.path().join("power1_average");
                        
                        if power_input.exists() || power_avg.exists() {
                            if let Ok(power) = read_hwmon_power(&entry.path()) {
                                sources.push(PowerSource {
                                    name: "amdgpu".to_string(),
                                    value: power,
                                    description: "AMD APU Total Power (CPU+iGPU)".to_string(),
                                });
                            }
                        }
                    },
                    "zenpower" => {
                        if let Ok(power) = read_hwmon_power(&entry.path()) {
                            sources.push(PowerSource {
                                name: "zenpower".to_string(),
                                value: power,
                                description: "Zenpower Driver (AMD Ryzen)".to_string(),
                            });
                        }
                    },
                    "amd_energy" => {
                        if let Ok(power) = read_hwmon_power(&entry.path()) {
                            sources.push(PowerSource {
                                name: "amd_energy".to_string(),
                                value: power,
                                description: "AMD Energy Driver".to_string(),
                            });
                        }
                    },
                    _ => {}
                }
            }
        }
    }
    
    sources
}

fn get_cpu_power() -> Option<f32> {
    let all_sources = get_all_power_sources();
    
    if is_amd_cpu() && get_amd_dgpu_count() == 0 {
        if let Some(amdgpu) = all_sources.iter().find(|s| s.name == "amdgpu") {
            return Some(amdgpu.value);
        }
    }
    
    if is_amd_cpu() {
        if let Some(zenpower) = all_sources.iter().find(|s| s.name == "zenpower") {
            return Some(zenpower.value);
        }
        
        if let Some(amd_energy) = all_sources.iter().find(|s| s.name == "amd_energy") {
            return Some(amd_energy.value);
        }
    }
    
    if let Some(rapl) = all_sources.iter().find(|s| s.name == "RAPL") {
        return Some(rapl.value);
    }
    
    None
}

fn detect_cpu_capabilities() -> CpuCapabilities {
    let base_path = "/sys/devices/system/cpu/cpu0/cpufreq";
    
    CpuCapabilities {
        has_boost: Path::new("/sys/devices/system/cpu/cpufreq/boost").exists() ||
                   Path::new("/sys/devices/system/cpu/intel_pstate/no_turbo").exists(),
        
        has_cpuinfo_max_freq: Path::new(&format!("{}/cpuinfo_max_freq", base_path)).exists(),
        
        has_cpuinfo_min_freq: Path::new(&format!("{}/cpuinfo_min_freq", base_path)).exists(),
        
        has_scaling_driver: Path::new(&format!("{}/scaling_driver", base_path)).exists() ||
                           Path::new("/sys/devices/system/cpu/cpufreq/policy0/scaling_driver").exists(),
        
        has_energy_performance_preference: 
            Path::new(&format!("{}/energy_performance_preference", base_path)).exists(),
        
        has_scaling_governor: Path::new(&format!("{}/scaling_governor", base_path)).exists(),
        
        has_smt: Path::new("/sys/devices/system/cpu/smt/control").exists(),
        
        has_scaling_min_freq: Path::new(&format!("{}/scaling_min_freq", base_path)).exists(),
        
        has_scaling_max_freq: Path::new(&format!("{}/scaling_max_freq", base_path)).exists(),
        
        has_available_governors: 
            Path::new(&format!("{}/scaling_available_governors", base_path)).exists(),
        
        has_amd_pstate: Path::new("/sys/devices/system/cpu/amd_pstate/status").exists(),
    }
}

fn read_governor() -> Result<String> {
    let path = "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor";
    
    if !Path::new(path).exists() {
        return Ok("not_available".to_string());
    }
    
    fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .map_err(|e| anyhow!("Failed to read governor: {}", e))
}

fn read_available_governors() -> Result<Vec<String>> {
    let path = "/sys/devices/system/cpu/cpu0/cpufreq/scaling_available_governors";
    
    if !Path::new(path).exists() {
        return Ok(vec![]);
    }
    
    let governors = fs::read_to_string(path)?;
    Ok(governors.split_whitespace().map(String::from).collect())
}

fn is_boost_enabled() -> Result<bool> {
    if let Ok(boost) = fs::read_to_string("/sys/devices/system/cpu/cpufreq/boost") {
        return Ok(boost.trim() == "1");
    }
    
    if let Ok(no_turbo) = fs::read_to_string("/sys/devices/system/cpu/intel_pstate/no_turbo") {
        return Ok(no_turbo.trim() == "0");
    }
    
    Ok(false)
}

fn is_smt_enabled() -> Result<bool> {
    let path = "/sys/devices/system/cpu/smt/control";
    
    if !Path::new(path).exists() {
        return Ok(true);
    }
    
    let status = fs::read_to_string(path)?;
    Ok(status.trim() == "on")
}

fn read_scaling_driver() -> Result<String> {
    let path = "/sys/devices/system/cpu/cpufreq/policy0/scaling_driver";
    
    if !Path::new(path).exists() {
        return Ok("unknown".to_string());
    }
    
    fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .map_err(|e| anyhow!("Failed to read scaling driver: {}", e))
}

fn read_amd_pstate_status() -> Result<String> {
    let path = "/sys/devices/system/cpu/amd_pstate/status";
    fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .map_err(|e| anyhow!("Failed to read AMD pstate status: {}", e))
}

fn read_frequency_limits() -> (Option<u64>, Option<u64>) {
    let min_freq = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_min_freq")
        .ok()
        .and_then(|s| s.trim().parse().ok());
    
    let max_freq = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq")
        .ok()
        .and_then(|s| s.trim().parse().ok());
    
    (min_freq, max_freq)
}

fn read_hw_frequency_limits() -> Result<(Option<u64>, Option<u64>)> {
    let min_path = "/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_min_freq";
    let max_path = "/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq";

    let min_freq = fs::read_to_string(min_path).ok().and_then(|s| s.trim().parse().ok());
    let max_freq = fs::read_to_string(max_path).ok().and_then(|s| s.trim().parse().ok());

    Ok((min_freq, max_freq))
}

fn read_energy_performance_preference() -> Option<String> {
    let path = "/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference";
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
}

fn read_available_epp_options() -> Vec<String> {
    let path = "/sys/devices/system/cpu/cpu0/cpufreq/energy_performance_available_preferences";
    
    if let Ok(content) = fs::read_to_string(path) {
        content.split_whitespace().map(String::from).collect()
    } else {
        vec![
            "performance".to_string(),
            "balance_performance".to_string(),
            "balance_power".to_string(),
            "power".to_string(),
        ]
    }
}

pub fn get_tdp_profiles() -> Result<Vec<String>> {
    if !TuxedoIo::is_available() {
        log::info!("TDP profiles not available (/dev/tuxedo_io not present)");
        return Ok(vec![]);
    }
    
    match TuxedoIo::new() {
        Ok(io) => {
            match io.get_available_profiles() {
                Ok(profiles) => {
                    log::info!("Available TDP profiles: {:?}", profiles);
                    Ok(profiles)
                }
                Err(e) => {
                    log::warn!("Failed to get TDP profiles: {}", e);
                    Ok(vec![])
                }
            }
        }
        Err(e) => {
            log::warn!("Failed to open /dev/tuxedo_io: {}", e);
            Ok(vec![])
        }
    }
}

pub fn get_current_tdp_profile() -> Result<String> {
    if !TuxedoIo::is_available() {
        return Err(anyhow!("TDP profiles not available"));
    }
    
    // Since there's no direct "get current profile" ioctl,
    // we return a default message or the first profile
    let profiles = get_tdp_profiles()?;
    if profiles.is_empty() {
        return Err(anyhow!("No TDP profiles available"));
    }
    
    // Return the first profile as default since we can't detect the current one
    Ok(profiles[0].clone())
}

pub fn get_fan_speeds() -> Result<Vec<(u32, u32)>> {
    if !TuxedoIo::is_available() {
        return Ok(vec![]);
    }
    
    let io = TuxedoIo::new()?;
    let mut fans = Vec::new();
    
    for fan_id in 0..io.get_fan_count() {
        match io.get_fan_speed(fan_id) {
            Ok(speed) => {
                if speed > 0 {
                    fans.push((fan_id, speed));
                }
            }
            Err(_) => break,
        }
    }
    
    Ok(fans)
}

pub fn get_fan_temperatures() -> Result<Vec<(u32, u32)>> {
    if !TuxedoIo::is_available() {
        return Ok(vec![]);
    }
    
    let io = TuxedoIo::new()?;
    let mut temps = Vec::new();
    
    for fan_id in 0..io.get_fan_count() {
        match io.get_fan_temperature(fan_id) {
            Ok(temp) => {
                if temp > 0 {
                    temps.push((fan_id, temp));
                }
            }
            Err(_) => break,
        }
    }
    
    Ok(temps)
}

pub fn get_tdp_info() -> Result<(i32, i32, i32)> {
    if !TuxedoIo::is_available() {
        return Err(anyhow!("TDP info not available"));
    }
    
    let io = TuxedoIo::new()?;
    
    // Try to get TDP0 (main TDP)
    let current = io.get_tdp(0)?;
    let min = io.get_tdp_min(0)?;
    let max = io.get_tdp_max(0)?;
    
    Ok((current, min, max))
}

pub fn get_cpu_info() -> Result<CpuInfo> {
    let name = get_cpu_name()?;
    let core_count = get_cpu_count()?;
    
    let loads = calculate_cpu_load().unwrap_or_default();
    
    let mut cores = Vec::new();
    let mut frequencies = Vec::new();
    
    for i in 0..core_count {
        let freq = read_cpu_frequency(i).unwrap_or(2000000);
        frequencies.push(freq);
        cores.push(CoreInfo {
            id: i,
            frequency: freq,
            load: loads.get(&i).copied().unwrap_or(0.0),
            temperature: get_core_temp(i).unwrap_or(0.0),
        });
    }
    
    let median_frequency = calculate_median(&frequencies);
    
    let loads_vec: Vec<f32> = loads.values().copied().collect();
    let median_load = if !loads_vec.is_empty() {
        let mut sorted = loads_vec.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        if sorted.len() % 2 == 0 {
            let mid = sorted.len() / 2;
            (sorted[mid - 1] + sorted[mid]) / 2.0
        } else {
            sorted[sorted.len() / 2]
        }
    } else {
        0.0
    };
    
    let package_temp = get_package_temp().unwrap_or(0.0);
    let package_power = get_cpu_power();
    
    let capabilities = detect_cpu_capabilities();
    
    let governor = if capabilities.has_scaling_governor {
        read_governor().unwrap_or_else(|_| "unknown".to_string())
    } else {
        "not_available".to_string()
    };
    
    let available_governors = if capabilities.has_available_governors {
        read_available_governors().unwrap_or_else(|_| vec![])
    } else {
        vec![]
    };
    
    let boost_enabled = if capabilities.has_boost {
        is_boost_enabled().unwrap_or(false)
    } else {
        false
    };
    
    let smt_enabled = if capabilities.has_smt {
        is_smt_enabled().unwrap_or(true)
    } else {
        true
    };
    
    let scaling_driver = if capabilities.has_scaling_driver {
        read_scaling_driver().unwrap_or_else(|_| "unknown".to_string())
    } else {
        "not_available".to_string()
    };
    
    let amd_pstate_status = if capabilities.has_amd_pstate {
        read_amd_pstate_status().ok()
    } else {
        None
    };
    
    let (min_freq, max_freq) = if capabilities.has_scaling_min_freq && capabilities.has_scaling_max_freq {
        read_frequency_limits()
    } else {
        (None, None)
    };
    
    let (hw_min_freq, hw_max_freq) = if capabilities.has_cpuinfo_min_freq && capabilities.has_cpuinfo_max_freq {
        read_hw_frequency_limits().unwrap_or((None, None))
    } else {
        (None, None)
    };
    
    let energy_performance_preference = if capabilities.has_energy_performance_preference {
        read_energy_performance_preference()
    } else {
        None
    };
    
    let available_epp_options = if capabilities.has_energy_performance_preference {
        read_available_epp_options()
    } else {
        vec![]
    };

    let all_power_sources = get_all_power_sources();
    
    let power_source = all_power_sources.iter()
        .find(|s| s.name == "amdgpu")
        .or_else(|| all_power_sources.iter().find(|s| s.name == "RAPL"))
        .cloned()
        .map(|s| s.name);

    let (scheduler, available_schedulers) = get_scheduler_info();

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
        power_source,
        energy_performance_preference,
        available_epp_options,
        capabilities,
        scheduler,
        available_schedulers,
    })
}

pub fn get_memory_info() -> Result<MemoryInfo> {
    let sys = System::new();
    let mem = sys.memory()?;

    let total_gib = mem.total.as_u64() as f64 / (1024.0 * 1024.0 * 1024.0);
    let free_gib = mem.free.as_u64() as f64 / (1024.0 * 1024.0 * 1024.0);
    let available_gib = mem.platform_memory.meminfo.get("MemAvailable").map_or(0.0, |v| v.as_u64() as f64 / (1024.0 * 1024.0 * 1024.0));
    let used_gib = total_gib - available_gib;
    let used_percent = if total_gib > 0.0 { (used_gib / total_gib * 100.0) as f32 } else { 0.0 };

    Ok(MemoryInfo {
        total_gib,
        used_gib,
        free_gib,
        available_gib,
        used_percent,
    })
}

pub fn get_system_info() -> Result<SystemInfo> {
    let product_name = fs::read_to_string("/sys/class/dmi/id/product_name")
        .unwrap_or_else(|_| "Unknown".to_string())
        .trim()
        .to_string();
    
    let manufacturer = fs::read_to_string("/sys/class/dmi/id/sys_vendor")
        .unwrap_or_else(|_| "Unknown".to_string())
        .trim()
        .to_string();
    
    let bios_version = fs::read_to_string("/sys/class/dmi/id/bios_version")
        .unwrap_or_else(|_| "Unknown".to_string())
        .trim()
        .to_string();
    
    Ok(SystemInfo {
        product_name,
        manufacturer,
        bios_version,
    })
}

pub fn get_gpu_info() -> Result<Vec<GpuInfo>> {
    let mut gpus = Vec::new();
    
    // First, try to get NVIDIA GPU info via NVML
    if Path::new("/sys/bus/pci/drivers/nvidia").exists() {
        if let Ok(nvidia_gpus) = get_nvidia_gpu_info() {
            for mut gpu in nvidia_gpus {
                // NVIDIA discrete GPU, not integrated
                gpu.gpu_type = GpuType::Discrete;
                gpus.push(gpu);
            }
        }
    }

    // Also get iGPU info from /sys/class/drm for Intel/AMD integrated graphics
    for i in 0..4 {
        let card_path = format!("/sys/class/drm/card{}", i);
        if !Path::new(&card_path).exists() {
            continue;
        }
        
        let device_path = format!("{}/device", card_path);
        let vendor_path = format!("{}/vendor", device_path);
        
        if let Ok(vendor) = fs::read_to_string(&vendor_path) {
            let vendor = vendor.trim();
            
            // Skip NVIDIA GPUs as we already got them from NVML
            if vendor == "0x10de" {
                continue;
            }
            
            let device_id_path = format!("{}/device", device_path);
            let device_id = fs::read_to_string(&device_id_path)
                .unwrap_or_else(|_| "unknown".to_string())
                .trim()
                .to_string();
            
            let name = match vendor {
                "0x1002" => get_amd_gpu_name(&device_id).unwrap_or_else(|| format!("AMD iGPU")),
                "0x8086" => get_intel_gpu_name(&device_id).unwrap_or_else(|| format!("Intel iGPU")),
                _ => format!("GPU {}", i),
            };
            
            let gpu_type = GpuType::Integrated;
            
            let status_path = format!("{}/power/runtime_status", device_path);
            let status = fs::read_to_string(&status_path)
                .unwrap_or_else(|_| "active".to_string())
                .trim()
                .to_string();
            
            // Read frequency
            let frequency = read_gpu_frequency(&device_path);
            
            // Read memory frequency for iGPUs
            let memory_frequency = read_gpu_memory_frequency(&device_path);
            
            // Read temperature
            let temperature = read_gpu_temperature(&device_path);
            
            // Read load
            let load = read_gpu_load(&device_path);
            
            // Read power
            let power = read_gpu_power(&device_path);
            
            // Read voltage (optional)
            let voltage = read_gpu_voltage(&device_path);
            
            gpus.push(GpuInfo {
                name,
                gpu_type,
                status,
                frequency,
                memory_frequency,
                temperature,
                vram_temperature: None,
                load,
                power,
                voltage,
            });
        }
    }
    
    if gpus.is_empty() {
        return Err(anyhow!("No GPUs detected"));
    }
    
    Ok(gpus)
}

/// Get AMD GPU name from device ID
fn get_amd_gpu_name(device_id: &str) -> Option<String> {
    // Common AMD iGPU device IDs
    match device_id {
        // Radeon Vega series (Ryzen APUs)
        "0x15dd" => Some("AMD Radeon Vega 8".to_string()),
        "0x15d8" => Some("AMD Radeon Vega 3".to_string()),
        "0x1636" => Some("AMD Radeon Graphics (Renoir)".to_string()),
        "0x1638" => Some("AMD Radeon Graphics (Cezanne)".to_string()),
        // RDNA2/RDNA3 iGPUs
        "0x164c" => Some("AMD Radeon 680M".to_string()),
        "0x164d" => Some("AMD Radeon 660M".to_string()),
        "0x15bf" => Some("AMD Radeon 780M".to_string()),
        "0x15c8" => Some("AMD Radeon 760M".to_string()),
        _ => None,
    }
}

/// Get Intel GPU name from device ID
fn get_intel_gpu_name(device_id: &str) -> Option<String> {
    // Common Intel iGPU device IDs
    match device_id {
        // Intel UHD Graphics
        "0x3ea0" | "0x3ea5" => Some("Intel UHD Graphics 620".to_string()),
        "0x9a49" => Some("Intel UHD Graphics (Tiger Lake)".to_string()),
        "0x9a78" => Some("Intel UHD Graphics (Rocket Lake)".to_string()),
        // Intel Iris Xe
        "0x9a40" | "0x9a60" => Some("Intel Iris Xe Graphics".to_string()),
        "0xa7a0" | "0xa7a1" => Some("Intel Iris Xe Graphics (Alder Lake)".to_string()),
        // Intel Iris Plus
        "0x8a52" | "0x8a5a" => Some("Intel Iris Plus Graphics".to_string()),
        // Intel Arc
        "0x56a0" | "0x56a1" => Some("Intel Arc Graphics (DG2)".to_string()),
        _ => None,
    }
}

use crate::hardware_control::get_nvml;
use nvml_wrapper::enum_wrappers::device::{Clock, PerformanceState};

pub fn get_gpu_clock_ranges(device_index: u32) -> Result<(u32, u32)> {
    let nvml = get_nvml()?;
    let device = nvml.device_by_index(device_index)?;

    // First, get the memory clocks to pass to supported_graphics_clocks
    let mut mem_clocks = device.supported_memory_clocks()?;
    if mem_clocks.is_empty() {
        return Err(anyhow!("No supported memory clocks found, cannot determine graphics clock ranges"));
    }
    mem_clocks.sort_unstable();
    // Use the highest memory clock to get the widest range of graphics clocks
    let target_mem_clock = *mem_clocks.last().unwrap();

    let mut clocks = device.supported_graphics_clocks(target_mem_clock)?;
    if clocks.is_empty() {
        return Err(anyhow!("No supported graphics clocks found for locking"));
    }
    clocks.sort_unstable();
    let min_clock = *clocks.first().unwrap();
    let max_clock = *clocks.last().unwrap();
    Ok((min_clock, max_clock))
}

pub fn get_gpu_mem_clock_ranges(device_index: u32) -> Result<Vec<u32>> {
    let nvml = get_nvml()?;
    let device = nvml.device_by_index(device_index)?;
    let clocks = device.supported_memory_clocks()?;
    Ok(clocks)
}

pub fn get_gpu_core_offset_limits(device_index: u32) -> Result<(i32, i32)> {
    let nvml = get_nvml()?;
    let device = nvml.device_by_index(device_index)?;
    let offset_info = device.clock_offset(Clock::Graphics, PerformanceState::Zero)?;
    Ok((offset_info.min_clock_offset_mhz, offset_info.max_clock_offset_mhz))
}

pub fn get_gpu_memory_offset_limits(device_index: u32) -> Result<(i32, i32)> {
    let nvml = get_nvml()?;
    let device = nvml.device_by_index(device_index)?;
    let offset_info = device.clock_offset(Clock::Memory, PerformanceState::Zero)?;
    Ok((offset_info.min_clock_offset_mhz, offset_info.max_clock_offset_mhz))
}

fn get_nvidia_gpu_info() -> Result<Vec<GpuInfo>> {
    let nvml = get_nvml()?;
    let mut gpus = Vec::new();

    // Try to get fan 2 temperature for VRAM temperature
    let vram_temp = if TuxedoIo::is_available() {
        TuxedoIo::new().ok().and_then(|io| {
            // Fan 2 is fan_id 1 (0-indexed)
            io.get_fan_temperature(1).ok().map(|t| t as f32)
        })
    } else {
        None
    };

    let device_count = nvml.device_count()?;
    for i in 0..device_count {
        let device = nvml.device_by_index(i)?;

        let name = device.name()?;
        let gpu_type = if i == 0 {
            GpuType::Integrated
        } else {
            GpuType::Discrete
        };
        
        // Get performance state for status
        let status = match device.performance_state() {
            Ok(state) => format!("{:?}", state),
            Err(_) => {
                // Fall back to power state
                match device.power_state() {
                    Ok(state) => format!("{:?}", state),
                    Err(_) => "Active".to_string(),
                }
            }
        };
        
        let frequency = device.clock_info(nvml_wrapper::enum_wrappers::device::Clock::Graphics).ok().map(|c| c as u64);
        let memory_frequency = device.clock_info(nvml_wrapper::enum_wrappers::device::Clock::Memory).ok().map(|c| c as u64);
        let temperature = device.temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu).ok().map(|t| t as f32);
        let load = device.utilization_rates().ok().map(|u| u.gpu as f32);
        let power = device.power_usage().ok().map(|p| p as f32 / 1000.0);
        let voltage = None;
        let vram_temperature = vram_temp;

        gpus.push(GpuInfo {
            name,
            gpu_type,
            status,
            frequency,
            memory_frequency,
            temperature,
            vram_temperature,
            load,
            power,
            voltage,
        });
    }

    Ok(gpus)
}

fn read_gpu_frequency(device_path: &str) -> Option<u64> {
    // AMD
    if let Ok(freq_str) = fs::read_to_string(format!("{}/pp_dpm_sclk", device_path)) {
        for line in freq_str.lines() {
            if line.contains('*') {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    // Handle both "Mhz" and "MHz" patterns
                    let freq_str = parts[1].trim_end_matches("Mhz").trim_end_matches("MHz");
                    if let Ok(freq) = freq_str.parse::<u64>() {
                        return Some(freq);
                    }
                }
            }
        }
    }
    
    // Intel
    if let Ok(freq_str) = fs::read_to_string(format!("{}/gt_cur_freq_mhz", device_path)) {
        if let Ok(freq) = freq_str.trim().parse::<u64>() {
            return Some(freq);
        }
    }
    
    None
}

fn read_gpu_memory_frequency(device_path: &str) -> Option<u64> {
    // AMD - memory clock from pp_dpm_mclk
    if let Ok(freq_str) = fs::read_to_string(format!("{}/pp_dpm_mclk", device_path)) {
        for line in freq_str.lines() {
            if line.contains('*') {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    // Handle both "Mhz" and "MHz" patterns
                    let freq_str = parts[1].trim_end_matches("Mhz").trim_end_matches("MHz");
                    if let Ok(freq) = freq_str.parse::<u64>() {
                        return Some(freq);
                    }
                }
            }
        }
    }
    
    // Intel doesn't typically expose memory frequency separately
    None
}

fn read_gpu_temperature(device_path: &str) -> Option<f32> {
    // Check hwmon
    let hwmon_path = format!("{}/hwmon", device_path);
    if let Ok(entries) = fs::read_dir(&hwmon_path) {
        for entry in entries.flatten() {
            let temp_input = entry.path().join("temp1_input");
            if let Ok(temp_str) = fs::read_to_string(&temp_input) {
                if let Ok(temp) = temp_str.trim().parse::<f32>() {
                    return Some(temp / 1000.0);
                }
            }
        }
    }
    
    // AMD specific
    if let Ok(temp_str) = fs::read_to_string(format!("{}/gpu_busy_percent", device_path)) {
        if let Ok(temp) = temp_str.trim().parse::<f32>() {
            return Some(temp);
        }
    }
    
    None
}

fn read_gpu_load(device_path: &str) -> Option<f32> {
    // AMD
    if let Ok(load_str) = fs::read_to_string(format!("{}/gpu_busy_percent", device_path)) {
        if let Ok(load) = load_str.trim().parse::<f32>() {
            return Some(load);
        }
    }
    
    // Intel
    if let Ok(load_str) = fs::read_to_string(format!("{}/gt_RP0_freq_mhz", device_path)) {
        // Intel doesn't directly expose load, would need calculation
    }
    
    None
}

fn read_gpu_power(device_path: &str) -> Option<f32> {
    let hwmon_path = format!("{}/hwmon", device_path);
    if let Ok(entries) = fs::read_dir(&hwmon_path) {
        for entry in entries.flatten() {
            // Try power1_average first
            let power_avg = entry.path().join("power1_average");
            if let Ok(power_str) = fs::read_to_string(&power_avg) {
                if let Ok(microwatts) = power_str.trim().parse::<f32>() {
                    return Some(microwatts / 1_000_000.0);
                }
            }
            
            // Try power1_input
            let power_input = entry.path().join("power1_input");
            if let Ok(power_str) = fs::read_to_string(&power_input) {
                if let Ok(microwatts) = power_str.trim().parse::<f32>() {
                    return Some(microwatts / 1_000_000.0);
                }
            }
        }
    }
    
    None
}

fn read_gpu_voltage(device_path: &str) -> Option<f32> {
    let hwmon_path = format!("{}/hwmon", device_path);
    if let Ok(entries) = fs::read_dir(&hwmon_path) {
        for entry in entries.flatten() {
            let voltage_input = entry.path().join("in0_input");
            if let Ok(volt_str) = fs::read_to_string(&voltage_input) {
                if let Ok(millivolts) = volt_str.trim().parse::<f32>() {
                    return Some(millivolts / 1000.0);
                }
            }
        }
    }
    
    None
}

// WiFi information detection
pub fn get_wifi_info() -> Result<Vec<WiFiInfo>> {
    let mut wifi_devices = Vec::new();
    
    // Find WiFi network interfaces
    let net_path = Path::new("/sys/class/net");
    if !net_path.exists() {
        return Err(anyhow!("Network interfaces not found"));
    }
    
    for entry in fs::read_dir(net_path)? {
        let entry = entry?;
        let interface = entry.file_name().to_string_lossy().to_string();
        
        // Check if it's a wireless interface
        let wireless_path = format!("/sys/class/net/{}/wireless", interface);
        if !Path::new(&wireless_path).exists() {
            continue;
        }
        
        // Get driver name
        let driver_path = format!("/sys/class/net/{}/device/driver/module", interface);
        let driver = if let Ok(link) = fs::read_link(&driver_path) {
            link.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        } else {
            "unknown".to_string()
        };
        
        // Read temperature if available
        let temp_path = format!("/sys/class/net/{}/device/hwmon", interface);
        let temperature = if let Ok(hwmon_entries) = fs::read_dir(&temp_path) {
            let mut temp = None;
            for hwmon_entry in hwmon_entries.flatten() {
                let temp_input_path = hwmon_entry.path().join("temp1_input");
                if let Ok(temp_str) = fs::read_to_string(&temp_input_path) {
                    if let Ok(temp_millidegrees) = temp_str.trim().parse::<i32>() {
                        temp = Some(temp_millidegrees as f32 / 1000.0);
                        break;
                    }
                }
            }
            temp
        } else {
            None
        };
        
        // Read signal level from /proc/net/wireless
        let signal_level = read_wifi_signal(&interface);
        
        // Read channel and rates from iwconfig or iw
        let (channel, channel_width) = read_wifi_channel(&interface);
        let (tx_rate, rx_rate) = read_wifi_rates(&interface);
        
        wifi_devices.push(WiFiInfo {
            interface,
            driver,
            temperature,
            signal_level,
            channel,
            channel_width,
            tx_rate,
            rx_rate,
        });
    }
    
    if wifi_devices.is_empty() {
        return Err(anyhow!("No WiFi devices found"));
    }
    
    Ok(wifi_devices)
}

fn read_wifi_signal(interface: &str) -> Option<i32> {
    // Read from /proc/net/wireless
    // Format: Inter-| sta-|   Quality        |   Discarded packets               | Missed | WE
    //  face | tus | link level noise |  nwid  crypt   frag  retry   misc | beacon | 22
    if let Ok(wireless) = fs::read_to_string("/proc/net/wireless") {
        for line in wireless.lines().skip(2) {
            if line.contains(interface) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 {
                    // Signal level is typically in parts[3] (in dBm, negative value)
                    if let Ok(signal) = parts[3].trim_end_matches('.').parse::<i32>() {
                        return Some(signal);
                    }
                }
            }
        }
    }
    None
}

fn read_wifi_channel(interface: &str) -> (Option<u32>, Option<u32>) {
    // Try to use iw command first (more modern)
    if let Ok(output) = std::process::Command::new("iw")
        .args(&["dev", interface, "info"])
        .output()
    {
        if output.status.success() {
            let info = String::from_utf8_lossy(&output.stdout);
            let mut channel = None;
            let mut width = None;
            
            for line in info.lines() {
                if line.contains("channel") {
                    // Parse: "channel 36 (5180 MHz), width: 80 MHz"
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    for (i, part) in parts.iter().enumerate() {
                        if *part == "channel" && i + 1 < parts.len() {
                            channel = parts[i + 1].parse().ok();
                        }
                        if *part == "width:" && i + 1 < parts.len() {
                            width = parts[i + 1].trim_end_matches(',').parse().ok();
                        }
                    }
                }
            }
            
            return (channel, width);
        }
    }
    
    // Fallback to iwconfig (older tool)
    if let Ok(output) = std::process::Command::new("iwconfig")
        .arg(interface)
        .output()
    {
        if output.status.success() {
            let info = String::from_utf8_lossy(&output.stdout);
            for line in info.lines() {
                if line.contains("Channel") || line.contains("Frequency") {
                    // Parse various formats
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    for (i, part) in parts.iter().enumerate() {
                        if part.contains("Channel:") || part.contains("Channel=") {
                            if let Some(ch_str) = part.split(&[':', '=']).nth(1) {
                                if let Ok(ch) = ch_str.parse() {
                                    return (Some(ch), None);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    
    (None, None)
}

fn read_wifi_rates(interface: &str) -> (Option<f64>, Option<f64>) {
    // Try to read from /sys/class/net/{interface}/statistics/
    let tx_bytes_path = format!("/sys/class/net/{}/statistics/tx_bytes", interface);
    let rx_bytes_path = format!("/sys/class/net/{}/statistics/rx_bytes", interface);
    
    // Note: This gives total bytes, not rates. Actual rate calculation would require
    // storing previous values and time, similar to CPU load calculation.
    // For now, we'll try to use iw to get link speed
    
    if let Ok(output) = std::process::Command::new("iw")
        .args(&["dev", interface, "link"])
        .output()
    {
        if output.status.success() {
            let info = String::from_utf8_lossy(&output.stdout);
            for line in info.lines() {
                if line.contains("tx bitrate:") {
                    // Parse: "tx bitrate: 866.7 MBit/s"
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    for (i, part) in parts.iter().enumerate() {
                        if (*part == "bitrate:" || *part == "tx" || *part == "rx") && i + 1 < parts.len() {
                            if let Ok(rate) = parts[i + 1].parse::<f64>() {
                                // Assume both tx and rx are similar for now
                                return (Some(rate), Some(rate));
                            }
                        }
                    }
                }
            }
        }
    }
    
    (None, None)
}

pub fn get_battery_info() -> Result<BatteryInfo> {
    let base = if Path::new("/sys/class/power_supply/BAT0").exists() {
        "/sys/class/power_supply/BAT0"
    } else if Path::new("/sys/class/power_supply/BAT1").exists() {
        "/sys/class/power_supply/BAT1"
    } else {
        return Err(anyhow!("No battery found"));
    };

    let status = read_sysfs_string(&format!("{}/status", base)).unwrap_or_else(|_| "Unknown".to_string());

    Ok(BatteryInfo {
        status,
        voltage_mv: read_sysfs_u64(&format!("{}/voltage_now", base))? / 1000,
        current_ma: read_sysfs_i64(&format!("{}/current_now", base))? / 1000,
        charge_percent: read_sysfs_u64(&format!("{}/capacity", base))?,
        capacity_mah: read_sysfs_u64(&format!("{}/charge_full", base))? / 1000,
        manufacturer: read_sysfs_string(&format!("{}/manufacturer", base))?,
        model: read_sysfs_string(&format!("{}/model_name", base))?,
        charge_start_threshold: read_sysfs_u64(&format!("{}/charge_control_start_threshold", base)).ok().map(|v| v as u8),
        charge_end_threshold: read_sysfs_u64(&format!("{}/charge_control_end_threshold", base)).ok().map(|v| v as u8),
    })
}

pub fn get_mount_info() -> Result<Vec<MountInfo>> {
    let sys = System::new();
    let mut mounts_info = Vec::new();

    if let Ok(mounts) = sys.mounts() {
        for mount in mounts.iter().filter(|m| m.fs_mounted_on == "/" || m.fs_mounted_on == "/home") {
            let total = mount.total.as_u64();
            let avail = mount.avail.as_u64();
            let used = total - avail;
            let used_percent = if total > 0 { (used as f64 / total as f64) * 100.0 } else { 0.0 };

            mounts_info.push(MountInfo {
                mount_point: mount.fs_mounted_on.clone(),
                filesystem_type: mount.fs_type.clone(),
                total_gb: total / 1_000_000_000,
                used_gb: used / 1_000_000_000,
                used_percent,
            });
        }
    }

    Ok(mounts_info)
}

fn read_sysfs_u64(path: &str) -> Result<u64> {
    Ok(fs::read_to_string(path)?.trim().parse()?)
}

fn read_sysfs_i64(path: &str) -> Result<i64> {
    Ok(fs::read_to_string(path)?.trim().parse()?)
}

fn read_sysfs_string(path: &str) -> Result<String> {
    Ok(fs::read_to_string(path)?.trim().to_string())
}

pub fn get_storage_device_info() -> Result<Vec<StorageDevice>> {
    let mut storage_devices = Vec::new();

    for entry in std::fs::read_dir("/sys/block")? {
        let entry = entry?;
        let dev_name = entry.file_name().to_string_lossy().to_string();

        if dev_name.starts_with("loop") || dev_name.starts_with("ram") {
            continue;
        }

        let path = entry.path();
        let model = std::fs::read_to_string(path.join("device/model"))
            .unwrap_or_else(|_| dev_name.clone())
            .trim()
            .to_string();

        let size_gb = if let Ok(size_str) = std::fs::read_to_string(path.join("size")) {
            if let Ok(sectors) = size_str.trim().parse::<u64>() {
                (sectors * 512) / 1_000_000_000
            } else {
                0
            }
        } else {
            0
        };

        // Try to read temperature from hwmon
        let mut temperature = None;
        if let Ok(hwmon_entries) = std::fs::read_dir(path.join("device/hwmon")) {
            for hwmon_entry in hwmon_entries.flatten() {
                let temp_input = hwmon_entry.path().join("temp1_input");
                if let Ok(temp_str) = std::fs::read_to_string(&temp_input) {
                    if let Ok(temp_millidegrees) = temp_str.trim().parse::<i32>() {
                        temperature = Some(temp_millidegrees as f32 / 1000.0);
                        break;
                    }
                }
            }
        }

        storage_devices.push(StorageDevice {
            device: format!("/dev/{}", dev_name),
            model,
            size_gb,
            temperature,
        });
    }

    Ok(storage_devices)
}
