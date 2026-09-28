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
    assert_eq!(phases, ["first_frame", "playing"]);
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
