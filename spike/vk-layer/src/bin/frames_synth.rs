//! `frames_synth` — a synthetic writer at an exact, reproducible rate.
//!
//! It drives the **same** `shm::Writer` the layer drives, so the segment it
//! leaves is protocol v1 and `frames_reader` cannot tell it apart from a game's
//! — which is the point: the reader's cost and CPU must be measurable at a
//! chosen rate (60 Hz, 240 Hz) without depending on what a compositor or a GPU
//! happens to produce. It also produces the "no reader", "two writers" and
//! "stale segment" cases on demand.
//!
//! What it does **not** do is say anything about the present path: it never
//! touches Vulkan, so only a real `vkcube` run is evidence about the layer.
//!
//! Usage:
//!   frames_synth --hz 240 --seconds 10 [--jitter-us 0] [--label x]

use lapsphere_frames_layer::shm::*;

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

fn sleep_until(target: u64) {
    // TIMER_ABSTIME takes the deadline itself, not a delta: passing a delta
    // here returns immediately and the "generator" then free-runs at whatever
    // the CPU allows (measured: 720 ticks in 3 ms instead of 3 s).
    let ts = libc::timespec {
        tv_sec: (target / 1_000_000_000) as libc::time_t,
        tv_nsec: (target % 1_000_000_000) as i64,
    };
    loop {
        let rc = unsafe {
            libc::clock_nanosleep(
                libc::CLOCK_MONOTONIC,
                libc::TIMER_ABSTIME,
                &ts,
                std::ptr::null_mut(),
            )
        };
        if rc != libc::EINTR {
            return;
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let val = |k: &str| -> Option<String> {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let hz: f64 = val("--hz").and_then(|v| v.parse().ok()).unwrap_or(60.0);
    let secs: f64 = val("--seconds")
        .and_then(|v| v.parse().ok())
        .unwrap_or(10.0);
    let jitter_us: u64 = val("--jitter-us").and_then(|v| v.parse().ok()).unwrap_or(0);
    let label = val("--label").unwrap_or_else(|| "synth".to_string());
    if hz <= 0.0 || secs <= 0.0 {
        eprintln!("usage: frames_synth --hz <hz> --seconds <s> [--jitter-us <us>] [--label <x>]");
        std::process::exit(2);
    }

    let period = (1_000_000_000.0 / hz) as u64;
    let mut w = match Writer::create() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("frames_synth: Writer::create failed: {e}");
            std::process::exit(1);
        }
    };
    if !w.enabled() {
        eprintln!("frames_synth: writer is in pass-through mode (LAPSPHERE_FRAMES_PASS set?)");
    }

    let t_start = now_ns();
    let deadline = t_start + (secs * 1e9) as u64;
    let mut next = t_start + period;
    let mut ticks: u64 = 0;
    let mut jitter_state: u64 = 0x9e3779b97f4a7c15;
    while next < deadline {
        sleep_until(next);
        let mut n = now_ns();
        if jitter_us > 0 {
            // Deterministic xorshift jitter: reproducible from run to run,
            // unlike reading the clock twice and hoping.
            jitter_state ^= jitter_state << 13;
            jitter_state ^= jitter_state >> 7;
            jitter_state ^= jitter_state << 17;
            n = n.saturating_sub(jitter_state % (jitter_us * 1000).max(1));
        }
        w.record(n, n);
        ticks += 1;
        next += period;
    }
    let wall = now_ns() - t_start;
    println!(
        "synth_final[{}]: hz={} ticks={} intervals={} dropped={} wall={:.3}s rate={:.2}Hz",
        label,
        hz,
        ticks,
        w.frames(),
        w.dropped(),
        wall as f64 / 1e9,
        ticks as f64 / (wall as f64 / 1e9)
    );
    w.destroy();
}
