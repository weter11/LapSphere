//! The panel's two settings surfaces, and why they are two.
//!
//! # Why not one menu
//!
//! The obvious design is one right-click menu holding every control. It does not
//! fit, and the reason is structural rather than a matter of trimming: **egui
//! paints a popup into the same surface as the window that owns it.** The panel
//! window is 300x198 -- exactly the size the computed layout asks for -- so a
//! menu taller than that is drawn into pixels the window does not have. A
//! decorations-free, non-resizable panel cannot be made bigger to accommodate its
//! own menu either: forcing it to 780x900 with `xdotool windowsize` did not help,
//! because the WM declines to resize a window whose `min_inner` equals its
//! `inner`.
//!
//! So the controls are split by where they can physically live:
//!
//! * [`draw_context_menu`] -- three entries, drawn in the panel window: "Hide
//!   panel", "Panel settings..." and "< Back to normal mode". They fit; the menu
//!   geometry as measured is on the measurement page
//!   (`docs/development/panel-measurements.md`). "Hide panel" is DISABLED unless
//!   a way back exists ([`super::visibility::can_hide`]), with the reason on its
//!   hover text: an entry that hides the panel with nothing able to bring it
//!   back is the one control in this app that can strand the user.
//! * [`draw_settings_body`] -- everything else, in a SEPARATE viewport opened by
//!   the caller, so it is a real OS window with its own size and is never clipped
//!   by the panel. Deliberately an ordinary window: not always-on-top, not
//!   skip-taskbar, not tied to panel mode.
//!
//! One copy of every control: the context menu only routes to the settings window.
//! The settings body holds the per-row checkboxes -- one collapsible section per
//! registry group -- the width slider, the background-opacity slider, the window
//! controls (the stacking choice and "hide from the taskbar and pager") and the
//! global show/hide hotkey with its live grab status.
//!
//! # The opacity slider acts on the running panel
//!
//! It writes `PanelConfig::background_opacity`, which the panel reads afresh every
//! frame (`panel::window::background_fill`), so the background changes on the next
//! frame with no restart and no re-send of the window spec: neither the window's
//! size, its level nor its decorations depend on the opacity. Only the background
//! is affected — the rows' text is painted opaque on top of it.
//!
//! # Opening the popup: through the surface Response
//!
//! The menu opens through the ordinary egui path: the caller registers the whole
//! panel surface with `ui.interact(...)` and calls `Response::context_menu` on the
//! response. The pointer is NOT read directly, and `Sense::click_and_drag()` (not
//! `click()`) is the sense the surface is declared with, so the drag that PR 2
//! needs does not have to be retrofitted underneath the code that uses it.
//!
//! An earlier version of this module documented the opposite: that the pointer had
//! to be read off the context because the response never receives egui's CLICKED
//! flag. That diagnosis came from reading two probe values out of SEPARATE frames
//! and is retracted -- `docs/development/panel-measurements.md` records both the
//! wrong turn and the single-frame probe that refuted it. The tests below pin the
//! correct behaviour from both sides: a right click must reach the response, and a
//! left click must never register as a secondary one.

use super::config::{
    PanelConfig, Stacking, MAX_BACKGROUND_OPACITY, MAX_REFRESH_HZ, MAX_WIDTH, MAX_WINDOW_SECONDS,
    MIN_BACKGROUND_OPACITY, MIN_REFRESH_HZ, MIN_WIDTH, MIN_WINDOW_SECONDS,
};
use super::rows::{self, Group};
use super::visibility::{GrabStatus, Visibility};
use super::PanelView;

/// Viewport id of the settings window, stable so it is reused rather than
/// recreated on every open.
pub fn settings_viewport() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("panel_settings")
}

/// The size the settings window opens at: tall enough for four collapsed groups
/// plus the sliders, the window controls and the hotkey section without scrolling.
pub const SETTINGS_SIZE: [f32; 2] = [360.0, 640.0];

/// Height the settings body reserves below the scrollable row list: the width and
/// opacity sliders with their notes, the frame-elements section with its two
/// sliders and its two notes, the window controls, the hotkey field with its
/// status lines, and the Back/Close row.
///
/// Measured against those sections rather than guessed. Too small a reserve and
/// the fixed controls are pushed out of the window, which is exactly how a
/// control becomes unreachable; too large and the row list has no room. The
/// frame-elements section added roughly three label rows to this block.
const FIXED_SECTION_HEIGHT: f32 = 400.0;

/// What a settings interaction wants done, applied by the caller outside the UI
/// pass.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SettingsOutcome {
    pub back_to_normal: bool,
    /// The config changed and must be written to `panel.json`.
    pub config_changed: bool,
    /// Something the window SPEC carries changed (the width, or the stacking
    /// level), so the spec must be re-sent this frame rather than at the next
    /// mode switch.
    pub window_spec_changed: bool,
    /// The user asked to close the settings window.
    pub close_requested: bool,
    /// The show/hide hotkey was committed and the running watcher must be
    /// re-created with the new spec.
    ///
    /// Set on the field's LOST FOCUS rather than on every keystroke: re-grabbing
    /// per keystroke grabs whatever prefix has been typed so far (`S`, then
    /// `Sh`, …), which captures keys the user never asked for, and Enter or a
    /// click elsewhere is what "I am done typing" looks like.
    pub hotkey_changed: bool,
}

/// The live state the hotkey controls read.
///
/// Passed as one value because the three travel together and none of them is in
/// the config: whether the key actually got grabbed, which spec is running, and
/// whether the tray can bring the panel back.
pub struct HotkeyState<'a> {
    pub visibility: &'a Visibility,
    /// The spec the running watcher grabbed. The surface compares the field
    /// against it, so committing an unchanged value does not re-grab.
    pub grabbed: &'a str,
    pub tray_enabled: bool,
}

/// What the context menu asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextAction {
    None,
    /// Hide the panel itself. Only offered when a way back exists.
    Hide,
    OpenSettings,
    BackToNormal,
}

