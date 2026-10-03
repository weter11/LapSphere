// Throwaway frametime probe: a synthetic "game" for A/B testing the panel.
//
// A window with a continuously animating scene, vsync-limited, writing the
// interval between presented frames to a CSV. Used to measure whether an
// overlay costs frames, with docs/development/tools/frametime_stats.py doing
// the statistics.
//
// It is deliberately NOT a game: it draws rotating geometry in egui, so it
// exercises the compositor and the panel's repaint without GPU load. Absolute
// numbers say nothing about a real title; only A/B deltas against the same
// scene on the same machine are meaningful.
//
// The eframe paint callback is the closest thing to "a frame was presented"
// available from inside the app. We record the interval between paint
// callbacks, which includes the app's own layout cost; vsync is left on
// (eframe's NativeOptions::vsync default) so the measurement is
// display-limited like a real game.
//
//   cargo run --release --example frametime_probe -- --seconds 120 --out f.csv
//
// Options:
//   --seconds <n>     run length (default 120)
//   --out <path>      CSV output (default /tmp/frametime.csv)
//   --w <px> --h <px> window size (default 1280x720)
//   --no-vsync        disable vsync (measures raw throughput instead)

use std::io::Write;
use std::time::{Duration, Instant};

use eframe::egui;

struct Probe {
    t0: Instant,
    last_paint: Option<Instant>,
    out: std::fs::File,
    frames: u64,
    deadline: Duration,
    vsync: bool,
    log: String,
}

impl eframe::App for Probe {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // eframe 0.34 calls App::ui once per painted frame, so the interval
        // between entries is one frame. There is no separate `paint` hook in
        // this version.
        let now = Instant::now();
        if let Some(prev) = self.last_paint {
            let dt = now.duration_since(prev).as_secs_f64() * 1000.0;
            let _ = writeln!(self.out, "{dt:.4}");
        }
        self.last_paint = Some(now);
        self.frames += 1;
        if now.duration_since(self.t0) > self.deadline {
            let _ = self.out.flush();
            eprintln!(
                "frametime_probe: {} frames in {:.1}s, log {}",
                self.frames,
                now.duration_since(self.t0).as_secs_f64(),
                self.log
            );
            std::process::exit(0);
        }

        let ctx = ui.ctx().clone();
        self.draw(ui, &ctx);
        ctx.request_repaint();
    }
}

impl Probe {
    fn draw(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // A continuously animating scene: forces a repaint every frame and
        // gives the GPU and the rasteriser something to do, so an overlay's
        // cost shows up in the frame interval.
        let t = ctx.input(|i| i.time) as f32;
        {
            let ui = &mut *ui;
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), ui.available_height()),
                egui::Sense::hover(),
            );
            let p = ui.painter();
            p.rect_filled(rect, 0.0, egui::Color32::from_rgb(20, 22, 30));
            let n = 64usize;
            for i in 0..n {
                let a = t * 1.7 + (i as f32) * 0.098;
                let rad = (rect.height() * 0.45) * (1.0 - (i as f32 / n as f32) * 0.8);
                let c = egui::pos2(
                    rect.center().x + rad * a.cos(),
                    rect.center().y + rad * a.sin(),
                );
                p.circle_filled(
                    c,
                    3.0 + 9.0 * (i as f32 / n as f32),
                    egui::Color32::from_rgb(
                        (40 + i * 3) as u8,
                        (120 + i * 2) as u8,
                        (200 - i) as u8,
                    ),
                );
            }
            p.text(
                rect.left_top() + egui::vec2(16.0, 24.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "frametime probe  frame {}  t={:.1}s  vsync={}",
                    self.frames, t, self.vsync
                ),
                egui::FontId::proportional(14.0),
                egui::Color32::LIGHT_GRAY,
            );
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut seconds = 120u64;
    let mut out = "/tmp/frametime.csv".to_string();
    let mut w = 1280.0f32;
    let mut h = 720.0f32;
    let mut vsync = true;
    let mut i = 0;
    while i < a.len() {
        match a[i].as_str() {
            "--seconds" => { seconds = a[i + 1].parse().unwrap_or(120); i += 2 }
            "--out" => { out = a[i + 1].clone(); i += 2 }
            "--w" => { w = a[i + 1].parse().unwrap_or(1280.0); i += 2 }
            "--h" => { h = a[i + 1].parse().unwrap_or(720.0); i += 2 }
            "--no-vsync" => { vsync = false; i += 1 }
            other => { eprintln!("unknown arg {other}"); i += 1 }
        }
    }

    let f = std::fs::File::create(&out).expect("create csv");
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([w, h])
            .with_title("frametime probe"),
        vsync,
        ..Default::default()
    };
    eprintln!("frametime_probe: writing to {out} for {seconds}s (vsync={vsync})");
    let r = eframe::run_native(
        "frametime probe",
        opts,
        Box::new(move |_cc| {
            Ok(Box::new(Probe {
                t0: Instant::now(),
                last_paint: None,
                out: f,
                frames: 0,
                deadline: Duration::from_secs(seconds),
                vsync,
                log: out.clone(),
            }))
        }),
    );
    eprintln!("frametime_probe: exit {r:?}");
}