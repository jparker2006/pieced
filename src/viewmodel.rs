//! Slice E — the first-person gun models, drawn by a separate viewmodel camera.
//!
//! A second 3D camera (a child of the main camera) renders only
//! [`VIEWMODEL_LAYER`] into the same [`WorldTarget`] after the world, with its
//! own depth and a fixed FOV, so the guns never clip into walls and don't warp
//! when the world FOV zooms. The layer has its own shadowless light that copies
//! the sun's direction, warmth and ambient.
//!
//! Every frame (PostUpdate, after the camera follows the eye) the rig's pose is
//! composed from the hip/ADS pose, look and movement sway, walk bob, spring
//! recoil, reload and switch animations, and the muzzle's world position is
//! published in [`MuzzlePoint`] for tracers.

pub mod anim;
pub mod mesh;
pub mod models;

use crate::{
    combat::Loadout,
    fx::sim::{FxRng, Spring},
    movement::Motor,
    palette,
    render::{
        CameraFollowSet, CurrentFov, MainCamera, VIEWMODEL_CAMERA_ORDER, VIEWMODEL_LAYER,
        WorldTarget,
    },
    shared::{ActiveTool, Ads, GameCue, LookAngles, PieceKind, Player, ShotFired, WeaponKind},
    tuning::Tuning,
};
use anim::{
    LOWERED, PUMP_RELOAD_STANCE, PoseOffset, ads_translation, euler, forend_offset, pump_rack,
    pump_shell, rifle_reload, smoothstep, switch_phase,
};
use bevy::{
    camera::{ClearColorConfig, RenderTarget, visibility::RenderLayers},
    light::{AmbientLight, GlobalAmbientLight, NotShadowCaster, NotShadowReceiver},
    prelude::*,
};
use models::{GunSpec, PUMP, PUMP_FOREND_REST, RIFLE, RIFLE_MAG_SEAT, RIFLE_MAG_TILT};

/// Viewmodel poses are written in this PostUpdate set: after the main camera
/// follows the eye (and after camera shake), before transform propagation.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ViewmodelSet;

/// World-space position of the held gun's muzzle as it appears on screen this
/// frame (projected from the viewmodel FOV into the world camera), or `None`
/// when no gun is up. Tracers start here.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct MuzzlePoint(pub Option<Vec3>);

/// The viewmodel camera.
#[derive(Component, Debug)]
pub struct ViewmodelCamera;

#[derive(Component, Debug)]
struct ViewmodelLight;

/// What the viewmodel can hold up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Rifle,
    Pump,
    Blueprint,
}

impl Item {
    fn of(tool: ActiveTool) -> Self {
        match tool {
            ActiveTool::Weapon(WeaponKind::Rifle) => Item::Rifle,
            ActiveTool::Weapon(WeaponKind::Pump) => Item::Pump,
            ActiveTool::Build(_) => Item::Blueprint,
        }
    }
}

/// Role of each animated viewmodel entity.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum VmPart {
    Rig,
    Item(Item),
    RifleMag,
    PumpForend,
    PumpShell,
    Flash(WeaponKind),
    Mini(PieceKind),
}

/// The blueprint tablet's hip pose (no ADS, no muzzle).
const BLUEPRINT: GunSpec = GunSpec {
    sight: Vec3::ZERO,
    muzzle: Vec3::ZERO,
    hip: Vec3::new(0.15, -0.19, -0.34),
    hip_euler: Vec3::new(0.95, -0.25, 0.12),
    ads_distance: 0.3,
};

