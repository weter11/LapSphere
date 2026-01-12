use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub product_name: String,
    pub manufacturer: String,
    pub bios_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuInfo {
    pub name: String,
    pub median_frequency: u64,
    pub median_load: f32,
    pub package_temp: f32,
    pub package_power: Option<f32>,
    pub power_source: Option<String>,
    pub all_power_sources: Vec<PowerSource>,
    pub cores: Vec<CoreInfo>,
    pub governor: String,
    pub available_governors: Vec<String>,
    pub boost_enabled: bool,
    pub smt_enabled: bool,
    pub scaling_driver: String,
    pub amd_pstate_status: Option<String>,
    pub min_freq: Option<u64>,
    pub max_freq: Option<u64>,
    pub hw_min_freq: u64,
    pub hw_max_freq: u64,
    pub available_pstate_controls: Vec<String>, // NEW: List of available controls for current pstate
    pub energy_performance_preference: Option<String>, // NEW: EPP value
    pub available_epp_preferences: Vec<String>, // NEW: Available EPP values
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerSource {
    pub name: String,
    pub value: f32,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreInfo {
    pub id: u32,
    pub frequency: u64,
    pub load: f32,
    pub temperature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    pub gpu_type: GpuType,
    pub status: GpuStatus, // NEW: Enum instead of string
    pub frequency: Option<u64>,
    pub temperature: Option<f32>,
    pub load: Option<f32>,
    pub power: Option<f32>,
    pub voltage: Option<f32>,
    pub driver: Option<String>, // NEW: Driver name
    pub pci_id: Option<String>, // NEW: PCI ID
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GpuType {
    Integrated,
    Discrete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GpuStatus {
    Active,
    Suspended,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatteryInfo {
    pub voltage_mv: u64,
    pub current_ma: i64,
    pub charge_percent: u64,
    pub capacity_mah: u64,
    pub manufacturer: String,
    pub model: String,
    pub charge_start_threshold: Option<u8>,
    pub charge_end_threshold: Option<u8>,
    pub status: BatteryStatus, // NEW: Charging/Discharging/Full/Unknown
    pub power_draw_w: Option<f32>, // NEW: Actual power draw in watts
    pub on_ac_power: bool, // NEW: Whether on AC power
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BatteryStatus {
    Charging,
    Discharging,
    Full,
    NotCharging,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanInfo {
    pub id: u32,
    pub name: String,
    pub rpm: Option<u32>, // NEW: Actual RPM reading
    pub duty_percent: u32, // Current duty cycle percentage
    pub temperature: Option<f32>, // NEW: Associated temperature sensor
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WiFiInfo {
    pub interface: String,
    pub chip_model: String, // NEW: Actual chip model
    pub driver: String,
    pub link_speed_mbps: Option<u32>, // NEW: Link speed
    pub signal_strength: Option<i32>, // NEW: Signal in dBm
    pub ssid: Option<String>, // NEW: Connected SSID
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageInfo {
    pub device: String,
    pub model: String,
    pub size_gb: u64,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub is_default: bool,
    pub cpu_settings: CpuSettings,
    pub gpu_settings: GpuSettings,
    pub keyboard_settings: KeyboardSettings,
    pub screen_settings: ScreenSettings,
    pub fan_settings: FanSettings,
    pub auto_switch: AutoSwitchSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuSettings {
    pub governor: Option<String>,
    pub min_frequency: Option<u64>,
    pub max_frequency: Option<u64>,
    pub boost: Option<bool>,
    pub smt: Option<bool>,
    pub performance_profile: Option<String>,
    pub tdp: Option<u32>,
    pub amd_pstate_status: Option<String>,
    pub energy_performance_preference: Option<String>, // NEW: EPP setting
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuSettings {
    pub dgpu_tdp: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardSettings {
    pub control_enabled: bool,
    pub mode: KeyboardMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeyboardMode {
    SingleColor { 
        r: u8, 
        g: u8, 
        b: u8, 
        brightness: u8 
    },
    Effect { 
        effect: String, 
        speed: u8 
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenSettings {
    pub brightness: u8,
    pub system_control: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanSettings {
    pub control_enabled: bool,
    pub curves: Vec<FanCurve>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanCurve {
    pub fan_id: u32,
    pub points: Vec<(u8, u8)>, // (temperature, speed) - 8 points
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoSwitchSettings {
    pub enabled: bool,
    pub app_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub theme: Theme,
    pub start_minimized: bool,
    pub autostart: bool,
    pub fan_daemon_enabled: bool,
    pub app_monitoring_enabled: bool,
    pub cpu_scheduler: String,
    pub statistics_sections: StatisticsSections,
    pub tuning_section_order: Vec<String>,
    pub profiles: Vec<Profile>,
    pub current_profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Theme {
    Auto,
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatisticsSections {
    pub show_system_info: bool,
    pub show_cpu: bool,
    pub show_gpu: bool,
    pub show_battery: bool,
    pub show_wifi: bool,
    pub show_storage: bool,
    pub show_fans: bool,
    pub section_order: Vec<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: Theme::Auto,
            start_minimized: false,
            autostart: false,
            fan_daemon_enabled: true,
            app_monitoring_enabled: false, // Disabled by default per requirements
            cpu_scheduler: "CFS".to_string(),
            statistics_sections: StatisticsSections::default(),
            tuning_section_order: vec![
                "Keyboard".to_string(),
                "CPU".to_string(),
                "GPU".to_string(),
                "Screen".to_string(),
                "Fans".to_string(),
            ],
            profiles: vec![Profile::default()],
            current_profile: "Default".to_string(),
        }
    }
}

impl Default for StatisticsSections {
    fn default() -> Self {
        Self {
            show_system_info: true,
            show_cpu: true,
            show_gpu: true,
            show_battery: true,
            show_wifi: true,
            show_storage: true,
            show_fans: true,
            section_order: vec![
                "SystemInfo".to_string(),
                "CPU".to_string(),
                "GPU".to_string(),
                "Battery".to_string(),
                "WiFi".to_string(),
                "Storage".to_string(),
                "Fans".to_string(),
            ],
        }
    }
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            is_default: true,
            cpu_settings: CpuSettings::default(),
            gpu_settings: GpuSettings::default(),
            keyboard_settings: KeyboardSettings::default(),
            screen_settings: ScreenSettings::default(),
            fan_settings: FanSettings::default(),
            auto_switch: AutoSwitchSettings::default(),
        }
    }
}

impl Default for CpuSettings {
    fn default() -> Self {
        Self {
            governor: None,
            min_frequency: None,
            max_frequency: None,
            boost: None,
            smt: None,
            performance_profile: None,
            tdp: None,
            amd_pstate_status: None,
            energy_performance_preference: None,
        }
    }
}

impl Default for GpuSettings {
    fn default() -> Self {
        Self { dgpu_tdp: None }
    }
}

impl Default for KeyboardSettings {
    fn default() -> Self {
        Self {
            control_enabled: false,
            mode: KeyboardMode::SingleColor {
                r: 255,
                g: 255,
                b: 255,
                brightness: 50,
            },
        }
    }
}

impl Default for ScreenSettings {
    fn default() -> Self {
        Self {
            brightness: 50,
            system_control: true,
        }
    }
}

impl Default for FanSettings {
    fn default() -> Self {
        Self {
            control_enabled: false,
            curves: vec![],
        }
    }
}

impl Default for AutoSwitchSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            app_names: vec![],
        }
    }
}
