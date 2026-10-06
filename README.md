# MouseCheck

**Honest mouse diagnostics. Measurements, not hype.**

Your aim feels butter-smooth one moment and rough the next. Is it the mouse, the wireless link, the PC, the sensor, or the pad? MouseCheck records every raw report your mouse sends and tells you, with numbers, what's going on and what to try. It's built for **before / after** testing: clean the sensor, change the pad, move the receiver, change the polling rate, then compare.

## Download

**[⬇ Download MouseCheck.exe (latest release)](https://github.com/zaidsulthan02-wq/MouseCheck/releases/latest)**: Windows 10/11, no install needed. Just run it.

- It's a single portable `.exe`. Your test runs are saved in a `sessions` folder next to it.
- Windows SmartScreen may warn "Windows protected your PC" because the app isn't code-signed (signing certificates cost money). Click **More info → Run anyway**. Every release is built by GitHub Actions straight from this source code, and a SHA-256 checksum is published next to the download.
- Keep MouseCheck as the active window while a test runs (see *Why a native app* below).

## What it measures

| Test | What you learn |
|---|---|
| **Live view** | Every raw report as it arrives: interval scatter, reports/s, sensor path, all mice on the system |
| **Polling & stability** | True polling rate, whether it holds while you move, timing jitter, and **hitches** (≥2 ms gaps while moving) |
| **Best polling rate** | Guided sweep over 500–8000 Hz (verifies each rate really applied), then a verdict: the highest rate that's as stable as your most stable one, what the step up gains in ms, and what it costs in hitches / CPU |
| **Swipe / tracking** | Your real aim motion: hitches plus sensor glitches (backwards jumps, spikes) and velocity noise |
| **Return-to-origin accuracy** | Motion the sensor loses or invents (move around, return to a physical stop) |
| **Real DPI** | True counts per inch against a ruler vs the DPI you set |
| **Buttons & wheel** | Double-click switch faults (chatter) and scroll-encoder wrong-way jumps |
| **Results & compare** | Every run auto-saved; tick runs to compare side by side with overlaid charts |
| **Device & battery** | HID interfaces of the receiver; reads the standard HID battery report if present |

### Hitch classification (the "why is it rough?" answer)

Every gap in the report stream while you were moving is classified by what happened next:

- **Wireless / mouse delay**: nothing arrived, then *one* oversized report carrying all the missed motion. The mouse had the data but the link was late → interference, distance, low battery.
- **PC-side stall**: reports arrived late in a *burst*. They were sent on time, but USB, the driver or the CPU held them up → port, hub, drivers, CPU load, too-high polling rate for the system.
- **Lost motion**: motion never arrived → sensor lost tracking (dirt, surface, lift) or data was dropped.

## Why a native app (and not a website)

1. **Windows 11 throttles background raw input.** Since build 25272, apps that aren't in the foreground get coalesced, rate-capped mouse input. MouseCheck only trusts data received while it is the foreground window, locks the cursor during tests, and flags any run where it lost focus.
2. **Browsers can't time 8000 Hz.** At 8 kHz reports are 125 µs apart. Browsers batch pointer events to frames and coarsen timestamps to ~100 µs. MouseCheck uses Win32 Raw Input on a dedicated time-critical thread and stamps each report with `QueryPerformanceCounter` (100 ns resolution).
3. **Speed-aware maths.** A mouse only sends a report when it has moved, so an 8000 Hz mouse moved slowly *correctly* sends fewer reports. MouseCheck judges polling only while you were fast enough to need it, and tells you the speed needed at your DPI.

## Limits (no hype)

- Timestamps are when Windows hands the report to the app, not when the sensor read it. End-to-end (click-to-photon) latency needs extra hardware (LDAT / high-speed camera).
- Lift-off distance isn't measured (needs physical spacers).
- Battery level: most wireless gaming mice only expose it through a brand-specific protocol. MouseCheck reads the standard HID battery usages; brand modules are welcome as contributions.
- Battery health and temperature: mice don't report them. Any tool that shows a mouse temperature is guessing.

## Build

Windows 10/11, Rust (stable, MSVC toolchain, which needs the Visual Studio C++ Build Tools).

```bash
cargo build --release
cargo test --release
```

The binary is `target/release/mousecheck.exe`. It's portable: runs are saved to a `sessions/` folder next to the exe as JSON (raw trace + results), so you can share them when asking for help.

## How to test fairly

1. Change **one** thing at a time.
2. Keep DPI, polling rate and your way of moving the same between runs.
3. Do 2–3 runs per condition.
4. Set the **run label** (e.g. "Before cleaning") before starting, then compare in *Results & compare*.

Suggested order for "sometimes smooth, sometimes rough": baseline polling test ×3 → receiver on an extension cable near the mouse → full battery → *Best polling rate* sweep → with the game running in the background → clean the sensor and re-run Accuracy + Swipe.

Saved runs keep their raw reports. When the analysis improves (`ANALYSIS_VERSION` in `analysis.rs`), old runs are recomputed automatically on load, so comparisons stay fair.

## Project layout

```
src/
  capture.rs    Raw Input thread (message-only window, QPC stamps, foreground tracking, CPU sampling)
  analysis.rs   All maths (pure functions + unit tests with synthetic mice)
  findings.rs   Plain-language findings and fixes
  hid.rs        Device names, HID collection inventory, standard battery report
  session.rs    Saved runs (JSON)
  ui/           egui interface, one file per page
```

## Roadmap / contributions welcome

- Brand battery modules (vendor HID protocols) and battery-drain logging for health estimates
- Background "monitor while gaming" mode (needs a design that isn't affected by Windows' background throttling)
- Linux (evdev) and macOS (IOHID) capture backends
- Export a shareable HTML report of a comparison

## License

MIT
