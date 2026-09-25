//! Telemetry and scenarios: latency statistics, gate thresholds, the perf
//! script's determinism, frame-cap arithmetic, and the perf and ttk directors
//! driven through the headless simulation seam.

use bevy::prelude::*;
use pieced::{
    combat::CombatStats,
    render::{FramePacer, LIMITER_SPIN, frame_interval, wait_split},
    scenario::{
        Director, DirectorStatus, ScenarioClock,
        perf::{Activity, PerfScript},
    },
    shared::{PieceChange, PieceChanged, PlayerIntent},
    sim::Sim,
    telemetry::{
        FrameRow, FrameSummary, LatencyStats, evaluate_g2, evaluate_g3, evaluate_g6,
        intervals_in_window, latency_stats, summarize,
    },
};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Latency statistics
// ---------------------------------------------------------------------------

#[test]
fn latency_stats_median_percentile_and_extremes() {
    // Odd count: the middle value.
    let s = latency_stats(&[30.0, 10.0, 20.0]);
    assert_eq!(s.samples, 3);
    assert_eq!((s.min_ms, s.median_ms, s.max_ms), (10.0, 20.0, 30.0));
    assert!((s.mean_ms - 20.0).abs() < 1e-12);

    // Even count: the mean of the two middle values.
    let s = latency_stats(&[4.0, 1.0, 3.0, 2.0]);
    assert_eq!(s.median_ms, 2.5);

    // Nearest-rank p95 over 1..=100 is 95; over 1..=20 it is 19.
    let hundred: Vec<f64> = (1..=100).map(f64::from).collect();
    assert_eq!(latency_stats(&hundred).p95_ms, 95.0);
    let twenty: Vec<f64> = (1..=20).rev().map(f64::from).collect();
    let s = latency_stats(&twenty);
    assert_eq!(s.p95_ms, 19.0);
    assert_eq!(s.median_ms, 10.5);

    // One outlier moves the max and the mean, not the median.
    let mut v = vec![12.0; 99];
    v.push(80.0);
    let s = latency_stats(&v);
    assert_eq!((s.median_ms, s.p95_ms, s.max_ms), (12.0, 12.0, 80.0));
    assert!((s.mean_ms - 12.68).abs() < 1e-9);

    assert_eq!(latency_stats(&[]), LatencyStats::default());
}

// ---------------------------------------------------------------------------
// Gates
// ---------------------------------------------------------------------------

fn frames(mean: f64, over_25: usize, pct_under_18: f64) -> FrameSummary {
    FrameSummary {
        frames: 18_000,
        mean_ms: mean,
        over_25_ms: over_25,
        pct_under_18_ms: pct_under_18,
        ..Default::default()
    }
}

#[test]
fn g2_passes_only_steady_60fps_without_hitches() {
    assert!(evaluate_g2(&frames(16.67, 0, 99.9)).pass);
    // The mean window is inclusive at both ends.
    assert!(evaluate_g2(&frames(16.4, 0, 99.0)).pass);
    assert!(evaluate_g2(&frames(17.0, 0, 99.0)).pass);
    // Too fast (no vsync), too slow, one hitch, too many slow frames.
    assert!(!evaluate_g2(&frames(16.39, 0, 100.0)).pass);
    assert!(!evaluate_g2(&frames(17.01, 0, 99.5)).pass);
    let hitch = evaluate_g2(&frames(16.67, 1, 99.9));
    assert!(!hitch.pass);
    let failed: Vec<&str> = hitch
        .checks
        .iter()
        .filter(|c| !c.pass)
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(failed, ["frames_over_25_ms"]);
    assert!(!evaluate_g2(&frames(16.67, 0, 98.99)).pass);
    // Nothing measured is not a pass.
    assert!(!evaluate_g2(&FrameSummary::default()).pass);
}

#[test]
fn g2_evaluates_a_real_summary() {
    let mut v = vec![16.667; 9_950];
    v.extend([17.5; 49]);
    v.push(24.9);
    assert!(evaluate_g2(&summarize(&v)).pass);
    v.push(25.1);
    assert!(!evaluate_g2(&summarize(&v)).pass);
}

