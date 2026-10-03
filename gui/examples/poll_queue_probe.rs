// Throwaway measurement harness for the bounded-poll-queue fix.
//
// Lives in `examples/` on purpose: it must not become production code and must
// not add telemetry to `gui/src/**`. It drives the REAL `RefreshCoordinator`
// and the REAL bounded update channel against a consumer that never drains —
// which is exactly what an iconified window does, since `ui()`/`logic()` stop
// running and nothing calls `handle_hardware_updates()`.
//
// It reports the three numbers the fix is about, per arm:
//   * tasks alive (spawned fetch tasks that have not finished)
//   * sends waiting (fetcher tasks parked on a full channel)
//   * RSS of this process
//
// Usage: cargo run --release --example poll_queue_probe -- <label> <seconds>
//        cargo run --release --example poll_queue_probe -- <label> <seconds> fixed

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

// The GUI crate is a binary, so an example cannot `use lapsphere::...`.
// Including the module by path keeps the probe driving the REAL coordinator
// source instead of a copy that could drift from it.
#[path = "../src/polling_scheduler.rs"]
mod polling_scheduler;

use polling_scheduler::{InFlightSet, RefreshCoordinator};

/// Stand-in for a polled reply payload, sized like the real ones (a daemon-log
/// ring reply is ~470 kB; a GpuInfo reply is a few kB).
#[derive(Clone)]
struct Sample(Vec<u8>);

#[derive(Clone, Copy, PartialEq, Debug)]
enum Arm {
    /// Pre-fix: no in-flight guard, fetcher blocks on `send().await`.
    Prefix,
    /// Post-fix: one request per component, polled sends use `try_send`.
    Fixed,
}

fn rss_kb() -> u64 {
    let statm = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
    let pages: u64 = statm
        .split_whitespace()
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    pages * 4
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let label = args.get(1).cloned().unwrap_or_else(|| "run".into());
    let seconds: u64 = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let arm = match args.get(3).map(|s| s.as_str()) {
        Some("fixed") => Arm::Fixed,
        _ => Arm::Prefix,
    };

    // The same component set and intervals the GUI registers by default.
    let components: Vec<(&str, u64)> = vec![
        ("cpu", 1000),
        ("gpu", 1500),
        ("memory", 2000),
        ("fans", 2500),
        ("battery", 3000),
        ("wifi", 3000),
        ("gamepads", 3000),
        ("storage", 3000),
        ("mount", 3000),
        ("webcam", 5000),
        ("logs", 5000),
    ];

    let (tx, mut rx) = tokio::sync::mpsc::channel::<Sample>(100);

    let alive = Arc::new(AtomicU64::new(0));
    let waiting = Arc::new(AtomicU64::new(0));
    let delivered = Arc::new(AtomicU64::new(0));
    let dropped = Arc::new(AtomicU64::new(0));
    let skipped = Arc::new(AtomicU64::new(0));

    let rt = tokio::runtime::Runtime::new().expect("runtime");

    println!(
        "# arm={arm:?} label={label} window={seconds}s channel=100 components={}",
        components.len()
    );
    println!("# t(s)\talive\twaiting\tdelivered\tdropped\tskipped\trss_kb");

    let start = Instant::now();
    let mut last_report = Instant::now();

    rt.block_on(async move {
        let coordinator = RefreshCoordinator::new();
        let handle = coordinator.get_handle();
        for (id, interval) in &components {
            let _ = handle.register(id.to_string(), Duration::from_millis(*interval));
        }

        let skipped_cb = Arc::clone(&skipped);
        // Clone once for the callback; the originals stay available for reporting.
        let cb_alive = Arc::clone(&alive);
        let cb_waiting = Arc::clone(&waiting);
        let cb_delivered = Arc::clone(&delivered);
        let cb_dropped = Arc::clone(&dropped);
        let cb_in_flight = InFlightSet::new();
        let coordinator_task = tokio::spawn(coordinator.run(move |component_id| {
            let component = component_id.to_string();

            let permit = match arm {
                // Pre-fix: unconditional spawn, nothing bounds concurrency.
                Arm::Prefix => None,
                Arm::Fixed => cb_in_flight.try_begin(&component),
            };
            if arm == Arm::Fixed && permit.is_none() {
                skipped_cb.fetch_add(1, Ordering::Relaxed);
                return;
            }

            let tx = tx.clone();
            let alive = Arc::clone(&cb_alive);
            let waiting = Arc::clone(&cb_waiting);
            let delivered = Arc::clone(&cb_delivered);
            let dropped = Arc::clone(&cb_dropped);
            tokio::spawn(async move {
                let _permit = permit;
                alive.fetch_add(1, Ordering::Relaxed);
                // Stand in for the D-Bus round trip (~1-10 ms).
                tokio::time::sleep(Duration::from_millis(3)).await;

                let sample = Sample(vec![0u8; 4096]);
                match arm {
                    // Pre-fix: block until the consumer takes it. With no
                    // consumer, every one of these parks forever.
                    Arm::Prefix => {
                        waiting.fetch_add(1, Ordering::Relaxed);
                        let _ = tx.send(sample).await;
                        waiting.fetch_sub(1, Ordering::Relaxed);
                    }
                    // Post-fix: a full channel means the sample is already
                    // stale — drop it and count it.
                    Arm::Fixed => {
                        if tx.try_send(sample).is_err() {
                            dropped.fetch_add(1, Ordering::Relaxed);
                        } else {
                            delivered.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                alive.fetch_sub(1, Ordering::Relaxed);
            });
        }));

        loop {
            if start.elapsed() >= Duration::from_secs(seconds) {
                break;
            }
            if last_report.elapsed() >= Duration::from_secs(10) {
                last_report = Instant::now();
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    start.elapsed().as_secs(),
                    alive.load(Ordering::Relaxed),
                    waiting.load(Ordering::Relaxed),
                    delivered.load(Ordering::Relaxed),
                    dropped.load(Ordering::Relaxed),
                    skipped.load(Ordering::Relaxed),
                    rss_kb()
                );
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        println!(
            "FINAL\talive={}\twaiting={}\tdelivered={}\tdropped={}\tskipped={}\trss_kb={}",
            alive.load(Ordering::Relaxed),
            waiting.load(Ordering::Relaxed),
            delivered.load(Ordering::Relaxed),
            dropped.load(Ordering::Relaxed),
            skipped.load(Ordering::Relaxed),
            rss_kb()
        );
        coordinator_task.abort();
        // Drain whatever arrived, then report what was still queued.
        let mut queued = 0usize;
        while rx.try_recv().is_ok() {
            queued += 1;
        }
        println!("QUEUED_AT_END={queued}");
    });
}