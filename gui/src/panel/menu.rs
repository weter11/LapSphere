//! The panel's own menu: the way back to the normal window, and the per-element
//! controls.
//!
//! Two rules the owner's decision imposes:
//!
//! * **There is no return path except an explicit control.** The panel hides and
//!   shows on a hotkey, but leaving panel mode is only ever done by pressing the
//!   back button here (or the equivalent in the normal window's menu, added in a
//!   later commit). Nothing else silently returns the user to the main window.
//! * **A panel with no way out is never created** — see `can_hide`. If the panel
//!   could be hidden with no key grabbed, no tray and no CLI, then hiding it
//!   would be a one-way trip, so the hide command falls back to returning to the
//!   normal window instead.
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
    /// Set the hotkey string.
    SetHotkey(String),
    /// Toggle click-through.
    ToggleClickThrough,
    /// Reset the position to the default corner and offset.
    ResetPosition,
    /// Return to the normal window.
    BackToNormal,
}

/// Draw the panel: the element strip plus the back button.
///
/// `pending_mode` is written rather than acted on here, so the actual viewport
/// change happens at the top of the next frame instead of inside a click
/// handler, where viewport commands are not safe.
pub fn draw(
    ui: &mut egui::Ui,
    state: &AppState,
    config: &mut PanelConfig,
    pending_mode: &mut Option<Mode>,
    tray_enabled: bool,
) {
    ui.horizontal(|ui| {
        // The back button is at a fixed leading position so it does not move as
        // elements are added or removed. It is the only in-panel route back to
        // the normal window.
        if ui.button("⬅ Normal mode").clicked() {
            *pending_mode = Some(Mode::Normal);
        }
        ui.separator();
        crate::panel::x11::draw_elements(ui, state, config);
    });

    let _ = tray_enabled;
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
        MenuAction::ToggleClickThrough => config.click_through = !config.click_through,
        MenuAction::ResetPosition => config.position = Default::default(),
        MenuAction::BackToNormal => {}
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
    use crate::panel::config::{self, PanelConfig};

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
        apply(&mut config, MenuAction::ToggleClickThrough);
        apply(&mut config, MenuAction::ResetPosition);
        apply(&mut config, MenuAction::BackToNormal);

        assert_eq!(config.font_scale, 1.5);
        assert_eq!(config.hotkey, "F9");
        assert!(config.click_through);
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
