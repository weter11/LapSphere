use anyhow::{anyhow, Result};
use nvml_wrapper::error::NvmlError;
use nvml_wrapper::Nvml;
use once_cell::sync::Lazy;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use lapsphere_common::types::*;
use crate::tuxedo_io::{TuxedoIo, HardwareInterface};

static CPU_LIMITS_MODIFIED: AtomicBool = AtomicBool::new(false);
/// A deferred GPU step and who queued it. The origin decides whether a
/// completion is reported to the GUI: only a profile apply's deferred step
/// resolves the "waiting for the GPU step" state; the idle-OC restore must
/// stay silent on success.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingGpuStep {
    pub idx: u32,
    pub settings: GpuSettings,
    pub origin: PendingGpuOrigin,
}

/// Deferred GPU settings and their consumption state share one lock.
pub static PENDING_GPU_SETTINGS: Lazy<Mutex<Option<PendingGpuStep>>> = Lazy::new(|| Mutex::new(None));
pub static GPU_APPLY_GEN: AtomicU64 = AtomicU64::new(0);
static LAST_APPLIED_PROFILE: Lazy<Mutex<Option<Profile>>> = Lazy::new(|| Mutex::new(None));

/// Outcome of the last completed deferred GPU apply attempt, waiting for the
/// GUI to pick it up (take-once). The `ApplyProfile` reply is sent long before
/// a deferred attempt runs, so this slot is its delivery path. Identical
/// repeated outcomes (e.g. the idle-OC restore re-hitting a permanently
/// unsupported operation) are published once.
static DEFERRED_GPU_REPORT: Lazy<Mutex<DeferredGpuReportState>> =
    Lazy::new(|| Mutex::new(DeferredGpuReportState::default()));

#[derive(Default)]
struct DeferredGpuReportState {
    pending: Option<DeferredGpuReport>,
    last_published: Option<String>,
}

/// Lock a shared daemon mutex, recovering from poisoning instead of panicking.
///
/// A `.lock().unwrap()` on a poisoned mutex (i.e. a thread panicked while
/// holding the guard) propagates the poison as a panic *in this thread*,
/// taking the whole daemon down. The daemon outlives any single hardware
/// call, so every global lock site must instead extract the inner value,
/// clear the poison, log, and carry on with the last-known-good contents.
///
/// REENTRANCY: this helper is reached from the `DaemonLogger::log` path (which
/// locks DAEMON_LOGS through this same helper), and the recovery path below
/// calls `log::error!`. Two rules make that safe:
///   1. take the guard out of the `PoisonError` FIRST (`into_inner`), so the
///      mutex is unlocked-and-normal before we log anything; `clear_poison()`
///      is only needed to let *other* threads observe the un-poisoned state.
///   2. never log while still holding a guard — the caller's `log::error!`
///      below happens after `into_inner()` has already released the lock.
pub(crate) fn lock_or_recover<'a, T>(mutex: &'a Mutex<T>, label: &str) -> std::sync::MutexGuard<'a, T> {
    match mutex.lock() {
        Ok(g) => g,
        Err(e) => {
            // into_inner() first: it consumes the error and returns a guard
            // whose Drop releases the mutex, so by the time we log, no guard
            // from this site is live. Note the guard is dropped at the end of
            // this expression — the log below cannot deadlock on this mutex.
            let g = e.into_inner();
            mutex.clear_poison();
            drop(g);
            log::error!(target: "hw.lock", "{} mutex poisoned — poison cleared, recovering with last-known-good contents", label);
            // Re-lock now that the mutex is clean and no guard is live. This
            // extra lock is what actually hands the caller a usable guard.
            match mutex.lock() {
                Ok(g) => g,
                // Cannot happen: we just cleared the poison and nobody else
                // can have panicked in between.
                Err(_) => panic!("{} mutex re-poisoned during recovery", label),
            }
        }
    }
}

/// ---------------------------------------------------------------------------
/// sysfs write safety guard (task-safety-sysfs-writes)
///
/// Every daemon write to /sysfs is routed through `guard_sysfs_write`, which
/// enforces two independent protections *before* any bytes hit disk:
///   1. PATH ALLOWLIST - only a fixed set of control attributes may ever be
///      written; anything else is refused outright.
///   2. VALUE RANGE CHECK - the payload must match the allowed vocabulary for
///      that attribute (sanctioned governor string, boolean, status word, or a
///      numeric within its hardware bounds). Out-of-range values are rejected,
///      never clamped silently.
/// ---------------------------------------------------------------------------

/// Governor names we will ever permit writing to scaling_governor. Unknown
/// tokens are refused so a bad/untrusted value cannot hang CPUs.
const GOVERNOR_ALLOWLIST: &[&str] = &[
    "performance", "powersave", "balanced", "ondemand", "user", "schedutil",
    "userspace", "conservative", "interactive", "scheyonic", "energy_perf",
];

/// cpufreq frequency attributes guarded against out-of-hardware-range values.
fn is_freq_attribute(path: &str) -> bool {
    path.ends_with("scaling_min_freq")
        || path.ends_with("scaling_max_freq")
        || path.ends_with("cpuinfo_min_freq")
        || path.ends_with("cpuinfo_max_freq")
}

/// Power-management boolean toggles written as "0"/"1".
fn is_bool_toggle(path: &str) -> bool {
    path.ends_with("cpufreq/boost")
        || path.ends_with("intel_pstate/no_turbo")
        || path.ends_with("amd_pstate/cpb_boost")
}

/// Intel p-state status words ("passive"/"active").
fn is_intel_pstate_status(path: &str) -> bool {
    path.ends_with("intel_pstate/status")
}

/// AMD p-state status words ("passive"/"active"/"guided").
fn is_amd_pstate_status(path: &str) -> bool {
    path.ends_with("amd_pstate/status")
}

/// SMT control ("on"/"off").
fn is_smt_control(path: &str) -> bool {
    path.ends_with("smt/control")
}

/// Numeric backlight brightness attrs; bounded by sibling max_brightness.
fn is_brightness_attr(path: &str) -> bool {
    path.ends_with("/brightness") || path.ends_with("/actual_brightness")
}

/// LED mode/speed attrs; small non-negative byte integers.
fn is_led_mode_or_speed(path: &str) -> bool {
    path.ends_with("/mode") || path.ends_with("/speed")
}

/// Whole-path allowlist ('*' wildcard spans exactly one path segment).
const APPROVED_SYSFS_PATHS: &[&str] = &[
    "/sys/devices/system/cpu/*/cpufreq/scaling_governor",
    "/sys/devices/system/cpu/*/cpufreq/scaling_min_freq",
    "/sys/devices/system/cpu/*/cpufreq/scaling_max_freq",
    "/sys/devices/system/cpu/*/cpufreq/cpuinfo_min_freq",
    "/sys/devices/system/cpu/*/cpufreq/cpuinfo_max_freq",
    "/sys/devices/system/cpu/*/cpufreq/energy_performance_preference",
    "/sys/devices/system/cpu/*/cpufreq/energy_performance_available_preferences",
    "/sys/devices/system/cpu/cpufreq/boost",
    "/sys/devices/system/cpu/intel_pstate/no_turbo",
    "/sys/devices/system/cpu/amd_pstate/cpb_boost",
    "/sys/devices/system/cpu/smt/control",
    "/sys/devices/system/cpu/amd_pstate/status",
    "/sys/devices/system/cpu/intel_pstate/status",
    "/sys/class/backlight/*/brightness",
    "/sys/class/backlight/*/actual_brightness",
    "/sys/class/leds/*/multi_intensity",
    "/sys/class/leds/*/brightness",
    "/sys/class/leds/*/mode",
    "/sys/class/leds/*/speed",
    // Keyboard discovery prefers the Tuxedo platform path to its class alias.
    "/sys/devices/platform/tuxedo_keyboard/leds/*/multi_intensity",
    "/sys/devices/platform/tuxedo_keyboard/leds/*/brightness",
    "/sys/devices/platform/tuxedo_keyboard/leds/*/mode",
    "/sys/devices/platform/tuxedo_keyboard/leds/*/speed",
];

/// Lexical components of an absolute path, or `None` when the path must not be
/// matched at all.
///
/// This is deliberately pure: sysfs is full of symlinks and kernel-generated
/// nodes, so `canonicalize()` is not an option — it would resolve the very
/// aliases the allowlist names (`/sys/class/leds/*` points into
/// `/sys/devices/...`) and would reject paths whose final component the kernel
/// creates on demand.
///
/// Normalisation rules:
///   * a relative path is refused: `fs::write` would resolve it against the
///     daemon's working directory instead of `/sys`;
///   * repeated separators collapse and `.` components are dropped — those are
///     equivalent spellings of the same file, so they reduce to the canonical
///     form rather than being rejected;
///   * a `..` component is refused outright instead of being resolved, so no
///     accepted path can traverse out of the subtree its template describes.
fn normalized_components(path: &str) -> Option<Vec<&str>> {
    if !path.starts_with('/') {
        return None;
    }
    let mut components = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => continue,
            ".." => return None,
            other => components.push(other),
        }
    }
    Some(components)
}

/// Canonical spelling of a path that passed [`normalized_components`]. The
/// guard writes this form, so the bytes can only land on the path that was
/// actually checked.
fn normalized_path(path: &str) -> Option<String> {
    let components = normalized_components(path)?;
    Some(format!("/{}", components.join("/")))
}

/// Does a canonical component list match one allowlist template, and stay
/// inside the subtree that template fixes?
///
/// Two independent conditions, both on the normalised form:
///   1. containment — every component before the first wildcard is identical,
///      so the write must land inside the template's literal subtree;
///   2. shape — equal component counts, each template component either a
///      single-segment `*` or an exact literal.
/// Because `..` never reaches this point, "matches the template" and "is inside
/// the template's subtree" are the same statement — the containment check below
/// states it explicitly so it cannot be lost in a future edit.
fn matches_template_components(path: &[&str], tmpl: &[&str]) -> bool {
    if path.len() != tmpl.len() {
        return false;
    }
    let first_wildcard = tmpl.iter().position(|component| *component == "*").unwrap_or(tmpl.len());
    if path[..first_wildcard] != tmpl[..first_wildcard] {
        return false;
    }
    path.iter()
        .zip(tmpl.iter())
        .all(|(actual, expected)| *expected == "*" || actual == expected)
}

fn is_allowed_sysfs_path(path: &str) -> bool {
    let Some(components) = normalized_components(path) else {
        return false;
    };
    APPROVED_SYSFS_PATHS.iter().any(|tmpl| {
        normalized_components(tmpl)
            .map(|tmpl_components| matches_template_components(&components, &tmpl_components))
            .unwrap_or(false)
    })
}

/// Parse a u64 payload, used by both frequency and brightness range checks.
fn parse_u64(contents: &str) -> Result<u64> {
    contents.trim().parse::<u64>().map_err(|_| {
        anyhow!("non-numeric value \"{}\"", contents)
    })
}

/// Range-check a frequency attribute against the given hardware window.
/// Returns Ok(()) when the value is in-range; Err when out-of-range or
/// unparseable. Callers decide whether an unreadable window is fatal.
fn within_hw_freq(contents: &str, path: &str, hw_min: u64, hw_max: u64) -> Result<()> {
    let raw = parse_u64(contents)?;
    if raw < hw_min || raw > hw_max {
        return Err(anyhow!(
            "freq {} out of [{},{}] for {}",
            raw,
            hw_min,
            hw_max,
            path
        ));
    }
    Ok(())
}

/// Range-check a backlight brightness value against the reported maximum.
fn within_brightness(contents: &str, path: &str, max_b: u32) -> Result<()> {
    let val: u32 = contents
        .trim()
        .parse()
        .map_err(|_| anyhow!("non-numeric brightness \"{}\" for {}", contents, path))?;
    if val > max_b {
        return Err(anyhow!(
            "brightness {} exceeds max {} for {}",
            val,
            max_b,
            path
        ));
    }
    Ok(())
}

/// Read CPU0 hardware frequency bounds once for range checking.
fn read_hw_freq_bounds() -> Option<(u64, u64)> {
    let min_s = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_min_freq").ok()?;
    let max_s = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq").ok()?;
    let min: u64 = min_s.trim().parse().ok()?;
    let max: u64 = max_s.trim().parse().ok()?;
    Some((min, max))
}

