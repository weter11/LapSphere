//! The element registry: id → where its value comes from in `AppState`.
//!
//! Every id in `config::PANEL_ITEMS` resolves here to a function of
//! `&AppState`. Three properties the panel depends on:
//!
//! * **Fixed slot.** Each element declares its width and height contribution at
//!   design time (`Slot`), so the layout is computable from the element set and
//!   the fonts alone — never from whether data arrived. That is what makes a
//!   missing value cost an em dash instead of a resize (ADR-2 in
//!   `docs/development/panel-design.md`).
//! * **No invented values.** An `Option` field that the hardware did not report
//!   becomes `—`. `ram_type`, `ram_freq` and the gamepad battery are all
//!   `Option` in `common/src/types.rs` and are all `—` on hardware that does not
//!   report them.
//! * **One GPU row.** The GPU elements read the first entry of
//!   `AppState::gpu_info`. `GpuInfo::load`, `.frequency` and
//!   `.memory_frequency` are `Option`, so an iGPU-only or driver-less state
//!   renders `—` rather than a fabricated number.
//!
//! Field provenance, checked against `common/src/types.rs` rather than assumed:
//!
//! | item | source |
//! | --- | --- |
//! | `cpu_name` | `CpuInfo::name` |
//! | `cpu_freq` | `CpuInfo::average_frequency` (u64 MHz) |
//! | `cpu_load` | `CpuInfo::average_load` (f32) |
//! | `gpu_name` | first `GpuInfo::name` |
//! | `gpu_clock` | first `GpuInfo::frequency` (`Option<u64>` MHz) |
//! | `gpu_load` | first `GpuInfo::load` (`Option<f32>`) |
//! | `gpu_mem_clock` | first `GpuInfo::memory_frequency` (`Option<u64>` MHz) |
//! | `gpu_mem_usage` | first `GpuInfo::vram_memory` → `GpuMemorySnapshot` used/total |
//! | `ram_usage` | `MemoryInfo::used_percent` (f32) |
//! | `ram_type` | `MemoryInfo::memory_type` (`Option<String>`) |
//! | `ram_freq` | `MemoryInfo::memory_frequency` (`Option<u64>`) |
//! | `gamepad` | first `GamepadInfo::name` + `battery_level` (`Option<u8>`) |
//! | `fps`, `fps_graph`, `frametime_graph` | **no source in this release** |
//!
//! The three frame elements have a receiver (`FrameSource`) and a reserved slot,
//! but no producer: the Vulkan layer is PR C and the data source is PR E. They
//! render `—` and their slot is already reserved, so enabling the data later
//! does not move the window.

use super::units::{self, Value};
use crate::app::AppState;

/// How much room an element occupies, in points at `font_scale = 1.0`.
///
/// Declared, not measured: the layout must be computable without data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slot {
    pub width: f32,
    pub height: f32,
}

/// Which coordinator component an element needs polled to have data at all.
///
/// Drives commit 4's poll narrowing: an element whose component is not polled
/// renders `—` permanently, so the poll set has to cover every visible element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Needs {
    Cpu,
    Gpu,
    Memory,
    Gamepads,
    /// No component: the frame elements read shared memory directly, not D-Bus.
    None,
}

/// A registered element.
pub struct PanelItem {
    pub id: &'static str,
    /// Built-in label, overridable per item in the config.
    pub default_label: &'static str,
    pub slot: Slot,
    pub needs: Needs,
    pub read: fn(&AppState) -> Value,
}

/// Look up an element by id.
pub fn find(id: &str) -> Option<&'static PanelItem> {
    PANEL_ITEMS.iter().find(|item| item.id == id)
}

