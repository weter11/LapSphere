# Mini-panel design (gkrellm-style compact mode)

Status: **the panel is implemented** on branch `feat/panel-ui` (PR D). The
design decisions below are recorded as they were reached; sections that the owner
has since decided differently are marked **superseded** and state the decision
that replaced them.

This document moved here from the `feat/mini-panel` branch (commit d071f24) when
that PR was superseded by the panel implementation PR. The Vulkan layer spike
under `spike/` and the probe binaries under `gui/examples/` stayed on the
original branch: the spike moves to its own branch for PR C.
Companion evidence documents: [`panel-spike.md`](./panel-spike.md) (frame
delivery, `ViewportCommand` behaviour),
[`panel-overlay-tests.md`](./panel-overlay-tests.md) (live overlay behaviour
over another window, B1–B8), and
[`frametime-layer-spike.md`](./frametime-layer-spike.md) (the in-game Vulkan
implicit layer that supplies fps, F22).

Base: the design was drafted against `5c57bc7`; the implementation is based on
`be77233` (`origin/lapsphere` with PRs #112 and #113 merged).
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

### Goal: fps and frame-time are elements of the panel — reserved now, filled later

**Superseded in scope, kept as intent.** An earlier draft of this document
decided that `fps`, `frametime_ms` and `frametime_graph` would carry real data
from the first release. That is narrowed: in this release the three frame
elements are **reserved slots with a data receiver and no producer** — `fps`
renders `—`, the graphs are empty fixed-size strips over an allocated 60-second
ring buffer. The Vulkan layer (PR C) and the fps source (PR E) are separate PRs
and are explicitly out of scope here.

The intent is unchanged and the reason the slots exist at all: the panel is a
MangoHud replacement, and that means it eventually carries frame data. Reserving
the slots now means turning the data on does not move the window.

**The technical consequence, and why it forces an in-process layer.** An
external process **cannot see the game's present**. Every host-side counter is
either invisible to the game (`/proc` gives CPU time per thread, not frames)
or too coarse (a compositor's damage count, a DRM vblank counter, `nvidia-smi`
samples). The panel therefore needs **a layer inside the game process**:

* a **Vulkan implicit layer** (Rust `cdylib`), which the loader inserts into
  every Vulkan app that opts in via an environment variable;
* it timestamps `vkQueuePresentKHR` — the one call that marks a frame boundary
  — and writes the intervals into **shared memory**;
* the panel (an external process) reads that shared memory and computes the
  statistics.

`docs/development/frametime-layer-spike.md` is the feasibility spike for that
layer, with its go/no-go criteria and measured results.

### Non-goals of the first release: frame measurement

Stated so that the scope is not silently widened later:

* **No OpenGL hook.** GLX/EGL interception would be a second, separate layer
  (GLX extension or `LD_PRELOAD`) with its own dispatch chain. The first
  release is **Vulkan only**; a GL game shows `—` in every frame element.
* **No 32-bit games.** The shipped layer will be a 64-bit `cdylib`. A 32-bit
  Vulkan app runs in a 32-bit process and cannot load it. Shm protocol v1 is
  architecture-neutral so a 32-bit build is *possible* later, but nothing in
  the first release provides one.
* **No Wayland-specific layer work.** The layer is an **API-level** mechanism,
  not a protocol-level one: implicit Vulkan layers are loaded by the Vulkan
  loader and are indifferent to whether the surface is X11, Wayland or DRM.
  So the layer itself has **no Wayland-specific code** — but every Wayland
  *claim about it is `NOT TESTED`*, because this host has no Wayland session
  (`XDG_SESSION_TYPE` is `x11` on every run here). What that means
  specifically: the loader path, the intercept, the shm write, and the
  MangoHud comparison have all been exercised only on X11. Wayland-specific
  frame pacing (presentation timing, explicit sync, `wp_presentation`) changes
  *what a present means*, and whether present timestamps still correspond to
  displayed frames is `NOT TESTED`.
* **No anti-cheat interaction.** Anti-cheat suites commonly enumerate loaded
  layers and shared objects. Nothing here has been tested against one, and
  no statement about any is made. See the spike's `NOT TESTED` list.
* **No DXVK/Proton coverage.** Untested, and untestable here without the
  games in question — the spike ran against `vkcube`.

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

### ADR-1 amendment (B9–B21) — one window, with an EWMH message; the second window is the fallback

**The core decision is unchanged and is now the cheaper one: mode switching
is in-process, on one long-lived window.** No second native window, no
second GL surface, no tray re-targeting.

**The change of plan, and why.** Three rounds have moved here, and the
reasons are worth recording because the recommendation flipped twice:

| Round | Recommendation | What changed it |
| --- | --- | --- |
| B9–B16 | type the **root** `Utility` for the whole process | the main GUI would be a `Utility` window for life — a cost we did not want |
| H1–H5 | a **second native window** (Immediate child, `Utility`), root stays `Normal` | works, but brings a second GL surface and its own lifecycle |
| **B18** | **one window**, root stays `Normal`, hide from the taskbar with an EWMH message | the cause of the earlier refusals turned out to be ours |

**The design, concretely.**

* **Root window**: `Normal`, reshaped in place by `ViewportCommand`, as
  originally intended.
* **Entering panel mode**: `ViewportCommand::WindowLevel(AlwaysOnTop)` —
  the **runtime** command, not the builder hint, which xfwm4 ignores (B9) —
  plus an EWMH `_NET_WM_STATE` ClientMessage adding `SKIP_TASKBAR` and
  `SKIP_PAGER`. The message is **event-driven and self-confirming**, not
  delayed: wait for the WM to accept the window, send, read the property
  back from the X server, retry while the atoms are absent.
* **Leaving panel mode**: REMOVE of both atoms, `WindowLevel(Normal)`.

**The numbers behind the recommendation** (xfwm4 4.20.0, no compositor):

| | H5, second window | B18b, single window |
| --- | --- | --- |
| native windows / GL surfaces | 2 / 2 | **1 / 1** |
| main window type | `Utility` (or a child) | **`Normal`, unchanged** |
| tray target | must change | unchanged |
| fullscreen stacking | measured: on top for 5 min idle | each half proven (B9 level, B18b atoms); **the combination is `NOT TESTED`** |
| RSS over 20 switches | flat (H5 run) | **flat: 107 004 kB, window count constant at 5** |
| reliability | proven over 5 min idle + Alt+Tab + focus return | 3/3 trials, stable at +0.5…+8 s; 20 switches |
| extra risk | second window's lifecycle, GL, focus, taskbar | WM-specific message; needs retry logic |

The single window is recommended because it removes three whole categories of
cost, and the one thing it does not have measured — the combined stacking
behaviour — is a measurement, not a redesign.

**Implementation requirements this implies** (the app is not changed in this
PR):

1. The X window id is available without a new dependency: `eframe::Frame`
   implements `HasWindowHandle` (`epi.rs:717`) and `Frame::window_handle()`
   gives a `RawWindowHandle::Xlib`. Production code would add `x11rb` to
   `gui/Cargo.toml` — it is already in `Cargo.lock` at 0.13.2 via `winit` and
   `arboard`; the probes use it as a dev-dependency.
2. **No fixed delay.** Wait until the WM has accepted the window (id appears
   in `_NET_CLIENT_LIST`, or the window is mapped), then send, then read
   `_NET_WM_STATE` back from the X server and retry if the atoms are absent.
   Measured: frame 0 and 50 ms fail, 100–300 ms work — but even at 82 ms,
   *after* acceptance, the first send was ignored. The retry is what makes
   it reliable, not the timing.
3. **Atom values are unremarkable here**: Wine's documented `data.l[3]=1`
   works (6/6 with a delay); so does `0`. The earlier claim that `l[3]`
   "must be 0" came from a confounded experiment and has been withdrawn.
4. The always-on-top level must be requested at runtime, on the first frame.
   `ViewportBuilder::with_window_level` is ignored.

**The fallback: H5, the second window.** H5 is fully measured and works —
the child viewport is `Utility` and keeps `SKIP_TASKBAR` + `SKIP_PAGER` from
the **window type alone, with no message at all**. That is its real
advantage: it depends on a WM honouring `_NET_WM_WINDOW_TYPE`, not on
accepting a particular EWMH request at a particular moment. If the message
route proves unreliable on a WM we cannot test — which is exactly the risk,
since every message result here is xfwm4-specific — H5 is the design to
fall back to. Its costs are the ones tabulated above: a second native
window, a second GL surface, and a separate show/hide lifecycle.

Recommendation: implement the single window, keep the H5 path available as a
fallback, and do not remove either until the panel has run on more than one
window manager.

**Wayland and other platforms — the limits of this design, stated plainly.**

* **Wayland: the whole message mechanism does not exist.** `_NET_WM_STATE` is
  an X11 protocol; there is no Wayland equivalent that a client may send to
  the compositor, and Wayland clients cannot set `_NET_WM_STATE` at all. So
  on Wayland the panel **cannot** be removed from the switcher by the app —
  only by `xdg-toplevel` hints whose effect is compositor-specific, and that
  path is `NOT TESTED` here (this host has no Wayland session and the owner's
  does not start).
* **Window position** is compositor-owned on Wayland: eframe documents
  `ViewportInfo.outer_rect` as always `None` there, so a saved position is a
  hint at best. The panel must not depend on it (ADR-3 already says this).
* **`override_redirect`, `WindowLevel` and click-through** are all X11- or
  WM-specific; their Wayland behaviour is `NOT TESTED`.
* **The fallback is worse on Wayland, not better**: H5 relies on
  `_NET_WM_WINDOW_TYPE`, which is also X11-only. So on Wayland neither
  route is verified, and the honest position is that panel mode there is
  best-effort and may simply appear in the compositor's window list.
* **Other X11 WMs (mutter, kwin, GNOME, KDE): `NOT TESTED`.** Everything in
  this amendment is xfwm4 4.20.0 with the compositor off. The single-window
  design is deliberately built to *detect* that: the send is confirmed by
  reading the property back and retried, so a WM that ignores the request
  shows up as "atoms absent after N attempts" rather than as a silent
  failure — at which point H5 is the fallback. What is genuinely unknown for
  other WMs is whether the *retry* converges there.

**Always-on-top, unchanged throughout.** B9, B14 and H5 all agree: send
`WindowLevel(AlwaysOnTop)` once, after the window exists. It held
indefinitely — 5 minutes idle, focus return, Alt+Tab — with no
re-assertion, which would have been a busy-wait running against a game.

**What remains `NOT TESTED`.** All of this is **xfwm4 4.20.0 with the
compositor OFF**. B21 could not obtain a compositing session at all: Xvfb
offers the Composite extension but xfwm4 refuses it with
`Unsupported GL renderer (llvmpipe)`, and the live session's setting only
takes effect at the next WM start. A compositor would most likely change
both the always-on-top result and the fps result — without one, a
fullscreen window is unredirected, which is the easy case for an overlay.
Whether mutter, kwin, GNOME or KDE honour `data.l[3]=0`, and with what
timing, is `NOT TESTED`. Commands for the owner are in
`docs/development/tools/README.md`.

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
| 7 | The panel must stay visible **above a borderless-fullscreen game** — the primary use case. | **SOLVED on xfwm4 4.20.0, compositor OFF (B18b combined).** One `Normal` window, runtime `WindowLevel(AlwaysOnTop)` + SKIP_TASKBAR/SKIP_PAGER together, stayed `PANEL-ON-TOP` against a real 2560x1440 fullscreen cube through focus return, a click on the cube, Alt+Tab both ways, a click on the panel (which focused it), and 5 min idle — then 300 mode switches with RSS and window count unchanged. (Earlier detail, B9/B14: (fullscreen cube, focus return, click on the cube, Alt+Tab both ways, 5 min idle — all `PANEL-ON-TOP`; 0.3% CPU). Also established in B9: With a genuine fullscreen cube (2560x1440@0,0) the panel stayed on top through focus return to the cube, a click on the cube, Alt+Tab away and back, and **5 minutes idle** — with a single runtime `ViewportCommand::WindowLevel(AlwaysOnTop)` and **no re-assertion**. B2's failure was entirely the *startup* path: `with_window_level()` is ignored, the runtime command is honoured. |
| 8 | The panel must not appear in the taskbar / window switcher. | **SOLVED on xfwm4 4.20.0 by B18 — a single window, message sent after it settles.** ADD of `SKIP_TASKBAR`+`SKIP_PAGER` via an EWMH `ClientMessage` with `data.l[3]=0` is honoured on a **Normal**-type window: 3/3 trials, stable at +0.5…+8 s; 20 alternating switches with constant window count and RSS. The earlier "refused by type" conclusion was our own bug. Control that mattered: `wmctrl` on the same window, not a Utility window. The **child** viewport is `Utility`, so xfwm4 grants `SKIP_TASKBAR`+`SKIP_PAGER` (measured); the **root** stays `_NET_WM_WINDOW_TYPE_NORMAL` with empty `_NET_WM_STATE`, so the main window is an ordinary window. Also measured: `_KDE_NET_WM_STATE_SKIP_SWITCHER` and `WM_TRANSIENT_FOR` have no effect (H3, H4), and the withdrawn-cycle trick does not help (H2). **Caveat: B13/H1's message-based route failed in *our* implementation while `wmctrl` succeeded — see the open bug below; a message-based solution may still exist.** |

Criteria 9–13 cover the frame-measurement layer (owner decision: fps in the
first release). Status column: what the spike in
[`frametime-layer-spike.md`](./frametime-layer-spike.md) measured, on this
host, release build.

| # | Criterion | Why it is a criterion | Status |
| --- | --- | --- | --- |
| 9 | **Layer overhead on present is small and measured.** The cost added to a frame by the layer: **median and p99 in microseconds**, taken as the wall time of the `vkQueuePresentKHR` hook itself (clock around the whole hook body, layer instrumentation versus a layer-loaded-but-pass-through build). | An overlay that costs the game frames is not a replacement for MangoHud. Median is the per-frame cost; p99 is the cost that shows up as a spike. | **Measured in the spike** — see `frametime-layer-spike.md` §3 for median/p99 and the 3-repeat requirement. Both are microsecond figures from a release build of the layer on `vkcube`; they do not transfer to a heavy title without re-measuring. |
| 10 | **Fail-open.** *Any* layer failure — shm cannot be created, the ring is full, a write faults, the exe name is unavailable — must have **no effect on the game**. The layer never changes a return value, never aborts, never panics across FFI (`catch_unwind` on every hook entry), and never blocks: if the segment cannot be written, that frame is dropped from the record and the game proceeds. | This is the one criterion that matters more than overhead. A crash or a stall in the measurement path would turn a diagnostic into the fault. | **Measured in the spike** — the fail-open cases (unwritable runtime dir, no reader, two writers, `kill -9` stale segment) were exercised deliberately. NOT TESTED: failure modes under an anti-cheat or a driver that aborts on an unexpected layer. |
| 11 | **The layer is inactive without its environment variable.** With `LAPSPHERE_FRAMES` unset, the layer is loaded but writes nothing, creates no segment, and adds no measurable overhead. Activation is opt-in only, so no existing app changes behaviour. | A layer that activates itself would silently change behaviour for every Vulkan program on the host, including the user's games, with no way to attribute a regression. | **Measured in the spike** — §1: the loader enumerates the layer with and without the variable, and the no-variable run produces no `frames-<pid>` segment and an unchanged frame-time distribution. What this does **not** prove: that the loader's layer *ordering* is identical in a real game that has other implicit layers (MangoHud, `VK_LAYER_MESA_device_select`, NVIDIA layers all present here). |
| 12 | **Values agree with MangoHud on the same run.** Running the layer and MangoHud against the same workload in the same session: **median frametime within 1 %**, **p99 within the measured run-to-run noise** of MangoHud against itself. | Otherwise the panel's fps is a second, differently-wrong number, and the user has no way to tell which to believe. | **Measured in the spike** — §2, layer-vs-MangoHud median/p99/p99.9, with MangoHud-vs-MangoHud repeats establishing the noise floor that p99 must sit inside. Median within 1 % is the hard gate; p99 is gated on noise, not on an absolute number, because p99 of a 60 fps idle-vsync workload is dominated by scheduling jitter. What this does **not** prove: agreement on a *GPU-bound* title, where present-to-present time includes GPU work the layer's CPU-side timestamp also includes — the spike workload (`vkcube`) is not GPU-bound. |
| 13 | **Statistics are defined once, by formula, in one place.** `1% low`, `0.1% low`, `max` and the count of frames over 33 ms are computed **exactly** as `docs/development/tools/frametime_stats.py` defines them: `low_percent()` for the worst-N-average definition, nearest-rank `percentile()` for p-values, and `over_33ms = count(interval > 33.3 ms)`. `1% low` is reported **both ways** (A: `1000/p99`; B: mean of the worst 1 % inverted), as that tool documents, because the two diverge on a mixed tail. | "1 % low" is used inconsistently across the ecosystem; a panel that silently picks one definition is not comparable to the overlays it replaces. Re-deriving it in Rust would create a second, subtly different definition. | **BY CONSTRUCTION, not yet verified in Rust** — the Rust reader must reuse these definitions exactly; the spike verifies agreement by **feeding the layer's raw intervals through `frametime_stats.py`**, not by comparing two independent implementations. NOT TESTED: a unit-test harness proving the Rust port matches `frametime_stats.py` on adversarial inputs (the tool's own `--self-test` covers the Python side only). That parity test is a build-time prerequisite for shipping the Rust reader. |

**The GO gate for the layer as a whole** is: criteria 1–4 of the spike's test
list (loads/inactive-without-var, distribution matches MangoHud, overhead
below 10 µs median, survives exit / `kill -9` / resize / two instances / no
reader), with no influence on the game on any failure. The spike's verdict and
the fallback if it is not met are in
[`frametime-layer-spike.md`](./frametime-layer-spike.md).

---

### ADR-5 — Frame data travels through a versioned shared-memory segment, one writer, lock-free readers

**Decision.** The Vulkan layer writes frame intervals into a POSIX shared
memory object; the GUI process reads it. The protocol below is **v1** and is
frozen — a later protocol gets a new `version` in the header, not a
reinterpretation of this one.

**Layout.** One file per game process:

```
$XDG_RUNTIME_DIR/lapsphere/frames-<pid>
```

`$XDG_RUNTIME_DIR` is the right root: it is `0700` and per-user by
specification, so the segment is unreachable to other users without an
additional permission decision, and it is emptied on logout by convention.
The directory `lapsphere/` is created by the writer with mode `0700`; the
segment file itself is **mode `0600`**, and it is created with
`O_CREAT|O_EXCL` so a pre-existing file can never be adopted or truncated by
a different process. The spike used the mmap-backed path
(`/dev/shm`-equivalent via `XDG_RUNTIME_DIR`) rather than a SysV segment,
because the file name carries the pid and the header carries everything a
reader needs — no `shmget` key collision space to get wrong.

**Header** (fixed 128 bytes, at offset 0; all little-endian, matching the
x86-64 and aarch64 targets):

| offset | size | field | notes |
| --- | --- | --- | --- |
| 0 | 8 | `magic` | `"LSPFRS01"`; a reader that does not match this closes the file without reading further |
| 8 | 4 | `version` | `1`; a reader refuses a version it does not know |
| 12 | 4 | `header_len` | `128`, so a future header can grow without moving the ring |
| 16 | 4 | `pid` | the writing process |
| 20 | 4 | `api` | `1` = Vulkan, `2` = reserved (GL), so a future protocol can label its source |
| 24 | 8 | `exe_name[64]` | NUL-padded, `/proc/self/comm` — what `game_name` shows |
| 88 | 4 | `ring_capacity` | number of `u64` slots (see below) |
| 92 | 4 | `flags` | bit 0: writer alive (heartbeat maintained) |
| 96 | 8 | `write_seq` | seqlock sequence — see below |
| 104 | 8 | `last_present_ns` | `CLOCK_MONOTONIC` nanoseconds of the most recent present; doubles as the heartbeat |
| 112 | 8 | `frame_count` | total presents observed, monotonic |
| 120 | 8 | `dropped_count` | presents the layer chose not to record (see "fail-open") |

**Ring buffer of intervals.** Immediately after the header, `ring_capacity`
slots of `u64`, each a **frame interval in nanoseconds** — not a timestamp, so
a reader needs no reference point and cannot be fooled by a clock change.
Capacity is fixed at creation (the spike used 4096 slots = 32 kB, which is
~68 s of history at 60 fps and ~17 s at 240 fps; the reader's window is a
configurable *subset* of the ring, which is what makes a fixed size possible).
A single frame's interval is the delta between consecutive presents. The
first present after activation establishes the origin and writes nothing, so
there is no fabricated interval for the gap between process start and first
frame.

**One writer, lock-free readers — a seqlock.** Only the game process writes;
readers never write and never block, and the writer never waits for a reader:

1. writer: `seq` += 1 (now odd — "write in progress"), as a release store;
2. writer: write the new interval at slot `count % capacity`, update
   `frame_count`, `last_present_ns`;
3. writer: `seq` += 1 (now even), release store;
4. reader: read `seq` (acquire); if odd, a write is in progress — retry or
   skip; copy the slots; read `seq` again; if it changed, discard the copy.

The alternative considered and rejected: an atomic free-running index with no
seqlock. It is marginally cheaper per frame, but a reader can then observe a
torn frame — an interval from one slot and a `frame_count` from another — and
a 0.1 % low computed over torn frames is a wrong number shown to the user. The
seqlock costs one atomic increment per frame and makes the reader's failure
mode "one stale sample", which is harmless. **Chosen.**

The writer's stores are relaxed atomics on the two `seq` words and plain
stores for the payload; there is no mutex and no syscall on the present path
after the segment exists. `last_present_ns` is a `CLOCK_MONOTONIC` read
(1 vDSO call, no syscall) and also serves as the liveness signal — see
"choosing the active process".

**Choosing the active process (the rule proposed here).** A panel can see
several segments at once (a launcher plus a game, two games, a leftover from a
crash), so it needs one rule. Proposed:

> **The active process is the segment with the most recent `last_present_ns`,
> provided that timestamp is younger than `STALE_AFTER = 2 s`. If no segment
> qualifies, the frame elements show `—`. Ties are broken by the larger pid.**

Why "most recent present" and not "most recently created" or "largest ring":
the element must reflect the process that is *actually presenting frames*, and
only the present timestamp distinguishes a running game from a live-but-idle
launcher. Why a timeout rather than trusting `flags`: a `kill -9`'d game
leaves a segment that still says "alive", and without the timeout the panel
would show a frozen frame time from a dead process indefinitely. 2 s is
chosen because it is several times the longest plausible gap between presents
in a running game (a 2 s gap *is* a 0.5 fps stall worth showing as `—` rather
than as a stale number) while being short enough that the panel reacts to a
game exit quickly. The cost of this rule: it is checked at the panel's refresh
cadence, not continuously, so the panel can show a number up to one refresh
interval after the game exits. That is acceptable — the next refresh shows
`—`.

**Cleaning up stale segments.** Two layers, both best-effort:

* **The writer removes its own segment** at `vkDestroyInstance`, and installs
  no signal handlers — it does not need to: the timeout rule already makes an
  orphan harmless, and a signal handler in a game process is a behaviour
  change (criterion 11) that the layer has no business making.
* **The reader prunes orphans.** Before choosing an active segment it may
  `unlink` any `frames-<pid>` whose pid does not exist (`kill(pid, 0)` →
  `ESRCH`) or whose timestamp is older than a much larger bound
  (`ORPHAN_AFTER = 300 s`). `kill(pid,0)` is advisory only: a recycled pid
  cannot cause a wrong deletion because the timestamp bound is checked too,
  and the worst case of a wrong decision is one missing segment, never a
  crash.

So an orphan is invisible within 2 s (by the timeout rule) and removed within
one panel session or on the next logout (by pruning). Nothing blocks on it
either way.

**`dropped_count`, and what fail-open means on the write path.** The present
hook is: read the clock, call the next function, write the interval. If the
write cannot happen — segment vanished (reader pruned it, or the runtime dir
was cleaned), mapping lost, header magic no longer matches (another writer) —
the frame is counted in `dropped_count` and the game is untouched. The layer
does **not** try to re-create the segment on the hot path: a failed
`mmap`/`open` in the hot path is the thing most likely to cost a frame, and
losing samples is preferable to costing frames. Re-creation is attempted
once, on the swapchain-create path, which is not hot.

**Why not the alternatives.**

* *A D-Bus method from the game process.* The game would have to link our
  library and speak D-Bus; that couples the layer to zbus inside the game's
  process and puts a bus round trip on the present path.
* *A file the game appends to.* `write(2)` per frame is a syscall per frame
  and, more importantly, an unbounded-growth file; the ring gives bounded
  memory with the same information.
* *The panel `ptrace`s the game.* Root-only, and it perturbs the game —
  directly contradicting criterion 10.
* *MangoHud's own output as the source.* Viable fallback, and the least-bad
  one if the layer fails; its drawbacks are in
  [`frametime-layer-spike.md`](./frametime-layer-spike.md) §7 (it requires
  MangoHud to be running, gives no low-percentile window control, and its log
  is a file the panel must tail with a delay).

**NOT TESTED on this design:** 32-bit processes (a `u64` header is
architecture-neutral in the layout but the cdylib is not loadable there),
Wayland presentation timing, and whether `CLOCK_MONOTONIC` in the game process
and in the panel agree — they need not, which is why the ring stores
*intervals*, and only `last_present_ns` crosses processes (used solely for
staleness comparison, where a skew between the two processes' monotonic clocks
does not matter).

---

## Config schema

**Owner decision: the panel's settings live in `~/.config/lapsphere/panel.json`,
NOT in `settings.json`.** This supersedes the earlier proposal below, which put
the panel in `settings.json`. Two reasons, both from the owner:

* `settings.json` is what the Settings page and the tray own. The panel is a
  separate surface with a separate lifecycle, and a corrupt or hand-edited panel
  file must not be able to take the tray's settings down with it.
* A panel release has to be revertible without touching tray behaviour.

**Tray settings are moving to their own `~/.config/lapsphere/tray.json` in a
separate PR (F).** The panel reads the tray's enabled state through one named
accessor, `LapSphereApp::tray_enabled(&AppState)`, precisely so PR F can change
where that state is stored without touching a single panel call site.

**Owner decision: the default hotkey is `Shift_R+F9`, not F10.** F10 is the
"activate the menu bar" key in the X11 default keymap, so grabbing it fights
the toolkit's own binding on some desktops.

The implemented shape:

```jsonc
// ~/.config/lapsphere/panel.json
{
  "version": 1,
  "active": false,             // start in panel mode
  "font_scale": 1.0,           // global scale for every element
  "always_on_top": true,
  "click_through": false,
  "hotkey": "Shift_R+F9",      // empty string means "no hotkey"
  "position": {                 // a corner, not an absolute point
    "corner": "top_left",      // top_left | top_right | bottom_left | bottom_right
    "offset_x": 24.0,
    "offset_y": 24.0
  },
  "items": [                    // ARRAY ORDER IS THE DISPLAY ORDER
    { "id": "cpu_name",  "visible": true, "label": null, "font_scale": null },
    { "id": "cpu_load",  "visible": true, "label": null, "font_scale": null }
    // ... see the element table for the full set
  ]
}
```

`label: null` means "use the built-in default"; a string overrides it.
`font_scale: null` means "inherit the global scale". Position is a **corner plus
an offset from it**, not an absolute point: a saved absolute position from a
different monitor resolution would place the panel off-screen, and the panel must
never be unplaceable.

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
| `cpu_name` | `GetCpuInfo` | `CpuInfo::name` | — | 150 |
| `cpu_freq` | `GetCpuInfo` | `CpuInfo::average_frequency` (`u64` MHz) | MHz/GHz | 92 |
| `cpu_load` | `GetCpuInfo` | `CpuInfo::average_load` (`f32`) | % | 84 |
| `gpu_name` | `GetGpuInfo` | `GpuInfo::name` (first GPU) | — | 170 |
| `gpu_clock` | `GetGpuInfo` | `GpuInfo::frequency` (`Option<u64>`) | MHz/GHz | 92 |
| `gpu_load` | `GetGpuInfo` | `GpuInfo::load` (`Option<f32>`) | % | 84 |
| `gpu_mem_clock` | `GetGpuInfo` | `GpuInfo::memory_frequency` (`Option<u64>`) | MHz/GHz | 92 |
| `gpu_mem_usage` | `GetGpuInfo` | `GpuInfo::vram_memory` → `GpuMemorySnapshot::{used_mib,total_mib}` | MiB | 118 |
| `ram_usage` | `GetMemoryInfo` | `MemoryInfo::used_percent` (`f32`) | % | 96 |
| `ram_type` | `GetMemoryInfo` | `MemoryInfo::memory_type` (`Option<String>`) | — | 104 |
| `ram_freq` | `GetMemoryInfo` | `MemoryInfo::memory_frequency` (`Option<u64>`) | MHz/GHz | 92 |
| `gamepad` | `GetGamepadInfo` | `GamepadInfo::name` + `battery_level` (`Option<u8>`) | % | 150 |
| `fps` | **no source in the first release** | — | — | 78 |
| `fps_graph` | **no source in the first release** | — | — | 120 |
| `frametime_graph` | **no source in the first release** | — | — | 120 |

**This is the owner's element list, exactly.** It supersedes the wider table the
earlier draft proposed (which included `hostname`, `cpu_temp`, `cpu_power`,
`gpu_temp`, `gpu_power`, `battery`, `wifi_signal`, `fans`, `cpu_core_chart`,
`game_name`, `frametime_ms`, `low_1pct`, `low_01pct` and separate
`gamepad_1`/`gamepad_2`). Those were not implemented and are not planned for
this release; if any is wanted later it is a new decision, not a default.

Every field above was checked against `common/src/types.rs` and exists —
including `GpuMemorySnapshot`, which carries `used_mib`/`total_mib`/`free_mib`,
so `gpu_mem_usage` is a real reading rather than a placeholder.

**The three frame elements are reserved slots with a data receiver and no
producer.** The Vulkan layer is PR C and the fps source is PR E; both are out of
scope for this release. `fps` renders `—`, and the two graphs are one fixed-size
strip each fed by a 60-second ring buffer that is allocated and empty. The slot
is already reserved at full size, so enabling the data later does not move the
window (ADR-2 applied to the frame rows).

Notes on the table, all checked in source rather than assumed:

* **The six frame rows are not D-Bus data, and deliberately so.** Every other
  row comes from a cache-backed daemon getter (ADR-4). The frame rows cannot:
  the data exists only inside the game process, in the shm segment the Vulkan
  layer wrote. So **the GUI process reads `frames-<pid>` itself** — no daemon
  method, no new interface, and therefore no versioned D-Bus change. The
  consequence to record: the panel's frame elements depend on
  `$XDG_RUNTIME_DIR/lapsphere/` being readable by the GUI's user, which it is
  by construction (both run as the same user, mode 0600, see protocol v1).
* **Absent source renders `—`, in an already-reserved slot.** With no layer
  active (env var unset), no Vulkan process (an OpenGL or 32-bit game), or no
  reader access, all six rows show `—` and the window keeps its size. This is
  ADR-2 applied to the frame rows: the width table above is fixed regardless
  of whether frame data ever arrives.
* **`frametime_graph` is one fixed-height strip**, exactly like
  `cpu_core_chart`. The number of samples in the window changes with the
  statistics window (see Q9) and with the refresh rate; a strip whose height
  tracked that would resize the window.

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

**Q2 — RESOLVED by owner decision: the hide-safety rule.** Closing the panel
window when the tray is enabled still hides to tray, as before. But **hiding is
only permitted when a way back exists**, and with no way back the request is
converted into "return to the normal window":

> Hiding the panel is allowed only when at least one of these holds:
> the hotkey was captured (`XGrabKey` succeeded), the tray is enabled, or the
> `lapsphere --toggle-panel` CLI works (which needs the D-Bus method). With none
> of them, a hide request returns to the normal window instead, with a message
> saying why.

The reason is that hiding is otherwise a one-way trip: the user is left looking
at an overlay they cannot remove. Returning to normal mode is the only safe
reading of "stop being a panel" in that state. Implemented as the pure functions
`panel::menu::can_hide` / `hide_outcome`, with a test per path.

The original open question was:
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

**Q8 — RESOLVED on xfwm4 4.20.0, and the answer is now the cheap one: a
single window, with an EWMH message sent after the window has settled.**

This supersedes both earlier answers. B18 traced the two clients byte for
byte and found the cause.

**The root cause: one thing only — timing.**

Decoding both clients' SendEvent requests from `strace -x` on the X socket
showed they are byte-identical except `data.l[3]` (opcode 25,
`propagate=0`, destination = root, mask `0x00180000`, window = target,
format 32, one atom per message, `l[0]=1`, `l[1]` the same atom `430`).
That difference looked like the cause and **was not**. The original
experiment was confounded — the `l[3]=1` runs sent on frame 0 and the
`l[3]=0` runs used a delay, so two variables moved together. A control
settled it: **`l[3]=1` with a 200/300 ms delay works, 3 repeats each**
(6/6). Wine's documented value of 1 is correct and `l[3]` is irrelevant.

What actually matters is when the message arrives:

| send | result |
| --- | --- |
| on frame 0 | fails (3/3) |
| 50 ms | fails |
| 100 / 150 / 200 / 300 ms | works (200 ms 3/3) |

And even "the WM has accepted the window" is not the same moment as "the WM
will act on a SubstructureRedirect for it": in the event-driven run the
window was already in `_NET_CLIENT_LIST` at t+82 ms and the **first** send
was still ignored; the second, ~1.1 s later, worked.

So the requirement is not a delay at all: **wait for acceptance, send, read
`_NET_WM_STATE` back from the X server, retry if the atoms are absent.**

So the design is back to a single window:

* **enter panel mode** — `ViewportCommand::WindowLevel(AlwaysOnTop)` (B9: the
  runtime command works, the builder hint is ignored) plus ADD of
  `SKIP_TASKBAR` and `SKIP_PAGER`, sent once the window has settled.
* **leave panel mode** — REMOVE of both, and `WindowLevel(Normal)`.

**B18b measured it** (3 fresh-window trials, atoms sampled at +0.5/1/2/4/8 s;
then 20 alternating switches): the atoms appeared and stayed at every sample
in 3/3 trials; `_NET_CLIENT_LIST` constant at 5 windows and **RSS constant at
107 004 kB** across the switches — no window or memory growth.

**What H5 taught us that still applies.** The child-viewport approach also
works (H5: 3/3, on top of a fullscreen cube through 5 min idle, clickable,
RSS flat). It is no longer *needed*, because the message route removes the
reason it existed. It stays in the record as the fallback if the message
route proves unreliable on a WM we cannot test.

**Open, and the first thing to measure when the panel work starts:** the
**combination** — one window that is simultaneously always-on-top and
skip-taskbar, above a fullscreen cube. The two halves are each proven
(B9 for the runtime level, B18b for the atoms); their combination over a
fullscreen game is `NOT TESTED`. Everything else about the single-window
route is already measured.

**Caveat that applies to any WM.** The `l[3]=0` and the settle-timing
findings are **xfwm4 4.20.0 observations**. Another WM may accept `l[3]=1`,
or require a different delay. The implementation should therefore treat the
message as best-effort: send it, read back `_NET_WM_STATE` after a short
delay, and retry once if the atom is absent. That makes the design
self-correcting on WMs where our specific request is not honoured, and costs
nothing where it is.

**Other routes, all measured and all rejected on xfwm4 4.20.0:**

* **H2** — withdrawn cycle: `Visible(false)` genuinely withdraws (30
  `Withdrawn` samples at 50 ms) and re-shows cleanly, no flash, position
  preserved — but atoms written while withdrawn do not survive to the re-show.
* **H3** — `_KDE_NET_WM_STATE_SKIP_SWITCHER`: no effect (KDE-private atom).
* **H4** — `WM_TRANSIENT_FOR` to a hidden owner: no effect; still in
  `_NET_CLIENT_LIST`, still took focus. Weakened by the owner being `0`
  rather than a real 1x1 window.
* **H6** — `WM_CLASS`, `WM_HINTS`, `_MOTIF_WM_HINTS`: no skip-taskbar
  meaning. Multi-atom `_NET_WM_WINDOW_TYPE` lists not tested; egui exposes a
  single `X11WindowType`.

**Dependency note.** The raw X window id needs **no new runtime dependency**:
`eframe::Frame` implements `HasWindowHandle` (`epi.rs:717`) and
`Frame::window_handle()` yields a `RawWindowHandle::Xlib` whose `.window` is
the X id. For production, add `x11rb` to `gui/Cargo.toml` (it is already in
`Cargo.lock` at 0.13.2, via `winit` and `arboard`; `raw-window-handle` and
`x11rb` are currently dev-dependencies of the probes only). Porting notes for
anyone doing so: in x11rb 0.13 the client data is `From<[u32; 5]>`, not
`[u32; 8]`, and `ClientMessageEvent` has no `event_mask` field — the mask
belongs to `send_event`, which is why EWMH messages go to the root window.

**Q9 — What is the statistics window for `low_1pct` / `low_01pct`?** The
definitions are fixed (criterion 13), but they need a *sample set*, and the
natural answers disagree with each other:

* "last N seconds" (proposed default `window_seconds: 10`) — stable, and the
  window is what the user can reason about;
* "the whole run so far" — what a benchmark tool reports, but it never
  forgets an early stutter, so the number ratchets down and stops being
  actionable an hour in;
* "since the last display change" — needs event plumbing the panel does not
  have.

The sub-question underneath: **should the window slide on a timer (a ring of
the last N seconds) or on a frame count (the last M frames)?** A time window
means a 240 fps game contributes 4× the samples of a 60 fps one, so `1% low`
means different things at different frame rates — the same reason
`frametime_stats.py` prints *both* definitions. Proposed: a **time window**
(`window_seconds`), because "what my 1 % low was over the last 10 seconds" is
the question a player actually asks, and because it is what makes the graph
strip and the numbers describe the same interval. Needs an owner decision.

**Q10 — How often does the panel refresh its frame elements, and does that
cost the game?** Two coupled questions.

* *Cadence.* The panel's repaint policy is already in question (Q6: the
  current one is a 500 ms timer, and staged step 1 changes it). Frame
  elements invite repainting at the game's frame rate, which would make the
  panel a 144 Hz redraw source over a game. Proposed: `refresh_hz: 4` — fast
  enough that fps looks live, slow enough not to compete. Whether 4 Hz is
  enough for `frametime_graph` to look like a graph rather than a staircase is
  `NOT TESTED` and is really a design question about what the element is for.
