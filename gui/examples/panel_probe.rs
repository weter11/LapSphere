// Throwaway spike probe for the gkrellm-style "panel" mode investigation.
//
// NOT production code. Lives in gui/examples/ on purpose: it is a standalone
// eframe binary that mimics the *delivery path* of gui/src/app.rs
// (tokio::spawn -> mpsc(100) -> try_recv() in ui() -> request_repaint_after)
// and instruments it, plus exercises every ViewportCommand we care about.
//
// It deliberately does not touch gui/src/**.
//
// Usage: panel_probe <mode> [seconds]
//   latency    1a: HardwareUpdate arrival -> ui() consumption latency
//   hidden     1b: Visible(false) for N s, queue/blocked-senders/RSS growth
//   cmds       2 : every ViewportCommand, state dumped per step
//   toggle     4 : N full<->panel mode switches, RSS + tray Show/Hide
//   panelboot  2c/3: start directly in panel geometry (flash check)

use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use eframe::egui::vec2;
use tokio::sync::mpsc;

const PANEL_SIZE: [f32; 2] = [260.0, 72.0];
const FULL_SIZE: [f32; 2] = [570.0, 620.0];

// ---------------------------------------------------------------- telemetry

#[derive(Default)]
struct Stats {
    produced: u64,
    consumed: u64,
    // sum/min/max of (ui_consume_time - produce_time) in ms
    lat_sum_ms: f64,
    lat_min_ms: f64,
    lat_max_ms: f64,
    // sends whose await actually blocked (>1 ms)
    blocked_sends: u64,
    send_await_sum_ms: f64,
    max_queue: usize,
}

struct Telemetry {
    // timestamp taken immediately before tx.send()
    t_produced: AtomicU64,
    produced: AtomicU64,
    consumed: AtomicU64,
    lat_sum_us: AtomicU64,
    lat_min_us: AtomicU64,
    lat_max_us: AtomicU64,
    blocked_sends: AtomicU64,
    send_await_sum_us: AtomicU64,
    inflight: AtomicUsize, // tasks currently inside/awaiting send()
    max_inflight: AtomicUsize,
    queued: AtomicUsize,    // best-effort mirror of mpsc occupancy
    max_queue: AtomicUsize,
    ui_calls: AtomicU64,
    paints: AtomicU64,
    frame_end_us: AtomicU64,
    log: Mutex<std::fs::File>,
    stop: AtomicBool,
}

impl Telemetry {
    fn new(path: &str) -> Self {
        let f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open telemetry log");
        Self {
            t_produced: AtomicU64::new(0),
            produced: AtomicU64::new(0),
            consumed: AtomicU64::new(0),
            lat_sum_us: AtomicU64::new(0),
            lat_min_us: AtomicU64::new(u64::MAX),
            lat_max_us: AtomicU64::new(0),
            blocked_sends: AtomicU64::new(0),
            send_await_sum_us: AtomicU64::new(0),
            inflight: AtomicUsize::new(0),
            max_inflight: AtomicUsize::new(0),
            queued: AtomicUsize::new(0),
            max_queue: AtomicUsize::new(0),
            ui_calls: AtomicU64::new(0),
            paints: AtomicU64::new(0),
            frame_end_us: AtomicU64::new(0),
            log: Mutex::new(f),
            stop: AtomicBool::new(false),
        }
    }

