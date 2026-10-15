//! A test-only fake of the writer side of protocol v1.
//!
//! It writes the header at ADR-5's offsets **by hand**, from the offset table,
//! rather than through [`super::shm::Header`]'s decoder. That is deliberate: it
//! makes the reader's layout the thing under test. A reader whose offsets drifted
//! from the protocol would decode this file as garbage and fail, where a fake that
//! reused the reader's own struct could drift with it and pass.

use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use super::shm::{API_VULKAN, FILE_PREFIX, FLAG_WRITER_ALIVE, HEADER_LEN, MAGIC, VERSION};

/// A segment file a test can write, corrupt and truncate.
pub struct FakeWriter {
    path: PathBuf,
    file: File,
    capacity: usize,
    frames: u64,
}

impl FakeWriter {
    /// Create `frames-<pid>` with a zeroed ring.
    pub fn new(dir: &Path, pid: u32, capacity: usize) -> FakeWriter {
        let path = dir.join(format!("{FILE_PREFIX}{pid}"));
        let file = File::create(&path).expect("create segment");
        let mut fake = FakeWriter {
            path,
            file,
            capacity,
            frames: 0,
        };
        fake.write_header(pid, 0);
        fake.zero_the_ring();
        fake
    }

    /// `frames` intervals, each `value_ns`, written into the ring the way the
    /// writer does it: slot `count % capacity`.
    pub fn record(&mut self, frames: u64, value_ns: u64) {
        for index in 0..frames {
            let slot = ((self.frames + index) % self.capacity as u64) as usize;
            self.write_slot(slot, value_ns);
        }
        self.frames += frames;
        self.write_header(self.pid().expect("pid"), 0);
    }

    /// The same, with interval `i` (1-based, across the whole run) equal to `i`
    /// milliseconds — so an accepted snapshot's contents say exactly which frames
    /// it holds.
    pub fn record_pattern(&mut self, frames: u64) {
        for index in 0..frames {
            let number = self.frames + index + 1;
            let slot = ((number - 1) % self.capacity as u64) as usize;
            self.write_slot(slot, number * 1_000_000);
        }
        self.frames += frames;
        self.write_header(self.pid().expect("pid"), 0);
    }

    /// Overwrite one ring slot **by slot index** (not by frame number), so a test
    /// can place a single interval — a spike — without disturbing the pattern the
    /// rest of the ring holds.
    pub fn set_slot(&mut self, slot: usize, value_ns: u64) {
        assert!(slot < self.capacity, "slot {slot} outside the {}-slot ring", self.capacity);
        self.write_slot(slot, value_ns);
        self.file.flush().expect("flush");
    }

    /// The header as of the last write, with a chosen sequence number.
    pub fn write_header(&mut self, pid: u32, seq: u64) {
        let mut buf = [0u8; HEADER_LEN];
        buf[0..8].copy_from_slice(&MAGIC.to_le_bytes());
        buf[8..12].copy_from_slice(&VERSION.to_le_bytes());
        buf[12..16].copy_from_slice(&(HEADER_LEN as u32).to_le_bytes());
        buf[16..20].copy_from_slice(&pid.to_le_bytes());
        buf[20..24].copy_from_slice(&API_VULKAN.to_le_bytes());
        let name = b"vkcube";
        buf[24..24 + name.len()].copy_from_slice(name);
        buf[88..92].copy_from_slice(&(self.capacity as u32).to_le_bytes());
        buf[92..96].copy_from_slice(&FLAG_WRITER_ALIVE.to_le_bytes());
        buf[96..104].copy_from_slice(&seq.to_le_bytes());
        buf[104..112].copy_from_slice(&self.last_present_ns().to_le_bytes());
        buf[112..120].copy_from_slice(&self.frames.to_le_bytes());
        buf[120..128].copy_from_slice(&0u64.to_le_bytes());
        self.file.seek(SeekFrom::Start(0)).expect("seek");
        self.file.write_all(&buf).expect("write header");
        self.file.flush().expect("flush");
    }

    /// The `CLOCK_MONOTONIC` stamp the writer would have taken: 10 ms per frame,
    /// from a fixed synthetic origin, so tests can name an age in nanoseconds.
    pub fn last_present_ns(&self) -> u64 {
        Self::present_ns_for(self.frames)
    }

    /// The stamp for an arbitrary frame count, for tests that need a segment to
    /// look stale without recording anything.
    pub fn present_ns_for(frames: u64) -> u64 {
        1_000_000_000 + frames * 10_000_000
    }

    /// Move the header's `last_present_ns` without touching anything else.
    pub fn set_last_present_ns(&mut self, ns: u64) {
        self.file.seek(SeekFrom::Start(104)).expect("seek");
        self.file.write_all(&ns.to_le_bytes()).expect("stamp");
        self.file.flush().expect("flush");
    }

    /// Overwrite `bytes` at `offset`, to break one field at a time.
    pub fn patch(&mut self, offset: usize, bytes: &[u8]) {
        self.file
            .seek(SeekFrom::Start(offset as u64))
            .expect("seek");
        self.file.write_all(bytes).expect("patch");
        self.file.flush().expect("flush");
    }

    /// Cut the file short.
    pub fn truncate(&mut self, len: u64) {
        self.file.set_len(len).expect("set_len");
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }

    pub fn pid(&self) -> Option<u32> {
        super::shm::pid_from_file_name(&self.path).ok()
    }

    fn write_slot(&mut self, slot: usize, value_ns: u64) {
        let offset = (HEADER_LEN + slot * 8) as u64;
        self.file.seek(SeekFrom::Start(offset)).expect("seek");
        self.file.write_all(&value_ns.to_le_bytes()).expect("slot");
    }

    fn zero_the_ring(&mut self) {
        let zeros = vec![0u8; self.capacity * 8];
        self.file
            .seek(SeekFrom::Start(HEADER_LEN as u64))
            .expect("seek");
        self.file.write_all(&zeros).expect("ring");
        self.file.flush().expect("flush");
    }
}

/// A self-removing scratch directory for segment files.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!(
            "lapsphere-frames-{}-{}-{}",
            std::process::id(),
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).expect("scratch dir");
        Scratch(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
