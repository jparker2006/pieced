//! Movement (slice A): scripted `PlayerIntent` in, feet positions, eye height and
//! cues out, stepped one fixed 60 Hz tick at a time.

use avian3d::prelude::*;
use bevy::prelude::*;
use pieced::{
    arena::ArenaLayout,
    movement::Motor,
    player::{HEAD_CENTER, spawn_character},
    shared::{
        EyeHeight, Facing, GameCue, Health, Hitbox, Layer, LookAngles, PlayerIntent, TICK_SECONDS,
    },
    sim::Sim,
};

const DT: f32 = TICK_SECONDS;
const RADIUS: f32 = 0.35;

/// A static obstacle on the piece layer, like a placed building piece.
fn spawn_box(sim: &mut Sim, center: Vec3, size: Vec3) -> Entity {
    sim.world_mut()
        .spawn((
            RigidBody::Static,
            Collider::cuboid(size.x, size.y, size.z),
            CollisionLayers::new(Layer::Piece, LayerMask::ALL),
            Transform::from_translation(center),
        ))
        .id()
}

/// A 4 m wide wedge rising 3 m over 4 m toward -Z. `low_edge` is the center of
/// its bottom edge on the ground; the top edge is 4 m further north.
fn spawn_ramp(sim: &mut Sim, low_edge: Vec3) -> Entity {
    let points = vec![
        Vec3::new(-2.0, 0.0, 0.0),
        Vec3::new(2.0, 0.0, 0.0),
        Vec3::new(-2.0, 0.0, -4.0),
        Vec3::new(2.0, 0.0, -4.0),
        Vec3::new(-2.0, 3.0, -4.0),
        Vec3::new(2.0, 3.0, -4.0),
    ];
    sim.world_mut()
        .spawn((
            RigidBody::Static,
            Collider::convex_hull(points).expect("wedge hull"),
            CollisionLayers::new(Layer::Piece, LayerMask::ALL),
            Transform::from_translation(low_edge),
        ))
        .id()
}

/// Puts the player somewhere and lets it settle onto whatever is below.
fn place_player(sim: &mut Sim, feet: Vec3, facing: Facing) -> Entity {
    let player = sim.player();
    sim.world_mut()
        .get_mut::<Transform>(player)
        .unwrap()
        .translation = feet;
    sim.set_look(player, facing.yaw(), 0.0);
    *sim.intent(player) = PlayerIntent::default();
    sim.ticks(20);
    player
}

/// A fresh sim with physics settled and any static obstacles registered.
fn settled() -> Sim {
    let mut sim = Sim::new();
    sim.ticks(5);
    sim
}

fn flat_speed(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(b.x - a.x, b.z - a.z).length() / DT
}

/// Advances one tick and returns the horizontal speed over it.
fn tick_speed(sim: &mut Sim, who: Entity) -> f32 {
    let before = sim.feet(who);
    sim.tick();
    flat_speed(before, sim.feet(who))
}

/// Average horizontal speed over `ticks` ticks.
fn average_speed(sim: &mut Sim, who: Entity, ticks: u32) -> f32 {
    let before = sim.feet(who);
    sim.ticks(ticks);
    flat_speed(before, sim.feet(who)) / ticks as f32
}

fn assert_close(actual: f32, expected: f32, tolerance: f32, what: &str) {
    assert!(
        (actual - expected).abs() <= expected.abs() * tolerance,
        "{what}: {actual} not within {:.0}% of {expected}",
        tolerance * 100.0
    );
}

fn press_jump(sim: &mut Sim, who: Entity) {
    let mut intent = sim.intent(who);
    intent.jump = true;
    intent.jump_pressed = true;
}

fn press_crouch(sim: &mut Sim, who: Entity) {
    let mut intent = sim.intent(who);
    intent.crouch = true;
    intent.crouch_pressed = true;
}

fn cues(sim: &Sim, matches: impl Fn(&GameCue) -> bool) -> usize {
    sim.recorded::<GameCue>()
        .iter()
        .filter(|c| matches(c))
        .count()
}

