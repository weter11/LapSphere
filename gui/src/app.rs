use chrono::Local;
use egui::{Align, CentralPanel, Context, FontFamily, FontId, Layout, RichText, TextStyle, TopBottomPanel};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use lapsphere_common::types::*;

use crate::dbus_client::DbusClient;
use crate::theme::LapSphereTheme;
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingsTab {
    Main,
    StatsConfiguration,
    Hardware,
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
    pub hardware_interface: Option<String>,
    pub keyboard_capabilities: Option<KeyboardCapabilities>,
    pub gpu_clock_ranges: Option<(u32, u32)>,
    pub gpu_mem_clock_ranges: Option<(u32, u32)>,
    pub gpu_core_offset_limits: Option<(i32, i32)>,
    pub gpu_mem_offset_limits: Option<(i32, i32)>,
    pub available_start_thresholds: Vec<u8>,
    pub available_end_thresholds: Vec<u8>,
    pub available_tdp_profiles: Vec<String>,
    
    // UI state
    pub current_page: Page,
    pub settings_tab: SettingsTab,
    pub status_message: Option<StatusMessage>,
    pub restart_confirmation_pending: bool,
    pub pending_prime_profile: Option<String>,
    
    // Profile editing
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
            hardware_interface: None,
            gpu_clock_ranges: None,
            gpu_mem_clock_ranges: None,
            gpu_core_offset_limits: None,
            gpu_mem_offset_limits: None,
            available_start_thresholds: Vec::new(),
            available_end_thresholds: Vec::new(),
            available_tdp_profiles: Vec::new(),
            keyboard_capabilities: None,
            current_page: Page::Statistics,
            settings_tab: SettingsTab::Main,
            status_message: None,
            restart_confirmation_pending: false,
            pending_prime_profile: None,
            editing_profile_name: None,
            pending_battery_update: None,
            coordinator_handle: None,
        }
    }
    
pub fn load_config(&mut self) {
    if let Ok(config) = load_config_from_disk() {
        self.config = config;
        self.config.statistics_sections.section_order =
            statistics::normalize_section_order(&self.config.statistics_sections.section_order);
    }
}
    
    pub fn save_settings(&mut self) -> anyhow::Result<()> {
        save_settings_to_disk(&self.config)?;
        self.show_message("Settings saved", false);
        Ok(())
    }

    pub fn save_profiles(&mut self) -> anyhow::Result<()> {
        save_profiles_to_disk(&self.config)?;
        self.show_message("Profiles saved", false);
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
    
    pub fn current_profile_index(&self) -> Option<usize> {
        self.config.profiles.iter()
            .position(|p| p.name == self.config.current_profile)
    }
}

pub struct LapSphereApp {
    state: AppState,
    dbus_client: Option<DbusClient>,
    theme: LapSphereTheme,
    system_tray: Option<SystemTray>,
    force_quit: bool,
    
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
    HardwareInterface(String),
    GpuClockRanges(Result<(u32, u32), String>),
    GpuMemClockRanges(Result<Vec<u32>, String>),
    GpuCoreOffsetLimits(Result<(i32, i32), String>),
    GpuMemOffsetLimits(Result<(i32, i32), String>),
    AvailableThresholds(Vec<u8>, Vec<u8>),
    TdpProfiles(Vec<String>),
    KeyboardCapabilities(KeyboardCapabilities),
}

