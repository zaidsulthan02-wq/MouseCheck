//! Round-based physical tests: return-to-origin accuracy and real DPI.
//! Both mark rounds with the keyboard (Space) so mouse buttons never disturb the sensor.

use egui::{Key, RichText, Ui};

use super::App;
use super::widgets::*;
use crate::analysis::{AccuracyReport, DpiReport, DpiRun, Round};
use crate::findings::{self, Finding};
use crate::session::{Session, TestKind};

pub struct RoundsTest {
    pub active: bool,
    mark: Option<i64>,
    dev: Option<u32>,
    cur: Round,
    pub rounds: Vec<Round>,
    pub target: usize,
    pub distance_mm: f64,
    pub result: Option<Session>,
    pub findings: Vec<Finding>,
    scan: usize,
}

impl RoundsTest {
    pub fn new(kind: TestKind) -> Self {
        Self {
            active: false,
            mark: None,
            dev: None,
            cur: Round::default(),
            rounds: vec![],
            target: if kind == TestKind::Dpi { 3 } else { 5 },
            distance_mm: 100.0,
            result: None,
            findings: vec![],
            scan: 0,
        }
    }
}

fn t(app: &mut App, kind: TestKind) -> &mut RoundsTest {
    if kind == TestKind::Accuracy { &mut app.accuracy } else { &mut app.dpi }
}

fn start(app: &mut App, kind: TestKind) {
    app.cap.start_recording();
    let x = t(app, kind);
    x.active = true;
    x.mark = None;
    x.rounds.clear();
    x.result = None;
    x.findings.clear();
    x.scan = 0;
}

fn tick(app: &mut App, kind: TestKind) {
    let choice = app.device;
    let (mark, scan, mut dev) = {
        let x = t(app, kind);
        (x.mark, x.scan, x.dev)
    };
    let mut add = Round::default();
    let len = {
        let s = app.cap.store.lock().unwrap();
        let Some(rec) = &s.recording else { return };
        if let Some(m) = mark {
            for e in &rec[scan.min(rec.len())..] {
                if e.t < m || !e.moved() || e.abs {
                    continue;
                }
                if dev.is_none() {
                    dev = Some(choice.unwrap_or(e.dev));
                }
                if Some(e.dev) == dev {
                    add.sum_dx += e.dx as i64;
                    add.sum_dy += e.dy as i64;
                    add.path += ((e.dx as f64).powi(2) + (e.dy as f64).powi(2)).sqrt();
                }
            }
        }
        rec.len()
    };
    let x = t(app, kind);
    x.scan = len;
    x.dev = dev;
    x.cur.sum_dx += add.sum_dx;
    x.cur.sum_dy += add.sum_dy;
    x.cur.path += add.path;
}

fn on_space(app: &mut App, kind: TestKind) {
    let now = app.cap.now();
    let freq = app.cap.freq as f64;
    let x = t(app, kind);
    match x.mark {
        None => {
            x.mark = Some(now);
            x.cur = Round::default();
            x.dev = None;
        }
        Some(m) => {
            let mut r = std::mem::take(&mut x.cur);
            r.duration_s = (now - m) as f64 / freq;
            x.mark = None;
            if r.path > 0.0 {
                x.rounds.push(r);
            }
            if x.rounds.len() >= x.target {
                finish(app, kind);
            }
        }
    }
}

fn finish(app: &mut App, kind: TestKind) {
    let _ = app.cap.stop_recording();
    let dpi = app.dpi();
    let dev = t(app, kind).dev;
    let device = dev.map(|d| app.device_name(d)).unwrap_or_else(|| "Unknown".into());
    let x = t(app, kind);
    x.active = false;
    let rounds = std::mem::take(&mut x.rounds);
    let distance = x.distance_mm;
    let mut s = Session::new(kind, app.current_setup(), device);
    if kind == TestKind::Accuracy {
        s.accuracy = Some(AccuracyReport::from_rounds(rounds, dpi));
    } else {
        let runs = rounds.into_iter().map(|round| DpiRun { distance_mm: distance, round }).collect();
        s.dpi = Some(DpiReport::from_runs(runs, dpi));
    }
    let s = app.store_session(s);
    let x = t(app, kind);
    x.findings = session_findings(&s);
    x.result = Some(s);
}

pub fn session_findings(s: &Session) -> Vec<Finding> {
    if let Some(a) = &s.accuracy {
        findings::accuracy(a)
    } else if let Some(d) = &s.dpi {
        findings::dpi(d)
    } else {
        vec![]
    }
}

