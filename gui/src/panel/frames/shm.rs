//! Read-only side of shared-memory protocol v1 — `frames-<pid>`.
//!
//! The writer is the Vulkan layer (`spike/vk-layer/src/shm.rs`, ADR-5 in
//! `docs/development/panel-design.md`); this is the GUI's reader. It is
//! deliberately one-directional:
//!
//! * the file is opened **read-only** and never written to;
//! * the segment is **never** truncated, renamed, moved or unlinked, and the
//!   ADR-5 "the reader prunes orphans" allowance is **not** implemented — an
//!   orphan is already invisible to the UI within `STALE_AFTER` and costs one
//!   `read_dir` entry per poll, which is cheaper than a wrong deletion;
//! * every read goes through the writer's seqlock, and a snapshot whose sequence
//!   moved is **discarded**, never partially used.
//!
//! # What is validated before a byte of frame data is read
//!
//! In this order, each with the value it refused, because a segment that fails
//! any of these is not protocol v1 and its ring contents are not intervals:
//!
//! 1. the file exists and its size is at least [`HEADER_LEN`];
//! 2. `magic` is `"LSPFRS01"`;
//! 3. `version` is [`VERSION`];
//! 4. `header_len` is [`HEADER_LEN`] (a future header may grow, and then the ring
//!    is no longer where this reader thinks it is);
//! 5. `api` is [`API_VULKAN`] — `2` is reserved for a source this reader cannot
//!    label, so it renders `—` instead of a mislabelled number;
//! 6. `ring_capacity` is non-zero and at most [`MAX_RING_CAPACITY`];
//! 7. the file is long enough for the ring the header declares;
//! 8. the pid in `frames-<pid>` agrees with `pid` in the header.
//!
//! # Bounded reads
//!
//! A snapshot reads the header (128 bytes) twice and the newest intervals out of
//! the ring, as at most two runs, for the slots the caller asks for and no more.
//! Nothing walks the file, and there is no mmap: the reader is a `pread`-shaped
//! `seek`/`read_exact` pair on an owned `File`, so the module needs no `unsafe`
//! and no platform-specific file API.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// `"LSPFRS01"` as little-endian u64 — the writer's own constant.
pub const MAGIC: u64 = u64::from_le_bytes(*b"LSPFRS01");
/// The protocol version this reader understands. Anything else is refused.
pub const VERSION: u32 = 1;
/// Fixed header length in bytes (ADR-5).
pub const HEADER_LEN: usize = 128;
/// The largest ring this reader will read.
///
/// v1's writer creates [`MAX_RING_CAPACITY`] slots. A header claiming more is
/// refused rather than partially read: a reader that silently truncated a bigger
/// ring would report a statistic computed over a subset it did not admit to.
pub const MAX_RING_CAPACITY: usize = 4096;
/// `api = 1` is Vulkan — the only source this reader knows how to label.
pub const API_VULKAN: u32 = 1;
/// `flags` bit 0: the writer is alive.
///
/// **Advisory, and deliberately not what selection uses**: the flag survives a
/// `SIGKILL`, so trusting it would leave a dead game's last number on screen for as
/// long as the panel runs. [`super::source::STALE_AFTER`] on `last_present_ns` is
/// the rule; this constant exists so the byte is named and the writer's own
/// heartbeat is testable.
#[cfg_attr(not(test), allow(dead_code))]
pub const FLAG_WRITER_ALIVE: u32 = 1;
/// File-name prefix: `frames-<pid>`.
pub const FILE_PREFIX: &str = "frames-";
/// Seqlock attempts one snapshot gets before it is refused as [`Refusal::Unstable`].
///
/// The writer bumps the sequence twice per recorded interval at up to a few
/// thousand presents per second, and a retry costs two 128-byte reads of a
/// page-cached file, so a handful of attempts is enough to tell "the writer is
/// busy" from "the writer is hammering".
pub const READ_RETRIES: usize = 8;

