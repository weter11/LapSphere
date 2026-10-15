//! The frame-time strip: the fixed-height element that draws the recorded
//! intervals.
//!
//! # Why a strip and not a plot
//!
//! The strip exists to answer one question at a glance — *is the frame time
//! steady, and where did it spike* — inside a 300 px column. So it is a
//! polyline over a fixed height ([`layout::GRAPH_STRIP_ROWS`] row heights) with a
//! **fixed** millisecond scale ([`FULL_SCALE_MS`]), not an auto-scaled chart:
//!
//! * a fixed height means the window size stays a pure function of the row set,
//!   which is what makes landing the data unable to reflow the panel;
//! * a fixed scale means the same shape means the same thing from one second to
//!   the next. An auto-scaled strip turns a steady 8.3 ms into the same picture
//!   as a steady 40 ms, and re-scales — visibly — the moment one hitch arrives,
//!   which reads as a change in the game rather than in the drawing.
//!
//! Intervals longer than the full scale are **clamped to the top** of the strip
//! rather than omitted: a 200 ms hitch has to be visible as *something*, and a
//! clipped line at the ceiling says "off the top of the scale" without pretending
//! to a value it did not measure. `fps`/`low_1pct` remain the numbers; the strip
//! is the shape.
//!
//! # The strip covers the whole statistics window, in a bounded column count
//!
//! The strip has [`GRAPH_COLUMNS`] columns — its geometry is fixed, and its width
//! is the panel's width, which is a user setting. The **series** it draws covers
//! the whole configured statistics window, not the newest N intervals of it: a
//! window of 10 s at 60 fps is ~600 intervals, at 165 fps ~1650, at 240 fps ~2400,
//! and a strip that showed only the newest 120 would cover ~2 s of a 10 s window
//! and quietly disagree with the numbers in the rows beside it.
//!
//! So when the window holds more intervals than the strip has columns, each column
//! stands for a contiguous run of them and carries **the worst interval in that
//! run** — peak-preserving decimation:
//!
//! * an isolated spike lands in exactly one column and *is* that column's value, so
//!   it cannot be averaged or dropped away. That is the property the graph exists
//!   for: a 200 ms hitch two seconds ago must still be visible;
//! * a column is never *invented*: with fewer intervals than columns, one column is
//!   one interval and nothing is interpolated;
//! * the cost is stated rather than hidden — [`Series::span`] is the largest number
//!   of intervals any column stands for, and the settings surface prints it beside
//!   the strip's scale, so "the shape is smoother than the frame rate" is visible
//!   instead of being an artefact the user has to guess at.
//!
//! # An empty source draws an empty strip
//!
//! Nothing is drawn but the strip's own background, at the same place and the
//! same size. No zero line, no placeholder polyline: a flat line at the bottom is
//! a *claim* about frame times, and the honest rendering of "no data" is no line
//! at all.

/// How many columns the strip draws.
///
/// `graph_samples` in the design's config proposal. It is a **column** count, not
/// a sample count: the strip's width is the panel's width (a user setting), so the
/// number of points is what the drawing has to bound, and everything the window
/// holds beyond it is folded into those points by [`series`].
pub const GRAPH_COLUMNS: usize = 120;

/// The top of the strip's scale, in milliseconds.
///
/// 50 ms is 20 fps: everything at or above it is at the ceiling, and everything
/// below it is drawn to scale. At 60/120/165 Hz the ordinary frames sit in the
/// bottom fifth to sixth of the strip, which leaves the rest of the height free
/// for the hitches the strip is there to show.
pub const FULL_SCALE_MS: f32 = 50.0;

/// What the strip draws: the whole window, in at most [`GRAPH_COLUMNS`] columns.
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    /// One value per column, in milliseconds, oldest first, at most
    /// [`GRAPH_COLUMNS`] of them.
    pub columns: Vec<f32>,
    /// The largest number of intervals any one column stands for.
    ///
    /// `1` when every interval has its own column, which is the case whenever the
    /// window holds no more intervals than the strip has columns.
    pub span: usize,
}

/// The series for one window: the whole interval list, folded into at most
/// [`GRAPH_COLUMNS`] peak-preserving columns.
///
/// `window_ms` is the statistics window's intervals in milliseconds, oldest first
/// (see `stats::window`). Fewer intervals than columns are drawn one per column;
/// more are bucketed, and each bucket contributes its **maximum**, so a spike is
/// never averaged away.
pub fn series(window_ms: &[f64]) -> Series {
    if window_ms.len() <= GRAPH_COLUMNS {
        return Series {
            columns: window_ms.iter().map(|ms| *ms as f32).collect(),
            // Nothing to draw is not "one interval per column": 0 keeps the two
            // cases apart for the caption and for the tests.
            span: usize::from(!window_ms.is_empty()),
        };
    }

    let total = window_ms.len();
    let mut columns = Vec::with_capacity(GRAPH_COLUMNS);
    let mut span = 0usize;
    for index in 0..GRAPH_COLUMNS {
        // Integer partition of the window: every interval belongs to exactly one
        // bucket, the buckets are contiguous, and none is empty (`total > columns`
        // makes each boundary advance by at least one).
        let start = index * total / GRAPH_COLUMNS;
        let end = ((index + 1) * total / GRAPH_COLUMNS).max(start + 1);
        span = span.max(end - start);
        let peak = window_ms[start..end]
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        columns.push(peak as f32);
    }

    Series { columns, span }
}