pub fn page(app: &mut App, ui: &mut Ui, kind: TestKind) {
    let active = t(app, kind).active;
    if active {
        tick(app, kind);
        let (space, back, esc) =
            ui.input(|i| (i.key_pressed(Key::Space), i.key_pressed(Key::Backspace), i.key_pressed(Key::Escape)));
        if esc {
            let _ = app.cap.stop_recording();
            t(app, kind).active = false;
            app.toast("Test cancelled");
        } else if space {
            on_space(app, kind);
        } else if back {
            let x = t(app, kind);
            if x.mark.take().is_none() {
                x.rounds.pop();
            }
        }
    }

    if kind == TestKind::Accuracy {
        title(
            ui,
            "Return-to-origin accuracy",
            "Does the sensor lose or invent motion? The most direct before/after test for cleaning the sensor or changing the mousepad.",
        );
    } else {
        title(
            ui,
            "Real DPI",
            "Measure the true counts per inch against a ruler, and check whether the DPI you set is what you get.",
        );
    }

    let active = t(app, kind).active;
    if !active {
        app.setup_editor(ui);
        card(ui, |ui| {
            ui.label(RichText::new("What you need").strong());
            if kind == TestKind::Accuracy {
                ui.label("A fixed stop on your pad: an inside corner made by a book and a ruler, or the corner of a box lid. The mouse should touch it on its left side and top.");
                ui.add_space(4.0);
                ui.label(RichText::new("Each round").strong());
                ui.label("1. Push the mouse into the corner and press Space.");
                ui.label(
                    "2. Move around the pad for 5–10 seconds: circles, swipes, flicks. Keep it on the pad, no lifting.",
                );
                ui.label("3. Bring it back into the corner, the same way it touched before, and press Space.");
                muted(
                    ui,
                    "A perfect sensor returns to (0, 0). The leftover is motion the sensor lost or invented. Do 5 rounds, then repeat after cleaning or on the other pad.",
                );
            } else {
                ui.label("A ruler or straight edge taped down, and two marks a known distance apart (100 mm is good).");
                ui.add_space(4.0);
                ui.label(RichText::new("Each round").strong());
                ui.label("1. Put the mouse's edge on the first mark and press Space.");
                ui.label(
                    "2. Slide it slowly along the straight edge to the second mark, without lifting, and press Space.",
                );
                muted(ui, "Enable DPI in the setup above to compare with your configured value.");
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let x = t(app, kind);
                ui.label("Rounds");
                ui.add(egui::DragValue::new(&mut x.target).range(1..=20));
                if kind == TestKind::Dpi {
                    ui.add_space(8.0);
                    ui.label("Distance");
                    ui.add(egui::DragValue::new(&mut x.distance_mm).range(10.0..=1000.0).suffix(" mm"));
                }
                ui.add_space(12.0);
                if big_button(ui, "Start", ACCENT).clicked() {
                    start(app, kind);
                }
            });
        });
    } else {
        running(app, ui, kind);
    }

    let x = t(app, kind);
    if let (Some(s), false) = (&x.result, x.active) {
        let s = s.clone();
        let f = x.findings.clone();
        ui.add_space(8.0);
        result_view(ui, &s, &f);
    }
}

fn running(app: &mut App, ui: &mut Ui, kind: TestKind) {
    let dpi = app.dpi();
    let x = t(app, kind);
    card(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(14.0);
            ui.label(
                RichText::new(format!("Round {} of {}", (x.rounds.len() + 1).min(x.target), x.target)).color(MUTED),
            );
            if x.mark.is_none() {
                let msg = if kind == TestKind::Accuracy {
                    "Put the mouse in the corner, then press Space"
                } else {
                    "Put the mouse on the first mark, then press Space"
                };
                ui.label(RichText::new(msg).size(26.0).strong().color(ACCENT));
            } else {
                let msg = if kind == TestKind::Accuracy {
                    "Move around, return to the corner, press Space"
                } else {
                    "Slide to the second mark, press Space"
                };
                ui.label(RichText::new(msg).size(26.0).strong().color(WARN));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - 480.0).max(0.0) / 2.0);
                    if kind == TestKind::Accuracy {
                        tile(
                            ui,
                            "Offset from start",
                            &format!("{}, {}", x.cur.sum_dx, x.cur.sum_dy),
                            "counts (x, y)",
                            TEXT,
                        );
                        tile(ui, "Distance travelled", &format!("{:.0}", x.cur.path), "counts", TEXT);
                    } else {
                        let live = x.cur.net() / (x.distance_mm / 25.4);
                        tile(ui, "Counts", &format!("{:.0}", x.cur.net()), &format!("over {} mm", x.distance_mm), TEXT);
                        tile(
                            ui,
                            "DPI so far",
                            &format!("{live:.0}"),
                            if dpi.is_some() { "vs your set DPI" } else { "" },
                            ACCENT,
                        );
                    }
                });
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - 520.0).max(0.0) / 2.0);
                key_hint(ui, "Space", "start / end round");
                key_hint(ui, "Backspace", "redo");
                key_hint(ui, "Esc", "cancel");
            });
            ui.add_space(10.0);
        });
    });
    if !x.rounds.is_empty() {
        rounds_table(ui, kind, &x.rounds, x.distance_mm, dpi);
    }
}

