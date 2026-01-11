use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::time;
use tuxedo_common::types::{FanCurve, FanSettings};
use crate::tuxedo_io;
use crate::tuxedo_io::TuxedoIo;
use crate::dbus_interface::ControlInterface;

pub struct FanCurveManager {
    io: Option<TuxedoIo>,
    settings: Option<FanSettings>,
    last_update: Instant,
    update_interval: Duration,
    last_speeds: HashMap<u32, u8>,
}

impl FanCurveManager {
    pub fn new() -> Result<Self> {
        let io = if TuxedoIo::is_available() {
            Some(TuxedoIo::new()?)
        } else {
            None
        };
        
        Ok(Self {
            io,
            settings: None,
            last_update: Instant::now(),
            update_interval: Duration::from_secs(2),
            last_speeds: HashMap::new(),
        })
    }
    
    pub fn update_settings(&mut self, settings: FanSettings) {
        self.settings = Some(settings);
    }
    
    pub fn is_enabled(&self) -> bool {
        self.settings.as_ref().map(|s| s.control_enabled).unwrap_or(false)
    }
    
    pub fn apply_curves(&mut self) -> Result<()> {
        if !self.is_enabled() {
            return Ok(());
        }
        
        // Rate limiting
        if self.last_update.elapsed() < self.update_interval {
            return Ok(());
        }
        
        let io = self.io.as_ref().ok_or_else(|| anyhow!("TuxedoIo not available"))?;
        let settings = self.settings.as_ref().ok_or_else(|| anyhow!("No settings configured"))?;
        
        for curve in &settings.curves {
            if curve.fan_id >= io.get_fan_count() {
                continue;
            }
            
            // Get current temperature for this fan
            let temp = match io.get_fan_temperature(curve.fan_id) {
                Ok(t) => t as f32,
                Err(e) => {
                    log::warn!("Failed to read fan {} temperature: {}", curve.fan_id, e);
                    continue;
                }
            };
            
            // Calculate speed from curve
            let speed = self.interpolate_fan_speed(&curve.points, temp);

            // Only apply if speed has changed
            let last_speed = self.last_speeds.get(&curve.fan_id).cloned().unwrap_or(0);
            if speed != last_speed {
                if let Err(e) = io.set_fan_speed(curve.fan_id, speed as u32) {
                    log::error!("Failed to set fan {} speed: {}", curve.fan_id, e);
                } else {
                    self.last_speeds.insert(curve.fan_id, speed);
                    log::debug!("Fan {}: temp={}°C, new speed={}% (was {}%)", curve.fan_id, temp, speed, last_speed);
                }
            }
        }
        
        self.last_update = Instant::now();
        Ok(())
    }
    
    fn interpolate_fan_speed(&self, points: &[(u8, u8)], temp: f32) -> u8 {
        if points.is_empty() {
            return 50; // Default fallback
        }
        
        if points.len() == 1 {
            return points[0].1;
        }
        
        // Sort points by temperature
        let mut sorted_points = points.to_vec();
        sorted_points.sort_by_key(|p| p.0);
        
        // Below first point
        if temp <= sorted_points[0].0 as f32 {
            return sorted_points[0].1;
        }
        
        // Above last point
        if temp >= sorted_points[sorted_points.len() - 1].0 as f32 {
            return sorted_points[sorted_points.len() - 1].1;
        }
        
        // Linear interpolation between points
        for i in 0..sorted_points.len() - 1 {
            let (temp1, speed1) = sorted_points[i];
            let (temp2, speed2) = sorted_points[i + 1];
            
            if temp >= temp1 as f32 && temp <= temp2 as f32 {
                let ratio = (temp - temp1 as f32) / (temp2 as f32 - temp1 as f32);
                let speed = speed1 as f32 + ratio * (speed2 as f32 - speed1 as f32);
                return speed.round() as u8;
            }
        }
        
        50 // Fallback
    }
    
    pub fn set_auto_mode(&mut self) -> Result<()> {
        if let Some(ref io) = self.io {
            io.set_fan_auto()?;
            self.settings = None;
            log::info!("Fan curve control disabled, returned to auto mode");
        }
        Ok(())
    }
}

