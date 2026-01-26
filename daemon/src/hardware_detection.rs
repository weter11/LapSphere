use anyhow::{anyhow, Result};
use std::fs;
use std::path::Path;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
use once_cell::sync::Lazy;
use crate::tuxedo_io::{TuxedoIo, HardwareInterface};
use systemstat::{System, Platform};
// use tuxedo_io::TuxedoIo;
use lapsphere_common::types::*;

// Thread-safe storage for previous CPU stats
static PREVIOUS_CPU_STATS: Mutex<Option<HashMap<u32, CpuStats>>> = Mutex::new(None);
static PREVIOUS_NET_STATS: Lazy<Mutex<HashMap<String, NetStats>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static PREVIOUS_STORAGE_STATS: Lazy<Mutex<HashMap<String, StorageStats>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

static NVIDIA_NAMES_CACHE: Lazy<Mutex<Vec<String>>> = Lazy::new(|| Mutex::new(Vec::new()));

const BITS_PER_BYTE: f64 = 8.0;
const BITS_PER_MEGABIT: f64 = 1_000_000.0;

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

#[derive(Debug, Clone)]
struct NetStats {
    rx_bytes: u64,
    tx_bytes: u64,
    timestamp: Instant,
}

#[derive(Debug, Clone)]
struct StorageStats {
    read_ios: u64,
    read_sectors: u64,
    write_ios: u64,
    write_sectors: u64,
    timestamp: Instant,
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

fn get_cpu_name() -> String {
    if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
        for line in cpuinfo.lines() {
            if line.starts_with("model name") {
                if let Some(name) = line.split(':').nth(1) {
                    return name.trim().to_string();
                }
            }
        }
    }
    "Unknown CPU".to_string()
}

fn get_cpu_topology() -> (u32, u32) {
    let mut logical = 0;
    let mut physical = 0;

    let output = std::process::Command::new("lscpu").output();
    if let Ok(out) = output {
        let s = String::from_utf8_lossy(&out.stdout);
        let mut _threads_per_core = 1;
        let mut cores_per_socket = 0;
        let mut sockets = 0;

        for line in s.lines() {
            let line = line.trim();
            if line.starts_with("CPU(s):") {
                logical = line.split(':').nth(1).unwrap_or("").trim().parse().unwrap_or(0);
            } else if line.starts_with("Thread(s) per core:") {
                _threads_per_core = line.split(':').nth(1).unwrap_or("").trim().parse().unwrap_or(1);
            } else if line.starts_with("Core(s) per socket:") {
                cores_per_socket = line.split(':').nth(1).unwrap_or("").trim().parse().unwrap_or(0);
            } else if line.starts_with("Socket(s):") {
                sockets = line.split(':').nth(1).unwrap_or("").trim().parse().unwrap_or(1);
            }
        }

        physical = cores_per_socket * sockets;
    }

    // Fallback if lscpu fails or gives incomplete info
    if logical == 0 {
        logical = fs::read_to_string("/proc/cpuinfo")
            .map(|s| s.lines().filter(|l| l.starts_with("processor")).count() as u32)
            .unwrap_or(1);
    }
    if physical == 0 {
        physical = logical;
    }

    (physical, logical)
}

fn read_cpu_frequencies(logical_cores: u32) -> Vec<u64> {
    let mut freqs = Vec::with_capacity(logical_cores as usize);
    
    // Try sysfs first as it is core-specific and efficient
    for i in 0..logical_cores {
        let mut found = false;
        for filename in &["scaling_cur_freq", "cpuinfo_cur_freq"] {
            let path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/{}", i, filename);
            if let Ok(s) = fs::read_to_string(&path) {
                if let Ok(freq) = s.trim().parse::<u64>() {
                    freqs.push(freq);
                    found = true;
                    break;
                }
            }
        }
        if found { continue; }
        freqs.push(0); // Placeholder
    }

    // If any are still 0, try parsing /proc/cpuinfo once
    if freqs.iter().any(|&f| f == 0) {
        if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
            let mut core_idx = 0;
            for line in cpuinfo.lines() {
                if line.starts_with("cpu MHz") {
                    if let Some(mhz_str) = line.split(':').nth(1) {
                        if let Ok(mhz) = mhz_str.trim().parse::<f64>() {
                            if core_idx < freqs.len() && freqs[core_idx] == 0 {
                                freqs[core_idx] = (mhz * 1000.0) as u64;
                            }
                            core_idx += 1;
                        }
                    }
                }
            }
        }
    }

    // Fill remaining with a default value
    for f in freqs.iter_mut() {
        if *f == 0 { *f = 2000000; }
    }
    
    freqs
}

fn calculate_median(values: &[u64]) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len() / 2]
}

fn get_core_temp(cpu: u32) -> f32 {
    if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
        for entry in entries.flatten() {
            let name_path = entry.path().join("name");
            if let Ok(name) = fs::read_to_string(&name_path) {
                let name = name.trim();
                if name == "k10temp" {
                    return get_package_temp().unwrap_or(0.0);
                } else if name == "coretemp" {
                    let temp_path = entry.path().join(format!("temp{}_input", cpu + 2));
                    if let Ok(temp_str) = fs::read_to_string(&temp_path) {
                        if let Ok(temp) = temp_str.trim().parse::<f32>() {
                            return temp / 1000.0;
                        }
                    }
                }
            }
        }
    }
    0.0
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
        has_intel_pstate: Path::new("/sys/devices/system/cpu/intel_pstate/status").exists(),
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

