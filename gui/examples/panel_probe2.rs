// Throwaway spike probe #2 for the mini-panel investigation.
//
// Difference from panel_probe.rs: this one talks to the REAL running
// lapsphere-daemon over the system bus (Connection::system) using the same
// component set and the same poll intervals as gui/src/app.rs, so the
// measurements include real D-Bus round trips and real payload sizes.
//
// Still throwaway. Does not touch gui/src/**.
//
//   panel_probe2 <mode> [seconds]
//     latency    (a) HardwareUpdate -> consumed, visible window
//     hidden     (b) Visible(false) soak: ui() calls, queue depth, blocked senders, RSS
//     logic      (d) does eframe call App::logic() while hidden?
//     toggle     (г) 300 mode switches + tray Show/Hide afterwards
//     watch      external xdotool manipulation (minimize/occlude)

use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui;
use eframe::egui::vec2;
use tokio::sync::mpsc;
use zbus::Connection;

const PANEL_SIZE: [f32; 2] = [260.0, 72.0];
const FULL_SIZE: [f32; 2] = [570.0, 620.0];

// The 11 components gui/src/app.rs registers, with the default intervals from
// common/src/types.rs (AppConfig::default). `logs` is included so the
// component count matches the shipped app even though the real GUI now gates
// it behind should_fetch_logs().
const COMPONENTS: &[(&str, u64)] = &[
    ("cpu", 1000),
    ("memory", 1000),
    ("fans", 1000),
    ("gpu_overclock", 1000),
    ("gpu", 2000),
    ("battery", 5000),
    ("wifi", 5000),
    ("gamepads", 5000),
    ("storage", 5000),
    ("mount", 5000),
    ("webcam", 5000),
    ("logs", 5000),
];

fn now_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64
}

fn rss_kb() -> u64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    s.lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
        .unwrap_or(0)
}

fn threads_live() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix("Threads:"))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0)
}

fn ts() -> String {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let t = chrono::DateTime::from_timestamp(d.as_secs() as i64, 0).unwrap();
    format!("{}.{:03}", t.format("%H:%M:%S"), d.subsec_millis())
}

struct T {
    // produce/consume latency
    lat_sum: AtomicU64,
    lat_min: AtomicU64,
    lat_max: AtomicU64,
    // histogram buckets in ms: <5, <50, <100, <200, <350, <500, >=500
    hist: [AtomicU64; 7],
    produced: AtomicU64,
    consumed: AtomicU64,
    // back-pressure
    blocked: AtomicU64,
    send_await_sum_us: AtomicU64,
    inflight: AtomicUsize,
    max_inflight: AtomicUsize,
    max_queue: AtomicUsize,
    // frame counters
    ui: AtomicU64,
    logic: AtomicU64,
    paint: AtomicU64,
    // errors
    dbus_err: AtomicU64,
    dbus_ok: AtomicU64,
    log: Mutex<std::fs::File>,
    stop: AtomicBool,
}

impl T {
    fn new(path: &str) -> Self {
        let f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open log");
        Self {
            lat_sum: AtomicU64::new(0),
            lat_min: AtomicU64::new(u64::MAX),
            lat_max: AtomicU64::new(0),
            hist: Default::default(),
            produced: AtomicU64::new(0),
            consumed: AtomicU64::new(0),
            blocked: AtomicU64::new(0),
            send_await_sum_us: AtomicU64::new(0),
            inflight: AtomicUsize::new(0),
            max_inflight: AtomicUsize::new(0),
            max_queue: AtomicUsize::new(0),
            ui: AtomicU64::new(0),
            logic: AtomicU64::new(0),
            paint: AtomicU64::new(0),
            dbus_err: AtomicU64::new(0),
            dbus_ok: AtomicU64::new(0),
            log: Mutex::new(f),
            stop: AtomicBool::new(false),
        }
    }

    fn note(&self, tag: &str, msg: &str) {
        let mut f = self.log.lock().unwrap();
        let _ = writeln!(f, "[{}] {:<9} {}", ts(), tag, msg);
        let _ = f.flush();
    }

