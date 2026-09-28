//! The always-on session log (M3 chunk 0, D86/D87).
//!
//! Every native play session (not scenarios, which keep their own evidence
//! folders, and never headless tests) writes
//! `userdata/sessions/<YYYYMMDD-HHMMSS>/` (UTC stamp):
//!
//! - `frames.csv`: `frame,t_ms,dt_ms,state,occluded`, one row per frame;
//! - `session.json`: commit, build id, preset, launch time (cold or warm) and
//!   its boot phases, power and Low Power Mode samples, occluded and play
//!   time, and the S2 verdict over the counted frames.
//!
//! The main thread only pushes a small `Copy` row into a bounded channel each
//! frame. A background writer thread owns the files: it buffers the CSV and
//! flushes it every [`FLUSH_EVERY`], samples power (`pmset`) and rewrites
//! `session.json` every [`SAMPLE_EVERY_MS`] of session time, and writes the
//! final summary at quit, so a crash still leaves the data and a recent
//! summary. At quit the game prints one line (see [`quit_line`]):
//!
//! `PIECED_S2 PASS|FAIL|N/A <mean> <p99> <n>>25ms <pct>%<18ms <why>`
//!
//! Every native launch (scenarios included, since they warm the same shader
//! caches) is **cold** when the binary's build id differs from the previous
//! launch's (`userdata/sessions/last_build`), and **warm** otherwise.

use crate::{
    render::QualityPreset,
    shared::AppState,
    telemetry::{
        self, BootPhases, FrameSummary, GateResult, LastFrame, LaunchTime, PowerState,
        RunConditions, TelemetrySystems, evaluate_g2, g2, summarize,
    },
    tuning::Tuning,
};
use bevy::prelude::*;
use serde::Serialize;
use serde_json::json;
use std::{
    fs::File,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel},
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

// ---------------------------------------------------------------------------
// Numbers
// ---------------------------------------------------------------------------

/// Frames in the first 10 s after launch never count for S2.
pub const LAUNCH_EXCLUDE_MS: f64 = 10_000.0;
/// Nor the first second after each return to `Playing`.
pub const REENTRY_EXCLUDE_MS: f64 = 1_000.0;
/// A session needs 5 minutes of counted play to qualify.
pub const MIN_COUNTED_PLAY_S: f64 = 300.0;
/// Session folders kept (the newest, by name).
pub const KEEP_SESSIONS: usize = 30;
/// How often the writer flushes `frames.csv`.
pub const FLUSH_EVERY: Duration = Duration::from_secs(5);
/// How often (session time) power is sampled and `session.json` rewritten.
pub const SAMPLE_EVERY_MS: f64 = 30_000.0;
/// Rows the channel holds before the main thread drops (and counts) rows:
/// over two minutes at 60 fps.
pub const CHANNEL_ROWS: usize = 8192;
/// `pmset -g batt`'s source name on battery.
pub const BATTERY_SOURCE: &str = "Battery Power";
/// How long quit waits for the writer's final summary.
pub const FINISH_TIMEOUT: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// Frames and the S2 filter
// ---------------------------------------------------------------------------

/// The game state a frame ended in, as the log records it. Any state that is
/// not `Playing` (the pause menu today; a main menu or results screen later)
/// never counts for S2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FrameState {
    Boot,
    Playing,
    Paused,
    /// Any other state (for example a future menu or results screen).
    Other,
}

impl FrameState {
    pub fn of(state: &AppState) -> Self {
        // An if-chain rather than a match, so a new `AppState` variant maps
        // to `Other` without touching this.
        if *state == AppState::Playing {
            Self::Playing
        } else if *state == AppState::Paused {
            Self::Paused
        } else if *state == AppState::Boot {
            Self::Boot
        } else {
            Self::Other
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::Playing => "playing",
            Self::Paused => "paused",
            Self::Other => "other",
        }
    }

    pub fn is_playing(self) -> bool {
        self == Self::Playing
    }
}

/// One row of the session log. Small and `Copy`: this is all the main thread
/// sends per frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SessionFrame {
    pub frame: u64,
    /// Milliseconds since process start at the end of the frame.
    pub t_ms: f64,
    /// Wall time since the previous frame ended (0 on the first frame).
    pub dt_ms: f64,
    pub state: FrameState,
    /// The window was occluded (covered, not composited) at the frame's end.
    pub occluded: bool,
}

