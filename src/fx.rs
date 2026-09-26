//! Effects: the spells ([`spells`]: bolts, pump sparks, impacts by type,
//! shield shimmer and break, the elimination poof and the dropped hat), and
//! here the piece-break debris, camera shake and hitstop. (The muzzle flashes
//! live on the viewmodel layer, in [`crate::viewmodel`]; [`spells`] restyles
//! them.)
//!
//! Every mesh and material is built once at startup, and every effect draws
//! from fixed pools of hidden entities sized by [`FeedbackTuning`]'s caps
//! (`max_particles`, `max_debris`). When a pool is full the oldest effect is
//! recycled, so the frame cost is bounded no matter how much is going on.
//! Per frame this only moves transforms and swaps handles.
//!
//! Every effect clock, the spells' included, reads
//! [`FreezableTime`](crate::shared::FreezableTime), so lifetimes, fades, bolt
//! flight and shake stand still while the gallery holds a moment
//! ([`GalleryFreeze`](crate::shared::GalleryFreeze)).

pub mod hat;
pub mod material;
pub mod shapes;
pub mod sim;
pub mod spells;

use crate::{
    building::{Piece, ramp_surface_height, visuals::PieceDebris},
    palette,
    render::{CameraFollowSet, MainCamera},
    shared::{Eliminated, PieceChange, PieceChanged, PieceKind, Player, ShotFired, WeaponKind},
    tuning::Tuning,
    viewmodel::{
        MuzzlePoint, ViewmodelSet,
        mesh::{ModelBuilder, linear, shade},
    },
};
use bevy::{
    light::NotShadowCaster, platform::collections::HashMap, prelude::*,
    render::render_resource::Face,
};
use serde::{Deserialize, Serialize};
use sim::{FxRng, Hitstop, Particle, Shake, SlotPool, fade_step};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct FeedbackTuning {
    /// 0 disables camera shake entirely.
    pub camera_shake: f32,
    pub hitstop_on_kill: bool,
    pub hitstop_frames: u32,
    pub viewmodel_sway: bool,
    pub max_debris: u32,
    pub max_particles: u32,
    /// Vertical FOV of the gun (viewmodel) camera, independent of the world FOV.
    pub viewmodel_fov_deg: f32,
}

impl Default for FeedbackTuning {
    fn default() -> Self {
        Self {
            camera_shake: 0.3,
            hitstop_on_kill: true,
            hitstop_frames: 2,
            viewmodel_sway: true,
            max_debris: 160,
            max_particles: 400,
            viewmodel_fov_deg: 58.0,
        }
    }
}

/// Fade steps per translucent effect color.
const FADE_STEPS: usize = 6;
/// Piece breaks closer than this shake the camera (m).
const SHAKE_RADIUS: f32 = 10.0;
/// Hard ceilings on the pools, whatever the settings file says.
const PARTICLE_CEILING: u32 = 2000;
const DEBRIS_CEILING: u32 = 1000;

/// Translucent effect colors that fade out through [`FADE_STEPS`] materials.
/// (Spells glow through [`spells`]; only the piece-break dust fades here.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Dust,
}

const FAMILIES: [Family; 1] = [Family::Dust];

/// How a fade family draws: color, blending, starting alpha, and whether back
/// faces are culled (closed shells) or drawn (flat cards and rings).
struct FamilyLook {
    color: Color,
    alpha_mode: AlphaMode,
    alpha: f32,
    cull_back: bool,
}

impl Family {
    fn index(self) -> usize {
        FAMILIES.iter().position(|f| *f == self).unwrap_or(0)
    }

    fn look(self) -> FamilyLook {
        let (color, alpha_mode, alpha, cull_back) = match self {
            Family::Dust => (shade(palette::SAND, 1.1), AlphaMode::Blend, 0.22, true),
        };
        FamilyLook {
            color,
            alpha_mode,
            alpha,
            cull_back,
        }
    }
}

#[derive(Clone)]
enum Paint {
    Solid(Handle<StandardMaterial>),
    Fade(Family),
}

#[derive(Resource)]
struct FxAssets {
    chunk: Handle<Mesh>,
    wedge: Handle<Mesh>,
    ico: Handle<Mesh>,
    wood: [Handle<StandardMaterial>; 4],
    fades: Vec<[Handle<StandardMaterial>; FADE_STEPS]>,
}

