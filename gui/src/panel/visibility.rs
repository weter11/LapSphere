//! Three ways to toggle the panel, and the rule that decides whether hiding is
//! allowed at all.
//!
//! The owner's decision is that there is **no way back to the normal window
//! except an explicit control**, so every route here is about the panel being
//! *hidden and shown again*:
//!
//! * a **global hotkey** on X11 — `XGrabKey` on the root window, watched by a
//!   dedicated thread, because egui only sees keys delivered to this window and
//!   the panel is often not focused;
//! * a **D-Bus method** on the GUI's own `io.lapsphere.Gui` name, which is what
//!   makes `lapsphere --toggle-panel` work and therefore what makes the panel
//!   reachable on Wayland, where no global hotkey is available;
//! * the **tray**, already in the tree.
//!
//! ## The safety rule
//!
//! Hiding is only permitted when a way back exists. If the hotkey could not be
//! grabbed, the tray is off and the D-Bus method is unreachable, then hiding the
//! panel would leave the user looking at an empty overlay with no way to remove
//! it. In that state a hide request is converted into "return to the normal
//! window" instead (`HideOutcome`). That is a deliberate degradation: the user
//! asked for the panel to go away and the only safe reading of that is "stop
//! being a panel".

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

#[cfg(target_os = "linux")]
use x11rb::protocol::xproto::ConnectionExt as _;

/// X11 constants x11rb-protocol does not generate, from the X11 headers:
/// `XK_F1` and the two lock masks used by `LOCK_VARIANTS`.
#[cfg(target_os = "linux")]
mod x11consts {
    /// `XK_F1` = 0xFFBE. The function keys are CONTIGUOUS IN STEPS OF 1
    /// (F1=0xFFBE, F2=0xFFBF, ... F9=0xFFC6), not in steps of 32 — verified
    /// against `xmodmap -pke` on this host, where F9 is keycode 75 / keysym
    /// 0xffc6.
    pub const KEYSYM_F1: u32 = 0xFFBE;
    /// Lowest keycode a server may report.
    pub const MIN_KEYCODE: u8 = 8;
    /// `Mod2Mask`, which NumLock sets on the standard keymap.
    pub const MOD_MASK_NUM_LOCK: u8 = 1 << 2;
    /// `Mod5Mask`, which ScrollLock sets on the standard keymap.
    pub const MOD_MASK_SCROLL_LOCK: u8 = 1 << 4;
}

use super::config::DEFAULT_HOTKEY;

/// The modifier mask a hotkey is grabbed under, as X11 `ModMask` bits.
///
/// The server's own type rather than a homegrown bit scheme, because the mask is
/// compared against what the server delivers.
pub type Mask = x11rb::protocol::xproto::ModMask;

/// Lock-key states the hotkey is grabbed under, in the order they are tried.
///
/// The reason there is a list: on X11 a NumLock/CapsLock/ScrollLock state
/// changes the modifier mask the server delivers, so a grab made only under plain
/// Shift can miss the combination while a lock is on. Each entry is a separate
/// grab with that lock's bit set.
/// Build a `ModMask` from its `u16` payload.
///
/// `ModMask` in x11rb 0.13 is a newtype over `u16` with a private field, no
/// bitwise operators, and a non-const `From`, so combined masks are assembled at
/// runtime from the payloads rather than spelled as constants.
fn mask(bits: u16) -> Mask {
    Mask::from(bits)
}

/// Combine two masks.
fn combine(a: Mask, b: Mask) -> Mask {
    mask(u16::from(a) | u16::from(b))
}

/// Lock-key states the hotkey is grabbed under, in the order they are tried.
///
/// The reason there is a list: on X11 a NumLock/CapsLock/ScrollLock state changes
/// the modifier mask the server delivers, so a grab made only under plain Shift
/// can miss the combination while a lock is on. Each entry is a separate grab
/// with that lock's bit set.
///
/// Built at runtime because the combined masks cannot be constants.
pub fn lock_variants() -> Vec<(String, Mask)> {
    vec![
        ("none".to_string(), Mask::SHIFT),
        ("capslock".to_string(), combine(Mask::SHIFT, Mask::LOCK)),
        ("numlock".to_string(), combine(Mask::SHIFT, Mask::M2)),
        ("scrolllock".to_string(), combine(Mask::SHIFT, Mask::M5)),
    ]
}

/// Why a grab failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrabStatus {
    /// Grabbed, with the index of the variant in `LOCK_VARIANTS`.
    Grabbed(usize),
    /// The key is already taken by another client (X11 `BadAccess`).
    Busy,
    /// No X11 display available (Wayland, or no `DISPLAY`).
    NoDisplay,
    /// Anything else.
    Failed,
}

impl GrabStatus {
    /// The message the menu shows.
    pub fn message(self) -> &'static str {
        match self {
            GrabStatus::Grabbed(_) => "Hotkey captured",
            GrabStatus::Busy => "Hotkey is taken by another application",
            GrabStatus::NoDisplay => "No X11 display: use the tray or `lapsphere --toggle-panel`",
            GrabStatus::Failed => "Could not capture the hotkey",
        }
    }

    pub fn is_usable(self) -> bool {
        matches!(self, GrabStatus::Grabbed(_))
    }
}

/// A parsed hotkey string such as `Shift_R+F9`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    pub modifiers: Mask,
    /// X11 keysym value.
    pub keysym: u32,
    /// X11 keycode, resolved at grab time.
    pub keycode: u8,
}

