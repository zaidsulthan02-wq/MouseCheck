//! Turns measurements into plain-language findings with concrete things to try.
//! Thresholds are deliberately conservative: we'd rather say "looks fine" than invent
//! a problem. Every finding names the number it is based on.

use crate::analysis::{AccuracyReport, ClickReport, DpiReport, MotionReport, cps_to_cm_s};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Bad,
    Warn,
    Info,
    Good,
}

#[derive(Clone, Debug)]
pub struct Finding {
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub fixes: Vec<&'static str>,
}

fn f(severity: Severity, title: impl Into<String>, detail: impl Into<String>, fixes: Vec<&'static str>) -> Finding {
    Finding { severity, title: title.into(), detail: detail.into(), fixes }
}

const FIX_RADIO: [&str; 6] = [
    "Put the receiver on a USB extension cable/dock so it sits 20–30 cm from the mouse with clear line of sight.",
    "Don't plug the receiver next to other 2.4 GHz receivers (keyboard dongle) or a USB 3.x port with a storage drive or hub — USB 3 emits 2.4 GHz noise.",
    "Move Wi-Fi routers, Bluetooth headsets and phones away from the desk, or put your Wi-Fi on 5/6 GHz.",
    "Charge the mouse fully and re-test — many mice lower their radio power or rate when the battery is low.",
    "Re-pair the receiver with the brand's software and update the mouse + receiver firmware.",
    "Plug the mouse in with its cable (wired mode) and re-test: if the problem disappears, it's the wireless link.",
];

const FIX_PC: [&str; 6] = [
    "Plug the receiver directly into a rear motherboard port (not a hub, monitor port or front panel).",
    "Close overlays and background apps that read the mouse (RGB suites, macro tools, screen recorders), then re-test.",
    "In Windows power settings use 'High performance' / 'Best performance' and disable USB selective suspend.",
    "Try a lower polling rate (4000 or 2000 Hz) — 8000 Hz costs a lot of CPU time and many games can't keep up.",
    "Update chipset / USB drivers from your motherboard vendor.",
    "Check whether the stalls line up with high CPU usage (shown in the results).",
];

const FIX_SENSOR: [&str; 5] = [
    "Clean the sensor window: blow it out with air, then wipe gently with a dry cotton swab (no alcohol on the lens if unsure).",
    "Look for hair or fluff caught in the sensor well and around the skates.",
    "Clean or replace the mousepad — worn, shiny or dirty spots cause tracking errors.",
    "Check the skates: a chipped or peeling skate tilts the sensor and changes its height.",
    "Re-test on a different surface to see whether it's the pad or the mouse.",
];

