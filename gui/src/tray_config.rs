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
//! [`TrayConfig`]; see `docs/development/known-problems.md` for the migration
//! behaviour.
//!
//! [`TrayConfig`] is a *mirror*: the runtime `AppConfig` in `lapsphere-common`
//! still owns the fields (the GUI and the daemon both consume `AppConfig`),
//! and this struct only translates between the on-disk file and those fields.
//! Keeping it here — rather than moving the fields into `common` — is what lets
//! this change stay out of the daemon and out of `common`.

use std::path::Path;

use serde::{Deserialize, Serialize};

use lapsphere_common::types::AppConfig;

/// Bumped when the on-disk shape changes in a way older builds cannot read.
pub const TRAY_CONFIG_VERSION: u32 = 1;

/// File name of the tray configuration inside the config directory.
pub const TRAY_CONFIG_FILE: &str = "tray.json";

/// Name of the one-time backup of `settings.json` taken before the split.
pub const SETTINGS_PRE_SPLIT_BACKUP: &str = "settings.json.pre-tray-split";

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
    /// default to `false`. Kept as literals rather than reading `AppConfig`
    /// so a change to the global default cannot silently change what an
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

/// What a migration attempt did, for logging and for the tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationOutcome {
    /// `tray.json` already existed — nothing was read from or written to
    /// `settings.json`. This is the steady state and must be side-effect free.
    AlreadyMigrated,
    /// `tray.json` did not exist and `settings.json` had no tray fields, so
    /// there was nothing to move.
    NothingToMigrate,
    /// Tray fields were copied into `tray.json`; `settings.json` was backed up
    /// and rewritten without them.
    Migrated,
}

/// The keys [`TrayConfig`] owns inside the old monolithic `settings.json`.
const TRAY_KEYS: [&str; 2] = ["start_minimized", "tray_enabled"];

pub fn tray_config_path(config_dir: &str) -> String {
    format!("{}/{}", config_dir, TRAY_CONFIG_FILE)
}

fn settings_path(config_dir: &str) -> String {
    format!("{}/settings.json", config_dir)
}

/// Write `contents` to `path` so a reader never observes a partial file.
///
/// The temp file is created in the same directory (so the final `rename` is
/// within one filesystem and therefore atomic), flushed, and only then moved
/// into place. A crash mid-write leaves either the old file or the new one.
///
/// `pub` so the `settings.json` rewrite that follows a migration uses the same
/// guarantee as `tray.json` itself.
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

/// Resolve the effective tray settings for one run.
///
/// Order matters and is the whole point of this function:
///
/// 1. `load_tray_config` — may quarantine a corrupt `tray.json` and return
///    defaults, leaving no `tray.json` behind.
/// 2. `migrate_tray_config` — with the corrupt file now out of the way, values
///    still sitting in `settings.json` are recovered into a fresh
///    `tray.json`. This is why step 1 must come first.
/// 3. A pre-split `config.json` is the last resort: it is read only when
///    neither of the current files exists, which is exactly the state a
///    never-migrated legacy install is in.
pub fn resolve_tray_config(config_dir: &str, legacy_config: Option<&AppConfig>) -> TrayConfig {
    let mut tray = load_tray_config(config_dir);
    if migrate_tray_config(config_dir) == MigrationOutcome::Migrated {
        tray = load_tray_config(config_dir);
    }

    if tray == TrayConfig::default() && !Path::new(&tray_config_path(config_dir)).exists() {
        if let Some(legacy) = legacy_config {
            let from_legacy = TrayConfig::from(legacy);
            if from_legacy != TrayConfig::default() {
                log::info!("Seeding tray.json from the legacy config.json");
            }
            return from_legacy;
        }
    }

    tray
}

