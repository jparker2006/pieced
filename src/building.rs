//! Slice B — the build grid: piece map, targeting, placement, damage and destruction.
//!
//! Gameplay here is headless-safe and runs in the fixed step:
//! - [`SimSet::Building`]: every character's [`BuildTarget`] is refreshed from its
//!   view, and pieces are placed on a fire press, on turbo (fire held) and, with
//!   `builder_pro`, on the piece key press.
//! - [`SimSet::Resolve`]: [`PieceHit`] requests are applied, cracks and
//!   destruction are reported, and each character's [`AimedPiece`] is refreshed.
//!
//! Presentation lives in [`visuals`] (client only).

mod grid;
mod mesh;
mod targeting;
pub mod visuals;

pub use grid::{EdgeAxis, EdgeKey, MapEntry, PieceMap, PieceSlot, SlotKey, ramp_surface_height};
pub use targeting::{
    BuildCandidate, LEVEL_SNAP, Placement, build_target, capsule_overlaps_box, check_placement,
    feet_level, target_slot,
};
pub use visuals::BuildingVisualsPlugin;

use crate::{
    shared::{
        ActiveTool, Character, DamageDealt, DamageTarget, EyeHeight, Facing, GameCue, GridCell,
        Layer, LookAngles, PieceChange, PieceChanged, PieceHit, PieceKind, PlayerIntent, SimSet,
        SimTick, TICK_SECONDS, eye_ray,
    },
    tuning::Tuning,
};
use avian3d::prelude::*;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BuildTuning {
    pub wall_hp: f32,
    pub floor_hp: f32,
    pub ramp_hp: f32,
    /// Crack stage 1 at or below this HP fraction.
    pub crack_stage_1: f32,
    /// Crack stage 2 at or below this HP fraction.
    pub crack_stage_2: f32,
    /// Seconds a destroyed piece's spot stays locked.
    pub rebuild_lock: f32,
    /// Seconds between placements while the primary action is held.
    pub turbo_interval: f32,
    pub wall_thickness: f32,
    pub floor_thickness: f32,
    /// Piece key selects *and* places in one press.
    pub builder_pro: bool,
    /// Radius of the body a new wall must not overlap (m). Covers the movement
    /// capsule and the hitboxes.
    pub trap_radius: f32,
    /// Height of the body a new wall must not overlap (m), from the feet.
    pub trap_height: f32,
    /// How far the crosshair looks for a piece to report in [`AimedPiece`] (m).
    pub aim_range: f32,
}

impl Default for BuildTuning {
    fn default() -> Self {
        Self {
            wall_hp: 200.0,
            floor_hp: 170.0,
            ramp_hp: 170.0,
            crack_stage_1: 0.66,
            crack_stage_2: 0.33,
            rebuild_lock: 0.15,
            turbo_interval: 0.05,
            wall_thickness: 0.2,
            floor_thickness: 0.2,
            builder_pro: false,
            trap_radius: 0.36,
            trap_height: 1.85,
            aim_range: 30.0,
        }
    }
}

impl BuildTuning {
    pub fn max_hp(&self, kind: PieceKind) -> f32 {
        match kind {
            PieceKind::Wall => self.wall_hp,
            PieceKind::Floor => self.floor_hp,
            PieceKind::Ramp => self.ramp_hp,
        }
    }

    /// Crack stage (0, 1 or 2) for an HP fraction.
    pub fn crack_stage(&self, fraction: f32) -> u8 {
        if fraction <= self.crack_stage_2 + 1e-5 {
            2
        } else if fraction <= self.crack_stage_1 + 1e-5 {
            1
        } else {
            0
        }
    }

    /// Closest a wall's grid line may be to the builder's feet before targeting
    /// moves on to the next line.
    pub fn min_wall_distance(&self) -> f32 {
        self.trap_radius + self.wall_thickness / 2.0 + 0.01
    }

    /// Whole fixed ticks between turbo placements (at least 1).
    pub fn turbo_ticks(&self) -> u64 {
        ((self.turbo_interval / TICK_SECONDS).round() as u64).max(1)
    }

    /// Whole fixed ticks a destroyed slot stays locked.
    pub fn lock_ticks(&self) -> u64 {
        (self.rebuild_lock / TICK_SECONDS).round() as u64
    }
}

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

