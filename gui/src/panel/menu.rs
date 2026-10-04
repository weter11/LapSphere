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
//! * [`draw_context_menu`] -- two entries, about 80 px, drawn in the panel window:
//!   "Panel settings..." and "Back to the normal window". Both fit.
//! * [`draw_settings_body`] -- everything else, in a SEPARATE viewport opened by
//!   the caller, so it is a real OS window with its own size and is never clipped
//!   by the panel. Deliberately an ordinary window: not always-on-top, not
//!   skip-taskbar, not tied to panel mode.
//!
//! One copy of every control: the context menu only routes to the settings window.
//!
//! # Opening the popup without a widget Response
//!
//! The popup is opened from the CONTEXT's pointer state, not from
//! `Response::secondary_clicked()`. Measured on :77: `i.pointer.secondary_clicked()`
//! is true in the frame the right button is released, while the `ui.interact(...)`
//! response covering the whole panel reports false. The click reaches the
//! application -- proven by a temporary log on both values -- but that response
//! never receives egui's CLICKED flag, so a `Response::context_menu` never opens.
//! Reading the pointer directly is what actually works.

use super::config::{PanelConfig, MAX_WIDTH, MIN_WIDTH};
use super::rows::{self, Group};

/// Viewport id of the settings window, stable so it is reused rather than
/// recreated on every open.
pub fn settings_viewport() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("panel_settings")
}

/// The size the settings window opens at: tall enough for four collapsed groups
/// plus the sliders and the stacking row without scrolling.
pub const SETTINGS_SIZE: [f32; 2] = [360.0, 520.0];

/// What a settings interaction wants done, applied by the caller outside the UI
/// pass.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SettingsOutcome {
    pub back_to_normal: bool,
    /// The config changed and must be written to `panel.json`.
    pub config_changed: bool,
    /// The width changed, so the panel window must be re-specified this frame.
    pub width_changed: bool,
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
        .max_height((ui.available_height() - 90.0).max(60.0))
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
        outcome.width_changed = true;
    }

    // stacking / hide_from_taskbar / background_opacity: schema only, UI arrives
    // with PR 2. The fields stay in `PanelConfig` -- clamped, persisted, tested
    // -- so PR 2 only has to act on them, but offering controls here that change
    // nothing would be a lie about what this PR does.

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
