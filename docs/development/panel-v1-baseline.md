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

*Revision 5 (a commit on the stage-4 acceptance PR): stage 4 is `PASS`. The
acceptance run on the merge commit `565930e` drove the opacity slider live — the
panel's surface went to alpha `122` and back to `255` with the process never
restarting — in the session §8 names. The run, its commands, its numbers and what
it does not claim are on
[`panel-transparency.md`](./panel-transparency.md) ("The acceptance run — merge
commit `565930e`"). §5 row 4 and §8 are updated by it. No other row, criterion or
conclusion changed, and this page's own base and the line numbers below are still
as of `17ece90`.*

*Revision 6 (a commit on the stage-6 implementation PR; base `b0b27e3` = the merge of
PR #130): **two rows change, and only two.** Stage 1 is no longer a failure:
interception landed in PR #124 (merge `ec7a359`, head `edbb096`), and the three
questions that stage left open — overhead in microseconds, agreement with MangoHud,
robustness — were measured on PR #125, which is **open**, on a clean tree at commit
`514bba3ce7ed`. §3.3 and §5 row 1 record that evidence and, just as importantly, what
it does not cover. Stage 6 is implemented and unit-tested on this commit's tree and is
recorded as `IMPLEMENTED — live acceptance NOT TESTED` rather than `PASS`, because this
page's rule is that a status needs an observation of the *behaviour*, and no live game
was driven with the reader in the loop; §5 row 6 and the "blocking non-conformance"
paragraph are rewritten by it, and §4's inconsistency table is settled. The six frame
elements now have their own page, [`panel-frames.md`](./panel-frames.md). Stage 7 is
not started and nothing here claims it.*

*Not re-checked by revision 6, and not claimed by it either: the page's own base and
every line number outside §3.3, §4 and §5's rows 1 and 6 are still as of `17ece90`. In
particular §5's rows 2, 3 and 5 are left exactly as revision 5 wrote them — this
revision offers no opinion on them, and they were not re-based onto the merges of PRs
#126, #127, #129 and #130 that followed their evidence. One of them is demonstrably
out of date as of this base and is flagged rather than silently repaired: row 3 says
"no `WindowLevel` anywhere in `gui/src`, no `ClientMessage`, no `_NET_WM_STATE`, no X11
code at all", while `gui/src/panel/ewmh.rs` and `gui/src/panel/window.rs` in this tree
send exactly those (PR #127, merge `7262c5b`). Re-stating stages 2, 3 and 5 belongs to
whoever holds their evidence; offering a verdict here would be a status assigned with
no observation behind it.*

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

### 3.3 The Vulkan layer — round 1 (#111), interception merged (#124), the gate measured on an open PR (#125)

**Round 1 is kept as history, because its diagnosis is instructive.** On branch
`feat/mini-panel`, `spike/vk-layer/STATUS.md` said *"**NOT WORKING** … nothing here has
produced a measurement yet"*: the layer was enumerated and loaded, `vkCreateInstance`
completed, `frames-<pid>` was created in `$XDG_RUNTIME_DIR/lapsphere/` with mode
0600 and the protocol-v1 size — and

```
enabled; segment created gipa=0 create_instance=1 create_device=0 gdpa=0
  destroy_device=0 create_swapchain=0 destroy_swapchain=0 queue_present=0
```

so `frame_count` stayed **0**, because our `vkGetInstanceProcAddr` was never called.
**Round 2 retracted the conclusion drawn from that counter**, and the retraction is
worth naming: the counters were printed at the end of `vkCreateInstance`, a point at
which only zero is possible, and the real blockers were three uses of the loader's
dispatch chain — the `VK_LAYER_LINK_INFO` node was never consumed (so
`libVkLayer_MESA_device_select.so` called itself), and "next" was resolved from the
loader terminator instead of from the link. The loader's own line numbers for all
three are in the header of `spike/vk-layer/src/lib.rs` on the merged tree. The manifest
`functions` map was a red herring: the loader consults it only when negotiation
supplied no `vkGetInstanceProcAddr`.

**Interception is merged, and it was measured.** PR #124 (merge `ec7a359`, branch head
`edbb096`) records `vkQueuePresentKHR=900` for `vkcube --c 900`, **899** intervals in
the shared-memory ring — `writer_final: intervals=899 dropped=0 presents=900`, asserted
by the harness rather than inferred — read back by an independent second process, with
the application exiting 0. With the layer's variable unset: no segment, no layer log,
the loader never activates it (checks C3/C3b). Both manifest shapes intercept 900
presents (C4). The ring's wraparound run returns the full 4096 slots with **0**
zero-valued slots (C5).

**The GO gate now has measured results — on PR #125, which is open.** Its battery ran
on a clean tree (`0 dirty paths`) at commit `514bba3ce7ed`, build id
`514bba3ce7ed/718336e1a120`:

* **overhead** — the layer's own in-band sample of its present work: median
  **0.050–0.090 µs** unthrottled and **0.120–0.141 µs** under vsync (p99 0.12–0.88 µs),
  with an out-of-band control (`primitive_bench`: 30 ns for the clock pair, 10–30 ns for
  `record()`, 6.792 µs for a hot 64 KiB mmap+copy) showing what the measurement itself
  costs. The throughput A/B is a *rate* delta (single reps scatter ±2.5–3.7 µs), and
  PR #125 reads it as a bound on the whole hook — below ~1–2 µs — rather than as an
  overhead figure. No frame time is quoted as overhead;
* **agreement with MangoHud** — p50 **6.0662 vs 6.0661 ms** (164.85 fps both), p99
  9.8543 vs 9.8243 ms; aligned per-frame durations median **+0.54 µs** (limiter off) and
  **+0.67 µs** (limiter 120). The gate's "median within 1 %" is met, and p99 is read
  against the run-to-run noise floor, as the design says it must be;
* **robustness** — R1 clean exit; R2 `kill -9` (status 137, orphan segment readable,
  `--prune` clears it, next run normal); R3 three live resizes (2500 presents, 4
  swapchain creations, `intervals = 2500 − 1 − 3` **holds**); R4 two instances; R5 no
  reader at all (`intervals = presents − 1`, exit 0); R6 reader cost 27–98 µs per poll
  at 60 and 240 Hz, 0.25–2.875 % of one core; R7 a writer that cannot be created stays
  fail-open; R8 `SIGSTOP`/`SIGCONT`. Two reader defects were found and fixed there with
  regression checks.

**What that does not cover, and must not be read as covered:** the pid-reuse `O_EXCL`
variant (it needs a pid namespace, and unprivileged ones are unavailable on this host —
measured), 240 Hz as a *present* rate (the panel is 165 Hz; the 240 Hz figure is a
reader poll rate or a synthetic writer), an interactive resize on the host session (it
offers no window control: `wmctrl -l` prints nothing and `xdotool search --class .`
finds no window while an xlib `vkcube` runs, which is why R3 ran under Xvfb+lavapipe),
and every workload other than `vkcube` — no DXVK/Proton/Wine, no OpenGL, no 32-bit, no
anti-cheat, no GPU-bound title.

**`docs/development/frametime-layer-spike.md` still does not exist on any branch**
(checked with `git ls-tree -r --name-only` on `feat/mini-panel` and `feat/panel-ui`), so
`panel-design.md`'s criteria 9–12, whose status text says "Measured in the spike — see
`frametime-layer-spike.md` §3", still point at a file that is not there. Those criteria
are no longer *unresolved* — the measurements exist — but they are not *here*: read them
against `spike/vk-layer/STATUS.md` and `spike/vk-layer/TESTING.md` on PR #125, or
against PR #125's body, which carries the same tables.

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
  **works** (§3.3: interception merged in PR #124; the GO gate's measurements on PR
  #125, still open), and the GUI reads its segment directly — no daemon method, no
  D-Bus object, no protocol change (stage 6).
* First-release non-goals, kept so the scope cannot widen silently: no OpenGL hook,
  no 32-bit games, no Wayland-specific layer work (every Wayland *claim* is NOT
  TESTED), no anti-cheat and no DXVK/Proton coverage (the spike ran on `vkcube`).
* **Do not substitute demonstration metrics.** The `gui/examples/*` demo and probe
  binaries on #111/#115 are throwaway instruments; their numbers are not product
  measurements and must not stand in for the layer's.

**The four contradicting statements: settled by the implementation (stage 6).** The
merged tree used to state the opposite of this decision in prose, and revision 5 listed
the four places. All four are gone, in the stage-6 commit:

| Where | What it says now |
|---|---|
| `gui/src/panel/mod.rs` | "The frame elements are first-release elements — settled": the registry rows, the reserved slot, the two settings and the reader are named, and the "no reserved slots" argument is recorded as withdrawn |
| `gui/src/panel/mod.rs` | the "the decision is the owner's and is not made here" note is replaced by what that decision produced |
| `gui/src/panel/config.rs` | "**No frame-time / fps rows and no ring buffer**" is replaced by the two keys those rows need (`window_seconds`, `refresh_hz`) and by the note that their defaults are the design's *proposals*, not owner decisions |
| `docs/development/panel-measurements.md`, "Not verified" | the frame elements are no longer called unimplemented or undecided; the row now points at the implementation and states what is still unobserved |

The first of those was the **layout decision** stage 6 depended on, and the reservation
is now real rather than promised: `layout::Slot::GraphStrip` is three row heights at
every font size, the label column is the measured `LABEL_WIDTH`, and
`the_same_geometry_with_and_without_frame_data` asserts that a config paints identical
rects with and without data. The six rows are **off by default** — the same rule the
gamepad block follows — so no existing user's panel changes size on upgrade.

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
| 1 | **Research: real fps / frametime from inside the game process** (Vulkan implicit layer + shared-memory protocol) | **GO gate MET — on evidence that is partly merged and partly on an open PR** | Interception is **merged**: PR #124, merge `ec7a359`, branch head `edbb096` — `vkQueuePresentKHR=900` for `vkcube --c 900`, 899 intervals in the ring, read back by an independent process, exit 0; inert with the variable unset; both manifest shapes intercept; 0 zero-slots after wraparound. The gate's four areas are measured on **PR #125 — open, unmerged** — a clean-tree battery at commit `514bba3ce7ed` (build id `514bba3ce7ed/718336e1a120`): (a) inert without its variable — the merged spike's C3/C3b checks; (b) overhead median **0.050-0.090 us** unthrottled and **0.120-0.141 us** vsync, with the throughput A/B bounding the whole hook below ~1-2 us, i.e. under the gate's 10 us median; (c) agreement with MangoHud p50 **6.0662 vs 6.0661 ms** (164.85 fps both), aligned per-frame median **+0.54 us** / **+0.67 us**; (d) robustness R1-R8 — clean exit, `kill -9` (137, orphan readable and prunable), three live resizes (2500 presents, 4 swapchain recreations), two instances, no reader, reader cost 27-98 us/poll at 60 and 240 Hz, fail-open, `SIGSTOP`/`SIGCONT`. Criterion 13's Rust-vs-reference parity test now exists (stage 6, `frames/stats.rs`, run against `docs/development/tools/frametime_stats.py` itself) | What the gate does not cover stays uncovered: the pid-reuse `O_EXCL` variant, 240 Hz as a **present** rate, an interactive resize on the host session, and every workload other than `vkcube` (no DXVK/Proton/Wine, no OpenGL, no 32-bit, no anti-cheat, no GPU-bound title). Under this page's own rule an unmerged branch is never sufficient for `PASS` (§3.4), so the row is recorded as **GO gate met on named evidence** rather than `PASS`: PR #125 must merge, or its battery be re-run on the merged tree, before that word is used. `docs/development/frametime-layer-spike.md` still does not exist, so `panel-design.md`'s criteria 9-12 point at a file that is not there; their measurements live in `spike/vk-layer/STATUS.md`, `TESTING.md` and PR #125's body |
| 2 | **Position: save and restore**, clamped against the monitor size and confirmed by read-back | **FAIL** | The merged tree has **no position code at all**: no `OuterPosition` / `ViewportCommand::OuterPosition`, no monitor clamp anywhere in `gui/src`, and no position field in `PanelConfig` (`gui/src/panel/config.rs:185-196`). `gui/src/panel/config.rs:26-28`: "**No position.** The panel is placed by the window manager / desktop for now; a stored corner is the second-monitor question, which is explicitly NOT claimed"; `gui/src/panel/mod.rs:36-37` repeats it for multi-monitor. The only attempt is on the unmerged branch: #115's historical result — stored `top_left, offset 24,24`, the panel came up at `0,43` after a restart — recorded there as a real defect, tied to no single commit, and therefore **not** a verification of anything in the merged tree | Implement (with the stacking work), apply, and confirm the applied position by read-back (`xwininfo`/`xprop`) rather than trusting the request — a request that is sent is not a request that landed (§3.4). Multi-monitor placement stays out of the first version unless the owner decides otherwise |
| 3 | **Window flags and behaviour over other windows**: runtime always-on-top, no taskbar/switcher entry, no-op-tolerant where the platform cannot do it | **FAIL** | Provably absent in the merged tree: `apply_spec` sends only `MinInnerSize`, `Resizable`, `Decorations`, `InnerSize` (`gui/src/panel/window.rs:124-131`); no `WindowLevel` anywhere in `gui/src`; no `ClientMessage`, no `_NET_WM_STATE`, no X11 code at all; `stacking` and `hide_from_taskbar` are parsed, clamped and normalized only. `gui/src/panel/mod.rs:30-33` assigns the atoms to a later PR and states that nothing claims they are applied. Historical (not evidence about the merged tree), from #111's overlay tests on xfwm4 4.20 with **no compositor**: the **runtime** `WindowLevel(AlwaysOnTop)` is honoured where the startup builder hint is ignored (B9/B14), and ADD of `SKIP_TASKBAR`+`SKIP_PAGER` by EWMH `ClientMessage` is honoured on a `Normal` window once the message arrives after the window settles (0 ms and 50 ms fail, 100 ms+ works — B18/B18d) | Implement, then measure the **combination** — one window that is both always-on-top *and* skip-taskbar, over a borderless-fullscreen game — which is **NOT TESTED anywhere**; each half was proven separately. No claim about another WM, about a compositor-on session, or about Wayland (`_NET_WM_STATE` is X11-only, and a Wayland client may not set it) |
| 4 | **Real transparency** (an actual translucent surface, not a drawn rectangle) | **PASS** on the merge commit `565930e` | In the merged tree at this page's base `background_opacity` is a config field only: defined with `MIN_BACKGROUND_OPACITY = 0.25`, clamped and normalized (`gui/src/panel/config.rs`), and **nothing applies it** — no `with_transparent`, no ARGB or alpha path anywhere in `gui/src`, and the panel's frame is left at egui's default opaque `CentralPanel` because "the opaque background and its transparency are PR 2's work" (`gui/src/app.rs:1252`). Historical: #111's B8 measured that a transparent window shows the root window's black rather than the desktop **when no compositor is present**. Implemented and measured on **PR #128 (unmerged, head `098c740`)**, in a session that composites — X11 on Xwayland under xfwl4: the window holds an ARGB visual (`Depth 32`), its own surface carries the configured alpha (one value `255` at 1.0; `128` at 0.5 with the glyph pixels still `255`), the screen at 50 % blends the fill with the measured backdrop (which is not visible through it at 100 %) while the text does not fade, and the ordinary window's surface is alpha `255` in every sampled pixel. Numbers, session and limits: [`panel-transparency.md`](./panel-transparency.md). **This is now `PASS`**: the implementation landed in the merge commit `565930e` (PR #128, 2026-10-14), and the acceptance run on that commit is the section "The acceptance run — merge commit `565930e`" on that page — clean worktree at `565930e`, `cargo build -p lapsphere --bin lapsphere` + `cargo test -p lapsphere --bin lapsphere` (163 passed), binary sha256 `81ab06c7…f6896`, session X11 on Xwayland under xfwl4, `Depth 32` surface, `255` in every surface pixel at 100 % with **0** of 87 271 sampled screen pixels inside the panel showing the green backdrop behind it (screen == surface exactly for 81 005, within ±2 for a further 5 426, and 840 further apart — the values repainted between the two captures); the slider dragged live to 0.48 gives a `(13,14,15)` fill at alpha `122` with the glyph pixels still `255` and reading `(219,220,219)` on screen (text sharing the opacity would read `(105,183,105)`) and the backdrop showing through at `B_eff`/backdrop = 0.748; one drag back to the right end returns every surface pixel to `255` and `panel.json` to `1.0` **with the process never restarting** (`pid 2475744` throughout, `panel.json` written by the app); the context menu is alpha `255` over 30 163 of 31 832 sampled pixels of its rectangle while the background behind it is `122`; and without `--panel` the ordinary window is `Depth 32` with all 1 107 882 surface pixels alpha `255` and **0** pixels showing the backdrop | Still NOT TESTED anywhere: a compositor other than xfwl4, a session without a compositor, and native Wayland. The slider's live path is claimed for a session in which injected pointer input reaches the X11 client; the condition that has to hold for that — and the state of this session in which it did not — is named on the transparency page ("The input route") |
| 5 | **Hotkey and D-Bus** control (mode toggle by key and over the bus, with the hide-safety rule) | **FAIL** | Absent from the merged tree: no hotkey handling (no `F8`/`Shift_R` code; `gui/src/panel/mod.rs:30-31` assigns it to a later PR), no panel D-Bus method in `daemon/src` or the GUI, no `--toggle-panel`; mode is switched by restarting with `--panel` or from the in-window button and menu. `gui/src/panel/config.rs:29-30`: "**No hotkey or D-Bus fields.** Those arrive with the visibility work (PR 3)". Historical, branch-only: #115 recorded 20/20 `--toggle-panel` runs and the hotkey firing under XTEST with NumLock/CapsLock/ScrollLock, and found three defects that exist **only** as its fixes — the D-Bus object exported on a second connection (the toggle hung), the key grabbed on one X connection and polled on another, and a function-key keysym/keycode error | Implement; carry those three fixes instead of rediscovering them. NOT TESTED anywhere: the "key already taken" (`BadAccess`) path, keyboard pass-through, and every Wayland claim |
| 6 | **Integration of the metrics into the panel** (the six frame elements, their config block, the shm reader) | **IMPLEMENTED and unit-tested on this commit — live acceptance NOT TESTED** | Six registry rows with the decision's ids under `rows::Group::Frame`, a reserved fixed-height slot (`layout::Slot::GraphStrip`, `GRAPH_STRIP_ROWS` = 3 row heights, at every font size), the two config keys with backwards-compatible defaults and clamps, a settings section, and the reader: protocol-v1 validation in a fixed order with the offending value named, a seqlock read that **discards** a snapshot whose sequence moved, ADR-5's selection rule (`STALE_AFTER = 2 s`, larger pid on ties, `CLOCK_MONOTONIC`), statistics ported from the reference definitions, `-` and an empty strip for an absent source, and no file operation on the rendering path. Evidence: **423 tests pass, 0 fail** on this commit's tree (`cargo test`; 341 before this change, 82 of them new), including the absence paths, the ring-wraparound and seqlock cases, a real writer thread whose every accepted snapshot must be self-consistent, the geometry-invariance test and the reference-script parity test. The blocking non-conformance is settled (below) | A **live** observation: the panel over a running Vulkan game, with the layer installed and `LAPSPHERE_FRAMES=1`, on a named commit. Nothing in this PR observes the behaviour, so this row is **not** `PASS` under §3.4's rule — unit tests are not an observation of behaviour. Also unmeasured: the panel's own cost with these rows on, and whether 4 Hz is enough for the strip to look like a graph rather than a staircase. Details, activation requirements and the limits: [`panel-frames.md`](./panel-frames.md) |
| 7 | **Final acceptance of the first version** | **NOT TESTED** — cannot start | No end-to-end acceptance run exists on any commit. The evidence behind stages 1–6 is now a mix that must be kept apart when it is used: measured on an unmerged PR (1 — overhead, MangoHud agreement, robustness), merged and measured (1 — interception), implemented and unit-tested with no live observation (6), and absent from the merged tree or held by other PRs (2–5). No acceptance criteria for the agreed stages existed in the merged tree before this page, and no environment matrix has been recorded for an acceptance run | Close stages 1–6 first. The run must name the commit and the build, state per-criterion evidence, name the environment it ran in (WM, compositing state, GPU, X11 or Wayland), and keep its `NOT TESTED` items visible instead of summarising them away. A pass in one of the three independent areas (transparency / window behaviour / fps) says nothing about the others |

**The blocking non-conformance (stage 6): settled.** Revision 5 recorded that the merged
layout decision and the owner's decision could not both hold — the panel refused to
reserve space for the frame rows, while the design required their reserved widths so
that landing the data could not reflow the panel. The stage-6 commit implements the
reservation instead of arguing about it: a strip slot of a fixed number of row heights,
a label column sized to the widest registered label and asserted by measurement, a value
column that elides, and `-`/an empty strip in the reserved space when there is no source.
It blocks nothing else, and it never did: the layout was not defective before the data
existed, and the six rows are off by default, so no existing panel changes size.

**The dependency chain this table encodes.** Stage 1 is independent research into
whether real frames can be captured at all: the layer and its reader live outside the
UI, and nothing in the panel — the layout included — may block it. Stage 6 needs the
layout decision, and it needed it **after** frame capture was confirmed — which is now
the case: presets are intercepted, the intervals are recorded, and the reservation the
integration needed is in the layout. Stage 7 gates on stages 1–6, and stage 7 still
cannot start. The order of work stays the one already agreed; this table records status
against it and does not re-plan it.

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
  (stage 4) has evidence in one environment only — X11 on Xwayland under xfwl4,
  now on the merge commit `565930e`, from the acceptance run — so XWayland with a
  compositor is claimed for that stage and for nothing else.
* **Three areas need three separate verdicts, on separate evidence:**
  *transparency* (stage 4 — `PASS` on the merge commit `565930e`, in one
  compositing session: X11 on Xwayland under xfwl4), *window behaviour* (stage 3: stacking,
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
| Round 1: layer not working, `frame_count = 0`, `gipa = 0`, GO gate outstanding (kept as history, retracted in round 2) | `spike/vk-layer/STATUS.md` on `feat/mini-panel`, PR #111 body |
| `frametime-layer-spike.md` absent | `git ls-tree -r --name-only <branch> -- docs/development/` on both branches |
| Transparency record (B8: black without a compositor) | `docs/development/panel-overlay-tests.md` on `feat/mini-panel` |
| Transparency measured on a compositing session (stage 4) | `docs/development/panel-transparency.md` (PR #128, head `098c740`), PR #128 body |
| Stage-4 acceptance on the merge commit: the build, the tests, the slider driven live, the context menu, the ordinary window | `565930e` in a clean worktree; `cargo build -p lapsphere --bin lapsphere` and `cargo test -p lapsphere --bin lapsphere` (163 passed); `xwininfo` / `xwd` / `grim` / `xdotool`, written up on `docs/development/panel-transparency.md` |
| The input route: XTEST reaches the X11 client while the compositor's pointer is over Xwayland, and not otherwise; synthetic core events are ignored by the toolkit | `xev` under a 904x654 and then a 2500x1300 X11 backdrop (`synthetic NO` vs `synthetic YES`); winit 0.30.13 `platform_impl/linux/x11/event_processor.rs`, the `x11::GenericEvent` / XInput2 branch |
| Host session is Wayland now: xfwl4 + Xwayland, compositing | `ps -eo pid,args` (`xfwl4 --backend=tty`, `Xwayland :0`), `xdpyinfo -display :0` (a depth-32 visual), `docs/development/panel-transparency.md` |
| No position / no hotkey / no transparency in the merged tree | `gui/src/panel/config.rs:23`, `:26-28`, `:29-30`; `gui/src/panel/mod.rs:27-29`, `:30-33`, `:36-37`; `gui/src/app.rs:1252`; `grep` over `gui/src` for `OuterPosition`/`WindowLevel`/`ClientMessage`/`with_transparent` |
| Owner decision (composition of the first version), non-goals, "no demo metrics" | PR #111 body |
| Agreed first-version stages 1–7 (this table's rows) | the owner-agreement recorded on PR #123; distinct from `panel-design.md`'s staged plan (PR A/D/C/E) |
| Design stages, criteria 1–13, ADRs, Q1–Q13 | `docs/development/panel-design.md` on `feat/panel-ui` |
| Retraction, gate-free status, UNRESOLVED compositing | `docs/development/panel-measurements.md` §"What these numbers are tied to", §"Compositing: unresolved", §"The right-click menu — a wrong diagnosis" |
| Stage-1 interception merged (counters, interval count, inert-without-variable, both manifests, wraparound) | PR #124 (merge `ec7a359`, head `edbb096`); `spike/vk-layer/STATUS.md` and `TESTING.md` on the merged tree |
| Round-1 diagnosis retracted, with the loader's line numbers | `spike/vk-layer/src/lib.rs` header on the merged tree; `spike/vk-layer/STATUS.md` ("Why round 1 stalled — three causes") |
| GO gate measured: overhead in us, MangoHud agreement, robustness R1-R8, reader cost at 60/240 Hz, the NOT TESTED list | PR #125 (open), body and `spike/vk-layer/TESTING.md` on branch `spike/vk-layer-stage1b`; clean-tree battery at commit `514bba3ce7ed`, build id `514bba3ce7ed/718336e1a120` |
| The six frame elements implemented: registry rows, reserved slot, config keys, reader, statistics, tests | this commit; `gui/src/panel/frames/`, `gui/src/panel/{rows,layout,render,config,menu,window}.rs`; `cargo test` = 423 passed / 0 failed |
| The statistics definitions are the reference's | `docs/development/tools/frametime_stats.py` (unchanged from `feat/mini-panel`, sha256 `4058a81f...`), `gui/src/panel/frames/stats.rs` |
| Activation requirements (`LAPSPHERE_FRAMES=1`, implicit-layer path, why `VK_LAYER_PATH` is not enough) | `spike/vk-layer/manifests/VK_LAYER_LAPSPHERE_frames.json` and `spike/vk-layer/install.sh` comments, merged tree |
| Stage-6 status, activation steps, tested/not-tested lists | `docs/development/panel-frames.md` (added by this commit) |