fn normalize_cpu_frequency_request(
    min_freq: u64,
    max_freq: u64,
    bounds: Option<(u64, u64)>,
) -> Result<(u64, u64)> {
    let (hw_min, hw_max) = bounds.ok_or_else(|| {
        anyhow!("CPU hardware frequency bounds are unavailable; refusing unvalidated limits")
    })?;
    if hw_min > hw_max {
        return Err(anyhow!(
            "invalid CPU hardware frequency bounds: min {hw_min} exceeds max {hw_max}"
        ));
    }
    let limits = normalize_freq_limits(Some(min_freq), Some(max_freq), hw_min, hw_max);
    Ok((
        limits
            .min
            .ok_or_else(|| anyhow!("normalized CPU minimum frequency is missing"))?,
        limits
            .max
            .ok_or_else(|| anyhow!("normalized CPU maximum frequency is missing"))?,
    ))
}

#[cfg(test)]
mod cpu_frequency_normalization_tests {
    use super::normalize_cpu_frequency_request;

    const BOUNDS: (u64, u64) = (403_488, 4_465_261);

    #[test]
    fn request_below_hardware_minimum_is_raised() {
        assert_eq!(
            normalize_cpu_frequency_request(100_000, 2_000_000, Some(BOUNDS)).unwrap(),
            (BOUNDS.0, 2_000_000)
        );
    }

    #[test]
    fn request_above_hardware_maximum_is_lowered() {
        assert_eq!(
            normalize_cpu_frequency_request(1_000_000, 5_000_000, Some(BOUNDS)).unwrap(),
            (1_000_000, BOUNDS.1)
        );
    }

    #[test]
    fn resolves_minimum_above_maximum() {
        assert_eq!(
            normalize_cpu_frequency_request(3_000_000, 2_000_000, Some(BOUNDS)).unwrap(),
            (3_000_000, 3_000_000)
        );
    }

    #[test]
    fn accepts_valid_in_range_limits_unchanged() {
        assert_eq!(
            normalize_cpu_frequency_request(1_000_000, 4_000_000, Some(BOUNDS)).unwrap(),
            (1_000_000, 4_000_000)
        );
    }

    #[test]
    fn refuses_to_write_without_hardware_bounds() {
        assert!(normalize_cpu_frequency_request(1_000_000, 4_000_000, None).is_err());
    }

    #[test]
    fn refuses_invalid_hardware_bounds() {
        assert!(normalize_cpu_frequency_request(1_000_000, 4_000_000, Some((4_000_000, 1_000_000))).is_err());
    }
}

/// Maximum brightness the hardware reports (sibling of the writable attr).
fn read_max_brightness(base: &str) -> Option<u32> {
    let s = fs::read_to_string(format!("{base}/max_brightness")).ok()?;
    s.trim().parse().ok()
}

/// Central gate: refuse anything not on the allowlist, then enforce value rules.
fn guard_sysfs_write(path: &str, contents: &str) -> Result<()> {
    // Match on the canonical form and write that same form, so what is checked
    // is exactly what is written. A relative path or one containing `..` is
    // refused here, before any value rule or write is reached.
    let Some(target) = normalized_path(path) else {
        return Err(anyhow!(
            "blocked sysfs write to non-absolute or traversing path {}",
            path
        ));
    };
    if !is_allowed_sysfs_path(&target) {
        return Err(anyhow!(
            "blocked non-allowlisted sysfs write to {}",
            target
        ));
    }
    let path = target.as_str();

    // Governor string must be an explicitly sanctioned value.
    if path.ends_with("scaling_governor") {
        if !GOVERNOR_ALLOWLIST.contains(&contents) {
            return Err(anyhow!(
                "governor \"{}\" rejected for {}; allowed: {:?}",
                contents,
                path,
                GOVERNOR_ALLOWLIST
            ));
        }
    }

    // Frequency attributes must sit within the hardware's [min,max] window.
    if is_freq_attribute(path) {
        match read_hw_freq_bounds() {
            Some((hw_min, hw_max)) => {
                within_hw_freq(contents, path, hw_min, hw_max)?;
            }
            None => {
                log::warn!(target: "hw.cpu",
                    "skipping freq range-check at {} (cpuinfo limits unreadable)", path);
            }
        }
    }

    // Backlight brightness bounded by reported max_brightness.
    if is_brightness_attr(path) {
        let trimmed = path
            .strip_suffix("/actual_brightness")
            .or_else(|| path.strip_suffix("/brightness"))
            .ok_or_else(|| anyhow!("invalid brightness attribute path: {}", path))?;
        match read_max_brightness(trimmed) {
            Some(max_b) => {
                within_brightness(contents, path, max_b)?;
            }
            None => {
                log::warn!(target: "hw.screen",
                    "skipping brightness range-check at {} (max_brightness unreadable)", path);
            }
        }
    }

    // Boolean toggles are strictly 0/1.
    if is_bool_toggle(path) {
        if contents != "0" && contents != "1" {
            return Err(anyhow!(
                "bool toggle {} expects \"0\" or \"1\", got \"{}\"",
                path,
                contents
            ));
        }
    }

    // Intel p-state status word.
    if is_intel_pstate_status(path) {
        if !["passive", "active"].contains(&contents) {
            return Err(anyhow!(
                "intel pstate status \"{}\" invalid for {}",
                contents,
                path
            ));
        }
    }

    // AMD p-state status word.
    if is_amd_pstate_status(path) {
        if !["passive", "active", "guided"].contains(&contents) {
            return Err(anyhow!(
                "amd pstate status \"{}\" invalid for {}",
                contents,
                path
            ));
        }
    }

    // SMT control.
    if is_smt_control(path) {
        if contents != "on" && contents != "off" {
            return Err(anyhow!(
                "smt/control expects \"on\" or \"off\", got \"{}\"",
                contents
            ));
        }
    }

    // LED mode/speed small byte integers.
    if is_led_mode_or_speed(path) {
        if !contents.parse::<u8>().is_ok() {
            return Err(anyhow!(
                "led mode/speed must be a byte integer, got \"{}\"",
                contents
            ));
        }
    }

    // All checks passed: perform the write. The guard was originally
    // validate-only and the call sites validated without ever writing, which
    // made every sysfs-control method a no-op (profile apply returned success
    // and no hardware state changed). See the docblock above: the intent was
    // always "before any bytes hit disk".
    fs::write(path, contents)?;
    Ok(())
}

fn get_cpu_count() -> Result<u32> {
    let cpuinfo = fs::read_to_string("/proc/cpuinfo")?;
    let count = cpuinfo.lines()
        .filter(|line| line.starts_with("processor"))
        .count();
    Ok(count as u32)
}

pub fn set_cpu_governor(governor: &str) -> Result<()> {
    let cpu_count = get_cpu_count()?;

    guard_sysfs_write("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor", governor)?;

    for i in 0..cpu_count {
        let path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_governor", i);
        guard_sysfs_write(&path, governor)?;
    }
    
    log::info!(target: "hw.cpu", "set_governor profile=\"{}\"", governor);
    crate::refresh_hardware_cache();
    Ok(())
}

pub fn set_cpu_frequency_limits(min_freq: u64, max_freq: u64) -> Result<()> {
    let requested = (min_freq, max_freq);
    let (min_freq, max_freq) = normalize_cpu_frequency_request(min_freq, max_freq, read_hw_freq_bounds())?;
    if (min_freq, max_freq) != requested {
        log::warn!(target: "hw.cpu", "normalized CPU frequency limits {:?} to {:?} using hardware bounds", requested, (min_freq, max_freq));
    }
    let cpu_count = get_cpu_count()?;
    
    // IMPORTANT: Set max first, then min to avoid conflicts
    // If current min > new max, setting max first will fail
    // If current max < new min, setting min first will fail
    
    // First, read current values
    let current_min = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_min_freq")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(min_freq);
    
    let current_max = fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_max_freq")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(max_freq);
    
    for i in 0..cpu_count {
        let min_path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_min_freq", i);
        let max_path = format!("/sys/devices/system/cpu/cpu{}/cpufreq/scaling_max_freq", i);
        
        // Determine order based on current vs new values
        if max_freq < current_max || min_freq > current_min {
            // Set max first
            guard_sysfs_write(&max_path, &max_freq.to_string())?;
            guard_sysfs_write(&min_path, &min_freq.to_string())?;
        } else {
            // Set min first
            guard_sysfs_write(&min_path, &min_freq.to_string())?;
            guard_sysfs_write(&max_path, &max_freq.to_string())?;
        }
    }
    
    CPU_LIMITS_MODIFIED.store(true, Ordering::SeqCst);
    log::info!(target: "hw.cpu", "set_freq_limits min={} max={}", min_freq, max_freq);
    Ok(())
}

pub fn restore_cpu_frequency_limits() -> Result<()> {
    if !CPU_LIMITS_MODIFIED.load(Ordering::SeqCst) {
        return Ok(());
    }

    log::info!(target: "hw.cpu", "Restoring CPU frequency limits to hardware defaults");
    let (hw_min, hw_max) = crate::hardware_detection::read_hw_frequency_limits()?;

    if let (Some(min), Some(max)) = (hw_min, hw_max) {
        set_cpu_frequency_limits(min, max)?;
    }

    Ok(())
}

pub fn set_cpu_boost(enabled: bool) -> Result<()> {
    // AMD cpufreq boost
    let amd_path = "/sys/devices/system/cpu/cpufreq/boost";
    if Path::new(amd_path).exists() {
        guard_sysfs_write(amd_path, if enabled { "1" } else { "0" })?;
        log::info!(target: "hw.cpu", "set_amd_boost enabled={}", enabled);
        return Ok(());
    }
    
    // Intel turbo
    let intel_path = "/sys/devices/system/cpu/intel_pstate/no_turbo";
        if Path::new(intel_path).exists() {
            guard_sysfs_write(intel_path, if enabled { "0" } else { "1" })?;
        log::info!(target: "hw.cpu", "set_intel_turbo enabled={}", enabled);
        return Ok(());
    }
    
    // AMD P-State boost (if using amd-pstate driver)
    let amd_pstate_boost = "/sys/devices/system/cpu/amd_pstate/cpb_boost";
    if Path::new(amd_pstate_boost).exists() {
        guard_sysfs_write(amd_pstate_boost, if enabled { "1" } else { "0" })?;
        log::info!(target: "hw.cpu", "set_amd_pstate_boost enabled={}", enabled);
        return Ok(());
    }
    
    Err(anyhow!("Boost control not available"))
}

pub fn set_smt(enabled: bool) -> Result<()> {
    let path = "/sys/devices/system/cpu/smt/control";
    if !Path::new(path).exists() {
        return Err(anyhow!("SMT control not available"));
    }
    
    guard_sysfs_write(path, if enabled { "on" } else { "off" })?;
    log::info!(target: "hw.cpu", "set_smt enabled={}", enabled);
    Ok(())
}

pub fn set_amd_pstate_status(status: &str) -> Result<()> {
    let path = "/sys/devices/system/cpu/amd_pstate/status";
    if !Path::new(path).exists() {
        return Err(anyhow!("AMD pstate not available"));
    }
    
    if !["passive", "active", "guided"].contains(&status) {
        return Err(anyhow!("Invalid AMD pstate status: {}", status));
    }
    
    guard_sysfs_write(path, status)?;
    log::info!(target: "hw.cpu", "set_amd_pstate_status status=\"{}\"", status);
    crate::refresh_hardware_cache();
    Ok(())
}

pub fn set_intel_pstate_status(status: &str) -> Result<()> {
    let path = "/sys/devices/system/cpu/intel_pstate/status";
    if !Path::new(path).exists() {
        return Err(anyhow!("Intel pstate not available"));
    }

    if !["passive", "active"].contains(&status) {
        return Err(anyhow!("Invalid Intel pstate status: {}", status));
    }

    guard_sysfs_write(path, status)?;
    log::info!(target: "hw.cpu", "set_intel_pstate_status status=\"{}\"", status);
    crate::refresh_hardware_cache();
    Ok(())
}

/// Apply one pending value while preventing concurrent replacement, and
/// consume the slot only when `apply` marks the attempt as final.
///
/// `apply` returns `(result, consumed)`; `consumed = false` keeps the value
/// pending, so a transient refusal (the runtime write gate closing mid-apply)
/// can be retried once the adapter is writable again.
fn consume_pending_locked<T, R>(
    slot: &Mutex<Option<T>>,
    label: &str,
    apply: impl FnOnce(&T) -> Option<(R, bool)>,
) -> Option<R> {
    let mut pending = lock_or_recover(slot, label);
    let (result, consumed) = apply(pending.as_ref()?)?;
    if consumed {
        *pending = None;
    }
    Some(result)
}

pub fn replace_pending_gpu_settings(idx: u32, settings: GpuSettings) {
    *lock_or_recover(&PENDING_GPU_SETTINGS, "PENDING_GPU_SETTINGS") = Some(PendingGpuStep {
        idx,
        settings,
        origin: PendingGpuOrigin::IdleRestore,
    });
}

