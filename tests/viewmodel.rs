//! The first-person guns (docs/M2-SPEC.md → Guns and gloves): aim-down-sights
//! alignment and the hip poses, the crystal ammo glow (as pure math and through
//! the simulation seam: scripted `PlayerIntent` in, `CrystalGlow` out), the
//! rifle's crystal-swap reload, the pump's shards, rack and spinning rings, the
//! squash-and-stretch kick, and the gloves landing on each gun's grips.

use bevy::{
    ecs::{message::Messages, system::RunSystemOnce},
    gltf::GltfPlugin,
    mesh::MeshPlugin,
    prelude::*,
    world_serialization::WorldSerializationPlugin,
};
use pieced::{
    combat::{CombatTuning, GunTuning, Loadout},
    models::{ModelLibrary, ModelParts, ModelSpawned, ModelsPlugin, spawn_model},
    shared::{ActiveTool, WeaponKind},
    sim::Sim,
    viewmodel::{
        ADS_SECONDS, CrystalGlow, CrystalGlowPlugin, WeaponBeat, WeaponCue, WeaponFeelTuning,
        anim::{
            CHAMBER_OPEN_LENGTH, DRAW_SPLIT, FRESH_GLOW, GLOVE_L_FOREARM, GLOW_MIN, GLOW_RAMP,
            HAND_HOLD_OFFSET, PUMP_SQUASH, PUMP_SQUASH_PEAK, RACK_BACK, RACK_DONE, RACK_JOLT,
            RACK_START, RIFLE_GRAB, RIFLE_POP, RIFLE_SLOT, RIFLE_SQUASH, RIFLE_SQUASH_PEAK,
            RING_IDLE_SPEED, RING_RACK_KICK, RingSpin, SEATED_GLOW, SHARD_FETCH, SHARD_MERGED,
            SHARD_PUSH, SHARD_START, Squash, ads_translation, ammo_glow, chamber_transform,
            draw_twist, euler, hand_hold, pump_crystal_glow, pump_grip_offset, pump_rack,
            pump_shard, rack_pose, rifle_crystal_glow, rifle_reload, shard_push_dip, squash_scale,
            squash_transform, switch_phase,
        },
        feel::{AdsBlend, CameraKick, KICK_MAX_DEG, KICK_MAX_SECONDS, RIFLE_RELOAD_BEATS},
        model_to_node,
        models::{
            GLOVES_MODEL, GunSpec, PUMP, PUMP_RACK_TRAVEL, RIFLE, RIFLE_INSPECT, gun_model,
            gun_spec, sidecar,
        },
        node_to_model,
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

/// The gallery-only inspect pose (target T02): the rifle turned well toward
/// the camera from the hip (which points almost straight ahead, S1 round 3),
/// crossing the frame from the crystal low right of centre to the muzzle up
/// and left of it, bigger on screen than at the hip.
#[test]
fn the_inspect_pose_turns_the_rifle_toward_the_camera_across_the_frame() {
    let spec = &*RIFLE;
    let hip = Transform::from_translation(spec.hip).with_rotation(euler(spec.hip_euler));
    let inspect = Transform::from_translation(RIFLE_INSPECT.rig_translation(spec))
        .with_rotation(euler(RIFLE_INSPECT.euler));
    assert!(
        inspect
            .transform_point(spec.socket)
            .distance(RIFLE_INSPECT.anchor)
            < 1e-5,
        "the crystal sits at the pose's anchor"
    );
    let turn = (RIFLE_INSPECT.euler.y - spec.hip_euler.y).to_degrees();
    assert!(
        (35.0..=50.0).contains(&turn),
        "turned {turn}° toward the camera"
    );
    let crystal = ndc(inspect.transform_point(spec.socket));
    let muzzle = ndc(inspect.transform_point(spec.muzzle));
    assert!(
        (0.05..0.6).contains(&crystal.x) && (-0.6..-0.05).contains(&crystal.y),
        "crystal low right of centre: {crystal}"
    );
    assert!(
        (-0.8..-0.2).contains(&muzzle.x) && (0.05..0.6).contains(&muzzle.y),
        "muzzle up and left: {muzzle}"
    );
    let span = |rig: Transform| {
        (ndc(rig.transform_point(spec.muzzle)) - ndc(rig.transform_point(spec.socket))).length()
    };
    assert!(
        span(inspect) > 1.15 * span(hip),
        "bigger on screen: {} vs {} at the hip",
        span(inspect),
        span(hip)
    );
    // Still pointing away into the scene.
    assert!(inspect.transform_point(spec.muzzle).z < inspect.transform_point(spec.socket).z);
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
    // ...goes dark once the glove flicks it out, so spent and fresh read
    // apart...
    assert!(rifle_crystal_glow(6, 30, Some(RIFLE_POP + 0.15), 99.0) < GLOW_MIN * 0.5);
    // ...the fresh one comes up glowing, starts charging as it clicks in...
    assert_eq!(rifle_crystal_glow(6, 30, Some(0.6), 99.0), FRESH_GLOW);
    assert!(rifle_crystal_glow(6, 30, Some(0.9), 99.0) > FRESH_GLOW);
    assert_eq!(rifle_crystal_glow(6, 30, Some(1.0), 99.0), SEATED_GLOW);
    // ...and flashes up to full the moment the reload completes.
    assert_eq!(rifle_crystal_glow(30, 30, None, 0.0), SEATED_GLOW);
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
    sim.run_seconds(0.9); // the fresh one is in the glove, not yet charged
    assert!(loadout(&mut sim).rifle.is_reloading());
    assert_eq!(glow(&sim).rifle, FRESH_GLOW);

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
    let popping = rifle_reload(0.3);
    assert!(!popping.fresh && popping.crystal.visible());
    assert!(
        popping.crystal.offset.y > 0.08,
        "pops up ({})",
        popping.crystal.offset
    );
    assert!(popping.crystal.spin > PI, "spinning away");
    assert!(popping.stance > 0.99, "the chamber is turned toward you");
    assert!(
        !rifle_reload(0.47).crystal.visible(),
        "the old crystal is gone"
    );

    let sliding = rifle_reload(0.58);
    assert!(sliding.fresh && sliding.crystal.visible());
    assert!(
        sliding.crystal.offset.y < -0.02,
        "coming up from below ({})",
        sliding.crystal.offset
    );
    assert!(
        rifle_reload(0.58).chamber_open > 0.9,
        "still open while it comes in"
    );
    let slotting = rifle_reload(RIFLE_SLOT - 0.03);
    assert!(
        slotting.crystal.offset.length() < sliding.crystal.offset.length(),
        "pushed in toward the socket"
    );
    assert!(slotting.chamber_open > 0.9);

    let seated = rifle_reload(0.9);
    assert!(seated.crystal.offset.length() < 1e-4 && (seated.crystal.scale - 1.0).abs() < 1e-3);
    let end = rifle_reload(1.0);
    assert!(end.fresh && end.chamber_open < 1e-4, "the glass is shut");
    assert!(end.stance < 1e-4, "back at the hip");
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

// The glove holds a crystal from below, so the crystal shows above it.
const _: () = assert!(HAND_HOLD_OFFSET.y < -0.03);

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
    // The rifle's glove gets under the crystal and flicks it out, then
    // carries the fresh one in, and is back on the forend at the end.
    assert_eq!(rifle_reload(0.0).hand, 0.0);
    let flick = rifle_reload(0.2);
    assert!(
        flick.hand > 0.99 && flick.hand_at.length() < 0.05,
        "under the crystal"
    );
    let fetching = rifle_reload(RIFLE_GRAB);
    assert!(
        fetching.hand > 0.99 && fetching.hand_at.y < -0.15,
        "down out of view"
    );
    let sliding = rifle_reload(0.6);
    assert!(sliding.hand > 0.99 && sliding.hand_at == sliding.crystal.offset);
    assert_eq!(rifle_reload(0.98).hand, 0.0);
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
        !pump_shard(0.02).crystal.visible(),
        "the hand picks it up first"
    );
    let lifted = pump_shard(0.3);
    assert!(lifted.crystal.visible() && lifted.hand_at == SHARD_START);
    assert_eq!(
        lifted.hand_at, lifted.crystal.offset,
        "the glove carries it over the rings"
    );
    let pushing = pump_shard(0.38);
    let d = pushing.crystal.offset.length();
    assert!(d > 0.0 && d < SHARD_START.length(), "pushed in");
    assert_eq!(pushing.hand_at, pushing.crystal.offset);
    assert!(
        pump_shard(SHARD_PUSH).crystal.offset.length() < 1e-5,
        "home"
    );
    let inside = pump_shard(0.55);
    assert!(inside.crystal.visible() && inside.crystal.offset.length() < 1e-4);
    assert!(pump_shard(SHARD_MERGED).crystal.scale < 0.5, "melting in");
    assert!(!pump_shard(0.9).crystal.visible());
    assert_eq!(
        pump_shard(0.95).hand_at,
        SHARD_FETCH,
        "back down for the next"
    );
    assert_eq!(pump_shard(0.0).hand_at, SHARD_FETCH);
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
    // The gloves model is authored holding the rifle (then lifted onto the
    // ground on its own), so putting its gloves on the rifle's grips must give
    // back exactly the authored nodes, up to that lift (a pure Y shift). This
    // pins how a model-space frame maps onto a glTF part node.
    let (_, rifle_gloves, _) = roots[0];
    let mut lift = None;
    for (name, grip) in [("GloveR", RIFLE.grip_r), ("GloveL", RIFLE.grip_l)] {
        let glove = find(&mut app, rifle_gloves, name);
        let authored = *app.world().get::<Transform>(glove).unwrap();
        let placed = model_to_node(grip);
        assert!(
            placed.rotation.angle_between(authored.rotation) < 1e-3,
            "{name}: placed turned {} rad from the authored node",
            placed.rotation.angle_between(authored.rotation)
        );
        let shift = authored.translation - placed.translation;
        assert!(shift.xz().length() < 1e-3, "{name}: shifted {shift}");
        let dy = *lift.get_or_insert(shift.y);
        assert!(
            (shift.y - dy).abs() < 1e-3,
            "{name}: both gloves share one lift"
        );
    }
    for &(kind, _, gun) in &roots {
        let spec = gun_spec(kind);
        // Every animated part's rest node, mapped to model space, is where the
        // sidecar says (the crystal on its socket, unturned).
        let crystal = find(&mut app, gun, "Crystal");
        let rest = node_to_model(*app.world().get::<Transform>(crystal).unwrap());
        assert!(
            rest.translation.distance(spec.socket) < 1e-3,
            "{kind:?}: crystal node at {} vs socket {}",
            rest.translation,
            spec.socket
        );
        assert!(rest.rotation.angle_between(Quat::IDENTITY) < 1e-3);
        let back = model_to_node(rest);
        let node = *app.world().get::<Transform>(crystal).unwrap();
        assert!(back.translation.distance(node.translation) < 1e-5);
        assert!(back.rotation.angle_between(node.rotation) < 1e-5);
    }
}

// ---------------------------------------------------------------------------
// The rig in the app: put away while paused, the inspect pose on request
// ---------------------------------------------------------------------------

/// The simulation plus the viewmodel, headless (no window, GPU or models).
fn rig_app() -> App {
    use pieced::{
        app::SimPlugins,
        look::{ModelDressed, ToonMaterial, warmup::WarmupState},
        render::{CurrentFov, MainCamera, WorldTarget},
        shared::tick_duration,
        viewmodel::ViewmodelPlugin,
    };
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        bevy::state::app::StatesPlugin,
        AssetPlugin::default(),
        MeshPlugin,
        WorldSerializationPlugin,
        avian3d::prelude::PhysicsPlugins::default(),
        bevy::input::InputPlugin,
    ))
    .add_plugins(SimPlugins)
    .init_asset::<Image>()
    .init_asset::<StandardMaterial>()
    .init_asset::<ToonMaterial>()
    .add_message::<ModelDressed>()
    .init_resource::<WarmupState>()
    .init_resource::<CurrentFov>()
    .insert_resource(WorldTarget {
        image: Handle::default(),
        size: UVec2::new(1280, 800),
    })
    .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        tick_duration(),
    ))
    .add_plugins(ViewmodelPlugin);
    app.world_mut()
        .spawn((MainCamera, Transform::default(), Visibility::default()));
    app.finish();
    app.cleanup();
    app
}

