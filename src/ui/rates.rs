//! Best polling rate: a guided sweep over polling rates, then an honest verdict on what
//! each step up actually gains and costs on *this* mouse + PC.

use egui::{Color32, RichText, Ui};

use super::App;
use super::charts;
use super::motion::{self, MotionTest, Phase, Slot};
use super::widgets::*;
use crate::findings::{Finding, Severity};
use crate::session::{Session, Setup, TestKind};

pub const RATE_CHOICES: [f64; 5] = [500.0, 1000.0, 2000.0, 4000.0, 8000.0];

pub struct RatesUi {
    pub test: MotionTest,
    pub enabled: [bool; 5],
    pub runs_per_rate: usize,
    /// Planned steps of the active sweep (one entry per run).
    plan: Vec<f64>,
    step: usize,
    sweep_label: Option<String>,
    /// Which runs to evaluate: a run label, or None for all polling runs.
    pub source: Option<String>,
    last_seen: Option<String>,
    note: Option<(Severity, String)>,
}

impl Default for RatesUi {
    fn default() -> Self {
        Self {
            test: MotionTest::new(10.0),
            enabled: [false, true, true, true, true],
            runs_per_rate: 2,
            plan: vec![],
            step: 0,
            sweep_label: None,
            source: None,
            last_seen: None,
            note: None,
        }
    }
}

impl RatesUi {
    fn active(&self) -> bool {
        self.sweep_label.is_some() && self.step < self.plan.len()
    }
}

fn rate_label(hz: f64) -> String {
    format!("{hz:.0} Hz")
}

/// Average delay added by waiting for the next poll: half a polling interval.
fn poll_delay_ms(hz: f64) -> f64 {
    0.5 * 1000.0 / hz
}

pub fn page(app: &mut App, ui: &mut Ui) {
    motion::tick(app, Slot::Rates);
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) && app.rates.test.busy() {
        motion::cancel(app, Slot::Rates);
    }
    consume_finished_run(app);

    title(
        ui,
        "Best polling rate",
        "Is a higher polling rate actually helping you, or is your mouse/PC paying for it with stutter? Test each rate, then get a verdict.",
    );

    if app.rates.test.busy() {
        let step = app.rates.step;
        let hz = app.rates.plan.get(step).copied().unwrap_or(0.0);
        ui.label(
            RichText::new(format!("Testing {} · step {} of {}", rate_label(hz), step + 1, app.rates.plan.len()))
                .color(MUTED),
        );
        motion::running_view(app, ui, Slot::Rates);
        return;
    }

    if app.rates.active() {
        step_card(app, ui);
    } else {
        setup_card(app, ui);
    }
    if let Some((sev, msg)) = &app.rates.note {
        finding_card(ui, &Finding { severity: *sev, title: msg.clone(), detail: String::new(), fixes: vec![] });
    }

    results(app, ui);
}

fn setup_card(app: &mut App, ui: &mut Ui) {
    card(ui, |ui| {
        ui.label(RichText::new("Run a polling-rate sweep").strong());
        ui.label("MouseCheck walks you through each rate: you switch the rate in your mouse's software, then do a 10-second fast-movement run. 2 runs per rate gives a trustworthy answer.");
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label("Rates to test:");
            for (i, hz) in RATE_CHOICES.iter().enumerate() {
                ui.checkbox(&mut app.rates.enabled[i], rate_label(*hz));
            }
            ui.separator();
            ui.label("Runs per rate");
            ui.add(egui::DragValue::new(&mut app.rates.runs_per_rate).range(1..=5));
            ui.label("Seconds per run");
            ui.add(egui::DragValue::new(&mut app.rates.test.duration_s).range(5.0..=60.0));
        });
        ui.add_space(4.0);
        let n: usize = app.rates.enabled.iter().filter(|e| **e).count();
        ui.horizontal(|ui| {
            let ok = n >= 2;
            ui.add_enabled_ui(ok, |ui| {
                if big_button(ui, "Start sweep", ACCENT).clicked() {
                    let label = format!("Rate sweep {}", chrono::Local::now().format("%d %b %H:%M"));
                    let r = &mut app.rates;
                    r.plan = RATE_CHOICES
                        .iter()
                        .zip(r.enabled)
                        .filter(|(_, e)| *e)
                        .flat_map(|(hz, _)| std::iter::repeat_n(*hz, r.runs_per_rate))
                        .collect();
                    r.step = 0;
                    r.sweep_label = Some(label.clone());
                    r.source = Some(label);
                    r.note = None;
                }
            });
            if !ok {
                muted(ui, "Pick at least two rates.");
            } else {
                muted(
                    ui,
                    format!(
                        "{} runs, about {:.0} minutes with switching.",
                        n * app.rates.runs_per_rate,
                        (n * app.rates.runs_per_rate) as f64 * 0.5 + 0.5
                    ),
                );
            }
        });
        muted(
            ui,
            "Already tested different rates on the Polling page? Pick those runs under 'Evaluate' below, no need to redo them.",
        );
    });
}

