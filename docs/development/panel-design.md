# Mini-panel design (gkrellm-style compact mode)

Status: **DRAFT for discussion.** No application code has been written.
Companion evidence documents: [`panel-spike.md`](./panel-spike.md) (frame
delivery, `ViewportCommand` behaviour) and
[`panel-overlay-tests.md`](./panel-overlay-tests.md) (live overlay behaviour
over another window, B1–B8).

Base: `5c57bc7` (branch `feat/mini-panel` from `origin/lapsphere`).
Measurement environment for the claims below: **X11 / XFCE / xfwm4 4.20.0,
no compositor.** Everything Wayland is `NOT TESTED` and is designed for on the
assumption that it degrades to a no-op, not on the assumption that it works.

---

## Goals and non-goals

### Goal

Replace **MangoHud** in the case where the panel's window is visible *over* a
running game — that is, in two situations:

* **Windowed games**, with the panel placed beside the game window.
* **Borderless fullscreen** (compositor-style fullscreen), where the game
  covers the whole screen and the panel is drawn on top of it.
* **A second monitor**, with the game on one screen and the panel on the
  other.

In other words the target is an *overlay HUD for games*, not a desktop
sidebar. That distinction matters: an overlay has to survive stacking above a
fullscreen window, must not steal focus while the game has it, and must not
cost the game frames. Criteria 5 and 6 in the acceptance list below exist
because of it.

Secondary goal: a compact always-visible meter in normal desktop use, in the
spirit of gkrellm — a single undecorated strip, draggable anywhere, showing
the hardware you care about at a glance. It is a **mode of the same window**,
not a second window and not a second process.

### Non-goal: an exclusive fullscreen mode

**The owner has decided against an exclusive-fullscreen panel mode.** This is
recorded as a product decision, not a technical one.

Rationale as given by the owner:

* Exclusive fullscreen (grabbing the display, hiding the desktop) is poorly
  behaved with modern desktop environments.
* It gives **no measurable fps benefit** over a borderless/fullscreen-window
  arrangement, per the owner's own measurements.
* Most games do not use it by default.

So the panel will never request exclusive fullscreen. If a game is
borderless-fullscreen, the panel is an ordinary window that sits above it —
which is exactly the case that is **currently failing** on xfwm4 (see
[`panel-overlay-tests.md`](./panel-overlay-tests.md) B2: with a genuine
fullscreen cube, the panel is covered until it is re-raised or
`_NET_WM_STATE_ABOVE` is set). This is the single most important open risk
against the primary goal and is tracked as an open question (Q7).

### Non-goals, other

* Replacing the Statistics page, or adding new data sources. The panel
  renders a chosen subset of what Statistics already shows.

### Wayland

Best-effort. **Not tested on the owner's machine — their Wayland session does
not start**, and this agent host has no Wayland session either. The design
position is that a window command which does not take effect is a harmless
no-op: the panel still draws, at whatever size the compositor gave it. This is
a deliberate choice to degrade rather than a claim that it works.

---

## Accepted decisions

### ADR-1 — Mode switching happens inside one process, via `ViewportCommand`

**Decision.** Full mode and panel mode are two states of the *same* eframe
process and the *same* root viewport. Switching mutates the window with
`ViewportCommand::{InnerSize, MinInnerSize, Resizable, Decorations,
WindowLevel}`. The single-instance guard (the `io.lapsphere.Gui` D-Bus name)
is untouched, as is the process lifecycle. Restarting the process remains
available as a fallback, not as the mechanism.

**Evidence.** All five commands were exercised at runtime on X11 and all five
work; 300 round trips produced a **+16 kB** RSS delta and a correct final
window state, with 313 external `xprop` samples showing clean alternation and
no stuck or intermediate geometry. See panel-spike §2 and §4(г).

A panel-shaped window can also be created **directly** through
`ViewportBuilder` with no large-window flash at all (frame 0 already
260x72, `outer == inner`, panel-spike §2). That is the path for "start in
panel mode".

**Rejected: restart the process to change mode.** Simple and always correct,
but it drops the D-Bus connection, re-runs discovery, flickers the tray icon,
and discards the poll scheduler. Unacceptable for a mode toggle.

**Rejected: a second `egui::Viewport` (a second native window).** egui
supports it and it would give independent size, but it introduces a second
GL surface and complicates the tray's single-window mental model. It also
buys nothing here: we want the *same* window reshaped, not a new one.

**Cost accepted.** X11 re-frames the window on every `Decorations` toggle, so
the restored position is not preserved across a switch (measured: ended at
0,21 rather than the pre-test 300,200). Position must be re-applied, not
assumed. Off-screen position requests are clamped by the WM rather than
rejected, so the panel must clamp against monitor size itself.

---

### ADR-2 — Panel size is a function of the selected elements only, never of data availability

**Decision.** The panel's width and height are computed **solely** from which
elements are enabled and their fixed layout metrics. They do **not** depend
on whether data has arrived, and they do not reflow when data changes.

* An element whose data is unavailable renders as `—` (em dash) in a slot
  that is **already reserved at full width**. No relayout, no width change,
  no reflow on the first successful poll.
* The per-core CPU graph is **one bar of fixed height**, not one bar per
  core. The number of cores can change (SMT toggled at runtime, or a
  different machine reading the same config), so per-core bars would change
  the window height. A single fixed-height strip does not.