#[test]
fn reaches_full_speed_in_about_a_tenth_of_a_second_and_stops_in_about_0_06() {
    let mut sim = settled();
    let player = place_player(&mut sim, Vec3::new(2.0, 0.0, 14.0), Facing::North);
    sim.intent(player).move_axis = Vec2::Y;
    let mut to_full = None;
    for n in 1..=30 {
        if tick_speed(&mut sim, player) >= 5.5 * 0.97 {
            to_full = Some(n as f32 * DT);
            break;
        }
    }
    let to_full = to_full.expect("reached run speed");
    assert!(
        (0.06..=0.12).contains(&to_full),
        "time to full speed {to_full}"
    );
    sim.ticks(30);
    sim.intent(player).move_axis = Vec2::ZERO;
    let mut to_stop = None;
    for n in 1..=30 {
        if tick_speed(&mut sim, player) < 0.05 {
            to_stop = Some(n as f32 * DT);
            break;
        }
    }
    let to_stop = to_stop.expect("stopped");
    assert!((0.03..=0.085).contains(&to_stop), "time to stop {to_stop}");
}

#[test]
fn run_sprint_and_crouch_speeds_match_the_spec() {
    let mut sim = settled();
    let player = place_player(&mut sim, Vec3::new(-20.0, 0.0, 20.0), Facing::North);

    sim.intent(player).move_axis = Vec2::Y;
    sim.ticks(30);
    assert_close(average_speed(&mut sim, player, 30), 5.5, 0.03, "run");

    sim.intent(player).sprint = true;
    sim.ticks(30);
    assert_close(average_speed(&mut sim, player, 30), 7.5, 0.03, "sprint");

    // Sprint only works moving forward: backpedalling with sprint held is a run.
    sim.intent(player).move_axis = Vec2::NEG_Y;
    sim.ticks(30);
    assert_close(
        average_speed(&mut sim, player, 30),
        5.5,
        0.03,
        "sprint backward",
    );

    // Holding crouch (no press edge) while sprinting is a crouch walk, not a slide.
    {
        let mut intent = sim.intent(player);
        intent.move_axis = Vec2::Y;
        intent.crouch = true;
    }
    sim.ticks(30);
    assert_close(average_speed(&mut sim, player, 30), 2.8, 0.03, "crouch");
}

#[test]
fn slide_boosts_decays_to_crouch_speed_and_has_a_cooldown() {
    let mut sim = settled();
    sim.record::<GameCue>();
    let player = place_player(&mut sim, Vec3::new(-20.0, 0.0, 22.0), Facing::North);
    {
        let mut intent = sim.intent(player);
        intent.move_axis = Vec2::Y;
        intent.sprint = true;
    }
    sim.ticks(30);
    press_crouch(&mut sim, player);
    let speeds: Vec<f32> = (0..48).map(|_| tick_speed(&mut sim, player)).collect();
    assert_eq!(cues(&sim, |c| matches!(c, GameCue::SlideStart { .. })), 1);
    assert_close(speeds[0], 9.0, 0.03, "slide entry speed");
    assert_close(speeds[24], (9.0 + 2.8) / 2.0, 0.05, "slide speed halfway");
    assert!(
        speeds.windows(2).all(|w| w[1] <= w[0] + 1e-3),
        "slide speed only decays: {speeds:?}"
    );
    assert!(
        sim.get::<Motor>(player).crouched,
        "stays crouched while sliding"
    );
    // Still holding crouch: the slide ends in a crouch walk (0.2 s of it).
    assert_close(
        average_speed(&mut sim, player, 12),
        2.8,
        0.03,
        "after slide",
    );

    // Stand and sprint back to full speed, then press crouch 0.3 s after the
    // slide ended, inside the 0.5 s cooldown: no slide.
    sim.intent(player).crouch = false;
    sim.ticks(6);
    assert!(tick_speed(&mut sim, player) > 7.4, "sprinting again");
    press_crouch(&mut sim, player);
    let during_cooldown = tick_speed(&mut sim, player);
    assert_eq!(cues(&sim, |c| matches!(c, GameCue::SlideStart { .. })), 1);
    assert!(during_cooldown < 7.6, "no boost during cooldown");

    // After the cooldown a new slide starts.
    sim.intent(player).crouch = false;
    sim.ticks(30);
    press_crouch(&mut sim, player);
    let entry = tick_speed(&mut sim, player);
    assert_eq!(cues(&sim, |c| matches!(c, GameCue::SlideStart { .. })), 2);
    assert_close(entry, 9.0, 0.03, "second slide entry speed");
}

