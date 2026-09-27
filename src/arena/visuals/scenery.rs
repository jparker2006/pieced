//! The floating island, generated in code (targets T01, T11): the grassy top
//! (the 48 m arena square plus a margin ring), rounded cartoon cliffs dropping
//! into space under its edge, dense low grass tufts and white, yellow and pink
//! flower clusters, pebbles, round bushes (on the margin and hugging the solid
//! props), a ring of framing trees and the margin's other trees, big rocks and
//! stumps (Blender models), and a sea of soft cartoon clouds under the rim and
//! around the far islands (D48). Everything here is pure, deterministic CPU
//! work ending in a few merged meshes and a list of model placements, so it is
//! tested headless and costs nothing per frame. The sky slice owns the far
//! view itself; the clouds only read its island layout.
//!
//! Triangle budget (Battery preset; see [`Island::triangles`]): at most
//! [`ISLAND_TRIANGLE_BUDGET`] for the merged scenery, about 113k today (tufts
//! about 38k, bushes about 31k, clouds about 22k, flowers about 15k, pebbles,
//! ground and cliffs the rest), plus about 53 margin models (30 trees ≤ 1.5k
//! each, rocks and stumps ≤ 300: about 51k). The Plugged-in preset adds about
//! 18k triangles of extra tufts. All of it is static: merged meshes sharing
//! six materials, split into chunks so the camera culls what it can't see.

use super::geo::{Geo, Rgba, blob, fbm2, lin, mix, noise2, shade, smoothstep};
use crate::{palette::cartoon, rng::Rng, shared::ARENA_HALF};
use bevy::prelude::*;
use std::f32::consts::{FRAC_PI_2, TAU};

/// Margin models keep at least this far outside the playable square, so walls
/// built on the arena edge never clip into scenery.
pub const EDGE_CLEARANCE: f32 = 0.35;
/// Tallest grass, flower or pebble allowed on the playable floor (m).
pub const FLOOR_CLUTTER_MAX_HEIGHT: f32 = 0.26;
/// Tallest bush allowed hugging a solid arena prop (m): below every prop's top,
/// so the prop still reads as the cover it is.
pub const PROP_BUSH_MAX_HEIGHT: f32 = 0.72;
/// How far (m) a bush hugging a prop may reach past the prop's footprint.
pub const PROP_BUSH_REACH: f32 = 0.45;
/// The merged scenery's triangle budget on the Battery preset (models aside).
pub const ISLAND_TRIANGLE_BUDGET: usize = 130_000;
/// Side of the square the island's clutter is chunked over (m), and chunks per
/// side: tufts and flowers are split into this grid so the camera culls them.
const CHUNK_SPAN: f32 = 80.0;
const CHUNKS: usize = 4;
/// The island's edge is never closer than this to the arena (m): the lip in
/// the middle of the close edge ([`LIP_Z`]), where the grass ends just past
/// the barrier.
pub const MIN_MARGIN: f32 = 0.9;
/// The close edge's margin (m) either side of the lip.
pub const CLOSE_MARGIN: f32 = 2.7;
/// The typical width of the island's margin beyond the arena (m).
pub const BASE_MARGIN: f32 = 9.0;
/// Spacing of the island top's grid (m); the outline samples the arena's
/// sides at the same spacing so the two meshes share their seam vertices.
pub const GROUND_STEP: f32 = 2.0;
/// Where the island's edge comes closest to the arena: the east side between
/// these z, [`CLOSE_MARGIN`] past the barrier.
pub const CLOSE_EDGE_Z: (f32, f32) = (-4.0, 12.0);
/// The middle of the close edge narrows further, between these z, to a lip
/// [`MIN_MARGIN`] past the barrier: the grass stops at the cliff right behind
/// the curtain. The island-edge view (T11) looks along it, at the rounded
/// cliffs where the island steps back out at its north end. Only the edge
/// samples inside the close edge move, so the margin's models stay put.
pub const LIP_Z: (f32, f32) = (0.0, 8.0);

/// The Milestone 1 sun (west-southwest, late afternoon). The Milestone 2 key
/// light is `look::ToonLighting`; this stays for the Milestone 1 gallery's
/// sunset framing until the gallery slice replaces it.
pub fn sun_direction() -> Vec3 {
    let elevation = 36f32.to_radians();
    let azimuth = 243f32.to_radians();
    Vec3::new(
        azimuth.sin() * elevation.cos(),
        elevation.sin(),
        -azimuth.cos() * elevation.cos(),
    )
    .normalize()
}

/// Distance outside the arena square (0 inside). Euclidean, so corners round off.
pub fn edge_distance(x: f32, z: f32) -> f32 {
    let dx = (x.abs() - ARENA_HALF).max(0.0);
    let dz = (z.abs() - ARENA_HALF).max(0.0);
    (dx * dx + dz * dz).sqrt()
}

/// Margin width (m) beyond the arena in the outward direction `dir` (XZ) from
/// a point `base` on the arena's edge: lobes of wider and narrower island, and
/// the close edge on the east side.
pub fn margin_at(base: Vec2, dir: Vec2) -> f32 {
    let p = base + dir * BASE_MARGIN;
    let a = p.y.atan2(p.x);
    let lobes =
        2.3 * (3.0 * a + 0.7).sin() + 1.4 * (5.0 * a + 2.1).sin() + 0.7 * (11.0 * a + 4.0).sin();
    let mut m = BASE_MARGIN + lobes;
    // The close edge: a stretch of the east side where the cliff drops right
    // behind the barrier, narrowing to a lip in its middle.
    if dir.x > 0.5 {
        let (z0, z1) = CLOSE_EDGE_Z;
        let t = smoothstep(z0 - 6.0, z0, base.y) * (1.0 - smoothstep(z1, z1 + 6.0, base.y));
        m += (CLOSE_MARGIN - m) * t;
        let (l0, l1) = LIP_Z;
        let u = smoothstep(l0 - 4.0, l0, base.y) * (1.0 - smoothstep(l1, l1 + 4.0, base.y));
        m += (MIN_MARGIN - m) * u;
    }
    m.max(MIN_MARGIN)
}

/// One sample of the island's outline: a point on the arena square's edge,
/// the outward direction there and the margin width beyond it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeSample {
    pub base: Vec3,
    pub normal: Vec3,
    pub margin: f32,
}

impl EdgeSample {
    /// The island's rim above the cliff.
    pub fn edge(&self) -> Vec3 {
        self.base + self.normal * self.margin
    }
}

/// Corner arc samples (including both ends).
const ARC_STEPS: usize = 6;

/// The island's outline, clockwise seen from above (the north side, west to
/// east, first). Side samples sit on the arena's edge every
/// [`GROUND_STEP`] m; each corner is a fan of outward directions.
pub fn outline() -> Vec<EdgeSample> {
    let h = ARENA_HALF;
    let n = (2.0 * h / GROUND_STEP).round() as i32;
    // (corner the side starts at, side direction, outward normal)
    let sides = [
        (Vec3::new(-h, 0.0, -h), Vec3::X, Vec3::NEG_Z),
        (Vec3::new(h, 0.0, -h), Vec3::Z, Vec3::X),
        (Vec3::new(h, 0.0, h), Vec3::NEG_X, Vec3::Z),
        (Vec3::new(-h, 0.0, h), Vec3::NEG_Z, Vec3::NEG_X),
    ];
    let mut out = Vec::new();
    for (start, along, normal) in sides {
        for i in 1..n {
            out.push(sample(start + along * (i as f32 * GROUND_STEP), normal));
        }
        // The corner at the side's end: sweep from this side's normal to the next's.
        let corner = start + along * (2.0 * h);
        for s in 0..=ARC_STEPS {
            let t = s as f32 / ARC_STEPS as f32;
            out.push(sample(
                corner,
                Quat::from_rotation_y(-FRAC_PI_2 * t) * normal,
            ));
        }
    }
    out
}

