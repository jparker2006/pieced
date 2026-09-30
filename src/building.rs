//! Slice B — the build grid: piece map, targeting, placement, damage and destruction.
//!
//! Gameplay here is headless-safe and runs in the fixed step:
//! - [`SimSet::Building`]: every character's [`BuildTarget`] is refreshed from its
//!   view, and pieces are placed on a fire press, on turbo (fire held) and, with
//!   `builder_pro`, on the piece key press.
//! - [`SimSet::Building`], first: edit mode (D44). `edit_pressed` on a piece in
//!   reach opens its edit grid; a click or a drag selects tiles and releasing
//!   confirms a valid shape at once (collider, piece map and, through
//!   [`PieceEdit`], the mesh follow); `reset_pressed` restores an edited piece,
//!   in edit mode or out of it. See [`edit`] for the grids and shapes.
//! - [`SimSet::Resolve`]: [`PieceHit`] requests are applied, cracks and
//!   destruction are reported, and each character's [`AimedPiece`] and
//!   [`EditTarget`] are refreshed.
//!
//! Presentation lives in [`visuals`] (client only).

pub mod debris;
pub mod edit;
pub mod edit_grid;
mod grid;
pub mod juice;
mod mesh;
mod targeting;
pub mod visuals;

pub use edit::{EditShape, PieceEdit};
pub use grid::{
    CONE_HEIGHT, EdgeAxis, EdgeKey, MapEntry, PieceMap, PieceSlot, SlotKey, ramp_surface_height,
};
pub use targeting::{
    BuildCandidate, FORWARD_BIAS, Gait, LEVEL_SNAP, Placement, RUSH_TOLERANCE_DEG, STEEP_LOOK_DEG,
    build_target, capsule_overlaps_box, check_placement, feet_level, target_slot,
};
pub use visuals::BuildingVisualsPlugin;

use crate::{
    movement::Motor,
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
    pub cone_hp: f32,
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
    /// capsule and the hitboxes. The builder is exempt: a wall built against
    /// yourself pushes you back into your cell.
    pub trap_radius: f32,
    /// Height of the body a new wall must not overlap (m), from the feet.
    pub trap_height: f32,
    /// How far the crosshair looks for a piece to report in [`AimedPiece`] (m).
    pub aim_range: f32,
    /// Floors, ramps and cones can go in the tiles this far around yours,
    /// diagonals included (1 is Fortnite's 3×3; 2 reaches further), wherever
    /// the aim lands.
    pub reach_tiles: i32,
    /// How far along the crosshair a piece can be edited or reset from (m):
    /// the far edge of the next tile, from anywhere in your own.
    pub edit_reach: f32,
}

impl Default for BuildTuning {
    fn default() -> Self {
        Self {
            wall_hp: 200.0,
            floor_hp: 170.0,
            ramp_hp: 170.0,
            cone_hp: 170.0,
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
            reach_tiles: 1,
            edit_reach: 7.5,
        }
    }
}

impl BuildTuning {
    pub fn max_hp(&self, kind: PieceKind) -> f32 {
        match kind {
            PieceKind::Wall => self.wall_hp,
            PieceKind::Floor => self.floor_hp,
            PieceKind::Ramp => self.ramp_hp,
            PieceKind::Cone => self.cone_hp,
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

/// A character's edit mode (D44). Gameplay drives it from [`PlayerIntent`];
/// the edit grid overlay and the HUD read it.
#[derive(Component, Debug, Default, Clone, PartialEq)]
pub struct EditMode {
    /// The open edit grid, while editing.
    pub session: Option<EditSession>,
    /// The primary action stays blocked until it is released after edit mode
    /// ended mid-click, so that click never fires a gun or places a piece.
    pub hold_fire: bool,
}

impl EditMode {
    pub fn is_editing(&self) -> bool {
        self.session.is_some()
    }

    /// Whether the primary action belongs to editing (no firing or placing).
    pub fn blocks_fire(&self) -> bool {
        self.session.is_some() || self.hold_fire
    }
}

/// An open edit grid on one piece.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditSession {
    pub piece: Entity,
    pub slot: PieceSlot,
    /// The piece's edit when the grid opened.
    pub base: PieceEdit,
    /// The tile under the crosshair.
    pub hovered: Option<u8>,
    /// The click or drag in progress, while the primary action is held.
    pub drag: Option<EditDrag>,
}

/// Tiles crossed by one click-and-drag in edit mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditDrag {
    /// Walls, floors and cones: the state painted onto every tile crossed
    /// (the opposite of the first tile's).
    pub paint: bool,
    /// The tiles crossed.
    pub tiles: u16,
    /// Ramps: the stair path, in order (`len` tiles).
    pub path: [u8; 4],
    pub len: u8,
}

impl EditDrag {
    fn start(session: &EditSession, tile: u8) -> Self {
        Self {
            paint: !session.base.has(tile),
            tiles: 1 << tile,
            path: [tile, 0, 0, 0],
            len: 1,
        }
    }

