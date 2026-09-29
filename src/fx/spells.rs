//! Spells (docs/M2-SPEC.md → Spells and effects; targets T03–T08): every shot
//! looks like magic, and every hit lands on the frame it registers.
//!
//! Amendment B (D50) made every spell 2–3× bigger and more saturated than the
//! first pass, as the targets paint them:
//!
//! - **Rifle:** a big spiky blue star burst at the muzzle (the viewmodel's
//!   [`MuzzleFlash`], restyled here) and a blue starburst bolt: a jagged
//!   starburst head under a white sparkle, a glowing ribbon and a trail of
//!   [`RIFLE_TRAIL_SPARKLES`] sparkles and stars, from the muzzle to the hit.
//!   Flight counts rendered frames: the head is part-way on the frame the shot
//!   lands and on the hit point within two ([`bolt_progress`]).
//! - **Pump:** a wide violet-and-gold fan flash at the bell, a fan of
//!   [`PUMP_FAN_SPARKS`] sparks with [`PUMP_FAN_SPARKLES`] sparkles among them,
//!   and ten sparks along the real pellet paths, each ending in a violet-gold
//!   burst with a white star and sparks.
//! - **Impacts,** on the frame of the hit: body (a solid cyan star under
//!   layered white-blue starbursts, [`BODY_SPARKS`] sparks and sparkles), head
//!   (the gold flash, rays, [`HEAD_SPARKS`] sparks, stars and sparkles),
//!   shield hits (a cyan hex shimmer on the knight), shield breaks (a wide
//!   burst of cyan glass panes and a storm of chips, sparkles, and stars
//!   circling his helmet for [`DIZZY_TIME`]), pieces (brick chips off walls,
//!   splinters off floors and ramps), misses (a fizzle puff where the bolt
//!   lands or ends).
//! - **Crystal chambers:** the crackling energy in each gun's chamber is
//!   [`super::chamber`]'s (it shares these assets).
//! - **Eliminations:** a big puffy cartoon poof with stars and sparkles, and
//!   his hat drops, spins and settles on the grass until he respawns
//!   ([`super::hat`]).
//!
//! **Pooling.** Everything draws from fixed pools of hidden entities made at
//! startup: glow shapes (sized by the particle cap, `FeedbackTuning::
//! max_particles`, 600 by default, so `--knobs particles=N` applies), solid
//! shapes (half that),
//! [`BOLT_POOL`] bolts, [`HALO_POOL`] halos and the hats. No mesh, material or
//! image is ever made after startup; per frame this only moves transforms and
//! writes `MeshTag`s. When a pool is full the oldest effect is recycled.
//!
//! **Look.** Glowing shapes share one additive [`SpellMaterial`] (colour in the
//! mesh, per-effect tint and fade in the `MeshTag`); soft glow comes from the
//! look's [`Halo`]s (so `--knobs halos=off` applies). Clouds, chips and stars
//! are toon-shaded. Every variant is registered with the pipeline warm-up.
//!
//! **Evidence.** [`SpellTiming`] counts rendered frames and logs, per player
//! hit on a character, the frame of the hit, of its impact effect and (with
//! the HUD) of its hitmarker and damage number, plus each rifle bolt's firing
//! and arrival frame. The `fx_check` scenario writes it out (gate S6).

use super::{
    chamber::{self, ARC_SHAPES},
    hat::{self, HatPool},
    material::{SPELL_SHADER_EMBEDDED, SpellMaterial, glow_tag},
    shapes::{self, *},
    sim::{
        BOLT_ARRIVAL_FRAMES, FxRng, Particle, RIFLE_SCREEN_FLIGHT, SlotPool, apparent_size,
        axial_billboard, billboard, bolt_progress, orbit_offset, pellet_flight, rifle_flight,
        screen_to_path,
    },
};
use crate::{
    app::BootGate,
    arena::visuals::TargetFigure,
    building::Piece,
    combat::Downed,
    hud::HitFeedbackStats,
    knight::KnightRig,
    look::{Halo, ModelDressed, NoOutline, ToonMaterial, warmup::Warmup, warmup::WarmupState},
    palette::cartoon,
    render::{CameraFollowSet, MainCamera, VIEWMODEL_LAYER},
    shared::{
        Character, DamageDealt, DamageTarget, Eliminated, FreezableTime, GameCue, LookAngles,
        PieceKind, Player, PreviousFeet, ShotFired, SimTick, WeaponKind,
    },
    tuning::Tuning,
    viewmodel::{MuzzleFlash, MuzzlePoint, ViewmodelSet},
};
use bevy::{
    asset::io::embedded::EmbeddedAssetRegistry,
    camera::visibility::{RenderLayers, VisibilitySystems},
    ecs::system::RunSystemOnce,
    light::NotShadowCaster,
    mesh::MeshTag,
    prelude::*,
};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Bolts in flight at once (a pump shot takes ten).
pub const BOLT_POOL: usize = 32;
/// Stand-alone glows at once.
pub const HALO_POOL: usize = 40;
/// Hard ceiling on the glow pool, whatever the settings say.
pub const GLOW_CEILING: u32 = 2000;
/// Stars circle a knight's helmet this long after his shield breaks (T07).
pub const DIZZY_TIME: f32 = 1.0;
/// Entries kept in each [`SpellTiming`] log.
pub const TIMING_LOG: usize = 512;
/// The rifle bolt's head: at least this big (m), and this wide an angle (rad)
/// wherever it is, so it reads as T03's big glowing starburst.
pub const RIFLE_HEAD_SIZE: f32 = 0.2;
pub const RIFLE_HEAD_ANGLE: f32 = 0.24;
/// The head's and the ribbon's glow (the spell material's intensity).
const RIFLE_HEAD_GLOW: f32 = 1.4;
const RIFLE_RIBBON_GLOW: f32 = 2.2;
/// Sparkles strewn along a rifle bolt's path (from the glow pool).
pub const RIFLE_TRAIL_SPARKLES: usize = 36;
/// The rifle's burst where a shot leaves the gun, per metre from the eye.
const RIFLE_MUZZLE_BURST: f32 = 0.4;
/// Sparks in the pump's fan out of the bell (T04), and sparkles among them.
pub const PUMP_FAN_SPARKS: usize = 56;
pub const PUMP_FAN_SPARKLES: usize = 20;
/// Sparks flung out of a body hit (T05) and a headshot (T06).
pub const BODY_SPARKS: usize = 22;
pub const HEAD_SPARKS: usize = 26;
/// Hot metal sparks off a body hit's armor (M4), on top of the starburst.
pub const METAL_SPARKS: usize = 10;
/// The headshot's solid gold flash (T06): its size (m; at least this wide an
/// angle, rad, so it reads far off without hiding him) and life (s). It holds
/// full size while the hat pops (about 0.2 s) and then shrinks away.
pub const HEAD_FLASH_SIZE: f32 = 1.05;
pub const HEAD_FLASH_ANGLE: f32 = 0.065;
pub const HEAD_FLASH_LIFE: f32 = 0.42;
/// Solid gold flash colours (sRGB): the starburst and its pale core.
const FLASH_GOLD: Color = Color::srgb(1.0, 0.76, 0.14);
const FLASH_CORE: Color = Color::srgb(1.0, 0.95, 0.6);
/// The body hit's solid star (sRGB): saturated cyan-blue under the glow, so it
/// reads blue on the bright sky too, where additive blue washes out (T05).
const FLASH_BLUE: Color = Color::srgb(0.22, 0.7, 1.0);

// ---------------------------------------------------------------------------
// Public markers and evidence
// ---------------------------------------------------------------------------

/// A pooled glowing shape (spell material). Hidden while unused.
#[derive(Component, Debug, Clone, Copy)]
pub struct SpellGlow;

/// A pooled solid, toon-shaded shape (clouds, chips, stars). Hidden while unused.
#[derive(Component, Debug, Clone, Copy)]
pub struct SpellSolid;

/// A pooled stand-alone glow halo. Hidden while unused.
#[derive(Component, Debug, Clone, Copy)]
pub struct SpellHalo;

/// A bolt's head (its sparkle, or a pump spark) and its ribbon; also
/// [`SpellGlow`]s. `end` is where the bolt is going.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct BoltHead {
    pub end: Vec3,
    pub weapon: Option<WeaponKind>,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct BoltRibbon;

/// What a character impact looked like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImpactKind {
    Body,
    Head,
    Shield,
    ShieldBreak,
}

/// One of the player's hits on a character, frame by frame (gate S6).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HitTiming {
    /// The rendered frame the hit reached presentation ([`SpellTiming::frame`]).
    pub frame: u64,
    /// The simulation tick of the hit, and whether that tick ran during this
    /// very frame (so the frame is the hit frame).
    pub tick: u64,
    pub hit_this_frame: bool,
    pub kind: ImpactKind,
    /// Frames its impact effect, hitmarker and damage number appeared on.
    pub impact_frame: Option<u64>,
    pub marker_frame: Option<u64>,
    pub number_frame: Option<u64>,
}

/// One of the player's rifle bolts.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BoltTiming {
    pub fired_frame: u64,
    pub tick: u64,
    /// The frame its head was drawn on the hit point.
    pub arrived_frame: Option<u64>,
    pub end: [f32; 3],
}

/// Spell timing evidence; see the module docs.
#[derive(Resource, Debug, Clone, Default, Serialize)]
pub struct SpellTiming {
    /// Rendered frames so far (counted in `First`).
    pub frame: u64,
    /// The simulation tick when this frame began.
    pub frame_start_tick: u64,
    pub hits: Vec<HitTiming>,
    pub bolts: Vec<BoltTiming>,
    /// Totals (not capped like the logs).
    pub character_hits: u32,
    pub impacts_same_frame: u32,
    pub markers_same_frame: u32,
    pub numbers_same_frame: u32,
    pub bolts_fired: u32,
    pub bolts_within_two_frames: u32,
    #[serde(skip)]
    hud_seen: Option<(u32, u32)>,
}

impl SpellTiming {
    fn log_hit(&mut self, hit: HitTiming) {
        self.character_hits += 1;
        if hit.impact_frame == Some(hit.frame) && hit.hit_this_frame {
            self.impacts_same_frame += 1;
        }
        if self.hits.len() < TIMING_LOG {
            self.hits.push(hit);
        }
    }
}

/// Live and total counts of every spell pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PoolCounts {
    pub glow: (usize, usize),
    pub solid: (usize, usize),
    pub bolts: (usize, usize),
    pub halos: (usize, usize),
    pub hats: (usize, usize),
}

// ---------------------------------------------------------------------------
// Assets
// ---------------------------------------------------------------------------