/// Seconds to ease into and out of aim-down-sights.
const ADS_SECONDS: f32 = 0.12;
/// Sprint "gun away" pose.
const SPRINT_POSE: PoseOffset = PoseOffset {
    pos: Vec3::new(-0.025, -0.035, 0.03),
    euler: Vec3::new(-0.16, 0.40, 0.20),
};
/// Slide lean.
const SLIDE_POSE: PoseOffset = PoseOffset {
    pos: Vec3::new(-0.01, -0.02, 0.0),
    euler: Vec3::new(0.0, 0.05, 0.22),
};
const RIFLE_KICK: Spring = Spring::new(38.0);
const PUMP_KICK: Spring = Spring::new(19.0);
const SWAY: Spring = Spring::new(13.0);
/// Walk bob: one left-right cycle per this many meters walked.
const BOB_STRIDE: f32 = 2.8;
/// The viewmodel light copies the sun at this strength, plus ambient at this gain.
const LIGHT_GAIN: f32 = 1.0;
const AMBIENT_GAIN: f32 = 1.25;

#[derive(Resource, Debug)]
struct ViewmodelState {
    tool: Option<ActiveTool>,
    prev: Option<ActiveTool>,
    /// Switch progress 0..=1 (1 = done).
    switch_t: f32,
    ads: f32,
    kick: Spring,
    kick_pos: Vec3,
    kick_pos_v: Vec3,
    kick_rot: Vec3,
    kick_rot_v: Vec3,
    sway_pos: Vec3,
    sway_pos_v: Vec3,
    sway_rot: Vec3,
    sway_rot_v: Vec3,
    bob_phase: f32,
    bob_amount: f32,
    sprint: f32,
    slide: f32,
    pump_reload: f32,
    last_look: Option<(f32, f32)>,
    rack_age: f32,
    flash_frames: u8,
    flash_kind: WeaponKind,
    flash_roll: f32,
    flash_scale: f32,
    rng: FxRng,
}

impl Default for ViewmodelState {
    fn default() -> Self {
        Self {
            tool: None,
            prev: None,
            switch_t: 1.0,
            ads: 0.0,
            kick: RIFLE_KICK,
            kick_pos: Vec3::ZERO,
            kick_pos_v: Vec3::ZERO,
            kick_rot: Vec3::ZERO,
            kick_rot_v: Vec3::ZERO,
            sway_pos: Vec3::ZERO,
            sway_pos_v: Vec3::ZERO,
            sway_rot: Vec3::ZERO,
            sway_rot_v: Vec3::ZERO,
            bob_phase: 0.0,
            bob_amount: 0.0,
            sprint: 0.0,
            slide: 0.0,
            pump_reload: 0.0,
            last_look: None,
            rack_age: 10.0,
            flash_frames: 0,
            flash_kind: WeaponKind::Rifle,
            flash_roll: 0.0,
            flash_scale: 1.0,
            rng: FxRng::new(0x51DE),
        }
    }
}

pub struct ViewmodelPlugin;

impl Plugin for ViewmodelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MuzzlePoint>()
            .init_resource::<ViewmodelState>()
            .configure_sets(
                PostUpdate,
                ViewmodelSet
                    .after(CameraFollowSet)
                    .before(TransformSystems::Propagate),
            )
            .add_systems(PostStartup, spawn_viewmodel)
            .add_systems(Update, match_sun)
            .add_systems(PostUpdate, animate_viewmodel.in_set(ViewmodelSet));
    }
}

