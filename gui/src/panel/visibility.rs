//! Showing and hiding the panel: the global hotkey, the D-Bus method, and the
//! rule that decides whether hiding is allowed at all.
//!
//! # Three routes to one window
//!
//! The app has one window and the panel has one hidden state, and it is brought
//! back by any of:
//!
//! * the **global hotkey** (`Shift_R+F9` by default) — a passive `XGrabKey` on
//!   the root window, watched by a dedicated thread. A global key is needed
//!   because egui only sees keys delivered to this window, and a hidden or
//!   unfocused panel receives none;
//! * the **`TogglePanel` D-Bus method** on the GUI's own `io.lapsphere.Gui`
//!   name, which is what makes `lapsphere --toggle-panel` reach the process that
//!   is already running instead of starting a second instance;
//! * the **tray**, already in the tree.
//!
//! # The safety rule
//!
//! A hide request is only carried out when a way back exists — the key was
//! grabbed, the tray is enabled, or the D-Bus method is reachable
//! ([`hide_outcome`]). With none of the three, hiding would leave the user
//! looking at an invisible window with nothing that can bring it back, so the
//! request becomes "return to the normal window": the user asked for the panel
//! to stop being a panel, and that is the only safe reading of it.
//!
//! The rule is one pure function and every call site reads it, so the menu, the
//! settings surface and the toggle path cannot disagree about whether hiding is
//! allowed. It reads the grab STATUS, never the configured string: a key the
//! server refused is not a key the user can press.
//!
//! # The X11 parts, and what they do NOT claim
//!
//! * **The grab and the poll are one connection.** X11 delivers a `KeyPress` to
//!   the connection that OWNS the grab, so a watcher that re-grabs on a
//!   connection of its own loses every press — with no error and a log that
//!   still says "captured". The watcher thread therefore connects, grabs and
//!   polls on a single connection.
//! * **A refused grab is never reported as captured.** `XGrabKey` has no reply:
//!   success and `BadAccess` (another client owns the combination) both return
//!   void, and the error arrives afterwards as an event. Each lock variant is
//!   confirmed through its cookie's `check()`, and [`grab_outcome`] turns the
//!   per-variant results into the reported status.
//! * **X11/Xwayland only, and only while an X11 window holds the keyboard
//!   focus.** The grab is an X server grab: a native Wayland client never routes
//!   its keys through the X server, so no global-hotkey support is claimed
//!   there. With no X display at all the status is [`GrabStatus::NoDisplay`], and
//!   the tray and `--toggle-panel` are the routes that remain.
//! * **Function keys resolve across the whole server keycode range.** They live
//!   well above `min-keycode`, so mapping a leading slice of the keyboard finds
//!   the letters and misses every function key; see [`keycode_from_mapping`].

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// X11 modifier bits and keysyms
// ---------------------------------------------------------------------------
//
// The protocol values, written as `u16` rather than as
// `x11rb::protocol::xproto::ModMask`: the mask is parsed, compared and tested on
// every platform, and `x11rb` is a Linux-only dependency of this crate. The
// Linux-gated code converts to `ModMask` where the request is built.

/// `ShiftMask`.
pub const MOD_SHIFT: u16 = 1 << 0;
/// `LockMask` — what CapsLock sets.
pub const MOD_LOCK: u16 = 1 << 1;
/// `ControlMask`.
pub const MOD_CONTROL: u16 = 1 << 2;
/// `Mod1Mask` — `Alt` on the standard keymap.
pub const MOD_M1: u16 = 1 << 3;
/// `Mod2Mask` — what NumLock sets on the standard keymap.
pub const MOD_M2: u16 = 1 << 4;
/// `Mod5Mask` — what ScrollLock sets on the standard keymap.
pub const MOD_M5: u16 = 1 << 7;

/// The lowest keycode an X server reports (`min-keycode` in the setup reply).
pub const MIN_KEYCODE: u8 = 8;

/// `XK_F1`. The function keys are **contiguous in steps of ONE**
/// (`F1` = 0xFFBE, `F2` = 0xFFBF, … `F9` = 0xFFC6), not in the steps of 32 that
/// a plausible-looking "`0xFFBE + 32 * (n - 1)`" produces. Cross-checked against
/// `xmodmap -pke` on this host, where F9 is keycode 75 with keysym 0xffc6.
pub const KEYSYM_F1: u32 = 0xFFBE;

/// The lock-key states the hotkey is grabbed under, in the order they are tried.
///
/// One grab per entry, and that is the point: on X11 a lock key changes the
/// modifier mask the server DELIVERS, so a grab made only under plain Shift
/// misses the combination while a lock is on. Each entry is the hotkey's own
/// modifiers plus that lock's bit.
pub fn lock_variants() -> Vec<(&'static str, u16)> {
    vec![
        ("none", MOD_SHIFT),
        ("capslock", MOD_SHIFT | MOD_LOCK),
        ("numlock", MOD_SHIFT | MOD_M2),
        ("scrolllock", MOD_SHIFT | MOD_M5),
    ]
}

/// What is known about the global hotkey.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrabStatus {
    /// No result yet: the watcher thread has not finished its attempt.
    ///
    /// A real state, not an internal one — the grab is made on a worker thread
    /// so that no frame ever waits on the X server, and this is what the UI shows
    /// until that thread reports.
    Unknown,
    /// Captured: the server accepted the grab for at least one lock variant.
    Grabbed,
    /// The user turned the key off (an empty spec in `panel.json`).
    ///
    /// Distinct from [`GrabStatus::Failed`] because the two need different
    /// sentences: "no key configured" is a choice the tray and the CLI already
    /// cover, while "could not capture" is something to act on.
    Disabled,
    /// The combination is already owned by another client (X11 `BadAccess`).
    Busy,
    /// No X11 display (native Wayland, or no `DISPLAY`).
    NoDisplay,
    /// Anything else: a spec that does not parse, a key the server has no
    /// keycode for, or a grab it refused for a reason other than `BadAccess`.
    Failed,
}

impl GrabStatus {
    /// The line the menu and the settings surface show.
    pub fn message(self) -> &'static str {
        match self {
            GrabStatus::Unknown => "Hotkey not checked yet",
            GrabStatus::Grabbed => "Hotkey captured",
            GrabStatus::Disabled => {
                "No hotkey configured: the tray and `lapsphere --toggle-panel` are the ways back"
            }
            GrabStatus::Busy => "Hotkey is taken by another application",
            GrabStatus::NoDisplay => {
                "No X11 display: the hotkey is unavailable here, use the tray or \
                 `lapsphere --toggle-panel`"
            }
            GrabStatus::Failed => "Could not capture the hotkey",
        }
    }

    /// Is this a key the user can actually press?
    ///
    /// The one place "captured" is defined, so the safety rule can never count a
    /// refused, disabled or not-yet-attempted grab as a way back.
    pub fn is_usable(self) -> bool {
        matches!(self, GrabStatus::Grabbed)
    }
}