fn rig(app: &mut App) -> (Transform, Visibility) {
    let world = app.world_mut();
    world
        .query::<(&Name, &Transform, &Visibility)>()
        .iter(world)
        .find(|(n, ..)| n.as_str() == "Viewmodel rig")
        .map(|(_, t, v)| (*t, *v))
        .expect("the viewmodel rig")
}

fn set_state(app: &mut App, to: pieced::shared::AppState) {
    app.world_mut()
        .resource_mut::<NextState<pieced::shared::AppState>>()
        .set(to);
    for _ in 0..3 {
        app.update();
    }
}

#[test]
fn the_gun_is_put_away_while_the_pause_menu_is_open_and_back_after() {
    use pieced::shared::AppState;
    let mut app = rig_app();
    set_state(&mut app, AppState::Playing);
    for _ in 0..30 {
        app.update();
    }
    assert_eq!(
        rig(&mut app).1,
        Visibility::Visible,
        "the gun is up in play"
    );
    set_state(&mut app, AppState::Paused);
    assert_eq!(
        rig(&mut app).1,
        Visibility::Hidden,
        "put away in the pause menu"
    );
    set_state(&mut app, AppState::Playing);
    assert_eq!(rig(&mut app).1, Visibility::Visible, "back up on resume");
}

#[test]
fn only_the_gallery_inspect_resource_swaps_the_rifle_into_its_inspect_pose() {
    use pieced::{shared::AppState, viewmodel::ViewmodelInspect};
    let mut app = rig_app();
    set_state(&mut app, AppState::Playing);
    for _ in 0..30 {
        app.update();
    }
    let hip = rig(&mut app).0;
    assert!(
        hip.translation.distance(RIFLE.hip) < 0.02,
        "at the hip: {hip:?}"
    );
    app.world_mut().insert_resource(ViewmodelInspect);
    app.update();
    let inspect = rig(&mut app).0;
    let want = RIFLE_INSPECT.rig_translation(&RIFLE);
    assert!(
        inspect.translation.distance(want) < 0.02,
        "inspect pose: {inspect:?}"
    );
    assert!(
        inspect.rotation.angle_between(euler(RIFLE_INSPECT.euler)) < 0.05,
        "turned toward the camera"
    );
    app.world_mut().remove_resource::<ViewmodelInspect>();
    app.update();
    assert!(
        rig(&mut app).0.translation.distance(RIFLE.hip) < 0.02,
        "back at the hip"
    );
}

