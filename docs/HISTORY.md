# LapSphere — feature history

What this file records: features. An entry states what the application gained — a subsystem, a
control path, a UI surface, an architecture change. Defects are not recorded here: a commit that
repairs behaviour gets no entry, and no entry describes an error, a crash or a regression. Fixes and
their diagnostics live in the commit log and the issue tracker.

How entries are dated: from `origin/lapsphere` at 79abeab51f8295828d7bd65f49a067df049d0ba2, never
from the GitHub API's `merged_at`, which is UTC and can land a day or a month away from the commit. A pull request's entry carries the
date of its merge commit; a direct commit's entry carries that commit's date; an entry that groups
several commits carries the date of the group's last commit. Unit commit counts are `git rev-list`
on `origin/lapsphere` restricted to the window by author date — the same basis the entry dates use.

How entries are cited: work that landed through a pull request cites `PR #N`, its merge commit and the
branch commits that arrived with it, so every commit in a window is either named by an entry or
listed as excluded. Work that landed as direct commits cites short SHAs, and each one resolves with
`git cat-file -t` to `commit` and is an ancestor of `origin/lapsphere`. The first pull request appears on
2026-01-01, so every entry in unit 1 is SHA-cited.

How entries are classified: a change is a feature when the net diff leaves the application producing
something it did not produce before — a value, a row, a log stream, a control, or a hardware call
withheld in a state where it was previously issued — and a repair when the same calls happen in the
same states and only their arguments, parsing or formatting change. A change that is both is
documented for the new functionality alone, and a wholly repair-only pull request goes to the unit's
excluded commits.

Covered up to: 3b7bd8f25fc96f07c7ad76de0607a2e1f171193f (2026-10-14)

## Maintainer's recollections

Stated 2026-10-25 and 2026-10-26, as each bullet says. Not derived from commits, and not verified
unless a line says so.

- Early branch experiments: the maintainer did not yet know how to work with branches and created
  extra ones (`main`, `master`, others) to separate experiments from the main line; `lapsphere` later
  became the main line.
- UI toolkit: GTK4 and Qt6 were both tried; because of many problems the maintainer moved to a
  Rust-only UI and settled on egui.
- Manual fan mode and keyboard backlight each took a long time to get working; the maintainer knew
  little at the time, the agents' code was weak, tests were run by hand and what was tried was not
  written down. Agents did not run live tests on the laptop.
- (Stated 2026-10-26.) Early pull requests often lack a description, or misstate what they changed:
  the agents had no access to the laptop hardware until around August 2026, and the maintainer's own
  knowledge of the codebase was limited at the time, so neither side could check a claim against a
  running machine.
- (Stated 2026-10-26.) Development paused after February 2026 and again between May and August while
  the maintainer worked on another project.

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
- The two development pauses show as the only gaps longer than 14 days between author dates:
  2026-02-10 → 2026-05-14 (93 days) and 2026-05-22 → 2026-08-22 (92 days), from
  `git log --format=%ad --date=short | sort | uniq -c`.

## Unit accounting

The history is split into twelve chronological units (units 2, 3, 5 and 11 are written in two
halves) plus a final unit for the commits after 2026-10-23, which is not written yet. The bands
are bounded by commit rather than by date, so adjacent windows overlap where a branch merged late.
Together they partition the branch: units 1 and 2 hold the 219 commits behind `c8189d3`, unit 3 the
96 up to `05a0918`, unit 4 the 51 up to `1d91b17`, and units 5 to 12 with the final unit the
remaining 305 — 219 + 96 + 51 + 305 = 671, the commit count of `origin/lapsphere`. A row marked
"written in two halves" is split at a merge when that unit is written.

