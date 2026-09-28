//! The knights' spell orbs (docs/M3-SPEC.md → The orb): a hot orange-red
//! fireball with a white-gold heart, a flame streak and a glow halo, trailing
//! flame sparkles; a flare at the wand tip when it leaves; on impact a small
//! orange burst (with brick chips or splinters off a piece, reusing the M2
//! chip shapes) or a fizzle puff on the ground. Everything is fire colours —
//! orange, red and gold — so an incoming orb never reads as the player's blue
//! bolts.
//!
//! **Pooling.** Each simulation orb (a fixed pool, `orb::ORB_POOL`) gets its
//! head, streak and halo entities once, the first frame it exists; trail
//! sparks, bursts, chips and puffs draw from fixed pools ([`GLOW_POOL`],
//! [`SOLID_POOL`], [`FLASH_POOL`]) made at startup, recycling the oldest when
//! full. No mesh or material is made after startup; per frame this only moves
//! transforms and writes `MeshTag`s.
//!
//! **Look.** Glows share one additive [`SpellMaterial`] asset (colour in the
//! mesh, tint and fade in the `MeshTag`), chips and puffs the toon material;
//! both are registered with the pipeline warm-up so the first orb never
//! hitches. The orb head is drawn where the simulation put it this frame,
//! interpolated between fixed ticks. Every effect clock reads
//! [`FreezableTime`], so the gallery can hold a moment.

use super::{
    material::{SpellMaterial, glow_tag},
    shapes::{brick_chip, puff, sparkle, sparkle_cross, starburst, streak, wood_splinter},
    sim::{FxRng, Particle, SlotPool, apparent_size, axial_billboard, billboard},
};
use crate::{
    look::{Halo, NoOutline, ToonMaterial, warmup::Warmup},
    orb::{Orb, OrbHit, OrbImpact, OrbSlot, Wand, wand_tip},
    render::{CameraFollowSet, MainCamera},
    shared::{FreezableTime, GameCue, LookAngles, PieceKind},
    viewmodel::ViewmodelSet,
};
use bevy::{
    camera::visibility::VisibilitySystems, light::NotShadowCaster, mesh::MeshTag, prelude::*,
};

/// Trail sparks, bursts and flares at once (16 orbs in flight trail about 200).
pub const GLOW_POOL: usize = 384;
/// Chips and puffs at once.
pub const SOLID_POOL: usize = 48;
/// Stand-alone glow flashes at once.
pub const FLASH_POOL: usize = 16;

/// Fire colours (sRGB): a white-gold heart, hot orange, and red-orange tips.
pub const FIRE_CORE: Color = Color::srgb(1.0, 0.93, 0.62);
pub const FIRE: Color = Color::srgb(1.0, 0.52, 0.1);
pub const FIRE_RED: Color = Color::srgb(0.96, 0.2, 0.06);
/// The orb's and flashes' glow halo.
pub const HALO_FIRE: Color = Color::srgb(1.0, 0.42, 0.12);

/// The fireball's size (m; at least this wide an angle, rad, far off).
pub const HEAD_SIZE: f32 = 0.55;
pub const HEAD_ANGLE: f32 = 0.022;
/// The flame streak behind it (m).
pub const STREAK_LENGTH: f32 = 1.5;
pub const STREAK_WIDTH: f32 = 0.5;
/// A trail spark every this many metres of flight.
pub const TRAIL_SPACING: f32 = 0.22;

/// Marks a pooled orb effect entity (hidden while unused).
#[derive(Component, Debug, Clone, Copy)]
pub struct OrbFx;

/// On a simulation orb once its head, streak and halo exist.
#[derive(Component, Debug, Clone, Copy)]
struct OrbDressed;

