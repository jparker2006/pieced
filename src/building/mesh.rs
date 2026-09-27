//! Piece geometry. The pieces themselves are Blender models
//! (`art/blender/assets/pieces.py`: a chunky brick wall, a warped plank floor
//! and ramp, each with two crack stages, and the debris chunks): this module
//! names them, turns a loaded model's glTF scene into one mesh in its piece's
//! local space (see `PieceSlot::transform`), and builds the translucent ghost
//! previews in code.
//!
//! The models keep Milestone 1's extents, so what you see matches the
//! unchanged colliders: a wall is 4 m wide, 3 m tall and 0.3 m thick (collider
//! 0.2 m) and centred on its piece; a floor is 4 × 4 m with its top at +0.1 m
//! (collider ±0.1 m); a ramp's plank tops lie on the collider's slope, rising
//! 3 m over 4 m toward local -Z from its base centre.

use super::edit::{self, EditShape, PieceEdit, WALL_TILE_H, WALL_TILE_W};
use crate::{
    models::MODEL_FORWARD_FIX,
    shared::{CELL_SIZE, LEVEL_HEIGHT, PieceKind},
};
use bevy::{
    asset::RenderAssetUsages,
    gltf::GltfMeshName,
    math::Affine3A,
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    prelude::*,
    world_serialization::WorldAsset,
};
use std::collections::BTreeMap;

/// Model names per kind and crack stage (0 intact, 1 at 66% HP, 2 at 33%).
pub(crate) const PIECE_MODELS: [[&str; 3]; 4] = [
    ["wall_brick", "wall_brick_crack1", "wall_brick_crack2"],
    ["floor_plank", "floor_plank_crack1", "floor_plank_crack2"],
    ["ramp_plank", "ramp_plank_crack1", "ramp_plank_crack2"],
    ["cone_plank", "cone_plank_crack1", "cone_plank_crack2"],
];
/// Every piece kind, in [`kind_index`] order.
pub(crate) const KINDS: [PieceKind; 4] = [
    PieceKind::Wall,
    PieceKind::Floor,
    PieceKind::Ramp,
    PieceKind::Cone,
];
/// Edit tile sets per crack stage (walls, floors, ramps; `art/blender/assets/pieces.py`):
/// one named part per edit tile, plus the diagonal half tiles and trims.
pub(crate) const TILE_MODELS: [[&str; 3]; 3] = [
    [
        "wall_brick_tiles",
        "wall_brick_tiles_crack1",
        "wall_brick_tiles_crack2",
    ],
    [
        "floor_plank_tiles",
        "floor_plank_tiles_crack1",
        "floor_plank_tiles_crack2",
    ],
    [
        "ramp_plank_tiles",
        "ramp_plank_tiles_crack1",
        "ramp_plank_tiles_crack2",
    ],
];
/// An edited cone's four roof shapes per crack stage, in [`ConeRoof`] order.
pub(crate) const CONE_ROOF_MODELS: [[&str; 3]; 4] = [
    [
        "cone_plank_peak",
        "cone_plank_peak_crack1",
        "cone_plank_peak_crack2",
    ],
    [
        "cone_plank_slope",
        "cone_plank_slope_crack1",
        "cone_plank_slope_crack2",
    ],
    [
        "cone_plank_ridge",
        "cone_plank_ridge_crack1",
        "cone_plank_ridge_crack2",
    ],
    [
        "cone_plank_tent",
        "cone_plank_tent_crack1",
        "cone_plank_tent_crack2",
    ],
];
/// What a broken wall bursts into.
pub(crate) const BRICK_DEBRIS: &str = "brick_chunk";
/// What a broken floor or ramp bursts into.
pub(crate) const PLANK_DEBRIS: &str = "plank_splinter";

pub(crate) fn kind_index(kind: PieceKind) -> usize {
    match kind {
        PieceKind::Wall => 0,
        PieceKind::Floor => 1,
        PieceKind::Ramp => 2,
        PieceKind::Cone => 3,
    }
}

/// The model drawing a piece of `kind` at crack `stage`.
pub fn piece_model(kind: PieceKind, stage: u8) -> &'static str {
    PIECE_MODELS[kind_index(kind)][stage.min(2) as usize]
}