/// Whether a frame counts for S2: it is a `Playing` frame with a measured
/// interval, at least [`LAUNCH_EXCLUDE_MS`] after launch and at least
/// [`REENTRY_EXCLUDE_MS`] after the game (re)entered `Playing` at
/// `playing_since_ms` (the first frame of the current `Playing` stretch).
pub fn counts_for_s2(t_ms: f64, dt_ms: f64, playing: bool, playing_since_ms: f64) -> bool {
    playing
        && dt_ms > 0.0
        && t_ms >= LAUNCH_EXCLUDE_MS
        && t_ms - playing_since_ms >= REENTRY_EXCLUDE_MS
}

/// Streams frames through [`counts_for_s2`], tracking when each `Playing`
/// stretch began.
#[derive(Debug, Clone, Default)]
pub struct S2Filter {
    playing_since: Option<f64>,
}

impl S2Filter {
    pub fn admit(&mut self, row: &SessionFrame) -> bool {
        if !row.state.is_playing() {
            self.playing_since = None;
            return false;
        }
        let since = *self.playing_since.get_or_insert(row.t_ms);
        counts_for_s2(row.t_ms, row.dt_ms, true, since)
    }
}

/// The intervals of the frames that count for S2, in order.
pub fn s2_intervals(rows: &[SessionFrame]) -> Vec<f64> {
    let mut filter = S2Filter::default();
    rows.iter()
        .filter(|row| filter.admit(row))
        .map(|row| row.dt_ms)
        .collect()
}

// ---------------------------------------------------------------------------
// Qualification and the verdict
// ---------------------------------------------------------------------------

/// One power reading, taken off the main thread.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PowerSample {
    /// Session time of the sample (milliseconds since process start).
    pub t_ms: f64,
    pub source: String,
    pub battery: String,
    pub low_power_mode: Option<bool>,
    /// System load averages (1, 5, 15 min) at the sample: background work
    /// (builds, tests, Blender) shows up here and explains slow sessions.
    pub load: Option<[f64; 3]>,
}

impl PowerSample {
    pub fn new(t_ms: f64, state: PowerState) -> Self {
        Self {
            t_ms,
            source: state.source,
            battery: state.battery,
            low_power_mode: state.low_power_mode,
            load: crate::telemetry::load_average(),
        }
    }

    pub fn on_battery(&self) -> bool {
        self.source == BATTERY_SOURCE
    }
}

/// Everything besides the frame times that decides whether a session can be
/// judged against the S2 bar.
#[derive(Debug, Clone, Default, Serialize)]
pub struct S2Conditions {
    pub counted_play_s: f64,
    pub power_samples: Vec<PowerSample>,
    /// Occluded time within counted play.
    pub occluded_counted_ms: f64,
    pub release: bool,
    /// The preset label (e.g. `Battery`).
    pub preset: String,
    /// The Battery preset was on for the whole session.
    pub battery_preset: bool,
    /// Rows the main thread dropped because the channel was full.
    pub rows_dropped: u64,
}

/// Why a session does not qualify for S2 (empty when it does). Each reason
/// is one short token without spaces, for the quit line.
pub fn qualification_problems(c: &S2Conditions) -> Vec<String> {
    let mut out = Vec::new();
    if c.counted_play_s < MIN_COUNTED_PLAY_S {
        out.push(format!(
            "play={:.0}s<{:.0}s",
            c.counted_play_s.floor(),
            MIN_COUNTED_PLAY_S
        ));
    }
    let n = c.power_samples.len();
    if n == 0 {
        out.push("power=unsampled".into());
    } else {
        let on_ac = c.power_samples.iter().filter(|s| !s.on_battery()).count();
        if on_ac > 0 {
            out.push(format!("not-on-battery={on_ac}/{n}"));
        }
        let lpm_off = c
            .power_samples
            .iter()
            .filter(|s| s.low_power_mode != Some(true))
            .count();
        if lpm_off > 0 {
            out.push(format!("low-power-off={lpm_off}/{n}"));
        }
    }
    if c.occluded_counted_ms > 0.0 {
        out.push(format!("occluded={:.0}ms", c.occluded_counted_ms.ceil()));
    }
    if !c.release {
        out.push("build=debug".into());
    }
    if !c.battery_preset {
        out.push(format!("preset={}", c.preset));
    }
    if c.rows_dropped > 0 {
        out.push(format!("rows-dropped={}", c.rows_dropped));
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum S2Verdict {
    /// Qualified and met the bar.
    #[serde(rename = "PASS")]
    Pass,
    /// Qualified but missed the bar.
    #[serde(rename = "FAIL")]
    Fail,
    /// Did not qualify.
    #[serde(rename = "N/A")]
    NotApplicable,
}

impl S2Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::NotApplicable => "N/A",
        }
    }
}