#[test]
fn g3_needs_100_samples_and_a_median_of_at_most_33ms() {
    let ok: Vec<f64> = (0..100).map(|i| 20.0 + (i % 20) as f64).collect();
    assert!(evaluate_g3("x", &latency_stats(&ok)).pass);
    let edge = vec![33.0; 100];
    assert!(evaluate_g3("x", &latency_stats(&edge)).pass);
    let slow = vec![33.1; 100];
    assert!(!evaluate_g3("x", &latency_stats(&slow)).pass);
    let few = vec![10.0; 99];
    assert!(!evaluate_g3("x", &latency_stats(&few)).pass);
    assert!(!evaluate_g3("x", &latency_stats(&[])).pass);
}

#[test]
fn g6_checks_every_kill_the_pump_and_the_wall() {
    let kills = [1.1667; 5];
    assert!(evaluate_g6(&kills, &[false, false], Some(1.1667)).pass);
    // Boundaries are inclusive.
    assert!(evaluate_g6(&[1.0, 2.0, 1.5, 1.2, 1.9], &[false], Some(1.0)).pass);
    // One slow kill fails the gate.
    assert!(!evaluate_g6(&[1.17, 1.17, 1.17, 1.17, 2.01], &[false], Some(1.2)).pass);
    assert!(!evaluate_g6(&[0.99, 1.17, 1.17, 1.17, 1.17], &[false], Some(1.2)).pass);
    // Too few kills; a kill that never happened.
    assert!(!evaluate_g6(&kills[..4], &[false], Some(1.2)).pass);
    assert!(
        !evaluate_g6(
            &[1.17, 1.17, 1.17, 1.17, f64::INFINITY],
            &[false],
            Some(1.2)
        )
        .pass
    );
    // Any pump kill from full, or no pump shot at all.
    assert!(!evaluate_g6(&kills, &[false, true], Some(1.2)).pass);
    assert!(!evaluate_g6(&kills, &[], Some(1.2)).pass);
    // The wall must soak a full second, and must have been measured.
    assert!(!evaluate_g6(&kills, &[false], Some(0.99)).pass);
    assert!(!evaluate_g6(&kills, &[false], None).pass);
}

#[test]
fn window_excludes_warmup_and_frames_after_the_end() {
    let rows: Vec<FrameRow> = (0..20)
        .map(|i| FrameRow {
            frame: i,
            t_ms: 500.0 + i as f64 * 10.0,
            dt_ms: 10.0 + i as f64,
            fixed_ticks: 1,
            playing: i >= 1,
        })
        .collect();
    // First playing frame at t=510; warm-up 50 ms keeps t ≥ 560 (frame 6);
    // the window ends at t=650 (frame 15).
    let v = intervals_in_window(&rows, 50.0, Some(650.0));
    assert_eq!(v, (6..=15).map(|i| 10.0 + i as f64).collect::<Vec<_>>());
    assert_eq!(intervals_in_window(&rows, 50.0, None).len(), 14);
}

// ---------------------------------------------------------------------------
// Frame cap
// ---------------------------------------------------------------------------

#[test]
fn frame_cap_intervals() {
    assert_eq!(frame_interval(0), None);
    let sixty = frame_interval(60).unwrap();
    assert!((sixty.as_secs_f64() * 1000.0 - 16.6667).abs() < 1e-3);
    assert!((frame_interval(120).unwrap().as_secs_f64() - 1.0 / 120.0).abs() < 1e-9);
}

