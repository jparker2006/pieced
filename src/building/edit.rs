//! Fortnite-style editing (D44): each piece's edit grid, the valid edit
//! shapes, the collider an edit leaves, and the tile under the crosshair.
//! Pure data and geometry, like the grid; the edit-mode systems live in
//! `building.rs`.
//!
//! **Grids** (tiles counted in reading order, Fortnite's 1–9 are our 0–8):
//! - wall 3×3, seen from the cell the wall belongs to: columns along the
//!   wall's local +X, rows from the top; a tile is 1.33 × 1 m;
//! - floor 2×2 over the cell (row 0 is the north half, local -Z);
//! - ramp 2×2 on the slope (row 0 is the high end, column 0 the left side
//!   walking up);
//! - cone 2×2 corners (tile = the corner of its quadrant).
//!
//! **What a selection means:** removed tiles for walls and floors, the stair
//! path for ramps (its first tile is the foot), raised corners for cones.
//!
//! **Valid shapes** ([`is_valid`]); anything else is refused with a "bwomp":
//! - wall: window (any single tile in the top two rows), wide window (two or
//!   three tiles of the middle row), door (a column's bottom two tiles), arch
//!   (tile 5 plus the bottom row), mid wall (top row), low wall (top two rows),
//!   triangle (an L of three tiles in a corner cuts the wall diagonally), and
//!   pillar (two columns, leaving one);
//! - floor: any one, two or three tiles (a hole, a half floor, a diagonal pair,
//!   a corner floor);
//! - ramp: a half ramp: a two-tile path along one side, rising in any of the
//!   four directions (L and U stairs are not built);
//! - cone: any one, two or three raised corners (a peak, a roof-ramp slope, a
//!   ridge, a tent).
//!
//! Selecting every tile is never valid: it would leave nothing of the piece.

use super::{
    BuildTuning,
    grid::{CONE_HEIGHT, PieceSlot},
};
use crate::shared::{CELL_SIZE, LEVEL_HEIGHT, PieceKind};
use avian3d::prelude::Collider;
use bevy::prelude::*;

const HALF_W: f32 = CELL_SIZE / 2.0;
const HALF_H: f32 = LEVEL_HEIGHT / 2.0;
/// Wall tile width (m).
pub const WALL_TILE_W: f32 = CELL_SIZE / 3.0;
/// Wall tile height (m).
pub const WALL_TILE_H: f32 = LEVEL_HEIGHT / 3.0;
/// How far past a piece's edge the crosshair still picks its edge tile (m).
const HOVER_MARGIN: f32 = 0.25;

/// Edit-grid size (columns, rows) for a piece kind.
pub const fn grid_size(kind: PieceKind) -> (u8, u8) {
    match kind {
        PieceKind::Wall => (3, 3),
        PieceKind::Floor | PieceKind::Ramp | PieceKind::Cone => (2, 2),
    }
}

/// Number of tiles in a kind's edit grid.
pub const fn tile_count(kind: PieceKind) -> u8 {
    let (c, r) = grid_size(kind);
    c * r
}

const fn bits(tiles: &[u8]) -> u16 {
    let mut m = 0u16;
    let mut i = 0;
    while i < tiles.len() {
        m |= 1 << tiles[i];
        i += 1;
    }
    m
}

/// A piece's edit (on the piece entity, and in the piece map).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct PieceEdit {
    /// Selected tiles: bit `i` is tile `i` (see the module docs for what a
    /// selection means per kind). 0 is the full, unedited piece.
    pub tiles: u16,
    /// Ramps: the stair path's first tile (its foot). 0 for other kinds.
    pub start: u8,
}

impl PieceEdit {
    /// The full, unedited piece.
    pub const FULL: Self = Self { tiles: 0, start: 0 };

    /// A wall, floor or cone edit selecting `tiles`.
    pub const fn of(tiles: &[u8]) -> Self {
        Self {
            tiles: bits(tiles),
            start: 0,
        }
    }

    /// A ramp edit: the stair path from `from` to `to`.
    pub const fn path(from: u8, to: u8) -> Self {
        Self {
            tiles: bits(&[from, to]),
            start: from,
        }
    }

