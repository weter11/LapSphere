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
//! there is no transition, so nothing ever writes `pending_mode`. An
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
//! Five things change between the modes: the inner size, the minimum size,
//! whether the window is resizable, whether it is decorated, and how it stacks
//! ([`WindowSpec::level`]). Panel mode also carries a position
//! ([`WindowSpec::position`]): the panel is undecorated, so dragging it is the
//! only way to move it, and where it ended up is restored on the next start.
//!
//! The level is set HERE, on a frame where the window already exists, rather than
//! with an initial `ViewportBuilder::with_window_level` hint: measured on xfwm4
//! 4.20, the builder hint is silently ignored and the panel comes up underneath a
//! fullscreen game, while the same level sent at runtime holds
//! (`docs/development/panel-overlay-tests.md`, B2 and B9 in PR #111). The taskbar
//! atoms are a separate, EWMH-specific path — see [`super::ewmh`].
//!
//! The size sent is [`layout::panel_size`] — computed from the visible row count
//! and the font row height, never measured — so there is no measure/resize/
//! measure loop. The width is the configured width, so moving the width slider
//! resizes the window on the very next frame.
//!
//! # Position: request, then read back
//!
//! `OuterPosition` is a request, not a result. A window manager may clamp it,
//! apply it late, or place the window itself, and nothing about sending it proves
//! the window moved. So the position that gets *persisted* is never the one that
//! was asked for: [`PositionWatch`] follows the position the window reports back
//! (`ViewportInfo::outer_rect`) and offers it for saving only once it has held
//! still — which is also what keeps a drag from writing the file on every frame
//! of the gesture.
//!
//! The clamp ([`clamp_to_monitor`]) keeps a stored corner from putting the panel
//! where it cannot be grabbed back from. It is applied on the way IN only
//! (restore): what is saved is what the window is actually at.
//!
//! # Transparency: the surface, and the half that is easy to omit
//!
//! The root window is created with `with_transparent(true)` (`main.rs`). That is
//! what obtains an ARGB visual on X11 and an alpha buffer format on Wayland, and
//! it is UNCONDITIONAL: winit picks the visual while creating the window and a
//! visual cannot be swapped on a live window, so a request made only when the
//! user asks for the panel could never be honoured.
//!
//! The two modes therefore differ by what they PAINT, not by what visual the
//! window holds:
//!
//! * [`clear_color`] — opaque in normal mode, so the ordinary window stays
//!   opaque, and transparent black in panel mode.
//! * [`background_fill`] — the panel's own background: `visuals.panel_fill` at
//!   the configured opacity.
//!
//! A translucent FILL over an opaque CLEAR is still an opaque window: the
//! composited alpha is `src + dst * (1 - src.a)`, so an opaque `dst` keeps it at
//! 1 whatever is painted over it. The clear is the half that decides whether the
//! window is see-through at all, and it is the half that is easy to omit.
//!
//! Only the background is translucent. Every row's text is painted with an opaque
//! colour on top of it, so text and values stay readable at any opacity.

use std::time::{Duration, Instant};

use super::config::{
    PanelConfig, PanelPosition, MAX_BACKGROUND_OPACITY, MIN_BACKGROUND_OPACITY,
};
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
    /// How the window stacks against other windows.
    ///
    /// A RUNTIME request, and it has to be one: an initial `ViewportBuilder`
    /// `with_window_level` hint is silently ignored by xfwm4 4.20, which leaves
    /// the panel underneath a fullscreen game while every calculation above is
    /// correct. The level therefore travels as a `ViewportCommand` on the frame
    /// the mode is applied — after the window exists — and never as a builder
    /// attribute.
    pub level: egui::WindowLevel,
    /// Where the window's outer top-left corner must go, in points, or `None`
    /// to leave placement to the window manager.
    ///
    /// Only panel mode sets this. Normal mode leaves it `None`: the panel and the
    /// normal window are the SAME window, so a mode switch does not re-place it
    /// and this code makes no claim about a normal window's position.
    pub position: Option<[f32; 2]>,
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
///
/// `monitor` is the current monitor's size in points, or `None` when the platform
/// does not report one. It is what the stored position is clamped against; see
/// [`restore_position`] for why an unknown monitor means "no position request".
pub fn panel_spec(
    config: &PanelConfig,
    font_row_height: f32,
    monitor: Option<[f32; 2]>,
) -> WindowSpec {
    let row_count = super::render::visible_row_count(config);
    let size: Size = layout::panel_size(config, row_count, font_row_height);

    WindowSpec {
        inner: [size.width, size.height],
        // A minimum equal to the inner size makes the panel non-resizable in
        // effect even on a WM that honours the hint loosely.
        min_inner: [size.width, size.height],
        resizable: false,
        decorations: false,
        level: config.stacking.window_level(),
        position: restore_position(config.position, [size.width, size.height], monitor),
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
        // An ordinary window, explicitly: the panel and this window are the same
        // X11 window, so the level has to be cleared rather than merely no longer
        // requested. Living here and not in a mode check is what makes "the
        // normal window never inherits the panel's stacking" a property of the
        // spec rather than of the order the code happens to run in.
        level: egui::WindowLevel::Normal,
        // The normal window is placed by the window manager, as it always was.
        position: None,
    }
}