fn sample(base: Vec3, normal: Vec3) -> EdgeSample {
    let normal = normal.normalize();
    EdgeSample {
        base,
        normal,
        margin: margin_at(base.xz(), normal.xz()),
    }
}

/// How far inside the island's rim a ground point is (m; negative beyond it):
/// the signed distance to the rim polygon through the outline's edge points.
pub fn inside_rim(outline: &[EdgeSample], p: Vec2) -> f32 {
    let n = outline.len();
    let mut inside = false;
    let mut nearest = f32::MAX;
    for i in 0..n {
        let a = outline[i].edge().xz();
        let b = outline[(i + 1) % n].edge().xz();
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
        nearest = nearest.min(p.distance(a + ab * t));
    }
    if inside { nearest } else { -nearest }
}

/// A model standing on the island's margin.
#[derive(Debug, Clone, PartialEq)]
pub struct Decor {
    pub model: &'static str,
    pub transform: Transform,
    /// Blob shadow radius (m).
    pub shadow: f32,
    /// Its footprint radius (m, scaled), for spacing and edge rules.
    pub radius: f32,
}

/// Everything the island is made of.
#[derive(Debug, Default)]
pub struct Island {
    /// The grassy top: the arena square and the margin, one soup.
    pub ground: Geo,
    /// The rounded cliffs under the rim, tapering to a point far below.
    pub skirt: Geo,
    /// Grass tufts, one soup per chunk (Battery preset).
    pub tufts: Vec<Geo>,
    /// Extra tufts for the Plugged-in preset, per chunk.
    pub dense_tufts: Vec<Geo>,
    /// White, yellow and pink flower clusters, per chunk.
    pub flowers: Vec<Geo>,
    pub pebbles: Geo,
    /// Round cartoon bushes on the margin (outlined), per chunk, so the
    /// camera culls far chunks and their outline hulls.
    pub bushes: Vec<Geo>,
    /// Small bushes hugging the solid arena props' feet (outlined; no
    /// collision, and never taller than the prop or far past its footprint).
    pub prop_bushes: Geo,
    /// Soft cartoon clouds below the rim and around the far islands (the far
    /// layer's unlit material), per sector of the sky.
    pub clouds: Vec<Geo>,
    /// Trees, big rocks and stumps on the margin.
    pub decor: Vec<Decor>,
}

impl Island {
    pub fn generate() -> Self {
        let mut rng = Rng::new(0x15_1A_4D);
        let outline = outline();
        let decor = decor(&outline, &mut rng.fork(3));
        let mut tufts = tufts(&outline, 5200, 0.55, &mut rng.fork(4));
        base_clumps(&mut tufts, &outline, &decor, &mut rng.fork(8));
        Island {
            ground: ground(&outline, &mut rng.fork(1)),
            skirt: skirt(&outline, &mut rng.fork(2)),
            bushes: bushes(&outline, &decor, &mut rng.fork(9)),
            prop_bushes: prop_bushes(&mut rng.fork(10)),
            clouds: clouds(&outline, &mut rng.fork(11)),
            decor,
            tufts,
            dense_tufts: tufts_dense(&outline, &mut rng.fork(5)),
            flowers: flowers(&outline, &mut rng.fork(6)),
            pebbles: pebbles(&outline, &mut rng.fork(7)),
        }
    }

    /// Triangles drawn on the Battery preset (models aside).
    pub fn triangles(&self) -> usize {
        [&self.ground, &self.skirt, &self.pebbles, &self.prop_bushes]
            .iter()
            .map(|g| g.tri_count())
            .sum::<usize>()
            + [&self.tufts, &self.flowers, &self.bushes, &self.clouds]
                .iter()
                .flat_map(|v| v.iter())
                .map(Geo::tri_count)
                .sum::<usize>()
    }
}

// ---------------------------------------------------------------------------
// The top
// ---------------------------------------------------------------------------

/// Grass colour at a ground point: soft darker patches and lighter streaks,
/// and a darker lip toward the rim (`to_rim` metres away).
fn grass_color(p: Vec2, to_rim: f32) -> Rgba {
    let grass = lin(cartoon::GRASS);
    let dark = mix(grass, lin(cartoon::GRASS_SHADOW), 0.3);
    let patches = fbm2(p.x * 0.07 + 3.0, p.y * 0.07 - 5.0, 41);
    let mut c = mix(grass, dark, smoothstep(0.48, 0.76, patches) * 0.8);
    let streaks = fbm2(p.x * 0.16 - 7.0, p.y * 0.05 + 2.0, 42);
    c = mix(c, shade(grass, 1.08), smoothstep(0.6, 0.85, streaks) * 0.6);
    let lip = 1.0 - smoothstep(0.0, 1.6, to_rim);
    shade(c, 1.0 - 0.1 * lip)
}

fn ground(outline: &[EdgeSample], rng: &mut Rng) -> Geo {
    let mut g = Geo::default();
    let up = Vec3::Y;
    let jitter = |rng: &mut Rng| 1.0 + (rng.next_f32() - 0.5) * 0.03;
    // The arena square: a 2 m grid, flat at y = 0 (the collision plane).
    let n = (2.0 * ARENA_HALF / GROUND_STEP).round() as i32;
    let at = |i: i32, j: i32| {
        Vec3::new(
            -ARENA_HALF + i as f32 * GROUND_STEP,
            0.0,
            -ARENA_HALF + j as f32 * GROUND_STEP,
        )
    };
    for i in 0..n {
        for j in 0..n {
            let (a, b, c, d) = (at(i, j), at(i, j + 1), at(i + 1, j + 1), at(i + 1, j));
            let k = jitter(rng);
            let col = |p: Vec3| shade(grass_color(p.xz(), BASE_MARGIN), k);
            // Counter-clockwise from above: a (x0,z0) → b (x0,z1) → c (x1,z1).
            g.tri_raw([a, b, c], [up; 3], [col(a), col(b), col(c)]);
            g.tri_raw([a, c, d], [up; 3], [col(a), col(c), col(d)]);
        }
    }
    // The margin ring: rows from the arena's edge out to the rim.
    let rows = [0.0, 0.3, 0.62, 1.0];
    let count = outline.len();
    let p = |s: &EdgeSample, t: f32| s.base + s.normal * (s.margin * t);
    for i in 0..count {
        let (s0, s1) = (&outline[i], &outline[(i + 1) % count]);
        for r in 0..rows.len() - 1 {
            let (t0, t1) = (rows[r], rows[r + 1]);
            let (a, b, c, d) = (p(s0, t0), p(s1, t0), p(s1, t1), p(s0, t1));
            let k = jitter(rng);
            let col = |q: Vec3, s: &EdgeSample, t: f32| {
                shade(grass_color(q.xz(), s.margin * (1.0 - t)), k)
            };
            let (ca, cb) = (col(a, s0, t0), col(b, s1, t0));
            let (cc, cd) = (col(c, s1, t1), col(d, s0, t1));
            push_up(&mut g, [a, d, c], [ca, cd, cc]);
            push_up(&mut g, [a, c, b], [ca, cc, cb]);
        }
    }
    g
}

