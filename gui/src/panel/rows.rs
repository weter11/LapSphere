//! The panel's row registry: which rows exist, which group each belongs to, and
//! how each one's value is read out of `AppState`.
//!
//! # Relationship to the Statistics tab
//!
//! Every row here corresponds to a row the Statistics tab shows, and the value
//! is formatted by `crate::stat_format` — the same functions the tab calls. So
//! `Processor`, `Average Frequency`, `Average Load` and `Package Temperature`
//! print byte-identically in both places. What is shared is the FORMATTING, not
//! the layout: the tab renders a two-column `Grid` under collapsing headers,
//! the panel renders one `Label: value` line per row.
//!
//! The six frame rows ([`Group::Frame`]) are the exception, and deliberately so:
//! the tab has no frame data at all — its numbers come from the daemon, and a
//! game's present time is not something the daemon can see. Those rows read the
//! GUI's own shared-memory reader ([`crate::panel::frames`]) and are listed in
//! `NO_TAB_LABEL` below with that reason.
//!
//! # Absent data
//!
//! An `Option` field the hardware did not report becomes [`ABSENT`] — an em
//! dash — rather than a hidden row. That is what keeps the panel's height a pure
//! function of the row SET: a gamepad that disconnects, a GPU that stops reporting
//! a clock, or a game that exits must not make the window shorter or otherwise move
//! the user's other rows.
//!
//! # Scope
//!
//! One row per hardware quantity. Multi-device rows (per-core frequency, every
//! mount) are not offered: rendering all sixteen cores in a 300 px column is not
//! what this surface is for. For GPU and gamepad there IS a selection rule, and
//! it is [`primary_gpu`] / [`primary_gamepad`] below rather than a bare
//! `.first()`.

use crate::stat_format::{self, ABSENT};
use lapsphere_common::types::{GamepadStatus, GpuType};

use super::frames;
use super::PanelView;

/// Which menu section a row's checkbox lives under.
///
/// Groups are for the MENU only — the panel itself renders one flat column. The
/// owner's decision was per-row selection grouped for browsing, not per-group
/// toggles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Cpu,
    Gpu,
    Ram,
    Gamepad,
    /// The six frame elements — the owner's first-release decision. Their data
    /// does not come from the daemon; see the module docs.
    Frame,
}

impl Group {
    /// Every group, in menu order. The frame group is last because it describes a
    /// source that is present only while a game is running, unlike the hardware
    /// groups above it.
    pub const ALL: [Group; 5] = [
        Group::Cpu,
        Group::Gpu,
        Group::Ram,
        Group::Gamepad,
        Group::Frame,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Group::Cpu => "CPU",
            Group::Gpu => "GPU",
            Group::Ram => "RAM",
            Group::Gamepad => "Gamepad",
            Group::Frame => "Frames",
        }
    }

    pub fn of(self, id: &str) -> bool {
        find(id).map(|row| row.group == self).unwrap_or(false)
    }
}

/// A row's value, resolved for this frame.
///
/// Kept as a plain string plus an optional colour rather than a typed variant:
/// the panel paints text, and the Statistics tab's own widgets (progress bars,
/// coloured labels) are not being reimplemented here.
#[derive(Debug, Clone, PartialEq)]
pub struct Value {
    pub text: String,
    /// `None` for a plain string; `Some` for a tinted one (temperature, load).
    pub tint: Option<egui::Color32>,
}

impl Value {
    fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tint: None,
        }
    }

    fn tinted(text: impl Into<String>, tint: egui::Color32) -> Self {
        Self {
            text: text.into(),
            tint: Some(tint),
        }
    }

    /// The placeholder for a value hardware did not report.
    fn absent() -> Self {
        Self::plain(ABSENT)
    }
}

