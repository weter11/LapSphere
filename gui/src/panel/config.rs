//! Panel configuration: its own file, its own schema, no contact with the tray
//! or with `settings.json`.
//!
//! The panel gets `~/.config/lapsphere/panel.json`. Two reasons, both the
//! owner's decision:
//!
//! * `settings.json` is what the Settings page and the tray own. The panel is a
//!   separate surface with a separate lifecycle, so a corrupt or hand-edited
//!   panel file must not be able to take the tray's settings with it.
//! * Panel settings and UI settings change for different reasons. Toggling a
//!   row in the panel must not rewrite the file that holds polling rates.
//!
//! Shape follows the convention already used by `settings.json` and
//! `tray.json`: `#[serde(default)]` at the container level so a partial or older
//! file loads, and a [`normalize`] step so a hand-edited file cannot produce a
//! panel with an unknown row, a duplicate row, an unusable width or a
//! transparent-to-invisible background.
//!
//! # What is NOT here
//!
//! Deliberately absent, and each for a reason:
//!
//! * **No frame-time / fps rows and no ring buffer.** Out of scope by decision;
//!   the reserved-slot idea that would have needed them is not carried over.
//! * **No click-through.** Out of scope by decision.
//! * **No position.** The panel is placed by the window manager / desktop for
//!   now; a stored corner is the second-monitor question, which is explicitly
//!   NOT claimed.
//! * **No hotkey or D-Bus fields.** Those arrive with the visibility work
//!   (PR 3) and belong with the code that implements them, not ahead of it.

use serde::{Deserialize, Serialize};

use super::rows::{self, DEFAULT_VISIBLE};

/// Bumped when the on-disk shape changes incompatibly.
pub const PANEL_CONFIG_VERSION: u32 = 1;

/// File name of the panel configuration inside the config directory.
pub const PANEL_CONFIG_FILE: &str = "panel.json";

/// Name a corrupt `panel.json` is moved aside to instead of being deleted.
///
/// Same contract as `tray.json.corrupt`: no user setting is ever destroyed by
/// this code path, and a moved-aside file can be inspected and hand-fixed.
pub const PANEL_CONFIG_CORRUPT: &str = "panel.json.corrupt";

/// Default panel width in points.
///
/// 300 px, as the owner's decision states. Wide enough for the longest label
/// (`Average Frequency`) plus a value, narrow enough to sit over a game.
pub const DEFAULT_WIDTH: f32 = 300.0;

/// Narrowest panel the layout can render without clipping the value column.
pub const MIN_WIDTH: f32 = 200.0;
/// Widest panel; beyond this the single column is mostly empty space.
pub const MAX_WIDTH: f32 = 640.0;

/// Default background opacity, 0.0–1.0.
///
/// Fully opaque: the transparency work is a separate change, and a panel that
/// starts translucent on a compositor that may not be running is a worse
/// default than a solid one.
pub const DEFAULT_BACKGROUND_OPACITY: f32 = 1.0;

/// Least opaque the background may be set to.
///
/// Not zero: at 0.0 the text floats on whatever is behind it, which on a
/// bright desktop is unreadable rather than "transparent".
pub const MIN_BACKGROUND_OPACITY: f32 = 0.25;
pub const MAX_BACKGROUND_OPACITY: f32 = 1.0;

/// How the panel window stacks against other windows.
///
/// `AlwaysOnTop` is the default because that is what an overlay panel is for.
/// `Normal` exists because an always-on-top window that covers a fullscreen
/// game is not always what the user wants.
///
/// Applying these is PR 2's work (the X11 atoms). The setting and its
/// normalization live here now so the file schema is settled; nothing in PR 1
/// claims the behaviour works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stacking {
    AlwaysOnTop,
    Normal,
}

impl Default for Stacking {
    fn default() -> Self {
        Stacking::AlwaysOnTop
    }
}

impl Stacking {
    pub const ALL: [Stacking; 2] = [Stacking::AlwaysOnTop, Stacking::Normal];