fn step_card(app: &mut App, ui: &mut Ui) {
    let r = &app.rates;
    let hz = r.plan[r.step];
    let run_no = r.plan[..r.step].iter().filter(|&&x| x == hz).count() + 1;
    let runs_here = r.plan.iter().filter(|&&x| x == hz).count();
    let first_of_rate = run_no == 1;
    let mut start = false;
    let mut skip = false;
    let mut end = false;
    card(ui, |ui| {
        // Progress chips
        ui.horizontal_wrapped(|ui| {
            for (i, p) in r.plan.iter().enumerate() {
                let (fill, txt) = if i < r.step {
                    (Color32::from_rgb(20, 83, 60), GOOD)
                } else if i == r.step {
                    (Color32::from_rgb(20, 83, 116), ACCENT)
                } else {
                    (CARD_HI, MUTED)
                };
                egui::Frame::new().fill(fill).corner_radius(6).inner_margin(egui::Margin::symmetric(8, 3)).show(
                    ui,
                    |ui| {
                        ui.label(RichText::new(format!("{:.0}", p)).small().color(txt));
                    },
                );
            }
        });
        ui.add_space(6.0);
        ui.label(RichText::new(format!("Step {} of {}", r.step + 1, r.plan.len())).color(MUTED));
        if first_of_rate {
            ui.label(
                RichText::new(format!("1. Open your mouse software and set the polling rate to {}", rate_label(hz)))
                    .size(20.0)
                    .strong()
                    .color(ACCENT),
            );
            ui.label(
                RichText::new("2. Come back here, press Start, and move in big fast circles / swipes until it stops.")
                    .size(16.0),
            );
        } else {
            ui.label(
                RichText::new(format!("Same rate again ({}), run {} of {}", rate_label(hz), run_no, runs_here))
                    .size(20.0)
                    .strong()
                    .color(ACCENT),
            );
            ui.label("Press Start and move in big fast circles / swipes until it stops.");
        }
        muted(ui, "MouseCheck checks the rate it actually receives, so if the setting didn't apply you'll be told.");
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            start = big_button(ui, "Start", ACCENT).clicked();
            ui.add_space(12.0);
            skip = ui.button(format!("Skip {}", rate_label(hz))).clicked();
            end = ui.button("End sweep").clicked();
        });
    });
    if start {
        let mut setup: Setup = app.current_setup();
        setup.label = app.rates.sweep_label.clone().unwrap_or_default();
        setup.polling_setting = rate_label(hz);
        app.rates.test.setup_override = Some(setup);
        app.rates.note = None;
        motion::start(app, Slot::Rates);
    } else if skip {
        let r = &mut app.rates;
        while r.step < r.plan.len() && r.plan[r.step] == hz {
            r.step += 1;
        }
    } else if end {
        app.rates.plan.truncate(app.rates.step);
    }
}