/// Where a piece's model origin sits in the piece's local space. Models stand
/// on the ground (their pivot), while a wall's piece transform is at the
/// wall's centre.
pub fn model_offset(kind: PieceKind) -> Vec3 {
    match kind {
        PieceKind::Wall => Vec3::NEG_Y * (LEVEL_HEIGHT / 2.0),
        PieceKind::Floor | PieceKind::Ramp | PieceKind::Cone => Vec3::ZERO,
    }
}

/// Merges every mesh in a loaded model's glTF scene into one mesh in model
/// space (node transforms and the +Z → -Z forward fix applied), ready to draw
/// on a plain entity: pieces of one kind then share this mesh and batch.
/// `None` if the scene has no loaded mesh.
pub fn model_mesh(scene: &WorldAsset, meshes: &Assets<Mesh>) -> Option<Mesh> {
    let world = &scene.world;
    let fix = Transform::from_rotation(MODEL_FORWARD_FIX);
    let mut parts: Vec<(Entity, Mesh)> = world
        .iter_entities()
        .filter_map(|e| {
            let mesh = meshes.get(&e.get::<Mesh3d>()?.0)?;
            // The node chain up to the scene root.
            let mut xf = e.get::<Transform>().copied().unwrap_or_default();
            let mut up = e.get::<ChildOf>().map(ChildOf::parent);
            while let Some(parent) = up {
                let p = world.get_entity(parent).ok()?;
                xf = p.get::<Transform>().copied().unwrap_or_default() * xf;
                up = p.get::<ChildOf>().map(ChildOf::parent);
            }
            Some((e.id(), mesh.clone().transformed_by(fix * xf)))
        })
        .collect();
    // A deterministic order whatever the scene's entity layout.
    parts.sort_by_key(|(e, _)| *e);
    let mut parts = parts.into_iter().map(|(_, m)| m);
    let mut merged = parts.next()?;
    for part in parts {
        merged.merge(&part).ok()?;
    }
    Some(merged)
}

