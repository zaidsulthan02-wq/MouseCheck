//! Polling & stability test and Swipe / tracking test (same engine, different focus).

use egui::{RichText, Ui};

use super::App;
use super::charts::{self, Series};
use super::widgets::*;
use crate::analysis::{self, MotionReport, Trace, cps_to_cm_s};
use crate::findings::{self, Finding};
use crate::session::{Session, Setup, TestKind};

#[derive(Clone, Copy, PartialEq)]
pub enum Phase {
    Idle,
    /// Recording; waiting for the first movement to start the clock.
    Armed,
    Running {
        t0: i64,
        dev: u32,
    },
}

/// Which MotionTest instance (and which page) a run belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    Polling,
    Swipe,
    /// Guided polling-rate sweep (saved as Polling runs).
    Rates,
}

impl Slot {
    pub fn of(kind: TestKind) -> Self {
        if kind == TestKind::Swipe { Slot::Swipe } else { Slot::Polling }
    }
    fn kind(self) -> TestKind {
        if self == Slot::Swipe { TestKind::Swipe } else { TestKind::Polling }
    }
}

pub struct MotionTest {
    pub phase: Phase,
    /// Setup to stamp on the saved run instead of the shared one (used by the rate sweep).
    pub setup_override: Option<Setup>,
    pub duration_s: f64,
    pub result: Option<Session>,
    pub findings: Vec<Finding>,
    scan: usize,
    live_hz: f64,
    live_cps: f64,
}

impl MotionTest {
    pub fn new(duration_s: f64) -> Self {
        Self {
            phase: Phase::Idle,
            setup_override: None,
            duration_s,
            result: None,
            findings: vec![],
            scan: 0,
            live_hz: 0.0,
            live_cps: 0.0,
        }
    }
    pub fn busy(&self) -> bool {
        self.phase != Phase::Idle
    }
}

pub(super) fn test_mut(app: &mut App, slot: Slot) -> &mut MotionTest {
    match slot {
        Slot::Polling => &mut app.polling,
        Slot::Swipe => &mut app.swipe,
        Slot::Rates => &mut app.rates.test,
    }
}

pub(super) fn start(app: &mut App, slot: Slot) {
    app.cap.start_recording();
    let t = test_mut(app, slot);
    t.phase = Phase::Armed;
    t.scan = 0;
    t.result = None;
    t.findings.clear();
}

pub(super) fn cancel(app: &mut App, slot: Slot) {
    let _ = app.cap.stop_recording();
    test_mut(app, slot).phase = Phase::Idle;
    app.toast("Test cancelled");
}

/// Advances the state machine; called every frame while the page is shown.
pub(super) fn tick(app: &mut App, slot: Slot) {
    let freq = app.cap.freq;
    let now = app.cap.now();
    let choice = app.device;
    let phase = test_mut(app, slot).phase;
    match phase {
        Phase::Idle => {}
        Phase::Armed => {
            let scan = test_mut_ref(app, slot).scan;
            let (found, len) = {
                let s = app.cap.store.lock().unwrap();
                match &s.recording {
                    Some(rec) => (
                        rec[scan.min(rec.len())..]
                            .iter()
                            .find(|e| e.moved() && !e.abs && choice.is_none_or(|c| c == e.dev))
                            .map(|e| (e.t, e.dev)),
                        rec.len(),
                    ),
                    None => (None, scan),
                }
            };
            test_mut(app, slot).scan = len;
            if let Some((t0, dev)) = found {
                test_mut(app, slot).phase = Phase::Running { t0, dev };
            }
        }
        Phase::Running { t0, dev } => {
            // Live rate over the last 250 ms (small copy, short lock).
            let window = freq / 4;
            let (mut n, mut c) = (0usize, 0f64);
            {
                let s = app.cap.store.lock().unwrap();
                for e in s.recent.iter().rev() {
                    if e.t < now - window {
                        break;
                    }
                    if e.dev == dev && e.moved() {
                        n += 1;
                        c += ((e.dx as f64).powi(2) + (e.dy as f64).powi(2)).sqrt();
                    }
                }
            }
            let t = test_mut(app, slot);
            t.live_hz = n as f64 * 4.0;
            t.live_cps = c * 4.0;
            let elapsed = (now - t0) as f64 / freq as f64;
            if elapsed >= t.duration_s {
                finish(app, slot, t0, dev);
            }
        }
    }
}