impl FxAssets {
    fn fade(&self, family: Family, step: usize) -> Handle<StandardMaterial> {
        self.fades[family.index()][step.min(FADE_STEPS - 1)].clone()
    }
}

struct Slot {
    p: Particle,
    mesh: Handle<Mesh>,
    paint: Paint,
    fresh: bool,
    step: usize,
}

struct ParticlePool {
    slots: SlotPool,
    entities: Vec<Entity>,
    live: Vec<Option<Slot>>,
}

impl ParticlePool {
    fn new(entities: Vec<Entity>) -> Self {
        Self {
            slots: SlotPool::new(entities.len()),
            live: (0..entities.len()).map(|_| None).collect(),
            entities,
        }
    }

    fn emit(&mut self, limit: usize, p: Particle, mesh: &Handle<Mesh>, paint: Paint) {
        if let Some(i) = self.slots.alloc(limit) {
            self.live[i] = Some(Slot {
                p,
                mesh: mesh.clone(),
                paint,
                fresh: true,
                step: usize::MAX,
            });
        }
    }
}

#[derive(Resource)]
struct FxPools {
    particles: ParticlePool,
    debris: ParticlePool,
}

#[derive(Resource)]
struct FxState {
    shake: Shake,
    hitstop: Hitstop,
    rng: FxRng,
}

impl Default for FxState {
    fn default() -> Self {
        Self {
            shake: Shake::default(),
            hitstop: Hitstop::default(),
            rng: FxRng::new(0xF00D),
        }
    }
}

/// Pieces removed in the last few frames, so a break can shatter along the
/// piece's real shape after its entity is gone.
#[derive(Resource, Default)]
struct RemovedPieces(HashMap<Entity, (Piece, u8)>);

/// Marks a pooled effect entity.
#[derive(Component)]
struct FxEntity;

/// Frames to keep the pipeline warm-up draws alive after startup.
const PREWARM_FRAMES: u32 = 120;

/// Tiny always-visible draws of each effect material during boot.
#[derive(Resource)]
struct Prewarm {
    entities: Vec<Entity>,
    frames_left: u32,
}

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FxState>()
            .init_resource::<RemovedPieces>()
            .init_resource::<MuzzlePoint>()
            .add_systems(Startup, setup_fx)
            .add_observer(remember_removed_piece)
            .configure_sets(
                PostUpdate,
                ViewmodelSet
                    .after(CameraFollowSet)
                    .before(TransformSystems::Propagate),
            )
            .add_systems(
                PostUpdate,
                shake_camera.after(CameraFollowSet).before(ViewmodelSet),
            )
            .add_systems(
                PostUpdate,
                (emit_fx, simulate_fx, prewarm_fx)
                    .chain()
                    .after(ViewmodelSet)
                    .before(TransformSystems::Propagate),
            )
            .add_systems(Last, end_hitstop_frame)
            .add_plugins(spells::SpellsPlugin);
    }
}

// ---------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------

fn unit_mesh(f: impl FnOnce(&mut ModelBuilder)) -> Mesh {
    let mut m = ModelBuilder::new();
    f(&mut m);
    m.build()
}

