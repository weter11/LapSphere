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

How entries are cited: work that landed through a pull request cites `PR #N`, its merge commit and the
branch commits that arrived with it, so every commit in a window is either named by an entry or
listed as excluded. Work that landed as direct commits cites short SHAs, and each one resolves with
`git cat-file -t` to `commit` and is an ancestor of `lapsphere`. The first pull request appears on
2026-01-01, so every entry in unit 1 is SHA-cited.

Covered up to: 9cbecab8df11bbb03c907b724e693e04c2d632fe (2026-01-07)

## Maintainer's recollections

Stated 2026-10-25. Not derived from commits, and not verified unless a line says so.

- Early branch experiments: the maintainer did not yet know how to work with branches and created
  extra ones (`main`, `master`, others) to separate experiments from the main line; `lapsphere` later
  became the main line.
- UI toolkit: GTK4 and Qt6 were both tried; because of many problems the maintainer moved to a
  Rust-only UI and settled on egui.
- Manual fan mode and keyboard backlight each took a long time to get working; the maintainer knew
  little at the time, the agents' code was weak, tests were run by hand and what was tried was not
  written down. Agents did not run live tests on the laptop.

## Repository evidence

What the repository records of those recollections, as the commands return it.

- `master` is a single orphan commit `ba34072` (2025-12-16, "Initial commit with my project") with no
  parent, and it is not an ancestor of `lapsphere`; `git diff --stat ba34072 f67d26b` reports 50 files
  changed, 339 insertions, 5466 deletions.
- `main` was folded into the main line by `cd13c4e` ("Merge pull request #9 from weter11/main") on
  2026-01-03; its tip `47cc586` is dated 2025-12-29 and is an ancestor of `lapsphere`.
- 145 refs under `refs/remotes/origin` (excluding `HEAD`), counted 2026-10-25.
- The workflow's push triggers name these branches in four commits: `5a62c30` (`main` →
  `dev-fan-logic-clean`), `8ba5787` (`dev-fan-logic-clean` → `new_variant`), `b949274` (`main` and
  `new_variant`), `47cc586` (adds `perplexity`).
- `gui/Cargo.toml` at `5d52662` declares `gtk = { version = "0.9", package = "gtk4", features =
  ["v4_14"] }` and `libadwaita = { version = "0.7", features = ["v1_5"] }`.
- PR #16 (`e0935b5`) replaces those two lines with `egui = { workspace = true }` and
  `eframe = { workspace = true }`; the merge commit's whole message is "Convert UI from GTK4 to egui",
  and no reason is recorded.
- No Qt6 trace in the repository: across all 145 refs, 0 `*Cargo.toml` files match
  `cxx-qt|qt6|qml|qt_core`, 0 paths match `qml` or `qt*.rs`/`qt*.toml`/`qt*.ui`, and 0 commit messages
  match `--grep=qt`.
- At `9cbecab` the tree holds 0 test files (paths matching `tests/` or `*_test.rs`).

To be placed in unit 2b, egui entry: "the maintainer's account of 2026-10-25 records that GTK4 and a
Qt6 attempt were both tried and that the UI moved to egui because of problems with both; no commit or
ADR records a reason."

## Unit accounting

The history is split into twelve chronological units (unit 2 is written in two halves) plus a final
unit for the commits after 2026-10-23, which is not written yet. The twelve bands partition 668 of
the 671 commits on `lapsphere`; no commit falls in two bands.

| Unit | Window | Commits | Written |
|------|--------|---------|---------|
| 1 | 2025-12-16 → 2025-12-30 | 71 | this pass: 30 commits in 13 entries + 41 excluded = 71 |
| 2a | 2026-01-01 → 2026-01-07 | 86 | this pass: 86 commits = 38 in entries + 48 excluded |
| 2b | 2026-01-06 → 2026-01-08 | 62 | pending |
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

Unit 2 is split at `9cbecab`, the merge of PR #15, so the two halves are bounded by commit rather
than by date: 2b begins at the commit after that merge, and its author dates reach back to
2026-01-06 in the branches merged later.

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

## 2026-01-01 → 2026-01-07 — Unit 2a (86 commits, first half of unit 2)

Unit 2 opens with the repository's first pull requests and closes on 2026-01-08; it is written in two
halves split at the merge of PR #15. This half holds PRs #5, #6, #9, #10, #11 and #15 plus the direct
commits of the `new_variant` line, and ends before the egui migration.