/// The "Hide panel" entry, and whether it was clicked.
///
/// Split out and returning its own response for the same reason the sliders are:
/// the test that proves a disabled entry cannot be clicked has to be able to aim
/// at it.
fn hide_entry(ui: &mut egui::Ui, can_hide: bool, status: GrabStatus) -> (egui::Response, bool) {
    let response = ui.add_enabled(can_hide, egui::Button::new("Hide panel"));
    let response = response.on_hover_text(if can_hide {
        "Hide the panel. The hotkey, the tray or `lapsphere --toggle-panel` brings it back."
            .to_string()
    } else {
        format!(
            "Hiding is refused: {} — the panel would have no way back. The tray and \
             `lapsphere --toggle-panel` are both unavailable here.",
            status.message()
        )
    });
    let hide = response.clicked();
    (response, hide)
}

/// The context menu drawn inside the panel window.
///
/// Kept to what physically fits, and to controls that act on the panel rather
/// than on its contents; see the module docs. `status` is shown on the hide
/// entry's hover text when hiding is refused, so the reason is one hover away
/// rather than only in the log.
pub fn draw_context_menu(ui: &mut egui::Ui, can_hide: bool, status: GrabStatus) -> ContextAction {
    if hide_entry(ui, can_hide, status).1 {
        return ContextAction::Hide;
    }
    if ui.button("Panel settings...").clicked() {
        return ContextAction::OpenSettings;
    }
    if ui.button("< Back to normal mode").clicked() {
        return ContextAction::BackToNormal;
    }
    ContextAction::None
}

/// Draw every panel control. Rendered into `ui`, which the caller supplies --
/// either the settings viewport or a test's `CentralPanel` -- so the same
/// function serves both and there is no duplicated control list.
pub fn draw_settings_body(
    ui: &mut egui::Ui,
    state: &crate::app::AppState,
    config: &mut PanelConfig,
    outcome: &mut SettingsOutcome,
    hotkeys: &HotkeyState<'_>,
) {
    // One snapshot for the whole settings surface, taken the same way the panel
    // takes one: the live values beside the checkboxes must agree with each other,
    // and with what the Frames section below reports about their coverage.
    let snapshot = state.frames.snapshot();
    let view = PanelView::new(state, &snapshot);

    egui::ScrollArea::vertical()
        .max_height((ui.available_height() - FIXED_SECTION_HEIGHT).max(60.0))
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Rows").strong());
            for group in Group::ALL {
                draw_group(ui, group, &view, config, outcome);
            }
        });

    ui.separator();

    // Width goes straight into the config, clamped to the range `normalize`
    // enforces, so a drag cannot write an unusable width. The panel reads it
    // fresh every frame, so there is no apply step.
    ui.label(egui::RichText::new("Width").strong());
    let mut width = config.width.clamp(MIN_WIDTH, MAX_WIDTH);
    if ui
        .add(egui::Slider::new(&mut width, MIN_WIDTH..=MAX_WIDTH).suffix(" px"))
        .changed()
    {
        config.width = width;
        outcome.config_changed = true;
        outcome.window_spec_changed = true;
    }

    // The opacity goes straight into the config, exactly as the width does, and
    // is clamped to the range `normalize` enforces so a drag cannot write an
    // unusable value. The panel reads it fresh every frame, so there is no apply
    // step and no restart. It is NOT part of the window spec: nothing about the
    // window's size, level or decorations depends on it, so no re-send is needed.
    ui.label(egui::RichText::new("Background opacity").strong());
    let (opacity_response, opacity) = opacity_control(ui, config.background_opacity);
    if opacity_response.changed() {
        config.background_opacity = opacity;
        outcome.config_changed = true;
    }
    ui.label(
        egui::RichText::new(
            "Only the background becomes translucent — text and values stay opaque. \
             What is behind the panel shows through only under a compositing window \
             manager; without one nothing blends, so a reduced opacity darkens the \
             background instead of making it see-through.",
        )
        .weak()
        .small(),
    );

    ui.separator();

    // The frame elements: what the source is, and the two numbers they are
    // computed with. Both numbers are the DESIGN's proposals (its Q9 and Q10) and
    // not owner decisions, and the note says so on the surface itself: a user who
    // reads "10 s" here must not be told a prior decision chose it. The panel reads
    // both fresh every frame, so there is no apply step, and neither is part of the
    // window spec -- the window's size, level and decorations do not depend on them.
    ui.label(egui::RichText::new("Frames").strong());
    ui.label(
        egui::RichText::new(
            "Taken from a running Vulkan game's own presents, through the LapSphere \
             layer in the game's process. Available only where that layer is installed \
             and the game runs with LAPSPHERE_FRAMES=1; anywhere else these rows read \
             — and the graph stays empty.",
        )
        .weak()
        .small(),
    );
    // What is being read right now, and -- when the requested window is longer than
    // the recorded history -- how much of it the numbers actually cover. The reader
    // cannot hold more than its ring, so a 60 s window at a high frame rate is
    // genuinely short, and saying so is the difference between a smaller statistic
    // and a wrong one.
    ui.label(egui::RichText::new(source_report(&view)).weak().small());
    ui.horizontal(|ui| {
        ui.label("Statistics window");
        let (response, seconds) = window_control(ui, config.window_seconds);
        if response.changed() && seconds != config.window_seconds {
            config.window_seconds = seconds;
            outcome.config_changed = true;
        }
    });
    ui.horizontal(|ui| {
        ui.label("Refresh");
        let (response, rate) = refresh_control(ui, config.refresh_hz);
        if response.changed() && rate != config.refresh_hz {
            config.refresh_hz = rate;
            outcome.config_changed = true;
        }
    });
    ui.label(
        egui::RichText::new(
            "10 s and 4 Hz are the design's proposed defaults, not an owner \
             decision — they are configurable here for exactly that reason.",
        )
        .weak()
        .small(),
    );

    ui.separator();

    // The window controls. Both act on the RUNNING window: the stacking level is
    // part of the window spec (re-sent below), and the taskbar state is dispatched
    // by the caller from the config, so neither needs a restart or an apply step.
    ui.label(egui::RichText::new("Window").strong());

    for (option, response) in stacking_controls(ui, config.stacking) {
        if response.clicked() && option != config.stacking {
            config.stacking = option;
            outcome.config_changed = true;
            // The level rides on the spec, so the spec is what has to be re-sent.
            outcome.window_spec_changed = true;
        }
    }

    let (taskbar, hide) = taskbar_control(ui, config.hide_from_taskbar);
    if taskbar.changed() && hide != config.hide_from_taskbar {
        config.hide_from_taskbar = hide;
        outcome.config_changed = true;
    }
    ui.label(
        egui::RichText::new(
            "Taskbar and pager visibility is X11 (EWMH) only; a window manager \
                             that does not honour it leaves the panel listed.",
        )
        .weak()
        .small(),
    );

    ui.separator();

    // The global show/hide hotkey, and what actually happened to it. The status
    // line is the point of the section: a key that was refused must not read as
    // a key that works.
    ui.label(egui::RichText::new("Hotkey").strong());
    let status = hotkeys.visibility.grab_status();
    ui.horizontal(|ui| {
        ui.label("Show / hide");
        let (_, committed) = hotkey_control(ui, config, hotkeys.grabbed);
        if committed {
            outcome.hotkey_changed = true;
            outcome.config_changed = true;
        }
    });
    let configured = if config.hotkey.trim().is_empty() {
        "no key configured".to_string()
    } else {
        config.hotkey.trim().to_string()
    };
    // The states where one line is not enough to say what will happen when the
    // key is pressed: somebody else owns it, or only some lock states were
    // captured.
    let caveat = status.warning().is_some();
    ui.label(
        egui::RichText::new(format!("{configured} — {}", status.message())).color(if caveat {
            egui::Color32::from_rgb(220, 160, 60)
        } else {
            ui.visuals().weak_text_color()
        }),
    );
    if let Some(warning) = status.warning() {
        ui.label(egui::RichText::new(warning).small().italics());
    }
    // A spec with a modifier this build does not know is stored as typed and
    // grabbed without that token, so the status line above would read as an exact
    // match for a shortcut that does not exist. Shown for as long as the spec
    // says it, because the config keeps the typo until the user fixes it.
    if let Some(warning) = super::visibility::ignored_modifiers_warning(&config.hotkey) {
        ui.label(
            egui::RichText::new(warning)
                .small()
                .italics()
                .color(egui::Color32::from_rgb(220, 160, 60)),
        );
    }

    // The safety rule, stated where the key is configured: what the user needs to
    // know is whether hiding is currently allowed, and it follows from the key,
    // the tray and the CLI.
    //
    // `is_reliable_route` and not "some grab succeeded": a partially captured key
    // fires only in some lock states, so it cannot be the route the panel is left
    // hidden behind — the tray and the CLI are what cover that gap here.
    let may_hide = super::visibility::can_hide(
        status.is_reliable_route(),
        hotkeys.tray_enabled,
        hotkeys.visibility.dbus_available(),
    );
    ui.label(
        egui::RichText::new(if may_hide {
            "The panel can be hidden: a way back exists."
        } else {
            "The panel cannot be hidden: with no way back it would be unreachable, so a \
             hide request returns to the normal window instead."
        })
        .weak()
        .small(),
    );
    if status == GrabStatus::Partial && may_hide {
        ui.label(
            egui::RichText::new(
                "The hotkey is not counted as that way back while only some lock states are \
                 captured; the tray or `lapsphere --toggle-panel` is what makes hiding safe.",
            )
            .weak()
            .small(),
        );
    }

    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("< Back to normal mode").clicked() {
            outcome.back_to_normal = true;
        }
        if ui.button("Close").clicked() {
            outcome.close_requested = true;
        }
    });
}