/// The colour the window's surface is cleared to before anything is drawn.
///
/// Premultiplied, gamma-space RGBA — the form `eframe::App::clear_color` hands to
/// the renderer (see `Color32::to_normalized_gamma_f32`), and the form the X11
/// ARGB and Wayland alpha buffers expect.
///
/// **Normal mode is opaque.** The colour is eframe's own default clear with its
/// alpha forced to 1.0, so the ordinary window looks exactly as it did before the
/// root window gained an alpha channel: the channel is present and never used.
/// Inheriting eframe's default instead would make the ordinary window 30 %
/// see-through the moment the visual changed, because that default carries an
/// alpha of 180.
///
/// **Panel mode clears to transparent black**, so the clear contributes no colour
/// of its own and the composited alpha comes from the panel's own fill
/// ([`background_fill`]). Transparent black specifically, not "the panel colour
/// with a zero alpha": premultiplied data with a channel above its alpha is
/// invalid, and a compositor applying `src + dst * (1 - src.a)` to it would add
/// that channel to everything behind the window.
pub fn clear_color(mode: Mode) -> [f32; 4] {
    match mode {
        Mode::Normal => NORMAL_CLEAR.to_normalized_gamma_f32(),
        Mode::Panel => [0.0, 0.0, 0.0, 0.0],
    }
}

/// eframe's default clear colour, premultiplied and opaque.
///
/// `eframe::App::clear_color`'s default is
/// `Color32::from_rgba_unmultiplied(12, 12, 12, 180)`, whose premultiplied form is
/// `(8, 8, 8)`. That is the colour the ordinary window has always shown wherever
/// the app paints nothing, so it is restated here — at full alpha — rather than
/// inherited, and a test holds the two together.
const NORMAL_CLEAR: egui::Color32 = egui::Color32::from_rgba_premultiplied(8, 8, 8, 255);

/// The colour the panel paints its own background in.
///
/// At the default opacity of 1.0 this is exactly the `visuals.panel_fill` that
/// egui's default `CentralPanel` frame painted before this change, so the default
/// panel is unchanged; below it, the fill carries the configured alpha and the
/// window behind it shows through.
///
/// Non-finite opacity is rejected BEFORE the clamp, deliberately. Clamping first
/// and checking finiteness afterwards lets `-inf` through as the one value the
/// check exists to prevent: `-inf` clamps cleanly to `MIN_BACKGROUND_OPACITY` and
/// so passes a post-clamp `is_finite` check, while `NaN as u8` is 0 — either way
/// an invisible panel, silently. A normaliser that maps out-of-range inputs onto
/// legal ones destroys the evidence a validator needs, so validate first.
pub fn background_fill(visuals: &egui::Visuals, opacity: f32) -> egui::Color32 {
    if !opacity.is_finite() {
        log::warn!("panel background opacity {opacity} is not finite; using an opaque background");
        return visuals.panel_fill;
    }
    let opacity = opacity.clamp(MIN_BACKGROUND_OPACITY, MAX_BACKGROUND_OPACITY);
    let [red, green, blue, _] = visuals.panel_fill.to_srgba_unmultiplied();
    egui::Color32::from_rgba_unmultiplied(red, green, blue, (opacity * 255.0).round() as u8)
}

/// The position to restore, or `None` when the window manager should place the
/// panel itself.
///
/// `None` in two cases, both of them a refusal to guess:
///
/// * **Nothing stored.** A fresh install, or a `panel.json` written before the
///   field existed: the panel is placed exactly as it was before this change.
/// * **The monitor size is unknown.** An unclamped corner cannot be shown to be on
///   screen, and a panel past the edge of the monitor cannot be dragged back. The
///   window manager's own placement is always reachable, so it is the safe answer.
///
/// A stored position that is *out of bounds* is clamped rather than refused, so a
/// smaller monitor moves the panel back into view instead of discarding where the
/// user put it.
pub fn restore_position(
    stored: Option<PanelPosition>,
    panel: [f32; 2],
    monitor: Option<[f32; 2]>,
) -> Option<[f32; 2]> {
    let stored = stored?;
    let monitor = monitor?;
    Some(clamp_to_monitor(stored.as_array(), panel, monitor))
}

