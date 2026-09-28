use anyhow::Result;
use lapsphere_common::types::*;
use zbus::{interface, Connection};
use std::time::Duration;
use nix::sys::signal::{raise, Signal};

const SHUTDOWN_SIGNAL_DELAY_MS: u64 = 200;

/// Acquire the hardware cache guard, converting a poisoned mutex into a
/// controlled D-Bus error instead of panicking the daemon.
macro_rules! cache_guard {
    () => {{
        match crate::HARDWARE_CACHE.lock() {
            Ok(guard) => guard,
            Err(_) => {
                return Err(zbus::fdo::Error::Failed(
                    "hardware cache mutex poisoned".to_string(),
                ))
            }
        }
    }};
}

macro_rules! log_api {
    ($method:expr, $call:expr, $ok_msg:expr) => {{
        log::debug!(target: "api.call", "{}", $method);
        let start = std::time::Instant::now();
        let res = $call;
        let duration = start.elapsed().as_millis();
        match &res {
            Ok(val) => log::debug!(target: "api.ok", "{} {} duration_ms={}", $method, $ok_msg(val), duration),
            Err(e) => log::warn!(target: "api.fail", "{} reason=\"{}\" duration_ms={}", $method, e, duration),
        }
        res.map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }};
}

macro_rules! log_api_json {
    ($method:expr, $call:expr, $ok_msg:expr) => {{
        log::debug!(target: "api.call", "{}", $method);
        let start = std::time::Instant::now();
        let res = $call;
        let duration = start.elapsed().as_millis();
        match res {
            Ok(info) => {
                log::debug!(target: "api.ok", "{} {} duration_ms={}", $method, $ok_msg(&info), duration);
                serde_json::to_string(&info).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
            }
            Err(e) => {
                log::warn!(target: "api.fail", "{} reason=\"{}\" duration_ms={}", $method, e, duration);
                Err(zbus::fdo::Error::Failed(e.to_string()))
            }
        }
    }};
}

/// Wait for an in-flight full-GPU refresh to publish, then return its payload.
///
/// Called only when `GPU_FULL_REFRESH_INFLIGHT` is already true, i.e. another
/// caller owns the refresh. Rather than starting a second `spawn_blocking`
/// (the thing that produced the arenas), wait for the owner to publish into
/// `HARDWARE_CACHE.gpu_info` and serialize that.
///
/// The wait is bounded so a refresh that dies without releasing the slot (a
/// panic inside `get_gpu_info`, a task abort) degrades to serving the cached
/// payload rather than hanging the D-Bus caller forever. Serving a payload a
/// few milliseconds old is the intended behavior of this method anyway — that
/// is what coalescing means.
async fn get_gpu_info_full_coalesced() -> Result<String, zbus::fdo::Error> {
    use std::time::{Duration, Instant};

    // Generous relative to a real NVML pass (single-digit ms typically) but
    // short enough that a wedged owner is a brief delay, not a hang.
    const MAX_WAIT: Duration = Duration::from_millis(2000);
    const POLL: Duration = Duration::from_millis(2);
    let start = Instant::now();

    while GPU_FULL_REFRESH_INFLIGHT.load(std::sync::atomic::Ordering::Acquire) {
        if start.elapsed() >= MAX_WAIT {
            log::warn!(target: "api.fail",
                "GetGpuInfoFull: coalesce wait exceeded {MAX_WAIT:?}; serving cached payload");
            break;
        }
        tokio::time::sleep(POLL).await;
    }

    // Read back what the owner published. This is a plain cache read, not a
    // hardware query, so it cannot block and cannot spawn a thread.
    let info = cache_guard!().gpu_info.clone();
    serde_json::to_string(&info).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
}

pub struct ControlInterface;

