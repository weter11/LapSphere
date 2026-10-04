//! One module for every unit the panel prints.
//!
//! The owner's requirement is that units live in one place with correct case,
//! because the panel is read at a glance while a game is running: `MHz` and
//! `GHz` are not interchangeable, `W` is not `mW`, and `°C` is not `C`. Every
//! element formats through this module rather than embedding a suffix string,
//! so a unit cannot drift between two elements.
//!
//! Two rules hold everywhere:
//!
//! * `—` (em dash) means "no data", and it is the ONLY representation of a
//!   missing value. It is never mixed into a number, and no element ever
//!   renders an empty string.
//! * A value is never invented. `None` in, `—` out. An `Option` field that
//!   the hardware did not report is not zero and is not a guess.

/// What an element prints when there is no data.
pub const NO_DATA: &str = "—";

/// The value an element renders, before layout.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A number plus its unit, already formatted.
    Text(String),
    /// No data: renders as `—`.
    Absent,
    /// A graph: a fixed-size strip, no text.
    Graph,
}

impl Value {
    pub fn text(&self) -> String {
        match self {
            Value::Text(text) => text.clone(),
            Value::Absent => NO_DATA.to_string(),
            Value::Graph => String::new(),
        }
    }

    pub fn is_absent(&self) -> bool {
        matches!(self, Value::Absent)
    }
}

/// Megahertz, switching to GHz at 1000 MHz so a 5 GHz part does not print
/// `5000 MHz` in a narrow slot.
///
/// Rounding: MHz values are printed as integers (the daemon reports whole MHz),
/// GHz to two decimals, because `4.90 GHz` and `4.00 GHz` are distinguishable
/// boost states and `4.9` / `4.0` read the same while being narrower.
pub fn mhz(value: u64) -> String {
    if value >= 1000 {
        format!("{:.2} GHz", value as f64 / 1000.0)
    } else {
        format!("{value} MHz")
    }
}

/// Celsius, one decimal: below 10 °C the integer is not informative.
pub fn celsius(value: f32) -> String {
    format!("{value:.1} °C")
}

/// Percent, one decimal. Load percentages below 10 % are worth the digit, and
/// 100.0 is a real state rather than a rounding artefact.
pub fn percent(value: f32) -> String {
    format!("{value:.1} %")
}

/// Watts, one decimal. A GPU idles around 10 W, where the decimal is the whole
/// number; a CPU package under load is 45 W, where it is a rounding detail. The
/// slot is sized for the worst case.
pub fn watts(value: f32) -> String {
    format!("{value:.1} W")
}

/// Volts, two decimals: GPU voltages move in hundredths of a volt and the
/// offset feature works in exactly that resolution.
pub fn volts(value: f32) -> String {
    format!("{value:.2} V")
}

/// Milliamps, no decimals — a current reading is a whole number of mA.
pub fn milliamps(value: i64) -> String {
    format!("{value} mA")
}

/// Frames per second, no decimals: sub-1 fps resolution is not a reading, and
/// the slot is narrow.
pub fn fps(value: f64) -> String {
    format!("{value:.0} fps")
}

/// Milliseconds, two decimals. Frame time is where the decimal lives: 16.7 ms
/// and 33.4 ms are the 60 fps and 30 fps cases and the difference is the whole
/// point of the element.
pub fn milliseconds(value: f64) -> String {
    format!("{value:.2} ms")
}

/// Bytes rendered as GiB, two decimals.
pub fn gib(value: f64) -> String {
    format!("{value:.2} GiB")
}

/// Bytes rendered as MiB, no decimals (VRAM totals and per-process figures are
/// whole MiB in the daemon's snapshot).
pub fn mib(value: u64) -> String {
    format!("{value} MiB")
}

/// A bare integer with no unit — device names, memory type, frequencies already
/// carrying their own word.
pub fn plain(value: u64) -> String {
    format!("{value}")
}

/// A percentage stored as 0–100 in a `u8` (gamepad battery).
pub fn percent_u8(value: u8) -> String {
    format!("{value} %")
}

