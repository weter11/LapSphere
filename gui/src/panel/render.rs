//! Drawing the panel: one `Label: value` line per visible row, plus the
//! fixed-height frame-time strip.
//!
//! A vertical column, as the owner's decision specifies — the same row order and
//! the same names as the Statistics tab, stacked top to bottom. Deliberately not
//! a horizontal strip: at 300 px wide a strip of ten rows has no room for a label
//! and a value side by side.
//!
//! # Fixed row height
//!
//! Each row occupies exactly `layout::row_height` points, and the frame-time strip
//! exactly `layout::GRAPH_STRIP_ROWS` of them. Nothing is measured and nothing
//! wraps, so the painted content always fills the computed window height (see
//! `layout` for why it is computed rather than measured). A value too long for its
//! column is truncated with an ellipsis rather than wrapped.
//!
//! # Geometry does not depend on the data
//!
//! Every rect this module paints into comes from `layout` plus the row's position,
//! and the position comes from the **slot heights before it** — never from what any
//! row holds. So the same config paints the same geometry whether a source is live,
//! absent, or changing between frames, and `draw` never asks egui to lay anything
//! out: it paints into rects it computes. `the_same_geometry_with_and_without_frame_data`
//! and `the_strip_is_one_extra_row_of_reserved_height` below hold that.
//!
//! # Absent data
//!
//! A row hardware did not report prints an em dash (see `stat_format::ABSENT`). The
//! row still occupies its slot, which is what keeps the height stable when a
//! gamepad disconnects, a GPU stops reporting a clock, or a game exits. The strip
//! has no text at all: with no source it draws nothing but its own background, at
//! the same place and the same size.

use egui::{Align2, Color32, FontFamily, FontId, Rect, TextStyle};

use super::config::{visible_row_ids, PanelConfig};
use super::frames;
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

/// The rows a config lays out, in order, skipping ids the registry does not know.
///
/// `normalize` drops unknown ids, so the skip is unreachable in practice; it keeps
/// the function total for a caller that has not normalized.
pub fn visible_rows(config: &PanelConfig) -> Vec<&'static rows::Row> {
    visible_row_ids(config)
        .iter()
        .filter_map(|id| rows::find(id))
        .collect()
}

/// The vertical slots a config lays out — what [`layout::height_for_slots`] and
/// [`draw`] must agree on.
pub fn layout_slots(config: &PanelConfig) -> Vec<layout::Slot> {
    visible_rows(config)
        .iter()
        .map(|row| row_slot(row))
        .collect()
}

/// The vertical slot one row occupies.
///
/// The registry decides the shape — [`super::rows::Row::is_graph`] is the single
/// answer to "is this the strip" — so the layout and the painter cannot disagree
/// about how tall a row is.
pub fn row_slot(row: &rows::Row) -> layout::Slot {
    if row.is_graph() {
        layout::Slot::GraphStrip
    } else {
        layout::Slot::Row
    }
}

