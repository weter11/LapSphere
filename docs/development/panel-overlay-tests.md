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

## What B12 says about ADR-1

ADR-1 keeps mode switching inside one process and treats a process restart as
a fallback. B12 tested the intermediate option it did not mention —
recreating the *window* to change its type:

* **79–89 ms with no panel on screen** (5 trials), and **no flash**: 40
  geometry samples at 50 ms across the handover were `744x81` throughout,
  never an oversized or intermediate window.
* It would still throw away the GL context and all in-process state, which a
  single `ViewportCommand` does not.

So recreation is cheap in time and still wrong in kind. The amendment proposed
in `panel-design.md` records the one real constraint B9–B12 exposed: the
`X11WindowType` is fixed at startup and cannot be changed by any
`ViewportCommand`, so it must serve both modes and has to be decided **before**
the panel work starts.

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

To reproduce the Q7/Q8 experiments (B9–B12) — this is the probe that closed
them:

```bash
# Q7: runtime always-on-top over a fullscreen cube, held for 5 minutes
cargo run --release --example panel_q78_probe -- --x 300 --y 700 --seconds 400

# Q8: window types, and SKIP_TASKBAR by property write vs window type
cargo run --release --example panel_q78_probe -- --x11-type dock      --skip-taskbar-prop
cargo run --release --example panel_q78_probe -- --x11-type utility   --skip-taskbar-msg
cargo run --release --example panel_q78_probe -- --override-redirect
```

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
| `watch_x11.sh` | generic PID-resolved X11 sampler |
| `shot.sh` | `xwd` → downscaled PNG (ImageMagick absent) |
