//! The dropped hat (T08): when the knight is eliminated his hat (hidden on his
//! head at once by `crate::knight`) becomes a prop that pops up, spins like a
//! top, lands on the grass, wobbles, settles and stays there until he respawns.
//!
//! The props are a fixed pool of [`HAT_POOL`] entities made at startup. Each
//! wears the `knight_hat` model (`art/blender/assets/knight.py`: the knight's
//! own `Hat` geometry, exported on its own with its pivot at the brim's base)
//! once the model library has loaded, in the knight's material and ink. The
//! physics is [`HatBody`] (pure, in `fx::sim`). Without the model library
//! (headless tests) the props still fly and settle, just without a mesh.

use super::{
    material::glow_tag,
    sim::{HatBody, SlotPool},
    spells::SpellAssets,
};
use crate::{
    combat::Downed,
    knight::KnightAssets,
    look::{BlobShadow, ModelDressed, Outline, ToonMaterial, warmup::Warmup},
    models::{EMBEDDED_MODELS, ModelLibrary, Sidecar, spawn_model},
    shared::{Character, FreezableTime},
};
use bevy::{light::NotShadowCaster, mesh::MeshTag, prelude::*};

/// The hat model's name in `assets/models/manifest.json`.
pub const HAT_MODEL: &str = "knight_hat";
/// How many hats can lie on the grass at once (one per eliminated knight).
pub const HAT_POOL: usize = 4;
/// Blob shadow under a hat.
const HAT_SHADOW: f32 = 0.26;
/// Where the hat sits on the knight's head (feet space, m), when his rig
/// can't say (no model).
pub const HAT_HEIGHT: f32 = 1.72;
/// The dropped hat grows this much bigger than it sat on his head over its
/// first [`HAT_GROW_TIME`] s, cartoon-style, so it reads on the grass (T08).
pub const HAT_SCALE: f32 = 1.3;
pub const HAT_GROW_TIME: f32 = 0.2;

/// A pooled hat prop's root. `victim` is the knight it fell off, while shown.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct HatProp {
    pub victim: Option<Entity>,
    /// It has come to rest on the ground.
    pub settled: bool,
}

/// On a hat prop's model root (a child of the prop).
#[derive(Component, Debug, Clone, Copy)]
pub struct HatModel;

/// The speed lines circling a hat while it spins on the grass (a child of the
/// prop, so it turns with the hat).
#[derive(Component, Debug, Clone, Copy)]
pub struct HatSwirl;

/// Swirl radius (m) and height above the pivot, and its glow at full spin.
const SWIRL_RADIUS: f32 = 0.33;
const SWIRL_HEIGHT: f32 = 0.05;
const SWIRL_GLOW: f32 = 1.3;
/// Spin (rad/s) at which the swirl is at full strength.
const SWIRL_SPIN: f32 = 9.0;

struct HatSlot {
    entity: Entity,
    swirl: Entity,
    body: Option<HatBody>,
    victim: Option<Entity>,
    fresh: bool,
}

/// The pool of hat props.
#[derive(Resource)]
pub struct HatPool {
    slots: SlotPool,
    hats: Vec<HatSlot>,
    /// The pivot's height above the ground at rest (from the model's bounds).
    rest: f32,
    models_attached: bool,
    warmed: bool,
}

impl HatPool {
    /// How many hats are out.
    pub fn live(&self) -> usize {
        self.hats.iter().filter(|h| h.body.is_some()).count()
    }

    pub fn capacity(&self) -> usize {
        self.hats.len()
    }

