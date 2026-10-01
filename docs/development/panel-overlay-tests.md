# Panel overlay tests (B1–B8)

Live behaviour of a gkrellm-style panel drawn over another application's
window, measured on X11.

**Status: complete for the X11 matrix. Wayland is `NOT TESTED` on this host** —
no Wayland session, no compositor, no nested compositor, no passwordless
sudo. Nothing below is extrapolated to mutter, kwin, GNOME or KDE.

Companion documents: [`panel-spike.md`](./panel-spike.md) (frame delivery and
`ViewportCommand` behaviour), [`panel-design.md`](./panel-design.md) (design
decisions and acceptance criteria).

## Test environment

Everything below was measured on **one** machine. Results are stated per
window manager and are not generalised.

| Property | Value |
| --- | --- |
| Window manager | **xfwm4 4.20.0** (revision unknown), Xfce 4.20 |
| Compositor | **none** — `xprop -root _NET_WM_COMPOSITING` → *no such atom* |
| Session | `XDG_SESSION_TYPE=x11`, `DISPLAY=:0.0`, XFCE, no taskbar in session |
| Display | 2560x1440, scale factor **1.771** (egui reports 1445.6 x 813.2 points) |
| GL renderer | `AMD Radeon Graphics (radeonsi, renoir, ACO, DRM 3.64)` — `glxinfo -B` reports `Accelerated: yes` |
| GPU devices | AMD Radeon (integrated, RADV), NVIDIA RTX 3070 Laptop (discrete), llvmpipe (CPU) |
| dGPU | `0000:01:00.0`, `power/control=auto`, runtime-PM used |
| Tooling | vkcube, glxgears, xdotool, xprop, xwd, ffmpeg, nvidia-smi |
| Not available | `wmctrl`, `import`, `scrot`, `convert` (ImageMagick), `eog` |

**On fps numbers:** the renderer is real hardware (not llvmpipe), so absolute
values mean something, but glxgears draws two rotating triangles and is *not*
game-representative. The useful figure is the delta between identical runs.
This is stated again in the B5 row.

### Scale gotcha that invalidated two early attempts

`xdotool` and `xprop` report **device pixels**; `ViewportBuilder` takes
**points**. At scale 1.771 a panel positioned at `y=700` points is at
`y≈1240` device pixels. Early pixel samples were taken at point coordinates
and therefore read desktop wallpaper instead of the panel. All results below
read the window's real geometry first and sample relative to it.

Reference colours used for pixel assertions:

| Colour | Meaning |
| --- | --- |
| `18,18,20` | the panel's own background (opaque) |
| `51,51,51` | the cube's clear colour |
| `8,0,11` | desktop wallpaper |
| `8,8,8` | root window / black, what a transparent window shows without a compositor |

## Results