### 2026-01-01 — `FanCurveEditor` widget module

SHAs: `43ab7d8`

`gui/src/ui/fan_curve_editor` adds the `FanCurveEditor` widget: a `DrawingArea` whose `draw_curve`
renders the points, one row per curve point with a temperature and a speed `SpinButton`, and a
`get_curve` accessor for the edited curve. The file is committed without the `.rs` extension and
`gui/src/ui/mod.rs` declares no module for it, so the tree does not compile it until PR #10 renames
the file and adds `pub mod fan_curve_editor;`.

Files: `gui/src/ui/fan_curve_editor`

### 2026-01-01 — Eight keyboard effect modes and per-section polling rates (PR #5)

SHAs: `ec7c584` `6220efa` `9d3e259` `d38e711` `85ead46` `564d2d1`

PR #5 (merge `ec7c584`). `KeyboardMode` drops its `Effect { effect, speed }` variant for eight
concrete ones — `SingleColor`, `Breathe`, `Cycle`, `Dance`, `Flash`, `RandomColor`, `Tempo` and
`Wave`. `RgbKeyboardControl::set_mode` writes an effect variant's keyword (`breathing`, `wave`,
`cycle`, `dance`, `flash`, `random`, `tempo`) into the backlight's `mode` node where that node exists,
sets colour and brightness for the variants that carry those fields, and returns `Err` for `wave`,
`cycle`, `dance`, `random` and `tempo` where it does not. The tuning page gains a mode `ComboRow` that
rebuilds its controls for the selected mode. Hardware support is not established by the diff.
`StatisticsSections` gains seven polling rates with defaults (`system_info_poll_rate` 60000 ms,
`cpu_poll_rate` 1000, `gpu_poll_rate` 2000, `battery_poll_rate` 5000, `wifi_poll_rate` 5000,
`storage_poll_rate` 30000 and `fans_poll_rate` 1000), and the settings page gains the rows that edit
them. `WiFiInfo` gains
`signal_level`, `channel`, `channel_width`, `tx_rate` and `rx_rate`; `FanInfo` gains `temperature` and
`is_rpm`. The same merge deletes the committed `Cargo.lock` (1967 lines removed) and adds a 21-line
`.gitignore`.

Files: `common/src/types.rs`, `daemon/src/hardware_control.rs`,
`gui/src/ui/{tuning_page,settings_page,statistics_page}.rs`, `.gitignore`, `Cargo.lock`

### 2026-01-01 — Keyboard preview, inline profile creation and per-section polling (PR #6)

SHAs: `d164c3f` `954ebd6` `3c026a3` `e639bff` `161b54f` `2b8f356` `25a9e22`

PR #6 (merge `d164c3f`). `PreviewKeyboardSettings` applies a `KeyboardSettings` value to the backlight
without storing it in a profile, and the tuning page's RGB, brightness and speed rows call it on every
change so the keyboard follows the sliders. The profiles page gains an inline creation group — a name
entry and a `Create` button — that answers a duplicate name with an `adw::Toast` found through a
`get_toast_overlay`/`find_toast_overlay_recursive` walk of the widget tree. Statistics polling is split
into `start_background_poll_system`, `_cpu`, `_battery` and `_fans`; the first, second and fourth
spawn a `std::thread` that sleeps for its section's poll rate, `_battery` uses
`glib::timeout_add_seconds_local`. `hardware_detection` adds `get_wifi_info` with
`read_wifi_signal`, `read_wifi_channel` and `read_wifi_rates`, the tuning page gains add and update
handlers for fan-curve points, and the window answers a close request. `AppConfig` gains
`auto_profile_switching` and the settings page a switch that saves it; nothing in the tree reads the
flag to switch profiles.

Files: `daemon/src/{dbus_interface,hardware_control,hardware_detection}.rs`, `gui/src/dbus_client.rs`,
`gui/src/ui/{profiles_page,settings_page,statistics_page,tuning_page,window}.rs`, `common/src/types.rs`

### 2026-01-03 — `GetWifiInfo` and `GetFanInfo` on the daemon (PR #10)

SHAs: `08e4ca2` `34b1761` `99e0888` `3b494cf` `66767d0` `0c5ac7c` `8f20b11` `85c3668`