/// In-flight guard for `GetGpuInfoFull` (see `get_gpu_info_full`).
///
/// Each distinct caller used to get its own `spawn_blocking` task, and tokio's
/// blocking pool spawns an OS thread per task when saturated. Every one of
/// those threads is a candidate for a fresh glibc thread arena, each of which
/// grows to glibc's 64 MiB `HEAP_MAX_SIZE` and is NEVER freed — measured on a
/// live daemon at five 64 MiB arenas, 320 MB of the 371 MB RSS, 96% of it
/// resident-but-never-touched (`Referenced` 14.3 MB total). `arena_max`
/// defaults to `8 * ncpu` (128 here), so the kernel was always free to make
/// more. This cap is what stops new arenas forming; `MALLOC_ARENA_MAX` in the
/// unit is what bounds the damage if one slips through anyway.
static GPU_FULL_REFRESH_INFLIGHT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Outcome of a coalesced full-GPU refresh.
enum FullRefresh {
    /// This caller performed the refresh and owns the payload.
    Owned(Result<Vec<GpuInfo>>),
    /// Another caller was already refreshing; its payload was published into
    /// the shared cache and read back from there.
    Coalesced,
}

#[interface(name = "io.lapsphere.Control")]
impl ControlInterface {
    async fn get_system_info(&self) -> Result<String, zbus::fdo::Error> {
        let guard = cache_guard!();
        let info = guard.system_info.clone()
            .ok_or_else(|| zbus::fdo::Error::Failed("System info not available".to_string()))?;

        log_api_json!(
            "GetSystemInfo",
            Ok::<_, anyhow::Error>(info),
            |i: &SystemInfo| format!("model=\"{}\"", i.product_name)
        )
    }

    async fn get_memory_info(&self) -> Result<String, zbus::fdo::Error> {
        let info = cache_guard!().memory_info.clone()
            .ok_or_else(|| zbus::fdo::Error::Failed("Memory info not available".to_string()))?;

        log_api_json!(
            "GetMemoryInfo",
            Ok::<_, anyhow::Error>(info),
            |i: &MemoryInfo| format!("total_gib={:.1} used_percent={:.1}", i.total_gib, i.used_percent)
        )
    }

    async fn get_cpu_info(&self) -> Result<String, zbus::fdo::Error> {
        let info = cache_guard!().cpu_info.clone()
            .ok_or_else(|| zbus::fdo::Error::Failed("CPU info not available".to_string()))?;

        log_api_json!(
            "GetCpuInfo",
            Ok::<_, anyhow::Error>(info),
            |i: &CpuInfo| format!("model=\"{}\" cores={}", i.name, i.physical_cores)
        )
    }

    async fn get_gpu_info(&self) -> Result<String, zbus::fdo::Error> {
        let info = cache_guard!().gpu_info.clone();

        log_api_json!(
            "GetGpuInfo",
            Ok::<_, anyhow::Error>(info),
            |i: &Vec<GpuInfo>| format!("count={}", i.len())
        )
    }

