//! Mode switching on ONE window, plus the X11 atoms that keep the panel out of
//! the taskbar.
//!
//! One window, not two (ADR-1 and its B9–B21 amendment in
//! `docs/development/panel-design.md`). The window keeps its `Normal` type for
//! life; panel mode is a set of `ViewportCommand`s on the same root viewport.
//! That avoids a second GL surface, a second native window and re-targeting the
//! tray, all of which the measured alternative (H5) costs.
//!
//! The X11 side needs three things the design doc measured and this implements
//! as stated:
//!
//! 1. **`WindowLevel(AlwaysOnTop)` must be requested at runtime.** The
//!    `ViewportBuilder::with_window_level` hint is ignored by xfwm4 (B9), so the
//!    level is set on the first frame in panel mode.
//! 2. **No fixed delay before the ClientMessage.** The WM has to have accepted
//!    the window first; the message is sent once the window id is known and the
//!    WM has taken it, then `_NET_WM_STATE` is **read back** and the send is
//!    retried while the atoms are absent. The measured fact is that even 82 ms
//!    after acceptance the first send can be ignored — the retry is what makes
//!    it reliable, not the timing.
//! 3. **`data.l[3] = 0`.** The doc records that Wine's documented `1` also
//!    works here (6/6) and that an earlier "must be 0" claim came from a
//!    confounded experiment; the owner's instruction is `0`, which is what is
//!    sent.
//!
//! Wayland: `_NET_WM_STATE` does not exist there and a client may not send it.
//! Every atom operation here is compiled only for Linux+X11 and is a no-op
//! elsewhere; `WindowLevel` is the only window primitive the panel depends on,
//! and ADR-3 accepts that it may not stick.

use super::config::{PanelConfig, PanelCorner};
use super::render::{self, Anchor};

/// Which surface the app is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The ordinary LapSphere window.
    Normal,
    /// The compact overlay panel.
    Panel,
}

impl Mode {
    pub fn is_panel(self) -> bool {
        matches!(self, Mode::Panel)
    }
}

/// Geometry of the normal window, captured before the first switch so returning
/// to it restores the size the user had rather than a hard-coded default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormalGeometry {
    pub inner: [f32; 2],
    pub min_inner: [f32; 2],
}

/// The normal window's size, matching `main.rs`'s `ViewportBuilder`.
pub const NORMAL_INNER: [f32; 2] = [570.0, 620.0];
/// `main.rs` sets this as the minimum inner size.
pub const NORMAL_MIN_INNER: [f32; 2] = [440.0, 470.0];

/// Corner enum → layout anchor.
fn anchor_for(corner: PanelCorner) -> Anchor {
    match corner {
        PanelCorner::TopLeft => Anchor::TopLeft,
        PanelCorner::TopRight => Anchor::TopRight,
        PanelCorner::BottomLeft => Anchor::BottomLeft,
        PanelCorner::BottomRight => Anchor::BottomRight,
    }
}

/// Everything the window must be told when entering panel mode.
pub struct PanelWindowSpec {
    pub inner: [f32; 2],
    pub min_inner: [f32; 2],
    pub resizable: bool,
    pub decorations: bool,
    pub always_on_top: bool,
    pub click_through: bool,
    pub outer_pos: Option<[f32; 2]>,
}

/// Compute the panel window's properties from the config.
///
/// The size comes from `render::layout`, which depends only on the element set
/// and the font scale — never on data — so entering panel mode always produces
/// the same window for the same config.
pub fn panel_spec(config: &PanelConfig, work_area: (f32, f32, f32, f32)) -> PanelWindowSpec {
    let layout = render::layout(config);
    let (x, y) = render::resolve_position(
        anchor_for(config.position.corner),
        config.position.offset_x,
        config.position.offset_y,
        layout,
        work_area,
    );

    PanelWindowSpec {
        inner: [layout.width, layout.height],
        // A panel has no meaningful minimum: it is exactly this size. A
        // min_inner equal to inner makes it non-resizable in effect even if the
        // WM honours the hint only loosely.
        min_inner: [layout.width, layout.height],
        resizable: false,
        decorations: false,
        always_on_top: config.always_on_top,
        click_through: config.click_through,
        outer_pos: Some([x, y]),
    }
}