impl Hotkey {
    /// Parse a hotkey string.
    ///
    /// Accepts `Shift_R`, `Shift_L`, `Shift`, `Control`, `Alt`/`Mod1`, `Super`/`Mod4`
    /// joined by `+` with a final key name (`F1`..`F24`, `A`..`Z`, `0`..`9`). An
    /// empty string means "no hotkey" and is not an error — the user can turn the
    /// hotkey off and rely on the tray or the CLI.
    pub fn parse(spec: &str) -> Option<Self> {
        let spec = spec.trim();
        if spec.is_empty() {
            return None;
        }

        let parts: Vec<&str> = spec.split('+').map(|p| p.trim()).collect();
        let (key, modifiers) = parts.split_last()?;

        let mut modifiers_bits = mask(0);
        for part in &parts[..parts.len() - 1] {
            modifiers_bits = combine(
                modifiers_bits,
                match *part {
                    "Shift" | "Shift_L" | "Shift_R" => Mask::SHIFT,
                    "Lock" | "CapsLock" => Mask::LOCK,
                    "Control" | "Ctrl" | "Control_L" | "Control_R" => Mask::CONTROL,
                    "Alt" | "Mod1" => Mask::M1,
                    "NumLock" => Mask::M2,
                    "ScrollLock" => Mask::M5,
                    other => {
                        log::warn!("panel: unknown hotkey modifier `{other}`");
                        mask(0)
                    }
                },
            );
        }

        let keysym = keysym_for(key)?;
        Some(Self {
            modifiers: modifiers_bits,
            keysym,
            keycode: 0,
        })
    }

    /// The X11 keycode for this hotkey, or `None` if the server has no such key.
    ///
    /// The whole `MIN_KEYCODE..=max_keycode` range is mapped, not a slice of it:
    /// F-keys live well above the minimum keycode, so mapping only the first few
    /// finds letters and misses every function key.
    #[cfg(target_os = "linux")]
    pub fn resolve_keycode(&self, conn: &x11rb::rust_connection::RustConnection) -> Option<u8> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::ConnectionExt;

        let min = x11consts::MIN_KEYCODE;
        let max = conn.setup().max_keycode;
        if max < min {
            return None;
        }
        let count = max - min + 1;

        let mapping = conn.get_keyboard_mapping(min, count).ok()?.reply().ok()?;
        let per_keycode = usize::from(mapping.keysyms_per_keycode);
        if per_keycode == 0 {
            return None;
        }

        // The keysym list is flat: `per_keycode` entries per keycode, starting at
        // `min`.
        mapping
            .keysyms
            .iter()
            .position(|sym| *sym == self.keysym)
            .map(|index| min + (index / per_keycode) as u8)
    }
}

/// X11 keysym for a key name.
fn keysym_for(key: &str) -> Option<u32> {
    // Function keys: XK_F1 .. XK_F35 are contiguous in steps of ONE.
    if let Some(number) = key.strip_prefix('F').or_else(|| key.strip_prefix('f')) {
        if let Ok(n) = number.parse::<u32>() {
            if (1..=35).contains(&n) {
                return Some(x11consts::KEYSYM_F1 + (n - 1));
            }
        }
    }

    // Letters map to their ASCII code points.
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

/// Grab the hotkey on the root window for every lock-key variant.
///
/// `XGrabKey` is void on the wire: success and "another client already owns this
/// combination" both return no reply, and the difference arrives asynchronously
/// as a `BadAccess` error event. So each variant is grabbed and the connection is
/// polled for an error before the next one, which is the only way to tell
/// "captured" from "already in use" — and "already in use" is exactly what the
/// menu has to report to the user when another application owns the key.
///
/// All four variants are grabbed together rather than trying them in turn: on X11
/// a lock key changes the modifier mask the server delivers, so a working hotkey
/// has to be captured under each of those masks. The returned status reflects the
/// best outcome across them — grabbed if any variant succeeded, busy if all of
/// them were refused.
#[cfg(target_os = "linux")]
pub fn grab_hotkey(conn: &x11rb::rust_connection::RustConnection, keycode: u8) -> GrabStatus {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, GrabMode};

    let Some(screen) = conn.setup().roots.first() else {
        return GrabStatus::Failed;
    };
    let root = screen.root;

    let mut any_grabbed = false;
    let mut any_busy = false;

    for (_, extra) in lock_variants() {
        let modifiers = extra;
        // Signature: (owner_events, window, keycode, modifiers, pointer_mode,
        // keyboard_mode). The lock variant is part of the modifier mask, which
        // is why one grab per variant is needed.
        let result = conn.grab_key(
            false,
            root,
            modifiers,
            keycode,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
        );
        if result.is_err() {
            continue;
        }
        let _ = conn.flush();

        // BadAccess (error code 1) means this exact combination is owned by
        // another client.
        let mut busy = false;
        while let Ok(Some(event)) = conn.poll_for_event() {
            if let x11rb::protocol::Event::Error(error) = event {
                if error.error_code == 1 {
                    busy = true;
                }
                break;
            }
        }

        if busy {
            any_busy = true;
        } else {
            any_grabbed = true;
        }
    }

    if any_grabbed {
        GrabStatus::Grabbed(0)
    } else if any_busy {
        GrabStatus::Busy
    } else {
        GrabStatus::Failed
    }
}