    fn note(&self, tag: &str) {
        let mut f = self.log.lock().unwrap();
        let _ = writeln!(
            f,
            "[{}] {:<10} ui={} paint={} prod={} cons={} queued~={} maxq={} inflight={} maxinflight={} blocked={} lat[min/avg/max]={:.1}/{:.1}/{:.1}ms rss={}kB",
            now_iso(),
            tag,
            self.ui_calls.load(Ordering::Relaxed),
            self.paints.load(Ordering::Relaxed),
            self.produced.load(Ordering::Relaxed),
            self.consumed.load(Ordering::Relaxed),
            self.queued.load(Ordering::Relaxed),
            self.max_queue.load(Ordering::Relaxed),
            self.inflight.load(Ordering::Relaxed),
            self.max_inflight.load(Ordering::Relaxed),
            self.blocked_sends.load(Ordering::Relaxed),
            self.lat_min_us.load(Ordering::Relaxed) as f64 / 1000.0,
            self.lat_sum_us.load(Ordering::Relaxed) as f64
                / 1000.0
                / self.consumed.load(Ordering::Relaxed).max(1) as f64,
            self.lat_max_us.load(Ordering::Relaxed) as f64 / 1000.0,
            rss_kb(),
        );
        let _ = f.flush();
    }

    fn note_raw(&self, tag: &str, msg: &str) {
        let mut f = self.log.lock().unwrap();
        let _ = writeln!(f, "[{}] {:<10} {}", now_iso(), tag, msg);
        let _ = f.flush();
    }

    fn snapshot(&self) -> Stats {
        let consumed = self.consumed.load(Ordering::Relaxed);
        let mut s = Stats {
            produced: self.produced.load(Ordering::Relaxed),
            consumed,
            lat_sum_ms: self.lat_sum_us.load(Ordering::Relaxed) as f64 / 1000.0,
            lat_min_ms: if self.lat_min_us.load(Ordering::Relaxed) == u64::MAX {
                0.0
            } else {
                self.lat_min_us.load(Ordering::Relaxed) as f64 / 1000.0
            },
            lat_max_ms: self.lat_max_us.load(Ordering::Relaxed) as f64 / 1000.0,
            blocked_sends: self.blocked_sends.load(Ordering::Relaxed),
            send_await_sum_ms: self.send_await_sum_us.load(Ordering::Relaxed) as f64 / 1000.0,
            max_queue: self.max_queue.load(Ordering::Relaxed),
        };
        s.max_queue = s.max_queue.max(0);
        s
    }
}

fn rss_kb() -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("VmRSS:") {
            return v
                .trim()
                .trim_end_matches("kB")
                .trim()
                .parse()
                .unwrap_or(0);
        }
    }
    0
}

fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let secs = d.as_secs();
    let ms = d.subsec_millis();
    let t = chrono::DateTime::from_timestamp(secs as i64, 0).unwrap();
    format!("{}.{:03}", t.format("%H:%M:%S"), ms)
}

fn now_us() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64
}

// ---------------------------------------------------------------- messages

enum HardwareUpdate {
    /// Mirrors app.rs: a payload produced by a tokio::spawn'd D-Bus task.
    Sample(u64),
}

/// app.rs registers 11 components; the tightest rates are 1 s.
/// We use a 1 s "cpu" tick to match the real poll rate, and scale down
/// with a multiplier so we can stress the pipeline.
struct Producer {
    tx: mpsc::Sender<HardwareUpdate>,
    tele: Arc<Telemetry>,
    interval: Duration,
}

impl Producer {
    /// Mirrors app.rs lines 268-364: bounded channel, coordinator, spawn per tick.
    fn spawn(tele: Arc<Telemetry>, interval: Duration) -> mpsc::Receiver<HardwareUpdate> {
        let (tx, rx) = mpsc::channel::<HardwareUpdate>(100);
        let p = Producer {
            tx: tx.clone(),
            tele: tele.clone(),
            interval,
        };
        tokio::spawn(async move {
            let mut n: u64 = 0;
            loop {
                tokio::time::sleep(p.interval).await;
                n += 1;
                let tele = p.tele.clone();
                let tx = p.tx.clone();
                // app.rs: tokio::spawn(async move { ... tx.send(..).await })
                tokio::spawn(async move {
                    // stand-in for the awaited D-Bus round trip
                    tokio::time::sleep(Duration::from_millis(5)).await;
                    let t0 = now_us();
                    tele.t_produced.store(t0, Ordering::Relaxed);
                    let inf = tele.inflight.fetch_add(1, Ordering::SeqCst) + 1;
                    tele.max_inflight.fetch_max(inf, Ordering::Relaxed);
                    let q = 100 - tx.capacity();
                    tele.queued.store(q, Ordering::Relaxed);
                    tele.max_queue.fetch_max(q, Ordering::Relaxed);
                    let s0 = Instant::now();
                    let _ = tx.send(HardwareUpdate::Sample(n)).await;
                    let await_ms = s0.elapsed().as_secs_f64() * 1000.0;
                    tele.send_await_sum_us
                        .fetch_add((await_ms * 1000.0) as u64, Ordering::Relaxed);
                    if await_ms > 1.0 {
                        tele.blocked_sends.fetch_add(1, Ordering::Relaxed);
                    }
                    tele.inflight.fetch_sub(1, Ordering::SeqCst);
                    tele.produced.fetch_add(1, Ordering::Relaxed);
                });
            }
        });
        rx
    }
}

