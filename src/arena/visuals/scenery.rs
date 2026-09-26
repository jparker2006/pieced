//! The floating island, generated in code (targets T01, T11): the grassy top
//! (the 48 m arena square plus a margin ring), rounded cartoon cliffs dropping
//! into space under its edge, low grass tufts, flowers and pebbles, and where
//! the margin's trees, big rocks and stumps (Blender models) stand. Everything
//! here is pure, deterministic CPU work ending in a few merged meshes and a
//! list of model placements, so it is tested headless and costs almost nothing
//! per frame. Only near scenery lives here; the sky slice owns the far view.

use super::geo::{Geo, Rgba, blob, fbm2, lin, mix, noise2, shade, smoothstep};
use crate::{palette::cartoon, rng::Rng, shared::ARENA_HALF};
use bevy::prelude::*;
use std::f32::consts::{FRAC_PI_2, TAU};

/// Margin models keep at least this far outside the playable square, so walls
/// built on the arena edge never clip into scenery.
pub const EDGE_CLEARANCE: f32 = 0.35;
/// Tallest grass, flower or pebble allowed on the playable floor (m).
pub const FLOOR_CLUTTER_MAX_HEIGHT: f32 = 0.26;
/// The island's edge is never closer than this to the arena (m).
pub const MIN_MARGIN: f32 = 2.4;
/// The typical width of the island's margin beyond the arena (m).
pub const BASE_MARGIN: f32 = 9.0;
/// Spacing of the island top's grid (m); the outline samples the arena's
/// sides at the same spacing so the two meshes share their seam vertices.
pub const GROUND_STEP: f32 = 2.0;
/// Where the island's edge comes closest to the arena: the east side between
/// these z (the island-edge view, T11, looks along it).
pub const CLOSE_EDGE_Z: (f32, f32) = (-4.0, 12.0);

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
    // behind the barrier.
    if dir.x > 0.5 {
        let (z0, z1) = CLOSE_EDGE_Z;
        let t = smoothstep(z0 - 6.0, z0, base.y) * (1.0 - smoothstep(z1, z1 + 6.0, base.y));
        m += (MIN_MARGIN + 0.3 - m) * t;
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
    /// Grass tufts, one soup per quadrant (Battery preset).
    pub tufts: Vec<Geo>,
    /// Extra tufts for the Plugged-in preset, per quadrant.
    pub dense_tufts: Vec<Geo>,
    pub flowers: Geo,
    pub pebbles: Geo,
    /// Round cartoon bushes on the margin (outlined).
    pub bushes: Geo,
    /// Trees, big rocks and stumps on the margin.
    pub decor: Vec<Decor>,
}

impl Island {
    pub fn generate() -> Self {
        let mut rng = Rng::new(0x15_1A_4D);
        let outline = outline();
        let decor = decor(&outline, &mut rng.fork(3));
        let mut tufts = tufts(&outline, 2000, 0.45, &mut rng.fork(4));
        base_clumps(&mut tufts, &outline, &decor, &mut rng.fork(8));
        Island {
            ground: ground(&outline, &mut rng.fork(1)),
            skirt: skirt(&outline, &mut rng.fork(2)),
            bushes: bushes(&outline, &decor, &mut rng.fork(9)),
            decor,
            tufts,
            dense_tufts: tufts_dense(&outline, &mut rng.fork(5)),
            flowers: flowers(&outline, &mut rng.fork(6)),
            pebbles: pebbles(&outline, &mut rng.fork(7)),
        }
    }

