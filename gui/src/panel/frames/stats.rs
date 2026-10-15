//! The statistics behind the frame elements — the reference definitions, ported.
//!
//! # Why this module is a port and not a re-derivation
//!
//! `docs/development/tools/frametime_stats.py` is the reference: it is what the
//! spike feeds the layer's raw intervals through, and it is where "1 % low" is
//! **defined** rather than re-invented. Re-deriving the same numbers in Rust
//! would create a second, subtly different definition, and the panel would then
//! disagree with the tool that measures it. The three formulas, as the reference
//! states them:
//!
//! * average fps — `1000 / mean(frame intervals in ms)`;
//! * `frametime_ms` — the same mean interval, in milliseconds;
//! * `low_1pct` / `low_01pct` — `1000 / mean(worst ceil(pct/100 × n) intervals)`,
//!   the reference's `low_percent()`, which is its definition **B** (the more
//!   common one). Its definition **A** (`1000 / p99`) is not shown: the panel has
//!   exactly one slot for the number, and two numbers in one slot is how a
//!   readout stops being comparable to the tool.
//!
//! `low_percent` uses `max(1, …)` for the count of the worst intervals, so with
//! one sample the 1 % low and the 0.1 % low are that sample — the reference's
//! behaviour, kept deliberately rather than "improved".
//!
//! Values that are not frame times are dropped, exactly as the reference's
//! loader drops non-positive values: an unwritten ring slot reads as `0`, and
//! counting it would drag every mean towards zero.
//!
//! # The window
//!
//! The sample set is a **time** window, newest first, stopping at the first
//! interval that brings the total to `window_seconds` or more — Q9's proposed
//! shape ("what my 1 % low was over the last 10 seconds"). The default is
//! [`super::DEFAULT_WINDOW_SECONDS`], which is a *proposal*, not an owner
//! decision; see [`super`] and the PR body.

/// Half a millisecond of slack, so a window whose samples sum to 9999.9999 ms does
/// not read as short of the 10 000 ms that was asked for. A genuinely short history
/// misses by seconds, never by a rounding error.
pub const SHORT_WINDOW_SLACK_MS: f64 = 0.5;

/// The frame statistics one panel refresh shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// Intervals the statistics were computed over.
    pub samples: usize,
    /// Total duration those intervals **actually cover**, in milliseconds.
    ///
    /// Not the configured window: the reader can only compute over the intervals
    /// that exist (the ring holds at most `shm::MAX_RING_CAPACITY`, and a game that
    /// just started has a handful). This is the covered span, and
    /// [`Metrics::requested_ms`] is what was asked for.
    pub window_ms: f64,
    /// The span the configured statistics window asked for, in milliseconds.
    ///
    /// `0.0` when no window was asked for — the pure-statistics entry point has no
    /// config to read; the poller passes the configured window to [`metrics`]. It is
    /// what [`Metrics::is_short`] compares against, and what the panel reports
    /// instead of presenting a shorter history as the requested window.
    pub requested_ms: f64,
    /// Mean frame interval in milliseconds — the `frametime_ms` element.
    pub frametime_ms: f64,
    /// `1000 / mean` — the `fps` element.
    pub avg_fps: f64,
    /// Mean of the worst 1 % of intervals, inverted — the `low_1pct` element.
    pub low_1pct: f64,
    /// Mean of the worst 0.1 % of intervals, inverted — the `low_01pct` element.
    pub low_01pct: f64,
}

/// An interval in nanoseconds as milliseconds, or `None` if it is not a frame.
///
/// Zero is what an unwritten ring slot holds, and the reference's loader drops
/// non-positive values for the same reason.
pub fn to_ms(interval_ns: u64) -> Option<f64> {
    if interval_ns == 0 {
        return None;
    }
    Some(interval_ns as f64 / 1_000_000.0)
}