fn read_intel_pstate_status() -> Result<String> {
    let path = "/sys/devices/system/cpu/intel_pstate/status";
    fs::read_to_string(path)
        .map(|s| s.trim().to_string())
        .map_err(|e| anyhow!("Failed to read Intel pstate status: {}", e))
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
    
    let io = TuxedoIo::new()?;
    let profiles = get_tdp_profiles()?;
    if profiles.is_empty() {
        return Err(anyhow!("No TDP profiles available"));
    }

    if io.get_interface() == HardwareInterface::Uniwill {
        if let Ok(profile_id) = io.get_uw_performance_profile() {
            // profile_id: 1=powersave, 2=enthusiast, 3=overboost
            let idx = (profile_id.saturating_sub(1)) as usize;
            if idx < profiles.len() {
                return Ok(profiles[idx].clone());
            }
        }
    }
    
    // Fallback to the first profile
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


pub fn get_cpu_info() -> Result<CpuInfo> {
    let name = get_cpu_name();
    let (physical_cores, logical_cores) = get_cpu_topology();
    
    let loads = calculate_cpu_load().unwrap_or_default();
    
    let frequencies = read_cpu_frequencies(logical_cores);
    let mut cores = Vec::new();
    
    for i in 0..logical_cores {
        let freq = frequencies[i as usize];
        cores.push(CoreInfo {
            id: i,
            frequency: freq,
            load: loads.get(&i).copied().unwrap_or(0.0),
            temperature: get_core_temp(i),
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

    let intel_pstate_status = if capabilities.has_intel_pstate {
        read_intel_pstate_status().ok()
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

    let mut tdp0 = None;
    let mut tdp1 = None;
    let mut tdp2 = None;
    let mut tdp0_range = None;
    let mut tdp1_range = None;
    let mut tdp2_range = None;

    if let Ok(io) = TuxedoIo::new() {
        if io.get_interface() == HardwareInterface::Uniwill {
            tdp0 = io.get_tdp(0).ok();
            tdp1 = io.get_tdp(1).ok();
            tdp2 = io.get_tdp(2).ok();

            if let (Ok(min), Ok(max)) = (io.get_tdp_min(0), io.get_tdp_max(0)) {
                tdp0_range = Some((min, max));
            }
            if let (Ok(min), Ok(max)) = (io.get_tdp_min(1), io.get_tdp_max(1)) {
                tdp1_range = Some((min, max));
            }
            if let (Ok(min), Ok(max)) = (io.get_tdp_min(2), io.get_tdp_max(2)) {
                tdp2_range = Some((min, max));
            }
        }
    }

    Ok(CpuInfo {
        name,
        median_frequency,
        median_load,
        package_temp,
        package_power,
        cores,
        physical_cores,
        logical_cores,
        governor,
        available_governors,
        boost_enabled,
        smt_enabled,
        scaling_driver,
        amd_pstate_status,
        intel_pstate_status,
        min_freq,
        max_freq,
        hw_min_freq,
        hw_max_freq,
        all_power_sources,
        power_source,
        energy_performance_preference,
        available_epp_options,
        tdp0,
        tdp1,
        tdp2,
        tdp0_range,
        tdp1_range,
        tdp2_range,
        capabilities,
        scheduler,
        available_schedulers,
    })
}

fn get_memory_type_and_freq() -> (Option<String>, Option<u64>) {
    let dmidecode_path = find_binary("dmidecode").unwrap_or_else(|| "dmidecode".to_string());
    let output = std::process::Command::new(dmidecode_path)
        .args(["-t", "memory"])
        .output();

    let mut mem_type = None;
    let mut mem_speed = None;

    if let Ok(out) = output {
        let s = String::from_utf8_lossy(&out.stdout);
        for line in s.lines() {
            let line = line.trim();
            if line.contains("Type: DDR") || line.contains("Type: LPDDR") {
                mem_type = Some(line.split(':').nth(1).unwrap_or("").trim().to_string());
            }
            if (line.starts_with("Speed:")
                || line.starts_with("Configured Memory Speed:")
                || line.starts_with("Configured Clock Speed:"))
                && mem_speed.is_none()
            {
                let speed_str = line.split(':').nth(1).unwrap_or("").trim();
                if !speed_str.to_lowercase().contains("unknown") && !speed_str.is_empty() {
                    // Expecting something like "3200 MT/s" or "3200 MHz"
                    if let Some(speed_val) = speed_str.split_whitespace().next() {
                        if let Ok(val) = speed_val.parse::<u64>() {
                            mem_speed = Some(val);
                        }
                    }
                }
            }
        }
    }
    (mem_type, mem_speed)
}

pub fn get_memory_info() -> Result<MemoryInfo> {
    let sys = System::new();
    let (total_gib, free_gib, available_gib, used_gib, used_percent) = match sys.memory() {
        Ok(mem) => {
            let total = mem.total.as_u64() as f64 / (1024.0 * 1024.0 * 1024.0);
            let free = mem.free.as_u64() as f64 / (1024.0 * 1024.0 * 1024.0);
            let available = mem.platform_memory.meminfo.get("MemAvailable")
                .map_or(free, |v| v.as_u64() as f64 / (1024.0 * 1024.0 * 1024.0));
            let used = total - available;
            let percent = if total > 0.0 { (used / total * 100.0) as f32 } else { 0.0 };
            (total, free, available, used, percent)
        }
        Err(_) => (0.0, 0.0, 0.0, 0.0, 0.0),
    };

    let (memory_type, memory_frequency) = get_memory_type_and_freq();

    Ok(MemoryInfo {
        total_gib,
        used_gib,
        free_gib,
        available_gib,
        used_percent,
        memory_type,
        memory_frequency,
    })
}

fn get_tuxedo_kernel_modules() -> String {
    if let Ok(modules) = fs::read_to_string("/proc/modules") {
        let module_names: Vec<String> = modules
            .lines()
            .filter(|line| line.contains("tuxedo"))
            .map(|line| line.split_whitespace().next().unwrap_or("").to_string())
            .collect();
        if module_names.is_empty() {
            "Not available".to_string()
        } else {
            module_names.join(", ")
        }
    } else {
        "Not available".to_string()
    }
}

pub fn get_keyboard_capabilities() -> KeyboardCapabilities {
    let mut capabilities = KeyboardCapabilities {
        keyboard_type: KeyboardType::None,
        supports_brightness: false,
        supports_color: false,
        supports_effects: false,
        num_zones: 0,
    };

    // Check for tuxedo_keyboard sysfs
    let base_path = "/sys/devices/platform/tuxedo_keyboard/leds";
    if Path::new(base_path).exists() {
        // Check for multiple zones
        let mut zones = 0;
        if Path::new(&format!("{}/left:kbd_backlight", base_path)).exists()
            || Path::new(&format!("{}/rgb:kbd_backlight", base_path)).exists()
        {
            zones = 1;
            capabilities.keyboard_type = KeyboardType::SingleZoneRGB;

            if Path::new(&format!("{}/center:kbd_backlight", base_path)).exists()
                && Path::new(&format!("{}/right:kbd_backlight", base_path)).exists()
            {
                zones = 3;
                capabilities.keyboard_type = KeyboardType::ThreeZoneRGB;
            }
        }

        if zones > 0 {
            capabilities.num_zones = zones;
            capabilities.supports_brightness = true;
            capabilities.supports_color = true;

            // Effects support - usually if there is a 'mode' file
            let test_path = if zones == 3 {
                format!("{}/left:kbd_backlight/mode", base_path)
            } else {
                format!("{}/rgb:kbd_backlight/mode", base_path)
            };
            capabilities.supports_effects = Path::new(&test_path).exists();

            return capabilities;
        }
    }

    // Fallback: check /sys/class/leds/
    let class_leds = "/sys/class/leds";
    if let Ok(entries) = fs::read_dir(class_leds) {
        let mut kbd_leds = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.contains("kbd_backlight") {
                kbd_leds.push(name);
            }
        }

        if !kbd_leds.is_empty() {
            capabilities.supports_brightness = true;

            let has_rgb = kbd_leds.iter().any(|n| n.contains("rgb:"));
            let has_tuxedo = kbd_leds.iter().any(|n| n.contains("tuxedo:"));

            if has_rgb || has_tuxedo {
                capabilities.supports_color = true;

                // Count zones
                let zones = kbd_leds.len() as u32;
                capabilities.num_zones = zones;

                if zones == 1 {
                    capabilities.keyboard_type = KeyboardType::SingleZoneRGB;
                } else if zones == 3 {
                    capabilities.keyboard_type = KeyboardType::ThreeZoneRGB;
                } else if zones == 4 {
                    capabilities.keyboard_type = KeyboardType::FourZoneRGB;
                } else {
                    capabilities.keyboard_type = KeyboardType::SingleZoneRGB;
                }

                // Check for effects in the first one
                let first_path = format!("{}/{}/mode", class_leds, kbd_leds[0]);
                capabilities.supports_effects = Path::new(&first_path).exists();
            } else {
                capabilities.keyboard_type = KeyboardType::WhiteOnly;
                capabilities.num_zones = 1;
            }
        }
    }

    // Per-key check - some ITE controllers might not show up as standard LEDs yet
    // or they might have a lot of them. If we see > 10 kbd_backlight leds, it might be per-key.
    if capabilities.num_zones > 10 {
        capabilities.keyboard_type = KeyboardType::PerKeyRGB;
    }

    capabilities
}

pub fn get_system_info() -> Result<SystemInfo> {
    let mut product_name = fs::read_to_string("/sys/class/dmi/id/product_name")
        .unwrap_or_else(|_| "Unknown".to_string())
        .trim()
        .to_string();

    if product_name == "Unknown" || product_name.is_empty() {
        if let Ok(model) = fs::read_to_string("/proc/device-tree/model") {
            product_name = model.trim_matches('\0').trim().to_string();
        }
    }

    let product_sku = fs::read_to_string("/sys/class/dmi/id/product_sku")
        .unwrap_or_else(|_| "Unknown".to_string())
        .trim()
        .to_string();
    
    let manufacturer = fs::read_to_string("/sys/class/dmi/id/sys_vendor")
        .unwrap_or_else(|_| "Unknown".to_string())
        .trim()
        .to_string();

    if manufacturer == "Unknown" {
        // Try to get from os-release or something else?
        // For now just leave it.
    }

    let board_name = fs::read_to_string("/sys/class/dmi/id/board_name")
        .unwrap_or_else(|_| "Unknown".to_string())
        .trim()
        .to_string();
    
    let bios_version = fs::read_to_string("/sys/class/dmi/id/bios_version")
        .unwrap_or_else(|_| "Unknown".to_string())
        .trim()
        .to_string();

    let kernel_modules = get_tuxedo_kernel_modules();
    
    Ok(SystemInfo {
        product_name,
        product_sku,
        manufacturer,
        board_name,
        bios_version,
        kernel_modules,
    })
}

pub fn get_gpu_info() -> Result<Vec<GpuInfo>> {
    let mut gpus = Vec::new();
    
    // First, try to get NVIDIA GPU info via NVML
    if Path::new("/sys/bus/pci/drivers/nvidia").exists() {
        if let Ok(nvidia_gpus) = get_nvidia_gpu_info() {
            for gpu in nvidia_gpus {
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
                load,
                power,
                voltage,
                freq_offset: None,
                drain_offset: None,
                power_offset: None,
                total_offset: None,
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

fn get_nvidia_voltage(index: u32) -> Option<f32> {
    let path_lock = crate::NVIDIA_SMI_LEGACY_PATH.lock().unwrap();
    if let Some(ref path) = *path_lock {
        if Path::new(path).exists() {
            // Try -q -d VOLTAGE format first as requested by user
            if let Ok(output) = std::process::Command::new(path)
                .args(["-i", &index.to_string(), "-q", "-d", "VOLTAGE"])
                .output()
            {
                if output.status.success() {
                    let s = String::from_utf8_lossy(&output.stdout);
                    for line in s.lines() {
                        if line.contains("Graphics") && line.contains(':') {
                            if let Some(val_part) = line.split(':').nth(1) {
                                let parts: Vec<&str> = val_part.trim().split_whitespace().collect();
                                if let Some(num_str) = parts.get(0) {
                                    if let Ok(val) = num_str.parse::<f32>() {
                                        return Some(val / 1000.0);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Fallback to query-gpu
            if let Ok(output) = std::process::Command::new(path)
                .args(["-i", &index.to_string(), "--query-gpu=voltage.graphics", "--format=csv,noheader,nounits"])
                .output()
            {
                if output.status.success() {
                    let s = String::from_utf8_lossy(&output.stdout);
                    return s.trim().parse::<f32>().ok().map(|v| v / 1000.0);
                }
            }
        }
    }
    None
}

fn get_nvidia_gpu_info() -> Result<Vec<GpuInfo>> {
    // 1. Check sysfs for NVIDIA devices and their status to avoid waking up suspended GPUs
    let mut nvidia_pci_ids = Vec::new();
    if let Ok(entries) = fs::read_dir("/sys/bus/pci/drivers/nvidia") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.contains(':') {
                nvidia_pci_ids.push(name);
            }
        }
    }
    nvidia_pci_ids.sort();

    if nvidia_pci_ids.is_empty() {
        return Ok(vec![]);
    }

    let mut all_suspended = true;
    let mut statuses = Vec::new();
    for id in &nvidia_pci_ids {
        let status_path = format!("/sys/bus/pci/drivers/nvidia/{}/power/runtime_status", id);
        let status = fs::read_to_string(status_path).unwrap_or_default().trim().to_lowercase();
        if status != "suspended" {
            all_suspended = false;
        }
        statuses.push(status);
    }

    // If all detected NVIDIA GPUs are suspended, bypass NVML completely to keep them asleep
    if all_suspended {
        let mut gpus = Vec::new();
        let names = NVIDIA_NAMES_CACHE.lock().unwrap();
        for (i, status) in statuses.into_iter().enumerate() {
            let name = names.get(i).cloned().unwrap_or_else(|| "NVIDIA GPU".to_string());
            gpus.push(GpuInfo {
                name,
                gpu_type: GpuType::Discrete,
                status,
                frequency: None,
                memory_frequency: None,
                temperature: None,
                load: None,
                power: None,
                voltage: None,
                freq_offset: None,
                drain_offset: None,
                power_offset: None,
                total_offset: None,
            });
        }
        return Ok(gpus);
    }

    // At least one GPU is active, proceed with NVML
    let nvml = get_nvml()?;
    let mut gpus = Vec::new();

    let device_count = nvml.device_count()?;
    for i in 0..device_count {
        let device = nvml.device_by_index(i)?;

        let name = device.name()?;

        // Update name cache
        {
            let mut cache = NVIDIA_NAMES_CACHE.lock().unwrap();
            if cache.len() <= i as usize {
                cache.push(name.clone());
            } else {
                cache[i as usize] = name.clone();
            }
        }

        let gpu_type = if i == 0 {
            GpuType::Integrated
        } else {
            GpuType::Discrete
        };
        
        // Use pre-read status to avoid waking up the GPU with PCI info requests
        let status_from_sysfs = statuses.get(i as usize).cloned();
        let is_suspended = status_from_sysfs
            .as_deref()
            .map(|s| s.eq_ignore_ascii_case("suspended"))
            .unwrap_or(false);

        // Get performance state for status if not suspended
        let status = if is_suspended {
            "suspended".to_string()
        } else {
            match device.performance_state() {
                Ok(state) => format!("{:?}", state),
                Err(_) => status_from_sysfs.unwrap_or_else(|| "active".to_string()),
            }
        };

        let (frequency, memory_frequency, temperature, load, power) = if is_suspended {
            (None, None, None, None, None)
        } else {
            (
                device.clock_info(nvml_wrapper::enum_wrappers::device::Clock::Graphics)
                    .ok()
                    .map(|c| c as u64),
                device.clock_info(nvml_wrapper::enum_wrappers::device::Clock::Memory)
                    .ok()
                    .map(|c| c as u64),
                device.temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu)
                    .ok()
                    .map(|t| t as f32),
                device.utilization_rates().ok().map(|u| u.gpu as f32),
                device.power_usage().ok().map(|p| p as f32 / 1000.0),
            )
        };

        let voltage = if is_suspended { None } else { get_nvidia_voltage(i) };

        let mut gpu_info = GpuInfo {
            name: name.clone(),
            gpu_type,
            status,
            frequency,
            memory_frequency,
            temperature,
            load,
            power,
            voltage,
            freq_offset: None,
            drain_offset: None,
            power_offset: None,
            total_offset: None,
        };

        // Fill in offsets if they exist in global state (assuming first NVIDIA GPU for now)
        if name.to_lowercase().contains("nvidia") {
            let stats_lock = crate::CURRENT_GPU_OVERCLOCK_STATS.lock().unwrap();
            if let Some(ref stats) = *stats_lock {
                gpu_info.freq_offset = Some(stats.freq_offset);
                gpu_info.drain_offset = Some(stats.drain_offset);
                gpu_info.power_offset = Some(stats.power_offset);
                gpu_info.total_offset = Some(stats.total_offset);
            }
        }

        gpus.push(gpu_info);
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
    if let Ok(_load_str) = fs::read_to_string(format!("{}/gt_RP0_freq_mhz", device_path)) {
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
fn find_binary(cmd: &str) -> Option<String> {
    let paths = ["/usr/bin", "/usr/sbin", "/usr/local/bin", "/usr/local/sbin", "/sbin", "/bin"];
    for path in paths {
        let full_path = format!("{}/{}", path, cmd);
        if Path::new(&full_path).exists() {
            return Some(full_path);
        }
    }
    // Try default path (hope it is in PATH)
    Some(cmd.to_string())
}

fn get_pci_info(interface: &str) -> (Option<String>, Option<String>) {
    let device_path = format!("/sys/class/net/{}/device", interface);
    if let Ok(link) = fs::read_link(&device_path) {
        if let Some(pci_addr) = link.file_name().and_then(|n| n.to_str()) {
            let cmd = find_binary("lspci").unwrap_or_else(|| "lspci".to_string());
            if let Ok(output) = std::process::Command::new(cmd)
                .args(["-s", pci_addr, "-k"])
                .output()
            {
                if output.status.success() {
                    let info = String::from_utf8_lossy(&output.stdout);
                    let mut controller = None;
                    let mut subsystem = None;
                    for line in info.lines() {
                        let trimmed = line.trim();
                        if line.contains("Network controller:") {
                            controller = line.split("Network controller:").nth(1).map(|s| s.trim().to_string());
                        } else if trimmed.starts_with("Subsystem:") {
                            subsystem = trimmed.split("Subsystem:").nth(1).map(|s| s.trim().to_string());
                        }
                    }
                    return (controller, subsystem);
                }
            }
        }
    }
    (None, None)
}

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
        // Check /wireless (old) or /phy80211 (new)
        let wireless_path = format!("/sys/class/net/{}/wireless", interface);
        let phy_path = format!("/sys/class/net/{}/phy80211", interface);
        if !Path::new(&wireless_path).exists() && !Path::new(&phy_path).exists() {
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
        
        let (driver_version, firmware_version) = read_wifi_driver_info(&interface);
        let temperature = read_wifi_temperature(&interface);
        
        // Get PCI info (Network controller and Subsystem)
        let (network_controller, subsystem) = get_pci_info(&interface);
        
        // Get all details from iw
        let (ssid, channel, channel_width, tx_bitrate, rx_bitrate, iw_rx_bytes, iw_tx_bytes, iw_signal) = get_wifi_details(&interface);

        // Signal level priority: iw > /proc/net/wireless
        let signal_level = iw_signal.or_else(|| read_wifi_signal(&interface));

        // RX/TX bytes priority: iw > /sys/class/net
        let (final_tx_bytes, final_rx_bytes) = match (iw_tx_bytes, iw_rx_bytes) {
            (Some(tx), Some(rx)) => (tx, rx),
            _ => read_wifi_bytes(&interface).unwrap_or((0, 0)),
        };

        // Calculate actual throughput
        let (tx_rate, rx_rate) = read_wifi_rates(&interface, final_tx_bytes, final_rx_bytes);
        
        log::info!("WiFi {} details: SSID={:?}, Signal={:?}, Channel={:?}, Rates={:?}/{:?}",
                   interface, ssid, signal_level, channel, tx_rate, rx_rate);

        wifi_devices.push(WiFiInfo {
            interface,
            driver,
            driver_version,
            firmware_version,
            temperature,
            signal_level,
            channel,
            channel_width,
            tx_rate,
            rx_rate,
            ssid,
            tx_bitrate,
            rx_bitrate,
            rx_bytes: Some(final_rx_bytes),
            tx_bytes: Some(final_tx_bytes),
            network_controller,
            subsystem,
        });
    }
    
    if wifi_devices.is_empty() {
        return Err(anyhow!("No WiFi devices found"));
    }
    
    Ok(wifi_devices)
}

fn get_wifi_details(interface: &str) -> (
    Option<String>,
    Option<u32>,
    Option<u32>,
    Option<f64>,
    Option<f64>,
    Option<u64>,
    Option<u64>,
    Option<i32>
) {
    let mut ssid = None;
    let mut channel = None;
    let mut width = None;
    let mut tx_bitrate = None;
    let mut rx_bitrate = None;
    let mut rx_bytes = None;
    let mut tx_bytes = None;
    let mut signal = None;

    // Try to get connection info from iw dev <interface> link
    let iw_cmd = find_binary("iw").unwrap_or_else(|| "iw".to_string());
    if let Ok(output) = std::process::Command::new(&iw_cmd)
        .args(["dev", interface, "link"])
        .output()
    {
        if output.status.success() {
            let info = String::from_utf8_lossy(&output.stdout);
            log::debug!("iw link output for {}: {}", interface, info);
            
            for line in info.lines() {
                let trimmed = line.trim();
                let lower = trimmed.to_lowercase();
                
                if let Some(pos) = lower.find("ssid:") {
                    ssid = normalize_ssid(trimmed[pos + 5..].trim());
                } else if lower.contains("rx bitrate:") {
                    rx_bitrate = parse_wifi_rate(trimmed);
                } else if lower.contains("tx bitrate:") {
                    tx_bitrate = parse_wifi_rate(trimmed);
                } else if let Some(pos) = lower.find("rx:") {
                    let part = trimmed[pos + 3..].trim();
                    rx_bytes = part.split_whitespace().next().and_then(|s| s.parse().ok());
                } else if let Some(pos) = lower.find("tx:") {
                    let part = trimmed[pos + 3..].trim();
                    tx_bytes = part.split_whitespace().next().and_then(|s| s.parse().ok());
                } else if let Some(pos) = lower.find("signal:") {
                    let part = trimmed[pos + 7..].trim();
                    signal = part.split_whitespace().next().and_then(|s| s.parse().ok());
                }
            }
        }
    }

    // Get channel and width from iw dev <interface> info
    if let Ok(output) = std::process::Command::new(&iw_cmd)
        .args(["dev", interface, "info"])
        .output()
    {
        if output.status.success() {
            let info = String::from_utf8_lossy(&output.stdout);
            log::debug!("iw info output for {}: {}", interface, info);
            
            for line in info.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("channel") {
                    let parts: Vec<&str> = trimmed.split_whitespace().collect();
                    
                    // Parse channel number
                    if let Some(ch_str) = parts.get(1) {
                        if let Ok(ch) = ch_str.parse::<u32>() {
                            channel = Some(ch);
                        }
                    }
                    
                    // Parse channel width
                    if let Some(pos) = trimmed.find("width:") {
                        if let Some(width_str) = trimmed[pos + 6..].split_whitespace().next() {
                            if let Ok(w) = width_str.parse::<u32>() {
                                width = Some(w);
                            }
                        }
                    }
                }
            }
        }
    }

    // Fallback: try iwgetid for SSID if not found
    if ssid.is_none() {
        if let Some(cmd) = find_binary("iwgetid") {
            if let Ok(output) = std::process::Command::new(cmd)
                .arg("-r")
                .arg(interface)
                .output()
            {
                if output.status.success() {
                    let value = String::from_utf8_lossy(&output.stdout);
                    ssid = normalize_ssid(&value);
                }
            }
        }
    }

    // Last resort: try iwconfig for SSID
    if ssid.is_none() {
        if let Some(cmd) = find_binary("iwconfig") {
            if let Ok(output) = std::process::Command::new(cmd)
                .arg(interface)
                .output()
            {
                if output.status.success() {
                    let info = String::from_utf8_lossy(&output.stdout);
                    for line in info.lines() {
                        if let Some(pos) = line.find("ESSID:") {
                            let value = line[pos + 6..].trim().trim_matches('"');
                            ssid = normalize_ssid(value);
                            break;
                        }
                    }
                }
            }
        }
    }

    (ssid, channel, width, tx_bitrate, rx_bitrate, rx_bytes, tx_bytes, signal)
}

fn read_wifi_temperature(interface: &str) -> Option<f32> {
    // Try device-specific hwmon first
    let temp_path = format!("/sys/class/net/{}/device/hwmon", interface);
    if let Ok(hwmon_entries) = fs::read_dir(&temp_path) {
        for hwmon_entry in hwmon_entries.flatten() {
            let temp_input_path = hwmon_entry.path().join("temp1_input");
            if let Ok(temp_str) = fs::read_to_string(&temp_input_path) {
                if let Ok(temp_millidegrees) = temp_str.trim().parse::<i32>() {
                    return Some(temp_millidegrees as f32 / 1000.0);
                }
            }
        }
    }

    // Fallback: search all hwmons for one associated with this device
    let device_path = format!("/sys/class/net/{}/device", interface);
    if let Ok(net_dev_path) = fs::canonicalize(&device_path) {
        if let Ok(hwmon_dir) = fs::read_dir("/sys/class/hwmon") {
            for entry in hwmon_dir.flatten() {
                if let Ok(hwmon_dev_path) = fs::canonicalize(entry.path().join("device")) {
                    if hwmon_dev_path == net_dev_path {
                        if let Ok(temp_str) = fs::read_to_string(entry.path().join("temp1_input")) {
                            if let Ok(temp_millidegrees) = temp_str.trim().parse::<i32>() {
                                return Some(temp_millidegrees as f32 / 1000.0);
                            }
                        }
                    }
                }
            }
        }
    }

    // Fallback 2: thermal zones
    if let Ok(thermal_dir) = fs::read_dir("/sys/class/thermal") {
        for entry in thermal_dir.flatten() {
            if let Ok(type_str) = fs::read_to_string(entry.path().join("type")) {
                let type_lower = type_str.to_lowercase();
                if type_lower.contains("wifi") || type_lower.contains("iwlwifi") {
                    if let Ok(temp_str) = fs::read_to_string(entry.path().join("temp")) {
                        if let Ok(temp_millidegrees) = temp_str.trim().parse::<i32>() {
                            return Some(temp_millidegrees as f32 / 1000.0);
                        }
                    }
                }
            }
        }
    }

    None
}

fn read_wifi_signal(interface: &str) -> Option<i32> {
    if let Ok(wireless) = fs::read_to_string("/proc/net/wireless") {
        for line in wireless.lines().skip(2) {
            let trimmed = line.trim();
            if trimmed.starts_with(interface) {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 4 {
                    if let Ok(signal) = parts[3].trim_end_matches('.').parse::<i32>() {
                        return Some(signal);
                    }
                }
            }
        }
    }
    None
}

fn read_wifi_driver_info(interface: &str) -> (Option<String>, Option<String>) {
    let ethtool_cmd = find_binary("ethtool").unwrap_or_else(|| "ethtool".to_string());
    if let Ok(output) = std::process::Command::new(ethtool_cmd)
        .args(["-i", interface])
        .output()
    {
        if output.status.success() {
            let info = String::from_utf8_lossy(&output.stdout);
            let mut driver_version = None;
            let mut firmware_version = None;

            for line in info.lines() {
                let trimmed = line.trim();
                let parts: Vec<&str> = trimmed.splitn(2, ':').collect();
                if parts.len() == 2 {
                    let key = parts[0].trim().to_lowercase();
                    let value = parts[1].trim();
                    if key == "version" {
                        driver_version = normalize_ssid(value);
                    } else if key == "firmware-version" {
                        firmware_version = normalize_ssid(value);
                    }
                }
            }

            return (driver_version, firmware_version);
        }
    }

    (None, None)
}

fn normalize_ssid(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches('"');
    if trimmed.is_empty() || trimmed == "off/any" {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn parse_wifi_rate(line: &str) -> Option<f64> {
    // Make sure there is a space after bitrate:
    let sanitized = line.replace("bitrate:", "bitrate: ");
    let parts: Vec<&str> = sanitized.split_whitespace().collect();
    let rate_index = parts.iter().position(|part| *part == "bitrate:");
    let value = rate_index
        .and_then(|idx| parts.get(idx + 1))
        .and_then(|value| value.parse::<f64>().ok())?;
    let unit = rate_index
        .and_then(|idx| parts.get(idx + 2))
        .copied()
        .unwrap_or("MBit/s")
        .trim_end_matches(',');

    Some(match unit {
        "Gbit/s" | "Gbit/sec" | "GBit/s" => value * 1000.0,
        "Kbit/s" | "Kbit/sec" | "KBit/s" => value / 1000.0,
        _ => value,
    })
}

fn read_wifi_bytes(interface: &str) -> Option<(u64, u64)> {
    let tx_bytes_path = format!("/sys/class/net/{}/statistics/tx_bytes", interface);
    let rx_bytes_path = format!("/sys/class/net/{}/statistics/rx_bytes", interface);
    let tx_bytes = fs::read_to_string(tx_bytes_path).ok()?.trim().parse().ok()?;
    let rx_bytes = fs::read_to_string(rx_bytes_path).ok()?.trim().parse().ok()?;
    Some((tx_bytes, rx_bytes))
}

fn read_wifi_rates(interface: &str, tx_bytes: u64, rx_bytes: u64) -> (Option<f64>, Option<f64>) {
    let now = Instant::now();
    let mut stats = PREVIOUS_NET_STATS.lock().unwrap();
    let rates = if let Some(prev) = stats.get(interface) {
        let elapsed = now.duration_since(prev.timestamp).as_secs_f64();
        if elapsed > 0.0 {
            let tx_rate = (tx_bytes.saturating_sub(prev.tx_bytes) as f64 * BITS_PER_BYTE)
                / elapsed
                / BITS_PER_MEGABIT;
            let rx_rate = (rx_bytes.saturating_sub(prev.rx_bytes) as f64 * BITS_PER_BYTE)
                / elapsed
                / BITS_PER_MEGABIT;
            (Some(tx_rate), Some(rx_rate))
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    stats.insert(
        interface.to_string(),
        NetStats {
            rx_bytes,
            tx_bytes,
            timestamp: now,
        },
    );

    rates
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

fn find_storage_temperature(block_device_path: &Path) -> Option<f32> {
    // Strategy 1: Check device/hwmon
    if let Ok(hwmon_entries) = std::fs::read_dir(block_device_path.join("device/hwmon")) {
        for hwmon_entry in hwmon_entries.flatten() {
            if let Some(temp) = read_hwmon_storage_temp(&hwmon_entry.path()) {
                return Some(temp);
            }
        }
    }

    // Strategy 2: Check device/device/hwmon (common for NVMe)
    if let Ok(hwmon_entries) = std::fs::read_dir(block_device_path.join("device/device/hwmon")) {
        for hwmon_entry in hwmon_entries.flatten() {
            if let Some(temp) = read_hwmon_storage_temp(&hwmon_entry.path()) {
                return Some(temp);
            }
        }
    }

    // Strategy 3: Global search for hwmon associated with this device
    if let Ok(device_link) = fs::canonicalize(block_device_path.join("device")) {
        if let Ok(hwmon_dir) = fs::read_dir("/sys/class/hwmon") {
            for entry in hwmon_dir.flatten() {
                if let Ok(hwmon_device_link) = fs::canonicalize(entry.path().join("device")) {
                    // Check if hwmon device is same as or parent of block device
                    if device_link.starts_with(&hwmon_device_link) || hwmon_device_link.starts_with(&device_link) {
                        if let Some(temp) = read_hwmon_storage_temp(&entry.path()) {
                            return Some(temp);
                        }
                    }
                }
            }
        }
    }

    None
}

fn read_hwmon_storage_temp(path: &Path) -> Option<f32> {
    // Try temp1_input, then temp2_input (sometimes composite)
    for i in 1..=3 {
        let temp_path = path.join(format!("temp{}_input", i));
        if let Ok(temp_str) = fs::read_to_string(&temp_path) {
            if let Ok(temp_millidegrees) = temp_str.trim().parse::<i32>() {
                return Some(temp_millidegrees as f32 / 1000.0);
            }
        }
    }
    None
}

fn read_sector_size(path: &Path) -> u64 {
    fs::read_to_string(path.join("queue/hw_sector_size"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(512)
}

fn read_storage_stats(path: &Path) -> Option<(u64, u64, u64, u64)> {
    let stats = fs::read_to_string(path.join("stat")).ok()?;
    let parts: Vec<&str> = stats.split_whitespace().collect();
    if parts.len() < 7 {
        return None;
    }
    let read_ios = parts.first()?.parse::<u64>().ok()?;
    let read_sectors = parts.get(2)?.parse::<u64>().ok()?;
    let write_ios = parts.get(4)?.parse::<u64>().ok()?;
    let write_sectors = parts.get(6)?.parse::<u64>().ok()?;
    Some((read_ios, read_sectors, write_ios, write_sectors))
}

fn calculate_storage_rates(
    device: &str,
    read_ios: u64,
    read_sectors: u64,
    write_ios: u64,
    write_sectors: u64,
    sector_size: u64,
) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
    let now = Instant::now();
    let mut stats = PREVIOUS_STORAGE_STATS.lock().unwrap();
    let rates = if let Some(prev) = stats.get(device) {
        let elapsed = now.duration_since(prev.timestamp).as_secs_f64();
        if elapsed > 0.0 {
            let read_bytes = read_sectors.saturating_sub(prev.read_sectors) as f64 * sector_size as f64;
            let write_bytes = write_sectors.saturating_sub(prev.write_sectors) as f64 * sector_size as f64;
            let read_speed = read_bytes / elapsed / 1_000_000.0;
            let write_speed = write_bytes / elapsed / 1_000_000.0;
            let read_iops = read_ios.saturating_sub(prev.read_ios) as f64 / elapsed;
            let write_iops = write_ios.saturating_sub(prev.write_ios) as f64 / elapsed;
            (Some(read_speed), Some(write_speed), Some(read_iops), Some(write_iops))
        } else {
            (None, None, None, None)
        }
    } else {
        (None, None, None, None)
    };

    stats.insert(
        device.to_string(),
        StorageStats {
            read_ios,
            read_sectors,
            write_ios,
            write_sectors,
            timestamp: now,
        },
    );

    rates
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

        let sector_size = read_sector_size(&path);
        let (read_speed, write_speed, read_iops, write_iops) = match read_storage_stats(&path) {
            Some((read_ios, read_sectors, write_ios, write_sectors)) => {
                calculate_storage_rates(&dev_name, read_ios, read_sectors, write_ios, write_sectors, sector_size)
            }
            None => (None, None, None, None),
        };

        // Try to read temperature from hwmon
        let temperature = find_storage_temperature(&path);

        storage_devices.push(StorageDevice {
            device: format!("/dev/{}", dev_name),
            model,
            size_gb,
            temperature,
            read_speed,
            write_speed,
            read_iops,
            write_iops,
        });
    }

    Ok(storage_devices)
}

#[cfg(test)]
mod wifi_tests {
    use super::*;

    #[test]
    fn test_parse_wifi_rate() {
        assert_eq!(parse_wifi_rate("rx bitrate: 1.1 MBit/s"), Some(1.1));
        assert_eq!(parse_wifi_rate("tx bitrate: 2.2 MBit/s"), Some(2.2));
        assert_eq!(parse_wifi_rate("tx bitrate:3.3 MBit/s"), Some(3.3));
        assert_eq!(parse_wifi_rate("rx bitrate: 4.4 GBit/s"), Some(4400.0));
    }

    #[test]
    fn test_normalize_ssid() {
        assert_eq!(normalize_ssid(" \"Generic-SSID\" "), Some("Generic-SSID".to_string()));
        assert_eq!(normalize_ssid(" Random SSID "), Some("Random SSID".to_string()));
        assert_eq!(normalize_ssid(" off/any "), None);
        assert_eq!(normalize_ssid(""), None);
    }

    #[test]
    fn test_ethtool_parsing() {
        let output = "driver: iwlwifi\nversion: 6.8.0-90-generic\nfirmware-version: 77.b405f9d4.0 cc-a0-77.ucode\nbus-info: 0000:02:00.0\nsupports-statistics: yes\nsupports-test: yes\nsupports-eeprom-access: no\nsupports-register-dump: yes\nsupports-priv-flags: no";

        let mut driver_version = None;
        let mut firmware_version = None;

        for line in output.lines() {
            let trimmed = line.trim();
            let parts: Vec<&str> = trimmed.splitn(2, ':').collect();
            if parts.len() == 2 {
                let key = parts[0].trim().to_lowercase();
                let value = parts[1].trim();
                if key == "version" {
                    driver_version = normalize_ssid(value);
                } else if key == "firmware-version" {
                    firmware_version = normalize_ssid(value);
                }
            }
        }

        assert_eq!(driver_version, Some("6.8.0-90-generic".to_string()));
        assert_eq!(firmware_version, Some("77.b405f9d4.0 cc-a0-77.ucode".to_string()));
    }
}
