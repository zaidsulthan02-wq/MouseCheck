//! Measurement maths. Pure functions over recorded samples, so everything here is
//! unit-tested with synthetic data and can be re-run on saved sessions.
//!
//! Key idea: a mouse only sends a report when it has something to say. At 8000 Hz a
//! report is due every 125 µs, but only if the sensor moved at least one count in that
//! window. So "polling rate" can only be judged while you move fast enough, and every
//! metric below is computed relative to how fast you were actually moving.

use serde::{Deserialize, Serialize};

/// Serde helper: NaN ("not measured") <-> JSON null.
mod nan {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
        if v.is_finite() { s.serialize_f64(*v) } else { s.serialize_none() }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
        Ok(Option::<f64>::deserialize(d)?.unwrap_or(f64::NAN))
    }
}

/// Bump when the motion maths changes. Saved runs keep their raw trace, so older runs are
/// recomputed automatically on load and stay comparable with new ones.
pub const ANALYSIS_VERSION: u32 = 2;

pub const RATES: [f64; 7] = [125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0];

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    /// Microseconds since the start of the recording.
    pub t: f64,
    pub dx: i32,
    pub dy: i32,
    pub bf: u16,
    pub bd: i16,
}

impl Sample {
    pub fn moved(&self) -> bool {
        self.dx != 0 || self.dy != 0
    }
    fn mag(&self) -> f64 {
        ((self.dx as f64).powi(2) + (self.dy as f64).powi(2)).sqrt()
    }
    fn cheb(&self) -> i32 {
        self.dx.abs().max(self.dy.abs())
    }
}

/// Compact, column-oriented storage for sessions on disk.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Trace {
    pub t: Vec<f64>,
    pub dx: Vec<i32>,
    pub dy: Vec<i32>,
    pub bf: Vec<u16>,
    pub bd: Vec<i16>,
}

