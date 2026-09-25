//! The training dummy through the simulation seam: its controller's intent,
//! elimination, untargetability while downed, and respawn.
//!
//! Movement lives in another slice, so these tests check the dummy's *intent*
//! (strafe axis, direction changes, jumps) and move it by `Transform` directly.

use bevy::prelude::*;
use pieced::{
    arena::ArenaLayout,
    dummy::{Downed, Dummy, look_toward},
    shared::{
        DamageDealt, Eliminated, EyeHeight, GameCue, Health, LookAngles, PlayerIntent, ShotFired,
        TICK_SECONDS,
    },
    sim::Sim,
};

fn dummy(sim: &mut Sim) -> Entity {
    sim.world_mut()
        .query_filtered::<Entity, With<Dummy>>()
        .single(sim.world())
        .expect("one dummy")
}

fn place(sim: &mut Sim, e: Entity, feet: Vec3) {
    sim.world_mut().get_mut::<Transform>(e).unwrap().translation = feet;
}

fn aim_player_at(sim: &mut Sim, point: Vec3) {
    let player = sim.player();
    let eye = sim.feet(player) + Vec3::Y * sim.get::<EyeHeight>(player).0;
    let look = look_toward(point - eye);
    sim.set_look(player, look.yaw, look.pitch);
}

/// Kills the dummy with one rifle hit after draining it to 10 HP.
fn eliminate(sim: &mut Sim, dummy: Entity) -> u64 {
    let feet = Vec3::new(2.0, 0.0, 4.0);
    place(sim, dummy, feet);
    sim.world_mut()
        .get_mut::<Health>(dummy)
        .unwrap()
        .apply(190.0);
    aim_player_at(sim, feet + Vec3::Y * 1.0);
    {
        let mut i = sim.player_intent();
        i.fire = true;
        i.fire_pressed = true;
    }
    sim.tick();
    sim.player_intent().fire = false;
    let kills = sim.recorded::<Eliminated>();
    let kill = kills.last().expect("dummy eliminated");
    assert_eq!(kill.victim, dummy);
    kill.tick
}

#[test]
fn dummy_spawns_at_the_layout_spot_with_full_health_and_shield() {
    let mut sim = Sim::new();
    let d = dummy(&mut sim);
    let layout = sim.world().resource::<ArenaLayout>().clone();
    assert_eq!(sim.feet(d), layout.dummy_spawn);
    assert_eq!(*sim.get::<Health>(d), Health::full(100.0, 100.0));
    assert!(sim.world().get::<Downed>(d).is_none());
}

#[test]
fn dummy_strafes_changes_direction_every_0_4_to_1_6_s_and_sometimes_jumps() {
    let mut sim = Sim::with_seed(7);
    let d = dummy(&mut sim);
    let mut last_sign = 0.0;
    let mut last_change: Option<u64> = None;
    let mut intervals = Vec::new();
    let mut jumps = 0;
    let mut was_jumping = false;
    for _ in 0..(60 * 60) {
        sim.tick();
        let intent = sim.get::<PlayerIntent>(d).clone();
        assert_eq!(intent.move_axis.y, 0.0, "pure strafe");
        assert!((intent.move_axis.x.abs() - 1.0).abs() < 1e-6, "run speed");
        assert!(!intent.sprint);
        let sign = intent.move_axis.x.signum();
        if last_sign != 0.0 && sign != last_sign {
            let now = sim.sim_tick();
            if let Some(prev) = last_change {
                intervals.push((now - prev) as f32 * TICK_SECONDS);
            }
            last_change = Some(now);
        }
        last_sign = sign;
        if intent.jump && !was_jumping {
            jumps += 1;
        }
        was_jumping = intent.jump;
    }
    assert!(
        intervals.len() > 30,
        "changes direction often: {}",
        intervals.len()
    );
    for dt in &intervals {
        assert!(
            (0.4 - 1e-3..=1.6 + TICK_SECONDS).contains(dt),
            "direction held for {dt} s"
        );
    }
    let spread = intervals.iter().cloned().fold(f32::MIN, f32::max)
        - intervals.iter().cloned().fold(f32::MAX, f32::min);
    assert!(spread > 0.6, "intervals are random, not fixed");
    // 0.15 jumps/s over 60 s: about 9.
    assert!((2..=25).contains(&jumps), "{jumps} jumps in a minute");
}

#[test]
fn dummy_faces_the_player() {
    let mut sim = Sim::new();
    let d = dummy(&mut sim);
    let player = sim.player();
    place(&mut sim, d, Vec3::new(-10.0, 0.0, -10.0));
    sim.tick();
    let look = *sim.get::<LookAngles>(d);
    let to_player = (sim.feet(player) - sim.feet(d)).normalize();
    assert!(look.forward().angle_between(to_player) < 0.05);
}

