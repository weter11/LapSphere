use anyhow::Result;
use tuxedo_common::types::*;
use zbus::{interface, Connection, ConnectionBuilder};
use std::time::Duration;

pub struct ControlInterface;

#[interface(name = "com.tuxedo.Control")]
impl ControlInterface {
    async fn get_system_info(&self) -> Result<String, zbus::fdo::Error> {
        match crate::hardware_detection::get_system_info() {
            Ok(info) => serde_json::to_string(&info)
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string())),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }

    async fn get_memory_info(&self) -> Result<String, zbus::fdo::Error> {
        match crate::hardware_detection::get_memory_info() {
            Ok(info) => serde_json::to_string(&info)
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string())),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }

    async fn get_cpu_info(&self) -> Result<String, zbus::fdo::Error> {
        match crate::hardware_detection::get_cpu_info() {
            Ok(info) => serde_json::to_string(&info)
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string())),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }

    async fn get_gpu_info(&self) -> Result<String, zbus::fdo::Error> {
        match crate::hardware_detection::get_gpu_info() {
            Ok(info) => serde_json::to_string(&info)
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string())),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }

    async fn get_battery_info(&self) -> Result<String, zbus::fdo::Error> {
        match crate::hardware_detection::get_battery_info() {
            Ok(info) => serde_json::to_string(&info)
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string())),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }

    async fn get_storage_device_info(&self) -> Result<String, zbus::fdo::Error> {
        match crate::hardware_detection::get_storage_device_info() {
            Ok(info) => serde_json::to_string(&info)
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string())),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }

    async fn get_mount_info(&self) -> Result<String, zbus::fdo::Error> {
        match crate::hardware_detection::get_mount_info() {
            Ok(info) => serde_json::to_string(&info)
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string())),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }

    async fn get_wifi_info(&self) -> Result<String, zbus::fdo::Error> {
        match crate::hardware_detection::get_wifi_info() {
            Ok(info) => serde_json::to_string(&info)
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string())),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }

    async fn set_cpu_governor(&self, governor: &str) -> Result<(), zbus::fdo::Error> {
        crate::hardware_control::set_cpu_governor(governor)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn set_cpu_frequency_limits(
        &self,
        min_freq: u64,
        max_freq: u64,
    ) -> Result<(), zbus::fdo::Error> {
        crate::hardware_control::set_cpu_frequency_limits(min_freq, max_freq)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn set_cpu_boost(&self, enabled: bool) -> Result<(), zbus::fdo::Error> {
        crate::hardware_control::set_cpu_boost(enabled)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn set_smt(&self, enabled: bool) -> Result<(), zbus::fdo::Error> {
        crate::hardware_control::set_smt(enabled)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn set_amd_pstate_status(&self, status: &str) -> Result<(), zbus::fdo::Error> {
        crate::hardware_control::set_amd_pstate_status(status)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn apply_profile(&self, profile_json: &str) -> Result<(), zbus::fdo::Error> {
        let profile: Profile = serde_json::from_str(profile_json)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        crate::hardware_control::apply_profile(&profile)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn get_tdp_profiles(&self) -> Result<String, zbus::fdo::Error> {
    match crate::hardware_detection::get_tdp_profiles() {
        Ok(profiles) => serde_json::to_string(&profiles)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string())),
        Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
    }
}

    async fn get_current_tdp_profile(&self) -> Result<String, zbus::fdo::Error> {
        match crate::hardware_detection::get_current_tdp_profile() {
            Ok(profile) => Ok(profile),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }

async fn set_tdp_profile(&self, profile: &str) -> Result<(), zbus::fdo::Error> {
    crate::hardware_control::set_tdp_profile(profile)
        .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
}

    async fn get_fan_speeds(&self) -> Result<String, zbus::fdo::Error> {
    match crate::hardware_detection::get_fan_speeds() {
        Ok(fans) => serde_json::to_string(&fans)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string())),
        Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
    }
}

    async fn get_fan_info(&self) -> Result<String, zbus::fdo::Error> {
        if !crate::tuxedo_io::TuxedoIo::is_available() {
            return Ok("[]".to_string());
        }
        
        match crate::tuxedo_io::TuxedoIo::new() {
            Ok(io) => {
                let mut fans_info = Vec::new();
                for fan_id in 0..io.get_fan_count() {
                    let speed = io.get_fan_speed(fan_id).ok();
                    let temperature = io.get_fan_temperature(fan_id).ok().map(|t| t as f32);
                    
                    let info = FanInfo {
                        id: fan_id,
                        name: format!("Fan {}", fan_id),
                        rpm_or_percent: speed.unwrap_or(0),
                        temperature,
                        is_rpm: false,  // Currently returning percentage
                    };
                    fans_info.push(info);
                }
                serde_json::to_string(&fans_info)
                    .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
            }
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }

    async fn get_fan_temperature(&self, fan_id: u32) -> Result<u32, zbus::fdo::Error> {
        if !crate::tuxedo_io::TuxedoIo::is_available() {
            return Err(zbus::fdo::Error::Failed("tuxedo_io not available".to_string()));
        }
        
        match crate::tuxedo_io::TuxedoIo::new() {
            Ok(io) => io.get_fan_temperature(fan_id)
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string())),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }
    
    async fn set_fan_speed(&self, fan_id: u32, speed: u32) -> Result<(), zbus::fdo::Error> {
        crate::hardware_control::set_fan_speed(fan_id, speed)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
    
    async fn set_fan_auto(&self, fan_id: u32) -> Result<(), zbus::fdo::Error> {
        crate::hardware_control::set_fan_auto(fan_id)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
    
    async fn get_webcam_state(&self) -> Result<bool, zbus::fdo::Error> {
        crate::hardware_control::get_webcam_state()
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
    
    async fn set_webcam_state(&self, enabled: bool) -> Result<(), zbus::fdo::Error> {
        crate::hardware_control::set_webcam_state(enabled)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
    
    // Battery charge control methods
    async fn get_battery_charge_type(&self) -> Result<String, zbus::fdo::Error> {
        match crate::battery_control::BatteryControl::new() {
            Ok(battery) => battery.get_charge_type()
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string())),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }
    
    async fn set_battery_charge_type(&self, charge_type: &str) -> Result<(), zbus::fdo::Error> {
        match crate::battery_control::BatteryControl::new() {
            Ok(battery) => battery.set_charge_type(charge_type)
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string())),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }
    
    async fn get_battery_charge_start_threshold(&self) -> Result<u8, zbus::fdo::Error> {
        match crate::battery_control::BatteryControl::new() {
            Ok(battery) => battery.get_charge_control_start_threshold()
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string())),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }
    
    async fn set_battery_charge_start_threshold(&self, threshold: u8) -> Result<(), zbus::fdo::Error> {
        match crate::battery_control::BatteryControl::new() {
            Ok(battery) => battery.set_charge_control_start_threshold(threshold)
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string())),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }
    
    async fn get_battery_charge_end_threshold(&self) -> Result<u8, zbus::fdo::Error> {
        match crate::battery_control::BatteryControl::new() {
            Ok(battery) => battery.get_charge_control_end_threshold()
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string())),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }
    
    async fn set_battery_charge_end_threshold(&self, threshold: u8) -> Result<(), zbus::fdo::Error> {
        match crate::battery_control::BatteryControl::new() {
            Ok(battery) => battery.set_charge_control_end_threshold(threshold)
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string())),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }
    
    async fn get_battery_available_start_thresholds(&self) -> Result<String, zbus::fdo::Error> {
        match crate::battery_control::BatteryControl::new() {
            Ok(battery) => {
                let thresholds = battery.get_available_start_thresholds()
                    .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
                serde_json::to_string(&thresholds)
                    .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
            }
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }
    
    async fn get_battery_available_end_thresholds(&self) -> Result<String, zbus::fdo::Error> {
        match crate::battery_control::BatteryControl::new() {
            Ok(battery) => {
                let thresholds = battery.get_available_end_thresholds()
                    .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
                serde_json::to_string(&thresholds)
                    .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
            }
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }
    
    async fn get_hardware_interface_info(&self) -> Result<String, zbus::fdo::Error> {
        if !crate::tuxedo_io::TuxedoIo::is_available() {
            return Ok("None".to_string());
        }
        
        match crate::tuxedo_io::TuxedoIo::new() {
            Ok(io) => {
                let interface = match io.get_interface() {
                    crate::tuxedo_io::HardwareInterface::Clevo => "Clevo",
                    crate::tuxedo_io::HardwareInterface::Uniwill => "Uniwill",
                    crate::tuxedo_io::HardwareInterface::None => "None",
                };
                let fan_count = io.get_fan_count();
                Ok(format!("Interface: {}, Fans: {}", interface, fan_count))
            }
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }
    
    // Keyboard preview - apply keyboard settings immediately without saving to profile
    async fn preview_keyboard_settings(&self, settings_json: &str) -> Result<(), zbus::fdo::Error> {
        let settings: KeyboardSettings = serde_json::from_str(settings_json)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        crate::hardware_control::preview_keyboard_settings(&settings)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn set_battery_settings(&self, settings_json: &str) -> Result<(), zbus::fdo::Error> {
        let settings: BatterySettings = serde_json::from_str(settings_json)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        crate::hardware_control::apply_battery_settings(&settings)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn set_gpu_locked_clocks(&self, device_index: u32, min_clock: u32, max_clock: u32) -> Result<(), zbus::fdo::Error> {
        crate::hardware_control::set_gpu_locked_clocks(device_index, min_clock, max_clock)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn set_memory_locked_clocks(&self, device_index: u32, min_clock: u32, max_clock: u32) -> Result<(), zbus::fdo::Error> {
        crate::hardware_control::set_memory_locked_clocks(device_index, min_clock, max_clock)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn reset_memory_locked_clocks(&self, device_index: u32) -> Result<(), zbus::fdo::Error> {
        crate::hardware_control::reset_memory_locked_clocks(device_index)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn reset_gpu_clocks(&self, device_index: u32) -> Result<(), zbus::fdo::Error> {
        crate::hardware_control::reset_gpu_clocks(device_index)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn get_gpu_clock_ranges(&self, device_index: u32) -> Result<String, zbus::fdo::Error> {
        match crate::hardware_detection::get_gpu_clock_ranges(device_index) {
            Ok(ranges) => serde_json::to_string(&ranges)
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string())),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }

    async fn get_gpu_mem_clock_ranges(&self, device_index: u32) -> Result<String, zbus::fdo::Error> {
        match crate::hardware_detection::get_gpu_mem_clock_ranges(device_index) {
            Ok(ranges) => serde_json::to_string(&ranges)
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string())),
            Err(e) => Err(zbus::fdo::Error::Failed(e.to_string())),
        }
    }

    async fn set_gpu_core_offset(&self, device_index: u32, offset: i32) -> Result<(), zbus::fdo::Error> {
        crate::hardware_control::set_gpu_core_offset(device_index, offset)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn set_gpu_memory_offset(&self, device_index: u32, offset: i32) -> Result<(), zbus::fdo::Error> {
        crate::hardware_control::set_gpu_memory_offset(device_index, offset)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn get_gpu_core_offset_limits(&self, device_index: u32) -> Result<(i32, i32), zbus::fdo::Error> {
        crate::hardware_detection::get_gpu_core_offset_limits(device_index)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn get_gpu_memory_offset_limits(&self, device_index: u32) -> Result<(i32, i32), zbus::fdo::Error> {
        crate::hardware_detection::get_gpu_memory_offset_limits(device_index)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn set_prime_profile(&self, profile: &str) -> Result<(), zbus::fdo::Error> {
        crate::hardware_control::set_prime_profile(profile)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    // Polling scheduler methods
    async fn update_polling_interval(&self, component: &str, interval_ms: u64) -> Result<(), zbus::fdo::Error> {
        if let Some(handle) = crate::SCHEDULER_HANDLE.get() {
            let interval = std::time::Duration::from_millis(interval_ms);
            handle.update_interval(component.to_string(), interval)
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
        } else {
            Err(zbus::fdo::Error::Failed("Scheduler not initialized".to_string()))
        }
    }

    async fn shutdown_daemon(&self) -> Result<(), zbus::fdo::Error> {
        if is_systemd_managed() {
            return Ok(());
        }

        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(200));
            std::process::exit(0);
        });

        Ok(())
    }
}

fn is_systemd_managed() -> bool {
    std::env::var_os("INVOCATION_ID").is_some()
        || std::env::var_os("SYSTEMD_EXEC_PID").is_some()
}

pub async fn start_service(_connection: Connection) -> Result<()> {
    let _conn = ConnectionBuilder::system()?
        .name("com.tuxedo.Control")?
        .serve_at("/com/tuxedo/Control", ControlInterface)?
        .build()
        .await?;
    
    // Keep connection alive
    std::future::pending::<()>().await;
    Ok(())
}
