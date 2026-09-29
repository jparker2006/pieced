//! Physical deaths (docs/M4-SPEC.md → Chunk 1, D105): when a knight goes
//! down his helmet pops off spinning and his gauntlets and boots fly off on
//! ballistic arcs, bounce on the island top or a piece's top, clatter on
//! their first bounce (at most [`KillFeelTuning::clatter_voices`] at once),
//! then shrink away within [`KillFeelTuning::armor_seconds`] (1.5 s). The
//! existing poof and hat drop play over it. Flung into the void, he comes
//! apart as he flies ([`GameCue::VoidFall`]) and the pieces fall with him.
//!
//! Also here: the **dented helmet**. A headshot swaps his helmet for the
//! `knight_helmet_dent` model (`art/blender/assets/knight.py`) until he goes
//! down or respawns ([`KnightArmorRig`], shown by `knight::write_armor`).
//!
//! **Cost.** A fixed pool of `max_armor` (48) entities made at startup
//! wear the knight's own part meshes and material (the same pipelines as the
//! knight, warmed behind the loading screen with him) and his ink. Per frame
//! only transforms move; a moving piece casts one short ray down (pieces and
//! the island top), a resting one none. Nothing is spawned or allocated per
//! death. The simulation is [`super::chunks`] (no avian bodies, never touches
//! the player). Without the model (headless tests) the pieces still fly and
//! land from where his parts would be, just without meshes.

use super::{
    chunks::{Chunk, ClatterGate},
    kills::KillFeelTuning,
    sim::{FxRng, SlotPool},
};
use crate::{
    arena::{IslandRim, visuals::TargetFigure},
    knight::{ARMOR_PARTS, KnightAnim, KnightArmorRig, KnightAssets, KnightRig},
    look::{
        ModelDressed, Outline, ToonMaterial,
        warmup::{Warmup, WarmupState},
    },
    models::{ModelLibrary, ModelParts, spawn_model},
    movement::{ISLAND_TOP, VoidFall},
    shared::{
        Character, DamageDealt, DamageTarget, Eliminated, FreezableTime, GameCue, Layer,
        LookAngles, Player,
    },
    tuning::Tuning,
};
use avian3d::prelude::*;
use bevy::{camera::primitives::MeshAabb, light::NotShadowCaster, prelude::*};

/// The dented helmet's model and its part.
pub const DENT_MODEL: &str = "knight_helmet_dent";
pub const DENT_PART: &str = "HelmetDent";
/// Hard ceiling on the armor pool, whatever the tuning says.
pub const ARMOR_CEILING: u32 = 128;
/// Clatter sound length (s), for the voice cap.
pub const CLATTER_SECONDS: f32 = 0.35;
/// Where the template dented helmet waits while it is dressed (far below).
const PARK: Vec3 = Vec3::new(0.0, -80.0, 0.0);
/// Rays for the ground under a flying piece start this far above it and
/// reach this far down (m).
const PROBE_UP: f32 = 0.4;
const PROBE_DEPTH: f32 = 40.0;

/// Where each [`ARMOR_PARTS`] piece sits on a standing knight (model space:
/// feet at the origin, +X his right, -Z his front), and its resting
/// half-height: used when there is no rigged model to take them from.
const FALLBACK: [(Vec3, f32); ARMOR_PARTS.len()] = [
    (Vec3::new(0.0, 1.6, 0.0), 0.16),
    (Vec3::new(-0.27, 0.9, -0.03), 0.07),
    (Vec3::new(0.27, 0.9, -0.03), 0.07),
    (Vec3::new(-0.17, 0.2, -0.06), 0.09),
    (Vec3::new(0.17, 0.2, -0.06), 0.09),
];

/// A clatter: an armor piece's first bounce (the audio plays it, capped).
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct ArmorClattered {
    pub at: Vec3,
}

/// Marks a pooled armor piece entity.
#[derive(Component, Debug, Clone, Copy)]
pub struct ArmorPiece;

/// The knight's armor meshes, shared by every knight: per [`ARMOR_PARTS`]
/// part, its mesh, its centre in the part node's frame and its resting
/// half-height; and the dented helmet. Taken from the first rigged knight.
#[derive(Resource, Debug, Clone, Default)]
pub struct ArmorMeshes {
    pub parts: Vec<(Handle<Mesh>, Vec3, f32)>,
    pub dent: Option<Handle<Mesh>>,
}

/// The dented helmet's mesh once its model is dressed.
#[derive(Resource, Debug, Clone, Default)]
struct DentModel {
    spawned: bool,
    template: Option<Entity>,
    mesh: Option<Handle<Mesh>>,
}