    /// On-demand full NVML query (RTD3-aware hybrid strategy).
    ///
    /// The default GetGpuInfo serves the 1 Hz monitor cache, which skips NVML
    /// while the dGPU is active-but-idle (so the kernel's 20 s autosuspend
    /// timer can expire and the GPU can drop into RTD3 suspend). When the GUI
    /// stats panel needs live values (voltage, clocks, temps), it calls this:
    /// it arms FULL_NVML_REFRESH_REQUESTED, which the next detection pass
    /// consumes for exactly ONE full NVML query. Note: if the dGPU is
    /// currently suspended, this deliberately wakes it (explicit user demand
    /// beats power saving) — the same trade-off nvidia-smi makes.
    ///
    /// # Concurrency: coalescing, not rejection
    ///
    /// Concurrent callers COALESCE onto a single in-flight refresh. A caller
    /// that arrives while another refresh is running does NOT get an error and
    /// does NOT return stale data — it waits for the in-flight pass to publish
    /// into the shared cache and then returns that payload, which is at most
    /// one refresh interval old (milliseconds). The GUI therefore still sees
    /// live values at the same cadence as before.
    ///
    /// The alternative — rejecting the second caller — would be a behavior
    /// regression: two overlapping stats-panel refreshes would turn into one
    /// success and one error. The other alternative — letting every caller
    /// spawn its own task — is what created the glibc arenas documented on
    /// `GPU_FULL_REFRESH_INFLIGHT`. Coalescing is the only design that keeps
    /// the payload fresh for every caller AND bounds the thread count.
    async fn get_gpu_info_full(&self) -> Result<String, zbus::fdo::Error> {
        // Fast path: a refresh is already running. Wait for it rather than
        // starting a competing one (see the concurrency note above).
        if GPU_FULL_REFRESH_INFLIGHT.load(std::sync::atomic::Ordering::Acquire) {
            log::debug!(target: "api.call",
                "GetGpuInfoFull: refresh already in flight, coalescing onto it");
            return get_gpu_info_full_coalesced().await;
        }

        log::debug!(target: "api.call", "GetGpuInfoFull: arming one-shot full NVML refresh");

        // Claim the refresh slot. `swap` is the arbiter: exactly one caller
        // transitions false->true and does the work, so the check above and
        // the claim cannot both succeed.
        if GPU_FULL_REFRESH_INFLIGHT.swap(true, std::sync::atomic::Ordering::AcqRel) {
            // Lost the race to another caller between the load and the swap.
            log::debug!(target: "api.call",
                "GetGpuInfoFull: lost the claim race, coalescing onto the winner");
            return get_gpu_info_full_coalesced().await;
        }

        // Arm the one-shot override BEFORE spawning, so the pass we are about
        // to run sees it set. (Unchanged from the original ordering.)
        crate::FULL_NVML_REFRESH_REQUESTED.store(true, std::sync::atomic::Ordering::Relaxed);

        // Run the full pass on the blocking pool to keep the async reactor
        // responsive. At most one such task exists at a time now, so the
        // blocking pool sees at most one new thread from this path.
        let result = tokio::task::spawn_blocking(crate::hardware_detection::get_gpu_info).await;

        // Publish into the shared cache so plain GetGpuInfo serves the same
        // fresh payload until the next monitor tick, and so coalesced callers
        // have something to read.
        //
        // ORDERING: the publish MUST happen before the slot is released below.
        // A coalesced caller waits on the slot and then reads the cache; if we
        // released first, it could wake up and read the PREVIOUS payload.
        // Single error type (`String`) across all three arms: the two hardware
        // errors are already Strings, and the serialize error is mapped here so
        // the match arms agree.
        let outcome: Result<String, String> = match result {
            Ok(Ok(info)) => {
                cache_guard!().gpu_info = info.clone();
                serde_json::to_string(&info).map_err(|e| e.to_string())
            }
            Ok(Err(e)) => Err(e.to_string()),
            Err(e) => Err(format!("GPU refresh task failed: {}", e)),
        };

        // Release the slot on EVERY exit path — success, inner error, or join
        // error — or the daemon would wedge: every future call would coalesce
        // onto a refresh that no longer exists. This is the one exit, so there
        // is no path that can skip it.
        GPU_FULL_REFRESH_INFLIGHT.store(false, std::sync::atomic::Ordering::Release);

        outcome.map_err(zbus::fdo::Error::Failed)
    }

    async fn get_battery_info(&self) -> Result<String, zbus::fdo::Error> {
        let info = cache_guard!().battery_info.clone()
            .ok_or_else(|| zbus::fdo::Error::Failed("Battery info not available".to_string()))?;

        log_api_json!(
            "GetBatteryInfo",
            Ok::<_, anyhow::Error>(info),
            |i: &BatteryInfo| format!("percent={} status=\"{}\"", i.charge_percent, i.status)
        )
    }

    async fn get_storage_device_info(&self) -> Result<String, zbus::fdo::Error> {
        let info = cache_guard!().storage_device_info.clone();

        log_api_json!(
            "GetStorageDeviceInfo",
            Ok::<_, anyhow::Error>(info),
            |i: &Vec<StorageDevice>| format!("count={}", i.len())
        )
    }

