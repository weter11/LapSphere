use serde::{Deserialize, Serialize};

/// System-level identification and kernel information.
///
/// Populated from `/sys/class/dmi/id/*` and `/proc/modules`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SystemInfo {
    /// Product name (e.g., "XPS 15 9520").
    pub product_name: String,
    /// Product SKU/serial.
    pub product_sku: String,
    /// Manufacturer name (e.g., "Dell Inc.").
    pub manufacturer: String,
    /// Motherboard/board name.
    pub board_name: String,
    /// BIOS/UEFI version string.
    pub bios_version: String,
    /// Loaded kernel modules as a single concatenated string.
    pub kernel_modules: String,
}

/// A single log line captured by the daemon for GUI display.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogEntry {
    /// Log level (trace, debug, info, warn, error).
    pub level: String,
    /// Crate/module target that emitted the log.
    pub target: String,
    /// Human-readable message.
    pub message: String,
    /// RFC3339 timestamp.
    pub timestamp: String,
}

/// Complete CPU telemetry snapshot.
///
/// Sourced from `/sys/devices/system/cpu/`, `/sys/class/thermal/`,
/// `/sys/class/powercap/`, and vendor-specific interfaces (amdgpu, zenpower, etc.).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CpuInfo {
    /// CPU model name (e.g., "AMD Ryzen 7 5800H").
    pub name: String,
    /// Average frequency across all cores in MHz.
    pub average_frequency: u64,
    /// Average load percentage across all cores (0.0–100.0).
    pub average_load: f32,
    /// Package temperature in °C.
    pub package_temp: f32,
    /// Total package power in watts (if supported by hardware).
    pub package_power: Option<f32>,
    /// Human-readable power source description (e.g., "Intel RAPL").
    pub power_source: Option<String>,
    /// All detected power sources with individual readings.
    pub all_power_sources: Vec<PowerSource>,
    /// Per-core breakdown.
    pub cores: Vec<CoreInfo>,
    /// Physical core count.
    pub physical_cores: u32,
    /// Logical core count (threads).
    pub logical_cores: u32,
    /// Active governor (e.g., "powersave", "performance").
    pub governor: String,
    /// List of available governors.
    pub available_governors: Vec<String>,
    /// Whether boost/turbo is currently enabled.
    pub boost_enabled: bool,
    /// Whether SMT/hyperthreading is enabled.
    pub smt_enabled: bool,
    /// Kernel scaling driver (e.g., "amd-pstate", "intel_pstate", "acpi-cpufreq").
    pub scaling_driver: String,
    /// AMD P-State driver status if applicable.
    pub amd_pstate_status: Option<String>,
    /// Intel P-State driver status if applicable.
    pub intel_pstate_status: Option<String>,
    /// Minimum frequency in MHz (from scaling_min_freq).
    pub min_freq: Option<u64>,
    /// Maximum frequency in MHz (from scaling_max_freq).
    pub max_freq: Option<u64>,
    /// Hardware minimum frequency in MHz.
    pub hw_min_freq: Option<u64>,
    /// Hardware maximum frequency in MHz.
    pub hw_max_freq: Option<u64>,
    /// Active Energy Performance Preference (e.g., "balance_performance").
    pub energy_performance_preference: Option<String>,
    /// Available EPP options.
    pub available_epp_options: Vec<String>,
    /// Active I/O scheduler.
    pub scheduler: String,
    /// Available I/O schedulers.
    pub available_schedulers: Vec<String>,
    /// TDP level 0 (base) in watts.
    pub tdp0: Option<u32>,
    /// TDP level 1 in watts.
    pub tdp1: Option<u32>,
    /// TDP level 2 in watts.
    pub tdp2: Option<u32>,
    /// TDP0 configurable range (min, max) in watts.
    pub tdp0_range: Option<(u32, u32)>,
    /// TDP1 configurable range (min, max) in watts.
    pub tdp1_range: Option<(u32, u32)>,
    /// TDP2 configurable range (min, max) in watts.
    pub tdp2_range: Option<(u32, u32)>,
    /// CPU capability flags.
    pub capabilities: CpuCapabilities,
}