| Scenario | Result | How it was checked |
| --- | --- | --- |
| **B1** windowed cube + panel always-on-top | **WORKS.** Panel draws above the cube; `18,18,20` over `51,51,51` in the overlap region | pixel sampling inside the panel's own rect + `xprop _NET_WM_STATE`; `b1-panel-over-cube.png` |
| **B2** cube in borderless fullscreen, panel AOT **set at startup** | **PANEL IS COVERED.** With a genuine fullscreen cube (2560x1440 @ 0,0) the pixel inside the panel is `51,51,51` — the cube. The panel reappears only after re-activation, or after forcing `_NET_WM_STATE_ABOVE` externally. **Superseded by B9**: the cause is that the level was set via `ViewportBuilder::with_window_level` at startup, which xfwm4 ignores. Setting it at runtime fixes it completely | F11 via xfwm4's own binding (setting `_NET_WM_STATE_FULLSCREEN` via `xprop` changed the property but left the geometry at 900x600 — no real fullscreen); pixel sampling at three points; `b2-fullscreen-cube-panel-covered.png`, `b2-fullscreen-cube-panel-visible.png`, `b2-fullscreen-transition.mp4` |
| **B2** compositor on vs off | **NOT TESTED.** This session has no compositor, and enabling xfwm4 compositing requires restarting the WM | `xprop -root _NET_WM_COMPOSITING` absent; no sudo |
| **B3** does the panel steal focus when it appears? | **YES.** On creation the panel holds focus (`_NET_WM_STATE_FOCUSED`, `xdotool getwindowfocus` → panel) | focus query immediately after launch |
| **B3** clicks reach the game with `MousePassthrough(true)`? | **WORKS.** Without passthrough a click over the panel moves focus to the panel. With passthrough, focus moves to whatever is **underneath** — the cube when the panel overlaps it, the desktop when it does not | 4-case matrix (over cube / over desktop × with / without), each starting from the desktop focused so any focus change is attributable; `xdotool mousemove … click 1` then `getwindowfocus` |
| **B3** keyboard pass-through | **NOT TESTED.** Only pointer routing was measured. winit implements passthrough via the X Shape input extension, which affects pointer input; keyboard focus is a separate question | source: `winit-0.30.13/.../x11/window.rs` `set_cursor_hittest` uses `shape_rectangles(SET, INPUT, …)` |
| **B4** excluded from taskbar / window switcher | **FAILS on X11.** `with_taskbar(false)` is a no-op: no `_NET_WM_STATE_SKIP_TASKBAR`, window still appears in both `_NET_CLIENT_LIST` and `_NET_CLIENT_LIST_STACKING`, `_NET_WM_WINDOW_TYPE` stays `_NET_WM_WINDOW_TYPE_NORMAL` | `xprop` on the panel and on the root window, with and without the flag; root cause in source below |
| **B5** effect on the underlying app's fps | **NO MEASURABLE COST.** glxgears median **162.1 fps** with no panel, **161.1** with the panel at 1 Hz (−0.62%), **162.3** at 2 Hz (+0.12%). p1 = 158.3 / 158.5 / 158.5. Panel process CPU 4.4% at 1 Hz, 1.9% at 2 Hz, RSS ≈ 107 MB. All three deltas are inside the run-to-run spread | `glxgears` numeric output, 13 five-second samples per run, 60 s per run, one run at a time; **NOT** game-representative (see above) |
| **B6** which GPU does the panel render on? | **The AMD integrated GPU.** The panel maps `libgallium-26.0.8`, `libGLX_mesa`, `libGLdispatch` — no `libnvidia*`, no `libvulkan`, no `swrast`/llvmpipe, and no `dri/*.so` loaded at all | `grep` over `/proc/<panel-pid>/maps`; cross-checked against `glxinfo -B` |
| **B6** does the panel pull a discrete GPU out of runtime suspend? (criterion A2.5) | **PASSES.** Starting from a genuinely `suspended` dGPU, the panel running with `--data live` (which calls `GetGpuInfo` every poll) left `runtime_status=suspended` at all ten 3-second samples and 5 s after exit. A control with no panel behaved identically. A second control that polls `nvidia-smi` instead flipped the GPU to `active` within 6 s | 3-phase controlled test, monitoring **only** the non-invasive sysfs `power/runtime_status`; log `ov/b6c_controlled.log` |
| **B7** live font-scale change | **Geometry follows the config.** Same 4 items: 584 px wide at scale 1.0 → **903 px** at scale 1.6. 12 items at scale 1.0 → **1743 px**. Height 81 → 118 px with the larger font | `xdotool getwindowgeometry` per configuration; `b7-scale1-4items.png`, `b7-scale16-4items.png`, `b7-12items.png`, `b7-fontscale-transition.mp4` |
| **B7** live item reorder | **Size is unchanged by order** — reversed 4 items still measure 584x81, same as the original order | `xdotool getwindowgeometry`; `b7-reordered-4items.png` |
| **B7** flicker on resize | **NOT TESTED** as a subjective judgement. A resized window on a compositor-less X11 root is repainted by the server, and a still frame cannot distinguish "clean" from "one flash frame". The measurable part — the window reaching the correct final size with no stale or intermediate geometry — passes | `xdotool` geometry sampled after each change; MP4 clips recorded but no frame-by-frame flicker analysis was done |
| **B7** changing scale/order **while running** | **NOT TESTED.** `--scale` and `--items` are startup flags in the demo, so each configuration above is a **fresh launch**, not a hot change. Hot-reload behaviour in the real app is untested | demo source: the config is read once in `main()` |
| **B8** transparent background | **Transparency is applied, but shows black rather than the desktop.** Opaque run: `18,18,20` through the panel. `--transparent` run: `8,8,8` — the root window, not the wallpaper or the cube behind. Without a compositor there is nothing to blend against | paired opaque/transparent control runs, vertical pixel strip through the panel centre compared against a desktop reference sampled 40 px to the side; `b8-opaque-control.png`, `b8-transparent.png` |
| **B8** transparency with compositor enabled | **NOT TESTED** — no compositor available | as B2 |
| **B9** does runtime `WindowLevel(AlwaysOnTop)` hold over a fullscreen cube? | **YES — Q7 is solved.** A single runtime command set `_NET_WM_STATE_ABOVE`, and the panel stayed `PANEL-ON-TOP` through: focus returned to the cube, a click on the cube (focus confirmed on the cube), Alt+Tab away and back, and **5 minutes idle** (10 samples at 30 s). **No re-assertion needed.** Idle cost: 0.3% CPU, ~100 MB RSS | pixel verdict inside the panel's own rect, panel background colour learned at runtime rather than hardcoded; log `ov/b9_baseline.log`; `b9-runtime-aot-over-fullscreen.png` |
| **B10** `X11WindowType` variants vs the fullscreen cube | **All five types sit on top of a fullscreen cube.** `Dock`, `Utility`, `Toolbar`, `Desktop`, `Normal` → `PANEL-ON-TOP` in every case. **`Dock` and `Desktop` do not take focus and do not take clicks**; `Normal`/`Utility`/`Toolbar` do. All created at `744x81` with no oversized intermediate window across 11 samples at 0.5 s | `xprop` for type/state, `xdotool` focus before/after a click, pixel verdict; `ov/b10_b11.log` |
| **B10** `override_redirect(true)` | Window has **no `WM_STATE`, no `_NET_WM_STATE`, and is absent from `_NET_CLIENT_LIST`** — the WM no longer manages it. Sits on top of the fullscreen cube, takes no focus, takes no clicks. Correct behaviour, but the panel would be invisible to any WM-driven UI (alt-tab list, taskbar, raise-on-click) | `xprop` / `xdotool`; `ov/b10_b11.log` |
| **B11** `_NET_WM_STATE_SKIP_TASKBAR` by property write, after the window exists | **False positive.** The write succeeds — atoms present **1.8 ms** later: `_NET_WM_STATE_SKIP_TASKBAR, _NET_WM_STATE_SKIP_PAGER` — but on a *normal* window xfwm4 **strips them within seconds**. A 24 s sample at 2 s intervals never once saw them persist | `xprop -f _NET_WM_STATE 32a -set` from inside the egui loop, then continuous sampling; `ov/b11b_normal.probe.log` |
| **B11** same via ClientMessage | **Ignored.** `xdotool windowstate --add SKIP_TASKBAR` returned "not found" for `_NET_WM_STATE` on the normal window | same; `ov/b11_skip-msg.probe.log` |
| **B11** window **type** instead of a flag | **WORKS — Q8 is solved.** `X11WindowType::Dock` → xfwm4 itself grants `SKIP_TASKBAR` + `SKIP_PAGER` + `STICKY`, stable across the full 24 s sample. `Utility` and `Toolbar` also get SKIP_TASKBAR + SKIP_PAGER; `Desktop` adds STICKY | `ov/b11b_dock.probe.log`, `ov/b10_b11.log` |
| **B11** is `x11rb` available, and is the raw X window id reachable from eframe? | **Yes to both.** `x11rb 0.13.2` is already in `Cargo.lock` (transitively via `winit` and `arboard`) but is not a *direct* dependency of `gui`, so naming it needs a `gui/Cargo.toml` entry. The window id needs **no new runtime dependency**: `eframe::Frame` implements `HasWindowHandle` (`epi.rs:717`) and `Frame::window_handle()` gives a `RawWindowHandle::Xlib` whose `.window` is the X id | `cargo tree -i x11rb`; probe reads it on the first frame and logs it |
| **B12** cost of recreating the window to change its type | **79–89 ms with no panel on screen** (5 trials: 79, 89, 88, 86, 89 ms), and **no flash** — 40 geometry samples at 50 ms across the handover were `744x81` throughout, never oversized. The GL context and all in-process state would still be lost, which a single `ViewportCommand` does not cost | `date +%s%N` around the handover, geometry polled at 50 ms; `ov/b12_recreate.log` |
| **B12** does the tray survive a window recreate? | **NOT TESTED.** `panel_q78_probe.rs` has no tray, and the session bus showed only `org.kde.StatusNotifierWatcher` with no LapSphere `StatusNotifierItem` before or after — there was nothing to observe. Tray survival across an in-window mode switch is already covered by `panel-spike.md` §4(г) | `gdbus ListNames` on the session bus |
| **B13** real EWMH `_NET_WM_STATE` ClientMessage via **x11rb**, Normal window | **REFUSED — and the message is provably well-formed.** Sent to the root window, `type=_NET_WM_STATE`, `format=32`, `event_mask=SubstructureRedirect\|SubstructureNotify`, `data=[1, <atom>, 0, 0, 0]` — the exact layout winit's `set_netwm` uses (`winit-0.30.13/.../x11/window.rs`). After 60 s of sampling at 5 s intervals, `_NET_WM_STATE` was `_NET_WM_STATE_FOCUSED` at **every** sample; the window stayed in `_NET_CLIENT_LIST` and `_NET_CLIENT_LIST_STACKING`. The same binary, same code, same message, other window types: **utility → `SKIP_PAGER, SKIP_TASKBAR, FOCUSED`; dock → `+ STICKY`; toolbar → `SKIP_PAGER, SKIP_TASKBAR`; normal → nothing.** | x11rb `RustConnection::send_event`; `xprop`, `_NET_CLIENT_LIST`; `ov/b13.log`, `ov/b13_ctl_*.log` |
| **B13** comparison with a reference implementation | `wmctrl` is **not installed and not installable** (`apt-get install -s` fails: archive unavailable, and no network/sudo). The comparison was made against **winit's own `set_netwm`**, which is a better reference because B9 proved it works on this host: the first attempt used `data=[action, atom, 1, target, 0]`, was ignored, and was corrected to winit's `[action, atom, 0, 0, 0]`. It made **no difference** — the message is correct and is still refused for a Normal window | source: `winit-0.30.13/src/platform_impl/linux/x11/window.rs` `toggle_atom` / `set_netwm` |
| **B14** `Utility` + runtime `WindowLevel(AlwaysOnTop)`, fullscreen cube | **PASSES — the proposed Q8 combination works.** `_NET_WM_STATE` = `SKIP_PAGER, SKIP_TASKBAR, ABOVE, FOCUSED`, and the panel was `PANEL-ON-TOP` at every point: after F11 fullscreen (2560x1440), after focus returned to the cube, after a click on the cube, after Alt+Tab away and back, and across **5 minutes idle** (10 samples at 30 s). CPU 0.3%, RSS ~106 MB | pixel verdict with the panel background learned at runtime; `ov/b14_utility.log` |
| **B14** is the panel still clickable in `Utility` mode? | **YES.** A click inside the panel moved focus from the cube to the panel, and a click on an actual egui widget registered (5 `BUTTON-CLICKABLE` log lines across three click strategies: simple click, press-hold-release straddling a repaint, and hover-then-click). This is the property `Dock` lacks | `xdotool` focus before/after; widget click count in the probe log; `ov/b14_click.log` |
| **B15** full window (`Normal`) vs the same window as `Utility` in taskbar / switcher / panel | **Both appear in `_NET_CLIENT_LIST`.** Normal: 5 windows in the stacking list, state `ABOVE, FOCUSED`. Utility: **6 windows**, state `SKIP_PAGER, SKIP_TASKBAR, ABOVE, FOCUSED`. So `Utility` does **not** remove the window from the EWMH client list — xfwm4 only keeps it out of its own UI. **Taskbar and Alt+Tab visuals are NOT TESTED**: this XFCE session has **no taskbar plugin loaded** (nothing in the `xfce4-panel` windows ever showed a window button) and the alt-tab switcher window was not locatable by name, so only the EWMH evidence exists | `xprop -root _NET_CLIENT_LIST[_STACKING]`, window counts, `xdotool`; `b15-normal-desktop.png`, `b15-utility-desktop.png` |
| **B16** can eframe 0.34.2 recreate the **root** window via `ViewportBuilder::patch` → `recreate_window`? | **NO — the root window cannot be re-created or re-typed.** `ViewportBuilder::patch()` *does* return `recreate_window = true` for `window_type` (`egui/src/viewport.rs:912`), but eframe only acts on that flag inside `initialize_or_update_viewport()` (`eframe glow_integration.rs:1432`), which is reached exclusively for **Immediate and Deferred (child)** viewports. The root viewport is built once from `NativeOptions` at startup and never re-patched. Empirically: no `ViewportCommand` changes a root window's X11 type, and `ViewportCommand::Close` on the root ends the process rather than re-creating the window. **App state, GL context and RSS across a root recreate are therefore NOT APPLICABLE — the operation does not exist.** | source read of `egui` and `eframe`; runtime check that no command re-types the root |

