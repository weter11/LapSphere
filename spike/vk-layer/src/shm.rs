//! Shared-memory protocol v1 — writer side.
//!
//! Documented in `docs/development/panel-design.md` ADR-5. The layout below
//! is the normative implementation of that document; if they ever disagree,
//! the document is wrong and this file is right.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// `"LSPFRS01"` as little-endian u64.
pub const MAGIC: u64 = u64::from_le_bytes(*b"LSPFRS01");
pub const VERSION: u32 = 1;
pub const HEADER_LEN: usize = 128;

/// 4096 slots = 32 kB of ring: ~68 s of history at 60 fps.
pub const RING_CAPACITY: usize = 4096;

pub const FLAG_WRITER_ALIVE: u32 = 1 << 0;
pub const API_VULKAN: u32 = 1;

/// Header layout. All little-endian, which is the layout of every platform
/// this can run on; asserted at compile time below.
#[repr(C)]
pub struct Header {
    pub magic: u64,
    pub version: u32,
    pub header_len: u32,
    pub pid: u32,
    pub api: u32,
    pub exe_name: [u8; 64],
    pub ring_capacity: u32,
    pub flags: AtomicU32,
    pub write_seq: AtomicU64,
    pub last_present_ns: AtomicU64,
    pub frame_count: AtomicU64,
    pub dropped_count: AtomicU64,
}

const _: () = assert!(std::mem::size_of::<Header>() == HEADER_LEN);

impl Header {
    fn zeroed() -> Self {
        // Safety: Header is a plain-old-data aggregate of integers, arrays and
        // atomics; all bit patterns are valid.
        unsafe { std::mem::zeroed() }
    }
}

/// The writer. Everything is created once, up front; the present path only
/// does atomics and a clock read.
pub struct Writer {
    base: *mut c_void,
    len: usize,
    header: *mut Header,
    ring: *mut u64,
    /// Last present timestamp, writer-private (never shared). Kept here so
    /// the hot path touches no shared state except the ring slot.
    last_ns: u64,
    /// Frames seen since the swapchain was (re)created.
    frames: u64,
    dropped: u64,
    /// If true, the hook does nothing at all — this is the A/B "layer loaded
    /// but pass-through" build used by the spike.
    pub pass_through: bool,
    /// Overhead samples, in nanoseconds, written to the side file. Opt-in so
    /// the overhead measurement can include its own clock cost.
    pub overhead: bool,
}

unsafe impl Send for Writer {}
unsafe impl Sync for Writer {}

fn now_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // CLOCK_MONOTONIC: vDSO, no syscall, unaffected by wall-clock steps.
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
    }
    (ts.tv_sec as u64) * 1_000_000_000 + (ts.tv_nsec as u64)
}