/// The S2 verdict for one session.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct S2Report {
    pub verdict: S2Verdict,
    /// N/A: why the session did not qualify. FAIL: which thresholds missed.
    pub reasons: Vec<String>,
    /// Summary of the counted frames.
    pub frames: FrameSummary,
    /// M2's G2 thresholds applied to the counted frames.
    pub gate: GateResult,
}

/// Judges counted frames against M2's thresholds, unless the session did
/// not qualify (`problems` non-empty), which makes it N/A.
pub fn s2_report(frames: FrameSummary, problems: Vec<String>) -> S2Report {
    let gate = evaluate_g2(&frames);
    let (verdict, reasons) = if !problems.is_empty() {
        (S2Verdict::NotApplicable, problems)
    } else if gate.pass {
        (S2Verdict::Pass, Vec::new())
    } else {
        (S2Verdict::Fail, missed_thresholds(&frames))
    };
    S2Report {
        verdict,
        reasons,
        frames,
        gate,
    }
}

fn missed_thresholds(f: &FrameSummary) -> Vec<String> {
    use g2::*;
    let mut out = Vec::new();
    if f.frames == 0 {
        out.push("frames=0".into());
        return out;
    }
    if f.mean_ms > MEAN_MAX_MS {
        out.push(format!("mean={:.2}>{MEAN_MAX_MS}", f.mean_ms));
    } else if f.mean_ms < MEAN_MIN_MS {
        out.push(format!("mean={:.2}<{MEAN_MIN_MS}", f.mean_ms));
    }
    if f.over_25_ms > MAX_HITCHES {
        out.push(format!("over25={}>{MAX_HITCHES}", f.over_25_ms));
    }
    if f.pct_under_18_ms < MIN_PCT_FAST {
        out.push(format!(
            "under18={:.2}%<{MIN_PCT_FAST}%",
            floor2(f.pct_under_18_ms)
        ));
    }
    out
}

/// Rounds down to two decimals, so a failing 98.996% never prints as 99.00%.
fn floor2(v: f64) -> f64 {
    (v * 100.0).floor() / 100.0
}

/// The one line printed at quit:
/// `PIECED_S2 <verdict> <mean> <p99> <n>>25ms <pct>%<18ms <why>`, where
/// `<why>` is the comma-separated reasons, or `ok` for a PASS.
pub fn quit_line(report: &S2Report) -> String {
    let f = &report.frames;
    let why = if report.reasons.is_empty() {
        "ok".to_string()
    } else {
        report.reasons.join(",")
    };
    format!(
        "PIECED_S2 {} {:.2} {:.2} {}>25ms {:.2}%<18ms {why}",
        report.verdict.label(),
        f.mean_ms,
        f.p99_ms,
        f.over_25_ms,
        floor2(f.pct_under_18_ms),
    )
}

// ---------------------------------------------------------------------------
// Build id and launch kind
// ---------------------------------------------------------------------------

/// The commit this binary was built from (embedded by `build.rs`), with a
/// `-dirty` suffix when tracked files had uncommitted changes.
pub fn build_commit() -> String {
    let commit = env!("PIECED_GIT_COMMIT");
    if env!("PIECED_GIT_DIRTY") == "1" {
        format!("{commit}-dirty")
    } else {
        commit.to_string()
    }
}

/// A build id that changes whenever the binary does: the commit plus a hash
/// of the executable's path, size and modification time (every rebuild or
/// copy of the binary gets a new mtime).
pub fn build_id_for(commit: &str, exe: &Path, len: u64, mtime_ns: u128) -> String {
    // FNV-1a, 64-bit: stable across runs and Rust versions (unlike `DefaultHasher`).
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes.iter().chain(std::iter::once(&0xff)) {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    feed(commit.as_bytes());
    feed(exe.to_string_lossy().as_bytes());
    feed(&len.to_le_bytes());
    feed(&mtime_ns.to_le_bytes());
    format!("{commit}:{hash:016x}")
}

/// This process's build id (see [`build_id_for`]).
pub fn current_build_id() -> String {
    let exe = std::env::current_exe().unwrap_or_default();
    let meta = std::fs::metadata(&exe).ok();
    let len = meta.as_ref().map_or(0, |m| m.len());
    let mtime_ns = meta
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    build_id_for(&build_commit(), &exe, len, mtime_ns)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LaunchKind {
    /// First launch of this binary (a rebuild or a new copy): the shader
    /// caches are likely cold.
    Cold,
    Warm,
}

impl LaunchKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Warm => "warm",
        }
    }
}

