//! Frame timing, launch time and performance summaries. Frame intervals are
//! app wall-clock pacing between frames (including renderer backpressure), not GPU
//! timestamps.

use crate::{render::GraphicsTuning, shared::AppState, tuning::Tuning};
use bevy::{
    prelude::*,
    render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems},
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, OnceLock},
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
        let submits = SubmitClock::default();
        app.init_resource::<LaunchTime>()
            .init_resource::<FrameLog>()
            .init_resource::<FrameStats>()
            .init_resource::<FrameClock>()
            .init_resource::<MainFrame>()
            .insert_resource(submits.clone())
            .add_systems(First, count_main_frame)
            .add_systems(FixedFirst, count_fixed_tick)
            .add_systems(Last, (record_frame, detect_controllable));
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .insert_resource(submits)
                .init_resource::<RenderedFrame>()
                .add_systems(ExtractSchedule, extract_main_frame)
                .add_systems(
                    Render,
                    record_submit
                        .after(RenderSystems::Render)
                        .before(RenderSystems::Cleanup),
                );
        }
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

// ---------------------------------------------------------------------------
// Render-submit clock (input-to-frame latency probe)
// ---------------------------------------------------------------------------

/// Main-world frame number, incremented at the very start of every frame
/// (`First`). A system that injects input in `PreUpdate` reads it to name the
/// frame that will show the input's effect.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MainFrame(pub u64);

/// The main-world frame the render world is drawing (pipelined rendering is
/// off, so it is always the frame that just finished its main schedule).
#[derive(Resource, Debug, Default, Clone, Copy)]
struct RenderedFrame(u64);

/// How many recent frames [`SubmitClock`] remembers.
pub const SUBMIT_HISTORY: usize = 4096;

/// When each recent frame's rendering was submitted: recorded in the render
/// sub-app's `Render` schedule right after [`RenderSystems::Render`], whose
/// `render_system` runs the render graph, submits the command buffers and calls
/// `present` on the window's swapchain texture. Shared by the main and render
/// worlds.
#[derive(Resource, Debug, Default, Clone)]
pub struct SubmitClock(Arc<Mutex<VecDeque<(u64, Instant)>>>);

impl SubmitClock {
    /// When frame `frame`'s rendering was submitted, if it is still remembered.
    pub fn submitted(&self, frame: u64) -> Option<Instant> {
        let log = self.0.lock().ok()?;
        log.iter().rev().find(|(f, _)| *f == frame).map(|(_, t)| *t)
    }

    pub fn record(&self, frame: u64, at: Instant) {
        if let Ok(mut log) = self.0.lock() {
            if log.len() >= SUBMIT_HISTORY {
                log.pop_front();
            }
            log.push_back((frame, at));
        }
    }
}

fn count_main_frame(mut frame: ResMut<MainFrame>) {
    frame.0 += 1;
}

fn extract_main_frame(mut rendered: ResMut<RenderedFrame>, main: Extract<Res<MainFrame>>) {
    rendered.0 = main.0;
}

fn record_submit(rendered: Res<RenderedFrame>, clock: Res<SubmitClock>) {
    clock.record(rendered.0, Instant::now());
}

// ---------------------------------------------------------------------------
// Latency statistics
// ---------------------------------------------------------------------------

/// Distribution of latency samples (milliseconds).
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct LatencyStats {
    pub samples: usize,
    pub min_ms: f64,
    /// Middle value; the mean of the two middle values for an even count.
    pub median_ms: f64,
    pub mean_ms: f64,
    /// Nearest-rank 95th percentile.
    pub p95_ms: f64,
    pub max_ms: f64,
}

pub fn latency_stats(samples_ms: &[f64]) -> LatencyStats {
    if samples_ms.is_empty() {
        return LatencyStats::default();
    }
    let mut sorted = samples_ms.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let n = sorted.len();
    let median = if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    };
    LatencyStats {
        samples: n,
        min_ms: sorted[0],
        median_ms: median,
        mean_ms: sorted.iter().sum::<f64>() / n as f64,
        p95_ms: percentile(&sorted, 95.0),
        max_ms: sorted[n - 1],
    }
}

// ---------------------------------------------------------------------------
// Gate thresholds (docs/GOAL.md)
// ---------------------------------------------------------------------------

/// One measured condition of a gate.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GateCheck {
    pub name: String,
    pub value: serde_json::Value,
    /// The condition, stated the way the goal brief states it.
    pub threshold: String,
    pub pass: bool,
}

/// A gate verdict from one run's measurements.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GateResult {
    pub gate: String,
    pub pass: bool,
    pub checks: Vec<GateCheck>,
}

impl GateResult {
    fn new(gate: &str, checks: Vec<GateCheck>) -> Self {
        Self {
            gate: gate.into(),
            pass: !checks.is_empty() && checks.iter().all(|c| c.pass),
            checks,
        }
    }
}

fn check(name: &str, value: impl Serialize, threshold: String, pass: bool) -> GateCheck {
    GateCheck {
        name: name.into(),
        value: serde_json::to_value(value).unwrap_or(serde_json::Value::Null),
        threshold,
        pass,
    }
}