    /// Drops a hat off `victim`'s head at `from` (its pivot, world) facing
    /// `yaw`, launched with `vel` and `spin`, over ground at `ground`. If every
    /// hat is out, the oldest is taken back.
    pub fn drop_hat(
        &mut self,
        victim: Entity,
        from: Vec3,
        yaw: f32,
        vel: Vec3,
        spin: f32,
        ground: f32,
    ) {
        // One hat per knight.
        let slot = self
            .hats
            .iter()
            .position(|h| h.victim == Some(victim))
            .or_else(|| self.slots.alloc(self.hats.len()));
        let Some(i) = slot else { return };
        let hat = &mut self.hats[i];
        hat.body = Some(HatBody::launch(from, vel, yaw, spin, ground, self.rest));
        hat.victim = Some(victim);
        hat.fresh = true;
    }

    /// Takes back the hat that fell off `victim` (on respawn); returns where it lay.
    pub fn pick_up(&mut self, victim: Entity) -> Option<Vec3> {
        let i = self.hats.iter().position(|h| h.victim == Some(victim))?;
        let hat = &mut self.hats[i];
        let at = hat.body.map(|b| b.pos);
        hat.body = None;
        hat.victim = None;
        self.slots.free(i);
        at
    }
}

/// The hat model's rest height: how far its lowest point hangs below the
/// pivot, plus the dip of a slightly tilted brim.
fn rest_height() -> f32 {
    let below = EMBEDDED_MODELS
        .iter()
        .find(|m| m.name == HAT_MODEL)
        .and_then(|m| Sidecar::parse(m.sidecar).ok())
        .map_or(0.03, |s| (-s.bounds.min[1]).max(0.0));
    below + 0.02
}

pub(super) fn spawn_hat_pool(commands: &mut Commands, assets: &SpellAssets) -> HatPool {
    let hats = (0..HAT_POOL)
        .map(|_| {
            let entity = commands
                .spawn((
                    Name::new("Hat prop"),
                    HatProp::default(),
                    BlobShadow::new(HAT_SHADOW),
                    Transform::default(),
                    Visibility::Hidden,
                ))
                .id();
            let swirl = commands
                .spawn((
                    Name::new("Hat swirl"),
                    HatSwirl,
                    Mesh3d(assets.swirl.clone()),
                    MeshMaterial3d(assets.material.clone()),
                    glow_tag(Color::WHITE, 0.0),
                    Transform::from_xyz(0.0, SWIRL_HEIGHT, 0.0)
                        .with_scale(Vec3::splat(SWIRL_RADIUS * 2.0)),
                    Visibility::Hidden,
                    NotShadowCaster,
                    ChildOf(entity),
                ))
                .id();
            HatSlot {
                entity,
                swirl,
                body: None,
                victim: None,
                fresh: false,
            }
        })
        .collect();
    HatPool {
        slots: SlotPool::new(HAT_POOL),
        hats,
        rest: rest_height() * HAT_SCALE,
        models_attached: false,
        warmed: false,
    }
}

/// Once the model library is ready, puts a `knight_hat` model on every prop.
pub(super) fn attach_hat_models(
    mut commands: Commands,
    library: Option<Res<ModelLibrary>>,
    pool: Option<ResMut<HatPool>>,
) {
    let (Some(library), Some(mut pool)) = (library, pool) else {
        return;
    };
    if pool.models_attached || !library.is_ready() {
        return;
    }
    pool.models_attached = true;
    if library.failed().iter().any(|f| f == HAT_MODEL) {
        error!("fx: no `{HAT_MODEL}` model; dropped hats will be invisible");
        return;
    }
    for hat in &pool.hats {
        match spawn_model(&mut commands, &library, HAT_MODEL, Transform::IDENTITY) {
            Some(model) => {
                commands
                    .entity(model)
                    .insert((HatModel, Outline::default(), ChildOf(hat.entity)));
            }
            None => {
                error!("fx: `{HAT_MODEL}` is not in the model library");
                return;
            }
        }
    }
}

