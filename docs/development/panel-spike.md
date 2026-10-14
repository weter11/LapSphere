# Panel mode spike — gkrellm-style compact mode in the eframe GUI

**Base commit: `5c57bc7`** (branch `feat/mini-panel`, created from
`origin/lapsphere` @ `5c57bc7efa20d6b840bf8d0ea474c087ba0fee17`).
All `file:line` references in this document are recomputed against that
commit. Run date: 2026-09-30.

Status: **X11 measured, Wayland not tested.** Every Wayland cell is marked
`NOT TESTED`; no extrapolation is presented as measurement.

> Correction to the previous revision: it was written against `09ba397`
> (not `7a041bc`). Both are ancestors of `5c57bc7`. The delivery path itself
> (`mpsc(100)`, `try_recv` in `ui()`, the single 500 ms
> `request_repaint_after`) is unchanged between them — verified by
> `git diff 09ba397 HEAD -- gui/src/app.rs`, which touches only the
> `logs` polling gate. What did change upstream is recorded in
> "Base drift" below, and one of those changes invalidates a conclusion the
> previous revision drew.

---

## Environment

```
XDG_SESSION_TYPE=x11   WAYLAND_DISPLAY=(unset)   DISPLAY=:0.0
XDG_CURRENT_DESKTOP=XFCE   WM: xfwm4 (_NET_SUPPORTING_WM_CHECK 0xa00032)
root-owned lapsphere-daemon already running: PID 1131, owns io.lapsphere.Control
  on the SYSTEM bus (busctl --system, uid 0)
```

eframe/egui/egui-winit/egui_glow **0.34.2**, winit **0.30.13**, tokio
**1.49.0**, zbus **5.13.2**, ksni **0.3.3**, glow **0.17.0**, rustc **1.98.0**.
Renderer: glow, eframe features `default_fonts, glow, wayland, x11`.
Display 2560x1440, scale 1.771 (egui monitor = 1445.6 x 813.2 points).

`xfwm4`'s version string was **not captured**. That remains a gap.

**No Wayland session exists on this host** (no GNOME, no KDE, no
sway/weston/cage, no nested compositor, no passwordless sudo), so all
Wayland / GNOME / KDE / XWayland rows are `NOT TESTED`. `Xwayland` is
present but cannot be exercised: it hosts X11 clients inside a Wayland
session, and there is no Wayland session. The X11 column below is plain X11
under xfwm4, **not** XWayland.

## Method

Two throwaway probes, both outside `gui/src/**`:

* `gui/examples/panel_probe.rs` — pipeline model with a synthetic producer.
  Retained from the first round; used for the `ViewportCommand` matrix and
  the panel-boot flash check.
* `gui/examples/panel_probe2.rs` — **talks to the live daemon** over the
  system bus via `zbus::Proxy` against `io.lapsphere.Control`, calling the
  same methods and on the same schedule as `gui/src/app.rs`. The real
  `gui/src/system_tray.rs` is included verbatim via `#[path]` and driven
  through its real `handle_events()`.

Component set and intervals for probe2, matching `app.rs:429-442` and
`common/src/types.rs` `AppConfig::default`:

| component | interval | D-Bus method |
| --- | --- | --- |
| cpu, memory, fans, gpu_overclock | 1000 ms | `GetCpuInfo`, `GetMemoryInfo`, `GetFanInfo`, `GetGpuInfo` |
| gpu | 2000 ms | `GetGpuInfo` |
| battery, wifi, gamepads, storage, mount, webcam, logs | 5000 ms | `GetBatteryInfo`, `GetWifiInfo`, `GetGamepadInfo`, `GetStorageDeviceInfo`, `GetMountInfo`, `GetWebcamState`, `GetDaemonLogs` |

(That is 12 registered components, matching `app.rs:429-442`. The brief
says "11 components"; the shipped code registers 12 including `logs`.
Counted, not assumed.)

Delivery path replicated exactly: `mpsc::channel(100)` (`app.rs:325`) →
coordinator `tokio::spawn` per due component → `tx.send(..).await` →
`while let Ok(u) = rx.try_recv()` in `ui()` (`app.rs:565`) →
`ctx.request_repaint_after(500ms)` (`app.rs:917`) as the only repaint
request.