fn test_mut_ref(app: &App, slot: Slot) -> &MotionTest {
    match slot {
        Slot::Polling => &app.polling,
        Slot::Swipe => &app.swipe,
        Slot::Rates => &app.rates.test,
    }
}

fn finish(app: &mut App, slot: Slot, t0: i64, first_dev: u32) {
    let (evs, cpu) = app.cap.stop_recording();
    let dur = test_mut(app, slot).duration_s;
    let end = t0 + (dur * app.cap.freq as f64) as i64;
    let evs: Vec<_> = evs.into_iter().filter(|e| e.t >= t0 && e.t <= end).collect();
    let dev = app.device.unwrap_or(first_dev);
    let (samples, fg_pct) = app.to_samples(&evs, dev, t0);
    let mut rep = analysis::analyze_motion(&samples, app.dpi());
    rep.foreground_pct = fg_pct;
    if !cpu.is_empty() {
        rep.cpu_avg = cpu.iter().sum::<f32>() / cpu.len() as f32;
        rep.cpu_max = cpu.iter().cloned().fold(0.0, f32::max);
    }
    let setup = test_mut(app, slot).setup_override.clone().unwrap_or_else(|| app.current_setup());
    let mut s = Session::new(slot.kind(), setup, app.device_name(dev));
    s.trace = Some(Trace::from_samples(&samples));
    s.motion = Some(rep);
    let s = app.store_session(s);
    let t = test_mut(app, slot);
    t.findings = findings::motion(s.motion.as_ref().unwrap());
    t.result = Some(s);
    t.phase = Phase::Idle;
}

pub fn page(app: &mut App, ui: &mut Ui, kind: TestKind) {
    let slot = Slot::of(kind);
    tick(app, slot);
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) && test_mut(app, slot).busy() {
        cancel(app, slot);
    }

    match kind {
        TestKind::Polling => title(
            ui,
            "Polling & stability",
            "Is every report arriving on time, and when it isn't, is the PC or the wireless link to blame?",
        ),
        _ => title(
            ui,
            "Swipe / tracking",
            "Your normal aiming motion: flicks, tracking, micro-adjustments. Shows hitches and sensor glitches in real use.",
        ),
    }

    let phase = test_mut(app, slot).phase;
    if phase == Phase::Idle {
        app.setup_editor(ui);
        card(ui, |ui| {
            ui.label(RichText::new("How to do it").strong());
            match kind {
                TestKind::Polling => {
                    ui.label("1. Press Start. The cursor is hidden and locked to this window.");
                    ui.label("2. Move the mouse in big, fast circles or quick left/right swipes, and don't stop.");
                    ui.label("3. The clock starts on your first movement and stops automatically.");
                    muted(
                        ui,
                        "Tip: run it once per polling rate (1000 / 2000 / 4000 / 8000 Hz) with the run label set to the rate, then compare them.",
                    );
                }
                _ => {
                    ui.label("1. Press Start. The cursor is hidden and locked to this window.");
                    ui.label("2. Aim like you do in game: flicks, tracking, small corrections, stops and starts.");
                    ui.label("3. The clock starts on your first movement and stops automatically.");
                    muted(
                        ui,
                        "Tip: do this before and after cleaning the sensor / changing the pad, with the same DPI and polling rate.",
                    );
                }
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let t = test_mut(app, slot);
                ui.label("Duration");
                ui.add(egui::DragValue::new(&mut t.duration_s).range(5.0..=120.0).suffix(" s").speed(1.0));
                ui.add_space(12.0);
                if big_button(ui, "Start", ACCENT).clicked() {
                    start(app, slot);
                }
            });
        });
    } else {
        running_view(app, ui, slot);
    }

    let t = test_mut_ref(app, slot);
    if let (Some(s), Phase::Idle) = (&t.result, t.phase) {
        let s = s.clone();
        let f = t.findings.clone();
        ui.add_space(8.0);
        result_view(ui, &s, &f, &format!("{kind:?}"));
    }
}