/// A flat triangle facing +Y (wound to face up whatever the input order);
/// degenerate ones (the corner fans' first row) are skipped.
fn push_up(g: &mut Geo, p: [Vec3; 3], c: [Rgba; 3]) {
    let n = (p[1] - p[0]).cross(p[2] - p[0]);
    if n.length_squared() < 1e-10 {
        return;
    }
    let up = Vec3::Y;
    if n.y >= 0.0 {
        g.tri_raw(p, [up; 3], c);
    } else {
        g.tri_raw([p[0], p[2], p[1]], [up; 3], [c[0], c[2], c[1]]);
    }
}

// ---------------------------------------------------------------------------
// The cliff skirt
// ---------------------------------------------------------------------------

/// Ring profile under the rim: (height, outward offset, shrink toward the
/// island's centre, colour band). Bands: 0 grass lip, 1 cliff, 2 underside.
const SKIRT: [(f32, f32, f32, u8); 12] = [
    (0.0, 0.0, 1.0, 0),
    (-0.2, 0.32, 1.0, 0),
    (-0.6, 0.24, 1.0, 0),
    (-0.8, -0.12, 1.0, 1),
    (-2.6, 0.5, 1.0, 1),
    (-5.0, -0.2, 1.0, 1),
    (-7.5, 0.45, 0.99, 1),
    (-10.5, -0.4, 0.97, 1),
    (-15.0, 0.0, 0.86, 2),
    (-22.0, 0.0, 0.64, 2),
    (-31.0, 0.0, 0.36, 2),
    (-42.0, 0.0, 0.06, 2),
];

fn skirt(outline: &[EdgeSample], rng: &mut Rng) -> Geo {
    let count = outline.len();
    let rings = SKIRT.len();
    // Per-column bulge noise so the cliff reads as rounded lumps and columns.
    let lumps: Vec<f32> = (0..count).map(|_| rng.range(-0.35, 0.45)).collect();
    let mut grid = vec![vec![Vec3::ZERO; count]; rings];
    for (i, s) in outline.iter().enumerate() {
        let edge = s.edge();
        let smooth = (lumps[i] + lumps[(i + 1) % count] + lumps[(i + count - 1) % count]) / 3.0;
        for (k, &(y, out, shrink, band)) in SKIRT.iter().enumerate() {
            let wobble = if band == 1 {
                smooth * (1.0 + 0.4 * noise2(i as f32 * 0.9, k as f32 * 1.3, 61))
            } else {
                0.0
            };
            let flat = edge.with_y(0.0) * shrink;
            grid[k][i] = flat + s.normal * (out + wobble) * shrink + Vec3::Y * y;
        }
    }
    // Smooth normals per grid vertex (rounded cartoon rock), from the quads
    // around it.
    let mut normals = vec![vec![Vec3::ZERO; count]; rings];
    for k in 0..rings - 1 {
        for i in 0..count {
            let j = (i + 1) % count;
            let (a, b, c, d) = (grid[k][i], grid[k][j], grid[k + 1][j], grid[k + 1][i]);
            let n = (b - a).cross(d - a) + (d - c).cross(b - c);
            for (kk, ii) in [(k, i), (k, j), (k + 1, j), (k + 1, i)] {
                normals[kk][ii] += n;
            }
        }
    }
    let grass = lin(cartoon::GRASS);
    let dirt = lin(cartoon::CLIFF_DIRT);
    let groove = shade(dirt, 0.86);
    let under = shade(dirt, 0.8);
    let mut g = Geo::default();
    for k in 0..rings - 1 {
        for i in 0..count {
            let j = (i + 1) % count;
            let band = SKIRT[k].3.max(SKIRT[k + 1].3);
            let color = match band {
                0 => shade(grass, if k == 0 { 1.0 } else { 0.9 }),
                1 if i % 3 == 0 => groove,
                1 => shade(dirt, 1.0 + 0.04 * ((i * 7 + k * 3) % 5) as f32 / 4.0),
                _ => under,
            };
            let v = |kk: usize, ii: usize| (grid[kk][ii], normals[kk][ii].normalize_or(Vec3::Y));
            let (a, b, c, d) = (v(k, i), v(k, j), v(k + 1, j), v(k + 1, i));
            // The outline runs clockwise seen from above and the rings go
            // down, so (a, c, d) and (a, b, c) face outward.
            for [p, q, r] in [[a, c, d], [a, b, c]] {
                if (q.0 - p.0).cross(r.0 - p.0).length_squared() < 1e-10 {
                    continue;
                }
                g.tri_raw([p.0, q.0, r.0], [p.1, q.1, r.1], [color; 3]);
            }
        }
    }
    g
}

// ---------------------------------------------------------------------------
// Margin decor (models)
// ---------------------------------------------------------------------------

/// Tree canopy radius at scale 1 (tree_a.json), and its trunk's root flare.
const TREE_CANOPY: f32 = 2.0;
const TREE_TRUNK: f32 = 1.1;
/// The same for the broad, round tree_b (tree_b.json).
const TREE_B_CANOPY: f32 = 2.45;
const TREE_B_TRUNK: f32 = 1.2;

/// Whether a margin model is a tree.
fn is_tree(model: &str) -> bool {
    model.starts_with("tree_")
}

/// A tree model's root flare radius at scale 1 (m).
fn trunk_radius(model: &str) -> f32 {
    if model == "tree_b" {
        TREE_B_TRUNK
    } else {
        TREE_TRUNK
    }
}

/// One kind of margin model and how it is scattered.
struct DecorKind {
    model: &'static str,
    count: usize,
    scale: (f32, f32),
    /// Footprint radius at scale 1 (m).
    foot: f32,
    /// Blob shadow radius at scale 1 (m).
    shadow: f32,
    /// Minimum spacing to other decor (m).
    spacing: f32,
    /// A framing ring: stands within this many metres past its closest
    /// allowed distance to the arena, so it lines the edge of the view from
    /// anywhere inside (T01, T05, T09, T11).
    ring: Option<f32>,
}