/// A parsed hotkey such as `Shift_R+F9`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    /// X11 modifier bits ([`MOD_SHIFT`] and friends).
    pub modifiers: u16,
    /// X11 keysym value.
    pub keysym: u32,
    /// X11 keycode, resolved against the running server at grab time.
    pub keycode: u8,
}

impl Hotkey {
    /// Parse a hotkey string.
    ///
    /// Modifiers are `Shift`/`Shift_L`/`Shift_R`, `Control`/`Ctrl`,
    /// `Alt`/`Mod1`, `Lock`/`CapsLock`, `NumLock`, `ScrollLock`, joined by `+`
    /// with a final key name (`F1`..`F35`, `A`..`Z`, `0`..`9`).
    ///
    /// An empty spec is not an error: it is the user turning the key off, and
    /// the tray and `--toggle-panel` still work. An unknown modifier is ignored
    /// with a warning rather than failing the whole spec, so one typo does not
    /// disable the key.
    pub fn parse(spec: &str) -> Option<Self> {
        let spec = spec.trim();
        if spec.is_empty() {
            return None;
        }

        let parts: Vec<&str> = spec.split('+').map(|part| part.trim()).collect();
        let (key, modifier_parts) = parts.split_last()?;

        let mut modifiers = 0u16;
        for part in modifier_parts {
            modifiers |= match *part {
                "Shift" | "Shift_L" | "Shift_R" => MOD_SHIFT,
                "Lock" | "CapsLock" => MOD_LOCK,
                "Control" | "Ctrl" | "Control_L" | "Control_R" => MOD_CONTROL,
                "Alt" | "Mod1" => MOD_M1,
                "NumLock" => MOD_M2,
                "ScrollLock" => MOD_M5,
                other => {
                    log::warn!("panel: unknown hotkey modifier `{other}` ignored");
                    0
                }
            };
        }

        Some(Self {
            modifiers,
            keysym: keysym_for(key)?,
            // Filled in against the running server; 0 means "not resolved yet".
            keycode: 0,
        })
    }
}

/// The X11 keysym a key name denotes, or `None` when this parser does not know
/// the name.
pub fn keysym_for(key: &str) -> Option<u32> {
    // Function keys: XK_F1..XK_F35, contiguous in steps of ONE.
    if let Some(number) = key.strip_prefix('F').or_else(|| key.strip_prefix('f')) {
        if let Ok(n) = number.parse::<u32>() {
            if (1..=35).contains(&n) {
                return Some(KEYSYM_F1 + (n - 1));
            }
        }
    }

    // Letters and digits are their ASCII code points, which is what their
    // Latin-1 keysyms are.
    if key.len() == 1 {
        let ch = key.chars().next()?;
        if ch.is_ascii_alphabetic() {
            return Some(ch.to_ascii_uppercase() as u32);
        }
        if ch.is_ascii_digit() {
            return Some(ch as u32);
        }
    }

    None
}

/// The keycode carrying `keysym` in a keyboard mapping, or `None`.
///
/// `keysyms` is the flat list the server returns, `keysyms_per_keycode` entries
/// per keycode starting at `min_keycode`. The WHOLE list is searched: the caller
/// asks for `min_keycode`..`max_keycode`, and a keysym that only appears on a
/// high keycode — every function key — is found by that search and missed by any
/// shorter range.
///
/// Split out from the request that fetches the mapping so the arithmetic
/// (flat index → keycode across the range) is testable without a server.
pub fn keycode_from_mapping(
    min_keycode: u8,
    keysyms_per_keycode: u8,
    keysyms: &[u32],
    keysym: u32,
) -> Option<u8> {
    let per_keycode = usize::from(keysyms_per_keycode);
    if per_keycode == 0 {
        return None;
    }
    let index = keysyms.iter().position(|candidate| *candidate == keysym)?;
    u8::try_from(usize::from(min_keycode) + index / per_keycode).ok()
}

/// The reported status for the variants one grab attempt produced.
///
/// One grab per lock variant, and the server refuses each independently: another
/// client may own the NumLock variant while the plain one is free. `grabbed`
/// counts the variants the server accepted and `busy` the ones it refused with
/// `BadAccess`, so:
///
/// * any accepted variant is a usable hotkey ([`GrabStatus::Grabbed`]);
/// * every variant refused as busy is [`GrabStatus::Busy`] — the key is somebody
///   else's;
/// * nothing accepted and nothing busy is [`GrabStatus::Failed`].
///
/// The order matters: a refused variant must never be reported as captured, and
/// a partial success must not be reported as busy.
pub fn grab_outcome(grabbed: usize, busy: usize) -> GrabStatus {
    if grabbed > 0 {
        GrabStatus::Grabbed
    } else if busy > 0 {
        GrabStatus::Busy
    } else {
        GrabStatus::Failed
    }
}

// ---------------------------------------------------------------------------
// The hide-safety rule
// ---------------------------------------------------------------------------

/// May a hide request be carried out?
///
/// True when at least one independent route back exists. `tray_enabled` and
/// `dbus_available` are parameters rather than assumptions because the state
/// where they are false is exactly the state the rule exists for.
pub fn can_hide(hotkey_grabbed: bool, tray_enabled: bool, dbus_available: bool) -> bool {
    hotkey_grabbed || tray_enabled || dbus_available
}

/// What a hide request must actually do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HideOutcome {
    /// Hide the panel: a way back exists.
    Hide,
    /// Refuse to hide and return to the normal window instead. With no route
    /// back, an invisible panel would be unreachable.
    ReturnToNormal,
}

/// The safety rule, as the one function every call site reads.
pub fn hide_outcome(hotkey_grabbed: bool, tray_enabled: bool, dbus_available: bool) -> HideOutcome {
    if can_hide(hotkey_grabbed, tray_enabled, dbus_available) {
        HideOutcome::Hide
    } else {
        HideOutcome::ReturnToNormal
    }
}

/// What one show/hide request should do, given the panel's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleAction {
    /// The panel is hidden: show it again. Checked first, so a request arriving
    /// while the panel is hidden can never hide it a second time.
    ShowAgain,
    /// The panel is not on screen: enter panel mode.
    EnterPanel,
    /// In panel mode, with a way back: hide.
    Hide,
    /// In panel mode, with no way back: go to the normal window instead.
    ReturnToNormal,
}

