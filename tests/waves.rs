//! The Waves run, chunk 1 (docs/M3-SPEC.md → Waves → Chunk 1 placeholder)
//! through the simulation seam: the grunt pool, wave 1 poofing in at seeded
//! edge points, clearing the wave, the player's death, and restarting in place.

use bevy::prelude::*;
use pieced::{
    arena::ArenaLayout,
    building::{self, Piece, PieceSlot},
    combat::Downed,
    dummy::{Dummy, look_toward},
    grunt::{Grunt, GruntStats, Parked},
    orb::{Orb, OrbSlot, PARKED},
    shared::{
        ActiveTool, Character, EyeHeight, Facing, GameCue, GridCell, Health, PlayerIntent,
        ShotFired, WeaponKind,
    },
    sim::Sim,
    tuning::Tuning,
    waves::{
        EDGE_INSET, FIRST_SPAWN_DELAY, PARK_SPOT, PoolGrunt, RestartRun, Run, RunPhase,
        SPAWN_MIN_PLAYER_DISTANCE, SPAWN_STAGGER,
    },
};

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

/// Ticks until the whole of wave 1 is in play, returning the tick each grunt
/// poofed in at.
fn run_until_wave_in(sim: &mut Sim) -> Vec<u64> {
    let size = sim
        .world()
        .resource::<Tuning>()
        .waves
        .wave_size(run(sim).wave) as usize;
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

/// The wave's spawn points and speeds, in poof-in order.
fn wave_signature(seed: u64) -> Vec<(Vec3, f32)> {
    let mut sim = Sim::waves(seed);
    let mut order: Vec<Entity> = Vec::new();
    for _ in 0..600 {
        sim.tick();
        for e in in_play(&mut sim) {
            if !order.contains(&e) {
                order.push(e);
            }
        }
        if order.len() == 3 {
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
        let eye = sim.feet(player) + Vec3::Y * sim.get::<EyeHeight>(player).0;
        let look = look_toward(at + Vec3::Y * 1.0 - eye);
        sim.set_look(player, look.yaw, look.pitch);
        sim.player_intent().fire_pressed = true;
        sim.tick();
        if sim.world().get::<Downed>(grunt).is_some() {
            return shot;
        }
    }
    panic!("three point-blank pumps didn't down the grunt");
}

#[test]
fn waves_has_no_dummy_and_practice_keeps_it() {
    let mut waves = Sim::waves(1);
    assert_eq!(count::<Dummy>(&mut waves), 0);
    assert!(waves.world().get_resource::<Run>().is_some());

    let mut practice = Sim::new();
    assert_eq!(count::<Dummy>(&mut practice), 1);
    assert!(practice.world().get_resource::<Run>().is_none());
    assert_eq!(count::<PoolGrunt>(&mut practice), 0);
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
}

#[test]
fn wave_one_poofs_in_three_grunts_at_the_edge_away_from_the_player() {
    let mut sim = Sim::waves(3);
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
    let tuning = sim.world().resource::<Tuning>().clone();
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
    }
    assert!(
        speeds.windows(2).any(|w| w[0] != w[1]),
        "speeds are jittered per knight: {speeds:?}"
    );
    let r = run(&sim);
    assert_eq!((r.remaining, r.alive), (0, 3));
    assert_eq!(
        pool(&mut sim).len() - grunts.len(),
        5,
        "the rest stay parked"
    );
}

#[test]
fn the_same_seed_gives_the_same_wave() {
    let a = wave_signature(11);
    assert_eq!(a.len(), 3);
    assert_eq!(a, wave_signature(11));
    assert_ne!(a, wave_signature(12));
}

#[test]
fn downing_all_three_clears_the_wave_and_it_comes_again() {
    let mut sim = Sim::waves(5);
    run_until_wave_in(&mut sim);
    let first = in_play(&mut sim);
    for &g in &first {
        pump_down(&mut sim, g);
    }
    sim.tick();
    let r = run(&sim);
    assert_eq!(r.eliminations, 3);
    assert!(
        matches!(r.phase, RunPhase::Cleared { .. }),
        "wave cleared: {:?}",
        r.phase
    );
    assert_eq!(r.waves_cleared, 1);
    // Downed grunts go back to the pool, and wave 1 comes again.
    sim.run_seconds(2.0);
    for &g in &first {
        assert!(is_parked(&sim, g), "{g} returned to the pool");
    }
    let arrivals = run_until_wave_in(&mut sim);
    assert_eq!(arrivals.len(), 3);
    let r = run(&sim);
    assert_eq!((r.wave, r.phase, r.alive), (1, RunPhase::Fighting, 3));
    assert_eq!(pool(&mut sim).len(), 8, "the pool never grows");
}

#[test]
fn the_players_death_ends_the_run_and_enter_restarts_it_in_place() {
    let mut sim = Sim::waves(9);
    let player = sim.player();
    let spawn = sim.world().resource::<ArenaLayout>().player_spawn;
    let cover = count::<Piece>(&mut sim);
    assert!(cover > 0, "Waves starts with the initial cover");
    run_until_wave_in(&mut sim);
    let seed = run(&sim).seed;

    // Mess the arena up: a built wall, an orb in flight, the player moved.
    building::place_piece(
        sim.world_mut(),
        PieceSlot::wall(GridCell::new(1, 1, 0), Facing::North),
    )
    .expect("wall placed");
    launch_orb(&mut sim, player, Vec3::X * 30.0);
    place(&mut sim, player, Vec3::new(-4.0, 0.0, 0.0));
    sim.run_seconds(0.2);

    // The player is eliminated.
    sim.world_mut()
        .get_mut::<Health>(player)
        .unwrap()
        .apply(1000.0);
    sim.tick();
    assert!(run(&sim).is_over(), "one life: the run is over");
    assert!(sim.world().get::<Downed>(player).is_some());

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

    // After the death beat the grunts stop.
    sim.run_seconds(1.5);
    for g in pool(&mut sim) {
        assert!(sim.world().get::<Parked>(g).is_some(), "{g} stopped");
    }

    // Enter: go again.
    sim.world_mut().write_message(RestartRun);
    sim.tick();
    let r = run(&sim);
    assert_eq!(r.phase, RunPhase::Fighting);
    assert_eq!((r.wave, r.eliminations, r.remaining), (1, 0, 3));
    assert_ne!(r.seed, seed, "a new seed");
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
    let tuning = sim.world().resource::<Tuning>().clone();
    assert_eq!(
        *sim.get::<Health>(player),
        Health::full(tuning.combat.max_hp, tuning.combat.max_shield)
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

#[test]
fn restarting_ten_times_leaks_nothing() {
    fn entities(sim: &mut Sim) -> usize {
        sim.world_mut().query::<Entity>().iter(sim.world()).count()
    }
    let mut sim = Sim::waves(21);
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
        sim.world_mut()
            .get_mut::<Health>(player)
            .unwrap()
            .apply(1000.0);
        sim.run_seconds(0.5);
        assert!(run(&sim).is_over());
        sim.world_mut().write_message(RestartRun);
        sim.tick();
    }
}
