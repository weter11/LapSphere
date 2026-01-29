use anyhow::Result;
use lapsphere_common::types::*;
use zbus::Connection;
use tokio::sync::{mpsc, oneshot};

#[derive(Clone)]
pub struct DbusClient {
    command_tx: mpsc::UnboundedSender<DbusCommand>,
}

// Commands sent from UI to background task
pub enum DbusCommand {
    GetSystemInfo { reply: oneshot::Sender<Result<SystemInfo>> },
    GetMemoryInfo { reply: oneshot::Sender<Result<MemoryInfo>> },
    GetCpuInfo { reply: oneshot::Sender<Result<CpuInfo>> },
    GetGpuInfo { reply: oneshot::Sender<Result<Vec<GpuInfo>>> },
    GetFanInfo { reply: oneshot::Sender<Result<Vec<FanInfo>>> },
    GetBatteryInfo { reply: oneshot::Sender<Result<BatteryInfo>> },
    GetStorageDeviceInfo { reply: oneshot::Sender<Result<Vec<StorageDevice>>> },
    GetMountInfo { reply: oneshot::Sender<Result<Vec<MountInfo>>> },
    GetWifiInfo { reply: oneshot::Sender<Result<Vec<WiFiInfo>>> },
    GetTdpProfiles { reply: oneshot::Sender<Result<Vec<String>>> },
    GetHardwareInterfaceInfo { reply: oneshot::Sender<Result<String>> },
    GetKeyboardCapabilities { reply: oneshot::Sender<Result<KeyboardCapabilities>> },
    ApplyProfile { profile: Profile, reply: oneshot::Sender<Result<()>> },
    SetAmdPstateStatus { status: String, reply: oneshot::Sender<Result<()>> },
    SetIntelPstateStatus { status: String, reply: oneshot::Sender<Result<()>> },
    PreviewKeyboard { settings: KeyboardSettings, reply: oneshot::Sender<Result<()>> },
    GetBatteryAvailableStartThresholds { reply: oneshot::Sender<Result<Vec<u8>>> },
    GetBatteryAvailableEndThresholds { reply: oneshot::Sender<Result<Vec<u8>>> },
    SetBatterySettings { settings: BatterySettings, reply: oneshot::Sender<Result<()>> },
    SetAllFansAuto { reply: oneshot::Sender<Result<()>> },
    SetGpuLockedClocks { device_index: u32, min_clock: u32, max_clock: u32, reply: oneshot::Sender<Result<()>> },
    ResetGpuClocks { device_index: u32, reply: oneshot::Sender<Result<()>> },
    GetGpuClockRanges { device_index: u32, reply: oneshot::Sender<Result<(u32, u32)>> },
    GetGpuMemClockRanges { device_index: u32, reply: oneshot::Sender<Result<Vec<u32>>> },
    SetMemoryLockedClocks { device_index: u32, min_clock: u32, max_clock: u32, reply: oneshot::Sender<Result<()>> },
    ResetMemoryLockedClocks { device_index: u32, reply: oneshot::Sender<Result<()>> },
    SetGpuCoreOffset { device_index: u32, offset: f32, reply: oneshot::Sender<Result<()>> },
    SetGpuMemoryOffset { device_index: u32, offset: f32, reply: oneshot::Sender<Result<()>> },
    GetGpuCoreOffsetLimits { device_index: u32, reply: oneshot::Sender<Result<(i32, i32)>> },
    GetGpuMemoryOffsetLimits { device_index: u32, reply: oneshot::Sender<Result<(i32, i32)>> },
    SetPrimeProfile { profile: String, reply: oneshot::Sender<Result<()>> },
    UpdatePollingInterval { component: String, interval_ms: u64, reply: oneshot::Sender<Result<()>> },
    GetWebcamState { reply: oneshot::Sender<Result<bool>> },
    SetWebcamState { enabled: bool, reply: oneshot::Sender<Result<()>> },
    GetDaemonLogs { reply: oneshot::Sender<Result<Vec<LogEntry>>> },
    ShutdownDaemon { reply: oneshot::Sender<Result<()>> },
    SetGpuFanSpeed { device_index: u32, fan_index: u32, speed: u32, reply: oneshot::Sender<Result<()>> },
    SetGpuFanAuto { device_index: u32, fan_index: u32, reply: oneshot::Sender<Result<()>> },
}