#[test]
fn frame_pacer_keeps_a_fixed_cadence() {
    let ms = |x: f64| Duration::from_secs_f64(x / 1000.0);
    let interval = frame_interval(60).unwrap();
    let mut pacer = FramePacer::new(interval);
    let t0 = Instant::now();
    // The first frame starts the cadence without waiting.
    assert_eq!(pacer.next_deadline(t0), t0);
    // Frames that take 5 ms end exactly one interval apart.
    let mut deadline = t0;
    for k in 1..=10u32 {
        let now = deadline + ms(5.0);
        let next = pacer.next_deadline(now);
        assert_eq!(next, t0 + interval * k);
        let (sleep, spin) = wait_split(now, next, LIMITER_SPIN);
        assert_eq!(sleep + spin, next - now);
        assert_eq!(spin, LIMITER_SPIN);
        deadline = next;
    }
    // A frame 3 ms late keeps the cadence (no wait; the next one is shorter).
    let late = deadline + interval + ms(3.0);
    let d = pacer.next_deadline(late);
    assert_eq!(d, deadline + interval);
    assert_eq!(
        wait_split(late, d, LIMITER_SPIN),
        (Duration::ZERO, Duration::ZERO)
    );
    let next = pacer.next_deadline(d + ms(2.0));
    assert_eq!(next, d + interval);
    // A frame more than half an interval late restarts the cadence at `now`.
    let very_late = next + interval + ms(12.0);
    assert_eq!(pacer.next_deadline(very_late), very_late);
    assert_eq!(
        pacer.next_deadline(very_late + ms(1.0)),
        very_late + interval
    );
    // Short waits are all spin.
    let now = Instant::now();
    assert_eq!(
        wait_split(now, now + ms(0.4), LIMITER_SPIN),
        (Duration::ZERO, ms(0.4))
    );
}

// ---------------------------------------------------------------------------
// The perf script
// ---------------------------------------------------------------------------

/// Plays a script with a (possibly jittery) frame clock and returns every frame.
fn play(
    script: &PerfScript,
    seconds: f64,
    dt: impl Fn(u32) -> f64,
) -> Vec<pieced::scenario::perf::ScriptFrame> {
    let mut frames = Vec::new();
    let (mut t_prev, mut t, mut i) = (0.0, 0.0, 0);
    while t < seconds {
        frames.push(script.frame(t_prev, t));
        t_prev = t;
        t += dt(i);
        i += 1;
    }
    frames
}

#[test]
fn perf_script_is_deterministic_and_covers_every_activity() {
    let a = PerfScript::new(7, 120.0);
    let b = PerfScript::new(7, 120.0);
    assert_eq!(a, b);
    let jitter = |i: u32| 1.0 / 60.0 + [0.0, 0.002, -0.001, 0.004][i as usize % 4];
    assert_eq!(play(&a, 120.0, jitter), play(&b, 120.0, jitter));
    assert_ne!(PerfScript::new(8, 120.0).segments(), a.segments());

    // Enough plan for the session, back to back, and every activity recurs in
    // each cycle.
    let segs = a.segments();
    let last = segs.last().unwrap();
    assert!(last.start + last.duration >= 121.0);
    assert!(
        segs.windows(2)
            .all(|w| (w[0].start + w[0].duration - w[1].start).abs() < 1e-9)
    );
    for cycle in segs.chunks(Activity::ALL.len()).take(3) {
        for activity in Activity::ALL {
            assert!(cycle.iter().any(|s| s.activity == activity), "{activity:?}");
        }
    }
}

#[test]
fn perf_script_edges_fire_once_whatever_the_frame_rate() {
    let script = PerfScript::new(3, 90.0);
    let count = |frames: &[pieced::scenario::perf::ScriptFrame]| {
        let n = |f: fn(&pieced::scenario::perf::ScriptFrame) -> bool| {
            frames.iter().filter(|x| f(x)).count()
        };
        (
            n(|f| f.fire_pressed),
            n(|f| f.jump_pressed),
            n(|f| f.crouch_pressed),
            n(|f| f.select.is_some()),
            n(|f| f.ads_toggle_pressed),
        )
    };
    let at_60 = count(&play(&script, 90.0, |_| 1.0 / 60.0));
    let at_45 = count(&play(&script, 90.0, |_| 1.0 / 45.0));
    let at_120 = count(&play(&script, 90.0, |_| 1.0 / 120.0));
    assert_eq!(at_60, at_45);
    assert_eq!(at_60, at_120);
    let (fire, jump, crouch, select, ads) = at_60;
    assert!(
        fire > 20 && jump > 10 && crouch > 5 && select > 30 && ads > 2,
        "{at_60:?}"
    );
}

// ---------------------------------------------------------------------------
// Directors through the headless simulation
// ---------------------------------------------------------------------------

