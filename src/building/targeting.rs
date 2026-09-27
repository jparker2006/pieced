//! First-person build targeting: where the ghost goes, and whether it can be placed.
//! Pure functions of the view, the feet and the piece map, so they're unit-testable
//! and shared by the player, scenarios and (later) bots.
//!
//! The rules, in player terms:
//! - Everything snaps to the nearest of the four yaws you're looking along (`facing`).
//! - Reach (Fortnite's): floors, ramps and cones go in the tile the aim ray lands on,
//!   among the tiles around yours, diagonals included (`reach_tiles`, 1 or 2),
//!   at your level looking down or one level up looking up. When the ray lands
//!   nowhere within reach (looking about level), they go in the cell ahead.
//! - **Wall:** on your own cell's edge in front of you, wherever you stand in the
//!   cell (a wall placed against you pushes you back into the cell; see
//!   `movement`). It sits on whatever you stand on, and looking up past that
//!   level's top builds one level higher. Climbing a ramp, the wall goes at the
//!   high end of the next ramp (the far edge of the cell ahead, one level up), or
//!   under your own ramp's top when you look below it: never on the edge you step
//!   across onto the next ramp.
//! - **Floor / ramp:** where the aim lands (above); straight ahead, at the level
//!   of the ground there (one up while climbing a ramp). Looks steeper than
//!   [`STEEP_LOOK_DEG`] always mean your own cell. While advancing, the landing
//!   point is pushed [`FORWARD_BIAS`] further ahead. Ramps rise away from you. Climbing a ramp, the
//!   cell above it is never targeted (it would cap the ramp and wedge you under
//!   it); turn away from its rise and it is (that's how 90s stack up).
//! - **Cone:** like a floor (D43, research R11), with the same reach: where the
//!   aim lands, your own cell looking steeply down, or on top of your box looking
//!   steeply up. A ramp rush never changes it.
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
//! - swinging the aim across by more than half a tile (by the time it's as far
//!   along as the next ramp's middle) builds on the tile beside, level with it:
//!   sweep the aim across and you get a double ramp.
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

/// While advancing, where the aim ray lands is pushed this much further along
/// the facing (m): about 0.2 s of sprint, making up for Fortnite's camera
/// sitting behind the player, so a casual downward look lands ahead.
pub const FORWARD_BIAS: f32 = 1.5;

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

/// How far across a ramp's side edge the feet may slip (the body still on it)
/// and still count as standing on that ramp: the body's radius.
const SIDE_SLIP: f32 = 0.35;

/// The cell we build from, at `base`: the one the feet are in or, with the feet
/// a hair across a side edge of the ramp the body is standing on, that ramp's
/// cell (so running up near a ramp's edge never builds on top of it).
fn own_cell(feet: Vec3, base: i32, map: &PieceMap) -> GridCell {
    let c = GridCell::containing(feet);
    let own = GridCell::new(c.x, c.z, base);
    if ramp_underfoot(feet, own, map).is_some() {
        return own;
    }
    Facing::ALL
        .into_iter()
        .filter(|&side| distance_to_front_edge(feet, own, side) < SIDE_SLIP)
        .map(|side| {
            (
                side,
                GridCell::new(own.x + side.offset().x, own.z + side.offset().y, base),
            )
        })
        .find(|&(side, cell)| {
            map.ramp_at(GridCell::new(
                cell.x,
                cell.z,
                (feet.y / LEVEL_HEIGHT).floor() as i32,
            ))
            .is_some_and(|(_, rise)| {
                let run = CELL_SIZE - distance_to_front_edge(feet, cell, rise);
                let level = (feet.y / LEVEL_HEIGHT).floor();
                let surface = (level + (run / CELL_SIZE).clamp(0.0, 1.0)) * LEVEL_HEIGHT;
                rise != side && rise != side.opposite() && (surface - feet.y).abs() < LEVEL_SNAP
            })
        })
        .map_or(own, |(_, cell)| cell)
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

/// One builder's view of the grid, which every piece kind picks its slot from
/// (see [`target_slot`]). Floor-like pieces (floors, standing ramps, cones)
/// take their cell from [`View::floor_cell`].
struct View<'a> {
    eye: Vec3,
    /// Unit look direction.
    dir: Vec3,
    /// Unit horizontal look direction.
    heading: Vec3,
    facing: Facing,
    feet: Vec3,
    /// The level we build from (see [`feet_level`]).
    base: i32,
    /// Our cell, at `base`.
    own: GridCell,
    /// Level of the ground across our cell's front edge.
    ahead_level: i32,
    /// Distance from the feet to our cell's front edge, along `facing`.
    d_front: f32,
    /// Ray height gained per meter travelled horizontally.
    slope: f32,
    /// Horizontal meters along the ray per meter along `facing`.
    along: f32,
    /// The level of the ramp we're climbing straight ahead, if any.
    climbing: Option<i32>,
    advancing: bool,
    reach: i32,
    map: &'a PieceMap,
}