fn setup_fx(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    tuning: Res<Tuning>,
) {
    let white = Color::WHITE;
    let chunk = meshes.add(unit_mesh(|m| {
        m.chamfer_box(Vec3::splat(-0.5), Vec3::splat(0.5), 0.12, white)
    }));
    let wedge = meshes.add(unit_mesh(|m| {
        let zy = [
            Vec2::new(-0.5, -0.5),
            Vec2::new(0.5, -0.5),
            Vec2::new(0.5, 0.05),
            Vec2::new(-0.15, 0.5),
            Vec2::new(-0.5, 0.3),
        ];
        m.prism_x(&zy, -0.5, 0.5, white);
    }));
    let ico = meshes.add(unit_mesh(|m| m.geosphere(1.0, 2, linear(white, 1.0))));

    let mut lit = |color: Color| {
        materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.85,
            ..default()
        })
    };
    let wood = [
        lit(palette::WOOD),
        lit(palette::WOOD_LIGHT),
        lit(palette::WOOD_DARK),
        lit(palette::WOOD_TRIM),
    ];
    let fades = FAMILIES
        .iter()
        .map(|family| {
            let look = family.look();
            std::array::from_fn(|k| {
                let alpha = look.alpha * (1.0 - k as f32 / FADE_STEPS as f32);
                materials.add(StandardMaterial {
                    base_color: look.color.with_alpha(alpha),
                    unlit: true,
                    alpha_mode: look.alpha_mode,
                    cull_mode: look.cull_back.then_some(Face::Back),
                    ..default()
                })
            })
        })
        .collect::<Vec<[Handle<StandardMaterial>; FADE_STEPS]>>();

    let mut spawn_pool = |n: usize, mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>| {
        (0..n)
            .map(|_| {
                commands
                    .spawn((
                        FxEntity,
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::default(),
                        Visibility::Hidden,
                        NotShadowCaster,
                    ))
                    .id()
            })
            .collect::<Vec<Entity>>()
    };
    let feedback = &tuning.feedback;
    let particles = spawn_pool(
        feedback.max_particles.min(PARTICLE_CEILING) as usize,
        &chunk,
        &wood[0],
    );
    let debris = spawn_pool(
        feedback.max_debris.min(DEBRIS_CEILING) as usize,
        &chunk,
        &wood[0],
    );

    // Draw every effect material once, too small to see, while the game boots,
    // so their pipelines are compiled before the first shot instead of during it.
    let mut warm: Vec<Handle<StandardMaterial>> =
        fades.iter().map(|steps| steps[0].clone()).collect();
    warm.push(wood[0].clone());
    let prewarm = warm
        .into_iter()
        .map(|material| {
            commands
                .spawn((
                    Mesh3d(ico.clone()),
                    MeshMaterial3d(material),
                    Transform::from_scale(Vec3::splat(1e-4)),
                    Visibility::Visible,
                    NotShadowCaster,
                ))
                .id()
        })
        .collect();
    commands.insert_resource(Prewarm {
        entities: prewarm,
        frames_left: PREWARM_FRAMES,
    });

    commands.insert_resource(FxPools {
        particles: ParticlePool::new(particles),
        debris: ParticlePool::new(debris),
    });
    commands.insert_resource(FxAssets {
        chunk,
        wedge,
        ico,
        wood,
        fades,
    });
}

fn remember_removed_piece(
    remove: On<Remove, Piece>,
    pieces: Query<&Piece>,
    mut removed: ResMut<RemovedPieces>,
) {
    if let Ok(piece) = pieces.get(remove.entity) {
        removed.0.insert(remove.entity, (*piece, 0));
    }
}

// ---------------------------------------------------------------------------
// Emitters
// ---------------------------------------------------------------------------

struct Emitter<'a> {
    pools: &'a mut FxPools,
    assets: &'a FxAssets,
    rng: &'a mut FxRng,
    particle_limit: usize,
    debris_limit: usize,
}

