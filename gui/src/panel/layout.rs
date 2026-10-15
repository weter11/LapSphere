//! Panel geometry: a vertical column whose size is computed, never measured.
//!
//! # Why no measurement
//!
//! The obvious implementation asks egui how tall the content is and then resizes
//! the window to match. That is a feedback loop, and it oscillates: the window
//! resize triggers a new frame, whose layout can differ by a rounding pixel,
//! which resizes the window again. On a WM that also re-decorates the window the
//! loop can be visible.
//!
//! So the size is computed from the **slot list** and the row height alone. The row
//! height is a font metric (`row_height` on the resolved `FontId`), which is a
//! pure function of the font and the text style — not of the values, not of how
//! many rows happen to be non-empty, and not of whether a source has data. Two
//! consequences, both required:
//!
//! * A row whose value is `—` costs exactly the same height as a row with a
//!   long value, because the label is drawn on one line and the value is
//!   clipped to the remaining width.
//! * Turning a row on or off changes the height by exactly that slot's height, so
//!   the window never jumps by an unpredictable amount.
//!
//! # Slots, and the two heights
//!
//! A row is either a line of text ([`Slot::Row`]) or the frame-time strip
//! ([`Slot::GraphStrip`]), which is [`GRAPH_STRIP_ROWS`] row heights tall. The
//! strip's height is a **constant multiple of the row height**, never a function of
//! how many intervals it holds: a strip whose height tracked its data would resize
//! the window every time the frame rate changed, which is exactly the reflow that
//! reserving the slot exists to prevent. So [`height_for_slots`] is a pure function
//! of `(slots, row height)` and is testable without a window.
//!
//! [`panel_size`] adds the configured width, and the width cannot move a row: the
//! label column is [`LABEL_WIDTH`] wide whatever the panel's width is, and the
//! value column takes what is left — so a longer value, or a value arriving where
//! there was none, changes no geometry at all.

use super::config::PanelConfig;

/// Vertical padding above the first row and below the last, in points.
pub const PADDING_Y: f32 = 6.0;
/// Horizontal padding left and right of the column.
pub const PADDING_X: f32 = 8.0;
/// Gap between the label and the value in one row.
pub const LABEL_GAP: f32 = 6.0;

/// Widest short label in the registry (`MemClk`, `1%low`), in points at the
/// default font.
///
/// A constant rather than a measurement for the same reason as the height: a
/// label column sized to its content would move every value right when a row is
/// toggled, and the numbers would visibly jump. Sized to the WIDEST label instead,
/// so it is the same whatever rows are visible — and it changes only if the
/// registry's short labels do. `render`'s `every_label_fits_the_label_column`
/// measures every registered label against this number with real font metrics, so a
/// label that does not fit fails a test instead of silently eliding into its own
/// value.
pub const LABEL_WIDTH: f32 = 52.0;

/// How many row heights tall the frame-time strip is.
///
/// Three: the smallest height at which a 0–50 ms scale puts an ordinary 6–17 ms
/// frame somewhere other than the bottom pixel, and still leaves room above it for
/// the hitches the strip exists to show.
pub const GRAPH_STRIP_ROWS: f32 = 3.0;

/// What occupies one vertical slot in the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// One `Label: value` line.
    Row,
    /// The frame-time strip: width from the panel, height from
    /// [`GRAPH_STRIP_ROWS`], whatever it holds.
    GraphStrip,
}

/// The computed panel size in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

/// Height of one row: the font's line height plus a little leading.
///
/// `row_height` is egui's own metric for the resolved font, so this tracks the
/// user's font-size setting without this module knowing what a font size is.
pub fn row_height(font_row_height: f32) -> f32 {
    font_row_height + 2.0
}

/// Height of one slot, by kind.
pub fn slot_height(slot: Slot, font_row_height: f32) -> f32 {
    match slot {
        Slot::Row => row_height(font_row_height),
        Slot::GraphStrip => row_height(font_row_height) * GRAPH_STRIP_ROWS,
    }
}

