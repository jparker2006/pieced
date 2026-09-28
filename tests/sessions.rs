//! The always-on session log (M3 chunk 0): the S2 frame filter, the
//! qualification rules, the verdict and quit line, cold versus warm launches,
//! retention, the background writer's CSV and JSON, and the plugin driving a
//! simulated session end to end. Everything runs in temp folders.

use bevy::{
    prelude::*,
    window::{WindowCreated, WindowOccluded},
};
use pieced::{
    render::QualityPreset,
    session::{
        FrameState, KEEP_SESSIONS, LaunchInfo, LaunchKind, LaunchRecord, PowerSample, PowerSampler,
        S2Conditions, S2Verdict, SessionFrame, SessionLog, SessionMeta, SessionPlugin,
        SessionWriter, WriterConfig, build_id_for, counts_for_s2, create_session_dir,
        is_session_name, last_build_path, launch_kind, prune_sessions, qualification_problems,
        quit_line, resolve_launch, s2_intervals, s2_report, stamp_utc,
    },
    shared::AppState,
    telemetry::{BootPhases, FrameSummary, LaunchTime, PowerState, TelemetryPlugin, summarize},
    tuning::Tuning,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// A fresh, empty temp folder unique to this test.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pieced-sessions-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn row(frame: u64, t_ms: f64, dt_ms: f64, state: FrameState) -> SessionFrame {
    SessionFrame::plain(frame, t_ms, dt_ms, state)
}

/// Frames every `dt` ms from `from` (exclusive) to `to` (inclusive) in `state`.
fn stretch(rows: &mut Vec<SessionFrame>, to_ms: f64, dt: f64, state: FrameState) {
    let mut t = rows.last().map_or(0.0, |r| r.t_ms);
    while t + dt <= to_ms + 1e-9 {
        t += dt;
        let n = rows.len() as u64 + 1;
        rows.push(row(n, t, dt, state));
    }
}

// ---------------------------------------------------------------------------
// The S2 frame filter
// ---------------------------------------------------------------------------

#[test]
fn s2_counts_only_playing_frames_after_the_launch_and_reentry_windows() {
    // The pure rule at its boundaries.
    assert!(!counts_for_s2(9_999.0, 16.7, true, 0.0), "first 10 s");
    assert!(counts_for_s2(10_000.0, 16.7, true, 0.0));
    assert!(
        !counts_for_s2(20_999.0, 16.7, true, 20_000.0),
        "first 1 s back"
    );
    assert!(counts_for_s2(21_000.0, 16.7, true, 20_000.0));
    assert!(!counts_for_s2(30_000.0, 16.7, false, 0.0), "not Playing");
    assert!(!counts_for_s2(30_000.0, 0.0, true, 0.0), "no interval");

    // Boot to 5 s, play to 20 s, pause to 25 s, play to 40 s, a future
    // menu-like state to 45 s, play to 50 s. 10 ms frames.
    let mut rows = Vec::new();
    stretch(&mut rows, 5_000.0, 10.0, FrameState::Boot);
    stretch(&mut rows, 20_000.0, 10.0, FrameState::Playing);
    stretch(&mut rows, 25_000.0, 10.0, FrameState::Paused);
    stretch(&mut rows, 40_000.0, 10.0, FrameState::Playing);
    stretch(&mut rows, 45_000.0, 10.0, FrameState::Other);
    stretch(&mut rows, 50_000.0, 10.0, FrameState::Playing);
    let counted = s2_intervals(&rows);
    // Stretch 1 starts at 5.01 s, so the 10 s launch rule binds: [10, 20].
    // Stretch 2 starts at 25.01 s: [26.01, 40]. Stretch 3 at 45.01: [46.01, 50].
    let expected = 1_001 + 1_400 + 400;
    assert_eq!(counted.len(), expected);
    assert!(counted.iter().all(|&dt| dt == 10.0));

    // A frame whose state is not Playing never counts, whatever its time.
    let late_pause = [row(1, 60_000.0, 16.7, FrameState::Paused)];
    assert!(s2_intervals(&late_pause).is_empty());
}

#[test]
fn frame_state_maps_every_non_playing_state_to_not_playing() {
    assert_eq!(FrameState::of(&AppState::Playing), FrameState::Playing);
    assert_eq!(FrameState::of(&AppState::Paused), FrameState::Paused);
    assert_eq!(FrameState::of(&AppState::Boot), FrameState::Boot);
    assert!(FrameState::Playing.is_playing());
    for s in [FrameState::Boot, FrameState::Paused, FrameState::Other] {
        assert!(!s.is_playing());
    }
    assert_eq!(FrameState::Other.label(), "other");
}

// ---------------------------------------------------------------------------
// Qualification
// ---------------------------------------------------------------------------

fn battery_lpm(t_ms: f64) -> PowerSample {
    PowerSample {
        t_ms,
        source: "Battery Power".into(),
        battery: "-InternalBattery-0 80%; discharging".into(),
        low_power_mode: Some(true),
        load: None,
    }
}

fn qualifying() -> S2Conditions {
    S2Conditions {
        counted_play_s: 320.0,
        power_samples: (0..12).map(|i| battery_lpm(i as f64 * 30_000.0)).collect(),
        occluded_counted_ms: 0.0,
        release: true,
        preset: "Battery".into(),
        battery_preset: true,
        rows_dropped: 0,
    }
}

