//! The panel's settings window: a CHILD viewport, not a second app window.
//!
//! Three deliberate constraints:
//!
//! * **A child viewport (`show_viewport_immediate`), not a second root.** The
//!   panel is one window on purpose (ADR-1); a second root window would bring
//!   back the second GL surface and the re-targeting the design doc measured.
//!   A child viewport is a separate OS window without a second GL context.
//! * **Window type `Normal`, and never always-on-top.** The settings window is
//!   something the user opens *from* the panel; if it were always-on-top it
//!   would sit above the panel, which is the opposite of where the eye is. It
//!   inherits the panel's `stacking` for `WindowLevel` only in the sense that it
//!   uses the default (`Normal`) — stated explicitly in the builder rather than
//!   left to the default.
//! * **The root window's own type is never touched.** Only this child is created;
//!   `apply_spec` on the root is unchanged in that respect.
//!
//! Everything written here goes to `panel.json` and nowhere else: no field of
//! `settings.json` and no field of `tray.json` is read or written.

use super::config::{PanelConfig, PanelCorner, PanelStacking};
use super::items;
use super::visibility::Visibility;
use crate::app::AppState;

/// The child viewport id, fixed so the window is reused rather than recreated on
/// every open: a new id each frame would make egui create and destroy a native
/// window every frame. A function rather than a `const` because
/// `ViewportId::from_hash_of` is not a `const fn`.
///
/// The hash salt is a plain literal, so the id is stable across runs — which is
/// what lets the window manager match a reopened window to the existing one.
pub fn settings_viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("lapsphere-panel-settings")
}

/// Title of the settings window.
pub const SETTINGS_TITLE: &str = "Panel settings";

/// What the settings window asks the caller to do.
#[derive(Debug, Default)]
pub struct SettingsOutcome {
    /// The show/hide hotkey changed and must be re-grabbed.
    pub hotkey_changed: bool,
    /// The interactivity hotkey changed and must be re-grabbed.
    pub interactivity_hotkey_changed: bool,
    /// `stacking` or `hide_from_taskbar` changed: the window must be updated
    /// without being recreated.
    pub stacking_changed: bool,
}

/// Draw the settings window, if it is open.
///
/// Returns `None` when the window is closed. Every change is written to
/// `panel.json` at the end of the pass, once, rather than per widget: a text
/// field fires `changed()` on every keystroke and writing the file that often
/// is a lot of I/O for no benefit.
pub fn draw(
    ctx: &egui::Context,
    state: &AppState,
    config: &mut PanelConfig,
    visibility: &Visibility,
    open: &mut bool,
    interactivity_grabbed: bool,
    cli_available: bool,
) -> Option<SettingsOutcome> {
    if !*open {
        return None;
    }

    let mut outcome = SettingsOutcome::default();
    let mut changed = false;

    let builder = egui::ViewportBuilder::default()
        .with_title(SETTINGS_TITLE)
        .with_inner_size([520.0, 640.0])
        .with_min_inner_size([420.0, 480.0])
        // Normal window level, stated rather than inherited: the settings
        // window must never be pinned above the panel it configures.
        .with_window_level(egui::WindowLevel::Normal)
        // Decorated and resizable, and NOT skip-taskbar: this is an ordinary
        // window the user may want to move to a second monitor.
        .with_decorations(true)
        .with_resizable(true);

    let close_requested =
        ctx.show_viewport_immediate(settings_viewport_id(), builder, |ui, _class| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Panel");
                ui.add_space(4.0);

                changed |= window_section(ui, config, &mut outcome);
                ui.separator();
                changed |= hotkeys_section(ui, config, visibility, &mut outcome);
                ui.separator();
                changed |= interactivity_section(ui, config, interactivity_grabbed, cli_available);
                ui.separator();
                changed |= appearance_section(ui, config);
                ui.separator();
                changed |= elements_section(ui, state, config);
            });

            ui.ctx().input(|i| i.viewport().close_requested())
        });

    if close_requested {
        *open = false;
    }

    if changed {
        if let Err(err) = super::save_panel_config(config) {
            log::warn!("panel: could not write panel.json: {err}");
        }
    }

    Some(outcome)
}