/// Every mesh and material the orb effects use, made once at startup.
#[derive(Resource, Debug, Clone)]
pub struct OrbFxAssets {
    pub material: Handle<SpellMaterial>,
    pub toon: Handle<ToonMaterial>,
    pub cloud: Handle<ToonMaterial>,
    pub head: Handle<Mesh>,
    pub heart: Handle<Mesh>,
    pub streak: Handle<Mesh>,
    pub sparkle: Handle<Mesh>,
    pub ember: Handle<Mesh>,
    pub burst: Handle<Mesh>,
    pub brick_chip: Handle<Mesh>,
    pub splinter: Handle<Mesh>,
    pub puffs: [Handle<Mesh>; 2],
}

/// How a pooled shape turns.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Face {
    /// Faces the camera, rolled about the view axis.
    Billboard { roll: f32, spin: f32 },
    /// Long along its velocity, turned to face the camera.
    Streak,
    /// Tumbles with the particle's own rotation.
    Free,
}

#[derive(Debug, Clone)]
struct Spark {
    p: Particle,
    mesh: Handle<Mesh>,
    material: Option<Handle<ToonMaterial>>,
    face: Face,
    tint: Color,
    intensity: f32,
    /// Fraction of life after which the glow fades out.
    fade_start: f32,
    fresh: bool,
    tag: u32,
}

impl Spark {
    fn new(pos: Vec3, mesh: &Handle<Mesh>) -> Self {
        Self {
            p: Particle {
                pos,
                shrink_start: 0.5,
                ..default()
            },
            mesh: mesh.clone(),
            material: None,
            face: Face::Billboard {
                roll: 0.0,
                spin: 0.0,
            },
            tint: Color::WHITE,
            intensity: 1.0,
            fade_start: 0.5,
            fresh: true,
            tag: u32::MAX,
        }
    }

    fn glow(&self) -> f32 {
        let t = (self.p.age / self.p.life.max(1e-4)).clamp(0.0, 1.0);
        let fade = if t <= self.fade_start {
            1.0
        } else {
            1.0 - (t - self.fade_start) / (1.0 - self.fade_start).max(1e-4)
        };
        self.intensity * fade.clamp(0.0, 1.0)
    }
}

/// A stand-alone fading glow.
#[derive(Debug, Clone, Copy)]
struct Flash {
    pos: Vec3,
    size: f32,
    intensity: f32,
    age: f32,
    life: f32,
    fresh: bool,
}

struct Pool<T> {
    slots: SlotPool,
    entities: Vec<Entity>,
    live: Vec<Option<T>>,
}

impl<T> Pool<T> {
    fn new(entities: Vec<Entity>) -> Self {
        Self {
            slots: SlotPool::new(entities.len()),
            live: (0..entities.len()).map(|_| None).collect(),
            entities,
        }
    }

    fn put(&mut self, item: T) {
        if let Some(i) = self.slots.alloc(self.entities.len()) {
            self.live[i] = Some(item);
        }
    }

    fn free(&mut self, i: usize) {
        self.live[i] = None;
        self.slots.free(i);
    }
}

/// One simulation orb's own shapes.
#[derive(Debug, Clone, Copy)]
struct OrbLook {
    orb: Entity,
    head: Entity,
    heart: Entity,
    streak: Entity,
    halo: Entity,
    /// Metres flown since the last trail spark.
    trail: f32,
    last: Option<Vec3>,
    roll: f32,
}

#[derive(Resource)]
struct OrbFxPools {
    glow: Pool<Spark>,
    solid: Pool<Spark>,
    flashes: Pool<Flash>,
    orbs: Vec<OrbLook>,
    rng: FxRng,
}

/// Spells' orange sibling (client). Added by `FxPlugin` after the spells
/// (it needs their material type).
pub struct OrbFxPlugin;

impl Plugin for OrbFxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_orb_fx)
            .add_systems(Update, dress_orbs)
            .add_systems(
                PostUpdate,
                (emit_orb_fx, simulate_orb_fx)
                    .chain()
                    .after(CameraFollowSet)
                    .after(ViewmodelSet)
                    .before(TransformSystems::Propagate)
                    .before(VisibilitySystems::CalculateBounds),
            );
    }
}

