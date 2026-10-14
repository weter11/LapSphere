//! The panel's X11 (EWMH) state: stacking and taskbar visibility.
//!
//! # Why this exists
//!
//! `egui`'s `ViewportBuilder::with_taskbar(false)` is a no-op on Linux —
//! `egui-winit` destructures the value to `_taskbar` and discards it — so "keep
//! the panel off the taskbar" has to be asked of the window manager as EWMH
//! state: `_NET_WM_STATE_SKIP_TASKBAR` and `_NET_WM_STATE_SKIP_PAGER` on the
//! panel's own window.
//!
//! # What the measurements say (xfwm4 4.20.0, PR #111)
//!
//! * Writing the property directly is a FALSE POSITIVE: the atoms read back a
//!   couple of milliseconds later, and the window manager strips them within
//!   seconds. The request has to go to the window manager, as a `_NET_WM_STATE`
//!   ClientMessage sent to the root window — which is what this module does.
//! * The same ClientMessage sent to a window the manager has only just mapped is
//!   *ignored*, and is accepted when it is re-sent about a second later. The
//!   message was never wrong; the timing was. So every send is boundedly retried,
//!   with the state read back from the X server between attempts, and nothing is
//!   sent until the window is managed.
//!
//! # Non-blocking
//!
//! The retry loop waits between attempts, so it runs on a worker thread: the UI
//! thread only hands the wanted state over and returns. Each request claims a
//! generation number from a counter shared with its worker, and the worker checks
//! it before every attempt — so a request the user has already replaced can never
//! finish last and win.
//!
//! # The level is in this set, and is also sent by the toolkit
//!
//! `_NET_WM_STATE_ABOVE` is asked for twice, deliberately. `ViewportCommand::
//! WindowLevel` carries it with the rest of the window spec, which is the route
//! that works on xfwm4 (B9); this module then asks for the same atom and reads the
//! state back. The second half is not redundancy for its own sake — it is the race
//! the rest of this module exists for. The spec is sent on the first frame, when
//! the window may not be managed yet, and a request made then is discarded with
//! nothing anywhere reporting it (`ViewportCommand` has no result). The read-back
//! is the only way to know the level landed, and the bounded retry is what makes
//! the panel come up on top anyway.
//!
//! # Not claimed
//!
//! X11 only. A Wayland window has no `_NET_WM_STATE` to write, and an X11 window
//! gets exactly what its window manager chooses to honour: nothing here claims
//! behaviour across window managers, or on a composited session. Where the state
//! cannot be applied it is logged and the window is left as it is.

use std::time::Duration;

/// One `_NET_WM_STATE` atom the panel can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateAtom {
    /// `_NET_WM_STATE_ABOVE` — the panel stacks over other windows.
    Above,
    /// `_NET_WM_STATE_SKIP_TASKBAR` — no button in the taskbar / window list.
    SkipTaskbar,
    /// `_NET_WM_STATE_SKIP_PAGER` — no entry in the workspace pager.
    SkipPager,
}

impl StateAtom {
    /// Every atom, in the order messages are sent in.
    ///
    /// A `const` array rather than a `match` in each caller: it is the single
    /// place the atom order is stated, and [`WantedState::get`]/[`set`] are the
    /// only readers, so the ordering cannot drift away from the transport's.
    pub const ALL: [StateAtom; 3] = [
        StateAtom::Above,
        StateAtom::SkipTaskbar,
        StateAtom::SkipPager,
    ];

    /// The X11 property name, as it appears in `xprop` and in logs.
    pub fn name(self) -> &'static str {
        match self {
            StateAtom::Above => "_NET_WM_STATE_ABOVE",
            StateAtom::SkipTaskbar => "_NET_WM_STATE_SKIP_TASKBAR",
            StateAtom::SkipPager => "_NET_WM_STATE_SKIP_PAGER",
        }
    }
}

/// The set of `_NET_WM_STATE` atoms the window should carry.
///
/// A struct of flags rather than a list, so "wanted" and "read back" are the same
/// type and confirming the state is one equality — which is what makes a
/// half-applied switch impossible to mistake for a complete one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WantedState {
    /// `_NET_WM_STATE_ABOVE`: the panel stacks over other windows.
    pub above: bool,
    /// `_NET_WM_STATE_SKIP_TASKBAR`: no taskbar button.
    pub skip_taskbar: bool,
    /// `_NET_WM_STATE_SKIP_PAGER`: no pager entry.
    pub skip_pager: bool,
}