/// Every mesh and material the spells use, made once at startup.
#[derive(Resource, Debug, Clone)]
pub struct SpellAssets {
    pub material: Handle<SpellMaterial>,
    pub solid: Handle<ToonMaterial>,
    pub cloud: Handle<ToonMaterial>,
    pub star: Handle<ToonMaterial>,
    /// Flat, unlit-looking gold for the headshot flash (and its pale core):
    /// opaque, so it stays gold over the grey helmet and the bright sky, where
    /// additive gold washes out to white.
    pub flash: Handle<ToonMaterial>,
    pub flash_core: Handle<ToonMaterial>,
    pow: Handle<Mesh>,
    sparkle_blue: Handle<Mesh>,
    sparkle_white: Handle<Mesh>,
    sparkle_gold: Handle<Mesh>,
    sparkle_violet: Handle<Mesh>,
    burst_blue: [Handle<Mesh>; 2],
    burst_gold: Handle<Mesh>,
    rays_gold: Handle<Mesh>,
    burst_pellet: Handle<Mesh>,
    burst_cyan: Handle<Mesh>,
    streak_blue: Handle<Mesh>,
    streak_gold: Handle<Mesh>,
    streak_violet: Handle<Mesh>,
    ribbon_blue: Handle<Mesh>,
    ribbon_violet: Handle<Mesh>,
    ribbon_gold: Handle<Mesh>,
    ring_cyan: Handle<Mesh>,
    ring_gold: Handle<Mesh>,
    pub swirl: Handle<Mesh>,
    hex_shell: Handle<Mesh>,
    panes: [Handle<Mesh>; 3],
    chips: [Handle<Mesh>; 2],
    pub rifle_flash: Handle<Mesh>,
    pub pump_flash: Handle<Mesh>,
    clouds: [Handle<Mesh>; 2],
    puffs: [Handle<Mesh>; 2],
    brick_chip: Handle<Mesh>,
    splinter: Handle<Mesh>,
    gold_star: Handle<Mesh>,
    /// Chips of steel armor off a body hit (M4).
    armor_chips: [Handle<Mesh>; 2],
    /// The rifle bolt's head: a jagged blue starburst under a white sparkle.
    bolt_head: Handle<Mesh>,
    /// The body hit's solid cyan star (under the glowing layers).
    pub flash_blue: Handle<ToonMaterial>,
    /// Crystal-chamber lightning ([`chamber`]): blue arcs, violet arcs, and a
    /// sparkle mote of each colour.
    arcs: [[Handle<Mesh>; ARC_SHAPES]; 2],
    motes: [Handle<Mesh>; 2],
}

impl SpellAssets {
    /// One of a gun's chamber lightning arc shapes.
    pub fn arc(&self, kind: WeaponKind, shape: usize) -> Handle<Mesh> {
        let k = usize::from(kind == WeaponKind::Pump);
        self.arcs[k][shape % ARC_SHAPES].clone()
    }

    /// A gun's chamber sparkle mote.
    pub fn mote(&self, kind: WeaponKind) -> Handle<Mesh> {
        self.motes[usize::from(kind == WeaponKind::Pump)].clone()
    }
}

/// Halo colours (sRGB).
const HALO_BLUE: Color = Color::srgb(0.45, 0.8, 1.0);
const HALO_GOLD: Color = Color::srgb(1.0, 0.8, 0.35);
const HALO_VIOLET: Color = Color::srgb(0.78, 0.4, 1.0);
const HALO_CYAN: Color = Color::srgb(0.45, 0.88, 1.0);
const HALO_POOF: Color = Color::srgb(0.95, 0.9, 1.0);

/// A toon material that shows one flat colour whatever the light: no albedo
/// (so no key, fill or shadow band), no rim, all emissive.
fn flat_glow(color: Color) -> ToonMaterial {
    ToonMaterial::new(Color::BLACK)
        .with_emissive(color, 1.0)
        .with_rim(0.0)
}

fn make_assets(
    meshes: &mut Assets<Mesh>,
    spell: &mut Assets<SpellMaterial>,
    toon: &mut Assets<ToonMaterial>,
) -> SpellAssets {
    let mut add = |m: Mesh| meshes.add(m);
    SpellAssets {
        material: spell.add(SpellMaterial::default()),
        solid: toon.add(ToonMaterial::vertex_colored()),
        cloud: toon.add(ToonMaterial::vertex_colored().with_rim(0.5)),
        star: toon.add(ToonMaterial::vertex_colored().with_emissive(cartoon::STAR_GOLD, 0.45)),
        flash: toon.add(flat_glow(FLASH_GOLD)),
        flash_core: toon.add(flat_glow(FLASH_CORE)),
        pow: add(starburst(10, 0x7, GOLD, &[GOLD_DEEP], 0.27)),
        sparkle_blue: add(sparkle(BOLT_BLUE, BOLT_CORE)),
        sparkle_white: add(sparkle(BOLT_CORE, Color::WHITE)),
        sparkle_gold: add(sparkle(GOLD, GOLD_CORE)),
        sparkle_violet: add(sparkle(VIOLET, VIOLET_CORE)),
        burst_blue: [
            add(starburst(
                14,
                0x1,
                BOLT_BLUE,
                &[BOLT_BLUE, BOLT_DEEP, BOLT_CORE],
                0.11,
            )),
            add(starburst(11, 0x2, BOLT_BLUE, &[BOLT_DEEP, BOLT_BLUE], 0.12)),
        ],
        burst_gold: add(starburst(14, 0x3, GOLD, &[GOLD, GOLD_DEEP], 0.13)),
        rays_gold: add(starburst(10, 0x4, GOLD_CORE, &[GOLD_DEEP, GOLD], 0.035)),
        burst_pellet: add(starburst(
            12,
            0x5,
            VIOLET,
            &[VIOLET, GOLD, VIOLET_DEEP],
            0.12,
        )),
        burst_cyan: add(starburst(14, 0x6, GLASS, &[GLASS, GLASS_EDGE], 0.11)),
        streak_blue: add(streak(Color::WHITE, BOLT_CORE, BOLT_BLUE)),
        streak_gold: add(streak(GOLD_CORE, GOLD, GOLD_DEEP)),
        streak_violet: add(streak(VIOLET_CORE, VIOLET, VIOLET_DEEP)),
        ribbon_blue: add(ribbon(BOLT_CORE, BOLT_BLUE)),
        ribbon_violet: add(ribbon(VIOLET_CORE, VIOLET)),
        ribbon_gold: add(ribbon(GOLD_CORE, GOLD)),
        ring_cyan: add(shapes::ring(GLASS_EDGE)),
        ring_gold: add(shapes::ring(GOLD)),
        swirl: add(swirl(Color::WHITE)),
        hex_shell: add(hex_shell(GLASS_EDGE, GLASS)),
        panes: [
            add(glass_pane(11)),
            add(glass_pane(12)),
            add(glass_pane(13)),
        ],
        chips: [add(glass_chip(21)), add(glass_chip(22))],
        rifle_flash: add(rifle_muzzle_burst()),
        pump_flash: add(pump_muzzle_fan()),
        clouds: [add(poof_cloud(31)), add(poof_cloud(32))],
        puffs: [add(puff(41)), add(puff(42))],
        brick_chip: add(brick_chip()),
        splinter: add(wood_splinter()),
        gold_star: add(gold_star()),
        armor_chips: [add(armor_chip(61)), add(armor_chip(62))],
        bolt_head: add(bolt_head()),
        flash_blue: toon.add(flat_glow(FLASH_BLUE)),
        arcs: [
            std::array::from_fn(|k| add(lightning_arc(0xA0 + k as u64, BOLT_BLUE, BOLT_DEEP))),
            std::array::from_fn(|k| add(lightning_arc(0xB0 + k as u64, VIOLET, VIOLET_DEEP))),
        ],
        motes: [
            add(sparkle_cross(BOLT_BLUE, BOLT_CORE)),
            add(sparkle_cross(VIOLET, VIOLET_CORE)),
        ],
    }
}

// ---------------------------------------------------------------------------
// Pools
// ---------------------------------------------------------------------------

/// How a pooled shape turns.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Face {
    /// Faces the camera, rolled about the view axis.
    Billboard { roll: f32, roll_speed: f32 },
    /// Long and thin along its velocity, turned to face the camera.
    Streak,
    /// Tumbles with the particle's own rotation.
    Free,
    /// A fixed rotation (ground rings, orbit rings).
    Fixed(Quat),
}

/// What a pooled shape stays attached to.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Follow {
    target: Entity,
    /// Offset from the target's feet (world axes).
    offset: Vec3,
    /// Circles the offset: (index, count, radius, rad/s).
    orbit: Option<(usize, usize, f32, f32)>,
    /// Vanishes if the target is eliminated.
    until_downed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Plain,
    Shimmer,
}

