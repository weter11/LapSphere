// Throwaway demo for the mini-panel overlay experiments (docs B1-B8).
//
// Renders a gkrellm-style panel strip in a real, undecorated eframe window
// on top of another application's window, with the A3 config knobs applied
// live: item order, global font scale, per-item font scale, per-item label
// override, always-on-top, click-through (MousePassthrough), transparency
// and skip-taskbar.
//
// Data can be live (from lapsphere-daemon over the system bus) or synthetic,
// selected with --data live|synth. The panel NEVER sends any D-Bus call
// other than the read-only Get* getters, so it cannot move a discrete GPU
// out of runtime suspend (acceptance criterion A2.5).
//
// NOT production code. Does not touch gui/src/**.
//
//   cargo run --example panel_overlay_demo -- [options]
//
// Options:
//   --data live|synth        data source (default synth)
//   --scale <f32>            global font scale (default 1.0)
//   --refresh <hz>           panel repaint rate (default 1)
//   --items a,b,c            comma-separated item ids in this order
//   --label id=text          override one item's label (repeatable)
//   --item-scale id=f        per-item font scale (repeatable)
//   --always-on-top          WindowLevel(AlwaysOnTop)
//   --click-through          ViewportCommand::MousePassthrough(true)
//   --transparent            transparent background
//   --skip-taskbar           with_taskbar(false)
//   --x <f32> --y <f32>      initial outer position
//   --seconds <n>            auto-exit after n seconds (0 = run forever)
//   --log <path>             telemetry log path

use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui;
use eframe::egui::{pos2, vec2, Context, Ui};
use tokio::sync::mpsc;

// ---------------------------------------------------------------- item table
//
// Every id here is checked against common/src/types.rs and the live D-Bus
// interface. The `unit` strings preserve case exactly as they appear in the
// source data (MHz, °C, GiB, dBm, mA, W, V). Per A3 acceptance item 3 these
// live in ONE place; widgets below never inline a unit string.

/// A panel item: where its data comes from and how it is rendered.
struct ItemDef {
    id: &'static str,
    /// D-Bus getter + `common::types` field path. Shown for documentation and
    /// for `--dump-items`; not used for dispatch (the match below is).
    source: &'static str,
    /// Fixed slot width in points at font scale 1.0. Per ADR-2 the window
    /// size is computed from these, never from data availability.
    width: f32,
}

const ITEMS: &[ItemDef] = &[
    ItemDef { id: "hostname",        source: "GetSystemInfo.product_name",                    width: 150.0 },
    ItemDef { id: "cpu_load_percent",source: "GetCpuInfo.average_load",                      width:  62.0 },
    ItemDef { id: "cpu_core_chart",  source: "GetCpuInfo.cores[].load",                      width:  92.0 },
    ItemDef { id: "cpu_freq",        source: "GetCpuInfo.average_frequency",                  width:  74.0 },
    ItemDef { id: "cpu_temp",        source: "GetCpuInfo.package_temp",                       width:  62.0 },
    ItemDef { id: "cpu_power",       source: "GetCpuInfo.package_power",                      width:  64.0 },
    ItemDef { id: "gpu_load",        source: "GetGpuInfo[].load",                             width:  62.0 },
    ItemDef { id: "gpu_temp",        source: "GetGpuInfo[].temperature",                      width:  62.0 },
    ItemDef { id: "gpu_clock",       source: "GetGpuInfo[].frequency",                        width:  74.0 },
    ItemDef { id: "gpu_power",       source: "GetGpuInfo[].power",                            width:  64.0 },
    ItemDef { id: "memory",          source: "GetMemoryInfo.used_percent / used_gib",         width:  84.0 },
    ItemDef { id: "battery",         source: "GetBatteryInfo.charge_percent",                 width:  62.0 },
    ItemDef { id: "wifi_signal",     source: "GetWifiInfo[].signal_level",                    width:  70.0 },
    ItemDef { id: "fans",            source: "GetFanInfo[].rpm_or_percent",                   width:  62.0 },
    ItemDef { id: "gamepad_1",       source: "GetGamepadInfo[0].name / battery_level",        width: 120.0 },
    ItemDef { id: "gamepad_2",       source: "GetGamepadInfo[1].name / battery_level",        width: 120.0 },
];