    pub fn is_edited(&self) -> bool {
        self.tiles != 0
    }

    pub fn has(&self, tile: u8) -> bool {
        self.tiles & (1 << tile) != 0
    }

    pub fn count(&self) -> u32 {
        self.tiles.count_ones()
    }

    /// The selected tiles, in index order.
    pub fn selected(&self) -> impl Iterator<Item = u8> + '_ {
        (0..16).filter(|&i| self.has(i))
    }

    /// For a wall's edit: whether it opens a gap you can walk through (a door,
    /// an arch, a pillar, or a lower triangle).
    pub fn is_walkable_opening(&self) -> bool {
        (0..3).any(|c| self.has(3 + c) && self.has(6 + c))
            || self.tiles == TRI_LOWER_LEFT
            || self.tiles == TRI_LOWER_RIGHT
    }
}

// Wall tiles:  0 1 2
//              3 4 5
//              6 7 8
const TRI_UPPER_LEFT: u16 = bits(&[0, 1, 3]);
const TRI_UPPER_RIGHT: u16 = bits(&[1, 2, 5]);
const TRI_LOWER_LEFT: u16 = bits(&[3, 6, 7]);
const TRI_LOWER_RIGHT: u16 = bits(&[5, 7, 8]);

/// Every valid wall edit: the removed tiles, with its name.
pub const WALL_SHAPES: [(u16, EditShape); 22] = [
    (bits(&[0]), EditShape::Window),
    (bits(&[1]), EditShape::Window),
    (bits(&[2]), EditShape::Window),
    (bits(&[3]), EditShape::Window),
    (bits(&[4]), EditShape::Window),
    (bits(&[5]), EditShape::Window),
    (bits(&[3, 4]), EditShape::WideWindow),
    (bits(&[4, 5]), EditShape::WideWindow),
    (bits(&[3, 4, 5]), EditShape::WideWindow),
    (bits(&[3, 6]), EditShape::Door),
    (bits(&[4, 7]), EditShape::Door),
    (bits(&[5, 8]), EditShape::Door),
    (bits(&[4, 6, 7, 8]), EditShape::Arch),
    (bits(&[0, 1, 2]), EditShape::MidWall),
    (bits(&[0, 1, 2, 3, 4, 5]), EditShape::LowWall),
    (TRI_UPPER_LEFT, EditShape::Triangle),
    (TRI_UPPER_RIGHT, EditShape::Triangle),
    (TRI_LOWER_LEFT, EditShape::Triangle),
    (TRI_LOWER_RIGHT, EditShape::Triangle),
    (bits(&[0, 1, 3, 4, 6, 7]), EditShape::Pillar),
    (bits(&[1, 2, 4, 5, 7, 8]), EditShape::Pillar),
    (bits(&[0, 2, 3, 5, 6, 8]), EditShape::Pillar),
];

/// The name of an edit shape (for tests, logs and the report).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditShape {
    Window,
    WideWindow,
    Door,
    Arch,
    MidWall,
    LowWall,
    Triangle,
    Pillar,
    FloorHole,
    HalfFloor,
    DiagonalFloor,
    CornerFloor,
    HalfRamp,
    ConePeak,
    ConeSlope,
    ConeRidge,
    ConeTent,
}

fn adjacent_2x2(a: u8, b: u8) -> bool {
    let (ac, ar, bc, br) = (a % 2, a / 2, b % 2, b / 2);
    (ac == bc) != (ar == br)
}