/// A placed building piece. Lives on the entity that also carries its collider
/// (`RigidBody::Static`, `CollisionLayers::new(Layer::Piece, LayerMask::ALL)`), so a
/// ray hit's entity is the piece.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    pub kind: PieceKind,
    pub cell: GridCell,
    pub facing: Facing,
    pub hp: f32,
    pub max_hp: f32,
    /// 0 = intact, 1 = at or below 66% HP, 2 = at or below 33% HP.
    pub crack_stage: u8,
}

impl Piece {
    pub fn slot(&self) -> PieceSlot {
        PieceSlot::new(self.kind, self.cell, self.facing)
    }

    pub fn hp_fraction(&self) -> f32 {
        if self.max_hp > 0.0 {
            (self.hp / self.max_hp).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// Every character's current build candidate (for the ghost preview). `None`
/// unless the character holds a build tool.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq)]
pub struct BuildTarget {
    pub candidate: Option<BuildCandidate>,
}

/// Turbo-build bookkeeping per character.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BuildTimer {
    /// Tick of this character's latest placement.
    pub last_placed_tick: Option<u64>,
}

/// The piece under a character's crosshair (within `aim_range`, not behind
/// world geometry), refreshed every fixed tick. The HUD's piece HP bar reads it.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq)]
pub struct AimedPiece(pub Option<AimedPieceInfo>);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AimedPieceInfo {
    pub entity: Entity,
    pub kind: PieceKind,
    pub hp: f32,
    pub max_hp: f32,
    pub crack_stage: u8,
    pub distance: f32,
}

/// Marks pieces placed at startup as arena cover.
#[derive(Component, Debug, Default)]
pub struct InitialCover;

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct BuildingPlugin;

impl Plugin for BuildingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PieceMap>()
            .add_observer(attach_builder_components)
            .add_observer(free_slot_on_removal)
            .add_systems(Startup, spawn_initial_cover)
            .add_systems(
                FixedUpdate,
                update_targets_and_place.in_set(SimSet::Building),
            )
            .add_systems(
                FixedUpdate,
                (apply_piece_hits, update_aimed_pieces)
                    .chain()
                    .in_set(SimSet::Resolve),
            );
    }
}

fn attach_builder_components(add: On<Add, Character>, mut commands: Commands) {
    commands.entity(add.entity).insert((
        BuildTarget::default(),
        BuildTimer::default(),
        AimedPiece::default(),
    ));
}

fn free_slot_on_removal(
    remove: On<Remove, Piece>,
    pieces: Query<&Piece>,
    mut map: ResMut<PieceMap>,
) {
    if let Ok(piece) = pieces.get(remove.entity) {
        map.remove(piece.slot().key(), remove.entity);
    }
}

// ---------------------------------------------------------------------------
// Placement
// ---------------------------------------------------------------------------

/// Spawns a piece entity for a slot already checked as valid, and records it.
fn spawn_piece(
    commands: &mut Commands,
    map: &mut PieceMap,
    slot: PieceSlot,
    tuning: &BuildTuning,
) -> Entity {
    let hp = tuning.max_hp(slot.kind);
    let entity = commands
        .spawn((
            Name::new(match slot.kind {
                PieceKind::Wall => "Wall",
                PieceKind::Floor => "Floor",
                PieceKind::Ramp => "Ramp",
            }),
            Piece {
                kind: slot.kind,
                cell: slot.cell,
                facing: slot.facing,
                hp,
                max_hp: hp,
                crack_stage: 0,
            },
            slot.transform(),
            RigidBody::Static,
            slot.collider(tuning),
            CollisionLayers::new(Layer::Piece, LayerMask::ALL),
        ))
        .id();
    map.insert(slot, entity);
    entity
}