/// CPU feature / capability flags.
///
/// Indicates which hardware interfaces and knobs are present on this system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CpuCapabilities {
    /// Boost/turbo capability exists.
    pub has_boost: bool,
    /// cpuinfo_max_freq is readable.
    pub has_cpuinfo_max_freq: bool,
    /// cpuinfo_min_freq is readable.
    pub has_cpuinfo_min_freq: bool,
    /// scaling_driver is set.
    pub has_scaling_driver: bool,
    /// energy_performance_preference is writable.
    pub has_energy_performance_preference: bool,
    /// scaling_governor is writable.
    pub has_scaling_governor: bool,
    /// SMT control exists.
    pub has_smt: bool,
    /// scaling_min_freq is writable.
    pub has_scaling_min_freq: bool,
    /// scaling_max_freq is writable.
    pub has_scaling_max_freq: bool,
    /// available_governors is non-empty.
    pub has_available_governors: bool,
    /// AMD P-State driver is active.
    pub has_amd_pstate: bool,
    /// Intel P-State driver is active.
    pub has_intel_pstate: bool,
}

/// A single power-domain reading (RAPL, AMD GPU, zenpower, etc.).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PowerSource {
    /// Short source identifier (e.g., "RAPL", "amdgpu", "zenpower").
    pub name: String,
    /// Measured power in watts.
    pub value: f32,
    /// Human-readable description (e.g., "Intel RAPL", "AMD APU (CPU+iGPU)").
    pub description: String,
}

/// Per-core frequency, load, and temperature.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CoreInfo {
    /// Logical core ID.
    pub id: u32,
    /// Current frequency in MHz.
    pub frequency: u64,
    /// Load percentage (0.0–100.0).
    pub load: f32,
    /// Temperature in °C (if per-core sensor exists).
    pub temperature: f32,
}

/// System memory (RAM) usage and specifications.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryInfo {
    /// Total physical memory in GiB.
    pub total_gib: f64,
    /// Currently used memory in GiB.
    pub used_gib: f64,
    /// Free memory in GiB.
    pub free_gib: f64,
    /// Available memory (free + reclaimable) in GiB.
    pub available_gib: f64,
    /// Used percentage (0.0–100.0).
    pub used_percent: f32,
    /// Memory technology (e.g., "DDR5", "LPDDR4X") if detectable.
    pub memory_type: Option<String>,
    /// Memory frequency in MHz if detectable.
    pub memory_frequency: Option<u64>,
}