pub(super) fn running_view(app: &mut App, ui: &mut Ui, slot: Slot) {
    let freq = app.cap.freq as f64;
    let now = app.cap.now();
    let dpi = app.dpi();
    let t = test_mut(app, slot);
    card(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(20.0);
            match t.phase {
                Phase::Armed => {
                    ui.label(RichText::new("Start moving the mouse").size(30.0).strong().color(ACCENT));
                    ui.label(RichText::new("Recording begins on the first movement").color(MUTED));
                }
                Phase::Running { t0, .. } => {
                    let el = (now - t0) as f64 / freq;
                    let left = (t.duration_s - el).max(0.0);
                    ui.label(
                        RichText::new(if slot != Slot::Swipe { "Keep moving fast!" } else { "Aim naturally" })
                            .size(30.0)
                            .strong()
                            .color(ACCENT),
                    );
                    ui.add_space(6.0);
                    ui.label(RichText::new(format!("{left:.1} s")).size(44.0).strong());
                    ui.add(egui::ProgressBar::new((el / t.duration_s) as f32).desired_width(420.0));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add_space((ui.available_width() - 440.0).max(0.0) / 2.0);
                        tile(ui, "Live reports / s", &format!("{:.0}", t.live_hz), "last 250 ms", ACCENT);
                        let speed = match dpi {
                            Some(d) => format!("{:.0} cm/s", cps_to_cm_s(t.live_cps, d)),
                            None => format!("{:.0} c/s", t.live_cps),
                        };
                        tile(ui, "Speed", &speed, "counts per second", TEXT);
                    });
                }
                Phase::Idle => {}
            }
            ui.add_space(14.0);
            ui.label(RichText::new("Esc to cancel").small().color(MUTED));
            ui.add_space(10.0);
        });
    });
}

/// Full result view for a motion session; reused by the Results page.
pub fn result_view(ui: &mut Ui, s: &Session, findings_list: &[Finding], id: &str) {
    let Some(r) = &s.motion else { return };
    section(ui, &format!("Result: {}", s.title()));
    muted(ui, format!("{} · {} · {}", s.created, s.device, setup_line(s)));
    if !r.ok {
        findings(ui, findings_list);
        return;
    }
    tiles(ui, r);
    ui.add_space(4.0);
    findings(ui, findings_list);
    charts_for(ui, r, id);
    hitch_table(ui, r);
}

pub fn setup_line(s: &Session) -> String {
    let mut parts = vec![];
    if let Some(d) = s.setup.dpi {
        parts.push(format!("{d:.0} DPI"));
    }
    if !s.setup.polling_setting.is_empty() {
        parts.push(format!("set {}", s.setup.polling_setting));
    }
    if !s.setup.surface.is_empty() {
        parts.push(s.setup.surface.clone());
    }
    if parts.is_empty() { "no setup details".into() } else { parts.join(" · ") }
}