| **H1** | Wine-exact `_NET_WM_STATE` ClientMessage: root window, `type=_NET_WM_STATE`, `format=32`, `mask=SubstructureRedirect\|SubstructureNotify`, `window=` **target window**, `data.l=[1, atom, 2nd-or-0, **1**, 0]`, one atom per message; atoms = SKIP_TASKBAR + SKIP_PAGER + `_KDE_NET_WM_STATE_SKIP_SWITCHER` | x11rb `send_event`, three `l[3]`/mask/`l[2]` variants, **sampled at t=+5 ms, +100 ms, +2 s, +10 s**; `xprop`, `_NET_CLIENT_LIST`, `WM_STATE` | **REFUSED on a NORMAL window — and `l[3]` is not the reason.** All four variants (`l[3]`=0/1 × `l[2]` packed/unpacked) left `_NET_WM_STATE` at `_NET_WM_STATE_ABOVE, _NET_WM_STATE_FOCUSED` at every sample from +5 ms to +10 s. The control — *identical code*, same message, **UTILITY** window — produced `SKIP_PAGER, SKIP_TASKBAR` at +5 ms and kept them at +10 s. So the message is well-formed and the WM refuses it by type. `ov/h1_wine.log` |
| **H1b** | cross-check against `wmctrl -b` on the *same* window | `wmctrl 1.07` (present on this host after all — see the correction below) vs our message, both orders | **wmctrl SUCCEEDS, we fail.** On one window: ours first → `_NET_WM_STATE_FOCUSED`; then `wmctrl -i -r -b add,_NET_WM_STATE_SKIP_TASKBAR` → `_NET_WM_STATE_SKIP_TASKBAR, _NET_WM_STATE_FOCUSED`. Reverse order: wmctrl sets it, ours leaves it. And `wmctrl -b remove` takes it back off. **The remaining difference is ours, not xfwm4's.** Not yet root-caused: candidates left open are the send-event mask (tested: both variants fail), `l[3]` (tested), and the exact byte/sequence framing. **This is an unresolved bug in our probe, not a property of the WM** — see "Open bug" below. `ov/h2_h4.log`, `ov/h1b_diff.log` |
| **H2** | withdrawn cycle: `Visible(false)` → wait for `WM_STATE=Withdrawn` → write `_NET_WM_STATE` as a property → `Visible(true)`; does xfwm4 re-read on show? | 3 cycles, geometry+`WM_STATE`+state sampled every **50 ms** (80 samples/cycle) | **The withdraw works, the trick does not.** The 50 ms series shows **30 `Withdrawn` samples** interleaved with 50 `Normal` — so `Visible(false)`/`Visible(true)` really does withdraw and re-map the window (this is the first confirmation that `Visible(false)` unmaps on X11 in this build). But after re-showing, `_NET_WM_STATE` is **empty every time**: xfwm4 re-reads (it never had the atoms, so there is nothing to preserve) and does not re-apply them. Position was preserved (`531,1240` before and after), focus returned to the panel, geometry constant at `850x159`, no flash visible in the series. **Switching window type `Normal ↔ Utility` this way is impossible** — it is not a `ViewportCommand`. `ov/h2_cycle.probe.log`, `ov/h2_geom_series.txt` |
| **H3** | add `SKIP_PAGER` + `_KDE_NET_WM_STATE_SKIP_SWITCHER`; does the KDE atom change the switcher? | message carrying only `_KDE_NET_WM_STATE_SKIP_SWITCHER` on a Normal window | **No effect.** `_NET_WM_STATE` unchanged at `ABOVE, FOCUSED`. It is a KDE-private atom and xfwm4 does not implement it; it would matter only on KDE, which is `NOT TESTED`. `ov/h1_wine.log` |
| **H4** | `WM_TRANSIENT_FOR` to a hidden 1x1 owner, as Wine does for windows with an owner | `xprop -f WM_TRANSIENT_FOR 32a -set` on a Normal window | **No effect on visibility.** With `WM_TRANSIENT_FOR = 0` set: `_NET_WM_STATE` still `ABOVE, FOCUSED`, still **in** `_NET_CLIENT_LIST`, and the window **still took focus** on appear. Note the test is weakened: the owner window could not be created through the x11rb 0.13 API, so `0` (None) was used rather than a real 1x1 window; a real owner might behave differently. `ov/h4.probe.log` |
| **H5** | panel as an **Immediate child viewport** with type Utility, root stays Normal | two windows of one PID enumerated separately; fullscreen battery + click test + 5 min idle | **WORKS, and it satisfies all four of the owner's goals.** One process, two native windows: `85983235` name `H5Root` type **`_NET_WM_WINDOW_TYPE_NORMAL`**, `_NET_WM_STATE` **empty**; `85983242` name `PanelChild` type **`_NET_WM_WINDOW_TYPE_UTILITY`**, `_NET_WM_STATE` = **`SKIP_PAGER, SKIP_TASKBAR, ABOVE, FOCUSED`**. Child stayed `CHILD-ON-TOP` after a genuine fullscreen cube, after focus returned to the cube, and through Alt+Tab both ways and **5 minutes idle**; RSS flat at ~105–107 MB; CPU 1.6–4.1 %. Child **clickable**: `CHILD-CLICK registered` ×3, focus moved to `PanelChild`. Window count stable at 9 — no leak over the run. `ov/h5_child.log` |
| **H6** | other levers: `WM_CLASS`, `WM_HINTS`, `_MOTIF_WM_HINTS`, `WM_NAME`, `_NET_WM_WINDOW_TYPE` ordering | `xprop` on a Normal window | **Nothing further.** `WM_CLASS = ("panel_h_probe","panel_h_probe")`, `WM_HINTS: not found`, `_MOTIF_WM_HINTS = 0x2,0x0,0x0,0x0,0x0`, `_NET_WM_WINDOW_TYPE = NORMAL`. None of these carry a "skip taskbar" meaning for xfwm4; the only lever is the window type. Multi-type lists were **not** tested (egui exposes `X11WindowType` as a single enum, not a list). |

