//! The shield potion's look (docs/M3-SPEC.md → Waves, D80): a round cyan
//! bottle with a cork and a gold collar, hovering a hand's height above the
//! grass, bobbing and turning, with a twinkling glint, a soft cyan halo and
//! motes drifting up off it. It pops in where the knight fell with a small
//! sparkle, blinks through its last seconds, and bursts into a shower of cyan
//! sparkles when the player walks through it (the gulp-chime is the audio
//! bank's; the "+25" the Waves HUD's).
//!
//! **Pooling.** The simulation keeps a fixed pool of
//! [`PotionSlot`](crate::waves::PotionSlot)s (hidden while idle); each gets
//! its bottle, glint and halo entities once, as children, so they show and
//! hide with the slot. Sparkles draw from a fixed pool made at startup
//! ([`SPARK_POOL`], recycling the oldest). No mesh or material is made after
//! startup, and both are warmed behind the loading screen. Every clock reads
//! [`FreezableTime`].

use super::{
    material::{SpellMaterial, glow_tag},
    shapes::{sparkle, sparkle_cross, starburst},
    sim::{FxRng, Particle, SlotPool, billboard},
};
use crate::{
    look::{Halo, NoOutline, Outline, ToonMaterial, warmup::Warmup, with_outline_normals},
    palette,
    render::{CameraFollowSet, MainCamera},
    shared::{FreezableTime, GameCue, SimTick, TICK_SECONDS},
    viewmodel::{
        ViewmodelSet,
        mesh::{ModelBuilder, linear},
    },
    waves::PotionSlot,
};
use bevy::{
    camera::visibility::VisibilitySystems, light::NotShadowCaster, mesh::MeshTag, prelude::*,
};

/// Sparkles at once (a pickup burst is about 20).
pub const SPARK_POOL: usize = 64;
/// Glow flashes at once.
pub const FLASH_POOL: usize = 4;

/// The potion's cyan (the shield's), its bright heart and pale glass.
pub const POTION: Color = palette::SHIELD;
pub const POTION_BRIGHT: Color = Color::srgb(0.62, 0.97, 1.0);
pub const POTION_DEEP: Color = Color::srgb(0.05, 0.5, 0.86);
pub const GLASS: Color = Color::srgb(0.84, 0.97, 1.0);
pub const CORK: Color = Color::srgb(0.62, 0.4, 0.22);
pub const COLLAR: Color = Color::srgb(0.95, 0.72, 0.25);

/// The bottle hovers this high above where it lies (m), bobbing this much.
pub const HOVER: f32 = 0.32;
pub const BOB: f32 = 0.06;
/// Its turn (rad/s) and bob rate (rad/s).
pub const SPIN: f32 = 1.7;
pub const BOB_RATE: f32 = 2.4;
/// The bottle's size over its modelled size (a hand-sized flask, drawn a
/// little big so it reads across the arena).
pub const BOTTLE_SCALE: f32 = 1.3;
/// Its ink outline.
const INK: Color = Color::srgb(0.035, 0.04, 0.075);
/// It blinks through its last seconds on the ground.
pub const BLINK_SECONDS: f32 = 3.0;
/// The pop-in when it drops (s).
pub const POP_SECONDS: f32 = 0.35;
/// A mote drifts up off each potion this often (s).
const MOTE_EVERY: f32 = 0.3;

/// Marks a pooled potion effect entity.
#[derive(Component, Debug, Clone, Copy)]
pub struct PotionFx;

/// On a potion slot once its bottle, glint and halo exist.
#[derive(Component, Debug, Clone, Copy)]
pub struct PotionDressed {
    pub look: Entity,
    pub glint: Entity,
    pub halo: Entity,
}

/// The potion's meshes and materials, made once at startup.
#[derive(Resource, Debug, Clone)]
pub struct PotionFxAssets {
    pub glass: Handle<Mesh>,
    pub trim: Handle<Mesh>,
    pub glass_material: Handle<ToonMaterial>,
    pub trim_material: Handle<ToonMaterial>,
    pub glow: Handle<SpellMaterial>,
    pub glint: Handle<Mesh>,
    pub mote: Handle<Mesh>,
    pub burst: Handle<Mesh>,
}

#[derive(Debug, Clone)]
struct Spark {
    p: Particle,
    mesh: Handle<Mesh>,
    roll: f32,
    spin: f32,
    intensity: f32,
    fresh: bool,
}

