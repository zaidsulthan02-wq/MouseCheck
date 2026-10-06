//! Device inventory, standard battery report, and an honest note on what can't be read.

use egui::{RichText, Ui};

use super::App;
use super::widgets::*;
use crate::hid::{self, HidInventory};

#[derive(Default)]
pub struct DeviceUi {
    pub scanned: Option<(u32, HidInventory)>,
}

pub fn page(app: &mut App, ui: &mut Ui) {
    title(ui, "Device & battery", "What Windows can see of your mouse and receiver.");

    let devices: Vec<crate::capture::DeviceInfo> = app.cap.store.lock().unwrap().devices.clone();
    card(ui, |ui| {
        egui::Grid::new("devices").striped(true).num_columns(5).show(ui, |ui| {
            for h in ["device", "manufacturer", "USB id", "reports seen", ""] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for (i, d) in devices.iter().enumerate() {
                ui.label(d.display_name());
                ui.label(if d.manufacturer.is_empty() { "–".to_string() } else { d.manufacturer.clone() });
                ui.label(if d.vid != 0 { format!("{:04X}:{:04X}", d.vid, d.pid) } else { "–".into() });
                ui.label(format!("{}", d.events));
                if d.vid != 0 && ui.button("Scan HID & battery").clicked() {
                    app.dev_ui.scanned = Some((i as u32, hid::inventory(d.vid, d.pid)));
                }
                ui.end_row();
            }
        });
    });

    if let Some((idx, inv)) = &app.dev_ui.scanned {
        let name = devices.get(*idx as usize).map(|d| d.display_name()).unwrap_or_default();
        section(ui, &format!("Receiver interfaces: {name}"));
        card(ui, |ui| {
            match &inv.battery {
                Some(b) => {
                    let pct = b.percent.map(|p| format!("{p:.0}%")).unwrap_or_else(|| format!("raw {}", b.raw));
                    tile(ui, "Battery", &pct, &b.source, if b.percent.unwrap_or(100.0) > 20.0 { GOOD } else { WARN });
                }
                None => {
                    ui.label(RichText::new("Battery level: not available through standard HID").strong().color(WARN));
                }
            }
            for n in &inv.notes {
                ui.label(RichText::new(n).color(MUTED));
            }
            ui.add_space(6.0);
            egui::Grid::new("collections").striped(true).num_columns(3).show(ui, |ui| {
                for h in ["usage", "what it is", "product string"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for c in &inv.collections {
                    ui.label(RichText::new(format!("{:04X}:{:04X}", c.usage_page, c.usage)).monospace());
                    ui.label(&c.kind);
                    ui.label(&c.product).on_hover_text(&c.path);
                    ui.end_row();
                }
            });
        });
    }

    section(ui, "Battery health & temperature: what's real");
    card(ui, |ui| {
        ui.label(RichText::new("Battery level").strong());
        ui.label("Wireless gaming mice send their battery level through a private, brand-specific protocol (that's what the brand's app or web driver uses). MouseCheck reads the standard HID battery field when a mouse exposes it. For other mice it needs a small protocol module per brand, and that's where open-source contributions come in.");
        ui.add_space(4.0);
        ui.label(RichText::new("Battery health").strong());
        ui.label("No mouse reports battery wear directly. The honest way to estimate it: log the battery % over time and compare hours-per-charge with the advertised figure. Once a battery reading is available, this can be added.");
        ui.add_space(4.0);
        ui.label(RichText::new("Temperature").strong());
        ui.label("Consumer mice don't have or don't expose a temperature sensor over USB. Any tool that shows a mouse temperature is guessing.");
        ui.add_space(4.0);
        ui.label(RichText::new("What you can test today").strong());
        ui.label("Low battery often makes wireless mice reduce radio power or polling rate. Run the Polling test with the label 'Low battery', charge fully, run it again as 'Full battery', and compare. Any difference in hitches or rate held is the battery's effect on your aim.");
    });
}