    async fn get_mount_info(&self) -> Result<String, zbus::fdo::Error> {
        let info = cache_guard!().mount_info.clone();

        log_api_json!(
            "GetMountInfo",
            Ok::<_, anyhow::Error>(info),
            |i: &Vec<MountInfo>| format!("count={}", i.len())
        )
    }

    async fn get_wifi_info(&self) -> Result<String, zbus::fdo::Error> {
        let info = cache_guard!().wifi_info.clone();

        log_api_json!(
            "GetWifiInfo",
            Ok::<_, anyhow::Error>(info),
            |i: &Vec<WiFiInfo>| format!("interfaces={}", i.len())
        )
    }

    async fn get_gamepad_info(&self) -> Result<String, zbus::fdo::Error> {
        let info = cache_guard!().gamepad_info.clone();

        log_api_json!(
            "GetGamepadInfo",
            Ok::<_, anyhow::Error>(info),
            |i: &Vec<GamepadInfo>| format!("count={}", i.len())
        )
    }

    async fn set_cpu_governor(&self, governor: &str) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetCpuGovernor",
            crate::hardware_control::set_cpu_governor(governor),
            |_| format!("governor=\"{}\"", governor)
        )
    }

    async fn set_cpu_frequency_limits(
        &self,
        min_freq: u64,
        max_freq: u64,
    ) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetCpuFrequencyLimits",
            crate::hardware_control::set_cpu_frequency_limits(min_freq, max_freq),
            |_| format!("min={} max={}", min_freq, max_freq)
        )
    }

    async fn set_cpu_boost(&self, enabled: bool) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetCpuBoost",
            crate::hardware_control::set_cpu_boost(enabled),
            |_| format!("enabled={}", enabled)
        )
    }

    async fn set_smt(&self, enabled: bool) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetSmt",
            crate::hardware_control::set_smt(enabled),
            |_| format!("enabled={}", enabled)
        )
    }

    async fn set_amd_pstate_status(&self, status: &str) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetAmdPstateStatus",
            crate::hardware_control::set_amd_pstate_status(status),
            |_| format!("status=\"{}\"", status)
        )
    }

    async fn set_intel_pstate_status(&self, status: &str) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetIntelPstateStatus",
            crate::hardware_control::set_intel_pstate_status(status),
            |_| format!("status=\"{}\"", status)
        )
    }

    async fn apply_profile(&self, profile_json: &str) -> Result<(), zbus::fdo::Error> {
        log::debug!(target: "api.call", "ApplyProfile");
        let start = std::time::Instant::now();
        let res = (|| async {
            let profile: Profile = serde_json::from_str(profile_json)?;
            // Update GPU daemon state for dynamic overclocking
            {
                let mut state = crate::hardware_control::lock_or_recover(&crate::GPU_DAEMON_STATE, "GPU_DAEMON_STATE");
                *state = Some(profile.gpu_settings.clone());
            }
            crate::hardware_control::apply_profile(&profile)
        })().await;

        let duration = start.elapsed().as_millis();
        match &res {
            Ok(_) => log::debug!(target: "api.ok", "ApplyProfile duration_ms={}", duration),
            Err(e) => log::warn!(target: "api.fail", "ApplyProfile reason=\"{}\" duration_ms={}", e, duration),
        }
        res.map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn get_tdp_profiles(&self) -> Result<String, zbus::fdo::Error> {
        log_api_json!(
            "GetTdpProfiles",
            crate::hardware_detection::get_tdp_profiles(),
            |i: &Vec<String>| format!("count={}", i.len())
        )
    }

    async fn get_current_tdp_profile(&self) -> Result<String, zbus::fdo::Error> {
        log_api!(
            "GetCurrentTdpProfile",
            crate::hardware_detection::get_current_tdp_profile(),
            |i: &String| format!("profile=\"{}\"", i)
        )
    }

    async fn set_tdp_profile(&self, profile: &str) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetTdpProfile",
            crate::hardware_control::set_tdp_profile(profile),
            |_| format!("profile=\"{}\"", profile)
        )
    }

    async fn get_fan_speeds(&self) -> Result<String, zbus::fdo::Error> {
        let info: Vec<(u32, u32)> = cache_guard!().fan_info.iter()
            .map(|f| (f.id, f.rpm_or_percent))
            .collect();

        log_api_json!(
            "GetFanSpeeds",
            Ok::<_, anyhow::Error>(info),
            |i: &Vec<(u32, u32)>| format!("count={}", i.len())
        )
    }

    async fn get_fan_info(&self) -> Result<String, zbus::fdo::Error> {
        let info = cache_guard!().fan_info.clone();

        log_api_json!(
            "GetFanInfo",
            Ok::<_, anyhow::Error>(info),
            |i: &Vec<FanInfo>| format!("count={}", i.len())
        )
    }

    async fn get_fan_temperature(&self, fan_id: u32) -> Result<u32, zbus::fdo::Error> {
        log_api!(
            "GetFanTemperature",
            (|| {
                if !crate::tuxedo_io::TuxedoIo::is_available() {
                    return Err(anyhow::anyhow!("tuxedo_io not available"));
                }
                crate::tuxedo_io::TuxedoIo::shared()
                    .ok_or_else(|| anyhow::anyhow!("tuxedo_io not available"))?
                    .get_fan_temperature(fan_id)
            })(),
            |i: &u32| format!("id={} temp={}C", fan_id, i)
        )
    }
    
    async fn set_fan_speed(&self, fan_id: u32, speed: u32) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetFanSpeed",
            crate::hardware_control::set_fan_speed(fan_id, speed),
            |_| format!("id={} speed={}", fan_id, speed)
        )
    }
    
    async fn set_fan_auto(&self, fan_id: u32) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetFanAuto",
            crate::hardware_control::set_fan_auto(fan_id),
            |_| format!("id={}", fan_id)
        )
    }

    async fn set_all_fans_auto(&self) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetAllFansAuto",
            crate::hardware_control::set_fan_auto(0),
            |_| "".to_string()
        )
    }

    async fn set_gpu_fan_speed(&self, device_index: u32, fan_index: u32, speed: u32) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetGpuFanSpeed",
            crate::hardware_control::set_gpu_fan_speed(device_index, fan_index, speed),
            |_| format!("gpu={} fan={} speed={}", device_index, fan_index, speed)
        )
    }

    async fn set_gpu_fan_auto(&self, device_index: u32, fan_index: u32) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetGpuFanAuto",
            crate::hardware_control::set_gpu_fan_auto(device_index, fan_index),
            |_| format!("gpu={} fan={}", device_index, fan_index)
        )
    }
    
    async fn get_webcam_state(&self) -> Result<bool, zbus::fdo::Error> {
        log_api!(
            "GetWebcamState",
            crate::hardware_control::get_webcam_state(),
            |i: &bool| format!("enabled={}", i)
        )
    }
    
    async fn set_webcam_state(&self, enabled: bool) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetWebcamState",
            crate::hardware_control::set_webcam_state(enabled),
            |_| format!("enabled={}", enabled)
        )
    }

    async fn get_daemon_logs(&self) -> Result<String, zbus::fdo::Error> {
        // No explicit API logging for logs retrieval to avoid recursion and noise
        let logs = crate::hardware_control::lock_or_recover(&crate::DAEMON_LOGS, "DAEMON_LOGS");
        let logs_vec: Vec<LogEntry> = logs.iter().cloned().collect();
        serde_json::to_string(&logs_vec)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
    
    // Battery charge control methods
    async fn get_battery_charge_type(&self) -> Result<String, zbus::fdo::Error> {
        log_api!(
            "GetBatteryChargeType",
            (|| {
                crate::battery_control::BatteryControl::new()?.get_charge_type()
            })(),
            |i: &String| format!("type=\"{}\"", i)
        )
    }
    
    async fn set_battery_charge_type(&self, charge_type: &str) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetBatteryChargeType",
            (|| {
                crate::battery_control::BatteryControl::new()?.set_charge_type(charge_type)
            })(),
            |_| format!("type=\"{}\"", charge_type)
        )
    }
    
    async fn get_battery_charge_start_threshold(&self) -> Result<u8, zbus::fdo::Error> {
        log_api!(
            "GetBatteryChargeStartThreshold",
            (|| {
                crate::battery_control::BatteryControl::new()?.get_charge_control_start_threshold()
            })(),
            |i: &u8| format!("threshold={}", i)
        )
    }
    
    async fn set_battery_charge_start_threshold(&self, threshold: u8) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetBatteryChargeStartThreshold",
            (|| {
                crate::battery_control::BatteryControl::new()?.set_charge_control_start_threshold(threshold)
            })(),
            |_| format!("threshold={}", threshold)
        )
    }
    
    async fn get_battery_charge_end_threshold(&self) -> Result<u8, zbus::fdo::Error> {
        log_api!(
            "GetBatteryChargeEndThreshold",
            (|| {
                crate::battery_control::BatteryControl::new()?.get_charge_control_end_threshold()
            })(),
            |i: &u8| format!("threshold={}", i)
        )
    }
    
    async fn set_battery_charge_end_threshold(&self, threshold: u8) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetBatteryChargeEndThreshold",
            (|| {
                crate::battery_control::BatteryControl::new()?.set_charge_control_end_threshold(threshold)
            })(),
            |_| format!("threshold={}", threshold)
        )
    }
    
    async fn get_battery_available_start_thresholds(&self) -> Result<String, zbus::fdo::Error> {
        log_api_json!(
            "GetBatteryAvailableStartThresholds",
            (|| {
                crate::battery_control::BatteryControl::new()?.get_available_start_thresholds()
            })(),
            |i: &Vec<u8>| format!("count={}", i.len())
        )
    }
    
    async fn get_battery_available_end_thresholds(&self) -> Result<String, zbus::fdo::Error> {
        log_api_json!(
            "GetBatteryAvailableEndThresholds",
            (|| {
                crate::battery_control::BatteryControl::new()?.get_available_end_thresholds()
            })(),
            |i: &Vec<u8>| format!("count={}", i.len())
        )
    }
    
    async fn get_hardware_interface_info(&self) -> Result<String, zbus::fdo::Error> {
        log_api!(
            "GetHardwareInterfaceInfo",
            (|| -> Result<String> {
                if !crate::tuxedo_io::TuxedoIo::is_available() {
                    return Ok("None".to_string());
                }
                let io = crate::tuxedo_io::TuxedoIo::shared()
                    .ok_or_else(|| anyhow::anyhow!("tuxedo_io not available"))?;
                let interface = match io.get_interface() {
                    crate::tuxedo_io::HardwareInterface::Clevo => "Clevo",
                    crate::tuxedo_io::HardwareInterface::Uniwill => "Uniwill",
                    crate::tuxedo_io::HardwareInterface::None => "None",
                };
                let fan_count = io.get_fan_count();
                Ok(format!("Interface: {}, Fans: {}", interface, fan_count))
            })(),
            |i: &String| i.clone()
        )
    }

    async fn get_keyboard_capabilities(&self) -> Result<String, zbus::fdo::Error> {
        log_api_json!(
            "GetKeyboardCapabilities",
            Ok::<_, anyhow::Error>(crate::hardware_detection::get_keyboard_capabilities()),
            |i: &KeyboardCapabilities| format!("type={:?} zones={}", i.keyboard_type, i.num_zones)
        )
    }
    
    // Keyboard preview - apply keyboard settings immediately without saving to profile
    async fn preview_keyboard_settings(&self, settings_json: &str) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "PreviewKeyboardSettings",
            (|| {
                let settings: KeyboardSettings = serde_json::from_str(settings_json)?;
                crate::hardware_control::preview_keyboard_settings(&settings)
            })(),
            |_| "".to_string()
        )
    }

    async fn set_battery_settings(&self, settings_json: &str) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetBatterySettings",
            (|| {
                let settings: BatterySettings = serde_json::from_str(settings_json)?;
                crate::hardware_control::apply_battery_settings(&settings)
            })(),
            |_| "".to_string()
        )
    }

    async fn set_gpu_locked_clocks(&self, device_index: u32, min_clock: u32, max_clock: u32) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetGpuLockedClocks",
            crate::hardware_control::set_gpu_locked_clocks(device_index, min_clock, max_clock),
            |_| format!("gpu={} min={} max={}", device_index, min_clock, max_clock)
        )
    }

    async fn reset_gpu_clocks(&self, device_index: u32) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "ResetGpuClocks",
            crate::hardware_control::reset_gpu_clocks(device_index),
            |_| format!("gpu={}", device_index)
        )
    }

    async fn get_gpu_clock_ranges(&self, device_index: u32) -> Result<String, zbus::fdo::Error> {
        log_api_json!(
            "GetGpuClockRanges",
            crate::hardware_detection::get_gpu_clock_ranges(device_index),
            |i: &(u32, u32)| format!("gpu={} range={}..{}", device_index, i.0, i.1)
        )
    }

    async fn set_gpu_core_offset(&self, device_index: u32, offset: f32) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetGpuCoreOffset",
            crate::hardware_control::set_gpu_core_offset(device_index, offset),
            |_| format!("gpu={} offset={}", device_index, offset)
        )
    }

    async fn set_gpu_memory_offset(&self, device_index: u32, offset: f32) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetGpuMemoryOffset",
            crate::hardware_control::set_gpu_memory_offset(device_index, offset),
            |_| format!("gpu={} offset={}", device_index, offset)
        )
    }

    async fn set_gpu_power_limit(&self, device_index: u32, limit_watts: u32) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetGpuPowerLimit",
            crate::hardware_control::set_gpu_power_limit(device_index, limit_watts),
            |_| format!("gpu={} limit={}W", device_index, limit_watts)
        )
    }

    async fn get_gpu_core_offset_limits(&self, device_index: u32) -> Result<(i32, i32), zbus::fdo::Error> {
        log_api!(
            "GetGpuCoreOffsetLimits",
            crate::hardware_detection::get_gpu_core_offset_limits(device_index),
            |i: &(i32, i32)| format!("gpu={} limits={}..{}", device_index, i.0, i.1)
        )
    }

    async fn get_gpu_memory_offset_limits(&self, device_index: u32) -> Result<(i32, i32), zbus::fdo::Error> {
        log_api!(
            "GetGpuMemoryOffsetLimits",
            crate::hardware_detection::get_gpu_memory_offset_limits(device_index),
            |i: &(i32, i32)| format!("gpu={} limits={}..{}", device_index, i.0, i.1)
        )
    }

    async fn set_prime_profile(&self, profile: &str) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SetPrimeProfile",
            crate::hardware_control::set_prime_profile(profile),
            |_| format!("profile=\"{}\"", profile)
        )
    }

    // Polling scheduler methods
    async fn update_polling_interval(&self, component: &str, interval_ms: u64) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "UpdatePollingInterval",
            (|| {
                if let Some(handle) = crate::SCHEDULER_HANDLE.get() {
                    let interval = std::time::Duration::from_millis(interval_ms);
                    handle.update_interval(component.to_string(), interval)
                        .map_err(|e| anyhow::anyhow!(e))
                } else {
                    Err(anyhow::anyhow!("Scheduler not initialized"))
                }
            })(),
            |_| format!("component=\"{}\" interval_ms={}", component, interval_ms)
        )
    }

    /// Sync daemon poll-job intervals from the GUI's statistics_sections JSON.
    ///
    /// Called by the GUI after it saves settings.json so interval changes take
    /// effect immediately without a daemon restart. Only changed jobs are
    /// rescheduled; the RTD3 hybrid NVML gating in hardware_detection is
    /// interval-independent and stays fully active at any rate.
    async fn sync_daemon_poll_settings(&self, settings_json: &str) -> Result<(), zbus::fdo::Error> {
        log_api!(
            "SyncDaemonPollSettings",
            (|| {
                match crate::daemon_settings::from_statistics_json(settings_json) {
                    Ok(next) => {
                        let applied = crate::daemon_settings::sync_from(
                            &crate::DAEMON_POLL_SETTINGS,
                            crate::SCHEDULER_HANDLE.get(),
                            next,
                        );
                        if applied.is_empty() {
                            log::debug!("SyncDaemonPollSettings: no interval changes");
                        }
                        Ok(())
                    }
                    Err(e) => Err(anyhow::anyhow!(
                        "invalid statistics_sections payload: {e}"
                    )),
                }
            })(),
            |_| format!("settings_json_len={}", settings_json.len())
        )
    }

    async fn shutdown_daemon(&self) -> Result<(), zbus::fdo::Error> {
        if is_systemd_managed() {
            return Ok(());
        }

        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(SHUTDOWN_SIGNAL_DELAY_MS));
            if let Err(err) = raise(Signal::SIGINT) {
                log::error!("Failed to signal daemon shutdown: {}", err);
            }
        });

        Ok(())
    }
}