/// Clamp a panel position so the whole panel stays inside the monitor.
///
/// `panel` is the panel's size in points and `monitor` the monitor's size, so the
/// result is the nearest corner at which no part of the panel is outside
/// `0..=monitor - panel` on either axis.
///
/// **Single monitor only.** The monitor's ORIGIN is not available: `ViewportInfo`
/// reports a monitor *size* and has no monitor rect, so there is nothing to
/// offset a second monitor by. This assumes the monitor starts at `(0, 0)`, which
/// is true for the only-monitor case and wrong for a second monitor placed to the
/// left of or above the first. That is why the stored position is documented as
/// single-monitor.
pub fn clamp_to_monitor(position: [f32; 2], panel: [f32; 2], monitor: [f32; 2]) -> [f32; 2] {
    [
        clamp_axis(position[0], panel[0], monitor[0]),
        clamp_axis(position[1], panel[1], monitor[1]),
    ]
}

/// One axis of [`clamp_to_monitor`].
///
/// A panel larger than the monitor has no corner that shows all of it, so the
/// range collapses to `0` — the top-left of the monitor — rather than to a
/// negative left overhang. A non-finite input also lands at `0`: `f32::clamp`
/// propagates NaN, and a NaN corner would be sent to the window untouched.
fn clamp_axis(value: f32, panel: f32, monitor: f32) -> f32 {
    if !value.is_finite() || !panel.is_finite() || !monitor.is_finite() {
        return 0.0;
    }
    value.clamp(0.0, (monitor - panel).max(0.0))
}

/// The commands a spec produces, in the order they must be sent.
///
/// Split out from [`apply_spec`] so the ORDER is testable without a display: it
/// is load-bearing (see `apply_spec`), and a `Vec` of the real command type is
/// the only form of it a unit test can see.
pub fn spec_commands(spec: &WindowSpec) -> Vec<egui::ViewportCommand> {
    let mut commands = vec![
        egui::ViewportCommand::MinInnerSize(egui::vec2(spec.min_inner[0], spec.min_inner[1])),
        egui::ViewportCommand::Resizable(spec.resizable),
        egui::ViewportCommand::Decorations(spec.decorations),
        egui::ViewportCommand::InnerSize(egui::vec2(spec.inner[0], spec.inner[1])),
        // Before the position, which must stay last (see `apply_spec`). The level
        // is an independent EWMH request that nothing else in this batch depends
        // on, so its order against the sizes is not load-bearing.
        egui::ViewportCommand::WindowLevel(spec.level),
    ];
    if let Some(position) = spec.position {
        commands.push(egui::ViewportCommand::OuterPosition(egui::pos2(
            position[0],
            position[1],
        )));
    }
    commands
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
///
/// **The position goes LAST**, after both sizes, for the same reason: a position
/// sent in the same batch as a size the window manager is still clamping to its
/// old minimum is applied against the old geometry. It is also the reason
/// `OuterPosition` is read back ([`PositionWatch`]) instead of trusted.
pub fn apply_spec(ctx: &egui::Context, spec: WindowSpec) {
    match spec.position {
        Some(position) => log::info!(
            "window spec: {:.0}x{:.0}, min {:.0}x{:.0}, decorations {}, position {:.0},{:.0}",
            spec.inner[0],
            spec.inner[1],
            spec.min_inner[0],
            spec.min_inner[1],
            spec.decorations,
            position[0],
            position[1]
        ),
        None => log::info!(
            "window spec: {:.0}x{:.0}, min {:.0}x{:.0}, decorations {}, position: window manager",
            spec.inner[0],
            spec.inner[1],
            spec.min_inner[0],
            spec.min_inner[1],
            spec.decorations
        ),
    }
    for command in spec_commands(&spec) {
        ctx.send_viewport_cmd(command);
    }
}

/// How far a window's reported position may differ from the one requested and
/// still count as "the request landed".
///
/// Window geometry comes back in whole pixels while the panel's size is computed
/// in fractional points — a ten-row panel is 198.25 points tall, so the lowest
/// position that still shows all of it is `monitor_height - 198.25`, and the
/// window manager rounds that to a whole pixel. Measured on xfwm4/X11 at
/// `pixels_per_point = 1`: requested `980,521.75`, window at `980,522`. Printed
/// at no decimal places both read `980,522`, which is why an exact comparison
/// reported a mismatch that did not exist.
///
/// One point is the right size for it: a position a window manager actually
/// clamped or overruled differs by much more than a pixel, unclamped.
pub const POSITION_MATCH_TOLERANCE: f32 = 1.0;

/// Did the window land where it was asked to?
pub fn position_matches(requested: [f32; 2], observed: [f32; 2]) -> bool {
    (requested[0] - observed[0]).abs() <= POSITION_MATCH_TOLERANCE
        && (requested[1] - observed[1]).abs() <= POSITION_MATCH_TOLERANCE
}

/// How long the panel's observed position must hold still before it is saved.
///
/// Longer than the panel's 500 ms repaint cadence, so a still position is seen
/// stable on two consecutive frames before it is written; and long enough that a
/// drag's move events restart the timer instead of landing a write mid-gesture.
pub const POSITION_SETTLE: Duration = Duration::from_millis(700);

/// What to do about the panel's position this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PositionStep {
    /// Nothing to do: no usable position yet, or this one has been dealt with.
    Idle,
    /// The window has held this position long enough to be worth persisting.
    Settled {
        /// Where the window actually is, read back from it.
        position: [f32; 2],
        /// What was asked for, when anything was. A mismatch here is the window
        /// manager having clamped, delayed or overruled the request.
        requested: Option<[f32; 2]>,
    },
}

