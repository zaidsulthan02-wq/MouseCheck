//! Theme and small reusable widgets.

use egui::{Color32, CornerRadius, FontId, Margin, RichText, Stroke, TextStyle, Ui, vec2};

use crate::findings::{Finding, Severity};

pub const BG: Color32 = Color32::from_rgb(12, 14, 19);
pub const PANEL: Color32 = Color32::from_rgb(18, 21, 28);
pub const CARD: Color32 = Color32::from_rgb(25, 29, 39);
pub const CARD_HI: Color32 = Color32::from_rgb(32, 37, 50);
pub const BORDER: Color32 = Color32::from_rgb(42, 48, 64);
pub const TEXT: Color32 = Color32::from_rgb(228, 233, 242);
pub const MUTED: Color32 = Color32::from_rgb(142, 152, 172);
pub const ACCENT: Color32 = Color32::from_rgb(56, 189, 248);
pub const GOOD: Color32 = Color32::from_rgb(52, 211, 153);
pub const WARN: Color32 = Color32::from_rgb(251, 191, 36);
pub const BAD: Color32 = Color32::from_rgb(248, 113, 113);
pub const INFO: Color32 = Color32::from_rgb(129, 140, 248);

/// Colours for overlaying several runs.
pub const SERIES: [Color32; 6] = [
    Color32::from_rgb(56, 189, 248),
    Color32::from_rgb(244, 114, 182),
    Color32::from_rgb(251, 191, 36),
    Color32::from_rgb(52, 211, 153),
    Color32::from_rgb(167, 139, 250),
    Color32::from_rgb(251, 146, 60),
];

pub fn apply_theme(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        s.text_styles.insert(TextStyle::Body, FontId::proportional(15.0));
        s.text_styles.insert(TextStyle::Button, FontId::proportional(15.0));
        s.text_styles.insert(TextStyle::Heading, FontId::proportional(26.0));
        s.text_styles.insert(TextStyle::Small, FontId::proportional(12.5));
        s.text_styles.insert(TextStyle::Monospace, FontId::monospace(14.0));
        s.spacing.item_spacing = vec2(8.0, 8.0);
        s.spacing.button_padding = vec2(12.0, 6.0);
        s.spacing.interact_size.y = 28.0;
        let v = &mut s.visuals;
        v.override_text_color = Some(TEXT);
        v.panel_fill = PANEL;
        v.window_fill = CARD;
        v.extreme_bg_color = BG;
        v.faint_bg_color = CARD_HI;
        v.hyperlink_color = ACCENT;
        v.selection.bg_fill = Color32::from_rgb(20, 83, 116);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(36, 42, 56);
        v.widgets.inactive.bg_fill = Color32::from_rgb(36, 42, 56);
        // Visible outline so checkboxes and inputs stand out on dark cards.
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(74, 84, 106));
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(46, 54, 72);
        v.widgets.hovered.bg_fill = Color32::from_rgb(46, 54, 72);
        v.widgets.active.weak_bg_fill = Color32::from_rgb(58, 68, 90);
        v.widgets.active.bg_fill = Color32::from_rgb(58, 68, 90);
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = CornerRadius::same(7);
        }
    });
}

pub fn severity_color(s: Severity) -> Color32 {
    match s {
        Severity::Bad => BAD,
        Severity::Warn => WARN,
        Severity::Info => INFO,
        Severity::Good => GOOD,
    }
}

pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(10)
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

pub fn title(ui: &mut Ui, text: &str, sub: &str) {
    ui.label(RichText::new(text).heading().strong());
    if !sub.is_empty() {
        ui.label(RichText::new(sub).color(MUTED));
    }
    ui.add_space(4.0);
}

pub fn section(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).size(17.0).strong());
}

pub fn muted(ui: &mut Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).color(MUTED));
}

const TILE_W: f32 = 178.0;

/// Collects metric tiles and lays them out in rows that fit the available width.
/// (egui can't wrap frames automatically because it doesn't know their size up front.)
#[derive(Default)]
pub struct TileGrid {
    items: Vec<(String, String, String, Color32)>,
}