fn update_targets_and_place(
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    mut map: ResMut<PieceMap>,
    mut commands: Commands,
    mut builders: Query<
        (
            Entity,
            &Transform,
            &EyeHeight,
            &LookAngles,
            &ActiveTool,
            &PlayerIntent,
            &mut BuildTarget,
            &mut BuildTimer,
        ),
        With<Character>,
    >,
    characters: Query<&Transform, With<Character>>,
    mut changed: MessageWriter<PieceChanged>,
    mut cues: MessageWriter<GameCue>,
) {
    let tick = tick.0;
    let tuning = &tuning.building;
    map.prune_locks(tick);
    let feet: Vec<Vec3> = characters.iter().map(|t| t.translation).collect();
    for (entity, transform, eye, look, tool, intent, mut target, mut timer) in &mut builders {
        let ActiveTool::Build(kind) = *tool else {
            if target.candidate.is_some() {
                target.candidate = None;
            }
            continue;
        };
        let (eye_pos, dir) = eye_ray(transform, eye, look);
        let candidate = build_target(
            eye_pos,
            *dir,
            transform.translation,
            kind,
            &map,
            tick,
            &feet,
            tuning,
        );
        target.candidate = Some(candidate);

        let pressed = intent.fire_pressed || (tuning.builder_pro && intent.select == Some(*tool));
        let turbo_ready = timer
            .last_placed_tick
            .is_none_or(|last| tick.saturating_sub(last) >= tuning.turbo_ticks());
        let turbo = intent.fire && !pressed && turbo_ready;
        if !(pressed || turbo) {
            continue;
        }
        if candidate.is_valid() {
            let piece = spawn_piece(&mut commands, &mut map, candidate.slot, tuning);
            timer.last_placed_tick = Some(tick);
            target.candidate = Some(BuildCandidate {
                slot: candidate.slot,
                placement: Placement::Occupied,
            });
            changed.write(PieceChanged {
                entity: piece,
                kind,
                change: PieceChange::Placed,
                center: candidate.slot.center(),
                tick,
            });
        } else if pressed {
            cues.write(GameCue::PlacementRejected { who: entity });
        }
    }
}

// ---------------------------------------------------------------------------
// Damage
// ---------------------------------------------------------------------------

/// One tick's hits on one piece from one source, summed (a pump's pellets land
/// as one damage event).
struct HitGroup {
    piece: Entity,
    source: Option<Entity>,
    amount: f32,
    point: Vec3,
    normal: Vec3,
}

fn apply_piece_hits(
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    mut hits: MessageReader<PieceHit>,
    mut pieces: Query<&mut Piece>,
    mut map: ResMut<PieceMap>,
    mut commands: Commands,
    mut dealt: MessageWriter<DamageDealt>,
    mut changed: MessageWriter<PieceChanged>,
) {
    let mut groups: Vec<HitGroup> = Vec::new();
    for hit in hits.read() {
        if hit.amount <= 0.0 {
            continue;
        }
        match groups
            .iter_mut()
            .find(|g| g.piece == hit.piece && g.source == hit.source)
        {
            Some(group) => group.amount += hit.amount,
            None => groups.push(HitGroup {
                piece: hit.piece,
                source: hit.source,
                amount: hit.amount,
                point: hit.point,
                normal: hit.normal,
            }),
        }
    }
    let tick = tick.0;
    let tuning = &tuning.building;
    for group in groups {
        let Ok(mut piece) = pieces.get_mut(group.piece) else {
            continue;
        };
        if piece.hp <= 0.0 {
            continue;
        }
        let applied = group.amount.min(piece.hp);
        piece.hp -= applied;
        let destroyed = piece.hp <= 1e-3;
        dealt.write(DamageDealt {
            source: group.source,
            target: group.piece,
            target_kind: DamageTarget::Piece,
            amount: applied,
            to_shield: 0.0,
            headshot: false,
            shield_broke: false,
            killed: destroyed,
            point: group.point,
            normal: group.normal,
            tick,
        });
        let slot = piece.slot();
        if destroyed {
            piece.hp = 0.0;
            changed.write(PieceChanged {
                entity: group.piece,
                kind: piece.kind,
                change: PieceChange::Destroyed,
                center: slot.center(),
                tick,
            });
            map.remove(slot.key(), group.piece);
            map.lock(slot.key(), tick + tuning.lock_ticks());
            commands.entity(group.piece).despawn();
        } else {
            let stage = tuning.crack_stage(piece.hp_fraction());
            if stage > piece.crack_stage {
                piece.crack_stage = stage;
                changed.write(PieceChanged {
                    entity: group.piece,
                    kind: piece.kind,
                    change: PieceChange::Cracked(stage),
                    center: slot.center(),
                    tick,
                });
            }
        }
    }
}

