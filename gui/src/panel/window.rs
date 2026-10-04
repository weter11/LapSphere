//! Switching the one window between Normal and Panel mode.
//!
//! # One window, two modes
//!
//! The app has always had one window; the panel does not add a second. Mode is
//! therefore not "which window is visible" but "what does this window look like
//! right now", and a switch is a batch of `ViewportCommand`s. That keeps one GL
//! surface, one window to decorate, and one tray icon.
//!
//! # The first-frame problem
//!
//! A process started with `--panel` sets `mode = Panel` directly in `new()`:
////! there is no transition, so nothing ever writes `pending_mode`. An
//! `apply_mode` that begins with `pending_mode.take()?` therefore returns on
//! every frame including the first, and the panel renders in the normal window's
//! 570x620 geometry forever — the bug commit `8057834` on the archived branch
//! fixed. The decision is therefore a pure function,
//! [`mode_to_apply`](mode_to_apply), taking the current mode, the pending
//! request and whether the spec has been sent, so the "same mode, spec not
//! applied yet" case is reachable and testable.
//!
//! # Applying the spec
//!
//! Only three things change between the modes in this PR: the inner size, the
//! minimum size and whether the window is resizable. Stacking and the taskbar
//! atoms are PR 2's work; they are configured but not claimed here.
//!
//! The size sent is [`layout::panel_size`] — computed from the visible row count
//! and the font row height, never measured — so there is no measure/resize/
//! measure loop. The width is the configured width, so moving the width slider
//! resizes the window on the very next frame.

use super::config::PanelConfig;
use super::layout::{self, Size};

/// Which surface the app is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The ordinary LapSphere window.
    Normal,
    /// The compact vertical panel.
    Panel,
}

impl Mode {
    pub fn is_panel(self) -> bool {
        matches!(self, Mode::Panel)
    }
}

/// The normal window's inner size, matching `main.rs`'s `ViewportBuilder`.
pub const NORMAL_INNER: [f32; 2] = [570.0, 620.0];
/// `main.rs`'s minimum inner size for the normal window.
pub const NORMAL_MIN_INNER: [f32; 2] = [440.0, 470.0];

/// What the window must be told for a given mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowSpec {
    pub inner: [f32; 2],
    pub min_inner: [f32; 2],
    /// A panel is exactly its computed size; letting the user resize it would
    /// desynchronise the window from the computed layout.
    pub resizable: bool,
    pub decorations: bool,
}

/// Decide whether this frame must send the window spec, and for which mode.
///
/// Returns the mode to apply, or `None` when the frame has nothing to do.
///
/// The case this exists for: a process started in panel mode (`--panel`) sets
/// `mode` DIRECTLY, so there is no transition and nothing writes `pending_mode`.
/// Reading `pending_mode` first and returning on `None` would make the "same
/// mode, spec never sent" branch unreachable forever.
pub fn mode_to_apply(mode: Mode, pending: Option<Mode>, spec_applied: bool) -> Option<Mode> {
    match pending {
        Some(target) => Some(target),
        // Nothing pending, but the spec has never been sent: the process started
        // in this mode and this frame is where it must be applied.
        None if !spec_applied => Some(mode),
        None => None,
    }
}

/// The panel's window spec, computed from the config and the font metrics.
pub fn panel_spec(config: &PanelConfig, font_row_height: f32) -> WindowSpec {
    let row_count = super::render::visible_row_count(config);
    let size: Size = layout::panel_size(config, row_count, font_row_height);

    WindowSpec {
        inner: [size.width, size.height],
        // A minimum equal to the inner size makes the panel non-resizable in
        // effect even on a WM that honours the hint loosely.
        min_inner: [size.width, size.height],
        resizable: false,
        decorations: false,
    }
}

/// The normal-mode spec, restoring the geometry captured before the first switch
/// so the user gets their own window size back rather than a hard-coded default.
pub fn normal_spec(previous: Option<[f32; 2]>) -> WindowSpec {
    let inner = previous.unwrap_or(NORMAL_INNER);
    WindowSpec {
        inner,
        min_inner: NORMAL_MIN_INNER,
        resizable: true,
        decorations: true,
    }
}