#[test]
fn qualification_names_every_reason_a_session_does_not_count() {
    assert!(qualification_problems(&qualifying()).is_empty());

    let one = |f: &dyn Fn(&mut S2Conditions)| {
        let mut c = qualifying();
        f(&mut c);
        qualification_problems(&c)
    };
    assert_eq!(
        one(&|c| c.counted_play_s = 299.9),
        ["play=299s<300s".to_string()]
    );
    assert_eq!(
        one(&|c| c.power_samples.clear()),
        ["power=unsampled".to_string()]
    );
    assert_eq!(
        one(&|c| c.power_samples[3].source = "AC Power".into()),
        ["not-on-battery=1/12".to_string()]
    );
    assert_eq!(
        one(&|c| c.power_samples[0].low_power_mode = Some(false)),
        ["low-power-off=1/12".to_string()]
    );
    // An unreadable Low Power Mode is not "on".
    assert_eq!(
        one(&|c| c.power_samples[11].low_power_mode = None),
        ["low-power-off=1/12".to_string()]
    );
    assert_eq!(
        one(&|c| c.occluded_counted_ms = 16.7),
        ["occluded=17ms".to_string()]
    );
    assert_eq!(one(&|c| c.release = false), ["build=debug".to_string()]);
    assert_eq!(
        one(&|c| {
            c.battery_preset = false;
            c.preset = "PluggedIn".into();
        }),
        ["preset=PluggedIn".to_string()]
    );
    assert_eq!(one(&|c| c.rows_dropped = 4), ["rows-dropped=4".to_string()]);

    // Several at once, in a stable order.
    let mut c = qualifying();
    c.counted_play_s = 42.0;
    c.release = false;
    c.power_samples[0].source = "AC Power".into();
    assert_eq!(
        qualification_problems(&c),
        ["play=42s<300s", "not-on-battery=1/12", "build=debug"]
    );
}

// ---------------------------------------------------------------------------
// The verdict and the quit line
// ---------------------------------------------------------------------------

#[test]
fn verdict_on_synthetic_frame_series() {
    let steady = vec![16.667; 18_000];
    let pass = s2_report(summarize(&steady), Vec::new());
    assert_eq!(pass.verdict, S2Verdict::Pass);
    assert!(pass.reasons.is_empty());
    assert_eq!(
        quit_line(&pass),
        "PIECED_S2 PASS 16.67 16.67 0>25ms 100.00%<18ms ok"
    );

    // One spike over 25 ms fails, even with a good mean and 99%+ fast.
    let mut spike = steady.clone();
    spike[9_000] = 31.0;
    let fail = s2_report(summarize(&spike), Vec::new());
    assert_eq!(fail.verdict, S2Verdict::Fail);
    assert_eq!(fail.reasons, ["over25=1>0"]);
    assert_eq!(
        quit_line(&fail),
        "PIECED_S2 FAIL 16.67 16.67 1>25ms 99.99%<18ms over25=1>0"
    );

    // 2% of frames at 19 ms: no hitch, mean in range, but < 99% under 18 ms.
    let mut slow = vec![16.6; 9_800];
    slow.extend(vec![19.0; 200]);
    let fail = s2_report(summarize(&slow), Vec::new());
    assert_eq!(fail.verdict, S2Verdict::Fail);
    assert_eq!(fail.reasons, ["under18=98.00%<99%"]);
    assert!(quit_line(&fail).starts_with("PIECED_S2 FAIL 16.65 19.00 0>25ms 98.00%<18ms "));

    // A mean outside 16.4–17.0 (no vsync) fails by the mean.
    let fast = s2_report(summarize(&vec![8.3; 1_000]), Vec::new());
    assert_eq!(fast.reasons, ["mean=8.30<16.4"]);

    // A session that did not qualify is N/A with its reasons, even if its
    // frames would pass; the stats still print.
    let na = s2_report(summarize(&steady), vec!["play=120s<300s".into()]);
    assert_eq!(na.verdict, S2Verdict::NotApplicable);
    assert_eq!(
        quit_line(&na),
        "PIECED_S2 N/A 16.67 16.67 0>25ms 100.00%<18ms play=120s<300s"
    );
    let na = s2_report(
        FrameSummary::default(),
        vec!["play=0s<300s".into(), "build=debug".into()],
    );
    assert_eq!(
        quit_line(&na),
        "PIECED_S2 N/A 0.00 0.00 0>25ms 0.00%<18ms play=0s<300s,build=debug"
    );
}

#[test]
fn quit_line_never_rounds_a_failing_percentage_up_to_the_bar() {
    // 98.996% under 18 ms prints as 98.99%, not 99.00%.
    let mut v = vec![16.6; 98_996];
    v.extend(vec![18.5; 1_004]);
    let report = s2_report(summarize(&v), Vec::new());
    assert_eq!(report.verdict, S2Verdict::Fail);
    assert!(quit_line(&report).contains(" 98.99%<18ms "));
    // Exactly one line, six fields plus the reasons.
    let line = quit_line(&report);
    assert!(!line.contains('\n'));
    assert_eq!(line.split(' ').count(), 7);
}

// ---------------------------------------------------------------------------
// Launch kind and build id
// ---------------------------------------------------------------------------

