//! Method notes: how to run fair before/after tests and what the numbers mean.

use egui::{RichText, Ui};

use super::widgets::*;

fn block(ui: &mut Ui, head: &str, lines: &[&str]) {
    card(ui, |ui| {
        ui.label(RichText::new(head).size(17.0).strong());
        for l in lines {
            ui.label(*l);
        }
    });
}

pub fn page(ui: &mut Ui) {
    title(
        ui,
        "How to test (and why it's honest)",
        "Numbers only mean something if the test is fair. Here's the method.",
    );

    block(
        ui,
        "Fair before / after",
        &[
            "• Change one thing at a time: clean the sensor OR swap the pad OR move the receiver.",
            "• Keep DPI, polling rate and your way of moving the same between runs.",
            "• Do each test 2–3 times per condition. One run can be a fluke.",
            "• Set the run label (e.g. 'Before cleaning') before you start, then tick the runs in Results & compare.",
        ],
    );

    block(
        ui,
        "Suggested workflow for 'sometimes smooth, sometimes rough'",
        &[
            "1. Polling test at your normal settings, label 'Baseline'. Do it 3 times.",
            "2. Move the receiver onto an extension cable close to the mouse, away from the keyboard dongle and USB 3 ports, and re-test.",
            "3. Re-test with a fully charged battery.",
            "4. Run the 'Best polling rate' sweep. If high rates have hitches but low ones don't, your PC can't keep up with them.",
            "5. Re-test with your game running in the background (alt-tabbed). If hitches appear, system load is the cause.",
            "6. Clean the sensor and run Accuracy + Swipe before and after.",
        ],
    );

    block(
        ui,
        "What a 'hitch' is",
        &[
            "A gap of at least 2 ms in the report stream while you were moving fast enough that reports should have kept coming. At 144–240 Hz a 2–7 ms gap shifts the crosshair by a frame: that's the 'suddenly not smooth' feeling.",
            "MouseCheck sorts every hitch by what happened next:",
            "• Wireless/mouse delay: nothing arrived, then one big report with all the missed motion. The mouse had the data but the radio link was late (interference, distance, battery).",
            "• PC-side stall: the reports were sent on time but Windows delivered them late, all at once (USB controller, drivers, CPU load).",
            "• Lost motion: the movement never arrived at all (sensor lost tracking or data was dropped).",
        ],
    );

    block(
        ui,
        "Why fast movement matters for 8000 Hz",
        &[
            "A mouse only sends a report when the sensor has moved at least one count since the last poll. At 8000 Hz that means at least 8000 counts per second.",
            "At 800 DPI that's 25 cm/s, at 1600 DPI 12.7 cm/s, at 3200 DPI 6.4 cm/s. Slower than that, an 8000 Hz mouse sends fewer reports, and that's correct behaviour, not a fault.",
            "MouseCheck only judges the polling rate during the moments you were fast enough.",
        ],
    );

    block(
        ui,
        "Why MouseCheck must be the active window",
        &[
            "Since a Windows 11 update (build 25272), raw mouse input for apps in the background is throttled and merged to protect game performance. Only the foreground app gets every report.",
            "So tests lock the cursor to this window and record whether it stayed in front. Runs where it didn't are flagged as invalid.",
            "This is also why browser-based polling testers can't measure 8000 Hz properly: browsers batch and round input timestamps.",
        ],
    );

    block(
        ui,
        "Limits (no hype)",
        &[
            "• Timestamps are taken when Windows hands each report to MouseCheck, not when the sensor read it. End-to-end latency (click-to-photon) needs extra hardware such as an LDAT or a high-speed camera.",
            "• Lift-off distance needs physical spacers; it isn't measured here.",
            "• The Accuracy test includes your hand's error returning to the stop. Compare runs instead of reading one number on its own.",
            "• Battery level only works for mice that use the standard HID battery report, or once a brand protocol module exists.",
        ],
    );
}
