//! Panel configuration: its own file, its own schema, no contact with the tray.
//!
//! The panel gets `~/.config/lapsphere/panel.json`, deliberately separate from
//! `settings.json`. Two reasons, both from the owner's decision:
//!
//! * `settings.json` is what the Settings page and the tray own. The panel is
//!   a separate surface with a separate lifecycle, and a corrupt or hand-edited
//!   panel file must not be able to take the tray's settings with it.
//! * The tray's settings are not modified by any of this. A panel release must
//!   be revertible without touching tray behaviour.
//!
//! Shape follows `settings.json`: `#[serde(default)]` at the container level so
//! a partial or older file loads, and a `normalize` step so a hand-edited file
//! cannot produce a panel that is missing elements or double-lists them.
//! Normalization mirrors `statistics::normalize_section_order`: keep the known
//! ids in the user's order, drop unknown and duplicate ids, then append every
//! id the code knows about that the file did not mention.

use serde::{Deserialize, Serialize};

/// Bumped when the on-disk shape changes incompatibly.
pub const PANEL_CONFIG_VERSION: u32 = 1;

/// Every panel element this build knows about, in default display order.
///
/// The three frame elements (`fps`, `fps_graph`, `frametime_graph`) are present
/// but have no data source yet: they render `—` in a reserved slot until the
/// Vulkan layer lands. They are listed here so the reserved slots exist from the
/// first release and the layout does not change when the data arrives.
pub const PANEL_ITEMS: &[(&str, &str)] = &[
    ("cpu_name", "CPU"),
    ("cpu_freq", "CPU"),
    ("cpu_load", "CPU"),
    ("gpu_name", "GPU"),
    ("gpu_clock", "GPU"),
    ("gpu_load", "GPU"),
    ("gpu_mem_clock", "VRAM"),
    ("gpu_mem_usage", "VRAM"),
    ("ram_usage", "RAM"),
    ("ram_type", "RAM"),
    ("ram_freq", "RAM"),
    ("gamepad", "PAD"),
    ("fps", "FPS"),
    ("fps_graph", "FPS"),
    ("frametime_graph", "FRAME"),
];

/// Default global hotkey. `Shift_R` = right shift, `F9` = F9.
///
/// F9, not F10: F10 is the "activate the menu bar" key in the X11 default
/// keymap, so grabbing it would fight the toolkit's own binding on some
/// desktops. F9 has no default binding in the base keymap.
pub const DEFAULT_HOTKEY: &str = "Shift_R+F9";

/// Default hotkey that turns click-through back off.
///
/// Separate from [`DEFAULT_HOTKEY`], because the two solve opposite problems:
/// the show/hide key is needed when the panel is *hidden*, the interactivity key
/// when it is *transparent to the mouse*. F8, and again right shift, so the two
/// are distinguishable by feel. F8 has no default binding in the base keymap.
pub const DEFAULT_INTERACTIVITY_HOTKEY: &str = "Shift_R+F8";

/// How the panel window stacks against other windows.
///
/// `always_on_top` is the default because that is what a HUD panel is for. The
/// `normal` value exists because an overlay that covers a fullscreen game is not
/// always what the user wants — sometimes the panel is an ordinary window that
/// can be covered, minimized and found in the taskbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelStacking {
    /// `WindowLevel(AlwaysOnTop)` plus the EWMH atoms.
    AlwaysOnTop,
    /// `WindowLevel(Normal)`, no atoms, ordinary stacking.
    Normal,
}

impl Default for PanelStacking {
    fn default() -> Self {
        PanelStacking::AlwaysOnTop
    }
}

impl PanelStacking {
    /// Every value, in menu order.
    pub const ALL: [PanelStacking; 2] = [PanelStacking::AlwaysOnTop, PanelStacking::Normal];