/// Units, in one place, case preserved. A2.3.
mod unit {
    pub const MHZ: &str = "MHz";
    pub const CELSIUS: &str = "°C";
    pub const PERCENT: &str = "%";
    pub const GIB: &str = "GiB";
    pub const DBM: &str = "dBm";
    pub const WATT: &str = "W";
    pub const MILLIAMP: &str = "mA";
    pub const VOLT: &str = "V";
    pub const GRAPH: &str = "";
}

// ---------------------------------------------------------------- data

#[derive(Clone, Default)]
struct Snap {
    hostname: Option<String>,
    cpu_load: Option<f32>,
    cores: Vec<f32>,
    cpu_freq_mhz: Option<u64>,
    cpu_temp: Option<f32>,
    cpu_power: Option<f32>,
    gpu_load: Option<f32>,
    gpu_temp: Option<f32>,
    gpu_clock: Option<u64>,
    gpu_power: Option<f32>,
    /// True when the panel read a *discrete* GPU at all.
    gpu_is_discrete: Option<bool>,
    mem_percent: Option<f32>,
    mem_used: Option<f64>,
    battery: Option<u64>,
    wifi_dbm: Option<i32>,
    fan: Option<(u32, bool)>, // (value, is_rpm)
    pads: Vec<(String, Option<u8>, String)>, // name, battery_level, status
}

