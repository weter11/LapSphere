//! Where the frame data comes from: the segment directory, the clock, and which
//! segment the panel reads.
//!
//! # The selection rule (ADR-5, as agreed)
//!
//! > The active process is the segment with the most recent `last_present_ns`,
//! > provided that timestamp is younger than `STALE_AFTER = 2 s`. If no segment
//! > qualifies, the frame elements show `—`. Ties are broken by the larger pid.
//!
//! Both halves are load-bearing:
//!
//! * **most recent present**, not most recently created and not largest ring: only
//!   the present stamp tells a running game apart from a live-but-idle launcher,
//!   and only it follows a game that is the one actually drawing;
//! * **a timeout**, not the writer's `WRITER_ALIVE` flag: the flag survives a
//!   `SIGKILL` (R2 in `spike/vk-layer/TESTING.md` measures exactly that), so
//!   trusting it would leave a dead game's last number frozen on screen for as
//!   long as the panel runs.
//!
//! The clock is `CLOCK_MONOTONIC`, the same one the writer stamps presents with —
//! and on Linux it is the same *system-wide* clock in both processes, which is
//! what makes a comparison between them meaningful. Only `last_present_ns` ever
//! crosses the process boundary: the ring holds intervals, not timestamps, so a
//! clock difference cannot corrupt a single frame time.
//!
//! # What this module deliberately does not do
//!
//! ADR-5 allows the reader to *prune* orphan segments. This reader does not: it
//! opens segments read-only, unlinks nothing and never writes to another
//! process's file. An orphan is already invisible to the UI after `STALE_AFTER`
//! and costs one `read_dir` entry per poll, which is cheaper than the only
//! failure mode a wrong deletion has — deleting a live game's segment.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::shm::{Refusal, Segment, FILE_PREFIX};

/// A present older than this is not a running game, and its segment is not the
/// source. Chosen in ADR-5 as several times the longest plausible gap between
/// presents in a running game: a 2 s gap *is* a 0.5 fps stall, and showing `—`
/// for it is better than showing a stale number indefinitely.
pub const STALE_AFTER: Duration = Duration::from_secs(2);

/// The directory the writer creates inside `$XDG_RUNTIME_DIR`, mode 0700.
pub const DIR_NAME: &str = "lapsphere";

/// What one directory scan found.
#[derive(Debug, Default)]
pub struct Scan {
    /// Segments that passed protocol validation, ordered by pid so a poll's
    /// result does not depend on `read_dir`'s order.
    pub segments: Vec<Segment>,
    /// Files that were refused, each with the reason and the value behind it.
    /// Kept so the worker can say *why* there is no data instead of "no data".
    pub refusals: Vec<(PathBuf, Refusal)>,
}

/// `$XDG_RUNTIME_DIR/lapsphere`, or `None` when the variable is unset or empty.
///
/// Nothing is created here. This reader never makes the directory: a directory
/// that does not exist means no game is running, and creating it on the GUI's
/// behalf would turn "no data" into "a directory that looks like a source".
pub fn segment_dir() -> Option<PathBuf> {
    segment_dir_in(std::env::var_os("XDG_RUNTIME_DIR"))
}

/// The pure half of [`segment_dir`], so the rule can be tested without mutating
/// the process environment.
pub fn segment_dir_in(runtime_dir: Option<std::ffi::OsString>) -> Option<PathBuf> {
    let root = runtime_dir?;
    if root.is_empty() {
        return None;
    }
    Some(PathBuf::from(root).join(DIR_NAME))
}

/// Open, validate and list every `frames-<pid>` in `dir`.
///
/// A missing directory is an empty scan, not an error: that is the state of every
/// machine on which no instrumented Vulkan application has run.
pub fn scan(dir: &Path) -> Scan {
    let mut scan = Scan::default();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return scan,
        Err(err) => {
            scan.refusals
                .push((dir.to_path_buf(), Refusal::Unreadable(err.to_string())));
            return scan;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|name| name.to_str()) {
            Some(name) => name,
            None => continue,
        };
        if !name.starts_with(FILE_PREFIX) {
            continue;
        }
        match Segment::open(&path) {
            Ok(segment) => scan.segments.push(segment),
            Err(refusal) => scan.refusals.push((path, refusal)),
        }
    }

    scan.segments.sort_by_key(|segment| segment.pid());
    scan
}