/// Like [`model_mesh`], but one mesh per named part (glTF mesh name, which is
/// the Blender part name), each in model space.
pub fn model_part_meshes(scene: &WorldAsset, meshes: &Assets<Mesh>) -> BTreeMap<String, Mesh> {
    let world = &scene.world;
    let fix = Transform::from_rotation(MODEL_FORWARD_FIX);
    let mut parts: BTreeMap<String, Vec<(Entity, Mesh)>> = BTreeMap::new();
    for e in world.iter_entities() {
        let (Some(handle), Some(name)) = (e.get::<Mesh3d>(), e.get::<GltfMeshName>()) else {
            continue;
        };
        let Some(mesh) = meshes.get(&handle.0) else {
            continue;
        };
        let mut xf = e.get::<Transform>().copied().unwrap_or_default();
        let mut up = e.get::<ChildOf>().map(ChildOf::parent);
        while let Some(parent) = up {
            let Ok(p) = world.get_entity(parent) else {
                break;
            };
            xf = p.get::<Transform>().copied().unwrap_or_default() * xf;
            up = p.get::<ChildOf>().map(ChildOf::parent);
        }
        parts
            .entry(name.0.clone())
            .or_default()
            .push((e.id(), mesh.clone().transformed_by(fix * xf)));
    }
    parts
        .into_iter()
        .filter_map(|(name, mut list)| {
            list.sort_by_key(|(e, _)| *e);
            let mut it = list.into_iter().map(|(_, m)| m);
            let mut merged = it.next()?;
            for m in it {
                merged.merge(&m).ok()?;
            }
            Some((name, merged))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Edited pieces: composed from the tile sets once, at load
// ---------------------------------------------------------------------------

/// An edited cone's roof shape (the model), before its turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConeRoof {
    Peak,
    Slope,
    Ridge,
    Tent,
}

/// Turns a 2×2 tile index a quarter turn about +Y (counter-clockwise seen
/// from above): corner (x, z) goes to (z, -x).
fn turn_tile(t: u8) -> u8 {
    [2, 0, 3, 1][t as usize]
}

fn turn_edit(e: PieceEdit, k: u8) -> PieceEdit {
    let mut out = e;
    for _ in 0..k {
        let tiles = (0..4u8)
            .filter(|&t| out.has(t))
            .fold(0u16, |m, t| m | 1 << turn_tile(t));
        out = PieceEdit {
            tiles,
            start: turn_tile(out.start),
        };
    }
    out
}

/// The canonical shape (a model's edit) and the quarter turns about +Y that
/// carry it onto `edit`, for the 2×2 pieces drawn by turning one model.
fn find_turn(edit: PieceEdit, canon: &[PieceEdit]) -> Option<(usize, u8)> {
    for (i, c) in canon.iter().enumerate() {
        for k in 0..4 {
            let turned = turn_edit(*c, k);
            let same = turned.tiles == edit.tiles && (edit.start == turned.start || c.start == 0);
            if same {
                return Some((i, k));
            }
        }
    }
    None
}

/// Which cone roof model draws `edit`, and how far to turn it.
pub(crate) fn cone_roof(edit: PieceEdit) -> Option<(ConeRoof, Quat)> {
    let roof = match edit::shape_of(PieceKind::Cone, edit)? {
        EditShape::ConePeak => ConeRoof::Peak,
        EditShape::ConeSlope => ConeRoof::Slope,
        EditShape::ConeRidge => ConeRoof::Ridge,
        _ => ConeRoof::Tent,
    };
    let canon = match roof {
        ConeRoof::Peak => PieceEdit::of(&[0]),
        ConeRoof::Slope => PieceEdit::of(&[0, 1]),
        ConeRoof::Ridge => PieceEdit::of(&[0, 3]),
        ConeRoof::Tent => PieceEdit::of(&[0, 1, 2]),
    };
    let (_, k) = find_turn(edit, &[canon])?;
    Some((roof, quarter_turns(k)))
}

/// Which ramp half ("Left", "Right") draws a half ramp, and how far to turn it.
/// The model's halves rise toward -Z: "Left" is the path 2 → 0, "Right" 3 → 1.
pub(crate) fn ramp_half(edit: PieceEdit) -> Option<(&'static str, Quat)> {
    edit::half_ramp_points(edit)?;
    let (i, k) = find_turn(edit, &[PieceEdit::path(2, 0), PieceEdit::path(3, 1)])?;
    Some((["Left", "Right"][i], quarter_turns(k)))
}

fn quarter_turns(k: u8) -> Quat {
    Quat::from_rotation_y(k as f32 * std::f32::consts::FRAC_PI_2)
}

/// Removes triangles lying on a plane between two kept tiles (the caps each
/// tile has where it was cut from the full piece), so a composed piece has no
/// hidden inner faces to catch the ink outline.
fn strip_inner_caps(mesh: Mesh, is_inner: impl Fn([Vec3; 3]) -> bool) -> Mesh {
    let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return mesh;
    };
    let pos: Vec<Vec3> = pos.iter().map(|p| Vec3::from_array(*p)).collect();
    let indices: Vec<u32> = match mesh.indices() {
        Some(Indices::U16(v)) => v.iter().map(|&i| i as u32).collect(),
        Some(Indices::U32(v)) => v.clone(),
        None => (0..pos.len() as u32).collect(),
    };
    let kept: Vec<u32> = indices
        .chunks_exact(3)
        .filter(|t| !is_inner([0, 1, 2].map(|k| pos[t[k] as usize])))
        .flatten()
        .copied()
        .collect();
    let mut mesh = mesh;
    mesh.insert_indices(Indices::U32(kept));
    mesh
}

fn part<'a>(parts: &'a BTreeMap<String, Mesh>, name: &str) -> Option<&'a Mesh> {
    let found = parts.get(name);
    if found.is_none() {
        error!("building: edit tile part {name} is missing");
    }
    found
}

fn merge_all(pieces: Vec<Mesh>) -> Option<Mesh> {
    let mut it = pieces.into_iter();
    let mut merged = it.next()?;
    for m in it {
        merged.merge(&m).ok()?;
    }
    Some(merged)
}

