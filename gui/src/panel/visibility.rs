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
    /// `XK_F1`. The function keys are contiguous in steps of 32.
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

    /// The X11 keycode for this hotkey, or 0 if the server has no such key.
    #[cfg(target_os = "linux")]
    pub fn resolve_keycode(&self, conn: &x11rb::rust_connection::RustConnection) -> Option<u8> {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{ConnectionExt, GrabMode};

        let count = conn
            .get_keyboard_mapping(x11consts::MIN_KEYCODE, u8::from(conn.setup().min_keycode))
            .ok()?
            .reply()
            .ok()?
            .keysyms_per_keycode;
        if count == 0 {
            return None;
        }

        // The keysym list is flat, with `keysyms_per_keycode` entries per
        // keycode, so the keycode is the index divided by that count.
        let per_keycode = usize::from(count);
        conn.get_keyboard_mapping(x11consts::MIN_KEYCODE, count)
            .ok()?
            .reply()
            .ok()?
            .keysyms
            .iter()
            .position(|sym| *sym == self.keysym)
            .map(|index| x11consts::MIN_KEYCODE + (index / per_keycode) as u8)
    }
}

/// X11 keysym for a key name.
fn keysym_for(key: &str) -> Option<u32> {
    // Function keys: XK_F1 .. XK_F35 are contiguous in steps of 0x20 (32).
    if let Some(number) = key.strip_prefix('F').or_else(|| key.strip_prefix('f')) {
        if let Ok(n) = number.parse::<u32>() {
            if (1..=35).contains(&n) {
                return Some(x11consts::KEYSYM_F1 + (n - 1) * 32);
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
    let Some(hotkey) = Hotkey::parse(spec) else {
        return (None, GrabStatus::Failed);
    };
    let Some(keycode) = hotkey.resolve_keycode(conn) else {
        log::warn!("panel: the X server has no key for `{spec}`");
        return (None, GrabStatus::Failed);
    };
    let status = grab_hotkey(conn, keycode);
    (Some(Hotkey { keycode, ..hotkey }), status)
}

/// Establish the grab and start the watcher thread.
///
/// Returns the visibility state and the resolved keycode, if the key was
/// captured. The watcher owns a SECOND connection on purpose: an X grab belongs
/// to the connection that made it, so the connection that grabs must also be the
/// one that watches, and the caller's connection is not available for that.
#[cfg(target_os = "linux")]
pub fn start_hotkey(spec: &str, visibility: Arc<Visibility>) -> Option<Hotkey> {
    let Ok((conn, _screen)) = x11rb::rust_connection::RustConnection::connect(None) else {
        visibility.set_grab_status(GrabStatus::NoDisplay);
        log::info!("panel: no X11 display, the hotkey is unavailable");
        return None;
    };

    let (hotkey, status) = grab_from_spec(&conn, spec);
    visibility.set_grab_status(status);

    match hotkey {
        Some(hotkey) if status.is_usable() => {
            // Both flags are clones of the ones inside `visibility`, so the
            // thread's writes are seen by `take_toggle` in the UI.
            let stop = visibility.stop_watcher();
            let trigger = visibility.trigger_flag();
            std::thread::spawn(move || watch_hotkey(hotkey, trigger, stop));
            log::info!("panel: hotkey `{spec}` captured");
            Some(hotkey)
        }
        _ => {
            log::warn!("panel: hotkey `{spec}` not captured: {}", status.message());
            None
        }
    }
}

/// Watch for the grabbed hotkey on a background thread.
///
/// The thread owns its own X connection: a grab is per-connection, so the
/// connection that grabbed the key is the one that must watch for it. It exits
/// when `stop` is set, which is what happens when the hotkey config changes or
/// the app quits.
#[cfg(target_os = "linux")]
fn watch_hotkey(hotkey: Hotkey, trigger: Arc<AtomicBool>, stop: Arc<AtomicBool>) {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, GrabMode};

    let Ok((conn, _screen)) = x11rb::rust_connection::RustConnection::connect(None) else {
        log::warn!("panel: hotkey watcher has no X11 connection");
        return;
    };
    let Some(screen) = conn.setup().roots.first().cloned() else {
        return;
    };
    let root = screen.root;

    // Re-grab on this connection: the grab belongs to the connection.
    for (_, extra) in lock_variants() {
        let _ = conn.grab_key(
            false,
            root,
            extra,
            hotkey.keycode,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
        );
        let _ = extra;
    }
    let _ = conn.flush();

    while !stop.load(Ordering::Relaxed) {
        match conn.poll_for_event() {
            Ok(Some(event)) => {
                if let x11rb::protocol::Event::KeyPress(key) = event {
                    // Compare the keycode: that is what was grabbed, and the
                    // keysym is only meaningful once the keyboard group is
                    // resolved, which is not guaranteed here.
                    if key.detail == hotkey.keycode {
                        trigger.store(true, Ordering::SeqCst);
                    }
                }
            }
            Ok(None) => {
                // No events pending: sleep rather than spin.
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(_) => break,
        }
    }

    for (_, extra) in lock_variants() {
        for (_, extra) in lock_variants() {
            let _ = conn.ungrab_key(hotkey.keycode, root, extra);
        }
        let _ = extra;
    }
    let _ = conn.flush();
}

/// Live visibility state, shared between the hotkey thread, the D-Bus method and
/// the UI.
pub struct Visibility {
    /// Set by the hotkey thread or the D-Bus method; consumed by `ui()`.
    toggle_requested: AtomicBool,
    /// Set by the hotkey thread. Owned here so the watcher and the UI share the
    /// same flag — the thread only ever gets a clone of this Arc.
    hotkey_fired: Arc<AtomicBool>,
    /// Whether the hotkey was actually grabbed.
    grab_status: AtomicU8,
    /// Set to tell the watcher thread to exit.
    stop: Arc<AtomicBool>,
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

impl Visibility {
    pub fn new() -> Self {
        Self {
            toggle_requested: AtomicBool::new(false),
            hotkey_fired: Arc::new(AtomicBool::new(false)),
            grab_status: AtomicU8::new(STATUS_UNKNOWN),
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Ask for a toggle. Safe from any thread.
    pub fn request_toggle(&self) {
        self.toggle_requested.store(true, Ordering::SeqCst);
    }

    /// Take a pending toggle request, from either source.
    ///
    /// Both flags are consumed, so a hotkey press and a D-Bus call that land in
    /// the same frame produce one toggle rather than two.
    pub fn take_toggle(&self) -> bool {
        let requested = self.toggle_requested.swap(false, Ordering::SeqCst);
        let hotkey = self.hotkey_fired.swap(false, Ordering::SeqCst);
        requested || hotkey
    }

    pub fn set_grab_status(&self, status: GrabStatus) {
        let value = match status {
            GrabStatus::Grabbed(_) => STATUS_GRABBED,
            GrabStatus::Busy => STATUS_BUSY,
            GrabStatus::NoDisplay => STATUS_NO_DISPLAY,
            GrabStatus::Failed => STATUS_FAILED,
        };
        self.grab_status.store(value, Ordering::Relaxed);
    }

    pub fn grab_status(&self) -> GrabStatus {
        match self.grab_status.load(Ordering::Relaxed) {
            STATUS_GRABBED => GrabStatus::Grabbed(0),
            STATUS_BUSY => GrabStatus::Busy,
            STATUS_NO_DISPLAY => GrabStatus::NoDisplay,
            STATUS_FAILED => GrabStatus::Failed,
            _ => GrabStatus::Failed,
        }
    }

    /// Was the hotkey captured?
    pub fn hotkey_grabbed(&self) -> bool {
        self.grab_status().is_usable()
    }

    /// Stop the watcher thread.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// The flag the watcher thread sets when the hotkey fires.
    pub fn trigger_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.hotkey_fired)
    }

    /// The flag that stops the watcher thread.
    pub fn stop_watcher(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
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
}

/// Handle `--toggle-panel`: ask a running GUI to toggle, and report whether one
/// was there.
#[cfg(target_os = "linux")]
pub async fn run_toggle_cli() -> anyhow::Result<bool> {
    use zbus::fdo::DBusProxy;

    let conn = zbus::Connection::session().await?;
    let proxy = zbus::Proxy::new(
        &conn,
        "io.lapsphere.Gui",
        "/io/lapsphere/Gui/Panel",
        "io.lapsphere.Gui.Panel",
    )
    .await?;

    match proxy.call_method("TogglePanel", &()).await {
        Ok(_) => Ok(true),
        Err(err) => {
            let _ = DBusProxy::new(&conn).await; // keep the type in scope for docs
            Err(anyhow::anyhow!("{err}"))
        }
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
        // XK_F1 .. XK_F35 are contiguous in steps of 32.
        assert_eq!(keysym_for("F1"), Some(x11consts::KEYSYM_F1));
        assert_eq!(keysym_for("F9"), Some(x11consts::KEYSYM_F1 + 8 * 32));
        assert_eq!(keysym_for("F10"), Some(x11consts::KEYSYM_F1 + 9 * 32));
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

    #[test]
    fn the_default_hotkey_is_exposed_for_the_config() {
        assert_eq!(default_hotkey(), "Shift_R+F9");
    }
}