#[test]
fn launch_is_cold_when_the_build_id_changes_and_warm_otherwise() {
    assert_eq!(launch_kind(None, "a"), LaunchKind::Cold);
    assert_eq!(launch_kind(Some("a"), "a"), LaunchKind::Warm);
    assert_eq!(launch_kind(Some("b"), "a"), LaunchKind::Cold);

    let dir = temp_dir("launch");
    let first = resolve_launch(&dir, "c1:0001");
    assert_eq!(first.kind, LaunchKind::Cold);
    assert_eq!(first.previous_build_id, None);
    let second = resolve_launch(&dir, "c1:0001");
    assert_eq!(second.kind, LaunchKind::Warm);
    assert_eq!(second.previous_build_id.as_deref(), Some("c1:0001"));
    let rebuilt = resolve_launch(&dir, "c1:0002");
    assert_eq!(rebuilt.kind, LaunchKind::Cold);
    assert_eq!(
        std::fs::read_to_string(last_build_path(&dir)).unwrap(),
        "c1:0002"
    );
    assert_eq!(resolve_launch(&dir, "c1:0002").kind, LaunchKind::Warm);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn build_id_changes_with_any_rebuild_or_copy_of_the_binary() {
    let exe = Path::new("/tmp/pieced-play");
    let id = build_id_for("abc123", exe, 1000, 5);
    assert_eq!(id, build_id_for("abc123", exe, 1000, 5), "stable");
    assert!(id.starts_with("abc123:"));
    assert_ne!(id, build_id_for("abc123", exe, 1000, 6), "new mtime");
    assert_ne!(id, build_id_for("abc123", exe, 1001, 5), "new size");
    assert_ne!(id, build_id_for("abc124", exe, 1000, 5), "new commit");
    assert_ne!(
        id,
        build_id_for("abc123", Path::new("/tmp/other"), 1000, 5),
        "new path"
    );
}

// ---------------------------------------------------------------------------
// Folders and retention
// ---------------------------------------------------------------------------

#[test]
fn stamps_are_utc_and_sortable() {
    let at = |secs: u64| stamp_utc(UNIX_EPOCH + Duration::from_secs(secs));
    assert_eq!(at(0), "19700101-000000");
    assert_eq!(at(1_790_523_012), "20260927-153012");
    assert_eq!(at(951_868_799), "20000229-235959");
    assert!(is_session_name("20260927-153012"));
    assert!(is_session_name("20260927-153012-2"));
    for bad in [
        "notes",
        "20260927",
        "20260927-15301x",
        "20260927-153012-",
        "last_build",
    ] {
        assert!(!is_session_name(bad), "{bad}");
    }
}

#[test]
fn retention_keeps_the_newest_30_sessions_and_nothing_else_is_touched() {
    let dir = temp_dir("retention");
    for i in 0..35 {
        std::fs::create_dir(dir.join(format!("20260901-1200{i:02}"))).unwrap();
    }
    std::fs::create_dir(dir.join("notes")).unwrap();
    std::fs::write(dir.join("last_build"), "x").unwrap();

    // Same-second collisions get a suffix.
    let current = create_session_dir(&dir, "20260927-090000").unwrap();
    let twin = create_session_dir(&dir, "20260927-090000").unwrap();
    assert_eq!(twin.file_name().unwrap(), "20260927-090000-2");

    let deleted = prune_sessions(&dir, KEEP_SESSIONS, &current).unwrap();
    assert_eq!(deleted.len(), 7);
    let mut left: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| is_session_name(n))
        .collect();
    left.sort();
    assert_eq!(left.len(), KEEP_SESSIONS);
    // The oldest seven went; the two new ones stayed.
    assert_eq!(left[0], "20260901-120007");
    assert!(left.contains(&"20260927-090000".to_string()));
    assert!(left.contains(&"20260927-090000-2".to_string()));
    assert!(dir.join("notes").is_dir());
    assert!(dir.join("last_build").is_file());

    // The protected (current) session survives even when it sorts oldest.
    let old = dir.join("19990101-000000");
    std::fs::create_dir(&old).unwrap();
    prune_sessions(&dir, 5, &old).unwrap();
    assert!(old.is_dir());
    let _ = std::fs::remove_dir_all(dir);
}

// ---------------------------------------------------------------------------
// The writer
// ---------------------------------------------------------------------------

fn sampler(source: &'static str, lpm: Option<bool>) -> PowerSampler {
    Arc::new(move || PowerState {
        source: source.into(),
        battery: "80%; discharging".into(),
        low_power_mode: lpm,
    })
}

fn meta(release: bool) -> SessionMeta {
    SessionMeta {
        commit: "abc123".into(),
        build_id: "abc123:00ff".into(),
        release,
        graphics_preset: "Battery".into(),
        graphics: "Battery preset".into(),
        battery_preset: true,
        launch: LaunchInfo {
            kind: LaunchKind::Warm,
            build_id: "abc123:00ff".into(),
            previous_build_id: Some("abc123:00ff".into()),
        },
        started_unix_s: 1.0,
    }
}

/// Boot for 3 s, then `play_s` seconds of steady 60 fps play with one pause.
fn simulated_rows(play_s: f64) -> Vec<SessionFrame> {
    let dt = 1000.0 / 60.0;
    let mut rows = Vec::new();
    stretch(&mut rows, 3_000.0, dt, FrameState::Boot);
    stretch(&mut rows, 3_000.0 + play_s * 500.0, dt, FrameState::Playing);
    let pause_end = rows.last().unwrap().t_ms + 4_000.0;
    stretch(&mut rows, pause_end, dt, FrameState::Paused);
    stretch(
        &mut rows,
        pause_end + play_s * 500.0,
        dt,
        FrameState::Playing,
    );
    rows
}