/// One pooled shape.
#[derive(Debug, Clone)]
struct Spark {
    p: Particle,
    mesh: Handle<Mesh>,
    /// Solid shapes only.
    material: Option<Handle<ToonMaterial>>,
    face: Face,
    tint: Color,
    intensity: f32,
    /// Fraction of life after which the glow fades out.
    fade_start: f32,
    /// Twinkle rate (Hz), 0 for none.
    twinkle: f32,
    /// Seconds before it appears.
    delay: f32,
    follow: Option<Follow>,
    /// Pulled this far toward the camera, so it never sinks into its target.
    pull: f32,
    role: Role,
    fresh: bool,
    /// Drawn at least once (its mesh is on the pooled entity).
    shown: bool,
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
                roll_speed: 0.0,
            },
            tint: Color::WHITE,
            intensity: 1.0,
            fade_start: 0.6,
            twinkle: 0.0,
            delay: 0.0,
            follow: None,
            pull: 0.0,
            role: Role::Plain,
            fresh: true,
            shown: false,
            tag: u32::MAX,
        }
    }
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

    fn put(&mut self, limit: usize, item: T) -> bool {
        match self.slots.alloc(limit) {
            Some(i) => {
                self.live[i] = Some(item);
                true
            }
            None => false,
        }
    }

    fn free(&mut self, i: usize) {
        self.live[i] = None;
        self.slots.free(i);
    }

    fn counts(&self) -> (usize, usize) {
        (self.slots.live_count(), self.entities.len())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoltStyle {
    Rifle,
    Pellet { gold: bool },
}

/// One bolt in flight (head and ribbon entities).
#[derive(Debug, Clone)]
struct Bolt {
    style: BoltStyle,
    start: Vec3,
    end: Vec3,
    /// Where the head is on the hit frame and the next ([`bolt_progress`]).
    early: [f32; 2],
    /// Rendered frames drawn so far.
    frames: u32,
    /// Seconds since firing.
    age: f32,
    /// The ribbon's life (s).
    life: f32,
    roll: f32,
    /// Index into [`SpellTiming::bolts`] for the player's rifle bolts.
    timing: Option<usize>,
    arrived: bool,
    fresh: bool,
}

/// A stand-alone glow.
#[derive(Debug, Clone)]
struct Glow {
    pos: Vec3,
    follow: Option<Follow>,
    color: Color,
    size: f32,
    grow: f32,
    intensity: f32,
    age: f32,
    life: f32,
    pull: f32,
    fresh: bool,
}

/// Every spell pool. See the module docs.
#[derive(Resource)]
pub struct SpellPools {
    glow: Pool<Spark>,
    solid: Pool<Spark>,
    bolts: Pool<Bolt>,
    /// (head, ribbon) per bolt slot.
    bolt_parts: Vec<(Entity, Entity)>,
    halos: Pool<Glow>,
}

impl SpellPools {
    /// Live and total entities in each pool (hats from [`HatPool`]).
    pub fn counts(&self) -> PoolCounts {
        PoolCounts {
            glow: self.glow.counts(),
            solid: self.solid.counts(),
            bolts: self.bolts.counts(),
            halos: self.halos.counts(),
            hats: (0, 0),
        }
    }
}

/// Live and total counts of every spell pool, hats included.
pub fn pool_counts(world: &World) -> Option<PoolCounts> {
    let pools = world.get_resource::<SpellPools>()?;
    let mut counts = pools.counts();
    if let Some(hats) = world.get_resource::<HatPool>() {
        counts.hats = (hats.live(), hats.capacity());
    }
    Some(counts)
}

#[derive(Resource)]
struct SpellRng(FxRng);

impl Default for SpellRng {
    fn default() -> Self {
        Self(FxRng::new(0x5BE11))
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

/// Spells and hit effects (client). Added by `FxPlugin`.
pub struct SpellsPlugin;

impl Plugin for SpellsPlugin {
    fn build(&self, app: &mut App) {
        let registry = app.world().resource::<EmbeddedAssetRegistry>();
        registry.insert_asset(
            PathBuf::new(),
            Path::new(SPELL_SHADER_EMBEDDED),
            include_bytes!("../../assets/shaders/spell.wgsl").as_slice(),
        );
        app.add_plugins(MaterialPlugin::<SpellMaterial>::default())
            .add_systems(Startup, setup_spells);
        add_systems(app);
    }
}

impl SpellsPlugin {
    /// Adds the spells to an app that is already running without a renderer (a
    /// test's headless simulation): the same pools, logic and evidence, with
    /// the assets made on the spot.
    pub fn install(app: &mut App) {
        if !app.world().contains_resource::<Assets<SpellMaterial>>() {
            app.init_asset::<SpellMaterial>();
        }
        if !app.world().contains_resource::<Assets<ToonMaterial>>() {
            app.init_asset::<ToonMaterial>();
        }
        app.init_resource::<BootGate>()
            .init_resource::<WarmupState>();
        add_systems(app);
        app.world_mut()
            .run_system_once(setup_spells)
            .expect("spell setup");
    }
}

fn add_systems(app: &mut App) {
    app.add_message::<ModelDressed>()
        .init_resource::<SpellTiming>()
        .init_resource::<SpellRng>()
        .init_resource::<MuzzlePoint>()
        .add_systems(First, count_frame)
        .add_systems(Update, (dress_muzzle_flashes, hat::attach_hat_models))
        .add_systems(
            PostUpdate,
            (
                hat::configure_hat_models,
                (emit_spells, simulate_spells, hat::simulate_hats)
                    .chain()
                    .after(CameraFollowSet)
                    .after(ViewmodelSet)
                    .before(TransformSystems::Propagate)
                    // Swapped pool meshes get their bounds this frame.
                    .before(VisibilitySystems::CalculateBounds),
            ),
        )
        .add_systems(Last, record_hud_feedback);
    chamber::add_systems(app);
}

fn setup_spells(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut spell: ResMut<Assets<SpellMaterial>>,
    mut toon: ResMut<Assets<ToonMaterial>>,
    tuning: Res<Tuning>,
    mut warmup: Warmup,
) {
    let assets = make_assets(&mut meshes, &mut spell, &mut toon);
    let glow_count = tuning.feedback.max_particles.min(GLOW_CEILING) as usize;
    let solid_count = glow_count / 2;
    let glow_entity = |commands: &mut Commands, name: &'static str| {
        commands
            .spawn((
                Name::new(name),
                SpellGlow,
                Mesh3d(assets.sparkle_blue.clone()),
                MeshMaterial3d(assets.material.clone()),
                glow_tag(Color::WHITE, 0.0),
                Transform::default(),
                Visibility::Hidden,
                NotShadowCaster,
                NoOutline,
            ))
            .id()
    };
    let glow: Vec<Entity> = (0..glow_count)
        .map(|_| glow_entity(&mut commands, "Spell glow"))
        .collect();
    let solid: Vec<Entity> = (0..solid_count)
        .map(|_| {
            commands
                .spawn((
                    Name::new("Spell solid"),
                    SpellSolid,
                    Mesh3d(assets.brick_chip.clone()),
                    MeshMaterial3d(assets.solid.clone()),
                    Transform::default(),
                    Visibility::Hidden,
                    NotShadowCaster,
                    NoOutline,
                ))
                .id()
        })
        .collect();
    let bolt_parts: Vec<(Entity, Entity)> = (0..BOLT_POOL)
        .map(|_| {
            let head = glow_entity(&mut commands, "Bolt head");
            commands
                .entity(head)
                .insert((BoltHead::default(), Halo::new(HALO_BLUE, 2.4, 2.0)));
            let ribbon = glow_entity(&mut commands, "Bolt ribbon");
            commands.entity(ribbon).insert(BoltRibbon);
            (head, ribbon)
        })
        .collect();
    let halos: Vec<Entity> = (0..HALO_POOL)
        .map(|_| {
            commands
                .spawn((
                    Name::new("Spell halo"),
                    SpellHalo,
                    Halo::new(Color::WHITE, 1.0, 0.0),
                    Transform::default(),
                    Visibility::Hidden,
                ))
                .id()
        })
        .collect();
    let hats = hat::spawn_hat_pool(&mut commands, &assets);

    // Every material and layout the spells draw, compiled behind the loading
    // screen: the glow material on the world and the viewmodel layers, and
    // the three toon variants.
    let tag = glow_tag(Color::WHITE, 1.0);
    warmup.add_with(
        assets.sparkle_blue.clone(),
        assets.material.clone(),
        tag.clone(),
    );
    warmup.add_with(
        assets.rifle_flash.clone(),
        assets.material.clone(),
        (tag, RenderLayers::layer(VIEWMODEL_LAYER)),
    );
    warmup.add(assets.brick_chip.clone(), assets.solid.clone());
    warmup.add(assets.armor_chips[0].clone(), assets.solid.clone());
    warmup.add(assets.clouds[0].clone(), assets.cloud.clone());
    warmup.add(assets.gold_star.clone(), assets.star.clone());
    warmup.add(assets.pow.clone(), assets.flash.clone());

    commands.insert_resource(SpellPools {
        glow: Pool::new(glow),
        solid: Pool::new(solid),
        bolts: Pool::new(bolt_parts.iter().map(|p| p.0).collect()),
        bolt_parts,
        halos: Pool::new(halos),
    });
    commands.insert_resource(hats);
    commands.insert_resource(assets);
}

fn count_frame(tick: Res<SimTick>, mut timing: ResMut<SpellTiming>) {
    timing.frame += 1;
    timing.frame_start_tick = tick.0;
}

/// On a viewmodel muzzle flash once it wears its spell burst.
#[derive(Component, Debug, Clone, Copy)]
pub struct SpellFlash;

/// Restyles the viewmodel's muzzle flashes as spell bursts: a blue star on
/// the rifle, the violet-and-gold fan on the pump, each with a glow.
fn dress_muzzle_flashes(
    mut commands: Commands,
    assets: Option<Res<SpellAssets>>,
    flashes: Query<(Entity, &MuzzleFlash), Without<SpellFlash>>,
) {
    let Some(assets) = assets else { return };
    for (entity, flash) in &flashes {
        let (mesh, halo) = match flash.0 {
            WeaponKind::Rifle => (assets.rifle_flash.clone(), Halo::new(HALO_BLUE, 0.3, 2.2)),
            WeaponKind::Pump => (assets.pump_flash.clone(), Halo::new(HALO_VIOLET, 0.36, 2.6)),
        };
        commands
            .entity(entity)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert((
                SpellFlash,
                Mesh3d(mesh),
                MeshMaterial3d(assets.material.clone()),
                glow_tag(Color::WHITE, 1.15),
                halo,
            ));
    }
}

// ---------------------------------------------------------------------------
// Emitting
// ---------------------------------------------------------------------------

struct Emitter<'a> {
    pools: &'a mut SpellPools,
    assets: &'a SpellAssets,
    rng: &'a mut FxRng,
    glow_limit: usize,
    solid_limit: usize,
    /// The camera (or, headless, the player's eye) and its view direction.
    eye: Vec3,
    forward: Vec3,
}

impl Emitter<'_> {
    fn glow(&mut self, spark: Spark) -> bool {
        let limit = self.glow_limit;
        self.pools.glow.put(limit, spark)
    }

    fn solid(&mut self, spark: Spark) -> bool {
        let limit = self.solid_limit;
        self.pools.solid.put(limit, spark)
    }

    fn halo(&mut self, glow: Glow) -> bool {
        self.pools.halos.put(HALO_POOL, glow)
    }

    fn dist(&self, p: Vec3) -> f32 {
        self.eye.distance(p)
    }

    fn roll(&mut self) -> f32 {
        self.rng.range(0.0, std::f32::consts::TAU)
    }

    /// A stand-alone glow that fades out over `life`.
    fn flash(&mut self, pos: Vec3, color: Color, size: f32, intensity: f32, life: f32, pull: f32) {
        self.halo(Glow {
            pos,
            follow: None,
            color,
            size,
            grow: 1.3,
            intensity,
            age: 0.0,
            life,
            pull,
            fresh: true,
        });
    }

    /// A flat glowing pop (a starburst or sparkle) that faces the camera.
    #[allow(clippy::too_many_arguments)]
    fn pop(
        &mut self,
        pos: Vec3,
        mesh: Handle<Mesh>,
        size: f32,
        life: f32,
        intensity: f32,
        pull: f32,
        spin: f32,
    ) -> bool {
        self.pop_held(pos, mesh, size, life, intensity, pull, spin, 0.4)
    }

    /// [`Self::pop`] that keeps full size for the first `hold` of its life
    /// (and full glow a little longer) before shrinking away.
    #[allow(clippy::too_many_arguments)]
    fn pop_held(
        &mut self,
        pos: Vec3,
        mesh: Handle<Mesh>,
        size: f32,
        life: f32,
        intensity: f32,
        pull: f32,
        spin: f32,
        hold: f32,
    ) -> bool {
        let roll = self.roll();
        let mut s = Spark::new(pos, &mesh);
        s.p.size = Vec3::splat(size);
        s.p.life = life;
        s.p.birth_scale = 0.45;
        s.p.shrink_start = hold;
        s.face = Face::Billboard {
            roll,
            roll_speed: spin,
        };
        s.intensity = intensity;
        s.fade_start = (hold + 0.1).min(0.95);
        s.pull = pull;
        self.glow(s)
    }

    /// Glowing sparks flung from `pos` around `dir`.
    #[allow(clippy::too_many_arguments)]
    fn sparks(
        &mut self,
        pos: Vec3,
        dir: Vec3,
        spread: f32,
        count: usize,
        speed: (f32, f32),
        meshes: &[Handle<Mesh>],
        width: f32,
        length: f32,
    ) {
        for i in 0..count {
            let d = self.rng.cone(dir, spread);
            let mut s = Spark::new(pos, &meshes[i % meshes.len()]);
            s.p.vel = d * self.rng.range(speed.0, speed.1);
            s.p.drag = 4.5;
            s.p.gravity = 4.0;
            s.p.life = self.rng.range(0.16, 0.3);
            s.p.size = Vec3::new(width, 1.0, length) * self.rng.range(0.75, 1.3);
            s.p.stretch = 0.03;
            s.p.shrink_start = 0.35;
            s.face = Face::Streak;
            s.intensity = 1.8;
            s.fade_start = 0.4;
            s.pull = 0.15;
            self.glow(s);
        }
    }

    /// Small twinkling sparkles scattered around `pos` within `radius`.
    fn sparkles(
        &mut self,
        pos: Vec3,
        radius: f32,
        count: usize,
        size: f32,
        meshes: &[Handle<Mesh>],
    ) {
        for i in 0..count {
            let at = pos + self.rng.dir() * self.rng.range(0.3, 1.0) * radius;
            let roll = self.roll();
            let mut s = Spark::new(at, &meshes[i % meshes.len()]);
            s.p.vel = self.rng.dir() * 0.4 + Vec3::Y * 0.3;
            s.p.drag = 2.0;
            s.p.life = self.rng.range(0.25, 0.55);
            s.p.size = Vec3::splat(size * self.rng.range(0.7, 1.3));
            s.p.birth_scale = 0.2;
            s.p.shrink_start = 0.55;
            s.face = Face::Billboard {
                roll,
                roll_speed: self.rng.range(-4.0, 4.0),
            };
            s.twinkle = self.rng.range(6.0, 11.0);
            s.delay = self.rng.range(0.0, 0.12);
            s.intensity = 1.4;
            self.glow(s);
        }
    }

    // -- Shots ----------------------------------------------------------------

    fn bolt(
        &mut self,
        style: BoltStyle,
        start: Vec3,
        end: Vec3,
        early: [f32; 2],
        timing: Option<usize>,
    ) -> bool {
        let roll = self.roll();
        // The head lands on frame 2 whatever the life; the ribbon lingers
        // (and its tail races after the head) for the rest of it.
        let life = match style {
            BoltStyle::Rifle => 0.2,
            BoltStyle::Pellet { .. } => 0.1,
        };
        self.pools.bolts.put(
            BOLT_POOL,
            Bolt {
                style,
                start,
                end,
                early,
                frames: 0,
                age: 0.0,
                life,
                roll,
                timing,
                arrived: false,
                fresh: true,
            },
        )
    }

    /// A burst where a shot leaves the gun, in the world (it outlasts the
    /// viewmodel's two-frame flash).
    ///
    /// It sits a little ahead of the muzzle along the shot, so the gun (drawn
    /// over the world) doesn't hide it; the pump's is the big fan flash (T04).
    fn muzzle_burst(&mut self, muzzle: Vec3, dir: Vec3, weapon: WeaponKind) {
        let (mesh, ahead, size, color) = match weapon {
            WeaponKind::Rifle => (
                self.assets.burst_blue[0].clone(),
                0.12,
                RIFLE_MUZZLE_BURST,
                HALO_BLUE,
            ),
            WeaponKind::Pump => (self.assets.burst_pellet.clone(), 0.3, 0.62, HALO_VIOLET),
        };
        let at = muzzle + dir.normalize_or(Vec3::NEG_Z) * ahead;
        let d = self.dist(at).max(0.3);
        let (glow, halo) = match weapon {
            WeaponKind::Rifle => (1.2, 0.3),
            WeaponKind::Pump => (1.2, 0.26),
        };
        self.pop(at, mesh, size * d, 0.12, glow, 0.0, 6.0);
        // A crisp white sparkle over the burst's middle.
        let core = match weapon {
            WeaponKind::Rifle => self.assets.sparkle_white.clone(),
            WeaponKind::Pump => self.assets.sparkle_violet.clone(),
        };
        self.pop(at, core, size * d * 0.4, 0.09, 1.4, 0.0, -3.0);
        self.flash(at, color, halo * d, 0.9, 0.1, 0.0);
    }

    /// Depth of `p` along the view.
    fn depth(&self, p: Vec3) -> f32 {
        (p - self.eye).dot(self.forward)
    }

    /// The rifle's bolt, with a trail of sparkles along its path. Its flight
    /// is laid out on screen (T03): see [`rifle_flight`].
    fn rifle_bolt(&mut self, start: Vec3, end: Vec3, timing: Option<usize>) {
        let (z0, z1) = (self.depth(start), self.depth(end));
        self.bolt(BoltStyle::Rifle, start, end, rifle_flight(z0, z1), timing);
        let path = end - start;
        let len = path.length();
        if len < 0.5 {
            return;
        }
        let dir = path / len;
        let side = dir.any_orthonormal_vector();
        let meshes = [
            self.assets.sparkle_white.clone(),
            self.assets.sparkle_blue.clone(),
            self.assets.sparkle_blue.clone(),
        ];
        for i in 0..RIFLE_TRAIL_SPARKLES {
            // Spread evenly across the screen, not down the (foreshortened)
            // path: one in each slice of the flight, jittered within it.
            let slice = (i as f32 + self.rng.range(0.0, 1.0)) / RIFLE_TRAIL_SPARKLES as f32;
            let across = 0.08 + 0.86 * slice;
            let u = screen_to_path(across, z0, z1);
            let around = Quat::from_axis_angle(dir, self.rng.range(0.0, std::f32::consts::TAU));
            let at = start + path * u;
            let d = self.dist(at);
            let spread = apparent_size(0.04, d, 0.02).min(0.22);
            let jitter = around * side * self.rng.range(0.3, 1.4) * spread;
            let roll = self.roll();
            let mut s = Spark::new(at + jitter, &meshes[i % 3]);
            s.p.vel = around * side * 0.3 * d.min(4.0);
            s.p.drag = 3.0;
            s.p.life = self.rng.range(0.24, 0.48);
            // Every fourth one is a big star (T03's trail of stars).
            let big = if i % 4 == 2 { 2.4 } else { 1.0 };
            s.p.size = Vec3::splat(apparent_size(0.08, d, 0.065) * self.rng.range(0.7, 1.35) * big);
            s.p.birth_scale = 0.3;
            s.p.shrink_start = 0.5;
            s.face = Face::Billboard {
                roll,
                roll_speed: self.rng.range(-5.0, 5.0),
            };
            s.twinkle = 12.0;
            // Sparkles appear as the head passes them.
            s.delay = if across <= RIFLE_SCREEN_FLIGHT[0] {
                0.0
            } else if across <= RIFLE_SCREEN_FLIGHT[1] {
                0.5 / 60.0
            } else {
                1.5 / 60.0
            };
            s.intensity = 2.4;
            self.glow(s);
        }
    }

    /// The pump's big fan of violet and gold sparks out of the bell (T04):
    /// dozens of teardrops along and around the pellets' directions, from the
    /// bell out past the knight, with sparkles twinkling among them (the
    /// viewmodel draws the flash itself).
    fn pump_fan(&mut self, start: Vec3, dirs: &[Vec3]) {
        if dirs.is_empty() {
            return;
        }
        let meshes = [
            self.assets.streak_violet.clone(),
            self.assets.streak_gold.clone(),
            self.assets.streak_violet.clone(),
            self.assets.streak_gold.clone(),
            self.assets.streak_violet.clone(),
        ];
        let aim = dirs.iter().copied().sum::<Vec3>().normalize_or(Vec3::NEG_Z);
        for i in 0..PUMP_FAN_SPARKS {
            // Most follow a pellet; some fill the cone between them.
            let base = if i % 3 == 2 {
                aim
            } else {
                dirs[i % dirs.len()]
            };
            let d = self.rng.cone(base, if i % 3 == 2 { 0.32 } else { 0.16 });
            let out = self.rng.range(0.1, 3.2);
            let mut s = Spark::new(start + d * out, &meshes[i % 5]);
            s.p.vel = d * self.rng.range(9.0, 20.0);
            s.p.drag = 6.0;
            s.p.life = self.rng.range(0.16, 0.3);
            let w = 0.045 + 0.03 * out;
            s.p.size = Vec3::new(w, 1.0, w * 6.0);
            s.p.stretch = 0.02;
            s.p.shrink_start = 0.4;
            s.face = Face::Streak;
            s.intensity = 1.9;
            s.fade_start = 0.35;
            self.glow(s);
        }
        let twinkles = [
            self.assets.sparkle_violet.clone(),
            self.assets.sparkle_gold.clone(),
            self.assets.sparkle_white.clone(),
        ];
        for i in 0..PUMP_FAN_SPARKLES {
            let d = self.rng.cone(aim, 0.38);
            let out = self.rng.range(0.3, 3.5);
            let roll = self.roll();
            let mut s = Spark::new(start + d * out, &twinkles[i % 3]);
            s.p.vel = d * self.rng.range(1.0, 3.0) + Vec3::Y * 0.3;
            s.p.drag = 2.5;
            s.p.life = self.rng.range(0.2, 0.42);
            s.p.size = Vec3::splat((0.05 + 0.035 * out) * self.rng.range(0.7, 1.3));
            s.p.birth_scale = 0.2;
            s.p.shrink_start = 0.5;
            s.face = Face::Billboard {
                roll,
                roll_speed: self.rng.range(-5.0, 5.0),
            };
            s.twinkle = self.rng.range(7.0, 12.0);
            s.intensity = 1.8;
            self.glow(s);
        }
    }

    /// A violet-gold burst where a pump pellet lands, a white star over it
    /// (T04's stars on the knight), sparks and sparkles flung off. Small
    /// cores and long rays (M4 art, V3): ten of them on a knight at 3 m read
    /// as a hollow starburst he's flung out of, not a disc hiding him.
    fn pellet_impact(&mut self, point: Vec3, normal: Vec3) {
        let d = self.dist(point);
        let n = normal.normalize_or(Vec3::Y);
        let out = (n + (self.eye - point).normalize_or_zero()).normalize_or(Vec3::Y);
        let mesh = self.assets.burst_pellet.clone();
        self.pop(
            point,
            mesh,
            apparent_size(0.26, d, 0.022),
            0.14,
            1.15,
            0.2,
            3.0,
        );
        let star = if self.rng.f() < 0.6 {
            self.assets.sparkle_white.clone()
        } else {
            self.assets.sparkle_gold.clone()
        };
        self.pop(
            point,
            star,
            apparent_size(0.2, d, 0.018),
            0.16,
            1.6,
            0.22,
            -2.0,
        );
        let sparks = [
            self.assets.streak_violet.clone(),
            self.assets.streak_gold.clone(),
        ];
        let w = apparent_size(0.03, d, 0.0026);
        self.sparks(point, out, 1.0, 4, (5.0, 9.0), &sparks, w, w * 7.0);
        let twinkles = [
            self.assets.sparkle_gold.clone(),
            self.assets.sparkle_violet.clone(),
        ];
        let at = point + n * 0.05;
        self.sparkles(at, 0.2, 2, apparent_size(0.08, d, 0.009), &twinkles);
    }

    /// A fizzle where a bolt that hit nothing alive lands (`landed`) or ends.
    fn fizzle(&mut self, end: Vec3, normal: Vec3, landed: bool) {
        let d = self.dist(end);
        if !landed {
            let mesh = self.assets.sparkle_blue.clone();
            self.pop(end, mesh, apparent_size(0.25, d, 0.02), 0.16, 1.2, 0.0, 6.0);
            self.flash(end, HALO_BLUE, apparent_size(0.4, d, 0.035), 1.0, 0.12, 0.0);
            return;
        }
        let n = normal.normalize_or(Vec3::Y);
        let at = end + n * 0.06;
        let mut s = Spark::new(at, &self.assets.puffs[self.rng.pick(2)].clone());
        s.material = Some(self.assets.cloud.clone());
        s.p.vel = n * 0.3 + Vec3::Y * 0.3;
        s.p.drag = 3.0;
        s.p.life = 0.32;
        s.p.size = Vec3::splat(apparent_size(0.22, d, 0.015));
        s.p.birth_scale = 0.3;
        s.p.grow = 1.4;
        s.p.shrink_start = 0.35;
        s.face = Face::Free;
        s.p.rot = Quat::from_rotation_y(self.roll());
        self.solid(s);
        // The glows sit up off the grass, so it doesn't cut them in half.
        let up = end + n * 0.16;
        let burst = self.assets.burst_blue[1].clone();
        self.pop(up, burst, apparent_size(0.3, d, 0.025), 0.12, 1.2, 0.1, 4.0);
        let meshes = [self.assets.sparkle_blue.clone()];
        self.sparkles(up, 0.2, 3, apparent_size(0.07, d, 0.008), &meshes);
        self.flash(up, HALO_BLUE, apparent_size(0.4, d, 0.035), 1.2, 0.12, 0.1);
    }

    /// Chips off a building piece: brick off walls, splinters off floors and
    /// ramps, a puff of dust and a little spell glow.
    fn piece_chips(&mut self, point: Vec3, normal: Vec3, kind: PieceKind, pellet: bool) {
        let n = normal.normalize_or(Vec3::Y);
        let d = self.dist(point);
        let count = if pellet { 2 } else { 5 };
        let wall = kind == PieceKind::Wall;
        for _ in 0..count {
            let dir = self.rng.cone(n, 0.7);
            let (mesh, size) = if wall {
                (self.assets.brick_chip.clone(), self.rng.range(0.07, 0.11))
            } else {
                (self.assets.splinter.clone(), self.rng.range(0.12, 0.2))
            };
            let mut s = Spark::new(point + n * 0.04, &mesh);
            s.material = Some(self.assets.solid.clone());
            s.p.vel = dir * self.rng.range(2.5, 5.0) + Vec3::Y * self.rng.range(0.5, 2.0);
            s.p.rot = Quat::from_scaled_axis(self.rng.dir() * 3.0);
            s.p.spin = self.rng.dir() * self.rng.range(8.0, 20.0);
            s.p.gravity = 18.0;
            s.p.bounce = Some(0.3);
            s.p.radius = size * 0.3;
            s.p.size = Vec3::splat(size);
            s.p.life = self.rng.range(0.55, 0.85);
            s.p.shrink_start = 0.6;
            s.face = Face::Free;
            self.solid(s);
        }
        let mut dust = Spark::new(
            point + n * 0.08,
            &self.assets.puffs[self.rng.pick(2)].clone(),
        );
        dust.material = Some(self.assets.cloud.clone());
        dust.p.vel = n * 0.6 + Vec3::Y * 0.3;
        dust.p.drag = 3.0;
        dust.p.life = 0.3;
        dust.p.size = Vec3::splat(if pellet { 0.1 } else { 0.16 });
        dust.p.birth_scale = 0.4;
        dust.p.grow = 1.8;
        dust.p.shrink_start = 0.3;
        dust.p.rot = Quat::from_rotation_y(self.roll());
        dust.face = Face::Free;
        self.solid(dust);
        if !pellet {
            let burst = self.assets.burst_blue[1].clone();
            self.pop(
                point + n * 0.05,
                burst,
                apparent_size(0.28, d, 0.02),
                0.1,
                1.3,
                0.1,
                4.0,
            );
            let meshes = [self.assets.sparkle_blue.clone()];
            self.sparkles(
                point + n * 0.05,
                0.15,
                2,
                apparent_size(0.07, d, 0.008),
                &meshes,
            );
        }
    }

    // -- Character impacts ------------------------------------------------------

    /// Metal on metal (M4): hot white-gold sparks that arc down off his armor
    /// under gravity, and `chips` small steel chips knocked off it that tumble
    /// to the ground at his feet (`ground`), bounce and shrink away.
    fn armor_hit(&mut self, point: Vec3, normal: Vec3, ground: f32, chips: u32) -> bool {
        let d = self.dist(point);
        let out = (normal.normalize_or(Vec3::Y) + (self.eye - point).normalize_or_zero())
            .normalize_or(Vec3::Y);
        let w = apparent_size(0.03, d, 0.003);
        let mut shown = false;
        for i in 0..METAL_SPARKS {
            let dir = self.rng.cone(out + Vec3::Y * 0.4, 0.8);
            let mesh = if i % 3 == 0 {
                self.assets.streak_blue.clone()
            } else {
                self.assets.streak_gold.clone()
            };
            let mut s = Spark::new(point, &mesh);
            s.p.vel = dir * self.rng.range(4.0, 8.0);
            s.p.gravity = 16.0;
            s.p.drag = 1.5;
            s.p.life = self.rng.range(0.22, 0.38);
            s.p.size = Vec3::new(w, 1.0, w * 5.0) * self.rng.range(0.8, 1.2);
            s.p.stretch = 0.05;
            s.p.shrink_start = 0.5;
            s.face = Face::Streak;
            s.intensity = 2.2;
            s.fade_start = 0.5;
            s.pull = 0.12;
            shown |= self.glow(s);
        }
        for i in 0..chips {
            let dir = self.rng.cone(out, 0.9);
            let size = apparent_size(0.07, d, 0.006) * self.rng.range(0.8, 1.25);
            let mut s = Spark::new(point + out * 0.05, &self.assets.armor_chips[i as usize % 2]);
            s.material = Some(self.assets.solid.clone());
            s.p.vel = dir * self.rng.range(2.0, 4.0) + Vec3::Y * self.rng.range(1.5, 3.0);
            s.p.rot = Quat::from_scaled_axis(self.rng.dir() * 3.0);
            s.p.spin = self.rng.dir() * self.rng.range(10.0, 22.0);
            s.p.gravity = 18.0;
            s.p.bounce = Some(0.35);
            s.p.ground = ground;
            s.p.radius = size * 0.12;
            s.p.size = Vec3::splat(size);
            s.p.life = self.rng.range(0.7, 0.95);
            s.p.shrink_start = 0.65;
            s.face = Face::Free;
            s.pull = 0.05;
            shown |= self.solid(s);
        }
        shown
    }

    /// Returns whether anything was shown.
    fn body_impact(&mut self, point: Vec3, normal: Vec3) -> bool {
        let d = self.dist(point);
        let out = (normal.normalize_or(Vec3::Y) + (self.eye - point).normalize_or_zero())
            .normalize_or(Vec3::Y);
        // A solid cyan star under the glow keeps the burst saturated blue
        // whatever is behind it (T05).
        let roll = self.roll();
        let mut solid = Spark::new(point, &self.assets.pow);
        solid.material = Some(self.assets.flash_blue.clone());
        solid.p.size = Vec3::splat(apparent_size(0.85, d, 0.07));
        solid.p.life = 0.15;
        solid.p.birth_scale = 0.4;
        solid.p.shrink_start = 0.35;
        solid.face = Face::Billboard {
            roll,
            roll_speed: 4.0,
        };
        solid.pull = 0.28;
        let mut shown = self.solid(solid);
        let k = self.rng.pick(2);
        let burst = self.assets.burst_blue[k].clone();
        shown |= self.pop(
            point,
            burst,
            apparent_size(1.25, d, 0.1),
            0.2,
            1.0,
            0.3,
            5.0,
        );
        let core = self.assets.sparkle_white.clone();
        shown |= self.pop(
            point,
            core,
            apparent_size(0.45, d, 0.038),
            0.14,
            1.3,
            0.32,
            0.0,
        );
        let meshes = [
            self.assets.streak_blue.clone(),
            self.assets.streak_blue.clone(),
            self.assets.streak_gold.clone(),
        ];
        let w = apparent_size(0.05, d, 0.0042);
        self.sparks(
            point,
            out,
            1.0,
            BODY_SPARKS,
            (5.0, 11.0),
            &meshes,
            w,
            w * 6.0,
        );
        let twinkles = [
            self.assets.sparkle_blue.clone(),
            self.assets.sparkle_white.clone(),
        ];
        self.sparkles(
            point + out * 0.1,
            apparent_size(0.45, d, 0.04),
            8,
            apparent_size(0.1, d, 0.009),
            &twinkles,
        );
        self.flash(
            point,
            HALO_BLUE,
            apparent_size(0.9, d, 0.075),
            0.8,
            0.16,
            0.3,
        );
        shown
    }

    /// A headshot (T06): a big solid gold starburst on the helmet with a pale
    /// core (opaque, so it reads gold on any background), long thin gold rays,
    /// a white-hot centre, gold sparks, little gold stars flung off and a warm
    /// glow. The flash holds full size while the hat pops, then shrinks away.
    fn head_impact(&mut self, point: Vec3, normal: Vec3) -> bool {
        let d = self.dist(point);
        let out = (normal.normalize_or(Vec3::Y) + (self.eye - point).normalize_or_zero())
            .normalize_or(Vec3::Y);
        let roll = self.roll();
        let size = apparent_size(HEAD_FLASH_SIZE, d, HEAD_FLASH_ANGLE);
        let mut shown = false;
        // The core sits a little in front of the starburst.
        for (material, scale, pull, roll_speed) in [
            (self.assets.flash.clone(), 1.0, 0.3, 1.2),
            (self.assets.flash_core.clone(), 0.55, 0.32, -1.8),
        ] {
            let mut s = Spark::new(point, &self.assets.pow);
            s.material = Some(material);
            s.p.size = Vec3::splat(size * scale);
            s.p.life = HEAD_FLASH_LIFE;
            s.p.birth_scale = 0.3;
            s.p.shrink_start = 0.6;
            s.face = Face::Billboard {
                roll: roll + 0.3 * scale,
                roll_speed,
            };
            s.pull = pull;
            shown |= self.solid(s);
        }
        // Glows go in front of the solid flash so it never hides them.
        let rays = self.assets.rays_gold.clone();
        shown |= self.pop_held(point, rays, size * 2.6, 0.32, 1.7, 0.36, -2.0, 0.55);
        let burst = self.assets.burst_gold.clone();
        shown |= self.pop_held(point, burst, size * 0.85, 0.3, 1.5, 0.38, 3.0, 0.5);
        let core = self.assets.sparkle_white.clone();
        shown |= self.pop_held(point, core, size * 0.55, 0.26, 2.4, 0.4, 0.0, 0.5);
        let meshes = [
            self.assets.streak_gold.clone(),
            self.assets.streak_gold.clone(),
            self.assets.streak_blue.clone(),
        ];
        let w = apparent_size(0.055, d, 0.0048);
        self.sparks(
            point,
            out,
            1.0,
            HEAD_SPARKS,
            (5.0, 11.0),
            &meshes,
            w,
            w * 6.0,
        );
        let twinkles = [
            self.assets.sparkle_gold.clone(),
            self.assets.sparkle_white.clone(),
        ];
        self.sparkles(
            point + out * 0.1,
            size * 0.55,
            8,
            apparent_size(0.1, d, 0.009),
            &twinkles,
        );
        for i in 0..7 {
            let a = std::f32::consts::TAU * (i as f32 + self.rng.range(-0.25, 0.25)) / 7.0;
            let side = Vec3::new(a.cos(), 0.0, a.sin());
            let mut s = Spark::new(point, &self.assets.gold_star);
            s.material = Some(self.assets.star.clone());
            s.p.vel = side * self.rng.range(1.2, 2.2) + Vec3::Y * self.rng.range(2.2, 3.4);
            s.p.gravity = 7.0;
            s.p.drag = 1.0;
            s.p.size = Vec3::splat(apparent_size(0.13, d, 0.012) * self.rng.range(0.85, 1.15));
            s.p.birth_scale = 0.3;
            s.p.life = self.rng.range(0.5, 0.65);
            s.p.shrink_start = 0.6;
            s.face = Face::Billboard {
                roll: self.rng.range(-0.4, 0.4),
                roll_speed: self.rng.range(-4.0, 4.0),
            };
            s.pull = 0.35;
            self.solid(s);
        }
        self.flash(point, HALO_GOLD, size * 2.0, 2.0, 0.3, 0.3);
        shown
    }

    /// The cyan hex shimmer over a knight whose shield took a hit (refreshed,
    /// not stacked, by quick hits).
    fn shimmer(&mut self, target: Entity) {
        for spark in self.pools.glow.live.iter_mut().flatten() {
            if spark.role == Role::Shimmer && spark.follow.is_some_and(|f| f.target == target) {
                spark.p.age = 0.0;
                return;
            }
        }
        let mut s = Spark::new(Vec3::ZERO, &self.assets.hex_shell);
        s.follow = Some(Follow {
            target,
            offset: Vec3::Y * 0.93,
            orbit: None,
            until_downed: false,
        });
        s.p.size = Vec3::new(0.56, 1.0, 0.56);
        s.p.birth_scale = 0.92;
        s.p.grow = 1.08;
        s.p.life = 0.26;
        s.p.shrink_start = 1.0;
        s.face = Face::Free;
        s.intensity = 1.1;
        s.fade_start = 0.0;
        s.role = Role::Shimmer;
        self.glow(s);
    }

    /// The shield shatters: glass panes and shards burst off, a ring, a flash,
    /// and stars circle his helmet (T07).
    fn shield_break(&mut self, target: Entity, center: Vec3) {
        let d = self.dist(center);
        // Big curved panes of the broken shell, flung out wide (T07).
        for i in 0..14 {
            let a = std::f32::consts::TAU * (i as f32 + self.rng.range(-0.3, 0.3)) / 14.0;
            let out = Vec3::new(a.cos(), self.rng.range(-0.2, 0.4), a.sin()).normalize();
            let at = center + out * 0.45 + Vec3::Y * self.rng.range(-0.6, 0.6);
            let mut s = Spark::new(at, &self.assets.panes[i % 3].clone());
            s.p.vel = out * self.rng.range(2.4, 4.4) + Vec3::Y * 1.1;
            s.p.rot = Transform::IDENTITY.looking_to(-out, Vec3::Y).rotation
                * Quat::from_rotation_z(self.rng.range(-0.6, 0.6));
            s.p.spin = self.rng.dir() * self.rng.range(3.0, 8.0);
            s.p.gravity = 5.0;
            s.p.drag = 1.4;
            s.p.size = Vec3::splat(self.rng.range(0.45, 0.78));
            s.p.life = self.rng.range(0.65, 0.95);
            s.p.shrink_start = 0.6;
            s.face = Face::Free;
            s.intensity = 1.8;
            s.fade_start = 0.7;
            self.glow(s);
        }
        // A storm of glass chips.
        for i in 0..40 {
            let out = (self.rng.dir() + Vec3::Y * 0.3).normalize();
            let mut s = Spark::new(center + out * 0.3, &self.assets.chips[i % 2].clone());
            s.p.vel = out * self.rng.range(3.5, 8.5) + Vec3::Y * 1.5;
            s.p.rot = Quat::from_scaled_axis(self.rng.dir() * 3.0);
            s.p.spin = self.rng.dir() * self.rng.range(10.0, 25.0);
            s.p.gravity = 9.0;
            s.p.drag = 1.0;
            s.p.size = Vec3::splat(self.rng.range(0.08, 0.17));
            s.p.life = self.rng.range(0.45, 0.8);
            s.p.shrink_start = 0.55;
            s.face = Face::Free;
            s.intensity = 1.5;
            self.glow(s);
        }
        let mut ring = Spark::new(center, &self.assets.ring_cyan);
        ring.p.size = Vec3::splat(1.3);
        ring.p.grow = 2.6;
        ring.p.life = 0.3;
        ring.p.shrink_start = 1.0;
        ring.intensity = 1.1;
        ring.fade_start = 0.1;
        ring.pull = 0.2;
        self.glow(ring);
        let burst = self.assets.burst_cyan.clone();
        self.pop(
            center,
            burst,
            apparent_size(1.5, d, 0.12),
            0.18,
            1.3,
            0.35,
            3.0,
        );
        let twinkles = [
            self.assets.sparkle_white.clone(),
            self.assets.sparkle_blue.clone(),
        ];
        self.sparkles(center, 1.0, 12, 0.12, &twinkles);
        self.flash(
            center,
            HALO_CYAN,
            apparent_size(2.0, d, 0.16),
            1.5,
            0.24,
            0.3,
        );
        self.dizzy_stars(target);
    }

    /// Stars circling above the helmet for [`DIZZY_TIME`], with a faint orbit.
    fn dizzy_stars(&mut self, target: Entity) {
        let height = 1.97;
        let (radius, speed) = (0.25, 5.5);
        for i in 0..3 {
            let mut s = Spark::new(Vec3::ZERO, &self.assets.gold_star);
            s.material = Some(self.assets.star.clone());
            s.follow = Some(Follow {
                target,
                offset: Vec3::Y * height,
                orbit: Some((i, 3, radius, speed)),
                until_downed: true,
            });
            s.p.size = Vec3::splat(0.17);
            s.p.life = DIZZY_TIME;
            s.p.birth_scale = 0.3;
            s.p.shrink_start = 0.85;
            s.face = Face::Billboard {
                roll: 0.3 * i as f32,
                roll_speed: 2.5,
            };
            self.solid(s);
        }
        let mut orbit = Spark::new(Vec3::ZERO, &self.assets.ring_gold);
        orbit.follow = Some(Follow {
            target,
            offset: Vec3::Y * (height - 0.02),
            orbit: None,
            until_downed: true,
        });
        let r = radius / 0.45;
        orbit.p.size = Vec3::new(r, r * 0.62, 1.0);
        orbit.p.life = DIZZY_TIME;
        orbit.p.shrink_start = 0.85;
        orbit.face = Face::Fixed(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2));
        orbit.intensity = 0.7;
        orbit.fade_start = 0.8;
        self.glow(orbit);
    }

    /// The elimination poof (T08): a big puffy cloud, little puffs rolling
    /// out, gold stars flung up, sparkles and a flash.
    fn poof(&mut self, feet: Vec3) {
        // M4 art (V4): the cloud billows round his legs rather than swallowing
        // him, so his knocked-out take and the helmet popping off read over it.
        let center = feet + Vec3::Y * 0.62;
        let toward = (self.eye - center).with_y(0.0).normalize_or(Vec3::Z);
        let k = self.rng.pick(2);
        let mut cloud = Spark::new(center, &self.assets.clouds[k].clone());
        cloud.material = Some(self.assets.cloud.clone());
        cloud.p.vel = Vec3::Y * 0.35;
        cloud.p.drag = 1.0;
        cloud.p.rot = Quat::from_rotation_y(self.roll());
        cloud.p.spin = Vec3::Y * self.rng.range(-0.6, 0.6);
        cloud.p.size = Vec3::splat(1.05);
        cloud.p.birth_scale = 0.25;
        cloud.p.life = 0.95;
        cloud.p.shrink_start = 0.55;
        cloud.face = Face::Free;
        self.solid(cloud);
        for i in 0..6 {
            let a = std::f32::consts::TAU * (i as f32 + self.rng.range(-0.3, 0.3)) / 6.0;
            let out = Vec3::new(a.cos(), 0.0, a.sin());
            let at = feet + out * 0.5 + Vec3::Y * self.rng.range(0.2, 0.7);
            let mut s = Spark::new(at, &self.assets.puffs[i % 2].clone());
            s.material = Some(self.assets.cloud.clone());
            s.p.vel = out * self.rng.range(2.0, 3.4) + Vec3::Y * self.rng.range(0.2, 0.9);
            s.p.drag = 3.5;
            s.p.rot = Quat::from_rotation_y(self.roll());
            s.p.size = Vec3::splat(self.rng.range(0.4, 0.6));
            s.p.birth_scale = 0.3;
            s.p.life = self.rng.range(0.5, 0.8);
            s.p.shrink_start = 0.45;
            s.face = Face::Free;
            self.solid(s);
        }
        for i in 0..7 {
            let a = std::f32::consts::TAU * (i as f32 + 0.5) / 7.0 + self.rng.range(-0.3, 0.3);
            let out = Vec3::new(a.cos(), 0.0, a.sin()) + toward * 0.4;
            let mut s = Spark::new(center + Vec3::Y * 0.3, &self.assets.gold_star);
            s.material = Some(self.assets.star.clone());
            s.p.vel = out * self.rng.range(1.6, 2.8) + Vec3::Y * self.rng.range(2.5, 4.0);
            s.p.gravity = 5.0;
            s.p.drag = 0.8;
            s.p.size = Vec3::splat(self.rng.range(0.22, 0.3));
            s.p.birth_scale = 0.3;
            s.p.life = self.rng.range(0.9, 1.15);
            s.p.shrink_start = 0.7;
            s.face = Face::Billboard {
                roll: self.rng.range(-0.4, 0.4),
                roll_speed: self.rng.range(-3.0, 3.0),
            };
            self.solid(s);
        }
        let meshes = [
            self.assets.sparkle_white.clone(),
            self.assets.sparkle_violet.clone(),
            self.assets.sparkle_gold.clone(),
        ];
        self.sparkles(center + Vec3::Y * 0.4, 1.2, 18, 0.2, &meshes);
        self.flash(center, HALO_POOF, 1.8, 1.4, 0.25, 0.4);
    }
}