fn setup_orb_fx(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut spell: ResMut<Assets<SpellMaterial>>,
    mut toon: ResMut<Assets<ToonMaterial>>,
    mut warmup: Warmup,
) {
    let mut add = |m: Mesh| meshes.add(m);
    let assets = OrbFxAssets {
        material: spell.add(SpellMaterial::default()),
        toon: toon.add(ToonMaterial::vertex_colored()),
        cloud: toon.add(ToonMaterial::vertex_colored().with_rim(0.5)),
        head: add(starburst(11, 0x0B1, FIRE, &[FIRE_RED, FIRE, FIRE_RED], 0.2)),
        heart: add(sparkle(FIRE_CORE, Color::WHITE)),
        streak: add(streak(FIRE_CORE, FIRE, FIRE_RED)),
        sparkle: add(sparkle(FIRE, FIRE_CORE)),
        ember: add(sparkle_cross(FIRE_RED, FIRE)),
        burst: add(starburst(
            13,
            0x0B2,
            FIRE,
            &[FIRE_RED, FIRE, FIRE_CORE],
            0.12,
        )),
        brick_chip: add(brick_chip()),
        splinter: add(wood_splinter()),
        puffs: [add(puff(0x0B3)), add(puff(0x0B4))],
    };
    let glow_entity = |commands: &mut Commands, name: &'static str| {
        commands
            .spawn((
                Name::new(name),
                OrbFx,
                Mesh3d(assets.sparkle.clone()),
                MeshMaterial3d(assets.material.clone()),
                glow_tag(Color::WHITE, 0.0),
                Transform::default(),
                Visibility::Hidden,
                NotShadowCaster,
                NoOutline,
            ))
            .id()
    };
    let glow = (0..GLOW_POOL)
        .map(|_| glow_entity(&mut commands, "Orb spark"))
        .collect();
    let solid = (0..SOLID_POOL)
        .map(|_| {
            commands
                .spawn((
                    Name::new("Orb chip"),
                    OrbFx,
                    Mesh3d(assets.brick_chip.clone()),
                    MeshMaterial3d(assets.toon.clone()),
                    Transform::default(),
                    Visibility::Hidden,
                    NotShadowCaster,
                    NoOutline,
                ))
                .id()
        })
        .collect();
    let flashes = (0..FLASH_POOL)
        .map(|_| {
            commands
                .spawn((
                    Name::new("Orb flash"),
                    OrbFx,
                    Halo::new(HALO_FIRE, 1.0, 0.0),
                    Transform::default(),
                    Visibility::Hidden,
                ))
                .id()
        })
        .collect();

    // Compiled behind the loading screen: the glow on the orb's meshes and
    // the toon chips and puffs (the spells warm the same pipelines; these
    // make sure of it for the orb's own assets).
    warmup.add_with(
        assets.head.clone(),
        assets.material.clone(),
        glow_tag(Color::WHITE, 1.0),
    );
    warmup.add(assets.brick_chip.clone(), assets.toon.clone());
    warmup.add(assets.puffs[0].clone(), assets.cloud.clone());

    commands.insert_resource(OrbFxPools {
        glow: Pool::new(glow),
        solid: Pool::new(solid),
        flashes: Pool::new(flashes),
        orbs: Vec::with_capacity(crate::orb::ORB_POOL),
        rng: FxRng::new(0x0B5),
    });
    commands.insert_resource(assets);
}

