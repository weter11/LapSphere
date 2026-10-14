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

## What was measured, and on what

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

* **The opacity slider was not driven live.** On this session the compositor does
  not deliver XTEST-injected pointer motion to the X11 client, so no click can
  reach the panel's surface: with the app logging its own input state,
  `xdotool mousemove` moved the X server's pointer (`xdotool getmouselocation`
  agrees) while the app's view of the pointer stayed at the last position it had
  seen, and a right click never reached the surface's `Response`. The reduced
  opacity was therefore produced by writing `background_opacity: 0.5` to
  `panel.json` and restarting — the same field the slider writes. That the slider
  applies **without** a restart rests on the panel reading the config every frame
  and on the control's own test, not on this measurement.
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
