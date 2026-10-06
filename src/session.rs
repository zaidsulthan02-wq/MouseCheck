//! Saved test runs. One JSON file per run in `sessions/` next to the executable, so the
//! tool stays portable and people can share/attach runs when asking for help.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::analysis::{AccuracyReport, ClickReport, DpiReport, MotionReport, Trace};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TestKind {
    Polling,
    Swipe,
    Accuracy,
    Dpi,
    Clicks,
}

impl TestKind {
    pub fn label(&self) -> &'static str {
        match self {
            TestKind::Polling => "Polling & stability",
            TestKind::Swipe => "Swipe / tracking",
            TestKind::Accuracy => "Return-to-origin accuracy",
            TestKind::Dpi => "Real DPI",
            TestKind::Clicks => "Buttons & wheel",
        }
    }
}

/// What the user tells us about the conditions of a run, so before/after makes sense.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Setup {
    /// e.g. "Before cleaning", "New pad", "8000 Hz"
    pub label: String,
    pub dpi: Option<f64>,
    /// Polling rate selected in the mouse software ("" = not set).
    pub polling_setting: String,
    pub surface: String,
    pub notes: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub version: u32,
    pub id: String,
    pub created: String,
    pub kind: TestKind,
    pub setup: Setup,
    pub device: String,
    /// Version of the motion analysis that produced `motion` (missing in old files = 1).
    #[serde(default = "first_version")]
    pub analysis_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<Trace>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion: Option<MotionReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accuracy: Option<AccuracyReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dpi: Option<DpiReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clicks: Option<ClickReport>,
    #[serde(skip)]
    pub file: Option<PathBuf>,
}

impl Session {
    pub fn new(kind: TestKind, setup: Setup, device: String) -> Self {
        let now = chrono::Local::now();
        Self {
            version: 1,
            id: now.format("%Y%m%d-%H%M%S").to_string(),
            created: now.format("%Y-%m-%d %H:%M:%S").to_string(),
            kind,
            setup,
            device,
            analysis_version: crate::analysis::ANALYSIS_VERSION,
            trace: None,
            motion: None,
            accuracy: None,
            dpi: None,
            clicks: None,
            file: None,
        }
    }

    /// Recomputes the motion report from the saved raw trace if it was made by an older
    /// analysis. Context the trace can't reproduce (focus, CPU) is carried over.
    pub fn reanalyze_if_stale(&mut self) -> bool {
        use crate::analysis::{ANALYSIS_VERSION, analyze_motion};
        if self.analysis_version >= ANALYSIS_VERSION {
            return false;
        }
        let (Some(trace), Some(old)) = (&self.trace, &self.motion) else {
            self.analysis_version = ANALYSIS_VERSION;
            return false;
        };
        let mut new = analyze_motion(&trace.samples(), self.setup.dpi);
        new.foreground_pct = old.foreground_pct;
        new.cpu_avg = old.cpu_avg;
        new.cpu_max = old.cpu_max;
        self.motion = Some(new);
        self.analysis_version = ANALYSIS_VERSION;
        true
    }

    pub fn title(&self) -> String {
        let label = if self.setup.label.trim().is_empty() { "untitled" } else { self.setup.label.trim() };
        format!("{} · {}", label, self.kind.label())
    }
}

fn first_version() -> u32 {
    1
}

pub fn dir() -> PathBuf {
    let base = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("sessions")
}

fn slug(s: &str) -> String {
    let s: String = s.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() { "run".into() } else { s.chars().take(40).collect() }
}

pub fn save(s: &mut Session) -> std::io::Result<PathBuf> {
    let d = dir();
    std::fs::create_dir_all(&d)?;
    let kind = format!("{:?}", s.kind).to_lowercase();
    let path = d.join(format!("{}_{}_{}.json", s.id, kind, slug(&s.setup.label)));
    let json = serde_json::to_string(s).map_err(std::io::Error::other)?;
    std::fs::write(&path, json)?;
    s.file = Some(path.clone());
    Ok(path)
}

pub fn delete(s: &Session) {
    if let Some(p) = &s.file {
        let _ = std::fs::remove_file(p);
    }
}

pub fn load_all() -> Vec<Session> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir()) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "json") {
            if let Ok(txt) = std::fs::read_to_string(&p) {
                if let Ok(mut s) = serde_json::from_str::<Session>(&txt) {
                    if s.reanalyze_if_stale() {
                        if let Ok(json) = serde_json::to_string(&s) {
                            let _ = std::fs::write(&p, json);
                        }
                    }
                    s.file = Some(p);
                    out.push(s);
                }
            }
        }
    }
    out.sort_by(|a, b| b.id.cmp(&a.id));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Re-analyses real saved runs and prints old vs new results. Not part of the normal
    /// suite (needs data): `MOUSECHECK_SESSIONS=dist/sessions cargo test real_sessions -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_sessions() {
        let dir = std::env::var("MOUSECHECK_SESSIONS").unwrap_or_else(|_| "dist/sessions".into());
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let txt = std::fs::read_to_string(e.path()).unwrap();
            let mut s: Session = serde_json::from_str(&txt).unwrap();
            let Some(old) = s.motion.clone() else { continue };
            s.analysis_version = 0;
            s.reanalyze_if_stale();
            let new = s.motion.unwrap();
            println!(
                "{:?} {:>5.0} Hz  hitches {:>3} -> {:>3}  (stall {} link {} lost {})  worst {:.1} -> {:.1} ms",
                s.kind,
                new.nominal_hz,
                old.hitches.len(),
                new.hitches.len(),
                new.pc_stalls,
                new.link_delays,
                new.lost,
                old.max_gap_ms,
                new.max_gap_ms
            );
        }
    }
}