/// One flying piece.
#[derive(Debug, Clone, Copy)]
struct Flying {
    chunk: Chunk,
    part: usize,
    dented: bool,
    /// Mesh centre in the node's frame (scaled), so it tumbles about its middle.
    offset: Vec3,
    scale: Vec3,
    floor: Option<f32>,
    fresh: bool,
}

/// The armor pool (see the module docs).
#[derive(Resource)]
pub struct ArmorPool {
    slots: SlotPool,
    entities: Vec<Entity>,
    live: Vec<Option<Flying>>,
    gate: ClatterGate,
    rng: FxRng,
    /// The pool entities wear the knight's meshes (set once).
    dressed: bool,
    /// Real seconds, for the clatter voice cap.
    clock: f64,
}

impl ArmorPool {
    /// Pieces flying or lying now.
    pub fn live(&self) -> usize {
        self.slots.live_count()
    }

    pub fn capacity(&self) -> usize {
        self.entities.len()
    }

    /// The live pieces' positions (for tests and the session log).
    pub fn positions(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.live.iter().flatten().map(|f| f.chunk.pos)
    }

    /// Clatter voices sounding now.
    pub fn clatters(&self) -> usize {
        self.gate.sounding(self.clock)
    }
}

/// The physical deaths and the dented helmet (client; headless-safe).
pub struct ArmorPlugin;

impl Plugin for ArmorPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ArmorClattered>()
            .add_message::<Eliminated>()
            .add_message::<DamageDealt>()
            .add_message::<GameCue>()
            .add_message::<ModelDressed>()
            .init_resource::<DentModel>()
            .init_resource::<crate::app::BootGate>()
            .init_resource::<WarmupState>()
            .add_systems(Startup, spawn_armor_pool)
            .add_systems(
                Update,
                (spawn_dent_template, capture_dent_mesh, rig_knight_armor).chain(),
            )
            .add_systems(
                PostUpdate,
                (emit_armor, simulate_armor)
                    .chain()
                    .before(crate::arena::visuals::animate_knights)
                    .before(TransformSystems::Propagate),
            );
    }
}

fn spawn_armor_pool(mut commands: Commands, tuning: Res<Tuning>) {
    let n = tuning.kills.max_armor.min(ARMOR_CEILING) as usize;
    let entities = (0..n)
        .map(|_| {
            commands
                .spawn((
                    Name::new("Armor piece"),
                    ArmorPiece,
                    Transform::default(),
                    Visibility::Hidden,
                    NotShadowCaster,
                ))
                .id()
        })
        .collect();
    commands.insert_resource(ArmorPool {
        slots: SlotPool::new(n),
        entities,
        live: vec![None; n],
        gate: ClatterGate::new(tuning.kills.clatter_voices, CLATTER_SECONDS),
        rng: FxRng::new(0xA2_40_0F),
        dressed: false,
        clock: 0.0,
    });
}

/// Once the models are loaded, spawns one dented helmet out of sight so the
/// look dresses it (toon material, outline normals); its mesh is taken then.
fn spawn_dent_template(
    mut commands: Commands,
    library: Option<Res<ModelLibrary>>,
    mut dent: ResMut<DentModel>,
) {
    let Some(library) = library else { return };
    if dent.spawned || !library.is_ready() {
        return;
    }
    dent.spawned = true;
    if library.failed().iter().any(|f| f == DENT_MODEL) {
        error!("fx: no `{DENT_MODEL}` model; headshots won't dent helmets");
        return;
    }
    dent.template = spawn_model(
        &mut commands,
        &library,
        DENT_MODEL,
        Transform::from_translation(PARK),
    );
}

fn capture_dent_mesh(
    mut commands: Commands,
    mut dressed: MessageReader<ModelDressed>,
    parts: ModelParts,
    children: Query<&Children>,
    meshes: Query<&Mesh3d>,
    mut dent: ResMut<DentModel>,
) {
    for event in dressed.read() {
        if event.name != DENT_MODEL || Some(event.root) != dent.template {
            continue;
        }
        dent.mesh = parts
            .find(event.root, DENT_PART)
            .and_then(|node| primitive(node, &children, &meshes))
            .and_then(|e| meshes.get(e).ok())
            .map(|m| m.0.clone());
        if dent.mesh.is_none() {
            error!("fx: `{DENT_MODEL}` has no `{DENT_PART}` mesh");
        }
        commands.entity(event.root).despawn();
        dent.template = None;
    }
}

