//! The knight (docs/M2-SPEC.md → The knight, Amendment B; gate S5): the model the
//! training dummy wears. It has every named part, joint pivot and eye state the
//! procedural animation needs, stays inside its triangle budget, and fits the
//! fixed gameplay hitboxes of `player.rs` (body capsule r 0.33 m from 0.05 to
//! 1.45 m, head sphere r 0.20 m centred at 1.62 m). The model is made to fit them,
//! never the reverse.
//!
//! - **Cosmetic:** the tall wizard hat (`Hat` and its floppy `HatTip`) takes no
//!   hits, like a Fortnite cosmetic (Amendment B, D49), so it is left out of the
//!   fit and may rise well above the head sphere.
//! - **Inside:** every other body part lies inside the body capsule within
//!   [`TOLERANCE`] (5 cm), and the helmet and eyes inside the head sphere
//!   within [`HEAD_TOLERANCE`] (9 cm, D121: the chibi bucket helm may
//!   overhang the head hitbox by up to +45% of its radius, cosmetic only).
//!   Checked on the sidecar's part bounds, then exactly on every vertex of the glb.
//!   Boots stand on the ground, where the capsule's rounded end can't hold two
//!   feet, so below [`FOOT_BAND`] the capsule counts as a cylinder (as for
//!   Milestone 1's figure).
//! - **Filled:** neither hitbox sticks out more than about 10 cm ([`FILL`]) past the
//!   non-cosmetic model: the parts' bounds reach every side of both hitboxes, and
//!   seen from the front (the dummy always turns to face the player) no point of
//!   either hitbox is more than 10 cm from the model's silhouette. Seen from the
//!   side, the small torso leaves the capsule's front emptier; that view is held
//!   to [`SIDE_FILL`] so it can't get worse unnoticed.
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

/// How far a body part may poke out of the body capsule.
const TOLERANCE: f32 = 0.05;
/// How far the helmet and eyes may poke out of the head sphere (D121).
const HEAD_TOLERANCE: f32 = 0.09;

/// A part's tolerance: the head's (D121) or the body's.
fn tolerance(part: &str) -> f32 {
    if is_head(part) {
        HEAD_TOLERANCE
    } else {
        TOLERANCE
    }
}
/// How far a hitbox may stick out past the model.
const FILL: f32 = 0.10;
/// The side view's limit (see the module docs).
const SIDE_FILL: f32 = 0.21;
/// Below this height the body capsule counts as a cylinder (boots on the ground).
const FOOT_BAND: f32 = 0.12;
/// Silhouette raster cell size.
const CELL: f32 = 0.01;

const PARTS: [&str; 18] = [
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
    "HatTip",
    "Helmet",
    "Robe",
    "Torso",
];

/// Every part and pivot with its parent node: the hierarchy procedural animation
/// relies on (rotate a pivot, its part follows; bob the torso, the upper body follows).
const PARENTS: [(&str, &str); 27] = [
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
    ("PivotRobe", "Torso"),
    ("Robe", "PivotRobe"),
    ("PivotHead", "Torso"),
    ("Helmet", "PivotHead"),
    ("PivotHat", "PivotHead"),
    ("Hat", "PivotHat"),
    ("PivotHatTip", "Hat"),
    ("HatTip", "PivotHatTip"),
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
    part == "Helmet" || part.starts_with("Eye")
}