PR #10 (merge `08e4ca2`). The daemon publishes `GetWifiInfo`, which serialises the WiFi rows to JSON,
and `GetFanInfo`, which walks `TuxedoIo::get_fan_count` and reads each fan's speed and temperature into
a `FanInfo` with `is_rpm: false` because the ioctl reports percent, returning `[]` when
`/dev/tuxedo_io` is absent; `DbusClient` gains `get_wifi_info` and `get_fan_info`. The fan curve editor
enters the build: `gui/src/ui/mod.rs` declares `pub mod fan_curve_editor;` and the file is renamed from
`fan_curve_editor` to `fan_curve_editor.rs`.

Files: `daemon/src/{dbus_interface,fan_daemon,hardware_detection}.rs`, `gui/src/dbus_client.rs`,
`gui/src/ui/{mod,fan_curve_editor,statistics_page}.rs`

### 2026-01-04 — Clevo fan scale conversion and four performance profiles

SHAs: `b7d7a9c` `70b7a00` `47adae7`

`TuxedoIo` gains `clevo_raw_to_percent` and `clevo_percent_to_raw`, mapping Clevo's 0–255 fan byte onto
the 0–100 percentage the D-Bus surface uses. `get_available_profiles` returns four Clevo entries —
`quiet` (0x00), `power_saving` (0x01), `performance` (0x02), `entertainment` (0x03) — in place of the
earlier three-name list, and `set_performance_profile` changes the Clevo profile id mapping from 1–3
(`a8dcaff` wrote `profile_id + 1`) to 0–3, rejecting an id above 3. The Clevo fan reads log their raw
`faninfo` words.

Files: `daemon/src/tuxedo_io.rs`

### 2026-01-05 — Manual fan mode from the ioctl layer

SHAs: `da0c83d`

`TuxedoIo::set_fan_manual` writes 0 to the auto register — `ioctl_cl_fanauto` on Clevo,
`ioctl_uw_fanauto` on Uniwill — and `set_fan_speed` calls it before the speed ioctl, the comment at
that call giving EC overwrite as the reason. A `Drop` impl calls `set_fan_auto` when the handle goes
away; it is deleted the same day in `92c29a4` (excluded below). Hardware support is not established by
the diff.

Files: `daemon/src/tuxedo_io.rs`

### 2026-01-06 — Fan curve loop in the daemon process

SHAs: `b2bba7f` `953c9f9`

`daemon/src/main.rs` gains `FAN_DAEMON_STATE`, a
`once_cell::sync::Lazy<Arc<Mutex<Option<FanSettings>>>>` (the `once_cell` dependency is added to
`daemon/Cargo.toml` by `b2bba7f`), and a `tokio::spawn` task that wakes every two seconds and re-applies
the stored curves through `apply_fan_curves`. `apply_fan_settings` fills the state when an applied
profile enables fan control and clears it when it does not. `FanCurveManager` in `fan_daemon.rs` is
still not instantiated anywhere in the tree; the loop calls the free function in `main.rs`.

Files: `daemon/src/main.rs`, `daemon/src/hardware_control.rs`, `daemon/Cargo.toml`

### 2026-01-06 — Battery charge control in the GUI

SHAs: `afce3b7` `60500fd`

The settings page gains a `Battery Charge Control` group: a charge-type `ComboRow` over
`standard`/`express`/`primarily_ac`, and start and end threshold rows built from
`GetBatteryAvailableStartThresholds`/`GetBatteryAvailableEndThresholds` as `Scale` widgets that write
back through `DbusClient`. The statistics page gains a battery section whose charge-type and threshold
rows are created hidden and filled only when the daemon answers. PR #15 withdraws the editable group
from the settings page again (`gui/src/ui/settings_page.rs` +1/−121 there); the read-only statistics
rows are what survives this half.

Files: `gui/src/ui/settings_page.rs`, `gui/src/ui/statistics_page.rs`

### 2026-01-06 — Points list in the fan curve editor, edits applied live

SHAs: `ba7a262` `0c85931`

`FanCurveEditor` moves its canvas into a `gtk::Frame` sized 600×400 and keeps the points in a
`gtk::ListBox` that `rebuild_points_list` refills whenever a row changes. `build_tuning_content` takes
the page's `ScrolledWindow`; the keyboard mode selector calls `PreviewKeyboardSettings` and rebuilds
the page so the controls for that mode appear, the brightness slider previews as it moves, and the
screen section writes the percentage into `/sys/class/backlight/*/brightness`, scaled against
`max_brightness`, unless the profile leaves the display under system control.

