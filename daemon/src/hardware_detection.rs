use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::Path;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
use std::mem;
use once_cell::sync::Lazy;
use crate::tuxedo_io::{TuxedoIo, HardwareInterface};
use systemstat::{System, Platform};
// use tuxedo_io::TuxedoIo;
use lapsphere_common::types::*;
use nix::ioctl_readwrite;
use std::os::fd::{AsRawFd, RawFd};

// Thread-safe storage for previous CPU stats
static PREVIOUS_CPU_STATS: Mutex<Option<HashMap<u32, CpuStats>>> = Mutex::new(None);
static PREVIOUS_NET_STATS: Lazy<Mutex<HashMap<String, NetStats>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static PREVIOUS_STORAGE_STATS: Lazy<Mutex<HashMap<String, StorageStats>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Last name reported per PCI BDF. Keyed by identity, not by list position: a
/// name is a property of the adapter, and the sysfs-only paths must not borrow
/// another adapter's name when NVML enumeration order changes.
static NVIDIA_NAMES_CACHE: Lazy<Mutex<HashMap<String, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn remember_gpu_name(bdf: &str, name: &str) {
    let mut cache = crate::hardware_control::lock_or_recover(&NVIDIA_NAMES_CACHE, "NVIDIA_NAMES_CACHE");
    cache.insert(bdf.to_lowercase(), name.to_string());
}

fn gpu_name_for_bdf(bdf: &str) -> Option<String> {
    let cache = crate::hardware_control::lock_or_recover(&NVIDIA_NAMES_CACHE, "NVIDIA_NAMES_CACHE");
    cache.get(&bdf.to_lowercase()).cloned()
}

#[derive(Clone)]
struct NvidiaMetadata {
    architecture: Option<String>,
    supported_p_states: Vec<String>,
    power_limit_range: Option<(u32, u32)>,
    supports_gpu_offset: bool,
    supports_mem_offset: bool,
    vram_type: Option<String>,
    vram_vendor: Option<String>,
    vram_bus_width: Option<u32>,
    vram_total: Option<u64>,
    core_clock_range: Option<(u32, u32)>,
    memory_clock_range: Option<(u32, u32)>,
    core_offset_limits: Option<(i32, i32)>,
    memory_offset_limits: Option<(i32, i32)>,
}

static NVIDIA_METADATA_CACHE: Lazy<Mutex<HashMap<u32, NvidiaMetadata>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// ---------------------------------------------------------------------------
// dGPU polling tiers (RTD3-aware)
//
// Any NVML call on an RTD3-capable dGPU resets the kernel's
// autosuspend_delay_ms (20 s) timer, even at 0% utilization. Polling NVML at
// 1 Hz therefore pins an active-but-idle GPU awake forever — that is why the
// old idle-metrics cache existed. The cache is gone: NO telemetry may be
// served from a stale store, because a cached "OK" reading is indistinguishable
// from a live one and mislabels the GPU. Instead, one poll block decides per
// tick whether invasive queries are allowed at all:
//
//   - suspended            -> sysfs-only payload, ZERO NVML/NVAPI calls (a call
//                             would wake the GPU)
//   - active, P0..P2       -> full live NVML + NVAPI query EVERY tick; a loaded
//                             GPU cannot runtime-suspend, so this is free
//   - active, with work    -> same live treatment as P0..P2 at ANY p-state:
//                             light 3D work sits on P3 and oscillates to P5,
//                             and those ticks must not go blank
//   - active, no work
//     (any p-state)        -> quiet tier: sysfs runtime_status only; every
//                             telemetry field is left blank rather than filled
//                             from a previous sample, so the GPU can complete
//                             its own idle-down (P3 -> P5 -> P8 -> suspended)
//
// The only cached values are the two cheap/static VRAM figures (total and
// available, see gpu_activity::record_memory) plus static device metadata
// (name, VRAM type/vendor/bus, clock ranges, supported p-states). Nothing
// dynamic — status, clocks, load, power, voltage, hotspot, memory temperature —
// is ever read back from a store.
//
// Entering the quiet tier is not a one-way door: a re-probe is allowed once
// QUIET_REPROBE_SECS have passed, so a GPU that ramps from a light P8 back to
// P0 is noticed without a wake. The cadence must stay longer than the
// kernel's autosuspend delay plus the driver's release latency: measured on
// this machine, the dGPU re-suspends 27 s after the last NVML touch, so by the
// time the cadence fires an unused GPU is already suspended — the tier check
// then reads "suspended" and skips the probe instead of waking it.
// ---------------------------------------------------------------------------

/// Minimum quiet-tier dwell before one bounded re-probe is allowed (seconds).
const QUIET_REPROBE_SECS: u64 = 45;

/// GPU utilization that counts as "this adapter is doing work" (percent).
/// Measured on the XMG: an idle desktop reports 0.0, while real 3D work sits
/// well above this (5 % at P0, 26-46 % at P3/P5). The margin keeps a one-off
/// Xorg blit from holding the live tier open.
const GPU_WORK_UTILIZATION_PERCENT: f32 = 1.0;

/// Per-GPU control state. This is NOT a telemetry store: `last_pstate` and
/// `last_runtime_status` decide the tier only and are never published as
/// measurements (the quiet-tier payload carries `performance_state: None`).
#[derive(Clone, Debug, Default)]
struct GpuPollState {
    /// Last runtime-PM word observed from sysfs.
    last_runtime_status: Option<String>,
    /// Last performance state seen by a live NVML pass.
    last_pstate: Option<u8>,
    /// GPU utilization observed by that same pass. P3 is the boundary state:
    /// P3 with work must stay live, P3 without work must be left alone.
    last_load: Option<f32>,
    /// When the last invasive NVML/NVAPI query for this GPU was issued.
    last_probe: Option<Instant>,
}

static GPU_POLL_STATE: Lazy<Mutex<HashMap<String, GpuPollState>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// NVML device index -> PCI BDF, learned from the driver during live passes.
///
/// NVML's index order is an implementation detail of the library, while the BDF
/// is the identity the kernel, the sysfs tree and NVML's own `pci_info()` all
/// agree on. Everything that pairs sysfs state with an NVML device resolves the
/// BDF through this map first, so a change in enumeration order cannot attach
/// one adapter's runtime/power state to another adapter's telemetry.
static NVIDIA_BDF_BY_INDEX: Lazy<Mutex<HashMap<u32, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// One adapter as sysfs sees it: directory name plus runtime-PM word.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SysfsNvidiaDevice {
    /// Directory name under /sys/bus/pci/drivers/nvidia, e.g. "0000:01:00.0".
    bdf: String,
    /// `power/runtime_status`, lower case.
    runtime_status: String,
}

/// One NVML device plus the identity NVML reports for it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct NvmlDeviceSlot {
    index: u32,
    /// `None` when NVML cannot report the PCI identity for this device.
    bdf: Option<String>,
}

/// Canonicalise an NVML PCI identity into the sysfs directory-name form used by
/// /sys/bus/pci/devices and /sys/bus/pci/drivers/nvidia ("0000:01:00.0").
fn canonical_bdf(bus_id: &str, domain: u32) -> Option<String> {
    let (_, rest) = bus_id.split_once(':')?;
    if rest.is_empty() {
        return None;
    }
    Some(format!("{:04x}:{}", domain, rest).to_lowercase())
}

/// Identity NVML reports for one device.
fn nvml_pci_bdf(device: &nvml_wrapper::Device) -> Option<String> {
    let pci = device.pci_info().ok()?;
    canonical_bdf(&pci.bus_id, pci.domain)
}

fn remember_nvml_bdf(index: u32, bdf: &str) {
    let mut map = crate::hardware_control::lock_or_recover(&NVIDIA_BDF_BY_INDEX, "NVIDIA_BDF_BY_INDEX");
    match map.get(&index) {
        Some(known) if known.eq_ignore_ascii_case(bdf) => {}
        Some(known) => {
            log::warn!(target: "hw.detect",
                "NVML index {} now reports BDF {} (was {}) - remapping", index, bdf, known);
            map.insert(index, bdf.to_lowercase());
        }
        None => {
            map.insert(index, bdf.to_lowercase());
        }
    }
}

/// BDF the most recent live pass saw for an NVML index.
fn bdf_for_nvml_index(index: u32) -> Option<String> {
    let map = crate::hardware_control::lock_or_recover(&NVIDIA_BDF_BY_INDEX, "NVIDIA_BDF_BY_INDEX");
    map.get(&index).cloned()
}

/// NVML index for a BDF, when a live pass already learned the association.
fn nvml_index_for_bdf(bdf: &str) -> Option<u32> {
    let map = crate::hardware_control::lock_or_recover(&NVIDIA_BDF_BY_INDEX, "NVIDIA_BDF_BY_INDEX");
    map.iter()
        .find(|(_, known)| known.eq_ignore_ascii_case(bdf))
        .map(|(index, _)| *index)
}

/// Poll-state key for one adapter. The BDF is used whenever it is known, so the
/// state follows the adapter across enumerations; only an adapter NVML cannot
/// identify falls back to an index-scoped key (which cannot collide with a BDF).
fn poll_state_key(bdf: Option<&str>, index: u32) -> String {
    match bdf {
        Some(bdf) if !bdf.is_empty() => bdf.to_lowercase(),
        _ => format!("index:{}", index),
    }
}

/// What a tick is allowed to do for one GPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GpuPollTier {
    /// Issue NVML + NVAPI queries this tick.
    Live,
    /// sysfs reads only; every telemetry field is published blank.
    Quiet,
    /// Runtime-suspended: sysfs reads only, and never a probe (it would wake it).
    Suspended,
}

/// Pure tier decision. `runtime_status` comes from sysfs and costs nothing.
fn gpu_poll_tier(state: Option<&GpuPollState>, runtime_status: &str) -> GpuPollTier {
    if runtime_status.eq_ignore_ascii_case("suspended") {
        return GpuPollTier::Suspended;
    }
    let Some(state) = state else {
        // Never observed: one pass is needed to learn the current p-state.
        return GpuPollTier::Live;
    };
    // A wake after a suspended observation is the cheapest possible evidence
    // that something started using the GPU: probe it while it is awake anyway.
    let woke = state
        .last_runtime_status
        .as_deref()
        .map(|previous| previous.eq_ignore_ascii_case("suspended"))
        .unwrap_or(false);
    if woke {
        return GpuPollTier::Live;
    }
    // P0..P2: the GPU is doing work — live values every tick, no cache.
    if matches!(state.last_pstate, Some(pstate) if pstate <= 2) {
        return GpuPollTier::Live;
    }
    // Anything with work is polled live, whatever its P-state: light 3D work
    // lands on P3 and oscillates to P5 (measured 26-46 % utilization), and the
    // user-facing rule is "real values whenever the GPU is actually working".
    // A pass that could not read the utilization counts as "no work": the
    // re-probe below still refreshes it, and an unmeasured GPU must never be
    // pinned awake.
    if state
        .last_load
        .map(|load| load > GPU_WORK_UTILIZATION_PERCENT)
        .unwrap_or(false)
    {
        return GpuPollTier::Live;
    }
    // Never probed yet: one pass is needed to learn the p-state.
    if state.last_probe.is_none() {
        return GpuPollTier::Live;
    }
    // P3 and deeper — or a pass that could not report a p-state at all — stay
    // out of the way until the bounded re-probe cadence elapses.
    match state.last_probe {
        Some(at) if at.elapsed().as_secs() < QUIET_REPROBE_SECS => GpuPollTier::Quiet,
        _ => GpuPollTier::Live,
    }
}

/// Record a sysfs-only observation (no probe issued, so the cadence clock is
/// deliberately not touched).
fn record_gpu_observation(key: &str, runtime_status: &str) {
    let mut states = crate::hardware_control::lock_or_recover(&GPU_POLL_STATE, "GPU_POLL_STATE");
    let state = states.entry(key.to_string()).or_default();
    state.last_runtime_status = Some(runtime_status.to_string());
}

/// Record a completed invasive pass: its p-state and utilization drive the
/// next tick's tier.
fn record_gpu_probe(key: &str, runtime_status: &str, pstate: Option<u8>, load: Option<f32>) {
    let mut states = crate::hardware_control::lock_or_recover(&GPU_POLL_STATE, "GPU_POLL_STATE");
    let state = states.entry(key.to_string()).or_default();
    state.last_runtime_status = Some(runtime_status.to_string());
    if let Some(pstate) = pstate {
        state.last_pstate = Some(pstate);
    }
    if let Some(load) = load {
        state.last_load = Some(load);
    }
    state.last_probe = Some(Instant::now());
}

/// Was the previous observation of this GPU "suspended"? Used to validate the
/// first power sample after a wake (a successful NVML read right after D3 wake
/// can report stale nonsense, measured at 753 W on a 150 W-max GPU).
fn gpu_woke_from_suspend(key: &str) -> bool {
    let states = crate::hardware_control::lock_or_recover(&GPU_POLL_STATE, "GPU_POLL_STATE");
    states
        .get(key)
        .and_then(|s| s.last_runtime_status.as_deref())
        .map(|s| s.eq_ignore_ascii_case("suspended"))
        .unwrap_or(false)
}

/// Runtime-PM status word for one NVIDIA PCI id (non-invasive sysfs read).
fn read_runtime_status(pci_id: &str) -> String {
    fs::read_to_string(format!(
        "/sys/bus/pci/drivers/nvidia/{}/power/runtime_status",
        pci_id
    ))
    .unwrap_or_default()
    .trim()
    .to_lowercase()
}

/// Every NVIDIA adapter sysfs lists, with its own runtime-PM word.
///
/// Sorted for deterministic output; the order carries no identity meaning — the
/// `bdf` field is the identity, and every lookup below matches on it.
fn sysfs_nvidia_devices() -> Vec<SysfsNvidiaDevice> {
    let mut ids: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir("/sys/bus/pci/drivers/nvidia") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.contains(':') {
                ids.push(name);
            }
        }
    }
    ids.sort();
    ids.into_iter()
        .map(|bdf| SysfsNvidiaDevice {
            runtime_status: read_runtime_status(&bdf),
            bdf,
        })
        .collect()
}

/// Resolve each NVML device's runtime-PM status by ITS OWN PCI identity.
///
/// Association is by BDF and never by list position, so a permuted sysfs
/// enumeration cannot hand one adapter's runtime state to another. A device
/// whose BDF sysfs does not list yields `None` (unknown) instead of borrowing a
/// neighbour's status; the one exception is a single-device system, where the
/// two views cannot describe different adapters — that preserves the
/// pre-existing single-GPU behaviour when NVML reports no BDF at all.
fn runtime_status_by_device(
    devices: &[NvmlDeviceSlot],
    sysfs: &[SysfsNvidiaDevice],
) -> Vec<Option<String>> {
    devices
        .iter()
        .map(|slot| {
            if let Some(bdf) = slot.bdf.as_deref() {
                if let Some(found) = sysfs.iter().find(|dev| dev.bdf.eq_ignore_ascii_case(bdf)) {
                    return Some(found.runtime_status.clone());
                }
                log::debug!(target: "hw.detect",
                    "NVML device {} reports BDF {} which sysfs does not list; runtime state stays unknown",
                    slot.index, bdf);
                return None;
            }
            if devices.len() == 1 && sysfs.len() == 1 {
                return Some(sysfs[0].runtime_status.clone());
            }
            log::debug!(target: "hw.detect",
                "NVML device {} reports no PCI identity and the system has {} adapters; runtime state stays unknown",
                slot.index, sysfs.len());
            None
        })
        .collect()
}

/// Poll-state keys whose current tier allows an invasive query, evaluated
/// WITHOUT touching the GPU. Shared by the metric poll and the GPU-fan poll so
/// both obey the same policy instead of each deciding for itself.
fn live_tier_keys() -> Vec<String> {
    let states = crate::hardware_control::lock_or_recover(&GPU_POLL_STATE, "GPU_POLL_STATE");
    sysfs_nvidia_devices()
        .iter()
        .filter(|dev| {
            gpu_poll_tier(states.get(&poll_state_key(Some(&dev.bdf), 0)), &dev.runtime_status)
                == GpuPollTier::Live
        })
        .map(|dev| poll_state_key(Some(&dev.bdf), 0))
        .collect()
}

