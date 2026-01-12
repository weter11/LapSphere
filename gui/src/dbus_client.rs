use anyhow::Result;
use tuxedo_common::types::*;
use zbus::blocking::Connection;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct DbusClient {
    connection: Arc<Mutex<Connection>>,
}

impl DbusClient {
    pub fn new() -> Result<Self> {
        let connection = Connection::system()?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }
    
    fn get_proxy(&self) -> Result<zbus::blocking::Proxy> {
        let conn = self.connection.lock().unwrap();
        let proxy = zbus::blocking::Proxy::new(
            &*conn,
            "com.tuxedo.Control",
            "/com/tuxedo/Control",
            "com.tuxedo.Control",
        )?;
        Ok(proxy)
    }
    
    // System info
    pub fn get_system_info(&self) -> Result<SystemInfo> {
        let proxy = self.get_proxy()?;
        let json: String = proxy.call("GetSystemInfo", &())?;
        Ok(serde_json::from_str(&json)?)
    }
    
    // CPU info
    pub fn get_cpu_info(&self) -> Result<CpuInfo> {
        let proxy = self.get_proxy()?;
        let json: String = proxy.call("GetCpuInfo", &())?;
        Ok(serde_json::from_str(&json)?)
    }
    
    // GPU info
    pub fn get_gpu_info(&self) -> Result<Vec<GpuInfo>> {
        let proxy = self.get_proxy()?;
        let json: String = proxy.call("GetGpuInfo", &())?;
        Ok(serde_json::from_str(&json)?)
    }
    
    // Battery info
    pub fn get_battery_info(&self) -> Result<BatteryInfo> {
        let proxy = self.get_proxy()?;
        let json: String = proxy.call("GetBatteryInfo", &())?;
        Ok(serde_json::from_str(&json)?)
    }
    
    // WiFi info
    pub fn get_wifi_info(&self) -> Result<Vec<WiFiInfo>> {
        let proxy = self.get_proxy()?;
        let json: String = proxy.call("GetWifiInfo", &())?;
        Ok(serde_json::from_str(&json)?)
    }
    
    // Fan info
    pub fn get_fan_info(&self) -> Result<Vec<FanInfo>> {
        let proxy = self.get_proxy()?;
        let json: String = proxy.call("GetFanInfo", &())?;
        Ok(serde_json::from_str(&json)?)
    }
    
    // CPU control
    pub fn set_cpu_governor(&self, governor: &str) -> Result<()> {
        let proxy = self.get_proxy()?;
        proxy.call::<_, _, ()>("SetCpuGovernor", &(governor,))?;
        Ok(())
    }
    
    pub fn set_cpu_frequency_limits(&self, min_freq: u64, max_freq: u64) -> Result<()> {
        let proxy = self.get_proxy()?;
        proxy.call::<_, _, ()>("SetCpuFrequencyLimits", &(min_freq, max_freq))?;
        Ok(())
    }
    
    pub fn set_cpu_boost(&self, enabled: bool) -> Result<()> {
        let proxy = self.get_proxy()?;
        proxy.call::<_, _, ()>("SetCpuBoost", &(enabled,))?;
        Ok(())
    }
    
    pub fn set_smt(&self, enabled: bool) -> Result<()> {
        let proxy = self.get_proxy()?;
        proxy.call::<_, _, ()>("SetSmt", &(enabled,))?;
        Ok(())
    }
    
    pub fn set_amd_pstate_status(&self, status: &str) -> Result<()> {
        let proxy = self.get_proxy()?;
        proxy.call::<_, _, ()>("SetAmdPstateStatus", &(status,))?;
        Ok(())
    }
    
    pub fn set_energy_performance_preference(&self, preference: &str) -> Result<()> {
        let proxy = self.get_proxy()?;
        proxy.call::<_, _, ()>("SetEnergyPerformancePreference", &(preference,))?;
        Ok(())
    }
    
    // Profile application
    pub fn apply_profile(&self, profile: &Profile) -> Result<()> {
        let proxy = self.get_proxy()?;
        let json = serde_json::to_string(profile)?;
        proxy.call::<_, _, ()>("ApplyProfile", &(json.as_str(),))?;
        Ok(())
    }
}
