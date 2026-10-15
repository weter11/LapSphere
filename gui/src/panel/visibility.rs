//! Showing and hiding the panel: the global hotkey, the D-Bus method, and the
//! rule that decides whether hiding is allowed at all.
//!
//! # Three routes to one window
//!
//! The app has one window and the panel has one hidden state, and it is brought
//! back by any of:
//!
//! * the **global hotkey** (`Shift+F9` by default) — a passive `XGrabKey` on
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
//! * **The grab masks come from the configured modifiers**, plus every
//!   combination of the lock bits ([`lock_variants`]). A fixed mask would grab a
//!   key nobody configured: the server delivers a `KeyPress` only for the mask
//!   the grab was made with, so the configured `Control+Alt+F5` grabbed under
//!   `Shift` does nothing while the log says "captured". The lock combinations
//!   are what make the key still work with CapsLock, NumLock and ScrollLock set —
//!   including several of them at once.
//! * **`Shift`, not `Shift_R`.** `XGrabKey` takes a `ModMask` and both Shift keys
//!   set the same `ShiftMask` bit, so a right-hand-only grab cannot be expressed:
//!   the shortcut is documented as `Shift+F9` and either Shift key fires it.
//!   `Shift_L`/`Shift_R` stay accepted as spellings of the same mask, so a
//!   `panel.json` written by an earlier build keeps working.
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

/// The lock-key bits the server may OR into the modifier mask it DELIVERS, as
/// the states those keys can be in at press time.
///
/// All eight combinations, not just the three single locks: the server reports
/// ONE mask for the state the keyboard is in, and with CapsLock **and** NumLock
/// both on the delivered mask carries both bits. A grab made only for the
/// single-lock variants misses the key in exactly the state a user who leaves
/// both locks set is always in.
pub const LOCK_COMBINATIONS: [(&str, u16); 8] = [
    ("none", 0),
    ("capslock", MOD_LOCK),
    ("numlock", MOD_M2),
    ("capslock+numlock", MOD_LOCK | MOD_M2),
    ("scrolllock", MOD_M5),
    ("capslock+scrolllock", MOD_LOCK | MOD_M5),
    ("numlock+scrolllock", MOD_M2 | MOD_M5),
    ("capslock+numlock+scrolllock", MOD_LOCK | MOD_M2 | MOD_M5),
];

/// Every mask `keycode` is grabbed under for a hotkey configured with
/// `modifiers`, in the order they are tried.
///
/// One grab per lock combination, and each is the hotkey's **own** modifiers plus
/// those lock bits — never a fixed set. This is not cosmetic: the server delivers
/// a `KeyPress` only for the mask the grab was made with, so a `Control+Alt+F5`
/// hotkey grabbed under `Shift` is a key that does nothing at all, and the
/// capture log still says it was captured.
///
/// Duplicates are dropped, because a spec that names a lock key as its modifier
/// (`CapsLock+F9`) collapses some combinations onto one mask, and a second
/// identical grab would only distort the per-variant counts.
pub fn lock_variants(modifiers: u16) -> Vec<(&'static str, u16)> {
    let mut variants: Vec<(&'static str, u16)> = Vec::with_capacity(LOCK_COMBINATIONS.len());
    for (name, lock_bits) in LOCK_COMBINATIONS {
        let bits = modifiers | lock_bits;
        if !variants.iter().any(|(_, seen)| *seen == bits) {
            variants.push((name, bits));
        }
    }
    variants
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
    /// Captured **in every lock state**: the server accepted the grab for each
    /// unique mask that was attempted.
    ///
    /// This is the only status the hide-safety rule counts as a guaranteed way
    /// back, because it is the only one where the key works whatever the lock
    /// keys are doing.
    Grabbed,
    /// Captured in **some** lock states: at least one unique mask was accepted and
    /// at least one was not (another client owns it, or its request was refused).
    ///
    /// A real hotkey — presses in the states that were acquired do arrive, so the
    /// watcher must run — but not a guaranteed route back, because whether the key
    /// fires depends on the lock keys. Distinct from [`GrabStatus::Grabbed`] so
    /// the UI and the safety rule cannot treat "usually works" as "always works".
    Partial,
    /// The user turned the key off (an empty spec in `panel.json`).
    ///
    /// Distinct from [`GrabStatus::Failed`] because the two need different
    /// sentences: "no key configured" is a choice the tray and the CLI already
    /// cover, while "could not capture" is something to act on.
    Disabled,
    /// The combination is already owned by another client (X11 `BadAccess`), and
    /// **no** variant was acquired.
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
            GrabStatus::Partial => "Hotkey captured for some lock states only",
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

    /// The longer explanation the settings surface shows under the status line,
    /// or `None` when the one line says everything there is to say.
    ///
    /// The details that need per-mask data — which combinations are somebody
    /// else's — stay in the log; this is the part the user has to read to know
    /// what will happen when they press the key.
    ///
    /// [`GrabStatus::Partial`] deliberately does **not** name a cause. The status is
    /// produced for two different situations — combinations another client owns
    /// (`BadAccess`) and variants the server refused for other reasons — and the status
    /// itself does not carry which one it was, so the line sends the user to the log
    /// rather than stating a reason it cannot know.
    pub fn warning(self) -> Option<&'static str> {
        match self {
            GrabStatus::Busy => Some(
                "Another application already owns this key. Pick a different one, or use \
                 the tray or `lapsphere --toggle-panel`.",
            ),
            GrabStatus::Partial => Some(
                "Only some lock-state combinations could be captured, so this key may not work \
                 with CapsLock, NumLock or ScrollLock on. Which combination was not acquired, and \
                 why, is in the log.",
            ),
            _ => None,
        }
    }

    /// Should the hotkey work at all — i.e. did the server accept at least one
    /// unique mask?
    ///
    /// The predicate the watcher's life depends on, and nothing else: a
    /// [`GrabStatus::Partial`] key does fire in the states that were acquired, so
    /// the thread that polls it must not be torn down. Reading the safety
    /// predicate here instead would silently kill a working key.
    pub fn keeps_watcher(self) -> bool {
        matches!(self, GrabStatus::Grabbed | GrabStatus::Partial)
    }

    /// Is this a key the user can rely on as a way back — captured in EVERY lock
    /// state?
    ///
    /// The one place a "way back" is defined, so the safety rule can never count a
    /// refused, disabled, not-yet-attempted or partially captured grab as a
    /// guaranteed route. A partial grab is not one: whether it fires depends on
    /// the lock keys.
    pub fn is_reliable_route(self) -> bool {
        matches!(self, GrabStatus::Grabbed)
    }
}

