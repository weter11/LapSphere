//! Tray settings, persisted separately from the rest of the application
//! configuration.
//!
//! # Why a separate file
//!
//! `settings.json` holds panel/window-level preferences (theme, font, polling
//! rates, battery thresholds). The two tray-owned booleans were previously in
//! that same file, which made it impossible to tell "which file do I edit to
//! change the tray?" — every other per-subsystem file (`profiles.json`) is
//! already split out. They now live in `~/.config/lapsphere/tray.json` as
//! [`TrayConfig`]; see `docs/development/config-files.md` for the full file
//! layout and the migration rules.
//!
//! [`TrayConfig`] is a *mirror*: the runtime `AppConfig` in `lapsphere-common`
//! still owns the fields (the GUI and the daemon both consume `AppConfig`),
//! and this struct only translates between the on-disk file and those fields.
//! Keeping it here — rather than moving the fields into `common` — is what lets
//! this change stay out of the daemon and out of `common`.

use serde::{Deserialize, Serialize};

use lapsphere_common::types::AppConfig;

/// Bumped when the on-disk shape changes in a way older builds cannot read.
pub const TRAY_CONFIG_VERSION: u32 = 1;

/// File name of the tray configuration inside the config directory.
pub const TRAY_CONFIG_FILE: &str = "tray.json";

/// Name a corrupt `tray.json` is moved aside to instead of being deleted.
pub const TRAY_CONFIG_CORRUPT: &str = "tray.json.corrupt";

/// Tray-owned settings.
///
/// `#[serde(default)]` at the container level means a file written by a newer
/// build (or hand-trimmed by a user) still deserializes: unknown fields are
/// ignored and missing ones fall back to [`TrayConfig::default`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrayConfig {
    /// On-disk schema version of this file.
    pub version: u32,
    /// Start minimized (to the tray) when the GUI launches.
    pub start_minimized: bool,
    /// Enable the system tray icon and minimize the window on close.
    pub tray_enabled: bool,
}

impl Default for TrayConfig {
    /// Mirrors `AppConfig::default()` (`common/src/types.rs`): both booleans
    /// default to `false`. Kept as literals rather than reading `AppConfig` so a
    /// change to the global default cannot silently change what an
    /// already-written `tray.json` means — see the mirror-drift note in the
    /// `lapsphere-development` skill.
    fn default() -> Self {
        Self {
            version: TRAY_CONFIG_VERSION,
            start_minimized: false,
            tray_enabled: false,
        }
    }
}

impl From<&AppConfig> for TrayConfig {
    fn from(config: &AppConfig) -> Self {
        Self {
            version: TRAY_CONFIG_VERSION,
            start_minimized: config.start_minimized,
            tray_enabled: config.tray_enabled,
        }
    }
}

impl TrayConfig {
    pub fn apply_to(&self, config: &mut AppConfig) {
        config.start_minimized = self.start_minimized;
        config.tray_enabled = self.tray_enabled;
    }
}

pub fn tray_config_path(config_dir: &str) -> String {
    format!("{}/{}", config_dir, TRAY_CONFIG_FILE)
}