Latency is timestamped immediately before `tx.send()` and sampled at
`try_recv()`, so it measures **HardwareUpdate produced → consumed by a
`ui()` frame**. It excludes GPU present, which is not observable from inside
`ui()`.

External ground truth via `probe/watch_x11.sh`, which resolves the probe
window **by PID** (matching by title with `xdotool search --name` was
tried first and matched unrelated windows), sampling `xprop`/`xdotool`:
geometry, `WM_STATE`, `_NET_WM_STATE`, `WM_NORMAL_HINTS`, `_MOTIF_WM_HINTS`.

`dbus_err` in the logs is entirely `GetWebcamState: Signature mismatch: got
'b', expected 's'` — the probe's own probe-side typing mistake, not a
daemon fault. It does not affect any other measurement (that component
simply returns no data). The real app calls it through `dbus_client.rs`,
which is typed correctly.

---

## 1. Frame delivery

### (a) Latency with a visible window — X11/xfwm4, live daemon

120 s, real D-Bus, 12 components, `probe/p2_latency.log`:

```
prod=684  cons=684  dbus_ok=684  dbus_err=24 (webcam only)
lat min=0.1 ms   avg=222.1 ms   max=484.2 ms
histogram [<5,<50,<100,<200,<350,<500,>=500] ms
            = [28, 70, 86, 145, 182, 173, 0]
queue_max=10  max_inflight=2  blocked=0
ui=1267  logic=1267  paint=1266
```

Readings:

* **`max = 484.2 ms` and the histogram is empty above 500 ms.** The
  distribution is bounded by the 500 ms `request_repaint_after`
  (`app.rs:917`). Nothing in the app calls `request_repaint` on data
  arrival, so an update waits for the next scheduled repaint. With a
  500 ms period and ~7 updates/s the expected latency is a broad
  distribution under 500 ms, which is what was measured.
* **avg 222 ms ≈ half the 500 ms period**, consistent with a uniform phase
  offset and no measurable transport cost — the D-Bus round trip is
  microseconds-to-low-milliseconds against these numbers.
* `paint = ui - 1` throughout: every `ui()` call produced a paint. On a
  visible window the frame path is healthy; the cost is *latency*, not
  dropped frames.
* Queue never exceeded 10 of 100 and `blocked = 0` even at 7 updates/s.

### (б) `Visible(false)` for 10 minutes — X11/xfwm4, live daemon

This is the case the brief asked for specifically, because the tray uses
`Visible(false)`. 600 s soak, `probe/p2_hidden.log`:

```
HIDE   at ui=19, rss=114308 kB
t+60s   ui_since_hide=241   prod=370   cons=370   queue_max=10  blocked=0  rss=121348
t+300s  ui_since_hide=916   prod=1738  cons=1738  queue_max=10  blocked=0  rss=126248
t+360s  ui_since_hide=1126  prod=2076  cons=2076  queue_max=10  blocked=0  rss=126312
t+420s  ui_since_hide=1251  prod=2422  cons=2422  queue_max=10  blocked=0  rss=126304
SHOW   after 600s hidden: ui+1645  prod=3444  cons=3444  queue_max=10  blocked=0
RESULT prod=3534 cons=3534 queue_max=10 max_inflight=3 blocked=0 rss_end=127036 kB
```

**Answers, in the order asked:**

* **Is `ui()` called while `Visible(false)`? — YES, continuously.**
  1645 `ui()` calls during the 600 s hidden window (~2.7/s, the 500 ms
  repaint tick, slightly below it). `Visible(false)` did **not** stop the
  UI loop.
* **Does the mpsc queue grow? — NO.** `prod == cons` at every sample
  (3534 == 3534 at the end), and `queue_max` never exceeded **10 of 100**
  across the whole 10 minutes. Because the consumer keeps running, nothing
  accumulates. `blocked = 0` and `max_inflight = 3` throughout: **zero
  tasks were ever blocked on `tx.send().await`.**