impl WantedState {
    /// The state an ordinary LapSphere window carries: none of them.
    pub const NONE: WantedState = WantedState {
        above: false,
        skip_taskbar: false,
        skip_pager: false,
    };

    /// Whether one atom is wanted.
    pub fn get(self, atom: StateAtom) -> bool {
        match atom {
            StateAtom::Above => self.above,
            StateAtom::SkipTaskbar => self.skip_taskbar,
            StateAtom::SkipPager => self.skip_pager,
        }
    }

    /// Set one atom's wanted value.
    pub fn set(&mut self, atom: StateAtom, value: bool) {
        match atom {
            StateAtom::Above => self.above = value,
            StateAtom::SkipTaskbar => self.skip_taskbar = value,
            StateAtom::SkipPager => self.skip_pager = value,
        }
    }

    /// The wanted atoms as names, for logs. Never empty: `NONE` has its own
    /// wording, so a log line cannot read as if a state were there.
    pub fn names(self) -> String {
        let names: Vec<&'static str> = StateAtom::ALL
            .iter()
            .copied()
            .filter(|atom| self.get(*atom))
            .map(StateAtom::name)
            .collect();
        if names.is_empty() {
            "none (an ordinary window)".to_string()
        } else {
            names.join(", ")
        }
    }
}

/// One atom to add to or remove from the window's `_NET_WM_STATE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomChange {
    Add(StateAtom),
    Remove(StateAtom),
}

impl AtomChange {
    /// The atom this change is about.
    pub fn atom(self) -> StateAtom {
        match self {
            AtomChange::Add(atom) | AtomChange::Remove(atom) => atom,
        }
    }
}

/// The EWMH state the one window should carry, as a pure function of where the
/// app is and what the user asked for.
///
/// Everything that must be undone when the panel is left is expressed by this
/// function returning the empty set: the panel and the ordinary window are the
/// SAME X11 window, so any atom left behind follows the user out of panel mode
/// and floats over their game.
///
/// In panel mode, `stacking` decides `_NET_WM_STATE_ABOVE` and
/// `hide_from_taskbar` decides both skip atoms.
pub fn wanted_state(
    panel: bool,
    stacking: super::config::Stacking,
    hide_from_taskbar: bool,
) -> WantedState {
    if !panel {
        return WantedState::NONE;
    }
    WantedState {
        above: stacking == super::config::Stacking::AlwaysOnTop,
        skip_taskbar: hide_from_taskbar,
        skip_pager: hide_from_taskbar,
    }
}

/// The changes that turn `actual` into `wanted`, in [`StateAtom::ALL`] order.
///
/// A diff and not a blanket add-or-remove: a live switch both adds and removes
/// (leaving always-on-top for the taskbar atoms, or the reverse), and sending all
/// three atoms every time would be three round trips to say nothing at all on the
/// attempts where the window manager has already done what was asked.
pub fn atom_changes(wanted: WantedState, actual: WantedState) -> Vec<AtomChange> {
    StateAtom::ALL
        .iter()
        .copied()
        .filter_map(|atom| match (wanted.get(atom), actual.get(atom)) {
            (true, false) => Some(AtomChange::Add(atom)),
            (false, true) => Some(AtomChange::Remove(atom)),
            _ => None,
        })
        .collect()
}

/// How many send/read-back rounds a state switch gets before it is reported as
/// not applied.
///
/// The measured failure mode is a request to a just-mapped window being dropped
/// silently, with the same request accepted ~1 s later, so the useful budget is
/// several rounds spaced well past that.
pub const ATOM_ATTEMPTS: u32 = 6;

/// How long a worker waits between rounds. Long enough to cover the ~1 s the
/// window manager needs on xfwm4, with the budget above covering slower runs.
pub const ATOM_RETRY_DELAY: Duration = Duration::from_millis(300);

/// What one read-back says about whether to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomStep {
    /// The X server reported exactly the wanted state.
    Confirmed,
    /// Not yet — send again and read back again.
    Retry,
    /// Not yet, and there is no budget left. Report it as not applied.
    Exhausted,
}

