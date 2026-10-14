//! `frames_reader` — the panel-side reader of shm protocol v1.
//!
//! This is the spike's stand-in for what PR E will put in the GUI. It is
//! deliberately a *separate process* from the game, because that is the whole
//! point of the shm design and a same-process reader would prove nothing.
//!
//! Usage:
//!   frames_reader --list
//!   frames_reader --dump <csv>          one snapshot, newest `n` frames
//!   frames_reader --watch --hz 60 --seconds 10 --cpu
//!   frames_reader --overhead            overhead-sample statistics
//!   frames_reader --prune
//!
//! It uses the seqlock exactly as ADR-5 specifies: read seq, copy, re-read
//! seq, discard on change. No locks, no syscalls per frame.

use lapsphere_frames_layer::shm::*;
use std::fs;
use std::sync::atomic::Ordering as AtomicOrdering;

fn now_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
    }
    (ts.tv_sec as u64) * 1_000_000_000 + (ts.tv_nsec as u64)
}

fn segments() -> Vec<(u32, String)> {
    let dir = match std::env::var("XDG_RUNTIME_DIR") {
        Ok(d) => format!("{}/lapsphere", d),
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(pid) = name
                .strip_prefix("frames-")
                .and_then(|s| s.parse::<u32>().ok())
            {
                out.push((pid, e.path().to_string_lossy().to_string()));
            }
        }
    }
    out.sort();
    out
}

/// One consistent snapshot of a segment.
struct Snapshot {
    pid: u32,
    exe: String,
    frame_count: u64,
    dropped: u64,
    last_present_ns: u64,
    alive: bool,
    intervals_ns: Vec<u64>,
    overhead_ns: Vec<u64>,
    /// How many seqlock retries this snapshot needed. Non-zero means we
    /// caught the writer mid-update — the mechanism working, not a failure.
    retries: u32,
}

fn read_segment(path: &str, max_frames: usize) -> Option<Snapshot> {
    let f = fs::File::open(path).ok()?;
    let total = (OVERHEAD_OFFSET + OVERHEAD_SLOTS * 8) as usize;
    let m = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            total,
            libc::PROT_READ,
            libc::MAP_SHARED,
            std::os::unix::io::AsRawFd::as_raw_fd(&f),
            0,
        )
    };
    if m == libc::MAP_FAILED {
        return None;
    }
    let base = m as *const u8;

    let mut retries = 0u32;
    let snap = loop {
        // The seqlock word, read first. Every other header field below is an
        // atomic too (the writer stores them with atomic ops), so each is
        // loaded through `.load()` rather than copied.
        let hp = base as *const Header;
        let seq0 = unsafe { (*hp).write_seq.load(AtomicOrdering::Acquire) };
        if seq0 & 1 == 1 {
            retries += 1;
            if retries > 1000 {
                unsafe { libc::munmap(m, total) };
                return None;
            }
            continue;
        }
        // Copy everything the reader needs while the writer may be running.
        // Plain (non-atomic) fields may be copied wholesale; the atomic ones
        // are loaded individually below.
        let magic = unsafe { std::ptr::read_unaligned(std::ptr::addr_of!((*hp).magic)) };
        let version = unsafe { std::ptr::read_unaligned(std::ptr::addr_of!((*hp).version)) };
        let hdr_pid = unsafe { std::ptr::read_unaligned(std::ptr::addr_of!((*hp).pid)) };
        let exe_name = unsafe { std::ptr::read_unaligned(std::ptr::addr_of!((*hp).exe_name)) };
        let ring_capacity =
            unsafe { std::ptr::read_unaligned(std::ptr::addr_of!((*hp).ring_capacity)) };
        if magic != MAGIC || version != VERSION {
            unsafe { libc::munmap(m, total) };
            return None;
        }
        let cap = (ring_capacity as usize).min(RING_CAPACITY);
        let frame_count = unsafe { (*hp).frame_count.load(AtomicOrdering::Relaxed) };
        let n = (frame_count as usize).min(cap);
        let start = cap.saturating_sub(n);
        let mut iv = Vec::with_capacity(n);
        for i in 0..n {
            iv.push(unsafe {
                std::ptr::read_volatile(base.add(HEADER_LEN + (start + i) * 8) as *const u64)
            });
        }
        let ov_n = (frame_count as usize).min(OVERHEAD_SLOTS);
        let ov_start = OVERHEAD_SLOTS - ov_n;
        let mut ov = Vec::with_capacity(ov_n);
        for i in 0..ov_n {
            ov.push(unsafe {
                std::ptr::read_volatile(
                    base.add(OVERHEAD_OFFSET + (ov_start + i) * 8) as *const u64,
                )
            });
        }
        let seq1 = unsafe { (*hp).write_seq.load(AtomicOrdering::Acquire) };
        if seq0 != seq1 {
            retries += 1;
            continue;
        }
        let exe_end = exe_name.iter().position(|c| *c == 0).unwrap_or(64);
        break Snapshot {
            pid: hdr_pid,
            exe: String::from_utf8_lossy(&exe_name[..exe_end]).to_string(),
            frame_count,
            dropped: unsafe { (*hp).dropped_count.load(AtomicOrdering::Relaxed) },
            last_present_ns: unsafe { (*hp).last_present_ns.load(AtomicOrdering::Relaxed) },
            alive: unsafe { (*hp).flags.load(AtomicOrdering::Relaxed) & FLAG_WRITER_ALIVE != 0 },
            intervals_ns: iv,
            overhead_ns: ov,
            retries,
        };
    };

    let mut snap = snap;
    if snap.intervals_ns.len() > max_frames {
        let skip = snap.intervals_ns.len() - max_frames;
        snap.intervals_ns.drain(0..skip);
    }
    unsafe { libc::munmap(m, total) };
    Some(snap)
}