// ---------------------------------------------------------------- the app

struct Probe {
    tele: Arc<Telemetry>,
    rx: mpsc::Receiver<HardwareUpdate>,
    mode: String,
    t0: Instant,
    frames: u64,
    quit_after: Option<Duration>,
    // phase bookkeeping
    phase: u32,
    last_phase_switch: Instant,
    tray: Option<ksni_tray::Tray>,
    panel_mode: bool,
    toggle_count: u64,
    rss_at_start: u64,
    ui_at_hide: u64,
}

impl Probe {
    fn env_report(&self, ctx: &egui::Context, label: &str) {
        let vp = ctx.input(|i| i.viewport().clone());
        let inner = vp
            .inner_rect
            .map(|r| format!("{:.0}x{:.0}", r.width(), r.height()))
            .unwrap_or_else(|| "None".into());
        let outer = vp
            .outer_rect
            .map(|r| format!("{:.0}x{:.0}", r.width(), r.height()))
            .unwrap_or_else(|| "None".into());
        let pos = vp
            .outer_rect
            .map(|r| format!("x={:.0} y={:.0}", r.min.x, r.min.y))
            .unwrap_or_else(|| "None".into());
        self.tele.note_raw(
            "VIEWPORT",
            &format!(
                "{label:<28} inner={inner} outer={outer} {pos} min={:?} max={:?} focused={:?} minimized={:?} maximized={:?} occluded={:?} screen={:?}",
                vp.minimized, vp.maximized, vp.focused, vp.minimized, vp.maximized, vp.occluded, vp.monitor_size
            ),
        );
    }
}

impl eframe::App for Probe {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.tele.ui_calls.fetch_add(1, Ordering::Relaxed);
        let frame_no = self.frames;
        self.frames += 1;

        // ---- app.rs handle_hardware_updates() equivalent (line 501) ----
        let mut n = 0usize;
        while let Ok(_u) = self.rx.try_recv() {
            let t0 = self.tele.t_produced.swap(0, Ordering::Relaxed);
            if t0 != 0 {
                let d = now_us().saturating_sub(t0);
                self.tele.lat_sum_us.fetch_add(d, Ordering::Relaxed);
                self.tele.lat_min_us.fetch_min(d, Ordering::Relaxed);
                self.tele.lat_max_us.fetch_max(d, Ordering::Relaxed);
            }
            self.tele.consumed.fetch_add(1, Ordering::Relaxed);
            n += 1;
        }
        if n > 0 {
            self.tele
                .queued
                .store(100usize.saturating_sub(self.rx.capacity()), Ordering::Relaxed);
        }

