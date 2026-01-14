use chrono::Local;
use egui::{Context, CentralPanel, TopBottomPanel, RichText};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use tuxedo_common::types::*;

use crate::dbus_client::DbusClient;
use crate::theme::TuxedoTheme;
use crate::pages::{statistics, profiles, tuning, settings};
use crate::keyboard_shortcuts::KeyboardShortcuts;
use crate::polling_scheduler::{RefreshCoordinator, CoordinatorHandle};
use crate::system_tray::{SystemTray, TrayEvent};

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
    pub pending_prime_profile: Option<String>,
    
    // Profile editing
    pub editing_profile_index: Option<usize>,
    pub editing_profile_name: Option<String>,
    
    // Async state
    pub pending_battery_update: Option<oneshot::Receiver<Result<(), anyhow::Error>>>,
    
    // Refresh coordinator handle
    pub coordinator_handle: Option<CoordinatorHandle>,
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
            pending_prime_profile: None,
            editing_profile_index: None,
            editing_profile_name: None,
            pending_battery_update: None,
            coordinator_handle: None,
        }
    }
    
pub fn load_config(&mut self) {
    if let Ok(config) = load_config_from_disk() {
        self.config = config;
    }
    self.config.autostart = false;
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
    system_tray: Option<SystemTray>,
    
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
        
        // Setup background polling with refresh coordinator
        let (hw_update_tx, hw_update_rx) = mpsc::unbounded_channel();
        let coordinator_handle = if let Some(ref client) = dbus_client {
            let coordinator = RefreshCoordinator::new();
            let handle = coordinator.get_handle();
            
            // Setup refresh callback
            let client_clone = client.clone();
            let tx_clone = hw_update_tx.clone();
            tokio::spawn(async move {
                coordinator.run(move |component_id| {
                    // Trigger refresh for the component
                    let client = client_clone.clone();
                    let tx = tx_clone.clone();
                    let component = component_id.to_string();
                    
                    tokio::spawn(async move {
                        match component.as_str() {
                            "cpu" => {
                                if let Ok(Ok(info)) = client.get_cpu_info().await {
                                    let _ = tx.send(HardwareUpdate::CpuInfo(info));
                                }
                            }
                            "gpu" => {
                                if let Ok(Ok(info)) = client.get_gpu_info().await {
                                    let _ = tx.send(HardwareUpdate::GpuInfo(info));
                                }
                            }
                            "memory" => {
                                if let Ok(Ok(info)) = client.get_memory_info().await {
                                    let _ = tx.send(HardwareUpdate::MemoryInfo(info));
                                }
                            }
                            "fans" => {
                                if let Ok(Ok(info)) = client.get_fan_info().await {
                                    let _ = tx.send(HardwareUpdate::FanInfo(info));
                                }
                            }
                            "battery" => {
                                if let Ok(Ok(info)) = client.get_battery_info().await {
                                    let _ = tx.send(HardwareUpdate::BatteryInfo(info));
                                }
                            }
                            "wifi" => {
                                if let Ok(Ok(info)) = client.get_wifi_info().await {
                                    let _ = tx.send(HardwareUpdate::WifiInfo(info));
                                }
                            }
                            "storage" => {
                                if let Ok(Ok(info)) = client.get_storage_device_info().await {
                                    let _ = tx.send(HardwareUpdate::StorageDeviceInfo(info));
                                }
                            }
                            "mount" => {
                                if let Ok(Ok(info)) = client.get_mount_info().await {
                                    let _ = tx.send(HardwareUpdate::MountInfo(info));
                                }
                            }
                            _ => {}
                        }
                    });
                }).await;
            });
            
            // Register components with their refresh intervals
            let _ = handle.register("cpu".to_string(), Duration::from_millis(state.config.statistics_sections.cpu_poll_rate));
            let _ = handle.register("gpu".to_string(), Duration::from_millis(state.config.statistics_sections.gpu_poll_rate));
            let _ = handle.register("memory".to_string(), Duration::from_millis(state.config.statistics_sections.cpu_poll_rate));
            let _ = handle.register("fans".to_string(), Duration::from_millis(state.config.statistics_sections.fans_poll_rate));
            let _ = handle.register("battery".to_string(), Duration::from_millis(state.config.statistics_sections.battery_poll_rate));
            let _ = handle.register("wifi".to_string(), Duration::from_millis(state.config.statistics_sections.wifi_poll_rate));
            let _ = handle.register("storage".to_string(), Duration::from_millis(state.config.statistics_sections.storage_poll_rate));
            let _ = handle.register("mount".to_string(), Duration::from_millis(state.config.statistics_sections.storage_poll_rate));

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
            
            Some(handle)
        } else {
            None
        };
        
        // Set coordinator handle in state
        state.coordinator_handle = coordinator_handle.clone();
        
        // Apply theme
        let theme = TuxedoTheme::new(&state.config.theme);
        theme.apply_with_font_size(&cc.egui_ctx, &state.config.font_size);

        let system_tray = match SystemTray::new(&state.config.profiles, &state.config.current_profile) {
            Ok(tray) => Some(tray),
            Err(e) => {
                log::warn!("Failed to initialize system tray: {}", e);
                None
            }
        };
        
        Self {
            state,
            dbus_client,
            theme,
            system_tray,
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

    fn handle_tray_events(&mut self, ctx: &Context) {
        let Some(tray) = self.system_tray.as_mut() else {
            return;
        };

        if let Some(event) = tray.handle_events() {
            match event {
                TrayEvent::ShowWindow => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                }
                TrayEvent::ShowStatistics => {
                    self.state.current_page = Page::Statistics;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                }
                TrayEvent::SwitchProfile(idx) => {
                    if let Some(profile) = self.state.config.profiles.get(idx).cloned() {
                        self.state.config.current_profile = profile.name.clone();
                        if let Some(client) = &self.dbus_client {
                            let _ = client.apply_profile(profile);
                        }
                    }
                }
                TrayEvent::Quit => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                TrayEvent::HideWindow => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
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

        self.handle_tray_events(ctx);
        
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

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(client) = &self.dbus_client {
            let _ = client.set_fan_auto(0);
            let _ = client.shutdown_daemon();
        }
    }
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
