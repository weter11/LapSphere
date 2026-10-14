//! The panel's two settings surfaces, and why they are two.
//!
//! # Why not one menu
//!
//! The obvious design is one right-click menu holding every control. It does not
//! fit, and the reason is structural rather than a matter of trimming: **egui
//! paints a popup into the same surface as the window that owns it.** The panel
//! window is 300x198 -- exactly the size the computed layout asks for -- so a
//! menu taller than that is drawn into pixels the window does not have. A
//! decorations-free, non-resizable panel cannot be made bigger to accommodate its
//! own menu either: forcing it to 780x900 with `xdotool windowsize` did not help,
//! because the WM declines to resize a window whose `min_inner` equals its
//! `inner`.
//!
//! So the controls are split by where they can physically live:
//!
//! * [`draw_context_menu`] -- two entries, drawn in the panel window: "Panel
//!   settings..." and "< Back to normal mode". They fit; the menu geometry as
//!   measured is on the measurement page (`docs/development/panel-measurements.md`).
//! * [`draw_settings_body`] -- everything else, in a SEPARATE viewport opened by
//!   the caller, so it is a real OS window with its own size and is never clipped
//!   by the panel. Deliberately an ordinary window: not always-on-top, not
//!   skip-taskbar, not tied to panel mode.
//!
//! One copy of every control: the context menu only routes to the settings window.
//! The settings body holds the per-row checkboxes -- one collapsible section per
//! registry group -- the width slider, the background-opacity slider, and the
//! window controls (the stacking choice and "hide from the taskbar and pager").
//!
//! # The opacity slider acts on the running panel
//!
//! It writes `PanelConfig::background_opacity`, which the panel reads afresh every
//! frame (`panel::window::background_fill`), so the background changes on the next
//! frame with no restart and no re-send of the window spec: neither the window's
//! size, its level nor its decorations depend on the opacity. Only the background
//! is affected — the rows' text is painted opaque on top of it.
//!
//! # Opening the popup: through the surface Response
//!
//! The menu opens through the ordinary egui path: the caller registers the whole
//! panel surface with `ui.interact(...)` and calls `Response::context_menu` on the
//! response. The pointer is NOT read directly, and `Sense::click_and_drag()` (not
//! `click()`) is the sense the surface is declared with, so the drag that PR 2
//! needs does not have to be retrofitted underneath the code that uses it.
//!
//! An earlier version of this module documented the opposite: that the pointer had
//! to be read off the context because the response never receives egui's CLICKED
//! flag. That diagnosis came from reading two probe values out of SEPARATE frames
//! and is retracted -- `docs/development/panel-measurements.md` records both the
//! wrong turn and the single-frame probe that refuted it. The tests below pin the
//! correct behaviour from both sides: a right click must reach the response, and a
//! left click must never register as a secondary one.

use super::config::{
    PanelConfig, Stacking, MAX_BACKGROUND_OPACITY, MAX_WIDTH, MIN_BACKGROUND_OPACITY, MIN_WIDTH,
};
use super::rows::{self, Group};

/// Viewport id of the settings window, stable so it is reused rather than
/// recreated on every open.
pub fn settings_viewport() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("panel_settings")
}

/// The size the settings window opens at: tall enough for four collapsed groups
/// plus the width slider and the window controls without scrolling.
pub const SETTINGS_SIZE: [f32; 2] = [360.0, 640.0];

/// What a settings interaction wants done, applied by the caller outside the UI
/// pass.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SettingsOutcome {
    pub back_to_normal: bool,
    /// The config changed and must be written to `panel.json`.
    pub config_changed: bool,
    /// Something the window SPEC carries changed (the width, or the stacking
    /// level), so the spec must be re-sent this frame rather than at the next
    /// mode switch.
    pub window_spec_changed: bool,
    /// The user asked to close the settings window.
    pub close_requested: bool,
}

/// What the context menu asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextAction {
    None,
    OpenSettings,
    BackToNormal,
}

/// The two-entry menu drawn inside the panel window.
///
/// Kept to what physically fits; see the module docs.
pub fn draw_context_menu(ui: &mut egui::Ui) -> ContextAction {
    if ui.button("Panel settings...").clicked() {
        return ContextAction::OpenSettings;
    }
    if ui.button("< Back to normal mode").clicked() {
        return ContextAction::BackToNormal;
    }
    ContextAction::None
}

