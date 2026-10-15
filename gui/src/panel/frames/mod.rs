//! Frame metrics in the panel: the six first-release elements and the reader
//! behind them.
//!
//! The elements are `game_name`, `fps`, `frametime_ms`, `frametime_graph`,
//! `low_1pct` and `low_01pct` — the owner's decision recorded in
//! `docs/development/panel-v1-baseline.md` §4, which is taken as settled. This
//! module is the integration: the registry rows live in [`super::rows`], the
//! layout reservation in [`super::layout`], and the numbers here.
//!
//! # Where the data comes from
//!
//! Nowhere the GUI can poll over D-Bus. An external process cannot see a game's
//! present, so the data is written from **inside** the game process by the Vulkan
//! implicit layer into `$XDG_RUNTIME_DIR/lapsphere/frames-<pid>`, and this module
//! reads that file. There is no daemon method, no D-Bus object, no protocol change
//! and no fabricated fallback: if the segment is absent, unreadable, invalid or
//! stale, every frame element renders as unavailable.
//!
//! * [`shm`] — protocol v1, read-only, with the validation order and the seqlock.
//! * [`source`] — the directory, `CLOCK_MONOTONIC`, and ADR-5's selection rule.
//! * [`stats`] — the statistics, ported from the reference definitions.
//! * [`graph`] — the fixed-height strip.
//!
//! # Threading and cost
//!
//! A single worker thread polls at [`super::config::DEFAULT_REFRESH_HZ`] (4 Hz by
//! default) and publishes one immutable [`FrameSnapshot`] per poll. The UI thread
//! only clones that snapshot — no file is opened, read or stat'd on the rendering
//! path, and the panel's repaint cadence is the poll cadence, never the game's
//! frame rate. The worker does **no** filesystem work at all while it is inactive
//! ([`Frames::set_active`]), which is the state when the panel is not on screen.
//!
//! # What is claimed, and what is not
//!
//! The activation requirement, verbatim from the layer's manifest
//! (`spike/vk-layer/manifests/VK_LAYER_LAPSPHERE_frames.json` and its
//! `install.sh`): the manifest is an **implicit** layer, so it must live in the
//! loader's implicit search path (`~/.local/share/vulkan/implicit_layer.d/`, or
//! `/usr/share/vulkan/implicit_layer.d/` system-wide) — `VK_LAYER_PATH` is not
//! enough, because `enable_environment` is only honoured for implicit layers — and
//! it is activated per process by `LAPSPHERE_FRAMES=1` (disabled by
//! `LAPSPHERE_FRAMES_DISABLE=1`). Nothing here installs it: this change adds no
//! global configuration and no system-wide file.
//!
//! Therefore, on the first release as it stands:
//!
//! * **Vulkan only.** An OpenGL game, or a 32-bit game, writes no segment and the
//!   elements show `—` — not because they are unsupported by the panel, but
//!   because they are not claimed supported by the layer.
//! * **No Proton/DXVK, no anti-cheat, no native-Wayland presentation claim.** All
//!   three are `NOT TESTED` in the spike; see `docs/development/panel-frames.md`
//!   for the evidence table and `spike/vk-layer/TESTING.md` for the measurements.
//! * **No live end-to-end run of this reader against a real game is recorded for
//!   this commit.** What is recorded is the unit-test evidence in this module and
//!   its neighbours, plus the spike's own measurements of the layer. The live
//!   acceptance run is stage 7's business and is not claimed here.
//!
//! # The two configurable proposals
//!
//! `window_seconds` and `refresh_hz` are Q9's and Q10's **proposed** defaults (10 s
//! and 4 Hz), used as configurable values because no owner decision records them.
//! They are documented as proposals on the panel settings surface as well, so the
//! assumption is visible to the user and not only to a reviewer.

pub mod graph;
pub mod shm;
pub mod source;
pub mod stats;

#[cfg(test)]
mod fake;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

pub use stats::Metrics;

use super::config::{PanelConfig, DEFAULT_REFRESH_HZ};

/// The six first-release elements, in the order the owner's decision lists them.
///
/// The registry owns the ids; this list is what the tests and the repaint rule
/// use to ask "is a frame element on screen", so it must stay in step with
/// [`super::rows::Group::Frame`]. `the_element_list_matches_the_registry_group`
/// in `rows` holds the two together.
pub const ELEMENT_IDS: [&str; 6] = [
    "game_name",
    "fps",
    "frametime_ms",
    "frametime_graph",
    "low_1pct",
    "low_01pct",
];

/// Wait between polls while the reader is inactive.
///
/// The thread stays alive (so no activation pays for a thread start) but touches
/// no file, no directory and no clock.
const INACTIVE_POLL_MS: u64 = 200;

/// Longest single sleep, so a stop request or a cadence change is noticed within
/// this bound rather than within a whole poll interval.
const SLEEP_SLICE_MS: u64 = 50;

