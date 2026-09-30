//! The camera's feel (docs/M4-SPEC.md → Chunk 5, D108) through the
//! simulation seam: scripted `PlayerIntent` in (a slide, a 6 m drop, a hit
//! from the right, rifle fire with a turning aim), and out: every shot's ray,
//! the look, the feet, and what the drawn camera did. With every effect at
//! 100% and at 0 the aim ray is bit-identical; the effects stay inside
//! D108's limits (FOV ≤ +4°, dip ≤ 6 cm, tilt 2–3°, nudge ≤ 0.5°).

use bevy::{ecs::message::Messages, prelude::*};
use pieced::{
    camera_feel::{
        CameraFeel, CameraFeelPlugin, DIP_MAX, FOV_KICK_MAX_DEG, NUDGE_MAX_DEG, SLIDE_ROLL_MAX_DEG,
    },
    render::{CameraFollowSet, MainCamera},
    shared::{DamageDealt, DamageTarget, EyeHeight, LookAngles, Player, PreviousFeet, ShotFired},
    sim::Sim,
};

const FOV: f32 = 70.0;

/// `app::headless_app` with `extra` plugins added before it is finished.
fn headless_with(seed: u64, extra: impl FnOnce(&mut App)) -> App {
    use bevy::time::TimeUpdateStrategy;
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        bevy::state::app::StatesPlugin,
        AssetPlugin::default(),
        bevy::mesh::MeshPlugin,
        bevy::scene::ScenePlugin,
        avian3d::prelude::PhysicsPlugins::default(),
    ))
    .add_plugins(pieced::app::SimPlugins)
    .insert_resource(TimeUpdateStrategy::ManualDuration(
        pieced::shared::tick_duration(),
    ))
    .insert_resource(pieced::rng::SimRng(pieced::rng::Rng::new(seed)));
    extra(&mut app);
    app.finish();
    app.cleanup();
    app
}

/// Places the camera at the eye, looking along the look (the game's
/// `render::follow_player_eye`, without interpolation).
fn follow(
    player: Single<(&Transform, &EyeHeight, &LookAngles), With<Player>>,
    mut camera: Single<&mut Transform, (With<MainCamera>, Without<Player>)>,
) {
    let (body, eye, look) = player.into_inner();
    camera.translation = body.translation + Vec3::Y * eye.0;
    camera.rotation = look.rotation();
}

fn rig(effects: f32) -> Sim {
    let mut app = headless_with(5, |app| {
        app.add_plugins(CameraFeelPlugin)
            .add_systems(PostUpdate, follow.in_set(CameraFollowSet));
    });
    app.world_mut().spawn((
        MainCamera,
        Transform::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: FOV.to_radians(),
            ..default()
        }),
    ));
    app.world_mut()
        .resource_mut::<pieced::tuning::Tuning>()
        .feedback
        .camera_effects = effects;
    app.world_mut()
        .resource_mut::<NextState<pieced::shared::AppState>>()
        .set(pieced::shared::AppState::Playing);
    app.update();
    let mut sim = Sim { app };
    pieced::building::clear_pieces(sim.world_mut());
    sim.tick();
    sim
}

/// What a run of the script leaves: every shot (origin and trace ends as
/// bits), the look and feet each tick, and the camera's biggest FOV kick,
/// dip and roll (radians, m, radians) against the look.
struct Take {
    shots: Vec<u32>,
    looks: Vec<u32>,
    feet: Vec<u32>,
    fov: f32,
    dip: f32,
    roll: f32,
    slide_roll: f32,
}

fn script(effects: f32) -> Take {
    let mut sim = rig(effects);
    let p = sim.player();
    let mut reader = sim.world().resource::<Messages<ShotFired>>().get_cursor();
    let mut take = Take {
        shots: Vec::new(),
        looks: Vec::new(),
        feet: Vec::new(),
        fov: 0.0,
        dip: 0.0,
        roll: 0.0,
        slide_roll: 0.0,
    };
    for tick in 0..320u32 {
        {
            let mut i = sim.player_intent();
            i.look_delta = Vec2::new(0.004, -0.0015);
            i.fire = tick < 40 || (200..240).contains(&tick);
            i.fire_pressed = tick == 0 || tick == 200;
            // Sprint forward, then slide.
            i.move_axis = if (40..120).contains(&tick) {
                Vec2::new(0.0, 1.0)
            } else {
                Vec2::ZERO
            };
            i.sprint = (40..120).contains(&tick);
            i.crouch = (70..110).contains(&tick);
            i.crouch_pressed = tick == 70;
        }
        if tick == 130 {
            // Dropped from 6 m.
            let at = sim.feet(p) + Vec3::Y * 6.0;
            sim.world_mut().get_mut::<Transform>(p).unwrap().translation = at;
            sim.world_mut().get_mut::<PreviousFeet>(p).unwrap().0 = at;
        }
        if tick == 190 {
            // A hit from the right.
            let look = *sim.get::<LookAngles>(p);
            let right = look.rotation() * Vec3::X;
            let point = sim.feet(p) + right * 5.0 + Vec3::Y;
            let t = sim.sim_tick();
            sim.world_mut().write_message(DamageDealt {
                source: None,
                target: p,
                target_kind: DamageTarget::Character,
                amount: 5.0,
                to_shield: 5.0,
                headshot: false,
                shield_broke: false,
                killed: false,
                point,
                normal: Vec3::Z,
                tick: t,
            });
        }
        sim.tick();
        let messages = sim.world().resource::<Messages<ShotFired>>();
        for shot in reader.read(messages) {
            take.shots.extend(shot.origin.to_array().map(f32::to_bits));
            for trace in &shot.traces {
                take.shots.extend(trace.end.to_array().map(f32::to_bits));
            }
        }
        let look = *sim.get::<LookAngles>(p);
        take.looks
            .extend([look.yaw.to_bits(), look.pitch.to_bits()]);
        take.feet.extend(sim.feet(p).to_array().map(f32::to_bits));
        let feel = *sim.world().resource::<CameraFeel>();
        let (camera, fov) = {
            let world = sim.world_mut();
            let mut q = world.query_filtered::<(&Transform, &Projection), With<MainCamera>>();
            let (t, proj) = q.single(world).unwrap();
            let fov = match proj {
                Projection::Perspective(p) => p.fov,
                _ => unreachable!(),
            };
            (*t, fov)
        };
        let eye = sim.feet(p) + Vec3::Y * sim.get::<EyeHeight>(p).0;
        take.fov = take.fov.max(fov - FOV.to_radians());
        take.dip = take.dip.max(eye.y - camera.translation.y);
        let rel = look.rotation().inverse() * camera.rotation;
        let angle = 2.0 * rel.xyz().length().min(1.0).asin();
        take.roll = take.roll.max(angle);
        if (85..110).contains(&tick) {
            take.slide_roll = take.slide_roll.max(feel.roll.abs());
        }
    }
    take
}

