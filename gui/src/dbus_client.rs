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
    GetCpuInfo { reply: oneshot::Sender<Result<CpuInfo>> },
    GetGpuInfo { reply: oneshot::Sender<Result<Vec<GpuInfo>>> },
    GetFanInfo { reply: oneshot::Sender<Result<Vec<FanInfo>>> },
    GetBatteryInfo { reply: oneshot::Sender<Result<BatteryInfo>> },
    GetStorageDeviceInfo { reply: oneshot::Sender<Result<Vec<StorageDevice>>> },
    GetMountInfo { reply: oneshot::Sender<Result<Vec<MountInfo>>> },
    GetWifiInfo { reply: oneshot::Sender<Result<Vec<WiFiInfo>>> },
    ApplyProfile { profile: Profile, reply: oneshot::Sender<Result<()>> },
    SetCpuGovernor { governor: String, reply: oneshot::Sender<Result<()>> },
    SetCpuBoost { enabled: bool, reply: oneshot::Sender<Result<()>> },
    PreviewKeyboard { settings: KeyboardSettings, reply: oneshot::Sender<Result<()>> },
    GetBatteryChargeThresholds { reply: oneshot::Sender<Result<(u8, u8)>> },
    SetBatteryChargeThresholds { start: u8, end: u8, reply: oneshot::Sender<Result<()>> },
    GetBatteryAvailableStartThresholds { reply: oneshot::Sender<Result<Vec<u8>>> },
    GetBatteryAvailableEndThresholds { reply: oneshot::Sender<Result<Vec<u8>>> },
    SetBatterySettings { settings: BatterySettings, reply: oneshot::Sender<Result<()>> },
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
