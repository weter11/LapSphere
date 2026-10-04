//! Panel mode: a compact vertical copy of the Statistics tab, on the app's one
//! window.
//!
//! # What is in this PR
//!
//! * [`config`] — `~/.config/lapsphere/panel.json`, its own file, its own
//!   schema, never touching `settings.json` or the tray's settings.
//! * [`rows`] — the row registry. One row per quantity the Statistics tab shows,
//!   grouped CPU / GPU / RAM / Gamepad for the menu, formatted by
//!   [`crate::stat_format`] — the same functions the tab calls.
//! * [`layout`] — the window size, COMPUTED from the visible row count and the
//!   font row height. Never measured, so there is no measure/resize/measure loop.
//! * [`render`] — the vertical `Label: value` column, one fixed-height line per
//!   row, an em dash where hardware reported nothing.
//! * [`menu`] — the right-click context menu: row checkboxes, the width slider,
//!   the opacity slider, the stacking choice and the way back to the normal
//!   window. No separate settings window.
//! * [`window`] — the Normal↔Panel switch and the poll narrowing that keeps the
//!   panel from polling what it does not draw.
//!
//! # What is NOT in this PR, by decision
//!
//! * fps, frame time, graphs, ring buffers, the Vulkan layer, click-through and
//!   the F8 interactivity key. No reserved slots for them either: a slot with no
//!   producer is a permanent row of dashes.
//! * The hotkey and the D-Bus toggle (PR 3), and the X11 stacking / taskbar atoms
//!   and real transparency (PR 2). `stacking` and `hide_from_taskbar` are stored
//!   and normalized, but nothing here claims they are applied.
//! * Wayland. The panel uses only `ViewportCommand`s, which is the portable
//!   subset; the X11-specific work is PR 2's and is not tested on Wayland.
//! * Multi-monitor placement. The panel is not positioned by this code, so no
//!   second-monitor claim is made.

pub mod config;
pub mod layout;
pub mod menu;
pub mod render;
pub mod rows;
pub mod window;

use std::path::PathBuf;

pub use config::{normalize, PanelConfig, PANEL_CONFIG_FILE};

/// `~/.config/lapsphere/panel.json`.
pub fn panel_config_path() -> PathBuf {
    let mut path = PathBuf::from(crate::app::get_config_dir());
    path.push(PANEL_CONFIG_FILE);
    path
}

/// Load `panel.json`, falling back to defaults.
///
/// A missing file is the normal first-run case and is not an error. A file that
/// exists but cannot be parsed is MOVED ASIDE (never deleted, never overwritten
/// in place) and defaults are used: a panel that refuses to start is worse than
/// a panel with default rows, and the accessory surface must not be able to keep
/// the GUI from launching.
pub fn load_panel_config() -> PanelConfig {
    let path = panel_config_path();

    let json = match std::fs::read_to_string(&path) {
        Ok(json) => json,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            log::info!("no {PANEL_CONFIG_FILE} yet, using panel defaults");
            return normalize(PanelConfig::fresh());
        }
        Err(err) => {
            log::error!(
                "could not read {}: {}, using panel defaults",
                path.display(),
                err
            );
            return normalize(PanelConfig::fresh());
        }
    };

    match serde_json::from_str::<PanelConfig>(&json) {
        Ok(config) => {
            let normalized = normalize(config);
            if normalized.rows.len() != rows::all_ids().len() {
                log::warn!("{PANEL_CONFIG_FILE} listed an unknown or duplicate row; normalized");
            }
            normalized
        }
        Err(err) => {
            let corrupt = path.with_file_name(config::PANEL_CONFIG_CORRUPT);
            match std::fs::rename(&path, &corrupt) {
                Ok(()) => log::warn!(
                    "corrupt {} ({}); moved to {} and using panel defaults",
                    path.display(),
                    err,
                    corrupt.display()
                ),
                Err(rename_err) => log::warn!(
                    "corrupt {} ({}); using panel defaults (could not move it aside: {})",
                    path.display(),
                    err,
                    rename_err
                ),
            }
            normalize(PanelConfig::fresh())
        }
    }
}