#[test]
fn the_aim_ray_is_bit_identical_with_every_camera_effect_at_100_and_at_0() {
    let on = script(1.0);
    let off = script(0.0);
    assert!(on.shots.len() > 30, "the rifle fired");
    assert_eq!(on.shots, off.shots, "every shot's ray is bit-identical");
    assert_eq!(on.looks, off.looks, "the look is bit-identical");
    assert_eq!(on.feet, off.feet, "the movement is bit-identical");
    // Off: the camera is exactly the eye.
    assert!(off.fov.abs() < 1e-6 && off.dip.abs() < 1e-6 && off.roll < 1e-5);
    // On: every effect showed, inside D108.
    assert!(
        on.fov > 1.0f32.to_radians() && on.fov <= FOV_KICK_MAX_DEG.to_radians() + 1e-4,
        "FOV kick {}°",
        on.fov.to_degrees()
    );
    assert!(
        on.dip > 0.01 && on.dip <= DIP_MAX + 1e-4,
        "dip {} m",
        on.dip
    );
    assert!(
        on.slide_roll >= 2.0f32.to_radians() * 0.9
            && on.slide_roll <= SLIDE_ROLL_MAX_DEG.to_radians(),
        "slide tilt {}°",
        on.slide_roll.to_degrees()
    );
    assert!(
        on.roll <= (SLIDE_ROLL_MAX_DEG + NUDGE_MAX_DEG).to_radians() + 1e-4,
        "roll {}°",
        on.roll.to_degrees()
    );
}

#[test]
fn a_hit_nudges_the_view_toward_it_by_at_most_half_a_degree() {
    for (side, sign) in [(1.0f32, -1.0f32), (-1.0, 1.0)] {
        let mut sim = rig(1.0);
        let p = sim.player();
        sim.ticks(10);
        let look = *sim.get::<LookAngles>(p);
        let point = sim.feet(p) + look.rotation() * Vec3::X * 4.0 * side + Vec3::Y;
        let t = sim.sim_tick();
        sim.world_mut().write_message(DamageDealt {
            source: None,
            target: p,
            target_kind: DamageTarget::Character,
            amount: 10.0,
            to_shield: 10.0,
            headshot: false,
            shield_broke: false,
            killed: false,
            point,
            normal: Vec3::Z,
            tick: t,
        });
        let mut peak: f32 = 0.0;
        for _ in 0..30 {
            sim.tick();
            let roll = sim.world().resource::<CameraFeel>().roll;
            if roll.abs() > peak.abs() {
                peak = roll;
            }
        }
        assert!(peak.abs() <= NUDGE_MAX_DEG.to_radians() + 1e-6);
        assert!(peak.abs() > 0.3f32.to_radians(), "{}°", peak.to_degrees());
        assert_eq!(peak.signum(), sign, "toward the hit on the {side} side");
        // Settled.
        assert_eq!(sim.world().resource::<CameraFeel>().roll, 0.0);
        // Half the slider, half the nudge.
        let mut half = rig(0.5);
        half.ticks(10);
        let p = half.player();
        let t = half.sim_tick();
        half.world_mut().write_message(DamageDealt {
            source: None,
            target: p,
            target_kind: DamageTarget::Character,
            amount: 10.0,
            to_shield: 10.0,
            headshot: false,
            shield_broke: false,
            killed: false,
            point,
            normal: Vec3::Z,
            tick: t,
        });
        let mut half_peak: f32 = 0.0;
        for _ in 0..30 {
            half.tick();
            half_peak = half_peak.max(half.world().resource::<CameraFeel>().roll.abs());
        }
        assert!((half_peak - peak.abs() / 2.0).abs() < 1e-3);
    }
}