/// What a valid edit is called; `None` for the full piece or an invalid edit.
pub fn shape_of(kind: PieceKind, edit: PieceEdit) -> Option<EditShape> {
    let n = tile_count(kind);
    if edit.tiles == 0 || edit.tiles >> n != 0 {
        return None;
    }
    let count = edit.count();
    let pair = || {
        let mut s = edit.selected();
        (s.next().unwrap_or(0), s.next().unwrap_or(0))
    };
    match kind {
        PieceKind::Wall => WALL_SHAPES
            .iter()
            .find(|(m, _)| *m == edit.tiles)
            .map(|(_, s)| *s),
        PieceKind::Floor => match count {
            1 => Some(EditShape::FloorHole),
            2 => {
                let (a, b) = pair();
                Some(if adjacent_2x2(a, b) {
                    EditShape::HalfFloor
                } else {
                    EditShape::DiagonalFloor
                })
            }
            3 => Some(EditShape::CornerFloor),
            _ => None,
        },
        PieceKind::Ramp => {
            let (a, b) = pair();
            (count == 2 && adjacent_2x2(a, b) && edit.has(edit.start))
                .then_some(EditShape::HalfRamp)
        }
        PieceKind::Cone => match count {
            1 => Some(EditShape::ConePeak),
            2 => {
                let (a, b) = pair();
                Some(if adjacent_2x2(a, b) {
                    EditShape::ConeSlope
                } else {
                    EditShape::ConeRidge
                })
            }
            3 => Some(EditShape::ConeTent),
            _ => None,
        },
    }
}

/// Whether a piece of `kind` may take `edit` (the full piece always may).
pub fn is_valid(kind: PieceKind, edit: PieceEdit) -> bool {
    edit == PieceEdit::FULL || shape_of(kind, edit).is_some()
}

/// Every valid edit of a kind (not counting the full piece), in a fixed order.
pub fn valid_edits(kind: PieceKind) -> Vec<PieceEdit> {
    match kind {
        PieceKind::Wall => WALL_SHAPES
            .iter()
            .map(|(m, _)| PieceEdit {
                tiles: *m,
                start: 0,
            })
            .collect(),
        PieceKind::Ramp => {
            let mut out = Vec::new();
            for a in 0..4u8 {
                for b in 0..4u8 {
                    if a != b && adjacent_2x2(a, b) {
                        out.push(PieceEdit::path(a, b));
                    }
                }
            }
            out
        }
        PieceKind::Floor | PieceKind::Cone => (1u16..15)
            .map(|tiles| PieceEdit { tiles, start: 0 })
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// Geometry of an edit
// ---------------------------------------------------------------------------

/// A wall's kept region when it is cut diagonally: the triangle's corners in
/// the wall's local (x, y).
pub fn wall_triangle(edit: PieceEdit) -> Option<[Vec2; 3]> {
    let (l, r, b, t) = (-HALF_W, HALF_W, -HALF_H, HALF_H);
    match edit.tiles {
        TRI_UPPER_LEFT => Some([vec2(l, b), vec2(r, b), vec2(r, t)]),
        TRI_UPPER_RIGHT => Some([vec2(l, b), vec2(r, b), vec2(l, t)]),
        TRI_LOWER_LEFT => Some([vec2(l, t), vec2(r, t), vec2(r, b)]),
        TRI_LOWER_RIGHT => Some([vec2(l, t), vec2(r, t), vec2(l, b)]),
        _ => None,
    }
}

/// Merges the kept tiles of a `cols × rows` grid into as few rectangles as
/// possible (greedy: runs along a row, grown down while the whole run stays
/// kept). Each is (col0, row0, col1, row1), inclusive.
fn kept_rects(cols: u8, rows: u8, removed: u16) -> Vec<(u8, u8, u8, u8)> {
    let idx = |c: u8, r: u8| r * cols + c;
    let mut used = removed;
    let mut out = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            if used & (1 << idx(c, r)) != 0 {
                continue;
            }
            let mut c1 = c;
            while c1 + 1 < cols && used & (1 << idx(c1 + 1, r)) == 0 {
                c1 += 1;
            }
            let mut r1 = r;
            while r1 + 1 < rows && (c..=c1).all(|cc| used & (1 << idx(cc, r1 + 1)) == 0) {
                r1 += 1;
            }
            for rr in r..=r1 {
                for cc in c..=c1 {
                    used |= 1 << idx(cc, rr);
                }
            }
            out.push((c, r, c1, r1));
        }
    }
    out
}