/// Gives each simulation orb its head, heart, streak and halo, once.
fn dress_orbs(
    mut commands: Commands,
    assets: Option<Res<OrbFxAssets>>,
    pools: Option<ResMut<OrbFxPools>>,
    orbs: Query<Entity, (With<OrbSlot>, Without<OrbDressed>)>,
) {
    let (Some(assets), Some(mut pools)) = (assets, pools) else {
        return;
    };
    for orb in &orbs {
        let glow = |commands: &mut Commands, name: &'static str, mesh: &Handle<Mesh>| {
            commands
                .spawn((
                    Name::new(name),
                    OrbFx,
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(assets.material.clone()),
                    glow_tag(Color::WHITE, 1.0),
                    Transform::default(),
                    Visibility::Hidden,
                    NotShadowCaster,
                    NoOutline,
                ))
                .id()
        };
        let head = glow(&mut commands, "Orb head", &assets.head);
        let heart = glow(&mut commands, "Orb heart", &assets.heart);
        let streak = glow(&mut commands, "Orb streak", &assets.streak);
        let halo = commands
            .spawn((
                Name::new("Orb halo"),
                OrbFx,
                Halo::new(HALO_FIRE, 1.6, 2.0),
                Transform::default(),
                Visibility::Hidden,
            ))
            .id();
        commands.entity(orb).insert(OrbDressed);
        pools.orbs.push(OrbLook {
            orb,
            head,
            heart,
            streak,
            halo,
            trail: 0.0,
            last: None,
            roll: 0.0,
        });
    }
}

/// The camera, or a stand-in.
#[derive(Clone, Copy)]
struct View {
    at: Vec3,
    rotation: Quat,
}

struct Emitter<'a> {
    pools: &'a mut OrbFxPools,
    assets: &'a OrbFxAssets,
    view: View,
}