/// The panel's repaint cadence in panel mode, from the config.
///
/// Without a frame element on screen the panel keeps its own 500 ms timer. With
/// one, the cadence is the poll cadence: the panel must not repaint at the game's
/// frame rate, and it must not repaint faster than the numbers can change either.
pub fn repaint_interval(config: &PanelConfig) -> Duration {
    if any_element_visible(config) {
        Duration::from_millis(poll_interval_ms(config.refresh_hz))
    } else {
        Duration::from_millis(500)
    }
}

/// The poll interval for a configured refresh rate, clamped to the range
/// `normalize` enforces.
pub fn poll_interval_ms(refresh_hz: u32) -> u64 {
    let hz = refresh_hz.clamp(super::config::MIN_REFRESH_HZ, super::config::MAX_REFRESH_HZ);
    (1000 / hz as u64).max(1)
}

/// Is any of the six frame elements switched on?
pub fn any_element_visible(config: &PanelConfig) -> bool {
    super::config::visible_row_ids(config)
        .iter()
        .any(|id| ELEMENT_IDS.contains(id))
}

/// Which process the numbers came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub pid: u32,
    /// The writer's `/proc/self/comm`, trimmed — the `game_name` element.
    pub exe_name: String,
}

/// One poll's result. Immutable, and cheap to clone: the strip's samples are
/// behind an `Arc`, so handing it to the renderer copies a pointer.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameSnapshot {
    /// `None` when no fresh segment qualified: no game, or one that stopped.
    pub source: Option<Source>,
    /// `None` when the window held no usable frame, even if a source exists (a
    /// segment that has just been created writes its first interval one present
    /// later).
    pub metrics: Option<Metrics>,
    /// The strip's columns in milliseconds, oldest first, at most
    /// [`graph::GRAPH_COLUMNS`]. Empty when there is nothing to draw.
    ///
    /// One value per interval until the window is wider than the strip, and the
    /// worst interval of each bucket after that — see [`graph::series`]. Held in an
    /// `Arc` so handing one snapshot to every element of a frame copies a pointer.
    pub samples_ms: Arc<[f32]>,
    /// The largest number of intervals one column of [`FrameSnapshot::samples_ms`]
    /// stands for: `1` when every interval has its own column.
    pub column_span: usize,
}

impl Default for FrameSnapshot {
    fn default() -> Self {
        FrameSnapshot {
            source: None,
            metrics: None,
            samples_ms: Arc::from(Vec::new()),
            column_span: 0,
        }
    }
}

/// What the reader is currently reading, as the thing the log announces.
///
/// **Identity, not activity.** The variant carries no sample count, no frame rate
/// and no timestamp on purpose: a key that moved with the window's contents would
/// announce a "change" on every poll — during warm-up the window grows on each one,
/// and afterwards the sample count drifts with the game's frame rate — and a log
/// that fires four times a second while nothing is wrong is a log nobody reads.
/// What it does carry is enough to tell the states apart that matter: no source
/// (with the reason), a source appearing, a source being read, and a *different*
/// source taking over.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Availability {
    /// No usable source, with the reason it is unusable.
    Absent(String),
    /// This pid is being read.
    Live { pid: u32 },
}

/// The reader's state machine: it turns a directory into a [`FrameSnapshot`] and
/// announces each change of availability **once**.
///
/// Split out of the thread so the whole rule is testable without one.
#[derive(Debug, Default)]
pub struct Poller {
    announced: Option<Availability>,
    announcements: u64,
}

impl Poller {
    /// One poll. `now_ns` is this process's `CLOCK_MONOTONIC` reading, passed in
    /// so every candidate is compared against the same instant.
    pub fn poll(
        &mut self,
        dir: Option<&Path>,
        window_seconds: u32,
        now_ns: Option<u64>,
    ) -> FrameSnapshot {
        let Some(dir) = dir else {
            self.announce(Availability::Absent("XDG_RUNTIME_DIR is unset".to_string()));
            return FrameSnapshot::default();
        };
        let Some(now_ns) = now_ns else {
            self.announce(Availability::Absent(
                "no CLOCK_MONOTONIC in this process".to_string(),
            ));
            return FrameSnapshot::default();
        };

        let mut scan = source::scan(dir);
        let Some(index) = source::select_index(&scan.segments, now_ns) else {
            self.announce(Availability::Absent(describe(&scan)));
            return FrameSnapshot::default();
        };

        // The selected segment, moved out of the scan: `read` needs it mutably, and
        // the other candidates are of no further interest this poll.
        let mut segment = scan.segments.swap_remove(index);
        let reading = match segment.read(shm::MAX_RING_CAPACITY, shm::READ_RETRIES) {
            Ok(reading) => reading,
            Err(refusal) => {
                // A segment that was valid at scan time and refuses to be read a
                // moment later is a writer that is unlinking, or a `kill -9` in
                // flight: no data this poll, and not a zero to show instead.
                self.announce(Availability::Absent(format!(
                    "{}: {refusal}",
                    segment.path().display()
                )));
                return FrameSnapshot::default();
            }
        };

        // The configured window, in milliseconds: what the statistics are computed
        // over when the history covers it, and what the result reports against when
        // it does not (`Metrics::is_short`).
        let requested_ms = window_seconds as f64 * 1000.0;
        let window = stats::window(&reading.intervals, requested_ms);
        // The strip covers the same window as the numbers: the whole sample set,
        // folded into the strip's fixed column count with its peaks intact.
        let series = graph::series(&window);
        let snapshot = FrameSnapshot {
            source: Some(Source {
                pid: reading.header.pid,
                exe_name: display_name(&reading.header.exe_name, reading.header.pid),
            }),
            metrics: stats::metrics(&window, requested_ms),
            samples_ms: Arc::from(series.columns),
            column_span: series.span,
        };

        self.announce(Availability::Live {
            pid: reading.header.pid,
        });
        snapshot
    }