/// Runs a director for `seconds` of fixed ticks (one frame per tick), stopping
/// early when it reports done. Returns the player's intent after each frame's
/// director update.
fn run_director(
    sim: &mut Sim,
    director: &mut dyn Director,
    seconds: f64,
    requested: Option<f64>,
) -> Vec<PlayerIntent> {
    let mut intents = Vec::new();
    let frames = (seconds * 60.0).round() as u64;
    for frame in 1..=frames {
        let clock = ScenarioClock {
            frame,
            seconds: (frame - 1) as f64 / 60.0,
            tick: sim.sim_tick(),
            requested_seconds: requested,
        };
        let status = director.update(sim.world_mut(), &clock);
        intents.push(sim.player_intent().clone());
        sim.tick();
        if status == DirectorStatus::Done {
            break;
        }
    }
    intents
}

#[test]
fn perf_director_generates_heavy_play_deterministically() {
    let run = || {
        let mut sim = Sim::with_seed(11);
        sim.record::<PieceChanged>();
        let mut director = pieced::scenario::perf::director("perf").unwrap();
        let intents = run_director(&mut sim, director.as_mut(), 60.0, Some(1000.0));
        let changes = sim.recorded::<PieceChanged>();
        let placed = changes
            .iter()
            .filter(|c| c.change == PieceChange::Placed)
            .count();
        let destroyed = changes
            .iter()
            .filter(|c| c.change == PieceChange::Destroyed)
            .count();
        let stats = sim.world().resource::<CombatStats>().clone();
        let player = sim.player();
        (intents, placed, destroyed, stats, sim.feet(player))
    };
    let (intents, placed, destroyed, stats, feet) = run();
    println!(
        "60 s perf: {placed} placed, {destroyed} destroyed, {} shots, {} hits, {} eliminations",
        stats.shots, stats.hits, stats.eliminations
    );
    assert!(placed >= 20, "builds boxes and ramps: {placed}");
    assert!(destroyed >= 2, "breaks pieces: {destroyed}");
    assert!(stats.shots >= 30, "sprays and pumps: {}", stats.shots);
    assert!(stats.hits >= 5, "hits the dummy: {}", stats.hits);
    assert!(intents.iter().any(|i| i.sprint) && intents.iter().any(|i| i.crouch));
    assert!(
        intents
            .iter()
            .filter(|i| i.look_delta == Vec2::ZERO)
            .count()
            < intents.len() / 20,
        "the view keeps moving"
    );

    let (again, placed2, destroyed2, stats2, feet2) = run();
    assert_eq!(intents, again, "same seed, same intents");
    assert_eq!((placed, destroyed), (placed2, destroyed2));
    assert_eq!((stats.shots, stats.hits), (stats2.shots, stats2.hits));
    assert_eq!(feet, feet2);
}

#[test]
fn ttk_director_measures_the_contract_headlessly() {
    let mut sim = Sim::with_seed(5);
    let mut director = pieced::scenario::ttk::director("ttk").unwrap();
    run_director(&mut sim, director.as_mut(), 90.0, None);
    let summary = director.summary(sim.world_mut());
    println!("{}", serde_json::to_string_pretty(&summary).unwrap());
    let kills = summary["rifle_kills"].as_array().unwrap();
    assert!(kills.len() >= 5);
    for k in kills {
        let ttk = k["ttk_first_damage_to_kill_s"].as_f64().unwrap();
        assert!((1.0..=2.0).contains(&ttk), "{ttk}");
        assert_eq!(
            k["combat_stats_last_ttk_s"]
                .as_f64()
                .map(|t| (t * 1e3).round()),
            Some((ttk * 1e3).round())
        );
    }
    for p in summary["pump_shots"].as_array().unwrap() {
        assert!(p["damage"].as_f64().unwrap() > 0.0);
        assert_eq!(p["killed"], false);
    }
    assert!(
        summary["wall_soak"]["first_hit_to_break_s"]
            .as_f64()
            .unwrap()
            >= 1.0
    );
    assert_eq!(summary["gate"]["pass"], true, "{}", summary["gate"]);
}