* *Cost.* With a compositor off, the panel's own window redraw is cheap
  (measured 4.4 % / 1.9 % CPU at 1 Hz / 2 Hz in the earlier round); with a
  compositor on, every panel redraw may cost a composite pass. The earlier
  numbers are all **no-compositor** (B21), so the panel-plus-frames cadence
  under a compositor is `NOT TESTED` and is the same untested condition that
  Q10 in the PR body flags for the panel as a whole.

**Q11 — What does a present mean when the game uses frame generation
(FSR 2 / DLSS / XeSS / AFMF)?** This is the largest conceptual gap in the
design and the owner should decide how the panel should behave.

* A frame-generation present **interpolates** frames the GPU never rendered.
  A present-interval histogram will therefore contain a large population of
  roughly-half intervals, and `fps` will read the *displayed* rate — which is
  what the user sees and arguably the right thing to show.
* But `1% low` computed over that population measures the generator's cadence,
  not the game's ability, and MangoHud handles this with a per-game toggle
  (`fsr2_measure_gpu` and equivalents) rather than by an automatic rule.
* The spike's workload has no frame generation, so **nothing here is
  measured.** Proposed default: show presented fps, and let the user select
  "base fps" or "GPU fps" per title later. Whether the layer can distinguish
  them at all — DLSS exposes markers, FSR 2 does not — is `NOT TESTED` and
  differs per technology.