    /// How many availability changes have been announced. The log contract is
    /// "one line per change", so this number is the thing the tests hold.
    #[cfg(test)]
    pub fn announcements(&self) -> u64 {
        self.announcements
    }

    fn announce(&mut self, state: Availability) {
        if self.announced.as_ref() == Some(&state) {
            return;
        }
        match &state {
            Availability::Absent(reason) => {
                log::debug!("panel frames: no source ({reason})");
            }
            Availability::Live { pid } => {
                log::info!("panel frames: reading pid {pid}");
            }
        }
        self.announced = Some(state);
        self.announcements += 1;
    }
}

/// A refused segment is worth naming: "no data" and "a v2 segment" are different
/// problems for whoever has to fix it.
fn describe(scan: &source::Scan) -> String {
    if scan.segments.is_empty() {
        if scan.refusals.is_empty() {
            return "no segments".to_string();
        }
        let (path, refusal) = &scan.refusals[0];
        return format!(
            "{} segment(s) refused, first: {} — {refusal}",
            scan.refusals.len(),
            path.display()
        );
    }
    format!("all {} segment(s) stale", scan.segments.len())
}

/// The `game_name` text: the writer's comm, or the pid when it is empty (the
/// writer reads `/proc/self/comm` and may have nothing to read).
fn display_name(exe_name: &str, pid: u32) -> String {
    if exe_name.is_empty() {
        format!("pid {pid}")
    } else {
        exe_name.to_string()
    }
}

/// The GUI's frame-metrics reader: one worker thread plus the last snapshot.
///
/// [`Frames::offline`] is the default and starts nothing, which is what the unit
/// tests and a process that never enters panel mode want.
pub struct Frames {
    shared: Arc<Shared>,
    worker: Option<Worker>,
}

struct Shared {
    snapshot: Mutex<FrameSnapshot>,
    dir: Option<PathBuf>,
    active: AtomicBool,
    window_seconds: AtomicU32,
    refresh_hz: AtomicU32,
}

struct Worker {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

impl Default for Frames {
    fn default() -> Self {
        Frames::offline()
    }
}

impl Frames {
    /// A reader with no worker: every element reads as unavailable.
    pub fn offline() -> Frames {
        Frames::offline_in(source::segment_dir())
    }

    /// The same, with the directory named explicitly. Used by the tests, which
    /// must not read the developer's own `$XDG_RUNTIME_DIR`.
    pub fn offline_in(dir: Option<PathBuf>) -> Frames {
        Frames {
            shared: Arc::new(Shared {
                snapshot: Mutex::new(FrameSnapshot::default()),
                dir,
                active: AtomicBool::new(false),
                window_seconds: AtomicU32::new(super::config::DEFAULT_WINDOW_SECONDS),
                refresh_hz: AtomicU32::new(DEFAULT_REFRESH_HZ),
            }),
            worker: None,
        }
    }

    /// Start the worker, once. Idempotent: a second call does nothing, so it can be
    /// called from the frame loop.
    pub fn start(&mut self) {
        if self.worker.is_some() {
            return;
        }
        let shared = Arc::clone(&self.shared);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let handle = std::thread::Builder::new()
            .name("panel-frames".to_string())
            .spawn(move || run(shared, worker_stop))
            .map_err(|err| log::warn!("panel frames: could not start the reader thread: {err}"))
            .ok();
        self.worker = handle.map(|handle| Worker { stop, handle });
    }

    /// Let the worker do filesystem work, or stop it doing any.
    ///
    /// One atomic store, called from the frame loop: while the panel is not on
    /// screen there is nothing to read, and a thread that kept polling every 250 ms
    /// would be a background process poking the filesystem for a surface nobody is
    /// looking at.
    pub fn set_active(&self, active: bool) {
        self.shared.active.store(active, Ordering::Relaxed);
    }

