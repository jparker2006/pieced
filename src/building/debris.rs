//! Broken pieces burst into chunks (M4 chunk 5, D109): bricks from a wall,
//! plank splinters from a floor, ramp or cone, launched from across the
//! piece's face on [`crate::fx::chunks`]' physics-lite simulation (the same
//! as the knights' armor, D105). They tumble, bounce on the island top or a
//! piece below, settle, then shrink away, all within [`DEBRIS_SECONDS`];
//! never more than [`DEBRIS_CAP`] at once (the oldest gives way).
//!
//! **Cost.** A fixed pool of [`DEBRIS_CAP`] entities made at startup, wearing
//! the Blender debris models (`PieceDebris`, warmed behind the loading
//! screen with the pieces) and one lit material, so they batch. Per frame
//! only transforms move; a flying chunk casts one short ray down, a resting
//! one none. Nothing is spawned or allocated per break. Headless (no
//! models) the chunks still fly, just without meshes. The dust and crumbs
//! of a break are the effects' (`fx`).

use super::visuals::PieceDebris;
use crate::{
    arena::IslandRim,
    fx::{
        chunks::Chunk,
        sim::{FxRng, SlotPool},
    },
    movement::ISLAND_TOP,
    shared::{FreezableTime, Layer, PieceChange, PieceChanged, PieceKind, Player},
};
use avian3d::prelude::*;
use bevy::{light::NotShadowCaster, prelude::*};

/// Chunks alive at once, whatever is breaking.
pub const DEBRIS_CAP: usize = 48;
/// Every chunk is gone this long after its break (s).
pub const DEBRIS_SECONDS: f32 = 2.0;
/// Chunks per break: a wall's bricks, a plank piece's splinters.
pub const WALL_CHUNKS: usize = 12;
pub const PLANK_CHUNKS: usize = 10;
/// Rays for the ground start this far above a chunk and reach this far down.
const PROBE_UP: f32 = 0.4;
const PROBE_DEPTH: f32 = 30.0;

/// The building debris (client; headless-safe, for tests).
pub struct BuildingDebrisPlugin;

impl Plugin for BuildingDebrisPlugin {
    fn build(&self, app: &mut App) {
        build(app);
    }
}

pub(super) fn build(app: &mut App) {
    app.add_message::<PieceChanged>()
        .add_systems(Startup, spawn_debris_pool)
        .add_observer(remember_frame)
        .add_systems(
            PostUpdate,
            (burst_pieces, simulate_debris)
                .chain()
                .before(TransformSystems::Propagate),
        );
}

/// Marks a pooled debris chunk.
#[derive(Component, Debug)]
pub struct DebrisChunk;

#[derive(Debug, Clone, Copy)]
struct Flying {
    chunk: Chunk,
    brick: bool,
    size: f32,
    floor: Option<f32>,
    fresh: bool,
}

/// The debris pool (see the module docs).
#[derive(Resource)]
pub struct BuildingDebris {
    slots: SlotPool,
    entities: Vec<Entity>,
    live: Vec<Option<Flying>>,
    rng: FxRng,
    /// The latest removed pieces' frames (a ring), so a break flies from
    /// the piece's real face after its entity is gone.
    frames: [(Option<Entity>, Transform); 16],
    next_frame: usize,
    /// Breaks burst so far.
    pub bursts: u32,
}

impl BuildingDebris {
    /// Chunks flying or lying now.
    pub fn live(&self) -> usize {
        self.slots.live_count()
    }

    pub fn capacity(&self) -> usize {
        self.entities.len()
    }

    /// The live chunks' (age, resting) for tests.
    pub fn chunks(&self) -> impl Iterator<Item = (f32, bool)> + '_ {
        self.live
            .iter()
            .flatten()
            .map(|f| (f.chunk.age, f.chunk.is_resting()))
    }
}

fn spawn_debris_pool(mut commands: Commands) {
    let entities = (0..DEBRIS_CAP)
        .map(|_| {
            commands
                .spawn((
                    Name::new("Piece debris"),
                    DebrisChunk,
                    Transform::default(),
                    Visibility::Hidden,
                    NotShadowCaster,
                ))
                .id()
        })
        .collect();
    commands.insert_resource(BuildingDebris {
        slots: SlotPool::new(DEBRIS_CAP),
        entities,
        live: vec![None; DEBRIS_CAP],
        rng: FxRng::new(0xB41C_D5),
        frames: [(None, Transform::IDENTITY); 16],
        next_frame: 0,
        bursts: 0,
    });
}

/// Keeps a removed piece's frame for its burst.
fn remember_frame(
    remove: On<Remove, super::Piece>,
    pieces: Query<&super::Piece>,
    pool: Option<ResMut<BuildingDebris>>,
) {
    let (Ok(piece), Some(mut pool)) = (pieces.get(remove.entity), pool) else {
        return;
    };
    let i = pool.next_frame % pool.frames.len();
    pool.frames[i] = (Some(remove.entity), piece.slot().transform());
    pool.next_frame = i + 1;
}

