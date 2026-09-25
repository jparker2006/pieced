//! Slice E — pure logic behind the viewmodel and effects: recoil springs, camera
//! shake, hitstop, effect pools, debris physics, tracer streaks and the
//! viewmodel's poses. (The rendered look is verified by the `fx_check` scenario.)

use bevy::prelude::*;
use pieced::{
    combat::GunTuning,
    fx::{
        FeedbackTuning,
        sim::{Hitstop, Particle, SHAKE_MAX_DEG, Shake, SlotPool, Spring, tracer_segment},
    },
    viewmodel::{
        PUMP_KICK, RIFLE_KICK,
        anim::{
            RACK_BACK, ads_translation, euler, pump_rack, pump_shell, rifle_reload, switch_phase,
        },
        models::{self, PUMP, PUMP_LOADING_PORT, RIFLE},
    },
};

const DT: f32 = 1.0 / 60.0;

/// Kicks a spring from rest and records its displacement every frame.
fn kick_trace(spring: Spring, peak: f32, dt: f32, seconds: f32) -> Vec<(f32, f32)> {
    let (mut x, mut v) = (Vec3::ZERO, Vec3::X * spring.kick_for_peak(peak));
    let mut out = Vec::new();
    let mut t = 0.0;
    while t < seconds {
        spring.step(&mut x, &mut v, Vec3::ZERO, dt);
        t += dt;
        out.push((t, x.x));
    }
    out
}

#[test]
fn rifle_kick_is_small_and_quick() {
    let trace = kick_trace(RIFLE_KICK, 0.018, DT, 0.5);
    let (peak_t, peak) = trace
        .iter()
        .copied()
        .fold((0.0, 0.0), |a, b| if b.1 > a.1 { b } else { a });
    assert!((peak - 0.018).abs() < 0.003, "peak {peak}");
    assert!(peak_t <= 0.05, "peaks after {peak_t}s");
    let settled = trace.iter().find(|(t, _)| *t >= 0.15).unwrap().1;
    assert!(settled.abs() < 0.05 * peak, "still {settled} at 150 ms");
}

#[test]
fn pump_kick_is_bigger_and_settles_before_the_next_shot() {
    let rifle = kick_trace(RIFLE_KICK, 0.018, DT, 1.0);
    let pump = kick_trace(PUMP_KICK, 0.075, DT, 1.0);
    let max = |t: &[(f32, f32)]| t.iter().map(|p| p.1).fold(0.0, f32::max);
    assert!(max(&pump) > 3.0 * max(&rifle));
    let interval = GunTuning::pump().fire_interval;
    let at_next_shot = pump.iter().find(|(t, _)| *t >= interval * 0.5).unwrap().1;
    assert!(at_next_shot.abs() < 0.05 * max(&pump));
}

#[test]
fn springs_do_not_depend_on_frame_rate() {
    let coarse = kick_trace(RIFLE_KICK, 0.02, 1.0 / 30.0, 0.2);
    let fine = kick_trace(RIFLE_KICK, 0.02, 1.0 / 240.0, 0.2);
    let at = |trace: &[(f32, f32)], t: f32| {
        trace
            .iter()
            .min_by(|a, b| (a.0 - t).abs().total_cmp(&(b.0 - t).abs()))
            .unwrap()
            .1
    };
    for t in [0.0667, 0.1333, 0.2] {
        assert!(
            (at(&coarse, t) - at(&fine, t)).abs() < 5e-4,
            "differs at {t}"
        );
    }
    // A very long frame never explodes.
    let (mut x, mut v) = (Vec3::ZERO, Vec3::X * 5.0);
    RIFLE_KICK.step(&mut x, &mut v, Vec3::ZERO, 2.0);
    assert!(x.length() < 1e-3 && v.length() < 1e-2);
}