/// The decision for one request, as a pure function.
///
/// `can_hide` is [`can_hide`]'s answer for the current environment, passed in so
/// the whole table is testable and so no call site can reach for its own
/// definition of "a way back".
pub fn toggle_action(panel_hidden: bool, in_panel_mode: bool, can_hide: bool) -> ToggleAction {
    if panel_hidden {
        ToggleAction::ShowAgain
    } else if !in_panel_mode {
        ToggleAction::EnterPanel
    } else if can_hide {
        ToggleAction::Hide
    } else {
        ToggleAction::ReturnToNormal
    }
}

// ---------------------------------------------------------------------------
// The state the three routes share
// ---------------------------------------------------------------------------

/// Live visibility state, shared between the hotkey watcher, the D-Bus method
/// and the UI.
///
/// The request flags are deliberately two: the watcher sets its own and the
/// D-Bus method sets the other, so neither has to know the other exists — and
/// [`Visibility::take_toggle`] drains BOTH, which is what makes two requests
/// arriving in the same frame one toggle rather than two.
pub struct Visibility {
    /// Set by a non-hotkey source (the D-Bus method).
    requested: AtomicBool,
    /// Set by the hotkey watcher when the key is pressed.
    hotkey_fired: Arc<AtomicBool>,
    /// Stops the CURRENT watcher thread. Replaced on every re-grab: a flag that
    /// is set to stop one thread and then handed to the next one makes the new
    /// watcher exit immediately, which looks exactly like a hotkey that is
    /// captured and does nothing.
    stop: Mutex<Arc<AtomicBool>>,
    /// The UI context, published by the frame loop.
    ///
    /// Needed because a request can arrive while the window is hidden: egui is
    /// not painting then, so a flag set without a repaint request waits for an
    /// unrelated frame that may never come.
    ctx: Arc<Mutex<Option<egui::Context>>>,
    /// What the grab attempt achieved.
    grab_status: AtomicU8,
    /// Whether the D-Bus control object was exported, which is what makes the
    /// CLI a way back. Recorded, never assumed: the export is fallible, and a
    /// rule that counted the CLI as always available would offer a route that
    /// does not exist.
    dbus_available: AtomicBool,
}

impl Default for Visibility {
    fn default() -> Self {
        Self::new()
    }
}

impl Visibility {
    pub fn new() -> Self {
        Self {
            requested: AtomicBool::new(false),
            hotkey_fired: Arc::new(AtomicBool::new(false)),
            stop: Mutex::new(Arc::new(AtomicBool::new(false))),
            ctx: Arc::new(Mutex::new(None)),
            // Nothing has been attempted yet, and `Unknown` is not usable: the
            // safety rule must not count a key that has not been grabbed.
            grab_status: AtomicU8::new(STATUS_UNKNOWN),
            dbus_available: AtomicBool::new(false),
        }
    }

    /// Publish the UI context so a background request can wake the frame loop.
    pub fn set_context(&self, ctx: egui::Context) {
        *self.ctx.lock().unwrap_or_else(|err| err.into_inner()) = Some(ctx);
    }

    /// Ask for a show/hide toggle. Safe from any thread.
    pub fn request_toggle(&self) {
        self.requested.store(true, Ordering::SeqCst);
        self.request_repaint();
    }

    /// Wake the frame loop, if a context has been published yet.
    fn request_repaint(&self) {
        if let Some(ctx) = self
            .ctx
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .as_ref()
        {
            ctx.request_repaint();
        }
    }

    /// Take a pending show/hide request, from either source.
    ///
    /// Both flags are consumed in one call, so a hotkey press and a D-Bus call
    /// landing in the same frame are ONE toggle: the frame loop asks once per
    /// frame, and a flag left set would fire again on the next one.
    pub fn take_toggle(&self) -> bool {
        let requested = self.requested.swap(false, Ordering::SeqCst);
        let hotkey = self.hotkey_fired.swap(false, Ordering::SeqCst);
        requested || hotkey
    }

    /// Record what the grab attempt achieved.
    pub fn set_grab_status(&self, status: GrabStatus) {
        self.grab_status
            .store(byte_from_status(status), Ordering::Relaxed);
    }

    pub fn grab_status(&self) -> GrabStatus {
        status_from_byte(self.grab_status.load(Ordering::Relaxed))
    }

    /// Was the show/hide hotkey captured?
    pub fn hotkey_grabbed(&self) -> bool {
        self.grab_status().is_usable()
    }

    /// Record whether the D-Bus control object was exported.
    pub fn set_dbus_available(&self, available: bool) {
        self.dbus_available.store(available, Ordering::Relaxed);
    }

    pub fn dbus_available(&self) -> bool {
        self.dbus_available.load(Ordering::Relaxed)
    }

    /// Is there a way back to the panel after it is hidden?
    pub fn has_return_path(&self, tray_enabled: bool) -> bool {
        can_hide(self.hotkey_grabbed(), tray_enabled, self.dbus_available())
    }

    /// The flag that stops the current watcher thread.
    pub fn stop_signal(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop.lock().unwrap_or_else(|err| err.into_inner()))
    }

    /// The flag the watcher sets when the hotkey is pressed.
    pub fn trigger_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.hotkey_fired)
    }

    /// The context slot, shared with the watcher thread.
    ///
    /// A clone of the SAME `Arc` the UI publishes into: a fresh mutex would be a
    /// slot nothing ever writes to, and the thread's repaint request would go
    /// nowhere.
    pub fn context_flag(&self) -> Arc<Mutex<Option<egui::Context>>> {
        Arc::clone(&self.ctx)
    }

    /// Ask the current watcher to exit, and release its grab.
    pub fn stop(&self) {
        self.stop_signal().store(true, Ordering::Relaxed);
    }

    /// Ask the current watcher to exit and hand back a fresh stop flag for the
    /// watcher that replaces it.
    ///
    /// The old thread must be told to go first — an X grab belongs to the
    /// connection that made it, so a live old thread keeps the previous key
    /// captured for the life of the process — and the new thread must NOT get the
    /// same flag, or it exits before it polls anything.
    fn hand_over_watcher(&self) -> Arc<AtomicBool> {
        let mut current = self.stop.lock().unwrap_or_else(|err| err.into_inner());
        current.store(true, Ordering::Relaxed);
        let fresh = Arc::new(AtomicBool::new(false));
        *current = Arc::clone(&fresh);
        fresh
    }
}

const STATUS_UNKNOWN: u8 = 0;
const STATUS_GRABBED: u8 = 1;
const STATUS_BUSY: u8 = 2;
const STATUS_NO_DISPLAY: u8 = 3;
const STATUS_FAILED: u8 = 4;
const STATUS_DISABLED: u8 = 5;

fn status_from_byte(byte: u8) -> GrabStatus {
    match byte {
        STATUS_GRABBED => GrabStatus::Grabbed,
        STATUS_BUSY => GrabStatus::Busy,
        STATUS_NO_DISPLAY => GrabStatus::NoDisplay,
        STATUS_FAILED => GrabStatus::Failed,
        STATUS_DISABLED => GrabStatus::Disabled,
        _ => GrabStatus::Unknown,
    }
}