/// Stacking, taskbar and position.
fn window_section(
    ui: &mut egui::Ui,
    config: &mut PanelConfig,
    outcome: &mut SettingsOutcome,
) -> bool {
    let mut changed = false;
    ui.label(egui::RichText::new("Window").strong().heading());

    egui::ComboBox::from_id_salt("settings_stacking")
        .selected_text(config.stacking.label())
        .show_ui(ui, |ui| {
            for option in PanelStacking::ALL {
                ui.selectable_value(&mut config.stacking, option, option.label());
            }
        });
    if config.stacking == PanelStacking::Normal {
        ui.label(
            egui::RichText::new(
                "A normal window: it stacks like any other, can be covered, and is listed \
                 in the taskbar. The taskbar checkbox below does not apply.",
            )
            .small()
            .italics(),
        );
    }

    let mut hide_from_taskbar = config.hide_from_taskbar;
    // Disabled rather than hidden while normal stacking is on, so the control
    // does not move when the mode changes — a checkbox that appears and vanishes
    // is how a setting gets forgotten.
    let normal_stacking = config.stacking == PanelStacking::Normal;
    if ui
        .add_enabled(
            !normal_stacking,
            egui::Checkbox::new(&mut hide_from_taskbar, "Hide from taskbar"),
        )
        .changed()
    {
        config.hide_from_taskbar = hide_from_taskbar;
        changed = true;
    }

    ui.horizontal(|ui| {
        ui.label("Corner");
        egui::ComboBox::from_id_salt("settings_corner")
            .selected_text(corner_label(config.position.corner))
            .show_ui(ui, |ui| {
                for corner in [
                    PanelCorner::TopLeft,
                    PanelCorner::TopRight,
                    PanelCorner::BottomLeft,
                    PanelCorner::BottomRight,
                ] {
                    ui.selectable_value(&mut config.position.corner, corner, corner_label(corner));
                }
            });
        if ui.button("Reset position").clicked() {
            config.position = Default::default();
            changed = true;
        }
    });

    if outcome.stacking_changed {
        changed = true;
    }
    changed
}

/// The show/hide hotkey and the interactivity hotkey, with grab status.
fn hotkeys_section(
    ui: &mut egui::Ui,
    config: &mut PanelConfig,
    visibility: &Visibility,
    outcome: &mut SettingsOutcome,
) -> bool {
    let mut changed = false;
    ui.label(egui::RichText::new("Hotkeys").strong().heading());

    ui.horizontal(|ui| {
        ui.label("Show / hide");
        if ui
            .text_edit_singleline(&mut config.hotkey)
            .on_hover_text("Empty disables the key; the tray and CLI still work")
            .changed()
        {
            changed = true;
            outcome.hotkey_changed = true;
        }
    });
    show_grab_status(ui, visibility);

    ui.horizontal(|ui| {
        ui.label("Interactivity");
        if ui
            .text_edit_singleline(&mut config.interactivity_hotkey)
            .on_hover_text("Turns click-through off, e.g. Shift_R+F8. Empty disables it.")
            .changed()
        {
            changed = true;
            outcome.interactivity_hotkey_changed = true;
        }
    });
    show_interactivity_grab_status(ui, visibility);

    changed
}

/// Click-through and the rule that guards it.
fn interactivity_section(
    ui: &mut egui::Ui,
    config: &mut PanelConfig,
    interactivity_grabbed: bool,
    cli_available: bool,
) -> bool {
    let mut changed = false;
    ui.label(egui::RichText::new("Interactivity").strong().heading());

    let can_enable = super::menu::can_enable_click_through(interactivity_grabbed, cli_available);
    let mut click_through = config.click_through;
    let response = ui
        .add_enabled(
            can_enable || click_through,
            egui::Checkbox::new(&mut click_through, "Click-through (clicks pass through)"),
        )
        .on_hover_text("With this on the panel ignores the mouse");
    if response.changed() {
        if click_through && !can_enable {
            log::warn!("panel: refused click-through from the settings window");
        } else {
            config.click_through = click_through;
            changed = true;
        }
    }

    if !can_enable && !config.click_through {
        ui.label(
            egui::RichText::new(super::menu::click_through_blocked_reason(
                interactivity_grabbed,
                cli_available,
            ))
            .small()
            .italics(),
        );
    }
    if config.click_through {
        ui.label(
            egui::RichText::new(
                "While this is on the panel cannot be clicked. Use the interactivity key \
                 or `lapsphere --toggle-interactive`.",
            )
            .small()
            .italics(),
        );
    }

    changed
}

/// Global and per-element font scale, plus labels.
fn appearance_section(ui: &mut egui::Ui, config: &mut PanelConfig) -> bool {
    let mut changed = false;
    ui.label(egui::RichText::new("Text").strong().heading());

    ui.horizontal(|ui| {
        ui.label("Font scale");
        let mut scale = config.font_scale;
        if ui
            .add(egui::Slider::new(&mut scale, 0.5..=4.0).step_by(0.05))
            .changed()
        {
            config.font_scale = scale;
            changed = true;
        }
        if ui.button("Reset").clicked() {
            config.font_scale = 1.0;
            changed = true;
        }
    });

    changed
}

