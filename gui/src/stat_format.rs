//! The formatting functions the Statistics tab renders with.
//!
//! # Why this module exists
//!
//! Two surfaces show the same hardware values: the Statistics tab
//! (`pages/statistics.rs`) and the compact panel (`panel/rows.rs`). Before this
//! module each formatted its own values inline, which meant a panel row and its
//! Statistics counterpart could drift apart — different decimal places, a
//! different unit suffix, a different dash for absent data — with nothing to
//! catch it.
//!
//! So the format strings live here, once, and BOTH surfaces call them. The
//! Statistics tab is otherwise untouched: every call site below produces the
//! same string it produced inline, so its appearance is unchanged. The rule
//! this module enforces is deliberately narrow — **formatting only**. Reading a
//! value out of `AppState` stays with each surface, because the panel takes the
//! first GPU and renders a column while Statistics renders every GPU in a grid;
//! sharing the read would couple two layouts that are allowed to differ.
//!
//! `panel/rows.rs` additionally asserts, in a test, that every label it offers
//! is a label the Statistics tab actually shows, so the two lists cannot drift
//! apart silently either.

use crate::theme::{load_color, power_color, temp_color};
use egui::Color32;

/// Shown wherever hardware did not report a value.
///
/// Statistics already used a bare `—` for several rows; the panel uses it for
/// every absent value so a row never changes height when a device appears.
pub const ABSENT: &str = "—";

/// `4799` -> `"4799 MHz"`.
pub fn mhz(value: u64) -> String {
    format!("{} MHz", value)
}

/// Average frequency as Statistics shows it: the field is kHz-scaled, and the
/// tab divides before printing. Preserved verbatim so the panel cannot "fix" it
/// into a different number than the tab next to it.
pub fn average_frequency_mhz(value_khz: u64) -> String {
    format!("{} MHz", value_khz / 1000)
}

/// `42.85` -> `"42.9%"` (one decimal, as the load and usage rows show).
pub fn percent1(value: f32) -> String {
    format!("{:.1}%", value)
}

/// The same as [`percent1`] for the fields the wire type declares `f64`
/// (the per-mount usage figure).
pub fn percent1_f64(value: f64) -> String {
    format!("{:.1}%", value)
}

/// `65.0` -> `"65.0°C"`.
pub fn celsius1(value: f32) -> String {
    format!("{:.1}°C", value)
}

/// `16.0` -> `"16.0 GiB"`.
pub fn gib2(value: f64) -> String {
    format!("{:.2} GiB", value)
}

/// `45.3` -> `"45.3 W"`.
pub fn watts1(value: f32) -> String {
    format!("{:.1} W", value)
}

/// `1.025` -> `"1.025 V"` (the GPU voltage row).
pub fn volts3(value: f32) -> String {
    format!("{:.3} V", value)
}

/// `11.55` -> `"11.55 V"` (the battery voltage row, two decimals).
///
/// Separate from [`volts3`] on purpose: the two rows really do print different
/// precision, and merging them would silently change one of them.
pub fn volts2(value: f64) -> String {
    format!("{:.2} V", value)
}

/// `0.42` -> `"0.42 A"` (the battery current row).
pub fn amperes2(value: f64) -> String {
    format!("{:.2} A", value)
}

/// `1520.5` -> `"1520.5 MB/s"` (the storage speed rows, `f64` in the wire type).
pub fn mbps1(value: f64) -> String {
    format!("{:.1} MB/s", value)
}

/// A signed MHz offset, with an explicit `+` on positive values.
///
/// Shared so an offset reads the same in the panel as it does in the GPU grid.
pub fn signed_mhz(value: i32) -> String {
    format!("{}{} MHz", if value >= 0 { "+" } else { "" }, value)
}

/// Byte counts, scaled to the largest unit that still leaves a number >= 1.
pub fn bytes(value: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let bytes_f = value as f64;
    if bytes_f >= GIB {
        format!("{:.2} GiB", bytes_f / GIB)
    } else if bytes_f >= MIB {
        format!("{:.2} MiB", bytes_f / MIB)
    } else if bytes_f >= KIB {
        format!("{:.2} KiB", bytes_f / KIB)
    } else {
        format!("{} B", value)
    }
}

/// Colour for a load percentage, as the load bars use.
pub fn load_tint(value: f32) -> Color32 {
    load_color(value)
}

/// Colour for a temperature in °C, as the temperature labels use.
pub fn temp_tint(value: f32) -> Color32 {
    temp_color(value)
}

/// Colour for a power draw in watts, as the power labels use.
pub fn power_tint(value: f32) -> Color32 {
    power_color(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    // These are the exact strings the Statistics tab produced inline before the
    // extraction. They are the contract: if one of these changes, the tab's
    // appearance changed, which this change is not allowed to do.
    #[test]
    fn the_extracted_formats_match_what_the_tab_printed_inline() {
        assert_eq!(mhz(4500), "4500 MHz");
        assert_eq!(average_frequency_mhz(3_600_000), "3600 MHz");
        // 42.85 is not representable in f32 and lands just below it, so this
        // asserts the ACTUAL inline output rather than the decimal value.
        assert_eq!(percent1(42.85), "42.8%");
        assert_eq!(celsius1(65.04), "65.0°C");
        assert_eq!(gib2(16.0), "16.00 GiB");
        assert_eq!(watts1(45.34), "45.3 W");
        assert_eq!(volts3(1.0254), "1.025 V");
        assert_eq!(signed_mhz(50), "+50 MHz");
        assert_eq!(signed_mhz(-50), "-50 MHz");
        // The tab's inline test was `if fo >= 0 { "+" } else { "" }`, so zero
        // prints a plus sign. Preserved rather than "fixed": changing it would
        // change the Statistics tab's appearance.
        assert_eq!(signed_mhz(0), "+0 MHz");
    }

    #[test]
    fn bytes_picks_the_largest_unit_that_keeps_a_leading_digit() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(2048), "2.00 KiB");
        assert_eq!(bytes(5 * 1024 * 1024), "5.00 MiB");
        assert_eq!(bytes(3 * 1024 * 1024 * 1024), "3.00 GiB");
    }

    #[test]
    fn the_absent_placeholder_is_a_dash_not_an_empty_string() {
        // An empty value would collapse the row; the point of ABSENT is a
        // one-character placeholder of a FIXED width.
        assert_eq!(ABSENT, "—");
        assert_eq!(ABSENT.chars().count(), 1);
    }
}