    pub fn label(self) -> &'static str {
        match self {
            Stacking::AlwaysOnTop => "Always on top",
            Stacking::Normal => "Normal window",
        }
    }

    /// The exact string written to `panel.json`.
    ///
    /// `#[cfg_attr(not(test), allow(dead_code))]`: the panel writes the enum
    /// through `Serialize`, so in a non-test build nothing calls this. It is kept
    /// as the readable, asserted statement of the wire format rather than
    /// deriving the string from `label()`, which is UI text.
    ///
    /// Spelled out rather than derived from [`Stacking::label`], because the
    /// label is UI text and the stored value is a wire format that must not
    /// drift with it.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn stored_value(self) -> &'static str {
        match self {
            Stacking::AlwaysOnTop => "always_on_top",
            Stacking::Normal => "normal",
        }
    }

    /// Parse a stored value; anything unrecognised becomes `always_on_top`.
    ///
    /// Degrading one field rather than failing the file is deliberate: a failing
    /// parse would discard the user's whole row selection, width and opacity to
    /// correct a single unknown enum.
    pub fn from_stored(raw: &str) -> Self {
        match raw.trim() {
            "normal" => Stacking::Normal,
            _ => Stacking::AlwaysOnTop,
        }
    }
}

impl<'de> Deserialize<'de> for Stacking {
    /// Degrade an unusable value instead of erroring — see [`Stacking::from_stored`].
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = serde_json::Value::deserialize(deserializer).map_err(serde::de::Error::custom)?;
        let text = match raw {
            serde_json::Value::String(text) => text,
            serde_json::Value::Null => return Ok(Stacking::default()),
            other => other.to_string(),
        };
        Ok(Stacking::from_stored(&text))
    }
}

/// One row's configuration. Display order is array order in `rows`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RowConfig {
    pub id: String,
    /// A row with `visible: false` is configured but not laid out.
    pub visible: bool,
}

impl Default for RowConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            // Default true, so a hand-added entry is visible; what a row defaults
            // to VISIBLE is decided by `normalize_default_visibility` below, not
            // here.
            visible: true,
        }
    }
}

impl RowConfig {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            visible: false,
        }
    }
}

/// The whole of `panel.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelConfig {
    pub version: u32,
    /// Width in points, clamped to `MIN_WIDTH..=MAX_WIDTH`.
    pub width: f32,
    /// Background opacity 0.0–1.0, clamped to
    /// `MIN_BACKGROUND_OPACITY..=MAX_BACKGROUND_OPACITY`.
    pub background_opacity: f32,
    pub stacking: Stacking,
    /// In `always_on_top` stacking: keep the panel out of the taskbar.
    pub hide_from_taskbar: bool,
    pub rows: Vec<RowConfig>,
}

impl Default for PanelConfig {
    fn default() -> Self {
        Self {
            version: PANEL_CONFIG_VERSION,
            width: DEFAULT_WIDTH,
            background_opacity: DEFAULT_BACKGROUND_OPACITY,
            stacking: Stacking::default(),
            hide_from_taskbar: true,
            rows: rows::all_ids().into_iter().map(RowConfig::new).collect(),
        }
    }
}

impl PanelConfig {
    /// The freshly-installed default, with the sensible rows actually visible.
    ///
    /// Separate from `Default` because `#[serde(default)]` on a missing field
    /// must produce *something*, and the rows' visibility depends on which ids
    /// the registry knows — a plain `Default` cannot consult it without a cycle.
    pub fn fresh() -> Self {
        let mut config = Self::default();
        for row in &mut config.rows {
            row.visible = DEFAULT_VISIBLE.contains(&row.id.as_str());
        }
        config
    }
}

/// Is `id` a row this build knows about?
pub fn is_known_row(id: &str) -> bool {
    rows::find(id).is_some()
}

/// Drop unknown and duplicate ids, keep the user's order, append the rest.
///
/// Same contract as `statistics::normalize_section_order`: an unknown id is
/// skipped rather than rendered as `—` forever, and a file that omits a row
/// still gets it, so a config written by an older build does not silently
/// shrink the panel.
pub fn normalize_rows(config_rows: &[RowConfig]) -> Vec<RowConfig> {
    let mut normalized: Vec<RowConfig> = Vec::new();

    for row in config_rows {
        if is_known_row(&row.id) && !normalized.iter().any(|existing| existing.id == row.id) {
            normalized.push(row.clone());
        }
    }

    for id in rows::all_ids() {
        if !normalized.iter().any(|existing| &existing.id == id) {
            normalized.push(RowConfig::new(id));
        }
    }

    normalized
}

