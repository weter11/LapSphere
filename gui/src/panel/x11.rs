//! X11 EWMH backend for panel mode, plus the panel's draw call.
//!
//! Split from `window.rs` (which is the pure, testable policy) because this is
//! the part that needs a display connection. Everything here is
//! `#[cfg(target_os = "linux")]`, and each public entry point degrades to a
//! no-op that logs once rather than failing: the panel must still draw on a
//! system where the atom protocol is unavailable (Wayland has no
//! `_NET_WM_STATE` a client may send at all — ADR-3).
//!
//! The sequencing the design doc measured, implemented as measured:
//!
//! ```text
//! frame 0      request WindowLevel(AlwaysOnTop)   (runtime only; the
//!                                                 ViewportBuilder hint is
//!                                                 ignored by xfwm4)
//! window id known + WM has taken the window
//!              send _NET_WM_STATE ClientMessage (l[3]=0)
//!              read _NET_WM_STATE back from the X server
//!              absent -> retry, up to AtomRetry::INITIAL_ATTEMPTS
//! present       done
//! ```

#[cfg(target_os = "linux")]
use x11rb::connection::Connection;
#[cfg(target_os = "linux")]
use x11rb::protocol::xproto::ConnectionExt;

use super::units::NO_DATA;
use super::window::{state_client_message, AtomAction, AtomRetry, AtomSet};
use crate::app::AppState;
use crate::panel::config::PanelConfig;
use crate::panel::items;
use crate::panel::render;
use crate::panel::units::Value;

/// How long to wait between atom retries.
pub const RETRY_INTERVAL_MS: u64 = 50;

/// The EWMH atoms the panel sets, resolved once per connection.
///
/// Four atoms, because "always on top" on xfwm4 is two mechanisms and they do
/// different jobs: `ABOVE` is the stacking level, `STICKY` keeps the window on
/// every workspace, `SKIP_TASKBAR` and `SKIP_PAGER` remove it from the two lists
/// an overlay has no business appearing in.
#[cfg(target_os = "linux")]
pub struct PanelAtoms {
    pub net_wm_state: u32,
    pub above: u32,
    pub sticky: u32,
    pub skip_taskbar: u32,
    pub skip_pager: u32,
}

#[cfg(target_os = "linux")]
impl PanelAtoms {
    /// The atom value for an index as used by `window::AtomSet`.
    ///
    /// The index order is the one `AtomSet::additions` / `removals` produce, so
    /// the two halves cannot drift apart silently: a wrong index shows up as a
    /// read-back that never matches, not as a plausible-looking no-op.
    pub fn by_index(&self, index: usize) -> u32 {
        match index {
            0 => self.above,
            1 => self.sticky,
            2 => self.skip_taskbar,
            3 => self.skip_pager,
            other => panic!("atom index {other} is out of range"),
        }
    }
}

/// The root window id of the given screen.
pub fn root_window(
    conn: &x11rb::rust_connection::RustConnection,
    screen_num: usize,
) -> anyhow::Result<u32> {
    use x11rb::connection::Connection;
    let setup = conn.setup();
    let screen = setup
        .roots
        .get(screen_num)
        .ok_or_else(|| anyhow::anyhow!("screen {screen_num} does not exist"))?;
    Ok(screen.root)
}

/// Resolve the atoms, interning them on the server.
pub fn resolve_atoms(conn: &x11rb::rust_connection::RustConnection) -> anyhow::Result<PanelAtoms> {
    let net_wm_state = conn.intern_atom(false, b"_NET_WM_STATE")?.reply()?.atom;
    let above = conn
        .intern_atom(false, b"_NET_WM_STATE_ABOVE")?
        .reply()?
        .atom;
    let sticky = conn
        .intern_atom(false, b"_NET_WM_STATE_STICKY")?
        .reply()?
        .atom;
    let skip_taskbar = conn
        .intern_atom(false, b"_NET_WM_STATE_SKIP_TASKBAR")?
        .reply()?
        .atom;
    let skip_pager = conn
        .intern_atom(false, b"_NET_WM_STATE_SKIP_PAGER")?
        .reply()?
        .atom;
    Ok(PanelAtoms {
        net_wm_state,
        above,
        sticky,
        skip_taskbar,
        skip_pager,
    })
}

