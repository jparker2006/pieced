//! The Waves run (docs/M3-SPEC.md → Waves, D79–D85) through the simulation
//! seam: the grunt pool, waves growing by two with at most eight in play, the
//! break and its skip, wave scaling, score, the personal best, the run log, the
//! death beat, restarting in place, the fixed seed, and no leaks.
//!
//! Most tests freeze the brains ([`GalleryFreeze`]) and down knights through
//! the same signals combat uses (`Downed` plus a killing [`DamageDealt`]), so
//! they check the director's rules, not the grunt AI; a few use real shots.

use bevy::prelude::*;
use pieced::{
    app::GameOptions,
    arena::ArenaLayout,
    building::{self, Piece, PieceSlot},
    combat::{CombatStats, Downed},
    dummy::{Dummy, look_toward},
    grunt::{Grunt, GruntStats, Parked},
    orb::{Orb, OrbSlot, PARKED},
    player::HEAD_CENTER,
    shared::{
        ActiveTool, AppState, Character, DamageDealt, DamageTarget, EyeHeight, Facing,
        GalleryFreeze, GameCue, GameMode, GridCell, Health, PlayerIntent, PreviousFeet, ShotFired,
        WeaponKind,
    },
    sim::Sim,
    tuning::Tuning,
    waves::{
        EDGE_INSET, EndRun, FIRST_SPAWN_DELAY, PARK_SPOT, PersonalBest, PoolGrunt, PotionSlot,
        RestartRun, Run, RunEnd, RunPhase, RunSeed, RunStore, RunSummary,
        SPAWN_MIN_PLAYER_DISTANCE, SPAWN_STAGGER, SkipBreak, VICTORY_HOP_SECONDS, VoidKill,
    },
};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// A Waves sim like [`Sim::waves`], with `setup` applied to the app before
/// `Startup` (a run store, a fixed seed).
fn start(seed: u64, setup: impl FnOnce(&mut App)) -> Sim {
    let mut app = pieced::app::headless_app(seed);
    app.insert_resource(GameMode::Waves);
    setup(&mut app);
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Playing);
    app.update();
    Sim { app }
}

/// A Waves sim whose player can't die (a billion health, the normal shield)
/// and whose knights stand still where they land.
fn frozen(seed: u64) -> Sim {
    let mut sim = Sim::waves(seed);
    sim.world_mut().insert_resource(GalleryFreeze);
    toughen(&mut sim);
    sim
}

fn toughen(sim: &mut Sim) {
    let p = sim.player();
    let mut health = Health::full(1.0e9, 100.0);
    health.shield = 100.0;
    *sim.world_mut().get_mut::<Health>(p).unwrap() = health;
}

/// A fresh temporary directory for a run store.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pieced-waves-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn count<C: Component>(sim: &mut Sim) -> usize {
    sim.world_mut()
        .query_filtered::<Entity, With<C>>()
        .iter(sim.world())
        .count()
}

/// Puts an orb in flight on an idle pool slot (far below the island, where
/// it hits nothing).
fn launch_orb(sim: &mut Sim, shooter: Entity, velocity: Vec3) {
    let slot = sim
        .world_mut()
        .query_filtered::<Entity, (With<OrbSlot>, Without<Orb>)>()
        .iter(sim.world())
        .next()
        .expect("an idle orb slot");
    sim.world_mut().entity_mut(slot).insert(Orb {
        shooter,
        velocity,
        travelled: 0.0,
        previous: PARKED,
        launched: 0,
    });
}

fn run(sim: &Sim) -> Run {
    sim.world().resource::<Run>().clone()
}

fn summary(sim: &Sim) -> RunSummary {
    sim.world().resource::<RunSummary>().clone()
}

fn tuning(sim: &Sim) -> Tuning {
    sim.world().resource::<Tuning>().clone()
}

fn pool(sim: &mut Sim) -> Vec<Entity> {
    let mut q = sim
        .world_mut()
        .query_filtered::<Entity, (With<PoolGrunt>, With<Grunt>)>();
    let mut v: Vec<Entity> = q.iter(sim.world()).collect();
    v.sort();
    v
}

/// Grunts in play: poofed in and not downed.
fn in_play(sim: &mut Sim) -> Vec<Entity> {
    let mut q = sim
        .world_mut()
        .query_filtered::<Entity, (With<PoolGrunt>, Without<Parked>, Without<Downed>)>();
    let mut v: Vec<Entity> = q.iter(sim.world()).collect();
    v.sort();
    v
}

fn is_parked(sim: &Sim, e: Entity) -> bool {
    sim.world().get::<Parked>(e).is_some()
        && sim.world().get::<Downed>(e).is_some()
        && sim.feet(e).distance(PARK_SPOT) < 1e-3
}

/// Ticks until the whole wave (or the first eight of it) is in play,
/// returning the tick each grunt poofed in at.
fn run_until_wave_in(sim: &mut Sim) -> Vec<u64> {
    let t = tuning(sim).waves;
    let size = t.wave_size(run(sim).wave).min(t.max_alive) as usize;
    let mut arrivals = Vec::new();
    let mut seen = 0;
    for _ in 0..600 {
        sim.tick();
        let now = in_play(sim).len();
        for _ in seen..now {
            arrivals.push(sim.sim_tick());
        }
        seen = now;
        if seen == size {
            return arrivals;
        }
    }
    panic!("wave never finished poofing in ({seen} of {size})");
}

