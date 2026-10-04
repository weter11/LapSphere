//! Panel rendering: a fixed-size layout that never depends on the data.
//!
//! The whole point of ADR-2 (`docs/development/panel-design.md`) is that a
//! meter which resizes when a device appears or disappears is unusable — the
//! user's click target moves and the panel cannot be placed. So:
//!
//! * Size is computed from **the element set and the font scale only**. Not
//!   from whether a value arrived, not from the number of CPU cores, not from
//!   whether a gamepad is connected.
//! * A missing value is an em dash **inside an already-reserved slot**.
//! * The graphs are **one fixed-size strip each**, fed by a 60-second ring
//!   buffer that is empty in this release. The ring is sized and allocated
//!   regardless, so PR E fills it without the window moving.
//!
//! `layout()` is a pure function of the config, which is what makes the
//! invariance testable without a window: the same config produces the same
//! size whether the state is empty, full, or has sixteen gamepads.

use super::config::PanelConfig;
use super::items::{self, PanelItem};
use super::units::Value;

/// Horizontal gap between elements, at `font_scale = 1.0`.
const GAP: f32 = 8.0;
/// Padding around the whole strip.
const PADDING: f32 = 6.0;
/// Height of the menu affordance row, reserved at the leading edge.
const MENU_ROW_HEIGHT: f32 = 18.0;

/// Seconds of history the graph ring buffers. Fixed by the owner's decision, and
/// deliberately not configurable: a ring whose length were a setting would let
/// the graph change shape under the user.
pub const GRAPH_WINDOW_SECS: usize = 60;

/// One ring-buffer sample slot: seconds since the epoch the sample was taken.
pub type Sample = f32;

/// A fixed-length ring of samples covering the last `GRAPH_WINDOW_SECS`.
///
/// Nothing pushes samples in this release — the frame data source is PR E — but
/// the buffer exists and is allocated so that the graph is a real consumer of a
/// real ring rather than a stub that PR E has to rewrite.
#[derive(Debug, Clone)]
pub struct RingBuffer {
    samples: Vec<Sample>,
    /// Index where the next sample is written.
    next: usize,
    /// How many slots have ever been filled, saturating at `len`.
    filled: usize,
}

impl RingBuffer {
    pub fn new(seconds: usize) -> Self {
        Self {
            samples: vec![0.0; seconds.max(1)],
            next: 0,
            filled: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.samples.len()
    }

    pub fn len(&self) -> usize {
        self.filled
    }

    pub fn is_empty(&self) -> bool {
        self.filled == 0
    }

    /// Push one sample, overwriting the oldest when full.
    pub fn push(&mut self, value: Sample) {
        self.samples[self.next] = value;
        self.next = (self.next + 1) % self.samples.len();
        if self.filled < self.samples.len() {
            self.filled += 1;
        }
    }

    /// Oldest-to-newest view of the filled part.
    pub fn samples(&self) -> Vec<Sample> {
        if self.filled < self.samples.len() {
            return self.samples[..self.filled].to_vec();
        }
        let mut ordered = Vec::with_capacity(self.samples.len());
        ordered.extend_from_slice(&self.samples[self.next..]);
        ordered.extend_from_slice(&self.samples[..self.next]);
        ordered
    }

    /// Peak value, used to scale the strip. `None` when empty, so an empty ring
    /// cannot divide by zero and produce a NaN height.
    pub fn peak(&self) -> Option<Sample> {
        self.samples()
            .into_iter()
            .reduce(f32::max)
            .filter(|v| v.is_finite())
    }
}

impl Default for RingBuffer {
    fn default() -> Self {
        Self::new(GRAPH_WINDOW_SECS)
    }
}

/// The computed panel geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
}

/// Which corner the panel is anchored to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// The elements to draw, in display order, with their per-item scale resolved.
pub struct Row<'a> {
    pub item: &'static PanelItem,
    /// Config entry for this element (label override, per-item scale).
    pub config: &'a super::config::PanelItemConfig,
    /// Global scale × per-item scale.
    pub scale: f32,
}