struct Tel {
    frames: AtomicU64,
    repaints: AtomicU64,
    stop: AtomicBool,
    log: Mutex<std::fs::File>,
    cpu_ms: Mutex<f64>,
}
impl Tel {
    fn note(&self, msg: &str) {
        let mut f = self.log.lock().unwrap();
        let _ = writeln!(f, "[{}] {}", ts(), msg);
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
fn cpu_ticks() -> Option<(u64, u64)> {
    let s = std::fs::read_to_string("/proc/self/stat").ok()?;
    let f = s.rsplit_once(')').map(|x| x.1)?;
    let v: Vec<u64> = f
        .split_whitespace()
        .map(|x| x.parse::<u64>().unwrap_or(0))
        .collect();
    // after "comm)" the fields start at index 0 == state(3); utime=14 stime=15
    Some((v[11] + v[12], v[13] + v[14]))
}

/// Live producer: read-only Get* calls against the running daemon.
fn spawn_live(rx_out: mpsc::Sender<Snap>) {
    tokio::spawn(async move {
        let conn = match zbus::Connection::system().await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("panel_overlay_demo: system bus unavailable: {e}");
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
                eprintln!("panel_overlay_demo: proxy unavailable: {e}");
                return;
            }
        };
        // free fn, not a closure returning an async block: the closure form
        // ties the future to the borrow of `p`.
        async fn get(p: &zbus::Proxy<'_>, m: &'static str) -> Option<String> {
            let r: zbus::Result<String> = p.call(m, &()).await;
            r.ok().map(|s| {
                let b: Vec<u8> = s.bytes().map(|c| c as u8).collect();
                String::from_utf8_lossy(&b).to_string()
            })
        }
        let mut first = true;
        loop {
            let t0 = Instant::now();
            let mut s = Snap::default();
            if let Some(v) = get(&proxy, "GetSystemInfo").await {
                if let Ok(j) = serde_json::from_str::<serde_json::Value>(&v) {
                    s.hostname = j.get("product_name").and_then(|x| x.as_str()).map(str::to_string);
                }
            }
            if let Some(v) = get(&proxy, "GetCpuInfo").await {
                if let Ok(j) = serde_json::from_str::<serde_json::Value>(&v) {
                    s.cpu_load = j.get("average_load").and_then(|x| x.as_f64()).map(|x| x as f32);
                    s.cpu_freq_mhz = j.get("average_frequency").and_then(|x| x.as_u64());
                    s.cpu_temp = j.get("package_temp").and_then(|x| x.as_f64()).map(|x| x as f32);
                    s.cpu_power = j.get("package_power").and_then(|x| x.as_f64()).map(|x| x as f32);
                    s.cores = j
                        .get("cores")
                        .and_then(|x| x.as_array())
                        .map(|a| a.iter().map(|c| c.get("load").and_then(|l| l.as_f64()).unwrap_or(0.0) as f32).collect())
                        .unwrap_or_default();
                }
            }
            if let Some(v) = get(&proxy, "GetGpuInfo").await {
                if let Ok(j) = serde_json::from_str::<serde_json::Value>(&v) {
                    if let Some(a) = j.as_array().filter(|a| !a.is_empty()) {
                        // Prefer the discrete GPU for the panel, but only
                        // among entries the daemon already listed. A2.5.
                        let pick = a
                            .iter()
                            .find(|g| g.get("gpu_type").and_then(|x| x.as_str()) == Some("Discrete"))
                            .unwrap_or(&a[0]);
                        s.gpu_is_discrete =
                            Some(pick.get("gpu_type").and_then(|x| x.as_str()) == Some("Discrete"));
                        s.gpu_load = pick.get("load").and_then(|x| x.as_f64()).map(|x| x as f32);
                        s.gpu_temp = pick.get("temperature").and_then(|x| x.as_f64()).map(|x| x as f32);
                        s.gpu_clock = pick.get("frequency").and_then(|x| x.as_u64());
                        s.gpu_power = pick.get("power").and_then(|x| x.as_f64()).map(|x| x as f32);
                    }
                }
            }
            if let Some(v) = get(&proxy, "GetMemoryInfo").await {
                if let Ok(j) = serde_json::from_str::<serde_json::Value>(&v) {
                    s.mem_percent = j.get("used_percent").and_then(|x| x.as_f64()).map(|x| x as f32);
                    s.mem_used = j.get("used_gib").and_then(|x| x.as_f64());
                }
            }
            if let Some(v) = get(&proxy, "GetBatteryInfo").await {
                if let Ok(j) = serde_json::from_str::<serde_json::Value>(&v) {
                    s.battery = j.get("charge_percent").and_then(|x| x.as_u64());
                }
            }
            if let Some(v) = get(&proxy, "GetWifiInfo").await {
                if let Ok(j) = serde_json::from_str::<serde_json::Value>(&v) {
                    s.wifi_dbm = j
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(|x| x.get("signal_level"))
                        .and_then(|x| x.as_i64())
                        .map(|x| x as i32);
                }
            }
            if let Some(v) = get(&proxy, "GetFanInfo").await {
                if let Ok(j) = serde_json::from_str::<serde_json::Value>(&v) {
                    s.fan = j.as_array().and_then(|a| a.first()).map(|f| {
                        (
                            f.get("rpm_or_percent").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
                            f.get("is_rpm").and_then(|x| x.as_bool()).unwrap_or(false),
                        )
                    });
                }
            }
            if let Some(v) = get(&proxy, "GetGamepadInfo").await {
                if let Ok(j) = serde_json::from_str::<serde_json::Value>(&v) {
                    s.pads = j
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .map(|g| {
                                    (
                                        g.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                                        g.get("battery_level").and_then(|x| x.as_u64()).map(|x| x as u8),
                                        g.get("status").and_then(|x| x.as_str()).unwrap_or("?").to_string(),
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                }
            }
            if first {
                println!("panel_overlay_demo: first live snapshot: cpu_load={:?} gpu_load={:?} pads={} hostname={:?}",
                    s.cpu_load, s.gpu_load, s.pads.len(), s.hostname);
                first = false;
            }
            if rx_out.send(s).await.is_err() {
                return;
            }
            let el = t0.elapsed();
            if el < Duration::from_millis(900) {
                tokio::time::sleep(Duration::from_millis(900) - el).await;
            }
        }
    });
}

/// Synthetic producer: same shape, no D-Bus at all.
fn spawn_synth(rx_out: mpsc::Sender<Snap>, t0: Instant) {
    tokio::spawn(async move {
        let mut n = 0u32;
        loop {
            n += 1;
            let t = t0.elapsed().as_secs_f32();
            let s = Snap {
                hostname: Some("XPS-15-9520".into()),
                cpu_load: Some(18.0 + 22.0 * ((t / 7.0).sin() * 0.5 + 0.5)),
                cores: (0..8)
                    .map(|i| 10.0 + 30.0 * (((t / (3.0 + i as f32)) + i as f32).sin() * 0.5 + 0.5))
                    .collect(),
                cpu_freq_mhz: Some(2_600 + (t as u64 % 400)),
                cpu_temp: Some(47.0 + 3.0 * (t / 11.0).sin()),
                cpu_power: Some(12.5 + 4.0 * (t / 5.0).sin()),
                // One in five snapshots has the dGPU absent, to exercise the
                // reserved-width "—" path of A2.4 live.
                gpu_load: if n % 5 == 0 { None } else { Some(24.0 + 30.0 * ((t / 4.0).cos() * 0.5 + 0.5)) },
                gpu_temp: if n % 5 == 0 { None } else { Some(52.0 + 6.0 * (t / 13.0).sin()) },
                gpu_clock: if n % 5 == 0 { None } else { Some(1_260 + (t as u64 % 300)) },
                gpu_power: if n % 5 == 0 { None } else { Some(38.0 + 12.0 * (t / 6.0).sin()) },
                gpu_is_discrete: if n % 5 == 0 { None } else { Some(true) },
                mem_percent: Some(51.7 + 2.0 * (t / 30.0).sin()),
                mem_used: Some(7.68f64 + 0.1 * (t / 30.0).sin() as f64),
                battery: Some(68 - (t as u64 / 60) % 5),
                wifi_dbm: if n % 7 == 0 { None } else { Some(-52 + (t as i32 % 9)) },
                fan: Some((2_200 + (t as u32 % 400), true)),
                pads: vec![("Xbox Wireless Controller".into(), Some(72), "Connected".into())],
            };
            if rx_out.send(s).await.is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(900)).await;
        }
    });
}

// ---------------------------------------------------------------- app

struct Cfg {
    data: String,
    scale: f32,
    refresh_hz: f32,
    items: Vec<String>,
    labels: HashMap<String, String>,
    item_scales: HashMap<String, f32>,
    always_on_top: bool,
    click_through: bool,
    transparent: bool,
    skip_taskbar: bool,
    pos: Option<(f32, f32)>,
    seconds: u64,
}

struct App {
    tele: Arc<Tel>,
    rx: mpsc::Receiver<Snap>,
    snap: Snap,
    cfg: Cfg,
    t0: Instant,
    frames: u64,
    layout_seen: Option<(usize, String)>,
    last_layout_change: Instant,
    cpu_t0: Option<(u64, u64, Instant)>,
}

impl App {
    /// Per ADR-2 + A2.2: the window size is a function of the *selected items
    /// and their fonts* only. No data lookup happens here, so an absent
    /// reading cannot change the geometry.
    fn panel_size(&self) -> egui::Vec2 {
        let pad = 6.0;
        let mut w = pad;
        for id in &self.cfg.items {
            let def = ITEMS.iter().find(|d| d.id == *id);
            let s = self
                .cfg
                .item_scales
                .get(id)
                .copied()
                .unwrap_or(1.0)
                * self.cfg.scale;
            w += def.map(|d| d.width * s).unwrap_or(80.0) + pad;
        }
        egui::vec2(w, 34.0 * self.cfg.scale + 2.0 * pad)
    }

    fn apply_size(&self, ctx: &Context) {
        let sz = self.panel_size();
        let _ = ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(sz));
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.tele.frames.fetch_add(1, Ordering::Relaxed);
        self.frames += 1;

        while let Ok(s) = self.rx.try_recv() {
            self.snap = s;
        }

        // Keep the window in sync with the config (font scale / item list).
        let sig = format!("{:?}|{}", self.cfg.items, self.cfg.scale);
        if self.layout_seen.as_ref().map(|s| s.0) != Some(self.frames as usize)
            && self.layout_seen.as_ref().map(|s| &s.1) != Some(&sig)
        {
            self.layout_seen = Some((self.frames as usize, sig.clone()));
            self.last_layout_change = Instant::now();
            self.apply_size(&ctx);
            self.tele.note(&format!(
                "layout change -> target inner size {:.0}x{:.0} (items={}, scale={})",
                self.panel_size().x,
                self.panel_size().y,
                self.cfg.items.len(),
                self.cfg.scale
            ));
        }

        // Periodic report so the owner can see fps and the size stayed put.
        if self.frames % (self.cfg.refresh_hz.max(1.0) as u64 * 20) == 0 {
            let vp = ctx.input(|i| i.viewport().clone());
            self.tele.note(&format!(
                "t+{:.0}s frames={} rss={}kB inner={:?} outer={:?} focused={:?} occluded={:?}",
                self.t0.elapsed().as_secs_f64(),
                self.frames,
                rss_kb(),
                vp.inner_rect.map(|r| format!("{:.0}x{:.0}", r.width(), r.height())),
                vp.outer_rect.map(|r| format!("{:.0}x{:.0}", r.width(), r.height())),
                vp.focused,
                vp.occluded
            ));
        }

        // ---- draw the panel strip ----
        let (bg, fg) = if self.cfg.transparent {
            (egui::Color32::TRANSPARENT, egui::Color32::LIGHT_GRAY)
        } else {
            (egui::Color32::from_rgb(18, 18, 20), egui::Color32::from_rgb(230, 230, 235))
        };

        egui::Frame::none()
            .fill(bg)
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for id in self.cfg.items.clone() {
                        let sc = self.cfg.item_scales.get(&id).copied().unwrap_or(1.0) * self.cfg.scale;
                        let def_w = ITEMS
                            .iter()
                            .find(|d| d.id == id)
                            .map(|d| d.width * sc)
                            .unwrap_or(80.0 * sc);
                        let label = self
                            .cfg
                            .labels
                            .get(&id)
                            .cloned()
                            .unwrap_or_else(|| default_label(&id));
                        let text = render_item(&id, &self.snap, &label);
                        // Fixed-width slot: allocated from the config, never
                        // from the string that came out of the data.
                        ui.vertical(|ui| {
                            ui.set_width(def_w);
                            ui.label(
                                egui::RichText::new(label)
                                    .size(9.0 * sc)
                                    .color(fg.gamma_multiply(0.7)),
                            );
                            ui.add(
                                egui::Label::new(egui::RichText::new(text).size(12.0 * sc).color(fg))
                                    .truncate(),
                            );
                        });
                    }
                });
            });

        // B7: no-flicker evidence. Report when the size has been stable.
        if self.last_layout_change.elapsed() > Duration::from_secs(2) {
            self.tele.repaints.fetch_add(1, Ordering::Relaxed);
        }

        ctx.request_repaint_after(Duration::from_secs_f32(1.0 / self.cfg.refresh_hz.max(0.2)));

        if self.cfg.seconds > 0 && self.t0.elapsed() > Duration::from_secs(self.cfg.seconds) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn default_label(id: &str) -> String {
    match id {
        "hostname" => "HOST".into(),
        "cpu_load_percent" => "CPU".into(),
        "cpu_core_chart" => "CORES".into(),
        "cpu_freq" => "FREQ".into(),
        "cpu_temp" => "TEMP".into(),
        "cpu_power" => "PWR".into(),
        "gpu_load" => "GPU".into(),
        "gpu_temp" => "GTEMP".into(),
        "gpu_clock" => "GCLK".into(),
        "gpu_power" => "GPWR".into(),
        "memory" => "RAM".into(),
        "battery" => "BAT".into(),
        "wifi_signal" => "WIFI".into(),
        "fans" => "FAN".into(),
        "gamepad_1" => "PAD1".into(),
        "gamepad_2" => "PAD2".into(),
        _ => id.into(),
    }
}