/// How a row produces what it shows.
///
/// Two shapes, and the difference is geometry rather than taste: a text row is one
/// line in a slot one row high, and a strip row is the frame-time graph in a slot
/// [`super::layout::GRAPH_STRIP_ROWS`] rows high. Making this a variant rather than
/// a flag means the renderer cannot ask a strip row for text it does not have.
pub enum RowKind {
    /// One `Label: value` line. Never fails: absent data becomes `—`.
    ///
    /// The reader takes a [`PanelView`] rather than an `AppState` so that a frame
    /// row has exactly one source for its value: the snapshot the frame was built
    /// with (see `PanelView`). A hardware row reads `view.state`; a frame row reads
    /// `view.frames`.
    Text(fn(&PanelView<'_>) -> Value),
    /// The frame-time strip: the recorded intervals, drawn in a reserved slot.
    /// Nothing is written into the value column.
    Graph,
}

/// One row of the panel.
pub struct Row {
    /// Stable id, and the key in `panel.json`.
    pub id: &'static str,
    /// The name shown before the colon, identical to the Statistics tab's.
    ///
    /// Used by the menu's checkbox and by the drift guard below.
    pub label: &'static str,
    /// The short name the panel prints before the colon.
    ///
    /// The tab has ~570 px of grid to spend on a label; the panel has a 300 px
    /// column shared with the value. Printing `Average Frequency` there means
    /// eliding it, and an elided LABEL is the worst outcome: the row stops being
    /// identifiable at a glance. So the panel prints a short name and the tab
    /// keeps its long one. The VALUE is what gets elided instead — an ellipsis
    /// inside a processor name is harmless, an ellipsis inside the word
    /// "Frequency" is not.
    ///
    /// Every label here must fit [`super::layout::LABEL_WIDTH`];
    /// `super::render`'s `every_label_fits_the_label_column` measures them all
    /// against real font metrics.
    pub panel_label: &'static str,
    pub group: Group,
    pub kind: RowKind,
}

impl Row {
    /// The text this row shows where a text is shown, or `None` for a strip row.
    ///
    /// The menu wants a live value next to each checkbox; a strip has none, so the
    /// menu prints the strip's scale instead.
    pub fn text(&self, view: &PanelView<'_>) -> Option<Value> {
        match self.kind {
            RowKind::Text(read) => Some(read(view)),
            RowKind::Graph => None,
        }
    }

    /// What the settings menu prints beside this row's checkbox.
    ///
    /// The menu wants a live value next to every checkbox, and a strip has no text
    /// value: it prints the scale the strip draws against instead of an empty cell,
    /// including how many intervals a column stands for when the window is wider than
    /// the strip.
    pub fn menu_value(&self, view: &PanelView<'_>) -> String {
        match self.text(view) {
            Some(value) => value.text,
            None => frames::graph::scale_caption(view.frames.column_span),
        }
    }

    /// Is this row the frame-time strip?
    pub fn is_graph(&self) -> bool {
        matches!(self.kind, RowKind::Graph)
    }
}

/// Every row, in display order: CPU, GPU, RAM, gamepad, then the frame elements.
pub static ROWS: &[Row] = &[
    // ---- CPU: the rows draw_cpu_info shows ----
    Row {
        id: "cpu_processor",
        label: "Processor",
        panel_label: "CPU",
        group: Group::Cpu,
        kind: RowKind::Text(|view| match &view.state.cpu_info {
            Some(cpu) => Value::plain(&cpu.name),
            None => Value::absent(),
        }),
    },
    Row {
        id: "cpu_average_frequency",
        label: "Average Frequency",
        panel_label: "Freq",
        group: Group::Cpu,
        kind: RowKind::Text(|view| match &view.state.cpu_info {
            // The tab prints this field kHz-scaled; stat_format keeps that.
            Some(cpu) => Value::plain(stat_format::average_frequency_mhz(cpu.average_frequency)),
            None => Value::absent(),
        }),
    },
    Row {
        id: "cpu_average_load",
        label: "Average Load",
        panel_label: "Load",
        group: Group::Cpu,
        kind: RowKind::Text(|view| match &view.state.cpu_info {
            Some(cpu) => Value::tinted(
                stat_format::percent1(cpu.average_load),
                stat_format::load_tint(cpu.average_load),
            ),
            None => Value::absent(),
        }),
    },
    Row {
        id: "cpu_package_temperature",
        label: "Package Temperature",
        panel_label: "Temp",
        group: Group::Cpu,
        kind: RowKind::Text(|view| match &view.state.cpu_info {
            Some(cpu) => Value::tinted(
                stat_format::celsius1(cpu.package_temp),
                stat_format::temp_tint(cpu.package_temp),
            ),
            None => Value::absent(),
        }),
    },
    Row {
        id: "cpu_governor",
        label: "Governor",
        panel_label: "Gov",
        group: Group::Cpu,
        kind: RowKind::Text(|view| match &view.state.cpu_info {
            Some(cpu) => Value::plain(&cpu.governor),
            None => Value::absent(),
        }),
    },
    Row {
        id: "cpu_scaling_driver",
        label: "Scaling Driver",
        panel_label: "Driver",
        group: Group::Cpu,
        kind: RowKind::Text(|view| match &view.state.cpu_info {
            Some(cpu) => Value::plain(&cpu.scaling_driver),
            None => Value::absent(),
        }),
    },
    // ---- GPU: the first entry, which is what a single-column panel can show ----
    Row {
        id: "gpu_name",
        label: "GPU",
        panel_label: "GPU",
        group: Group::Gpu,
        kind: RowKind::Text(|view| match primary_gpu(&view.state.gpu_info) {
            Some(gpu) => Value::plain(&gpu.name),
            None => Value::absent(),
        }),
    },
    Row {
        id: "gpu_type",
        label: "Type",
        panel_label: "Type",
        group: Group::Gpu,
        kind: RowKind::Text(|view| match primary_gpu(&view.state.gpu_info) {
            Some(gpu) => Value::plain(match gpu.gpu_type {
                GpuType::Integrated => "Integrated",
                GpuType::Discrete => "Discrete",
            }),
            None => Value::absent(),
        }),
    },
    Row {
        id: "gpu_core_frequency",
        label: "Core Frequency",
        panel_label: "Clock",
        group: Group::Gpu,
        kind: RowKind::Text(
            |view| match primary_gpu(&view.state.gpu_info).and_then(|g| g.frequency) {
                Some(mhz) => Value::plain(stat_format::mhz(mhz)),
                None => Value::absent(),
            },
        ),
    },
    Row {
        id: "gpu_temperature",
        label: "Temperature",
        panel_label: "Temp",
        group: Group::Gpu,
        kind: RowKind::Text(|view| {
            match primary_gpu(&view.state.gpu_info).and_then(|g| g.temperature) {
                Some(temp) => {
                    Value::tinted(stat_format::celsius1(temp), stat_format::temp_tint(temp))
                }
                None => Value::absent(),
            }
        }),
    },
    Row {
        id: "gpu_load",
        label: "Load",
        panel_label: "Load",
        group: Group::Gpu,
        kind: RowKind::Text(
            |view| match primary_gpu(&view.state.gpu_info).and_then(|g| g.load) {
                Some(load) => {
                    Value::tinted(stat_format::percent1(load), stat_format::load_tint(load))
                }
                None => Value::absent(),
            },
        ),
    },
    Row {
        id: "gpu_power",
        label: "Power",
        panel_label: "Power",
        group: Group::Gpu,
        kind: RowKind::Text(
            |view| match primary_gpu(&view.state.gpu_info).and_then(|g| g.power) {
                Some(watts) => {
                    Value::tinted(stat_format::watts1(watts), stat_format::power_tint(watts))
                }
                None => Value::absent(),
            },
        ),
    },
    Row {
        id: "gpu_memory_frequency",
        label: "Memory Frequency",
        panel_label: "MemClk",
        group: Group::Gpu,
        kind: RowKind::Text(|view| {
            match primary_gpu(&view.state.gpu_info).and_then(|g| g.memory_frequency) {
                Some(mhz) => Value::plain(stat_format::mhz(mhz)),
                None => Value::absent(),
            }
        }),
    },
    Row {
        id: "gpu_memory_usage",
        label: "VRAM available / total",
        panel_label: "VRAM",
        group: Group::Gpu,
        kind: RowKind::Text(|view| {
            match primary_gpu(&view.state.gpu_info).and_then(|g| g.vram_memory.as_ref()) {
                Some(memory) => {
                    Value::plain(format!("{} / {} MiB", memory.free_mib, memory.total_mib))
                }
                None => Value::absent(),
            }
        }),
    },
    // ---- RAM ----
    Row {
        id: "ram_usage",
        label: "Usage",
        panel_label: "Usage",
        group: Group::Ram,
        kind: RowKind::Text(|view| match &view.state.memory_info {
            Some(mem) => Value::tinted(
                stat_format::percent1(mem.used_percent),
                stat_format::load_tint(mem.used_percent),
            ),
            None => Value::absent(),
        }),
    },
    Row {
        id: "ram_used",
        label: "Used",
        panel_label: "Used",
        group: Group::Ram,
        kind: RowKind::Text(|view| match &view.state.memory_info {
            Some(mem) => Value::plain(stat_format::gib2(mem.used_gib)),
            None => Value::absent(),
        }),
    },
    Row {
        id: "ram_available",
        label: "Available",
        panel_label: "Free",
        group: Group::Ram,
        kind: RowKind::Text(|view| match &view.state.memory_info {
            Some(mem) => Value::plain(stat_format::gib2(mem.available_gib)),
            None => Value::absent(),
        }),
    },
    Row {
        id: "ram_total",
        label: "Total",
        panel_label: "Total",
        group: Group::Ram,
        kind: RowKind::Text(|view| match &view.state.memory_info {
            Some(mem) => Value::plain(stat_format::gib2(mem.total_gib)),
            None => Value::absent(),
        }),
    },
    Row {
        id: "ram_type",
        label: "Memory Type",
        panel_label: "Type",
        group: Group::Ram,
        kind: RowKind::Text(|view| match &view.state.memory_info {
            Some(mem) => match &mem.memory_type {
                Some(kind) => Value::plain(kind),
                None => Value::absent(),
            },
            None => Value::absent(),
        }),
    },
    Row {
        id: "ram_frequency",
        label: "Memory Frequency",
        panel_label: "MemClk",
        group: Group::Ram,
        kind: RowKind::Text(|view| match &view.state.memory_info {
            Some(mem) => match mem.memory_frequency {
                Some(mhz) => Value::plain(stat_format::mhz(mhz)),
                None => Value::absent(),
            },
            None => Value::absent(),
        }),
    },
    // ---- Gamepad: the first remembered one, as the tab's first block ----
    Row {
        id: "gamepad_name",
        label: "Gamepad",
        panel_label: "Pad",
        group: Group::Gamepad,
        kind: RowKind::Text(
            |view| match primary_gamepad(&view.state.config.remembered_gamepads) {
                Some(pad) => Value::plain(&pad.name),
                None => Value::absent(),
            },
        ),
    },
    Row {
        id: "gamepad_status",
        label: "Status",
        panel_label: "Status",
        group: Group::Gamepad,
        kind: RowKind::Text(
            |view| match primary_gamepad(&view.state.config.remembered_gamepads) {
                Some(pad) => Value::plain(match pad.status {
                    GamepadStatus::Connected => "Connected",
                    GamepadStatus::Disconnected => "Disconnected",
                }),
                None => Value::absent(),
            },
        ),
    },
    Row {
        id: "gamepad_battery",
        label: "Battery",
        panel_label: "Batt",
        group: Group::Gamepad,
        kind: RowKind::Text(|view| {
            match view
                .state
                .config
                .remembered_gamepads
                .first()
                .and_then(|p| p.battery_level)
            {
                Some(level) => Value::plain(format!("{}%", level)),
                None => Value::absent(),
            }
        }),
    },
    // ---- Frames: the owner's six first-release elements ----
    //
    // Each reads the GUI's own reader (see `crate::panel::frames`), which is the
    // only place a game's present time can come from. `snapshot()` clones one
    // immutable value; no file is touched here.
    Row {
        id: "game_name",
        label: "Game",
        panel_label: "Game",
        group: Group::Frame,
        kind: RowKind::Text(|view| match &view.frames.source {
            Some(source) => Value::plain(source.exe_name.as_str()),
            None => Value::absent(),
        }),
    },
    Row {
        id: "fps",
        label: "FPS",
        panel_label: "FPS",
        group: Group::Frame,
        kind: RowKind::Text(|view| match view.frames.metrics {
            Some(metrics) => Value::plain(fps_text(metrics.avg_fps)),
            None => Value::absent(),
        }),
    },
    Row {
        id: "frametime_ms",
        label: "Frame Time",
        panel_label: "Frame",
        group: Group::Frame,
        kind: RowKind::Text(|view| match view.frames.metrics {
            Some(metrics) => Value::plain(ms_text(metrics.frametime_ms)),
            None => Value::absent(),
        }),
    },
    Row {
        id: "frametime_graph",
        label: "Frame Time Graph",
        panel_label: "Graph",
        group: Group::Frame,
        // The strip. Its menu value is the scale it draws against.
        kind: RowKind::Graph,
    },
    Row {
        id: "low_1pct",
        label: "1% Low",
        panel_label: "1%low",
        group: Group::Frame,
        kind: RowKind::Text(|view| match view.frames.metrics {
            Some(metrics) => Value::plain(fps_text(metrics.low_1pct)),
            None => Value::absent(),
        }),
    },
    Row {
        id: "low_01pct",
        label: "0.1% Low",
        panel_label: "0.1%",
        group: Group::Frame,
        kind: RowKind::Text(|view| match view.frames.metrics {
            Some(metrics) => Value::plain(fps_text(metrics.low_01pct)),
            None => Value::absent(),
        }),
    },
];

/// An fps value, one decimal: the precision every overlay's readout uses, and
/// enough to see a 0.1 fps change without the text width moving the eye.
fn fps_text(fps: f64) -> String {
    format!("{fps:.1} fps")
}

/// A frame time in milliseconds, two decimals: 16.67 and 6.07 are both readable,
/// and the extra digit is where a hitch shows before the fps does.
fn ms_text(ms: f64) -> String {
    format!("{ms:.2} ms")
}

/// Look a row up by id.
pub fn find(id: &str) -> Option<&'static Row> {
    ROWS.iter().find(|row| row.id == id)
}

/// The GPU whose rows the panel reads.
///
/// **Rule: the discrete GPU if there is one, otherwise the first entry.**
///
/// Not `.first()`, and the distinction is deliberate. A machine with an iGPU plus
/// a dGPU reports the iGPU first on some drivers and the dGPU first on others, so
/// "first" would make the panel show different quantities — and usually the less
/// interesting ones — depending on enumeration order. The discrete GPU is the one
/// with its own memory, clocks and power draw, which is what a panel exists to
/// report. Integrated-only machines fall back to the first entry, which is then
/// the only one.
///
/// With neither, every GPU row renders `—` (see [`Value::absent`]).
pub fn primary_gpu<'a>(
    gpus: &'a [lapsphere_common::types::GpuInfo],
) -> Option<&'a lapsphere_common::types::GpuInfo> {
    gpus.iter()
        .find(|gpu| gpu.gpu_type == GpuType::Discrete)
        .or_else(|| gpus.first())
}