static PREVIOUS_RAPL_STATS: Mutex<Option<(f64, Instant)>> = Mutex::new(None);

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
    let mut prev_stats_lock = crate::hardware_control::lock_or_recover(&PREVIOUS_CPU_STATS, "PREVIOUS_CPU_STATS");
    
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
    static CACHED_NAME: Lazy<String> = Lazy::new(|| {
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
    });
    CACHED_NAME.clone()
}


fn get_cpu_topology() -> (u32, u32) {
    static CACHED_TOPOLOGY: Lazy<(u32, u32)> = Lazy::new(|| {
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
    });
    *CACHED_TOPOLOGY
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
                        let now = Instant::now();
                        let mut prev_lock = crate::hardware_control::lock_or_recover(&PREVIOUS_RAPL_STATS, "PREVIOUS_RAPL_STATS");

                        let power = if let Some((prev_energy, prev_time)) = *prev_lock {
                            let elapsed = now.duration_since(prev_time).as_secs_f64();
                            if elapsed > 0.01 { // Only update if enough time has passed
                                let diff = energy - prev_energy;
                                if diff >= 0.0 {
                                    // energy is in microjoules.
                                    // Power (Watts) = Joules / Seconds
                                    (diff / 1_000_000.0 / elapsed) as f32
                                } else {
                                    0.0 // Counter reset?
                                }
                            } else {
                                return Err(anyhow!("RAPL: Too soon for update"));
                            }
                        } else {
                            0.0
                        };

                        *prev_lock = Some((energy, now));
                        return Ok(power);
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

pub fn read_hw_frequency_limits() -> Result<(Option<u64>, Option<u64>)> {
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
        log::debug!(target: "hw.detect", "TDP profiles not available (/dev/tuxedo_io not present)");
        return Ok(vec![]);
    }
    
    match TuxedoIo::shared() {
        Some(io) => {
            match io.get_available_profiles() {
                Ok(profiles) => {
                    static LOGGED_ONCE: Mutex<bool> = Mutex::new(false);
                    let mut logged = crate::hardware_control::lock_or_recover(&LOGGED_ONCE, "LOGGED_ONCE");
                    if !*logged {
                        log::debug!(target: "hw.detect", "Available TDP profiles: {:?}", profiles);
                        *logged = true;
                    }
                    Ok(profiles)
                }
                Err(e) => {
                    log::warn!(target: "hw.detect", "Failed to get TDP profiles: {}", e);
                    Ok(vec![])
                }
            }
        }
        None => {
            log::warn!(target: "hw.detect", "Failed to open /dev/tuxedo_io");
            Ok(vec![])
        }
    }
}

/// Internal helper to get base memory clock ranges across all performance states
fn get_base_memory_clock_ranges(device: &nvml_wrapper::Device) -> Result<(u32, u32)> {
    let mut range = None;
    if let Ok(supported_states) = device.supported_performance_states() {
        let mut m_min = u32::MAX;
        let mut m_max = 0;
        for pstate in supported_states {
            if let Ok((p_min, p_max)) = device.min_max_clock_of_pstate(Clock::Memory, pstate) {
                if p_min < m_min { m_min = p_min; }
                if p_max > m_max { m_max = p_max; }
            }
        }
        if m_min != u32::MAX {
            range = Some((m_min, m_max));
        }
    }

    if range.is_none() {
        if let Ok(clocks) = device.supported_memory_clocks() {
            if let (Some(&c_min), Some(&c_max)) = (clocks.iter().min(), clocks.iter().max()) {
                range = Some((c_min, c_max));
            }
        }
    }

    range.ok_or_else(|| anyhow!("Could not determine memory clock ranges"))
}

