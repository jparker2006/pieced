//! First-person build targeting: where the ghost goes, and whether it can be placed.
//! Pure functions of the view, the feet and the piece map, so they're unit-testable
//! and shared by the player, scenarios and (later) bots.
//!
//! The rules, in player terms:
//! - Everything snaps to the nearest of the four yaws you're looking along (`facing`).
//! - Reach is your own cell plus one cell ahead along `facing`.
//! - **Wall:** on the first grid line in front of you that is at least a body
//!   width away: your cell's front edge, or the next cell's front edge when you're
//!   pressed up to an empty one (up against your own wall, the ghost stays on it).
//!   It sits on whatever you stand on (the top of a ramp you're climbing), and
//!   looking up past that level's top builds one level higher.
//! - **Floor / ramp:** in the cell ahead, at the level of the ground there (one up
//!   while climbing a ramp). Looking down at your own cell targets it instead;
//!   looking up past the next level's height builds one level higher (above your
//!   own cell when you look steeply up). Ramps rise away from you.

use super::{
    BuildTuning,
    grid::{PieceMap, PieceSlot},
};
use crate::shared::{CELL_SIZE, Facing, GridCell, LEVEL_HEIGHT, PieceKind};
use bevy::prelude::*;

/// Feet up to this far below a level's height count as standing on that level
/// (ramp tops, small bumps, floor slabs).
pub const LEVEL_SNAP: f32 = 0.3;

/// Why a candidate can or can't be placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Placement {
    Valid,
    /// Outside the arena, or below the ground.
    OutOfBounds,
    /// At or above `MAX_LEVELS`.
    AboveHeightLimit,
    /// The slot already holds a piece.
    Occupied,
    /// A piece was destroyed here less than `rebuild_lock` seconds ago.
    RebuildLocked,
    /// A wall here would overlap a character's body.
    BlocksCharacter,
}

/// A targeted placement plus whether it can be placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildCandidate {
    pub slot: PieceSlot,
    pub placement: Placement,
}

impl BuildCandidate {
    pub fn is_valid(&self) -> bool {
        self.placement == Placement::Valid
    }
}

/// Level a character standing with feet at `y` builds from.
pub fn feet_level(y: f32) -> i32 {
    (((y + LEVEL_SNAP) / LEVEL_HEIGHT).floor() as i32).max(0)
}

/// Level of the ground just across the front edge of the feet's cell: one level
/// up while climbing a ramp that rises along `facing`, the ramp's base while
/// descending one, otherwise the feet level.
fn level_across_front(feet: Vec3, own: GridCell, facing: Facing, map: &PieceMap) -> i32 {
    let exact = (feet.y / LEVEL_HEIGHT).floor() as i32;
    if let Some((_, ramp)) = map.ramp_at(GridCell::new(own.x, own.z, exact)) {
        if ramp == facing {
            return exact + 1;
        }
        if ramp == facing.opposite() {
            return exact.max(0);
        }
    }
    feet_level(feet.y)
}

/// Horizontal distance from `p` to the `facing` edge of `cell`, measured along `facing`.
fn distance_to_front_edge(p: Vec3, cell: GridCell, facing: Facing) -> f32 {
    let min = cell.min_corner();
    match facing {
        Facing::North => p.z - min.z,
        Facing::South => min.z + CELL_SIZE - p.z,
        Facing::West => p.x - min.x,
        Facing::East => min.x + CELL_SIZE - p.x,
    }
}

