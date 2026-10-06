//! Plain-language explanations shown when hovering a metric. Keyed by the label the user
//! sees, so every tile and table row that uses the same name gets the same explanation.
//! Written for people who don't know mouse jargon: what it is, then what's good / bad.

pub fn for_label(label: &str) -> Option<&'static str> {
    let key = label.trim().trim_start_matches("  ").to_ascii_lowercase();
    Some(match key.as_str() {
        // --- Polling / stability ------------------------------------------------------
        "polling rate" | "detected polling rate" => {
            "How many times per second your PC asks the mouse for new movement.\n\n1000 Hz = every 1 ms, 8000 Hz = every 0.125 ms. 'Measured' is what actually arrived, which should match what you set in the mouse software."
        }
        "rate held while fast" => {
            "While you moved fast enough, how often the mouse really delivered its full polling rate (checked in tiny 20 ms slices).\n\n100% = it never dipped.\nBelow ~97% = the rate drops now and then, which can feel like your aim randomly getting rough."
        }
        "hitches / min" | "hitches per minute" => {
            "A hitch is a short freeze: no movement data for 2 ms or more while your hand was moving steadily.\n\nOn a 144–240 Hz monitor that's enough to make the crosshair skip a frame.\n0 = perfect · a few per minute = noticeable to sensitive players · 20+ = bad.\n\nNormal pauses (stopping, changing direction) are NOT counted."
        }
        "pc / wireless / lost" | "wireless/mouse delays" | "pc-side stalls" | "lost motion" => {
            "WHY each hitch happened:\n\n• PC: the mouse sent data on time, but your computer delivered it late (USB port, hub, drivers, CPU busy).\n• Wireless: the mouse had the data but the radio link was late (interference, distance to the receiver, low battery).\n• Lost: the movement never arrived (dirty sensor, bad surface, lifting the mouse)."
        }
        "worst gap" => {
            "The longest freeze found while you were moving.\n\nUnder ~4 ms: very hard to notice.\nOver ~8 ms: a visible stutter on high-refresh monitors."
        }
        "timing jitter p99" => {
            "How evenly spaced the reports are. Each report should arrive exactly one polling interval after the last one. This shows how far off the worst 1% were.\n\nµs = microseconds (1000 µs = 1 ms). Lower = steadier. Values up to about one polling interval are normal on Windows and not something you can feel."
        }
        "cpu avg / peak" | "pc cpu load" | "cpu load avg" => {
            "How busy your whole PC's processor was during the test: the average, and the busiest moment.\n\nHigher polling rates cost more CPU. If it peaks near 100%, your PC may struggle to keep up and cause stutter."
        }
        "fast enough" | "time fast enough" => {
            "A mouse only sends data when it moves. At high rates you have to move quickly for there to be new movement at every single poll.\n\nThis is how much of the test you moved fast enough for the polling rate to be judged. Under 30%: move faster next time (big, quick circles)."
        }
        "avg polling delay" => {
            "On average, new movement waits half a polling interval before the PC asks for it. This is the delay a higher polling rate removes.\n\n1000 Hz = 0.5 ms · 4000 Hz = 0.125 ms · 8000 Hz = 0.0625 ms.\nThis is theory (calculated), not measured."
        }
        "runs" => {
            "How many test runs exist for this rate, and how many seconds of movement they contain. More runs = a more trustworthy answer. 2–3 per rate is good."
        }
        "recommended" => {
            "The highest polling rate that was just as stable as your most stable rate. You get the lowest delay without paying for it in stutter."
        }
        "stable" => "No meaningfully worse than your most stable rate. Safe to use.",
        "worse" => {
            "Clearly more hitches, a rate that didn't hold, or a maxed-out CPU compared with your best rate. It costs you more than it gives."
        }
        "not enough data" => {
            "This rate wasn't tested long enough, you didn't move fast enough, or the window lost focus, so it can't be judged fairly."
        }

        // --- Sensor --------------------------------------------------------------------
        "sensor glitches" => {
            "Moments where, during smooth movement, the sensor reported a jump backwards or a sudden spike.\n\nA clean sensor on a good pad shows about 0. Dust or hair on the sensor, worn skates or a dirty/worn pad cause them."
        }
        "sensor reversals /10k" => {
            "Times per 10,000 reports the sensor briefly said you moved backwards while you were moving smoothly forwards. Should be ~0. A classic sign of a dirty sensor or a bad surface."
        }
        "sensor spikes /10k" => {
            "Times per 10,000 reports one report was suddenly far bigger than its neighbours (a jump). Should be ~0. Points at the sensor or the surface."
        }
        "velocity noise" => {
            "How shaky your recorded speed is from one moment to the next. Lower = smoother.\n\nOnly compare runs with the same polling rate and DPI, since both affect it."
        }
        "speed" => {
            "How fast the mouse is moving. 'c/s' = sensor counts per second. Set your DPI at the top of a test page to see it in cm/s."
        }

        // --- Live view ------------------------------------------------------------------
        "motion reports, last 1 s" | "live reports / s" => {
            "How many movement reports the mouse sent in the last second. While you move fast this should be close to your polling rate."
        }
        "hitches (last 5 s)" => "Freezes (2 ms+) during steady movement in the last 5 seconds. Should stay at 0.",
        "longest gap, last 1 s" => {
            "The longest pause between two movement reports in the last second. Includes your own natural pauses, so it's only meaningful while you keep moving."
        }

        // --- Accuracy / DPI --------------------------------------------------------------
        "average error" => {
            "After moving around and returning to the exact starting point, how far off the sensor thinks you are, as a % of the distance travelled.\n\nUnder 0.5% = excellent · under 1.5% = normal · over 4% = the sensor is losing tracking. Part of it is your hand not landing perfectly, so compare runs rather than reading one number."
        }
        "average miss" | "average miss (counts)" => {
            "The same leftover error in sensor counts (and mm if you set DPI). 0 = the sensor ended exactly where it started."
        }
        "worst round" => "The single round with the biggest error.",
        "measured dpi" => {
            "DPI = how many counts the sensor reports per inch of movement. This is the real value measured against your ruler."
        }
        "vs set dpi" => {
            "Difference between the measured DPI and the DPI you set. Most sensors are within ±5%. It only matters if you match sensitivity between mice or games."
        }
        "spread" | "spread between rounds" => {
            "How much the rounds disagree with each other. Over ~5% means the measurement was sloppy (mouse not kept straight along the ruler). Redo it carefully."
        }

        // --- Buttons ---------------------------------------------------------------------
        "presses" | "total presses" => "How many times this button was pressed during the test.",
        "double-click faults" => {
            "A press that came impossibly fast after a release while you were clicking slowly. That's a worn switch registering one click as two. Should be 0."
        }
        "shortest gap" => {
            "The shortest time between releasing the button and pressing it again. With deliberate slow clicks, anything under ~80 ms is suspicious."
        }
        "median hold" => "How long you typically held the button down per click. Just context, not a health measure.",
        "wheel" | "wheel wrong-way" => {
            "Scroll notches up / down, and 'wrong-way' jumps: a notch in the opposite direction while you scroll one way. That's a worn or dirty scroll encoder. Should be 0."
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    /// Every metric label shown in tiles and tables must explain itself on hover.
    #[test]
    fn every_metric_has_an_explanation() {
        let labels = [
            // Polling / swipe result tiles
            "Polling rate",
            "Rate held while fast",
            "Hitches / min",
            "Worst gap",
            "Timing jitter p99",
            "Sensor glitches",
            "Velocity noise",
            "Fast enough",
            "PC CPU load",
            // Best polling rate table
            "Runs",
            "  PC / wireless / lost",
            "CPU avg / peak",
            "Avg polling delay",
            "recommended",
            "stable",
            "worse",
            "not enough data",
            // Compare table
            "Hitches per minute",
            "  wireless/mouse delays",
            "  PC-side stalls",
            "  lost motion",
            "Sensor reversals /10k",
            "Sensor spikes /10k",
            "Time fast enough",
            "CPU load avg",
            "Average error",
            "Average miss",
            "Worst round",
            "Measured DPI",
            "Spread between rounds",
            "Double-click faults",
            "Wheel wrong-way",
            "Total presses",
            // Live view & running tests
            "Motion reports, last 1 s",
            "Detected polling rate",
            "Hitches (last 5 s)",
            "Longest gap, last 1 s",
            "Speed",
            "Live reports / s",
            // Accuracy / DPI / clicks
            "vs set DPI",
            "Spread",
            "presses",
            "double-click faults",
            "shortest gap",
            "median hold",
            "Wheel",
        ];
        let missing: Vec<_> = labels.iter().filter(|l| super::for_label(l).is_none()).collect();
        assert!(missing.is_empty(), "no explanation for: {missing:?}");
    }
}