pub fn get_current_tdp_profile() -> Result<String> {
    if !TuxedoIo::is_available() {
        return Err(anyhow!("TDP profiles not available"));
    }
    
    let io = TuxedoIo::shared().ok_or_else(|| anyhow!("tuxedo_io not available"))?;
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


pub fn get_all_fan_info() -> Result<Vec<FanInfo>> {
    let mut all_fans = Vec::new();

    // 1. Get system fans (Tuxedo/Uniwill/Clevo)
    if TuxedoIo::is_available() {
        if let Some(io) = TuxedoIo::shared() {
            let fan_settings = crate::hardware_control::lock_or_recover(&crate::FAN_DAEMON_STATE, "FAN_DAEMON_STATE");
            let manual_mode = fan_settings.as_ref().map_or(false, |s| s.control_enabled);

            for fan_id in 0..io.get_fan_count() {
                if let Ok(speed) = io.get_fan_speed(fan_id) {
                    let temperature = io.get_fan_temperature(fan_id).ok().map(|t| t as f32);
                    all_fans.push(FanInfo {
                        id: fan_id,
                        name: format!("System Fan {}", fan_id),
                        rpm_or_percent: speed,
                        temperature,
                        is_rpm: false,
                        mode: Some(if manual_mode { "Manual".to_string() } else { "Auto".to_string() }),
                    });
                }
            }
        }
    }

    // 2. Get NVIDIA GPU fans
    let gpu_settings = crate::hardware_control::lock_or_recover(&crate::GPU_DAEMON_STATE, "GPU_DAEMON_STATE");

    // Check suspension status first to avoid waking up GPU
    let mut nvidia_active = false;
    if let Ok(entries) = fs::read_dir("/sys/bus/pci/drivers/nvidia") {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().contains(':') {
                let status_path = entry.path().join("power/runtime_status");
                if let Ok(status) = fs::read_to_string(status_path) {
                    if status.trim() != "suspended" {
                        nvidia_active = true;
                        break;
                    }
                }
            }
        }
    }

    // Same RTD3 policy as the metric poll: NVML fan reads happen only for a
    // GPU in the live tier (see the tier block at the top of this file). This
    // second, independently scheduled NVML toucher must not reset the
    // autosuspend timer for a GPU that is idling down. Fan speeds are never
    // cached: a GPU that may not be queried simply has no fan rows until it is
    // queried again.
    let live_keys = live_tier_keys();
    if nvidia_active && !live_keys.is_empty() {
        if let Ok(nvml) = get_nvml() {
            if let Ok(device_count) = nvml.device_count() {
                for i in 0..device_count {
                    let Ok(device) = nvml.device_by_index(i) else { continue };

                    // Identify the adapter through the same canonicalisation the
                    // metric poll uses. NVML reports the domain as eight hex
                    // digits ("00000000:01:00.0") while sysfs directories use
                    // four ("0000:01:00.0"), so hand-building the path from
                    // pci_info().bus_id silently missed every file: the status
                    // read failed, the GPU counted as suspended and its fan rows
                    // were never emitted (observed on the XMG: zero NVIDIA fan
                    // entries in the payload).
                    let bdf = nvml_pci_bdf(&device);
                    if let Some(ref bdf) = bdf {
                        remember_nvml_bdf(i, bdf);
                    }
                    let poll_key = poll_state_key(bdf.as_deref(), i);
                    if read_runtime_status(
                        bdf.as_deref()
                            .unwrap_or("__unknown__"),
                    )
                    .eq_ignore_ascii_case("suspended")
                    {
                        continue;
                    }
                    if !live_keys.contains(&poll_key) {
                        continue;
                    }

                    {
                        if let Ok(num_fans) = device.num_fans() {
                            for f in 0..num_fans {
                                let speed = device.fan_speed(f).unwrap_or(0);

                                let mode = if let Some(settings) = &*gpu_settings {
                                    if settings.nvidia_fans.iter().any(|s| s.device_index == i && s.fan_id == f && s.manual) {
                                        "Manual".to_string()
                                    } else {
                                        "Auto".to_string()
                                    }
                                } else {
                                    "Auto".to_string()
                                };

                                all_fans.push(FanInfo {
                                    id: 100 + i * 10 + f, // Unique ID for GPU fans
                                    name: format!("NVIDIA GPU {} Fan {}", i, f),
                                    rpm_or_percent: speed,
                                    temperature: device.temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu).ok().map(|t| t as f32),
                                    is_rpm: false,
                                    mode: Some(mode),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(all_fans)
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
    
    let average_frequency = if !frequencies.is_empty() {
        frequencies.iter().sum::<u64>() / frequencies.len() as u64
    } else {
        0
    };
    
    let loads_vec: Vec<f32> = loads.values().copied().collect();
    let average_load = if !loads_vec.is_empty() {
        loads_vec.iter().sum::<f32>() / loads_vec.len() as f32
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
        average_frequency,
        average_load,
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
    static CACHED_MEM_METADATA: Lazy<(Option<String>, Option<u64>)> = Lazy::new(|| {
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
    });
    CACHED_MEM_METADATA.clone()
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

fn has_ite_keyboard() -> bool {
    if let Ok(entries) = fs::read_dir("/sys/bus/hid/devices") {
        for entry in entries.flatten() {
            if let Ok(uevent) = fs::read_to_string(entry.path().join("uevent")) {
                if uevent.contains("HID_ID=0003:0000048D:") {
                    return true;
                }
            }
        }
    }
    false
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
    if capabilities.num_zones > 10 || (capabilities.keyboard_type == KeyboardType::None && has_ite_keyboard()) {
        capabilities.keyboard_type = KeyboardType::PerKeyRGB;
        // If it's ITE, it usually supports brightness and color even if not yet fully detected via sysfs
        if capabilities.num_zones == 0 {
            capabilities.supports_brightness = true;
            capabilities.supports_color = true;
        }
    }

    capabilities
}

pub fn get_system_info() -> Result<SystemInfo> {
    static CACHED_SYSTEM_INFO: Mutex<Option<SystemInfo>> = Mutex::new(None);

    {
        let cache = crate::hardware_control::lock_or_recover(&CACHED_SYSTEM_INFO, "CACHED_SYSTEM_INFO");
        if let Some(info) = &*cache {
            return Ok(info.clone());
        }
    }

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
    
    let info = SystemInfo {
        product_name,
        product_sku,
        manufacturer,
        board_name,
        bios_version,
        kernel_modules,
    };

    {
        let mut cache = crate::hardware_control::lock_or_recover(&CACHED_SYSTEM_INFO, "CACHED_SYSTEM_INFO");
        *cache = Some(info.clone());
    }

    Ok(info)
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
            
            // No runtime_status for integrated graphics: the adapter never
            // runtime-suspends, so the word would be a constant "active" and
            // carries no information. Its holders list and VRAM figures are
            // suppressed for the same reason (see gpu_activity::attach).
            
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
                pci_bus_id: fs::canonicalize(&device_path).ok().and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned())),
                process_snapshot: GpuProcessSnapshot::default(),
                vram_memory: None,
                name,
                gpu_type,
                runtime_status: None,
                performance_state: None,
                frequency,
                memory_frequency,
                temperature,
                hotspot_temperature: None,  // Not available for AMD GPUs
                memory_temperature: None,    // Not available for AMD GPUs
                load,
                power,
                voltage,
                freq_offset: None,
                drain_offset: None,
                power_offset: None,
                total_offset: None,
                min_core_clock: None,
                max_core_clock: None,
                min_memory_clock: None,
                max_memory_clock: None,
                core_clock_range: None,
                memory_clock_range: None,
                core_offset_limits: None,
                memory_offset_limits: None,
                is_desktop: false,
                architecture: None,
                nvml_index: None,
                driver_version: None,
                supported_p_states: vec![],
                supports_power_limit: false,
                power_limit_range: None,
                supports_gpu_offset: false,
                supports_mem_offset: false,
                fan_speed_range: None,
                vram_type: None,
                vram_vendor: None,
                vram_bus_width: None,
                vram_bandwidth: None,
                vram_total: None,
            });
        }
    }
    
    crate::gpu_activity::attach(&mut gpus);
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



/// Internal helper to get base (un-offset) GPU clock ranges for P-State 0
fn get_base_gpu_clock_ranges(device: &nvml_wrapper::Device) -> Result<(u32, u32)> {
    match device.min_max_clock_of_pstate(Clock::Graphics, PerformanceState::Zero) {
        Ok((min, max)) => Ok((min, max)),
        Err(e) => {
            log::warn!(target: "hw.detect", "Failed to get P0 clock ranges via min_max_clock_of_pstate: {}. Using fallback.", e);
            // Fallback to absolute max supported clocks if P0 ranges fail
            let mut fallback = None;
            if let Ok(mem_clocks) = device.supported_memory_clocks() {
                if let Some(&target_mem_clock) = mem_clocks.iter().max() {
                    if let Ok(clocks) = device.supported_graphics_clocks(target_mem_clock) {
                        if let (Some(&c_min), Some(&c_max)) = (clocks.iter().min(), clocks.iter().max()) {
                            fallback = Some((c_min, c_max));
                        }
                    }
                }
            }
            fallback.ok_or_else(|| anyhow!("Could not determine graphics clock ranges for P-State 0: {}", e))
        }
    }
}

pub fn get_gpu_clock_ranges(device_index: u32) -> Result<(u32, u32)> {
    // Try cache first to avoid waking up GPU
    let cached_range = {
        let cache = crate::hardware_control::lock_or_recover(&NVIDIA_METADATA_CACHE, "NVIDIA_METADATA_CACHE");
        cache.get(&device_index).and_then(|m| m.core_clock_range)
    };

    let (mut min, mut max) = if let Some(range) = cached_range {
        range
    } else {
        // If not in cache, check if GPU is suspended before waking it up
        if is_gpu_suspended_by_index(device_index) {
            return Err(anyhow!("GPU suspended and metadata not cached"));
        }
        let nvml = get_nvml()?;
        let device = nvml.device_by_index(device_index)?;
        get_base_gpu_clock_ranges(&device)?
    };

    // Add current core offset if any to show real-time effective ranges in the UI
    let offset = {
        let map = crate::hardware_control::lock_or_recover(&crate::MANUAL_GPU_OFFSETS, "MANUAL_GPU_OFFSETS");
        map.get(&device_index).map(|(c, _)| *c).unwrap_or(0.0)
    };

    min = (min as f32 + offset).max(0.0) as u32;
    max = (max as f32 + offset).max(0.0) as u32;

    Ok((min, max))
}


pub fn get_gpu_core_offset_limits(device_index: u32) -> Result<(i32, i32)> {
    // Try cache first
    let cached_limits = {
        let cache = crate::hardware_control::lock_or_recover(&NVIDIA_METADATA_CACHE, "NVIDIA_METADATA_CACHE");
        cache.get(&device_index).and_then(|m| m.core_offset_limits)
    };

    if let Some(limits) = cached_limits {
        return Ok(limits);
    }

    if is_gpu_suspended_by_index(device_index) {
        return Err(anyhow!("GPU suspended and metadata not cached"));
    }

    let nvml = get_nvml()?;
    let device = nvml.device_by_index(device_index)?;
    let offset_info = device.clock_offset(Clock::Graphics, PerformanceState::Zero)?;
    Ok((offset_info.min_clock_offset_mhz, offset_info.max_clock_offset_mhz))
}

pub fn get_gpu_memory_offset_limits(device_index: u32) -> Result<(i32, i32)> {
    // Try cache first
    let cached_limits = {
        let cache = crate::hardware_control::lock_or_recover(&NVIDIA_METADATA_CACHE, "NVIDIA_METADATA_CACHE");
        cache.get(&device_index).and_then(|m| m.memory_offset_limits)
    };

    if let Some(limits) = cached_limits {
        return Ok(limits);
    }

    if is_gpu_suspended_by_index(device_index) {
        return Err(anyhow!("GPU suspended and metadata not cached"));
    }

    let nvml = get_nvml()?;
    let device = nvml.device_by_index(device_index)?;
    let offset_info = device.clock_offset(Clock::Memory, PerformanceState::Zero)?;
    Ok((offset_info.min_clock_offset_mhz, offset_info.max_clock_offset_mhz))
}

/// Pure decision behind [`is_gpu_suspended_by_index`], so the identity handling
/// is testable without hardware.
///
/// `Some(status)` means "this index is known to be suspended or not"; `None`
/// means the mapping is not established (no live pass yet) and the index cannot
/// be attributed safely. A single-adapter system has only one possible
/// interpretation, so there the sysfs entry is used directly; with several
/// adapters an unestablished mapping is never guessed from list position.
fn suspended_decision(
    sysfs: &[SysfsNvidiaDevice],
    known_bdf: Option<&str>,
    candidates: usize,
) -> Option<bool> {
    if let Some(bdf) = known_bdf {
        return sysfs
            .iter()
            .find(|dev| dev.bdf.eq_ignore_ascii_case(bdf))
            .map(|dev| dev.runtime_status.eq_ignore_ascii_case("suspended"));
    }
    if candidates == 1 && sysfs.len() == 1 {
        return Some(sysfs[0].runtime_status.eq_ignore_ascii_case("suspended"));
    }
    None
}

/// Is the GPU behind this NVML index runtime-suspended?
///
/// The BDF learned from NVML decides it. When NVML has not reported an identity
/// yet, an index on a multi-adapter system is treated as suspended: refusing to
/// query is the conservative choice for RTD3 (a wrong "awake" answer would wake
/// the adapter), and the caller's error path already says the metadata is not
/// cached.
fn is_gpu_suspended_by_index(index: u32) -> bool {
    let sysfs = sysfs_nvidia_devices();
    let known_bdf = bdf_for_nvml_index(index);
    match suspended_decision(&sysfs, known_bdf.as_deref(), sysfs.len()) {
        Some(suspended) => suspended,
        None => {
            log::debug!(target: "hw.detect",
                "no NVML->BDF mapping for index {} on a {}-adapter system; treating as suspended",
                index, sysfs.len());
            true
        }
    }
}

// NVIDIA Direct Driver Constants and Structs
//
// Escape numbers and the registration direction are verified against the
// installed driver (610.57.04) by issuing each ioctl and reading the result:
//   * 0x23 -> EINVAL: not a valid escape at all. The RM allocation escape is
//     0x2B, which allocates a client and then device/subdevice objects
//     (returned handles land in the 0xC1D0_0000 client and 0xCAF0_0000 device
//     ranges with status NV_OK).
//   * 0x2A is the RM control escape; issuing a control with 0x2B routes it into
//     the allocator instead (it answers with an allocation status).
//   * Registration is NV_ESC_REGISTER_FD (201, i.e. NV_IOCTL_BASE + 1, see
//     common/inc/nv-ioctl-numbers.h), it must be issued ON THE DEVICE fd and
//     pass the CONTROL fd as its argument. Every other combination
//     (0x27, array sizes 8, ctl fd on nvidiactl, ...) returns EINVAL.
const NV_IOCTL_MAGIC: u8 = b'F';
const NV_ESC_RM_ALLOC: u8 = 0x2B;
const NV_ESC_RM_CONTROL: u8 = 0x2A;
const NV_ESC_REGISTER_FD: u8 = 201;

const NV01_DEVICE_0: u32 = 0x00000080;
const NV20_SUBDEVICE_0: u32 = 0x00002080;

// NVIDIA RM Control API constants
// These values are verified to match NVIDIA driver headers and LACT implementation
const NV2080_CTRL_CMD_FB_GET_INFO: u32 = 0x20800101;
const NV2080_CTRL_FB_INFO_INDEX_RAM_TYPE: u32 = 0x01;
const NV2080_CTRL_FB_INFO_INDEX_BUS_WIDTH: u32 = 0x02;
const NV2080_CTRL_FB_INFO_INDEX_MEMORYINFO_VENDOR_ID: u32 = 0x06;

type NvHandle = u32;

#[allow(non_snake_case)]
#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct NVOS21_PARAMETERS {
    hRoot: NvHandle,
    hObjectParent: NvHandle,
    hObjectNew: NvHandle,
    hClass: u32,
    pAllocParms: *mut std::ffi::c_void,
    status: u32,
}

#[allow(non_snake_case)]
#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct NVOS64_PARAMETERS {
    hRoot: NvHandle,
    hObjectParent: NvHandle,
    hObjectNew: NvHandle,
    hClass: u32,
    pAllocParms: *mut std::ffi::c_void,
    pRightsRequested: *mut std::ffi::c_void,
    paramsSize: u32,
    flags: u32,
    status: u32,
}

#[allow(non_snake_case)]
#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct NVOS54_PARAMETERS {
    hClient: NvHandle,
    hObject: NvHandle,
    cmd: u32,
    flags: u32,
    params: *mut std::ffi::c_void,
    paramsSize: u32,
    status: u32,
}

#[allow(non_snake_case)]
#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct NV0080_ALLOC_PARAMETERS {
    deviceId: u32,
    hClientShare: NvHandle,
    hTargetClient: NvHandle,
    hTargetDevice: NvHandle,
    flags: u32,
    pad: [u32; 2],
}

#[allow(non_snake_case)]
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
struct NV2080_ALLOC_PARAMETERS {
    subdeviceNumber: u32,
}

#[allow(non_snake_case)]
#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct NV2080_CTRL_FB_GET_INFO_PARAMS {
    fbInfoListSize: u32,
    fbInfoList: *mut NV2080_CTRL_FB_INFO,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct NV2080_CTRL_FB_INFO {
    index: u32,
    data: u32,
}

ioctl_readwrite!(rm_alloc_nvos21, NV_IOCTL_MAGIC, NV_ESC_RM_ALLOC, NVOS21_PARAMETERS);
ioctl_readwrite!(rm_alloc_nvos64, NV_IOCTL_MAGIC, NV_ESC_RM_ALLOC, NVOS64_PARAMETERS);
ioctl_readwrite!(register_fd, NV_IOCTL_MAGIC, NV_ESC_REGISTER_FD, RawFd);
ioctl_readwrite!(rm_control_nvos54, NV_IOCTL_MAGIC, NV_ESC_RM_CONTROL, NVOS54_PARAMETERS);

struct NvidiaDriverHandle {
    nvidiactl_fd: std::fs::File,
    #[allow(dead_code)] // Keeps the device file descriptor open for the lifetime of this handle
    device_fd: std::fs::File,
    client_handle: NvHandle,
    subdevice_handle: NvHandle,
}

impl NvidiaDriverHandle {
    fn open(minor_number: u32) -> Result<Self> {
        log::debug!(target: "hw.detect", "NvidiaDriverHandle::open: Attempting to open for minor {}", minor_number);
        
        // Open /dev/nvidiactl with enhanced error reporting
        let nvidiactl_fd = fs::File::options()
            .read(true)
            .write(true)
            .open("/dev/nvidiactl")
            .with_context(|| "Failed to open /dev/nvidiactl - ensure NVIDIA driver is loaded and you have permissions (try: sudo usermod -aG video <username>)")?;
        log::debug!(target: "hw.detect", "NvidiaDriverHandle::open: Opened /dev/nvidiactl");

        // Open device-specific file with enhanced error reporting
        let device_path = format!("/dev/nvidia{}", minor_number);
        let device_fd = fs::File::options()
            .read(true)
            .write(true)
            .open(&device_path)
            .with_context(|| format!("Failed to open {} - GPU device may not exist or is not accessible", device_path))?;
        log::debug!(target: "hw.detect", "NvidiaDriverHandle::open: Opened /dev/nvidia{}", minor_number);

        // Registration goes the other way round from what it may look like: the
        // ioctl is issued on the DEVICE fd and carries the CONTROL fd. Measured
        // on driver 610.57.04: issuing it on nvidiactl (any fd, any size) fails
        // with EINVAL, and so does issuing 0x27 on the device fd.
        let mut control_fd_raw = nvidiactl_fd.as_raw_fd();
        unsafe {
            register_fd(device_fd.as_raw_fd(), &mut control_fd_raw)
                .with_context(|| format!("Failed to register the control FD for /dev/nvidia{} via IOCTL NV_ESC_REGISTER_FD", minor_number))?;
        }
        log::debug!(target: "hw.detect", "NvidiaDriverHandle::open: Registered /dev/nvidiactl fd on the device fd");

        let mut client_params: NVOS21_PARAMETERS = unsafe { std::mem::zeroed() };
        unsafe {
            rm_alloc_nvos21(nvidiactl_fd.as_raw_fd(), &mut client_params)
                .with_context(|| "Failed to allocate NVIDIA RM client handle via IOCTL NV_ESC_RM_ALLOC")?;
        }
        let client_handle = client_params.hObjectNew;
        log::debug!(target: "hw.detect", "NvidiaDriverHandle::open: Got client_handle=0x{:x}", client_handle);

        let mut alloc_params: NV0080_ALLOC_PARAMETERS = unsafe { std::mem::zeroed() };
        alloc_params.deviceId = minor_number;
        let mut device_request = NVOS64_PARAMETERS {
            hRoot: client_handle,
            hObjectParent: client_handle,
            hObjectNew: 0,
            hClass: NV01_DEVICE_0,
            pAllocParms: &mut alloc_params as *mut _ as *mut _,
            pRightsRequested: std::ptr::null_mut(),
            paramsSize: std::mem::size_of::<NV0080_ALLOC_PARAMETERS>() as u32,
            flags: 0,
            status: 0,
        };
        unsafe {
            rm_alloc_nvos64(nvidiactl_fd.as_raw_fd(), &mut device_request)
                .with_context(|| "Failed IOCTL NV_ESC_RM_ALLOC for device object")?;
        }
        if device_request.status != 0 {
            log::error!(target: "hw.detect", "NvidiaDriverHandle::open: Failed to alloc device handle: status=0x{:08x}", device_request.status);
            return Err(anyhow!("Failed to alloc device handle: status=0x{:08x}", device_request.status));
        }
        let device_handle = device_request.hObjectNew;
        log::debug!(target: "hw.detect", "NvidiaDriverHandle::open: Got device_handle=0x{:x}", device_handle);

        let mut subdevice_alloc: NV2080_ALLOC_PARAMETERS = Default::default();
        let mut subdevice_request = NVOS64_PARAMETERS {
            hRoot: client_handle,
            hObjectParent: device_handle,
            hObjectNew: 0,
            hClass: NV20_SUBDEVICE_0,
            pAllocParms: &mut subdevice_alloc as *mut _ as *mut _,
            pRightsRequested: std::ptr::null_mut(),
            paramsSize: std::mem::size_of::<NV2080_ALLOC_PARAMETERS>() as u32,
            flags: 0,
            status: 0,
        };
        unsafe {
            rm_alloc_nvos64(nvidiactl_fd.as_raw_fd(), &mut subdevice_request)
                .with_context(|| "Failed IOCTL NV_ESC_RM_ALLOC for subdevice object")?;
        }
        if subdevice_request.status != 0 {
            log::error!(target: "hw.detect", "NvidiaDriverHandle::open: Failed to alloc subdevice handle: status=0x{:08x}", subdevice_request.status);
            return Err(anyhow!("Failed to alloc subdevice handle: status=0x{:08x}", subdevice_request.status));
        }
        log::debug!(target: "hw.detect", "NvidiaDriverHandle::open: Got subdevice_handle=0x{:x}, successfully opened", subdevice_request.hObjectNew);

        Ok(Self {
            nvidiactl_fd,
            device_fd,
            client_handle,
            subdevice_handle: subdevice_request.hObjectNew,
        })
    }

    fn get_fb_info(&self, index: u32) -> Result<u32> {
        let mut info = NV2080_CTRL_FB_INFO { index, data: 0 };
        let mut params = NV2080_CTRL_FB_GET_INFO_PARAMS {
            fbInfoListSize: 1,
            fbInfoList: &mut info,
        };
        let mut request = NVOS54_PARAMETERS {
            hClient: self.client_handle,
            hObject: self.subdevice_handle,
            cmd: NV2080_CTRL_CMD_FB_GET_INFO,
            flags: 0,
            params: &mut params as *mut _ as *mut _,
            paramsSize: std::mem::size_of::<NV2080_CTRL_FB_GET_INFO_PARAMS>() as u32,
            status: 0,
        };
        log::debug!(target: "hw.detect", "get_fb_info: index=0x{:02x}, hClient=0x{:x}, hObject=0x{:x}, cmd=0x{:08x}, paramsSize={}",
            index, self.client_handle, self.subdevice_handle, request.cmd, request.paramsSize);
        
        let ioctl_result = unsafe {
            rm_control_nvos54(self.nvidiactl_fd.as_raw_fd(), &mut request)
        };
        
        // Log IOCTL result
        if let Err(ref e) = ioctl_result {
            log::error!(target: "hw.detect", "get_fb_info: IOCTL failed for index 0x{:02x}: errno={}, error={}", 
                index, 
                std::io::Error::last_os_error().raw_os_error().unwrap_or(-1),
                e);
            return Err(anyhow!("IOCTL NV_ESC_RM_CONTROL failed: {}", e));
        }
        
        log::debug!(target: "hw.detect", "get_fb_info: IOCTL succeeded, request.status=0x{:08x}, info.data=0x{:08x}", 
            request.status, info.data);
        
        if request.status != 0 {
            // Decode common NVIDIA RM error codes
            // Decoded from the driver's own table: common/inc/nvstatuscodes.h
            // in the installed kernel source. The values previously hardcoded
            // here (0x01/0x02/0x05...) belong to no released driver, so every
            // real failure was reported as "UNKNOWN_ERROR".
            let error_desc = match request.status {
                0x0000001a => "NV_ERR_INSUFFICIENT_RESOURCES",
                0x0000001b => "NV_ERR_INSUFFICIENT_PERMISSIONS",
                0x0000001c => "NV_ERR_INSUFFICIENT_POWER",
                0x0000001e => "NV_ERR_INVALID_ADDRESS",
                0x0000001f => "NV_ERR_INVALID_ARGUMENT",
                0x00000033 => "NV_ERR_INVALID_OBJECT_HANDLE",
                0x00000040 => "NV_ERR_INVALID_STATE",
                0x00000056 => "NV_ERR_NOT_SUPPORTED",
                0x00000057 => "NV_ERR_OBJECT_NOT_FOUND",
                0x0000ffff => "NV_ERR_GENERIC",
                _ => "UNKNOWN_ERROR",
            };
            log::error!(target: "hw.detect", "get_fb_info: RM control returned error status 0x{:08x} ({}) for index 0x{:02x}", 
                request.status, error_desc, index);
            return Err(anyhow!("RM control failed: status=0x{:08x} ({})", request.status, error_desc));
        }
        Ok(info.data)
    }
}

// NVAPI Constants
const NVAPI_LIBRARY: &str = "libnvidia-api.so.1";
const QUERY_NVAPI_INITIALIZE: u32 = 0x0150e828;
const QUERY_NVAPI_UNLOAD: u32 = 0xd22bdd7e;
const QUERY_NVAPI_ENUM_PHYSICAL_GPUS: u32 = 0xe5ac921f;
const QUERY_NVAPI_THERMALS: u32 = 0x65fe3aad;  // Undocumented - thermal sensors
const QUERY_NVAPI_VOLTAGE: u32 = 0x465f9bcf;   // Undocumented - voltage

const NVAPI_MAX_PHYSICAL_GPUS: usize = 64;

type NvPhysicalGpuHandle = *mut std::ffi::c_void;
type NvApiStatus = i32;

#[repr(C)]
struct NvApiThermals {
    version: u32,
    mask: i32,
    values: [i32; 40],
}

#[repr(C)]
struct NvApiVoltage {
    version: u32,
    flags: u32,
    padding_1: [u32; 8],
    value_uv: u32,
    padding_2: [u32; 8],
}

// Helper function to calculate VRAM bandwidth from metadata
/// Calculates VRAM bandwidth in GB/s based on memory type, bus width, and clock speed.
///
/// # Arguments
/// * `vram_type` - The type of VRAM (e.g., "GDDR6", "GDDR6X", "GDDR5")
/// * `vram_bus_width` - Bus width in bits (e.g., 256)
/// * `memory_clock_range` - Range of memory clock speeds in MHz (min, max)
///
/// # Returns
/// Bandwidth in GB/s, or None if required parameters are missing
///
/// # Formula
/// Bandwidth (GB/s) = (Max Clock MHz × Multiplier × Bus Width bits) / 8000
///
/// Where multiplier depends on memory type:
/// - GDDR6X: 16.0 (due to PAM4 signaling)
/// - GDDR6: 8.0 (quad data rate)
/// - GDDR5: 4.0 (quad data rate)
/// - Others: 2.0 (default DDR)
fn calculate_vram_bandwidth(vram_type: Option<&String>, vram_bus_width: Option<u32>, memory_clock_range: Option<(u32, u32)>) -> Option<f32> {
    if let (Some(bus), Some((_, max_mem))) = (vram_bus_width, memory_clock_range) {
        let mut multiplier = 2.0; // Default for DDR
        if let Some(t) = vram_type {
            if t.contains("GDDR6X") {
                multiplier = 16.0;
            } else if t.contains("GDDR6") {
                multiplier = 8.0;
            } else if t.contains("GDDR5") {
                multiplier = 4.0;
            }
        }
        // Bandwidth (GB/s) = (Clock * Multiplier * Bus Width) / 8 bits / 1000 MHz
        Some((max_mem as f32 * multiplier * bus as f32) / 8000.0)
    } else {
        None
    }
}

/// Rate-limited report for an unusable FB-info query. The failure is a driver
/// interface limit, not a hardware fault, so it is reported once per run
/// instead of once per poll (it used to be a warn! on every tick).
fn report_fb_info_unavailable(minor_number: u32, index: u32, error: &anyhow::Error) {
    static REPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    log::debug!(target: "hw.detect", "FB info index 0x{:02x} unavailable for minor {}: {}", index, minor_number, error);
    if !REPORTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        log::warn!(target: "hw.detect",
            "VRAM type/vendor could not be read through the driver's RM control interface (FB info 0x{:02x}, minor {}): {} — bus width falls back to NVML, type/vendor stay unknown",
            index, minor_number, error);
    }
}