/// The newest intervals covering `window_ms`, oldest first.
///
/// Walks the interval series from the newest end and stops at the first sample
/// that reaches the window, so the returned set **contains** that sample rather
/// than being cut short of it. Values that are not frame times are skipped: they
/// neither count nor end the walk, which is what keeps a hole in the ring from
/// truncating the window.
pub fn window(intervals_ns: &[u64], window_ms: f64) -> Vec<f64> {
    let mut taken: Vec<f64> = Vec::new();
    let mut total = 0.0;
    for interval in intervals_ns.iter().rev() {
        if let Some(ms) = to_ms(*interval) {
            taken.push(ms);
            total += ms;
        }
        if total >= window_ms {
            break;
        }
    }
    taken.reverse();
    taken
}

/// `sum(sorted) / n`, accumulated in ascending order exactly as the reference
/// does, so the two implementations round the same way.
pub fn mean(sorted_ms: &[f64]) -> Option<f64> {
    if sorted_ms.is_empty() {
        return None;
    }
    let mut total = 0.0;
    for ms in sorted_ms {
        total += *ms;
    }
    Some(total / sorted_ms.len() as f64)
}

/// The reference's `low_percent()`: the average of the worst `pct` per cent of
/// frame times, returned as fps.
///
/// `sorted_ms` must be ascending. `k = max(1, ceil(pct / 100 × n))`, the mean of
/// the last `k` entries, inverted. With `pct = 1` and a mixed tail the answer
/// differs from `1000 / p99`, which is the difference the reference documents
/// between its A and B readings.
pub fn low_percent(sorted_ms: &[f64], pct: f64) -> Option<f64> {
    if sorted_ms.is_empty() {
        return None;
    }
    let n = sorted_ms.len();
    let k = ((pct / 100.0 * n as f64).ceil() as usize).max(1);
    let worst = &sorted_ms[n - k.min(n)..];
    let worst_mean = mean(worst)?;
    if worst_mean <= 0.0 {
        return Some(f64::INFINITY);
    }
    Some(1000.0 / worst_mean)
}

/// Every statistic the six frame elements need, or `None` when the window holds
/// no usable frame.
///
/// `None` is the honest answer to "there is nothing to show": it becomes `—` and
/// an empty strip, never a zero, never the previous value.
/// `requested_ms` is the configured statistics window, and it is carried onto the
/// result rather than used to clamp anything.
///
/// The distinction it exists for: `window_seconds` permits 60 s, while the reader
/// retains at most `shm::MAX_RING_CAPACITY` intervals — at 165 fps that is ~24 s of
/// history, at 240 fps under 2 s. A configured window longer than the ring can hold
/// must therefore be **reported as partly covered**, not presented as if the
/// requested window had been measured. Nothing is rejected and nothing is
/// extrapolated: the statistics are what they are, over the samples that exist, and
/// the caller gets both spans so it can say so. `0.0` means "no window was asked
/// for" — a caller with no config — and then [`Metrics::is_short`] has no opinion.
pub fn metrics(intervals_ms: &[f64], requested_ms: f64) -> Option<Metrics> {
    let mut sorted: Vec<f64> = intervals_ms
        .iter()
        .copied()
        .filter(|ms| ms.is_finite() && *ms > 0.0)
        .collect();
    if sorted.is_empty() {
        return None;
    }
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let frametime_ms = mean(&sorted)?;
    if frametime_ms <= 0.0 {
        return None;
    }
    Some(Metrics {
        samples: sorted.len(),
        window_ms: sorted.iter().sum(),
        requested_ms,
        frametime_ms,
        avg_fps: 1000.0 / frametime_ms,
        low_1pct: low_percent(&sorted, 1.0)?,
        low_01pct: low_percent(&sorted, 0.1)?,
    })
}

impl Metrics {
    /// Is there less history than the configured window asked for?
    ///
    /// True for two ordinary cases, and both are worth showing rather than hiding:
    /// a game that started less than a window ago (the layer's first intervals), and
    /// a window longer than the ring can hold (60 s at 240 fps is ~14400 intervals
    /// against a 4096-slot ring). `false` when no window was recorded — the
    /// pure-statistics entry point has no opinion about coverage.
    pub fn is_short(&self) -> bool {
        self.requested_ms > 0.0 && self.window_ms + SHORT_WINDOW_SLACK_MS < self.requested_ms
    }