impl DbusClient {
    pub fn new() -> Result<Self> {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        
        // Spawn background task that handles all DBus calls
        tokio::spawn(async move {
            if let Err(e) = dbus_worker(command_rx).await {
                log::error!("DBus worker died: {}", e);
            }
        });
        
        Ok(Self { command_tx })
    }
    
    // Non-blocking methods - return immediately with oneshot receiver
    
    pub fn get_cpu_info(&self) -> oneshot::Receiver<Result<CpuInfo>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::GetCpuInfo { reply: tx });
        rx
    }
    
    pub fn get_system_info(&self) -> oneshot::Receiver<Result<SystemInfo>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::GetSystemInfo { reply: tx });
        rx
    }

    pub fn get_memory_info(&self) -> oneshot::Receiver<Result<MemoryInfo>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::GetMemoryInfo { reply: tx });
        rx
    }
    
    pub fn get_gpu_info(&self) -> oneshot::Receiver<Result<Vec<GpuInfo>>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::GetGpuInfo { reply: tx });
        rx
    }
    
    pub fn get_fan_info(&self) -> oneshot::Receiver<Result<Vec<FanInfo>>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::GetFanInfo { reply: tx });
        rx
    }

    pub fn get_battery_info(&self) -> oneshot::Receiver<Result<BatteryInfo>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::GetBatteryInfo { reply: tx });
        rx
    }

    pub fn get_storage_device_info(&self) -> oneshot::Receiver<Result<Vec<StorageDevice>>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::GetStorageDeviceInfo { reply: tx });
        rx
    }

    pub fn get_mount_info(&self) -> oneshot::Receiver<Result<Vec<MountInfo>>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::GetMountInfo { reply: tx });
        rx
    }

    pub fn get_wifi_info(&self) -> oneshot::Receiver<Result<Vec<WiFiInfo>>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::GetWifiInfo { reply: tx });
        rx
    }

    pub fn get_tdp_profiles(&self) -> oneshot::Receiver<Result<Vec<String>>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::GetTdpProfiles { reply: tx });
        rx
    }

    pub fn get_hardware_interface_info(&self) -> oneshot::Receiver<Result<String>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::GetHardwareInterfaceInfo { reply: tx });
        rx
    }

    pub fn get_keyboard_capabilities(&self) -> oneshot::Receiver<Result<KeyboardCapabilities>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::GetKeyboardCapabilities { reply: tx });
        rx
    }
    
    pub fn apply_profile(&self, profile: Profile) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::ApplyProfile { 
            profile: profile.clone(), 
            reply: tx 
        });
        rx
    }
    
    pub fn set_amd_pstate_status(&self, status: String) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::SetAmdPstateStatus { status, reply: tx });
        rx
    }

    pub fn set_intel_pstate_status(&self, status: String) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::SetIntelPstateStatus { status, reply: tx });
        rx
    }

    pub fn preview_keyboard_settings(&self, settings: KeyboardSettings) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::PreviewKeyboard { 
            settings: settings.clone(), 
            reply: tx 
        });
        rx
    }
    
    pub fn get_battery_available_start_thresholds(&self) -> oneshot::Receiver<Result<Vec<u8>>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::GetBatteryAvailableStartThresholds { reply: tx });
        rx
    }

    pub fn get_battery_available_end_thresholds(&self) -> oneshot::Receiver<Result<Vec<u8>>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::GetBatteryAvailableEndThresholds { reply: tx });
        rx
    }

    pub fn set_battery_settings(&self, settings: BatterySettings) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::SetBatterySettings { settings, reply: tx });
        rx
    }

    pub fn set_all_fans_auto(&self) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::SetAllFansAuto { reply: tx });
        rx
    }

    pub fn set_gpu_locked_clocks(&self, device_index: u32, min_clock: u32, max_clock: u32) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::SetGpuLockedClocks { device_index, min_clock, max_clock, reply: tx });
        rx
    }

    pub fn reset_gpu_clocks(&self, device_index: u32) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::ResetGpuClocks { device_index, reply: tx });
        rx
    }

    pub fn get_gpu_clock_ranges(&self, device_index: u32) -> oneshot::Receiver<Result<(u32, u32)>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::GetGpuClockRanges { device_index, reply: tx });
        rx
    }

    pub fn get_gpu_mem_clock_ranges(&self, device_index: u32) -> oneshot::Receiver<Result<Vec<u32>>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::GetGpuMemClockRanges { device_index, reply: tx });
        rx
    }

    pub fn set_memory_locked_clocks(&self, device_index: u32, min_clock: u32, max_clock: u32) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::SetMemoryLockedClocks { device_index, min_clock, max_clock, reply: tx });
        rx
    }

    pub fn reset_memory_locked_clocks(&self, device_index: u32) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::ResetMemoryLockedClocks { device_index, reply: tx });
        rx
    }

    pub fn set_gpu_core_offset(&self, device_index: u32, offset: f32) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::SetGpuCoreOffset { device_index, offset, reply: tx });
        rx
    }

    pub fn set_gpu_memory_offset(&self, device_index: u32, offset: f32) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::SetGpuMemoryOffset { device_index, offset, reply: tx });
        rx
    }

    pub fn get_gpu_core_offset_limits(&self, device_index: u32) -> oneshot::Receiver<Result<(i32, i32)>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::GetGpuCoreOffsetLimits { device_index, reply: tx });
        rx
    }

    pub fn get_gpu_memory_offset_limits(&self, device_index: u32) -> oneshot::Receiver<Result<(i32, i32)>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::GetGpuMemoryOffsetLimits { device_index, reply: tx });
        rx
    }

    pub fn set_prime_profile(&self, profile: &str) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::SetPrimeProfile { profile: profile.to_string(), reply: tx });
        rx
    }

    pub fn update_polling_interval(&self, component: &str, interval_ms: u64) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::UpdatePollingInterval {
            component: component.to_string(),
            interval_ms,
            reply: tx
        });
        rx
    }

    pub fn shutdown_daemon(&self) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::ShutdownDaemon { reply: tx });
        rx
    }

    pub fn get_webcam_state(&self) -> oneshot::Receiver<Result<bool>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::GetWebcamState { reply: tx });
        rx
    }

    pub fn set_webcam_state(&self, enabled: bool) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::SetWebcamState { enabled, reply: tx });
        rx
    }

    pub fn get_daemon_logs(&self) -> oneshot::Receiver<Result<Vec<LogEntry>>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::GetDaemonLogs { reply: tx });
        rx
    }

    pub fn set_gpu_fan_speed(&self, device_index: u32, fan_index: u32, speed: u32) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::SetGpuFanSpeed { device_index, fan_index, speed, reply: tx });
        rx
    }

    pub fn set_gpu_fan_auto(&self, device_index: u32, fan_index: u32) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::SetGpuFanAuto { device_index, fan_index, reply: tx });
        rx
    }
}