/// Follows where the panel window actually is, and decides when to persist it.
///
/// Fed from `ViewportInfo::outer_rect` — the window server's answer, in points —
/// and never from the value that was requested, so a request the window manager
/// declined cannot be saved as if it were where the panel is.
///
/// The "once per position" part is what makes a per-frame caller safe: a still
/// position reports [`PositionStep::Settled`] once and then `Idle` until the
/// window moves again, so a stationary panel is not written to once per frame.
#[derive(Debug, Default)]
pub struct PositionWatch {
    observed: Option<[f32; 2]>,
    stable_since: Option<Instant>,
    requested: Option<[f32; 2]>,
    reported: Option<[f32; 2]>,
}

impl PositionWatch {
    /// Record where the window is. `None` means the platform does not report a
    /// position at all (Wayland reports no outer rect), which leaves any settle
    /// window running rather than resetting it.
    pub fn observe(&mut self, position: Option<[f32; 2]>, now: Instant) {
        let Some(position) = position else {
            return;
        };
        if self.observed != Some(position) {
            self.observed = Some(position);
            self.stable_since = Some(now);
        }
    }

    /// Note that a position was just requested of the window manager.
    ///
    /// Restarts the settle window, so a position that has not been applied yet
    /// cannot settle as the final one.
    pub fn requested(&mut self, position: [f32; 2], now: Instant) {
        self.requested = Some(position);
        self.stable_since = Some(now);
    }

    /// The settled position, offered once per position.
    pub fn step(&mut self, now: Instant) -> PositionStep {
        let Some(position) = self.observed else {
            return PositionStep::Idle;
        };
        if self.reported == Some(position) {
            return PositionStep::Idle;
        }
        let Some(since) = self.stable_since else {
            return PositionStep::Idle;
        };
        if now.duration_since(since) < POSITION_SETTLE {
            return PositionStep::Idle;
        }
        self.reported = Some(position);
        PositionStep::Settled {
            position,
            requested: self.requested.take(),
        }
    }
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

    /// The monitor the size tests are stated against: 1280x720, the size of the
    /// X11 test screen (`references/x11-gui-test-isolation.md`).
    const TEST_MONITOR: Option<[f32; 2]> = Some([1280.0, 720.0]);
    /// The font row height the specs below are computed against.
    const TEST_ROW_HEIGHT: f32 = 14.0;

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
        let spec = panel_spec(&config, TEST_ROW_HEIGHT, TEST_MONITOR);
        let expected = layout::panel_size(&config, 3, TEST_ROW_HEIGHT);