/// Clamp everything that could make the panel unusable, then normalize the rows.
///
/// The float handling is the load-bearing part. `f32::clamp` propagates `NaN`,
/// and `NaN as u8` is `0` — so a corrupt width would become a zero-width window
/// and a corrupt opacity a fully invisible panel, both silently. Non-finite
/// falls back to the default; infinities clamp to the nearer bound, which is
/// what the caller meant.
pub fn normalize(mut config: PanelConfig) -> PanelConfig {
    config.version = PANEL_CONFIG_VERSION;

    config.width = normalize_float(config.width, DEFAULT_WIDTH, MIN_WIDTH, MAX_WIDTH, "width");
    config.background_opacity = normalize_float(
        config.background_opacity,
        DEFAULT_BACKGROUND_OPACITY,
        MIN_BACKGROUND_OPACITY,
        MAX_BACKGROUND_OPACITY,
        "background_opacity",
    );

    config.rows = normalize_rows(&config.rows);
    config
}

/// One non-finite-aware clamp, so width and opacity cannot diverge in policy.
fn normalize_float(value: f32, default: f32, min: f32, max: f32, field: &str) -> f32 {
    if value.is_nan() {
        log::warn!("panel.json has a non-finite {field}, falling back to {default}");
        return default;
    }
    value.clamp(min, max)
}

