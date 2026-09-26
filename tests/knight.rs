//! The knight (docs/M2-SPEC.md → The knight; gate S5): the model the training
//! dummy wears. It has every named part, joint pivot and eye state the procedural
//! animation needs, stays inside its triangle budget, and fits the fixed gameplay
//! hitboxes of `player.rs` (body capsule r 0.33 m from 0.05 to 1.45 m, head sphere
//! r 0.20 m centred at 1.62 m). The model is made to fit them, never the reverse.
//!
//! - **Inside:** every body part lies inside the body capsule, and the helmet, hat
//!   and eyes inside the head sphere, each within [`TOLERANCE`] (5 cm). Checked on
//!   the sidecar's part bounds, then exactly on every vertex of the glb. Boots stand
//!   on the ground, where the capsule's rounded end can't hold two feet, so below
//!   [`FOOT_BAND`] the capsule counts as a cylinder (as for Milestone 1's figure).
//! - **Filled:** neither hitbox sticks out more than about 10 cm ([`FILL`]) past the
//!   model: the parts' bounds reach every side of both hitboxes, and seen from the
//!   front (the dummy always turns to face the player) no point of either hitbox is
//!   more than 10 cm from the model's silhouette. Seen from the side, the small
//!   torso leaves the capsule's front emptier; that view is held to [`SIDE_FILL`]
//!   so it can't get worse unnoticed.
//!
//! Model space is the character's frame: feet at the origin, +Y up, -Z forward,
//! +X to the knight's right.

use bevy::{math::Affine3A, prelude::*};
use pieced::{
    models::{Bounds, Sidecar, gltf_to_model},
    player::{BODY_BOTTOM, BODY_RADIUS, BODY_TOP, HEAD_CENTER, HEAD_RADIUS},
};
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::PathBuf};

/// How far a part may poke out of its hitbox.
const TOLERANCE: f32 = 0.05;
/// How far a hitbox may stick out past the model.
const FILL: f32 = 0.10;
/// The side view's limit (see the module docs).
const SIDE_FILL: f32 = 0.21;
/// Below this height the body capsule counts as a cylinder (boots on the ground).
const FOOT_BAND: f32 = 0.12;
/// Silhouette raster cell size.
const CELL: f32 = 0.01;

const PARTS: [&str; 16] = [
    "BootL",
    "BootR",
    "Cape",
    "EyeBlinkL",
    "EyeBlinkR",
    "EyeL",
    "EyeR",
    "EyeWideL",
    "EyeWideR",
    "EyeXL",
    "EyeXR",
    "GauntletL",
    "GauntletR",
    "Hat",
    "Helmet",
    "Torso",
];

/// Every part and pivot with its parent node: the hierarchy procedural animation
/// relies on (rotate a pivot, its part follows; bob the torso, the upper body follows).
const PARENTS: [(&str, &str); 23] = [
    ("PivotLegL", "knight"),
    ("PivotLegR", "knight"),
    ("BootL", "PivotLegL"),
    ("BootR", "PivotLegR"),
    ("Torso", "knight"),
    ("PivotArmL", "Torso"),
    ("PivotArmR", "Torso"),
    ("GauntletL", "PivotArmL"),
    ("GauntletR", "PivotArmR"),
    ("PivotCape", "Torso"),
    ("Cape", "PivotCape"),
    ("PivotHead", "Torso"),
    ("Helmet", "PivotHead"),
    ("PivotHat", "PivotHead"),
    ("Hat", "PivotHat"),
    ("EyeL", "Helmet"),
    ("EyeR", "Helmet"),
    ("EyeWideL", "Helmet"),
    ("EyeWideR", "Helmet"),
    ("EyeBlinkL", "Helmet"),
    ("EyeBlinkR", "Helmet"),
    ("EyeXL", "Helmet"),
    ("EyeXR", "Helmet"),
];

fn models_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/models")
}

fn sidecar() -> Sidecar {
    Sidecar::parse(&fs::read_to_string(models_dir().join("knight.json")).unwrap()).unwrap()
}

fn is_head(part: &str) -> bool {
    part == "Helmet" || part == "Hat" || part.starts_with("Eye")
}

/// Eye states other than open are hidden in play until needed.
fn starts_hidden(part: &str) -> bool {
    ["EyeWide", "EyeBlink", "EyeX"]
        .iter()
        .any(|p| part.strip_prefix(p).is_some_and(|s| s == "L" || s == "R"))
}

