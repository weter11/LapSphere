# Panel — stage 5: the hotkey, the D-Bus toggle, and the rule that keeps the panel reachable

How the panel is shown and hidden again, what was measured, and what is **not**
claimed. Companion to [`panel-v1-baseline.md`](./panel-v1-baseline.md) (stage
status) and [`panel-measurements.md`](./panel-measurements.md) (the panel's other
numbers).

## The three routes out of a hidden panel

* **A global hotkey** (`Shift+F9` by default, `hotkey` in `panel.json`): a
  passive `XGrabKey` on the root window, watched by a dedicated thread. A global
  key is needed because egui only sees keys delivered to this window, and a hidden
  or unfocused panel receives none. Grabbed **and** polled on ONE connection —
  X11 delivers a `KeyPress` to the connection that owns the grab. The grab masks
  are the **configured** modifiers plus every lock combination (`Shift` alone
  grabs nothing but `Shift+…`): a `Control+Alt+…` hotkey grabbed under `Shift` is
  a key that does nothing while the log says "captured". Not every combination is
  always available — another client can own one — so the status distinguishes a
  complete grab from a **partial** one, and the settings surface warns about the
  latter and about any modifier token it had to ignore.
* **`TogglePanel` on `io.lapsphere.Gui`**, which is what `lapsphere
  --toggle-panel` calls. The object is exported on the connection that owns that
  name (parked before `run_native`); on any other connection it would be reachable
  only through that connection's unique name.
* **The tray**, already in the tree.

## The hide-safety rule, and which rows a live run can reach

Hiding is only carried out when a way back exists; with none of the three, the
request becomes "return to the normal window" instead — the user asked for the
panel to stop being a panel, and that is the only reading of it that does not
leave an invisible window nothing can bring back.

| hotkey | tray | D-Bus | outcome | reachable in a live run here? |
|---|---|---|---|---|
| no | no | no | ReturnToNormal | **no** — see below |
| yes | no | no | Hide | yes (session A) |
| no | yes | no | Hide | not run (no StatusNotifier host in either session) |
| no | no | yes | Hide | yes (session C) |
| yes | yes | no | Hide | not run |
| yes | no | yes | Hide | yes (session A: the hotkey hid it, the CLI brought it back) |
| no | yes | yes | Hide | not run |
| yes | yes | yes | Hide | not run |

**The all-three-false row cannot be produced on a running Linux GUI.** The
D-Bus route is recorded when the export succeeds, and the export happens at
startup on the connection the single-instance guard owns; a GUI that cannot own
that name does not get as far as a window. So on this platform `ReturnToNormal`
is defensive code: it is covered by the table-driven unit test above (all eight
rows, mutation-checked), and by the refusal path the menu and the toggle share.

## The acceptance run — branch commit `ac70e5b`