/// GPU (integrated or discrete) telemetry and capabilities.
///
/// Populated via NVML (NVIDIA), amdgpu sysfs, or Intel GPU sysfs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GpuInfo {
    /// Device name (e.g., "NVIDIA GeForce RTX 3070 Laptop GPU").
    pub name: String,
    /// Integrated vs discrete classification.
    pub gpu_type: GpuType,
    /// Driver-reported status string.
    pub status: String,
    /// Current core clock in MHz.
    pub frequency: Option<u64>,
    /// Current memory clock in MHz.
    pub memory_frequency: Option<u64>,
    /// Core temperature in °C.
    pub temperature: Option<f32>,
    /// Junction/hotspot temperature in °C (NVIDIA/AMD).
    pub hotspot_temperature: Option<f32>,
    /// VRAM temperature in °C.
    pub memory_temperature: Option<f32>,
    /// GPU compute load percentage (0.0–100.0).
    pub load: Option<f32>,
    /// Current power draw in watts.
    pub power: Option<f32>,
    /// Core voltage in volts.
    pub voltage: Option<f32>,
    /// Applied core frequency offset in MHz.
    pub freq_offset: Option<i32>,
    /// Applied memory frequency offset in MHz.
    pub drain_offset: Option<i32>,
    /// Applied power limit offset in watts.
    pub power_offset: Option<i32>,
    /// Combined offset (vendor-specific meaning).
    pub total_offset: Option<i32>,
    /// Minimum supported core clock in MHz.
    pub min_core_clock: Option<u32>,
    /// Maximum supported core clock in MHz.
    pub max_core_clock: Option<u32>,
    /// Minimum supported memory clock in MHz.
    pub min_memory_clock: Option<u32>,
    /// Maximum supported memory clock in MHz.
    pub max_memory_clock: Option<u32>,
    /// Allowed core clock range (min, max) in MHz.
    pub core_clock_range: Option<(u32, u32)>,
    /// Allowed memory clock range (min, max) in MHz.
    pub memory_clock_range: Option<(u32, u32)>,
    /// Allowed core offset range (min, max) in MHz.
    pub core_offset_limits: Option<(i32, i32)>,
    /// Allowed memory offset range (min, max) in MHz.
    pub memory_offset_limits: Option<(i32, i32)>,
    /// True if this is a desktop (not laptop) GPU.
    pub is_desktop: bool,
    /// Architecture string (e.g., "Ampere", "RDNA 2").
    pub architecture: Option<String>,
    /// NVML device index (NVIDIA only).
    pub nvml_index: Option<u32>,
    /// Driver version string.
    pub driver_version: Option<String>,
    /// Supported performance states.
    pub supported_p_states: Vec<String>,
    /// Whether power limit control is supported.
    pub supports_power_limit: bool,
    /// Power limit range (min, max) in watts.
    pub power_limit_range: Option<(u32, u32)>,
    /// Whether GPU core offset control is supported.
    pub supports_gpu_offset: bool,
    /// Whether memory offset control is supported.
    pub supports_mem_offset: bool,
    /// Fan speed range (min, max) in percent.
    pub fan_speed_range: Option<(u32, u32)>,
    /// VRAM type (e.g., "GDDR6", "HBM2").
    pub vram_type: Option<String>,
    /// VRAM vendor (e.g., "Samsung", "Micron").
    pub vram_vendor: Option<String>,
    /// VRAM bus width in bits.
    pub vram_bus_width: Option<u32>,
    /// VRAM bandwidth in GB/s.
    pub vram_bandwidth: Option<f32>,
    /// Total VRAM in MiB.
    pub vram_total: Option<u64>,
}

/// GPU classification: integrated (iGPU) or discrete (dGPU).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GpuType {
    /// Integrated graphics (shared system memory).
    Integrated,
    /// Discrete graphics (dedicated VRAM).
    Discrete,
}

/// Battery status, health, and charging thresholds.
///
/// Sourced from `/sys/class/power_supply/BAT*/` and vendor-specific
/// charge-control interfaces (e.g., `/sys/class/power_supply/BAT0/charge_control_*`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BatteryInfo {
    /// Charging state: "Charging", "Discharging", "Full", "Unknown".
    pub status: String,
    /// Voltage in millivolts.
    pub voltage_mv: u64,
    /// Current in milliamps (negative = discharging).
    pub current_ma: i64,
    /// Charge percentage (0–100).
    pub charge_percent: u64,
    /// Design capacity in mAh.
    pub capacity_mah: u64,
    /// Battery health as percentage of design capacity (0.0–100.0).
    pub battery_health: Option<f32>,
    /// Manufacturer name.
    pub manufacturer: String,
    /// Model/part number.
    pub model: String,
    /// Charge start threshold (%) — battery won't charge below this.
    pub charge_start_threshold: Option<u8>,
    /// Charge stop threshold (%) — battery stops charging at this.
    pub charge_end_threshold: Option<u8>,
}

/// Single fan RPM/percentage reading and associated temperature sensor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FanInfo {
    /// Fan index (matches hwmon/fan*_input).
    pub id: u32,
    /// Fan label (e.g., "cpu_fan", "gpu_fan").
    pub name: String,
    /// Current speed — RPM if `is_rpm=true`, otherwise percentage (0–100).
    pub rpm_or_percent: u32,
    /// Temperature sensor this fan targets (if mapped).
    pub temperature: Option<f32>,
    /// True if `rpm_or_percent` is RPM; false if it's a PWM percentage.
    pub is_rpm: bool,
    /// Control mode if reported by hardware ("Auto", "Manual", etc.).
    pub mode: Option<String>,
}

