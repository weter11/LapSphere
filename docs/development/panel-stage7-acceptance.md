# Stage 7 — first-version final acceptance: the run, its evidence, and what it does not claim

The final acceptance of the integrated panel, performed on the **merged tree** at
`bbe0c4f6d573f544fc70bdaf8fe4fad5b1878be5` (= merge of PR #131, `origin/lapsphere`,
2026-10-15 23:28:46 +0000). This page is the auditable record: every verdict below
names the commit, the build, the environment and the raw artifact behind it.

**This run did not produce a `PASS` for stage 7, and it does not claim one.** Stage
6's live half was driven against a real Vulkan client, not against a real game — a
real game could not be brought to a rendering state in this environment, and that
blocker is named in §8.3 rather than summarised away. Stages 1, 2, 4 and 5 are
accepted on the evidence below; stage 3 is accepted **on an X11 window manager only**
and fails on the host's own session; stage 6 is accepted in part.

Companion pages: [`panel-v1-baseline.md`](./panel-v1-baseline.md) (status and
criteria), [`panel-measurements.md`](./panel-measurements.md) (the merged panel's
own numbers), [`panel-transparency.md`](./panel-transparency.md) (the stage-4
measurement and its own acceptance run), [`panel-frames.md`](./panel-frames.md) (the
six frame elements), [`panel-visibility.md`](./panel-visibility.md) (stage 5).

## 1. The tree under test, and how the layer under test was pinned

| | |
|---|---|
| Commit | `bbe0c4f6d573f544fc70bdaf8fe4fad5b1878be5`, `git log -1 --format=%s` = *Merge pull request #131 from weter11/feat/panel-frame-metrics* |
| Worktree | a fresh worktree at that SHA (`git worktree add … bbe0c4f`); `git status --porcelain -uno` = **0** tracked modifications for the whole run |
| Untracked | exactly seven files, all copies of PR #125's **test tooling** (see §3.1) — no product file is untracked or modified |
| Build | `cargo build --workspace --release` → exit 0 (29 warnings, all pre-existing) in 1m31s; rustc/cargo 1.98.0 |
| Binaries | `target/release/lapsphere` sha256 `c722c3155ebce3fefe42c5a6afb92c650a1ff044bd0c6bdbd5f8d31eac93df34` (23 807 192 B); `target/release/lapsphere-daemon` sha256 `e08841e05a6c7cb6d11ae1b362cd7e22a6756620ae2722c00f76d1cc9d6406ec` |
| Tests | `cargo test --workspace` → exit 0; **438 passed, 0 failed, 6 ignored** (324 + 7 + 107 + 0 across the four binaries) |

**The layer under test is the merged tree's source, pinned three ways.** PR #125 is
still open and its battery is the only complete measurement of the stage-1 gate, so
the question "did you test the merged source or the PR's source?" has to be answered
by bytes, not by intent:

1. `git diff bbe0c4f origin/spike/vk-layer-stage1b -- spike/vk-layer/src/{lib,shm,vkraw,buildid}.rs`
   is **empty** — the layer implementation is identical in the two trees.
2. `build.sh`'s source stamp (`sha256(lib.rs+shm.rs+vkraw.rs)`, first 12 hex) is
   `718336e1a120` here, which is exactly the stamp PR #125 recorded in its own build
   id (`514bba3ce7ed/718336e1a120`).
3. A pristine `git archive bbe0c4f` copy of `spike/vk-layer`, built on its own,
   produces `so_sha256 e5663ccae83f778b7eed1cc6b20624953fe1c55a23d2e49e07e6e96751ccc8ea`
   (`build_id nogit/718336e1a120`). The battery's build differs from it **only** by
   the compiled-in build-id string (`bbe0c4f6d573/718336e1a120+dirty7`, the `+dirty7`
   being the seven untracked harness copies), which is a build-time constant, not
   code.

## 2. Environment — the exact matrix this run happened in

| | |
|---|---|
| Host | AMD Ryzen 7 5800H, 16 threads, 32 GB, Ubuntu 26.04.1 LTS, kernel 7.0.0-38-generic, eDP-1 2560x1440 |
| Session | **Wayland**: `XDG_SESSION_TYPE=wayland`, `WAYLAND_DISPLAY=wayland-1` |
| Compositor / WM | **xfwl4 4.21.1+dev-fb8941f** (`xfwl4 --backend=tty`, pid 2653) |
| X11 | **Xwayland 2:24.1.10-1** on `DISPLAY=:0` |
| GPU | NVIDIA RTX 3070 Laptop GPU (GA104M), driver **615.71.09**; AMD Cezanne iGPU (RADV); llvmpipe 26.0.8 |
| Vulkan loader | `libvulkan.so.1.4.341` (`libvulkan1 1.4.341.0-1`), `mesa-vulkan-drivers 26.0.8-1ubuntu0.3` |
| Other instruments | mangohud 0.8.2~rc2-1 (also loaded as a layer in the same chain), xdotool 1:3.20160805.1, xfwm4 4.20.0-1build1, xterm, Xvfb, grim, xwd |
| CPU governor | `conservative` for the whole run |
| Layer installation | `~/.local/share/vulkan/implicit_layer.d/VK_LAYER_LAPSPHERE_frames.json` (the manifest `install.sh` writes; no root, nothing system-wide) |
| Test applications | `vkcube` (Vulkan, `--wsi xlib` on the host session and on Xvfb) for the layer and the frame elements; `xterm` as the screen-covering client for the window-behaviour checks; Portal Stories: Mel (appid 317400) for the real-game attempt (§8.3) |
| Throwaway sessions | `Xvfb :77 -screen 0 1920x1080x24` + `xfwm4 --display=:77`, a private session bus **and** a private system bus, and a scratch `HOME` — so the user's `panel.json`, `settings.json` and the real daemon were never touched |