`ac70e5b3ede458f2230feb0d1ab1bfc7be6bceb9` (`gui/src/panel/visibility.rs` and
friends), worktree clean, built with `cargo build -p lapsphere --bin lapsphere`
and tested with `cargo test -p lapsphere --bin lapsphere` — **204 passed, 0
failed**; the same command on the base tree gives **163 passed, 0 failed**, so
this stage adds **41** tests to that binary. The base count was taken in a
checkout whose `gui/` is byte-identical to `ee759ef`'s (`git diff --stat
ee759ef..42d1551 -- gui/` is empty). The binary driven live is
`target/debug/lapsphere`, sha256
`53b817f14527eb8d564b340c1b7e9b346e3581bada29219400d12e066059e587` (rebuilt
once after a `touch gui/src/main.rs` and identical, so the debug build is
reproducible for this tree).

**Session A — a throwaway X11 session, isolated on four axes.** Its own display
(`Xvfb :77 -screen 0 1280x720x24` + `xfwm4 --display=:77`), its own *session* bus
**and its own system bus** (a private bus for `DBUS_SYSTEM_BUS_ADDRESS`, so the
GUI cannot reach the real `lapsphere-daemon`: the run logs
`ServiceUnknown: The name io.lapsphere.Control was not provided by any .service
files` and `startup profile apply is still waiting for CPU frequency limits;
profile not applied`, which is the evidence that no profile was applied to this
host's hardware), and its own `HOME`, so the user's `panel.json`, `settings.json`
and `tray.json` were never touched. Every kill was PID-scoped, after reading the
target's `/proc/<pid>/environ` — never by name.

| Claim | Result | Evidence |
|---|---|---|
| the configured key is grabbed, and resolved to the right keycode | **PASS** | at `ac70e5b` the default spec was spelled `Shift_R+F9`, so that is the string in the run's log — it parses to the same single `ShiftMask` bit as today's `Shift+F9`: `panel: hotkey `Shift_R+F9` captured (keycode 75)`; the server's own keymap agrees: `xmodmap -pk` → `75  0xffc6 (F9)  0xffc6 (F9) …` |
| a press hides the panel and a second press shows it | **PASS** | XTEST (`XTestFakeKeyEvent`) Shift+F9: `Map State: IsViewable` → `IsUnMapped` → `IsViewable`, with `panel: hotkey pressed` and `panel: hidden; the hotkey, the tray or `lapsphere --toggle-panel` brings it back` / `panel: shown again` between them |
| the grab delivers to the connection that owns it | **PASS** | a second client's `XGrabKey(Shift+F9)` on the same server is refused with error code **10** (`Access`) — the app's grab is installed on the server, not merely reported as captured by its own `check()` |
| a hidden panel still hears the key | **PASS** | the panel was hidden (`IsUnMapped`) when the press that produced `panel: shown again` arrived |
| NumLock and CapsLock variants | **PASS** | `XQueryPointer` mask read during the press: `0x11` (Shift\|Mod2, NumLock) and `0x3` (Shift\|Lock, CapsLock); the hotkey toggled under both |
| the object is reachable through the name that owns it | **PASS** | `busctl --address=unix:path=… tree io.lapsphere.Gui` → `/io/lapsphere/Gui/Panel`; `introspect` lists `io.lapsphere.Gui.Panel.TogglePanel` |
| `--toggle-panel` reaches the RUNNING process | **PASS** | visible → `IsUnMapped` in ≤0.1 s and hidden → `IsViewable` in 0.1 s, `LapSphere: panel toggled`, exit 0, with `panel: TogglePanel requested over D-Bus` in that process's log; the process count never changed and a second GUI still printed `Another instance of LapSphere GUI is already running.` |
| no GUI on the bus is reported as such | **PASS** | on a bus with nothing on it: `LapSphere: no running LapSphere GUI owns io.lapsphere.Gui`, exit 1 |
| the key can be turned off | **PASS** | `hotkey: ""` → `panel: no hotkey configured; the tray and `lapsphere --toggle-panel` still work`, and an XTEST Shift+F9 changed nothing (no grab was made). With the tray off as well, the panel still hid and showed over D-Bus |

**Session B — this host's live Wayland desktop** (xfwl4, Xwayland on `:0`; the
run had `DISPLAY` unset and `WAYLAND_DISPLAY`/`XDG_RUNTIME_DIR` from the
session, so the app made a native Wayland surface). Same isolation: private
session bus, scratch `HOME`.

| Claim | Result | Evidence |
|---|---|---|
| the hotkey is reported unavailable, and nothing is claimed for it | **PASS** | `panel: no X11 display, so the global hotkey is unavailable; the tray and `lapsphere --toggle-panel` are the ways back` — `GrabStatus::NoDisplay` |
| the D-Bus path is usable in the current Wayland desktop | **PASS** | the object is exported and `busctl tree io.lapsphere.Gui` on that session's bus lists `/io/lapsphere/Gui/Panel`; `--toggle-panel` twice → exit 0, `panel: hidden; …` then `panel: shown again` |

## The focused re-run — branch commit `408d1b0`

`408d1b01bd0882ab1eb1037a26ca4e8aad3092c5`: the grab masks come from the
configured modifiers and cover every lock combination. Focused GUI tests
(`cargo test -p lapsphere --bin lapsphere`) **208 passed, 0 failed** — the 204 of
`ac70e5b` plus the four added here — and `cargo test --workspace` **322 passed, 0
failed, 6 ignored**. Sessions A and B above are unchanged and were not re-run.

**Session C — the same four-axis isolation** (throwaway `:77`, private session
*and* system bus, scratch `HOME`, every kill PID-scoped after reading
`/proc/<pid>/environ`), with a **non-default** hotkey and a **combined** lock
state.

| Claim | Result | Evidence |
|---|---|---|
| the masks are the configured modifiers, not a fixed set | **PASS** | `Control+Alt+A` → `panel: grabbed keycode 38 under none (mask 0x000c)`, `capslock (0x000e)`, `numlock (0x001c)`, `capslock+numlock (0x001e)`, `scrolllock (0x008c)`, `capslock+scrolllock (0x008e)`, `numlock+scrolllock (0x009c)`, `capslock+numlock+scrolllock (0x009e)`; `panel: hotkey `Control+Alt+A` captured (keycode 38, modifiers 0x000c)`. No mask carries `ShiftMask` |
| those combinations are installed on the server | **PASS** | a second client's `XGrabKey` of `0x000c`, `0x001e` and `0x009e` at keycode 38 is refused with error code **10** (`Access`), while `0x0001` (the mask the defect hardcoded) and `0x0000` are accepted |
| the key itself toggles the panel | **PASS** | XTEST `Control+Alt+A`: `IsViewable` → `IsUnMapped` → `IsViewable`, with `panel: hotkey pressed` and `panel: hidden; …` / `panel: shown again` between them |
| a press that is not the configured combination does nothing | **PASS** | XTEST `A` (state `0x0`) and `Shift+A` (state `0x01`) left the map state `IsViewable`; `Shift` is the mask the old code grabbed under |
| a **combined** lock state is a grab of its own | **PASS** | with CapsLock and NumLock both on: `XQueryPointer` mask `0x12` at rest and `0x1e` during the press (Control, Mod1, Lock and Mod2), and the panel hid and showed on that press |
| the default is `Shift+F9` and either Shift key fires it | **PASS** | fresh `HOME`, no `panel.json`: `panel: hotkey `Shift+F9` captured (keycode 75, modifiers 0x0001)` with the eight masks (`0x0001` … `0x0093`), and the file it writes holds `"hotkey": "Shift+F9"`; XTEST `Shift_L+F9` hid the panel and `Shift_R+F9` showed it, the server having delivered state `0x01` for both |

**This run had no window manager, and that is a measurement, not a shortcut.**
With `xfwm4` running on the throwaway display, `XGrabKey` of `Ctrl+<any key>` and
`Alt+<any key>` is refused to a second client, and it is accepted with the WM
stopped — so under the WM the non-default combination cannot be exercised at all.
A WM is not part of anything checked here (grab, delivery, watcher, map state),
and its absence announces itself: `panel EWMH state: could not apply
_NET_WM_STATE_ABOVE … the window manager did not accept the request`, which is
the expected line on a bare display.

**`Ctrl+Alt+Fn` reaches no client through XTEST, which cost the first attempt at
this run.** With the app holding `XGrabKey(Control|Mod1, F5)` — its grab proven by
the second client's refusal — an injected `Ctrl+Alt+F5` produced nothing, and a
grab of my own on the same combination received nothing either; the same test
against `Control+Alt+A` was delivered immediately. The X server consumes the
VT-switch combination before grab processing, so the live combination is a letter
here. `Ctrl+Alt+Fn` is a poor choice of hotkey on any X server, not just Xvfb.

**One more thing the first attempt showed.** A stale client of mine held the
`none` variant while the app started, so that one grab came back `BadAccess` and
the other seven did not — and the app reported `captured` and then ignored every
press, because `grab_outcome` counts any accepted variant as a usable key. That
is the designed status, so the per-variant `… is already grabbed by another
client` line is now logged: without it, a partial grab is invisible and the run
reads as "the code does not work".

## The partial-grab pass — branch commit `9bcc6db`

`9bcc6dba6868dcd078ca449cbe50ff6381ecdca1`: a grab is per lock combination and
another client can own exactly one of them, so "captured" carried two different
meanings. The outcomes are now four, decided by one rule,
`grab_outcome(grabbed, busy, attempted)`: **Grabbed** when every unique mask was
acquired, **Partial** when some were, **Busy** when none were and a `BadAccess`
conflict is why, **Failed** otherwise. The comparison is against the unique masks
the spec needs, never a constant eight — `CapsLock+F9` asks for a bit the variant
table already adds, so it has four.

**Two questions, two predicates**, because one cannot answer both:

| question | predicate | `Partial` |
|---|---|---|
| must the watcher run, i.e. will some presses arrive? | `keeps_watcher()` | **yes** |
| is the key a guaranteed way back? | `is_reliable_route()` | **no** |

A partial grab keeps its watcher — the key is installed and does fire in the states
that were acquired, so returning early would leave a grab nothing listens to — while
hide-safety counts only the complete grab. Hiding stays allowed whenever the tray or
the D-Bus method exists, and the settings surface says which route it is relying on
rather than the hotkey. The refusal message and the "hidden" log line follow the
same split: "no hotkey" and "a key that covers only some lock states" are different
sentences.

What the settings surface now shows under the status line:

* **Partial** — "Only some lock-key combinations could be captured: another
  application owns the rest, so this key may not work with CapsLock, NumLock or
  ScrollLock on. Which combinations were refused is in the log." The per-mask detail
  stays in the log: `panel: keycode 38 under none (mask 0x000c) is already grabbed by
  another client; the hotkey does not fire in that lock state`.
* **An ignored modifier token** — a spec of `Wibble+F9` is stored as typed and
  grabbed as plain `F9`, and the status line alone would read as an exact match for
  a shortcut that does not exist. The warning names the token and the **effective**
  shortcut. Nothing is rewritten: the supported aliases (`Shift`/`Shift_L`/`Shift_R`,
  `Control`/`Ctrl`, `Alt`/`Mod1`, `Lock`/`CapsLock`, `NumLock`, `ScrollLock`) parse
  as before, so the warning means "this is a typo".

### Session D — a partial grab, driven live

Same isolation as session C (throwaway `:77`, private session and system bus, scratch
`HOME`, no WM, every kill PID-scoped). A second client of mine held **one**
combination — `Control|Mod1` at keycode 38, the plain state — before the app started.

| Claim | Result | Evidence |
|---|---|---|
| a partially captured key is reported as partial, not as captured | **PASS** | `panel: keycode 38 under none (mask 0x000c) is already grabbed by another client; the hotkey does not fire in that lock state`, then `panel: hotkey `Control+Alt+A` captured for some lock states only (keycode 38, modifiers 0x000c); it may not fire with CapsLock, NumLock or ScrollLock on` |
| the watcher still runs | **PASS** | `panel: watching keycode 38 on the connection that holds the grab`, and the press below arrived |
| the missing state does not fire, and goes to the client that owns it | **PASS** | XTEST `Control+Alt+A` (delivered state `0x0c`): the map state stayed `IsViewable`, and my client's log shows `KeyPress delivered: detail=38 state=0x000c` |
| an acquired state does fire | **PASS** | with CapsLock **and** NumLock on (delivered mask `0x1e`): `IsViewable` → `IsUnMapped` → `IsViewable`, with `panel: hotkey pressed` and `panel: shown again` |
| hiding is still allowed because another route exists | **PASS** | that was `Hide`, not `ReturnToNormal` — the window was unmapped, and the object is exported on the run's own bus |
| a second client cannot take either combination | **PASS** | `XGrabKey(0x000c, 38)` refused (my client owns it) and `XGrabKey(0x001e, 38)` refused (the app owns it) |

Tests for this commit: focused GUI bin **223 passed, 0 failed** (208 at `408d1b0`);
`cargo test --workspace` **337 passed, 0 failed, 6 ignored** (322 before). The two
settings warnings are asserted on the painted frame — the text egui actually
produced — with a negative control each: the same spec with a complete grab paints
neither warning, and `Shift_R` / `Shift_L` alias specs paint none either.

## The parser is side-effect free — branch commit `624702d`

`624702d53e0d75c49c7d1dc3aba95fed2c702e80`. `parse_spec` is also the settings
surface's read path: `ignored_modifiers_warning` calls it while rendering, so an open
settings window emitted the ignored-modifier warning and the Shift-alias info line on
every repaint. The parser now only parses — the ignored tokens and the
`Shift_L`/`Shift_R` spelling travel in the value it returns
(`ParsedSpec::ignored`, `ParsedSpec::named_shift_key`) — and the two notes are emitted
by `log_applied_spec`, whose only production caller is `start_hotkey`: the initial grab
and every re-grab after a settings change. The settings warning says exactly what it
said before and has no side effect; the spelling table and every parsing behaviour are
unchanged, and `Hotkey::parse` stays as a test-only wrapper.

| claim | evidence |
|---|---|
| the render path logs nothing | a capturing `log` logger in the test binary: parsing, `Hotkey::parse` and `ignored_modifiers_warning` produce no line about a spec, and twenty render-path calls produce none either |
| the notes still happen where they belong | the positive control: the same two records are visible when `log_applied_spec` runs (one `Warn` naming the dropped token and the effective shortcut, one `Info` about the single `ShiftMask` bit) |
| the tests discriminate | both silence tests fail when one `log::warn!` is put back into `parse_spec`, and pass once it is reverted |
| the app still says it once | live run with `hotkey: "Wibble+Shift_R+F9"` on the throwaway `:77` session: exactly one `ignores unknown modifier(s) \`Wibble\`; the effective shortcut is `Shift+F9`` and one `names one Shift key` line, both at startup, none repeated over the following ten seconds — and the grab came from the effective shortcut, `captured (keycode 75, modifiers 0x0001)` |

