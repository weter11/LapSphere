# Panel mode

The panel is a compact always-on-top overlay strip, in the spirit of MangoHud,
drawn on **the same window** as the main UI. Design decisions and their evidence
live in [`panel-design.md`](./panel-design.md); this page is the operational
view: how to use it, how it behaves, what was measured, and what was not.

## Using it

| Action | How |
|---|---|
| Enter panel mode | "📊 Panel" button in the main window's top bar, or start with `--panel` |
| Leave panel mode | "⬅ Normal mode" — the leading button, or the same item in the right-click menu — the **only** route back |
| Show / hide | Global hotkey (default `Shift_R+F9`), or `lapsphere --toggle-panel` |
| Open the menus | **Right-click anywhere on the panel** |
| Move the panel | **Left-drag** the panel (window-manager drag); the new position is stored as a corner plus an offset |
| Panel settings | "Panel settings…" in the right-click menu |
| Click-through on/off | The "Click-through" item, or the interactivity hotkey (default `Shift_R+F8`), or `lapsphere --toggle-interactive` |

There is deliberately no other way back to the normal window. The panel hides and
shows on a hotkey; leaving it is always an explicit control.

### The right-click menu

Right-clicking the panel opens a menu with, in order:

| Section | Items |
|---|---|
| Elements | One visibility checkbox per element, each showing that element's current value |
| Window | Stacking (`Always on top` / `Normal window`), and `Hide from taskbar` while always-on-top |
| Interactivity | `Click-through`, with the reason it is unavailable when there is no way back |
| Routes out | `Panel settings…`, `⬅ Normal mode`, `Hide (Shift_R+F9)` |

`Hide` is greyed out — with the reason in the tooltip and in the text below it —
when no return path exists, per the hide-safety rule below.

### The settings window

`Panel settings…` opens a **child viewport** (`show_viewport_immediate`), window
type `Normal`, never always-on-top: it must not pin itself above the panel it
configures. It is a second OS window without a second GL surface, and the root
window's own type is never touched.

| Section | Contents |
|---|---|
| Window | Stacking, hide-from-taskbar, corner, reset position |
| Hotkeys | Show/hide key and interactivity key, each with its own grab status and the conflict message |
| Interactivity | Click-through, gated by the safety rule below |
| Text | Global font scale, with a reset |
| Elements | Per element: order (⬆/⬇), visibility, label, per-element font scale with `Inherit` |

Element reordering uses the same pattern as `pages/settings.rs`'s Section Order:
the move request is collected during the loop and applied after it, so two swaps
requested in one frame cannot fight each other.

Everything changed in either the menu or this window is written to `panel.json`.
No field of `settings.json` and no field of `tray.json` is read or written.

### The click-through safety rule

`MousePassthrough` makes the window stop taking mouse input, which means the
right-click menu and the settings window both become unreachable. Click-through
may therefore only be **switched on** when there is a way to switch it off:

- the interactivity hotkey was captured (`XGrabKey` succeeded), or
- `lapsphere --toggle-interactive` works (which needs the D-Bus method).

With neither, the checkbox is disabled and the reason is spelled out in the menu
and in the settings window. The tray deliberately does **not** count here: it is a
separate window whose menu the panel cannot drive. Implemented as
`panel::menu::can_enable_click_through`, with a test for every combination.

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

**X11 window integration.** Four `_NET_WM_STATE` atoms are driven: `ABOVE`,
`STICKY`, `SKIP_TASKBAR` and `SKIP_PAGER`. Each is sent as its own ClientMessage
(`data.l[3] = 0`) and then **read back**, retried while the read-back does not
equal the wanted set. The read-back is what makes the send self-confirming: a
window manager that ignores the request surfaces as "state not reached after N
attempts" and a warning, not as a silent failure. `WindowLevel(AlwaysOnTop)` is
requested at runtime, because the `ViewportBuilder` hint is ignored by xfwm4.
The window **type** never changes — the root stays `Normal` for life.

**Switching stacking does not recreate the window.** `stacking` is compared
against the last confirmed atom set every panel frame; on a difference the level
is re-sent and the atoms are re-diffed on the same window. `always_on_top` wants
all four atoms; `normal` wants **none**, including clearing `ABOVE`/`STICKY`/
`SKIP_TASKBAR`/`SKIP_PAGER` if an earlier always-on-top session left them on the
window — which is why a normal-mode panel behaves like an ordinary window and is
listed in the taskbar. `SKIP_TASKBAR` is a separate `hide_from_taskbar` flag
(default true) and only applies while always-on-top.

