use anyhow::Result;
use tuxedo_common::types::*;
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
    ApplyProfile { profile: Profile, reply: oneshot::Sender<Result<()>> },
    SetCpuGovernor { governor: String, reply: oneshot::Sender<Result<()>> },
    SetCpuBoost { enabled: bool, reply: oneshot::Sender<Result<()>> },
    SetAmdPstateStatus { status: String, reply: oneshot::Sender<Result<()>> },
    PreviewKeyboard { settings: KeyboardSettings, reply: oneshot::Sender<Result<()>> },
    GetBatteryChargeThresholds { reply: oneshot::Sender<Result<(u8, u8)>> },
    SetBatteryChargeThresholds { start: u8, end: u8, reply: oneshot::Sender<Result<()>> },
    GetBatteryAvailableStartThresholds { reply: oneshot::Sender<Result<Vec<u8>>> },
    GetBatteryAvailableEndThresholds { reply: oneshot::Sender<Result<Vec<u8>>> },
    SetBatterySettings { settings: BatterySettings, reply: oneshot::Sender<Result<()>> },
    SetFanAuto { fan_id: u32, reply: oneshot::Sender<Result<()>> },
    SetGpuLockedClocks { device_index: u32, min_clock: u32, max_clock: u32, reply: oneshot::Sender<Result<()>> },
    ResetGpuClocks { device_index: u32, reply: oneshot::Sender<Result<()>> },
    GetGpuClockRanges { device_index: u32, reply: oneshot::Sender<Result<(u32, u32)>> },
    GetGpuMemClockRanges { device_index: u32, reply: oneshot::Sender<Result<Vec<u32>>> },
    SetMemoryLockedClocks { device_index: u32, min_clock: u32, max_clock: u32, reply: oneshot::Sender<Result<()>> },
    ResetMemoryLockedClocks { device_index: u32, reply: oneshot::Sender<Result<()>> },
    SetGpuCoreOffset { device_index: u32, offset: i32, reply: oneshot::Sender<Result<()>> },
    SetGpuMemoryOffset { device_index: u32, offset: i32, reply: oneshot::Sender<Result<()>> },
    GetGpuCoreOffsetLimits { device_index: u32, reply: oneshot::Sender<Result<(i32, i32)>> },
    GetGpuMemoryOffsetLimits { device_index: u32, reply: oneshot::Sender<Result<(i32, i32)>> },
    SetPrimeProfile { profile: String, reply: oneshot::Sender<Result<()>> },
    ShutdownDaemon { reply: oneshot::Sender<Result<()>> },
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
    
    pub fn apply_profile(&self, profile: Profile) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::ApplyProfile { 
            profile: profile.clone(), 
            reply: tx 
        });
        rx
    }
    
    pub fn set_cpu_governor(&self, governor: String) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::SetCpuGovernor { governor, reply: tx });
        rx
    }

    pub fn set_amd_pstate_status(&self, status: String) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::SetAmdPstateStatus { status, reply: tx });
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
    
    pub fn get_battery_charge_thresholds(&self) -> oneshot::Receiver<Result<(u8, u8)>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::GetBatteryChargeThresholds { reply: tx });
        rx
    }
    
    pub fn set_battery_charge_thresholds(&self, start: u8, end: u8) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::SetBatteryChargeThresholds { 
            start, end, reply: tx 
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

    pub fn set_fan_auto(&self, fan_id: u32) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::SetFanAuto { fan_id, reply: tx });
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

    pub fn set_gpu_core_offset(&self, device_index: u32, offset: i32) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::SetGpuCoreOffset { device_index, offset, reply: tx });
        rx
    }

    pub fn set_gpu_memory_offset(&self, device_index: u32, offset: i32) -> oneshot::Receiver<Result<()>> {
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

    pub fn shutdown_daemon(&self) -> oneshot::Receiver<Result<()>> {
        let (tx, rx) = oneshot::channel();
        let _ = self.command_tx.send(DbusCommand::ShutdownDaemon { reply: tx });
        rx
    }
}