/// WiFi interface status, link quality, and throughput.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WiFiInfo {
    /// Interface name (e.g., "wlan0").
    pub interface: String,
    /// Kernel driver name (e.g., "iwlwifi", "ath9k").
    pub driver: String,
    /// Driver version string.
    pub driver_version: Option<String>,
    /// Firmware version string.
    pub firmware_version: Option<String>,
    /// Radio temperature in °C (if supported).
    pub temperature: Option<f32>,
    /// Signal level in dBm (negative, closer to 0 = stronger).
    pub signal_level: Option<i32>,
    /// Current channel number.
    pub channel: Option<u32>,
    /// Channel width in MHz (20, 40, 80, 160).
    pub channel_width: Option<u32>,
    /// Channel center frequency in MHz.
    pub channel_freq: Option<u32>,
    /// Actual TX throughput in Mbps.
    pub tx_rate: Option<f64>,
    /// Actual RX throughput in Mbps.
    pub rx_rate: Option<f64>,
    /// Connected SSID.
    pub ssid: Option<String>,
    /// PHY link speed (TX) in Mbps.
    pub tx_bitrate: Option<f64>,
    /// PHY link speed (RX) in Mbps.
    pub rx_bitrate: Option<f64>,
    /// Total RX bytes since interface up.
    pub rx_bytes: Option<u64>,
    /// Total TX bytes since interface up.
    pub tx_bytes: Option<u64>,
    /// Network controller hardware name.
    pub network_controller: Option<String>,
    /// Subsystem identifier.
    pub subsystem: Option<String>,
}

/// Block device (NVMe, SATA, etc.) capacity and I/O stats.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StorageDevice {
    /// Device path (e.g., "/dev/nvme0n1").
    pub device: String,
    /// Model string from identify.
    pub model: String,
    /// Total size in GB.
    pub size_gb: u64,
    /// Temperature in °C (if SMART/NVMe temp supported).
    pub temperature: Option<f32>,
    /// Current read speed in MB/s.
    pub read_speed: Option<f64>,
    /// Current write speed in MB/s.
    pub write_speed: Option<f64>,
    /// Current read IOPS.
    pub read_iops: Option<f64>,
    /// Current write IOPS.
    pub write_iops: Option<f64>,
}

/// Gamepad/controller connection state and capabilities.
///
/// Populated from udev + evdev + upower/udev battery info.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct GamepadInfo {
    /// Product name (e.g., "Xbox Wireless Controller").
    #[serde(default)]
    pub name: String,
    /// Dynamic kernel input device ID (e.g., "input0").
    #[serde(default)]
    pub id: String,
    /// Stable unique identifier from udev (ID_PATH).
    #[serde(default)]
    pub uid: String,
    /// Connection status.
    #[serde(default)]
    pub status: GamepadStatus,
    /// Battery level 0–100 (if wireless + upower).
    #[serde(default)]
    pub battery_level: Option<u8>,
    /// Wired / wireless / unknown.
    #[serde(default)]
    pub connection_type: ConnectionType,
    /// Power/charging state.
    #[serde(default)]
    pub power_status: PowerStatus,
}

/// Gamepad connection state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum GamepadStatus {
    /// Connected and responding.
    Connected,
    /// Not connected (default).
    #[default]
    Disconnected,
}

/// Physical transport type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum ConnectionType {
    /// USB / wired.
    Wired,
    /// Bluetooth / 2.4 GHz dongle.
    Wireless,
    /// Could not be determined (default).
    #[default]
    Unknown,
}

/// Battery/charging state for wireless gamepads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum PowerStatus {
    /// Currently charging.
    Charging,
    /// Discharging on battery.
    Discharging,
    /// Plugged in and full.
    Full,
    /// Unknown (default).
    #[default]
    Unknown,
}

/// Filesystem mount point usage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MountInfo {
    /// Mount point path (e.g., "/", "/home").
    pub mount_point: String,
    /// Filesystem type (e.g., "ext4", "btrfs", "vfat").
    pub filesystem_type: String,
    /// Total space in GB.
    pub total_gb: u64,
    /// Used space in GB.
    pub used_gb: u64,
    /// Used percentage (0.0–100.0).
    pub used_percent: f64,
}