/// VRAM type/vendor/bus width with an NVML fallback for the bus width.
///
/// The FB-info query is the only source for type and vendor, but its list
/// pointer cannot be resolved through NV_ESC_RM_CONTROL on every driver version
/// (measured on 610.57.04: NV_ERR_INVALID_ADDRESS). The bus width has a proper
/// NVML accessor, so publish that rather than nothing.
fn resolve_vram_identity(
    device: &nvml_wrapper::Device,
    minor_number: u32,
) -> (Option<String>, Option<String>, Option<u32>) {
    let (vram_type, vram_vendor, bus_width, _bandwidth) = get_vram_info(minor_number);
    let bus_width = match bus_width {
        Some(bits) if bits > 0 => Some(bits),
        _ => {
            let from_nvml = device.memory_bus_width().ok().filter(|bits| *bits > 0);
            if from_nvml.is_some() {
                log::debug!(target: "hw.detect",
                    "VRAM bus width for minor {} taken from NVML ({:?} bits) because the RM control query was unavailable",
                    minor_number, from_nvml);
            }
            from_nvml
        }
    };
    (vram_type, vram_vendor, bus_width)
}

// Function to get NVIDIA extended stats (hotspot, memory temp, voltage)
fn get_vram_info(minor_number: u32) -> (Option<String>, Option<String>, Option<u32>, Option<f32>) {
    // Returns (type, vendor, bus_width, bandwidth)
    log::debug!(target: "hw.detect", "Attempting to get VRAM info for NVIDIA device minor {}", minor_number);
    match NvidiaDriverHandle::open(minor_number) {
        Ok(handle) => {
            let ram_type_val = handle.get_fb_info(NV2080_CTRL_FB_INFO_INDEX_RAM_TYPE)
                .map_err(|e| report_fb_info_unavailable(minor_number, NV2080_CTRL_FB_INFO_INDEX_RAM_TYPE, &e))
                .ok();

            let bus_width = handle.get_fb_info(NV2080_CTRL_FB_INFO_INDEX_BUS_WIDTH)
                .map_err(|e| report_fb_info_unavailable(minor_number, NV2080_CTRL_FB_INFO_INDEX_BUS_WIDTH, &e))
                .ok();

            let vendor_id = handle.get_fb_info(NV2080_CTRL_FB_INFO_INDEX_MEMORYINFO_VENDOR_ID)
                .map_err(|e| report_fb_info_unavailable(minor_number, NV2080_CTRL_FB_INFO_INDEX_MEMORYINFO_VENDOR_ID, &e))
                .ok();

            log::debug!(target: "hw.detect", "VRAM raw info for minor {}: type={:?}, bus={:?}, vendor={:?}",
                minor_number, ram_type_val, bus_width, vendor_id);
            log::info!(target: "hw.detect",
                "hw.detect GPU minor {}: FB-info raw codes ram_type=0x{:x} bus_width={:?} vendor=0x{:x}",
                minor_number,
                ram_type_val.unwrap_or(0),
                bus_width,
                vendor_id.unwrap_or(0));
            
            // Log summary of detection results
            if ram_type_val.is_some() || bus_width.is_some() || vendor_id.is_some() {
                log::debug!(target: "hw.detect", "Successfully retrieved partial VRAM info for minor {}: type={}, bus={}, vendor={}",
                    minor_number, 
                    ram_type_val.map(|v| format!("0x{:08x}", v)).unwrap_or_else(|| "None".to_string()),
                    bus_width.map(|v| format!("{} bits", v)).unwrap_or_else(|| "None".to_string()),
                    vendor_id.map(|v| format!("0x{:08x}", v)).unwrap_or_else(|| "None".to_string()));
            } else {
                log::warn!(target: "hw.detect", "Failed to retrieve any VRAM info for minor {} - all queries returned errors", minor_number);
            }

            let ram_type = ram_type_val.map(|v| match v {
                0x00000001 => "SDRAM",
                0x00000002 => "DDR1",
                0x00000003 => "DDR2",
                0x00000004 => "DDR3",
                0x00000005 => "GDDR2",
                0x00000006 => "GDDR3",
                0x00000007 => "GDDR4",
                0x00000008 => "GDDR5",
                0x00000009 => "LPDDR2",
                0x0000000A => "GDDR5X",
                0x0000000B => "GDDR6",
                0x0000000C => "GDDR6X",
                0x0000000D => "HBM1",
                0x0000000E => "HBM2",
                0x0000000F => "HBM3",
                0x00000010 => "LPDDR4",
                0x00000011 => "LPDDR5",
                0x00000012 => "GDDR7",
                _ => "Unknown",
            }.to_string());

            let vendor = vendor_id.map(|v| match v {
                0x00000001 => "Micron",
                0x00000002 => "Samsung",
                0x00000003 => "Qimonda",
                0x00000004 => "Elpida",
                0x00000005 => "Etron",
                0x00000006 => "Nanya",
                0x00000007 => "Hynix",
                0x00000008 => "Mosel",
                0x00000009 => "Winbond",
                0x0000000A => "ESMT",
                _ => "Unknown",
            }.to_string());

            // Bandwidth calculation: (Clock * 2 (for DDR) * BusWidth) / 8 / 1000?
            // Actually bandwidth is complex to calculate without current clock.
            // NVML already provides memory bandwidth in some versions but not all.
            // nvidia/nvidia.rs doesn't seem to calculate it, just returns None.

            // The elements of the FB-info list are not documented in the driver's
            // open headers, so a returned word is only used when it is a value the
            // hardware can really report. A mis-decoded number must never become a
            // confident label ("93-bit") in the UI: unknown is better than wrong,
            // and resolve_vram_identity() has an NVML fallback for the bus width.
            let bus_width = bus_width.filter(|bits| (32..=1024).contains(bits) && bits % 32 == 0);
            if bus_width.is_none() {
                log::debug!(target: "hw.detect",
                    "Discarding implausible FB-info bus width for minor {} (raw {:?})",
                    minor_number, bus_width);
            }

            (ram_type, vendor, bus_width, None)
        }
        Err(e) => {
            // Enhanced error logging with more diagnostics
            let error_msg = format!("{}", e);
            
            // Check for common error conditions
            if error_msg.contains("Permission denied") || error_msg.contains("EACCES") {
                log::error!(target: "hw.detect", 
                    "Failed to open NvidiaDriverHandle for minor {}: Permission denied - Daemon may need to run as root or user needs to be in 'video' group. Error: {}", 
                    minor_number, e);
            } else if error_msg.contains("No such file") || error_msg.contains("ENOENT") {
                log::error!(target: "hw.detect", 
                    "Failed to open NvidiaDriverHandle for minor {}: Device not found - NVIDIA driver may not be loaded or GPU is not available. Error: {}", 
                    minor_number, e);
            } else if error_msg.contains("Device or resource busy") || error_msg.contains("EBUSY") {
                log::error!(target: "hw.detect", 
                    "Failed to open NvidiaDriverHandle for minor {}: Device busy - Another process may be using the GPU. Error: {}", 
                    minor_number, e);
            } else {
                log::error!(target: "hw.detect", 
                    "Failed to open NvidiaDriverHandle for minor {}: {} - Check that NVIDIA driver is loaded and device /dev/nvidia{} exists", 
                    minor_number, e, minor_number);
            }
            
            (None, None, None, None)
        }
    }
}

fn get_nvidia_extended_stats(gpu_index: u32) -> (Option<f32>, Option<f32>, Option<f32>) {
    // Returns (hotspot_temp, memory_temp, voltage_v)
    // Note: This loads and initializes NVAPI on each call. This is acceptable for 
    // polling intervals of 1+ seconds but may need optimization for higher frequencies.
    
    unsafe {
        // Load library - must be kept alive until after unload is called
        let lib = match libloading::Library::new(NVAPI_LIBRARY) {
            Ok(l) => l,
            Err(_) => return (None, None, None),
        };
        
        // Get query interface function
        let query_interface: libloading::Symbol<unsafe extern "C" fn(u32) -> *const ()> = 
            match lib.get(b"nvapi_QueryInterface\0") {
                Ok(f) => f,
                Err(_) => return (None, None, None),
            };
        
        // Initialize NVAPI
        let init_fn = query_interface(QUERY_NVAPI_INITIALIZE);
        if init_fn.is_null() { return (None, None, None); }
        let init: unsafe extern "C" fn() -> NvApiStatus = mem::transmute(init_fn);
        let init_result = init();
        
        // Helper to safely unload NVAPI before returning
        let safe_unload = || {
            let unload_fn = query_interface(QUERY_NVAPI_UNLOAD);
            if !unload_fn.is_null() {
                let unload: unsafe extern "C" fn() -> NvApiStatus = mem::transmute(unload_fn);
                let _ = unload();
            }
        };
        
        if init_result != 0 {
            return (None, None, None);
        }
        
        // Enumerate GPUs
        let enum_fn = query_interface(QUERY_NVAPI_ENUM_PHYSICAL_GPUS);
        if enum_fn.is_null() {
            safe_unload();
            return (None, None, None);
        }
        let enum_gpus: unsafe extern "C" fn(
            handles: &mut [NvPhysicalGpuHandle; NVAPI_MAX_PHYSICAL_GPUS],
            count: &mut u32,
        ) -> NvApiStatus = mem::transmute(enum_fn);
        
        let mut handles = [std::ptr::null_mut(); NVAPI_MAX_PHYSICAL_GPUS];
        let mut count = 0u32;
        if enum_gpus(&mut handles, &mut count) != 0 {
            safe_unload();
            return (None, None, None);
        }
        
        if gpu_index >= count {
            safe_unload();
            return (None, None, None);
        }
        let handle = handles[gpu_index as usize];
        
        let mut hotspot_temp = None;
        let mut memory_temp = None;
        let mut voltage = None;
        
        // Get thermals
        let thermals_fn = query_interface(QUERY_NVAPI_THERMALS);
        if !thermals_fn.is_null() {
            let get_thermals: unsafe extern "C" fn(
                handle: NvPhysicalGpuHandle,
                sensors: &mut NvApiThermals,
            ) -> NvApiStatus = mem::transmute(thermals_fn);
            
            // Calculate mask by probing (some GPUs fail if unsupported bits are set)
            let mut mask = 1;
            let mut sensors = NvApiThermals {
                version: (mem::size_of::<NvApiThermals>() | (2 << 16)) as u32,
                mask: 1,
                values: [0; 40],
            };

            if get_thermals(handle, &mut sensors) == 0 {
                for bit in 0..32 {
                    sensors.mask = 1 << bit;
                    if get_thermals(handle, &mut sensors) != 0 {
                        mask = sensors.mask - 1;
                        break;
                    }
                    if bit == 31 { mask = !0; }
                }
            }

            sensors.mask = mask;
            if get_thermals(handle, &mut sensors) == 0 {
                // Hotspot is at index 9
                let hotspot_raw = sensors.values[9] as f32 / 256.0;
                if hotspot_raw > 0.0 && hotspot_raw < 255.0 {
                    hotspot_temp = Some(hotspot_raw);
                }
                
                // VRAM/Memory is at index 15
                let vram_raw = sensors.values[15] as f32 / 256.0;
                if vram_raw > 0.0 && vram_raw < 255.0 {
                    memory_temp = Some(vram_raw);
                }
            }
        }
        
        // Get voltage
        let voltage_fn = query_interface(QUERY_NVAPI_VOLTAGE);
        if !voltage_fn.is_null() {
            let get_voltage: unsafe extern "C" fn(
                handle: NvPhysicalGpuHandle,
                data: &mut NvApiVoltage,
            ) -> NvApiStatus = mem::transmute(voltage_fn);
            
            let mut volt_data = NvApiVoltage {
                version: (mem::size_of::<NvApiVoltage>() | (1 << 16)) as u32,
                flags: 0,
                padding_1: [0; 8],
                value_uv: 0,
                padding_2: [0; 8],
            };
            
            if get_voltage(handle, &mut volt_data) == 0 && volt_data.value_uv > 0 {
                voltage = Some(volt_data.value_uv as f32 / 1_000_000.0); // Convert µV to V
            }
        }
        
        // Unload NVAPI before library is dropped
        safe_unload();
        
        // Keep library alive until after all NVAPI calls and unload
        drop(lib);
        
        (hotspot_temp, memory_temp, voltage)
    }
}

/// Validate the power sample on an explicitly requested cold wake only.
/// A successful NVML call can return stale pre-ready data (753 W on a
/// 150 W laptop). Never clamp it into a plausible-looking measurement.
fn settle_cold_power(
    initial: u32,
    maximum: u32,
    mut read: impl FnMut() -> Result<u32>,
    mut wait: impl FnMut(),
) -> Result<u32> {
    if initial <= maximum {
        return Ok(initial);
    }
    let mut last = format!("{} mW exceeds {} mW", initial, maximum);
    // At most ten additional reads and one second of intentional waiting.
    // NVML itself is synchronous; this does not impose a driver-call timeout.
    for _ in 0..10 {
        wait();
        match read() {
            Ok(value) if value <= maximum => return Ok(value),
            Ok(value) => last = format!("{} mW exceeds {} mW", value, maximum),
            Err(error) => last = error.to_string(),
        }
    }
    Err(anyhow!("Cold GPU power telemetry not ready after 10 retries: {}", last))
}

#[cfg(test)]
mod cold_power_tests {
    use super::settle_cold_power;

    #[test]
    fn valid_initial_sample_needs_no_retry() {
        assert_eq!(settle_cold_power(0, 150_000,
            || panic!("unexpected read"), || panic!("unexpected wait")).unwrap(), 0);
    }