// ---------------------------------------------------------------------------
// M4 weapon feel (D106): every animation inside its gameplay time
// ---------------------------------------------------------------------------

#[test]
fn every_animation_finishes_inside_its_gameplay_time() {
    // The rifle reload (2.0 s): every beat before the end, in order, and
    // everything back at rest at the end.
    let beats: Vec<f32> = RIFLE_RELOAD_BEATS.iter().map(|b| b.0).collect();
    assert!(beats.windows(2).all(|w| w[0] < w[1]), "in order: {beats:?}");
    assert!(beats.iter().all(|&b| b > 0.0 && b < 1.0));
    let end = rifle_reload(1.0);
    assert_eq!(end.hand, 0.0, "the glove is back on the forend");
    assert!(end.stance < 1e-6 && end.chamber_open < 1e-6);
    assert!(end.gun.pos.length() < 1e-4 && end.gun.euler.length() < 1e-4);
    assert_eq!(end.crystal.offset, Vec3::ZERO);
    assert!((end.crystal.scale - 1.0).abs() < 1e-4);
    // No jumps anywhere along it: the pose moves smoothly frame to frame.
    let frames = (2.0 * 60.0) as usize;
    let at = |i: usize| rifle_reload(i as f32 / frames as f32);
    for i in 1..=frames {
        let (a, b) = (at(i - 1), at(i));
        assert!(
            (a.hand - b.hand).abs() < 0.15 && (a.stance - b.stance).abs() < 0.15,
            "frame {i}"
        );
        assert!(
            a.hand_at.distance(b.hand_at) < 0.04,
            "hand jumps at frame {i}"
        );
    }

    // Each pump shell (0.5 s): the glove's loop ends where it starts, the
    // push lands before the shard counts, and the dip is over by the end.
    let shell = GunTuning::pump().reload_time;
    assert!((shell - 0.5).abs() < 1e-6);
    assert_eq!(pump_shard(0.0).hand_at, pump_shard(1.0).hand_at);
    const { assert!(SHARD_PUSH < SHARD_MERGED && SHARD_MERGED < 1.0) };
    assert!(!pump_shard(1.0).crystal.visible());
    for p in [0.0, 1.0] {
        let dip = shard_push_dip(p);
        assert!(dip.pos.length() < 1e-5 && dip.euler.length() < 1e-5);
    }
    assert!(
        shard_push_dip(SHARD_PUSH).pos.y < -0.005,
        "the push dips the gun"
    );

    // The rack (before the next pump shot, 0.9 s): pulled, held, slammed,
    // and the clack's jolt settled.
    let interval = GunTuning::pump().fire_interval;
    assert!(RACK_DONE + RACK_JOLT < interval);
    for age in [0.0, RACK_DONE + RACK_JOLT, interval] {
        let pose = rack_pose(age);
        assert!(
            pose.pos.length() < 1e-5 && pose.euler.length() < 1e-5,
            "{age}"
        );
    }
    assert!(pump_rack(RACK_START + 0.5 * (RACK_BACK - RACK_START)) > 0.3);
    assert!(rack_pose(RACK_BACK).euler.x > 0.1, "a big, heavy pull");
    assert!(
        rack_pose(RACK_DONE + 0.5 * RACK_JOLT).pos.z < -0.01,
        "the clack jolts the gun forward"
    );

    // The switch (0.2 s): the old item drops away, the new one rises from
    // below, overshoots a touch and lands exactly at the end.
    assert!(switch_phase(0.1).0 && !switch_phase(DRAW_SPLIT + 0.01).0);
    assert!(switch_phase(DRAW_SPLIT).1 > 0.99, "starts from below");
    let (shown_prev, lowered) = switch_phase(1.0);
    assert!(!shown_prev && lowered.abs() < 1e-6);
    let twist = draw_twist(1.0);
    assert!(twist.euler.length() < 1e-6);
    let overshoot = (0..=100)
        .map(|i| switch_phase(DRAW_SPLIT + (1.0 - DRAW_SPLIT) * i as f32 / 100.0).1)
        .fold(f32::MAX, f32::min);
    assert!(
        (-0.12..-0.02).contains(&overshoot),
        "settles from a small overshoot ({overshoot})"
    );

    // ADS (0.12 s): lands exactly on the sights at the end of the blend,
    // overshooting a little on the way; the swing is back to zero.
    let feel = WeaponFeelTuning::default();
    let mut ads = AdsBlend::default();
    let mut peak: f32 = 0.0;
    let ads_frames = (ADS_SECONDS * 60.0).ceil() as usize;
    for i in 0..ads_frames {
        ads.step(1.0, 1.0 / 60.0, ADS_SECONDS, feel.ads_overshoot);
        peak = peak.max(ads.pose(feel.ads_overshoot));
        if i + 1 < ads_frames {
            assert!(!ads.settled());
        }
    }
    assert!(ads.settled() && ads.pose(feel.ads_overshoot) == 1.0);
    assert_eq!(ads.swing(), 0.0);
    assert!((1.01..1.08).contains(&peak), "a slight overshoot ({peak})");
}