pub fn motion(r: &MotionReport) -> Vec<Finding> {
    let mut out = Vec::new();
    if !r.ok {
        out.push(f(Severity::Warn, "Not enough data", r.message.clone(), vec![]));
        return out;
    }

    if r.foreground_pct < 99.0 {
        out.push(f(
            Severity::Bad,
            "Test window lost focus",
            format!("Only {:.0}% of reports arrived while MouseCheck was the active window. Windows 11 throttles mouse input for background apps, so this run's timing can't be trusted.", r.foreground_pct),
            vec!["Re-run the test and keep MouseCheck in front (don't alt-tab or click other windows)."],
        ));
    }

    // Polling rate itself
    let rate_txt = if r.nominal_snapped {
        format!("Detected polling rate: {:.0} Hz (measured {:.0} Hz).", r.nominal_hz, r.measured_hz)
    } else {
        format!("Measured rate {:.0} Hz doesn't match a standard polling rate.", r.measured_hz)
    };
    if !r.nominal_snapped {
        out.push(f(
            Severity::Warn,
            "Unusual polling rate",
            rate_txt,
            vec!["Check the polling rate in the mouse software and re-test while moving faster."],
        ));
    } else {
        out.push(f(Severity::Info, format!("Polling rate: {:.0} Hz", r.nominal_hz), rate_txt, vec![]));
    }

    // Did the user actually move fast enough?
    let need = r.needed_cps();
    let need_txt = match r.dpi {
        Some(d) => format!(
            "At {d:.0} DPI you must move at least {:.1} cm/s for a report to be due every poll.",
            cps_to_cm_s(need, d)
        ),
        None => format!(
            "You must move at least {need:.0} counts/s for a report to be due every poll (set your DPI to see this in cm/s)."
        ),
    };
    if r.fast_time_pct < 30.0 {
        out.push(f(
            Severity::Warn,
            "You rarely moved fast enough to use the full polling rate",
            format!("Only {:.0}% of moving time was fast enough. {need_txt} Below that speed the mouse has nothing new to send, so it skips polls. That's normal, not a fault.", r.fast_time_pct),
            vec![
                "For the polling test, move faster (big, quick circles or swipes).",
                "High rates like 8000 Hz only help with high DPI (1600+) and a lower in-game sensitivity to match.",
            ],
        ));
    }

    // Rate held while fast
    if r.rate_held_pct.is_finite() {
        if r.rate_held_pct >= 97.0 {
            out.push(f(
                Severity::Good,
                "Polling rate held steady",
                format!(
                    "While you moved fast, {:.1}% of 20 ms windows delivered ≥85% of {:.0} Hz.",
                    r.rate_held_pct, r.nominal_hz
                ),
                vec![],
            ));
        } else if r.rate_held_pct >= 85.0 {
            out.push(f(Severity::Warn, "Polling rate dips at times", format!("Only {:.1}% of fast-moving windows kept ≥85% of {:.0} Hz. Brief dips are what makes aim feel smooth one moment and rough the next.", r.rate_held_pct, r.nominal_hz), vec![]));
        } else {
            out.push(f(
                Severity::Bad,
                "Polling rate often collapses",
                format!("Only {:.1}% of fast-moving windows kept ≥85% of {:.0} Hz.", r.rate_held_pct, r.nominal_hz),
                vec![],
            ));
        }
    }

    // Hitches
    let total = r.hitches.len();
    if total == 0 {
        out.push(f(
            Severity::Good,
            "No hitches",
            "No gaps of 2 ms or more while you were moving. The report stream was continuous.",
            vec![],
        ));
    } else {
        let sev = if r.hitches_per_min > 20.0 || r.max_gap_ms > 16.0 {
            Severity::Bad
        } else if r.hitches_per_min > 3.0 || r.max_gap_ms > 6.0 {
            Severity::Warn
        } else {
            Severity::Info
        };
        out.push(f(
            sev,
            format!("{total} hitches ({:.1}/min, worst {:.1} ms)", r.hitches_per_min, r.max_gap_ms),
            "A hitch is a gap of 2 ms or more in the report stream while you were moving. At 144–240 Hz that's a visible stutter.",
            vec![],
        ));
        let dominant = [(r.link_delays, 0), (r.pc_stalls, 1), (r.lost, 2)].into_iter().max().unwrap();
        if r.link_delays > 0 && dominant.1 == 0 {
            out.push(f(
                sev,
                "Most hitches come from the wireless link / mouse",
                format!("{} of {total} gaps ended with one oversized report carrying the missed motion. The mouse had the data but couldn't deliver it on time, typically radio interference, distance or low battery.", r.link_delays),
                FIX_RADIO.to_vec(),
            ));
        }
        if r.pc_stalls > 0 && dominant.1 == 1 {
            out.push(f(
                sev,
                "Most hitches come from the PC side",
                format!("{} of {total} gaps ended with a burst of reports arriving at once. The receiver sent them on time, but USB, the driver or the CPU delivered them late. CPU during test: avg {:.0}%, max {:.0}%.", r.pc_stalls, r.cpu_avg, r.cpu_max),
                FIX_PC.to_vec(),
            ));
        }
        if r.lost > 0 && dominant.1 == 2 {
            out.push(f(
                sev,
                "Motion went missing",
                format!("{} of {total} gaps were never caught up: the motion was simply lost. That points at the sensor losing tracking (dirt, surface, lifting) or the radio dropping packets.", r.lost),
                FIX_SENSOR.iter().chain(FIX_RADIO.iter().take(2)).copied().collect(),
            ));
        }
    }

    // Jitter
    if r.saturated_intervals > 500 {
        let period = 1e6 / r.nominal_hz;
        let rel = r.jitter_p99_us / period;
        if r.bunched_pct > 25.0 && r.nominal_hz >= 4000.0 {
            out.push(f(
                Severity::Info,
                "Reports often arrive in pairs/bursts",
                format!("{:.0}% of intervals were under half a polling period. At {:.0} Hz Windows often hands reports over in small batches. You won't feel this unless it comes with hitches.", r.bunched_pct, r.nominal_hz),
                vec![],
            ));
        } else if rel > 1.0 && r.nominal_hz <= 2000.0 {
            out.push(f(
                Severity::Warn,
                "High timing jitter",
                format!(
                    "99th-percentile timing error is {:.0} µs, more than a full polling period ({period:.0} µs).",
                    r.jitter_p99_us
                ),
                FIX_PC[..3].to_vec(),
            ));
        }
    }

    // Sensor
    if r.reversals_per_10k > 5.0 || r.spikes_per_10k > 5.0 {
        out.push(f(
            if r.reversals_per_10k > 20.0 || r.spikes_per_10k > 20.0 { Severity::Bad } else { Severity::Warn },
            "Sensor glitches detected",
            format!("{:.1} backwards jumps and {:.1} sudden spikes per 10,000 reports during smooth motion. A clean sensor on a good surface shows almost none.", r.reversals_per_10k, r.spikes_per_10k),
            FIX_SENSOR.to_vec(),
        ));
    } else if r.saturated_intervals > 500 {
        out.push(f(
            Severity::Good,
            "Sensor tracking looks clean",
            format!("{:.1} reversals and {:.1} spikes per 10k reports.", r.reversals_per_10k, r.spikes_per_10k),
            vec![],
        ));
    }

    out.sort_by_key(|x| x.severity);
    out
}

