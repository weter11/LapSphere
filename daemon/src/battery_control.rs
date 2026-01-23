use anyhow::{anyhow, Result};
use std::fs;
use std::path::PathBuf;

pub struct BatteryControl {
    battery_path: PathBuf,
}

impl BatteryControl {
    pub fn new() -> Result<Self> {
        let battery_path = Self::find_battery_path()?;
        Ok(Self { battery_path })
    }
    
    pub fn is_available() -> bool {
        Self::find_battery_path().is_ok()
    }
    
    fn find_battery_path() -> Result<PathBuf> {
        for bat in &["BAT0", "BAT1"] {
            let path = PathBuf::from(format!("/sys/class/power_supply/{}", bat));
            if path.exists() {
                // Check if charge control is available
                let charge_type_path = path.join("charge_type");
                if charge_type_path.exists() {
                    return Ok(path);
                }
            }
        }
        Err(anyhow!("No battery with charge control found"))
    }
    
    /// Get charge control mode: "Standard" or "Custom"
    pub fn get_charge_type(&self) -> Result<String> {
        let path = self.battery_path.join("charge_type");
        let content = fs::read_to_string(&path)?;
        Ok(content.trim().to_string())
    }
    
    /// Set charge control mode: "Standard" or "Custom"
    pub fn set_charge_type(&self, charge_type: &str) -> Result<()> {
        if charge_type != "Standard" && charge_type != "Custom" {
            return Err(anyhow!("Invalid charge type. Must be 'Standard' or 'Custom'"));
        }
        
        let path = self.battery_path.join("charge_type");
        fs::write(&path, charge_type)?;
        Ok(())
    }
    
    /// Get charge start threshold (percentage)
    pub fn get_charge_control_start_threshold(&self) -> Result<u8> {
        let path = self.battery_path.join("charge_control_start_threshold");
        let content = fs::read_to_string(&path)?;
        let value: u8 = content.trim().parse()?;
        Ok(value)
    }
    
    /// Set charge start threshold (percentage)
    pub fn set_charge_control_start_threshold(&self, threshold: u8) -> Result<()> {
        if threshold > 100 {
            return Err(anyhow!("Threshold must be between 0 and 100"));
        }
        
        let path = self.battery_path.join("charge_control_start_threshold");
        fs::write(&path, threshold.to_string())?;
        Ok(())
    }
    
    /// Get charge end threshold (percentage)
    pub fn get_charge_control_end_threshold(&self) -> Result<u8> {
        let path = self.battery_path.join("charge_control_end_threshold");
        let content = fs::read_to_string(&path)?;
        let value: u8 = content.trim().parse()?;
        Ok(value)
    }
    
    /// Set charge end threshold (percentage)
    pub fn set_charge_control_end_threshold(&self, threshold: u8) -> Result<()> {
        if threshold > 100 {
            return Err(anyhow!("Threshold must be between 0 and 100"));
        }
        
        let path = self.battery_path.join("charge_control_end_threshold");
        fs::write(&path, threshold.to_string())?;
        Ok(())
    }
    
    /// Get available start thresholds
    pub fn get_available_start_thresholds(&self) -> Result<Vec<u8>> {
        let path = self.battery_path.join("charge_control_start_available_thresholds");
        if !path.exists() {
            // Return default values if not available
            return Ok(vec![40, 50, 60, 70, 80, 95]);
        }
        
        let content = fs::read_to_string(&path)?;
        let thresholds: Vec<u8> = content
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect();
        Ok(thresholds)
    }
    
    /// Get available end thresholds
    pub fn get_available_end_thresholds(&self) -> Result<Vec<u8>> {
        let path = self.battery_path.join("charge_control_end_available_thresholds");
        if !path.exists() {
            // Return default values if not available
            return Ok(vec![60, 70, 80, 90, 100]);
        }
        
        let content = fs::read_to_string(&path)?;
        let thresholds: Vec<u8> = content
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect();
        Ok(thresholds)
    }
    
}
