//! Slice E — pooled effects: tracers, impact sparks, wood chips, hit bursts,
//! shield shimmer and break, piece-break debris, the elimination burst, camera
//! shake and hitstop. (The muzzle flash lives on the viewmodel layer, in
//! [`crate::viewmodel`].)
//!
//! Every mesh and material is built once at startup, and every effect draws
//! from fixed pools of hidden entities sized by [`FeedbackTuning`]'s caps
//! (`max_particles`, `max_debris`). When a pool is full the oldest effect is
//! recycled, so the frame cost is bounded no matter how much is going on.
//! Per frame this only moves transforms and swaps handles.

pub mod sim;

use crate::{
    building::{Piece, ramp_surface_height},
    palette,
    render::{CameraFollowSet, MainCamera},
    shared::{
        Character, DamageDealt, DamageTarget, Eliminated, PieceChange, PieceChanged, PieceKind,
        Player, ShotFired, WeaponKind,
    },
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
use sim::{FxRng, Hitstop, Particle, Shake, SlotPool, fade_step, tracer_segment};

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
const TRACER_POOL: usize = 64;
const SHIMMER_POOL: usize = 6;
/// Longest visible tracer streak (m).
const MAX_STREAK: f32 = 14.0;
/// Piece breaks closer than this shake the camera (m).
const SHAKE_RADIUS: f32 = 10.0;
/// Hard ceilings on the pools, whatever the settings file says.
const PARTICLE_CEILING: u32 = 2000;
const DEBRIS_CEILING: u32 = 1000;

/// Translucent effect colors that fade out through [`FADE_STEPS`] materials.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Spark,
    Tracer,
    White,
    Yellow,
    Shield,
    ShieldShell,
    Dust,
    Rim,
}

const FAMILIES: [Family; 8] = [
    Family::Spark,
    Family::Tracer,
    Family::White,
    Family::Yellow,
    Family::Shield,
    Family::ShieldShell,
    Family::Dust,
    Family::Rim,
];

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
            Family::Spark => (Color::srgb(1.0, 0.9, 0.5), AlphaMode::Blend, 1.0, false),
            Family::Tracer => (Color::srgb(1.0, 0.88, 0.5), AlphaMode::Blend, 0.95, false),
            Family::White => (palette::HIT_WHITE, AlphaMode::Add, 0.8, true),
            Family::Yellow => (palette::HEADSHOT, AlphaMode::Add, 0.95, true),
            Family::Shield => (shade(palette::SHIELD, 1.2), AlphaMode::Blend, 0.9, false),
            Family::ShieldShell => (palette::SHIELD, AlphaMode::Blend, 0.38, true),
            Family::Dust => (shade(palette::SAND, 1.1), AlphaMode::Blend, 0.32, true),
            Family::Rim => (palette::TARGET, AlphaMode::Blend, 0.85, false),
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
    shard: Handle<Mesh>,
    spark: Handle<Mesh>,
    ico: Handle<Mesh>,
    ring: Handle<Mesh>,
    wood: [Handle<StandardMaterial>; 4],
    coral: [Handle<StandardMaterial>; 3],
    hot_white: Handle<StandardMaterial>,
    hot_coral: Handle<StandardMaterial>,
    hot_yellow: Handle<StandardMaterial>,
    hot_shield: Handle<StandardMaterial>,
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

struct Tracer {
    start: Vec3,
    dir: Vec3,
    length: f32,
    age: f32,
    life: f32,
    width: f32,
    fresh: bool,
    step: usize,
}

struct Shimmer {
    target: Entity,
    age: f32,
    life: f32,
    fresh: bool,
    step: usize,
}

