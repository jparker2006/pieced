//! The logical build grid: slot keys, piece placements and the piece map.
//! Pure data and geometry; no systems. Also the future bot navigation graph.

use super::BuildTuning;
use crate::shared::{
    ARENA_CELLS, CELL_SIZE, Facing, GridCell, LEVEL_HEIGHT, MAX_LEVELS, PieceKind,
};
use avian3d::prelude::Collider;
use bevy::prelude::*;
use std::collections::BTreeMap;

/// Which way a grid edge runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EdgeAxis {
    /// Runs along X: the north or south side of a cell. `line` indexes Z grid lines.
    AlongX,
    /// Runs along Z: the west or east side of a cell. `line` indexes X grid lines.
    AlongZ,
}

/// One cell edge at one level, stored once for both cells that share it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EdgeKey {
    pub axis: EdgeAxis,
    /// Grid line index, `0..=ARENA_CELLS`.
    pub line: i32,
    /// Cell index along the edge, `0..ARENA_CELLS`.
    pub span: i32,
    pub level: i32,
}

impl EdgeKey {
    /// The edge on `facing` side of `cell`.
    pub fn of(cell: GridCell, facing: Facing) -> Self {
        let (axis, line, span) = match facing {
            Facing::North => (EdgeAxis::AlongX, cell.z, cell.x),
            Facing::South => (EdgeAxis::AlongX, cell.z + 1, cell.x),
            Facing::West => (EdgeAxis::AlongZ, cell.x, cell.z),
            Facing::East => (EdgeAxis::AlongZ, cell.x + 1, cell.z),
        };
        Self {
            axis,
            line,
            span,
            level: cell.level,
        }
    }

    /// Inside the arena horizontally (the outer boundary edges count).
    pub fn in_arena(&self) -> bool {
        (0..=ARENA_CELLS).contains(&self.line) && (0..ARENA_CELLS).contains(&self.span)
    }

    /// The two (cell, side) pairs that name this edge. Either cell may lie outside
    /// the arena on the boundary.
    pub fn sides(&self) -> [(GridCell, Facing); 2] {
        match self.axis {
            EdgeAxis::AlongX => [
                (
                    GridCell::new(self.span, self.line, self.level),
                    Facing::North,
                ),
                (
                    GridCell::new(self.span, self.line - 1, self.level),
                    Facing::South,
                ),
            ],
            EdgeAxis::AlongZ => [
                (
                    GridCell::new(self.line, self.span, self.level),
                    Facing::West,
                ),
                (
                    GridCell::new(self.line - 1, self.span, self.level),
                    Facing::East,
                ),
            ],
        }
    }
}

/// A piece slot in the map. Each cell has one floor slot and one ramp slot (any
/// facing); each cell edge has one wall slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SlotKey {
    Floor(GridCell),
    Ramp(GridCell),
    Wall(EdgeKey),
}

/// A concrete placement: which piece, in which cell, facing which way.
///
/// - Wall: on the `facing` edge of `cell`.
/// - Floor: covers `cell` at its base height (`facing` only orients the planks).
/// - Ramp: fills `cell`, rising `LEVEL_HEIGHT` toward `facing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PieceSlot {
    pub kind: PieceKind,
    pub cell: GridCell,
    pub facing: Facing,
}

impl PieceSlot {
    pub const fn new(kind: PieceKind, cell: GridCell, facing: Facing) -> Self {
        Self { kind, cell, facing }
    }

    pub const fn wall(cell: GridCell, facing: Facing) -> Self {
        Self::new(PieceKind::Wall, cell, facing)
    }

    pub const fn floor(cell: GridCell) -> Self {
        Self::new(PieceKind::Floor, cell, Facing::North)
    }

    pub const fn ramp(cell: GridCell, facing: Facing) -> Self {
        Self::new(PieceKind::Ramp, cell, facing)
    }

    pub fn key(&self) -> SlotKey {
        match self.kind {
            PieceKind::Wall => SlotKey::Wall(EdgeKey::of(self.cell, self.facing)),
            PieceKind::Floor => SlotKey::Floor(self.cell),
            PieceKind::Ramp => SlotKey::Ramp(self.cell),
        }
    }

    /// Inside the arena horizontally (levels are checked separately).
    pub fn in_arena(&self) -> bool {
        match self.kind {
            PieceKind::Wall => EdgeKey::of(self.cell, self.facing).in_arena(),
            _ => (0..ARENA_CELLS).contains(&self.cell.x) && (0..ARENA_CELLS).contains(&self.cell.z),
        }
    }

    pub fn level_ok(&self) -> bool {
        (0..MAX_LEVELS).contains(&self.cell.level)
    }