        // ---- scenario driver ----
        let elapsed = self.t0.elapsed();
        match self.mode.as_str() {
            "latency" => {
                if elapsed.as_secs() % 10 == 0 && frame_no % 30 == 0 {
                    self.tele.note("LATENCY");
                }
                if elapsed > self.quit_after.unwrap() {
                    let s = self.tele.snapshot();
                    self.tele.note_raw(
                        "RESULT",
                        &format!(
                            "latency over {}s: consumed={} min={:.1}ms avg={:.1}ms max={:.1}ms | blocked_sends={} max_inflight={} max_queue={} rss_end={}kB",
                            self.quit_after.unwrap().as_secs(),
                            s.consumed, s.lat_min_ms,
                            s.lat_sum_ms / s.consumed.max(1) as f64,
                            s.lat_max_ms, s.blocked_sends,
                            self.tele.max_inflight.load(Ordering::Relaxed),
                            s.max_queue, rss_kb()
                        ),
                    );
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }

            "hidden" => {
                // t=5s hide, t=hide+duration show again
                let hide_at = Duration::from_secs(5);
                let show_at = hide_at + self.quit_after.unwrap();
                if elapsed >= hide_at && self.phase == 0 {
                    self.phase = 1;
                    self.last_phase_switch = Instant::now();
                    self.ui_at_hide = self.tele.ui_calls.load(Ordering::Relaxed);
                    self.tele.note_raw("HIDE", &format!("sending Visible(false); ui_calls={} rss={}kB", self.ui_at_hide, rss_kb()));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
                if self.phase == 1 {
                    let he = self.last_phase_switch.elapsed().as_secs();
                    if he % 30 == 0
                        && self.tele.ui_calls.load(Ordering::Relaxed) % 5 == 0
                    {
                        let s = self.tele.snapshot();
                        self.tele.note_raw(
                            "HIDDEN",
                            &format!(
                                "t+{}s ui_calls={} (+{} since hide) queue~={} maxq={} inflight={} maxinflight={} blocked={} produced={} consumed={} rss={}kB",
                                he,
                                self.tele.ui_calls.load(Ordering::Relaxed),
                                self.tele.ui_calls.load(Ordering::Relaxed) - self.ui_at_hide,
                                self.tele.queued.load(Ordering::Relaxed),
                                self.tele.max_queue.load(Ordering::Relaxed),
                                self.tele.inflight.load(Ordering::Relaxed),
                                self.tele.max_inflight.load(Ordering::Relaxed),
                                self.tele.blocked_sends.load(Ordering::Relaxed),
                                s.produced, s.consumed, rss_kb()
                            ),
                        );
                    }
                    if self.last_phase_switch.elapsed() > self.quit_after.unwrap() {
                        self.phase = 2;
                        let s = self.tele.snapshot();
                        self.tele.note_raw(
                            "SHOW",
                            &format!(
                                "Visible(true) after {}s hidden: ui_calls={} queue~={} blocked={} maxq={} rss={}kB",
                                self.quit_after.unwrap().as_secs(),
                                self.tele.ui_calls.load(Ordering::Relaxed),
                                self.tele.queued.load(Ordering::Relaxed),
                                s.blocked_sends, s.max_queue, rss_kb()
                            ),
                        );
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        // measure re-show latency
                        self.tele.t_produced.store(now_us(), Ordering::Relaxed);
                        self.last_phase_switch = Instant::now();
                    }
                }
                if self.phase == 2 && self.last_phase_switch.elapsed() > Duration::from_secs(10) {
                    let s = self.tele.snapshot();
                    self.tele.note_raw(
                        "RESULT",
                        &format!(
                            "hidden test total: ui_calls={} consumed={} produced={} blocked={} maxq={} maxinflight={} rss_end={}kB",
                            self.tele.ui_calls.load(Ordering::Relaxed), s.consumed, s.produced,
                            s.blocked_sends, s.max_queue,
                            self.tele.max_inflight.load(Ordering::Relaxed), rss_kb()
                        ),
                    );
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }

            "cmds" => self.step_cmds(&ctx),

            "toggle" => {
                // 300 mode switches, then a tray Show/Hide round trip
                let target: u64 = 300;
                if self.toggle_count < target && self.frames % 4 == 0 {
                    self.panel_mode = !self.panel_mode;
                    self.toggle_count += 1;
                    if self.panel_mode {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(vec2(
                            PANEL_SIZE[0], PANEL_SIZE[1],
                        )));
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(
                            PANEL_SIZE[0], PANEL_SIZE[1],
                        )));
                    } else {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(vec2(
                            FULL_SIZE[0], FULL_SIZE[1],
                        )));
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(
                            FULL_SIZE[0], FULL_SIZE[1],
                        )));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
                    }
                }
                if self.toggle_count == target && self.phase == 0 {
                    self.phase = 1;
                    self.rss_at_start = rss_kb();
                    self.tele.note_raw(
                        "TOGGLE",
                        &format!("{target} switches done, rss={}kB -> entering tray test", self.rss_at_start),
                    );
                }
                if self.phase == 1 {
                    self.env_report(&ctx, "after 300 switches");
                    // emulate tray ShowWindow
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    self.phase = 2;
                    self.last_phase_switch = Instant::now();
                }
                if self.phase == 2 && self.last_phase_switch.elapsed() > Duration::from_secs(3) {
                    self.tele
                        .note_raw("TRAY", "sending Visible(true) (ShowWindow equivalent)");
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    self.phase = 3;
                    self.last_phase_switch = Instant::now();
                }
                if self.phase == 3 && self.last_phase_switch.elapsed() > Duration::from_secs(5) {
                    self.env_report(&ctx, "after tray Show/Hide");
                    self.tele.note_raw(
                        "RESULT",
                        &format!(
                            "toggle test: rss_start={}kB rss_end={}kB delta={}kB maxq={} blocked={} ui_calls={}",
                            self.rss_at_start, rss_kb(),
                            rss_kb() as i64 - self.rss_at_start as i64,
                            self.tele.max_queue.load(Ordering::Relaxed),
                            self.tele.blocked_sends.load(Ordering::Relaxed),
                            self.tele.ui_calls.load(Ordering::Relaxed)
                        ),
                    );
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }

            "panelboot" => {
                if self.frames <= 12 {
                    self.env_report(&ctx, &format!("frame {frame_no}"));
                }
                if elapsed > Duration::from_secs(5) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            "watch" => {
                // no in-app window manipulation: the driver script (xdotool)
                // minimizes / occludes / shows the window from outside.
                if frame_no % 60 == 0 {
                    self.env_report(&ctx, "watch");
                    self.tele.note("WATCH");
                }
                if elapsed > self.quit_after.unwrap() {
                    let s = self.tele.snapshot();
                    self.tele.note_raw(
                        "RESULT",
                        &format!(
                            "watch: ui_calls={} (~{:.1}/s) consumed={} blocked={} maxq={} maxinflight={} rss={}kB",
                            self.tele.ui_calls.load(Ordering::Relaxed),
                            self.tele.ui_calls.load(Ordering::Relaxed) as f64
                                / self.quit_after.unwrap().as_secs_f64(),
                            s.consumed, s.blocked_sends, s.max_queue,
                            self.tele.max_inflight.load(Ordering::Relaxed), rss_kb()
                        ),
                    );
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            _ => {}
        }

        // ---- draw something (mimics a panel) ----
        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.heading(if self.panel_mode { "PANEL" } else { "FULL" });
            ui.label(format!("frames: {}", self.frames));
            ui.label(format!("ui calls: {}", self.tele.ui_calls.load(Ordering::Relaxed)));
            ui.label(format!("rss: {} kB", rss_kb()));
            ui.label(format!("queue~: {}", self.tele.queued.load(Ordering::Relaxed)));
        });

        // this frame's work is done; eframe paints right after ui() returns.
        self.tele.paints.fetch_add(1, Ordering::Relaxed);
        self.tele.frame_end_us.store(now_us(), Ordering::Relaxed);

        // app.rs line 847: the ONLY repaint request in the whole app
        ctx.request_repaint_after(Duration::from_millis(500));
    }
}