/// A part node's mesh primitive (its first child with a mesh).
fn primitive(node: Entity, children: &Query<&Children>, meshes: &Query<&Mesh3d>) -> Option<Entity> {
    children
        .get(node)
        .ok()?
        .iter()
        .find(|c| meshes.contains(*c))
}

/// Finds each rigged knight's armor parts; the first one lends its meshes to
/// the pool. Adds the dented helmet under his helmet once its mesh exists.
#[allow(clippy::too_many_arguments)]
fn rig_knight_armor(
    mut commands: Commands,
    parts: ModelParts,
    children: Query<&Children>,
    meshes: Query<&Mesh3d>,
    mesh_assets: Res<Assets<Mesh>>,
    knight: Option<Res<KnightAssets>>,
    dent: Res<DentModel>,
    armor: Option<ResMut<ArmorMeshes>>,
    pool: Option<ResMut<ArmorPool>>,
    mut figures: Query<(Entity, &KnightRig, Option<&mut KnightArmorRig>)>,
    mut warmup: Warmup,
) {
    let mut captured: Option<ArmorMeshes> = None;
    let have_meshes = armor.is_some();
    for (figure, rig, armor_rig) in &mut figures {
        let rig_dent = |commands: &mut Commands, helmet: Entity| -> Option<Entity> {
            let (mesh, knight) = (dent.mesh.clone()?, knight.as_ref()?);
            Some(
                commands
                    .spawn((
                        Name::new(DENT_PART),
                        Mesh3d(mesh),
                        MeshMaterial3d(knight.material.clone()),
                        Transform::IDENTITY,
                        Visibility::Hidden,
                        ChildOf(helmet),
                    ))
                    .id(),
            )
        };
        if let Some(mut armor_rig) = armor_rig {
            if armor_rig.dent.is_none() && dent.mesh.is_some() {
                armor_rig.dent = rig_dent(&mut commands, armor_rig.nodes[0]);
            }
            continue;
        }
        let found: Option<Vec<(Entity, Entity)>> = ARMOR_PARTS
            .iter()
            .map(|name| {
                let node = parts.find(rig.model, name)?;
                Some((node, primitive(node, &children, &meshes)?))
            })
            .collect();
        let Some(found) = found else {
            continue;
        };
        let nodes: [Entity; ARMOR_PARTS.len()] = std::array::from_fn(|i| found[i].0);
        // The helmet hides its own mesh (his eyes stay); the limbs their nodes.
        let shown = std::array::from_fn(|i| if i == 0 { found[i].1 } else { found[i].0 });
        if !have_meshes && captured.is_none() {
            let parts = found
                .iter()
                .enumerate()
                .filter_map(|(i, (_, prim))| {
                    let handle = meshes.get(*prim).ok()?.0.clone();
                    let aabb = mesh_assets.get(&handle).and_then(Mesh::compute_aabb);
                    let (center, half) = aabb.map_or((Vec3::ZERO, FALLBACK[i].1), |b| {
                        let h = Vec3::from(b.half_extents);
                        (Vec3::from(b.center), h.min_element().max(0.05))
                    });
                    Some((handle, center, half))
                })
                .collect::<Vec<_>>();
            if parts.len() == ARMOR_PARTS.len() {
                captured = Some(ArmorMeshes { parts, dent: None });
            }
        }
        // What gets hidden must carry a visibility of its own (a glTF mesh
        // primitive may not).
        for &e in &shown {
            commands.entity(e).insert(Visibility::Inherited);
        }
        let dent_entity = rig_dent(&mut commands, nodes[0]);
        commands.entity(figure).insert(KnightArmorRig {
            nodes,
            shown,
            dent: dent_entity,
        });
    }
    if let Some(mut armor) = armor
        && armor.dent.is_none()
        && let (Some(mesh), Some(knight)) = (dent.mesh.clone(), knight.as_ref())
    {
        armor.dent = Some(mesh.clone());
        // The dented helmet's draw, compiled behind the loading screen.
        warmup.add_with(mesh, knight.material.clone(), Outline::default());
    }
    let Some(captured) = captured else { return };
    // Dress the pool once: the knight's material, a mesh to start from, ink.
    if let (Some(mut pool), Some(knight)) = (pool, knight.as_ref())
        && !pool.dressed
    {
        pool.dressed = true;
        let first = captured.parts[0].0.clone();
        for &e in &pool.entities {
            commands.entity(e).insert((
                Mesh3d(first.clone()),
                MeshMaterial3d::<ToonMaterial>(knight.material.clone()),
                Outline::default(),
            ));
        }
    }
    commands.insert_resource(captured);
}