    /// Adds a tile the crosshair crossed (a ramp path only grows to a tile
    /// beside its last one).
    fn cross(&mut self, kind: PieceKind, tile: u8) {
        if self.tiles & (1 << tile) != 0 {
            return;
        }
        if kind == PieceKind::Ramp {
            let last = self.path[self.len as usize - 1];
            let beside = (last % 2 == tile % 2) != (last / 2 == tile / 2);
            if !beside || self.len as usize >= self.path.len() {
                return;
            }
            self.path[self.len as usize] = tile;
            self.len += 1;
        }
        self.tiles |= 1 << tile;
    }
}

impl EditSession {
    /// The edit the grid shows now: the piece's edit with the drag painted on
    /// (walls, floors, cones), or the path being dragged (ramps).
    pub fn selection(&self) -> PieceEdit {
        let Some(drag) = self.drag else {
            return self.base;
        };
        if self.slot.kind == PieceKind::Ramp {
            return PieceEdit {
                tiles: drag.tiles,
                start: drag.path[0],
            };
        }
        let tiles = if drag.paint {
            self.base.tiles | drag.tiles
        } else {
            self.base.tiles & !drag.tiles
        };
        PieceEdit { tiles, start: 0 }
    }

    /// Whether releasing now would apply the selection.
    pub fn selection_valid(&self) -> bool {
        edit::is_valid(self.slot.kind, self.selection())
    }
}

/// The piece a character would edit or reset (its full shape under the
/// crosshair, within `edit_reach`), refreshed every fixed tick. The input
/// adapter reads it to send R as a reset instead of a reload.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq)]
pub struct EditTarget(pub Option<EditTargetInfo>);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditTargetInfo {
    pub entity: Entity,
    pub kind: PieceKind,
    /// The piece is edited (R resets it).
    pub edited: bool,
    pub distance: f32,
}

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
                (update_edit_modes, update_targets_and_place)
                    .chain()
                    .in_set(SimSet::Building),
            )
            .add_systems(
                FixedUpdate,
                (apply_piece_hits, update_aimed_pieces, update_edit_targets)
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
        EditMode::default(),
        EditTarget::default(),
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
                PieceKind::Cone => "Cone",
            }),
            Piece {
                kind: slot.kind,
                cell: slot.cell,
                facing: slot.facing,
                hp,
                max_hp: hp,
                crack_stage: 0,
            },
            PieceEdit::FULL,
            slot.transform(),
            RigidBody::Static,
            slot.collider(tuning),
            CollisionLayers::new(Layer::Piece, LayerMask::ALL),
        ))
        .id();
    map.insert(slot, entity);
    entity
}