/// After a sweep run finishes: verify it, then advance (or ask to redo).
fn consume_finished_run(app: &mut App) {
    let r = &mut app.rates;
    if r.test.phase != Phase::Idle {
        return;
    }
    let Some(s) = &r.test.result else { return };
    if r.last_seen.as_deref() == Some(s.id.as_str()) {
        return;
    }
    r.last_seen = Some(s.id.clone());
    let Some(m) = &s.motion else { return };
    let planned = r.plan.get(r.step).copied().unwrap_or(0.0);
    if !m.ok {
        r.note = Some((
            Severity::Warn,
            "Not enough movement was recorded. Redo this step and keep moving fast the whole time.".into(),
        ));
    } else if m.foreground_pct < 99.0 {
        r.note = Some((
            Severity::Bad,
            "MouseCheck lost focus during the run, so the data isn't valid. Redo this step.".into(),
        ));
    } else if (m.nominal_hz - planned).abs() > 1.0 {
        r.note = Some((
            Severity::Warn,
            format!(
                "Your mouse sent {:.0} Hz, not {}. The setting probably didn't apply (some mice need a moment or a re-plug of the receiver). The run is saved under its real rate; set {} and redo this step.",
                m.nominal_hz,
                rate_label(planned),
                rate_label(planned)
            ),
        ));
    } else {
        r.note = Some((
            Severity::Good,
            format!(
                "{} run saved: {:.1} hitches/min, rate held {}%.",
                rate_label(planned),
                m.hitches_per_min,
                fmt(m.rate_held_pct, 1)
            ),
        ));
        r.step += 1;
        if r.step >= r.plan.len() {
            r.note = Some((Severity::Good, "Sweep complete. Your verdict is below.".into()));
        }
    }
}

// ---------------------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------------------

struct RateStats {
    hz: f64,
    runs: usize,
    motion_min: f64,
    hitches: usize,
    stalls: usize,
    links: usize,
    lost: usize,
    per_run_min: f64,
    per_run_max: f64,
    worst_gap: f64,
    held: f64,
    fast: f64,
    jitter_p99: f64,
    cpu_avg: f64,
    cpu_max: f64,
    valid: bool,
}

impl RateStats {
    fn per_min(&self) -> f64 {
        if self.motion_min > 0.0 { self.hitches as f64 / self.motion_min } else { f64::NAN }
    }
}