Tests for this commit: focused GUI bin **226 passed, 0 failed**; `cargo test
--workspace` **340 passed, 0 failed, 6 ignored**. `rustfmt --check` on
`gui/src/panel/visibility.rs` is clean, and the build reports the same 29 warnings as
at `367a36c` (`Hotkey::parse` is `#[cfg(test)]`, so the move adds none).

## What is NOT claimed

* **No global hotkey on native Wayland.** The grab is an X server grab: a native
  Wayland client never routes its keys through the X server, so the key is
  claimed for X11/Xwayland only, and the status says so in both the log and the
  settings surface.
* **The key works only while an X11/Xwayland window has the keyboard focus.** A
  Wayland-native game will not see the key press go to the X server.
* **Whether the native Wayland surface actually leaves the screen on hide was not
  measured.** Session B shows the state and its log lines; this repo's notes
  record winit's Wayland `set_visible` as a no-op, and no map state exists to read
  there (there is no X window). X11/Xwayland hide is the measured case.
* **A tray route was not exercised**: neither session has a StatusNotifier host,
  so `tray_enabled` counted as false in every run.
* **Another compositor, another window manager, a multi-monitor setup**: not
  tested and not claimed.
* **The `ReturnToNormal` row** (above): unit-test only.

## The traps this stage's fixes encode