#[derive(Resource)]
struct FxPools {
    particles: ParticlePool,
    debris: ParticlePool,
    tracer_slots: SlotPool,
    tracer_entities: Vec<Entity>,
    tracers: Vec<Option<Tracer>>,
    shimmer_slots: SlotPool,
    shimmer_entities: Vec<Entity>,
    shimmers: Vec<Option<Shimmer>>,
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
            .add_systems(Last, end_hitstop_frame);
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
    let shard = meshes.add(unit_mesh(|m| {
        let zy = [
            Vec2::new(-0.5, -0.45),
            Vec2::new(0.5, -0.25),
            Vec2::new(0.05, 0.5),
        ];
        m.prism_x(&zy, -0.5, 0.5, white);
    }));
    let spark = meshes.add(unit_mesh(|m| {
        m.cube(Vec3::splat(-0.5), Vec3::splat(0.5), white)
    }));
    let tracer = meshes.add(unit_mesh(|m| {
        m.cube(Vec3::new(-0.5, -0.5, 0.0), Vec3::new(0.5, 0.5, 1.0), white)
    }));
    let ico = meshes.add(unit_mesh(|m| m.geosphere(1.0, 2, linear(white, 1.0))));
    let ring = meshes.add(unit_mesh(|m| m.ring_xz(0.72, 1.0, 32, linear(white, 1.0))));

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
    let coral = [
        lit(palette::TARGET),
        lit(palette::TARGET_DARK),
        lit(palette::TARGET_RIM),
    ];
    let mut unlit = |color: Color| {
        materials.add(StandardMaterial {
            base_color: color,
            unlit: true,
            ..default()
        })
    };
    let hot_white = unlit(palette::HIT_WHITE);
    let hot_coral = unlit(palette::TARGET);
    let hot_yellow = unlit(palette::HEADSHOT);
    let hot_shield = unlit(shade(palette::SHIELD, 1.25));
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
        &spark,
        &hot_white,
    );
    let debris = spawn_pool(
        feedback.max_debris.min(DEBRIS_CEILING) as usize,
        &chunk,
        &wood[0],
    );
    let tracer_entities = spawn_pool(TRACER_POOL, &tracer, &fades[Family::Tracer.index()][0]);
    let shimmer_entities = spawn_pool(SHIMMER_POOL, &ico, &fades[Family::ShieldShell.index()][0]);

    // Draw every effect material once, too small to see, while the game boots,
    // so their pipelines are compiled before the first shot instead of during it.
    let mut warm: Vec<Handle<StandardMaterial>> =
        fades.iter().map(|steps| steps[0].clone()).collect();
    warm.extend([wood[0].clone(), coral[0].clone(), hot_white.clone()]);
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
        tracer_slots: SlotPool::new(TRACER_POOL),
        tracers: (0..TRACER_POOL).map(|_| None).collect(),
        tracer_entities,
        shimmer_slots: SlotPool::new(SHIMMER_POOL),
        shimmers: (0..SHIMMER_POOL).map(|_| None).collect(),
        shimmer_entities,
    });
    commands.insert_resource(FxAssets {
        chunk,
        wedge,
        shard,
        spark,
        ico,
        ring,
        wood,
        coral,
        hot_white,
        hot_coral,
        hot_yellow,
        hot_shield,
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

    fn tracer(&mut self, start: Vec3, end: Vec3, width: f32, life: f32) {
        let d = end - start;
        let length = d.length();
        if length < 0.3 {
            return;
        }
        if let Some(i) = self.pools.tracer_slots.alloc(TRACER_POOL) {
            self.pools.tracers[i] = Some(Tracer {
                start,
                dir: d / length,
                length,
                age: 0.0,
                life,
                width,
                fresh: true,
                step: usize::MAX,
            });
        }
    }

    fn pop(&mut self, pos: Vec3, size: f32, grow: f32, life: f32, family: Family) {
        let p = Particle {
            pos,
            size: Vec3::splat(size),
            birth_scale: 0.45,
            grow,
            life,
            shrink_start: 1.0,
            ..default()
        };
        let mesh = self.assets.ico.clone();
        self.particle(p, mesh, Paint::Fade(family));
    }

    fn ring(&mut self, pos: Vec3, facing: Vec3, size: f32, grow: f32, life: f32, family: Family) {
        let p = Particle {
            pos,
            rot: Quat::from_rotation_arc(Vec3::Y, facing.normalize_or(Vec3::Y)),
            size: Vec3::splat(size),
            grow,
            life,
            shrink_start: 1.0,
            ..default()
        };
        let mesh = self.assets.ring.clone();
        self.particle(p, mesh, Paint::Fade(family));
    }

    /// Hot sparks off world geometry.
    fn sparks(&mut self, point: Vec3, normal: Vec3, count: usize) {
        let normal = normal.normalize_or(Vec3::Y);
        for _ in 0..count {
            let dir = self.rng.cone(normal, 0.9);
            let p = Particle {
                pos: point + normal * 0.02,
                vel: dir * self.rng.range(4.0, 10.0),
                gravity: 16.0,
                drag: 3.0,
                life: self.rng.range(0.12, 0.26),
                size: Vec3::new(0.028, 0.028, 0.03),
                stretch: 0.02,
                shrink_start: 0.3,
                ..default()
            };
            let mesh = self.assets.spark.clone();
            self.particle(p, mesh, Paint::Fade(Family::Spark));
        }
        self.pop(point + normal * 0.03, 0.11, 2.0, 0.08, Family::Spark);
    }

    /// Wood chips and a puff of dust off a building piece.
    fn chips(&mut self, point: Vec3, normal: Vec3, count: usize) {
        let normal = normal.normalize_or(Vec3::Y);
        for _ in 0..count {
            let dir = self.rng.cone(normal, 0.75);
            let size = Vec3::new(
                self.rng.range(0.03, 0.06),
                self.rng.range(0.02, 0.035),
                self.rng.range(0.05, 0.10),
            );
            let p = Particle {
                pos: point + normal * 0.03,
                vel: dir * self.rng.range(2.0, 5.0) + Vec3::Y * self.rng.range(0.5, 2.0),
                rot: Quat::from_scaled_axis(self.rng.dir() * 3.0),
                spin: self.rng.dir() * self.rng.range(8.0, 20.0),
                gravity: 18.0,
                bounce: Some(0.3),
                radius: size.y * 0.5,
                size,
                life: self.rng.range(0.5, 0.9),
                shrink_start: 0.6,
                ..default()
            };
            let k = self.rng.pick(4);
            let (mesh, paint) = (
                self.assets.chunk.clone(),
                Paint::Solid(self.assets.wood[k].clone()),
            );
            self.particle(p, mesh, paint);
        }
        let dust = Particle {
            pos: point + normal * 0.05,
            vel: normal * 0.6 + Vec3::Y * 0.3,
            size: Vec3::splat(0.1),
            birth_scale: 0.6,
            grow: 2.4,
            drag: 3.0,
            life: 0.3,
            shrink_start: 1.0,
            ..default()
        };
        let mesh = self.assets.ico.clone();
        self.particle(dust, mesh, Paint::Fade(Family::Dust));
    }

    /// Small bits of a character hit.
    fn bits(&mut self, point: Vec3, normal: Vec3, count: usize, headshot: bool, speed: f32) {
        let normal = normal.normalize_or(Vec3::Y);
        for i in 0..count {
            let dir = self.rng.cone(normal, 0.85);
            let s = self.rng.range(0.025, 0.045);
            let p = Particle {
                pos: point + normal * 0.03,
                vel: dir * self.rng.range(0.4, 1.0) * speed,
                rot: Quat::from_scaled_axis(self.rng.dir() * 3.0),
                spin: self.rng.dir() * self.rng.range(10.0, 25.0),
                gravity: 9.0,
                drag: 2.0,
                life: self.rng.range(0.18, 0.32),
                size: Vec3::splat(s),
                shrink_start: 0.4,
                ..default()
            };
            let material = if headshot {
                self.assets.hot_yellow.clone()
            } else if i % 2 == 0 {
                self.assets.hot_coral.clone()
            } else {
                self.assets.hot_white.clone()
            };
            let mesh = self.assets.chunk.clone();
            self.particle(p, mesh, Paint::Solid(material));
        }
    }

    /// The coral/white pop on a character hit (yellow flash for headshots).
    fn hit_burst(&mut self, point: Vec3, normal: Vec3, headshot: bool) {
        let normal = normal.normalize_or(Vec3::Y);
        if headshot {
            self.pop(point + normal * 0.05, 0.15, 2.1, 0.12, Family::Yellow);
            self.bits(point, normal, 10, true, 6.0);
        } else {
            self.pop(point + normal * 0.05, 0.09, 1.9, 0.08, Family::White);
            self.bits(point, normal, 6, false, 5.0);
        }
    }

    fn shimmer(&mut self, target: Entity) {
        let pools = &mut *self.pools;
        if let Some(existing) = pools
            .shimmers
            .iter_mut()
            .flatten()
            .find(|s| s.target == target)
        {
            existing.age = 0.0;
            return;
        }
        if let Some(i) = pools.shimmer_slots.alloc(SHIMMER_POOL) {
            pools.shimmers[i] = Some(Shimmer {
                target,
                age: 0.0,
                life: 0.18,
                fresh: true,
                step: usize::MAX,
            });
        }
    }

    fn shield_break(&mut self, point: Vec3, center: Vec3, eye: Vec3) {
        self.pop(point, 0.32, 2.6, 0.16, Family::Shield);
        self.ring(center, eye - center, 0.4, 4.0, 0.24, Family::Shield);
        let out = (point - center).normalize_or(Vec3::Y);
        for _ in 0..18 {
            let dir = (self.rng.dir() + out * 0.8).normalize_or(out);
            let p = Particle {
                pos: point + self.rng.dir() * 0.15,
                vel: dir * self.rng.range(3.0, 6.5) + Vec3::Y * 1.5,
                rot: Quat::from_scaled_axis(self.rng.dir() * 3.0),
                spin: self.rng.dir() * self.rng.range(10.0, 25.0),
                gravity: 12.0,
                drag: 1.0,
                life: self.rng.range(0.35, 0.6),
                size: Vec3::new(
                    0.018,
                    self.rng.range(0.09, 0.14),
                    self.rng.range(0.09, 0.14),
                ),
                shrink_start: 0.5,
                ..default()
            };
            let mesh = self.assets.shard.clone();
            let paint = Paint::Solid(self.assets.hot_shield.clone());
            self.particle(p, mesh, paint);
        }
    }

    /// The signature moment: a destroyed piece shatters into chunky planks that
    /// tumble, bounce and shrink away, with splinters and dust.
    fn piece_debris(&mut self, piece: Option<Piece>, kind: PieceKind, center: Vec3, eye: Vec3) {
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
            let size = Vec3::new(
                self.rng.range(0.5, 0.95),
                self.rng.range(0.2, 0.32),
                self.rng.range(0.09, 0.14),
            );
            let tilt = Quat::from_scaled_axis(self.rng.dir() * self.rng.range(0.0, 0.35));
            let vel = away * self.rng.range(1.5, 4.0)
                + (pos - center).normalize_or_zero() * self.rng.range(1.0, 2.5)
                + Vec3::Y * self.rng.range(2.0, 5.0);
            let p = Particle {
                pos,
                vel,
                rot: frame.rotation * lie * tilt,
                spin: self.rng.dir() * self.rng.range(3.0, 11.0),
                gravity: 20.0,
                drag: 0.2,
                bounce: Some(0.35),
                radius: size.z * 0.5,
                size,
                life: self.rng.range(1.05, 1.35),
                shrink_start: 0.72,
                ..default()
            };
            let mesh = if self.rng.f() < 0.3 {
                self.assets.wedge.clone()
            } else {
                self.assets.chunk.clone()
            };
            let k = [0, 0, 1, 2, 3][self.rng.pick(5)];
            let paint = Paint::Solid(self.assets.wood[k].clone());
            self.chunk(p, mesh, paint);
        }
        for _ in 0..16 {
            let local = match kind {
                PieceKind::Wall => {
                    Vec3::new(self.rng.range(-1.8, 1.8), self.rng.range(-1.3, 1.3), 0.0)
                }
                _ => Vec3::new(self.rng.range(-1.8, 1.8), 0.1, self.rng.range(-1.8, 1.8)),
            };
            let pos = frame.transform_point(local);
            let dir = (self.rng.dir() + away * 0.6 + Vec3::Y * 0.5).normalize_or(Vec3::Y);
            let size = Vec3::new(
                self.rng.range(0.04, 0.08),
                self.rng.range(0.025, 0.04),
                self.rng.range(0.12, 0.22),
            );
            let p = Particle {
                pos,
                vel: dir * self.rng.range(3.0, 7.0),
                rot: Quat::from_scaled_axis(self.rng.dir() * 3.0),
                spin: self.rng.dir() * self.rng.range(8.0, 20.0),
                gravity: 18.0,
                bounce: Some(0.3),
                radius: size.y * 0.5,
                size,
                life: self.rng.range(0.6, 1.0),
                shrink_start: 0.6,
                ..default()
            };
            let k = self.rng.pick(4);
            let (mesh, paint) = (
                self.assets.chunk.clone(),
                Paint::Solid(self.assets.wood[k].clone()),
            );
            self.particle(p, mesh, paint);
        }
        for _ in 0..4 {
            let local = match kind {
                PieceKind::Wall => {
                    Vec3::new(self.rng.range(-1.4, 1.4), self.rng.range(-1.0, 1.0), 0.0)
                }
                _ => Vec3::new(self.rng.range(-1.4, 1.4), 0.2, self.rng.range(-1.4, 1.4)),
            };
            let p = Particle {
                pos: frame.transform_point(local),
                vel: Vec3::Y * 0.5 + self.rng.dir() * 0.5,
                size: Vec3::splat(0.3),
                birth_scale: 0.5,
                grow: 2.4,
                drag: 2.5,
                life: 0.55,
                shrink_start: 1.0,
                ..default()
            };
            let mesh = self.assets.ico.clone();
            self.particle(p, mesh, Paint::Fade(Family::Dust));
        }
    }

    /// Coral chunks, a flash and an expanding ring, readable across the arena.
    fn elimination(&mut self, feet: Vec3, eye: Vec3) {
        let center = feet + Vec3::Y * 0.95;
        self.pop(center, 0.42, 2.2, 0.18, Family::White);
        self.ring(center, eye - center, 0.5, 7.0, 0.42, Family::Rim);
        self.ring(feet + Vec3::Y * 0.06, Vec3::Y, 0.4, 8.0, 0.5, Family::Rim);
        for _ in 0..24 {
            let dir = (self.rng.dir() + Vec3::Y * 0.7).normalize_or(Vec3::Y);
            let s = self.rng.range(0.11, 0.22);
            let size = Vec3::new(
                s * self.rng.range(0.8, 1.2),
                s,
                s * self.rng.range(0.8, 1.2),
            );
            let p = Particle {
                pos: center + self.rng.dir() * self.rng.range(0.0, 0.35),
                vel: dir * self.rng.range(3.5, 8.0),
                rot: Quat::from_scaled_axis(self.rng.dir() * 3.0),
                spin: self.rng.dir() * self.rng.range(4.0, 14.0),
                gravity: 20.0,
                drag: 0.2,
                bounce: Some(0.4),
                radius: s * 0.5,
                size,
                life: self.rng.range(1.0, 1.35),
                shrink_start: 0.7,
                ..default()
            };
            let mesh = if self.rng.f() < 0.35 {
                self.assets.wedge.clone()
            } else {
                self.assets.chunk.clone()
            };
            let k = self.rng.pick(3);
            let paint = Paint::Solid(self.assets.coral[k].clone());
            self.chunk(p, mesh, paint);
        }
        self.bits(center, Vec3::Y, 16, false, 8.0);
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Camera shake on your own pump shots and nearby piece breaks, as a small
/// rotation offset applied after the camera has followed the eye.
fn shake_camera(
    time: Res<Time>,
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

fn emit_fx(
    tuning: Res<Tuning>,
    assets: Option<Res<FxAssets>>,
    pools: Option<ResMut<FxPools>>,
    mut state: ResMut<FxState>,
    mut removed: ResMut<RemovedPieces>,
    muzzle: Res<MuzzlePoint>,
    mut virtual_time: ResMut<Time<Virtual>>,
    mut shots: MessageReader<ShotFired>,
    mut damage: MessageReader<DamageDealt>,
    mut changes: MessageReader<PieceChanged>,
    mut eliminated: MessageReader<Eliminated>,
    player: Option<Single<(Entity, &Transform), With<Player>>>,
    pieces: Query<(), With<Piece>>,
    characters: Query<&Transform, With<Character>>,
    exists: Query<()>,
) {
    let (Some(assets), Some(mut pools)) = (assets, pools) else {
        return;
    };
    let feedback = &tuning.feedback;
    let (me, eye) = player
        .map(|p| (Some(p.0), p.1.translation + Vec3::Y * 1.6))
        .unwrap_or((None, Vec3::ZERO));
    let FxState { hitstop, rng, .. } = &mut *state;
    let mut fx = Emitter {
        pools: &mut pools,
        assets: &assets,
        rng,
        particle_limit: feedback.max_particles as usize,
        debris_limit: feedback.max_debris as usize,
    };

    for shot in shots.read() {
        let pump = shot.weapon == WeaponKind::Pump;
        let first_dir = shot
            .traces
            .first()
            .map(|t| (t.end - shot.origin).normalize_or(Vec3::NEG_Z))
            .unwrap_or(Vec3::NEG_Z);
        let start = if Some(shot.shooter) == me {
            muzzle
                .0
                .unwrap_or(shot.origin + first_dir * 0.5 - Vec3::Y * 0.12)
        } else {
            shot.origin + first_dir * 0.5
        };
        let (width, life) = if pump { (0.018, 0.075) } else { (0.03, 0.085) };
        for trace in &shot.traces {
            fx.tracer(start, trace.end, width, life);
            let Some(hit) = trace.hit else {
                continue;
            };
            if characters.contains(hit) {
                if pump {
                    fx.bits(trace.end, trace.normal, 2, false, 4.0);
                }
            } else if pieces.contains(hit) || !exists.contains(hit) {
                fx.chips(trace.end, trace.normal, if pump { 2 } else { 5 });
            } else {
                fx.sparks(trace.end, trace.normal, if pump { 3 } else { 8 });
            }
        }
    }

    for hit in damage.read() {
        if hit.target_kind != DamageTarget::Character {
            continue;
        }
        fx.hit_burst(hit.point, hit.normal, hit.headshot);
        let center = characters
            .get(hit.target)
            .map(|t| t.translation + Vec3::Y * 0.92)
            .unwrap_or(hit.point);
        if hit.to_shield > 0.0 {
            fx.shimmer(hit.target);
        }
        if hit.shield_broke {
            fx.shield_break(hit.point, center, eye);
        }
    }

    for change in changes.read() {
        if change.change == PieceChange::Destroyed {
            let piece = removed.0.get(&change.entity).map(|(p, _)| *p);
            fx.piece_debris(piece, change.kind, change.center, eye);
        }
    }

    for elimination in eliminated.read() {
        fx.elimination(elimination.position, eye);
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
    time: Res<Time>,
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
    characters: Query<&Transform, (With<Character>, Without<FxEntity>)>,
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

    for i in 0..pools.tracers.len() {
        let Some(tracer) = pools.tracers[i].as_mut() else {
            continue;
        };
        let Ok((mut tf, mut vis, _, mut material)) = q.get_mut(pools.tracer_entities[i]) else {
            continue;
        };
        if tracer.fresh {
            tracer.fresh = false;
        } else {
            tracer.age += dt;
        }
        let Some((tail, head)) = tracer_segment(tracer.age, tracer.life, tracer.length, MAX_STREAK)
        else {
            vis.set_if_neq(Visibility::Hidden);
            pools.tracers[i] = None;
            pools.tracer_slots.free(i);
            continue;
        };
        let t = tracer.age / tracer.life;
        let step = fade_step(t, FADE_STEPS);
        if step != tracer.step {
            tracer.step = step;
            material.0 = assets.fade(Family::Tracer, step);
        }
        let width = tracer.width * (1.0 - 0.4 * t);
        tf.translation = tracer.start + tracer.dir * tail;
        tf.rotation = Quat::from_rotation_arc(Vec3::Z, tracer.dir);
        tf.scale = Vec3::new(width, width, head - tail);
        vis.set_if_neq(Visibility::Visible);
    }

    for i in 0..pools.shimmers.len() {
        let Some(shimmer) = pools.shimmers[i].as_mut() else {
            continue;
        };
        let Ok((mut tf, mut vis, _, mut material)) = q.get_mut(pools.shimmer_entities[i]) else {
            continue;
        };
        if shimmer.fresh {
            shimmer.fresh = false;
        } else {
            shimmer.age += dt;
        }
        let target = characters.get(shimmer.target).ok();
        let (Some(target), true) = (target, shimmer.age < shimmer.life) else {
            vis.set_if_neq(Visibility::Hidden);
            pools.shimmers[i] = None;
            pools.shimmer_slots.free(i);
            continue;
        };
        let t = shimmer.age / shimmer.life;
        let step = fade_step(t, FADE_STEPS);
        if step != shimmer.step {
            shimmer.step = step;
            material.0 = assets.fade(Family::ShieldShell, step);
        }
        tf.translation = target.translation + Vec3::Y * 0.92;
        tf.rotation = Quat::IDENTITY;
        tf.scale = Vec3::new(0.56, 1.0, 0.56) * (1.0 + 0.14 * (1.0 - (1.0 - t) * (1.0 - t)));
        vis.set_if_neq(Visibility::Visible);
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