/// Why a segment, or a read of one, was refused.
///
/// Each variant carries the value that failed, so a log line can name the number
/// instead of a category: the difference between "not protocol v1" and "protocol
/// v1 with a version of 2" is the difference between a stale file and a version
/// skew, and only one of them is our bug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// No such file (the normal case on a machine with no instrumented game).
    Missing,
    /// The file could not be read, or is not a regular file.
    Unreadable(String),
    /// The file is shorter than the header, or than the ring it declares.
    TooShort {
        len: u64,
        needed: u64,
    },
    BadMagic(u64),
    UnsupportedVersion(u32),
    BadHeaderLen(u32),
    UnsupportedApi(u32),
    BadRingCapacity(u32),
    /// `frames-<pid>` disagrees with the header's `pid`.
    PidMismatch {
        file: u32,
        header: u32,
    },
    /// The file name is not `frames-<pid>`.
    BadFileName(String),
    /// The sequence moved on every attempt: a write was in flight the whole time.
    Unstable,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Missing => write!(f, "no segment"),
            Refusal::Unreadable(err) => write!(f, "unreadable: {err}"),
            Refusal::TooShort { len, needed } => {
                write!(f, "short file: {len} bytes, {needed} needed")
            }
            Refusal::BadMagic(magic) => write!(f, "not protocol v1: magic {magic:#018x}"),
            Refusal::UnsupportedVersion(version) => write!(f, "protocol version {version}"),
            Refusal::BadHeaderLen(len) => write!(f, "header_len {len}, expected {HEADER_LEN}"),
            Refusal::UnsupportedApi(api) => write!(f, "api {api} is not Vulkan"),
            Refusal::BadRingCapacity(capacity) => write!(
                f,
                "ring_capacity {capacity} outside 1..={MAX_RING_CAPACITY}"
            ),
            Refusal::PidMismatch { file, header } => {
                write!(f, "file says pid {file}, header says pid {header}")
            }
            Refusal::BadFileName(name) => write!(f, "{name:?} is not {FILE_PREFIX}<pid>"),
            Refusal::Unstable => write!(f, "the writer was mid-write on every attempt"),
        }
    }
}

/// The 128-byte header, decoded. Field offsets are ADR-5's table, which is the
/// written form of the writer's `#[repr(C)]` layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub magic: u64,
    pub version: u32,
    pub header_len: u32,
    pub pid: u32,
    pub api: u32,
    /// `/proc/self/comm` of the writing process, NUL-padded in the protocol and
    /// trimmed here. This is what `game_name` shows.
    pub exe_name: String,
    pub ring_capacity: u32,
    pub flags: u32,
    pub write_seq: u64,
    /// `CLOCK_MONOTONIC` nanoseconds of the most recent present: the liveness
    /// signal the selection rule uses.
    pub last_present_ns: u64,
    /// Total intervals recorded in the current cadence — presents − 1, not
    /// presents (the writer's first present only establishes the origin).
    pub frame_count: u64,
    pub dropped_count: u64,
}

impl Header {
    /// Decode 128 bytes at ADR-5's offsets.
    pub fn decode(buf: &[u8]) -> Header {
        let mut exe_name = [0u8; 64];
        exe_name.copy_from_slice(&buf[24..88]);
        let cut = exe_name
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(exe_name.len());
        Header {
            magic: u64_at(buf, 0),
            version: u32_at(buf, 8),
            header_len: u32_at(buf, 12),
            pid: u32_at(buf, 16),
            api: u32_at(buf, 20),
            exe_name: String::from_utf8_lossy(&exe_name[..cut]).into_owned(),
            ring_capacity: u32_at(buf, 88),
            flags: u32_at(buf, 92),
            write_seq: u64_at(buf, 96),
            last_present_ns: u64_at(buf, 104),
            frame_count: u64_at(buf, 112),
            dropped_count: u64_at(buf, 120),
        }
    }

    /// Is a write in flight? The seqlock's odd state.
    pub fn write_in_flight(&self) -> bool {
        self.write_seq % 2 == 1
    }
}

fn u32_at(buf: &[u8], offset: usize) -> u32 {
    let mut word = [0u8; 4];
    word.copy_from_slice(&buf[offset..offset + 4]);
    u32::from_le_bytes(word)
}

fn u64_at(buf: &[u8], offset: usize) -> u64 {
    let mut word = [0u8; 8];
    word.copy_from_slice(&buf[offset..offset + 8]);
    u64::from_le_bytes(word)
}

/// One consistent snapshot: the header and the newest intervals it describes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    pub header: Header,
    /// Frame intervals in nanoseconds, **oldest first**, at most the requested
    /// number and never more than the ring holds.
    pub intervals: Vec<u64>,
}

/// An opened, validated segment.
///
/// `header` is the header read at open time; [`Segment::read`] returns a fresh one
/// which is what a snapshot must use, because the segment keeps moving between
/// the two.
#[derive(Debug)]
pub struct Segment {
    file: File,
    path: PathBuf,
    header: Header,
}