/// Downs `grunt` the way combat does: dead, `Downed`, and a killing
/// [`DamageDealt`] from the player (a headshot if `headshot`).
fn kill(sim: &mut Sim, grunt: Entity, headshot: bool) {
    let player = sim.player();
    let tick = sim.sim_tick();
    let at = sim.feet(grunt);
    let hp = sim.get::<Health>(grunt).hp;
    sim.world_mut().get_mut::<Health>(grunt).unwrap().hp = 0.0;
    sim.world_mut().entity_mut(grunt).insert(Downed { tick });
    sim.world_mut().write_message(DamageDealt {
        source: Some(player),
        target: grunt,
        target_kind: DamageTarget::Character,
        amount: hp,
        to_shield: 0.0,
        headshot,
        shield_broke: false,
        killed: true,
        point: at + Vec3::Y,
        normal: Vec3::Z,
        tick,
    });
}

/// Downs every knight of the current wave as it arrives, until the break,
/// checking the 8-alive cap every tick. Returns the knights downed.
fn clear_wave(sim: &mut Sim) -> Vec<Entity> {
    let wave = run(sim).wave;
    let cap = tuning(sim).waves.max_alive as usize;
    let mut downed = Vec::new();
    for _ in 0..20_000 {
        let alive = in_play(sim);
        assert!(alive.len() <= cap, "{} in play (cap {cap})", alive.len());
        for g in alive {
            kill(sim, g, false);
            downed.push(g);
        }
        sim.tick();
        let r = run(sim);
        if matches!(r.phase, RunPhase::Break { .. }) {
            assert_eq!(r.wave, wave, "the break follows wave {wave}");
            return downed;
        }
    }
    panic!("wave {wave} never cleared");
}

fn skip_break(sim: &mut Sim) {
    assert!(matches!(run(sim).phase, RunPhase::Break { .. }));
    sim.world_mut().write_message(SkipBreak);
    sim.tick();
    assert_eq!(
        run(sim).phase,
        RunPhase::Fighting,
        "Enter skipped the break"
    );
}

fn eliminate_player(sim: &mut Sim) {
    let player = sim.player();
    sim.world_mut()
        .get_mut::<Health>(player)
        .unwrap()
        .apply(1.0e12);
    sim.tick();
    assert!(run(sim).is_ended(), "one life: the run is over");
}

/// The wave's spawn points and speeds, in poof-in order.
fn wave_signature(sim: &mut Sim) -> Vec<(Vec3, f32)> {
    let size = tuning(sim).waves.wave_size(run(sim).wave).min(8) as usize;
    let mut order: Vec<Entity> = Vec::new();
    for _ in 0..600 {
        sim.tick();
        for e in in_play(sim) {
            if !order.contains(&e) {
                order.push(e);
            }
        }
        if order.len() == size {
            break;
        }
    }
    order
        .into_iter()
        .map(|e| (sim.feet(e), sim.get::<GruntStats>(e).speed))
        .collect()
}

fn place(sim: &mut Sim, who: Entity, feet: Vec3) {
    sim.world_mut()
        .get_mut::<Transform>(who)
        .unwrap()
        .translation = feet;
    sim.world_mut().get_mut::<PreviousFeet>(who).unwrap().0 = feet;
}

fn aim_at(sim: &mut Sim, point: Vec3) {
    let player = sim.player();
    let eye = sim.feet(player) + Vec3::Y * sim.get::<EyeHeight>(player).0;
    let look = look_toward(point - eye);
    sim.set_look(player, look.yaw, look.pitch);
}

/// Walks up to `grunt` (2 m inward of it) and pumps it at point blank until it
/// goes down: the spec's "one close pump".
fn pump_down(sim: &mut Sim, grunt: Entity) -> u32 {
    let player = sim.player();
    sim.player_intent().select = Some(ActiveTool::Weapon(WeaponKind::Pump));
    for shot in 1..=3 {
        let at = sim.feet(grunt);
        let inward = (-at).with_y(0.0).normalize_or(Vec3::Z);
        place(sim, player, at + inward * 2.0);
        sim.run_seconds(1.0);
        let at = sim.feet(grunt);
        aim_at(sim, at + Vec3::Y * 1.0);
        sim.player_intent().fire_pressed = true;
        sim.tick();
        if sim.world().get::<Downed>(grunt).is_some() {
            return shot;
        }
    }
    panic!("three point-blank pumps didn't down the grunt");
}

fn read_lines(path: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).expect("each line is JSON"))
        .collect()
}

// ---------------------------------------------------------------------------
// The pool and the first wave (chunk 1)
// ---------------------------------------------------------------------------

#[test]
fn waves_has_no_dummy_and_practice_keeps_it() {
    let mut waves = Sim::waves(1);
    assert_eq!(count::<Dummy>(&mut waves), 0);
    assert!(waves.world().get_resource::<Run>().is_some());
    assert!(waves.world().get_resource::<RunSummary>().is_some());

    let mut practice = Sim::new();
    assert_eq!(count::<Dummy>(&mut practice), 1);
    assert!(practice.world().get_resource::<Run>().is_none());
    assert!(practice.world().get_resource::<RunSummary>().is_none());
    assert_eq!(count::<PoolGrunt>(&mut practice), 0);
    assert_eq!(count::<PotionSlot>(&mut practice), 0);
    practice.run_seconds(3.0);
    assert_eq!(count::<Grunt>(&mut practice), 0, "no grunts in Practice");
}