| A2.4 absent data shows `—` in reserved width, window does not resize | **WORKS in the demo.** The synthetic producer drops the dGPU fields on every 5th snapshot and WiFi on every 7th; the panel renders `—` and the window geometry was unchanged across the run (584x81 for the whole session) | synthetic data mode + `xdotool` geometry sampled throughout; window size in the demo is computed only from the config, never from data |

## Root cause: why B2 failed, and why B9 fixed it

B2 found the panel covered by a fullscreen cube. B9 found the same setup
holding perfectly. The difference is **one line**, and it is a startup-vs-runtime
distinction:

| How the level is requested | `_NET_WM_STATE_ABOVE` | Result vs fullscreen cube |
| --- | --- | --- |
| `ViewportBuilder::with_window_level(AlwaysOnTop)` at startup | **never appears** | panel covered (B2) |
| `ViewportCommand::WindowLevel(AlwaysOnTop)` at runtime | appears and stays | panel on top for 5 min (B9) |

xfwm4 4.20.0 silently ignores the builder hint. The runtime command goes
through winit's `set_window_level`, which the WM honours.

This is the single most useful result in this document, because it converts a
"the primary use case does not work" finding into a one-line implementation
requirement: **request always-on-top after the window exists.** In
particular, "start directly in panel mode" — which ADR-1 proposes via
`ViewportBuilder` so that there is no large-window flash — must still set the
level on the first frame, or the panel comes up underneath the game.