**Two environment facts that shaped every live verdict below, both measured:**

* the host session's X11 window management is **not** a conventional EWMH window
  manager: the compositor's XWM advertises EWMH states it then refuses (§5.2), and
  it overrules `OuterPosition` requests (§4.2). Verdicts about *window behaviour*
  therefore come from the throwaway `xfwm4` session, and the host session's own
  behaviour is reported as what it is — a failure — not as an untested cell.
* the panel's window is 300x198 **points**, which on this session (Xft DPI 170) is
  **531x351 pixels** (`xwininfo`), and 531x450 px once the graph strip is on. Every
  geometry number below is quoted in the unit it was read in.

## 3. Stage 1 — the gate re-run on the merged source

### 3.1 What was run, and which parts of it are unmerged

The layer implementation is merged; the *measurement harness* is PR #125's and is
still open. That split is deliberate and is stated here so the evidence is not read
as "the merged tree contains this harness":

* **merged tree, unmodified**: `spike/vk-layer/{build.sh,install.sh,run.sh,verify.sh}`,
  `manifests/`, `src/{lib,shm,vkraw,buildid}.rs`, `src/bin/frames_reader.rs`
* **copied in as temporary test tooling** (untracked, never committed):
  `agreement.sh`, `bench.sh`, `overhead.sh`, `robustness.sh`, `run-battery.sh`,
  `src/bin/frames_synth.rs`, `src/bin/primitive_bench.rs`

The battery ran serial and alone on the host session (`run-battery.sh`'s own
discipline: M1/M3, agreement and reader-cost are timing measurements and must not
compete). Header recorded: loadavg 1.65 3.12 2.34, governor `conservative`,
`so_sha256 e39dfe6d…`, `build_id bbe0c4f6d573/718336e1a120+dirty7`.

### 3.2 Interception, inertness, manifests, wraparound — the merged `verify.sh`

`./verify.sh 0` and `VERIFY_LONG=1 ./verify.sh 0`: **`RESULT: all checks passed`**,
both times. The numbers, as printed:

| Check | Result |
|---|---|
| C1 | `vkQueuePresentKHR` entered **900x** for `vkcube --c 900`, exit 0, segment present during the run |
| C2 | final `intervals=899` for 900 presents (the first present of a cadence writes no interval); reader sampled 3 s apart 322 → 563; 562 intervals dumped by a second process; **p50 8.334 ms** (120.0 fps — MangoHud's `fps_limit=120` is set in the user's environment), min 0.485 ms, max 9.087 ms; segment unlinked at `vkDestroyInstance` |
| C3 | with `LAPSPHERE_FRAMES` unset: exit 0, **0** segments, no layer log, the loader log never names the layer as enabled |
| C3b | `LAPSPHERE_FRAMES=1 LAPSPHERE_FRAMES_DISABLE=1`: exit 0, 0 segments |
| C4 | the legacy `functions` manifest shape intercepts 900 presents too |
| C5 | after wraparound: **4096** intervals returned, **0** zero-valued slots, p50 8.334 ms |

### 3.3 Overhead, agreement, robustness — PR #125's harness, merged source