#[test]
fn the_camera_kick_is_small_and_gone_within_120_ms() {
    let feel = WeaponFeelTuning::default();
    assert!(feel.kick_rifle_deg <= KICK_MAX_DEG && feel.kick_pump_deg <= KICK_MAX_DEG);
    assert!(feel.kick_seconds <= KICK_MAX_SECONDS);
    // A full-auto burst at the rifle's rate, then a pump shot: the kick never
    // exceeds one shot's peak (overlaps don't add up) and it's gone the kick
    // time after the last shot.
    let mut kick = CameraKick::default();
    let dt = 1.0 / 240.0;
    let max = KICK_MAX_DEG.to_radians();
    let mut t = 0.0;
    let mut next_shot = 0.0;
    let mut shots = 0;
    let mut biggest: f32 = 0.0;
    while t < 2.0 {
        if t >= next_shot && shots < 12 {
            // Even a tuning past the cap is clamped to it.
            kick.add(1.0f32.to_radians(), Vec2::new(1.0, 0.2));
            shots += 1;
            next_shot += 1.0 / 60.0;
        }
        kick.step(dt);
        biggest = biggest.max(kick.angles(1.0).length());
        t += dt;
    }
    assert!(biggest <= max + 1e-6, "{} deg", biggest.to_degrees());
    assert_eq!(kick.angles(feel.kick_seconds), Vec2::ZERO);
    // One pump shot: snaps up within a frame, then recovers.
    let mut kick = CameraKick::default();
    kick.add(feel.kick_pump_deg.to_radians(), Vec2::X);
    kick.step(1.0 / 60.0);
    let up = kick.angles(feel.kick_seconds);
    assert!(up.x > 0.8 * feel.kick_pump_deg.to_radians() && up.x <= max);
    kick.step(KICK_MAX_SECONDS);
    assert_eq!(kick.angles(feel.kick_seconds), Vec2::ZERO);
}

