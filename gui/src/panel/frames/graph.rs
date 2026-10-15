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
//! # An empty source draws an empty strip
//!
//! Nothing is drawn but the strip's own background, at the same place and the
//! same size. No zero line, no placeholder polyline: a flat line at the bottom is
//! a *claim* about frame times, and the honest rendering of "no data" is no line
//! at all.

/// How many recorded intervals the strip draws — the newest ones.
///
/// `graph_samples` in the design's config proposal. It is a sample count and not
/// a duration on purpose: the strip's width is the panel's width (a user
/// setting), so the number of points is what the drawing has to bound.
pub const GRAPH_SAMPLES: usize = 120;

/// The top of the strip's scale, in milliseconds.
///
/// 50 ms is 20 fps: everything at or above it is at the ceiling, and everything
/// below it is drawn to scale. At 60/120/165 Hz the ordinary frames sit in the
/// bottom fifth to sixth of the strip, which leaves the rest of the height free
/// for the hitches the strip is there to show.
pub const FULL_SCALE_MS: f32 = 50.0;

/// The newest [`GRAPH_SAMPLES`] intervals in milliseconds, oldest first.
pub fn samples(window_ms: &[f64]) -> Vec<f32> {
    let start = window_ms.len().saturating_sub(GRAPH_SAMPLES);
    window_ms[start..].iter().map(|ms| *ms as f32).collect()
}

/// Where an interval sits in the strip: 0.0 at the baseline, 1.0 at the ceiling.
pub fn normalized(ms: f32) -> f32 {
    (ms / FULL_SCALE_MS).clamp(0.0, 1.0)
}

/// The strip's scale, as the settings menu prints it next to the checkbox.
///
/// A strip has no text value to preview, so the menu shows what the strip's
/// height means instead of an empty cell.
pub fn scale_caption() -> String {
    format!("0–{} ms", FULL_SCALE_MS as u32)
}

/// Paint the strip into `rect`.
///
/// `rect` is the element's reserved slot, computed from the row set and the font
/// metrics before anything was read: this function never changes it, so a strip
/// that gains data cannot move the panel.
pub fn draw(
    ui: &egui::Ui,
    rect: egui::Rect,
    window_ms: &[f32],
    line: egui::Color32,
    background: egui::Color32,
) {
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, background);

    let points: Vec<egui::Pos2> = match window_ms.len() {
        0 => return,
        1 => {
            // One interval is not a line; a single point is what it is.
            painter.circle_filled(point_at(rect, 0, 1, window_ms[0]), 1.5, line);
            return;
        }
        count => (0..count)
            .map(|index| point_at(rect, index, count, window_ms[index]))
            .collect(),
    };

    painter.add(egui::Shape::line(points, egui::Stroke::new(1.0, line)));
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

    #[test]
    fn the_strip_takes_the_newest_samples_and_keeps_them_in_time_order() {
        let window: Vec<f64> = (1..=200).map(|ms| ms as f64).collect();
        let drawn = samples(&window);
        assert_eq!(drawn.len(), GRAPH_SAMPLES);
        assert_eq!(drawn.first(), Some(&81.0), "the newest 120, not the oldest");
        assert_eq!(drawn.last(), Some(&200.0));
        assert!(
            drawn.windows(2).all(|pair| pair[0] < pair[1]),
            "oldest first"
        );
    }

    #[test]
    fn fewer_samples_than_the_strip_holds_are_drawn_as_they_are() {
        // No padding and no interpolation: three frames are three points, which is
        // honest about how little is known just after the layer activated.
        assert_eq!(samples(&[8.0, 9.0, 10.0]), vec![8.0, 9.0, 10.0]);
        assert!(samples(&[]).is_empty());
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
        assert_eq!(scale_caption(), "0–50 ms");
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
    fn an_empty_or_single_sample_window_paints_without_panicking() {
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
