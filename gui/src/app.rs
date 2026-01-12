use chrono::Local;
use egui::{Context, CentralPanel, TopBottomPanel, RichText};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use tuxedo_common::types::*;

use crate::dbus_client::DbusClient;
use crate::theme::TuxedoTheme;
use crate::pages::{statistics, profiles, tuning, settings};
use crate::keyboard_shortcuts::KeyboardShortcuts;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Page {
    Statistics,
    Profiles,
    Tuning,
    Settings,
}

pub struct AppState {
    // Core data
    pub config: AppConfig,
    
    // Hardware info (updated in background)
    pub system_info: Option<SystemInfo>,
    pub memory_info: Option<MemoryInfo>,
    pub cpu_info: Option<CpuInfo>,
    pub gpu_info: Vec<GpuInfo>,
    pub battery_info: Option<BatteryInfo>,
    pub wifi_info: Vec<WiFiInfo>,
    pub fan_info: Vec<FanInfo>,
    pub storage_device_info: Vec<StorageDevice>,
    pub mount_info: Vec<MountInfo>,
    pub gpu_clock_ranges: Option<(u32, u32)>,
    pub gpu_mem_clock_ranges: Option<(u32, u32)>,
    pub gpu_core_offset_limits: Option<(i32, i32)>,
    pub gpu_mem_offset_limits: Option<(i32, i32)>,
    pub available_start_thresholds: Vec<u8>,
    pub available_end_thresholds: Vec<u8>,
    
    // UI state
    pub current_page: Page,
    pub status_message: Option<StatusMessage>,
    pub restart_confirmation_pending: bool,
    
    // Profile editing
    pub editing_profile_index: Option<usize>,
    pub editing_profile_name: Option<String>,
    
    // Async state
    pub pending_battery_update: Option<oneshot::Receiver<Result<(), anyhow::Error>>>,
}

#[derive(Debug, Clone)]
pub struct StatusMessage {
    pub text: String,
    pub is_error: bool,
    pub shown_at: Instant,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            config: AppConfig::default(),
            system_info: None,
            memory_info: None,
            cpu_info: None,
            gpu_info: Vec::new(),
            battery_info: None,
            wifi_info: Vec::new(),
            fan_info: Vec::new(),
            storage_device_info: Vec::new(),
            mount_info: Vec::new(),
            gpu_clock_ranges: None,
            gpu_mem_clock_ranges: None,
            gpu_core_offset_limits: None,
            gpu_mem_offset_limits: None,
            available_start_thresholds: Vec::new(),
            available_end_thresholds: Vec::new(),
            current_page: Page::Statistics,
            status_message: None,
            restart_confirmation_pending: false,
            editing_profile_index: None,
            editing_profile_name: None,
            pending_battery_update: None,
        }
    }
    
pub fn load_config(&mut self) {
    if let Ok(config) = load_config_from_disk() {
        self.config = config;
    }
}
    
    pub fn save_config(&mut self) -> anyhow::Result<()> {
        save_config_to_disk(&self.config)?;
        self.show_message("Configuration saved", false);
        Ok(())
    }
    
    pub fn show_message(&mut self, text: impl Into<String>, is_error: bool) {
        self.status_message = Some(StatusMessage {
            text: text.into(),
            is_error,
            shown_at: Instant::now(),
        });
    }
    
    pub fn current_profile(&self) -> Option<&Profile> {
        self.config.profiles.iter()
            .find(|p| p.name == self.config.current_profile)
    }
    
    pub fn current_profile_mut(&mut self) -> Option<&mut Profile> {
        let current = self.config.current_profile.clone();
        self.config.profiles.iter_mut()
            .find(|p| p.name == current)
    }
    
    pub fn current_profile_index(&self) -> Option<usize> {
        self.config.profiles.iter()
            .position(|p| p.name == self.config.current_profile)
    }
}

pub struct TuxedoApp {
    state: AppState,
    dbus_client: Option<DbusClient>,
    theme: TuxedoTheme,
    
    // Background update channel
    hw_update_tx: mpsc::UnboundedSender<HardwareUpdate>,
    hw_update_rx: mpsc::UnboundedReceiver<HardwareUpdate>,
    
    // Keyboard shortcuts
    shortcuts: KeyboardShortcuts,
}

#[derive(Debug)]
pub enum HardwareUpdate {
    SystemInfo(SystemInfo),
    MemoryInfo(MemoryInfo),
    CpuInfo(CpuInfo),
    GpuInfo(Vec<GpuInfo>),
    BatteryInfo(BatteryInfo),
    WifiInfo(Vec<WiFiInfo>),
    FanInfo(Vec<FanInfo>),
    StorageDeviceInfo(Vec<StorageDevice>),
    MountInfo(Vec<MountInfo>),
    GpuClockRanges(Result<(u32, u32), String>),
    GpuMemClockRanges(Result<Vec<u32>, String>),
    GpuCoreOffsetLimits(Result<(i32, i32), String>),
    GpuMemOffsetLimits(Result<(i32, i32), String>),
    AvailableThresholds(Vec<u8>, Vec<u8>),
    Error(String),
}

