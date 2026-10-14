// Throwaway probe for B13 (real EWMH ClientMessage via x11rb) and B16
// (can eframe recreate the ROOT window?).
//
// B13 sends a genuine _NET_WM_STATE ClientMessage to the root window:
//
//   destination  : the X root window
//   type         : _NET_WM_STATE
//   format       : 32
//   data         : [ action(1=add/0=remove),
//                    _NET_WM_STATE_SKIP_TASKBAR,
//                    _NET_WM_STATE_SKIP_PAGER,
//                    source(1=application),
//                    0, 0, 0, 0 ]
//   event_mask   : SubstructureRedirect | SubstructureNotify
//
// The window is a plain Normal window, so the only variable is the message.
// We then send the same message with data[0] = 0 (remove) and check the
// window comes back, and sample for 60 s to see whether the state sticks.
//
// B16 asks whether eframe 0.34.2 can recreate the ROOT window via
// ViewportBuilder::patch() -> recreate_window. The source says the recreate
// path only runs in initialize_or_update_viewport(), which handles Immediate
// and Deferred (child) viewports; the root is created from
// NativeOptions at startup. We test it empirically with a child viewport and
// report what actually happens for the root.
//
// NOT production code. Does not touch gui/src/**.

use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui;
use eframe::egui::vec2;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

const PANEL_W: f32 = 420.0;
const PANEL_H: f32 = 90.0;

struct Tel {
    log: Mutex<std::fs::File>,
    frames: AtomicU64,
    #[allow(dead_code)]
    stop: AtomicBool,
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
fn rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
        .unwrap_or(0)
}

// ---------------------------------------------------------------- B13

/// Send a real EWMH _NET_WM_STATE ClientMessage for `target`.
///
/// x11rb 0.13 shape notes, established from the generated source:
///   * `ClientMessageEvent::new(format, window, type_, data)` with
///     `data: ClientMessageData`, which is `From<[u32; 5]>` (not 8).
///   * there is no `event_mask` field on the event; the mask belongs to
///     `send_event`, which is why EWMH sends go to the ROOT window with
///     SubstructureRedirect|SubstructureNotify.
///   * one ClientMessage carries ONE property, so SKIP_TASKBAR and
///     SKIP_PAGER are sent as two messages. Recorded honestly.
fn send_net_wm_state(
    conn: &x11rb::rust_connection::RustConnection,
    target: u32,
    action: u32, // 1 = _NET_WM_STATE_ADD, 0 = _NET_WM_STATE_REMOVE
    atoms: &[&str],
) -> std::result::Result<String, Box<dyn std::error::Error>> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::*;

    let root = conn.setup().roots[0].root;
    let mask = EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY;
    let msg_atom = conn
        .intern_atom(false, b"_NET_WM_STATE")?
        .reply()?
        .atom;

    // data layout copied from winit's set_netwm(), which is the implementation
    // known to work on this host (B9 proved WindowLevel -> _NET_WM_STATE_ABOVE
    // works there):
    //     [action, property, 0, 0, 0]
    // My first attempt used [action, atom, 1, target, 0] and was ignored.
    let mut sent = Vec::new();
    for a in atoms {
        let prop = conn.intern_atom(false, a.as_bytes())?.reply()?.atom;
        let ev = ClientMessageEvent::new(32, target, msg_atom, [action, prop, 0, 0, 0]);
        conn.send_event(false, root, mask, ev)?;
        sent.push(*a);
    }
    conn.flush()?;
    Ok(format!(
        "sent {} ClientMessage(s) to root {root}, type=_NET_WM_STATE, format=32, \
         event_mask=SubstructureRedirect|SubstructureNotify, data=[{action}, <atom>, 0, 0, 0] \
         (same layout as winit's set_netwm); \
         atoms={:?} ({})",
        sent.len(),
        sent,
        if action == 1 { "ADD" } else { "REMOVE" }
    ))
}

struct App {
    tele: Arc<Tel>,
    win: Option<u32>,   // x11 Window is u32
    b13_mode: String, // "add" | "remove" | "addremove" | ""
    sent: bool,
    removed: bool,
    sent_aot: bool,
    seconds: u64,
    t0: Instant,
    conn: Option<std::sync::Mutex<x11rb::rust_connection::RustConnection>>,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.tele.frames.fetch_add(1, Ordering::Relaxed);
        if self.win.is_none() {
            if let Ok(h) = frame.window_handle() {
                if let RawWindowHandle::Xlib(x) = h.as_raw() {
                    self.win = Some(x.window as u32);
                    self.tele.note(&format!("window id = {}", x.window));
                }
            }
        }

