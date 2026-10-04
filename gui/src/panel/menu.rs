//! The panel's own menu: the right-click context menu, the settings window, and
//! the rules that decide what the user is allowed to do from them.
//!
//! Three rules the owner's decision imposes:
//!
//! * **There is no return path to the normal window except an explicit
//!   control.** The panel hides and shows on a hotkey, but leaving panel mode is
//!   only ever done by pressing the button in the context menu. Nothing else
//!   silently returns the user to the main window.
//! * **A panel with no way out is never created** — see `can_hide`. If the panel
//!   could be hidden with no key grabbed, no tray and no CLI, then hiding it
//!   would be a one-way trip, so the hide command falls back to returning to the
//!   normal window instead.
//! * **Click-through may only be switched on when there is a way to switch it
//!   off** — see `can_enable_click_through`. With `MousePassthrough` set the
//!   panel does not receive the mouse, so the context menu cannot be opened; if
//!   neither the interactivity hotkey nor the CLI is available, turning
//!   click-through on would leave a permanently unreachable panel.
//!
//! The element list follows the settings page's reorder pattern
//! (`pages/settings.rs` Section Order: collect the move request inside the loop,
//! apply it after, so swapping twice in one frame cannot fight itself).

use super::config::{PanelConfig, PanelItemConfig};
use super::items;
use super::window::Mode;
use crate::app::AppState;

/// Is there a way to bring the panel back after it is hidden?
///
/// The owner's safety rule: hiding is only allowed when a return path exists.
/// Any one of these counts, because each is an independent way back:
///
/// * the hotkey is grabbed (X11 `XGrabKey` succeeded), or
/// * the tray is enabled, or
/// * the `lapsphere --toggle-panel` CLI works, which needs the D-Bus method.
pub fn can_hide(hotkey_grabbed: bool, tray_enabled: bool, dbus_available: bool) -> bool {
    hotkey_grabbed || tray_enabled || dbus_available
}

/// What a hide request should actually do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HideOutcome {
    /// Safe to hide: a return path exists.
    Hide,
    /// Unsafe to hide: return to the normal window instead, so the user is not
    /// stranded in a panel they cannot bring back.
    ReturnToNormal,
}

pub fn hide_outcome(hotkey_grabbed: bool, tray_enabled: bool, dbus_available: bool) -> HideOutcome {
    if can_hide(hotkey_grabbed, tray_enabled, dbus_available) {
        HideOutcome::Hide
    } else {
        HideOutcome::ReturnToNormal
    }
}

/// Is there a way to turn click-through back off once it is on?
///
/// The mirror image of [`can_hide`], and for the same reason. With
/// `ViewportCommand::MousePassthrough(true)` the window stops taking mouse input,
/// so the context menu — the only place click-through can be switched off from
/// the panel — becomes unreachable. What is left is the interactivity hotkey
/// (which is an X11 root grab, so it works precisely because the panel is not
/// focused) and `lapsphere --toggle-interactive` over D-Bus.
///
/// The tray is deliberately **not** counted here: the tray is a separate window
/// whose menu the panel cannot influence, and the owner's rule names the key and
/// the CLI.
pub fn can_enable_click_through(interactivity_grabbed: bool, cli_available: bool) -> bool {
    interactivity_grabbed || cli_available
}

/// Why the click-through checkbox is unavailable, in the user's words.
pub fn click_through_blocked_reason(
    interactivity_grabbed: bool,
    cli_available: bool,
) -> &'static str {
    if can_enable_click_through(interactivity_grabbed, cli_available) {
        ""
    } else {
        "Click-through can only be enabled with a way back: capture the interactivity \
         hotkey (see Interactivity key below) or make `lapsphere --toggle-interactive` \
         available. With click-through on and no way back, this panel could not be \
         clicked again."
    }
}