/// User-defined hardware profile (CPU, GPU, keyboard, screen, fans).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    /// Profile display name.
    pub name: String,
    /// Whether this is the default profile.
    pub is_default: bool,
    /// CPU-related settings.
    pub cpu_settings: CpuSettings,
    /// GPU-related settings.
    pub gpu_settings: GpuSettings,
    /// Keyboard backlight/effects settings.
    pub keyboard_settings: KeyboardSettings,
    /// Screen brightness/control settings.
    pub screen_settings: ScreenSettings,
    /// Fan curve settings.
    pub fan_settings: FanSettings,
}

/// CPU tuning knobs (governor, frequencies, boost, SMT, TDP, EPP).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CpuSettings {
    /// Governor to set (e.g., "powersave", "performance").
    pub governor: Option<String>,
    /// Minimum frequency in MHz.
    pub min_frequency: Option<u64>,
    /// Maximum frequency in MHz.
    pub max_frequency: Option<u64>,
    /// Boost/turbo enable.
    pub boost: Option<bool>,
    /// SMT/hyperthreading enable.
    pub smt: Option<bool>,
    /// AMD performance profile (e.g., "low", "high").
    pub performance_profile: Option<String>,
    /// TDP profile name (vendor-specific).
    pub tdp_profile: Option<String>,
    /// Energy Performance Preference (e.g., "balance_performance").
    pub energy_performance_preference: Option<String>,
    /// Legacy single TDP value in watts.
    pub tdp: Option<u32>,
    /// TDP level 0 in watts.
    pub tdp0: Option<u32>,
    /// TDP level 1 in watts.
    pub tdp1: Option<u32>,
    /// TDP level 2 in watts.
    pub tdp2: Option<u32>,
    /// AMD P-State status string.
    pub amd_pstate_status: Option<String>,
    /// Intel P-State status string.
    pub intel_pstate_status: Option<String>,
}

/// GPU tuning knobs (clocks, offsets, power limit, fan curves).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GpuSettings {
    /// Discrete GPU TDP limit in watts.
    pub dgpu_tdp: Option<u32>,
    /// Minimum core clock in MHz.
    pub min_gpu_clock: Option<u32>,
    /// Maximum core clock in MHz.
    pub max_gpu_clock: Option<u32>,
    /// Minimum memory clock in MHz.
    pub min_mem_clock: Option<u32>,
    /// Maximum memory clock in MHz.
    pub max_mem_clock: Option<u32>,
    /// Whether manual clock control is active.
    pub manual_clocks: bool,
    /// Core frequency offset in MHz.
    pub core_offset: Option<f32>,
    /// Memory frequency offset in MHz.
    pub memory_offset: Option<f32>,
    /// Power limit in watts.
    pub power_limit: Option<u32>,
    /// NVIDIA Prime profile (e.g., "on-demand", "performance").
    pub prime_profile: Option<String>,
    /// Whether advanced control panel is enabled.
    #[serde(default)]
    pub advanced_control: bool,
    /// Advanced tuning parameters.
    #[serde(default)]
    pub advanced: GpuAdvancedSettings,
    /// Advanced min core clock (overrides basic).
    #[serde(default)]
    pub advanced_min_gpu_clock: Option<u32>,
    /// Advanced max core clock (overrides basic).
    #[serde(default)]
    pub advanced_max_gpu_clock: Option<u32>,
    /// Advanced min memory clock (overrides basic).
    #[serde(default)]
    pub advanced_min_mem_clock: Option<u32>,
    /// Advanced max memory clock (overrides basic).
    #[serde(default)]
    pub advanced_max_mem_clock: Option<u32>,
    /// Advanced memory offset in MHz.
    #[serde(default)]
    pub advanced_memory_offset: Option<i32>,
    /// Per-fan manual settings (NVIDIA).
    #[serde(default)]
    pub nvidia_fans: Vec<NvidiaFanSettings>,
}

/// Per-fan manual speed for NVIDIA GPUs.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct NvidiaFanSettings {
    /// NVML device index.
    pub device_index: u32,
    /// Fan index on that device.
    pub fan_id: u32,
    /// Target speed in percent (0–100).
    pub speed: u32,
    /// Whether manual control is active for this fan.
    pub manual: bool,
}