impl Segment {
    /// Validate and open `path`; see the module docs for the order of checks.
    pub fn open(path: &Path) -> Result<Segment, Refusal> {
        let file_pid = pid_from_file_name(path)?;

        let file = File::open(path).map_err(|err| match err.kind() {
            std::io::ErrorKind::NotFound => Refusal::Missing,
            _ => Refusal::Unreadable(err.to_string()),
        })?;

        let metadata = file
            .metadata()
            .map_err(|err| Refusal::Unreadable(err.to_string()))?;
        if !metadata.is_file() {
            return Err(Refusal::Unreadable("not a regular file".to_string()));
        }
        let len = metadata.len();
        if len < HEADER_LEN as u64 {
            return Err(Refusal::TooShort {
                len,
                needed: HEADER_LEN as u64,
            });
        }

        let mut file = file;
        let mut buf = [0u8; HEADER_LEN];
        read_exact_at(&mut file, 0, &mut buf)?;
        let header = Header::decode(&buf);

        if header.magic != MAGIC {
            return Err(Refusal::BadMagic(header.magic));
        }
        if header.version != VERSION {
            return Err(Refusal::UnsupportedVersion(header.version));
        }
        if header.header_len as usize != HEADER_LEN {
            return Err(Refusal::BadHeaderLen(header.header_len));
        }
        if header.api != API_VULKAN {
            return Err(Refusal::UnsupportedApi(header.api));
        }
        let capacity = header.ring_capacity as usize;
        if capacity == 0 || capacity > MAX_RING_CAPACITY {
            return Err(Refusal::BadRingCapacity(header.ring_capacity));
        }
        let needed = (HEADER_LEN + capacity * 8) as u64;
        if len < needed {
            return Err(Refusal::TooShort { len, needed });
        }
        if file_pid != header.pid {
            return Err(Refusal::PidMismatch {
                file: file_pid,
                header: header.pid,
            });
        }

        Ok(Segment {
            file,
            path: path.to_path_buf(),
            header,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The header as of open. A snapshot uses [`Reading::header`] instead.
    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn pid(&self) -> u32 {
        self.header.pid
    }

    /// A consistent snapshot: the header plus its newest `slots` intervals.
    ///
    /// The seqlock protocol, stated as this function implements it:
    ///
    /// 1. read the header; an odd `write_seq` means a write is in flight, so try
    ///    again;
    /// 2. copy the intervals;
    /// 3. read the header again; a different `write_seq` means the writer moved
    ///    while the intervals were being copied, so the pair cannot be trusted
    ///    and **the whole attempt is discarded** — a torn pair is not repaired,
    ///    not clipped and not partially returned.
    ///
    /// After `retries` attempts the read is refused as [`Refusal::Unstable`],
    /// which the caller reports as "no new data this poll", never as zeros.
    pub fn read(&mut self, slots: usize, retries: usize) -> Result<Reading, Refusal> {
        let slots = slots.max(1);
        for _ in 0..retries.max(1) {
            let first = self.read_header()?;
            if first.write_in_flight() {
                continue;
            }
            let intervals =
                self.read_newest(first.frame_count, first.ring_capacity as usize, slots)?;
            let second = self.read_header()?;
            if second.write_seq == first.write_seq {
                return Ok(Reading {
                    header: first,
                    intervals,
                });
            }
        }
        Err(Refusal::Unstable)
    }

    fn read_header(&mut self) -> Result<Header, Refusal> {
        let mut buf = [0u8; HEADER_LEN];
        read_exact_at(&mut self.file, 0, &mut buf)?;
        Ok(Header::decode(&buf))
    }

    /// The newest `slots` intervals, oldest first, read as at most two runs.
    fn read_newest(
        &mut self,
        frame_count: u64,
        capacity: usize,
        slots: usize,
    ) -> Result<Vec<u64>, Refusal> {
        if frame_count == 0 || capacity == 0 {
            return Ok(Vec::new());
        }
        let wanted = slots.min(capacity).min(frame_count as usize);
        // Slot 0 until the ring has wrapped: the newest interval lives at
        // `frame_count - 1` (the writer's rule), so the oldest of the `wanted`
        // newest lives at `frame_count - wanted`.
        let start = ((frame_count - wanted as u64) % capacity as u64) as usize;
        let mut out = vec![0u64; wanted];

        let head = (capacity - start).min(wanted);
        self.read_run(start, &mut out[..head])?;
        if wanted > head {
            self.read_run(0, &mut out[head..])?;
        }
        Ok(out)
    }

    fn read_run(&mut self, index: usize, out: &mut [u64]) -> Result<(), Refusal> {
        if out.is_empty() {
            return Ok(());
        }
        let offset = (HEADER_LEN + index * 8) as u64;
        let mut bytes = vec![0u8; out.len() * 8];
        read_exact_at(&mut self.file, offset, &mut bytes)?;
        for (slot, chunk) in out.iter_mut().zip(bytes.chunks_exact(8)) {
            let mut word = [0u8; 8];
            word.copy_from_slice(chunk);
            *slot = u64::from_le_bytes(word);
        }
        Ok(())
    }
}

fn read_exact_at(file: &mut File, offset: u64, buf: &mut [u8]) -> Result<(), Refusal> {
    file.seek(SeekFrom::Start(offset))
        .map_err(|err| Refusal::Unreadable(err.to_string()))?;
    file.read_exact(buf)
        .map_err(|err| Refusal::Unreadable(err.to_string()))
}

/// `frames-1234` → `1234`.
pub fn pid_from_file_name(path: &Path) -> Result<u32, Refusal> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| Refusal::BadFileName(format!("{path:?}")))?;
    name.strip_prefix(FILE_PREFIX)
        .and_then(|rest| rest.parse::<u32>().ok())
        .ok_or_else(|| Refusal::BadFileName(name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // The fake writer and the scratch directory live in `super::super::fake`,
    // so the reader and the selection rule are tested against the SAME
    // hand-written protocol layout rather than against two of them.
    use super::super::fake::{FakeWriter, Scratch};

    // ---- the header ----

    #[test]
    fn the_header_is_decoded_from_adr_5_offsets() {
        let scratch = Scratch::new("layout");
        let mut fake = FakeWriter::new(scratch.path(), 4242, 4096);
        fake.record(7, 16_666_667);

        let segment = Segment::open(&fake.path()).expect("opens");
        let header = segment.header();

        assert_eq!(header.magic, MAGIC);
        assert_eq!(header.version, 1);
        assert_eq!(header.header_len, HEADER_LEN as u32);
        assert_eq!(header.pid, 4242);
        assert_eq!(header.api, API_VULKAN);
        assert_eq!(header.exe_name, "vkcube");
        assert_eq!(header.ring_capacity, 4096);
        assert_eq!(header.flags & FLAG_WRITER_ALIVE, FLAG_WRITER_ALIVE);
        assert_eq!(header.frame_count, 7);
        assert_eq!(segment.pid(), 4242);
    }

    // ---- refusals, each with the value that caused it ----

    #[test]
    fn a_missing_segment_is_missing_and_not_an_error() {
        let scratch = Scratch::new("missing");
        let path = scratch.path().join("frames-1234");
        assert_eq!(Segment::open(&path).unwrap_err(), Refusal::Missing);
    }

    #[test]
    fn a_file_that_is_not_frames_pid_is_refused_by_name() {
        let scratch = Scratch::new("badname");
        let path = scratch.path().join("frames-abc");
        std::fs::write(&path, [0u8; 256]).expect("write");
        assert_eq!(
            Segment::open(&path).unwrap_err(),
            Refusal::BadFileName("frames-abc".to_string())
        );
    }

    #[test]
    fn a_file_shorter_than_the_header_or_the_ring_is_refused_with_its_length() {
        let scratch = Scratch::new("short");
        let mut fake = FakeWriter::new(scratch.path(), 11, 16);
        fake.truncate(64);
        assert_eq!(
            Segment::open(&fake.path()).unwrap_err(),
            Refusal::TooShort {
                len: 64,
                needed: 128
            }
        );

        let mut fake = FakeWriter::new(scratch.path(), 12, 16);
        // 128 header + 16 slots = 256; cut one slot's worth of bytes.
        fake.truncate(248);
        assert_eq!(
            Segment::open(&fake.path()).unwrap_err(),
            Refusal::TooShort {
                len: 248,
                needed: 256
            }
        );
    }

    #[test]
    fn a_wrong_magic_version_header_len_or_api_is_refused_with_the_value() {
        let scratch = Scratch::new("header");
        let mut fake = FakeWriter::new(scratch.path(), 21, 16);
        fake.patch(0, &0xdead_beefu64.to_le_bytes());
        assert_eq!(
            Segment::open(&fake.path()).unwrap_err(),
            Refusal::BadMagic(0xdead_beef)
        );

        let mut fake = FakeWriter::new(scratch.path(), 22, 16);
        fake.patch(8, &2u32.to_le_bytes());
        assert_eq!(
            Segment::open(&fake.path()).unwrap_err(),
            Refusal::UnsupportedVersion(2)
        );

        let mut fake = FakeWriter::new(scratch.path(), 23, 16);
        fake.patch(12, &64u32.to_le_bytes());
        assert_eq!(
            Segment::open(&fake.path()).unwrap_err(),
            Refusal::BadHeaderLen(64)
        );

        // `api = 2` is the reserved GL slot: a value this reader cannot label.
        let mut fake = FakeWriter::new(scratch.path(), 24, 16);
        fake.patch(20, &2u32.to_le_bytes());
        assert_eq!(
            Segment::open(&fake.path()).unwrap_err(),
            Refusal::UnsupportedApi(2)
        );
    }

    #[test]
    fn a_ring_capacity_of_zero_or_beyond_the_bound_is_refused() {
        let scratch = Scratch::new("capacity");
        let mut fake = FakeWriter::new(scratch.path(), 31, 16);
        fake.patch(88, &0u32.to_le_bytes());
        assert_eq!(
            Segment::open(&fake.path()).unwrap_err(),
            Refusal::BadRingCapacity(0)
        );

        // The bound is the reader's, not the protocol's: v1's writer creates 4096,
        // and a bigger ring is refused rather than partially read.
        let mut fake = FakeWriter::new(scratch.path(), 32, 16);
        fake.patch(88, &(MAX_RING_CAPACITY as u32 + 1).to_le_bytes());
        assert_eq!(
            Segment::open(&fake.path()).unwrap_err(),
            Refusal::BadRingCapacity(MAX_RING_CAPACITY as u32 + 1)
        );
    }

    #[test]
    fn a_pid_that_disagrees_between_the_name_and_the_header_is_refused() {
        // The name carries the pid the writer created the file under; a recycled
        // pid with a leftover segment is what this refuses.
        let scratch = Scratch::new("pid");
        let mut fake = FakeWriter::new(scratch.path(), 41, 16);
        fake.patch(16, &99u32.to_le_bytes());
        assert_eq!(
            Segment::open(&fake.path()).unwrap_err(),
            Refusal::PidMismatch {
                file: 41,
                header: 99
            }
        );
    }

    // ---- reads ----

    #[test]
    fn a_segment_with_no_intervals_reads_as_an_empty_ring_not_as_a_number() {
        let scratch = Scratch::new("empty");
        let fake = FakeWriter::new(scratch.path(), 51, 16);
        let mut segment = Segment::open(&fake.path()).expect("opens");

        let reading = segment.read(16, READ_RETRIES).expect("stable");
        assert_eq!(reading.header.frame_count, 0);
        assert!(reading.intervals.is_empty());
    }

    #[test]
    fn the_newest_intervals_are_read_oldest_first_before_the_ring_wraps() {
        let scratch = Scratch::new("linear");
        let mut fake = FakeWriter::new(scratch.path(), 61, 16);
        fake.record_pattern(5);
        let mut segment = Segment::open(&fake.path()).expect("opens");

        let reading = segment.read(16, READ_RETRIES).expect("stable");
        assert_eq!(reading.header.frame_count, 5);
        assert_eq!(
            reading.intervals,
            vec![1_000_000, 2_000_000, 3_000_000, 4_000_000, 5_000_000]
        );
    }

    #[test]
    fn after_the_ring_wraps_the_newest_slots_are_the_ones_returned() {
        // The defect this pins: a reader that indexes the ring as `capacity - n`
        // (the last n slots) returns unwritten slots once the ring has wrapped.
        let scratch = Scratch::new("wrap");
        let capacity = 8;
        let mut fake = FakeWriter::new(scratch.path(), 71, capacity);
        fake.record_pattern(20); // wraps twice, ends with frame_count = 20
        let mut segment = Segment::open(&fake.path()).expect("opens");

        let reading = segment.read(capacity, READ_RETRIES).expect("stable");
        assert_eq!(reading.header.frame_count, 20);
        // Intervals 13..20 ms, i.e. frames 13..=20: the newest `capacity` frames.
        assert_eq!(
            reading.intervals,
            (13..=20).map(|ms| ms * 1_000_000).collect::<Vec<u64>>()
        );

        // And fewer slots than the ring holds returns only the newest.
        let reading = segment.read(3, READ_RETRIES).expect("stable");
        assert_eq!(reading.intervals, vec![18_000_000, 19_000_000, 20_000_000]);
    }

    #[test]
    fn a_write_in_flight_is_never_accepted() {
        let scratch = Scratch::new("inflight");
        let mut fake = FakeWriter::new(scratch.path(), 81, 16);
        fake.record(4, 16_666_667);
        // Odd sequence: the writer is between its two release stores.
        fake.patch(96, &1u64.to_le_bytes());
        let mut segment = Segment::open(&fake.path()).expect("opens");

        assert_eq!(
            segment.read(16, READ_RETRIES).unwrap_err(),
            Refusal::Unstable
        );
    }

    #[test]
    fn a_snapshot_whose_sequence_moved_is_discarded_rather_than_partially_used() {
        // A real writer thread, hammering the segment the way the layer does. Every
        // accepted reading must be internally consistent: its intervals must be
        // exactly the newest `k` frames its own `frame_count` claims. A torn pair
        // (old frame_count with new intervals, or the reverse) cannot satisfy that,
        // which is what makes this a check and not a smoke test.
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let scratch = Scratch::new("seqlock");
        let capacity = 64;
        let path = Arc::new(scratch.path().join(format!("{FILE_PREFIX}91")));
        {
            let mut fake = FakeWriter::new(scratch.path(), 91, capacity);
            fake.record_pattern(1);
        }

        let stop = Arc::new(AtomicBool::new(false));
        let writer_path = Arc::clone(&path);
        let writer_stop = Arc::clone(&stop);
        let writer = std::thread::spawn(move || {
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&*writer_path)
                .expect("open for writing");
            let mut frame_count: u64 = 1;
            let mut seq: u64 = 0;
            while !writer_stop.load(Ordering::Relaxed) {
                // The writer's order: seq odd, payload, seq even.
                seq += 1;
                write_u64_at(&mut file, 96, seq);
                let slot = (frame_count % capacity as u64) as usize;
                write_u64_at(
                    &mut file,
                    (HEADER_LEN + slot * 8) as u64,
                    (frame_count + 1) * 1_000_000,
                );
                write_u64_at(&mut file, 104, 1_000_000_000 + frame_count * 10_000_000);
                write_u64_at(&mut file, 112, frame_count);
                seq += 1;
                write_u64_at(&mut file, 96, seq);
                frame_count += 1;
                std::thread::sleep(std::time::Duration::from_micros(200));
            }
        });

        let mut segment = Segment::open(&path).expect("opens");
        let mut accepted = 0usize;
        let mut refused = 0usize;
        for _ in 0..2000 {
            match segment.read(capacity, READ_RETRIES) {
                Ok(reading) => {
                    accepted += 1;
                    let count = reading.header.frame_count;
                    let k = reading.intervals.len() as u64;
                    assert!(k <= count, "more intervals ({k}) than frames ({count})");
                    // Oldest first, each interval identifying its frame: the
                    // interval recorded for frame `n` is `n` ms.
                    for (offset, interval) in reading.intervals.iter().enumerate() {
                        let frame = count - k + 1 + offset as u64;
                        assert_eq!(
                            *interval,
                            frame * 1_000_000,
                            "torn snapshot: frame_count {count} with intervals {:?}",
                            reading.intervals
                        );
                    }
                }
                Err(Refusal::Unstable) => refused += 1,
                Err(other) => panic!("unexpected refusal {other}"),
            }
            if accepted > 40 {
                break;
            }
        }
        stop.store(true, Ordering::Relaxed);
        writer.join().expect("writer");

        assert!(
            accepted > 0,
            "no snapshot was ever accepted (refused {refused})"
        );
        // The refusals are the point of the protocol, and they are allowed to be
        // 0 if the writer happened never to be mid-write; what must never happen
        // is a partially-read snapshot being accepted, which the loop above checks
        // on every accepted reading.
        assert!(segment.read(capacity, READ_RETRIES).is_ok());
    }

    fn write_u64_at(file: &mut File, offset: u64, value: u64) {
        file.seek(SeekFrom::Start(offset)).expect("seek");
        file.write_all(&value.to_le_bytes()).expect("write");
    }

    #[test]
    fn reading_more_slots_than_the_ring_holds_is_clamped_not_an_error() {
        let scratch = Scratch::new("clamp");
        let mut fake = FakeWriter::new(scratch.path(), 101, 4);
        fake.record_pattern(2);
        let mut segment = Segment::open(&fake.path()).expect("opens");

        let reading = segment.read(4096, READ_RETRIES).expect("stable");
        assert_eq!(reading.intervals, vec![1_000_000, 2_000_000]);
    }
}