/// Cold unless the previous launch ran the same build.
pub fn launch_kind(previous_build_id: Option<&str>, build_id: &str) -> LaunchKind {
    if previous_build_id == Some(build_id) {
        LaunchKind::Warm
    } else {
        LaunchKind::Cold
    }
}

/// This launch's build and whether it is cold or warm.
#[derive(Resource, Debug, Clone, PartialEq, Serialize)]
pub struct LaunchInfo {
    pub kind: LaunchKind,
    pub build_id: String,
    pub previous_build_id: Option<String>,
}

/// The file holding the previous launch's build id.
pub fn last_build_path(sessions_dir: &Path) -> PathBuf {
    sessions_dir.join("last_build")
}

/// Compares `build_id` with the previous launch's and records it for the
/// next launch.
pub fn resolve_launch(sessions_dir: &Path, build_id: &str) -> LaunchInfo {
    let path = last_build_path(sessions_dir);
    let previous = std::fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let kind = launch_kind(previous.as_deref(), build_id);
    if previous.as_deref() != Some(build_id) {
        let written =
            std::fs::create_dir_all(sessions_dir).and_then(|()| std::fs::write(&path, build_id));
        if let Err(e) = written {
            eprintln!("last build id not saved to {}: {e}", path.display());
        }
    }
    LaunchInfo {
        kind,
        build_id: build_id.to_string(),
        previous_build_id: previous,
    }
}

// ---------------------------------------------------------------------------
// Session folders
// ---------------------------------------------------------------------------

/// `<PIECED_ROOT or project>/userdata/sessions`.
pub fn default_sessions_dir() -> PathBuf {
    Tuning::settings_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
        .join("sessions")
}

/// `YYYYMMDD-HHMMSS` in UTC.
pub fn stamp_utc(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// Whether a folder name is a session stamp (`YYYYMMDD-HHMMSS`, optionally
/// followed by `-N` when two sessions start in the same second). Retention
/// only ever deletes such folders.
pub fn is_session_name(name: &str) -> bool {
    let b = name.as_bytes();
    let stamp = b.len() >= 15
        && b[..8].iter().all(u8::is_ascii_digit)
        && b[8] == b'-'
        && b[9..15].iter().all(u8::is_ascii_digit);
    stamp
        && (b.len() == 15
            || (b.len() > 16 && b[15] == b'-' && b[16..].iter().all(u8::is_ascii_digit)))
}

/// Creates `<sessions_dir>/<stamp>` (or `<stamp>-2`, `-3`... if taken).
pub fn create_session_dir(sessions_dir: &Path, stamp: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(sessions_dir)?;
    for n in 1..1000 {
        let name = if n == 1 {
            stamp.to_string()
        } else {
            format!("{stamp}-{n}")
        };
        let dir = sessions_dir.join(name);
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("no free session folder name"))
}

/// Keeps the `keep` newest session folders in `sessions_dir` (by name, which
/// sorts by time) and deletes the rest, never `protect` (the current
/// session). Only folders named like a session are touched. Returns the
/// deleted folders.
pub fn prune_sessions(
    sessions_dir: &Path,
    keep: usize,
    protect: &Path,
) -> std::io::Result<Vec<PathBuf>> {
    let mut names: Vec<String> = std::fs::read_dir(sessions_dir)?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| is_session_name(name))
        .collect();
    // Newest first. `-N` suffixes sort after the plain stamp, as they should.
    names.sort_unstable_by(|a, b| b.cmp(a));
    let mut deleted = Vec::new();
    for name in names.into_iter().skip(keep) {
        let dir = sessions_dir.join(&name);
        if dir == protect {
            continue;
        }
        std::fs::remove_dir_all(&dir)?;
        deleted.push(dir);
    }
    Ok(deleted)
}

// ---------------------------------------------------------------------------
// The writer thread
// ---------------------------------------------------------------------------

/// Reads the power state (`pmset` in the game; scripted in tests). Called
/// only on the writer thread.
pub type PowerSampler = Arc<dyn Fn() -> PowerState + Send + Sync>;

/// What a session knows at its start.
#[derive(Debug, Clone, Serialize)]
pub struct SessionMeta {
    pub commit: String,
    pub build_id: String,
    pub release: bool,
    pub graphics_preset: String,
    pub graphics: String,
    pub battery_preset: bool,
    pub launch: LaunchInfo,
    pub started_unix_s: f64,
}

pub struct WriterConfig {
    pub sessions_dir: PathBuf,
    /// The folder name (normally [`stamp_utc`] of now).
    pub stamp: String,
    pub keep: usize,
    pub power: PowerSampler,
    pub meta: SessionMeta,
}