    /// The label the menus show.
    pub fn label(self) -> &'static str {
        match self {
            PanelStacking::AlwaysOnTop => "Always on top",
            PanelStacking::Normal => "Normal window",
        }
    }

    /// The exact string written to `panel.json`.
    ///
    /// Spelled out rather than derived from `label()`, because the label is UI
    /// text and the stored value is a wire format that must not drift with it.
    pub fn stored_value(self) -> &'static str {
        match self {
            PanelStacking::AlwaysOnTop => "always_on_top",
            PanelStacking::Normal => "normal",
        }
    }

    pub fn is_always_on_top(self) -> bool {
        matches!(self, PanelStacking::AlwaysOnTop)
    }

    /// Parse a stored value.
    ///
    /// An **unknown** value yields `AlwaysOnTop` rather than an error, and that is
    /// deliberate: a build that does not know a newer mode must not fall back to
    /// the *whole* default panel, which would silently discard the user's element
    /// order, labels and position. Only this one field degrades.
    pub fn from_stored(raw: &str) -> Self {
        match raw.trim() {
            "normal" => PanelStacking::Normal,
            // `always_on_top`, and every unrecognised string.
            _ => PanelStacking::AlwaysOnTop,
        }
    }
}

impl<'de> Deserialize<'de> for PanelStacking {
    /// Degrade an unusable value to `always_on_top` — see [`PanelStacking::from_stored`].
    ///
    /// Hand-written rather than derived, so an unknown string is not an error: a
    /// failing parse would make the whole `panel.json` load fail and discard the
    /// user's element order, labels and position, which is a far larger loss than
    /// choosing the safe default for one field.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = serde_json::Value::deserialize(deserializer).map_err(serde::de::Error::custom)?;
        let text = match raw {
            serde_json::Value::String(text) => text,
            serde_json::Value::Null => return Ok(PanelStacking::default()),
            other => other.to_string(),
        };
        Ok(PanelStacking::from_stored(&text))
    }
}

/// One element's configuration: display order is array order in `items`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelItemConfig {
    pub id: String,
    /// An element with `visible: false` is configured but not laid out.
    pub visible: bool,
    /// `None` uses the built-in label from the item registry.
    pub label: Option<String>,
    /// `None` inherits the global `font_scale`.
    pub font_scale: Option<f32>,
}

impl Default for PanelItemConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            visible: true,
            label: None,
            font_scale: None,
        }
    }
}

impl PanelItemConfig {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            ..Default::default()
        }
    }
}

/// Which screen corner the panel is anchored to.
///
/// A corner, not an absolute point: an absolute position from a different
/// monitor resolution would land the panel off-screen, and the panel must never
/// be unplaceable. The offset is measured from that corner of the monitor's
/// work area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelCorner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Default for PanelCorner {
    fn default() -> Self {
        Self::TopLeft
    }
}

/// Panel placement: a corner plus an offset from it, in points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelPosition {
    pub corner: PanelCorner,
    pub offset_x: f32,
    pub offset_y: f32,
}

impl Default for PanelPosition {
    fn default() -> Self {
        Self {
            corner: PanelCorner::default(),
            offset_x: 24.0,
            offset_y: 24.0,
        }
    }
}

/// The whole of `panel.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelConfig {
    pub version: u32,
    /// Start the process in panel mode.
    pub active: bool,
    /// Global font scale for every element.
    pub font_scale: f32,
    /// How the panel window stacks against other windows.
    ///
    /// An unknown `stacking` string becomes `always_on_top` rather than failing
    /// the file — see `PanelStacking`'s `Deserialize`.
    pub stacking: PanelStacking,
    /// In `always_on_top` stacking: keep the panel out of the taskbar.
    ///
    /// Only meaningful for `always_on_top`. In `normal` stacking the window is an
    /// ordinary window and IS in the taskbar, which is the point of that mode.
    pub hide_from_taskbar: bool,
    /// The panel does not receive the mouse; clicks go to the window below.
    ///
    /// Off by default. With it on and no way back the panel becomes unreachable,
    /// which is why enabling it is gated on
    /// `crate::panel::menu::can_enable_click_through`.
    pub click_through: bool,
    pub position: PanelPosition,
    /// Hotkey string, e.g. `Shift_R+F9`. Empty means "no hotkey".
    pub hotkey: String,
    /// Hotkey that turns click-through back off, e.g. `Shift_R+F8`.
    ///
    /// Empty means "no interactivity hotkey": allowed, but then
    /// `lapsphere --toggle-interactive` is the only way out of click-through.
    pub interactivity_hotkey: String,
    pub items: Vec<PanelItemConfig>,
}