/// Load `tray.json`.
///
/// A missing file yields the defaults. A file that fails to parse is *moved
/// aside*, not deleted and not overwritten in place, so no user setting is
/// ever destroyed by this code path; the defaults are used and a warning is
/// logged.
pub fn load_tray_config(config_dir: &str) -> TrayConfig {
    let path = tray_config_path(config_dir);
    if !Path::new(&path).exists() {
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

/// Move tray fields out of a legacy `settings.json` and into `tray.json`.
///
/// Ordering is chosen so that no failure can lose settings:
///
/// 1. If `tray.json` exists, stop — `settings.json` is left untouched, which
///    is what makes repeat runs idempotent.
/// 2. If `settings.json` has no tray key, stop without creating either file.
/// 3. Write `tray.json` atomically. If this fails, `settings.json` is
///    untouched and the values are still there for the next attempt.
/// 4. Only after that write succeeded, copy `settings.json` to
///    `settings.json.pre-tray-split` (once — an existing backup is never
///    overwritten) and rewrite `settings.json` without the tray keys.
///
/// Because step 1 is the guard, a successful run followed by any number of
/// further runs changes nothing.
pub fn migrate_tray_config(config_dir: &str) -> MigrationOutcome {
    let tray_path = tray_config_path(config_dir);
    if Path::new(&tray_path).exists() {
        return MigrationOutcome::AlreadyMigrated;
    }

    let settings_file = settings_path(config_dir);
    let json = match std::fs::read_to_string(&settings_file) {
        Ok(json) => json,
        Err(_) => return MigrationOutcome::NothingToMigrate,
    };

    let mut settings: serde_json::Value = match serde_json::from_str(&json) {
        Ok(value) => value,
        Err(err) => {
            log::warn!(
                "Could not parse {} for tray migration: {}",
                settings_file,
                err
            );
            return MigrationOutcome::NothingToMigrate;
        }
    };

    if !settings.is_object() {
        return MigrationOutcome::NothingToMigrate;
    }

    let has_tray_key = TRAY_KEYS.iter().any(|key| settings.get(*key).is_some());
    if !has_tray_key {
        return MigrationOutcome::NothingToMigrate;
    }

    // Read the values with a per-field fallback for a field that is present
    // but of the wrong type: a hand-edited `tray_enabled: "yes"` must not abort
    // the whole migration and strand the other field in settings.json forever.
    let field = |name: &str| settings.get(name).and_then(|v| v.as_bool());
    let legacy: TrayConfig = serde_json::from_value(settings.clone()).unwrap_or_else(|err| {
        log::warn!(
            "Unusable tray fields in {} ({}); migrating the rest of them with defaults",
            settings_file,
            err
        );
        TrayConfig {
            version: TRAY_CONFIG_VERSION,
            start_minimized: field("start_minimized").unwrap_or(false),
            tray_enabled: field("tray_enabled").unwrap_or(false),
        }
    });

    let tray_json = match serde_json::to_string_pretty(&legacy) {
        Ok(json) => json,
        Err(err) => {
            log::warn!("Could not serialize tray config: {}", err);
            return MigrationOutcome::NothingToMigrate;
        }
    };
    if let Err(err) = write_atomic(&tray_path, &tray_json) {
        log::warn!(
            "Tray migration aborted, {} left unchanged: {}",
            settings_file,
            err
        );
        return MigrationOutcome::NothingToMigrate;
    }

    // tray.json now holds the values; only now is it safe to drop them.
    let backup = format!("{}/{}", config_dir, SETTINGS_PRE_SPLIT_BACKUP);
    if !Path::new(&backup).exists() {
        if let Err(err) = std::fs::copy(&settings_file, &backup) {
            log::warn!("Could not back up {} to {}: {}", settings_file, backup, err);
        }
    }

    if let Some(object) = settings.as_object_mut() {
        for key in TRAY_KEYS {
            object.remove(key);
        }
    }
    match serde_json::to_string_pretty(&settings) {
        Ok(pretty) => {
            if let Err(err) = write_atomic(&settings_file, &pretty) {
                log::warn!(
                    "Wrote tray.json but could not clean {}: {} (values are still in the backup if it exists)",
                    settings_file, err
                );
                return MigrationOutcome::NothingToMigrate;
            }
        }
        Err(err) => log::warn!("Could not re-serialize {}: {}", settings_file, err),
    }

    log::info!(
        "Migrated tray settings from {} to {} (backup: {})",
        settings_file,
        tray_path,
        backup
    );
    MigrationOutcome::Migrated
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
        Path::new(&format!("{}/{}", dir, name)).exists()
    }

    /// A legacy settings.json with tray fields plus unrelated fields that must
    /// survive the migration untouched.
    const LEGACY_SETTINGS: &str = r#"{
  "theme": "Dark",
  "start_minimized": true,
  "tray_enabled": true,
  "autostart": false,
  "font_size": "Large"
}"#;

    #[test]
    fn clean_config_yields_defaults_and_no_tray_file_is_created() {
        let dir = TestDir::new("clean");

        assert_eq!(load_tray_config(&dir.path()), TrayConfig::default());

        assert_eq!(
            migrate_tray_config(&dir.path()),
            MigrationOutcome::NothingToMigrate
        );
        assert!(!exists(&dir.path(), TRAY_CONFIG_FILE));
        assert!(!exists(&dir.path(), SETTINGS_PRE_SPLIT_BACKUP));
    }

    #[test]
    fn legacy_settings_without_tray_keys_are_left_alone() {
        let dir = TestDir::new("no-tray-keys");
        write(
            &dir.path(),
            "settings.json",
            r#"{ "theme": "Dark", "autostart": true }"#,
        );

        assert_eq!(
            migrate_tray_config(&dir.path()),
            MigrationOutcome::NothingToMigrate
        );
        assert!(!exists(&dir.path(), TRAY_CONFIG_FILE));
        assert_eq!(
            read(&dir.path(), "settings.json"),
            r#"{ "theme": "Dark", "autostart": true }"#
        );
    }

    #[test]
    fn migration_moves_tray_values_and_backs_up_settings() {
        let dir = TestDir::new("migrate");
        write(&dir.path(), "settings.json", LEGACY_SETTINGS);

        assert_eq!(migrate_tray_config(&dir.path()), MigrationOutcome::Migrated);

        let tray: serde_json::Value =
            serde_json::from_str(&read(&dir.path(), TRAY_CONFIG_FILE)).unwrap();
        assert_eq!(tray["start_minimized"], serde_json::json!(true));
        assert_eq!(tray["tray_enabled"], serde_json::json!(true));
        assert_eq!(tray["version"], serde_json::json!(TRAY_CONFIG_VERSION));

        let settings: serde_json::Value =
            serde_json::from_str(&read(&dir.path(), "settings.json")).unwrap();
        assert!(settings.get("start_minimized").is_none());
        assert!(settings.get("tray_enabled").is_none());
        // Unrelated fields survive.
        assert_eq!(settings["theme"], serde_json::json!("Dark"));
        assert_eq!(settings["font_size"], serde_json::json!("Large"));
        assert_eq!(settings["autostart"], serde_json::json!(false));

        // The backup is a byte-for-byte copy of the pre-split file, so an
        // older build that only knows settings.json still reads the values.
        assert_eq!(
            read(&dir.path(), SETTINGS_PRE_SPLIT_BACKUP),
            LEGACY_SETTINGS
        );
    }

    #[test]
    fn migration_is_idempotent() {
        let dir = TestDir::new("idempotent");
        write(&dir.path(), "settings.json", LEGACY_SETTINGS);

        assert_eq!(migrate_tray_config(&dir.path()), MigrationOutcome::Migrated);
        let tray_after_first = read(&dir.path(), TRAY_CONFIG_FILE);
        let settings_after_first = read(&dir.path(), "settings.json");

        for _ in 0..3 {
            assert_eq!(
                migrate_tray_config(&dir.path()),
                MigrationOutcome::AlreadyMigrated
            );
        }

        assert_eq!(read(&dir.path(), TRAY_CONFIG_FILE), tray_after_first);
        assert_eq!(read(&dir.path(), "settings.json"), settings_after_first);
    }

    #[test]
    fn a_second_migration_never_overwrites_the_backup() {
        // tray.json deleted by hand after a successful migration, with the
        // already-cleaned settings.json still on disk: the backup must survive
        // as the only copy of the original values.
        let dir = TestDir::new("backup-once");
        write(&dir.path(), "settings.json", LEGACY_SETTINGS);
        assert_eq!(migrate_tray_config(&dir.path()), MigrationOutcome::Migrated);

        std::fs::remove_file(format!("{}/{}", dir.path(), TRAY_CONFIG_FILE)).unwrap();
        assert_eq!(
            migrate_tray_config(&dir.path()),
            MigrationOutcome::NothingToMigrate
        );

        assert_eq!(
            read(&dir.path(), SETTINGS_PRE_SPLIT_BACKUP),
            LEGACY_SETTINGS
        );
    }

    #[test]
    fn existing_tray_file_is_authoritative_even_if_settings_still_has_the_keys() {
        let dir = TestDir::new("tray-wins");
        write(&dir.path(), "settings.json", LEGACY_SETTINGS);
        write(
            &dir.path(),
            TRAY_CONFIG_FILE,
            r#"{ "version": 1, "start_minimized": false, "tray_enabled": false }"#,
        );

        assert_eq!(
            migrate_tray_config(&dir.path()),
            MigrationOutcome::AlreadyMigrated
        );
        assert_eq!(read(&dir.path(), "settings.json"), LEGACY_SETTINGS);
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
    fn corrupt_tray_file_is_moved_aside_and_settings_are_recovered() {
        let dir = TestDir::new("corrupt-with-settings");
        write(&dir.path(), "settings.json", LEGACY_SETTINGS);
        write(&dir.path(), TRAY_CONFIG_FILE, "{ this is not json");

        // Corrupt file is quarantined, not silently deleted...
        let cfg = load_tray_config(&dir.path());
        assert_eq!(cfg, TrayConfig::default());
        assert!(!exists(&dir.path(), TRAY_CONFIG_FILE));
        assert_eq!(read(&dir.path(), TRAY_CONFIG_CORRUPT), "{ this is not json");

        // ...and the still-present settings.json values are then migrated, so
        // no preference is lost to the corruption.
        assert_eq!(migrate_tray_config(&dir.path()), MigrationOutcome::Migrated);
        let tray: serde_json::Value =
            serde_json::from_str(&read(&dir.path(), TRAY_CONFIG_FILE)).unwrap();
        assert_eq!(tray["tray_enabled"], serde_json::json!(true));
    }

    #[test]
    fn corrupt_tray_file_without_a_settings_fallback_uses_defaults() {
        let dir = TestDir::new("corrupt-no-settings");
        write(&dir.path(), TRAY_CONFIG_FILE, "{\"start_minimized\": tru");

        assert_eq!(load_tray_config(&dir.path()), TrayConfig::default());
        // Reading alone must not re-create or overwrite the file.
        assert!(!exists(&dir.path(), TRAY_CONFIG_FILE));
        assert_eq!(
            migrate_tray_config(&dir.path()),
            MigrationOutcome::NothingToMigrate
        );
        // Nothing was invented from nothing.
        assert_eq!(load_tray_config(&dir.path()), TrayConfig::default());
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

        // Overwriting leaves the complete new document, never a mix.
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
    fn unusable_tray_value_types_do_not_strand_the_other_field() {
        let dir = TestDir::new("bad-type");
        write(
            &dir.path(),
            "settings.json",
            r#"{ "start_minimized": true, "tray_enabled": "yes", "theme": "Dark" }"#,
        );

        assert_eq!(migrate_tray_config(&dir.path()), MigrationOutcome::Migrated);

        let tray: serde_json::Value =
            serde_json::from_str(&read(&dir.path(), TRAY_CONFIG_FILE)).unwrap();
        assert_eq!(tray["start_minimized"], serde_json::json!(true));
        assert_eq!(tray["tray_enabled"], serde_json::json!(false));

        let settings: serde_json::Value =
            serde_json::from_str(&read(&dir.path(), "settings.json")).unwrap();
        assert_eq!(settings["theme"], serde_json::json!("Dark"));
    }

    // ---- resolve_tray_config: the order the real startup path uses ----

    fn legacy_config_with_tray() -> AppConfig {
        let mut config = AppConfig::default();
        config.start_minimized = true;
        config.tray_enabled = true;
        config
    }

    #[test]
    fn resolve_on_a_clean_config_uses_defaults() {
        let dir = TestDir::new("resolve-clean");
        // A fresh install: no tray.json, no settings.json, and no legacy
        // config.json, so `load_config_from_disk` passes None.
        assert_eq!(
            resolve_tray_config(&dir.path(), None),
            TrayConfig::default()
        );
        assert!(!exists(&dir.path(), TRAY_CONFIG_FILE));
        assert!(!exists(&dir.path(), SETTINGS_PRE_SPLIT_BACKUP));
    }

    #[test]
    fn resolve_migrates_from_settings_json_in_one_call() {
        let dir = TestDir::new("resolve-migrate");
        write(&dir.path(), "settings.json", LEGACY_SETTINGS);

        let tray = resolve_tray_config(&dir.path(), None);
        assert!(tray.start_minimized);
        assert!(tray.tray_enabled);
        assert!(exists(&dir.path(), TRAY_CONFIG_FILE));
        assert!(exists(&dir.path(), SETTINGS_PRE_SPLIT_BACKUP));
    }

    #[test]
    fn resolve_recovers_from_settings_json_when_tray_json_is_corrupt() {
        let dir = TestDir::new("resolve-corrupt");
        write(&dir.path(), "settings.json", LEGACY_SETTINGS);
        write(&dir.path(), TRAY_CONFIG_FILE, "not json at all");

        let tray = resolve_tray_config(&dir.path(), None);

        assert!(
            tray.start_minimized,
            "user setting recovered, not defaulted away"
        );
        assert!(tray.tray_enabled);
        // And the recovered value is now persisted, so the next run is stable.
        assert_eq!(load_tray_config(&dir.path()), tray);
    }

    #[test]
    fn resolve_falls_back_to_legacy_config_json_only_as_a_last_resort() {
        let dir = TestDir::new("resolve-legacy");
        write(&dir.path(), "config.json", r#"{ "tray_enabled": true }"#);

        assert_eq!(
            resolve_tray_config(&dir.path(), Some(&legacy_config_with_tray())),
            TrayConfig {
                version: TRAY_CONFIG_VERSION,
                start_minimized: true,
                tray_enabled: true
            }
        );

        // Once a tray.json exists it wins, even over the legacy file.
        std::fs::write(
            format!("{}/{}", dir.path(), TRAY_CONFIG_FILE),
            r#"{ "version": 1, "start_minimized": false, "tray_enabled": false }"#,
        )
        .unwrap();
        assert_eq!(
            resolve_tray_config(&dir.path(), Some(&legacy_config_with_tray())),
            TrayConfig::default()
        );
    }

    #[test]
    fn resolve_over_repeated_runs_is_stable() {
        let dir = TestDir::new("resolve-repeat");
        write(&dir.path(), "settings.json", LEGACY_SETTINGS);

        let first = resolve_tray_config(&dir.path(), None);
        let tray_after_first = read(&dir.path(), TRAY_CONFIG_FILE);
        let settings_after_first = read(&dir.path(), "settings.json");
        let backup_after_first = read(&dir.path(), SETTINGS_PRE_SPLIT_BACKUP);

        for _ in 0..3 {
            assert_eq!(resolve_tray_config(&dir.path(), None), first);
        }

        assert_eq!(read(&dir.path(), TRAY_CONFIG_FILE), tray_after_first);
        assert_eq!(read(&dir.path(), "settings.json"), settings_after_first);
        assert_eq!(
            read(&dir.path(), SETTINGS_PRE_SPLIT_BACKUP),
            backup_after_first
        );
    }
}