    #[test]
    fn retries_invalid_sample_without_clamping() {
        let mut reads = 0;
        let mut waits = 0;
        let value = settle_cold_power(753_173, 150_000, || {
            reads += 1;
            Ok(if reads < 3 { 753_173 } else { 31_088 })
        }, || waits += 1).unwrap();
        assert_eq!((value, reads, waits), (31_088, 3, 3));
    }

    #[test]
    fn persistent_invalid_sample_returns_error_with_bounded_retries() {
        let mut reads = 0;
        let mut waits = 0;
        let error = settle_cold_power(753_173, 150_000,
            || { reads += 1; Ok(753_173) }, || waits += 1).unwrap_err();
        assert_eq!((reads, waits), (10, 10));
        assert!(error.to_string().contains("not ready after 10 retries"));
    }

    #[test]
    fn driver_errors_do_not_turn_into_a_power_value() {
        let error = settle_cold_power(753_173, 150_000,
            || Err(anyhow::anyhow!("GPU lost")), || {}).unwrap_err();
        assert!(error.to_string().contains("GPU lost"));
    }
}

/// Currently applied GPU offsets, from daemon state rather than from a sample.
/// These are authoritative "what is set right now" values, so they stay
/// available even when the GPU itself may not be queried.
fn current_gpu_offsets(index: u32) -> (Option<i32>, Option<i32>, Option<i32>, Option<i32>) {
    {
        let stats = crate::hardware_control::lock_or_recover(
            &crate::CURRENT_GPU_OVERCLOCK_STATS,
            "CURRENT_GPU_OVERCLOCK_STATS",
        );
        if let Some(ref stats) = *stats {
            return (
                Some(stats.freq_offset),
                Some(stats.drain_offset),
                Some(stats.power_offset),
                Some(stats.total_offset),
            );
        }
    }
    let manual = crate::hardware_control::lock_or_recover(
        &crate::MANUAL_GPU_OFFSETS,
        "MANUAL_GPU_OFFSETS",
    );
    match manual.get(&index) {
        Some(offsets) => (None, None, None, Some(offsets.0.round() as i32)),
        None => (None, None, None, None),
    }
}

/// Payload for an adapter whose tier forbids invasive queries (P3+ idle-down, or
/// runtime-suspended).
///
/// Every dynamic field is deliberately blank. Clocks, load, power, voltage and
/// both temperatures are NOT carried over from the previous sample: a stale
/// number rendered as live is the exact defect this replaces, and `None`
/// renders in the GUI as "not queried". `performance_state` is likewise not
/// back-filled — the tier state that remembers the last p-state is control
/// input for the poller and never leaves the daemon as telemetry.
///
/// What is attached: the sysfs `runtime_status` word, static device metadata
/// (name, VRAM type/vendor/bus/total, clock ranges, supported p-states — the
/// same kind of static capability data as VRAM total) and the applied offsets.
fn degraded_gpu_info(device: &SysfsNvidiaDevice) -> GpuInfo {
    let name = gpu_name_for_bdf(&device.bdf).unwrap_or_else(|| "NVIDIA GPU".to_string());
    // Static capability metadata is cached under the NVML index; the index is
    // resolved from the BDF learned during a live pass, never guessed from list
    // position. Without a learned association the metadata is absent (the fields
    // stay blank) rather than borrowed from another adapter.
    let nvml_index = nvml_index_for_bdf(&device.bdf);
    let metadata = nvml_index.and_then(|index| {
        let cache = crate::hardware_control::lock_or_recover(&NVIDIA_METADATA_CACHE, "NVIDIA_METADATA_CACHE");
        cache.get(&index).cloned()
    });

    let (vram_type, vram_vendor, vram_bus_width, vram_bandwidth, vram_total) =
        match &metadata {
            Some(meta) => {
                let bandwidth = calculate_vram_bandwidth(
                    meta.vram_type.as_ref(),
                    meta.vram_bus_width,
                    meta.memory_clock_range,
                );
                (
                    meta.vram_type.clone(),
                    meta.vram_vendor.clone(),
                    meta.vram_bus_width,
                    bandwidth,
                    meta.vram_total,
                )
            }
            None => (None, None, None, None, None),
        };

    log::debug!(
        target: "hw.detect",
        "GPU {}: runtime_status=\"{}\" — no NVML/NVAPI query this tick; dynamic fields published blank",
        device.bdf, device.runtime_status
    );

    // Offsets come from daemon state addressed by the NVML index the GUI uses, so
    // they are resolved through the same learned BDF mapping.
    let (freq_offset, drain_offset, power_offset, total_offset) =
        match nvml_index {
            Some(index) => current_gpu_offsets(index),
            None => (None, None, None, None),
        };

    GpuInfo {
        pci_bus_id: Some(device.bdf.clone()),
        process_snapshot: GpuProcessSnapshot::default(),
        vram_memory: None,
        name,
        gpu_type: GpuType::Discrete,
        runtime_status: Some(device.runtime_status.clone()),
        performance_state: None,
        frequency: None,
        memory_frequency: None,
        temperature: None,
        hotspot_temperature: None,
        memory_temperature: None,
        load: None,
        power: None,
        voltage: None,
        freq_offset,
        drain_offset,
        power_offset,
        total_offset,
        min_core_clock: None,
        max_core_clock: None,
        min_memory_clock: None,
        max_memory_clock: None,
        core_clock_range: metadata.as_ref().and_then(|m| m.core_clock_range),
        memory_clock_range: metadata.as_ref().and_then(|m| m.memory_clock_range),
        core_offset_limits: metadata.as_ref().and_then(|m| m.core_offset_limits),
        memory_offset_limits: metadata.as_ref().and_then(|m| m.memory_offset_limits),
        is_desktop: false,
        architecture: metadata.as_ref().and_then(|m| m.architecture.clone()),
        nvml_index,
        driver_version: None,
        supported_p_states: metadata
            .as_ref()
            .map(|m| m.supported_p_states.clone())
            .unwrap_or_default(),
        supports_power_limit: metadata.as_ref().and_then(|m| m.power_limit_range).is_some(),
        power_limit_range: metadata.as_ref().and_then(|m| m.power_limit_range),
        supports_gpu_offset: metadata.as_ref().map(|m| m.supports_gpu_offset).unwrap_or(false),
        supports_mem_offset: metadata.as_ref().map(|m| m.supports_mem_offset).unwrap_or(false),
        fan_speed_range: None,
        vram_type,
        vram_vendor,
        vram_bus_width,
        vram_bandwidth,
        vram_total,
    }
}