#[test]
fn camera_shake_is_capped_by_the_setting_and_zero_disables_it() {
    for strength in [0.0, 0.3, 1.0] {
        let mut shake = Shake::default();
        shake.add(5.0);
        let cap = SHAKE_MAX_DEG.to_radians() * strength + 1e-6;
        let mut biggest: f32 = 0.0;
        for _ in 0..60 {
            let a = shake.angles(strength);
            biggest = biggest.max(a.abs().max_element());
            assert!(a.abs().max_element() <= cap, "{a:?} over cap at {strength}");
            shake.step(DT);
        }
        if strength == 0.0 {
            assert_eq!(biggest, 0.0);
        } else {
            assert!(biggest > 0.3 * cap, "shake at {strength} barely moves");
        }
        assert_eq!(
            shake.angles(strength),
            Vec3::ZERO,
            "still shaking after 1 s"
        );
    }
    assert!(FeedbackTuning::default().camera_shake <= 1.0);
}

#[test]
fn hitstop_freezes_exactly_the_requested_frames() {
    let mut hitstop = Hitstop::default();
    assert!(!hitstop.trigger(0), "0 frames never pauses");
    for frames in [1u32, 2, 4] {
        assert!(hitstop.trigger(frames));
        // The triggering frame already advanced; count frozen frames after it.
        assert!(!hitstop.end_frame());
        let mut frozen = 0;
        loop {
            frozen += 1;
            if hitstop.end_frame() {
                break;
            }
            assert!(frozen < 100);
        }
        assert_eq!(frozen, frames);
        assert!(!hitstop.active());
    }
}

#[test]
fn pools_respect_their_cap_and_recycle_the_oldest() {
    let mut pool = SlotPool::new(10);
    let first: Vec<usize> = (0..4).map(|_| pool.alloc(4).unwrap()).collect();
    assert_eq!(pool.live_count(), 4);
    // Full: the oldest (the first slot handed out) is reused.
    assert_eq!(pool.alloc(4), Some(first[0]));
    for _ in 0..50 {
        pool.alloc(4);
        assert!(pool.live_count() <= 4);
    }
    pool.free(first[1]);
    assert!(!pool.is_live(first[1]));
    assert_eq!(pool.alloc(4), Some(first[1]), "free slots are used first");
    assert_eq!(SlotPool::new(0).alloc(10), None);
    assert!(pool.alloc(100).unwrap() < 10);
}

#[test]
fn debris_tumbles_bounces_settles_and_fades() {
    let mut chunk = Particle {
        // Launched from the middle of a wall.
        pos: Vec3::new(0.0, 1.5, 0.0),
        vel: Vec3::new(2.5, 2.0, 0.0),
        spin: Vec3::new(4.0, 7.0, 1.0),
        gravity: 20.0,
        bounce: Some(0.35),
        radius: 0.06,
        life: 1.2,
        shrink_start: 0.72,
        ..default()
    };
    let start_rot = chunk.rot;
    let mut bounced = false;
    let mut was_falling = false;
    let mut t = 0.0;
    while chunk.step(DT) {
        t += DT;
        assert!(chunk.pos.y >= chunk.radius - 1e-4, "sank into the ground");
        if was_falling && chunk.vel.y > 0.0 && chunk.pos.y < 0.2 {
            bounced = true;
        }
        was_falling = chunk.vel.y < 0.0;
        if (0.3..0.7).contains(&t) {
            assert!(chunk.scale().x > 0.99, "shrinking too early");
        }
    }
    assert!(bounced, "never bounced");
    assert!((t - 1.2).abs() < 2.0 * DT, "expired at {t}");
    assert!(chunk.rot.angle_between(start_rot) > 0.1, "never tumbled");
    assert!(chunk.is_resting(), "still moving at {:?}", chunk.vel);
    assert!(chunk.scale().x < 0.1, "didn't shrink away");
}