const DECOR: [DecorKind; 10] = [
    // The framing ring first, so it gets the spots along the arena's edge:
    // a few big trees (like the ones the targets paint at the edges of the
    // view), then both tree shapes, mixed.
    DecorKind {
        model: "tree_b",
        count: 6,
        scale: (1.55, 1.85),
        foot: TREE_B_CANOPY,
        shadow: 2.3,
        spacing: 7.5,
        ring: Some(3.0),
    },
    DecorKind {
        model: "tree_a",
        count: 6,
        scale: (1.5, 1.8),
        foot: TREE_CANOPY,
        shadow: 2.1,
        spacing: 7.0,
        ring: Some(3.0),
    },
    DecorKind {
        model: "tree_b",
        count: 12,
        scale: (1.0, 1.3),
        foot: TREE_B_CANOPY,
        shadow: 2.2,
        spacing: 5.6,
        ring: Some(2.6),
    },
    DecorKind {
        model: "tree_a",
        count: 20,
        scale: (1.05, 1.4),
        foot: TREE_CANOPY,
        shadow: 1.9,
        spacing: 5.2,
        ring: Some(2.6),
    },
    DecorKind {
        model: "tree_a",
        count: 12,
        scale: (0.85, 1.25),
        foot: TREE_CANOPY,
        shadow: 1.8,
        spacing: 5.5,
        ring: None,
    },
    DecorKind {
        model: "tree_b",
        count: 8,
        scale: (0.85, 1.2),
        foot: TREE_B_CANOPY,
        shadow: 2.1,
        spacing: 6.0,
        ring: None,
    },
    DecorKind {
        model: "rock_a",
        count: 7,
        scale: (1.5, 2.2),
        foot: 0.95,
        shadow: 1.35,
        spacing: 3.5,
        ring: None,
    },
    DecorKind {
        model: "rock_b",
        count: 7,
        scale: (1.4, 2.0),
        foot: 1.07,
        shadow: 1.45,
        spacing: 3.5,
        ring: None,
    },
    DecorKind {
        model: "stump_a",
        count: 9,
        scale: (0.9, 1.3),
        foot: 0.64,
        shadow: 0.9,
        spacing: 2.5,
        ring: None,
    },
    DecorKind {
        model: "tree_a",
        count: 8,
        scale: (0.7, 0.95),
        foot: TREE_CANOPY,
        shadow: 1.7,
        spacing: 4.5,
        ring: None,
    },
];

fn decor(outline: &[EdgeSample], rng: &mut Rng) -> Vec<Decor> {
    let mut placed: Vec<Decor> = Vec::new();
    let reach = ARENA_HALF + BASE_MARGIN + 5.0;
    for kind in &DECOR {
        let tree = is_tree(kind.model);
        let mut made = 0;
        let mut attempts = 0;
        while made < kind.count && attempts < kind.count * 400 {
            attempts += 1;
            let p = Vec2::new(rng.range(-reach, reach), rng.range(-reach, reach));
            let scale = rng.range(kind.scale.0, kind.scale.1);
            let radius = kind.foot * scale;
            // Trees' canopies stay clear of walls on the arena edge; their
            // trunks, like rocks and stumps, sit well inside the rim.
            let (inner, rim_foot) = if tree {
                (radius + EDGE_CLEARANCE, trunk_radius(kind.model) * scale)
            } else {
                (radius + 0.8, radius)
            };
            let edge = edge_distance(p.x, p.y);
            if edge < inner || inside_rim(outline, p) < rim_foot + 0.9 {
                continue;
            }
            if let Some(band) = kind.ring {
                if edge > inner + band {
                    continue;
                }
            } else if tree && fbm2(p.x * 0.05, p.y * 0.05, 71) < 0.35 {
                // Other trees gather in groves.
                continue;
            }
            let crowded = placed.iter().any(|o| {
                let gap = o.transform.translation.xz().distance(p);
                gap < kind.spacing.max(0.6 * (o.radius + radius))
            });
            if crowded {
                continue;
            }
            placed.push(Decor {
                model: kind.model,
                transform: Transform::from_xyz(p.x, 0.0, p.y)
                    .with_rotation(Quat::from_rotation_y(rng.range(0.0, TAU)))
                    .with_scale(Vec3::splat(scale)),
                shadow: kind.shadow * scale,
                radius,
            });
            made += 1;
        }
    }
    placed
}

// ---------------------------------------------------------------------------
// Clutter: grass tufts, flowers, pebbles (low, never solid)
// ---------------------------------------------------------------------------

/// The clutter chunk a ground point falls in ([`CHUNKS`] × [`CHUNKS`] over
/// [`CHUNK_SPAN`] m, clamped at the sides).
fn chunk(x: f32, z: f32) -> usize {
    let cell = |v: f32| {
        (((v + CHUNK_SPAN / 2.0) / (CHUNK_SPAN / CHUNKS as f32)).floor() as i32)
            .clamp(0, CHUNKS as i32 - 1) as usize
    };
    cell(x) + CHUNKS * cell(z)
}

fn chunks() -> Vec<Geo> {
    vec![Geo::default(); CHUNKS * CHUNKS]
}

/// A random point on the island top at least `rim_gap` inside the rim, and
/// whether it is on the arena floor.
fn island_point(outline: &[EdgeSample], rng: &mut Rng, rim_gap: f32) -> Option<(Vec2, bool)> {
    let reach = ARENA_HALF + BASE_MARGIN + 5.0;
    for _ in 0..64 {
        let p = Vec2::new(rng.range(-reach, reach), rng.range(-reach, reach));
        let on_floor = edge_distance(p.x, p.y) <= 0.0;
        if on_floor && (p.x.abs() > ARENA_HALF - 0.25 || p.y.abs() > ARENA_HALF - 0.25) {
            continue;
        }
        if on_floor || inside_rim(outline, p) > rim_gap {
            return Some((p, on_floor));
        }
    }
    None
}

/// A chunky cartoon grass clump (T01, T09): four to seven wide, pointed
/// blades fanning out of one dark root, their tips catching the light.
fn tuft(geo: &mut Geo, at: Vec3, height: f32, rng: &mut Rng) {
    let blades = 4 + (rng.next_u64() % 4) as usize;
    let tip = mix(
        lin(cartoon::TUFT),
        lin(cartoon::GRASS_LIGHT),
        rng.range(0.0, 0.45),
    );
    let root = mix(lin(cartoon::TUFT), lin(cartoon::GRASS_SHADOW), 1.0);
    let up = Vec3::Y;
    let spin = rng.range(0.0, TAU);
    for b in 0..blades {
        let a = spin + TAU * b as f32 / blades as f32 + rng.range(-0.3, 0.3);
        let out = Vec3::new(a.cos(), 0.0, a.sin());
        let side = out.cross(up);
        let w = height * rng.range(0.42, 0.6);
        let h = height * rng.range(0.65, 1.0);
        let lean = out * h * rng.range(0.3, 0.6);
        let base = at + out * w * 0.25;
        let top = base + lean + Vec3::Y * h;
        geo.tri_raw(
            [base - side * w * 0.5, base + side * w * 0.5, top],
            [up; 3],
            [root, root, shade(tip, rng.range(0.96, 1.12))],
        );
    }
}

fn tufts(outline: &[EdgeSample], count: usize, margin_share: f32, rng: &mut Rng) -> Vec<Geo> {
    let mut chunks = chunks();
    let mut made = 0;
    let mut attempts = 0;
    while made < count && attempts < count * 20 {
        attempts += 1;
        let Some((p, on_floor)) = island_point(outline, rng, 0.4) else {
            continue;
        };
        // Clumped: denser in patches, and along the arena's edge and the
        // island's rim, where the painted lawn grows thickest.
        let patch = smoothstep(0.42, 0.7, fbm2(p.x * 0.11, p.y * 0.11, 81));
        let edge = if on_floor {
            let to_edge = ARENA_HALF - p.x.abs().max(p.y.abs());
            1.0 - smoothstep(0.0, 2.5, to_edge)
        } else {
            1.0 - smoothstep(0.5, 3.0, inside_rim(outline, p))
        };
        let density = if on_floor {
            0.3 + 0.6 * patch + 0.4 * edge
        } else {
            margin_share + 0.4 * patch + 0.3 * edge
        };
        if rng.next_f32() > density {
            continue;
        }
        let height = if on_floor || edge_distance(p.x, p.y) < 0.7 {
            rng.range(0.14, FLOOR_CLUTTER_MAX_HEIGHT - 0.01)
        } else {
            rng.range(0.2, 0.45)
        };
        let at = Vec3::new(p.x, 0.0, p.y);
        tuft(&mut chunks[chunk(p.x, p.y)], at, height, rng);
        made += 1;
    }
    chunks
}