impl Probe {
    fn step_cmds(&mut self, ctx: &egui::Context) {
        // (label, command) applied one per phase; settle for 12 frames between.
        let steps: Vec<(&str, Option<egui::ViewportCommand>)> = vec![
            ("00 baseline", None),
            ("01 InnerSize(1200x800)", Some(egui::ViewportCommand::InnerSize(vec2(1200.0, 800.0)))),
            ("02 MinInnerSize(300x200)", Some(egui::ViewportCommand::MinInnerSize(vec2(300.0, 200.0)))),
            ("03 MinInnerSize(800x600)", Some(egui::ViewportCommand::MinInnerSize(vec2(800.0, 600.0)))),
            ("04 Decorations(false)", Some(egui::ViewportCommand::Decorations(false))),
            ("05 Decorations(true)", Some(egui::ViewportCommand::Decorations(true))),
            ("06 WindowLevel(AlwaysOnTop)", Some(egui::ViewportCommand::WindowLevel(egui::WindowLevel::AlwaysOnTop))),
            ("07 WindowLevel(AlwaysOnBottom)", Some(egui::ViewportCommand::WindowLevel(egui::WindowLevel::AlwaysOnBottom))),
            ("08 WindowLevel(Normal)", Some(egui::ViewportCommand::WindowLevel(egui::WindowLevel::Normal))),
            ("09 Resizable(false)", Some(egui::ViewportCommand::Resizable(false))),
            ("10 Resizable(true)", Some(egui::ViewportCommand::Resizable(true))),
            ("11 OuterPosition(200,150)", Some(egui::ViewportCommand::OuterPosition(egui::pos2(200.0, 150.0)))),
            ("12 OuterPosition(-1)", Some(egui::ViewportCommand::OuterPosition(egui::pos2(2000.0, 1200.0)))),
            ("13 Visible(false)", Some(egui::ViewportCommand::Visible(false))),
            ("14 Visible(true)", Some(egui::ViewportCommand::Visible(true))),
            ("15 after re-show", None),
        ];
        let i = self.phase as usize;
        if i >= steps.len() {
            self.tele
                .note_raw("RESULT", "cmds sequence complete");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.frames % 12 != 0 {
            return;
        }
        let (label, cmd) = &steps[i];
        match cmd {
            Some(c) => {
                self.tele.note_raw("CMD", &format!("sending {label}"));
                ctx.send_viewport_cmd(c.clone());
            }
            None => {
                self.tele.note_raw("CMD", &format!("baseline: {label}"));
            }
        }
        // the command is applied by the winit event loop; report state on the
        // NEXT settle step so the change is visible in the report.
        if self.phase > 0 {
            if let Some((prev_label, _)) = steps.get(i as usize - 1) {
                self.env_report(ctx, &format!("after {prev_label}"));
            }
        }
        // Visible(false) still keeps painting on X11; we do not need it to
        if *label == "13 Visible(false)" {
            // emit a shell xprop dump right now, while hidden
            self.dump_x(&format!("hidden-{}", i));
        }
        self.phase += 1;
    }

    fn dump_x(&self, tag: &str) {
        // External ground truth via xdotool/xprop on the X11 window.
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg("W=$(xdotool search --name 'LapSpherePanelProbe' | tail -1); \
                  echo \"win=$W geom=$(xdotool getwindowgeometry $W 2>/dev/null | tr '\\n' ' ')\"; \
                  xprop -id $W _NET_WM_STATE WM_STATE _NET_WM_WINDOW_TYPE WM_NORMAL_HINTS 2>/dev/null | tr '\\n' '|'; echo")
            .output();
        match out {
            Ok(o) => self.tele.note_raw(
                "XPROP",
                &format!("{tag}: {}", String::from_utf8_lossy(&o.stdout).trim()),
            ),
            Err(e) => self.tele.note_raw("XPROP", &format!("{tag}: failed {e}")),
        }
    }
}

// ---------------------------------------------------------------- tray stub

mod ksni_tray {
    use ksni::{blocking::TrayMethods, menu::StandardItem, Tray as _KsniTray};
    use std::sync::mpsc;

