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
    ("gpu_mem_clock", "GPU"),
    ("gpu_mem_usage", "GPU"),
    ("ram_usage", "RAM"),
    ("ram_type", "RAM"),
    ("ram_freq", "RAM"),
    ("gamepad", "PAD"),
    ("fps", "FPS"),
    ("fps_graph", "FPS"),
    ("frametime_graph", "FRAME"),
];

/// Default global hotkey. `Shift_R` = right shift, `F10` = F10.
pub const DEFAULT_HOTKEY: &str = "Shift_R+F10";

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
    pub always_on_top: bool,
    pub click_through: bool,
    pub position: PanelPosition,
    /// Hotkey string, e.g. `Shift_R+F10`. Empty means "no hotkey".
    pub hotkey: String,
    pub items: Vec<PanelItemConfig>,
}

impl Default for PanelConfig {
    fn default() -> Self {
        Self {
            version: PANEL_CONFIG_VERSION,
            active: false,
            font_scale: 1.0,
            always_on_top: true,
            click_through: false,
            position: PanelPosition::default(),
            hotkey: DEFAULT_HOTKEY.to_string(),
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
        assert_eq!(config.hotkey, "Shift_R+F10");
        assert!(config.always_on_top);
        assert!(!config.click_through);
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
}