/// The index of the segment to read, or `None` when none qualifies.
///
/// `now_ns` is this process's `CLOCK_MONOTONIC` reading; a caller that has one
/// passes it in so a poll compares every candidate against the same instant.
pub fn select_index(segments: &[Segment], now_ns: u64) -> Option<usize> {
    let stale_after = STALE_AFTER.as_nanos() as u64;
    segments
        .iter()
        .enumerate()
        .filter(|(_, segment)| {
            // `saturating_sub` makes a stamp from the future (a clock that moved
            // between the two processes, which monotonic clocks do not) read as
            // "just now" rather than as an enormous age.
            now_ns.saturating_sub(segment.header().last_present_ns) < stale_after
        })
        .max_by_key(|(_, segment)| (segment.header().last_present_ns, segment.pid()))
        .map(|(index, _)| index)
}

/// The segment [`select_index`] chose.
///
/// The poller itself goes through [`select_index`], because it needs to take the
/// chosen segment out of the scan to read it; this is the same rule as a reference,
/// which is what the selection tests state their cases against.
#[cfg_attr(not(test), allow(dead_code))]
pub fn select<'a>(segments: &'a [Segment], now_ns: u64) -> Option<&'a Segment> {
    select_index(segments, now_ns).map(|index| &segments[index])
}

/// `CLOCK_MONOTONIC` in nanoseconds, the clock the writer stamps with.
///
/// `None` on a platform where this reader cannot read that clock — and then there
/// is no source at all, rather than a comparison between two unrelated clocks.
#[cfg(target_os = "linux")]
pub fn now_ns() -> Option<u64> {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // Safety: `clock_gettime` writes one initialized `timespec` through the
    // pointer it is given; it goes through the vDSO and cannot fail for a valid
    // clock id.
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
    }
    Some((ts.tv_sec as u64) * 1_000_000_000 + (ts.tv_nsec as u64))
}

