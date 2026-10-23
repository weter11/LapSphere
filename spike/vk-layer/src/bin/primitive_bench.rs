//! Out-of-band control for the stage-1b numbers.
//!
//! Every figure here is a **floor or a shape**, measured with no Vulkan, no
//! loader and no game in the process. Their job is to keep the instrument's own
//! cost from being read as the layer's:
//!
//! * `clock_pair` — two `clock_gettime(CLOCK_MONOTONIC)` calls around a volatile
//!   store. No clock-based window can resolve below this, because the in-band
//!   overhead sample contains exactly one such read inside its own window.
//! * `mutex_lock` — one lock/unlock of the same `Mutex<Option<Writer>>` the
//!   present hook takes.
//! * `record_full` — the real `shm::Writer::record()`, same code the layer runs
//!   per present: seqlock, volatile ring write, four atomic header stores.
//! * `record_overhead` — the same with the overhead sample enabled, i.e. the
//!   extra time a `LAPSPHERE_FRAMES_OVERHEAD=1` run pays for being measured.
//! * `hook_passthrough` / `hook_recording` — the *shape* of the present hook:
//!   lock, entry clock read, `record()`, unlock. `hook_passthrough` is the
//!   "layer loaded, doing nothing" cost, `hook_recording` adds the record body.
//!   Neither contains `next_present_for` or the call down into the layer below,
//!   which is why the throughput A/B (~/overhead.sh) exists as well.
//! * `mmap_read` — open + mmap of a real 65664-byte segment, copy both rings
//!   (4096 + 4096 `u64`, volatile), munmap. The floor for `frames_reader`'s
//!   per-snapshot read cost (check D1 in `verify.sh`).
//! * `readdir` — one `read_dir()` of the segment directory plus `metadata()`.
//!
//! Windows that contain a clock read contain it; the derived block at the end
//! subtracts `clock_pair` where that is the honest thing to do, and says so.
//!
//! Run: `./bench.sh` (or `primitive_bench --iters N --io-iters N`).

use lapsphere_frames_layer::shm::*;
use std::sync::Mutex;

fn now_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // The same call, in the same shape, as `shm::Writer`'s private `now_ns`.
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
    }
    (ts.tv_sec as u64) * 1_000_000_000 + (ts.tv_nsec as u64)
}

struct Row {
    name: &'static str,
    v: Vec<u64>,
}

/// Time `f` `n` times, each inside one clock pair (so every row below contains
/// `clock_pair`'s cost on top of the body it names).
fn bench(n: usize, mut f: impl FnMut()) -> Vec<u64> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let a = now_ns();
        f();
        let b = now_ns();
        out.push(b.saturating_sub(a));
    }
    out
}

fn pct(v: &[u64], p: f64) -> u64 {
    if v.is_empty() {
        return 0;
    }
    let i = (((p / 100.0) * v.len() as f64).ceil() as usize).clamp(1, v.len()) - 1;
    v[i]
}

fn row(name: &'static str, mut v: Vec<u64>) -> Row {
    v.sort_unstable();
    Row { name, v }
}

fn print_row(r: &Row) {
    println!(
        "{:<18} n={:<8} median={:>8} ns  p99={:>8} ns  max={:>9} ns  (median {:.3} us)",
        r.name,
        r.v.len(),
        pct(&r.v, 50.0),
        pct(&r.v, 99.0),
        r.v[r.v.len() - 1],
        pct(&r.v, 50.0) as f64 / 1000.0
    );
}

fn median(v: &[u64]) -> u64 {
    pct(v, 50.0)
}

