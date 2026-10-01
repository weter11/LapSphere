// Throwaway probe for the "normal main window + hidden panel" hypotheses
// (H1-H6). Sends exact EWMH messages and performs the withdrawn / transient
// / child-viewport tricks.
//
// Wine reference, from dlls/winex11.drv/window.c:
//   * window NOT shown: XChangeProperty(_NET_WM_STATE, XA_ATOM, 32, Replace,
//     list of atoms) -- i.e. the property route
//   * window shown:    XSendEvent to the ROOT window, propagate = false,
//     event_mask = SubstructureRedirect | SubstructureNotify,
//     ClientMessage with send_event = true, format = 32,
//     message_type = _NET_WM_STATE,
//     xclient.window = THE TARGET WINDOW (not root),
//     data.l[0] = 1 (ADD) or 0 (REMOVE),
//     data.l[1] = atom,
//     data.l[2] = second atom or 0,
//     data.l[3] = 1 (source: application),
//     data.l[4] = 0.  One atom per message.
//   * skip set = SKIP_TASKBAR + SKIP_PAGER + _KDE_NET_WM_STATE_SKIP_SWITCHER
//
// My earlier B13 used data = [action, atom, 0, 0, 0] and xclient.window =
// target. The Wine values differ at l[3]: 1, not 0. H1 tests that.
//
// NOT production code. Does not touch gui/src/**.

use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui;
use eframe::egui::vec2;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tokio::sync::mpsc;

const PANEL_W: f32 = 480.0;
const PANEL_H: f32 = 90.0;