#[derive(Debug, Clone, Copy)]
struct Flash {
    pos: Vec3,
    size: f32,
    intensity: f32,
    age: f32,
    life: f32,
}

#[derive(Resource)]
struct PotionFxPools {
    slots: SlotPool,
    sparks: Vec<Entity>,
    live: Vec<Option<Spark>>,
    flash_slots: SlotPool,
    flashes: Vec<Entity>,
    flash_live: Vec<Option<Flash>>,
    rng: FxRng,
    /// The effect clock (s) and the next mote's time.
    clock: f32,
    next_mote: f32,
}

impl PotionFxPools {
    fn spark(&mut self, spark: Spark) {
        if let Some(i) = self.slots.alloc(self.sparks.len()) {
            self.live[i] = Some(spark);
        }
    }

    fn flash(&mut self, pos: Vec3, size: f32, intensity: f32, life: f32) {
        if let Some(i) = self.flash_slots.alloc(self.flashes.len()) {
            self.flash_live[i] = Some(Flash {
                pos,
                size,
                intensity,
                age: 0.0,
                life,
            });
        }
    }
}

/// The potion's look (client). Added by `FxPlugin`.
pub struct PotionFxPlugin;

impl Plugin for PotionFxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_potion_fx)
            .add_systems(Update, dress_potions)
            .add_systems(
                PostUpdate,
                (emit_potion_fx, animate_potions)
                    .chain()
                    .after(CameraFollowSet)
                    .after(ViewmodelSet)
                    .before(TransformSystems::Propagate)
                    .before(VisibilitySystems::CalculateBounds),
            );
    }
}

/// A lathed shape: `rings` of (radius, height, colour) turned about +Y with
/// `sides` flat facets, capped at both ends.
fn lathe(m: &mut ModelBuilder, rings: &[(f32, f32, Color)], sides: usize, offset: f32) {
    let ring = |r: f32, y: f32| -> Vec<Vec3> {
        (0..sides)
            .map(|i| {
                let a = offset + std::f32::consts::TAU * i as f32 / sides as f32;
                Vec3::new(r * a.cos(), y, r * a.sin())
            })
            .collect()
    };
    for w in rings.windows(2) {
        let (r0, y0, c0) = w[0];
        let (r1, y1, c1) = w[1];
        let a = ring(r0, y0);
        let b = ring(r1, y1);
        let inside = Vec3::Y * (y0 + y1) * 0.5;
        for i in 0..sides {
            let j = (i + 1) % sides;
            let quad = [a[i], a[j], b[j], b[i]];
            let colors = [
                linear(c0, 1.0),
                linear(c0, 1.0),
                linear(c1, 1.0),
                linear(c1, 1.0),
            ];
            let n = (quad[1] - quad[0]).cross(quad[3] - quad[0]);
            let centroid = quad.iter().copied().sum::<Vec3>() / 4.0;
            let n = if n.dot(centroid - inside) < 0.0 {
                -n
            } else {
                n
            };
            if n.length_squared() > 1e-12 {
                m.poly_colored(&quad, &colors, n);
            }
        }
    }
    if let (Some(&(r, y, c)), Some(&(r2, y2, c2))) = (rings.first(), rings.last()) {
        if r > 1e-4 {
            m.poly(&ring(r, y), Vec3::Y * (y + 1.0), linear(c, 1.0));
        }
        if r2 > 1e-4 {
            m.poly(&ring(r2, y2), Vec3::Y * (y2 - 1.0), linear(c2, 1.0));
        }
    }
}

/// The bottle's glass: a round flask (radius 0.13 m) of glowing cyan potion
/// under pale glass shoulders and a short neck. Its base sits at y = 0.
pub fn potion_glass() -> Mesh {
    let mut m = ModelBuilder::new();
    lathe(
        &mut m,
        &[
            (0.075, 0.0, POTION_DEEP),
            (0.118, 0.03, POTION),
            (0.132, 0.085, POTION),
            (0.126, 0.13, POTION_BRIGHT),
            (0.104, 0.165, GLASS),
            (0.068, 0.19, GLASS),
            (0.046, 0.205, GLASS),
            (0.042, 0.255, GLASS),
        ],
        10,
        0.0,
    );
    m.build()
}