fn spawn_viewmodel(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    target: Option<Res<WorldTarget>>,
    main: Option<Single<Entity, With<MainCamera>>>,
    tuning: Res<Tuning>,
) {
    let (Some(target), Some(main)) = (target, main) else {
        return;
    };
    let layer = RenderLayers::layer(VIEWMODEL_LAYER);
    let gun_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.48,
        reflectance: 0.45,
        ..default()
    });
    let glow_mat = materials.add(StandardMaterial {
        base_color: mesh::shade(palette::GUN_ACCENT, 1.35),
        unlit: true,
        ..default()
    });
    let flash_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        ..default()
    });

    let camera = commands
        .spawn((
            Name::new("Viewmodel camera"),
            ViewmodelCamera,
            Camera3d::default(),
            RenderTarget::Image(target.image.clone().into()),
            Camera {
                order: VIEWMODEL_CAMERA_ORDER,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            Projection::Perspective(PerspectiveProjection {
                fov: tuning.feedback.viewmodel_fov_deg.to_radians(),
                near: 0.01,
                far: 5.0,
                ..default()
            }),
            Msaa::Sample4,
            layer.clone(),
            AmbientLight {
                color: palette::AMBIENT,
                brightness: 500.0 * AMBIENT_GAIN,
                ..default()
            },
            Transform::IDENTITY,
            ChildOf(*main),
        ))
        .id();
    commands.spawn((
        Name::new("Viewmodel light"),
        ViewmodelLight,
        DirectionalLight {
            illuminance: 9000.0,
            color: palette::SUNLIGHT,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(20.0, 30.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        layer.clone(),
    ));

    let rig = commands
        .spawn((
            Name::new("Viewmodel rig"),
            VmPart::Rig,
            Transform::from_translation(RIFLE.hip),
            Visibility::Visible,
            ChildOf(camera),
        ))
        .id();

    let item = |commands: &mut Commands, it: Item| {
        commands
            .spawn((
                Name::new(format!("Viewmodel {it:?}")),
                VmPart::Item(it),
                Transform::IDENTITY,
                Visibility::Hidden,
                ChildOf(rig),
            ))
            .id()
    };
    let rifle_item = item(&mut commands, Item::Rifle);
    let pump_item = item(&mut commands, Item::Pump);
    let blueprint_item = item(&mut commands, Item::Blueprint);

    let part = |commands: &mut Commands,
                parent: Entity,
                mesh: Handle<Mesh>,
                material: &Handle<StandardMaterial>,
                transform: Transform,
                role: Option<VmPart>,
                visible: bool| {
        let mut e = commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material.clone()),
            transform,
            if visible {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            },
            layer.clone(),
            NotShadowCaster,
            NotShadowReceiver,
            ChildOf(parent),
        ));
        if let Some(role) = role {
            e.insert(role);
        }
    };

    let flash = meshes.add(models::muzzle_flash().build());

    let rifle = models::rifle();
    let body = meshes.add(rifle.body.build());
    part(
        &mut commands,
        rifle_item,
        body,
        &gun_mat,
        Transform::IDENTITY,
        None,
        true,
    );
    let mag = meshes.add(rifle.mag.build());
    part(
        &mut commands,
        rifle_item,
        mag,
        &gun_mat,
        Transform::from_translation(RIFLE_MAG_SEAT)
            .with_rotation(Quat::from_rotation_x(RIFLE_MAG_TILT)),
        Some(VmPart::RifleMag),
        true,
    );
    let dot = meshes.add(rifle.dot.build());
    part(
        &mut commands,
        rifle_item,
        dot,
        &glow_mat,
        Transform::from_translation(RIFLE.sight),
        None,
        true,
    );
    part(
        &mut commands,
        rifle_item,
        flash.clone(),
        &flash_mat,
        Transform::from_translation(RIFLE.muzzle),
        Some(VmPart::Flash(WeaponKind::Rifle)),
        false,
    );

    let pump = models::pump();
    let body = meshes.add(pump.body.build());
    part(
        &mut commands,
        pump_item,
        body,
        &gun_mat,
        Transform::IDENTITY,
        None,
        true,
    );
    let forend = meshes.add(pump.forend.build());
    part(
        &mut commands,
        pump_item,
        forend,
        &gun_mat,
        Transform::from_translation(PUMP_FOREND_REST),
        Some(VmPart::PumpForend),
        true,
    );
    let shell = meshes.add(pump.shell.build());
    part(
        &mut commands,
        pump_item,
        shell,
        &gun_mat,
        Transform::IDENTITY,
        Some(VmPart::PumpShell),
        false,
    );
    part(
        &mut commands,
        pump_item,
        flash,
        &flash_mat,
        Transform::from_translation(PUMP.muzzle),
        Some(VmPart::Flash(WeaponKind::Pump)),
        false,
    );

    let tablet = meshes.add(models::blueprint().build());
    part(
        &mut commands,
        blueprint_item,
        tablet,
        &gun_mat,
        Transform::IDENTITY,
        None,
        true,
    );
    for kind in [PieceKind::Wall, PieceKind::Floor, PieceKind::Ramp] {
        let mini = meshes.add(models::mini_piece(kind).build());
        part(
            &mut commands,
            blueprint_item,
            mini,
            &gun_mat,
            Transform::from_xyz(-0.01, 0.0036, 0.0),
            Some(VmPart::Mini(kind)),
            false,
        );
    }
}