    /// Triangles drawn on the Battery preset (models aside).
    pub fn triangles(&self) -> usize {
        [
            &self.ground,
            &self.skirt,
            &self.flowers,
            &self.pebbles,
            &self.bushes,
        ]
        .iter()
        .map(|g| g.tri_count())
        .sum::<usize>()
            + self.tufts.iter().map(Geo::tri_count).sum::<usize>()
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
}

const DECOR: [DecorKind; 5] = [
    DecorKind {
        model: "tree_a",
        count: 24,
        scale: (0.85, 1.25),
        foot: TREE_CANOPY,
        shadow: 1.8,
        spacing: 5.5,
    },
    DecorKind {
        model: "rock_a",
        count: 5,
        scale: (1.5, 2.2),
        foot: 0.95,
        shadow: 1.35,
        spacing: 3.5,
    },
    DecorKind {
        model: "rock_b",
        count: 5,
        scale: (1.4, 2.0),
        foot: 1.07,
        shadow: 1.45,
        spacing: 3.5,
    },
    DecorKind {
        model: "stump_a",
        count: 6,
        scale: (0.9, 1.3),
        foot: 0.64,
        shadow: 0.9,
        spacing: 2.5,
    },
    DecorKind {
        model: "tree_a",
        count: 8,
        scale: (0.7, 0.95),
        foot: TREE_CANOPY,
        shadow: 1.7,
        spacing: 4.5,
    },
];

fn decor(outline: &[EdgeSample], rng: &mut Rng) -> Vec<Decor> {
    let mut placed: Vec<Decor> = Vec::new();
    let reach = ARENA_HALF + BASE_MARGIN + 5.0;
    for kind in &DECOR {
        let tree = kind.model == "tree_a";
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
                (radius + EDGE_CLEARANCE, TREE_TRUNK * scale)
            } else {
                (radius + 0.8, radius)
            };
            if edge_distance(p.x, p.y) < inner || inside_rim(outline, p) < rim_foot + 0.9 {
                continue;
            }
            // Trees gather in groves.
            if tree && fbm2(p.x * 0.05, p.y * 0.05, 71) < 0.35 {
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

fn quadrant(x: f32, z: f32) -> usize {
    (x >= 0.0) as usize + 2 * (z >= 0.0) as usize
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

/// A cartoon grass clump: 3–5 pointed blades fanning out of one root.
fn tuft(geo: &mut Geo, at: Vec3, height: f32, rng: &mut Rng) {
    let blades = 3 + (rng.next_u64() % 3) as usize;
    let tip = lin(cartoon::TUFT);
    let root = mix(tip, lin(cartoon::GRASS_SHADOW), 0.7);
    let up = Vec3::Y;
    let spin = rng.range(0.0, TAU);
    for b in 0..blades {
        let a = spin + TAU * b as f32 / blades as f32 + rng.range(-0.3, 0.3);
        let out = Vec3::new(a.cos(), 0.0, a.sin());
        let side = out.cross(up);
        let w = height * rng.range(0.28, 0.4);
        let h = height * rng.range(0.7, 1.0);
        let lean = out * h * rng.range(0.25, 0.55);
        let base = at + out * w * 0.3;
        let top = base + lean + Vec3::Y * h;
        geo.tri_raw(
            [base - side * w * 0.5, base + side * w * 0.5, top],
            [up; 3],
            [root, root, shade(tip, rng.range(0.98, 1.1))],
        );
    }
}

fn tufts(outline: &[EdgeSample], count: usize, margin_share: f32, rng: &mut Rng) -> Vec<Geo> {
    let mut chunks = vec![Geo::default(); 4];
    let mut made = 0;
    let mut attempts = 0;
    while made < count && attempts < count * 20 {
        attempts += 1;
        let Some((p, on_floor)) = island_point(outline, rng, 0.4) else {
            continue;
        };
        // Clumped: denser in patches.
        let patch = smoothstep(0.42, 0.7, fbm2(p.x * 0.11, p.y * 0.11, 81));
        let density = if on_floor {
            0.12 + 0.75 * patch
        } else {
            margin_share + 0.5 * patch
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
        tuft(&mut chunks[quadrant(p.x, p.y)], at, height, rng);
        made += 1;
    }
    chunks
}

fn tufts_dense(outline: &[EdgeSample], rng: &mut Rng) -> Vec<Geo> {
    tufts(outline, 1800, 0.25, rng)
}

/// Clumps of grass around the foot of every solid prop and margin model, so
/// they sit in the grass instead of on it (T05, T08).
fn base_clumps(chunks: &mut [Geo], outline: &[EdgeSample], decor: &[Decor], rng: &mut Rng) {
    let props = crate::arena::ARENA_PROPS
        .iter()
        .map(|p| (p.position, p.kind.footprint_radius(), true));
    let margin = decor.iter().map(|d| {
        let r = if d.model == "tree_a" {
            TREE_TRUNK * d.transform.scale.x * 0.55
        } else {
            d.radius
        };
        (d.transform.translation, r, false)
    });
    for (at, radius, on_floor) in props.chain(margin) {
        let n = 5 + (rng.next_u64() % 4) as usize;
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
            tuft(&mut chunks[quadrant(p.x, p.z)], p, height, rng);
        }
    }
}

/// A round cartoon bush: a few overlapping faceted puffs.
fn bush(geo: &mut Geo, at: Vec3, size: f32, rng: &mut Rng) {
    let leaf = lin(cartoon::FOLIAGE);
    let puffs = 3 + (rng.next_u64() % 2) as usize;
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
            |n| shade(leaf, tone * (0.97 + 0.08 * n.y.max(0.0))),
        );
    }
}

fn bushes(outline: &[EdgeSample], decor: &[Decor], rng: &mut Rng) -> Geo {
    let mut geo = Geo::default();
    let mut placed: Vec<(Vec2, f32)> = decor
        .iter()
        .filter(|d| d.model != "tree_a")
        .map(|d| (d.transform.translation.xz(), d.radius))
        .collect();
    let reach = ARENA_HALF + BASE_MARGIN + 5.0;
    let mut made = 0;
    let mut attempts = 0;
    while made < 34 && attempts < 20_000 {
        attempts += 1;
        let p = Vec2::new(rng.range(-reach, reach), rng.range(-reach, reach));
        let size = rng.range(0.6, 1.0);
        let foot = size * 1.8;
        if edge_distance(p.x, p.y) < foot + EDGE_CLEARANCE || inside_rim(outline, p) < foot + 0.3 {
            continue;
        }
        // Bushes gather at tree bases and along the rim.
        let near_tree = decor
            .iter()
            .filter(|d| d.model == "tree_a")
            .any(|d| d.transform.translation.xz().distance(p) < 3.2 * d.transform.scale.x);
        let near_rim = inside_rim(outline, p) < foot + 2.0;
        if !(near_tree || near_rim) {
            continue;
        }
        if placed.iter().any(|(q, r)| q.distance(p) < r + foot) {
            continue;
        }
        placed.push((p, foot));
        bush(&mut geo, Vec3::new(p.x, 0.0, p.y), size, rng);
        made += 1;
    }
    geo
}

/// A little white daisy with a gold centre, facing up just above the grass.
fn daisy(geo: &mut Geo, at: Vec3, size: f32, rng: &mut Rng) {
    let white = lin(cartoon::GLOVE_WHITE);
    let gold = lin(cartoon::STAR_GOLD);
    let up = Vec3::Y;
    let spin = rng.range(0.0, TAU);
    let c = at + Vec3::Y * rng.range(0.05, 0.1);
    for k in 0..5 {
        let a = spin + TAU * k as f32 / 5.0;
        let dir = Vec3::new(a.cos(), 0.0, a.sin());
        let side = dir.cross(up) * size * 0.32;
        let tip = c + dir * size + Vec3::Y * size * 0.12;
        geo.tri_raw([c - side, c + side, tip], [up; 3], [white; 3]);
    }
    let r = size * 0.32;
    let ring: Vec<Vec3> = (0..3)
        .map(|k| {
            let a = spin + TAU * k as f32 / 3.0;
            c + Vec3::Y * 0.004 + Vec3::new(a.cos(), 0.0, a.sin()) * r
        })
        .collect();
    geo.tri_raw([ring[0], ring[2], ring[1]], [up; 3], [gold; 3]);
}

fn flowers(outline: &[EdgeSample], rng: &mut Rng) -> Geo {
    let mut geo = Geo::default();
    // Flowers grow in little clusters.
    for _ in 0..70 {
        let Some((p, on_floor)) = island_point(outline, rng, 0.6) else {
            continue;
        };
        let n = 2 + (rng.next_u64() % 4) as usize;
        for _ in 0..n {
            let q = p + Vec2::new(rng.range(-0.6, 0.6), rng.range(-0.6, 0.6));
            let off_floor = q.x.abs() > ARENA_HALF - 0.2 || q.y.abs() > ARENA_HALF - 0.2;
            if (on_floor && off_floor) || (!on_floor && inside_rim(outline, q) < 0.4) {
                continue;
            }
            daisy(
                &mut geo,
                Vec3::new(q.x, 0.0, q.y),
                rng.range(0.055, 0.085),
                rng,
            );
        }
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
        // The close edge on the east side comes within about 3 m; elsewhere
        // the margin is wide enough for trees.
        let east = outline
            .iter()
            .filter(|s| s.normal.x > 0.99 && (0.0..8.0).contains(&s.base.z))
            .map(|s| s.margin)
            .fold(0.0, f32::max);
        assert!(east < 3.2, "close edge margin {east}");
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
            assert!(
                edge_distance(v.x, v.z) > MIN_MARGIN - 0.6 || v.y < -8.0,
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
        for geo in island
            .tufts
            .iter()
            .chain(&island.dense_tufts)
            .chain([&island.flowers, &island.pebbles])
        {
            assert!(!geo.is_empty());
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
    fn margin_decor_stays_off_the_arena_and_on_the_island() {
        let island = generated();
        let outline = outline();
        let trees = island.decor.iter().filter(|d| d.model == "tree_a").count();
        assert!(trees >= 18, "{trees} trees");
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
        assert!(island.bushes.tri_count() > 1000);
        for v in island.bushes.vertices() {
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
        assert!(tris < 40_000, "island has {tris} triangles");
        assert_eq!(a.tufts.len(), 4);
    }
}