/// Send `spec` to the window.
///
/// Split from the decision so the sequencing is testable without a display, and
/// so the caller owns the `spec_applied` bookkeeping in one place.
///
/// **The minimum size must be sent BEFORE the inner size**, and that order is
/// load-bearing rather than cosmetic. `main.rs` creates the window with
/// `with_min_inner_size(440x470)`; the WM clamps an `InnerSize` request to the
/// minimum it currently holds. Sending `InnerSize(300x214)` first is therefore
/// clamped straight back up to 440x470 and the panel never shrinks — which is
/// exactly what a first attempt did. Lowering the minimum first lets the size
/// through.
pub fn apply_spec(ctx: &egui::Context, spec: WindowSpec) {
    let inner = egui::vec2(spec.inner[0], spec.inner[1]);
    let min_inner = egui::vec2(spec.min_inner[0], spec.min_inner[1]);
    ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(min_inner));
    ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(spec.resizable));
    ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(spec.decorations));
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(inner));
}

/// Which coordinator components a config's visible rows need polled.
///
/// The panel is a much cheaper surface than the main window, and part of keeping
/// it that way is not polling what it does not draw: `logs` alone is a ~470 kB
/// reply every tick. A row whose component is not polled would render `—`, so
/// this set must cover every visible row.
pub fn required_components(config: &PanelConfig) -> Vec<&'static str> {
    let mut needed: Vec<&'static str> = Vec::new();
    for id in super::config::visible_row_ids(config) {
        let Some(component) = component_for(id) else {
            continue;
        };
        if !needed.contains(&component) {
            needed.push(component);
        }
    }
    needed
}

