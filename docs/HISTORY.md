# LapSphere — feature history

What this file records: features. An entry states what the application gained — a subsystem, a
control path, a UI surface, an architecture change. Defects are not recorded here: a commit that
repairs behaviour gets no entry, and no entry describes an error, a crash or a regression. Fixes and
their diagnostics live in the commit log and the issue tracker.

How entries are dated: from the base branch `lapsphere`, never from the GitHub API's `merged_at`,
which is UTC and can land a day or a month away from the commit. A pull request's entry carries the
date of its merge commit; a direct commit's entry carries that commit's date; an entry that groups
several commits carries the date of the group's last commit. Unit commit counts are `git rev-list`
on `lapsphere` restricted to the window by author date — the same basis the entry dates use.

How entries are cited: work that landed through a pull request cites `PR #N`. Work that landed as
direct commits cites short SHAs, and each one resolves with `git cat-file -t` to `commit` and is an
ancestor of `lapsphere`. The first pull request appears on 2026-01-01, so every entry in unit 1 is
SHA-cited.

Covered up to: a06e5a2c17b9e0fa894540f61541e4996f666aa8 (2025-12-30)

## Unit accounting

The history is split into twelve chronological units plus a final unit for the commits after
2026-10-23, which is not written yet. The twelve bands partition 668 of the 671 commits on
`lapsphere`; no commit falls in two bands.

| Unit | Window | Commits | Written |
|------|--------|---------|---------|
| 1 | 2025-12-16 → 2025-12-30 | 71 | this pass: 30 commits in 13 entries + 41 excluded = 71 |
| 2 | 2026-01-01 → 2026-01-08 | 148 | pending |
| 3 | 2026-01-09 → 2026-01-17 | 95 | pending |
| 4 | 2026-01-18 → 2026-01-26 | 55 | pending |
| 5 | 2026-01-27 → 2026-01-31 | 61 | pending |
| 6 | 2026-02-01 → 2026-02-10 | 37 | pending |
| 7 | 2026-05-14 → 2026-05-22 | 8 | pending |
| 8 | 2026-08-22 → 2026-08-29 | 44 | pending |
| 9 | 2026-09-12 → 2026-09-30 | 33 | pending |
| 10 | 2026-10-03 → 2026-10-10 | 45 | pending |
| 11 | 2026-10-11 → 2026-10-16 | 63 | pending |
| 12 | 2026-10-17 → 2026-10-23 | 8 | pending |
| final unit | 2026-10-24 → 2026-10-25 | 3 | not written |

---

## 2025-12-16 → 2025-12-30 — Unit 1 (71 commits, no pull requests)

### 2025-12-16 — Initial import: Cargo workspace, D-Bus daemon, GTK4 shell

SHAs: `f67d26b` `5d52662`

The repository opens with its GPL-2.0 licence and then a three-crate Cargo workspace: `common`
(serde-serialised hardware and profile types), `daemon` (tokio + zbus service that exits unless
started as root) and `gui` (GTK4 0.9 and libadwaita 0.7, application id `com.tuxedo.ControlCenter`).
The GUI ships four libadwaita pages — profiles, settings, statistics, tuning — and the daemon
publishes `com.tuxedo.Control` on the system bus, with the D-Bus policy, systemd unit and Debian
packaging that go with it.

Files: `Cargo.toml`, `common/src/types.rs`, `daemon/src/{main,dbus_interface,hardware_control,hardware_detection,fan_daemon}.rs`,
`gui/src/{main,application,config,dbus_client}.rs`, `gui/src/ui/{window,profiles_page,settings_page,statistics_page,tuning_page}.rs`,
`daemon/com.tuxedo.Control.conf`, `debian/`

### 2025-12-27 — GitHub Actions build and release packaging

SHAs: `2578378` `7a83d19` `3e594f9`

Adds a `Rust` workflow that builds the workspace on `ubuntu-24.04` after installing the
GTK4/libadwaita/dbus development packages, then uploads the compiled binaries as artifacts. A later
step assembles `release-package/` from the two release binaries, the systemd and D-Bus files, the
README and licence; it generates an `install.sh` that installs both binaries into `/usr/local/bin`,
installs the service and policy files, reloads systemd and enables `tuxedo-daemon.service`, then
uploads the resulting tarball with 90-day retention.

Files: `.github/workflows/rust.yml`

### 2025-12-28 — Statistics page rebuilt as sections with live rows

SHAs: `1122a38`

`gui/src/ui/statistics_page.rs` becomes a set of `adw::PreferencesGroup` sections whose rows are kept
in per-section widget maps (`WidgetRefs`) and refreshed through a single `update_statistics`
dispatch. GPU rows are enumerated from `/sys/class/drm/{card,renderD}*` including the device's
`power/runtime_status`, WiFi from the wireless nodes in `/sys/class/net` by parsing `uevent` and
`modalias` into PCI ids and deriving a chip name, storage from `/sys/block`, and fans from
`tuxedo_io` with a fallback scan of `/sys/class/hwmon`. The System Information rows are created but
never filled in this commit — they keep the `Loading...` subtitle they are built with.

