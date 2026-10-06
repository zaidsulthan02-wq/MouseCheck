//! Saved runs and before/after comparison.

use std::collections::BTreeSet;

use egui::{Color32, RichText, Ui};

use super::App;
use super::charts::{self, Series};
use super::widgets::*;
use crate::findings;
use crate::session::{self, Session, TestKind};

#[derive(Default)]
pub struct SessionsUi {
    pub selected: BTreeSet<String>,
    pub view: Option<String>,
    pub filter: Option<TestKind>,
}

pub fn page(app: &mut App, ui: &mut Ui) {
    title(
        ui,
        "Results & compare",
        "Every run is saved automatically. Tick two or more runs of the same test to compare them side by side.",
    );

    ui.horizontal(|ui| {
        let f = &mut app.sess_ui.filter;
        ui.selectable_value(f, None, "All");
        for k in [TestKind::Polling, TestKind::Swipe, TestKind::Accuracy, TestKind::Dpi, TestKind::Clicks] {
            ui.selectable_value(f, Some(k), k.label());
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Open folder").clicked() {
                let d = session::dir();
                let _ = std::fs::create_dir_all(&d);
                let _ = std::process::Command::new("explorer").arg(d).spawn();
            }
            if ui.button("Reload").clicked() {
                app.sessions = session::load_all();
            }
        });
    });

    let mut delete: Option<String> = None;
    card(ui, |ui| {
        if app.sessions.is_empty() {
            muted(ui, "No runs yet. Do a test and it shows up here.");
            return;
        }
        egui::ScrollArea::vertical().id_salt("sess_list").max_height(300.0).show(ui, |ui| {
            egui::Grid::new("sessions").striped(true).num_columns(6).min_col_width(20.0).show(ui, |ui| {
                for h in ["", "run", "test", "when", "device", ""] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for s in app.sessions.iter().filter(|s| app.sess_ui.filter.is_none_or(|k| k == s.kind)) {
                    let key = s.file_key();
                    let mut on = app.sess_ui.selected.contains(&key);
                    if ui.checkbox(&mut on, "").changed() {
                        if on {
                            app.sess_ui.selected.insert(key.clone());
                        } else {
                            app.sess_ui.selected.remove(&key);
                        }
                    }
                    let label = if s.setup.label.is_empty() { "untitled" } else { &s.setup.label };
                    let viewing = app.sess_ui.view.as_ref() == Some(&key);
                    if ui.selectable_label(viewing, RichText::new(label).strong()).clicked() {
                        app.sess_ui.view = Some(key.clone());
                    }
                    ui.label(RichText::new(s.kind.label()).color(MUTED));
                    ui.label(RichText::new(&s.created).color(MUTED));
                    ui.label(RichText::new(&s.device).small().color(MUTED));
                    if ui.small_button("Delete").clicked() {
                        delete = Some(key.clone());
                    }
                    ui.end_row();
                }
            });
        });
    });

    if let Some(k) = delete {
        if let Some(i) = app.sessions.iter().position(|s| s.file_key() == k) {
            let s = app.sessions.remove(i);
            session::delete(&s);
            app.sess_ui.selected.remove(&k);
            if app.sess_ui.view.as_ref() == Some(&k) {
                app.sess_ui.view = None;
            }
        }
    }

    let chosen: Vec<Session> =
        app.sessions.iter().filter(|s| app.sess_ui.selected.contains(&s.file_key())).cloned().collect();
    if chosen.len() >= 2 {
        compare(ui, &chosen);
    } else if let Some(s) =
        app.sess_ui.view.as_ref().and_then(|k| app.sessions.iter().find(|s| &s.file_key() == k)).cloned()
    {
        ui.add_space(8.0);
        match s.kind {
            TestKind::Polling | TestKind::Swipe => {
                let f = s.motion.as_ref().map(findings::motion).unwrap_or_default();
                super::motion::result_view(ui, &s, &f, &s.file_key());
            }
            TestKind::Accuracy | TestKind::Dpi => {
                let f = super::rounds::session_findings(&s);
                super::rounds::result_view(ui, &s, &f);
            }
            TestKind::Clicks => {
                let f = s.clicks.as_ref().map(findings::clicks).unwrap_or_default();
                super::clicks::result_view(ui, &s, &f);
            }
        }
    } else {
        muted(ui, "Click a run's name to open it, or tick 2+ runs to compare.");
    }
}

impl Session {
    fn file_key(&self) -> String {
        self.file.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| self.id.clone())
    }
}