/// An edited wall in model space (standing on its pivot, like `wall_brick`):
/// its kept tiles (or a triangle's tiles and half tiles), with frame boards
/// along every edge of the opening.
pub(crate) fn compose_wall(edit: PieceEdit, parts: &BTreeMap<String, Mesh>) -> Option<Mesh> {
    const EPS: f32 = 2e-3;
    let removed = edit.tiles;
    let kept = |t: u8| removed & (1 << t) == 0;
    let center = |t: u8| {
        let (c, r) = ((t % 3) as f32, (t / 3) as f32);
        Vec2::new(
            -CELL_SIZE / 2.0 + (c + 0.5) * WALL_TILE_W,
            LEVEL_HEIGHT - (r + 0.5) * WALL_TILE_H,
        )
    };
    let mut pieces = Vec::new();
    // A triangle: whole tiles on the kept side, halves along the diagonal.
    let triangle: Option<(&str, [u8; 3], &str, f32)> = match edit::wall_triangle(edit) {
        Some(_) if removed == PieceEdit::of(&[0, 1, 3]).tiles => {
            Some(("A", [2, 4, 6], "Lo", 1.0))
        }
        Some(_) if removed == PieceEdit::of(&[5, 7, 8]).tiles => {
            Some(("A", [2, 4, 6], "Hi", 1.0))
        }
        Some(_) if removed == PieceEdit::of(&[1, 2, 5]).tiles => {
            Some(("B", [0, 4, 8], "Lo", -1.0))
        }
        Some(_) => Some(("B", [0, 4, 8], "Hi", -1.0)),
        None => None,
    };
    let split: &[u8] = triangle.as_ref().map_or(&[], |t| &t.1);
    // Tiles that count as kept for seams (split tiles meet their neighbours
    // along whole edges on the kept side).
    let solid = |t: u8| kept(t) || split.contains(&t);
    for t in (0..9u8).filter(|&t| kept(t) && !split.contains(&t)) {
        pieces.push(part(parts, &format!("T{t}"))?.clone());
    }
    if let Some((diag, tiles, side, sign)) = triangle {
        for t in tiles {
            pieces.push(part(parts, &format!("{diag}{t}{side}"))?.clone());
        }
        let angle = sign * (LEVEL_HEIGHT / CELL_SIZE).atan();
        let xf = Transform::from_xyz(0.0, LEVEL_HEIGHT / 2.0, 0.0)
            .with_rotation(Quat::from_rotation_z(angle));
        pieces.push(part(parts, "TrimD")?.clone().transformed_by(xf));
    } else {
        // Frame boards along every edge between a kept tile and a removed one.
        for t in (0..9u8).filter(|&t| kept(t)) {
            let (c, r) = (t % 3, t / 3);
            let p = center(t);
            let mut board = |name: &str, at: Vec2, vertical: bool| -> Option<()> {
                let rot = if vertical {
                    Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)
                } else {
                    Quat::IDENTITY
                };
                let xf = Transform::from_translation(at.extend(0.0)).with_rotation(rot);
                pieces.push(part(parts, name)?.clone().transformed_by(xf));
                Some(())
            };
            if r > 0 && !kept(t - 3) {
                board("TrimH", p + Vec2::Y * WALL_TILE_H / 2.0, false)?;
            }
            if r < 2 && !kept(t + 3) {
                board("TrimH", p - Vec2::Y * WALL_TILE_H / 2.0, false)?;
            }
            if c > 0 && !kept(t - 1) {
                board("TrimV", p - Vec2::X * WALL_TILE_W / 2.0, true)?;
            }
            if c < 2 && !kept(t + 1) {
                board("TrimV", p + Vec2::X * WALL_TILE_W / 2.0, true)?;
            }
        }
    }
    let merged = merge_all(pieces)?;
    let tile_at = |x: f32, y: f32| -> u8 {
        let c = ((x + CELL_SIZE / 2.0) / WALL_TILE_W).floor().clamp(0.0, 2.0) as u8;
        let r = ((LEVEL_HEIGHT - y) / WALL_TILE_H).floor().clamp(0.0, 2.0) as u8;
        r * 3 + c
    };
    Some(strip_inner_caps(merged, |tri| {
        let mid = (tri[0] + tri[1] + tri[2]) / 3.0;
        for xb in [-CELL_SIZE / 2.0 + WALL_TILE_W, CELL_SIZE / 2.0 - WALL_TILE_W] {
            if tri.iter().all(|p| (p.x - xb).abs() < EPS) {
                let (a, b) = (tile_at(xb - 0.1, mid.y), tile_at(xb + 0.1, mid.y));
                return solid(a) && solid(b);
            }
        }
        for yb in [WALL_TILE_H, 2.0 * WALL_TILE_H] {
            if tri.iter().all(|p| (p.y - yb).abs() < EPS) {
                let (a, b) = (tile_at(mid.x, yb - 0.1), tile_at(mid.x, yb + 0.1));
                return solid(a) && solid(b);
            }
        }
        false
    }))
}