/// Resolve the hotkey's keycode and grab it, reporting the status.
///
/// Returns `NoDisplay` when there is no X connection at all — the Wayland case,
/// where the hotkey is simply unavailable and the tray or the CLI is the way back.
#[cfg(target_os = "linux")]
pub fn grab_from_spec(
    conn: &x11rb::rust_connection::RustConnection,
    spec: &str,
) -> (Option<Hotkey>, GrabStatus) {
    // Owned copy: the watcher thread outlives this function and logs it.
    let spec = spec.to_string();

    let Some(hotkey) = Hotkey::parse(&spec) else {
        return (None, GrabStatus::Failed);
    };
    let Some(keycode) = hotkey.resolve_keycode(conn) else {
        log::warn!("panel: the X server has no key for `{spec}`");
        return (None, GrabStatus::Failed);
    };
    let status = grab_hotkey(conn, keycode);
    (Some(Hotkey { keycode, ..hotkey }), status)
}

/// Which of the two hotkeys a grab is for.
///
/// The panel has two independent global keys, and they are modelled as two slots
/// rather than one parameterised watcher because they have different jobs, fail
/// independently — either can be taken by another application while the other is
/// free — and feed different flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeySlot {
    /// The show/hide key (`hotkey` in `panel.json`).
    ShowHide,
    /// The interactivity key (`interactivity_hotkey`), which turns click-through
    /// off.
    Interactivity,
}

impl HotkeySlot {
    /// The human name used in log lines.
    fn name(self) -> &'static str {
        match self {
            HotkeySlot::ShowHide => "show/hide",
            HotkeySlot::Interactivity => "interactivity",
        }
    }
}

/// Establish the grab and start the watcher thread.
///
/// **One connection does both.** An earlier version grabbed on one connection
/// and then had the watcher thread re-grab on a second one. X11 delivers a
/// `KeyPress` to the client connection that owns the grab, and with two
/// connections of the same client competing the event went to the one nothing
/// was polling: the log reported "captured" and every press was silently lost,
/// which XTEST confirmed (geometry unchanged across repeated presses, no
/// mode-change line).
///
/// So the watcher owns its connection outright: it connects, grabs all four lock
/// variants, and polls — and the grab status is reported back through a channel
/// rather than assumed.
#[cfg(target_os = "linux")]
pub fn start_hotkey(spec: &str, visibility: Arc<Visibility>) -> Option<Hotkey> {
    start_hotkey_for(spec, visibility, HotkeySlot::ShowHide)
}

/// The interactivity hotkey's grab, with its own status and its own thread.
///
/// Separate from [`start_hotkey`] because it is the only way back out of
/// click-through when no D-Bus CLI is available, so its status is what the
/// click-through safety rule reads. An empty spec is not an error: it means the
/// user turned the key off, which the rule treats as "no key available".
#[cfg(target_os = "linux")]
pub fn start_interactivity_hotkey(spec: &str, visibility: Arc<Visibility>) -> Option<Hotkey> {
    start_hotkey_for(spec, visibility, HotkeySlot::Interactivity)
}

/// Stop the watcher for `slot` and grab `spec` again.
///
/// The old thread must be stopped first: an X grab belongs to the connection
/// that made it, and a live old thread would keep the previous key captured for
/// the life of the process even after the user changed the setting.
#[cfg(target_os = "linux")]
pub fn regrab(spec: &str, visibility: Arc<Visibility>, slot: HotkeySlot) {
    visibility.stop_slot(slot);
    visibility.set_slot_status(slot, GrabStatus::Failed);
    start_hotkey_for(spec, visibility, slot);
}

#[cfg(target_os = "linux")]
fn start_hotkey_for(spec: &str, visibility: Arc<Visibility>, slot: HotkeySlot) -> Option<Hotkey> {
    // Two owned copies: the watcher thread outlives this function and logs its
    // own, while the code below reports with the other. `spec` itself is moved
    // into the thread.
    let spec = spec.to_string();
    let spec_for_thread = spec.clone();

    let Some(hotkey) = Hotkey::parse(&spec) else {
        visibility.set_slot_status(slot, GrabStatus::Failed);
        log::warn!("panel: {} hotkey `{spec}` does not parse", slot.name());
        return None;
    };

    let (status_tx, status_rx) = std::sync::mpsc::channel::<(Hotkey, GrabStatus)>();

    // The watcher thread owns the connection, the grab and the poll loop.
    // A handle for the error branch, so `visibility` is not moved into the
    // closure and is still available below.
    let visibility_for_thread = Arc::clone(&visibility);

    std::thread::spawn(move || {
        let Ok((conn, _screen)) = x11rb::rust_connection::RustConnection::connect(None) else {
            let _ = status_tx.send((hotkey, GrabStatus::NoDisplay));
            return;
        };

        let Some(keycode) = hotkey.resolve_keycode(&conn) else {
            log::warn!("panel: the X server has no key for `{spec_for_thread}`");
            let _ = status_tx.send((hotkey, GrabStatus::Failed));
            return;
        };
        let hotkey = Hotkey { keycode, ..hotkey };

        // Grab on THIS connection, which is the one that will poll it.
        let status = grab_hotkey(&conn, keycode);
        let _ = status_tx.send((hotkey, status));
        if !status.is_usable() {
            return;
        }

        let trigger = visibility_for_thread.trigger_flag_for(slot);
        let stop = visibility_for_thread.stop_watcher_for(slot);
        let ctx_flag = visibility_for_thread.context_flag();
        watch_hotkey(&conn, hotkey, keycode, trigger, stop, ctx_flag);
    });

    // Report the grab result the caller asked about.
    match status_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok((hotkey, status)) => {
            visibility.set_slot_status(slot, status);
            if status.is_usable() {
                log::info!("panel: {} hotkey `{spec}` captured", slot.name());
                Some(hotkey)
            } else {
                log::warn!(
                    "panel: {} hotkey `{spec}` not captured: {}",
                    slot.name(),
                    status.message()
                );
                None
            }
        }
        Err(err) => {
            visibility.set_slot_status(slot, GrabStatus::Failed);
            log::warn!(
                "panel: the {} hotkey thread did not report a status: {err}",
                slot.name()
            );
            None
        }
    }
}

