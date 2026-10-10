use chrono::Local;
use egui::{Align, CentralPanel, Context, FontFamily, FontId, Layout, RichText, TextStyle};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::collections::VecDeque;
use tokio::sync::{mpsc, oneshot};
use lapsphere_common::types::*;

use crate::dbus_client::DbusClient;
use crate::keyboard_shortcuts::KeyboardShortcuts;
use crate::pages::{profiles, settings, statistics, tuning};
use crate::panel::menu::{settings_viewport, SETTINGS_SIZE};
use crate::panel::window::Mode;
use crate::polling_scheduler::{CoordinatorHandle, RefreshCoordinator};
use crate::system_tray::{SystemTray, TrayEvent};
use crate::theme::LapSphereTheme;
use crate::tray_config;

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
    Logs,
    Help,
    About,
}

// ---------------------------------------------------------------------------
// Daemon log ring: fetch it only while the Logs tab is actually on screen
// ---------------------------------------------------------------------------
//
// `GetDaemonLogs` hands back the daemon's whole 2000-entry ring on every call
// (measured: 481,816 bytes of JSON — ~2000 `LogEntry` records carrying four
// `String`s each). The `logs` component used to be registered at a flat 5 s
// interval no matter which page was up, so an idle GUI (tray-only, Statistics,
// any other tab) still allocated, parsed and dropped ~470 kB every 5 s —
// ~96 kB/s of churn with nobody reading it. That churn is the allocation
// pressure the allocator has to absorb; it is cheaper to not create it.
//
// So: the UI stamps a frame heartbeat once per frame together with whether that
// frame drew the Logs tab, and the polling callback only spends the D-Bus round
// trip when both hold. A heartbeat older than `UI_FRAME_FRESH_MS` means no frame
// is being painted (minimized / hidden-to-tray window), so nothing can be
// reading logs either. Opening the Logs tab costs at most one polling tick
// (5 s) before the first refresh.

/// How long a UI frame stamp stays fresh; beyond this the window is not painting.
const UI_FRAME_FRESH_MS: u64 = 5_000;

/// Wall-clock stamp (ms since the UNIX epoch) of the most recent UI frame.
static LAST_UI_FRAME_MS: AtomicU64 = AtomicU64::new(0);
/// Whether the most recent UI frame drew the Logs tab.
static LOGS_TAB_ON_SCREEN: AtomicBool = AtomicBool::new(false);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Is the daemon log ring worth one D-Bus round trip right now?
///
/// `frame_age_ms` is the age of the last painted frame (0 while painting).
pub fn logs_fetch_needed(page: Page, tab: SettingsTab, frame_age_ms: u64) -> bool {
    page == Page::Settings && tab == SettingsTab::Logs && frame_age_ms <= UI_FRAME_FRESH_MS
}

/// Record that a frame has been painted (called once per frame from the UI loop).
pub fn note_ui_frame(page: Page, tab: SettingsTab) {
    LOGS_TAB_ON_SCREEN.store(logs_fetch_needed(page, tab, 0), Ordering::Relaxed);
    LAST_UI_FRAME_MS.store(now_ms(), Ordering::Relaxed);
}

/// Gate consulted by the `logs` polling callback.
pub fn should_fetch_logs() -> bool {
    if !LOGS_TAB_ON_SCREEN.load(Ordering::Relaxed) {
        return false;
    }
    let last_frame = LAST_UI_FRAME_MS.load(Ordering::Relaxed);
    logs_fetch_needed(Page::Settings, SettingsTab::Logs, now_ms().saturating_sub(last_frame))
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
    pub gamepad_info: Vec<GamepadInfo>,
    pub fan_info: Vec<FanInfo>,
    pub storage_device_info: Vec<StorageDevice>,
    pub mount_info: Vec<MountInfo>,
    pub hardware_interface: Option<String>,
    pub keyboard_capabilities: Option<KeyboardCapabilities>,
    pub gpu_clock_ranges: Option<(u32, u32)>,
    pub gpu_clock_ranges_error: Option<String>,
    pub gpu_mem_clock_ranges: Option<(u32, u32)>,
    pub gpu_core_offset_limits: Option<(i32, i32)>,
    pub gpu_core_offset_error: Option<String>,
    pub gpu_mem_offset_limits: Option<(i32, i32)>,
    pub gpu_mem_offset_error: Option<String>,
    pub available_start_thresholds: Vec<u8>,
    pub available_end_thresholds: Vec<u8>,
    pub available_tdp_profiles: Vec<String>,
    pub webcam_enabled: Option<bool>,
    pub daemon_logs: VecDeque<LogEntry>,
    pub new_version_available: Option<String>,
    pub latest_changelog: Option<String>,
    pub log_filter_trace: bool,
    pub log_filter_debug: bool,
    pub log_filter_info: bool,
    pub log_filter_warn: bool,
    pub log_filter_error: bool,
    pub log_paused: bool,
    pub log_search_text: String,
    
    // UI state
    pub current_page: Page,
    pub settings_tab: SettingsTab,
    pub status_message: Option<StatusMessage>,
    pub restart_confirmation_pending: bool,
    pub pending_prime_profile: Option<String>,
    pub selected_fan_curve: usize,
    
    // Profile editing
    pub editing_profile_name: Option<String>,
    
    // Async state
    pub pending_battery_update: Option<oneshot::Receiver<Result<(), anyhow::Error>>>,
    
    // Refresh coordinator handle
    pub coordinator_handle: Option<CoordinatorHandle>,

    pub keyboard_brush_color: [u8; 3],
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
            gamepad_info: Vec::new(),
            fan_info: Vec::new(),
            storage_device_info: Vec::new(),
            mount_info: Vec::new(),
            hardware_interface: None,
            gpu_clock_ranges: None,
            gpu_clock_ranges_error: None,
            gpu_mem_clock_ranges: None,
            gpu_core_offset_limits: None,
            gpu_core_offset_error: None,
            gpu_mem_offset_limits: None,
            gpu_mem_offset_error: None,
            available_start_thresholds: Vec::new(),
            available_end_thresholds: Vec::new(),
            available_tdp_profiles: Vec::new(),
            webcam_enabled: None,
            daemon_logs: VecDeque::new(),
            new_version_available: None,
            latest_changelog: None,
            log_filter_trace: false,
            log_filter_debug: false,
            log_filter_info: true,
            log_filter_warn: true,
            log_filter_error: true,
            log_paused: true,
            log_search_text: String::new(),
            keyboard_capabilities: None,
            current_page: Page::Statistics,
            settings_tab: SettingsTab::Main,
            status_message: None,
            restart_confirmation_pending: false,
            pending_prime_profile: None,
            editing_profile_name: None,
            pending_battery_update: None,
            coordinator_handle: None,
            selected_fan_curve: 0,
            keyboard_brush_color: [255, 255, 255],
        }
    }

    fn clamp_fan_selection(&mut self) {
        let fan_count = self.fan_info.len();
        if fan_count == 0 {
            self.selected_fan_curve = 0;
        } else if self.selected_fan_curve >= fan_count {
            self.selected_fan_curve = fan_count.saturating_sub(1);
        }
    }
    
