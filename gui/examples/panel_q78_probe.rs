// Throwaway probe for Q7 (stacking above a fullscreen game) and Q8 (taskbar
// exclusion) on X11.
//
// Adds to panel_overlay_demo.rs's feature set:
//   --reassert-sec N        re-send WindowLevel(AlwaysOnTop) every N seconds
//   --skip-taskbar-prop     set _NET_WM_STATE_SKIP_TASKBAR / _SKIP_PAGER by
//                           property write once the window exists
//   --skip-taskbar-msg      same, but via a _NET_WM_STATE ClientMessage
//   --x11-type dock|utility|toolbar|normal|desktop
//   --override-redirect     override_redirect(true)
//
// Property/message writes need the raw X window id, which eframe exposes
// through Frame::window_handle() (HasWindowHandle). No new dependency: the
// window id is read from the RawWindowHandle, and the EWMH writes are done by
// shelling out to `xprop` / `xdotool`, which are already required by the test
// harness. x11rb 0.13.2 is present in Cargo.lock (via winit and arboard) but
// is not a direct dependency of the gui crate, so using it from here would
// require editing gui/Cargo.toml -- deliberately not done here.
//
// NOT production code. Does not touch gui/src/**.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui;
use eframe::egui::vec2;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tokio::sync::mpsc;

const PANEL_W: f32 = 420.0;
const PANEL_H: f32 = 46.0;

struct Tel {
    log: Mutex<std::fs::File>,
    stop: AtomicBool,
    frames: AtomicU64,
}
impl Tel {
    fn note(&self, m: &str) {
        let mut f = self.log.lock().unwrap();
        let _ = writeln!(f, "[{}] {}", ts(), m);
        let _ = f.flush();
    }
}
fn ts() -> String {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
        .unwrap()
        .format("%H:%M:%S")
        .to_string()
        + &format!(".{:03}", d.subsec_millis())
}
fn sh(cmd: &str) -> String {
    let o = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::null())
        .output();
    match o {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        Err(e) => format!("<err {e}>"),
    }
}

struct Cfg {
    reassert_sec: f64,
    skip_prop: bool,
    skip_msg: bool,
    x11_type: String,
    override_redirect: bool,
    seconds: u64,
    x: f32,
    y: f32,
}

struct App {
    tele: Arc<Tel>,
    cfg: Cfg,
    t0: Instant,
    frames: u64,
    win_id: Option<u64>,
    did_prop: bool,
    did_msg: bool,
    sent_initial_aot: bool,
    reasserts: u64,
    last_reassert: Instant,
    _rx: mpsc::Receiver<u8>,
}