    fn note_lat(&self, tag: &str) {
        let c = self.consumed.load(Ordering::Relaxed);
        let hist: Vec<String> = self
            .hist
            .iter()
            .map(|h| format!("{}", h.load(Ordering::Relaxed)))
            .collect();
        self.note(
            tag,
            &format!(
                "ui={} logic={} paint={} | prod={} cons={} dbus_ok={} dbus_err={} \
                 | lat min={:.1} avg={:.1} max={:.1} ms hist[<5,<50,<100,<200,<350,<500,>=500]=[{}] \
                 | queue_max={} inflight={} maxinflight={} blocked={} rss={}kB threads={}",
                self.ui.load(Ordering::Relaxed),
                self.logic.load(Ordering::Relaxed),
                self.paint.load(Ordering::Relaxed),
                self.produced.load(Ordering::Relaxed),
                c,
                self.dbus_ok.load(Ordering::Relaxed),
                self.dbus_err.load(Ordering::Relaxed),
                if self.lat_min.load(Ordering::Relaxed) == u64::MAX {
                    0.0
                } else {
                    self.lat_min.load(Ordering::Relaxed) as f64 / 1000.0
                },
                self.lat_sum.load(Ordering::Relaxed) as f64 / 1000.0 / c.max(1) as f64,
                self.lat_max.load(Ordering::Relaxed) as f64 / 1000.0,
                hist.join(","),
                self.max_queue.load(Ordering::Relaxed),
                self.inflight.load(Ordering::Relaxed),
                self.max_inflight.load(Ordering::Relaxed),
                self.blocked.load(Ordering::Relaxed),
                rss_kb(),
                threads_live(),
            ),
        );
    }
}

enum HU {
    Blob(usize, u64), // payload bytes, produce timestamp
}

// ---------------------------------------------------------------- producer

/// Calls the same D-Bus methods app.rs calls, on the same schedule, and
/// pushes the reply down the same mpsc(100).
fn spawn_producer(t: Arc<T>, rx_will_close: Arc<AtomicBool>) -> mpsc::Receiver<HU> {
    let (tx, rx) = mpsc::channel::<HU>(100);
    tokio::spawn(async move {
        let conn = match Connection::system().await {
            Ok(c) => c,
            Err(e) => {
                t.note("FATAL", &format!("Connection::system failed: {e}"));
                return;
            }
        };
        let proxy = match zbus::Proxy::new(
            &conn,
            "io.lapsphere.Control",
            "/io/lapsphere/Control",
            "io.lapsphere.Control",
        )
        .await
        {
            Ok(p) => p,
            Err(e) => {
                t.note("FATAL", &format!("Proxy::new failed: {e}"));
                return;
            }
        };
        t.note(
            "DBUS",
            &format!(
                "connected to system bus, proxy OK; polling {} components",
                COMPONENTS.len()
            ),
        );

        // One task per component, exactly like app.rs: the coordinator's
        // refresh_callback tokio::spawn's a task per due component.
        for (name, interval_ms) in COMPONENTS {
            let proxy = proxy.clone();
            let tx = tx.clone();
            let t = t.clone();
            let name = name.to_string();
            let method = match name.as_str() {
                // app.rs maps "gpu_overclock" onto the full GPU getter.
                "cpu" => "GetCpuInfo",
                "memory" => "GetMemoryInfo",
                "fans" => "GetFanInfo",
                "gpu" | "gpu_overclock" => "GetGpuInfo",
                "battery" => "GetBatteryInfo",
                "wifi" => "GetWifiInfo",
                "gamepads" => "GetGamepadInfo",
                "storage" => "GetStorageDeviceInfo",
                "mount" => "GetMountInfo",
                "webcam" => "GetWebcamState",
                "logs" => "GetDaemonLogs",
                _ => continue,
            }
            .to_string();
            let iv = *interval_ms;
            let rx_will_close = rx_will_close.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(iv)).await;
                    if rx_will_close.load(Ordering::Relaxed) {
                        return;
                    }
                    let proxy = proxy.clone();
                    let tx = tx.clone();
                    let t = t.clone();
                    let method = method.clone();
                    tokio::spawn(async move {
                        let r: zbus::Result<String> = proxy.call(method.as_str(), &()).await;
                        match r {
                            Ok(s) => {
                                t.dbus_ok.fetch_add(1, Ordering::Relaxed);
                                let t0 = now_us();
                                let inf =
                                    t.inflight.fetch_add(1, Ordering::SeqCst) + 1;
                                t.max_inflight.fetch_max(inf, Ordering::Relaxed);
                                let q = 100usize.saturating_sub(tx.capacity());
                                t.max_queue.fetch_max(q, Ordering::Relaxed);
                                let s0 = Instant::now();
                                let ok =
                                    tx.send(HU::Blob(s.len(), t0)).await.is_ok();
                                let await_ms = s0.elapsed().as_secs_f64() * 1000.0;
                                t.send_await_sum_us.fetch_add(
                                    (await_ms * 1000.0) as u64,
                                    Ordering::Relaxed,
                                );
                                if await_ms > 1.0 {
                                    t.blocked.fetch_add(1, Ordering::Relaxed);
                                }
                                t.inflight.fetch_sub(1, Ordering::SeqCst);
                                if ok {
                                    t.produced.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                            Err(e) => {
                                t.dbus_err.fetch_add(1, Ordering::Relaxed);
                                t.note("DBUS_ERR", &format!("{method}: {e}"));
                            }
                        }
                    });
                }
            });
        }
    });
    rx
}