/// The row ids the panel will actually lay out, in order.
pub fn visible_row_ids(config: &PanelConfig) -> Vec<&str> {
    config
        .rows
        .iter()
        .filter(|row| row.visible)
        .map(|row| row.id.as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, visible: bool) -> RowConfig {
        RowConfig {
            visible,
            ..RowConfig::new(id)
        }
    }

    #[test]
    fn a_fresh_config_shows_exactly_the_default_rows() {
        let config = normalize(PanelConfig::fresh());
        let visible = visible_row_ids(&config);
        assert_eq!(visible, DEFAULT_VISIBLE.to_vec());
        assert_eq!(visible.len(), 10);
        // No gamepad dashes on a machine that has never seen a gamepad.
        assert!(!visible.iter().any(|id| id.starts_with("gamepad_")));
    }

    #[test]
    fn every_registry_row_appears_exactly_once_after_normalizing() {
        let config = normalize(PanelConfig::default());
        assert_eq!(config.rows.len(), rows::all_ids().len());
        let mut ids: Vec<&str> = config.rows.iter().map(|r| r.id.as_str()).collect();
        ids.sort_unstable();
        let mut expected = rows::all_ids();
        expected.sort_unstable();
        assert_eq!(ids, expected);
    }

    #[test]
    fn unknown_ids_are_dropped_and_the_missing_known_ones_are_appended() {
        let input = vec![
            row("cpu_processor", true),
            row("not_a_real_row", true),
            row("ram_usage", true),
        ];
        let normalized = normalize_rows(&input);
        let ids: Vec<&str> = normalized.iter().map(|r| r.id.as_str()).collect();

        assert!(
            !ids.contains(&"not_a_real_row"),
            "unknown id must be skipped"
        );
        assert_eq!(ids.len(), rows::all_ids().len());
        assert_eq!(ids[0], "cpu_processor", "user order is preserved");
        assert_eq!(ids[1], "ram_usage");
    }

    #[test]
    fn duplicates_collapse_and_the_first_occurrence_wins() {
        let input = vec![
            row("cpu_processor", false),
            row("ram_usage", true),
            row("cpu_processor", true),
        ];
        let normalized = normalize_rows(&input);
        let cpu = normalized
            .iter()
            .find(|r| r.id == "cpu_processor")
            .expect("present");
        assert!(!cpu.visible, "the first entry's flags win");
        assert_eq!(
            normalized
                .iter()
                .filter(|r| r.id == "cpu_processor")
                .count(),
            1
        );
    }

    #[test]
    fn the_width_is_clamped_at_both_ends() {
        let mut config = PanelConfig::fresh();
        config.width = 10_000.0;
        assert_eq!(normalize(config.clone()).width, MAX_WIDTH);
        config.width = 1.0;
        assert_eq!(normalize(config).width, MIN_WIDTH);
    }

    #[test]
    fn a_non_finite_width_falls_back_instead_of_becoming_zero() {
        // The failure this prevents: `NaN.clamp(..)` is NaN, and NaN as a window
        // dimension is a zero-width window.
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut config = PanelConfig::fresh();
            config.width = bad;
            let normalized = normalize(config);
            assert!(
                normalized.width.is_finite(),
                "{bad} must not survive normalization"
            );
            assert!(normalized.width >= MIN_WIDTH && normalized.width <= MAX_WIDTH);
        }
        // NaN specifically falls back to the default rather than to a bound.
        let mut config = PanelConfig::fresh();
        config.width = f32::NAN;
        assert_eq!(normalize(config).width, DEFAULT_WIDTH);
    }

    #[test]
    fn the_opacity_is_clamped_and_never_reaches_invisible() {
        let mut config = PanelConfig::fresh();
        config.background_opacity = 0.0;
        assert_eq!(
            normalize(config.clone()).background_opacity,
            MIN_BACKGROUND_OPACITY,
            "a fully transparent background is unreadable, not transparent"
        );
        config.background_opacity = 5.0;
        assert_eq!(normalize(config.clone()).background_opacity, 1.0);
        config.background_opacity = f32::NAN;
        let normalized = normalize(config);
        assert_eq!(normalized.background_opacity, DEFAULT_BACKGROUND_OPACITY);
        assert!(normalized.background_opacity.is_finite());
    }

    #[test]
    fn visible_row_ids_respects_visibility_and_order() {
        let config = PanelConfig {
            rows: normalize_rows(&[row("ram_usage", true), row("cpu_processor", false)]),
            ..PanelConfig::fresh()
        };
        let visible = visible_row_ids(&config);
        assert_eq!(visible[0], "ram_usage", "the configured-visible row leads");
        assert!(!visible.contains(&"cpu_processor"));
    }

    #[test]
    fn a_partial_file_decodes_and_fills_in() {
        // What a hand-written or older panel.json looks like.
        let parsed: PanelConfig =
            serde_json::from_str(r#"{"width": 260.0}"#).expect("partial file decodes");
        let config = normalize(parsed);
        assert_eq!(config.width, 260.0);
        assert_eq!(config.rows.len(), rows::all_ids().len());
        assert_eq!(config.version, PANEL_CONFIG_VERSION);
        assert_eq!(config.stacking, Stacking::AlwaysOnTop);
    }

    #[test]
    fn an_unknown_stacking_value_degrades_to_always_on_top() {
        // Degrading one field, not failing the file: a failing parse would throw
        // away the user's row selection and width as well.
        for raw in ["\"floating\"", "\"\"", "17", "true", "null", "[]"] {
            let json = format!("{{\"stacking\": {raw}}}");
            let parsed: PanelConfig =
                serde_json::from_str(&json).unwrap_or_else(|_| panic!("{json} must decode"));
            assert_eq!(
                parsed.stacking,
                Stacking::AlwaysOnTop,
                "{raw} must not silently become normal"
            );
        }
    }

    #[test]
    fn the_two_known_stacking_values_round_trip() {
        for stacking in Stacking::ALL {
            let json = serde_json::to_string(&stacking).expect("serialize");
            assert_eq!(json, format!("\"{}\"", stacking.stored_value()));
            let parsed: Stacking = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(parsed, stacking);
        }
    }

    #[test]
    fn round_trips_through_json() {
        let config = normalize(PanelConfig::fresh());
        let json = serde_json::to_string_pretty(&config).expect("serialize");
        let back: PanelConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(normalize(back), config);
    }

    #[test]
    fn panel_json_carries_no_tray_or_settings_keys() {
        // The owner's separation decision, asserted against the schema so a
        // future field cannot quietly reintroduce the coupling.
        let json = serde_json::to_string(&PanelConfig::fresh()).expect("serialize");
        for forbidden in [
            "tray",
            "theme",
            "autostart",
            "statistics_sections",
            "battery_settings",
            "click_through",
            "hotkey",
        ] {
            assert!(
                !json.contains(forbidden),
                "panel.json must not carry `{forbidden}`"
            );
        }
    }
}