fn byte_from_status(status: GrabStatus) -> u8 {
    match status {
        GrabStatus::Unknown => STATUS_UNKNOWN,
        GrabStatus::Grabbed => STATUS_GRABBED,
        GrabStatus::Busy => STATUS_BUSY,
        GrabStatus::NoDisplay => STATUS_NO_DISPLAY,
        GrabStatus::Failed => STATUS_FAILED,
        GrabStatus::Disabled => STATUS_DISABLED,
    }
}

// ---------------------------------------------------------------------------
// The X11 hotkey: grabbed and polled on ONE connection
// ---------------------------------------------------------------------------

/// The keycode the server maps `keysym` to, or `None` if it maps none.
///
/// The request covers `MIN_KEYCODE..=max_keycode` — the whole range, because
/// function keys live well above the minimum keycode.
#[cfg(target_os = "linux")]
fn resolve_keycode(conn: &x11rb::rust_connection::RustConnection, keysym: u32) -> Option<u8> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::ConnectionExt;

    let min = MIN_KEYCODE;
    let max = conn.setup().max_keycode;
    if max < min {
        return None;
    }

    let mapping = conn
        .get_keyboard_mapping(min, max - min + 1)
        .ok()?
        .reply()
        .ok()?;

    keycode_from_mapping(min, mapping.keysyms_per_keycode, &mapping.keysyms, keysym)
}

/// Grab `keycode` on the root window under every lock variant.
///
/// Each variant's result is confirmed before the next is attempted, and that is
/// not optional: `XGrabKey` is void on the wire, so success and "another client
/// already owns this combination" look identical until the error event arrives.
/// Reporting "captured" for a refused grab would make the safety rule offer a
/// key that does nothing.
#[cfg(target_os = "linux")]
fn grab_hotkey(conn: &x11rb::rust_connection::RustConnection, keycode: u8) -> GrabStatus {
    use x11rb::connection::Connection;
    use x11rb::errors::ReplyError;
    use x11rb::protocol::xproto::{ConnectionExt, GrabMode, ModMask};
    use x11rb::protocol::ErrorKind;

    let Some(screen) = conn.setup().roots.first() else {
        return GrabStatus::Failed;
    };
    let root = screen.root;

    let mut grabbed = 0usize;
    let mut busy = 0usize;

    for (_, bits) in lock_variants() {
        // Signature: (owner_events, grab_window, modifiers, key, pointer_mode,
        // keyboard_mode).
        let Ok(cookie) = conn.grab_key(
            false,
            root,
            ModMask::from(bits),
            keycode,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
        ) else {
            continue;
        };

        // `check()` round-trips, which is the only way to learn whether THIS
        // request was refused: without it the error arrives later, and a
        // `poll_for_event` that finds an empty queue reads a refusal as a
        // success.
        match cookie.check() {
            Ok(()) => grabbed += 1,
            // `ErrorKind::Access` is `BadAccess` on the wire: another client
            // already owns this exact combination.
            Err(ReplyError::X11Error(err)) if err.error_kind == ErrorKind::Access => busy += 1,
            Err(err) => log::warn!(
                "panel: grab of keycode {keycode} refused ({err}); trying the next lock variant"
            ),
        }
    }

    grab_outcome(grabbed, busy)
}

/// Establish the grab and start the watcher thread.
///
/// Nothing is waited for: the connect, the keycode lookup and the grab run on
/// the worker, which records the outcome on `visibility` when it has it. The UI
/// therefore never blocks on the X server, and reads [`GrabStatus::Unknown`]
/// until the answer lands.
#[cfg(target_os = "linux")]
pub fn start_hotkey(spec: &str, visibility: Arc<Visibility>) {
    // Owned copy: the thread outlives this call and logs its own.
    let spec = spec.to_string();

    // An empty spec is the user turning the key off, which is a legal choice and
    // not a failure to report as one.
    if spec.trim().is_empty() {
        visibility.set_grab_status(GrabStatus::Disabled);
        log::info!(
            "panel: no hotkey configured; the tray and `lapsphere --toggle-panel` still work"
        );
        return;
    }

    let Some(hotkey) = Hotkey::parse(&spec) else {
        visibility.set_grab_status(GrabStatus::Failed);
        log::warn!("panel: hotkey `{spec}` does not parse; the tray and the CLI still work");
        return;
    };

    let stop = visibility.stop_signal();
    let trigger = visibility.trigger_flag();
    let ctx_flag = visibility.context_flag();

    std::thread::spawn(move || {
        let Ok((conn, _screen)) = x11rb::rust_connection::RustConnection::connect(None) else {
            // No X server: native Wayland, or no DISPLAY.
            visibility.set_grab_status(GrabStatus::NoDisplay);
            log::warn!(
                "panel: no X11 display, so the global hotkey is unavailable; the tray and \
                 `lapsphere --toggle-panel` are the ways back"
            );
            return;
        };

        let Some(keycode) = resolve_keycode(&conn, hotkey.keysym) else {
            visibility.set_grab_status(GrabStatus::Failed);
            log::warn!("panel: the X server has no key for `{spec}`");
            return;
        };

        // Grab on THIS connection, which is the one that will poll it.
        let status = grab_hotkey(&conn, keycode);
        visibility.set_grab_status(status);
        if !status.is_usable() {
            log::warn!(
                "panel: hotkey `{spec}` not captured (keycode {keycode}): {}",
                status.message()
            );
            return;
        }
        log::info!("panel: hotkey `{spec}` captured (keycode {keycode})");

        watch_hotkey(&conn, keycode, trigger, stop, ctx_flag);
    });
}

/// Stop the current watcher and grab `spec` again.
#[cfg(target_os = "linux")]
pub fn regrab(spec: &str, visibility: Arc<Visibility>) {
    // Old watcher first: its grab belongs to its connection and outlives the
    // settings change otherwise. The new watcher gets a fresh stop flag.
    visibility.hand_over_watcher();
    visibility.set_grab_status(GrabStatus::Unknown);
    start_hotkey(spec, visibility);
}