/// Where a shot's visuals start: the gun's muzzle as it appears on screen for
/// the player, just ahead of the eye for anyone else.
fn shot_start(shot: &ShotFired, player: Option<Entity>, muzzle: Option<Vec3>) -> Vec3 {
    let first = shot
        .traces
        .first()
        .map(|t| (t.end - shot.origin).normalize_or(Vec3::NEG_Z))
        .unwrap_or(Vec3::NEG_Z);
    if Some(shot.shooter) == player
        && let Some(m) = muzzle
    {
        m
    } else {
        shot.origin + first * 0.5
    }
}

/// Feet of a character as drawn this frame (interpolated between ticks).
fn drawn_feet(transform: &Transform, previous: Option<&PreviousFeet>, alpha: f32) -> Vec3 {
    previous.map_or(transform.translation, |p| {
        p.0.lerp(transform.translation, alpha)
    })
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn emit_spells(
    tuning: Res<Tuning>,
    assets: Option<Res<SpellAssets>>,
    pools: Option<ResMut<SpellPools>>,
    mut hats: Option<ResMut<HatPool>>,
    mut rng: ResMut<SpellRng>,
    mut timing: ResMut<SpellTiming>,
    muzzle: Res<MuzzlePoint>,
    (mut shots, mut damage, mut eliminated, mut cues): (
        MessageReader<ShotFired>,
        MessageReader<DamageDealt>,
        MessageReader<Eliminated>,
        MessageReader<GameCue>,
    ),
    player: Option<Single<(Entity, &Transform, Option<&LookAngles>), With<Player>>>,
    camera: Option<Single<&Transform, (With<MainCamera>, Without<Player>)>>,
    pieces: Query<&Piece>,
    characters: Query<(&Transform, Option<&LookAngles>), (With<Character>, Without<MainCamera>)>,
    figures: Query<(&TargetFigure, Option<&KnightRig>)>,
    globals: Query<&GlobalTransform>,
) {
    let (Some(assets), Some(mut pools)) = (assets, pools) else {
        shots.clear();
        damage.clear();
        eliminated.clear();
        cues.clear();
        return;
    };
    let (me, my_eye, my_look) = player
        .map(|p| {
            (
                Some(p.0),
                Some(p.1.translation + Vec3::Y * 1.62),
                p.2.map(LookAngles::forward),
            )
        })
        .unwrap_or((None, None, None));
    let feedback = &tuning.feedback;
    let glow_limit = feedback.max_particles.min(GLOW_CEILING) as usize;
    let frame = timing.frame;
    let start_tick = timing.frame_start_tick;
    let mut fx = Emitter {
        pools: &mut pools,
        assets: &assets,
        rng: &mut rng.0,
        glow_limit,
        solid_limit: glow_limit / 2,
        eye: camera
            .as_ref()
            .map(|c| c.translation)
            .or(my_eye)
            .unwrap_or(Vec3::new(0.0, 1.62, 0.0)),
        forward: camera
            .as_ref()
            .map(|c| c.forward().as_vec3())
            .or(my_look)
            .unwrap_or(Vec3::NEG_Z),
    };

    // Characters hit by pump pellets this frame: each pellet shows its own
    // violet-gold burst, so the white-blue body burst is left out for them.
    let mut pelleted: Vec<Entity> = Vec::new();
    for shot in shots.read() {
        let start = shot_start(shot, me, muzzle.0);
        let mine = Some(shot.shooter) == me;
        match shot.weapon {
            WeaponKind::Rifle => {
                for trace in &shot.traces {
                    let index = if mine {
                        timing.bolts_fired += 1;
                        (timing.bolts.len() < TIMING_LOG).then(|| {
                            timing.bolts.push(BoltTiming {
                                fired_frame: frame,
                                tick: shot.tick,
                                arrived_frame: None,
                                end: trace.end.to_array(),
                            });
                            timing.bolts.len() - 1
                        })
                    } else {
                        None
                    };
                    fx.rifle_bolt(start, trace.end, index);
                    match trace.hit {
                        None => fx.fizzle(trace.end, trace.normal, false),
                        Some(hit) if characters.contains(hit) => {}
                        Some(hit) => match pieces.get(hit) {
                            Ok(piece) => fx.piece_chips(trace.end, trace.normal, piece.kind, false),
                            Err(_) => fx.fizzle(trace.end, trace.normal, true),
                        },
                    }
                }
            }
            WeaponKind::Pump => {
                let dirs: Vec<Vec3> = shot
                    .traces
                    .iter()
                    .map(|t| (t.end - start).normalize_or(Vec3::NEG_Z))
                    .collect();
                fx.pump_fan(start, &dirs);
                for (i, trace) in shot.traces.iter().enumerate() {
                    fx.bolt(
                        BoltStyle::Pellet { gold: i % 2 == 1 },
                        start,
                        trace.end,
                        pellet_flight(start.distance(trace.end)),
                        None,
                    );
                    match trace.hit {
                        None => {}
                        Some(hit) => {
                            fx.pellet_impact(trace.end, trace.normal);
                            if characters.contains(hit) && !pelleted.contains(&hit) {
                                pelleted.push(hit);
                            }
                            if let Ok(piece) = pieces.get(hit) {
                                fx.piece_chips(trace.end, trace.normal, piece.kind, true);
                            }
                        }
                    }
                }
            }
        }
        let aim = shot.traces.first().map_or(Vec3::NEG_Z, |t| t.end - start);
        fx.muzzle_burst(start, aim, shot.weapon);
    }

    for hit in damage.read() {
        // Hits on the player himself (knights' orbs, M3) show no impact at
        // his own chest: it would fill the screen. The damage arrow, the
        // bonk and the bars say it instead.
        if hit.target_kind != DamageTarget::Character || hit.amount <= 0.0 || Some(hit.target) == me
        {
            continue;
        }
        let kind = if hit.shield_broke {
            ImpactKind::ShieldBreak
        } else if hit.headshot {
            ImpactKind::Head
        } else if hit.to_shield > 0.0 {
            ImpactKind::Shield
        } else {
            ImpactKind::Body
        };
        let mut shown = if hit.headshot {
            fx.head_impact(hit.point, hit.normal)
        } else if pelleted.contains(&hit.target) {
            // Its pellets' bursts are already on him this frame.
            true
        } else {
            fx.body_impact(hit.point, hit.normal)
        };
        // Metal sparks and armor chips off a body hit that reached his armor
        // (M4); a headshot rings and dents the helmet instead.
        if !hit.headshot && hit.amount > hit.to_shield + 1e-3 {
            let ground = characters
                .get(hit.target)
                .map_or(hit.point.y - 1.0, |(t, _)| t.translation.y);
            let (lo, hi) = tuning.kills.chips;
            let chips = lo + fx.rng.pick((hi.saturating_sub(lo) + 1) as usize) as u32;
            shown |= fx.armor_hit(hit.point, hit.normal, ground, chips);
        }
        if hit.to_shield > 0.0 {
            fx.shimmer(hit.target);
            shown = true;
        }
        if hit.shield_broke {
            let center = characters
                .get(hit.target)
                .map(|(t, _)| t.translation + Vec3::Y * 1.0)
                .unwrap_or(hit.point);
            fx.shield_break(hit.target, center);
        }
        if hit.source.is_some() && hit.source == me {
            timing.log_hit(HitTiming {
                frame,
                tick: hit.tick,
                hit_this_frame: hit.tick > start_tick,
                kind,
                impact_frame: shown.then_some(frame),
                marker_frame: None,
                number_frame: None,
            });
        }
    }

    for elimination in eliminated.read() {
        // The player has no knight hat to drop (M3: knights can down him).
        if Some(elimination.victim) == me {
            continue;
        }
        let feet = elimination.position;
        fx.poof(feet);
        let Some(hats) = hats.as_mut() else {
            continue;
        };
        let (at, yaw) = hat_on_head(elimination.victim, feet, &characters, &figures, &globals);
        let side = fx.rng.range(-0.5, 0.5);
        let spin_sign = if fx.rng.f() < 0.5 { -1.0 } else { 1.0 };
        let (vel, spin) = hat::hat_launch(fx.eye - feet, side, spin_sign);
        hats.drop_hat(elimination.victim, at, yaw, vel, spin, feet.y);
    }

    for cue in cues.read() {
        if let GameCue::Respawned { who } = *cue
            && let Some(hats) = hats.as_mut()
            && let Some(at) = hats.pick_up(who)
        {
            // The hat vanishes in a little puff of sparkles.
            let meshes = [
                fx.assets.sparkle_violet.clone(),
                fx.assets.sparkle_gold.clone(),
            ];
            fx.sparkles(at + Vec3::Y * 0.1, 0.25, 4, 0.12, &meshes);
        }
    }
}

/// Where the knight's hat is right now (its pivot, world) and his heading:
/// from his rigged figure when there is one, else from his feet.
fn hat_on_head(
    victim: Entity,
    feet: Vec3,
    characters: &Query<(&Transform, Option<&LookAngles>), (With<Character>, Without<MainCamera>)>,
    figures: &Query<(&TargetFigure, Option<&KnightRig>)>,
    globals: &Query<&GlobalTransform>,
) -> (Vec3, f32) {
    let yaw = characters
        .get(victim)
        .ok()
        .and_then(|(_, look)| look)
        .map_or(0.0, |l| l.yaw);
    let rigged = figures
        .iter()
        .find(|(f, _)| f.owner == victim)
        .and_then(|(_, rig)| rig)
        .and_then(|rig| rig.joints.last())
        .and_then(|(hat, _)| globals.get(*hat).ok())
        .map(|g| g.translation())
        .filter(|p| p.is_finite() && p.distance(feet) < 3.0);
    (rigged.unwrap_or(feet + Vec3::Y * hat::HAT_HEIGHT), yaw)
}

// ---------------------------------------------------------------------------
// Simulating
// ---------------------------------------------------------------------------

type GlowQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Transform,
        &'static mut Visibility,
        &'static mut Mesh3d,
        &'static mut MeshTag,
        Option<&'static mut Halo>,
        Option<&'static mut BoltHead>,
    ),
    (With<SpellGlow>, Without<SpellSolid>, Without<SpellHalo>),
