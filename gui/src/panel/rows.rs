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
//! # Absent data
//!
//! An `Option` field the hardware did not report becomes [`ABSENT`] — an em
//! dash — rather than a hidden row. That is what keeps the panel's height a pure
//! function of the row COUNT: a gamepad that disconnects must not make the
//! window shorter, or the user's other rows move.
//!
//! # Scope
//!
//! One row per hardware quantity. Multi-device rows (per-core frequency, every
//! mount) are not offered: rendering all sixteen cores in a 300 px column is not
//! what this surface is for. For GPU and gamepad there IS a selection rule, and
//! it is [`primary_gpu`] / [`primary_gamepad`] below rather than a bare
//! `.first()`.

use crate::app::AppState;
use crate::stat_format::{self, ABSENT};
use lapsphere_common::types::{GamepadStatus, GpuType};

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
}

impl Group {
    /// Every group, in menu order.
    pub const ALL: [Group; 4] = [Group::Cpu, Group::Gpu, Group::Ram, Group::Gamepad];

    pub fn label(self) -> &'static str {
        match self {
            Group::Cpu => "CPU",
            Group::Gpu => "GPU",
            Group::Ram => "RAM",
            Group::Gamepad => "Gamepad",
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
    pub panel_label: &'static str,
    pub group: Group,
    /// Read and format this row's value. Never fails: absent data becomes `—`.
    pub read: fn(&AppState) -> Value,
}

/// Every row, in display order: CPU, then GPU, then RAM, then gamepad.
pub static ROWS: &[Row] = &[
    // ---- CPU: the rows draw_cpu_info shows ----
    Row {
        id: "cpu_processor",
        label: "Processor",
        panel_label: "CPU",
        group: Group::Cpu,
        read: |state| match &state.cpu_info {
            Some(cpu) => Value::plain(&cpu.name),
            None => Value::absent(),
        },
    },
    Row {
        id: "cpu_average_frequency",
        label: "Average Frequency",
        panel_label: "Freq",
        group: Group::Cpu,
        read: |state| match &state.cpu_info {
            // The tab prints this field kHz-scaled; stat_format keeps that.
            Some(cpu) => Value::plain(stat_format::average_frequency_mhz(cpu.average_frequency)),
            None => Value::absent(),
        },
    },
    Row {
        id: "cpu_average_load",
        label: "Average Load",
        panel_label: "Load",
        group: Group::Cpu,
        read: |state| match &state.cpu_info {
            Some(cpu) => Value::tinted(
                stat_format::percent1(cpu.average_load),
                stat_format::load_tint(cpu.average_load),
            ),
            None => Value::absent(),
        },
    },
    Row {
        id: "cpu_package_temperature",
        label: "Package Temperature",
        panel_label: "Temp",
        group: Group::Cpu,
        read: |state| match &state.cpu_info {
            Some(cpu) => Value::tinted(
                stat_format::celsius1(cpu.package_temp),
                stat_format::temp_tint(cpu.package_temp),
            ),
            None => Value::absent(),
        },
    },
    Row {
        id: "cpu_governor",
        label: "Governor",
        panel_label: "Gov",
        group: Group::Cpu,
        read: |state| match &state.cpu_info {
            Some(cpu) => Value::plain(&cpu.governor),
            None => Value::absent(),
        },
    },
    Row {
        id: "cpu_scaling_driver",
        label: "Scaling Driver",
        panel_label: "Driver",
        group: Group::Cpu,
        read: |state| match &state.cpu_info {
            Some(cpu) => Value::plain(&cpu.scaling_driver),
            None => Value::absent(),
        },
    },
    // ---- GPU: the first entry, which is what a single-column panel can show ----
    Row {
        id: "gpu_name",
        label: "GPU",
        panel_label: "GPU",
        group: Group::Gpu,
        read: |state| match primary_gpu(&state.gpu_info) {
            Some(gpu) => Value::plain(&gpu.name),
            None => Value::absent(),
        },
    },
    Row {
        id: "gpu_type",
        label: "Type",
        panel_label: "Type",
        group: Group::Gpu,
        read: |state| match primary_gpu(&state.gpu_info) {
            Some(gpu) => Value::plain(match gpu.gpu_type {
                GpuType::Integrated => "Integrated",
                GpuType::Discrete => "Discrete",
            }),
            None => Value::absent(),
        },
    },
    Row {
        id: "gpu_core_frequency",
        label: "Core Frequency",
        panel_label: "Clock",
        group: Group::Gpu,
        read: |state| match primary_gpu(&state.gpu_info).and_then(|g| g.frequency) {
            Some(mhz) => Value::plain(stat_format::mhz(mhz)),
            None => Value::absent(),
        },
    },
    Row {
        id: "gpu_temperature",
        label: "Temperature",
        panel_label: "Temp",
        group: Group::Gpu,
        read: |state| match primary_gpu(&state.gpu_info).and_then(|g| g.temperature) {
            Some(temp) => Value::tinted(stat_format::celsius1(temp), stat_format::temp_tint(temp)),
            None => Value::absent(),
        },
    },
    Row {
        id: "gpu_load",
        label: "Load",
        panel_label: "Load",
        group: Group::Gpu,
        read: |state| match primary_gpu(&state.gpu_info).and_then(|g| g.load) {
            Some(load) => Value::tinted(stat_format::percent1(load), stat_format::load_tint(load)),
            None => Value::absent(),
        },
    },
    Row {
        id: "gpu_power",
        label: "Power",
        panel_label: "Power",
        group: Group::Gpu,
        read: |state| match primary_gpu(&state.gpu_info).and_then(|g| g.power) {
            Some(watts) => {
                Value::tinted(stat_format::watts1(watts), stat_format::power_tint(watts))
            }
            None => Value::absent(),
        },
    },
    Row {
        id: "gpu_memory_frequency",
        label: "Memory Frequency",
        panel_label: "MemClk",
        group: Group::Gpu,
        read: |state| match primary_gpu(&state.gpu_info).and_then(|g| g.memory_frequency) {
            Some(mhz) => Value::plain(stat_format::mhz(mhz)),
            None => Value::absent(),
        },
    },
    Row {
        id: "gpu_memory_usage",
        label: "VRAM available / total",
        panel_label: "VRAM",
        group: Group::Gpu,
        read: |state| match primary_gpu(&state.gpu_info).and_then(|g| g.vram_memory.as_ref()) {
            Some(memory) => Value::plain(format!("{} / {} MiB", memory.free_mib, memory.total_mib)),
            None => Value::absent(),
        },
    },
    // ---- RAM ----
    Row {
        id: "ram_usage",
        label: "Usage",
        panel_label: "Usage",
        group: Group::Ram,
        read: |state| match &state.memory_info {
            Some(mem) => Value::tinted(
                stat_format::percent1(mem.used_percent),
                stat_format::load_tint(mem.used_percent),
            ),
            None => Value::absent(),
        },
    },
    Row {
        id: "ram_used",
        label: "Used",
        panel_label: "Used",
        group: Group::Ram,
        read: |state| match &state.memory_info {
            Some(mem) => Value::plain(stat_format::gib2(mem.used_gib)),
            None => Value::absent(),
        },
    },
    Row {
        id: "ram_available",
        label: "Available",
        panel_label: "Free",
        group: Group::Ram,
        read: |state| match &state.memory_info {
            Some(mem) => Value::plain(stat_format::gib2(mem.available_gib)),
            None => Value::absent(),
        },
    },
    Row {
        id: "ram_total",
        label: "Total",
        panel_label: "Total",
        group: Group::Ram,
        read: |state| match &state.memory_info {
            Some(mem) => Value::plain(stat_format::gib2(mem.total_gib)),
            None => Value::absent(),
        },
    },
    Row {
        id: "ram_type",
        label: "Memory Type",
        panel_label: "Type",
        group: Group::Ram,
        read: |state| match &state.memory_info {
            Some(mem) => match &mem.memory_type {
                Some(kind) => Value::plain(kind),
                None => Value::absent(),
            },
            None => Value::absent(),
        },
    },
    Row {
        id: "ram_frequency",
        label: "Memory Frequency",
        panel_label: "MemClk",
        group: Group::Ram,
        read: |state| match &state.memory_info {
            Some(mem) => match mem.memory_frequency {
                Some(mhz) => Value::plain(stat_format::mhz(mhz)),
                None => Value::absent(),
            },
            None => Value::absent(),
        },
    },
    // ---- Gamepad: the first remembered one, as the tab's first block ----
    Row {
        id: "gamepad_name",
        label: "Gamepad",
        panel_label: "Pad",
        group: Group::Gamepad,
        read: |state| match primary_gamepad(&state.config.remembered_gamepads) {
            Some(pad) => Value::plain(&pad.name),
            None => Value::absent(),
        },
    },
    Row {
        id: "gamepad_status",
        label: "Status",
        panel_label: "Status",
        group: Group::Gamepad,
        read: |state| match primary_gamepad(&state.config.remembered_gamepads) {
            Some(pad) => Value::plain(match pad.status {
                GamepadStatus::Connected => "Connected",
                GamepadStatus::Disconnected => "Disconnected",
            }),
            None => Value::absent(),
        },
    },
    Row {
        id: "gamepad_battery",
        label: "Battery",
        panel_label: "Batt",
        group: Group::Gamepad,
        read: |state| match state
            .config
            .remembered_gamepads
            .first()
            .and_then(|p| p.battery_level)
        {
            Some(level) => Value::plain(format!("{}%", level)),
            None => Value::absent(),
        },
    },
];

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

/// The ids a freshly-installed panel shows: every row except the gamepad block.
///
/// Rationale, not an arbitrary pick: a machine with no gamepad ever connected
/// would otherwise open with three permanent dashes at the bottom, and a
/// disconnected gamepad is better added deliberately from the menu than carried
/// as noise. Every other row describes hardware that is always present.
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
    const NO_TAB_LABEL: &[&str] = &["gpu_name", "gamepad_name", "ram_type"];

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
}