#[test]
fn jump_peaks_about_1_2_m_and_lands() {
    let mut sim = settled();
    sim.record::<GameCue>();
    let player = place_player(&mut sim, Vec3::new(2.0, 0.0, 14.0), Facing::North);
    let ground = sim.feet(player).y;
    press_jump(&mut sim, player);
    let mut apex = ground;
    for _ in 0..60 {
        sim.tick();
        apex = apex.max(sim.feet(player).y);
    }
    let height = apex - ground;
    assert!((1.1..=1.3).contains(&height), "jump apex {height}");
    assert!((sim.feet(player).y - ground).abs() < 0.02, "landed again");
    assert_eq!(cues(&sim, |c| matches!(c, GameCue::Jump { .. })), 1);
    let lands: Vec<f32> = sim
        .recorded::<GameCue>()
        .iter()
        .filter_map(|c| match c {
            GameCue::Land { speed, .. } => Some(*speed),
            _ => None,
        })
        .collect();
    assert_eq!(lands.len(), 1, "one landing cue");
    assert!(lands[0] > 5.0, "landing speed {}", lands[0]);
}

#[test]
fn sprint_jumping_never_clears_a_3_m_wall() {
    let mut sim = settled();
    let wall_z = 8.0;
    spawn_box(
        &mut sim,
        Vec3::new(2.0, 1.5, wall_z),
        Vec3::new(8.0, 3.0, 0.2),
    );
    sim.tick();
    let player = place_player(&mut sim, Vec3::new(2.0, 0.0, 14.0), Facing::North);
    {
        let mut intent = sim.intent(player);
        intent.move_axis = Vec2::Y;
        intent.sprint = true;
    }
    let mut max_y: f32 = 0.0;
    for n in 0..300 {
        if n % 15 == 0 {
            press_jump(&mut sim, player);
        }
        sim.tick();
        let feet = sim.feet(player);
        max_y = max_y.max(feet.y);
        assert!(
            feet.z >= wall_z + 0.1 + RADIUS - 0.02,
            "went through: {feet}"
        );
    }
    assert!(max_y < 1.3, "jumped {max_y}");
}

/// Runs a jump from flat ground and returns the tick count at which it lands.
fn landing_tick(sim: &mut Sim, player: Entity) -> u32 {
    press_jump(sim, player);
    for n in 1..=120 {
        sim.tick();
        if n > 5 && sim.get::<Motor>(player).grounded {
            return n;
        }
    }
    panic!("never landed");
}

#[test]
fn jump_pressed_shortly_before_landing_jumps_on_landing() {
    let mut control = settled();
    let player = place_player(&mut control, Vec3::new(2.0, 0.0, 14.0), Facing::North);
    let land = landing_tick(&mut control, player);

    for (early, should_jump) in [(5, true), (12, false)] {
        let mut sim = settled();
        let player = place_player(&mut sim, Vec3::new(2.0, 0.0, 14.0), Facing::North);
        let ground = sim.feet(player).y;
        press_jump(&mut sim, player);
        sim.tick();
        sim.intent(player).jump = false;
        for n in 2..=land {
            if n == land - early {
                press_jump(&mut sim, player);
            }
            sim.tick();
            sim.intent(player).jump = false;
        }
        let mut apex = ground;
        for _ in 0..40 {
            sim.tick();
            apex = apex.max(sim.feet(player).y);
        }
        let rebound = apex - ground;
        if should_jump {
            assert!(
                rebound > 1.0,
                "press {early} ticks early should jump on landing ({rebound})"
            );
        } else {
            assert!(
                rebound < 0.05,
                "press {early} ticks early is outside the buffer ({rebound})"
            );
        }
    }
}

