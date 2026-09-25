//! Frame timing, launch time and performance summaries. Frame intervals are
//! app wall-clock pacing between frames (including renderer backpressure), not GPU
//! timestamps.

use crate::{render::GraphicsTuning, shared::AppState, tuning::Tuning};
use bevy::{
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use serde::Serialize;
use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

static PROCESS_START: OnceLock<Instant> = OnceLock::new();

/// Call first thing in `main` so launch time is measured from process start.
pub fn mark_process_start() {
    let _ = PROCESS_START.set(Instant::now());
}

pub fn process_start() -> Instant {
    *PROCESS_START.get_or_init(Instant::now)
}

/// Time from process start to the first frame where the player can act.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct LaunchTime(pub Option<Duration>);

/// One row of the frame log.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct FrameRow {
    pub frame: u64,
    /// Milliseconds since process start at the end of this frame.
    pub t_ms: f64,
    /// Wall time since the previous frame ended.
    pub dt_ms: f64,
    pub fixed_ticks: u32,
    pub playing: bool,
}

/// Per-frame log, recorded when enabled (scenarios, or `--frame-log`).
#[derive(Resource, Debug, Default)]
pub struct FrameLog {
    pub enabled: bool,
    pub rows: Vec<FrameRow>,
}

/// Rolling stats for the in-game performance overlay.
#[derive(Resource, Debug, Default, Clone)]
pub struct FrameStats {
    pub last_dt_ms: f64,
    pub fps: f64,
    /// Worst frame over the last ~5 seconds.
    pub worst_ms: f64,
    recent: std::collections::VecDeque<(f64, f64)>,
}

#[derive(Resource, Debug, Default)]
struct FrameClock {
    frame: u64,
    last: Option<Instant>,
    fixed_ticks: u32,
}

pub struct TelemetryPlugin;

impl Plugin for TelemetryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LaunchTime>()
            .init_resource::<FrameLog>()
            .init_resource::<FrameStats>()
            .init_resource::<FrameClock>()
            .add_systems(FixedFirst, count_fixed_tick)
            .add_systems(Last, (record_frame, detect_controllable));
    }
}

fn count_fixed_tick(mut clock: ResMut<FrameClock>) {
    clock.fixed_ticks += 1;
}

fn record_frame(
    mut clock: ResMut<FrameClock>,
    mut log: ResMut<FrameLog>,
    mut stats: ResMut<FrameStats>,
    state: Res<State<AppState>>,
) {
    let now = Instant::now();
    let dt_ms = clock
        .last
        .map(|last| (now - last).as_secs_f64() * 1000.0)
        .unwrap_or(0.0);
    clock.last = Some(now);
    clock.frame += 1;
    let t_ms = (now - process_start()).as_secs_f64() * 1000.0;
    if log.enabled {
        let row = FrameRow {
            frame: clock.frame,
            t_ms,
            dt_ms,
            fixed_ticks: clock.fixed_ticks,
            playing: *state.get() == AppState::Playing,
        };
        log.rows.push(row);
    }
    clock.fixed_ticks = 0;

    stats.last_dt_ms = dt_ms;
    stats.recent.push_back((t_ms, dt_ms));
    while stats.recent.front().is_some_and(|(t, _)| t_ms - t > 5000.0) {
        stats.recent.pop_front();
    }
    let one_second: Vec<f64> = stats
        .recent
        .iter()
        .filter(|(t, _)| t_ms - t <= 1000.0)
        .map(|(_, dt)| *dt)
        .collect();
    let total: f64 = one_second.iter().sum();
    stats.fps = if total > 0.0 {
        one_second.len() as f64 * 1000.0 / total
    } else {
        0.0
    };
    stats.worst_ms = stats.recent.iter().map(|(_, dt)| *dt).fold(0.0, f64::max);
}

fn detect_controllable(
    mut launch: ResMut<LaunchTime>,
    state: Res<State<AppState>>,
    cursor: Option<Single<&CursorOptions, With<PrimaryWindow>>>,
    scenario: Option<Res<crate::scenario::ScenarioRun>>,
) {
    if launch.0.is_some() || *state.get() != AppState::Playing {
        return;
    }
    let input_live =
        scenario.is_some() || cursor.is_some_and(|c| c.grab_mode == CursorGrabMode::Locked);
    if input_live {
        let elapsed = process_start().elapsed();
        launch.0 = Some(elapsed);
        println!("PIECED_LAUNCH_MS {:.1}", elapsed.as_secs_f64() * 1000.0);
    }
}