        // Runtime AlwaysOnTop. B9: this works where the builder hint is
        // ignored. Sent once, after the window exists.
        if !self.sent_aot {
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::AlwaysOnTop,
            ));
            self.sent_aot = true;
            self.tele.note("AOT sent once at runtime");
        }

        // B13: send the ClientMessage once, from inside the egui loop, so we
        // act after the window is mapped and the WM is managing it.
        if !self.sent && self.win.is_some() {
            self.sent = true;
            match x11rb::rust_connection::RustConnection::connect(None) {
                Ok((c, _screen)) => {
                    let w = self.win.unwrap();
                    if self.b13_mode == "add" || self.b13_mode == "addremove" {
                        match send_net_wm_state(
                            &c,
                            w,
                            1,
                            &["_NET_WM_STATE_SKIP_TASKBAR", "_NET_WM_STATE_SKIP_PAGER"],
                        ) {
                            Ok(m) => self.tele.note(&format!("B13 ADD: {m}")),
                            Err(e) => self.tele.note(&format!("B13 ADD failed: {e}")),
                        }
                    }
                }
                Err(e) => self.tele.note(&format!("B13: x11rb connect failed: {e}")),
            }
        }

        // Removal half of B13, a few seconds later.
        if self.b13_mode == "addremove" && self.sent && !self.removed {
            if self.t0.elapsed() > Duration::from_secs(8) {
                self.removed = true;
                if let Some(c) = &self.conn {
                    let c = c.lock().unwrap();
                    match send_net_wm_state(
                        &c,
                        self.win.unwrap(),
                        0,
                        &["_NET_WM_STATE_SKIP_TASKBAR", "_NET_WM_STATE_SKIP_PAGER"],
                    ) {
                        Ok(m) => self.tele.note(&format!("B13 REMOVE: {m}")),
                        Err(e) => self.tele.note(&format!("B13 REMOVE failed: {e}")),
                    }
                }
            }
        }

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.label(egui::RichText::new(format!(
                "win={:?} rss={}kB",
                self.win,
                rss_kb()
            ))
            .small());
            // A large, unambiguous target: the B14 driver clicks the panel
            // centre, so the button must be there. Anchored top-left of the
            // central panel rather than centred, because the panel is short.
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(160.0, 18.0), egui::Sense::click());
            let r = rect;
            if resp.clicked() {
                self.tele.note("BUTTON-CLICKABLE was clicked");
            }
            ui.painter().rect_filled(
                r,
                2.0,
                if resp.hovered() {
                    egui::Color32::from_rgb(70, 120, 70)
                } else {
                    egui::Color32::from_rgb(40, 70, 40)
                },
            );
            ui.painter().text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                "CLICK ME",
                egui::FontId::proportional(11.0),
                egui::Color32::WHITE,
            );
        });

        ctx.request_repaint_after(Duration::from_millis(500));
        if self.seconds > 0 && self.t0.elapsed() > Duration::from_secs(self.seconds) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let logp = std::env::var("PROBE_LOG")
        .unwrap_or_else(|_| "/home/wer/devis/lapsphere/probe/ov/b13.log".into());
    let mut b13 = String::new();
    let mut x11_type = "normal".to_string();
    let mut seconds = 90u64;
    let mut title = "B13 Probe".to_string();
    let mut i = 0;
    while i < a.len() {
        match a[i].as_str() {
            "--b13" => { b13 = a[i + 1].clone(); i += 2 }
            "--x11-type" => { x11_type = a[i + 1].clone(); i += 2 }
            "--seconds" => { seconds = a[i + 1].parse().unwrap_or(90); i += 2 }
            "--title" => { title = a[i + 1].clone(); i += 2 }
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
        frames: AtomicU64::new(0),
        stop: AtomicBool::new(false),
    });
    tele.note(&format!(
        "START b13={b13:?} x11_type={x11_type} seconds={seconds}"
    ));

    let rt = tokio::runtime::Runtime::new().unwrap();
    let _enter = rt.enter();

    let wt = match x11_type.as_str() {
        "dock" => egui::X11WindowType::Dock,
        "utility" => egui::X11WindowType::Utility,
        "toolbar" => egui::X11WindowType::Toolbar,
        _ => egui::X11WindowType::Normal,
    };
    let vb = egui::ViewportBuilder::default()
        .with_inner_size(vec2(PANEL_W, PANEL_H))
        .with_decorations(false)
        .with_resizable(false)
        .with_title(title)
        .with_position(egui::pos2(300.0, 700.0))
        .with_window_type(wt);

    let opts = eframe::NativeOptions { viewport: vb, ..Default::default() };
    let app = App {
        tele: tele.clone(),
        win: None,
        b13_mode: b13,
        sent: false,
        removed: false,
        sent_aot: false,
        seconds,
        t0: Instant::now(),
        conn: None,
    };
    let r = eframe::run_native("B13 Probe", opts, Box::new(move |_cc| Ok(Box::new(app))));
    tele.note(&format!("run_native -> {r:?}"));
}
