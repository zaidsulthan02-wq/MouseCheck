//! Application shell: navigation, shared test setup, device selection, status bar.

mod charts;
mod clicks;
mod device;
mod explain;
mod guide;
mod live;
mod motion;
mod rates;
mod rounds;
mod sessions;
pub mod widgets;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use egui::{RichText, Ui};

use crate::analysis::Sample;
use crate::capture::{Ev, Shared};
use crate::session::{self, Session, Setup, TestKind};
use widgets::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Live,
    Polling,
    Rates,
    Swipe,
    Accuracy,
    Dpi,
    Clicks,
    Sessions,
    Device,
    Guide,
}

const NAV: [(Page, &str, &str); 10] = [
    (Page::Live, "Live view", "What the mouse sends right now"),
    (Page::Polling, "Polling & stability", "Rate, jitter, hitches"),
    (Page::Rates, "Best polling rate", "What each rate gains or costs"),
    (Page::Swipe, "Swipe / tracking", "Your real aim motion"),
    (Page::Accuracy, "Accuracy", "Return-to-origin drift"),
    (Page::Dpi, "Real DPI", "Measure with a ruler"),
    (Page::Clicks, "Buttons & wheel", "Double-click & scroll faults"),
    (Page::Sessions, "Results & compare", "Before / after"),
    (Page::Device, "Device & battery", "What the mouse exposes"),
    (Page::Guide, "How to test", "Method & meaning"),
];

pub const POLLING_CHOICES: [&str; 8] = ["", "125 Hz", "250 Hz", "500 Hz", "1000 Hz", "2000 Hz", "4000 Hz", "8000 Hz"];
pub const LABEL_PRESETS: [&str; 8] = [
    "Baseline",
    "Before cleaning",
    "After cleaning",
    "Old mousepad",
    "New mousepad",
    "Receiver moved closer",
    "Low battery",
    "Full battery",
];

pub struct App {
    pub cap: Arc<Shared>,
    pub page: Page,
    pub setup: Setup,
    pub dpi_known: bool,
    pub dpi_value: f64,
    /// None = automatically use the mouse that moved most.
    pub device: Option<u32>,
    pub sessions: Vec<Session>,
    pub polling: motion::MotionTest,
    pub swipe: motion::MotionTest,
    pub rates: rates::RatesUi,
    pub accuracy: rounds::RoundsTest,
    pub dpi: rounds::RoundsTest,
    pub clicks: clicks::ClickTest,
    pub sess_ui: sessions::SessionsUi,
    pub dev_ui: device::DeviceUi,
    pub live: live::LiveUi,
    pub toast: Option<(String, Instant)>,
    grab: (bool, bool),
}

impl App {
    pub fn new(cap: Arc<Shared>) -> Self {
        Self {
            cap,
            page: Page::Live,
            setup: Setup { label: "Baseline".into(), ..Default::default() },
            dpi_known: false,
            dpi_value: 1600.0,
            device: None,
            sessions: session::load_all(),
            polling: motion::MotionTest::new(10.0),
            swipe: motion::MotionTest::new(15.0),
            rates: Default::default(),
            accuracy: rounds::RoundsTest::new(TestKind::Accuracy),
            dpi: rounds::RoundsTest::new(TestKind::Dpi),
            clicks: clicks::ClickTest::default(),
            sess_ui: Default::default(),
            dev_ui: Default::default(),
            live: Default::default(),
            toast: None,
            grab: (false, true),
        }
    }

    pub fn dpi(&self) -> Option<f64> {
        self.dpi_known.then_some(self.dpi_value)
    }

    /// The setup that will be stamped on the next saved run.
    pub fn current_setup(&self) -> Setup {
        Setup { dpi: self.dpi(), ..self.setup.clone() }
    }

    pub fn busy(&self) -> bool {
        self.polling.busy()
            || self.swipe.busy()
            || self.rates.test.busy()
            || self.accuracy.active
            || self.dpi.active
            || self.clicks.active
    }

    pub fn device_name(&self, idx: u32) -> String {
        self.cap
            .store
            .lock()
            .unwrap()
            .devices
            .get(idx as usize)
            .map(|d| d.display_name())
            .unwrap_or_else(|| "Unknown".into())
    }