**Overhead, in-band (M1, the layer's own sample of its present-hook work).** Nine
runs, three per configuration; out-of-band floors from `primitive_bench` in the same
session (`clock_pair` 30 ns, `record_full` 40 ns, `mmap_read` 6612 ns):

| arm | median | p99 | p99.9 | max |
|---|---|---|---|---|
| vsync, MangoHud off, r1/r2/r3 | 0.151 / 0.191 / 0.160 µs | 1.343 / 1.373 / 1.553 µs | 16.611 / 14.998 / 16.932 µs | 20.920 / 51.167 / 18.095 µs |
| unthrottled, MangoHud off, r1/r2/r3 | 0.090 / 0.090 / 0.080 µs | 0.251 / 0.281 / 0.161 µs | 0.531 / 0.591 / 0.491 µs | 6.092 / 9.819 / 11.372 µs |
| vsync, MangoHud on, r1/r2/r3 | 0.231 / 0.211 / 0.451 µs | 1.262 / 1.273 / 1.423 µs | 10.430 / 13.626 / 13.555 µs | 16.120 / 27.101 / 15.148 µs |

Sample/interval pairs were exact (1999/1999, 4096/4096, 1899/1900, …), i.e. the
instrumented run yields one sample per recorded interval.

**Throughput (M3), unthrottled presents, slope between `--c 20000` and `--c 150000`:**

```
arm A rates 4417 4528 4239 fps (median 4416.9, spread 6.53%)
arm B rates 4546 4567 4299 fps (median 4545.8, spread 5.88%)
arm C rates 4544 4544 4526 fps (median 4543.8, spread 0.40%)
arm D rates 4550 4546 4519 fps (median 4546.1, spread 0.67%)
arm B - arm A: -6.42 -1.87 -3.29 us per present (median -3.288 us)
arm C - arm A: -6.32 -0.79 -14.93 us per present (median -6.324 us)
arm D - arm A: -6.61 -0.88 -14.62 us per present (median -6.613 us)
```

Read as PR #125 reads it: a **rate delta with the wrong sign, inside the 6.5 %
run-to-run spread of arm A itself** — a bound on the whole hook, not an overhead
figure. No frame time is quoted as overhead anywhere here.

**Agreement with MangoHud on the same presents** (MangoHud is below this layer in
the loader chain), two reps × two limiter configurations:

| run | MangoHud p50 / fps | layer p50 / fps | paired frame-duration difference (ours − MH) | p50 difference |
|---|---|---|---|---|
| limiter off, r1 | 6.0662 ms / 164.85 | 6.0661 ms / 164.85 | median +0.54 µs | within 1 % |
| limiter off, r2 | 6.0621 ms / 164.96 | 6.0627 ms / 164.94 | median **+0.50 µs**, p99 480.92 µs | **0.008 %** of the frame time |
| limiter 120, r1 | 8.3330 ms / 120.00 | 8.3337 ms / 120.00 | median +0.80 µs | 0.010 % |
| limiter 120, r2 | 8.3330 ms / 120.00 | 8.3337 ms / 120.00 | median +0.80 µs, p99 77.02 µs | 0.010 % |

Coverage was complete in every run (`outside_window=0`: 2469/2469 and 1799/1799
MangoHud frames compared). The gate's "median within 1 %" is met; p99 is read against
the run-to-run noise floor, as the design says it must be.

**Robustness (R1–R8), all on the merged layer:**

| | measured |
|---|---|
| R1 clean exit | exit 0, 900 presents, `writer_final: intervals=899 dropped=0 presents=900`, 0 segments left |
| R2 `kill -9` | wait status **137**, orphan segment readable (`frames 443`), `--prune` clears it, the next run is normal (exit 0, 600 presents, 0 segments left) |
| R3 live resizes | under Xvfb+lavapipe: 2500 presents, 4 swapchain creations, dumped 2452 intervals, p50 14.843 ms, and the crate's invariant **holds**: `2500 − 1 − 3 = 2496` = measured |
| R3b second swapchain, no resize | 800 presents, 2 creations → `800 − 1 − 1 = 798` = measured |
| R4 two instances | two live segments, two pids; SIGKILL of the second leaves the first presenting (560 further intervals) and exiting 0; the orphan is pruned |
| R5 no reader at all | exit 0, `intervals=899` for 900 presents — the layer does not depend on being read |
| R6 reader cost | see §3.4 — the merged tree's own reader cannot measure this; measured with PR #125's reader: **median 48.8 µs, p99 90.0 µs, max 116.7 µs** per poll at 60 Hz against a live writer, 0.25–2.0 % of one core across the arms |
| R7/R8 | fail-open writer creation and `SIGSTOP`/`SIGCONT`: no check failed in the section |

### 3.4 One measurement the merged tree cannot make, and why it matters here

The merged `spike/vk-layer/src/bin/frames_reader.rs` is the pre-#125 reader, and its
`--watch` "read cost" window sits *inside* the read: the mmap+copy of every segment
is outside the timestamps, so it reports ~0.1 µs. Measured both ways against the
same live writer and the same layer `.so`:

```
fixed reader (PR #125, built against the merged layer in a scratch copy):
  hz=60 samples=480 wall=8.00s seqlock_retries=1 max_frame_count=1705
  read cost: median 48.8 us  p99 90.0 us  max 116.7 us
merged tree's own reader:
  hz=60 samples=480 wall=8.00s seqlock_retries=0 max_frame_count=3860
  read cost: median 0.1 us  p99 0.1 us  max 0.2 us
out-of-band floor (primitive_bench, same session): mmap_read median 6612 ns
```

So the gate's reader-cost item is met on the merged *layer* (48.8 µs median, inside
PR #125's recorded 27–98 µs band), while the merged *instrument* under-reports it by
~500x. This is spike tooling, not product code: the GUI's own reader
(`gui/src/panel/frames/`) is a different implementation and reports no cost at all.
It is recorded here because a reader-cost number quoted from the merged tree would be
wrong by two orders of magnitude.

### 3.5 One environment anomaly, recorded rather than smoothed over

The first two `vkcube` runs of this session presented at **~1.4 fps** (53 and 56
presents in 30 s and 40 s, the process burning no CPU — blocked in present) and
every run after them presented at **139–163 fps** with the identical command on the
identical session. It did not recur during the battery and is not a property of the
layer (the same stall appeared with the layer unset). It is recorded because it is
the kind of thing that turns a timing measurement into a false negative if it lands
mid-battery.

### 3.6 What stage 1's acceptance covers, and what it still does not

Covered: interception, interval plausibility, inertness both ways, both manifest
shapes, wraparound, overhead, agreement with an independent instrument, and the
robustness battery — all on the merged source, on this host, on `vkcube`.

**Not covered, unchanged from PR #125 and still true:** the pid-reuse `O_EXCL`
variant (needs a pid namespace, unprivileged ones are unavailable here), 240 Hz as a
*present* rate, an interactive resize on the host session (the host session offers no
window control to `xdotool`/`wmctrl` — measured again in R3: `wmctrl lines=0`,
`xdotool windows (any class)=0` while an xlib `vkcube` runs), and **every workload
other than `vkcube`**: no DXVK/Proton/Wine, no OpenGL, no 32-bit, no anti-cheat, no
GPU-bound title. §8.3 is what happened when that last gap was attacked.

## 4. Stage 2 — position: save, restore, clamp

### 4.1 On a real EWMH window manager (throwaway `:77`, Xvfb 1920x1080, xfwm4 4.20)

| Step | Evidence |
|---|---|
| restore on start | stored `724,12` → `window spec: 300x198, min 300x198, decorations false, position 724,12` → `panel position: 724,12 applied as requested`; `xwininfo` `+724+12` |
| **drag with the WM** | XTEST Alt+drag from the window's centre: `xwininfo` `+724+12` → **`+370+281`**; the app's own watch logs `panel position: settled at 370,281` then `panel position 370,281 written to panel.json`; the file holds `{"x":370.0,"y":281.0}` |
| **restart** | window comes up at `+370+281` after a full process restart; `position 370,281 applied as requested` |
| **clamp** | `panel.json` hand-set to `5000,5000` → `panel position: stored 5000,5000 clamped to 1620,882 for a 1920x1080 monitor`; window at `+1620+882`; the file is re-written with the applied `1620,882` |

The stored position is taken from the window's **actual** position, not from the
request: after the drag the file holds the observed `370,281`, which is the property
`panel-measurements.md` asked for when it recorded the archived branch's failure.

### 4.2 On the host session (`:0`, Xwayland under xfwl4) — the request is overruled

```
panel position: the window is at 286,12, not the requested 0,0 (clamped or overruled by the window manager)
panel position 286,12 written to panel.json
```

and, on a second start, requested `866,141` → observed `724,12`. This is the same
behaviour the earlier transparency run recorded (`-3,12`, `-3,141`, `866,12` for the
same request). **The panel's position is not stable across runs on this session, and
no code change is proposed here** — the compositor's XWM declines `OuterPosition`
requests for Xwayland clients; the app records what actually happened, which is the
honest half of the criterion.

**Stage 2 verdict: PASS on X11 (xfwm4 4.20, Xvfb, 1920x1080): drag, persistence
across a restart and clamping are all verified by read-back. On the host's own
session restore is overruled by the compositor (NOT a pass there), and multi-monitor
placement remains unimplemented and unclaimed.**

## 5. Stage 3 — window behaviour: always-on-top and taskbar/switcher

### 5.1 On `:77`/xfwm4 — the combination, over a borderless-fullscreen window

```
panel EWMH state: _NET_WM_STATE_ABOVE, _NET_WM_STATE_SKIP_TASKBAR, _NET_WM_STATE_SKIP_PAGER
                  confirmed on window 0x400004 after 3 attempt(s)
_NET_WM_STATE(ATOM) = _NET_WM_STATE_SKIP_PAGER, _NET_WM_STATE_SKIP_TASKBAR, _NET_WM_STATE_ABOVE, _NET_WM_STATE_FOCUSED
```

With a **borderless-fullscreen** client running — an `xterm` at
`_NET_WM_STATE_FULLSCREEN`, geometry `1920x1080+0+0`, activated so the WM raised it —
`_NET_CLIENT_LIST_STACKING` returned `0xa00002, 0xc0000c, 0x400004`: the fullscreen
client in the middle, **the panel last, i.e. topmost**. Activating the fullscreen
window did not lift it above the panel.

The covering client here is an `xterm`, **not a game**: a Vulkan client cannot
present on this Xvfb display, and both attempts were measured rather than assumed —
`vkcube --wsi xlib` printed `MESA: info: vulkan: No DRI3 support detected - required
for presentation` and **never created a window** (killed by its timeout, exit 124),
and the lavapipe path (`VK_ICD_FILENAMES=…/lvp_icd.json`, which did select
`llvmpipe (LLVM 21.1.8, 256 bits)`) also never created one (exit 124). The real-game
version of this check is §8.3's blocker, not a silent substitution.

### 5.2 On the host session (`:0`, xfwl4) — both halves fail, measurably

| Half | Result |
|---|---|
| always-on-top | the app's request is refused: `panel EWMH state: could not apply _NET_WM_STATE_ABOVE, _NET_WM_STATE_SKIP_TASKBAR, _NET_WM_STATE_SKIP_PAGER to window 0xc00003 after 6 attempt(s); the window manager did not accept the request…`. Independently, `xprop` shows `_NET_WM_STATE_ABOVE` **did** land — the refusal is the other two atoms (see defect C) |
| taskbar exclusion | **refused.** `wmctrl -i -r <win> -b add,skip_taskbar` (single atom, source 2) leaves `_NET_WM_STATE` unchanged and the window in `_NET_CLIENT_LIST`. xfwl4 **advertises** `_NET_WM_STATE_SKIP_TASKBAR` and `_NET_WM_STATE_SKIP_PAGER` in the root's `_NET_SUPPORTED` and does not apply them |
| behavioural consequence | with the panel running, the top panel (xfce4-panel) shows a **LapSphere** button; with the process stopped there is no LapSphere button. Committed captures: [`taskbar-with-panel.png`](./panel-stage7-acceptance/taskbar-with-panel.png) / [`taskbar-without-panel.png`](./panel-stage7-acceptance/taskbar-without-panel.png) — both are the *same* `grim -g "0,0 2560x64"` pair, cropped to the region that differs between them (found by differencing the pair, not guessed) so that the rest of the user's taskbar is not published |
| stacking | with `_NET_WM_STATE_ABOVE` set, `_NET_CLIENT_LIST_STACKING` still lists another X11 client above the panel (`0x140001e, 0x800003`) — the atom is set and the compositor's own order does not follow it. A screen capture during that run did not show that other client where its X geometry said it was, so the *visual* half of this cell is inconclusive on this session rather than disproved |

**Stage 3 verdict: PASS for the mechanism and for the combination over a
screen-covering fullscreen window on X11 (xfwm4 4.20). FAIL on the host's own
session** — the compositor's XWM refuses the taskbar/pager atoms (so the panel does
appear in the taskbar) and does not stack the panel above another client. A pass in
this area says nothing about transparency or frame metrics, and vice versa.