/// An edited floor in model space: its kept quarters, with a beam along every
/// edge between a kept quarter and a removed one.
pub(crate) fn compose_floor(edit: PieceEdit, parts: &BTreeMap<String, Mesh>) -> Option<Mesh> {
    const EPS: f32 = 2e-3;
    let kept = |t: u8| !edit.has(t);
    let mut pieces = Vec::new();
    for t in (0..4u8).filter(|&t| kept(t)) {
        pieces.push(part(parts, &format!("Q{t}"))?.clone());
    }
    let half = CELL_SIZE / 4.0;
    for t in (0..4u8).filter(|&t| kept(t)) {
        let (c, r) = (t % 2, t / 2);
        let x = if c == 0 { -half } else { half };
        let z = if r == 0 { -half } else { half };
        // The neighbour across x = 0 and across z = 0.
        if !kept(t ^ 1) {
            let xf = Transform::from_xyz(0.0, 0.0, z)
                .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2));
            pieces.push(part(parts, "Trim")?.clone().transformed_by(xf));
        }
        if !kept(t ^ 2) {
            pieces.push(
                part(parts, "Trim")?
                    .clone()
                    .transformed_by(Transform::from_xyz(x, 0.0, 0.0)),
            );
        }
    }
    let merged = merge_all(pieces)?;
    let quarter = |x: f32, z: f32| (x >= 0.0) as u8 + 2 * (z >= 0.0) as u8;
    Some(strip_inner_caps(merged, |tri| {
        let mid = (tri[0] + tri[1] + tri[2]) / 3.0;
        if tri.iter().all(|p| p.x.abs() < EPS) {
            return kept(quarter(-0.1, mid.z)) && kept(quarter(0.1, mid.z));
        }
        if tri.iter().all(|p| p.z.abs() < EPS) {
            return kept(quarter(mid.x, -0.1)) && kept(quarter(mid.x, 0.1));
        }
        false
    }))
}

// ---------------------------------------------------------------------------
// Ghost previews: the piece's volume, slightly enlarged, with bright edges and
// faint brick or plank lines, drawn translucent and glowing.
// ---------------------------------------------------------------------------

const HALF_W: f32 = CELL_SIZE / 2.0;
const HALF_H: f32 = LEVEL_HEIGHT / 2.0;
const GHOST_FACE_ALPHA: f32 = 0.26;
const GHOST_LINE_ALPHA: f32 = 0.6;
const GHOST_EDGE_ALPHA: f32 = 0.9;
const GHOST_EDGE: f32 = 0.035;
const GHOST_LINE: f32 = 0.022;
/// The brick wall model's courses and mortar joints (pieces.py), for the
/// ghost's lines.
const COURSES: usize = 8;
const MORTAR: f32 = 0.055;

/// Accumulates flat polygons with per-vertex colours (white; the ghost
/// material tints them) and alpha.
#[derive(Default)]
pub(crate) struct MeshBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl MeshBuilder {
    #[cfg(test)]
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    /// A convex polygon, wound so its normal points away from `inside`.
    pub fn poly(&mut self, points: &[Vec3], inside: Vec3, alpha: f32) {
        if points.len() < 3 {
            return;
        }
        // Newell's method: a robust normal for any planar polygon.
        let mut n = Vec3::ZERO;
        for (i, a) in points.iter().enumerate() {
            let b = points[(i + 1) % points.len()];
            n += Vec3::new(
                (a.y - b.y) * (a.z + b.z),
                (a.z - b.z) * (a.x + b.x),
                (a.x - b.x) * (a.y + b.y),
            );
        }
        let centroid = points.iter().copied().sum::<Vec3>() / points.len() as f32;
        let flip = n.dot(centroid - inside) < 0.0;
        let normal = if flip { -n } else { n }.normalize_or_zero();
        let start = self.positions.len() as u32;
        let count = points.len();
        for k in 0..count {
            let p = if flip {
                points[count - 1 - k]
            } else {
                points[k]
            };
            self.positions.push(p.to_array());
            self.normals.push(normal.to_array());
            self.colors.push([1.0, 1.0, 1.0, alpha]);
        }
        for k in 1..count as u32 - 1 {
            self.indices.extend([start, start + k, start + k + 1]);
        }
    }