>;

type SolidQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Transform,
        &'static mut Visibility,
        &'static mut Mesh3d,
        &'static mut MeshMaterial3d<ToonMaterial>,
    ),
    (With<SpellSolid>, Without<SpellGlow>, Without<SpellHalo>),
>;

type HaloQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Transform,
        &'static mut Visibility,
        &'static mut Halo,
    ),
    (With<SpellHalo>, Without<SpellGlow>, Without<SpellSolid>),
>;

type FollowQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Transform,
        Option<&'static PreviousFeet>,
        Has<Downed>,
    ),
    (
        With<Character>,
        Without<SpellGlow>,
        Without<SpellSolid>,
        Without<SpellHalo>,
    ),
>;

/// The camera's position and rotation this frame (or a stand-in headless).
#[derive(Clone, Copy)]
struct View {
    at: Vec3,
    rotation: Quat,
}

impl View {
    fn toward_camera(&self, p: Vec3) -> Vec3 {
        (self.at - p).normalize_or(Vec3::Z)
    }

    /// `p` pulled toward the camera by up to `pull` m (never past half-way).
    fn pulled(&self, p: Vec3, pull: f32) -> Vec3 {
        if pull <= 0.0 {
            return p;
        }
        let d = self.at - p;
        let len = d.length();
        if len < 1e-4 {
            return p;
        }
        p + d / len * pull.min(len * 0.4)
    }
}

