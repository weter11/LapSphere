# Panel mode

The panel is a compact always-on-top overlay strip, in the spirit of MangoHud,
drawn on **the same window** as the main UI. Design decisions and their evidence
live in [`panel-design.md`](./panel-design.md); this page is the operational
view: how to use it, how it behaves, what was measured, and what was not.

## Using it

| Action | How |
|---|---|
| Enter panel mode | "📊 Panel" button in the main window's top bar, or start with `--panel` |
| Leave panel mode | "⬅ Normal mode" inside the panel — the **only** route back |
| Show / hide | Global hotkey (default `Shift_R+F9`), or `lapsphere --toggle-panel` |
| Panel settings | "⚙" inside the panel |

There is deliberately no other way back to the normal window. The panel hides and
shows on a hotkey; leaving it is always an explicit control.

### The hide-safety rule

Hiding the panel is permitted **only when a way back exists**:

- the hotkey was captured (`XGrabKey` succeeded), or
- the tray is enabled, or
- `lapsphere --toggle-panel` works (which needs the D-Bus method).

With none of those, a hide request **returns to the normal window instead**, with
a message saying why. Hiding would otherwise be a one-way trip: the user is left
looking at an overlay they cannot remove. Implemented as `panel::menu::can_hide`
and `hide_outcome`, with a test for each path.

The menu shows the current hotkey status, and reports a taken key as "Hotkey is
taken by another application" rather than as a silent failure.

## Behaviour

**Size is fixed by the element set, not by data.** The panel's width and height
come from the element list and the font scale alone. A missing value renders `—`
in an already-reserved slot. A panel that resized whenever a device appeared or
disappeared would move the user's click target, so it does not. This is ADR-2 in
the design document, and it is structural rather than aspirational: the layout
function never receives the application state.

**Units come from one module.** Every value is formatted in `panel::units`, with
one rule about case (`MHz`/`GHz` are not interchangeable, `W` is not `mW`, `°C`
is not `C`) and one rule about absence (`—`, never an empty string, never a
fabricated zero).

**Polling narrows in panel mode.** Only the components the visible elements need
are polled. No GPU element means `gpu` is not polled at all, and `logs` is never
in a panel poll set — its reply is ~735 kB and no panel element reads it. This
is a battery and CPU argument, not a stability fix.

**X11 window integration.** `SKIP_TASKBAR` and `SKIP_PAGER` are set through an
EWMH `_NET_WM_STATE` ClientMessage (`data.l[3] = 0`) and then **read back**,
retried while the atoms are absent. The read-back is what makes it
self-confirming: a window manager that ignores the request surfaces as "atoms
absent after N attempts" and a warning, not as a silent failure. `WindowLevel(AlwaysOnTop)`
is requested at runtime, because the `ViewportBuilder` hint is ignored by xfwm4.
The window **type** never changes — the root stays `Normal` for life.

**Wayland** has no `_NET_WM_STATE` a client may send, so the atom work is a
no-op there and position is compositor-owned. The panel still draws at whatever
size the compositor gives it, and `lapsphere --toggle-panel` is the way to reach
it. This is deliberate degradation, not a claim that it works.

## Files

| Path | Contents |
|---|---|
| `gui/src/panel/config.rs` | `panel.json` schema, normalization, tests |
| `gui/src/panel/items.rs` | The element registry: id → a function of `AppState` |
| `gui/src/panel/units.rs` | Every unit, one place |
| `gui/src/panel/render.rs` | Fixed layout, the 60-second graph ring |
| `gui/src/panel/window.rs` | Mode switching, the EWMH retry policy (pure) |
| `gui/src/panel/x11.rs` | The X11 backend and the draw call |
| `gui/src/panel/menu.rs` | The panel menu and the hide-safety rule |
| `gui/src/panel/visibility.rs` | Hotkey grab, D-Bus method, `--toggle-panel` |

## Acceptance criteria

Each states how it is checked. "By test" means an automated test in the tree;
"by measurement" means a live run on X11/xfwm4.

1. The normal UI is unchanged until panel mode is entered. — By test and by
   inspection: panel code is reached only behind `mode.is_panel()`.
2. The panel size depends only on the element set and the font scale. — By test:
   repeated layout computation with no data is identical; adding a data value
   cannot reach the layout function.
3. A missing value renders `—` in a reserved slot. — By test: every element read
   against an empty `AppState` yields the em dash.
4. Element order, visibility, label and per-item scale are configurable and
   persist. — By test on the config, and by the menu writing `panel.json` after
   each change.
5. No GPU element means no `gpu` poll; `logs` is never polled in panel mode. — By
   test on the poll set, and by counting coordinator polls in two windows.
6. Resuming the panel does not fire a burst of refreshes. — By test:
   `last_refresh` survives a pause.
7. The hide-safety rule holds for every combination of the three return paths. —
   By test: one assertion per path plus the refused case.
8. The default hotkey is `Shift_R+F9` and parses. — By test.
9. The window type is never changed. — By the shape of the API: the spec carries
   no type field; only a stacking level and click-through.
10. A panel larger than the work area, or an offset from another resolution,
    stays on screen. — By test over all four corners.
11. `lapsphere --toggle-panel` reaches a running GUI, and reports clearly when
    there is none. — By measurement.
12. A hidden panel comes back with the hotkey, the tray or the CLI. — By
    measurement.

## NOT TESTED

Everything below is unverified, with the reason. Nothing in this list should be
read as working.

| Item | Why not tested |
|---|---|
| Wayland (any claim) | This host has no Wayland session (`XDG_SESSION_TYPE=x11`) and the owner's Wayland session does not start. The design degrades rather than claims. |
| A compositor (mutter/kwin, GNOME, KDE) | No such session exists here. All X11 results are **xfwmm4 4.20.0 with the compositor off**. |
| Other X11 window managers | As above. The atom work is built to *detect* refusal (read-back + retry), so an unsupported WM should surface as a warning rather than a silent failure — but that is a design property, not a measurement. |
| Panel stacking over a genuine fullscreen cube | Depends on the compositor environment above; see `panel-overlay-tests.md` for what the earlier spike measured on xfwm4. |
| Anti-cheat interaction, DXVK/Proton | Out of scope; the layer is PR C. |
| The fps data path | The producer is PR E. Only the reserved slots and the receiver exist. |