    /// A thin bar of square section `2r` from `a` to `b`.
    pub fn bar(&mut self, a: Vec3, b: Vec3, r: f32, alpha: f32) {
        let d = b - a;
        let len = d.length();
        if len < 1e-4 {
            return;
        }
        let z = d / len;
        let x = z.any_orthonormal_vector();
        let y = z.cross(x);
        let xf = Affine3A::from_mat3_translation(Mat3::from_cols(x, y, z), a);
        let c = Vec3::new(0.0, 0.0, len / 2.0);
        let h = Vec3::new(r, r, len / 2.0 + r);
        for axis in 0..3 {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            for s in [-1.0, 1.0] {
                let pts = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(su, sv)| {
                    let mut p = Vec3::ZERO;
                    p[axis] = s * h[axis];
                    p[u] = su * h[u];
                    p[v] = sv * h[v];
                    xf.transform_point3(c + p)
                });
                self.poly(&pts, xf.transform_point3(c), alpha);
            }
        }
    }

    /// A flat line (a thin quad) from `a` to `b` on a face with outward normal
    /// `n`, lifted a hair off it.
    pub fn line(&mut self, a: Vec3, b: Vec3, n: Vec3, alpha: f32) {
        let d = (b - a).normalize_or_zero();
        let side = n.cross(d) * GHOST_LINE;
        let lift = n * 0.006;
        let pts = [
            a - side + lift,
            b - side + lift,
            b + side + lift,
            a + side + lift,
        ];
        self.poly(&pts, a - n, alpha);
    }

    pub fn build(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

fn ghost_convex(m: &mut MeshBuilder, corners: &[Vec3], faces: &[&[usize]]) {
    let center = corners.iter().copied().sum::<Vec3>() / corners.len() as f32;
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for face in faces {
        let pts: Vec<Vec3> = face.iter().map(|&i| corners[i]).collect();
        m.poly(&pts, center, GHOST_FACE_ALPHA);
        for k in 0..face.len() {
            let (a, b) = (face[k], face[(k + 1) % face.len()]);
            let e = (a.min(b), a.max(b));
            if !edges.contains(&e) {
                edges.push(e);
            }
        }
    }
    for (a, b) in edges {
        m.bar(corners[a], corners[b], GHOST_EDGE, GHOST_EDGE_ALPHA);
    }
}

fn ghost_box(half: Vec3) -> MeshBuilder {
    let mut m = MeshBuilder::default();
    let corners: Vec<Vec3> = (0..8)
        .map(|i| {
            half * Vec3::new(
                if i & 1 == 0 { -1.0 } else { 1.0 },
                if i & 2 == 0 { -1.0 } else { 1.0 },
                if i & 4 == 0 { -1.0 } else { 1.0 },
            )
        })
        .collect();
    let faces: [&[usize]; 6] = [
        &[0, 1, 3, 2],
        &[4, 5, 7, 6],
        &[0, 1, 5, 4],
        &[2, 3, 7, 6],
        &[0, 2, 6, 4],
        &[1, 3, 7, 5],
    ];
    ghost_convex(&mut m, &corners, &faces);
    m
}

/// The wall ghost: a glowing slab with the brick wall's courses and joints.
pub(crate) fn ghost_wall_mesh(thickness: f32) -> MeshBuilder {
    let half = Vec3::new(HALF_W + 0.02, HALF_H + 0.02, thickness / 2.0 + 0.06);
    let mut m = ghost_box(half);
    let pitch_y = (LEVEL_HEIGHT + MORTAR) / COURSES as f32;
    let pitch_x = (CELL_SIZE + MORTAR) / 5.0;
    for s in [-1.0f32, 1.0] {
        let n = Vec3::Z * s;
        let z = half.z * s;
        for c in 0..COURSES {
            let y0 = -HALF_H + c as f32 * pitch_y - MORTAR / 2.0;
            if c > 0 {
                let (a, b) = (Vec3::new(-HALF_W, y0, z), Vec3::new(HALF_W, y0, z));
                m.line(a, b, n, GHOST_LINE_ALPHA);
            }
            let (lo, hi) = (y0.max(-HALF_H), (y0 + pitch_y).min(HALF_H));
            let first = if c % 2 == 0 { pitch_x } else { pitch_x / 2.0 };
            let mut x = -HALF_W + first - MORTAR / 2.0;
            while x < HALF_W - 0.1 {
                m.line(
                    Vec3::new(x, lo, z),
                    Vec3::new(x, hi, z),
                    n,
                    GHOST_LINE_ALPHA,
                );
                x += pitch_x;
            }
        }
    }
    m
}

/// The floor ghost: a glowing slab with the floor's plank lines.
pub(crate) fn ghost_floor_mesh(thickness: f32) -> MeshBuilder {
    let half = Vec3::new(HALF_W + 0.02, thickness / 2.0 + 0.04, HALF_W + 0.02);
    let mut m = ghost_box(half);
    let pitch = CELL_SIZE / 5.0;
    for s in [-1.0f32, 1.0] {
        let n = Vec3::Y * s;
        let y = half.y * s;
        for k in 1..5 {
            let x = -HALF_W + k as f32 * pitch;
            let (a, b) = (Vec3::new(x, y, -HALF_W), Vec3::new(x, y, HALF_W));
            m.line(a, b, n, GHOST_LINE_ALPHA);
        }
    }
    m
}

/// The ramp ghost: a glowing wedge with the ramp's plank lines across its slope.
pub(crate) fn ghost_ramp_mesh() -> MeshBuilder {
    let mut m = MeshBuilder::default();
    let w = HALF_W + 0.02;
    let top = LEVEL_HEIGHT + 0.04;
    let corners = [
        Vec3::new(-w, -0.02, w),
        Vec3::new(w, -0.02, w),
        Vec3::new(-w, -0.02, -w),
        Vec3::new(w, -0.02, -w),
        Vec3::new(-w, top, -w),
        Vec3::new(w, top, -w),
    ];
    let faces: [&[usize]; 5] = [
        &[0, 1, 3, 2],
        &[0, 1, 5, 4],
        &[2, 3, 5, 4],
        &[0, 2, 4],
        &[1, 3, 5],
    ];
    ghost_convex(&mut m, &corners, &faces);
    // Plank lines across the slope face, from its low edge (0-1) to its top (4-5).
    let n = Vec3::new(0.0, CELL_SIZE, LEVEL_HEIGHT).normalize();
    for k in 1..6 {
        let t = k as f32 / 6.0;
        let (a, b) = (
            corners[0].lerp(corners[4], t),
            corners[1].lerp(corners[5], t),
        );
        m.line(a, b, n, GHOST_LINE_ALPHA);
    }
    m
}

/// The cone ghost: a glowing pyramid with the cone's plank lines on each face.
pub(crate) fn ghost_cone_mesh() -> MeshBuilder {
    let mut m = MeshBuilder::default();
    let w = HALF_W + 0.02;
    let top = super::grid::CONE_HEIGHT + 0.04;
    let corners = [
        Vec3::new(-w, -0.02, -w),
        Vec3::new(w, -0.02, -w),
        Vec3::new(w, -0.02, w),
        Vec3::new(-w, -0.02, w),
        Vec3::new(0.0, top, 0.0),
    ];
    let faces: [&[usize]; 5] = [&[0, 1, 2, 3], &[0, 1, 4], &[1, 2, 4], &[2, 3, 4], &[3, 0, 4]];
    ghost_convex(&mut m, &corners, &faces);
    let center = Vec3::Y * 0.3;
    for k in 0..4 {
        let (a, b, apex) = (corners[k], corners[(k + 1) % 4], corners[4]);
        let n = (b - a).cross(apex - a).normalize();
        let n = if n.dot((a + b + apex) / 3.0 - center) < 0.0 { -n } else { n };
        for j in 1..4 {
            let t = j as f32 / 4.0;
            m.line(a.lerp(apex, t), b.lerp(apex, t), n, GHOST_LINE_ALPHA);
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_piece_and_stage_has_a_model() {
        for kind in KINDS {
            let names: Vec<&str> = (0..3).map(|s| piece_model(kind, s)).collect();
            assert!(names[1].ends_with("_crack1") && names[2].ends_with("_crack2"));
            assert!(names[1].starts_with(names[0]));
            // Deeper stages than 2 show stage 2.
            assert_eq!(piece_model(kind, 7), names[2]);
            for name in names.iter().chain([&BRICK_DEBRIS, &PLANK_DEBRIS]) {
                assert!(
                    crate::models::EMBEDDED_MODELS
                        .iter()
                        .any(|m| m.name == *name),
                    "{name} is not embedded"
                );
            }
        }
        assert_eq!(model_offset(PieceKind::Wall).y, -1.5);
    }

    /// The turned model covers exactly the edit's shape (the collider's hull).
    #[test]
    fn turned_half_ramps_and_cone_roofs_match_their_edit() {
        let same = |a: &[Vec3], b: &[Vec3]| {
            a.iter().all(|p| b.iter().any(|q| p.distance(*q) < 1e-4))
                && b.iter().all(|q| a.iter().any(|p| p.distance(*q) < 1e-4))
        };
        for e in edit::valid_edits(PieceKind::Ramp) {
            let (side, turn) = ramp_half(e).unwrap();
            let canon = if side == "Left" {
                PieceEdit::path(2, 0)
            } else {
                PieceEdit::path(3, 1)
            };
            let turned: Vec<Vec3> = edit::half_ramp_points(canon)
                .unwrap()
                .iter()
                .map(|p| turn * *p)
                .collect();
            assert!(same(&turned, &edit::half_ramp_points(e).unwrap()), "{e:?}");
        }
        for e in edit::valid_edits(PieceKind::Cone) {
            let (roof, turn) = cone_roof(e).unwrap();
            let canon = match roof {
                ConeRoof::Peak => PieceEdit::of(&[0]),
                ConeRoof::Slope => PieceEdit::of(&[0, 1]),
                ConeRoof::Ridge => PieceEdit::of(&[0, 3]),
                ConeRoof::Tent => PieceEdit::of(&[0, 1, 2]),
            };
            let turned: Vec<Vec3> = edit::cone_points(canon).iter().map(|p| turn * *p).collect();
            assert!(same(&turned, &edit::cone_points(e)), "{e:?}");
        }
        assert_eq!(ramp_half(PieceEdit::path(2, 0)), Some(("Left", Quat::IDENTITY)));
    }

    #[test]
    fn ghosts_cover_their_piece_and_carry_pattern_lines() {
        for (name, m, lo, hi) in [
            (
                "wall",
                ghost_wall_mesh(0.2),
                Vec3::new(-2.1, -1.6, -0.2),
                Vec3::new(2.1, 1.6, 0.2),
            ),
            (
                "floor",
                ghost_floor_mesh(0.2),
                Vec3::new(-2.1, -0.2, -2.1),
                Vec3::new(2.1, 0.2, 2.1),
            ),
            (
                "ramp",
                ghost_ramp_mesh(),
                Vec3::new(-2.1, -0.1, -2.1),
                Vec3::new(2.1, 3.1, 2.1),
            ),
            (
                "cone",
                ghost_cone_mesh(),
                Vec3::new(-2.1, -0.1, -2.1),
                Vec3::new(2.1, 1.6, 2.1),
            ),
        ] {
            assert!(m.triangles() > 60, "{name}: {} triangles", m.triangles());
            for p in &m.positions {
                let p = Vec3::from_array(*p);
                assert!(p.cmpge(lo).all() && p.cmple(hi).all(), "{name}: {p}");
            }
            // Faint faces, brighter pattern lines, the brightest edges.
            let alphas: Vec<f32> = m.colors.iter().map(|c| c[3]).collect();
            for a in [GHOST_FACE_ALPHA, GHOST_LINE_ALPHA, GHOST_EDGE_ALPHA] {
                assert!(alphas.contains(&a), "{name}: no alpha {a}");
            }
            assert!(
                m.normals
                    .iter()
                    .all(|n| (Vec3::from_array(*n).length() - 1.0).abs() < 1e-3)
            );
        }
    }
}