/// The cork and the gold collar round the neck.
pub fn potion_trim() -> Mesh {
    let mut m = ModelBuilder::new();
    lathe(
        &mut m,
        &[
            (0.056, 0.2, COLLAR),
            (0.058, 0.212, COLLAR),
            (0.056, 0.224, COLLAR),
        ],
        10,
        0.0,
    );
    lathe(
        &mut m,
        &[
            (0.036, 0.235, CORK),
            (0.048, 0.25, CORK),
            (0.052, 0.3, CORK),
            (0.04, 0.315, CORK),
        ],
        8,
        0.2,
    );
    m.build()
}

fn setup_potion_fx(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut toon: ResMut<Assets<ToonMaterial>>,
    mut spell: ResMut<Assets<SpellMaterial>>,
    mut warmup: Warmup,
) {
    let assets = PotionFxAssets {
        glass: meshes.add(with_outline_normals(potion_glass())),
        trim: meshes.add(with_outline_normals(potion_trim())),
        glass_material: toon.add(
            ToonMaterial::vertex_colored()
                .with_emissive(POTION, 0.22)
                .with_rim(1.6)
                .with_specular(0.8, 40.0),
        ),
        trim_material: toon.add(ToonMaterial::vertex_colored()),
        glow: spell.add(SpellMaterial::default()),
        glint: meshes.add(sparkle(POTION_BRIGHT, Color::WHITE)),
        mote: meshes.add(sparkle_cross(POTION, POTION_BRIGHT)),
        burst: meshes.add(starburst(
            10,
            0x9071,
            POTION,
            &[POTION_BRIGHT, POTION, POTION_DEEP],
            0.2,
        )),
    };
    let sparks = (0..SPARK_POOL)
        .map(|_| {
            commands
                .spawn((
                    Name::new("Potion sparkle"),
                    PotionFx,
                    Mesh3d(assets.mote.clone()),
                    MeshMaterial3d(assets.glow.clone()),
                    glow_tag(Color::WHITE, 0.0),
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
                    Name::new("Potion flash"),
                    PotionFx,
                    Halo::new(POTION, 1.0, 0.0),
                    Transform::default(),
                    Visibility::Hidden,
                ))
                .id()
        })
        .collect();
    // Compiled behind the loading screen: the bottle (toon + ink hull) and
    // its glow.
    warmup.add_with(
        assets.glass.clone(),
        assets.glass_material.clone(),
        Outline::default(),
    );
    warmup.add_with(
        assets.trim.clone(),
        assets.trim_material.clone(),
        Outline::default(),
    );
    warmup.add_with(
        assets.glint.clone(),
        assets.glow.clone(),
        glow_tag(Color::WHITE, 1.0),
    );
    commands.insert_resource(PotionFxPools {
        slots: SlotPool::new(SPARK_POOL),
        sparks,
        live: vec![None; SPARK_POOL],
        flash_slots: SlotPool::new(FLASH_POOL),
        flashes,
        flash_live: vec![None; FLASH_POOL],
        rng: FxRng::new(0x0090_710F),
        clock: 0.0,
        next_mote: 0.0,
    });
    commands.insert_resource(assets);
}

/// Gives each potion slot its bottle, glint and halo, once.
fn dress_potions(
    mut commands: Commands,
    assets: Option<Res<PotionFxAssets>>,
    slots: Query<Entity, (With<PotionSlot>, Without<PotionDressed>)>,
) {
    let Some(assets) = assets else {
        return;
    };
    for slot in &slots {
        let look = commands
            .spawn((
                Name::new("Potion bottle"),
                PotionFx,
                Transform::from_translation(Vec3::Y * HOVER),
                Visibility::Inherited,
                ChildOf(slot),
            ))
            .with_children(|b| {
                b.spawn((
                    Mesh3d(assets.glass.clone()),
                    MeshMaterial3d(assets.glass_material.clone()),
                    Outline::ink(INK),
                    NotShadowCaster,
                ));
                b.spawn((
                    Mesh3d(assets.trim.clone()),
                    MeshMaterial3d(assets.trim_material.clone()),
                    Outline::ink(INK),
                    NotShadowCaster,
                ));
            })
            .id();
        let glint = commands
            .spawn((
                Name::new("Potion glint"),
                PotionFx,
                Mesh3d(assets.glint.clone()),
                MeshMaterial3d(assets.glow.clone()),
                glow_tag(Color::WHITE, 1.4),
                Transform::from_scale(Vec3::splat(0.14)),
                Visibility::Inherited,
                NotShadowCaster,
                NoOutline,
                ChildOf(slot),
            ))
            .id();
        let halo = commands
            .spawn((
                Name::new("Potion halo"),
                PotionFx,
                Halo::new(POTION, 0.9, 0.9),
                Transform::from_translation(Vec3::Y * (HOVER + 0.12)),
                Visibility::Inherited,
                ChildOf(slot),
            ))
            .id();
        commands
            .entity(slot)
            .insert(PotionDressed { look, glint, halo });
    }
}