No re-assertion was needed. The periodic re-assert mechanism was built
(`--reassert-sec`) and then went unused, which is the better outcome: a timer
that re-raises the panel every N seconds would be a busy-wait running against
a game.

## Root cause: why B4 failed, and what actually works

Two independent causes, both established from source and then confirmed.

**1. egui's `with_taskbar` is Windows-only.** `egui-winit-0.34.2/src/lib.rs:2003`:

```rust
#[cfg(target_os = "windows")]
{
    use winit::platform::windows::WindowAttributesExtWindows as _;
    if let Some(enable) = _drag_and_drop { … }
    if let Some(show) = _taskbar {
        window_attributes = window_attributes.with_skip_taskbar(!show);
    }
}
```

On Linux the value is destructured to `_taskbar` and discarded (same file,
line 1865). There is no X11 or Wayland equivalent.

**2. Writing the EWMH atom directly is a false positive.** Setting
`_NET_WM_STATE` by property write on a *normal* window appears to work — the
atoms are readable 1.8 ms later — but xfwm4 removes them within seconds, and
a 24 s sample never saw them persist. The ClientMessage route is ignored
outright. Had this been judged on the immediate read-back, it would have been
recorded as a success.

**3. Even a correct ClientMessage is refused for a Normal window.** B13 sent
a real EWMH message from Rust via `x11rb` — to the root window, `type=
_NET_WM_STATE`, `format=32`, `event_mask=SubstructureRedirect|
SubstructureNotify`, `data=[1, <atom>, 0, 0, 0]`, one message per atom
because a ClientMessage carries a single property. Over 60 s of sampling the
window's state was `_NET_WM_STATE_FOCUSED` at every single sample.

The message is not the problem, and this is established by a control rather
than asserted: the **same binary and the same code**, pointed at other window
types, produces

| Window type | `_NET_WM_STATE` after the identical message |
| --- | --- |
| `normal` | `_NET_WM_STATE_FOCUSED` (nothing added) |
| `utility` | `SKIP_PAGER, SKIP_TASKBAR, FOCUSED` |
| `dock` | `STICKY, SKIP_PAGER, SKIP_TASKBAR, FOCUSED` |
| `toolbar` | `SKIP_PAGER, SKIP_TASKBAR` |

So **xfwm4 4.20.0 grants SKIP_TASKBAR according to `_NET_WM_WINDOW_TYPE` and
refuses to grant it on request to a Normal window.** There is no message,
flag, or ordering that gets a Normal-typed window out of the taskbar on this
WM. Q8 without a window-type change is therefore **not possible here**, not
merely awkward.

Reference-implementation comparison: `wmctrl` is not installed and could not
be installed (`apt-get install -s wmctrl` reports the archive is unavailable,
and there is no network or sudo on this host). The comparison was made against
**winit's `set_netwm`**, which is a stronger reference because B9 proved that
path works on this host. The first attempt used
`data=[action, atom, 1, target, 0]` and was ignored; corrected to winit's
`[action, atom, 0, 0, 0]` it was **still** ignored for a Normal window — the
layout was never the issue.

**What works is the window type, not the flag.** `egui::X11WindowType::Dock`
makes xfwm4 grant `SKIP_TASKBAR` + `SKIP_PAGER` (plus `STICKY`) itself, and
those persist. `Utility` and `Toolbar` also get the two skip atoms; `Desktop`
adds `STICKY`.