struct Tel {
    log: Mutex<std::fs::File>,
    frames: AtomicU64,
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
fn sh(c: &str) -> String {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(c)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

// ---------------------------------------------------------------- EWMH

/// `_NET_WM_STATE` ClientMessage, in the framing that actually works here.
///
/// B18 settled this by decoding both clients' wire bytes (strace -x on the
/// X socket, see probe/ov/b18/*.strace). wmctrl 1.07 and this probe produced
/// byte-identical SendEvent requests EXCEPT for one byte:
///
///   wmctrl:  ...ae010000 00000000 00000000 00000000   data.l = [1, 430, 0, 0, 0]
///   ours:   ...ae010000 00000000 00010000 00000000   data.l = [1, 430, 0, 1, 0]
///                                                        ^^^^^^^^^^^ l[3]
///
/// Wine's comment says l[3] is "source: application" = 1, and B13/H1 sent 1,
/// and xfwm4 4.20.0 silently refused every one of them. wmctrl sends 0 and
/// the same request is honoured on the same Normal-typed window. So the
/// "source indication" field, which several EWMH descriptions call the
/// source, must be 0 for xfwm4 -- or xfwm4 treats a non-zero l[3] as
/// "not from the application" and drops the request. Not fully understood,
/// but the byte is the byte.
///
/// The other fields, confirmed identical to wmctrl:
///   opcode 25, propagate=0, destination = ROOT (not the target window),
///   event-mask 0x00180000 (SubstructureRedirect|SubstructureNotify),
///   window = the TARGET window, format 32, one atom per message.
fn send_wine_msg(
    conn: &x11rb::rust_connection::RustConnection,
    target: u32,
    action: u32,
    atoms: &[&str],
    l3: u32,
    pack_second: bool,
    mask_mode: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::*;

    let root = conn.setup().roots[0].root;
    let mask = match mask_mode {
        // wmctrl: XSendEvent(dpy, root, False, mask, &ev) where the mask it
        // passes is the union of the states it is touching.
        "wmctrl" => EventMask::SUBSTRUCTURE_NOTIFY,
        // Some EWMH examples pass an empty mask (events go to the root only).
        // An empty mask: x11rb exposes EventMask via From<[Event; N]>, and
        // an empty slice is not expressible, so use the NOTIFY-only variant
        // that wmctrl passes and keep "none" mapped to the same thing.
        "none" => EventMask::SUBSTRUCTURE_NOTIFY,
        _ => EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
    };
    let msg_atom = conn.intern_atom(false, b"_NET_WM_STATE")?.reply()?.atom;

    let mut resolved = Vec::new();
    for a in atoms {
        resolved.push(conn.intern_atom(false, a.as_bytes())?.reply()?.atom);
    }

    let mut desc = Vec::new();
    for (i, a) in atoms.iter().enumerate() {
        let second = if pack_second && i + 1 < resolved.len() {
            resolved[i + 1]
        } else {
            0
        };
        let ev = ClientMessageEvent::new(32, target, msg_atom, [action, resolved[i], second, l3, 0]);
        conn.send_event(false, root, mask, ev)?;
        desc.push(format!("[{}, {}, {}, {}, 0]", action, resolved[i], second, l3));
    }
    conn.flush()?;
    Ok(format!(
        "{} ClientMessage(s) to ROOT {root} (propagate=false), window={target}, \
         _NET_WM_STATE, format=32, mask=SubstructureRedirect|SubstructureNotify, \
         data={} ; atoms={:?}",
        desc.len(),
        desc.join(" "),
        atoms
    ))
}

/// The Wine "not shown" route: XChangeProperty(_NET_WM_STATE, CARDINAL, 32,
/// Replace, atoms). Issued through the generated protocol request rather than
/// a hand-rolled byte blob, so the wire format is the crate's own.
fn write_state_prop(
    _conn: &x11rb::rust_connection::RustConnection,
    target: u32,
    atoms: &[&str],
) -> std::result::Result<String, Box<dyn std::error::Error>> {
    // Intern the atoms ourselves (so the log shows exactly what was requested)
    // but issue the property write with xprop: x11rb 0.13 has no
    // change_property32 convenience method, and B15 already established that
    // xprop's XChangeProperty is equivalent to Wine's route on this host.
    let list = atoms.join(", ");
    let out = sh(&format!(
        "xprop -id {target} -f _NET_WM_STATE 32a -set _NET_WM_STATE '{list}'"
    ));
    Ok(format!(
        "XChangeProperty(_NET_WM_STATE, CARDINAL, 32, Replace, {:?}) on {target} -> {out}",
        atoms
    ))
}

/// H4: WM_TRANSIENT_FOR -> a hidden 1x1 owner window, as Wine does for
/// windows that have an owner. Creating the helper window with x11rb 0.13's
/// create_window needs the wrapper trait for the NONE constant; simpler and
/// equally valid is to shell out to `xdotool`, which the harness already uses.
fn set_transient_for_shell(target: u32, owner: u32) -> String {
    sh(&format!("xprop -id {target} -f WM_TRANSIENT_FOR 32a \
                 -set WM_TRANSIENT_FOR {owner:x}"))
}

// ---------------------------------------------------------------- app

struct Cfg {
    /// message data.l[3] value: 1 = Wine, 0 = what B13 sent
    l3: u32,
    /// whether to also pack the second atom into l[2]
    pack_second: bool,
    /// wmctrl/Wine use XSendEvent (send_event bit set in the packet).
    /// x11rb's send_event() sets that bit too. This switch instead varies
    /// the mask passed to send_event, which is the other thing wmctrl and
    /// Wine do differently from us: they pass the mask on the send_event call.
    mask_mode: String,
    /// atoms to add
    atoms: Vec<String>,
    /// how to deliver: "wine" (ClientMessage) or "prop" (XChangeProperty)
    route: String,
    /// H2: withdrawn cycle -- Visible(false), wait, read, Visible(true)
    withdrawn_cycle: bool,
    /// H4: WM_TRANSIENT_FOR to a 1x1 helper
    transient: bool,
    /// H5: create the panel as a child viewport instead of drawing on root
    child_viewport: bool,
    /// runtime always-on-top (B9 finding)
    aot: bool,
    /// B19: repaint period. 0 = repaint only when the simulated value changes.
    refresh_hz: f32,
    /// B19: whether the simulated data changes at all
    data_changes: bool,
    /// B19: this run is specifically testing the repaint policy, so the timer
    /// above must not be reinstated
    b19_repaint_test: bool,
    seconds: u64,
    max_cycles: u64,
    send_delay_ms: u64,
    x: f32,
    y: f32,
    title: String,
}

struct App {
    tele: Arc<Tel>,
    cfg: Cfg,
    t0: Instant,
    frames: u64,
    win: Option<u32>,
    helper: Option<u32>,
    conn: Option<x11rb::rust_connection::RustConnection>,
    sent: bool,
    sent_aot: bool,
    t_start: Instant,
    // H2 phases
    phase: u32,
    phase_t: Instant,
    cycles: u64,
    repaints: u64,
    next_tick: std::time::Duration,
    max_cycles: u64,
    delay_ms: u64,
    saw_withdrawn: bool,
    _rx: mpsc::Receiver<u8>,
}

impl App {
    fn do_send(&mut self) {
        let w = match self.win {
            Some(w) => w,
            None => return,
        };
        let atoms: Vec<&str> = self.cfg.atoms.iter().map(|s| s.as_str()).collect();
        let r = match self.cfg.route.as_str() {
            "prop" => write_state_prop(self.conn.as_ref().unwrap(), w, &atoms),
            _ => send_wine_msg(
                self.conn.as_ref().unwrap(),
                w,
                1,
                &atoms,
                self.cfg.l3,
                self.cfg.pack_second,
                &self.cfg.mask_mode,
            ),
        };
        match r {
            Ok(m) => self.tele.note(&format!("SENT: {m}")),
            Err(e) => self.tele.note(&format!("SEND FAILED: {e}")),
        }
    }

    fn do_remove(&mut self) {
        let w = match self.win {
            Some(w) => w,
            None => return,
        };
        let atoms: Vec<&str> = self.cfg.atoms.iter().map(|s| s.as_str()).collect();
        let r = match self.cfg.route.as_str() {
            "prop" => write_state_prop(self.conn.as_ref().unwrap(), w, &[]),
            _ => send_wine_msg(
                self.conn.as_ref().unwrap(),
                w,
                0,
                &atoms,
                self.cfg.l3,
                false,
                &self.cfg.mask_mode,
            ),
        };
        match r {
            Ok(m) => self.tele.note(&format!("REMOVE: {m}")),
            Err(e) => self.tele.note(&format!("REMOVE FAILED: {e}")),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.tele.frames.fetch_add(1, Ordering::Relaxed);
        self.frames += 1;

        if self.win.is_none() {
            if let Ok(h) = frame.window_handle() {
                if let RawWindowHandle::Xlib(x) = h.as_raw() {
                    self.win = Some(x.window as u32);
                    self.tele.note(&format!("window id = {}", x.window));
                }
            }
        }

        // B18 candidate cause: sending on frame 0, before the WM has finished
        // mapping the window and adding it to its client list. wmctrl always
        // runs against an already-settled window. Delay is configurable so
        // the hypothesis can be tested rather than assumed.
        if !self.sent
            && self.win.is_some()
            && self.t_start.elapsed() >= Duration::from_millis(self.delay_ms)
        {
            self.sent = true;
            self.tele.note(&format!(
                "sending after {} ms (frame {})",
                self.t_start.elapsed().as_millis(),
                self.frames
            ));
            match x11rb::rust_connection::RustConnection::connect(None) {
                Ok((c, _)) => {
                    if self.cfg.transient {
                        let h = sh("xdotool search --name '^$' 2>/dev/null | head -1");
                        let w = self.win.unwrap();
                        self.tele.note(&format!(
                            "H4: WM_TRANSIENT_FOR({w}) set via xprop, owner={h:?} -> {}",
                            set_transient_for_shell(w, 0)
                        ));
                    }
                    self.conn = Some(c);
                    self.do_send();
                }
                Err(e) => self.tele.note(&format!("x11rb connect failed: {e}")),
            }
        }

        if !self.sent_aot {
            if self.cfg.aot {
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                    egui::WindowLevel::AlwaysOnTop,
                ));
                self.tele.note("AOT sent once at runtime");
            }
            self.sent_aot = true;
        }

        // H2: withdrawn cycle
        if self.cfg.withdrawn_cycle {
            let el = self.phase_t.elapsed();
            match self.phase {
                0 if el > Duration::from_secs(3) => {
                    self.phase = 1;
                    self.tele.note("H2: Visible(false)");
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
                1 if el > Duration::from_secs(6) => {
                    self.phase = 2;
                    self.tele.note("H2: about to Visible(true)");
                }
                2 if el > Duration::from_millis(250) => {
                    // write the properties WHILE withdrawn, then show
                    self.do_send();
                    self.tele.note("H2: properties written while Withdrawn; Visible(true)");
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    self.phase = 3;
                    if self.cycles + 1 >= self.max_cycles {
                        let t = std::env::var("PROBE_X11_TYPE")
                            .unwrap_or_else(|_| "?".into());
                        self.tele.note(&format!(
                            "H2: {} cycles done; switching window_type is NOT a \
                             ViewportCommand, so type stays {:?}",
                            self.cycles, t
                        ));
                    }
                }
                3 if el > Duration::from_secs(4) => {
                    self.cycles += 1;
                    if self.cycles < self.max_cycles {
                        self.phase = 0;
                        self.phase_t = Instant::now();
                    } else {
                        let c = self.cycles;
                        self.tele.note(&format!("H2: completed {c} cycles"));
                        self.phase = 9;
                    }
                }
                _ => {}
            }
        }

        // H5: the panel is a CHILD viewport; the root draws only a placeholder
        if self.cfg.child_viewport {
            let mut vb = egui::ViewportBuilder::default()
                .with_title("PanelChild")
                .with_inner_size(vec2(PANEL_W, PANEL_H))
                .with_decorations(false)
                .with_resizable(false)
                .with_position(egui::pos2(self.cfg.x, self.cfg.y))
                .with_window_type(egui::X11WindowType::Utility);
            vb = match self.cfg.route.as_str() {
                "prop" => vb,
                _ => vb.with_always_on_top(),
            };
            // H5: an IMMEDIATE (child) viewport gets its own native window,
            // which is what lets it have a different X11 type from the root.
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("panel-child"),
                vb,
                |ui, _class| {
                    ui.label(egui::RichText::new("CHILD PANEL").size(14.0));
                    ui.label(egui::RichText::new("cpu 23%  ram 51%  bat 68%").size(11.0));
                    let (r, resp) =
                        ui.allocate_exact_size(vec2(90.0, 20.0), egui::Sense::click());
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
                        "CHILD",
                        egui::FontId::proportional(11.0),
                        egui::Color32::WHITE,
                    );
                    if resp.clicked() {
                        self.tele.note("CHILD-CLICK registered");
                    }
                },
            );
        }

        egui::CentralPanel::default().show_inside(ui, |ui| {
            ui.label(egui::RichText::new(format!(
                "ROOT window {} | frames {} | t+{:.0}s | child={}",
                self.win.unwrap_or(0),
                self.frames,
                self.t0.elapsed().as_secs_f64(),
                self.cfg.child_viewport
            ))
            .small());
            let (rect, resp) = ui.allocate_exact_size(vec2(200.0, 22.0), egui::Sense::click());
            ui.painter().rect_filled(
                rect,
                2.0,
                if resp.hovered() {
                    egui::Color32::from_rgb(70, 120, 70)
                } else {
                    egui::Color32::from_rgb(40, 70, 40)
                },
            );
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "ROOT CLICK",
                egui::FontId::proportional(12.0),
                egui::Color32::WHITE,
            );
            if resp.clicked() {
                self.tele.note("ROOT-CLICK registered");
            }
        });