fn tufts_dense(outline: &[EdgeSample], rng: &mut Rng) -> Vec<Geo> {
    tufts(outline, 3200, 0.35, rng)
}

/// Clumps of grass around the foot of every solid prop and margin model, so
/// they sit in the grass instead of on it (T05, T08).
fn base_clumps(chunks: &mut [Geo], outline: &[EdgeSample], decor: &[Decor], rng: &mut Rng) {
    let props = crate::arena::ARENA_PROPS
        .iter()
        .map(|p| (p.position, p.kind.footprint_radius(), true));
    let margin = decor.iter().map(|d| {
        let r = if is_tree(d.model) {
            trunk_radius(d.model) * d.transform.scale.x * 0.55
        } else {
            d.radius
        };
        (d.transform.translation, r, false)
    });
    for (at, radius, on_floor) in props.chain(margin) {
        let n = 8 + (rng.next_u64() % 5) as usize;
        for _ in 0..n {
            let a = rng.range(0.0, TAU);
            let r = radius * rng.range(0.85, 1.2);
            let p = at + Vec3::new(a.cos() * r, 0.0, a.sin() * r);
            let on_arena = edge_distance(p.x, p.z) <= 0.0;
            if (on_arena && (p.x.abs() > ARENA_HALF - 0.25 || p.z.abs() > ARENA_HALF - 0.25))
                || (!on_arena && inside_rim(outline, p.xz()) < 0.8)
            {
                continue;
            }
            let height = if on_floor || edge_distance(p.x, p.z) < 0.7 {
                rng.range(0.16, FLOOR_CLUTTER_MAX_HEIGHT - 0.01)
            } else {
                rng.range(0.25, 0.5)
            };
            tuft(&mut chunks[chunk(p.x, p.z)], p, height, rng);
        }
    }
}

/// A round cartoon bush: a few overlapping faceted puffs, their upward faces
/// blotched with the lighter, yellower foliage as the painted bushes are.
fn bush(geo: &mut Geo, at: Vec3, size: f32, rng: &mut Rng) {
    bush_of(geo, at, size, 3 + (rng.next_u64() % 2) as usize, rng);
}

fn bush_of(geo: &mut Geo, at: Vec3, size: f32, puffs: usize, rng: &mut Rng) {
    let leaf = lin(cartoon::FOLIAGE);
    let sun = lin(cartoon::FOLIAGE_LIGHT);
    let seed = rng.next_u64() as u32;
    for k in 0..puffs {
        let r = size * if k == 0 { 1.0 } else { rng.range(0.6, 0.8) };
        let offset = if k == 0 {
            Vec3::ZERO
        } else {
            let a = rng.range(0.0, TAU);
            Vec3::new(a.cos(), 0.0, a.sin()) * size * rng.range(0.55, 0.85)
        };
        let c = at + offset + Vec3::Y * r * 0.55;
        let mut shape = rng.fork(k as u64 + 31);
        let tone = rng.range(0.95, 1.06);
        blob(
            geo,
            1,
            |v| {
                let v = Vec3::new(v.x, v.y.max(-0.55), v.z);
                c + v * r * Vec3::new(1.0, 0.82, 1.0) * shape.range(0.93, 1.07)
            },
            |n| {
                let c = shade(leaf, tone * (0.97 + 0.08 * n.y.max(0.0)));
                let lit = n.y > 0.45 && noise2(n.x * 2.3 + k as f32 * 3.7, n.z * 2.3, seed) > 0.45;
                if lit { mix(c, sun, 0.8) } else { c }
            },
        );
    }
}

fn bushes(outline: &[EdgeSample], decor: &[Decor], rng: &mut Rng) -> Vec<Geo> {
    let mut geo = chunks();
    let mut placed: Vec<(Vec2, f32)> = decor
        .iter()
        .filter(|d| !is_tree(d.model))
        .map(|d| (d.transform.translation.xz(), d.radius))
        .collect();
    let reach = ARENA_HALF + BASE_MARGIN + 5.0;
    let mut made = 0;
    let mut attempts = 0;
    while made < 110 && attempts < 60_000 {
        attempts += 1;
        let p = Vec2::new(rng.range(-reach, reach), rng.range(-reach, reach));
        let size = rng.range(0.6, 1.1);
        let foot = size * 1.8;
        let edge = edge_distance(p.x, p.y);
        if edge < foot + EDGE_CLEARANCE || inside_rim(outline, p) < foot + 0.3 {
            continue;
        }
        // Bushes gather at tree and rock bases, along the rim, and in a
        // loose hedge just beyond the arena's edge (T01, T11).
        let near_tree = decor.iter().any(|d| {
            let reach = if is_tree(d.model) {
                3.2 * d.transform.scale.x
            } else {
                d.radius + 1.6
            };
            d.transform.translation.xz().distance(p) < reach
        });
        let near_rim = inside_rim(outline, p) < foot + 2.0;
        let hedge = edge < foot + EDGE_CLEARANCE + 1.2;
        if !(near_tree || near_rim || hedge) {
            continue;
        }
        if placed.iter().any(|(q, r)| q.distance(p) < r + foot) {
            continue;
        }
        placed.push((p, foot));
        bush(
            &mut geo[chunk(p.x, p.y)],
            Vec3::new(p.x, 0.0, p.y),
            size,
            rng,
        );
        made += 1;
    }
    geo
}

/// A little five-petal flower just above the grass, tipped a little toward a
/// random side so it catches the eye (T01, T11): rounded diamond petals in
/// `petal` round a raised `centre` (white daisies, yellow buttercups, pink
/// blossoms). 13 triangles.
fn flower(geo: &mut Geo, at: Vec3, size: f32, petal: Rgba, centre: Rgba, rng: &mut Rng) {
    let tip_dir = rng.range(0.0, TAU);
    let up = Quat::from_axis_angle(
        Vec3::new(tip_dir.cos(), 0.0, tip_dir.sin()),
        rng.range(0.2, 0.55),
    ) * Vec3::Y;
    let t = up.any_orthonormal_vector();
    let b = up.cross(t);
    let spin = rng.range(0.0, TAU);
    let c = at + Vec3::Y * rng.range(0.06, 0.12);
    let dir = |a: f32| t * a.cos() + b * a.sin();
    for k in 0..5 {
        let a = spin + TAU * k as f32 / 5.0;
        let (d, side) = (dir(a), dir(a + FRAC_PI_2));
        let root = c + d * size * 0.18;
        let mid = c + d * size * 0.62 + up * size * 0.08;
        let tip = c + d * size + up * size * 0.14;
        let (l, r) = (mid - side * size * 0.3, mid + side * size * 0.3);
        geo.tri_raw([root, r, tip], [up; 3], [petal; 3]);
        geo.tri_raw([root, tip, l], [up; 3], [petal; 3]);
    }
    let ring: Vec<Vec3> = (0..3)
        .map(|k| c + up * size * 0.06 + dir(spin + TAU * k as f32 / 3.0) * size * 0.3)
        .collect();
    geo.tri_raw([ring[0], ring[2], ring[1]], [up; 3], [centre; 3]);
    geo.tri_raw([ring[0], ring[1], ring[2]], [up; 3], [centre; 3]);
    geo.tri_raw(
        [ring[0], ring[1], c + up * size * 0.16],
        [up; 3],
        [shade(centre, 1.1); 3],
    );
}

