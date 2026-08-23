//! Daemon polling-interval settings, synced with the GUI's per-section rates.
//!
//! The GUI persists user-configured poll rates in
//! `~/.config/lapsphere/settings.json` under `statistics_sections`. This
//! module mirrors ONLY the fields the daemon needs, with defaults equal to
//! the previously hardcoded intervals (1 s monitor / 2 s fan / 1 s GPU OC),
//! so a headless daemon or an old/partial settings file keeps the exact
//! pre-refactor behavior.
//!
//! Field mapping (GUI rate -> daemon PollJob):
//! - `cpu_poll_rate`      \
//! - `gpu_poll_rate`       > hardware_monitor = min(cpu, gpu): the job fills
//!                        /    one shared HARDWARE_CACHE consumed by both
//!                       /     sections, so it must run at the faster rate.
//! - `fans_poll_rate`          -> fan_control
//! - `gpu_overclock_poll_rate` -> gpu_overclock
//!
//! Remaining per-section rates (memory, battery, wifi, storage, gamepad) are
//! GUI-side RefreshCoordinator concerns and intentionally not mirrored here.
//!
//! RTD3 composition: the hybrid NVML gating inside hardware_detection is
//! interval-independent — suspended/idle dGPU ticks never touch NVML, so
//! arbitrarily fast user-configured rates cannot wake a sleeping GPU.

use crate::polling_scheduler::SchedulerHandle;
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use std::time::Duration;

// Legacy intervals — also the fallback defaults everywhere below.
pub const DEFAULT_HARDWARE_MONITOR_MS: u64 = 1000;
pub const DEFAULT_FAN_CONTROL_MS: u64 = 2000;
pub const DEFAULT_GPU_OVERCLOCK_MS: u64 = 1000;

/// Minimal mirror of the GUI `StatisticsSections` fields the daemon consumes.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DaemonPollSettings {
    pub hardware_monitor_ms: u64,
    pub fan_control_ms: u64,
    pub gpu_overclock_ms: u64,
}

impl Default for DaemonPollSettings {
    fn default() -> Self {
        Self {
            hardware_monitor_ms: DEFAULT_HARDWARE_MONITOR_MS,
            fan_control_ms: DEFAULT_FAN_CONTROL_MS,
            gpu_overclock_ms: DEFAULT_GPU_OVERCLOCK_MS,
        }
    }
}

/// Subset of GUI `StatisticsSections` we read from settings.json.
#[derive(Debug, Deserialize)]
#[serde(default)]
struct StatisticsSectionsMirror {
    cpu_poll_rate: u64,
    memory_poll_rate: u64,
    gpu_poll_rate: u64,
    fans_poll_rate: u64,
    gpu_overclock_poll_rate: u64,
}

impl Default for StatisticsSectionsMirror {
    fn default() -> Self {
        // Matches lapsphere_common StatisticsSections::default().
        Self {
            cpu_poll_rate: 1000,
            memory_poll_rate: 1000,
            gpu_poll_rate: 2000,
            fans_poll_rate: 1000,
            gpu_overclock_poll_rate: DEFAULT_GPU_OVERCLOCK_MS,
        }
    }
}

#[derive(Debug, Deserialize)]
struct SettingsFileMirror {
    statistics_sections: StatisticsSectionsMirror,
}

impl From<&StatisticsSectionsMirror> for DaemonPollSettings {
    fn from(s: &StatisticsSectionsMirror) -> Self {
        Self {
            // hardware_monitor feeds CPU + Memory + GPU (+ everything else)
            // from ONE shared cache, so honor the fastest consumer rate.
            hardware_monitor_ms: s.cpu_poll_rate.min(s.gpu_poll_rate).max(50),
            fan_control_ms: s.fans_poll_rate.max(50),
            gpu_overclock_ms: s.gpu_overclock_poll_rate.max(50),
        }
    }
}

fn settings_path() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .map(|home| format!("{home}/.config/lapsphere/settings.json"))
}

impl DaemonPollSettings {
    /// Load from the GUI settings.json; fall back to legacy defaults when the
    /// file is missing, unreadable, or lacks the statistics_sections object.
    /// Never fails: any problem logs a warning and returns defaults.
    pub fn load() -> Self {
        let Some(path) = settings_path() else {
            log::debug!("No HOME set; using legacy daemon poll intervals");
            return Self::default();
        };

        let json = match std::fs::read_to_string(&path) {
            Ok(json) => json,
            Err(e) => {
                log::debug!(
                    "No GUI settings file at {path} ({e}); using legacy daemon poll intervals"
                );
                return Self::default();
            }
        };

        let parsed: Result<SettingsFileMirror, _> = serde_json::from_str(&json);
        match parsed {
            Ok(file) => {
                let settings: Self = (&file.statistics_sections).into();
                log::info!("Daemon poll intervals synced from {path}: {:?}", settings);
                settings
            }
            Err(e) => {
                log::warn!(
                    "Failed to parse statistics_sections from {path} ({e}); \
                     using legacy daemon poll intervals"
                );
                Self::default()
            }
        }
    }

    pub fn hardware_monitor(&self) -> Duration {
        Duration::from_millis(self.hardware_monitor_ms)
    }

    pub fn fan_control(&self) -> Duration {
        Duration::from_millis(self.fan_control_ms)
    }