#[test]
fn coyote_time_allows_a_jump_just_after_walking_off_an_edge() {
    // A 1 m high platform whose north edge is at z = 10.
    let setup = || {
        let mut sim = settled();
        spawn_box(
            &mut sim,
            Vec3::new(2.0, 0.5, 13.0),
            Vec3::new(4.0, 1.0, 6.0),
        );
        sim.tick();
        let player = place_player(&mut sim, Vec3::new(2.0, 1.0, 14.0), Facing::North);
        assert!(sim.get::<Motor>(player).grounded);
        sim.intent(player).move_axis = Vec2::Y;
        (sim, player)
    };
    let (mut control, player) = setup();
    let mut left = None;
    for n in 1..=120 {
        control.tick();
        if !control.get::<Motor>(player).grounded {
            left = Some(n);
            break;
        }
    }
    let left = left.expect("walked off the edge");

    for (late, should_jump) in [(5, true), (12, false)] {
        let (mut sim, player) = setup();
        sim.ticks(left + late - 1);
        let before = sim.feet(player).y;
        assert!(before < 1.0 + 0.02, "already off the edge");
        press_jump(&mut sim, player);
        let mut apex = before;
        for _ in 0..40 {
            sim.tick();
            apex = apex.max(sim.feet(player).y);
        }
        if should_jump {
            assert!(
                apex > 1.8,
                "jump {late} ticks after leaving the edge should work ({apex})"
            );
        } else {
            assert!(apex <= before + 1e-3, "no jump {late} ticks late ({apex})");
        }
    }
}

#[test]
fn walks_up_a_ramp_to_the_next_level_and_back_down() {
    let mut sim = settled();
    // Ramp from z = 12 (ground) up to z = 8 (3 m), then a floor at level 1.
    spawn_ramp(&mut sim, Vec3::new(2.0, 0.0, 12.0));
    spawn_box(&mut sim, Vec3::new(2.0, 2.9, 4.0), Vec3::new(4.0, 0.2, 8.0));
    sim.tick();
    let player = place_player(&mut sim, Vec3::new(2.0, 0.0, 15.0), Facing::North);
    sim.intent(player).move_axis = Vec2::Y;
    let mut last = sim.feet(player);
    for _ in 0..120 {
        sim.tick();
        let feet = sim.feet(player);
        assert!(
            feet.y >= last.y - 1e-3,
            "no dips climbing: {last} -> {feet}"
        );
        assert!(feet.y - last.y < 0.12, "no pops climbing: {last} -> {feet}");
        last = feet;
    }
    let top = sim.feet(player);
    assert!(top.z < 8.0, "reached the floor at the top: {top}");
    assert!((2.99..=3.05).contains(&top.y), "feet at level 1: {top}");

    // Walk back down: stays glued to the ramp, never airborne.
    sim.intent(player).move_axis = Vec2::ZERO;
    sim.ticks(10);
    sim.set_look(player, Facing::South.yaw(), 0.0);
    {
        let mut intent = sim.intent(player);
        intent.move_axis = Vec2::Y;
        intent.sprint = true;
    }
    for _ in 0..90 {
        sim.tick();
        let feet = sim.feet(player);
        assert!(
            sim.get::<Motor>(player).grounded,
            "left the ground going down at {feet}"
        );
    }
    let bottom = sim.feet(player);
    assert!(
        bottom.z > 12.0 && bottom.y < 0.05,
        "back on the ground: {bottom}"
    );
}

#[test]
fn ramp_rush_climbs_consecutive_ramps_without_leaving_the_ground() {
    let mut sim = settled();
    // Two ramps end to end, then a floor at level 2.
    spawn_ramp(&mut sim, Vec3::new(2.0, 0.0, 12.0));
    spawn_ramp(&mut sim, Vec3::new(2.0, 3.0, 8.0));
    spawn_box(&mut sim, Vec3::new(2.0, 5.9, 2.0), Vec3::new(4.0, 0.2, 4.0));
    sim.tick();
    let player = place_player(&mut sim, Vec3::new(2.0, 0.0, 14.0), Facing::North);
    {
        let mut intent = sim.intent(player);
        intent.move_axis = Vec2::Y;
        intent.sprint = true;
    }
    let mut last = sim.feet(player);
    for n in 0..100 {
        sim.tick();
        let feet = sim.feet(player);
        assert!(sim.get::<Motor>(player).grounded, "airborne at {feet}");
        assert!(feet.y - last.y < 0.12, "popped up at {feet}");
        // Full sprint speed (reached in 0.1 s) all the way up: no snagging.
        assert!(
            n < 8 || feet.z < 4.0 || flat_speed(last, feet) > 7.5 * 0.97,
            "lost speed at {feet}"
        );
        last = feet;
    }
    let top = sim.feet(player);
    assert!(
        top.z < 4.0 && (5.99..=6.05).contains(&top.y),
        "at level 2: {top}"
    );
}

