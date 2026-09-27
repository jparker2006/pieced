//! First-person build targeting: where the ghost goes, and whether it can be placed.
//! Pure functions of the view, the feet and the piece map, so they're unit-testable
//! and shared by the player, scenarios and (later) bots.
//!
//! The rules, in player terms:
//! - Everything snaps to the nearest of the four yaws you're looking along (`facing`).
//! - Reach is your own cell plus one cell ahead along `facing`.
//! - **Wall:** on your own cell's edge in front of you, wherever you stand in the
//!   cell (a wall placed against you pushes you back into the cell; see
//!   `movement`). It sits on whatever you stand on, and looking up past that
//!   level's top builds one level higher. Climbing a ramp, the wall goes at the
//!   high end of the next ramp (the far edge of the cell ahead, one level up), or
//!   under your own ramp's top when you look below it: never on the edge you step
//!   across onto the next ramp.
//! - **Floor / ramp:** in the cell ahead, at the level of the ground there (one up
//!   while climbing a ramp). Looking down at your own cell targets it instead;
//!   looking up past the next level's height builds one level higher (above your
//!   own cell when you look steeply up). Looks steeper than [`STEEP_LOOK_DEG`]
//!   always mean your own cell. Ramps rise away from you. Climbing a ramp, the
//!   cell above it is never targeted (it would cap the ramp and wedge you under
//!   it); turn away from its rise and it is (that's how 90s stack up).
//! - **Cone:** like a floor (D43, research R11): the cell ahead, your own cell
//!   looking down, or on top of your box looking steeply up. A ramp rush never
//!   changes it.
//!
//! **Ramp rushing** (moving forward with the ramp: `advancing`) swaps the pitch
//! rules for the chain rule, so holding forward and build runs up an endless ramp
//! whatever the look pitch:
//! - on a ramp rising the way you're heading (within [`RUSH_TOLERANCE_DEG`]), the
//!   next ramp continues it: the cell past its top, one level up, same facing;
//! - anywhere else, the chain starts in the cell ahead at the level of the ground
//!   there. Never a level up (you'd run under it and wedge beneath it), and never
//!   in your own cell (you'd run off it), except when you look steeply down or a
//!   wall of yours stands across the way: then it goes under you and lifts you.
//!
//! While falling (off the top of a finished chain, or off its side) the rush
//! only continues a ramp you're dropping onto; a new chain waits for your feet,
//! since a ramp at your level would pass over your head.
//!
//! A wall built while rushing, with a ramp to climb ahead (or underfoot), goes on
//! the far edge of the next ramp's cell at that ramp's level: it shields the
//! climb instead of blocking the way onto it.

use super::{
    BuildTuning,
    grid::{PieceMap, PieceSlot},
};
use crate::shared::{CELL_SIZE, Facing, GridCell, LEVEL_HEIGHT, PieceKind};
use bevy::prelude::*;

/// Feet up to this far below a level's height count as standing on that level
/// (ramp tops, small bumps, floor slabs).
pub const LEVEL_SNAP: f32 = 0.3;

/// While ramp rushing, a ramp under you whose rise is within this many degrees of
/// your look keeps the chain going its way, so a wobbly look can't bend the chain.
pub const RUSH_TOLERANCE_DEG: f32 = 60.0;

/// Floors and ramps go in your own cell when you look at least this far down
/// (at your level) or up (one level up), wherever you stand in the cell.
pub const STEEP_LOOK_DEG: f32 = 55.0;

/// How a builder is moving, which ramp rushing depends on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Gait {
    /// Not moving forward: the look pitch picks where pieces go.
    #[default]
    Standing,
    /// Moving forward on the ground (or rising): ramps rush.
    Advancing,
    /// Moving forward while falling: the rush waits to land.
    Falling,
}

impl Gait {
    pub fn is_advancing(self) -> bool {
        self != Gait::Standing
    }
}

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
    /// Rushing (ramps and walls) while falling: the rush waits until you land.
    Falling,
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