/// A wall tile's local rectangle (x0, y0)..(x1, y1).
pub fn wall_tile_rect(tile: u8) -> (Vec2, Vec2) {
    let (c, r) = ((tile % 3) as f32, (tile / 3) as f32);
    (
        vec2(-HALF_W + c * WALL_TILE_W, HALF_H - (r + 1.0) * WALL_TILE_H),
        vec2(-HALF_W + (c + 1.0) * WALL_TILE_W, HALF_H - r * WALL_TILE_H),
    )
}

/// A floor, ramp or cone tile's local rectangle (x0, z0)..(x1, z1).
pub fn quarter_rect(tile: u8) -> (Vec2, Vec2) {
    let (c, r) = ((tile % 2) as f32, (tile / 2) as f32);
    (
        vec2(-HALF_W + c * HALF_W, -HALF_W + r * HALF_W),
        vec2(c * HALF_W, r * HALF_W),
    )
}

/// A half ramp's hull: the two path tiles' rectangle on the ground, and its
/// far edge along the path lifted a level. `None` unless `edit` is a half ramp.
pub fn half_ramp_points(edit: PieceEdit) -> Option<[Vec3; 6]> {
    if shape_of(PieceKind::Ramp, edit) != Some(EditShape::HalfRamp) {
        return None;
    }
    let (a, b) = {
        let mut s = edit.selected();
        let (x, y) = (s.next()?, s.next()?);
        if x == edit.start { (x, y) } else { (y, x) }
    };
    let (a0, a1) = quarter_rect(a);
    let (b0, b1) = quarter_rect(b);
    let (lo, hi) = (a0.min(b0), a1.max(b1));
    // Rise direction in local (x, z).
    let rise = vec2(
        (b % 2) as f32 - (a % 2) as f32,
        (b / 2) as f32 - (a / 2) as f32,
    );
    let corners = [
        vec2(lo.x, lo.y),
        vec2(hi.x, lo.y),
        vec2(lo.x, hi.y),
        vec2(hi.x, hi.y),
    ];
    let mid = (lo + hi) / 2.0;
    let mut top = corners.iter().filter(|c| (**c - mid).dot(rise) > 0.0);
    let (t0, t1) = (*top.next()?, *top.next()?);
    Some([
        corners[0].extend(0.0).xzy(),
        corners[1].extend(0.0).xzy(),
        corners[2].extend(0.0).xzy(),
        corners[3].extend(0.0).xzy(),
        t0.extend(LEVEL_HEIGHT).xzy(),
        t1.extend(LEVEL_HEIGHT).xzy(),
    ])
}

/// A cone corner tile's corner on the ground (local x, z).
pub fn cone_corner(tile: u8) -> Vec2 {
    vec2(
        -HALF_W + CELL_SIZE * (tile % 2) as f32,
        -HALF_W + CELL_SIZE * (tile / 2) as f32,
    )
}

/// An edited cone's hull: the base square, each raised corner lifted to the
/// cone's height, and the apex, except for a roof-ramp slope (two adjacent
/// corners raised), which is a plain wedge.
pub fn cone_points(edit: PieceEdit) -> Vec<Vec3> {
    let mut pts: Vec<Vec3> = (0..4).map(|i| cone_corner(i).extend(0.0).xzy()).collect();
    for i in edit.selected().filter(|&i| i < 4) {
        pts.push(cone_corner(i).extend(CONE_HEIGHT).xzy());
    }
    if shape_of(PieceKind::Cone, edit) != Some(EditShape::ConeSlope) {
        pts.push(Vec3::Y * CONE_HEIGHT);
    }
    pts
}