| Type | SKIP_TASKBAR + SKIP_PAGER | Takes focus | Takes clicks | Above fullscreen cube |
| --- | --- | --- | --- | --- |
| `Normal` | no | yes | yes | yes (with runtime AOT) |
| `Utility` | **yes** | yes | yes | yes |
| `Toolbar` | **yes** | no | yes | yes |
| `Dock` | **yes** (+STICKY) | **no** | **no** | yes |
| `Desktop` | **yes** (+STICKY) | no | yes | yes |
| `override_redirect` | n/a — no `WM_STATE` at all | no | no | yes |

The `Dock` row is attractive for an overlay: it stays above the game *and*
ignores input, so a click can never disturb the game even without explicit
click-through. But it is conventionally a non-interactive screen-edge surface,
and on xfwm4 that means the panel's back button would not be clickable.
`Utility` is the compromise that keeps the panel interactive.

## B16: the root window cannot be re-created, so ADR-1's constraint is permanent

`ViewportBuilder::patch()` returns `recreate_window = true` when the X11
window type changes (`egui/src/viewport.rs:912`), and eframe *does* implement
window recreation — but only in `initialize_or_update_viewport()`
(`eframe glow_integration.rs:1432`), which is reached exclusively for
**Immediate and Deferred (child)** viewports. The root viewport is created once
from `NativeOptions` and is never re-patched.

Confirmed empirically as well: no `ViewportCommand` changes a root window's
X11 type, and `ViewportCommand::Close` on the root terminates the process
rather than re-creating the window.

The consequence is worth stating plainly, because it closes off a whole family
of designs: **"app state, GL context and RSS across a root re-create" are not
applicable, because that operation does not exist in eframe 0.34.2.** The
window type is a permanent property of the process, and the only way to change
it is to end the process and start a new one.

## H5: the winning design — two windows in one process

H5 answers the owner's actual goal, which the earlier rounds had narrowed to a
bad trade-off. In **one process**, egui gives us two native windows:

```
one process, two native windows
  window A  "H5Root"      _NET_WM_WINDOW_TYPE_NORMAL
                          _NET_WM_STATE:  (empty)          <- ordinary app window
  window B  "PanelChild"  _NET_WM_WINDOW_TYPE_UTILITY
                          _NET_WM_STATE:  SKIP_PAGER,
                                          SKIP_TASKBAR,
                                          ABOVE,
                                          FOCUSED         <- the panel
```

That satisfies all four of the stated goals at once:

| Goal | How H5 meets it |
| --- | --- |
| (a) hidden from taskbar / window switcher | the **child** is Utility, so xfwm4 grants `SKIP_TASKBAR` + `SKIP_PAGER` (B10/B11, confirmed again here) |
| (b) stays above a fullscreen game | the child is on top through a genuine fullscreen cube, focus return, Alt+Tab both ways and 5 minutes idle |
| (c) clickable | `CHILD-CLICK registered` ×3 — focus moved from the cube to `PanelChild` and the widget took clicks |
| (d) the full-size GUI stays an ordinary window | the **root** keeps `_NET_WM_WINDOW_TYPE_NORMAL` with empty `_NET_WM_STATE`, so nothing forces it to Utility |

Measured: RSS flat at ~105–107 MB over the 5-minute idle, CPU 1.6–4.1 %,
`_NET_CLIENT_LIST` stable at 9 windows from start to finish — no window or GL
leak over the run.

**This removes the "Utility for the whole process" cost that ADR-1 had
accepted.** The main window goes back to being a Normal window.

Costs, stated rather than glossed over:

* **A second native window and a second GL surface.** That was one of the
  arguments against a second window back in ADR-1, and it still applies: more
  memory, one more window to track, and the WM now has two windows for one
  application. Measured memory did **not** grow versus the single-window
  probe (both sit around 105–107 MB), which is reassuring but is one
  measurement on one host.
* **The panel window must be told to disappear when the panel is off.** The
  brief's H5 phrasing ("the root hides via `Visible(false)` while the panel is
  shown") was tested in this shape but **the hide/show of the *child* was not
  separately verified** — `NOT TESTED`.
* **Whether the child keeps painting when the root is hidden is `NOT
  TESTED`.** `ViewportInfo::visible()` derives from `minimized`/`occluded`
  only (panel-spike §1б), and a child viewport has no such guarantee in what
  was measured.
* Taskbar and Alt-Tab appearance is still `NOT TESTED` here — no tasklist
  plugin in this session. `panel-owner-check.sh` exists so the owner can
  settle it by eye.

## Open bug: our ClientMessage is wrong, wmctrl's is not

This is recorded because it changes what the H1 result means.

`wmctrl 1.07` **is** present on this host — contrary to B13's note that it was
absent and uninstallable. It became available during this round (a package
change outside our control), which is exactly the cross-check the earlier round
said it needed. The result is the opposite of what B13 concluded:

* `wmctrl -i -r <win> -b add,_NET_WM_STATE_SKIP_TASKBAR` **works on a
  Normal-type window** — added within 0.5 s, still present at +2 s and +10 s,
  and removable with `-b remove`.
* Our x11rb message, on the same window in the same session, is **refused**.

So xfwm4 4.20.0 **does** honour a well-formed request on a Normal window, and
B13's "xfwm4 refuses by type" conclusion is **wrong** — what is actually
refused is *our specific message*. Four variants were tested and all fail:
`l[3]` = 0 and 1, second atom packed into `l[2]` or not, and both send-event
masks (`SubstructureRedirect|SubstructureNotify` and wmctrl's
`SubstructureNotify`). The root cause is **not yet identified**; the remaining
candidates are the byte framing or the request sequencing, and this probe does
not yet have a root cause.

Consequences:

* **H1 stands as a negative result about our implementation, not about
  xfwm4.**
* **Q8 may well be solvable without changing the window type** — wmctrl
  proves a Normal window can be hidden from the taskbar by message alone on
  this WM. It is not solved yet, because we cannot send the message.