impl Writer {
    /// Create `$XDG_RUNTIME_DIR/lapsphere/frames-<pid>` and map it.
    ///
    /// Returns `Err` rather than panicking on every failure: a layer that
    /// cannot record frames must still let the game run.
    pub fn create() -> Result<Writer, String> {
        let runtime_dir =
            std::env::var("XDG_RUNTIME_DIR").map_err(|_| "XDG_RUNTIME_DIR unset".to_string())?;
        let dir = format!("{}/lapsphere", runtime_dir);
        // mkdir -p semantics, ignoring EEXIST.
        let rc = unsafe {
            libc::mkdir(
                std::ffi::CString::new(dir.as_bytes()).unwrap().as_ptr(),
                0o700,
            )
        };
        if rc != 0 {
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() != Some(libc::EEXIST) {
                return Err(format!("mkdir {}: {}", dir, e));
            }
        }

        let path = format!("{}/frames-{}", dir, std::process::id());
        let cpath = std::ffi::CString::new(path.as_bytes()).unwrap();

        // O_EXCL: never adopt or truncate an existing file. A recycled pid
        // with a leftover segment therefore fails cleanly instead of
        // corrupting a dead writer's data.
        let fd = unsafe {
            libc::open(
                cpath.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(format!(
                "open {}: {}",
                path,
                std::io::Error::last_os_error()
            ));
        }

        // The overhead region is always mapped, whether or not it is written,
        // so the offset arithmetic below never depends on the env var.
        let len = OVERHEAD_OFFSET + OVERHEAD_SLOTS * 8;
        let rc = unsafe { libc::ftruncate(fd, len as libc::off_t) };
        if rc != 0 {
            let e = std::io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(format!("ftruncate: {}", e));
        }

        let base: *mut c_void = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        // The mapping keeps the file alive; the fd is not needed afterwards.
        unsafe { libc::close(fd) };
        if base == libc::MAP_FAILED {
            return Err(format!("mmap: {}", std::io::Error::last_os_error()));
        }

        let header = base as *mut Header;
        let ring = unsafe { (base as *mut u8).add(HEADER_LEN) } as *mut u64;

        let mut h = Header::zeroed();
        h.magic = MAGIC;
        h.version = VERSION;
        h.header_len = HEADER_LEN as u32;
        h.pid = std::process::id();
        h.api = API_VULKAN;
        h.ring_capacity = RING_CAPACITY as u32;
        // exe_name from /proc/self/comm, NUL-padded, truncated.
        if let Ok(comm) = std::fs::read("/proc/self/comm") {
            let name = comm.split(|c| *c == b'\n').next().unwrap_or(&comm);
            let n = name.len().min(63);
            h.exe_name[..n].copy_from_slice(&name[..n]);
        }
        unsafe { std::ptr::write_unaligned(header, h) };

        let pass_through = std::env::var("LAPSPHERE_FRAMES_PASS").is_ok();
        let overhead = std::env::var("LAPSPHERE_FRAMES_OVERHEAD").is_ok();

        Ok(Writer {
            base,
            len,
            header,
            ring,
            last_ns: 0,
            frames: 0,
            dropped: 0,
            pass_through,
            overhead,
        })
    }

    #[inline]
    pub fn enabled(&self) -> bool {
        !self.pass_through
    }

    /// Record one present. `t1` is the `CLOCK_MONOTONIC` timestamp taken
    /// immediately after the call into the next layer returned — the frame
    /// boundary the layer observes. `work_start` is the same moment from the
    /// layer's point of view: the overhead sample is measured from there to
    /// the end of this function, so it contains **only this layer's own work**
    /// (the mutex, the clock-free stores, the ring write) and none of the
    /// downstream present, which would otherwise dominate it by orders of
    /// magnitude.
    #[inline]
    pub fn record(&mut self, t1: u64, work_start: u64) {
        if self.pass_through {
            return;
        }
        if self.last_ns == 0 {
            // First present establishes the origin; there is no interval for
            // the gap between process start and the first frame.
            self.last_ns = t1;
            self.flush_header();
            return;
        }
        let interval = t1.saturating_sub(self.last_ns).max(1);
        self.last_ns = t1;
        if interval > 10_000_000_000 {
            // >10 s gap: not a frame, a suspend or a breakpoint. Dropping it
            // keeps it out of p99.9 and out of the graph.
            self.dropped += 1;
            self.flush_header();
            return;
        }

        let seq = unsafe { (*self.header).write_seq.load(Ordering::Relaxed) };
        unsafe {
            // odd => write in progress
            (*self.header)
                .write_seq
                .store(seq.wrapping_add(1), Ordering::Release);
            // payload
            let idx = (self.frames % RING_CAPACITY as u64) as usize;
            std::ptr::write_volatile(self.ring.add(idx), interval);
            self.frames += 1;
            (*self.header).last_present_ns.store(t1, Ordering::Relaxed);
            (*self.header)
                .frame_count
                .store(self.frames, Ordering::Relaxed);
            (*self.header)
                .dropped_count
                .store(self.dropped, Ordering::Relaxed);
            (*self.header)
                .flags
                .store(FLAG_WRITER_ALIVE, Ordering::Relaxed);
            // even => write done
            (*self.header)
                .write_seq
                .store(seq.wrapping_add(2), Ordering::Release);
        }

        if self.overhead {
            let d = now_ns().saturating_sub(work_start);
            // One buffer of samples at a fixed offset; the reader bounds the
            // valid prefix by the header's frame_count, so wrap-around is
            // harmless and no per-frame index has to be stored.
            let idx = ((self.frames - 1) % OVERHEAD_SLOTS as u64) as usize;
            unsafe {
                    std::ptr::write_volatile(
                        (self.base as *mut u8).add(OVERHEAD_OFFSET + idx * 8) as *mut u64,
                        d,
                    )
                };
        }
    }

    /// Forget the present origin, so the next present writes no interval.
    /// Called on swapchain creation: a new swapchain means the old cadence
    /// ended, and the gap across the recreation is not a frame time.
    #[inline]
    pub fn reset_origin(&mut self) {
        self.last_ns = 0;
    }

    /// Header-only refresh, used on the paths that record no interval (first
    /// present, dropped frame). Keeps `last_present_ns` fresh, which is the
    /// liveness signal ADR-5's active-process rule depends on.
    #[inline]
    fn flush_header(&mut self) {
        unsafe {
            (*self.header)
                .last_present_ns
                .store(self.last_ns, Ordering::Relaxed);
            (*self.header)
                .frame_count
                .store(self.frames, Ordering::Relaxed);
            (*self.header)
                .dropped_count
                .store(self.dropped, Ordering::Relaxed);
            (*self.header)
                .flags
                .store(FLAG_WRITER_ALIVE, Ordering::Relaxed);
        }
    }

    /// Remove the segment. Called from `vkDestroyInstance`. Errors are
    /// ignored: an orphan is already harmless under ADR-5's timeout rule.
    pub fn destroy(&mut self) {
        if !self.base.is_null() && self.base != libc::MAP_FAILED {
            unsafe {
                (*self.header).flags.store(0, Ordering::Relaxed);
                libc::munmap(self.base, self.len);
            }
            self.base = std::ptr::null_mut();
            let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
            let path = format!("{}/lapsphere/frames-{}", runtime_dir, std::process::id());
            unsafe { libc::unlink(std::ffi::CString::new(path.as_bytes()).unwrap().as_ptr()) };
        }
    }
}

/// Overhead samples live right after the ring; the reader reads them by name.
pub const OVERHEAD_SLOTS: usize = 4096;
pub const OVERHEAD_OFFSET: usize = HEADER_LEN + RING_CAPACITY * 8;
pub const TOTAL_LEN: usize = OVERHEAD_OFFSET + OVERHEAD_SLOTS * 8;