/// G2 frame pacing: mean interval 16.4–17.0 ms, no frame over 25 ms, at least
/// 99% of frames under 18 ms (after the warm-up).
pub mod g2 {
    pub const MEAN_MIN_MS: f64 = 16.4;
    pub const MEAN_MAX_MS: f64 = 17.0;
    pub const HITCH_MS: f64 = 25.0;
    pub const MAX_HITCHES: usize = 0;
    pub const FAST_MS: f64 = 18.0;
    pub const MIN_PCT_FAST: f64 = 99.0;
}

pub fn evaluate_g2(frames: &FrameSummary) -> GateResult {
    use g2::*;
    GateResult::new(
        "G2",
        vec![
            check(
                "frames_measured",
                frames.frames,
                "> 0".into(),
                frames.frames > 0,
            ),
            check(
                "mean_interval_ms",
                frames.mean_ms,
                format!("{MEAN_MIN_MS} ≤ mean ≤ {MEAN_MAX_MS}"),
                (MEAN_MIN_MS..=MEAN_MAX_MS).contains(&frames.mean_ms),
            ),
            check(
                "frames_over_25_ms",
                frames.over_25_ms,
                format!("= {MAX_HITCHES} frames > {HITCH_MS} ms"),
                frames.over_25_ms == MAX_HITCHES,
            ),
            check(
                "pct_frames_under_18_ms",
                frames.pct_under_18_ms,
                format!("≥ {MIN_PCT_FAST}% of frames < {FAST_MS} ms"),
                frames.pct_under_18_ms >= MIN_PCT_FAST,
            ),
        ],
    )
}

/// G3 latency: median input-to-frame latency at most 33 ms (two 60 Hz frames),
/// display scanout excluded.
pub mod g3 {
    pub const MEDIAN_MAX_MS: f64 = 33.0;
    pub const MIN_SAMPLES: usize = 100;
}

pub fn evaluate_g3(basis: &str, stats: &LatencyStats) -> GateResult {
    use g3::*;
    GateResult::new(
        "G3",
        vec![
            check(
                "samples",
                stats.samples,
                format!("≥ {MIN_SAMPLES}"),
                stats.samples >= MIN_SAMPLES,
            ),
            check(
                &format!("{basis}_median_ms"),
                stats.median_ms,
                format!("median ≤ {MEDIAN_MAX_MS} ms"),
                stats.samples > 0 && stats.median_ms <= MEDIAN_MAX_MS,
            ),
        ],
    )
}

/// G6 time to kill: rifle 1.0–2.0 s on a standing dummy at 15 m (every kill,
/// at least five), one close pump body shot never kills from full, and a wall
/// soaks at least 1.0 s of continuous rifle fire.
pub mod g6 {
    pub const RIFLE_TTK_MIN_S: f64 = 1.0;
    pub const RIFLE_TTK_MAX_S: f64 = 2.0;
    pub const MIN_RIFLE_KILLS: usize = 5;
    pub const MIN_PUMP_SHOTS: usize = 1;
    pub const WALL_SOAK_MIN_S: f64 = 1.0;
}

/// `pump_kills` holds, per point-blank pump shot on a full-health dummy, whether
/// it killed.
pub fn evaluate_g6(
    rifle_ttks_s: &[f64],
    pump_kills: &[bool],
    wall_soak_s: Option<f64>,
) -> GateResult {
    use g6::*;
    let in_range = |t: &f64| (RIFLE_TTK_MIN_S..=RIFLE_TTK_MAX_S).contains(t);
    GateResult::new(
        "G6",
        vec![
            check(
                "rifle_kills",
                rifle_ttks_s.len(),
                format!("≥ {MIN_RIFLE_KILLS}"),
                rifle_ttks_s.len() >= MIN_RIFLE_KILLS,
            ),
            check(
                "rifle_ttk_s",
                rifle_ttks_s,
                format!("every kill {RIFLE_TTK_MIN_S}–{RIFLE_TTK_MAX_S} s"),
                !rifle_ttks_s.is_empty() && rifle_ttks_s.iter().all(in_range),
            ),
            check(
                "pump_shots_that_killed_from_full",
                pump_kills.iter().filter(|k| **k).count(),
                format!("0 of ≥ {MIN_PUMP_SHOTS} shots"),
                pump_kills.len() >= MIN_PUMP_SHOTS && !pump_kills.iter().any(|k| *k),
            ),
            check(
                "wall_soak_s",
                wall_soak_s,
                format!("≥ {WALL_SOAK_MIN_S} s"),
                wall_soak_s.is_some_and(|s| s >= WALL_SOAK_MIN_S),
            ),
        ],
    )
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

/// Frame intervals after `warmup_ms` since the first playing frame, up to and
/// including frames that ended at `end_ms` (process-start milliseconds, like
/// [`FrameRow::t_ms`]). Scenarios use it to measure exactly their timed window.
pub fn intervals_in_window(rows: &[FrameRow], warmup_ms: f64, end_ms: Option<f64>) -> Vec<f64> {
    let Some(start) = rows.iter().find(|r| r.playing).map(|r| r.t_ms) else {
        return Vec::new();
    };
    rows.iter()
        .filter(|r| {
            r.playing
                && r.t_ms - start >= warmup_ms
                && r.dt_ms > 0.0
                && end_ms.is_none_or(|end| r.t_ms <= end)
        })
        .map(|r| r.dt_ms)
        .collect()
}

/// Milliseconds since process start (the clock [`FrameRow::t_ms`] uses).
pub fn now_ms() -> f64 {
    process_start().elapsed().as_secs_f64() * 1000.0
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