**Q12 — Two swapchains in one process, and two processes on one screen.**
Both happen:

* *Multiple swapchains per process* (e.g. a game window plus an internal
  thumbnail/preview swapchain, or a second window). The layer's swapchain
  hooks know how many exist, but which one defines "the frame" is a policy
  choice. Proposed: **count presents from the swapchain that presents most
  often**, tracked per-process, and expose the choice in the header
  (`ring_capacity`'s neighbour would need a `primary_swapchain` field — a
  protocol v1 amendment, not a reinterpretation). The spike measured window
  *resizing* (swapchain recreation, §4 of the spike) but **not** two
  simultaneous swapchains.
* *Two Vulkan games at once* (a game and a launcher, or two instances). ADR-5's
  rule — most recent present within 2 s, larger pid on ties — resolves it, but
  the user may want the panel to *follow* one game rather than switch to
  whichever presented last, which would make the number flicker between
  processes. Proposed: once a process is selected, keep it until it goes stale
  for `STALE_AFTER`. Needs an owner decision.

**Q13 — Does the owner accept shipping an implicit Vulkan layer with the
product?** Not a technical question, and the only one in this list that cannot
be answered by measurement. Points to weigh: it must be installed into the
Vulkan loader's search path on the host (a package or a udev-managed symlink
under `/usr/share/vulkan/implicit_layer.d/`), it is opt-in per process via
`LAPSPHERE_FRAMES=1`, and an implicit layer is loaded by *every* Vulkan
application on the machine even when inactive. The alternative — an explicit
layer, activated by `VK_INSTANCE_LAYERS` per launch — does not require host
installation and is more obviously scoped, at the cost of the user having to
set the variable for every launch (or Steam launch options), and of not
covering a launcher that spawns the game itself. The spike builds a manifest
with **both** an `enable_environment` and a `disable_environment` key, so both
routes are available; which one ships is this question.