    /// Rotation that points local -Z along `facing`.
    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_y(self.facing.yaw())
    }

    /// The piece entity's transform. Its collider and meshes are authored around it:
    /// walls and floors are centered on it, ramps rise from it (base center).
    pub fn transform(&self) -> Transform {
        let base = self.cell.base_center();
        let translation = match self.kind {
            PieceKind::Wall => {
                base + self.facing.vector() * (CELL_SIZE / 2.0) + Vec3::Y * (LEVEL_HEIGHT / 2.0)
            }
            PieceKind::Floor | PieceKind::Ramp => base,
        };
        Transform::from_translation(translation).with_rotation(self.rotation())
    }

    /// World-space center of the piece's volume (for effects and messages).
    pub fn center(&self) -> Vec3 {
        let t = self.transform().translation;
        match self.kind {
            PieceKind::Ramp => t + Vec3::Y * (LEVEL_HEIGHT / 3.0),
            _ => t,
        }
    }

    /// Collider in the piece's local space (see [`PieceSlot::transform`]).
    pub fn collider(&self, tuning: &BuildTuning) -> Collider {
        match self.kind {
            PieceKind::Wall => Collider::cuboid(CELL_SIZE, LEVEL_HEIGHT, tuning.wall_thickness),
            PieceKind::Floor => Collider::cuboid(CELL_SIZE, tuning.floor_thickness, CELL_SIZE),
            PieceKind::Ramp => Collider::convex_hull(ramp_hull_points())
                .unwrap_or_else(|| Collider::cuboid(CELL_SIZE, LEVEL_HEIGHT / 2.0, CELL_SIZE)),
        }
    }

    /// World-space AABB (min, max) of the piece's collision volume.
    pub fn aabb(&self, tuning: &BuildTuning) -> (Vec3, Vec3) {
        let t = self.transform();
        let half = match self.kind {
            PieceKind::Wall => {
                let along = self.facing.vector().cross(Vec3::Y).abs();
                along * (CELL_SIZE / 2.0)
                    + Vec3::Y * (LEVEL_HEIGHT / 2.0)
                    + self.facing.vector().abs() * (tuning.wall_thickness / 2.0)
            }
            PieceKind::Floor => Vec3::new(CELL_SIZE, tuning.floor_thickness, CELL_SIZE) / 2.0,
            PieceKind::Ramp => {
                let c = t.translation + Vec3::Y * (LEVEL_HEIGHT / 2.0);
                let h = Vec3::new(CELL_SIZE, LEVEL_HEIGHT, CELL_SIZE) / 2.0;
                return (c - h, c + h);
            }
        };
        (t.translation - half, t.translation + half)
    }
}

/// The ramp wedge in local space: base center at the origin, rising toward -Z.
pub fn ramp_hull_points() -> Vec<Vec3> {
    let h = CELL_SIZE / 2.0;
    vec![
        Vec3::new(-h, 0.0, h),
        Vec3::new(h, 0.0, h),
        Vec3::new(-h, 0.0, -h),
        Vec3::new(h, 0.0, -h),
        Vec3::new(-h, LEVEL_HEIGHT, -h),
        Vec3::new(h, LEVEL_HEIGHT, -h),
    ]
}

/// Height of a ramp's walking surface above its base at a local Z (−2 = top).
pub fn ramp_surface_height(local_z: f32) -> f32 {
    ((CELL_SIZE / 2.0 - local_z) / CELL_SIZE * LEVEL_HEIGHT).clamp(0.0, LEVEL_HEIGHT)
}

/// What occupies a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapEntry {
    pub entity: Entity,
    pub slot: PieceSlot,
}

/// The source of truth for every placed piece. Colliders and meshes are derived
/// from it. Iteration order is deterministic (sorted by slot).
#[derive(Resource, Debug, Default, Clone)]
pub struct PieceMap {
    slots: BTreeMap<SlotKey, MapEntry>,
    /// Slot → first tick at which it may be rebuilt.
    locks: BTreeMap<SlotKey, u64>,
}

impl PieceMap {
    pub fn get(&self, key: SlotKey) -> Option<MapEntry> {
        self.slots.get(&key).copied()
    }

    /// The piece occupying the slot `slot` would use.
    pub fn occupant(&self, slot: &PieceSlot) -> Option<Entity> {
        self.get(slot.key()).map(|e| e.entity)
    }

    pub fn floor_at(&self, cell: GridCell) -> Option<Entity> {
        self.get(SlotKey::Floor(cell)).map(|e| e.entity)
    }

    /// The ramp in `cell`, with the direction it rises toward.
    pub fn ramp_at(&self, cell: GridCell) -> Option<(Entity, Facing)> {
        self.get(SlotKey::Ramp(cell))
            .map(|e| (e.entity, e.slot.facing))
    }

    pub fn wall_at(&self, cell: GridCell, facing: Facing) -> Option<Entity> {
        self.get(SlotKey::Wall(EdgeKey::of(cell, facing)))
            .map(|e| e.entity)
    }

    /// True when a wall blocks walking from `cell` toward `facing` at its level.
    pub fn is_edge_blocked(&self, cell: GridCell, facing: Facing) -> bool {
        self.wall_at(cell, facing).is_some()
    }

    /// True while a destroyed piece's slot is still locked at `tick`.
    pub fn is_locked(&self, key: SlotKey, tick: u64) -> bool {
        self.locks.get(&key).is_some_and(|&until| tick < until)
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Every placed piece, sorted by slot.
    pub fn iter(&self) -> impl Iterator<Item = &MapEntry> {
        self.slots.values()
    }

    pub(crate) fn insert(&mut self, slot: PieceSlot, entity: Entity) {
        self.slots.insert(slot.key(), MapEntry { entity, slot });
    }

    /// Frees the slot if `entity` still occupies it.
    pub(crate) fn remove(&mut self, key: SlotKey, entity: Entity) -> bool {
        if self.slots.get(&key).is_some_and(|e| e.entity == entity) {
            self.slots.remove(&key);
            true
        } else {
            false
        }
    }

    pub(crate) fn lock(&mut self, key: SlotKey, until_tick: u64) {
        self.locks.insert(key, until_tick);
    }

    pub(crate) fn prune_locks(&mut self, tick: u64) {
        self.locks.retain(|_, until| tick < *until);
    }

    pub(crate) fn clear(&mut self) {
        self.slots.clear();
        self.locks.clear();
    }
}