    /// Follow the config's statistics window and refresh cadence.
    pub fn configure(&self, window_seconds: u32, refresh_hz: u32) {
        self.shared
            .window_seconds
            .store(window_seconds, Ordering::Relaxed);
        self.shared.refresh_hz.store(refresh_hz, Ordering::Relaxed);
    }

    /// The last published snapshot. Never blocks on the filesystem: the worker has
    /// already done the reading.
    pub fn snapshot(&self) -> FrameSnapshot {
        let guard = self
            .shared
            .snapshot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.clone()
    }

    /// Is the worker running? (Started, not necessarily doing work.)
    pub fn is_running(&self) -> bool {
        self.worker.is_some()
    }

    /// Publish a snapshot directly, with no worker and no segment file.
    ///
    /// Test-only: the registry's tests need "what do the six rows print when there
    /// IS data" without a live game, and a real [`FrameSnapshot`] is the only thing
    /// allowed to reach a row — there is no test-only shortcut through the rows.
    #[cfg(test)]
    pub fn publish_for_test(&self, snapshot: FrameSnapshot) {
        publish(&self.shared, snapshot);
    }
}

impl Drop for Frames {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop.store(true, Ordering::Relaxed);
            // Bounded: the loop wakes at least every [`SLEEP_SLICE_MS`].
            let _ = worker.handle.join();
        }
    }
}

fn publish(shared: &Shared, snapshot: FrameSnapshot) {
    let mut guard = shared
        .snapshot
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = snapshot;
}

fn run(shared: Arc<Shared>, stop: Arc<AtomicBool>) {
    let mut poller = Poller::default();
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        if shared.active.load(Ordering::Relaxed) {
            let snapshot = poller.poll(
                shared.dir.as_deref(),
                shared.window_seconds.load(Ordering::Relaxed),
                source::now_ns(),
            );
            publish(&shared, snapshot);
            sleep_slices(
                poll_interval_ms(shared.refresh_hz.load(Ordering::Relaxed)),
                &stop,
            );
        } else {
            // Nothing to read while the panel is off screen. The snapshot is
            // cleared so that re-entering panel mode cannot show a number that was
            // true a minute ago, however briefly.
            let stale = {
                let guard = shared
                    .snapshot
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *guard != FrameSnapshot::default()
            };
            if stale {
                publish(&shared, FrameSnapshot::default());
            }
            sleep_slices(INACTIVE_POLL_MS, &stop);
        }
    }
}