/// Decide what to do after one read-back.
///
/// `observed` is the state read back from the X server, or `None` when the read
/// itself failed (or the window is not managed yet) — "unknown" is never treated
/// as "nothing is set", or a window that cannot be queried would look like a
/// confirmed removal.
pub fn atom_step(
    wanted: WantedState,
    observed: Option<WantedState>,
    attempt: u32,
    attempts: u32,
) -> AtomStep {
    if observed == Some(wanted) {
        return AtomStep::Confirmed;
    }
    if attempt >= attempts {
        return AtomStep::Exhausted;
    }
    AtomStep::Retry
}

/// How a bounded apply loop ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopOutcome {
    /// The window carries the wanted state; `attempts` is how many rounds it took.
    Confirmed { attempts: u32 },
    /// The budget ran out with the state still wrong.
    Exhausted { attempts: u32 },
    /// A newer request claimed the generation number; this one stopped.
    Superseded { attempts: u32 },
}

/// Send-and-confirm, bounded and cancellable.
///
/// The X connection lives behind `apply`: it is called once per round and returns
/// the state read back from the server after whatever it decided to send, or
/// `None` when it could not look (no connection, unreadable window, the window
/// manager has not taken the window yet). Split out from the transport so the
/// stopping rules are testable without a display and without a window manager —
/// including the interleaving that matters, which the tests drive with real
/// threads.
///
/// `superseded` is checked at the TOP of every round, before anything is sent: a
/// worker that only noticed after its loop would be free to finish last and
/// overwrite the newer state.
pub fn retry_loop<P, A, W>(
    wanted: WantedState,
    attempts: u32,
    superseded: P,
    mut apply: A,
    wait: W,
) -> LoopOutcome
where
    P: Fn() -> bool,
    A: FnMut(WantedState) -> Option<WantedState>,
    W: Fn(),
{
    for attempt in 1..=attempts {
        if superseded() {
            return LoopOutcome::Superseded { attempts: attempt };
        }
        let observed = apply(wanted);
        match atom_step(wanted, observed, attempt, attempts) {
            AtomStep::Confirmed => return LoopOutcome::Confirmed { attempts: attempt },
            AtomStep::Exhausted => return LoopOutcome::Exhausted { attempts: attempt },
            AtomStep::Retry => wait(),
        }
    }
    LoopOutcome::Exhausted { attempts }
}

/// Which window this is, as far as the EWMH work is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanelWindow {
    /// Not resolved yet: the frame has not handed over a handle.
    #[default]
    Unknown,
    /// An X11 window id, which `_NET_WM_STATE` can be set on.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    X11(u32),
    /// Not an X11 window — Wayland, or no handle at all. `_NET_WM_STATE` is not a
    /// protocol a client may send to a Wayland compositor, so this is a no-op.
    NotX11,
}

/// The X11 window id behind an eframe frame, or [`PanelWindow::NotX11`] when
/// there is not one.
///
/// The id is stable for the process lifetime, so it is resolved once and cached
/// by the caller. On a Wayland (or XWayland-on-Wayland) window the handle is not
/// an Xlib one and this reports `NotX11` rather than guessing.
#[cfg(target_os = "linux")]
pub fn window_of(frame: &eframe::Frame) -> PanelWindow {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = frame.window_handle() else {
        return PanelWindow::Unknown;
    };
    match handle.as_raw() {
        // A window id is a `c_ulong`; anything that does not fit a `u32` is not a
        // window id, so it is refused rather than truncated into some other
        // window's id.
        RawWindowHandle::Xlib(xlib) => match u32::try_from(xlib.window) {
            Ok(id) => PanelWindow::X11(id),
            Err(_) => PanelWindow::NotX11,
        },
        RawWindowHandle::Xcb(xcb) => PanelWindow::X11(xcb.window.get()),
        _ => PanelWindow::NotX11,
    }
}