/// Gives a dressed hat model the knight's material (warm rim) and warms its
/// draw up behind the loading screen.
#[allow(clippy::too_many_arguments)]
pub(super) fn configure_hat_models(
    mut dressed: MessageReader<ModelDressed>,
    hat_models: Query<(), With<HatModel>>,
    children: Query<&Children>,
    toon_meshes: Query<&Mesh3d, With<MeshMaterial3d<ToonMaterial>>>,
    knight: Option<Res<KnightAssets>>,
    pool: Option<ResMut<HatPool>>,
    mut warmup: Warmup,
    mut commands: Commands,
) {
    let (Some(knight), Some(mut pool)) = (knight, pool) else {
        dressed.clear();
        return;
    };
    for event in dressed.read() {
        if !hat_models.contains(event.root) {
            continue;
        }
        for entity in children.iter_descendants(event.root) {
            if let Ok(mesh) = toon_meshes.get(entity) {
                commands
                    .entity(entity)
                    .insert(MeshMaterial3d(knight.material.clone()));
                if !pool.warmed {
                    pool.warmed = true;
                    warmup.add_with(mesh.0.clone(), knight.material.clone(), Outline::default());
                }
            }
        }
    }
}

/// How strongly a hat's swirl shows: only once it spins on the ground.
pub fn swirl_strength(body: &HatBody) -> f32 {
    if body.landed && !body.settled {
        (body.spin.abs() / SWIRL_SPIN).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Flies, lands and settles every dropped hat; hides hats whose knight is back.
pub(super) fn simulate_hats(
    time: FreezableTime,
    pool: Option<ResMut<HatPool>>,
    victims: Query<Has<Downed>, With<Character>>,
    mut props: Query<(&mut HatProp, &mut Transform, &mut Visibility), Without<HatSwirl>>,
    mut swirls: Query<(&mut MeshTag, &mut Visibility), (With<HatSwirl>, Without<HatProp>)>,
) {
    let Some(mut pool) = pool else { return };
    let dt = time.delta_secs();
    let pool = &mut *pool;
    for i in 0..pool.hats.len() {
        let hat = &mut pool.hats[i];
        let Ok((mut prop, mut transform, mut visibility)) = props.get_mut(hat.entity) else {
            continue;
        };
        // The knight is back (or gone): the hat goes with him.
        let back = hat.victim.is_some_and(|v| !victims.get(v).unwrap_or(false));
        if back {
            hat.body = None;
            hat.victim = None;
            pool.slots.free(i);
        }
        let swirl = swirls.get_mut(hat.swirl).ok();
        let Some(body) = hat.body.as_mut() else {
            visibility.set_if_neq(Visibility::Hidden);
            if prop.victim.is_some() {
                *prop = HatProp::default();
            }
            continue;
        };
        if hat.fresh {
            hat.fresh = false;
        } else {
            body.step(dt);
        }
        let t = (body.age / HAT_GROW_TIME).clamp(0.0, 1.0);
        let grow = 1.0 + (HAT_SCALE - 1.0) * t * t * (3.0 - 2.0 * t);
        *transform = body.transform().with_scale(Vec3::splat(grow));
        visibility.set_if_neq(Visibility::Visible);
        if let Some((mut tag, mut vis)) = swirl {
            let k = swirl_strength(body);
            let want = glow_tag(Color::WHITE, SWIRL_GLOW * k);
            if tag.0 != want.0 {
                *tag = want;
            }
            vis.set_if_neq(if k > 0.02 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
        let next = HatProp {
            victim: hat.victim,
            settled: body.settled,
        };
        if prop.victim != next.victim || prop.settled != next.settled {
            *prop = next;
        }
    }
}

/// The hat's launch when its knight is eliminated: up and toward whoever is
/// watching (so it lands in view, as in T08), a little sideways, spinning.
pub fn hat_launch(toward: Vec3, side: f32, spin_sign: f32) -> (Vec3, f32) {
    let toward = toward.with_y(0.0).normalize_or(Vec3::Z);
    let right = Vec3::Y.cross(toward);
    let vel = Vec3::Y * 1.2 + toward * 1.25 + right * side;
    (vel, 11.0 * spin_sign)
}