pub fn tiles(ui: &mut Ui, r: &MotionReport) {
    {
        let mut tg = TileGrid::default();
        tg.add("Polling rate", &format!("{:.0} Hz", r.nominal_hz), &format!("measured {:.0}", r.measured_hz), ACCENT);
        tg.add(
            "Rate held while fast",
            &format!("{}%", fmt(r.rate_held_pct, 1)),
            "≥85% of nominal",
            grade(r.rate_held_pct, 97.0, 85.0),
        );
        tg.add(
            "Hitches / min",
            &fmt(r.hitches_per_min, 1),
            "gaps ≥ 2 ms while moving",
            grade(r.hitches_per_min, 3.0, 20.0),
        );
        tg.add("Worst gap", &format!("{} ms", fmt(r.max_gap_ms, 1)), "longest freeze", grade(r.max_gap_ms, 6.0, 16.0));
        tg.add(
            "Timing jitter p99",
            &format!("{} µs", fmt(r.jitter_p99_us, 0)),
            &format!("ideal interval {:.0} µs", 1e6 / r.nominal_hz),
            MUTED,
        );
        tg.add(
            "Sensor glitches",
            &fmt(r.reversals_per_10k + r.spikes_per_10k, 1),
            "per 10k reports",
            grade(r.reversals_per_10k + r.spikes_per_10k, 2.0, 10.0),
        );
        tg.add("Velocity noise", &format!("{}%", fmt(r.velocity_noise_pct, 1)), "lower = smoother", MUTED);
        tg.add(
            "Fast enough",
            &format!("{:.0}%", r.fast_time_pct),
            "of moving time",
            grade(r.fast_time_pct, 50.0, 20.0),
        );
        tg.add(
            "PC CPU load",
            &format!("{:.0}%", r.cpu_avg),
            &format!("max {:.0}%", r.cpu_max),
            grade(r.cpu_max as f64, 70.0, 95.0),
        );
        tg.show(ui);
    }
}

fn charts_for(ui: &mut Ui, r: &MotionReport, id: &str) {
    section(ui, "Reports per second over time");
    muted(
        ui,
        "Each point is a 20 ms window. Vertical lines mark hitches: amber = wireless/mouse delay, violet = PC-side stall, red = lost motion.",
    );
    charts::rate_plot(
        ui,
        &format!("{id}_rate"),
        &[Series { name: "reports/s".into(), points: &r.rate_series, color: ACCENT }],
        Some(r.nominal_hz),
        &r.hitches,
        220.0,
    );
    let (speed, needed, unit): (Vec<[f64; 2]>, f64, &str) = match r.dpi {
        Some(d) => (
            r.speed_series.iter().map(|p| [p[0], cps_to_cm_s(p[1], d)]).collect(),
            cps_to_cm_s(r.needed_cps(), d),
            "cm/s",
        ),
        None => (r.speed_series.clone(), r.needed_cps(), "counts/s"),
    };
    section(ui, "Your movement speed");
    muted(
        ui,
        "Below the dashed line the mouse has nothing new to send at every poll, so a lower report rate there is normal.",
    );
    charts::speed_plot(
        ui,
        &format!("{id}_speed"),
        &[Series { name: "speed".into(), points: &speed, color: GOOD }],
        Some(needed),
        unit,
        160.0,
    );
    if !r.interval_hist.is_empty() {
        section(ui, "Interval distribution (while moving fast)");
        muted(
            ui,
            "A healthy mouse makes one tall bar at the ideal interval. Bars near zero = reports arriving in bursts; bars to the right = late or missing reports.",
        );
        charts::hist_plot(
            ui,
            &format!("{id}_hist"),
            &[Series { name: "intervals".into(), points: &r.interval_hist, color: INFO }],
            Some(1e6 / r.nominal_hz),
            180.0,
        );
    }
}

fn hitch_table(ui: &mut Ui, r: &MotionReport) {
    if r.hitches.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(format!("All hitches ({})", r.hitches.len())).show(ui, |ui| {
        egui::Grid::new("hitches").striped(true).num_columns(3).show(ui, |ui| {
            ui.label(RichText::new("time (s)").strong());
            ui.label(RichText::new("gap (ms)").strong());
            ui.label(RichText::new("likely cause").strong());
            ui.end_row();
            for h in r.hitches.iter().take(200) {
                ui.label(format!("{:.3}", h.t_s));
                ui.label(format!("{:.2}", h.gap_ms));
                ui.label(h.kind.label());
                ui.end_row();
            }
        });
    });
}