/// Element order, visibility, labels and per-element scale.
///
/// The reorder follows `pages/settings.rs`'s Section Order pattern: the move
/// request is collected inside the loop and applied after it, so two swaps
/// requested in one frame cannot fight each other.
fn elements_section(ui: &mut egui::Ui, state: &AppState, config: &mut PanelConfig) -> bool {
    let mut changed = false;
    ui.label(egui::RichText::new("Elements").strong().heading());

    let total = config.items.len();
    let mut move_request: Option<(usize, usize)> = None;

    for index in 0..total {
        let id = config.items[index].id.clone();
        let label = config.items[index].label.clone();
        let current = super::x11::menu_value(state, &id);

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(super::menu::default_label(&id)).strong());
            ui.label(egui::RichText::new(current).monospace().weak());
            if ui
                .add_enabled(index > 0, egui::Button::new("⬆").small())
                .clicked()
            {
                move_request = Some((index, index - 1));
            }
            if ui
                .add_enabled(index + 1 < total, egui::Button::new("⬇").small())
                .clicked()
            {
                move_request = Some((index, index + 1));
            }
        });

        ui.horizontal(|ui| {
            if ui
                .checkbox(&mut config.items[index].visible, "Visible")
                .changed()
            {
                changed = true;
            }
            ui.label("Label");
            let mut text = label.unwrap_or_default();
            if ui.text_edit_singleline(&mut text).changed() {
                let trimmed = text.trim().to_string();
                config.items[index].label = if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                };
                changed = true;
            }
        });

        ui.horizontal(|ui| {
            ui.label("Scale");
            // `None` means "inherit the global scale", so the slider edits a
            // concrete value defaulted from the global one and the Inherit button
            // puts it back to None.
            let mut scale = config.items[index].font_scale.unwrap_or(config.font_scale);
            if ui
                .add(egui::Slider::new(&mut scale, 0.5..=4.0).step_by(0.05))
                .changed()
            {
                config.items[index].font_scale = Some(scale);
                changed = true;
            }
            if config.items[index].font_scale.is_some() && ui.button("Inherit").clicked() {
                config.items[index].font_scale = None;
                changed = true;
            }
        });

        ui.separator();
    }

    if let Some((from, to)) = move_request {
        if from < config.items.len() && to < config.items.len() {
            config.items.swap(from, to);
            changed = true;
        }
    }

    changed
}

/// The show/hide hotkey's grab status, with the conflict message.
fn show_grab_status(ui: &mut egui::Ui, visibility: &Visibility) {
    let status = visibility.grab_status();
    let conflict = status == super::visibility::GrabStatus::Busy;
    ui.label(egui::RichText::new(status.message()).color(if conflict {
        egui::Color32::from_rgb(220, 160, 60)
    } else {
        ui.visuals().weak_text_color()
    }));
    if conflict {
        ui.label(
            egui::RichText::new(
                "Another application already owns this key. Pick a different one, or use \
                 the tray or `lapsphere --toggle-panel`.",
            )
            .small()
            .italics(),
        );
    }
}

/// The interactivity hotkey's grab status.
fn show_interactivity_grab_status(ui: &mut egui::Ui, visibility: &Visibility) {
    let status = visibility.interactivity_grab_status();
    let conflict = status == super::visibility::GrabStatus::Busy;
    ui.label(
        egui::RichText::new(format!("Interactivity key: {}", status.message())).color(
            if conflict {
                egui::Color32::from_rgb(220, 160, 60)
            } else {
                ui.visuals().weak_text_color()
            },
        ),
    );
    if conflict {
        ui.label(
            egui::RichText::new(
                "Another application owns this key, so it cannot turn click-through off. \
                 Pick another key, or rely on `lapsphere --toggle-interactive`.",
            )
            .small()
            .italics(),
        );
    }
}

/// Human-readable corner name.
fn corner_label(corner: PanelCorner) -> &'static str {
    match corner {
        PanelCorner::TopLeft => "Top left",
        PanelCorner::TopRight => "Top right",
        PanelCorner::BottomLeft => "Bottom left",
        PanelCorner::BottomRight => "Bottom right",
    }
}

/// Every element the settings window can edit, in registry order — the test
/// anchor for "the settings window covers the whole element set".
pub fn editable_ids() -> Vec<&'static str> {
    items::PANEL_ITEMS.iter().map(|item| item.id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_window_covers_every_element() {
        // If the registry grows and the settings window is not updated, this is
        // the test that says so. The window iterates `config.items`, which
        // `normalize` keeps equal to the registry, so equality with the registry
        // is the invariant that matters.
        let config = super::super::config::normalize(PanelConfig::default());
        let from_config: Vec<&str> = config.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(from_config, editable_ids());
    }

    #[test]
    fn the_settings_viewport_id_is_stable() {
        // A fresh id per frame would create and destroy a native window per
        // frame; a fixed id is what makes the window reusable.
        assert_eq!(
            settings_viewport_id(),
            egui::ViewportId::from_hash_of("lapsphere-panel-settings")
        );
        assert_ne!(
            settings_viewport_id(),
            egui::ViewportId::ROOT,
            "the settings window must not be the root viewport"
        );
    }

    #[test]
    fn every_corner_has_a_label() {
        for corner in [
            PanelCorner::TopLeft,
            PanelCorner::TopRight,
            PanelCorner::BottomLeft,
            PanelCorner::BottomRight,
        ] {
            assert!(!corner_label(corner).is_empty());
        }
    }

    #[test]
    fn both_stacking_modes_have_a_label_and_a_stored_value() {
        for stacking in PanelStacking::ALL {
            assert!(!stacking.label().is_empty());
            assert!(!stacking.stored_value().is_empty());
        }
        assert_ne!(
            PanelStacking::AlwaysOnTop.stored_value(),
            PanelStacking::Normal.stored_value()
        );
    }
}