/// The launch time and its boot phases, once the player can act.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LaunchRecord {
    pub ms: f64,
    pub phases: Vec<(String, f64)>,
}

/// What the writer hands back at the end of a session.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionOutcome {
    /// The session folder, if it could be created.
    pub dir: Option<PathBuf>,
    pub report: S2Report,
    /// The `PIECED_S2 ...` line.
    pub line: String,
}

enum Msg {
    Frame(SessionFrame),
    Launch(LaunchRecord),
    Preset {
        preset: String,
        graphics: String,
        battery: bool,
    },
    Finish(SyncSender<SessionOutcome>),
    /// A mode started from the main menu: Play → controllable (chunk 5).
    Play(PlayRecord),
}

/// One `PIECED_PLAY_MS`: the main menu's Play click to the first frame the
/// player can act, and the mode started.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlayRecord {
    pub ms: f64,
    pub mode: &'static str,
}

/// The main thread's handle on the writer thread.
pub struct SessionWriter {
    tx: SyncSender<Msg>,
    dropped: Arc<AtomicU64>,
    handle: Option<JoinHandle<()>>,
    finished: bool,
    outcome: Option<SessionOutcome>,
}

impl SessionWriter {
    pub fn spawn(config: WriterConfig) -> std::io::Result<Self> {
        let (tx, rx) = sync_channel(CHANNEL_ROWS);
        let dropped = Arc::new(AtomicU64::new(0));
        let thread_dropped = dropped.clone();
        let handle = std::thread::Builder::new()
            .name("session-log".into())
            .spawn(move || run_writer(config, rx, thread_dropped))?;
        Ok(Self {
            tx,
            dropped,
            handle: Some(handle),
            finished: false,
            outcome: None,
        })
    }

    /// Queues one frame. Never blocks and never allocates (the channel is
    /// bounded and preallocated); a full channel drops the row and counts it.
    pub fn push(&self, row: SessionFrame) {
        if let Err(TrySendError::Full(_)) = self.tx.try_send(Msg::Frame(row)) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Queues one frame, waiting for room instead of dropping it. For tools
    /// and tests that replay a whole session at once; the game uses [`push`].
    ///
    /// [`push`]: SessionWriter::push
    pub fn push_wait(&self, row: SessionFrame) {
        let _ = self.tx.send(Msg::Frame(row));
    }

    pub fn record_launch(&self, record: LaunchRecord) {
        let _ = self.tx.send(Msg::Launch(record));
    }

    pub fn record_play(&self, record: PlayRecord) {
        let _ = self.tx.send(Msg::Play(record));
    }

    pub fn set_preset(&self, preset: String, graphics: String, battery: bool) {
        let _ = self.tx.send(Msg::Preset {
            preset,
            graphics,
            battery,
        });
    }

    /// Rows dropped so far because the channel was full.
    pub fn rows_dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Ends the session: the writer drains every queued row, flushes, writes
    /// the final `session.json` and returns the verdict. Idempotent; waits at
    /// most `timeout` for the writer.
    pub fn finish(&mut self, timeout: Duration) -> Option<&SessionOutcome> {
        if !self.finished {
            self.finished = true;
            let (reply_tx, reply_rx) = sync_channel(1);
            if self.tx.send(Msg::Finish(reply_tx)).is_ok() {
                self.outcome = reply_rx.recv_timeout(timeout).ok();
            }
            if self.outcome.is_some()
                && let Some(handle) = self.handle.take()
            {
                let _ = handle.join();
            }
        }
        self.outcome.as_ref()
    }
}

impl Drop for SessionWriter {
    fn drop(&mut self) {
        self.finish(FINISH_TIMEOUT);
    }
}

fn run_writer(config: WriterConfig, rx: Receiver<Msg>, dropped: Arc<AtomicU64>) {
    let mut state = WriterState::open(config, dropped);
    let mut last_flush = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Msg::Frame(row)) => state.frame(row),
            Ok(Msg::Launch(record)) => {
                state.launch = Some(record);
                state.write_json(false);
            }
            Ok(Msg::Play(record)) => {
                state.plays.push(record);
                state.write_json(false);
            }
            Ok(Msg::Preset {
                preset,
                graphics,
                battery,
            }) => {
                state.meta.graphics_preset = preset;
                state.meta.graphics = graphics;
                state.meta.battery_preset &= battery;
            }
            Ok(Msg::Finish(reply)) => {
                let outcome = state.close();
                let _ = reply.send(outcome);
                return;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                state.close();
                return;
            }
        }
        if last_flush.elapsed() >= FLUSH_EVERY {
            state.flush();
            last_flush = Instant::now();
        }
    }
}