/// Where an interval sits in the strip: 0.0 at the baseline, 1.0 at the ceiling.
pub fn normalized(ms: f32) -> f32 {
    (ms / FULL_SCALE_MS).clamp(0.0, 1.0)
}

/// The strip's scale, as the settings menu prints it next to the checkbox.
///
/// A strip has no text value to preview, so the menu shows what the strip's
/// height means instead of an empty cell. `span` is folded in: one interval per
/// column prints the scale alone, and a decimated series says how many intervals a
/// column stands for, so a smooth shape is never mistaken for a smooth game.
pub fn scale_caption(span: usize) -> String {
    if span <= 1 {
        format!("0–{} ms", FULL_SCALE_MS as u32)
    } else {
        format!("0–{} ms · worst of {}", FULL_SCALE_MS as u32, span)
    }
}

/// Paint the strip into `rect`.
///
/// `rect` is the element's reserved slot, computed from the row set and the font
/// metrics before anything was read: this function never changes it, so a strip
/// that gains data cannot move the panel.
pub fn draw(
    ui: &egui::Ui,
    rect: egui::Rect,
    columns: &[f32],
    line: egui::Color32,
    background: egui::Color32,
) {
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, background);

    let points: Vec<egui::Pos2> = match columns.len() {
        0 => return,
        1 => {
            // One interval is not a line; a single point is what it is.
            painter.circle_filled(point_at(rect, 0, 1, columns[0]), 1.5, line);
            return;
        }
        count => (0..count)
            .map(|index| point_at(rect, index, count, columns[index]))
            .collect(),
    };

    painter.add(egui::Shape::line(points, egui::Stroke::new(1.0_f32, line)));
}