/// Sleep `total_ms`, in slices, so a stop or a cadence change is noticed quickly.
fn sleep_slices(total_ms: u64, stop: &AtomicBool) {
    let mut left = total_ms;
    while left > 0 && !stop.load(Ordering::Relaxed) {
        let slice = left.min(SLEEP_SLICE_MS);
        std::thread::sleep(Duration::from_millis(slice));
        left -= slice;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::config::PanelConfig;
    use crate::panel::frames::fake::{FakeWriter, Scratch};
    use crate::panel::rows;
    use std::sync::atomic::AtomicBool;

    const SECOND: u64 = 1_000_000_000;

    fn seeded(dir: &Path, pid: u32, frames: u64, age_ns: u64) -> FakeWriter {
        let mut fake = FakeWriter::new(dir, pid, 256);
        fake.record_pattern(frames);
        let stamp = FakeWriter::present_ns_for(frames).saturating_sub(age_ns);
        fake.set_last_present_ns(stamp);
        fake
    }

    fn now_for(frames: u64) -> u64 {
        FakeWriter::present_ns_for(frames)
    }

    // ---- the snapshot ----

    #[test]
    fn a_live_segment_produces_the_game_name_the_numbers_and_the_strip() {
        let scratch = Scratch::new("live");
        // 120 frames of 8 ms: 125 fps everywhere.
        let mut fake = FakeWriter::new(scratch.path(), 7001, 256);
        fake.record(120, 8_000_000);
        fake.set_last_present_ns(now_for(0));

        let mut poller = Poller::default();
        let snapshot = poller.poll(
            Some(scratch.path()),
            10,
            Some(FakeWriter::present_ns_for(0)),
        );

        let source = snapshot.source.expect("a source");
        assert_eq!(source.pid, 7001);
        assert_eq!(source.exe_name, "vkcube", "the writer's comm is game_name");
        let metrics = snapshot.metrics.expect("numbers");
        assert_eq!(metrics.samples, 120);
        assert!((metrics.avg_fps - 125.0).abs() < 1e-6, "{metrics:?}");
        assert!((metrics.low_1pct - 125.0).abs() < 1e-6, "{metrics:?}");
        assert_eq!(snapshot.samples_ms.len(), 120);
        assert_eq!(snapshot.column_span, 1, "120 intervals, 120 columns");
        assert!(snapshot.samples_ms.iter().all(|ms| *ms == 8.0));
        assert!(snapshot.metrics.is_some());
    }

    #[test]
    fn an_empty_game_name_falls_back_to_the_pid_rather_than_an_empty_row() {
        let scratch = Scratch::new("noname");
        let mut fake = FakeWriter::new(scratch.path(), 7002, 256);
        fake.record(10, 8_000_000);
        // The writer copies /proc/self/comm; an empty one is possible.
        for offset in 24..88 {
            fake.patch(offset, &[0u8]);
        }
        fake.set_last_present_ns(now_for(0));

        let mut poller = Poller::default();
        let snapshot = poller.poll(
            Some(scratch.path()),
            10,
            Some(FakeWriter::present_ns_for(0)),
        );
        assert_eq!(snapshot.source.expect("a source").exe_name, "pid 7002");
    }

    #[test]
    fn a_missing_directory_still_produces_unavailable_values() {
        let scratch = Scratch::new("nodir");
        let mut poller = Poller::default();
        let snapshot = poller.poll(Some(&scratch.path().join("absent")), 10, Some(now_for(0)));
        assert_eq!(snapshot, FrameSnapshot::default());
        assert!(!snapshot.metrics.is_some());
    }

    #[test]
    fn no_runtime_directory_and_no_clock_are_unavailable_rather_than_zero() {
        // The two ways the reader can have nowhere to look. Both must produce the
        // absent snapshot: no source, no metrics, no samples.
        let mut poller = Poller::default();
        assert_eq!(poller.poll(None, 10, Some(1)), FrameSnapshot::default());
        let scratch = Scratch::new("noclk");
        assert_eq!(
            poller.poll(Some(scratch.path()), 10, None),
            FrameSnapshot::default()
        );
        assert!(poller.announcements() >= 2);
    }

    #[test]
    fn a_stale_segment_reads_as_absent_and_not_as_its_last_number() {
        // One game exiting must not leave a number frozen on screen.
        let scratch = Scratch::new("exited");
        seeded(scratch.path(), 7003, 120, 5 * SECOND);
        let mut poller = Poller::default();
        let snapshot = poller.poll(Some(scratch.path()), 10, Some(now_for(120)));
        assert_eq!(snapshot, FrameSnapshot::default());
        assert!(snapshot.source.is_none());
    }

    #[test]
    fn a_segment_without_intervals_has_a_name_but_no_numbers() {
        // The layer creates the segment at instance creation and writes its first
        // interval one present later: for that moment the game is named and the
        // numbers are `—`, which is the honest reading of an empty ring.
        let scratch = Scratch::new("nointervals");
        let fake = FakeWriter::new(scratch.path(), 7004, 256);
        assert_eq!(fake.frames(), 0);

        let mut poller = Poller::default();
        let snapshot = poller.poll(Some(scratch.path()), 10, Some(now_for(0)));
        assert_eq!(snapshot.source.expect("named").pid, 7004);
        assert!(snapshot.metrics.is_none());
        assert!(snapshot.samples_ms.is_empty());
        assert_eq!(snapshot.column_span, 0);
        assert!(!snapshot.metrics.is_some());
    }

    #[test]
    fn all_zero_slots_are_not_a_measurement() {
        // A ring the writer has not reached yet is zeros, and zeros are not frame
        // times. `fps = 0.0` and an "infinitely fast" 0 ms frame are both wrong,
        // so there must be no number at all.
        let scratch = Scratch::new("zeros");
        let mut fake = FakeWriter::new(scratch.path(), 7005, 256);
        fake.write_header(7005, 0);
        fake.patch(112, &120u64.to_le_bytes()); // frame_count = 120, ring all zeros

        let mut poller = Poller::default();
        let snapshot = poller.poll(Some(scratch.path()), 10, Some(now_for(0)));
        assert!(snapshot.metrics.is_none(), "{:?}", snapshot.metrics);
        assert!(snapshot.samples_ms.is_empty());
    }

    #[test]
    fn the_window_seconds_chooses_how_much_history_the_statistics_cover() {
        // 8 ms frames, so 10 s is 1250 intervals and 1 s is 125 — the config value
        // is what decides, not the ring's contents.
        let scratch = Scratch::new("window");
        let mut fake = FakeWriter::new(scratch.path(), 7006, 4096);
        fake.record(2000, 8_000_000);
        fake.set_last_present_ns(now_for(0));
        let now = Some(FakeWriter::present_ns_for(0));

        let mut poller = Poller::default();
        let ten = poller.poll(Some(scratch.path()), 10, now);
        let one = poller.poll(Some(scratch.path()), 1, now);
        assert_eq!(ten.metrics.expect("numbers").samples, 1250);
        assert_eq!(one.metrics.expect("numbers").samples, 125);
        // The strip draws the newest 120 either way.
        assert_eq!(one.samples_ms.len(), 120);
        assert_eq!(ten.samples_ms.len(), 120);
        // Both windows are wider than the strip, so both are bucketed: 125 intervals
        // into 120 columns puts two in one of them, 1250 puts eleven.
        assert_eq!(one.column_span, 2);
        assert_eq!(ten.column_span, 11);
    }

    #[test]
    fn the_strip_is_the_newest_intervals_of_a_wrapped_ring() {
        // The ring has wrapped twice. What the reader returns must be the NEWEST
        // slots: a reader that indexes the ring as `capacity - n` (the last n
        // slots) returns unwritten slots, which read as zeros, and a window walk
        // that started at the other end would return the oldest frames instead.
        let scratch = Scratch::new("wrapped");
        let mut fake = FakeWriter::new(scratch.path(), 7007, 256);
        fake.record_pattern(600); // interval n is n ms; wraps a 256-slot ring twice
        fake.set_last_present_ns(now_for(0));
        let now = Some(FakeWriter::present_ns_for(0));

        let mut poller = Poller::default();
        // A one-second window over a series whose newest intervals are ~600 ms: two
        // frames, and they are the newest two.
        let short = poller.poll(Some(scratch.path()), 1, now);
        assert_eq!(short.samples_ms.as_ref(), [599.0f32, 600.0].as_slice());
        assert_eq!(short.metrics.expect("numbers").samples, 2);

        // A constant 8 ms ring in its own directory: a 10 s window covers all 600
        // intervals, so the strip draws its full sample count, and not one of them
        // is a zero from a slot the writer had not reached.
        let flat_scratch = Scratch::new("wrapped-flat");
        let mut flat = FakeWriter::new(flat_scratch.path(), 7008, 4096);
        flat.record(600, 8_000_000);
        flat.set_last_present_ns(now_for(0));
        let long = poller.poll(Some(flat_scratch.path()), 10, now);
        let drawn = &long.samples_ms;
        assert_eq!(drawn.len(), graph::GRAPH_COLUMNS);
        assert_eq!(long.metrics.expect("numbers").samples, 600);
        assert_eq!(long.column_span, 5, "600 intervals into 120 columns");
        assert!(drawn.iter().all(|ms| *ms == 8.0), "no unwritten slots");
    }

    #[test]
    fn the_strip_covers_the_whole_window_and_keeps_an_early_spike() {
        // The regression this exists for, stated through the poller rather than
        // through `graph` alone: a 10 s window of 8 ms frames with one 40 ms hitch at
        // its very beginning. The strip used to draw the newest 120 intervals -- all
        // of them ordinary, 2000 frames after the hitch -- so the graph showed a flat
        // line while `low_1pct` in the row beside it counted the hitch.
        let scratch = Scratch::new("earlyspike");
        let mut fake = FakeWriter::new(scratch.path(), 7009, 4096);
        fake.record(2000, 8_000_000);
        // Overwrite the oldest slot the window will read with the hitch. The window
        // is 10 s = 1250 intervals, so the hitch sits at the front of it and 1249
        // ordinary frames follow.
        fake.set_slot((2000 - 1250) % 4096, 40_000_000);
        fake.set_last_present_ns(now_for(0));

        let mut poller = Poller::default();
        let snapshot = poller.poll(Some(scratch.path()), 10, Some(FakeWriter::present_ns_for(0)));
        let metrics = snapshot.metrics.expect("numbers");
        assert_eq!(metrics.samples, 1250, "the whole window, not the newest 120");
        assert!(
            metrics.low_1pct < 100.0,
            "the hitch is in the statistics: {metrics:?}"
        );
        let columns = &snapshot.samples_ms;
        assert_eq!(columns.len(), graph::GRAPH_COLUMNS);
        assert!(
            columns.iter().any(|ms| *ms == 40.0),
            "the earliest spike must still own a column: {columns:?}"
        );
    }

    #[test]
    fn a_window_longer_than_the_ring_history_is_reported_as_short() {
        // `window_seconds` permits 60 s; the ring holds 4096 intervals. At 6.06 ms
        // per frame that is ~24.8 s of history, so the requested window can never be
        // covered -- and the snapshot says so through `Metrics`, over the statistics
        // of what does exist, instead of presenting 24.8 s as 60 s.
        let scratch = Scratch::new("shortwindow");
        let mut fake = FakeWriter::new(scratch.path(), 7010, 4096);
        fake.record(5000, 6_000_000); // 30 s recorded, 4096 intervals (24.576 s) retained
        fake.set_last_present_ns(now_for(0));

        let mut poller = Poller::default();
        let snapshot = poller.poll(Some(scratch.path()), 60, Some(FakeWriter::present_ns_for(0)));
        let metrics = snapshot.metrics.expect("numbers");
        assert_eq!(metrics.samples, 4096, "everything the ring still holds");
        assert_eq!(metrics.requested_ms, 60_000.0);
        assert!(metrics.is_short(), "{metrics:?}");
        assert_eq!(metrics.window_ms, 4096.0 * 6.0, "the covered span is the samples'");
        assert!(
            (metrics.coverage() - 0.4096).abs() < 0.001,
            "{}",
            metrics.coverage()
        );

        // The same segment under a window the history covers reports full coverage,
        // so "short" is a property of the request, not a blanket warning.
        let fits = poller
            .poll(Some(scratch.path()), 10, Some(FakeWriter::present_ns_for(0)))
            .metrics
            .expect("numbers");
        assert!(!fits.is_short(), "{fits:?}");
        assert_eq!(fits.coverage(), 1.0);
        assert_eq!(fits.samples, 1667, "10 s of exactly-6 ms frames");
    }

    #[test]
    fn a_segment_refused_between_the_scan_and_the_read_is_absent_not_zero() {
        // The window between "this file is valid" and "read it" is where a game can
        // unlink its segment (or die). The poll must not fall back to the previous
        // poll's numbers, and it must not invent zeros.
        let scratch = Scratch::new("vanished");
        let _fake = seeded(scratch.path(), 7008, 120, 0);
        let mut poller = Poller::default();
        assert!(poller
            .poll(Some(scratch.path()), 10, Some(now_for(120)))
            .metrics
            .is_some());
        // The writer's own exit path: unlink at vkDestroyInstance.
        std::fs::remove_file(scratch.path().join("frames-7008")).expect("unlink");
        let after = poller.poll(Some(scratch.path()), 10, Some(now_for(120)));
        assert_eq!(after, FrameSnapshot::default());
    }

    // ---- logging ----

    #[test]
    fn a_live_source_is_not_re_announced_as_its_window_grows_or_fluctuates() {
        // The regression this exists for: the announcement key used to include the
        // window's sample count, so warm-up (a growing window) and ordinary frame-rate
        // variation (a wandering one) each produced a "change" -- four log lines a
        // second for the whole run of a game, saying nothing had changed.
        let scratch = Scratch::new("warmup");
        let mut fake = FakeWriter::new(scratch.path(), 7050, 4096);
        let mut poller = Poller::default();

        // A segment that has just appeared with one interval: the source is new, so
        // exactly one announcement.
        fake.record(1, 8_000_000);
        fake.set_last_present_ns(now_for(0));
        let _ = poller.poll(Some(scratch.path()), 10, Some(now_for(0)));
        assert_eq!(poller.announcements(), 1, "the source appeared");

        // Warm-up: the window grows from 1 to 600 intervals, and the frame time
        // wanders between 6 and 16 ms while it does.
        let mut samples_seen = Vec::new();
        for step in 1..=12u64 {
            let value = if step % 2 == 0 { 6_000_000 } else { 16_000_000 };
            fake.record(50, value);
            fake.set_last_present_ns(now_for(0));
            let snapshot = poller.poll(Some(scratch.path()), 10, Some(now_for(0)));
            let samples = snapshot.metrics.expect("numbers").samples;
            samples_seen.push(samples);
            assert_eq!(
                poller.announcements(),
                1,
                "the sample count changed to {samples} while the source stayed live"
            );
        }
        assert!(
            samples_seen.first() < samples_seen.last(),
            "premise: the window grew during the run: {samples_seen:?}"
        );
        assert!(
            samples_seen.windows(2).all(|pair| pair[0] <= pair[1]),
            "premise: monotone growth here, fluctuation in the frame times: {samples_seen:?}"
        );

        // A different game taking over IS a change worth one line.
        let mut other = FakeWriter::new(scratch.path(), 7051, 4096);
        other.record(60, 8_000_000);
        other.set_last_present_ns(now_for(0) + 1_000);
        let _ = poller.poll(Some(scratch.path()), 10, Some(now_for(0) + 1_001));
        assert_eq!(poller.announcements(), 2, "a different pid is a new source");
    }

    #[test]
    fn availability_changes_are_announced_once_and_unavailability_is_not_spammed() {
        let scratch = Scratch::new("logging");
        let mut poller = Poller::default();

        // Absent, three polls: one announcement.
        for _ in 0..3 {
            let _ = poller.poll(Some(&scratch.path().join("absent")), 10, Some(1));
        }
        assert_eq!(poller.announcements(), 1, "one line for one change");

        // A game appears: one announcement.
        seeded(scratch.path(), 7100, 60, 0);
        let _ = poller.poll(Some(scratch.path()), 10, Some(now_for(60)));
        assert_eq!(poller.announcements(), 2);

        // Live polls stay quiet.
        for _ in 0..5 {
            let _ = poller.poll(Some(scratch.path()), 10, Some(now_for(60) + 1_000_000));
        }
        assert_eq!(
            poller.announcements(),
            2,
            "a live source is not re-announced"
        );

        // The game exits: one announcement, then quiet again.
        let _ = poller.poll(Some(scratch.path()), 10, Some(now_for(60) + 5 * SECOND));
        assert_eq!(poller.announcements(), 3);
        for _ in 0..5 {
            let _ = poller.poll(Some(scratch.path()), 10, Some(now_for(60) + 6 * SECOND));
        }
        assert_eq!(
            poller.announcements(),
            3,
            "unavailability is not a log spammer"
        );
    }

    #[test]
    fn the_refusal_reason_is_named_so_the_log_says_what_is_wrong() {
        let scratch = Scratch::new("reason");
        let mut bad = FakeWriter::new(scratch.path(), 7200, 64);
        bad.record_pattern(4);
        bad.patch(8, &9u32.to_le_bytes());
        let scan = source::scan(scratch.path());
        let reason = describe(&scan);
        assert!(reason.contains("frames-7200"), "{reason}");
        assert!(reason.contains("protocol version 9"), "{reason}");

        // And an empty directory says so plainly.
        let empty = Scratch::new("reason2");
        assert_eq!(describe(&source::scan(empty.path())), "no segments");
    }

    // ---- the repaint rule ----

    fn config_with(ids: &[&str], refresh_hz: u32) -> PanelConfig {
        PanelConfig {
            refresh_hz,
            rows: rows::all_ids()
                .into_iter()
                .map(|id| crate::panel::config::RowConfig {
                    id: id.to_string(),
                    visible: ids.contains(&id),
                })
                .collect(),
            ..PanelConfig::fresh()
        }
    }

    #[test]
    fn the_repaint_cadence_is_the_poll_cadence_and_never_the_frame_rate() {
        let live = config_with(&["fps"], 4);
        assert_eq!(repaint_interval(&live), Duration::from_millis(250));
        // 30 Hz is the top of the configurable range: a panel that repainted at
        // 165 Hz would be a redraw source over the game it measures.
        let fast = config_with(&["frametime_graph"], 30);
        assert_eq!(repaint_interval(&fast), Duration::from_millis(33));
        assert_eq!(poll_interval_ms(0), 1000, "clamped to 1 Hz");
        assert_eq!(
            poll_interval_ms(super::super::config::MAX_REFRESH_HZ + 100),
            1000 / super::super::config::MAX_REFRESH_HZ as u64
        );
    }

    #[test]
    fn without_a_frame_element_the_panel_keeps_its_own_timer() {
        let hardware_only = config_with(&["cpu_processor", "ram_usage"], 4);
        assert_eq!(repaint_interval(&hardware_only), Duration::from_millis(500));
        assert!(!any_element_visible(&hardware_only));

        let hidden = config_with(&[], 4);
        assert_eq!(repaint_interval(&hidden), Duration::from_millis(500));
    }

    // ---- the worker ----

    #[test]
    fn an_offline_reader_has_no_thread_and_reports_nothing() {
        let frames = Frames::offline_in(None);
        assert!(!frames.is_running());
        assert_eq!(frames.snapshot(), FrameSnapshot::default());
        // Dropping an offline reader must not hang: there is nothing to join.
        drop(frames);
    }

    #[test]
    fn the_worker_publishes_a_snapshot_and_stops_when_dropped() {
        let scratch = Scratch::new("worker");
        let mut fake = FakeWriter::new(scratch.path(), 7300, 256);
        fake.record(120, 8_000_000);
        fake.set_last_present_ns(source::now_ns().expect("linux clock"));

        let mut frames = Frames::offline_in(Some(scratch.path().to_path_buf()));
        frames.start();
        frames.start(); // idempotent
        assert!(frames.is_running());
        frames.configure(10, 4);
        frames.set_active(true);

        // Bounded wait: the worker polls immediately and then every 250 ms, and the
        // assertion below fails loudly rather than hanging if it never publishes.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut snapshot = frames.snapshot();
        while !snapshot.metrics.is_some() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
            snapshot = frames.snapshot();
        }
        assert!(
            snapshot.metrics.is_some(),
            "the worker published nothing in 5 s"
        );
        assert_eq!(snapshot.source.expect("source").pid, 7300);
        assert_eq!(snapshot.metrics.expect("metrics").samples, 120);

        // Going inactive clears the snapshot rather than leaving the last numbers
        // on the UI thread's copy for the next activation.
        frames.set_active(false);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while frames.snapshot().metrics.is_some() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(frames.snapshot(), FrameSnapshot::default());

        // And the drop joins the thread.
        let started = std::time::Instant::now();
        drop(frames);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the worker must exit promptly"
        );
    }

    #[test]
    fn the_sleep_helper_returns_early_when_asked_to_stop() {
        let stop = AtomicBool::new(false);
        let started = std::time::Instant::now();
        sleep_slices(5_000, &stop);
        // Without a stop it sleeps the whole interval, in slices.
        assert!(started.elapsed() >= Duration::from_millis(4_900));

        stop.store(true, Ordering::Relaxed);
        let started = std::time::Instant::now();
        sleep_slices(5_000, &stop);
        assert!(started.elapsed() < Duration::from_millis(500));
    }
}