fn flowers(outline: &[EdgeSample], rng: &mut Rng) -> Vec<Geo> {
    let mut geo = chunks();
    let kinds = [
        (lin(cartoon::GLOVE_WHITE), lin(cartoon::STAR_GOLD)),
        (lin(cartoon::GLOVE_WHITE), lin(cartoon::STAR_GOLD)),
        (lin(cartoon::SPELL_GOLD), lin(cartoon::STAR_GOLD)),
        (lin(cartoon::FLOWER_PINK), lin(cartoon::SPELL_GOLD)),
    ];
    // Flowers grow in little clusters of one kind.
    for _ in 0..260 {
        let Some((p, on_floor)) = island_point(outline, rng, 0.6) else {
            continue;
        };
        let (petal, centre) = kinds[(rng.next_u64() % kinds.len() as u64) as usize];
        let n = 3 + (rng.next_u64() % 4) as usize;
        for _ in 0..n {
            let q = p + Vec2::new(rng.range(-0.6, 0.6), rng.range(-0.6, 0.6));
            let off_floor = q.x.abs() > ARENA_HALF - 0.2 || q.y.abs() > ARENA_HALF - 0.2;
            if (on_floor && off_floor) || (!on_floor && inside_rim(outline, q) < 0.4) {
                continue;
            }
            flower(
                &mut geo[chunk(q.x, q.y)],
                Vec3::new(q.x, 0.0, q.y),
                rng.range(0.08, 0.12),
                petal,
                centre,
                rng,
            );
        }
    }
    geo
}

/// Two or three small bushes hugging the foot of every solid arena prop
/// (T01, T05): tucked mostly under the prop's own footprint, no taller than
/// [`PROP_BUSH_MAX_HEIGHT`], so they dress the cover without changing it.
fn prop_bushes(rng: &mut Rng) -> Geo {
    let mut geo = Geo::default();
    for prop in crate::arena::ARENA_PROPS {
        let foot = prop.kind.footprint_radius();
        // A two-puff bush reaches about 1.75 × its size from its root and
        // stands about 1.45 × its size tall.
        let largest = 0.4_f32
            .min((PROP_BUSH_REACH + 0.4 * foot) / 1.75)
            .min(0.9 * prop.kind.height().min(PROP_BUSH_MAX_HEIGHT) / 1.45);
        let n = 2 + (rng.next_u64() % 2) as usize;
        let spin = rng.range(0.0, TAU);
        for k in 0..n {
            let a = spin + TAU * k as f32 / n as f32 + rng.range(-0.4, 0.4);
            let size = largest * rng.range(0.8, 1.0);
            let r = foot * rng.range(0.5, 0.6);
            let at = prop.position + Vec3::new(a.cos() * r, 0.0, a.sin() * r);
            bush_of(&mut geo, at, size, 2, rng);
        }
    }
    geo
}

/// A soft cartoon cloud: overlapping domes, flat underneath, their colour
/// running per vertex from the lavender `cloud` in the hollows and under the
/// puffs to the pale `cloud_light` on their sunlit tops (T01, T03, T11).
///
/// Each puff is a dome of `segments` sides: 12 for the banks close under the
/// rim (120 triangles a puff), 8 far away (64).
fn cloud(geo: &mut Geo, centre: Vec3, radius: f32, puffs: usize, segments: usize, rng: &mut Rng) {
    let segments = segments.max(3);
    // Latitudes from the top down to a little past the equator.
    const LATS: [f32; 6] = [0.0, 0.42, 0.8, 1.15, 1.45, 1.75];
    let (body, top) = (lin(cartoon::CLOUD), lin(cartoon::CLOUD_LIGHT));
    for k in 0..puffs {
        // The middle puffs are the biggest and highest.
        let f = if puffs > 1 {
            k as f32 / (puffs - 1) as f32 * 2.0 - 1.0
        } else {
            0.0
        };
        let along = f * radius * 0.85;
        let r = radius * rng.range(0.42, 0.6) * (1.0 - 0.35 * f.abs());
        let c = centre
            + Vec3::new(along, r * 0.1 + (1.0 - f.abs()) * radius * 0.12, 0.0)
            + Vec3::new(0.0, 0.0, rng.range(-0.3, 0.3) * radius);
        let squash = rng.range(0.62, 0.78);
        let spin = rng.range(0.0, TAU);
        let ring = |lat: f32| -> Vec<(Vec3, Rgba)> {
            (0..segments)
                .map(|i| {
                    let a = spin + TAU * i as f32 / segments as f32;
                    let n = Vec3::new(lat.sin() * a.cos(), lat.cos(), lat.sin() * a.sin());
                    let p = c + Vec3::new(n.x * r, n.y.max(-0.25) * r * squash, n.z * r);
                    let t = smoothstep(-0.2, 0.8, n.y + (c.y - centre.y) / radius);
                    (p, mix(body, top, t))
                })
                .collect()
        };
        let apex = (c + Vec3::Y * r * squash, top);
        let rings: Vec<Vec<(Vec3, Rgba)>> = LATS[1..].iter().map(|&l| ring(l)).collect();
        let mut face = |a: (Vec3, Rgba), b: (Vec3, Rgba), d: (Vec3, Rgba)| {
            let n = (b.0 - a.0).cross(d.0 - a.0).normalize_or_zero();
            if n != Vec3::ZERO {
                geo.tri_raw([a.0, b.0, d.0], [n; 3], [a.1, b.1, d.1]);
            }
        };
        for i in 0..segments {
            let j = (i + 1) % segments;
            face(apex, rings[0][j], rings[0][i]);
            for w in rings.windows(2) {
                let (u, l) = (&w[0], &w[1]);
                face(u[i], u[j], l[j]);
                face(u[i], l[j], l[i]);
            }
        }
        // A flat underside.
        let last = rings.last().unwrap();
        let under = (c - Vec3::Y * r * squash * 0.25, body);
        for i in 0..segments {
            face(under, last[i], last[(i + 1) % segments]);
        }
    }
}

/// Sky sectors the clouds are split into (for culling).
const CLOUD_SECTORS: usize = 8;

fn sector(p: Vec3) -> usize {
    let a = p.x.atan2(-p.z).rem_euclid(TAU);
    ((a / TAU * CLOUD_SECTORS as f32) as usize).min(CLOUD_SECTORS - 1)
}