## 6. Stage 4 — transparency, exercised live

Session: the host's `:0` (Xwayland under xfwl4 — a compositing session, which is
what this criterion needs). Method: `xwd` on the panel's own window (an ARGB surface,
`Depth 32`) and a per-pixel alpha histogram of 186 381 pixels; the analyser is in
§11.

| `background_opacity` | surface measurement |
|---|---|
| 1.0 | **alpha 255 in 100.00 %** of 186 381 pixels; the fill is `(28,29,32,255)` at 97.28 % |
| 0.5 | the fill is `(14,15,16,**128**)` at 97.28 % — **exactly the premultiplied half** of the 1.0 fill; brightest text pixels `(220,220,220,**255**)`, body text `(137,138,138,204)`; 181 310 pixels at alpha 128 vs 150 at 255 |

The text does not inherit the background opacity: its own alpha (204/255, and 255 at
the glyph cores) is **above** the background's 128 — the falsifiable form of "the
text does not fade". The premultiplication is correct too: a 0.5 fill is half the
1.0 fill in every colour channel, which is what a compositor applying
`src + dst·(1−a)` needs.

The mechanism is unchanged since the stage-4 acceptance: `git diff 565930e bbe0c4f --
gui/src/panel/window.rs gui/src/main.rs gui/src/app.rs | grep -E '^[+-].*(transparent|alpha|opacity|clear_color)'`
is **empty**. That is why the screen-level blend measurement (the green-backdrop
capture of `panel-transparency.md`) was **not repeated** on this commit: on this
session the compositor would not stack the panel above a screen-covering X11 client
(§5.2), so the backdrop could not be put behind the panel for a repeat. The earlier
measurement stands on its own page for its own commit; it is not copied into this one.