/// Total height for a slot list.
///
/// No content is consulted: this is the whole reason the window does not
/// oscillate. Zero slots still yields a valid, non-zero height (padding only) so
/// the window never collapses to nothing when every row is switched off.
pub fn height_for_slots(slots: &[Slot], font_row_height: f32) -> f32 {
    PADDING_Y * 2.0
        + slots
            .iter()
            .map(|slot| slot_height(*slot, font_row_height))
            .sum::<f32>()
}

/// Total height for `row_count` text rows — the all-text case, kept because the
/// arithmetic it states (padding plus N row heights, nothing else) is what the
/// no-oscillation argument rests on and what several tests are written against.
#[cfg_attr(not(test), allow(dead_code))]
pub fn height_for(row_count: usize, font_row_height: f32) -> f32 {
    height_for_slots(&vec![Slot::Row; row_count], font_row_height)
}

/// The full panel size for a config and a resolved font row height.
///
/// `slots` is passed separately rather than read from the config so a caller that
/// has already filtered the rows does not filter them twice, and so the
/// function has no hidden dependency on `AppState`.
pub fn panel_size(config: &PanelConfig, slots: &[Slot], font_row_height: f32) -> Size {
    Size {
        width: config.width,
        height: height_for_slots(slots, font_row_height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FONT_ROW: f32 = 14.0;

    #[test]
    fn adding_or_removing_one_row_moves_the_height_by_exactly_one_row() {
        // The "no jumping" property, stated as an assertion: the delta is the row
        // height and nothing else, so a user toggling a row sees a predictable
        // change.
        let delta = height_for(7, FONT_ROW) - height_for(6, FONT_ROW);
        assert_eq!(delta, row_height(FONT_ROW));

        // And the strip costs exactly its reserved height, not its data's worth.
        let with_strip = [Slot::Row, Slot::Row, Slot::GraphStrip];
        let without_strip = [Slot::Row, Slot::Row];
        assert_eq!(
            height_for_slots(&with_strip, FONT_ROW) - height_for_slots(&without_strip, FONT_ROW),
            slot_height(Slot::GraphStrip, FONT_ROW)
        );
    }

    #[test]
    fn height_does_not_depend_on_width_or_on_the_values() {
        // Two configs differing only in width produce the same height, which is
        // what makes width a free, instantly-applied setting: dragging the width
        // slider cannot reflow the rows.
        let narrow = PanelConfig {
            width: 200.0,
            ..PanelConfig::fresh()
        };
        let wide = PanelConfig {
            width: 640.0,
            ..PanelConfig::fresh()
        };
        let rows = [Slot::Row; 6];
        assert_eq!(
            panel_size(&narrow, &rows, FONT_ROW).height,
            panel_size(&wide, &rows, FONT_ROW).height
        );
    }

    #[test]
    fn the_strip_height_is_a_constant_multiple_of_the_row_height() {
        // Not a function of the data, of the sample count or of the panel's width:
        // three row heights, at every font size.
        for font in [10.0f32, 14.0, 22.0] {
            assert_eq!(
                slot_height(Slot::GraphStrip, font),
                row_height(font) * GRAPH_STRIP_ROWS
            );
        }
        assert_eq!(GRAPH_STRIP_ROWS, 3.0);
    }

    #[test]
    fn no_visible_rows_still_yields_a_usable_window() {
        let height = height_for(0, FONT_ROW);
        assert!(height > 0.0, "the window must not collapse to zero height");
        assert_eq!(height, PADDING_Y * 2.0);
        assert_eq!(height_for_slots(&[], FONT_ROW), PADDING_Y * 2.0);
    }

    #[test]
    fn a_larger_font_makes_a_taller_panel() {
        assert!(height_for(6, 22.0) > height_for(6, 12.0));
        assert!(
            height_for_slots(&[Slot::GraphStrip], 22.0)
                > height_for_slots(&[Slot::GraphStrip], 12.0)
        );
    }

    #[test]
    fn the_height_is_the_sum_of_its_slots_and_the_padding() {
        // Stated as arithmetic because the window's size claims rest on it: no
        // rounding allowance, no fudge term.
        let slots = [Slot::GraphStrip, Slot::Row, Slot::Row];
        let expected = PADDING_Y * 2.0 + row_height(FONT_ROW) * (GRAPH_STRIP_ROWS + 2.0);
        assert_eq!(height_for_slots(&slots, FONT_ROW), expected);
    }
}