#[test]
fn steps_onto_low_ledges_but_not_high_ones() {
    let mut sim = settled();
    // A 0.2 m slab (a floor on the ground) and, further east, a 0.45 m block.
    spawn_box(
        &mut sim,
        Vec3::new(-10.0, 0.1, 8.0),
        Vec3::new(4.0, 0.2, 4.0),
    );
    spawn_box(
        &mut sim,
        Vec3::new(10.0, 0.225, 8.0),
        Vec3::new(4.0, 0.45, 4.0),
    );
    sim.tick();
    let player = place_player(&mut sim, Vec3::new(-10.0, 0.0, 12.0), Facing::North);
    sim.intent(player).move_axis = Vec2::Y;
    let mut last = sim.feet(player);
    for _ in 0..45 {
        sim.tick();
        let feet = sim.feet(player);
        assert!(
            feet.y - last.y < 0.2,
            "step pops smoothly: {last} -> {feet}"
        );
        last = feet;
    }
    let on_slab = sim.feet(player);
    assert!(
        on_slab.z < 9.0 && (0.2..0.23).contains(&on_slab.y),
        "stepped up onto the slab: {on_slab}"
    );

    let player = place_player(&mut sim, Vec3::new(10.0, 0.0, 12.0), Facing::North);
    sim.intent(player).move_axis = Vec2::Y;
    sim.run_seconds(1.5);
    let blocked = sim.feet(player);
    assert!(
        blocked.z >= 10.0 + RADIUS - 0.02 && blocked.y < 0.05,
        "blocked by the block: {blocked}"
    );
}

#[test]
fn never_tunnels_through_a_thin_wall_at_sprint_or_slide_speed() {
    let mut sim = settled();
    let wall_z = 6.0;
    spawn_box(
        &mut sim,
        Vec3::new(2.0, 2.0, wall_z),
        Vec3::new(12.0, 4.0, 0.2),
    );
    sim.tick();
    let player = place_player(&mut sim, Vec3::new(2.0, 0.0, 14.0), Facing::North);
    {
        let mut intent = sim.intent(player);
        intent.move_axis = Vec2::Y;
        intent.sprint = true;
    }
    let limit = wall_z + 0.1 + RADIUS - 0.02;
    for n in 0..300 {
        // Back off, sprint in and slide into the wall, repeatedly.
        match n % 100 {
            0 => sim.intent(player).move_axis = Vec2::NEG_Y,
            40 => sim.intent(player).move_axis = Vec2::Y,
            55 => press_crouch(&mut sim, player),
            85 => sim.intent(player).crouch = false,
            _ => {}
        }
        sim.tick();
        assert!(sim.feet(player).z >= limit, "tunneled at tick {n}");
    }
    // Far faster than the game ever goes: back off, sprint and slide head-on.
    {
        let mut tuning = sim.tuning_mut();
        tuning.movement.run_speed = 20.0;
        tuning.movement.sprint_speed = 30.0;
        tuning.movement.slide_speed = 40.0;
    }
    for n in 0..300 {
        match n % 60 {
            0 => {
                let mut intent = sim.intent(player);
                intent.crouch = false;
                intent.move_axis = Vec2::NEG_Y;
            }
            25 => sim.intent(player).move_axis = Vec2::Y,
            31 => press_crouch(&mut sim, player),
            _ => {}
        }
        sim.tick();
        assert!(sim.feet(player).z >= limit, "tunneled at speed, tick {n}");
    }
}

#[test]
fn stays_inside_the_arena_bounds() {
    let mut sim = settled();
    let layout = sim.world().resource::<ArenaLayout>().clone();
    let player = place_player(&mut sim, Vec3::new(18.0, 0.0, 18.0), Facing::East);
    {
        let mut intent = sim.intent(player);
        intent.move_axis = Vec2::new(-0.6, 0.8);
        intent.sprint = true;
    }
    let check = |sim: &Sim| {
        let feet = sim.feet(player);
        assert!(
            feet.x >= layout.bounds_min.x
                && feet.x <= layout.bounds_max.x
                && feet.z >= layout.bounds_min.y
                && feet.z <= layout.bounds_max.y,
            "out of bounds: {feet}"
        );
    };
    for n in 0..300 {
        if n % 20 == 0 {
            press_jump(&mut sim, player);
        }
        if n == 150 {
            sim.set_look(player, Facing::South.yaw(), 0.0);
        }
        sim.tick();
        check(&sim);
    }
    let feet = sim.feet(player);
    assert!(
        (feet.x - layout.bounds_max.x).abs() < 0.3,
        "pressed against the east edge: {feet}"
    );
}