/// Watch for the grabbed hotkey on the connection that owns the grab.
///
/// Exits when `stop` is set, which is what happens when the app quits.
#[cfg(target_os = "linux")]
fn watch_hotkey(
    conn: &x11rb::rust_connection::RustConnection,
    hotkey: Hotkey,
    keycode: u8,
    trigger: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    ctx_flag: Arc<std::sync::Mutex<Option<egui::Context>>>,
) {
    use x11rb::connection::Connection;

    log::info!("panel: watching hotkey, keycode {keycode} on the connection that holds the grab");

    while !stop.load(Ordering::Relaxed) {
        match conn.poll_for_event() {
            Ok(Some(event)) => {
                if let x11rb::protocol::Event::KeyPress(key) = event {
                    // Compare the keycode: that is what was grabbed, and the
                    // keysym is only meaningful once the keyboard group is
                    // resolved, which is not guaranteed here.
                    if key.detail == keycode {
                        log::info!("panel: hotkey pressed");
                        trigger.store(true, Ordering::SeqCst);
                        // A press while the window is hidden must still be
                        // processed, so wake the UI rather than waiting for the
                        // next unrelated frame.
                        if let Some(ctx) =
                            ctx_flag.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
                        {
                            ctx.request_repaint();
                        }
                    }
                }
            }
            Ok(None) => {
                // No events pending: sleep rather than spin.
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(err) => {
                log::warn!("panel: hotkey watcher stopped: {err}");
                break;
            }
        }
    }

    // Release every variant this thread grabbed.
    if let Some(screen) = conn.setup().roots.first() {
        for (_, extra) in lock_variants() {
            let _ = conn.ungrab_key(keycode, screen.root, extra);
        }
        let _ = conn.flush();
    }
    let _ = hotkey;
}

/// Live visibility state, shared between the hotkey threads, the D-Bus methods
/// and the UI.
///
/// Two hotkey slots (show/hide and interactivity) and two pending-request flags.
/// The interactivity request is a *separate* flag from the show/hide one on
/// purpose: they mean opposite things (hide the panel vs. give the panel the
/// mouse back) and must not be consumed by the same call.
pub struct Visibility {
    /// Set by the show/hide hotkey thread or the D-Bus method; consumed by `ui()`.
    toggle_requested: AtomicBool,
    /// Set by the interactivity hotkey thread or `--toggle-interactive`.
    interactivity_requested: AtomicBool,
    /// The UI context, published once the first frame has run.
    ///
    /// Needed because a toggle can arrive while the window is hidden or
    /// minimized: egui does not repaint when nothing asks it to, so without
    /// this the command would sit unprocessed until some unrelated repaint
    /// happened to occur.
    ctx: Arc<std::sync::Mutex<Option<egui::Context>>>,
    /// Set by the show/hide hotkey thread. Owned here so the watcher and the UI
    /// share the same flag — the thread only ever gets a clone of this Arc.
    hotkey_fired: Arc<AtomicBool>,
    /// Set by the interactivity hotkey thread.
    interactivity_fired: Arc<AtomicBool>,
    /// Whether each hotkey was actually grabbed.
    grab_status: AtomicU8,
    interactivity_grab_status: AtomicU8,
    /// Set to tell a watcher thread to exit.
    stop: Arc<AtomicBool>,
    /// The same, for the interactivity watcher.
    interactivity_stop: Arc<AtomicBool>,
    /// Whether the D-Bus control object was exported, which is what makes the
    /// CLI a way back.
    dbus_available: AtomicBool,
}

impl Default for Visibility {
    fn default() -> Self {
        Self::new()
    }
}

const STATUS_UNKNOWN: u8 = 0;
const STATUS_GRABBED: u8 = 1;
const STATUS_BUSY: u8 = 2;
const STATUS_NO_DISPLAY: u8 = 3;
const STATUS_FAILED: u8 = 4;

/// Decode the stored status byte.
fn status_from_byte(byte: u8) -> GrabStatus {
    match byte {
        STATUS_GRABBED => GrabStatus::Grabbed(0),
        STATUS_BUSY => GrabStatus::Busy,
        STATUS_NO_DISPLAY => GrabStatus::NoDisplay,
        _ => GrabStatus::Failed,
    }
}

fn byte_from_status(status: GrabStatus) -> u8 {
    match status {
        GrabStatus::Grabbed(_) => STATUS_GRABBED,
        GrabStatus::Busy => STATUS_BUSY,
        GrabStatus::NoDisplay => STATUS_NO_DISPLAY,
        GrabStatus::Failed => STATUS_FAILED,
    }
}

impl Visibility {
    pub fn new() -> Self {
        Self {
            toggle_requested: AtomicBool::new(false),
            interactivity_requested: AtomicBool::new(false),
            hotkey_fired: Arc::new(AtomicBool::new(false)),
            interactivity_fired: Arc::new(AtomicBool::new(false)),
            ctx: Arc::new(std::sync::Mutex::new(None)),
            grab_status: AtomicU8::new(STATUS_UNKNOWN),
            interactivity_grab_status: AtomicU8::new(STATUS_UNKNOWN),
            stop: Arc::new(AtomicBool::new(false)),
            interactivity_stop: Arc::new(AtomicBool::new(false)),
            dbus_available: AtomicBool::new(false),
        }
    }

    /// Publish the UI context so a background toggle can wake the UI.
    pub fn set_context(&self, ctx: egui::Context) {
        *self.ctx.lock().unwrap_or_else(|e| e.into_inner()) = Some(ctx);
    }

    /// Ask for a show/hide toggle. Safe from any thread.
    ///
    /// Requests a repaint as well: when the window is hidden or minimized egui is
    /// not painting, and the command would otherwise wait for an unrelated frame
    /// that may never come.
    pub fn request_toggle(&self) {
        self.toggle_requested.store(true, Ordering::SeqCst);
        self.request_repaint();
    }

    /// Ask for a click-through toggle. Safe from any thread.
    ///
    /// The same repaint request as [`Visibility::request_toggle`], and for the
    /// same reason: with `MousePassthrough` set the panel gets no mouse input, so
    /// nothing else could ever wake the UI to react.
    pub fn request_interactivity_toggle(&self) {
        self.interactivity_requested.store(true, Ordering::SeqCst);
        self.request_repaint();
    }

    fn request_repaint(&self) {
        if let Some(ctx) = self.ctx.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            ctx.request_repaint();
        }
    }

    /// Take a pending show/hide request, from either source.
    ///
    /// Both flags are consumed, so a hotkey press and a D-Bus call that land in
    /// the same frame produce one toggle rather than two.
    pub fn take_toggle(&self) -> bool {
        let requested = self.toggle_requested.swap(false, Ordering::SeqCst);
        let hotkey = self.hotkey_fired.swap(false, Ordering::SeqCst);
        requested || hotkey
    }

    /// Take a pending interactivity request, from either source.
    pub fn take_interactivity_toggle(&self) -> bool {
        let requested = self.interactivity_requested.swap(false, Ordering::SeqCst);
        let hotkey = self.interactivity_fired.swap(false, Ordering::SeqCst);
        requested || hotkey
    }

    pub fn set_grab_status(&self, status: GrabStatus) {
        self.grab_status
            .store(byte_from_status(status), Ordering::Relaxed);
    }

    /// Record the interactivity hotkey's grab status.
    pub fn set_interactivity_grab_status(&self, status: GrabStatus) {
        self.interactivity_grab_status
            .store(byte_from_status(status), Ordering::Relaxed);
    }

    /// Record the status for one of the two slots.
    #[cfg(target_os = "linux")]
    pub fn set_slot_status(&self, slot: HotkeySlot, status: GrabStatus) {
        match slot {
            HotkeySlot::ShowHide => self.set_grab_status(status),
            HotkeySlot::Interactivity => self.set_interactivity_grab_status(status),
        }
    }

    pub fn grab_status(&self) -> GrabStatus {
        status_from_byte(self.grab_status.load(Ordering::Relaxed))
    }

    /// The interactivity hotkey's grab status.
    pub fn interactivity_grab_status(&self) -> GrabStatus {
        status_from_byte(self.interactivity_grab_status.load(Ordering::Relaxed))
    }

    /// Was the show/hide hotkey captured?
    pub fn hotkey_grabbed(&self) -> bool {
        self.grab_status().is_usable()
    }

    /// Was the interactivity hotkey captured?
    ///
    /// This is the value the click-through safety rule reads: with it true, a
    /// click-through panel can always be given the mouse back.
    pub fn interactivity_grabbed(&self) -> bool {
        self.interactivity_grab_status().is_usable()
    }

    /// Was the D-Bus control object exported?
    ///
    /// Recorded rather than assumed: the export is fallible, and a rule that
    /// counted the CLI as always available would offer click-through on a build
    /// where the CLI could not reach the panel.
    pub fn set_dbus_available(&self, available: bool) {
        self.dbus_available.store(available, Ordering::Relaxed);
    }

    pub fn dbus_available(&self) -> bool {
        self.dbus_available.load(Ordering::Relaxed)
    }

    /// Is there any way back to the panel after it is hidden?
    ///
    /// `dbus_available` is a parameter rather than an assumption: the D-Bus
    /// method is a return path in normal operation, but the rule has to be
    /// testable in the case where it is not, which is the case the rule exists
    /// for.
    pub fn has_return_path(&self, tray_enabled: bool, dbus_available: bool) -> bool {
        super::menu::can_hide(self.hotkey_grabbed(), tray_enabled, dbus_available)
    }

    /// Can click-through be switched on right now?
    ///
    /// The rule, in one place so the menu and the settings window cannot
    /// disagree: the key must be captured or the CLI must be reachable.
    pub fn can_enable_click_through(&self) -> bool {
        super::menu::can_enable_click_through(self.interactivity_grabbed(), self.dbus_available())
    }

    /// Tell the watcher thread to exit.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        self.interactivity_stop.store(true, Ordering::Relaxed);
    }

    /// Tell the watcher for one slot to exit.
    #[cfg(target_os = "linux")]
    pub fn stop_slot(&self, slot: HotkeySlot) {
        match slot {
            HotkeySlot::ShowHide => self.stop.store(true, Ordering::Relaxed),
            HotkeySlot::Interactivity => self.interactivity_stop.store(true, Ordering::Relaxed),
        }
    }

    /// The flag that stops the show/hide watcher thread.
    pub fn stop_watcher(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }

    /// The flag that stops one slot's watcher thread.
    #[cfg(target_os = "linux")]
    pub fn stop_watcher_for(&self, slot: HotkeySlot) -> Arc<AtomicBool> {
        match slot {
            HotkeySlot::ShowHide => Arc::clone(&self.stop),
            HotkeySlot::Interactivity => Arc::clone(&self.interactivity_stop),
        }
    }

    /// The flag the show/hide watcher thread sets when the hotkey fires.
    pub fn trigger_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.hotkey_fired)
    }

    /// The trigger flag for one slot.
    #[cfg(target_os = "linux")]
    pub fn trigger_flag_for(&self, slot: HotkeySlot) -> Arc<AtomicBool> {
        match slot {
            HotkeySlot::ShowHide => Arc::clone(&self.hotkey_fired),
            HotkeySlot::Interactivity => Arc::clone(&self.interactivity_fired),
        }
    }

    /// The context slot, shared with the watcher thread.
    ///
    /// A clone of the SAME Arc the UI publishes into: handing the thread a fresh
    /// mutex would give it a slot nothing ever writes to.
    pub fn context_flag(&self) -> Arc<std::sync::Mutex<Option<egui::Context>>> {
        Arc::clone(&self.ctx)
    }
}