/// Where a followed shape's anchor is, or `None` if it should vanish.
fn follow_anchor(follow: &Follow, targets: &FollowQuery, alpha: f32, age: f32) -> Option<Vec3> {
    let (transform, previous, downed) = targets.get(follow.target).ok()?;
    if follow.until_downed && downed {
        return None;
    }
    let mut at = drawn_feet(transform, previous, alpha) + follow.offset;
    if let Some((index, count, radius, speed)) = follow.orbit {
        at += orbit_offset(age, index, count, radius, speed);
    }
    Some(at)
}

fn glow_intensity(spark: &Spark) -> f32 {
    let t = (spark.p.age / spark.p.life.max(1e-4)).clamp(0.0, 1.0);
    let fade = if t <= spark.fade_start {
        1.0
    } else {
        1.0 - (t - spark.fade_start) / (1.0 - spark.fade_start).max(1e-4)
    };
    spark.intensity * fade.clamp(0.0, 1.0)
}

/// Moves one pooled shape; false once it is done (and should be freed).
fn place_spark(
    spark: &mut Spark,
    dt: f32,
    view: &View,
    targets: &FollowQuery,
    alpha: f32,
    transform: &mut Transform,
) -> bool {
    if spark.delay > 0.0 {
        if spark.fresh {
            spark.fresh = false;
        } else {
            spark.delay -= dt;
        }
        if spark.delay > 0.0 {
            return true;
        }
        // It appears now, where it was put.
        spark.fresh = true;
    }
    if spark.fresh {
        spark.fresh = false;
    } else if !spark.p.step(dt) {
        return false;
    }
    let anchor = match &spark.follow {
        Some(follow) => match follow_anchor(follow, targets, alpha, spark.p.age) {
            Some(at) => at,
            None => return false,
        },
        None => Vec3::ZERO,
    };
    let pos = view.pulled(anchor + spark.p.pos, spark.pull);
    let rotation = match &mut spark.face {
        Face::Billboard { roll, roll_speed } => {
            *roll += *roll_speed * dt;
            billboard(view.rotation, *roll)
        }
        // The streak's round head (z = 0) leads, its tail trails behind.
        Face::Streak => axial_billboard(-spark.p.vel, view.toward_camera(pos)),
        Face::Free => spark.p.render_rotation(),
        Face::Fixed(q) => *q,
    };
    let mut scale = spark.p.scale();
    if spark.twinkle > 0.0 {
        let w = (spark.p.age * spark.twinkle * std::f32::consts::PI)
            .sin()
            .abs();
        scale *= 0.55 + 0.45 * w;
    }
    *transform = Transform {
        translation: pos,
        rotation,
        scale: scale.max(Vec3::splat(1e-4)),
    };
    true
}

