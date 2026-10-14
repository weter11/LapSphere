# Panel, first version — baseline and acceptance criteria

What is merged, what exists only on an unmerged branch, what has evidence, and what
still has to be done before the panel's first version can be called accepted.

**Base:** `17ece901754fb2cfcaf0b509dcb8d56e3639eea6` (= merge of PR #122,
`origin/lapsphere`, 2026-10-14 10:11 +0200). Re-check it before quoting anything
here; every path and line number below is as of that commit.

This page **records status; it does not settle design**. Where the history
disagrees with itself, both sides are quoted and the disagreement is left open.
Nothing on this page was re-measured to write it, and no number below is
re-confirmed by it.

Companion pages: [`panel-measurements.md`](./panel-measurements.md) holds the
measurements taken for the merged panel (PR 1) and their own limits.

## 1. Baseline — what is merged

| Item | Where | State |
|---|---|---|
| Panel, part 1 (this is the panel that exists today) | PR #116, branch `feat/panel-column`, merged `e296679` (2026-10-04) | merged |
| Repaint on data arrival (staged step 1) | PR #112, branch `feat/repaint-on-data`, merged `f0dac95` (2026-10-03) | merged |
| Measurement page reconciled with its evidence | PR #121, branch `docs/panel-docs-reconcile`, merged `3b7bd8f` | merged |
| Panel draw / click-target contract corrected | PR #122, branch `fix/panel-surface-contract`, merged `17ece90` | merged |
| Panel, **whole** feature (elements, hotkey, D-Bus toggle, X11 atoms) | PR #115, branch `feat/panel-ui`, base `be77233`, 18 own commits | **OPEN, unmerged**, 62 commits behind `lapsphere` |
| Frame-measurement design + Vulkan layer spike | PR #111, branch `feat/mini-panel`, base `5c57bc7` | **OPEN, unmerged** — docs, `gui/examples/` and `spike/` only: `git diff 17ece90...feat/mini-panel -- gui/src daemon/src common/src` is empty |

PR #116's body states the relation between the two panel branches explicitly:
*"Supersedes #115 (kept on feat/panel-ui as reference), which tried to land the whole
feature at once. This is the smallest part that is useful on its own; the rest
follows as separate PRs."* #115 was therefore not merged and not ported — it is a
reference, and its commit list is a design input, not a queue of cherry-picks.

## 2. What of #115 is in the merged branch: nothing, commit-wise

```
$ git cherry -v origin/lapsphere origin/feat/panel-ui | head -3
+ 445dd9c… gui: panel config in its own panel.json, separate from settings.json
+ bd943b5… gui: panel item registry and one unit module, absent data renders —
+ 4644fae… gui: panel render — fixed layout from the element set, 60 s ring buffer
```

All 18 commits are marked `+` — **not in the upstream branch, and not
patch-equivalent to anything there either**. Nothing from `feat/panel-ui` has
landed in any form. What overlaps is *capability*, implemented independently by
#116:

| Capability | `feat/panel-ui` (#115) | `lapsphere` today (#116) | Verdict |
|---|---|---|---|
| Panel config in its own `panel.json` | `gui/src/panel/config.rs` (710 lines), own version/normalize/atomic write | `gui/src/panel/config.rs` (493) + `panel/mod.rs` (331), own `panel.json` | both, independent — not the same code |
| Element registry | `items.rs` (462), 15 ids — including `fps`, `fps_graph`, `frametime_graph` as registry entries with no producer — plus `units.rs` (234) | `rows.rs` (618), 23 ids (cpu 6, gpu 8, ram 6, gamepad 3); no separate units module | both, independent; different registries |
| Fixed layout from the element set | `render.rs` (519) + `window.rs` (664) + a 60 s frametime ring (`4644fae`) | `layout.rs` (124) + `render.rs` (333) + `window.rs` (328); **no ring buffer** | both, independent; ring is PR E |
| Poll narrowing in panel mode | `7e08300` — pause unselected coordinator components | `window.rs::required_components` + `app.rs::sync_poll_set` | both, independent |
| Mode switching on one window | `3aee0a9` — reshape **plus** X11 atoms confirmed by read-back and retry | `window.rs` — size, decorations, resizable only | **main is missing the X11 half** |
| Panel menu / settings window | `b5183e0`, `72bceef`, `settings_window.rs` (503) | `menu.rs` (378) + `app.rs::draw_settings_window` (own viewport) | both, independent |
| Hotkey, D-Bus toggle, `--toggle-panel`, hide-safety | `d88c1a3`, `8c77593`, `bf7945d`, `fead23e`, `0bec93d`, `visibility.rs` (1209), `bus_connection.rs` | absent (`--panel` exists; no hotkey, no panel D-Bus method, no F8) | **branch-only — PR 3** |
| Stacking / taskbar atoms / transparency / position / drag | `b018d25`, `36267b9`, `x11.rs` (425) | config fields `stacking`, `hide_from_taskbar`, `background_opacity` **stored and normalized only**; nothing applies them | **config surface only — PR 2** |
| Design, overlay tests, tools | `panel-design.md` (1335), `panel-overlay-tests.md` (808), `panel.md` (246), `tools/frametime_stats.py` | only `docs/development/panel-measurements.md`; no `tools/` | **branch-only** |
| Overlay evidence media, owner check script | PR #111: `docs/development/panel-overlay-tests/` (15 PNG + 2 MP4 at the branch head; the PR body says 14 PNG, the tree has 15), `panel-owner-check.sh` | none | **branch-only** |

The two things a reader is most likely to assume wrong:

* **The X11 stacking work is not "half merged".** The commands the merged panel
  sends are exactly `MinInnerSize`, `Resizable`, `Decorations`, `InnerSize`
  (`gui/src/panel/window.rs:124-131`) — there is no `WindowLevel`, no
  `_NET_WM_STATE` `ClientMessage`, and no position code anywhere in `gui/src`.
  `gui/src/panel/mod.rs:30-33` says so in prose as well.
* **`--toggle-panel`, the F8 key and the panel D-Bus object do not exist in the
  merged tree.** `--panel` and `--tray` do.

## 3. What has evidence, and what does not

### 3.1 In the merged branch

Live, on **X11 / Xvfb / xfwm4 4.20, llvmpipe software rendering, ONE session**,
recorded in [`panel-measurements.md`](./panel-measurements.md) and PR #116's body:

* geometry 300x198 at the 10 default rows; width slider live 300 → 640 → 410 → 200
  with clamping at both bounds, and still working after closing/reopening the
  settings window;
* settings-window close paths: `WM_DELETE`, `Alt+F4` on the settings window only,
  `Alt+F4` on the panel closes the app;
* idle CPU, 3 x 60 s per mode, same daemon: panel 120/111/106 jiffies (1.87/s) vs
  normal window 523/558/537 (8.99/s) — **the panel is ~4.8x cheaper**, attributed
  to the poll set rather than to drawing.

Its limits, stated by that page itself: a branch rather than a commit SHA, one
machine, debug build, software rendering, **no known compositing state**, nothing
re-measured since — *"nothing on this page is a release gate"*.

Unit-tested only, with **no live record**: `panel.json` load / normalize / atomic
write and the "loading must not rewrite the file" property; the poll set's
contents; height continuity when a row is toggled; the draw-vs-interaction rect
equality (PR #122).

### 3.2 On `feat/panel-ui` (#115) — historical, not re-confirmed

Claimed measured: 300 mode switches with RSS +4 kB; 60 min hidden panel; 20/20
`--toggle-panel` runs; the hotkey firing under XTEST with NumLock/CapsLock/
ScrollLock; atoms confirmed by read-back; panel above a normal window, click
focuses it.

**These are not a gate and are not re-confirmed here**, for a reason #115's own
body gives: the numbers *"were taken on a binary that does not correspond to a
single named commit"* — a ~1780-line change appeared mid-run — so they *"must be
repeated on a binary built from a named commit before they gate anything"*.

Failed or inconclusive there, and **not fixed anywhere**:

| Item | Recorded result |
|---|---|
| Position persistence across restart | **FAILED** — stored `top_left, offset 24,24`; came up at `0,43` |
| Panel-mode idle CPU | ~9x the normal window (7.933 vs 0.867 jiffies/s) — "a finding to address, not a pass". Later inverted to ~4.8x *cheaper* by the #116 measurement above; the two disagree and neither is a gate |
| Alt+Tab focus | inconclusive — panel survived, xfwm4 moved focus to `0x0` |
| Hotkey conflict (`BadAccess`) path | never exercised — a competing grab could not be made to win |

Two defects found and fixed **only on that branch**. They are the expected first
bugs of PR 3 and must not be re-introduced: the panel D-Bus object was exported on
a second zbus connection (so `--toggle-panel` hung forever), and the hotkey was
grabbed on one X connection while every press was polled on another (the log said
"captured" and the key did nothing). A third, `bf7945d`, corrected function-key
keysym/keycode lookup.

### 3.3 The Vulkan layer (#111) — not working

`spike/vk-layer/STATUS.md` (branch `feat/mini-panel`): *"**NOT WORKING** … nothing
here has produced a measurement yet."* Concretely: the layer is enumerated and
loaded, `vkCreateInstance` completes, `frames-<pid>` is created in
`$XDG_RUNTIME_DIR/lapsphere/` with mode 0600 and the protocol-v1 size — but

```
enabled; segment created gipa=0 create_instance=1 create_device=0 gdpa=0
  destroy_device=0 create_swapchain=0 destroy_swapchain=0 queue_present=0
```

`frame_count` stays **0**. Our `vkGetInstanceProcAddr` is never called, so no
device is registered and `vkQueuePresentKHR` is never intercepted. The remaining
blocker is the **loader interface**, not frame timing. Five bugs are already
documented there with the value that proved each one (including a gipa from
`libVkLayer_MESA_device_select.so` that never returns on this host, and the
working route via the loader terminator in `libvulkan.so.1.4.341`).

The design's **GO gate** for the layer as a whole — loads and stays inert without
its variable, agreement with MangoHud, overhead below 10 µs median, survives exit /
`kill -9` / resize / two instances / no reader, with no influence on the game on any
failure — therefore has **no measured result at all**: #111 states that tests 1–6
(MangoHud agreement, per-hook overhead, robustness, reader latency and CPU at 60 and
240 Hz, the `frametime_stats.py` A/B) are all outstanding.

**`docs/development/frametime-layer-spike.md` does not exist on any branch**
(checked with `git ls-tree` on both `feat/mini-panel` and `feat/panel-ui`). That
matters: `panel-design.md`'s criteria 9–12 point their status at that file
("Measured in the spike — see `frametime-layer-spike.md` §3" and so on). Those
pointers resolve to nothing, so criteria 9–12 are recorded here as
**UNRESOLVED**, and no spike number may be quoted as a measurement.

### 3.4 What #121 settled about the credibility of the history — kept as history

Reused from the reconciling PR (#121) and its page, not restated as findings:

* the numbers are tied to a **branch, not a commit SHA**;
* the environment's **compositing state is UNRESOLVED** — a fresh check on the same
  host reproduced `Unsupported GL renderer (llvmpipe …)` and `_NET_WM_COMPOSITING`
  absent, so "compositor on" was removed rather than flipped;
* the earlier panel-mode CPU figure came from a binary matching no single commit;
* the **right-click diagnosis was retracted**: the conclusion that a `Response`
  never receives egui's click flag came from reading two probe values from
  *different frames*. One line of one frame showed `ctx_sclick=true
  resp_sclick=true`. The menu is driven the ordinary way.

Consequences for this page: these are the reasons no row in §5 may be marked
`PASS` on a historical number, and the reason the measurement page stays a
measurement page.

## 4. Owner decision on the first version (from #111, 2026-10-03)

* `fps`, `frametime_ms`, `frametime_graph`, `low_1pct`, `low_01pct` and
  `game_name` are **first-release panel elements**, not a later milestone.
* Consequence: an external process cannot see the game's present, so the data must
  come from **inside the game process** — a Vulkan implicit layer that timestamps
  `vkQueuePresentKHR` and publishes intervals through shared memory. That layer
  **does not work yet** (§3.3, `frame_count = 0`).
* First-release non-goals, stated so the scope cannot widen silently: no OpenGL
  hook, no 32-bit games, no Wayland-specific layer work (every Wayland *claim* is
  NOT TESTED), no anti-cheat and no DXVK/Proton coverage (the spike ran on
  `vkcube`).
* **Do not substitute demonstration metrics.** The `gui/examples/*` demo and probe
  binaries on #111/#115 are throwaway instruments; their numbers are not product
  measurements and must not stand in for the layer's.
* **This is unsettled in the merged tree.** `gui/src/panel/mod.rs:39-49` and
  `docs/development/panel-measurements.md` ("Not verified") record the opposite reading
  and explicitly decline to decide it. The merged code must not be read as the
  decision, and this page does not make it either.

## 5. Acceptance criteria for the original stages 1–7

The seven steps and their PR letters are as written in
`docs/development/panel-design.md` on `feat/panel-ui` (#115; the same list, older,
on `feat/mini-panel`): steps 1–2 = **PR A**, steps 3–7 = **PR D**, the layer =
**PR C**, fps elements = **PR E**.

**How a status is assigned here.** `PASS` requires an observation of the *behaviour*
on a named commit (or a build reproduced from the committed tree), with the evidence
named below; code existing, and unit tests existing, are never sufficient.
`FAIL` = attempted and did not work, or provably absent where the criterion demands
it. `NOT TESTED` = no observation of any kind. `UNRESOLVED` = the requirement itself
is undecided, or the merged scope contradicts it.

| # | Criterion (staged step) | State | Evidence | What remains |
|---|---|---|---|---|
| 1 | Data arrival requests an immediate repaint instead of waiting for the 500 ms tick | **PASS** | PR #112, merged `f0dac95`: update→draw latency 0.1 / **225.6** / 483.7 ms → 0.0 / **1.0** / 14.1 ms, 168 s per case, release build, live daemon, X11/xfwm4; PR states the shipped binary was rebuilt from the committed tree and re-run. Code in the merged tree: `app.rs::request_repaint_now` — the `REPAINT_PENDING` one-shot latch, requested on every send into the update channel and cleared at the start of `handle_hardware_updates`'s drain, so a burst costs one request per frame (latch covered by unit tests as well) | Nothing for this criterion. Not tested under a compositor or Wayland (as #112 states) |
| 2 | Move the channel drain into `logic()` — *only if Q3 puts the iconify case in scope* | **UNRESOLVED** | Design doc Q3: the spike **refuted** the motivation for `Visible(false)` (drain keeps up), leaving WM iconify as the only case, which is the owner's scope call. Merged tree has no `logic()` hook at all (`gui/src/app.rs` defines only `fn ui`), and handles the adjacent problem differently (`note_ui_frame`, the logs gate) | The owner answers Q3. Default if unanswered: skip the step (it is conditional in the plan) |
| 3 | Panel mode in `AppState` + settings, persisted and reloaded | **PASS** for mode and config; **NOT TESTED** for persistence across a real restart | Live: `--panel` start, 300x198 geometry, the live width slider, and each mode's idle CPU (#116 body, measurement page). The mode is switched by restarting rather than by a toggle, because no D-Bus toggle exists yet (that is PR 3). Code: `panel/config.rs`, `panel/mod.rs` (`panel.json`, version, normalize, atomic write). Persistence evidence is unit tests only | One observation of a full restart reading back a changed `panel.json`. The `stacking` / `hide_from_taskbar` / `background_opacity` settings UI is deliberately absent (PR 2) |
| 4 | Fixed layout with reserved slots, `—` for absent data, **including the six frame rows' reserved widths** | **PASS** for the fixed layout and `—`; **FAIL** for the frame-row reservation, and the requirement is **UNRESOLVED** | Layout: the size is computed, never measured from content — `height = visible_rows * row_height + 2 * padding` with `row_height` from egui's own font metric (`panel-measurements.md`, "Window size"). Live observation: 300x198 at the 10 default rows, and the width slider applying at once (same page, "The width slider, live"). `gui/src/panel/mod.rs:27-29`: *"No reserved slots for them either: a slot with no producer is a permanent row of dashes"* — the merged decision is the opposite of this criterion | The owner settles §4. Until then no frame row may be added, and no element may be designed against it |
| 5 | In panel mode, poll only the components the visible rows need; never `logs` | **PASS** | Code + named unit tests: `window.rs::required_components`, `app.rs::sync_poll_set`; `the_poll_set_covers_every_visible_row_and_nothing_else`, `no_visible_rows_means_nothing_is_polled`, `the_expensive_log_ring_is_never_in_a_panel_poll_set`. Supporting live signal: the 4.8x idle-CPU ratio, which both #116 and the reconciled page attribute to the poll set | The actual poll set on the wire was never captured live, and the CPU ratio is historical and not a gate. No live check that returning to the normal window resumes every component |
| 6 | Window switching, single-window shape: in-place reshape **+ runtime `WindowLevel(AlwaysOnTop)` + EWMH `SKIP_TASKBAR`/`SKIP_PAGER`, position persistence, Wayland no-op path** | **FAIL** as specified | In-place reshape: PASS (live geometry, `panel-measurements.md`, "Window size"). The X11 half is provably absent: the merged `apply_spec` sends only `MinInnerSize`, `Resizable`, `Decorations`, `InnerSize` (`gui/src/panel/window.rs:124-131`); no `WindowLevel`, no `ClientMessage`, no position code in `gui/src`; `gui/src/panel/mod.rs:30-33` assigns atoms/transparency to PR 2 and the hotkey/toggle to PR 3; `stacking`/`hide_from_taskbar` are stored and normalized only | PR 2 in full (atoms, transparency, position, drag). The **combination** "always-on-top *and* skip-taskbar over a fullscreen game" is NOT TESTED anywhere — each half was proven separately, on xfwm4 only |
| 7 | README section describing panel mode, the element list and the Wayland caveats | **FAIL** | `grep -i panel README.md` returns nothing on the base. The panel is described only in `gui/src/panel/mod.rs` and `docs/development/panel-measurements.md` | Write the section, with the Wayland caveats from §6 and the element list from `panel/rows.rs` |

Frame measurement (PR C / PR E, criteria 9–13 of `panel-design.md`) is **not** in
this table because it is not reachable: the layer records no frames (§3.3) and
criteria 9–12 cite a document that does not exist. Its status is `UNRESOLVED`
across the board, and the one item that can be started without the loader — the
Rust statistics port matching `frametime_stats.py` (criterion 13) — has no test in
the merged tree, since `docs/development/tools/` is itself branch-only.

## 6. Confirmed scope boundaries

* **Exclusive fullscreen is out of scope** — an owner product decision (#111):
  poorly behaved with modern desktop environments, no measured fps benefit, and
  most games do not use it by default. The target is windowed, borderless
  fullscreen, or a second monitor.
* **Wayland is not supported and is not to be declared supported.** No Wayland
  session exists on this host and the owner's does not start. The merged panel uses
  only `ViewportCommand`s; the X11 work is PR 2's and untested on Wayland. On the
  available evidence, switcher exclusion by `_NET_WM_STATE` is X11-only and not
  achievable the same way there.
* **Unverified environments stay unclaimed:** GNOME/mutter, KDE/kwin, XWayland,
  and any session with a compositor running. xfwm4 4.20.0 on X11 **without** a
  compositor is the only environment with any evidence, and the compositing state
  of the historical sessions is UNRESOLVED.
* **Three areas need three separate verdicts, on separate evidence:**
  *transparency* (real ARGB, not a drawn rectangle), *window behaviour* (stacking,
  taskbar/switcher, position, drag, multi-monitor placement), and *fps/frametime*
  (layer, reader, elements, statistics). None of them can be carried by the
  panel's geometry or idle-CPU measurements, and a pass in one says nothing about
  the others.
* **Multi-monitor placement** is not implemented and not claimed.

## 7. Sources

| Claim | Source |
|---|---|
| Base commit, what it is | `git rev-parse origin/lapsphere`, `git log -1 17ece90` |
| #115 not merged, not patch-equivalent | `git cherry -v origin/lapsphere origin/feat/panel-ui` (18 × `+`), `git merge-base` = `be77233` |
| PR #116 supersedes #115 | PR #116 body, first paragraph |
| Merged X11 half absent | `gui/src/panel/window.rs:124-131`, `gui/src/panel/mod.rs:30-33` |
| Merged panel's live numbers and their limits | `docs/development/panel-measurements.md`, PR #116 body |
| #115's numbers tied to no commit; failed items | PR #115 body ("Confirmed by measurement", "Failed and inconclusive") |
| Layer not working, `frame_count = 0` | `spike/vk-layer/STATUS.md` on `feat/mini-panel` |
| `frametime-layer-spike.md` absent | `git ls-tree -r --name-only <branch> -- docs/development/` on both branches |
| Owner decision, non-goals, "no demo metrics" | PR #111 body |
| Stages 1–7, PR letters, criteria 1–13 | `docs/development/panel-design.md` on `feat/panel-ui` |
| Retraction, gate-free status, UNRESOLVED compositing | `docs/development/panel-measurements.md` §"What these numbers are tied to", §"Compositing: unresolved", §"The right-click menu — a wrong diagnosis" |