/// The bottle's pose `age` seconds after it dropped, at effect clock `clock`
/// (s), with `phase` staggering bottles: (height above the ground, turn,
/// tilt, scale).
pub fn bottle_pose(age: f32, clock: f32, phase: f32) -> (f32, f32, f32, f32) {
    let t = clock + phase;
    let height = HOVER + BOB * (t * BOB_RATE).sin();
    let turn = t * SPIN;
    let tilt = 0.12 * (t * BOB_RATE * 0.5).sin();
    let pop = if age < POP_SECONDS {
        let x = (age / POP_SECONDS).clamp(0.0, 1.0);
        // Swells past full size, then settles.
        1.0 - (1.0 - x).powi(3) + 0.35 * (x * std::f32::consts::PI).sin() * (1.0 - x)
    } else {
        1.0
    };
    (height, turn, tilt, pop)
}

/// Whether a potion with `left` seconds on the ground shows this instant (it
/// blinks, faster and faster, through its last [`BLINK_SECONDS`]).
pub fn blink_visible(left: f32, clock: f32) -> bool {
    if left >= BLINK_SECONDS {
        return true;
    }
    let rate = 4.0 + 8.0 * (1.0 - left / BLINK_SECONDS).clamp(0.0, 1.0);
    (clock * rate).fract() < 0.65
}

/// The sparkle pop where a potion drops, and the burst where it's drunk.
fn emit_potion_fx(
    time: FreezableTime,
    assets: Option<Res<PotionFxAssets>>,
    pools: Option<ResMut<PotionFxPools>>,
    mut cues: MessageReader<GameCue>,
    slots: Query<&PotionSlot>,
) {
    let (Some(assets), Some(mut pools)) = (assets, pools) else {
        cues.clear();
        return;
    };
    let pools = &mut *pools;
    pools.clock += time.delta_secs();
    for cue in cues.read() {
        match *cue {
            GameCue::PotionDropped { at, .. } => {
                let centre = at + Vec3::Y * (HOVER + 0.12);
                pools.flash(centre, 1.2, 2.0, 0.3);
                for _ in 0..6 {
                    let dir = pools.rng.dir() + Vec3::Y * 0.6;
                    sparkle_out(pools, &assets.mote, centre, dir, (1.0, 2.2), 0.09);
                }
            }
            GameCue::PotionPicked { at, .. } => {
                let centre = at + Vec3::Y * (HOVER + 0.12);
                // Around the drinker's view too, so the burst is seen.
                pools.flash(centre + Vec3::Y * 0.9, 2.4, 2.2, 0.35);
                let mut pop = Spark {
                    p: Particle {
                        pos: centre,
                        life: 0.22,
                        size: Vec3::splat(0.7),
                        birth_scale: 0.4,
                        grow: 1.4,
                        shrink_start: 0.5,
                        ..default()
                    },
                    mesh: assets.burst.clone(),
                    roll: 0.0,
                    spin: 3.0,
                    intensity: 2.2,
                    fresh: true,
                };
                pop.roll = pools.rng.range(0.0, std::f32::consts::TAU);
                pools.spark(pop);
                for i in 0..18 {
                    let dir = pools.rng.dir() + Vec3::Y * 1.1;
                    let mesh = if i % 3 == 0 {
                        &assets.glint
                    } else {
                        &assets.mote
                    };
                    sparkle_out(pools, mesh, centre, dir, (3.5, 6.5), 0.12);
                }
            }
            _ => {}
        }
    }
    // Motes drifting up off every potion on the ground.
    if pools.clock >= pools.next_mote {
        pools.next_mote = pools.clock + MOTE_EVERY;
        for slot in &slots {
            if let Some(live) = slot.live {
                let jitter = pools.rng.dir() * Vec3::new(0.12, 0.05, 0.12);
                let at = live.at + Vec3::Y * (HOVER + 0.08) + jitter;
                let mut s = Spark {
                    p: Particle {
                        pos: at,
                        vel: Vec3::Y * pools.rng.range(0.35, 0.6),
                        life: pools.rng.range(0.7, 1.0),
                        size: Vec3::splat(pools.rng.range(0.05, 0.08)),
                        birth_scale: 0.3,
                        shrink_start: 0.5,
                        ..default()
                    },
                    mesh: assets.mote.clone(),
                    roll: 0.0,
                    spin: 2.0,
                    intensity: 1.3,
                    fresh: true,
                };
                s.roll = pools.rng.range(0.0, std::f32::consts::TAU);
                pools.spark(s);
            }
        }
    }
}