/// Write `contents` to `path` so a reader never observes a partial file.
///
/// The temp file is created in the same directory (so the final `rename` is
/// within one filesystem and therefore atomic), flushed, and only then moved
/// into place. A crash mid-write leaves either the old file or the new one.
///
/// `pub` so the `settings.json` writes use the same guarantee as `tray.json`.
pub(crate) fn write_atomic(path: &str, contents: &str) -> anyhow::Result<()> {
    let tmp = format!("{}.tmp", path);
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Load `tray.json`.
///
/// A missing file yields the defaults. A file that fails to parse is *moved
/// aside*, not deleted and not overwritten in place, so no user setting is ever
/// destroyed by this code path; the defaults are used and a warning is logged.
pub fn load_tray_config(config_dir: &str) -> TrayConfig {
    let path = tray_config_path(config_dir);
    if !std::path::Path::new(&path).exists() {
        return TrayConfig::default();
    }

    match std::fs::read_to_string(&path) {
        Ok(json) => match serde_json::from_str::<TrayConfig>(&json) {
            Ok(cfg) => cfg,
            Err(err) => {
                let corrupt = format!("{}/{}", config_dir, TRAY_CONFIG_CORRUPT);
                match std::fs::rename(&path, &corrupt) {
                    Ok(()) => log::warn!(
                        "Corrupt {} ({}); moved to {} and falling back to tray defaults",
                        path,
                        err,
                        corrupt
                    ),
                    Err(rename_err) => log::warn!(
                        "Corrupt {} ({}); falling back to tray defaults (could not move it aside: {})",
                        path,
                        err,
                        rename_err
                    ),
                }
                TrayConfig::default()
            }
        },
        Err(err) => {
            log::warn!(
                "Could not read {} ({}); falling back to tray defaults",
                path,
                err
            );
            TrayConfig::default()
        }
    }
}

/// Persist the tray fields of `config` to `tray.json`.
pub fn save_tray_config(config_dir: &str, config: &AppConfig) -> anyhow::Result<()> {
    std::fs::create_dir_all(config_dir)?;
    let json = serde_json::to_string_pretty(&TrayConfig::from(config))?;
    write_atomic(&tray_config_path(config_dir), &json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Unique scratch directory per test case; removed on drop so a failing
    /// assertion cannot leak state into the next one.
    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "lapsphere-tray-{}-{}-{}",
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

    fn write(dir: &str, name: &str, contents: &str) {
        std::fs::write(format!("{}/{}", dir, name), contents).unwrap();
    }

    fn read(dir: &str, name: &str) -> String {
        std::fs::read_to_string(format!("{}/{}", dir, name)).unwrap()
    }

    fn exists(dir: &str, name: &str) -> bool {
        std::path::Path::new(&format!("{}/{}", dir, name)).exists()
    }

    #[test]
    fn a_missing_file_yields_defaults_and_nothing_is_created() {
        let dir = TestDir::new("clean");
        assert_eq!(load_tray_config(&dir.path()), TrayConfig::default());
        // Reading alone must not create the file.
        assert!(!exists(&dir.path(), TRAY_CONFIG_FILE));
    }

    #[test]
    fn a_corrupt_file_is_moved_aside_and_defaults_are_used() {
        let dir = TestDir::new("corrupt");
        write(&dir.path(), TRAY_CONFIG_FILE, "{ this is not json");

        assert_eq!(load_tray_config(&dir.path()), TrayConfig::default());

        // Quarantined, never deleted or overwritten in place.
        assert!(!exists(&dir.path(), TRAY_CONFIG_FILE));
        assert_eq!(read(&dir.path(), TRAY_CONFIG_CORRUPT), "{ this is not json");
    }

    #[test]
    fn a_truncated_write_cannot_be_observed_because_writes_are_atomic() {
        let dir = TestDir::new("atomic");
        let path = tray_config_path(&dir.path());
        write_atomic(
            &path,
            r#"{ "version": 1, "start_minimized": true, "tray_enabled": true }"#,
        )
        .unwrap();
        assert!(!exists(&dir.path(), &format!("{}.tmp", TRAY_CONFIG_FILE)));

        // Overwriting leaves the complete new document, never a mix of the two.
        write_atomic(
            &path,
            r#"{ "version": 1, "start_minimized": false, "tray_enabled": false }"#,
        )
        .unwrap();
        assert_eq!(
            load_tray_config(&dir.path()),
            TrayConfig {
                version: TRAY_CONFIG_VERSION,
                start_minimized: false,
                tray_enabled: false
            }
        );
    }

    #[test]
    fn partial_tray_file_falls_back_per_field() {
        // A file written by a build that predates one of the fields must not
        // fail wholesale: the missing field takes its default.
        let dir = TestDir::new("partial");
        write(&dir.path(), TRAY_CONFIG_FILE, r#"{ "tray_enabled": true }"#);

        let cfg = load_tray_config(&dir.path());
        assert!(cfg.tray_enabled);
        assert!(!cfg.start_minimized);
        assert_eq!(cfg.version, TRAY_CONFIG_VERSION);
    }

    #[test]
    fn unknown_fields_from_a_newer_build_are_ignored() {
        let dir = TestDir::new("future");
        write(
            &dir.path(),
            TRAY_CONFIG_FILE,
            r#"{ "version": 99, "start_minimized": true, "tray_enabled": true, "some_future_flag": 3 }"#,
        );

        let cfg = load_tray_config(&dir.path());
        assert_eq!(cfg.version, 99);
        assert!(cfg.start_minimized);
        assert!(cfg.tray_enabled);
    }

    #[test]
    fn save_and_load_round_trip_through_app_config() {
        let dir = TestDir::new("roundtrip");
        let mut config = AppConfig::default();
        config.start_minimized = true;
        config.tray_enabled = true;

        save_tray_config(&dir.path(), &config).unwrap();

        let mut restored = AppConfig::default();
        restored.start_minimized = false;
        restored.tray_enabled = false;
        TrayConfig::apply_to(&load_tray_config(&dir.path()), &mut restored);

        assert!(restored.start_minimized);
        assert!(restored.tray_enabled);
    }

    #[test]
    fn the_saved_file_contains_only_tray_fields() {
        let dir = TestDir::new("only-tray");
        let mut config = AppConfig::default();
        config.tray_enabled = true;
        config.theme = lapsphere_common::types::Theme::Dark;
        config.log_limit = 4242;

        save_tray_config(&dir.path(), &config).unwrap();

        let value: serde_json::Value =
            serde_json::from_str(&read(&dir.path(), TRAY_CONFIG_FILE)).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["start_minimized", "tray_enabled", "version"],
            "tray.json must not carry panel settings"
        );
    }

    #[test]
    fn defaults_match_app_config_defaults() {
        // Guards the mirror-drift trap: if `AppConfig` ever changes its
        // defaults, this test fails and points at the literals above.
        let defaults = AppConfig::default();
        assert!(!defaults.start_minimized);
        assert!(!defaults.tray_enabled);
    }
}