    pub fn gpu_overclock(&self) -> Duration {
        Duration::from_millis(self.gpu_overclock_ms)
    }
}

/// Compute which job intervals differ between two setting sets.
/// Pure function so the diff behavior is unit-testable without a scheduler.
pub fn compute_interval_updates(
    prev: &DaemonPollSettings,
    next: &DaemonPollSettings,
) -> Vec<(&'static str, Duration)> {
    let mut updates = Vec::new();
    if prev.hardware_monitor_ms != next.hardware_monitor_ms {
        updates.push(("hardware_monitor", next.hardware_monitor()));
    }
    if prev.fan_control_ms != next.fan_control_ms {
        updates.push(("fan_control", next.fan_control()));
    }
    if prev.gpu_overclock_ms != next.gpu_overclock_ms {
        updates.push(("gpu_overclock", next.gpu_overclock()));
    }
    updates
}

/// Apply `next` globally: store it and push changed intervals to the running
/// scheduler. Returns the list of applied job updates (for logging/tests).
pub fn sync_from(
    state: &Arc<Mutex<DaemonPollSettings>>,
    handle: Option<&SchedulerHandle>,
    next: DaemonPollSettings,
) -> Vec<(String, Duration)> {
    let applied: Vec<(String, Duration)> = {
        let current = state.lock().unwrap();
        compute_interval_updates(&current, &next)
            .into_iter()
            .map(|(id, d)| (id.to_string(), d))
            .collect()
    };

    *state.lock().unwrap() = next;

    if let Some(handle) = handle {
        for (id, interval) in &applied {
            if let Err(e) = handle.update_interval(id.clone(), *interval) {
                log::error!("Failed to update {id} interval: {e}");
            }
        }
    }
    applied
}

/// Parse a GUI statistics_sections JSON payload (as sent by the GUI over
/// D-Bus) into daemon poll settings. Falls back to defaults on parse errors
/// so a malformed payload can never wedge the scheduler into a bad state.
pub fn from_statistics_json(json: &str) -> Result<DaemonPollSettings, serde_json::Error> {
    let sections: StatisticsSectionsMirror = serde_json::from_str(json)?;
    Ok((&sections).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_legacy_defaults() {
        // HOME pointing at an empty dir must behave like headless.
        let dir = std::env::temp_dir().join(format!("lapsphere-ds-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("HOME", &dir);
        assert_eq!(DaemonPollSettings::load().hardware_monitor_ms, 1000);
        assert_eq!(DaemonPollSettings::load().fan_control_ms, 2000);
        assert_eq!(DaemonPollSettings::load().gpu_overclock_ms, 1000);
    }

    #[test]
    fn full_gui_payload_maps_jobs() {
        let json = r#"{
            "show_system_info": true,
            "cpu_poll_rate": 500,
            "memory_poll_rate": 800,
            "gpu_poll_rate": 2000,
            "battery_poll_rate": 5000,
            "wifi_poll_rate": 5000,
            "storage_poll_rate": 5000,
            "fans_poll_rate": 1500,
            "gamepad_poll_rate": 5000,
            "gpu_overclock_poll_rate": 3000
        }"#;
        let s = from_statistics_json(json).unwrap();
        // hardware_monitor = min(cpu=500, gpu=2000)
        assert_eq!(s.hardware_monitor_ms, 500);
        assert_eq!(s.fan_control_ms, 1500);
        assert_eq!(s.gpu_overclock_ms, 3000);
    }

    #[test]
    fn partial_payload_takes_serde_defaults_for_missing_fields() {
        let json = r#"{ "gpu_poll_rate": 400 }"#;
        let s = from_statistics_json(json).unwrap();
        // Missing fields take the lapsphere-common serde defaults (what a
        // fresh GUI install would write), NOT the old daemon hardcodes.
        // cpu falls back to its serde default (1000); min(1000, 400) = 400.
        assert_eq!(s.hardware_monitor_ms, 400);
        // fans_poll_rate missing -> common default 1000.
        assert_eq!(s.fan_control_ms, 1000);
        assert_eq!(s.gpu_overclock_ms, 1000);
    }

    #[test]
    fn garbage_payload_is_an_error_not_a_panic() {
        assert!(from_statistics_json("not json").is_err());
    }

    #[test]
    fn absurdly_fast_rates_are_clamped() {
        let json = r#"{ "cpu_poll_rate": 1, "fans_poll_rate": 0, "gpu_overclock_poll_rate": 10 }"#;
        let s = from_statistics_json(json).unwrap();
        assert!(s.hardware_monitor_ms >= 50);
        assert!(s.fan_control_ms >= 50);
        assert!(s.gpu_overclock_ms >= 50);
    }

    #[test]
    fn diff_only_reports_changed_jobs() {
        let prev = DaemonPollSettings::default();
        let next = DaemonPollSettings {
            hardware_monitor_ms: 1000,
            fan_control_ms: 750,
            gpu_overclock_ms: 1000,
        };
        let updates = compute_interval_updates(&prev, &next);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].0, "fan_control");
        assert_eq!(updates[0].1, Duration::from_millis(750));
    }

    #[test]
    fn identical_settings_produce_no_updates() {
        let a = DaemonPollSettings::default();
        assert!(compute_interval_updates(&a, &a).is_empty());
    }
}