fn aggregate(runs: &[&Session]) -> Vec<RateStats> {
    let mut rates: Vec<f64> =
        runs.iter().filter_map(|s| s.motion.as_ref()).filter(|m| m.ok).map(|m| m.nominal_hz).collect();
    rates.sort_by(|a, b| a.partial_cmp(b).unwrap());
    rates.dedup();
    rates
        .into_iter()
        .map(|hz| {
            let ms: Vec<_> =
                runs.iter().filter_map(|s| s.motion.as_ref()).filter(|m| m.ok && m.nominal_hz == hz).collect();
            let n = ms.len() as f64;
            let mean = |f: &dyn Fn(&crate::analysis::MotionReport) -> f64| {
                let v: Vec<f64> = ms.iter().map(|m| f(m)).filter(|x| x.is_finite()).collect();
                if v.is_empty() { f64::NAN } else { v.iter().fold(0.0, |a, b| a + b) / v.len() as f64 }
            };
            let per_run: Vec<f64> = ms.iter().map(|m| m.hitches_per_min).collect();
            let motion_min = ms.iter().map(|m| m.motion_time_s).fold(0.0, |a, b| a + b) / 60.0;
            let fast = mean(&|m| m.fast_time_pct);
            let fg_ok = ms.iter().all(|m| m.foreground_pct >= 99.0);
            RateStats {
                hz,
                runs: ms.len(),
                motion_min,
                hitches: ms.iter().map(|m| m.hitches.len()).sum(),
                stalls: ms.iter().map(|m| m.pc_stalls).sum(),
                links: ms.iter().map(|m| m.link_delays).sum(),
                lost: ms.iter().map(|m| m.lost).sum(),
                per_run_min: per_run.iter().cloned().fold(f64::MAX, f64::min),
                per_run_max: per_run.iter().cloned().fold(0.0, f64::max),
                worst_gap: ms.iter().map(|m| m.max_gap_ms).fold(0.0, f64::max),
                held: mean(&|m| m.rate_held_pct),
                fast,
                jitter_p99: mean(&|m| m.jitter_p99_us),
                cpu_avg: mean(&|m| m.cpu_avg as f64),
                cpu_max: ms.iter().map(|m| m.cpu_max as f64).fold(0.0, f64::max),
                valid: n > 0.0 && motion_min * 60.0 >= 5.0 && fast >= 30.0 && fg_ok,
            }
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Recommended,
    Stable,
    Worse,
    NoData,
}

/// Hitch rate of `a` is meaningfully worse than `b`: beyond 2 standard errors (Poisson)
/// and by at least one hitch per minute, so a single unlucky hitch doesn't decide it.
fn significantly_worse(a: &RateStats, b: &RateStats) -> bool {
    let (la, lb) = (a.per_min(), b.per_min());
    let se = ((a.hitches as f64).max(1.0) / a.motion_min.powi(2) + (b.hitches as f64).max(1.0) / b.motion_min.powi(2))
        .sqrt();
    la - lb > (2.0 * se).max(1.0)
}

fn evaluate(stats: &[RateStats]) -> (Vec<Status>, Option<usize>) {
    let valid: Vec<usize> = (0..stats.len()).filter(|&i| stats[i].valid).collect();
    let mut status = vec![Status::NoData; stats.len()];
    if valid.is_empty() {
        return (status, None);
    }
    let best = *valid.iter().min_by(|&&a, &&b| stats[a].per_min().partial_cmp(&stats[b].per_min()).unwrap()).unwrap();
    for &i in &valid {
        let s = &stats[i];
        let ok = !significantly_worse(s, &stats[best]) && s.held >= 95.0 && s.cpu_max < 95.0;
        status[i] = if ok { Status::Stable } else { Status::Worse };
    }
    let rec = valid.iter().rev().find(|&&i| status[i] == Status::Stable).copied();
    if let Some(r) = rec {
        status[r] = Status::Recommended;
    }
    (status, rec)
}

fn results(app: &mut App, ui: &mut Ui) {
    // Sources: labels of polling runs, newest first.
    let mut labels: Vec<String> = Vec::new();
    for s in app.sessions.iter().filter(|s| s.kind == TestKind::Polling) {
        if !labels.contains(&s.setup.label) {
            labels.push(s.setup.label.clone());
        }
    }
    section(ui, "Evaluate");
    ui.horizontal(|ui| {
        ui.label("Use runs:");
        let txt = app.rates.source.clone().unwrap_or_else(|| "All polling runs".into());
        egui::ComboBox::from_id_salt("rate_source").selected_text(txt).width(260.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut app.rates.source, None, "All polling runs");
            for l in &labels {
                let name = if l.is_empty() { "untitled".to_string() } else { l.clone() };
                ui.selectable_value(&mut app.rates.source, Some(l.clone()), name);
            }
        });
        if app.rates.source.is_none() {
            muted(ui, "Mixing runs from different days or conditions muddies the answer. Prefer one sweep.");
        }
    });

    let runs: Vec<&Session> = app
        .sessions
        .iter()
        .filter(|s| s.kind == TestKind::Polling && app.rates.source.as_ref().is_none_or(|l| &s.setup.label == l))
        .collect();
    let stats = aggregate(&runs);
    if stats.len() < 2 {
        card(ui, |ui| {
            muted(ui, "Test at least two different polling rates to get a verdict.");
        });
        return;
    }
    let (status, rec) = evaluate(&stats);
    let devices: std::collections::BTreeSet<&str> = runs.iter().map(|s| s.device.as_str()).collect();

    verdict(ui, &stats, &status, rec, devices.len());
    table(ui, &stats, &status);

    section(ui, "Hitches per minute by rate");
    let labels: Vec<String> = stats.iter().map(|s| format!("{:.0}", s.hz)).collect();
    let colors: Vec<Color32> = status
        .iter()
        .map(|s| match s {
            Status::Recommended => GOOD,
            Status::Stable => ACCENT,
            Status::Worse => BAD,
            Status::NoData => MUTED,
        })
        .collect();
    let hpm: Vec<f64> = stats.iter().map(|s| s.per_min()).collect();
    if hpm.iter().all(|v| *v == 0.0) {
        card(ui, |ui| {
            ui.label(RichText::new("0 hitches at every rate tested").strong().color(GOOD));
            muted(ui, "No gap of 2 ms or more while you were moving, at any rate.");
        });
    } else {
        charts::category_bars(ui, "rates_hitches", &labels, &hpm, &colors, "hitches / min", 180.0);
    }
    section(ui, "CPU load by rate");
    let cpu: Vec<f64> = stats.iter().map(|s| s.cpu_avg).collect();
    charts::category_bars(ui, "rates_cpu", &labels, &cpu, &colors, "avg CPU %", 150.0);
}