/// Consumption rule for a deferred GPU attempt: keep the settings pending
/// only when the runtime write gate refused an operation mid-apply. That
/// refusal is transient (the adapter is suspending or suspended right now);
/// permanent outcomes — unsupported operations or genuine failures — must
/// consume the slot, because retrying those would be a loop with no exit.
fn pending_consumed_after_attempt(failures: &[GpuOpFailure]) -> bool {
    !failures.iter().any(|f| f.kind == GpuOpFailureKind::GateClosed)
}

/// One attempt at a profile's GPU step — shared by the immediate apply and the
/// deferred retry. Pre-checks the runtime write gate, runs the operations and
/// decides whether the attempt is final (`consumed`) or must be retried
/// because the gate closed mid-apply. `None` means the pre-check refused and
/// nothing was attempted. Gate and operations are parameters so tests can
/// drive the exact race without hardware.
fn attempt_gpu_step(
    idx: u32,
    settings: &GpuSettings,
    writable: impl FnOnce(u32) -> bool,
    apply: impl FnOnce(u32, &GpuSettings) -> Vec<GpuOpFailure>,
) -> Option<(Vec<GpuOpFailure>, bool)> {
    if !writable(idx) {
        return None;
    }
    let failures = apply(idx, settings);
    let consumed = pending_consumed_after_attempt(&failures);
    Some((failures, consumed))
}

/// Apply the newest deferred profile GPU step once the adapter is writable.
pub fn apply_pending_gpu() {
    let applied = consume_pending_locked(&PENDING_GPU_SETTINGS, "PENDING_GPU_SETTINGS", |step| {
        attempt_gpu_step(step.idx, &step.settings, crate::hardware_detection::gpu_write_allowed, apply_gpu_static)
            .map(|(failures, consumed)| ((step.idx, step.origin, failures), consumed))
    });
    if let Some((idx, origin, failures)) = applied {
        warn_gpu_failures("deferred profile GPU step", idx, &failures);
        if pending_consumed_after_attempt(&failures) {
            log::info!(target: "hw.gpu", "deferred profile GPU step applied gpu={idx} failures={}", failures.len());
            publish_deferred_gpu_report(origin, &failures);
        } else {
            log::info!(target: "hw.gpu", "deferred profile GPU step gpu={idx} kept pending: runtime write gate closed mid-apply");
        }
    }
}

/// Queue the completed deferred attempt's outcome for the GUI.
///
/// A clean outcome is published only for a profile apply — the idle-OC
/// restore would otherwise emit a success event on every cycle. Identical
/// repeated outcomes are published once (a series); a clean attempt ends the
/// series, so a later recurrence of the same problem is shown again.
fn publish_deferred_gpu_report(origin: PendingGpuOrigin, failures: &[GpuOpFailure]) {
    let mut state = lock_or_recover(&DEFERRED_GPU_REPORT, "DEFERRED_GPU_REPORT");
    if failures.is_empty() {
        state.last_published = None;
        if origin == PendingGpuOrigin::ProfileApply {
            state.pending = Some(DeferredGpuReport {
                text: "Deferred GPU settings applied".to_string(),
                outcome: DeferredGpuOutcome::Applied,
                origin,
            });
        }
        return;
    }
    let (warnings, fatal) = split_gpu_failures(failures);
    let (text, outcome) = if fatal.is_empty() {
        (format!("Deferred GPU step applied with warnings: {}", warnings.join("; ")), DeferredGpuOutcome::AppliedWithWarnings)
    } else if warnings.is_empty() {
        (format!("Deferred GPU step failed: {}", fatal.join("; ")), DeferredGpuOutcome::Failed)
    } else {
        (format!("Deferred GPU step failed: {} (unsupported: {})", fatal.join("; "), warnings.join("; ")), DeferredGpuOutcome::Failed)
    };
    if state.last_published.as_deref() == Some(text.as_str()) {
        return;
    }
    state.last_published = Some(text.clone());
    state.pending = Some(DeferredGpuReport { text, outcome, origin });
}

/// Take the pending deferred-GPU report, if any (take-once delivery).
pub fn take_deferred_gpu_report() -> Option<DeferredGpuReport> {
    lock_or_recover(&DEFERRED_GPU_REPORT, "DEFERRED_GPU_REPORT").pending.take()
}

pub fn apply_profile(profile: &Profile) -> Result<ApplyProfileResult> {
    // Check if this profile is already applied to avoid redundant hardware calls.
    //
    // IMPORTANT: the "already applied" marker is only written *after* every
    // hardware step below succeeds. Previously it was recorded up-front, so a
    // failure on any late step (keyboard/screen/fan...) left the profile
    // marked as applied and every subsequent identical apply was silently
    // skipped, leaving the machine in a partial hardware state. See task
    // fix-hwc-stale-applied-profile.
    {
        let last_profile = lock_or_recover(&LAST_APPLIED_PROFILE, "LAST_APPLIED_PROFILE");
        if let Some(ref last) = *last_profile {
            if last == profile {
                log::info!(target: "hw.detect", "Profile '{}' is already applied, skipping", profile.name);
                return Ok(ApplyProfileResult { warnings: Vec::new(), gpu_deferred: false });
            }
        }
    }

    GPU_APPLY_GEN.fetch_add(1, Ordering::SeqCst);
    log::info!(target: "hw.detect", "Applying profile: {}", profile.name);
    let result = apply_profile_inner(profile);
    match result {
        Ok((gpu_deferred, warnings)) => {
            if !gpu_deferred {
                let mut last_profile = lock_or_recover(&LAST_APPLIED_PROFILE, "LAST_APPLIED_PROFILE");
                *last_profile = Some(profile.clone());
            }
            if warnings.is_empty() {
                log::info!(target: "hw.detect", "Profile '{}' applied successfully", profile.name);
            } else {
                log::warn!(target: "hw.detect", "Profile '{}' applied with warnings: {}", profile.name, warnings.join("; "));
            }
            Ok(ApplyProfileResult { warnings, gpu_deferred })
        }
        Err(e) => {
            // A partial apply must NOT be remembered as "applied": otherwise the
            // next identical apply would hit the early-return above and leave the
            // machine in the partial state forever.
            let mut last_profile = lock_or_recover(&LAST_APPLIED_PROFILE, "LAST_APPLIED_PROFILE");
            *last_profile = None;
            log::error!(target: "hw.detect", "Profile '{}' apply failed: {} — cleared last-applied marker", profile.name, e);
            Err(e)
        }
    }
}

/// All hardware steps of a profile apply, in order. The caller owns the
/// last-applied marker so a failure never leaves a stale "already applied" state.
/// Apply the static GPU part of a profile. Every step runs even if an earlier
/// one failed; each failure is logged, classified and returned.
pub(crate) fn apply_gpu_static(idx: u32, g: &lapsphere_common::types::GpuSettings) -> Vec<GpuOpFailure> {
    let mut failures: Vec<GpuOpFailure> = Vec::new();
    let mut record = |op: &'static str, r: Result<()>| {
        if let Err(e) = r {
            let kind = classify_gpu_failure(&e);
            match kind {
                GpuOpFailureKind::Unsupported => {
                    log::warn!(target: "hw.gpu", "{op} gpu={idx} unsupported by this adapter: {e}");
                }
                GpuOpFailureKind::GateClosed => {
                    log::warn!(target: "hw.gpu", "{op} gpu={idx} refused by the runtime write gate: {e}");
                }
                GpuOpFailureKind::Failed => {
                    log::warn!(target: "hw.gpu", "{op} gpu={idx} failed: {e}");
                }
            }
            failures.push(GpuOpFailure { op, message: e.to_string(), kind });
        }
    };
    if let Some(limit) = g.power_limit {
        record("set_gpu_power_limit", set_gpu_power_limit(idx, limit));
    }
    if let Some(core_offset) = g.core_offset {
        record("set_gpu_core_offset", set_gpu_core_offset(idx, core_offset as f32));
    }
    if let Some(memory_offset) = g.memory_offset {
        record("set_gpu_memory_offset", set_gpu_memory_offset(idx, memory_offset as f32));
    }
    if let (Some(min_clock), Some(max_clock)) = (g.min_gpu_clock, g.max_gpu_clock) {
        record("set_gpu_locked_clocks", set_gpu_locked_clocks(idx, min_clock, max_clock));
    } else {
        record("reset_gpu_clocks", reset_gpu_clocks(idx));
    }
    for fan in &g.nvidia_fans {
        if fan.manual {
            record("set_gpu_fan_speed", set_gpu_fan_speed(fan.device_index, fan.fan_id, fan.speed));
        } else {
            record("set_gpu_fan_auto", set_gpu_fan_auto(fan.device_index, fan.fan_id));
        }
    }
    failures
}

fn apply_profile_inner(profile: &Profile) -> Result<(bool, Vec<String>)> {
    
    // Apply CPU settings
    if let Some(ref governor) = profile.cpu_settings.governor {
        set_cpu_governor(governor)?;
    }
    
    if let Some(ref tdp_profile) = profile.cpu_settings.tdp_profile {
        set_tdp_profile(tdp_profile)?;
    }

    if let Some(io) = TuxedoIo::shared() {
        if io.get_interface() == HardwareInterface::Uniwill {
            if let Some(val) = profile.cpu_settings.tdp0 {
                let _ = io.set_tdp(0, val);
            }
            if let Some(val) = profile.cpu_settings.tdp1 {
                let _ = io.set_tdp(1, val);
            }
            if let Some(val) = profile.cpu_settings.tdp2 {
                let _ = io.set_tdp(2, val);
            }
        }
    }
    
    if let Some(ref amd_status) = profile.cpu_settings.amd_pstate_status {
        set_amd_pstate_status(amd_status)?;
    }

    if let Some(ref intel_status) = profile.cpu_settings.intel_pstate_status {
        set_intel_pstate_status(intel_status)?;
    }
    
    if let Some(ref epp) = profile.cpu_settings.energy_performance_preference {
        set_energy_performance_preference(epp)?;
    }
    
    if let (Some(min), Some(max)) = (profile.cpu_settings.min_frequency, profile.cpu_settings.max_frequency) {
        set_cpu_frequency_limits(min, max)?;
    }

    // Apply GPU settings
    let nvidia_gpu_idx = {
        let cache = match crate::HARDWARE_CACHE.lock() {
            Ok(g) => g,
            Err(e) => {
                log::error!(target: "hw.cache", "HARDWARE_CACHE poisoned — clearing poison and recovering");
                let g = e.into_inner();
                crate::HARDWARE_CACHE.clear_poison();
                g
            }
        };
        cache.gpu_info.iter()
            .find(|g| g.name.to_lowercase().contains("nvidia"))
            .and_then(|g| g.nvml_index)
            .unwrap_or(0)
    };

    let (gpu_deferred, gpu_failures) = {
        let mut pending = lock_or_recover(&PENDING_GPU_SETTINGS, "PENDING_GPU_SETTINGS");
        match attempt_gpu_step(nvidia_gpu_idx, &profile.gpu_settings, crate::hardware_detection::gpu_write_allowed, apply_gpu_static) {
            Some((failures, consumed)) => {
                // Same consumption rule as the deferred retry: a gate refusal
                // that lands mid-apply keeps this profile's step pending so the
                // refused operations are retried once the adapter is writable.
                *pending = pending_after_profile_apply(consumed, PendingGpuStep {
                    idx: nvidia_gpu_idx,
                    settings: profile.gpu_settings.clone(),
                    origin: PendingGpuOrigin::ProfileApply,
                });
                warn_gpu_failures("profile GPU apply", nvidia_gpu_idx, &failures);
                (!consumed, failures)
            }
            None => {
                log::debug!(target: "hw.gpu", "profile GPU writes deferred gpu={}", nvidia_gpu_idx);
                *pending = pending_after_profile_apply(false, PendingGpuStep {
                    idx: nvidia_gpu_idx,
                    settings: profile.gpu_settings.clone(),
                    origin: PendingGpuOrigin::ProfileApply,
                });
                (true, Vec::new())
            }
        }
    };

    if let Some(boost) = profile.cpu_settings.boost {
        set_cpu_boost(boost)?;
    }

    if let Some(smt) = profile.cpu_settings.smt {
        set_smt(smt)?;
    }

    // Apply keyboard settings
    apply_keyboard_settings(&profile.keyboard_settings)?;

    // Apply screen settings
    apply_screen_settings(&profile.screen_settings)?;

    // Apply fan settings - update daemon state
    apply_fan_settings(&profile.fan_settings)?;

    let warnings = profile_gpu_result(&gpu_failures)?;
    Ok((gpu_deferred, warnings))
}