    /// How much of the requested window the samples cover, 0.0–1.0.
    pub fn coverage(&self) -> f64 {
        if self.requested_ms <= 0.0 {
            1.0
        } else {
            (self.window_ms / self.requested_ms).clamp(0.0, 1.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Intervals in nanoseconds from a list in milliseconds.
    fn ns(values: &[f64]) -> Vec<u64> {
        values.iter().map(|ms| (*ms * 1e6).round() as u64).collect()
    }

    /// The same values as the statistics take them: milliseconds, via the
    /// reader's own nanosecond-to-millisecond step, so the tests exercise the
    /// conversion the ring feeds.
    fn ms(values: &[f64]) -> Vec<f64> {
        ns(values)
            .iter()
            .filter_map(|interval| to_ms(*interval))
            .collect()
    }

    fn close(got: f64, want: f64, tol: f64, what: &str) {
        assert!(
            (got - want).abs() <= tol,
            "{what}: got {got}, want {want} (±{tol})"
        );
    }

    fn stats_of(values: &[f64]) -> Metrics {
        metrics(&ms(values), 0.0).expect("usable samples")
    }

    // ---- the reference's own self-test cases, on the Rust side ----

    #[test]
    fn a_constant_interval_stream_is_exactly_its_own_fps_everywhere() {
        // The reference's "constant 16.67 ms stream": 60 fps in every statistic,
        // including both lows, because the worst 1 % is a 16.67 ms frame too.
        let st = stats_of(&[16.67; 600]);
        close(st.avg_fps, 60.0, 0.2, "average fps");
        close(st.frametime_ms, 16.67, 0.01, "frametime_ms");
        close(st.low_1pct, 60.0, 0.2, "1 % low");
        close(st.low_01pct, 60.0, 0.2, "0.1 % low");
        assert_eq!(st.samples, 600);
        close(st.window_ms, 16.67 * 600.0, 1.0, "window");
    }

    #[test]
    fn the_1_to_100_ms_ramp_finds_the_slow_end_and_not_the_fast_one() {
        // The reference's tripwire: on this ramp `low_percent(1 %)` must be the
        // worst frame (100 ms → 10 fps). A "slow end" indexing bug returns 2 ms
        // → 500 fps, and this test is what notices.
        let ramp: Vec<f64> = (1..=100).map(|ms| ms as f64).collect();
        let st = stats_of(&ramp);
        close(st.low_1pct, 10.0, 0.01, "1 % low of the ramp");
        close(st.low_01pct, 10.0, 0.01, "0.1 % low of the ramp");
        close(st.frametime_ms, 50.5, 1e-9, "mean of 1..100");
        close(st.avg_fps, 1000.0 / 50.5, 1e-9, "average fps of the ramp");
    }

    #[test]
    fn isolated_long_frames_land_in_both_lows_but_do_not_move_the_mean_much() {
        // 590 ordinary frames and 10 hitches: the reference's "hitch" case, where
        // the tail is exactly 1 % of the run.
        let mut values = vec![16.67; 590];
        values.extend(std::iter::repeat(200.0).take(10));
        let st = stats_of(&values);
        close(st.low_1pct, 5.0, 1e-9, "1 % low with a 1 % tail");
        close(st.low_01pct, 5.0, 1e-9, "0.1 % low with a 1 % tail");
        close(
            st.frametime_ms,
            (590.0 * 16.67 + 10.0 * 200.0) / 600.0,
            1e-6,
            "mean",
        );
        close(st.avg_fps, 50.7, 0.05, "average fps");
    }

    #[test]
    fn a_mixed_tail_separates_the_worst_n_average_from_the_p99() {
        // 995 fast frames and 5 slow ones: the worst 1 % (10 frames) mixes fast
        // and slow, so the B reading (18.18 fps) is not the A reading
        // (1000 / p99 = 10 fps). The panel shows B, because that is what the
        // reference calls `low_percent`.
        let mut values = vec![10.0; 995];
        values.extend(std::iter::repeat(100.0).take(5));
        let st = stats_of(&values);
        close(
            st.low_1pct,
            1000.0 / 55.0,
            1e-9,
            "1 % low, worst-10 average",
        );
        close(st.low_01pct, 10.0, 1e-9, "0.1 % low picks the single worst");
        assert!(
            (st.low_1pct - 10.0).abs() > 1.0,
            "the B and A readings must be distinguishable here"
        );
    }

    #[test]
    fn a_thin_tail_does_not_move_the_higher_percentiles() {
        // The reference's "must not invent a problem" case: 0.5 % of slow frames
        // is outside the worst 1 %? No -- it is inside it, but it leaves the
        // mean almost unchanged. What it must not do is move the 0.1 % low off
        // the slow frames it does contain.
        let mut values = vec![10.0; 995];
        values.extend(std::iter::repeat(100.0).take(5));
        let st = stats_of(&values);
        close(st.frametime_ms, 10.45, 1e-9, "mean barely moves");
        close(st.low_01pct, 10.0, 1e-9, "0.1 % low is the slow frame");
    }

    #[test]
    fn small_sample_counts_use_the_reference_max_one_rule() {
        // n = 1: every statistic is that frame. n = 3: ceil(0.1 % × 3) = 1, so the
        // 0.1 % low is the single worst frame, and ceil(1 % × 3) = 1 as well.
        let st = stats_of(&[16.0]);
        assert_eq!(st.samples, 1);
        close(st.avg_fps, 62.5, 1e-9, "one frame");
        close(st.low_1pct, 62.5, 1e-9, "1 % low of one frame");
        close(st.low_01pct, 62.5, 1e-9, "0.1 % low of one frame");

        let st = stats_of(&[10.0, 20.0, 30.0]);
        assert_eq!(st.samples, 3);
        close(st.frametime_ms, 20.0, 1e-9, "mean of three");
        close(st.low_1pct, 1000.0 / 30.0, 1e-9, "1 % low of three");
        close(st.low_01pct, 1000.0 / 30.0, 1e-9, "0.1 % low of three");

        let st = stats_of(&[10.0, 20.0]);
        close(st.low_1pct, 50.0, 1e-9, "1 % low of two");
    }

    #[test]
    fn unwritten_slots_and_unusable_values_are_dropped_not_counted_as_frames() {
        // The reference drops non-positive frame times; a ring that has not been
        // written yet is full of zeros, and counting them as 0 ms frames would
        // make every mean meaningless.
        assert_eq!(to_ms(0), None);
        let values = vec![0u64, 1_000_000, 0, 2_000_000, 0];
        let w = window(&values, 10_000.0);
        assert_eq!(w, vec![1.0, 2.0]);

        let st = metrics(&[0.0, 16.0, 0.0], 0.0).expect("one usable sample");
        assert_eq!(st.samples, 1);
        close(st.avg_fps, 62.5, 1e-9, "only the real frame counts");
    }

    #[test]
    fn a_window_with_no_usable_frame_has_no_metrics_at_all() {
        // `—`, not 0.0 fps and not an infinity: the elements must not invent a
        // measurement out of an empty ring.
        assert_eq!(metrics(&[], 0.0), None);
        assert_eq!(metrics(&[0.0, 0.0, 0.0], 0.0), None);
        assert_eq!(metrics(&[f64::NAN, f64::INFINITY, -1.0], 0.0), None);
    }

    // ---- the window ----

    #[test]
    fn the_window_keeps_the_newest_intervals_that_cover_its_span() {
        // 10 ms frames, a 25 ms window: the newest 3 samples cover it (the third
        // takes the total to exactly 30 ms), and the fourth is outside.
        let values = ns(&[1.0, 2.0, 3.0, 4.0, 10.0, 10.0, 10.0]);
        let w = window(&values, 25.0);
        assert_eq!(w, vec![10.0, 10.0, 10.0]);

        // A window longer than the history takes everything, oldest first.
        let w = window(&values, 1000.0);
        assert_eq!(w, vec![1.0, 2.0, 3.0, 4.0, 10.0, 10.0, 10.0]);
    }

    #[test]
    fn the_window_is_a_time_window_and_not_a_sample_count() {
        // The same interval count means different durations at different frame
        // rates, which is exactly why Q9 chose seconds: 10 s of a 240 fps game is
        // 2400 samples, 10 s of a 60 fps game is 600.
        let fast = ns(&vec![1000.0 / 240.0; 3000]);
        let slow = ns(&vec![1000.0 / 60.0; 3000]);
        assert_eq!(window(&fast, 10_000.0).len(), 2400);
        assert_eq!(window(&slow, 10_000.0).len(), 600);
    }

    #[test]
    fn a_single_sample_window_still_produces_a_measurement() {
        // The first frames after the layer activates: one or two intervals. The
        // elements show a number rather than `—` as soon as there is one frame,
        // which is what makes the panel come alive instead of waiting 10 s.
        let values = ns(&[8.0, 9.0]);
        let w = window(&values, 10_000.0);
        let st = metrics(&w, 0.0).expect("two frames");
        assert_eq!(st.samples, 2);
        close(st.frametime_ms, 8.5, 1e-9, "mean of two");
        close(st.low_1pct, 1000.0 / 9.0, 1e-9, "worst of two");
    }

    // ---- window coverage ----

    #[test]
    fn a_window_the_history_fills_is_not_reported_as_short() {
        // 1250 intervals of 8 ms is exactly the 10 s window the config asks for, and
        // the walk takes the sample that crosses the boundary, so coverage is full
        // and the requested span is not reported as missing.
        let values = vec![8.0f64; 1250];
        let full = metrics(&ms(&values), 10_000.0).expect("samples");
        assert_eq!(full.samples, 1250);
        assert!(!full.is_short(), "{full:?}");
        assert_eq!(full.coverage(), 1.0);
        assert_eq!(full.requested_ms, 10_000.0);
    }

    #[test]
    fn a_window_longer_than_the_history_is_reported_as_short_not_presented_as_full() {
        // The case the reader cannot avoid: the ring holds 4096 intervals, so at
        // 165 fps (6.06 ms) it holds ~24.8 s and a configured 60 s window can never
        // be covered. The numbers are still computed over what exists -- and the
        // result says how much of the window it covers instead of letting a caller
        // present a 24.8 s statistic as a 60 s one.
        let values = vec![6.06f64; 4096];
        let short = metrics(&ms(&values), 60_000.0).expect("samples");
        assert_eq!(short.samples, 4096);
        assert!(short.is_short(), "{short:?}");
        assert!(
            (short.window_ms - 4096.0 * 6.06).abs() < 0.5,
            "the covered span is the samples', not the request: {}",
            short.window_ms
        );
        assert!(
            (short.coverage() - 0.4136).abs() < 0.01,
            "coverage {}",
            short.coverage()
        );

        // A game one second old is the same report, for the other reason.
        let fresh = metrics(&ms(&vec![8.0f64; 125]), 10_000.0).expect("samples");
        assert!(fresh.is_short());
        assert!((fresh.coverage() - 0.1).abs() < 0.01);
    }

    #[test]
    fn the_pure_statistics_entry_point_records_no_window() {
        // `metrics()` has no config to read, so it must not claim coverage either
        // way: `is_short` is false and `coverage` is 1.0 there, and only the
        // poller's `metrics` call carries the request.
        let plain = metrics(&ms(&vec![8.0f64; 10]), 0.0).expect("samples");
        assert_eq!(plain.requested_ms, 0.0);
        assert!(!plain.is_short());
        assert_eq!(plain.coverage(), 1.0);
    }

    #[test]
    fn a_short_window_does_not_change_the_statistics_themselves() {
        // Reporting a short window must not become a reason to distort the numbers:
        // the same samples give the same fps, frame time and lows whatever the
        // request was.
        let values = vec![16.0f64; 300];
        let plain = metrics(&ms(&values), 0.0).expect("samples");
        let asked = metrics(&ms(&values), 60_000.0).expect("samples");
        assert_eq!(plain.samples, asked.samples);
        assert_eq!(plain.window_ms, asked.window_ms);
        assert_eq!(plain.frametime_ms, asked.frametime_ms);
        assert_eq!(plain.avg_fps, asked.avg_fps);
        assert_eq!(plain.low_1pct, asked.low_1pct);
        assert_eq!(plain.low_01pct, asked.low_01pct);
        assert!(asked.is_short() && !plain.is_short());
    }

    // ---- parity with the reference implementation ----

    #[test]
    fn the_statistics_match_the_reference_script_when_python3_is_available() {
        // Criterion 13's parity requirement, run against the reference itself
        // rather than against a transcription of its answers. The comparison is
        // limited by the script's printed precision (one decimal for fps, three
        // for milliseconds), and the tolerance below is that precision plus one
        // unit in the last place.
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("docs/development/tools/frametime_stats.py");
        if !script.exists() {
            eprintln!("PARITY SKIPPED: {} is not present", script.display());
            return;
        }
        if std::process::Command::new("python3")
            .arg("--version")
            .output()
            .is_err()
        {
            eprintln!("PARITY SKIPPED: no python3 on PATH");
            return;
        }

        // A series that exercises every branch: a warm-up, a steady 60 fps body
        // with jitter, a burst of hitches, a mixed tail, and a few very long
        // frames that the writer itself would have dropped.
        let mut values: Vec<f64> = Vec::new();
        for index in 0..1200 {
            let jitter = ((index % 7) as f64 - 3.0) * 0.05;
            values.push(16.6667 + jitter);
        }
        for index in 0..40 {
            values.push(if index % 3 == 0 { 33.0 } else { 120.0 });
        }
        for index in 0..12 {
            values.push(if index % 2 == 0 { 8.0 } else { 250.0 });
        }

        let dir = std::env::temp_dir().join(format!(
            "lapsphere-frames-parity-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("scratch");
        let csv = dir.join("intervals.csv");
        let body: String = values.iter().map(|ms| format!("{ms:.6}\n")).collect();
        std::fs::write(&csv, body).expect("write csv");

        let output = std::process::Command::new("python3")
            .arg(&script)
            .arg(&csv)
            .output()
            .expect("run the reference script");
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        assert!(
            output.status.success(),
            "the reference script failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let _ = std::fs::remove_dir_all(&dir);

        let reference = |label: &str| -> f64 {
            for line in stdout.lines() {
                let line = line.trim();
                if let Some(rest) = line.strip_prefix(label) {
                    let value = rest.trim().trim_start_matches(':').trim();
                    let number: String = value
                        .chars()
                        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
                        .collect();
                    return number
                        .parse()
                        .unwrap_or_else(|_| panic!("could not parse {label:?} out of {line:?}"));
                }
            }
            panic!("the reference script printed no {label:?} line:\n{stdout}");
        };

        let st = metrics(&ms(&values), 0.0).expect("usable samples");
        close(st.avg_fps, reference("average fps"), 0.06, "average fps");
        close(
            st.frametime_ms,
            reference("frametime mean"),
            0.0006,
            "frametime mean",
        );
        close(
            st.low_1pct,
            reference("1% low  (B, worst)"),
            0.06,
            "1 % low (B)",
        );
        close(
            st.low_01pct,
            reference("0.1% low (B, worst)"),
            0.06,
            "0.1 % low (B)",
        );
    }
}