impl Default for PanelConfig {
    fn default() -> Self {
        Self {
            version: PANEL_CONFIG_VERSION,
            active: false,
            font_scale: 1.0,
            stacking: PanelStacking::default(),
            hide_from_taskbar: true,
            click_through: false,
            position: PanelPosition::default(),
            hotkey: DEFAULT_HOTKEY.to_string(),
            interactivity_hotkey: DEFAULT_INTERACTIVITY_HOTKEY.to_string(),
            items: PANEL_ITEMS
                .iter()
                .map(|(id, _)| PanelItemConfig::new(id))
                .collect(),
        }
    }
}

/// Is `id` an element this build knows about?
pub fn is_known_item(id: &str) -> bool {
    PANEL_ITEMS.iter().any(|(known, _)| *known == id)
}

/// Drop unknown and duplicate ids, keep the user's order, append the rest.
///
/// Same contract as `statistics::normalize_section_order`: an unknown id is
/// skipped rather than rendered as `—` forever, and a file that omits an
/// element still gets it, so a config written by an older build does not
/// silently shrink the panel.
pub fn normalize_items(items: &[PanelItemConfig]) -> Vec<PanelItemConfig> {
    let mut normalized: Vec<PanelItemConfig> = Vec::new();

    for item in items {
        if is_known_item(&item.id) && !normalized.iter().any(|existing| existing.id == item.id) {
            normalized.push(item.clone());
        }
    }

    for (id, _) in PANEL_ITEMS {
        if !normalized.iter().any(|existing| &existing.id == id) {
            normalized.push(PanelItemConfig::new(id));
        }
    }

    normalized
}

/// Clamp values that would make the panel unusable, then normalize the items.
///
/// A hand-edited file is not trusted for anything that could produce a window
/// the user cannot see or operate: font scale is clamped to a sane range, and
/// a non-finite scale (JSON allows `null`/garbage to decode as `NaN` through
/// some paths) falls back to 1.0.
pub fn normalize(mut config: PanelConfig) -> PanelConfig {
    config.version = PANEL_CONFIG_VERSION;

    if !config.font_scale.is_finite() {
        log::warn!("panel.json has a non-finite font_scale, falling back to 1.0");
        config.font_scale = 1.0;
    }
    config.font_scale = config.font_scale.clamp(0.5, 4.0);

    if !config.position.offset_x.is_finite() {
        config.position.offset_x = 0.0;
    }
    if !config.position.offset_y.is_finite() {
        config.position.offset_y = 0.0;
    }
    config.position.offset_x = config.position.offset_x.clamp(-4096.0, 4096.0);
    config.position.offset_y = config.position.offset_y.clamp(-4096.0, 4096.0);

    config.hotkey = config.hotkey.trim().to_string();
    config.interactivity_hotkey = config.interactivity_hotkey.trim().to_string();

    for item in &mut config.items {
        if let Some(scale) = item.font_scale {
            if !scale.is_finite() || scale <= 0.0 {
                item.font_scale = None;
            } else {
                item.font_scale = Some(scale.clamp(0.5, 4.0));
            }
        }
        if let Some(label) = &item.label {
            // An empty label would render as a bare value with no name; treat
            // it as "use the default" instead.
            if label.trim().is_empty() {
                item.label = None;
            }
        }
    }

    config.items = normalize_items(&config.items);
    config
}