/// Forward move input above which a builder counts as advancing (ramp rushing).
/// Movement runs earlier in the tick, so its [`Motor`] is this tick's.
const ADVANCING_AXIS: f32 = 0.1;

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
            Option<&Motor>,
            &mut BuildTarget,
            &mut BuildTimer,
            Option<&EditMode>,
        ),
        With<Character>,
    >,
    characters: Query<(Entity, &Transform), With<Character>>,
    mut changed: MessageWriter<PieceChanged>,
    mut cues: MessageWriter<GameCue>,
) {
    let tick = tick.0;
    let tuning = &tuning.building;
    map.prune_locks(tick);
    let everyone: Vec<(Entity, Vec3)> =
        characters.iter().map(|(e, t)| (e, t.translation)).collect();
    for (entity, transform, eye, look, tool, intent, motor, mut target, mut timer, edit) in
        &mut builders
    {
        let editing = edit.is_some_and(EditMode::is_editing);
        let ActiveTool::Build(kind) = *tool else {
            if target.candidate.is_some() {
                target.candidate = None;
            }
            continue;
        };
        if editing {
            // The edit grid replaces the ghost.
            if target.candidate.is_some() {
                target.candidate = None;
            }
            continue;
        }
        let (eye_pos, dir) = eye_ray(transform, eye, look);
        // A wall may be built against its builder (movement pushes them back into
        // their cell), never against anyone else.
        let others: Vec<Vec3> = everyone
            .iter()
            .filter(|(e, _)| *e != entity)
            .map(|(_, feet)| *feet)
            .collect();
        // Holding forward: ramps follow the rush (see `targeting`).
        let gait = if intent.move_axis.y <= ADVANCING_AXIS {
            Gait::Standing
        } else if motor.is_some_and(|m| !m.grounded && m.velocity.y < 0.0) {
            Gait::Falling
        } else {
            Gait::Advancing
        };
        let candidate = build_target(
            eye_pos,
            *dir,
            transform.translation,
            kind,
            gait,
            &map,
            tick,
            &others,
            tuning,
        );
        target.candidate = Some(candidate);
        if edit.is_some_and(EditMode::blocks_fire) {
            // The click that ended an edit places nothing until it's released.
            continue;
        }

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
// Edit mode
// ---------------------------------------------------------------------------

/// The piece a view ray would edit: the nearest piece whose full, unedited
/// shape the ray enters within `edit_reach`, unless world geometry (the
/// ground, a rock) is nearer (`world_hit`, a distance along the same ray).
pub fn edit_target(
    eye: Vec3,
    dir: Vec3,
    map: &PieceMap,
    world_hit: Option<f32>,
    tuning: &BuildTuning,
) -> Option<(MapEntry, f32)> {
    let reach = tuning.edit_reach;
    let near = reach + crate::shared::CELL_SIZE;
    let best = map
        .iter()
        .filter(|e| e.slot.center().distance(eye) <= near)
        .filter_map(|e| {
            let d = edit::full_shape_hit(&e.slot, eye, dir, tuning)?;
            (d <= reach).then_some((*e, d))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    match world_hit {
        Some(w) if w < best.1 => None,
        _ => Some(best),
    }
}

fn world_hit(spatial: &SpatialQuery, eye: Vec3, dir: Dir3, reach: f32) -> Option<f32> {
    let filter = SpatialQueryFilter::from_mask(Layer::World);
    spatial
        .cast_ray(eye, dir, reach, true, &filter)
        .map(|h| h.distance)
}

/// Gives a piece its edit: the collider and the piece map follow at once (the
/// visuals follow [`PieceEdit`]). HP is untouched, so its fraction is kept.
fn apply_edit(
    entity: Entity,
    edit: PieceEdit,
    pieces: &mut Query<(&Piece, &mut PieceEdit)>,
    commands: &mut Commands,
    map: &mut PieceMap,
    tuning: &BuildTuning,
) -> bool {
    let Ok((piece, mut current)) = pieces.get_mut(entity) else {
        return false;
    };
    if *current == edit || !edit::is_valid(piece.kind, edit) {
        return false;
    }
    *current = edit;
    // Inserted before this tick's physics step, so the next one moves and
    // shoots against the edited shape.
    commands
        .entity(entity)
        .insert(edit::edited_collider(&piece.slot(), edit, tuning));
    map.set_edit(piece.slot().key(), entity, edit);
    true
}

fn leave_edit_mode(mode: &mut EditMode, intent: &PlayerIntent) {
    mode.session = None;
    mode.hold_fire = intent.fire;
}

fn update_edit_modes(
    tuning: Res<Tuning>,
    spatial: SpatialQuery,
    mut map: ResMut<PieceMap>,
    mut characters: Query<
        (
            Entity,
            &Transform,
            &EyeHeight,
            &LookAngles,
            &PlayerIntent,
            &mut EditMode,
        ),
        With<Character>,
    >,
    mut pieces: Query<(&Piece, &mut PieceEdit)>,
    mut commands: Commands,
    mut cues: MessageWriter<GameCue>,
) {
    let tuning = &tuning.building;
    for (who, transform, eye, look, intent, mut mode) in &mut characters {
        let (eye_pos, dir) = eye_ray(transform, eye, look);
        if mode.hold_fire && !intent.fire && !intent.fire_pressed {
            mode.hold_fire = false;
        }
        // The piece broke, you walked away from it, or you picked a tool.
        if let Some(session) = mode.session {
            let gone = pieces.get(session.piece).is_err();
            let far = session.slot.center().distance(eye_pos) > tuning.edit_reach + 3.0;
            if gone || far || intent.select.is_some() {
                leave_edit_mode(&mut mode, intent);
            }
        }
        let target = || {
            let hit = world_hit(&spatial, eye_pos, dir, tuning.edit_reach);
            edit_target(eye_pos, *dir, &map, hit, tuning)
        };

        if intent.reset_pressed {
            // In edit mode: the piece being edited. Otherwise the edited piece
            // under the crosshair (Fortnite's reset without entering edit mode).
            let piece = match mode.session {
                Some(session) => Some(session.piece),
                None => target()
                    .filter(|(e, _)| e.edit.is_edited())
                    .map(|(e, _)| e.entity),
            };
            if let Some(piece) = piece
                && apply_edit(
                    piece,
                    PieceEdit::FULL,
                    &mut pieces,
                    &mut commands,
                    &mut map,
                    tuning,
                )
            {
                cues.write(GameCue::PieceEdited { who, piece });
            }
            if mode.is_editing() {
                leave_edit_mode(&mut mode, intent);
            }
            continue;
        }
        if intent.edit_pressed {
            if mode.is_editing() {
                leave_edit_mode(&mut mode, intent);
                continue;
            }
            if let Some((entry, _)) = target() {
                mode.session = Some(EditSession {
                    piece: entry.entity,
                    slot: entry.slot,
                    base: pieces.get(entry.entity).map_or(entry.edit, |(_, e)| *e),
                    hovered: None,
                    drag: None,
                });
                mode.hold_fire = false;
            }
        }

        let Some(session) = mode.session.as_mut() else {
            continue;
        };
        session.hovered = edit::hovered_tile(&session.slot, eye_pos, *dir, tuning);
        if intent.fire_pressed
            && session.drag.is_none()
            && let Some(tile) = session.hovered
        {
            session.drag = Some(EditDrag::start(session, tile));
        }
        let kind = session.slot.kind;
        if let (Some(drag), Some(tile)) = (session.drag.as_mut(), session.hovered) {
            drag.cross(kind, tile);
        }
        if session.drag.is_none() || intent.fire {
            continue;
        }
        // Released: confirm (D44: release confirms, no delay).
        let selection = session.selection();
        let piece = session.piece;
        if selection == session.base {
            mode.session = None;
        } else if edit::is_valid(kind, selection) {
            if apply_edit(
                piece,
                selection,
                &mut pieces,
                &mut commands,
                &mut map,
                tuning,
            ) {
                cues.write(GameCue::PieceEdited { who, piece });
            }
            mode.session = None;
        } else {
            // Not a Fortnite shape: nothing changes and the grid stays open.
            session.drag = None;
            cues.write(GameCue::EditRejected { who });
        }
    }
}

fn update_edit_targets(
    spatial: SpatialQuery,
    tuning: Res<Tuning>,
    map: Res<PieceMap>,
    // Knights never edit, and only the player's target is read (input, HUD):
    // skipping the pool saves its rays and piece scans every tick.
    mut characters: Query<
        (&Transform, &EyeHeight, &LookAngles, &mut EditTarget),
        Without<crate::grunt::Grunt>,
    >,
    pieces: Query<&Piece>,
) {
    let tuning = &tuning.building;
    for (transform, eye, look, mut target) in &mut characters {
        let (origin, dir) = eye_ray(transform, eye, look);
        let hit = world_hit(&spatial, origin, dir, tuning.edit_reach);
        let info = edit_target(origin, *dir, &map, hit, tuning)
            .filter(|(e, _)| pieces.contains(e.entity))
            .map(|(e, distance)| EditTargetInfo {
                entity: e.entity,
                kind: e.slot.kind,
                edited: e.edit.is_edited(),
                distance,
            });
        if target.0 != info {
            target.0 = info;
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
    // As above: only the player's aimed piece is shown.
    mut characters: Query<
        (&Transform, &EyeHeight, &LookAngles, &mut AimedPiece),
        Without<crate::grunt::Grunt>,
    >,
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

/// Gives `piece` an edit immediately, as a confirmed edit does (collider and
/// piece map at once, HP untouched). Returns false (and changes nothing) for a
/// missing piece or an edit that isn't a valid shape for its kind.
pub fn edit_piece(world: &mut World, piece: Entity, edit: PieceEdit) -> bool {
    let tuning = world.resource::<Tuning>().building.clone();
    let Some(p) = world.get::<Piece>(piece).copied() else {
        return false;
    };
    if !edit::is_valid(p.kind, edit) {
        return false;
    }
    world
        .entity_mut(piece)
        .insert((edit, edit::edited_collider(&p.slot(), edit, &tuning)));
    world
        .resource_mut::<PieceMap>()
        .set_edit(p.slot().key(), piece, edit);
    true
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