| Unit | Window | Commits | Written |
|------|--------|---------|---------|
| 1 | 2025-12-16 → 2025-12-30 | 71 | this pass: 30 commits in 13 entries + 41 excluded = 71 |
| 2a | 2026-01-01 → 2026-01-07 | 86 | unit 2a: 86 = 39 in 11 entries + 47 excluded |
| 2b | 2026-01-06 → 2026-01-08 | 62 | unit 2b: 62 = 43 in 9 entries + 19 excluded (unit 2: 86 + 62 = 148) |
| 3a | 2026-01-09 → 2026-01-13 | 48 | unit 3a: 48 = 30 in 11 entries + 18 excluded |
| 3b | 2026-01-14 → 2026-01-19 | 48 | unit 3b: 48 = 44 in 6 entries + 4 excluded (unit 3: 48 + 48 = 96) |
| 4 | 2026-01-19 → 2026-01-26 | 51 | unit 4: 51 = 43 in 9 entries + 8 excluded |
| 5 | 2026-01-26 → 2026-02-03 | 77 | unit 5a: 40 = 40 in 7 entries + 0 excluded; unit 5b: 37 = 25 in 3 entries + 12 excluded |
| 6 | 2026-02-03 → 2026-02-10 | 24 | unit 6: 24 = 24 in 4 entries + 0 excluded |
| 7 | 2026-05-14 → 2026-05-22 | 8 | unit 7: 8 = 6 in 1 entry + 2 excluded |
| 8 | 2026-08-22 → 2026-08-29 | 41 | unit 8: 41 = 4 in 2 entries + 37 excluded |
| 9 | 2026-08-25 → 2026-09-29 | 35 | unit 9: 35 = 29 in 5 entries + 6 excluded |
| 10 | 2026-09-30 → 2026-10-10 | 25 | unit 10: 25 = 16 named in the 4 entries above + 9 excluded |
| 11a | 2026-10-10 → 2026-10-14 | 41 | unit 11a: 41 = 39 named in the 1 entry above + 2 excluded |
| 11b | 2026-10-14 → 2026-10-16 | 43 | pending — `17ece90` (#122) to `f174a8e` (#132) |
| 12 | 2026-10-23 | 8 | pending — `75d8767` (#133) to `2ff246f` (#136) |
| final unit | 2026-10-25 | 3 | not written — `79abeab` (#140) |

Unit 2 is split at `9cbecab`, the merge of PR #15, so the two halves are bounded by commit rather
than by date: 2b begins at the commit after that merge, and its author dates reach back to
2026-01-06 in the branches merged later. Unit 3 is split at `1532df6`, the merge of PR #25: 3a runs
from the commit after `c8189d3` to that merge, 48 commits, and 3b from there to the last first-parent
commit before the merge of PR #51, also 48; that boundary commit is dated 2026-01-19, so 3b's window
reaches past 3a's. Unit 5 is split at `e6b1476`, the merge of PR #72: 5a runs from `35db664` to that
merge, 40 commits, and 5b the remaining 37. Unit 11 is split at `3b7bd8f`, the merge of PR #121:
11a runs from `b1c8719`'s branch to that merge, 41 commits, and 11b the remaining 43.

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
`/sys/devices/platform/tuxedo_io/fan{N}_input` with an `hwmon` fallback. `hardware_control` rewrites
`set_tdp_profile` to look the profile name up in `get_performance_profiles` and pass its index to
`set_performance_profile` — the function first exists in `4720cb1`, writing the name to the sysfs node
— and gains `set_fan_speed` (clamped to 100), `set_fan_auto`, a piecewise-linear
`calculate_fan_speed_from_curve`, and webcam get/set. The daemon publishes
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
away; it is deleted the same day in `92c29a4` (excluded below). `7533981` removes that call from
`set_fan_speed`, its comment at line 289 saying the speed write implicitly disables auto mode;
`91c3a0d` puts an explicit auto write back in the Clevo arm, so at `9cbecab` the Clevo arm writes 0 to
request `0x11` (lines 274–276) and the Uniwill arm writes only its speed request (lines 313–327),
nothing to the auto request `0x14`. Hardware support is not established by the diff.

Files: `daemon/src/tuxedo_io.rs`

### 2026-01-05 — `Reset to Saved` applies the stored profile from the tuning page

SHAs: `22fc78b`

The `↺ Reset to Saved` button on the tuning page, present since `32de2b6` with no handler, gains one: it
reloads `Config` from disk, replaces the in-memory data and applies the named profile through
`ApplyProfile`.

Files: `gui/src/ui/tuning_page.rs`

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
back through `DbusClient`. The statistics page's battery section, which exists since `1122a38`, gains
charge-type and threshold rows, created hidden and filled only when the daemon answers. PR #15 withdraws the editable group
from the settings page again (`gui/src/ui/settings_page.rs` +1/−121 there); the read-only statistics
rows are what survives this half.

Files: `gui/src/ui/settings_page.rs`, `gui/src/ui/statistics_page.rs`

### 2026-01-06 — Points list in the fan curve editor, edits applied live

SHAs: `ba7a262` `0c85931`

`FanCurveEditor` moves its canvas into a `gtk::Frame` sized 600×400, makes its points `gtk::ListBox`
— present since `603eddb` — a struct field, and adds `rebuild_points_list`, which refills it whenever a
row changes. `build_tuning_content` takes
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
back into the profile when it is saved. The battery charge group leaves the settings page for the
statistics page's `create_battery_section`, which is rewritten with a signature that drops the client
argument, and the statistics page's CPU rows keep the capability-driven visibility present since
`1122a38`; `apply_curves` skips the ioctl when the computed speed equals the last one written, kept per
fan in `last_speeds`. The branch merges `new_variant` (`26a7175`) before it lands. The files this PR
edits under `gui/src/ui` are restored to their pre-PR content by `a408870`, `a371d2b` and `1bffbea` —
except `fan_curve_editor.rs`, which keeps the PR's version — and deleted by `2d3f26e` (unit 2b).

Files: `gui/src/ui/{fan_curve_editor,tuning_page,statistics_page,settings_page,window}.rs`,
`daemon/src/fan_daemon.rs`

### Unit 2a accounting

86 commits = 39 named in the 11 entries above + 47 excluded:

- 5 CI workflow edits — dependency lists, branch triggers and the merge of PR #9, whose branch commits
  carry 2025-12-28/29 author dates and are counted in unit 1
  (`cd13c4e` +4/−1, `1984acc` +3/−2, `a6adc6d` +2/−1, `14a2592` +5/−1, `d661428` +2/−1).
- 3 commits of PR #11, a fix with no new functionality — the merge `a2acf15` (+4/−4), `174a55c`
  (+4/−4), and the empty `7fc8159` (+0/−0).
- 2 fix commits — `ea557c5` (+1/−1) downcast check, `ac31131` (+2/−1) profile-not-found handling.
- 27 refactors, plumbing and UI restructuring — `695a42c` (+67/−45), `c0afcdd` (+37/−44),
  `c036e9e` (+23/−17), `361eb61` (+126/−138), `f17f80e` (+163/−167), `74966d1` (+15/−9),
  `ac0dee0` (+27/−56), `603eddb` (+121/−109), `a6c0a2a` (+18/−14), `b9e0b6c` (+23/−29),
  `7555e06` (+79/−117), `f2414fb` (+116/−109), `c8e6bb3` (+35/−31), `c5654f8` (+32/−35),
  `4c4d878` (+20/−6), `4eb946b` (+51/−41), `91c3a0d` (+211/−225), `d9cc2cd` (+3/−3),
  `4d4af2a` (+14/−70), `3d541a5` (+1/−1), `cf793f1` (+2/−2), `d71c234` (+55/−96), `28791f6` (+1/−1),
  `7533981` (+147/−123), `c4a9069` (+79/−67), `92c29a4` (+0/−9) and `8352728` (+37/−106). `8352728`
  removes the tuning page's full-page rebuild, leaving the header-label update, and the inline per-row
  delete button that unit 1's fan-curve editor added. `92c29a4` removes the `Drop` guard that the manual-fan-mode entry
  above describes. `f2414fb` introduces the 16-point cap on a curve and the 10 °C step for a new point,
  both kept by PR #15.
- 5 label and icon edits — `e4fddfb` (+4/−4), `e02fa86` (+2/−2), `1d0cb80` (+9/−11),
  `b318824` (+1/−1), `90bcdfc` (+1/−1).
- 5 formatting and logging edits — `9e0efe6` (+0/−3), `40dadea` (+0/−1), `7c8ead9` (+0/−1),
  `0fb8fd0` (+1/−0), `c9f35df` (+8/−0).

Six pull requests are merged in this half: #5, #6, #9, #10, #11 and #15 — of which #5, #6, #10 and #15
carry entries and #9 (a workflow-only merge of `main`) and #11 (a fix) are excluded above. The
four empty `Initial plan` commits sit inside those branches: three are counted with the PRs above and
the one in PR #11 is in the excluded list.

## 2026-01-06 → 2026-01-08 — Unit 2b (62 commits, second half of unit 2)

This half carries the egui migration (PR #16 and the direct commits that follow it), the system tray
(PR #17), battery charge thresholds as a setting (PR #18), and the deletion of the repository's
packaging and service files. Its author dates start at 2026-01-06 because four commits dated then
restore the pre-PR-#15 content of files under `gui/src/ui/`; they are first-parent commits and sit
between `9cbecab` and PR #16's merge.

### 2026-01-07 — egui dependencies and the first egui modules (PR #16)

SHAs: `e0935b5` `f8030d2`

PR #16 (merge `e0935b5`) is the net diff `e0935b5^1..e0935b5`: 10 files, 6 added, 4 modified, 0
deleted, +255/−23; the GTK deletion is the direct commit `2d3f26e`, not part of this PR. The workspace
manifest gains the `[workspace.dependencies]` block that is absent at `e0935b5^1` — `egui = "0.28.0"`
and `eframe = "0.28.0"` — `gui/Cargo.toml` replaces
`gtk = { version = "0.9", package = "gtk4", features = ["v4_14"] }` and
`libadwaita = { version = "0.7", features = ["v1_5"] }` with `egui = { workspace = true }` and
`eframe = { workspace = true }`, and `common/src/types.rs` adds `PartialEq` to `Theme`. The six added
files are `gui/src/egui_ui/{app,mod,profiles_page,settings_page,statistics_page,tuning_page}.rs`, run
from an eframe entry point in `gui/src/main.rs`; `gui/src/egui_ui/` is removed in `a96d7f3` and is
not present at `c8189d3`, where `gui/src` holds no `egui_ui` path and no `mod egui_ui`, and it
contains no fan-curve code — the GTK editor goes with its directory in `2d3f26e` (next entry) and a
fan-curve editor returns in `dc1e53b`. The merge message is "Convert UI from GTK4 to egui", the reason is not
recorded in any commit or ADR, and the maintainer's account of 2026-10-25 records that GTK4 and a Qt6
attempt were both tried and that the UI moved to egui because of problems with both, the Qt6 attempt
leaving no trace in the 145 refs.

Files: `Cargo.toml`, `gui/Cargo.toml`, `gui/src/main.rs`,
`gui/src/egui_ui/{app,mod,profiles_page,settings_page,statistics_page,tuning_page}.rs`,
`common/src/types.rs`

### 2026-01-07 — GTK tree deleted, egui shell rebuilt as flat modules

SHAs: `2d3f26e` `a96d7f3` `28b2f99` `2b5c6d6` `5c24903` `794fe31` `730d6ac` `07e307f` `672e659`

`2d3f26e` deletes the seven files of the GTK UI —
`gui/src/ui/{fan_curve_editor,mod,profiles_page,settings_page,statistics_page,tuning_page,window}.rs`,
3570 lines — and `07e307f` and `672e659` delete the two GTK-era files outside that directory,
`gui/src/application.rs` (126 lines) and `gui/src/config.rs` (49). `a96d7f3` deletes the `egui_ui/`
scaffold PR #16 had just added (6 files, 234 lines), and the UI is rebuilt as flat modules:
`gui/src/app.rs` with `Page`, `AppState`, `StatusMessage` and `PollTimers`, `gui/src/theme.rs` with
`TuxedoTheme` (`apply`, `temp_color`, `load_color`, `power_color`), and a `gui/src/main.rs` reduced to
the module list and the eframe entry point. `28b2f99` moves `egui` and `eframe` from the workspace's
`0.28.0` to `0.29`, pinning `egui = "0.29"`, `eframe = "0.29"` and
`egui_extras = { version = "0.29", features = ["all_plots"] }` in `gui/Cargo.toml`, adds `tokio` with
`rt-multi-thread`/`sync`/`time` and `chrono = "0.4"`, sets a release profile and renames the package
to `tuxedo-control-center-egui`. `794fe31` writes a second copy of the app structures into `main.rs` (386
lines, the file at 415), so the types exist twice until `464ab11` removes the copy.

Files: `gui/src/{app,theme,main,application,config}.rs`, `gui/src/ui/`, `gui/src/egui_ui/`,
`gui/Cargo.toml`

### 2026-01-07 — egui pages: statistics, profiles, tuning, settings

SHAs: `12e94e3` `68de3f0` `cf8c21e` `64d287d` `2ef9fc4`

`12e94e3` builds the statistics page as `draw` plus `draw_system_info`, `draw_cpu_info`,
`draw_gpu_info`, `draw_battery_info` and `draw_fan_info`, all reading `AppState`; `68de3f0` adds
`profiles::draw`, `cf8c21e` the tuning page with `draw_cpu_tuning`, `draw_keyboard_tuning`,
`draw_screen_tuning` and `draw_fan_tuning`, `64d287d` the settings page, and `2ef9fc4` declares the
four page modules.

Files: `gui/src/pages/{statistics,profiles,tuning,settings,mod}.rs`

### 2026-01-07 — WiFi and storage rows in the egui statistics page

SHAs: `ca95165` `881f023` `ba741a9`

`ca95165` adds `draw_wifi_info` to the egui statistics page, `ba741a9` adds `read_storage_info` and
`find_storage_temp` to `gui/src/app.rs`, and `881f023` adds `draw_storage_info` beside the WiFi rows.

Files: `gui/src/pages/statistics.rs`, `gui/src/app.rs`

### 2026-01-08 — Fan curve editor on egui_plot

SHAs: `dc1e53b` `9e126ec` `f07a516` `7d73c21` `bd8ecb8` `638f684` `e5291bc`

`dc1e53b` adds `FanCurveEditor` with `new`, `show` and `get_curve`, and `9e126ec` replaces the tuning
page's "Fan curve editor coming soon... For now, use the GTK version to create fan curves."
placeholder with one editor per fan, creating a curve for every fan the daemon reports. `bd8ecb8` adds
`egui_plot = "0.29.0"`, which the editor uses for `Plot`, `Line`, `Points` and `Polygon`, and
`f07a516` and `7d73c21` declare the `widgets` module and its `fan_curve_editor`. `638f684` refactors
the editor's methods, and `e5291bc` refactors the tuning page's curve handling while cutting its
keyboard section to four modes whose only controls are `SingleColor`'s (+36/−86) — the state the
page keeps until PR #42.

Files: `gui/src/widgets/{fan_curve_editor,mod}.rs`, `gui/src/pages/tuning.rs`, `gui/Cargo.toml`

### 2026-01-08 — Async D-Bus client with a command enum

SHAs: `5c7423f` `2a03b02` `f5f5950` `6732403`

`5c7423f` rewrites `gui/src/dbus_client.rs` around async calls (+120/−382) and `2a03b02` adds a
`DbusCommand` enum whose variants carry `oneshot::Sender` replies, with `set_battery_settings`,
`get_battery_available_start_thresholds` and `get_battery_available_end_thresholds` among the request
methods. `f5f5950` adds pending-reply checks before a request is sent, and `6732403` removes
`dbus_client: Option<DbusClient>` from `AppState`, leaving the client outside the state and the
pending receivers inside it.

Files: `gui/src/dbus_client.rs`, `gui/src/app.rs`

### 2026-01-08 — Keyboard shortcuts and a help window

SHAs: `1b0bb1d` `8cd4b4c` `4a4fe2f` `464ab11` `1a8531e`

`1b0bb1d` adds `KeyboardShortcuts` with `handle_shortcuts`, which maps `Ctrl+S` to
`state.save_config()` and opens a help window; `8cd4b4c` and `4a4fe2f` add the field to the app and
call it from the update method. `464ab11` declares `mod keyboard_shortcuts;` and removes the 387-line
duplicate of the app structures from `main.rs` (415 lines to 29), and `1a8531e` trims the module.

Files: `gui/src/keyboard_shortcuts.rs`, `gui/src/app.rs`, `gui/src/main.rs`

### 2026-01-08 — System tray module (PR #17)

SHAs: `af26127` `6b69096`

PR #17 (merge `af26127`) is a four-file net diff — 1 added, 3 modified, +141/−11 — whose payload is
`gui/src/system_tray.rs` (121 lines: `SystemTray`, `TrayEvent`, `load_tray_icon`) and
`tray-icon = "0.18"` in `gui/Cargo.toml`. `gui/src/main.rs` at `c8189d3` does not declare
`mod system_tray` — its module list is `mod app; mod dbus_client; mod theme; mod pages; mod
keyboard_shortcuts; mod widgets;` — so the module is not compiled at that tree. The PR's title also
names WiFi and storage, which are not in its net diff: those rows arrive in the direct commits of the
entry above.

Files: `gui/src/system_tray.rs`, `gui/src/app.rs`, `gui/src/pages/tuning.rs`, `gui/Cargo.toml`

### 2026-01-08 — Battery charge thresholds as a setting (PR #18)

SHAs: `c8189d3` `180105d` `8f74f89` `0aa2fd5` `b2d9daa` `eca1941`

PR #18 (merge `c8189d3`). `BatterySettings` enters `common/src/types.rs` with `charge_start_threshold`
(default 40) and `charge_end_threshold` (default 80), and `set_battery_settings` — first in `eca1941` —
applies `set_charge_type("Custom")` plus both thresholds through `BatteryControl`, returning to
"Standard" when the setting is off. `gui/src/dbus_client.rs` gains `set_battery_settings`,
`get_battery_available_start_thresholds` and `get_battery_available_end_thresholds` over oneshot
channels, and `gui/src/pages/settings.rs` gains the threshold controls. The branch also carries
`0aa2fd5` (binary renamed to `tuxedo-control-center`), `8f74f89` (the workflow's binary name) and
`b2d9daa` (a Tokio runtime entered in `main`).

Files: `common/src/types.rs`, `daemon/src/{dbus_interface,hardware_control}.rs`,
`gui/src/{dbus_client,app,main}.rs`, `gui/src/pages/{settings,tuning,profiles}.rs`, `gui/Cargo.toml`

### Unit 2b accounting

62 commits = 43 named in the 9 entries above + 19 excluded:

- 4 commits that restore the pre-PR-#15 content of files under `gui/src/ui/` — `a408870` (+105/−97),
  `a371d2b` (+236/−111), `61330a8` (+1/−1) and `1bffbea` (+121/−1). `a408870`'s diff is patch-identical
  to the reverse of PR #15's `statistics_page.rs` change (`git patch-id` 525a143bbb7b on both) and
  `a371d2b`'s to the reverse of its `tuning_page.rs` change (3e6eaef70993); `1bffbea` is
  patch-identical to the unit 2a commit that first added the battery group (`git patch-id`
  674de0dce8ed) and restores the pre-PR-#15 content of `gui/src/ui/settings_page.rs`, while `61330a8`
  restores the client argument in that page's `create_page` (+1/−1). At `1bffbea` four of the five
  files PR #15 edits under `gui/src/ui/` are identical to `9cbecab^1`, and `fan_curve_editor.rs`
  keeps the PR's version until the directory is deleted in the next entry.
- 4 CI workflow edits — `221ab9c` (+1/−1), `0c503f0` (+1/−1), `50b5b0f` (+2/−2), `fb0c4f9` (+1/−1).
- 3 import-only edits — `4713f35` (+2/−0) `oneshot`, `b813a20` (+1/−0) `Color32`, `a65adff` (+1/−0)
  `Profile`.
- 1 dependency with no caller — `b26946a` (+1/−0) adds `rfd = "0.15"` to `gui/Cargo.toml`, and
  `git grep rfd c8189d3 -- gui/src` returns nothing.
- 7 repository cleanups that delete stray and packaging files — `582520e` (+0/−115, a Russian-named
  markdown file of GTK build instructions), `73ba58d` (+0/−321, `setup-debian-packaging.sh`), `c293805`
  (+0/−84, `build-deb.sh`), `4cc5beb` (+0/−13, `tuxedo-daemon.conf`), `d127175` (+0/−12,
  `tuxedo-daemo.service`), `e2b4633` (+0/−5, `com.tuxedo.Control.service`) and `5c3288c` (+0/−0, the
  binary `tcc_claude.zip`).

Three pull requests are merged in this half — #16, #17 and #18 — and all three carry entries. Unit 2
closes at 86 + 62 = 148 commits, the count the inventory gives for 2026-01-01..2026-01-08.

## 2026-01-09 → 2026-01-13 — Unit 3a (48 commits, first half of unit 3)

Unit 3 begins at the commit after the cursor and ends at the last first-parent commit before PR #51's
merge; this half stops at the merge of PR #25, and the second half carries the remaining 48 commits.
The half holds the 31 direct commits of 2026-01-09 and the four pulls merged up to 2026-01-13; 3b
holds the boundary commit's 2026-01-19 author date.

### 2026-01-09 — Font size setting and per-section polling sliders

SHAs: `b27f9ca` `abff151` `09208be`

`b27f9ca` puts a `FontSize` enum — Small, Medium, Large — into `AppConfig`, its first appearance in
the tree; `09208be` adds `TuxedoTheme::apply_with_font_size`, which derives the five text styles from
the variant (heading 18/22/26, body 12/14/16); and `abff151` gives the settings page a three-way
selector that writes the config and resizes the text at once, drops the page's Save button, and adds
a slider for each statistics section's polling rate — 0.5 s steps, WiFi and storage added, the
system-info slider dropped. Those sliders write `*_poll_rate` fields that nothing reads at
1532df6: the poll loop in `gui/src/app.rs` keeps hardcoded intervals.

Files: `common/src/types.rs`, `gui/src/theme.rs`, `gui/src/pages/settings.rs`

### 2026-01-09 — WiFi rows and storage temperature read in the GUI process

SHAs: `0090b90`

`read_wifi_info` walks `/sys/class/net`, keeps the interfaces that carry a `wireless` node, and takes
the driver from the module symlink and the temperature from `device/hwmon/temp1_input`; the signal
level comes from `/proc/net/wireless`, the channel, width and rates from `iw dev <if> info` and
`iw dev <if> link`. `read_storage_info` fills the temperature it had left `None`, and `AppState`
loses `config_dirty` and `PollTimers`. The poll loop becomes six independent intervals under
`tokio::select!`; PR #20 below returns it to a single joint poll.

Files: `gui/src/app.rs`

### 2026-01-09 — Storage usage rows in the statistics page

SHAs: `0bb7c5b` `ffc490f` `71fd960`

`0bb7c5b` gives each storage device a usage row: `get_free_space` resolves the device's mount point
with `findmnt -n -o TARGET --source <dev>` and reads `libc::statvfs`, the first `statvfs` use in the
tree, and the page draws a progress bar of used of total beside the free and total figures. The same
commit opens the system, GPU and storage sections by default. `ffc490f` and `71fd960` rewrite the
mount lookup twice on the same day, the second through `systemstat`'s mount list. PR #20 below
removes `get_free_space` from the GUI and computes usage in the daemon.

Files: `gui/src/pages/statistics.rs`

### 2026-01-09 — Draggable points in the fan curve editor

SHAs: `07004cd` `257db20`

`07004cd` makes the plot interactive: a drag starting within five units of a point moves it, clamped
to 0–100 on both axes and written into `curve.points` as it moves; a click within eight units selects
the nearest point; and a point added with "Add point" is selected on creation. The axes are pinned to
0–100 with zoom, drag and boxed zoom disabled. `257db20` removes the "Remove Selected" button and
returns the per-row delete button to every row while more than two points remain.

Files: `gui/src/widgets/fan_curve_editor.rs`

### 2026-01-09 — Tuning page rebuilt around the selected profile

SHAs: `223b57b` `504e9da`

`223b57b` rebuilds the page: a `TopBottomPanel` header names the profile being edited and carries Save
(writes the config, then applies that profile over D-Bus) and Reset to Default; the CPU section gains
an AMD P-State mode combo — `active`, `passive`, `guided` — above the governor, energy-performance
preference, boost and SMT controls and the frequency sliders; keyboard, screen and fan sections
follow — the keyboard section listing the four modes and offering the colour-channel and brightness
controls of `SingleColor` alone — with the fan section instantiating one `FanCurveEditor` per fan out
of `fan_info.len().max(2)`.
`504e9da` takes the governor's default from `available_governors` and clamps the min and max sliders
against each other.

Files: `gui/src/pages/tuning.rs`

### 2026-01-09 — Profiles page: radio selection, Standard reset, save on switch

SHAs: `12f4e46`

Radio buttons replace the ●/○ labels and selecting one switches the profile. The Standard profile
gains a "Reset to Default" button that rebuilds it from `create_standard_profile` (governor
`schedutil`, EPP `balance_performance`, AMD P-State `active`), deleting the current profile switches
to Standard first, a new profile is a copy of the current one instead of the default, and switching,
resetting, deleting and creating all write the config at once where the page previously set
`config_dirty`.

Files: `gui/src/pages/profiles.rs`

### 2026-01-09 — GPU frequency, temperature, load, power and voltage in the daemon

SHAs: `d83b33d` `66edd60`

`get_gpu_info` had pushed all five metrics as `None`; `d83b33d` reads them from the card's `device`
directory — frequency from `pp_dpm_sclk` (AMD) or `gt_cur_freq_mhz` (Intel), temperature from
`hwmon/*/temp1_input`, load from `gpu_busy_percent`, power from `hwmon/power1_average` or
`power1_input`, voltage from `hwmon/in0_input` — and names each card after its vendor id. Two of the
readers take a second path the metric does not justify: `read_gpu_temperature` falls back to
`/gpu_busy_percent` and returns that number as a temperature (`hardware_detection.rs:913`), and
`read_gpu_load`'s Intel branch opens `gt_RP0_freq_mhz` and falls through to `None` (`:931`). It also
adds `get_scheduler_info`, which reports CFS or EEVDF according to whether the `sched_features` file
mentions `EEVDF`, and `66edd60` carries `scheduler` and `available_schedulers` in `CpuInfo`.

Files: `daemon/src/hardware_detection.rs`, `common/src/types.rs`

### 2026-01-10 — Hardware reads move from the GUI into the daemon (PR #20)

SHAs: `55531b6` `527a11d` `b90d3a6`

PR #20 (merge `55531b6`, 12 files, +406/−451) is an architecture change: the GUI process stops
reading sysfs. `gui/src/app.rs` loses `read_battery_info`, `read_wifi_info` with its three helpers,
`read_storage_info` and the `read_sysfs_*` helpers (+54/−283 for the file), and the daemon gains
`get_battery_info`, `get_storage_device_info` and `get_mount_info` beside the existing
`get_wifi_info`; the three new readers are serialised to JSON and exposed as new D-Bus methods, and
`gui/src/dbus_client.rs` gains a command for each of the four. `StorageInfo` becomes
`StorageDevice` plus a separate `MountInfo` carrying mount point, filesystem type, total, used and
percent, so usage is computed in the daemon from `systemstat`'s mount list. The poll loop collapses
from per-section intervals to one 1 s `tokio::join!` of all seven requests, repaint becomes
`request_repaint_after(500 ms)` instead of continuous, and the fan daemon holds one `Arc<TuxedoIo>`
and caches the sorted curves instead of opening the io device on every pass.

Files: `Cargo.toml`, `common/src/types.rs`,
`daemon/src/{main,dbus_interface,hardware_detection,fan_daemon}.rs`, `gui/src/{app,dbus_client}.rs`,
`gui/src/pages/{statistics,settings}.rs`

### 2026-01-12 — NVIDIA locked clocks over NVML (PR #23)

SHAs: `1d7da7f` `945711f`

PR #23 (merge `1d7da7f`, 13 files, +697/−105). NVML arrives through `nvml_wrapper = "0.11"`: a
`Lazy` `Nvml::init()` in `hardware_control.rs` and `set_gpu_locked_clocks`, which calls
`device.set_gpu_locked_clocks(GpuLockedClocksSetting::Numeric { … })`, with
`set_memory_locked_clocks`, the two reset calls and two clock-range queries behind D-Bus methods, and
a GPU Tuning section of min/max sliders. `MemoryInfo` and a Memory (RAM) statistics section,
`BatteryInfo.status` and the `GpuSettings` clock fields land in the same PR, and the service file
becomes a systemd unit with `BusName=com.tuxedo.Control`, replacing the `[D-BUS Service]`
registration. Its title's "overclocking" is not in the diff: it sets locked clock ranges, and offsets
arrive in the next entry. Hardware behaviour is not established by the diff.

Files: `common/src/types.rs`,
`daemon/src/{hardware_control,hardware_detection,dbus_interface}.rs`, `daemon/tuxedo-daemon.service`,
`gui/src/{app,dbus_client}.rs`, `gui/src/pages/{statistics,tuning,profiles}.rs`, `gui/Cargo.toml`

### 2026-01-12 — GPU clock offsets and Prime profile selection (PR #24)

SHAs: `d549b7a` `2245b71` `e62dc72` `067ec54` `c4548c8`

PR #24 (merge `d549b7a`, 8 files, +319/−26). `GpuSettings` gains `core_offset`, `memory_offset` and
`prime_profile`; `set_gpu_core_offset` and `set_gpu_memory_offset` call
`device.set_clock_offset(Clock::Graphics|Clock::Memory, PerformanceState::Zero, offset)`, four D-Bus
methods query the limits and apply them, and `set_prime_profile` checks the name against `on-demand`,
`nvidia` and `intel` and runs `prime-select <profile>`. The branch first writes a hand-rolled
`libloading` binding in `daemon/src/nvml.rs` — `nvmlDeviceSetClockOffsets` into an `nvmlClockOffset`
struct, with a second file under `daemon/srcs/` — and `e62dc72` deletes both in favour of
`nvml_wrapper`. `prime-select` appears first in `2245b71`. Hardware behaviour is not established by
the diff.

Files: `common/src/types.rs`,
`daemon/src/{hardware_control,hardware_detection,dbus_interface}.rs`, `gui/src/{app,dbus_client}.rs`,
`gui/src/pages/{tuning,profiles}.rs`

### 2026-01-13 — iGPU detection, memory frequency and the Prime profile in Settings (PR #25)

SHAs: `1532df6` `584f7e0` `7541fbd` `7969c69` `bac5293` `fdfe3b8`

PR #25 (merge `1532df6`, 6 files, +302/−68). `get_gpu_info` no longer returns NVML's NVIDIA list
alone: the `/sys/class/drm` iGPUs are listed too, named from their device id by `get_amd_gpu_name`
(`daemon/src/hardware_detection.rs:909`, eight arms from Vega 8 to 760M) and `get_intel_gpu_name`
(`:927`, seven arms from UHD Graphics 620 to Arc Graphics (DG2)); NVIDIA's status is
`device.performance_state()` (`:1008`), `device.power_state()` (`:1012`), `"Active"` (`:1014`).
`GpuInfo.memory_frequency` is added, from NVML `Clock::Memory` or `pp_dpm_mclk`. The Prime selector
moves to Settings: Apply writes `gpu_settings.prime_profile` into every profile
(`gui/src/pages/settings.rs:407-409`), saves and calls `set_prime_profile` over D-Bus (`:414`), and the
restart confirmation's "Yes" spawns `systemctl reboot` from the GUI process itself,
`std::process::Command::new("systemctl").args(["reboot"]).spawn()` (`:439`). Hardware behaviour is not
established by the diff.

Files: `common/src/types.rs`, `daemon/src/hardware_detection.rs`, `gui/src/app.rs`,
`gui/src/pages/{settings,statistics,tuning}.rs`

### Unit 3a accounting

48 commits = 30 named in the 11 entries above + 18 excluded:

- 3 commits withdrawing the automatic-profile-switching setting — `d862851` (+0/−7) removes the field
  from `AppConfig`, `940b3ca` (+0/−4) its controls in the profiles page and `69d81cc` (+0/−4) the
  remaining ones in the tuning page.
- 1 commit hardening the existing CPU and backlight writes — `e219ac7` (+63/−19): it orders the min
  and max frequency writes by the current values, adds the `amd_pstate/cpb_boost` boost path and
  `amdgpu_bl1` to the backlight list, tries `actual_brightness` before `brightness`, and returns `Err`
  where it previously returned `Ok` with no writable backlight.
- 1 configuration-loading refactor — `be78b40` (+4/−13) reduces `load_config` to the disk read alone,
  dropping the "Default"→"Standard" rename that the new defaults make unnecessary.
- 1 window-size tweak — `adc3310` (+2/−2).
- 1 agent-instruction file — `b4a8070` (+44/−0) adds `.github/agents/my-agent.agent.md`.
- 11 manifest edits — `a48c286` (+1/−0), `012ab63` (+2/−0), `ab75063` (+0/−2), `3b14869` (+3/−1),
  `cfec290` (+2/−0), `15cc7fc` (+1/−1), `a9fa9c0` (+1/−1), `edeb11b` (+0/−2), `a6197a3` (+4/−4),
  `e49c6f3` (+1/−1), `11adc1d` (+1/−1). `012ab63` and `cfec290` add `ashpd` and `zvariant` to
  `daemon/Cargo.toml` and `ab75063` is the exact reverse of that addition; `3b14869` adds both to
  `gui/Cargo.toml` and `a6197a3` removes them again. The only dependency of this group in the tree at
  the merge of PR #20 is `systemstat`, which `a6197a3` adds to `gui/Cargo.toml`.

## 2026-01-14 → 2026-01-19 — Unit 3b (48 commits, second half of unit 3)

This half begins at the commit after PR #25's merge and ends at `05a0918`, the last first-parent
commit before PR #51; it carries five pulls (#37, #40, #42, #43, #47), three direct edits of the
WiFi paths, one CI workflow edit and the `drivers_src/` upload.

### 2026-01-14 — Polling scheduler in the daemon, refresh coordinator in the GUI (PR #37)

SHAs: `d5c5a6d` `df1edd5` `581a978` `de376ef` `9111140` `de8e544` `fd4bada`

PR #37 (merge `d5c5a6d`, 7 files, +608/−104). `daemon/src/polling_scheduler.rs` adds `PollJob`
(id, `next_run`, interval, `poll_fn`) in a `BinaryHeap` ordered by `next_run`, run by
`PollingScheduler::run` and driven through `SchedulerHandle::{add_job, update_interval}`; the daemon
keeps one in a `SCHEDULER_HANDLE` `OnceCell`, runs its fan loop as a job, and exposes
`update_polling_interval(component, interval_ms)` over D-Bus. `gui/src/polling_scheduler.rs` adds a
`RefreshCoordinator` with `register`, `update_interval`, `unregister` and `shutdown`. The
`*_poll_rate` fields that nothing read before this are read from here: `gui/src/app.rs:251-258`
registers cpu, gpu, memory, fans, battery, wifi, storage and mount with intervals taken from
`statistics_sections` (`memory` from `cpu_poll_rate`, `mount` from `storage_poll_rate`), and
`fd4bada` makes each job's first refresh immediate.

Files: `daemon/src/{main,polling_scheduler,dbus_interface}.rs`,
`gui/src/{app,main,polling_scheduler}.rs`, `gui/src/pages/settings.rs`

### 2026-01-15 — Status-notifier tray, runtime-status gating, storage speeds, daemon shutdown (PR #40)

SHAs: `7699009` `c31c6aa` `b0c88ad` `e6b02d9` `ea25e94` `9d01051` `40fc45b` `2c7b212`
`807aab0` `ba0b064` `01a884d` `f5f2c6c` `e702184` `06997c5` `35074b0`

PR #40 (merge `7699009`, 14 files, +5585/−157, of which `Cargo.lock` is +4918/−0, the
`.gitignore` entry dropped). `tray-icon = "0.18"` becomes
`ksni = { version = "0.3.3", features = ["blocking"] }`: `SystemTray` wraps a
`ksni::blocking::Handle<TrayState>`, `TrayState` implements `ksni::Tray` with a profile menu, and
the app dispatches `TrayEvent::{ShowWindow, ShowStatistics, SwitchProfile, Quit}`.
`mod system_tray` is declared in `gui/src/main.rs` first in `40fc45b`; `c8189d3` did not declare
it. The daemon gains `shutdown_daemon`, raising `SIGINT` on itself after 200 ms unless
`INVOCATION_ID` or `SYSTEMD_EXEC_PID` marks a systemd-managed process. `get_gpu_info` reads
`/sys/bus/pci/devices/<id>/power/runtime_status` and skips the NVML metrics for a suspended card;
`StorageDevice` gains `read_speed`/`write_speed` from `/sys/block/<dev>/stat` sector deltas and
`hw_sector_size`, and `WiFiInfo` gains `ssid` and `data_rate`.

Files: `common/src/types.rs`, `daemon/src/{hardware_detection,dbus_interface}.rs`,
`gui/src/{app,system_tray}.rs`, `gui/src/pages/{statistics,tuning}.rs`, `gui/Cargo.toml`, `Cargo.lock`

### 2026-01-16 — Settings split into tabs, configuration split into two files (PR #42)

SHAs: `7143c95` `4a71320` `70b2322` `fd0f4dc` `d802693` `838a59d` `b7fec24` `9aaf7bc`
`995ac5b` `08341b0` `8d77531` `d662106` `919b552`

PR #42 (merge `7143c95`, 15 files, +1507/−417) restructures the settings surface and its
storage. The settings page becomes three tabs — `SettingsTab::{Main, StatsConfiguration, Hardware}`
— and it draws System, CPU, Memory, GPU and Storage sections from the state the
daemon reports. The configuration is split into `settings.json` and `profiles.json`, each
with a `from`/`apply_to` pair over `AppConfig` (`SettingsConfig`, `ProfilesConfig`) and its own
`save_settings`/`save_profiles`. The statistics page draws its sections in
`statistics_sections.section_order`, normalised against the eight `STATISTICS_SECTIONS` entries,
and the polling controls gain a Memory slider. The four effect modes that `e5291bc` removed — `Dance`,
`Flash`, `RandomColor`, `Tempo` — return to the keyboard combo, whose `Breathe` and `Flash` arms regain
their colour channels and whose effect arms regain the `Speed:` slider, with
`write_effect_mode`/`write_effect_speed` in `hardware_control.rs`; `channel_from_frequency_mhz` (first
in `b7fec24`) derives channel numbers from the frequency.

Files: `common/src/types.rs`, `daemon/src/{hardware_control,hardware_detection}.rs`,
`gui/src/{app,dbus_client}.rs`, `gui/src/pages/{settings,statistics,tuning,profiles}.rs`,
`screenshots/`

### 2026-01-16 — SKU, board and kernel modules in the hardware tab (PR #43)

SHAs: `b2627dc` `8021990` `3c9127d`

PR #43 (merge `b2627dc`, 3 files, +125/−251). `SystemInfo` gains `product_sku`, `board_name` and
`tuxedo_kernel_modules`, the last filled by `get_tuxedo_kernel_modules`, and the hardware tab shows
SKU, Board and TUXEDO Kernel Modules rows. The daemon's WiFi reading is consolidated into
`get_wifi_details` (one `iw dev <if> link` and one `iw dev <if> info` pass, the channel falling back
to `channel_from_frequency_mhz`, SSID to `iwgetid -r`) beside `read_wifi_temperature` and
`read_wifi_signal`. Its title's "Display WiFi Stats" is not in the net diff: no statistics file is
touched, and `draw_wifi_info` first appears in `ca95165` (2026-01-07), whose rows PR #42 extends.

Files: `common/src/types.rs`, `daemon/src/hardware_detection.rs`, `gui/src/pages/settings.rs`

### 2026-01-17 — WiFi detection through resolved binaries, PCI ids and ethtool (PR #47)

SHAs: `6e85f27` `7c61c33` `5521bcb` `42d5af0` `bc79df2`

PR #47 (merge `6e85f27`, 4 files, +349/−197). `find_binary` resolves `iw`, `iwconfig`, `iwgetid`,
`lspci` and `ethtool` from six fixed directories before falling back to the bare name;
`get_pci_info` turns `/sys/class/net/<if>/device` into a PCI address and runs `lspci -s <addr> -k`
for the "Network controller" and "Subsystem" lines, and the driver and firmware versions come from
`ethtool -i`. Channel numbers come from the `iw dev <if> info` line alone, which leaves
`channel_from_frequency_mhz` without a caller. The statistics page gains network controller,
subsystem, driver version, firmware, PHY rate, actual throughput and adapter temperature rows, and
the file carries three unit tests (`test_parse_wifi_rate`, `test_normalize_ssid`,
`test_ethtool_parsing`).

Files: `common/src/types.rs`, `daemon/src/hardware_detection.rs`,
`gui/src/pages/{statistics,settings}.rs`

### 2026-01-19 — Vendored `drivers_src/` kernel-module tree

SHAs: `05a0918`

`05a0918` ("Add files via upload") adds `drivers_src/` — 55 files, 12 module directories and 12
top-level files around a `Kbuild`, no `Makefile` and no `dkms.conf` — a copy of TUXEDO Computers'
`tuxedo-drivers` kernel modules, GPL-2.0-or-later, identified in the 2026-10-25 audit as release
v4.18.2 with 51 of the 55 files byte-identical to it and local edits in the other four. Nothing in
the tree builds, packages, installs or loads it: there is no `build.rs` and no `cc`/`bindgen`
dependency, and CI, Debian and Arch packaging never reference the path; at 2026-10-25 only fifteen
documentation lines in four files do. PR #140 (merged 2026-10-25) deletes all 55 files (−14527)
and re-points five documents.

Files: `drivers_src/`

### Unit 3b accounting

48 commits = 44 named in the 6 entries above + 4 excluded:

- 1 CI workflow edit — `8a1388a` (+3/−4) drops the `-v` flags from the workflow's build and test
  commands and one development package.
- 2 refinements of the WiFi rows between PR #43 and PR #47 — `641a9f8` (+75/−44) styles the SSID cell
  and reorders the signal rows; `a6ac5b6` (+53/−5) rewrites the `iw` parsing as explicit branches with
  debug logging and adds a second copy of `normalize_ssid`. Both sit inside the display that PR #47
  replaces.
- 1 duplicate removal — `814ca58` (+0/−16) deletes one of the two `normalize_ssid` definitions in
  `daemon/src/hardware_detection.rs`, keeping the copy at line 1562.

## 2026-01-19 → 2026-01-26 — Unit 4 (51 commits)

Unit 4 runs from the commit after `05a0918`, where unit 3b ends, to the merge of PR #62 (`1d91b17`).
Its ten first-parent merges are #51, #52, #53, #55, #56, #57, #58, #60, #61 and #62; six direct
commits sit between the merges of #52 and #53. Its first commit is dated 2026-01-19 and its last
2026-01-26.

### 2026-01-22 — Keyboard types, zones and Clevo per-zone writes (PR #51)

SHAs: `9bb6517` `c9f2777` `9b76559` `78d9179`

PR #51 (merge `9bb6517`, 9 files, +630/−317). `KeyboardType` — `None`, `WhiteOnly`, `SingleZoneRGB`,
`ThreeZoneRGB`, `FourZoneRGB`, `PerKeyRGB` (`common/src/types.rs:207-214`) — and
`KeyboardCapabilities` (`:217-223`) are filled by `get_keyboard_capabilities`, which counts
`left`/`center`/`right:kbd_backlight` nodes under the `tuxedo_keyboard` LEDs, falls back to the
`kbd_backlight` names in `/sys/class/leds`, and calls more than ten of them per-key
(`daemon/src/hardware_detection.rs:836`, `:927`). `KeyboardMode::MultipleZones { zones, brightness }`
carries a `Vec<ZoneColor>` (`common/src/types.rs:226`, `:235`); the tuning page offers it when
`num_zones > 1` (`gui/src/pages/tuning.rs:555`) with one RGB row per zone (`:639-657`), and
`set_zone_color` sends the Clevo request `0x67` with the prefix `0xF0`/`0xF1`/`0xF2`
(`daemon/src/tuxedo_io.rs:613`, `:618`) before writing the zone's `multi_intensity`
(`daemon/src/hardware_control.rs:529`). Hardware support is not established by
the diff.

Files: `common/src/types.rs`, `daemon/src/{hardware_control,hardware_detection,dbus_interface,tuxedo_io}.rs`,
`gui/src/{app,dbus_client}.rs`, `gui/src/pages/{settings,tuning}.rs`

### 2026-01-23 — Rename to LapSphere and a daemon-launched GUI (PR #52)

SHAs: `4c211f2` `a6d64c5` `1a7d9a7` `420f0fa` `cb9be5c` `30c5868` `37d63d8` `68664ed` `786bb55`

PR #52 (merge `4c211f2`, 44 files, +1093/−754) renames the project: the crates become
`lapsphere-common`, `lapsphere-daemon` and `lapsphere` (`common/Cargo.toml:2`, `daemon/Cargo.toml:2`,
`gui/Cargo.toml:2`), the D-Bus interface and bus name `com.tuxedo.Control` become
`io.lapsphere.Control`, served at a new object path `/io/lapsphere/Control`
(`daemon/src/dbus_interface.rs:11`, `:414-415`), the config directory becomes `~/.config/lapsphere`
(`gui/src/app.rs:723`), and the desktop entries, Debian package and icon take the name
(`data/io.lapsphere.LapSphere.desktop`, `debian/lapsphere.desktop`, `lapsphere.svg`).
`daemon/tuxedo-daemon.service` and the old activation files are deleted rather than renamed. Not
renamed: `daemon/src/tuxedo_io.rs` with `TuxedoIo` and `/dev/tuxedo_io`, the two
`com.tuxedo.Control.conf` filenames, `drivers_src/`. The daemon gains `--gui`, spawning the GUI as
`SUDO_UID`/`PKEXEC_UID` and raising `SIGINT` when it exits (`daemon/src/main.rs:134-197`). It also
adds `get_cpu_topology` (`lscpu`), `read_intel_pstate_status` with `has_intel_pstate`, and a drawn
tray icon (`daemon/src/hardware_detection.rs:161`, `:483`, `:552`, `gui/src/system_tray.rs:187`).

Files: `Cargo.lock`, `.github/workflows/rust.yml`, `common/{Cargo.toml,src/types.rs}`,
`daemon/{Cargo.toml,src/{main,dbus_interface,hardware_control,hardware_detection,fan_daemon}}.rs`,
`gui/{Cargo.toml,src/{app,dbus_client,main,system_tray,theme}.rs}`,
`gui/src/pages/{profiles,settings,statistics,tuning}.rs`, `data/`, `debian/`

### 2026-01-23 — Daemon CLI help, `--tray` and a console statistics mode (PR #53)

SHAs: `a39b4f4` `6fca9bf` `fda00a1`

PR #53 (merge `a39b4f4`, 10 files, +194/−62). `lapsphere-daemon` gains an argument surface:
`--help`/`-h` prints the usage, the option list and the two configuration paths, then returns before
any hardware access (`daemon/src/main.rs:31-45`). A run without `--gui` or `--tray` prints CPU name,
load, package temperature and power, memory use and one row per GPU to stdout before the bus service
starts (`:82-113`); without root the same block prints a `Limited` variant and exits 1 (`:58-79`).
`--tray` suppresses those prints and is forwarded when `--gui` also launches the GUI.
`data/io.lapsphere.Control.service` adds D-Bus activation (`Name=io.lapsphere.Control`, `User=root`),
installed by `debian/rules:18`.

Files: `daemon/src/{main,dbus_interface,hardware_detection}.rs`,
`data/{io.lapsphere.Control.conf,io.lapsphere.Control.service,io.lapsphere.LapSphere.desktop}`,
`debian/{io.lapsphere.Control.service,lapsphere.desktop,rules}`, `gui/src/app.rs`

### 2026-01-23 — Memory module speed and a broader storage temperature lookup (PR #55)

SHAs: `c1d547f` `9faca69`

PR #55 (merge `c1d547f`, 4 files, +85/−47). `MemoryInfo` gains `memory_frequency`
(`common/src/types.rs:82`), read from the first `Speed:` or `Configured Memory Speed:` line of
`dmidecode -t memory`, ignoring an `Unknown` value (`daemon/src/hardware_detection.rs:868`, `:883`).
Storage temperature reads through `find_storage_temperature`, which tries `temp1_input`–`temp3_input`
under `device/hwmon`, then `device/device/hwmon`, then every `/sys/class/hwmon` entry whose `device`
link matches the block device (`:1989`, `:2027`).

Files: `common/src/types.rs`, `daemon/src/hardware_detection.rs`,
`gui/src/pages/{settings,tuning}.rs`

### 2026-01-24 — GPU tuning in Standard and Advanced modes (PR #57)

SHAs: `6bce901` `86e0ecb` `f4dcba5` `e2028a7` `a12c6ec`

PR #57 (merge `6bce901`, 11 files, +493/−228) splits GPU tuning into Standard and Advanced modes
(`gui/src/pages/tuning.rs:397-398`). Standard bounds the locked minimum and maximum GPU and memory
clocks and the core and memory offsets by ranges queried from the daemon (`:475-550`); Advanced edits
the new `GpuSettings::advanced` table of ten min/max pairs — temperature, power limit, frequency,
frequency offset, low and high frequency with their drain offsets, critical temperature, power offset
(`common/src/types.rs:210`, `gui/src/pages/tuning.rs:552`, `:632`). Fan curves move to a per-fan
selector (`gui/src/app.rs:62`, `gui/src/pages/tuning.rs:112`), and a top bar carries the clock, date
and current profile (`gui/src/app.rs`).

Files: `common/src/types.rs`, `gui/src/{app,main,theme,keyboard_shortcuts}.rs`,
`gui/src/pages/{settings,statistics,tuning,profiles}.rs`, `gui/src/widgets/fan_curve_editor.rs`,
`screenshots/ui-update.png`

### 2026-01-25 — One-second NVIDIA offset loop with smart rounding (PR #58)

SHAs: `6687160` `98d25ec` `f01a87b` `10f48c5` `fc6b8f6` `e388342` `b29d0f2`

PR #58 (merge `6687160`, 12 files, +568/−33). The daemon registers a `gpu_overclock` `PollJob` at
1000 ms (`daemon/src/main.rs:229`) whose `apply_gpu_overclocking` derives one offset from the card's
frequency, temperature and power and writes it with `set_gpu_core_offset(0, offset)` →
`set_clock_offset(Clock::Graphics, PerformanceState::Zero, offset)`, only while `advanced_control`
and `manual_clocks` are set (`:353-364`), the card is not `suspended` (`:373`), the performance state
is `Zero` (`:455`) and the value differs from `LAST_APPLIED_OFFSET` (`:477-479`). Voltage comes from
`nvidia-smi -q -d VOLTAGE` or `--query-gpu=voltage.graphics` once a D-Bus `set_nvidia_smi_legacy_path`
names the binary (`daemon/src/hardware_detection.rs:1253`, `:1259`). Its "dynamic overclocking" is this
periodic re-application, not a changing clock request. Hardware support is not established by the
diff.

Files: `common/src/types.rs`, `daemon/src/{main,dbus_interface,hardware_detection}.rs`,
`gui/src/{app,dbus_client,keyboard_shortcuts}.rs`, `gui/src/pages/{settings,statistics,tuning,profiles}.rs`,
`gui/src/widgets/fan_curve_editor.rs`

### 2026-01-26 — Help and About tabs, advanced clock fields, help window (PR #60)

SHAs: `e2f2af5` `6662097` `51b3c6b` `7c385ff` `8619b7f`

PR #60 (merge `e2f2af5`, 9 files, +344/−100) adds `SettingsTab::Help` and `SettingsTab::About`
(`gui/src/app.rs:29-30`), `draw_help_info` with a static shortcut grid and `draw_about_info`
(`gui/src/pages/settings.rs:38`, `:101`), and a separate egui viewport titled `Help` rendered by
`draw_help_contents` (`gui/src/keyboard_shortcuts.rs:64`, `:94`). `GpuSettings` gains
`advanced_min_gpu_clock`, `advanced_max_gpu_clock`, `advanced_min_mem_clock`, `advanced_max_mem_clock`
and `advanced_memory_offset` (`common/src/types.rs:214`), bound in the advanced pane while the
standard pane's core and memory offset sliders draw only when `advanced_control` is off
(`gui/src/pages/tuning.rs:555-571`).

Files: `common/src/types.rs`, `daemon/src/{main,hardware_detection}.rs`,
`gui/src/{app,keyboard_shortcuts}.rs`, `gui/src/pages/{settings,statistics,tuning,profiles}.rs`

### 2026-01-26 — Suspended NVIDIA cards answered from sysfs without NVML (PR #61)

SHAs: `60bbbc0` `910480c` `53a0b1d` `81afbab`

PR #61 (merge `60bbbc0`, 4 files, +254/−50). `get_nvidia_gpu_info` first lists the addresses under
`/sys/bus/pci/drivers/nvidia` and reads each card's `power/runtime_status`; when every card reports
`suspended` it builds the rows from `NVIDIA_NAMES_CACHE` with no metrics and returns without calling
NVML (`daemon/src/hardware_detection.rs:1298-1312`), so a sleeping discrete GPU stays asleep across a
poll. The cache is filled the first time a card is read awake (`:1346`). The same merge clamps each
locked-clock slider to its partner (`gui/src/pages/tuning.rs:513`, `:522`) and moves the help text
from a viewport into an in-app `NVIDIA Overclocking Help` window (`gui/src/keyboard_shortcuts.rs:75`).

Files: `daemon/src/hardware_detection.rs`, `gui/src/{keyboard_shortcuts}.rs`,
`gui/src/pages/{settings,tuning}.rs`

### 2026-01-26 — Arch Linux package, optimus-manager and a `--tray` start (PR #62)

SHAs: `1d91b17` `be2411a` `5294cf3` `1c97206`

PR #62 (merge `1d91b17`, 12 files, +110/−21) adds a `PKGBUILD` — `pkgname=lapsphere`, both
binaries, the D-Bus policy and activation file, the desktop entry plus an `/etc/xdg/autostart` copy
rewritten to `Exec=lapsphere --tray`, and `data/icon.svg` (`:2`, `:10`, `:32-33`); `debian/rules:24-25`
installs the same autostart file. `set_prime_profile` tries
`optimus-manager --switch {hybrid,integrated,nvidia} --no-confirm` before `prime-select`
(`daemon/src/hardware_control.rs:469-486`), and `--tray` opens the GUI hidden (`gui/src/main.rs:16`,
`:28`). Its follow-up `5294cf3` excludes the tarball from itself, moves `rustup` into the `builduser`
step, makes `optimus-manager` an `optdepends`, and carries the "Clevo/Uniwill laptops" wording into
`PKGBUILD`, both desktop entries, `debian/control`, the daemon banner, the About text and
`MODULE_DESCRIPTION` (`drivers_src/tuxedo_io/tuxedo_io.c:37`).

Files: `PKGBUILD`, `.github/workflows/rust.yml`, `debian/{control,rules,lapsphere.desktop}`,
`data/{icon.svg,io.lapsphere.LapSphere.desktop}`, `daemon/src/{main,hardware_control}.rs`,
`gui/src/{main}.rs`, `gui/src/pages/settings.rs`, `drivers_src/tuxedo_io/tuxedo_io.c`

### Unit 4 accounting

51 commits = 43 named in the 9 entries above + 8 excluded:

- 2 commits of PR #56, a cleanup with no new functionality — the merge `52b57ec` (+21/−663) and its
  branch commit `2486a3e` (+21/−663), which delete `FanCurveManager` and `FanDaemon` from
  `daemon/src/fan_daemon.rs`, the fan-curve D-Bus methods, `set_fan_auto`, the unused `BatteryControl`
  accessors, `get_fan_temperatures`/`get_tdp_info`, the `TuxedoIo` TDP getters,
  `channel_from_frequency_mhz` (the mapping the unit 3b PR #42 entry credits) and the matching
  `DbusClient` methods.
- 6 direct commits of packaging, wording and workflow — `544346d` (+3/−3) desktop `Exec` and comment,
  `701d3c2` (+2/−2) changelog wording, `35a7307` (+3/−3) `debian/control` maintainer, homepage and
  description, `2c5a92d` (+2/−2) `debian/copyright`, `383915a` (+1/−1) and `50eae0a` (+1/−1) workflow
  branch triggers from `egui_mod` to `lapsphere`.

Ten pull requests are merged in this band — #51, #52, #53, #55, #56, #57, #58, #60, #61 and #62 — of
which #56 carries no entry; #63 and #64 are dated 2026-01-27 and open unit 5.

## 2026-01-26 → 2026-01-30 — Unit 5a (40 commits, first half of unit 5)

Unit 5 is split at `e6b1476`, the merge of PR #72: this half holds PRs #63, #64, #69, #70, #71 and
#72 plus four direct commits that add `nvidia/`, and the second half the remaining 37 commits up to
#77. The halves are bounded by commit, not by date: the branch commits of #63 carry 2026-01-26 dates.

### 2026-01-27 — Per-key RGB, Uniwill TDP indices and a logs tab (PR #63)

SHAs: `35db664` `000a86b` `4231bf7` `83e7549` `7bb7c06` `11c1746` `053950a` `13167fa` `d95920c`

PR #63 (merge `35db664`, 16 files, +1140/−173). `KeyboardMode::PerKeyRGB { keys, brightness }`
(`common/src/types.rs:314`) becomes selectable when `get_keyboard_capabilities` counts more than
ten `kbd_backlight` LEDs (`daemon/src/hardware_detection.rs:1046`, `gui/src/pages/tuning.rs:967`);
`set_mode` walks the list through `set_zone_color`, which writes `multi_intensity` under each LED
path and also sends the Clevo `0x67` request (`daemon/src/hardware_control.rs:678`, `:624`).
Uniwill TDP reads the performance profile with request `0x14` (`daemon/src/tuxedo_io.rs:416`),
reads current, minimum and maximum with `0x18`/`0x1b`/`0x1e` plus index (`:373`, `:384`, `:395`)
and writes a target with `0x15` plus index (`:406`); the tuning page draws one slider per index
(`gui/src/pages/tuning.rs:175`). A `Logs` settings tab lists the daemon's log lines with level
filters and a copy button (`gui/src/pages/settings.rs:40`). Hardware support is not established by
the diff. The client's background worker, in place since `2a03b02`, now opens the connection inside a
retry loop and carries `GetWebcamState`, `SetWebcamState` and `GetDaemonLogs`
(`gui/src/dbus_client.rs:302`, `:304`).

Files: `common/src/types.rs`, `Cargo.lock`, `daemon/Cargo.toml`,
`daemon/src/{main,dbus_interface,hardware_control,hardware_detection,tuxedo_io}.rs`,
`gui/{Cargo.toml,src/{app,dbus_client,keyboard_shortcuts}.rs}`,
`gui/src/pages/{profiles,settings,statistics,tuning}.rs`, `gui/src/widgets/fan_curve_editor.rs`

### 2026-01-27 — Single instance over the session bus, panic reports on disk (PR #64)

SHAs: `9509c52` `436d689` `73b0b5d` `ed6d5e4` `256471d`

PR #64 (merge `9509c52`, 12 files, +295/−692) claims the session-bus name `io.lapsphere.Gui` with
`DoNotQueue`, exits when another process owns it and installs a panic hook writing
`~/.config/lapsphere/crashes/crash_<timestamp>.log`; `get_config_dir`/`get_crash_dir` centralise
the paths and the settings save rewrites the autostart desktop file (`gui/src/main.rs:59`, `:72-74`,
`:15`; `gui/src/app.rs:877`, `:887`, `:952`). `73b0b5d` names the Clevo effect words — 0x1002a000
breathe, 0xB0000000 wave, 0x33010000 cycle, 0x80000000 dance, 0xA0000000 flash, 0x70000000 random,
0x90000000 tempo (`daemon/src/hardware_control.rs:705`, `:719`, `:730`, `:741`, `:752`, `:766`,
`:777`; defined at `drivers_src/clevo_keyboard.h:126-132` since `05a0918`), packs colour as 0xBBRRGG
(`daemon/src/tuxedo_io.rs:606`) and adds the `0x67` ioctl with its `CLEVO_CMD_SET_KB_RGB_LEDS` case
(`drivers_src/tuxedo_io/tuxedo_io_ioctl.h:67`, `tuxedo_io.c:385`). The −692 is `Cargo.lock` −635 (52
packages leaving with `eframe`'s default features, `zbus` 4.4 and `nix` 0.27 replaced), 29 manifest and
28 code lines.

Files: `Cargo.lock`, `Cargo.toml`, `daemon/{Cargo.toml,src/{hardware_control,hardware_detection,tuxedo_io}.rs}`,
`gui/{Cargo.toml,src/{app,main}.rs}`, `gui/src/pages/statistics.rs`,
`drivers_src/tuxedo_io/{tuxedo_io.c,tuxedo_io_ioctl.h}`

### 2026-01-28 — NVIDIA driver and NVAPI tree committed beside the workspace

SHAs: `4cc8df5` `25ba93e` `f50d8d5` `07b7be1`

Four direct commits add `nvidia/` — three files, 1654 lines: `driver.rs` (357, `NV01_DEVICE_0` and
`NV2080_ALLOC_PARAMETERS` allocation with `NV2080_CTRL_CMD_FB_GET_INFO`), `nvidia.rs` (1051, a GPU
controller) and `nvapi.rs` (246, NvAPI bindings). No `.rs` file at `3f59aee` declares a `nvidia`
module, no `.toml` manifest names it, and the tree holds no `bindings/` or `server/gpu_controller`
path; `driver.rs:10` and `nvapi.rs:4` import `crate::bindings::nvidia` and `nvidia.rs:4` imports
`crate::server::gpu_controller`, so nothing compiles the directory. `323b8ab` (2026-10-23) deletes
the tree.

Files: `nvidia/{driver,nvidia,nvapi}.rs`

### 2026-01-29 — NVAPI thermals and voltage inside the daemon (PR #69)

SHAs: `3f59aee` `928f276` `0fd947b` `34483de` `142949a`

PR #69 (merge `3f59aee`, 10 files, +190/−112). A `libloading` binding opens `libnvidia-api.so.1`,
resolves `nvapi_QueryInterface` and calls the IDs `0x0150e828`, `0xe5ac921f`, `0x65fe3aad` and
`0x465f9bcf` through two `#[repr(C)]` structures (`daemon/src/hardware_detection.rs:1302`, `:1331`,
`:1306-1307`), filling `GpuInfo.hotspot_temperature` and `memory_temperature`
(`common/src/types.rs:106-107`); two of the four IDs are marked undocumented in the source. The
`nvidia_smi_legacy_path` setting, its D-Bus method and its Settings row are removed
(`daemon/src/dbus_interface.rs`, `gui/src/pages/settings.rs`). Hardware support is not established by
the diff.

Files: `Cargo.lock`, `common/src/types.rs`, `daemon/{Cargo.toml,src/{main,dbus_interface,hardware_detection}.rs}`,
`gui/src/{app,dbus_client}.rs`, `gui/src/pages/{settings,statistics}.rs`

### 2026-01-29 — NVIDIA extended statistics gated by performance state (PR #70)

SHAs: `2177d94` `e3f5524` `d0f2585`

PR #70 (merge `2177d94`, 1 file, +64/−23) stops the NVAPI extended-statistics call while the card's
performance state is `Eight`, `Twelve` or `Fifteen`, leaving the card to reach its suspended state
(the comment at `daemon/src/hardware_detection.rs:1611` gives that reason) and polls as before
in every other state (`:1606`, `:1613`, `:1620`). Hardware support is not established by the diff.

Files: `daemon/src/hardware_detection.rs`

### 2026-01-29 — Start minimized, and one all-fans-auto call (PR #71)

SHAs: `aae8179` `d3b05e0` `2c0dc13` `9a57972`

PR #71 (merge `aae8179`, 5 files, +58/−16). The `start_minimized` setting, carried since the
initial import without effect, now hides the viewport for the first ten frames when it or `--tray`
asks for it (`gui/src/app.rs:699-704`); when autostart is off the file under `~/.config/autostart/`
is rewritten with `X-GNOME-Autostart-enabled=false`, `Hidden=true` and `NoDisplay=true` instead of
being deleted (`:981`). `set_all_fans_auto` replaces the per-fan `SetFanAuto` on the D-Bus
interface and in the client (`daemon/src/dbus_interface.rs:203`). Its `on_exit` blocks on a fresh
current-thread runtime to send `set_all_fans_auto` and `shutdown_daemon` (`gui/src/app.rs:760`,
`:765`).

Files: `daemon/src/dbus_interface.rs`, `gui/src/{app,dbus_client,main}.rs`, `gui/src/pages/settings.rs`

### 2026-01-30 — NVIDIA capability detection, direct-driver VRAM and GPU fans (PR #72)

SHAs: `e6b1476` `28ee62f` `16eb10f` `82dc748` `4e79c30` `258498b` `8971567` `9747512` `1007faa` `50b47d2`

PR #72 (merge `e6b1476`, 10 files, +1157/−222) adds a direct NVIDIA driver interface beside NVML:
`/dev/nvidiaN` is opened and driven with `NV_ESC_RM_ALLOC` `0x23`, `NV_ESC_REGISTER_FD` `0x27` and
`NV_ESC_RM_CONTROL` `0x2B`, issuing `NV2080_CTRL_CMD_FB_GET_INFO` `0x20800101` for VRAM type, vendor,
bus width and bandwidth (`daemon/src/hardware_detection.rs:1453-1460`, `:1622`). `GpuInfo` gains
clock ranges, architecture, driver version, P-states, power-limit range, offset and fan capability
flags, and the NVML index (`common/src/types.rs`); `get_all_fan_info` returns system and GPU fans
(`:715`), and profiles gain `NvidiaFanSettings` with a manual or auto fan per device, applied through
`set_gpu_fan_speed`/`set_gpu_fan_auto` (`daemon/src/hardware_control.rs:507`, `:515`) beside
`set_gpu_power_limit` (`:499`). Hardware support is not established by the diff.

Files: `common/src/types.rs`, `daemon/src/{main,dbus_interface,hardware_control,hardware_detection}.rs`,
`gui/src/dbus_client.rs`, `gui/src/pages/{profiles,settings,statistics,tuning}.rs`

### Unit 5a accounting

40 commits = 40 named in the 7 entries above + 0 excluded:

No commit in this half is excluded. Six pull requests are merged in this half — #63, #64, #69, #70,
#71 and #72 — and each carries an entry; the four `nvidia/` commits belong to none of them.

## 2026-01-29 → 2026-02-03 — Unit 5b (37 commits, second half of unit 5)

This half runs from the commit after `e6b1476` to `8ed5ae3`: PRs #73, #74, #75, #76 and #77 plus one
direct commit that widens a slider. The halves are bounded by commit, not by date: `24d2fd3` carries
a 2026-01-29 author date, inside 5a's window.

### 2026-01-30 — Structured daemon logging with per-call durations (PR #73)

SHAs: `956d14b` `43470f4` `d77637f` `2e8e39e` `09f2d98` `dbef16d`

PR #73 (merge `956d14b`, 9 files, +558/−348) adds a structured log stream to the daemon: `LogEntry`
gains `target` (`common/src/types.rs:16`), `DaemonLogger::log` keeps the last 1000 records at
`Info` or above in `DAEMON_LOGS` whether or not `RUST_LOG` admits them (`daemon/src/main.rs:69`),
and an unset `RUST_LOG` defaults to `Info` with `zbus` at `Warn` (`:91`). `log_api!` and
`log_api_json!` (`daemon/src/dbus_interface.rs:9`, `:23`) wrap each D-Bus method so it logs
`api.call`, `api.ok` or `api.fail` with `duration_ms`.

Files: `common/src/types.rs`, `daemon/src/{main,dbus_interface,hardware_control,hardware_detection,tuxedo_io}.rs`,
`gui/src/app.rs`, `gui/src/pages/{settings,statistics}.rs`

### 2026-01-30 — VRAM bandwidth for a suspended card and a log search field (PR #74)

SHAs: `bedfede` `64ff2c1` `1033cfc` `952d2cc` `18a2867` `133cf1e` `2c51fc2` `2303b2f` `28c5d87`

PR #74 (merge `bedfede`, 4 files, +237/−59) fills `GpuInfo.vram_bandwidth` for a suspended card
from `NVIDIA_METADATA_CACHE`: `calculate_vram_bandwidth` multiplies the top memory clock by 16 for
GDDR6X, 8 for GDDR6, 4 for GDDR5 and 2 otherwise, times the bus width, over 8000
(`daemon/src/hardware_detection.rs:1713`, `:1978`). The Logs tab gains a case-insensitive search
field with a Clear button (`gui/src/pages/settings.rs:87`, `gui/src/app.rs:66`).

Files: `daemon/src/{main,hardware_detection}.rs`, `gui/src/app.rs`, `gui/src/pages/settings.rs`

### 2026-02-03 — Tiered GPU polling and P0 core-offset feedback (PR #76)

SHAs: `2519afe` `a7184e0` `d6b1601` `d98ecfa` `8b3b8e6` `0bfdc82` `50763ec` `fab8e08` `077159a` `2854f6f`

PR #76 (merge `2519afe`, 8 files, +290/−317) reworks GPU overclocking around P0: each one-second
`gpu_overclock` tick reads the card's P-state, temperature, power and frequency, with the NVAPI
extended statistics only in P0–P3 and NVML alone from P4 (`daemon/src/hardware_detection.rs:2197`),
and writes `set_gpu_core_offset` only while the state is `P0` and the smart-rounded total differs
from `LAST_APPLIED_OFFSET` (`daemon/src/main.rs:587`). `get_gpu_clock_ranges` returns the P0 range
shifted by the current offset (`daemon/src/hardware_detection.rs:1427`), and the core-offset slider
applies on change and re-queries that range (`gui/src/pages/tuning.rs:615`, `:617`); the memory
locked-clock and memory-clock-range D-Bus methods and their client commands are removed. Hardware
support is not established by the diff.

Files: `daemon/src/{main,dbus_interface,hardware_control,hardware_detection}.rs`,
`gui/src/{app,dbus_client,system_tray}.rs`, `gui/src/pages/tuning.rs`

### Unit 5b accounting

37 commits = 25 named in the 3 entries above + 12 excluded:

- 5 commits of PR #75, a repair with no new functionality — the merge `d854801` (+14/−3) and its
  branch commits `4d1b8c0` (+0/−0), `84ace8a` (+26/−3), `613064d` (+9/−21) and `b3f9230` (+4/−4):
  `NvidiaDriverHandle` gains a `device_fd` field so the `/dev/nvidiaN` descriptor opened in `open()`
  outlives that function, and the three `get_fb_info` calls gain `inspect_err` logging.
- 6 commits of PR #77, diagnostics and a document — the merge `8ed5ae3` (+303/−29) and its branch
  commits `ca708bf` (+0/−0), `74bf511` (+101/−23), `ba8978d` (+17/−6), `61e86ed` (+1/−1) and
  `4fd8f58` (+185/−0): VRAM detection logging with decoded RM status codes, and
  `docs/VRAM_DETECTION_TROUBLESHOOTING.md`.
- 1 direct commit `24d2fd3` (+1/−1) — a fan-control slider width.

Five pull requests are merged in this half — #73, #74, #75, #76 and #77 — of which #75 and #77 carry
no entry.

## 2026-02-03 → 2026-02-10 — Unit 6 (24 commits)

Two halves of the same week: gamepad monitoring and the statistics settings land on 2026-02-03, the
hardware cache on 2026-02-04, and PR #81 on 2026-02-10. All twenty-four commits belong to the four
pull requests.

### 2026-02-03 — Gamepad monitoring from sysfs and the udev database (PR #78)

SHAs: `15fb5fe` `eb9dc2e` `96acf9d` `7067fc0`

PR #78 (merge `15fb5fe`, 7 files, +425/−1) adds `get_gamepad_info`, which walks `/sys/class/input`
(`daemon/src/hardware_detection.rs:3063`), reads each `event*` child's `uevent` for MAJOR/MINOR and
then the udev record `/run/udev/data/c{maj}:{min}` (`:3088`), and calls a device a gamepad when
that record holds `E:ID_INPUT_JOYSTICK=1` (`:3090`) or its name names a controller, gamepad or
joystick and not a keyboard (`:3120`), with touchpad and sensor names removed (`:3133`). Each
device becomes `GamepadInfo` — name, `id`, `uid`, status, battery level, connection type, power
status (`common/src/types.rs:202`) — its `uid` being `E:ID_PATH`, else `E:ID_SERIAL`, else the
sysfs path (`:3104`, `:3146`); battery comes from `/sys/class/power_supply` (`:3219`), no
`/dev/input` node is opened, and the GUI remembers devices by `uid` (`gui/src/app.rs:538`).

Files: `common/src/types.rs`, `daemon/src/{dbus_interface,hardware_detection}.rs`,
`gui/src/{app,dbus_client}.rs`, `gui/src/pages/{settings,statistics}.rs`

### 2026-02-03 — Average CPU figures, WiFi channel frequency, log limit (PR #79)

SHAs: `9ecc62a` `193f352` `4130368` `9bf65d5` `2a228a9` `158e596` `5d7f8fe`

PR #79 (merge `9ecc62a`, 9 files, +153/−63) replaces `CpuInfo.median_frequency`/`median_load` with
`average_frequency`/`average_load` (`common/src/types.rs:24-25`), the mean of the per-core values
(`daemon/src/hardware_detection.rs:827`, `:834`), drawn as Average Frequency and Average Load
(`gui/src/pages/statistics.rs:178`, `:186`). `WiFiInfo` gains `channel_freq`
(`common/src/types.rs:178`) and the WiFi block a Channel Info row
(`gui/src/pages/statistics.rs:572`). `AppConfig` gains `log_limit` (default 100) and
`log_filter_trace` (`common/src/types.rs:441-444`), and the daemon gains `read_hw_frequency_limits`
and `restore_cpu_frequency_limits` (`daemon/src/hardware_detection.rs:601`,
`daemon/src/hardware_control.rs:77`).

Files: `common/src/types.rs`, `daemon/src/{hardware_control,hardware_detection,main,tuxedo_io}.rs`,
`gui/src/{app,main}.rs`, `gui/src/pages/{settings,statistics}.rs`

### 2026-02-04 — Hardware statistics served from a one-second cache (PR #80)

SHAs: `bfad0e2` `1232f63` `79b56ad` `7f2b8ff` `88964f5` `19e9fcf` `b3a9ac6`

PR #80 (merge `bfad0e2`, 8 files, +390/−174) moves hardware detection off the D-Bus request path: a
`hardware_monitor` job refreshes `HARDWARE_CACHE` every second (`daemon/src/main.rs:269-273`,
`:458`), one refresh runs at startup (`:265`), and the getters return clones of that snapshot
instead of reading the hardware (`daemon/src/dbus_interface.rs:47`, `:58`, `:69`, `:80`). `GpuInfo`
gains `vram_total` (`common/src/types.rs:136`), filled from the cached NVIDIA metadata
(`daemon/src/hardware_detection.rs:2080`); CPU name and topology, memory metadata and system info
are computed once into `Lazy` statics.

Files: `common/src/types.rs`, `daemon/src/{dbus_interface,hardware_detection,main,tuxedo_io}.rs`,
`gui/src/app.rs`, `gui/src/pages/{settings,tuning}.rs`

### 2026-02-10 — Offset limits cached per GPU, NVML withheld while the card is suspended (PR #81)

SHAs: `a1b958a` `cba3bdc` `3fe7b21` `c3eeae0` `e997653` `2942b0f`

PR #81 (merge `a1b958a`, 13 files, +420/−213) adds `GpuInfo.core_offset_limits` and
`memory_offset_limits` (`common/src/types.rs:122-123`), captured into `NVIDIA_METADATA_CACHE` by the
first full pass (`daemon/src/hardware_detection.rs:2438-2450`) and served to both the NVML path and the
suspended stub. `get_gpu_clock_ranges`, `get_gpu_core_offset_limits` and
`get_gpu_memory_offset_limits` (:1465, :1497, :1518) answer from that cache, and when the cache is
empty and `power/runtime_status` reads `suspended` (:1539) they return
`GPU suspended and metadata not cached` (:1477, :1509, :1530) instead of issuing NVML. A cached entry
missing any field is retried for the missing fields only (:2441).

Files: `common/src/types.rs`, `daemon/src/{dbus_interface,hardware_control,hardware_detection,main}.rs`,
`gui/src/app.rs`, `gui/src/pages/{settings,tuning}.rs`, `drivers_src/tuxedo_io/tuxedo_io.c`

### Unit 6 accounting

24 commits = 24 named in the 4 entries above + 0 excluded:

PR #81's six commits — its merge and five branch commits — are named in the entry above; their
log-verbosity and description-ordering changes carry no entry of their own.

Four pull requests are merged in this block — #78, #79, #80 and #81 — and all four carry an entry.

## 2026-05-14 → 2026-05-22 — Unit 7 (8 commits)

A README and an egui upgrade: two direct commits on 2026-05-14, then the five commits of PR #84.

### 2026-05-22 — egui and eframe upgraded to 0.34.2 (PR #84)

SHAs: `a453a75` `26b0725` `a27cd98` `ceef90b` `e02fb52` `5748f8c`

PR #84 (merge `a453a75`, 10 files, +502/−559) carries the workspace from egui, eframe and
egui_extras 0.29 to 0.34.2 and egui_plot 0.29 to 0.35 in five upgrade commits (`Cargo.toml:11-14`,
`gui/Cargo.toml` keeping `default-features = false` with `default_fonts`, `glow`, `wayland`,
`x11`). 121 changed lines in `gui/src` (+61/−60 across seven files) follow the new API —
`egui::Panel::top(...).show_inside` for the top bar, status bar and update banner,
`ui.global_style()`, `ui.ctx().fonts_mut()` — and the workflow moves `actions/checkout` to v6 and
`actions/upload-artifact` to v7 with `compression-level: 0` (`.github/workflows/rust.yml`).

Files: `.github/workflows/rust.yml`, `Cargo.lock`, `Cargo.toml`, `gui/src/{app,keyboard_shortcuts,theme}.rs`,
`gui/src/pages/{profiles,settings,tuning}.rs`, `gui/src/widgets/fan_curve_editor.rs`

### Unit 7 accounting

8 commits = 6 named in the 1 entry above + 2 excluded:

- 2 direct commits that create the README — `d16d79f` (+229/−0), the first `README.md` in the tree
  (`git log --diff-filter=A -- README.md` returns it), and `4df8bb7` (+3/−0), a caution about Uniwill
  laptop speed limits.

One pull request is merged in this block — #84 — and it carries an entry.

## 2026-08-22 → 2026-08-29 — Unit 8 (41 commits)

Two GPU-polling features on 2026-08-23, three repairs, and six architecture-documentation pull
requests that add 29 commits of prose and no code.

### 2026-08-23 — RTD3-aware NVML gate: suspended, idle and working (PR #88)

SHAs: `a33f83e` `35865a5`

PR #88 (merge `a33f83e`, 3 files, +459/−4) puts a tier decision before every NVML touch. A tick
serves the suspended stub untouched when every adapter reads `suspended`
(`daemon/src/hardware_detection.rs:2222`); when every adapter is either suspended or holds a snapshot
newer than `IDLE_CACHE_TTL_SECS` = 30 s (`:62`), it returns the last full pass's frequency, memory
clock, temperature, load, power and voltage from `IDLE_METRICS_CACHE` (`:80`, `:2306-2313`) and issues
no NVML call — the tier comment at `:46-55` gives the 20 s autosuspend timer as the reason.
`GetGpuInfoFull` (`daemon/src/dbus_interface.rs:99`) arms `FULL_NVML_REFRESH_REQUESTED`
(`daemon/src/main.rs:46`) for one forced pass that the comment at `:92-98` says
deliberately wakes a suspended card. Hardware behaviour is not established by the diff. PR #105
replaces the gate with a three-tier model.

Files: `daemon/src/{dbus_interface,hardware_detection,main}.rs`

### 2026-08-23 — Daemon poll intervals read from the GUI settings (PR #90)

SHAs: `67bb939` `f3c48a8`

PR #90 (merge `67bb939`, 5 files, +376/−4) adds `daemon/src/daemon_settings.rs`: the daemon reads
`statistics_sections` from `~/.config/lapsphere/settings.json` (`:105`) and maps three jobs —
`hardware_monitor` to `min(cpu_poll_rate, gpu_poll_rate)` (`:88`), `fan_control` to
`fans_poll_rate` (`:89`), `gpu_overclock` to `gpu_overclock_poll_rate` (`:90`) — each floored at
50 ms, over the previous hardcoded 1000/2000/1000 ms (`:31-33`). `SyncDaemonPollSettings`
(`daemon/src/dbus_interface.rs:592`) reschedules only the intervals that changed, and the GUI calls it
after writing settings, so a rate change needs no daemon restart.

Files: `daemon/src/{daemon_settings,dbus_interface,main}.rs`, `gui/src/{app,dbus_client}.rs`

### Unit 8 accounting

41 commits = 4 named in the 2 entries above + 37 excluded:

- 2 commits of PR #86, the GUI's D-Bus proxy — merge `a4ae466` (+94/−316) and `47d5beb` (+94/−316):
  one `zbus::Proxy` per connection replaces one per call, and every call site passes it; the same
  calls return the same values, so this is plumbing.
- 3 commits of PR #87, CI — merge `3d76317` (+42/−2), `dd2bd4b` (+27/−2) and `3b516e1` (+20/−5):
  `actions/checkout` to v7, and unzipped per-binary artifacts in `.github/workflows/rust.yml`. PR #100
  later removes the unzipped-binary artifacts #87 adds.
- 3 commits of PR #89, D-Bus reconnect classification — merge `360fb70` (+48/−9), `5ec121e`
  (+49/−9) and `8649a64` (+0/−1): the worker reconnects on the service-absent error family and after
  ten consecutive unrecognized failures, then backs off 2 s; the calls and the states they run in are
  unchanged.
- 10 commits of PR #92, architecture archaeology — merge `3d6f3d4` (+259/−0), `ff17801` (+49/−15),
  `f30e4af` (+14/−0), `ea56d69` (+13/−0), `22845a6` (+18/−0), `e5b19e2` (+13/−0), `0a28b8e` (+6/−0),
  `f1065f6` (+117/−0), `9b993ed` (+143/−0) and `b4c8d3d` (+1/−2): "Architecture overview", "Hardware
  detection deep-dive", "Module and crate map", "Architecture invariants" and "Subsystem
  documentation".
- 5 commits of PR #93, subsystem documents — merge `5f4d1d4` (+265/−65), `0ba4245` (+222/−65),
  `f746e6f` (+0/−1), `8615796` (+74/−30) and `bb1e336` (+4/−4): "Daemon lifecycle and concurrency".
- 3 commits of PR #94 — merge `987d9cd` (+4/−42), `9c8f333` (+3/−42) and `aafafb9` (+2/−1):
  "Correct architecture documentation", which removes ADR-001 "One process-local polling scheduler",
  ADR-002 "Process-global locked state" and ADR-003 "Explicit blocking pool for full GPU refresh" from
  `docs/architecture/decisions.md`.
- 4 commits of PR #95 — merge `012913a` (+199/−0), `a038432` (+199/−0), `e23bd96` (+9/−9) and
  `a124879` (+2/−2): "GUI state synchronization" and the gamepad section of "Architecture
  invariants".
- 4 commits of PR #96 — merge `9724e0a` (+238/−0), `d2782f9` (+241/−0), `8a0212e` (+2/−4) and
  `b8cc784` (+0/−1): "NVIDIA crate archaeology" and "Tuxedo ioctl client archaeology".
- 3 commits of PR #97 — merge `4965305` (+52/−0), `a5e022b` (+53/−0) and `b093ad9` (+1/−2): ADR-001
  "Separate the privileged daemon from the per-user GUI", ADR-002 "Use system D-Bus as the daemon/GUI
  IPC boundary" and ADR-003 "Keep daemon/GUI domain contracts in a shared Rust crate".

Eleven pull requests are merged in this block — #86, #87, #88, #89, #90 and #92 to #97 — of which two
carry an entry.

## 2026-08-25 → 2026-09-29 — Unit 9 (35 commits)

Device ownership, an allocator change, a GPU poll model and a fan-write gate, plus two repair-only
pull requests.

### 2026-09-12 — Daemon-owned `/dev/tuxedo_io` handle and a supervising unit (PR #99)

SHAs: `4444c43` `3fe0148`

PR #99 (merge `4444c43`, 8 files, +80/−21) opens `/dev/tuxedo_io` once per run: `TuxedoIo::shared()`
(`daemon/src/tuxedo_io.rs:136`) keeps one `Arc` in a `OnceLock`, and the D-Bus methods and poll loops
use it instead of opening the device themselves, so interface detection no longer runs inside a poll
tick. The same PR adds `data/lapsphere-daemon.service` — `Type=dbus`, `Restart=on-failure`,
`CPUQuota=50%`, `MemoryMax=768M`, `TasksMax=256` (`:11-22`) — and installs it from `PKGBUILD` and
`debian/rules`, so the daemon runs under systemd supervision and resource caps on both packaging
paths.

Files: `PKGBUILD`, `daemon/src/{dbus_interface,hardware_control,hardware_detection,main,tuxedo_io}.rs`,
`data/lapsphere-daemon.service`, `debian/rules`

### 2026-09-12 — mimalloc as the GUI's global allocator (PR #101)

SHAs: `3bf1507` `e6eec43` `1f8af48`

PR #101 (merge `3bf1507`, 3 files, +39/−0) adds `mimalloc = { version = "0.1", default-features =
true }` to `gui/Cargo.toml:37` — the default features link the bundled C library statically — and sets
`#[global_allocator] static ALLOCATOR: mimalloc::MiMalloc` in `gui/src/main.rs:125-126`, so the GUI
process allocates through mimalloc instead of glibc malloc. `Cargo.lock` gains `mimalloc 0.1.52` and
`libmimalloc-sys 0.1.49`; the daemon keeps the system allocator.

Files: `Cargo.lock`, `gui/Cargo.toml`, `gui/src/main.rs`

### 2026-09-17 — Work-driven GPU live tier, holder inventory, VRAM availability (PR #105)

SHAs: `8105751` `b30aa91` `2b0b9a6` `e2d8642` `4c9cf6f` `02c3abf` `8575bdf` `f3e0f9e` `25fe846`
`c235566` `a7680cb` `607a29a` `e2c313e` `04eaa6f` `8563b17` `854bb6f` `a19a35f`

PR #105 (merge `8105751`, 15 files, +3604/−847) replaces the idle-snapshot gate with one GPU poll path
over three tiers chosen from sysfs (`daemon/src/hardware_detection.rs:219`): `Suspended` (sysfs only,
never probed), `Live` (P0 to P2, a wake from suspend, or any P-state while utilization exceeds 1 %,
`:104`, `:249`) and `Quiet` (sysfs only, one bounded re-probe after `QUIET_REPROBE_SECS` = 45 s,
`:98`). `GpuInfo` gains `pci_bus_id`, `process_snapshot` and `vram_memory`
(`common/src/types.rs:185-191`); `gpu_activity.rs` fills the holder list from `/proc/*/fd` without
opening a device (`:22`) and `hardware_detection.rs:1757` attaches it. The statistics page draws VRAM
availability (`gui/src/pages/statistics.rs:489`); the holder rows it first had are removed again in
the same PR (`04eaa6f`).

Files: `.github/workflows/rust.yml`, `PKGBUILD`, `common/src/{lib,types}.rs`,
`daemon/src/{dbus_interface,gpu_activity,hardware_control,hardware_detection,main}.rs`,
`docs/`, `gui/src/pages/{settings,statistics}.rs`, `gui/tests/fixtures/gpu_legacy.json`

### 2026-09-18 — Remembered gamepads keyed by session-scoped uids (PR #91)

SHAs: `31e3a83` `7a041bc`

PR #91 (merge `31e3a83`, one branch commit, 4 files, +258/−36) moves the GUI's remembered-pad
reconciliation into `gui/src/gamepad_registry.rs`: `is_volatile_uid()` (`:21-22`) treats a `/sys/...`
uid as session-scoped, `reconcile()` (`:39`) removes such a row once the pad is gone (`:43`) instead of
keeping it as disconnected, and a connected pad whose stable uid is new adopts the row with the same
name and connection type (`:86`) rather than appending another. The daemon's uid is unchanged: the
commit labels the two sources (`udev`, `sysfs-path-volatile`) and logs which one produced it
(`daemon/src/hardware_detection.rs:3953-3959`, `:3971`).

Files: `daemon/src/hardware_detection.rs`, `gui/src/{app,gamepad_registry,main}.rs`

### 2026-09-29 — Clevo fan speed written only when that fan needs it (PR #109)

SHAs: `5c57bc7` `4e16cf7` `0b9a1b7` `003302e` `09ba397`

PR #109 (merge `5c57bc7`, 4 files, +364/−24) puts a per-fan gate in front of the Clevo speed write.
`set_fan_speed` (`daemon/src/tuxedo_io.rs:392`) still reads each fan's raw byte and packs all three
into one `i32`, but issues the `W_CL_FANSPEED` ioctl (`MAGIC_WRITE_CL`, sequence `0x10`, `:511`) only
when the request differs from the last commanded value for that fan, the readback differs from the
request, or 30 s have passed (`clevo_should_write`, `:104`; `CLEVO_RECOMMAND_INTERVAL`, `:119`). A
failed read skips the write, and an explicit `SetFanSpeed` bypasses the gate
(`force_next_speed_write`, `:388`). Hardware support is not established by the diff.

Files: `daemon/src/{hardware_control,main,tuxedo_io}.rs`, `data/lapsphere-daemon.service`

### Unit 9 accounting

35 commits = 29 named in the 5 entries above + 6 excluded:

- 2 commits of PR #100, CI and README — merge `ca20c13` (+9/−40) and `b933b9e` (+9/−40): the workflow
  uploads packages only and drops the unzipped-binary artifacts PR #87 added, and the README names the
  two artifact types.
- 4 commits of PR #106, allocator and log-ring retention — merge `4ba9711` (+274/−14), `eabb7c4`
  (+25/−5), `d5b1e90` (+158/−9) and `8880716` (+116/−25): sets mimalloc's `arena_eager_commit`,
  `allow_thp` and `purge_delay` from inside the GUI process, drops Trace records from the daemon log
  ring, and fetches that ring only while the Logs tab is on screen. The allocator stays; which calls
  are issued, and at which moments, is what changes.

Seven pull requests are merged in this block — #99, #100, #101, #105, #106, #109 and #91 — of which
five carry an entry.
## 2026-09-30 → 2026-10-10 — Unit 10 (25 commits)

Seven pull requests land in this window: four carry an entry — the repaint trigger, the in-flight poll
guard, the tray settings file and the first panel release — and three are documentation or repairs.

### 2026-10-03 — Repaint on polled data, coalesced per frame (PR #112)

SHAs: `f0dac95` `97d8134`

PR #112 (merge `f0dac95`, 1 file, +89/−17) adds a repaint trigger to the GUI's poll path:
`REPAINT_PENDING` (`gui/src/app.rs:273`) is a one-shot latch behind `request_repaint_now` (`:276`),
`send_update` (`:284`) asks for a frame only when its send succeeded, and `handle_hardware_updates`
clears the latch at the start of each drain (`:616`), so a burst of updates costs one request per
frame. Every polled component sends through it, so an arrived update is drawn in the next frame
instead of waiting for the 500 ms fallback timer; the comment at `:257-264` gives the measured
update→draw latency (min 0.1, avg 226, max 484 ms) as the reason.

Files: `gui/src/app.rs`

### 2026-10-04 — One in-flight poll request per component, stale replies dropped (PR #113)

SHAs: `be77233` `2a88fa2` `b2c59fe` `2deb3e3`

PR #113 (merge `be77233`, 3 files, +467/−12) bounds the GUI's poll concurrency in
`gui/src/polling_scheduler.rs`: `InFlightSet::try_begin` (`:42`) issues an `InFlightPermit` (`:78`)
whose `Drop` releases the component's claim (`:83`), a refused claim counts a skipped tick (`:58`),
and the coordinator skips a component whose request is still in flight (`gui/src/app.rs:451`).
Polled replies go through `try_send` (`:344`), so an update the UI has no room for is dropped as
stale instead of parking the fetch task on a full channel; `gui/examples/poll_queue_probe.rs`
measures the queue growth the comment at `:12-19` describes.

Files: `gui/examples/poll_queue_probe.rs`, `gui/src/app.rs`, `gui/src/polling_scheduler.rs`

### 2026-10-04 — Tray settings in their own `tray.json` (PR #114)

SHAs: `ffb4caa` `ba219f7` `70cd176` `7c95cd7` `c1765bf`

PR #114 (merge `ffb4caa`, 6 files, +1163/−14) moves the tray settings into
`~/.config/lapsphere/tray.json` (`gui/src/tray_config.rs:30`): a versioned file
(`TRAY_CONFIG_VERSION` = 1, `:27`) holding `start_minimized` and `tray_enabled` (`TRAY_KEYS`, `:101`),
written through a same-directory temporary and a `rename` (`write_atomic`, `:119`).
`migrate_tray_config` (`:229`) copies the two fields out of `settings.json` once, backs that file up
as `settings.json.pre-tray-split` (`:33`) and drops them from it only after `tray.json` is written; a
corrupt `tray.json` is renamed aside instead of deleted. `docs/development/config-files.md` documents
the split.

Files: `README.md`, `docs/development/config-files.md`, `gui/src/app.rs`, `gui/src/main.rs`,
`gui/src/pages/settings.rs`, `gui/src/tray_config.rs`

### 2026-10-04 — Panel mode: a compact column on the app's window (PR #116)

SHAs: `e296679` `0292b70` `a71e39a` `5e58928` `f94499f`

PR #116 (merge `e296679`, 13 files, +3619/−113) adds `gui/src/panel/`: `config` (its own
`panel.json`, `PanelConfig` at `gui/src/panel/config.rs:185`), `rows` (one row per quantity the
Statistics tab shows), `layout`, `render`, `menu` and `window`, with the tab's formatters moved to
`gui/src/stat_format.rs`, and a `--panel` start flag (`gui/src/main.rs:227`) that opens the column in
the app's single window. Width, `background_opacity`, `stacking`, `hide_from_taskbar` and the row
list are stored and normalized; the module comment says the X11 stacking and taskbar atoms are not
applied here (`gui/src/panel/mod.rs:26-28`), and `docs/development/panel-measurements.md` records the
first release's measurements.

Files: `docs/development/panel-measurements.md`, `gui/src/app.rs`, `gui/src/main.rs`,
`gui/src/pages/statistics.rs`, `gui/src/panel/{config,layout,menu,mod,render,rows,window}.rs`,
`gui/src/polling_scheduler.rs`, `gui/src/stat_format.rs`

### Unit 10 accounting

25 commits = 16 named in the 4 entries above + 9 excluded:

- 3 commits of PR #110, README documentation — merge `08f6bb5` (+81/−34), `ed2be44` (+81/−33) and the
  sync merge `3d53701` (+1707/−32): the README rewritten against the current sources, no code.
- 2 commits of PR #118, FB-info indices — merge `8e215fb` (+47/−30) and `8040a39` (+47/−30):
  `get_fb_info` asks for the driver's real struct indices and enum values; the same call in the same
  states.
- 4 commits of PR #117, idle allocation churn — merge `3686f1c` (+2/−4), `8ed4948` (+63/−1),
  `62439d8` (+5/−41) and `43d82fa` (+0/−28): `SystemdService=lapsphere-daemon.service` in both unit
  files, then the removal of the `ui_is_alive()` gate that `8ed4948` adds, with its test, and comment
  and dead-code trimming.

Seven pull requests are merged in this block — #110, #112, #113, #114, #116, #117 and #118 — of which
four carry an entry.

## 2026-10-10 → 2026-10-14 — Unit 11a (41 commits, first half of unit 11)

Unit 11 runs from the merge of PR #120 to that of PR #132 and is written in two halves split at the
merge of PR #121 (`3b7bd8f`): 41 commits here, 43 in the second half. The halves are bounded by commit
rather than by date: PR #120's branch commits carry 2026-10-10 author dates, inside the previous
unit's window.

### 2026-10-12 — CPU frequency limits clamped, GPU writes gated, overclock released at idle (PR #120)

SHAs: `b1c8719` `46faa7f` `4693295` `b3b8cec` `3182c5a` `0f7b97e` `176fba3`
`12a2a74` `14aecbc` `20efeb8` `b730246` `de57f74` `145b1a0` `ba65a91`
`bb5a3f0` `a4688e8` `2d2f670` `71046e9` `d7edb62` `bcf1f6d` `f1bb358`
`bccc389` `32f8d1b` `d544db1` `4befd0e` `8b072cd` `5398c21` `dd4af45`
`ed7368e` `d06bc47` `21f6917` `7851c43` `cfd9722` `5163a42` `16ab74b`
`a2f7d2f` `8dea03e` `51867d4` `8f3a5fa`

PR #120 (merge `b1c8719`, 9 files, +2119/−254) clamps CPU frequency requests to the hardware range:
`normalize_freq_limits` (`common/src/types.rs:1221`) and `normalize_profiles_freq` (`:1241`) run
through `normalize_cpu_frequency_request` (`daemon/src/hardware_control.rs:296`) inside
`set_cpu_frequency_limits` (`:580`, reached from the D-Bus method at `daemon/src/dbus_interface.rs:171`),
on profile load and save (`gui/src/app.rs:893`) and at the tuning page's two controls
(`gui/src/pages/tuning.rs:372`, `:403`). GPU writes pass `gpu_write_allowed`
(`daemon/src/hardware_detection.rs:5060`), which reads `power/runtime_status`; an apply that meets a
closed gate is deferred and reports `gpu_deferred` (`common/src/types.rs:80`, rule at
`daemon/src/hardware_control.rs:1052`), and `release_oc_when_idle` (`:617`) releases the dynamic
overclock after measured idle, restoring it on load (`daemon/src/main.rs:1007`). Hardware support is
not established by the diff.

Files: `common/src/types.rs`,
`daemon/src/{dbus_interface,hardware_control,hardware_detection,main}.rs`, `gui/src/app.rs`,
`gui/src/dbus_client.rs`, `gui/src/pages/{profiles,tuning}.rs`

### Unit 11a accounting

41 commits = 39 named in the 1 entry above + 2 excluded:

- 2 commits of PR #121, panel documentation — merge `3b7bd8f` (+120/−25) and `411f68a` (+120/−25): the
  panel module documentation and the PR 1 measurements reconciled, with doc comments in `menu.rs`,
  `mod.rs` and `window.rs`; no behaviour changes.

One pull request in this half carries an entry — #120; #121 documents the panel and is excluded above.