/// ADR-5's active-process rule.
fn pick_active(segs: &[Snapshot], now: u64, stale_ns: u64) -> Option<&Snapshot> {
    segs.iter()
        .filter(|s| now.saturating_sub(s.last_present_ns) < stale_ns)
        .max_by(|a, b| {
            a.last_present_ns
                .cmp(&b.last_present_ns)
                .then(a.pid.cmp(&b.pid))
        })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let has = |k: &str| args.iter().any(|a| a == k);
    let val = |k: &str| -> Option<String> {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };

    let all = segments();
    if has("--list") {
        let now = now_ns();
        println!("{} segment(s) in $XDG_RUNTIME_DIR/lapsphere", all.len());
        for (pid, path) in &all {
            match read_segment(path, 1) {
                Some(s) => println!(
                    "  pid {:<8} exe {:<20} frames {:<9} dropped {:<6} last_present {:>8.3} s ago  alive={}  size={}",
                    s.pid,
                    s.exe,
                    s.frame_count,
                    s.dropped,
                    (now.saturating_sub(s.last_present_ns)) as f64 / 1e9,
                    s.alive,
                    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
                ),
                None => println!("  pid {:<8} (unreadable: stale magic or truncated) {}", pid, path),
            }
        }
        return;
    }

    if has("--prune") {
        let now = now_ns();
        let orphan_ms = 300_000u64 * 1_000_000;
        let mut n = 0;
        for (pid, path) in &all {
            let alive = unsafe { libc::kill(*pid as i32, 0) == 0 };
            let old = read_segment(path, 1)
                .map(|s| now.saturating_sub(s.last_present_ns) > orphan_ms)
                .unwrap_or(true);
            if !alive || old {
                if fs::remove_file(path).is_ok() {
                    println!("pruned {} (pid_alive={}, too_old={})", path, alive, old);
                    n += 1;
                }
            }
        }
        println!("pruned {} segment(s)", n);
        return;
    }

    if has("--overhead") {
        let now = now_ns();
        for (_, path) in &all {
            let s = match read_segment(path, usize::MAX) {
                Some(s) => s,
                None => continue,
            };
            let mut v: Vec<u64> = s.overhead_ns.into_iter().filter(|x| *x > 0).collect();
            v.sort_unstable();
            if v.is_empty() {
                println!(
                    "pid {}: no overhead samples (run with LAPSPHERE_FRAMES_OVERHEAD=1)",
                    s.pid
                );
                continue;
            }
            let pc =
                |p: f64| v[(((p / 100.0) * v.len() as f64).ceil() as usize).clamp(1, v.len()) - 1];
            println!(
                "pid {} exe {}  samples {}  seqlock_retries {}  age {:.3}s",
                s.pid,
                s.exe,
                v.len(),
                s.retries,
                now.saturating_sub(s.last_present_ns) as f64 / 1e9
            );
            println!(
                "  min {:.3} us   median {:.3} us   p99 {:.3} us   p99.9 {:.3} us   max {:.3} us",
                v[0] as f64 / 1000.0,
                pc(50.0) as f64 / 1000.0,
                pc(99.0) as f64 / 1000.0,
                pc(99.9) as f64 / 1000.0,
                v[v.len() - 1] as f64 / 1000.0
            );
        }
        return;
    }

    if has("--dump") {
        let out = val("--dump").unwrap_or_else(|| "/tmp/frames.csv".into());
        let max = val("--n")
            .and_then(|v| v.parse().ok())
            .unwrap_or(usize::MAX);
        let now = now_ns();
        let segs: Vec<Snapshot> = all
            .iter()
            .filter_map(|(_, p)| read_segment(p, usize::MAX))
            .collect();
        let s = match pick_active(&segs, now, 2_000_000_000) {
            Some(s) => s,
            None => {
                eprintln!("no active segment (no present within 2 s)");
                std::process::exit(3);
            }
        };
        let mut txt = String::from("frametime_ms\n");
        let iv = if s.intervals_ns.len() > max {
            &s.intervals_ns[s.intervals_ns.len() - max..]
        } else {
            &s.intervals_ns
        };
        for v in iv {
            txt.push_str(&format!("{:.6}\n", *v as f64 / 1e6));
        }
        fs::write(&out, txt).unwrap();
        eprintln!(
            "wrote {} intervals (exe {}, pid {}) to {}",
            iv.len(),
            s.exe,
            s.pid,
            out
        );
        return;
    }

    // --watch: the reader-cost test (spike test 5).
    let hz: u64 = val("--hz").and_then(|v| v.parse().ok()).unwrap_or(60);
    let secs: f64 = val("--seconds")
        .and_then(|v| v.parse().ok())
        .unwrap_or(10.0);
    let measure_cpu = has("--cpu");
    let period_ns: u64 = 1_000_000_000 / hz.max(1);
    let deadline = now_ns() + (secs * 1e9) as u64;
    let mut samples = 0u64;
    let mut read_ns: Vec<u64> = Vec::new();
    let mut retries_total = 0u32;
    let mut seen_frames = 0u64;
    let mut first_last_ns = 0u64;

    let t_start = now_ns();
    let mut next = t_start;
    loop {
        let now = now_ns();
        if now >= deadline {
            break;
        }
        // Sleep to the target slot; a read costs far less than the period at
        // any rate the spike uses, so this is a real 60/240 Hz reader.
        if now < next {
            let d: u64 = next - now;
            let ts = libc::timespec {
                tv_sec: (d / 1_000_000_000) as libc::time_t,
                tv_nsec: (d % 1_000_000_000) as i64,
            };
            unsafe {
                libc::clock_nanosleep(libc::CLOCK_MONOTONIC, 0, &ts, std::ptr::null_mut());
            }
            continue;
        }
        next += period_ns;

        let segs: Vec<Snapshot> = all
            .iter()
            .filter_map(|(_, p)| read_segment(p, usize::MAX))
            .collect();
        let t0 = now_ns();
        let active = pick_active(&segs, t0, 2_000_000_000);
        let t1 = now_ns();
        read_ns.push(t1 - t0);
        retries_total += segs.iter().map(|s| s.retries).sum::<u32>();
        if let Some(a) = active {
            if first_last_ns == 0 {
                first_last_ns = a.last_present_ns;
            }
            seen_frames = seen_frames.max(a.frame_count);
        }
        samples += 1;
    }
    let wall = now_ns() - t_start;

    let mut v = read_ns.clone();
    v.sort_unstable();
    let pc = |p: f64| v[(((p / 100.0) * v.len() as f64).ceil() as usize).clamp(1, v.len()) - 1];

    let cpu: String = if measure_cpu {
        let stat = fs::read_to_string("/proc/self/stat").unwrap_or_default();
        let f: Vec<&str> = stat.split_whitespace().collect();
        // utime and stime are fields 14 and 15 (1-based), i.e. index 13,14;
        // after the comm field the split above already removed it.
        let ticks: u64 = f.get(13).and_then(|s| s.parse().ok()).unwrap_or(0)
            + f.get(14).and_then(|s| s.parse().ok()).unwrap_or(0);
        let hz = 100.0; // USER_HZ; read it rather than assume
        let hz = fs::read_to_string("/proc/self/stat")
            .ok()
            .and_then(|_| {
                let up = std::process::Command::new("getconf")
                    .arg("CLK_TCK")
                    .output()
                    .ok()?;
                String::from_utf8(up.stdout)
                    .ok()?
                    .trim()
                    .parse::<f64>()
                    .ok()
            })
            .unwrap_or(hz);
        format!(
            "  cpu {:.3} % of wall ({:.1} ticks at {:.0} Hz over {:.2} s)",
            ticks as f64 / hz * 100.0 / (wall as f64 / 1e9),
            ticks as f64,
            hz,
            wall as f64 / 1e9
        )
    } else {
        String::new()
    };

    println!(
        "hz={} samples={} wall={:.2}s actual_rate={:.1}Hz seqlock_retries={} max_frame_count={}",
        hz,
        samples,
        wall as f64 / 1e9,
        samples as f64 / (wall as f64 / 1e9),
        retries_total,
        seen_frames
    );
    println!(
        "  read cost: median {:.1} us  p99 {:.1} us  max {:.1} us",
        pc(50.0) as f64 / 1000.0,
        pc(99.0) as f64 / 1000.0,
        v[v.len() - 1] as f64 / 1000.0
    );
    print!("{}", cpu);
    if !cpu.is_empty() {
        println!();
    }
    let _ = AtomicOrdering::Relaxed;
}