#[test]
fn stand_still_gives_a_zero_intent() {
    let mut sim = Sim::with_seed(3);
    sim.tuning_mut().dummy.stand_still = true;
    let d = dummy(&mut sim);
    for _ in 0..(60 * 20) {
        sim.tick();
        let intent = sim.get::<PlayerIntent>(d);
        assert_eq!(intent.move_axis, Vec2::ZERO);
        assert!(!intent.jump && !intent.sprint && !intent.crouch && !intent.fire);
    }
}

#[test]
fn dummy_turns_back_before_leaving_the_arena() {
    let mut sim = Sim::with_seed(11);
    let d = dummy(&mut sim);
    let layout = sim.world().resource::<ArenaLayout>().clone();
    // Hug the east edge: whatever the pattern says, it must never strafe outward.
    // Movement carries it inward, so pin it back to the edge before every tick.
    for z in [-15.0, 0.0, 10.0] {
        for _ in 0..(60 * 5) {
            place(&mut sim, d, Vec3::new(layout.bounds_max.x, 0.0, z));
            sim.tick();
            let (_, right) = sim.get::<LookAngles>(d).flat_basis();
            let world_dir = right * sim.get::<PlayerIntent>(d).move_axis.x;
            assert!(world_dir.x <= 1e-3, "strafing out of bounds at z {z}");
        }
    }
}

#[test]
fn eliminated_dummy_respawns_after_two_seconds_away_from_the_player() {
    let mut sim = Sim::with_seed(5);
    sim.record::<Eliminated>();
    sim.record::<GameCue>();
    let d = dummy(&mut sim);
    let layout = sim.world().resource::<ArenaLayout>().clone();
    let player = sim.player();
    let mut spots = Vec::new();
    for _ in 0..4 {
        sim.clear_recorded::<GameCue>();
        let killed_at = eliminate(&mut sim, d);
        assert_eq!(
            sim.world().get::<Downed>(d),
            Some(&Downed { tick: killed_at })
        );
        let downed_at = sim.feet(d);

        // Down for exactly 2.0 s (120 ticks), not moving and not trying to.
        sim.ticks(119);
        assert!(sim.world().get::<Downed>(d).is_some());
        assert_eq!(sim.feet(d), downed_at);
        assert_eq!(*sim.get::<PlayerIntent>(d), PlayerIntent::default());
        sim.tick();
        assert_eq!(sim.sim_tick() - killed_at, 120);
        assert!(
            sim.world().get::<Downed>(d).is_none(),
            "respawned after 2.0 s"
        );

        let feet = sim.feet(d);
        assert_eq!(*sim.get::<Health>(d), Health::full(100.0, 100.0));
        assert!(
            feet.xz().distance(sim.feet(player).xz()) >= 12.0,
            "respawned {feet} too close to the player"
        );
        assert!(feet.x >= layout.bounds_min.x && feet.x <= layout.bounds_max.x);
        assert!(feet.z >= layout.bounds_min.y && feet.z <= layout.bounds_max.y);
        assert_eq!(feet.y, 0.0);
        assert!(
            sim.recorded::<GameCue>()
                .contains(&GameCue::Respawned { who: d })
        );
        spots.push(feet);
    }
    assert!(
        spots.windows(2).all(|w| w[0].distance(w[1]) > 0.1),
        "respawn spots vary: {spots:?}"
    );
}

#[test]
fn downed_dummy_cannot_be_hit() {
    let mut sim = Sim::with_seed(9);
    sim.record::<Eliminated>();
    sim.record::<DamageDealt>();
    sim.record::<ShotFired>();
    let d = dummy(&mut sim);
    eliminate(&mut sim, d);
    sim.clear_recorded::<DamageDealt>();
    sim.clear_recorded::<ShotFired>();
    let feet = sim.feet(d);
    for aim in [0.9, 1.62] {
        aim_player_at(&mut sim, feet + Vec3::Y * aim);
        {
            let mut i = sim.player_intent();
            i.fire = true;
            i.fire_pressed = true;
        }
        sim.ticks(30);
        sim.player_intent().fire = false;
    }
    let shots = sim.recorded::<ShotFired>();
    assert!(shots.len() >= 4);
    assert!(sim.recorded::<DamageDealt>().is_empty());
    assert!(shots.iter().all(|s| s.traces[0].hit != Some(d)));
    assert!(sim.get::<Health>(d).is_dead());
}