pub fn accuracy(r: &AccuracyReport) -> Vec<Finding> {
    let mut out = Vec::new();
    if r.rounds.is_empty() {
        return out;
    }
    let mm = r.mean_err_mm().map(|v| format!(" (≈{v:.1} mm)")).unwrap_or_default();
    let detail = format!(
        "Average return error {:.1} counts{mm}, {:.2}% of the distance travelled.",
        r.mean_err_counts, r.mean_err_pct
    );
    let (sev, title) = if r.mean_err_pct < 0.5 {
        (Severity::Good, "Excellent tracking accuracy")
    } else if r.mean_err_pct < 1.5 {
        (Severity::Info, "Normal tracking accuracy")
    } else if r.mean_err_pct < 4.0 {
        (Severity::Warn, "Tracking drifts")
    } else {
        (Severity::Bad, "Tracking loses a lot of motion")
    };
    out.push(f(sev, title, detail, if sev <= Severity::Warn { FIX_SENSOR.to_vec() } else { vec![] }));
    out.push(f(Severity::Info, "How to read this", "Some error comes from your hand: the mouse never lands exactly against the stop. Compare runs (before/after cleaning, pad A vs pad B) rather than reading one number on its own.", vec![]));
    out
}

pub fn dpi(r: &DpiReport) -> Vec<Finding> {
    let mut out = Vec::new();
    if r.runs.is_empty() {
        return out;
    }
    match r.deviation_pct() {
        Some(dev) => {
            let sev = if dev.abs() <= 3.0 {
                Severity::Good
            } else if dev.abs() <= 8.0 {
                Severity::Info
            } else {
                Severity::Warn
            };
            out.push(f(sev, format!("Real DPI ≈ {:.0} ({:+.1}% vs set)", r.mean_dpi, dev),
                "Most sensors are within ±5% of the advertised DPI. A bigger difference only matters if you match sensitivity across mice/games.",
                if sev == Severity::Warn { vec!["Check the DPI in the mouse software and redo the test carefully along a ruler.", "Surface affects measured DPI a bit, so compare on the same pad."] } else { vec![] }));
        }
        None => out.push(f(
            Severity::Info,
            format!("Real DPI ≈ {:.0}", r.mean_dpi),
            "Enter your configured DPI to see the deviation.",
            vec![],
        )),
    }
    if r.spread_pct > 5.0 {
        out.push(f(Severity::Warn, "Runs disagree", format!("Runs differ by {:.1}%. Re-measure: keep the mouse against a straight edge and stop exactly on the mark.", r.spread_pct), vec![]));
    }
    out
}

pub fn clicks(r: &ClickReport) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut any = false;
    for b in &r.buttons {
        if b.presses == 0 {
            continue;
        }
        any = true;
        if b.chatter > 0 {
            out.push(f(
                Severity::Bad,
                format!("{}: possible double-click fault ({}x)", b.name, b.chatter),
                format!("A new press came {:.1} ms after a release. Clicking deliberately at a normal pace, a healthy switch never does that faster than ~{:.0} ms.", b.min_gap_ms, r.threshold_ms),
                vec![
                    "Re-test clicking slowly. If it repeats, the switch is wearing out.",
                    "Raise the debounce time in the mouse software (if it has one).",
                    "Under warranty: RMA. Otherwise the switch can be replaced (soldering) or cleaned with contact cleaner.",
                ],
            ));
        } else {
            out.push(f(
                Severity::Good,
                format!("{}: {} clean clicks", b.name, b.presses),
                format!("Shortest release→press gap {:.0} ms, median hold {:.0} ms.", b.min_gap_ms, b.hold_median_ms),
                vec![],
            ));
        }
    }
    if r.wheel.up + r.wheel.down > 0 {
        any = true;
        if r.wheel.wrong_way > 0 {
            out.push(f(Severity::Bad, format!("Scroll wheel jumped the wrong way {}x", r.wheel.wrong_way),
                "While scrolling in one direction the wheel reported a notch the other way. That's a worn or dirty scroll encoder.",
                vec!["Blow compressed air around the wheel and spin it a lot in both directions.", "If it persists, the encoder needs replacing."]));
        } else {
            out.push(f(
                Severity::Good,
                "Scroll wheel consistent",
                format!("{} up / {} down notches, no wrong-way jumps.", r.wheel.up, r.wheel.down),
                vec![],
            ));
        }
    }
    if !any {
        out.push(f(
            Severity::Info,
            "No clicks recorded",
            "Click each button 20+ times and scroll both directions.",
            vec![],
        ));
    }
    out.sort_by_key(|x| x.severity);
    out
}