fn set_tag(tag: &mut MeshTag, tint: Color, intensity: f32, last: &mut u32) {
    let next = glow_tag(tint, intensity).0;
    if *last != next {
        *last = next;
        tag.0 = next;
    }
}

#[allow(clippy::too_many_arguments)]
fn simulate_spells(
    time: FreezableTime,
    fixed: Res<Time<Fixed>>,
    assets: Option<Res<SpellAssets>>,
    pools: Option<ResMut<SpellPools>>,
    mut timing: ResMut<SpellTiming>,
    camera: Option<
        Single<
            &Transform,
            (
                With<MainCamera>,
                Without<SpellGlow>,
                Without<SpellSolid>,
                Without<SpellHalo>,
            ),
        >,
    >,
    player: Option<
        Single<
            &Transform,
            (
                With<Player>,
                Without<MainCamera>,
                Without<SpellGlow>,
                Without<SpellSolid>,
                Without<SpellHalo>,
            ),
        >,
    >,
    mut glow: GlowQuery,
    mut solid: SolidQuery,
    mut halos: HaloQuery,
    targets: FollowQuery,
) {
    let (Some(assets), Some(mut pools)) = (assets, pools) else {
        return;
    };
    let dt = time.delta_secs();
    let frozen = time.is_frozen();
    let alpha = fixed.overstep_fraction().clamp(0.0, 1.0);
    let view = match (camera, player) {
        (Some(c), _) => View {
            at: c.translation,
            rotation: c.rotation,
        },
        (None, Some(p)) => View {
            at: p.translation + Vec3::Y * 1.62,
            rotation: p.rotation,
        },
        (None, None) => View {
            at: Vec3::new(0.0, 1.62, 0.0),
            rotation: Quat::IDENTITY,
        },
    };
    let pools = &mut *pools;
    let timing = &mut *timing;
    let frame = timing.frame;

    // Glow and solid shapes.
    for i in 0..pools.glow.live.len() {
        let Some(spark) = pools.glow.live[i].as_mut() else {
            continue;
        };
        let Ok((mut tf, mut vis, mut mesh, mut tag, ..)) = glow.get_mut(pools.glow.entities[i])
        else {
            continue;
        };
        if !place_spark(spark, dt, &view, &targets, alpha, &mut tf) {
            vis.set_if_neq(Visibility::Hidden);
            pools.glow.free(i);
            continue;
        }
        if spark.delay > 0.0 {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        }
        if !spark.shown {
            spark.shown = true;
            if mesh.0 != spark.mesh {
                mesh.0 = spark.mesh.clone();
            }
            spark.tag = u32::MAX;
        }
        let intensity = glow_intensity(spark);
        set_tag(&mut tag, spark.tint, intensity, &mut spark.tag);
        vis.set_if_neq(Visibility::Visible);
    }
    for i in 0..pools.solid.live.len() {
        let Some(spark) = pools.solid.live[i].as_mut() else {
            continue;
        };
        let Ok((mut tf, mut vis, mut mesh, mut material)) = solid.get_mut(pools.solid.entities[i])
        else {
            continue;
        };
        if !place_spark(spark, dt, &view, &targets, alpha, &mut tf) {
            vis.set_if_neq(Visibility::Hidden);
            pools.solid.free(i);
            continue;
        }
        if spark.delay > 0.0 {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        }
        if !spark.shown {
            spark.shown = true;
            if mesh.0 != spark.mesh {
                mesh.0 = spark.mesh.clone();
            }
            let want = spark
                .material
                .clone()
                .unwrap_or_else(|| assets.solid.clone());
            if material.0 != want {
                material.0 = want;
            }
        }
        vis.set_if_neq(Visibility::Visible);
    }

    // Bolts: the head flies by rendered frame, the ribbon lingers and thins.
    for i in 0..pools.bolts.live.len() {
        let Some(bolt) = pools.bolts.live[i].as_mut() else {
            continue;
        };
        let (head_e, ribbon_e) = pools.bolt_parts[i];
        if bolt.fresh {
            bolt.fresh = false;
        } else if !frozen {
            bolt.frames += 1;
            bolt.age += dt;
        }
        let done = bolt.age >= bolt.life && bolt.frames > BOLT_ARRIVAL_FRAMES;
        if done {
            for e in [head_e, ribbon_e] {
                if let Ok((_, mut vis, ..)) = glow.get_mut(e) {
                    vis.set_if_neq(Visibility::Hidden);
                }
            }
            pools.bolts.free(i);
            continue;
        }
        let path = bolt.end - bolt.start;
        let len = path.length().max(1e-4);
        let dir = path / len;
        let (head_mesh, ribbon_mesh, halo_color) = match bolt.style {
            BoltStyle::Rifle => (&assets.bolt_head, &assets.ribbon_blue, HALO_BLUE),
            BoltStyle::Pellet { gold: false } => {
                (&assets.streak_violet, &assets.ribbon_violet, HALO_VIOLET)
            }
            BoltStyle::Pellet { gold: true } => {
                (&assets.streak_gold, &assets.ribbon_gold, HALO_GOLD)
            }
        };
        let progress = bolt_progress(bolt.frames, bolt.early);
        let head = bolt.start + path * progress;
        let head_shown = bolt.frames <= BOLT_ARRIVAL_FRAMES;
        if progress >= 1.0 && !bolt.arrived {
            bolt.arrived = true;
            if let Some(entry) = bolt.timing.and_then(|k| timing.bolts.get_mut(k)) {
                entry.arrived_frame = Some(frame);
                if frame - entry.fired_frame <= u64::from(BOLT_ARRIVAL_FRAMES) {
                    timing.bolts_within_two_frames += 1;
                }
            }
        }
        let d_head = view.at.distance(head);
        if let Ok((mut tf, mut vis, mut mesh, mut tag, halo, marker)) = glow.get_mut(head_e) {
            if head_shown {
                if mesh.0 != *head_mesh {
                    mesh.0 = head_mesh.clone();
                }
                *tf = match bolt.style {
                    BoltStyle::Rifle => {
                        // About 8° across wherever it is; smaller as it lands
                        // in the impact.
                        let size = apparent_size(RIFLE_HEAD_SIZE, d_head, RIFLE_HEAD_ANGLE)
                            * if progress >= 1.0 { 0.6 } else { 1.0 };
                        Transform {
                            translation: head,
                            rotation: billboard(
                                view.rotation,
                                bolt.roll + bolt.frames as f32 * 0.7,
                            ),
                            scale: Vec3::splat(size),
                        }
                    }
                    BoltStyle::Pellet { .. } => {
                        let w = apparent_size(0.1, d_head, 0.007);
                        Transform {
                            translation: head,
                            rotation: axial_billboard(-dir, view.toward_camera(head)),
                            scale: Vec3::new(w, 1.0, w * 12.0),
                        }
                    }
                };
                let mut last = u32::MAX;
                let glow = match bolt.style {
                    BoltStyle::Rifle => RIFLE_HEAD_GLOW,
                    BoltStyle::Pellet { .. } => 1.6,
                };
                set_tag(&mut tag, Color::WHITE, glow, &mut last);
                if let Some(mut halo) = halo {
                    // A softer halo than the star, so its jagged shape reads.
                    let (size, intensity) = match bolt.style {
                        BoltStyle::Rifle => (0.95, 1.0),
                        BoltStyle::Pellet { .. } => (4.0, 1.0),
                    };
                    let want = Halo::new(halo_color, size, intensity);
                    if *halo != want {
                        *halo = want;
                    }
                }
                if let Some(mut marker) = marker
                    && marker.end != bolt.end
                {
                    marker.end = bolt.end;
                    marker.weapon = Some(match bolt.style {
                        BoltStyle::Rifle => WeaponKind::Rifle,
                        BoltStyle::Pellet { .. } => WeaponKind::Pump,
                    });
                }
                vis.set_if_neq(Visibility::Visible);
            } else {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
        // The ribbon: from the muzzle (its tail retracting once the bolt has
        // landed) to the head, thinning as it goes.
        let t = (bolt.age / bolt.life).clamp(0.0, 1.0);
        // The streak comes out of the gun until the head lands (T03, T05),
        // then its tail races after it.
        let tail_u = if bolt.frames <= BOLT_ARRIVAL_FRAMES {
            0.0
        } else {
            let after = BOLT_ARRIVAL_FRAMES as f32 / 60.0;
            let k = ((bolt.age - after) / (bolt.life - after).max(1e-3)).clamp(0.0, 1.0);
            (k * (2.0 - k)).min(progress)
        };
        let tail = bolt.start + path * tail_u;
        let span = (progress - tail_u) * len;
        if let Ok((mut tf, mut vis, mut mesh, mut tag, ..)) = glow.get_mut(ribbon_e) {
            if span > 0.02 {
                if mesh.0 != *ribbon_mesh {
                    mesh.0 = ribbon_mesh.clone();
                }
                let (base, glow) = match bolt.style {
                    BoltStyle::Rifle => (0.24, RIFLE_RIBBON_GLOW),
                    BoltStyle::Pellet { .. } => (0.05, 1.6),
                };
                let width = apparent_size(base * 0.15, d_head, base * 0.06) * (1.0 - 0.7 * t);
                let mid = tail + dir * (span * 0.5);
                *tf = Transform {
                    translation: tail,
                    rotation: axial_billboard(dir, view.toward_camera(mid)),
                    scale: Vec3::new(width, 1.0, span),
                };
                let mut last = u32::MAX;
                set_tag(&mut tag, Color::WHITE, glow * (1.0 - t * t), &mut last);
                vis.set_if_neq(Visibility::Visible);
            } else {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }

    // Stand-alone glows.
    for i in 0..pools.halos.live.len() {
        let Some(glow_item) = pools.halos.live[i].as_mut() else {
            continue;
        };
        let Ok((mut tf, mut vis, mut halo)) = halos.get_mut(pools.halos.entities[i]) else {
            continue;
        };
        if glow_item.fresh {
            glow_item.fresh = false;
        } else {
            glow_item.age += dt;
        }
        let anchor = match &glow_item.follow {
            Some(f) => follow_anchor(f, &targets, alpha, glow_item.age),
            None => Some(Vec3::ZERO),
        };
        let (Some(anchor), true) = (anchor, glow_item.age < glow_item.life) else {
            vis.set_if_neq(Visibility::Hidden);
            pools.halos.free(i);
            continue;
        };
        let t = (glow_item.age / glow_item.life).clamp(0.0, 1.0);
        let want = Halo::new(
            glow_item.color,
            1.0,
            glow_item.intensity * (1.0 - t) * (1.0 - t),
        );
        if *halo != want {
            *halo = want;
        }
        let size = glow_item.size * (1.0 + (glow_item.grow - 1.0) * (1.0 - (1.0 - t) * (1.0 - t)));
        *tf = Transform {
            translation: view.pulled(anchor + glow_item.pos, glow_item.pull),
            rotation: Quat::IDENTITY,
            scale: Vec3::splat(size.max(1e-4)),
        };
        vis.set_if_neq(Visibility::Visible);
    }
}

/// Matches this frame's hits with the HUD's hitmarkers and damage numbers
/// (counted by `hud::HitFeedbackStats`), so the evidence shows all of them
/// on one frame index.
fn record_hud_feedback(hud: Option<Res<HitFeedbackStats>>, mut timing: ResMut<SpellTiming>) {
    let Some(hud) = hud else { return };
    let now = (hud.markers_same_frame, hud.numbers_same_frame);
    let (markers, numbers) = match timing.hud_seen {
        Some((m, n)) => (now.0.saturating_sub(m), now.1.saturating_sub(n)),
        None => (0, 0),
    };
    timing.hud_seen = Some(now);
    timing.markers_same_frame += markers;
    timing.numbers_same_frame += numbers;
    let frame = timing.frame;
    let (mut markers, mut numbers) = (markers, numbers);
    for hit in timing.hits.iter_mut().filter(|h| h.frame == frame) {
        if markers > 0 && hit.marker_frame.is_none() {
            hit.marker_frame = Some(frame);
            markers -= 1;
        }
        if numbers > 0 && hit.number_frame.is_none() {
            hit.number_frame = Some(frame);
            numbers -= 1;
        }
    }
}