/// Extended GPU tuning bounds and feature flags.
///
/// These define the allowed ranges for advanced offsets and control.
/// Values outside these ranges will be clamped by the daemon.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GpuAdvancedSettings {
    /// Allowed temperature target range (°C).
    pub temperature_min: i32,
    pub temperature_max: i32,
    /// Allowed power limit range (watts).
    pub plimit_min: i32,
    pub plimit_max: i32,
    /// Allowed frequency range (MHz).
    pub frequency_min: i32,
    pub frequency_max: i32,
    /// Allowed core offset range (MHz).
    pub freq_offset_max: i32,
    pub freq_offset_min: i32,
    /// Low-frequency mode bounds (MHz).
    pub low_freq_min: i32,
    pub low_freq_max: i32,
    /// Low-frequency drain offset bounds.
    pub drain_offset_lmin: i32,
    pub drain_offset_lmax: i32,
    /// High-frequency mode bounds (MHz).
    pub high_freq_min: i32,
    pub high_freq_max: i32,
    /// High-frequency drain offset bounds.
    pub drain_offset_hmin: i32,
    pub drain_offset_hmax: i32,
    /// Critical temperature bounds (°C).
    pub critical_temp_min: i32,
    pub critical_temp_max: i32,
    /// Power offset bounds (watts).
    pub power_offset_max: i32,
    pub power_offset_min: i32,
    /// Whether drain offset control is enabled.
    #[serde(default)]
    pub drain_offset_control: bool,
    /// Whether power offset control is enabled.
    #[serde(default)]
    pub power_offset_control: bool,
    /// Whether critical temp range control is enabled.
    #[serde(default)]
    pub critical_temp_range_control: bool,
    /// Smart rounding threshold for frequency steps.
    #[serde(default = "default_smart_rounding_threshold")]
    pub smart_rounding_threshold: i32,
}

fn default_smart_rounding_threshold() -> i32 {
    15
}

/// Keyboard backlight control settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeyboardSettings {
    /// Whether backlight control is enabled by the daemon.
    pub control_enabled: bool,
    /// Currently selected backlight mode/effect.
    pub mode: KeyboardMode,
}

/// Keyboard backlight type classification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum KeyboardType {
    /// No backlight.
    None,
    /// Single-color white backlight.
    WhiteOnly,
    /// Single-zone RGB (entire keyboard one color).
    SingleZoneRGB,
    /// Three-zone RGB (left, center, right).
    ThreeZoneRGB,
    /// Four-zone RGB.
    FourZoneRGB,
    /// Per-key RGB.
    PerKeyRGB,
}

/// Hardware keyboard capabilities reported by the daemon.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KeyboardCapabilities {
    /// Backlight type.
    pub keyboard_type: KeyboardType,
    /// Brightness control supported.
    pub supports_brightness: bool,
    /// Color control supported.
    pub supports_color: bool,
    /// Effects (breathe, cycle, etc.) supported.
    pub supports_effects: bool,
    /// Number of addressable zones.
    pub num_zones: u32,
}

/// Single RGB zone color.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ZoneColor {
    /// Red channel (0–255).
    pub r: u8,
    /// Green channel (0–255).
    pub g: u8,
    /// Blue channel (0–255).
    pub b: u8,
}

/// Keyboard backlight mode / effect.
///
/// Variants correspond to the daemon's effect IDs (CUSTOM=0, BREATHE=1, CYCLE=2, DANCE=3, FLASH=4, RANDOM_COLOR=5, TEMPO=6, WAVE=7).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum KeyboardMode {
    /// Static single color (CUSTOM / effect 0).
    SingleColor {
        r: u8,
        g: u8,
        b: u8,
        brightness: u8,
    },
    /// Multiple independent zones (effect varies by zone count).
    MultipleZones {
        zones: Vec<ZoneColor>,
        brightness: u8,
    },
    /// Per-key RGB map.
    PerKeyRGB {
        keys: Vec<ZoneColor>,
        brightness: u8,
    },
    /// Breathing fade in/out (BREATHE / effect 1).
    Breathe {
        r: u8,
        g: u8,
        b: u8,
        brightness: u8,
        speed: u8,
    },
    /// Color cycle through spectrum (CYCLE / effect 2).
    Cycle {
        brightness: u8,
        speed: u8,
    },
    /// Dance/random pattern (DANCE / effect 3).
    Dance {
        brightness: u8,
        speed: u8,
    },
    /// Strobe flash (FLASH / effect 4).
    Flash {
        r: u8,
        g: u8,
        b: u8,
        brightness: u8,
        speed: u8,
    },
    /// Random color per cycle (RANDOM_COLOR / effect 5).
    RandomColor {
        brightness: u8,
        speed: u8,
    },
    /// Tempo/reactive (TEMPO / effect 6).
    Tempo {
        brightness: u8,
        speed: u8,
    },
    /// Wave propagation (WAVE / effect 7).
    Wave {
        brightness: u8,
        speed: u8,
    },
}