impl Emitter<'_> {
    fn particle(&mut self, p: Particle, mesh: Handle<Mesh>, paint: Paint) {
        let limit = self.particle_limit;
        self.pools.particles.emit(limit, p, &mesh, paint);
    }

    fn chunk(&mut self, p: Particle, mesh: Handle<Mesh>, paint: Paint) {
        let limit = self.debris_limit;
        self.pools.debris.emit(limit, p, &mesh, paint);
    }

    /// The signature moment: a destroyed piece bursts into chunky bricks (walls)
    /// or plank splinters (floors and ramps) that tumble, bounce and shrink
    /// away, with crumbs and a dust poof. Uses the Blender debris models when
    /// the building visuals have them, plain wood chunks otherwise.
    fn piece_debris(
        &mut self,
        piece: Option<Piece>,
        kind: PieceKind,
        center: Vec3,
        eye: Vec3,
        models: Option<&PieceDebris>,
    ) {
        let frame = piece
            .map(|p| p.slot().transform())
            .unwrap_or_else(|| Transform::from_translation(center));
        // Local sample grid across the panel, and how planks lie on it.
        let (cols, rows) = match kind {
            PieceKind::Wall => (4, 3),
            PieceKind::Floor | PieceKind::Ramp => (4, 3),
        };
        let slope = (crate::shared::LEVEL_HEIGHT / crate::shared::CELL_SIZE).atan();
        let lie = match kind {
            PieceKind::Wall => Quat::IDENTITY,
            PieceKind::Floor => Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            PieceKind::Ramp => Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2 + slope),
        };
        let away = (center - eye).with_y(0.0).normalize_or(Vec3::Z);
        let mut samples = Vec::with_capacity(cols * rows + 2);
        for c in 0..cols {
            for r in 0..rows {
                let u = (c as f32 + 0.5) / cols as f32 * 2.0 - 1.0;
                let v = (r as f32 + 0.5) / rows as f32 * 2.0 - 1.0;
                samples.push((u, v));
            }
        }
        samples.push((self.rng.range(-0.5, 0.5), self.rng.range(-0.5, 0.5)));
        samples.push((self.rng.range(-0.5, 0.5), self.rng.range(-0.5, 0.5)));
        let jitter = 0.25;
        // The debris model for this piece: bricks from walls, splinters from planks.
        let model = models.map(|m| {
            let mesh = if kind == PieceKind::Wall {
                m.brick.clone()
            } else {
                m.splinter.clone()
            };
            (mesh, Paint::Solid(m.material.clone()))
        });
        // Resting half-height of a model chunk at scale 1.
        let model_radius = if kind == PieceKind::Wall { 0.13 } else { 0.05 };
        for (u, v) in samples {
            let u = u + self.rng.range(-jitter, jitter);
            let v = v + self.rng.range(-jitter, jitter);
            let local = match kind {
                PieceKind::Wall => Vec3::new(u * 1.7, v * 1.25, self.rng.range(-0.05, 0.05)),
                PieceKind::Floor => Vec3::new(u * 1.7, 0.0, v * 1.7),
                PieceKind::Ramp => {
                    let z = v * 1.7;
                    Vec3::new(u * 1.7, ramp_surface_height(z) - 0.1, z)
                }
            };
            let pos = frame.transform_point(local);
            let tilt = Quat::from_scaled_axis(self.rng.dir() * self.rng.range(0.0, 0.35));
            let vel = away * self.rng.range(1.5, 4.0)
                + (pos - center).normalize_or_zero() * self.rng.range(1.0, 2.5)
                + Vec3::Y * self.rng.range(2.0, 5.0);
            let (size, radius, mesh, paint) = match &model {
                Some((mesh, paint)) => {
                    let k = self.rng.range(0.85, 1.35);
                    (
                        Vec3::splat(k),
                        model_radius * k,
                        mesh.clone(),
                        paint.clone(),
                    )
                }
                None => {
                    let size = Vec3::new(
                        self.rng.range(0.5, 0.95),
                        self.rng.range(0.2, 0.32),
                        self.rng.range(0.09, 0.14),
                    );
                    let mesh = if self.rng.f() < 0.3 {
                        self.assets.wedge.clone()
                    } else {
                        self.assets.chunk.clone()
                    };
                    let k = [0, 0, 1, 2, 3][self.rng.pick(5)];
                    (
                        size,
                        size.z * 0.5,
                        mesh,
                        Paint::Solid(self.assets.wood[k].clone()),
                    )
                }
            };
            let p = Particle {
                pos,
                vel,
                rot: frame.rotation * lie * tilt,
                spin: self.rng.dir() * self.rng.range(3.0, 11.0),
                gravity: 20.0,
                drag: 0.2,
                bounce: Some(0.35),
                radius,
                size,
                life: self.rng.range(1.05, 1.35),
                shrink_start: 0.72,
                ..default()
            };
            self.chunk(p, mesh, paint);
        }
        // Crumbs: small bits of brick or splinters flung further.
        for _ in 0..16 {
            let local = match kind {
                PieceKind::Wall => {
                    Vec3::new(self.rng.range(-1.8, 1.8), self.rng.range(-1.3, 1.3), 0.0)
                }
                _ => Vec3::new(self.rng.range(-1.8, 1.8), 0.1, self.rng.range(-1.8, 1.8)),
            };
            let pos = frame.transform_point(local);
            let dir = (self.rng.dir() + away * 0.6 + Vec3::Y * 0.5).normalize_or(Vec3::Y);
            let (size, radius, mesh, paint) = match &model {
                Some((mesh, paint)) => {
                    let k = self.rng.range(0.3, 0.5);
                    (
                        Vec3::splat(k),
                        model_radius * k,
                        mesh.clone(),
                        paint.clone(),
                    )
                }
                None => {
                    let size = Vec3::new(
                        self.rng.range(0.04, 0.08),
                        self.rng.range(0.025, 0.04),
                        self.rng.range(0.12, 0.22),
                    );
                    let k = self.rng.pick(4);
                    (
                        size,
                        size.y * 0.5,
                        self.assets.chunk.clone(),
                        Paint::Solid(self.assets.wood[k].clone()),
                    )
                }
            };
            let p = Particle {
                pos,
                vel: dir * self.rng.range(3.0, 7.0),
                rot: Quat::from_scaled_axis(self.rng.dir() * 3.0),
                spin: self.rng.dir() * self.rng.range(8.0, 20.0),
                gravity: 18.0,
                bounce: Some(0.3),
                radius,
                size,
                life: self.rng.range(0.6, 1.0),
                shrink_start: 0.6,
                ..default()
            };
            self.particle(p, mesh, paint);
        }
        // The dust poof: a ring of big soft puffs rolling out from the piece.
        for k in 0..5 {
            let local = match kind {
                PieceKind::Wall => {
                    Vec3::new(self.rng.range(-1.5, 1.5), self.rng.range(-1.2, 0.6), 0.0)
                }
                _ => Vec3::new(self.rng.range(-1.5, 1.5), 0.2, self.rng.range(-1.5, 1.5)),
            };
            let pos = frame.transform_point(local);
            let out = (pos - center).with_y(0.0).normalize_or(away);
            let p = Particle {
                pos,
                vel: out * self.rng.range(0.8, 1.6) + Vec3::Y * (0.3 + 0.1 * k as f32),
                size: Vec3::splat(self.rng.range(0.3, 0.42)),
                birth_scale: 0.4,
                grow: 2.6,
                drag: 2.5,
                life: self.rng.range(0.55, 0.75),
                shrink_start: 1.0,
                ..default()
            };
            let mesh = self.assets.ico.clone();
            self.particle(p, mesh, Paint::Fade(Family::Dust));
        }
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Camera shake on your own pump shots and nearby piece breaks, as a small
/// rotation offset applied after the camera has followed the eye.
fn shake_camera(
    time: crate::shared::FreezableTime,
    tuning: Res<Tuning>,
    mut state: ResMut<FxState>,
    mut shots: MessageReader<ShotFired>,
    mut changes: MessageReader<PieceChanged>,
    player: Option<Single<(Entity, &Transform), With<Player>>>,
    camera: Option<Single<&mut Transform, (With<MainCamera>, Without<Player>)>>,
) {
    let (me, eye) = player
        .map(|p| (Some(p.0), p.1.translation + Vec3::Y * 1.6))
        .unwrap_or((None, Vec3::ZERO));
    for shot in shots.read() {
        if Some(shot.shooter) == me && shot.weapon == WeaponKind::Pump {
            state.shake.add(0.55);
        }
    }
    for change in changes.read() {
        if change.change == PieceChange::Destroyed && me.is_some() {
            let d = change.center.distance(eye);
            if d < SHAKE_RADIUS {
                state.shake.add(0.9 * (1.0 - d / SHAKE_RADIUS));
            }
        }
    }
    state.shake.step(time.delta_secs());
    let angles = state.shake.angles(tuning.feedback.camera_shake);
    if angles != Vec3::ZERO
        && let Some(mut camera) = camera
    {
        camera.rotation *= Quat::from_euler(EulerRot::YXZ, angles.y, angles.x, angles.z);
    }
}

