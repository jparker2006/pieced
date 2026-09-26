//! The first-person guns (docs/M2-SPEC.md → Guns and gloves): aim-down-sights
//! alignment and the hip poses, the crystal ammo glow (as pure math and through
//! the simulation seam: scripted `PlayerIntent` in, `CrystalGlow` out), the
//! rifle's crystal-swap reload, the pump's shards, rack and spinning rings, the
//! squash-and-stretch kick, and the gloves landing on each gun's grips.

use bevy::{
    ecs::system::RunSystemOnce, gltf::GltfPlugin, mesh::MeshPlugin, prelude::*,
    world_serialization::WorldSerializationPlugin,
};
use pieced::{
    combat::{GunTuning, Loadout},
    models::{ModelLibrary, ModelParts, ModelSpawned, ModelsPlugin, spawn_model},
    shared::{ActiveTool, WeaponKind},
    sim::Sim,
    viewmodel::{
        CrystalGlow, CrystalGlowPlugin,
        anim::{
            CHAMBER_OPEN_LENGTH, GLOVE_L_FOREARM, GLOW_MIN, GLOW_RAMP, HAND_HOLD_OFFSET,
            PUMP_SQUASH, PUMP_SQUASH_PEAK, RACK_BACK, RACK_DONE, RACK_START, RIFLE_SQUASH,
            RIFLE_SQUASH_PEAK, RING_IDLE_SPEED, RING_RACK_KICK, RingSpin, SHARD_MERGED,
            SHARD_START, Squash, ads_translation, ammo_glow, chamber_transform, euler, hand_hold,
            pump_crystal_glow, pump_grip_offset, pump_rack, pump_shard, rifle_crystal_glow,
            rifle_reload, squash_scale, squash_transform,
        },
        model_to_node,
        models::{GLOVES_MODEL, GunSpec, PUMP, PUMP_RACK_TRAVEL, RIFLE, gun_model, gun_spec},
    },
};
use std::{f32::consts::PI, time::Duration};

const GUNS: [WeaponKind; 2] = [WeaponKind::Rifle, WeaponKind::Pump];

fn specs() -> [&'static GunSpec; 2] {
    [&RIFLE, &PUMP]
}

// ---------------------------------------------------------------------------
// Poses
// ---------------------------------------------------------------------------

#[test]
fn ads_lines_up_both_iron_sights_on_the_screen_centre() {
    for spec in specs() {
        let rig =
            Transform::from_translation(ads_translation(spec)).with_rotation(euler(Vec3::ZERO));
        let rear = rig.transform_point(spec.sight);
        let front = rig.transform_point(spec.sight_front);
        assert!(rear.xy().length() < 1e-5, "rear sight {rear}");
        assert!((rear.z + spec.ads_distance).abs() < 1e-5);
        assert!(
            front.xy().length() < 1e-3,
            "front sight {front} on the same line"
        );
        assert!(front.z < rear.z - 0.3, "front sight ahead");
        let muzzle = rig.transform_point(spec.muzzle);
        assert!(muzzle.y < -0.05, "the bore sits below the sight line");
    }
}

/// Screen position (-1..1 both ways) in the 58° viewmodel camera at 16:9.
fn ndc(p: Vec3) -> Vec2 {
    let tan = (58f32.to_radians() / 2.0).tan();
    Vec2::new(p.x / (-p.z * tan * 16.0 / 9.0), p.y / (-p.z * tan))
}

#[test]
fn hip_poses_hold_the_gun_low_right_with_the_crystal_in_view() {
    for (kind, spec) in GUNS.into_iter().zip(specs()) {
        let rig = Transform::from_translation(spec.hip).with_rotation(euler(spec.hip_euler));
        let crystal = ndc(rig.transform_point(spec.socket));
        assert!(
            (0.2..0.95).contains(&crystal.x) && (-0.95..-0.2).contains(&crystal.y),
            "{kind:?}: crystal at {crystal} should be on screen, lower right"
        );
        let muzzle_p = rig.transform_point(spec.muzzle);
        let muzzle = ndc(muzzle_p);
        assert!(
            muzzle.abs().max_element() < 0.9 && muzzle.x > 0.0,
            "{kind:?}: muzzle on screen right of centre ({muzzle})"
        );
        assert!(
            muzzle_p.z < rig.transform_point(spec.socket).z,
            "points away"
        );
        let forward = rig.rotation * Vec3::NEG_Z;
        assert!(forward.dot(Vec3::NEG_Z) > 0.9, "{kind:?}: points ahead");
    }
}

// ---------------------------------------------------------------------------
// Crystal ammo glow
// ---------------------------------------------------------------------------