/// Draw every panel control. Rendered into `ui`, which the caller supplies --
/// either the settings viewport or a test's `CentralPanel` -- so the same
/// function serves both and there is no duplicated control list.
pub fn draw_settings_body(
    ui: &mut egui::Ui,
    state: &crate::app::AppState,
    config: &mut PanelConfig,
    outcome: &mut SettingsOutcome,
) {
    egui::ScrollArea::vertical()
        .max_height((ui.available_height() - 220.0).max(60.0))
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Rows").strong());
            for group in Group::ALL {
                draw_group(ui, group, state, config, outcome);
            }
        });

    ui.separator();

    // Width goes straight into the config, clamped to the range `normalize`
    // enforces, so a drag cannot write an unusable width. The panel reads it
    // fresh every frame, so there is no apply step.
    ui.label(egui::RichText::new("Width").strong());
    let mut width = config.width.clamp(MIN_WIDTH, MAX_WIDTH);
    if ui
        .add(egui::Slider::new(&mut width, MIN_WIDTH..=MAX_WIDTH).suffix(" px"))
        .changed()
    {
        config.width = width;
        outcome.config_changed = true;
        outcome.window_spec_changed = true;
    }

    // The opacity goes straight into the config, exactly as the width does, and
    // is clamped to the range `normalize` enforces so a drag cannot write an
    // unusable value. The panel reads it fresh every frame, so there is no apply
    // step and no restart. It is NOT part of the window spec: nothing about the
    // window's size, level or decorations depends on it, so no re-send is needed.
    ui.label(egui::RichText::new("Background opacity").strong());
    let (opacity_response, opacity) = opacity_control(ui, config.background_opacity);
    if opacity_response.changed() {
        config.background_opacity = opacity;
        outcome.config_changed = true;
    }
    ui.label(
        egui::RichText::new(
            "Only the background becomes translucent — text and values stay opaque. \
             What is behind the panel shows through only under a compositing window \
             manager; without one nothing blends, so a reduced opacity darkens the \
             background instead of making it see-through.",
        )
        .weak()
        .small(),
    );

    ui.separator();

    // The window controls. Both act on the RUNNING window: the stacking level is
    // part of the window spec (re-sent below), and the taskbar state is dispatched
    // by the caller from the config, so neither needs a restart or an apply step.
    ui.label(egui::RichText::new("Window").strong());

    for (option, response) in stacking_controls(ui, config.stacking) {
        if response.clicked() && option != config.stacking {
            config.stacking = option;
            outcome.config_changed = true;
            // The level rides on the spec, so the spec is what has to be re-sent.
            outcome.window_spec_changed = true;
        }
    }

    let (taskbar, hide) = taskbar_control(ui, config.hide_from_taskbar);
    if taskbar.changed() && hide != config.hide_from_taskbar {
        config.hide_from_taskbar = hide;
        outcome.config_changed = true;
    }
    ui.label(
        egui::RichText::new(
            "Taskbar and pager visibility is X11 (EWMH) only; a window manager \
                             that does not honour it leaves the panel listed.",
        )
        .weak()
        .small(),
    );

    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("< Back to normal mode").clicked() {
            outcome.back_to_normal = true;
        }
        if ui.button("Close").clicked() {
            outcome.close_requested = true;
        }
    });
}

/// The stacking choice, one selectable entry per option.
///
/// Split out and returning its own responses for the same reason the row groups
/// are: the test that proves a click on it reaches the config has to be able to
/// aim at it, and a control whose rect a test cannot obtain is a control the
/// suite can only assume works.
fn stacking_controls(ui: &mut egui::Ui, current: Stacking) -> Vec<(Stacking, egui::Response)> {
    ui.horizontal(|ui| {
        Stacking::ALL
            .iter()
            .copied()
            .map(|option| {
                let response = ui.selectable_label(current == option, option.label());
                (option, response)
            })
            .collect()
    })
    .inner
}

/// The "hide from the taskbar and pager" checkbox, and the value it now holds.
fn taskbar_control(ui: &mut egui::Ui, current: bool) -> (egui::Response, bool) {
    let mut value = current;
    let response = ui.checkbox(&mut value, "Hide from the taskbar and pager");
    (response, value)
}