#[test]
fn the_pool_starts_with_eight_parked_grunts_that_stay_put() {
    let mut sim = Sim::waves(1);
    let pool = pool(&mut sim);
    assert_eq!(pool.len(), 8, "max_alive pool members");
    // Before the first spawn, every one stays parked under the island: not
    // dropped to the kill plane, not respawned, not in play.
    sim.run_seconds(FIRST_SPAWN_DELAY * 0.8);
    for &g in &pool {
        assert!(is_parked(&sim, g), "{g} is parked");
        assert!(sim.world().get::<Character>(g).is_some());
    }
    assert!(in_play(&mut sim).is_empty());
    let r = run(&sim);
    assert_eq!((r.wave, r.remaining, r.alive), (1, 3, 0));
    assert_eq!(r.phase, RunPhase::Fighting);
    let s = summary(&sim);
    assert_eq!((s.wave, s.knights_left, s.score), (1, 3, 0));
    assert_eq!(s.seed, r.seed);
}

#[test]
fn wave_one_poofs_in_three_grunts_at_the_edge_away_from_the_player() {
    let mut sim = Sim::waves(3);
    // Brains frozen, so the knights stand where they landed.
    sim.world_mut().insert_resource(GalleryFreeze);
    sim.record::<GameCue>();
    let arrivals = run_until_wave_in(&mut sim);
    assert_eq!(arrivals.len(), 3);
    let stagger = (SPAWN_STAGGER * 60.0).round() as u64;
    for pair in arrivals.windows(2) {
        assert_eq!(pair[1] - pair[0], stagger, "staggered {SPAWN_STAGGER} s");
    }
    let player = sim.player();
    let player_feet = sim.feet(player);
    let layout = sim.world().resource::<ArenaLayout>().clone();
    let tuning = tuning(&sim);
    let wave1 = GruntStats::for_wave(1, &tuning.grunt);
    let grunts = in_play(&mut sim);
    let cues = sim.recorded::<GameCue>();
    let mut speeds = Vec::new();
    for &g in &grunts {
        let feet = sim.feet(g);
        let edge = (layout.bounds_max.x - feet.x.abs()).min(layout.bounds_max.y - feet.z.abs());
        assert!(
            (EDGE_INSET - 0.5..=EDGE_INSET + 0.5).contains(&edge),
            "{feet} is {edge:.2} m inside the edge"
        );
        let from_player = feet.xz().distance(player_feet.xz());
        assert!(
            from_player >= SPAWN_MIN_PLAYER_DISTANCE,
            "{feet} is {from_player:.1} m from the player"
        );
        assert_eq!(*sim.get::<Health>(g), Health::full(wave1.hp, 0.0));
        let stats = *sim.get::<GruntStats>(g);
        assert_eq!(stats.hp, wave1.hp);
        let jitter = tuning.waves.speed_jitter * wave1.speed;
        assert!(
            (stats.speed - wave1.speed).abs() <= jitter + 1e-4,
            "speed {} within ±{:.0}%",
            stats.speed,
            tuning.waves.speed_jitter * 100.0
        );
        speeds.push(stats.speed);
        assert!(
            cues.contains(&GameCue::Respawned { who: g }),
            "{g} poofed in with the respawn cue"
        );
        assert_eq!(sim.get::<PoolGrunt>(g).wave, 1);
    }
    assert!(
        speeds.windows(2).any(|w| w[0] != w[1]),
        "speeds are jittered per knight: {speeds:?}"
    );
    let r = run(&sim);
    assert_eq!((r.remaining, r.alive), (0, 3));
    assert_eq!(summary(&sim).knights_left, 3);
    assert_eq!(
        pool(&mut sim).len() - grunts.len(),
        5,
        "the rest stay parked"
    );
}

#[test]
fn pumping_down_wave_one_scores_it_and_starts_the_break() {
    let mut sim = Sim::waves(5);
    toughen(&mut sim);
    run_until_wave_in(&mut sim);
    let first = in_play(&mut sim);
    for &g in &first {
        pump_down(&mut sim, g);
    }
    sim.tick();
    let r = run(&sim);
    assert_eq!(r.eliminations, 3);
    assert!(
        matches!(r.phase, RunPhase::Break { .. }),
        "wave cleared: {:?}",
        r.phase
    );
    assert_eq!(r.waves_cleared, 1);
    let t = tuning(&sim).waves;
    assert_eq!(
        r.score,
        3 * t.score_kill + t.score_wave,
        "3 × 100 + 250 × 1"
    );
    // Downed grunts go back to the pool during the break.
    sim.run_seconds(2.0);
    for &g in &first {
        assert!(is_parked(&sim, g), "{g} returned to the pool");
    }
    assert_eq!(pool(&mut sim).len(), 8, "the pool never grows");
}

// ---------------------------------------------------------------------------
// Endless waves (D68, D77)
// ---------------------------------------------------------------------------