/// Write `panel.json`, atomically.
///
/// Same contract as `tray.json`: the temp file is created in the same directory
/// so the final rename is within one filesystem, flushed, and only then moved
/// into place, so a reader (including our own next start) never sees a partial
/// document.
pub fn save_panel_config(config: &PanelConfig) -> anyhow::Result<()> {
    let path = panel_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(config)?;
    crate::tray_config::write_atomic(&path.to_string_lossy(), &json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Unique scratch config directory per test, removed on drop.
    ///
    /// `load_panel_config` reads `crate::app::get_config_dir()`, which is
    /// `$HOME`-derived and therefore process-global and racy across parallel
    /// tests. So the path-taking helpers below are the tested surface and the
    /// crate-root wrappers are only checked for what they build.
    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "lapsphere-panel-{}-{}-{}",
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
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn panel_file(dir: &str) -> String {
        format!("{}/{}", dir, PANEL_CONFIG_FILE)
    }

    fn corrupt_file(dir: &str) -> String {
        format!("{}/{}", dir, config::PANEL_CONFIG_CORRUPT)
    }

    /// `load_panel_config` with an explicit directory.
    ///
    /// Mirrors [`load_panel_config`] exactly; the split exists only so the tests
    /// do not have to mutate `HOME`.
    fn load_from(config_dir: &str) -> PanelConfig {
        let path = panel_file(config_dir);
        let json = match std::fs::read_to_string(&path) {
            Ok(json) => json,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return normalize(PanelConfig::fresh())
            }
            Err(_) => return normalize(PanelConfig::fresh()),
        };
        match serde_json::from_str::<PanelConfig>(&json) {
            Ok(config) => normalize(config),
            Err(_) => {
                let _ = std::fs::rename(&path, corrupt_file(config_dir));
                normalize(PanelConfig::fresh())
            }
        }
    }

    /// `save_panel_config` with an explicit directory.
    fn save_to(config_dir: &str, config: &PanelConfig) -> anyhow::Result<()> {
        std::fs::create_dir_all(config_dir)?;
        let json = serde_json::to_string_pretty(config)?;
        crate::tray_config::write_atomic(&panel_file(config_dir), &json)
    }

    #[test]
    fn the_panel_file_is_panel_json_next_to_settings_json() {
        let path = panel_config_path();
        assert!(path.ends_with(PANEL_CONFIG_FILE), "got {}", path.display());
        assert_eq!(
            path.parent(),
            Some(std::path::Path::new(&crate::app::get_config_dir())),
            "same directory as the other config files, separate file"
        );
    }

    #[test]
    fn a_missing_file_yields_the_fresh_defaults_and_writes_nothing() {
        let dir = TestDir::new("missing");
        let config = load_from(&dir.path());

        assert_eq!(config.version, config::PANEL_CONFIG_VERSION);
        assert_eq!(config.width, config::DEFAULT_WIDTH);
        assert_eq!(
            config.rows.len(),
            rows::all_ids().len(),
            "every known row must be present in a fresh config"
        );
        assert!(!std::path::Path::new(&panel_file(&dir.path())).exists());
    }

    #[test]
    fn a_saved_config_round_trips_through_the_real_file() {
        let dir = TestDir::new("roundtrip");
        let mut original = normalize(PanelConfig::fresh());
        original.width = 412.0;
        original.background_opacity = 0.8;
        original.stacking = config::Stacking::Normal;
        original.rows[0].visible = false;

        save_to(&dir.path(), &original).unwrap();
        let reloaded = load_from(&dir.path());

        assert_eq!(reloaded, original);
        assert_eq!(reloaded.width, 412.0);
        assert!(!reloaded.rows[0].visible);
    }

    #[test]
    fn the_width_survives_a_save_and_is_read_back_verbatim() {
        // The "the slider applies at once" claim rests on this: what the menu
        // wrote is what the next frame and the next start read.
        let dir = TestDir::new("width");
        let mut config = normalize(PanelConfig::fresh());
        config.width = 355.0;
        save_to(&dir.path(), &config).unwrap();

        let json = std::fs::read_to_string(panel_file(&dir.path())).unwrap();
        assert!(json.contains("\"width\": 355.0"), "got {json}");
        assert_eq!(load_from(&dir.path()).width, 355.0);
    }

    #[test]
    fn a_corrupt_file_is_moved_aside_not_deleted() {
        let dir = TestDir::new("corrupt");
        std::fs::write(panel_file(&dir.path()), "{ this is not json").unwrap();

        let config = load_from(&dir.path());

        // Usable defaults, so the panel still opens.
        assert_eq!(config.rows.len(), rows::all_ids().len());
        // And the user's bytes are preserved verbatim for inspection.
        assert_eq!(
            std::fs::read_to_string(corrupt_file(&dir.path())).unwrap(),
            "{ this is not json"
        );
        assert!(!std::path::Path::new(&panel_file(&dir.path())).exists());
    }

    #[test]
    fn a_partial_file_keeps_what_it_says_and_fills_in_the_rest() {
        let dir = TestDir::new("partial");
        std::fs::write(
            panel_file(&dir.path()),
            r#"{ "width": 260.0, "rows": [{ "id": "ram_usage", "visible": true }] }"#,
        )
        .unwrap();

        let config = load_from(&dir.path());

        assert_eq!(config.width, 260.0, "the user's own value survives");
        assert_eq!(config.rows.len(), rows::all_ids().len());
        let ram = config.rows.iter().find(|r| r.id == "ram_usage").unwrap();
        assert!(ram.visible, "the one row the file mentioned is honoured");
    }

    #[test]
    fn repeated_loads_are_stable_and_never_rewrite_the_file() {
        let dir = TestDir::new("stable");
        let config = normalize(PanelConfig::fresh());
        save_to(&dir.path(), &config).unwrap();
        let before = std::fs::read_to_string(panel_file(&dir.path())).unwrap();

        for _ in 0..3 {
            assert_eq!(load_from(&dir.path()), config);
        }
        assert_eq!(
            std::fs::read_to_string(panel_file(&dir.path())).unwrap(),
            before,
            "loading must not rewrite panel.json"
        );
    }

    #[test]
    fn no_temp_file_is_left_behind_by_a_save() {
        let dir = TestDir::new("atomic");
        save_to(&dir.path(), &PanelConfig::fresh()).unwrap();
        assert!(
            !std::path::Path::new(&format!("{}.tmp", panel_file(&dir.path()))).exists(),
            "the atomic-write temp file must be renamed away"
        );
    }
}