impl App {
    /// Apply the EWMH property writes once the X window exists. Done from
    /// inside the egui loop because that is the first point at which
    /// Frame::window_handle() is available.
    fn maybe_write_props(&mut self, ctx: &egui::Context) {
        if self.did_prop && self.did_msg {
            return;
        }
        if self.win_id.is_none() {
            return;
        }
        let w = self.win_id.unwrap();
        if self.cfg.skip_prop && !self.did_prop {
            let t0 = Instant::now();
            // _NET_WM_STATE is a list of atoms; -set replaces it, so both
            // atoms are written in one call.
            let out = sh(&format!(
                "xprop -id {w} -f _NET_WM_STATE 32a -set _NET_WM_STATE \
                 '_NET_WM_STATE_SKIP_TASKBAR, _NET_WM_STATE_SKIP_PAGER'"
            ));
            self.tele.note(&format!(
                "PROP set _NET_WM_STATE SKIP_TASKBAR+SKIP_PAGER on {w} in {:?}: {}",
                t0.elapsed(),
                out
            ));
            self.did_prop = true;
            let after = sh(&format!("xprop -id {w} _NET_WM_STATE"));
            self.tele.note(&format!("PROP result: {after}"));
        }
        if self.cfg.skip_msg && !self.did_msg {
            let t0 = Instant::now();
            // ClientMessage route: _NET_WM_STATE = _NET_WM_STATE_REMOVE for
            // SKIP_PAGER (a toggle-capable atom) and ADD for SKIP_TASKBAR.
            let out = sh(&format!(
                "xdotool windowstate --add SKIP_TASKBAR {w}; \
                 xdotool windowstate --add SKIP_PAGER {w}"
            ));
            self.tele.note(&format!(
                "MSG clientmessage SKIP_TASKBAR+SKIP_PAGER on {w} in {:?}: {}",
                t0.elapsed(),
                out
            ));
            self.did_msg = true;
            let after = sh(&format!("xprop -id {w} _NET_WM_STATE"));
            self.tele.note(&format!("MSG result: {after}"));
        }
        let _ = ctx;
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.tele.frames.fetch_add(1, Ordering::Relaxed);
        self.frames += 1;

        // First frame: capture the X window id.
        if self.win_id.is_none() {
            if let Ok(h) = frame.window_handle() {
                let raw = h.as_raw();
                if let RawWindowHandle::Xlib(x) = raw {
                    self.win_id = Some(x.window);
                    self.tele.note(&format!(
                        "window id from Frame::window_handle(): {} (visual {})",
                        x.window, x.visual_id
                    ));
                } else {
                    self.tele.note("window handle is not Xlib");
                }
            }
        }
        // B2 showed with_window_level() at startup does not stick on xfwm4,
        // but the RUNTIME ViewportCommand::WindowLevel does. So every run
        // sends it once after the window exists, which is the baseline the
        // re-assert variants then build on.
        if !self.sent_initial_aot {
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::AlwaysOnTop,
            ));
            self.sent_initial_aot = true;
            self.tele.note("AOT sent once at runtime (ViewportCommand)");
        }
        self.maybe_write_props(&ctx);

        // Re-assert always-on-top periodically (Q7 follow-up).
        if self.cfg.reassert_sec > 0.0
            && self.last_reassert.elapsed().as_secs_f64() >= self.cfg.reassert_sec
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::AlwaysOnTop,
            ));
            self.last_reassert = Instant::now();
            self.reasserts += 1;
            self.tele.note(&format!(
                "REASSERT #{} WindowLevel(AlwaysOnTop) at t+{:.0}s",
                self.reasserts,
                self.t0.elapsed().as_secs_f64()
            ));
        }

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.heading("Q7/Q8 PROBE");
            ui.label(format!("win={:?}", self.win_id));
            ui.label(format!("reasserts={} t+{:.0}s", self.reasserts, self.t0.elapsed().as_secs_f64()));
        });

        ctx.request_repaint_after(Duration::from_millis(500));

        if self.cfg.seconds > 0 && self.t0.elapsed() > Duration::from_secs(self.cfg.seconds) {
            self.tele.note(&format!(
                "EXIT frames={} reasserts={}",
                self.frames, self.reasserts
            ));
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let logp = std::env::var("PROBE_LOG")
        .unwrap_or_else(|_| "/home/wer/devis/lapsphere/probe/ov/q78.log".into());
    let mut cfg = Cfg {
        reassert_sec: 0.0,
        skip_prop: false,
        skip_msg: false,
        x11_type: "normal".into(),
        override_redirect: false,
        seconds: 0,
        x: 200.0,
        y: 700.0,
    };
    let mut i = 0;
    while i < a.len() {
        match a[i].as_str() {
            "--reassert-sec" => { cfg.reassert_sec = a[i+1].parse().unwrap_or(0.0); i += 2 }
            "--skip-taskbar-prop" => { cfg.skip_prop = true; i += 1 }
            "--skip-taskbar-msg" => { cfg.skip_msg = true; i += 1 }
            "--x11-type" => { cfg.x11_type = a[i+1].clone(); i += 2 }
            "--override-redirect" => { cfg.override_redirect = true; i += 1 }
            "--seconds" => { cfg.seconds = a[i+1].parse().unwrap_or(0); i += 2 }
            "--x" => { cfg.x = a[i+1].parse().unwrap_or(200.0); i += 2 }
            "--y" => { cfg.y = a[i+1].parse().unwrap_or(700.0); i += 2 }
            other => { eprintln!("unknown arg {other}"); i += 1 }
        }
    }

    let f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&logp)
        .expect("open log");
    let tele = Arc::new(Tel {
        log: Mutex::new(f),
        stop: AtomicBool::new(false),
        frames: AtomicU64::new(0),
    });
    tele.note(&format!(
        "START type={} override_redirect={} skip_prop={} skip_msg={} reassert_sec={} seconds={}",
        cfg.x11_type, cfg.override_redirect, cfg.skip_prop, cfg.skip_msg,
        cfg.reassert_sec, cfg.seconds
    ));

    let rt = tokio::runtime::Runtime::new().unwrap();
    let _enter = rt.enter();
    let (_tx, rx) = mpsc::channel::<u8>(1);

    let wt = match cfg.x11_type.as_str() {
        "dock" => egui::X11WindowType::Dock,
        "utility" => egui::X11WindowType::Utility,
        "toolbar" => egui::X11WindowType::Toolbar,
        "desktop" => egui::X11WindowType::Desktop,
        _ => egui::X11WindowType::Normal,
    };

    let mut vb = egui::ViewportBuilder::default()
        .with_inner_size(vec2(PANEL_W, PANEL_H))
        .with_decorations(false)
        .with_resizable(false)
        .with_title("Q78 Probe")
        .with_position(egui::pos2(cfg.x, cfg.y))
        .with_window_type(wt)
        .with_override_redirect(cfg.override_redirect);
    // The level is deliberately NOT set at startup: B2 showed
    // with_window_level(AlwaysOnTop) does not stick on xfwm4.

    let opts = eframe::NativeOptions { viewport: vb, ..Default::default() };
    let app = App {
        tele: tele.clone(),
        cfg: Cfg {
            reassert_sec: if cfg.reassert_sec > 0.0 { 0.0 } else { cfg.reassert_sec },
            ..cfg
        },
        t0: Instant::now(),
        frames: 0,
        win_id: None,
        did_prop: false,
        did_msg: false,
        sent_initial_aot: false,
        reasserts: 0,
        last_reassert: Instant::now(),
        _rx: rx,
    };

    let r = eframe::run_native("Q78 Probe", opts, Box::new(move |_cc| Ok(Box::new(app))));
    tele.note(&format!("run_native -> {r:?}"));
    tele.stop.store(true, Ordering::Relaxed);
}