fn rounds_table(ui: &mut Ui, kind: TestKind, rounds: &[Round], distance: f64, dpi: Option<f64>) {
    card(ui, |ui| {
        egui::Grid::new(format!("rounds_{kind:?}")).striped(true).num_columns(5).show(ui, |ui| {
            let heads: &[&str] = if kind == TestKind::Accuracy {
                &["#", "error (counts)", "error %", "path (counts)", "time"]
            } else {
                &["#", "counts", "DPI", "angle", "time"]
            };
            for h in heads {
                tip_label(ui, h, TEXT, true);
            }
            ui.end_row();
            for (i, r) in rounds.iter().enumerate() {
                ui.label(format!("{}", i + 1));
                if kind == TestKind::Accuracy {
                    let mm = dpi.map(|d| format!(" (≈{:.1} mm)", r.error() / d * 25.4)).unwrap_or_default();
                    ui.label(format!("{:.1}{mm}", r.error()));
                    ui.label(RichText::new(format!("{:.2}%", r.error_pct())).color(grade(r.error_pct(), 0.5, 4.0)));
                    ui.label(format!("{:.0}", r.path));
                } else {
                    let run = DpiRun { distance_mm: distance, round: r.clone() };
                    ui.label(format!("{:.0}", r.net()));
                    ui.label(format!("{:.0}", run.dpi()));
                    ui.label(format!("{:.1}°", run.angle()));
                }
                ui.label(format!("{:.1} s", r.duration_s));
                ui.end_row();
            }
        });
    });
}

pub fn result_view(ui: &mut Ui, s: &Session, list: &[Finding]) {
    section(ui, &format!("Result: {}", s.title()));
    muted(ui, format!("{} · {} · {}", s.created, s.device, super::motion::setup_line(s)));
    if let Some(a) = &s.accuracy {
        {
            let mut tg = TileGrid::default();
            tg.add(
                "Average error",
                &format!("{:.2}%", a.mean_err_pct),
                "of distance travelled",
                grade(a.mean_err_pct, 0.5, 4.0),
            );
            let mm = a.mean_err_mm().map(|v| format!("≈{v:.1} mm")).unwrap_or_else(|| "set DPI for mm".into());
            tg.add("Average miss", &format!("{:.1} counts", a.mean_err_counts), &mm, TEXT);
            tg.add("Worst round", &format!("{:.1} counts", a.max_err_counts), "", TEXT);
            tg.add("Rounds", &format!("{}", a.rounds.len()), "", MUTED);
            tg.show(ui);
        }
        findings(ui, list);
        rounds_table(ui, TestKind::Accuracy, &a.rounds, 0.0, a.dpi);
    }
    if let Some(d) = &s.dpi {
        {
            let mut tg = TileGrid::default();
            tg.add("Measured DPI", &format!("{:.0}", d.mean_dpi), "average of rounds", ACCENT);
            if let Some(dev) = d.deviation_pct() {
                tg.add(
                    "vs set DPI",
                    &format!("{dev:+.1}%"),
                    &format!("set {:.0}", d.set_dpi.unwrap_or(0.0)),
                    grade(dev.abs(), 3.0, 8.0),
                );
            }
            tg.add("Spread", &format!("{:.1}%", d.spread_pct), "between rounds", grade(d.spread_pct, 2.0, 5.0));
            tg.show(ui);
        }
        findings(ui, list);
        let rounds: Vec<Round> = d.runs.iter().map(|r| r.round.clone()).collect();
        let dist = d.runs.first().map(|r| r.distance_mm).unwrap_or(100.0);
        rounds_table(ui, TestKind::Dpi, &rounds, dist, d.set_dpi);
    }
}