/// The hat takes no hits (Amendment B): it is left out of the fit.
fn is_cosmetic(part: &str) -> bool {
    part == "Hat" || part == "HatTip"
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
        side.triangles <= 12000 && side.triangle_budget == 12000,
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

    // The robe hangs from the waist, the hat's tip turns at its bend, up the crown.
    let waist = at("PivotRobe");
    assert!(
        waist.x.abs() < 1e-4 && (0.65..0.95).contains(&waist.y),
        "waist {waist}"
    );
    let robe = side.parts["Robe"].bounds;
    assert!(
        robe.max[1] < waist.y + 0.05 && robe.min[1] < 0.4,
        "the robe runs from the waist to the knees: {robe:?}"
    );
    let bend = at("PivotHatTip");
    assert!(
        bend.y > hat.y + 0.2,
        "the hat's bend {bend} high above its base"
    );

    // Palette: steel helmet, purple hat, robe and cape, gold star and trims,
    // white eyes with pupils.
    let colors = |p: &str| side.parts[p].colors.clone();
    assert!(colors("Helmet").contains(&"knight_steel".to_string()));
    for (part, color) in [
        ("Hat", "knight_purple"),
        ("Hat", "star_gold"),
        ("HatTip", "knight_purple"),
        ("Robe", "knight_purple"),
        ("Robe", "star_gold"),
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
    for (name, part) in side.parts.iter().filter(|(n, _)| !is_cosmetic(n)) {
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
            out <= tolerance(name),
            "{name}'s bounds poke {out:.3} m out of its hitbox's bounds"
        );
    }
    // The bounds reach every side of both hitboxes.
    let body = union(
        side.parts
            .iter()
            .filter(|(n, _)| !is_head(n) && !is_cosmetic(n))
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

/// The hat is cosmetic (Amendment B): tall and pointed, rising well clear of
/// the head sphere, with a wide brim and a tip bent over to his side. The
/// dropped-hat prop (`knight_hat`) is exactly this hat, pivoted at its base.
#[test]
fn the_cosmetic_hat_is_tall_and_pointed_and_the_dropped_prop_matches_it() {
    let side = sidecar();
    let hat = side.parts["Hat"].bounds;
    let tip = side.parts["HatTip"].bounds;
    let top = hat.max[1].max(tip.max[1]);
    println!(
        "hat top {top:.3} m, the head sphere's top {:.3} m",
        HEAD_CENTER + HEAD_RADIUS
    );
    assert!(
        top > HEAD_CENTER + HEAD_RADIUS + 0.2,
        "a tall hat: top {top}"
    );
    let helmet = side.parts["Helmet"].bounds;
    assert!(
        hat.size().x > helmet.size().x + 0.08,
        "the brim ({:.3} m) is wider than the helmet ({:.3} m)",
        hat.size().x,
        helmet.size().x
    );
    // Bent over to his right (+X) at the tip, not a straight cone.
    let bend = side.attach["PivotHatTip"].position();
    assert!(tip.max[0] > bend.x + 0.12, "the tip bends over: {tip:?}");

    let prop =
        Sidecar::parse(&fs::read_to_string(models_dir().join("knight_hat.json")).unwrap()).unwrap();
    let base = side.attach["PivotHat"].position();
    for part in ["Hat", "HatTip"] {
        let (a, b) = (side.parts[part].bounds, prop.parts[part].bounds);
        assert!(
            (a.min() - base - b.min()).abs().max_element() < 2e-3
                && (a.max() - base - b.max()).abs().max_element() < 2e-3,
            "knight_hat's {part} is the knight's, pivoted at the hat's base"
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
    for (name, part) in parts.iter().filter(|(n, _)| !is_cosmetic(n)) {
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
            worst <= tolerance(name),
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
        if starts_hidden(name) || is_cosmetic(name) {
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

// ---------------------------------------------------------------------------
// Procedural animation and eyes (the pure core, fixed 60 Hz frames)
// ---------------------------------------------------------------------------

use pieced::knight::{
    BLINK_EVERY, BLINK_TIME, EyeState, KO_TIME, KnightAnim, KnightEvent, KnightInput, KnightPose,
    WIDE_TIME,
};

const DT: f32 = 1.0 / 60.0;

fn frames(seconds: f32) -> usize {
    (seconds / DT).round() as usize
}

fn run(anim: &mut KnightAnim, input: KnightInput, seconds: f32) -> Vec<KnightPose> {
    (0..frames(seconds))
        .map(|_| anim.step(DT, &input))
        .collect()
}

fn still() -> KnightInput {
    KnightInput::default()
}

fn moving(velocity: Vec3) -> KnightInput {
    KnightInput {
        velocity,
        ..default()
    }
}

/// Where a point 0.5 m below a pivot goes under a rotation (a boot, a fist, the hem).
fn hang(q: Quat) -> Vec3 {
    q * Vec3::new(0.0, -0.5, 0.0)
}

fn range(values: impl Iterator<Item = f32>) -> (f32, f32) {
    values.fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)))
}

/// Frames at which a state starts, and how long each run of it lasts (frames).
fn runs_of(poses: &[KnightPose], state: EyeState) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (i, p) in poses.iter().enumerate() {
        if p.eyes == state {
            match out.last_mut() {
                Some((start, len)) if *start + *len == i => *len += 1,
                _ => out.push((i, 1)),
            }
        }
    }
    out
}

#[test]
fn idle_knight_bobs_and_blinks_every_two_to_five_seconds() {
    let mut anim = KnightAnim::new(11);
    let poses = run(&mut anim, still(), 60.0);
    let (lo, hi) = range(poses.iter().map(|p| p.torso_offset.y));
    assert!(
        (0.015..0.06).contains(&(hi - lo)),
        "idle bob {:.3} m",
        hi - lo
    );
    assert!(
        poses
            .iter()
            .all(|p| hang(p.legs[0]).z.abs() < 0.01 && p.visible && p.take == 0.0)
    );
    // Goofy: the torso rocks side to side, the robe swings and the hat's tip
    // wobbles; the model never leaves his feet.
    let (lo, hi) = range(poses.iter().map(|p| (p.torso * Vec3::Y).x));
    assert!(hi - lo > 0.08, "rock {lo:.3}..{hi:.3}");
    let (lo, hi) = range(poses.iter().map(|p| hang(p.robe).x));
    assert!(hi - lo > 0.03, "robe swing {lo:.3}..{hi:.3}");
    for axis in [Vec3::X, Vec3::Z] {
        let (lo, hi) = range(poses.iter().map(|p| (p.hat_tip * Vec3::Y).dot(axis)));
        assert!(
            hi - lo > 0.1,
            "the hat tip wobbles along {axis}: {lo:.3}..{hi:.3}"
        );
    }
    assert!(poses.iter().all(|p| p.offset == Vec3::ZERO));
    let blinks = runs_of(&poses, EyeState::Blink);
    assert!(blinks.len() >= 11, "{} blinks in a minute", blinks.len());
    assert!(blinks[0].0 as f32 * DT <= BLINK_EVERY.1 + DT);
    for pair in blinks.windows(2) {
        let gap = (pair[1].0 - pair[0].0) as f32 * DT;
        assert!(
            (BLINK_EVERY.0 - DT..=BLINK_EVERY.1 + DT).contains(&gap),
            "blinks {gap:.2} s apart"
        );
    }
    for (_, len) in &blinks {
        assert!(
            (*len as f32 * DT - BLINK_TIME).abs() <= DT * 1.5,
            "a blink of {len} frames"
        );
    }
    // The rest of the time the eyes are open.
    let blinking: usize = blinks.iter().map(|b| b.1).sum();
    assert_eq!(
        runs_of(&poses, EyeState::Open)
            .iter()
            .map(|r| r.1)
            .sum::<usize>()
            + blinking,
        poses.len()
    );
}

#[test]
fn running_swings_boots_pumps_gauntlets_bobs_and_trails_the_cape() {
    let mut anim = KnightAnim::new(3);
    run(&mut anim, moving(Vec3::new(0.0, 0.0, -5.5)), 0.5);
    let poses = run(&mut anim, moving(Vec3::new(0.0, 0.0, -5.5)), 2.0);
    let left: Vec<f32> = poses.iter().map(|p| hang(p.legs[0]).z).collect();
    let right: Vec<f32> = poses.iter().map(|p| hang(p.legs[1]).z).collect();
    let (lo, hi) = range(left.iter().copied());
    assert!(lo < -0.2 && hi > 0.2, "boot swing {lo:.2}..{hi:.2} m");
    for ((l, r), p) in left.iter().zip(&right).zip(&poses) {
        assert!((l + r).abs() < 0.02, "boots swing in opposite phase");
        // Each gauntlet pumps against the boot on its side.
        let fist = hang(p.arms[0]).z;
        assert!(
            fist * l <= 1e-4,
            "left fist {fist:.2} with left boot {l:.2}"
        );
    }
    // About 3.2 strides a second: 2 zero crossings per stride.
    let crossings = left
        .windows(2)
        .filter(|w| w[0].signum() != w[1].signum())
        .count();
    assert!(
        (10..=16).contains(&crossings),
        "{crossings} crossings in 2 s"
    );
    let (lo, hi) = range(poses.iter().map(|p| p.torso_offset.y));
    assert!(hi - lo > 0.05, "run bob {:.3} m", hi - lo);
    let lean = poses[0].torso * Vec3::Y;
    assert!(lean.z < -0.2, "leans well into the run: {lean}");
    let mean =
        |f: &dyn Fn(&KnightPose) -> f32| poses.iter().map(f).sum::<f32>() / poses.len() as f32;
    let hem = mean(&|p| hang(p.cape).z);
    assert!(hem > 0.1, "the cape trails behind: {hem:.2}");
    let robe = mean(&|p| hang(p.robe).z);
    assert!(robe > 0.05, "the robe trails behind: {robe:.2}");
    let (lo, hi) = range(poses.iter().map(|p| hang(p.robe).x));
    assert!(
        hi - lo > 0.05,
        "the robe sways with the hips: {lo:.2}..{hi:.2}"
    );
    // A bouncy stride: squashed at each footfall, stretched in flight.
    let (lo, hi) = range(poses.iter().map(|p| p.scale.y));
    assert!(hi - lo > 0.06, "bounce {lo:.3}..{hi:.3}");
    let tip = mean(&|p| (p.hat_tip * Vec3::Y).z);
    assert!(tip > 0.1, "the hat's tip flops back: {tip:.2}");
}

#[test]
fn strafing_side_steps_without_crossing_the_boots() {
    // To his right (+X), the dummy's usual move.
    let mut anim = KnightAnim::new(5);
    run(&mut anim, moving(Vec3::new(5.5, 0.0, 0.0)), 0.5);
    let poses = run(&mut anim, moving(Vec3::new(5.5, 0.0, 0.0)), 2.0);
    // The left boot hangs at -X: outwards is -x, inwards (towards the right boot) +x.
    let (out_l, in_l) = range(poses.iter().map(|p| hang(p.legs[0]).x));
    let (in_r, out_r) = range(poses.iter().map(|p| hang(p.legs[1]).x));
    assert!(
        out_l < -0.2 && out_r > 0.2,
        "side steps {out_l:.2} / {out_r:.2}"
    );
    assert!(
        in_l < 0.1 && in_r > -0.1,
        "boots cross: {in_l:.2} / {in_r:.2}"
    );
    assert!(
        poses.iter().all(|p| hang(p.legs[0]).z.abs() < 0.02),
        "no forward swing"
    );
    // The cape swings out to the trailing side.
    let hem = poses.iter().map(|p| hang(p.cape).x).sum::<f32>() / poses.len() as f32;
    assert!(hem < -0.05, "the cape trails left: {hem:.2}");
}

#[test]
fn jump_stretches_the_air_pose_splits_and_landing_squashes() {
    let mut anim = KnightAnim::new(2);
    run(&mut anim, still(), 1.0);
    anim.event(KnightEvent::Jump);
    let up = KnightInput {
        grounded: false,
        ..default()
    };
    let rising = run(&mut anim, up, 0.1);
    let tallest = rising.iter().map(|p| p.scale.y).fold(0.0, f32::max);
    assert!(tallest > 1.06, "take-off stretch {tallest:.3}");
    assert!(rising.iter().any(|p| p.scale.x < 0.98));
    let air = run(&mut anim, up, 0.4);
    let last = air.last().unwrap();
    assert!(
        hang(last.legs[0]).z < -0.1 && hang(last.legs[1]).z > 0.05,
        "boots split"
    );
    assert!(
        hang(last.arms[0]).x < -0.2 && hang(last.arms[1]).x > 0.2,
        "arms flail out"
    );
    anim.event(KnightEvent::Land { speed: 9.0 });
    let landing = run(&mut anim, still(), 0.12);
    let flattest = landing.iter().map(|p| p.scale.y).fold(f32::MAX, f32::min);
    assert!(flattest < 0.93, "landing squash {flattest:.3}");
    let settled = run(&mut anim, still(), 1.5);
    let last = settled.last().unwrap();
    assert!((last.scale.x - 1.0).abs() < 0.02, "{}", last.scale);
    assert!(hang(last.legs[0]).z.abs() < 0.01, "back on his feet");
}

/// The pose's hands and boots, 0.5 m down their limbs.
fn hands(p: &KnightPose) -> [Vec3; 2] {
    p.arms.map(hang)
}

fn boots(p: &KnightPose) -> [Vec3; 2] {
    p.legs.map(hang)
}

/// A shot from the front pushes him back (+Z): the knight after one standing
/// second, hit on the next frame.
fn hit_standing(seed: u64, hits: &[KnightEvent], seconds: f32) -> Vec<KnightPose> {
    let mut anim = KnightAnim::new(seed);
    run(&mut anim, still(), 1.0);
    for &e in hits {
        anim.event(e);
    }
    run(&mut anim, still(), seconds)
}

const BODY_HIT: KnightEvent = KnightEvent::Hit {
    push: Vec3::Z,
    headshot: false,
};

#[test]
fn a_body_hit_is_a_big_cartoon_take_that_settles() {
    let poses = hit_standing(9, &[BODY_HIT], 2.0);
    // Two frames on (the gallery's rifle views capture then), the take is in.
    let p = poses[1];
    assert!(p.take > 0.85, "take {:.2} two frames after the hit", p.take);
    let [l, r] = hands(&p);
    println!("hands {l:.2} {r:.2}, boots {:.2?}", boots(&p));
    // Arms fling up and out: one hand above the shoulder, the other out wide.
    assert!(l.y.max(r.y) > 0.15, "a hand flung up: {l} {r}");
    assert!(l.y.min(r.y) > -0.2, "the other flung out: {l} {r}");
    assert!(l.x < -0.25 && r.x > 0.25, "out to both sides: {l} {r}");
    // A boot kicks up toward the shooter (in front, -Z).
    let kick = boots(&p).iter().map(|b| b.z).fold(f32::MAX, f32::min);
    assert!(kick < -0.2, "a boot kicks up: {kick:.2}");
    // He leans back, his face (and the hat's crown) still turned to the shooter.
    assert!(
        (p.torso * Vec3::Y).z > 0.12,
        "leans back: {}",
        p.torso * Vec3::Y
    );
    assert!(
        (p.tilt * Vec3::Y).z > 0.08,
        "rocks back: {}",
        p.tilt * Vec3::Y
    );
    let face = p.tilt * p.torso * p.head * Vec3::NEG_Z;
    assert!(
        face.y.abs() < 0.2 && face.z < -0.95,
        "faces the shooter: {face}"
    );
    assert_eq!(poses[0].eyes, EyeState::Wide, "wide on the hit frame");
    // A little hop and a slide back, visual only.
    let hop = poses[..frames(0.25)]
        .iter()
        .map(|p| p.offset.y)
        .fold(0.0, f32::max);
    let slide = poses[..frames(0.5)]
        .iter()
        .map(|p| p.offset.z)
        .fold(0.0, f32::max);
    assert!((0.04..0.12).contains(&hop), "hop {hop:.3} m");
    assert!((0.06..0.2).contains(&slide), "slide back {slide:.3} m");
    // He holds the take for a beat...
    assert!(poses[frames(0.2)].take > 0.9, "held at 0.2 s");
    // ...then bounces back and settles on his feet, under the character.
    let (lo, _) = range(poses[frames(0.3)..frames(1.0)].iter().map(|p| p.take));
    assert!(lo < -0.02, "bounces back past rest: {lo:.3}");
    let last = poses.last().unwrap();
    assert!(last.take.abs() < 0.01, "take {:.3}", last.take);
    assert!(
        last.offset.length() < 0.005,
        "back under the character: {}",
        last.offset
    );
    assert!(hands(last).iter().all(|h| h.y < -0.45), "arms down");
    assert!(
        (last.tilt * Vec3::Y).angle_between(Vec3::Y) < 0.01,
        "upright"
    );
    let wide = runs_of(&poses, EyeState::Wide);
    assert_eq!(wide.len(), 1);
    assert!((wide[0].1 as f32 * DT - WIDE_TIME).abs() <= DT * 1.5);
    assert!(
        poses.iter().all(|p| p.hat_offset.y == 0.0),
        "body shots leave the hat"
    );
}

#[test]
fn hits_rock_him_away_from_the_shot() {
    // From his right (pushing left, -X), he leans and slides left and kicks
    // toward the shooter on his right.
    let poses = hit_standing(
        9,
        &[KnightEvent::Hit {
            push: Vec3::NEG_X,
            headshot: false,
        }],
        0.3,
    );
    let lean = poses
        .iter()
        .map(|p| (p.tilt * Vec3::Y).x)
        .fold(0.0, f32::min);
    assert!(lean < -0.08, "rocks left: {lean:.3}");
    let slide = poses.iter().map(|p| p.offset.x).fold(0.0, f32::min);
    assert!(slide < -0.05, "slides left: {slide:.3}");
    let kick = poses
        .iter()
        .flat_map(boots)
        .map(|b| b.x)
        .fold(0.0, f32::max);
    assert!(kick > 0.2, "kicks toward the shooter: {kick:.2}");
}

#[test]
fn stacked_hits_headshots_and_shield_breaks_make_bigger_takes() {
    let peaks = |hits: &[KnightEvent]| {
        let poses = hit_standing(13, hits, 1.5);
        let take = poses.iter().map(|p| p.take).fold(0.0, f32::max);
        let hop = poses.iter().map(|p| p.offset.y).fold(0.0, f32::max);
        let slide = poses.iter().map(|p| p.offset.z).fold(0.0, f32::max);
        let wide = runs_of(&poses, EyeState::Wide)[0].1;
        (take, hop, slide, wide)
    };
    let body = peaks(&[BODY_HIT]);
    let stacked = peaks(&[BODY_HIT; 8]);
    let head = peaks(&[KnightEvent::Hit {
        push: Vec3::Z,
        headshot: true,
    }]);
    let broke = peaks(&[BODY_HIT, KnightEvent::ShieldBreak]);
    let blast = peaks(&[BODY_HIT, KnightEvent::Damage { amount: 90.0 }]);
    // A rifle body hit's damage changes nothing.
    assert_eq!(
        peaks(&[BODY_HIT, KnightEvent::Damage { amount: 28.0 }]),
        body
    );
    println!("body {body:?}\nstacked {stacked:?}\nhead {head:?}\nshield break {broke:?}");
    for (what, big) in [
        ("8 hits in a frame", stacked),
        ("a close pump blast", blast),
        ("a headshot", head),
        ("a shield break", broke),
    ] {
        assert!(
            big.0 > body.0 * 1.25,
            "{what}: take {:.2} vs {:.2}",
            big.0,
            body.0
        );
        assert!(
            big.1 > body.1 * 1.1,
            "{what}: hop {:.3} vs {:.3}",
            big.1,
            body.1
        );
        assert!(
            big.2 > body.2 * 1.2,
            "{what}: slide {:.3} vs {:.3}",
            big.2,
            body.2
        );
        assert!(big.3 > body.3, "{what}: wide eyes longer");
    }
    assert!(broke.0 >= head.0, "a shield break is the biggest");
    // Even the biggest take keeps his arms from swinging over his head.
    let poses = hit_standing(13, &[BODY_HIT, KnightEvent::ShieldBreak], 0.4);
    for p in &poses {
        for h in hands(p) {
            assert!(h.x.abs() > 0.05, "a hand swung over his head: {h}");
        }
    }
}

#[test]
fn a_running_knight_flinches_without_stopping() {
    let mut anim = KnightAnim::new(21);
    let fast = moving(Vec3::new(5.5, 0.0, 0.0));
    run(&mut anim, fast, 1.0);
    anim.event(BODY_HIT);
    let poses = run(&mut anim, fast, 0.6);
    let take = poses.iter().map(|p| p.take).fold(0.0, f32::max);
    assert!(
        (0.25..0.5).contains(&take),
        "a flinch, not a full take: {take:.2}"
    );
    let (lo, hi) = range(poses.iter().map(|p| boots(p)[0].x));
    assert!(hi - lo > 0.3, "the boots keep striding: {lo:.2}..{hi:.2}");
}

#[test]
fn each_take_flings_the_other_arm_high() {
    let mut anim = KnightAnim::new(5);
    run(&mut anim, still(), 1.0);
    let mut high = Vec::new();
    for _ in 0..3 {
        anim.event(BODY_HIT);
        let poses = run(&mut anim, still(), 0.15);
        let [l, r] = hands(poses.last().unwrap());
        high.push(if l.y > r.y { "left" } else { "right" });
        run(&mut anim, still(), 1.2);
    }
    assert_ne!(high[0], high[1], "{high:?}");
    assert_ne!(high[1], high[2], "{high:?}");
}

#[test]
fn a_hit_while_frozen_waits_for_the_thaw() {
    let mut anim = KnightAnim::new(6);
    let held = run(&mut anim, still(), 1.0).pop().unwrap();
    anim.set_frozen(true);
    anim.event(KnightEvent::Hit {
        push: Vec3::Z,
        headshot: true,
    });
    for p in run(&mut anim, still(), 1.0) {
        assert_eq!(p, held, "the pose moved while frozen");
    }
    anim.set_frozen(false);
    let thawed = run(&mut anim, still(), 0.1);
    assert_eq!(thawed[0].eyes, EyeState::Wide);
    assert!(
        thawed.last().unwrap().take > 1.0,
        "the take plays out once thawed"
    );
}

#[test]
fn headshots_bounce_the_hat() {
    let mut anim = KnightAnim::new(4);
    run(&mut anim, still(), 1.0);
    anim.event(KnightEvent::Hit {
        push: Vec3::Z,
        headshot: true,
    });
    let poses = run(&mut anim, still(), 1.2);
    let peak = poses[..frames(0.25)]
        .iter()
        .map(|p| p.hat_offset.y)
        .fold(0.0, f32::max);
    // Well clear of the helmet (T06 paints it about a helmet's height up).
    assert!(peak > 0.35, "hat pops {peak:.3} m");
    assert!(
        poses[frames(1.0)..].iter().all(|p| p.hat_offset.y < 1e-3),
        "and lands"
    );
    assert!(poses.iter().all(|p| p.hat_visible));
    assert_eq!(poses[0].eyes, EyeState::Wide);
}

#[test]
fn a_headshot_knocks_the_hat_up_away_from_the_hit_and_it_drops_back_on() {
    // A shot into the right side of his helmet as the shooter sees it (his
    // left) pushes toward his right (+X).
    let mut anim = KnightAnim::new(5);
    run(&mut anim, still(), 1.0);
    let rest = run(&mut anim, still(), 0.05).pop().unwrap();
    anim.event(KnightEvent::Hit {
        push: Vec3::new(0.5, 0.0, 0.87),
        headshot: true,
    });
    let poses = run(&mut anim, still(), 1.5);
    let top = poses[..frames(0.3)]
        .iter()
        .copied()
        .max_by(|a, b| a.hat_offset.y.total_cmp(&b.hat_offset.y))
        .unwrap();
    assert!(top.hat_offset.y > 0.35, "up {:.3} m", top.hat_offset.y);
    assert!(
        top.hat_offset.x > 0.1,
        "off to his right: {}",
        top.hat_offset
    );
    // Cocked over toward the side it flies to, and tipped toward the shooter
    // (he faces -Z).
    let up = top.hat * Vec3::Y;
    assert!(up.x > 0.25, "cocked over: {up}");
    assert!(up.z < -0.2, "crown tipped toward the shooter: {up}");
    assert!(top.hat_scale > 1.25, "swells: {}", top.hat_scale);
    // Back on his helmet, sitting as it did before (give or take the idle
    // wobble).
    let last = poses.last().unwrap();
    assert!(last.hat_offset.length() < 1e-3, "{}", last.hat_offset);
    assert!((last.hat_scale - 1.0).abs() < 1e-3, "its own size again");
    let sat = (last.hat * Vec3::Y).angle_between(rest.hat * Vec3::Y);
    assert!(sat < 0.15, "sits as before: {sat:.3} rad");
}

#[test]
fn elimination_shows_x_eyes_then_hides_and_respawn_pops_back_in() {
    let mut anim = KnightAnim::new(8);
    run(&mut anim, still(), 1.0);
    let down = KnightInput {
        downed: true,
        ..default()
    };
    let poses = run(&mut anim, down, 2.0);
    assert!(
        poses
            .iter()
            .all(|p| p.eyes == EyeState::X && !p.hat_visible)
    );
    let shown = poses.iter().take_while(|p| p.visible).count();
    assert!(
        (shown as f32 * DT - KO_TIME).abs() <= DT * 1.5,
        "KO beat of {shown} frames"
    );
    assert!(poses[shown..].iter().all(|p| !p.visible));
    // Hits while down change nothing.
    anim.event(KnightEvent::Hit {
        push: Vec3::Z,
        headshot: true,
    });
    assert_eq!(anim.step(DT, &down).eyes, EyeState::X);

    let back = run(&mut anim, still(), 1.2);
    assert!(back.iter().all(|p| p.visible && p.hat_visible));
    assert!(
        back[0].scale.y < 0.3,
        "pops in from small: {}",
        back[0].scale
    );
    let peak = back.iter().map(|p| p.scale.y).fold(0.0, f32::max);
    assert!(peak > 1.1, "overshoots: {peak:.3}");
    let last = back.last().unwrap();
    assert!(
        (last.scale - Vec3::ONE).abs().max_element() < 0.02,
        "settles: {}",
        last.scale
    );
    assert!(back.iter().all(|p| p.eyes != EyeState::X));
}

#[test]
fn animation_is_deterministic_per_seed() {
    let script = |seed| {
        let mut anim = KnightAnim::new(seed);
        let mut poses = run(&mut anim, still(), 3.0);
        anim.event(KnightEvent::Jump);
        poses.extend(run(&mut anim, moving(Vec3::new(3.0, 0.0, -2.0)), 3.0));
        poses
    };
    assert_eq!(script(1), script(1));
    assert_ne!(
        runs_of(&script(1), EyeState::Blink),
        runs_of(&script(2), EyeState::Blink)
    );
}

// ---------------------------------------------------------------------------
// The figure in the ECS: the real glb, loaded headless, worn by a character
// ---------------------------------------------------------------------------

mod figure {
    use super::*;
    use bevy::{
        gltf::GltfPlugin, mesh::MeshPlugin, state::app::StatesPlugin, time::TimeUpdateStrategy,
        world_serialization::WorldSerializationPlugin,
    };
    use pieced::{
        app::BootGate,
        arena::visuals::{TargetFigure, TargetFigurePlugin},
        combat::Downed,
        knight::{KO_TIME, KnightRig, RespawnSparkle},
        look::{ModelDressed, ToonMaterial, warmup::WarmupState},
        models::{ModelParts, ModelSpawned, ModelsPlugin},
        movement::Motor,
        shared::{AppState, Character, DamageDealt, DamageTarget, Health, LookAngles},
    };
    use std::time::Duration;

    /// Stands in for `look`, which dresses models (it needs a renderer).
    fn dress(mut spawned: MessageReader<ModelSpawned>, mut dressed: MessageWriter<ModelDressed>) {
        for m in spawned.read() {
            dressed.write(ModelDressed {
                root: m.root,
                name: m.name.clone(),
            });
        }
    }

    fn app() -> App {
        app_with(|_| {})
    }

    /// The figure's app, with `extra` plugins added before it is finished.
    fn app_with(extra: impl FnOnce(&mut App)) -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            TransformPlugin,
            MeshPlugin,
            GltfPlugin::default(),
            WorldSerializationPlugin,
            StatesPlugin,
            ModelsPlugin,
            TargetFigurePlugin,
        ))
        .init_state::<AppState>()
        .init_asset::<ToonMaterial>()
        .init_resource::<BootGate>()
        .init_resource::<WarmupState>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            DT,
        )))
        .add_systems(Update, dress);
        extra(&mut app);
        app.finish();
        app.cleanup();
        app
    }

    fn frames_of(app: &mut App, seconds: f32) {
        for _ in 0..super::frames(seconds) {
            app.update();
        }
    }

    fn part(app: &mut App, root: Entity, name: &str) -> Entity {
        let name = name.to_string();
        app.world_mut()
            .run_system_once(move |parts: ModelParts| parts.find(root, &name))
            .unwrap()
            .unwrap_or_else(|| panic!("no part"))
    }

    fn shown(app: &App, e: Entity) -> bool {
        app.world().get::<Visibility>(e) != Some(&Visibility::Hidden)
    }

    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn a_character_wears_the_rigged_knight_and_it_moves() {
        let mut app = app();
        let owner = app
            .world_mut()
            .spawn((
                Character,
                Transform::from_xyz(2.0, 0.0, -5.0),
                // No movement here: stand him on the ground by hand.
                {
                    let mut motor = Motor::default();
                    motor.grounded = true;
                    motor
                },
                LookAngles::default(),
                Health::default(),
            ))
            .id();
        let mut rig = None;
        for _ in 0..3000 {
            app.update();
            let world = app.world_mut();
            rig = world
                .query::<(&TargetFigure, &KnightRig)>()
                .iter(world)
                .find(|(f, _)| f.owner == owner)
                .map(|(_, r)| r.clone());
            // The knight can be rigged before the rest of the model library
            // has loaded (under a parallel test run it often is); Boot's
            // knight gate waits for both, so wait for both too.
            let library_ready = world
                .get_resource::<pieced::models::ModelLibrary>()
                .is_some_and(|l| l.is_ready());
            if rig.is_some() && library_ready {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let rig = rig.expect("the figure's knight was rigged");
        app.update();
        assert!(
            !app.world()
                .resource::<BootGate>()
                .held()
                .any(|k| k == pieced::arena::visuals::KNIGHT_GATE),
            "Boot is let go once the knight is rigged"
        );
        let figure = {
            let world = app.world_mut();
            world
                .query_filtered::<Entity, With<TargetFigure>>()
                .single(world)
                .unwrap()
        };
        frames_of(&mut app, 0.1);
        let model = rig.model;
        let eye = |s: &str| part_named(&rig, s);
        fn part_named(rig: &KnightRig, s: &str) -> Entity {
            let states = ["Eye", "EyeBlink", "EyeWide", "EyeX"];
            let (state, side) = s.split_at(s.len() - 1);
            let k = states.iter().position(|p| *p == state).unwrap();
            rig.eyes[k][usize::from(side == "R")]
        }
        assert!(shown(&app, eye("EyeL")) && shown(&app, eye("EyeR")));
        for hidden in ["EyeWideL", "EyeBlinkR", "EyeXL", "EyeXR"] {
            assert!(!shown(&app, eye(hidden)), "{hidden} starts hidden");
        }
        // The figure stands on its owner's feet.
        let feet = app.world().get::<Transform>(figure).unwrap().translation;
        assert!((feet - Vec3::new(2.0, 0.0, -5.0)).length() < 1e-4);

        // Running forward swings the boots in opposite directions.
        let leg_l = part(&mut app, model, "PivotLegL");
        let leg_r = part(&mut app, model, "PivotLegR");
        app.world_mut().get_mut::<Motor>(owner).unwrap().velocity = Vec3::new(0.0, 0.0, -5.5);
        let mut widest = 0.0f32;
        for _ in 0..super::frames(0.6) {
            app.update();
            let l = app.world().get::<Transform>(leg_l).unwrap().rotation;
            let r = app.world().get::<Transform>(leg_r).unwrap().rotation;
            widest = widest.max(l.angle_between(r));
        }
        assert!(widest > 0.6, "boots {widest:.2} rad apart");
        app.world_mut().get_mut::<Motor>(owner).unwrap().velocity = Vec3::ZERO;
        frames_of(&mut app, 1.0);

        // A headshot: the hat pops up and the eyes go wide.
        let hat = part(&mut app, model, "PivotHat");
        let rest = app.world().get::<Transform>(hat).unwrap().translation.y;
        app.world_mut().write_message(DamageDealt {
            source: None,
            target: owner,
            target_kind: DamageTarget::Character,
            amount: 50.0,
            to_shield: 50.0,
            headshot: true,
            shield_broke: false,
            killed: false,
            point: Vec3::new(2.0, 1.7, -4.8),
            normal: Vec3::Z,
            tick: 1,
        });
        let mut highest = rest;
        let mut wide = false;
        for _ in 0..10 {
            app.update();
            highest = highest.max(app.world().get::<Transform>(hat).unwrap().translation.y);
            wide |= shown(&app, eye("EyeWideL")) && !shown(&app, eye("EyeL"));
        }
        assert!(highest > rest + 0.05, "hat {rest:.3} -> {highest:.3}");
        assert!(wide, "wide eyes after the hit");

        // A body hit's take hops and slides the model back under the figure;
        // the character (and so its hitboxes) and the figure stay put.
        frames_of(&mut app, 1.5);
        let feet_before = *app.world().get::<Transform>(owner).unwrap();
        let arm = part(&mut app, model, "PivotArmL");
        let arm_rest = app.world().get::<Transform>(arm).unwrap().rotation;
        app.world_mut().write_message(DamageDealt {
            source: None,
            target: owner,
            target_kind: DamageTarget::Character,
            amount: 24.0,
            to_shield: 0.0,
            headshot: false,
            shield_broke: false,
            killed: false,
            point: Vec3::new(2.0, 1.0, -4.8),
            normal: Vec3::Z,
            tick: 2,
        });
        let (mut hop, mut flung) = (0.0f32, 0.0f32);
        for _ in 0..super::frames(0.3) {
            app.update();
            let offset = app.world().get::<Transform>(model).unwrap().translation;
            hop = hop.max(offset.y);
            let arm_now = app.world().get::<Transform>(arm).unwrap().rotation;
            flung = flung.max(arm_now.angle_between(arm_rest));
            assert_eq!(
                *app.world().get::<Transform>(owner).unwrap(),
                feet_before,
                "the character never moves"
            );
            let at = app.world().get::<Transform>(figure).unwrap().translation;
            assert!(
                (at - feet_before.translation).length() < 1e-4,
                "the figure stays on his feet"
            );
        }
        assert!(hop > 0.03, "the model hops: {hop:.3} m");
        assert!(flung > 1.2, "the arm flings up: {flung:.2} rad");
        frames_of(&mut app, 1.5);
        let offset = app.world().get::<Transform>(model).unwrap().translation;
        assert!(
            offset.length() < 0.005,
            "and lands back under him: {offset}"
        );

        // Eliminated: X eyes and no hat, then the figure hides...
        let hat_part = part(&mut app, model, "Hat");
        app.world_mut().entity_mut(owner).insert(Downed { tick: 3 });
        app.update();
        assert!(shown(&app, eye("EyeXL")) && !shown(&app, eye("EyeL")));
        assert!(!shown(&app, hat_part));
        assert!(shown(&app, figure));
        frames_of(&mut app, KO_TIME + 0.1);
        assert!(!shown(&app, figure), "hidden after the KO beat");

        // ...and pops back in on respawn.
        app.world_mut().entity_mut(owner).remove::<Downed>();
        app.update();
        assert!(shown(&app, figure) && shown(&app, hat_part));
        let scale = app.world().get::<Transform>(model).unwrap().scale;
        assert!(scale.y < 0.5, "pops in from small: {scale}");
        let sparkles = |app: &mut App| {
            let world = app.world_mut();
            world.query::<&RespawnSparkle>().iter(world).count()
        };
        app.update();
        assert_eq!(sparkles(&mut app), 1, "a respawn sparkle");
        frames_of(&mut app, 1.2);
        assert_eq!(sparkles(&mut app), 0, "the sparkle fades out");
        let scale = app.world().get::<Transform>(model).unwrap().scale;
        assert!((scale - Vec3::ONE).abs().max_element() < 0.03, "{scale}");
    }

    /// The game's path (M4 chunk 4): with `AnimationPlugin` the knight model
    /// loads its clips, every figure's player takes the one shared graph,
    /// the clips move his joints, and only the model's nodes are animation
    /// targets: the character (and so its hitboxes) never moves.
    #[test]
    fn the_clips_drive_the_figure_and_its_hitboxes_never_move() {
        use bevy::animation::{AnimatedBy, AnimationPlugin};
        use pieced::{
            knight::{KnightAnim, KnightGraph},
            models::ModelRoot,
        };
        let mut app = app_with(|app| {
            app.add_plugins(AnimationPlugin);
        });
        let owners: Vec<Entity> = (0..3)
            .map(|k| {
                let mut motor = Motor::default();
                motor.grounded = true;
                app.world_mut()
                    .spawn((
                        Character,
                        Transform::from_xyz(3.0 * k as f32, 0.0, -5.0),
                        motor,
                        LookAngles::default(),
                        Health::default(),
                    ))
                    .id()
            })
            .collect();
        let driven = |app: &mut App| {
            let world = app.world_mut();
            world
                .query::<&KnightAnim>()
                .iter(world)
                .filter(|a| a.clips_enabled())
                .count()
        };
        for _ in 0..3000 {
            app.update();
            if driven(&mut app) == owners.len() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(
            driven(&mut app),
            owners.len(),
            "every knight is driven by the clips"
        );
        let graph = app.world().resource::<KnightGraph>().graph.clone();
        {
            let world = app.world_mut();
            let handles: Vec<_> = world
                .query::<&AnimationGraphHandle>()
                .iter(world)
                .map(|h| h.0.clone())
                .collect();
            assert_eq!(handles.len(), owners.len());
            assert!(handles.iter().all(|h| *h == graph), "one shared graph");
        }

        // Running forward, the run clip swings his boots; the character
        // stays exactly where the simulation put it.
        let rig = {
            let world = app.world_mut();
            world
                .query::<(&TargetFigure, &KnightRig)>()
                .iter(world)
                .find(|(f, _)| f.owner == owners[0])
                .map(|(_, r)| r.clone())
                .unwrap()
        };
        let leg_l = part(&mut app, rig.model, "PivotLegL");
        let leg_r = part(&mut app, rig.model, "PivotLegR");
        let torso = part(&mut app, rig.model, "Torso");
        let rest_torso = app.world().get::<Transform>(torso).unwrap().translation;
        app.world_mut()
            .get_mut::<Motor>(owners[0])
            .unwrap()
            .velocity = Vec3::new(0.0, 0.0, -5.5);
        let before: Vec<Transform> = owners
            .iter()
            .map(|o| *app.world().get::<Transform>(*o).unwrap())
            .collect();
        let (mut apart, mut dip) = (0.0f32, 0.0f32);
        for _ in 0..super::frames(0.8) {
            app.update();
            let l = app.world().get::<Transform>(leg_l).unwrap().rotation;
            let r = app.world().get::<Transform>(leg_r).unwrap().rotation;
            apart = apart.max(l.angle_between(r));
            let t = app.world().get::<Transform>(torso).unwrap().translation;
            dip = dip.max((rest_torso - t).length());
            for (o, b) in owners.iter().zip(&before) {
                assert_eq!(app.world().get::<Transform>(*o).unwrap(), b);
            }
        }
        assert!(
            apart > 0.25,
            "the run clip swings the boots: {apart:.2} rad apart"
        );
        assert!(dip > 0.005, "and bobs him at each footfall: {dip:.3} m");

        // Only the models' nodes are animated.
        let world = app.world_mut();
        let targets: Vec<Entity> = world
            .query_filtered::<Entity, With<AnimatedBy>>()
            .iter(world)
            .collect();
        assert!(targets.len() >= 8 * owners.len());
        let mut roots = world.query_filtered::<Entity, With<ModelRoot>>();
        let roots: Vec<Entity> = roots.iter(world).collect();
        for target in targets {
            assert!(world.get::<Character>(target).is_none());
            let mut e = target;
            while let Some(parent) = world.get::<ChildOf>(e).map(ChildOf::parent) {
                e = parent;
                if roots.contains(&e) {
                    break;
                }
            }
            assert!(roots.contains(&e), "an animation target outside a model");
        }
    }
}

// ---------------------------------------------------------------------------
// The authored clips (M4 chunk 4, D104): in the glb, their fit, their mix
// ---------------------------------------------------------------------------

mod clips {
    use super::*;
    use pieced::{
        arena::visuals::wand::{GRIP_R, SHOULDER_R, WAND_LENGTH, WAND_SCALE, rest_rotation},
        knight::{
            CLIP_JOINTS, KO_TIME, KnightAnim, KnightClip, KnightEvent, KnightInput,
            clips::{FLINCH_SECONDS, LAND_SECONDS, RUN_CYCLE, WINDUP_SECONDS},
        },
        models::MODEL_FORWARD_FIX,
        orb::WAND_TIP,
    };
    use std::collections::HashMap;

    /// Seconds between the fit's samples.
    const FIT_STEP: f32 = 0.05;
    /// How far a body part may leave the capsule in a clip (the helmet keeps
    /// D121's [`HEAD_TOLERANCE`]).
    const CLIP_TOLERANCE: f32 = 0.10;

    /// One glTF animation channel.
    struct Channel {
        node: usize,
        path: String,
        times: Vec<f32>,
        values: Vec<Vec4>,
    }

    struct Anim {
        name: String,
        channels: Vec<Channel>,
    }

    impl Anim {
        fn duration(&self) -> f32 {
            self.channels
                .iter()
                .map(|c| *c.times.last().unwrap())
                .fold(0.0, f32::max)
        }
    }

    impl Channel {
        /// The channel at `t` (linear; rotations slerped), clamped to its keys.
        fn sample(&self, t: f32) -> Vec4 {
            let k = self.times.partition_point(|&x| x <= t);
            if k == 0 {
                return self.values[0];
            }
            if k >= self.times.len() {
                return *self.values.last().unwrap();
            }
            let (t0, t1) = (self.times[k - 1], self.times[k]);
            let u = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
            let (a, b) = (self.values[k - 1], self.values[k]);
            if self.path == "rotation" {
                Vec4::from(Quat::from_vec4(a).slerp(Quat::from_vec4(b), u))
            } else {
                a.lerp(b, u)
            }
        }
    }

    fn floats(glb: &Glb, accessor: usize, comps: usize) -> Vec<Vec4> {
        let (count, offset, stride, kind) = glb.layout(accessor, 4 * comps);
        assert_eq!(kind, 5126, "animation data is f32");
        let f = |i: usize| f32::from_le_bytes(glb.bin[i..i + 4].try_into().unwrap());
        (0..count)
            .map(|k| {
                let i = offset + k * stride;
                let mut v = [0.0; 4];
                for (c, slot) in v.iter_mut().enumerate().take(comps) {
                    *slot = f(i + 4 * c);
                }
                Vec4::from_array(v)
            })
            .collect()
    }

    fn animations(glb: &Glb) -> Vec<Anim> {
        let json = |v: &Value| v.as_u64().unwrap() as usize;
        glb.doc["animations"]
            .as_array()
            .expect("knight.glb has animations")
            .iter()
            .map(|a| Anim {
                name: a["name"].as_str().unwrap().to_string(),
                channels: a["channels"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|c| {
                        let sampler = &a["samplers"][json(&c["sampler"])];
                        let path = c["target"]["path"].as_str().unwrap().to_string();
                        let comps = if path == "rotation" { 4 } else { 3 };
                        assert_eq!(sampler["interpolation"].as_str(), Some("LINEAR"));
                        Channel {
                            node: json(&c["target"]["node"]),
                            times: floats(glb, json(&sampler["input"]), 1)
                                .into_iter()
                                .map(|v| v.x)
                                .collect(),
                            values: floats(glb, json(&sampler["output"]), comps),
                            path,
                        }
                    })
                    .collect(),
            })
            .collect()
    }

    fn node_name(glb: &Glb, node: usize) -> &str {
        glb.doc["nodes"][node]["name"].as_str().unwrap()
    }

    /// Node overrides (translation, rotation) at `t` of a clip, on top of
    /// `base` for an additive clip (its rotations multiply the base's).
    type Pose = HashMap<usize, (Option<Vec3>, Option<Quat>)>;

    fn sample(anim: &Anim, t: f32, base: Option<&Pose>) -> Pose {
        let mut pose: Pose = base.cloned().unwrap_or_default();
        for c in &anim.channels {
            let v = c.sample(t);
            let entry = pose.entry(c.node).or_default();
            if c.path == "rotation" {
                let q = Quat::from_vec4(v).normalize();
                entry.1 = Some(match (base, entry.1) {
                    (Some(_), Some(b)) => q * b,
                    _ => q,
                });
            } else {
                entry.0 = Some(v.truncate());
            }
        }
        pose
    }

    /// Every node's global transform (glTF scene space) under a pose.
    fn globals(glb: &Glb, pose: &Pose) -> Vec<Affine3A> {
        let nodes = glb.doc["nodes"].as_array().unwrap();
        let f = |v: &Value, n: usize| -> Vec<f32> {
            (0..n).map(|i| v[i].as_f64().unwrap() as f32).collect()
        };
        let mut out = vec![Affine3A::IDENTITY; nodes.len()];
        let mut stack: Vec<(usize, Affine3A)> = glb.doc["scenes"][0]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| (i.as_u64().unwrap() as usize, Affine3A::IDENTITY))
            .collect();
        while let Some((i, parent)) = stack.pop() {
            let n = &nodes[i];
            let (mut t, mut r) = (
                n.get("translation")
                    .map_or(Vec3::ZERO, |v| Vec3::from_slice(&f(v, 3))),
                n.get("rotation")
                    .map_or(Quat::IDENTITY, |v| Quat::from_slice(&f(v, 4))),
            );
            if let Some((pt, pr)) = pose.get(&i) {
                t = pt.unwrap_or(t);
                r = pr.unwrap_or(r);
            }
            let s = n
                .get("scale")
                .map_or(Vec3::ONE, |v| Vec3::from_slice(&f(v, 3)));
            out[i] = parent * Affine3A::from_scale_rotation_translation(s, r, t);
            for c in n["children"].as_array().into_iter().flatten() {
                stack.push((c.as_u64().unwrap() as usize, out[i]));
            }
        }
        out
    }

    /// Each named mesh part's worst overshoot of its own hitbox under a pose
    /// (model space).
    fn overshoots(glb: &Glb, pose: &Pose) -> BTreeMap<String, f32> {
        let g = globals(glb, pose);
        let mut out = BTreeMap::new();
        for (i, n) in glb.doc["nodes"].as_array().unwrap().iter().enumerate() {
            let (Some(name), Some(mesh)) = (n["name"].as_str(), n["mesh"].as_u64()) else {
                continue;
            };
            if is_cosmetic(name) {
                continue;
            }
            let mut worst = f32::MIN;
            for prim in glb.doc["meshes"][mesh as usize]["primitives"]
                .as_array()
                .unwrap()
            {
                let pos = prim["attributes"]["POSITION"].as_u64().unwrap() as usize;
                for p in glb.vec3s(pos) {
                    let p = gltf_to_model(g[i].transform_point3(p));
                    let o = if is_head(name) {
                        sphere_overshoot(p)
                    } else {
                        capsule_overshoot(p)
                    };
                    worst = worst.max(o);
                }
            }
            out.insert(name.to_string(), worst);
        }
        out
    }

    #[test]
    fn every_clip_is_in_the_glb_with_its_authored_length() {
        let side = sidecar();
        let glb = load_glb();
        let anims = animations(&glb);
        assert_eq!(side.clips.len(), KnightClip::ALL.len());
        assert_eq!(anims.len(), side.clips.len());
        for (info, anim) in side.clips.iter().zip(&anims) {
            assert_eq!(
                info.name, anim.name,
                "the sidecar lists the clips in glTF order"
            );
            assert!((info.duration - anim.duration()).abs() < 1e-5);
        }
        for clip in KnightClip::ALL {
            let anim = anims
                .iter()
                .find(|a| a.name == clip.name())
                .unwrap_or_else(|| panic!("no {} clip in knight.glb", clip.name()));
            println!("clip {:12} {:.3} s", clip.name(), anim.duration());
            assert!(
                (anim.duration() - clip.duration()).abs() < 1e-5,
                "{}: {} s, authored {}",
                clip.name(),
                anim.duration(),
                clip.duration()
            );
        }
        // The wind-up is exactly the gameplay wind-up.
        let windup = anims.iter().find(|a| a.name == "WindUp").unwrap();
        assert!((windup.duration() - 0.4).abs() < 1e-6);
        assert_eq!(WINDUP_SECONDS, 0.4);
        assert_eq!(
            pieced::tuning::Tuning::default().grunt.windup,
            WINDUP_SECONDS,
            "the clip is the gameplay wind-up"
        );
        // The deaths last the KO beat.
        for clip in [
            KnightClip::DeathBack,
            KnightClip::DeathSpin,
            KnightClip::DeathCrumple,
        ] {
            assert_eq!(clip.duration(), KO_TIME);
        }
    }

    #[test]
    fn clips_key_the_body_joints_and_flinches_only_turn_them() {
        let glb = load_glb();
        for anim in animations(&glb) {
            let clip = KnightClip::ALL
                .into_iter()
                .find(|c| c.name() == anim.name)
                .unwrap();
            let mut keyed: Vec<(String, String)> = anim
                .channels
                .iter()
                .map(|c| (node_name(&glb, c.node).to_string(), c.path.clone()))
                .collect();
            keyed.sort();
            for (node, path) in &keyed {
                assert!(
                    CLIP_JOINTS.contains(&node.as_str()),
                    "{}: keys {node}, not a clip joint (the hat stays procedural)",
                    anim.name
                );
                assert!(
                    path == "rotation" || (path == "translation" && node == "Torso"),
                    "{}: keys {node}'s {path}",
                    anim.name
                );
                if clip.is_additive() {
                    assert_eq!(
                        path, "rotation",
                        "{} is additive: rotations only",
                        anim.name
                    );
                }
            }
            if !clip.is_additive() {
                // Every full-body clip poses every joint, so no joint keeps a
                // stale pose from another clip.
                for joint in CLIP_JOINTS {
                    assert!(
                        keyed.iter().any(|(n, p)| n == joint && p == "rotation"),
                        "{} doesn't key {joint}",
                        anim.name
                    );
                }
                assert!(keyed.contains(&("Torso".to_string(), "translation".to_string())));
            }
        }
    }

    /// The sampled hitbox fit (docs/M4-SPEC.md → Chunk 4, S5 extended, D121):
    /// in every clip he can be shot in, every 50 ms, the body parts stay in
    /// the body capsule within 10 cm and the helmet and eyes in the head
    /// sphere within D121's 9 cm. The hitboxes themselves never move (they
    /// are the character's; see `the_clips_drive_the_figure_...`).
    #[test]
    fn in_play_clips_keep_him_inside_his_hitboxes() {
        let glb = load_glb();
        let anims = animations(&glb);
        let idle = anims.iter().find(|a| a.name == "Idle").unwrap();
        let idle0 = sample(idle, 0.0, None);
        let mut checked = 0;
        for clip in KnightClip::ALL.into_iter().filter(|c| c.in_play()) {
            let anim = anims.iter().find(|a| a.name == clip.name()).unwrap();
            let mut worst: BTreeMap<String, (f32, f32)> = BTreeMap::new();
            let n = (anim.duration() / FIT_STEP + 1e-4).floor() as usize;
            let times = (0..=n)
                .map(|k| k as f32 * FIT_STEP)
                .chain([anim.duration()]);
            for t in times {
                let base = clip.is_additive().then_some(&idle0);
                let pose = sample(anim, t, base);
                for (part, o) in overshoots(&glb, &pose) {
                    let w = worst.entry(part).or_insert((f32::MIN, 0.0));
                    if o > w.0 {
                        *w = (o, t);
                    }
                }
                checked += 1;
            }
            for (part, (o, t)) in worst {
                let limit = if is_head(&part) {
                    HEAD_TOLERANCE
                } else {
                    CLIP_TOLERANCE
                };
                let exempt = clip.fit_exempt().contains(&part.as_str());
                println!(
                    "clip {:12} {part:10} worst {:+.1} cm at {t:.2} s{}",
                    clip.name(),
                    o * 100.0,
                    if exempt { " (exempt)" } else { "" }
                );
                assert!(
                    exempt || o <= limit,
                    "{}: {part} leaves its hitbox by {o:.3} m at {t:.2} s",
                    clip.name()
                );
            }
        }
        assert!(checked > 100, "{checked} samples");
    }

    /// The wind-up's last pose puts the wand's tip (the wand as
    /// `arena::visuals::wand` attaches it) on the orb's launch point, where
    /// the gameplay orb leaves.
    #[test]
    fn the_windup_ends_with_the_wand_on_the_orbs_launch_point() {
        let glb = load_glb();
        let anims = animations(&glb);
        let windup = anims.iter().find(|a| a.name == "WindUp").unwrap();
        let arm = (0..glb.doc["nodes"].as_array().unwrap().len())
            .find(|&i| node_name(&glb, i) == "PivotArmR")
            .unwrap();
        let rest = globals(&glb, &Pose::default())[arm];
        let posed = globals(&glb, &sample(windup, WINDUP_SECONDS, None))[arm];
        let tip_rest = SHOULDER_R
            + (GRIP_R - SHOULDER_R)
            + rest_rotation() * Vec3::NEG_Z * WAND_LENGTH * WAND_SCALE;
        let fix = MODEL_FORWARD_FIX;
        let tip = fix * (posed * rest.inverse()).transform_point3(fix.inverse() * tip_rest);
        println!("wind-up wand tip {tip:.3}, the orb leaves from {WAND_TIP:.3}");
        assert!(
            tip.distance(WAND_TIP) < 0.03,
            "the wand's tip {tip} is {:.3} m from the orb's launch point {WAND_TIP}",
            tip.distance(WAND_TIP)
        );
        // At its start he stands at rest (the clip blends in from any pose).
        let start = globals(&glb, &sample(windup, 0.0, None))[arm];
        assert!(start.abs_diff_eq(rest, 1e-3));
    }

    fn clip_anim(seed: u64) -> KnightAnim {
        let mut anim = KnightAnim::new(seed);
        anim.set_clips(true);
        anim
    }

    fn steps(anim: &mut KnightAnim, input: KnightInput, seconds: f32) {
        for _ in 0..frames(seconds) {
            anim.step(DT, &input);
        }
    }

    #[test]
    fn blend_weights_follow_the_velocity() {
        let mut anim = clip_anim(3);
        steps(&mut anim, KnightInput::default(), 0.5);
        let m = anim.clip_mix();
        assert_eq!(m.weight(KnightClip::Idle), 1.0);
        assert_eq!(m.weight(KnightClip::Run), 0.0);

        // Running forward: the run cycle, its clock following the stride.
        let fwd = KnightInput {
            velocity: Vec3::new(0.0, 0.0, -5.5),
            ..default()
        };
        steps(&mut anim, fwd, 1.0);
        let m = *anim.clip_mix();
        assert!(m.weight(KnightClip::Run) > 0.95, "{:?}", m.weight);
        assert!(m.weight(KnightClip::Idle) < 0.05);
        assert!((m.body_weight() - 1.0).abs() < 1e-4);
        let t0 = m.time(KnightClip::Run);
        anim.step(DT, &fwd);
        let dt_clip = (anim.clip_mix().time(KnightClip::Run) - t0).rem_euclid(RUN_CYCLE);
        // 5.5 m/s over a 1.7 m stride: the 0.6 s cycle plays about 1.94x.
        let rate = dt_clip / DT;
        assert!((1.7..2.2).contains(&rate), "run clip rate {rate:.2}");

        // Strafing to his right (+X): the right strafe, no run.
        let right = KnightInput {
            velocity: Vec3::new(4.0, 0.0, 0.0),
            ..default()
        };
        steps(&mut anim, right, 1.0);
        let m = *anim.clip_mix();
        assert!(m.weight(KnightClip::StrafeR) > 0.9, "{:?}", m.weight);
        assert!(m.weight(KnightClip::StrafeL) < 1e-3 && m.weight(KnightClip::Run) < 0.05);

        // Diagonally forward-left: the run and the left strafe share it.
        let diag = KnightInput {
            velocity: Vec3::new(-3.0, 0.0, -3.0),
            ..default()
        };
        steps(&mut anim, diag, 1.0);
        let m = *anim.clip_mix();
        let (r, l) = (m.weight(KnightClip::Run), m.weight(KnightClip::StrafeL));
        assert!(
            (r - l).abs() < 0.1 && r > 0.35 && l > 0.35,
            "run {r:.2} strafe {l:.2}"
        );
        assert!((m.body_weight() - 1.0).abs() < 1e-4);

        // Backing off: the run cycle plays backwards.
        let back = KnightInput {
            velocity: Vec3::new(0.0, 0.0, 5.0),
            ..default()
        };
        steps(&mut anim, back, 1.0);
        let t0 = anim.clip_mix().time(KnightClip::Run);
        anim.step(DT, &back);
        let t1 = anim.clip_mix().time(KnightClip::Run);
        assert!(
            (t1 - t0).rem_euclid(RUN_CYCLE) > RUN_CYCLE / 2.0,
            "{t0} -> {t1}"
        );

        // Slowing to a stop blends back to idle.
        steps(&mut anim, KnightInput::default(), 1.0);
        assert!(anim.clip_mix().weight(KnightClip::Idle) > 0.99);
    }

    #[test]
    fn the_windup_clip_runs_on_the_gameplay_windup_clock() {
        let mut anim = clip_anim(4);
        steps(&mut anim, KnightInput::default(), 0.3);
        // The wind-up, 0.4 s of gameplay: the clip's time is its progress.
        for k in 0..=24 {
            let p = k as f32 / 24.0;
            anim.step(
                DT,
                &KnightInput {
                    windup: Some(p),
                    ..default()
                },
            );
            let m = anim.clip_mix();
            assert!((m.time(KnightClip::WindUp) - p * WINDUP_SECONDS).abs() < 1e-6);
            if p >= 0.25 {
                assert_eq!(m.weight(KnightClip::WindUp), 1.0, "at {p}");
            }
        }
        // The release: held at the thrust, then let go.
        let released = |anim: &mut KnightAnim, s: f32| {
            steps(anim, KnightInput::default(), s);
            *anim.clip_mix()
        };
        let m = released(&mut anim, 0.1);
        assert_eq!(m.weight(KnightClip::WindUp), 1.0);
        assert_eq!(m.time(KnightClip::WindUp), WINDUP_SECONDS);
        let m = released(&mut anim, 0.25);
        let w = m.weight(KnightClip::WindUp);
        assert!(w > 0.0 && w < 1.0, "lowering: {w}");
        let m = released(&mut anim, 0.3);
        assert_eq!(m.weight(KnightClip::WindUp), 0.0);
        assert!((m.body_weight() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn flinches_landings_hops_falls_and_deaths_play_their_clips() {
        let mut anim = clip_anim(5);
        steps(&mut anim, KnightInput::default(), 0.2);
        // A shot on his left: that flinch, added on top, for its length.
        anim.event(KnightEvent::Flinch {
            region: pieced::knight::HitRegion::Left,
            push: Vec3::Z,
        });
        anim.step(DT, &KnightInput::default());
        let m = *anim.clip_mix();
        assert_eq!(m.weight(KnightClip::FlinchL), 1.0);
        assert_eq!(m.weight(KnightClip::FlinchR), 0.0);
        assert!(
            (m.body_weight() - 1.0).abs() < 1e-4,
            "additive: the body blend is kept"
        );
        steps(&mut anim, KnightInput::default(), FLINCH_SECONDS);
        assert_eq!(anim.clip_mix().weight(KnightClip::FlinchL), 0.0);

        // Down the beam: the landing clip's first pose; touchdown plays it
        // and squashes him, then it fades out.
        let beam = KnightInput {
            beaming: true,
            grounded: false,
            ..default()
        };
        steps(&mut anim, beam, 0.5);
        let m = *anim.clip_mix();
        assert_eq!(
            (m.weight(KnightClip::BeamLand), m.time(KnightClip::BeamLand)),
            (1.0, 0.0)
        );
        let squash = anim.step(DT, &KnightInput::default()).scale.y;
        steps(&mut anim, KnightInput::default(), 0.1);
        let m = *anim.clip_mix();
        assert_eq!(m.weight(KnightClip::BeamLand), 1.0);
        assert!(m.time(KnightClip::BeamLand) > 0.09);
        let later = anim.last_pose().scale.y;
        assert!(later < 1.0 || squash < 1.0, "a squash on touchdown");
        steps(&mut anim, KnightInput::default(), LAND_SECONDS);
        assert_eq!(anim.clip_mix().weight(KnightClip::BeamLand), 0.0);

        // The victory hop while the player is down.
        let party = KnightInput {
            celebrating: true,
            ..default()
        };
        steps(&mut anim, party, 0.3);
        assert!(anim.clip_mix().weight(KnightClip::VictoryHop) > 0.99);
        steps(&mut anim, KnightInput::default(), 0.1);
        assert_eq!(anim.clip_mix().weight(KnightClip::VictoryHop), 0.0);

        // Flung into the void: the fall, round and round.
        let fall = KnightInput {
            falling: true,
            grounded: false,
            ..default()
        };
        steps(&mut anim, fall, 0.8);
        let m = *anim.clip_mix();
        assert_eq!(m.weight(KnightClip::VoidFall), 1.0);
        assert!(m.time(KnightClip::VoidFall) < KnightClip::VoidFall.duration());

        // Eliminated: one of the deaths over the KO beat. A headshot knocks
        // him flat on his back; a heavy blast spins him round.
        for (headshot, damage, want) in [
            (true, 50.0, Some(KnightClip::DeathBack)),
            (false, 90.0, Some(KnightClip::DeathSpin)),
            (false, 20.0, None),
        ] {
            let mut anim = clip_anim(9);
            steps(&mut anim, KnightInput::default(), 0.2);
            anim.event(KnightEvent::Hit {
                push: Vec3::Z,
                headshot,
            });
            anim.event(KnightEvent::Damage { amount: damage });
            let down = KnightInput {
                downed: true,
                ..default()
            };
            steps(&mut anim, down, KO_TIME * 0.5);
            let m = *anim.clip_mix();
            let deaths = [
                KnightClip::DeathBack,
                KnightClip::DeathSpin,
                KnightClip::DeathCrumple,
            ];
            let playing: Vec<_> = deaths.into_iter().filter(|d| m.weight(*d) == 1.0).collect();
            assert_eq!(playing.len(), 1, "{playing:?}");
            if let Some(want) = want {
                assert_eq!(playing[0], want);
            }
            assert!((m.time(playing[0]) - KO_TIME * 0.5).abs() < 0.02);
            assert!((m.body_weight() - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn with_clips_the_procedural_idle_and_run_step_aside() {
        let mut procedural = KnightAnim::new(7);
        let mut clipped = clip_anim(7);
        let fwd = KnightInput {
            velocity: Vec3::new(0.0, 0.0, -5.5),
            ..default()
        };
        let mut swing = (0.0f32, 0.0f32);
        for _ in 0..frames(1.0) {
            let a = procedural.step(DT, &fwd);
            let b = clipped.step(DT, &fwd);
            swing.0 = swing.0.max(a.legs[0].angle_between(Quat::IDENTITY));
            swing.1 = swing.1.max(b.legs[0].angle_between(Quat::IDENTITY));
            assert_eq!(
                b.torso_offset,
                Vec3::ZERO,
                "the clip bobs him, not the pose"
            );
        }
        assert!(swing.0 > 0.4 && swing.1 < 1e-4, "{swing:?}");
        // The springs still play on top: a hit's take flings the arms.
        clipped.event(KnightEvent::Hit {
            push: Vec3::Z,
            headshot: false,
        });
        let mut fling = 0.0f32;
        for _ in 0..frames(0.2) {
            let p = clipped.step(DT, &KnightInput::default());
            fling = fling.max(p.arms[0].angle_between(Quat::IDENTITY));
        }
        assert!(fling > 0.6, "the take on top of the clips: {fling:.2}");
    }
}