#[test]
fn crystal_glow_follows_the_magazine_fraction() {
    assert_eq!(ammo_glow(30, 30), 1.0);
    assert_eq!(ammo_glow(0, 30), GLOW_MIN);
    assert!((ammo_glow(15, 30) - 0.625).abs() < 1e-6);
    assert!((ammo_glow(2, 5) - 0.55).abs() < 1e-6);
    // The rifle: the dim crystal keeps its glow as it pops out...
    assert_eq!(rifle_crystal_glow(6, 30, Some(0.2), 99.0), ammo_glow(6, 30));
    // ...the fresh one slides in dark...
    assert_eq!(rifle_crystal_glow(6, 30, Some(0.6), 99.0), GLOW_MIN);
    assert_eq!(rifle_crystal_glow(6, 30, Some(1.0), 99.0), GLOW_MIN);
    // ...and charges up the moment the reload completes, with a flash.
    assert_eq!(rifle_crystal_glow(30, 30, None, 0.0), GLOW_MIN);
    let ramp: Vec<f32> = (0..=30)
        .map(|i| rifle_crystal_glow(30, 30, None, GLOW_RAMP * i as f32 / 30.0))
        .collect();
    assert!(ramp.windows(2).take(15).all(|w| w[1] >= w[0]), "charges up");
    assert!(ramp.iter().any(|&g| g > 1.05), "flashes past full");
    assert_eq!(rifle_crystal_glow(30, 30, None, GLOW_RAMP), 1.0);
    // The pump: a shard counts once it has melted into the crystal.
    assert_eq!(pump_crystal_glow(2, 5, Some(0.3)), ammo_glow(2, 5));
    assert_eq!(pump_crystal_glow(2, 5, Some(SHARD_MERGED)), ammo_glow(3, 5));
    assert_eq!(pump_crystal_glow(5, 5, Some(0.9)), 1.0);
}

fn glow(sim: &Sim) -> CrystalGlow {
    *sim.world().resource::<CrystalGlow>()
}

fn loadout(sim: &mut Sim) -> Loadout {
    let player = sim.player();
    sim.get::<Loadout>(player).clone()
}

fn fire(sim: &mut Sim, held: bool) {
    let mut intent = sim.player_intent();
    intent.fire = held;
    intent.fire_pressed = held;
}

#[test]
fn rifle_crystal_dims_as_you_fire_and_recharges_after_a_reload() {
    let mut sim = Sim::with_seed(21);
    sim.tuning_mut().dummy.stand_still = true;
    CrystalGlowPlugin::install(&mut sim.app);
    sim.tick();
    assert_eq!(glow(&sim).rifle, 1.0, "full magazine, full glow");

    fire(&mut sim, true);
    sim.ticks(10 * 12 + 1);
    fire(&mut sim, false);
    sim.tick();
    let ammo = loadout(&mut sim).rifle.ammo;
    assert!(ammo < 30 && ammo > 10, "fired some ({ammo} left)");
    assert!((glow(&sim).rifle - ammo_glow(ammo, 30)).abs() < 1e-5);

    sim.run_seconds(0.5);
    sim.player_intent().reload_pressed = true;
    sim.tick();
    sim.player_intent().reload_pressed = false;
    assert!(loadout(&mut sim).rifle.is_reloading());
    sim.run_seconds(0.4); // the old crystal is popping out
    assert!((glow(&sim).rifle - ammo_glow(ammo, 30)).abs() < 1e-5);
    sim.run_seconds(0.9); // the fresh one is in, not yet charged
    assert!(loadout(&mut sim).rifle.is_reloading());
    assert_eq!(glow(&sim).rifle, GLOW_MIN);

    let mut peak: f32 = 0.0;
    let mut charged_after = None;
    for tick in 0..120 {
        sim.tick();
        let g = glow(&sim).rifle;
        peak = peak.max(g);
        if !loadout(&mut sim).rifle.is_reloading() && g == 1.0 && charged_after.is_none() {
            charged_after = Some(tick);
        }
    }
    assert_eq!(loadout(&mut sim).rifle.ammo, 30);
    assert!(peak > 1.0, "a flash as the fresh crystal charges");
    assert!(charged_after.is_some(), "fully charged after the reload");
}