// DBus interface methods for fan curve management
impl ControlInterface {
    async fn set_fan_curve(&self, fan_id: u32, curve_json: &str) -> Result<(), zbus::fdo::Error> {
        let curve: FanCurve = serde_json::from_str(curve_json)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        
        // Validate curve points
        if curve.points.is_empty() {
            return Err(zbus::fdo::Error::Failed("Curve must have at least one point".to_string()));
        }
        
        if curve.points.len() > 16 {
            return Err(zbus::fdo::Error::Failed("Curve can have at most 16 points".to_string()));
        }
        
        // Validate point values
        for (temp, speed) in &curve.points {
            if *temp > 100 {
                return Err(zbus::fdo::Error::Failed("Temperature must be 0-100°C".to_string()));
            }
            if *speed > 100 {
                return Err(zbus::fdo::Error::Failed("Speed must be 0-100%".to_string()));
            }
        }
        
        // Store curve (would need a global state manager)
        log::info!("Set fan curve for fan {}: {} points", fan_id, curve.points.len());
        Ok(())
    }
    
    async fn get_fan_curve(&self, fan_id: u32) -> Result<String, zbus::fdo::Error> {
        // Retrieve stored curve (would need a global state manager)
        // For now, return empty curve
        let curve = FanCurve {
            fan_id,
            points: vec![],
        };
        serde_json::to_string(&curve)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
    
    async fn test_fan_curve(&self, fan_id: u32, temperature: u8) -> Result<u8, zbus::fdo::Error> {
        // Test what speed would be applied at given temperature
        // This helps users preview their curve
        Ok(50) // Placeholder
    }
}

pub struct FanDaemon {
    settings: Option<FanSettings>,
    running: bool,
}

impl FanDaemon {
    pub fn new() -> Self {
        Self {
            settings: None,
            running: false,
        }
    }
    
    pub fn update_settings(&mut self, settings: FanSettings) {
        self.settings = Some(settings);
    }
    
    pub async fn run(&mut self) {
        self.running = true;
        let mut interval = time::interval(Duration::from_secs(2));
        
        while self.running {
            interval.tick().await;
            
            if let Some(ref settings) = self.settings {
                if settings.control_enabled {
                    if let Err(e) = self.apply_fan_curves(settings) {
                        log::error!("Failed to apply fan curves: {}", e);
                    }
                }
            }
        }
    }
    
    pub fn stop(&mut self) {
        self.running = false;
    }
    
    fn apply_fan_curves(&self, settings: &FanSettings) -> Result<()> {
        // Use /dev/tuxedo_io instead of sysfs
        if !TuxedoIo::is_available() {
            return Ok(());
        }
        
        let io = TuxedoIo::new()?;
        
        for curve in &settings.curves {
            // Read current temperature for this fan
            let temp = match io.get_fan_temperature(curve.fan_id) {
                Ok(t) => t as u8,
                Err(e) => {
                    log::warn!("Failed to read temperature for fan {}: {}", curve.fan_id, e);
                    continue;
                }
            };
            
            // Find appropriate speed based on curve
            let speed = self.calculate_fan_speed(temp, &curve.points);
            
            // Apply speed using tuxedo_io ioctl
            if let Err(e) = io.set_fan_speed(curve.fan_id, speed as u32) {
                log::error!("Failed to set speed for fan {}: {}", curve.fan_id, e);
            }
        }
        
        Ok(())
    }
    
    fn calculate_fan_speed(&self, temp: u8, points: &[(u8, u8)]) -> u8 {
        if points.is_empty() {
            return 50; // Default
        }
        
        // Find the two points to interpolate between
        for i in 0..points.len() - 1 {
            let (temp1, speed1) = points[i];
            let (temp2, speed2) = points[i + 1];
            
            if temp >= temp1 && temp <= temp2 {
                // Linear interpolation
                let temp_range = temp2 - temp1;
                if temp_range == 0 {
                    return speed1;
                }
                
                let temp_offset = temp - temp1;
                let speed_range = speed2 as i16 - speed1 as i16;
                let speed = speed1 as i16 + (speed_range * temp_offset as i16) / temp_range as i16;
                
                return speed.clamp(0, 100) as u8;
            }
        }
        
        // If temp is below first point
        if temp < points[0].0 {
            return points[0].1;
        }
        
        // If temp is above last point
        points.last().unwrap().1
    }
}