/// The collider a piece keeps under `edit`, in its local space (see
/// `PieceSlot::transform`). Removed tiles have no collision at all.
pub fn edited_collider(slot: &PieceSlot, edit: PieceEdit, tuning: &BuildTuning) -> Collider {
    if !edit.is_edited() || !is_valid(slot.kind, edit) {
        return slot.collider(tuning);
    }
    let boxes = |rects: Vec<(Vec3, Vec3)>| -> Collider {
        Collider::compound(
            rects
                .into_iter()
                .map(|(center, size)| {
                    (
                        center,
                        Quat::IDENTITY,
                        Collider::cuboid(size.x, size.y, size.z),
                    )
                })
                .collect(),
        )
    };
    match slot.kind {
        PieceKind::Wall => {
            let t = tuning.wall_thickness / 2.0;
            if let Some(tri) = wall_triangle(edit) {
                let pts = tri
                    .iter()
                    .flat_map(|p| [p.extend(-t), p.extend(t)])
                    .collect();
                return Collider::convex_hull(pts).unwrap_or_else(|| slot.collider(tuning));
            }
            // Each kept block is a slab like the full wall's: where it reaches
            // the wall's top it keeps the full wall's bevelled ridge, so a ramp
            // still runs straight on over an edited wall.
            let shoulder = HALF_H - t * LEVEL_HEIGHT / CELL_SIZE;
            let blocks: Vec<_> = kept_rects(3, 3, edit.tiles)
                .into_iter()
                .filter_map(|(c0, r0, c1, r1)| {
                    let (lo, _) = wall_tile_rect(r1 * 3 + c0);
                    let (_, hi) = wall_tile_rect(r0 * 3 + c1);
                    let mut pts = Vec::with_capacity(10);
                    for x in [lo.x, hi.x] {
                        for z in [-t, t] {
                            pts.push(Vec3::new(x, lo.y, z));
                            pts.push(Vec3::new(x, if r0 == 0 { shoulder } else { hi.y }, z));
                        }
                        if r0 == 0 {
                            pts.push(Vec3::new(x, hi.y, 0.0));
                        }
                    }
                    Collider::convex_hull(pts).map(|c| (Vec3::ZERO, Quat::IDENTITY, c))
                })
                .collect();
            Collider::compound(blocks)
        }
        PieceKind::Floor => {
            let t = tuning.floor_thickness;
            boxes(
                kept_rects(2, 2, edit.tiles)
                    .into_iter()
                    .map(|(c0, r0, c1, r1)| {
                        let (lo, _) = quarter_rect(r0 * 2 + c0);
                        let (_, hi) = quarter_rect(r1 * 2 + c1);
                        let c = (lo + hi) / 2.0;
                        let s = hi - lo;
                        (Vec3::new(c.x, 0.0, c.y), Vec3::new(s.x, t, s.y))
                    })
                    .collect(),
            )
        }
        PieceKind::Ramp => half_ramp_points(edit)
            .and_then(|p| Collider::convex_hull(p.to_vec()))
            .unwrap_or_else(|| slot.collider(tuning)),
        PieceKind::Cone => {
            Collider::convex_hull(cone_points(edit)).unwrap_or_else(|| slot.collider(tuning))
        }
    }
}

// ---------------------------------------------------------------------------
// Rays: the full shape (edit targeting) and the tile under the crosshair
// ---------------------------------------------------------------------------

/// Outward planes (normal, offset: inside is `n · p <= d`) of a piece's full,
/// unedited shape in its local space.
fn full_shape_planes(kind: PieceKind, tuning: &BuildTuning) -> Vec<(Vec3, f32)> {
    let slab = |half: Vec3| {
        vec![
            (Vec3::X, half.x),
            (Vec3::NEG_X, half.x),
            (Vec3::Y, half.y),
            (Vec3::NEG_Y, half.y),
            (Vec3::Z, half.z),
            (Vec3::NEG_Z, half.z),
        ]
    };
    let slope = LEVEL_HEIGHT / CELL_SIZE;
    let cone = CONE_HEIGHT / HALF_W;
    match kind {
        PieceKind::Wall => slab(Vec3::new(HALF_W, HALF_H, tuning.wall_thickness / 2.0)),
        PieceKind::Floor => slab(Vec3::new(HALF_W, tuning.floor_thickness / 2.0, HALF_W)),
        PieceKind::Ramp => vec![
            (Vec3::NEG_Y, 0.0),
            (Vec3::X, HALF_W),
            (Vec3::NEG_X, HALF_W),
            (Vec3::Z, HALF_W),
            (Vec3::NEG_Z, HALF_W),
            // y + slope·z <= slope·2 (the walking surface).
            {
                let n = Vec3::new(0.0, 1.0, slope);
                (n.normalize(), slope * HALF_W / n.length())
            },
        ],
        PieceKind::Cone => {
            let mut planes = vec![(Vec3::NEG_Y, 0.0)];
            for n in [
                Vec3::new(0.0, 1.0, cone),
                Vec3::new(0.0, 1.0, -cone),
                Vec3::new(cone, 1.0, 0.0),
                Vec3::new(-cone, 1.0, 0.0),
            ] {
                planes.push((n.normalize(), CONE_HEIGHT / n.length()));
            }
            planes
        }
    }
}