fn is_systemd_managed() -> bool {
    std::env::var_os("INVOCATION_ID").is_some()
        || std::env::var_os("SYSTEMD_EXEC_PID").is_some()
}

pub async fn start_service(connection: Connection) -> Result<()> {
    connection
        .object_server()
        .at("/io/lapsphere/Control", ControlInterface)
        .await?;

    connection.request_name("io.lapsphere.Control").await?;
    
    Ok(())
}

#[cfg(test)]
mod inflight_guard_tests {
    use super::GPU_FULL_REFRESH_INFLIGHT;
    use std::sync::atomic::Ordering::{AcqRel, Acquire, Release};

    /// The claim must be exclusive: N threads racing, exactly one wins.
    ///
    /// This is the property that bounds thread (and therefore glibc arena)
    /// creation. If two callers could both win, each would spawn its own
    /// blocking task and the guard would be decorative.
    #[test]
    fn claim_is_exclusive_under_contention() {
        const THREADS: usize = 16;
        const ROUNDS: usize = 200;

        for _ in 0..ROUNDS {
            // store() only accepts Release/Relaxed/SeqCst — AcqRel is
            // read-modify-write ordering and is rejected on a plain store.
            GPU_FULL_REFRESH_INFLIGHT.store(false, Release);

            let winners = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let mut handles = Vec::with_capacity(THREADS);

            for _ in 0..THREADS {
                let w = winners.clone();
                handles.push(std::thread::spawn(move || {
                    // Mirrors the real claim: swap() returns the PREVIOUS
                    // value, so exactly one caller sees false and proceeds.
                    let previous = GPU_FULL_REFRESH_INFLIGHT.swap(true, AcqRel);
                    if !previous {
                        w.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                }));
            }
            for h in handles {
                h.join().expect("claim thread panicked");
            }

            assert_eq!(
                winners.load(std::sync::atomic::Ordering::Relaxed),
                1,
                "exactly one caller may claim the refresh slot"
            );
        }
    }

    /// A coalescing caller must observe the slot as taken (so it routes to the
    /// wait path) and must observe it clear again once the owner releases.
    #[test]
    fn slot_releases_so_later_callers_are_not_wedged() {
        GPU_FULL_REFRESH_INFLIGHT.store(true, Release);
        assert!(
            GPU_FULL_REFRESH_INFLIGHT.load(Acquire),
            "coalescing fast-path depends on observing the slot as taken"
        );

        // The owner releases on every exit path; if any path skipped this the
        // daemon would serve nothing but stale payloads forever.
        GPU_FULL_REFRESH_INFLIGHT.store(false, Release);
        assert!(
            !GPU_FULL_REFRESH_INFLIGHT.load(Acquire),
            "a released slot must let the next caller do real work"
        );
    }
}