/// Keeps the viewmodel light and ambient matched to the world's sun and ambient.
fn match_sun(
    suns: Query<(&DirectionalLight, &GlobalTransform), Without<ViewmodelLight>>,
    light: Option<Single<(&mut DirectionalLight, &mut Transform), With<ViewmodelLight>>>,
    global: Res<GlobalAmbientLight>,
    ambient: Option<Single<&mut AmbientLight, With<ViewmodelCamera>>>,
) {
    if let Some(light) = light
        && let Some((sun, sun_tf)) = suns
            .iter()
            .max_by(|a, b| a.0.illuminance.total_cmp(&b.0.illuminance))
    {
        let (mut vm_light, mut tf) = light.into_inner();
        let rotation = sun_tf.rotation();
        if tf.rotation != rotation {
            tf.rotation = rotation;
        }
        let illuminance = sun.illuminance * LIGHT_GAIN;
        if vm_light.illuminance != illuminance || vm_light.color != sun.color {
            vm_light.illuminance = illuminance;
            vm_light.color = sun.color;
        }
    }
    if let Some(mut ambient) = ambient {
        let brightness = global.brightness * AMBIENT_GAIN;
        if ambient.brightness != brightness || ambient.color != global.color {
            ambient.brightness = brightness;
            ambient.color = global.color;
        }
    }
}

fn spec_of(item: Item) -> GunSpec {
    match item {
        Item::Rifle => RIFLE,
        Item::Pump => PUMP,
        Item::Blueprint => BLUEPRINT,
    }
}

/// Moves `x` toward `target` by at most `step`.
fn approach(x: f32, target: f32, step: f32) -> f32 {
    if x < target {
        (x + step).min(target)
    } else {
        (x - step).max(target)
    }
}