/// Piece-break debris and the hitstop on eliminations. (Shots, hits and
/// eliminations look like spells: see [`spells`].)
fn emit_fx(
    tuning: Res<Tuning>,
    assets: Option<Res<FxAssets>>,
    pools: Option<ResMut<FxPools>>,
    mut state: ResMut<FxState>,
    mut removed: ResMut<RemovedPieces>,
    debris: Option<Res<PieceDebris>>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut changes: MessageReader<PieceChanged>,
    mut eliminated: MessageReader<Eliminated>,
    player: Option<Single<(Entity, &Transform), With<Player>>>,
) {
    let (Some(assets), Some(mut pools)) = (assets, pools) else {
        return;
    };
    let feedback = &tuning.feedback;
    let eye = player.map_or(Vec3::ZERO, |p| p.1.translation + Vec3::Y * 1.6);
    let FxState { hitstop, rng, .. } = &mut *state;
    let mut fx = Emitter {
        pools: &mut pools,
        assets: &assets,
        rng,
        particle_limit: feedback.max_particles as usize,
        debris_limit: feedback.max_debris as usize,
    };

    for change in changes.read() {
        if change.change == PieceChange::Destroyed {
            let piece = removed.0.get(&change.entity).map(|(p, _)| *p);
            fx.piece_debris(piece, change.kind, change.center, eye, debris.as_deref());
        }
    }

    for _ in eliminated.read() {
        if feedback.hitstop_on_kill && hitstop.trigger(feedback.hitstop_frames) {
            virtual_time.pause();
        }
    }

    removed.0.retain(|_, (_, age)| {
        *age += 1;
        *age < 4
    });
}