fn verdict(ui: &mut Ui, stats: &[RateStats], status: &[Status], rec: Option<usize>, n_devices: usize) {
    let mut out: Vec<Finding> = Vec::new();
    let f =
        |severity, title: String, detail: String, fixes: Vec<&'static str>| Finding { severity, title, detail, fixes };

    match rec {
        Some(r) => {
            let s = &stats[r];
            let top = stats.iter().rposition(|x| x.valid).unwrap_or(r);
            let all_clean = (0..stats.len()).filter(|&i| stats[i].valid).all(|i| status[i] != Status::Worse);
            let detail = if all_clean && r == top {
                format!(
                    "Every rate you tested was delivered equally cleanly ({:.1} hitches/min at {:.0} Hz, rate held {:.1}%). Your mouse and PC handle {:.0} Hz with no measurable stability cost.",
                    s.per_min(),
                    s.hz,
                    s.held,
                    s.hz
                )
            } else {
                format!(
                    "The highest rate that's as stable as your most stable rate: {:.1} hitches/min, rate held {:.1}%, CPU {:.0}% avg.",
                    s.per_min(),
                    s.held,
                    s.cpu_avg
                )
            };
            out.push(f(Severity::Good, format!("Use {:.0} Hz", s.hz), detail, vec![]));

            // What the step up gains
            if let Some(lower) = (0..r).rev().find(|&i| stats[i].valid) {
                let (a, b) = (poll_delay_ms(stats[lower].hz), poll_delay_ms(s.hz));
                out.push(f(
                    Severity::Info,
                    format!("What {:.0} Hz gains over {:.0} Hz", s.hz, stats[lower].hz),
                    format!(
                        "Average wait for the next poll drops from {a:.3} ms to {b:.3} ms ({:.3} ms less), and the cursor path gets finer. Above 1000 Hz these are fractions of a millisecond: real, but small. The bigger visible benefit is smoother motion on high-refresh (240 Hz+) monitors.",
                        a - b
                    ),
                    vec![],
                ));
            }
        }
        None => out.push(f(
            Severity::Warn,
            "No clear winner yet".into(),
            "None of the tested rates has enough valid data. Each needs at least 5 s of fast movement with MouseCheck in front.".into(),
            vec![],
        )),
    }

    // What higher rates cost
    for (i, s) in stats.iter().enumerate() {
        if status[i] != Status::Worse {
            continue;
        }
        let mut why = Vec::new();
        if let Some(r) = rec {
            let d = s.per_min() - stats[r].per_min();
            if d > 0.0 {
                why.push(format!("+{:.1} hitches/min vs {:.0} Hz", d, stats[r].hz));
            }
        }
        if s.held < 95.0 {
            why.push(format!("rate only held {:.1}% of the time", s.held));
        }
        if s.cpu_max >= 95.0 {
            why.push(format!("CPU peaked at {:.0}%", s.cpu_max));
        }
        let cause = if s.stalls >= s.links && s.stalls >= s.lost && s.stalls > 0 {
            " Mostly PC-side stalls: your USB/driver/CPU can't keep up with this rate."
        } else if s.links > s.lost && s.links > 0 {
            " Mostly wireless delays: the radio link struggles at this rate (interference, distance, battery)."
        } else if s.lost > 0 {
            " Mostly lost motion."
        } else {
            ""
        };
        out.push(f(
            Severity::Warn,
            format!("{:.0} Hz costs you more than it gives", s.hz),
            format!("{}.{}", why.join(", "), cause),
            vec![],
        ));
    }

    // Data quality
    let thin: Vec<String> = stats.iter().filter(|s| s.valid && s.runs < 2).map(|s| format!("{:.0}", s.hz)).collect();
    if !thin.is_empty() {
        out.push(f(
            Severity::Info,
            "Confidence: low".into(),
            format!("Only one run at {} Hz. Hitches come and go, so do 2–3 runs per rate before trusting small differences.", thin.join(" / ")),
            vec![],
        ));
    }
    for s in stats.iter().filter(|s| !s.valid && s.runs > 0) {
        out.push(f(
            Severity::Info,
            format!("{:.0} Hz not judged", s.hz),
            format!("You were fast enough for this rate only {:.0}% of the time (or the window lost focus). Move faster on this rate's runs.", s.fast),
            vec![],
        ));
    }
    if n_devices > 1 {
        out.push(f(
            Severity::Warn,
            "Runs from different mice mixed".into(),
            "Pick a source with runs from one mouse only.".into(),
            vec![],
        ));
    }
    out.push(f(
        Severity::Info,
        "Not measured here".into(),
        "Battery: on wireless mice, higher polling rates drain the battery much faster (often several times). Games: some engines can't process 4000–8000 Hz input smoothly even when delivery is clean. If a game feels worse at a high rate, step down for that game.".into(),
        vec![],
    ));
    findings(ui, &out);
}