impl<'a> View<'a> {
    fn new(
        eye: Vec3,
        look: Vec3,
        feet: Vec3,
        advancing: bool,
        map: &'a PieceMap,
        tuning: &BuildTuning,
    ) -> Self {
        let dir = look.try_normalize().unwrap_or(Vec3::NEG_Z);
        let flat = Vec3::new(dir.x, 0.0, dir.z);
        let flat_len = flat.length().max(1e-3);
        let facing = if flat.length_squared() > 1e-8 {
            Facing::from_direction(flat)
        } else {
            Facing::North
        };
        let heading = if flat.length_squared() > 1e-8 {
            flat / flat_len
        } else {
            facing.vector()
        };
        let base = feet_level(feet.y);
        let own = own_cell(feet, base, map);
        let ahead_level = level_across_front(feet, own, facing, map);
        Self {
            eye,
            dir,
            heading,
            facing,
            feet,
            base,
            own,
            ahead_level,
            d_front: distance_to_front_edge(feet, own, facing),
            slope: dir.y / flat_len,
            along: facing.vector().dot(heading).max(0.3),
            climbing: ramp_underfoot(feet, own, map)
                .filter(|&(_, rise)| rise == facing)
                .map(|(level, _)| level),
            advancing,
            reach: tuning.reach_tiles.max(1),
            map,
        }
    }

    /// The cell `offset` tiles from ours, at `level`.
    fn beside(&self, offset: IVec2, level: i32) -> GridCell {
        GridCell::new(self.own.x + offset.x, self.own.z + offset.y, level)
    }

    /// The cell ahead of ours, at `level`.
    fn ahead(&self, level: i32) -> GridCell {
        self.beside(self.facing.offset(), level)
    }

    /// Height of the aim ray `meters` along `facing` from the feet.
    fn height_at(&self, meters: f32) -> f32 {
        self.eye.y + self.slope * meters / self.along
    }

    fn steep_down(&self) -> bool {
        self.dir.y <= -STEEP_LOOK_DEG.to_radians().sin()
    }

    fn steep_up(&self) -> bool {
        self.dir.y >= STEEP_LOOK_DEG.to_radians().sin()
    }

    /// Where the aim ray lands (XZ) on the level plane at height `y`, pushed
    /// [`FORWARD_BIAS`] further along `facing` while advancing.
    fn landing(&self, y: f32) -> Option<Vec2> {
        let rise = y - self.eye.y;
        if rise * self.slope <= 0.0 || self.slope.abs() < 1e-4 {
            return None;
        }
        let mut p = self.eye + self.heading * (rise / self.slope);
        if self.advancing {
            p += self.facing.vector() * FORWARD_BIAS;
        }
        Some(Vec2::new(p.x, p.z))
    }

    /// Offset (in tiles) of the tile under `p` from ours, if within reach.
    fn tile_within_reach(&self, p: Vec2) -> Option<IVec2> {
        let tile = GridCell::containing(Vec3::new(p.x, 0.0, p.y));
        let offset = IVec2::new(tile.x - self.own.x, tile.z - self.own.z);
        (offset.x.abs() <= self.reach && offset.y.abs() <= self.reach).then_some(offset)
    }

    /// The cell a floor or (standing) ramp goes in: the tile the aim ray lands
    /// on, around ours (diagonals included, up to `reach_tiles` away), at our
    /// level when looking down or one up when looking up; the cell ahead at the
    /// ground level there when the ray doesn't land within reach.
    fn floor_cell(&self) -> GridCell {
        if self.climbing.is_some() {
            // Climbing a ramp puts the eye just under the next level, so any
            // look crosses it inside our own cell. Building there would cap
            // the ramp we're on (and wedge us under it), so while climbing we
            // build ahead: one level higher only when looking up past the next
            // level inside the cell ahead (a cover ramp).
            let next = (self.ahead_level + 1) as f32 * LEVEL_HEIGHT;
            let crosses = self.slope > 1e-4
                && self.eye.y < next
                && (next - self.eye.y) / self.slope < (self.d_front + CELL_SIZE) / self.along;
            return self.ahead(self.ahead_level + i32::from(crosses));
        }
        if self.steep_down() {
            return self.own;
        }
        if self.steep_up() {
            return self.beside(IVec2::ZERO, self.base + 1);
        }
        let up = i32::from(self.slope > 0.0);
        let plane = (self.base + up) as f32 * LEVEL_HEIGHT;
        match self.landing(plane).and_then(|p| self.tile_within_reach(p)) {
            // Straight ahead, the ground there may be a level off ours (a ramp
            // down, say).
            Some(offset) if offset == self.facing.offset() => self.ahead(self.ahead_level + up),
            Some(offset) => self.beside(offset, self.base + up),
            None => self.ahead(self.ahead_level),
        }
    }