    struct T;
    impl _KsniTray for T {
        fn id(&self) -> String { "lapsphere-panel-probe".into() }
        fn title(&self) -> String { "probe".into() }
        fn icon_pixmap(&self) -> Vec<ksni::Icon> { Vec::new() }
        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            vec![StandardItem {
                label: "Show Window".into(),
                activate: Box::new(|_t: &mut Self| {}),
                ..Default::default()
            }.into()]
        }
    }
    pub struct Tray(pub ksni::blocking::Handle<T>, pub mpsc::Receiver<()>);
    pub fn new() -> anyhow::Result<Tray> {
        let (tx, rx) = mpsc::channel();
        Ok(Tray(T.spawn()?, rx))
    }
}

// ---------------------------------------------------------------- main

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).cloned().unwrap_or_else(|| "latency".into());
    let secs: u64 = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(match mode.as_str() {
            "latency" => 60,
            "hidden" => 600,
            "cmds" => 60,
            "toggle" => 120,
            "watch" => 90,
            _ => 10,
        });
    let out = std::env::var("PROBE_LOG").unwrap_or_else(|_| {
        format!("/home/wer/devis/lapsphere/probe/panel_probe_{mode}.log")
    });
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    let tele = Arc::new(Telemetry::new(&out));
    tele.note_raw("START", &format!("mode={mode} secs={secs} DISPLAY={:?} WAYLAND_DISPLAY={:?} XDG_SESSION_TYPE={:?} XDG_CURRENT_DESKTOP={:?}", std::env::var("DISPLAY").ok(), std::env::var("WAYLAND_DISPLAY").ok(), std::env::var("XDG_SESSION_TYPE").ok(), std::env::var("XDG_CURRENT_DESKTOP").ok()));

    let rt = tokio::runtime::Runtime::new().unwrap();
    let _enter = rt.enter();

    // 1 s "cpu" tick = the real poll rate from common/src/types.rs default
    let interval = Duration::from_millis(1000);
    let rx = Producer::spawn(tele.clone(), interval);

    // panel-mode geometry when asked, otherwise the app's real defaults
    let (size, deco, resizable) = if mode == "panelboot" {
        (PANEL_SIZE, false, false)
    } else {
        (FULL_SIZE, true, true)
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(size)
            .with_min_inner_size(if mode == "panelboot" {
                vec2(size[0], size[1])
            } else {
                vec2(440.0, 470.0)
            })
            .with_resizable(resizable)
            .with_decorations(deco)
            .with_position(if mode == "panelboot" {
                egui::pos2(20.0, 20.0)
            } else {
                egui::pos2(300.0, 200.0)
            })
            .with_title("LapSpherePanelProbe"),
        ..Default::default()
    };

    // RSS sampler thread: keeps sampling even if ui() stops being called.
    let ts = tele.clone();
    std::thread::spawn(move || {
        let mut last = 0;
        while !ts.stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(5));
            let r = rss_kb();
            if last != 0 {
                ts.note_raw("RSS", &format!("rss={r}kB delta={}kB ui_calls={}", r as i64 - last as i64, ts.ui_calls.load(Ordering::Relaxed)));
            }
            last = r;
        }
    });

    let tray = if mode == "toggle" { ksni_tray::new().ok() } else { None };
    tele.note_raw("TRAY", &format!("tray handle created: {}", tray.is_some()));

    let app = Probe {
        tele: tele.clone(),
        rx,
        mode: mode.clone(),
        t0: Instant::now(),
        frames: 0,
        quit_after: Some(Duration::from_secs(secs)),
        phase: 0,
        last_phase_switch: Instant::now(),
        tray,
        panel_mode: false,
        toggle_count: 0,
        rss_at_start: 0,
        ui_at_hide: 0,
    };

    let r = eframe::run_native("LapSpherePanelProbe", options, Box::new(move |_cc| Ok(Box::new(app))));
    tele.note_raw("EXIT", &format!("run_native -> {r:?}"));
    tele.stop.store(true, Ordering::Relaxed);
}