/// How far a point lies outside the body capsule (negative inside).
fn capsule_overshoot(p: Vec3) -> f32 {
    let radial = p.xz().length();
    if p.y < FOOT_BAND {
        return radial - BODY_RADIUS;
    }
    let y = p.y.clamp(BODY_BOTTOM + BODY_RADIUS, BODY_TOP - BODY_RADIUS);
    Vec2::new(radial, p.y - y).length() - BODY_RADIUS
}

fn sphere_overshoot(p: Vec3) -> f32 {
    p.distance(Vec3::Y * HEAD_CENTER) - HEAD_RADIUS
}

fn union(bounds: impl Iterator<Item = Bounds>) -> (Vec3, Vec3) {
    bounds.fold((Vec3::MAX, Vec3::MIN), |(lo, hi), b| {
        (lo.min(b.min()), hi.max(b.max()))
    })
}

// ---------------------------------------------------------------------------
// Parts, pivots, eyes, budget
// ---------------------------------------------------------------------------

#[test]
fn knight_has_its_parts_pivots_and_eye_states() {
    let side = sidecar();
    assert_eq!(side.kind, "knight");
    assert!(
        side.triangles <= 8000 && side.triangle_budget == 8000,
        "{} triangles (budget {})",
        side.triangles,
        side.triangle_budget
    );
    let parts: Vec<&str> = side.parts.keys().map(String::as_str).collect();
    assert_eq!(parts, PARTS, "the knight's named parts");
    for (node, parent) in PARENTS {
        let actual = side
            .part(node)
            .map(|p| p.parent.as_deref())
            .or_else(|| side.attach(node).map(|a| a.parent.as_deref()))
            .unwrap_or_else(|| panic!("{node} is missing"));
        assert_eq!(actual, Some(parent), "{node}'s parent");
    }

    // L is the knight's own left (model -X), mirrored by R.
    for (l, r) in [
        ("BootL", "BootR"),
        ("GauntletL", "GauntletR"),
        ("EyeL", "EyeR"),
    ] {
        let (cl, cr) = (side.parts[l].bounds.center(), side.parts[r].bounds.center());
        assert!(cl.x < -0.02 && cr.x > 0.02, "{l} {cl} / {r} {cr}");
        assert!((cl - cr * Vec3::new(-1.0, 1.0, 1.0)).length() < 0.005);
    }
    for (l, r) in [("PivotLegL", "PivotLegR"), ("PivotArmL", "PivotArmR")] {
        let (pl, pr) = (side.attach[l].position(), side.attach[r].position());
        assert!(pl.x < 0.0 && (pl - pr * Vec3::new(-1.0, 1.0, 1.0)).length() < 1e-3);
    }

    // Pivots sit at the joints they turn.
    let at = |n: &str| side.attach[n].position();
    assert!(
        (0.5..0.75).contains(&at("PivotLegL").y),
        "hip {}",
        at("PivotLegL")
    );
    assert!(
        (1.1..1.3).contains(&at("PivotArmL").y),
        "shoulder {}",
        at("PivotArmL")
    );
    assert!((0.15..0.25).contains(&-at("PivotArmL").x));
    let head = at("PivotHead");
    assert!(
        head.x.abs() < 1e-4 && (1.35..1.46).contains(&head.y),
        "neck {head}"
    );
    assert!(at("PivotCape").z > 0.05, "the cape hangs from the back");
    let helmet = side.parts["Helmet"].bounds;
    let hat = at("PivotHat");
    assert!(
        hat.y > helmet.center().y && hat.y < helmet.max[1] + 0.01,
        "hat pivot {hat} on the helmet's top"
    );

    // Each eye state sits where the open eye does, in the visor, on the front.
    for s in ["L", "R"] {
        let open = side.parts[&format!("Eye{s}")].bounds;
        assert!(open.center().z < -0.1, "eyes look out the front");
        assert!(
            open.min().cmpge(helmet.min()).all() && open.max().cmple(helmet.max()).all(),
            "Eye{s} inside the helmet"
        );
        for state in ["EyeWide", "EyeBlink", "EyeX"] {
            // Same place face-on; lines may sit a little in front of the eyeball.
            let b = side.parts[&format!("{state}{s}")].bounds;
            assert!(
                b.center().xy().distance(open.center().xy()) < 0.01
                    && (b.center().z - open.center().z).abs() < 0.03,
                "{state}{s} at {} vs open {}",
                b.center(),
                open.center()
            );
        }
    }
    assert!(
        side.parts["Hat"].bounds.min[1] > side.parts["EyeL"].bounds.center().y,
        "the hat sits above the eyes"
    );

    // Palette: steel helmet, purple hat and cape, a gold star, white eyes with pupils.
    let colors = |p: &str| side.parts[p].colors.clone();
    assert!(colors("Helmet").contains(&"knight_steel".to_string()));
    for (part, color) in [
        ("Hat", "knight_purple"),
        ("Hat", "star_gold"),
        ("Cape", "knight_purple"),
        ("Torso", "knight_purple"),
        ("Torso", "star_gold"),
        ("GauntletL", "knight_steel"),
        ("BootR", "knight_steel"),
        ("EyeL", "eye_white"),
        ("EyeL", "pupil_black"),
        ("EyeXR", "eye_white"),
    ] {
        assert!(colors(part).contains(&color.to_string()), "{part}: {color}");
    }
}