fn mkdirp(path: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(path)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let val = |k: &str| -> Option<String> {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let iters: usize = val("--iters").and_then(|v| v.parse().ok()).unwrap_or(1_000_000);
    let io_iters: usize = val("--io-iters")
        .and_then(|v| v.parse().ok())
        .unwrap_or(20_000);
    let root = val("--dir").unwrap_or_else(|| {
        format!(
            "{}/lapsphere-primitive-bench-{}",
            std::env::temp_dir().display(),
            std::process::id()
        )
    });

    // Three separate runtime dirs: `Writer::create` is O_EXCL, so two writers
    // with the same pid cannot share one path.
    let dir_rec = format!("{root}/rec");
    let dir_ovh = format!("{root}/ovh");
    let dir_pass = format!("{root}/pass");
    for d in [&dir_rec, &dir_ovh, &dir_pass] {
        mkdirp(&format!("{d}/lapsphere")).expect("mkdir");
    }

    println!("primitive_bench — out-of-band floors and shapes");
    println!(
        "  iters={iters} io_iters={io_iters} dir={root} pid={}",
        std::process::id()
    );
    println!("  every window below contains one clock pair on top of the body it names");
    println!();

    let mut rows: Vec<Row> = Vec::new();

    // --- 1. the clock pair itself ------------------------------------------
    // Not wrapped in another pair: this row *is* the pair.
    let mut sink: u64 = 0;
    let mut pair_v: Vec<u64> = Vec::with_capacity(iters);
    for _ in 0..iters {
        let a = now_ns();
        let b = now_ns();
        pair_v.push(b.wrapping_sub(a));
        sink = sink.wrapping_add(1);
    }
    rows.push(row("clock_pair", pair_v));
    let clock_median = median(&rows[0].v);

    // --- 2. the writer mutex ------------------------------------------------
    let slot: Mutex<Option<Writer>> = Mutex::new(None);
    rows.push(row(
        "mutex_lock",
        bench(iters, || {
            if let Ok(g) = slot.lock() {
                sink = sink.wrapping_add(g.is_none() as u64);
            }
        }),
    ));

    // --- 3. a recording writer in a private runtime dir ---------------------
    std::env::set_var("XDG_RUNTIME_DIR", &dir_rec);
    std::env::remove_var("LAPSPHERE_FRAMES_PASS");
    std::env::remove_var("LAPSPHERE_FRAMES_OVERHEAD");
    let mut w = Writer::create().expect("Writer::create (recording)");
    let mut t: u64 = 1_000_000_000;
    rows.push(row(
        "record_full",
        bench(iters, || {
            t += 8_333_333;
            let n = now_ns();
            w.record(n, n);
        }),
    ));

    let hook = Mutex::new(Some(w));
    rows.push(row(
        "hook_recording",
        bench(iters, || {
            // The hook's shape: entry clock read, lock, record, unlock. No
            // `next_present_for`, no call down.
            let n = now_ns();
            if let Ok(mut g) = hook.lock() {
                if let Some(w) = g.as_mut() {
                    w.record(n, n);
                }
            }
        }),
    ));

    // --- 4. the reader's read: mmap + copy of a real segment ----------------
    let seg_path = format!("{}/lapsphere/frames-{}", dir_rec, std::process::id());
    let seg_len = TOTAL_LEN;
    let seg_file_bytes = std::fs::metadata(&seg_path).map(|m| m.len()).unwrap_or(0);
    let p = seg_path.clone();
    let copy_ring = |m: *mut libc::c_void| -> u64 {
        let mut acc: u64 = 0;
        let base = m as *const u8;
        unsafe {
            let mut i = 0usize;
            while i < RING_CAPACITY {
                acc = acc.wrapping_add(std::ptr::read_volatile(
                    base.add(HEADER_LEN + i * 8) as *const u64,
                ));
                i += 1;
            }
            let mut i = 0usize;
            while i < OVERHEAD_SLOTS {
                acc = acc.wrapping_add(std::ptr::read_volatile(
                    base.add(OVERHEAD_OFFSET + i * 8) as *const u64,
                ));
                i += 1;
            }
        }
        acc
    };
    rows.push(row(
        "mmap_read",
        bench(io_iters, || {
            if let Ok(f) = std::fs::File::open(&p) {
                use std::os::unix::io::AsRawFd;
                let fd = f.as_raw_fd();
                let m = unsafe {
                    libc::mmap(
                        std::ptr::null_mut(),
                        seg_len,
                        libc::PROT_READ,
                        libc::MAP_SHARED,
                        fd,
                        0,
                    )
                };
                if m != libc::MAP_FAILED {
                    sink = sink.wrapping_add(copy_ring(m));
                    unsafe { libc::munmap(m, seg_len) };
                }
            }
        }),
    ));

    // --- 5. the reader's directory scan -------------------------------------
    let scan_dir = format!("{}/lapsphere", dir_rec);
    rows.push(row(
        "readdir",
        bench(io_iters, || {
            if let Ok(rd) = std::fs::read_dir(&scan_dir) {
                let mut n = 0u64;
                for e in rd.flatten() {
                    n += std::fs::metadata(e.path()).map(|m| m.len()).unwrap_or(0);
                }
                sink = sink.wrapping_add(n & 1);
            }
        }),
    ));

    // --- 6. an instrumented writer (the extra sample store) -----------------
    std::env::set_var("XDG_RUNTIME_DIR", &dir_ovh);
    std::env::set_var("LAPSPHERE_FRAMES_OVERHEAD", "1");
    let mut w_ovh = Writer::create().expect("Writer::create (overhead)");
    let mut t2: u64 = 1_000_000_000;
    rows.push(row(
        "record_overhead",
        bench(iters, || {
            t2 += 8_333_333;
            let n = now_ns();
            w_ovh.record(n, n);
        }),
    ));
    w_ovh.destroy();

    // --- 7. a pass-through writer (the hook's fixed cost) -------------------
    std::env::set_var("XDG_RUNTIME_DIR", &dir_pass);
    std::env::remove_var("LAPSPHERE_FRAMES_OVERHEAD");
    std::env::set_var("LAPSPHERE_FRAMES_PASS", "1");
    let w_pass = Writer::create().expect("Writer::create (pass-through)");
    assert!(!w_pass.enabled(), "pass-through writer reported enabled");
    let pass = Mutex::new(Some(w_pass));
    rows.push(row(
        "hook_passthrough",
        bench(iters, || {
            let n = now_ns();
            if let Ok(mut g) = pass.lock() {
                if let Some(w) = g.as_mut() {
                    w.record(n, n);
                }
            }
        }),
    ));

    println!(
        "{:<18} {:>10} {:>13} {:>13} {:>16}",
        "bench", "n", "median", "p99", "max"
    );
    for r in &rows {
        print_row(r);
    }
    println!();
    println!("segment file the read floor was taken on:");
    println!(
        "  {} bytes={} expected={} (header {} + ring {} + overhead {})",
        seg_path,
        seg_file_bytes,
        seg_len,
        HEADER_LEN,
        RING_CAPACITY * 8,
        OVERHEAD_SLOTS * 8
    );
    println!();
    println!("derived (subtracting the clock pair, i.e. what the body costs alone):");
    for r in &rows {
        if r.name == "clock_pair" {
            continue;
        }
        let m = median(&r.v);
        println!(
            "{:<18} window {:>7} ns  minus clock_pair {:>5} ns = {:>7} ns ({:.3} us)",
            r.name,
            m,
            clock_median,
            m.saturating_sub(clock_median),
            m.saturating_sub(clock_median) as f64 / 1000.0
        );
    }
    println!();
    println!(
        "sink={} (prevents the loop bodies from being optimised away)",
        sink
    );
    let _ = std::fs::remove_dir_all(&root);
}