fn wrap_angle(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

fn flag(b: bool) -> f32 {
    if b { 1.0 } else { 0.0 }
}

fn animate_viewmodel(
    time: Res<Time>,
    tuning: Res<Tuning>,
    fov: Res<CurrentFov>,
    mut state: ResMut<ViewmodelState>,
    mut muzzle: ResMut<MuzzlePoint>,
    mut shots: MessageReader<ShotFired>,
    mut cues: MessageReader<GameCue>,
    player: Option<
        Single<
            (
                Entity,
                &ActiveTool,
                &Ads,
                &Loadout,
                &LookAngles,
                Option<&Motor>,
            ),
            With<Player>,
        >,
    >,
    main_camera: Option<Single<&Transform, (With<MainCamera>, Without<VmPart>)>>,
    mut vm_camera: Option<Single<&mut Projection, With<ViewmodelCamera>>>,
    mut parts: Query<(&VmPart, &mut Transform, &mut Visibility)>,
) {
    let dt = time.delta_secs();
    let Some(player) = player else {
        for (part, _, mut vis) in &mut parts {
            if *part == VmPart::Rig {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
        muzzle.0 = None;
        shots.clear();
        cues.clear();
        return;
    };
    let (me, tool, ads, loadout, look, motor) = player.into_inner();
    let st = &mut *state;
    let combat = &tuning.combat;
    let feedback = &tuning.feedback;

    // Tool changes start a lower/raise (not between two build pieces).
    if st.tool != Some(*tool) {
        match st.tool {
            Some(old) if !(old.is_build() && tool.is_build()) => {
                st.prev = Some(old);
                st.switch_t = 0.0;
            }
            None => st.switch_t = 1.0,
            _ => {}
        }
        st.tool = Some(*tool);
    }
    st.switch_t = (st.switch_t + dt / combat.switch_time.max(0.05)).min(1.0);
    let (show_prev, lowered) = match st.prev {
        Some(_) if st.switch_t < 1.0 => switch_phase(st.switch_t),
        _ => (false, 0.0),
    };
    let shown_tool = if show_prev {
        st.prev.unwrap_or(*tool)
    } else {
        *tool
    };
    let shown = Item::of(shown_tool);
    let spec = spec_of(shown);
    let gun = match shown {
        Item::Rifle => Some(WeaponKind::Rifle),
        Item::Pump => Some(WeaponKind::Pump),
        Item::Blueprint => None,
    };

    // ADS eases in over ~0.12 s.
    let ads_target = flag(ads.0 && !tool.is_build());
    st.ads = approach(st.ads, ads_target, dt / ADS_SECONDS);
    let ads_e = smoothstep(st.ads);
    let calm = 1.0 - 0.85 * ads_e;

    // Shots: spring kick, muzzle flash, pump rack.
    for shot in shots.read() {
        if shot.shooter != me {
            continue;
        }
        let rng = &mut st.rng;
        let (spring, pos_peak, rot_peak) = match shot.weapon {
            WeaponKind::Rifle => (
                RIFLE_KICK,
                Vec3::new(rng.range(-0.002, 0.002), 0.003, 0.018),
                Vec3::new(0.030, rng.range(-0.008, 0.008), rng.range(-0.012, 0.012)),
            ),
            WeaponKind::Pump => (
                PUMP_KICK,
                Vec3::new(0.0, 0.010, 0.075),
                Vec3::new(0.19, rng.range(-0.02, 0.02), rng.range(-0.05, -0.02)),
            ),
        };
        if shot.weapon == WeaponKind::Pump {
            st.rack_age = 0.0;
        }
        let rot_scale = 1.0 - 0.55 * ads_e;
        let pos_scale = Vec3::new(1.0, 1.0, 1.0 - 0.3 * ads_e);
        st.kick = spring;
        st.kick_pos_v += Vec3::new(
            spring.kick_for_peak(pos_peak.x),
            spring.kick_for_peak(pos_peak.y),
            spring.kick_for_peak(pos_peak.z),
        ) * pos_scale;
        st.kick_rot_v += Vec3::new(
            spring.kick_for_peak(rot_peak.x),
            spring.kick_for_peak(rot_peak.y),
            spring.kick_for_peak(rot_peak.z),
        ) * rot_scale;
        st.flash_frames = 2;
        st.flash_kind = shot.weapon;
        st.flash_roll = st.rng.range(0.0, std::f32::consts::TAU);
        st.flash_scale = st.rng.range(0.85, 1.15);
    }
    for cue in cues.read() {
        match *cue {
            GameCue::Land { who, speed } if who == me => {
                let s = (speed / 8.0).clamp(0.0, 1.0);
                st.kick_pos_v.y -= st.kick.kick_for_peak(0.02 * s);
                st.kick_rot_v.x -= st.kick.kick_for_peak(0.04 * s);
            }
            GameCue::Jump { who } if who == me => {
                st.kick_pos_v.y -= st.kick.kick_for_peak(0.008);
            }
            _ => {}
        }
    }
    let kick = st.kick;
    kick.step(&mut st.kick_pos, &mut st.kick_pos_v, Vec3::ZERO, dt);
    kick.step(&mut st.kick_rot, &mut st.kick_rot_v, Vec3::ZERO, dt);

    // Sway from look and movement.
    let (dyaw, dpitch) = match st.last_look {
        Some((yaw, pitch)) => (wrap_angle(look.yaw - yaw), look.pitch - pitch),
        None => (0.0, 0.0),
    };
    st.last_look = Some((look.yaw, look.pitch));
    let sway_on = feedback.viewmodel_sway;
    let (mut sway_pos, mut sway_rot) = (Vec3::ZERO, Vec3::ZERO);
    let (speed, grounded, sliding, sprinting) = motor
        .map(|m| {
            (
                m.velocity.with_y(0.0).length(),
                m.grounded,
                m.sliding,
                m.sprinting,
            )
        })
        .unwrap_or((0.0, true, false, false));
    if sway_on && dt > 1e-4 {
        let rate = (Vec2::new(dyaw, dpitch) / dt).clamp_length_max(9.0);
        sway_pos += Vec3::new(rate.x * 0.0045, -rate.y * 0.0045, 0.0);
        sway_rot += Vec3::new(-rate.y * 0.012, -rate.x * 0.010, rate.x * 0.018);
        if let Some(m) = motor {
            let (fwd, right) = look.flat_basis();
            let vr = m.velocity.dot(right);
            let vf = m.velocity.dot(fwd);
            sway_pos += Vec3::new(
                -vr * 0.0022,
                (-m.velocity.y * 0.004).clamp(-0.025, 0.025),
                vf * 0.0018,
            );
            sway_rot.z -= vr * 0.010;
        }
        sway_pos = sway_pos.clamp_length_max(0.04);
    }
    SWAY.step(&mut st.sway_pos, &mut st.sway_pos_v, sway_pos, dt);
    SWAY.step(&mut st.sway_rot, &mut st.sway_rot_v, sway_rot, dt);

    // Walk bob on the gun only (never the camera).
    let bob_target = if sway_on && grounded && !sliding {
        (speed / 5.5).min(1.4)
    } else {
        0.0
    };
    st.bob_amount += (bob_target - st.bob_amount) * (1.0 - (-10.0 * dt).exp());
    if grounded {
        st.bob_phase = (st.bob_phase + dt * speed * std::f32::consts::TAU / BOB_STRIDE)
            % std::f32::consts::TAU;
    }
    let a = st.bob_amount * (1.0 - 0.9 * ads_e);
    let phase = st.bob_phase;
    let bob = PoseOffset {
        pos: Vec3::new(phase.sin() * 0.006 * a, -phase.sin().abs() * 0.009 * a, 0.0),
        euler: Vec3::new(0.0, 0.0, phase.sin() * 0.012 * a),
    };

    // Sprint and slide stances.
    let since_shot = gun.map_or(10.0, |k| loadout.gun(k).since_shot);
    let sprint_target = sprinting && grounded && !sliding && ads_e < 0.01 && since_shot > 0.3;
    st.sprint = approach(st.sprint, flag(sprint_target), dt / 0.16);
    st.slide = approach(st.slide, flag(sliding), dt / 0.12);

    // Reloads and the pump rack.
    let mut reload = PoseOffset::default();
    let mut mag = PoseOffset::default();
    let mut mag_visible = true;
    let mut shell = pump_shell(0.0);
    let pump_reloading = loadout.pump.is_reloading() && shown == Item::Pump;
    st.pump_reload = approach(
        st.pump_reload,
        flag(pump_reloading),
        dt / if pump_reloading { 0.12 } else { 0.16 },
    );
    st.rack_age += dt;
    let rack = if shown == Item::Pump {
        pump_rack(st.rack_age)
    } else {
        0.0
    };
    match shown {
        Item::Rifle => {
            if let Some(p) = loadout.rifle.reload_progress(&combat.rifle) {
                let pose = rifle_reload(p);
                reload = pose.gun;
                mag = pose.mag;
                mag_visible = pose.mag_visible;
            }
        }
        Item::Pump => {
            reload = PUMP_RELOAD_STANCE.scaled(smoothstep(st.pump_reload));
            if pump_reloading && let Some(p) = loadout.pump.reload_progress(&combat.pump) {
                shell = pump_shell(p);
                reload.pos.z += shell.gun_push;
            } else {
                shell.visible = false;
            }
            reload = reload
                + PoseOffset {
                    pos: Vec3::new(0.0, -0.008, 0.01),
                    euler: Vec3::new(0.0, 0.0, -0.07),
                }
                .scaled(rack);
        }
        Item::Blueprint => {}
    }

    // Compose the rig pose.
    let hip = PoseOffset {
        pos: spec.hip,
        euler: spec.hip_euler,
    };
    let base = if gun.is_some() {
        hip.scaled(1.0 - ads_e)
            + PoseOffset {
                pos: ads_translation(&spec),
                euler: Vec3::ZERO,
            }
            .scaled(ads_e)
    } else {
        hip
    };
    let sway = PoseOffset {
        pos: st.sway_pos,
        euler: st.sway_rot,
    };
    let kick_pose = PoseOffset {
        pos: st.kick_pos,
        euler: st.kick_rot,
    };
    let offset = sway.scaled(calm)
        + bob
        + SPRINT_POSE.scaled(smoothstep(st.sprint))
        + SLIDE_POSE.scaled(smoothstep(st.slide) * calm)
        + reload
        + kick_pose
        + LOWERED.scaled(lowered);
    let rig_tf = Transform {
        translation: base.pos + offset.pos,
        rotation: euler(base.euler + offset.euler),
        scale: Vec3::ONE,
    };

    // Viewmodel FOV tightens a little in ADS.
    let fov_vm = feedback.viewmodel_fov_deg.clamp(40.0, 90.0).to_radians() * (1.0 - 0.1 * ads_e);
    if let Some(projection) = vm_camera.as_mut()
        && let Projection::Perspective(p) = projection.as_mut()
        && (p.fov - fov_vm).abs() > 1e-5
    {
        p.fov = fov_vm;
    }

    // Where the muzzle appears on screen, placed in the world at the same depth.
    muzzle.0 = match (gun, main_camera) {
        (Some(_), Some(cam)) if lowered < 0.5 => {
            let m = rig_tf.transform_point(spec.muzzle);
            let s = (fov.0 * 0.5).tan() / (fov_vm * 0.5).tan();
            Some(cam.transform_point(Vec3::new(m.x * s, m.y * s, m.z)))
        }
        _ => None,
    };

    let flash_on = st.flash_frames > 0 && gun == Some(st.flash_kind) && lowered < 0.5;
    let flash_size = st.flash_scale
        * if st.flash_frames >= 2 { 1.0 } else { 0.6 }
        * if st.flash_kind == WeaponKind::Pump {
            1.9
        } else {
            1.0
        };
    let build_kind = match tool {
        ActiveTool::Build(kind) => Some(*kind),
        _ => None,
    };
    for (part, mut tf, mut vis) in &mut parts {
        let want = match *part {
            VmPart::Rig => {
                *tf = rig_tf;
                true
            }
            VmPart::Item(it) => it == shown,
            VmPart::RifleMag => {
                tf.translation = RIFLE_MAG_SEAT + mag.pos;
                tf.rotation = euler(Vec3::new(RIFLE_MAG_TILT, 0.0, 0.0) + mag.euler);
                mag_visible
            }
            VmPart::PumpForend => {
                tf.translation = PUMP_FOREND_REST + forend_offset(rack);
                true
            }
            VmPart::PumpShell => {
                tf.translation = shell.pos;
                tf.rotation = Quat::from_rotation_x(shell.tilt);
                shell.visible
            }
            VmPart::Flash(kind) => {
                let on = flash_on && kind == st.flash_kind;
                if on {
                    tf.rotation = Quat::from_rotation_z(st.flash_roll);
                    tf.scale = Vec3::splat(flash_size);
                }
                on
            }
            VmPart::Mini(kind) => Some(kind) == build_kind,
        };
        let v = match (want, *part) {
            (false, _) => Visibility::Hidden,
            (true, VmPart::Rig) => Visibility::Visible,
            (true, _) => Visibility::Inherited,
        };
        vis.set_if_neq(v);
    }
    st.flash_frames = st.flash_frames.saturating_sub(1);
}