---

## Staged implementation plan

One commit each. No code in this PR.

The owner's staging table for this work is **PR A → PR D → PR C → PR E**
(repaint, panel without fps, the layer, fps in the panel). Below, the seven
steps that existed before fps were added are mapped onto that table, and
**three problems with the ordering are stated**, because two of them change
what should be committed.

### Mapping to the owner's PR letters

| Owner PR | Contains | Steps | Depends on |
| --- | --- | --- | --- |
| **A** | `request_repaint` on data arrival | 1 (+2 only if Q3 says so) | nothing |
| **D** | the panel, **with no fps** | 3, 4, 5, 6, 7 | A (a panel that repaints at 500 ms is not a panel); Q1 answered |
| **C** | the Vulkan implicit layer + the shm protocol, **as a library and its tests — no panel code** | C1–C4 below | nothing in the app; only the loader, `ash` and the host |
| **E** | fps elements in the panel: the shm reader, the six frame elements, the config surface | C5 below | D (there must be a panel to add elements to) and C (there must be a layer producing data) |

### Problem 1 — the layer (C) has **no dependency on the panel (D)**, so C should be developed and merged in parallel, not after D

Nothing in C touches `gui/src`, `daemon/` or `common/`: the layer is a
`cdylib` in its own crate, the shm protocol is a document plus a Python and a
Rust reader, and its acceptance criteria (9–13) are all about the *layer and
the loader*. Putting C after D therefore buys nothing and costs wall-clock
time on the only part of this work with a genuine feasibility risk. C is also
the part that can invalidate the most design: if the overhead or the MangoHud
agreement fails, the owner wants to know before the panel is finished, not
after.