/// Non-Linux builds have no `_NET_WM_STATE`, so there is never an X11 window.
#[cfg(not(target_os = "linux"))]
pub fn window_of(_frame: &eframe::Frame) -> PanelWindow {
    PanelWindow::NotX11
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt, EventMask, Window};
    use x11rb::rust_connection::RustConnection;

    /// The EWMH state operation, as `_NET_WM_STATE`'s `data.l[0]`.
    const STATE_REMOVE: u32 = 0;
    const STATE_ADD: u32 = 1;

    /// Hands the wanted state to a worker thread. Never blocks the caller.
    ///
    /// Requests are serialised by a generation counter: `request` claims one and
    /// the worker stops as soon as it is no longer the newest, so a burst of
    /// changes cannot leave an older state applied last.
    #[derive(Default)]
    pub struct AtomsController {
        generation: Arc<AtomicU64>,
    }

    impl AtomsController {
        /// Ask for `wanted` on `window`, and return immediately.
        ///
        /// A window that is not an X11 window is a logged no-op: there is nothing
        /// to ask a Wayland compositor with these atoms.
        pub fn request(&mut self, window: PanelWindow, wanted: WantedState) {
            let id = match window {
                PanelWindow::X11(id) => id,
                PanelWindow::NotX11 => {
                    log::debug!(
                        "panel EWMH state: {} not requested — this is not an X11 window",
                        wanted.names()
                    );
                    return;
                }
                PanelWindow::Unknown => {
                    log::debug!(
                        "panel EWMH state: no window yet, not requesting {}",
                        wanted.names()
                    );
                    return;
                }
            };

            let claim = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
            let current = Arc::clone(&self.generation);
            let spawned = std::thread::Builder::new()
                .name("panel-ewmh".to_string())
                .spawn(move || worker(id, wanted, claim, current));
            if let Err(err) = spawned {
                log::warn!(
                    "panel EWMH state: could not start the worker ({err}); {} not applied",
                    wanted.names()
                );
            }
        }
    }

    /// The atoms this module needs, interned once for the worker's connection.
    struct Atoms {
        /// `_NET_WM_STATE`, the property the messages and the read-back use.
        state: u32,
        /// `WM_STATE`, whose presence means the window manager has the window.
        wm_state: u32,
        above: u32,
        skip_taskbar: u32,
        skip_pager: u32,
    }

    impl Atoms {
        fn intern(conn: &RustConnection) -> Result<Self, String> {
            let intern = |name: &[u8]| -> Result<u32, String> {
                let reply = conn
                    .intern_atom(false, name)
                    .map_err(|err| err.to_string())?
                    .reply()
                    .map_err(|err| err.to_string())?;
                Ok(reply.atom)
            };
            Ok(Self {
                state: intern(b"_NET_WM_STATE")?,
                wm_state: intern(b"WM_STATE")?,
                above: intern(StateAtom::Above.name().as_bytes())?,
                skip_taskbar: intern(StateAtom::SkipTaskbar.name().as_bytes())?,
                skip_pager: intern(StateAtom::SkipPager.name().as_bytes())?,
            })
        }

        /// The interned id of one policy atom.
        ///
        /// The one place the policy's atoms meet their wire names, so a mismatch
        /// between [`StateAtom::ALL`] and this match cannot silently send the
        /// wrong atom: it is asserted by `each_atom_resolves_to_its_own_name`.
        fn id(&self, atom: StateAtom) -> u32 {
            match atom {
                StateAtom::Above => self.above,
                StateAtom::SkipTaskbar => self.skip_taskbar,
                StateAtom::SkipPager => self.skip_pager,
            }
        }
    }

    /// A detached worker: connect, then run the bounded loop.
    fn worker(window: u32, wanted: WantedState, claim: u64, current: Arc<AtomicU64>) {
        // `ConnectError` is not a `std::error::Error` in x11rb 0.13, so it is
        // formatted rather than propagated with `?`.
        let conn = match x11rb::connect(None) {
            Ok((conn, _screen)) => conn,
            Err(err) => {
                log::warn!(
                    "panel EWMH state: no X11 connection ({err}); {} not applied",
                    wanted.names()
                );
                return;
            }
        };
        let atoms = match Atoms::intern(&conn) {
            Ok(atoms) => atoms,
            Err(err) => {
                log::warn!(
                    "panel EWMH state: could not intern the atoms ({err}); {} not applied",
                    wanted.names()
                );
                return;
            }
        };
        let root = conn.setup().roots[0].root;

        let outcome = retry_loop(
            wanted,
            ATOM_ATTEMPTS,
            || current.load(Ordering::SeqCst) != claim,
            |wanted| apply(&conn, root, window, &atoms, wanted),
            || std::thread::sleep(ATOM_RETRY_DELAY),
        );

        match outcome {
            LoopOutcome::Confirmed { attempts } => log::info!(
                "panel EWMH state: {} confirmed on window 0x{window:x} after {attempts} attempt(s)",
                wanted.names()
            ),
            LoopOutcome::Exhausted { attempts } => log::warn!(
                "panel EWMH state: could not apply {} to window 0x{window:x} after {attempts} \
                 attempt(s); the window manager did not accept the request, so the window keeps \
                 the state it has",
                wanted.names()
            ),
            LoopOutcome::Superseded { attempts } => log::debug!(
                "panel EWMH state: request for {} on window 0x{window:x} superseded after \
                 {attempts} attempt(s)",
                wanted.names()
            ),
        }
    }

    /// One round: look, send the difference, look again.
    ///
    /// Returns the state the X server reports after the sends, or `None` when
    /// there is nothing to report on — including the case this loop exists for, a
    /// window the manager has not taken yet, where a request is discarded rather
    /// than queued and so must not be sent at all.
    fn apply(
        conn: &RustConnection,
        root: Window,
        window: Window,
        atoms: &Atoms,
        wanted: WantedState,
    ) -> Option<WantedState> {
        match is_managed(conn, window, atoms.wm_state) {
            Ok(true) => {}
            Ok(false) => {
                log::debug!("panel EWMH state: window 0x{window:x} is not managed yet");
                return None;
            }
            Err(err) => {
                log::debug!("panel EWMH state: could not read WM_STATE ({err})");
                return None;
            }
        }

        let observed = read_state(conn, window, atoms)?;
        for change in atom_changes(wanted, observed) {
            if let Err(err) = send_change(conn, root, window, atoms, change) {
                log::debug!("panel EWMH state: sending {change:?} failed ({err})");
                return None;
            }
        }
        if let Err(err) = conn.flush() {
            log::debug!("panel EWMH state: flush failed ({err})");
            return None;
        }
        read_state(conn, window, atoms)
    }

    /// Has the window manager taken this window?
    ///
    /// `WM_STATE` is set on a window the manager manages and absent on one it has
    /// not mapped yet. This is the guard against the measured failure: a request
    /// sent before then is dropped, and dropping one is invisible from here.
    fn is_managed(conn: &RustConnection, window: Window, wm_state: u32) -> Result<bool, String> {
        let reply = conn
            .get_property(false, window, wm_state, AtomEnum::ANY, 0, 2)
            .map_err(|err| err.to_string())?
            .reply()
            .map_err(|err| err.to_string())?;
        Ok(reply.value_len > 0)
    }

    /// The window's `_NET_WM_STATE`, as the atoms this module knows about.
    ///
    /// An absent property is the empty state, not an error: that is what a window
    /// with nothing set looks like. `None` means the question could not be asked.
    fn read_state(conn: &RustConnection, window: Window, atoms: &Atoms) -> Option<WantedState> {
        let reply = conn
            .get_property(false, window, atoms.state, AtomEnum::ATOM, 0, 1024)
            .ok()?
            .reply()
            .ok()?;

        let mut state = WantedState::NONE;
        for atom in StateAtom::ALL {
            let id = atoms.id(atom);
            // The property arrives as bytes with no alignment guarantee, so the
            // atoms are decoded explicitly rather than by casting the slice.
            let present = reply
                .value
                .chunks_exact(4)
                .any(|bytes| u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) == id);
            state.set(atom, present);
        }
        Some(state)
    }

    /// Send one atom add or remove to the window manager.
    ///
    /// Addressed to the ROOT window, `type = _NET_WM_STATE`, one atom per message
    /// (a ClientMessage carries a single property, and additions and removals of a
    /// switch are usually different sets), with `data.l[3] = 0` as the source
    /// indication — the same layout winit's `set_netwm` sends.
    fn send_change(
        conn: &RustConnection,
        root: Window,
        window: Window,
        atoms: &Atoms,
        change: AtomChange,
    ) -> Result<(), String> {
        let action = match change {
            AtomChange::Add(_) => STATE_ADD,
            AtomChange::Remove(_) => STATE_REMOVE,
        };
        let atom = atoms.id(change.atom());
        let event = ClientMessageEvent::new(32, window, atoms.state, [action, atom, 0, 0, 0]);
        conn.send_event(
            false,
            root,
            EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
            event,
        )
        .map_err(|err| err.to_string())?;
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The atom table is the policy's atoms meeting their wire names: a drift
        /// here sends the WRONG atom (hide-from-taskbar becoming sticky, say) and
        /// the read-back simply never converges, with nothing in the log to say
        /// why. Offline, this asserts the mapping both names and separates them.
        #[test]
        fn each_atom_resolves_to_its_own_name() {
            let names: Vec<&str> = StateAtom::ALL
                .iter()
                .copied()
                .map(StateAtom::name)
                .collect();
            assert_eq!(
                names,
                vec![
                    "_NET_WM_STATE_ABOVE",
                    "_NET_WM_STATE_SKIP_TASKBAR",
                    "_NET_WM_STATE_SKIP_PAGER"
                ]
            );
            let mut unique = names.clone();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(unique.len(), names.len(), "two atoms share a wire name");
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod platform {
    use super::*;

    /// Non-Linux builds cannot set `_NET_WM_STATE`; the request is recorded and
    /// dropped so the call site resolves and the behaviour is stated rather than
    /// silently missing.
    #[derive(Default)]
    pub struct AtomsController;

    impl AtomsController {
        pub fn request(&mut self, _window: PanelWindow, wanted: WantedState) {
            log::debug!(
                "panel EWMH state: {} not requested — EWMH is an X11 protocol",
                wanted.names()
            );
        }
    }
}

pub use platform::AtomsController;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    use super::super::config::Stacking;

    fn all_set() -> WantedState {
        WantedState {
            above: true,
            skip_taskbar: true,
            skip_pager: true,
        }
    }

    // ---- the state a mode and a setting map to ----

    #[test]
    fn the_normal_window_asks_for_nothing_whatever_the_settings_say() {
        // The panel and the ordinary window are the same X11 window, so anything
        // left set follows the user out of panel mode and floats over their game.
        for stacking in Stacking::ALL {
            for hide in [true, false] {
                assert_eq!(
                    wanted_state(false, stacking, hide),
                    WantedState::NONE,
                    "normal mode with {stacking:?}, hide_from_taskbar {hide}"
                );
            }
        }
    }

    #[test]
    fn the_panel_maps_stacking_and_the_taskbar_setting_to_their_own_atoms() {
        let panel = true;
        assert_eq!(
            wanted_state(panel, Stacking::AlwaysOnTop, true),
            all_set(),
            "the default panel: above, and out of the taskbar and pager"
        );
        assert_eq!(
            wanted_state(panel, Stacking::AlwaysOnTop, false),
            WantedState {
                above: true,
                ..WantedState::NONE
            },
            "still an overlay, but it keeps its taskbar button"
        );
        assert_eq!(
            wanted_state(panel, Stacking::Normal, true),
            WantedState {
                skip_taskbar: true,
                skip_pager: true,
                ..WantedState::NONE
            },
            "a normally-stacked panel is still not a taskbar window"
        );
        assert_eq!(
            wanted_state(panel, Stacking::Normal, false),
            WantedState::NONE,
            "and with both settings off it is an ordinary window"
        );
    }

    #[test]
    fn the_stacking_setting_selects_exactly_the_above_atom() {
        // The two settings must not bleed into each other: a change of taskbar
        // visibility can never move the window's level.
        let with_above = wanted_state(true, Stacking::AlwaysOnTop, false);
        let without = wanted_state(true, Stacking::Normal, false);
        assert!(with_above.get(StateAtom::Above) && !without.get(StateAtom::Above));
        assert!(!with_above.get(StateAtom::SkipTaskbar) && !without.get(StateAtom::SkipTaskbar));
    }

    #[test]
    fn a_wanted_state_is_named_for_the_log() {
        assert_eq!(
            wanted_state(true, Stacking::AlwaysOnTop, true).names(),
            "_NET_WM_STATE_ABOVE, _NET_WM_STATE_SKIP_TASKBAR, _NET_WM_STATE_SKIP_PAGER"
        );
        // Never an empty string: a blank "state: " would read as a state that is
        // there but was not named.
        assert_eq!(WantedState::NONE.names(), "none (an ordinary window)");
    }

    // ---- the request is a diff, not a blanket ----

    #[test]
    fn a_switch_both_adds_and_removes() {
        assert_eq!(
            atom_changes(all_set(), WantedState::NONE),
            vec![
                AtomChange::Add(StateAtom::Above),
                AtomChange::Add(StateAtom::SkipTaskbar),
                AtomChange::Add(StateAtom::SkipPager),
            ]
        );
        assert_eq!(
            atom_changes(WantedState::NONE, all_set()),
            vec![
                AtomChange::Remove(StateAtom::Above),
                AtomChange::Remove(StateAtom::SkipTaskbar),
                AtomChange::Remove(StateAtom::SkipPager),
            ]
        );

        // Leaving always-on-top for the taskbar atoms is the case a blanket
        // add-or-remove gets wrong in one direction or the other.
        let to_taskbar_only = wanted_state(true, Stacking::Normal, true);
        assert_eq!(
            atom_changes(to_taskbar_only, all_set()),
            vec![AtomChange::Remove(StateAtom::Above)]
        );
        assert_eq!(
            atom_changes(all_set(), to_taskbar_only),
            vec![AtomChange::Add(StateAtom::Above)]
        );
    }

    #[test]
    fn a_state_that_already_matches_sends_nothing() {
        for state in [WantedState::NONE, all_set()] {
            assert!(atom_changes(state, state).is_empty());
        }
    }

    // ---- the retry / read-back policy ----

    #[test]
    fn only_an_equal_read_back_confirms_the_state() {
        assert_eq!(
            atom_step(all_set(), Some(all_set()), 1, ATOM_ATTEMPTS),
            AtomStep::Confirmed,
            "the same set is the confirmation, wherever it was on the way"
        );
        assert_eq!(
            atom_step(all_set(), None, 1, ATOM_ATTEMPTS),
            AtomStep::Retry
        );
        assert_eq!(
            atom_step(WantedState::NONE, Some(WantedState::NONE), 3, ATOM_ATTEMPTS),
            AtomStep::Confirmed,
            "clearing the state is confirmed the same way as setting it"
        );
    }

    #[test]
    fn a_half_applied_state_is_retried_rather_than_confirmed() {
        // The shape a per-atom check gets wrong: two of three atoms landed, which
        // is not the state that was asked for.
        let half = WantedState {
            above: true,
            skip_taskbar: true,
            ..WantedState::NONE
        };
        assert_eq!(
            atom_step(all_set(), Some(half), 1, ATOM_ATTEMPTS),
            AtomStep::Retry
        );
    }

    #[test]
    fn the_attempt_budget_bounds_the_retries() {
        let wrong = WantedState {
            above: true,
            ..WantedState::NONE
        };
        assert_eq!(
            atom_step(all_set(), Some(wrong), ATOM_ATTEMPTS - 1, ATOM_ATTEMPTS),
            AtomStep::Retry,
            "one attempt short of the budget is still a retry"
        );
        assert_eq!(
            atom_step(all_set(), Some(wrong), ATOM_ATTEMPTS, ATOM_ATTEMPTS),
            AtomStep::Exhausted,
            "the last attempt reports the failure instead of retrying forever"
        );
        assert_eq!(
            atom_step(all_set(), None, 1, 1),
            AtomStep::Exhausted,
            "a single-attempt budget cannot retry at all"
        );
    }

    #[test]
    fn an_unreadable_state_is_not_a_confirmed_removal() {
        // "I could not look" and "nothing is set" are different answers, and only
        // the second one confirms an empty wanted state.
        assert_ne!(
            atom_step(WantedState::NONE, None, 1, ATOM_ATTEMPTS),
            AtomStep::Confirmed
        );
    }

    // ---- the loop that uses the policy ----

    #[test]
    fn the_loop_retries_until_the_read_back_matches_and_says_how_many_rounds() {
        // The measured shape: the first round's read-back is not there yet, the
        // second one is. A fixed delay with no read-back could not tell the two
        // apart, and a one-shot send would report a success it never had.
        let reads = Arc::new(AtomicUsize::new(0));
        let outcome = retry_loop(
            all_set(),
            ATOM_ATTEMPTS,
            || false,
            {
                let reads = Arc::clone(&reads);
                move |wanted| {
                    let round = reads.fetch_add(1, Ordering::SeqCst);
                    (round >= 1).then_some(wanted)
                }
            },
            || {},
        );
        assert_eq!(outcome, LoopOutcome::Confirmed { attempts: 2 });
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn the_loop_stops_on_the_first_round_when_the_state_is_already_there() {
        let reads = Arc::new(AtomicUsize::new(0));
        let outcome = retry_loop(
            all_set(),
            ATOM_ATTEMPTS,
            || false,
            {
                let reads = Arc::clone(&reads);
                move |wanted| {
                    reads.fetch_add(1, Ordering::SeqCst);
                    Some(wanted)
                }
            },
            || {},
        );
        assert_eq!(outcome, LoopOutcome::Confirmed { attempts: 1 });
        assert_eq!(reads.load(Ordering::SeqCst), 1, "no redundant second round");
    }

    #[test]
    fn the_loop_gives_up_at_the_budget_and_reports_that_it_did_not_land() {
        // A window manager that never honours the request: the outcome must say
        // so rather than leaving the caller to assume the state is set.
        let reads = Arc::new(AtomicUsize::new(0));
        let outcome = retry_loop(
            all_set(),
            ATOM_ATTEMPTS,
            || false,
            {
                let reads = Arc::clone(&reads);
                move |_| {
                    reads.fetch_add(1, Ordering::SeqCst);
                    None
                }
            },
            || {},
        );
        assert_eq!(
            outcome,
            LoopOutcome::Exhausted {
                attempts: ATOM_ATTEMPTS
            }
        );
        assert_eq!(
            reads.load(Ordering::SeqCst),
            ATOM_ATTEMPTS as usize,
            "bounded: exactly the budgeted number of rounds, no more"
        );
    }

    #[test]
    fn a_worker_whose_request_was_replaced_stops_before_it_sends_again() {
        // Real threads, because the property under test is the interleaving: the
        // other thread stands in for the UI claiming a newer generation while the
        // worker is between rounds. Arithmetic on the counter would prove nothing.
        let claimed = Arc::new(AtomicU64::new(1));
        let applies = Arc::new(AtomicUsize::new(0));
        let first_round = Arc::new(Barrier::new(2));
        let supersede = Arc::new(Barrier::new(2));

        let worker = {
            let claimed = Arc::clone(&claimed);
            let applies = Arc::clone(&applies);
            let first_round = Arc::clone(&first_round);
            let supersede = Arc::clone(&supersede);
            std::thread::spawn(move || {
                retry_loop(
                    all_set(),
                    ATOM_ATTEMPTS,
                    || claimed.load(Ordering::SeqCst) != 1,
                    move |_| {
                        applies.fetch_add(1, Ordering::SeqCst);
                        // Hold the first round open until the UI thread has
                        // replaced this request, so the replacement really does
                        // arrive mid-flight.
                        first_round.wait();
                        supersede.wait();
                        None
                    },
                    || {},
                )
            })
        };

        first_round.wait();
        claimed.store(2, Ordering::SeqCst); // the UI thread's newer request
        supersede.wait();

        assert_eq!(
            worker.join().expect("the worker must not panic"),
            LoopOutcome::Superseded { attempts: 2 }
        );
        assert_eq!(
            applies.load(Ordering::SeqCst),
            1,
            "the superseded request must not send a second time"
        );
    }

    #[test]
    fn every_atom_is_indexable_by_its_own_flag() {
        // `get`/`set` are the only way the policy's atoms are read and written, so
        // a flag that answers for the wrong atom would make the diff send the
        // wrong message.
        let mut state = WantedState::NONE;
        for (index, atom) in StateAtom::ALL.iter().copied().enumerate() {
            state.set(atom, true);
            let set: Vec<StateAtom> = StateAtom::ALL
                .iter()
                .copied()
                .filter(|other| state.get(*other))
                .collect();
            assert_eq!(
                set.len(),
                index + 1,
                "{atom:?} did not add a flag of its own"
            );
        }
        for atom in StateAtom::ALL {
            state.set(atom, false);
            assert!(!state.get(atom));
        }
    }
}