fn read_json(path: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn writer_streams_the_csv_and_writes_a_passing_session_json() {
    let root = temp_dir("writer-pass");
    for i in 0..31 {
        std::fs::create_dir(root.join(format!("20250101-0000{i:02}"))).unwrap();
    }
    let mut writer = SessionWriter::spawn(WriterConfig {
        sessions_dir: root.clone(),
        stamp: "20260927-100000".into(),
        keep: KEEP_SESSIONS,
        power: sampler("Battery Power", Some(true)),
        meta: meta(true),
    })
    .unwrap();
    let rows = simulated_rows(360.0);
    for r in &rows {
        writer.push_wait(*r);
    }
    writer.record_launch(LaunchRecord {
        ms: 2345.6,
        phases: vec![("window".into(), 300.0), ("models".into(), 1200.0)],
    });
    let outcome = writer.finish(Duration::from_secs(10)).unwrap().clone();
    assert_eq!(writer.rows_dropped(), 0);

    assert_eq!(outcome.report.verdict, S2Verdict::Pass, "{}", outcome.line);
    assert!(
        outcome
            .line
            .starts_with("PIECED_S2 PASS 16.67 16.67 0>25ms 100.00%<18ms ok")
    );
    let dir = outcome.dir.unwrap();
    assert_eq!(dir, root.join("20260927-100000"));

    // The CSV: a header and one line per frame, with the state label.
    let csv = std::fs::read_to_string(dir.join("frames.csv")).unwrap();
    let lines: Vec<&str> = csv.lines().collect();
    assert!(lines[0].starts_with("frame,t_ms,dt_ms,state,occluded,"));
    assert_eq!(lines[0], pieced::session::csv_header());
    assert_eq!(lines.len(), rows.len() + 1);
    let state_of = |l: &str| l.split(',').skip(3).take(2).collect::<Vec<_>>().join(",");
    assert_eq!(state_of(lines[1]), "boot,0");
    assert!(lines.iter().any(|l| state_of(l) == "paused,0"));
    assert_eq!(state_of(lines.last().unwrap()), "playing,0");
    let columns = lines[0].split(',').count();
    assert!(lines.iter().all(|l| l.split(',').count() == columns));

    // The final JSON.
    let doc = read_json(&dir.join("session.json"));
    assert_eq!(doc["final"], true);
    assert_eq!(doc["commit"], "abc123");
    assert_eq!(doc["build"], "release");
    assert_eq!(doc["graphics_preset"], "Battery");
    assert_eq!(doc["launch"]["ms"], 2345.6);
    assert_eq!(doc["launch"]["kind"], "warm");
    assert_eq!(doc["launch"]["boot_phases"][1]["phase"], "models");
    assert_eq!(doc["s2"]["verdict"], "PASS");
    assert_eq!(doc["s2_line"], outcome.line.as_str());
    assert_eq!(doc["frames_logged"], rows.len() as u64);
    let counted = doc["counted_play_s"].as_f64().unwrap();
    // 360 s of play, less the first 10 s after launch (7 s of it in play)
    // and 1 s after the pause.
    assert!((351.5..352.5).contains(&counted), "{counted}");
    assert!((doc["play_s"].as_f64().unwrap() - 360.0).abs() < 0.1);
    // A sample on the first frame, one every 30 s, one at the end.
    let samples = doc["power_samples"].as_array().unwrap().len();
    let span_s = rows.last().unwrap().t_ms / 1000.0;
    assert_eq!(samples, (span_s / 30.0).ceil() as usize + 1);

    // Retention ran: 31 old + 1 new, 30 kept.
    let sessions = std::fs::read_dir(&root)
        .unwrap()
        .filter(|e| is_session_name(&e.as_ref().unwrap().file_name().to_string_lossy()))
        .count();
    assert_eq!(sessions, KEEP_SESSIONS);
    assert_eq!(doc["sessions_pruned"], 2);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn writer_marks_a_plugged_in_or_short_session_not_applicable() {
    let root = temp_dir("writer-na");
    let mut writer = SessionWriter::spawn(WriterConfig {
        sessions_dir: root.clone(),
        stamp: "20260927-110000".into(),
        keep: KEEP_SESSIONS,
        power: sampler("AC Power", Some(false)),
        meta: meta(false),
    })
    .unwrap();
    for r in simulated_rows(60.0) {
        writer.push_wait(r);
    }
    let outcome = writer.finish(Duration::from_secs(10)).unwrap().clone();
    assert_eq!(outcome.report.verdict, S2Verdict::NotApplicable);
    let why = outcome.line.rsplit(' ').next().unwrap();
    let reasons: Vec<&str> = why.split(',').collect();
    assert!(reasons[0].starts_with("play="));
    assert!(reasons[1].starts_with("not-on-battery="));
    assert!(reasons[2].starts_with("low-power-off="));
    assert_eq!(reasons[3], "build=debug");
    let doc = read_json(&outcome.dir.unwrap().join("session.json"));
    assert_eq!(doc["s2"]["verdict"], "N/A");
    assert_eq!(doc["build"], "debug");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_session_dropped_without_finishing_still_leaves_its_data() {
    // Dropping the writer (the app's world cleared on an abrupt quit)
    // finishes the session: rows flushed and a final summary written.
    let root = temp_dir("writer-drop");
    let writer = SessionWriter::spawn(WriterConfig {
        sessions_dir: root.clone(),
        stamp: "20260927-120000".into(),
        keep: KEEP_SESSIONS,
        power: sampler("Battery Power", Some(true)),
        meta: meta(true),
    })
    .unwrap();
    let rows = simulated_rows(20.0);
    for r in &rows {
        writer.push_wait(*r);
    }
    drop(writer);
    let dir = root.join("20260927-120000");
    let csv = std::fs::read_to_string(dir.join("frames.csv")).unwrap();
    assert_eq!(csv.lines().count(), rows.len() + 1);
    let doc = read_json(&dir.join("session.json"));
    assert_eq!(doc["final"], true);
    assert_eq!(doc["s2"]["verdict"], "N/A");
    let _ = std::fs::remove_dir_all(root);
}

// ---------------------------------------------------------------------------
// Boot phases
// ---------------------------------------------------------------------------

#[test]
fn boot_phases_record_each_gate_release_in_order() {
    let mut phases = BootPhases::default();
    phases.observe(120.0, &["far", "models", "warmup"], false, false);
    phases.observe(400.0, &["far", "models", "warmup"], true, false);
    phases.observe(1500.0, &["far", "warmup"], false, false);
    phases.observe(2100.0, &["warmup"], false, false);
    // The warm-up re-holds for a second round, then releases.
    phases.observe(2200.0, &[], false, false);
    phases.observe(2300.0, &["warmup"], false, false);
    phases.observe(4800.0, &[], false, true);
    phases.controllable_ms = Some(4850.0);
    assert_eq!(
        phases.line(),
        "PIECED_BOOT first_frame=120 window=400 models=1500 far=2100 warmup=4800 \
         playing=4800 controllable=4850"
    );
}

#[test]
fn boot_phases_split_the_time_before_the_window() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
        .init_state::<AppState>()
        .add_message::<WindowOccluded>()
        .add_message::<WindowCreated>()
        .add_plugins(TelemetryPlugin)
        .add_plugins(pieced::telemetry::BootMarkPlugin);
    pieced::telemetry::mark_app_built(&mut app);
    app.finish();
    app.cleanup();
    app.update();
    let p = app.world().resource::<BootPhases>().clone();
    let order = [
        p.app_built_ms,
        p.plugins_ready_ms,
        p.startup_start_ms,
        p.startup_end_ms,
        p.first_frame_ms,
    ];
    assert!(order.iter().all(Option::is_some), "{p:?}");
    assert!(order.windows(2).all(|w| w[0] <= w[1]), "{p:?}");
    let line = p.line();
    assert!(line.starts_with("PIECED_BOOT app_built="), "{line}");
    for phase in [
        "plugins_ready=",
        "startup_start=",
        "startup_end=",
        "first_frame=",
    ] {
        assert!(line.contains(phase), "{line}");
    }
    // Later frames don't move them.
    app.update();
    assert_eq!(
        app.world().resource::<BootPhases>().startup_end_ms,
        p.startup_end_ms
    );
}

// ---------------------------------------------------------------------------
// The plugin, end to end
// ---------------------------------------------------------------------------

fn session_app(root: &Path, stamp: &str) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
        .init_state::<AppState>()
        .init_resource::<Tuning>()
        .add_message::<WindowOccluded>()
        .add_message::<WindowCreated>()
        .add_plugins(TelemetryPlugin)
        .add_plugins(SessionPlugin {
            log_frames: true,
            sessions_dir: root.to_path_buf(),
            power: sampler("Battery Power", Some(true)),
            stamp: Some(stamp.into()),
        });
    app
}

#[test]
fn a_simulated_session_writes_its_folder_and_prints_the_quit_line() {
    let root = temp_dir("plugin");
    let mut app = session_app(&root, "20260927-130000");
    for _ in 0..3 {
        app.update();
    }
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Playing);
    for _ in 0..3 {
        app.update();
    }
    // The first launch into an empty folder is cold.
    assert_eq!(app.world().resource::<LaunchInfo>().kind, LaunchKind::Cold);
    // Controllable (a real launch waits for the cursor lock).
    app.world_mut().resource_mut::<LaunchTime>().0 = Some(Duration::from_millis(1234));
    app.update();
    app.world_mut().write_message(AppExit::Success);
    app.update();

    let log = app.world().resource::<SessionLog>();
    let line = log.quit_line().expect("printed at AppExit").to_string();
    assert!(line.starts_with("PIECED_S2 N/A "), "{line}");
    assert!(line.contains("play=0s<300s"), "{line}");
    assert_eq!(line.lines().count(), 1);
    let dir = log.outcome().unwrap().dir.clone().unwrap();
    assert_eq!(dir, root.join("20260927-130000"));
    drop(app);

    let csv = std::fs::read_to_string(dir.join("frames.csv")).unwrap();
    let states: Vec<&str> = csv
        .lines()
        .skip(1)
        .map(|l| l.split(',').nth(3).unwrap())
        .collect();
    assert!(states.len() >= 7, "{states:?}");
    assert_eq!(states[0], "boot");
    assert_eq!(*states.last().unwrap(), "playing");
    let doc = read_json(&dir.join("session.json"));
    assert_eq!(doc["final"], true);
    assert_eq!(doc["launch"]["kind"], "cold");
    assert_eq!(doc["launch"]["ms"], 1234.0);
    let phases: Vec<&str> = doc["launch"]["boot_phases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["phase"].as_str().unwrap())
        .collect();
    assert_eq!(
        phases,
        ["startup_start", "startup_end", "first_frame", "playing"]
    );
    assert_eq!(
        doc["graphics_preset"],
        format!("{:?}", QualityPreset::Battery)
    );
    assert!(root.join("last_build").is_file());

    // The same binary again: warm, in a second folder.
    let mut again = session_app(&root, "20260927-130500");
    again.update();
    assert_eq!(
        again.world().resource::<LaunchInfo>().kind,
        LaunchKind::Warm
    );
    drop(again);
    // Dropped without AppExit, it still finished its folder.
    let doc = read_json(&root.join("20260927-130500").join("session.json"));
    assert_eq!(doc["final"], true);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn stamp_of_now_is_a_session_name() {
    assert!(is_session_name(&stamp_utc(SystemTime::now())));
}

#[test]
fn the_commit_is_embedded_at_build_time() {
    // build.rs embeds the short hash (with -dirty for uncommitted changes),
    // or "unknown" when built outside the repository.
    let commit = pieced::session::build_commit();
    let hash = commit.strip_suffix("-dirty").unwrap_or(&commit);
    assert!(
        hash == "unknown" || (hash.len() == 12 && hash.chars().all(|c| c.is_ascii_hexdigit())),
        "{commit}"
    );
    eprintln!("embedded commit: {commit}");
}

// ---------------------------------------------------------------------------
// Frame attribution (M3 performance slice)
// ---------------------------------------------------------------------------

/// The columns `scripts/sessions.py --spikes` reads (its tests use the same
/// header).
const ATTRIBUTED_HEADER: &str = "frame,t_ms,dt_ms,state,occluded,pre_ms,fixed_ms,physics_ms,\
    ticks,update_ms,post_ms,extract_ms,prepare_ms,acquire_ms,graph_ms,render_end_ms,idle_ms,\
    vsync_dt_ms,gpu_ms,work_ms,knights,knights_spawned,orbs,orbs_fired,shots,damage,placed,\
    cracked,broken,particles,debris,spell_fx,potions,damage_numbers,voices,voices_started,\
    pipelines_compiled,entities";

#[test]
fn the_csv_header_lists_every_attribution_column() {
    assert_eq!(pieced::session::csv_header(), ATTRIBUTED_HEADER);
}

/// A normal frame: 16.7 ms, most of it waiting for the drawable.
fn attributed(frame: u64, t_ms: f64, dt_ms: f64) -> SessionFrame {
    let mut row = row(frame, t_ms, dt_ms, FrameState::Playing);
    let c = &mut row.cost;
    c.pre_ms = 0.5;
    c.fixed_ms = 2.0;
    c.ticks = 1;
    c.update_ms = 2.0;
    c.post_ms = 1.0;
    c.extract_ms = 0.5;
    c.prepare_ms = 1.0;
    c.acquire_ms = 8.0;
    c.graph_ms = 1.0;
    c.render_end_ms = 0.1;
    c.idle_ms = 0.3;
    c.vsync_dt_ms = 16.667;
    c.counters.knights = 4;
    c.counters.entities = 3000;
    row
}

/// The scenario `scripts/test_sessions.py` uses: 30 s of play with a 40 ms
/// first-use compile (after a knight spawn), a 34 ms Update overrun, then a
/// 30 ms compile at 20 s, and one late-then-early pair at 25 s.
fn spiky_rows() -> Vec<SessionFrame> {
    let mut rows = Vec::new();
    let mut t = 0.0;
    for i in 0..1800u64 {
        let dt = match i {
            1200 => 40.0,
            1201 => 34.0,
            1203 => 30.0,
            1500 => 19.0,
            1501 => 14.0,
            _ => 16.667,
        };
        t += dt;
        let mut r = attributed(i + 1, t, dt);
        if i == 1199 {
            r.cost.counters.knights_spawned = 1;
        }
        if i == 1200 || i == 1203 {
            r.cost.graph_ms = 20.0;
            r.cost.counters.pipelines_compiled = 2;
        }
        if i == 1201 {
            r.cost.update_ms = 12.0;
        }
        rows.push(r);
    }
    rows
}

#[test]
fn spikes_are_attributed_to_the_bucket_that_ran_long() {
    let mut stats = pieced::session::SpikeStats::default();
    let mut filter = pieced::session::S2Filter::default();
    for r in spiky_rows() {
        let counted = filter.admit(&r);
        stats.observe(&r, counted);
    }
    assert_eq!(stats.spikes(), 3);
    assert_eq!(stats.by_cause(), vec![("graph", 2), ("update", 1)]);
    let doc = stats.to_json();
    assert_eq!(doc["spike_runs"], 2);
    assert_eq!(doc["late_then_early_pairs"], 1);
    // Events count on the spike's row or the row before.
    assert_eq!(doc["events"]["pipeline_compile"]["spikes"], 3);
    assert_eq!(doc["events"]["knight_spawn"]["spikes"], 1);
    assert_eq!(doc["worst"][0]["dt_ms"], 40.0);
    assert_eq!(doc["worst"][0]["cause"], "graph");
    assert!((doc["worst"][0]["excess_ms"].as_f64().unwrap() - 19.0).abs() < 1e-3);
    assert_eq!(doc["vsync_pct_under_18_ms"], 100.0);
    assert!((doc["mean_ms_on_normal_frames"]["acquire"].as_f64().unwrap() - 8.0).abs() < 1e-6);
}

#[test]
fn the_writer_streams_attribution_and_writes_the_spike_report() {
    let root = temp_dir("writer-spikes");
    let mut writer = SessionWriter::spawn(WriterConfig {
        sessions_dir: root.clone(),
        stamp: "20260928-100000".into(),
        keep: KEEP_SESSIONS,
        power: sampler("Battery Power", Some(true)),
        meta: meta(true),
    })
    .unwrap();
    for r in spiky_rows() {
        writer.push_wait(r);
    }
    let outcome = writer.finish(Duration::from_secs(10)).unwrap().clone();
    let dir = outcome.dir.unwrap();
    let csv = std::fs::read_to_string(dir.join("frames.csv")).unwrap();
    let mut lines = csv.lines();
    let header: Vec<&str> = lines.next().unwrap().split(',').collect();
    let spike: Vec<&str> = lines.nth(1200).unwrap().split(',').collect();
    let col = |name: &str| spike[header.iter().position(|h| *h == name).unwrap()];
    assert_eq!(col("dt_ms"), "40.000");
    assert_eq!(col("graph_ms"), "20.000");
    assert_eq!(col("acquire_ms"), "8.000");
    assert_eq!(col("pipelines_compiled"), "2");
    assert_eq!(col("knights"), "4");
    assert_eq!(col("entities"), "3000");
    // No GPU timing without the knob: an empty cell, not a zero.
    assert_eq!(col("gpu_ms"), "");
    assert_eq!(col("work_ms"), "27.100");
    let doc = read_json(&dir.join("session.json"));
    assert_eq!(doc["spikes"]["spikes"], 3);
    assert_eq!(doc["spikes"]["by_cause"][0]["cause"], "graph");
    assert_eq!(doc["spikes"]["by_cause"][0]["spikes"], 2);
    let _ = std::fs::remove_dir_all(root);
}

/// The headless game (as `app::headless_app`) plus telemetry, the frame
/// profiler and the session log, in Waves mode with a player who can't die.
fn profiled_waves_app(root: &Path, seed: u64) -> App {
    use avian3d::prelude::PhysicsPlugins;
    use bevy::time::TimeUpdateStrategy;
    use pieced::{
        rng::{Rng, SimRng},
        shared::{GameMode, tick_duration},
    };
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        bevy::state::app::StatesPlugin,
        AssetPlugin::default(),
        bevy::mesh::MeshPlugin,
        bevy::scene::ScenePlugin,
        PhysicsPlugins::default(),
    ))
    .add_plugins(pieced::app::SimPlugins)
    .add_message::<WindowOccluded>()
    .add_message::<WindowCreated>()
    .add_plugins((TelemetryPlugin, pieced::profile::FrameProfilePlugin))
    .add_plugins(SessionPlugin {
        log_frames: true,
        sessions_dir: root.to_path_buf(),
        power: sampler("Battery Power", Some(true)),
        stamp: Some("20260928-110000".into()),
    })
    .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()))
    .insert_resource(SimRng(Rng::new(seed)))
    .insert_resource(GameMode::Waves);
    app.finish();
    app.cleanup();
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Playing);
    app.update();
    let player = app
        .world_mut()
        .query_filtered::<Entity, With<pieced::shared::Player>>()
        .single(app.world())
        .unwrap();
    let mut health = pieced::shared::Health::full(1.0e9, 100.0);
    health.shield = 100.0;
    *app.world_mut()
        .get_mut::<pieced::shared::Health>(player)
        .unwrap() = health;
    app
}