#[test]
fn waves_grow_by_two_with_never_more_than_eight_in_play() {
    let mut sim = frozen(2);
    sim.record::<GameCue>();
    let pool = pool(&mut sim);
    let mut sizes = Vec::new();
    for wave in 1..=5u32 {
        let t = tuning(&sim).waves;
        let size = t.wave_size(wave);
        let r = run(&sim);
        assert_eq!((r.wave, r.left()), (wave, size), "wave {wave} starts whole");
        // Left alone, the wave fills the island up to the cap and waits.
        sim.run_seconds(FIRST_SPAWN_DELAY + SPAWN_STAGGER * 12.0);
        let first = size.min(t.max_alive);
        assert_eq!(in_play(&mut sim).len() as u32, first, "wave {wave}");
        assert_eq!(run(&sim).remaining, size - first);
        assert_eq!(summary(&sim).knights_left, size);
        // Downed knights make room for the rest.
        sim.clear_recorded::<GameCue>();
        let downed = clear_wave(&mut sim);
        let later = sim
            .recorded::<GameCue>()
            .iter()
            .filter(|c| matches!(c, GameCue::Respawned { who } if pool.contains(who)))
            .count() as u32;
        assert_eq!(downed.len() as u32, size, "every knight of wave {wave}");
        assert_eq!(later, size - first, "the rest arrived as slots freed");
        sizes.push(size);
        skip_break(&mut sim);
    }
    assert_eq!(sizes, [3, 5, 7, 9, 11]);
}

#[test]
fn clearing_a_wave_starts_a_ten_second_break_with_fifty_shield() {
    let mut sim = frozen(6);
    let player = sim.player();
    sim.world_mut().get_mut::<Health>(player).unwrap().shield = 20.0;
    let hp = sim.get::<Health>(player).hp;
    clear_wave(&mut sim);
    let health = *sim.get::<Health>(player);
    assert_eq!(health.shield, 70.0, "+50 shield at the break's start");
    assert_eq!(health.hp, hp, "health only comes back from potions");
    let s = summary(&sim);
    assert_eq!((s.wave, s.knights_left), (1, 0));
    let left = s.break_seconds_left.expect("the countdown shows");
    assert!((left - 10.0).abs() < 0.05, "{left} s left");

    // The break runs its 10 s, then wave 2 comes.
    let mut ticks = 0;
    while matches!(run(&sim).phase, RunPhase::Break { .. }) {
        sim.tick();
        ticks += 1;
        assert!(ticks <= 700, "the break never ended");
        if ticks == 300 {
            let left = summary(&sim).break_seconds_left.unwrap();
            assert!((left - 5.0).abs() < 0.05, "{left} s left halfway");
        }
    }
    assert!(
        (599..=601).contains(&ticks),
        "the break lasted {ticks} ticks"
    );
    let r = run(&sim);
    assert_eq!((r.wave, r.phase, r.left()), (2, RunPhase::Fighting, 5));
    assert_eq!(summary(&sim).break_seconds_left, None);

    // A nearly full shield caps at its max.
    sim.world_mut().get_mut::<Health>(player).unwrap().shield = 80.0;
    clear_wave(&mut sim);
    assert_eq!(sim.get::<Health>(player).shield, 100.0, "capped");
}

#[test]
fn enter_skips_the_break_but_only_during_one() {
    let mut sim = frozen(7);
    run_until_wave_in(&mut sim);
    // An early press (mid-fight) is ignored and doesn't carry over.
    sim.world_mut().write_message(SkipBreak);
    sim.tick();
    assert_eq!(run(&sim).phase, RunPhase::Fighting);
    clear_wave(&mut sim);
    sim.run_seconds(1.0);
    assert!(
        matches!(run(&sim).phase, RunPhase::Break { .. }),
        "the early press didn't skip this break"
    );
    skip_break(&mut sim);
    let r = run(&sim);
    assert_eq!((r.wave, r.remaining), (2, 5));
}

#[test]
fn the_next_waves_knights_have_that_waves_stats() {
    let mut sim = frozen(8);
    clear_wave(&mut sim);
    skip_break(&mut sim);
    run_until_wave_in(&mut sim);
    let t = tuning(&sim);
    let wave2 = GruntStats::for_wave(2, &t.grunt);
    assert!(wave2.hp > GruntStats::for_wave(1, &t.grunt).hp);
    let grunts = in_play(&mut sim);
    assert_eq!(grunts.len(), 5);
    for g in grunts {
        let stats = *sim.get::<GruntStats>(g);
        assert_eq!(stats.hp, wave2.hp);
        assert_eq!(stats.fire_interval, wave2.fire_interval);
        assert_eq!(stats.reaction, wave2.reaction);
        assert_eq!(*sim.get::<Health>(g), Health::full(wave2.hp, 0.0));
        let jitter = t.waves.speed_jitter * wave2.speed;
        assert!((stats.speed - wave2.speed).abs() <= jitter + 1e-4);
        assert_eq!(sim.get::<PoolGrunt>(g).wave, 2);
    }
}

// ---------------------------------------------------------------------------
// Score (D81)
// ---------------------------------------------------------------------------