/// Every registered element, in default order.
///
/// Order matches `config::PANEL_ITEMS`, which is the default display order; the
/// config's `items` array is what actually decides layout order.
pub static PANEL_ITEMS: &[PanelItem] = &[
    PanelItem {
        id: "cpu_name",
        default_label: "CPU",
        slot: Slot {
            width: 150.0,
            height: 18.0,
        },
        needs: Needs::Cpu,
        read: |state| match &state.cpu_info {
            Some(cpu) => Value::Text(cpu.name.clone()),
            None => Value::Absent,
        },
    },
    PanelItem {
        id: "cpu_freq",
        default_label: "CPU",
        slot: Slot {
            width: 92.0,
            height: 18.0,
        },
        needs: Needs::Cpu,
        read: |state| match &state.cpu_info {
            Some(cpu) => Value::Text(units::mhz(cpu.average_frequency)),
            None => Value::Absent,
        },
    },
    PanelItem {
        id: "cpu_load",
        default_label: "CPU",
        slot: Slot {
            width: 84.0,
            height: 18.0,
        },
        needs: Needs::Cpu,
        read: |state| match &state.cpu_info {
            Some(cpu) => Value::Text(units::percent(cpu.average_load)),
            None => Value::Absent,
        },
    },
    PanelItem {
        id: "gpu_name",
        default_label: "GPU",
        slot: Slot {
            width: 170.0,
            height: 18.0,
        },
        needs: Needs::Gpu,
        read: |state| match state.gpu_info.first() {
            Some(gpu) => Value::Text(gpu.name.clone()),
            None => Value::Absent,
        },
    },
    PanelItem {
        id: "gpu_clock",
        default_label: "GPU",
        slot: Slot {
            width: 92.0,
            height: 18.0,
        },
        needs: Needs::Gpu,
        read: |state| match state.gpu_info.first().and_then(|g| g.frequency) {
            Some(mhz) => Value::Text(units::mhz(mhz)),
            None => Value::Absent,
        },
    },
    PanelItem {
        id: "gpu_load",
        default_label: "GPU",
        slot: Slot {
            width: 84.0,
            height: 18.0,
        },
        needs: Needs::Gpu,
        read: |state| match state.gpu_info.first().and_then(|g| g.load) {
            Some(load) => Value::Text(units::percent(load)),
            None => Value::Absent,
        },
    },
    PanelItem {
        id: "gpu_mem_clock",
        default_label: "VRAM",
        slot: Slot {
            width: 92.0,
            height: 18.0,
        },
        needs: Needs::Gpu,
        read: |state| match state.gpu_info.first().and_then(|g| g.memory_frequency) {
            Some(mhz) => Value::Text(units::mhz(mhz)),
            None => Value::Absent,
        },
    },
    PanelItem {
        id: "gpu_mem_usage",
        default_label: "VRAM",
        slot: Slot {
            width: 118.0,
            height: 18.0,
        },
        needs: Needs::Gpu,
        // `GpuMemorySnapshot` carries used/total/free in MiB. Rendered as
        // "used / total MiB" in one fixed slot.
        read: |state| match state.gpu_info.first().and_then(|g| g.vram_memory.as_ref()) {
            Some(vram) => Value::Text(format!(
                "{} / {}",
                units::mib(vram.used_mib),
                units::mib(vram.total_mib)
            )),
            None => Value::Absent,
        },
    },
    PanelItem {
        id: "ram_usage",
        default_label: "RAM",
        slot: Slot {
            width: 96.0,
            height: 18.0,
        },
        needs: Needs::Memory,
        read: |state| match &state.memory_info {
            Some(memory) => Value::Text(units::percent(memory.used_percent)),
            None => Value::Absent,
        },
    },
    PanelItem {
        id: "ram_type",
        default_label: "RAM",
        slot: Slot {
            width: 104.0,
            height: 18.0,
        },
        needs: Needs::Memory,
        // `Option<String>`: absent on hardware that does not report a memory type.
        read: |state| match state
            .memory_info
            .as_ref()
            .and_then(|m| m.memory_type.clone())
        {
            Some(kind) => Value::Text(kind),
            None => Value::Absent,
        },
    },
    PanelItem {
        id: "ram_freq",
        default_label: "RAM",
        slot: Slot {
            width: 92.0,
            height: 18.0,
        },
        needs: Needs::Memory,
        read: |state| {
            units::optional(
                state.memory_info.as_ref().and_then(|m| m.memory_frequency),
                units::mhz,
            )
        },
    },
    PanelItem {
        id: "gamepad",
        default_label: "PAD",
        slot: Slot {
            width: 150.0,
            height: 18.0,
        },
        needs: Needs::Gamepads,
        // Name always; battery only when the pad reports one (`Option<u8>`).
        // No pad connected is `—`, not an empty row.
        read: |state| match state.gamepad_info.first() {
            Some(pad) => match pad.battery_level {
                Some(level) => Value::Text(format!("{} · {}", pad.name, units::percent_u8(level))),
                None => Value::Text(pad.name.clone()),
            },
            None => Value::Absent,
        },
    },
    // ---- Frame elements: slot reserved, no producer in this release ----
    PanelItem {
        id: "fps",
        default_label: "FPS",
        slot: Slot {
            width: 78.0,
            height: 18.0,
        },
        needs: Needs::None,
        read: |_state| Value::Absent,
    },
    PanelItem {
        id: "fps_graph",
        default_label: "FPS",
        slot: Slot {
            width: 120.0,
            height: 34.0,
        },
        needs: Needs::None,
        read: |_state| Value::Graph,
    },
    PanelItem {
        id: "frametime_graph",
        default_label: "FRAME",
        slot: Slot {
            width: 120.0,
            height: 34.0,
        },
        needs: Needs::None,
        read: |_state| Value::Graph,
    },
];