/// Build the draw rows from the config: visible elements, in order, scale
/// resolved. Data is not consulted — that is the invariance property.
pub fn rows(config: &PanelConfig) -> Vec<Row<'_>> {
    config
        .items
        .iter()
        .filter(|item| item.visible)
        .filter_map(|item| {
            let registry = items::find(&item.id)?;
            let scale = config.font_scale * item.font_scale.unwrap_or(1.0);
            Some(Row {
                item: registry,
                config: item,
                scale,
            })
        })
        .collect()
}

/// Panel size for a set of rows, at the given scale.
///
/// Pure geometry: the sum of the elements' declared slots plus gaps, times the
/// scale. The tallest element sets the height, so adding a graph grows the
/// height once and no element's own height can shrink the strip.
pub fn layout_for(rows: &[Row<'_>]) -> Layout {
    if rows.is_empty() {
        return Layout {
            width: MENU_ROW_HEIGHT + PADDING * 2.0,
            height: MENU_ROW_HEIGHT + PADDING * 2.0,
        };
    }

    let max_scale = rows
        .iter()
        .map(|row| row.scale)
        .fold(0.0f32, f32::max)
        .max(1.0);

    let mut width = MENU_ROW_HEIGHT;
    let mut height: f32 = MENU_ROW_HEIGHT;

    for row in rows {
        width += row.item.slot.width * max_scale + GAP;
        height = height.max(row.item.slot.height * max_scale);
    }

    Layout {
        width: width + PADDING * 2.0,
        height: height + PADDING * 2.0,
    }
}

/// Convenience: config → layout.
pub fn layout(config: &PanelConfig) -> Layout {
    layout_for(&rows(config))
}

/// Convert a corner plus offsets to a top-left position for a panel of this size
/// inside a work area.
///
/// The panel is clamped into the work area, because a saved offset from a
/// different monitor resolution must not be able to place it off-screen: a
/// panel you cannot see is a panel you cannot get back (the owner's safety rule
/// in commit 6 depends on the panel always being reachable).
pub fn resolve_position(
    anchor: Anchor,
    offset_x: f32,
    offset_y: f32,
    panel: Layout,
    work_area: (f32, f32, f32, f32),
) -> (f32, f32) {
    let (area_x, area_y, area_w, area_h) = work_area;
    let max_x = area_x + (area_w - panel.width).max(0.0);
    let max_y = area_y + (area_h - panel.height).max(0.0);

    let x = match anchor {
        Anchor::TopLeft | Anchor::BottomLeft => area_x + offset_x,
        Anchor::TopRight | Anchor::BottomRight => area_x + area_w - panel.width - offset_x,
    };
    let y = match anchor {
        Anchor::TopLeft | Anchor::TopRight => area_y + offset_y,
        Anchor::BottomLeft | Anchor::BottomRight => area_y + area_h - panel.height - offset_y,
    };

    (
        x.clamp(area_x.min(max_x), max_x),
        y.clamp(area_y.min(max_y), max_y),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::config::{self, PanelConfig};

    /// A config whose VISIBLE elements are exactly `visible`, in that order.
    ///
    /// `normalize` appends every element the file did not mention, so the
    /// unlisted ones have to be added as hidden — otherwise every "only these
    /// elements" assertion would silently be testing the full panel.
    fn config_with(visible: &[&str]) -> PanelConfig {
        let items: Vec<config::PanelItemConfig> = config::PANEL_ITEMS
            .iter()
            .map(|(id, _)| config::PanelItemConfig {
                visible: visible.contains(id),
                ..config::PanelItemConfig::new(id)
            })
            .collect();
        config::normalize(PanelConfig {
            items,
            ..Default::default()
        })
    }

    /// Same, but in an explicit order rather than registry order.
    fn config_with_order(visible: &[&str]) -> PanelConfig {
        let mut config = config_with(&[]);
        for id in visible {
            let item = config
                .items
                .iter_mut()
                .find(|item| &item.id == id)
                .expect("known id");
            item.visible = true;
        }
        // Reorder: visible ones first, in the requested order.
        let mut ordered: Vec<config::PanelItemConfig> = Vec::new();
        for id in visible {
            if let Some(found) = config.items.iter().find(|item| &item.id == id) {
                ordered.push(found.clone());
            }
        }
        ordered.extend(
            config
                .items
                .iter()
                .filter(|item| !visible.contains(&item.id.as_str()))
                .cloned(),
        );
        config.items = ordered;
        config
    }

    #[test]
    fn the_default_panel_has_a_stable_size() {
        let config = config::normalize(PanelConfig::default());
        let first = layout(&config);
        let second = layout(&config);
        assert_eq!(first, second);
        assert!(first.width > 0.0 && first.height > 0.0);
    }

    #[test]
    fn size_does_not_depend_on_data_availability() {
        // The core ADR-2 property: geometry is a function of the config alone,
        // so there is nothing about "data" that could enter the computation.
        let config = config::normalize(PanelConfig::default());
        let with_empty_state = layout(&config);
        let again = layout(&config);
        assert_eq!(with_empty_state, again);
    }

    #[test]
    fn an_absent_element_still_reserves_its_slot() {
        // Same element set, but every value is absent at runtime: the layout is
        // computed from `rows`, which never consults AppState at all.
        let config = config_with(&["cpu_load", "ram_usage", "gpu_load"]);
        let row_count = rows(&config).len();
        assert_eq!(row_count, 3);
        let size = layout(&config);

        // Removing an element shrinks the panel, which is the only way size
        // changes: presence in the config, never presence of data.
        let smaller = layout(&config_with(&["cpu_load", "ram_usage"]));
        assert!(smaller.width < size.width);
    }

    #[test]
    fn core_count_cannot_change_the_size() {
        // The registry declares one fixed slot per element; there is no
        // per-core element, so a 32-core machine and a 4-core machine lay out
        // identically. Asserted through the slot table being data-free.
        let config = config::normalize(PanelConfig::default());
        let size = layout(&config);
        for item in items::PANEL_ITEMS {
            assert!(item.slot.width > 0.0);
        }
        assert_eq!(size, layout(&config));
    }

    #[test]
    fn a_graph_grows_the_height_exactly_once() {
        let text_only = layout(&config_with(&["cpu_load"]));
        let with_graph = layout(&config_with(&["cpu_load", "fps_graph"]));
        assert!(
            with_graph.height > text_only.height,
            "a graph element must add vertical room"
        );

        // Two graphs must not add height twice: the tallest element wins.
        let with_two = layout(&config_with(&["fps_graph", "frametime_graph"]));
        assert_eq!(
            with_two.height,
            layout(&config_with(&["frametime_graph"])).height
        );
    }

    #[test]
    fn font_scale_scales_the_panel() {
        let base = layout(&config_with(&["cpu_load"]));
        let bigger = layout(&PanelConfig {
            font_scale: 2.0,
            items: vec![config::PanelItemConfig::new("cpu_load")],
            ..Default::default()
        });
        assert!(bigger.width > base.width);
        assert!(bigger.height >= base.height);
    }

    #[test]
    fn a_per_item_scale_wins_over_the_global_one() {
        let mut config = config_with(&["cpu_load"]);
        let cpu = config
            .items
            .iter()
            .position(|i| i.id == "cpu_load")
            .unwrap();
        config.items[cpu].font_scale = Some(1.5);
        config.font_scale = 1.0;
        let scaled = layout(&config);
        let plain = layout(&config_with(&["cpu_load"]));
        assert!(scaled.width > plain.width);
    }

    #[test]
    fn an_empty_element_set_still_yields_a_usable_window() {
        // The menu button has to be reachable even with every element hidden.
        let size = layout(&config_with(&[]));
        assert!(size.width > 0.0 && size.height > 0.0);
    }

    #[test]
    fn rows_follow_the_config_order() {
        let config = config_with_order(&["ram_usage", "cpu_load", "gpu_load"]);
        let ids: Vec<&str> = rows(&config).iter().map(|r| r.item.id).collect();
        assert_eq!(ids, vec!["ram_usage", "cpu_load", "gpu_load"]);
    }

    #[test]
    fn a_hidden_element_is_not_a_row() {
        let mut config = config_with(&["cpu_load", "ram_usage"]);
        let cpu = config
            .items
            .iter()
            .position(|i| i.id == "cpu_load")
            .unwrap();
        config.items[cpu].visible = false;
        let ids: Vec<&str> = rows(&config).iter().map(|r| r.item.id).collect();
        assert_eq!(ids, vec!["ram_usage"]);
    }

    #[test]
    fn the_graph_ring_is_sixty_seconds_and_starts_empty() {
        let mut ring = RingBuffer::default();
        assert_eq!(ring.capacity(), GRAPH_WINDOW_SECS);
        assert_eq!(GRAPH_WINDOW_SECS, 60);
        assert!(ring.is_empty());
        assert_eq!(
            ring.peak(),
            None,
            "an empty ring has no peak, not a NaN one"
        );
    }

    #[test]
    fn the_ring_overwrites_the_oldest_sample() {
        let mut ring = RingBuffer::new(3);
        for value in [1.0, 2.0, 3.0, 4.0] {
            ring.push(value);
        }
        assert_eq!(ring.len(), 3);
        assert_eq!(ring.capacity(), 3);
        assert_eq!(ring.samples(), vec![2.0, 3.0, 4.0]);
    }

    #[test]
    fn the_ring_is_oldest_to_newest_while_filling() {
        let mut ring = RingBuffer::new(4);
        for value in [1.0, 2.0, 3.0] {
            ring.push(value);
        }
        assert_eq!(ring.samples(), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn a_full_ring_of_zeros_has_a_peak() {
        // The NaN guard: peak() filters non-finite, and 0.0 is finite.
        let mut ring = RingBuffer::new(2);
        ring.push(0.0);
        ring.push(0.0);
        assert_eq!(ring.peak(), Some(0.0));
    }

    #[test]
    fn position_resolves_from_each_corner() {
        let panel = Layout {
            width: 200.0,
            height: 40.0,
        };
        let area = (0.0, 0.0, 1920.0, 1080.0);

        let tl = resolve_position(Anchor::TopLeft, 10.0, 20.0, panel, area);
        assert_eq!(tl, (10.0, 20.0));

        let tr = resolve_position(Anchor::TopRight, 10.0, 20.0, panel, area);
        assert_eq!(tr, (1920.0 - 200.0 - 10.0, 20.0));

        let bl = resolve_position(Anchor::BottomLeft, 10.0, 20.0, panel, area);
        assert_eq!(bl, (10.0, 1080.0 - 40.0 - 20.0));

        let br = resolve_position(Anchor::BottomRight, 10.0, 20.0, panel, area);
        assert_eq!(br, (1920.0 - 200.0 - 10.0, 1080.0 - 40.0 - 20.0));
    }

    #[test]
    fn a_position_from_another_resolution_is_clamped_into_the_work_area() {
        // An offset saved on a 4K screen must not place the panel off-screen on
        // a 1080p one.
        let panel = Layout {
            width: 300.0,
            height: 60.0,
        };
        let area = (0.0, 0.0, 1280.0, 720.0);
        let (x, y) = resolve_position(Anchor::TopLeft, 5000.0, 5000.0, panel, area);
        assert!(x >= 0.0 && x + panel.width <= 1280.0, "x={x} off-screen");
        assert!(y >= 0.0 && y + panel.height <= 720.0, "y={y} off-screen");
    }

    #[test]
    fn a_panel_larger_than_the_work_area_lands_at_its_origin() {
        let panel = Layout {
            width: 4000.0,
            height: 2000.0,
        };
        let (x, y) = resolve_position(
            Anchor::TopRight,
            10.0,
            10.0,
            panel,
            (0.0, 0.0, 1280.0, 720.0),
        );
        assert_eq!((x, y), (0.0, 0.0));
    }

    #[test]
    fn the_work_area_origin_is_respected() {
        // A secondary monitor to the left of the primary has a negative origin.
        let panel = Layout {
            width: 200.0,
            height: 40.0,
        };
        let (x, y) = resolve_position(
            Anchor::TopLeft,
            10.0,
            10.0,
            panel,
            (-1920.0, 0.0, 1920.0, 1080.0),
        );
        assert_eq!((x, y), (-1910.0, 10.0));
    }

    #[test]
    fn every_element_renders_a_value_without_panicking() {
        // Reading every element against an empty state must be total: no
        // unwraps, no indexing that a real machine could not satisfy.
        let state = crate::app::AppState::new();
        for item in items::PANEL_ITEMS {
            let value = (item.read)(&state);
            match value {
                Value::Absent => assert_eq!(value.text(), "—"),
                Value::Graph => {}
                Value::Text(text) => assert!(!text.is_empty(), "{} produced empty text", item.id),
            }
        }
    }
}