#[test]
fn a_headless_waves_session_fills_the_attribution_columns() {
    let root = temp_dir("profiled");
    let mut app = profiled_waves_app(&root, 7);
    // 40 simulated seconds: the first wave poofs in and starts casting.
    for _ in 0..(40 * 60) {
        app.update();
    }
    app.world_mut().write_message(AppExit::Success);
    app.update();
    let dir = app
        .world()
        .resource::<SessionLog>()
        .outcome()
        .unwrap()
        .dir
        .clone()
        .unwrap();
    drop(app);

    let csv = std::fs::read_to_string(dir.join("frames.csv")).unwrap();
    let mut lines = csv.lines();
    let header: Vec<&str> = lines.next().unwrap().split(',').collect();
    assert_eq!(header.join(","), ATTRIBUTED_HEADER);
    let idx = |name: &str| header.iter().position(|h| *h == name).unwrap();
    let rows: Vec<Vec<&str>> = lines
        .map(|l| l.split(',').collect::<Vec<&str>>())
        .filter(|r| r[idx("state")] == "playing")
        .collect();
    assert!(rows.len() > 2000, "{} playing rows", rows.len());
    assert!(rows.iter().all(|r| r.len() == header.len()));
    let num = |r: &Vec<&str>, name: &str| r[idx(name)].parse::<f64>().unwrap();
    let sum = |name: &str| rows.iter().map(|r| num(r, name)).sum::<f64>();
    let max = |name: &str| rows.iter().map(|r| num(r, name)).fold(0.0, f64::max);
    // The main world's parts are timed on every frame...
    for bucket in ["pre_ms", "fixed_ms", "update_ms", "post_ms"] {
        assert!(
            rows.iter().all(|r| num(r, bucket) > 0.0),
            "{bucket} not filled on every playing frame"
        );
    }
    assert!(sum("physics_ms") > 0.0);
    assert!(sum("physics_ms") < sum("fixed_ms"));
    // ...one fixed tick per update here...
    assert!(rows.iter().skip(1).all(|r| num(r, "ticks") == 1.0));
    // ...and there is no render world headless, so its parts read zero.
    for bucket in [
        "extract_ms",
        "prepare_ms",
        "acquire_ms",
        "graph_ms",
        "vsync_dt_ms",
    ] {
        assert_eq!(sum(bucket), 0.0, "{bucket}");
    }
    assert!(rows.iter().all(|r| r[idx("gpu_ms")].is_empty()));
    // The counters follow the fight.
    assert!(sum("knights_spawned") >= 3.0, "knights spawned");
    assert!(max("knights") >= 3.0);
    assert!(sum("orbs_fired") > 0.0, "no orb fired in 40 s");
    assert!(max("orbs") > 0.0);
    assert!(sum("damage") > 0.0);
    assert!(max("entities") > 100.0);
    assert!(max("potions") <= pieced::waves::POTION_POOL as f64);
    let doc = read_json(&dir.join("session.json"));
    assert!(doc["spikes"]["attributed_frames"].is_u64());
    let _ = std::fs::remove_dir_all(root);
}