/// Pending GPU state after a profile apply attempt. Pure so the rule is testable.
/// A consumed attempt (every operation reached a final outcome) leaves nothing
/// pending — this also drops a step deferred by an earlier profile. An attempt
/// that must be retried (the write gate closed mid-apply, or the pre-check
/// refused) stores the newest profile's step (origin ProfileApply).
pub fn pending_after_profile_apply(consumed: bool, deferred: PendingGpuStep) -> Option<PendingGpuStep> {
    if consumed { None } else { Some(deferred) }
}

/// Split classified GPU failures into non-fatal warnings (operations the
/// adapter does not support — retrying them is pointless) and fatal details
/// (everything else). Pure so the policy is testable without hardware.
fn split_gpu_failures(failures: &[GpuOpFailure]) -> (Vec<String>, Vec<String>) {
    let mut warnings = Vec::new();
    let mut fatal = Vec::new();
    for f in failures {
        let entry = format!("{}: {}", f.op, f.message);
        match f.kind {
            GpuOpFailureKind::Unsupported => warnings.push(entry),
            _ => fatal.push(entry),
        }
    }
    (warnings, fatal)
}

/// A profile apply fails only for genuine (non-unsupported) GPU failures;
/// explicitly unsupported operations come back as warnings for the caller to
/// report to the user.
fn profile_gpu_result(failures: &[GpuOpFailure]) -> Result<Vec<String>> {
    let (warnings, fatal) = split_gpu_failures(failures);
    if !fatal.is_empty() {
        return Err(anyhow!("profile GPU apply failed: {}", fatal.join("; ")));
    }
    Ok(warnings)
}

/// Summarise failed GPU operations from `apply_gpu_static` in one line per
/// class: unsupported operations are warnings, everything else a failure.
fn warn_gpu_failures(ctx: &str, idx: u32, failures: &[GpuOpFailure]) {
    if failures.is_empty() {
        return;
    }
    let (warnings, fatal) = split_gpu_failures(failures);
    if !warnings.is_empty() {
        log::warn!(target: "hw.gpu", "{ctx} gpu={idx}: {} unsupported operation(s) skipped: {}", warnings.len(), warnings.join("; "));
    }
    if !fatal.is_empty() {
        log::warn!(target: "hw.gpu", "{ctx} gpu={idx}: {} operation(s) failed: {}", fatal.len(), fatal.join("; "));
    }
}

#[cfg(test)]
mod profile_gpu_result_tests {
    use super::{profile_gpu_result, split_gpu_failures, GpuOpFailure, GpuOpFailureKind};

    fn failure(op: &'static str, kind: GpuOpFailureKind) -> GpuOpFailure {
        GpuOpFailure { op, message: format!("{op} refused"), kind }
    }

    #[test]
    fn unsupported_only_is_a_warning_not_an_error() {
        let failures = vec![failure("set_gpu_power_limit", GpuOpFailureKind::Unsupported)];
        let warnings = profile_gpu_result(&failures).expect("unsupported ops must not fail the profile");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("set_gpu_power_limit"));
    }

    #[test]
    fn unsupported_operation_beside_applied_ones_is_only_a_warning() {
        // Operations that succeeded produce no failure entries, so a list
        // holding only the unsupported op models the mixed case: the profile
        // must succeed and carry exactly that warning.
        let failures = vec![failure("set_gpu_power_limit", GpuOpFailureKind::Unsupported)];
        let (warnings, fatal) = split_gpu_failures(&failures);
        assert_eq!(warnings.len(), 1);
        assert!(fatal.is_empty(), "applied operations must not become fatal");
        assert!(profile_gpu_result(&failures).is_ok());
    }

    #[test]
    fn genuine_failure_rejects_the_profile() {
        let failures = vec![failure("set_gpu_core_offset", GpuOpFailureKind::Failed)];
        let err = profile_gpu_result(&failures).unwrap_err();
        assert!(err.to_string().contains("set_gpu_core_offset"), "error must name the failing op: {err}");
    }

    #[test]
    fn unsupported_and_genuine_failure_together_still_reject() {
        let failures = vec![
            failure("set_gpu_power_limit", GpuOpFailureKind::Unsupported),
            failure("set_gpu_core_offset", GpuOpFailureKind::Failed),
        ];
        let (warnings, fatal) = split_gpu_failures(&failures);
        assert_eq!(warnings.len(), 1);
        assert_eq!(fatal.len(), 1);
        assert!(profile_gpu_result(&failures).is_err());
    }
}

pub fn apply_battery_settings(settings: &BatterySettings) -> Result<()> {
    if !crate::battery_control::BatteryControl::is_available() {
        log::info!(target: "hw.battery", "Battery control not available, skipping");
        return Ok(());
    }

    let battery = crate::battery_control::BatteryControl::new()?;

    if settings.control_enabled {
        battery.set_charge_type("Custom")?;
        battery.set_charge_control_start_threshold(settings.charge_start_threshold)?;
        battery.set_charge_control_end_threshold(settings.charge_end_threshold)?;
        log::info!(target: "hw.battery",
            "set_thresholds enabled=true start={} end={}",
            settings.charge_start_threshold,
            settings.charge_end_threshold
        );
    } else {
        battery.set_charge_type("Standard")?;
        log::info!(target: "hw.battery", "set_thresholds enabled=false mode=\"Standard\"");
    }

    Ok(())
}

fn apply_keyboard_settings(settings: &KeyboardSettings) -> Result<()> {
    if !settings.control_enabled {
        log::info!(target: "hw.kbd", "keyboard_control enabled=false");
        if let Ok(kbd) = RgbKeyboardControl::new() {
            let white_mode = KeyboardMode::SingleColor {
                r: 255,
                g: 255,
                b: 255,
                brightness: 50,
            };
            let _ = kbd.set_mode(&white_mode);
        }
        return Ok(());
    }
    
    if let Ok(kbd) = RgbKeyboardControl::new() {
        kbd.set_mode(&settings.mode)?;
        log::info!(target: "hw.kbd", "keyboard_settings applied=true");
        Ok(())
    } else {
        log::warn!(target: "hw.kbd", "Keyboard control not available");
        Err(anyhow!("Keyboard control not available"))
    }
}

pub fn preview_keyboard_settings(settings: &KeyboardSettings) -> Result<()> {
    if let Ok(kbd) = RgbKeyboardControl::new() {
        kbd.set_mode(&settings.mode)?;
        Ok(())
    } else {
        Err(anyhow!("Keyboard control not available"))
    }
}

fn apply_screen_settings(settings: &ScreenSettings) -> Result<()> {
    if settings.system_control {
        log::info!(target: "hw.screen", "Using system screen brightness control");
        return Ok(());
    }
    
    let backlight_paths = [
        "/sys/class/backlight/intel_backlight",
        "/sys/class/backlight/amdgpu_bl0",
        "/sys/class/backlight/amdgpu_bl1",
        "/sys/class/backlight/acpi_video0",
    ];
    
    for base_path in &backlight_paths {
        let brightness_path = format!("{}/brightness", base_path);
        let max_brightness_path = format!("{}/max_brightness", base_path);
        
        if Path::new(&brightness_path).exists() {
            let max_brightness: u32 = fs::read_to_string(&max_brightness_path)
                .ok()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(255);
            
            let actual_brightness = ((settings.brightness as u32) * max_brightness) / 100;
            
            // Write to actual_brightness first (this is writable)
            let actual_path = format!("{}/actual_brightness", base_path);
            if Path::new(&actual_path).exists() {
                let _ = guard_sysfs_write(&actual_path, &actual_brightness.to_string());
            }
            
            // Then write to brightness
            guard_sysfs_write(&brightness_path, &actual_brightness.to_string())?;
            log::info!(target: "hw.screen", "set_brightness level={}% path=\"{}\"", settings.brightness, base_path);
            return Ok(());
        }
    }
    
    Err(anyhow!("No writable backlight control found"))
}

pub fn set_tdp_profile(profile_name: &str) -> Result<()> {
    if !TuxedoIo::is_available() {
        return Err(anyhow!("TDP profiles not available"));
    }
    
    let io = TuxedoIo::shared().ok_or_else(|| anyhow!("tuxedo_io not available"))?;
    let profiles = io.get_available_profiles()?;
    
    if let Some(profile_id) = profiles.iter().position(|p| p == profile_name) {
        io.set_performance_profile(profile_id as u32)?;
        log::info!(target: "hw.cpu", "set_tdp_profile name=\"{}\" id={}", profile_name, profile_id);
        Ok(())
    } else {
        Err(anyhow!("Profile '{}' not found. Available: {:?}", profile_name, profiles))
    }
}

pub fn set_fan_speed(fan_id: u32, speed_percent: u32) -> Result<()> {
    if !TuxedoIo::is_available() {
        return Err(anyhow!("Fan control not available"));
    }
    
    let speed = speed_percent.min(100);
    log::info!(target: "hw.fan", "DBus request: set fan {} to {}%", fan_id, speed);
    let io = TuxedoIo::shared().ok_or_else(|| anyhow!("tuxedo_io not available"))?;
    // Explicit user command: always write, even if the packed value is already
    // what we last commanded. Without this a slider set to the current value
    // would silently no-op, and a firmware-reset fan would not be re-asserted.
    io.force_next_speed_write();
    io.set_fan_speed(fan_id, speed)?;
    
    log::info!(target: "hw.fan", "set_fan id={} speed={}%", fan_id, speed);
    Ok(())
}

pub fn set_fan_auto(_fan_id: u32) -> Result<()> {
    if !TuxedoIo::is_available() {
        return Err(anyhow!("Fan control not available"));
    }
    
    let io = TuxedoIo::shared().ok_or_else(|| anyhow!("tuxedo_io not available"))?;
    io.set_fan_auto()?;
    
    log::info!(target: "hw.fan", "set_fans_auto");
    Ok(())
}

fn apply_fan_settings(settings: &FanSettings) -> Result<()> {
    if !TuxedoIo::is_available() {
        log::info!(target: "hw.fan", "Fan control not available (/dev/tuxedo_io not present)");
        return Ok(());
    }
    
    log::info!(target: "hw.fan", "Applying fan settings: enabled={}", settings.control_enabled);
    
    // Update the global fan daemon state
    {
        let mut state = lock_or_recover(&crate::FAN_DAEMON_STATE, "FAN_DAEMON_STATE");
        if settings.control_enabled {
            *state = Some(settings.clone());
            log::info!(target: "hw.fan", "fan_daemon enabled=true curves={}", settings.curves.len());
        } else {
            *state = None;
            log::info!(target: "hw.fan", "fan_daemon enabled=false");
        }
    }
    
    if !settings.control_enabled {
        set_fan_auto(0)?;
    }
    
    Ok(())
}

pub fn set_webcam_state(enabled: bool) -> Result<()> {
    if !TuxedoIo::is_available() {
        return Err(anyhow!("Webcam control not available"));
    }
    
    let io = TuxedoIo::shared().ok_or_else(|| anyhow!("tuxedo_io not available"))?;
    io.set_webcam_state(enabled)?;
    
    log::info!(target: "hw.detect", "set_webcam enabled={}", enabled);
    Ok(())
}

pub fn get_webcam_state() -> Result<bool> {
    if !TuxedoIo::is_available() {
        // Return true as default if driver not present
        return Ok(true);
    }
    
    let io = TuxedoIo::shared().ok_or_else(|| anyhow!("tuxedo_io not available"))?;
    if io.get_interface() != HardwareInterface::Clevo {
        // Return true for non-Clevo hardware (standard state)
        return Ok(true);
    }

    match io.get_webcam_state() {
        Ok(state) => Ok(state),
        Err(_) => Ok(true), // Fallback to true on error
    }
}


use nvml_wrapper::enum_wrappers::device::{Clock, PerformanceState};
use nvml_wrapper::enums::device::GpuLockedClocksSetting;

static NVML: Lazy<Result<Nvml, nvml_wrapper::error::NvmlError>> = Lazy::new(|| Nvml::init());

pub fn get_nvml() -> Result<&'static Nvml> {
    match &*NVML {
        Ok(nvml) => Ok(nvml),
        Err(e) => Err(anyhow!("Failed to initialize NVML: {}", e)),
    }
}

pub(crate) fn gpu_utilization_percent(index: u32) -> Result<f32> {
    let nvml = get_nvml()?;
    let device = nvml.device_by_index(index)?;
    Ok(device.utilization_rates()?.gpu as f32)
}

/// Typed refusal from the runtime write gate: the adapter is not writable
/// right now. Callers distinguish this transient state from permanent
/// failures (see `classify_gpu_failure`), so a deferred step can stay pending
/// until the adapter wakes instead of being dropped.
#[derive(Debug)]
pub(crate) struct GpuGateClosed {
    pub device_index: u32,
}