struct WriterState {
    meta: SessionMeta,
    power_fn: PowerSampler,
    dir: Option<PathBuf>,
    csv: Option<BufWriter<File>>,
    errors: Vec<String>,
    pruned: usize,
    filter: S2Filter,
    counted: Vec<f64>,
    counted_ms: f64,
    play_ms: f64,
    occluded_play_ms: f64,
    occluded_counted_ms: f64,
    frames: u64,
    last_t_ms: f64,
    power: Vec<PowerSample>,
    next_sample_ms: f64,
    launch: Option<LaunchRecord>,
    plays: Vec<PlayRecord>,
    dropped: Arc<AtomicU64>,
}

impl WriterState {
    fn open(config: WriterConfig, dropped: Arc<AtomicU64>) -> Self {
        let mut state = Self {
            meta: config.meta,
            power_fn: config.power,
            dir: None,
            csv: None,
            errors: Vec::new(),
            pruned: 0,
            filter: S2Filter::default(),
            // About 20 minutes of counted frames before the first regrowth.
            counted: Vec::with_capacity(72_000),
            counted_ms: 0.0,
            play_ms: 0.0,
            occluded_play_ms: 0.0,
            occluded_counted_ms: 0.0,
            frames: 0,
            last_t_ms: 0.0,
            power: Vec::new(),
            next_sample_ms: 0.0,
            launch: None,
            plays: Vec::new(),
            dropped,
        };
        match create_session_dir(&config.sessions_dir, &config.stamp) {
            Ok(dir) => {
                match prune_sessions(&config.sessions_dir, config.keep, &dir) {
                    Ok(deleted) => state.pruned = deleted.len(),
                    Err(e) => state.error(format!("pruning old sessions: {e}")),
                }
                let csv = File::create(dir.join("frames.csv")).and_then(|file| {
                    let mut out = BufWriter::with_capacity(1 << 16, file);
                    writeln!(out, "frame,t_ms,dt_ms,state,occluded")?;
                    out.flush()?;
                    Ok(out)
                });
                match csv {
                    Ok(out) => state.csv = Some(out),
                    Err(e) => state.error(format!("frames.csv: {e}")),
                }
                state.dir = Some(dir);
            }
            Err(e) => state.error(format!(
                "session folder in {}: {e}",
                config.sessions_dir.display()
            )),
        }
        state
    }

    fn error(&mut self, message: String) {
        eprintln!("session log: {message}");
        self.errors.push(message);
    }

    fn frame(&mut self, row: SessionFrame) {
        if let Some(csv) = self.csv.as_mut() {
            let written = writeln!(
                csv,
                "{},{:.3},{:.3},{},{}",
                row.frame,
                row.t_ms,
                row.dt_ms,
                row.state.label(),
                u8::from(row.occluded)
            );
            if let Err(e) = written {
                self.csv = None;
                self.error(format!("frames.csv write: {e}"));
            }
        }
        self.frames += 1;
        self.last_t_ms = row.t_ms;
        if row.state.is_playing() {
            self.play_ms += row.dt_ms;
            if row.occluded {
                self.occluded_play_ms += row.dt_ms;
            }
        }
        if self.filter.admit(&row) {
            self.counted.push(row.dt_ms);
            self.counted_ms += row.dt_ms;
            if row.occluded {
                self.occluded_counted_ms += row.dt_ms;
            }
        }
        if row.t_ms >= self.next_sample_ms {
            self.sample_power();
            self.next_sample_ms = row.t_ms + SAMPLE_EVERY_MS;
            self.write_json(false);
        }
    }

    fn sample_power(&mut self) {
        let state = (self.power_fn)();
        self.power.push(PowerSample::new(self.last_t_ms, state));
    }

    fn flush(&mut self) {
        if let Some(csv) = self.csv.as_mut()
            && let Err(e) = csv.flush()
        {
            self.csv = None;
            self.error(format!("frames.csv flush: {e}"));
        }
    }

    fn conditions(&self) -> S2Conditions {
        S2Conditions {
            counted_play_s: self.counted_ms / 1000.0,
            power_samples: self.power.clone(),
            occluded_counted_ms: self.occluded_counted_ms,
            release: self.meta.release,
            preset: self.meta.graphics_preset.clone(),
            battery_preset: self.meta.battery_preset,
            rows_dropped: self.dropped.load(Ordering::Relaxed),
        }
    }

    fn report(&self) -> S2Report {
        s2_report(
            summarize(&self.counted),
            qualification_problems(&self.conditions()),
        )
    }