/// A parsed hotkey such as `Shift+F9`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    /// X11 modifier bits ([`MOD_SHIFT`] and friends). These are the bits the grab
    /// is made with — see [`lock_variants`].
    pub modifiers: u16,
    /// X11 keysym value.
    pub keysym: u32,
    /// X11 keycode, resolved against the running server at grab time.
    pub keycode: u8,
}

#[cfg(test)]
impl Hotkey {
    /// The hotkey a spec denotes, without the parts of it that were dropped.
    ///
    /// Test-only, and deliberately so: production reads a spec through
    /// [`parse_spec`], which also reports the tokens it ignored and whether one of
    /// the two Shift keys was named — the data the apply-time notes
    /// ([`log_applied_spec`]) and the settings warning are built from.
    pub fn parse(spec: &str) -> Option<Self> {
        parse_spec(spec).map(|parsed| parsed.hotkey)
    }
}

/// The modifier bits a token denotes, or `None` when this parser does not know
/// the token.
///
/// The ONE spelling table: [`parse_spec`], the ignored-token report and the
/// effective-shortcut description all read it, so a spec cannot parse one way and
/// be explained another.
pub fn modifier_bits(token: &str) -> Option<u16> {
    Some(match token {
        "Shift" | "Shift_L" | "Shift_R" => MOD_SHIFT,
        "Lock" | "CapsLock" => MOD_LOCK,
        "Control" | "Ctrl" | "Control_L" | "Control_R" => MOD_CONTROL,
        "Alt" | "Mod1" => MOD_M1,
        "NumLock" => MOD_M2,
        "ScrollLock" => MOD_M5,
        _ => return None,
    })
}

/// The canonical spelling of each modifier bit, in the order the effective
/// shortcut is written.
///
/// Canonical, not the user's alias: `Ctrl` is described as `Control` and `Lock`
/// as `CapsLock`, because this string is what the app will actually grab.
const MODIFIER_NAMES: [(u16, &str); 6] = [
    (MOD_SHIFT, "Shift"),
    (MOD_CONTROL, "Control"),
    (MOD_M1, "Alt"),
    (MOD_LOCK, "CapsLock"),
    (MOD_M2, "NumLock"),
    (MOD_M5, "ScrollLock"),
];

/// A spec, the hotkey it denotes, and the parts of it that were dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSpec {
    pub hotkey: Hotkey,
    /// Modifier tokens this parser does not know, in the order they appear.
    ///
    /// They are ignored rather than fatal, so the effective shortcut is shorter
    /// than what the user typed — which is exactly what the settings surface has
    /// to say out loud instead of reporting the spec as an exact match.
    pub ignored: Vec<String>,
    /// The key token as it was written, for the effective-shortcut line.
    pub key_token: String,
    /// Did the spec name one of the two Shift keys (`Shift_L` / `Shift_R`)?
    ///
    /// One of them parses to the same `ShiftMask` bit as `Shift` does, so the
    /// distinction is dropped: the note about that belongs to the caller that
    /// applies the key, and this flag is what lets it say so without parsing the
    /// string a second time.
    pub named_shift_key: bool,
}

impl ParsedSpec {
    /// The shortcut that will actually be grabbed: the recognised modifiers in
    /// canonical spelling, plus the key.
    ///
    /// This is the string the user has to be shown for `Wibble+F9`, whose
    /// effective shortcut is plain `F9`.
    pub fn effective(&self) -> String {
        let mut parts: Vec<&str> = MODIFIER_NAMES
            .iter()
            .filter(|(bit, _)| self.hotkey.modifiers & bit != 0)
            .map(|(_, name)| *name)
            .collect();
        let key = canonical_key_name(&self.key_token, self.hotkey.keysym);
        parts.push(&key);
        parts.join("+")
    }
}

/// The key as the app names it, derived from the keysym that was resolved rather
/// than from the token as typed: `f5` and `F5` are the same key and must not
/// produce two different descriptions, and a letter's keysym is its upper case.
fn canonical_key_name(token: &str, keysym: u32) -> String {
    const F35_KEYKSYM: u32 = KEYSYM_F1 + 34;
    if (KEYSYM_F1..=F35_KEYKSYM).contains(&keysym) {
        return format!("F{}", keysym - KEYSYM_F1 + 1);
    }
    if token.len() == 1 {
        if let Some(ch) = token.chars().next() {
            if ch.is_ascii_alphabetic() {
                return ch.to_ascii_uppercase().to_string();
            }
        }
    }
    token.to_string()
}

/// Parse a spec, keeping the modifier tokens that were ignored.
///
/// Modifiers are `Shift`/`Shift_L`/`Shift_R`, `Control`/`Ctrl`, `Alt`/`Mod1`,
/// `Lock`/`CapsLock`, `NumLock`, `ScrollLock`, joined by `+` with a final key name
/// (`F1`..`F35`, `A`..`Z`, `0`..`9`).
///
/// `Shift_L` and `Shift_R` are accepted but parse to plain `ShiftMask`: X11 has no
/// way to ask for one of the two Shift keys in a grab, and a spec that claimed a
/// right-hand-only key would promise something the server cannot deliver. That the
/// distinction was dropped is one of the notes [`log_applied_spec`] emits when the
/// key is applied — the flag travels in the value ([`ParsedSpec::named_shift_key`]).
///
/// An empty spec is not an error: it is the user turning the key off, and the tray
/// and `--toggle-panel` still work. An unknown modifier is ignored rather than
/// failing the whole spec, so one typo does not disable the key, and the token is
/// returned in [`ParsedSpec::ignored`] — which is what the settings warning and the
/// apply-time note are built from.
///
/// `None` for an empty spec or for a key name this parser does not know.
///
/// **Side-effect free on purpose.** Everything the parse learned is returned in the
/// value, because this is also the settings surface's read path — it parses the
/// configured spec on every repaint, so a parser that logged would turn an open
/// window into a stream of identical lines. The one-off notes belong to the caller
/// that APPLIES the key: [`log_applied_spec`].
pub fn parse_spec(spec: &str) -> Option<ParsedSpec> {
    let spec = spec.trim();
    if spec.is_empty() {
        return None;
    }

    let parts: Vec<&str> = spec.split('+').map(|part| part.trim()).collect();
    let (key, modifier_parts) = parts.split_last()?;

    let mut modifiers = 0u16;
    let mut ignored: Vec<String> = Vec::new();
    let mut named_shift_key = false;
    for part in modifier_parts {
        match modifier_bits(part) {
            Some(bits) => {
                modifiers |= bits;
                if *part == "Shift_L" || *part == "Shift_R" {
                    named_shift_key = true;
                }
            }
            // Ignored, not fatal: one typo must not disable the key. The token is
            // kept so the UI can say which one it was.
            None => ignored.push((*part).to_string()),
        }
    }

    let keysym = keysym_for(key)?;

    Some(ParsedSpec {
        hotkey: Hotkey {
            modifiers,
            keysym,
            // Filled in against the running server; 0 means "not resolved yet".
            keycode: 0,
        },
        ignored,
        key_token: (*key).to_string(),
        named_shift_key,
    })
}