#[test]
fn weapon_feel_numbers_are_never_saved() {
    // Designer numbers (D117's feel numbers): a saved copy would freeze them.
    let json = serde_json::to_string(&pieced::tuning::Tuning::default()).unwrap();
    assert!(
        !json.contains("kick_rifle_deg"),
        "Tuning::weapons is skipped"
    );
}

#[test]
fn the_guns_stay_within_their_triangle_budget() {
    for kind in GUNS {
        let side = sidecar(gun_model(kind));
        assert!(side.triangles <= 8000, "{kind:?}: {}", side.triangles);
    }
    assert!(sidecar(GLOVES_MODEL).triangles <= 2000);
}

// ---------------------------------------------------------------------------
// M4 weapon feel through the rig: scripted intents in, rig pose and cues out
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
struct Cues(Vec<(u32, WeaponCue)>);

#[derive(Resource, Default)]
struct Frame(u32);

fn record_cues(mut reader: MessageReader<WeaponCue>, frame: Res<Frame>, mut cues: ResMut<Cues>) {
    for cue in reader.read() {
        cues.0.push((frame.0, *cue));
    }
}

fn count_frame(mut frame: ResMut<Frame>) {
    frame.0 += 1;
}

/// Puts the main camera at the player's eye every frame, as the client does
/// (`render::follow_player_eye`), so render-only effects start from it.
fn follow_eye(
    player: Single<(&Transform, &pieced::shared::LookAngles), With<pieced::shared::Player>>,
    mut camera: Single<
        &mut Transform,
        (
            With<pieced::render::MainCamera>,
            Without<pieced::shared::Player>,
        ),
    >,
) {
    let (feet, look) = player.into_inner();
    camera.translation = feet.translation + Vec3::Y * 1.6;
    camera.rotation = look.rotation();
}