// ---------------------------------------------------------------- app

struct App {
    t: Arc<T>,
    rx: mpsc::Receiver<HU>,
    mode: String,
    t0: Instant,
    frames: u64,
    quit: Duration,
    phase: u32,
    phase_t: Instant,
    // hidden-tracking
    ui_at_hide: u64,
    logic_at_hide: u64,
    rss_at_switch_start: u64,
    switches: u64,
    panel: bool,
    // logic-mode: drain here instead of ui()
    drain_in_logic: bool,
    logic_drained: u64,
    tray: Option<Tray>,
    rx_closed_flag: Arc<AtomicBool>,
}

impl App {
    fn drain(&mut self) {
        while let Ok(HU::Blob(_bytes, t0)) = self.rx.try_recv() {
            let d = now_us().saturating_sub(t0);
            self.t.lat_sum.fetch_add(d, Ordering::Relaxed);
            self.t.lat_min.fetch_min(d, Ordering::Relaxed);
            self.t.lat_max.fetch_max(d, Ordering::Relaxed);
            let b = match d / 1000 {
                0..=4 => 0,
                5..=49 => 1,
                50..=99 => 2,
                100..=199 => 3,
                200..=349 => 4,
                350..=499 => 5,
                _ => 6,
            };
            self.t.hist[b].fetch_add(1, Ordering::Relaxed);
            self.t.consumed.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn env(&self, ctx: &egui::Context, label: &str) {
        let vp = ctx.input(|i| i.viewport().clone());
        self.t.note(
            "VIEWPORT",
            &format!(
                "{label:<26} inner={:?} outer={:?} pos={:?} focused={:?} minimized={:?} occluded={:?}",
                vp.inner_rect.map(|r| format!("{:.0}x{:.0}", r.width(), r.height())),
                vp.outer_rect.map(|r| format!("{:.0}x{:.0}", r.width(), r.height())),
                vp.outer_rect.map(|r| format!("{:.0},{:.0}", r.min.x, r.min.y)),
                vp.focused, vp.minimized, vp.occluded
            ),
        );
    }
}

impl eframe::App for App {
    /// eframe calls this UNCONDITIONALLY, even when the window is hidden
    /// (epi_integration.rs:285-287, outside the `if is_visible` block).
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.t.logic.fetch_add(1, Ordering::Relaxed);
        if self.drain_in_logic {
            let before = self.t.consumed.load(Ordering::Relaxed);
            self.drain();
            let got = self.t.consumed.load(Ordering::Relaxed) - before;
            self.logic_drained += got;
        }

        // mode: logic — prove logic() keeps running while the window is hidden
        if self.mode == "logic" {
            let e = self.t0.elapsed();
            if e.as_secs() % 30 == 0
                && self.t.logic.load(Ordering::Relaxed) % 25 == 0
            {
                self.t.note_lat("LOGIC");
            }
            if e >= Duration::from_secs(5) && self.phase == 0 {
                self.phase = 1;
                self.ui_at_hide = self.t.ui.load(Ordering::Relaxed);
                self.logic_at_hide = self.t.logic.load(Ordering::Relaxed);
                self.t.note(
                    "HIDE",
                    &format!(
                        "Visible(false) at ui={} logic={}",
                        self.ui_at_hide, self.logic_at_hide
                    ),
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
            if self.phase == 1 && self.phase_t.elapsed() > Duration::from_secs(60) {
                self.t.note(
                    "SHOW",
                    &format!(
                        "60s hidden: ui +{} (was {}) logic +{} (was {}) consumed={} queue_max={} blocked={} rss={}kB",
                        self.t.ui.load(Ordering::Relaxed) - self.ui_at_hide,
                        self.ui_at_hide,
                        self.t.logic.load(Ordering::Relaxed) - self.logic_at_hide,
                        self.logic_at_hide,
                        self.t.consumed.load(Ordering::Relaxed),
                        self.t.max_queue.load(Ordering::Relaxed),
                        self.t.blocked.load(Ordering::Relaxed),
                        rss_kb()
                    ),
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                self.phase = 2;
                self.phase_t = Instant::now();
            }
            if self.phase == 2 && self.phase_t.elapsed() > Duration::from_secs(20) {
                self.t.note(
                    "RESULT",
                    &format!(
                        "logic-mode total: ui={} logic={} consumed_in_logic={} prod={} \
                         queue_max={} blocked={} rss={}kB",
                        self.t.ui.load(Ordering::Relaxed),
                        self.t.logic.load(Ordering::Relaxed),
                        self.logic_drained,
                        self.t.produced.load(Ordering::Relaxed),
                        self.t.max_queue.load(Ordering::Relaxed),
                        self.t.blocked.load(Ordering::Relaxed),
                        rss_kb()
                    ),
                );
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.t.ui.fetch_add(1, Ordering::Relaxed);
        let n = self.frames;
        self.frames += 1;

        // the shipped placement: drain inside ui()
        if !self.drain_in_logic {
            self.drain();
        }

        let e = self.t0.elapsed();
        match self.mode.as_str() {
            "latency" => {
                if n % 90 == 0 {
                    self.t.note_lat("LATENCY");
                }
                if e > self.quit {
                    self.t.note_lat("RESULT");
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            "hidden" => {
                if e >= Duration::from_secs(5) && self.phase == 0 {
                    self.phase = 1;
                    self.ui_at_hide = self.t.ui.load(Ordering::Relaxed);
                    self.phase_t = Instant::now();
                    self.t.note(
                        "HIDE",
                        &format!(
                            "Visible(false) at ui={} rss={}kB",
                            self.ui_at_hide,
                            rss_kb()
                        ),
                    );
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
                if self.phase == 1 {
                    let h = self.phase_t.elapsed().as_secs();
                    if h % 60 == 0 && self.t.ui.load(Ordering::Relaxed) % 5 == 0 {
                        self.t.note(
                            "HIDDEN",
                            &format!(
                                "t+{}s ui_total={} ui_since_hide={} prod={} cons={} queue_max={} \
                                 inflight={} maxinflight={} blocked={} rss={}kB threads={}",
                                h,
                                self.t.ui.load(Ordering::Relaxed),
                                self.t.ui.load(Ordering::Relaxed) - self.ui_at_hide,
                                self.t.produced.load(Ordering::Relaxed),
                                self.t.consumed.load(Ordering::Relaxed),
                                self.t.max_queue.load(Ordering::Relaxed),
                                self.t.inflight.load(Ordering::Relaxed),
                                self.t.max_inflight.load(Ordering::Relaxed),
                                self.t.blocked.load(Ordering::Relaxed),
                                rss_kb(),
                                threads_live()
                            ),
                        );
                    }
                    if self.phase_t.elapsed() > self.quit {
                        self.t.note(
                            "SHOW",
                            &format!(
                                "after {}s hidden: ui+{} prod={} cons={} queue_max={} blocked={} rss={}kB",
                                self.quit.as_secs(),
                                self.t.ui.load(Ordering::Relaxed) - self.ui_at_hide,
                                self.t.produced.load(Ordering::Relaxed),
                                self.t.consumed.load(Ordering::Relaxed),
                                self.t.max_queue.load(Ordering::Relaxed),
                                self.t.blocked.load(Ordering::Relaxed),
                                rss_kb()
                            ),
                        );
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        self.phase = 2;
                        self.phase_t = Instant::now();
                    }
                }
                if self.phase == 2 && self.phase_t.elapsed() > Duration::from_secs(15) {
                    self.t.note(
                        "RESULT",
                        &format!(
                            "hidden-soak total: ui={} prod={} cons={} queue_max={} maxinflight={} \
                             blocked={} rss_end={}kB",
                            self.t.ui.load(Ordering::Relaxed),
                            self.t.produced.load(Ordering::Relaxed),
                            self.t.consumed.load(Ordering::Relaxed),
                            self.t.max_queue.load(Ordering::Relaxed),
                            self.t.max_inflight.load(Ordering::Relaxed),
                            self.t.blocked.load(Ordering::Relaxed),
                            rss_kb()
                        ),
                    );
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            "watch" => {
                if n % 60 == 0 {
                    self.env(&ctx, "watch");
                    self.t.note_lat("WATCH");
                }
                if e > self.quit {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            "toggle" => {
                let target = 300u64;
                if self.switches < target && n % 4 == 0 {
                    self.panel = !self.panel;
                    self.switches += 1;
                    if self.panel {
                        for c in [
                            egui::ViewportCommand::Decorations(false),
                            egui::ViewportCommand::Resizable(false),
                            egui::ViewportCommand::MinInnerSize(vec2(
                                PANEL_SIZE[0], PANEL_SIZE[1],
                            )),
                            egui::ViewportCommand::InnerSize(vec2(
                                PANEL_SIZE[0], PANEL_SIZE[1],
                            )),
                        ] {
                            ctx.send_viewport_cmd(c);
                        }
                    } else {
                        for c in [
                            egui::ViewportCommand::Resizable(true),
                            egui::ViewportCommand::MinInnerSize(vec2(
                                FULL_SIZE[0], FULL_SIZE[1],
                            )),
                            egui::ViewportCommand::InnerSize(vec2(
                                FULL_SIZE[0], FULL_SIZE[1],
                            )),
                            egui::ViewportCommand::Decorations(true),
                        ] {
                            ctx.send_viewport_cmd(c);
                        }
                    }
                }
                if self.switches >= target && self.phase == 0 {
                    self.phase = 1;
                    self.rss_at_switch_start = rss_kb();
                    self.t.note(
                        "TOGGLE",
                        &format!(
                            "{target} switches done; rss={}kB queue_max={} blocked={}; entering tray test",
                            self.rss_at_switch_start,
                            self.t.max_queue.load(Ordering::Relaxed),
                            self.t.blocked.load(Ordering::Relaxed)
                        ),
                    );
                }
                if self.phase == 1 {
                    self.env(&ctx, "after 300 switches");
                    // tray Show/Hide cycle
                    if let Some(tray) = self.tray.as_mut() {
                        match tray.handle_events() {
                            Some(TrayEvent::ShowWindow) => {
                                self.t.note("TRAY", "ShowWindow -> Visible(true)");
                                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(
                                    true,
                                ));
                            }
                            None => {
                                self.t.note(
                                    "TRAY",
                                    "handle_events -> None (no user click injected; expected)",
                                );
                            }
                            Some(_) => {
                                self.t.note("TRAY", "handle_events -> Some(event)");
                            }
                        }
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    self.phase = 2;
                    self.phase_t = Instant::now();
                }
                if self.phase == 2 && self.phase_t.elapsed() > Duration::from_secs(4) {
                    self.t.note(
                        "TRAY",
                        "after Visible(false): querying tray handle, then Visible(true)",
                    );
                    let alive = self
                        .tray
                        .as_mut()
                        .map(|t| t.handle_events().is_none())
                        .unwrap_or(false);
                    self.t.note(
                        "TRAY",
                        &format!("tray handle still responsive (no pending event) = {alive}"),
                    );
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    self.phase = 3;
                    self.phase_t = Instant::now();
                }
                if self.phase == 3 && self.phase_t.elapsed() > Duration::from_secs(6) {
                    self.env(&ctx, "after tray Show/Hide");
                    self.t.note(
                        "RESULT",
                        &format!(
                            "toggle: rss_start={}kB rss_end={}kB delta={}kB ui={} queue_max={} \
                             maxinflight={} blocked={} dbus_ok={} dbus_err={}",
                            self.rss_at_switch_start,
                            rss_kb(),
                            rss_kb() as i64 - self.rss_at_switch_start as i64,
                            self.t.ui.load(Ordering::Relaxed),
                            self.t.max_queue.load(Ordering::Relaxed),
                            self.t.max_inflight.load(Ordering::Relaxed),
                            self.t.blocked.load(Ordering::Relaxed),
                            self.t.dbus_ok.load(Ordering::Relaxed),
                            self.t.dbus_err.load(Ordering::Relaxed)
                        ),
                    );
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            _ => {}
        }

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.heading(if self.panel { "PANEL" } else { "FULL" });
            ui.label(format!("ui={} logic={}", self.t.ui.load(Ordering::Relaxed), self.t.logic.load(Ordering::Relaxed)));
            ui.label(format!("consumed={} rss={}kB", self.t.consumed.load(Ordering::Relaxed), rss_kb()));
        });

        self.t.paint.fetch_add(1, Ordering::Relaxed);
        ctx.request_repaint_after(Duration::from_millis(500));
    }
}

// Re-use the REAL tray implementation, unchanged, by including its source.
// Examples are separate crates, so `crate::tray` cannot resolve; #[path]
// pulls in gui/src/system_tray.rs verbatim.
#[path = "../src/system_tray.rs"]
mod tray;
use tray::{SystemTray as Tray, TrayEvent};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).cloned().unwrap_or_else(|| "latency".into());
    let secs: u64 = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(match mode.as_str() {
            "latency" => 120,
            "hidden" => 600,
            "logic" => 90,
            "toggle" => 240,
            _ => 90,
        });
    let drain_in_logic = args.iter().any(|a| a == "--drain-in-logic");
    let out = std::env::var("PROBE_LOG").unwrap_or_else(|_| {
        format!("/home/wer/devis/lapsphere/probe/p2_{mode}.log")
    });
    env_logger::builder()
        .filter_level(log::LevelFilter::Warn)
        .init();

    let t = Arc::new(T::new(&out));
    t.note(
        "START",
        &format!(
            "mode={mode} secs={secs} drain_in_logic={drain_in_logic} \
             DISPLAY={:?} XDG_SESSION_TYPE={:?} DESKTOP={:?} base=5c57bc7",
            std::env::var("DISPLAY").ok(),
            std::env::var("XDG_SESSION_TYPE").ok(),
            std::env::var("XDG_CURRENT_DESKTOP").ok()
        ),
    );

    let rt = tokio::runtime::Runtime::new().unwrap();
    let _enter = rt.enter();
    let flag = Arc::new(AtomicBool::new(false));
    let rx = spawn_producer(t.clone(), flag.clone());

    let (size, deco, resz) = if mode == "panelboot" {
        (PANEL_SIZE, false, false)
    } else {
        (FULL_SIZE, true, true)
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(size)
            .with_resizable(resz)
            .with_decorations(deco)
            .with_title("LapSpherePanelProbe2"),
        ..Default::default()
    };

    let tsamp = t.clone();
    std::thread::spawn(move || {
        let mut last = 0u64;
        while !tsamp.stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(10));
            let r = rss_kb();
            if last != 0 {
                tsamp.note(
                    "RSS",
                    &format!(
                        "rss={r}kB delta={}kB ui={} logic={} cons={} qmax={} blocked={} threads={}",
                        r as i64 - last as i64,
                        tsamp.ui.load(Ordering::Relaxed),
                        tsamp.logic.load(Ordering::Relaxed),
                        tsamp.consumed.load(Ordering::Relaxed),
                        tsamp.max_queue.load(Ordering::Relaxed),
                        tsamp.blocked.load(Ordering::Relaxed),
                        threads_live()
                    ),
                );
            }
            last = r;
        }
    });

    let tray = if mode == "toggle" {
        match Tray::new(&[], "") {
            Ok(tr) => {
                t.note("TRAY", "real SystemTray::new OK");
                Some(tr)
            }
            Err(e) => {
                t.note("TRAY", &format!("SystemTray::new failed: {e}"));
                None
            }
        }
    } else {
        None
    };

    let app = App {
        t: t.clone(),
        rx,
        mode: mode.clone(),
        t0: Instant::now(),
        frames: 0,
        quit: Duration::from_secs(secs),
        phase: 0,
        phase_t: Instant::now(),
        ui_at_hide: 0,
        logic_at_hide: 0,
        rss_at_switch_start: 0,
        switches: 0,
        panel: false,
        drain_in_logic,
        logic_drained: 0,
        tray,
        rx_closed_flag: flag,
    };

    let r = eframe::run_native(
        "LapSpherePanelProbe2",
        options,
        Box::new(move |_cc| Ok(Box::new(app))),
    );
    t.note("EXIT", &format!("run_native -> {r:?}"));
    t.stop.store(true, Ordering::Relaxed);
}
