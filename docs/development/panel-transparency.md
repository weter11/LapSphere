# Panel — stage 4: real transparency, measured

What the panel's background opacity actually does, the one session it was measured
in, the numbers, and what is **not** claimed. Companion to
[`panel-measurements.md`](./panel-measurements.md) (the panel's other numbers) and
[`panel-v1-baseline.md`](./panel-v1-baseline.md) (stage status).

## What the alpha path is

* `main.rs` creates the root window with `with_transparent(true)`. Unconditional,
  because winit picks the visual (X11) or the buffer format (Wayland) **while
  creating the window** and neither can be swapped on a live window; the panel is
  entered at runtime.
* `panel::window::clear_color` decides whether the window is see-through at all:
  opaque in normal mode, transparent black in panel mode.
* `panel::window::background_fill` is the panel's own background —
  `visuals.panel_fill` at `background_opacity` — painted as the `CentralPanel`
  frame's fill. A translucent *fill* over an opaque *clear* would still be an
  opaque window, because the composited alpha is `src + dst * (1 - src.a)`.

## The acceptance run — merge commit `565930e`

Stage 4's acceptance was run on the merge commit, not on the branch: `565930e`
(merge of PR #128 into `lapsphere`, 2026-10-14), checked out detached in a clean
worktree (`git status --porcelain` empty), built with
`cargo build -p lapsphere --bin lapsphere` and unit-tested with
`cargo test -p lapsphere --bin lapsphere` — **163 passed, 0 failed**. The binary
measured is `target/debug/lapsphere`, sha256
`81ab06c76f03adac7957efc4a3feaaecc1464e3e14d9e29971ed66ada77f6896`.

* **Session:** the same host and session as the branch measurement — xfwl4
  (Wayland) with Xwayland on `:0`, 2560x1440. The app was started with
  `WAYLAND_DISPLAY` unset so that winit takes the **X11** backend, which is the
  session this page's procedure describes.
* **Backdrop:** one `xterm` painted solid `#00c800` (fg = bg), sized to cover the
  panel's whole area, so every sample point inside the panel has the green client
  behind it. The compositor's offset was re-measured rather than reused: a client
  at X origin `(0,65)` maps to screen `(6,108)`, i.e. the surface sits **6 px
  right and 43 px below** the client's X origin, and the green client's screen
  rect equals its X size (904x524 → 904x524) — X coordinates and screen pixels are
  1:1 on this session.
* **The slider was driven live**, in one process: pointer input injected with
  `xdotool` (XTEST). Details and the numbers are below the table.

| Criterion | Result | Evidence |
|---|---|---|
| the panel's surface carries an alpha channel | **PASS** | `xwininfo -id 0xa00003`: `Depth: 32`, `Visual Class: TrueColor`; `xwd` returns 885x393 at 32 bits per pixel |
| at 100 % the background is opaque | **PASS** | surface alpha `255` for all 347 805 pixels; of 87 271 sampled screen pixels inside the panel **0** showed the green client; screen == surface exactly for 81 005 of them, within ±2 for a further 5 426, and 840 further apart — the values the panel repainted between the surface dump and the screenshot |
| at 0.48 the background lets the window behind through, the text does not fade | **PASS** | surface: fill `(13,14,15)` at alpha `122` (0.48 premultiplied), glyph pixels alpha `255`; screen over the green backdrop `(1,200,1)` reads `(13,92,15)`, giving `B_eff`/backdrop = **0.748**; a glyph at `(220,220,220)` reads `(219,220,219)` on screen, where text sharing the background's opacity would read `(105,183,105)` |
| the slider changes the opacity without a restart, and 100 % makes the panel solid again | **PASS** | one drag (settings window, track x 166 → 58) → the same pid `2475744` repainted its surface at alpha `122` and wrote `"background_opacity": 0.48` to `panel.json` itself; a second drag back to the right end → alpha `255` for all 347 805 pixels, `panel.json` `1.0`, pid unchanged |
| the context menu stays opaque | **PASS** | with the background at alpha `122`, the menu opened by a right click is alpha `255` over 30 163 of 31 832 sampled pixels of its rectangle; the remainder are its rounded corners and border, where the panel shows through |
| the ordinary window gets no transparency | **PASS** | without `--panel`: 1009x1098, `Depth 32`, all 1 107 882 surface pixels alpha `255`; over the green client (120 646 of 123 342 sampled points green behind it) **0** pixels showed it through; screen == surface for 110 980, within ±2 for the other 12 362 |

### Driven by the slider

Four steps, all in the process started at `pid 2475744`, which was checked before
and after every one of them and never changed:

1. right click on the panel (XTEST) → the two-entry context menu, drawn inside the
   panel window;
2. click **Panel settings...** → the settings viewport opens as a second X window
   (638x1133);
3. drag the **Background opacity** handle from x 166 to x 58 of its track → the
   panel's surface is at alpha `122` on the next frame, and `panel.json` on disk
   reads `"background_opacity": 0.48` — written by the app;
4. drag it back to the right end → alpha `255` again, `panel.json` `1.0`.

A **click** on the track is not enough: egui's slider senses `drag()`, so a press
and release without movement only highlights the handle. The value moves on a
press-and-move — which is what the procedure's "drag it down" says.

### The input route — what had to be true

The branch measurement recorded that the slider could not be driven on this
session: XTEST-injected pointer motion moved the X server's pointer
(`xdotool getmouselocation` agreed) but no event reached the client. That is
reproducible, and conditional, and the condition is now named.

With a 904x654 backdrop, `xdotool mousemove 1000 200` produced **0** events in an
`xev` window that selected pointer events, and reported the *root* window as the
pointer's window; an XTEST button-3 click produced none either. With the backdrop
grown to 2500x1300 — an X11 client covering the screen, so the compositor's
pointer is over Xwayland's surface region — the same injection produced
`EnterNotify`, `MotionNotify` and `ButtonPress` (`synthetic NO`) in the same
`xev`, and reported the `xev` window as the pointer's window. The panel then took
the menu click and the slider drag. The compositor's pointer was never moved by
us; nothing here creates a virtual input device — only XTEST and `XSendEvent`.
The compositor's pointer position was not read directly and no mechanism inside
Xwayland is claimed: what is measured is the difference between the two states.

Two further facts about the route, both measured:

* A synthetic **core** event (`XSendEvent`, `xdotool click --window`) *is*
  delivered to the client — `xev` logs `ButtonPress ... synthetic YES` — but the
  app ignores it: winit 0.30.13's X11 backend takes pointer input from XInput2
  only (`event_processor.rs`, the `x11::GenericEvent` branch: `XI_ButtonPress`,
  `XI_ButtonRelease`, `XI_Motion`, `XI_Enter`, `XI_Leave`; its core-event branch
  carries `KeyPress`/`KeyRelease` and no pointer event). A synthetic click is
  therefore not a route into this UI.
* **Keyboard** injection arrives in *both* states: with the window focused through
  `XSetInputFocus`, an XTEST `F1` reached the running app
  (`lapsphere::keyboard_shortcuts: F1 key pressed, toggling help`) even in the
  state where pointer injection did not. The failure is specific to the pointer,
  not to injection as such.

## What was measured, and on what

*This is the branch measurement, taken before the merge. Stage 4's status rests
on the acceptance run above; the numbers here are its own and are not re-measured
by it.*

* **Build:** debug, branch `feat/panel-transparency` (base `7262c5b`), the commit
  that adds this page; `target/debug/lapsphere`, clean tree at the time.
* **Session:** X11 — `DISPLAY=:0`, i.e. **Xwayland** — under the **xfwl4** Wayland
  compositor. 2560x1440, `pixels_per_point` 1.7708, AMD Radeon (renoir) via
  radeonsi. This is a **compositing** session, which the panel's earlier pages did
  not have.
* **Backdrop:** an `xterm` painted solid `#00c800` (fg = bg, so no glyphs) at
  `+60+65`, 724x524, over which the panel was placed.
* **Panel:** `--panel`, the ten default rows, 531x351 px at `(0,293)`, `Depth 32`
  / `Visual Class: TrueColor` from `xwininfo` — an ARGB visual was obtained.
* **The compositor's own decoration.** xfwl4 draws a titlebar and a shadow for X11
  windows itself, and draws the window's surface **6 px right and 43 px below** the
  origin `xwininfo` reports for the client, with its titlebar in the gap. That
  offset was measured, not assumed: the tinted value text (`(100,200,120)`) is at
  window `x 133..205, y 263..280` in the window's own framebuffer and at screen
  `x 139..211, y 599..616`. Every screen coordinate below is the surface
  coordinate plus `(6, 336)`.

## The alpha is real, and it is the configured one

`xwd -id <wid>` of the panel window returns 32-bit pixels including the alpha
channel. Sampled every 3 px over the whole surface:

| `background_opacity` | surface alpha histogram |
|---|---|
| 1.0 | `[(255, 20709)]` — one value, opaque |
| 0.5 | `[(128, 19118)]` plus the text pixels, which rise to 255 |

`128` is `0.5 * 255` rounded: the surface carries exactly the configured opacity,
and it is the *background* that carries it. The 13959 pixels that are not the
panel's fill (every glyph) are alpha **255** at 1.0 and stay 255 at 0.5 — the text
never becomes translucent.

## What the compositor puts on screen

Screen sampled at the surface point, at both opacities, and at the same pixel with
the panel gone (the true backdrop):

| surface px | 100 % | 50 % | backdrop (panel gone) | `B_eff` from the 50 % blend |
|---|---|---|---|---|
| (60,60) | (28,29,32) | (14,90,16) | (1,200,1) | (0,151,0) |
| (194,64) | (28,29,32) | (14,90,16) | (1,200,1) | (0,151,0) |
| (494,164) | (28,29,32) | (14,90,16) | (1,200,1) | (0,151,0) |
| (294,314) | (28,29,32) | (103,104,105) | (239,239,239) | (178,179,178) |
| (60,330) | (29,30,33) | (109,110,111) | (255,255,255) | (190,191,190) |

* **At 100 %** the screen shows the panel's own fill exactly, `(28,29,32)` — the
  window behind it is not visible through it at all. The only green pixels inside
  the panel are the app's own tinted values (`(100,200,120)`), which is why a
  crude "is anything green here" test is not evidence and a sample point was
  chosen that is the fill in a 3x3 neighbourhood.
* **At 50 %** the screen is a blend of the fill and what is behind it. Solving
  `screen = 0.5*fill + 0.5*B_eff` gives a `B_eff` that is the measured backdrop
  scaled by **~0.75** — the same 0.75 over a green backdrop `(1,200,1)` and over a
  white one `(239,239,239)`. That factor is a layer **between** the backdrop and
  the panel, i.e. the compositor's window shadow (which is also visible just
  outside the panel's edge: the backdrop reads `(1,189,1)` 5 px to the right of
  it). It is not the alpha: the window's own surface says 128.
* **Text does not fade.** 152 of the 187 tinted-value pixels are byte-identical on
  screen at 50 % and at 100 % (157 within +-2).

## The ordinary window stays opaque

Same window, started without `--panel`, over the green backdrop:

* `xwd` of it: 1009x1098, `Depth 32`, and **every** one of 44440 sampled pixels
  has alpha 255 — the surface carries no transparency at all.
* Screen comparison with the backdrop behind it and with the backdrop removed:
  81363 sampled pixels identical, 33147 differing by +-1 (compositor rounding).
  No background pixel is tinted toward the backdrop; the only green-dominant
  pixels are the app's own tinted values.

## Not verified, and not claimed

* **The slider was not driven live in this measurement.** It *was* driven live in
  the acceptance run on the merge commit (above), and that run is what stage 4's
  status rests on; here it could not be, because the compositor's pointer was
  outside Xwayland's surfaces and the injection never reached the client (see
  "The input route"). The reduced opacity on this run was therefore produced by
  writing `background_opacity: 0.5` to `panel.json` and restarting — the same
  field the slider writes. That the slider applies **without** a restart is the
  acceptance run's result, not this run's.