// ---------------------------------------------------------------------------
// Five minutes of live waves with the visuals: nothing grows
// ---------------------------------------------------------------------------

/// The headless game with the look, models, arena and building visuals,
/// effects (orbs, potions, spells, debris), the HUD and the frame profiler:
/// one fixed tick per update, no GPU. Waves mode, models loaded, `Playing`.
fn waves_with_visuals(seed: u64) -> App {
    use avian3d::prelude::PhysicsPlugins;
    use bevy::{
        gltf::GltfPlugin, input::InputPlugin, time::TimeUpdateStrategy,
        world_serialization::WorldSerializationPlugin,
    };
    use pieced::{
        rng::{Rng, SimRng},
        shared::{GameMode, tick_duration},
    };
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        bevy::state::app::StatesPlugin,
        AssetPlugin::default(),
        bevy::mesh::MeshPlugin,
        GltfPlugin::default(),
        WorldSerializationPlugin,
        PhysicsPlugins::default(),
        InputPlugin,
    ))
    .init_asset::<Image>()
    .init_asset::<StandardMaterial>()
    .add_plugins(pieced::app::SimPlugins)
    .add_plugins((
        pieced::look::LookPlugin,
        pieced::models::ModelsPlugin,
        pieced::arena::visuals::ArenaVisualsPlugin,
        pieced::building::BuildingVisualsPlugin,
        pieced::fx::FxPlugin,
        pieced::hud::HudPlugin,
        pieced::profile::FrameProfilePlugin,
    ))
    .init_resource::<pieced::render::CurrentFov>()
    .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()))
    .insert_resource(SimRng(Rng::new(seed)))
    .insert_resource(GameMode::Waves);
    app.finish();
    app.cleanup();
    let mut loaded = false;
    for _ in 0..3000 {
        app.update();
        if app
            .world()
            .get_resource::<pieced::models::ModelLibrary>()
            .is_some_and(|m| m.is_ready())
            && app
                .world()
                .contains_resource::<pieced::building::visuals::PieceAssets>()
        {
            loaded = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(loaded, "the models never loaded");
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Playing);
    app.update();
    let player = app
        .world_mut()
        .query_filtered::<Entity, With<pieced::shared::Player>>()
        .single(app.world())
        .unwrap();
    let mut health = pieced::shared::Health::full(1.0e9, 100.0);
    health.shield = 100.0;
    *app.world_mut()
        .get_mut::<pieced::shared::Health>(player)
        .unwrap() = health;
    app
}

