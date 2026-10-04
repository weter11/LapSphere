# Configuration files

All user configuration lives in `~/.config/lapsphere/` (`%APPDATA%/lapsphere` on
Windows). The directory is created on first save.

| File | Owner | Contents |
|---|---|---|
| `settings.json` | `gui/src/app.rs` (`SettingsConfig`) | Theme, font size, polling rates, battery thresholds, autostart, remembered gamepads |
| `tray.json` | `gui/src/tray_config.rs` (`TrayConfig`) | System tray icon on/off, start minimized |
| `profiles.json` | `gui/src/app.rs` (`ProfilesConfig`) | All tuning profiles and the active profile name |
| `config.json` | legacy | Pre-split single-file config; read once, then superseded |

One file per subsystem, so "which file do I edit?" has one answer per feature.
`panel.json` (window/panel layout) is a separate change and not part of this
document.

## `tray.json`

```json
{
  "version": 1,
  "start_minimized": true,
  "tray_enabled": true
}
```

`version` is the on-disk schema version and is bumped when the shape changes in
a way older builds cannot read. Unknown keys are ignored and missing keys fall
back to the defaults above (`false` / `false`), so a hand-trimmed or
newer-version file still loads.

`tray_enabled` is also forced on while `start_minimized` is true — starting
minimized with no tray icon would leave the window unreachable.

The two fields are still owned at runtime by `AppConfig` in
`lapsphere-common` (the daemon and the GUI both consume it); `TrayConfig` is
only the on-disk projection, which is why this change does not touch `common/`
or the daemon.

## Migration from the pre-split `settings.json`

`start_minimized` and `tray_enabled` used to live in `settings.json`. On startup
`tray_config::resolve_tray_config` runs, in this order:

1. **Read `tray.json`.** Absent → defaults. Unparseable → the file is *moved
   aside* to `tray.json.corrupt`, a warning is logged, and defaults are used.
   The bad file is never deleted or overwritten, so nothing is lost.
2. **Migrate, only if `tray.json` does not exist.** Values are copied out of
   `settings.json` and written to `tray.json` atomically. Only after that write
   succeeds is `settings.json` copied to `settings.json.pre-tray-split` (once —
   an existing backup is never overwritten) and rewritten without the two keys.
   If the `tray.json` write fails, `settings.json` is left untouched.
3. **Fall back to legacy `config.json`** only when neither current file exists.

Because step 2 is guarded on `tray.json` being absent, repeat runs change
nothing — no rewrite, no new backup.

### Rollback to an older build

An older build reads the tray fields from `settings.json`, so restore the
backup:

```bash
cp ~/.config/lapsphere/settings.json.pre-tray-split ~/.config/lapsphere/settings.json
```

Moving forward again re-migrates on the next start.

One case does fall back to defaults: a corrupt `tray.json` on an *already
migrated* directory, because `settings.json` no longer holds the values. The
guarantee there is recoverability rather than continuity — the bad file is
preserved as `tray.json.corrupt`, and the pre-split backup still carries the
values.

## Write safety

`tray.json` and `settings.json` are both written via `tray_config::write_atomic`:
write to `<path>.tmp` in the same directory, `fsync`, then `rename`. A crash or
a full disk mid-write leaves either the old complete file or the new one, never
a half-written document that the next start would have to interpret.

## Tests

- `gui/src/tray_config.rs` — load/save defaults, per-field fallback, unknown
  fields, corrupt-file quarantine, migration idempotency, atomic writes, and the
  order of `resolve_tray_config`.
- `gui/src/app.rs::tray_migration_e2e_tests` — the real load and save path
  against a scratch config directory: clean dir, legacy dir, saving the split,
  repeat runs, rollback via the backup, and corruption before and after
  migration.