#[test]
fn standing_still_does_not_drift_on_flat_ground_or_a_ramp() {
    let mut sim = settled();
    spawn_ramp(&mut sim, Vec3::new(-10.0, 0.0, 12.0));
    sim.tick();
    let player = place_player(&mut sim, Vec3::new(2.0, 0.0, 14.0), Facing::North);
    let start = sim.feet(player);
    sim.run_seconds(5.0);
    assert!(
        sim.feet(player).distance(start) < 1e-4,
        "drifted on flat ground"
    );

    // Halfway up the ramp (surface at 1.5 m).
    let player = place_player(&mut sim, Vec3::new(-10.0, 1.7, 10.0), Facing::East);
    let start = sim.feet(player);
    assert!(sim.get::<Motor>(player).grounded, "standing on the ramp");
    assert!(
        (1.5..1.65).contains(&start.y),
        "on the ramp surface: {start}"
    );
    sim.run_seconds(3.0);
    assert!(
        sim.feet(player).distance(start) < 1e-3,
        "slid on the ramp: {start} -> {}",
        sim.feet(player)
    );
}

#[test]
fn another_character_moves_from_its_own_intent() {
    let mut sim = settled();
    let player = sim.player();
    let other = {
        let world = sim.world_mut();
        let entity = spawn_character(
            &mut world.commands(),
            Vec3::new(-10.0, 0.0, 0.0),
            LookAngles {
                yaw: Facing::East.yaw(),
                pitch: 0.0,
            },
            Health::default(),
            (),
        );
        world.flush();
        entity
    };
    sim.ticks(10);
    let player_start = sim.feet(player);
    let other_start = sim.feet(other);
    sim.intent(other).move_axis = Vec2::Y;
    sim.run_seconds(1.0);
    let moved = sim.feet(other) - other_start;
    assert!(moved.x > 5.0 && moved.z.abs() < 1e-3, "moved east: {moved}");
    assert!(
        sim.feet(player).distance(player_start) < 1e-4,
        "player stayed"
    );
}

#[test]
fn cannot_stand_up_under_a_low_ceiling() {
    let mut sim = settled();
    // A slab whose underside is 1.5 m up, covering z 6..10.
    spawn_box(&mut sim, Vec3::new(2.0, 1.6, 8.0), Vec3::new(6.0, 0.2, 4.0));
    sim.tick();

    // Standing, you can't walk under it.
    let player = place_player(&mut sim, Vec3::new(2.0, 0.0, 12.0), Facing::North);
    sim.intent(player).move_axis = Vec2::Y;
    sim.run_seconds(1.5);
    assert!(
        sim.feet(player).z >= 10.0 + RADIUS - 0.02,
        "walked into the slab"
    );

    // Crouched, you can; releasing crouch underneath keeps you crouched.
    press_crouch(&mut sim, player);
    sim.ticks(60);
    assert!(
        sim.feet(player).z < 8.5,
        "crawled under: {}",
        sim.feet(player)
    );
    {
        let mut intent = sim.intent(player);
        intent.crouch = false;
        intent.move_axis = Vec2::ZERO;
    }
    sim.ticks(30);
    assert!(sim.get::<Motor>(player).crouched, "still crouched");
    assert!((sim.get::<EyeHeight>(player).0 - 1.05).abs() < 1e-3);

    // Walking out the far side, you stand up.
    sim.intent(player).move_axis = Vec2::Y;
    sim.ticks(60);
    assert!(sim.feet(player).z < 6.0 - RADIUS, "left the slab");
    sim.intent(player).move_axis = Vec2::ZERO;
    sim.ticks(10);
    assert!(!sim.get::<Motor>(player).crouched, "stood up");
    assert!((sim.get::<EyeHeight>(player).0 - 1.62).abs() < 1e-3);
}