impl Emitter<'_> {
    fn rng(&mut self) -> &mut FxRng {
        &mut self.pools.rng
    }

    fn dist(&self, p: Vec3) -> f32 {
        self.view.at.distance(p)
    }

    fn flash(&mut self, pos: Vec3, size: f32, intensity: f32, life: f32) {
        self.pools.flashes.put(Flash {
            pos,
            size,
            intensity,
            age: 0.0,
            life,
            fresh: true,
        });
    }

    /// A flat glowing pop facing the camera.
    fn pop(&mut self, pos: Vec3, mesh: Handle<Mesh>, size: f32, life: f32, intensity: f32) {
        let roll = self.rng().range(0.0, std::f32::consts::TAU);
        let spin = self.rng().range(-5.0, 5.0);
        let mut s = Spark::new(pos, &mesh);
        s.p.size = Vec3::splat(size);
        s.p.life = life;
        s.p.birth_scale = 0.45;
        s.p.shrink_start = 0.4;
        s.face = Face::Billboard { roll, spin };
        s.intensity = intensity;
        s.fade_start = 0.5;
        self.pools.glow.put(s);
    }

    /// Glowing flame sparks flung from `pos` around `dir`.
    fn sparks(&mut self, pos: Vec3, dir: Vec3, spread: f32, count: usize, speed: (f32, f32)) {
        for i in 0..count {
            let d = self.rng().cone(dir, spread);
            let v = self.rng().range(speed.0, speed.1);
            let k = self.rng().range(0.75, 1.3);
            let life = self.rng().range(0.16, 0.3);
            let mesh = &self.assets.streak;
            let mut s = Spark::new(pos, mesh);
            s.p.vel = d * v;
            s.p.drag = 4.5;
            s.p.gravity = 5.0;
            s.p.life = life;
            s.p.size = Vec3::new(0.07, 1.0, 0.3) * k;
            s.p.stretch = 0.03;
            s.p.shrink_start = 0.35;
            s.face = Face::Streak;
            s.intensity = if i % 3 == 0 { 2.2 } else { 1.7 };
            s.fade_start = 0.4;
            self.pools.glow.put(s);
        }
    }

    /// Embers: small twinkling fire sparkles drifting up from `pos`.
    fn embers(&mut self, pos: Vec3, radius: f32, count: usize, size: f32) {
        for i in 0..count {
            let at = pos + self.rng().dir() * self.rng().range(0.2, 1.0) * radius;
            let mesh = if i % 2 == 0 {
                self.assets.sparkle.clone()
            } else {
                self.assets.ember.clone()
            };
            let roll = self.rng().range(0.0, std::f32::consts::TAU);
            let spin = self.rng().range(-4.0, 4.0);
            let drift = self.rng().dir() * 0.5 + Vec3::Y * 0.8;
            let life = self.rng().range(0.25, 0.45);
            let k = self.rng().range(0.7, 1.3);
            let mut s = Spark::new(at, &mesh);
            s.p.vel = drift;
            s.p.drag = 2.0;
            s.p.life = life;
            s.p.size = Vec3::splat(size * k);
            s.p.birth_scale = 0.3;
            s.p.shrink_start = 0.5;
            s.face = Face::Billboard { roll, spin };
            s.intensity = 1.6;
            self.pools.glow.put(s);
        }
    }

    /// One flame sparkle shed from an orb flying along `vel`.
    fn trail(&mut self, pos: Vec3, vel: Vec3) {
        let back = -vel.normalize_or(Vec3::NEG_Z);
        let jitter = self.rng().dir() * 0.07;
        let drift = back * self.rng().range(1.0, 2.5) + self.rng().dir() * 0.6 + Vec3::Y * 0.5;
        let mesh = match self.rng().pick(3) {
            0 => self.assets.ember.clone(),
            _ => self.assets.sparkle.clone(),
        };
        let roll = self.rng().range(0.0, std::f32::consts::TAU);
        let spin = self.rng().range(-6.0, 6.0);
        let life = self.rng().range(0.14, 0.26);
        let size = self.rng().range(0.13, 0.24);
        let size = apparent_size(size, self.dist(pos), 0.006);
        let mut s = Spark::new(pos + jitter, &mesh);
        s.p.vel = drift;
        s.p.drag = 3.0;
        s.p.life = life;
        s.p.size = Vec3::splat(size);
        s.p.shrink_start = 0.15;
        s.face = Face::Billboard { roll, spin };
        s.intensity = 1.8;
        s.fade_start = 0.3;
        self.pools.glow.put(s);
    }

    /// The flare at the wand tip as the orb leaves.
    fn cast(&mut self, tip: Vec3, dir: Vec3) {
        let d = self.dist(tip);
        let burst = self.assets.burst.clone();
        self.pop(tip, burst, apparent_size(0.45, d, 0.02), 0.14, 1.8);
        self.flash(tip, apparent_size(1.3, d, 0.05), 2.2, 0.16);
        self.sparks(tip, dir, 0.6, 6, (2.0, 5.0));
    }

    /// The small orange burst where an orb stops.
    fn burst(&mut self, point: Vec3, normal: Vec3) {
        let d = self.dist(point);
        let n = normal.normalize_or(Vec3::Y);
        let at = point + n * 0.1;
        let burst = self.assets.burst.clone();
        self.pop(at, burst, apparent_size(0.75, d, 0.03), 0.18, 1.9);
        let heart = self.assets.heart.clone();
        self.pop(at, heart, apparent_size(0.35, d, 0.014), 0.12, 2.2);
        self.flash(at, apparent_size(2.0, d, 0.07), 2.4, 0.22);
        self.sparks(at, n, 0.9, 10, (3.0, 7.0));
        self.embers(at, 0.25, 4, apparent_size(0.1, d, 0.008));
    }

    /// Chips off a piece: brick off walls, splinters off floors, ramps and
    /// cones (the M2 chip shapes), with a puff of dust.
    fn chips(&mut self, point: Vec3, normal: Vec3, kind: PieceKind) {
        let n = normal.normalize_or(Vec3::Y);
        let wall = kind == PieceKind::Wall;
        for _ in 0..5 {
            let dir = self.rng().cone(n, 0.7);
            let (mesh, size) = if wall {
                (self.assets.brick_chip.clone(), self.rng().range(0.07, 0.11))
            } else {
                (self.assets.splinter.clone(), self.rng().range(0.12, 0.2))
            };
            let speed = self.rng().range(2.5, 5.0);
            let up = self.rng().range(0.5, 2.0);
            let rot = Quat::from_scaled_axis(self.rng().dir() * 3.0);
            let spin = self.rng().dir() * self.rng().range(8.0, 20.0);
            let life = self.rng().range(0.55, 0.85);
            let mut s = Spark::new(point + n * 0.04, &mesh);
            s.material = Some(self.assets.toon.clone());
            s.p.vel = dir * speed + Vec3::Y * up;
            s.p.rot = rot;
            s.p.spin = spin;
            s.p.gravity = 18.0;
            s.p.bounce = Some(0.3);
            s.p.radius = size * 0.3;
            s.p.size = Vec3::splat(size);
            s.p.life = life;
            s.p.shrink_start = 0.6;
            s.face = Face::Free;
            self.pools.solid.put(s);
        }
        self.puff(point, n, 0.18, 0.3);
    }

    /// A soft cloud puff (dust off a piece, the fizzle on the ground).
    fn puff(&mut self, point: Vec3, n: Vec3, size: f32, life: f32) {
        let mesh = self.assets.puffs[self.rng().pick(2)].clone();
        let yaw = self.rng().range(0.0, std::f32::consts::TAU);
        let mut s = Spark::new(point + n * 0.08, &mesh);
        s.material = Some(self.assets.cloud.clone());
        s.p.vel = n * 0.6 + Vec3::Y * 0.3;
        s.p.drag = 3.0;
        s.p.life = life;
        s.p.size = Vec3::splat(size);
        s.p.birth_scale = 0.4;
        s.p.grow = 1.8;
        s.p.shrink_start = 0.3;
        s.p.rot = Quat::from_rotation_y(yaw);
        s.face = Face::Free;
        self.pools.solid.put(s);
    }

    /// An orb dying on the ground or a prop: a puff, a small pop and embers.
    fn fizzle(&mut self, point: Vec3, normal: Vec3) {
        let d = self.dist(point);
        let n = normal.normalize_or(Vec3::Y);
        self.puff(point, n, apparent_size(0.26, d, 0.015), 0.34);
        let at = point + n * 0.16;
        let burst = self.assets.burst.clone();
        self.pop(at, burst, apparent_size(0.4, d, 0.02), 0.14, 1.5);
        self.flash(at, apparent_size(1.2, d, 0.05), 1.6, 0.16);
        self.embers(at, 0.2, 4, apparent_size(0.09, d, 0.008));
    }

    /// An orb at the end of its range: it gutters out in the air.
    fn gutter(&mut self, point: Vec3) {
        let d = self.dist(point);
        let heart = self.assets.heart.clone();
        self.pop(point, heart, apparent_size(0.3, d, 0.012), 0.16, 1.2);
        self.embers(point, 0.3, 5, apparent_size(0.1, d, 0.008));
    }
}