#[test]
fn tracers_leave_the_muzzle_reach_the_hit_and_fade_within_90ms() {
    let life = 0.085;
    for length in [3.0, 25.0, 150.0] {
        let (tail, head) = tracer_segment(0.0, life, length, 14.0).expect("visible at once");
        assert_eq!(tail, 0.0, "starts at the muzzle");
        assert!(head > 0.0);
        let mut reached = false;
        let mut age = 0.0;
        while let Some((tail, head)) = tracer_segment(age, life, length, 14.0) {
            assert!(0.0 <= tail && tail <= head && head <= length + 1e-4);
            assert!(head - tail <= 14.0_f32.max(0.34 * length) + 1e-3);
            reached |= (head - length).abs() < 1e-3;
            age += 0.004;
        }
        assert!(reached, "never reached the end of a {length} m trace");
        assert!(age <= 0.09, "lasted {age}s");
    }
}

#[test]
fn ads_puts_each_sight_on_the_screen_center() {
    for spec in [RIFLE, PUMP] {
        let rig =
            Transform::from_translation(ads_translation(&spec)).with_rotation(euler(Vec3::ZERO));
        let sight = rig.transform_point(spec.sight);
        assert!(sight.x.abs() < 1e-5 && sight.y.abs() < 1e-5, "{sight:?}");
        assert!((sight.z + spec.ads_distance).abs() < 1e-5);
        // The hip pose keeps the gun low and to the right.
        assert!(spec.hip.x > 0.1 && spec.hip.y < -0.1);
    }
}

#[test]
fn rifle_reload_drops_and_reseats_the_magazine() {
    let start = rifle_reload(0.0);
    assert!(start.mag_visible && start.mag.pos == Vec3::ZERO);
    assert!(
        start.gun.pos.length() < 1e-4,
        "no dip before the reload starts"
    );
    let dropping = rifle_reload(0.28);
    assert!(dropping.mag_visible && dropping.mag.pos.y < -0.1);
    assert!(dropping.gun.euler.z < -0.3, "tilts the magwell toward you");
    assert!(!rifle_reload(0.4).mag_visible, "old magazine gone");
    let inserting = rifle_reload(0.6);
    assert!(inserting.mag_visible && inserting.mag.pos.y < -0.01);
    let end = rifle_reload(1.0);
    assert!(end.mag_visible && end.mag.pos.length() < 1e-5);
    assert!(end.gun.pos.length() < 1e-3 && end.gun.euler.length() < 1e-3);
}

#[test]
fn pump_racks_after_a_shot_and_is_done_before_the_next() {
    assert_eq!(pump_rack(0.0), 0.0, "recoil first, then the rack");
    assert!((pump_rack(RACK_BACK) - 1.0).abs() < 1e-3, "fully back");
    assert_eq!(pump_rack(GunTuning::pump().fire_interval), 0.0);
    let mid = pump_shell(0.55);
    assert!(mid.visible && mid.pos.distance(PUMP_LOADING_PORT) < 1e-4);
    assert!(!pump_shell(0.9).visible, "shell is in the tube");
}

#[test]
fn switching_lowers_the_old_gun_then_raises_the_new_one() {
    assert_eq!(switch_phase(0.0), (true, 0.0));
    let (prev, low) = switch_phase(0.49);
    assert!(prev && low > 0.99);
    let (prev, low) = switch_phase(0.51);
    assert!(!prev && low > 0.99);
    assert_eq!(switch_phase(1.0), (false, 0.0));
}

#[test]
fn gun_models_stay_low_poly() {
    let rifle = models::rifle();
    let rifle_tris = rifle.body.triangles() + rifle.mag.triangles() + rifle.dot.triangles();
    let pump = models::pump();
    let pump_tris = pump.body.triangles() + pump.forend.triangles() + pump.shell.triangles();
    assert!(rifle_tris < 2500, "rifle has {rifle_tris} triangles");
    assert!(pump_tris < 2500, "pump has {pump_tris} triangles");
    assert!(models::muzzle_flash().triangles() < 100);
    // Chunky, but gun-sized: the rifle is about 1.1 m long, the pump about 1.25 m.
    let (min, max) = rifle.body.bounds();
    assert!((1.0..1.25).contains(&(max.z - min.z)));
    let (min, max) = pump.body.bounds();
    assert!((1.1..1.35).contains(&(max.z - min.z)));
}