/// A change the menu wants applied to the config, applied after the draw.
pub enum MenuAction {
    /// Move an element up or down in display order.
    Move { from: usize, to: usize },
    /// Toggle an element's visibility.
    ToggleVisible { index: usize },
    /// Set an element's label.
    SetLabel { index: usize, label: String },
    /// Set an element's per-item font scale.
    SetItemScale { index: usize, scale: Option<f32> },
    /// Set the global font scale.
    SetFontScale(f32),
    /// Set the show/hide hotkey string.
    SetHotkey(String),
    /// Set the interactivity hotkey string.
    SetInteractivityHotkey(String),
    /// Set the stacking mode.
    SetStacking(super::config::PanelStacking),
    /// Set whether an always-on-top panel is hidden from the taskbar.
    SetHideFromTaskbar(bool),
    /// Turn click-through on or off.
    SetClickThrough(bool),
    /// Toggle click-through.
    ToggleClickThrough,
    /// Reset the position to the default corner and offset.
    ResetPosition,
    /// Return to the normal window.
    BackToNormal,
    /// Hide the panel (subject to the safety rule).
    Hide,
}

/// What the context menu produced, for the caller to act on outside the UI pass.
#[derive(Debug, Default)]
pub struct MenuOutcome {
    /// Return to the normal window.
    pub back_to_normal: bool,
    /// Hide the panel, if the safety rule allows it.
    pub hide: bool,
    /// Open the settings window.
    pub open_settings: bool,
    /// The left button started a window drag: the caller must re-read the
    /// window's position afterwards and write it back as corner + offset.
    pub dragged: bool,
}

/// Draw the panel surface: the element strip and its right-click menu.
///
/// Actions are collected during the draw and applied afterwards, following the
/// settings page's reorder pattern. `pending_mode` is written rather than acted on
/// here: viewport commands are not safe from inside a click handler, so the mode
/// change happens at the top of the next frame.
pub fn draw(
    ui: &mut egui::Ui,
    state: &AppState,
    config: &mut PanelConfig,
    pending_mode: &mut Option<Mode>,
    visibility: &super::visibility::Visibility,
    tray_enabled: bool,
    interactivity_grabbed: bool,
    cli_available: bool,
) -> MenuOutcome {
    let mut actions: Vec<MenuAction> = Vec::new();
    let mut outcome = MenuOutcome::default();

    ui.horizontal(|ui| {
        // The back button is at a fixed leading position so it does not move as
        // elements are added or removed. It is the explicit route back to the
        // normal window: there is no way back except an explicit control.
        if ui.button("⬅ Normal mode").clicked() {
            actions.push(MenuAction::BackToNormal);
        }
        ui.separator();

        // The whole strip is the right-click target. `Sense::click` covers both
        // buttons: secondary gives the context menu, primary starts the window
        // drag below.
        let response = super::x11::draw_strip(ui, state, config);

        if response.clicked() {
            // `StartDrag` hands the pointer to the window manager, so the panel
            // keeps its decorations-free border-drag behaviour while the user
            // repositions it with the left button. The new position is written
            // back into `panel.json` by the caller from the viewport rect.
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            outcome.dragged = true;
        }

        response.context_menu(|ui| {
            draw_context_menu(
                ui,
                state,
                config,
                visibility,
                tray_enabled,
                interactivity_grabbed,
                cli_available,
                &mut actions,
                &mut outcome,
            );
        });
    });

    for action in actions {
        match action {
            MenuAction::BackToNormal => *pending_mode = Some(Mode::Normal),
            MenuAction::Move { from, to } => {
                if from < config.items.len() && to < config.items.len() {
                    config.items.swap(from, to);
                }
            }
            MenuAction::Hide => outcome.hide = true,
            MenuAction::SetClickThrough(value) => {
                // The gate is enforced here as well as in the checkbox's enabled
                // state: a stale UI must not be able to write an unreachable
                // panel into `panel.json`.
                if !value || can_enable_click_through(interactivity_grabbed, cli_available) {
                    config.click_through = value;
                } else {
                    log::warn!("panel: refused click-through: no interactivity hotkey and no CLI");
                }
            }
            other => apply(config, other),
        }
    }

    if let Err(err) = super::save_panel_config(config) {
        log::warn!("panel: could not write panel.json: {err}");
    }

    outcome
}