/// The rig app in play, with its cues recorded, the camera following the eye,
/// and the gun settled at the hip.
fn playing_rig() -> App {
    use pieced::{render::CameraFollowSet, shared::AppState};
    let mut app = rig_app();
    app.init_resource::<Cues>()
        .init_resource::<Frame>()
        .add_systems(First, count_frame)
        .add_systems(Last, record_cues)
        .add_systems(PostUpdate, follow_eye.in_set(CameraFollowSet));
    set_state(&mut app, AppState::Playing);
    for _ in 0..30 {
        app.update();
    }
    app
}

fn with_player<T: Component<Mutability = bevy::ecs::component::Mutable>, R>(
    app: &mut App,
    f: impl FnOnce(&mut T) -> R,
) -> R {
    let world = app.world_mut();
    let mut q = world.query_filtered::<&mut T, With<pieced::shared::Player>>();
    let mut c = q.single_mut(world).unwrap();
    f(&mut c)
}

fn frame(app: &App) -> u32 {
    app.world().resource::<Frame>().0
}

fn cues(app: &App) -> Vec<(u32, WeaponCue)> {
    app.world().resource::<Cues>().0.clone()
}

fn beats(app: &App, since: u32) -> Vec<(u32, WeaponBeat)> {
    cues(app)
        .into_iter()
        .filter(|(f, _)| *f > since)
        .map(|(f, c)| (f, c.beat))
        .collect()
}

fn press(app: &mut App, f: impl Fn(&mut pieced::shared::PlayerIntent)) {
    with_player::<pieced::shared::PlayerIntent, _>(app, f);
}

#[test]
fn drawing_a_gun_rises_from_below_and_lands_inside_the_switch_time() {
    let mut app = playing_rig();
    assert!(rig(&mut app).0.translation.distance(RIFLE.hip) < 0.02);
    let start = frame(&app);
    press(&mut app, |i| {
        i.select = Some(ActiveTool::Weapon(WeaponKind::Pump))
    });
    let switch_frames = (CombatTuning::default().switch_time * 60.0).round() as usize;
    let mut path = Vec::new();
    for _ in 0..switch_frames {
        app.update();
        path.push(rig(&mut app).0);
    }
    let landed = path.last().unwrap();
    assert!(
        landed.translation.distance(PUMP.hip) < 0.01,
        "on the hip pose when the switch ends: {landed:?}"
    );
    assert!(landed.rotation.angle_between(euler(PUMP.hip_euler)) < 0.02);
    let lowest = path
        .iter()
        .map(|t| t.translation.y)
        .fold(f32::MAX, f32::min);
    assert!(lowest < PUMP.hip.y - 0.15, "rose in from below ({lowest})");
    let highest = path[switch_frames / 2..]
        .iter()
        .map(|t| t.translation.y)
        .fold(f32::MIN, f32::max);
    assert!(
        highest > PUMP.hip.y + 0.01,
        "overshot a touch before settling ({highest} vs {})",
        PUMP.hip.y
    );
    assert_eq!(
        cues(&app)
            .iter()
            .filter(|(f, _)| *f > start)
            .map(|(_, c)| *c)
            .collect::<Vec<_>>(),
        vec![WeaponCue {
            weapon: WeaponKind::Pump,
            beat: WeaponBeat::Draw
        }]
    );
}