    fn write_json(&mut self, final_write: bool) {
        let Some(dir) = self.dir.clone() else {
            return;
        };
        let report = self.report();
        let launch = self.launch.as_ref();
        let doc = json!({
            "schema": 1,
            "final": final_write,
            "folder": dir.file_name().map(|n| n.to_string_lossy().into_owned()),
            "started_unix_s": self.meta.started_unix_s,
            "updated_t_ms": self.last_t_ms,
            "commit": self.meta.commit,
            "build_id": self.meta.build_id,
            "build": if self.meta.release { "release" } else { "debug" },
            "graphics_preset": self.meta.graphics_preset,
            "graphics": self.meta.graphics,
            "battery_preset_throughout": self.meta.battery_preset,
            "launch": {
                "ms": launch.map(|l| l.ms),
                "kind": self.meta.launch.kind,
                "build_id": self.meta.launch.build_id,
                "previous_build_id": self.meta.launch.previous_build_id,
                "boot_phases": launch.map(|l| {
                    l.phases
                        .iter()
                        .map(|(phase, ms)| json!({ "phase": phase, "ms": ms }))
                        .collect::<Vec<_>>()
                }),
                "ready": "the main menu can be clicked (or, when play starts straight away, the player can act)",
            },
            "plays": self.plays,
            "power_samples": self.power,
            "frames_logged": self.frames,
            "rows_dropped": self.dropped.load(Ordering::Relaxed),
            "play_s": self.play_ms / 1000.0,
            "counted_play_s": self.counted_ms / 1000.0,
            "occluded_play_ms": self.occluded_play_ms,
            "occluded_counted_ms": self.occluded_counted_ms,
            "sessions_pruned": self.pruned,
            "errors": self.errors,
            "s2": report,
            "s2_line": quit_line(&report),
            "s2_rules": {
                "counted_frames": "Playing only; not the first 10 s after launch nor the first 1 s after each return to Playing",
                "qualifies": "≥ 300 s counted play, every power sample on battery with Low Power Mode on, never occluded during counted play, release build, Battery preset",
                "bar": "mean 16.4–17.0 ms, 0 frames > 25 ms, ≥ 99% < 18 ms",
            },
        });
        let path = dir.join("session.json");
        let tmp = dir.join("session.json.tmp");
        let written = serde_json::to_string_pretty(&doc)
            .map_err(std::io::Error::other)
            .and_then(|text| std::fs::write(&tmp, text))
            .and_then(|()| std::fs::rename(&tmp, &path));
        if let Err(e) = written {
            self.error(format!("session.json: {e}"));
        }
    }

    fn close(&mut self) -> SessionOutcome {
        self.flush();
        if self.frames > 0 {
            // A last reading, so unplugging near the end is not missed.
            self.sample_power();
        }
        self.write_json(true);
        self.flush();
        let report = self.report();
        SessionOutcome {
            dir: self.dir.clone(),
            line: quit_line(&report),
            report,
        }
    }
}

// ---------------------------------------------------------------------------
// The plugin
// ---------------------------------------------------------------------------

/// Resolves the launch kind on every native launch and, when `log_frames`
/// (native play, not scenarios), runs the session log. Needs
/// [`crate::telemetry::TelemetryPlugin`]. Only [`crate::app::game_app`] adds
/// it, so headless tests never write sessions unless they add it themselves.
#[derive(Clone)]
pub struct SessionPlugin {
    pub log_frames: bool,
    pub sessions_dir: PathBuf,
    pub power: PowerSampler,
    /// Folder name override (tests); defaults to [`stamp_utc`] of now.
    pub stamp: Option<String>,
}

impl SessionPlugin {
    /// The game's configuration: `userdata/sessions`, `pmset` power samples.
    pub fn native(log_frames: bool) -> Self {
        Self {
            log_frames,
            sessions_dir: default_sessions_dir(),
            power: Arc::new(telemetry::power_state),
            stamp: None,
        }
    }
}

#[derive(Resource, Clone)]
struct SessionSettings(SessionPlugin);

impl Plugin for SessionPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SessionSettings(self.clone()))
            .add_systems(Startup, start_session)
            .add_systems(
                Last,
                (push_frame, track_preset, forward_launch, finish_on_exit)
                    .chain()
                    .after(TelemetrySystems)
                    .after(bevy::window::ExitSystems)
                    .run_if(resource_exists::<SessionLog>),
            );
    }
}

/// The running session log (native play only).
#[derive(Resource)]
pub struct SessionLog {
    writer: SessionWriter,
    preset: Option<QualityPreset>,
    launch_sent: bool,
    quit_line: Option<String>,
    outcome: Option<SessionOutcome>,
}