Files: `gui/src/ui/statistics_page.rs`

### 2025-12-28 — Tuning page rebuilt as five profile-backed sections

SHAs: `32de2b6`

`gui/src/ui/tuning_page.rs` gains a single `build_tuning_content` that assembles TDP, CPU tuning,
keyboard, screen and fan sections for the profile being edited, each section writing its values back
into that profile through the shared `Config`. The page takes its inputs from the daemon: CPU
information drives which controls the tuning section offers (for example the `amd_pstate` status
row), and the TDP combo is filled from the daemon's profile list.

Files: `gui/src/ui/tuning_page.rs`

### 2025-12-28 — Profiles page gains create, delete and error dialogs

SHAs: `88aae13`

`gui/src/ui/profiles_page.rs` builds the profile list from `Config` and adds the three missing
dialogs: a new-profile dialog that copies the current default profile's settings, refuses an empty
name and rejects a duplicate one through a shared error dialog; a destructive delete confirmation
that removes the named profile and saves; and the error dialog itself.

Files: `gui/src/ui/profiles_page.rs`

### 2025-12-28 — CPU load, power, temperature and capability detection

SHAs: `a761acd` `826ebc1`

`daemon/src/hardware_detection.rs` now derives per-core load from `/proc/stat` deltas, per-core
frequency from `cpufreq`, package temperature from the CPU `hwmon` node's `temp1_input`, and package
power by selecting the first available labelled source — `amdgpu` when an AMD CPU has no discrete
GPU, then `zenpower`, `amd_energy`, then `RAPL` — reporting which source was used alongside the
value. A new `CpuCapabilities` struct records which of the boost, cpufreq, EPP, governor, SMT and
`amd_pstate` nodes exist on the host, and the current energy-performance preference plus its
available values are read for the GUI.

Files: `daemon/src/hardware_detection.rs`, `common/src/types.rs`

### 2025-12-28 — TDP profile and energy-performance preference control over D-Bus

SHAs: `99501de` `2724cd6` `4720cb1` `2b187e7`

`CpuSettings` gains `tdp_profile` and `energy_performance_preference`, and applying a profile writes
the TDP performance profile to `/sys/devices/platform/tuxedo_io/performance_profile` and the EPP
value into every core's `cpufreq/energy_performance_preference`, with the EPP string checked against
a fixed list of accepted values. The daemon exposes `GetTdpProfiles`, `GetCurrentTdpProfile`,
`SetTdpProfile` and `SetEnergyPerformancePreference`, and the client carries matching calls. The same
series makes keyboard application discover the backlight node instead of hard-coding
`rgb:kbd_backlight`.

Files: `common/src/types.rs`, `daemon/src/{hardware_control,dbus_interface}.rs`, `gui/src/dbus_client.rs`

### 2025-12-29 — Fan, TDP and webcam access through the daemon

SHAs: `4793968` `132a2be` `392b8f3` `9d0f1f3` `7bb8eca`

`hardware_detection` adds `get_fan_speeds`, `get_fan_temperatures` and `get_tdp_info` (no earlier
form of these exists in the file) and repoints `get_tdp_profiles` and `get_current_tdp_profile` from
the `performance_profiles_available` and `performance_profile` nodes under
`/sys/devices/platform/tuxedo_io` to `TuxedoIo` ioctls — `TuxedoIo` itself arrives in `5258d35` the
same day (next entry), and until `7bb8eca` the statistics page read fan speeds directly from
`/sys/devices/platform/tuxedo_io/fan{N}_input` with an `hwmon` fallback. `hardware_control` gains
`set_tdp_profile` (profile name to index), `set_fan_speed` (clamped to 100), `set_fan_auto`, a
piecewise-linear `calculate_fan_speed_from_curve`, and webcam get/set. The daemon publishes
`GetFanSpeeds`, `GetFanTemperature`, `SetFanSpeed`, `SetFanAuto`, `GetWebcamState` and
`SetWebcamState`, and the client mirrors them. The statistics fan rows then show RPM and temperature
read from the daemon, falling back to `hwmon` inputs and hiding the rows when no fan is found.

Files: `daemon/src/{hardware_detection,hardware_control,dbus_interface}.rs`, `gui/src/dbus_client.rs`, `gui/src/ui/statistics_page.rs`

### 2025-12-30 — `/dev/tuxedo_io` ioctl layer with Clevo and Uniwill detection

SHAs: `5258d35` `9f1c998` `a8dcaff`