/// The coordinator component a row reads its data from.
///
/// `None` for a row fed by data that is not polled (none today: every row reads
/// a polled field, and a row with a non-polled source must not silently add a
/// component to the set).
fn component_for(id: &str) -> Option<&'static str> {
    match id {
        id if id.starts_with("cpu_") => Some("cpu"),
        id if id.starts_with("gpu_") => Some("gpu"),
        id if id.starts_with("ram_") => Some("memory"),
        id if id.starts_with("gamepad_") => Some("gamepads"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::config::{PanelConfig, RowConfig};
    use crate::panel::rows;

    fn config_with(ids: &[&str]) -> PanelConfig {
        PanelConfig {
            rows: rows::all_ids()
                .into_iter()
                .map(|id| RowConfig {
                    id: id.to_string(),
                    visible: ids.contains(&id),
                })
                .collect(),
            ..PanelConfig::fresh()
        }
    }

    // ---- the first-frame bug this function exists for ----

    #[test]
    fn a_process_started_in_panel_applies_the_spec_on_the_first_frame() {
        // Exactly the state `new()` leaves behind for `--panel`: the mode is set
        // directly and nothing is pending.
        assert_eq!(
            mode_to_apply(Mode::Panel, None, false),
            Some(Mode::Panel),
            "the first frame must apply the panel spec, not skip it"
        );
    }

    #[test]
    fn the_spec_is_applied_once_and_then_the_frame_is_a_no_op() {
        assert_eq!(mode_to_apply(Mode::Panel, None, false), Some(Mode::Panel));
        assert_eq!(mode_to_apply(Mode::Panel, None, true), None);
        assert_eq!(mode_to_apply(Mode::Normal, None, true), None);
    }

    #[test]
    fn a_pending_request_wins_even_when_the_spec_was_already_applied() {
        // Returning to normal after the panel spec was sent must still act.
        assert_eq!(
            mode_to_apply(Mode::Panel, Some(Mode::Normal), true),
            Some(Mode::Normal)
        );
        // And re-entering panel mode must re-apply, not be treated as done.
        assert_eq!(
            mode_to_apply(Mode::Normal, Some(Mode::Panel), true),
            Some(Mode::Panel)
        );
        // Even when it is the same mode the spec was last sent for: the user may
        // have resized the normal window since, and the panel size is computed
        // fresh each switch.
        assert_eq!(
            mode_to_apply(Mode::Panel, Some(Mode::Panel), true),
            Some(Mode::Panel)
        );
    }

    // ---- sizes ----

    #[test]
    fn the_panel_spec_matches_the_computed_layout_size() {
        let config = config_with(&["cpu_processor", "cpu_average_load", "ram_usage"]);
        let spec = panel_spec(&config, 14.0);
        let expected = layout::panel_size(&config, 3, 14.0);

        assert_eq!(spec.inner[0], expected.width);
        assert_eq!(spec.inner[1], expected.height);
        assert_eq!(spec.min_inner, spec.inner, "a panel is exactly its size");
        assert!(!spec.resizable);
        assert!(!spec.decorations);
    }

    #[test]
    fn the_panel_width_follows_the_config_so_the_slider_applies_at_once() {
        let mut config = config_with(&["ram_usage"]);
        config.width = 420.0;
        assert_eq!(panel_spec(&config, 14.0).inner[0], 420.0);

        config.width = 240.0;
        assert_eq!(
            panel_spec(&config, 14.0).inner[0],
            240.0,
            "the width must be read fresh each time, not cached"
        );
    }

    #[test]
    fn switching_rows_changes_the_height_by_exactly_one_row_each() {
        let one = config_with(&["ram_usage"]);
        let two = config_with(&["ram_usage", "ram_used"]);
        let three = config_with(&["ram_usage", "ram_used", "ram_total"]);

        let h1 = panel_spec(&one, 14.0).inner[1];
        let h2 = panel_spec(&two, 14.0).inner[1];
        let h3 = panel_spec(&three, 14.0).inner[1];

        let row = layout::row_height(14.0);
        assert!((h2 - h1 - row).abs() < 1e-4, "{h2} - {h1} != {row}");
        assert!((h3 - h2 - row).abs() < 1e-4, "{h3} - {h2} != {row}");
    }

    #[test]
    fn a_panel_with_no_visible_rows_is_still_a_valid_window() {
        let spec = panel_spec(&config_with(&[]), 14.0);
        assert!(spec.inner[1] > 0.0, "must not collapse to zero height");
        assert!(spec.inner[0] > 0.0, "must not collapse to zero width");
    }

    #[test]
    fn the_normal_spec_restores_the_geometry_the_user_had() {
        let spec = normal_spec(Some([800.0, 900.0]));
        assert_eq!(spec.inner, [800.0, 900.0]);
        assert_eq!(spec.min_inner, NORMAL_MIN_INNER);
        assert!(spec.resizable);
        assert!(spec.decorations);

        // And falls back to the compiled-in default when nothing was captured.
        assert_eq!(normal_spec(None).inner, NORMAL_INNER);
    }

    // ---- the poll set ----

    #[test]
    fn the_poll_set_covers_every_visible_row_and_nothing_else() {
        let config = config_with(&["cpu_processor", "gpu_load", "ram_used", "gamepad_battery"]);
        let mut needed = required_components(&config);
        needed.sort_unstable();
        assert_eq!(needed, vec!["cpu", "gamepads", "gpu", "memory"]);
    }

    #[test]
    fn no_visible_rows_means_nothing_is_polled() {
        assert!(required_components(&config_with(&[])).is_empty());
    }

    #[test]
    fn the_expensive_log_ring_is_never_in_a_panel_poll_set() {
        // `logs` is a ~470 kB reply per tick and no panel row reads it.
        for ids in [
            vec!["cpu_processor"],
            vec!["ram_usage"],
            crate::panel::rows::all_ids(),
        ] {
            let config = config_with(&ids);
            let needed = required_components(&config);
            assert!(
                !needed.contains(&"logs"),
                "logs must not be polled for {ids:?}"
            );
        }
    }

    #[test]
    fn an_unknown_row_id_contributes_no_component() {
        // `normalize` drops these, but `required_components` must not panic on
        // one if it is ever called before normalization.
        assert_eq!(component_for("not_a_row"), None);
    }
}