**Rationale.** gkrellm panels have a stable footprint; a meter that resizes
the window when a dGPU drops off the bus or a gamepad disconnects is
unusable, because you cannot place it. Reserving width for absent data is
the price of a window that never moves under the user's cursor.

**Rejected: hide elements with no data.** The panel would change size
whenever a device appeared or disappeared, and the user's click target would
move. Directly contradicts the goal.

**Rejected: reflow to fit available data.** Same objection, worse: the
layout would depend on daemon timing, so the same config would produce
different windows on different runs.

**Consequence for the config schema.** Every element needs a declared fixed
width and height contribution, decided at design time, not measured at
runtime. This is why the config format (open question Q1) has to carry
layout metrics rather than just a boolean per element.

---

### ADR-3 — Wayland and X11 are both supported, and no-op is an acceptable outcome

**Decision.** Keep the current dual-backend build (`eframe` features
`wayland, x11`, unchanged). A window command that does not take effect is a
**harmless no-op** — the panel still draws, just at the size the compositor
chose. The panel must not depend on:

* **window position** — Wayland does not expose it
  (`ViewportInfo.outer_rect` is documented as always `None` on Wayland), and
  the compositor owns placement. The panel is positioned by the user
  dragging it, and its position is a *hint* on Wayland and a *request* on X11.
* **always-on-top** — the only window-level primitive, and the one most
  likely to be restricted by mutter/kwin for a non-fullscreen client. If
  `WindowLevel(AlwaysOnTop)` does not stick, the panel is an ordinary
  window. That is a degraded experience, not a broken one.

**Not tested.** Every Wayland claim in panel-spike is `NOT TESTED`: this host
has no Wayland session, no compositor and no passwordless sudo. The design
above is chosen *because* it is the option that degrades most gracefully, not
because Wayland behaviour was observed.

**Rejected: gate the panel behind X11.** Rejected on the grounds that we
cannot demonstrate the X11-only alternative is better on the machines we
cannot test. Degrading gracefully is the honest default.

**Rejected: shell out to `xdotool`/`wmctrl` to place the panel.** Adds a
runtime dependency, breaks on Wayland entirely, and is redundant given
`ViewportCommand` already does the job on X11.

---

### ADR-4 — In panel mode, poll only the selected subset, and only via cache-backed getters

**Decision.** In panel mode the UI polls only the components the panel
actually shows: `cpu`, `gpu`, `memory`, `fans`, `battery`, plus `wifi` and
`gamepads` **if those elements are selected**. Components whose elements are
all disabled are not polled at all. Every getter the panel uses is
**cache-backed**: the daemon's `hardware_monitor` job fills one shared
`HARDWARE_CACHE` (`daemon/src/main.rs:31`, refreshed by the `hardware_monitor` poll job at
`daemon/src/main.rs:332-345`), and the D-Bus getters only clone out of it
(`daemon/src/dbus_interface.rs:73-107, 265-273`). So narrowing the UI poll
set costs **no extra hardware access** — it only stops the GUI from
marshalling JSON it will not draw.

**There is no `GetGpuInfoFull` method.** The brief assumed one; the interface
does not have it. Verified by introspecting the live interface — the full
method list contains `GetGpuInfo`, `GetGpuClockRanges`, `GetGpuCoreOffsetLimits`,
`GetGpuMemoryOffsetLimits`, and no `*Full` variant. The expensive part is not
a separate method but a field **inside** `GetGpuInfo`: every `GpuInfo` carries
a `process_snapshot` with a per-process list. Measured payload: `GetGpuInfo`
2 452 bytes, `GetCpuInfo` 2 463, `GetFanInfo` 239, `GetWifiInfo` 552,
`GetStorageDeviceInfo` 255, `GetGamepadInfo` 7, and `GetDaemonLogs` **735 087**.

So the rule is: **the panel never polls `logs`** (735 kB per call), and
GPU process lists are not rendered in panel mode. If a cheaper GPU getter is
wanted later, adding one is a daemon-side change and out of scope here.

**Existing precedent.** The `logs` component is already gated by exactly this
kind of policy: `UI_FRAME_FRESH_MS` / `LOGS_TAB_ON_SCREEN`
(`gui/src/app.rs:56-90`) stop the callback unless the Logs tab is on screen
and a frame was painted within 5 s, registered at `gui/src/app.rs:442`. The
mechanism to reuse is `RefreshCoordinator::UpdateInterval`
(`gui/src/polling_scheduler.rs`), which already exists and is currently
unused for this purpose.

**Evidence for why it matters.** The live measurement ran 12 components
concurrently and consumed 684 updates over 120 s with `queue_max=10/100` and
`blocked=0` — the pipeline is nowhere near saturation today. So narrowing the
poll set is a **battery and CPU argument, not a stability argument**, and
should be presented that way rather than as a fix.

**Rejected: poll everything, filter at draw time.** Simplest, and today's
behaviour. Costs D-Bus traffic and JSON allocation for data nothing reads
(735 kB per `GetDaemonLogs` call is the extreme case, already mitigated for
the Logs tab but not for other unused components).

**Rejected: one new `GetPanelData` D-Bus method returning exactly the
selected fields.** Attractive — one round trip, minimal payload — but it is a
daemon interface change with its own versioning and compatibility burden,
and it duplicates the cache. Deferred; revisit if the poll set proves too
coarse in practice.