/// The ramp the feet are on (its level and the way it rises), if any: the ramp
/// in the level band the feet are in, or, with the feet a hair across a level
/// line, the ramp whose foot or top edge they stand on.
fn ramp_underfoot(feet: Vec3, own: GridCell, map: &PieceMap) -> Option<(i32, Facing)> {
    let exact = (feet.y / LEVEL_HEIGHT).floor() as i32;
    let ramp = |level: i32| {
        map.ramp_at(GridCell::new(own.x, own.z, level))
            .map(|(_, rise)| (level, rise))
    };
    let surface = |(level, rise): (i32, Facing)| {
        let run = CELL_SIZE - distance_to_front_edge(feet, own, rise);
        level as f32 * LEVEL_HEIGHT + (run / CELL_SIZE).clamp(0.0, 1.0) * LEVEL_HEIGHT
    };
    let line = exact as f32 * LEVEL_HEIGHT;
    ramp(exact)
        .or_else(|| {
            ramp(exact + 1).filter(|&r| {
                line + LEVEL_HEIGHT - feet.y < LEVEL_SNAP && surface(r) < feet.y + LEVEL_SNAP
            })
        })
        .or_else(|| {
            ramp(exact - 1)
                .filter(|&r| feet.y - line < LEVEL_SNAP && surface(r) > feet.y - LEVEL_SNAP)
        })
}

/// Level of the ground just across the front edge of the feet's cell: one level
/// up while climbing a ramp that rises along `facing`, the ramp's base while
/// descending one, otherwise the feet level.
fn level_across_front(feet: Vec3, own: GridCell, facing: Facing, map: &PieceMap) -> i32 {
    match ramp_underfoot(feet, own, map) {
        Some((level, rise)) if rise == facing => level + 1,
        Some((level, rise)) if rise == facing.opposite() => level.max(0),
        _ => feet_level(feet.y),
    }
}