**Stage 4 verdict: PASS** for the criterion this run can test on `bbe0c4f` — a real
translucent surface whose alpha is the configured opacity, with text that keeps its
own. **NOT TESTED here:** the composited screen blend on this commit (blocked by
§5.2), and every compositor other than xfwl4's Xwayland.

## 7. Stage 5 — the hotkey, the D-Bus toggle, and hide-safety

On `:77` (Xvfb+xfwm4) and on `:0` for the bus path. Note on the harness: `xdotool key F9`
on this host injects **Alt_L** (keycode 64) instead of F9 (keycode 75) — measured with
`xev` — so every key press below is XTEST at the keycode level
(`XTestFakeKeyEvent`), the same instrument the stage-5 acceptance used.

| Claim | Result | Evidence |
|---|---|---|
| the configured key is grabbed | **PASS** | `panel: grabbed keycode 75 under none/capslock/numlock/capslock+numlock/scrolllock/… (mask 0x0001 … 0x0093)` — all eight lock combinations — then `panel: hotkey \`Shift+F9\` captured (keycode 75, modifiers 0x0001)` and `panel: watching keycode 75 on the connection that holds the grab` |
| a press hides and a second shows | **PASS** | XTEST `Shift+F9`: `IsViewable` → `IsUnMapped` → `IsViewable`, with `panel: hotkey pressed`, `panel: hidden; the hotkey, the tray or \`lapsphere --toggle-panel\` brings it back`, `panel: shown again` |
| a hidden panel still hears the key | **PASS** | the second press arrived while the window's map state was `IsUnMapped` |
| the D-Bus object is reachable through the name that owns it | **PASS** | `busctl --user tree io.lapsphere.Gui` → `/io/lapsphere/Gui/Panel`; introspect lists `io.lapsphere.Gui.Panel.TogglePanel` |
| `--toggle-panel` reaches the running process | **PASS** | `LapSphere: panel toggled`, exit 0, and the map state flips within **~50 ms** (polled, not sampled once); the process's log shows `panel: TogglePanel requested over D-Bus` |
| no GUI on the bus is reported as such | **PASS** | on an empty private bus: `LapSphere: no running LapSphere GUI owns io.lapsphere.Gui`, **exit 1** |
| the key can be turned off | **PASS** | `hotkey:""` → `panel: no hotkey configured; the tray and \`lapsphere --toggle-panel\` still work`; an XTEST `Shift+F9` changed nothing; the D-Bus path still hid and showed the panel, and the hide line becomes the other sentence: `panel: hidden; the hotkey is not a reliable route in this state, so the tray or \`lapsphere --toggle-panel\` is what brings it back` |