// Background worker - handles all DBus calls asynchronously
async fn dbus_worker(mut command_rx: mpsc::UnboundedReceiver<DbusCommand>) -> Result<()> {
    let connection = Connection::system().await?;
    
    while let Some(command) = command_rx.recv().await {
        match command {
            DbusCommand::GetSystemInfo { reply } => {
                let result = get_system_info_impl(&connection).await;
                let _ = reply.send(result);
            }
            DbusCommand::GetMemoryInfo { reply } => {
                let result = get_memory_info_impl(&connection).await;
                let _ = reply.send(result);
            }
            DbusCommand::GetCpuInfo { reply } => {
                let result = get_cpu_info_impl(&connection).await;
                let _ = reply.send(result);
            }
            DbusCommand::GetGpuInfo { reply } => {
                let result = get_gpu_info_impl(&connection).await;
                let _ = reply.send(result);
            }
            DbusCommand::GetFanInfo { reply } => {
                let result = get_fan_info_impl(&connection).await;
                let _ = reply.send(result);
            }
            DbusCommand::GetBatteryInfo { reply } => {
                let result = get_battery_info_impl(&connection).await;
                let _ = reply.send(result);
            }
            DbusCommand::GetStorageDeviceInfo { reply } => {
                let result = get_storage_device_info_impl(&connection).await;
                let _ = reply.send(result);
            }
            DbusCommand::GetMountInfo { reply } => {
                let result = get_mount_info_impl(&connection).await;
                let _ = reply.send(result);
            }
            DbusCommand::GetWifiInfo { reply } => {
                let result = get_wifi_info_impl(&connection).await;
                let _ = reply.send(result);
            }
            DbusCommand::GetTdpProfiles { reply } => {
                let result = get_tdp_profiles_impl(&connection).await;
                let _ = reply.send(result);
            }
            DbusCommand::ApplyProfile { profile, reply } => {
                let result = apply_profile_impl(&connection, &profile).await;
                let _ = reply.send(result);
            }
            DbusCommand::SetCpuGovernor { governor, reply } => {
                let result = set_cpu_governor_impl(&connection, &governor).await;
                let _ = reply.send(result);
            }
            DbusCommand::SetCpuBoost { enabled, reply } => {
                let result = set_cpu_boost_impl(&connection, enabled).await;
                let _ = reply.send(result);
            }
            DbusCommand::SetAmdPstateStatus { status, reply } => {
                let result = set_amd_pstate_status_impl(&connection, &status).await;
                let _ = reply.send(result);
            }
            DbusCommand::PreviewKeyboard { settings, reply } => {
                let result = preview_keyboard_impl(&connection, &settings).await;
                let _ = reply.send(result);
            }
            DbusCommand::GetBatteryChargeThresholds { reply } => {
                let result = get_battery_thresholds_impl(&connection).await;
                let _ = reply.send(result);
            }
            DbusCommand::SetBatteryChargeThresholds { start, end, reply } => {
                let result = set_battery_thresholds_impl(&connection, start, end).await;
                let _ = reply.send(result);
            }
            DbusCommand::GetBatteryAvailableStartThresholds { reply } => {
                let result = get_battery_available_start_thresholds_impl(&connection).await;
                let _ = reply.send(result);
            }
            DbusCommand::GetBatteryAvailableEndThresholds { reply } => {
                let result = get_battery_available_end_thresholds_impl(&connection).await;
                let _ = reply.send(result);
            }
            DbusCommand::SetBatterySettings { settings, reply } => {
                let result = set_battery_settings_impl(&connection, settings).await;
                let _ = reply.send(result);
            }
            DbusCommand::SetFanAuto { fan_id, reply } => {
                let result = set_fan_auto_impl(&connection, fan_id).await;
                let _ = reply.send(result);
            }
            DbusCommand::SetGpuLockedClocks { device_index, min_clock, max_clock, reply } => {
                let result = set_gpu_locked_clocks_impl(&connection, device_index, min_clock, max_clock).await;
                let _ = reply.send(result);
            }
            DbusCommand::ResetGpuClocks { device_index, reply } => {
                let result = reset_gpu_clocks_impl(&connection, device_index).await;
                let _ = reply.send(result);
            }
            DbusCommand::GetGpuClockRanges { device_index, reply } => {
                let result = get_gpu_clock_ranges_impl(&connection, device_index).await;
                let _ = reply.send(result);
            }
            DbusCommand::GetGpuMemClockRanges { device_index, reply } => {
                let result = get_gpu_mem_clock_ranges_impl(&connection, device_index).await;
                let _ = reply.send(result);
            }
            DbusCommand::SetMemoryLockedClocks { device_index, min_clock, max_clock, reply } => {
                let result = set_memory_locked_clocks_impl(&connection, device_index, min_clock, max_clock).await;
                let _ = reply.send(result);
            }
            DbusCommand::ResetMemoryLockedClocks { device_index, reply } => {
                let result = reset_memory_locked_clocks_impl(&connection, device_index).await;
                let _ = reply.send(result);
            }
            DbusCommand::SetGpuCoreOffset { device_index, offset, reply } => {
                let result = set_gpu_core_offset_impl(&connection, device_index, offset).await;
                let _ = reply.send(result);
            }
            DbusCommand::SetGpuMemoryOffset { device_index, offset, reply } => {
                let result = set_gpu_memory_offset_impl(&connection, device_index, offset).await;
                let _ = reply.send(result);
            }
            DbusCommand::GetGpuCoreOffsetLimits { device_index, reply } => {
                let result = get_gpu_core_offset_limits_impl(&connection, device_index).await;
                let _ = reply.send(result);
            }
            DbusCommand::GetGpuMemoryOffsetLimits { device_index, reply } => {
                let result = get_gpu_memory_offset_limits_impl(&connection, device_index).await;
                let _ = reply.send(result);
            }
            DbusCommand::SetPrimeProfile { profile, reply } => {
                let result = set_prime_profile_impl(&connection, &profile).await;
                let _ = reply.send(result);
            }
            DbusCommand::ShutdownDaemon { reply } => {
                let result = shutdown_daemon_impl(&connection).await;
                let _ = reply.send(result);
            }
        }
    }
    
    Ok(())
}