/// One sparkle flung from `at` along `dir`.
fn sparkle_out(
    pools: &mut PotionFxPools,
    mesh: &Handle<Mesh>,
    at: Vec3,
    dir: Vec3,
    speed: (f32, f32),
    size: f32,
) {
    let v = dir.normalize_or(Vec3::Y) * pools.rng.range(speed.0, speed.1);
    let k = pools.rng.range(0.7, 1.3);
    let roll = pools.rng.range(0.0, std::f32::consts::TAU);
    let spin = pools.rng.range(-6.0, 6.0);
    let life = pools.rng.range(0.35, 0.6);
    pools.spark(Spark {
        p: Particle {
            pos: at,
            vel: v,
            drag: 3.0,
            gravity: 2.5,
            life,
            size: Vec3::splat(size * k),
            birth_scale: 0.3,
            shrink_start: 0.45,
            ..default()
        },
        mesh: mesh.clone(),
        roll,
        spin,
        intensity: 1.8,
        fresh: true,
    });
}

type PartQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Transform,
        &'static mut Visibility,
        Option<&'static mut Mesh3d>,
        Option<&'static mut MeshTag>,
        Option<&'static mut Halo>,
    ),
    (With<PotionFx>, Without<MainCamera>, Without<PotionSlot>),
>;