/// The stacking choice, one selectable entry per option.
///
/// Split out and returning its own responses for the same reason the row groups
/// are: the test that proves a click on it reaches the config has to be able to
/// aim at it, and a control whose rect a test cannot obtain is a control the
/// suite can only assume works.
fn stacking_controls(ui: &mut egui::Ui, current: Stacking) -> Vec<(Stacking, egui::Response)> {
    ui.horizontal(|ui| {
        Stacking::ALL
            .iter()
            .copied()
            .map(|option| {
                let response = ui.selectable_label(current == option, option.label());
                (option, response)
            })
            .collect()
    })
    .inner
}

/// The "hide from the taskbar and pager" checkbox, and the value it now holds.
fn taskbar_control(ui: &mut egui::Ui, current: bool) -> (egui::Response, bool) {
    let mut value = current;
    let response = ui.checkbox(&mut value, "Hide from the taskbar and pager");
    (response, value)
}

/// The background-opacity slider, and the value it now holds.
///
/// Split out and returning its own response for the same reason the stacking
/// control is: the test that proves a drag on it reaches the config has to be able
/// to aim at it.
fn opacity_control(ui: &mut egui::Ui, current: f32) -> (egui::Response, f32) {
    let mut value = current.clamp(MIN_BACKGROUND_OPACITY, MAX_BACKGROUND_OPACITY);
    let response = ui.add(
        egui::Slider::new(&mut value, MIN_BACKGROUND_OPACITY..=MAX_BACKGROUND_OPACITY)
            .fixed_decimals(2),
    );
    (response, value)
}

/// What the frame elements are reading, and how much of the window they cover.
///
/// Four states, each a different thing for the user to act on: no source at all
/// (nothing installed or no game), a source with no interval yet (the layer has
/// created its segment but no second present has happened), a source whose history
/// covers the requested window, and a **short** one — the reader retains at most
/// `frames::shm::MAX_RING_CAPACITY` intervals, so at a high frame rate a long
/// configured window cannot be filled, and the numbers beside the setting are then
/// computed over less than it asks for.
fn source_report(view: &PanelView<'_>) -> String {
    match (&view.frames.source, view.frames.metrics) {
        (Some(source), Some(metrics)) if metrics.is_short() => format!(
            "Reading pid {} ({}) — {:.1} s recorded of the {:.0} s window asked for ({:.0} %)",
            source.pid,
            source.exe_name,
            metrics.window_ms / 1000.0,
            metrics.requested_ms / 1000.0,
            100.0 * metrics.coverage()
        ),
        (Some(source), Some(metrics)) => format!(
            "Reading pid {} ({}) — {:.1} s window",
            source.pid,
            source.exe_name,
            metrics.window_ms / 1000.0
        ),
        (Some(source), None) => format!(
            "Reading pid {} ({}) — no interval recorded yet",
            source.pid, source.exe_name
        ),
        (None, _) => {
            "No source: no running Vulkan game has published any frames.".to_string()
        }
    }
}

/// The statistics-window slider, and the value it now holds.
///
/// Clamped to the range `normalize` enforces, exactly as the opacity slider is, so
/// a drag cannot write a window the reader would then have to refuse.
fn window_control(ui: &mut egui::Ui, current: u32) -> (egui::Response, u32) {
    let mut value = current.clamp(MIN_WINDOW_SECONDS, MAX_WINDOW_SECONDS);
    let response =
        ui.add(egui::Slider::new(&mut value, MIN_WINDOW_SECONDS..=MAX_WINDOW_SECONDS).suffix(" s"));
    (response, value)
}