---

### ADR-1 amendment (from H1–H6) — mode switching stays in-process; the panel becomes a second native window

**The core decision is unchanged: mode switching is in-process, via
`ViewportCommand`, on the existing GUI window.** What changed is where the
*panel* lives. It is no longer a reshape of the root window; it is a second
native window in the same process, created with
`Context::show_viewport_immediate` and
`ViewportBuilder::with_window_type(Utility)`.

**Why the amendment.** The earlier rounds concluded that the X11 window type
was permanent (B16) and that Q8 required a `Utility` type, so the root had to
be `Utility` too — which made the *main* GUI window a Utility window for its
whole life. H5 removes that constraint: a child viewport gets its own window
and its own type, so the root can stay `Normal`.

```
root  "LapSphere"          NORMAL, _NET_WM_STATE empty       <- ordinary window
child "LapSphere Panel"    UTILITY, SKIP_PAGER + SKIP_TASKBAR <- the panel
```

Measured on xfwm4 4.20.0, no compositor: the child stays above a fullscreen
cube through focus return, Alt-Tab both ways and 5 minutes idle; it is
clickable; RSS is flat at ~105–107 MB with no window-count growth.

**What this costs, and it is not free:**

1. **A second native window and a second GL surface.** This is the original
   objection to a second window and it stands. The WM now sees two windows for
   one application, which affects alt-tab grouping and any WM task UI.
2. **The child is a separate lifecycle.** Mode switching must explicitly show
   and hide the child. Whether it keeps painting while the root is hidden is
   `NOT TESTED` — `ViewportInfo::visible()` derives from
   `minimized`/`occluded` and nothing here establishes a guarantee for a child.
3. **The tray has to learn about two windows.** Today `TrayEvent::ShowWindow`
   maps to the root's `Visible(true)` (`gui/src/app.rs:827`), which is the
   wrong target in panel mode. `NOT TESTED`.
4. **Taskbar / Alt-Tab appearance is `NOT TESTED`** — this host has no tasklist
   plugin. `panel-owner-check.sh` exists for the owner to settle it by eye.

**Alternatives, with their measured status:**

| Option | Status |
| --- | --- |
| Root window reshaped, type `Utility` for the process | worked (B14) but makes the main GUI a Utility window — **superseded by H5** |
| Root window reshaped, type `Dock` | **rejected**: B10 measured that a Dock window takes no clicks, which breaks the panel's back button |
| Message-based SKIP_TASKBAR on a Normal window | **`wmctrl` proves it works on xfwm4 4.20.0**, but our own x11rb message is refused and the root cause is unknown (see the open bug in Q8). Cheaper than H5 if solved: no second window. Worth one more attempt before committing to H5. |
| Withdrawn-cycle re-read (H2) | **rejected**: the withdraw/re-show is clean, but the atoms do not survive to the re-show |
| `WM_TRANSIENT_FOR` (H4), KDE switcher atom (H3) | **rejected**: no measurable effect on xfwm4 |
| Separate process for the panel | **rejected**: breaks the single-instance guard and the shared tray; ADR-1's reason |

**Recommendation.** Implement H5, but first spend one short attempt on the
message route, because `wmctrl` demonstrates it is achievable on this WM and
H5 carries costs 2–4 above. If the message route cannot be made to work,
H5 is the fallback and it is measured to work.

**Always-on-top, unchanged.** B9/B14/H5 all agree: send
`ViewportCommand::WindowLevel(AlwaysOnTop)` **once, after the window exists**.
It holds indefinitely; no re-assertion, which would be a busy-wait against a
game. `ViewportBuilder::with_window_level` is ignored by xfwm4, so a
start-in-panel-mode path must still set the level on the first frame.

**Unverified.** All of this is xfwm4 4.20.0 with **no compositor**. Whether
mutter, kwin, GNOME or KDE behave the same — for the window-type rule, for
the runtime `WindowLevel`, and for whether a child viewport stays above a
fullscreen game — is `NOT TESTED`. A compositor would most likely honour the
startup hint, making the always-on-top part moot, which remains the strongest
reason to ask whether the owner runs one.

---

## Acceptance criteria

Each item is checkable, and each names the staged-plan step that delivers it
(see "Staged implementation plan" at the end). Status column refers to what
the throwaway `gui/examples/panel_overlay_demo.rs` has already demonstrated,
not to shipped code — no application code exists yet.