/// The gamepad whose rows the panel reads.
///
/// **Rule: the first CONNECTED gamepad, otherwise the first remembered one.**
///
/// A remembered gamepad can be long disconnected (the registry keeps rows for
/// identity stability across reconnects), so preferring a connected one keeps the
/// panel showing live values rather than a stale `Disconnected`. Falls back to
/// the first entry so a disconnected pad still names itself rather than going
/// fully blank.
pub fn primary_gamepad<'a>(
    pads: &'a [lapsphere_common::types::GamepadInfo],
) -> Option<&'a lapsphere_common::types::GamepadInfo> {
    pads.iter()
        .find(|pad| pad.status == GamepadStatus::Connected)
        .or_else(|| pads.first())
}

/// Every row id, in display order.
pub fn all_ids() -> Vec<&'static str> {
    ROWS.iter().map(|row| row.id).collect()
}

/// The ids a freshly-installed panel shows: the hardware rows it drew before the
/// frame elements existed, and none of the frame rows.
///
/// Rationale, not an arbitrary pick:
///
/// * a machine with no gamepad ever connected would otherwise open with three
///   permanent dashes at the bottom, and a disconnected gamepad is better added
///   deliberately from the menu than carried as noise. Every other hardware row
///   describes hardware that is always present;
/// * the six frame elements are first-release elements by the owner's decision
///   (`docs/development/panel-v1-baseline.md` §4), and being in the registry,
///   configurable and drawn is what that decision requires of them — but they are
///   **not** on by default, by the same rule the gamepad block follows. Their
///   source is an implicit Vulkan layer that nothing installs for the user, so a
///   fresh install would otherwise open with five permanent `—` rows and an empty
///   strip. They are one click away, under "Frames" in the panel settings, and a
///   `panel.json` from any earlier build keeps its existing rows untouched.
pub const DEFAULT_VISIBLE: &[&str] = &[
    "cpu_processor",
    "cpu_average_frequency",
    "cpu_average_load",
    "cpu_package_temperature",
    "gpu_name",
    "gpu_core_frequency",
    "gpu_load",
    "ram_usage",
    "ram_used",
    "ram_total",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppState;
    use crate::panel::frames::{stats, FrameSnapshot, Source};
    use crate::panel::PanelView;
    use std::sync::Arc;

    #[test]
    fn ids_are_unique() {
        let mut ids = all_ids();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate row id in the registry");
    }

    #[test]
    fn every_default_visible_id_exists() {
        for id in DEFAULT_VISIBLE {
            assert!(find(id).is_some(), "default id {id} is not in ROWS");
        }
    }

    #[test]
    fn every_row_is_reachable_from_its_group() {
        // `Group::of` is what the menu uses to bucket the checkboxes; a row no
        // group claims would be invisible in the menu and unreachable.
        for row in ROWS {
            assert!(
                Group::ALL.iter().any(|group| group.of(row.id)),
                "row {} belongs to no group",
                row.id
            );
        }
    }

    /// Rows with no `"Label:"` counterpart in the Statistics tab, each with the
    /// reason. Listed explicitly rather than weakening the guard, so a NEW
    /// row that drifts from the tab still fails the test.
    ///
    /// * `gpu_name`, `gamepad_name` — the tab shows a GPU's and a gamepad's name
    ///   as a bold heading above the grid
    ///   (`ui.label(RichText::new(&gpu.name).strong())`), not as a labelled row.
    /// * `ram_type` — `MemoryInfo::memory_type` exists on the wire type, but
    ///   `draw_memory_info` never renders it. The panel shows a value the tab
    ///   does not; that is a deliberate gap, not a duplicate.
    /// * the six frame rows — the tab shows no frame data at all, and cannot: the
    ///   numbers come from inside the game process (the Vulkan layer's shared
    ///   memory), not from a daemon getter. There is nothing in the tab for them to
    ///   duplicate, and nothing to drift from.
    const NO_TAB_LABEL: &[&str] = &[
        "gpu_name",
        "gamepad_name",
        "ram_type",
        "game_name",
        "fps",
        "frametime_ms",
        "frametime_graph",
        "low_1pct",
        "low_01pct",
    ];

    #[test]
    fn every_row_label_appears_in_the_statistics_tab() {
        // The drift guard. The two surfaces must not offer different names for
        // the same quantity, and this is the check that notices when someone
        // renames a row in the tab without touching the registry.
        let statistics = include_str!("../pages/statistics.rs");
        for row in ROWS {
            // The tab prints labels with a trailing colon, e.g.
            // `ui.label("Average Frequency:")`; the panel prints them without
            // one. So the guard looks for the tab's form.
            let found = statistics.contains(&format!("\"{}:\"", row.label))
                || NO_TAB_LABEL.contains(&row.id);
            assert!(
                found,
                "the Statistics tab has no row labelled {:?}; panel row {} duplicates nothing",
                row.label, row.id
            );
        }
    }

    /// Build a `GpuInfo` from JSON: the wire type has 30+ required fields and no
    /// `Default`, so a hand-written fixture would not compile — and a JSON
    /// fixture proves the shape is one the daemon actually sends.
    fn gpu(name: &str, gpu_type: GpuType) -> lapsphere_common::types::GpuInfo {
        let mut value: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/gpu_legacy.json"))
                .expect("fixture parses");
        value["name"] = serde_json::json!(name);
        value["gpu_type"] = serde_json::json!(match gpu_type {
            GpuType::Integrated => "Integrated",
            GpuType::Discrete => "Discrete",
        });
        serde_json::from_value(value).expect("fixture decodes")
    }

    fn pad(status: GamepadStatus, name: &str) -> lapsphere_common::types::GamepadInfo {
        lapsphere_common::types::GamepadInfo {
            name: name.to_string(),
            status,
            ..serde_json::from_str::<lapsphere_common::types::GamepadInfo>("{}").expect("defaults")
        }
    }

    #[test]
    fn the_discrete_gpu_wins_even_when_the_igpu_is_listed_first() {
        // The rule the owner set, and the reason it is not `.first()`: driver
        // enumeration order decides which entry comes first, and it differs
        // between machines. Whichever order the daemon uses, the dGPU is the one
        // with its own clocks and power.
        let igpu_then_dgpu = vec![
            gpu("AMD Radeon 780M", GpuType::Integrated),
            gpu("NVIDIA GeForce RTX 3070", GpuType::Discrete),
        ];
        assert_eq!(
            primary_gpu(&igpu_then_dgpu).map(|g| g.name.as_str()),
            Some("NVIDIA GeForce RTX 3070")
        );

        // And the reverse order gives the same answer, which is the point.
        let dgpu_then_igpu = vec![
            gpu("NVIDIA GeForce RTX 3070", GpuType::Discrete),
            gpu("AMD Radeon 780M", GpuType::Integrated),
        ];
        assert_eq!(
            primary_gpu(&dgpu_then_igpu).map(|g| g.name.as_str()),
            Some("NVIDIA GeForce RTX 3070")
        );
    }

    #[test]
    fn an_integrated_only_machine_falls_back_to_the_first_gpu() {
        let only = vec![gpu("AMD Radeon 780M", GpuType::Integrated)];
        assert_eq!(
            primary_gpu(&only).map(|g| g.name.as_str()),
            Some("AMD Radeon 780M")
        );
        // No GPU at all: every GPU row renders the absent placeholder.
        assert!(primary_gpu(&[]).is_none());
    }

    #[test]
    fn a_connected_gamepad_wins_over_a_remembered_disconnected_one() {
        let pads = vec![
            pad(GamepadStatus::Disconnected, "Old Pad"),
            pad(GamepadStatus::Connected, "Xbox Wireless Controller"),
        ];
        assert_eq!(
            primary_gamepad(&pads).map(|p| p.name.as_str()),
            Some("Xbox Wireless Controller")
        );
    }

    #[test]
    fn with_no_connected_gamepad_the_first_one_is_still_named() {
        // The registry keeps rows for disconnected pads so identities survive a
        // reconnect, so "first" here is a remembered pad, not a live one.
        let pads = vec![
            pad(GamepadStatus::Disconnected, "Old Pad"),
            pad(GamepadStatus::Disconnected, "Another Pad"),
        ];
        assert_eq!(
            primary_gamepad(&pads).map(|p| p.name.as_str()),
            Some("Old Pad")
        );
        assert!(primary_gamepad(&[]).is_none());
    }

    #[test]
    fn the_examples_from_the_owners_decision_all_exist() {
        // The decision named these four rows explicitly, so they are the rows
        // most likely to be dropped in a refactor.
        for id in [
            "cpu_processor",
            "cpu_average_frequency",
            "cpu_average_load",
            "cpu_package_temperature",
        ] {
            assert!(find(id).is_some(), "{id} must exist");
        }
    }

    // ---- the frame elements ----

    #[test]
    fn the_six_frame_elements_are_the_ones_the_owners_decision_names() {
        // The decision's list, in its order. A missing element is a silently
        // dropped requirement, and an extra one is scope creep.
        for id in frames::ELEMENT_IDS {
            let row = find(id).unwrap_or_else(|| panic!("{id} is not in the registry"));
            assert_eq!(row.group, Group::Frame, "{id} must be in the Frames group");
        }
        let registry: Vec<&str> = ROWS
            .iter()
            .filter(|row| row.group == Group::Frame)
            .map(|row| row.id)
            .collect();
        assert_eq!(registry, frames::ELEMENT_IDS.to_vec());
    }

    #[test]
    fn the_frame_rows_are_not_on_by_default_and_are_not_in_the_shown_set() {
        // The rule the gamepad block follows: a source that may never exist does
        // not open as a permanent row of dashes. They are one click away in the
        // Frames section of the settings menu, and an older panel.json keeps
        // whatever it already had.
        for id in frames::ELEMENT_IDS {
            assert!(
                !DEFAULT_VISIBLE.contains(&id),
                "{id} must not be shown by a fresh install"
            );
        }
    }

    #[test]
    fn exactly_one_row_is_the_strip_and_it_is_the_frame_time_graph() {
        let strips: Vec<&str> = ROWS
            .iter()
            .filter(|row| row.is_graph())
            .map(|row| row.id)
            .collect();
        assert_eq!(strips, vec!["frametime_graph"]);
        // A strip row has no text value; the menu prints its scale instead.
        let state = AppState::new();
        let empty = FrameSnapshot::default();
        let view = PanelView::new(&state, &empty);
        assert!(find("frametime_graph").expect("row").text(&view).is_none());
        assert_eq!(
            find("frametime_graph").expect("row").menu_value(&view),
            frames::graph::scale_caption(1)
        );

        // And when the window is wider than the strip, the caption says how many
        // intervals a column stands for rather than leaving a smooth shape to read as
        // a smooth game.
        let decimated = FrameSnapshot {
            column_span: 20,
            ..FrameSnapshot::default()
        };
        assert_eq!(
            find("frametime_graph")
                .expect("row")
                .menu_value(&PanelView::new(&state, &decimated)),
            "0–50 ms · worst of 20"
        );
        // Every other row is a text row.
        for row in ROWS.iter().filter(|row| row.id != "frametime_graph") {
            assert!(!row.is_graph(), "{} must be a text row", row.id);
        }
    }

    #[test]
    fn with_no_source_every_frame_row_reads_as_absent() {
        // The reader is offline in `AppState::new()`: no segment, no thread. The
        // six rows must render `—` (and a strip with nothing in it), never a zero.
        let state = AppState::new();
        let snapshot = FrameSnapshot::default();
        let view = PanelView::new(&state, &snapshot);
        for id in frames::ELEMENT_IDS {
            let row = find(id).expect("row");
            if let Some(value) = row.text(&view) {
                assert_eq!(value.text, ABSENT, "{id} must render the absent marker");
            }
        }
    }

    #[test]
    fn with_a_live_source_the_frame_rows_print_the_decision_s_numbers() {
        // 120 frames of 8 ms and two 40 ms hitches: 122 intervals, so the mean is
        // 8.5246 ms, the average is 117.3 fps, and both lows are the hitches —
        // ceil(1 % × 122) = 2 worst intervals, and ceil(0.1 % × 122) = 1, which is
        // the same 40 ms frame. The values below are the registry's FORMATTING,
        // not the statistics; `frames::stats` owns the statistics themselves.
        let state = AppState::new();
        let mut window = vec![8.0f64; 120];
        window.push(40.0);
        window.push(40.0);
        let metrics = stats::metrics(&window, 0.0).expect("metrics");
        let snapshot = FrameSnapshot {
            source: Some(Source {
                pid: 4242,
                exe_name: "vkcube".to_string(),
            }),
            metrics: Some(metrics),
            samples_ms: Arc::from(vec![8.0f32; 5]),
            column_span: 1,
        };
        state.frames.publish_for_test(snapshot.clone());
        let view = PanelView::new(&state, &snapshot);

        let text = |id: &str| find(id).expect("row").text(&view).expect("text").text;
        assert_eq!(text("game_name"), "vkcube");
        assert_eq!(text("fps"), "117.3 fps");
        assert_eq!(text("frametime_ms"), "8.52 ms");
        assert_eq!(text("low_1pct"), "25.0 fps");
        assert_eq!(text("low_01pct"), "25.0 fps");
        // The strip is drawn, not printed.
        assert!(find("frametime_graph").expect("row").text(&view).is_none());
    }

    #[test]
    fn every_frame_element_reads_the_snapshot_it_was_given() {
        // The guard for "one snapshot per frame". A view is built from the FIRST
        // snapshot and the reader is then handed a SECOND one -- a different game,
        // different numbers, a different series -- which is exactly the interleaving a
        // row that fetched its own snapshot would pick up mid-frame. Every element
        // must still report the first.
        let state = AppState::new();
        let first = FrameSnapshot {
            source: Some(Source {
                pid: 1111,
                exe_name: "first-game".to_string(),
            }),
            metrics: Some(stats::metrics(&[8.0f64; 120], 0.0).expect("metrics")),
            samples_ms: Arc::from(vec![8.0f32; 3]),
            column_span: 1,
        };
        let second = FrameSnapshot {
            source: Some(Source {
                pid: 2222,
                exe_name: "second-game".to_string(),
            }),
            metrics: Some(stats::metrics(&[20.0f64; 120], 0.0).expect("metrics")),
            samples_ms: Arc::from(vec![20.0f32; 5]),
            column_span: 9,
        };
        let view = PanelView::new(&state, &first);

        // The source of truth changes under the view.
        state.frames.publish_for_test(second);

        let text = |id: &str| find(id).expect("row").text(&view).expect("text").text;
        assert_eq!(text("game_name"), "first-game", "not the newer source");
        assert_eq!(text("fps"), "125.0 fps", "not the newer numbers");
        assert_eq!(text("frametime_ms"), "8.00 ms");
        assert_eq!(text("low_1pct"), "125.0 fps");
        assert_eq!(text("low_01pct"), "125.0 fps");
        // The strip's series and its caption come from the view too: five columns or
        // "worst of 9" would mean an element reached past the view into the reader.
        assert_eq!(view.frames.samples_ms.len(), 3);
        assert_eq!(view.frames.column_span, 1, "one column per interval");
        assert_eq!(
            find("frametime_graph").expect("row").menu_value(&view),
            "0–50 ms"
        );
    }
}