impl Trace {
    pub fn from_samples(s: &[Sample]) -> Self {
        Self {
            t: s.iter().map(|x| (x.t * 10.0).round() / 10.0).collect(),
            dx: s.iter().map(|x| x.dx).collect(),
            dy: s.iter().map(|x| x.dy).collect(),
            bf: s.iter().map(|x| x.bf).collect(),
            bd: s.iter().map(|x| x.bd).collect(),
        }
    }
    /// Inverse of `from_samples`, for re-analysing saved runs.
    pub fn samples(&self) -> Vec<Sample> {
        (0..self.t.len())
            .map(|i| Sample {
                t: self.t[i],
                dx: self.dx[i],
                dy: self.dy[i],
                bf: self.bf.get(i).copied().unwrap_or(0),
                bd: self.bd.get(i).copied().unwrap_or(0),
            })
            .collect()
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let idx = ((sorted.len() - 1) as f64 * p / 100.0).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn sorted(mut v: Vec<f64>) -> Vec<f64> {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

fn median(v: &[f64]) -> f64 {
    percentile(&sorted(v.to_vec()), 50.0)
}

/// Snaps a measured rate to the nearest standard polling rate (within ±25 %).
pub fn snap_rate(hz: f64) -> (f64, bool) {
    let best =
        RATES.iter().copied().min_by(|a, b| (hz / a).ln().abs().partial_cmp(&(hz / b).ln().abs()).unwrap()).unwrap();
    if (hz / best).ln().abs() < 1.25f64.ln() { (best, true) } else { (hz, false) }
}

pub fn cps_to_cm_s(cps: f64, dpi: f64) -> f64 {
    cps / dpi * 2.54
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HitchKind {
    /// Reports were held up on the PC side (USB / driver / CPU) and arrived in a burst.
    PcStall,
    /// The receiver delivered nothing, then one report containing all the accumulated
    /// motion: the mouse/radio link was late (interference, distance, low battery).
    LinkDelay,
    /// Motion went missing and was never caught up: sensor lost tracking (lift, dirt,
    /// surface) or the radio dropped data.
    Lost,
}

impl HitchKind {
    pub fn label(&self) -> &'static str {
        match self {
            HitchKind::PcStall => "PC-side stall",
            HitchKind::LinkDelay => "Wireless/mouse delay",
            HitchKind::Lost => "Lost motion",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Hitch {
    pub t_s: f64,
    pub gap_ms: f64,
    pub kind: HitchKind,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MotionReport {
    pub ok: bool,
    pub message: String,
    pub duration_s: f64,
    pub motion_time_s: f64,
    pub reports: usize,
    pub motion_reports: usize,

    // Polling
    pub measured_hz: f64,
    pub nominal_hz: f64,
    pub nominal_snapped: bool,
    pub saturated_intervals: usize,
    pub on_time_pct: f64,
    pub bunched_pct: f64,
    pub late_pct: f64,
    #[serde(with = "nan", default)]
    pub jitter_p50_us: f64,
    #[serde(with = "nan", default)]
    pub jitter_p99_us: f64,
    /// Share of 20 ms windows (while moving fast enough) that reached ≥85 % of the nominal rate.
    #[serde(with = "nan", default)]
    pub rate_held_pct: f64,
    /// Share of moving time you were fast enough for the mouse to *need* the full rate.
    pub fast_time_pct: f64,

    // Hitches (gaps you can feel)
    pub hitches: Vec<Hitch>,
    pub hitches_per_min: f64,
    pub pc_stalls: usize,
    pub link_delays: usize,
    pub lost: usize,
    pub max_gap_ms: f64,

    // Sensor / tracking
    pub reversals_per_10k: f64,
    pub spikes_per_10k: f64,
    #[serde(with = "nan", default)]
    pub velocity_noise_pct: f64,
    pub peak_speed_cps: f64,
    pub median_speed_cps: f64,

    // Context
    pub dpi: Option<f64>,
    pub foreground_pct: f64,
    pub cpu_avg: f32,
    pub cpu_max: f32,

    // Charts
    pub rate_series: Vec<[f64; 2]>,
    pub speed_series: Vec<[f64; 2]>,
    pub interval_hist: Vec<[f64; 2]>,
}

impl MotionReport {
    /// Counts/second the mouse must move for the nominal rate to be fully used.
    pub fn needed_cps(&self) -> f64 {
        self.nominal_hz
    }
}

const PAUSE_US: f64 = 50_000.0;
/// "Lost motion" is only judged during fast motion (≈8 cm/s at 1600 DPI), where a hand
/// can't stop and resume at the exact same speed and direction.
const LOST_MIN_CPS: f64 = 5_000.0;

/// Local speed (counts per second) over up to `k` reports ending at `end` (inclusive),
/// walking backwards. Stops at pauses.
fn speed_back(m: &[Sample], end: usize, k: usize) -> f64 {
    let mut sum = 0.0;
    let mut j = end;
    let mut n = 0;
    while j > 0 && n < k {
        if m[j].t - m[j - 1].t > 20_000.0 {
            break;
        }
        sum += m[j].cheb() as f64;
        j -= 1;
        n += 1;
    }
    let span = m[end].t - m[j].t;
    if n < 3 || span <= 0.0 { 0.0 } else { sum / (span / 1e6) }
}

pub fn analyze_motion(all: &[Sample], dpi: Option<f64>) -> MotionReport {
    let mut r = MotionReport { dpi, reports: all.len(), ..Default::default() };
    let m: Vec<Sample> = all.iter().copied().filter(|s| s.moved()).collect();
    r.motion_reports = m.len();
    if m.len() < 200 {
        r.message = "Not enough movement was recorded. Keep the mouse moving for the whole test.".into();
        return r;
    }
    r.duration_s = (m.last().unwrap().t - m[0].t) / 1e6;
    let n = m.len();
    let dt: Vec<f64> = (0..n - 1).map(|i| m[i + 1].t - m[i].t).collect();
    r.motion_time_s = dt.iter().filter(|&&d| d < PAUSE_US).fold(0.0, |a, b| a + b) / 1e6;

    // --- Nominal polling interval -------------------------------------------------
    // Intervals between two reports that each carried ≥2 counts: the mouse had data for
    // every poll, so the interval should equal the polling period.
    let sat_idx: Vec<usize> =
        (0..n - 1).filter(|&i| m[i].cheb() >= 2 && m[i + 1].cheb() >= 2 && dt[i] < 20_000.0).collect();
    let base = if sat_idx.len() >= 200 {
        median(&sat_idx.iter().map(|&i| dt[i]).collect::<Vec<_>>())
    } else {
        let s = sorted(dt.iter().copied().filter(|&d| d < 20_000.0).collect());
        let low = &s[..(s.len() / 5).max(1)];
        median(low)
    };
    r.measured_hz = 1e6 / base.max(1.0);
    let (nom, snapped) = snap_rate(r.measured_hz);
    r.nominal_hz = nom;
    r.nominal_snapped = snapped;
    let nom_us = 1e6 / nom;

    // --- Interval classification -----------------------------------------------------
    r.saturated_intervals = sat_idx.len();
    if !sat_idx.is_empty() {
        let (mut b, mut o, mut l) = (0usize, 0usize, 0usize);
        let mut jit = Vec::with_capacity(sat_idx.len());
        for &i in &sat_idx {
            let ratio = dt[i] / nom_us;
            if ratio < 0.5 {
                b += 1
            } else if ratio < 1.5 {
                o += 1
            } else {
                l += 1
            }
            if ratio < 4.0 {
                jit.push((dt[i] - nom_us).abs());
            }
        }
        let tot = sat_idx.len() as f64;
        r.bunched_pct = b as f64 / tot * 100.0;
        r.on_time_pct = o as f64 / tot * 100.0;
        r.late_pct = l as f64 / tot * 100.0;
        let js = sorted(jit);
        r.jitter_p50_us = percentile(&js, 50.0);
        r.jitter_p99_us = percentile(&js, 99.0);

        // Histogram: 0 .. 3x nominal, 24 bins + overflow.
        let bins = 24usize;
        let w = nom_us * 3.0 / bins as f64;
        let mut h = vec![0usize; bins + 1];
        for &i in &sat_idx {
            let k = ((dt[i] / w) as usize).min(bins);
            h[k] += 1;
        }
        r.interval_hist = h.iter().enumerate().map(|(k, &c)| [(k as f64 + 0.5) * w, c as f64 / tot * 100.0]).collect();
    }

    // --- Hitches ------------------------------------------------------------------------
    // A gap only counts when the hand was moving *steadily through it*: same direction,
    // similar speed before and after, not slowing into it or speeding up out of it.
    // Real hands pause, reverse and hesitate all the time, and the mouse correctly sends
    // nothing then. Those are not faults.
    let mut px = vec![0.0f64; n + 1];
    let mut py = vec![0.0f64; n + 1];
    let mut pc = vec![0.0f64; n + 1];
    for (k, s) in m.iter().enumerate() {
        px[k + 1] = px[k] + s.dx as f64;
        py[k + 1] = py[k] + s.dy as f64;
        pc[k + 1] = pc[k] + s.cheb() as f64;
    }
    // Sums over samples with lo < t <= hi.
    let window = |lo: f64, hi: f64| {
        let a = m.partition_point(|s| s.t <= lo);
        let b = m.partition_point(|s| s.t <= hi);
        (px[b] - px[a], py[b] - py[a], pc[b] - pc[a])
    };
    let w = (16.0 * nom_us).max(2_000.0);
    let per_s = 1e6 / w;
    let mut hitches = Vec::new();
    for i in 0..n - 1 {
        let gap = dt[i];
        if !(2_000.0..1_000_000.0).contains(&gap) {
            continue;
        }
        let (ta, tb) = (m[i].t, m[i + 1].t);
        let (bx, by, bc) = window(ta - w, ta);
        let (_, _, prev_c) = window(ta - 2.0 * w, ta - w);
        let (ax, ay, ac) = window(tb, tb + w); // excludes the first report after the gap
        let (_, _, next_c) = window(tb + w, tb + 2.0 * w);
        let (v_before, v_after) = (bc * per_s, ac * per_s);
        let speed = v_before.min(v_after);
        if speed < 300.0 {
            continue; // effectively stopped
        }
        // When slower than one count per poll, the mouse legitimately skips polls.
        let expected = nom_us.max(1e6 / speed);
        if gap < (4.0 * expected).max(2_000.0) {
            continue;
        }
        let (lb, la) = ((bx * bx + by * by).sqrt(), (ax * ax + ay * ay).sqrt());
        let cos = if lb > 0.0 && la > 0.0 { (bx * ax + by * ay) / (lb * la) } else { -1.0 };
        if cos < 0.7 {
            continue; // direction changed: a turn or reversal, the hand paused
        }
        let missed = gap / expected - 1.0;
        let look = (missed.ceil() as usize).clamp(1, 64);
        let bunched = (i + 1..(i + 1 + look).min(n - 1)).filter(|&j| dt[j] < 0.5 * nom_us).count();
        let kind = if bunched as f64 >= (0.3 * missed).max(1.0) {
            // A burst of late reports never happens naturally after a pause.
            HitchKind::PcStall
        } else {
            let steady = bc >= 0.6 * prev_c && ac >= 0.6 * next_c && ac >= 0.5 * bc && ac <= 2.0 * bc;
            if !steady {
                continue; // slowing into / speeding out of the gap: natural hesitation
            }
            let med_mag = {
                let lo = i.saturating_sub(8);
                median(&m[lo..=i].iter().map(|s| s.cheb() as f64).collect::<Vec<_>>())
            };
            if m[i + 1].cheb() as f64 >= (0.4 * (missed + 1.0) * med_mag).max(3.0) {
                HitchKind::LinkDelay
            } else if speed >= LOST_MIN_CPS {
                HitchKind::Lost
            } else {
                // Slow micro-adjustment: a pause and missing data look identical, and a
                // pause is far more likely. Without positive evidence, don't accuse the mouse.
                continue;
            }
        };
        hitches.push(Hitch { t_s: (m[i].t - m[0].t) / 1e6, gap_ms: gap / 1000.0, kind });
    }
    r.pc_stalls = hitches.iter().filter(|h| h.kind == HitchKind::PcStall).count();
    r.link_delays = hitches.iter().filter(|h| h.kind == HitchKind::LinkDelay).count();
    r.lost = hitches.iter().filter(|h| h.kind == HitchKind::Lost).count();
    r.max_gap_ms = hitches.iter().map(|h| h.gap_ms).fold(0.0, f64::max);
    r.hitches_per_min = if r.motion_time_s > 0.0 { hitches.len() as f64 / r.motion_time_s * 60.0 } else { 0.0 };
    hitches.truncate(2000);
    r.hitches = hitches;

    // --- 20 ms windows: rate & speed over time ------------------------------------------
    let win = 20_000.0;
    let t0 = m[0].t;
    let nw = ((m[n - 1].t - t0) / win).ceil() as usize + 1;
    let mut cnt = vec![0usize; nw];
    let mut mag = vec![0.0f64; nw];
    let mut cheb = vec![0.0f64; nw];
    for s in &m {
        let k = ((s.t - t0) / win) as usize;
        cnt[k] += 1;
        mag[k] += s.mag();
        cheb[k] += s.cheb() as f64;
    }
    let (mut moving_w, mut fast_w, mut held_w) = (0usize, 0usize, 0usize);
    let mut speeds = Vec::new();
    for k in 0..nw {
        if cnt[k] < 2 {
            continue;
        }
        moving_w += 1;
        let tm = (k as f64 + 0.5) * win / 1e6;
        let rate = cnt[k] as f64 / (win / 1e6);
        let sp = mag[k] / (win / 1e6);
        r.rate_series.push([tm, rate]);
        r.speed_series.push([tm, sp]);
        speeds.push(sp);
        if cheb[k] / (win / 1e6) >= 1.5 * nom {
            fast_w += 1;
            if rate >= 0.85 * nom {
                held_w += 1;
            }
        }
    }
    r.fast_time_pct = if moving_w > 0 { fast_w as f64 / moving_w as f64 * 100.0 } else { 0.0 };
    r.rate_held_pct = if fast_w > 0 { held_w as f64 / fast_w as f64 * 100.0 } else { f64::NAN };
    let ss = sorted(speeds);
    r.peak_speed_cps = ss.last().copied().unwrap_or(0.0);
    r.median_speed_cps = percentile(&ss, 50.0);

    // --- Sensor: reversals & spikes -----------------------------------------------------
    let (mut rev, mut spk, mut eligible) = (0usize, 0usize, 0usize);
    const K: usize = 8;
    for i in K..n.saturating_sub(K) {
        if dt[i - K..i + K].iter().any(|&d| d > 20_000.0) {
            continue;
        }
        eligible += 1;
        let (mut sx, mut sy) = (0.0, 0.0);
        let mut mags = Vec::with_capacity(2 * K);
        for j in i - K..=i + K {
            if j == i {
                continue;
            }
            sx += m[j].dx as f64;
            sy += m[j].dy as f64;
            mags.push(m[j].mag());
        }
        let s_len = (sx * sx + sy * sy).sqrt();
        let di = m[i].mag();
        if s_len >= 24.0 && m[i].cheb() >= 2 {
            let cos = (m[i].dx as f64 * sx + m[i].dy as f64 * sy) / (di * s_len);
            if cos < -0.3 {
                rev += 1;
            }
        }
        let med = median(&mags);
        let after_gap = dt[i - 1] > 1.5 * nom_us.max(1e6 / speed_back(&m, i - 1, 8).max(1.0));
        if di >= 10.0 && di > 4.0 * med.max(1.0) && !after_gap {
            spk += 1;
        }
    }
    if eligible > 0 {
        r.reversals_per_10k = rev as f64 / eligible as f64 * 1e4;
        r.spikes_per_10k = spk as f64 / eligible as f64 * 1e4;
    }

    // --- Velocity noise ---------------------------------------------------------------
    let bin = (8.0 * nom_us).max(2_000.0);
    let nb = ((m[n - 1].t - t0) / bin).ceil() as usize + 1;
    let mut v = vec![0.0f64; nb];
    for s in &m {
        v[((s.t - t0) / bin) as usize] += s.mag();
    }
    let mut devs = Vec::new();
    for b in 3..nb.saturating_sub(3) {
        let w = &v[b - 3..=b + 3];
        if w.iter().any(|&x| x == 0.0) {
            continue;
        }
        let sm = w.iter().fold(0.0, |a, b| a + b) / 7.0;
        if sm < 30.0 {
            continue;
        }
        devs.push((v[b] - sm).abs() / sm);
    }
    r.velocity_noise_pct = if devs.is_empty() { f64::NAN } else { median(&devs) * 100.0 };

    r.ok = true;
    r
}

// ===================================================================================
// Accuracy: return-to-origin and DPI
// ===================================================================================

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Round {
    pub sum_dx: i64,
    pub sum_dy: i64,
    pub path: f64,
    pub duration_s: f64,
}

impl Round {
    #[allow(dead_code)]
    pub fn from_samples(s: &[Sample]) -> Self {
        let mut r = Round::default();
        for x in s {
            r.sum_dx += x.dx as i64;
            r.sum_dy += x.dy as i64;
            r.path += x.mag();
        }
        if let (Some(a), Some(b)) = (s.first(), s.last()) {
            r.duration_s = (b.t - a.t) / 1e6;
        }
        r
    }
    pub fn error(&self) -> f64 {
        ((self.sum_dx as f64).powi(2) + (self.sum_dy as f64).powi(2)).sqrt()
    }
    pub fn error_pct(&self) -> f64 {
        if self.path > 0.0 { self.error() / self.path * 100.0 } else { 0.0 }
    }
    pub fn net(&self) -> f64 {
        self.error()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AccuracyReport {
    pub rounds: Vec<Round>,
    pub mean_err_counts: f64,
    pub max_err_counts: f64,
    pub mean_err_pct: f64,
    pub dpi: Option<f64>,
}

impl AccuracyReport {
    pub fn from_rounds(rounds: Vec<Round>, dpi: Option<f64>) -> Self {
        let n = rounds.len().max(1) as f64;
        Self {
            mean_err_counts: rounds.iter().map(|r| r.error()).fold(0.0, |a, b| a + b) / n,
            max_err_counts: rounds.iter().map(|r| r.error()).fold(0.0, f64::max),
            mean_err_pct: rounds.iter().map(|r| r.error_pct()).fold(0.0, |a, b| a + b) / n,
            rounds,
            dpi,
        }
    }
    pub fn mean_err_mm(&self) -> Option<f64> {
        self.dpi.map(|d| self.mean_err_counts / d * 25.4)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DpiRun {
    pub distance_mm: f64,
    pub round: Round,
}

impl DpiRun {
    pub fn dpi(&self) -> f64 {
        self.round.net() / (self.distance_mm / 25.4)
    }
    /// Angle of travel in degrees (0 = right, 90 = down). Big values = not along the ruler.
    pub fn angle(&self) -> f64 {
        (self.round.sum_dy as f64).atan2(self.round.sum_dx as f64).to_degrees()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DpiReport {
    pub runs: Vec<DpiRun>,
    pub mean_dpi: f64,
    pub spread_pct: f64,
    pub set_dpi: Option<f64>,
}

impl DpiReport {
    pub fn from_runs(runs: Vec<DpiRun>, set_dpi: Option<f64>) -> Self {
        let d: Vec<f64> = runs.iter().map(|r| r.dpi()).collect();
        let mean = if d.is_empty() { 0.0 } else { d.iter().fold(0.0, |a, b| a + b) / d.len() as f64 };
        let spread = if d.len() > 1 && mean > 0.0 {
            (d.iter().cloned().fold(f64::MIN, f64::max) - d.iter().cloned().fold(f64::MAX, f64::min)) / mean * 100.0
        } else {
            0.0
        };
        Self { runs, mean_dpi: mean, spread_pct: spread, set_dpi }
    }
    pub fn deviation_pct(&self) -> Option<f64> {
        self.set_dpi.filter(|&s| s > 0.0 && self.mean_dpi > 0.0).map(|s| (self.mean_dpi - s) / s * 100.0)
    }
}

// ===================================================================================
// Buttons & wheel
// ===================================================================================

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ButtonStats {
    pub name: String,
    pub presses: u32,
    #[serde(with = "nan", default)]
    pub hold_min_ms: f64,
    #[serde(with = "nan", default)]
    pub hold_median_ms: f64,
    #[serde(with = "nan", default)]
    pub min_gap_ms: f64,
    /// Press that followed the previous release faster than the threshold.
    pub chatter: u32,
    pub chatter_gaps_ms: Vec<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WheelStats {
    pub up: u32,
    pub down: u32,
    /// Notches in the opposite direction in the middle of a one-way scroll (encoder wear).
    pub wrong_way: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ClickReport {
    pub threshold_ms: f64,
    pub buttons: Vec<ButtonStats>,
    pub wheel: WheelStats,
}

pub fn analyze_clicks(s: &[Sample], threshold_ms: f64) -> ClickReport {
    use crate::capture::btn;
    let mut rep = ClickReport { threshold_ms, ..Default::default() };
    for (b, &(down, up)) in btn::PAIRS.iter().enumerate() {
        let mut st = ButtonStats { name: btn::NAMES[b].into(), min_gap_ms: f64::NAN, ..Default::default() };
        let mut pressed_at: Option<f64> = None;
        let mut released_at: Option<f64> = None;
        let mut holds = Vec::new();
        for x in s {
            if x.bf & down != 0 {
                st.presses += 1;
                if let Some(rel) = released_at {
                    let gap = (x.t - rel) / 1000.0;
                    st.min_gap_ms = if st.min_gap_ms.is_nan() { gap } else { st.min_gap_ms.min(gap) };
                    if gap < threshold_ms {
                        st.chatter += 1;
                        st.chatter_gaps_ms.push(gap);
                    }
                }
                pressed_at = Some(x.t);
            }
            if x.bf & up != 0 {
                if let Some(p) = pressed_at.take() {
                    holds.push((x.t - p) / 1000.0);
                }
                released_at = Some(x.t);
            }
        }
        let hs = sorted(holds);
        st.hold_min_ms = hs.first().copied().unwrap_or(f64::NAN);
        st.hold_median_ms = percentile(&hs, 50.0);
        rep.buttons.push(st);
    }
    // Wheel
    let notches: Vec<(f64, i32)> = s
        .iter()
        .filter(|x| x.bf & btn::WHEEL != 0)
        .map(|x| (x.t, (x.bd as i32).signum()))
        .filter(|x| x.1 != 0)
        .collect();
    for (i, &(t, dir)) in notches.iter().enumerate() {
        if dir > 0 {
            rep.wheel.up += 1
        } else {
            rep.wheel.down += 1
        }
        // Neighbours within 400 ms that mostly go the other way.
        let near: Vec<i32> = notches
            .iter()
            .enumerate()
            .filter(|(j, (tj, _))| *j != i && (tj - t).abs() < 400_000.0)
            .map(|(_, (_, d))| *d)
            .collect();
        if near.len() >= 3 {
            let opp = near.iter().filter(|&&d| d == -dir).count();
            if opp as f64 >= 0.75 * near.len() as f64 {
                rep.wheel.wrong_way += 1;
            }
        }
    }
    rep
}

// ===================================================================================
// Tests with synthetic mice
// ===================================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Simple deterministic PRNG so tests don't need a crate.
    struct Lcg(u64);
    impl Lcg {
        fn f(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    /// Synthetic mouse: polls at `hz`, moves at `cps` counts/s along x with sinusoidal
    /// speed, sends a report only when it has accumulated at least one count.
    fn synth(hz: f64, cps: f64, seconds: f64, jitter_us: f64, seed: u64) -> Vec<Sample> {
        let mut rng = Lcg(seed);
        let period = 1e6 / hz;
        let mut out = Vec::new();
        let mut acc = 0.0f64;
        let polls = (seconds * hz) as usize;
        for p in 0..polls {
            let t = p as f64 * period;
            let speed = cps * (0.75 + 0.25 * (t / 1e6 * 3.0).sin());
            acc += speed / hz;
            let whole = acc.floor();
            if whole >= 1.0 {
                acc -= whole;
                let j = (rng.f() - 0.5) * jitter_us;
                out.push(Sample { t: t + j, dx: whole as i32, dy: 0, bf: 0, bd: 0 });
            }
        }
        out.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
        out
    }

    /// Synthetic 2-D mouse driven by a signed velocity function (counts/s per axis).
    /// Sends a report only when an axis accumulated at least one whole count, like a real mouse.
    fn synth_v(hz: f64, seconds: f64, v: impl Fn(f64) -> (f64, f64)) -> Vec<Sample> {
        let period = 1e6 / hz;
        let (mut ax, mut ay) = (0.0f64, 0.0f64);
        let mut out = Vec::new();
        for p in 0..(seconds * hz) as usize {
            let t = p as f64 * period;
            let (vx, vy) = v(t / 1e6);
            ax += vx / hz;
            ay += vy / hz;
            let (wx, wy) = (ax.trunc(), ay.trunc());
            if wx != 0.0 || wy != 0.0 {
                ax -= wx;
                ay -= wy;
                out.push(Sample { t, dx: wx as i32, dy: wy as i32, bf: 0, bd: 0 });
            }
        }
        out
    }

    const TAU: f64 = std::f64::consts::TAU;

    #[test]
    fn swipe_reversals_are_not_hitches() {
        // Left/right swipes at 1.5 Hz: speed passes through zero and direction flips.
        for hz in [1000.0, 2000.0, 4000.0, 8000.0] {
            let s = synth_v(hz, 6.0, |t| (60_000.0 * (TAU * 1.5 * t).sin(), 3_000.0 * (TAU * 0.7 * t).sin()));
            let r = analyze_motion(&s, None);
            assert!(r.hitches.is_empty(), "{hz} Hz: {:?}", &r.hitches[..r.hitches.len().min(5)]);
        }
    }

    #[test]
    fn slowing_down_and_holding_still_are_not_hitches() {
        // Same direction, but speed dips to almost zero (hesitation), plus full stops.
        let s = synth_v(8000.0, 6.0, |t| {
            let phase = (t * 2.0).fract();
            if phase > 0.8 { (0.0, 0.0) } else { (-40_000.0 * (TAU * 0.5 * phase / 0.8).sin().powi(2), 0.0) }
        });
        let r = analyze_motion(&s, None);
        assert!(r.hitches.is_empty(), "{:?}", &r.hitches[..r.hitches.len().min(5)]);
    }

    #[test]
    fn slow_pause_in_same_direction_is_not_a_hitch() {
        // Taken from a real swipe run: ~1500 counts/s, 195 ms pause, same direction after.
        let s = synth_v(4000.0, 3.0, |t| if (1.0..1.195).contains(&t) { (0.0, 0.0) } else { (1_500.0, 300.0) });
        let r = analyze_motion(&s, None);
        assert!(r.hitches.is_empty(), "{:?}", r.hitches);
    }

    #[test]
    fn data_lost_mid_swipe_is_still_caught() {
        let mut s = synth_v(8000.0, 6.0, |t| (60_000.0 * (TAU * 0.5 * t).sin(), 0.0));
        // Remove 3 ms around 0.5 s and 2.5 s (peak speed, steady motion).
        for c in [500_000.0, 2_500_000.0] {
            s.retain(|x| !(x.t >= c && x.t < c + 3_000.0));
        }
        let r = analyze_motion(&s, None);
        assert_eq!(r.hitches.len(), 2, "{:?}", r.hitches);
        assert_eq!(r.lost, 2);
    }

    #[test]
    fn detects_8k_and_clean_stream() {
        let s = synth(8000.0, 40_000.0, 5.0, 20.0, 1);
        let r = analyze_motion(&s, Some(1600.0));
        assert!(r.ok, "{}", r.message);
        assert_eq!(r.nominal_hz, 8000.0);
        assert!(r.on_time_pct > 95.0, "on time {}", r.on_time_pct);
        assert!(r.hitches.is_empty(), "{:?}", &r.hitches[..r.hitches.len().min(5)]);
        assert!(r.rate_held_pct > 95.0, "held {}", r.rate_held_pct);
        assert!(r.reversals_per_10k < 1.0);
        assert!(r.spikes_per_10k < 1.0);
    }

    #[test]
    fn detects_1k() {
        let s = synth(1000.0, 8_000.0, 5.0, 30.0, 2);
        let r = analyze_motion(&s, None);
        assert_eq!(r.nominal_hz, 1000.0);
        assert!(r.hitches.is_empty());
    }

    #[test]
    fn slow_motion_is_not_a_hitch() {
        // 8 kHz mouse moved slowly (500 counts/s): legitimately skips most polls.
        let s = synth(8000.0, 500.0, 5.0, 10.0, 3);
        let r = analyze_motion(&s, None);
        assert!(r.hitches.is_empty(), "{} hitches", r.hitches.len());
    }

    #[test]
    fn classifies_link_delay_pc_stall_and_lost() {
        let mut s = synth(8000.0, 40_000.0, 6.0, 10.0, 4);
        // 1) Link delay at ~1 s: remove 3 ms of reports, add their motion to the next one.
        let (a, b) = (1_000_000.0, 1_003_000.0);
        let removed: i32 = s.iter().filter(|x| x.t >= a && x.t < b).map(|x| x.dx).sum();
        s.retain(|x| !(x.t >= a && x.t < b));
        let k = s.iter().position(|x| x.t >= b).unwrap();
        s[k].dx += removed;

        // 2) PC stall at ~2 s: hold 3 ms of reports and deliver them in a burst at the end.
        let (a, b) = (2_000_000.0, 2_003_000.0);
        let mut c = 0.0;
        for x in s.iter_mut().filter(|x| x.t >= a && x.t < b) {
            x.t = b + c;
            c += 2.0;
        }
        s.sort_by(|p, q| p.t.partial_cmp(&q.t).unwrap());

        // 3) Lost motion at ~3 s: reports and their motion simply vanish.
        let (a, b) = (3_000_000.0, 3_004_000.0);
        s.retain(|x| !(x.t >= a && x.t < b));

        let r = analyze_motion(&s, None);
        let kinds: Vec<_> = r.hitches.iter().map(|h| (h.t_s.round(), h.kind)).collect();
        assert_eq!(r.hitches.len(), 3, "{kinds:?}");
        assert_eq!(r.link_delays, 1, "{kinds:?}");
        assert_eq!(r.pc_stalls, 1, "{kinds:?}");
        assert_eq!(r.lost, 1, "{kinds:?}");
    }

    #[test]
    fn detects_sensor_reversals_and_spikes() {
        let mut s = synth(1000.0, 20_000.0, 5.0, 5.0, 5);
        for (i, x) in s.iter_mut().enumerate() {
            if i % 200 == 100 {
                x.dx = -x.dx; // jitter backwards
            }
            if i % 200 == 150 {
                x.dx *= 8; // jump
            }
        }
        let r = analyze_motion(&s, None);
        assert!(r.reversals_per_10k > 30.0, "rev {}", r.reversals_per_10k);
        assert!(r.spikes_per_10k > 30.0, "spk {}", r.spikes_per_10k);
    }

    #[test]
    fn clicks_and_chatter() {
        use crate::capture::btn;
        let mut s = Vec::new();
        let mut t = 0.0;
        for i in 0..10 {
            s.push(Sample { t, bf: btn::PAIRS[0].0, ..Default::default() });
            s.push(Sample { t: t + 60_000.0, bf: btn::PAIRS[0].1, ..Default::default() });
            if i == 5 {
                // switch bounce: second press 15 ms after release
                s.push(Sample { t: t + 75_000.0, bf: btn::PAIRS[0].0, ..Default::default() });
                s.push(Sample { t: t + 90_000.0, bf: btn::PAIRS[0].1, ..Default::default() });
            }
            t += 500_000.0;
        }
        let r = analyze_clicks(&s, 80.0);
        assert_eq!(r.buttons[0].presses, 11);
        assert_eq!(r.buttons[0].chatter, 1);
        assert!((r.buttons[0].min_gap_ms - 15.0).abs() < 0.01);
    }

    #[test]
    fn wheel_wrong_way() {
        let mut s = Vec::new();
        for i in 0..20 {
            let d: i16 = if i == 10 { 120 } else { -120 };
            s.push(Sample { t: i as f64 * 80_000.0, bf: crate::capture::btn::WHEEL, bd: d, ..Default::default() });
        }
        let r = analyze_clicks(&s, 80.0);
        assert_eq!(r.wheel.down, 19);
        assert_eq!(r.wheel.wrong_way, 1);
    }

    #[test]
    fn accuracy_round() {
        let s = vec![
            Sample { t: 0.0, dx: 100, dy: 0, ..Default::default() },
            Sample { t: 1.0, dx: -98, dy: 1, ..Default::default() },
        ];
        let r = Round::from_samples(&s);
        assert_eq!((r.sum_dx, r.sum_dy), (2, 1));
        assert!((r.error_pct() - 5f64.sqrt() / (100.0 + (98f64 * 98.0 + 1.0).sqrt()) * 100.0).abs() < 1e-9);
    }

    #[test]
    fn snapping() {
        assert_eq!(snap_rate(7950.0), (8000.0, true));
        assert_eq!(snap_rate(1012.0), (1000.0, true));
        assert!(!snap_rate(1400.0).1);
    }
}