#[test]
fn ads_swings_up_with_weight_and_lands_on_the_sights_in_time() {
    let mut app = playing_rig();
    let hip = rig(&mut app).0.translation;
    let sights = ads_translation(&RIFLE);
    let start = frame(&app);
    press(&mut app, |i| i.ads_held = true);
    let ads_frames = (ADS_SECONDS * 60.0).ceil() as usize;
    let mut along: f32 = 0.0;
    for _ in 0..ads_frames {
        app.update();
        let p = rig(&mut app).0.translation;
        along = along.max((p - hip).dot(sights - hip) / (sights - hip).length_squared());
    }
    let (tf, _) = rig(&mut app);
    assert!(
        tf.translation.distance(sights) < 0.002,
        "on the sights by the end of the blend: {tf:?}"
    );
    assert!(tf.rotation.angle_between(Quat::IDENTITY) < 0.01);
    assert!(along > 1.01, "swung a touch past the sights ({along})");
    press(&mut app, |i| i.ads_held = false);
    for _ in 0..ads_frames {
        app.update();
    }
    assert!(rig(&mut app).0.translation.distance(RIFLE.hip) < 0.02);
    assert_eq!(
        beats(&app, start)
            .into_iter()
            .map(|(_, b)| b)
            .collect::<Vec<_>>(),
        vec![WeaponBeat::AdsIn, WeaponBeat::AdsOut]
    );
}

#[test]
fn the_rifle_reload_plays_its_beats_in_order_inside_two_seconds() {
    let mut app = playing_rig();
    with_player::<Loadout, _>(&mut app, |l| l.rifle.ammo = 9);
    let start = frame(&app);
    press(&mut app, |i| i.reload_pressed = true);
    let mut done_at = None;
    let mut rig_at_done = None;
    for _ in 0..(2.0 * 60.0) as u32 + 10 {
        app.update();
        let full = with_player::<Loadout, _>(&mut app, |l| l.rifle.ammo == 30);
        if full && done_at.is_none() {
            done_at = Some(frame(&app));
            rig_at_done = Some(rig(&mut app).0);
        }
    }
    let done_at = done_at.expect("the reload completed");
    assert!(done_at - start <= 122, "{} frames", done_at - start);
    let seen = beats(&app, start);
    assert_eq!(
        seen.iter().map(|(_, b)| *b).collect::<Vec<_>>(),
        vec![
            WeaponBeat::ChamberOpen,
            WeaponBeat::CrystalPop,
            WeaponBeat::CrystalGrab,
            WeaponBeat::CrystalSlot,
            WeaponBeat::ChamberShut,
            WeaponBeat::CrystalCharged,
        ]
    );
    assert!(seen.windows(2).all(|w| w[0].0 < w[1].0), "{seen:?}");
    let slot = seen[3].0 - start;
    assert!(
        (slot as f32 / 120.0 - RIFLE_SLOT).abs() < 0.02,
        "the click lands at {slot} frames"
    );
    assert_eq!(seen[5].0, done_at, "charges up as the reload completes");
    let pose = rig_at_done.unwrap();
    assert!(
        pose.translation.distance(RIFLE.hip) < 0.01,
        "back at the hip as the reload completes: {pose:?}"
    );
    let _ = (RIFLE_POP, RIFLE_GRAB);
}

#[test]
fn the_pump_racks_and_clacks_before_its_next_shot_and_pushes_one_shard_per_shell() {
    let mut app = playing_rig();
    press(&mut app, |i| {
        i.select = Some(ActiveTool::Weapon(WeaponKind::Pump))
    });
    for _ in 0..40 {
        app.update();
    }
    let start = frame(&app);
    press(&mut app, |i| {
        i.fire = true;
        i.fire_pressed = true;
    });
    app.update();
    press(&mut app, |i| i.fire = false);
    let interval = (GunTuning::pump().fire_interval * 60.0) as u32;
    for _ in 0..interval {
        app.update();
    }
    let rack = beats(&app, start);
    assert_eq!(
        rack.iter().map(|(_, b)| *b).collect::<Vec<_>>(),
        vec![WeaponBeat::RackPull, WeaponBeat::RackClack]
    );
    assert!(
        rack[1].0 - start < interval,
        "the clack before the next shot"
    );

    // Reload three shells: one shard pushed per shell, half a second apart.
    with_player::<Loadout, _>(&mut app, |l| l.pump.ammo = 2);
    let start = frame(&app);
    press(&mut app, |i| i.reload_pressed = true);
    for _ in 0..(3 * 30 + 10) {
        app.update();
    }
    assert_eq!(with_player::<Loadout, _>(&mut app, |l| l.pump.ammo), 5);
    let pushes: Vec<u32> = beats(&app, start)
        .into_iter()
        .filter(|(_, b)| *b == WeaponBeat::ShardPush)
        .map(|(f, _)| f - start)
        .collect();
    assert_eq!(pushes.len(), 3, "{pushes:?}");
    assert!(
        pushes.windows(2).all(|w| (w[1] - w[0]).abs_diff(30) <= 1),
        "one per 0.5 s shell: {pushes:?}"
    );
    assert!(
        pushes
            .iter()
            .enumerate()
            .all(|(i, &f)| f < 30 * (i as u32 + 1) + 1),
        "each inside its own shell: {pushes:?}"
    );
}