/// See the Linux implementation above; elsewhere the reader reports no source.
#[cfg(not(target_os = "linux"))]
pub fn now_ns() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::frames::fake::{FakeWriter, Scratch};

    /// A segment whose newest present is `age_ns` old, with `frames` intervals in
    /// its ring. The stamp it is given is the one the writer would have taken
    /// `age_ns` before `now_for(frames)`.
    fn seeded(dir: &Path, pid: u32, frames: u64, age_ns: u64) -> FakeWriter {
        let mut fake = FakeWriter::new(dir, pid, 64);
        fake.record_pattern(frames);
        let stamp = FakeWriter::present_ns_for(frames).saturating_sub(age_ns);
        fake.set_last_present_ns(stamp);
        fake
    }

    /// The process's "now" for a segment seeded with `frames` intervals: the
    /// instant at which the seed's `age_ns` is its real age.
    fn now_for(frames: u64) -> u64 {
        FakeWriter::present_ns_for(frames)
    }

    const SECOND: u64 = 1_000_000_000;

    #[test]
    fn a_missing_directory_is_an_empty_scan_and_not_an_error() {
        let scratch = Scratch::new("nosuchdir");
        let scan = scan(&scratch.path().join("absent"));
        assert!(scan.segments.is_empty());
        assert!(
            scan.refusals.is_empty(),
            "a missing directory is not a refusal"
        );
    }

    #[test]
    fn only_frames_pid_files_are_looked_at() {
        let scratch = Scratch::new("filter");
        std::fs::write(scratch.path().join("spike.log"), b"log").expect("write");
        std::fs::write(scratch.path().join("frames-abc"), b"nope").expect("write");
        seeded(scratch.path(), 501, 4, 0);

        let scan = scan(scratch.path());
        assert_eq!(scan.segments.len(), 1);
        assert_eq!(scan.segments[0].pid(), 501);
        // `frames-abc` is a file this reader must say no to, with the name.
        assert_eq!(scan.refusals.len(), 1);
        assert_eq!(
            scan.refusals[0].1,
            Refusal::BadFileName("frames-abc".into())
        );
    }

    #[test]
    fn an_invalid_segment_is_refused_and_named_rather_than_skipped_silently() {
        let scratch = Scratch::new("invalid");
        let mut broken = FakeWriter::new(scratch.path(), 601, 64);
        broken.record_pattern(4);
        broken.patch(8, &9u32.to_le_bytes()); // version 9
        seeded(scratch.path(), 602, 4, 0);

        let scan = scan(scratch.path());
        assert_eq!(scan.segments.len(), 1, "the valid one is still usable");
        assert_eq!(scan.segments[0].pid(), 602);
        assert_eq!(scan.refusals.len(), 1);
        assert_eq!(scan.refusals[0].1, Refusal::UnsupportedVersion(9));
    }

    #[test]
    fn the_most_recent_present_wins_even_when_its_pid_is_the_smaller_one() {
        // The rule's whole point, arranged so that the two halves cannot agree by
        // accident: the larger pid here is the OLDER segment. A "largest pid"
        // implementation passes every other test in this module and fails this one.
        let scratch = Scratch::new("recent");
        seeded(scratch.path(), 900, 20, 1_500_000_000);
        seeded(scratch.path(), 100, 20, 500_000_000);

        let scan = scan(scratch.path());
        assert_eq!(scan.segments.len(), 2);
        let chosen = select(&scan.segments, now_for(20)).expect("a fresh segment");
        assert_eq!(
            chosen.pid(),
            100,
            "the newest present wins; the pid is only a tie-break"
        );
    }

    #[test]
    fn a_tie_on_the_timestamp_is_broken_by_the_larger_pid() {
        let scratch = Scratch::new("tie");
        for pid in [200u32, 700, 400] {
            seeded(scratch.path(), pid, 10, 100_000_000);
        }

        let scan = scan(scratch.path());
        assert_eq!(scan.segments.len(), 3);
        assert_eq!(
            select(&scan.segments, now_for(10)).map(|s| s.pid()),
            Some(700)
        );
    }

    #[test]
    fn a_stale_segment_is_not_a_source_and_a_fresh_one_is() {
        // Exactly at the bound and beyond it: both stale. The comparison is
        // "< STALE_AFTER", so a game that stopped presenting 2.0 s ago shows `—`.
        let scratch = Scratch::new("stale");
        let stamp = FakeWriter::present_ns_for(10);
        seeded(scratch.path(), 300, 10, 0);

        let scan = scan(scratch.path());
        let stale_after = STALE_AFTER.as_nanos() as u64;

        assert!(
            select(&scan.segments, stamp).is_some(),
            "a present happening now is a source"
        );
        assert!(
            select(&scan.segments, stamp + stale_after - 1).is_some(),
            "one nanosecond short of the bound is still fresh"
        );
        assert!(
            select(&scan.segments, stamp + stale_after).is_none(),
            "exactly STALE_AFTER old is stale"
        );
        assert!(
            select(&scan.segments, stamp + stale_after * 100).is_none(),
            "a segment this old is not a source"
        );
    }

    #[test]
    fn a_segment_with_no_fresh_candidate_yields_nothing_rather_than_the_nearest() {
        // "No source" must not degrade into "the least stale source": a frozen
        // number is exactly what the timeout exists to prevent.
        let scratch = Scratch::new("none");
        // 3000 frames put the seed's stamp at ~31000 s of synthetic clock, so a
        // 30 s age is a real age rather than a floor at zero.
        seeded(scratch.path(), 350, 3000, 30 * SECOND);
        let scan = scan(scratch.path());
        assert!(select(&scan.segments, now_for(3000)).is_none());
    }

    #[test]
    fn a_stamp_from_the_future_reads_as_fresh() {
        // Monotonic clocks do not move backwards, but a segment written by another
        // process is not ours to assume about: `saturating_sub` keeps the age at
        // zero instead of wrapping to an enormous one.
        let scratch = Scratch::new("future");
        seeded(scratch.path(), 360, 10, 0);
        let scan = scan(scratch.path());
        assert!(select(&scan.segments, 0).is_some());
    }

    #[test]
    fn the_selection_is_deterministic_across_repeated_scans() {
        // Same instant, same three segments, five scans: read_dir's order must not
        // decide the answer.
        let scratch = Scratch::new("deterministic");
        for pid in [11u32, 12, 13] {
            seeded(scratch.path(), pid, 10, 200_000_000);
        }
        let mut seen = Vec::new();
        for _ in 0..5 {
            let scan = scan(scratch.path());
            seen.push(select(&scan.segments, now_for(10)).map(|s| s.pid()));
        }
        assert_eq!(seen, vec![Some(13); 5], "tie, then the larger pid");
    }

    #[test]
    fn the_segment_directory_is_xdg_runtime_dir_lapsphere() {
        // Read from the environment, never created: the reader must not leave a
        // directory behind on a machine with no instrumented game.
        assert_eq!(
            segment_dir_in(Some("/run/user/4242".into())),
            Some(PathBuf::from("/run/user/4242").join(DIR_NAME))
        );
        assert_eq!(segment_dir_in(None), None, "no variable, no directory");
        assert_eq!(
            segment_dir_in(Some("".into())),
            None,
            "an empty variable is not a directory"
        );
        // And the real one follows the real environment.
        let from_env = segment_dir();
        match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(value) if !value.is_empty() => {
                assert_eq!(from_env, Some(PathBuf::from(value).join(DIR_NAME)))
            }
            _ => assert_eq!(from_env, None),
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_clock_is_monotonic_and_moves_forward() {
        let first = now_ns().expect("linux clock");
        std::thread::sleep(Duration::from_millis(5));
        let second = now_ns().expect("linux clock");
        assert!(second > first, "{second} must be after {first}");
        assert!(
            second - first >= 5_000_000,
            "five milliseconds of sleep must show as at least that"
        );
    }
}