#[test]
fn pump_crystal_brightens_shard_by_shard() {
    let mut sim = Sim::with_seed(22);
    sim.tuning_mut().dummy.stand_still = true;
    CrystalGlowPlugin::install(&mut sim.app);
    sim.player_intent().select = Some(ActiveTool::Weapon(WeaponKind::Pump));
    sim.tick();
    sim.run_seconds(0.5);
    for _ in 0..3 {
        fire(&mut sim, true);
        sim.tick();
        fire(&mut sim, false);
        sim.run_seconds(1.0);
    }
    let ammo = loadout(&mut sim).pump.ammo;
    assert_eq!(ammo, 2);
    assert!((glow(&sim).pump - ammo_glow(2, 5)).abs() < 1e-5);
    sim.player_intent().reload_pressed = true;
    sim.tick();
    sim.player_intent().reload_pressed = false;
    let mut seen = vec![glow(&sim).pump];
    for _ in 0..(3 * 30 + 10) {
        sim.tick();
        let g = glow(&sim).pump;
        if (g - seen[seen.len() - 1]).abs() > 1e-4 {
            seen.push(g);
        }
    }
    assert_eq!(loadout(&mut sim).pump.ammo, 5);
    assert_eq!(
        seen,
        vec![ammo_glow(2, 5), ammo_glow(3, 5), ammo_glow(4, 5), 1.0],
        "one step per shard"
    );
}

// ---------------------------------------------------------------------------
// The rifle's reload: the crystal swap
// ---------------------------------------------------------------------------

#[test]
fn rifle_reload_pops_the_crystal_out_and_slides_a_fresh_one_in() {
    let start = rifle_reload(0.0);
    assert_eq!(start.crystal.offset, Vec3::ZERO);
    assert!(start.chamber_open < 1e-4 && !start.fresh);
    assert!(
        start.gun.pos.length() < 1e-4,
        "no stance before the reload starts"
    );

    assert!(
        rifle_reload(0.17).chamber_open > 0.9,
        "the glass slides open"
    );
    let popping = rifle_reload(0.28);
    assert!(!popping.fresh && popping.crystal.visible());
    assert!(
        popping.crystal.offset.y > 0.08,
        "pops up ({})",
        popping.crystal.offset
    );
    assert!(popping.crystal.spin > PI, "spinning away");
    assert!(popping.gun.euler.z > 0.2, "the chamber rolls toward you");
    assert!(
        !rifle_reload(0.44).crystal.visible(),
        "the old crystal is gone"
    );

    let sliding = rifle_reload(0.58);
    assert!(sliding.fresh && sliding.crystal.visible());
    assert!(sliding.crystal.offset.y > 0.01, "coming in from above");
    assert!(
        rifle_reload(0.58).chamber_open > 0.9,
        "still open while it slides in"
    );

    let seated = rifle_reload(0.74);
    assert!(seated.crystal.offset.length() < 1e-4 && (seated.crystal.scale - 1.0).abs() < 1e-3);
    let end = rifle_reload(1.0);
    assert!(end.fresh && end.chamber_open < 1e-4, "the glass is shut");
    assert!(end.gun.pos.length() < 1e-3 && end.gun.euler.length() < 1e-3);

    // The glass slides into the front collar: its front end stays put.
    let rest = Transform::from_xyz(0.0, 0.27, -0.005);
    let half = 0.109;
    let open = chamber_transform(rest, half, 1.0);
    assert!((open.scale.z - CHAMBER_OPEN_LENGTH).abs() < 1e-6);
    let front = |t: Transform| t.translation.z - half * t.scale.z;
    assert!((front(open) - front(rest)).abs() < 1e-6);
    assert_eq!(chamber_transform(rest, half, 0.0), rest);
}

#[test]
fn the_left_glove_carries_the_fresh_crystal_in() {
    for spec in specs() {
        let at = spec.socket + Vec3::new(-0.05, 0.15, 0.02);
        let start = hand_hold(0.0, spec.grip_l, at);
        assert!(start.translation.distance(spec.grip_l.translation) < 1e-5);
        assert!(start.rotation.angle_between(spec.grip_l.rotation) < 1e-4);
        let end = hand_hold(1.0, spec.grip_l, at);
        assert!(
            end.translation.distance(at + HAND_HOLD_OFFSET) < 1e-5,
            "holds the crystal"
        );
        assert!(
            HAND_HOLD_OFFSET.y < -0.03,
            "from below, so the crystal shows"
        );
        // Its forearm reaches back toward you on your side, not up into the sky.
        let arm = end.rotation * spec.grip_l.rotation.inverse() * GLOVE_L_FOREARM;
        assert!(arm.z > 0.4 && arm.x < -0.4 && arm.y < 0.1, "forearm {arm}");
        // On the way it swings out on your side, clear of the gun.
        let mid = hand_hold(0.5, spec.grip_l, at).translation;
        let straight = spec.grip_l.translation.lerp(at + HAND_HOLD_OFFSET, 0.5);
        assert!(
            mid.x < straight.x - 0.05 && mid.x < spec.socket.x - 0.08,
            "{mid}"
        );
    }
    // The rifle's glove carries the fresh crystal as it slides in.
    assert_eq!(rifle_reload(0.2).hand, 0.0);
    let sliding = rifle_reload(0.6);
    assert!(sliding.hand > 0.99 && sliding.hand_at == sliding.crystal.offset);
    assert_eq!(rifle_reload(0.95).hand, 0.0);
}