fn table(ui: &mut Ui, stats: &[RateStats], status: &[Status]) {
    card(ui, |ui| {
        egui::Grid::new("rates_table").striped(true).num_columns(stats.len() + 1).min_col_width(105.0).show(ui, |ui| {
            ui.label("");
            for (i, s) in stats.iter().enumerate() {
                let (txt, c) = match status[i] {
                    Status::Recommended => ("recommended", GOOD),
                    Status::Stable => ("stable", ACCENT),
                    Status::Worse => ("worse", BAD),
                    Status::NoData => ("not enough data", MUTED),
                };
                ui.vertical(|ui| {
                    ui.label(RichText::new(rate_label(s.hz)).strong().size(17.0));
                    tip_text(ui, RichText::new(txt).small().color(c), txt);
                });
            }
            ui.end_row();
            let row = |ui: &mut Ui, name: &str, vals: Vec<String>| {
                tip_label(ui, name, MUTED, false);
                for v in vals {
                    ui.label(v);
                }
                ui.end_row();
            };
            row(
                ui,
                "Runs",
                stats.iter().map(|s| format!("{} ({:.0} s moving)", s.runs, s.motion_min * 60.0)).collect(),
            );
            row(ui, "Rate held while fast", stats.iter().map(|s| format!("{}%", fmt(s.held, 1))).collect());
            row(
                ui,
                "Hitches / min",
                stats
                    .iter()
                    .map(|s| {
                        if s.runs > 1 {
                            format!("{} (runs {:.0}–{:.0})", fmt(s.per_min(), 1), s.per_run_min, s.per_run_max)
                        } else {
                            fmt(s.per_min(), 1)
                        }
                    })
                    .collect(),
            );
            row(
                ui,
                "  PC / wireless / lost",
                stats.iter().map(|s| format!("{} / {} / {}", s.stalls, s.links, s.lost)).collect(),
            );
            row(ui, "Worst gap", stats.iter().map(|s| format!("{} ms", fmt(s.worst_gap, 1))).collect());
            row(ui, "Timing jitter p99", stats.iter().map(|s| format!("{} µs", fmt(s.jitter_p99, 0))).collect());
            row(ui, "CPU avg / peak", stats.iter().map(|s| format!("{:.0}% / {:.0}%", s.cpu_avg, s.cpu_max)).collect());
            row(ui, "Fast enough", stats.iter().map(|s| format!("{:.0}% of time", s.fast)).collect());
            row(ui, "Avg polling delay", stats.iter().map(|s| format!("{:.3} ms", poll_delay_ms(s.hz))).collect());
        });
        muted(
            ui,
            "Avg polling delay is theory: half a polling interval, the average wait before the mouse is asked for new data.",
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(hz: f64, runs: usize, hitches: usize, minutes: f64, held: f64) -> RateStats {
        RateStats {
            hz,
            runs,
            motion_min: minutes,
            hitches,
            stalls: hitches,
            links: 0,
            lost: 0,
            per_run_min: 0.0,
            per_run_max: 0.0,
            worst_gap: 0.0,
            held,
            fast: 90.0,
            jitter_p99: 0.0,
            cpu_avg: 5.0,
            cpu_max: 20.0,
            valid: true,
        }
    }

    #[test]
    fn all_clean_recommends_highest() {
        let s = vec![st(1000.0, 2, 0, 0.33, 100.0), st(4000.0, 2, 0, 0.33, 100.0), st(8000.0, 2, 1, 0.33, 99.8)];
        let (status, rec) = evaluate(&s);
        assert_eq!(rec, Some(2), "one stray hitch must not disqualify 8000 Hz");
        assert!(status[..2].iter().all(|x| *x == Status::Stable));
    }

    #[test]
    fn unstable_top_rate_steps_down() {
        let s = vec![st(1000.0, 2, 0, 0.33, 100.0), st(4000.0, 2, 1, 0.33, 99.9), st(8000.0, 2, 30, 0.33, 90.0)];
        let (status, rec) = evaluate(&s);
        assert_eq!(rec, Some(1));
        assert!(status[2] == Status::Worse);
    }
}