/// Send the `_NET_WM_STATE` ClientMessage for `action` on one atom.
///
/// `data.l[3] = 0` per the owner's instruction; the design doc records that
/// xfwm4 also accepts 1 and that the earlier "must be 1" claim came from a
/// confounded experiment.
pub fn send_state_message(
    conn: &x11rb::rust_connection::RustConnection,
    root: u32,
    window: u32,
    net_wm_state: u32,
    atom: u32,
    action: AtomAction,
) -> anyhow::Result<()> {
    use x11rb::protocol::xproto::*;

    let event =
        ClientMessageEvent::new(32, window, net_wm_state, state_client_message(action, atom));

    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
        event,
    )?;
    conn.flush()?;
    Ok(())
}

/// Read `_NET_WM_STATE` back and report which of the panel's atoms are set.
///
/// This read-back is what makes the send self-confirming: a WM that ignores the
/// request shows up as "atoms absent after N attempts" rather than as a silent
/// failure, which is the failure mode the design doc says this design is built to
/// detect. It is also how a switch to normal stacking is verified — the removal
/// counts as done only when the atoms are actually gone.
pub fn atoms_present(
    conn: &x11rb::rust_connection::RustConnection,
    window: u32,
    atoms: &PanelAtoms,
) -> anyhow::Result<AtomSet> {
    use x11rb::protocol::xproto::*;

    let reply = conn
        .get_property(false, window, atoms.net_wm_state, AtomEnum::ATOM, 0, 32)?
        .reply()?;

    // In x11rb 0.13 `value` is a plain `Vec<u8>` field, so the ATOM array it
    // carries is decoded here rather than by a typed accessor.
    let value_slice: Vec<u32> = reply
        .value
        .chunks_exact(4)
        .map(|chunk| u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect();

    let has = |atom: u32| value_slice.contains(&atom);
    Ok(AtomSet {
        above: has(atoms.above),
        sticky: has(atoms.sticky),
        skip_taskbar: has(atoms.skip_taskbar),
        skip_pager: has(atoms.skip_pager),
    })
}

/// Drive the window's atoms to `wanted`, confirming by read-back and retrying.
///
/// Works by diff rather than by a blind Add or Remove: a stacking switch on a
/// live window has to both add and remove, and sending a blanket "remove all four"
/// to a window that never had them is a wasted round trip per atom per frame.
///
/// Returns true once the read-back equals `wanted`. Returns false if the attempts
/// ran out — a legitimate outcome on a WM that does not implement the request,
/// logged rather than treated as an error.
#[cfg(target_os = "linux")]
pub fn apply_panel_atoms(
    conn: &x11rb::rust_connection::RustConnection,
    root: u32,
    window: u32,
    atoms: &PanelAtoms,
    wanted: AtomSet,
) -> bool {
    let mut retry = AtomRetry::new();

    while retry.next_attempt() {
        let actual = match atoms_present(conn, window, atoms) {
            Ok(actual) => actual,
            Err(err) => {
                log::warn!("panel: could not read _NET_WM_STATE back: {err}");
                return false;
            }
        };
        if retry.is_satisfied(wanted, actual) {
            return true;
        }

        let additions = wanted.additions(actual);
        let removals = wanted.removals(actual);

        for (index, action) in additions
            .into_iter()
            .map(|index| (index, AtomAction::Add))
            .chain(
                removals
                    .into_iter()
                    .map(|index| (index, AtomAction::Remove)),
            )
        {
            if let Err(err) = send_state_message(
                conn,
                root,
                window,
                atoms.net_wm_state,
                atoms.by_index(index),
                action,
            ) {
                log::warn!("panel: could not send _NET_WM_STATE: {err}");
                return false;
            }
        }

        // Nothing to change and the state still does not match means the WM is
        // holding an atom we did not ask about, or refusing; sleeping is the
        // only thing left to try.
        std::thread::sleep(std::time::Duration::from_millis(RETRY_INTERVAL_MS));
    }

    log::warn!(
        "panel: _NET_WM_STATE not applied after {} attempts; the window manager ignored the request",
        AtomRetry::INITIAL_ATTEMPTS
    );
    false
}

/// Draw the panel strip into the given `ui` and return its response.
///
/// The response is returned rather than dropped because it is the panel's ONLY
/// interactive surface: the secondary click opens the context menu and the
/// primary click starts the window drag. The rect is allocated with
/// `Sense::click`, which covers both buttons — the previous `Sense::hover` made
/// the panel a picture.
///
/// Sense::click also stops the strip from swallowing drags aimed at the window
/// frame, which matters now that the panel is borderless: without a click sense
/// the WM drag would only start from a few pixels of edge.
pub fn draw_strip(ui: &mut egui::Ui, state: &AppState, config: &PanelConfig) -> egui::Response {
    let rows = render::rows(config);
    let layout = render::layout(config);

    egui::Frame::NONE
        .fill(egui::Color32::from_black_alpha(140))
        .inner_margin(6.0)
        .show(ui, |ui| {
            // Allocate the computed layout size rather than letting the
            // contents decide: the point of `layout` is that the window is this
            // size regardless of what the values turned out to be.
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(layout.width, layout.height),
                egui::Sense::click(),
            );
            let painter = ui.painter();
            painter.rect_filled(rect, 0.0, egui::Color32::from_black_alpha(140));
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect)
                    .layout(egui::Layout::top_down(egui::Align::Center)),
            );
            draw_contents(&mut child, state, config, &rows);
            response
        })
        .inner
}