* **One compositor.** xfwl4 over Xwayland is the only environment measured.
  Nothing here is claimed for xfwm4+compositor, mutter, kwin, a bare X server, or
  native Wayland.
* **No compositor: the background gets darker, not see-through.** Nothing blends
  there, so the X server shows the window's own pixels — and the panel's
  background is premultiplied, i.e. `panel_fill * opacity`. At 1.0 (the default)
  that is exactly the old colour; below it, the background is darker than the
  configured colour rather than translucent. The settings window says this next to
  the slider.
* **Whole-window opacity is not used.** `_NET_WM_WINDOW_OPACITY` and friends would
  fade the text with the background; the alpha here is painted into the surface
  and the text is drawn opaque on top of it.
* Multi-monitor, Wayland, and the compositing state of the earlier (Xvfb/xfwm4)
  sessions are untouched by this page.

## Manual verification procedure (any compositing session)

1. Start the app with `--panel` and put a window with visible content behind it.
2. Right-click the panel -> **Panel settings...** -> **Background opacity**, and
   drag it down to about half.
3. What is behind the panel must show through the background; the rows' text and
   the context menu must stay crisp and readable.
4. Drag it back to 100 %: the panel must be solid again.
5. Leave panel mode: the ordinary LapSphere window must be fully opaque.

## Reproducing the measurement

```sh
# the window's own surface, with its alpha channel
xwininfo -id <wid> | grep -E 'Depth|Visual'      # Depth 32 = ARGB obtained
xwd -id <wid> -silent -out surface.xwd           # 32-bit pixels, alpha in byte 3
# what the compositor actually shows (needs a Wayland compositor that implements
# the screencopy protocol; grim is one)
grim screen.png
```

Decoding `xwd` by hand: the header is 25 big-endian `u32`s, pixel data starts at
`header_size + ncolors*12 + xoffset`, and the row stride is the `bytes_per_line`
field. The channel order for a 32-bit ZPixmap is B, G, R, A.