#[test]
fn score_counts_kills_headshots_void_knockoffs_and_wave_clears() {
    let mut sim = frozen(9);
    let t = tuning(&sim).waves;
    assert_eq!(
        (t.score_kill, t.score_headshot, t.score_void, t.score_wave),
        (100, 50, 150, 250)
    );

    // Wave 1: one headshot kill, two body kills, then the clear.
    run_until_wave_in(&mut sim);
    let w1 = in_play(&mut sim);
    kill(&mut sim, w1[0], true);
    sim.tick();
    assert_eq!(run(&sim).score, 150, "a headshot kill");
    kill(&mut sim, w1[1], false);
    sim.tick();
    assert_eq!(run(&sim).score, 250, "a body kill");
    kill(&mut sim, w1[2], false);
    sim.tick();
    assert_eq!(run(&sim).score, 350 + 250, "the last kill and the clear");
    skip_break(&mut sim);

    // Wave 2: a void knock-off signalled with its down, another signalled a
    // tick after, three body kills, and the clear (+250 × 2).
    run_until_wave_in(&mut sim);
    let w2 = in_play(&mut sim);
    kill(&mut sim, w2[0], false);
    sim.world_mut().write_message(VoidKill { knight: w2[0] });
    sim.tick();
    assert_eq!(run(&sim).score, 600 + 250);
    kill(&mut sim, w2[1], false);
    sim.tick();
    sim.world_mut().write_message(VoidKill { knight: w2[1] });
    sim.tick();
    assert_eq!(run(&sim).score, 850 + 250);
    for &g in &w2[2..] {
        kill(&mut sim, g, false);
    }
    sim.tick();
    let r = run(&sim);
    assert_eq!(r.score, 1100 + 300 + 500);
    assert_eq!((r.eliminations, r.headshot_kills, r.void_kills), (8, 1, 2));
    let s = summary(&sim);
    assert_eq!((s.score, s.results.eliminations), (1900, 8));
}

#[test]
fn a_rifle_headshot_kill_earns_the_bonus() {
    let mut sim = frozen(10);
    run_until_wave_in(&mut sim);
    let player = sim.player();
    let spawn = sim.world().resource::<ArenaLayout>().player_spawn;
    place(&mut sim, player, spawn);
    let target = in_play(&mut sim)[0];
    // Stand the knight 8 m up the (clear) spawn line.
    let feet = spawn + Vec3::NEG_Z * 8.0;
    place(&mut sim, target, feet);
    sim.run_seconds(0.2);
    let mut shots = 0;
    while sim.world().get::<Downed>(target).is_none() {
        aim_at(&mut sim, feet + Vec3::Y * HEAD_CENTER);
        sim.player_intent().fire = true;
        sim.player_intent().fire_pressed = true;
        sim.tick();
        sim.player_intent().fire = false;
        sim.run_seconds(1.0);
        shots += 1;
        assert!(shots <= 6, "the rifle never downed the knight");
    }
    sim.tick();
    let stats = sim.world().resource::<CombatStats>().clone();
    assert!(stats.headshots >= 1);
    let r = run(&sim);
    assert_eq!(r.headshot_kills, 1, "the killing hit was a headshot");
    assert_eq!(r.score, 150);
    assert_eq!(summary(&sim).results.headshots, stats.headshots);
}

// ---------------------------------------------------------------------------
// The personal best and the run log (D81, D89)
// ---------------------------------------------------------------------------

