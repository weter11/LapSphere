# Panel — stage 5: the hotkey, the D-Bus toggle, and the rule that keeps the panel reachable

How the panel is shown and hidden again, what was measured, and what is **not**
claimed. Companion to [`panel-v1-baseline.md`](./panel-v1-baseline.md) (stage
status) and [`panel-measurements.md`](./panel-measurements.md) (the panel's other
numbers).

## The three routes out of a hidden panel

* **A global hotkey** (`Shift_R+F9` by default, `hotkey` in `panel.json`): a
  passive `XGrabKey` on the root window, watched by a dedicated thread. A global
  key is needed because egui only sees keys delivered to this window, and a hidden
  or unfocused panel receives none. Grabbed **and** polled on ONE connection —
  X11 delivers a `KeyPress` to the connection that owns the grab.
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
| the configured key is grabbed, and resolved to the right keycode | **PASS** | `panel: hotkey `Shift_R+F9` captured (keycode 75)`; the server's own keymap agrees: `xmodmap -pk` → `75  0xffc6 (F9)  0xffc6 (F9) …` |
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
* **Lock keys change the delivered mask**, so the grab is made once per lock
  variant (CapsLock / NumLock / ScrollLock), not once for plain Shift.
* **A D-Bus object belongs to the connection that owns the name.** Exported on a
  fresh connection it is invisible through `io.lapsphere.Gui`, and every call
  hangs while the process looks healthy.
* **A handler that only sets a flag must also request a repaint**, and the UI has
  to publish its context: a toggle arriving while the panel is hidden otherwise
  waits for a frame egui is not going to paint.
* **Drain both request flags in one call**, or two requests in the same frame
  toggle twice and land back where they started — which reads as "the hotkey does
  nothing".