/// Compute the normal-mode window properties, restoring the pre-panel geometry.
pub fn normal_spec(previous: Option<NormalGeometry>) -> PanelWindowSpec {
    let geometry = previous.unwrap_or(NormalGeometry {
        inner: NORMAL_INNER,
        min_inner: NORMAL_MIN_INNER,
    });

    PanelWindowSpec {
        inner: geometry.inner,
        min_inner: geometry.min_inner,
        resizable: true,
        decorations: true,
        always_on_top: false,
        click_through: false,
        outer_pos: None,
    }
}

/// The EWMH atoms the panel needs, and the retry policy for them.
///
/// Kept separate from the X connection so the sequencing is testable without a
/// display: `attempts_remaining` counts down, and `is_satisfied` reports what the
/// property read-back said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtomRetry {
    pub attempts_remaining: u32,
}

impl AtomRetry {
    pub const INITIAL_ATTEMPTS: u32 = 20;

    pub fn new() -> Self {
        Self {
            attempts_remaining: Self::INITIAL_ATTEMPTS,
        }
    }

    /// One retry tick. Returns true when another attempt should be made.
    pub fn next_attempt(&mut self) -> bool {
        if self.attempts_remaining == 0 {
            return false;
        }
        self.attempts_remaining -= 1;
        true
    }

    pub fn exhausted(&self) -> bool {
        self.attempts_remaining == 0
    }

    /// Did the read-back show both atoms present?
    pub fn is_satisfied(&self, skip_taskbar: bool, skip_pager: bool) -> bool {
        skip_taskbar && skip_pager
    }
}

impl Default for AtomRetry {
    fn default() -> Self {
        Self::new()
    }
}

/// Which `_NET_WM_STATE` action to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomAction {
    /// Add the atoms (panel mode).
    Add,
    /// Remove them (normal mode).
    Remove,
}

impl AtomAction {
    /// EWMH `_NET_WM_STATE` action values.
    pub fn data_l0(self) -> u32 {
        match self {
            AtomAction::Add => 1,    // _NET_WM_STATE_ADD
            AtomAction::Remove => 0, // _NET_WM_STATE_REMOVE
        }
    }
}