fn point_at(rect: egui::Rect, index: usize, count: usize, ms: f32) -> egui::Pos2 {
    let x = if count <= 1 {
        rect.center().x
    } else {
        rect.left() + rect.width() * (index as f32 / (count - 1) as f32)
    };
    let y = rect.bottom() - normalized(ms) * rect.height();
    egui::pos2(x, y.max(rect.top()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::layout;

    /// A window of `count` intervals, all `steady_ms` except `spike` at `at`.
    fn window(count: usize, steady_ms: f64, spike: Option<(usize, f64)>) -> Vec<f64> {
        let mut values = vec![steady_ms; count];
        if let Some((index, ms)) = spike {
            values[index] = ms;
        }
        values
    }

    #[test]
    fn a_window_no_wider_than_the_strip_is_drawn_one_interval_per_column() {
        // No bucketing, no padding, no interpolation: three frames are three
        // columns, which is honest about how little is known just after the layer
        // activated.
        let drawn = series(&[8.0, 9.0, 10.0]);
        assert_eq!(drawn.columns, vec![8.0, 9.0, 10.0]);
        assert_eq!(drawn.span, 1, "one interval per column");

        let full = series(&window(GRAPH_COLUMNS, 8.0, None));
        assert_eq!(full.columns.len(), GRAPH_COLUMNS);
        assert_eq!(full.span, 1);

        assert!(series(&[]).columns.is_empty());
        assert_eq!(series(&[]).span, 0);
    }

    #[test]
    fn an_early_spike_survives_more_than_a_strip_of_newer_samples() {
        // The regression this exists for: the strip used to draw the newest 120
        // intervals of the window, so a 40 ms hitch at the START of a 10 s window
        // was simply not in the picture once 2000 ordinary frames followed it --
        // while `low_1pct` in the row beside it still counted it. The window is now
        // covered in full, and the spike owns its column.
        let mut values = window(2001, 8.0, Some((0, 40.0)));
        assert!(values.len() > GRAPH_COLUMNS, "premise: more than 120 samples");
        // The newest GRAPH_COLUMNS intervals are all ordinary, which is exactly what
        // the old implementation drew.
        let newest: Vec<f64> = values[values.len() - GRAPH_COLUMNS..].to_vec();
        assert!(newest.iter().all(|ms| *ms == 8.0), "premise: quiet tail");

        let drawn = series(&values);
        assert_eq!(drawn.columns.len(), GRAPH_COLUMNS);
        assert_eq!(
            drawn.columns.first(),
            Some(&40.0),
            "the earliest spike must own its column: {:?}",
            drawn.columns
        );
        assert!(drawn.columns.iter().all(|ms| *ms >= 8.0));
        assert!(drawn.span > 1, "the window is wider than the strip");

        // And the same spike at the END of the window is still itself.
        let mut trailing = values.clone();
        let last = trailing.len() - 1;
        trailing.swap(0, last);
        assert_eq!(series(&trailing).columns.last(), Some(&40.0));
    }

    #[test]
    fn every_interval_of_the_window_is_accounted_for_by_some_column() {
        // Peak-preserving decimation is only honest if nothing is skipped: each
        // column is the maximum of a contiguous, non-overlapping run that covers
        // the whole window, so an interval above every column's value is
        // impossible.
        for len in [121usize, 200, 600, 1650, 2400, 4096] {
            let drawn = series(&window(len, 8.0, None));
            assert_eq!(drawn.columns.len(), GRAPH_COLUMNS, "len {len}");
            assert!(drawn.span >= 1, "len {len}");
            assert!(
                drawn.span * GRAPH_COLUMNS >= len,
                "len {len}: columns of {} cannot cover it",
                drawn.span
            );

            // The worst interval anywhere in the window is present in a column,
            // whatever its position: this is the property that cannot hold if any
            // interval is skipped or averaged.
            for at in [0, len / 3, len / 2, len - 1] {
                let spike = 33.0;
                let drawn = series(&window(len, 8.0, Some((at, spike))));
                assert!(
                    drawn.columns.iter().any(|ms| *ms == spike as f32),
                    "len {len}: a {spike} ms spike at {at} vanished"
                );
            }
        }
    }

    #[test]
    fn the_columns_are_oldest_first_and_within_the_scale() {
        let drawn = series(&window(1000, 8.0, Some((999, 200.0))));
        assert_eq!(drawn.columns.len(), GRAPH_COLUMNS);
        assert!(drawn.columns.iter().all(|ms| *ms <= 200.0));
        assert_eq!(drawn.columns.last(), Some(&200.0), "oldest first");
        for column in &drawn.columns {
            assert!((0.0..=1.0).contains(&normalized(*column)));
        }
    }

    #[test]
    fn the_scale_is_fixed_and_values_above_it_clamp_to_the_ceiling() {
        assert_eq!(normalized(0.0), 0.0);
        assert_eq!(normalized(FULL_SCALE_MS / 2.0), 0.5);
        assert_eq!(normalized(FULL_SCALE_MS), 1.0);
        // A 200 ms hitch is off the top of the scale: drawn at the ceiling, and
        // never above the strip.
        assert_eq!(normalized(200.0), 1.0);
        assert_eq!(normalized(-5.0), 0.0);
    }

    #[test]
    fn the_caption_reports_the_scale_and_says_when_a_column_is_a_bucket() {
        assert_eq!(scale_caption(1), "0–50 ms");
        // An empty or single-column strip prints the scale alone (`span` 0 or 1).
        assert_eq!(scale_caption(0), "0–50 ms");
        // The decimation is printed, not silent: a smooth shape over a bucketed
        // series must not read as a smooth game.
        assert_eq!(scale_caption(20), "0–50 ms · worst of 20");
    }

    #[test]
    fn the_strip_height_is_the_reserved_slot_height() {
        // One definition of the strip's height, in `layout`, shared by the window
        // size and by this renderer -- a second one here would be a number that can
        // drift from the space that was reserved.
        let slot = layout::slot_height(layout::Slot::GraphStrip, 14.0);
        assert_eq!(slot, layout::row_height(14.0) * layout::GRAPH_STRIP_ROWS);
        assert!(slot > layout::row_height(14.0));
    }

    #[test]
    fn an_empty_or_single_column_window_paints_without_panicking() {
        let height =
            crate::panel::layout::slot_height(crate::panel::layout::Slot::GraphStrip, 14.0);
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(300.0, 100.0),
        ));
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let rect =
                    egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(120.0, height));
                draw(ui, rect, &[], egui::Color32::WHITE, egui::Color32::BLACK);
                draw(ui, rect, &[8.3], egui::Color32::WHITE, egui::Color32::BLACK);
                draw(
                    ui,
                    rect,
                    &[8.3, 200.0, 9.0],
                    egui::Color32::WHITE,
                    egui::Color32::BLACK,
                );
            });
        });
    }

    #[test]
    fn every_painted_point_stays_inside_the_strip() {
        // The strip cannot draw outside its reserved slot: that is what keeps the
        // element's geometry independent of its data.
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(120.0, 34.0));
        for ms in [0.0f32, 0.001, 8.3, 33.0, FULL_SCALE_MS, 250.0] {
            let point = point_at(rect, 0, 2, ms);
            assert!(
                rect.contains(point) || (point.y - rect.top()).abs() < 0.001,
                "{ms} ms painted at {point:?}, outside {rect:?}"
            );
        }
    }
}