A new `daemon/src/tuxedo_io.rs` wraps `/dev/tuxedo_io` with `nix` ioctl macros on magic `0xEC`,
covering fan speed (`0x10` read, `0x11` write), fan temperature (`0x12`), the webcam switch (`0x13`
read, `0x14` write), TDP get/set/min/max with no index argument (`0x20`–`0x23`) and performance
profiles (`0x30`–`0x32`); fan auto and the per-index TDP commands are not in this revision. The
second revision splits the commands into a Clevo set (`0xEC+1` read, `0xEC+2` write) and a Uniwill
set (`0xEC+3`/`0xEC+4`), asks the kernel module which interface it is, probes the fan count instead
of assuming four, and packs Clevo's single all-fans write and the 1-based profile ids, adding fan
auto and the three TDP indices.

Files: `daemon/src/tuxedo_io.rs`, `daemon/Cargo.toml`

### 2025-12-30 — Battery charge control with a D-Bus surface

SHAs: `c7e4a75` `535a745` `9de9ec4` `188678f`

`daemon/src/battery_control.rs` adds `BatteryControl`, which picks the first of `BAT0`/`BAT1` that
exposes `charge_type` and reads or writes `charge_type` (Standard/Custom),
`charge_control_start_threshold` and `charge_control_end_threshold`, falling back to built-in
threshold lists when the `*_available_thresholds` nodes are absent, plus status, capacity,
manufacturer and model. The daemon exposes the type and threshold methods together with
`GetBatteryAvailableStartThresholds`, `GetBatteryAvailableEndThresholds` and
`GetHardwareInterfaceInfo`; the client mirrors them, and startup logs the detected interface, fan
count and flexicharger availability.

Files: `daemon/src/{battery_control,dbus_interface,main}.rs`, `gui/src/dbus_client.rs`

### 2025-12-30 — RGB keyboard backlight control

SHAs: `1b80eab`

`RgbKeyboardControl` locates a backlight under `/sys/class/leds` — trying `rgb:kbd_backlight`,
`tuxedo::kbd_backlight`, the `tuxedo_keyboard` platform LED and an Asus path — writes colour to
`multi_intensity` and scales a 0–100 brightness against `max_brightness`. A `KeyboardMode` enum
(Static, Breathing, Wave, Rainbow) writes the `mode` node where the hardware has one, and
`SetKeyboardRgb`, `SetKeyboardBrightness`, `GetKeyboardBrightness` and `SetKeyboardMode` (JSON) join
the D-Bus interface.

Files: `daemon/src/hardware_control.rs` (D-Bus methods added to the interface impl)

### 2025-12-30 — `FanCurveManager` and fan-curve D-Bus methods

SHAs: `f0a8d80`

`daemon/src/fan_daemon.rs` adds `FanCurveManager`, which holds the fan settings, applies each curve
by reading that fan's temperature and interpolating a speed, rate-limited to one application every
two seconds, with an auto-mode path. `SetFanCurve` (validates the point ranges), `GetFanCurve` and
`TestFanCurve` are added to the D-Bus interface, but the read side is a stub — `GetFanCurve` returns
an empty curve and `TestFanCurve` a fixed 50, the code noting that a global state manager does not
exist yet, and the manager is not instantiated anywhere in the tree at this point.

Files: `daemon/src/fan_daemon.rs`

### 2025-12-30 — Fan-curve editor moved from the daemon into the GUI

SHAs: `a4baf71` `5e690e4`

`create_fan_curve_editor` is deleted from `daemon/src/fan_daemon.rs`, where it had built GTK widgets
inside the daemon crate, and re-added in `gui/src/ui/tuning_page.rs`: it renders one
`adw::ActionRow` per curve point with temperature and speed spin buttons, a remove button, and an
`Add Point` button. In this commit the function is defined but not called from
`build_tuning_content`, and none of the controls it builds has a handler attached.

Files: `daemon/src/fan_daemon.rs`, `gui/src/ui/tuning_page.rs`

### Unit 1 accounting

71 commits = 30 named in the 13 entries above + 41 excluded:

- 13 CI workflow edits — runner image, dependency lists, artifact action version and path,
  artifact format, and the branch triggers (`1444787` `6a4df52` `8db3272` `24a0e57` `ea035fd`
  `53b7a00` `8628265` `d17d639` `eae3dc6` `5a62c30` `8ba5787` `b949274` `47cc586`).
- 27 refactors, plumbing and formatting edits — module and import paths, layout expansion flags,
  a removed function, an unused-function deletion, field-type changes, whitespace-only reindents,
  the add-then-remove of `create_cpu_tuning_section`, and the flip-flop between
  `get_performance_profiles` and `get_available_profiles` (`272bbae` `ab109ef` `7613320` `228dcd2`
  `c04b524` `cf34891` `d383b5a` `cc490d6` `d889e61` `05f38a2` `3142ae9` `9aba7fc` `8a9e6a6`
  `66dabf7` `e8bb47b` `8a93a54` `089db69` `a1e6893` `6e32bcd` `9c53923` `4f7f80c` `cc5121d`
  `25663c9` `c3bcfc4` `1c5cdf6` `6016fec` `a06e5a2`).
- 1 fix — `f86da8f` drops the current-profile ioctl lookup and falls back to the first profile.

No pull requests were merged in this window; all 71 commits are direct commits to the base line.