pub fn load_config(&mut self) {
    if let Ok(config) = load_config_from_disk() {
        self.config = config;
        self.config.statistics_sections.section_order =
            statistics::normalize_section_order(&self.config.statistics_sections.section_order);
        self.log_filter_trace = self.config.log_filter_trace;
    }
}
    
    pub fn save_settings(&mut self) -> anyhow::Result<()> {
        save_settings_to_disk(&self.config)?;
        self.show_message("Settings saved", false);
        Ok(())
    }

    /// Persist only the tray fields, to `tray.json`.
    ///
    /// Used by the tray checkboxes in Settings so toggling the tray does not
    /// rewrite `settings.json`, whose content did not change. `autostart`
    /// stays on `save_settings`: it lives in `settings.json` and is applied by
    /// writing the desktop entry there.
    pub fn save_tray_settings(&mut self) -> anyhow::Result<()> {
        tray_config::save_tray_config(&get_config_dir(), &self.config)?;
        self.show_message("Tray settings saved", false);
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

// ---------------------------------------------------------------------------
// Repaint on data arrival
// ---------------------------------------------------------------------------
//
// The only repaint request the app used to make was the 500 ms fallback timer
// at the end of `ui()`. A polled update therefore waited for the next scheduled
// frame: measured update->draw latency was min 0.1 / avg 226 / max 484 ms, a
// distribution bounded by exactly that 500 ms period. Every send into
// `hw_update_rx` now asks for a frame instead.
//
// The requests are coalesced: `REPAINT_PENDING` is a one-shot latch, so a batch
// of N updates costs one `request_repaint`, not N. `handle_hardware_updates`
// clears the latch at the start of each drain, which makes the invariant "at
// most one repaint request per frame" hold for both the updates it is about to
// consume and any that land while the frame is being built.

static REPAINT_PENDING: AtomicBool = AtomicBool::new(false);

/// Ask for a frame, coalescing bursts into a single request.
fn request_repaint_now(ctx: &egui::Context) {
    if !REPAINT_PENDING.swap(true, Ordering::AcqRel) {
        ctx.request_repaint();
    }
}

/// Send one update and repaint when it landed. A `SendError` (receiver dropped
/// during exit) is ignored exactly as before, and asks for no frame.
///
/// This is the lossless path, reserved for the one-shot startup requests
/// (`SystemInfo`, `AvailableThresholds`, `TdpProfiles`, the update check): they
/// carry state nothing else will re-fetch, so they wait for room rather than
/// being dropped. Polling replies use `send_polled_update` instead.
async fn send_update(
    tx: &mpsc::Sender<HardwareUpdate>,
    update: HardwareUpdate,
    ctx: &egui::Context,
) {
    if tx.send(update).await.is_ok() {
        request_repaint_now(ctx);
    }
}

// ---------------------------------------------------------------------------
// Polled updates: drop rather than block
// ---------------------------------------------------------------------------
//
// A polled reply is a snapshot of a sample that is already ageing — the next
// tick supersedes it. Blocking on `send()` is therefore the wrong trade for
// these: the sender waits for a consumer that may not exist (hidden window),
// and each waiter is one more live task holding its payload. The bounded queue
// does not fix that; it just relocates the growth into the task set.
//
// So polled replies use `try_send`. Full channel means the UI is behind and the
// update is stale: it is dropped, and the frame that eventually arrives is the
// newer sample. `Closed` (receiver dropped at exit) is ignored, as before.
//
// Dropped is not silent: the running count is logged at debug, rate-limited so
// a long iconified stretch cannot turn the log into the next firehose.

static DROPPED_UPDATES: AtomicU64 = AtomicU64::new(0);
static DROPPED_LOGS: AtomicU64 = AtomicU64::new(0);

/// Log the first few drops and then every 64th, so the counter stays visible
/// without becoming a hot path of its own.
fn should_log_drop() -> bool {
    let n = DROPPED_UPDATES.fetch_add(1, Ordering::Relaxed) + 1;
    n <= 4 || n % 64 == 0
}

/// Total polled updates dropped so far because the channel was full.
pub fn dropped_update_count() -> u64 {
    DROPPED_UPDATES.load(Ordering::Relaxed)
}

/// The subset of those that were daemon-log-ring replies (the largest payload).
pub fn dropped_log_update_count() -> u64 {
    DROPPED_LOGS.load(Ordering::Relaxed)
}

/// Send a polled reply, dropping it when the queue is full, and repaint on
/// success.
fn send_polled_update(
    tx: &mpsc::Sender<HardwareUpdate>,
    update: HardwareUpdate,
    ctx: &egui::Context,
) {
    let is_log_ring = matches!(update, HardwareUpdate::DaemonLogs(_));
    match tx.try_send(update) {
        Ok(()) => request_repaint_now(ctx),
        Err(mpsc::error::TrySendError::Full(_)) => {
            if is_log_ring {
                DROPPED_LOGS.fetch_add(1, Ordering::Relaxed);
            }
            if should_log_drop() {
                log::debug!(
                    "dropped polled update: update channel full ({} dropped so far, {} of them daemon logs)",
                    dropped_update_count(),
                    dropped_log_update_count(),
                );
            }
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {}
    }
}

pub struct LapSphereApp {
    state: AppState,
    dbus_client: Option<DbusClient>,
    theme: LapSphereTheme,
    system_tray: Option<SystemTray>,
    force_quit: bool,
    
    // Background update channel
    hw_update_tx: mpsc::Sender<HardwareUpdate>,
    hw_update_rx: mpsc::Receiver<HardwareUpdate>,
    
    // Keyboard shortcuts
    shortcuts: KeyboardShortcuts,

    startup_frames: u32,
    startup_apply_pending: bool,
    startup_started: std::time::Instant,
    startup_timeout_warned: bool,
    startup_apply_reply: Option<oneshot::Receiver<anyhow::Result<()>>>,

    last_tray_profile: String,
    last_tray_profiles_count: usize,

    // ---- Panel mode ----
    /// Which surface the one window is currently showing.
    mode: Mode,
    /// A mode change requested by a control, acted on at the top of the next
    /// frame. Viewport commands are not safe from inside a click handler.
    pending_mode: Option<Mode>,
    /// Whether the window spec for the current mode has been sent. See
    /// `panel::window::mode_to_apply` for why this cannot be inferred from
    /// `pending_mode` alone.
    panel_spec_applied: bool,
    /// The normal window's inner size, captured before the first switch so
    /// returning to it restores the user's own geometry.
    normal_geometry: Option<[f32; 2]>,
    /// The poll set last pushed to the coordinator, so a stable panel costs
    /// nothing per frame.
    last_poll_set: Vec<String>,
    /// The two-entry context menu, open in the panel window.
    panel_menu_open: bool,
    /// Whether the separate settings window is open.
    settings_open: bool,
    /// What the settings window asked for, applied at the top of the next frame.
    settings_outcome: crate::panel::menu::SettingsOutcome,
    /// The panel's settings, from `panel.json`.
    panel_config: crate::panel::PanelConfig,
}

#[derive(Debug)]
pub enum HardwareUpdate {
    SystemInfo(SystemInfo),
    MemoryInfo(MemoryInfo),
    CpuInfo(CpuInfo),
    GpuInfo(Vec<GpuInfo>),
    BatteryInfo(BatteryInfo),
    WifiInfo(Vec<WiFiInfo>),
    GamepadInfo(Vec<GamepadInfo>),
    FanInfo(Vec<FanInfo>),
    StorageDeviceInfo(Vec<StorageDevice>),
    MountInfo(Vec<MountInfo>),
    HardwareInterface(String),
    WebcamState(bool),
    DaemonLogs(Vec<LogEntry>),
    UpdateInfo(String, String),
    GpuClockRanges(Result<(u32, u32), String>),
    GpuCoreOffsetLimits(Result<(i32, i32), String>),
    GpuMemOffsetLimits(Result<(i32, i32), String>),
    AvailableThresholds(Vec<u8>, Vec<u8>),
    TdpProfiles(Vec<String>),
    KeyboardCapabilities(KeyboardCapabilities),
}

impl LapSphereApp {
    /// `start_in_panel` comes from the `--panel` command-line flag.
    ///
    /// It sets `mode` DIRECTLY rather than going through `pending_mode`: there
    /// is no transition at startup, so there is nothing to pend. `apply_mode`
    /// handles that case explicitly (see `panel::window::mode_to_apply`) — a
    /// first-frame guard keyed only on `pending_mode` would skip the spec
    /// forever and the panel would render in the normal window's geometry.
    pub fn new(cc: &eframe::CreationContext<'_>, start_in_panel: bool) -> Self {
        let mut state = AppState::new();
        state.load_config();
        
        // Create DBus client
        let dbus_client = match DbusClient::new() {
            Ok(client) => {
                log::info!("✅ Connected to LapSphere daemon");
                crate::dbus_client::set_shared_client(client.clone());
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
        // Use a bounded channel to prevent potential memory leaks if UI processing stalls
        let (hw_update_tx, hw_update_rx) = mpsc::channel(100);
        // Cloned once here and moved into the polling tasks so a completed
        // D-Bus fetch can ask for a frame immediately (see `send_update`).
        let repaint_ctx = cc.egui_ctx.clone();
        // Bounded concurrency for the polling callbacks (see `InFlightSet`).
        let in_flight = crate::polling_scheduler::InFlightSet::new();
        let coordinator_handle = if let Some(ref client) = dbus_client {
            let coordinator = RefreshCoordinator::new();
            let handle = coordinator.get_handle();
            
            // Setup refresh callback
            let client_clone = client.clone();
            let tx_clone = hw_update_tx.clone();
            let ctx_clone = repaint_ctx.clone();
            let in_flight = in_flight.clone();
            tokio::spawn(async move {
                coordinator.run(move |component_id| {
                    // One request per component at a time. If the previous tick's
                    // fetch has not finished, this tick is skipped rather than
                    // stacked: with the window hidden nothing drains the update
                    // channel, and a queued task per tick is unbounded growth.
                    let Some(permit) = in_flight.try_begin(component_id) else {
                        // Rate-limited: a hidden window skips every tick, and an
                        // unconditional debug line per skip would be a firehose.
                        let skipped = in_flight.skipped_ticks();
                        if skipped <= 4 || skipped % 64 == 0 {
                            log::debug!(
                                "polling tick skipped, request still in flight: {} ({} skipped so far, {} in flight)",
                                component_id,
                                skipped,
                                in_flight.active_len(),
                            );
                        }
                        return;
                    };

                    // Trigger refresh for the component
                    let client = client_clone.clone();
                    let tx = tx_clone.clone();
                    let ctx = ctx_clone.clone();
                    let component = component_id.to_string();

                    tokio::spawn(async move {
                        // Released when this fetch ends, on every path.
                        let _permit = permit;
                        match component.as_str() {
                            "cpu" => {
                                match client.get_cpu_info().await {
                                    Ok(Ok(info)) => { send_polled_update(&tx, HardwareUpdate::CpuInfo(info), &ctx); }
                                    Ok(Err(e)) => log::error!("Failed to get CPU info: {}", e),
                                    Err(e) => log::error!("DBus error getting CPU info: {}", e),
                                }
                            }
                            "gpu" => {
                                match client.get_gpu_info().await {
                                    Ok(Ok(info)) => { send_polled_update(&tx, HardwareUpdate::GpuInfo(info), &ctx); }
                                    Ok(Err(e)) => log::error!("Failed to get GPU info: {}", e),
                                    Err(e) => log::error!("DBus error getting GPU info: {}", e),
                                }
                            }
                            "memory" => {
                                match client.get_memory_info().await {
                                    Ok(Ok(info)) => { send_polled_update(&tx, HardwareUpdate::MemoryInfo(info), &ctx); }
                                    Ok(Err(e)) => log::error!("Failed to get Memory info: {}", e),
                                    Err(e) => log::error!("DBus error getting Memory info: {}", e),
                                }
                            }
                            "fans" => {
                                match client.get_fan_info().await {
                                    Ok(Ok(info)) => { send_polled_update(&tx, HardwareUpdate::FanInfo(info), &ctx); }
                                    Ok(Err(e)) => log::error!("Failed to get Fan info: {}", e),
                                    Err(e) => log::error!("DBus error getting Fan info: {}", e),
                                }
                            }
                            "battery" => {
                                match client.get_battery_info().await {
                                    Ok(Ok(info)) => { send_polled_update(&tx, HardwareUpdate::BatteryInfo(info), &ctx); }
                                    Ok(Err(e)) => log::error!("Failed to get Battery info: {}", e),
                                    Err(e) => log::error!("DBus error getting Battery info: {}", e),
                                }
                            }
                            "wifi" => {
                                match client.get_wifi_info().await {
                                    Ok(Ok(info)) => { send_polled_update(&tx, HardwareUpdate::WifiInfo(info), &ctx); }
                                    Ok(Err(e)) => log::error!("Failed to get WiFi info: {}", e),
                                    Err(e) => log::error!("DBus error getting WiFi info: {}", e),
                                }
                            }
                            "gamepads" => {
                                match client.get_gamepad_info().await {
                                    Ok(Ok(info)) => { send_polled_update(&tx, HardwareUpdate::GamepadInfo(info), &ctx); }
                                    Ok(Err(e)) => log::error!("Failed to get Gamepad info: {}", e),
                                    Err(e) => log::error!("DBus error getting Gamepad info: {}", e),
                                }
                            }
                            "storage" => {
                                match client.get_storage_device_info().await {
                                    Ok(Ok(info)) => { send_polled_update(&tx, HardwareUpdate::StorageDeviceInfo(info), &ctx); }
                                    Ok(Err(e)) => log::error!("Failed to get Storage info: {}", e),
                                    Err(e) => log::error!("DBus error getting Storage info: {}", e),
                                }
                            }
                            "mount" => {
                                match client.get_mount_info().await {
                                    Ok(Ok(info)) => { send_polled_update(&tx, HardwareUpdate::MountInfo(info), &ctx); }
                                    Ok(Err(e)) => log::error!("Failed to get Mount info: {}", e),
                                    Err(e) => log::error!("DBus error getting Mount info: {}", e),
                                }
                            }
                            "webcam" => {
                                match client.get_webcam_state().await {
                                    Ok(Ok(state)) => { send_polled_update(&tx, HardwareUpdate::WebcamState(state), &ctx); }
                                    _ => {}
                                }
                            }
                            "logs" => {
                                // Only while the Logs tab is on screen: this reply is the
                                // daemon's whole ring (~470 kB of JSON), so polling it for a
                                // GUI nobody is reading logs in is pure allocation churn.
                                if should_fetch_logs() {
                                    match client.get_daemon_logs().await {
                                        Ok(Ok(logs)) => { send_polled_update(&tx, HardwareUpdate::DaemonLogs(logs), &ctx); }
                                        _ => {}
                                    }
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
            let _ = handle.register("gamepads".to_string(), Duration::from_millis(state.config.statistics_sections.gamepad_poll_rate));
            let _ = handle.register("storage".to_string(), Duration::from_millis(state.config.statistics_sections.storage_poll_rate));
            let _ = handle.register("mount".to_string(), Duration::from_millis(state.config.statistics_sections.storage_poll_rate));
            let _ = handle.register("webcam".to_string(), Duration::from_secs(5));
            // Registered, but the callback only fetches while the Logs tab is on
            // screen (see `should_fetch_logs`): the ring reply is ~470 kB each time.
            let _ = handle.register("logs".to_string(), Duration::from_secs(5));

            // Initial system info load
            let client_clone = client.clone();
            let tx_clone = hw_update_tx.clone();
            let ctx_clone = repaint_ctx.clone();
            tokio::spawn(async move {
                if let Ok(Ok(info)) = client_clone.get_system_info().await {
                    send_update(&tx_clone, HardwareUpdate::SystemInfo(info), &ctx_clone).await;
                }
            });

            // Fetch available thresholds
            let client_clone = client.clone();
            let tx_clone = hw_update_tx.clone();
            let ctx_clone = repaint_ctx.clone();
            tokio::spawn(async move {
                let start_rx = client_clone.get_battery_available_start_thresholds();
                let end_rx = client_clone.get_battery_available_end_thresholds();

                match (start_rx.await, end_rx.await) {
                    (Ok(Ok(start)), Ok(Ok(end))) => {
                        send_update(&tx_clone, HardwareUpdate::AvailableThresholds(start, end), &ctx_clone).await;
                    }
                    _ => {}
                }
            });

            let client_clone = client.clone();
            let tx_clone = hw_update_tx.clone();
            let ctx_clone = repaint_ctx.clone();
            tokio::spawn(async move {
                if let Ok(Ok(profiles)) = client_clone.get_tdp_profiles().await {
                    send_update(&tx_clone, HardwareUpdate::TdpProfiles(profiles), &ctx_clone).await;
                }
            });

            let client_clone = client.clone();
            let tx_clone = hw_update_tx.clone();
            let ctx_clone = repaint_ctx.clone();
            tokio::spawn(async move {
                if let Ok(Ok(interface)) = client_clone.get_hardware_interface_info().await {
                    send_update(&tx_clone, HardwareUpdate::HardwareInterface(interface), &ctx_clone).await;
                }
            });

            let client_clone = client.clone();
            let tx_clone = hw_update_tx.clone();
            let ctx_clone = repaint_ctx.clone();
            tokio::spawn(async move {
                if let Ok(Ok(caps)) = client_clone.get_keyboard_capabilities().await {
                    send_update(&tx_clone, HardwareUpdate::KeyboardCapabilities(caps), &ctx_clone).await;
                }
            });
            
            Some(handle)
        } else {
            None
        };

        // Check for updates
        let tx_update = hw_update_tx.clone();
        let ctx_update = repaint_ctx.clone();
        tokio::spawn(async move {
            let current_version = env!("CARGO_PKG_VERSION");
            let url = "https://api.github.com/repos/weter11/lapsphere/releases/latest";

            let agent = ureq::AgentBuilder::new()
                .user_agent("LapSphere-Update-Checker")
                .build();

            match agent.get(url).call() {
                Ok(response) => {
                    if let Ok(json) = response.into_json::<serde_json::Value>() {
                        if let Some(tag) = json["tag_name"].as_str() {
                            let latest = tag.trim_start_matches('v');
                            if latest != current_version {
                                let body = json["body"].as_str().unwrap_or("No changelog provided.").to_string();
                                send_update(&tx_update, HardwareUpdate::UpdateInfo(latest.to_string(), body), &ctx_update).await;
                            }
                        }
                    }
                }
                Err(e) => log::warn!("Failed to check for updates: {}", e),
            }
        });
        
        // Set coordinator handle in state
        state.coordinator_handle = coordinator_handle.clone();
        
        // Apply theme
        let theme = LapSphereTheme::new(
            &state.config.theme,
            cc.egui_ctx.global_style().visuals.dark_mode,
        );
        theme.apply_with_font_size(&cc.egui_ctx, &state.config.font_size);


        let system_tray = match SystemTray::new(&state.config.profiles, &state.config.current_profile) {
            Ok(tray) => Some(tray),
            Err(e) => {
                log::warn!("Failed to initialize system tray: {}", e);
                None
            }
        };
        
        let last_tray_profile = state.config.current_profile.clone();
        let last_tray_profiles_count = state.config.profiles.len();

        // A missing or corrupt panel.json is not an error: the panel is an
        // accessory surface and must not be able to keep the GUI from starting.
        let panel_config = crate::panel::load_panel_config();

        Self {
            mode: if start_in_panel {
                Mode::Panel
            } else {
                Mode::Normal
            },
            // Nothing pending at startup, and the spec has not been sent: the
            // first frame is where `apply_mode` sends it.
            pending_mode: None,
            panel_spec_applied: false,
            normal_geometry: None,
            panel_config,
            last_poll_set: Vec::new(),
            panel_menu_open: false,
            settings_open: false,
            settings_outcome: Default::default(),
            state,
            dbus_client,
            theme,
            system_tray,
            force_quit: false,
            hw_update_tx,
            hw_update_rx,
            shortcuts: KeyboardShortcuts::new(),
            startup_frames: 10,
            startup_apply_pending: true,
            startup_started: std::time::Instant::now(),
            startup_timeout_warned: false,
            startup_apply_reply: None,
            last_tray_profile,
            last_tray_profiles_count,
        }
    }
    
    fn handle_hardware_updates(&mut self) {
        // Re-arm the repaint latch: every update consumed here is now on screen,
        // so the next arrival is allowed to request its own frame. Clearing it
        // here (rather than at the request site) is what bounds the repaint
        // rate to one per frame.
        REPAINT_PENDING.store(false, Ordering::Release);

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
                    if let (Some(hw_min), Some(hw_max)) = (info.hw_min_freq, info.hw_max_freq) {
                        let changed = lapsphere_common::types::normalize_profiles_freq(
                            &mut self.state.config.profiles, hw_min, hw_max,
                        );
                        for (name, (old_min, old_max), r) in &changed {
                            log::warn!(
                                "profile '{}' CPU freq clamped: min {:?} -> {:?}, max {:?} -> {:?} (hw {}..{} kHz)",
                                name, old_min, r.min, old_max, r.max, hw_min, hw_max
                            );
                        }
                        if !changed.is_empty() {
                            if let Err(e) = save_profiles_to_disk(&self.state.config) {
                                log::warn!("failed to save normalized profiles: {}", e);
                            }
                        }
                        if self.startup_apply_pending {
                            self.startup_apply_pending = false;
                            if let (Some(profile), Some(client)) =
                                (self.state.current_profile().cloned(), &self.dbus_client)
                            {
                                self.startup_apply_reply = Some(client.apply_profile(profile));
                            }
                        }
                    }
                    self.state.cpu_info = Some(info);
                }
                HardwareUpdate::GpuInfo(info) => {
                    self.state.gpu_info = info;

                    // Auto-populate ranges and limits if we found them in the periodic update
                    if let Some(nvidia) = self.state.gpu_info.iter().find(|g| g.name.contains("NVIDIA")) {
                        if self.state.gpu_clock_ranges.is_none() {
                            if let Some(range) = nvidia.core_clock_range {
                                self.state.gpu_clock_ranges = Some(range);
                                self.state.gpu_clock_ranges_error = None;
                            }
                        }
                        if self.state.gpu_core_offset_limits.is_none() {
                            if let Some(limits) = nvidia.core_offset_limits {
                                self.state.gpu_core_offset_limits = Some(limits);
                                self.state.gpu_core_offset_error = None;
                            }
                        }
                        if self.state.gpu_mem_offset_limits.is_none() {
                            if let Some(limits) = nvidia.memory_offset_limits {
                                self.state.gpu_mem_offset_limits = Some(limits);
                                self.state.gpu_mem_offset_error = None;
                            }
                        }
                    }
                }
                HardwareUpdate::BatteryInfo(info) => {
                    self.state.battery_info = Some(info);
                }
                HardwareUpdate::WifiInfo(info) => {
                    self.state.wifi_info = info;
                }
                HardwareUpdate::GamepadInfo(connected_gamepads) => {
                    self.state.gamepad_info = connected_gamepads.clone();

                    // Single reconciliation point (see gamepad_registry): stable
                    // uids persist as before, volatile sysfs-path uids are
                    // session-scoped so reconnects cannot grow the database,
                    // and newly resolved stable identities adopt same-device
                    // rows instead of duplicating them.
                    if crate::gamepad_registry::reconcile(
                        &mut self.state.config.remembered_gamepads,
                        &connected_gamepads,
                    ) {
                        let _ = self.state.save_settings();
                    }
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
                HardwareUpdate::WebcamState(state) => {
                    self.state.webcam_enabled = Some(state);
                }
                HardwareUpdate::DaemonLogs(mut logs) => {
                    if !self.state.log_paused {
                        if logs.len() > 2000 {
                            logs.drain(0..logs.len() - 2000);
                        }
                        self.state.daemon_logs = logs.into();
                    }
                }
                HardwareUpdate::UpdateInfo(version, changelog) => {
                    self.state.new_version_available = Some(version);
                    self.state.latest_changelog = Some(changelog);
                }
                HardwareUpdate::GpuClockRanges(result) => {
                    match result {
                        Ok(ranges) => {
                            self.state.gpu_clock_ranges = Some(ranges);
                            self.state.gpu_clock_ranges_error = None;
                        }
                        Err(e) => self.state.gpu_clock_ranges_error = Some(e),
                    }
                }
                HardwareUpdate::GpuCoreOffsetLimits(result) => {
                    match result {
                        Ok(limits) => {
                            self.state.gpu_core_offset_limits = Some(limits);
                            self.state.gpu_core_offset_error = None;
                        }
                        Err(e) => self.state.gpu_core_offset_error = Some(e),
                    }
                }
                HardwareUpdate::GpuMemOffsetLimits(result) => {
                    match result {
                        Ok(limits) => {
                            self.state.gpu_mem_offset_limits = Some(limits);
                            self.state.gpu_mem_offset_error = None;
                        }
                        Err(e) => self.state.gpu_mem_offset_error = Some(e),
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
        
        if self.startup_apply_pending
            && !self.startup_timeout_warned
            && self.startup_started.elapsed() >= std::time::Duration::from_secs(10)
        {
            self.startup_timeout_warned = true;
            log::warn!("стартовое применение профиля ждёт пределы CPU от демона, профиль не применён");
        }
        if let Some(mut rx) = self.startup_apply_reply.take() {
            match rx.try_recv() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::warn!("startup apply of current profile failed: {}", e),
                Err(oneshot::error::TryRecvError::Empty) => self.startup_apply_reply = Some(rx),
                Err(oneshot::error::TryRecvError::Closed) => {
                    log::warn!("startup apply reply channel closed");
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
    
    fn draw_top_bar(&mut self, ui: &mut egui::Ui) {
        let mut dismiss_update = false;
        if let Some(version) = &self.state.new_version_available {
            let version = version.clone();
            egui::Panel::top("update_banner").show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(RichText::new(format!("🚀 Update Available: v{}", version)).strong().color(egui::Color32::from_rgb(255, 200, 0)));
                    if ui.link("View Details").clicked() {
                        self.state.current_page = Page::Settings;
                        self.state.settings_tab = crate::app::SettingsTab::About;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Dismiss").clicked() {
                            dismiss_update = true;
                        }
                    });
                });
            });
        }

        if dismiss_update {
            self.state.new_version_available = None;
        }

        egui::Panel::top("top_bar").show_inside(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);

                let time_str = Local::now().format("%H:%M:%S").to_string();
                let date_str = Local::now().format("%Y-%m-%d").to_string();
                let profile_str = format!("Profile: {}", self.state.config.current_profile);
                let base_size = TextStyle::Small.resolve(&ui.global_style()).size;
                let top_bar_size = base_size + 1.0;
                let mono_font = FontId::new(top_bar_size, FontFamily::Monospace);
                let text_color = ui.visuals().text_color();
                let text_font = FontId::new(top_bar_size, FontFamily::Proportional);
                let right_width = ui.ctx().fonts_mut(|fonts| {
                    let time_width = fonts.layout_no_wrap(time_str.clone(), mono_font.clone(), text_color).size().x;
                    let date_width = fonts.layout_no_wrap(date_str.clone(), mono_font.clone(), text_color).size().x;
                    let profile_width = fonts.layout_no_wrap(profile_str.clone(), text_font.clone(), text_color).size().x;
                    time_width.max(date_width).max(profile_width)
                }) + 16.0;
                let tabs_width = (ui.available_width() - right_width).max(0.0);

                ui.allocate_ui_with_layout(
                    egui::vec2(tabs_width, ui.available_height()),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.horizontal_centered(|ui| {
                            ui.selectable_value(
                                &mut self.state.current_page,
                                Page::Statistics,
                                "📊 Statistics",
                            );
                            ui.selectable_value(
                                &mut self.state.current_page,
                                Page::Profiles,
                                "📋 Profiles",
                            );
                            ui.selectable_value(
                                &mut self.state.current_page,
                                Page::Tuning,
                                "🔧 Tuning",
                            );
                            ui.selectable_value(
                                &mut self.state.current_page,
                                Page::Settings,
                                "⚙ Settings",
                            );
                            if ui.button("📈 Panel").clicked() {
                                self.pending_mode = Some(Mode::Panel);
                            }
                            if ui.button("❓ Help").clicked() {
                                self.shortcuts.toggle_help();
                            }
                        });
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
                egui::Panel::top("status_bar").show_inside(ui, |ui| {
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

    /// Draw the panel body and its right-click menu.
    ///
    /// The whole surface is the right-click target, so the menu opens wherever
    /// the user clicks. `CentralPanel::frame` is left at its default here: the
    /// opaque background and its transparency are PR 2's work, and adding a
    /// `Frame` now would only make the later change harder to read.
    fn draw_panel(&mut self, ui: &mut egui::Ui, ctx: &Context) {
        // The interactive area is the whole panel surface, registered with the
        // ordinary egui path so the menu opens from the Response.
        //
        // `Sense::click_and_drag()` rather than `click()`: dragging the panel
        // around is PR 2's work, and the sense has to be declared before the
        // code that uses it, not changed later underneath it. `click()` today
        // would behave identically for a right click.
        let mut menu_action = crate::panel::menu::ContextAction::None;

        CentralPanel::default().show_inside(ui, |ui| {
            crate::panel::render::draw(ui, &self.state, &self.panel_config);
            let response = ui.interact(
                ui.max_rect(),
                ui.id().with("panel_surface"),
                egui::Sense::click_and_drag(),
            );

            // The right click opens the menu through the ordinary egui path.
            // A left click on the surface closes it rather than opening it.
            response.context_menu(|ui| {
                menu_action = crate::panel::menu::draw_context_menu(ui);
            });
            if response.clicked() {
                self.panel_menu_open = false;
            }
        });

        self.panel_menu_open = self.panel_menu_open || menu_action != crate::panel::menu::ContextAction::None;

        match menu_action {
            crate::panel::menu::ContextAction::OpenSettings => {
                self.panel_menu_open = false;
                self.settings_open = true;
            }
            crate::panel::menu::ContextAction::BackToNormal => {
                self.panel_menu_open = false;
                self.pending_mode = Some(Mode::Normal);
            }
            crate::panel::menu::ContextAction::None => {}
        }

        self.draw_settings_window(ctx);
        self.apply_settings_outcome(ctx);
    }

    /// The settings window: a SEPARATE viewport, so it is a real OS window and is
    /// never clipped by the 300x198 panel.
    ///
    /// An ordinary window on purpose -- no always-on-top, no skip-taskbar, not
    /// tied to panel mode. A settings dialog floating over a game would be the
    /// same mistake the panel itself is.
    fn draw_settings_window(&mut self, ctx: &Context) {
        if !self.settings_open {
            return;
        }

        // The documented close protocol (egui 0.34 `show_viewport_immediate`):
        // "You can check if the user wants to close the viewport by checking the
        // `ViewportInfo::close_requested` flags". The caller must STOP showing it.
        //
        // The previous check here was `ctx.viewport_for(.., |_| true)`, which can
        // never be false and so never closed anything: the WM close button left
        // `settings_open` set and we went on asking for a viewport the user had
        // already dismissed.
        //
        // Scoped to the settings viewport id on purpose: the root viewport's own
        // `close_requested` is the window-close path and is handled elsewhere, so
        // closing the settings window must not reach the panel.
        let close_requested =
            ctx.input_for(settings_viewport(), |input| input.viewport().close_requested());
        if close_requested {
            self.settings_open = false;
            ctx.send_viewport_cmd_to(settings_viewport(), egui::ViewportCommand::Close);
            return;
        }

        let mut outcome = crate::panel::menu::SettingsOutcome::default();
        let mut config = std::mem::take(&mut self.panel_config);
        ctx.show_viewport_immediate(
            settings_viewport(),
            egui::ViewportBuilder::default()
                .with_inner_size(SETTINGS_SIZE)
                .with_title("LapSphere \u{2014} Panel settings")
                .with_resizable(true),
            |ui, _class| {
                crate::panel::menu::draw_settings_body(ui, &self.state, &mut config, &mut outcome);
            },
        );
        self.panel_config = config;
        self.settings_outcome = outcome;
    }

    /// Apply what the settings window asked for.
    fn apply_settings_outcome(&mut self, ctx: &Context) {
        let outcome = std::mem::take(&mut self.settings_outcome);
        if outcome == Default::default() {
            return;
        }
        if outcome.back_to_normal {
            self.pending_mode = Some(Mode::Normal);
        }
        if outcome.config_changed {
            if let Err(err) = crate::panel::save_panel_config(&self.panel_config) {
                log::warn!("could not write {}: {err}", crate::panel::PANEL_CONFIG_FILE);
            }
        }
        if outcome.width_changed {
            // The width is read fresh from the config every frame, so
            // re-sending the spec is all "applies immediately" needs -- and it
            // happens while the settings window is still open.
            self.panel_spec_applied = false;
            ctx.request_repaint();
        }
        if outcome.close_requested {
            // Same path as the WM close button: drop the flag so the next frame
            // stops showing the viewport, then ask for the close.
            self.settings_open = false;
            ctx.send_viewport_cmd_to(settings_viewport(), egui::ViewportCommand::Close);
        }
    }

    /// Narrow the coordinator's poll set to what the current surface draws.
    ///
    /// In panel mode only the components backing a VISIBLE row are polled; `logs`
    /// alone is a ~470 kB reply every 5 s that no panel row reads. In normal mode
    /// everything is polled, as before. Re-sent only when the set changes.
    fn sync_poll_set(&mut self) {
        let wanted: Vec<String> = if self.mode.is_panel() {
            crate::panel::window::required_components(&self.panel_config)
                .into_iter()
                .map(|name| name.to_string())
                .collect()
        } else {
            Vec::new()
        };

        if wanted == self.last_poll_set {
            return;
        }
        let Some(handle) = self.state.coordinator_handle.clone() else {
            return;
        };

        // An empty set in normal mode must NOT pause anything. `set_poll_set(vec![])`
        // pauses everything, which is correct for a panel with no rows enabled but
        // would freeze the main window for ever after one visit to the panel.
        let result = if wanted.is_empty() {
            handle.clear_poll_set()
        } else {
            handle.set_poll_set(wanted.clone())
        };
        if let Err(err) = result {
            log::debug!("could not update the poll set: {err}");
            return;
        }
        self.last_poll_set = wanted;
    }

    /// Push the panel window spec when the mode requires it.
    ///
    /// The decision is `panel::window::mode_to_apply`, a pure function, so the
    /// first-frame case (a process started with `--panel`) is reachable and
    /// covered by a test rather than being dead code behind a `take()?`.
    fn apply_mode(&mut self, ctx: &Context) {
        let target = crate::panel::window::mode_to_apply(
            self.mode,
            self.pending_mode.take(),
            self.panel_spec_applied,
        );
        let Some(target) = target else {
            return;
        };

        // Capture the normal geometry before the first switch away from it, so
        // returning restores the user's own window size.
        if self.mode == Mode::Normal {
            self.normal_geometry = Some(crate::panel::window::NORMAL_INNER);
        }

        let spec = match target {
            Mode::Panel => {
                let font_row_height = self.panel_font_row_height(ctx);
                crate::panel::window::panel_spec(&self.panel_config, font_row_height)
            }
            Mode::Normal => crate::panel::window::normal_spec(self.normal_geometry),
        };

        crate::panel::window::apply_spec(ctx, spec);
        self.mode = target;
        self.panel_spec_applied = true;
    }

    /// The font row height the panel layout is computed against.
    ///
    /// Read from egui's own metrics for the panel font, so the computed height
    /// tracks the user's font-size setting. A fixed approximation would drift
    /// from the painted text and the window would no longer fit its content.
    fn panel_font_row_height(&self, ctx: &Context) -> f32 {
        let font = egui::FontId::new(
            egui::TextStyle::Body.resolve(&ctx.global_style()).size,
            egui::FontFamily::Monospace,
        );
        ctx.fonts_mut(|fonts| fonts.row_height(&font))
    }

    fn handle_tray_events(&mut self, ctx: &Context) {
        let Some(tray) = self.system_tray.as_mut() else {
            return;
        };

        // Sync profile list if count changed
        if self.state.config.profiles.len() != self.last_tray_profiles_count {
            tray.set_profiles(&self.state.config.profiles);
            self.last_tray_profiles_count = self.state.config.profiles.len();
            // Force current profile sync as well since menu rebuilt
            tray.set_current_profile(&self.state.config.current_profile);
            self.last_tray_profile = self.state.config.current_profile.clone();
        }

        // Sync current profile if changed in main window
        if self.state.config.current_profile != self.last_tray_profile {
            tray.set_current_profile(&self.state.config.current_profile);
            self.last_tray_profile = self.state.config.current_profile.clone();
        }

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
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Tell the log-ring poll gate what is actually on screen (see
        // `should_fetch_logs`): a hidden/minimized window stops painting, so the
        // ring stops being fetched without any extra teardown logic.
        note_ui_frame(self.state.current_page, self.state.settings_tab);

        if self.startup_frames > 0 {
            let start_in_tray = std::env::args().any(|arg| arg == "--tray");
            if self.state.config.start_minimized || start_in_tray {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
            self.startup_frames -= 1;
        }

        // Handle keyboard shortcuts
        self.shortcuts.handle_shortcuts(&ctx, &mut self.state);
        
        // Handle background hardware updates
        self.handle_hardware_updates();

        // Act on a mode request from the top of the frame, before anything is
        // drawn: viewport commands issued mid-click are not safe, and the panel
        // body must be laid out in a window that already has the panel's size.
        self.apply_mode(&ctx);
        self.sync_poll_set();

        self.state.clamp_fan_selection();

        self.handle_tray_events(&ctx);
        
        if ctx.input(|input| input.viewport().close_requested())
            && self.state.config.tray_enabled
            && !self.force_quit
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        if self.mode.is_panel() {
            self.draw_panel(ui, &ctx);
            ctx.request_repaint_after(Duration::from_millis(500));
            return;
        }

        // Draw top bar
        self.draw_top_bar(ui);
        
        // Update theme if it's Auto to react to system theme changes
        if self.state.config.theme == Theme::Auto {
            let is_dark = ctx.global_style().visuals.dark_mode;
            if is_dark != self.theme.visuals.dark_mode {
                self.theme = LapSphereTheme::new(&self.state.config.theme, is_dark);
                self.theme.apply_with_font_size(&ctx, &self.state.config.font_size);
            }
        }

        // Draw main content
        CentralPanel::default().show_inside(ui, |ui| match self.state.current_page {
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
                settings::draw(
                    ui,
                    &mut self.state,
                    &mut self.theme,
                    &ctx,
                    self.dbus_client.as_ref(),
                );
            }
        });
        
        // Request repaint if there are pending updates
        ctx.request_repaint_after(Duration::from_millis(500));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(client) = &self.dbus_client {
            let client = client.clone();
            // Use a fresh runtime for shutdown to avoid potential nesting issues
            // and ensure commands are processed before the main runtime closes.
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
                let _ = rt.block_on(async move {
                    let _ = tokio::time::timeout(Duration::from_secs(2), client.set_all_fans_auto()).await;
                    let _ = tokio::time::timeout(Duration::from_secs(2), client.shutdown_daemon()).await;
                });
            }
        }
    }
}

/// Panel/window-level settings persisted to `settings.json`.
///
/// The two tray fields (`start_minimized`, `tray_enabled`) are no longer part
/// of this file — they are persisted to `tray.json` by `TrayConfig` (see
/// `gui/src/tray_config.rs`). `AppConfig` still carries them at runtime; this
/// struct is only the on-disk projection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct SettingsConfig {
    theme: Theme,
    autostart: bool,
    cpu_scheduler: String,
    font_size: FontSize,
    statistics_sections: StatisticsSections,
    tuning_section_order: Vec<String>,
    battery_settings: BatterySettings,
    log_limit: usize,
    log_filter_trace: bool,
    remembered_gamepads: Vec<GamepadInfo>,
}

impl Default for SettingsConfig {
    fn default() -> Self {
        let config = AppConfig::default();
        Self {
            theme: config.theme,
            autostart: config.autostart,
            cpu_scheduler: config.cpu_scheduler,
            font_size: config.font_size,
            statistics_sections: config.statistics_sections,
            tuning_section_order: config.tuning_section_order,
            battery_settings: config.battery_settings,
            log_limit: config.log_limit,
            log_filter_trace: config.log_filter_trace,
            remembered_gamepads: config.remembered_gamepads.clone(),
        }
    }
}

impl From<&AppConfig> for SettingsConfig {
    fn from(config: &AppConfig) -> Self {
        Self {
            theme: config.theme.clone(),
            autostart: config.autostart,
            cpu_scheduler: config.cpu_scheduler.clone(),
            font_size: config.font_size.clone(),
            statistics_sections: config.statistics_sections.clone(),
            tuning_section_order: config.tuning_section_order.clone(),
            battery_settings: config.battery_settings.clone(),
            log_limit: config.log_limit,
            log_filter_trace: config.log_filter_trace,
            remembered_gamepads: config.remembered_gamepads.clone(),
        }
    }
}

impl SettingsConfig {
    fn apply_to(&self, config: &mut AppConfig) {
        config.theme = self.theme.clone();
        config.autostart = self.autostart;
        config.cpu_scheduler = self.cpu_scheduler.clone();
        config.font_size = self.font_size.clone();
        config.statistics_sections = self.statistics_sections.clone();
        config.tuning_section_order = self.tuning_section_order.clone();
        config.battery_settings = self.battery_settings.clone();
        config.log_limit = self.log_limit;
        config.log_filter_trace = self.log_filter_trace;
        config.remembered_gamepads = self.remembered_gamepads.clone();
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

pub fn get_config_dir() -> String {
    if cfg!(target_os = "windows") {
        let app_data = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        format!("{}/lapsphere", app_data.replace("\\", "/"))
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        format!("{}/.config/lapsphere", home)
    }
}

pub fn get_crash_dir() -> String {
    get_config_dir()
}

pub fn load_config_from_disk() -> anyhow::Result<AppConfig> {
    load_config_from_dir(&get_config_dir())
}

/// Same as [`load_config_from_disk`] but against an explicit config directory.
///
/// Split out so the migration can be exercised end-to-end in tests without
/// touching the real `~/.config/lapsphere` (or `HOME`, which is process-global
/// and therefore racy across parallel tests).
pub fn load_config_from_dir(config_dir: &str) -> anyhow::Result<AppConfig> {
    let config_dir = config_dir.to_string();
    let settings_path = format!("{}/settings.json", config_dir);
    let profiles_path = format!("{}/profiles.json", config_dir);
    let legacy_path = format!("{}/config.json", config_dir);

    let legacy_config = if Path::new(&legacy_path).exists() {
        let json = std::fs::read_to_string(&legacy_path)?;
        Some(serde_json::from_str::<AppConfig>(&json)?)
    } else {
        None
    };

    // Tray settings: resolve runs load -> migrate -> legacy fallback in that
    // order (see `resolve_tray_config`). The load first matters: it quarantines
    // a corrupt tray.json, which then lets the migration recover the values
    // from settings.json instead of falling back to defaults.
    let tray = tray_config::resolve_tray_config(&config_dir, legacy_config.as_ref());

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
    // tray.json is authoritative: it is the file this build owns, and
    // `resolve_tray_config` has already folded any settings.json values into
    // it when that file did not exist.
    tray.apply_to(&mut config);

    if config.start_minimized {
        config.tray_enabled = true;
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
    save_settings_to_dir(&get_config_dir(), config)
}

/// Same as [`save_settings_to_disk`] but against an explicit config directory.
fn save_settings_to_dir(config_dir: &str, config: &AppConfig) -> anyhow::Result<()> {
    let config_dir = config_dir.to_string();
    std::fs::create_dir_all(&config_dir)?;
    let settings_path = format!("{}/settings.json", config_dir);
    let json = serde_json::to_string_pretty(&SettingsConfig::from(config))?;
    tray_config::write_atomic(&settings_path, &json)?;

    // Tray settings live in their own file. Both writes are atomic, so a reader
    // (including our own next start) never sees a half-written document.
    tray_config::save_tray_config(&config_dir, config)?;

    // Push the new poll rates to the daemon so job intervals update live
    // (fire-and-forget; the daemon keeps its own defaults when this fails
    // or no daemon connection exists yet, e.g. during startup migration).
    if let Some(client) = crate::dbus_client::shared_client() {
        if let Ok(sections) = serde_json::to_string(&config.statistics_sections) {
            let _ = client.sync_daemon_poll_settings(sections);
        }
    }

    // Handle autostart
    #[cfg(target_os = "linux")]
    {
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
        } else {
            // Write a desktop file that explicitly disables autostart to override system-wide one
            std::fs::create_dir_all(&autostart_dir)?;
            let content = format!(
                "[Desktop Entry]\n\
                Type=Application\n\
                Name=LapSphere\n\
                Exec=lapsphere --tray\n\
                Icon=lapsphere\n\
                X-GNOME-Autostart-enabled=false\n\
                NoDisplay=true\n\
                Hidden=true\n"
            );
            std::fs::write(&desktop_file, content)?;
        }
    }

    Ok(())
}

fn save_profiles_to_disk(config: &AppConfig) -> anyhow::Result<()> {
    let config_dir = get_config_dir();
    std::fs::create_dir_all(&config_dir)?;
    let profiles_path = format!("{}/profiles.json", config_dir);
    let json = serde_json::to_string_pretty(&ProfilesConfig::from(config))?;
    std::fs::write(profiles_path, json)?;
    Ok(())
}


#[cfg(test)]
mod tray_migration_e2e_tests {
    use super::*;
    use crate::tray_config::{SETTINGS_PRE_SPLIT_BACKUP, TRAY_CONFIG_CORRUPT, TRAY_CONFIG_FILE};
    use std::path::PathBuf;

    /// Per-test scratch config dir, removed on drop. `load_config_from_dir`
    /// exists precisely so these can run against a real directory without
    /// touching `HOME` (process-global, racy across parallel tests).
    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "lapsphere-traye2e-{}-{}-{}",
                std::process::id(),
                name,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> String {
            self.0.to_string_lossy().into_owned()
        }

        fn write(&self, name: &str, contents: &str) {
            std::fs::write(self.0.join(name), contents).unwrap();
        }

        fn read(&self, name: &str) -> String {
            std::fs::read_to_string(self.0.join(name)).unwrap()
        }

        fn exists(&self, name: &str) -> bool {
            self.0.join(name).exists()
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A pre-split settings.json: panel settings plus the two tray fields.
    const LEGACY_SETTINGS: &str = r#"{
  "theme": "Dark",
  "start_minimized": true,
  "tray_enabled": true,
  "autostart": false,
  "font_size": "Large"
}"#;

    /// The shape `settings.json` had before this split — what a rolled-back
    /// build deserializes. Declared here so the rollback claim is checked
    /// against the old contract, not against the new struct.
    #[derive(serde::Deserialize)]
    #[serde(default)]
    struct PreSplitSettings {
        theme: Theme,
        start_minimized: bool,
        tray_enabled: bool,
        autostart: bool,
    }

    impl Default for PreSplitSettings {
        fn default() -> Self {
            Self {
                theme: Theme::Auto,
                start_minimized: false,
                tray_enabled: false,
                autostart: false,
            }
        }
    }

    /// Check 1: a clean configuration directory.
    #[test]
    fn clean_config_dir_loads_without_touching_the_disk() {
        let dir = TestDir::new("clean");

        let config = load_config_from_dir(&dir.path()).unwrap();

        assert!(!config.tray_enabled);
        assert!(!config.start_minimized);
        assert!(!dir.exists(TRAY_CONFIG_FILE));
        assert!(!dir.exists(SETTINGS_PRE_SPLIT_BACKUP));
    }

    /// The write path, against a real directory: enabling the tray must land in
    /// tray.json and leave settings.json free of tray keys — the split the
    /// separate panel.json change depends on.
    #[test]
    fn saving_writes_tray_json_and_keeps_settings_json_free_of_tray_keys() {
        let dir = TestDir::new("save-split");
        let mut config = load_config_from_dir(&dir.path()).unwrap();
        config.tray_enabled = true;

        save_settings_to_dir(&dir.path(), &config).unwrap();

        assert!(dir.exists(TRAY_CONFIG_FILE));
        assert!(dir.read(TRAY_CONFIG_FILE).contains(r#""tray_enabled": true"#));
        let settings = dir.read("settings.json");
        assert!(!settings.contains("tray_enabled"));
        assert!(!settings.contains("start_minimized"));

        // And it round-trips through the real load path.
        let reloaded = load_config_from_dir(&dir.path()).unwrap();
        assert!(reloaded.tray_enabled);
        assert!(!reloaded.start_minimized);
    }

    /// Check 2: a configuration that still has the old fields.
    #[test]
    fn legacy_config_dir_is_migrated_on_first_load() {
        let dir = TestDir::new("legacy");
        dir.write("settings.json", LEGACY_SETTINGS);

        let config = load_config_from_dir(&dir.path()).unwrap();

        assert!(config.start_minimized, "start minimized must survive the split");
        assert!(config.tray_enabled);
        // The start_minimized -> tray_enabled dependency still holds after load.
        assert!(config.tray_enabled);

        assert!(dir.exists(TRAY_CONFIG_FILE));
        assert!(dir.exists(SETTINGS_PRE_SPLIT_BACKUP));
        // Byte-for-byte: that is what makes the rollback below work.
        assert_eq!(dir.read(SETTINGS_PRE_SPLIT_BACKUP), LEGACY_SETTINGS);

        // The tray keys are gone from settings.json, panel settings are not.
        let settings = dir.read("settings.json");
        assert!(!settings.contains("tray_enabled"));
        assert!(!settings.contains("start_minimized"));
        assert!(settings.contains(r#""theme": "Dark"#));
    }

    /// Check 3: repeat runs change nothing.
    #[test]
    fn second_and_third_load_change_nothing() {
        let dir = TestDir::new("repeat");
        dir.write("settings.json", LEGACY_SETTINGS);

        load_config_from_dir(&dir.path()).unwrap();
        let tray = dir.read(TRAY_CONFIG_FILE);
        let settings = dir.read("settings.json");
        let backup = dir.read(SETTINGS_PRE_SPLIT_BACKUP);

        for _ in 0..3 {
            let config = load_config_from_dir(&dir.path()).unwrap();
            assert!(config.tray_enabled);
            assert!(config.start_minimized);
        }

        assert_eq!(dir.read(TRAY_CONFIG_FILE), tray, "tray.json rewritten");
        assert_eq!(
            dir.read("settings.json"),
            settings,
            "settings.json rewritten"
        );
        assert_eq!(
            dir.read(SETTINGS_PRE_SPLIT_BACKUP),
            backup,
            "backup overwritten"
        );
    }

    /// Check 4: rolling back to a pre-split build.
    #[test]
    fn the_backup_still_satisfies_a_pre_split_reader() {
        let dir = TestDir::new("rollback");
        dir.write("settings.json", LEGACY_SETTINGS);
        load_config_from_dir(&dir.path()).unwrap();

        // What an older build reads after `cp settings.json.pre-tray-split settings.json`.
        let legacy_view: PreSplitSettings =
            serde_json::from_str(&dir.read(SETTINGS_PRE_SPLIT_BACKUP)).unwrap();

        assert!(legacy_view.start_minimized);
        assert!(legacy_view.tray_enabled);
        assert_eq!(legacy_view.theme, Theme::Dark);
    }

    /// A corrupt tray.json on an already-migrated dir is the one case that
    /// falls back to defaults, because settings.json no longer holds the values.
    /// The guarantee there is recoverability, not silent loss.
    #[test]
    fn a_corrupt_tray_json_is_quarantined_and_the_values_stay_recoverable() {
        let dir = TestDir::new("corrupt");
        dir.write("settings.json", LEGACY_SETTINGS);
        load_config_from_dir(&dir.path()).unwrap();
        // User breaks the new file by hand afterwards.
        dir.write(TRAY_CONFIG_FILE, "{ oops");

        let config = load_config_from_dir(&dir.path()).unwrap();
        assert!(!config.tray_enabled);
        assert!(!config.start_minimized);

        // Quarantined byte-for-byte, never deleted or overwritten in place.
        assert_eq!(dir.read(TRAY_CONFIG_CORRUPT), "{ oops");
        assert!(!dir.exists(TRAY_CONFIG_FILE));

        // The backup still satisfies a rolled-back build.
        let backup: PreSplitSettings =
            serde_json::from_str(&dir.read(SETTINGS_PRE_SPLIT_BACKUP)).unwrap();
        assert!(backup.tray_enabled);
        assert!(backup.start_minimized);
    }

    /// A corrupt tray.json BEFORE any migration is fully recoverable: the
    /// quarantine step leaves no tray.json, so the migration then recovers the
    /// values from settings.json.
    #[test]
    fn a_corrupt_tray_json_before_any_migration_recovers_the_values() {
        let dir = TestDir::new("corrupt-pre-migration");
        dir.write("settings.json", LEGACY_SETTINGS);
        dir.write(TRAY_CONFIG_FILE, "}}}");

        let config = load_config_from_dir(&dir.path()).unwrap();

        assert!(config.tray_enabled, "recovered from settings.json");
        assert!(config.start_minimized);
        assert!(dir.exists(TRAY_CONFIG_FILE), "and re-persisted");
    }
}

#[cfg(test)]
mod log_fetch_gate_tests {
    use super::*;

    // The daemon log ring is the single most expensive polling reply (~470 kB of
    // JSON), so it must only be fetched while it is on screen.
    #[test]
    fn logs_fetched_only_on_the_logs_tab() {
        assert!(logs_fetch_needed(Page::Settings, SettingsTab::Logs, 0));
        for tab in [
            SettingsTab::Main,
            SettingsTab::StatsConfiguration,
            SettingsTab::Hardware,
            SettingsTab::Help,
            SettingsTab::About,
        ] {
            assert!(
                !logs_fetch_needed(Page::Settings, tab, 0),
                "Logs must not be polled from the {:?} tab",
                tab
            );
        }
        for page in [Page::Statistics, Page::Profiles, Page::Tuning] {
            assert!(
                !logs_fetch_needed(page, SettingsTab::Logs, 0),
                "Logs must not be polled from the {:?} page",
                page
            );
        }
    }

    #[test]
    fn logs_not_fetched_when_the_window_stops_painting() {
        // Tray-only / minimized / hidden window: no frames, nothing being read.
        assert!(!logs_fetch_needed(Page::Settings, SettingsTab::Logs, UI_FRAME_FRESH_MS + 1));
        assert!(!logs_fetch_needed(Page::Settings, SettingsTab::Logs, 60_000));
        // A window that is painting normally keeps the ring flowing.
        assert!(logs_fetch_needed(Page::Settings, SettingsTab::Logs, UI_FRAME_FRESH_MS));
    }

    #[test]
    fn should_fetch_logs_follows_the_recorded_frame() {
        // No frame recorded yet (startup): nothing is on screen, no polling.
        LOGS_TAB_ON_SCREEN.store(false, Ordering::Relaxed);
        LAST_UI_FRAME_MS.store(now_ms(), Ordering::Relaxed);
        assert!(!should_fetch_logs());

        // Frame on the Logs tab: poll.
        note_ui_frame(Page::Settings, SettingsTab::Logs);
        assert!(should_fetch_logs());

        // Frame elsewhere: stop.
        note_ui_frame(Page::Statistics, SettingsTab::Logs);
        assert!(!should_fetch_logs());

        // Frame on the Logs tab, but paint stopped > freshness window ago.
        note_ui_frame(Page::Settings, SettingsTab::Logs);
        LAST_UI_FRAME_MS.store(now_ms() - (UI_FRAME_FRESH_MS + 1), Ordering::Relaxed);
        assert!(!should_fetch_logs());
    }

    #[test]
    fn repaint_requests_are_coalesced_into_one_per_frame() {
        let ctx = Context::default();

        // A burst of arrivals with no frame in between costs one request: the
        // latch stays set, so later arrivals do not re-request.
        REPAINT_PENDING.store(false, Ordering::Release);
        assert!(!REPAINT_PENDING.swap(true, Ordering::AcqRel), "latch armed");
        for _ in 0..10 {
            request_repaint_now(&ctx);
        }
        assert!(REPAINT_PENDING.load(Ordering::Acquire), "latch still set");

        // Consuming a frame re-arms it, so the next arrival requests again.
        REPAINT_PENDING.store(false, Ordering::Release);
        assert!(!REPAINT_PENDING.load(Ordering::Acquire));
    }
}