impl TileGrid {
    pub fn add(&mut self, label: &str, value: &str, hint: &str, color: Color32) {
        self.items.push((label.into(), value.into(), hint.into(), color));
    }
    pub fn show(self, ui: &mut Ui) {
        let gap = ui.spacing().item_spacing.x;
        let per = ((ui.available_width() + gap) / (TILE_W + gap)).floor().max(1.0) as usize;
        for row in self.items.chunks(per) {
            ui.horizontal(|ui| {
                for (l, v, h, c) in row {
                    tile(ui, l, v, h, *c);
                }
            });
        }
    }
}

/// Small circled "i" drawn by hand (the ⓘ glyph isn't in egui's default font).
pub fn info_icon(ui: &mut Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(13.0, 13.0), egui::Sense::hover());
    let c = if resp.hovered() { ACCENT } else { MUTED };
    ui.painter().circle_stroke(rect.center(), 5.5, Stroke::new(1.0, c));
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "i", FontId::proportional(9.5), c);
    resp
}

/// Text followed by an info icon; the whole thing explains itself on hover if the glossary
/// knows the label.
pub fn tip_text(ui: &mut Ui, text: RichText, key: &str) -> egui::Response {
    let Some(tip) = super::explain::for_label(key) else { return ui.label(text) };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let r = ui.label(text);
        r | info_icon(ui)
    })
    .inner
    .on_hover_text(tip)
}

pub fn tip_label(ui: &mut Ui, text: &str, color: Color32, strong: bool) -> egui::Response {
    let mut rt = RichText::new(text).color(color);
    if strong {
        rt = rt.strong();
    }
    tip_text(ui, rt, text)
}

/// A metric tile: big value, small label, optional hint below.
pub fn tile(ui: &mut Ui, label: &str, value: &str, hint: &str, color: Color32) {
    let tip = super::explain::for_label(label);
    let resp = egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(10)
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(TILE_W - 30.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(RichText::new(label).small().color(MUTED));
                    if tip.is_some() {
                        info_icon(ui);
                    }
                });
                ui.label(RichText::new(value).size(24.0).strong().color(color));
                if !hint.is_empty() {
                    ui.label(RichText::new(hint).small().color(MUTED));
                }
            });
        })
        .response;
    if let Some(t) = tip {
        resp.on_hover_text(t);
    }
}

pub fn finding_card(ui: &mut Ui, f: &Finding) {
    let c = severity_color(f.severity);
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(10)
        .inner_margin(Margin { left: 0, right: 14, top: 10, bottom: 10 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(4.0, 22.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 2.0, c);
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    let tag = match f.severity {
                        Severity::Bad => "PROBLEM",
                        Severity::Warn => "CHECK",
                        Severity::Info => "INFO",
                        Severity::Good => "OK",
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(tag).small().strong().color(c));
                        ui.label(RichText::new(&f.title).strong());
                    });
                    ui.label(RichText::new(&f.detail).color(MUTED));
                    if !f.fixes.is_empty() {
                        ui.add_space(2.0);
                        ui.label(RichText::new("Try this:").small().strong());
                        for (i, fix) in f.fixes.iter().enumerate() {
                            ui.label(format!("{}. {fix}", i + 1));
                        }
                    }
                });
            });
        });
}

pub fn findings(ui: &mut Ui, list: &[Finding]) {
    for f in list {
        finding_card(ui, f);
    }
}

pub fn fmt(v: f64, decimals: usize) -> String {
    if v.is_finite() { format!("{v:.decimals$}") } else { "–".into() }
}

/// Colour for a value given good/bad thresholds (works for both directions).
pub fn grade(v: f64, good: f64, bad: f64) -> Color32 {
    if !v.is_finite() {
        return MUTED;
    }
    let higher_is_better = good > bad;
    let (is_good, is_bad) = if higher_is_better { (v >= good, v < bad) } else { (v <= good, v > bad) };
    if is_good {
        GOOD
    } else if is_bad {
        BAD
    } else {
        WARN
    }
}

pub fn big_button(ui: &mut Ui, text: &str, color: Color32) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).size(16.0).strong().color(Color32::from_rgb(10, 14, 20)))
            .fill(color)
            .min_size(vec2(180.0, 38.0))
            .corner_radius(8),
    )
}

pub fn key_hint(ui: &mut Ui, key: &str, what: &str) {
    ui.horizontal(|ui| {
        egui::Frame::new()
            .fill(CARD_HI)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(5)
            .inner_margin(Margin::symmetric(8, 2))
            .show(ui, |ui| ui.label(RichText::new(key).monospace().strong()));
        ui.label(RichText::new(what).color(MUTED));
    });
}