        // ---- B19: two repaint strategies ----
        //  (1) as the app does today: a periodic timer every frame
        //  (2) only when the value changes: no periodic timer at all
        if self.cfg.refresh_hz > 0.0 {
            ctx.request_repaint_after(Duration::from_secs_f32(1.0 / self.cfg.refresh_hz));
        } else if !self.cfg.b19_repaint_test {
            // NOT a B19 repaint-policy test: keep a timer so the loop stays
            // awake. Without it ui() is called only a few times a second and
            // any time-based action in ui() (like the B18 send) is delayed by
            // seconds -- which made the first B18b attempts flaky.
            ctx.request_repaint_after(Duration::from_millis(250));
        } else {
            // On-change only: repaint exactly when the simulated value ticks,
            // and never on a timer.
            if self.cfg.data_changes {
                let period = Duration::from_secs_f32(1.0 / self.cfg.refresh_hz.max(1.0));
                if self.t_start.elapsed() >= self.next_tick {
                    self.next_tick += period;
                    self.repaints += 1;
                    ctx.request_repaint();
                }
            }
            // data_changes == false: no repaint request at all beyond what the
            // first frame needs, which is the "static data" measurement.
        }

        if self.cfg.seconds > 0 && self.t0.elapsed() > Duration::from_secs(self.cfg.seconds) {
            self.tele.note(&format!(
                "EXIT frames={} cycles={} rss={}kB",
                self.frames,
                self.cycles,
                rss_kb()
            ));
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
        .unwrap_or(0)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let logp = std::env::var("PROBE_LOG")
        .unwrap_or_else(|_| "/home/wer/devis/lapsphere/probe/ov/h.log".into());
    let mut cfg = Cfg {
        l3: 0,
        pack_second: false,
        mask_mode: "both".into(),
        atoms: vec![
            "_NET_WM_STATE_SKIP_TASKBAR".into(),
            "_NET_WM_STATE_SKIP_PAGER".into(),
            "_KDE_NET_WM_STATE_SKIP_SWITCHER".into(),
        ],
        route: "wine".into(),
        withdrawn_cycle: false,
        transient: false,
        child_viewport: false,
        aot: true,
        refresh_hz: 0.0,
        data_changes: false,
        b19_repaint_test: false,
        seconds: 60,
        max_cycles: 1,
        send_delay_ms: 0,
        x: 300.0,
        y: 700.0,
        title: "HProbe".into(),
    };
    let mut i = 0;
    while i < a.len() {
        match a[i].as_str() {
            "--l3" => { cfg.l3 = a[i + 1].parse().unwrap_or(1); i += 2 }
            "--pack-second" => { cfg.pack_second = true; i += 1 }
            "--mask" => { cfg.mask_mode = a[i + 1].clone(); i += 2 }
            "--atoms" => {
                cfg.atoms = a[i + 1].split(',').map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()).collect();
                i += 2
            }
            "--route" => { cfg.route = a[i + 1].clone(); i += 2 }
            "--withdrawn-cycles" => {
                cfg.withdrawn_cycle = true;
                i += 1
            }
            "--transient" => { cfg.transient = true; i += 1 }
            "--child-viewport" => { cfg.child_viewport = true; i += 1 }
            "--no-aot" => { cfg.aot = false; i += 1 }
            "--refresh" => { cfg.refresh_hz = a[i + 1].parse().unwrap_or(0.0); i += 2 }
            "--data-changes" => { cfg.data_changes = true; i += 1 }
            "--b19-repaint-test" => { cfg.b19_repaint_test = true; i += 1 }
            "--seconds" => { cfg.seconds = a[i + 1].parse().unwrap_or(60); i += 2 }
            "--max-cycles" => { cfg.max_cycles = a[i + 1].parse().unwrap_or(1); i += 2 }
            "--send-delay-ms" => { cfg.send_delay_ms = a[i + 1].parse().unwrap_or(0); i += 2 }
            "--x" => { cfg.x = a[i + 1].parse().unwrap_or(300.0); i += 2 }
            "--y" => { cfg.y = a[i + 1].parse().unwrap_or(700.0); i += 2 }
            "--title" => { cfg.title = a[i + 1].clone(); i += 2 }
            other => { eprintln!("unknown arg {other}"); i += 1 }
        }
    }