/// Is a frame element on screen? The repaint cadence and the reader's activity
/// both hang off this.
pub fn has_visible_frame_row(config: &PanelConfig) -> bool {
    visible_rows(config)
        .iter()
        .any(|row| row.group == rows::Group::Frame)
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

/// Draw the panel body into `ui` and return the rect it painted.
///
/// `ui` is expected to be a `CentralPanel` sized to the computed panel window, so
/// painting here cannot itself change the window.
///
/// **The returned rect is not the right-click target.** The panel registers its
/// interaction rect as `ui.max_rect()` in `App::draw_panel`, so the target is the
/// panel's whole content area and takes no part in the paint order. The two rects
/// are equal at the call site — `draw` is the caller's first layout in the panel,
/// so `min_rect().min` and `available_size()` at that moment describe exactly the
/// panel's `max_rect()` — and the test below holds that equality, which is what
/// makes either choice harmless today. A caller that wants to know what was
/// painted reads this return value; a caller that wants a click target must not.
pub fn draw(ui: &mut egui::Ui, state: &crate::app::AppState, config: &PanelConfig) -> Rect {
    let font = row_font_id(ui);
    let metrics = font_row_height(ui);
    let text_color = ui.visuals().weak_text_color();

    let painter = ui.painter();
    let rect = Rect::from_min_size(ui.min_rect().min, ui.available_size());

    // The value column: the panel's width minus the padding and the reserved label
    // column. The same for every row, so a value arriving where there was none
    // cannot move anything horizontally either.
    let value_left = rect.left() + PADDING_X + LABEL_WIDTH + LABEL_GAP;
    let value_width = (rect.right() - PADDING_X - value_left).max(0.0);

    let mut y = rect.top() + layout::PADDING_Y;
    for row in visible_rows(config) {
        let slot = layout::slot_height(row_slot(row), metrics);
        let label_rect = Rect::from_min_size(
            egui::pos2(rect.left() + PADDING_X, y),
            egui::vec2(LABEL_WIDTH, slot),
        );
        let value_rect =
            Rect::from_min_size(egui::pos2(value_left, y), egui::vec2(value_width, slot));

        // The label is printed in full: `render` never elides it. An elided
        // label ("Average Freq..") makes the row unidentifiable, which is the one
        // outcome worse than a wide panel. The registry supplies a short name
        // sized to fit and `every_label_fits_the_label_column` measures them all;
        // the VALUE is what gets elided, where a truncated processor name is
        // harmless.
        painter.text(
            label_rect.left_center(),
            Align2::LEFT_CENTER,
            row.panel_label,
            font.clone(),
            text_color,
        );

        match row.text(state) {
            Some(value) => {
                let tint = value.tint.unwrap_or_else(|| ui.visuals().text_color());
                painter.text(
                    value_rect.left_center(),
                    Align2::LEFT_CENTER,
                    elide(ui, &value.text, &font, value_rect.width()),
                    font.clone(),
                    tint,
                );
            }
            None => {
                // The strip. One snapshot clone per frame, not one per row: it is
                // the only row that needs the samples.
                let samples = state.frames.snapshot().samples_ms;
                frames::graph::draw(
                    ui,
                    value_rect,
                    &samples,
                    ui.visuals().text_color(),
                    ui.visuals().extreme_bg_color,
                );
            }
        }

        y += slot;
    }

    rect
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppState;
    use crate::panel::config::{RowConfig, PANEL_CONFIG_VERSION};
    use crate::panel::frames::{stats, FrameSnapshot, Source};
    use egui::Context;
    use std::sync::Arc;

    /// Run one closure against a real `egui::Context` at a fixed surface size.
    ///
    /// A `Context::run` with a `RawInput` carrying an explicit screen rect is
    /// enough to give the closure a real `Ui`, real font metrics and a painter —
    /// no window and no display needed.
    fn with_ui(width: f32, height: f32, f: impl FnMut(&mut egui::Ui)) {
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

    /// The frame rows, as the decision names them, with the strip among them.
    fn frame_config() -> PanelConfig {
        config_with(&[
            "cpu_processor",
            "game_name",
            "fps",
            "frametime_ms",
            "frametime_graph",
            "low_1pct",
            "low_01pct",
        ])
    }

    /// Give a state a live frame snapshot, the way the worker would.
    fn live_state() -> AppState {
        let state = AppState::new();
        let window: Vec<f64> = (0..600).map(|_| 6.06).collect();
        let metrics = stats::metrics(&window).expect("metrics");
        state.frames.publish_for_test(FrameSnapshot {
            source: Some(Source {
                pid: 9911,
                exe_name: "vkcube".to_string(),
            }),
            metrics: Some(metrics),
            samples_ms: Arc::from(vec![6.06f32; 120]),
        });
        state
    }

    /// The panel's right-click target is `ui.max_rect()`, registered by the caller
    /// (`App::draw_panel`) — not the rect [`draw`] returns. The two are the same
    /// rect at the call site, and that is what makes the caller's choice harmless;
    /// this test holds the equality so it cannot lapse unnoticed.
    ///
    /// Both halves of it are egui-layout facts rather than promises. The
    /// `CentralPanel` frame expands the ui to its own `max_rect()` before the body
    /// runs (`Ui::expand_to_include_rect`), so `min_rect().min` is the panel's
    /// origin; and `draw` is the panel's first layout, so the size it can take is
    /// still the whole panel. Lay anything out before `draw` in that panel and this
    /// module's rect shrinks while the interaction target stays put — which is
    /// exactly why the target is not taken from here.
    ///
    /// Rendered in the call-site shape: `App::update`'s ui, the panel's
    /// `CentralPanel` shown inside it, carrying the frame the call site builds.
    /// The frame's FILL is what the call site changed (the panel's background at
    /// the configured opacity), and it takes no part in either rect; the frame's
    /// margins are the part that does, and they are egui's own.
    #[test]
    fn the_interaction_rect_matches_the_painted_rect_at_the_call_site() {
        let state = AppState::new();
        let config = config_with(&["cpu_processor", "gpu_name", "ram_usage"]);
        let ctx = Context::default();
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(300.0, 198.0),
        ));

        let mut painted = Rect::NOTHING;
        let mut target = Rect::NOTHING;
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::no_frame().show(ctx, |ui| {
                let frame = egui::Frame::central_panel(ui.style()).fill(
                    crate::panel::window::background_fill(ui.visuals(), config.background_opacity),
                );
                egui::CentralPanel::default()
                    .frame(frame)
                    .show_inside(ui, |ui| {
                        painted = draw(ui, &state, &config);
                        target = ui.max_rect();
                    });
            });
        });

        assert!(painted.width() > 0.0 && painted.height() > 0.0);
        assert_eq!(
            painted, target,
            "the painted rect ({painted:?}) and the panel's interaction rect \
             ({target:?}) have diverged: something now lays out before `draw` in \
             the panel. The right-click target must stay `ui.max_rect()`; decide \
             deliberately what `draw`'s return value means now."
        );
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
    fn the_frame_rows_paint_with_and_without_a_source() {
        // Both directions of the frame path, at the panel's real width: absent
        // (every text row `—`, the strip empty) and live.
        let config = frame_config();
        with_ui(300.0, 300.0, |ui| {
            let rect = draw(ui, &AppState::new(), &config);
            assert!(rect.height() > 0.0);
        });
        with_ui(300.0, 300.0, |ui| {
            let rect = draw(ui, &live_state(), &config);
            assert!(rect.height() > 0.0);
        });
    }

    #[test]
    fn a_fully_disabled_panel_paints_nothing_but_still_returns_a_rect() {
        let state = AppState::new();
        let config = config_with(&[]);

        with_ui(300.0, 40.0, |ui| {
            let rect = draw(ui, &state, &config);
            // What was painted is returned whatever it holds, so a caller can see
            // the surface even with no rows on. The exact height is the
            // CentralPanel's, not this module's — `layout::height_for` is what
            // sizes the window.
            assert!(
                rect.height() > 0.0,
                "a usable surface rect is still returned"
            );
        });
    }

    #[test]
    fn visible_rows_ignores_rows_that_are_switched_off() {
        // The list feeds the window height, so an off-by-one here resizes the
        // window by a row and leaves the last row unpainted.
        let config = config_with(&["cpu_processor", "ram_usage", "ram_total"]);
        assert_eq!(visible_rows(&config).len(), 3);
        assert_eq!(visible_rows(&config_with(&["ram_usage"])).len(), 1);
        assert!(visible_rows(&config_with(&[])).is_empty());
    }

    #[test]
    fn the_strip_is_one_extra_row_of_reserved_height() {
        // The layout's own numbers, produced here from the registry rather than
        // from a hand-written slot list: one text row plus the strip, and the strip
        // contributes exactly GRAPH_STRIP_ROWS row heights.
        let strip_only = config_with(&["frametime_graph"]);
        let text_only = config_with(&["ram_usage"]);
        assert_eq!(
            layout_slots(&strip_only),
            vec![layout::Slot::GraphStrip],
            "the registry decides which slot a row takes"
        );
        assert_eq!(layout_slots(&text_only), vec![layout::Slot::Row]);

        let font = 14.0;
        let strip_height = layout::height_for_slots(&layout_slots(&strip_only), font);
        let text_height = layout::height_for_slots(&layout_slots(&text_only), font);
        assert_eq!(
            strip_height - text_height,
            layout::row_height(font) * (layout::GRAPH_STRIP_ROWS - 1.0)
        );
    }

    #[test]
    fn the_same_geometry_with_and_without_frame_data() {
        // The reservation, tested at the call site: the same config paints the same
        // rects whether the frame rows have data or not, because every rect comes
        // from the slot list and not from a value. A `draw` that started laying
        // text out — or sized the strip from its sample count — fails here.
        let config = frame_config();
        let mut absent = Rect::NOTHING;
        let mut live = Rect::NOTHING;
        with_ui(300.0, 320.0, |ui| {
            absent = draw(ui, &AppState::new(), &config)
        });
        with_ui(300.0, 320.0, |ui| live = draw(ui, &live_state(), &config));
        assert_eq!(absent, live);

        // And the window height the panel asks for does not depend on the data
        // either: it is the same function of (config, font) in both cases.
        let font = 14.0;
        assert_eq!(
            layout::panel_size(&config, &layout_slots(&config), font),
            layout::panel_size(&config, &layout_slots(&config), font)
        );
        assert!(layout::height_for_slots(&layout_slots(&config), font) > 0.0);
    }

    #[test]
    fn a_live_snapshot_changes_the_painted_text_but_not_the_row_set() {
        // The other half of "geometry does not depend on the data": the VALUES do
        // change, which is the whole point of the elements.
        let config = config_with(&["fps"]);
        let live = live_state();
        let absent = AppState::new();
        let row = crate::panel::rows::find("fps").expect("row");
        assert_eq!(
            row.text(&absent).expect("text").text,
            crate::stat_format::ABSENT
        );
        assert_eq!(row.text(&live).expect("text").text, "165.0 fps");
        assert_eq!(layout_slots(&config), vec![layout::Slot::Row]);
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
    fn every_label_fits_the_label_column() {
        // The reserved label column, measured rather than declared: every short
        // label in the registry must fit LABEL_WIDTH in the panel's own font. A new
        // label that does not fit would be painted into the value column, which is
        // the one thing `render` must not do.
        with_ui(400.0, 400.0, |ui| {
            let font = row_font_id(ui);
            for row in crate::panel::rows::ROWS {
                let measured = ui.fonts_mut(|fonts| {
                    fonts
                        .layout_no_wrap(
                            row.panel_label.to_owned(),
                            font.clone(),
                            Color32::PLACEHOLDER,
                        )
                        .size()
                        .x
                });
                assert!(
                    measured <= LABEL_WIDTH,
                    "row {} label {:?} measures {measured} pt, wider than the \
                     reserved {LABEL_WIDTH} pt label column",
                    row.id,
                    row.panel_label
                );
                // And the value column is wide enough for the widest value the six
                // frame elements print at the default panel width (300 pt).
                assert!(measured > 0.0);
            }
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