#[test]
fn the_best_run_is_saved_loaded_and_beaten_by_wave_then_score() {
    let dir = temp_dir("best");
    let store = RunStore::at(&dir);
    let best_path = store.best_path().unwrap();
    let mut sim = start(4, |app| {
        app.insert_resource(store.clone());
    });
    sim.world_mut().insert_resource(GalleryFreeze);
    assert_eq!(sim.world().resource::<PersonalBest>().0, None);
    assert_eq!(summary(&sim).best, None);

    // Run 1: clear wave 1 (550), die in wave 2: the first best.
    clear_wave(&mut sim);
    skip_break(&mut sim);
    eliminate_player(&mut sim);
    let s = summary(&sim);
    assert!(s.new_best);
    let best = s.best.clone().expect("a best run");
    assert_eq!((best.wave, best.score, best.seed), (2, 550, s.seed));
    assert_eq!(store.load_best().as_ref(), Some(&best), "saved");

    // Run 2: die in wave 1 with nothing: not a best, the file untouched.
    let saved = std::fs::read_to_string(&best_path).unwrap();
    sim.world_mut().write_message(RestartRun);
    sim.tick();
    assert!(!summary(&sim).new_best);
    assert_eq!(summary(&sim).best.as_ref(), Some(&best), "the best stays");
    eliminate_player(&mut sim);
    assert!(!summary(&sim).new_best);
    assert_eq!(std::fs::read_to_string(&best_path).unwrap(), saved);

    // Run 3: wave 2 again with a higher score (a headshot kill): the tiebreak.
    sim.world_mut().write_message(RestartRun);
    sim.tick();
    run_until_wave_in(&mut sim);
    let w1 = in_play(&mut sim);
    kill(&mut sim, w1[0], true);
    clear_wave(&mut sim);
    skip_break(&mut sim);
    eliminate_player(&mut sim);
    let s = summary(&sim);
    assert!(s.new_best, "same wave, higher score");
    assert_eq!(
        (
            s.best.as_ref().unwrap().wave,
            s.best.as_ref().unwrap().score
        ),
        (2, 600)
    );

    // A new game loads it.
    let loaded = start(5, |app| {
        app.insert_resource(store.clone());
    });
    assert_eq!(loaded.world().resource::<PersonalBest>().0, s.best);
    assert_eq!(summary(&loaded).best, s.best);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn each_run_appends_a_line_to_the_run_log() {
    let dir = temp_dir("runs");
    let store = RunStore::at(&dir);
    let mut sim = start(12, |app| {
        app.insert_resource(store.clone());
    });
    let player = sim.player();
    let spawn = sim.world().resource::<ArenaLayout>().player_spawn;
    // Stand in the open and let the knights win (real orbs), after one rifle
    // shot at the sky for the accuracy.
    sim.player_intent().select = Some(ActiveTool::Weapon(WeaponKind::Rifle));
    sim.run_seconds(0.5);
    sim.set_look(player, 0.0, 1.2);
    sim.player_intent().fire_pressed = true;
    sim.player_intent().fire = true;
    sim.tick();
    sim.player_intent().fire = false;
    let mut killer = None;
    for _ in 0..(150 * 60) {
        place(&mut sim, player, spawn);
        sim.tick();
        if run(&sim).is_ended() {
            killer = Some(());
            break;
        }
    }
    assert!(killer.is_some(), "the knights never got the player");
    let seed = run(&sim).seed;
    let lines = read_lines(&store.runs_path().unwrap());
    assert_eq!(lines.len(), 1, "one line per run");
    let line = &lines[0];
    for field in [
        "seed",
        "commit",
        "wave",
        "score",
        "run_seconds",
        "eliminations",
        "accuracy",
        "headshots",
        "ended",
        "cause",
        "unix_time",
    ] {
        assert!(line.get(field).is_some(), "{field} in {line}");
    }
    assert_eq!(line["seed"].as_u64(), Some(seed));
    assert!(!line["commit"].as_str().unwrap().is_empty());
    assert_eq!(line["wave"].as_u64(), Some(1));
    assert_eq!(line["score"].as_u64(), Some(0));
    assert_eq!(line["eliminations"].as_u64(), Some(0));
    assert_eq!(line["accuracy"].as_f64(), Some(0.0), "one shot, no hit");
    assert_eq!(line["headshots"].as_u64(), Some(0));
    assert_eq!(line["ended"].as_str(), Some("eliminated"));
    let seconds = line["run_seconds"].as_f64().unwrap();
    assert!(seconds > 1.0, "{seconds} s");
    assert!((seconds as f32 - summary(&sim).results.run_seconds).abs() < 1e-3);
    // The cause: an orb from a wave-1 knight on the island, some time after
    // the previous hit.
    let cause = &line["cause"];
    assert_eq!(cause["source_wave"].as_u64(), Some(1), "{cause}");
    let at: Vec<f64> = cause["source_position"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    assert!(at.len() == 3 && at[0].abs() < 30.0 && at[2].abs() < 30.0 && at[1] > -2.0);
    let since = cause["seconds_since_previous_hit"].as_f64().unwrap();
    assert!((0.0..60.0).contains(&since), "{since}");

    // Quitting from the pause menu ends the next run straight to the results,
    // with no cause of death.
    sim.world_mut().write_message(RestartRun);
    sim.tick();
    sim.run_seconds(1.0);
    sim.world_mut().write_message(EndRun);
    sim.tick();
    assert!(run(&sim).is_over(), "straight to the results");
    assert_eq!(run(&sim).ended, Some(RunEnd::Quit));
    let lines = read_lines(&store.runs_path().unwrap());
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1]["ended"].as_str(), Some("quit"));
    assert!(lines[1]["cause"].is_null());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_default_store_writes_nothing() {
    let sim = Sim::waves(1);
    assert_eq!(sim.world().resource::<RunStore>().dir, None);
}

// ---------------------------------------------------------------------------
// Death, the beat, and going again (D84)
// ---------------------------------------------------------------------------

#[test]
fn death_plays_the_beat_then_the_results() {
    let mut sim = Sim::waves(9);
    let player = sim.player();
    run_until_wave_in(&mut sim);
    let survivors = in_play(&mut sim);
    sim.world_mut()
        .get_mut::<Health>(player)
        .unwrap()
        .apply(1.0e12);
    sim.tick();
    let died = sim.sim_tick();
    let r = run(&sim);
    let RunPhase::Dying { until } = r.phase else {
        panic!("the death beat: {:?}", r.phase)
    };
    let t = tuning(&sim).waves;
    let beat = (t.death_seconds * t.death_time_scale * 60.0).round() as u64;
    assert!(
        until.abs_diff(died) <= beat + 1 && until > died,
        "{beat} ticks of slow motion"
    );
    assert!(sim.world().get::<Downed>(player).is_some());
    assert!(r.is_ended() && !r.is_over());
    assert_eq!(summary(&sim).phase, r.phase);

    // The survivors hop while the player lies there.
    let mut highest: f32 = 0.0;
    for _ in 0..(VICTORY_HOP_SECONDS * 60.0) as u32 {
        sim.tick();
        for &g in &survivors {
            highest = highest.max(sim.feet(g).y);
        }
    }
    assert!(highest > 0.3, "a victory hop ({highest:.2} m)");
    assert!(run(&sim).is_over(), "then the results");
    assert!(matches!(summary(&sim).phase, RunPhase::Over { .. }));

    // The controls do nothing now.
    sim.record::<ShotFired>();
    let at = sim.feet(player);
    for _ in 0..30 {
        {
            let mut i = sim.player_intent();
            i.move_axis = Vec2::Y;
            i.fire = true;
            i.fire_pressed = true;
            i.jump_pressed = true;
        }
        sim.tick();
    }
    assert!(
        sim.feet(player).distance(at) < 0.05,
        "the dead player can't move"
    );
    assert!(sim.recorded::<ShotFired>().is_empty(), "or shoot");
    assert_eq!(*sim.get::<PlayerIntent>(player), PlayerIntent::default());

    // After the hop the knights stand still.
    sim.run_seconds(1.0);
    for g in survivors {
        assert!(sim.world().get::<Parked>(g).is_some(), "{g} stopped");
    }
}

#[test]
fn going_again_restarts_everything_in_place_with_a_new_seed() {
    let mut sim = Sim::waves(9);
    sim.tuning_mut().waves.potion_chance = 1.0;
    let player = sim.player();
    let spawn = sim.world().resource::<ArenaLayout>().player_spawn;
    let cover = count::<Piece>(&mut sim);
    assert!(cover > 0, "Waves starts with the initial cover");
    run_until_wave_in(&mut sim);
    let seed = run(&sim).seed;

    // Mess the arena up: a kill (score, a potion), a built wall, an orb in
    // flight, a shot fired, the player moved.
    let first = in_play(&mut sim)[0];
    kill(&mut sim, first, true);
    sim.tick();
    assert_eq!(run(&sim).score, 150);
    let live = |sim: &mut Sim| {
        sim.world_mut()
            .query::<&PotionSlot>()
            .iter(sim.world())
            .filter(|p| p.live.is_some())
            .count()
    };
    assert_eq!(live(&mut sim), 1, "a potion dropped");
    building::place_piece(
        sim.world_mut(),
        PieceSlot::wall(GridCell::new(1, 1, 0), Facing::North),
    )
    .expect("wall placed");
    launch_orb(&mut sim, player, Vec3::X * 30.0);
    sim.player_intent().fire_pressed = true;
    place(&mut sim, player, Vec3::new(-4.0, 0.0, 0.0));
    sim.run_seconds(0.2);
    eliminate_player(&mut sim);
    sim.run_seconds(1.5);

    // Enter: go again.
    sim.world_mut().write_message(RestartRun);
    sim.tick();
    let r = run(&sim);
    assert_eq!(r.phase, RunPhase::Fighting);
    assert_eq!((r.wave, r.eliminations, r.remaining, r.score), (1, 0, 3, 0));
    assert_eq!((r.ended_tick, r.ended, r.new_best), (None, None, false));
    assert_ne!(r.seed, seed, "a new seed");
    let s = summary(&sim);
    assert_eq!((s.wave, s.knights_left, s.score, s.seed), (1, 3, 0, r.seed));
    assert_eq!(s.results.eliminations, 0);
    assert_eq!(sim.world().resource::<CombatStats>().shots, 0);
    assert_eq!(live(&mut sim), 0, "potions cleared");
    assert_eq!(count::<Piece>(&mut sim), cover, "back to the initial cover");
    assert!(
        building::place_piece(
            sim.world_mut(),
            PieceSlot::wall(GridCell::new(1, 1, 0), Facing::North),
        )
        .is_ok(),
        "the built wall is gone"
    );
    assert_eq!(count::<Orb>(&mut sim), 0, "orbs cleared");
    for g in pool(&mut sim) {
        assert!(is_parked(&sim, g), "{g} parked");
    }
    let t = tuning(&sim);
    assert_eq!(
        *sim.get::<Health>(player),
        Health::full(t.combat.max_hp, t.combat.max_shield)
    );
    assert!(sim.world().get::<Downed>(player).is_none());
    assert!(sim.feet(player).xz().distance(spawn.xz()) < 0.05);

    // The player is back in control, and the new wave poofs in.
    sim.player_intent().move_axis = Vec2::Y;
    sim.run_seconds(0.5);
    assert!(
        sim.feet(player).distance(spawn) > 1.0,
        "the player moves again"
    );
    sim.player_intent().move_axis = Vec2::ZERO;
    assert_eq!(run_until_wave_in(&mut sim).len(), 3);
}

// ---------------------------------------------------------------------------
// The seed (D83)
// ---------------------------------------------------------------------------

/// Waves 1 and 2 of a run with brains frozen: where and how fast each knight
/// landed.
fn two_wave_signature(sim_seed: u64, run_seed: Option<u64>) -> (u64, Vec<(Vec3, f32)>) {
    let mut sim = start(sim_seed, |app| {
        if let Some(seed) = run_seed {
            app.insert_resource(RunSeed(seed));
        }
    });
    sim.world_mut().insert_resource(GalleryFreeze);
    let mut signature = wave_signature(&mut sim);
    for g in in_play(&mut sim) {
        kill(&mut sim, g, false);
    }
    sim.tick();
    skip_break(&mut sim);
    signature.extend(wave_signature(&mut sim));
    assert_eq!(signature.len(), 3 + 5);
    (run(&sim).seed, signature)
}

#[test]
fn the_same_seed_gives_the_same_waves() {
    let (seed, a) = two_wave_signature(11, None);
    assert_eq!((seed, a.clone()), two_wave_signature(11, None));
    assert_ne!(a, two_wave_signature(12, None).1);
}

#[test]
fn a_fixed_seed_replays_the_run() {
    let args: Vec<String> = ["pieced", "--seed", "4242"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(GameOptions::from_args(&args).seed, Some(4242));
    assert_eq!(GameOptions::from_args(&args[..1]).seed, None);

    let (seed, a) = two_wave_signature(1, Some(4242));
    assert_eq!(seed, 4242, "the run uses the given seed");
    let (_, b) = two_wave_signature(2, Some(4242));
    assert_eq!(a, b, "same seed, same knights, whatever else differs");
    assert_ne!(a, two_wave_signature(1, Some(4243)).1);
}

// ---------------------------------------------------------------------------
// Leaks
// ---------------------------------------------------------------------------

fn entities(sim: &mut Sim) -> usize {
    sim.world_mut()
        .query_filtered::<Entity, Without<Piece>>()
        .iter(sim.world())
        .count()
}

#[test]
fn restarting_ten_times_leaks_nothing() {
    let mut sim = Sim::waves(21);
    sim.tuning_mut().waves.potion_chance = 1.0;
    let player = sim.player();
    let mut baseline = None;
    for cycle in 0..10 {
        run_until_wave_in(&mut sim);
        let now = entities(&mut sim);
        match baseline {
            None => baseline = Some(now),
            Some(b) => assert_eq!(now, b, "entities after restart {cycle}"),
        }
        let x = cycle % 4;
        building::place_piece(
            sim.world_mut(),
            PieceSlot::floor(GridCell::new(1 + x, 1, 0)),
        )
        .expect("floor placed");
        launch_orb(&mut sim, player, Vec3::X);
        let g = in_play(&mut sim)[0];
        kill(&mut sim, g, false);
        sim.tick();
        eliminate_player(&mut sim);
        sim.run_seconds(0.5);
        assert!(run(&sim).is_over());
        sim.world_mut().write_message(RestartRun);
        sim.tick();
    }
}

/// Five simulated minutes of endless waves (brains frozen to keep the suite
/// quick; the orb and grunt suites soak those): the player can't die; every
/// knight is downed 2 s after it lands (every third by a headshot) and every
/// break is skipped. Nothing leaks, the cap holds, the
/// score adds up, and about one knight in ten drops a potion.
#[test]
fn five_minutes_of_waves_leak_nothing_and_add_up() {
    let mut sim = frozen(31);
    sim.record::<GameCue>();
    let t = tuning(&sim).waves;
    let baseline = entities(&mut sim);
    let mut landed: Vec<(Entity, u64)> = Vec::new();
    let mut kills = 0u32;
    let mut headshots = 0u32;
    let mut breaks = 0u32;
    for _ in 0..(5 * 60 * 60) {
        let now = sim.sim_tick();
        let alive = in_play(&mut sim);
        assert!(alive.len() as u32 <= t.max_alive);
        landed.retain(|(e, _)| alive.contains(e));
        for &g in &alive {
            if !landed.iter().any(|(e, _)| *e == g) {
                landed.push((g, now));
            }
        }
        let due: Vec<Entity> = landed
            .iter()
            .filter(|(_, at)| now >= at + 120)
            .map(|(e, _)| *e)
            .collect();
        for g in due {
            kills += 1;
            let headshot = kills.is_multiple_of(3);
            headshots += u32::from(headshot);
            kill(&mut sim, g, headshot);
        }
        if matches!(run(&sim).phase, RunPhase::Break { .. }) {
            breaks += 1;
            sim.world_mut().write_message(SkipBreak);
        }
        sim.tick();
    }
    let r = run(&sim);
    assert!(!r.is_ended(), "the player survived");
    assert!(r.wave >= 8, "reached wave {}", r.wave);
    assert_eq!(breaks, r.waves_cleared);
    assert_eq!(r.eliminations, kills, "every down counted once");
    let cleared: u32 = (1..=r.waves_cleared).sum();
    assert_eq!(
        r.score,
        r.eliminations * t.score_kill
            + r.headshot_kills * t.score_headshot
            + cleared * t.score_wave
    );
    assert_eq!(r.headshot_kills, headshots);
    let drops = sim
        .recorded::<GameCue>()
        .iter()
        .filter(|c| matches!(c, GameCue::PotionDropped { .. }))
        .count() as f32;
    let rate = drops / r.eliminations as f32;
    assert!(
        (0.05..=0.16).contains(&rate),
        "{drops} potions from {} knights",
        r.eliminations
    );
    assert_eq!(entities(&mut sim), baseline, "no entity leaked");
    assert_eq!(pool(&mut sim).len(), 8);
    assert_eq!(count::<PotionSlot>(&mut sim), pieced::waves::POTION_POOL);
}
