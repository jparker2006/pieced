//! The void (docs/M3-SPEC.md → The grunt, items 11–12; D78; Testing
//! Decisions → Knockback and void) through the simulation seam: a point-blank
//! pump near the edge knocks a knight through the barrier; it's flung clear of
//! the island's margin, falls, and counts as an elimination with the void
//! bonus, credited to the player who pumped it. The rifle never does it, and
//! the player can never leave the island.

use bevy::prelude::*;
use pieced::{
    arena::{ArenaLayout, IslandRim},
    combat::{CombatStats, Downed},
    dummy::look_toward,
    grunt::{self, GruntStats, Parked},
    movement::{ISLAND_TOP, Knockback, VOID_DEPTH, VoidFall},
    player,
    shared::{
        ARENA_HALF, ActiveTool, Eliminated, EyeHeight, GalleryFreeze, GameCue, Health, LookAngles,
        PreviousFeet, WeaponKind,
    },
    sim::Sim,
    tuning::Tuning,
    waves::{PoolGrunt, Run, VoidKill},
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn place(sim: &mut Sim, who: Entity, feet: Vec3) {
    sim.world_mut()
        .get_mut::<Transform>(who)
        .unwrap()
        .translation = feet;
    if let Some(mut prev) = sim.world_mut().get_mut::<PreviousFeet>(who) {
        prev.0 = feet;
    }
}

fn toughen(sim: &mut Sim, who: Entity) {
    let mut health = Health::full(1.0e9, 100.0);
    health.shield = 100.0;
    *sim.world_mut().get_mut::<Health>(who).unwrap() = health;
}

fn aim(sim: &mut Sim, who: Entity, at: Vec3) {
    let eye = sim.feet(who) + Vec3::Y * sim.get::<EyeHeight>(who).0;
    let look = look_toward(at - eye);
    sim.set_look(who, look.yaw, look.pitch);
}

/// `shooter` switches to `weapon`, aims at `target`'s chest and fires once.
fn shoot(sim: &mut Sim, shooter: Entity, target: Entity, weapon: WeaponKind) {
    sim.intent(shooter).select = Some(ActiveTool::Weapon(weapon));
    sim.run_seconds(0.4);
    let chest = sim.feet(target) + Vec3::Y * 1.0;
    aim(sim, shooter, chest);
    sim.intent(shooter).fire_pressed = true;
    sim.tick();
}

/// A grunt-lab sim (Waves rules, no director) with a frozen grunt standing
/// at `knight_feet` and the player at `player_feet`.
fn lab(knight_feet: Vec3, player_feet: Vec3) -> (Sim, Entity, Entity) {
    let mut sim = Sim::grunt_lab(1);
    sim.world_mut().insert_resource(GalleryFreeze);
    let stats = GruntStats::for_wave(1, &sim.world().resource::<Tuning>().grunt);
    let knight = {
        let world = sim.world_mut();
        let mut commands = world.commands();
        let k = grunt::spawn_grunt(&mut commands, knight_feet, LookAngles::default(), stats);
        world.flush();
        k
    };
    let player = sim.player();
    toughen(&mut sim, player);
    toughen(&mut sim, knight);
    place(&mut sim, player, player_feet);
    sim.run_seconds(0.3);
    (sim, player, knight)
}

/// East side, on the close edge (the narrowest margin), and north side (a
/// wide one): the knight 3 m inside the barrier, the player 2 m behind it.
const EAST: (Vec3, Vec3) = (Vec3::new(21.0, 0.0, 0.0), Vec3::new(19.0, 0.0, 0.0));
const NORTH: (Vec3, Vec3) = (Vec3::new(-4.0, 0.0, -21.0), Vec3::new(-4.0, 0.0, -19.0));

// ---------------------------------------------------------------------------
// Knocked off
// ---------------------------------------------------------------------------

#[test]
fn a_point_blank_pump_at_the_edge_knocks_a_knight_through_the_barrier() {
    for (knight_feet, player_feet) in [EAST, NORTH] {
        let (mut sim, player, knight) = lab(knight_feet, player_feet);
        sim.record::<GameCue>();
        sim.record::<Eliminated>();
        sim.record::<VoidKill>();
        shoot(&mut sim, player, knight, WeaponKind::Pump);
        assert!(sim.world().get::<Knockback>(knight).is_some(), "shoved");
        // Within a second it's past the barrier and flying.
        let mut crossed = None;
        for n in 0..60 {
            sim.tick();
            if sim.world().get::<VoidFall>(knight).is_some() {
                crossed = Some(n);
                break;
            }
        }
        assert!(crossed.is_some(), "knocked off at {knight_feet}");
        let fall = *sim.get::<VoidFall>(knight);
        assert_eq!(fall.by, Some(player), "credited to the pumper");
        let feet = sim.feet(knight);
        assert!(
            feet.x.abs() > ARENA_HALF || feet.z.abs() > ARENA_HALF,
            "past the barrier line: {feet}"
        );
        assert!(
            sim.recorded::<GameCue>()
                .contains(&GameCue::VoidFall { who: knight }),
            "the yelp cue"
        );
        assert!(sim.world().get::<Knockback>(knight).is_none());

        // It falls; the moment it is 3 m below the island top it's out.
        let mut eliminated_at = None;
        for _ in 0..(3 * 60) {
            sim.tick();
            if sim.world().get::<Downed>(knight).is_some() {
                eliminated_at = Some(sim.feet(knight).y);
                break;
            }
            assert!(
                sim.feet(knight).y > ISLAND_TOP - VOID_DEPTH - 1.0,
                "not yet counted at {}",
                sim.feet(knight).y
            );
        }
        let y = eliminated_at.expect("eliminated in the void");
        assert!(
            y <= ISLAND_TOP - VOID_DEPTH && y > ISLAND_TOP - VOID_DEPTH - 1.0,
            "counted as it passed 3 m down ({y:.2})"
        );
        let elim = sim.recorded::<Eliminated>();
        assert_eq!(elim.len(), 1);
        assert_eq!((elim[0].victim, elim[0].by), (knight, Some(player)));
        assert_eq!(sim.recorded::<VoidKill>().len(), 1);
        assert_eq!(sim.world().resource::<CombatStats>().eliminations, 1);
    }
}

#[test]
fn the_fling_clears_the_islands_margin_before_it_drops() {
    let rim = IslandRim::default();
    for (knight_feet, player_feet) in [EAST, NORTH] {
        let (mut sim, player, knight) = lab(knight_feet, player_feet);
        shoot(&mut sim, player, knight, WeaponKind::Pump);
        let mut below = false;
        for _ in 0..(2 * 60) {
            sim.tick();
            let feet = sim.feet(knight);
            if sim.world().get::<VoidFall>(knight).is_some() && feet.y < ISLAND_TOP {
                below = true;
                assert!(
                    !rim.on_island(feet),
                    "fell through the island's top at {feet}"
                );
            }
        }
        assert!(below, "it fell");
        // And it tumbles away from the island, never back over it.
        let feet = sim.feet(knight);
        assert!(!rim.on_island(feet), "{feet}");
    }
}

#[test]
fn the_rifle_never_knocks_a_knight_off() {
    let (mut sim, player, knight) = lab(EAST.0 + Vec3::X * 1.5, EAST.1 + Vec3::X * 1.5);
    for _ in 0..6 {
        shoot(&mut sim, player, knight, WeaponKind::Rifle);
        sim.run_seconds(0.3);
    }
    sim.run_seconds(1.0);
    assert!(sim.world().get::<VoidFall>(knight).is_none());
    let bounds = sim.world().resource::<ArenaLayout>().bounds_max;
    assert!(sim.feet(knight).x <= bounds.x + 1e-3);
}

#[test]
fn a_far_pump_near_the_edge_only_nudges() {
    // Beyond the pump's falloff the shove is at most `pump_knockback` times
    // the falloff floor: short of the barrier 3 m behind the knight.
    let t = Tuning::default();
    let distance = t.combat.pump.falloff_end + 2.0;
    let most = t.grunt.pump_knockback * t.combat.pump.falloff(distance);
    assert!(most < 2.5, "a far pump shoves up to {most:.2} m");
    // z = -10: a clear lane (the initial cover stands across z = 0).
    let knight_feet = Vec3::new(ARENA_HALF - 3.0, 0.0, -10.0);
    let (mut sim, player, knight) = lab(knight_feet, knight_feet - Vec3::X * distance);
    shoot(&mut sim, player, knight, WeaponKind::Pump);
    sim.run_seconds(1.5);
    assert!(sim.world().get::<VoidFall>(knight).is_none());
    let bounds = sim.world().resource::<ArenaLayout>().bounds_max;
    assert!(
        sim.feet(knight).x <= bounds.x + 1e-3,
        "back inside the bounds"
    );
}

// ---------------------------------------------------------------------------
// Scoring (the director)
// ---------------------------------------------------------------------------

#[test]
fn a_void_knockoff_scores_the_kill_and_the_bonus() {
    let mut sim = Sim::waves(4);
    sim.world_mut().insert_resource(GalleryFreeze);
    let player = sim.player();
    toughen(&mut sim, player);
    // The first knight to land.
    let mut knight = None;
    for _ in 0..(15 * 60) {
        sim.tick();
        let mut q = sim
            .world_mut()
            .query_filtered::<Entity, (With<PoolGrunt>, Without<Parked>, Without<Downed>)>();
        if let Some(k) = q.iter(sim.world()).next() {
            knight = Some(k);
            break;
        }
    }
    let knight = knight.expect("a knight landed");
    toughen(&mut sim, knight);
    place(&mut sim, knight, NORTH.0);
    place(&mut sim, player, NORTH.1);
    sim.run_seconds(0.2);
    let before = sim.world().resource::<Run>().clone();
    assert_eq!((before.score, before.eliminations), (0, 0));
    shoot(&mut sim, player, knight, WeaponKind::Pump);
    sim.run_seconds(2.0);
    let r = sim.world().resource::<Run>().clone();
    let t = sim.world().resource::<Tuning>().waves.clone();
    assert_eq!(r.eliminations, 1, "an elimination");
    assert_eq!(r.void_kills, 1, "a void knock-off");
    assert_eq!(r.score, t.score_kill + t.score_void, "100 + 150");
    // It goes back to the pool like any downed knight.
    sim.run_seconds(1.0);
    assert!(sim.world().get::<Parked>(knight).is_some());
    assert!(sim.world().get::<VoidFall>(knight).is_none());
}

// ---------------------------------------------------------------------------
// The player never falls
// ---------------------------------------------------------------------------

#[test]
fn the_player_walking_into_the_barrier_never_leaves_the_island() {
    let mut sim = Sim::waves(5);
    sim.world_mut().insert_resource(GalleryFreeze);
    let player = sim.player();
    toughen(&mut sim, player);
    let bounds = sim.world().resource::<ArenaLayout>().clone();
    // Run, sprint and jump into each side, and the corners.
    for (yaw_deg, start) in [
        (-90.0f32, Vec3::new(18.0, 0.0, 0.0)),
        (90.0, Vec3::new(-18.0, 0.0, 0.0)),
        (0.0, Vec3::new(0.0, 0.0, -18.0)),
        (180.0, Vec3::new(0.0, 0.0, 18.0)),
        (-45.0, Vec3::new(18.0, 0.0, -18.0)),
    ] {
        place(&mut sim, player, start);
        sim.set_look(player, yaw_deg.to_radians(), 0.0);
        for n in 0..(3 * 60) {
            {
                let mut i = sim.player_intent();
                i.move_axis = Vec2::Y;
                i.sprint = true;
                i.jump_pressed = n % 40 == 0;
                i.jump = n % 40 < 10;
            }
            sim.tick();
            let feet = sim.feet(player);
            assert!(
                feet.x >= bounds.bounds_min.x - 1e-3
                    && feet.x <= bounds.bounds_max.x + 1e-3
                    && feet.z >= bounds.bounds_min.y - 1e-3
                    && feet.z <= bounds.bounds_max.y + 1e-3,
                "the player left the island: {feet}"
            );
            assert!(feet.y > -0.1, "the player never falls: {feet}");
        }
        assert!(sim.world().get::<VoidFall>(player).is_none());
    }
}

#[test]
fn a_pump_at_the_edge_never_pushes_the_player_off() {
    let mut sim = Sim::waves(6);
    sim.world_mut().insert_resource(GalleryFreeze);
    let player = sim.player();
    toughen(&mut sim, player);
    place(&mut sim, player, Vec3::new(22.5, 0.0, 0.0));
    let shooter = {
        let world = sim.world_mut();
        let mut commands = world.commands();
        let e = player::spawn_character(
            &mut commands,
            Vec3::new(21.0, 0.0, 0.0),
            LookAngles::default(),
            Health::default(),
            (),
        );
        world.flush();
        e
    };
    sim.run_seconds(0.3);
    for _ in 0..3 {
        shoot(&mut sim, shooter, player, WeaponKind::Pump);
        sim.run_seconds(0.8);
        assert!(sim.world().get::<Knockback>(player).is_none());
        assert!(sim.world().get::<VoidFall>(player).is_none());
    }
    let feet = sim.feet(player);
    let bounds = sim.world().resource::<ArenaLayout>().bounds_max;
    assert!(feet.x <= bounds.x + 1e-3 && feet.y > -0.1, "{feet}");
}