fn simulate_fx(
    time: crate::shared::FreezableTime,
    assets: Option<Res<FxAssets>>,
    pools: Option<ResMut<FxPools>>,
    mut q: Query<
        (
            &mut Transform,
            &mut Visibility,
            &mut Mesh3d,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        With<FxEntity>,
    >,
) {
    let (Some(assets), Some(mut pools)) = (assets, pools) else {
        return;
    };
    let dt = time.delta_secs();
    let pools = &mut *pools;

    for pool in [&mut pools.particles, &mut pools.debris] {
        for i in 0..pool.live.len() {
            let Some(slot) = pool.live[i].as_mut() else {
                continue;
            };
            let Ok((mut tf, mut vis, mut mesh, mut material)) = q.get_mut(pool.entities[i]) else {
                continue;
            };
            if slot.fresh {
                slot.fresh = false;
                mesh.0 = slot.mesh.clone();
                if let Paint::Solid(handle) = &slot.paint {
                    material.0 = handle.clone();
                }
                vis.set_if_neq(Visibility::Visible);
            } else if !slot.p.step(dt) {
                vis.set_if_neq(Visibility::Hidden);
                pool.live[i] = None;
                pool.slots.free(i);
                continue;
            }
            if let Paint::Fade(family) = slot.paint {
                let step = fade_step(slot.p.age / slot.p.life, FADE_STEPS);
                if step != slot.step {
                    slot.step = step;
                    material.0 = assets.fade(family, step);
                }
            }
            tf.translation = slot.p.pos;
            tf.rotation = slot.p.render_rotation();
            tf.scale = slot.p.scale().max(Vec3::splat(1e-4));
        }
    }
}

fn end_hitstop_frame(mut state: ResMut<FxState>, mut virtual_time: ResMut<Time<Virtual>>) {
    if state.hitstop.end_frame() {
        virtual_time.unpause();
    }
}

/// Keeps the warm-up draws just in front of the camera for the first frames,
/// then removes them.
fn prewarm_fx(
    mut commands: Commands,
    prewarm: Option<ResMut<Prewarm>>,
    camera: Option<Single<&Transform, (With<MainCamera>, Without<FxEntity>)>>,
    mut transforms: Query<&mut Transform, (Without<MainCamera>, Without<FxEntity>)>,
) {
    let Some(mut prewarm) = prewarm else {
        return;
    };
    if prewarm.frames_left == 0 {
        for entity in prewarm.entities.drain(..) {
            commands.entity(entity).despawn();
        }
        commands.remove_resource::<Prewarm>();
        return;
    }
    prewarm.frames_left -= 1;
    let Some(camera) = camera else {
        return;
    };
    for (i, entity) in prewarm.entities.iter().enumerate() {
        if let Ok(mut tf) = transforms.get_mut(*entity) {
            tf.translation = camera.transform_point(Vec3::new(i as f32 * 0.002, 0.0, -1.0));
        }
    }
}