/// Where a piece of `kind` would go for a character with its eye at `eye`
/// looking along `look`, feet at `feet`. Never fails: the result may be invalid
/// (check it with [`check_placement`]).
pub fn target_slot(
    eye: Vec3,
    look: Vec3,
    feet: Vec3,
    kind: PieceKind,
    map: &PieceMap,
    tuning: &BuildTuning,
) -> PieceSlot {
    let dir = look.try_normalize().unwrap_or(Vec3::NEG_Z);
    let flat = Vec2::new(dir.x, dir.z);
    let flat_len = flat.length().max(1e-3);
    let facing = if flat.length_squared() > 1e-8 {
        Facing::from_direction(Vec3::new(dir.x, 0.0, dir.z))
    } else {
        Facing::North
    };
    // Ray height gained per meter travelled horizontally.
    let slope = dir.y / flat_len;
    // Horizontal meters along the ray per meter along `facing` (diagonal looks
    // travel further before crossing an edge).
    let along = (Vec2::new(facing.vector().x, facing.vector().z).dot(flat) / flat_len).max(0.3);

    let base = feet_level(feet.y);
    let own = {
        let c = GridCell::containing(feet);
        GridCell::new(c.x, c.z, base)
    };
    let off = facing.offset();
    let ahead_level = level_across_front(feet, own, facing, map);
    let ahead = |level: i32| GridCell::new(own.x + off.x, own.z + off.y, level);
    let d_front = distance_to_front_edge(feet, own, facing);
    let height_at = |meters_along_facing: f32| eye.y + slope * meters_along_facing / along;

    match kind {
        PieceKind::Wall => {
            let ground = ahead_level as f32 * LEVEL_HEIGHT;
            let wall = |cell: GridCell, edge_distance: f32| {
                let up = ((height_at(edge_distance) - ground) / LEVEL_HEIGHT)
                    .floor()
                    .clamp(0.0, 1.0) as i32;
                PieceSlot::wall(GridCell::new(cell.x, cell.z, ahead_level + up), facing)
            };
            let own_wall = wall(own, d_front);
            // Pressed up to a free grid line, build on the next one; pressed up to
            // your own wall, keep showing that wall (occupied) rather than
            // building unseen behind it.
            if d_front >= tuning.min_wall_distance() || map.get(own_wall.key()).is_some() {
                own_wall
            } else {
                wall(ahead(0), d_front + CELL_SIZE)
            }
        }
        PieceKind::Floor | PieceKind::Ramp => {
            let own_ground = base as f32 * LEVEL_HEIGHT;
            let ahead_ground = ahead_level as f32 * LEVEL_HEIGHT;
            let cell = if slope < -1e-4 {
                // Looking down: own cell if the ray reaches our ground before our front edge.
                let reach = (eye.y - own_ground) / -slope;
                if reach < d_front / along {
                    own
                } else {
                    ahead(ahead_level)
                }
            } else {
                let ceiling = own_ground + LEVEL_HEIGHT;
                let next = ahead_ground + LEVEL_HEIGHT;
                if slope > 1e-4 && eye.y < ceiling && (ceiling - eye.y) / slope < d_front / along {
                    // Looking steeply up: above our own cell.
                    GridCell::new(own.x, own.z, base + 1)
                } else if slope > 1e-4
                    && eye.y < next
                    && (next - eye.y) / slope < (d_front + CELL_SIZE) / along
                {
                    // Looking up past the next level inside the cell ahead.
                    ahead(ahead_level + 1)
                } else {
                    ahead(ahead_level)
                }
            };
            PieceSlot::new(kind, cell, facing)
        }
    }
}

/// Whether `slot` can be placed at `tick`, given the feet of every character.
pub fn check_placement(
    slot: &PieceSlot,
    map: &PieceMap,
    tick: u64,
    character_feet: &[Vec3],
    tuning: &BuildTuning,
) -> Placement {
    if !slot.in_arena() || slot.cell.level < 0 {
        return Placement::OutOfBounds;
    }
    if !slot.level_ok() {
        return Placement::AboveHeightLimit;
    }
    let key = slot.key();
    if map.get(key).is_some() {
        return Placement::Occupied;
    }
    if map.is_locked(key, tick) {
        return Placement::RebuildLocked;
    }
    if slot.kind == PieceKind::Wall {
        let (min, max) = slot.aabb(tuning);
        if character_feet
            .iter()
            .any(|&feet| capsule_overlaps_box(feet, tuning, min, max))
        {
            return Placement::BlocksCharacter;
        }
    }
    Placement::Valid
}

/// Target plus validity in one call: what the ghost preview shows.
pub fn build_target(
    eye: Vec3,
    look: Vec3,
    feet: Vec3,
    kind: PieceKind,
    map: &PieceMap,
    tick: u64,
    character_feet: &[Vec3],
    tuning: &BuildTuning,
) -> BuildCandidate {
    let slot = target_slot(eye, look, feet, kind, map, tuning);
    BuildCandidate {
        slot,
        placement: check_placement(&slot, map, tick, character_feet, tuning),
    }
}

/// Does a standing character's body (a vertical capsule from the feet up to
/// `trap_height`, radius `trap_radius`) overlap the box `min..max`?
pub fn capsule_overlaps_box(feet: Vec3, tuning: &BuildTuning, min: Vec3, max: Vec3) -> bool {
    let r = tuning.trap_radius;
    let lo = feet.y + r;
    let hi = feet.y + (tuning.trap_height - r).max(r);
    let dx = (min.x - feet.x).max(0.0).max(feet.x - max.x);
    let dz = (min.z - feet.z).max(0.0).max(feet.z - max.z);
    let dy = (min.y - hi).max(0.0).max(lo - max.y);
    dx * dx + dy * dy + dz * dz < r * r
}