// ---------------------------------------------------------------------------
// Hitbox fit on the sidecar bounds (gate S5)
// ---------------------------------------------------------------------------

#[test]
fn knight_part_bounds_fit_the_hitboxes() {
    let side = sidecar();
    let r = BODY_RADIUS;
    let capsule = (Vec3::new(-r, BODY_BOTTOM, -r), Vec3::new(r, BODY_TOP, r));
    let hr = HEAD_RADIUS;
    let sphere = (
        Vec3::new(-hr, HEAD_CENTER - hr, -hr),
        Vec3::new(hr, HEAD_CENTER + hr, hr),
    );
    for (name, part) in &side.parts {
        let (lo, hi) = if is_head(name) { sphere } else { capsule };
        let out = (lo - part.bounds.min())
            .max(part.bounds.max() - hi)
            .max_element();
        println!(
            "bounds {name:10} {} overshoot {:+.1} cm",
            if is_head(name) { "head" } else { "body" },
            out * 100.0
        );
        assert!(
            out <= TOLERANCE,
            "{name}'s bounds poke {out:.3} m out of its hitbox's bounds"
        );
    }
    // The bounds reach every side of both hitboxes.
    let body = union(
        side.parts
            .iter()
            .filter(|(n, _)| !is_head(n))
            .map(|(_, p)| p.bounds),
    );
    let head = union(
        side.parts
            .iter()
            .filter(|(n, _)| is_head(n))
            .map(|(_, p)| p.bounds),
    );
    for (what, (lo, hi), (blo, bhi)) in [("body", capsule, body), ("head", sphere, head)] {
        let gap = (blo - lo).max(hi - bhi).max_element();
        println!("bounds fill {what}: the hitbox's bounds stick out {gap:.3} m at most");
        assert!(
            gap <= FILL,
            "the {what} hitbox's bounds stick out {gap:.3} m past the parts"
        );
    }
}

// ---------------------------------------------------------------------------
// The glb itself: every vertex, and the silhouettes
// ---------------------------------------------------------------------------

struct Glb {
    doc: Value,
    bin: Vec<u8>,
}

fn load_glb() -> Glb {
    let bytes = fs::read(models_dir().join("knight.glb")).unwrap();
    assert_eq!(&bytes[0..4], b"glTF");
    let u32_at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
    let json_len = u32_at(12);
    assert_eq!(&bytes[16..20], b"JSON");
    let doc = serde_json::from_slice(&bytes[20..20 + json_len]).unwrap();
    let off = 20 + json_len;
    let bin_len = u32_at(off);
    assert_eq!(&bytes[off + 4..off + 8], b"BIN\0");
    Glb {
        doc,
        bin: bytes[off + 8..off + 8 + bin_len].to_vec(),
    }
}

impl Glb {
    /// (element count, byte offset, stride, component type) of an accessor.
    fn layout(&self, accessor: usize, elem: usize) -> (usize, usize, usize, u64) {
        let acc = &self.doc["accessors"][accessor];
        let view = &self.doc["bufferViews"][acc["bufferView"].as_u64().unwrap() as usize];
        let offset = view["byteOffset"].as_u64().unwrap_or(0) as usize
            + acc["byteOffset"].as_u64().unwrap_or(0) as usize;
        let stride = view["byteStride"].as_u64().map_or(elem, |s| s as usize);
        (
            acc["count"].as_u64().unwrap() as usize,
            offset,
            stride,
            acc["componentType"].as_u64().unwrap(),
        )
    }

    fn vec3s(&self, accessor: usize) -> Vec<Vec3> {
        let (count, offset, stride, kind) = self.layout(accessor, 12);
        assert_eq!(kind, 5126, "positions are f32");
        let f = |i: usize| f32::from_le_bytes(self.bin[i..i + 4].try_into().unwrap());
        (0..count)
            .map(|k| {
                let i = offset + k * stride;
                Vec3::new(f(i), f(i + 4), f(i + 8))
            })
            .collect()
    }