* The control in every H1 run (a Utility window) was never the real control;
  **wmctrl was.** Controls should have been run from the start.

Recorded as a fifth measurement error: the earlier rounds asserted a WM
behaviour ("refused by type") from a failure whose cause was in our own code.

## The H2 result in one line

`Visible(false)` genuinely withdraws the window (30 `Withdrawn` samples at
50 ms), and `Visible(true)` re-maps it with no geometry change, no position
loss and no visible flash — but writing `_NET_WM_STATE` while withdrawn does
not help, because the atoms never survive to the re-show. **Not a workaround.**

## What B12 says about ADR-1

ADR-1 keeps mode switching inside one process and treats a process restart as
a fallback. B12 tested the intermediate option it did not mention —
recreating the *window* to change its type:

* **79–89 ms with no panel on screen** (5 trials), and **no flash**: 40
  geometry samples at 50 ms across the handover were `744x81` throughout,
  never an oversized or intermediate window.
* It would still throw away the GL context and all in-process state, which a
  single `ViewportCommand` does not.

So recreation is cheap in time and still wrong in kind. B16 then showed that
for the *root* window the question is moot: eframe cannot re-create it at all,
so the type is a permanent property of the process.

B13 closed the last escape route: SKIP_TASKBAR cannot be obtained on a
Normal-typed window by any message or flag on xfwm4 4.20.0. The type is the
only lever, the type is permanent, therefore **one type must serve both the
full window and the panel**. B14 then measured the proposed compromise
(`Utility` + runtime `WindowLevel(AlwaysOnTop)`) and it passes on every point:
on top of a fullscreen cube, after focus returns, after Alt+Tab, across five
minutes idle, and still clickable.

The remaining trade-off is recorded as such in `panel-design.md`: `Utility`
makes the *full* window a `_NET_WM_WINDOW_TYPE_UTILITY` window for its whole
life, which is not what a normal application window should be. B15 quantifies
the cost: the window is still in `_NET_CLIENT_LIST` either way, so the EWMH
consequence is limited to the type atom itself; the taskbar and Alt-Tab
appearance is `NOT TESTED` because this XFCE session has no taskbar plugin.

## Two measurement errors, corrected

Recorded because they changed conclusions, and because both are easy to repeat.

**B6, first attempt — false positive.** An early run appeared to show the
panel waking the dGPU (`runtime_status` → `active`, pstate P0 at t+3 s). The
monitoring loop called `nvidia-smi` every 3 s, and querying NVML is itself
enough to wake a suspended dGPU — proved later by control phase 3. Re-run with
sysfs-only monitoring from a suspended baseline, the panel never woke it. The
original conclusion was an artifact of the instrument.

**B8, first attempt — flag not actually passed.** A capture labelled
`--transparent` was invoked without the flag, so it was a second opaque run
and showed no difference. Fixed by passing the flag and re-running as a
paired control; that is the result reported above.

**B9, first attempt — a hardcoded pixel colour made everything read
"OTHER".** The Q78 probe does not use the overlay demo's dark theme, so its
panel background is `27,27,27`, not the `18,18,20` the verdict function was
looking for. Every sample came back `OTHER(...)` and the run proved nothing.
Fixed by *learning* the panel's background colour at runtime, while the cube
is still windowed and the panel is known to be on top of it. A second, smaller
bug in the same area: the first "no re-assert" run never sent a
`WindowLevel` at all, because the initial send was inside the re-assert code
path — so the baseline was not actually testing what it claimed.

Also worth recording: B3 needed three attempts. The first was invalid because
the panel sat outside the cube, so a pass-through click landed on the
desktop; the second used a fullscreen cube, which on xfwm4 does not surrender
focus, making "click passed through" and "WM refused focus" indistinguishable.
The resolution was to start every case from the *desktop* focused, so any
focus change is attributable to the click.

## Reproducing

Single command, on a machine with a running `lapsphere-daemon` owning
`io.lapsphere.Control` on the system bus:

```bash
cd /home/wer/devis/lapsphere/repo
cargo run --release --example panel_overlay_demo -- \
  --data live --always-on-top --click-through \
  --x 200 --y 500 \
  --items hostname,cpu_load_percent,cpu_core_chart,cpu_temp,memory,battery,wifi_signal \
  --label cpu_temp=CPU --label memory=RAM
```

Useful flags (full list via `--help`, item table via `--dump-items`):

| Flag | Meaning |
| --- | --- |
| `--data live\|synth` | live daemon data, or synthetic (synth drops dGPU every 5th tick to exercise the `—` path) |
| `--scale <f32>` | global font scale; drives both the text and the computed window size |
| `--item-scale id=<f32>` | per-item font scale |
| `--items a,b,c` | item ids, **in display order** |
| `--label id=text` | override one item's label (repeatable) |
| `--refresh <hz>` | panel repaint rate |
| `--always-on-top` | `WindowLevel(AlwaysOnTop)` at startup (see B2 caveat) |
| `--click-through` | `ViewportCommand::MousePassthrough(true)` |
| `--transparent` | transparent background |
| `--skip-taskbar` | Windows-only on X11 (see B4) |
| `--x/--y` | initial outer position, in **points** |
| `--seconds <n>` | auto-exit |
| `--dump-items` | print the id → D-Bus source → width table and exit |

`panel_q78_probe.rs` (the Q7/Q8 probe) takes `--reassert-sec N`,
`--skip-taskbar-prop`, `--skip-taskbar-msg`, `--x11-type
normal|dock|utility|toolbar|desktop` and `--override-redirect`. It sends
`WindowLevel(AlwaysOnTop)` once at runtime, which is the finding of B9; note
it does **not** set the level in the builder, because that is the path B2
showed to be ignored.

To reproduce the Q7/Q8 experiments (B9–B16):