/// One comparison row: name, values, whether higher is better (None = neutral).
struct Row {
    name: &'static str,
    vals: Vec<f64>,
    unit: &'static str,
    decimals: usize,
    higher_better: Option<bool>,
}

fn compare(ui: &mut Ui, runs: &[Session]) {
    let kind = runs[0].kind;
    section(ui, "Comparison");
    if runs.iter().any(|r| r.kind != kind) {
        let mixed_motion = runs.iter().all(|r| matches!(r.kind, TestKind::Polling | TestKind::Swipe));
        if !mixed_motion {
            muted(ui, "Pick runs of the same test type to compare.");
            return;
        }
    }

    let mut rows: Vec<Row> = Vec::new();
    match kind {
        TestKind::Polling | TestKind::Swipe => {
            let m: Vec<_> = runs.iter().map(|r| r.motion.clone().unwrap_or_default()).collect();
            let col = |f: fn(&crate::analysis::MotionReport) -> f64| m.iter().map(f).collect::<Vec<_>>();
            rows.push(Row {
                name: "Polling rate",
                vals: col(|r| r.nominal_hz),
                unit: "Hz",
                decimals: 0,
                higher_better: None,
            });
            rows.push(Row {
                name: "Rate held while fast",
                vals: col(|r| r.rate_held_pct),
                unit: "%",
                decimals: 1,
                higher_better: Some(true),
            });
            rows.push(Row {
                name: "Hitches per minute",
                vals: col(|r| r.hitches_per_min),
                unit: "",
                decimals: 1,
                higher_better: Some(false),
            });
            rows.push(Row {
                name: "Worst gap",
                vals: col(|r| r.max_gap_ms),
                unit: "ms",
                decimals: 1,
                higher_better: Some(false),
            });
            rows.push(Row {
                name: "  wireless/mouse delays",
                vals: col(|r| r.link_delays as f64),
                unit: "",
                decimals: 0,
                higher_better: Some(false),
            });
            rows.push(Row {
                name: "  PC-side stalls",
                vals: col(|r| r.pc_stalls as f64),
                unit: "",
                decimals: 0,
                higher_better: Some(false),
            });
            rows.push(Row {
                name: "  lost motion",
                vals: col(|r| r.lost as f64),
                unit: "",
                decimals: 0,
                higher_better: Some(false),
            });
            rows.push(Row {
                name: "Timing jitter p99",
                vals: col(|r| r.jitter_p99_us),
                unit: "µs",
                decimals: 0,
                higher_better: Some(false),
            });
            rows.push(Row {
                name: "Sensor reversals /10k",
                vals: col(|r| r.reversals_per_10k),
                unit: "",
                decimals: 1,
                higher_better: Some(false),
            });
            rows.push(Row {
                name: "Sensor spikes /10k",
                vals: col(|r| r.spikes_per_10k),
                unit: "",
                decimals: 1,
                higher_better: Some(false),
            });
            rows.push(Row {
                name: "Velocity noise",
                vals: col(|r| r.velocity_noise_pct),
                unit: "%",
                decimals: 1,
                higher_better: Some(false),
            });
            rows.push(Row {
                name: "Time fast enough",
                vals: col(|r| r.fast_time_pct),
                unit: "%",
                decimals: 0,
                higher_better: None,
            });
            rows.push(Row {
                name: "CPU load avg",
                vals: col(|r| r.cpu_avg as f64),
                unit: "%",
                decimals: 0,
                higher_better: None,
            });
            table(ui, runs, &rows);
            muted(
                ui,
                "Velocity noise and jitter are only comparable between runs with the same polling rate and DPI and a similar way of moving.",
            );

            let rate: Vec<Series> = m
                .iter()
                .enumerate()
                .map(|(i, r)| Series {
                    name: run_name(&runs[i], i),
                    points: &r.rate_series,
                    color: SERIES[i % SERIES.len()],
                })
                .collect();
            section(ui, "Reports per second");
            charts::rate_plot(ui, "cmp_rate", &rate, None, &[], 220.0);
            let hist: Vec<Series> = m
                .iter()
                .enumerate()
                .filter(|(_, r)| !r.interval_hist.is_empty())
                .map(|(i, r)| Series {
                    name: run_name(&runs[i], i),
                    points: &r.interval_hist,
                    color: SERIES[i % SERIES.len()],
                })
                .collect();
            section(ui, "Interval distribution");
            charts::hist_plot(ui, "cmp_hist", &hist, None, 200.0);
        }
        TestKind::Accuracy => {
            let a: Vec<_> = runs.iter().map(|r| r.accuracy.clone().unwrap_or_default()).collect();
            rows.push(Row {
                name: "Average error",
                vals: a.iter().map(|x| x.mean_err_pct).collect(),
                unit: "%",
                decimals: 2,
                higher_better: Some(false),
            });
            rows.push(Row {
                name: "Average miss",
                vals: a.iter().map(|x| x.mean_err_counts).collect(),
                unit: "counts",
                decimals: 1,
                higher_better: Some(false),
            });
            rows.push(Row {
                name: "Worst round",
                vals: a.iter().map(|x| x.max_err_counts).collect(),
                unit: "counts",
                decimals: 1,
                higher_better: Some(false),
            });
            rows.push(Row {
                name: "Rounds",
                vals: a.iter().map(|x| x.rounds.len() as f64).collect(),
                unit: "",
                decimals: 0,
                higher_better: None,
            });
            table(ui, runs, &rows);
        }
        TestKind::Dpi => {
            let d: Vec<_> = runs.iter().map(|r| r.dpi.clone().unwrap_or_default()).collect();
            rows.push(Row {
                name: "Measured DPI",
                vals: d.iter().map(|x| x.mean_dpi).collect(),
                unit: "",
                decimals: 0,
                higher_better: None,
            });
            rows.push(Row {
                name: "Spread between rounds",
                vals: d.iter().map(|x| x.spread_pct).collect(),
                unit: "%",
                decimals: 1,
                higher_better: Some(false),
            });
            table(ui, runs, &rows);
        }
        TestKind::Clicks => {
            let c: Vec<_> = runs.iter().map(|r| r.clicks.clone().unwrap_or_default()).collect();
            rows.push(Row {
                name: "Double-click faults",
                vals: c.iter().map(|x| x.buttons.iter().map(|b| b.chatter as f64).sum()).collect(),
                unit: "",
                decimals: 0,
                higher_better: Some(false),
            });
            rows.push(Row {
                name: "Wheel wrong-way",
                vals: c.iter().map(|x| x.wheel.wrong_way as f64).collect(),
                unit: "",
                decimals: 0,
                higher_better: Some(false),
            });
            rows.push(Row {
                name: "Total presses",
                vals: c.iter().map(|x| x.buttons.iter().map(|b| b.presses as f64).sum()).collect(),
                unit: "",
                decimals: 0,
                higher_better: None,
            });
            table(ui, runs, &rows);
        }
    }
}