#[test]
fn crouching_eases_the_eye_down_and_lowers_the_hitboxes() {
    let mut sim = settled();
    let player = place_player(&mut sim, Vec3::new(2.0, 0.0, 14.0), Facing::North);
    let head = sim
        .world_mut()
        .query::<(Entity, &Hitbox)>()
        .iter(sim.world())
        .find(|(_, h)| h.owner == player && h.head)
        .map(|(e, _)| e)
        .expect("head hitbox");
    let head_y = |sim: &Sim| sim.get::<Transform>(head).translation.y;
    assert!((head_y(&sim) - HEAD_CENTER).abs() < 1e-4);

    press_crouch(&mut sim, player);
    sim.tick();
    let first = sim.get::<EyeHeight>(player).0;
    assert!(first < 1.62 && first > 1.05, "eases, not instant: {first}");
    sim.ticks(6);
    assert!((sim.get::<EyeHeight>(player).0 - 1.05).abs() < 1e-4);
    assert!((head_y(&sim) - (HEAD_CENTER - 0.57)).abs() < 1e-3);

    sim.intent(player).crouch = false;
    sim.ticks(7);
    assert!((sim.get::<EyeHeight>(player).0 - 1.62).abs() < 1e-4);
    assert!((head_y(&sim) - HEAD_CENTER).abs() < 1e-4);
}

#[test]
fn pieces_placed_into_a_character_push_it_out() {
    let mut sim = settled();
    let player = place_player(&mut sim, Vec3::new(2.0, 0.0, 14.0), Facing::North);

    // A wall placed through the edge of the capsule.
    let wall_x = 2.15;
    spawn_box(
        &mut sim,
        Vec3::new(wall_x, 1.5, 14.0),
        Vec3::new(0.2, 3.0, 4.0),
    );
    sim.ticks(3);
    let feet = sim.feet(player);
    assert!(
        (feet.x - wall_x).abs() >= 0.1 + RADIUS - 0.005,
        "still inside the wall: {feet}"
    );

    // A floor slab placed across the character's feet lifts it on top.
    spawn_box(
        &mut sim,
        Vec3::new(0.0, 0.1, 14.0),
        Vec3::new(6.0, 0.2, 6.0),
    );
    sim.ticks(3);
    let feet = sim.feet(player);
    assert!((0.2..0.25).contains(&feet.y), "on top of the slab: {feet}");
    assert!(sim.get::<Motor>(player).grounded);
}

#[test]
fn air_control_is_weaker_than_ground_control() {
    let mut sim = settled();
    let player = place_player(&mut sim, Vec3::new(2.0, 0.0, 20.0), Facing::North);
    sim.intent(player).move_axis = Vec2::Y;
    sim.ticks(30);
    press_jump(&mut sim, player);
    sim.tick();
    // Let go mid-air: momentum carries.
    sim.intent(player).move_axis = Vec2::ZERO;
    assert_close(
        average_speed(&mut sim, player, 6),
        5.5,
        0.02,
        "air momentum",
    );
    // Pull back mid-air: slows at 40% of ground acceleration.
    sim.intent(player).move_axis = Vec2::NEG_Y;
    sim.ticks(6);
    let speed = sim.get::<Motor>(player).velocity.z;
    assert!(
        (-4.0..=-2.6).contains(&speed),
        "forward speed after 0.1 s of pulling back in the air: {speed}"
    );
}

#[test]
fn footsteps_follow_distance_walked() {
    let mut sim = settled();
    sim.record::<GameCue>();
    let player = place_player(&mut sim, Vec3::new(2.0, 0.0, 20.0), Facing::North);
    sim.clear_recorded::<GameCue>();
    let start = sim.feet(player);
    sim.intent(player).move_axis = Vec2::Y;
    sim.run_seconds(2.0);
    let walked = sim.feet(player).distance(start);
    let steps = cues(&sim, |c| matches!(c, GameCue::Footstep { .. }));
    assert_eq!(steps, (walked / 2.2).floor() as usize, "walked {walked}");

    // Crouch walking is quiet.
    sim.clear_recorded::<GameCue>();
    sim.intent(player).crouch = true;
    sim.run_seconds(2.0);
    assert_eq!(cues(&sim, |c| matches!(c, GameCue::Footstep { .. })), 0);
}