/// The ground under `p`: a piece's top below it, else the island top where
/// there is island (none over the void).
fn ground_under(spatial: &SpatialQuery, layout: Option<&IslandRim>, p: Vec3) -> Option<f32> {
    let pieces = SpatialQueryFilter::from_mask([Layer::Piece]);
    let origin = p + Vec3::Y * PROBE_UP;
    let piece = spatial
        .cast_ray(origin, Dir3::NEG_Y, PROBE_DEPTH, false, &pieces)
        .map(|hit| origin.y - hit.distance);
    let island = layout.is_none_or(|l| l.on_island(p)).then_some(ISLAND_TOP);
    match (piece, island) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

/// Launches a downed knight's armor (see the module docs).
#[allow(clippy::too_many_arguments)]
fn emit_armor(
    tuning: Res<Tuning>,
    pool: Option<ResMut<ArmorPool>>,
    meshes: Option<Res<ArmorMeshes>>,
    mut eliminated: MessageReader<Eliminated>,
    mut cues: MessageReader<GameCue>,
    mut damage: MessageReader<DamageDealt>,
    player: Option<Single<Entity, With<Player>>>,
    characters: Query<(&GlobalTransform, Option<&LookAngles>, Option<&VoidFall>), With<Character>>,
    figures: Query<(&TargetFigure, &KnightArmorRig, Option<&KnightAnim>)>,
    nodes: Query<&GlobalTransform>,
    mut stats: Option<ResMut<super::kills::KillFeedbackStats>>,
    mut headshots: Local<Vec<Entity>>,
) {
    let Some(mut pool) = pool else {
        eliminated.clear();
        cues.clear();
        damage.clear();
        return;
    };
    let me = player.map(|p| *p);
    headshots.clear();
    for hit in damage.read() {
        if hit.killed && hit.headshot && hit.target_kind == DamageTarget::Character {
            headshots.push(hit.target);
        }
    }
    let feel = &tuning.kills;
    let limit = feel.max_armor.min(ARMOR_CEILING) as usize;
    let shed = |victim: Entity, void: Option<Vec3>, pool: &mut ArmorPool| -> bool {
        if Some(victim) == me {
            return false;
        }
        let Ok((body, look, _)) = characters.get(victim) else {
            return false;
        };
        let feet = body.translation();
        let figure = figures.iter().find(|(f, _, _)| f.owner == victim);
        if figure.is_some_and(|(_, _, anim)| anim.is_some_and(KnightAnim::armor_is_off)) {
            return false;
        }
        let dented = headshots.contains(&victim)
            || figure.is_some_and(|(_, _, anim)| anim.is_some_and(KnightAnim::is_dented));
        let yaw = look.map_or(0.0, |l| l.yaw);
        let facing = Quat::from_rotation_y(yaw);
        // Away from whoever did it (the player), or along the void fling.
        let away = void
            .or_else(|| {
                me.and_then(|p| characters.get(p).ok())
                    .map(|(t, _, _)| (feet - t.translation()).with_y(0.0))
            })
            .and_then(|d| d.try_normalize())
            .unwrap_or(facing * Vec3::Z);
        let flung = void.is_some();
        for (part, &(local, half)) in FALLBACK.iter().enumerate() {
            let (start, rot, scale, offset, radius) = match (figure, meshes.as_deref()) {
                (Some((_, rig, _)), Some(m)) if m.parts.len() == ARMOR_PARTS.len() => {
                    let Ok(node) = nodes.get(rig.nodes[part]) else {
                        continue;
                    };
                    let (s, r, t) = node.to_scale_rotation_translation();
                    let (_, center, half) = m.parts[part];
                    let offset = s * center;
                    (t + r * offset, r, s, offset, half * s.min_element())
                }
                _ => (feet + facing * local, facing, Vec3::ONE, Vec3::ZERO, half),
            };
            let chunk = launch(
                &mut pool.rng,
                part,
                start,
                rot,
                feet,
                away,
                flung,
                radius,
                feel,
            );
            let Some(i) = pool.slots.alloc(limit) else {
                continue;
            };
            pool.live[i] = Some(Flying {
                chunk,
                part,
                dented,
                offset,
                scale,
                floor: None,
                fresh: true,
            });
        }
        true
    };
    for cue in cues.read() {
        if let GameCue::VoidFall { who } = *cue
            && let Ok((_, _, Some(fall))) = characters.get(who)
        {
            shed(who, Some(fall.direction), &mut pool);
        }
    }
    for kill in eliminated.read() {
        // A knight flung into the void came apart as he flew.
        if characters
            .get(kill.victim)
            .is_ok_and(|(_, _, fall)| fall.is_some())
        {
            continue;
        }
        if shed(kill.victim, None, &mut pool)
            && kill.by.is_some()
            && kill.by == me
            && let Some(stats) = stats.as_mut()
        {
            stats.armor_same_frame += 1;
        }
    }
}

/// The launch of one piece off a downed knight (see the module docs).
#[allow(clippy::too_many_arguments)]
fn launch(
    rng: &mut FxRng,
    part: usize,
    start: Vec3,
    rot: Quat,
    feet: Vec3,
    away: Vec3,
    flung: bool,
    radius: f32,
    feel: &KillFeelTuning,
) -> Chunk {
    let out = (start - feet).with_y(0.0).normalize_or(away);
    let side = Vec3::Y.cross(away);
    let (vel, spin, restitution) = match part {
        // The helmet pops straight up off his head, spinning end over end.
        0 => (
            Vec3::Y * rng.range(5.0, 5.8)
                + away * rng.range(0.9, 1.5)
                + side * rng.range(-0.7, 0.7),
            side * rng.range(11.0, 15.0) + Vec3::Y * rng.range(-3.0, 3.0),
            0.42,
        ),
        // Gauntlets fly out to his sides.
        1 | 2 => (
            out * rng.range(1.8, 2.8) + Vec3::Y * rng.range(3.2, 4.4) + away * rng.range(0.6, 1.4),
            rng.dir() * rng.range(9.0, 16.0),
            0.36,
        ),
        // Boots kick out low.
        _ => (
            out * rng.range(1.0, 1.8) + Vec3::Y * rng.range(2.2, 3.2) + away * rng.range(0.4, 1.0),
            rng.dir() * rng.range(6.0, 11.0),
            0.3,
        ),
    };
    // Flung into the void, the pieces go with him.
    let vel = if flung { vel * 0.7 + away * 4.0 } else { vel };
    Chunk {
        pos: start,
        vel,
        rot,
        spin,
        life: feel.armor_seconds * rng.range(0.9, 1.0),
        shrink_start: 0.7,
        radius,
        restitution,
        ..default()
    }
}

/// Flies, bounces and shrinks every piece; the first bounce clatters.
#[allow(clippy::type_complexity)]
fn simulate_armor(
    time: FreezableTime,
    real: Res<Time<Real>>,
    pool: Option<ResMut<ArmorPool>>,
    meshes: Option<Res<ArmorMeshes>>,
    layout: Option<Res<IslandRim>>,
    spatial: SpatialQuery,
    mut clatter: MessageWriter<ArmorClattered>,
    mut pieces: Query<(&mut Transform, &mut Visibility, Option<&mut Mesh3d>), With<ArmorPiece>>,
) {
    let Some(mut pool) = pool else { return };
    let dt = time.delta_secs();
    let pool = &mut *pool;
    pool.clock += f64::from(real.delta_secs());
    for i in 0..pool.live.len() {
        let Some(piece) = pool.live[i].as_mut() else {
            continue;
        };
        let Ok((mut tf, mut vis, mesh)) = pieces.get_mut(pool.entities[i]) else {
            continue;
        };
        if piece.fresh {
            piece.fresh = false;
            if let (Some(mut mesh), Some(m)) = (mesh, meshes.as_deref()) {
                let want = match (piece.part, &m.dent) {
                    (0, Some(dent)) if piece.dented => Some(dent),
                    (part, _) => m.parts.get(part).map(|p| &p.0),
                };
                if let Some(want) = want
                    && mesh.0 != *want
                {
                    mesh.0 = want.clone();
                }
            }
            vis.set_if_neq(Visibility::Visible);
        } else {
            if piece.floor.is_none() || !piece.chunk.is_resting() {
                piece.floor = ground_under(&spatial, layout.as_deref(), piece.chunk.pos);
            }
            let floor = piece.floor;
            let step = piece.chunk.step(dt, |_| floor);
            if !step.alive {
                vis.set_if_neq(Visibility::Hidden);
                pool.live[i] = None;
                pool.slots.free(i);
                continue;
            }
            if step.first_bounce && pool.gate.try_start(pool.clock) {
                clatter.write(ArmorClattered {
                    at: piece.chunk.pos,
                });
            }
        }
        let c = &piece.chunk;
        // The chunk is the mesh's middle; the entity is the part's node.
        tf.rotation = c.rot;
        tf.scale = (piece.scale * c.shrink()).max(Vec3::splat(1e-4));
        tf.translation = c.pos - c.rot * (piece.offset * c.shrink());
    }
}