    pub fn to_samples(&self, evs: &[Ev], dev: u32, t0: i64) -> (Vec<Sample>, f64) {
        let mut fg = 0usize;
        let s: Vec<Sample> = evs
            .iter()
            .filter(|e| e.dev == dev)
            .inspect(|e| fg += e.fg as usize)
            .map(|e| Sample { t: self.cap.ticks_to_us(e.t - t0), dx: e.dx, dy: e.dy, bf: e.bflags, bd: e.bdata })
            .collect();
        let pct = if s.is_empty() { 100.0 } else { fg as f64 / s.len() as f64 * 100.0 };
        (s, pct)
    }

    pub fn store_session(&mut self, mut s: Session) -> Session {
        match session::save(&mut s) {
            Ok(_) => self.toast("Saved to Results & compare"),
            Err(e) => self.toast(&format!("Could not save run: {e}")),
        }
        self.sessions.insert(0, s.clone());
        s
    }

    pub fn toast(&mut self, msg: &str) {
        self.toast = Some((msg.to_string(), Instant::now()));
    }

    /// Confine (and optionally hide) the cursor while a test runs so flicks can't click
    /// other windows and steal focus (which would throttle raw input).
    fn apply_grab(&mut self, ctx: &egui::Context, confine: bool, visible: bool) {
        if self.grab != (confine, visible) {
            self.grab = (confine, visible);
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(if confine {
                egui::CursorGrab::Confined
            } else {
                egui::CursorGrab::None
            }));
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(visible));
        }
    }

    fn resolve_devices(&mut self) {
        let pending: Vec<(usize, crate::capture::DeviceInfo)> = {
            let s = self.cap.store.lock().unwrap();
            s.devices.iter().enumerate().filter(|(_, d)| !d.resolved).map(|(i, d)| (i, d.clone())).collect()
        };
        for (i, mut d) in pending {
            crate::hid::resolve(&mut d);
            let mut s = self.cap.store.lock().unwrap();
            if let Some(t) = s.devices.get_mut(i) {
                t.vid = d.vid;
                t.pid = d.pid;
                t.product = d.product;
                t.manufacturer = d.manufacturer;
                t.resolved = true;
            }
        }
    }

    pub fn setup_editor(&mut self, ui: &mut Ui) {
        let enabled = !self.busy();
        card(ui, |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Run label").strong());
                    ui.add(
                        egui::TextEdit::singleline(&mut self.setup.label)
                            .hint_text("e.g. Before cleaning")
                            .desired_width(170.0),
                    );
                    egui::ComboBox::from_id_salt("label_presets").selected_text("presets").width(90.0).show_ui(
                        ui,
                        |ui| {
                            for p in LABEL_PRESETS {
                                if ui.selectable_label(self.setup.label == p, p).clicked() {
                                    self.setup.label = p.into();
                                }
                            }
                        },
                    );
                    ui.separator();
                    ui.checkbox(&mut self.dpi_known, "DPI");
                    ui.add_enabled(
                        self.dpi_known,
                        egui::DragValue::new(&mut self.dpi_value).range(50.0..=50_000.0).speed(10.0),
                    );
                    ui.separator();
                    ui.label("Rate set in software");
                    let txt = if self.setup.polling_setting.is_empty() {
                        "not set".to_string()
                    } else {
                        self.setup.polling_setting.clone()
                    };
                    egui::ComboBox::from_id_salt("poll_setting").selected_text(txt).width(90.0).show_ui(ui, |ui| {
                        for c in POLLING_CHOICES {
                            let name = if c.is_empty() { "not set" } else { c };
                            ui.selectable_value(&mut self.setup.polling_setting, c.to_string(), name);
                        }
                    });
                    ui.separator();
                    ui.label("Surface");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.setup.surface).hint_text("pad name").desired_width(120.0),
                    );
                });
            });
            ui.label(RichText::new("These details are saved with each run so before/after comparisons make sense. DPI lets results show cm/s and mm.").small().color(MUTED));
        });
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("MouseCheck").size(20.0).strong().color(ACCENT));
            ui.label(RichText::new("honest mouse diagnostics").color(MUTED));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let devices: Vec<(u32, String, u64)> = {
                    let s = self.cap.store.lock().unwrap();
                    s.devices.iter().enumerate().map(|(i, d)| (i as u32, d.display_name(), d.motion_events)).collect()
                };
                let sel = match self.device {
                    None => "Auto (mouse that moves)".to_string(),
                    Some(i) => devices.iter().find(|d| d.0 == i).map(|d| d.1.clone()).unwrap_or_default(),
                };
                ui.add_enabled_ui(!self.busy(), |ui| {
                    egui::ComboBox::from_id_salt("device").selected_text(sel).width(300.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.device, None, "Auto (mouse that moves)");
                        for (i, name, ev) in &devices {
                            ui.selectable_value(&mut self.device, Some(*i), format!("{name}  ·  {ev} moves"));
                        }
                    });
                });
                ui.label(RichText::new("Device under test").color(MUTED));
            });
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let reg = self.cap.registered.load(Ordering::Relaxed);
            let fg = self.cap.foreground.load(Ordering::Relaxed);
            let restores = self.cap.reg_restores.load(Ordering::Relaxed);
            dot(
                ui,
                if reg { GOOD } else { BAD },
                if reg { "Raw input capture active" } else { "Raw input NOT registered" },
            );
            dot(
                ui,
                if fg { GOOD } else { WARN },
                if fg {
                    "Window active: full-rate input"
                } else {
                    "Window inactive: Windows throttles background input, timing invalid"
                },
            );
            if restores > 0 {
                dot(ui, WARN, &format!("capture re-claimed {restores}x"));
            }
            let freq = self.cap.freq;
            ui.label(RichText::new(format!("timer {:.1} MHz", freq as f64 / 1e6)).small().color(MUTED));
            if let Some((msg, at)) = &self.toast {
                if at.elapsed() < Duration::from_secs(4) {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(msg).color(ACCENT));
                    });
                }
            }
        });
    }
}