impl TuxedoApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut state = AppState::new();
        state.load_config();
        
        // Create DBus client
        let dbus_client = match DbusClient::new() {
            Ok(client) => {
                log::info!("✅ Connected to TUXEDO daemon");
                Some(client)
            }
            Err(e) => {
                log::error!("❌ Failed to connect to daemon: {}", e);
                state.show_message(
                    format!("Failed to connect to daemon: {}", e),
                    true
                );
                None
            }
        };
        
        // Setup background polling
        let (hw_update_tx, hw_update_rx) = mpsc::unbounded_channel();
        if let Some(ref client) = dbus_client {
            start_background_polling(client.clone(), hw_update_tx.clone(), &state.config);

            // Initial system info load
            let client_clone = client.clone();
            let tx_clone = hw_update_tx.clone();
            tokio::spawn(async move {
                if let Ok(Ok(info)) = client_clone.get_system_info().await {
                    let _ = tx_clone.send(HardwareUpdate::SystemInfo(info));
                }
            });

            // Fetch available thresholds
            let client_clone = client.clone();
            let tx_clone = hw_update_tx.clone();
            tokio::spawn(async move {
                let start_rx = client_clone.get_battery_available_start_thresholds();
                let end_rx = client_clone.get_battery_available_end_thresholds();

                match (start_rx.await, end_rx.await) {
                    (Ok(Ok(start)), Ok(Ok(end))) => {
                        let _ = tx_clone.send(HardwareUpdate::AvailableThresholds(start, end));
                    }
                    _ => {}
                }
            });
        }
        
        // Apply theme
        let theme = TuxedoTheme::new(&state.config.theme);
        theme.apply_with_font_size(&cc.egui_ctx, &state.config.font_size);
        
        Self {
            state,
            dbus_client,
            theme,
            hw_update_tx,
            hw_update_rx,
            shortcuts: KeyboardShortcuts::new(),
        }
    }
    
    fn handle_hardware_updates(&mut self) {
        // Process all pending updates (non-blocking)
        while let Ok(update) = self.hw_update_rx.try_recv() {
            match update {
                HardwareUpdate::SystemInfo(info) => {
                    self.state.system_info = Some(info);
                }
                HardwareUpdate::MemoryInfo(info) => {
                    self.state.memory_info = Some(info);
                }
                HardwareUpdate::CpuInfo(info) => {
                    self.state.cpu_info = Some(info);
                }
                HardwareUpdate::GpuInfo(info) => {
                    self.state.gpu_info = info;
                }
                HardwareUpdate::BatteryInfo(info) => {
                    self.state.battery_info = Some(info);
                }
                HardwareUpdate::WifiInfo(info) => {
                    self.state.wifi_info = info;
                }
                HardwareUpdate::FanInfo(info) => {
                    self.state.fan_info = info;
                }
                HardwareUpdate::StorageDeviceInfo(info) => {
                    self.state.storage_device_info = info;
                }
                HardwareUpdate::MountInfo(info) => {
                    self.state.mount_info = info;
                }
                HardwareUpdate::GpuClockRanges(result) => {
                    match result {
                        Ok(ranges) => self.state.gpu_clock_ranges = Some(ranges),
                        Err(e) => self.state.show_message(format!("Failed to get GPU clock ranges: {}", e), true),
                    }
                }
                HardwareUpdate::GpuMemClockRanges(result) => {
                    match result {
                        Ok(mut ranges) => {
                            if !ranges.is_empty() {
                                ranges.sort_unstable();
                                self.state.gpu_mem_clock_ranges = Some((*ranges.first().unwrap(), *ranges.last().unwrap()));
                            }
                        },
                        Err(e) => self.state.show_message(format!("Failed to get GPU memory clock ranges: {}", e), true),
                    }
                }
                HardwareUpdate::GpuCoreOffsetLimits(result) => {
                    match result {
                        Ok(limits) => self.state.gpu_core_offset_limits = Some(limits),
                        Err(e) => self.state.show_message(format!("Failed to get GPU core offset limits: {}", e), true),
                    }
                }
                HardwareUpdate::GpuMemOffsetLimits(result) => {
                    match result {
                        Ok(limits) => self.state.gpu_mem_offset_limits = Some(limits),
                        Err(e) => self.state.show_message(format!("Failed to get GPU memory offset limits: {}", e), true),
                    }
                }
                HardwareUpdate::AvailableThresholds(start, end) => {
                    self.state.available_start_thresholds = start;
                    self.state.available_end_thresholds = end;
                }
                HardwareUpdate::Error(err) => {
                    log::error!("Hardware update error: {}", err);
                }
            }
        }
        
        // Check pending battery update
        if let Some(mut rx) = self.state.pending_battery_update.take() {
            match rx.try_recv() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    self.state.show_message(format!("Battery update failed: {}", e), true);
                }
                Err(oneshot::error::TryRecvError::Empty) => {
                    self.state.pending_battery_update = Some(rx);
                }
                Err(oneshot::error::TryRecvError::Closed) => {
                    self.state.show_message("Battery update channel closed", true);
                }
            }
        }
    }
    
    fn draw_top_bar(&mut self, ctx: &Context) {
        TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                
                // Navigation tabs
                ui.selectable_value(&mut self.state.current_page, Page::Statistics, "📊 Statistics");
                ui.selectable_value(&mut self.state.current_page, Page::Profiles, "📋 Profiles");
                ui.selectable_value(&mut self.state.current_page, Page::Tuning, "🔧 Tuning");
                ui.selectable_value(&mut self.state.current_page, Page::Settings, "⚙️ Settings");
                
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(12.0);
                    let time_str = Local::now().format("%H:%M:%S").to_string();
                    ui.label(RichText::new(time_str).monospace());

                    ui.add_space(12.0);
                    let date_str = Local::now().format("%Y-%m-%d").to_string();
                    ui.label(RichText::new(date_str).monospace());

                    // Current profile indicator
                    ui.label(format!("Profile: {}", self.state.config.current_profile));
                });
            });
            ui.add_space(8.0);
        });
        
        // Status message bar (if any)
        if let Some(ref msg) = self.state.status_message.clone() {
            if msg.shown_at.elapsed() < Duration::from_secs(5) {
                TopBottomPanel::top("status_bar").show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        let color = if msg.is_error {
                            egui::Color32::from_rgb(220, 80, 80)
                        } else {
                            egui::Color32::from_rgb(80, 200, 120)
                        };
                        ui.colored_label(color, &msg.text);
                    });
                });
            } else {
                self.state.status_message = None;
            }
        }
    }
}