    /// The wall slot: our cell's edge in front of us (see the module docs).
    fn wall(&self) -> PieceSlot {
        // Looking up past the top of the level the wall stands on builds one higher.
        let wall = |cell: GridCell, level: i32, edge_distance: f32| {
            let ground = level as f32 * LEVEL_HEIGHT;
            let up = ((self.height_at(edge_distance) - ground) / LEVEL_HEIGHT)
                .floor()
                .clamp(0.0, 1.0) as i32;
            PieceSlot::wall(GridCell::new(cell.x, cell.z, level + up), self.facing)
        };
        match self.climbing {
            // Aiming below our own ramp's top: the wall under it.
            Some(level) if self.height_at(self.d_front) < (level + 1) as f32 * LEVEL_HEIGHT => {
                PieceSlot::wall(GridCell::new(self.own.x, self.own.z, level), self.facing)
            }
            // Otherwise the high end of the next ramp.
            Some(level) => wall(self.ahead(0), level + 1, self.d_front + CELL_SIZE),
            None => wall(self.own, self.ahead_level, self.d_front),
        }
    }

    /// The next ramp of a ramp rush, and whether it continues a ramp we're
    /// climbing: past the top of the ramp underfoot, one level up, when it rises
    /// within [`RUSH_TOLERANCE_DEG`] of our heading; under us when looking
    /// steeply down or up against a wall of ours (it lifts us, and the rush goes
    /// on from there); otherwise the chain's first ramp, in the cell ahead at
    /// the ground level there. Aiming at the tile beside the next ramp builds
    /// there instead, level with it (double ramps).
    fn rush(&self) -> (PieceSlot, bool) {
        let tolerance = RUSH_TOLERANCE_DEG.to_radians().cos();
        let (next, continues) = match ramp_underfoot(self.feet, self.own, self.map) {
            Some((level, rise)) if rise.vector().dot(self.heading) >= tolerance => {
                let cell = self.beside(rise.offset(), level + 1);
                (PieceSlot::ramp(cell, rise), true)
            }
            _ if self.steep_down() || self.map.wall_at(self.own, self.facing).is_some() => {
                return (PieceSlot::ramp(self.own, self.facing), false);
            }
            _ => (
                PieceSlot::ramp(self.ahead(self.ahead_level), self.facing),
                false,
            ),
        };
        // How far to the side the aim has swung by the time it's as far along
        // as the next ramp's middle, measured from wherever we are across our
        // own column (so running off-centre never pulls the chain aside): past
        // half a tile, it's on the tile beside.
        let rise = next.facing.vector();
        let side = rise.cross(Vec3::Y);
        let to_middle = (next.cell.base_center() - self.eye).dot(rise);
        let heading_along = self.heading.dot(rise);
        if to_middle <= 0.0 || heading_along < 0.1 {
            return (next, continues);
        }
        let lateral = self.heading.dot(side) * to_middle / heading_along;
        if lateral.abs() <= CELL_SIZE / 2.0 {
            return (next, continues);
        }
        let step = side * lateral.signum();
        let cell = GridCell::new(
            next.cell.x + step.x.round() as i32,
            next.cell.z + step.z.round() as i32,
            next.cell.level,
        );
        (PieceSlot::ramp(cell, next.facing), false)
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
    tuning: &BuildTuning,
) -> PieceSlot {
    let view = View::new(eye, look, feet, advancing, map, tuning);
    if advancing {
        let (next_ramp, continues) = view.rush();
        match kind {
            PieceKind::Ramp => return next_ramp,
            // The wall that shields the next ramp's climb: on the far edge of its
            // cell, at its level (never across the way onto it).
            PieceKind::Wall
                if continues
                    || next_ramp.cell != view.own
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
        PieceKind::Wall => view.wall(),
        // Cones target like floors (docs/research/fortnite-building.md, R11),
        // sharing the cell with a floor or ramp at that level.
        PieceKind::Floor | PieceKind::Ramp | PieceKind::Cone => {
            PieceSlot::new(kind, view.floor_cell(), view.facing)
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
    let slot = target_slot(eye, look, feet, kind, gait.is_advancing(), map, tuning);
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
    let own = own_cell(feet, feet_level(feet.y), map);
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