/// Summary of frame intervals (milliseconds).
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct FrameSummary {
    pub frames: usize,
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    pub over_18_ms: usize,
    pub over_20_ms: usize,
    pub over_25_ms: usize,
    pub pct_under_18_ms: f64,
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    // Nearest-rank percentile.
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// Summarizes frame intervals. The caller excludes warm-up frames.
pub fn summarize(intervals_ms: &[f64]) -> FrameSummary {
    if intervals_ms.is_empty() {
        return FrameSummary::default();
    }
    let mut sorted = intervals_ms.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let n = sorted.len();
    let under_18 = sorted.iter().filter(|&&d| d < 18.0).count();
    FrameSummary {
        frames: n,
        mean_ms: sorted.iter().sum::<f64>() / n as f64,
        p50_ms: percentile(&sorted, 50.0),
        p95_ms: percentile(&sorted, 95.0),
        p99_ms: percentile(&sorted, 99.0),
        max_ms: sorted[n - 1],
        over_18_ms: sorted.iter().filter(|&&d| d > 18.0).count(),
        over_20_ms: sorted.iter().filter(|&&d| d > 20.0).count(),
        over_25_ms: sorted.iter().filter(|&&d| d > 25.0).count(),
        pct_under_18_ms: under_18 as f64 * 100.0 / n as f64,
    }
}

/// Frame intervals from the log after `warmup_ms` since the first playing frame.
pub fn intervals_after_warmup(rows: &[FrameRow], warmup_ms: f64) -> Vec<f64> {
    let Some(start) = rows.iter().find(|r| r.playing).map(|r| r.t_ms) else {
        return Vec::new();
    };
    rows.iter()
        .filter(|r| r.playing && r.t_ms - start >= warmup_ms && r.dt_ms > 0.0)
        .map(|r| r.dt_ms)
        .collect()
}

/// Writes the frame log as CSV.
pub fn write_csv(rows: &[FrameRow], path: &std::path::Path) -> std::io::Result<()> {
    use std::io::Write;
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    writeln!(out, "frame,t_ms,dt_ms,fixed_ticks,playing")?;
    for r in rows {
        writeln!(
            out,
            "{},{:.3},{:.3},{},{}",
            r.frame, r.t_ms, r.dt_ms, r.fixed_ticks, r.playing as u8
        )?;
    }
    out.flush()
}

/// Power source and Low Power Mode, for evidence records.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PowerState {
    pub source: String,
    pub battery: String,
    pub low_power_mode: Option<bool>,
}

pub fn power_state() -> PowerState {
    let run = |args: &[&str]| {
        std::process::Command::new("pmset")
            .args(args)
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    };
    let batt = run(&["-g", "batt"]);
    let source = batt
        .lines()
        .next()
        .and_then(|l| l.split('\'').nth(1))
        .unwrap_or("unknown")
        .to_string();
    let battery = batt.lines().nth(1).unwrap_or("").trim().to_string();
    let low_power_mode = run(&["-g"])
        .lines()
        .find(|l| l.trim_start().starts_with("lowpowermode"))
        .and_then(|l| l.split_whitespace().nth(1))
        .map(|v| v == "1");
    PowerState {
        source,
        battery,
        low_power_mode,
    }
}

/// Short git commit of the working tree, when available.
pub fn git_commit() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

/// Human description of the active graphics configuration.
pub fn graphics_label(g: &GraphicsTuning) -> String {
    format!(
        "{:?} preset, render scale {:.2}, vsync {}, cap {}",
        g.preset, g.render_scale, g.vsync, g.frame_cap
    )
}

/// Used by scenario summaries.
pub fn tuning_graphics(tuning: &Tuning) -> String {
    graphics_label(&tuning.graphics)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_math() {
        let mut v = vec![16.7; 98];
        v.push(19.0);
        v.push(26.0);
        let s = summarize(&v);
        assert_eq!(s.frames, 100);
        assert_eq!(s.over_25_ms, 1);
        assert_eq!(s.over_20_ms, 1);
        assert_eq!(s.over_18_ms, 2);
        assert!((s.pct_under_18_ms - 98.0).abs() < 1e-9);
        assert_eq!(s.p50_ms, 16.7);
        assert_eq!(s.p99_ms, 19.0);
        assert_eq!(s.max_ms, 26.0);
        assert!((s.mean_ms - (16.7 * 98.0 + 45.0) / 100.0).abs() < 1e-9);
    }

    #[test]
    fn warmup_is_measured_from_first_playing_frame() {
        let rows: Vec<FrameRow> = (0..10)
            .map(|i| FrameRow {
                frame: i,
                t_ms: 1000.0 + i as f64 * 100.0,
                dt_ms: 100.0,
                fixed_ticks: 1,
                playing: i >= 2,
            })
            .collect();
        // First playing frame is at t=1200; warm-up 300 ms keeps t ≥ 1500.
        assert_eq!(intervals_after_warmup(&rows, 300.0).len(), 5);
        assert!(summarize(&[]).frames == 0);
    }
}