/// The cloud sea (T01, T03, T10, T11): banks just under the rim, where the
/// cliffs drop away; a bank round the station's rock; puffs wrapped round the
/// middle of every far island's rocky underside; and a low band far out that
/// peeks over the rim at the horizon.
/// Nothing rises above the island top near it, so no cloud ever sits on the
/// grass or in front of the far view's landmarks.
fn clouds(outline: &[EdgeSample], rng: &mut Rng) -> Vec<Geo> {
    let mut geo = vec![Geo::default(); CLOUD_SECTORS];
    fn put(geo: &mut [Geo], c: Vec3, r: f32, puffs: usize, rng: &mut Rng) {
        let yaw = rng.range(0.0, TAU);
        let mut one = Geo::default();
        let segments = if c.xz().length() < 90.0 { 12 } else { 8 };
        cloud(&mut one, Vec3::ZERO, r, puffs, segments, rng);
        let t = Transform::from_translation(c).with_rotation(Quat::from_rotation_y(yaw));
        geo[sector(c)].append(&one, &t);
    }
    // Banks under the rim, evenly round the island.
    let banks = 22;
    for k in 0..banks {
        let s =
            &outline[(k * outline.len() / banks + (rng.next_u64() % 3) as usize) % outline.len()];
        let out = rng.range(10.0, 42.0);
        let r = rng.range(6.0, 11.0);
        // Deep under the rim close in; the farthest rise toward the horizon.
        let y = rng.range(-24.0, -12.0) + (out - 22.0).max(0.0) * 0.3;
        put(&mut geo, s.edge() + s.normal * out + Vec3::Y * y, r, 5, rng);
    }
    let far = crate::far::FarLayout::default();
    // A bank round the middle of the station's rock (it hangs about 175 m
    // under its platform), on the side facing the arena (T01, T10).
    let station = far.station.position;
    let toward = (-station.with_y(0.0)).normalize_or(Vec3::Z);
    let side = toward.cross(Vec3::Y);
    for k in 0..7 {
        let across = (k as f32 / 6.0 * 2.0 - 1.0) * 210.0 + rng.range(-20.0, 20.0);
        let c = station.with_y(0.0)
            + side * across
            + toward * rng.range(120.0, 170.0)
            + Vec3::Y * rng.range(35.0, 85.0);
        put(&mut geo, c, rng.range(45.0, 65.0), 5, rng);
    }
    // Round the far islands' undersides.
    for island in &far.islands {
        let p = island.piece.position;
        let s = island.piece.scale;
        let out = p.with_y(0.0).normalize_or(Vec3::Z);
        let side = out.cross(Vec3::Y);
        let c = p - Vec3::Y * 30.0 * s + side * rng.range(-8.0, 8.0) * s + out * 6.0 * s;
        put(&mut geo, c, rng.range(17.0, 24.0) * s, 5, rng);
    }
    // A low band far out, at the horizon.
    for k in 0..12 {
        let a = TAU * (k as f32 + rng.range(0.1, 0.9)) / 12.0;
        let d = rng.range(270.0, 380.0);
        let c = Vec3::new(a.sin() * d, rng.range(-38.0, -24.0), -a.cos() * d);
        put(&mut geo, c, rng.range(30.0, 44.0), 4, rng);
    }
    geo
}

