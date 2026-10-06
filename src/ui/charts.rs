//! Plot helpers built on egui_plot.

use egui::{Color32, Ui};
use egui_plot::{Bar, BarChart, HLine, Legend, Line, Plot, VLine};

use super::widgets::{BAD, MUTED, WARN};
use crate::analysis::{Hitch, HitchKind};

pub struct Series<'a> {
    pub name: String,
    pub points: &'a [[f64; 2]],
    pub color: Color32,
}

fn hitch_color(k: HitchKind) -> Color32 {
    match k {
        HitchKind::PcStall => Color32::from_rgb(167, 139, 250),
        HitchKind::LinkDelay => WARN,
        HitchKind::Lost => BAD,
    }
}

/// Reports-per-second over time (20 ms windows), with nominal line and hitch markers.
pub fn rate_plot(ui: &mut Ui, id: &str, series: &[Series], nominal: Option<f64>, hitches: &[Hitch], height: f32) {
    let max_y = series.iter().flat_map(|s| s.points.iter().map(|p| p[1])).fold(nominal.unwrap_or(0.0), f64::max) * 1.15;
    Plot::new(id)
        .height(height)
        .legend(Legend::default())
        .allow_scroll(false)
        .include_y(0.0)
        .include_y(max_y.max(10.0))
        .x_axis_label("seconds")
        .y_axis_label("reports / s")
        .show(ui, |p| {
            if let Some(n) = nominal {
                p.hline(HLine::new("polling rate", n).color(MUTED).style(egui_plot::LineStyle::dashed_loose()));
            }
            for h in hitches.iter().take(300) {
                p.vline(VLine::new(h.kind.label(), h.t_s).color(hitch_color(h.kind)).width(1.0));
            }
            for s in series {
                p.line(Line::new(s.name.clone(), s.points.to_vec()).color(s.color).width(1.5));
            }
        });
}

/// Movement speed over time, with the speed needed to use the full polling rate.
pub fn speed_plot(ui: &mut Ui, id: &str, series: &[Series], needed: Option<f64>, unit: &str, height: f32) {
    Plot::new(id)
        .height(height)
        .legend(Legend::default())
        .allow_scroll(false)
        .include_y(0.0)
        .x_axis_label("seconds")
        .y_axis_label(unit.to_string())
        .show(ui, |p| {
            if let Some(n) = needed {
                p.hline(HLine::new("needed for full rate", n).color(WARN).style(egui_plot::LineStyle::dashed_loose()));
            }
            for s in series {
                p.line(
                    Line::new(s.name.clone(), s.points.to_vec()).color(s.color).width(1.5).fill(0.0).fill_alpha(0.12),
                );
            }
        });
}

/// Interval histogram (percent of intervals per bin).
pub fn hist_plot(ui: &mut Ui, id: &str, series: &[Series], nominal_us: Option<f64>, height: f32) {
    Plot::new(id)
        .height(height)
        .legend(Legend::default())
        .allow_scroll(false)
        .allow_drag(false)
        .include_y(0.0)
        .x_axis_label("interval between reports (µs)")
        .y_axis_label("% of intervals")
        .show(ui, |p| {
            if let Some(n) = nominal_us {
                p.vline(VLine::new("ideal interval", n).color(MUTED).style(egui_plot::LineStyle::dashed_loose()));
            }
            let single = series.len() == 1;
            for s in series {
                if single {
                    let w = if s.points.len() > 1 { (s.points[1][0] - s.points[0][0]) * 0.9 } else { 1.0 };
                    let bars = s.points.iter().map(|q| Bar::new(q[0], q[1]).width(w).fill(s.color)).collect();
                    p.bar_chart(BarChart::new(s.name.clone(), bars).color(s.color));
                } else {
                    p.line(Line::new(s.name.clone(), s.points.to_vec()).color(s.color).width(2.0));
                }
            }
        });
}

/// One bar per category (e.g. per polling rate), labelled on the x axis.
pub fn category_bars(
    ui: &mut Ui,
    id: &str,
    labels: &[String],
    values: &[f64],
    colors: &[Color32],
    y_label: &str,
    height: f32,
) {
    let names = labels.to_vec();
    Plot::new(id)
        .height(height)
        .allow_scroll(false)
        .allow_drag(false)
        .allow_zoom(false)
        .include_y(0.0)
        .include_y(values.iter().cloned().filter(|v| v.is_finite()).fold(1.0, f64::max) * 1.2)
        .y_axis_label(y_label.to_string())
        .x_axis_formatter(move |mark, _| {
            let i = mark.value.round();
            if (mark.value - i).abs() < 1e-6 && i >= 0.0 {
                names.get(i as usize).map(|n| format!("{n} Hz")).unwrap_or_default()
            } else {
                String::new()
            }
        })
        .show(ui, |p| {
            let bars = values
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    Bar::new(i as f64, if v.is_finite() { *v } else { 0.0 }).width(0.6).fill(colors[i]).name(&labels[i])
                })
                .collect();
            p.bar_chart(BarChart::new(y_label.to_string(), bars));
        });
}