**Wayland** has no `_NET_WM_STATE` a client may send, so the atom work is a
no-op there and position is compositor-owned. The panel still draws at whatever
size the compositor gives it, and `lapsphere --toggle-panel` is the way to reach
it. This is deliberate degradation, not a claim that it works.

## `panel.json`

`~/.config/lapsphere/panel.json`, written by both the menu and the settings
window. `settings.json` and `tray.json` are never touched by any of this.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `version` | number | `1` | Stamped by the normalizer on every write |
| `active` | bool | `false` | Start the process in panel mode |
| `font_scale` | number | `1.0` | Global font scale, clamped to 0.5–4.0 |
| `stacking` | string | `"always_on_top"` | `always_on_top` or `normal` |
| `hide_from_taskbar` | bool | `true` | Skip the taskbar entry, **only** while always-on-top |
| `click_through` | bool | `false` | The panel does not receive the mouse |
| `position` | object | top-left, 24/24 | `corner` (`top_left`…`bottom_right`) plus `offset_x`/`offset_y` |
| `hotkey` | string | `"Shift_R+F9"` | Show/hide key; empty disables it |
| `interactivity_hotkey` | string | `"Shift_R+F8"` | Turns click-through off; empty disables it |
| `items` | array | all 15 | `id`, `visible`, `label`, `font_scale`; array order is display order |

**An unknown `stacking` value becomes `always_on_top`.** The field has its own
deserializer for exactly this reason: a plain enum would fail the parse, and a
failed parse makes the loader fall back to the *whole* default panel, discarding
the user's element order, labels and position. Degrading one field instead of
losing the file is the point. A wrong JSON type (`17`, `true`, `null`) is treated
the same way. Every other key keeps `#[serde(default)]`, so a file written by an
older build loads with the documented defaults above.

## Files

| Path | Contents |
|---|---|
| `gui/src/panel/config.rs` | `panel.json` schema, normalization, tests |
| `gui/src/panel/items.rs` | The element registry: id → a function of `AppState` |
| `gui/src/panel/units.rs` | Every unit, one place |
| `gui/src/panel/render.rs` | Fixed layout, the 60-second graph ring |
| `gui/src/panel/window.rs` | Mode switching, the EWMH retry policy (pure) |
| `gui/src/panel/x11.rs` | The X11 backend and the draw call |
| `gui/src/panel/menu.rs` | The right-click menu, the hide rule and the click-through rule |
| `gui/src/panel/settings_window.rs` | The settings window, a child viewport |
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
13. The right-click menu opens on the panel and its items act. — By measurement.
14. The settings window opens, saves to `panel.json` and survives a reopen. — By
    measurement.
15. `stacking` switches on the fly without recreating the window. — By
    measurement: `xprop`/`wmctrl` read-back after each switch, including 20
    consecutive switches with no RSS growth.
16. In `normal` stacking the panel does not cover other windows and is in the
    taskbar; in `always_on_top` it is above a fullscreen window. — By
    measurement.
17. `interactivity_hotkey` flips click-through, including with NumLock/CapsLock
    on. — By measurement.
18. Click-through cannot be enabled with no way back, and the checkbox says why.
    — By test over every combination, and by measurement of the disabled state.
19. An unknown `stacking` value loads as `always_on_top` with the rest of the
    file intact. — By test.

## Measured on this host

All measurements below are from a **release** build on this machine: X11 with
**xfwm4** (compositor off), `XDG_SESSION_TYPE=x11`, tools `xprop`, `wmctrl`,
`xdotool`.

| # | Measurement | Result |
|---|---|---|
| M1 | Right-click opens the menu | See the run log below |
| M2 | The settings window opens, saves `panel.json`, and survives a reopen | See the run log below |
| M3 | 20 consecutive stacking switches, atoms read back each time, RSS growth | See the run log below |
| M4 | `normal` stacking: panel is listed by `wmctrl -l` and does not cover a raised window | See the run log below |
| M5 | `always_on_top` stacking over a fullscreen window | See the run log below |
| M6 | `interactivity_hotkey` flips click-through with NumLock/CapsLock on | See the run log below |

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
| Behaviour under a compositor with a real cube/desktop | No such session exists here; see the rows above. |
| Wayland stacking and taskbar behaviour | `_NET_WM_STATE` is not a client-settable protocol there, so only the `WindowLevel` half applies. Not measured. |