**Stage 5 verdict: PASS** for the routes a live run on this host can reach. **NOT
TESTED / not claimed:** a tray route (no StatusNotifier host in either session, so
`tray_enabled` was false in every run), the partial-grab (`BadAccess`) live case (the
stage-5 page's session D covers it; it was not repeated here), and every Wayland
claim — a native Wayland client never routes its keys through the X server.

## 8. Stage 6 — the six frame elements, live

### 8.1 The source, and what it is not

The live source was **`vkcube`** with the layer installed and `LAPSPHERE_FRAMES=1`
— a real Vulkan client whose presents go through the layer under test, vsync'd at
~165 Hz on the host session. **It is not a game.** §8.3 records what happened when a
game was attempted; this section must not be read as that run.

### 8.2 What the panel showed, read off its own surface

Captures are `xwd` of the panel window, converted to PNG (`xwd2png.py`, §11) and read
by eye; both are committed beside this page. The row list is the config's: five
hardware rows plus the six frame elements.

With the live source running (window 531x450 px — 11 rows including the strip;
committed capture [`panel-frame-rows-live.png`](./panel-stage7-acceptance/panel-frame-rows-live.png)):

```
CPU —   Load —   GPU —   Load —   Usage —
Game   vkcube
FPS    164.8 fps
Frame  6.07 ms
Graph  <a drawn line: flat with small upward spikes>
1%low  131.2 fps
0.1%   98.4 fps
```

After the source exited (killed, then 6 s later; committed capture
[`panel-frame-rows-source-gone.png`](./panel-stage7-acceptance/panel-frame-rows-source-gone.png)):

```
Game —   FPS —   Frame —   Graph <empty strip, no line>   1%low —   0.1% —
```

So: the game name, a changing FPS and frametime, both lows and a drawn strip are
real and come from the layer's segment; and an absent source is `—` with an empty
strip — **no zero line and no stale number**, exactly as `panel-frames.md` claims.

The availability announcement is per *source*, not per poll:
`panel frames: reading pid 1948481` appears **once** for the run, not four times a
second. The reader's own worker is off the rendering path; the panel repaints at the
configured 4 Hz.

### 8.3 The real-game attempt, and the exact blocker

**What was attempted.** Portal Stories: Mel (appid 317400) through the user's own
launcher pipeline — `steamflow test-launch 317400`, which runs the game as
`SteamLinuxRuntime_4/run … steamflow-proton-11.0-purepe/proton run … portal2.exe`
with `-windowed -noborder -width 1280 -height 720`. The layer's activation variable
was injected through SteamFlow's own env block (`~/.config/SteamFlow/debug.json`,
backed up and **restored** afterwards).

**Why that pipeline and not the Steam client.** The Steam client binary cannot run on
this host at all: `env LAPSPHERE_FRAMES=1 DISPLAY=:0 ~/.steam/steam/ubuntu12_32/steam
-silent` exits **127** with `env: '…/ubuntu12_32/steam': No such file or directory`,
and `/lib/ld-linux.so.2` — the interpreter that 32-bit ELF needs — is absent
(`ls: cannot access '/lib/ld-linux.so.2'`). `steam -applaunch` was therefore not
available as a route, and the game was launched through SteamFlow's own pipeline,
which runs its own Windows Steam runtime inside Wine (`steamflow test-steam` brings up
`steam.exe` + `steamwebhelper.exe` in the master prefix).