impl SessionLog {
    pub fn new(writer: SessionWriter) -> Self {
        Self {
            writer,
            preset: None,
            launch_sent: false,
            quit_line: None,
            outcome: None,
        }
    }

    /// Ends the session and prints the `PIECED_S2` line (once).
    pub fn finish(&mut self) -> &str {
        if self.quit_line.is_none() {
            self.outcome = self.writer.finish(FINISH_TIMEOUT).cloned();
            let line = self.outcome.as_ref().map_or_else(
                || "PIECED_S2 N/A 0.00 0.00 0>25ms 0.00%<18ms writer-timeout".to_string(),
                |o| o.line.clone(),
            );
            println!("{line}");
            if let Some(dir) = self.outcome.as_ref().and_then(|o| o.dir.as_ref()) {
                println!("PIECED_SESSION {}", dir.display());
            }
            self.quit_line = Some(line);
        }
        self.quit_line.as_deref().unwrap_or_default()
    }

    /// Records a `PIECED_PLAY_MS` (a mode started from the main menu) in
    /// `session.json`.
    pub fn record_play(&self, record: PlayRecord) {
        self.writer.record_play(record);
    }

    /// The printed quit line, once the session has finished.
    pub fn quit_line(&self) -> Option<&str> {
        self.quit_line.as_deref()
    }

    /// The writer's final result, once the session has finished.
    pub fn outcome(&self) -> Option<&SessionOutcome> {
        self.outcome.as_ref()
    }
}

impl Drop for SessionLog {
    /// Quitting through a path that skips `AppExit` (the app's world being
    /// cleared on a macOS terminate) still finishes the session.
    fn drop(&mut self) {
        self.finish();
    }
}

fn start_session(
    mut commands: Commands,
    settings: Res<SessionSettings>,
    tuning: Option<Res<Tuning>>,
) {
    let settings = &settings.0;
    let build_id = current_build_id();
    let info = resolve_launch(&settings.sessions_dir, &build_id);
    commands.insert_resource(info.clone());
    if !settings.log_frames {
        return;
    }
    let graphics = tuning.map(|t| t.graphics.clone()).unwrap_or_default();
    let now = SystemTime::now();
    let meta = SessionMeta {
        commit: build_commit(),
        build_id,
        release: !cfg!(debug_assertions),
        graphics_preset: format!("{:?}", graphics.preset),
        graphics: telemetry::graphics_label(&graphics),
        battery_preset: graphics.preset == QualityPreset::Battery,
        launch: info,
        started_unix_s: now
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64()),
    };
    let config = WriterConfig {
        sessions_dir: settings.sessions_dir.clone(),
        stamp: settings.stamp.clone().unwrap_or_else(|| stamp_utc(now)),
        keep: KEEP_SESSIONS,
        power: settings.power.clone(),
        meta,
    };
    match SessionWriter::spawn(config) {
        Ok(writer) => commands.insert_resource(SessionLog::new(writer)),
        Err(e) => eprintln!("session log not started: {e}"),
    }
}

fn push_frame(
    log: Res<SessionLog>,
    last: Res<LastFrame>,
    state: Res<State<AppState>>,
    conditions: Res<RunConditions>,
) {
    log.writer.push(SessionFrame {
        frame: last.frame,
        t_ms: last.t_ms,
        dt_ms: last.dt_ms,
        state: FrameState::of(state.get()),
        occluded: conditions.occluded_now,
    });
}

/// Tells the writer when the quality preset changes (the settings menu can
/// switch it mid-session); the first frame records the starting preset.
fn track_preset(mut log: ResMut<SessionLog>, tuning: Option<Res<Tuning>>) {
    let Some(tuning) = tuning else { return };
    let g = &tuning.graphics;
    if log.preset != Some(g.preset) {
        let first = log.preset.is_none();
        log.preset = Some(g.preset);
        if first {
            return;
        }
        log.writer.set_preset(
            format!("{:?}", g.preset),
            telemetry::graphics_label(g),
            g.preset == QualityPreset::Battery,
        );
    }
}

fn forward_launch(mut log: ResMut<SessionLog>, launch: Res<LaunchTime>, phases: Res<BootPhases>) {
    if log.launch_sent {
        return;
    }
    if let Some(elapsed) = launch.0 {
        log.launch_sent = true;
        log.writer.record_launch(LaunchRecord {
            ms: elapsed.as_secs_f64() * 1000.0,
            phases: phases.phases(),
        });
    }
}

fn finish_on_exit(mut log: ResMut<SessionLog>, mut exits: MessageReader<AppExit>) {
    if exits.read().count() > 0 {
        log.finish();
    }
}
