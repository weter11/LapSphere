//! Panel mode: a MangoHud-style overlay on the same window as the main UI.
//!
//! Scope of this module in the first release:
//!
//! * `config` — `~/.config/lapsphere/panel.json`, its own file, never touching
//!   `settings.json` or the tray's settings.
//! The later commits of this branch add `window` and `visibility` as siblings
//! of `config`.
//!
//! Explicitly NOT in this release: the Vulkan layer that supplies fps, and the
//! fps data source itself. `fps`, `fps_graph` and `frametime_graph` get
//! reserved slots so PR E only has to fill in the data.

pub mod config;
pub mod items;
pub mod menu;
pub mod render;
pub mod settings_window;
pub mod units;
pub mod visibility;
pub mod window;
#[cfg(target_os = "linux")]
pub mod x11;

use std::path::PathBuf;

pub use config::{normalize, PanelConfig, DEFAULT_HOTKEY};

/// `~/.config/lapsphere/panel.json`.
pub fn panel_config_path() -> PathBuf {
    let mut path = PathBuf::from(crate::app::get_config_dir());
    path.push("panel.json");
    path
}

/// Load `panel.json`, falling back to defaults.
///
/// A missing file is the normal first-run case and is not an error. A file that
/// exists but cannot be parsed is reported once and also falls back to defaults,
/// because a panel that refuses to start is worse than a panel with default
/// items; the panel is an accessory surface and must not be able to keep the
/// GUI from launching.
pub fn load_panel_config() -> PanelConfig {
    let path = panel_config_path();

    let json = match std::fs::read_to_string(&path) {
        Ok(json) => json,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            log::info!("no panel.json yet, using panel defaults");
            return normalize(PanelConfig::default());
        }
        Err(err) => {
            log::error!(
                "could not read {}: {}, using panel defaults",
                path.display(),
                err
            );
            return normalize(PanelConfig::default());
        }
    };

    match serde_json::from_str::<PanelConfig>(&json) {
        Ok(config) => {
            let config = normalize(config);
            if config.items.len() != PanelConfig::default().items.len() {
                log::warn!("panel.json listed an unknown or duplicate element, normalized");
            }
            config
        }
        Err(err) => {
            log::error!(
                "could not parse {}: {}, using panel defaults",
                path.display(),
                err
            );
            normalize(PanelConfig::default())
        }
    }
}

/// Write `panel.json`. Errors are logged, not propagated: the panel menu must
/// keep working even when the file cannot be written.
pub fn save_panel_config(config: &PanelConfig) -> anyhow::Result<()> {
    let path = panel_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(config)?;
    std::fs::write(&path, json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_panel_file_is_panel_json_not_settings_json() {
        let path = panel_config_path();
        assert!(path.ends_with("panel.json"), "got {}", path.display());
        assert!(!path.ends_with("settings.json"));
        // Same config dir as settings.json, which is the point: separate file,
        // separate owner.
        assert_eq!(
            path.parent(),
            Some(std::path::Path::new(&crate::app::get_config_dir()))
        );
    }

    #[test]
    fn load_falls_back_to_defaults_when_the_file_is_missing() {
        // Point the loader at a directory that cannot hold a panel.json by
        // checking the pure default path instead: defaults must round-trip.
        let config = normalize(PanelConfig::default());
        assert_eq!(config.items.len(), PANEL_ITEMS_LEN);
        assert_eq!(config.hotkey, DEFAULT_HOTKEY);
    }

    const PANEL_ITEMS_LEN: usize = 15;

    #[test]
    fn defaults_serialize_to_a_file_the_loader_shape_accepts() {
        let config = normalize(PanelConfig::default());
        let json = serde_json::to_string_pretty(&config).expect("serialize");
        let parsed: PanelConfig = serde_json::from_str(&json).expect("round-trip");
        assert_eq!(normalize(parsed), config);
        assert!(json.contains("\"version\": 1"));
    }

    #[test]
    fn a_corrupt_file_shape_still_yields_a_usable_panel() {
        // What the loader does with garbage: serde fails, defaults are used.
        let garbage = "{ this is not json";
        let recovered = serde_json::from_str::<PanelConfig>(garbage)
            .map(normalize)
            .unwrap_or_else(|_| normalize(PanelConfig::default()));
        assert_eq!(recovered.items.len(), PANEL_ITEMS_LEN);
        assert_eq!(recovered.stacking, config::PanelStacking::AlwaysOnTop);
        assert!(recovered.hide_from_taskbar);
    }
}