fn camera_view(camera: Option<&Transform>) -> View {
    match camera {
        Some(c) => View {
            at: c.translation,
            rotation: c.rotation,
        },
        None => View {
            at: Vec3::new(0.0, 1.62, 0.0),
            rotation: Quat::IDENTITY,
        },
    }
}

/// Flares, bursts, chips and fizzles from this frame's casts and impacts.
fn emit_orb_fx(
    assets: Option<Res<OrbFxAssets>>,
    pools: Option<ResMut<OrbFxPools>>,
    camera: Option<Single<&Transform, (With<MainCamera>, Without<OrbFx>)>>,
    knights: Query<(&Transform, &LookAngles), (With<Wand>, Without<OrbFx>)>,
    mut cues: MessageReader<GameCue>,
    mut impacts: MessageReader<OrbImpact>,
) {
    let (Some(assets), Some(mut pools)) = (assets, pools) else {
        cues.clear();
        impacts.clear();
        return;
    };
    let view = camera_view(camera.as_deref().copied());
    let mut fx = Emitter {
        pools: &mut pools,
        assets: &assets,
        view,
    };
    for cue in cues.read() {
        if let GameCue::OrbFired { who } = *cue
            && let Ok((knight, look)) = knights.get(who)
        {
            fx.cast(wand_tip(knight.translation, look.yaw), look.forward());
        }
    }
    for impact in impacts.read() {
        match impact.hit {
            // The player feels it (arrow, bonk, bars); a burst in his face
            // would only blind him.
            OrbHit::Player { .. } => {}
            OrbHit::Piece(kind) => {
                fx.burst(impact.point, impact.normal);
                fx.chips(impact.point, impact.normal, kind);
            }
            OrbHit::World => fx.fizzle(impact.point, impact.normal),
            OrbHit::Expired => fx.gutter(impact.point),
        }
    }
}

type GlowQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Transform,
        &'static mut Visibility,
        Option<&'static mut Mesh3d>,
        Option<&'static mut MeshTag>,
        Option<&'static mut MeshMaterial3d<ToonMaterial>>,
        Option<&'static mut Halo>,
    ),
    (With<OrbFx>, Without<MainCamera>, Without<Orb>),
>;

/// Moves every pooled shape and draws each orb in flight.
fn simulate_orb_fx(
    time: FreezableTime,
    fixed: Res<Time<Fixed>>,
    assets: Option<Res<OrbFxAssets>>,
    pools: Option<ResMut<OrbFxPools>>,
    camera: Option<Single<&Transform, (With<MainCamera>, Without<OrbFx>)>>,
    orbs: Query<(&Orb, &Transform), Without<OrbFx>>,
    mut shapes: GlowQuery,
) {
    let (Some(assets), Some(mut pools)) = (assets, pools) else {
        return;
    };
    let dt = time.delta_secs();
    let alpha = fixed.overstep_fraction().clamp(0.0, 1.0);
    let view = camera_view(camera.as_deref().copied());
    let to_camera = |p: Vec3| (view.at - p).normalize_or(Vec3::Z);

    // The orbs themselves.
    let mut looks = std::mem::take(&mut pools.orbs);
    for look in &mut looks {
        let flying = orbs
            .get(look.orb)
            .ok()
            .map(|(orb, tf)| (orb.previous.lerp(tf.translation, alpha), orb.velocity));
        let parts = [look.head, look.heart, look.streak, look.halo];
        let Some((pos, vel)) = flying else {
            look.last = None;
            look.trail = 0.0;
            for part in parts {
                if let Ok((_, mut vis, ..)) = shapes.get_mut(part) {
                    vis.set_if_neq(Visibility::Hidden);
                }
            }
            continue;
        };
        // Trail sparks, evenly spaced along the path flown since last frame.
        if let Some(last) = look.last {
            let step = pos - last;
            let flown = step.length();
            look.trail += flown;
            let mut fx = Emitter {
                pools: &mut pools,
                assets: &assets,
                view,
            };
            while look.trail >= TRAIL_SPACING && flown > 1e-4 {
                look.trail -= TRAIL_SPACING;
                let back = (look.trail / flown).min(1.0);
                fx.trail(pos - step * back, vel);
            }
        }
        look.last = Some(pos);
        look.roll += dt * 9.0;
        let d = view.at.distance(pos);
        let size = apparent_size(HEAD_SIZE, d, HEAD_ANGLE);
        let flicker = 1.0 + 0.08 * (look.roll * 3.7).sin();
        let place = |shapes: &mut GlowQuery, e: Entity, tf: Transform, intensity: Option<f32>| {
            if let Ok((mut t, mut vis, _, tag, _, _)) = shapes.get_mut(e) {
                *t = tf;
                vis.set_if_neq(Visibility::Visible);
                if let (Some(mut tag), Some(i)) = (tag, intensity) {
                    tag.set_if_neq(glow_tag(Color::WHITE, i));
                }
            }
        };
        place(
            &mut shapes,
            look.head,
            Transform {
                translation: pos,
                rotation: billboard(view.rotation, look.roll),
                scale: Vec3::splat(size * flicker),
            },
            Some(1.7),
        );
        place(
            &mut shapes,
            look.heart,
            Transform {
                translation: pos + to_camera(pos) * 0.02,
                rotation: billboard(view.rotation, -look.roll * 0.6),
                scale: Vec3::splat(size * 0.55),
            },
            Some(2.0),
        );
        // The streak's round head sits on the orb; its tail trails behind.
        place(
            &mut shapes,
            look.streak,
            Transform {
                translation: pos,
                rotation: axial_billboard(-vel, to_camera(pos)),
                scale: Vec3::new(
                    STREAK_WIDTH * size / HEAD_SIZE,
                    1.0,
                    STREAK_LENGTH * (size / HEAD_SIZE).sqrt(),
                ),
            },
            Some(1.5),
        );
        place(
            &mut shapes,
            look.halo,
            Transform::from_translation(pos),
            None,
        );
        if let Ok((.., Some(mut halo))) = shapes.get_mut(look.halo) {
            let want = Halo::new(HALO_FIRE, 1.6 * size / HEAD_SIZE, 2.0);
            if *halo != want {
                *halo = want;
            }
        }
    }
    pools.orbs = looks;

    // Pooled sparks, chips and puffs.
    let pools = &mut *pools;
    for pool in [&mut pools.glow, &mut pools.solid] {
        for i in 0..pool.live.len() {
            let Some(spark) = pool.live[i].as_mut() else {
                continue;
            };
            let Ok((mut tf, mut vis, mesh, tag, material, _)) = shapes.get_mut(pool.entities[i])
            else {
                continue;
            };
            if spark.fresh {
                spark.fresh = false;
                if let Some(mut mesh) = mesh
                    && mesh.0 != spark.mesh
                {
                    mesh.0 = spark.mesh.clone();
                }
                if let (Some(want), Some(mut have)) = (spark.material.clone(), material)
                    && have.0 != want
                {
                    have.0 = want;
                }
            } else if !spark.p.step(dt) {
                vis.set_if_neq(Visibility::Hidden);
                pool.free(i);
                continue;
            }
            let rotation = match &mut spark.face {
                Face::Billboard { roll, spin } => {
                    *roll += *spin * dt;
                    billboard(view.rotation, *roll)
                }
                Face::Streak => axial_billboard(-spark.p.vel, to_camera(spark.p.pos)),
                Face::Free => spark.p.render_rotation(),
            };
            *tf = Transform {
                translation: spark.p.pos,
                rotation,
                scale: spark.p.scale().max(Vec3::splat(1e-4)),
            };
            if let Some(mut tag) = tag {
                let next = glow_tag(spark.tint, spark.glow()).0;
                if spark.tag != next {
                    spark.tag = next;
                    tag.0 = next;
                }
            }
            vis.set_if_neq(Visibility::Visible);
        }
    }
    for i in 0..pools.flashes.live.len() {
        let Some(flash) = pools.flashes.live[i].as_mut() else {
            continue;
        };
        let Ok((mut tf, mut vis, _, _, _, Some(mut halo))) =
            shapes.get_mut(pools.flashes.entities[i])
        else {
            continue;
        };
        if flash.fresh {
            flash.fresh = false;
        } else {
            flash.age += dt;
        }
        if flash.age >= flash.life {
            vis.set_if_neq(Visibility::Hidden);
            pools.flashes.free(i);
            continue;
        }
        let t = flash.age / flash.life;
        tf.translation = flash.pos;
        *halo = Halo::new(
            HALO_FIRE,
            flash.size * (1.0 + 0.3 * t),
            flash.intensity * (1.0 - t) * (1.0 - t),
        );
        vis.set_if_neq(Visibility::Visible);
    }
}
