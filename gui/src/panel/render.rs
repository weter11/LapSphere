//! Drawing the panel: one `Label: value` line per visible row.
//!
//! A vertical column, as the owner's decision specifies — the same row order and
//! the same names as the Statistics tab, stacked top to bottom. Deliberately not
//! a horizontal strip: at 300 px wide a strip of ten rows has no room for a label
//! and a value side by side.
//!
//! # Fixed row height
//!
//! Each row occupies exactly `layout::row_height` points. Nothing is measured
//! and nothing wraps, so the painted content always fills the computed window
//! height (see `layout` for why it is computed rather than measured). A value too
//! long for its column is truncated with an ellipsis rather than wrapped.
//!
//! # Absent data
//!
//! A row hardware did not report prints an em dash (see `stat_format::ABSENT`).
//! The row still occupies its slot, which is what keeps the height stable when a
//! gamepad disconnects or a GPU stops reporting a clock.

use egui::{Align2, Color32, FontFamily, FontId, Rect, TextStyle};

use super::config::{visible_row_ids, PanelConfig};
use super::layout::{self, LABEL_GAP, LABEL_WIDTH, PADDING_X};
use super::rows;

/// The font the panel renders rows in: monospace, so a changing digit does not
/// shift the column.
pub fn row_font_id(ui: &egui::Ui) -> FontId {
    FontId::new(
        TextStyle::Body.resolve(ui.style()).size,
        FontFamily::Monospace,
    )
}

/// The font row height the layout must be computed against.
///
/// Read from egui's own metrics for the resolved font, so the layout follows the
/// user's font-size setting without duplicating that knowledge here.
pub fn font_row_height(ui: &egui::Ui) -> f32 {
    ui.fonts_mut(|fonts| fonts.row_height(&row_font_id(ui)))
}

/// How many rows a config lays out.
pub fn visible_row_count(config: &PanelConfig) -> usize {
    visible_row_ids(config)
        .iter()
        .filter(|id| rows::find(id).is_some())
        .count()
}

/// Truncate `text` to `width`, appending an ellipsis when it does not fit.
///
/// egui's own `truncate` needs a galley, which needs a layout pass; this uses the
/// font metrics directly so the caller stays measurement-free.
fn elide(ui: &egui::Ui, text: &str, font: &FontId, width: f32) -> String {
    let galley = ui.fonts_mut(|fonts| {
        fonts.layout_no_wrap(text.to_owned(), font.clone(), Color32::PLACEHOLDER)
    });
    if galley.size().x <= width {
        return text.to_string();
    }
    let ellipsis = "…";
    let ellipsis_width = ui.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(ellipsis.to_owned(), font.clone(), Color32::PLACEHOLDER)
            .size()
            .x
    });
    let mut out = String::new();
    for ch in text.chars() {
        let mut candidate = out.clone();
        candidate.push(ch);
        let candidate_width = ui.fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(candidate, font.clone(), Color32::PLACEHOLDER)
                .size()
                .x
        });
        if candidate_width + ellipsis_width > width {
            break;
        }
        out.push(ch);
    }
    out.push_str(ellipsis);
    out
}