**What is proven to work.** The implicit layer **is** found and loaded inside the
container, and the variable does reach the game: the game's log contains six lines of

```
[lapsphere-frames] negotiate offered=2 answered=2 build=bbe0c4f6d573/718336e1a120+dirty7
```

and `/proc/<pid>/environ` of the container and game processes shows
`LAPSPHERE_FRAMES=1` with `XDG_RUNTIME_DIR=/run/user/1000` — the **same** runtime
directory the panel reads, so the "the game is in a container, the panel will not see
its segment" hypothesis is **falsified**, not assumed.

**The blocker.** The game never reaches a rendering state: no game window appears on
`:0` within 2 minutes, no `frames-<pid>` segment is created (the layer creates one at
instance creation), the layer's own `spike.log` is untouched, and the game exits by
itself. This install of the title carries an RTX Remix shim (a `.trex` process is in
its process tree), and the earlier record for this mod stack on this host is a
white-screen/early-exit failure — consistent with what was observed here, and outside
this acceptance task's scope to fix.

**Verdict for the real-game half of stage 6: NOT TESTED — blocked**, with the
reproduction above. It is not reported as a pass, and no game other than this one was
substituted. What the layer does inside a *containerised, DXVK-based* game process is
now known (it loads and negotiates); what it does with that game's presents is not.

### 8.4 The panel's own cost with the frame rows enabled

Same method the repo's own page uses (`/proc/<pid>/stat` utime+stime over 60 s), both
arms against the **same** live source so the difference is the frame rows:

| arm | jiffies in 60 s | jiffies/s | window |
|---|---|---|---|
| frame rows **off** | 13 | 0.22 | 531x186 px |
| frame rows **on** (6 rows + strip, reader at 4 Hz) | 27 | 0.45 | 531x450 px |

**The frame rows cost about +0.23 jiffies/s ≈ +0.23 % of one core**, and the panel
remains an order of magnitude below the normal window's 8.99 jiffies/s recorded in
`panel-measurements.md`. Is the 4 Hz graph useful? At 4 Hz the numbers move visibly
(164.8 fps against a 120 fps limiter is a different reading within a second) and the
strip is decimated peak-preserving, so a hitch survives; the cost of that cadence is
the 0.23 jiffies/s above. This is a *reading* of a measured number, not a product
decision, and the cadence stays a proposal (`panel-frames.md` §4).

**Stage 6 verdict: accepted in part.** The six elements are verified live against a
real Vulkan client: name, changing fps/frametime, both lows, a drawn strip, `—` and an
empty strip when the source goes away, one announcement per source, and a measured
panel cost. **NOT TESTED: the same elements over a real game** (§8.3), and the strip's
behaviour over a game whose frame rate varies by an order of magnitude.

## 9. Stage 7 — the overall verdict

| Stage | Verdict on `bbe0c4f` | Where the evidence is |
|---|---|---|
| 1 — fps/frametime research through the Vulkan layer | **GO gate MET on the merged source** (interception, overhead, agreement, robustness re-run; the pid-reuse variant, 240 Hz presents and non-`vkcube` workloads stay uncovered) | §3 |
| 2 — position save/restore | **PASS on X11**; overruled by the compositor on the host session | §4 |
| 3 — always-on-top + taskbar/switcher | **PASS on X11** (both halves together, over a fullscreen window); **FAIL on the host session** (atoms refused; the panel is in the taskbar) | §5 |
| 4 — real transparency | **PASS** (surface alpha is the configured opacity; text keeps its own) | §6 |
| 5 — hotkey and D-Bus control | **PASS** (grab, both directions, hide-safety sentence, bus errors, key disabled) | §7 |
| 6 — metrics integrated into the panel | **accepted in part**: live against a real Vulkan client, cost measured; **the real-game run is NOT TESTED (blocked)** | §8 |
| 7 — final acceptance of the first version | **INCOMPLETE — not PASS.** Stage 6's real-game half is blocked (§8.3) and stage 3 does not hold on the host's own session (§5.2) | this page |

The three independent areas stay separate, as the criteria require: **transparency**
is accepted in §6 on its own surface measurement, **window behaviour** is accepted in
§5.1 for X11 and fails in §5.2 for the host session, and **fps/frametime** is accepted
in §3 (layer) and §8 (elements) with the real-game gap named. A pass in one of them
carries nothing into the others.

## 10. Defects found live, with reproduction

### A. A hide/show cycle loses the panel's place and destroys the stored position

Reproduced on `:77`/xfwm4 with the merged binary, by both the hotkey and the D-Bus
path:

```
T0  panel at 500,300   panel.json {"x":500,"y":300}   _NET_WM_STATE = SKIP_PAGER, SKIP_TASKBAR, ABOVE, FOCUSED
    XTEST Shift+F9  ->  T1  IsUnMapped, file still 500,300, _NET_WM_STATE gone ("not found")
    XTEST Shift+F9  ->  T2  window at 0,0
                            panel position: settled at 0,0
                            panel position 0,0 written to panel.json
```

The panel returns at the top-left of the monitor and the *stored* position is
overwritten with that bogus value, so the loss survives the next restart. The show
path emits no `window spec:` and no new `panel EWMH state:` line, i.e. the mode spec
is not re-applied.

*Minimal remediation to consider (not done here):* treat a position read while the
window is unmapped as "no observation" — the watch should refuse a value that arrives
while `Visible(false)` is in effect (and re-apply the mode spec on the show path), so
the stored position keeps the last *mapped* value.

### B. The EWMH state is not re-requested after a hide/show

Same trace, T2: the window comes back with `_NET_WM_STATE = _NET_WM_STATE_FOCUSED`
only — `ABOVE`, `SKIP_TASKBAR` and `SKIP_PAGER` are gone, and the app does not ask
again (the worker ran once, at startup). After any hide/show the panel is silently no
longer always-on-top and is back in the taskbar, even on a window manager that
honours those atoms.

*Minimal remediation to consider:* re-request the mode's atoms on the transition back
to visible — the same call the startup path already makes, on the show path.

### C. A partial refusal is logged as a total one

On the host session the app logs

```
panel EWMH state: could not apply _NET_WM_STATE_ABOVE, _NET_WM_STATE_SKIP_TASKBAR, _NET_WM_STATE_SKIP_PAGER
                  to window 0xc00003 after 6 attempt(s); the window manager did not accept the request, so the
                  window keeps the state it has
```

while `xprop` shows `_NET_WM_STATE_ABOVE` **did** land — the refusal is the two
`SKIP_*` atoms. The loop's verdict is a set-level "all or nothing", and the message
names the whole wanted set, so a reader cannot tell which half worked. This is a
reporting defect; it cost time in this very run.

*Minimal remediation to consider:* log the atoms that are still missing (the loop
already computes them) instead of the wanted set.

## 11. Raw artifacts, and where they are

**Committed with this page** (the evidence a reviewer cannot re-derive from the text):

| Artifact | Path |
|---|---|
| The taskbar pair behind §5.2 — the panel's button present and absent, cropped to the region that differs between the two captures | [`panel-stage7-acceptance/taskbar-with-panel.png`](./panel-stage7-acceptance/taskbar-with-panel.png), [`taskbar-without-panel.png`](./panel-stage7-acceptance/taskbar-without-panel.png) |
| The panel's own surface behind §8.2 — the six elements live, and `—` with an empty strip after the source exits | [`panel-stage7-acceptance/panel-frame-rows-live.png`](./panel-stage7-acceptance/panel-frame-rows-live.png), [`panel-frame-rows-source-gone.png`](./panel-stage7-acceptance/panel-frame-rows-source-gone.png) |

**Local to the machine the run happened on, and deliberately not committed.** For each
one, the record above already quotes the output the artifact carries, so a reviewer is
not asked to take a file on trust:

| Artifact | Where it is | What it carries, quoted above |
|---|---|---|
| Stage-1 battery console, per-check output, battery header | `spike/vk-layer/results/stage7-*.txt`, `results/{verify,overhead/overhead.txt,agreement/agreement.txt,robustness/robustness.txt,primitives.txt,stage7-battery-header.txt}` | every PASS/FAIL line and every number in §3 — these are build/test output and are not committed |
| Reader-cost addendum | the transcript of §3.4 | the fixed-vs-merged reader medians; the scratch crate copy it ran in is not part of the tree |
| `:77` session harness and per-step output | not in the tree | the commands and their raw output are quoted in §4, §5.1 and §7 |
| Transparency alpha histograms (`xwd` → per-pixel alpha) | `/tmp/panel-100.xwd`, `/tmp/panel-050.xwd`; analyser `xwdalpha.py` | the alpha histograms and pixel values in §6 |
| Full-screen captures from the stacking cell (the green-backdrop pair and the full taskbar strip) | `/tmp` and the run's scratch directory | they contain the user's unrelated desktop; only the cropped, differenced pair above is published |
| Cost run | the transcript of §8.4 | jiffies per arm and the resulting +0.23 jiffies/s |
| Real-game attempt | `/tmp/steamflow-launch2.log`, `~/.config/SteamFlow/logs/wine_317400.log` | the six `[lapsphere-frames] negotiate` lines and the game's exit, quoted in §8.3 |

The analysers used here (`xwdalpha.py`, `xwd2png.py`, an XTEST injector, the session
harnesses) are throwaway instruments kept outside the repository; they are described
in enough detail above to be re-created, and none of them is committed. No build
output and no harness copy is committed — the four PNGs above are the whole of what
this change adds to the tree besides text.