| # | Criterion | Verified in | Step | Status |
| --- | --- | --- | --- | --- |
| 1 | Panel elements can be in **any order**. The ordering UI reuses the existing pattern from `gui/src/pages/settings.rs:446-471` — ⬆/⬉ buttons per row, normalised through `normalize_section_order` exactly as the statistics section order is. | demo `--items a,b,c` | 3 | **PARTIAL** — reordering changes draw order and leaves the window size unchanged (584x81 reversed vs 584x81 original). The settings UI itself is not written. |
| 2 | **Font size**: a global panel scale plus an optional per-item override. The window size and every reserved field width are computed from the metrics of the *selected font*, not from a constant. Restating ADR-2 for the font case: **window size depends only on the set of selected elements and their fonts, never on data availability.** | demo `--scale`, `--item-scale` | 3, 4 | **PARTIAL** — geometry follows the config: 4 items 584px → 903px at scale 1.6; 12 items 1743px. Hot-reload of scale at runtime is **NOT TESTED** (demo flags are startup-only). |
| 3 | **Units and labels preserve case**: `MHz`, `dBi`, `dBm`, `°C`, `mA`, `Wh`, `W`, `V`. All unit strings live in **one module**; no widget formats a unit inline. An element's label can be overridden with user text. | `panel_overlay_demo.rs` `mod unit` + `--label` | 4 | **PARTIAL** — the demo centralises units in one `mod unit` and takes `--label id=text`. Two corrections to the brief, both verified in source: **`dBi` does not exist anywhere in the codebase** (WiFi is `signal_level`, dBm only), and **`BatteryInfo` has no Wh field** (only `voltage_mv`, `current_ma`, `capacity_mah`). See the config-schema table. `statistics.rs` still formats units inline — that is the code to fix. |
| 4 | **Unavailable data** (dGPU, gamepad, WiFi) shows as `—` in an already-reserved width; the window never resizes. | demo synthetic mode | 4 | **WORKS in the demo** — dGPU fields drop every 5th snapshot, WiFi every 7th; panel renders `—` and geometry held at 584x81 for the whole session. |
| 5 | The panel **does not bring a discrete GPU out of runtime suspend**. | B6c, 3-phase control | 4, 5 | **PASSES** — from a genuinely `suspended` dGPU, the panel running with live `GetGpuInfo` left it `suspended` at all ten 3-second samples and 5s after exit. Control: no panel, identical. Control 2: `nvidia-smi` polling alone wakes it in 6s. (The panel itself renders on the **AMD iGPU** — it maps `libgallium`/`libGLX_mesa`, no NVIDIA libs.) |
| 6 | With click-through enabled, **clicks on the panel must not disturb the game**. | B3, 4-case matrix | 6 | **WORKS** — without passthrough a click moves focus to the panel; with `MousePassthrough(true)` the click reaches the window underneath (the cube when overlapped, the desktop when not). Keyboard pass-through is **NOT TESTED**. |

Two criteria that came out of the overlay testing and are **not yet met**,
listed here so they are not lost:

| # | Criterion | Status |
| --- | --- | --- |
| 7 | The panel must stay visible **above a borderless-fullscreen game** — the primary use case. | **SOLVED on xfwm4 4.20.0 without a compositor (B9/B14), and confirmed for the H5 child viewport**: the `Utility` child stayed on top through a fullscreen cube, focus return, Alt+Tab both ways and 5 min idle, while the root stayed `Normal`. (B9/B14 detail: (fullscreen cube, focus return, click on the cube, Alt+Tab both ways, 5 min idle — all `PANEL-ON-TOP`; 0.3% CPU). Also established in B9: With a genuine fullscreen cube (2560x1440@0,0) the panel stayed on top through focus return to the cube, a click on the cube, Alt+Tab away and back, and **5 minutes idle** — with a single runtime `ViewportCommand::WindowLevel(AlwaysOnTop)` and **no re-assertion**. B2's failure was entirely the *startup* path: `with_window_level()` is ignored, the runtime command is honoured. |
| 8 | The panel must not appear in the taskbar / window switcher. | **SOLVED on xfwm4 4.20.0 by H5 — two windows in one process.** The **child** viewport is `Utility`, so xfwm4 grants `SKIP_TASKBAR`+`SKIP_PAGER` (measured); the **root** stays `_NET_WM_WINDOW_TYPE_NORMAL` with empty `_NET_WM_STATE`, so the main window is an ordinary window. Also measured: `_KDE_NET_WM_STATE_SKIP_SWITCHER` and `WM_TRANSIENT_FOR` have no effect (H3, H4), and the withdrawn-cycle trick does not help (H2). **Caveat: B13/H1's message-based route failed in *our* implementation while `wmctrl` succeeded — see the open bug below; a message-based solution may still exist.** |

---

## Config schema (proposal for discussion — not an implementation)

A proposed shape for `settings.json`, to be argued over rather than adopted:

```jsonc
"panel": {
  "mode_active": true,          // start in panel mode
  "font_scale": 1.0,            // global scale
  "always_on_top": true,
  "click_through": false,
  "position": [200, 500],       // [x, y] in points, or null for "let the WM place it"
  "items": [
    { "id": "cpu_load_percent", "visible": true, "label": null, "font_scale": null },
    { "id": "memory",           "visible": true, "label": "RAM", "font_scale": 1.2 }
  ]
}
```

`label: null` means "use the built-in default"; a string overrides it
(criterion 3). `font_scale: null` means "inherit the global scale".
Array order **is** the display order (criterion 1).

### Item table

Every row verified against `common/src/types.rs` and the live D-Bus
interface. `width` is the reserved slot in points at `font_scale = 1.0` and is
the number that makes ADR-2 computable without looking at any data. **No
`GetGpuInfoFull` exists** — the live interface exposes `GetGpuInfo`,
`GetGpuClockRanges`, `GetGpuCoreOffsetLimits` and `GetGpuMemoryOffsetLimits`,
and nothing else GPU-related; the expensive part is a `process_snapshot` field
*inside* `GetGpuInfo`.

| id | Data source | Field (`common::types`) | Unit | width |
| --- | --- | --- | --- | --- |
| `hostname` | `GetSystemInfo` | `SystemInfo.product_name` | — | 150 |
| `cpu_load_percent` | `GetCpuInfo` | `CpuInfo.average_load` (f32) | `%` | 62 |
| `cpu_core_chart` | `GetCpuInfo` | `CpuInfo.cores[].load` (`Vec<CoreInfo>`, one fixed-height strip) | `%` | 92 |
| `cpu_freq` | `GetCpuInfo` | `CpuInfo.average_frequency` (u64) | `MHz` | 74 |
| `cpu_temp` | `GetCpuInfo` | `CpuInfo.package_temp` (f32) | `°C` | 62 |
| `cpu_power` | `GetCpuInfo` | `CpuInfo.package_power` (`Option<f32>`) | `W` | 64 |
| `gpu_load` | `GetGpuInfo` | `GpuInfo.load` (`Option<f32>`) | `%` | 62 |
| `gpu_temp` | `GetGpuInfo` | `GpuInfo.temperature` (`Option<f32>`) | `°C` | 62 |
| `gpu_clock` | `GetGpuInfo` | `GpuInfo.frequency` (`Option<u64>`) | `MHz` | 74 |
| `gpu_power` | `GetGpuInfo` | `GpuInfo.power` (`Option<f32>`) | `W` | 64 |
| `memory` | `GetMemoryInfo` | `MemoryInfo.used_percent` (f32) + `used_gib` (f64) | `%`, `GiB` | 84 |
| `battery` | `GetBatteryInfo` | `BatteryInfo.charge_percent` (u64) | `%` | 62 |
| `wifi_signal` | `GetWifiInfo` | `WiFiInfo.signal_level` (`Option<i32>`) | `dBm` | 70 |
| `fans` | `GetFanInfo` | `FanInfo.rpm_or_percent` (u32) + `is_rpm` (bool) | `rpm` or `%` | 62 |
| `gamepad_1` | `GetGamepadInfo` | `GamepadInfo.name`, `battery_level` (`Option<u8>`), `status` | `%` | 120 |
| `gamepad_2` | `GetGamepadInfo` | same, second entry | `%` | 120 |

Notes on the table, all checked in source rather than assumed:

* **`dBi` is not a field anywhere.** `WiFiInfo` carries `signal_level` in dBm
  (negative, closer to 0 is stronger) and nothing in dBi. The brief listed
  `dBi`; it should be dropped or the field added to the daemon first.
* **No watt-hours.** `BatteryInfo` has `voltage_mv` (u64), `current_ma` (i64)
  and `capacity_mah` (u64) but no Wh. Wh is computable from
  `voltage_mv × capacity_mah`, so an energy readout is possible without a
  daemon change, but it is a derived value and should be labelled as one.
* **`hostname` is a name, not a hostname.** `GetSystemInfo` returns
  `product_name`, `product_sku`, `manufacturer`, `board_name`,
  `bios_version`, `kernel_modules`. The daemon also has a `GetMachineId`
  method. Deciding which of these the user means by "hostname" is part of Q1.
* **`fans` is ambiguous.** `FanInfo.rpm_or_percent` is RPM *or* percent
  depending on `is_rpm`, and the array can hold several fans. One slot for
  several fans needs a rule (first, max, or a cycle) — part of Q1.
* **`gamepad_*` slots.** `GetGamepadInfo` returns a `Vec<GamepadInfo>` whose
  length is however many are connected. "Slot 1" and "slot 2" therefore mean
  *array index*, which shifts when a controller disconnects. Part of Q1.
* **Optional-ness drives the `—`.** `GpuInfo.load`, `.temperature`,
  `.frequency` and `.power` are all `Option<f32>`/`Option<u64>`, as are
  `WiFiInfo.signal_level`, `CpuInfo.package_power` and
  `GamepadInfo.battery_level`. That is exactly the case criterion 4 covers.

---

## Interface

**In the main window.** A toggle button in the existing top bar
(`gui/src/app.rs:886`, the `draw_top_bar(ui)` call; defined at `gui/src/app.rs:706`) switches to panel mode. It is
the only place the mode can be entered from, other than config at startup.

**In the panel.**

* A **back button** — returns to full mode in the same process. Placed at a
  fixed position (leading edge of the panel) so it does not move as elements
  change.
* An **element-selection menu**, opened by a small affordance next to the
  back button. Element set, drawn from what the Statistics page already has
  (`gui/src/pages/statistics.rs`): hostname / system name, CPU load graphs
  (the single fixed-height strip of ADR-2), CPU load percentages, CPU
  frequency, CPU temperature, GPU name, GPU load, GPU temperature, GPU
  frequency, GPU VRAM, memory used / total, memory percentage, fan speeds and
  temperatures, battery percentage, battery status, WiFi signal, WiFi SSID,
  gamepad name, gamepad battery.

Selection is **per-mode persistent**, not a transient toggle: it is written to
`settings.json` and restored on next start, so the panel looks the same every
time it appears.

**Note on element identity.** "Hostname" is not currently a Statistics
element. `GetSystemInfo` returns product name, manufacturer and BIOS version
(`gui/src/pages/statistics.rs:145-156`), and `GetMachineId` exists on the
daemon. Deciding which of these is "hostname" is part of Q1.

---

## Open questions

**Q1 — Config schema for the panel in `settings.json`.** Needs: the element
list, ordering, per-element flags (visible, units, decimals, graph vs
number), the fixed width/height each element contributes (ADR-2 makes these
mandatory data, not derived values), and gamepad slot handling. Open
sub-questions: does the schema hardcode the element enum or accept arbitrary
strings (unknown entries must degrade to `—`, not crash); is ordering a list
of ids or a map with an index; are widths in points or character cells;
and how are gamepad slots numbered when a second controller appears. Note
`AppConfig` (`common/src/types.rs:829`) already has `statistics_sections`
with `show_*` booleans and poll rates — the panel config should probably
follow that shape rather than invent a new one.

**Q2 — What does closing the panel window do when the tray is enabled?**
Today `gui/src/app.rs:877-882` converts a close request into
`CancelClose` + `Visible(false)` when the tray is on, so the window hides
rather than exits. Should the panel follow the same rule (close = hide to
tray, restore from tray) or actually quit? A panel is a persistent
fixture, so "close" meaning "hide" is probably right — but then a panel user
who never opens the tray has no way back. Needs a decision.

**Q3 — When, if ever, to move the channel drain into `logic()`.** The spike
**refuted** the original motivation: over 600 s of `Visible(false)` with a
live daemon, `prod == cons == 3534` and `ui()` was called 1 645 times, so the
existing drain in `ui()` keeps up and moving it to `logic()` fixes nothing
(panel-spike §1б, §1д). It would only matter under a **WM iconify**, where
`ui()` provably stops (frozen for a measured 20 s). So: is the iconify case
in scope? If not, drop the step. If yes, `logic()` is the right hook and the
justification is iconify, not tray-hide. Related: a panel that the user
minimizes should arguably keep updating state, which is the same argument.

**Q4 — Queue policy.** The 100-slot channel (`gui/src/app.rs:325`) was never
under pressure in any measurement (`queue_max` 10/100, `blocked=0` over both
120 s visible and 600 s hidden). So this is **not** a fix for an observed
problem. The question is whether to adopt `try_send` + drop-oldest for
staleness-prone data (a CPU load reading 8 s old is worthless; a new one
should replace it), or keep `send().await`. The tradeoff is that `try_send`
changes back-pressure semantics for the Tuning page, which may have actions
where dropping is wrong. Suggests a split: `watch`/latest-value semantics
for telemetry, `try_send` or a queue for actions.

**Q5 — Should the tray click wake the event loop explicitly?** A tray click
does not wake the loop today; it is only seen on the next `ui()` call, i.e.
within 500 ms, and only because `ui()` keeps running while `Visible(false)`.
That is an accident of `ViewportInfo::visible()` returning `None` on X11, not
a designed behaviour. Giving the ksni thread a cloned `egui::Context` to call
`request_repaint()` would make it robust (and would fix the iconify case).
Small, but it is an app change outside the panel's scope.

**Q6 — What is the first thing to fix, given the measured latency?** With a
visible window, update→draw latency is **min 0.1 / avg 222 / max 484 ms**,
bounded entirely by the single `request_repaint_after(500ms)`
(`gui/src/app.rs:917`). A live meter in a 72-point-tall panel will visibly
lag. This is independent of the panel and worth doing first (staged step 1).

**Q7 — RESOLVED on xfwm4 4.20.0: request always-on-top at runtime, once.**
Closed by B9. The answer is simpler than the question anticipated.

* Send `ViewportCommand::WindowLevel(AlwaysOnTop)` **once, after the window
  exists**. It is honoured: `_NET_WM_STATE_ABOVE` appears and stays.
* The panel then **held on top for the whole test**: focus returned to the
  cube, a click on the cube, Alt+Tab away and back, and 5 minutes of complete
  inactivity. Every sample read `PANEL-ON-TOP`.
* **No periodic re-assertion is needed.** The re-assert machinery was built
  and then not required. A re-assert loop would have been a busy-wait running
  against a game, so dropping it is the better outcome — and the panel cost
  0.3% CPU / ~100 MB RSS while idle over the 5-minute window.

The original B2 failure was entirely a **startup-vs-runtime** distinction:
`ViewportBuilder::with_window_level(AlwaysOnTop)` is silently ignored by
xfwm4, while the runtime `ViewportCommand` works. The design implication is
that "start directly in panel mode" (ADR-1, staged step 6) must set the
level on the first frame rather than in the builder, or it will come up
*underneath* the game.

Still open: this is xfwm4 4.20.0 with **no compositor**. Whether mutter or
kwin behave the same is `NOT TESTED`. A compositor would likely honour the
startup hint and make the whole question moot, which is a further reason to
ask whether the owner runs one.

**Q8 — RESOLVED on xfwm4 4.20.0 by H5: the panel is a second native window,
so the main window does not have to be a `Utility` window.**

The previous round accepted a permanent `Utility` type for the whole process
and paid for it with the main window. H5 removes that cost.

**The working design.** One process, two native windows:

```
root  "…LapSphere"          _NET_WM_WINDOW_TYPE_NORMAL, _NET_WM_STATE empty
child "LapSphere Panel"      _NET_WM_WINDOW_TYPE_UTILITY,
                             _NET_WM_STATE = SKIP_PAGER, SKIP_TASKBAR, ABOVE
```

The root is the existing GUI, unchanged in every respect that matters. The
child is created with `Context::show_viewport_immediate` and a
`ViewportBuilder::with_window_type(Utility)`; egui gives it its own native
window and its own GL surface. Measured on xfwm4 4.20.0 without a compositor,
this satisfies all four of the owner's goals simultaneously:

| Goal | Result |
| --- | --- |
| (a) out of the taskbar and window switcher | child is Utility → `SKIP_TASKBAR` + `SKIP_PAGER` present |
| (b) above a fullscreen game | on top through a fullscreen cube, focus return, Alt+Tab both ways, 5 min idle |
| (c) clickable | `CHILD-CLICK registered`; focus moved from the game to the panel |
| (d) the full-size GUI stays an ordinary window | root keeps `NORMAL` and empty `_NET_WM_STATE` |

Overhead measured: RSS flat at ~105–107 MB across the 5-minute run (the
single-window probes sat at the same level), CPU 1.6–4.1 %, and
`_NET_CLIENT_LIST` stable at 9 windows from start to finish — no window or GL
leak.

**The trade-offs, named:**

* A **second native window and a second GL surface**. This was the original
  objection to a second window in ADR-1 and it is real: one more window to
  track, and the WM sees two windows per application. Measured memory did
  not grow, but that is one host and one run.
* **The child must be explicitly shown/hidden** with the mode, rather than
  following the root automatically. Whether the child keeps painting while
  the root is hidden is `NOT TESTED` — `ViewportInfo::visible()` is derived
  from `minimized`/`occluded` (panel-spike §1б) and a child viewport has no
  measured guarantee here.
* **Tray interaction has to be re-pointed** at whichever window is live; the
  current tray maps `ShowWindow` to the root's `Visible(true)`, which is now
  the wrong window in panel mode. Not measured.
* Taskbar and Alt-Tab appearance is **`NOT TESTED`** on this host (no tasklist
  plugin). `docs/development/panel-owner-check.sh` exists so the owner can
  settle it by eye.

**What was tried and did not work** (all measured, all on xfwm4 4.20.0):

* **H1 — the Wine-exact ClientMessage** on a Normal window: refused at every
  sample from +5 ms to +10 s, across four variants (`l[3]` 0/1, second atom
  packed or not, both send masks). **But see the open bug below — this is a
  statement about our message, not about xfwm4.**
* **H2 — the withdrawn cycle**: `Visible(false)` really does withdraw (30
  `Withdrawn` samples at 50 ms) and re-show cleanly, no flash, position and
  focus preserved — but writing `_NET_WM_STATE` while withdrawn does not
  survive to the re-show.
* **H3 — `_KDE_NET_WM_STATE_SKIP_SWITCHER`**: no effect. A KDE-private atom;
  xfwm4 does not implement it.
* **H4 — `WM_TRANSIENT_FOR` to a hidden owner**: no effect; the window stayed
  in `_NET_CLIENT_LIST` and still took focus. Weakened by the owner window
  being `0` rather than a real 1×1 window (x11rb 0.13's `create_window` was
  not usable here).
* **H6 — `WM_CLASS`, `WM_HINTS`, `_MOTIF_WM_HINTS`**: no "skip taskbar" meaning
  for xfwm4. Multi-type `_NET_WM_WINDOW_TYPE` lists were not tested; egui
  exposes a single `X11WindowType`, not a list.

**Open bug — our ClientMessage is wrong, `wmctrl`'s is not.** `wmctrl 1.07`
turned out to be present on this host (it was absent and uninstallable in the
previous round; a package change made it available). Running it against the
same Normal window: `wmctrl -b add,_NET_WM_STATE_SKIP_TASKBAR` **works** —
present at +0.5 s, +2 s, +10 s, and removable — while our x11rb message on the
same window is refused. So **xfwm4 does honour a correct message on a Normal
window, and the earlier "xfwm4 refuses by type" conclusion was wrong**; what
is refused is our specific message, whose root cause is not yet identified.

This matters for the design: **a message-based solution may well exist**, which
would be simpler than H5 (no second window). It is not available today because
we cannot send the message. Before committing to H5's second window, the
cheaper option is to root-cause the message — and in production code the
immediate workaround is to shell out to the same call `wmctrl` makes, or to
copy its framing.

Dependency note: the raw X window id needs **no new runtime dependency** —
`eframe::Frame` implements `HasWindowHandle` (`epi.rs:717`) and
`Frame::window_handle()` yields a `RawWindowHandle::Xlib` whose `.window` is
the X id. `raw-window-handle 0.6` and `x11rb 0.13` are **dev-dependencies of
the throwaway probes only**. For anyone porting the probe: in x11rb 0.13 the
client data is `From<[u32; 5]>`, not `[u32; 8]`, and `ClientMessageEvent` has
no `event_mask` field — the mask belongs to `send_event`, which is why EWMH
messages go to the root window.


---

## Staged implementation plan

One commit each. No code in this PR.

1. **`request_repaint` on data arrival.** When a `HardwareUpdate` is
   consumed (or arrives), request an immediate repaint instead of waiting for
   the 500 ms timer. Improves the existing Statistics page too, and is the
   single change that makes a live panel usable. *Prerequisite: nothing.*
   Expected effect: avg latency 222 ms → low tens of ms.
2. **Move the channel drain into `logic()`** — *only if Q3 concludes the
   iconify case is in scope.* Otherwise skip. Per the spike this is a no-op
   for `Visible(false)`. *Prerequisite: Q3 answered.*
3. **Panel mode in `AppState` and settings.** Add the mode enum and the
   `show_*` config surface following the `statistics_sections` shape; persist
   and reload. No drawing yet. *Prerequisite: Q1 answered.*
4. **`pages/panel.rs` with a fixed layout and `—` placeholders.** Fixed
   widths per ADR-2, reserved slots, single fixed-height core strip. Draws
   with whatever data is in `AppState`. *Prerequisite: step 3.*
5. **Narrow the polled component set in panel mode.** Use
   `RefreshCoordinator::UpdateInterval` to park unselected components at a
   long interval (or add a pause/unregister). Never poll `logs` in panel
   mode. *Prerequisite: step 3; needs the pause mechanism.*
6. **Window switching, H5 shape:** the panel becomes a **second native
   window** via `Context::show_viewport_immediate` with
   `with_window_type(Utility)`; the root stays `Normal` and is reshaped in
   full mode. Mode switching shows/hides the child; the root keeps its own
   geometry. Plus mode and position persistence (X11) with clamping against
   monitor size (ADR-1) and a no-op-tolerant path for Wayland (ADR-3).
   Things this step must get right, all measured:
   * The **child** is `Utility`; the **root** is never re-typed (H5; B16
     showed the root cannot be re-typed at all).
   * Always-on-top is requested with
     `ViewportCommand::WindowLevel(AlwaysOnTop)` on the **first frame**,
     not in the builder — the builder hint is ignored by xfwm4 (B9/B14).
   * The child's own show/hide, and the tray's `ShowWindow`, must target the
     right window (`gui/src/app.rs:827` currently targets the root). **Both
     `NOT TESTED`.**
   * Before committing to the second window: spend one attempt on the
     message route, since `wmctrl` proves it works on xfwm4 4.20.0 and our
     message is refused for an unknown reason (Q8 open bug).
   *Prerequisite: steps 3-5.*
7. **README section** describing panel mode, the element list, and the
   Wayland caveats. *Prerequisite: step 6.*

Steps 1 and 2 are independent of the panel and could ship on their own.

**Step 0, revised:** the earlier version asked for a window-type decision up
front, because the type was permanent for the process (B16). Under H5 that
constraint is lifted for the *root*: the panel's type lives on the child
viewport, created and destroyed with the mode. What remains a one-time
decision is whether to use H5's second window at all, or first try to fix the
EWMH message — see Q8 and the open bug there.
Steps 3-6 are one feature; splitting them further would ship a mode that
draws but does not switch, or switches but does not poll.

---

## Explicit NOT TESTED list

Everything below is `NOT TESTED`, with the reason. All positive results in
this document are **xfwm4 4.20.0, X11, no compositor**
(`xprop -root _NET_WM_COMPOSITING` → *no such atom*).

**Other platforms — no evidence of any kind.**

* **Wayland.** No Wayland session exists on this host, and the owner's Wayland
  session does not start. Nothing about `X11WindowType`, the EWMH messages,
  child-viewport stacking, or click-through on Wayland has been observed.
* **GNOME / mutter, KDE / kwin, XWayland.** Absent from this host.
* **Any compositor** (mutter's compositor, kwin, xfwm4 with compositing
  enabled). A compositor would most likely change the always-on-top result
  (honouring the startup hint) and the transparency result (B8 shows black
  today). Enabling xfwm4 compositing needs a WM restart and there is no sudo.

**Behaviours not measured on xfwm4 either.**

* **Taskbar and Alt-Tab appearance**, for any window type. This session's
  XFCE has **no tasklist plugin loaded**, so no window button ever appeared
  and the alt-tab switcher window was not locatable by name. All conclusions
  are from EWMH state and `_NET_CLIENT_LIST` only, which is *not* equivalent —
  a window can carry `SKIP_TASKBAR` and still be listed. `wnckprop` is not
  installed; `libwnck-3.so.0` is present but has no headers, so no tool was
  built against it. `docs/development/panel-owner-check.sh` exists for the
  owner to settle this by eye with `wmctrl`.
* **H5 specifics**: whether the child keeps painting while the root is
  hidden; whether the child hides and shows cleanly with the mode; 300
  show/hide cycles with window-count and GL-resource tracking; tray behaviour
  with two windows.
* **H4** is weakened: the transient-for owner was `0` rather than a real 1x1
  window, because x11rb 0.13's `create_window` was not usable here.
* **Multi-atom `_NET_WM_WINDOW_TYPE`** (e.g. `[UTILITY, NORMAL]`) and atom
  ordering: not tested; egui exposes a single `X11WindowType`, not a list.
* **The root cause of our ClientMessage being refused while `wmctrl`'s is
  accepted.** Unresolved; see the open bug in Q8.
* From earlier rounds, still open: keyboard pass-through; hot-reload of font
  scale and item order; flicker as a subjective judgement; a real tray click;
  the 10-minute iconify soak; tray survival across a window recreate.
* **Runtime-PM on a discrete GPU** was measured only on this host's NVIDIA
  3070 Laptop GPU under one compositing-less X11 session.

## What is deliberately not decided here

* Whether the panel can be dragged by any part of its surface or only by a
  dedicated handle (interacts with Q2 and with ADR-3's position story).
* Whether multiple panels are ever wanted (out of scope; would change ADR-1).
* The exact element→width table (blocked on Q1).
* Whether panel mode should be startable from the CLI (`--panel`) alongside
  the existing `--tray`.