/// What a hide request should do, given the available return paths.
pub fn hide_outcome(
    hotkey_grabbed: bool,
    tray_enabled: bool,
    dbus_available: bool,
) -> super::menu::HideOutcome {
    super::menu::hide_outcome(hotkey_grabbed, tray_enabled, dbus_available)
}

/// The default hotkey, for the config and the menu.
pub fn default_hotkey() -> &'static str {
    DEFAULT_HOTKEY
}

/// The D-Bus interface the GUI exports for `lapsphere --toggle-panel`.
///
/// Lives on the GUI's own `io.lapsphere.Gui` session-bus name — the name the
/// single-instance guard already claims, so no second name is taken and a second
/// instance still refuses to start. There was no suitable method on the daemon:
/// the daemon does not know about panel mode, and routing a GUI-local action
/// through the privileged daemon would be the wrong dependency direction.
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
    /// Show or hide the panel.
    fn toggle_panel(&self) {
        log::info!("panel: TogglePanel requested over D-Bus");
        self.visibility.request_toggle();
    }

    /// Is the panel's hotkey currently captured?
    fn hotkey_status(&self) -> String {
        self.visibility.grab_status().message().to_string()
    }

    /// Turn click-through on or off.
    ///
    /// What `lapsphere --toggle-interactive` calls. It exists because click-through
    /// is the one panel state the panel itself cannot undo: with
    /// `MousePassthrough` set the window takes no mouse input, so neither the
    /// context menu nor the settings window can be reached to switch it off.
    fn toggle_interactive(&self) {
        log::info!("panel: ToggleInteractive requested over D-Bus");
        self.visibility.request_interactivity_toggle();
    }

    /// Is the interactivity hotkey currently captured?
    fn interactivity_hotkey_status(&self) -> String {
        self.visibility
            .interactivity_grab_status()
            .message()
            .to_string()
    }
}