/// Which components must be polled to fill the given visible elements.
///
/// The coordinator poll set in panel mode is exactly this set. `logs` is never
/// in it: the ring reply is ~735 kB and no panel element reads it.
pub fn required_components<'a>(ids: impl Iterator<Item = &'a str>) -> Vec<&'static str> {
    let mut needed: Vec<&'static str> = Vec::new();
    for id in ids {
        let Some(item) = find(id) else { continue };
        let component = match item.needs {
            Needs::Cpu => "cpu",
            Needs::Gpu => "gpu",
            Needs::Memory => "memory",
            Needs::Gamepads => "gamepads",
            Needs::None => continue,
        };
        if !needed.contains(&component) {
            needed.push(component);
        }
    }
    needed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::config::{self, PanelConfig};

    /// An `AppState` with nothing polled: every element must be absent, and no
    /// element may panic on an empty state.
    fn empty_state() -> AppState {
        AppState::new()
    }

    #[test]
    fn every_config_id_has_a_registered_element() {
        for (id, _) in config::PANEL_ITEMS {
            assert!(
                find(id).is_some(),
                "config lists `{id}` but the registry does not define it"
            );
        }
    }

    #[test]
    fn every_registered_element_is_listed_in_the_config() {
        for item in PANEL_ITEMS {
            assert!(
                config::is_known_item(item.id),
                "registry defines `{}` but the config does not list it",
                item.id
            );
        }
    }

    #[test]
    fn an_empty_state_renders_absent_for_every_data_element() {
        let state = empty_state();
        for item in PANEL_ITEMS {
            let value = (item.read)(&state);
            match item.id {
                // Graphs are present-but-empty by design; everything else is `—`.
                "fps_graph" | "frametime_graph" => {
                    assert_eq!(value, Value::Graph, "{} must stay a graph", item.id)
                }
                _ => assert_eq!(
                    value,
                    Value::Absent,
                    "{} must be absent with no data, got {:?}",
                    item.id,
                    value.text()
                ),
            }
        }
    }

    #[test]
    fn absent_renders_the_em_dash_never_an_empty_string() {
        let state = empty_state();
        for item in PANEL_ITEMS {
            if matches!(item.id, "fps_graph" | "frametime_graph") {
                continue;
            }
            assert_eq!(
                (item.read)(&state).text(),
                units::NO_DATA,
                "{} must render the em dash",
                item.id
            );
        }
    }

    #[test]
    fn the_frame_elements_have_a_reserved_slot_and_no_component() {
        for id in ["fps", "fps_graph", "frametime_graph"] {
            let item = find(id).expect("registered");
            assert_eq!(item.needs, Needs::None, "{id} must not require a poll");
            assert!(item.slot.width > 0.0, "{id} must reserve width");
            assert!(item.slot.height > 0.0, "{id} must reserve height");
        }
        // The graphs are taller than a text row: a graph needs vertical room.
        assert!(find("fps_graph").unwrap().slot.height > find("cpu_load").unwrap().slot.height);
    }

    #[test]
    fn every_slot_has_a_positive_fixed_size() {
        for item in PANEL_ITEMS {
            assert!(item.slot.width > 0.0, "{} has no width", item.id);
            assert!(item.slot.height > 0.0, "{} has no height", item.id);
            assert!(
                item.slot.width.is_finite() && item.slot.height.is_finite(),
                "{} has a non-finite slot",
                item.id
            );
        }
    }

    #[test]
    fn required_components_follows_the_visible_elements() {
        // No GPU element selected -> gpu is not polled.
        let default_config = config::normalize(PanelConfig::default());
        let ids = config::visible_item_ids(&default_config);
        let with_gpu = required_components(ids.iter().copied());
        assert!(with_gpu.contains(&"gpu"));

        let only_cpu = required_components(["cpu_name", "cpu_load"].into_iter());
        assert_eq!(only_cpu, vec!["cpu"]);

        let no_gpu = required_components(["cpu_load", "ram_usage"].into_iter());
        assert!(
            !no_gpu.contains(&"gpu"),
            "gpu must not be polled without gpu items"
        );
        assert!(!no_gpu.contains(&"gamepads"));
        assert!(no_gpu.contains(&"memory"));
    }

    #[test]
    fn frame_elements_never_add_a_component_to_poll() {
        let components = required_components(["fps", "fps_graph", "frametime_graph"].into_iter());
        assert!(
            components.is_empty(),
            "frame elements read shm, not D-Bus: got {components:?}"
        );
    }

    #[test]
    fn unknown_ids_are_ignored_by_the_poll_set() {
        let components = required_components(["cpu_load", "not_an_element"].into_iter());
        assert_eq!(components, vec!["cpu"]);
    }

    #[test]
    fn the_poll_set_never_includes_logs() {
        // The ring reply is ~735 kB per call and no panel element reads it.
        let default_config = config::normalize(PanelConfig::default());
        let ids = config::visible_item_ids(&default_config);
        let components = required_components(ids.iter().copied());
        assert!(!components.contains(&"logs"));
    }

    #[test]
    fn required_components_are_deduplicated() {
        let components =
            required_components(["cpu_name", "cpu_freq", "cpu_load", "ram_usage"].into_iter());
        assert_eq!(components, vec!["cpu", "memory"]);
    }
}