        assert_eq!(spec.inner[0], expected.width);
        assert_eq!(spec.inner[1], expected.height);
        assert_eq!(spec.min_inner, spec.inner, "a panel is exactly its size");
        assert!(!spec.resizable);
        assert!(!spec.decorations);
        assert_eq!(
            spec.level,
            egui::WindowLevel::AlwaysOnTop,
            "a fresh panel is an overlay"
        );
    }

    #[test]
    fn the_panel_width_follows_the_config_so_the_slider_applies_at_once() {
        let mut config = config_with(&["ram_usage"]);
        config.width = 420.0;
        assert_eq!(
            panel_spec(&config, TEST_ROW_HEIGHT, TEST_MONITOR).inner[0],
            420.0
        );

        config.width = 240.0;
        assert_eq!(
            panel_spec(&config, TEST_ROW_HEIGHT, TEST_MONITOR).inner[0],
            240.0,
            "the width must be read fresh each time, not cached"
        );
    }

    #[test]
    fn switching_rows_changes_the_height_by_exactly_one_row_each() {
        let one = config_with(&["ram_usage"]);
        let two = config_with(&["ram_usage", "ram_used"]);
        let three = config_with(&["ram_usage", "ram_used", "ram_total"]);

        let h1 = panel_spec(&one, TEST_ROW_HEIGHT, TEST_MONITOR).inner[1];
        let h2 = panel_spec(&two, TEST_ROW_HEIGHT, TEST_MONITOR).inner[1];
        let h3 = panel_spec(&three, TEST_ROW_HEIGHT, TEST_MONITOR).inner[1];

        let row = layout::row_height(TEST_ROW_HEIGHT);
        assert!((h2 - h1 - row).abs() < 1e-4, "{h2} - {h1} != {row}");
        assert!((h3 - h2 - row).abs() < 1e-4, "{h3} - {h2} != {row}");
    }

    #[test]
    fn a_panel_with_no_visible_rows_is_still_a_valid_window() {
        let spec = panel_spec(&config_with(&[]), TEST_ROW_HEIGHT, TEST_MONITOR);
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
        assert_eq!(
            spec.level,
            egui::WindowLevel::Normal,
            "the ordinary window must not inherit the panel's level"
        );

        // And falls back to the compiled-in default when nothing was captured.
        assert_eq!(normal_spec(None).inner, NORMAL_INNER);
    }

    #[test]
    fn the_level_travels_as_a_runtime_command_and_normal_mode_clears_it() {
        // An initial `ViewportBuilder` level hint is not a route this code can
        // rely on (xfwm4 4.20 ignores it, leaving the panel under a fullscreen
        // game), so the level must be carried by the spec that is sent once the
        // window exists -- and returning to normal must actively ask for `Normal`
        // rather than merely stop asking for `AlwaysOnTop`.
        let mut config = config_with(&["ram_usage"]);
        config.stacking = crate::panel::config::Stacking::AlwaysOnTop;
        let panel = panel_spec(&config, TEST_ROW_HEIGHT, TEST_MONITOR);
        assert_eq!(panel.level, egui::WindowLevel::AlwaysOnTop);
        assert!(
            spec_commands(&panel).iter().any(|command| matches!(
                command,
                egui::ViewportCommand::WindowLevel(egui::WindowLevel::AlwaysOnTop)
            )),
            "the level must be in the commands the spec sends, not a builder hint"
        );

        config.stacking = crate::panel::config::Stacking::Normal;
        let panel = panel_spec(&config, TEST_ROW_HEIGHT, TEST_MONITOR);
        assert_eq!(panel.level, egui::WindowLevel::Normal);
        assert!(
            spec_commands(&panel).iter().any(|command| matches!(
                command,
                egui::ViewportCommand::WindowLevel(egui::WindowLevel::Normal)
            )),
            "switching the setting back must ask for an ordinary level"
        );

        let normal = normal_spec(Some([800.0, 900.0]));
        assert!(
            spec_commands(&normal).iter().any(|command| matches!(
                command,
                egui::ViewportCommand::WindowLevel(egui::WindowLevel::Normal)
            )),
            "leaving panel mode must clear the level, not just stop refreshing it"
        );
    }

    // ---- the transparent surface ----

    /// The ordinary window must not become translucent just because the root
    /// window now holds an alpha channel: eframe's own default clear carries an
    /// alpha of 180, so inheriting it would make the whole app 30 % see-through.
    #[test]
    fn the_normal_window_clears_fully_opaque() {
        let [red, green, blue, alpha] = clear_color(Mode::Normal);
        assert_eq!(alpha, 1.0, "the ordinary window must stay opaque");

        // And it is eframe's own default colour, not a new one: the same
        // premultiplied RGB the window showed before the visual changed.
        let eframe_default = egui::Color32::from_rgba_unmultiplied(12, 12, 12, 180);
        assert_eq!((red * 255.0).round() as u8, eframe_default.r());
        assert_eq!((green * 255.0).round() as u8, eframe_default.g());
        assert_eq!((blue * 255.0).round() as u8, eframe_default.b());
    }

    /// A translucent fill over an opaque clear is still an opaque window, so the
    /// panel's clear must contribute nothing at all.
    #[test]
    fn the_panel_clears_to_nothing_so_its_own_fill_carries_the_alpha() {
        assert_eq!(
            clear_color(Mode::Panel),
            [0.0, 0.0, 0.0, 0.0],
            "any colour here would either keep the window opaque or be added to \
             whatever is behind it"
        );
    }

    /// At the default opacity the panel looks exactly as it did before this
    /// change — the fill is the frame's own `panel_fill`, opaque.
    #[test]
    fn the_panel_background_at_full_opacity_is_the_panel_fill_unchanged() {
        for visuals in [egui::Visuals::dark(), egui::Visuals::light()] {
            let fill = background_fill(&visuals, 1.0);
            assert_eq!(fill, visuals.panel_fill);
            assert_eq!(fill.a(), 255);
        }
    }

    /// Below full opacity only the ALPHA changes: the colour stays the theme's
    /// panel fill, and the fill never becomes brighter than the panel behind it.
    #[test]
    fn a_lower_opacity_keeps_the_colour_and_drops_only_the_alpha() {
        let visuals = egui::Visuals::dark();
        let [red, green, blue, _] = visuals.panel_fill.to_srgba_unmultiplied();

        for (opacity, expected_alpha) in [(1.0, 255), (0.75, 191), (0.5, 128), (0.25, 64)] {
            let fill = background_fill(&visuals, opacity);
            assert_eq!(
                fill.a(),
                expected_alpha,
                "alpha for opacity {opacity} must be {expected_alpha}"
            );

            let recovered = fill.to_srgba_unmultiplied();
            for (got, want) in [
                (recovered[0], red),
                (recovered[1], green),
                (recovered[2], blue),
            ] {
                assert!(
                    (got as i32 - want as i32).abs() <= 1,
                    "{opacity}: colour drifted to {got} from {want}"
                );
            }

            // Premultiplied, so the fill can never add light of its own.
            assert!(fill.r() <= visuals.panel_fill.r());
            assert!(fill.g() <= visuals.panel_fill.g());
            assert!(fill.b() <= visuals.panel_fill.b());
        }
    }

    /// The range `config::normalize` enforces, enforced again where the value
    /// becomes a pixel — a panel that cannot be seen is worse than an opaque one.
    #[test]
    fn the_background_alpha_is_clamped_to_the_configured_range() {
        let visuals = egui::Visuals::dark();
        assert_eq!(
            background_fill(&visuals, 0.0).a(),
            (MIN_BACKGROUND_OPACITY * 255.0).round() as u8,
            "below the minimum must not mean invisible"
        );
        assert_eq!(background_fill(&visuals, -1.0).a(), background_fill(&visuals, 0.25).a());
        assert_eq!(background_fill(&visuals, 5.0), visuals.panel_fill);
    }

    /// A non-finite opacity is not a value a compositor can be handed. `-inf`
    /// clamps cleanly to the minimum and `NaN` becomes alpha 0, so both would
    /// survive a post-clamp finiteness check while producing an invisible panel:
    /// the check has to come first.
    #[test]
    fn a_non_finite_opacity_falls_back_to_an_opaque_background() {
        let visuals = egui::Visuals::dark();
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let fill = background_fill(&visuals, bad);
            assert_eq!(fill, visuals.panel_fill, "{bad} must not be painted");
            assert_eq!(fill.a(), 255, "{bad} must not make the panel invisible");
        }
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

    // ---- the stored position ----

    #[test]
    fn a_panel_with_nothing_stored_is_placed_by_the_window_manager() {
        // The behaviour before this field existed, and the behaviour for every
        // `panel.json` written before it: no position request at all.
        let spec = panel_spec(&config_with(&["ram_usage"]), TEST_ROW_HEIGHT, TEST_MONITOR);
        assert_eq!(spec.position, None);
        assert!(
            !spec_commands(&spec)
                .iter()
                .any(|command| matches!(command, egui::ViewportCommand::OuterPosition(_))),
            "nothing stored must not move the window"
        );
    }

    #[test]
    fn a_stored_position_is_restored_and_clamped_into_the_monitor() {
        let mut config = config_with(&["ram_usage"]);
        let monitor = [1280.0, 720.0];
        let panel = panel_spec(&config, TEST_ROW_HEIGHT, Some(monitor)).inner;

        // Inside the monitor: restored exactly, so the panel comes back where the
        // user left it.
        config.position = Some(PanelPosition::new(64.0, 96.0));
        assert_eq!(
            panel_spec(&config, TEST_ROW_HEIGHT, Some(monitor)).position,
            Some([64.0, 96.0])
        );

        // Past the bottom-right: the last corner that still shows the whole panel.
        config.position = Some(PanelPosition::new(5000.0, 5000.0));
        assert_eq!(
            panel_spec(&config, TEST_ROW_HEIGHT, Some(monitor)).position,
            Some([monitor[0] - panel[0], monitor[1] - panel[1]])
        );

        // Past the top-left.
        config.position = Some(PanelPosition::new(-100.0, -100.0));
        assert_eq!(
            panel_spec(&config, TEST_ROW_HEIGHT, Some(monitor)).position,
            Some([0.0, 0.0])
        );
    }

    #[test]
    fn the_clamp_keeps_the_whole_panel_inside_the_monitor() {
        let panel = [300.0, 200.0];
        let monitor = [1280.0, 720.0];
        for (requested, expected) in [
            ([100.0, 50.0], [100.0, 50.0]),
            ([0.0, 0.0], [0.0, 0.0]),
            ([-40.0, -5.0], [0.0, 0.0]),
            ([980.0, 520.0], [980.0, 520.0]),
            ([981.0, 521.0], [980.0, 520.0]),
            ([5000.0, 5000.0], [980.0, 520.0]),
            ([5000.0, 10.0], [980.0, 10.0]),
            ([10.0, -5000.0], [10.0, 0.0]),
        ] {
            assert_eq!(
                clamp_to_monitor(requested, panel, monitor),
                expected,
                "clamping {requested:?} against a {monitor:?} monitor"
            );
        }
    }

    #[test]
    fn a_panel_larger_than_the_monitor_lands_at_the_monitor_origin() {
        // No corner shows all of a 300x200 panel on a 200x100 monitor. The top
        // left is at least on screen; a negative overhang would hide the panel's
        // own left edge and the value column with it.
        assert_eq!(
            clamp_to_monitor([50.0, 50.0], [300.0, 200.0], [200.0, 100.0]),
            [0.0, 0.0]
        );
    }

    #[test]
    fn a_non_finite_corner_or_monitor_never_reaches_the_window() {
        // `f32::clamp` propagates NaN, and `OuterPosition(NaN)` is a move request
        // with no defined meaning.
        let panel = [300.0, 200.0];
        let monitor = [1280.0, 720.0];
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(clamp_to_monitor([bad, 10.0], panel, monitor), [0.0, 10.0]);
            assert_eq!(clamp_to_monitor([10.0, bad], panel, monitor), [10.0, 0.0]);
            assert_eq!(
                clamp_to_monitor([10.0, 10.0], [bad, 200.0], monitor),
                [0.0, 10.0]
            );
            assert_eq!(
                clamp_to_monitor([10.0, 10.0], panel, [bad, 720.0]),
                [0.0, 10.0]
            );
        }
    }

    #[test]
    fn a_position_that_landed_within_a_pixel_counts_as_applied() {
        // The measured case: the panel's computed height is 198.25 points, so the
        // lowest corner that shows all of it is 521.75, and xfwm4 put the window
        // at 522. An exact comparison called that a window-manager overrule.
        assert!(position_matches([980.0, 521.75], [980.0, 522.0]));
        assert!(position_matches([980.0, 522.0], [980.0, 522.0]));
        assert!(position_matches([980.0, 522.0], [979.5, 522.5]));
        assert!(position_matches([980.0, 522.0], [979.0, 521.0]));

        // A position the window manager actually overruled or clamped is off by
        // far more than a pixel, and must not read as applied.
        assert!(!position_matches([5000.0, 5000.0], [980.0, 522.0]));
        assert!(!position_matches([980.0, 522.0], [980.0, 523.5]));
        assert!(!position_matches([980.0, 522.0], [978.0, 522.0]));
    }

    #[test]
    fn an_unknown_monitor_size_means_no_position_request() {
        // Nothing to clamp against, so nothing can be shown to be on screen. The
        // window manager's own placement is always reachable, so it is the answer.
        let config = PanelConfig {
            position: Some(PanelPosition::new(40.0, 60.0)),
            ..config_with(&["ram_usage"])
        };
        assert_eq!(panel_spec(&config, TEST_ROW_HEIGHT, None).position, None);
        assert_eq!(
            restore_position(config.position, [300.0, 200.0], None),
            None
        );
    }

    #[test]
    fn the_position_is_sent_after_the_size_and_the_minimum() {
        // The order is load-bearing, not cosmetic: a window manager clamps a size
        // request to the minimum it currently holds, so the minimum must be
        // lowered first -- and a position sent alongside a size that is still
        // being clamped is applied against the old geometry. This is the only
        // form of that order a display-free test can see.
        let config = PanelConfig {
            position: Some(PanelPosition::new(24.0, 36.0)),
            ..config_with(&["ram_usage"])
        };
        let spec = panel_spec(&config, TEST_ROW_HEIGHT, TEST_MONITOR);
        let commands = spec_commands(&spec);

        assert_eq!(
            commands,
            vec![
                egui::ViewportCommand::MinInnerSize(egui::vec2(
                    spec.min_inner[0],
                    spec.min_inner[1]
                )),
                egui::ViewportCommand::Resizable(false),
                egui::ViewportCommand::Decorations(false),
                egui::ViewportCommand::InnerSize(egui::vec2(spec.inner[0], spec.inner[1])),
                egui::ViewportCommand::WindowLevel(egui::WindowLevel::AlwaysOnTop),
                egui::ViewportCommand::OuterPosition(egui::pos2(24.0, 36.0)),
            ]
        );
    }

    #[test]
    fn the_normal_spec_asks_for_no_position() {
        // The panel and the normal window are the same window: normal mode leaves
        // placement to the window manager and makes no claim about it.
        assert_eq!(normal_spec(None).position, None);
        assert_eq!(normal_spec(Some([800.0, 900.0])).position, None);
    }

    // ---- the settle window ----

    #[test]
    fn a_position_that_holds_still_is_offered_once_and_then_not_again() {
        let t0 = Instant::now();
        let mut watch = PositionWatch::default();
        watch.observe(Some([10.0, 20.0]), t0);

        assert_eq!(watch.step(t0), PositionStep::Idle, "not settled yet");
        assert_eq!(
            watch.step(t0 + POSITION_SETTLE - Duration::from_millis(1)),
            PositionStep::Idle,
            "one millisecond short of settled is not settled"
        );
        assert_eq!(
            watch.step(t0 + POSITION_SETTLE),
            PositionStep::Settled {
                position: [10.0, 20.0],
                requested: None,
            }
        );
        assert_eq!(
            watch.step(t0 + POSITION_SETTLE + Duration::from_secs(60)),
            PositionStep::Idle,
            "a still window must not be persisted once per frame"
        );
    }

    #[test]
    fn a_moving_window_never_settles_in_the_middle_of_a_drag() {
        let mut now = Instant::now();
        let mut watch = PositionWatch::default();
        watch.observe(Some([0.0, 0.0]), now);

        for step in 1..=50 {
            now += Duration::from_millis(100);
            watch.observe(Some([step as f32 * 5.0, 0.0]), now);
            assert_eq!(
                watch.step(now),
                PositionStep::Idle,
                "a drag must not write the file while it is still moving"
            );
        }

        // And it settles once the pointer stops.
        now += POSITION_SETTLE;
        assert_eq!(
            watch.step(now),
            PositionStep::Settled {
                position: [250.0, 0.0],
                requested: None,
            }
        );
    }

    #[test]
    fn a_request_restarts_the_settle_window() {
        let t0 = Instant::now();
        let mut watch = PositionWatch::default();
        watch.observe(Some([10.0, 20.0]), t0);

        let at_settle = t0 + POSITION_SETTLE;
        watch.requested([300.0, 400.0], at_settle);
        assert_eq!(
            watch.step(at_settle),
            PositionStep::Idle,
            "a request restarts the window, so the old position cannot settle under it"
        );

        watch.observe(Some([300.0, 400.0]), at_settle + Duration::from_millis(20));
        assert_eq!(
            watch.step(at_settle + POSITION_SETTLE + Duration::from_millis(19)),
            PositionStep::Idle
        );
        assert_eq!(
            watch.step(at_settle + POSITION_SETTLE + Duration::from_millis(20)),
            PositionStep::Settled {
                position: [300.0, 400.0],
                requested: Some([300.0, 400.0]),
            }
        );
    }

    #[test]
    fn the_settled_position_is_what_the_window_reports_not_what_was_asked_for() {
        // The failure this exists to prevent: persisting the request, so a panel
        // the window manager clamped comes back at a corner it was never at.
        let t0 = Instant::now();
        let mut watch = PositionWatch::default();
        watch.requested([5000.0, 5000.0], t0);
        watch.observe(Some([980.0, 520.0]), t0 + Duration::from_millis(16));

        assert_eq!(
            watch.step(t0 + POSITION_SETTLE + Duration::from_millis(16)),
            PositionStep::Settled {
                position: [980.0, 520.0],
                requested: Some([5000.0, 5000.0]),
            },
            "the observed corner is what gets saved, with the request alongside it \
             so the caller can report that the two disagree"
        );
    }

    #[test]
    fn an_unreported_position_neither_settles_nor_cancels_one() {
        // Wayland reports no outer rect. That is "unknown", not "at 0,0", and it
        // must not restart a settle window that is already running.
        let t0 = Instant::now();
        let mut watch = PositionWatch::default();
        assert_eq!(watch.step(t0 + POSITION_SETTLE), PositionStep::Idle);

        watch.observe(Some([1.0, 2.0]), t0);
        watch.observe(None, t0 + Duration::from_millis(600));
        assert_eq!(
            watch.step(t0 + POSITION_SETTLE),
            PositionStep::Settled {
                position: [1.0, 2.0],
                requested: None,
            },
            "an unknown position leaves the settle window where it was"
        );
    }
}
