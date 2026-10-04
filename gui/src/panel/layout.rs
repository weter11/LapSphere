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
//! So the size is computed from the row COUNT and the row height alone. The row
//! height is a font metric (`row_height` on the resolved `FontId`), which is a
//! pure function of the font and the text style — not of the values, not of how
//! many rows happen to be non-empty. Two consequences, both required:
//!
//! * A row whose value is `—` costs exactly the same height as a row with a
//!   long value, because the label is drawn on one line and the value is
//!   clipped to the remaining width.
//! * Turning a row on or off changes the height by exactly one row height, so
//!   the window never jumps by an unpredictable amount.
//!
//! [`panel_size`] is therefore a pure function of `(row count, row height,
//! width)` and is testable without a window.

use super::config::PanelConfig;

/// Vertical padding above the first row and below the last, in points.
pub const PADDING_Y: f32 = 6.0;
/// Horizontal padding left and right of the column.
pub const PADDING_X: f32 = 8.0;
/// Gap between the label and the value in one row.
pub const LABEL_GAP: f32 = 6.0;

/// Widest short label in the registry (`MemClk`), in points at the default font.
///
/// A constant rather than a measurement for the same reason as the height: a
/// label column sized to its content would move every value right when a row is
/// toggled, and the numbers would visibly jump. Sized to the WIDEST label instead,
/// so it is the same whatever rows are visible — and it changes only if the
/// registry's short labels do.
pub const LABEL_WIDTH: f32 = 52.0;

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

/// Total height for `row_count` rows.
///
/// No content is consulted: this is the whole reason the window does not
/// oscillate. Zero rows still yields a valid, non-zero height (padding only) so
/// the window never collapses to nothing when every row is switched off.
pub fn height_for(row_count: usize, font_row_height: f32) -> f32 {
    PADDING_Y * 2.0 + row_count as f32 * row_height(font_row_height)
}

/// The full panel size for a config and a resolved font row height.
///
/// `row_count` is passed separately rather than read from the config so a caller
/// that has already filtered the rows does not filter them twice, and so the
/// function has no hidden dependency on `AppState`.
pub fn panel_size(config: &PanelConfig, row_count: usize, font_row_height: f32) -> Size {
    Size {
        width: config.width,
        height: height_for(row_count, font_row_height),
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
    }

    #[test]
    fn height_does_not_depend_on_width_or_on_the_values() {
        // Two configs differing only in width produce the same height, which is
        // what makes width a free, instantly-applied setting: dragging the
        // width slider cannot reflow the rows.
        let narrow = PanelConfig {
            width: 200.0,
            ..PanelConfig::fresh()
        };
        let wide = PanelConfig {
            width: 640.0,
            ..PanelConfig::fresh()
        };
        assert_eq!(
            panel_size(&narrow, 6, FONT_ROW).height,
            panel_size(&wide, 6, FONT_ROW).height
        );
    }

    #[test]
    fn no_visible_rows_still_yields_a_usable_window() {
        let height = height_for(0, FONT_ROW);
        assert!(height > 0.0, "the window must not collapse to zero height");
        assert_eq!(height, PADDING_Y * 2.0);
    }

    #[test]
    fn a_larger_font_makes_a_taller_panel() {
        assert!(height_for(6, 22.0) > height_for(6, 12.0));
    }
}
