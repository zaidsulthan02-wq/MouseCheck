//! Live view: what each mouse is sending right now. No saving; for poking around.

use std::time::Instant;

use egui::{Color32, RichText, Stroke, Ui, pos2, vec2};
use egui_plot::{HLine, Line, Plot, Points};

use super::App;
use super::widgets::*;
use crate::analysis::{self, MotionReport, Sample, cps_to_cm_s};

#[derive(Default)]
pub struct LiveUi {
    last_analysis: Option<Instant>,
    report: Option<MotionReport>,
}

pub fn page(app: &mut App, ui: &mut Ui) {
    title(ui, "Live view", "Move the mouse. Every raw report is timestamped the moment Windows hands it over.");

    let freq = app.cap.freq;
    let now = app.cap.now();
    let five = freq * 5;

    // Per-device report rate over the last second + copy of the last 5 s.
    let (devs, evs): (Vec<(u32, String, usize, usize)>, Vec<crate::capture::Ev>) = {
        let s = app.cap.store.lock().unwrap();
        let mut per: Vec<(usize, usize)> = vec![(0, 0); s.devices.len()];
        for e in s.recent.iter().rev() {
            if e.t < now - freq {
                break;
            }
            if let Some(p) = per.get_mut(e.dev as usize) {
                p.0 += 1;
                p.1 += e.moved() as usize;
            }
        }
        let devs =
            s.devices.iter().enumerate().map(|(i, d)| (i as u32, d.display_name(), per[i].0, per[i].1)).collect();
        let evs = s.recent.iter().filter(|e| e.t >= now - five).copied().collect();
        (devs, evs)
    };

    let dev = app.device.or_else(|| devs.iter().filter(|d| d.3 > 0).max_by_key(|d| d.3).map(|d| d.0));

    card(ui, |ui| {
        egui::Grid::new("live_devs").striped(true).num_columns(3).show(ui, |ui| {
            ui.label(RichText::new("Mouse device").strong());
            ui.label(RichText::new("reports in last 1 s").strong());
            ui.label(RichText::new("").strong());
            ui.end_row();
            for (i, name, n, _) in &devs {
                let active = Some(*i) == dev;
                ui.label(RichText::new(name).color(if active { ACCENT } else { TEXT }));
                ui.label(format!("{n}"));
                ui.label(if active { RichText::new("analysing").small().color(ACCENT) } else { RichText::new("") });
                ui.end_row();
            }
        });
        muted(ui, "Receivers for keyboards often show up as a 'mouse' too. Only the one that moves matters.");
    });

    let Some(dev) = dev else {
        card(ui, |ui| {
            ui.label(RichText::new("Move your mouse to start").size(22.0).color(MUTED));
        });
        return;
    };

    let samples: Vec<Sample> = evs
        .iter()
        .filter(|e| e.dev == dev)
        .map(|e| Sample { t: app.cap.ticks_to_us(e.t - (now - five)), dx: e.dx, dy: e.dy, bf: e.bflags, bd: e.bdata })
        .collect();

    // Re-run the full analysis on the last 5 s a few times per second.
    if app.live.last_analysis.is_none_or(|t| t.elapsed().as_millis() > 300) {
        app.live.last_analysis = Some(Instant::now());
        let r = analysis::analyze_motion(&samples, app.dpi());
        app.live.report = r.ok.then_some(r);
    }

    let last_s: Vec<&Sample> = samples.iter().filter(|s| s.t >= 4e6 && s.moved()).collect();
    let rate_1s = last_s.len();
    let max_gap = last_s.windows(2).map(|w| w[1].t - w[0].t).filter(|&g| g < 50_000.0).fold(0.0, f64::max);
    let speed: f64 = samples
        .iter()
        .filter(|s| s.t >= 4.75e6)
        .map(|s| ((s.dx as f64).powi(2) + (s.dy as f64).powi(2)).sqrt())
        .fold(0.0, |a, b| a + b)
        * 4.0;

    {
        let mut tg = TileGrid::default();
        tg.add("Motion reports, last 1 s", &format!("{rate_1s}"), "= reports per second", ACCENT);
        if let Some(r) = &app.live.report {
            tg.add("Detected polling rate", &format!("{:.0} Hz", r.nominal_hz), "from the last 5 s", TEXT);
            tg.add(
                "Hitches (last 5 s)",
                &format!("{}", r.hitches.len()),
                "gaps ≥ 2 ms while moving",
                if r.hitches.is_empty() { GOOD } else { WARN },
            );
        } else {
            tg.add("Detected polling rate", "–", "keep moving", MUTED);
        }
        tg.add("Longest gap, last 1 s", &format!("{:.2} ms", max_gap / 1000.0), "between moves", TEXT);
        let sp = match app.dpi() {
            Some(d) => format!("{:.0} cm/s", cps_to_cm_s(speed, d)),
            None => format!("{speed:.0} c/s"),
        };
        tg.add("Speed", &sp, "last 250 ms", TEXT);
        tg.show(ui);
    }

    ui.columns(2, |cols| {
        let ui = &mut cols[0];
        section(ui, "Interval between reports (last 1 s)");
        let pts: Vec<[f64; 2]> =
            last_s.windows(2).map(|w| [w[1].t / 1e6 - 4.0, (w[1].t - w[0].t).min(5000.0)]).collect();
        let nominal = app.live.report.as_ref().map(|r| 1e6 / r.nominal_hz);
        Plot::new("live_intervals")
            .height(240.0)
            .allow_scroll(false)
            .allow_drag(false)
            .include_y(0.0)
            .include_y(nominal.map(|n| n * 3.0).unwrap_or(2000.0))
            .include_x(0.0)
            .include_x(1.0)
            .x_axis_label("seconds")
            .y_axis_label("µs")
            .show(ui, |p| {
                if let Some(n) = nominal {
                    p.hline(HLine::new("ideal", n).color(MUTED));
                }
                p.points(Points::new("intervals", pts).radius(1.5).color(ACCENT));
            });
        muted(ui, "Healthy: a tight band at the ideal line. Dots far above it are late or missing reports.");

        let ui = &mut cols[1];
        section(ui, "Sensor path (last 0.5 s)");
        trail(ui, &samples);
    });

    if let Some(r) = &app.live.report {
        section(ui, "Reports per second (last 5 s)");
        Plot::new("live_rate")
            .height(160.0)
            .allow_scroll(false)
            .allow_drag(false)
            .include_y(0.0)
            .include_y(r.nominal_hz * 1.15)
            .include_x(0.0)
            .include_x(5.0)
            .show(ui, |p| {
                p.hline(HLine::new("polling rate", r.nominal_hz).color(MUTED));
                p.line(Line::new("reports/s", r.rate_series.clone()).color(ACCENT));
            });
    }
}