impl LapSphereApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut state = AppState::new();
        state.load_config();
        
        // Create DBus client
        let dbus_client = match DbusClient::new() {
            Ok(client) => {
                log::info!("✅ Connected to LapSphere daemon");
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
                                match client.get_cpu_info().await {
                                    Ok(Ok(info)) => { let _ = tx.send(HardwareUpdate::CpuInfo(info)); }
                                    Ok(Err(e)) => log::error!("Failed to get CPU info: {}", e),
                                    Err(e) => log::error!("DBus error getting CPU info: {}", e),
                                }
                            }
                            "gpu" => {
                                match client.get_gpu_info().await {
                                    Ok(Ok(info)) => { let _ = tx.send(HardwareUpdate::GpuInfo(info)); }
                                    Ok(Err(e)) => log::error!("Failed to get GPU info: {}", e),
                                    Err(e) => log::error!("DBus error getting GPU info: {}", e),
                                }
                            }
                            "memory" => {
                                match client.get_memory_info().await {
                                    Ok(Ok(info)) => { let _ = tx.send(HardwareUpdate::MemoryInfo(info)); }
                                    Ok(Err(e)) => log::error!("Failed to get Memory info: {}", e),
                                    Err(e) => log::error!("DBus error getting Memory info: {}", e),
                                }
                            }
                            "fans" => {
                                match client.get_fan_info().await {
                                    Ok(Ok(info)) => { let _ = tx.send(HardwareUpdate::FanInfo(info)); }
                                    Ok(Err(e)) => log::error!("Failed to get Fan info: {}", e),
                                    Err(e) => log::error!("DBus error getting Fan info: {}", e),
                                }
                            }
                            "battery" => {
                                match client.get_battery_info().await {
                                    Ok(Ok(info)) => { let _ = tx.send(HardwareUpdate::BatteryInfo(info)); }
                                    Ok(Err(e)) => log::error!("Failed to get Battery info: {}", e),
                                    Err(e) => log::error!("DBus error getting Battery info: {}", e),
                                }
                            }
                            "wifi" => {
                                match client.get_wifi_info().await {
                                    Ok(Ok(info)) => { let _ = tx.send(HardwareUpdate::WifiInfo(info)); }
                                    Ok(Err(e)) => log::error!("Failed to get WiFi info: {}", e),
                                    Err(e) => log::error!("DBus error getting WiFi info: {}", e),
                                }
                            }
                            "storage" => {
                                match client.get_storage_device_info().await {
                                    Ok(Ok(info)) => { let _ = tx.send(HardwareUpdate::StorageDeviceInfo(info)); }
                                    Ok(Err(e)) => log::error!("Failed to get Storage info: {}", e),
                                    Err(e) => log::error!("DBus error getting Storage info: {}", e),
                                }
                            }
                            "mount" => {
                                match client.get_mount_info().await {
                                    Ok(Ok(info)) => { let _ = tx.send(HardwareUpdate::MountInfo(info)); }
                                    Ok(Err(e)) => log::error!("Failed to get Mount info: {}", e),
                                    Err(e) => log::error!("DBus error getting Mount info: {}", e),
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
            let _ = handle.register("memory".to_string(), Duration::from_millis(state.config.statistics_sections.memory_poll_rate));
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

            let client_clone = client.clone();
            let tx_clone = hw_update_tx.clone();
            tokio::spawn(async move {
                if let Ok(Ok(profiles)) = client_clone.get_tdp_profiles().await {
                    let _ = tx_clone.send(HardwareUpdate::TdpProfiles(profiles));
                }
            });

            let client_clone = client.clone();
            let tx_clone = hw_update_tx.clone();
            tokio::spawn(async move {
                if let Ok(Ok(interface)) = client_clone.get_hardware_interface_info().await {
                    let _ = tx_clone.send(HardwareUpdate::HardwareInterface(interface));
                }
            });

            let client_clone = client.clone();
            let tx_clone = hw_update_tx.clone();
            tokio::spawn(async move {
                if let Ok(Ok(caps)) = client_clone.get_keyboard_capabilities().await {
                    let _ = tx_clone.send(HardwareUpdate::KeyboardCapabilities(caps));
                }
            });
            
            Some(handle)
        } else {
            None
        };
        
        // Set coordinator handle in state
        state.coordinator_handle = coordinator_handle.clone();
        
        // Apply theme
        let theme = LapSphereTheme::new(&state.config.theme, cc.egui_ctx.style().visuals.dark_mode);
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
            force_quit: false,
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
                HardwareUpdate::HardwareInterface(info) => {
                    self.state.hardware_interface = Some(info);
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
                HardwareUpdate::TdpProfiles(profiles) => {
                    self.state.available_tdp_profiles = profiles;
                }
                HardwareUpdate::KeyboardCapabilities(caps) => {
                    self.state.keyboard_capabilities = Some(caps);
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
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);

                let time_str = Local::now().format("%H:%M:%S").to_string();
                let date_str = Local::now().format("%Y-%m-%d").to_string();
                let profile_str = format!("Profile: {}", self.state.config.current_profile);
                let base_size = TextStyle::Small.resolve(ui.style()).size;
                let top_bar_size = base_size + 1.0;
                let mono_font = FontId::new(top_bar_size, FontFamily::Monospace);
                let text_font = FontId::new(top_bar_size, FontFamily::Proportional);
                let text_color = ui.visuals().text_color();
                let right_width = ui.fonts(|fonts| {
                    let time_width = fonts.layout_no_wrap(time_str.clone(), mono_font.clone(), text_color).size().x;
                    let date_width = fonts.layout_no_wrap(date_str.clone(), mono_font.clone(), text_color).size().x;
                    let profile_width = fonts.layout_no_wrap(profile_str.clone(), text_font.clone(), text_color).size().x;
                    time_width.max(date_width).max(profile_width)
                }) + 16.0;
                let tabs_width = (ui.available_width() - right_width).max(0.0);

                ui.allocate_ui_with_layout(
                    egui::vec2(tabs_width, ui.available_height()),
                    Layout::left_to_right(Align::Center).with_main_align(Align::Center),
                    |ui| {
                        // Navigation tabs
                        ui.selectable_value(&mut self.state.current_page, Page::Statistics, "📊 Statistics");
                        ui.selectable_value(&mut self.state.current_page, Page::Profiles, "📋 Profiles");
                        ui.selectable_value(&mut self.state.current_page, Page::Tuning, "🔧 Tuning");
                        ui.selectable_value(&mut self.state.current_page, Page::Settings, "⚙ Settings");
                    },
                );

                ui.allocate_ui_with_layout(
                    egui::vec2(right_width, ui.available_height()),
                    Layout::right_to_left(Align::Center),
                    |ui| {
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(time_str).font(mono_font.clone()));
                            ui.label(RichText::new(date_str).font(mono_font.clone()));
                            ui.label(RichText::new(profile_str).font(text_font.clone()));
                        });
                    },
                );
            });
            ui.add_space(6.0);
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
                    self.force_quit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }
}