/// Watch for the grabbed hotkey on the connection that owns the grab.
#[cfg(target_os = "linux")]
fn watch_hotkey(
    conn: &x11rb::rust_connection::RustConnection,
    keycode: u8,
    trigger: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    ctx_flag: Arc<Mutex<Option<egui::Context>>>,
) {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::ConnectionExt;

    log::info!("panel: watching keycode {keycode} on the connection that holds the grab");

    while !stop.load(Ordering::Relaxed) {
        match conn.poll_for_event() {
            Ok(Some(event)) => {
                if let x11rb::protocol::Event::KeyPress(key) = event {
                    // The keycode is what was grabbed; comparing keysyms would
                    // depend on the keyboard group, which is not carried here.
                    if key.detail == keycode {
                        log::info!("panel: hotkey pressed");
                        trigger.store(true, Ordering::SeqCst);
                        // A press while the window is hidden must still be
                        // processed, and egui is not painting then.
                        if let Some(ctx) = ctx_flag
                            .lock()
                            .unwrap_or_else(|err| err.into_inner())
                            .as_ref()
                        {
                            ctx.request_repaint();
                        }
                    }
                }
            }
            // Nothing pending: sleep rather than spin.
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(err) => {
                log::warn!("panel: the hotkey watcher stopped: {err}");
                break;
            }
        }
    }

    // Release every variant this thread grabbed.
    if let Some(screen) = conn.setup().roots.first() {
        for (_, bits) in lock_variants() {
            let _ = conn.ungrab_key(
                keycode,
                screen.root,
                x11rb::protocol::xproto::ModMask::from(bits),
            );
        }
        let _ = conn.flush();
    }
}

// ---------------------------------------------------------------------------
// D-Bus: the object, exported on the connection that owns io.lapsphere.Gui
// ---------------------------------------------------------------------------

/// Path the panel control object is exported at.
pub const PANEL_CONTROL_PATH: &str = "/io/lapsphere/Gui/Panel";
/// Interface name of the panel control object.
pub const PANEL_CONTROL_INTERFACE: &str = "io.lapsphere.Gui.Panel";

/// The D-Bus interface `lapsphere --toggle-panel` calls.
///
/// It lives on the GUI's own `io.lapsphere.Gui` session-bus name — the name the
/// single-instance guard already claims — so no second name is taken and a
/// second GUI still refuses to start. Nothing on the daemon fits: the daemon
/// knows nothing about panel mode, and routing a GUI-local action through the
/// privileged daemon would be the wrong dependency direction.
pub struct PanelControl {
    visibility: Arc<Visibility>,
}

impl PanelControl {
    pub fn new(visibility: Arc<Visibility>) -> Self {
        Self { visibility }
    }
}

#[zbus::interface(name = "io.lapsphere.Gui.Panel")]
impl PanelControl {
    /// Show the panel, or hide it if it is showing.
    fn toggle_panel(&self) {
        log::info!("panel: TogglePanel requested over D-Bus");
        self.visibility.request_toggle();
    }
}

/// Export the control object on the connection that owns `io.lapsphere.Gui`.
///
/// **That connection, not a fresh one.** An object exported on a newly created
/// connection is reachable only through that connection's unique name, so
/// `busctl introspect io.lapsphere.Gui` returns an empty tree, `--toggle-panel`
/// hangs, and the process looks healthy the whole time. The name-owning
/// connection is parked in `crate::bus_connection` before `run_native` for
/// exactly this reason.
///
/// Records whether the export succeeded. A failure is not fatal — the hotkey and
/// the tray still work — and the menu reports the missing route rather than
/// pretending the CLI is available.
pub async fn export_control(visibility: Arc<Visibility>) {
    let Some(conn) = crate::bus_connection::connection() else {
        log::warn!(
            "panel: no session-bus connection owns {}; the D-Bus control method is \
             unavailable, so `lapsphere --toggle-panel` will not reach this process",
            crate::bus_connection::GUI_BUS_NAME
        );
        visibility.set_dbus_available(false);
        return;
    };

    let control = PanelControl::new(Arc::clone(&visibility));
    match conn.object_server().at(PANEL_CONTROL_PATH, control).await {
        Ok(_) => {
            log::info!("panel: D-Bus control object exported at {PANEL_CONTROL_PATH}");
            visibility.set_dbus_available(true);
        }
        Err(err) => {
            log::warn!("panel: could not export the D-Bus control object: {err}");
            visibility.set_dbus_available(false);
        }
    }
}

/// How long the CLI waits for a reply before giving up.
///
/// Bounded on purpose: without a timeout every misconfiguration is an indefinite
/// hang rather than an error the user can act on.
pub const CLI_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// `--toggle-panel`: ask a running GUI to toggle, and report whether one was
/// there.
///
/// Three properties, each of which was missing when this command hung forever: a
/// timeout on the call, a clear message when no GUI owns the name, and no
/// `block_on` inside a running runtime (the caller runs this on its own runtime
/// before the GUI starts).
#[cfg(target_os = "linux")]
pub async fn run_toggle_cli() -> anyhow::Result<()> {
    let conn = zbus::Connection::session().await?;
    let proxy = zbus::Proxy::new(
        &conn,
        crate::bus_connection::GUI_BUS_NAME,
        PANEL_CONTROL_PATH,
        PANEL_CONTROL_INTERFACE,
    )
    .await?;

    match tokio::time::timeout(CLI_TIMEOUT, proxy.call_method("TogglePanel", &())).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(err)) => Err(toggle_cli_failure(&err)),
        Err(_) => anyhow::bail!(
            "the running LapSphere GUI did not answer within {} s — it may be stuck",
            CLI_TIMEOUT.as_secs()
        ),
    }
}

/// What a failed `TogglePanel` call reported, reduced to what a message needs.
enum CallFailure<'a> {
    /// The service or the bus answered with a D-Bus error: its name and text.
    Method { name: &'a str, message: &'a str },
    /// Anything else, as text.
    Other(&'a str),
}

/// The sentence the user reads when `--toggle-panel` fails.
///
/// Three cases with three different fixes, so they must not be merged: a bus with
/// no owner of the name means no GUI is running (start one); an owned name with
/// nothing at the object path means a GUI that predates this build (update it);
/// and anything else is passed through in its own words rather than guessed at.
///
/// `ServiceUnknown` is how the bus answers the first case, and it arrives as an
/// ordinary `MethodError` — matched on the D-Bus error NAME, not on the text,
/// because the text is the bus's wording and differs between implementations.
fn failure_message(failure: CallFailure<'_>) -> String {
    match failure {
        CallFailure::Method { name, .. } if name == "org.freedesktop.DBus.Error.ServiceUnknown" => {
            format!(
                "no running LapSphere GUI owns {}",
                crate::bus_connection::GUI_BUS_NAME
            )
        }
        CallFailure::Method { message, .. } => {
            format!("the running LapSphere GUI has no panel control object ({message})")
        }
        CallFailure::Other(text) => {
            let lowered = text.to_lowercase();
            if lowered.contains("no such name") || lowered.contains("name has no owner") {
                format!(
                    "no running LapSphere GUI owns {}",
                    crate::bus_connection::GUI_BUS_NAME
                )
            } else {
                text.to_string()
            }
        }
    }
}