/// The right-click menu: element visibility, stacking, click-through, the route
/// back, and the way into the settings window.
#[allow(clippy::too_many_arguments)]
fn draw_context_menu(
    ui: &mut egui::Ui,
    state: &AppState,
    config: &mut PanelConfig,
    visibility: &super::visibility::Visibility,
    tray_enabled: bool,
    interactivity_grabbed: bool,
    cli_available: bool,
    actions: &mut Vec<MenuAction>,
    outcome: &mut MenuOutcome,
) {
    // ---- Elements: one visibility checkbox each, with the live value ----
    egui::ScrollArea::vertical()
        .max_height(320.0)
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Elements").strong());
            let total = config.items.len();
            for index in 0..total {
                let id = config.items[index].id.clone();
                let label = items::find(&id)
                    .map(|item| item.default_label)
                    .unwrap_or(id.as_str());
                let current = super::x11::menu_value(state, &id);
                ui.horizontal(|ui| {
                    if ui
                        .checkbox(&mut config.items[index].visible, label)
                        .changed()
                    {
                        actions.push(MenuAction::ToggleVisible { index });
                    }
                    ui.label(egui::RichText::new(current).monospace().weak());
                });
            }
        });

    ui.separator();

    // ---- Stacking and taskbar ----
    ui.label(egui::RichText::new("Window").strong());
    let mut stacking = config.stacking;
    egui::ComboBox::from_id_salt("panel_stacking")
        .selected_text(stacking.label())
        .show_ui(ui, |ui| {
            for option in super::config::PanelStacking::ALL {
                ui.selectable_value(&mut stacking, option, option.label());
            }
        });
    if stacking != config.stacking {
        actions.push(MenuAction::SetStacking(stacking));
    }
    if config.stacking.is_always_on_top() {
        let mut hide = config.hide_from_taskbar;
        if ui
            .checkbox(&mut hide, "Hide from taskbar")
            .on_hover_text("Only applies while the panel is always on top")
            .changed()
        {
            actions.push(MenuAction::SetHideFromTaskbar(hide));
        }
    }

    // ---- Click-through, gated on a way back ----
    ui.separator();
    let can_enable = can_enable_click_through(interactivity_grabbed, cli_available);
    let mut click_through = config.click_through;
    let response = ui
        .add_enabled(
            can_enable || click_through,
            egui::Checkbox::new(&mut click_through, "Click-through"),
        )
        .on_hover_text("Clicks pass to the window below the panel");
    if response.changed() {
        actions.push(MenuAction::SetClickThrough(click_through));
    }
    if !can_enable && !config.click_through {
        ui.label(
            egui::RichText::new(click_through_blocked_reason(
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
                "The panel ignores the mouse. Interactivity key or \
                     `lapsphere --toggle-interactive` brings it back.",
            )
            .small()
            .italics(),
        );
    }

    ui.separator();

    // ---- Routes out ----
    if ui.button("Panel settings…").clicked() {
        outcome.open_settings = true;
    }
    if ui.button("⬅ Normal mode").clicked() {
        actions.push(MenuAction::BackToNormal);
    }
    let hide_label = format!("Hide ({})", hotkey_label(&config.hotkey));
    let can_hide_now = can_hide(visibility.hotkey_grabbed(), tray_enabled, cli_available);
    if ui
        .add_enabled(can_hide_now, egui::Button::new(hide_label))
        .on_hover_text(if can_hide_now {
            "Hide the panel; the hotkey or `lapsphere --toggle-panel` brings it back"
        } else {
            "Not available: no hotkey, no tray and no CLI to bring the panel back"
        })
        .clicked()
    {
        actions.push(MenuAction::Hide);
    }
    if !can_hide_now {
        ui.label(
            egui::RichText::new(
                "Hiding is unavailable: no hotkey captured, tray off and no D-Bus CLI. \
                     Leaving panel mode is still possible with Normal mode above.",
            )
            .small()
            .italics(),
        );
    }
}