// ---------------------------------------------------------------------------
// The pump: rack, rings and shards
// ---------------------------------------------------------------------------

#[test]
fn pump_racks_and_its_rings_whirr_before_the_next_shot() {
    assert_eq!(pump_rack(0.0), 0.0, "recoil first, then the rack");
    assert!((pump_rack(RACK_BACK) - 1.0).abs() < 1e-3, "fully back");
    assert!(RACK_DONE < GunTuning::pump().fire_interval);
    assert_eq!(pump_rack(GunTuning::pump().fire_interval), 0.0);
    assert!((pump_grip_offset(1.0).z - PUMP_RACK_TRAVEL).abs() < 1e-6);
    assert!(pump_grip_offset(1.0).z > 0.0, "the grip slides back");

    // At rest the rings turn slowly...
    let mut rings = RingSpin::default();
    for _ in 0..60 {
        rings.step(1.0 / 60.0);
    }
    assert!((rings.angle - RING_IDLE_SPEED).abs() < 1e-3);
    // ...and whirr through most of a turn on the rack.
    let mut t = 0.0;
    let mut turned = 0.0;
    while t < GunTuning::pump().fire_interval {
        let prev = t;
        t += 1.0 / 60.0;
        if prev < RACK_START && t >= RACK_START {
            rings.kick(RING_RACK_KICK);
        }
        let a = rings.angle;
        rings.step(1.0 / 60.0);
        turned += (rings.angle - a).rem_euclid(std::f32::consts::TAU);
    }
    assert!(turned > PI * 1.5, "turned {turned} rad");
    assert!(
        rings.speed < RING_IDLE_SPEED + 2.0,
        "settles back toward idle"
    );
}

#[test]
fn each_shell_is_a_shard_pushed_into_the_rings() {
    assert!(
        !pump_shard(0.05).crystal.visible(),
        "the hand fetches it first"
    );
    let rising = pump_shard(0.3);
    assert!(rising.crystal.visible());
    assert_eq!(
        rising.hand_at, rising.crystal.offset,
        "the glove carries it in"
    );
    let d = rising.crystal.offset.length();
    assert!(d > 0.0 && d < SHARD_START.length(), "on its way in");
    let inside = pump_shard(0.55);
    assert!(inside.crystal.visible() && inside.crystal.offset.length() < 1e-4);
    assert!(pump_shard(SHARD_MERGED).crystal.scale < 0.5, "melting in");
    assert!(!pump_shard(0.9).crystal.visible());
    assert_eq!(
        pump_shard(0.95).hand_at,
        SHARD_START,
        "back up for the next"
    );
    assert_eq!(pump_shard(0.0).hand_at, SHARD_START);
}

// ---------------------------------------------------------------------------
// Squash and stretch
// ---------------------------------------------------------------------------

fn squash_trace(s: Squash, peak: f32, dt: f32, seconds: f32) -> Vec<f32> {
    let (mut x, mut v) = (0.0, s.kick_for_peak(peak));
    let mut out = Vec::new();
    let mut t = 0.0;
    while t < seconds {
        s.step(&mut x, &mut v, dt);
        t += dt;
        out.push(x);
    }
    out
}