* **RSS over 10 minutes: +12.7 MB, and it plateaus.** 114 308 kB at hide →
  121 348 (t+60 s) → 126 248 (t+300 s) → 126 312 (t+360 s) → 126 304
  (t+420 s) → 127 036 (end). Growth is concentrated in the first ~5 minutes
  and then flattens to ~±60 kB of noise. This is ordinary warm-up of the
  allocator and the D-Bus connection, not a leak signature; a leak would
  be monotonic.

**Why `ui()` keeps running — mechanism, from eframe 0.34.2 source:**

`epi_integration.rs:285-292` calls `app.logic(...)` **unconditionally**, then
gates `app.update(...)` and `app.ui(...)` behind `if is_visible`. The gate
value is computed at `glow_integration.rs:559`:

```rust
let is_visible = viewport.info.visible().unwrap_or(true);
```

and `ViewportInfo::visible()` (`egui/src/data/input.rs:270-276`) is:

```rust
match (self.minimized, self.occluded) {
    (Some(true), _) | (_, Some(true)) => Some(false),
    (Some(false), Some(false)) => Some(true),
    (_, None) | (None, _) => None,      // -> unwrap_or(true) -> VISIBLE
}
```

**It is derived from `minimized` and `occluded` only. It never consults
whether `ViewportCommand::Visible(false)` was sent.** `egui-winit` fills
those fields from the winit window (`update_viewport_info`,
egui-winit-0.34.2/src/lib.rs:1226+`), and on X11 an unmapped-but-not-
minimized window reports `minimized: false`, `occluded: None` →
`visible()` returns `None` → `unwrap_or(true)` → **`is_visible == true`** →
`ui()` is called.

This is the mechanism behind the tray's `Visible(false)` behaviour, and it
is the single most important fact for the panel work. It also **corrects
the previous revision of this document**, which asserted that `ui()` stops
while hidden. That assertion came from the earlier probe measuring `ui()`
during an **iconify** (WM minimize) test, and it does not generalize to
`Visible(false)`. The two are genuinely different:

| Window state | `minimized` | `occluded` | `visible()` | `ui()` called? | Evidence |
| --- | --- | --- | --- | --- | --- |
| normal | false | some(false) | Some(true) | yes | all runs |
| **iconified (WM minimize)** | **true** | — | **Some(false)** | **NO** | `panel_probe_watch.log`: `ui_calls` frozen at 11 for the whole 20 s iconify, resumed to 24 on restore |
| **`Visible(false)` (tray hide)** | **false** | **None** | **None → true** | **YES** | `p2_hidden.log`: +1645 `ui()` over 600 s hidden |

So the app's own tray-hide path keeps the full UI loop running, and the
WM-iconify path does not. The panel must behave correctly in **both**.

### (в) What wakes the event loop when the tray is clicked?

`handle_events()` (`system_tray.rs:153-159`) is a plain
`self.menu_rx.try_recv()` on a `std::sync::mpsc::Receiver`, and it is called
only from `ui()` (`app.rs:875` → `handle_tray_events`, `app.rs:804`).
`ksni::blocking::TrayMethods::spawn` runs the tray service on a **dedicated
OS thread** (`ksni-0.3.3/src/blocking.rs`, `spawn_with_options`:
`thread::spawn(move || compat::block_on(service_loop))`). The tray thread
never holds an `egui::Context` and cannot call `request_repaint`.

Consequence, and the answer to the question:

> **A tray click does not wake the loop. It is only ever noticed on the
> next `ui()` call, which happens on the next 500 ms
> `request_repaint_after` tick.**

The click lands in the `std::mpsc` immediately (the tray thread is
independent), but it stays queued until `ui()` runs. Since (б) shows `ui()`
keeps ticking while `Visible(false)`, **the tray's ShowWindow path works
today** — verified in (г) below: `tray handle still responsive = true`
after 300 switches and a hide/show cycle. The latency cost is up to 500 ms,
which is invisible to a user clicking a tray icon.

The fragility is not "the tray cannot wake the app" but that it *depends*
on `ui()` continuing to run, and that is an accident of
`ViewportInfo::visible()` returning `None` on X11 rather than a designed
behaviour. Under a WM iconify, `ui()` stops, and a tray click would then
wait for the user to restore the window. A robust design would have the
tray thread call `ctx.request_repaint()` on a cloned `egui::Context` — it is
`Send + Sync + Clone`, so this is possible, but it is a code change and is
therefore out of scope here.

### (г) 300 mode switches, tray afterwards, RSS — X11/xfwm4, live daemon

`probe/p2_toggle.log`, real `SystemTray` included verbatim, 300
full↔panel transitions over ~93 s:

```
TRAY    real SystemTray::new OK
TOGGLE  300 switches done; rss=122456 kB queue_max=10 blocked=0
VIEWPORT after 300 switches  inner=260x72 outer=260x72 pos=3,45 focused=true
TRAY    handle_events -> None (no user click injected; expected)
TRAY    after Visible(false): querying tray handle, then Visible(true)
TRAY    tray handle still responsive (no pending event) = true
VIEWPORT after tray Show/Hide inner=570x620 outer=577x648 pos=0,21 focused=true
RESULT  rss_start=122456 rss_end=122472 delta=16 kB
        ui=1230 queue_max=10 max_inflight=2 blocked=0 dbus_ok=583
```

**RSS delta across 300 switches: +16 kB. No leak.**

X11 ground truth over the same run (`probe/x11_toggle.log`, 3 Hz): 313
samples, geometry distribution `{'460x128': 135, '1009x1098': 178}` — i.e.
clean alternation between the panel size (260x72 pt × 1.771 = 460x128 devpx)
and the full size (570x620 pt = 1009x1098 devpx) with no intermediate or
stuck states, and `_MOTIF_WM_HINTS` third field flipping `0x1 ↔ 0x0`
(resizable) in lockstep. Final state was correct after the tray cycle:
`WM_STATE` returned from `Withdrawn` to `Normal`.

**Tray after mode switches: still functional.** `SystemTray::new` succeeded
against the real SNI bus, `handle_events()` was callable before and after
the hide/show cycle, and the window came back to 570x620 with
`WM_STATE=Normal`. The one thing **not** proven is a genuine user click
producing `TrayEvent::ShowWindow`: no synthetic click was injected into the
SNI menu, so the `Some(event)` arm was not exercised. What is proven is
that the handle stays alive and the queue is reachable across 300 switches.

### (д) Does eframe call `App::logic()` when hidden? Does draining in
`logic()` fix anything?

**First half — CONFIRMED, from source and from measurement.**

Source: `epi_integration.rs:285-287` calls `app.logic(...)` outside the
`if is_visible` block, so it runs whenever a frame is produced.

Measured, 60 s hidden (`probe/p2_logic.log`):

```
HIDE  Visible(false) at ui=514 logic=515
SHOW  60s hidden: ui +124 (was 514)  logic +124 (was 515)
```

`ui()` and `logic()` advanced by **exactly the same count, 124**, over the
hidden window. `ui()` is not being starved; both hooks run every frame.

**Second half — REFUTED. Moving the drain into `logic()` solves nothing.**

`gui/src/app.rs:850-851` implements only `ui()`. Because `ui()` keeps
running while `Visible(false)`, the existing drain in
`handle_hardware_updates()` (`app.rs:565`) already executes on schedule.
`prod == cons` (3534 == 3534 over 600 s) is the direct evidence: nothing
was left unconsumed, so there is no backlog for `logic()` to recover.

A second run with the drain moved into `logic()`
(`probe/p2_logicdrain.log`, `--drain-in-logic`):

```
HIDE   Visible(false) at ui=18 logic=19
SHOW   60s hidden: ui +117  logic +117  consumed=342  queue_max=10 blocked=0
RESULT consumed_in_logic=456  prod=456  queue_max=10  blocked=0
```

Draining in `logic()` does consume everything (`consumed_in_logic=456 ==
prod=456`) — it is functionally correct, and harmless. But it fixes nothing,
because the thing it was meant to fix is not happening. The only scenario
where it would matter is a WM iconify, where `ui()` genuinely stops; and
that scenario is a real user path (minimize the window, then restore).

**Recommendation: do not do step 2 of the staged plan as originally framed.**
It is a no-op for the tray-hide case. If the WM-iconify case is in scope,
`logic()` is the right place — but it should be justified by the iconify
measurements, not the `Visible(false)` ones.

---

## 2. `ViewportCommand` at runtime

From `panel_probe_cmds.log` + `x11_cmds.log` (probe1; the command semantics
are eframe-level and did not change between the two bases). `devpx` =
device pixels, scale 1.771.

| Function | Environment | Result | Evidence |
| --- | --- | --- | --- |
| `InnerSize(1200x800)` | X11/xfwm4 | **WORKS** | inner 570x620 → 1200x800, outer 1207x828 |
| `MinInnerSize(300x200)` | X11/xfwm4 | **WORKS** | `WM_NORMAL_HINTS` "program specified minimum size" 779 → 531 devpx (300 × 1.771) |
| `MinInnerSize(800x600)` | X11/xfwm4 | **WORKS** | 531 → 1417 devpx (800 × 1.771) |
| `Decorations(false)` | X11/xfwm4 | **WORKS** | outer 1207x828 → 1200x800 (outer == inner), position shifts 239,21 → 242,45 as the WM re-frames |
| `Decorations(true)` | X11/xfwm4 | **WORKS** | outer back to 1207x828 |
| `WindowLevel(AlwaysOnTop)` | X11/xfwm4 | **WORKS** | `_NET_WM_STATE_ABOVE` appears |
| `WindowLevel(AlwaysOnBottom)` | X11/xfwm4 | **WORKS** | `_NET_WM_STATE_BELOW` appears |
| `WindowLevel(Normal)` | X11/xfwm4 | **WORKS** | state cleared |
| `Resizable(false)` | X11/xfwm4 | **WORKS** | `_MOTIF_WM_HINTS` 3rd field `0x1` → `0x0` |
| `Resizable(true)` | X11/xfwm4 | **WORKS** | `0x0` → `0x1` |
| `Visible(false)` | X11/xfwm4 | **WORKS** | `WM_STATE = Withdrawn` |
| `Visible(true)` | X11/xfwm4 | **WORKS** | `WM_STATE = Normal` |
| Panel geometry at boot, no flash | X11/xfwm4 | **WORKS** | frame 0 already `inner=260x72 outer=260x72 x=20 y=20`; identical through frame 11 |
| 300 round trips of the above | X11/xfwm4 | **WORKS** | 313 X11 samples, clean alternation, correct final state |
| all of the above | **GNOME Wayland** | **NOT TESTED** | no Wayland session on host |
| all of the above | **KDE Wayland** | **NOT TESTED** | no Wayland session on host |
| all of the above | **XWayland** | **NOT TESTED** | no Wayland host session; cannot be exercised standalone |
| all of the above | **GNOME/KDE on X11** | **NOT TESTED** | only XFCE/xfwm4 present |

A panel-shaped window can be created directly through `ViewportBuilder` —
size, `resizable(false)`, `decorations(false)` and `position` all applied
before frame 0, so there is **no large-window flash**. That is the clean
path for "start in panel mode" and needs no runtime command.

`OuterPosition(2000, 1200)` (off-screen) was **clamped by the WM** to
x=1421 y=789 rather than rejected. Any saved-position restore must clamp
against monitor size itself.

---

## 3. Window position

| Case | Environment | Result | Evidence |
| --- | --- | --- | --- |
| Restore position at runtime | X11/xfwm4 | **WORKS** | `OuterPosition(200,150)` → reported `x=200 y=150` |
| Off-screen position | X11/xfwm4 | **CLAMPED, not rejected** | 2000,1200 → x=1421 y=789 |
| Position survives 300 switches | X11/xfwm4 | **PARTIAL** | position ended at 0,21 rather than the pre-test 300,200; the WM re-frames on every `Decorations` toggle and the final placement is WM-chosen. Not a defect, but a restored position must be re-applied, not assumed |
| Restore saved position | **Wayland** | **NOT TESTED** | per eframe's own docs on `ViewportInfo.outer_rect` ("On Android / Wayland, this will always be `None`"), position is not retrievable and the compositor owns placement. Not verified here. |

The app persists no window geometry today: `main.rs:245-252` hardcodes
`with_inner_size([570.0, 620.0])` / `with_min_inner_size([440.0, 470.0])`
and there is no geometry field in `AppConfig`. Position restore is net-new
work on any backend.

---

## 4. Regression risk

| Case | Environment | Result | Evidence |
| --- | --- | --- | --- |
| RSS over 300 mode switches | X11 | **NO GROWTH** | 122 456 → 122 472 kB, **delta +16 kB** |
| Tray alive + reachable after 300 switches | X11 | **WORKS** | `SystemTray::new` OK, `handle_events()` callable pre- and post-cycle, window restored to 570x620, `WM_STATE` Withdrawn → Normal |
| Tray click → `ShowWindow` end-to-end | X11 | **NOT TESTED** | no synthetic SNI click injected; `Some(event)` arm never taken |
| RSS over 600 s `Visible(false)` | X11 | **+12.7 MB then plateau** | 114 308 → 126 248 (t+300 s) → 127 036 (end); flat within ~60 kB from t+300 s |
| RSS over 120 s visible | X11 | flat | 119 856 kB at end, 684 updates consumed |
| WM iconify → RSS/queue | X11 | **PARTIAL** | 20 s iconify: `ui()` frozen, queue reached 19/100, `blocked=0`, RSS flat 110 560 kB. Too short to characterise a 10 min trend |
| Occluded-by-another-window | X11 | **NOT TESTED** | `occluded` stays `Some(false)` on plain X11 (no compositor computes it), and this xdotool build has no `windowlower`, so a genuine occlusion could not be staged |

---

## Base drift (`09ba397` → `5c57bc7`) — what changed and why it matters

`git diff 09ba397 HEAD -- gui/src/app.rs gui/src/main.rs gui/Cargo.toml`
= +252/−9 across three files. None of it touches the delivery path measured
above, but one item is directly relevant:

* **`app.rs:36-90` adds a daemon-log fetch gate.** `GetDaemonLogs` returns
  the daemon's whole 2000-entry ring (~470 kB of JSON) and was being
  fetched every 5 s regardless of the visible page. The new
  `UI_FRAME_FRESH_MS` / `LAST_UI_FRAME_MS` / `LOGS_TAB_ON_SCREEN` statics
  (`app.rs:56-90`) gate the callback behind "the Logs tab is on screen and a
  frame was painted within the last 5 s". Registered at `app.rs:442`.
  Relevant to the panel because it establishes the existing precedent for
  **conditional, mode-dependent polling** — which is what panel item 5
  needs, and `RefreshCoordinator::UpdateInterval`
  (`polling_scheduler.rs`) already provides the mechanism.

The important consequence: the previous revision's headline finding
("`ui()` stops when hidden, therefore move the drain to `logic()`") is
**wrong for `Visible(false)`** and survives only for WM iconify. That
correction is the main new result of this round.

---

## Problems found in the current code

Facts with locations against `5c57bc7`.

1. **`app.rs:917`** — the only repaint request in the app is
   `request_repaint_after(500ms)`. Measured: min 0.1 / **avg 222** /
   **max 484 ms** with the window visible, histogram empty above 500 ms.
   Nothing calls `request_repaint` when data arrives, so a live meter in a
   panel would lag by up to half a second by design. Fixing this helps the
   normal mode too (staged plan step 1).

2. **`app.rs:850-851`** — only `ui()` is implemented. `logic()`
   (`epi_integration.rs:147`) runs unconditionally and is the documented
   hook for the hidden case, but it is unused. Today this costs nothing
   for `Visible(false)` (see б) but is the only thing that would keep data
   flowing under a WM iconify, where `ui()` provably stops.

3. **`app.rs:325` + `app.rs:565`** — `mpsc::channel(100)` with no
   coalescing policy. Harmless as measured (`prod == cons` over 600 s
   hidden, `queue_max` 10/100, `blocked=0`), but the bound is doing no work:
   the consumer keeps up, so the channel never limits anything. Any future
   change that slows the drain turns this into a silent 100-message backlog.

4. **`app.rs:385-415`** — every handler is `let _ = tx.send(...).await`.
   A `SendError` (receiver dropped during exit) is silently discarded.
   Not a panel issue, but it is in the delivery path and makes send
   failures invisible.

5. **`main.rs:245-252`** — no geometry persisted, no position restored,
   hardcoded 570x620. Off-screen position requests are clamped by the WM
   rather than rejected, and a restored position is not preserved across
   decoration toggles (ended at 0,21 after 300 switches). A panel needs to
   clamp and re-apply explicitly.

6. **`app.rs:859-865`** — `startup_frames` issues `Visible(false)` for up
   to 10 frames when `start_minimized`/`--tray` is set. Since `ui()` keeps
   running under `Visible(false)` (measured), this resolves normally, but
   it means "start hidden" currently costs 10 frames of full UI work before
   the window is withdrawn. A start-to-panel path will touch this.

7. **Tray wake-up is implicit** — `handle_events()` (`system_tray.rs:153`)
   is only reachable from `ui()`; the ksni service thread holds no
   `egui::Context`. Tray response therefore depends on `ui()` continuing to
   tick, which for `Visible(false)` is a side effect of
   `ViewportInfo::visible()` returning `None` on X11 rather than a designed
   behaviour. Works today (verified), ≤500 ms latency, fragile.

---

## Not verified

1. **All Wayland / GNOME / KDE / XWayland behaviour.** No Wayland session,
   no compositor, no passwordless sudo. Open in particular: whether
   mutter/kwin honour `WindowLevel(AlwaysOnTop)` and `Visible(false/true)`
   for a non-fullscreen client (both commonly restricted), and whether
   Wayland's damage/occlusion model changes the (б) findings.
2. **A real tray click** producing `TrayEvent::ShowWindow`.
3. **WM-iconify soak** at 10 min scale (only 20 s measured).
4. **Genuine occlusion** by another window (`occluded` unavailable on X11;
   no `windowlower` in this xdotool).
5. **`xfwm4` version string** was not recorded.
6. **The shipped GUI binary itself was not exercised** — all measurements
   come from probes that replicate `app.rs`'s delivery path. The one place
   the replica is known to differ from the real app: the real
   `dbus_client.rs` is a command/oneshot worker with reconnection logic,
   while the probe calls `zbus::Proxy` directly, so reconnect behaviour is
   not covered.

## Reproduce

```bash
cd /home/wer/devis/lapsphere/repo
git checkout -B feat/mini-panel origin/lapsphere   # 5c57bc7
CARGO_BUILD_JOBS=2 cargo build -p lapsphere --example panel_probe --example panel_probe2

# (a) latency, live daemon, 12 components
./target/debug/examples/panel_probe2 latency 120
# (б) 10-minute Visible(false) soak
./target/debug/examples/panel_probe2 hidden 600
# (в)+(д) logic() vs ui() while hidden; --drain-in-logic for the variant
./target/debug/examples/panel_probe2 logic 100
PROBE_LOG=/tmp/p2_logicdrain.log ./target/debug/examples/panel_probe2 logic 100 --drain-in-logic
# (г) 300 switches + tray, with X11 ground truth
probe/drive_toggle.sh ./target/debug/examples/panel_probe2 300 probe/x11_toggle.log probe
# 2  ViewportCommand matrix + X11 ground truth
probe/drive_cmds.sh  ./target/debug/examples/panel_probe  cmds 60 probe/x11_cmds.log probe
# 1b iconify / occlude
probe/drive_watch.sh ./target/debug/examples/panel_probe 80 probe/x11_watch.log probe
# 2  panel-mode boot, flash check
./target/debug/examples/panel_probe panelboot 6
```

Prerequisite: a `lapsphere-daemon` owning `io.lapsphere.Control` on the
**system** bus (this host: PID 1131, uid 0).

Logs: `probe/p2_<mode>.log` (in-process telemetry),
`probe/panel_probe_<mode>.log` (probe1), `probe/x11_<mode>.log` (external
X11 sampler), `probe/driver.log`.