/// Screen brightness and auto-control settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScreenSettings {
    /// Backlight brightness 0–255.
    pub brightness: u8,
    /// Whether the daemon should manage brightness automatically.
    pub system_control: bool,
}

/// Fan curve configuration per fan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FanSettings {
    /// Whether fan control is enabled.
    pub control_enabled: bool,
    /// Per-fan temperature→speed curves (8 points each).
    pub curves: Vec<FanCurve>,
}

/// Battery charge threshold settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BatterySettings {
    /// Whether charge threshold control is enabled.
    pub control_enabled: bool,
    /// Start charging when below this percentage (0–100).
    pub charge_start_threshold: u8,
    /// Stop charging when above this percentage (0–100).
    pub charge_end_threshold: u8,
}

/// Single fan temperature→speed curve (8 (temp, speed) points).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FanCurve {
    /// Fan index this curve applies to.
    pub fan_id: u32,
    /// 8 (temperature°C, speed%) control points.
    pub points: Vec<(u8, u8)>,
}

/// Full application configuration (persisted to disk).
///
/// Contains all user preferences: theme, profiles, polling rates,
/// UI sections, and remembered devices.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppConfig {
    /// UI theme preference.
    pub theme: Theme,
    /// Start minimized to tray.
    pub start_minimized: bool,
    /// Enable system tray icon.
    #[serde(default)]
    pub tray_enabled: bool,
    /// Launch at desktop login (systemd --user enable).
    pub autostart: bool,
    /// CPU scheduler name for display (e.g., "CFS", "EEVDF").
    pub cpu_scheduler: String,
    /// UI font size.
    pub font_size: FontSize,
    /// Which statistics sections to show and their polling rates.
    pub statistics_sections: StatisticsSections,
    /// Order of tuning sections in the GUI.
    pub tuning_section_order: Vec<String>,
    /// All saved hardware profiles.
    pub profiles: Vec<Profile>,
    /// Currently active profile name.
    pub current_profile: String,
    /// Battery charge thresholds.
    pub battery_settings: BatterySettings,
    /// Maximum log lines retained in the GUI.
    #[serde(default = "default_log_limit")]
    pub log_limit: usize,
    /// Enable trace-level logging in the GUI.
    #[serde(default = "default_log_filter_trace")]
    pub log_filter_trace: bool,
    /// Previously paired gamepads (for auto-reconnect).
    #[serde(default)]
    pub remembered_gamepads: Vec<GamepadInfo>,
}

fn default_log_limit() -> usize {
    100
}

fn default_log_filter_trace() -> bool {
    false
}

/// UI font size preset.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FontSize {
    /// Small text.
    Small,
    /// Medium text (default).
    Medium,
    /// Large text.
    Large,
}

/// Application color theme.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Theme {
    /// Follow system/GTK theme.
    Auto,
    /// Force light mode.
    Light,
    /// Force dark mode.
    Dark,
}