Recommended order: **A, then C and D in parallel, then E.** If C comes back
NO-GO or conditional, D is unaffected and still ships — which is exactly why
D must not contain any fps code.

### Problem 2 — D must not contain a *placeholder* for fps, and E must not re-touch the layout

The temptation when staging is to put an fps slot with a `—` into D. That
would violate ADR-2 as soon as E lands (the slot's reserved width changes at
the moment the data appears, which is the reflow criterion 4 forbids), and it
would make D→E a window-resize change rather than a data change. The width
table above is the contract: **D reserves the widths for all six frame rows
from the start** even though it draws nothing but `—`, so E only changes what
is drawn inside slots that already exist. That is a one-line config default in
D (`"visible": false`) and an addition to the drawing code in E.

### Problem 3 — C is one PR, not four, and its "commits" are not mergeable independently

C1–C4 below are four *commits inside PR C* (protocol + reader, manifest +
negotiation, present hook, hardening), chosen so that each is reviewable and
each compiles. They are **not** four PRs: the layer is useless without the
manifest, and the manifest is worthless without the hook. Do not ship C1–C3
without C4 — the fail-open work is what makes criterion 10 true, and a
half-hardened layer that can panic in a game process is worse than no layer.

### The steps, in the letters' order

**PR A**

1. **`request_repaint` on data arrival.** When a `HardwareUpdate` is
   consumed (or arrives), request an immediate repaint instead of waiting for
   the 500 ms timer. Improves the existing Statistics page too, and is the
   single change that makes a live panel usable. *Prerequisite: nothing.*
   Expected effect: avg latency 222 ms → low tens of ms.