```bash
# Q7 via panel_q78_probe: runtime always-on-top over a fullscreen cube
cargo run --release --example panel_q78_probe -- --x 300 --y 700 --seconds 400

# Q8 window types and the two SKIP_TASKBAR routes (B10/B11)
cargo run --release --example panel_q78_probe -- --x11-type dock     --skip-taskbar-prop
cargo run --release --example panel_q78_probe -- --x11-type utility  --skip-taskbar-msg
cargo run --release --example panel_q78_probe -- --override-redirect

# Q8 the decisive test (B13): a real EWMH ClientMessage, then the control
# that shows the message is sound and xfwm4 refuses it for Normal
cargo run --release --example panel_b13_probe -- --b13 addremove --x11-type normal  --seconds 78
cargo run --release --example panel_b13_probe -- --b13 add       --x11-type utility --seconds 14

# Q7+Q8 combined, with a clickable widget (B14)
cargo run --release --example panel_b13_probe -- --x11-type utility --seconds 420
```

`panel_b13_probe` sends `WindowLevel(AlwaysOnTop)` once at runtime and draws a
large click target so a click can be aimed reliably — the first attempt used
a 46 pt-tall window and the button was clipped outside it, which is why the
first click test reported zero clicks.

`panel_h_probe` is the H1–H6 probe. `PROBE_X11_TYPE` selects the root window's
type (the child viewport in H5 is always Utility), `--mask` selects the send
mask, `--l3` the `data.l[3]` value, `--route wine|prop` the ClientMessage vs
XChangeProperty route, `--child-viewport` H5, `--transient` H4,
`--withdrawn-cycles`/`--max-cycles` H2:

```bash
# H1 with the required sampling points, plus the Utility control
cargo run --release --example panel_h_probe -- --l3 1 --mask both \
  --atoms "_NET_WM_STATE_SKIP_TASKBAR,_NET_WM_STATE_SKIP_PAGER" --seconds 14
PROBE_X11_TYPE=utility cargo run --release --example panel_h_probe -- --l3 1 --seconds 14

# H2 withdrawn cycles
cargo run --release --example panel_h_probe -- --route prop \
  --withdrawn-cycles --max-cycles 3 --seconds 45

# H5: root Normal, panel as an Immediate Utility child
cargo run --release --example panel_h_probe -- --child-viewport --no-aot --seconds 460
```

**Owner-side check:** `docs/development/panel-owner-check.sh` walks through the
taskbar and Alt-Tab behaviour with `wmctrl`, pausing for the owner to look.

To reproduce the fullscreen case, run the cube and toggle xfwm4's own
fullscreen binding:

```bash
vkcube --gpu_number 0 --width 900 --height 600 &     # --gpu_number 0 = iGPU
xdotool key --clearmodifiers F11
```

Note: `vkcube` without `--gpu_number 0` selects the NVIDIA dGPU on this host
and creates no usable X window; and `xdotool search --name Vkcube` does not
match it reliably (`WM_NAME` is `Vkcube X11`, and the window carries no
usable `_NET_WM_PID`), so window lookup walks the list and matches on name.

## Test scripts

Under `probe/` on the agent's machine, not committed to the repository:

| Script | Scenario |
| --- | --- |
| `start_cube.sh` | start vkcube detached, verify a window appeared |
| `b1_capture.sh` | B1 stills + B2 MP4 transition |
| `b2_final.sh` | B2 with a genuine F11 fullscreen |
| `b3_disambig.sh` | B3, 4-case click matrix |
| `b4_taskbar.sh` | B4 EWMH atoms |
| `b5_fps.sh` | B5 fps, three runs |
| `b6c_controlled.sh` | B6 3-phase runtime-PM control |
| `b7_b8.sh`, `b7_clip.sh` | B7 geometry + stills, B7 MP4 |
| `b8_decisive.sh` | B8 paired opaque/transparent control |
| `b9_aot_hold.sh` | **B9** — runtime AOT vs fullscreen cube, incl. the 5-minute idle wait |
| `b9_shot.sh` | B9 screenshot, with a fullscreen-geometry assertion |
| `b10_b11_matrix.sh` | **B10/B11** — window-type and override-redirect matrix, SKIP_TASKBAR routes |
| `b11b_persist.sh` | **B11** — does the SKIP_TASKBAR property survive on normal vs dock |
| `b12_recreate.sh` | **B12** — cost of recreating the window to change its type |
| `b13_clientmessage.sh` | **B13** — real EWMH ClientMessage via x11rb, 60 s stability |
| `b13_control.sh` | **B13** — the same message against four window types (the control that proves the message is sound) |
| `b14_utility.sh` | **B14** — `Utility` + runtime AOT: fullscreen battery + interactivity |
| `b14_click.sh` | **B14** — does a click on an egui widget inside the panel register |
| `b15_taskbar.sh` | **B15** — `Normal` vs `Utility` in the client list, plus stills |
| `b16_recreate.sh` | **B16** — can the root window be re-created / re-typed |
| `h1_wine.sh` | **H1** — Wine-exact ClientMessage, 4 variants, sampled +5 ms/+100 ms/+2 s/+10 s, with a Utility control |
| `h1b_diff.sh` | **H1b** — `wmctrl` vs our message on the same window, both orders |
| `h1c_variants.sh` | **H1c** — mask × `l[3]` variant grid on Normal, plus Utility controls |
| `h2_h4.sh` | **H2** withdrawn cycle + **H4** transient-for + `wmctrl` cross-check + **H6** hints |
| `h5_child.sh` | **H5** — panel as an Immediate child viewport; fullscreen battery, click test, 5 min idle |
| `watch_x11.sh` | generic PID-resolved X11 sampler |
| `shot.sh` | `xwd` → downscaled PNG (ImageMagick absent) |
