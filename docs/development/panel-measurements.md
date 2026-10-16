# Panel — PR 1 measurements and findings

Measurements for the vertical panel (branch `feat/panel-column`), taken on the
**debug** build against the live system daemon on **X11 / Xvfb :77 / xfwm4 4.20,
software rendering (llvmpipe)**.

**The compositing state of that session is UNRESOLVED.** This page previously
stated "compositor on"; no record supports that, and the records that survive point
the other way. Read "Compositing: unresolved" below before quoting anything here.

**Nothing on this page is a release gate.** These are single-session readings on
one machine, tied to a branch rather than to a commit SHA (see "What these numbers
are tied to"), with no compositing state established for them, and nothing here
has been re-measured since it was written.

For what is **merged** today and what still has to be done before the panel's first
version can be called accepted, see
[`panel-v1-baseline.md`](./panel-v1-baseline.md). That page records status and
re-uses the credibility limits below; it does not re-confirm any number on this one.

Real transparency (stage 4) is measured on its own page,
[`panel-transparency.md`](./panel-transparency.md). It is a different session — X11
on Xwayland under the xfwl4 compositor, which the sessions below did not have — so
its numbers are not mixed into these, and the compositing state of the sessions
below stays exactly what this page says it is.

Everything is read from outside the process: `/proc/<pid>/stat` for CPU jiffies,
`xwininfo` for window geometry, `xwd` + OCR for what was painted. No telemetry
was added to the app for the measurement.

## What these numbers are tied to

* **A named branch, not a commit SHA.** The numbers below are associated with the
  branch `feat/panel-column` — the branch that became PR #116, merged as
  `e296679`. The run did not record the SHA of the binary it measured, so the
  method can be re-run from that branch but the exact tree that produced these
  readings cannot be identified.
* **The older number is weaker still.** The 9x figure quoted from `feat/panel-ui`
  was recorded against a binary that corresponded to no single named commit at
  all, as that page states itself.
* **One machine, one session, debug build, software rendering.** Absolute jiffies
  under llvmpipe say nothing about the owner's hardware; only the ratios within a
  table carry meaning, and only on the machine that produced them.
* **Not a gate.** None of this is a release gate, and none of it has been
  re-measured since it was written.

## Compositing: unresolved

The environment line above used to read "compositor on". That is not established,
and neither is its opposite, so the state is recorded as **UNRESOLVED**: no log of
the measurement session survives in this repository, and the session's state
cannot be recovered from anything else in it.

The records disagree. Both are quoted rather than one being silently chosen:

| Record | What it states |
|---|---|
| This page as written for `feat/panel-column` (merged via PR #116) | xfwm4 4.20 with the **compositor on** |
| PR #111 (open, unmerged) — finding **B21** and open question **10** | xfwm4 **refuses** to composite under llvmpipe (`Unsupported GL renderer (llvmpipe (LLVM 21.1.8, 256 bits))`), so its results are the *harder* no-compositor case and "compositor-on cases" are listed there as **NOT TESTED**, the host having none |
| `feat/panel-ui` (the superseded branch), this same page | xfwm4 4.20.0, **compositor off** |

A fresh check was made while reconciling this page (2026-10-14, same host; **it is
not a re-measurement of any result below, and no result below was repeated**):
xfwm4 4.20.0 on a new `Xvfb` display with llvmpipe, started with
`--compositor=on`, logs the identical warning —

```
(xfwm4:PID): xfwm4-WARNING **: Unsupported GL renderer (llvmpipe (LLVM 21.1.8, 256 bits)).
```

— then manages the display (`_NET_SUPPORTING_WM_CHECK` is set) but leaves
`_NET_WM_COMPOSITING` **absent**: the request is refused. No standalone compositor
(`picom`, `compton`, `xcompmgr`) is installed on this host. On the live session
`xfconf-query -c xfwm4 -p /general/use_compositing` reads `true` while
`_NET_WM_COMPOSITING` is absent, and the value only applies at the next WM start —
so it is not evidence in either direction.

What this settles and what it does not. A compositing session in the environment
named in the header is **not reproducible here**, and no evidence supports the
"compositor on" line, so that claim has been removed rather than restated. That is
not proof of what the original session had — no log of it survives — which is
exactly why the state is left UNRESOLVED rather than flipped to "off". **Every
number on this page is therefore quoted without a known compositing state.**

## Method, and what it cannot show

* **CPU is sampled from `/proc/<pid>/stat` fields 14 and 15** (`utime`, `stime`),
  which the kernel counts in `USER_HZ` jiffies, read at both ends of a 60 s
  window. The mode is set by restarting the process with or without `--panel`
  rather than by a toggle, because no D-Bus toggle exists yet (that is PR 3) and
  a restart removes all doubt about which mode was under test. **Every run
  confirms the window geometry from `xwininfo` before sampling**, so a mislabelled
  arm cannot read as a result.
* **Rendering is software.** Absolute jiffies under llvmpipe say nothing about
  this hardware. The **ratio between the two modes** is the result.
* Both arms see the same daemon, so the poll traffic is comparable; what differs
  is what each surface draws.

## Idle CPU — 60 s per run, three runs each

| Mode | run 1 | run 2 | run 3 | mean | jiffies/s |
|---|---|---|---|---|---|
| panel (`--panel`, 300x198) | 120 | 111 | 106 | 112.3 | 1.87 |
| normal window (570x620) | 523 | 558 | 537 | 539.3 | 8.99 |

**The panel is about 4.8x cheaper than the normal window.** This inverts the
archived branch's finding, where panel mode measured **9x more expensive** than
the normal window (`docs/development/panel-measurements.md`, 60 s each, 52 vs 476
jiffies, recorded on `feat/panel-ui`) and was logged there as "a finding to
address, not a pass".

The cause is the poll set, not the rendering: the normal window polls every
registered component — including `logs`, a ~470 kB ring reply every five seconds
— while the panel polls only the components backing its visible rows. The panel
also draws ten fixed-height text lines against the tab's grids, progress bars and
collapsing headers.

Both modes request a repaint on the same terms: on polled-data arrival
(coalesced to one per frame) plus a 500 ms fallback tick. The panel is **not**
repainting every frame.

## Window size

The size is computed, not measured: `height = visible_rows * row_height +
2 * padding`, with `row_height` from egui's own metric for the resolved font.

| | measured |
|---|---|
| configured width | 300 px |
| window width | 300 px |
| window height (10 default rows) | 198 px |
| depth | 24 |

**`MinInnerSize` must be sent before `InnerSize`.** `main.rs` creates the window
with `with_min_inner_size(440x470)`; a WM clamps an `InnerSize` request to the
minimum it currently holds, so sending `InnerSize(300x198)` first is clamped
straight back to 440x470. Measured: 440x470 with the commands in the wrong
order, 300x198 with them in this one.

## The width slider, live

Dragging the slider in the settings window resizes the panel **during** the drag,
with no apply step — the width is read from the config every frame and the window
re-specified from it.

| step | drag | panel width |
|---|---|---|
| start | — | 300 |
| right | 384 → 404 → 424 → 444 | **640** |
| release | — | 640 |
| left | 400 → 360 | **410** |
| left | 380 → 300 | **200** |

640 and 200 are `MAX_WIDTH` and `MIN_WIDTH`, so the clamp holds in both
directions. Re-checked after the settings window was closed and reopened (two
cycles): 200 → 500, still working.

## What the panel looks like

Transcribed from a capture of the panel window, live data from the daemon:

```
CPU     AMD Ryzen 7 580...
Freq    2269 MHz
Load    1.5%
Temp    44.2°C
GPU     NVIDIA GeForce ..
Clock   -
Load    -
Usage   50.2%
Used    7.52 GiB
Total   14.97 GiB
```

A vertical `label: value` column. The em dashes are hardware that reported
nothing (this dGPU publishes neither clock nor load), and they occupy their row
rather than being hidden — which is what keeps the height a function of the row
count and nothing else.

The labels here are the **short** ones. An earlier capture of this same panel
read `Average Freq..` and `Package Temp..`: the row labels were being elided. A
truncated label makes a row unidentifiable at a glance, which is worse than a
wide panel, so each row now carries a short `panel_label` sized to the widest of
them and the **value** is what gets the ellipsis (visible above in the processor
and GPU names).

## Statistics is unchanged by the formatter extraction

Proven by replaying the same generated test against **both** trees
(`origin/lapsphere` and this branch) with one fixed `AppState` and comparing the
strings the tab actually paints:

| tree | result |
|---|---|
| `origin/lapsphere` (pre-extraction) | 2 passed; 0 failed |
| `feat/panel-column` (post-extraction) | 2 passed; 0 failed |

The probe asserts the tab still renders `42.8%`, `16.03 GiB`, `65.0°C`,
`3600 MHz`, `16.03 GiB`, `31.94 GiB` and the row labels — the values its inline
format calls produced before. The probe is not committed; it lives outside the
tree and is copied in, run and removed.

## Findings carried into PR 2

### Position persistence across restart — a known defect, not re-measured here

From the archived branch's measurements
(`docs/development/panel-measurements.md`, "Not verified"):

> **Position persistence across restart** — **Failed.** The stored config is
> `corner: top_left, offset 24,24`, but after a full process restart the panel
> came up at `0,43` — not the stored offset. Either `OuterPosition` is being
> clamped by the WM or the saved position is not being applied. **This is a real
> defect, not a limitation of the environment.**

Two things to take from it, both about ordering:

1. That branch's `apply_spec` sent `InnerSize` **before** `MinInnerSize` and
   `OuterPosition` last. The `MinInnerSize` half of that is the bug this PR found
   and fixed independently, measured above. `OuterPosition` sits in the same
   batch, so it is under the same suspicion: **a position sent once, alongside a
   size the WM was still clamping, is not evidence that the position was ever
   applied.**
2. The observed `0,43` is the WM's own placement, i.e. the position request was
   ignored rather than clamped to something derived from the config. Whatever PR 2
   does, it must **confirm the applied position by reading it back**
   (`xwininfo` / `xprop`) after the request, the same way the stacking atoms are
   confirmed by read-back — a request that is sent is not a request that landed.

The archived branch also recorded that its own measurement run was taken against a
binary that did not correspond to a single named commit. The numbers on this page
are one step better and no more: they belong to one named BRANCH,
`feat/panel-column`, at whatever its tip was when they were taken, and the run
recorded no commit SHA — so they describe that branch's behaviour, but no reader
can identify the exact tree that produced them. See "What these numbers are tied
to" and "Compositing: unresolved" above for the two limits that apply to every
number here.

The CPU table above and the geometry table below were both re-taken after the
menu and close-path fixes; none of them depend on either.

### The right-click menu — a wrong diagnosis, and what it cost

This page previously claimed the context menu could not open at the panel's real
size, and listed three ways to redesign it. **That diagnosis was wrong**, and the
wrongness is worth recording because the failure was in the method, not in egui.

The symptom: on :77 a right click visibly did nothing, while a temporary probe
showed `i.pointer.secondary_clicked() == true` and the `ui.interact` Response over
the panel reporting `false`. The obvious reading was that the Response never
receives egui's CLICKED flag, so `Response::context_menu` is unusable and the
workaround (reading the pointer off the context) was necessary.

It was not. The probe logged the two values in **separate statements**, and the
conclusion was drawn from the `tail` of each — **readings from different frames**.
With both values on one line of one frame:

```
PROBE ctx_sclick=true resp_sclick=true resp_clicked=false hovered=true
```

They are true together. `Sense::click` was always sufficient, and
`Response::context_menu` works. The menu is now driven the ordinary way, from the
Response, and the pointer is no longer read.

Two entries — "Panel settings…" and "Back to normal mode" — and it does fit:
about 52 px in a 300x198 window. They were moved out of the menu into a separate
360x520 settings viewport for a real reason (egui paints a popup into the surface
of the window that owns it, and twenty rows of checkboxes do not fit in 198 px),
but that is a design decision, not a failure to make the menu appear.

Cost of the wrong turn: one commit built the workaround, a second removed it.

### Window close paths

Verified live on :77, panel `0x400004`, settings window `0x40000b`:

| action | app | windows after |
|---|---|---|
| `wmctrl -ic <settings>` (WM_DELETE) | alive | panel only |
| two more open/close cycles | alive | panel only, width slider still works afterwards |
| `Alt+F4` on the settings window | alive | panel only |
| `Alt+F4` on the panel | closed | none — this is the window-close path and is meant to close the app |

**`xdotool windowclose` is not a close, it is a kill.** Its own man page: *"This
action will destroy the window, but will not try to kill the client controlling
it."* That is `XDestroyWindow` from outside, so winit unwraps a `BadWindow` while
querying a window that no longer exists, and the panic that follows is an artefact
of the test. It was read as a bug in this panel for one whole round, which is
what produced the "closing the settings window takes the app down" finding. That
finding was withdrawn. Real close paths are WM_DELETE_WINDOW (`wmctrl -ic`),
`Alt+F4`, the title-bar button and the in-window Close button.

One real bug did surface alongside it: the close check was
`ctx.viewport_for(id, |_| true)`, which can never be false, so `settings_open` was
never cleared when the WM button was used. Fixed by reading the documented
`close_requested` for the settings viewport id.

The in-window **Close** button takes that same path and is verified by code
reading, not by a live click — see "Not verified".

## Not verified

| Item | Why |
|---|---|
| fps, frametime, frametime graph, 1 % low, 0.1 % low, game name | **Implemented** (stage 6: `gui/src/panel/frames/`, registry rows under `Group::Frame`, the two settings in `panel.json`) and covered by unit tests, but **not measured here and never observed live**: this page's numbers were all taken without those rows. The owner's decision that they are first-release elements is settled, not open. What is measured about their source, and what is not, is on `panel-frames.md` and §5 row 1 of `panel-v1-baseline.md`. |
| The in-window Close button | Takes the same path as WM_DELETE, which is verified, but the click itself was not confirmed. |
| A row checkbox changing the panel height | Covered by unit tests on the layout function; the live click was not performed. |
| Compositing transparency | **Implemented since this page was written** (PR #128, merged `565930e`) and accepted on that commit; re-verified on `bbe0c4f` by the stage-7 run, which measured the panel's own ARGB surface (alpha 255 at opacity 1.0; a `(14,15,16,128)` fill at 0.5 with text alpha above it) — [`panel-stage7-acceptance.md`](./panel-stage7-acceptance.md) §6. This page's own sessions keep their **UNRESOLVED** compositing state — see "Compositing: unresolved". |
| Stacking / taskbar atoms | **Implemented since this page was written** (PR #127, merged `7262c5b`): asked of the WM and confirmed by read-back. On an X11 WM it works; on the host's xfwl4 session the taskbar/pager atoms are refused — [`panel-stage7-acceptance.md`](./panel-stage7-acceptance.md) §5. |
| Wayland | **Stale as written — corrected by revision 8 of `panel-v1-baseline.md` and by the stage-7 run.** This host now runs xfwl4 (Wayland) with Xwayland on `:0`, and the panel does run there as an X11 client; what it does *not* get on that session is the X11 window behaviour (the XWM refuses `_NET_WM_STATE_SKIP_TASKBAR`/`SKIP_PAGER` and overrules `OuterPosition`). A native Wayland surface is still not claimed. See [`panel-stage7-acceptance.md`](./panel-stage7-acceptance.md) §4.2 and §5.2 |
| Multi-monitor placement | Not implemented and not claimed. |