// Implementation functions
async fn get_system_info_impl(conn: &Connection) -> Result<SystemInfo> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    
    let json: String = proxy.call("GetSystemInfo", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_memory_info_impl(conn: &Connection) -> Result<MemoryInfo> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;

    let json: String = proxy.call("GetMemoryInfo", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_cpu_info_impl(conn: &Connection) -> Result<CpuInfo> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    
    let json: String = proxy.call("GetCpuInfo", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_gpu_info_impl(conn: &Connection) -> Result<Vec<GpuInfo>> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    
    let json: String = proxy.call("GetGpuInfo", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_fan_info_impl(conn: &Connection) -> Result<Vec<FanInfo>> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    
    let json: String = proxy.call("GetFanInfo", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_battery_info_impl(conn: &Connection) -> Result<BatteryInfo> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;

    let json: String = proxy.call("GetBatteryInfo", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_storage_device_info_impl(conn: &Connection) -> Result<Vec<StorageDevice>> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;

    let json: String = proxy.call("GetStorageDeviceInfo", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_mount_info_impl(conn: &Connection) -> Result<Vec<MountInfo>> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;

    let json: String = proxy.call("GetMountInfo", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_wifi_info_impl(conn: &Connection) -> Result<Vec<WiFiInfo>> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;

    let json: String = proxy.call("GetWifiInfo", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_tdp_profiles_impl(conn: &Connection) -> Result<Vec<String>> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;

    let json: String = proxy.call("GetTdpProfiles", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn apply_profile_impl(conn: &Connection, profile: &Profile) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    
    let json = serde_json::to_string(profile)?;
    proxy.call::<_, _, ()>("ApplyProfile", &(json.as_str(),)).await?;
    Ok(())
}

async fn set_cpu_governor_impl(conn: &Connection, governor: &str) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    
    proxy.call::<_, _, ()>("SetCpuGovernor", &(governor,)).await?;
    Ok(())
}

async fn preview_keyboard_impl(conn: &Connection, settings: &KeyboardSettings) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    
    let json = serde_json::to_string(settings)?;
    proxy.call::<_, _, ()>("PreviewKeyboardSettings", &(json.as_str(),)).await?;
    Ok(())
}

async fn set_cpu_boost_impl(conn: &Connection, enabled: bool) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;

    proxy.call::<_, _, ()>("SetCpuBoost", &(enabled,)).await?;
    Ok(())
}

async fn set_amd_pstate_status_impl(conn: &Connection, status: &str) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;

    proxy.call::<_, _, ()>("SetAmdPstateStatus", &(status,)).await?;
    Ok(())
}

async fn get_battery_thresholds_impl(conn: &Connection) -> Result<(u8, u8)> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    
    let start: u8 = proxy.call("GetBatteryChargeStartThreshold", &()).await?;
    let end: u8 = proxy.call("GetBatteryChargeEndThreshold", &()).await?;
    Ok((start, end))
}

async fn set_battery_thresholds_impl(conn: &Connection, start: u8, end: u8) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    
    proxy.call::<_, _, ()>("SetBatteryChargeStartThreshold", &(start,)).await?;
    proxy.call::<_, _, ()>("SetBatteryChargeEndThreshold", &(end,)).await?;
    Ok(())
}

async fn get_battery_available_start_thresholds_impl(conn: &Connection) -> Result<Vec<u8>> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;

    let json: String = proxy.call("GetBatteryAvailableStartThresholds", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_battery_available_end_thresholds_impl(conn: &Connection) -> Result<Vec<u8>> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;

    let json: String = proxy.call("GetBatteryAvailableEndThresholds", &()).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn set_battery_settings_impl(conn: &Connection, settings: BatterySettings) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;

    let json = serde_json::to_string(&settings)?;
    proxy.call::<_, _, ()>("SetBatterySettings", &(json.as_str(),)).await?;
    Ok(())
}

async fn set_fan_auto_impl(conn: &Connection, fan_id: u32) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    proxy.call::<_, _, ()>("SetFanAuto", &(fan_id,)).await?;
    Ok(())
}

async fn set_gpu_locked_clocks_impl(conn: &Connection, device_index: u32, min_clock: u32, max_clock: u32) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    proxy.call::<_, _, ()>("SetGpuLockedClocks", &(device_index, min_clock, max_clock)).await?;
    Ok(())
}

async fn reset_gpu_clocks_impl(conn: &Connection, device_index: u32) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    proxy.call::<_, _, ()>("ResetGpuClocks", &(device_index,)).await?;
    Ok(())
}