/// A2.4: an absent reading renders as "—" inside a slot whose width was
/// already reserved. `Option::None` never widens or narrows anything.
fn render_item(id: &str, s: &Snap, _label: &str) -> String {
    let dash = "—";
    let f1 = |o: Option<f32>| match o {
        Some(v) => format!("{v:.0}{}", unit::PERCENT),
        None => dash.into(),
    };
    match id {
        "hostname" => s.hostname.clone().unwrap_or_else(|| dash.into()),
        "cpu_load_percent" => f1(s.cpu_load),
        "cpu_core_chart" => {
            // ONE fixed-height strip, not one bar per core (ADR-2). The core
            // count may change under SMT without changing the window.
            let n = s.cores.len().max(1);
            let sp: String = s
                .cores
                .iter()
                .take(16)
                .map(|c| {
                    let levels = b" .:-=+*#%@";
                    let i = ((*c).clamp(0.0, 100.0) / 100.0 * 9.0) as usize;
                    levels[i] as char
                })
                .collect();
            if sp.is_empty() { dash.into() } else { format!("{sp} {n}c") }
        }
        "cpu_freq" => match s.cpu_freq_mhz {
            Some(v) => format!("{} {}", v / 1000, unit::MHZ),
            None => dash.into(),
        },
        "cpu_temp" => match s.cpu_temp {
            Some(v) => format!("{v:.0}{}", unit::CELSIUS),
            None => dash.into(),
        },
        "cpu_power" => match s.cpu_power {
            Some(v) => format!("{v:.1}{}", unit::WATT),
            None => dash.into(),
        },
        "gpu_load" => f1(s.gpu_load),
        "gpu_temp" => match s.gpu_temp {
            Some(v) => format!("{v:.0}{}", unit::CELSIUS),
            None => dash.into(),
        },
        "gpu_clock" => match s.gpu_clock {
            Some(v) => format!("{} {}", v / 1000, unit::MHZ),
            None => dash.into(),
        },
        "gpu_power" => match s.gpu_power {
            Some(v) => format!("{v:.0}{}", unit::WATT),
            None => dash.into(),
        },
        "memory" => match (s.mem_percent, s.mem_used) {
            (Some(p), Some(u)) => format!("{p:.0}{} {u:.1}{}", unit::PERCENT, unit::GIB),
            _ => dash.into(),
        },
        "battery" => match s.battery {
            Some(v) => format!("{v}{}", unit::PERCENT),
            None => dash.into(),
        },
        "wifi_signal" => match s.wifi_dbm {
            // signal_level is dBm in common/src/types.rs. There is no dBi
            // field anywhere in the codebase.
            Some(v) => format!("{v}{}", unit::DBM),
            None => dash.into(),
        },
        "fans" => match s.fan {
            Some((v, true)) => format!("{v} rpm"),
            Some((v, false)) => format!("{v}{}", unit::PERCENT),
            None => dash.into(),
        },
        "gamepad_1" => pad_cell(s, 0),
        "gamepad_2" => pad_cell(s, 1),
        _ => dash.into(),
    }
}