impl eframe::App for LapSphereApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Handle keyboard shortcuts
        self.shortcuts.handle_shortcuts(ctx, &mut self.state);
        
        // Handle background hardware updates
        self.handle_hardware_updates();

        self.handle_tray_events(ctx);
        
        if ctx.input(|input| input.viewport().close_requested())
            && self.state.config.tray_enabled
            && !self.force_quit
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        // Draw top bar
        self.draw_top_bar(ctx);
        
        // Update theme if it's Auto to react to system theme changes
        if self.state.config.theme == Theme::Auto {
            let is_dark = ctx.style().visuals.dark_mode;
            if is_dark != self.theme.visuals.dark_mode {
                self.theme = LapSphereTheme::new(&self.state.config.theme, is_dark);
                self.theme.apply_with_font_size(ctx, &self.state.config.font_size);
            }
        }

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
            let client = client.clone();
            tokio::spawn(async move {
                let _ = tokio::time::timeout(Duration::from_secs(2), client.set_fan_auto(0)).await;
                let _ = tokio::time::timeout(Duration::from_secs(2), client.shutdown_daemon()).await;
            });
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct SettingsConfig {
    theme: Theme,
    start_minimized: bool,
    tray_enabled: bool,
    autostart: bool,
    cpu_scheduler: String,
    font_size: FontSize,
    statistics_sections: StatisticsSections,
    tuning_section_order: Vec<String>,
    battery_settings: BatterySettings,
}

impl Default for SettingsConfig {
    fn default() -> Self {
        let config = AppConfig::default();
        Self {
            theme: config.theme,
            start_minimized: config.start_minimized,
            tray_enabled: config.tray_enabled,
            autostart: config.autostart,
            cpu_scheduler: config.cpu_scheduler,
            font_size: config.font_size,
            statistics_sections: config.statistics_sections,
            tuning_section_order: config.tuning_section_order,
            battery_settings: config.battery_settings,
        }
    }
}

impl From<&AppConfig> for SettingsConfig {
    fn from(config: &AppConfig) -> Self {
        Self {
            theme: config.theme.clone(),
            start_minimized: config.start_minimized,
            tray_enabled: config.tray_enabled,
            autostart: config.autostart,
            cpu_scheduler: config.cpu_scheduler.clone(),
            font_size: config.font_size.clone(),
            statistics_sections: config.statistics_sections.clone(),
            tuning_section_order: config.tuning_section_order.clone(),
            battery_settings: config.battery_settings.clone(),
        }
    }
}