/// The ids the panel will actually lay out, in order.
pub fn visible_item_ids(config: &PanelConfig) -> Vec<&str> {
    config
        .items
        .iter()
        .filter(|item| item.visible)
        .map(|item| item.id.as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, visible: bool) -> PanelItemConfig {
        PanelItemConfig {
            visible,
            ..PanelItemConfig::new(id)
        }
    }

    #[test]
    fn defaults_carry_every_known_item_once() {
        let config = PanelConfig::default();
        assert_eq!(config.items.len(), PANEL_ITEMS.len());
        assert_eq!(visible_item_ids(&config).len(), PANEL_ITEMS.len());
        assert_eq!(config.hotkey, "Shift_R+F9");
        assert_eq!(config.stacking, PanelStacking::AlwaysOnTop);
        assert!(config.hide_from_taskbar);
        assert!(!config.click_through);
        assert_eq!(config.interactivity_hotkey, "Shift_R+F8");
        assert!(!config.active, "the panel must not start itself");
    }

    #[test]
    fn the_frame_elements_are_reserved_from_the_first_release() {
        let config = PanelConfig::default();
        let ids = visible_item_ids(&config);
        for id in ["fps", "fps_graph", "frametime_graph"] {
            assert!(ids.contains(&id), "{id} must have a reserved slot");
        }
    }

    #[test]
    fn unknown_ids_are_dropped() {
        let items = vec![
            item("cpu_load", true),
            item("not_a_real_element", true),
            item("fps", true),
        ];
        let normalized = normalize_items(&items);
        let ids: Vec<&str> = normalized.iter().map(|i| i.id.as_str()).collect();

        assert!(
            !ids.contains(&"not_a_real_element"),
            "unknown id must be skipped"
        );
        assert_eq!(
            ids.len(),
            PANEL_ITEMS.len(),
            "the skipped id must be replaced by the missing known ones"
        );
    }

    #[test]
    fn duplicates_are_collapsed_and_order_is_kept() {
        let items = vec![
            item("fps", true),
            item("cpu_load", true),
            item("fps", false),
            item("gpu_load", true),
        ];
        let ids: Vec<String> = normalize_items(&items).into_iter().map(|i| i.id).collect();

        assert_eq!(ids.iter().filter(|id| *id == "fps").count(), 1);
        assert_eq!(ids[0], "fps", "user order is preserved");
        assert_eq!(ids[1], "cpu_load");
        assert_eq!(ids[2], "gpu_load");
    }

    #[test]
    fn the_first_occurrence_wins_for_a_duplicated_id() {
        let items = vec![
            item("fps", false),
            item("cpu_load", true),
            item("fps", true),
        ];
        let normalized = normalize_items(&items);
        let fps = normalized
            .iter()
            .find(|i| i.id == "fps")
            .expect("fps present");
        assert!(!fps.visible, "the first entry's flags win");
    }

    #[test]
    fn an_empty_item_list_gives_the_full_default_panel() {
        let normalized = normalize_items(&[]);
        assert_eq!(normalized.len(), PANEL_ITEMS.len());
        assert_eq!(normalized[0].id, PANEL_ITEMS[0].0);
    }

    #[test]
    fn per_item_flags_survive_normalization() {
        let items = vec![
            PanelItemConfig {
                label: Some("Ryzen".into()),
                font_scale: Some(1.5),
                ..item("cpu_name", true)
            },
            item("cpu_load", false),
        ];
        let normalized = normalize_items(&items);
        let name = normalized.iter().find(|i| i.id == "cpu_name").unwrap();
        assert_eq!(name.label.as_deref(), Some("Ryzen"));
        assert_eq!(name.font_scale, Some(1.5));
        assert!(
            !normalized
                .iter()
                .find(|i| i.id == "cpu_load")
                .unwrap()
                .visible
        );
    }

    #[test]
    fn normalize_clamps_a_hostile_font_scale() {
        let mut config = PanelConfig::default();
        config.font_scale = 1000.0;
        let normalized = normalize(config.clone());
        assert_eq!(normalized.font_scale, 4.0);

        let mut nan = config.clone();
        nan.font_scale = f32::NAN;
        let normalized = normalize(nan);
        assert_eq!(normalized.font_scale, 1.0);
        assert!(normalized.font_scale.is_finite());
    }

    #[test]
    fn normalize_drops_a_non_positive_per_item_scale() {
        let mut config = PanelConfig::default();
        config.items[0].font_scale = Some(-2.0);
        let normalized = normalize(config);
        assert_eq!(normalized.items[0].font_scale, None, "inherit instead");
    }

    #[test]
    fn normalize_treats_an_empty_label_as_unset() {
        let mut config = PanelConfig::default();
        config.items[0].label = Some("   ".into());
        let normalized = normalize(config);
        assert_eq!(normalized.items[0].label, None);
    }

    #[test]
    fn normalize_clamps_a_wild_position_offset() {
        let mut config = PanelConfig::default();
        config.position.offset_x = 1e9;
        let normalized = normalize(config);
        assert_eq!(normalized.position.offset_x, 4096.0);
    }

    #[test]
    fn normalize_stamps_the_current_version() {
        let mut config = PanelConfig::default();
        config.version = 0;
        assert_eq!(normalize(config).version, PANEL_CONFIG_VERSION);
    }

    #[test]
    fn visible_item_ids_respects_visibility_and_order() {
        let config = PanelConfig {
            items: normalize_items(&[
                item("fps", true),
                item("cpu_load", false),
                item("gpu_load", true),
            ]),
            ..Default::default()
        };
        // The three configured-visible ids come first in the user's order; the
        // rest append in PANEL_ITEMS order, so cpu_name/cpu_freq (which come
        // before gpu_name in the registry) land after the user's block.
        assert_eq!(
            visible_item_ids(&config),
            vec![
                "fps",
                "gpu_load",
                "cpu_name",
                "cpu_freq",
                "gpu_name",
                "gpu_clock",
                "gpu_mem_clock",
                "gpu_mem_usage",
                "ram_usage",
                "ram_type",
                "ram_freq",
                "gamepad",
                "fps_graph",
                "frametime_graph",
            ]
        );
    }

    #[test]
    fn a_partial_file_decodes_and_fills_in() {
        // What a hand-written or older panel.json looks like: only `font_scale`.
        let json = r#"{"font_scale": 1.25}"#;
        let parsed: PanelConfig = serde_json::from_str(json).expect("partial file decodes");
        let normalized = normalize(parsed);
        assert_eq!(normalized.font_scale, 1.25);
        assert_eq!(normalized.items.len(), PANEL_ITEMS.len());
        assert_eq!(normalized.version, PANEL_CONFIG_VERSION);
    }

    #[test]
    fn round_trips_through_json() {
        let config = normalize(PanelConfig::default());
        let json = serde_json::to_string_pretty(&config).expect("serialize");
        let back: PanelConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(normalize(back), config);
    }

    #[test]
    fn panel_json_does_not_contain_tray_or_settings_keys() {
        // The owner's decision: panel settings live in their own file and the
        // tray's settings are not modified. Assert the schema cannot express
        // them, so a future field cannot quietly reintroduce the coupling.
        let json = serde_json::to_string(&PanelConfig::default()).expect("serialize");
        for forbidden in [
            "tray",
            "theme",
            "autostart",
            "statistics_sections",
            "battery",
        ] {
            assert!(
                !json.contains(forbidden),
                "panel.json must not carry `{forbidden}`"
            );
        }
    }

    // ---- stacking / hide_from_taskbar / click_through / interactivity_hotkey ----

    #[test]
    fn an_unknown_stacking_value_becomes_always_on_top() {
        // The owner's rule: an unrecognised `stacking` degrades to
        // `always_on_top` rather than failing the file. A failing file would
        // fall back to the WHOLE default panel and silently discard the user's
        // element order, labels and position — far more destructive than
        // choosing the safe default for one field.
        for raw in [
            "\"floating\"",
            "\"Always_On_Top\"",
            "\"\"",
            "17",
            "true",
            "null",
            "[]",
        ] {
            let json = format!("{{\"stacking\": {raw}}}");
            let parsed: PanelConfig =
                serde_json::from_str(&json).unwrap_or_else(|_| panic!("{json} must decode"));
            assert_eq!(
                parsed.stacking,
                PanelStacking::AlwaysOnTop,
                "{raw} must not silently become normal"
            );
        }
    }

    #[test]
    fn the_two_known_stacking_values_round_trip() {
        for stacking in PanelStacking::ALL {
            let json = serde_json::to_string(&stacking).expect("serialize");
            assert_eq!(json, format!("\"{}\"", stacking.stored_value()));
            let parsed: PanelStacking = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(parsed, stacking);
        }
    }

    #[test]
    fn normal_stacking_is_read_from_the_file() {
        let parsed: PanelConfig =
            serde_json::from_str(r#"{"stacking": "normal"}"#).expect("decodes");
        assert_eq!(parsed.stacking, PanelStacking::Normal);
        assert_eq!(PanelStacking::Normal.stored_value(), "normal");
        assert_eq!(PanelStacking::AlwaysOnTop.stored_value(), "always_on_top");
    }

    #[test]
    fn the_new_fields_round_trip_through_json() {
        let config = PanelConfig {
            stacking: PanelStacking::Normal,
            hide_from_taskbar: false,
            click_through: true,
            interactivity_hotkey: "Shift_L+F8".into(),
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&config).expect("serialize");
        for key in [
            "\"stacking\"",
            "\"hide_from_taskbar\"",
            "\"click_through\"",
            "\"interactivity_hotkey\"",
        ] {
            assert!(json.contains(key), "panel.json must carry {key}");
        }
        let back: PanelConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.stacking, PanelStacking::Normal);
        assert!(!back.hide_from_taskbar);
        assert!(back.click_through);
        assert_eq!(back.interactivity_hotkey, "Shift_L+F8");
        assert_eq!(normalize(back), normalize(config));
    }

    #[test]
    fn an_older_file_without_the_new_fields_loads_with_the_documented_defaults() {
        // A panel.json written before this change has none of the four keys.
        let json = r#"{"version": 1, "active": true, "font_scale": 1.0, "hotkey": "Shift_R+F9",
                        "position": {"corner": "bottom_right", "offset_x": 10.0, "offset_y": 10.0},
                        "items": [{"id": "cpu_load", "visible": true, "label": null, "font_scale": null}]}"#;
        let config = normalize(serde_json::from_str::<PanelConfig>(json).expect("decodes"));
        assert_eq!(config.stacking, PanelStacking::AlwaysOnTop);
        assert!(config.hide_from_taskbar);
        assert!(!config.click_through);
        assert_eq!(config.interactivity_hotkey, DEFAULT_INTERACTIVITY_HOTKEY);
        // The user's own data survived, which is the reason the field-level
        // deserializer exists.
        assert_eq!(config.items[0].id, "cpu_load");
        assert_eq!(config.position.corner, PanelCorner::BottomRight);
    }

    #[test]
    fn a_partial_file_never_invents_the_unsafe_click_through_state() {
        // `click_through` defaults to false and normalize does not touch it: the
        // file records the user's last choice, but a file that says nothing must
        // land on the safe side.
        let parsed: PanelConfig = serde_json::from_str(r#"{"font_scale": 1.2}"#).expect("decodes");
        let config = normalize(parsed);
        assert!(!config.click_through);
        assert_eq!(config.font_scale, 1.2);
    }

    #[test]
    fn the_interactivity_hotkey_is_trimmed_and_may_be_emptied() {
        let mut padded = PanelConfig::default();
        padded.interactivity_hotkey = "  Shift_R+F8  ".into();
        assert_eq!(normalize(padded).interactivity_hotkey, "Shift_R+F8");

        // Empty means "no interactivity key". The safety rule accounts for that,
        // so normalize must not silently refill it with the default.
        let mut cleared = PanelConfig::default();
        cleared.interactivity_hotkey = "   ".into();
        assert_eq!(normalize(cleared).interactivity_hotkey, "");
    }
}