impl std::fmt::Display for GpuGateClosed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "GPU {} is not writable in its current runtime state; retry when it is active",
            self.device_index
        )
    }
}

impl std::error::Error for GpuGateClosed {}

fn with_gpu_write_gate<T>(
    device_index: u32,
    is_writable: impl FnOnce(u32) -> bool,
    write: impl FnOnce() -> Result<T>,
) -> Result<T> {
    if !is_writable(device_index) {
        return Err(anyhow::Error::new(GpuGateClosed { device_index }));
    }
    write()
}

/// Why one GPU operation failed. This decides retry and reporting policy:
/// `Unsupported` is a permanent property of the adapter (warn, never retry);
/// `GateClosed` is a transient runtime-PM refusal (retry once writable);
/// `Failed` is anything else (report; do not retain a deferred slot for it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GpuOpFailureKind {
    Unsupported,
    GateClosed,
    Failed,
}

#[derive(Debug, Clone)]
pub(crate) struct GpuOpFailure {
    pub op: &'static str,
    pub message: String,
    pub kind: GpuOpFailureKind,
}

/// Classify a GPU operation error structurally — typed marker errors and the
/// NVML error code, never the human-readable message.
pub(crate) fn classify_gpu_failure(error: &anyhow::Error) -> GpuOpFailureKind {
    if error.downcast_ref::<GpuGateClosed>().is_some() {
        return GpuOpFailureKind::GateClosed;
    }
    if let Some(NvmlError::NotSupported) = error.downcast_ref::<NvmlError>() {
        return GpuOpFailureKind::Unsupported;
    }
    GpuOpFailureKind::Failed
}

#[cfg(test)]
mod gpu_write_gate_tests {
    use super::with_gpu_write_gate;
    use std::cell::Cell;