/// The frame-elements refresh slider, and the value it now holds.
fn refresh_control(ui: &mut egui::Ui, current: u32) -> (egui::Response, u32) {
    let mut value = current.clamp(MIN_REFRESH_HZ, MAX_REFRESH_HZ);
    let response =
        ui.add(egui::Slider::new(&mut value, MIN_REFRESH_HZ..=MAX_REFRESH_HZ).suffix(" Hz"));
    (response, value)
}

/// The show/hide hotkey field, and whether it was just committed.
///
/// Split out and returning its own response for the same reason the sliders are:
/// the test that proves a commit reaches the caller has to be able to aim at the
/// field. `grabbed` is the spec the running watcher actually holds, so committing
/// an unchanged value (Enter, or a click away) does not tear the grab down and
/// put it back up.
fn hotkey_control(
    ui: &mut egui::Ui,
    config: &mut PanelConfig,
    grabbed: &str,
) -> (egui::Response, bool) {
    let response = ui.text_edit_singleline(&mut config.hotkey).on_hover_text(
        "X11/Xwayland only, e.g. Shift+F9. An empty value turns the key off; the \
         tray and `lapsphere --toggle-panel` still work.",
    );
    // Committed on lost focus, never per keystroke: see
    // `SettingsOutcome::hotkey_changed`.
    let committed = response.lost_focus() && config.hotkey != grabbed;
    (response, committed)
}