/// Where a ray (origin, unit direction) enters a convex volume, as a distance.
fn ray_convex(o: Vec3, d: Vec3, planes: &[(Vec3, f32)]) -> Option<f32> {
    let (mut enter, mut exit) = (0.0f32, f32::INFINITY);
    for &(n, off) in planes {
        let denom = n.dot(d);
        let dist = off - n.dot(o);
        if denom.abs() < 1e-7 {
            if dist < 0.0 {
                return None;
            }
        } else if denom < 0.0 {
            enter = enter.max(dist / denom);
        } else {
            exit = exit.min(dist / denom);
        }
        if enter > exit {
            return None;
        }
    }
    Some(enter)
}

fn to_local(slot: &PieceSlot, origin: Vec3, dir: Vec3) -> (Vec3, Vec3) {
    let inv = slot.transform().compute_affine().inverse();
    (
        inv.transform_point3(origin),
        inv.transform_vector3(dir).normalize_or_zero(),
    )
}

/// Distance along a ray (world space, unit `dir`) to a piece's full, unedited
/// shape: edits never hide a piece from edit targeting (you can aim through
/// your own door to reset it).
pub fn full_shape_hit(
    slot: &PieceSlot,
    origin: Vec3,
    dir: Vec3,
    tuning: &BuildTuning,
) -> Option<f32> {
    let (o, d) = to_local(slot, origin, dir);
    ray_convex(o, d, &full_shape_planes(slot.kind, tuning))
}

/// The edit tile of a piece under a ray (world space): where the ray crosses
/// the piece's edit surface (a wall's or floor's middle plane, a ramp's
/// slope, a cone's pyramid or its base).
pub fn hovered_tile(slot: &PieceSlot, origin: Vec3, dir: Vec3, tuning: &BuildTuning) -> Option<u8> {
    let (o, d) = to_local(slot, origin, dir);
    let on_plane = |n: Vec3, k: f32| -> Option<Vec3> {
        let denom = n.dot(d);
        if denom.abs() < 1e-6 {
            return None;
        }
        let t = (k - n.dot(o)) / denom;
        (t > 0.0).then(|| o + d * t)
    };
    let within = |v: f32, half: f32| v.abs() <= half + HOVER_MARGIN;
    let quarter = |x: f32, z: f32| -> Option<u8> {
        (within(x, HALF_W) && within(z, HALF_W)).then(|| (x >= 0.0) as u8 + 2 * (z >= 0.0) as u8)
    };
    match slot.kind {
        PieceKind::Wall => {
            let p = on_plane(Vec3::Z, 0.0)?;
            if !(within(p.x, HALF_W) && within(p.y, HALF_H)) {
                return None;
            }
            let c = ((p.x + HALF_W) / WALL_TILE_W).floor().clamp(0.0, 2.0) as u8;
            let r = ((HALF_H - p.y) / WALL_TILE_H).floor().clamp(0.0, 2.0) as u8;
            Some(r * 3 + c)
        }
        PieceKind::Floor => {
            let p = on_plane(Vec3::Y, 0.0)?;
            quarter(p.x, p.z)
        }
        PieceKind::Ramp => {
            let slope = LEVEL_HEIGHT / CELL_SIZE;
            let p = on_plane(Vec3::new(0.0, 1.0, slope), slope * HALF_W)?;
            quarter(p.x, p.z)
        }
        PieceKind::Cone => {
            let planes = full_shape_planes(PieceKind::Cone, tuning);
            let p = match ray_convex(o, d, &planes) {
                Some(t) if t > 0.0 => o + d * t,
                _ => on_plane(Vec3::Y, 0.0)?,
            };
            quarter(p.x, p.z)
        }
    }
}