/// Does this error mean "the name is not owned"?
///
/// Matched on the message rather than a variant, because zbus 5 has no
/// `NameError`-style variant that covers every "no such name" path and the
/// point is only the message the user sees.
fn is_no_such_name(err: &zbus::Error) -> bool {
    let text = err.to_string().to_lowercase();
    text.contains("no such name") || text.contains("name has no owner")
}

/// How long the CLI waits for a reply before giving up.
pub const CLI_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Handle `--toggle-panel`: ask a running GUI to toggle, and report whether one
/// was there.
///
/// Three properties this must have, each of which was missing when the command
/// hung indefinitely:
///
/// * **A timeout on the call.** Without one, any misconfiguration is an
///   indefinite hang rather than an error.
/// * **No GUI is a clear message with a non-zero exit**, not silence.
/// * **It does not block inside a running tokio runtime.** `block_on` inside a
///   runtime context panics; the caller runs this on its own runtime before the
///   GUI starts.
#[cfg(target_os = "linux")]
pub async fn run_toggle_cli() -> anyhow::Result<bool> {
    call_panel_method("TogglePanel").await
}

/// Handle `--toggle-interactive`: flip click-through on a running GUI.
#[cfg(target_os = "linux")]
pub async fn run_toggle_interactive_cli() -> anyhow::Result<bool> {
    call_panel_method("ToggleInteractive").await
}