Each of these was a real defect in the archived branch's attempt, and each cost a
debugging session when it was hit again:

* **One connection for the grab and the poll.** Grabbing on one X connection and
  polling on another loses every press while the log still says "captured".
* **`XGrabKey` has no reply.** Success and `BadAccess` are indistinguishable until
  the error arrives, so each lock variant is confirmed through its cookie's
  `check()`; without that, a refused combination is reported as captured and the
  safety rule counts a key that does nothing.
* **Function-key keysyms are contiguous in steps of ONE** (`F9` = 0xFFC6), not in
  steps of 32, and the keycode lookup has to cover
  `min_keycode..=max_keycode` — the F-keys live above any leading slice, so a
  short range finds the letters and misses every function key.
* **Lock keys change the delivered mask**, so the grab is made once per combination
  of the lock bits (CapsLock / NumLock / ScrollLock, including two or three at
  once), not once for the hotkey's own modifiers alone. Grabbing only the
  single-lock variants loses the key in a state a user can be in permanently.
* **The masks come from the configured modifiers**, never from a fixed set. The
  server delivers a `KeyPress` only for the mask the grab was made with, so a
  hardcoded `ShiftMask` makes every non-Shift hotkey dead while the capture log
  says it was captured.
* **`Shift`, not `Shift_R`.** `XGrabKey` takes one `ModMask`, both Shift keys set
  the same `ShiftMask` bit and the delivered event carries no distinction, so a
  right-hand-only key cannot be expressed. The shortcut is described as
  `Shift+F9` and either Shift key fires it (`Shift_L+F9` / `Shift_R+F9` stay
  accepted as spellings of the same mask).