async fn get_gpu_clock_ranges_impl(conn: &Connection, device_index: u32) -> Result<(u32, u32)> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    let json: String = proxy.call("GetGpuClockRanges", &(device_index,)).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn get_gpu_mem_clock_ranges_impl(conn: &Connection, device_index: u32) -> Result<Vec<u32>> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    let json: String = proxy.call("GetGpuMemClockRanges", &(device_index,)).await?;
    Ok(serde_json::from_str(&json)?)
}

async fn set_memory_locked_clocks_impl(conn: &Connection, device_index: u32, min_clock: u32, max_clock: u32) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    proxy.call::<_, _, ()>("SetMemoryLockedClocks", &(device_index, min_clock, max_clock)).await?;
    Ok(())
}

async fn reset_memory_locked_clocks_impl(conn: &Connection, device_index: u32) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    proxy.call::<_, _, ()>("ResetMemoryLockedClocks", &(device_index,)).await?;
    Ok(())
}

async fn set_gpu_core_offset_impl(conn: &Connection, device_index: u32, offset: i32) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    proxy.call::<_, _, ()>("SetGpuCoreOffset", &(device_index, offset)).await?;
    Ok(())
}

async fn set_gpu_memory_offset_impl(conn: &Connection, device_index: u32, offset: i32) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    proxy.call::<_, _, ()>("SetGpuMemoryOffset", &(device_index, offset)).await?;
    Ok(())
}

async fn get_gpu_core_offset_limits_impl(conn: &Connection, device_index: u32) -> Result<(i32, i32)> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    let limits: (i32, i32) = proxy.call("GetGpuCoreOffsetLimits", &(device_index,)).await?;
    Ok(limits)
}

async fn get_gpu_memory_offset_limits_impl(conn: &Connection, device_index: u32) -> Result<(i32, i32)> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    let limits: (i32, i32) = proxy.call("GetGpuMemoryOffsetLimits", &(device_index,)).await?;
    Ok(limits)
}

async fn set_prime_profile_impl(conn: &Connection, profile: &str) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    proxy.call::<_, _, ()>("SetPrimeProfile", &(profile,)).await?;
    Ok(())
}

async fn shutdown_daemon_impl(conn: &Connection) -> Result<()> {
    let proxy = zbus::Proxy::new(
        conn,
        "com.tuxedo.Control",
        "/com/tuxedo/Control",
        "com.tuxedo.Control",
    ).await?;
    proxy.call::<_, _, ()>("ShutdownDaemon", &()).await?;
    Ok(())
}