/// Which statistics sections to display and their polling intervals.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatisticsSections {
    /// Show system info section.
    pub show_system_info: bool,
    /// Show CPU section.
    pub show_cpu: bool,
    /// Show memory section.
    #[serde(default = "default_show_memory")]
    pub show_memory: bool,
    /// Show GPU section.
    pub show_gpu: bool,
    /// Show battery section.
    pub show_battery: bool,
    /// Show WiFi section.
    pub show_wifi: bool,
    /// Show storage section.
    pub show_storage: bool,
    /// Show fans section.
    pub show_fans: bool,
    /// Show gamepads section.
    #[serde(default = "default_show_gamepads")]
    pub show_gamepads: bool,
    /// Display order of sections.
    pub section_order: Vec<String>,
    /// CPU polling rate in milliseconds.
    pub cpu_poll_rate: u64,
    /// Memory polling rate in milliseconds.
    #[serde(default = "default_memory_poll_rate")]
    pub memory_poll_rate: u64,
    /// GPU polling rate in milliseconds.
    pub gpu_poll_rate: u64,
    /// Battery polling rate in milliseconds.
    pub battery_poll_rate: u64,
    /// WiFi polling rate in milliseconds.
    pub wifi_poll_rate: u64,
    /// Storage polling rate in milliseconds.
    pub storage_poll_rate: u64,
    /// Fans polling rate in milliseconds.
    pub fans_poll_rate: u64,
    /// Gamepad polling rate in milliseconds.
    #[serde(default = "default_gamepad_poll_rate")]
    pub gamepad_poll_rate: u64,
    /// GPU overclock polling rate in milliseconds.
    #[serde(default = "default_gpu_overclock_poll_rate")]
    pub gpu_overclock_poll_rate: u64,
}

fn default_gpu_overclock_poll_rate() -> u64 {
    1000
}

fn default_memory_poll_rate() -> u64 {
    1000
}

fn default_gamepad_poll_rate() -> u64 {
    5000
}

fn default_show_memory() -> bool {
    true
}

fn default_show_gamepads() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: Theme::Auto,
            start_minimized: false,
            tray_enabled: false,
            autostart: false,
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
            log_limit: default_log_limit(),
            log_filter_trace: default_log_filter_trace(),
            remembered_gamepads: vec![],
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
            show_gamepads: true,
            section_order: vec![
                "SystemInfo".to_string(),
                "CPU".to_string(),
                "Memory".to_string(),
                "GPU".to_string(),
                "Battery".to_string(),
                "WiFi".to_string(),
                "Storage".to_string(),
                "Fans".to_string(),
                "Gamepads".to_string(),
            ],
            cpu_poll_rate: 1000,            // 1 second
            memory_poll_rate: default_memory_poll_rate(),
            gpu_poll_rate: 2000,            // 2 seconds
            battery_poll_rate: 5000,        // 5 seconds
            wifi_poll_rate: 5000,           // 5 seconds
            storage_poll_rate: 5 * 1000,    // 5 seconds
            fans_poll_rate: 1000,           // 1 second
            gamepad_poll_rate: 5000,        // 5 seconds
            gpu_overclock_poll_rate: 1000,
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
            intel_pstate_status: None,
            tdp_profile: None,
            energy_performance_preference: None,
            tdp0: None,
            tdp1: None,
            tdp2: None,
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
            core_offset: Some(0.0),
            memory_offset: Some(0.0),
            power_limit: None,
            prime_profile: Some("on-demand".to_string()),
            advanced_control: false,
            advanced: GpuAdvancedSettings::default(),
            advanced_min_gpu_clock: None,
            advanced_max_gpu_clock: None,
            advanced_min_mem_clock: None,
            advanced_max_mem_clock: None,
            advanced_memory_offset: Some(0),
            nvidia_fans: vec![],
        }
    }
}

impl Default for GpuAdvancedSettings {
    fn default() -> Self {
        Self {
            temperature_min: 20,
            temperature_max: 80,
            plimit_min: 20,
            plimit_max: 120,
            frequency_min: 900,
            frequency_max: 1800,
            freq_offset_max: 300,
            freq_offset_min: 150,
            low_freq_min: 1000,
            low_freq_max: 1440,
            drain_offset_lmin: -30,
            drain_offset_lmax: 0,
            high_freq_min: 1440,
            high_freq_max: 1800,
            drain_offset_hmin: 0,
            drain_offset_hmax: 15,
            critical_temp_min: 48,
            critical_temp_max: 61,
            power_offset_max: 35,
            power_offset_min: 0,
            drain_offset_control: false,
            power_offset_control: false,
            critical_temp_range_control: false,
            smart_rounding_threshold: 15,
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