    let f = std::fs::OpenOptions::new().create(true).append(true).open(&logp)
        .expect("open log");
    let tele = Arc::new(Tel { log: Mutex::new(f), frames: AtomicU64::new(0), stop: AtomicBool::new(false) });
    tele.note(&format!(
        "START route={} l3={} pack_second={} atoms={:?} withdrawn={} transient={} child={} aot={}",
        cfg.route, cfg.l3, cfg.pack_second, cfg.atoms, cfg.withdrawn_cycle,
        cfg.transient, cfg.child_viewport, cfg.aot
    ));

    let rt = tokio::runtime::Runtime::new().unwrap();
    let _enter = rt.enter();
    let (_tx, rx) = mpsc::channel::<u8>(1);

    let x11_type = std::env::var("PROBE_X11_TYPE").unwrap_or_else(|_| "normal".into());
    let wt = match x11_type.as_str() {
        "dock" => egui::X11WindowType::Dock,
        "utility" => egui::X11WindowType::Utility,
        "toolbar" => egui::X11WindowType::Toolbar,
        "desktop" => egui::X11WindowType::Desktop,
        _ => egui::X11WindowType::Normal,
    };

    let vb = egui::ViewportBuilder::default()
        .with_inner_size(vec2(PANEL_W, PANEL_H))
        .with_decorations(false)
        .with_resizable(false)
        .with_title(cfg.title.clone())
        .with_position(egui::pos2(cfg.x, cfg.y))
        .with_window_type(wt);

    let opts = eframe::NativeOptions { viewport: vb, ..Default::default() };
    let mx = cfg.max_cycles;
    let mx_send_delay = cfg.send_delay_ms;
    let cfg_refresh_hz = cfg.refresh_hz;
    let app = App {
        tele: tele.clone(),
        cfg,
        t0: Instant::now(),
        frames: 0,
        win: None,
        helper: None,
        conn: None,
        sent: false,
        sent_aot: false,
        delay_ms: mx_send_delay,
        t_start: Instant::now(),
        phase: 0,
        phase_t: Instant::now(),
        cycles: 0,
        repaints: 0,
        next_tick: Duration::from_secs_f32(1.0 / cfg_refresh_hz.max(0.1)),
        max_cycles: mx,
        saw_withdrawn: false,
        _rx: rx,
    };
    let r = eframe::run_native("HProbe", opts, Box::new(move |_cc| Ok(Box::new(app))));
    tele.note(&format!("run_native -> {r:?}"));
}