impl SettingsConfig {
    fn apply_to(&self, config: &mut AppConfig) {
        config.theme = self.theme.clone();
        config.start_minimized = self.start_minimized;
        config.tray_enabled = self.tray_enabled;
        config.autostart = self.autostart;
        config.cpu_scheduler = self.cpu_scheduler.clone();
        config.font_size = self.font_size.clone();
        config.statistics_sections = self.statistics_sections.clone();
        config.tuning_section_order = self.tuning_section_order.clone();
        config.battery_settings = self.battery_settings.clone();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct ProfilesConfig {
    profiles: Vec<Profile>,
    current_profile: String,
}

impl Default for ProfilesConfig {
    fn default() -> Self {
        let config = AppConfig::default();
        Self {
            profiles: config.profiles,
            current_profile: config.current_profile,
        }
    }
}

impl From<&AppConfig> for ProfilesConfig {
    fn from(config: &AppConfig) -> Self {
        Self {
            profiles: config.profiles.clone(),
            current_profile: config.current_profile.clone(),
        }
    }
}

impl ProfilesConfig {
    fn apply_to(&self, config: &mut AppConfig) {
        config.profiles = self.profiles.clone();
        config.current_profile = self.current_profile.clone();
    }
}

fn load_settings_from_disk(path: &str) -> anyhow::Result<SettingsConfig> {
    let json = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&json)?)
}

fn load_profiles_from_disk(path: &str) -> anyhow::Result<ProfilesConfig> {
    let json = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&json)?)
}

fn load_config_from_disk() -> anyhow::Result<AppConfig> {
    let config_dir = std::env::var("HOME")? + "/.config/lapsphere";
    let settings_path = format!("{}/settings.json", config_dir);
    let profiles_path = format!("{}/profiles.json", config_dir);
    let legacy_path = format!("{}/config.json", config_dir);

    let legacy_config = if Path::new(&legacy_path).exists() {
        let json = std::fs::read_to_string(&legacy_path)?;
        Some(serde_json::from_str::<AppConfig>(&json)?)
    } else {
        None
    };

    let settings = if Path::new(&settings_path).exists() {
        Some(load_settings_from_disk(&settings_path)?)
    } else {
        legacy_config.as_ref().map(SettingsConfig::from)
    };

    let profiles = if Path::new(&profiles_path).exists() {
        Some(load_profiles_from_disk(&profiles_path)?)
    } else {
        legacy_config.as_ref().map(ProfilesConfig::from)
    };

    let mut config = AppConfig::default();
    if let Some(settings) = settings {
        settings.apply_to(&mut config);
    }
    if let Some(profiles) = profiles {
        profiles.apply_to(&mut config);
    }

    config.statistics_sections.section_order =
        statistics::normalize_section_order(&config.statistics_sections.section_order);

    if legacy_config.is_some()
        && (!Path::new(&settings_path).exists() || !Path::new(&profiles_path).exists())
    {
        if let Err(err) = save_settings_to_disk(&config) {
            log::warn!("Failed to migrate settings config: {}", err);
        }
        if let Err(err) = save_profiles_to_disk(&config) {
            log::warn!("Failed to migrate profiles config: {}", err);
        }
    }

    Ok(config)
}

fn save_settings_to_disk(config: &AppConfig) -> anyhow::Result<()> {
    let config_dir = std::env::var("HOME")? + "/.config/lapsphere";
    std::fs::create_dir_all(&config_dir)?;
    let settings_path = format!("{}/settings.json", config_dir);
    let json = serde_json::to_string_pretty(&SettingsConfig::from(config))?;
    std::fs::write(settings_path, json)?;

    // Handle autostart
    let home = std::env::var("HOME")?;
    let autostart_dir = format!("{}/.config/autostart", home);
    let desktop_file = format!("{}/io.lapsphere.LapSphere.desktop", autostart_dir);

    if config.autostart {
        std::fs::create_dir_all(&autostart_dir)?;
        let content = format!(
            "[Desktop Entry]\n\
            Type=Application\n\
            Name=LapSphere\n\
            Exec=lapsphere --tray\n\
            Icon=lapsphere\n\
            X-GNOME-Autostart-enabled=true\n"
        );
        std::fs::write(&desktop_file, content)?;
    } else if std::path::Path::new(&desktop_file).exists() {
        std::fs::remove_file(&desktop_file)?;
    }

    Ok(())
}

fn save_profiles_to_disk(config: &AppConfig) -> anyhow::Result<()> {
    let config_dir = std::env::var("HOME")? + "/.config/lapsphere";
    std::fs::create_dir_all(&config_dir)?;
    let profiles_path = format!("{}/profiles.json", config_dir);
    let json = serde_json::to_string_pretty(&ProfilesConfig::from(config))?;
    std::fs::write(profiles_path, json)?;
    Ok(())
}