/// Where a broken piece's chunks start (in its frame) and fly.
fn launch(
    rng: &mut FxRng,
    kind: PieceKind,
    frame: &Transform,
    centre: Vec3,
    away: Vec3,
    i: usize,
    n: usize,
) -> Chunk {
    // Spread over the face on a jittered grid.
    let cols = 4;
    let rows = n.div_ceil(cols);
    let u = ((i % cols) as f32 + 0.5) / cols as f32 * 2.0 - 1.0 + rng.range(-0.2, 0.2);
    let v = ((i / cols) as f32 + 0.5) / rows as f32 * 2.0 - 1.0 + rng.range(-0.2, 0.2);
    let local = match kind {
        PieceKind::Wall => Vec3::new(u * 1.7, v * 1.25, rng.range(-0.05, 0.05)),
        PieceKind::Floor => Vec3::new(u * 1.7, 0.0, v * 1.7),
        PieceKind::Ramp => {
            let z = v * 1.7;
            Vec3::new(u * 1.7, super::ramp_surface_height(z) - 0.1, z)
        }
        PieceKind::Cone => Vec3::new(u * 1.7, 0.4, v * 1.7),
    };
    let pos = frame.transform_point(local);
    let vel = away * rng.range(1.2, 3.2)
        + (pos - centre).normalize_or_zero() * rng.range(0.8, 2.2)
        + Vec3::Y * rng.range(2.0, 4.8);
    Chunk {
        pos,
        vel,
        rot: frame.rotation * Quat::from_scaled_axis(rng.dir() * rng.range(0.0, 0.5)),
        spin: rng.dir() * rng.range(3.0, 10.0),
        life: DEBRIS_SECONDS * rng.range(0.78, 0.95),
        shrink_start: 0.72,
        radius: if kind == PieceKind::Wall { 0.12 } else { 0.05 },
        restitution: 0.32,
        gravity: 20.0,
        drag: 0.2,
        ..default()
    }
}

/// Bursts every destroyed piece into chunks.
fn burst_pieces(
    pool: Option<ResMut<BuildingDebris>>,
    mut changes: MessageReader<PieceChanged>,
    player: Option<Single<&Transform, With<Player>>>,
) {
    let Some(mut pool) = pool else {
        changes.clear();
        return;
    };
    let eye = player.map_or(Vec3::ZERO, |p| p.translation + Vec3::Y * 1.6);
    let pool = &mut *pool;
    for change in changes.read() {
        if change.change != PieceChange::Destroyed {
            continue;
        }
        pool.bursts += 1;
        // The piece's own frame (or, if it wasn't seen go, one about its
        // centre facing across the line of sight).
        let away = (change.center - eye).with_y(0.0).normalize_or(Vec3::Z);
        let frame = pool
            .frames
            .iter()
            .find(|(e, _)| *e == Some(change.entity))
            .map(|(_, f)| *f)
            .unwrap_or_else(|| {
                Transform::from_translation(change.center).looking_to(away, Vec3::Y)
            });
        let n = match change.kind {
            PieceKind::Wall => WALL_CHUNKS,
            _ => PLANK_CHUNKS,
        };
        for i in 0..n {
            let chunk = launch(
                &mut pool.rng,
                change.kind,
                &frame,
                change.center,
                away,
                i,
                n,
            );
            let size = pool.rng.range(0.85, 1.3);
            let Some(slot) = pool.slots.alloc(DEBRIS_CAP) else {
                continue;
            };
            pool.live[slot] = Some(Flying {
                chunk,
                brick: change.kind == PieceKind::Wall,
                size,
                floor: None,
                fresh: true,
            });
        }
    }
}

/// The ground under `p`: a piece's top below it, else the island top where
/// there is island (none over the void).
fn ground_under(spatial: &SpatialQuery, rim: Option<&IslandRim>, p: Vec3) -> Option<f32> {
    let pieces = SpatialQueryFilter::from_mask([Layer::Piece]);
    let origin = p + Vec3::Y * PROBE_UP;
    let piece = spatial
        .cast_ray(origin, Dir3::NEG_Y, PROBE_DEPTH, false, &pieces)
        .map(|hit| origin.y - hit.distance);
    let island = rim.is_none_or(|r| r.on_island(p)).then_some(ISLAND_TOP);
    match (piece, island) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

#[allow(clippy::type_complexity)]
fn simulate_debris(
    time: FreezableTime,
    pool: Option<ResMut<BuildingDebris>>,
    models: Option<Res<PieceDebris>>,
    rim: Option<Res<IslandRim>>,
    // Apps without physics (some visual tests) have no spatial queries.
    spatial: If<SpatialQuery>,
    mut commands: Commands,
    mut chunks: Query<
        (
            &mut Transform,
            &mut Visibility,
            Option<&mut Mesh3d>,
            Has<MeshMaterial3d<StandardMaterial>>,
        ),
        With<DebrisChunk>,
    >,
) {
    let Some(mut pool) = pool else { return };
    let dt = time.delta_secs();
    let pool = &mut *pool;
    for i in 0..pool.live.len() {
        let Some(piece) = pool.live[i].as_mut() else {
            continue;
        };
        let entity = pool.entities[i];
        let Ok((mut tf, mut vis, mesh, dressed)) = chunks.get_mut(entity) else {
            continue;
        };
        if piece.fresh {
            piece.fresh = false;
            if let Some(m) = models.as_deref() {
                let want = if piece.brick { &m.brick } else { &m.splinter };
                match mesh {
                    Some(mut mesh) => {
                        if mesh.0 != *want {
                            mesh.0 = want.clone();
                        }
                    }
                    None => {
                        commands.entity(entity).insert(Mesh3d(want.clone()));
                    }
                }
                if !dressed {
                    commands
                        .entity(entity)
                        .insert(MeshMaterial3d(m.material.clone()));
                }
            }
            vis.set_if_neq(Visibility::Visible);
        } else {
            if piece.floor.is_none() || !piece.chunk.is_resting() {
                piece.floor = ground_under(&spatial, rim.as_deref(), piece.chunk.pos);
            }
            let floor = piece.floor;
            if !piece.chunk.step(dt, |_| floor).alive {
                vis.set_if_neq(Visibility::Hidden);
                pool.live[i] = None;
                pool.slots.free(i);
                continue;
            }
        }
        let c = &piece.chunk;
        tf.translation = c.pos;
        tf.rotation = c.rot;
        tf.scale = Vec3::splat((piece.size * c.shrink()).max(1e-4));
    }
}