fn run_name(s: &Session, i: usize) -> String {
    let l = if s.setup.label.is_empty() { format!("run {}", i + 1) } else { s.setup.label.clone() };
    format!("{l} ({})", &s.created[5..16.min(s.created.len())])
}

fn table(ui: &mut Ui, runs: &[Session], rows: &[Row]) {
    card(ui, |ui| {
        egui::Grid::new("cmp_table").striped(true).num_columns(runs.len() + 1).min_col_width(110.0).show(ui, |ui| {
            ui.label("");
            for (i, r) in runs.iter().enumerate() {
                ui.vertical(|ui| {
                    ui.label(RichText::new(run_name(r, i)).strong().color(SERIES[i % SERIES.len()]));
                    ui.label(RichText::new(super::motion::setup_line(r)).small().color(MUTED));
                });
            }
            ui.end_row();
            for row in rows {
                tip_label(ui, row.name, MUTED, false);
                let finite: Vec<f64> = row.vals.iter().copied().filter(|v| v.is_finite()).collect();
                let best = match row.higher_better {
                    Some(true) => finite.iter().copied().fold(f64::MIN, f64::max),
                    Some(false) => finite.iter().copied().fold(f64::MAX, f64::min),
                    None => f64::NAN,
                };
                let all_same = finite.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-9);
                for (i, v) in row.vals.iter().enumerate() {
                    let mut txt = format!("{} {}", fmt(*v, row.decimals), row.unit);
                    if i > 0 && v.is_finite() && row.vals[0].is_finite() && row.higher_better.is_some() {
                        let d = v - row.vals[0];
                        if d.abs() > 1e-9 {
                            txt.push_str(&format!("  ({}{})", if d > 0.0 { "+" } else { "" }, fmt(d, row.decimals)));
                        }
                    }
                    let c: Color32 = if !all_same && v.is_finite() && (*v - best).abs() < 1e-9 { GOOD } else { TEXT };
                    ui.label(RichText::new(txt).color(c));
                }
                ui.end_row();
            }
        });
        muted(ui, "Green = best value in the row. Differences are relative to the first (leftmost) run.");
    });
}
