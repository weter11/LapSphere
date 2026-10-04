# Panel measurements

Measurements for PR #115 (`feat/panel-ui`), taken on the release build against
the live daemon on **X11 / XFCE / xfwm4 4.20.0, compositor off**.

Everything here is read from outside the process: `/proc/<pid>/status` for RSS,
`/proc/<pid>/stat` for CPU jiffies, `xdotool`/`xprop` for window state, and the
GUI's own log lines for what it actually did. No telemetry was added to the app.

## Method, and the false negatives it had to exclude

Two of my own checks produced false negatives before producing numbers, and
both are recorded because they would otherwise read as results:

1. **`wmctrl -b add,hidden` without `-r` is a silent no-op.** An earlier soak
   used it, the window stayed visible, and the RSS curve was flat — which reads
   as "no leak" and measured nothing. Every mode change below is confirmed by a
   log line in the GUI, not by geometry alone.
2. **A test that started in the wrong state read as a failure.** The first
   NumLock/CapsLock round reported "hotkey did not fire" because the panel was
   already hidden, so the toggle was consumed by the show/hide path rather than
   the mode path. A positive control (a D-Bus toggle, which demonstrably works)
   is run alongside each such check.

## Bugs found and fixed by running it

Both were invisible to the unit tests and to a clean build.

| Bug | Evidence |
|---|---|
| The panel D-Bus object was exported on a *second* zbus connection, where it is reachable only by that connection's unique name. `--toggle-panel` hung forever. | `busctl --user tree io.lapsphere.Gui` returned an empty tree; even `busctl --user introspect` timed out (rc=124). Fix: `fead23e`. |
| The hotkey grabbed on one X connection and re-grabbed on a second; X11 delivers the `KeyPress` to the grab owner, and the event went to the connection nothing polled. Every press was lost while the log said "captured". | XTEST press: no log line, geometry unchanged across 3 presses. Fix: `0bec93d`. |

## Results

### CPU idle, normal vs panel mode — 60 s each

| Mode | CPU jiffies | jiffies/s |
|---|---|---|
| normal | 52 | 0.867 |
| panel | 476 | 7.933 |

**Panel mode costs roughly 9× the CPU of the normal window.** That is the
expected shape and not a defect in itself: in normal mode the window is idle and
egui is throttled, while the panel repaints on every polled update. But it is the
number that matters for a background overlay, and it is higher than I would want
without further work. **Treat this as a finding to address, not a pass.**

### Wakeups — 30 s window

Voluntary context switches 12131 → 12138 (+7); involuntary preemptions 1 → 1
(unchanged); minor faults 12 → 12 (unchanged). The measurement is a 30 s sample
and the deltas are small; it is evidence of no wakeup storm, not a precise figure.

### 300 mode switches via D-Bus

300 invocations in 18 s (50 ms apart, so the calls cannot race the GUI).

| Metric | Before | After | Delta |
|---|---|---|---|
| RSS | 98708 kB | 98712 kB | **+4 kB** |
| CPU | 873 jiffies | 926 jiffies | +53 jiffies |

**RSS is flat across 300 switches.** No leak from mode churn.

### 60 minutes with the panel hidden

Entered panel mode via the hotkey and hid it, both confirmed by log lines
(`hotkey pressed`, `panel: hidden; it can be brought back`), then sampled every
minute for 60 samples.

| | RSS |
|---|---|
| start | 98604 kB |
| min | 98152 kB |
| max | 98756 kB |
| end | 98152 kB |
| **delta end − start** | **−452 kB** |
| spread over the hour | 604 kB |

**No growth over 60 minutes hidden.** The 604 kB spread is allocator noise, and
the last three samples settle 452 kB *below* the start. CPU over the hour:
2946 jiffies = 0.818 jiffies/s, i.e. ~0.8 % of one core.

A first attempt at this soak died silently after 3 minutes with no OOM and no
panic report. It was re-run from scratch with the exit reason captured; the
numbers above are from the clean run, and the script now aborts rather than
report if the panel is not confirmed hidden first.

### Hotkey

| Condition | Result |
|---|---|
| `Shift_R+F9` via XTEST | fires — `hotkey pressed` then `window mode is now Panel`, geometry 1009x1098 → 3173x832 |
| NumLock on | fires |
| CapsLock on | fires |
| ScrollLock / all locks | fires |

Each press was confirmed by a matching `hotkey pressed` log line *and* a mode
line, never by geometry alone.

### X11 atoms, confirmed by read-back

| Mode | `_NET_WM_STATE` |
|---|---|
| normal | `_NET_WM_STATE_FOCUSED` |
| panel | `_NET_WM_STATE_STICKY, _NET_WM_STATE_SKIP_PAGER, _NET_WM_STATE_SKIP_TASKBAR, _NET_WM_STATE_ABOVE, _NET_WM_STATE_FOCUSED` |

`SKIP_TASKBAR` and `SKIP_PAGER` are present, so the panel is out of the taskbar
and pager, and `ABOVE` confirms the always-on-top level took effect at runtime.

### Stacking and click

With a normal window opened over the panel, `_NET_CLIENT_LIST_STACKING`
(bottom-to-top) reads `… 0x5400020 (the test window), 0x3000003 (the panel),
0x1000003 (the desktop panel)` — the panel is above the test window. A synthetic
click inside the panel sets `_NET_ACTIVE_WINDOW` to the panel's own window, so
the panel takes focus rather than the click falling through.

Alt+Tab moved the focus away and the panel stayed mapped at 3173x1098. The active
window afterwards read `0x0`, so **the focus half of the Alt+Tab test is
inconclusive** — xfwm4 did not hand focus to another window in this setup. What
is confirmed is that the panel survived Alt+Tab without being unmapped or
losing its geometry.

## Not verified

| Item | Why |
|---|---|
| **Panel over a fullscreen cube** | xfwm4 has no 3D desktop plugin here, so there is no cube to test against. A fullscreen *window* is not the same thing and is not substituted for it. |
| **Position persistence across restart** | **Failed.** The stored config is `corner: top_left, offset 24,24`, but after a full process restart the panel came up at `0,43` — not the stored offset. Either `OuterPosition` is being clamped by the WM or the saved position is not being applied. **This is a real defect, not a limitation of the environment.** |
| **Hotkey conflict with another application** | **Inconclusive.** I installed python-xlib and wrote a grabber for the same keycode+modifier, but every attempt to make the *competing* grab succeed failed with `BadAccess` — the grab was already held, and the script reported success only because the error was asynchronous. So the panel's "key already taken" path was never exercised with a genuine conflict. The code path exists and is unit-tested; the live conflict was not reproduced. |
| Compositing (mutter/kwin, GNOME, KDE) | No such session exists on this host. |
| Wayland, any claim | No Wayland session here, and the owner's does not start. |
| The fps data path | The producer is PR E. Only the reserved slots exist. |

## Provenance caveat

Partway through this measurement run a large unrelated change (~1780 lines across
`gui/src/panel/*` and `gui/src/app.rs`, including a second `Shift_R+F8`
"interactivity hotkey") appeared in the worktree, not authored by me. The binary
under test emitted log lines about that F8 hotkey, which does not exist at my
branch head `0bec93d`.

The measurements above were therefore taken against a binary that does **not**
cleanly correspond to a single commit. The findings that depend only on behaviour
present at `0bec93d` — the F9 hotkey, the atoms, the RSS curves — are unaffected
in substance, but **the exact build provenance of this run is not established**,
and the run should be repeated on a binary built from a named commit before any
of it is quoted as a release-gating number.