// Background worker - handles all DBus calls asynchronously with reconnection logic
async fn dbus_worker(mut command_rx: mpsc::UnboundedReceiver<DbusCommand>) -> Result<()> {
    loop {
        let connection = match Connection::system().await {
            Ok(conn) => {
                log::info!("Successfully connected to system bus");
                conn
            }
            Err(e) => {
                log::error!("Failed to connect to system bus: {}. Retrying in 2 seconds...", e);
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            }
        };

        while let Some(command) = command_rx.recv().await {
            let res: Result<(), anyhow::Error> = match command {
                DbusCommand::GetSystemInfo { reply } => {
                    let result = get_system_info_impl(&connection).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::GetMemoryInfo { reply } => {
                    let result = get_memory_info_impl(&connection).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::GetCpuInfo { reply } => {
                    let result = get_cpu_info_impl(&connection).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::GetGpuInfo { reply } => {
                    let result = get_gpu_info_impl(&connection).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::GetFanInfo { reply } => {
                    let result = get_fan_info_impl(&connection).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::GetBatteryInfo { reply } => {
                    let result = get_battery_info_impl(&connection).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::GetStorageDeviceInfo { reply } => {
                    let result = get_storage_device_info_impl(&connection).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::GetMountInfo { reply } => {
                    let result = get_mount_info_impl(&connection).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::GetWifiInfo { reply } => {
                    let result = get_wifi_info_impl(&connection).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::GetTdpProfiles { reply } => {
                    let result = get_tdp_profiles_impl(&connection).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::GetHardwareInterfaceInfo { reply } => {
                    let result = get_hardware_interface_info_impl(&connection).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::GetKeyboardCapabilities { reply } => {
                    let result = get_keyboard_capabilities_impl(&connection).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::ApplyProfile { profile, reply } => {
                    let result = apply_profile_impl(&connection, &profile).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::SetAmdPstateStatus { status, reply } => {
                    let result = set_amd_pstate_status_impl(&connection, &status).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::SetIntelPstateStatus { status, reply } => {
                    let result = set_intel_pstate_status_impl(&connection, &status).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::PreviewKeyboard { settings, reply } => {
                    let result = preview_keyboard_impl(&connection, &settings).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::GetBatteryAvailableStartThresholds { reply } => {
                    let result = get_battery_available_start_thresholds_impl(&connection).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::GetBatteryAvailableEndThresholds { reply } => {
                    let result = get_battery_available_end_thresholds_impl(&connection).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::SetBatterySettings { settings, reply } => {
                    let result = set_battery_settings_impl(&connection, settings).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::SetAllFansAuto { reply } => {
                    let result = set_all_fans_auto_impl(&connection).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::SetGpuLockedClocks { device_index, min_clock, max_clock, reply } => {
                    let result = set_gpu_locked_clocks_impl(&connection, device_index, min_clock, max_clock).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::ResetGpuClocks { device_index, reply } => {
                    let result = reset_gpu_clocks_impl(&connection, device_index).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::GetGpuClockRanges { device_index, reply } => {
                    let result = get_gpu_clock_ranges_impl(&connection, device_index).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::GetGpuMemClockRanges { device_index, reply } => {
                    let result = get_gpu_mem_clock_ranges_impl(&connection, device_index).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::SetMemoryLockedClocks { device_index, min_clock, max_clock, reply } => {
                    let result = set_memory_locked_clocks_impl(&connection, device_index, min_clock, max_clock).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::ResetMemoryLockedClocks { device_index, reply } => {
                    let result = reset_memory_locked_clocks_impl(&connection, device_index).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::SetGpuCoreOffset { device_index, offset, reply } => {
                    let result = set_gpu_core_offset_impl(&connection, device_index, offset).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::SetGpuMemoryOffset { device_index, offset, reply } => {
                    let result = set_gpu_memory_offset_impl(&connection, device_index, offset).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::GetGpuCoreOffsetLimits { device_index, reply } => {
                    let result = get_gpu_core_offset_limits_impl(&connection, device_index).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::GetGpuMemoryOffsetLimits { device_index, reply } => {
                    let result = get_gpu_memory_offset_limits_impl(&connection, device_index).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::SetPrimeProfile { profile, reply } => {
                    let result = set_prime_profile_impl(&connection, &profile).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::UpdatePollingInterval { component, interval_ms, reply } => {
                    let result = update_polling_interval_impl(&connection, &component, interval_ms).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::ShutdownDaemon { reply } => {
                    let result = shutdown_daemon_impl(&connection).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::GetWebcamState { reply } => {
                    let result = get_webcam_state_impl(&connection).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::SetWebcamState { enabled, reply } => {
                    let result = set_webcam_state_impl(&connection, enabled).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::GetDaemonLogs { reply } => {
                    let result = get_daemon_logs_impl(&connection).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::SetGpuFanSpeed { device_index, fan_index, speed, reply } => {
                    let result = set_gpu_fan_speed_impl(&connection, device_index, fan_index, speed).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
                DbusCommand::SetGpuFanAuto { device_index, fan_index, reply } => {
                    let result = set_gpu_fan_auto_impl(&connection, device_index, fan_index).await;
                    let is_err = result.is_err();
                    let _ = reply.send(result);
                    if is_err { Err(anyhow::anyhow!("DBus call failed")) } else { Ok(()) }
                }
            };

            if let Err(e) = res {
                let err_str = e.to_string();
                log::error!("DBus call failed: {}", err_str);

                // Only reconnect on connection-level errors, not logical/method errors
                // Method errors usually contain the interface name or "MethodError"
                let is_connection_error = err_str.contains("connection")
                    || err_str.contains("Closed")
                    || err_str.contains("Broken pipe")
                    || err_str.contains("io.lapsphere.Control not found");

                if is_connection_error {
                    log::warn!("DBus connection lost. Attempting to reconnect...");
                    break;
                }
            }
        }

        // If command_rx is closed, exit the worker
        if command_rx.is_closed() {
            break;
        }
    }
    
    Ok(())
}

// Implementation functions
async fn get_system_info_impl(conn: &Connection) -> Result<SystemInfo> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    
    let json: String = proxy.call("GetSystemInfo", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_memory_info_impl(conn: &Connection) -> Result<MemoryInfo> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;

    let json: String = proxy.call("GetMemoryInfo", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_cpu_info_impl(conn: &Connection) -> Result<CpuInfo> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    
    let json: String = proxy.call("GetCpuInfo", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_gpu_info_impl(conn: &Connection) -> Result<Vec<GpuInfo>> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    
    let json: String = proxy.call("GetGpuInfo", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_fan_info_impl(conn: &Connection) -> Result<Vec<FanInfo>> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    
    let json: String = proxy.call("GetFanInfo", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_battery_info_impl(conn: &Connection) -> Result<BatteryInfo> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;

    let json: String = proxy.call("GetBatteryInfo", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_storage_device_info_impl(conn: &Connection) -> Result<Vec<StorageDevice>> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;

    let json: String = proxy.call("GetStorageDeviceInfo", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_mount_info_impl(conn: &Connection) -> Result<Vec<MountInfo>> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;

    let json: String = proxy.call("GetMountInfo", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_wifi_info_impl(conn: &Connection) -> Result<Vec<WiFiInfo>> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;

    let json: String = proxy.call("GetWifiInfo", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_tdp_profiles_impl(conn: &Connection) -> Result<Vec<String>> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;

    let json: String = proxy.call("GetTdpProfiles", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_hardware_interface_info_impl(conn: &Connection) -> Result<String> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;

    let info: String = proxy.call("GetHardwareInterfaceInfo", &()).await?;
    Ok(info)
}

async fn get_keyboard_capabilities_impl(conn: &Connection) -> Result<KeyboardCapabilities> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;

    let json: String = proxy.call("GetKeyboardCapabilities", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn apply_profile_impl(conn: &Connection, profile: &Profile) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    
    let json = serde_json::to_string(profile)?;
    proxy.call::<_, _, ()>("ApplyProfile", &(json.as_str(),)).await?;
    Ok(())
}

async fn preview_keyboard_impl(conn: &Connection, settings: &KeyboardSettings) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    
    let json = serde_json::to_string(settings)?;
    proxy.call::<_, _, ()>("PreviewKeyboardSettings", &(json.as_str(),)).await?;
    Ok(())
}

async fn set_amd_pstate_status_impl(conn: &Connection, status: &str) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;

    proxy.call::<_, _, ()>("SetAmdPstateStatus", &(status,)).await?;
    Ok(())
}

async fn set_intel_pstate_status_impl(conn: &Connection, status: &str) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;

    proxy.call::<_, _, ()>("SetIntelPstateStatus", &(status,)).await?;
    Ok(())
}

async fn get_battery_available_start_thresholds_impl(conn: &Connection) -> Result<Vec<u8>> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;

    let json: String = proxy.call("GetBatteryAvailableStartThresholds", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_battery_available_end_thresholds_impl(conn: &Connection) -> Result<Vec<u8>> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;

    let json: String = proxy.call("GetBatteryAvailableEndThresholds", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn set_battery_settings_impl(conn: &Connection, settings: BatterySettings) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;

    let json = serde_json::to_string(&settings)?;
    proxy.call::<_, _, ()>("SetBatterySettings", &(json.as_str(),)).await?;
    Ok(())
}

async fn set_all_fans_auto_impl(conn: &Connection) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    proxy.call::<_, _, ()>("SetAllFansAuto", &()).await?;
    Ok(())
}

async fn set_gpu_locked_clocks_impl(conn: &Connection, device_index: u32, min_clock: u32, max_clock: u32) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    proxy.call::<_, _, ()>("SetGpuLockedClocks", &(device_index, min_clock, max_clock)).await?;
    Ok(())
}

async fn reset_gpu_clocks_impl(conn: &Connection, device_index: u32) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    proxy.call::<_, _, ()>("ResetGpuClocks", &(device_index,)).await?;
    Ok(())
}

async fn get_gpu_clock_ranges_impl(conn: &Connection, device_index: u32) -> Result<(u32, u32)> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    let json: String = proxy.call("GetGpuClockRanges", &(device_index,)).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_gpu_mem_clock_ranges_impl(conn: &Connection, device_index: u32) -> Result<Vec<u32>> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    let json: String = proxy.call("GetGpuMemClockRanges", &(device_index,)).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn set_memory_locked_clocks_impl(conn: &Connection, device_index: u32, min_clock: u32, max_clock: u32) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    proxy.call::<_, _, ()>("SetMemoryLockedClocks", &(device_index, min_clock, max_clock)).await?;
    Ok(())
}

async fn reset_memory_locked_clocks_impl(conn: &Connection, device_index: u32) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    proxy.call::<_, _, ()>("ResetMemoryLockedClocks", &(device_index,)).await?;
    Ok(())
}

async fn set_gpu_core_offset_impl(conn: &Connection, device_index: u32, offset: f32) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    proxy.call::<_, _, ()>("SetGpuCoreOffset", &(device_index, offset)).await?;
    Ok(())
}

async fn set_gpu_memory_offset_impl(conn: &Connection, device_index: u32, offset: f32) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    proxy.call::<_, _, ()>("SetGpuMemoryOffset", &(device_index, offset)).await?;
    Ok(())
}

async fn get_gpu_core_offset_limits_impl(conn: &Connection, device_index: u32) -> Result<(i32, i32)> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    let limits: (i32, i32) = proxy.call("GetGpuCoreOffsetLimits", &(device_index,)).await?;
    Ok(limits)
}

async fn get_gpu_memory_offset_limits_impl(conn: &Connection, device_index: u32) -> Result<(i32, i32)> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    let limits: (i32, i32) = proxy.call("GetGpuMemoryOffsetLimits", &(device_index,)).await?;
    Ok(limits)
}

async fn set_prime_profile_impl(conn: &Connection, profile: &str) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    proxy.call::<_, _, ()>("SetPrimeProfile", &(profile,)).await?;
    Ok(())
}

async fn update_polling_interval_impl(conn: &Connection, component: &str, interval_ms: u64) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    proxy.call::<_, _, ()>("UpdatePollingInterval", &(component, interval_ms)).await?;
    Ok(())
}

async fn get_webcam_state_impl(conn: &Connection) -> Result<bool> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    let state: bool = proxy.call("GetWebcamState", &()).await?;
    Ok(state)
}

async fn set_webcam_state_impl(conn: &Connection, enabled: bool) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    proxy.call::<_, _, ()>("SetWebcamState", &(enabled,)).await?;
    Ok(())
}

async fn get_daemon_logs_impl(conn: &Connection) -> Result<Vec<LogEntry>> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    let json: String = proxy.call("GetDaemonLogs", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn shutdown_daemon_impl(conn: &Connection) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    proxy.call::<_, _, ()>("ShutdownDaemon", &()).await?;
    Ok(())
}

async fn set_gpu_fan_speed_impl(conn: &Connection, device_index: u32, fan_index: u32, speed: u32) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    proxy.call::<_, _, ()>("SetGpuFanSpeed", &(device_index, fan_index, speed)).await?;
    Ok(())
}

async fn set_gpu_fan_auto_impl(conn: &Connection, device_index: u32, fan_index: u32) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "io.lapsphere.Control",
        "/io/lapsphere/Control",
        "io.lapsphere.Control",
    ).await?;
    proxy.call::<_, _, ()>("SetGpuFanAuto", &(device_index, fan_index)).await?;
    Ok(())
}