Files: `gui/src/ui/fan_curve_editor.rs`, `gui/src/ui/tuning_page.rs`

### 2026-01-07 — Fan curve editor integrated into the tuning page (PR #15)

SHAs: `9cbecab` `2cbf57d` `3c5480e` `d0c6b7e` `5b359bb` `26a7175`

PR #15 (merge `9cbecab`). The editor is rebuilt as two panes — a points `ListBox` with add and remove on
the left, a 500×400 `DrawingArea` on the right that renders the curve from `set_draw_func`.
`create_fans_tuning_section` creates one
`FanCurveEditor` per fan, two when the profile defines no curves, and copies each editor's `get_curve`
back into the profile when it is saved. The battery charge group leaves the settings page for a
`create_battery_section` on the statistics page, whose CPU rows are now shown or hidden from
`CpuInfo::capabilities`; `apply_curves` skips the ioctl when the computed speed equals the last one
written, kept per fan in `last_speeds`. The branch merges `new_variant` (`26a7175`) before it lands.

Files: `gui/src/ui/{fan_curve_editor,tuning_page,statistics_page,settings_page,window}.rs`,
`daemon/src/fan_daemon.rs`

### Unit 2a accounting

86 commits = 38 named in the 10 entries above + 48 excluded:

- 5 CI workflow edits — dependency lists, branch triggers and the merge of PR #9, whose branch commits
  carry 2025-12-28/29 author dates and are counted in unit 1
  (`cd13c4e` +4/−1, `1984acc` +3/−2, `a6adc6d` +2/−1, `14a2592` +5/−1, `d661428` +2/−1).
- 3 commits of PR #11, a fix with no new functionality — the merge `a2acf15` (+4/−4), `174a55c`
  (+4/−4), and the empty `7fc8159` (+0/−0).
- 2 fix commits — `ea557c5` (+1/−1) downcast check, `ac31131` (+2/−1) profile-not-found handling.
- 28 refactors, plumbing and UI restructuring — `695a42c` (+67/−45), `c0afcdd` (+37/−44),
  `c036e9e` (+23/−17), `361eb61` (+126/−138), `f17f80e` (+163/−167), `74966d1` (+15/−9),
  `ac0dee0` (+27/−56), `603eddb` (+121/−109), `a6c0a2a` (+18/−14), `b9e0b6c` (+23/−29),
  `7555e06` (+79/−117), `f2414fb` (+116/−109), `c8e6bb3` (+35/−31), `c5654f8` (+32/−35),
  `4c4d878` (+20/−6), `4eb946b` (+51/−41), `91c3a0d` (+211/−225), `d9cc2cd` (+3/−3),
  `4d4af2a` (+14/−70), `3d541a5` (+1/−1), `cf793f1` (+2/−2), `d71c234` (+55/−96), `28791f6` (+1/−1),
  `7533981` (+147/−123), `c4a9069` (+79/−67), `92c29a4` (+0/−9), `22fc78b` (+51/−9) and
  `8352728` (+37/−106). Two of them are a pair: `22fc78b` makes the tuning page rebuild itself on a
  profile change and `8352728` takes the rebuild out again, leaving the header-only update. `92c29a4`
  removes the `Drop` guard that the manual-fan-mode entry above describes. `f2414fb` introduces the
  16-point cap on a curve and the 10 °C step for a new point, both kept by PR #15.
- 5 label and icon edits — `e4fddfb` (+4/−4), `e02fa86` (+2/−2), `1d0cb80` (+9/−11),
  `b318824` (+1/−1), `90bcdfc` (+1/−1).
- 5 formatting and logging edits — `9e0efe6` (+0/−3), `40dadea` (+0/−1), `7c8ead9` (+0/−1),
  `0fb8fd0` (+1/−0), `c9f35df` (+8/−0).

Six pull requests are merged in this half: #5, #6, #9, #10, #11 and #15 — of which #5, #6, #10 and #15
carry entries and #9 (a workflow-only merge of `main`) and #11 (a fix) are excluded above. The
four empty `Initial plan` commits sit inside those branches: three are counted with the PRs above and
the one in PR #11 is in the excluded list.