/// Reduce a zbus error to [`failure_message`]'s input.
#[cfg(target_os = "linux")]
fn toggle_cli_failure(err: &zbus::Error) -> anyhow::Error {
    match err {
        zbus::Error::MethodError(name, _, message) => anyhow::anyhow!(
            "{}",
            failure_message(CallFailure::Method {
                name: name.as_str(),
                message: &message.to_string(),
            })
        ),
        other => anyhow::anyhow!(
            "{}",
            failure_message(CallFailure::Other(&other.to_string()))
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::config::DEFAULT_HOTKEY;

    // ---- parsing ----

    #[test]
    fn the_default_hotkey_parses_to_its_real_keysym() {
        // The shipped default, and the keysym it has to resolve to: 0xFFC6 is
        // XK_F9, which `xmodmap -pke` reports for keycode 75 on this host.
        assert_eq!(DEFAULT_HOTKEY, "Shift_R+F9");
        let hotkey = Hotkey::parse(DEFAULT_HOTKEY).expect("the default must parse");
        assert_eq!(hotkey.modifiers, MOD_SHIFT);
        assert_eq!(hotkey.keysym, 0xFFC6);
    }

    #[test]
    fn an_empty_spec_is_a_disabled_hotkey_not_a_parse_error() {
        // The user turning the key off: the tray and the CLI still work, and the
        // safety rule treats it as "no key". Measured live, this used to be
        // reported as `Failed`, i.e. the UI told the user their key "could not be
        // captured" when they had deliberately turned it off.
        assert_eq!(Hotkey::parse(""), None);
        assert_eq!(Hotkey::parse("   "), None);
        assert!(!GrabStatus::Disabled.is_usable());
        let message = GrabStatus::Disabled.message();
        assert!(message.contains("No hotkey configured"), "got {message}");
        assert!(message.contains("--toggle-panel"), "got {message}");
    }

    #[test]
    fn every_status_round_trips_through_the_shared_byte() {
        // The store is a byte behind an atomic, so a variant missing from either
        // direction reads as a different state: `Disabled` would come back as
        // `Unknown`, and the UI would say "not checked yet" forever.
        let visibility = Visibility::new();
        for status in [
            GrabStatus::Unknown,
            GrabStatus::Grabbed,
            GrabStatus::Disabled,
            GrabStatus::Busy,
            GrabStatus::NoDisplay,
            GrabStatus::Failed,
        ] {
            visibility.set_grab_status(status);
            assert_eq!(visibility.grab_status(), status);
        }
    }

    #[test]
    fn function_keys_are_contiguous_in_steps_of_one() {
        // The arithmetic that was wrong: steps of 32 put F1 at 0xFFBE and F9 at
        // 0xFFDE, a keysym no key carries, so every function-key hotkey failed
        // with "the X server has no key for ...".
        assert_eq!(keysym_for("F1"), Some(0xFFBE));
        assert_eq!(keysym_for("F2"), Some(0xFFBF));
        assert_eq!(keysym_for("F9"), Some(0xFFC6));
        assert_eq!(keysym_for("F10"), Some(0xFFC7));
        assert_eq!(keysym_for("f12"), Some(0xFFC9));
    }

    #[test]
    fn letters_and_digits_map_to_their_ascii_keysyms() {
        assert_eq!(keysym_for("A"), Some(0x41));
        assert_eq!(keysym_for("z"), Some(0x5A));
        assert_eq!(keysym_for("7"), Some(0x37));
    }

    #[test]
    fn an_unknown_key_name_is_refused_rather_than_guessed() {
        assert_eq!(keysym_for("F0"), None);
        assert_eq!(keysym_for("F36"), None);
        assert_eq!(keysym_for("Nope"), None);
        assert_eq!(keysym_for("AB"), None);
        assert_eq!(Hotkey::parse("Shift+Nope"), None);
    }

    #[test]
    fn modifiers_accumulate_and_unknown_ones_are_ignored() {
        // A typo in the config must not disable the key: the modifier is dropped
        // with a warning and the rest of the spec still parses.
        assert_eq!(
            Hotkey::parse("Shift+Control+F9").unwrap().modifiers,
            MOD_SHIFT | MOD_CONTROL
        );
        assert_eq!(Hotkey::parse("Wibble+F9").unwrap().modifiers, 0);
        assert_eq!(Hotkey::parse("F9").unwrap().modifiers, 0);
    }

    // ---- the keycode mapping ----

    /// A keyboard mapping shaped like a real one: `MIN_KEYCODE` upwards, four
    /// keysyms per keycode, letters low and function keys high, which is where
    /// they are.
    fn synthetic_mapping() -> (u8, u8, Vec<u32>) {
        let per_keycode = 4u8;
        let mut keysyms = vec![0u32; per_keycode as usize * 80];
        // Keycode 8 = 'A' unshifted, keycode 9 = 'B'.
        keysyms[0] = keysym_for("A").unwrap();
        keysyms[per_keycode as usize] = keysym_for("B").unwrap();
        // Keycode 75 = F9, keycode 76 = F10 — the layout this host reports.
        keysyms[per_keycode as usize * (75 - MIN_KEYCODE as usize)] = keysym_for("F9").unwrap();
        keysyms[per_keycode as usize * (76 - MIN_KEYCODE as usize)] = keysym_for("F10").unwrap();
        (MIN_KEYCODE, per_keycode, keysyms)
    }

    #[test]
    fn a_function_key_resolves_across_the_whole_keycode_range() {
        // The defect this pins: mapping only the first few keycodes finds the
        // letters and misses every function key, because the F-keys live far
        // above `min-keycode`.
        let (min, per, keysyms) = synthetic_mapping();
        assert_eq!(
            keycode_from_mapping(min, per, &keysyms, keysym_for("F9").unwrap()),
            Some(75)
        );
        assert_eq!(
            keycode_from_mapping(min, per, &keysyms, keysym_for("F10").unwrap()),
            Some(76)
        );
        assert_eq!(
            keycode_from_mapping(min, per, &keysyms, keysym_for("A").unwrap()),
            Some(MIN_KEYCODE)
        );
    }

    #[test]
    fn the_keycode_index_is_per_keysym_slot_not_per_keycode() {
        // `keysyms_per_keycode` entries per keycode. The flat list holds F9 at
        // index 4 * (75 - 8) = 268, so four per keycode gives keycode 75 — and
        // reading the same list as eight per keycode gives 268 / 8 + 8 = 41,
        // which is a keycode no key in this layout carries. The stride is the
        // whole lookup.
        let (min, _, keysyms) = synthetic_mapping();
        assert_eq!(
            keycode_from_mapping(min, 4, &keysyms, keysym_for("F9").unwrap()),
            Some(75),
            "four keysyms per keycode: the layout's real F9"
        );
        assert_eq!(
            keycode_from_mapping(min, 8, &keysyms, keysym_for("F9").unwrap()),
            Some(41),
            "the same list read as eight per keycode lands elsewhere"
        );
    }

    #[test]
    fn a_keysym_the_server_does_not_have_resolves_to_nothing() {
        let (min, per, keysyms) = synthetic_mapping();
        assert_eq!(
            keycode_from_mapping(min, per, &keysyms, keysym_for("F35").unwrap()),
            None
        );
        assert_eq!(
            keycode_from_mapping(min, 0, &keysyms, keysym_for("A").unwrap()),
            None
        );
        assert_eq!(
            keycode_from_mapping(min, per, &[], keysym_for("A").unwrap()),
            None
        );
    }

    // ---- lock variants ----

    #[test]
    fn every_lock_key_variant_is_its_own_grab() {
        let variants = lock_variants();
        let names: Vec<&str> = variants.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, vec!["none", "capslock", "numlock", "scrolllock"]);

        for (name, bits) in &variants {
            assert_eq!(
                bits & MOD_SHIFT,
                MOD_SHIFT,
                "{name} must still require the hotkey's own modifier"
            );
            if *name != "none" {
                assert_ne!(*bits, MOD_SHIFT, "{name} must differ from the plain grab");
            }
        }

        // The three lock bits are distinct, or two variants would be one grab.
        let mut bits: Vec<u16> = variants.iter().map(|(_, bits)| *bits).collect();
        let before = bits.len();
        bits.sort_unstable();
        bits.dedup();
        assert_eq!(bits.len(), before, "lock variants must be distinct grabs");
    }

    #[test]
    fn the_lock_bits_are_the_ones_the_server_sets() {
        // NumLock is Mod2 and ScrollLock is Mod5 on the standard keymap. A wrong
        // bit is a grab under a mask the server never delivers — a key that works
        // only while every lock is off.
        let bits = |wanted: &str| {
            lock_variants()
                .into_iter()
                .find(|(name, _)| *name == wanted)
                .unwrap()
                .1
        };
        assert_eq!(bits("numlock"), MOD_SHIFT | MOD_M2);
        assert_eq!(bits("scrolllock"), MOD_SHIFT | MOD_M5);
        assert_eq!(bits("capslock"), MOD_SHIFT | MOD_LOCK);
    }

    // ---- grab failure states ----

    #[test]
    fn a_refused_grab_is_never_reported_as_captured() {
        // Every variant refused by another client's grab.
        assert_eq!(grab_outcome(0, 4), GrabStatus::Busy);
        assert!(!grab_outcome(0, 4).is_usable());
        assert!(!GrabStatus::Busy.is_usable());

        // Nothing accepted and nothing busy is a failure, not a capture.
        assert_eq!(grab_outcome(0, 0), GrabStatus::Failed);
        assert!(!GrabStatus::Failed.is_usable());

        // One accepted variant is a working key even when others are taken.
        assert_eq!(grab_outcome(1, 3), GrabStatus::Grabbed);
        assert!(grab_outcome(1, 3).is_usable());
        assert_eq!(grab_outcome(4, 0), GrabStatus::Grabbed);
    }

    #[test]
    fn no_x_display_is_a_reported_state_not_a_capture() {
        // Native Wayland, or no DISPLAY: no global hotkey is claimed, and the
        // message names the routes that do work there.
        assert!(!GrabStatus::NoDisplay.is_usable());
        let message = GrabStatus::NoDisplay.message();
        assert!(message.contains("--toggle-panel"), "got {message}");
        assert!(message.contains("tray"), "got {message}");
    }

    #[test]
    fn an_unattempted_grab_is_not_a_way_back() {
        // The state the UI starts in: the watcher thread has not reported yet, so
        // there is no key to count on.
        let visibility = Visibility::new();
        assert_eq!(visibility.grab_status(), GrabStatus::Unknown);
        assert!(!visibility.hotkey_grabbed());
        assert!(!visibility.has_return_path(false));
    }

    #[test]
    fn the_busy_message_says_the_key_belongs_to_someone_else() {
        assert_eq!(
            GrabStatus::Busy.message(),
            "Hotkey is taken by another application"
        );
        assert_eq!(GrabStatus::Grabbed.message(), "Hotkey captured");
        assert_eq!(GrabStatus::Failed.message(), "Could not capture the hotkey");
        assert_eq!(GrabStatus::Unknown.message(), "Hotkey not checked yet");
        assert!(!GrabStatus::Disabled.is_usable());
    }

    // ---- request coalescing ----

    #[test]
    fn a_request_is_delivered_once() {
        let visibility = Visibility::new();
        assert!(!visibility.take_toggle(), "nothing pending initially");

        visibility.request_toggle();
        assert!(visibility.take_toggle());
        assert!(!visibility.take_toggle(), "and only once");
    }

    #[test]
    fn two_sources_in_one_frame_are_one_toggle() {
        // The watcher thread and the D-Bus method set different flags; a frame
        // that consumed them separately would toggle twice and land back where it
        // started, which reads as "the hotkey does nothing".
        let visibility = Visibility::new();
        visibility.request_toggle(); // D-Bus
        visibility.trigger_flag().store(true, Ordering::SeqCst); // watcher

        assert!(visibility.take_toggle(), "the first call sees a request");
        assert!(
            !visibility.trigger_flag().load(Ordering::SeqCst),
            "and it drained the hotkey flag as well, so the press cannot fire again"
        );
        assert!(
            !visibility.take_toggle(),
            "and the next frame has nothing to consume"
        );
    }

    #[test]
    fn a_hotkey_press_alone_is_delivered() {
        let visibility = Visibility::new();
        visibility.trigger_flag().store(true, Ordering::SeqCst);
        assert!(visibility.take_toggle());
        assert!(!visibility.take_toggle());
    }

    #[test]
    fn a_background_request_wakes_the_frame_loop() {
        // Without this the request waits for an unrelated frame, and egui is not
        // painting at all while the panel is hidden.
        let ctx = egui::Context::default();
        let visibility = Visibility::new();
        visibility.set_context(ctx.clone());

        // Two idle frames, so nothing is pending on the context any more.
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |_| {});
        }
        assert!(!ctx.has_requested_repaint(), "idle before the request");

        visibility.request_toggle();
        assert!(
            ctx.has_requested_repaint(),
            "the toggle must request a repaint"
        );
        assert!(visibility.take_toggle());
    }

    #[test]
    fn a_request_before_the_context_is_published_is_still_delivered() {
        // The D-Bus method can be called in the window between the export and the
        // first frame: the flag must survive, and only the repaint is skipped.
        let visibility = Visibility::new();
        visibility.request_toggle();
        assert!(visibility.take_toggle());
    }

    // ---- D-Bus availability ----

    #[test]
    fn the_d_bus_route_is_recorded_not_assumed() {
        let visibility = Visibility::new();
        assert!(
            !visibility.dbus_available(),
            "before the export succeeds there is no such route"
        );

        visibility.set_dbus_available(true);
        assert!(visibility.dbus_available());

        // And a failed export takes it away again.
        visibility.set_dbus_available(false);
        assert!(!visibility.dbus_available());
        assert!(
            !visibility.has_return_path(false),
            "no key, no tray, no D-Bus"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_cli_gives_up_in_bounded_time() {
        assert_eq!(
            CLI_TIMEOUT.as_secs(),
            2,
            "the CLI must not wait indefinitely"
        );
    }

    #[test]
    fn the_cli_addresses_the_gui_s_own_name_and_object() {
        assert_eq!(crate::bus_connection::GUI_BUS_NAME, "io.lapsphere.Gui");
        assert_eq!(PANEL_CONTROL_PATH, "/io/lapsphere/Gui/Panel");
        assert_eq!(PANEL_CONTROL_INTERFACE, "io.lapsphere.Gui.Panel");
    }

    #[test]
    fn a_failed_toggle_says_which_failure_it_was() {
        // Measured live: with no GUI on the bus the bus answers ServiceUnknown,
        // and an earlier version of this reported "the running GUI has no panel
        // control object" — which sends the user to update a GUI that is not
        // running.
        let no_owner = failure_message(CallFailure::Method {
            name: "org.freedesktop.DBus.Error.ServiceUnknown",
            message: "The name io.lapsphere.Gui was not provided by any .service files \
                      from org.freedesktop.DBus",
        });
        assert!(
            no_owner.contains("no running LapSphere GUI owns io.lapsphere.Gui"),
            "got {no_owner}"
        );

        // An owned name whose object is missing is the other case, and it keeps
        // the service's own words.
        let no_object = failure_message(CallFailure::Method {
            name: "org.freedesktop.DBus.Error.UnknownObject",
            message: "No such object path '/io/lapsphere/Gui/Panel'",
        });
        assert!(
            no_object.contains("has no panel control object"),
            "got {no_object}"
        );
        assert!(
            no_object.contains("No such object path"),
            "the service's own words are kept: {no_object}"
        );

        // Anything unrecognised is passed through rather than guessed at.
        assert_eq!(
            failure_message(CallFailure::Other("connection closed")),
            "connection closed"
        );
        assert!(failure_message(CallFailure::Other(
            "org.freedesktop.DBus.Error.NameHasNoOwner: name has no owner"
        ))
        .contains("no running LapSphere GUI owns"));
    }

    // ---- the hide-safety rule ----

    #[test]
    fn every_hide_safety_combination_is_decided_by_one_rule() {
        // One row per combination of (hotkey, tray, D-Bus): the eight cases the
        // rule has to answer, and only one of them hides.
        let cases = [
            (false, false, false, HideOutcome::ReturnToNormal),
            (true, false, false, HideOutcome::Hide),
            (false, true, false, HideOutcome::Hide),
            (false, false, true, HideOutcome::Hide),
            (true, true, false, HideOutcome::Hide),
            (true, false, true, HideOutcome::Hide),
            (false, true, true, HideOutcome::Hide),
            (true, true, true, HideOutcome::Hide),
        ];
        for (hotkey, tray, dbus, expected) in cases {
            assert_eq!(
                hide_outcome(hotkey, tray, dbus),
                expected,
                "hotkey={hotkey} tray={tray} dbus={dbus}"
            );
        }
    }

    #[test]
    fn the_rule_reads_the_grab_status_rather_than_the_configured_key() {
        // A configured key that was NOT captured is not a way back: the user
        // cannot press it. This is the difference between "a spec exists" and
        // "the key works", and the whole safety rule turns on it.
        let visibility = Visibility::new();
        assert!(!visibility.hotkey_grabbed());
        assert!(!visibility.has_return_path(false));

        visibility.set_grab_status(GrabStatus::Busy);
        assert!(!visibility.has_return_path(false), "busy is not captured");

        visibility.set_grab_status(GrabStatus::NoDisplay);
        assert!(
            !visibility.has_return_path(false),
            "no display is not captured"
        );

        visibility.set_grab_status(GrabStatus::Failed);
        assert!(!visibility.has_return_path(false), "failed is not captured");

        visibility.set_grab_status(GrabStatus::Disabled);
        assert!(
            !visibility.has_return_path(false),
            "a deliberately disabled key is not a way back"
        );
    }

    #[test]
    fn one_route_is_enough() {
        let visibility = Visibility::new();
        visibility.set_grab_status(GrabStatus::Grabbed);
        assert!(visibility.has_return_path(false), "the key alone");

        let no_key = Visibility::new();
        assert!(no_key.has_return_path(true), "the tray alone");
        no_key.set_dbus_available(true);
        assert!(no_key.has_return_path(false), "the D-Bus method alone");
    }

    #[test]
    fn a_hidden_panel_is_shown_before_anything_else_is_considered() {
        // A request that arrives while the panel is hidden cannot hide it again,
        // whichever routes exist.
        for can_hide_now in [false, true] {
            assert_eq!(
                toggle_action(true, true, can_hide_now),
                ToggleAction::ShowAgain
            );
            assert_eq!(
                toggle_action(true, false, can_hide_now),
                ToggleAction::ShowAgain
            );
        }
    }

    #[test]
    fn a_request_from_the_normal_window_enters_panel_mode() {
        assert_eq!(toggle_action(false, false, true), ToggleAction::EnterPanel);
        assert_eq!(
            toggle_action(false, false, false),
            ToggleAction::EnterPanel,
            "entering panel mode is not gated: the panel is the visible state, \
             not an unreachable one"
        );
    }

    #[test]
    fn hiding_from_panel_mode_follows_the_safety_rule() {
        assert_eq!(toggle_action(false, true, true), ToggleAction::Hide);
        assert_eq!(
            toggle_action(false, true, false),
            ToggleAction::ReturnToNormal
        );
    }

    #[test]
    fn the_status_and_the_toggle_are_decided_by_the_same_rule() {
        // The two call sites must not disagree: an environment the rule says
        // cannot hide is an environment whose toggle returns to normal mode.
        for hotkey in [false, true] {
            for tray in [false, true] {
                for dbus in [false, true] {
                    let may_hide = can_hide(hotkey, tray, dbus);
                    assert_eq!(
                        toggle_action(false, true, may_hide),
                        if may_hide {
                            ToggleAction::Hide
                        } else {
                            ToggleAction::ReturnToNormal
                        }
                    );
                }
            }
        }
    }
}