fn get_nvidia_gpu_info() -> Result<Vec<GpuInfo>> {
    let (manual_clocks_enabled, _advanced_control_enabled) = {
        let state = crate::hardware_control::lock_or_recover(&crate::GPU_DAEMON_STATE, "GPU_DAEMON_STATE");
        state.as_ref().map_or((false, false), |s| (s.manual_clocks, s.advanced_control))
    };

    // 1. sysfs only: one entry per adapter, carrying its own BDF and runtime-PM
    // word. Reading these never wakes the GPU, so this is safe on every tick and
    // is the sole input to the tier decision and to the published
    // runtime_status field. Adapters are identified by BDF here, not by an
    // assumed NVML index: the two orders need not agree.
    let sysfs_devices = sysfs_nvidia_devices();

    if sysfs_devices.is_empty() {
        return Ok(vec![]);
    }

    // -----------------------------------------------------------------------
    // Tier decision — the single gate for invasive queries on this tick.
    //
    // The only input is the sysfs runtime_status word above, so a suspended
    // adapter is never touched. Rules (see gpu_poll_tier):
    //   P0..P2  -> live NVML + NVAPI query every tick, no cached values
    //   work at any p-state -> live too (light 3D work sits on P3/P5)
    //   no work -> quiet: sysfs only, all telemetry blank, GPU idles down
    //   suspended -> sysfs only, and never a probe (it would wake the GPU)
    //
    // The state is keyed by BDF, so it follows the adapter rather than a list
    // position.
    // -----------------------------------------------------------------------
    let tiers: Vec<GpuPollTier> = {
        let states = crate::hardware_control::lock_or_recover(&GPU_POLL_STATE, "GPU_POLL_STATE");
        sysfs_devices
            .iter()
            .map(|dev| {
                gpu_poll_tier(
                    states.get(&poll_state_key(Some(&dev.bdf), 0)),
                    &dev.runtime_status,
                )
            })
            .collect()
    };

    // Nothing may be queried this tick: build the sysfs-only payloads WITHOUT
    // initialising NVML at all, since loading the library and taking a device
    // handle is itself a touch that resets the autosuspend timer.
    if !tiers.contains(&GpuPollTier::Live) {
        let mut gpus = Vec::new();
        for dev in &sysfs_devices {
            record_gpu_observation(&poll_state_key(Some(&dev.bdf), 0), &dev.runtime_status);
            gpus.push(degraded_gpu_info(dev));
        }
        return Ok(gpus);
    }

    // At least one adapter is in the live tier. It gets a full NVML + NVAPI
    // pass; every adapter that is not gets a sysfs-only payload below.
    let nvml = get_nvml()?;
    let mut gpus = Vec::new();

    // 2. Identity mapping: pair every NVML device with the BDF NVML reports for
    // it, and remember the association for the sysfs-only paths. This is what
    // replaces "sorted PCI devices[i] == NVML device index[i]".
    let mut slots: Vec<NvmlDeviceSlot> = Vec::new();
    for i in 0..nvml.device_count().unwrap_or(0) {
        let bdf = match nvml.device_by_index(i) {
            Ok(device) => nvml_pci_bdf(&device),
            Err(_) => None,
        };
        if let Some(ref bdf) = bdf {
            remember_nvml_bdf(i, bdf);
        }
        slots.push(NvmlDeviceSlot { index: i, bdf });
    }
    let statuses = runtime_status_by_device(&slots, &sysfs_devices);

    let driver_version = nvml.sys_driver_version().ok();

    let device_count = nvml.device_count().unwrap_or(0);
    for i in 0..device_count {
        // Identity for this NVML device, and its own runtime-PM word: resolved
        // by BDF above, so a permuted sysfs enumeration cannot hand this GPU
        // another adapter's state.
        let device_bdf = slots
            .get(i as usize)
            .and_then(|slot| slot.bdf.clone());
        let poll_key = poll_state_key(device_bdf.as_deref(), i);
        let status_from_sysfs = statuses
            .get(i as usize)
            .cloned()
            .flatten()
            .unwrap_or_default();

        // Tier for this adapter: sysfs word + the state keyed by its own BDF.
        // Anything other than Live gets the sysfs-only payload: no query, no
        // cached numbers, no plausible-looking stale value.
        let tier = {
            let states = crate::hardware_control::lock_or_recover(&GPU_POLL_STATE, "GPU_POLL_STATE");
            gpu_poll_tier(states.get(&poll_key), &status_from_sysfs)
        };
        if tier != GpuPollTier::Live {
            record_gpu_observation(&poll_key, &status_from_sysfs);
            if let Some(dev) = sysfs_devices
                .iter()
                .find(|dev| device_bdf.as_deref().map(|bdf| dev.bdf.eq_ignore_ascii_case(bdf)).unwrap_or(false))
            {
                gpus.push(degraded_gpu_info(dev));
            }
            continue;
        }

        // First authorized pass after a suspend: validate the power sample,
        // which can be stale nonsense right after a D3 wake.
        let cold_wake = gpu_woke_from_suspend(&poll_key);

        // Active GPU - proceed with NVML
        let device = match nvml.device_by_index(i) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let name = device.name().unwrap_or_else(|_| "NVIDIA GPU".to_string());

        // Name and identity are properties of the adapter: cached under its BDF
        // so the sysfs-only paths can find them again without assuming an order.
        // `device_bdf` was already resolved from NVML's own PCI identity above.
        let pci_identity = device_bdf.clone();
        if let Some(ref bdf) = pci_identity {
            remember_gpu_name(bdf, &name);
            if let Ok(memory) = device.memory_info() {
                crate::gpu_activity::record_memory(bdf, memory.free, memory.used, memory.total);
            }
        }
        let gpu_type = GpuType::Discrete;

        // Get performance state
        use nvml_wrapper::enum_wrappers::device::PerformanceState;
        let pstate = device.performance_state().ok();

        // Two distinct values, never merged: `performance_state` is the NVML
        // performance state from THIS pass, `runtime_status` is the sysfs
        // runtime-PM word carried alongside it.
        let performance_state = match pstate {
            Some(state) => {
                // Map nvml_wrapper::PerformanceState to "PX" format for GUI
                Some(match state {
                    PerformanceState::Zero => "P0".to_string(),
                    PerformanceState::One => "P1".to_string(),
                    PerformanceState::Two => "P2".to_string(),
                    PerformanceState::Three => "P3".to_string(),
                    PerformanceState::Four => "P4".to_string(),
                    PerformanceState::Five => "P5".to_string(),
                    PerformanceState::Six => "P6".to_string(),
                    PerformanceState::Seven => "P7".to_string(),
                    PerformanceState::Eight => "P8".to_string(),
                    PerformanceState::Nine => "P9".to_string(),
                    PerformanceState::Ten => "P10".to_string(),
                    PerformanceState::Eleven => "P11".to_string(),
                    PerformanceState::Twelve => "P12".to_string(),
                    PerformanceState::Thirteen => "P13".to_string(),
                    PerformanceState::Fourteen => "P14".to_string(),
                    PerformanceState::Fifteen => "P15".to_string(),
                    PerformanceState::Unknown => "unknown".to_string(),
                })
            }
            None => None,
        };

        let pstate_val = pstate.map(|s| match s {
            PerformanceState::Zero => 0,
            PerformanceState::One => 1,
            PerformanceState::Two => 2,
            PerformanceState::Three => 3,
            PerformanceState::Four => 4,
            PerformanceState::Five => 5,
            PerformanceState::Six => 6,
            PerformanceState::Seven => 7,
            PerformanceState::Eight => 8,
            PerformanceState::Nine => 9,
            PerformanceState::Ten => 10,
            PerformanceState::Eleven => 11,
            PerformanceState::Twelve => 12,
            PerformanceState::Thirteen => 13,
            PerformanceState::Fourteen => 14,
            PerformanceState::Fifteen => 15,
            PerformanceState::Unknown => 99,
        });


        // This pass only runs when the tier authorized it (P0..P2, work at any
        // P-state, a fresh wake, or the bounded re-probe), so NVML is queried
        // fully and NVAPI comes with it: when the adapter is being polled, every
        // statistic is reported live rather than some of them silently missing.
        let should_poll_nvapi = true;

        let (frequency, memory_frequency, temperature, load, mut power) = (
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
        );

        // Only the first pass after a suspend may wait: a successful NVML read
        // right after a D3 wake can report stale nonsense (measured 753 W on a
        // 150 W-max GPU). Ordinary ticks never acquire a retry loop.
        if cold_wake {
            if let Some(watts) = power {
                power = match device.power_management_limit_constraints() {
                    Ok(bounds) if bounds.max_limit > 0 => match settle_cold_power(
                        (watts * 1000.0).round() as u32,
                        bounds.max_limit,
                        || device.power_usage().map_err(Into::into),
                        || std::thread::sleep(std::time::Duration::from_millis(100)),
                    ) {
                        Ok(value) => Some(value as f32 / 1000.0),
                        Err(error) => {
                            // Keep the remaining telemetry and reach the flag
                            // consumption below; an early error would leave the
                            // one-shot armed and wake again on a monitor tick.
                            log::warn!(target: "hw.detect", "GPU {}: {}", i, error);
                            None
                        }
                    },
                    // No trustworthy bound: do not certify cold power data.
                    _ => None,
                };
            }
        }

        // Get extended stats via NVAPI
        let (hotspot_temp, memory_temp, nvapi_voltage) = if should_poll_nvapi {
            get_nvidia_extended_stats(i)
        } else {
            (None, None, None)
        };

        let voltage = nvapi_voltage;

        // Bookkeeping for the NEXT tick's tier — the pass's own p-state and
        // utilization decide it: P0..P2 always live, P3 live while it has work,
        // otherwise quiet so the kernel can finish its idle-down. Neither value
        // leaves the daemon as telemetry.
        record_gpu_probe(&poll_key, &status_from_sysfs, pstate_val, load);

        let (min_core_clock, max_core_clock) = (None, None); // NVML wrapper 0.11 doesn't have a getter

        let num_fans = device.num_fans().unwrap_or(0);
        let is_desktop = false; // Deprecated, using capability flags instead

        // Get metadata from cache or fetch it
        let metadata = {
            let mut cache = crate::hardware_control::lock_or_recover(&NVIDIA_METADATA_CACHE, "NVIDIA_METADATA_CACHE");
            if let Some(meta) = cache.get(&i) {
                log::debug!(target: "hw.detect", "GPU {}: Using cached metadata", i);
                // Check if cached metadata is incomplete - if so, retry getting it
                // This handles cases where initial detection failed (GPU suspended, driver not ready, etc.)
                let incomplete_vram = meta.vram_type.is_none() && meta.vram_vendor.is_none() && meta.vram_bus_width.is_none();
                let incomplete_ranges = meta.core_clock_range.is_none() || meta.memory_clock_range.is_none();
                let incomplete_offsets = meta.core_offset_limits.is_none() || meta.memory_offset_limits.is_none();

                if incomplete_vram || incomplete_ranges || incomplete_offsets {
                    log::debug!(target: "hw.detect", "GPU {}: Cached metadata is incomplete, retrying detection", i);
                    let minor_number = device.minor_number().unwrap_or(i);
                    
                    let (vram_type, vram_vendor, vram_bus_width) = if incomplete_vram {
                        resolve_vram_identity(&device, minor_number)
                    } else {
                        (meta.vram_type.clone(), meta.vram_vendor.clone(), meta.vram_bus_width)
                    };

                    let core_range = if meta.core_clock_range.is_none() {
                        get_base_gpu_clock_ranges(&device).ok()
                    } else {
                        meta.core_clock_range
                    };

                    let mem_range = if meta.memory_clock_range.is_none() {
                        get_base_memory_clock_ranges(&device).ok()
                    } else {
                        meta.memory_clock_range
                    };

                    let core_offset_limits = if meta.core_offset_limits.is_none() {
                        device.clock_offset(Clock::Graphics, PerformanceState::Zero).ok()
                            .map(|o| (o.min_clock_offset_mhz, o.max_clock_offset_mhz))
                    } else {
                        meta.core_offset_limits
                    };

                    let memory_offset_limits = if meta.memory_offset_limits.is_none() {
                        device.clock_offset(Clock::Memory, PerformanceState::Zero).ok()
                            .map(|o| (o.min_clock_offset_mhz, o.max_clock_offset_mhz))
                    } else {
                        meta.memory_offset_limits
                    };

                    let updated_meta = NvidiaMetadata {
                        vram_type,
                        vram_vendor,
                        vram_bus_width,
                        core_clock_range: core_range,
                        memory_clock_range: mem_range,
                        core_offset_limits,
                        memory_offset_limits,
                        ..meta.clone()
                    };
                    cache.insert(i, updated_meta.clone());
                    updated_meta
                } else {
                    log::debug!(target: "hw.detect", "GPU {}: Using cached metadata", i);
                    meta.clone()
                }
            } else {
                log::info!(target: "hw.detect", "GPU {}: Initializing metadata cache (first detection)", i);
                let arch = device.architecture().ok().map(|arch| arch.to_string());
                let p_states = device.supported_performance_states().ok()
                    .map(|states| states.iter().map(|s| format!("{:?}", s)).collect())
                    .unwrap_or_default();
                let p_limit_range = match device.power_management_limit_constraints() {
                    Ok(constraints) => Some((constraints.min_limit / 1000, constraints.max_limit / 1000)),
                    Err(_) => None,
                };
                let s_gpu_offset = device.clock_offset(Clock::Graphics, PerformanceState::Zero).is_ok();
                let s_mem_offset = device.clock_offset(Clock::Memory, PerformanceState::Zero).is_ok();

                let vram_total = device.memory_info().ok().map(|m| m.total / 1024 / 1024);

                let minor_number = device.minor_number().unwrap_or(i);
                log::debug!(target: "hw.detect", "GPU {}: Performing initial VRAM detection for minor {}", i, minor_number);
                let (vram_type, vram_vendor, vram_bus_width) = resolve_vram_identity(&device, minor_number);

                let core_range = get_base_gpu_clock_ranges(&device).ok();
                let mem_range = get_base_memory_clock_ranges(&device).ok();


                let core_offset_limits = device.clock_offset(Clock::Graphics, PerformanceState::Zero).ok()
                    .map(|o| (o.min_clock_offset_mhz, o.max_clock_offset_mhz));
                let memory_offset_limits = device.clock_offset(Clock::Memory, PerformanceState::Zero).ok()
                    .map(|o| (o.min_clock_offset_mhz, o.max_clock_offset_mhz));

                let meta = NvidiaMetadata {
                    architecture: arch,
                    supported_p_states: p_states,
                    power_limit_range: p_limit_range,
                    supports_gpu_offset: s_gpu_offset,
                    supports_mem_offset: s_mem_offset,
                    vram_type,
                    vram_vendor,
                    vram_bus_width,
                    vram_total,
                    core_clock_range: core_range,
                    memory_clock_range: mem_range,
                    core_offset_limits,
                    memory_offset_limits,
                };
                cache.insert(i, meta.clone());
                meta
            }
        };

        let architecture = metadata.architecture;
        let supported_p_states = metadata.supported_p_states;
        let power_limit_range = metadata.power_limit_range;
        let supports_power_limit = power_limit_range.is_some();
        let supports_gpu_offset = metadata.supports_gpu_offset;
        let supports_mem_offset = metadata.supports_mem_offset;
        let v_type = metadata.vram_type;
        let v_vendor = metadata.vram_vendor;
        let v_bus = metadata.vram_bus_width;
        let v_total = metadata.vram_total;
        // Apply current offset to the cached base range for real-time reporting
        let core_clock_range = metadata.core_clock_range.map(|(min, max)| {
            let offset = {
                let map = crate::hardware_control::lock_or_recover(&crate::MANUAL_GPU_OFFSETS, "MANUAL_GPU_OFFSETS");
                map.get(&i).map(|(c, _)| *c).unwrap_or(0.0)
            };
            ((min as f32 + offset).max(0.0) as u32, (max as f32 + offset).max(0.0) as u32)
        });
        let memory_clock_range = metadata.memory_clock_range;
        let core_offset_limits = metadata.core_offset_limits;
        let memory_offset_limits = metadata.memory_offset_limits;
        let v_bw = calculate_vram_bandwidth(v_type.as_ref(), v_bus, memory_clock_range);

        // Log VRAM info for diagnostics (rate-limited)
        static LAST_VRAM_LOG: Lazy<Mutex<HashMap<u32, Instant>>> = Lazy::new(|| Mutex::new(HashMap::new()));
        let should_log_vram = {
            let mut last_log = crate::hardware_control::lock_or_recover(&LAST_VRAM_LOG, "LAST_VRAM_LOG");
            match last_log.get(&i) {
                Some(instant) if instant.elapsed() < std::time::Duration::from_secs(60) => false,
                _ => {
                    last_log.insert(i, Instant::now());
                    true
                }
            }
        };

        if should_log_vram {
            if v_type.is_some() || v_vendor.is_some() || v_bus.is_some() {
                if v_bw.is_none() && memory_clock_range.is_none() {
                    log::debug!(target: "hw.detect",
                        "GPU {}: VRAM detected - Type: {:?}, Vendor: {:?}, Bus Width: {:?} bits, Bandwidth: N/A (memory clock range unknown)",
                        i, v_type, v_vendor, v_bus);
                } else {
                    log::debug!(target: "hw.detect",
                        "GPU {}: VRAM detected - Type: {:?}, Vendor: {:?}, Bus Width: {:?} bits, Bandwidth: {:?} GB/s",
                        i, v_type, v_vendor, v_bus, v_bw);
                }
            } else {
                log::warn!(target: "hw.detect",
                    "GPU {}: VRAM info not available - Check daemon logs above for detailed error messages",
                    i);
            }
        }

        let mut gpu_info = GpuInfo {
                pci_bus_id: pci_identity,
                process_snapshot: GpuProcessSnapshot::default(),
                vram_memory: None,
            name: name.clone(),
            gpu_type,
            runtime_status: Some(status_from_sysfs.clone()),
            performance_state,
            frequency,
            memory_frequency,
            temperature,
            hotspot_temperature: hotspot_temp,
            memory_temperature: memory_temp,
            load,
            power,
            voltage,
            freq_offset: None,
            drain_offset: None,
            power_offset: None,
            total_offset: None,
            min_core_clock,
            max_core_clock,
            min_memory_clock: None,
            max_memory_clock: None,
            core_clock_range,
            memory_clock_range,
            core_offset_limits,
            memory_offset_limits,
            is_desktop,
            architecture,
            nvml_index: Some(i),
            driver_version: driver_version.clone(),
            supported_p_states,
            supports_power_limit,
            power_limit_range,
            supports_gpu_offset,
            supports_mem_offset,
            fan_speed_range: if num_fans > 0 { Some((0, 100)) } else { None },
            vram_type: v_type,
            vram_vendor: v_vendor,
            vram_bus_width: v_bus,
            vram_bandwidth: v_bw,
            vram_total: v_total,
        };

        // Currently applied offsets (daemon state: an authoritative now-value,
        // not a cached measurement).
        if name.to_lowercase().contains("nvidia") && manual_clocks_enabled {
            let (freq, drain, power_offset, total) = current_gpu_offsets(i);
            gpu_info.freq_offset = freq;
            gpu_info.drain_offset = drain;
            gpu_info.power_offset = power_offset;
            gpu_info.total_offset = total;
        }

        gpus.push(gpu_info);
    }

    // Adapters sysfs lists but NVML did not expose (or could not identify): they
    // still belong in the payload, with their own BDF and runtime word, and with
    // telemetry blank. Identified by BDF, so this cannot duplicate a device the
    // loop above already reported.
    for dev in &sysfs_devices {
        let covered = slots.iter().any(|slot| {
            slot.bdf
                .as_deref()
                .map(|bdf| bdf.eq_ignore_ascii_case(&dev.bdf))
                .unwrap_or(false)
        });
        if !covered {
            record_gpu_observation(&poll_state_key(Some(&dev.bdf), 0), &dev.runtime_status);
            gpus.push(degraded_gpu_info(dev));
        }
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
        let (ssid, channel, channel_width, channel_freq, tx_bitrate, rx_bitrate, iw_rx_bytes, iw_tx_bytes, iw_signal) = get_wifi_details(&interface);

        // Signal level priority: iw > /proc/net/wireless
        let signal_level = iw_signal.or_else(|| read_wifi_signal(&interface));

        // RX/TX bytes priority: iw > /sys/class/net
        let (final_tx_bytes, final_rx_bytes) = match (iw_tx_bytes, iw_rx_bytes) {
            (Some(tx), Some(rx)) => (tx, rx),
            _ => read_wifi_bytes(&interface).unwrap_or((0, 0)),
        };

        // Calculate actual throughput
        let (tx_rate, rx_rate) = read_wifi_rates(&interface, final_tx_bytes, final_rx_bytes);
        
        log::debug!(target: "hw.detect", "WiFi {} details: SSID={:?}, Signal={:?}, Channel={:?}, Rates={:?}/{:?}",
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
            channel_freq,
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
    let mut freq = None;
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
                } else if let Some(pos) = lower.find("freq:") {
                    let part = trimmed[pos + 5..].trim();
                    freq = part.split_whitespace().next().and_then(|s| s.parse().ok());
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

    (ssid, channel, width, freq, tx_bitrate, rx_bitrate, rx_bytes, tx_bytes, signal)
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
    let mut stats = crate::hardware_control::lock_or_recover(&PREVIOUS_NET_STATS, "PREVIOUS_NET_STATS");
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

fn normalize_uid(uid: String) -> String {
    let trimmed = uid.trim();
    // Check if it looks like a MAC address (6 pairs of hex digits separated by colons or dashes, or just 12 hex digits)
    let is_mac = (trimmed.len() == 17 && (trimmed.contains(':') || trimmed.contains('-'))) ||
                 (trimmed.len() == 12 && trimmed.chars().all(|c| c.is_ascii_hexdigit()));

    if is_mac {
        trimmed.replace(':', "").replace('-', "").to_lowercase()
    } else {
        trimmed.to_string()
    }
}

pub fn get_gamepad_info() -> Result<Vec<GamepadInfo>> {
    let mut gamepads = Vec::new();
    let mut seen_uids = std::collections::HashSet::new();

    if let Ok(entries) = fs::read_dir("/sys/class/input") {
        for entry in entries.flatten() {
            let input_name = entry.file_name().to_string_lossy().to_string();
            if input_name.starts_with("input") {
                let path = entry.path();

                // Find event child to check udev properties
                let mut is_gamepad = false;
                let mut udev_uid = None;
                if let Ok(children) = fs::read_dir(&path) {
                    for child in children.flatten() {
                        let child_name = child.file_name().to_string_lossy().to_string();
                        if child_name.starts_with("event") {
                            if let Ok(uevent) = fs::read_to_string(child.path().join("uevent")) {
                                let mut major = None;
                                let mut minor = None;
                                for line in uevent.lines() {
                                    if let Some(val) = line.strip_prefix("MAJOR=") {
                                        major = Some(val);
                                    } else if let Some(val) = line.strip_prefix("MINOR=") {
                                        minor = Some(val);
                                    }
                                }

                                if let (Some(maj), Some(min)) = (major, minor) {
                                    let udev_path = format!("/run/udev/data/c{}:{}", maj, min);
                                    if let Ok(udev_data) = fs::read_to_string(udev_path) {
                                        if udev_data.contains("E:ID_INPUT_JOYSTICK=1") {
                                            is_gamepad = true;
                                        }

                                        // Try to find a stable UID from udev data
                                        let mut id_path = None;
                                        let mut id_serial = None;
                                        let mut id_serial_short = None;
                                        for line in udev_data.lines() {
                                            if let Some(val) = line.strip_prefix("E:ID_PATH=") {
                                                id_path = Some(val.to_string());
                                            } else if let Some(val) = line.strip_prefix("E:ID_SERIAL_SHORT=") {
                                                id_serial_short = Some(val.to_string());
                                            } else if let Some(val) = line.strip_prefix("E:ID_SERIAL=") {
                                                id_serial = Some(val.to_string());
                                            }
                                        }
                                        // Priority: Serial Short > Serial > Path
                        udev_uid = id_serial_short.or(id_serial).or(id_path).map(normalize_uid);

                                        if is_gamepad {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Fallback to name heuristic if udev info is missing or not joystick
                if !is_gamepad {
                    if let Ok(device_name) = fs::read_to_string(path.join("name")) {
                        let device_name_lower = device_name.to_lowercase();
                        if (device_name_lower.contains("controller") ||
                            device_name_lower.contains("gamepad") ||
                            device_name_lower.contains("joystick")) &&
                           !device_name_lower.contains("keyboard") {
                            is_gamepad = true;
                        }
                    }
                }

                // Apply exclusions based on name (even if udev says joystick)
                if is_gamepad {
                    if let Ok(device_name) = fs::read_to_string(path.join("name")) {
                        let device_name_lower = device_name.to_lowercase();
                        if device_name_lower.contains("touchpad") ||
                           device_name_lower.contains("motion sensors") ||
                           device_name_lower.contains("consumer control") ||
                           device_name_lower.contains("system control") {
                            is_gamepad = false;
                        }
                    }
                }

                if is_gamepad {
                    if let Ok(device_name) = fs::read_to_string(path.join("name")) {
                        let device_name = device_name.trim().to_string();
                        // Identity chain for the remembered-gamepad database:
                        // udev serials/ID_PATH when published, then the HID
                        // MAC (`device/uniq`), and ONLY as a last resort the
                        // sysfs input path -- which the kernel renumbers on
                        // every reconnect (BT pads), so it is volatile.
                        let (uid, uid_source) = match &udev_uid {
                            Some(u) => (u.clone(), "udev"),
                            None => (
                                path.to_string_lossy().to_string(),
                                "sysfs-path-volatile",
                            ),
                        };

                        // Prefer the uniq (MAC address): stable across
                        // connection types; overrides the chain above.
                        let mut uid = uid;
                        if let Ok(uniq) = fs::read_to_string(path.join("device/uniq")) {
                            let uniq = uniq.trim();
                            if !uniq.is_empty() && uniq != "00:00:00:00:00:00" {
                                uid = normalize_uid(uniq.to_string());
                            }
                        }

                        log::debug!(target: "hw.detect", "Gamepad '{}': uid={} (source={})", device_name, uid, uid_source);

                        if !seen_uids.contains(&uid) {
                            let bustype = fs::read_to_string(path.join("id/bustype"))
                                .ok()
                                .and_then(|s| u16::from_str_radix(s.trim(), 16).ok())
                                .unwrap_or(0);

                            let connection_type = match bustype {
                                0x03 => ConnectionType::Wired,
                                0x05 => ConnectionType::Wireless,
                                _ => ConnectionType::Unknown,
                            };

                            let (battery_level, power_status) = find_battery_for_input(&path);

                            gamepads.push(GamepadInfo {
                                name: device_name,
                                id: input_name,
                                uid: uid.clone(),
                                status: GamepadStatus::Connected,
                                battery_level,
                                connection_type,
                                power_status,
                            });
                            seen_uids.insert(uid);
                        }
                    }
                }
            }
        }
    }

    Ok(gamepads)
}

fn find_battery_for_input(input_path: &Path) -> (Option<u8>, PowerStatus) {
    if let Ok(device_path) = fs::canonicalize(input_path.join("device")) {
        let mut current = Some(device_path.as_path());
        while let Some(path) = current {
            let ps_path = path.join("power_supply");
            if ps_path.exists() {
                if let Ok(ps_entries) = fs::read_dir(ps_path) {
                    for ps_entry in ps_entries.flatten() {
                        let mut level = None;
                        let mut status = PowerStatus::Unknown;
                        if let Ok(cap) = fs::read_to_string(ps_entry.path().join("capacity")) {
                            level = cap.trim().parse().ok();
                        }
                        if let Ok(st) = fs::read_to_string(ps_entry.path().join("status")) {
                            status = match st.trim().to_lowercase().as_str() {
                                "charging" => PowerStatus::Charging,
                                "discharging" => PowerStatus::Discharging,
                                "full" => PowerStatus::Full,
                                _ => PowerStatus::Unknown,
                            };
                        }
                        return (level, status);
                    }
                }
            }

            // Also check for power_supply as a sibling in some cases or child of parent
            current = path.parent();
            if let Some(p) = current {
                if p == Path::new("/sys/devices") || p == Path::new("/sys") {
                    break;
                }
            }
        }
    }

    // Fallback: search all power supplies for names matching the input device
    if let Ok(ps_entries) = fs::read_dir("/sys/class/power_supply") {
        for ps_entry in ps_entries.flatten() {
            let ps_name = ps_entry.file_name().to_string_lossy().to_lowercase();
            if ps_name.contains("controller") || ps_name.contains("gamepad") {
                 let mut level = None;
                 let mut status = PowerStatus::Unknown;
                 if let Ok(cap) = fs::read_to_string(ps_entry.path().join("capacity")) {
                     level = cap.trim().parse().ok();
                 }
                 if let Ok(st) = fs::read_to_string(ps_entry.path().join("status")) {
                     status = match st.trim().to_lowercase().as_str() {
                         "charging" => PowerStatus::Charging,
                         "discharging" => PowerStatus::Discharging,
                         "full" => PowerStatus::Full,
                         _ => PowerStatus::Unknown,
                     };
                 }
                 return (level, status);
            }
        }
    }

    (None, PowerStatus::Unknown)
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

    let charge_full = read_sysfs_u64(&format!("{}/charge_full", base)).or_else(|_| read_sysfs_u64(&format!("{}/energy_full", base))).ok();
    let charge_full_design = read_sysfs_u64(&format!("{}/charge_full_design", base)).or_else(|_| read_sysfs_u64(&format!("{}/energy_full_design", base))).ok();

    let battery_health = if let (Some(full), Some(design)) = (charge_full, charge_full_design) {
        if design > 0 {
            Some((full as f32 / design as f32) * 100.0)
        } else {
            None
        }
    } else {
        None
    };

    Ok(BatteryInfo {
        status,
        voltage_mv: read_sysfs_u64(&format!("{}/voltage_now", base))? / 1000,
        current_ma: read_sysfs_i64(&format!("{}/current_now", base))? / 1000,
        charge_percent: read_sysfs_u64(&format!("{}/capacity", base))?,
        capacity_mah: charge_full.unwrap_or(0) / 1000,
        battery_health,
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
    let mut stats = crate::hardware_control::lock_or_recover(&PREVIOUS_STORAGE_STATS, "PREVIOUS_STORAGE_STATS");
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
// ---------------------------------------------------------------------------
// dGPU polling-tier tests.
//
// The pure tests pin the policy (they run everywhere). The HIL tests exercise
// the REAL detection path against the REAL driver at the REAL cadence:
//
//   1. Live tier (P0..P2 or a fresh wake) -> full NVML + NVAPI pass, and the
//      published payload carries live values, never a cached one.
//   2. Quiet tier (P3 and deeper) -> sysfs-only payload: runtime_status is
//      published, every dynamic field is blank, and the GPU is still able to
//      finish its idle-down and reach runtime_status "suspended".
//   3. Suspended -> sysfs-only payload and the call must not wake the GPU.
//
// Run the HIL set with:
//   cargo test -p lapsphere-daemon --bin lapsphere-daemon -- --ignored --nocapture
// ---------------------------------------------------------------------------
#[cfg(test)]
mod gpu_tier_tests {
    use super::*;

    fn state(status: &str, pstate: Option<u8>, probed_secs_ago: Option<u64>) -> GpuPollState {
        state_with_load(status, pstate, None, probed_secs_ago)
    }

    fn state_with_load(
        status: &str,
        pstate: Option<u8>,
        load: Option<f32>,
        probed_secs_ago: Option<u64>,
    ) -> GpuPollState {
        GpuPollState {
            last_runtime_status: Some(status.to_string()),
            last_pstate: pstate,
            last_load: load,
            last_probe: probed_secs_ago.map(|secs| Instant::now() - std::time::Duration::from_secs(secs)),
        }
    }

    #[test]
    fn suspended_never_authorizes_a_probe() {
        assert_eq!(gpu_poll_tier(None, "suspended"), GpuPollTier::Suspended);
        assert_eq!(
            gpu_poll_tier(Some(&state("suspended", Some(0), Some(0))), "suspended"),
            GpuPollTier::Suspended,
            "even a freshly observed P0 must not authorize a wake"
        );
    }

    #[test]
    fn p0_to_p2_stays_live_every_tick() {
        for pstate in 0..=2u8 {
            assert_eq!(
                gpu_poll_tier(Some(&state("active", Some(pstate), Some(0))), "active"),
                GpuPollTier::Live,
                "P{} must be polled live on every tick",
                pstate
            );
        }
    }

    #[test]
    fn work_is_live_at_any_pstate_above_p0() {
        // Light 3D work lands on P3 and oscillates to P5: both must report live
        // values at the configured polling rate, not blanks.
        for pstate in [3u8, 5, 8] {
            assert_eq!(
                gpu_poll_tier(Some(&state_with_load("active", Some(pstate), Some(27.0), Some(1))), "active"),
                GpuPollTier::Live,
                "P{} with utilization must be polled live",
                pstate
            );
        }
    }

    #[test]
    fn low_utilization_noise_does_not_open_the_live_tier() {
        assert_eq!(
            gpu_poll_tier(Some(&state_with_load("active", Some(8), Some(0.5), Some(1))), "active"),
            GpuPollTier::Quiet,
            "a sub-threshold blip must not hold the adapter awake"
        );
        assert_eq!(
            gpu_poll_tier(Some(&state_with_load("active", Some(3), None, Some(1))), "active"),
            GpuPollTier::Quiet,
            "an unmeasurable P3 must not be pinned awake"
        );
    }

    #[test]
    fn idle_p3_and_deeper_go_quiet_until_the_reprobe_cadence() {
        for pstate in [3u8, 5, 8] {
            assert_eq!(
                gpu_poll_tier(Some(&state_with_load("active", Some(pstate), Some(0.0), Some(1))), "active"),
                GpuPollTier::Quiet,
                "P{} with no work must not be probed while it idles down",
                pstate
            );
        }
        assert_eq!(
            gpu_poll_tier(
                Some(&state("active", Some(8), Some(QUIET_REPROBE_SECS + 1))),
                "active"
            ),
            GpuPollTier::Live,
            "the bounded re-probe must eventually notice a ramp out of P8"
        );
    }

    #[test]
    fn reprobe_cadence_outlives_the_autosuspend_window() {
        // The kernel's autosuspend delay is 20 s; the measured dGPU re-suspend
        // latency after the last NVML touch is ~27 s. A shorter cadence would
        // keep resetting the timer and pin the GPU awake.
        assert!(
            QUIET_REPROBE_SECS > 30,
            "cadence {} s would interrupt the idle-down transition",
            QUIET_REPROBE_SECS
        );
    }

    #[test]
    fn wake_from_suspend_authorizes_one_pass() {
        assert_eq!(
            gpu_poll_tier(Some(&state("suspended", Some(8), Some(1))), "active"),
            GpuPollTier::Live,
            "a suspended -> active transition must be probed"
        );
    }

    #[test]
    fn first_ever_tick_learns_the_pstate_once() {
        assert_eq!(gpu_poll_tier(None, "active"), GpuPollTier::Live);
        // A pass that could not report a p-state must not authorize a hot loop.
        assert_eq!(
            gpu_poll_tier(Some(&state("active", None, Some(1))), "active"),
            GpuPollTier::Quiet
        );
    }
}

#[cfg(test)]
mod degraded_payload_tests {
    use super::*;

    /// sysfs view of one adapter, for tests that must not depend on hardware.
    fn sysfs_device(bdf: &str, runtime_status: &str) -> SysfsNvidiaDevice {
        SysfsNvidiaDevice {
            bdf: bdf.to_string(),
            runtime_status: runtime_status.to_string(),
        }
    }

    /// The quiet/suspended payload must publish NO dynamic telemetry: no value
    /// may be carried over from an earlier sample and rendered as live.
    #[test]
    fn sysfs_only_payload_has_no_stale_telemetry() {
        let gpu = degraded_gpu_info(&sysfs_device("0000:01:00.0", "active"));
        assert_eq!(gpu.runtime_status.as_deref(), Some("active"));
        assert!(gpu.performance_state.is_none(), "perf state must not be back-filled");
        assert_eq!(gpu.frequency, None);
        assert_eq!(gpu.memory_frequency, None);
        assert_eq!(gpu.temperature, None);
        assert_eq!(gpu.hotspot_temperature, None);
        assert_eq!(gpu.memory_temperature, None);
        assert_eq!(gpu.load, None);
        assert_eq!(gpu.power, None);
        assert_eq!(gpu.voltage, None);
        assert_eq!(gpu.gpu_type, GpuType::Discrete);
    }

    #[test]
    fn suspended_payload_reports_the_pm_word_it_was_given() {
        let gpu = degraded_gpu_info(&sysfs_device("0000:01:00.0", "suspended"));
        assert_eq!(gpu.runtime_status.as_deref(), Some("suspended"));
        assert!(gpu.performance_state.is_none());
        assert_eq!(gpu.power, None);
    }
}

/// Regression coverage for the PCI/BDF <-> NVML identity mapping.
///
/// The failure mode these tests pin: correlating "sorted sysfs devices[i]" with
/// "NVML device index[i]". On a multi-GPU machine that silently attaches one
/// adapter's runtime/power state to another adapter's telemetry, so these tests
/// exercise the production mapping function with the sysfs enumeration in both
/// orders and with a deliberately mismatched device.
#[cfg(test)]
mod gpu_identity_tests {
    use super::*;

    /// sysfs view of one adapter.
    fn sysfs(bdf: &str, status: &str) -> SysfsNvidiaDevice {
        SysfsNvidiaDevice {
            bdf: bdf.to_string(),
            runtime_status: status.to_string(),
        }
    }

    /// One NVML device and the BDF NVML reports for it.
    fn slot(index: u32, bdf: Option<&str>) -> NvmlDeviceSlot {
        NvmlDeviceSlot {
            index,
            bdf: bdf.map(|b| b.to_string()),
        }
    }

    #[test]
    fn nvml_bus_ids_canonicalise_to_sysfs_directory_names() {
        // NVML reports eight domain digits; sysfs directories use four, lower
        // case. The fan path used to hand-build this and missed every file.
        assert_eq!(
            canonical_bdf("00000000:01:00.0", 0).as_deref(),
            Some("0000:01:00.0")
        );
        assert_eq!(
            canonical_bdf("00000000:0A:00.0", 0x0001).as_deref(),
            Some("0001:0a:00.0")
        );
        assert_eq!(canonical_bdf("no-colon-here", 0), None);
        assert_eq!(canonical_bdf("00000000:", 0), None);
    }

    #[test]
    fn runtime_status_follows_the_bdf_not_the_list_position() {
        let gpu_a = "0000:01:00.0"; // suspended
        let gpu_b = "0000:05:00.0"; // active
        let devices = [slot(0, Some(gpu_a)), slot(1, Some(gpu_b))];

        // sysfs listed in the same order, in reverse, and interleaved with a
        // third adapter: the per-device answer must not change.
        for order in [
            vec![sysfs(gpu_a, "suspended"), sysfs(gpu_b, "active")],
            vec![sysfs(gpu_b, "active"), sysfs(gpu_a, "suspended")],
            vec![
                sysfs("0000:00:02.0", "active"),
                sysfs(gpu_b, "active"),
                sysfs(gpu_a, "suspended"),
            ],
        ] {
            let statuses = runtime_status_by_device(&devices, &order);
            assert_eq!(
                statuses[0].as_deref(),
                Some("suspended"),
                "device 0 (BDF {}) picked up another adapter's state for order {:?}",
                gpu_a,
                order.iter().map(|d| &d.bdf).collect::<Vec<_>>()
            );
            assert_eq!(statuses[1].as_deref(), Some("active"), "device 1 (BDF {})", gpu_b);
        }
    }

    #[test]
    fn an_unlisted_bdf_yields_unknown_instead_of_a_neighbours_state() {
        let devices = [slot(0, Some("0000:02:00.0")), slot(1, Some("0000:03:00.0"))];
        let sysfs_view = [sysfs("0000:01:00.0", "suspended"), sysfs("0000:09:00.0", "active")];

        // Neither device is present in the sysfs view: both must be unknown, and
        // crucially neither may borrow the "suspended" entry that sorts first.
        let statuses = runtime_status_by_device(&devices, &sysfs_view);
        assert_eq!(statuses, vec![None, None]);
    }

    #[test]
    fn a_device_without_reported_bdf_only_falls_back_on_a_single_adapter_system() {
        let single = [sysfs("0000:01:00.0", "suspended")];
        // One NVML device, one sysfs adapter: the two views can only be the same
        // adapter, so the pre-existing single-GPU behaviour is preserved.
        assert_eq!(
            runtime_status_by_device(&[slot(0, None)], &single),
            vec![Some("suspended".to_string())]
        );

        // Two adapters and no identity: unknown, never the first entry.
        let two = [sysfs("0000:01:00.0", "suspended"), sysfs("0000:05:00.0", "active")];
        assert_eq!(
            runtime_status_by_device(&[slot(0, None), slot(1, None)], &two),
            vec![None, None]
        );
    }

    #[test]
    fn invalidated_mapping_is_reported_as_suspended_and_never_guessed() {
        let gpu_a = "0000:01:00.0";
        let gpu_b = "0000:05:00.0";
        let sysfs_view = [sysfs(gpu_a, "suspended"), sysfs(gpu_b, "active")];

        // Known association: the answer is that adapter's own word.
        assert_eq!(suspended_decision(&sysfs_view, Some(gpu_b), 2), Some(false));
        assert_eq!(suspended_decision(&sysfs_view, Some(gpu_a), 2), Some(true));
        // BDF that sysfs does not list: unknown, not "whatever sorts first".
        assert_eq!(suspended_decision(&sysfs_view, Some("0000:07:00.0"), 2), None);
        // No association on a two-adapter system: unknown (the caller refuses to
        // query, which is the conservative RTD3 answer).
        assert_eq!(suspended_decision(&sysfs_view, None, 2), None);
        // ... but a one-adapter system has only one interpretation.
        assert_eq!(suspended_decision(&[sysfs(gpu_a, "suspended")], None, 1), Some(true));
    }

    #[test]
    fn poll_state_key_is_the_adapter_identity() {
        // Same adapter, different enumeration position: same key, so the tier
        // state cannot migrate to another GPU when the order changes.
        assert_eq!(
            poll_state_key(Some("0000:01:00.0"), 0),
            poll_state_key(Some("0000:01:00.0"), 1)
        );
        assert_ne!(
            poll_state_key(Some("0000:01:00.0"), 0),
            poll_state_key(Some("0000:05:00.0"), 0)
        );
        // Case-insensitive identity, and an index-scoped fallback that cannot
        // collide with a BDF.
        assert_eq!(
            poll_state_key(Some("0000:0A:00.0"), 3),
            poll_state_key(Some("0000:0a:00.0"), 3)
        );
        assert_eq!(poll_state_key(None, 3), "index:3");
        assert_eq!(poll_state_key(Some(""), 3), "index:3");
    }

    /// End-to-end shape of the failure the audit describes: with the sysfs list
    /// reversed, GPU A must still be Suspended and GPU B Live, i.e. the tier a
    /// device receives follows its own runtime state.
    #[test]
    fn tier_decisions_survive_a_permuted_sysfs_enumeration() {
        let gpu_a = "0000:01:00.0"; // suspended in sysfs
        let gpu_b = "0000:05:00.0"; // active in sysfs
        let devices = [slot(0, Some(gpu_a)), slot(1, Some(gpu_b))];
        let forward = [sysfs(gpu_a, "suspended"), sysfs(gpu_b, "active")];
        let reversed = [sysfs(gpu_b, "active"), sysfs(gpu_a, "suspended")];

        for sysfs_view in [&forward, &reversed] {
            let statuses = runtime_status_by_device(&devices, sysfs_view);
            let tiers: Vec<GpuPollTier> = statuses
                .iter()
                .enumerate()
                .map(|(position, status)| {
                    // Resolve the per-adapter state through the identity key the
                    // production code uses, then decide the tier from this
                    // device's own status word.
                    let key = poll_state_key(
                        devices[position].bdf.as_deref(),
                        devices[position].index,
                    );
                    assert_eq!(key, devices[position].bdf.clone().unwrap().to_lowercase());
                    gpu_poll_tier(None, status.as_deref().unwrap_or(""))
                })
                .collect();
            assert_eq!(
                tiers,
                vec![GpuPollTier::Suspended, GpuPollTier::Live],
                "permuted enumeration changed a device's tier"
            );
        }
    }
}

#[cfg(test)]
mod holder_filter_tests {
    use crate::gpu_activity::is_structural_holder;

    #[test]
    fn non_blocking_holders_are_dropped() {
        let self_pid = std::process::id();
        // Confirmed on the XMG: none of these keep the dGPU from suspending.
        assert!(is_structural_holder("Xorg", 1234));
        // comm is truncated to 15 bytes by the kernel.
        assert!(is_structural_holder("nvidia-persiste", 1234));
        assert!(is_structural_holder("nvidia-persistenced", 1234));
        // The monitoring daemon's own device handle (its own PID).
        assert!(is_structural_holder("lapsphere-daemo", self_pid));
    }

    #[test]
    fn real_holders_survive_the_filter() {
        assert!(!is_structural_holder("lapsphere-daemo", 4242));
        assert!(!is_structural_holder("thorium", 1234));
        assert!(!is_structural_holder("glxgears", 1234));
        assert!(!is_structural_holder("", 1234));
    }
}

#[cfg(test)]
mod hil_gpu_tier_tests {
    // Serializes HIL tests: they share the process-global poll state and the
    // single physical GPU, while cargo runs test threads in parallel.
    static HIL_LOCK: once_cell::sync::Lazy<std::sync::Mutex<()>> =
        once_cell::sync::Lazy::new(|| std::sync::Mutex::new(()));

    use super::*;

    fn runtime_status() -> String {
        read_runtime_status("0000:01:00.0")
    }

    fn discrete(gpus: &[GpuInfo]) -> &GpuInfo {
        gpus.iter()
            .find(|g| g.gpu_type == GpuType::Discrete)
            .expect("no discrete GPU in payload")
    }

    fn wait_for(status: &str, within_secs: u64) -> bool {
        let deadline = Instant::now() + std::time::Duration::from_secs(within_secs);
        while Instant::now() < deadline {
            if runtime_status() == status {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        runtime_status() == status
    }

    /// Acceptance 2 + 3: the quiet tier publishes blanks instead of cached
    /// numbers, and because it issues no invasive call the GPU still reaches
    /// "suspended" by itself.
    ///
    /// State-agnostic on purpose: the machine may be idle (suspended) or busy
    /// (P0) when this starts, and both flows must satisfy the same invariants.
    #[test]
    #[ignore = "HIL: touches the real dGPU and waits for a real suspend"]
    fn hil_quiet_tier_publishes_no_stale_values_and_lets_gpu_suspend() {
        let _guard = HIL_LOCK.lock().unwrap();
        let _ = env_logger::Builder::from_env(env_logger::Env::default())
            .is_test(true)
            .try_init();

        // Reset the tier state so the first call is a learning pass.
        crate::hardware_control::lock_or_recover(&GPU_POLL_STATE, "GPU_POLL_STATE").clear();

        let first = get_nvidia_gpu_info().expect("first pass failed");
        let gpu = discrete(&first);
        println!(
            "[+] first pass: runtime_status={:?} perf={:?} freq={:?} load={:?} power={:?} hotspot={:?}",
            gpu.runtime_status, gpu.performance_state, gpu.frequency, gpu.load, gpu.power,
            gpu.hotspot_temperature
        );
        assert!(gpu.runtime_status.is_some(), "runtime_status must be published");
        if gpu.performance_state.is_some() {
            // Live tier: a reported p-state must come with live telemetry.
            assert!(
                gpu.frequency.is_some() && gpu.load.is_some() && gpu.power.is_some(),
                "live tier published a p-state without telemetry"
            );
        } else {
            // Suspended: sysfs-only, nothing measured.
            assert_eq!(
                gpu.runtime_status.as_deref(),
                Some("suspended"),
                "a missing p-state is only legitimate while suspended"
            );
            assert!(gpu.frequency.is_none() && gpu.power.is_none());
        }

        let mut saw_quiet_tick = false;
        let mut saw_live_tick = false;
        for tick in 0..40 {
            let gpus = get_nvidia_gpu_info().unwrap();
            let g = discrete(&gpus);
            if g.performance_state.is_none() {
                // Quiet or suspended tier: blanks only, never a value from a
                // previous sample, and the PM word must still be published.
                assert!(
                    g.frequency.is_none()
                        && g.memory_frequency.is_none()
                        && g.load.is_none()
                        && g.power.is_none()
                        && g.voltage.is_none()
                        && g.hotspot_temperature.is_none()
                        && g.memory_temperature.is_none(),
                    "tick {}: quiet tier published telemetry: freq={:?} load={:?} power={:?} hotspot={:?}",
                    tick, g.frequency, g.load, g.power, g.hotspot_temperature
                );
                assert!(
                    g.runtime_status.is_some(),
                    "tick {}: quiet tier must still publish the runtime-PM word",
                    tick
                );
                saw_quiet_tick = true;
            } else {
                assert!(
                    g.frequency.is_some() && g.load.is_some() && g.power.is_some(),
                    "tick {}: a reported p-state must come with live telemetry",
                    tick
                );
                saw_live_tick = true;
            }
            let status = runtime_status();
            println!(
                "[tick {:>2}] status={} perf={:?} freq={:?} hotspot={:?}",
                tick, status, g.performance_state, g.frequency, g.hotspot_temperature
            );
            if status == "suspended" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1000));
        }

        assert_eq!(
            runtime_status(),
            "suspended",
            "the GPU never suspended: something in the poll path keeps touching it"
        );
        assert!(
            saw_quiet_tick,
            "the quiet tier was never exercised (live={})",
            saw_live_tick
        );
        println!("[+] ACCEPTANCE: quiet tier publishes blanks and the GPU still suspends");
    }

    /// Live tier at P0..P3: every call must return live values including
    /// hotspot/memory temperature. Requires an external GPU load — run it while
    /// a game or a load generator is active. A light load (one small glxgears)
    /// lands on P3, which is the case this test exists for as much as P0.
    #[test]
    #[ignore = "HIL: needs an external P0 load on the dGPU"]
    fn hil_p0_returns_live_values_including_hotspot_on_every_call() {
        let _guard = HIL_LOCK.lock().unwrap();
        crate::hardware_control::lock_or_recover(&GPU_POLL_STATE, "GPU_POLL_STATE").clear();

        let mut p0_seen = false;
        for round in 0..6 {
            let gpus = get_nvidia_gpu_info().expect("pass failed");
            let g = discrete(&gpus);
            println!(
                "[round {}] perf={:?} runtime={:?} freq={:?} load={:?} power={:?} hotspot={:?} memtemp={:?} volt={:?}",
                round, g.performance_state, g.runtime_status, g.frequency, g.load, g.power,
                g.hotspot_temperature, g.memory_temperature, g.voltage
            );
            if let Some(pstate) = g.performance_state.as_deref() {
                // Any reported p-state comes from a live pass, and a live pass
                // publishes every statistic — no silently missing hotspot.
                p0_seen = true;
                println!("    ^ live tier at {}", pstate);
                assert!(
                    g.frequency.is_some() && g.load.is_some() && g.power.is_some(),
                    "live pass at {} must publish clocks/load/power",
                    pstate
                );
                assert!(
                    g.hotspot_temperature.is_some() && g.memory_temperature.is_some(),
                    "live pass at {} must publish hotspot and memory temperature",
                    pstate
                );
                assert!(g.voltage.is_some(), "live pass at {} must publish voltage", pstate);
            } else {
                // Quiet tick: no value may be invented for it.
                assert!(
                    g.frequency.is_none()
                        && g.load.is_none()
                        && g.power.is_none()
                        && g.hotspot_temperature.is_none(),
                    "a quiet tick must not report telemetry"
                );
                assert!(g.runtime_status.is_some(), "a quiet tick must still publish the PM word");
            }
            std::thread::sleep(std::time::Duration::from_millis(1000));
        }
        assert!(
            p0_seen,
            "no live-tier observation: run this test with a real GPU load active"
        );
    }

    /// Suspended path: the payload is sysfs-only and the call itself must not
    /// wake the GPU.
    #[test]
    #[ignore = "HIL: requires the dGPU to be runtime-suspended first"]
    fn hil_suspended_payload_does_not_wake_the_gpu() {
        let _guard = HIL_LOCK.lock().unwrap();
        crate::hardware_control::lock_or_recover(&GPU_POLL_STATE, "GPU_POLL_STATE").clear();
        assert!(
            wait_for("suspended", 75),
            "precondition failed: dGPU did not reach \"suspended\""
        );
        let gpus = get_nvidia_gpu_info().expect("suspended pass failed");
        let g = discrete(&gpus);
        assert_eq!(g.runtime_status.as_deref(), Some("suspended"));
        assert!(g.performance_state.is_none());
        assert!(g.power.is_none() && g.frequency.is_none() && g.hotspot_temperature.is_none());
        assert_eq!(runtime_status(), "suspended", "the poll woke the GPU");
        println!("[+] suspended payload is sysfs-only and did not wake the GPU");
    }

    /// "dGPU only": integrated graphics publish no status, no VRAM figures and
    /// no holder list (an iGPU never runtime-suspends, so all three are constant
    /// noise), and the holder scan runs for discrete adapters only.
    #[test]
    #[ignore = "HIL: reads the real GPU list"]
    fn hil_integrated_gpu_publishes_no_status_vram_or_holders() {
        let _guard = HIL_LOCK.lock().unwrap();
        let gpus = get_gpu_info().expect("get_gpu_info failed");
        for gpu in &gpus {
            println!(
                "[{}] {:28} runtime={:?} perf={:?} vram={:?} holders={} complete={}",
                format!("{:?}", gpu.gpu_type),
                gpu.name,
                gpu.runtime_status,
                gpu.performance_state,
                gpu.vram_memory.is_some(),
                gpu.process_snapshot.processes.len(),
                gpu.process_snapshot.complete
            );
            if gpu.gpu_type == GpuType::Integrated {
                assert!(gpu.runtime_status.is_none(), "iGPU must not publish a runtime status");
                assert!(gpu.performance_state.is_none(), "iGPU must not publish a perf state");
                assert!(gpu.vram_memory.is_none(), "iGPU must not publish VRAM figures");
                assert!(
                    gpu.process_snapshot.processes.is_empty(),
                    "iGPU must not publish a holder list"
                );
                assert!(gpu.frequency.is_some(), "iGPU telemetry itself is still published");
            }
        }
        let discrete = gpus.iter().find(|g| g.gpu_type == GpuType::Discrete);
        if let Some(discrete) = discrete {
            assert!(discrete.runtime_status.is_some(), "dGPU must publish the PM word");
            for holder in &discrete.process_snapshot.processes {
                assert!(
                    holder.pid != std::process::id(),
                    "the daemon's own handle must be filtered out"
                );
                assert!(
                    !holder.name.starts_with("Xorg") && !holder.name.starts_with("nvidia-persiste"),
                    "structural holder {} leaked into the list",
                    holder.name
                );
            }
            println!("[+] discrete holders after filtering: {:?}",
                discrete.process_snapshot.processes.iter().map(|p| p.name.clone()).collect::<Vec<_>>());
        }
    }

    /// Diagnostic for the direct-ioctl VRAM path (`get_vram_info`): prints what
    /// the driver returns for RAM type / bus width / vendor, per FB-info index.
    /// Run with the GPU awake; nothing is cached, so this is the raw driver
    /// answer to the NV_ESC_RM_ALLOC / RM_CONTROL sequence.
    #[test]
    #[ignore = "HIL: opens /dev/nvidiactl and allocates RM objects"]
    fn hil_vram_ioctl_metadata_readable() {
        let _guard = HIL_LOCK.lock().unwrap();
        println!("[*] runtime_status before: {}", runtime_status());
        match NvidiaDriverHandle::open(0) {
            Ok(handle) => {
                for (label, index) in [
                    ("RAM_TYPE", NV2080_CTRL_FB_INFO_INDEX_RAM_TYPE),
                    ("BUS_WIDTH", NV2080_CTRL_FB_INFO_INDEX_BUS_WIDTH),
                    ("VENDOR_ID", NV2080_CTRL_FB_INFO_INDEX_MEMORYINFO_VENDOR_ID),
                ] {
                    match handle.get_fb_info(index) {
                        Ok(value) => println!("[+] {} = 0x{:08x} ({})", label, value, value),
                        Err(e) => println!("[-] {} failed: {}", label, e),
                    }
                }
            }
            Err(e) => println!("[-] NvidiaDriverHandle::open(0) failed: {:#}", e),
        }
        println!("[*] runtime_status after: {}", runtime_status());
    }
}