* **"Any variant succeeded" is not one status, and the two questions it hides need
  two predicates.** A grab missing one combination *does* fire in the states it
  acquired, so the watcher must run for it; it may not fire in the lock state the
  user happens to be in, so it is not the route to leave the panel hidden behind.
  Answering both with one predicate either kills a working key (returning early on
  anything but a complete grab) or hides the panel behind a key that may not fire.
  The comparison is against the **unique** masks a spec needs: a spec naming a lock
  key as its modifier collapses the table, and measuring against a constant reports
  its complete grab as partial forever.
* **An ignored modifier token needs a warning on the surface, not only a log line.**
  `Wibble+F9` is stored as typed and grabbed as plain `F9`, so the status line reads
  as an exact match for a shortcut that does not exist. Keep the entered string, say
  which token was dropped and what the effective shortcut is.
* **A parser called from a render path must not log.** The settings surface re-parses
  the configured spec on every repaint, so a parser that emits its own warnings turns
  an open window into a stream of identical lines. Return what the parse learned
  (ignored tokens, spelling flags) and let the caller that APPLIES the value do the
  talking — one caller, one note per apply. Assert the silence, and assert the note
  too: a "nothing was logged" test proves nothing unless the same harness can see a
  line that was emitted, and it is only worth having if it fails when the log call
  comes back.
* **A D-Bus object belongs to the connection that owns the name.** Exported on a
  fresh connection it is invisible through `io.lapsphere.Gui`, and every call
  hangs while the process looks healthy.
* **A handler that only sets a flag must also request a repaint**, and the UI has
  to publish its context: a toggle arriving while the panel is hidden otherwise
  waits for a frame egui is not going to paint.
* **Drain both request flags in one call**, or two requests in the same frame
  toggle twice and land back where they started — which reads as "the hotkey does
  nothing".