/// `Option` passthrough: `None` is absent, never zero.
pub fn optional<T>(value: Option<T>, format: impl FnOnce(T) -> String) -> Value {
    match value {
        Some(value) => Value::Text(format(value)),
        None => Value::Absent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_data_is_an_em_dash() {
        assert_eq!(NO_DATA, "—");
        assert_eq!(Value::Absent.text(), "—");
        assert!(Value::Absent.is_absent());
        assert!(!Value::Text("1".into()).is_absent());
    }

    #[test]
    fn mhz_switches_to_ghz_at_one_thousand() {
        assert_eq!(mhz(800), "800 MHz");
        assert_eq!(mhz(999), "999 MHz");
        assert_eq!(mhz(1000), "1.00 GHz");
        assert_eq!(mhz(5800), "5.80 GHz");
        assert_eq!(mhz(0), "0 MHz");
    }

    #[test]
    fn a_gpu_boost_state_is_distinguishable() {
        // The reason GHz gets two decimals rather than none.
        assert_ne!(mhz(4900), mhz(4000));
    }

    #[test]
    fn celsius_keeps_a_decimal_below_ten() {
        assert_eq!(celsius(45.0), "45.0 °C");
        assert_eq!(celsius(7.25), "7.2 °C", "7.25 formats to 7.2, not 7.3");
    }

    #[test]
    fn percent_has_a_space_and_a_sign() {
        assert_eq!(percent(0.0), "0.0 %");
        assert_eq!(percent(100.0), "100.0 %");
        assert_eq!(percent(12.34), "12.3 %");
    }

    #[test]
    fn watts_voltages_and_currents_use_their_own_units() {
        assert_eq!(watts(45.0), "45.0 W");
        assert_eq!(volts(1.05), "1.05 V");
        assert_eq!(milliamps(1500), "1500 mA");
    }

    #[test]
    fn frame_units_keep_the_decimals_that_matter() {
        assert_eq!(fps(59.94), "60 fps");
        assert_eq!(milliseconds(16.67), "16.67 ms");
        assert_ne!(milliseconds(16.67), milliseconds(33.4));
    }

    #[test]
    fn memory_units_are_gib_and_mib() {
        assert_eq!(gib(15.5), "15.50 GiB");
        assert_eq!(mib(8192), "8192 MiB");
    }

    #[test]
    fn optional_maps_none_to_absent_not_zero() {
        assert_eq!(optional(None::<f32>, watts), Value::Absent);
        assert_eq!(optional(Some(10.0f32), watts), Value::Text("10.0 W".into()));
        assert_eq!(optional(Some(0.0f32), watts), Value::Text("0.0 W".into()));
        assert_eq!(optional(Some(50u8), percent_u8), Value::Text("50 %".into()));
    }

    #[test]
    fn a_graph_renders_no_text() {
        assert_eq!(Value::Graph.text(), "");
        assert!(
            !Value::Graph.is_absent(),
            "a graph is present, just not a number"
        );
    }

    #[test]
    fn every_unit_suffix_has_the_documented_case() {
        // A regression guard on the thing the owner asked for: correct case,
        // one place. If someone adds "mhz" or "C" or "mW" here, this fails.
        let rendered = [
            mhz(800),
            celsius(1.0),
            percent(1.0),
            watts(1.0),
            volts(1.0),
            milliamps(1),
            fps(1.0),
            milliseconds(1.0),
            gib(1.0),
            mib(1),
            percent_u8(1),
            plain(1),
        ];
        for value in rendered {
            assert!(!value.contains("mhz"), "lowercase MHz in {value:?}");
            assert!(!value.contains(" Mhz"), "wrong case in {value:?}");
            assert!(!value.contains("mW"), "milliwatts in {value:?}");
            assert!(!value.contains(" C"), "bare Celsius in {value:?}");
            assert!(!value.contains("Percent"), "spelled-out unit in {value:?}");
        }
    }
}