/// Assets of every kind the game makes at run time.
fn asset_counts(app: &App) -> Vec<(&'static str, usize)> {
    let w = app.world();
    vec![
        ("meshes", w.resource::<Assets<Mesh>>().len()),
        (
            "toon",
            w.resource::<Assets<pieced::look::ToonMaterial>>().len(),
        ),
        (
            "ink",
            w.resource::<Assets<pieced::look::InkMaterial>>().len(),
        ),
        ("standard", w.resource::<Assets<StandardMaterial>>().len()),
        ("images", w.resource::<Assets<Image>>().len()),
    ]
}

/// Five simulated minutes of endless waves with live brains and every
/// effect: knights poof in, walk, wind up and cast orbs at a player who can't
/// die, and each is downed 6 s after it lands (so the waves grow to the
/// 8-alive cap); breaks are skipped. After the first half minute (lazy
/// one-time assets), no asset is created and no entity is left behind.
#[test]
fn five_minutes_of_live_waves_with_every_effect_grow_nothing() {
    use pieced::{
        combat::Downed,
        grunt::Parked,
        shared::{DamageDealt, DamageTarget, Health, Player},
        waves::{PoolGrunt, Run, RunPhase, SkipBreak},
    };
    let mut app = waves_with_visuals(5);
    let in_play = |app: &mut App| -> Vec<Entity> {
        app.world_mut()
            .query_filtered::<Entity, (With<PoolGrunt>, Without<Parked>, Without<Downed>)>()
            .iter(app.world())
            .collect()
    };
    let entities =
        |app: &mut App| -> usize { app.world_mut().query::<Entity>().iter(app.world()).count() };
    let player = app
        .world_mut()
        .query_filtered::<Entity, With<Player>>()
        .single(app.world())
        .unwrap();
    let mut landed: Vec<(Entity, u32)> = Vec::new();
    let mut baseline: Option<(Vec<(&'static str, usize)>, usize)> = None;
    let mut most_alive = 0;
    let mut orbs_fired = 0u32;
    for frame in 0..(5 * 60 * 60u32) {
        let alive = in_play(&mut app);
        most_alive = most_alive.max(alive.len());
        landed.retain(|(e, _)| alive.contains(e));
        for &g in &alive {
            if !landed.iter().any(|(e, _)| *e == g) {
                landed.push((g, frame));
            }
        }
        let due: Vec<Entity> = landed
            .iter()
            .filter(|(_, at)| frame >= at + 360)
            .map(|(e, _)| *e)
            .collect();
        for g in due {
            let at = app.world().get::<Transform>(g).unwrap().translation;
            let hp = app.world().get::<Health>(g).unwrap().hp;
            app.world_mut().get_mut::<Health>(g).unwrap().hp = 0.0;
            let tick = app.world().resource::<pieced::shared::SimTick>().0;
            app.world_mut().entity_mut(g).insert(Downed { tick });
            app.world_mut().write_message(DamageDealt {
                source: Some(player),
                target: g,
                target_kind: DamageTarget::Character,
                amount: hp,
                to_shield: 0.0,
                headshot: false,
                shield_broke: false,
                killed: true,
                point: at + Vec3::Y,
                normal: Vec3::Z,
                tick,
            });
        }
        if matches!(app.world().resource::<Run>().phase, RunPhase::Break { .. }) {
            app.world_mut().write_message(SkipBreak);
        }
        app.update();
        orbs_fired += u32::from(
            app.world()
                .resource::<pieced::profile::LastCounters>()
                .0
                .orbs_fired,
        );
        if frame == 30 * 60 {
            baseline = Some((asset_counts(&app), entities(&mut app)));
        }
    }
    let run = app.world().resource::<Run>().clone();
    assert!(!run.is_ended(), "the player survived");
    assert!(run.wave >= 6, "reached wave {}", run.wave);
    assert_eq!(most_alive, 8, "the cap of eight was reached");
    assert!(orbs_fired > 50, "only {orbs_fired} orbs cast");
    let (assets, count) = baseline.unwrap();
    assert_eq!(asset_counts(&app), assets, "assets created during play");
    let now = entities(&mut app);
    assert!(now <= count, "entities grew from {count} to {now}");
}

// The per-frame path of the profiler and the session log allocates nothing:
// a counting allocator (armed on the test thread only) compares the frames of
// an app with and without them.

struct CountingAlloc;

static ALLOCATIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

thread_local! {
    static ARMED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn count_allocation() {
    if ARMED.with(std::cell::Cell::get) {
        ALLOCATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

// SAFETY: forwards every call to the system allocator unchanged.
unsafe impl std::alloc::GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        count_allocation();
        unsafe { std::alloc::System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        count_allocation();
        unsafe { std::alloc::System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, size: usize) -> *mut u8 {
        count_allocation();
        unsafe { std::alloc::System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

/// Allocations on this thread while `f` runs.
fn allocations_in(f: impl FnOnce()) -> u64 {
    let before = ALLOCATIONS.load(std::sync::atomic::Ordering::Relaxed);
    ARMED.with(|a| a.set(true));
    f();
    ARMED.with(|a| a.set(false));
    ALLOCATIONS.load(std::sync::atomic::Ordering::Relaxed) - before
}

/// A minimal app with the messages and resources the session log reads, its
/// schedules single-threaded (so every system runs on this thread), with or
/// without the profiler and the session log.
fn minimal_app(root: &Path, profiled: bool) -> App {
    use bevy::ecs::schedule::{Schedules, SingleThreadedExecutor};
    use pieced::shared::{DamageDealt, GameCue, PieceChanged, ShotFired};
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
        .init_state::<AppState>()
        .init_resource::<Tuning>()
        .init_resource::<pieced::telemetry::LastFrame>()
        .init_resource::<pieced::telemetry::RunConditions>()
        .init_resource::<LaunchTime>()
        .init_resource::<BootPhases>()
        .add_message::<WindowOccluded>()
        .add_message::<WindowCreated>()
        .add_message::<GameCue>()
        .add_message::<ShotFired>()
        .add_message::<DamageDealt>()
        .add_message::<PieceChanged>();
    if profiled {
        app.add_plugins(pieced::profile::FrameProfilePlugin)
            .add_plugins(SessionPlugin {
                log_frames: true,
                sessions_dir: root.to_path_buf(),
                power: sampler("Battery Power", Some(true)),
                stamp: Some("20260928-120000".into()),
            });
    }
    app.finish();
    app.cleanup();
    for (_, schedule) in app.world_mut().resource_mut::<Schedules>().iter_mut() {
        schedule.set_executor(SingleThreadedExecutor::new());
    }
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Playing);
    app
}

#[test]
fn profiling_and_logging_a_frame_allocate_nothing() {
    let root = temp_dir("allocations");
    let mut plain = minimal_app(&root, false);
    let mut profiled = minimal_app(&root, true);
    // Warm up: systems initialize, message buffers and the channel settle.
    for _ in 0..30 {
        plain.update();
        profiled.update();
    }
    let frames = 300;
    let base = allocations_in(|| {
        for _ in 0..frames {
            plain.update();
        }
    });
    let with = allocations_in(|| {
        for _ in 0..frames {
            profiled.update();
        }
    });
    assert!(
        with <= base,
        "the profiler and session log allocated {} times over {frames} frames ({with} vs {base})",
        with.saturating_sub(base)
    );
    // And the pieces called directly.
    let profile = pieced::profile::FrameProfile::default();
    let direct = allocations_in(|| {
        for i in 0..1000u64 {
            for m in pieced::profile::Mark::ALL {
                profile.mark(m);
            }
            profile.physics_start(i);
            profile.physics_end(i + 1);
            profile.pipelines_ready(100 + i as u32);
            std::hint::black_box(profile.cost());
        }
    });
    assert_eq!(direct, 0);
    drop(profiled);
    let _ = std::fs::remove_dir_all(root);
}