    fn indices(&self, accessor: usize) -> Vec<usize> {
        let size = match self.doc["accessors"][accessor]["componentType"].as_u64() {
            Some(5121) => 1,
            Some(5123) => 2,
            _ => 4,
        };
        let (count, offset, stride, _) = self.layout(accessor, size);
        (0..count)
            .map(|k| {
                let i = offset + k * stride;
                match size {
                    1 => self.bin[i] as usize,
                    2 => u16::from_le_bytes(self.bin[i..i + 2].try_into().unwrap()) as usize,
                    _ => u32::from_le_bytes(self.bin[i..i + 4].try_into().unwrap()) as usize,
                }
            })
            .collect()
    }
}

/// One part's mesh in model space.
struct PartMesh {
    verts: Vec<Vec3>,
    tris: Vec<[usize; 3]>,
}

/// Every named mesh part of the glb, in model space (the spawn helper's forward fix applied).
fn part_meshes() -> BTreeMap<String, PartMesh> {
    let glb = load_glb();
    let nodes = glb.doc["nodes"].as_array().unwrap();
    let floats = |v: &Value, n: usize| -> Vec<f32> {
        (0..n).map(|i| v[i].as_f64().unwrap() as f32).collect()
    };
    let local = |n: &Value| {
        let t = n
            .get("translation")
            .map_or(Vec3::ZERO, |v| Vec3::from_slice(&floats(v, 3)));
        let s = n
            .get("scale")
            .map_or(Vec3::ONE, |v| Vec3::from_slice(&floats(v, 3)));
        let r = n
            .get("rotation")
            .map_or(Quat::IDENTITY, |v| Quat::from_slice(&floats(v, 4)));
        Affine3A::from_scale_rotation_translation(s, r, t)
    };
    let mut out = BTreeMap::new();
    let mut stack: Vec<(usize, Affine3A)> = glb.doc["scenes"][0]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| (i.as_u64().unwrap() as usize, Affine3A::IDENTITY))
        .collect();
    while let Some((i, parent)) = stack.pop() {
        let node = &nodes[i];
        let world = parent * local(node);
        if let (Some(name), Some(mesh)) = (node["name"].as_str(), node["mesh"].as_u64()) {
            let mut part = PartMesh {
                verts: Vec::new(),
                tris: Vec::new(),
            };
            for prim in glb.doc["meshes"][mesh as usize]["primitives"]
                .as_array()
                .unwrap()
            {
                let base = part.verts.len();
                let pos = prim["attributes"]["POSITION"].as_u64().unwrap() as usize;
                part.verts.extend(
                    glb.vec3s(pos)
                        .into_iter()
                        .map(|p| gltf_to_model(world.transform_point3(p))),
                );
                let idx = glb.indices(prim["indices"].as_u64().unwrap() as usize);
                part.tris.extend(
                    idx.chunks(3)
                        .map(|t| [base + t[0], base + t[1], base + t[2]]),
                );
            }
            out.insert(name.to_string(), part);
        }
        for c in node["children"].as_array().into_iter().flatten() {
            stack.push((c.as_u64().unwrap() as usize, world));
        }
    }
    out
}

#[test]
fn knight_mesh_lies_inside_the_hitboxes() {
    let parts = part_meshes();
    let names: Vec<&str> = parts.keys().map(String::as_str).collect();
    assert_eq!(names, PARTS);
    for (name, part) in &parts {
        let overshoot = |p: Vec3| {
            if is_head(name) {
                sphere_overshoot(p)
            } else {
                capsule_overshoot(p)
            }
        };
        let (worst, at) = part
            .verts
            .iter()
            .map(|&p| (overshoot(p), p))
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap();
        println!(
            "mesh {name:10} {} worst {:+.1} cm at {at:.3}",
            if is_head(name) { "head" } else { "body" },
            worst * 100.0
        );
        assert!(
            worst <= TOLERANCE,
            "{name} pokes {worst:.3} m out of its hitbox at {at}"
        );
    }
    // Boots stand on the ground.
    let lowest = parts["BootL"]
        .verts
        .iter()
        .map(|p| p.y)
        .fold(f32::MAX, f32::min);
    assert!(lowest.abs() < 1e-3, "boot soles at {lowest}");
}

/// The model's silhouette seen along one axis, rasterised on a `CELL` grid over
/// u in -0.5..0.5 and y in -0.05..1.95.
struct Silhouette {
    covered: Vec<bool>,
}