#[test]
fn every_shot_squashes_then_stretches_and_settles() {
    for (s, peak, settle) in [
        (RIFLE_SQUASH, RIFLE_SQUASH_PEAK, 0.25),
        (PUMP_SQUASH, PUMP_SQUASH_PEAK, 0.5),
    ] {
        let trace = squash_trace(s, peak, 1.0 / 60.0, 1.0);
        let max = trace.iter().cloned().fold(f32::MIN, f32::max);
        let min = trace.iter().cloned().fold(f32::MAX, f32::min);
        assert!(
            (max - peak).abs() < 0.2 * peak,
            "squash peak {max} vs {peak}"
        );
        assert!(min < -0.15 * peak, "stretches past rest ({min})");
        let settled = &trace[(settle * 60.0) as usize..];
        assert!(
            settled.iter().all(|x| x.abs() < 0.2 * peak),
            "settles by {settle} s"
        );
        // The same bounce at 30 and 240 fps.
        let coarse = squash_trace(s, peak, 1.0 / 30.0, 0.2);
        let fine = squash_trace(s, peak, 1.0 / 240.0, 0.2);
        assert!((coarse.last().unwrap() - fine.last().unwrap()).abs() < 0.1 * peak);
    }
    // Squash shortens the gun along the barrel, bulges it, keeps its volume,
    // and pivots on the right hand.
    let scale = squash_scale(0.1);
    assert!((scale.z - 0.9).abs() < 1e-6 && scale.x > 1.0 && scale.x == scale.y);
    assert!((scale.x * scale.y * scale.z - 1.0).abs() < 1e-4);
    let pivot = RIFLE.grip_r.translation;
    let t = squash_transform(0.1, pivot);
    assert!(t.transform_point(pivot).distance(pivot) < 1e-6);
    let muzzle = t.transform_point(RIFLE.muzzle);
    assert!(
        muzzle.z > RIFLE.muzzle.z,
        "the muzzle comes back toward the hand"
    );
}

// ---------------------------------------------------------------------------
// Gloves on the grips (the real glTF hierarchy, headless)
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
struct Spawned(Vec<ModelSpawned>);

fn record(mut reader: MessageReader<ModelSpawned>, mut seen: ResMut<Spawned>) {
    seen.0.extend(reader.read().cloned());
}

fn loader_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        TransformPlugin,
        MeshPlugin,
        GltfPlugin::default(),
        WorldSerializationPlugin,
        ModelsPlugin,
    ))
    .init_resource::<Spawned>()
    .add_systems(Update, record);
    app.finish();
    app.cleanup();
    app
}

fn update_until(app: &mut App, what: &str, done: impl Fn(&mut App) -> bool) {
    for _ in 0..2000 {
        app.update();
        if done(app) {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("timed out waiting for {what}");
}

fn find(app: &mut App, root: Entity, name: &'static str) -> Entity {
    app.world_mut()
        .run_system_once(move |parts: ModelParts| parts.find(root, name))
        .unwrap()
        .unwrap_or_else(|| panic!("no part {name}"))
}

#[test]
fn gloves_land_on_each_guns_grips_and_parts_on_their_sockets() {
    let mut app = loader_app();
    update_until(&mut app, "the model library", |app| {
        app.world()
            .get_resource::<ModelLibrary>()
            .is_some_and(|l| l.is_ready())
    });
    let roots = app
        .world_mut()
        .run_system_once(|mut commands: Commands, library: Res<ModelLibrary>| {
            GUNS.map(|kind| {
                let mut spawn =
                    |name| spawn_model(&mut commands, &library, name, Transform::IDENTITY).unwrap();
                (kind, spawn(GLOVES_MODEL), spawn(gun_model(kind)))
            })
        })
        .unwrap();
    update_until(&mut app, "the models to spawn", |app| {
        app.world().resource::<Spawned>().0.len() >= 4
    });
    for &(kind, gloves, gun) in &roots {
        let spec = gun_spec(kind);
        // Rest poses: every animated part's node, mapped back, is where the
        // sidecar says (the crystal on its socket).
        let crystal = find(&mut app, gun, "Crystal");
        let rest = *app.world().get::<Transform>(crystal).unwrap();
        let rest_model = model_to_node(rest); // the half turn is its own inverse
        assert!(
            rest_model.translation.distance(spec.socket) < 1e-3,
            "{kind:?}: crystal node at {} vs socket {}",
            rest_model.translation,
            spec.socket
        );
        // Put the gloves on the grips as the viewmodel does.
        for (name, grip) in [("GloveR", spec.grip_r), ("GloveL", spec.grip_l)] {
            let glove = find(&mut app, gloves, name);
            *app.world_mut().get_mut::<Transform>(glove).unwrap() = model_to_node(grip);
        }
    }
    app.update();
    for &(kind, gloves, _) in &roots {
        let spec = gun_spec(kind);
        for (name, grip) in [("GloveR", spec.grip_r), ("GloveL", spec.grip_l)] {
            let glove = find(&mut app, gloves, name);
            let world = app.world().get::<GlobalTransform>(glove).unwrap();
            let (_, rotation, translation) = world.to_scale_rotation_translation();
            assert!(
                translation.distance(grip.translation) < 1e-4,
                "{kind:?} {name}: {translation} vs grip {}",
                grip.translation
            );
            assert!(
                rotation.angle_between(grip.rotation) < 1e-3,
                "{kind:?} {name}: turned like its grip"
            );
        }
    }
}