impl eframe::App for TuxedoApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Handle keyboard shortcuts
        self.shortcuts.handle_shortcuts(ctx, &mut self.state);
        
        // Handle background hardware updates
        self.handle_hardware_updates();
        
        // Draw top bar
        self.draw_top_bar(ctx);
        
        // Draw main content
        CentralPanel::default().show(ctx, |ui| {
            match self.state.current_page {
                Page::Statistics => {
                    statistics::draw(ui, &mut self.state);
                }
                Page::Profiles => {
                    profiles::draw(ui, &mut self.state, self.dbus_client.as_ref());
                }
                Page::Tuning => {
                    let hw_update_tx = self.hw_update_tx.clone();
                    tuning::draw(ui, &mut self.state, self.dbus_client.as_ref(), hw_update_tx);
                }
                Page::Settings => {
                    settings::draw(ui, &mut self.state, &mut self.theme, ctx, self.dbus_client.as_ref());
                }
            }
        });
        
        // Request repaint if there are pending updates
        ctx.request_repaint_after(Duration::from_millis(500));
    }
}

fn start_background_polling(
    client: DbusClient,
    tx: mpsc::UnboundedSender<HardwareUpdate>,
    _config: &AppConfig,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(1000));
        
        loop {
            interval.tick().await;

            let client = client.clone();
            let tx = tx.clone();

            tokio::spawn(async move {
                let (cpu, gpu, memory, fans, battery, wifi, storage_device, mount) = tokio::join!(
                    client.get_cpu_info(),
                    client.get_gpu_info(),
                    client.get_memory_info(),
                    client.get_fan_info(),
                    client.get_battery_info(),
                    client.get_wifi_info(),
                    client.get_storage_device_info(),
                    client.get_mount_info()
                );

                if let Ok(Ok(info)) = cpu {
                    let _ = tx.send(HardwareUpdate::CpuInfo(info));
                }
                if let Ok(Ok(info)) = gpu {
                    let _ = tx.send(HardwareUpdate::GpuInfo(info));
                }
                if let Ok(Ok(info)) = memory {
                    let _ = tx.send(HardwareUpdate::MemoryInfo(info));
                }
                if let Ok(Ok(info)) = fans {
                    let _ = tx.send(HardwareUpdate::FanInfo(info));
                }
                if let Ok(Ok(info)) = battery {
                    let _ = tx.send(HardwareUpdate::BatteryInfo(info));
                }
                if let Ok(Ok(info)) = wifi {
                    let _ = tx.send(HardwareUpdate::WifiInfo(info));
                }
                if let Ok(Ok(info)) = storage_device {
                    let _ = tx.send(HardwareUpdate::StorageDeviceInfo(info));
                }
                if let Ok(Ok(info)) = mount {
                    let _ = tx.send(HardwareUpdate::MountInfo(info));
                }
            });
        }
    });
}

fn load_config_from_disk() -> anyhow::Result<AppConfig> {
    let config_dir = std::env::var("HOME")? + "/.config/tuxedo-control-center";
    let config_path = format!("{}/config.json", config_dir);
    let json = std::fs::read_to_string(config_path)?;
    Ok(serde_json::from_str(&json)?)
}

fn save_config_to_disk(config: &AppConfig) -> anyhow::Result<()> {
    let config_dir = std::env::var("HOME")? + "/.config/tuxedo-control-center";
    std::fs::create_dir_all(&config_dir)?;
    let config_path = format!("{}/config.json", config_dir);
    let json = serde_json::to_string_pretty(config)?;
    std::fs::write(config_path, json)?;
    Ok(())
}