const NU: usize = 100;
const NV: usize = 200;
const U0: f32 = -0.5;
const V0: f32 = -0.05;

fn cell_center(i: usize, j: usize) -> Vec2 {
    Vec2::new(U0 + (i as f32 + 0.5) * CELL, V0 + (j as f32 + 0.5) * CELL)
}

fn silhouette(parts: &BTreeMap<String, PartMesh>, project: fn(Vec3) -> Vec2) -> Silhouette {
    let mut covered = vec![false; NU * NV];
    for (name, part) in parts {
        if starts_hidden(name) {
            continue;
        }
        for t in &part.tris {
            let [a, b, c] = t.map(|k| project(part.verts[k]));
            let d = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
            if d.abs() < 1e-12 {
                continue;
            }
            let (lo, hi) = (a.min(b).min(c), a.max(b).max(c));
            let cell = |v: f32, v0: f32, n: usize| {
                ((v - v0) / CELL).floor().clamp(0.0, n as f32 - 1.0) as usize
            };
            for j in cell(lo.y, V0, NV)..=cell(hi.y, V0, NV) {
                for i in cell(lo.x, U0, NU)..=cell(hi.x, U0, NU) {
                    let p = cell_center(i, j);
                    let l1 = ((b.y - c.y) * (p.x - c.x) + (c.x - b.x) * (p.y - c.y)) / d;
                    let l2 = ((c.y - a.y) * (p.x - c.x) + (a.x - c.x) * (p.y - c.y)) / d;
                    if l1 >= 0.0 && l2 >= 0.0 && l1 + l2 <= 1.0 {
                        covered[j * NU + i] = true;
                    }
                }
            }
        }
    }
    Silhouette { covered }
}

impl Silhouette {
    /// The farthest any point of a hitbox's silhouette (`inside`) lies from the
    /// model's silhouette, and where.
    fn gap(&self, inside: impl Fn(Vec2) -> bool) -> (f32, Vec2) {
        let edge: Vec<Vec2> = (0..NV)
            .flat_map(|j| (0..NU).map(move |i| (i, j)))
            .filter(|&(i, j)| {
                self.covered[j * NU + i]
                    && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(di, dj)| {
                        let (ni, nj) = (i as i64 + di, j as i64 + dj);
                        ni < 0
                            || nj < 0
                            || ni >= NU as i64
                            || nj >= NV as i64
                            || !self.covered[nj as usize * NU + ni as usize]
                    })
            })
            .map(|(i, j)| cell_center(i, j))
            .collect();
        let mut worst = (0.0, Vec2::ZERO);
        for j in 0..NV {
            for i in 0..NU {
                let p = cell_center(i, j);
                if self.covered[j * NU + i] || !inside(p) {
                    continue;
                }
                let d = edge
                    .iter()
                    .map(|e| e.distance_squared(p))
                    .fold(f32::MAX, f32::min)
                    .sqrt();
                if d > worst.0 {
                    worst = (d, p);
                }
            }
        }
        worst
    }
}

fn in_capsule(p: Vec2) -> bool {
    let y = p.y.clamp(BODY_BOTTOM + BODY_RADIUS, BODY_TOP - BODY_RADIUS);
    Vec2::new(p.x, p.y - y).length() <= BODY_RADIUS
}

fn in_sphere(p: Vec2) -> bool {
    p.distance(Vec2::new(0.0, HEAD_CENTER)) <= HEAD_RADIUS
}

/// A view: its name, how it projects model space onto (across, up), and its
/// limit for the body capsule.
type View = (&'static str, fn(Vec3) -> Vec2, f32);

#[test]
fn knight_silhouette_fills_the_hitboxes() {
    let parts = part_meshes();
    let views: [View; 2] = [
        ("front", |p| Vec2::new(p.x, p.y), FILL),
        ("side", |p| Vec2::new(p.z, p.y), SIDE_FILL),
    ];
    for (view, project, limit) in views {
        let s = silhouette(&parts, project);
        for (hitbox, inside) in [
            ("body capsule", in_capsule as fn(Vec2) -> bool),
            ("head sphere", in_sphere),
        ] {
            let (gap, at) = s.gap(inside);
            println!(
                "{view} view: the {hitbox} sticks out {:.1} cm past the model at most (at {at:.3})",
                gap * 100.0
            );
            let limit = if hitbox == "head sphere" { FILL } else { limit };
            // Raster slack: cell centres are up to half a cell's diagonal off.
            assert!(
                gap <= limit + CELL * 0.75,
                "{view} view: the {hitbox} sticks out {gap:.3} m past the model at {at}"
            );
        }
    }
}