fn pad_cell(s: &Snap, i: usize) -> String {
    match s.pads.get(i) {
        Some((name, Some(b), st)) => {
            let short: String = name.chars().take(8).collect();
            format!("{short} {b}{} {st}", unit::PERCENT)
        }
        Some((name, None, st)) => {
            let short: String = name.chars().take(10).collect();
            format!("{short} {st}")
        }
        _ => "—".into(),
    }
}

// ---------------------------------------------------------------- main

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg = Cfg {
        data: "synth".into(),
        scale: 1.0,
        refresh_hz: 1.0,
        items: vec![
            "cpu_load_percent".into(),
            "cpu_core_chart".into(),
            "cpu_temp".into(),
            "memory".into(),
            "battery".into(),
        ],
        labels: HashMap::new(),
        item_scales: HashMap::new(),
        always_on_top: false,
        click_through: false,
        transparent: false,
        skip_taskbar: false,
        pos: Some((20.0, 20.0)),
        seconds: 0,
    };
    let logp = std::env::var("PROBE_LOG")
        .unwrap_or_else(|_| "/home/wer/devis/lapsphere/probe/overlay_demo.log".into());
    let mut i = 0;
    while i < a.len() {
        match a[i].as_str() {
            "--data" => { cfg.data = a[i + 1].clone(); i += 2 }
            "--scale" => { cfg.scale = a[i + 1].parse().unwrap_or(1.0); i += 2 }
            "--refresh" => { cfg.refresh_hz = a[i + 1].parse().unwrap_or(1.0); i += 2 }
            "--items" => {
                cfg.items = a[i + 1]
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                i += 2;
            }
            "--label" => {
                if let Some((k, v)) = a[i + 1].split_once('=') {
                    cfg.labels.insert(k.to_string(), v.to_string());
                }
                i += 2
            }
            "--item-scale" => {
                if let Some((k, v)) = a[i + 1].split_once('=') {
                    cfg.item_scales.insert(k.to_string(), v.parse().unwrap_or(1.0));
                }
                i += 2
            }
            "--always-on-top" => { cfg.always_on_top = true; i += 1 }
            "--click-through" => { cfg.click_through = true; i += 1 }
            "--transparent" => { cfg.transparent = true; i += 1 }
            "--skip-taskbar" => { cfg.skip_taskbar = true; i += 1 }
            "--x" => { let x: f32 = a[i + 1].parse().unwrap_or(20.0); cfg.pos = Some((x, cfg.pos.map(|p| p.1).unwrap_or(20.0))); i += 2 }
            "--y" => { let y: f32 = a[i + 1].parse().unwrap_or(20.0); cfg.pos = Some((cfg.pos.map(|p| p.0).unwrap_or(20.0), y)); i += 2 }
            "--seconds" => { cfg.seconds = a[i + 1].parse().unwrap_or(0); i += 2 }
            "--log" => { i += 2 }
            "--dump-items" => {
                println!("{:<18} {:<44} {:>7}", "id", "source", "width@1.0");
                for d in ITEMS {
                    println!("{:<18} {:<44} {:>7.0}", d.id, d.source, d.width);
                }
                return;
            }
            other => { eprintln!("unknown arg {other}"); i += 1 }
        }
    }

    let f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&logp)
        .expect("open log");
    let tele = Arc::new(Tel {
        frames: AtomicU64::new(0),
        repaints: AtomicU64::new(0),
        stop: AtomicBool::new(false),
        log: Mutex::new(f),
        cpu_ms: Mutex::new(0.0),
    });
    tele.note(&format!(
        "start data={} scale={} refresh={}Hz items={:?} aot={} click_through={} transparent={} skip_taskbar={} pos={:?}",
        cfg.data, cfg.scale, cfg.refresh_hz, cfg.items, cfg.always_on_top,
        cfg.click_through, cfg.transparent, cfg.skip_taskbar, cfg.pos
    ));

    let rt = tokio::runtime::Runtime::new().unwrap();
    let _enter = rt.enter();
    let t0 = Instant::now();
    let (tx, rx) = mpsc::channel::<Snap>(4);
    if cfg.data == "live" {
        spawn_live(tx);
    } else {
        spawn_synth(tx, t0);
    }

    let mut w = 100.0;
    for id in &cfg.items {
        w += ITEMS
            .iter()
            .find(|d| d.id == *id)
            .map(|d| d.width * cfg.scale)
            .unwrap_or(80.0 * cfg.scale)
            + 6.0;
    }
    let size = vec2(w, 34.0 * cfg.scale + 12.0);

    let mut vb = egui::ViewportBuilder::default()
        .with_inner_size(size)
        .with_decorations(false)
        .with_resizable(false)
        .with_transparent(cfg.transparent)
        .with_title("LapSphere Panel Demo");
    if !cfg.skip_taskbar {
        vb = vb.with_taskbar(true);
    } else {
        vb = vb.with_taskbar(false);
    }
    if let Some((x, y)) = cfg.pos {
        vb = vb.with_position(pos2(x, y));
    }
    if cfg.always_on_top {
        vb = vb.with_window_level(egui::WindowLevel::AlwaysOnTop);
    }

    let opts = eframe::NativeOptions { viewport: vb, ..Default::default() };
    let click_through = cfg.click_through;
    // Capture the CPU baseline BEFORE the app is moved into the closure.
    let cpu_start = cpu_ticks().map(|(u, s)| (u, s, Instant::now()));

    let app = App {
        tele: tele.clone(),
        rx,
        snap: Snap::default(),
        cfg,
        t0,
        frames: 0,
        layout_seen: None,
        last_layout_change: Instant::now(),
        cpu_t0: cpu_start,
    };

    let r = eframe::run_native("LapSphere Panel Demo", opts, Box::new(move |cc| {
        let mut app = app;
        if click_through {
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
        }
        Ok(Box::new(app))
    }));

    if let (Some((u0, s0, i0)), Some((u1, s1))) = (cpu_start, cpu_ticks()) {
        let hz = 100.0;
        let pct = ((u1 - u0) as f64 + (s1 - s0) as f64) / hz / i0.elapsed().as_secs_f64() * 100.0;
        tele.note(&format!("mean CPU over run: {pct:.2}%"));
    }
    tele.note(&format!("exit: {r:?}"));
    tele.stop.store(true, Ordering::Relaxed);
}
