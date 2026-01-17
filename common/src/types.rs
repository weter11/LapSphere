use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub product_name: String,
    pub product_sku: String,
    pub manufacturer: String,
    pub board_name: String,
    pub bios_version: String,
    pub tuxedo_kernel_modules: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuInfo {
    pub name: String,
    pub median_frequency: u64,
    pub median_load: f32,
    pub package_temp: f32,
    pub package_power: Option<f32>,
    pub power_source: Option<String>,  // NEW: Shows source of power reading
    pub all_power_sources: Vec<PowerSource>,  // NEW: All available power sources
    pub cores: Vec<CoreInfo>,
    pub governor: String,
    pub available_governors: Vec<String>,
    pub boost_enabled: bool,
    pub smt_enabled: bool,
    pub scaling_driver: String,
    pub amd_pstate_status: Option<String>,
    pub min_freq: Option<u64>,
    pub max_freq: Option<u64>,
    pub hw_min_freq: Option<u64>,
    pub hw_max_freq: Option<u64>,
    pub energy_performance_preference: Option<String>,
    pub available_epp_options: Vec<String>,
    pub scheduler: String,
    pub available_schedulers: Vec<String>,
    pub capabilities: CpuCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuCapabilities {
    pub has_boost: bool,
    pub has_cpuinfo_max_freq: bool,
    pub has_cpuinfo_min_freq: bool,
    pub has_scaling_driver: bool,
    pub has_energy_performance_preference: bool,
    pub has_scaling_governor: bool,
    pub has_smt: bool,
    pub has_scaling_min_freq: bool,
    pub has_scaling_max_freq: bool,
    pub has_available_governors: bool,
    pub has_amd_pstate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerSource {
    pub name: String,      // e.g., "RAPL", "amdgpu", "zenpower"
    pub value: f32,        // Power in watts
    pub description: String,  // e.g., "Intel RAPL", "AMD APU (CPU+iGPU)", "Zenpower driver"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreInfo {
    pub id: u32,
    pub frequency: u64,
    pub load: f32,
    pub temperature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryInfo {
    pub total_gib: f64,
    pub used_gib: f64,
    pub free_gib: f64,
    pub available_gib: f64,
    pub used_percent: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    pub gpu_type: GpuType,
    pub status: String,
    pub frequency: Option<u64>,
    pub memory_frequency: Option<u64>,
    pub temperature: Option<f32>,
    pub load: Option<f32>,
    pub power: Option<f32>,
    pub voltage: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GpuType {
    Integrated,
    Discrete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatteryInfo {
    pub status: String,
    pub voltage_mv: u64,
    pub current_ma: i64,
    pub charge_percent: u64,
    pub capacity_mah: u64,
    pub manufacturer: String,
    pub model: String,
    pub charge_start_threshold: Option<u8>,
    pub charge_end_threshold: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanInfo {
    pub id: u32,
    pub name: String,
    pub rpm_or_percent: u32,
    pub temperature: Option<f32>,  // Temperature sensor for this fan
    pub is_rpm: bool,              // true if rpm_or_percent is RPM, false if it's percentage
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WiFiInfo {
    pub interface: String,
    pub driver: String,
    pub network_controller: Option<String>,
    pub subsystem: Option<String>,
    pub driver_version: Option<String>,
    pub firmware_version: Option<String>,
    pub temperature: Option<f32>,
    pub signal_level: Option<i32>,      // Signal level in dBm
    pub channel: Option<u32>,           // Current channel
    pub channel_width: Option<u32>,     // Channel width in MHz (20/40/80/160)
    pub tx_rate: Option<f64>,           // Upload rate in Mbps
    pub rx_rate: Option<f64>,           // Download rate in Mbps
    pub ssid: Option<String>,
    pub data_rate: Option<f64>,         // Link speed in Mbps
    pub rx_bytes: Option<u64>,
    pub tx_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageDevice {
    pub device: String,
    pub model: String,
    pub size_gb: u64,
    pub temperature: Option<f32>,
    pub read_speed: Option<f64>,
    pub write_speed: Option<f64>,
    pub read_iops: Option<f64>,
    pub write_iops: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountInfo {
    pub mount_point: String,
    pub filesystem_type: String,
    pub total_gb: u64,
    pub used_gb: u64,
    pub used_percent: f64,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuSettings {
    pub governor: Option<String>,
    pub min_frequency: Option<u64>,
    pub max_frequency: Option<u64>,
    pub boost: Option<bool>,
    pub smt: Option<bool>,
    pub performance_profile: Option<String>,
    pub tdp_profile: Option<String>,              // ADD
    pub energy_performance_preference: Option<String>,  // ADD
    pub tdp: Option<u32>,
    pub amd_pstate_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuSettings {
    pub dgpu_tdp: Option<u32>,
    pub min_gpu_clock: Option<u32>,
    pub max_gpu_clock: Option<u32>,
    pub min_mem_clock: Option<u32>,
    pub max_mem_clock: Option<u32>,
    pub manual_clocks: bool,
    pub core_offset: Option<i32>,
    pub memory_offset: Option<i32>,
    pub prime_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardSettings {
    pub control_enabled: bool,
    pub mode: KeyboardMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeyboardMode {
    SingleColor { r: u8, g: u8, b: u8, brightness: u8 },  // CUSTOM (0) - Static color
    Breathe { r: u8, g: u8, b: u8, brightness: u8, speed: u8 },  // BREATHE (1)
    Cycle { brightness: u8, speed: u8 },  // CYCLE (2) - Color cycle through spectrum
    Dance { brightness: u8, speed: u8 },  // DANCE (3)
    Flash { r: u8, g: u8, b: u8, brightness: u8, speed: u8 },  // FLASH (4)
    RandomColor { brightness: u8, speed: u8 },  // RANDOM_COLOR (5)
    Tempo { brightness: u8, speed: u8 },  // TEMPO (6)
    Wave { brightness: u8, speed: u8 },  // WAVE (7)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenSettings {
    pub brightness: u8,
    pub system_control: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FanSettings {
    pub control_enabled: bool,
    pub curves: Vec<FanCurve>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatterySettings {
    pub control_enabled: bool,
    pub charge_start_threshold: u8,
    pub charge_end_threshold: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FanCurve {
    pub fan_id: u32,
    pub points: Vec<(u8, u8)>, // (temperature, speed) - 8 points
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub theme: Theme,
    pub start_minimized: bool,
    #[serde(default)]
    pub tray_enabled: bool,
    pub autostart: bool,
    pub fan_daemon_enabled: bool,
    pub app_monitoring_enabled: bool,
    pub cpu_scheduler: String,
    pub font_size: FontSize,
    pub statistics_sections: StatisticsSections,
    pub tuning_section_order: Vec<String>,
    pub profiles: Vec<Profile>,
    pub current_profile: String,
    pub battery_settings: BatterySettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FontSize {
    Small,
    Medium,
    Large,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Theme {
    Auto,
    Light,
    Dark,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatisticsSections {
    pub show_system_info: bool,
    pub show_cpu: bool,
    #[serde(default = "default_show_memory")]
    pub show_memory: bool,
    pub show_gpu: bool,
    pub show_battery: bool,
    pub show_wifi: bool,
    pub show_storage: bool,
    pub show_fans: bool,
    pub section_order: Vec<String>,
    // Polling rates in milliseconds
    pub cpu_poll_rate: u64,
    #[serde(default = "default_memory_poll_rate")]
    pub memory_poll_rate: u64,
    pub gpu_poll_rate: u64,
    pub battery_poll_rate: u64,
    pub wifi_poll_rate: u64,
    pub storage_poll_rate: u64,
    pub fans_poll_rate: u64,
}

fn default_memory_poll_rate() -> u64 {
    1000
}

fn default_show_memory() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: Theme::Auto,
            start_minimized: false,
            tray_enabled: false,
            autostart: false,
            fan_daemon_enabled: true,
            app_monitoring_enabled: true,
            cpu_scheduler: "CFS".to_string(),
            font_size: FontSize::Medium,
            statistics_sections: StatisticsSections::default(),
            tuning_section_order: vec![
                "Keyboard".to_string(),
                "CPU".to_string(),
                "GPU".to_string(),
                "Screen".to_string(),
                "Fans".to_string(),
            ],
            profiles: vec![Profile::default()],
            current_profile: "Standard".to_string(),
            battery_settings: BatterySettings::default(),
        }
    }
}

impl Default for BatterySettings {
    fn default() -> Self {
        Self {
            control_enabled: false,
            charge_start_threshold: 40,
            charge_end_threshold: 80,
        }
    }
}

impl Default for StatisticsSections {
    fn default() -> Self {
        Self {
            show_system_info: true,
            show_cpu: true,
            show_memory: true,
            show_gpu: true,
            show_battery: true,
            show_wifi: true,
            show_storage: true,
            show_fans: true,
            section_order: vec![
                "SystemInfo".to_string(),
                "CPU".to_string(),
                "Memory".to_string(),
                "GPU".to_string(),
                "Battery".to_string(),
                "WiFi".to_string(),
                "Storage".to_string(),
                "Fans".to_string(),
            ],
            cpu_poll_rate: 1000,            // 1 second
            memory_poll_rate: default_memory_poll_rate(),
            gpu_poll_rate: 2000,            // 2 seconds
            battery_poll_rate: 5000,        // 5 seconds
            wifi_poll_rate: 5000,           // 5 seconds
            storage_poll_rate: 5 * 1000,    // 5 seconds
            fans_poll_rate: 1000,           // 1 second
        }
    }
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            name: "Standard".to_string(),
            is_default: true,
            cpu_settings: CpuSettings::default(),
            gpu_settings: GpuSettings::default(),
            keyboard_settings: KeyboardSettings::default(),
            screen_settings: ScreenSettings::default(),
            fan_settings: FanSettings::default(),
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
            tdp_profile: None,                          // ADD
            energy_performance_preference: None,        // ADD
        }
    }
}

impl Default for GpuSettings {
    fn default() -> Self {
        Self {
            dgpu_tdp: None,
            min_gpu_clock: None,
            max_gpu_clock: None,
            min_mem_clock: None,
            max_mem_clock: None,
            manual_clocks: false,
            core_offset: Some(0),
            memory_offset: Some(0),
            prime_profile: Some("on-demand".to_string()),
        }
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
