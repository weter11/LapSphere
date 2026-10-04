# Panel — PR 1 measurements and findings

Measurements for the vertical panel (branch `feat/panel-column`), taken on the
**debug** build against the live system daemon on **X11 / Xvfb :77 / xfwm4 4.20,
compositor on, software rendering (llvmpipe)**.

Everything is read from outside the process: `/proc/<pid>/stat` for CPU jiffies,
`xwininfo` for window geometry, `xwd` + OCR for what was painted. No telemetry
was added to the app for the measurement.

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

The archived branch also recorded that its own measurement run was taken against
a binary that did not correspond to a single named commit. The numbers on this
page are from one named tree: `feat/panel-column` as committed.

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
| The in-window Close button | Takes the same path as WM_DELETE, which is verified, but the click itself was not confirmed. |
| A row checkbox changing the panel height | Covered by unit tests on the layout function; the live click was not performed. |
| Compositing transparency | PR 2. The owner checks this by hand; nothing is claimed here. |
| Stacking / taskbar atoms | PR 2. `stacking` and `hide_from_taskbar` are stored and normalized only. |
| Wayland | No Wayland session on this host, and the owner's does not start. The panel uses only `ViewportCommand`s; nothing here claims Wayland support. |
| Multi-monitor placement | Not implemented and not claimed. |