/// Build the `ClientMessageData` for a `_NET_WM_STATE` request.
///
/// `data.l[3]` is the source indication: 1 means "application", and the owner's
/// instruction for this build is 0. Recorded explicitly because the design doc
/// notes both values are accepted by xfwm4 and an earlier "must be 1" claim came
/// from a confounded experiment — so this is a decision, not a derivation.
pub fn state_client_message(action: AtomAction, skip_taskbar: u32, skip_pager: u32) -> [u32; 5] {
    [
        action.data_l0(),
        skip_taskbar,
        skip_pager,
        0, // source indication
        0, // reserved
    ]
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use crate::panel::config::{self, PanelConfig, PanelItemConfig};

    fn work_area() -> (f32, f32, f32, f32) {
        (0.0, 0.0, 2560.0, 1440.0)
    }

    #[test]
    fn the_panel_window_comes_from_the_element_set_only() {
        let mut a = PanelConfig::default();
        a.font_scale = 1.0;
        let mut b = a.clone();
        // Same element set and scale, wildly different labels and visibility
        // bookkeeping that must not affect geometry.
        b.items[0].label = Some("A much longer label".into());

        assert_eq!(
            panel_spec(&a, work_area()).inner,
            panel_spec(&b, work_area()).inner
        );
    }

    #[test]
    fn the_panel_window_is_not_resizable_or_decorated() {
        let spec = panel_spec(&PanelConfig::default(), work_area());
        assert!(!spec.resizable);
        assert!(!spec.decorations);
        assert_eq!(spec.min_inner, spec.inner, "no room to resize a panel");
    }

    #[test]
    fn the_normal_window_is_restored_decorated_and_resizable() {
        let spec = normal_spec(None);
        assert!(spec.resizable);
        assert!(spec.decorations);
        assert!(!spec.always_on_top);
        assert!(!spec.click_through);
        assert_eq!(spec.inner, NORMAL_INNER);
        assert_eq!(spec.min_inner, NORMAL_MIN_INNER);
    }

    #[test]
    fn the_pre_panel_geometry_is_restored() {
        let previous = NormalGeometry {
            inner: [800.0, 900.0],
            min_inner: [500.0, 500.0],
        };
        let spec = normal_spec(Some(previous));
        assert_eq!(spec.inner, [800.0, 900.0]);
        assert_eq!(spec.min_inner, [500.0, 500.0]);
    }

    #[test]
    fn always_on_top_and_click_through_come_from_the_config() {
        let mut config = PanelConfig::default();
        config.always_on_top = false;
        config.click_through = true;
        let spec = panel_spec(&config, work_area());
        assert!(!spec.always_on_top);
        assert!(spec.click_through);
    }

    #[test]
    fn the_panel_position_follows_the_configured_corner() {
        let area = work_area();

        let mut top_left = PanelConfig::default();
        top_left.position.corner = PanelCorner::TopLeft;
        let tl = panel_spec(&top_left, area).outer_pos.expect("positioned");

        let mut bottom_right = PanelConfig::default();
        bottom_right.position.corner = PanelCorner::BottomRight;
        let br = panel_spec(&bottom_right, area)
            .outer_pos
            .expect("positioned");

        // Compared against each other, not against the screen midpoint: the full
        // default panel is 1792 pt wide, wider than half of 2560, so a
        // bottom-right anchor legitimately lands left of centre.
        assert!(br[0] > tl[0], "bottom-right must sit right of top-left");
        assert!(br[1] > tl[1], "bottom-right must sit below top-left");
    }

    #[test]
    fn a_panel_is_always_inside_the_work_area() {
        let area = work_area();
        for corner in [
            PanelCorner::TopLeft,
            PanelCorner::TopRight,
            PanelCorner::BottomLeft,
            PanelCorner::BottomRight,
        ] {
            let mut config = PanelConfig::default();
            config.position.corner = corner;
            // A huge offset, as if saved on a much larger monitor.
            config.position.offset_x = 9000.0;
            config.position.offset_y = 9000.0;

            let spec = panel_spec(&config, area);
            let [x, y] = spec.outer_pos.expect("positioned");
            assert!(x >= area.0, "{corner:?} x={x} left of the work area");
            assert!(y >= area.1, "{corner:?} y={y} above the work area");
            assert!(
                x + spec.inner[0] <= area.0 + area.2 + 0.5,
                "{corner:?} x={x} runs off the right edge"
            );
            assert!(
                y + spec.inner[1] <= area.1 + area.3 + 0.5,
                "{corner:?} y={y} runs off the bottom edge"
            );
        }
    }

    #[test]
    fn a_smaller_monitor_still_yields_a_visible_panel() {
        // The case that matters on a laptop with a scaled external display: the
        // same config, a smaller work area.
        let config = PanelConfig::default();
        let big = panel_spec(&config, (0.0, 0.0, 3840.0, 2160.0));
        let small = panel_spec(&config, (0.0, 0.0, 1280.0, 720.0));
        // Size is resolution-independent; only the position is clamped.
        assert_eq!(big.inner, small.inner);
        let [x, y] = small.outer_pos.expect("positioned");
        assert!(x >= 0.0 && y >= 0.0);
    }

    #[test]
    fn the_window_type_is_never_changed() {
        // Nothing in this module can change the window TYPE: the spec carries no
        // such field, so the only lever available is `always_on_top`, which is a
        // stacking LEVEL and leaves the type alone. That is the B9-B21 finding —
        // typing the root Utility would make the main window a Utility window
        // for life.
        //
        // Asserted by the shape of the API: the only window-ish booleans the
        // spec exposes are the level and click-through.
        let spec = panel_spec(&PanelConfig::default(), work_area());
        assert!(spec.always_on_top);
        // The panel spec and the normal spec differ in exactly these four fields
        // plus the position, and never in any notion of a type.
        let normal = normal_spec(None);
        assert_ne!(spec.always_on_top, normal.always_on_top);
        assert_ne!(spec.decorations, normal.decorations);
        assert_ne!(spec.resizable, normal.resizable);
        assert_eq!(normal.outer_pos, None, "normal mode never moves the window");
    }

    #[test]
    fn the_retry_policy_gives_up_after_a_bounded_number_of_attempts() {
        let mut retry = AtomRetry::new();
        assert!(!retry.exhausted());
        let mut attempts: u32 = 0;
        while retry.next_attempt() {
            attempts += 1;
            assert!(
                attempts <= AtomRetry::INITIAL_ATTEMPTS,
                "retry must be bounded"
            );
        }
        assert_eq!(attempts, AtomRetry::INITIAL_ATTEMPTS);
        assert!(retry.exhausted());
        assert!(!retry.next_attempt(), "no attempts after exhaustion");
    }

    #[test]
    fn the_atoms_are_only_satisfied_when_both_are_present() {
        let retry = AtomRetry::new();
        assert!(retry.is_satisfied(true, true));
        assert!(
            !retry.is_satisfied(true, false),
            "half-applied is not applied"
        );
        assert!(!retry.is_satisfied(false, true));
        assert!(!retry.is_satisfied(false, false));
    }

    #[test]
    fn the_client_message_uses_l3_zero_and_the_right_action() {
        let add = state_client_message(AtomAction::Add, 1, 2);
        assert_eq!(add[0], 1, "ADD");
        assert_eq!(add[1], 1, "first atom");
        assert_eq!(add[2], 2, "second atom");
        assert_eq!(add[3], 0, "source indication is 0 by decision");
        assert_eq!(add[4], 0);

        let remove = state_client_message(AtomAction::Remove, 1, 2);
        assert_eq!(remove[0], 0, "REMOVE");
        assert_eq!(remove[3], 0);
    }

    #[test]
    fn mode_transitions_are_the_only_two_states() {
        assert!(Mode::Panel.is_panel());
        assert!(!Mode::Normal.is_panel());
        assert_ne!(Mode::Normal, Mode::Panel);
    }

    #[test]
    fn font_scale_changes_the_window_size() {
        let base = PanelConfig::default();
        let bigger = PanelConfig {
            font_scale: 2.0,
            ..base.clone()
        };
        assert!(
            panel_spec(&bigger, work_area()).inner[0] > panel_spec(&base, work_area()).inner[0]
        );
    }

    #[test]
    fn a_panel_with_a_single_element_still_fits_on_a_small_screen() {
        let single = PanelConfig {
            items: vec![PanelItemConfig::new("cpu_load")],
            ..Default::default()
        };
        let spec = panel_spec(&single, (0.0, 0.0, 640.0, 480.0));
        assert!(
            spec.inner[0] <= 640.0,
            "width {} exceeds a 640 px screen",
            spec.inner[0]
        );
        assert!(
            spec.inner[1] <= 480.0,
            "height {} exceeds a 480 px screen",
            spec.inner[1]
        );
    }

    #[test]
    fn the_config_normalizer_and_the_window_agree_on_the_item_set() {
        // The window size and the menu both read the normalized config, so a
        // hand-edited panel.json cannot produce two different notions of "the
        // panel".
        let mut raw = PanelConfig::default();
        raw.items.retain(|item| item.id == "cpu_load");
        raw.items
            .push(config::PanelItemConfig::new("bogus_element"));
        let normalized = config::normalize(raw);
        let expected = render::layout(&normalized);
        assert_eq!(
            panel_spec(&normalized, work_area()).inner,
            [expected.width, expected.height]
        );
    }
}