2. **Move the channel drain into `logic()`** — *only if Q3 concludes the
   iconify case is in scope.* Otherwise skip. Per the spike this is a no-op
   for `Visible(false)`. *Prerequisite: Q3 answered.*

**PR D — the panel, no fps**

3. **Panel mode in `AppState` and settings.** Add the mode enum and the
   `show_*` config surface following the `statistics_sections` shape; persist
   and reload. No drawing yet. *Prerequisite: Q1 answered.*
4. **`pages/panel.rs` with a fixed layout and `—` placeholders.** Fixed
   widths per ADR-2, reserved slots, single fixed-height core strip. Draws
   with whatever data is in `AppState`. **Includes the six frame rows'
   reserved widths, drawn as `—`.** *Prerequisite: step 3.*
5. **Narrow the polled component set in panel mode.** Use
   `RefreshCoordinator::UpdateInterval` to park unselected components at a
   long interval (or add a pause/unregister). Never poll `logs` in panel
   mode. *Prerequisite: step 3; needs the pause mechanism.*
6. **Window switching, single-window shape (B18b):** the root stays
   `Normal` and is reshaped in place. Entering panel mode sends
   `ViewportCommand::WindowLevel(AlwaysOnTop)` **at runtime** plus an EWMH
   `ClientMessage` adding `SKIP_TASKBAR`+`SKIP_PAGER`; leaving sends REMOVE
   and `WindowLevel(Normal)`. Plus mode and position persistence (X11) with
   clamping against monitor size (ADR-1) and a no-op-tolerant path for
   Wayland (ADR-3). Things this step must get right, all measured:
   * The message is sent **after the first frames, not on frame 0** — 0 ms
     and 50 ms fail, 100 ms+ works (B18d). Send, read back, retry once; do
     not hard-code the threshold.
   * `data.l[3] = 0` (B18). Verify by read-back rather than trusting either
     Wine's comment or ours.
   * Always-on-top goes through the **runtime** `ViewportCommand`, never the
     builder hint, which xfwm4 ignores (B9/B14).
   * **First measurement to run:** the *combination* over a fullscreen game —
     one window that is both always-on-top and skip-taskbar. Each half is
     proven; the combination is `NOT TESTED`.
   * Fallback if the message proves unreliable: H5's second window, fully
     measured, at the costs tabulated in the ADR-1 amendment.
   *Prerequisite: steps 3-5.*