/// The hotkey as shown in the menu, or a fallback when none is configured.
fn hotkey_label(hotkey: &str) -> String {
    if hotkey.trim().is_empty() {
        "no hotkey".to_string()
    } else {
        hotkey.trim().to_string()
    }
}

/// Apply a menu action to the config.
pub fn apply(config: &mut PanelConfig, action: MenuAction) {
    match action {
        MenuAction::Move { from, to } => {
            if from < config.items.len() && to < config.items.len() {
                config.items.swap(from, to);
            }
        }
        MenuAction::ToggleVisible { index } => {
            if let Some(item) = config.items.get_mut(index) {
                item.visible = !item.visible;
            }
        }
        MenuAction::SetLabel { index, label } => {
            if let Some(item) = config.items.get_mut(index) {
                let trimmed = label.trim().to_string();
                item.label = if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                };
            }
        }
        MenuAction::SetItemScale { index, scale } => {
            if let Some(item) = config.items.get_mut(index) {
                item.font_scale = scale.filter(|s| s.is_finite() && *s > 0.0);
            }
        }
        MenuAction::SetFontScale(scale) => config.font_scale = scale,
        MenuAction::SetHotkey(key) => config.hotkey = key,
        MenuAction::SetInteractivityHotkey(key) => config.interactivity_hotkey = key,
        MenuAction::SetStacking(stacking) => config.stacking = stacking,
        MenuAction::SetHideFromTaskbar(hide) => config.hide_from_taskbar = hide,
        MenuAction::SetClickThrough(value) => config.click_through = value,
        MenuAction::ToggleClickThrough => config.click_through = !config.click_through,
        MenuAction::ResetPosition => config.position = Default::default(),
        MenuAction::BackToNormal => {}
        MenuAction::Hide => {}
    }
}

/// Move an element up (`delta < 0`) or down, by id, leaving the rest in order.
pub fn move_item(config: &mut PanelConfig, id: &str, delta: isize) -> bool {
    let Some(from) = config.items.iter().position(|item| item.id == id) else {
        return false;
    };
    let to = from as isize + delta;
    if to < 0 || to >= config.items.len() as isize {
        return false;
    }
    config.items.swap(from, to as usize);
    true
}

/// Set visibility by id.
pub fn set_visible(config: &mut PanelConfig, id: &str, visible: bool) -> bool {
    match config.items.iter_mut().find(|item| item.id == id) {
        Some(item) => {
            item.visible = visible;
            true
        }
        None => false,
    }
}

/// Set a label by id.
pub fn set_label(config: &mut PanelConfig, id: &str, label: &str) -> bool {
    let trimmed = label.trim();
    let value = if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    };
    match config.items.iter_mut().find(|item| item.id == id) {
        Some(item) => {
            item.label = value;
            true
        }
        None => false,
    }
}

/// The default label an element would show with no override.
pub fn default_label(id: &str) -> &'static str {
    items::find(id).map(|item| item.default_label).unwrap_or("")
}