fn pebbles(outline: &[EdgeSample], rng: &mut Rng) -> Geo {
    let mut geo = Geo::default();
    let rock = lin(cartoon::ROCK);
    for made in 0..220u64 {
        let Some((p, _)) = island_point(outline, rng, 0.5) else {
            continue;
        };
        let r = rng.range(0.05, 0.14);
        let squash = rng.range(0.45, 0.7);
        let tone = rng.range(0.9, 1.05);
        let mut shape = rng.fork(made);
        blob(
            &mut geo,
            0,
            |v| {
                Vec3::new(
                    p.x + v.x * r * shape.range(0.8, 1.2),
                    (v.y * squash + squash * 0.4) * r,
                    p.y + v.z * r * shape.range(0.8, 1.2),
                )
            },
            |n| shade(rock, tone * (0.92 + 0.1 * n.y)),
        );
    }
    geo
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generated() -> &'static Island {
        static ISLAND: std::sync::OnceLock<Island> = std::sync::OnceLock::new();
        ISLAND.get_or_init(Island::generate)
    }

    #[test]
    fn the_island_top_is_flat_at_zero_and_faces_up() {
        let island = generated();
        for (i, v) in island.ground.vertices().enumerate() {
            assert_eq!(v.y, 0.0, "the island top is the collision plane: {v}");
            assert_eq!(island.ground.normals[i], [0.0, 1.0, 0.0]);
        }
        for t in island.ground.positions.chunks(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(Vec3::from_array);
            assert!((b - a).cross(c - a).y > 0.0, "a ground triangle faces down");
        }
    }

    #[test]
    fn the_top_covers_the_arena_and_a_margin_and_meets_the_skirt() {
        let island = generated();
        let outline = outline();
        let (lo, hi) = island.ground.bounds().unwrap();
        assert!(lo.x < -ARENA_HALF - MIN_MARGIN + 0.01 && hi.x > ARENA_HALF + MIN_MARGIN - 0.01);
        for s in &outline {
            assert!(s.margin >= MIN_MARGIN - 1e-4);
            assert!(edge_distance(s.edge().x, s.edge().z) >= MIN_MARGIN - 0.05);
        }
        // The close edge on the east side comes within about a metre;
        // elsewhere the margin is wide enough for trees.
        let east = outline
            .iter()
            .filter(|s| s.normal.x > 0.99 && (0.0..8.0).contains(&s.base.z))
            .map(|s| s.margin)
            .fold(0.0, f32::max);
        assert!(east < 1.2, "close edge margin {east}");
        let widest = outline.iter().map(|s| s.margin).fold(0.0, f32::max);
        assert!(widest > 10.0, "widest margin {widest}");
        // The skirt's top ring is the ground's rim.
        for s in &outline {
            let p = s.edge();
            assert!(
                island.skirt.vertices().any(|v| v.distance(p) < 1e-4),
                "the skirt starts at the rim {p}"
            );
            assert!(island.ground.vertices().any(|v| v.distance(p) < 1e-4));
        }
    }

    #[test]
    fn the_skirt_drops_away_below_the_rim_and_faces_out() {
        let island = generated();
        let (lo, hi) = island.skirt.bounds().unwrap();
        assert!(hi.y <= 1e-4, "no cliff pokes above the island top");
        assert!(lo.y < -30.0, "the island hangs deep into space");
        for v in island.skirt.vertices() {
            // Its lumps may tuck back under the rim, never into the arena.
            assert!(
                edge_distance(v.x, v.z) > 0.1 || v.y < -8.0,
                "cliff inside the playable area at {v}"
            );
        }
        // The cliff faces point outward.
        let (mut outward, mut total) = (0, 0);
        for (t, n) in island
            .skirt
            .positions
            .chunks(3)
            .zip(island.skirt.normals.chunks(3))
        {
            let c = t.iter().map(|p| Vec3::from_array(*p)).sum::<Vec3>() / 3.0;
            let face = (Vec3::from_array(t[1]) - Vec3::from_array(t[0]))
                .cross(Vec3::from_array(t[2]) - Vec3::from_array(t[0]));
            if c.y > -8.0 {
                total += 1;
                let n = Vec3::from_array(n[0]);
                if n.xz().dot(c.xz()) > 0.0 && face.dot(n) > 0.0 {
                    outward += 1;
                }
            }
        }
        assert!(outward as f32 > total as f32 * 0.95, "{outward} of {total}");
    }

    #[test]
    fn clutter_is_low_on_the_floor_and_never_leaves_the_island() {
        let island = generated();
        let outline = outline();
        let clutter: Vec<&Geo> = island
            .tufts
            .iter()
            .chain(&island.dense_tufts)
            .chain(&island.flowers)
            .chain([&island.pebbles])
            .collect();
        assert!(clutter.iter().all(|g| g.tri_count() < 20_000));
        for geo in clutter {
            for v in geo.vertices() {
                if edge_distance(v.x, v.z) <= 0.0 {
                    assert!(v.y <= FLOOR_CLUTTER_MAX_HEIGHT, "clutter too tall: {v}");
                } else {
                    assert!(v.y <= 0.6, "margin clutter too tall: {v}");
                    assert!(
                        inside_rim(&outline, v.xz()) > 0.0,
                        "clutter off the rim: {v}"
                    );
                }
                assert!(v.y >= -0.05);
            }
        }
    }

    #[test]
    fn the_lawn_is_dense_with_tufts_and_three_kinds_of_flowers() {
        let island = generated();
        // Tufts on the arena floor near the spawn view: at least one per
        // square metre, like the painted lawn.
        let (lo, hi) = (Vec2::new(-10.0, -6.0), Vec2::new(14.0, 18.0));
        let roots = island
            .tufts
            .iter()
            .flat_map(|g| g.positions.chunks(3))
            .filter(|t| {
                let p = Vec3::from_array(t[0]).xz();
                p.cmpge(lo).all() && p.cmple(hi).all()
            })
            .count();
        let tufts = roots as f32 / 5.5; // about 5.5 blades a tuft
        let area = (hi - lo).x * (hi - lo).y;
        assert!(tufts / area > 1.0, "{} tufts per m²", tufts / area);
        // White, yellow and pink petals.
        let petals: Vec<Rgba> = island
            .flowers
            .iter()
            .flat_map(|g| g.colors.iter().copied())
            .collect();
        for (name, color) in [
            ("white", cartoon::GLOVE_WHITE),
            ("yellow", cartoon::SPELL_GOLD),
            ("pink", cartoon::FLOWER_PINK),
        ] {
            let c = lin(color);
            assert!(
                petals.iter().filter(|p| **p == c).count() > 300,
                "{name} flowers"
            );
        }
    }

    #[test]
    fn prop_bushes_hug_the_solid_props_and_stay_low() {
        let island = generated();
        assert!(island.prop_bushes.tri_count() > 1000);
        for v in island.prop_bushes.vertices() {
            assert!(v.y <= PROP_BUSH_MAX_HEIGHT, "a prop bush too tall at {v}");
            let near = crate::arena::ARENA_PROPS
                .iter()
                .any(|p| p.footprint_distance(v.xz()) <= PROP_BUSH_REACH);
            assert!(near, "a bush strays from the props at {v}");
            assert!(edge_distance(v.x, v.z) <= 0.0, "inside the arena");
        }
        // Every prop is dressed.
        for prop in crate::arena::ARENA_PROPS {
            assert!(
                island
                    .prop_bushes
                    .vertices()
                    .any(|v| v.xz().distance(prop.position.xz()) < 1.5),
                "{prop:?}"
            );
        }
    }

    #[test]
    fn framing_trees_ring_the_arena_on_every_side() {
        let island = generated();
        let ring: Vec<&Decor> = island
            .decor
            .iter()
            .filter(|d| {
                is_tree(d.model)
                    && edge_distance(d.transform.translation.x, d.transform.translation.z)
                        < d.radius + EDGE_CLEARANCE + 2.7
            })
            .collect();
        assert!(ring.len() >= 24, "{} framing trees", ring.len());
        // North, east, south and west of the arena all have some (the east's
        // close edge has room for fewer).
        for (dir, need) in [(-Vec2::Y, 5), (Vec2::X, 2), (Vec2::Y, 5), (-Vec2::X, 5)] {
            let n = ring
                .iter()
                .filter(|d| d.transform.translation.xz().normalize().dot(dir) > 0.7)
                .count();
            assert!(n >= need, "{n} framing trees toward {dir}");
        }
    }

    #[test]
    fn clouds_hang_below_the_island_and_clear_of_the_arena() {
        let island = generated();
        assert_eq!(island.clouds.len(), CLOUD_SECTORS);
        let mut total = 0;
        let outline = outline();
        for v in island.clouds.iter().flat_map(|g| g.vertices()) {
            total += 1;
            // Never over the island's top: past its rim, or well under it.
            assert!(
                inside_rim(&outline, v.xz()) < 0.0 || v.y < -6.0,
                "a cloud over the island at {v}"
            );
            // Near the island they stay under its top, so none sits by the
            // grass; far out they may peek over the rim at the horizon.
            if edge_distance(v.x, v.z) < 25.0 {
                assert!(v.y < -2.0, "a cloud too high near the rim at {v}");
            }
        }
        assert!(total > 0);
        // Some bank under the rim close enough to see over the edge (T11), and
        // some far out round the far islands.
        let verts: Vec<Vec3> = island.clouds.iter().flat_map(|g| g.vertices()).collect();
        assert!(verts.iter().any(|v| edge_distance(v.x, v.z) < 25.0));
        assert!(verts.iter().any(|v| v.xz().length() > 150.0 && v.y > 0.0));
    }

    #[test]
    fn margin_decor_stays_off_the_arena_and_on_the_island() {
        let island = generated();
        let outline = outline();
        let trees = island.decor.iter().filter(|d| is_tree(d.model)).count();
        assert!(trees >= 28, "{trees} trees");
        assert!(island.decor.iter().any(|d| d.model == "tree_b"));
        assert!(island.decor.iter().any(|d| d.model.starts_with("rock")));
        assert!(island.decor.iter().any(|d| d.model == "stump_a"));
        for d in &island.decor {
            let p = d.transform.translation;
            assert_eq!(p.y, 0.0);
            assert!(
                edge_distance(p.x, p.z) >= d.radius + EDGE_CLEARANCE - 1e-3,
                "{} at {p} (radius {}) reaches the arena",
                d.model,
                d.radius
            );
            assert!(
                inside_rim(&outline, p.xz()) > 0.5,
                "{} at {p} off the rim",
                d.model
            );
        }
    }

    #[test]
    fn bushes_grow_on_the_margin_only() {
        let island = generated();
        let outline = outline();
        let tris: usize = island.bushes.iter().map(Geo::tri_count).sum();
        assert!(tris > 10_000, "{tris}");
        for v in island.bushes.iter().flat_map(|g| g.vertices()) {
            assert!(
                edge_distance(v.x, v.z) >= EDGE_CLEARANCE - 1e-3,
                "a bush reaches the arena at {v}"
            );
            if v.y < 0.2 {
                assert!(
                    inside_rim(&outline, v.xz()) > -0.3,
                    "a bush overhangs the rim at {v}"
                );
            }
        }
    }

    #[test]
    fn the_island_is_deterministic_and_cheap() {
        let a = Island::generate();
        let b = generated();
        assert_eq!(a.ground.positions, b.ground.positions);
        assert_eq!(a.skirt.colors, b.skirt.colors);
        assert_eq!(a.decor, b.decor);
        let tris = a.triangles();
        assert!(tris < ISLAND_TRIANGLE_BUDGET, "island has {tris} triangles");
        assert_eq!(a.tufts.len(), CHUNKS * CHUNKS);
    }
}