7. **README section** describing panel mode, the element list, and the
   Wayland caveats. *Prerequisite: step 6.*

**PR C — the layer, four commits, no application code**

C1. **Protocol v1 and its readers.** ADR-5 as code: the header layout and
ring as a documented `#[repr(C)]` struct in one module, the Rust reader with
the seqlock, and a Python reader used by the spike's measurements. Plus the
unit test that proves the Rust `low_percent`/`percentile` port matches
`frametime_stats.py` on the same inputs (criterion 13's parity requirement).
*No loader involvement yet — this commit is testable on its own with a fake
writer.*
C2. **Manifest, negotiation and the dispatch chain.** The implicit-layer JSON
with `enable_environment: LAPSPHERE_FRAMES` and `disable_environment`,
`vkNegotiateLayerInterfaceVersion`, `vkGetInstanceProcAddr` chaining,
instance/device dispatch tables, and `vkCreateSwapchainKHR`/
`vkDestroySwapchainKHR` hooks. Test: with the variable unset, the loader
enumerates the layer and no segment appears (criterion 11).
C3. **The `vkQueuePresentKHR` hook.** Timestamp, forward, write the interval
to the ring. Test: a segment appears, monotonic, and its intervals go through
`frametime_stats.py` with the expected median (criteria 9, 12).
C4. **Hardening, and this is the commit that must not be skipped.**
`catch_unwind` on every hook entry, no allocation and no syscall on the
present path, `O_EXCL` creation, 0600, stale-segment pruning, `dropped_count`,
and the fail-open cases (no reader, two instances, `kill -9`, unwritable
runtime dir). Test: the game is unaffected in every case (criterion 10).

**PR E — fps in the panel**

C5. **The shm reader in the GUI, the six frame elements, and the `frames`
config block.** Selection by ADR-5's rule (`STALE_AFTER = 2 s`), the
statistics window from Q9, the refresh cadence from Q10, `frametime_graph` as
one fixed-height strip, `—` when no source. *Prerequisite: PR D shipped and
PR C merged; Q9, Q10, Q12, Q13 answered.*

**One change to step 1, justified by the spike, not by B19.** B19's CPU
columns are inconclusive (a debug build, no true idle baseline — see the B19
section), so the argument for changing the repaint policy is the measured
**latency**, not a CPU saving: the only repaint request is
`request_repaint_after(500ms)`, and update→draw latency measured
**min 0.1 / avg 222 / max 484 ms** with the window visible. Requesting a
repaint on data arrival fixes that and, as a side effect, removes the
periodic wakeups. Whether it also reduces CPU enough to matter for a game is
`NOT TESTED` here and needs the B20 tooling on a real title.

Steps 1 and 2 are independent of the panel and could ship on their own.

**Step 0, now settled:** the window type is no longer a decision. B18 found
why the message was refused, and a single `Normal` root window with a
best-effort EWMH message satisfies every goal. The only remaining step-0
question is whether to keep H5's second-window implementation as a fallback;
the recommendation is to do so, since it is already measured and costs
nothing until used.
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
* **The single-window stacking combination**: one window that is both
  runtime always-on-top *and* skip-taskbar, above a fullscreen game. B9
  proves the level, B18b proves the atoms, neither proves them together.
* **`data.l[3]` on other WMs** — moot in the sense that the field is *not*
  the cause here: Wine's `1` and wmctrl's `0` both work on xfwm4 4.20.0 once
  the message arrives at the right time (6/6 each). Whether any other WM
  cares about the field is `NOT TESTED` and is not worth worrying about,
  because the design confirms by read-back rather than trusting a byte.
* **The "wait for acceptance" signal on other WMs.** Observed here as the id
  appearing in `_NET_CLIENT_LIST`; a WM need not maintain that list the same
  way. `NOT TESTED`.
* **Wayland cannot do this at all**: `_NET_WM_STATE` is X11-only and a
  Wayland client may not set it. Panel-mode switcher exclusion on Wayland is
  `NOT TESTED` and, on the evidence available here, not achievable by the
  same mechanism. The same is true of H5's `_NET_WM_WINDOW_TYPE` fallback.
* **The root cause of our ClientMessage being refused while `wmctrl`'s is
  accepted.** Unresolved; see the open bug in Q8.
* From earlier rounds, still open: keyboard pass-through; hot-reload of font
  scale and item order; flicker as a subjective judgement; a real tray click;
  the 10-minute iconify soak; tray survival across a window recreate.
**Runtime-PM on a discrete GPU** was measured only on this host's NVIDIA
3070 Laptop GPU under one compositing-less X11 session.

**Frame measurement — nothing below has any evidence on this host.**

* **DXVK / Proton / Wine.** The spike's workload is a native Vulkan
  application (`vkcube`). A DXVK-translated Direct3D game presents through the
  same Vulkan entry point, so the mechanism should work, but whether a
  translation layer's present cadence matches what the user sees, and whether
  any of them interacts badly with an implicit layer, is `NOT TESTED`. **This
  is the single largest gap**: most Windows games reach this host through
  exactly this path, so a spike on `vkcube` is a mechanism proof, not a
  product proof.
* **OpenGL / GLX / EGL games.** Not hooked at all — a first-release non-goal.
  Whatever those games show in the frame elements is `—`, untested because
  never attempted.
* **32-bit games.** The layer is a 64-bit `cdylib`; a 32-bit process cannot
  load it. Not attempted.
* **Anti-cheat.** No game with an anti-cheat suite was run. Whether any suite
  enumerates layers, objects or `$XDG_RUNTIME_DIR/lapsphere/` and objects is
  `NOT TESTED`, and no claim is made.
* **Wayland.** No Wayland session on this host. The layer has no Wayland code
  (it is API-level), but whether a present on a Wayland surface still marks a
  displayed frame — presentation timing, explicit sync, `wp_presentation`
  feedback — is `NOT TESTED`, and that is what the numbers would depend on.
* **A GPU-bound game.** The spike workload is not GPU-bound, so the
  MangoHud-agreement result (criterion 12) is measured where present-to-present
  time is dominated by CPU and vsync. On a GPU-bound title the CPU-side
  timestamp includes GPU work too, so the same code path yields *different*
  numbers, and agreement has not been shown there.
* **Frame generation** (FSR 2 / DLSS / XeSS / AFMF): see Q11 — untested and
  conceptually undecided.
* **Multiple swapchains in one process** (Q12): swapchain *recreation* on
  window resize was measured; two live swapchains at once was not.
* **The Rust reader's parity with `frametime_stats.py`** beyond the spike's
  cross-check of raw intervals: no adversarial unit-test harness (criterion
  13).

## What is deliberately not decided here

* Whether the panel can be dragged by any part of its surface or only by a
  dedicated handle (interacts with Q2 and with ADR-3's position story).
* Whether multiple panels are ever wanted (out of scope; would change ADR-1).
* The exact element→width table (blocked on Q1) — the frame rows are the
  exception: their widths are proposed above and, unlike the D-Bus rows, no
  source code or live interface can confirm them, because the data does not
  exist yet. They are **design decisions, not measurements**, and the panel's
  size will be whatever the first real font metrics say.
* Whether panel mode should be startable from the CLI (`--panel`) alongside
  the existing `--tray`.