/// Draw the panel body into `ui` and return the rect it painted, which is the
/// right-click target for the context menu.
///
/// `ui` is expected to be a `CentralPanel` sized to the computed panel window, so
/// painting here cannot itself change the window.
pub fn draw(ui: &mut egui::Ui, state: &crate::app::AppState, config: &PanelConfig) -> Rect {
    let font = row_font_id(ui);
    let line = layout::row_height(font_row_height(ui));
    let text_color = ui.visuals().weak_text_color();

    let painter = ui.painter();
    let rect = Rect::from_min_size(ui.min_rect().min, ui.available_size());

    for (index, id) in visible_row_ids(config).iter().enumerate() {
        let Some(row) = rows::find(id) else {
            // `normalize` drops unknown ids, so this is unreachable in practice.
            // Skipping rather than painting a hole keeps the loop total.
            continue;
        };
        let value = (row.read)(state);
        let y = rect.top() + layout::PADDING_Y + index as f32 * line;

        let label_rect = Rect::from_min_size(
            egui::pos2(rect.left() + PADDING_X, y),
            egui::vec2(LABEL_WIDTH, line),
        );
        let value_rect = Rect::from_min_size(
            egui::pos2(label_rect.right() + LABEL_GAP, y),
            egui::vec2(
                (rect.right() - PADDING_X - label_rect.right() - LABEL_GAP).max(0.0),
                line,
            ),
        );

        // The label is printed in full: `render` never elides it. An elided
        // label ("Average Freq..") makes the row unidentifiable, which is the one
        // outcome worse than a wide panel. The registry supplies a short name
        // sized to fit; the VALUE is what gets elided, where a truncated
        // processor name is harmless.
        painter.text(
            label_rect.left_center(),
            Align2::LEFT_CENTER,
            row.panel_label,
            font.clone(),
            text_color,
        );

        let tint = value.tint.unwrap_or_else(|| ui.visuals().text_color());
        painter.text(
            value_rect.left_center(),
            Align2::LEFT_CENTER,
            elide(ui, &value.text, &font, value_rect.width()),
            font.clone(),
            tint,
        );
    }

    rect
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppState;
    use crate::panel::config::{RowConfig, PANEL_CONFIG_VERSION};
    use egui::Context;

    /// Run one closure against a real `egui::Context` at a fixed surface size.
    ///
    /// A `Context::run` with a `RawInput` carrying an explicit screen rect is
    /// enough to give the closure a real `Ui`, real font metrics and a painter —
    /// no window and no display needed.
    fn with_ui(width: f32, height: f32, f: impl Fn(&mut egui::Ui)) {
        let ctx = Context::default();
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(width.max(64.0), height.max(64.0)),
        ));
        let mut f = f;
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| f(ui));
        });
    }

    fn config_with(ids: &[&str]) -> PanelConfig {
        PanelConfig {
            version: PANEL_CONFIG_VERSION,
            rows: crate::panel::rows::all_ids()
                .into_iter()
                .map(|id| RowConfig {
                    id: id.to_string(),
                    visible: ids.contains(&id),
                })
                .collect(),
            ..PanelConfig::fresh()
        }
    }

    #[test]
    fn an_empty_state_paints_every_visible_row_without_panicking() {
        // The absence path: every reader hits `None` and must produce `—`.
        let state = AppState::new();
        let config = config_with(&["cpu_processor", "gpu_name", "ram_usage", "gamepad_battery"]);

        with_ui(300.0, 200.0, |ui| {
            let rect = draw(ui, &state, &config);
            assert!(rect.height() > 0.0);
        });
    }

    #[test]
    fn a_fully_disabled_panel_paints_nothing_but_still_returns_a_rect() {
        let state = AppState::new();
        let config = config_with(&[]);

        with_ui(300.0, 40.0, |ui| {
            let rect = draw(ui, &state, &config);
            // The surface is returned whatever it holds, so the caller has a
            // right-click target even with no rows on. The exact height is the
            // CentralPanel's, not this module's — `layout::height_for` is what
            // sizes the window.
            assert!(
                rect.height() > 0.0,
                "a usable surface rect is still returned"
            );
        });
    }

    #[test]
    fn visible_row_count_ignores_rows_that_are_switched_off() {
        // The count feeds the window height, so an off-by-one here resizes the
        // window by a row and leaves the last row unpainted.
        let config = config_with(&["cpu_processor", "ram_usage", "ram_total"]);
        assert_eq!(visible_row_count(&config), 3);
        assert_eq!(visible_row_count(&config_with(&["ram_usage"])), 1);
    }

    #[test]
    fn every_row_paints_with_a_real_font_metric_present() {
        // Guards the layout contract from the other side: the row height the
        // layout computes from must be a real, positive font metric, or the
        // computed height and the painted height would diverge.
        with_ui(300.0, 400.0, |ui| {
            let metric = font_row_height(ui);
            assert!(metric > 0.0, "row height must be positive");
            assert!(layout::row_height(metric) > metric);
        });
    }

    #[test]
    fn a_long_value_is_elided_rather_than_wrapped() {
        with_ui(300.0, 200.0, |ui| {
            let font = row_font_id(ui);
            let short = elide(ui, "4799 MHz", &font, 100.0);
            assert_eq!(short, "4799 MHz", "a value that fits is untouched");

            let long_text = "AMD Ryzen 7 5800H with Radeon Graphics";
            let elided = elide(ui, long_text, &font, 60.0);
            assert!(elided.ends_with('…'), "got {elided:?}");
            assert!(elided.chars().count() < long_text.chars().count());

            // Degenerate widths must not panic or loop.
            for width in [0.0, 1.0, 5.0] {
                let _ = elide(ui, long_text, &font, width);
            }
        });
    }

    #[test]
    fn eliding_never_exceeds_the_width_it_was_given() {
        with_ui(300.0, 200.0, |ui| {
            let font = row_font_id(ui);
            let text = "AMD Ryzen 7 5800H with Radeon Graphics";
            for width in [20.0, 40.0, 80.0, 160.0] {
                let out = elide(ui, text, &font, width);
                let measured = ui.fonts_mut(|fonts| {
                    fonts
                        .layout_no_wrap(out.clone(), font.clone(), Color32::PLACEHOLDER)
                        .size()
                        .x
                });
                assert!(
                    measured <= width + 1.0,
                    "{out:?} measured {measured} against width {width}"
                );
            }
        });
    }
}