/// The next ramp of a ramp rush, and whether it continues a ramp we're climbing:
/// past the top of the ramp underfoot, one level up, when it rises within
/// [`RUSH_TOLERANCE_DEG`] of `heading`; otherwise the chain's first ramp, in the
/// cell ahead at the level of the ground there.
fn rush_ramp(
    feet: Vec3,
    own: GridCell,
    heading: Vec3,
    facing: Facing,
    ahead_level: i32,
    steep_down: bool,
    map: &PieceMap,
) -> (PieceSlot, bool) {
    let tolerance = RUSH_TOLERANCE_DEG.to_radians().cos();
    match ramp_underfoot(feet, own, map) {
        Some((level, rise)) if rise.vector().dot(heading) >= tolerance => {
            let o = rise.offset();
            let next = GridCell::new(own.x + o.x, own.z + o.y, level + 1);
            (PieceSlot::ramp(next, rise), true)
        }
        // Looking steeply down, or up against a wall of ours: under us (the ramp
        // lifts us onto it and the rush goes on from there).
        _ if steep_down || map.wall_at(own, facing).is_some() => {
            (PieceSlot::ramp(own, facing), false)
        }
        _ => {
            let o = facing.offset();
            let next = GridCell::new(own.x + o.x, own.z + o.y, ahead_level);
            (PieceSlot::ramp(next, facing), false)
        }
    }
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
/// looking along `look`, feet at `feet`, `advancing` when it's moving forward
/// (ramp rushing, see the module docs). Never fails: the result may be invalid
/// (check it with [`check_placement`]).
pub fn target_slot(
    eye: Vec3,
    look: Vec3,
    feet: Vec3,
    kind: PieceKind,
    advancing: bool,
    map: &PieceMap,
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
    let steep = STEEP_LOOK_DEG.to_radians().sin();
    // On a ramp rising straight ahead: its level, if so.
    let climbing = ramp_underfoot(feet, own, map)
        .filter(|&(_, rise)| rise == facing)
        .map(|(level, _)| level);

    if advancing {
        let heading = Vec3::new(flat.x, 0.0, flat.y) / flat_len;
        let (next_ramp, continues) = rush_ramp(
            feet,
            own,
            heading,
            facing,
            ahead_level,
            dir.y <= -steep,
            map,
        );
        match kind {
            PieceKind::Ramp => return next_ramp,
            // The wall that shields the next ramp's climb: on the far edge of its
            // cell, at its level (never across the way onto it).
            PieceKind::Wall
                if continues
                    || next_ramp.cell != own
                        && map
                            .ramp_at(next_ramp.cell)
                            .is_some_and(|(_, rise)| rise == next_ramp.facing) =>
            {
                return PieceSlot::wall(next_ramp.cell, next_ramp.facing);
            }
            _ => {}
        }
    }

    match kind {
        PieceKind::Wall => {
            // Looking up past the top of the level the wall stands on builds one higher.
            let wall = |cell: GridCell, level: i32, edge_distance: f32| {
                let ground = level as f32 * LEVEL_HEIGHT;
                let up = ((height_at(edge_distance) - ground) / LEVEL_HEIGHT)
                    .floor()
                    .clamp(0.0, 1.0) as i32;
                PieceSlot::wall(GridCell::new(cell.x, cell.z, level + up), facing)
            };
            match climbing {
                // Aiming below our own ramp's top: the wall under it.
                Some(level) if height_at(d_front) < (level + 1) as f32 * LEVEL_HEIGHT => {
                    PieceSlot::wall(GridCell::new(own.x, own.z, level), facing)
                }
                // Otherwise the high end of the next ramp.
                Some(level) => wall(ahead(0), level + 1, d_front + CELL_SIZE),
                None => wall(own, ahead_level, d_front),
            }
        }
        // Cones target like floors (docs/research/fortnite-building.md, R11).
        PieceKind::Floor | PieceKind::Ramp | PieceKind::Cone => {
            let own_ground = base as f32 * LEVEL_HEIGHT;
            let ahead_ground = ahead_level as f32 * LEVEL_HEIGHT;
            let cell = if climbing.is_some() {
                // Climbing a ramp puts the eye just under the next level, so any
                // look crosses it inside our own cell. Building there would cap
                // the ramp we're on (and wedge us under it), so while climbing
                // we build ahead: one level higher only when looking up past the
                // next level inside the cell ahead (a cover ramp).
                let next = ahead_ground + LEVEL_HEIGHT;
                if slope > 1e-4
                    && eye.y < next
                    && (next - eye.y) / slope < (d_front + CELL_SIZE) / along
                {
                    ahead(ahead_level + 1)
                } else {
                    ahead(ahead_level)
                }
            } else if dir.y <= -steep {
                own
            } else if dir.y >= steep {
                GridCell::new(own.x, own.z, base + 1)
            } else if slope < -1e-4 {
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
    gait: Gait,
    map: &PieceMap,
    tick: u64,
    character_feet: &[Vec3],
    tuning: &BuildTuning,
) -> BuildCandidate {
    let slot = target_slot(eye, look, feet, kind, gait.is_advancing(), map);
    let placement = match check_placement(&slot, map, tick, character_feet, tuning) {
        // Falling, the rush only continues a ramp we're dropping onto (and walls it).
        Placement::Valid
            if gait == Gait::Falling
                && matches!(kind, PieceKind::Ramp | PieceKind::Wall)
                && !continues_ramp_underfoot(
                    &PieceSlot::ramp(slot.cell, slot.facing),
                    feet,
                    map,
                ) =>
        {
            Placement::Falling
        }
        placement => placement,
    };
    BuildCandidate { slot, placement }
}

/// Whether `slot` is the ramp past the top of the ramp under `feet`.
fn continues_ramp_underfoot(slot: &PieceSlot, feet: Vec3, map: &PieceMap) -> bool {
    let own = GridCell::containing(feet);
    let o = slot.facing.offset();
    (own.x + o.x, own.z + o.y) == (slot.cell.x, slot.cell.z)
        && ramp_underfoot(feet, own, map) == Some((slot.cell.level - 1, slot.facing))
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