fn update_aimed_pieces(
    spatial: SpatialQuery,
    tuning: Res<Tuning>,
    mut characters: Query<(&Transform, &EyeHeight, &LookAngles, &mut AimedPiece)>,
    pieces: Query<&Piece>,
) {
    let filter = SpatialQueryFilter::from_mask([Layer::World, Layer::Piece]);
    for (transform, eye, look, mut aimed) in &mut characters {
        let (origin, dir) = eye_ray(transform, eye, look);
        let info = spatial
            .cast_ray(origin, dir, tuning.building.aim_range, true, &filter)
            .and_then(|hit| {
                let piece = pieces.get(hit.entity).ok()?;
                Some(AimedPieceInfo {
                    entity: hit.entity,
                    kind: piece.kind,
                    hp: piece.hp,
                    max_hp: piece.max_hp,
                    crack_stage: piece.crack_stage,
                    distance: hit.distance,
                })
            });
        if aimed.0 != info {
            aimed.0 = info;
        }
    }
}

// ---------------------------------------------------------------------------
// Initial cover
// ---------------------------------------------------------------------------

/// The pre-placed arena cover: two short wall runs, a lone ramp, and a raised
/// two-cell platform (walled underneath on its far sides) with a ramp up to it.
/// All of it sits well off the spawn line (x = 2, cell column 6).
pub fn initial_cover() -> Vec<PieceSlot> {
    let c = GridCell::new;
    vec![
        // Wall run west of center, facing the spawn line.
        PieceSlot::wall(c(2, 6, 0), Facing::North),
        PieceSlot::wall(c(3, 6, 0), Facing::North),
        // Wall run east of center.
        PieceSlot::wall(c(9, 5, 0), Facing::West),
        PieceSlot::wall(c(9, 6, 0), Facing::West),
        // A lone ramp in the south-east, rising north.
        PieceSlot::ramp(c(9, 8, 0), Facing::North),
        // North-west platform one level up, with a ramp rising to it from the south.
        PieceSlot::floor(c(2, 2, 1)),
        PieceSlot::floor(c(3, 2, 1)),
        PieceSlot::wall(c(2, 2, 0), Facing::West),
        PieceSlot::wall(c(2, 2, 0), Facing::North),
        PieceSlot::wall(c(3, 2, 0), Facing::North),
        PieceSlot::wall(c(2, 2, 1), Facing::North),
        PieceSlot::ramp(c(3, 3, 0), Facing::North),
    ]
}

fn spawn_initial_cover(mut commands: Commands, mut map: ResMut<PieceMap>, tuning: Res<Tuning>) {
    for slot in initial_cover() {
        if check_placement(&slot, &map, 0, &[], &tuning.building) == Placement::Valid {
            let entity = spawn_piece(&mut commands, &mut map, slot, &tuning.building);
            commands.entity(entity).insert(InitialCover);
        }
    }
}

// ---------------------------------------------------------------------------
// World-level API (tests, scenarios, tuning panel)
// ---------------------------------------------------------------------------

fn character_feet(world: &mut World) -> Vec<Vec3> {
    world
        .query_filtered::<&Transform, With<Character>>()
        .iter(world)
        .map(|t| t.translation)
        .collect()
}

/// Places a piece immediately through the normal placement rules (no
/// [`PieceChanged`] message). Returns the entity, or why it was rejected.
pub fn place_piece(world: &mut World, slot: PieceSlot) -> Result<Entity, Placement> {
    let tick = world.resource::<SimTick>().0;
    let tuning = world.resource::<Tuning>().building.clone();
    let feet = character_feet(world);
    let placement = check_placement(&slot, world.resource::<PieceMap>(), tick, &feet, &tuning);
    if placement != Placement::Valid {
        return Err(placement);
    }
    let entity = world.resource_scope(|world, mut map: Mut<PieceMap>| {
        let mut commands = world.commands();
        spawn_piece(&mut commands, &mut map, slot, &tuning)
    });
    world.flush();
    Ok(entity)
}

/// Requests `amount` structure damage on `piece`, exactly as combat does. It is
/// applied in the next fixed tick's [`SimSet::Resolve`].
pub fn damage_piece(world: &mut World, piece: Entity, amount: f32) {
    let point = world
        .get::<Piece>(piece)
        .map(|p| p.slot().center())
        .unwrap_or_default();
    world.write_message(PieceHit {
        piece,
        amount,
        source: None,
        point,
        normal: Vec3::Y,
    });
}

/// Removes every piece and rebuild lock (for tests and scenarios that want an
/// empty grid).
pub fn clear_pieces(world: &mut World) {
    world.resource_mut::<PieceMap>().clear();
    let pieces: Vec<Entity> = world
        .query_filtered::<Entity, With<Piece>>()
        .iter(world)
        .collect();
    for piece in pieces {
        world.despawn(piece);
    }
}