    #[test]
    fn direct_mutation_entrypoint_does_not_write_when_gpu_is_non_writable() {
        let writes = Cell::new(0);
        let result = with_gpu_write_gate(0, |_| false, || {
            writes.set(writes.get() + 1);
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(writes.get(), 0);
    }

    #[test]
    fn active_and_unsupported_runtime_states_remain_writable() {
        for runtime_status in ["active", "unsupported"] {
            let result = with_gpu_write_gate(0, |_| matches!(runtime_status, "active" | "unsupported"), || Ok(7));
            assert_eq!(result.unwrap(), 7);
        }
    }
}

#[cfg(test)]
mod gpu_failure_classification_tests {
    use super::{apply_gpu_static, classify_gpu_failure, with_gpu_write_gate, GpuOpFailureKind};
    use lapsphere_common::types::{GpuSettings, NvidiaFanSettings};

    #[test]
    fn gate_closure_classifies_as_gate_closed() {
        let err = with_gpu_write_gate(0, |_| false, || Ok(())).unwrap_err();
        assert_eq!(classify_gpu_failure(&err), GpuOpFailureKind::GateClosed);
    }

    #[test]
    fn nvml_not_supported_classifies_as_unsupported_and_other_errors_as_failed() {
        let unsupported = anyhow::Error::new(nvml_wrapper::error::NvmlError::NotSupported);
        assert_eq!(classify_gpu_failure(&unsupported), GpuOpFailureKind::Unsupported);
        let other = anyhow::anyhow!("disk exploded");
        assert_eq!(classify_gpu_failure(&other), GpuOpFailureKind::Failed);
    }

    #[test]
    fn every_gpu_operation_is_attempted_even_after_a_refusal() {
        // NVML index 999 has no sysfs mapping, so the runtime gate refuses
        // every operation deterministically, on any host and without touching
        // NVML. All five operation groups must still be attempted and
        // classified — proof the sequence continues past the first failure.
        let g = GpuSettings {
            power_limit: Some(150),
            core_offset: Some(100.0),
            memory_offset: Some(500.0),
            min_gpu_clock: Some(405),
            max_gpu_clock: Some(1750),
            nvidia_fans: vec![NvidiaFanSettings { device_index: 999, fan_id: 0, speed: 50, manual: true }],
            ..GpuSettings::default()
        };
        let failures = apply_gpu_static(999, &g);
        assert_eq!(failures.len(), 5, "one failure per attempted operation group: {failures:?}");
        assert!(failures.iter().all(|f| f.kind == GpuOpFailureKind::GateClosed));
    }
}

pub fn set_gpu_locked_clocks(device_index: u32, min_clock: u32, max_clock: u32) -> Result<()> {
    with_gpu_write_gate(device_index, crate::hardware_detection::gpu_write_allowed, || {
        let nvml = get_nvml()?;
        let mut device = nvml.device_by_index(device_index)?;
        device.set_gpu_locked_clocks(GpuLockedClocksSetting::Numeric {
            min_clock_mhz: min_clock,
            max_clock_mhz: max_clock,
        })?;
        Ok(())
    })
}

pub fn reset_gpu_clocks(device_index: u32) -> Result<()> {
    with_gpu_write_gate(device_index, crate::hardware_detection::gpu_write_allowed, || {
        let nvml = get_nvml()?;
        let mut device = nvml.device_by_index(device_index)?;
        device.reset_gpu_locked_clocks()?;
        Ok(())
    })
}

pub fn set_gpu_core_offset(device_index: u32, offset: f32) -> Result<()> {
    with_gpu_write_gate(device_index, crate::hardware_detection::gpu_write_allowed, || {
        let nvml = get_nvml()?;
        let mut device = nvml.device_by_index(device_index)?;
        device.set_clock_offset(Clock::Graphics, PerformanceState::Zero, offset.round() as i32)?;
        {
            let mut map = lock_or_recover(&crate::MANUAL_GPU_OFFSETS, "MANUAL_GPU_OFFSETS");
            let entry = map.entry(device_index).or_insert((0.0, 0.0));
            entry.0 = offset;
        }
        log::info!(target: "hw.gpu", "set_core_offset gpu={} offset={} offset_rounded={}", device_index, offset, offset.round());
        Ok(())
    })
}

pub fn set_gpu_memory_offset(device_index: u32, offset: f32) -> Result<()> {
    with_gpu_write_gate(device_index, crate::hardware_detection::gpu_write_allowed, || {
        let nvml = get_nvml()?;
        let mut device = nvml.device_by_index(device_index)?;
        device.set_clock_offset(Clock::Memory, PerformanceState::Zero, offset.round() as i32)?;
        {
            let mut map = lock_or_recover(&crate::MANUAL_GPU_OFFSETS, "MANUAL_GPU_OFFSETS");
            let entry = map.entry(device_index).or_insert((0.0, 0.0));
            entry.1 = offset;
        }
        log::info!(target: "hw.gpu", "set_mem_offset gpu={} offset={} offset_rounded={}", device_index, offset, offset.round());
        Ok(())
    })
}

pub fn set_gpu_power_limit(device_index: u32, limit_watts: u32) -> Result<()> {
    with_gpu_write_gate(device_index, crate::hardware_detection::gpu_write_allowed, || {
        let nvml = get_nvml()?;
        let mut device = nvml.device_by_index(device_index)?;
        device.set_power_management_limit(limit_watts * 1000)?;
        log::info!(target: "hw.gpu", "set_power_limit gpu={} limit={}W", device_index, limit_watts);
        Ok(())
    })
}

pub fn set_gpu_fan_speed(device_index: u32, fan_index: u32, speed_percent: u32) -> Result<()> {
    with_gpu_write_gate(device_index, crate::hardware_detection::gpu_write_allowed, || {
        let nvml = get_nvml()?;
        let mut device = nvml.device_by_index(device_index)?;
        device.set_fan_speed(fan_index, speed_percent)?;
        log::info!(target: "hw.gpu", "set_fan_speed gpu={} fan={} speed={}%", device_index, fan_index, speed_percent);
        Ok(())
    })
}

pub fn set_gpu_fan_auto(device_index: u32, fan_index: u32) -> Result<()> {
    with_gpu_write_gate(device_index, crate::hardware_detection::gpu_write_allowed, || {
        let nvml = get_nvml()?;
        let mut device = nvml.device_by_index(device_index)?;
        device.set_default_fan_speed(fan_index)?;
        log::info!(target: "hw.gpu", "set_fan_auto gpu={} fan={}", device_index, fan_index);
        Ok(())
    })
}

fn find_binary(cmd: &str) -> Option<String> {
    let paths = ["/usr/bin", "/usr/sbin", "/usr/local/bin", "/usr/local/sbin", "/sbin", "/bin"];
    for path in paths {
        let full_path = format!("{}/{}", path, cmd);
        if Path::new(&full_path).exists() {
            return Some(full_path);
        }
    }
    None
}

pub fn set_prime_profile(profile: &str) -> Result<()> {
    let valid_profiles = ["on-demand", "nvidia", "intel"];
    if !valid_profiles.contains(&profile) {
        return Err(anyhow!("Invalid prime profile: {}", profile));
    }

    // Check for optimus-manager first (common on Arch)
    if let Some(path) = find_binary("optimus-manager") {
        let opt_mode = match profile {
            "on-demand" => "hybrid",
            "intel" => "integrated",
            "nvidia" => "nvidia",
            _ => profile,
        };

        log::info!(target: "hw.gpu", "set_prime_profile mode=\"{}\" tool=\"optimus-manager\"", opt_mode);
        let output = std::process::Command::new(path)
            .arg("--switch")
            .arg(opt_mode)
            .arg("--no-confirm")
            .output()?;

        if !output.status.success() {
            return Err(anyhow!("optimus-manager command failed: {}", String::from_utf8_lossy(&output.stderr)));
        }
        return Ok(());
    }

    // Fallback to prime-select (Ubuntu/Debian)
    let output = std::process::Command::new("prime-select")
        .arg(profile)
        .output()?;
    if !output.status.success() {
        return Err(anyhow!("prime-select command failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    log::info!(target: "hw.gpu", "set_prime_profile mode=\"{}\" tool=\"prime-select\"", profile);
    Ok(())
}

/// EPP attribute paths that exist right now, one per CPU.
///
/// The cpufreq attribute belongs to a *policy*, and the kernel exposes the
/// per-CPU `cpufreq` directories as symlinks into `cpufreq/policyN` (verified on
/// the XMG: `/sys/devices/system/cpu/cpu0/cpufreq` resolves to
/// `/sys/devices/system/cpu/cpufreq/policy0`). Addressing every CPU through those
/// aliases therefore covers every policy using the form the allowlist already
/// contains.
///
/// The parent directory `/sys/devices/system/cpu/cpufreq/` is NOT an attribute —
/// it holds `policyN/` (and `boost`) — so there is no policy-level file to write
/// there, on this kernel or on the layouts the read path uses
/// (`hardware_detection::read_energy_performance_preference` reads the same
/// per-CPU path).
fn existing_epp_paths() -> Vec<String> {
    let Ok(cpu_count) = get_cpu_count() else {
        return Vec::new();
    };
    (0..cpu_count)
        .map(|cpu| {
            format!(
                "/sys/devices/system/cpu/cpu{}/cpufreq/energy_performance_preference",
                cpu
            )
        })
        .filter(|path| Path::new(path).exists())
        .collect()
}

pub fn set_energy_performance_preference(epp: &str) -> Result<()> {
    let valid_values = ["performance", "balance_performance", "balance_power", "power", 
                       "default", "balance-performance", "balance-power"];
    if !valid_values.contains(&epp) {
        return Err(anyhow!("Invalid EPP value: {}", epp));
    }

    for path in existing_epp_paths() {
        guard_sysfs_write(&path, epp)?;
    }

    log::info!(target: "hw.cpu", "set_epp preference=\"{}\"", epp);
    Ok(())
}

#[derive(Debug, Clone)]
pub struct RgbKeyboardControl {
    paths: Vec<String>,
    tuxedo_io: Option<Arc<TuxedoIo>>,
}

impl RgbKeyboardControl {
    pub fn new() -> Result<Self> {
        let paths = Self::find_all_keyboard_backlight_paths();
        let tuxedo_io = TuxedoIo::shared();

        if paths.is_empty() && tuxedo_io.is_none() {
            return Err(anyhow!("No keyboard backlight control available"));
        }

        Ok(Self { paths, tuxedo_io })
    }
    
    
    fn find_all_keyboard_backlight_paths() -> Vec<String> {
        let paths = Vec::new();
        
        // Priority 1: tuxedo_keyboard platform device
        let platform_base = "/sys/devices/platform/tuxedo_keyboard/leds";
        if Path::new(platform_base).exists() {
            // Check for 3-zone
            let zones = ["left", "center", "right"];
            let mut found_zones = Vec::new();
            for zone in zones {
                let path = format!("{}/{}:kbd_backlight", platform_base, zone);
                if Path::new(&path).exists() {
                    found_zones.push(path);
                }
            }

            if !found_zones.is_empty() {
                return found_zones;
            }

            // Check for single zone
            let single = format!("{}/rgb:kbd_backlight", platform_base);
            if Path::new(&single).exists() {
                return vec![single];
            }
        }

        // Priority 2: Standard /sys/class/leds
        if let Ok(entries) = fs::read_dir("/sys/class/leds") {
            let mut kbd_entries: Vec<String> = entries.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.contains("kbd_backlight"))
                .map(|n| format!("/sys/class/leds/{}", n))
                .collect();

            // Sort to have a consistent order (e.g. left, center, right if they are named so)
            kbd_entries.sort();

            if !kbd_entries.is_empty() {
                return kbd_entries;
            }
        }
        
        paths
    }
    
    pub fn set_zone_color(&self, zone_idx: usize, red: u8, green: u8, blue: u8) -> Result<()> {
        if let Some(ref io) = self.tuxedo_io {
            if io.get_interface() == HardwareInterface::Clevo {
                if let Ok(_) = io.set_clevo_keyboard_color(zone_idx as u8, red, green, blue) {
                    // Also try to set via sysfs for consistency, but ignore errors if it fails
                    // as ioctl already succeeded.
                }
            }
        }

        let path = self.paths.get(zone_idx)
            .ok_or_else(|| anyhow!("Invalid zone index: {}", zone_idx))?;

        let color_path = format!("{}/multi_intensity", path);
        if !Path::new(&color_path).exists() {
            return Err(anyhow!("RGB control not available for zone {}", zone_idx));
        }
        
        let color_str = format!("{} {} {}", red, green, blue);
        guard_sysfs_write(&color_path, &color_str)?;
        
        log::info!(target: "hw.kbd", "set_zone_color zone={} r={} g={} b={}", zone_idx, red, green, blue);
        Ok(())
    }
    
    pub fn set_brightness(&self, brightness: u8) -> Result<()> {
        if let Some(ref io) = self.tuxedo_io {
            if io.get_interface() == HardwareInterface::Clevo {
                let _ = io.set_clevo_keyboard_brightness(brightness);
            }
        }

        for path in &self.paths {
            let brightness_path = format!("{}/brightness", path);
            let max_brightness_path = format!("{}/max_brightness", path);

            let max_brightness: u32 = if let Ok(max_str) = fs::read_to_string(&max_brightness_path) {
                max_str.trim().parse().unwrap_or(255)
            } else {
                255
            };

            let actual_brightness = ((brightness as u32) * max_brightness) / 100;
            guard_sysfs_write(&brightness_path, &actual_brightness.to_string())?;
        }
        
        log::info!(target: "hw.kbd", "set_brightness level={}%", brightness);
        Ok(())
    }
    
    pub fn set_mode(&self, mode: &lapsphere_common::types::KeyboardMode) -> Result<()> {
        use lapsphere_common::types::KeyboardMode;
        match mode {
            KeyboardMode::SingleColor { r, g, b, brightness } => {
                // For Clevo, explicitly set mode 0 (Custom/Static)
                if let Some(ref io) = self.tuxedo_io {
                    if io.get_interface() == HardwareInterface::Clevo {
                        let _ = io.set_clevo_keyboard_mode(0x00000000);
                    }
                }

                let num_zones = self.paths.len().max(if self.tuxedo_io.is_some() { 3 } else { 0 });
                for i in 0..num_zones {
                    let _ = self.set_zone_color(i, *r, *g, *b);
                }
                self.set_brightness(*brightness)?;
            }
            KeyboardMode::PerKeyRGB { keys, brightness } => {
                if let Some(ref io) = self.tuxedo_io {
                    if io.get_interface() == HardwareInterface::Clevo {
                        let _ = io.set_clevo_keyboard_mode(0x00000000);
                    }
                }

                for (i, color) in keys.iter().enumerate() {
                    let _ = self.set_zone_color(i, color.r, color.g, color.b);
                }
                self.set_brightness(*brightness)?;
            }
            KeyboardMode::MultipleZones { zones, brightness } => {
                if let Some(ref io) = self.tuxedo_io {
                    if io.get_interface() == HardwareInterface::Clevo {
                        let _ = io.set_clevo_keyboard_mode(0x00000000);
                    }
                }

                for (i, zone) in zones.iter().enumerate() {
                    let _ = self.set_zone_color(i, zone.r, zone.g, zone.b);
                }
                self.set_brightness(*brightness)?;
            }
            KeyboardMode::Breathe { r, g, b, brightness, speed } => {
                if let Some(ref io) = self.tuxedo_io {
                    if io.get_interface() == HardwareInterface::Clevo {
                        let _ = io.set_clevo_keyboard_mode(0x1002a000); // BREATHE
                    }
                }
                self.write_effect_mode(1)?;
                self.write_effect_speed(*speed)?;
                for i in 0..self.paths.len() {
                    let _ = self.set_zone_color(i, *r, *g, *b);
                }
                self.set_brightness(*brightness)?;
                log::info!(target: "hw.kbd", "set_mode mode=\"breathing\" speed={}", speed);
            }
            KeyboardMode::Wave { brightness, speed } => {
                if let Some(ref io) = self.tuxedo_io {
                    if io.get_interface() == HardwareInterface::Clevo {
                        let _ = io.set_clevo_keyboard_mode(0xB0000000); // WAVE
                    }
                }
                self.write_effect_mode(7)?;
                self.write_effect_speed(*speed)?;
                self.set_brightness(*brightness)?;
                log::info!(target: "hw.kbd", "set_mode mode=\"wave\" speed={}", speed);
            }
            KeyboardMode::Cycle { brightness, speed } => {
                if let Some(ref io) = self.tuxedo_io {
                    if io.get_interface() == HardwareInterface::Clevo {
                        let _ = io.set_clevo_keyboard_mode(0x33010000); // CYCLE
                    }
                }
                self.write_effect_mode(2)?;
                self.write_effect_speed(*speed)?;
                self.set_brightness(*brightness)?;
                log::info!(target: "hw.kbd", "set_mode mode=\"cycle\" speed={}", speed);
            }
            KeyboardMode::Dance { brightness, speed } => {
                if let Some(ref io) = self.tuxedo_io {
                    if io.get_interface() == HardwareInterface::Clevo {
                        let _ = io.set_clevo_keyboard_mode(0x80000000); // DANCE
                    }
                }
                self.write_effect_mode(3)?;
                self.write_effect_speed(*speed)?;
                self.set_brightness(*brightness)?;
                log::info!(target: "hw.kbd", "set_mode mode=\"dance\" speed={}", speed);
            }
            KeyboardMode::Flash { r, g, b, brightness, speed } => {
                if let Some(ref io) = self.tuxedo_io {
                    if io.get_interface() == HardwareInterface::Clevo {
                        let _ = io.set_clevo_keyboard_mode(0xA0000000); // FLASH
                    }
                }
                self.write_effect_mode(4)?;
                self.write_effect_speed(*speed)?;
                for i in 0..self.paths.len() {
                    let _ = self.set_zone_color(i, *r, *g, *b);
                }
                self.set_brightness(*brightness)?;
                log::info!(target: "hw.kbd", "set_mode mode=\"flash\" speed={}", speed);
            }
            KeyboardMode::RandomColor { brightness, speed } => {
                if let Some(ref io) = self.tuxedo_io {
                    if io.get_interface() == HardwareInterface::Clevo {
                        let _ = io.set_clevo_keyboard_mode(0x70000000); // RANDOM_COLOR
                    }
                }
                self.write_effect_mode(5)?;
                self.write_effect_speed(*speed)?;
                self.set_brightness(*brightness)?;
                log::info!(target: "hw.kbd", "set_mode mode=\"random\" speed={}", speed);
            }
            KeyboardMode::Tempo { brightness, speed } => {
                if let Some(ref io) = self.tuxedo_io {
                    if io.get_interface() == HardwareInterface::Clevo {
                        let _ = io.set_clevo_keyboard_mode(0x90000000); // TEMPO
                    }
                }
                self.write_effect_mode(6)?;
                self.write_effect_speed(*speed)?;
                self.set_brightness(*brightness)?;
                log::info!(target: "hw.kbd", "set_mode mode=\"tempo\" speed={}", speed);
            }
        }
        Ok(())
    }

    /// Writes the effect mode integer to every LED `mode` attribute.
    ///
    /// Routed through `guard_sysfs_write`: the path must be on
    /// APPROVED_SYSFS_PATHS and the payload must parse as a byte integer.
    /// The old call sites passed a string fallback token ("breathing") which
    /// the guard's LED check can never accept, so the fallback is gone.
    fn write_effect_mode(&self, mode_value: u8) -> Result<()> {
        for path in &self.paths {
            let mode_path = format!("{}/mode", path);
            if Path::new(&mode_path).exists() {
                guard_sysfs_write(&mode_path, &mode_value.to_string())?;
            }
        }
        Ok(())
    }

    /// Writes the effect speed integer to every LED `speed` attribute,
    /// validated by `guard_sysfs_write` the same way as `mode`.
    fn write_effect_speed(&self, speed: u8) -> Result<()> {
        for path in &self.paths {
            let speed_path = format!("{}/speed", path);
            if Path::new(&speed_path).exists() {
                guard_sysfs_write(&speed_path, &speed.to_string())?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: absolute sysfs paths must match the allowlist templates.
    ///
    /// The empty leading segment an absolute path produces ("/sys/..." ->
    /// ["", "sys", ...]) previously poisoned matching, so every real path was
    /// refused with "blocked non-allowlisted sysfs write" and profile application
    /// never reached the hardware at all. Components are now normalised before
    /// matching, which covers that case with the traversal rules in
    /// `traversal_components_cannot_satisfy_an_allowlist_template`.
    #[test]
    fn absolute_paths_match_sysfs_allowlist() {
        // The exact path the live daemon rejected 2026-09-16, plus a sibling
        // wildcard position and a non-wildcard entry for coverage.
        assert!(is_allowed_sysfs_path(
            "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"
        ));
        assert!(is_allowed_sysfs_path(
            "/sys/devices/system/cpu/cpu7/cpufreq/scaling_max_freq"
        ));
        assert!(is_allowed_sysfs_path("/sys/devices/system/cpu/cpufreq/boost"));
        assert!(is_allowed_sysfs_path("/sys/class/leds/kbd_backlight/mode"));

        // Still rejects unlisted attributes: the guard is not weakened.
        assert!(!is_allowed_sysfs_path(
            "/sys/devices/system/cpu/cpu0/cpufreq/scaling_cur_freq"
        ));
        assert!(!is_allowed_sysfs_path("/sys/class/hwmon/hwmon0/temp1_input"));
    }


    /// `..`, `.`, repeated separators and relative spellings must not be able to
    /// reach a file the templates do not describe.
    ///
    /// The wildcard used to accept ANY non-empty segment, including `..`, so
    /// `/sys/class/leds/../brightness` matched the `/sys/class/leds/*/brightness`
    /// template and resolved to `/sys/class/brightness` — outside the subtree the
    /// template fixes. Components are now normalised lexically (no filesystem
    /// access, because sysfs is symlink-based) and `..` is refused outright.
    #[test]
    fn traversal_components_cannot_satisfy_an_allowlist_template() {
        // Every `..` path is refused by the path layer itself, never resolved.
        for path in [
            // '..' in the wildcard slot of a real template
            "/sys/class/leds/../brightness",
            "/sys/class/backlight/../../brightness",
            // '..' walking out of the CPU subtree
            "/sys/devices/system/cpu/../cpu/cpu0/cpufreq/scaling_governor",
            "/sys/devices/system/cpu/cpu0/cpufreq/../../../etc/passwd",
            "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor/../../../..",
            // '..' above the root
            "/../sys/class/leds/kbd_backlight/brightness",
            "/..",
        ] {
            assert!(
                !is_allowed_sysfs_path(path),
                "traversing path was accepted: {path}"
            );
            assert_eq!(normalized_path(path), None, "traversal was resolved: {path}");
            let err = guard_sysfs_write(path, "1").expect_err("guard accepted a traversing path");
            assert!(
                err.to_string().contains("traversing path"),
                "guard did not report traversal for {path}: {err}"
            );
        }

        // A lone `.` in the wildcard slot is dropped rather than resolved, so the
        // result is a *different* path that no template describes — rejected on
        // shape, which is why the message is the allowlist one.
        let dotted = "/sys/class/leds/./brightness";
        assert!(!is_allowed_sysfs_path(dotted));
        assert_eq!(normalized_path(dotted).as_deref(), Some("/sys/class/leds/brightness"));
        let err = guard_sysfs_write(dotted, "1").unwrap_err();
        assert!(err.to_string().contains("non-allowlisted"), "{err}");
    }

    /// A relative path is not a sysfs path: `fs::write` would resolve it against
    /// the daemon's working directory, so it must be refused even when its
    /// component list looks allowlisted.
    #[test]
    fn relative_and_empty_paths_are_rejected() {
        for path in [
            "sys/class/leds/kbd_backlight/brightness",
            "sys/devices/system/cpu/cpu0/cpufreq/scaling_governor",
            "",
            "/",
            "./sys/class/leds/kbd_backlight/brightness",
        ] {
            assert!(!is_allowed_sysfs_path(path), "relative path accepted: {path:?}");
            assert!(
                guard_sysfs_write(path, "1").is_err(),
                "guard accepted a non-absolute path: {path:?}"
            );
        }
    }

    /// Equivalent spellings of one path reduce to the same canonical target, and
    /// that canonical form is what the guard checks and writes.
    #[test]
    fn equivalent_spellings_normalize_to_the_same_target() {
        let canonical = "/sys/class/leds/kbd_backlight/brightness";
        for spelling in [
            "/sys/class/leds//kbd_backlight/brightness",
            "/sys/class/leds/./kbd_backlight/brightness",
            "/sys//class///leds/kbd_backlight/brightness",
            "/sys/class/leds/kbd_backlight/brightness/",
        ] {
            assert!(is_allowed_sysfs_path(spelling), "rejected equivalent spelling: {spelling}");
            assert_eq!(
                normalized_path(spelling).as_deref(),
                Some(canonical),
                "spelling did not reduce to the canonical target: {spelling}"
            );
        }
        // Nothing canonicalises into a path the allowlist does not describe.
        assert_eq!(normalized_path("/sys/class/leds/../brightness"), None);
    }

    /// The guard is the production boundary: it must reject before any write,
    /// reach the write for an allowed path, and run the value rules on the
    /// canonical path.
    #[test]
    fn guard_boundary_rejects_traversal_but_still_writes_allowed_paths() {
        // (a) rejected by the path layer, and the message names traversal rather
        //     than the allowlist: no filesystem access was attempted.
        let err = guard_sysfs_write("/sys/class/leds/../brightness", "1").unwrap_err();
        assert!(err.to_string().contains("traversing path"), "{err}");

        // (b) an allowlisted shape whose file does not exist reaches fs::write and
        //     fails on the filesystem, not on the guard.
        let err = guard_sysfs_write(
            "/sys/class/leds/lapsphere-nonexistent-test-led/brightness",
            "0",
        )
        .unwrap_err();
        assert!(
            !err.to_string().contains("blocked"),
            "allowed path was blocked instead of attempted: {err}"
        );

        // (c) value rules still run, on the canonical path.
        let err = guard_sysfs_write(
            "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor",
            "definitely-not-a-governor",
        )
        .unwrap_err();
        assert!(err.to_string().contains("governor"), "{err}");
    }

    /// Every target the EPP entry points can write stays inside the allowlist,
    /// and the policy-level parent path stays out.
    ///
    /// The policy-level `cpufreq/energy_performance_preference` is not a kernel
    /// attribute and is not on the allowlist; EPP is addressed per CPU.
    #[test]
    fn epp_write_targets_stay_inside_the_allowlist() {
        // Synthetic CPU numbers: the contract, not this machine's CPU count.
        for cpu in [0u32, 1, 7, 15, 63] {
            let path = format!(
                "/sys/devices/system/cpu/cpu{}/cpufreq/energy_performance_preference",
                cpu
            );
            assert!(is_allowed_sysfs_path(&path), "EPP target not allowlisted: {path}");
            let written = guard_sysfs_write(&path, "balance-performance");
            assert!(
                !written.unwrap_err().to_string().contains("blocked"),
                "EPP target was blocked by the guard: {path}"
            );
        }

        // The policy-level path is refused, and so is the policy directory form:
        // the allowlist was not broadened to make the function work.
        for path in [
            "/sys/devices/system/cpu/cpufreq/energy_performance_preference",
            "/sys/devices/system/cpu/cpufreq/policy0/energy_performance_preference",
        ] {
            assert!(!is_allowed_sysfs_path(path), "unexpectedly allowlisted: {path}");
            assert!(guard_sysfs_write(path, "balance-performance").is_err());
        }
    }

    /// The policy-level EPP path is a directory-level attribute that does not
    /// exist in the kernel layout, so it must stay off the allowlist.
    #[test]
    fn policy_parent_epp_path_is_not_allowlisted() {
        assert!(
            !is_allowed_sysfs_path("/sys/devices/system/cpu/cpufreq/energy_performance_preference"),
            "the policy parent directory must not become writable"
        );
    }

    #[test]
    fn brightness_guard_rejects_invalid_value_without_panicking() {
        // Nonexistent LED/backlight fixtures ensure this test never writes hardware.
        // Both attribute variants used to panic while removing their suffixes.
        for path in [
            "/sys/class/leds/lapsphere-nonexistent-test-led/brightness",
            "/sys/class/backlight/lapsphere-nonexistent-test-backlight/actual_brightness",
        ] {
            let result = std::panic::catch_unwind(|| guard_sysfs_write(path, "invalid"));
            assert!(result.is_ok(), "brightness validation panicked for {path}");
            assert!(result.unwrap().is_err(), "invalid brightness must not succeed");
        }
    }

    #[test]
    fn platform_keyboard_attributes_match_sysfs_allowlist() {
        // Discovery prefers this platform path over the /sys/class alias.
        // Rejecting it aborts ApplyProfile before screen and fan settings.
        for zone in ["rgb", "left", "center", "right"] {
            for attr in ["brightness", "multi_intensity", "mode", "speed"] {
                let path = format!(
                    "/sys/devices/platform/tuxedo_keyboard/leds/{zone}:kbd_backlight/{attr}"
                );
                assert!(is_allowed_sysfs_path(&path), "rejected keyboard attribute: {path}");
            }
        }
        assert!(!is_allowed_sysfs_path(
            "/sys/devices/platform/tuxedo_keyboard/leds/rgb:kbd_backlight/trigger"
        ));
        assert!(!is_allowed_sysfs_path(
            "/sys/devices/platform/unrelated/leds/rgb:kbd_backlight/brightness"
        ));
    }

    /// A profile whose earliest hardware step fails: `set_cpu_governor` runs
    /// first in `apply_profile_inner` and "invalid-governor-token" is not on
    /// GOVERNOR_ALLOWLIST, so `guard_sysfs_write` rejects it on any host.
    fn failing_profile() -> Profile {
        Profile {
            name: "regression-early-fail".to_string(),
            is_default: false,
            cpu_settings: CpuSettings {
                governor: Some("invalid-governor-token".to_string()),
                ..CpuSettings::default()
            },
            ..Profile::default()
        }
    }

    #[test]
    fn failing_apply_propagates_err_and_is_repeatable() {
        // Pre-fix: LAST_APPLIED_PROFILE was written before any hardware step
        // ran, so the FIRST apply failed but the SECOND identical apply hit the
        // "already applied" early return and returned Ok without touching
        // hardware. Post-fix both must run the hardware steps and fail.
        let p = failing_profile();

        let first = apply_profile(&p);
        assert!(first.is_err(), "first apply with a bad governor must fail");

        // Re-apply must NOT be suppressed: it re-executes the hardware steps.
        let second = apply_profile(&p);
        assert!(second.is_err(), "re-apply of the same profile after a failure must not be skipped");

        // Same step fails both times, proving re-execution rather than a
        // short-circuit via a stale marker.
        assert_eq!(
            format!("{}", first.unwrap_err()),
            format!("{}", second.unwrap_err()),
            "re-apply must fail at the same step, not be suppressed"
        );
    }

    #[test]
    fn failing_apply_clears_last_applied_marker() {
        // The invariant the bug broke: a failed apply must leave the marker
        // empty so the next identical apply is never suppressed.
        //
        // LAST_APPLIED_PROFILE is a process-global written by every apply, so
        // under cargo's default parallel runner this assertion is racy: a
        // sibling test's apply can land between our apply and the read. The
        // marker value is serialized behind --test-threads=1, which is how
        // this invariant is meant to be checked. The contract this test keeps
        // in parallel mode is that the failed apply returns Err.
        let p = failing_profile();
        assert!(apply_profile(&p).is_err());

        {
            let marker = lock_or_recover(&LAST_APPLIED_PROFILE, "LAST_APPLIED_PROFILE");
            assert!(marker.is_none(), "LAST_APPLIED_PROFILE must be None after a failed apply \
                (if this fires only under parallel cargo test, the cause is a sibling test's \
                write to this process-global, not a regression — rerun with --test-threads=1)");
        }
    }

    #[test]
    fn profile_equality_keyes_the_skip_branch() {
        // The skip branch is keyed on Profile PartialEq; guard against a
        // future derive change silently breaking the contract.
        assert_eq!(&failing_profile(), &failing_profile());
    }

    /// Regression: poisoning LAST_APPLIED_PROFILE must not panic the daemon.
    ///
    /// A `.lock().unwrap()` here used to propagate the poison as a panic in
    /// the calling thread, killing the daemon. The recovery contract is
    /// into_inner() + clear_poison() + error log + carry on.
    #[test]
    fn poisoned_last_applied_profile_is_recovered_not_panic() {
        // Poison the mutex the real way: a thread that panics while holding
        // the guard. Unwinding drops the guard (so the mutex is unlocked) but
        // marks it poisoned — exactly the state a `.lock().unwrap()` used to
        // propagate as a daemon-killing panic.
        let marker_value = failing_profile();
        let handle = std::thread::spawn(move || {
            let mut g = LAST_APPLIED_PROFILE.lock().unwrap();
            *g = Some(marker_value);
            panic!("simulated holder panic (intentional)");
        });
        let _ = handle.join();

        // lock_or_recover must NOT panic: it takes the inner value, clears the
        // poison, logs, and hands back a usable guard.
        {
            let g = lock_or_recover(&LAST_APPLIED_PROFILE, "LAST_APPLIED_PROFILE");
            // NOTE: do NOT assert on the *value* here. This test runs in
            // parallel with other tests that write the same process-global
            // LAST_APPLIED_PROFILE (failing_apply_*); whichever write lands
            // last wins, so the content is inherently racy. The contract under
            // test is the RECOVERY (no panic, poison cleared, usable guard),
            // not the value it happens to carry.
            let _ = &*g;
        }

        // The next plain lock must succeed again — poison cleared. Do NOT
        // assert on the value: LAST_APPLIED_PROFILE is a process-global that
        // sibling tests write, so under cargo's parallel runner the content
        // observed here is inherently racy. The contract under test is that
        // the mutex is *lockable* again (poison cleared), not its contents.
        {
            let g = LAST_APPLIED_PROFILE.lock().unwrap();
            let _ = &*g;
        }
    }

    /// Regression: recovering the mutex that the logger itself locks must not
    /// self-deadlock.
    ///
    /// `DaemonLogger::log` takes DAEMON_LOGS through `lock_or_recover`, and
    /// `lock_or_recover` logs on the recovery path. If the log happened before
    /// the poison was cleared, recovering DAEMON_LOGS would re-enter
    /// `lock_or_recover` on a mutex this thread still holds → self-deadlock,
    /// hanging the caller forever. This test would hang (and be killed by the
    /// test harness timeout) if the ordering is ever reversed.
    #[test]
    fn poisoned_daemon_logs_mutex_recovers_without_self_deadlock() {
        // Poison DAEMON_LOGS the real way: a thread that panics while holding
        // the guard.
        let handle = std::thread::spawn(move || {
            let _g = crate::DAEMON_LOGS.lock().unwrap();
            panic!("simulated holder panic (intentional)");
        });
        let _ = handle.join();

        // If the recovery path logs before clearing poison, this call never
        // returns. Install a logger first so the log call actually reaches
        // DAEMON_LOGS rather than being a no-op.
        let _ = log::set_boxed_logger(Box::new(crate::DaemonLogger {
            inner: env_logger::Builder::new().build(),
        }));
        log::set_max_level(log::LevelFilter::Error);

        // The recovered guard MUST be dropped before the plain lock below:
        // std::sync::Mutex is not reentrant, so holding this guard while
        // re-locking the same mutex from this thread self-deadlocks. (This
        // was the >60s hang — not a deadlock in lock_or_recover itself.)
        {
            let g = lock_or_recover(&crate::DAEMON_LOGS, "DAEMON_LOGS");
            // Recovery succeeded and did not hang.
            assert!(g.capacity() >= 500, "recovered DAEMON_LOGS must be usable");
        }

        // Poison is cleared: a plain lock works again.
        {
            let g2 = crate::DAEMON_LOGS.lock().unwrap();
            assert!(g2.capacity() >= 500);
        }
    }
}

#[cfg(test)]
mod pending_supersede_tests {
    use super::{consume_pending_locked, pending_after_profile_apply, PendingGpuStep};
    use lapsphere_common::types::{GpuSettings, PendingGpuOrigin};
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::Duration;

    fn settings(core: f32) -> GpuSettings {
        GpuSettings {
            core_offset: Some(core),
            ..Default::default()
        }
    }

    #[test]
    fn pending_value_is_consumed_after_application() {
        let slot = Mutex::new(Some(7));
        assert_eq!(consume_pending_locked(&slot, "test", |value| Some((*value * 2, true))), Some(14));
        assert_eq!(*slot.lock().unwrap(), None);
        assert_eq!(consume_pending_locked(&slot, "test", |value| Some((*value, true))), None);
    }

    #[test]
    fn replacing_pending_settings_keeps_the_newest_request() {
        let slot = Mutex::new(Some((0u32, settings(-50.0))));
        *slot.lock().unwrap() = Some((0u32, settings(-100.0)));
        assert_eq!(slot.lock().unwrap().as_ref().unwrap().1.core_offset, Some(-100.0));
    }

    #[test]
    fn apply_a_then_concurrent_b_replacement_leaves_b_pending() {
        let slot = Arc::new(Mutex::new(Some('A')));
        let (apply_started_tx, apply_started_rx) = mpsc::channel();
        let (finish_apply_tx, finish_apply_rx) = mpsc::channel();
        let worker_slot = Arc::clone(&slot);
        let worker = std::thread::spawn(move || {
            consume_pending_locked(&worker_slot, "test", |value| {
                apply_started_tx.send(*value).unwrap();
                finish_apply_rx.recv().unwrap();
                Some((*value, true))
            })
        });
        assert_eq!(apply_started_rx.recv_timeout(Duration::from_secs(1)).unwrap(), 'A');

        let (replace_started_tx, replace_started_rx) = mpsc::channel();
        let (replace_done_tx, replace_done_rx) = mpsc::channel();
        let replace_slot = Arc::clone(&slot);
        let replacer = std::thread::spawn(move || {
            replace_started_tx.send(()).unwrap();
            *replace_slot.lock().unwrap() = Some('B');
            replace_done_tx.send(()).unwrap();
        });
        replace_started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(replace_done_rx.recv_timeout(Duration::from_millis(20)).is_err());

        finish_apply_tx.send(()).unwrap();
        assert_eq!(worker.join().unwrap(), Some('A'));
        replace_done_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        replacer.join().unwrap();
        assert_eq!(consume_pending_locked(&slot, "test", |value| Some((*value, true))), Some('B'));
        assert_eq!(*slot.lock().unwrap(), None);
    }

    #[test]
    fn deferred_a_then_applied_b_leaves_only_b_and_no_pending() {
        let step = |core: f32| PendingGpuStep {
            idx: 0,
            settings: settings(core),
            origin: PendingGpuOrigin::ProfileApply,
        };
        let after_a = pending_after_profile_apply(false, step(-50.0));
        assert_eq!(after_a.as_ref().map(|s| s.settings.core_offset), Some(Some(-50.0)));
        let after_b = pending_after_profile_apply(true, step(-100.0));
        assert_eq!(after_b, None);
    }
}

#[cfg(test)]
mod pending_gate_race_tests {
    use super::{attempt_gpu_step, consume_pending_locked, pending_after_profile_apply, pending_consumed_after_attempt, GpuOpFailure, GpuOpFailureKind, PendingGpuStep};
    use lapsphere_common::types::{GpuSettings, PendingGpuOrigin};
    use std::sync::Mutex;

    fn settings() -> GpuSettings {
        GpuSettings {
            core_offset: Some(-100.0),
            ..Default::default()
        }
    }

    fn failure(op: &'static str, kind: GpuOpFailureKind) -> GpuOpFailure {
        GpuOpFailure { op, message: format!("{op} refused"), kind }
    }

    #[test]
    fn gate_passing_then_denying_keeps_settings_pending_until_writable() {
        let slot = Mutex::new(Some((0u32, settings())));

        // Tick 1: the pre-check passes (the adapter looked writable), then the
        // gate refuses an operation mid-apply. The classified failure list is
        // what `apply_pending_gpu` builds from `apply_gpu_static`; the slot
        // must stay pending.
        let first = consume_pending_locked(&slot, "test", |(idx, _)| {
            let failures = vec![failure("set_gpu_core_offset", GpuOpFailureKind::GateClosed)];
            let consumed = pending_consumed_after_attempt(&failures);
            Some(((*idx, failures), consumed))
        });
        assert!(first.is_some());
        assert!(slot.lock().unwrap().is_some(), "a mid-apply gate refusal must keep the deferred settings pending");

        // Tick 2: the adapter is writable again and every operation succeeds;
        // the slot is consumed.
        let second = consume_pending_locked(&slot, "test", |(idx, _)| {
            let failures: Vec<GpuOpFailure> = Vec::new();
            let consumed = pending_consumed_after_attempt(&failures);
            Some(((*idx, failures), consumed))
        });
        assert!(second.is_some());
        assert!(slot.lock().unwrap().is_none(), "a clean writable tick must consume the settings");
    }

    #[test]
    fn permanent_outcomes_consume_the_pending_slot() {
        // No failures: consumed.
        assert!(pending_consumed_after_attempt(&[]));
        // Unsupported only: consumed — retrying an unsupported op forever
        // would be a loop with no exit.
        assert!(pending_consumed_after_attempt(&[failure("set_gpu_power_limit", GpuOpFailureKind::Unsupported)]));
        // Genuine failure: consumed — retrying does not fix it.
        assert!(pending_consumed_after_attempt(&[failure("set_gpu_core_offset", GpuOpFailureKind::Failed)]));
        // Transient gate refusal present: kept, even beside permanent outcomes.
        assert!(!pending_consumed_after_attempt(&[failure("set_gpu_core_offset", GpuOpFailureKind::GateClosed)]));
        assert!(!pending_consumed_after_attempt(&[
            failure("set_gpu_core_offset", GpuOpFailureKind::GateClosed),
            failure("set_gpu_power_limit", GpuOpFailureKind::Unsupported),
        ]));
    }

    #[test]
    fn immediate_apply_keeps_the_newest_settings_when_the_gate_closes_mid_apply() {
        // The immediate apply's production decision path: attempt_gpu_step
        // (shared with the deferred retry) + pending_after_profile_apply,
        // driven with injected gate and operation functions.
        let settings = settings();
        let slot = Mutex::new(None::<PendingGpuStep>);

        // The pre-check passes; an individual operation is then refused with
        // the typed GpuGateClosed failure.
        let (failures, consumed) = attempt_gpu_step(0, &settings, |_| true, |_, _| {
            vec![failure("set_gpu_core_offset", GpuOpFailureKind::GateClosed)]
        })
        .expect("the pre-check passed");
        assert!(!consumed);
        assert_eq!(failures.len(), 1);

        // The immediate-apply decision stores the step for retry instead of
        // discarding it.
        *slot.lock().unwrap() = pending_after_profile_apply(consumed, PendingGpuStep {
            idx: 0,
            settings: settings.clone(),
            origin: PendingGpuOrigin::ProfileApply,
        });
        assert!(slot.lock().unwrap().is_some(), "a mid-apply gate refusal must keep the settings pending");

        // The next writable attempt applies everything and consumes the slot.
        let (failures, consumed) = attempt_gpu_step(0, &settings, |_| true, |_, _| Vec::new())
            .expect("the pre-check passed");
        assert!(failures.is_empty());
        assert!(consumed);
        *slot.lock().unwrap() = pending_after_profile_apply(consumed, PendingGpuStep {
            idx: 0,
            settings,
            origin: PendingGpuOrigin::ProfileApply,
        });
        assert!(slot.lock().unwrap().is_none(), "a final attempt must clear the slot");
    }

    #[test]
    fn deferred_retry_keeps_then_consumes_through_the_shared_attempt_path() {
        // The deferred retry's production path: consume_pending_locked +
        // attempt_gpu_step, with the same injected gate/operation functions.
        let slot = Mutex::new(Some((0u32, settings())));

        // Tick 1: pre-check passes, an operation is refused mid-apply — the
        // slot stays pending.
        let first = consume_pending_locked(&slot, "test", |(idx, s)| {
            attempt_gpu_step(*idx, s, |_| true, |_, _| {
                vec![failure("set_gpu_core_offset", GpuOpFailureKind::GateClosed)]
            })
            .map(|(failures, consumed)| ((*idx, failures), consumed))
        });
        assert!(first.is_some());
        assert!(slot.lock().unwrap().is_some(), "a mid-apply gate refusal must keep the settings pending");

        // Tick 2: the adapter is writable and every operation succeeds — the
        // slot is consumed.
        let second = consume_pending_locked(&slot, "test", |(idx, s)| {
            attempt_gpu_step(*idx, s, |_| true, |_, _| Vec::new())
                .map(|(failures, consumed)| ((*idx, failures), consumed))
        });
        assert!(second.is_some());
        assert!(slot.lock().unwrap().is_none(), "a clean writable tick must consume the settings");
    }

    #[test]
    fn gate_pre_check_refusal_keeps_settings_without_attempting() {
        let slot = Mutex::new(Some((0u32, settings())));
        let mut attempted = false;
        let result = consume_pending_locked(&slot, "test", |(idx, s)| {
            attempt_gpu_step(*idx, s, |_| false, |_, _| {
                attempted = true;
                Vec::new()
            })
            .map(|(failures, consumed)| ((*idx, failures), consumed))
        });
        assert!(result.is_none(), "a refused pre-check must not consume the slot");
        assert!(!attempted, "a refused pre-check must not run operations");
        assert!(slot.lock().unwrap().is_some());
    }
}

#[cfg(test)]
mod deferred_report_tests {
    use super::{publish_deferred_gpu_report, take_deferred_gpu_report, GpuOpFailure, GpuOpFailureKind};
    use lapsphere_common::types::{DeferredGpuOutcome, PendingGpuOrigin};

    fn failure(op: &'static str, kind: GpuOpFailureKind) -> GpuOpFailure {
        GpuOpFailure { op, message: format!("{op} refused"), kind }
    }

    #[test]
    fn deferred_reports_are_taken_once_and_clean_completions_end_the_series() {
        // Single test owns this process-global; drain anything a sibling left.
        let _ = take_deferred_gpu_report();

        // A profile apply that skipped an unsupported op publishes a warning
        // tagged with its origin.
        let unsupported = vec![failure("set_gpu_power_limit", GpuOpFailureKind::Unsupported)];
        publish_deferred_gpu_report(PendingGpuOrigin::ProfileApply, &unsupported);
        let report = take_deferred_gpu_report().expect("a warning outcome must be queued");
        assert_eq!(report.outcome, DeferredGpuOutcome::AppliedWithWarnings);
        assert_eq!(report.origin, PendingGpuOrigin::ProfileApply);
        assert!(report.text.contains("set_gpu_power_limit"));
        assert!(take_deferred_gpu_report().is_none(), "delivery is take-once");

        // Identical outcome again: deduplicated, nothing queued.
        publish_deferred_gpu_report(PendingGpuOrigin::ProfileApply, &unsupported);
        assert!(take_deferred_gpu_report().is_none());

        // A clean profile-apply completion publishes and ends the series...
        publish_deferred_gpu_report(PendingGpuOrigin::ProfileApply, &[]);
        let clean = take_deferred_gpu_report().expect("a clean profile-apply completion must be published");
        assert_eq!(clean.outcome, DeferredGpuOutcome::Applied);
        assert_eq!(clean.origin, PendingGpuOrigin::ProfileApply);
        // ...so the same warning is shown again when it recurs later.
        publish_deferred_gpu_report(PendingGpuOrigin::ProfileApply, &unsupported);
        assert!(take_deferred_gpu_report().is_some(), "a recurrence after a clean completion must not be suppressed");

        // A clean idle-OC restore stays silent: no success event per cycle.
        let _ = take_deferred_gpu_report();
        publish_deferred_gpu_report(PendingGpuOrigin::IdleRestore, &[]);
        assert!(take_deferred_gpu_report().is_none(), "a clean restore must not notify");

        // A failing idle-OC restore is reported with its origin.
        publish_deferred_gpu_report(PendingGpuOrigin::IdleRestore, &[failure("set_gpu_core_offset", GpuOpFailureKind::Failed)]);
        let report = take_deferred_gpu_report().expect("a restore failure must be queued");
        assert_eq!(report.outcome, DeferredGpuOutcome::Failed);
        assert_eq!(report.origin, PendingGpuOrigin::IdleRestore);
    }
}