/// Every shot the player fires over a scripted fight (origin and every
/// trace's end, as bits), the look each tick, and the biggest angle between
/// the rendered camera and the look.
fn fight(kick: bool) -> (Vec<u32>, Vec<u32>, f32, f32) {
    let mut app = playing_rig();
    if !kick {
        let mut tuning = app.world_mut().resource_mut::<pieced::tuning::Tuning>();
        tuning.weapons.kick_rifle_deg = 0.0;
        tuning.weapons.kick_pump_deg = 0.0;
    }
    app.world_mut()
        .resource_mut::<Messages<pieced::shared::ShotFired>>()
        .clear();
    let mut shots = Vec::new();
    let mut looks = Vec::new();
    let mut biggest: f32 = 0.0;
    let mut settled: f32 = 0.0;
    let mut reader = app
        .world()
        .resource::<Messages<pieced::shared::ShotFired>>()
        .get_cursor();
    for tick in 0..150u32 {
        press(&mut app, |i| {
            // Hold the rifle's trigger with a turning aim, then swap to the
            // pump and fire it.
            i.fire = tick < 50 || tick == 100;
            i.fire_pressed = tick == 0 || tick == 100;
            i.look_delta = Vec2::new(0.8, -0.3);
            if tick == 60 {
                i.select = Some(ActiveTool::Weapon(WeaponKind::Pump));
            }
        });
        app.update();
        let messages = app
            .world()
            .resource::<Messages<pieced::shared::ShotFired>>();
        for shot in reader.read(messages) {
            shots.extend(shot.origin.to_array().map(f32::to_bits));
            for trace in &shot.traces {
                shots.extend(trace.end.to_array().map(f32::to_bits));
            }
        }
        let look = with_player::<pieced::shared::LookAngles, _>(&mut app, |l| *l);
        looks.extend([look.yaw.to_bits(), look.pitch.to_bits()]);
        let camera = {
            let world = app.world_mut();
            let mut q = world.query_filtered::<&Transform, With<pieced::render::MainCamera>>();
            *q.single(world).unwrap()
        };
        // 2·asin of the relative rotation's vector part: exact for tiny
        // angles, where `angle_between`'s acos rounds to ~1e-4.
        let rel = look.rotation().inverse() * camera.rotation;
        let angle = 2.0 * rel.xyz().length().min(1.0).asin();
        biggest = biggest.max(angle);
        if tick >= 110 {
            settled = settled.max(angle);
        }
    }
    (shots, looks, biggest, settled)
}

#[test]
fn the_camera_kick_never_changes_where_a_shot_goes() {
    let (shots, looks, kicked, settled) = fight(true);
    let (shots_still, looks_still, still, _) = fight(false);
    assert!(shots.len() > 40, "the rifle and the pump both fired");
    assert_eq!(shots, shots_still, "every shot's ray is bit-identical");
    assert_eq!(looks, looks_still, "the aim is bit-identical");
    assert!(still < 1e-6, "no kick when it's tuned off");
    assert!(
        kicked > 0.05f32.to_radians() && kicked <= (KICK_MAX_DEG + 0.01).to_radians(),
        "the rendered camera kicks, a fraction of a degree ({}°)",
        kicked.to_degrees()
    );
    assert!(
        settled < 1e-5,
        "and recovers within 120 ms of the last shot"
    );
}

#[test]
fn breathing_sways_the_idle_gun_and_the_sway_switch_stops_it() {
    let mut app = playing_rig();
    let spread = |app: &mut App| {
        let mut lo = Vec3::splat(f32::MAX);
        let mut hi = Vec3::splat(f32::MIN);
        for _ in 0..(4.5 * 60.0) as usize {
            app.update();
            let p = rig(app).0.translation;
            lo = lo.min(p);
            hi = hi.max(p);
        }
        (hi - lo).length()
    };
    let alive = spread(&mut app);
    assert!(
        (0.001..0.01).contains(&alive),
        "the idle gun breathes, millimetres ({alive})"
    );
    app.world_mut()
        .resource_mut::<pieced::tuning::Tuning>()
        .feedback
        .viewmodel_sway = false;
    for _ in 0..60 {
        app.update();
    }
    let still = spread(&mut app);
    assert!(still < 1e-5, "sway off: the gun holds still ({still})");
}
