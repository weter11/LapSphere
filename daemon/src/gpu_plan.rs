//! Pure decision for which NVML-touching GPU operations to run.
//!
//! Waking a runtime-suspended dGPU to push settings costs power and can fight
//! RTD3, so every NVML write is gated on the sysfs suspend state. This module
//! holds the decision only; it performs no I/O, so it is testable without
//! hardware.

use lapsphere_common::types::GpuSettings;

/// One NVML-touching operation the daemon may perform for the dGPU.
#[derive(Debug, Clone, PartialEq)]
pub enum GpuOp {
    PowerLimit(u32),
    CoreOffset(f32),
    MemoryOffset(f32),
    LockedClocks(u32, u32),
    ResetClocks,
    FanSpeed { fan_id: u32, speed: u32 },
    FanAuto { fan_id: u32 },
}

/// What to do for the current tick.
#[derive(Debug, Clone, PartialEq)]
pub enum GpuPlan {
    /// dGPU is suspended: touch nothing, settings stay in GPU_DAEMON_STATE.
    Skip,
    /// dGPU is active and the generation is already applied: nothing to do.
    UpToDate,
    /// dGPU is active and the generation is new: apply the full set.
    Apply { ops: Vec<GpuOp>, generation: u64 },
}

/// Decide the GPU work for one pass.
///
/// `applied_gen` is the generation that was last fully applied. A new profile
/// bumps the generation, so a re-apply is needed exactly once per profile
/// change, not on every poll tick.
pub fn gpu_apply_plan(
    suspended: bool,
    settings: &GpuSettings,
    generation: u64,
    applied_gen: u64,
) -> GpuPlan {
    if suspended {
        return GpuPlan::Skip;
    }
    if generation == applied_gen {
        return GpuPlan::UpToDate;
    }

    let mut ops = Vec::new();
    if let Some(limit) = settings.power_limit {
        ops.push(GpuOp::PowerLimit(limit));
    }
    if let Some(offset) = settings.core_offset {
        ops.push(GpuOp::CoreOffset(offset));
    }
    if let Some(offset) = settings.memory_offset {
        ops.push(GpuOp::MemoryOffset(offset));
    }
    match (settings.min_gpu_clock, settings.max_gpu_clock) {
        (Some(min), Some(max)) => ops.push(GpuOp::LockedClocks(min, max)),
        _ => ops.push(GpuOp::ResetClocks),
    }
    for fan in &settings.nvidia_fans {
        if fan.manual {
            ops.push(GpuOp::FanSpeed { fan_id: fan.fan_id, speed: fan.speed });
        } else {
            ops.push(GpuOp::FanAuto { fan_id: fan.fan_id });
        }
    }

    GpuPlan::Apply { ops, generation }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lapsphere_common::types::{GpuSettings, NvidiaFanSettings};

    fn full_settings() -> GpuSettings {
        let mut s = GpuSettings::default();
        s.power_limit = Some(120);
        s.core_offset = Some(50.0);
        s.memory_offset = Some(-200.0);
        s.min_gpu_clock = Some(500);
        s.max_gpu_clock = Some(1800);
        s.nvidia_fans = vec![NvidiaFanSettings {
            device_index: 0,
            fan_id: 0,
            speed: 60,
            manual: true,
        }];
        s
    }

    #[test]
    fn suspended_gpu_plans_no_nvml_operation() {
        let plan = gpu_apply_plan(true, &full_settings(), 1, 0);
        assert_eq!(plan, GpuPlan::Skip);
    }

    #[test]
    fn suspended_gpu_skips_even_when_generation_is_new() {
        // A new generation must not wake the adapter.
        assert_eq!(gpu_apply_plan(true, &full_settings(), 7, 3), GpuPlan::Skip);
    }

    #[test]
    fn active_gpu_plans_full_set_for_new_generation() {
        match gpu_apply_plan(false, &full_settings(), 1, 0) {
            GpuPlan::Apply { ops, generation } => {
                assert_eq!(generation, 1);
                assert!(ops.contains(&GpuOp::PowerLimit(120)));
                assert!(ops.contains(&GpuOp::CoreOffset(50.0)));
                assert!(ops.contains(&GpuOp::MemoryOffset(-200.0)));
                assert!(ops.contains(&GpuOp::LockedClocks(500, 1800)));
                assert!(ops.contains(&GpuOp::FanSpeed { fan_id: 0, speed: 60 }));
            }
            other => panic!("expected Apply, got {:?}", other),
        }
    }

    #[test]
    fn active_gpu_resets_clocks_when_no_locked_range() {
        let mut s = full_settings();
        s.min_gpu_clock = None;
        match gpu_apply_plan(false, &s, 1, 0) {
            GpuPlan::Apply { ops, .. } => assert!(ops.contains(&GpuOp::ResetClocks)),
            other => panic!("expected Apply, got {:?}", other),
        }
    }

    #[test]
    fn same_generation_applies_nothing() {
        assert_eq!(
            gpu_apply_plan(false, &full_settings(), 4, 4),
            GpuPlan::UpToDate
        );
    }
}