/// Draws the raw sensor path, auto-scaled, newest segment brightest.
fn trail(ui: &mut Ui, samples: &[Sample]) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 240.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 8.0, CARD);
    let recent: Vec<&Sample> = samples.iter().filter(|s| s.t >= 4.5e6 && s.moved()).collect();
    if recent.len() < 2 {
        p.text(rect.center(), egui::Align2::CENTER_CENTER, "move the mouse", egui::FontId::proportional(14.0), MUTED);
        return;
    }
    let mut pos = vec![(0.0f64, 0.0f64)];
    for s in &recent {
        let (x, y) = *pos.last().unwrap();
        pos.push((x + s.dx as f64, y + s.dy as f64));
    }
    let (minx, maxx) = pos.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.0), a.1.max(p.0)));
    let (miny, maxy) = pos.iter().fold((f64::MAX, f64::MIN), |a, p| (a.0.min(p.1), a.1.max(p.1)));
    let span = (maxx - minx).max(maxy - miny).max(1.0);
    let scale = (rect.height().min(rect.width()) as f64 - 30.0) / span;
    let cx = (minx + maxx) / 2.0;
    let cy = (miny + maxy) / 2.0;
    let to = |q: (f64, f64)| {
        pos2(rect.center().x + ((q.0 - cx) * scale) as f32, rect.center().y + ((q.1 - cy) * scale) as f32)
    };
    let n = pos.len();
    for i in 1..n {
        let a = (i as f32 / n as f32 * 255.0) as u8;
        p.line_segment(
            [to(pos[i - 1]), to(pos[i])],
            Stroke::new(2.0, Color32::from_rgba_unmultiplied(56, 189, 248, a)),
        );
    }
    p.circle_filled(to(pos[n - 1]), 4.0, ACCENT);
    p.text(
        rect.left_bottom() + vec2(8.0, -8.0),
        egui::Align2::LEFT_BOTTOM,
        format!("{span:.0} counts across"),
        egui::FontId::proportional(12.0),
        MUTED,
    );
}