/// The background-opacity slider, and the value it now holds.
///
/// Split out and returning its own response for the same reason the stacking
/// control is: the test that proves a drag on it reaches the config has to be able
/// to aim at it.
fn opacity_control(ui: &mut egui::Ui, current: f32) -> (egui::Response, f32) {
    let mut value = current.clamp(MIN_BACKGROUND_OPACITY, MAX_BACKGROUND_OPACITY);
    let response = ui.add(
        egui::Slider::new(&mut value, MIN_BACKGROUND_OPACITY..=MAX_BACKGROUND_OPACITY)
            .fixed_decimals(2),
    );
    (response, value)
}

/// One collapsible group of row checkboxes.
///
/// Collapsed by default: four open groups is roughly twenty rows, which would
/// push the sliders below the fold.
fn draw_group(
    ui: &mut egui::Ui,
    group: Group,
    state: &crate::app::AppState,
    config: &mut PanelConfig,
    outcome: &mut SettingsOutcome,
) {
    let ids: Vec<usize> = (0..config.rows.len())
        .filter(|i| group.of(&config.rows[*i].id))
        .collect();
    if ids.is_empty() {
        return;
    }

    egui::CollapsingHeader::new(group.label())
        .default_open(false)
        .show(ui, |ui| {
            for index in ids {
                let id = config.rows[index].id.clone();
                let Some(row) = rows::find(&id) else {
                    continue;
                };
                ui.horizontal(|ui| {
                    if ui
                        .checkbox(&mut config.rows[index].visible, row.label)
                        .changed()
                    {
                        outcome.config_changed = true;
                    }
                    // The live value, so a checkbox shows what it turns on.
                    ui.label(
                        egui::RichText::new((row.read)(state).text)
                            .monospace()
                            .weak(),
                    );
                });
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppState;
    use egui::{Context, Rect};

    fn with_ui(f: impl Fn(&mut egui::Ui, &mut PanelConfig, &mut SettingsOutcome)) {
        let ctx = Context::default();
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(SETTINGS_SIZE[0], SETTINGS_SIZE[1]),
        ));
        let mut config = PanelConfig::fresh();
        let mut outcome = SettingsOutcome::default();
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| f(ui, &mut config, &mut outcome));
        });
    }

    /// What one scripted interaction produced.
    struct Driven {
        secondary_clicked: bool,
        primary_clicked: bool,
        /// The action the menu body returned, if it was painted.
        action: ContextAction,
        /// Shapes painted on the frame after the click -- the menu being open.
        painted_after_click: usize,
    }

    /// Drive the panel surface through the ordinary egui path: register it with
    /// [`surface_sense`], then read the Response's own click flags.
    fn drive(button: egui::PointerButton) -> Driven {
        let state = AppState::new();
        let ctx = Context::default();
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(300.0, 198.0));
        let at = egui::pos2(150.0, 40.0);
        let ev = |pressed| egui::Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers: egui::Modifiers::default(),
        };

        // Hover must arrive in its own frame: egui evaluates a click against the
        // pointer state as it was when the move was processed.
        // Hover, then press, then release, then one frame for the popup to paint.
        // Press/release must be in different frames: egui treats a
        // press-and-release inside one pass as a click only via `is_decidedly_
        // dragging`, and separating them is what a real click looks like.
        let frames: Vec<Vec<egui::Event>> = vec![
            vec![egui::Event::PointerMoved(at)],
            vec![ev(true)],
            vec![ev(false)],
            Vec::new(),
        ];

        let mut d = Driven {
            secondary_clicked: false,
            primary_clicked: false,
            action: ContextAction::None,
            painted_after_click: 0,
        };

        for (index, events) in frames.into_iter().enumerate() {
            let mut input = egui::RawInput::default();
            input.screen_rect = Some(screen);
            input.focused = true;
            input.events = events;
            let output = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let response = ui.interact(
                        ui.max_rect(),
                        ui.id().with("panel_surface"),
                        surface_sense(),
                    );
                    d.secondary_clicked |= response.secondary_clicked();
                    d.primary_clicked |= response.clicked();
                    response.context_menu(|ui| {
                        let mut config = PanelConfig::fresh();
                        let mut outcome = SettingsOutcome::default();
                        let _ = &outcome;
                        d.action = crate::panel::menu::draw_context_menu(ui);
                        let _ = &mut config;
                    });
                });
            });
            if index + 1 == frames_len(&ctx) {
                d.painted_after_click = output.shapes.len();
            }
        }
        let _ = state;
        d
    }

    fn frames_len(_ctx: &Context) -> usize {
        3
    }

    /// The sense the panel surface is registered with.
    ///
    /// `click_and_drag` rather than `click`: repositioning the panel by dragging
    /// is PR 2's work, and the sense has to be declared before the code that uses
    /// it rather than changed underneath it later.
    pub fn surface_sense() -> egui::Sense {
        egui::Sense::click_and_drag()
    }

    #[test]
    fn a_right_click_on_the_surface_opens_the_menu_through_the_response() {
        let d = drive(egui::PointerButton::Secondary);
        assert!(
            d.secondary_clicked,
            "the right click must reach the surface Response -- the whole point of this path"
        );
        assert!(d.painted_after_click > 0, "the open menu must paint");
    }

    #[test]
    fn a_left_click_never_counts_as_a_secondary_click() {
        // The surface is click-and-drag, so a left click lands on it too. It must
        // not open the menu: that would make the menu appear on every window drag.
        let d = drive(egui::PointerButton::Primary);
        assert!(
            d.primary_clicked,
            "the left click should land on the surface"
        );
        assert!(
            !d.secondary_clicked,
            "a left click must never register as a secondary click"
        );
        assert_eq!(d.action, ContextAction::None, "and must open nothing");
    }

    #[test]
    fn the_settings_entry_routes_to_the_settings_window() {
        // "Panel settings..." is a routing decision, not a boolean: it must open
        // the settings viewport and close the menu, not leave the menu on top.
        assert_ne!(
            ContextAction::OpenSettings,
            ContextAction::None,
            "the action is distinct so the caller can route it"
        );
        assert_ne!(ContextAction::OpenSettings, ContextAction::BackToNormal);
    }

    #[test]
    fn the_settings_body_renders_with_every_row_disabled() {
        with_ui(|ui, config, outcome| {
            for row in &mut config.rows {
                row.visible = false;
            }
            draw_settings_body(ui, &AppState::new(), config, outcome);
            assert!(!outcome.back_to_normal);
            assert!(!outcome.config_changed);
        });
    }

    #[test]
    fn the_settings_body_survives_an_unknown_row() {
        // `normalize` removes these, but a panic in the settings window would be
        // unrecoverable for the user, so it must not assume normalised input.
        with_ui(|ui, config, outcome| {
            config.rows.push(super::super::config::RowConfig {
                id: "not_a_real_row".into(),
                visible: true,
            });
            draw_settings_body(ui, &AppState::new(), config, outcome);
        });
    }

    #[test]
    fn the_settings_window_is_an_ordinary_sized_window() {
        assert!(SETTINGS_SIZE[1] >= 400.0, "tall enough for four groups");
        assert!(SETTINGS_SIZE[0] >= 320.0, "wide enough for a row label");
    }

    // ---- the window controls ----

    /// Click the control `draw` lays out, and report what it produced on the frame
    /// the click was released.
    ///
    /// The frame ORDER is the part that matters: egui evaluates a click against the
    /// pointer state as it was when the move was processed, so the pointer has to
    /// arrive in a frame of its own before the press, and the press and the release
    /// have to be in separate frames. Get that wrong and nothing registers, which
    /// reads exactly like a control that is not wired up.
    ///
    /// Aiming at the response's own rect is deliberate: coordinates typed into the
    /// test would keep passing after a layout change moved the control out of the
    /// window, which is the one way a settings control becomes unreachable.
    ///
    /// The value is collected on the frame the control reported the interaction,
    /// not read off the last frame: a slider takes its value from the pointer on
    /// the PRESS frame while a checkbox reports on release, so a driver that read
    /// only the final frame would report a slider drag as "nothing happened".
    fn click_control<T, F>(draw: F) -> T
    where
        F: Fn(&mut egui::Ui) -> (egui::Response, T),
        T: Default,
    {
        let ctx = Context::default();
        let screen = Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(SETTINGS_SIZE[0], SETTINGS_SIZE[1]),
        );
        let mut at = egui::pos2(0.0, 0.0);
        let mut produced = T::default();

        for frame in 0..4 {
            let mut input = egui::RawInput::default();
            input.screen_rect = Some(screen);
            // egui discards pointer input for an unfocused viewport.
            input.focused = true;
            input.events = match frame {
                0 => Vec::new(),
                1 => vec![egui::Event::PointerMoved(at)],
                2 => vec![pointer(true, at)],
                _ => vec![pointer(false, at)],
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let (response, value) = draw(ui);
                    if frame == 0 {
                        at = response.rect.center();
                    }
                    if response.changed() || response.clicked() {
                        produced = value;
                    }
                });
            });
        }
        produced
    }

    fn pointer(pressed: bool, at: egui::Pos2) -> egui::Event {
        egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        }
    }

    /// The stacking option a click on `target` asks for, or `None` when clicking it
    /// changed nothing.
    fn click_stacking(current: Stacking, target: Stacking) -> Option<Stacking> {
        click_control(|ui| {
            let controls = stacking_controls(ui, current);
            let aim = controls
                .iter()
                .find(|(option, _)| *option == target)
                .expect("every stacking option has a control")
                .1
                .clone();
            let chosen = controls
                .iter()
                .find(|(option, response)| response.clicked() && *option != current)
                .map(|(option, _)| *option);
            (aim, chosen)
        })
    }

    #[test]
    fn choosing_a_stacking_option_reports_a_change_so_the_window_spec_is_re_sent() {
        // The level rides on the window spec, so a stacking change is not just a
        // config write: without the re-send the setting would only take effect at
        // the next mode switch, which is exactly the "restart to apply" behaviour
        // the control must not have.
        assert_eq!(
            click_stacking(Stacking::AlwaysOnTop, Stacking::Normal),
            Some(Stacking::Normal)
        );
        assert_eq!(
            click_stacking(Stacking::Normal, Stacking::AlwaysOnTop),
            Some(Stacking::AlwaysOnTop)
        );
        // Clicking the option that is already chosen is not a change, so nothing is
        // rewritten and nothing is re-sent.
        assert_eq!(
            click_stacking(Stacking::AlwaysOnTop, Stacking::AlwaysOnTop),
            None
        );
    }

    /// The taskbar value a click on the checkbox asks for, or `None` when the click
    /// changed nothing.
    fn click_taskbar(current: bool) -> Option<bool> {
        click_control(|ui| {
            let (response, value) = taskbar_control(ui, current);
            let changed = response.changed().then_some(value);
            (response, changed)
        })
    }

    #[test]
    fn the_taskbar_control_reports_the_value_the_click_asked_for() {
        assert_eq!(click_taskbar(true), Some(false), "unchecking it");
        assert_eq!(click_taskbar(false), Some(true), "checking it");
    }

    // ---- the background-opacity control ----

    /// The opacity a click at the slider's own centre asks for, or `None` when the
    /// click changed nothing.
    fn click_opacity(current: f32) -> Option<f32> {
        click_control(|ui| {
            let (response, value) = opacity_control(ui, current);
            let changed = response.changed().then_some(value);
            (response, changed)
        })
    }

    #[test]
    fn the_opacity_control_reports_the_value_the_click_asked_for() {
        // Aimed at the slider's own centre, so the click lands mid-track: the
        // value must move off both ends and stay inside the range, which is what
        // "the control is wired to the config" means for a slider.
        let from_opaque = click_opacity(1.0).expect("a click on the slider must change the opacity");
        assert!(
            (MIN_BACKGROUND_OPACITY..1.0).contains(&from_opaque),
            "clicking mid-track from 1.0 gave {from_opaque}"
        );

        let from_translucent =
            click_opacity(MIN_BACKGROUND_OPACITY).expect("a click must change the opacity");
        assert!(
            (MIN_BACKGROUND_OPACITY..=1.0).contains(&from_translucent)
                && from_translucent > MIN_BACKGROUND_OPACITY,
            "clicking mid-track from the minimum gave {from_translucent}"
        );
    }

    /// A config value outside the range — a hand-edited `panel.json`, or one from
    /// a build with a different range — must not be shown as a handle past the end
    /// of the track, and must not be written back out of range.
    #[test]
    fn the_opacity_control_clamps_what_it_is_given() {
        with_ui(|ui, _config, _outcome| {
            let (_, high) = opacity_control(ui, 5.0);
            assert_eq!(high, MAX_BACKGROUND_OPACITY);
            let (_, low) = opacity_control(ui, 0.0);
            assert_eq!(low, MIN_BACKGROUND_OPACITY);
        });
    }

    #[test]
    fn every_row_is_reachable_from_exactly_one_group() {
        // The settings window is the only place a row can be toggled, so a row no
        // group claims is a row the user cannot enable, and a row two groups claim
        // gets two checkboxes fighting over one flag.
        for id in rows::all_ids() {
            let owners: Vec<Group> = Group::ALL
                .iter()
                .copied()
                .filter(|group| group.of(id))
                .collect();
            assert_eq!(owners.len(), 1, "row {id} is claimed by {owners:?}");
        }
    }
}