/// A tile's quad on its piece's edit surface, in local space (for the edit
/// grid overlay): wall tiles on the middle plane, floor tiles on the middle
/// plane, ramp tiles on the slope, cone tiles on the pyramid (a quadrant is
/// folded over the hip: its corners are the ground corner, two edge midpoints
/// and the apex).
pub fn tile_quad(kind: PieceKind, tile: u8) -> [Vec3; 4] {
    match kind {
        PieceKind::Wall => {
            let (lo, hi) = wall_tile_rect(tile);
            [
                Vec3::new(lo.x, lo.y, 0.0),
                Vec3::new(hi.x, lo.y, 0.0),
                Vec3::new(hi.x, hi.y, 0.0),
                Vec3::new(lo.x, hi.y, 0.0),
            ]
        }
        PieceKind::Floor | PieceKind::Ramp => {
            let (lo, hi) = quarter_rect(tile);
            let y = |z: f32| {
                if kind == PieceKind::Ramp {
                    super::grid::ramp_surface_height(z)
                } else {
                    0.0
                }
            };
            [
                Vec3::new(lo.x, y(lo.y), lo.y),
                Vec3::new(hi.x, y(lo.y), lo.y),
                Vec3::new(hi.x, y(hi.y), hi.y),
                Vec3::new(lo.x, y(hi.y), hi.y),
            ]
        }
        PieceKind::Cone => {
            let corner = cone_corner(tile);
            [
                corner.extend(0.0).xzy(),
                Vec3::new(0.0, 0.0, corner.y),
                Vec3::Y * CONE_HEIGHT,
                Vec3::new(corner.x, 0.0, 0.0),
            ]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::{Facing, GridCell};

    #[test]
    fn shapes_are_named_and_everything_else_is_invalid() {
        assert_eq!(valid_edits(PieceKind::Wall).len(), 22);
        assert_eq!(valid_edits(PieceKind::Floor).len(), 14);
        assert_eq!(valid_edits(PieceKind::Ramp).len(), 8);
        assert_eq!(valid_edits(PieceKind::Cone).len(), 14);
        for kind in [
            PieceKind::Wall,
            PieceKind::Floor,
            PieceKind::Ramp,
            PieceKind::Cone,
        ] {
            for e in valid_edits(kind) {
                assert!(shape_of(kind, e).is_some(), "{kind:?} {e:?}");
            }
            // Everything selected is never valid.
            let all = (1u16 << tile_count(kind)) - 1;
            assert!(!is_valid(
                kind,
                PieceEdit {
                    tiles: all,
                    start: 0
                }
            ));
            assert!(is_valid(kind, PieceEdit::FULL));
        }
        let wall = |t: &[u8]| shape_of(PieceKind::Wall, PieceEdit::of(t));
        assert_eq!(wall(&[4]), Some(EditShape::Window));
        assert_eq!(wall(&[4, 7]), Some(EditShape::Door));
        assert_eq!(wall(&[4, 6, 7, 8]), Some(EditShape::Arch));
        assert_eq!(wall(&[0, 1, 2]), Some(EditShape::MidWall));
        assert_eq!(wall(&[0, 8]), None, "two loose corners");
        assert_eq!(wall(&[0, 4, 8]), None, "a diagonal");
        assert_eq!(wall(&[7]), None, "a lone bottom tile");
        let ramp = |a: u8, b: u8| shape_of(PieceKind::Ramp, PieceEdit::path(a, b));
        assert_eq!(ramp(2, 0), Some(EditShape::HalfRamp));
        assert_eq!(ramp(0, 3), None, "diagonal tiles aren't a path");
        let one = PieceEdit::of(&[1]);
        assert_eq!(
            shape_of(PieceKind::Ramp, one),
            None,
            "one tile isn't a stair"
        );
        let cone = |t: &[u8]| shape_of(PieceKind::Cone, PieceEdit::of(t));
        assert_eq!(cone(&[0]), Some(EditShape::ConePeak));
        assert_eq!(cone(&[0, 1]), Some(EditShape::ConeSlope));
        assert_eq!(cone(&[0, 3]), Some(EditShape::ConeRidge));
        assert_eq!(cone(&[0, 1, 2]), Some(EditShape::ConeTent));
    }

    #[test]
    fn kept_tiles_merge_into_few_rectangles() {
        // A door leaves the top row and the two side columns.
        let rects = kept_rects(3, 3, PieceEdit::of(&[4, 7]).tiles);
        assert_eq!(rects, vec![(0, 0, 2, 0), (0, 1, 0, 2), (2, 1, 2, 2)]);
        assert_eq!(kept_rects(3, 3, 0), vec![(0, 0, 2, 2)]);
        assert_eq!(
            kept_rects(2, 2, PieceEdit::of(&[0, 1]).tiles),
            vec![(0, 1, 1, 1)]
        );
    }

    #[test]
    fn half_ramps_rise_along_their_path() {
        // Tiles 2 -> 0: the left side, rising toward the high end (-Z).
        let p = half_ramp_points(PieceEdit::path(2, 0)).unwrap();
        let top: Vec<Vec3> = p.iter().copied().filter(|v| v.y > 1.0).collect();
        assert!(top.iter().all(|v| v.z == -2.0 && v.x <= 0.0), "{top:?}");
        assert!(p.iter().all(|v| v.x <= 0.0));
        // Tiles 3 -> 2: the low half, rising toward -X.
        let p = half_ramp_points(PieceEdit::path(3, 2)).unwrap();
        let top: Vec<Vec3> = p.iter().copied().filter(|v| v.y > 1.0).collect();
        assert!(top.iter().all(|v| v.x == -2.0 && v.z >= 0.0), "{top:?}");
    }

    #[test]
    fn the_crosshair_picks_tiles_in_reading_order() {
        let tuning = BuildTuning::default();
        // A north-facing wall at the north edge of cell (5, 5), seen from its cell.
        let slot = PieceSlot::wall(GridCell::new(5, 5, 0), Facing::North);
        let c = slot.transform().translation;
        let eye = c + Vec3::Z * 3.0;
        let at = |x: f32, y: f32| {
            let target = c + Vec3::new(x, y, 0.0);
            hovered_tile(&slot, eye, (target - eye).normalize(), &tuning)
        };
        assert_eq!(at(-1.6, 1.2), Some(0));
        assert_eq!(at(0.0, 0.0), Some(4));
        assert_eq!(at(1.6, -1.2), Some(8));
        assert_eq!(at(0.0, 5.0), None);
        // Floors, ramps and cones: quarters.
        let floor = PieceSlot::floor(GridCell::new(5, 5, 0));
        let b = floor.transform().translation;
        let from = b + Vec3::new(0.0, 5.0, 0.0);
        let down_to = |x: f32, z: f32| (b + Vec3::new(x, 0.0, z) - from).normalize();
        assert_eq!(
            hovered_tile(&floor, from, down_to(-1.0, -1.0), &tuning),
            Some(0)
        );
        assert_eq!(
            hovered_tile(&floor, from, down_to(1.0, 1.0), &tuning),
            Some(3)
        );
        let cone = PieceSlot::cone(GridCell::new(5, 5, 0));
        assert_eq!(
            hovered_tile(&cone, from, down_to(1.0, -1.0), &tuning),
            Some(1)
        );
        // From under the cone (inside a box), its base.
        let under = b - Vec3::Y * 1.5;
        let up_to = (b + Vec3::new(-1.0, 0.0, 1.0) - under).normalize();
        assert_eq!(hovered_tile(&cone, under, up_to, &tuning), Some(2));
        // The full shape is found through an edit.
        assert!(full_shape_hit(&slot, eye, Vec3::NEG_Z, &tuning).is_some());
        assert!(full_shape_hit(&slot, eye, Vec3::Z, &tuning).is_none());
    }
}