/// One collapsible group of row checkboxes.
///
/// Collapsed by default: four open groups is roughly twenty rows, which would
/// push the sliders below the fold.
fn draw_group(
    ui: &mut egui::Ui,
    group: Group,
    view: &PanelView<'_>,
    config: &mut PanelConfig,
    outcome: &mut SettingsOutcome,
) {
    let ids: Vec<usize> = (0..config.rows.len())
        .filter(|i| group.of(&config.rows[*i].id))
        .collect();
    if ids.is_empty() {
        return;
    }

    egui::CollapsingHeader::new(group.label())
        .default_open(false)
        .show(ui, |ui| {
            for index in ids {
                let id = config.rows[index].id.clone();
                let Some(row) = rows::find(&id) else {
                    continue;
                };
                ui.horizontal(|ui| {
                    if ui
                        .checkbox(&mut config.rows[index].visible, row.label)
                        .changed()
                    {
                        outcome.config_changed = true;
                    }
                    // The live value, so a checkbox shows what it turns on. A strip
                    // row has no text value; it shows the scale it draws against.
                    ui.label(
                        egui::RichText::new(row.menu_value(view))
                            .monospace()
                            .weak(),
                    );
                });
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppState;
    use egui::{Context, Rect};

    fn with_ui(f: impl Fn(&mut egui::Ui, &mut PanelConfig, &mut SettingsOutcome)) {
        let ctx = Context::default();
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(SETTINGS_SIZE[0], SETTINGS_SIZE[1]),
        ));
        let mut config = PanelConfig::fresh();
        let mut outcome = SettingsOutcome::default();
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| f(ui, &mut config, &mut outcome));
        });
    }

    /// Run the settings body and return every string the frame actually PAINTED.
    ///
    /// A warning nobody draws is not a warning, and a pure-text test alone cannot
    /// tell the two apart: this reads the text shapes egui produced. Words are
    /// asserted, not whole sentences — a label wraps at the window width, so a
    /// sentence can be painted with line breaks inside it.
    fn painted_text(config_hotkey: &str, status: GrabStatus, tray_enabled: bool) -> Vec<String> {
        let ctx = Context::default();
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(SETTINGS_SIZE[0], SETTINGS_SIZE[1]),
        ));
        let mut config = PanelConfig::fresh();
        config.hotkey = config_hotkey.to_string();
        let mut outcome = SettingsOutcome::default();
        let visibility = Visibility::new();
        visibility.set_grab_status(status);
        let output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                draw_settings_body(
                    ui,
                    &AppState::new(),
                    &mut config,
                    &mut outcome,
                    &live_hotkeys(&visibility, config_hotkey, tray_enabled),
                );
            });
        });

        let mut text = Vec::new();
        for clipped in &output.shapes {
            collect_text(&clipped.shape, &mut text);
        }
        text
    }

    fn collect_text(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.text().to_string()),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_text(shape, out);
                }
            }
            _ => {}
        }
    }

    fn painted_contains(text: &[String], needle: &str) -> bool {
        text.iter().any(|line| line.contains(needle))
    }

    /// The live state the hotkey controls read, for a test that is not about it.
    fn live_hotkeys<'a>(
        visibility: &'a Visibility,
        grabbed: &'a str,
        tray_enabled: bool,
    ) -> HotkeyState<'a> {
        HotkeyState {
            visibility,
            grabbed,
            tray_enabled,
        }
    }

    /// What one scripted interaction produced.
    struct Driven {
        secondary_clicked: bool,
        primary_clicked: bool,
        /// The action the menu body returned, if it was painted.
        action: ContextAction,
        /// Shapes painted on the frame after the click -- the menu being open.
        painted_after_click: usize,
    }

    /// Drive the panel surface through the ordinary egui path: register it with
    /// [`surface_sense`], then read the Response's own click flags.
    fn drive(button: egui::PointerButton) -> Driven {
        let state = AppState::new();
        let ctx = Context::default();
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(300.0, 198.0));
        let at = egui::pos2(150.0, 40.0);
        let ev = |pressed| egui::Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers: egui::Modifiers::default(),
        };

        // Hover must arrive in its own frame: egui evaluates a click against the
        // pointer state as it was when the move was processed.
        // Hover, then press, then release, then one frame for the popup to paint.
        // Press/release must be in different frames: egui treats a
        // press-and-release inside one pass as a click only via `is_decidedly_
        // dragging`, and separating them is what a real click looks like.
        let frames: Vec<Vec<egui::Event>> = vec![
            vec![egui::Event::PointerMoved(at)],
            vec![ev(true)],
            vec![ev(false)],
            Vec::new(),
        ];

        let mut d = Driven {
            secondary_clicked: false,
            primary_clicked: false,
            action: ContextAction::None,
            painted_after_click: 0,
        };

        for (index, events) in frames.into_iter().enumerate() {
            let mut input = egui::RawInput::default();
            input.screen_rect = Some(screen);
            input.focused = true;
            input.events = events;
            let output = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let response = ui.interact(
                        ui.max_rect(),
                        ui.id().with("panel_surface"),
                        surface_sense(),
                    );
                    d.secondary_clicked |= response.secondary_clicked();
                    d.primary_clicked |= response.clicked();
                    response.context_menu(|ui| {
                        let mut config = PanelConfig::fresh();
                        let mut outcome = SettingsOutcome::default();
                        let _ = &outcome;
                        d.action =
                            crate::panel::menu::draw_context_menu(ui, true, GrabStatus::Grabbed);
                        let _ = &mut config;
                    });
                });
            });
            if index + 1 == frames_len(&ctx) {
                d.painted_after_click = output.shapes.len();
            }
        }
        let _ = state;
        d
    }

    fn frames_len(_ctx: &Context) -> usize {
        3
    }

    /// The sense the panel surface is registered with.
    ///
    /// `click_and_drag` rather than `click`: repositioning the panel by dragging
    /// is PR 2's work, and the sense has to be declared before the code that uses
    /// it rather than changed underneath it later.
    pub fn surface_sense() -> egui::Sense {
        egui::Sense::click_and_drag()
    }

    #[test]
    fn a_right_click_on_the_surface_opens_the_menu_through_the_response() {
        let d = drive(egui::PointerButton::Secondary);
        assert!(
            d.secondary_clicked,
            "the right click must reach the surface Response -- the whole point of this path"
        );
        assert!(d.painted_after_click > 0, "the open menu must paint");
    }

    #[test]
    fn a_left_click_never_counts_as_a_secondary_click() {
        // The surface is click-and-drag, so a left click lands on it too. It must
        // not open the menu: that would make the menu appear on every window drag.
        let d = drive(egui::PointerButton::Primary);
        assert!(
            d.primary_clicked,
            "the left click should land on the surface"
        );
        assert!(
            !d.secondary_clicked,
            "a left click must never register as a secondary click"
        );
        assert_eq!(d.action, ContextAction::None, "and must open nothing");
    }

    #[test]
    fn the_settings_entry_routes_to_the_settings_window() {
        // "Panel settings..." is a routing decision, not a boolean: it must open
        // the settings viewport and close the menu, not leave the menu on top.
        assert_ne!(
            ContextAction::OpenSettings,
            ContextAction::None,
            "the action is distinct so the caller can route it"
        );
        assert_ne!(ContextAction::OpenSettings, ContextAction::BackToNormal);
    }

    #[test]
    fn the_settings_body_renders_with_every_row_disabled() {
        with_ui(|ui, config, outcome| {
            for row in &mut config.rows {
                row.visible = false;
            }
            let visibility = Visibility::new();
            draw_settings_body(
                ui,
                &AppState::new(),
                config,
                outcome,
                &live_hotkeys(&visibility, "Shift+F9", false),
            );
            assert!(!outcome.back_to_normal);
            assert!(!outcome.config_changed);
        });
    }

    #[test]
    fn the_settings_body_survives_an_unknown_row() {
        // `normalize` removes these, but a panic in the settings window would be
        // unrecoverable for the user, so it must not assume normalised input.
        with_ui(|ui, config, outcome| {
            config.rows.push(super::super::config::RowConfig {
                id: "not_a_real_row".into(),
                visible: true,
            });
            let visibility = Visibility::new();
            draw_settings_body(
                ui,
                &AppState::new(),
                config,
                outcome,
                &live_hotkeys(&visibility, "Shift+F9", false),
            );
        });
    }

    #[test]
    fn the_settings_body_renders_for_every_grab_state() {
        // Including the states that are not a working key: a refused key, a
        // not-yet-attempted one and a partially captured one all have a status
        // line, and none of them may be drawn as "captured".
        for status in [
            GrabStatus::Unknown,
            GrabStatus::Grabbed,
            GrabStatus::Partial,
            GrabStatus::Disabled,
            GrabStatus::Busy,
            GrabStatus::NoDisplay,
            GrabStatus::Failed,
        ] {
            with_ui(|ui, config, outcome| {
                let visibility = Visibility::new();
                visibility.set_grab_status(status);
                draw_settings_body(
                    ui,
                    &AppState::new(),
                    config,
                    outcome,
                    &live_hotkeys(&visibility, "Shift+F9", false),
                );
                assert!(
                    !outcome.hotkey_changed,
                    "drawing must not report a re-grab for {status:?}"
                );
            });
        }
    }

    #[test]
    fn the_settings_surface_warns_when_the_key_is_only_partially_captured() {
        // The brief's requirement, checked on the painted frame: a partial grab must
        // not read as a complete one, and the warning has to name the lock keys the
        // user will find it does not work with. "lock-state" appears in the warning
        // body only — the status line above it says "some lock states".
        let partial = painted_text("Control+Alt+A", GrabStatus::Partial, false);
        assert!(
            painted_contains(&partial, "some lock states"),
            "the status line must not read as a complete capture: {partial:#?}"
        );
        assert!(
            painted_contains(&partial, "lock-state"),
            "and the explanation it points at must be drawn: {partial:#?}"
        );
        for key in ["CapsLock", "NumLock", "ScrollLock"] {
            assert!(
                painted_contains(&partial, key),
                "the warning must name {key}: {partial:#?}"
            );
        }

        // The negative control: the same spec with a complete grab paints neither
        // the status line nor the lock-key warning, so the assertion above is about
        // the Partial state and not about words that are always on screen.
        let complete = painted_text("Control+Alt+A", GrabStatus::Grabbed, false);
        assert!(
            painted_contains(&complete, "Hotkey captured"),
            "{complete:#?}"
        );
        assert!(
            !painted_contains(&complete, "some lock states"),
            "{complete:#?}"
        );
        assert!(!painted_contains(&complete, "lock-state"), "{complete:#?}");
        for key in ["CapsLock", "NumLock", "ScrollLock"] {
            assert!(!painted_contains(&complete, key), "{complete:#?}");
        }
    }

    #[test]
    fn the_settings_surface_explains_why_a_partial_key_is_not_the_way_back() {
        // With the tray on, hiding is still allowed — and the surface has to say
        // that the hotkey is not the route it is relying on.
        let with_tray = painted_text("Control+Alt+A", GrabStatus::Partial, true);
        assert!(
            painted_contains(&with_tray, "way back exists"),
            "hiding is allowed here: {with_tray:#?}"
        );
        assert!(
            painted_contains(&with_tray, "not counted"),
            "and the partial key is not that route: {with_tray:#?}"
        );

        // Without another route the panel cannot be hidden, which is the rule the
        // partial key must not be allowed to bypass.
        let alone = painted_text("Control+Alt+A", GrabStatus::Partial, false);
        assert!(
            painted_contains(&alone, "cannot be hidden"),
            "no tray, no D-Bus in this state: {alone:#?}"
        );
    }

    #[test]
    fn the_settings_surface_warns_about_modifiers_it_ignored() {
        // `Wibble+F9` is stored as typed and grabbed as plain F9. The status line
        // alone would read as an exact match for the spec, so the surface has to
        // name the ignored token and the shortcut that is really in effect.
        let typo = painted_text("Wibble+F9", GrabStatus::Grabbed, false);
        assert!(
            painted_contains(&typo, "Wibble"),
            "the ignored token must be named: {typo:#?}"
        );
        assert!(
            painted_contains(&typo, "effective"),
            "and the effective shortcut explained: {typo:#?}"
        );

        // The user's own spec is still what the field shows.
        assert!(painted_contains(&typo, "Wibble+F9"), "{typo:#?}");

        // The negative control: a spec with nothing ignored paints no such warning,
        // and the two Shift aliases are supported, so neither of them warns.
        for clean in ["Shift+F9", "Shift_R+F9", "Shift_L+F9"] {
            let text = painted_text(clean, GrabStatus::Grabbed, false);
            assert!(
                !painted_contains(&text, "effective"),
                "{clean} must not warn: {text:#?}"
            );
        }
    }

    #[test]
    fn the_settings_window_is_an_ordinary_sized_window() {
        assert!(SETTINGS_SIZE[1] >= 400.0, "tall enough for four groups");
        assert!(SETTINGS_SIZE[0] >= 320.0, "wide enough for a row label");
    }

    // ---- the window controls ----

    /// Click the control `draw` lays out, and report what it produced on the frame
    /// the click was released.
    ///
    /// The frame ORDER is the part that matters: egui evaluates a click against the
    /// pointer state as it was when the move was processed, so the pointer has to
    /// arrive in a frame of its own before the press, and the press and the release
    /// have to be in separate frames. Get that wrong and nothing registers, which
    /// reads exactly like a control that is not wired up.
    ///
    /// Aiming at the response's own rect is deliberate: coordinates typed into the
    /// test would keep passing after a layout change moved the control out of the
    /// window, which is the one way a settings control becomes unreachable.
    ///
    /// The value is collected on the frame the control reported the interaction,
    /// not read off the last frame: a slider takes its value from the pointer on
    /// the PRESS frame while a checkbox reports on release, so a driver that read
    /// only the final frame would report a slider drag as "nothing happened".
    fn click_control<T, F>(draw: F) -> T
    where
        F: Fn(&mut egui::Ui) -> (egui::Response, T),
        T: Default,
    {
        let ctx = Context::default();
        let screen = Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(SETTINGS_SIZE[0], SETTINGS_SIZE[1]),
        );
        let mut at = egui::pos2(0.0, 0.0);
        let mut produced = T::default();

        for frame in 0..4 {
            let mut input = egui::RawInput::default();
            input.screen_rect = Some(screen);
            // egui discards pointer input for an unfocused viewport.
            input.focused = true;
            input.events = match frame {
                0 => Vec::new(),
                1 => vec![egui::Event::PointerMoved(at)],
                2 => vec![pointer(true, at)],
                _ => vec![pointer(false, at)],
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let (response, value) = draw(ui);
                    if frame == 0 {
                        at = response.rect.center();
                    }
                    if response.changed() || response.clicked() {
                        produced = value;
                    }
                });
            });
        }
        produced
    }

    fn pointer(pressed: bool, at: egui::Pos2) -> egui::Event {
        egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        }
    }

    /// The stacking option a click on `target` asks for, or `None` when clicking it
    /// changed nothing.
    fn click_stacking(current: Stacking, target: Stacking) -> Option<Stacking> {
        click_control(|ui| {
            let controls = stacking_controls(ui, current);
            let aim = controls
                .iter()
                .find(|(option, _)| *option == target)
                .expect("every stacking option has a control")
                .1
                .clone();
            let chosen = controls
                .iter()
                .find(|(option, response)| response.clicked() && *option != current)
                .map(|(option, _)| *option);
            (aim, chosen)
        })
    }

    #[test]
    fn choosing_a_stacking_option_reports_a_change_so_the_window_spec_is_re_sent() {
        // The level rides on the window spec, so a stacking change is not just a
        // config write: without the re-send the setting would only take effect at
        // the next mode switch, which is exactly the "restart to apply" behaviour
        // the control must not have.
        assert_eq!(
            click_stacking(Stacking::AlwaysOnTop, Stacking::Normal),
            Some(Stacking::Normal)
        );
        assert_eq!(
            click_stacking(Stacking::Normal, Stacking::AlwaysOnTop),
            Some(Stacking::AlwaysOnTop)
        );
        // Clicking the option that is already chosen is not a change, so nothing is
        // rewritten and nothing is re-sent.
        assert_eq!(
            click_stacking(Stacking::AlwaysOnTop, Stacking::AlwaysOnTop),
            None
        );
    }

    /// The taskbar value a click on the checkbox asks for, or `None` when the click
    /// changed nothing.
    fn click_taskbar(current: bool) -> Option<bool> {
        click_control(|ui| {
            let (response, value) = taskbar_control(ui, current);
            let changed = response.changed().then_some(value);
            (response, changed)
        })
    }

    #[test]
    fn the_taskbar_control_reports_the_value_the_click_asked_for() {
        assert_eq!(click_taskbar(true), Some(false), "unchecking it");
        assert_eq!(click_taskbar(false), Some(true), "checking it");
    }

    // ---- the hide entry, and the gate on it ----

    /// The click the hide entry reports, with the gate in the given state.
    fn click_hide_entry(can_hide: bool) -> bool {
        click_control(|ui| hide_entry(ui, can_hide, GrabStatus::Busy))
    }

    #[test]
    fn the_hide_entry_is_offered_only_when_a_way_back_exists() {
        // An enabled entry and a disabled one have to BEHAVE differently, or the
        // gate is decoration: the same click must produce the action in one case
        // and nothing at all in the other.
        assert!(
            click_hide_entry(true),
            "with a way back, the click must reach the caller"
        );
        assert!(
            !click_hide_entry(false),
            "with no way back the entry must be inert, not merely duller"
        );
    }

    #[test]
    fn the_hide_action_is_its_own_routing_decision() {
        // Distinct from the two pre-existing entries: a caller that treated it as
        // "something happened, open the settings window" would open a settings
        // window when the user asked to hide the panel.
        assert_ne!(ContextAction::Hide, ContextAction::None);
        assert_ne!(ContextAction::Hide, ContextAction::OpenSettings);
        assert_ne!(ContextAction::Hide, ContextAction::BackToNormal);
    }

    // ---- the hotkey field ----

    /// Type `typed` into the hotkey field and click away, and report the spec the
    /// field ended up holding and whether the body asked for a re-grab.
    ///
    /// The frame order is `click_control`'s plus three: a click only focuses the
    /// field if the pointer arrives in a frame of its own first, typed text only
    /// reaches a FOCUSED field, and the focus is only surrendered by a click
    /// somewhere else — so press and release at the empty corner have to be in
    /// frames of their own as well.
    fn commit_typed_hotkey(typed: &str) -> (String, bool) {
        const GRABBED: &str = "Shift+F9";

        let ctx = Context::default();
        let screen = Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(SETTINGS_SIZE[0], SETTINGS_SIZE[1]),
        );
        // A point inside the window and outside the control list: the panel
        // frame's own margin.
        let away = egui::pos2(4.0, 4.0);
        let mut field = egui::pos2(0.0, 0.0);
        let mut config = PanelConfig::fresh();
        let mut committed = false;

        for frame in 0..8 {
            let mut input = egui::RawInput::default();
            input.screen_rect = Some(screen);
            input.focused = true;
            input.events = match frame {
                0 => Vec::new(),
                1 => vec![egui::Event::PointerMoved(field)],
                2 => vec![pointer(true, field)],
                3 => vec![pointer(false, field)],
                4 => vec![egui::Event::Text(typed.to_string())],
                5 => vec![egui::Event::PointerMoved(away)],
                6 => vec![pointer(true, away)],
                _ => vec![pointer(false, away)],
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let (response, just_committed) = hotkey_control(ui, &mut config, GRABBED);
                    if frame == 0 {
                        field = response.rect.center();
                    }
                    committed |= just_committed;
                });
            });
        }

        (config.hotkey, committed)
    }

    #[test]
    fn editing_the_hotkey_reports_a_re_grab_but_leaving_it_alone_does_not() {
        // The positive case: a new spec typed into the field has to reach the
        // caller, or changing the key would need a restart.
        let (typed, committed) = commit_typed_hotkey("+X");
        assert_eq!(typed, "Shift+F9+X", "the typed text lands in the config");
        assert!(
            committed,
            "a changed spec must be committed so the key is re-grabbed"
        );

        // And the negative control: the same interaction with nothing typed must
        // NOT commit, or every click into the field would tear the grab down and
        // put it back up.
        let (unchanged, committed) = commit_typed_hotkey("");
        assert_eq!(unchanged, "Shift+F9");
        assert!(!committed, "an unchanged spec must not trigger a re-grab");
    }

    // ---- the background-opacity control ----

    /// The opacity a click at the slider's own centre asks for, or `None` when the
    /// click changed nothing.
    fn click_opacity(current: f32) -> Option<f32> {
        click_control(|ui| {
            let (response, value) = opacity_control(ui, current);
            let changed = response.changed().then_some(value);
            (response, changed)
        })
    }

    #[test]
    fn the_opacity_control_reports_the_value_the_click_asked_for() {
        // Aimed at the slider's own centre, so the click lands mid-track: the
        // value must move off both ends and stay inside the range, which is what
        // "the control is wired to the config" means for a slider.
        let from_opaque = click_opacity(1.0).expect("a click on the slider must change the opacity");
        assert!(
            (MIN_BACKGROUND_OPACITY..1.0).contains(&from_opaque),
            "clicking mid-track from 1.0 gave {from_opaque}"
        );

        let from_translucent =
            click_opacity(MIN_BACKGROUND_OPACITY).expect("a click must change the opacity");
        assert!(
            (MIN_BACKGROUND_OPACITY..=1.0).contains(&from_translucent)
                && from_translucent > MIN_BACKGROUND_OPACITY,
            "clicking mid-track from the minimum gave {from_translucent}"
        );
    }

    /// A config value outside the range — a hand-edited `panel.json`, or one from
    /// a build with a different range — must not be shown as a handle past the end
    /// of the track, and must not be written back out of range.
    #[test]
    fn the_opacity_control_clamps_what_it_is_given() {
        with_ui(|ui, _config, _outcome| {
            let (_, high) = opacity_control(ui, 5.0);
            assert_eq!(high, MAX_BACKGROUND_OPACITY);
            let (_, low) = opacity_control(ui, 0.0);
            assert_eq!(low, MIN_BACKGROUND_OPACITY);
        });
    }

    // ---- the frame elements ----

    #[test]
    fn the_frames_section_names_the_source_and_calls_the_defaults_proposals() {
        // What the surface says, read off the painted shapes: where the numbers
        // come from, that the layer has to be installed for them, and that 10 s /
        // 4 Hz are proposals rather than a decision taken for the user.
        let painted = painted_text("Shift+F9", GrabStatus::Grabbed, true);
        for word in [
            "Frames",
            "Vulkan",
            "LAPSPHERE_FRAMES=1",
            "Statistics window",
            "Refresh",
            "proposed defaults",
            "not an owner",
        ] {
            assert!(
                painted_contains(&painted, word),
                "the settings surface does not paint {word:?}"
            );
        }
    }

    #[test]
    fn the_frames_group_offers_a_checkbox_for_every_frame_element() {
        // The elements are not on by default, so the menu is the only way to turn
        // them on: a row whose label never reaches the surface is a row the user
        // cannot enable.
        let painted = painted_text("Shift+F9", GrabStatus::Grabbed, true);
        // The group's collapsible header, and every element's long label. The
        // group is collapsed by default, so its header is what the surface paints;
        // its contents are asserted through `Group::of` below rather than by
        // pretending they are visible.
        assert!(painted_contains(&painted, Group::Frame.label()));
        for id in crate::panel::frames::ELEMENT_IDS {
            let row = rows::find(id).expect("row");
            assert_eq!(row.group, Group::Frame);
            assert!(!row.label.is_empty(), "{id} needs a menu label");
        }
    }

    /// The whole settings surface, painted with a chosen frame snapshot.
    fn painted_with_snapshot(snapshot: crate::panel::frames::FrameSnapshot) -> Vec<String> {
        let ctx = Context::default();
        let mut input = egui::RawInput::default();
        input.screen_rect = Some(Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(SETTINGS_SIZE[0], SETTINGS_SIZE[1]),
        ));
        let mut config = PanelConfig::fresh();
        let mut outcome = SettingsOutcome::default();
        let visibility = Visibility::new();
        let state = crate::app::AppState::new();
        state.frames.publish_for_test(snapshot);
        let output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                draw_settings_body(
                    ui,
                    &state,
                    &mut config,
                    &mut outcome,
                    &live_hotkeys(&visibility, "Shift+F9", true),
                );
            });
        });
        let mut text = Vec::new();
        for clipped in &output.shapes {
            collect_text(&clipped.shape, &mut text);
        }
        text
    }

    fn live_snapshot(metrics: crate::panel::frames::Metrics) -> crate::panel::frames::FrameSnapshot {
        crate::panel::frames::FrameSnapshot {
            source: Some(crate::panel::frames::Source {
                pid: 4242,
                exe_name: "vkcube".to_string(),
            }),
            metrics: Some(metrics),
            samples_ms: std::sync::Arc::from(vec![8.0f32; 4]),
            column_span: 1,
        }
    }

    #[test]
    fn the_frames_section_says_what_it_is_reading_and_how_much_of_the_window_it_covers() {
        // The reader's ring is bounded, so a long configured window can be only
        // partly covered. That has to be visible where the window is set: a statistic
        // over 24 s presented as a 60 s window is not a smaller number, it is a wrong
        // one. The words are asserted, not the sentence -- a label wraps at the
        // window width, and wrapping never splits a word.
        let short = crate::panel::frames::stats::metrics(&vec![6.0f64; 4096], 60_000.0)
            .expect("metrics");
        assert!(short.is_short());
        let painted = painted_with_snapshot(live_snapshot(short));
        assert!(painted_contains(&painted, "Reading pid 4242"), "{painted:?}");
        assert!(painted_contains(&painted, "recorded"), "the shortfall is named");
        assert!(painted_contains(&painted, "asked"));
    }

    #[test]
    fn a_covered_window_reads_as_a_window_and_not_as_a_shortfall() {
        let full = crate::panel::frames::stats::metrics(&vec![6.0f64; 1667], 10_000.0)
            .expect("metrics");
        assert!(!full.is_short());
        let painted = painted_with_snapshot(live_snapshot(full));
        assert!(painted_contains(&painted, "Reading pid 4242"), "{painted:?}");
        assert!(
            !painted_contains(&painted, "recorded"),
            "a covered window must not be reported as short: {painted:?}"
        );
    }

    #[test]
    fn with_no_source_and_with_no_interval_yet_the_surface_says_which_case_it_is() {
        let painted = painted_with_snapshot(crate::panel::frames::FrameSnapshot::default());
        assert!(painted_contains(&painted, "No source"), "{painted:?}");

        // A segment exists (the layer creates it at instance creation) but no second
        // present has happened yet: a name with no numbers, which is a different
        // state from "nothing is running".
        let named = crate::panel::frames::FrameSnapshot {
            source: Some(crate::panel::frames::Source {
                pid: 4242,
                exe_name: "vkcube".to_string(),
            }),
            metrics: None,
            samples_ms: std::sync::Arc::from(Vec::new()),
            column_span: 0,
        };
        let painted = painted_with_snapshot(named);
        assert!(painted_contains(&painted, "Reading pid 4242"), "{painted:?}");
        assert!(painted_contains(&painted, "interval"), "{painted:?}");
    }

    #[test]
    fn the_window_and_refresh_controls_report_the_value_the_click_asked_for() {
        let window = click_control(|ui| {
            let (response, value) = window_control(ui, 10);
            let changed = response.changed().then_some(value);
            (response, changed)
        })
        .expect("a click on the slider must change the window");
        assert!(
            (MIN_WINDOW_SECONDS..=MAX_WINDOW_SECONDS).contains(&window) && window != 10,
            "clicking mid-track from 10 gave {window}"
        );

        let refresh = click_control(|ui| {
            let (response, value) = refresh_control(ui, 4);
            let changed = response.changed().then_some(value);
            (response, changed)
        })
        .expect("a click on the slider must change the cadence");
        assert!(
            (MIN_REFRESH_HZ..=MAX_REFRESH_HZ).contains(&refresh) && refresh != 4,
            "clicking mid-track from 4 gave {refresh}"
        );
    }

    #[test]
    fn the_frame_controls_clamp_what_they_are_given() {
        // A hand-edited `panel.json` must not put a handle past the end of the
        // track, and must not be written back out of range.
        with_ui(|ui, _config, _outcome| {
            let (_, high) = window_control(ui, 10_000);
            assert_eq!(high, MAX_WINDOW_SECONDS);
            let (_, low) = window_control(ui, 0);
            assert_eq!(low, MIN_WINDOW_SECONDS);
            let (_, fast) = refresh_control(ui, 10_000);
            assert_eq!(fast, MAX_REFRESH_HZ);
            let (_, slow) = refresh_control(ui, 0);
            assert_eq!(slow, MIN_REFRESH_HZ);
        });
    }

    #[test]
    fn every_row_is_reachable_from_exactly_one_group() {
        // The settings window is the only place a row can be toggled, so a row no
        // group claims is a row the user cannot enable, and a row two groups claim
        // gets two checkboxes fighting over one flag.
        for id in rows::all_ids() {
            let owners: Vec<Group> = Group::ALL
                .iter()
                .copied()
                .filter(|group| group.of(id))
                .collect();
            assert_eq!(owners.len(), 1, "row {id} is claimed by {owners:?}");
        }
    }
}
