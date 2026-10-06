//! Buttons & wheel: double-click (switch chatter) and scroll-encoder faults.

use egui::{Key, RichText, Ui};

use super::App;
use super::widgets::*;
use crate::analysis::{self, Sample, Trace};
use crate::capture::btn;
use crate::findings::{self, Finding};
use crate::session::{Session, TestKind};

pub struct ClickTest {
    pub active: bool,
    pub threshold_ms: f64,
    t0: i64,
    scan: usize,
    dev: Option<u32>,
    events: Vec<Sample>,
    pub result: Option<Session>,
    pub findings: Vec<Finding>,
}

impl Default for ClickTest {
    fn default() -> Self {
        Self {
            active: false,
            threshold_ms: 80.0,
            t0: 0,
            scan: 0,
            dev: None,
            events: vec![],
            result: None,
            findings: vec![],
        }
    }
}

fn tick(app: &mut App) {
    let choice = app.device;
    let (scan, t0, mut dev) = (app.clicks.scan, app.clicks.t0, app.clicks.dev);
    let mut new = Vec::new();
    let len = {
        let s = app.cap.store.lock().unwrap();
        let Some(rec) = &s.recording else { return };
        for e in &rec[scan.min(rec.len())..] {
            if e.bflags == 0 || e.abs {
                continue;
            }
            if dev.is_none() {
                dev = Some(choice.unwrap_or(e.dev));
            }
            if Some(e.dev) == dev {
                new.push(Sample { t: app.cap.ticks_to_us(e.t - t0), dx: 0, dy: 0, bf: e.bflags, bd: e.bdata });
            }
        }
        rec.len()
    };
    let c = &mut app.clicks;
    c.scan = len;
    c.dev = dev;
    c.events.extend(new);
}

fn finish(app: &mut App, trim_last_left_click: bool) {
    let _ = app.cap.stop_recording();
    let c = &mut app.clicks;
    c.active = false;
    if trim_last_left_click {
        // The click on the "Finish" button itself shouldn't count.
        if let Some(i) = c.events.iter().rposition(|e| e.bf & btn::PAIRS[0].0 != 0) {
            c.events.truncate(i);
        }
    }
    let rep = analysis::analyze_clicks(&c.events, c.threshold_ms);
    let trace = Trace::from_samples(&c.events);
    let device = c.dev.map(|d| app.device_name(d)).unwrap_or_else(|| "Unknown".into());
    let mut s = Session::new(TestKind::Clicks, app.current_setup(), device);
    s.trace = Some(trace);
    s.clicks = Some(rep);
    let s = app.store_session(s);
    app.clicks.findings = findings::clicks(s.clicks.as_ref().unwrap());
    app.clicks.result = Some(s);
}

pub fn page(app: &mut App, ui: &mut Ui) {
    if app.clicks.active {
        tick(app);
        let (enter, esc) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
        if esc {
            let _ = app.cap.stop_recording();
            app.clicks.active = false;
            app.toast("Test cancelled");
        } else if enter {
            finish(app, false);
        }
    }

    title(
        ui,
        "Buttons & wheel",
        "Finds the classic 2-year-old-mouse faults: switches that double-click and scroll wheels that jump.",
    );

    if !app.clicks.active {
        app.setup_editor(ui);
        card(ui, |ui| {
            ui.label(RichText::new("How to do it").strong());
            ui.label("1. Press Start. The cursor stays inside this window.");
            ui.label("2. Click each button about 20 times, slowly and deliberately (about 2 clicks per second). Include the side buttons.");
            ui.label("3. Scroll the wheel slowly down 20 notches, then up 20 notches.");
            ui.label("4. Press Enter (or the Finish button) when done.");
            muted(
                ui,
                "A healthy switch never registers a second press within a few dozen ms of a deliberate single click, so any such press is flagged.",
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Flag presses faster than");
                ui.add(egui::DragValue::new(&mut app.clicks.threshold_ms).range(10.0..=200.0).suffix(" ms"));
                ui.add_space(12.0);
                if big_button(ui, "Start", ACCENT).clicked() {
                    app.cap.start_recording();
                    let c = &mut app.clicks;
                    c.active = true;
                    c.t0 = app.cap.now();
                    c.scan = 0;
                    c.dev = None;
                    c.events.clear();
                    c.result = None;
                }
            });
        });
    } else {
        let live = analysis::analyze_clicks(&app.clicks.events, app.clicks.threshold_ms);
        card(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                ui.label(RichText::new("Click every button, scroll both ways").size(26.0).strong().color(ACCENT));
                ui.add_space(8.0);
            });
            {
                let mut tg = TileGrid::default();
                for b in &live.buttons {
                    let c = if b.chatter > 0 {
                        BAD
                    } else if b.presses > 0 {
                        GOOD
                    } else {
                        MUTED
                    };
                    let hint =
                        if b.chatter > 0 { format!("{} double-click faults!", b.chatter) } else { "presses".into() };
                    tg.add(&b.name, &format!("{}", b.presses), &hint, c);
                }
                let wc = if live.wheel.wrong_way > 0 {
                    BAD
                } else if live.wheel.up + live.wheel.down > 0 {
                    GOOD
                } else {
                    MUTED
                };
                tg.add(
                    "Wheel",
                    &format!("↑{} ↓{}", live.wheel.up, live.wheel.down),
                    &format!("{} wrong-way", live.wheel.wrong_way),
                    wc,
                );
                tg.show(ui);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if big_button(ui, "Finish", GOOD).clicked() {
                    finish(app, true);
                }
                key_hint(ui, "Enter", "finish");
                key_hint(ui, "Esc", "cancel");
            });
        });
    }

    if let (Some(s), false) = (&app.clicks.result, app.clicks.active) {
        let s = s.clone();
        let f = app.clicks.findings.clone();
        ui.add_space(8.0);
        result_view(ui, &s, &f);
    }
}

pub fn result_view(ui: &mut Ui, s: &Session, list: &[Finding]) {
    let Some(r) = &s.clicks else { return };
    section(ui, &format!("Result: {}", s.title()));
    muted(ui, format!("{} · {}", s.created, s.device));
    findings(ui, list);
    card(ui, |ui| {
        egui::Grid::new(format!("clicks_{}", s.id)).striped(true).num_columns(5).show(ui, |ui| {
            for h in ["button", "presses", "double-click faults", "shortest gap", "median hold"] {
                tip_label(ui, h, TEXT, true);
            }
            ui.end_row();
            for b in &r.buttons {
                ui.label(&b.name);
                ui.label(format!("{}", b.presses));
                ui.label(RichText::new(format!("{}", b.chatter)).color(if b.chatter > 0 { BAD } else { TEXT }));
                ui.label(format!("{} ms", fmt(b.min_gap_ms, 1)));
                ui.label(format!("{} ms", fmt(b.hold_median_ms, 0)));
                ui.end_row();
            }
        });
    });
}
