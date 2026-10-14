# Panel, first version — baseline and acceptance criteria

What is merged, what exists only on an unmerged branch, what has evidence, and what
still has to be done before the panel's first version can be called accepted.

**Base:** `17ece901754fb2cfcaf0b509dcb8d56e3639eea6` (= merge of PR #122,
`origin/lapsphere`, 2026-10-14 10:11 +0200). Re-check it before quoting anything
here; every path and line number below is as of that commit.

This page **records status; it does not settle design**, with one exception stated
in §4: the owner's decision on the composition of the first version is **taken as
settled** and is used below as a requirement, not as an open question. Nothing on
this page was re-measured to write it, and no number below is re-confirmed by it.

*Revision 2 (second commit on PR #123): the acceptance table in §5 is re-aligned to
the agreed first-version stages 1–7. Revision 1 used `panel-design.md`'s staged-plan
steps (repaint, drain, panel mode, layout, polling, window switching, README), which
are **implementation steps inside that plan's PR A/D, not the agreed stages**;
mixing the two enumerations made the table read as if window switching and a README
section were stages of the first version. Everything else from revision 1 — the
baseline, the credibility limits, the scope boundaries and the documentation-only
character of this work — is unchanged.*

*Revision 3 (third commit on PR #123): stage 6 is stated as `FAIL` for the current
implementation, with the reservation conflict called out separately as a **blocking
non-conformance**, and the dependency note no longer reads work on stages 2–5 as a
plan. No technical conclusion, criterion or piece of evidence changed.*

*Revision 4 (a commit on PR #128, the transparency implementation): the
**environment** statements are corrected. This host now runs a **Wayland** session
(xfwl4) with **Xwayland** on `:0`, and that session **composites** — so "no session
on this host could carry a transparency verdict" is withdrawn, and "no Wayland
session exists on this host" is withdrawn with it (§3.4, §8). The stage-4 row keeps
`FAIL` and gains the implementation's own measurement as evidence, which is **not**
`PASS`: it is on an unmerged branch, and the acceptance run on the merge commit is
still outstanding. No other row, criterion or conclusion changed, and the page's own
base and every line number below are unchanged — still as of `17ece90`.*

Companion pages: [`panel-measurements.md`](./panel-measurements.md) holds the
measurements taken for the merged panel (PR 1) and their own limits;
[`panel-transparency.md`](./panel-transparency.md) holds the stage-4 transparency
measurement, its session and its limits.

**This work is documentation only.** It changes no runtime code, no Vulkan layer,
no UI, no settings and no panel behaviour; the problems it records are recorded as
work to do, not done. PR #111 and PR #115 are neither modified nor merged by it.

## 1. Baseline — what is merged

| Item | Where | State |
|---|---|---|
| Panel, part 1 (this is the panel that exists today) | PR #116, branch `feat/panel-column`, merged `e296679` (2026-10-04) | merged |
| Repaint on data arrival (panel usability; **not** frame capture — see stage 1) | PR #112, branch `feat/repaint-on-data`, merged `f0dac95` (2026-10-03) | merged |
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
landed in any form. What overlaps is *capability*, implemented independently by the
merged PR #116:

| Capability | `feat/panel-ui` (#115) | `lapsphere` today (#116) | Verdict |
|---|---|---|---|
| Panel config in its own `panel.json` | `gui/src/panel/config.rs` (710 lines), own version/normalize/atomic write | `gui/src/panel/config.rs` (493) + `panel/mod.rs` (331), own `panel.json` | both, independent — not the same code |
| Element registry | `items.rs` (462), 15 ids — including `fps`, `fps_graph`, `frametime_graph` as registry entries with no producer — plus `units.rs` (234) | `rows.rs` (618), 23 ids (cpu 6, gpu 8, ram 6, gamepad 3); no separate units module, **no frame row** | both, independent; different registries |
| Fixed layout from the element set | `render.rs` (519) + `window.rs` (664) + a 60 s frametime ring (`4644fae`) | `layout.rs` (124) + `render.rs` (333) + `window.rs` (328); **no ring buffer** | both, independent; ring belongs to stage 6 |
| Poll narrowing in panel mode | `7e08300` — pause unselected coordinator components | `window.rs::required_components` + `app.rs::sync_poll_set` | both, independent |
| Mode switching on one window | `3aee0a9` — reshape **plus** X11 atoms confirmed by read-back and retry, position, hide-safety | `window.rs` — size, decorations, resizable only | **main is missing the X11 half** (stages 2–3) |
| Panel menu / settings window | `b5183e0`, `72bceef`, `settings_window.rs` (503) | `menu.rs` (378) + `app.rs::draw_settings_window` (own viewport) | both, independent |
| Hotkey, D-Bus toggle, `--toggle-panel`, hide-safety | `d88c1a3`, `8c77593`, `bf7945d`, `fead23e`, `0bec93d`, `visibility.rs` (1209), `bus_connection.rs` | absent (`--panel` exists; no hotkey, no panel D-Bus method, no F8) | **branch-only — stage 5** |
| Stacking / taskbar atoms / transparency / position / drag | `b018d25`, `36267b9`, `x11.rs` (425) | config fields `stacking`, `hide_from_taskbar`, `background_opacity` **stored and normalized only**; nothing applies them | **config surface only — stages 2–4** (transparency is implemented and measured on PR #128, unmerged — §5 row 4) |
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

Everything in this table describes the *unmerged* branch. None of it is evidence
about the current implementation; where the branch's own numbers are used below,
they are labelled historical.

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
  to the poll set rather than to drawing;
* repaint latency (PR #112, its own measurement): update→draw 0.1 / **225.6** /
  483.7 ms → 0.0 / **1.0** / 14.1 ms. That is the latency of **daemon data reaching
  a frame**. It observes no presents and says nothing about fps — see stage 1.

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

**These are not a gate, are not re-confirmed here, and are not evidence about the
merged implementation** — nothing from that branch is in `lapsphere` (§2), and for a
reason #115's own body gives: the numbers *"were taken on a binary that does not
correspond to a single named commit"* — a ~1780-line change appeared mid-run — so
they *"must be repeated on a binary built from a named commit before they gate
anything"*.

Failed or inconclusive there, and **not fixed anywhere**:

| Item | Recorded result |
|---|---|
| Position persistence across restart | **FAILED** — stored `top_left, offset 24,24`; came up at `0,43` |
| Panel-mode idle CPU | ~9x the normal window (7.933 vs 0.867 jiffies/s) — "a finding to address, not a pass". Later inverted to ~4.8x *cheaper* by the #116 measurement above; the two disagree and neither is a gate |
| Alt+Tab focus | inconclusive — panel survived, xfwm4 moved focus to `0x0` |
| Hotkey conflict (`BadAccess`) path | never exercised — a competing grab could not be made to win |

Two defects found and fixed **only on that branch**. They are the expected first
bugs of stage 5 and must not be re-introduced: the panel D-Bus object was exported
on a second zbus connection (so `--toggle-panel` hung forever), and the hotkey was
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
  absent, so "compositor on" was removed rather than flipped. **That host state has
  since changed** (revision 4): the desktop now runs xfwl4 (Wayland) with Xwayland on
  `:0`, and that session does composite. This does not re-confirm the historical
  Xvfb/xfwm4 sessions — their compositing state stays UNRESOLVED — and the new
  session's evidence belongs to the transparency measurement on its own page (§8);
* the earlier panel-mode CPU figure came from a binary matching no single commit;
* the **right-click diagnosis was retracted**: the conclusion that a `Response`
  never receives egui's click flag came from reading two probe values from
  *different frames*. One line of one frame showed `ctx_sclick=true
  resp_sclick=true`. The menu is driven the ordinary way.

Consequences for this page: these are the reasons no row in §5 may be marked
`PASS` on a historical number, on a demo binary, or on an unmerged branch, and the
reason the measurement page stays a measurement page.

## 4. Owner decision on the first version — settled

The owner's decision (#111, 2026-10-03) is **fixed** and is used below as a
requirement:

* `fps`, `frametime_ms`, `frametime_graph`, `low_1pct`, `low_01pct` and
  `game_name` are **first-release panel elements**, not a later milestone.
* Consequence: an external process cannot see the game's present, so the data must
  come from **inside the game process** — a Vulkan implicit layer that timestamps
  `vkQueuePresentKHR` and publishes intervals through shared memory. That layer
  **does not work yet** (§3.3, `frame_count = 0`), and stage 1 is the work that gets
  it to produce frames.
* First-release non-goals, kept so the scope cannot widen silently: no OpenGL hook,
  no 32-bit games, no Wayland-specific layer work (every Wayland *claim* is NOT
  TESTED), no anti-cheat and no DXVK/Proton coverage (the spike ran on `vkcube`).
* **Do not substitute demonstration metrics.** The `gui/examples/*` demo and probe
  binaries on #111/#115 are throwaway instruments; their numbers are not product
  measurements and must not stand in for the layer's.

**Inconsistency to remove during implementation.** The merged tree states the
opposite of this decision in prose. Those statements are no longer a live
disagreement — the decision is the owner's and it is fixed — but they contradict it
and they will mislead whoever implements stages 1 and 6:

| Where | What it says |
|---|---|
| `gui/src/panel/mod.rs:27-29` | "**No reserved slots** for them either: a slot with no producer is a permanent row of dashes" |
| `gui/src/panel/mod.rs:39-49` | records the scope question as undecided ("Whether the FIRST release carries fps … is NOT decided by the merged code… the decision is the owner's and is not made here") |
| `gui/src/panel/config.rs:23` | "**No frame-time / fps rows and no ring buffer.** Out of scope by decision; the reserved-slot idea that would have needed them is not carried over" |
| `docs/development/panel-measurements.md`, "Not verified" | lists the frame elements as "not implemented anywhere" and calls their inclusion an unresolved owner decision |

Note the first row especially: it is not wording, it is the **layout decision**
stage 6 depends on. Removing the four statements belongs to the implementation of
stages 1 and 6; this page only records them as an inconsistency to settle, and
makes no code change.

## 5. Acceptance criteria for the first version — the agreed stages 1–7

| Stage | What it covers |
|---|---|
| 1 | Research: getting **real fps / frametime** out of a running game, through the Vulkan layer |
| 2 | Position: **save and restore** of the panel's place on screen |
| 3 | Window flags and behaviour **over other windows** (always-on-top, taskbar/switcher) |
| 4 | **Real transparency** |
| 5 | **Hotkey and D-Bus** control |
| 6 | **Integration of the metrics into the panel** |
| 7 | **Final acceptance of the first version** |

This is the agreed staging, and the table below is written against it. It is **not**
the enumeration in `panel-design.md` on the unmerged branches: that document's
"staged implementation plan" has its own steps 1–7 (repaint, drain into `logic()`,
panel mode, fixed layout, poll narrowing, window switching, README) spread over
PR A / PR D, plus C1–C5 and PR E. Both enumerations exist; they are different lists,
and a cross-read that mixes them produces a wrong status.

**How a status is assigned here.** `PASS` requires an observation of the *behaviour*
on a named commit (or a build reproduced from the committed tree), with the evidence
named; code existing, unit tests existing, an unmerged branch, or a demo binary are
never sufficient. `FAIL` = attempted and did not work, or provably absent where the
criterion demands it. `NOT TESTED` = no observation of any kind. `UNRESOLVED` = the
requirement itself is undecided, or the merged scope contradicts it.

| # | Stage | State | Evidence | What remains |
|---|---|---|---|---|
| 1 | **Research: real fps / frametime from inside the game process** (Vulkan implicit layer + shared-memory protocol) | **FAIL** — in progress, no frames recorded | Branch `feat/mini-panel` (PR #111), `spike/vk-layer/STATUS.md`: the layer enumerates and loads, `vkCreateInstance` completes, `frames-<pid>` is created with mode 0600 and the protocol-v1 size — but the counters read `gipa=0 create_instance=1 create_device=0 gdpa=0 destroy_device=0 create_swapchain=0 destroy_swapchain=0 queue_present=0`, so **`frame_count = 0`** and **`gipa = 0`**: our `vkGetInstanceProcAddr` is never called, no device is registered, and `vkQueuePresentKHR` is never intercepted. STATUS.md: "NOT WORKING … nothing here has produced a measurement yet". The blocker is an **interface question with the Vulkan loader**, not frame timing. The design's GO gate has no result (tests 1–6 outstanding), and `frametime-layer-spike.md` does not exist, so criteria 9–12 are UNRESOLVED. **PR #112's repaint work is not frame capture**: it requests a frame when *polled daemon data* arrives, observes no presents, and the merged tree contains no fps/frametime code at all (`gui/src/panel/rows.rs` has no frame row) | Answer the loader-interface question (the manifest's `functions`/negotiation route, or a working reference layer for this loader), then run the four spike tests (agreement with MangoHud, per-hook overhead, robustness, reader latency and CPU at 60 and 240 Hz), then criterion 13's Rust-vs-`frametime_stats.py` parity test. Until `frame_count > 0` no fps element may be claimed, and stage 6 cannot start |
| 2 | **Position: save and restore**, clamped against the monitor size and confirmed by read-back | **FAIL** | The merged tree has **no position code at all**: no `OuterPosition` / `ViewportCommand::OuterPosition`, no monitor clamp anywhere in `gui/src`, and no position field in `PanelConfig` (`gui/src/panel/config.rs:185-196`). `gui/src/panel/config.rs:26-28`: "**No position.** The panel is placed by the window manager / desktop for now; a stored corner is the second-monitor question, which is explicitly NOT claimed"; `gui/src/panel/mod.rs:36-37` repeats it for multi-monitor. The only attempt is on the unmerged branch: #115's historical result — stored `top_left, offset 24,24`, the panel came up at `0,43` after a restart — recorded there as a real defect, tied to no single commit, and therefore **not** a verification of anything in the merged tree | Implement (with the stacking work), apply, and confirm the applied position by read-back (`xwininfo`/`xprop`) rather than trusting the request — a request that is sent is not a request that landed (§3.4). Multi-monitor placement stays out of the first version unless the owner decides otherwise |
| 3 | **Window flags and behaviour over other windows**: runtime always-on-top, no taskbar/switcher entry, no-op-tolerant where the platform cannot do it | **FAIL** | Provably absent in the merged tree: `apply_spec` sends only `MinInnerSize`, `Resizable`, `Decorations`, `InnerSize` (`gui/src/panel/window.rs:124-131`); no `WindowLevel` anywhere in `gui/src`; no `ClientMessage`, no `_NET_WM_STATE`, no X11 code at all; `stacking` and `hide_from_taskbar` are parsed, clamped and normalized only. `gui/src/panel/mod.rs:30-33` assigns the atoms to a later PR and states that nothing claims they are applied. Historical (not evidence about the merged tree), from #111's overlay tests on xfwm4 4.20 with **no compositor**: the **runtime** `WindowLevel(AlwaysOnTop)` is honoured where the startup builder hint is ignored (B9/B14), and ADD of `SKIP_TASKBAR`+`SKIP_PAGER` by EWMH `ClientMessage` is honoured on a `Normal` window once the message arrives after the window settles (0 ms and 50 ms fail, 100 ms+ works — B18/B18d) | Implement, then measure the **combination** — one window that is both always-on-top *and* skip-taskbar, over a borderless-fullscreen game — which is **NOT TESTED anywhere**; each half was proven separately. No claim about another WM, about a compositor-on session, or about Wayland (`_NET_WM_STATE` is X11-only, and a Wayland client may not set it) |
| 4 | **Real transparency** (an actual translucent surface, not a drawn rectangle) | **FAIL** for the merged tree | In the merged tree `background_opacity` is a config field only: defined with `MIN_BACKGROUND_OPACITY = 0.25`, clamped and normalized (`gui/src/panel/config.rs`), and **nothing applies it** — no `with_transparent`, no ARGB or alpha path anywhere in `gui/src`, and the panel's frame is left at egui's default opaque `CentralPanel` because "the opaque background and its transparency are PR 2's work" (`gui/src/app.rs:1252`). Historical: #111's B8 measured that a transparent window shows the root window's black rather than the desktop **when no compositor is present**. Implemented and measured on **PR #128 (unmerged, head `098c740`)**, in a session that composites — X11 on Xwayland under xfwl4: the window holds an ARGB visual (`Depth 32`), its own surface carries the configured alpha (one value `255` at 1.0; `128` at 0.5 with the glyph pixels still `255`), the screen at 50 % blends the fill with the measured backdrop (which is not visible through it at 100 %) while the text does not fade, and the ordinary window's surface is alpha `255` in every sampled pixel. Numbers, session and limits: [`panel-transparency.md`](./panel-transparency.md). **Not `PASS` here**: the evidence is on an unmerged branch | Merge, then run the acceptance on the **merge commit**, following the procedure on the transparency page. The earlier note that the criterion "cannot currently be passed on this host at all" is **withdrawn** — a compositing session now exists on it (§3.4, §8). Still NOT TESTED anywhere: a compositor other than xfwl4, a session without a compositor, and native Wayland |
| 5 | **Hotkey and D-Bus** control (mode toggle by key and over the bus, with the hide-safety rule) | **FAIL** | Absent from the merged tree: no hotkey handling (no `F8`/`Shift_R` code; `gui/src/panel/mod.rs:30-31` assigns it to a later PR), no panel D-Bus method in `daemon/src` or the GUI, no `--toggle-panel`; mode is switched by restarting with `--panel` or from the in-window button and menu. `gui/src/panel/config.rs:29-30`: "**No hotkey or D-Bus fields.** Those arrive with the visibility work (PR 3)". Historical, branch-only: #115 recorded 20/20 `--toggle-panel` runs and the hotkey firing under XTEST with NumLock/CapsLock/ScrollLock, and found three defects that exist **only** as its fixes — the D-Bus object exported on a second connection (the toggle hung), the key grabbed on one X connection and polled on another, and a function-key keysym/keycode error | Implement; carry those three fixes instead of rediscovering them. NOT TESTED anywhere: the "key already taken" (`BadAccess`) path, keyboard pass-through, and every Wayland claim |
| 6 | **Integration of the metrics into the panel** (the six frame elements, their config block, the shm reader) | **FAIL** for the current implementation — and the integration is blocked by a non-conformance in the merged layout (see **Blocking non-conformance** below the table) | Not implemented: the merged registry has 23 ids and **no frame row** (`gui/src/panel/rows.rs`), and the merged layout decision explicitly refuses what this stage needs — `gui/src/panel/mod.rs:27-29`: "No reserved slots for them either: a slot with no producer is a permanent row of dashes" — while the design required the six frame rows' **reserved widths** from the start so that landing the data cannot reflow the panel (its criterion 4). The owner's decision (§4) therefore has nowhere to land in the merged layout, and `gui/src/panel/mod.rs:39-49` still describes the question as open. On the unmerged branch the registry did carry `fps`, `fps_graph` and `frametime_graph` ids — with no producer, which is exactly what the merged decision rejects | Settle the reservation question for the integration: the layout decision this stage needs is required **after** frame capture is confirmed, not before it, and stage 1 does not depend on it. Then: reader selection by `STALE_AFTER = 2 s`, the statistics window for 1 %/0.1 % low, the fixed-height graph strip, `—` when there is no source. Blocked on stage 1: with `frame_count = 0` there is no value to integrate |
| 7 | **Final acceptance of the first version** | **NOT TESTED** — cannot start | No end-to-end acceptance run exists on any commit. The evidence behind stages 1–6 is either absent from the merged tree (2–5), contradicted by it (6), or a failure on an unmerged branch with no commit identity (1). No acceptance criteria for the agreed stages existed in the merged tree before this page, and no environment matrix has been recorded for an acceptance run | Close stages 1–6 first. The run must name the commit and the build, state per-criterion evidence, name the environment it ran in (WM, compositing state, GPU, X11 or Wayland), and keep its `NOT TESTED` items visible instead of summarising them away. A pass in one of the three independent areas (transparency / window behaviour / fps) says nothing about the others |

**Blocking non-conformance (stage 6).** The merged layout decision and the owner's
decision cannot both hold: the panel refuses to reserve space for the frame rows
(`gui/src/panel/mod.rs:27-29`), while the design requires the six frame rows'
**reserved widths** from the start, so that landing the data cannot reflow the panel
(its criterion 4). This is what blocks the *integration* — a reserved slot is where
the metrics are meant to live, and today there is nowhere for them to land. It blocks
nothing else: it is not a defect in the layout as it stands, and it is no reason to
change the panel before there is data to put in it. Settling the reservation, and
removing the four contradicting statements listed in §4, belong to the implementation
of stages 1 and 6.

**The dependency chain this table encodes.** Stage 1 is independent research into
whether real frames can be captured at all: the layer and its reader live outside the
UI, and nothing in the panel — the layout included — may block it. Stage 6 is what
needs the layout decision, and it needs it **after** frame capture is confirmed: with
`frame_count = 0` there is no data for a reserved slot to hold, so the non-conformance
above blocks the integration, not the research. Stage 7 gates on stages 1–6. The order
of work stays the one already agreed; this table records status against it and does
not re-plan it.

## 6. Already-merged infrastructure (context, not a stage)

These are the merged pieces the seven stages build on. They are **not** stages of
the first version, and none of them may be counted as one:

| Piece | State | Evidence and limits |
|---|---|---|
| Panel mode in one window, `--panel`, the in-window panel button and menu, `panel.json` config with its own file, version and normalize step | **PASS** for mode and config; **NOT TESTED** for persistence across a real restart | Live: 300x198 geometry, live width slider with clamping, settings-window open/close cycles (#116 body, measurement page). Persistence evidence is unit tests only. Mode is switched by restart, not by a toggle (that is stage 5) |
| Fixed layout, size computed from the row count, `—` for absent data | **PASS** | `height = visible_rows * row_height + 2 * padding` (`panel-measurements.md`, "Window size"); live 300x198 at the 10 default rows (`panel/layout.rs`, `panel/render.rs`) |
| Poll only the components the visible rows need; `logs` never | **PASS** | `window.rs::required_components` + `app.rs::sync_poll_set`; named tests `the_poll_set_covers_every_visible_row_and_nothing_else`, `no_visible_rows_means_nothing_is_polled`, `the_expensive_log_ring_is_never_in_a_panel_poll_set`. Supporting, non-gating live signal: the 4.8x idle-CPU ratio. The poll set on the wire was never captured live |
| Repaint on data arrival | **PASS** for its own criterion (PR #112: 225.6 → 1.0 ms avg, 168 s per case, release build) — and explicitly **not** frame capture | Code: `app.rs::request_repaint_now` with the `REPAINT_PENDING` one-shot latch, cleared at the start of `handle_hardware_updates`'s drain. See stage 1: it observes no presents |
| Panel draw / click-target contract | **PASS** | PR #122: the painted rect and `ui.max_rect()` are asserted equal at the call site, and the function's contract states that the return value is not the click target |

## 7. Other outstanding items from the earlier audit (not stages)

* **No README section about panel mode.** `grep -i panel README.md` is empty on the
  base; the panel is described only in `gui/src/panel/mod.rs` and the measurement
  page. Wanted eventually, with the Wayland caveats from §8 — but it is a
  documentation deliverable, not one of the seven stages.
* **The "drain into `logic()`" question (design Q3)** is unanswered. The spike
  refuted its motivation for `Visible(false)` (the drain keeps up), leaving WM
  iconify as the only case, and the merged tree has no `logic()` hook at all
  (`gui/src/app.rs` defines only `fn ui`). It is conditional in the design's PR A
  and is not part of any of the seven stages.
* **The settings window's in-window Close button** takes the same path as
  `WM_DELETE`, which is verified, but the click itself was never confirmed live.
* **A row checkbox changing the panel height** is covered by unit tests on the
  layout function; the live click was not performed.
* **An intermittent `lapsphere-daemon` test failure** was seen once in a full
  workspace run and did not reproduce in five consecutive package runs; its name is
  unknown because earlier output was filtered down to the result line.

## 8. Confirmed scope boundaries

* **Exclusive fullscreen is out of scope** — an owner product decision (#111):
  poorly behaved with modern desktop environments, no measured fps benefit, and
  most games do not use it by default. The target is windowed, borderless
  fullscreen, or a second monitor.
* **The host's session is Wayland now, and the panel is not claimed on Wayland.**
  The desktop runs **xfwl4 (Wayland) with Xwayland on `:0`**; that is the session the
  stage-4 measurement was taken in, and it is an **X11 window on Xwayland**, not a
  native Wayland surface. The merged panel uses only `ViewportCommand`s, which is the
  portable subset, but the X11 work of stages 2–3 is untested on Wayland, and on the
  available evidence `_NET_WM_STATE`-based switcher exclusion is X11-only and not
  achievable the same way there. Revision 4 **withdraws** the earlier statement that
  no Wayland session exists on this host.
* **A pure X11 session without Wayland is no longer what this host runs.** X11
  access is through Xwayland under the Wayland compositor. An Xvfb/X11 session can
  still be started, but it is software-rendered and xfwm4 refuses to composite under
  llvmpipe (§3.4), so it cannot carry the transparency criterion; tests in an X11
  session **without** Wayland are deferred and are not evidence for any row above.
  The compositing criterion has to be judged on a session that is shown to composite
  — here, the Wayland session's Xwayland.
* **Unverified environments stay unclaimed:** GNOME/mutter, KDE/kwin, and any
  compositor other than xfwl4. xfwm4 4.20.0 on X11 **without** a compositor is the
  only environment with evidence for **window behaviour** (stage 3), and the
  compositing state of those historical sessions is UNRESOLVED. **Transparency**
  (stage 4) has evidence in one environment only — X11 on Xwayland under xfwl4, on
  the unmerged PR #128 — so XWayland with a compositor is claimed for that stage and
  for nothing else.
* **Three areas need three separate verdicts, on separate evidence:**
  *transparency* (stage 4 — a compositing session now exists on this host, and the
  unmerged measurement is its only evidence), *window behaviour* (stage 3: stacking,
  taskbar/switcher, position, drag, multi-monitor placement), and *fps/frametime*
  (stages 1 and 6: layer, reader, elements, statistics). None of them can be carried
  by the panel's geometry or idle-CPU measurements, and a pass in one says nothing
  about the others.
* **Multi-monitor placement** is not implemented and not claimed.

## 9. Sources

| Claim | Source |
|---|---|
| Base commit, what it is | `git rev-parse origin/lapsphere`, `git log -1 17ece90` |
| #115 not merged, not patch-equivalent | `git cherry -v origin/lapsphere origin/feat/panel-ui` (18 × `+`), `git merge-base` = `be77233` |
| PR #116 supersedes #115 | PR #116 body, first paragraph |
| Merged X11 half absent | `gui/src/panel/window.rs:124-131`, `gui/src/panel/mod.rs:30-33` |
| Merged panel's live numbers and their limits | `docs/development/panel-measurements.md`, PR #116 body |
| Repaint latency measured | PR #112 body ("Measurements") |
| #115's numbers tied to no commit; failed items | PR #115 body ("Confirmed by measurement", "Failed and inconclusive") |
| Layer not working, `frame_count = 0`, `gipa = 0`, GO gate outstanding | `spike/vk-layer/STATUS.md` on `feat/mini-panel`, PR #111 body |
| `frametime-layer-spike.md` absent | `git ls-tree -r --name-only <branch> -- docs/development/` on both branches |
| Transparency record (B8: black without a compositor) | `docs/development/panel-overlay-tests.md` on `feat/mini-panel` |
| Transparency measured on a compositing session (stage 4) | `docs/development/panel-transparency.md` (PR #128, head `098c740`), PR #128 body |
| Host session is Wayland now: xfwl4 + Xwayland, compositing | `ps -eo pid,args` (`xfwl4 --backend=tty`, `Xwayland :0`), `xdpyinfo -display :0` (a depth-32 visual), `docs/development/panel-transparency.md` |
| No position / no hotkey / no transparency in the merged tree | `gui/src/panel/config.rs:23`, `:26-28`, `:29-30`; `gui/src/panel/mod.rs:27-29`, `:30-33`, `:36-37`; `gui/src/app.rs:1252`; `grep` over `gui/src` for `OuterPosition`/`WindowLevel`/`ClientMessage`/`with_transparent` |
| Owner decision (composition of the first version), non-goals, "no demo metrics" | PR #111 body |
| Agreed first-version stages 1–7 (this table's rows) | the owner-agreement recorded on PR #123; distinct from `panel-design.md`'s staged plan (PR A/D/C/E) |
| Design stages, criteria 1–13, ADRs, Q1–Q13 | `docs/development/panel-design.md` on `feat/panel-ui` |
| Retraction, gate-free status, UNRESOLVED compositing | `docs/development/panel-measurements.md` §"What these numbers are tied to", §"Compositing: unresolved", §"The right-click menu — a wrong diagnosis" |