/// Emit the one-off notes about a spec that is being **applied**: which modifier
/// tokens were dropped, and that a `Shift_L`/`Shift_R` spelling collapses onto the
/// single `ShiftMask` bit.
///
/// Called from [`start_hotkey`] — the initial grab and every re-grab after a
/// settings change — and from nowhere else on purpose. The settings surface parses
/// and re-parses the spec as it repaints ([`ignored_modifiers_warning`]), and a
/// parser that logged there would emit the same lines on every frame.
pub fn log_applied_spec(spec: &str, parsed: &ParsedSpec) {
    if !parsed.ignored.is_empty() {
        log::warn!(
            "panel: hotkey `{spec}` ignores unknown modifier(s) {}; the effective shortcut is \
             `{}`",
            parsed
                .ignored
                .iter()
                .map(|token| format!("`{token}`"))
                .collect::<Vec<_>>()
                .join(", "),
            parsed.effective()
        );
    }
    if parsed.named_shift_key {
        log::info!(
            "panel: `{spec}` names one Shift key, but an X11 grab takes one ShiftMask bit; \
             the shortcut is Shift+<key> and either Shift key fires it"
        );
    }
}

/// The warning the settings surface shows for a spec with ignored modifiers, or
/// `None` when the spec is exactly what it says.
///
/// Both halves matter: which tokens were dropped, and what the shortcut will
/// really be. A spec of `Wibble+F9` that reports "Hotkey captured" reads as an
/// exact match, and the key the user thinks they configured does not exist.
///
/// Called while rendering, so it is pure: parsing is silent and the returned string
/// is derived only from the spec.
pub fn ignored_modifiers_warning(spec: &str) -> Option<String> {
    let parsed = parse_spec(spec)?;
    if parsed.ignored.is_empty() {
        return None;
    }
    let tokens = parsed
        .ignored
        .iter()
        .map(|token| format!("`{token}`"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "Ignored modifier{plural} {tokens}: not a name this build knows, so the effective \
         shortcut is `{effective}`. Fix the spelling, or clear the field to turn the key off.",
        plural = if parsed.ignored.len() == 1 { "" } else { "s" },
        effective = parsed.effective(),
    ))
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
/// One grab per **unique** mask ([`lock_variants`] drops the combinations a spec
/// already asks for), and the server refuses each independently: another client
/// may own the NumLock combination while the plain one is free. So the comparison
/// is against `attempted`, the number of unique masks that were tried — not
/// against a constant:
///
/// * every attempted mask accepted is [`GrabStatus::Grabbed`] — the key works in
///   every lock state, which is what makes it a way back;
/// * some accepted and some not is [`GrabStatus::Partial`] — the key fires in the
///   states that were acquired and not in the others, so it is usable but not
///   guaranteed;
/// * nothing accepted with at least one `BadAccess` is [`GrabStatus::Busy`] — the
///   key is somebody else's;
/// * nothing accepted and nothing busy is [`GrabStatus::Failed`].
///
/// The order matters: a refused variant must never be reported as captured, a
/// partial success must never be reported as complete, and a partial success must
/// not be reported as busy.
pub fn grab_outcome(grabbed: usize, busy: usize, attempted: usize) -> GrabStatus {
    if attempted == 0 {
        // Nothing was even attempted: there is no key, so there is nothing to
        // report as captured or as somebody else's.
        GrabStatus::Failed
    } else if grabbed == attempted {
        GrabStatus::Grabbed
    } else if grabbed > 0 {
        GrabStatus::Partial
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

/// The line logged when the panel is hidden, naming what actually brings it back.
///
/// A partially captured key keeps its watcher and its presses, but it is the tray
/// and the CLI that make hiding safe: whether it fires depends on the lock keys in
/// use, so the log must not promise a key that may not work. One function for both
/// hide paths (a toggle request and the menu) so neither can promise it.
pub fn hidden_log_line(status: GrabStatus) -> &'static str {
    if status.is_reliable_route() {
        "panel: hidden; the hotkey, the tray or `lapsphere --toggle-panel` brings it back"
    } else {
        "panel: hidden; the hotkey is not a reliable route in this state, so the tray or \
         `lapsphere --toggle-panel` is what brings it back"
    }
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

    /// Is the show/hide hotkey a **reliable** way back — captured in every lock
    /// state?
    ///
    /// The hide-safety input, and deliberately not "did any grab succeed": a
    /// [`GrabStatus::Partial`] key may or may not fire depending on the lock keys,
    /// so it cannot be the only route back. The tray and the D-Bus method are the
    /// routes that cover that gap, and [`can_hide`] counts them separately.
    pub fn hotkey_is_reliable_route(&self) -> bool {
        self.grab_status().is_reliable_route()
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
        can_hide(
            self.hotkey_is_reliable_route(),
            tray_enabled,
            self.dbus_available(),
        )
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
const STATUS_PARTIAL: u8 = 6;

fn status_from_byte(byte: u8) -> GrabStatus {
    match byte {
        STATUS_GRABBED => GrabStatus::Grabbed,
        STATUS_PARTIAL => GrabStatus::Partial,
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
        GrabStatus::Partial => STATUS_PARTIAL,
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

/// Grab `keycode` on the root window under every mask [`lock_variants`] derives
/// from the hotkey's `modifiers`.
///
/// Each variant's result is confirmed before the next is attempted, and that is
/// not optional: `XGrabKey` is void on the wire, so success and "another client
/// already owns this combination" look identical until the error event arrives.
/// Reporting "captured" for a refused grab would make the safety rule offer a
/// key that does nothing.
#[cfg(target_os = "linux")]
fn grab_hotkey(
    conn: &x11rb::rust_connection::RustConnection,
    keycode: u8,
    modifiers: u16,
) -> GrabStatus {
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

    let variants = lock_variants(modifiers);
    // The comparison base for the outcome: the UNIQUE masks this spec needs, not a
    // constant. A spec that asks for a lock key itself (`CapsLock+F9`) has fewer.
    let attempted = variants.len();

    for (name, bits) in variants {
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
            Ok(()) => {
                grabbed += 1;
                // The mask actually sent, per variant: the only place the
                // configured modifiers and the lock bits are visible on the
                // wire, and what a live run is checked against.
                log::info!("panel: grabbed keycode {keycode} under {name} (mask {bits:#06x})");
            }
            // `ErrorKind::Access` is `BadAccess` on the wire: another client
            // already owns this exact combination.
            Err(ReplyError::X11Error(err)) if err.error_kind == ErrorKind::Access => {
                busy += 1;
                // The per-mask detail the user's status line cannot carry: which
                // combination is somebody else's, so which lock state the hotkey
                // will not fire in.
                log::warn!(
                    "panel: keycode {keycode} under {name} (mask {bits:#06x}) is already grabbed \
                     by another client; the hotkey does not fire in that lock state"
                );
            }
            Err(err) => log::warn!(
                "panel: grab of keycode {keycode} under {name} refused ({err}); \
                 trying the next lock variant"
            ),
        }
    }

    grab_outcome(grabbed, busy, attempted)
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

    let Some(parsed) = parse_spec(&spec) else {
        visibility.set_grab_status(GrabStatus::Failed);
        log::warn!("panel: hotkey `{spec}` does not parse; the tray and the CLI still work");
        return;
    };
    // The parse is silent; these are the lines the user gets once per apply. This
    // is the only production caller: the initial grab and the re-grab after a
    // settings change, never a repaint.
    log_applied_spec(&spec, &parsed);
    let hotkey = parsed.hotkey;

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

        // Grab on THIS connection, which is the one that will poll it, and with
        // the modifiers the user configured: a fixed mask would grab a
        // combination nobody asked for.
        let status = grab_hotkey(&conn, keycode, hotkey.modifiers);
        visibility.set_grab_status(status);
        // `keeps_watcher`, not the safety predicate: a PARTIAL grab is a working
        // key in the lock states it acquired, so the thread that polls it must
        // run. Returning here on anything but a complete grab would leave a key
        // that is installed on the server and never listened to.
        if !status.keeps_watcher() {
            log::warn!(
                "panel: hotkey `{spec}` not captured (keycode {keycode}, mask {:#06x}): {}",
                hotkey.modifiers,
                status.message()
            );
            return;
        }
        if status.is_reliable_route() {
            log::info!(
                "panel: hotkey `{spec}` captured (keycode {keycode}, modifiers {:#06x})",
                hotkey.modifiers
            );
        } else {
            // Not the "captured" line: some lock states are missing, and the
            // per-mask warnings above say which.
            log::warn!(
                "panel: hotkey `{spec}` captured for some lock states only (keycode {keycode}, \
                 modifiers {:#06x}); it may not fire with CapsLock, NumLock or ScrollLock on",
                hotkey.modifiers
            );
        }

        watch_hotkey(&conn, keycode, hotkey.modifiers, trigger, stop, ctx_flag);
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
    modifiers: u16,
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

    // Release every variant this thread grabbed — the same masks it grabbed,
    // derived from the same modifiers.
    if let Some(screen) = conn.setup().roots.first() {
        for (_, bits) in lock_variants(modifiers) {
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
        assert_eq!(DEFAULT_HOTKEY, "Shift+F9");
        let hotkey = Hotkey::parse(DEFAULT_HOTKEY).expect("the default must parse");
        assert_eq!(hotkey.modifiers, MOD_SHIFT);
        assert_eq!(hotkey.keysym, 0xFFC6);
    }

    #[test]
    fn shift_and_the_two_shift_keys_are_one_grab() {
        // `Shift_R` was the old default's spelling. X11 cannot deliver on it:
        // both Shift keys set `ShiftMask` and the grab takes one mask, so a
        // right-hand-only claim would be unimplementable. The three spellings are
        // therefore one mask — which is what "either Shift key fires it" means —
        // and a `panel.json` written by an earlier build keeps working.
        for spec in ["Shift+F9", "Shift_L+F9", "Shift_R+F9"] {
            let hotkey = Hotkey::parse(spec).unwrap_or_else(|| panic!("{spec} parses"));
            assert_eq!(hotkey.modifiers, MOD_SHIFT, "{spec}");
            assert_eq!(hotkey.keysym, 0xFFC6, "{spec}");
        }

        // And the described shortcut really is the default one: the string the
        // settings hint, the docs and the config all show.
        assert_eq!(
            Hotkey::parse(DEFAULT_HOTKEY).unwrap().modifiers,
            Hotkey::parse("Shift_R+F9").unwrap().modifiers
        );
    }

    #[test]
    fn an_empty_spec_is_a_disabled_hotkey_not_a_parse_error() {
        // The user turning the key off: the tray and the CLI still work, and the
        // safety rule treats it as "no key". Measured live, this used to be
        // reported as `Failed`, i.e. the UI told the user their key "could not be
        // captured" when they had deliberately turned it off.
        assert_eq!(Hotkey::parse(""), None);
        assert_eq!(Hotkey::parse("   "), None);
        assert!(!GrabStatus::Disabled.is_reliable_route());
        assert!(!GrabStatus::Disabled.keeps_watcher());
        let message = GrabStatus::Disabled.message();
        assert!(message.contains("No hotkey configured"), "got {message}");
        assert!(message.contains("--toggle-panel"), "got {message}");
    }

    #[test]
    fn an_ignored_modifier_is_reported_rather_than_silently_dropped() {
        // `Wibble+F9` grabs plain F9. Without a report the settings surface shows
        // `Wibble+F9 — Hotkey captured`, which reads as an exact match for a
        // shortcut that does not exist.
        let parsed = parse_spec("Wibble+F9").expect("the key is known, so the spec parses");
        assert_eq!(parsed.ignored, vec!["Wibble".to_string()]);
        assert_eq!(parsed.effective(), "F9");
        assert_eq!(parsed.hotkey.modifiers, 0, "nothing was recognised");
        assert_eq!(parsed.hotkey.keysym, keysym_for("F9").unwrap());

        let warning = ignored_modifiers_warning("Wibble+F9").expect("a warning");
        assert!(warning.contains("Wibble"), "got {warning}");
        assert!(warning.contains("effective"), "got {warning}");
        assert!(warning.contains("F9"), "got {warning}");
    }

    #[test]
    fn a_spec_with_nothing_ignored_gets_no_warning() {
        // The negative control: a warning that fires for a correct spec would be
        // noise the user cannot clear.
        for spec in [
            "Shift+F9",
            "Shift_R+F9",
            "Shift_L+F9",
            "Control+Alt+A",
            "F9",
        ] {
            assert_eq!(
                ignored_modifiers_warning(spec),
                None,
                "{spec} is understood exactly as written"
            );
            assert!(parse_spec(spec).unwrap().ignored.is_empty(), "{spec}");
        }

        // And the states with no key at all say nothing either.
        assert_eq!(ignored_modifiers_warning(""), None);
        assert_eq!(ignored_modifiers_warning("   "), None);
        assert_eq!(ignored_modifiers_warning("Wibble+Nope"), None);
    }

    #[test]
    fn the_effective_shortcut_names_the_aliases_canonically() {
        // The string the user is shown has to be what the app will grab: `Ctrl` is
        // `Control`, `Lock` is `CapsLock`, and a lower-case key is upper case.
        let cases = [
            ("Ctrl+F9", "Control+F9"),
            ("Control+Alt+A", "Control+Alt+A"),
            ("Lock+NumLock+F9", "CapsLock+NumLock+F9"),
            ("Shift+Wibble+ScrollLock+f5", "Shift+ScrollLock+F5"),
            ("Alt+Mod1+F9", "Alt+F9"),
            ("f9", "F9"),
        ];
        for (spec, expected) in cases {
            let parsed = parse_spec(spec).unwrap_or_else(|| panic!("{spec} parses"));
            assert_eq!(parsed.effective(), expected, "{spec}");
        }
    }

    #[test]
    fn every_ignored_token_is_named_and_the_supported_ones_are_not() {
        // Two typos in one spec: both are reported, in the order they appear, and
        // the recognised modifiers survive.
        let parsed = parse_spec("Wibble+Shift+Nope+F9").expect("parses");
        assert_eq!(
            parsed.ignored,
            vec!["Wibble".to_string(), "Nope".to_string()]
        );
        assert_eq!(parsed.hotkey.modifiers, MOD_SHIFT);
        assert_eq!(parsed.effective(), "Shift+F9");

        let warning = ignored_modifiers_warning("Wibble+Shift+Nope+F9").unwrap();
        assert!(
            warning.contains("Wibble") && warning.contains("Nope"),
            "{warning}"
        );
        assert!(
            warning.contains("modifiers"),
            "more than one token is reported in the plural: {warning}"
        );

        // The spelled aliases are all supported, so none of them is ever reported
        // as ignored — that is what makes the warning mean "this is a typo".
        let aliases = [
            ("Shift_L", MOD_SHIFT),
            ("Shift_R", MOD_SHIFT),
            ("Shift", MOD_SHIFT),
            ("Ctrl", MOD_CONTROL),
            ("Control", MOD_CONTROL),
            ("Control_R", MOD_CONTROL),
            ("Alt", MOD_M1),
            ("Mod1", MOD_M1),
            ("Lock", MOD_LOCK),
            ("CapsLock", MOD_LOCK),
            ("NumLock", MOD_M2),
            ("ScrollLock", MOD_M5),
        ];
        for (alias, bits) in aliases {
            assert_eq!(modifier_bits(alias), Some(bits), "{alias}");
            assert!(
                parse_spec(&format!("{alias}+F9"))
                    .unwrap()
                    .ignored
                    .is_empty(),
                "{alias} is supported"
            );
        }
        assert_eq!(modifier_bits("Wibble"), None);
    }

    // ---- what gets logged, and when ----

    /// Every line the `log` facade produced, for the tests that have to tell "said
    /// nothing" from "said something".
    ///
    /// The test binary installs this logger once. Records from other tests are
    /// filtered out by matching on a spec string only the test in question uses,
    /// because the tests run in parallel and share the facade.
    static CAPTURED: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());

    struct CaptureLines;

    impl log::Log for CaptureLines {
        fn enabled(&self, _metadata: &log::Metadata) -> bool {
            true
        }
        fn log(&self, record: &log::Record) {
            let mut captured = CAPTURED.lock().unwrap_or_else(|err| err.into_inner());
            captured.push((record.level(), record.args().to_string()));
        }
        fn flush(&self) {}
    }

    fn capture_installed() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            log::set_logger(&CaptureLines).expect("the test binary installs no other logger");
            log::set_max_level(log::LevelFilter::Trace);
        });
    }

    fn captured_len() -> usize {
        CAPTURED.lock().unwrap_or_else(|err| err.into_inner()).len()
    }

    /// The lines captured since `mark` that mention `needle`.
    fn lines_about(mark: usize, needle: &str) -> Vec<(log::Level, String)> {
        let captured = CAPTURED.lock().unwrap_or_else(|err| err.into_inner());
        captured[mark.min(captured.len())..]
            .iter()
            .filter(|(_, line)| line.contains(needle))
            .cloned()
            .collect()
    }

    #[test]
    fn parsing_a_spec_is_silent_and_applying_it_says_it_once() {
        capture_installed();
        // A spec string nothing else in this binary uses, so the records can be
        // attributed even though other tests log in parallel.
        let spec = "Wibblestem+Shift_R+Nope+F9";

        let mark = captured_len();
        let parsed = parse_spec(spec).expect("the key is known");
        let warning = ignored_modifiers_warning(spec).expect("a warning for the settings surface");
        assert!(!warning.is_empty());
        let _ = Hotkey::parse(spec);
        assert_eq!(
            lines_about(mark, spec),
            Vec::new(),
            "parsing, and the settings surface's read path, must not log"
        );

        // The positive control: those records ARE visible when emitted, so the
        // assertion above is about silence rather than about a deaf harness.
        let mark = captured_len();
        log_applied_spec(spec, &parsed);
        let notes = lines_about(mark, spec);
        assert_eq!(notes.len(), 2, "one warning, one info line: {notes:#?}");
        assert!(
            notes.iter().any(|(level, line)| *level == log::Level::Warn
                && line.contains("Wibblestem")
                && line.contains("Nope")
                && line.contains("effective")),
            "{notes:#?}"
        );
        assert!(
            notes
                .iter()
                .any(|(level, line)| *level == log::Level::Info && line.contains("ShiftMask")),
            "{notes:#?}"
        );
    }

    #[test]
    fn a_repainting_settings_window_does_not_log() {
        // The review finding: `ignored_modifiers_warning` runs on every repaint, and
        // it used to parse — and therefore log — each time.
        capture_installed();
        let spec = "Repainttest+F9";
        let mark = captured_len();
        for _ in 0..20 {
            assert!(ignored_modifiers_warning(spec).is_some());
            let _ = parse_spec(spec);
            let _ = Hotkey::parse(spec);
        }
        assert_eq!(
            lines_about(mark, spec),
            Vec::new(),
            "twenty repaints must produce nothing"
        );
    }

    #[test]
    fn the_shift_alias_is_data_for_the_apply_time_note() {
        // The flag the note reads, so both Shift spellings still parse to the one
        // mask and the "either Shift key fires it" line needs no second parse.
        for spec in ["Shift_L+F9", "Shift_R+F9", "Control+Shift_L+A"] {
            assert!(parse_spec(spec).unwrap().named_shift_key, "{spec}");
        }
        for spec in ["Shift+F9", "Control+Alt+A", "F9"] {
            assert!(!parse_spec(spec).unwrap().named_shift_key, "{spec}");
        }
    }

    #[test]
    fn the_entered_spec_is_what_gets_grabbed_except_for_ignored_tokens() {
        // Parsing must not "fix" the spec: the recognised bits are used and the rest
        // is dropped, which is exactly the behaviour the warning describes.
        let with_typo = Hotkey::parse("Wibble+Control+F5").expect("parses");
        let clean = Hotkey::parse("Control+F5").expect("parses");
        assert_eq!(with_typo, clean, "the same grab, spec aside");
        assert_eq!(
            lock_variants(with_typo.modifiers),
            lock_variants(clean.modifiers)
        );
    }

    #[test]
    fn every_status_round_trips_through_the_shared_byte() {
        // The store is a byte behind an atomic, so a variant missing from either
        // direction reads as a different state: `Disabled` would come back as
        // `Unknown`, and the UI would say "not checked yet" forever. `Partial` is
        // the newest variant and the one the safety rule distinguishes, so a gap
        // here would silently turn it into "not checked yet" — which is *not* a way
        // back either, but for the wrong reason and with the wrong sentence.
        let visibility = Visibility::new();
        for status in [
            GrabStatus::Unknown,
            GrabStatus::Grabbed,
            GrabStatus::Partial,
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
        let variants = lock_variants(MOD_SHIFT);
        let names: Vec<&str> = variants.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            vec![
                "none",
                "capslock",
                "numlock",
                "capslock+numlock",
                "scrolllock",
                "capslock+scrolllock",
                "numlock+scrolllock",
                "capslock+numlock+scrolllock",
            ]
        );

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

        // The variants are distinct grabs, or the per-variant counts in
        // `grab_outcome` count the same grab twice.
        let mut bits: Vec<u16> = variants.iter().map(|(_, bits)| *bits).collect();
        let before = bits.len();
        bits.sort_unstable();
        bits.dedup();
        assert_eq!(bits.len(), before, "lock variants must be distinct grabs");
    }

    #[test]
    fn the_configured_modifiers_are_the_masks_that_are_grabbed() {
        // The defect this pins: the masks were built from a hardcoded
        // `ShiftMask`, so a hotkey configured as `Control+Alt+F5` was grabbed
        // under Shift. The server delivers a `KeyPress` only for the mask the
        // grab was made with, so the key did nothing at all while the log said
        // "captured".
        let hotkey = Hotkey::parse("Control+Alt+F5").expect("a non-default combination parses");
        let base = MOD_CONTROL | MOD_M1;
        assert_eq!(hotkey.modifiers, base);
        assert_eq!(hotkey.keysym, keysym_for("F5").unwrap());

        let masks: Vec<u16> = lock_variants(hotkey.modifiers)
            .into_iter()
            .map(|(_, bits)| bits)
            .collect();
        assert_eq!(
            masks,
            vec![
                base,
                base | MOD_LOCK,
                base | MOD_M2,
                base | MOD_LOCK | MOD_M2,
                base | MOD_M5,
                base | MOD_LOCK | MOD_M5,
                base | MOD_M2 | MOD_M5,
                base | MOD_LOCK | MOD_M2 | MOD_M5,
            ],
            "every mask is the configured combination plus a lock state"
        );

        // Not one of them may carry Shift: it is not in the hotkey, and a grab
        // under a modifier the user did not configure is a key that never fires.
        for bits in &masks {
            assert_eq!(
                bits & MOD_SHIFT,
                0,
                "the configured hotkey has no Shift ({bits:#06x})"
            );
            assert_eq!(
                bits & base,
                base,
                "the configured modifiers stay in ({bits:#06x})"
            );
        }
    }

    #[test]
    fn simultaneous_lock_states_are_grabbed() {
        // CapsLock and NumLock both on is one mask carrying both bits, and a user
        // who leaves both locks set is in it permanently. Grabbing only the
        // single-lock variants loses the key in exactly that state — the state
        // can be seen at press time, not chosen by the app.
        let base = MOD_SHIFT;
        let variants = lock_variants(base);
        assert_eq!(
            variants.len(),
            8,
            "three lock keys have 2^3 states, and each state is one delivered mask"
        );

        let mask_of = |wanted: &str| {
            variants
                .iter()
                .find(|(name, _)| *name == wanted)
                .unwrap_or_else(|| panic!("{wanted} is a state the key can be in"))
                .1
        };
        assert_eq!(mask_of("capslock+numlock"), base | MOD_LOCK | MOD_M2);
        assert_eq!(mask_of("capslock+scrolllock"), base | MOD_LOCK | MOD_M5);
        assert_eq!(mask_of("numlock+scrolllock"), base | MOD_M2 | MOD_M5);
        assert_eq!(
            mask_of("capslock+numlock+scrolllock"),
            base | MOD_LOCK | MOD_M2 | MOD_M5,
            "all three locks at once is a mask of its own"
        );

        // Every combination is present exactly once, over all eight subsets.
        let mut masks: Vec<u16> = variants.iter().map(|(_, bits)| *bits).collect();
        masks.sort_unstable();
        masks.dedup();
        assert_eq!(masks.len(), 8, "no combination is missing or duplicated");
    }

    #[test]
    fn the_lock_bits_are_the_ones_the_server_sets() {
        // NumLock is Mod2 and ScrollLock is Mod5 on the standard keymap. A wrong
        // bit is a grab under a mask the server never delivers — a key that works
        // only while every lock is off.
        let bits = |wanted: &str| {
            lock_variants(MOD_SHIFT)
                .into_iter()
                .find(|(name, _)| *name == wanted)
                .unwrap()
                .1
        };
        assert_eq!(bits("numlock"), MOD_SHIFT | MOD_M2);
        assert_eq!(bits("scrolllock"), MOD_SHIFT | MOD_M5);
        assert_eq!(bits("capslock"), MOD_SHIFT | MOD_LOCK);
    }

    #[test]
    fn a_lock_key_named_as_a_modifier_does_not_double_grab() {
        // `CapsLock+F9` is a legal spec, and `CapsLockMask` is one of the bits the
        // variants add — so four of the eight collapse onto the other four. The
        // collision is dropped rather than grabbed twice.
        let variants = lock_variants(MOD_LOCK);
        let masks: Vec<u16> = variants.iter().map(|(_, bits)| *bits).collect();
        assert_eq!(masks.len(), 4);
        assert_eq!(
            masks,
            vec![
                MOD_LOCK,
                MOD_LOCK | MOD_M2,
                MOD_LOCK | MOD_M5,
                MOD_LOCK | MOD_M2 | MOD_M5,
            ]
        );
        for (name, bits) in &variants {
            assert_eq!(
                bits & MOD_LOCK,
                MOD_LOCK,
                "{name} must keep the configured lock request"
            );
        }
    }

    // ---- grab outcomes ----

    #[test]
    fn every_mask_accepted_is_a_complete_grab() {
        // The only outcome that is a guaranteed way back.
        let status = grab_outcome(8, 0, 8);
        assert_eq!(status, GrabStatus::Grabbed);
        assert!(status.is_reliable_route());
        assert!(status.keeps_watcher());
        assert_eq!(status.message(), "Hotkey captured");
        assert_eq!(status.warning(), None, "nothing to warn about");
    }

    #[test]
    fn some_masks_accepted_is_a_partial_grab() {
        // The case the old rule reported as `Grabbed`: seven combinations of eight,
        // with another client owning the eighth. The key fires in the states that
        // were acquired, so the watcher must live — and it must NOT be treated as a
        // guaranteed way back, because whether it fires depends on the lock keys.
        let status = grab_outcome(7, 1, 8);
        assert_eq!(status, GrabStatus::Partial);
        assert!(
            status.keeps_watcher(),
            "presses in the acquired states arrive"
        );
        assert!(!status.is_reliable_route(), "not a guaranteed way back");

        let warning = status.warning().expect("a partial grab needs a warning");
        assert!(warning.contains("CapsLock"), "got {warning}");
        assert!(warning.contains("NumLock"), "got {warning}");
        assert!(warning.contains("ScrollLock"), "got {warning}");
        assert!(
            GrabStatus::Partial.message().contains("some lock states"),
            "the one line must not read as a complete capture: {}",
            GrabStatus::Partial.message()
        );
    }

    #[test]
    fn the_partial_warning_does_not_state_a_reason_the_status_cannot_know() {
        // `Partial` covers two different situations: combinations another client owns
        // (`BadAccess`) and variants the server refused for some other reason. The one
        // warning is drawn for both, so it must not claim ownership — the per-mask
        // reason is in the log, and only there.
        assert_eq!(
            grab_outcome(7, 1, 8),
            GrabStatus::Partial,
            "one busy combination"
        );
        assert_eq!(
            grab_outcome(7, 0, 8),
            GrabStatus::Partial,
            "one combination refused for a reason that is not `BadAccess`"
        );

        let warning = GrabStatus::Partial.warning().expect("a partial grab warns");
        assert!(
            warning.contains("lock-state"),
            "it says what is missing: {warning}"
        );
        assert!(
            warning.contains("may not work"),
            "and what that means for the user: {warning}"
        );
        for key in ["CapsLock", "NumLock", "ScrollLock"] {
            assert!(warning.contains(key), "and names {key}: {warning}");
        }
        assert!(warning.contains("log"), "and points at the log: {warning}");
        for claim in ["another application", "another client", "owns"] {
            assert!(
                !warning.contains(claim),
                "the status does not carry the cause, so the line must not say {claim:?}: \
                 {warning}"
            );
        }
    }

    #[test]
    fn a_partial_grab_by_a_refusal_that_is_not_busy_is_still_partial() {
        // One mask accepted, the rest refused for a reason other than `BadAccess`:
        // still some lock states missing, so still not complete. The old rule could
        // not tell this from a full grab either.
        let status = grab_outcome(1, 0, 8);
        assert_eq!(status, GrabStatus::Partial);
        assert!(status.keeps_watcher());
        assert!(!status.is_reliable_route());
    }

    #[test]
    fn a_refused_grab_is_never_reported_as_captured() {
        // Every unique mask refused by another client's grab: the key is somebody
        // else's, and none of it is ours to report as captured.
        assert_eq!(grab_outcome(0, 8, 8), GrabStatus::Busy);
        assert!(!grab_outcome(0, 8, 8).is_reliable_route());
        assert!(!GrabStatus::Busy.keeps_watcher(), "nothing to watch");
        assert!(!GrabStatus::Busy.is_reliable_route());

        // Busy on some masks with the rest refused for other reasons is still
        // Busy: no variant was acquired and a `BadAccess` conflict is why.
        assert_eq!(grab_outcome(0, 5, 8), GrabStatus::Busy);

        // Nothing accepted and nothing busy is a failure, not a capture.
        assert_eq!(grab_outcome(0, 0, 8), GrabStatus::Failed);
        assert!(!GrabStatus::Failed.is_reliable_route());
        assert!(!GrabStatus::Failed.keeps_watcher());

        // Nothing was even attempted.
        assert_eq!(grab_outcome(0, 0, 0), GrabStatus::Failed);
    }

    #[test]
    fn the_outcome_counts_the_unique_masks_not_a_constant() {
        // `CapsLock+F9` asks for a lock bit the variant table already adds, so only
        // four unique masks exist. Measuring the result against eight would report a
        // complete grab as partial forever — a warning the user can never clear.
        let variants = lock_variants(MOD_SHIFT | MOD_LOCK);
        assert_eq!(variants.len(), 4);

        assert_eq!(grab_outcome(4, 0, variants.len()), GrabStatus::Grabbed);
        assert_eq!(grab_outcome(3, 1, variants.len()), GrabStatus::Partial);
        assert_eq!(grab_outcome(4, 0, 8), GrabStatus::Partial);

        // And the eight-mask case is unaffected by the same arithmetic.
        assert_eq!(
            grab_outcome(8, 0, lock_variants(MOD_SHIFT).len()),
            GrabStatus::Grabbed
        );
    }

    #[test]
    fn the_watcher_and_the_safety_rule_read_different_predicates() {
        // One table per status: which of the two questions each answer gives. The
        // distinction is the whole point of `Partial` — a single predicate would
        // either kill a working key or hide the panel behind one that may not fire.
        let cases = [
            (GrabStatus::Unknown, false, false),
            (GrabStatus::Grabbed, true, true),
            (GrabStatus::Partial, true, false),
            (GrabStatus::Disabled, false, false),
            (GrabStatus::Busy, false, false),
            (GrabStatus::NoDisplay, false, false),
            (GrabStatus::Failed, false, false),
        ];
        for (status, watch, reliable) in cases {
            assert_eq!(status.keeps_watcher(), watch, "{status:?} keeps_watcher");
            assert_eq!(status.is_reliable_route(), reliable, "{status:?} reliable");
        }
    }

    #[test]
    fn no_x_display_is_a_reported_state_not_a_capture() {
        // Native Wayland, or no DISPLAY: no global hotkey is claimed, and the
        // message names the routes that do work there.
        assert!(!GrabStatus::NoDisplay.is_reliable_route());
        assert!(!GrabStatus::NoDisplay.keeps_watcher());
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
        assert!(!visibility.hotkey_is_reliable_route());
        assert!(!visibility.has_return_path(false));
    }

    #[test]
    fn the_busy_message_says_the_key_belongs_to_someone_else() {
        assert_eq!(
            GrabStatus::Busy.message(),
            "Hotkey is taken by another application"
        );
        assert_eq!(GrabStatus::Grabbed.message(), "Hotkey captured");
        assert_eq!(
            GrabStatus::Partial.message(),
            "Hotkey captured for some lock states only"
        );
        assert_eq!(GrabStatus::Failed.message(), "Could not capture the hotkey");
        assert_eq!(GrabStatus::Unknown.message(), "Hotkey not checked yet");
        assert!(!GrabStatus::Disabled.is_reliable_route());

        // Every line the settings surface can show is non-empty for every state,
        // so no status can render as a blank explanation.
        for status in [
            GrabStatus::Unknown,
            GrabStatus::Grabbed,
            GrabStatus::Partial,
            GrabStatus::Disabled,
            GrabStatus::Busy,
            GrabStatus::NoDisplay,
            GrabStatus::Failed,
        ] {
            assert!(!status.message().is_empty(), "{status:?} needs a line");
            if let Some(warning) = status.warning() {
                assert!(!warning.is_empty(), "{status:?} needs a real warning");
            }
        }
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
        assert!(!visibility.hotkey_is_reliable_route());
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

        // Partial is the new row, and the one that has to keep its watcher: the
        // key works in some lock states, so it is not nothing — but it is not the
        // route the panel may be left hidden behind either.
        visibility.set_grab_status(GrabStatus::Partial);
        assert!(
            !visibility.has_return_path(false),
            "a partially captured key is not a reliable way back"
        );
        assert!(
            !visibility.hotkey_is_reliable_route(),
            "and the safety input says so"
        );
        assert!(
            GrabStatus::Partial.keeps_watcher(),
            "while the watcher still runs for it"
        );
    }

    #[test]
    fn the_hidden_line_names_the_routes_that_actually_work() {
        // The log is read by whoever is debugging "the panel is gone": it must not
        // name a key that only fires in some lock states.
        let complete = hidden_log_line(GrabStatus::Grabbed);
        assert!(complete.contains("hotkey"), "{complete}");

        for status in [
            GrabStatus::Unknown,
            GrabStatus::Partial,
            GrabStatus::Disabled,
            GrabStatus::Busy,
            GrabStatus::NoDisplay,
            GrabStatus::Failed,
        ] {
            let line = hidden_log_line(status);
            assert!(
                !line.contains("the hotkey, the tray"),
                "{status:?} must not promise the hotkey: {line}"
            );
            assert!(line.contains("--toggle-panel"), "{status:?}: {line}");
        }
        assert!(
            hidden_log_line(GrabStatus::Partial).contains("not a reliable route"),
            "a partial key is the case this line exists for"
        );
    }

    #[test]
    fn a_partial_key_does_not_block_hiding_when_another_route_exists() {
        // The rule the brief states explicitly: partial success must not make the
        // panel unreachable. Hiding stays allowed as soon as the tray or the CLI
        // can bring it back, and equal-weight with a complete grab in that case.
        let partial = Visibility::new();
        partial.set_grab_status(GrabStatus::Partial);
        assert_eq!(
            hide_outcome(
                partial.hotkey_is_reliable_route(),
                false,
                true // D-Bus exported, which is how the GUI runs in practice
            ),
            HideOutcome::Hide,
            "the D-Bus method is the route that covers a partial hotkey"
        );
        assert!(partial.has_return_path(true), "the tray does the same");
        assert_eq!(
            hide_outcome(partial.hotkey_is_reliable_route(), false, false),
            HideOutcome::ReturnToNormal,
            "with nothing else, there is no reliable way back and hiding is refused"
        );

        // The same environment with a complete grab hides, so the only difference
        // is the status itself.
        let complete = Visibility::new();
        complete.set_grab_status(GrabStatus::Grabbed);
        assert_eq!(
            hide_outcome(complete.hotkey_is_reliable_route(), false, false),
            HideOutcome::Hide
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