/// Bobs, turns and blinks every live potion; moves the sparkles and flashes.
fn animate_potions(
    time: FreezableTime,
    tick: Option<Res<SimTick>>,
    pools: Option<ResMut<PotionFxPools>>,
    camera: Option<Single<&Transform, (With<MainCamera>, Without<PotionFx>)>>,
    slots: Query<(Entity, &PotionSlot, &PotionDressed)>,
    mut parts: PartQuery,
) {
    let Some(mut pools) = pools else {
        return;
    };
    let pools = &mut *pools;
    let dt = time.delta_secs();
    let clock = pools.clock;
    let now = tick.map_or(0, |t| t.0);
    let view = camera.map_or(Quat::IDENTITY, |c| c.rotation);

    for (slot, potion, dressed) in &slots {
        let Some(live) = potion.live else {
            continue;
        };
        let age = now.saturating_sub(live.dropped) as f32 * TICK_SECONDS;
        let left = live.expires.saturating_sub(now) as f32 * TICK_SECONDS;
        let phase = (slot.to_bits() % 7) as f32 * 0.9;
        let (height, turn, tilt, pop) = bottle_pose(age, clock, phase);
        let on = blink_visible(left, clock);
        let vis = if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if let Ok((mut tf, mut v, ..)) = parts.get_mut(dressed.look) {
            tf.translation = Vec3::Y * height;
            tf.rotation = Quat::from_rotation_y(turn) * Quat::from_rotation_z(tilt);
            tf.scale = Vec3::splat((pop * BOTTLE_SCALE).max(1e-3));
            v.set_if_neq(vis);
        }
        if let Ok((mut tf, mut v, ..)) = parts.get_mut(dressed.glint) {
            let twinkle = 0.75 + 0.35 * (clock * 5.3 + phase).sin().max(0.0);
            tf.translation = Vec3::new(0.08, height + 0.22, 0.0);
            tf.rotation = billboard(view, clock * 1.3 + phase);
            tf.scale = Vec3::splat(0.16 * twinkle * pop);
            v.set_if_neq(vis);
        }
        if let Ok((mut tf, mut v, _, _, Some(mut halo))) = parts.get_mut(dressed.halo) {
            tf.translation = Vec3::Y * (height + 0.1);
            let pulse = 0.9 + 0.25 * (clock * 3.0 + phase).sin();
            let want = Halo::new(POTION, 0.95 * pop.max(0.2), pulse);
            if *halo != want {
                *halo = want;
            }
            v.set_if_neq(vis);
        }
    }

    // Sparkles.
    for i in 0..pools.live.len() {
        let Some(spark) = pools.live[i].as_mut() else {
            continue;
        };
        let Ok((mut tf, mut v, mesh, tag, _)) = parts.get_mut(pools.sparks[i]) else {
            continue;
        };
        if spark.fresh {
            spark.fresh = false;
            if let Some(mut mesh) = mesh {
                mesh.0 = spark.mesh.clone();
            }
        } else if !spark.p.step(dt) {
            v.set_if_neq(Visibility::Hidden);
            pools.live[i] = None;
            pools.slots.free(i);
            continue;
        }
        spark.roll += spark.spin * dt;
        *tf = Transform {
            translation: spark.p.pos,
            rotation: billboard(view, spark.roll),
            scale: spark.p.scale().max(Vec3::splat(1e-4)),
        };
        let t = (spark.p.age / spark.p.life.max(1e-4)).clamp(0.0, 1.0);
        let fade = if t < 0.5 { 1.0 } else { 1.0 - (t - 0.5) / 0.5 };
        if let Some(mut tag) = tag {
            tag.set_if_neq(glow_tag(Color::WHITE, spark.intensity * fade));
        }
        v.set_if_neq(Visibility::Visible);
    }

    // Flashes.
    for i in 0..pools.flash_live.len() {
        let Some(flash) = pools.flash_live[i].as_mut() else {
            continue;
        };
        let Ok((mut tf, mut v, _, _, Some(mut halo))) = parts.get_mut(pools.flashes[i]) else {
            continue;
        };
        flash.age += dt;
        if flash.age >= flash.life {
            v.set_if_neq(Visibility::Hidden);
            pools.flash_live[i] = None;
            pools.flash_slots.free(i);
            continue;
        }
        let t = flash.age / flash.life;
        tf.translation = flash.pos;
        *halo = Halo::new(
            POTION,
            flash.size * (1.0 + 0.4 * t),
            flash.intensity * (1.0 - t) * (1.0 - t),
        );
        v.set_if_neq(Visibility::Visible);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bottle_hovers_bobs_and_pops_in() {
        let (h0, ..) = bottle_pose(5.0, 0.0, 0.0);
        assert!((h0 - HOVER).abs() < 1e-5);
        let heights: Vec<f32> = (0..60)
            .map(|i| bottle_pose(5.0, i as f32 * 0.05, 0.0).0)
            .collect();
        let lo = heights.iter().copied().fold(f32::MAX, f32::min);
        let hi = heights.iter().copied().fold(f32::MIN, f32::max);
        assert!(lo > HOVER - BOB - 1e-4 && hi < HOVER + BOB + 1e-4);
        assert!(hi - lo > BOB, "it bobs");
        assert_eq!(bottle_pose(0.0, 0.0, 0.0).3, 0.0, "pops in from nothing");
        let peak = (1..20)
            .map(|i| bottle_pose(POP_SECONDS * i as f32 / 20.0, 0.0, 0.0).3)
            .fold(0.0, f32::max);
        assert!(peak > 1.05, "overshoots: {peak}");
        assert_eq!(bottle_pose(POP_SECONDS, 0.0, 0.0).3, 1.0);
    }

    #[test]
    fn it_blinks_only_in_its_last_seconds() {
        assert!((0..100).all(|i| blink_visible(5.0, i as f32 * 0.013)));
        let shown = (0..100)
            .filter(|&i| blink_visible(1.0, i as f32 * 0.013))
            .count();
        assert!((30..90).contains(&shown), "{shown}");
    }

    #[test]
    fn the_bottle_is_cheap_and_about_a_hand_tall() {
        let glass = potion_glass();
        let trim = potion_trim();
        let tris = |m: &Mesh| m.indices().map_or(0, |i| i.len() / 3);
        assert!(tris(&glass) + tris(&trim) < 400);
        let top = |m: &Mesh| {
            m.attribute(Mesh::ATTRIBUTE_POSITION)
                .and_then(|a| a.as_float3())
                .map_or(0.0, |p| p.iter().map(|v| v[1]).fold(f32::MIN, f32::max))
        };
        assert!((0.25..0.35).contains(&top(&trim)));
        assert!((0.2..0.3).contains(&top(&glass)));
    }
}