fn draw_contents(
    ui: &mut egui::Ui,
    state: &AppState,
    _config: &PanelConfig,
    rows: &[render::Row<'_>],
) {
    ui.horizontal_centered(|ui| {
        for row in rows {
            let scale = row.scale;
            let label = row
                .config
                .label
                .clone()
                .unwrap_or_else(|| row.item.default_label.to_string());
            let value = (row.item.read)(state);

            // The declared slot, so the layout cannot depend on the value.
            let slot_width = row.item.slot.width * scale;
            let slot_height = row.item.slot.height * scale;

            ui.allocate_ui(egui::vec2(slot_width, slot_height), |ui| {
                ui.horizontal(|ui| {
                    let text_style = egui::TextStyle::Small;
                    let base = ui
                        .style()
                        .text_styles
                        .get(&text_style)
                        .map(|s| s.size)
                        .unwrap_or(12.0);
                    let font = egui::FontId::monospace(base * scale);

                    if !label.is_empty() {
                        ui.label(egui::RichText::new(label).font(font.clone()).strong());
                    }
                    ui.label(egui::RichText::new(display(&value)).font(font));
                });
            });
        }
    });
}

/// The string an element contributes. A graph shows nothing; absent shows the
/// em dash.
fn display(value: &Value) -> String {
    match value {
        Value::Graph => String::new(),
        other => other.text(),
    }
}

/// The string the menu shows for an element, including the reserved-slot
/// marker, so a user can tell a `—` from a broken element.
pub fn menu_value(state: &AppState, id: &str) -> String {
    match items::find(id) {
        Some(item) => {
            let value = (item.read)(state);
            match value {
                Value::Graph => "—".to_string(),
                other => other.text(),
            }
        }
        None => NO_DATA.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::units;

    #[test]
    fn an_absent_value_displays_as_the_em_dash() {
        assert_eq!(display(&Value::Absent), NO_DATA);
    }

    #[test]
    fn a_graph_displays_as_nothing() {
        assert_eq!(display(&Value::Graph), "");
    }

    #[test]
    fn a_value_displays_its_text() {
        assert_eq!(display(&Value::Text("5.80 GHz".into())), "5.80 GHz");
    }

    #[test]
    fn the_menu_reports_the_em_dash_for_an_empty_state() {
        let state = AppState::new();
        for id in ["cpu_load", "ram_type", "gamepad", "fps"] {
            assert_eq!(
                menu_value(&state, id),
                NO_DATA,
                "{id} should read as absent with no data"
            );
        }
    }

    #[test]
    fn the_menu_reports_a_graph_as_a_reserved_slot() {
        let state = AppState::new();
        for id in ["fps_graph", "frametime_graph"] {
            assert_eq!(
                menu_value(&state, id),
                NO_DATA,
                "{id} is a reserved slot with no producer yet"
            );
        }
    }

    #[test]
    fn the_menu_never_panics_on_an_unknown_id() {
        let state = AppState::new();
        assert_eq!(menu_value(&state, "not_an_element"), NO_DATA);
    }

    #[test]
    fn the_retry_interval_is_short_enough_to_be_invisible() {
        // 20 attempts x 50 ms is about one second of trying before giving up:
        // long enough to outlast a slow WM, short enough that a failing WM does
        // not delay the panel appearing.
        let total = RETRY_INTERVAL_MS * AtomRetry::INITIAL_ATTEMPTS as u64;
        assert!(total <= 2000, "retry loop would take {total} ms");
        assert!(
            total >= 500,
            "retry loop gives up too eagerly at {total} ms"
        );
    }

    #[test]
    fn every_element_has_a_menu_value_without_a_display() {
        let state = AppState::new();
        for item in items::PANEL_ITEMS {
            let shown = menu_value(&state, item.id);
            assert!(
                !shown.is_empty(),
                "{} produced an empty menu value",
                item.id
            );
        }
    }
}