/// A blank item config for a known id, used when adding an element.
pub fn new_item(id: &str) -> Option<PanelItemConfig> {
    items::find(id).map(|_| PanelItemConfig::new(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::config::{self, PanelConfig, PanelStacking};

    #[test]
    fn hiding_is_safe_when_the_key_is_grabbed() {
        assert!(can_hide(true, false, false));
        assert_eq!(hide_outcome(true, false, false), HideOutcome::Hide);
    }

    #[test]
    fn hiding_is_safe_with_the_tray() {
        assert!(can_hide(false, true, false));
    }

    #[test]
    fn hiding_is_safe_with_the_dbus_cli() {
        assert!(can_hide(false, false, true));
    }

    #[test]
    fn hiding_is_refused_with_no_return_path_at_all() {
        // No key, no tray, no CLI: hiding would strand the user in a panel they
        // cannot bring back, so the request becomes "return to normal" instead.
        assert!(!can_hide(false, false, false));
        assert_eq!(
            hide_outcome(false, false, false),
            HideOutcome::ReturnToNormal
        );
    }

    // ---- The click-through safety rule ----

    #[test]
    fn click_through_needs_a_way_back() {
        // Neither the key nor the CLI: enabling it would leave a panel that
        // cannot be clicked again.
        assert!(!can_enable_click_through(false, false));
        assert_eq!(
            can_enable_click_through(false, false),
            can_enable_click_through(false, false)
        );
    }

    #[test]
    fn click_through_is_allowed_with_either_way_back() {
        assert!(can_enable_click_through(true, false), "the key is enough");
        assert!(can_enable_click_through(false, true), "the CLI is enough");
        assert!(can_enable_click_through(true, true));
    }

    #[test]
    fn the_tray_does_not_count_as_a_way_back_out_of_click_through() {
        // Deliberate, and asserted so a future change is a conscious one: the
        // owner's rule names the interactivity key and the CLI. The tray is a
        // separate window whose menu the panel cannot drive.
        assert!(
            can_hide(false, true, false),
            "the tray is a way back from hidden"
        );
        assert!(
            !can_enable_click_through(false, false),
            "but it is not a way back from click-through"
        );
    }

    #[test]
    fn the_blocked_reason_is_empty_exactly_when_the_gate_is_open() {
        for grabbed in [false, true] {
            for cli in [false, true] {
                let reason = click_through_blocked_reason(grabbed, cli);
                assert_eq!(
                    reason.is_empty(),
                    can_enable_click_through(grabbed, cli),
                    "grabbed={grabbed} cli={cli}"
                );
                if !reason.is_empty() {
                    assert!(reason.contains("--toggle-interactive"));
                    assert!(reason.contains("hotkey"));
                }
            }
        }
    }

    #[test]
    fn the_hide_label_falls_back_when_no_hotkey_is_configured() {
        assert_eq!(hotkey_label("Shift_R+F9"), "Shift_R+F9");
        assert_eq!(hotkey_label("  "), "no hotkey");
    }

    #[test]
    fn moving_an_element_changes_its_index_and_keeps_the_rest() {
        let mut config = config::normalize(PanelConfig::default());
        let first = config.items[0].id.clone();
        let second = config.items[1].id.clone();

        assert!(move_item(&mut config, &first, 1));
        assert_eq!(config.items[0].id, second);
        assert_eq!(config.items[1].id, first);
    }

    #[test]
    fn moving_past_the_ends_is_refused() {
        let mut config = config::normalize(PanelConfig::default());
        let first = config.items[0].id.clone();
        let last = config.items[config.items.len() - 1].id.clone();

        assert!(
            !move_item(&mut config, &first, -1),
            "cannot move up from the top"
        );
        assert!(
            !move_item(&mut config, &last, 1),
            "cannot move down from the end"
        );
    }

    #[test]
    fn moving_an_unknown_id_is_refused() {
        let mut config = config::normalize(PanelConfig::default());
        assert!(!move_item(&mut config, "not_an_element", 1));
    }

    #[test]
    fn visibility_and_labels_are_settable_by_id() {
        let mut config = config::normalize(PanelConfig::default());

        assert!(set_visible(&mut config, "fps_graph", false));
        let fps_graph = config.items.iter().find(|i| i.id == "fps_graph").unwrap();
        assert!(!fps_graph.visible);

        assert!(set_label(&mut config, "cpu_load", "  Ryzen  "));
        let cpu = config.items.iter().find(|i| i.id == "cpu_load").unwrap();
        assert_eq!(cpu.label.as_deref(), Some("Ryzen"), "label is trimmed");

        assert!(!set_visible(&mut config, "not_an_element", true));
        assert!(!set_label(&mut config, "not_an_element", "x"));
    }

    #[test]
    fn an_empty_label_clears_the_override() {
        let mut config = config::normalize(PanelConfig::default());
        set_label(&mut config, "cpu_load", "Ryzen");
        set_label(&mut config, "cpu_load", "   ");
        let cpu = config.items.iter().find(|i| i.id == "cpu_load").unwrap();
        assert_eq!(cpu.label, None, "blank means use the default again");
    }

    #[test]
    fn apply_handles_every_action_without_panicking() {
        let mut config = config::normalize(PanelConfig::default());
        let out_of_range = config.items.len() + 10;

        apply(
            &mut config,
            MenuAction::Move {
                from: out_of_range,
                to: 0,
            },
        );
        apply(
            &mut config,
            MenuAction::Move {
                from: 0,
                to: out_of_range,
            },
        );
        apply(
            &mut config,
            MenuAction::ToggleVisible {
                index: out_of_range,
            },
        );
        apply(
            &mut config,
            MenuAction::SetLabel {
                index: out_of_range,
                label: "x".into(),
            },
        );
        apply(
            &mut config,
            MenuAction::SetItemScale {
                index: out_of_range,
                scale: Some(2.0),
            },
        );
        apply(&mut config, MenuAction::SetFontScale(1.5));
        apply(&mut config, MenuAction::SetHotkey("F9".into()));
        apply(&mut config, MenuAction::SetInteractivityHotkey("F8".into()));
        apply(&mut config, MenuAction::SetStacking(PanelStacking::Normal));
        apply(&mut config, MenuAction::SetHideFromTaskbar(false));
        apply(&mut config, MenuAction::SetClickThrough(true));
        apply(&mut config, MenuAction::ToggleClickThrough);
        apply(&mut config, MenuAction::ResetPosition);
        apply(&mut config, MenuAction::BackToNormal);
        apply(&mut config, MenuAction::Hide);

        assert_eq!(config.font_scale, 1.5);
        assert_eq!(config.hotkey, "F9");
        assert_eq!(config.interactivity_hotkey, "F8");
        assert_eq!(config.stacking, PanelStacking::Normal);
        assert!(!config.hide_from_taskbar);
        assert!(!config.click_through, "toggled on then off again");
        // The config still normalizes to a complete, usable panel afterwards.
        let normalized = config::normalize(config);
        assert_eq!(normalized.items.len(), config::PANEL_ITEMS.len());
    }

    #[test]
    fn a_non_positive_item_scale_is_dropped_rather_than_stored() {
        let mut config = config::normalize(PanelConfig::default());
        apply(
            &mut config,
            MenuAction::SetItemScale {
                index: 0,
                scale: Some(-1.0),
            },
        );
        assert_eq!(config.items[0].font_scale, None);

        apply(
            &mut config,
            MenuAction::SetItemScale {
                index: 0,
                scale: Some(2.0),
            },
        );
        assert_eq!(config.items[0].font_scale, Some(2.0));
    }

    #[test]
    fn resetting_the_position_restores_the_default_corner() {
        let mut config = config::normalize(PanelConfig::default());
        config.position.corner = config::PanelCorner::BottomRight;
        config.position.offset_x = 500.0;
        apply(&mut config, MenuAction::ResetPosition);
        assert_eq!(config.position, Default::default());
    }

    #[test]
    fn every_element_has_a_default_label() {
        for (id, expected) in config::PANEL_ITEMS {
            assert_eq!(default_label(id), *expected);
            assert!(new_item(id).is_some(), "{id} must be addable");
        }
        assert_eq!(default_label("not_an_element"), "");
        assert!(new_item("not_an_element").is_none());
    }
}