/// Call one no-argument method on the GUI's panel object, with the timeout, the
/// clear messages and the no-blocking-in-a-runtime rules the toggle CLI needs.
#[cfg(target_os = "linux")]
async fn call_panel_method(method: &str) -> anyhow::Result<bool> {
    let conn = zbus::Connection::session().await?;
    let proxy = zbus::Proxy::new(
        &conn,
        "io.lapsphere.Gui",
        "/io/lapsphere/Gui/Panel",
        "io.lapsphere.Gui.Panel",
    )
    .await?;

    match tokio::time::timeout(CLI_TIMEOUT, proxy.call_method(method, &())).await {
        Ok(Ok(_)) => Ok(true),
        Ok(Err(zbus::Error::MethodError(_, _, message))) => {
            // No object exported at that path: the GUI is running but predates
            // this build, or the export failed.
            anyhow::bail!("the running LapSphere GUI has no panel control object ({message})")
        }
        Ok(Err(err)) if is_no_such_name(&err) => {
            anyhow::bail!("no running LapSphere GUI owns the name io.lapsphere.Gui")
        }
        Ok(Err(err)) => Err(anyhow::anyhow!("{err}")),
        Err(_) => anyhow::bail!(
            "the running LapSphere GUI did not answer within {} s — it may be stuck",
            CLI_TIMEOUT.as_secs()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::menu::HideOutcome;

    #[test]
    fn the_default_hotkey_parses() {
        let hotkey = Hotkey::parse(DEFAULT_HOTKEY).expect("default hotkey must parse");
        assert_eq!(hotkey.modifiers, Mask::SHIFT);
        assert_ne!(hotkey.keysym, 0);
    }

    #[test]
    fn an_empty_hotkey_means_no_hotkey() {
        assert_eq!(Hotkey::parse(""), None);
        assert_eq!(Hotkey::parse("   "), None);
    }

    #[test]
    fn function_keys_parse_to_their_keysyms() {
        // The real values, cross-checked against `xmodmap -pke` on this host,
        // where F9 is keycode 75 with keysym 0xffc6.
        assert_eq!(keysym_for("F1"), Some(0xFFBE));
        assert_eq!(keysym_for("F9"), Some(0xFFC6));
        assert_eq!(keysym_for("F10"), Some(0xFFC7));
    }

    #[test]
    fn letters_and_digits_parse() {
        assert_eq!(keysym_for("A"), Some('A' as u32));
        assert_eq!(keysym_for("z"), Some('Z' as u32));
        assert_eq!(keysym_for("7"), Some('7' as u32));
    }

    #[test]
    fn an_unknown_key_name_is_refused() {
        assert_eq!(keysym_for("F99"), None);
        assert_eq!(keysym_for("F0"), None);
        assert_eq!(keysym_for("Nope"), None);
        assert_eq!(keysym_for("AB"), None);
    }

    #[test]
    fn modifiers_accumulate() {
        let hotkey = Hotkey::parse("Shift+Control+F9").expect("parses");
        assert_eq!(hotkey.modifiers, combine(Mask::SHIFT, Mask::CONTROL));
    }

    #[test]
    fn an_unknown_modifier_is_ignored_with_a_warning() {
        // Not fatal: a typo in the config should not stop the panel working.
        let hotkey = Hotkey::parse("Wibble+F9").expect("still parses");
        assert_eq!(hotkey.modifiers, mask(0));
    }

    #[test]
    fn lock_variants_cover_the_three_lock_keys() {
        let variants = lock_variants();
        let names: Vec<&str> = variants.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["none", "capslock", "numlock", "scrolllock"]);

        // Each lock must actually ADD its bit to Shift, which is the whole point
        // of grabbing once per variant.
        for (name, mask) in &variants {
            if name == "none" {
                continue;
            }
            assert_ne!(
                u16::from(*mask),
                u16::from(Mask::SHIFT),
                "{name} must differ from the plain Shift grab"
            );
            assert_eq!(
                u16::from(*mask) & u16::from(Mask::SHIFT),
                u16::from(Mask::SHIFT),
                "{name} must still require Shift"
            );
        }

        // The three lock bits must be distinct from one another, or two variants
        // would be the same grab.
        let mut bits: Vec<u16> = variants.iter().map(|(_, m)| u16::from(*m)).collect();
        let before = bits.len();
        bits.sort_unstable();
        bits.dedup();
        assert_eq!(bits.len(), before, "lock variants must be distinct grabs");
    }

    #[test]
    fn a_toggle_request_is_delivered_once() {
        let visibility = Visibility::new();
        assert!(!visibility.take_toggle(), "nothing pending initially");

        visibility.request_toggle();
        assert!(visibility.take_toggle(), "the request is delivered");
        assert!(!visibility.take_toggle(), "and only once");
    }

    #[test]
    fn the_grab_status_drives_the_return_path() {
        let visibility = Visibility::new();

        visibility.set_grab_status(GrabStatus::Grabbed(0));
        assert!(visibility.hotkey_grabbed());
        assert!(
            visibility.has_return_path(false, false),
            "the key alone is enough"
        );

        visibility.set_grab_status(GrabStatus::Busy);
        assert!(!visibility.hotkey_grabbed());
        assert!(
            !visibility.has_return_path(false, false),
            "no key, no tray and no D-Bus means no way back at all"
        );
        assert!(
            visibility.has_return_path(true, false),
            "the tray alone is enough"
        );
        assert!(
            visibility.has_return_path(false, true),
            "the D-Bus method alone is enough"
        );
    }

    #[test]
    fn a_busy_hotkey_has_a_message_the_menu_can_show() {
        assert_eq!(
            GrabStatus::Busy.message(),
            "Hotkey is taken by another application"
        );
        assert!(!GrabStatus::Busy.is_usable());
        assert!(GrabStatus::Grabbed(0).is_usable());
        assert!(!GrabStatus::NoDisplay.is_usable());
        assert!(!GrabStatus::Failed.is_usable());
    }

    #[test]
    fn hiding_without_any_return_path_falls_back_to_normal_mode() {
        assert_eq!(
            hide_outcome(false, false, false),
            HideOutcome::ReturnToNormal
        );
        assert_eq!(hide_outcome(true, false, false), HideOutcome::Hide);
        assert_eq!(hide_outcome(false, true, false), HideOutcome::Hide);
        assert_eq!(hide_outcome(false, false, true), HideOutcome::Hide);
    }

    #[test]
    fn the_dbus_method_always_counts_as_a_return_path() {
        // The D-Bus method is what makes the panel reachable on Wayland, so it
        // is treated as available whenever the GUI is running.
        let visibility = Visibility::new();
        visibility.set_grab_status(GrabStatus::NoDisplay);
        assert!(
            visibility.has_return_path(false, true),
            "with no X11 display the D-Bus method is the only way back"
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
    fn the_default_hotkey_is_exposed_for_the_config() {
        assert_eq!(default_hotkey(), "Shift_R+F9");
    }

    // ---- The interactivity slot ----

    #[test]
    fn the_two_hotkey_slots_have_independent_statuses() {
        // Either key can be taken by another application while the other is
        // free, so one status field for both would make the safety rule read a
        // stale answer.
        let visibility = Visibility::new();

        visibility.set_grab_status(GrabStatus::Grabbed(0));
        visibility.set_interactivity_grab_status(GrabStatus::Busy);
        assert!(visibility.hotkey_grabbed());
        assert!(
            !visibility.interactivity_grabbed(),
            "a busy interactivity key must not read as grabbed"
        );

        visibility.set_interactivity_grab_status(GrabStatus::Grabbed(0));
        assert!(visibility.interactivity_grabbed());
        assert_eq!(
            visibility.grab_status(),
            GrabStatus::Grabbed(0),
            "the show/hide status is untouched"
        );

        visibility.set_grab_status(GrabStatus::Failed);
        assert!(
            visibility.interactivity_grabbed(),
            "losing the show/hide key must not lose the interactivity key"
        );
    }

    #[test]
    fn an_unset_slot_reads_as_failed_not_as_grabbed() {
        // The initial byte is UNKNOWN; mapping it to `Grabbed` would let
        // click-through be enabled before the grab was ever attempted.
        let visibility = Visibility::new();
        assert!(!visibility.interactivity_grabbed());
        assert!(!visibility.hotkey_grabbed());
    }

    #[test]
    fn the_click_through_gate_follows_the_key_and_the_cli() {
        let visibility = Visibility::new();
        visibility.set_interactivity_grab_status(GrabStatus::Grabbed(0));
        assert!(visibility.can_enable_click_through());

        let no_key = Visibility::new();
        no_key.set_interactivity_grab_status(GrabStatus::Busy);
        assert!(!no_key.can_enable_click_through());
        no_key.set_dbus_available(true);
        assert!(no_key.can_enable_click_through());
    }

    #[test]
    fn an_interactivity_toggle_is_a_separate_request_from_show_hide() {
        // They mean opposite things: one hides the panel, the other gives it the
        // mouse back. Sharing a flag would make one keypress do both.
        let visibility = Visibility::new();

        visibility.request_interactivity_toggle();
        assert!(
            visibility.take_interactivity_toggle(),
            "the interactivity request is delivered"
        );
        assert!(!visibility.take_interactivity_toggle(), "and only once");
        assert!(
            !visibility.take_toggle(),
            "an interactivity request must not be read as a show/hide request"
        );

        visibility.request_toggle();
        assert!(visibility.take_toggle());
        assert!(
            !visibility.take_interactivity_toggle(),
            "a show/hide request must not be read as an interactivity request"
        );
    }

    #[test]
    fn the_dbus_availability_is_recorded_not_assumed() {
        let visibility = Visibility::new();
        assert!(!visibility.dbus_available(), "unknown means unavailable");
        visibility.set_dbus_available(true);
        assert!(visibility.dbus_available());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn stopping_one_slot_leaves_the_other_running() {
        // The watchers are separate threads with separate grabs: stopping the
        // show/hide watcher must not take the interactivity key with it, or
        // re-grabbing one key would silently drop the other.
        let visibility = Visibility::new();
        visibility.stop_slot(HotkeySlot::ShowHide);
        assert!(visibility
            .stop_watcher_for(HotkeySlot::ShowHide)
            .load(Ordering::Relaxed));
        assert!(
            !visibility
                .stop_watcher_for(HotkeySlot::Interactivity)
                .load(Ordering::Relaxed),
            "the other watcher is untouched"
        );
        visibility.stop();
        assert!(visibility
            .stop_watcher_for(HotkeySlot::Interactivity)
            .load(Ordering::Relaxed));
    }

    #[test]
    fn the_slot_names_are_distinct() {
        // A log line saying "hotkey captured" without saying WHICH hotkey is
        // exactly the ambiguity that made the earlier bug hard to see.
        assert_ne!(
            HotkeySlot::ShowHide.name(),
            HotkeySlot::Interactivity.name()
        );
    }
}