fn dot(ui: &mut Ui, c: egui::Color32, text: &str) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    ui.painter().circle_filled(r.center(), 4.0, c);
    ui.label(RichText::new(text).small());
    ui.add_space(8.0);
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.resolve_devices();

        // Cursor confinement follows test state.
        let (confine, visible) = if self.clicks.active {
            (true, true)
        } else if self.busy() {
            (true, false)
        } else {
            (false, true)
        };
        self.apply_grab(&ctx, confine, visible);

        egui::Panel::top("top")
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::symmetric(16, 10)))
            .show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::symmetric(16, 6)))
            .show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("nav")
            .exact_size(230.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(10, 12)))
            .show(ui, |ui| {
                let busy = self.busy();
                for (page, name, sub) in NAV {
                    let selected = self.page == page;
                    let resp = ui.add_enabled_ui(!busy || selected, |ui| {
                        let fill = if selected { CARD_HI } else { PANEL };
                        egui::Frame::new()
                            .fill(fill)
                            .corner_radius(8)
                            .inner_margin(egui::Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(RichText::new(name).strong().color(if selected { ACCENT } else { TEXT }));
                                ui.label(RichText::new(sub).small().color(MUTED));
                            })
                            .response
                            .interact(egui::Sense::click())
                    });
                    if resp.inner.clicked() && !busy {
                        self.page = page;
                    }
                    if resp.inner.hovered() && !busy {
                        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                }
                if busy {
                    ui.add_space(8.0);
                    ui.label(RichText::new("Test running. Press Esc to cancel.").small().color(WARN));
                }
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::symmetric(22, 16)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_max_width(1200.0);
                    match self.page {
                        Page::Live => live::page(self, ui),
                        Page::Polling => motion::page(self, ui, TestKind::Polling),
                        Page::Rates => rates::page(self, ui),
                        Page::Swipe => motion::page(self, ui, TestKind::Swipe),
                        Page::Accuracy => rounds::page(self, ui, TestKind::Accuracy),
                        Page::Dpi => rounds::page(self, ui, TestKind::Dpi),
                        Page::Clicks => clicks::page(self, ui),
                        Page::Sessions => sessions::page(self, ui),
                        Page::Device => device::page(self, ui),
                        Page::Guide => guide::page(ui),
                    }
                });
            });

        // Live pages and running tests animate; otherwise idle politely.
        if self.busy() || self.page == Page::Live {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }
}
