//! The floating island, generated in code (targets T01, T11): the grassy top
//! (the 48 m arena square plus a margin ring), cliffs under its edge (a grassy
//! lip overhanging brown rock in strata, dropping into space), dense low grass
//! tufts and white, yellow and pink flower clusters, pebbles, leafy bushes (on
//! the margin, hugging the solid props, and low at the arena's edge where the
//! gallery views stand), a ring of framing trees and the margin's other trees,
//! big rocks and stumps (Blender models, some placed by hand to frame the
//! gallery views: [`FRAMING`]), the small knoll the station view looks up
//! from, and a sea of soft cartoon clouds under the rim and around the far
//! islands (D48). Everything here is pure, deterministic CPU work ending in a
//! few merged meshes and a list of model placements, so it is tested headless
//! and costs nothing per frame. The sky slice owns the far view itself; the
//! clouds only read its island layout.
//!
//! Triangle budget (Battery preset; see [`Island::triangles`]): at most
//! [`ISLAND_TRIANGLE_BUDGET`] for the merged scenery, about 121k today (bushes
//! about 32k with 5k more at the arena's edge, tufts about 32k, clouds about
//! 25k, flowers about 14k, pebbles 4k, the knoll 3k, ground and cliffs the
//! rest), plus about 60 models on the margin and the knoll (about 36 trees
//! ≤ 1.4k each, rocks and stumps ≤ 300: about 52k), instanced per model. The
//! Plugged-in preset adds about 18k triangles of extra tufts. All of it is
//! static: merged meshes sharing six materials, split into chunks so the
//! camera culls what it can't see.

use super::geo::{Geo, Rgba, blob, fbm2, lin, mix, noise2, shade, smooth_blob, smoothstep};
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
/// The island's edge is never closer than this to the arena (m): the lip at
/// the north end of the close edge ([`LIP_Z`]), where the grass ends just
/// past the barrier.
pub const MIN_MARGIN: f32 = 0.9;
/// The close edge's margin (m) either side of the lip.
pub const CLOSE_MARGIN: f32 = 2.7;
/// The typical width of the island's margin beyond the arena (m).
pub const BASE_MARGIN: f32 = 9.0;
/// Spacing of the island top's grid (m); the outline samples the arena's
/// sides at the same spacing so the two meshes share their seam vertices.
pub const GROUND_STEP: f32 = 2.0;
/// Where the island's edge comes closest to the arena: the east side between
/// these z, [`CLOSE_MARGIN`] past the barrier (the lip at its north end is
/// narrower still).
pub const CLOSE_EDGE_Z: (f32, f32) = (-8.0, 4.0);
/// The north end of the close edge narrows further, between these z, to a
/// lip [`MIN_MARGIN`] past the barrier: the grass stops at the cliff right
/// behind the curtain. The island-edge view (T11) looks along it, at the
/// cliff face where the island steps straight back out at its north end.
pub const LIP_Z: (f32, f32) = (-8.0, 0.0);

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
    // behind the barrier, narrowing to a lip at its north end. North of the
    // lip the island steps straight back out to its full width, so that
    // step's cliff face looks south, back along the lip (T11).
    if dir.x > 0.5 {
        let (_, z1) = CLOSE_EDGE_Z;
        let (l0, l1) = LIP_Z;
        let open = if base.y >= l0 - 1.0 { 1.0 } else { 0.0 };
        let t = (1.0 - smoothstep(z1, z1 + 6.0, base.y)) * open;
        m += (CLOSE_MARGIN - m) * t;
        let u = (1.0 - smoothstep(l1, l1 + 4.0, base.y)) * open;
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
    /// Low bushes just inside the arena's edge where the gallery views stand
    /// (outlined; no collision; see [`EDGE_COVER`]).
    pub edge_bushes: Geo,
    /// Soft cartoon clouds below the rim and around the far islands (the far
    /// layer's unlit material), per sector of the sky.
    pub clouds: Vec<Geo>,
    /// Trees, big rocks and stumps on the margin.
    pub decor: Vec<Decor>,
    /// The little floating knoll the station view (T10) looks up from.
    pub knoll: Option<Knoll>,
}

/// A small floating hill out over the void, carrying trees, a stump, rocks
/// and bushes: the grassy rise the station view (T10) looks up from, framing
/// the bottom of its shot as the painted hillside does. From the arena it is
/// one more small floating island.
#[derive(Debug, Default)]
pub struct Knoll {
    /// Its grassy top (the island's ground material, no grid).
    pub top: Geo,
    /// Its cliffs and bushes (toon, outlined).
    pub rock: Geo,
    /// Grass tufts and flowers.
    pub grass: Geo,
    /// Trees, a stump and rocks standing on it.
    pub decor: Vec<Decor>,
}

impl Island {
    pub fn generate() -> Self {
        let mut rng = Rng::new(0x15_1A_4D);
        let outline = outline();
        let decor = decor(&outline, &mut rng.fork(3));
        let mut tufts = tufts(&outline, 5200, 0.55, &mut rng.fork(4));
        base_clumps(&mut tufts, &outline, &decor, &mut rng.fork(8));
        let mut flowers = flowers(&outline, &mut rng.fork(6));
        let edge_bushes = edge_cover(&mut flowers, &mut tufts, &mut rng.fork(12));
        Island {
            ground: ground(&outline, &mut rng.fork(1)),
            skirt: skirt(&outline, &mut rng.fork(2)),
            bushes: bushes(&outline, &decor, &mut rng.fork(9)),
            edge_bushes,
            prop_bushes: prop_bushes(&mut rng.fork(10)),
            clouds: clouds(&outline, &mut rng.fork(11)),
            decor,
            tufts,
            dense_tufts: tufts_dense(&outline, &mut rng.fork(5)),
            flowers,
            pebbles: pebbles(&outline, &mut rng.fork(7)),
            knoll: station_view_knoll(&outline, &mut rng.fork(13)),
        }
    }

    /// Triangles drawn on the Battery preset (models aside).
    pub fn triangles(&self) -> usize {
        let knoll = self.knoll.as_ref().map_or(0, |k| {
            k.top.tri_count() + k.rock.tri_count() + k.grass.tri_count()
        });
        knoll
            + [
                &self.ground,
                &self.skirt,
                &self.pebbles,
                &self.prop_bushes,
                &self.edge_bushes,
            ]
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

/// Ring profile under the rim (T01, T11): (height, outward offset, shrink
/// toward the island's centre, band). Bands: 0 the grass lip, bulging out over
/// the cliff; 1 its ragged fringe, dripping down by a per-column amount; 2 the
/// shadow tucked in under the overhang; 3 the rock face, in strata: each
/// stratum steps out into a ledge (a lit top) and tucks back in under it; 4
/// the underside, tapering to a point far below.
const SKIRT: [(f32, f32, f32, u8); 16] = [
    (0.0, 0.0, 1.0, 0),
    (-0.14, 0.26, 1.0, 0),
    (-0.36, 0.4, 1.0, 0),
    (-0.52, 0.3, 1.0, 1),
    (-0.66, -0.06, 1.0, 2),
    (-1.2, -0.12, 1.0, 3),
    (-1.32, 0.16, 1.0, 3),
    (-2.2, 0.06, 1.0, 3),
    (-2.32, 0.36, 1.0, 3),
    (-3.6, 0.22, 1.0, 3),
    (-3.75, 0.5, 0.99, 3),
    (-5.8, 0.3, 0.99, 3),
    (-10.5, -0.4, 0.97, 3),
    (-15.0, 0.0, 0.86, 4),
    (-26.0, 0.0, 0.56, 4),
    (-42.0, 0.0, 0.06, 4),
];

/// How far (m) the grass fringe drips below its ring, per outline column: a
/// ragged, scalloped hem of grass hanging over the rock (T11).
fn fringe_drip(i: usize) -> f32 {
    0.08 + 0.3 * noise2(i as f32 * 0.83, 3.1, 57) + 0.12 * noise2(i as f32 * 2.9, 7.7, 58)
}

/// Whether an outline column is a vertical crack in the rock face (pushed in
/// and darker): clusters of cracks every few metres, like the painted cliffs'
/// rock columns.
fn crack(i: usize) -> bool {
    noise2(i as f32 * 1.7, 0.5, 59) > 0.62
}

fn skirt(outline: &[EdgeSample], rng: &mut Rng) -> Geo {
    let count = outline.len();
    let rings = SKIRT.len();
    // Per-column bulge noise so the cliff reads as rounded lumps and columns.
    let lumps: Vec<f32> = (0..count).map(|_| rng.range(-0.3, 0.4)).collect();
    // Each column's rock steps out square to the rim itself, not just to the
    // arena's side, so where the rim turns (the step out north of the lip,
    // T11) the ledges and the overhang face the way the cliff does.
    let out_dir: Vec<Vec3> = (0..count)
        .map(|i| {
            let (prev, next) = (
                outline[(i + count - 1) % count].edge(),
                outline[(i + 1) % count].edge(),
            );
            let along = (next - prev).with_y(0.0);
            let mut n = Vec3::new(-along.z, 0.0, along.x).normalize_or(outline[i].normal);
            if n.dot(outline[i].normal) < 0.0 {
                n = -n;
            }
            (n + outline[i].normal).normalize_or(outline[i].normal)
        })
        .collect();
    let mut grid = vec![vec![Vec3::ZERO; count]; rings];
    for (i, s) in outline.iter().enumerate() {
        let edge = s.edge();
        let smooth = (lumps[i] + lumps[(i + 1) % count] + lumps[(i + count - 1) % count]) / 3.0;
        for (k, &(y, out, shrink, band)) in SKIRT.iter().enumerate() {
            let (wobble, dy) = match band {
                1 => (0.0, -fringe_drip(i)),
                3 => (
                    smooth * (1.0 + 0.4 * noise2(i as f32 * 0.9, k as f32 * 1.3, 61))
                        - if crack(i) { 0.22 } else { 0.0 },
                    0.0,
                ),
                _ => (0.0, 0.0),
            };
            let flat = edge.with_y(0.0) * shrink;
            grid[k][i] = flat + out_dir[i] * (out + wobble) * shrink + Vec3::Y * (y + dy);
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
    let lit_grass = lin(cartoon::GRASS_LIGHT);
    let dirt = lin(cartoon::CLIFF_DIRT);
    let tucked = lin(cartoon::CLIFF_DIRT_SHADOW);
    let under = shade(dirt, 0.8);
    let mut g = Geo::default();
    for k in 0..rings - 1 {
        for i in 0..count {
            let j = (i + 1) % count;
            let band = SKIRT[k].3.max(SKIRT[k + 1].3);
            // Strata: each stratum a shade of its own, ledge tops (the quads
            // stepping out as they go down) lit, cracks dark.
            // Strata alternate light and dark, face by face (a face and the
            // ledge under it share an index), crossed by the rock's vertical
            // slabs, each its own shade (T11).
            let stratum = [0.96, 0.8, 0.92, 0.78, 0.9, 0.76][(k / 2) % 6];
            let slab = 0.88 + 0.2 * noise2(i as f32 * 0.45, 1.7, 63);
            let ledge = SKIRT[k + 1].1 > SKIRT[k].1 + 0.15;
            let color = match band {
                0 if k == 0 => mix(grass, lit_grass, 0.5),
                0 => grass,
                1 => shade(grass, 0.82),
                2 => tucked,
                3 if crack(i) || crack(j) => shade(dirt, 0.6),
                3 if ledge => shade(dirt, 1.2 * slab),
                3 => shade(dirt, stratum * slab),
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
const TREE_CANOPY: f32 = 2.55;
const TREE_TRUNK: f32 = 1.1;
/// The same for the big, broad tree_b (tree_b.json): its crown reaches
/// further out over its long limb.
const TREE_B_CANOPY: f32 = 3.05;
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

/// A margin model placed by hand to frame a gallery view: a big tree at the
/// edge of the shot, a rock or stump near its foot. The targets frame every
/// shot this way (T01, T03, T04, T05, T11); the views stand near the arena's
/// edge so these fall at the frame's edges (`scenario::gallery`).
struct Framing {
    model: &'static str,
    x: f32,
    z: f32,
    yaw_deg: f32,
    scale: f32,
}

const fn framing(model: &'static str, x: f32, z: f32, yaw_deg: f32, scale: f32) -> Framing {
    Framing {
        model,
        x,
        z,
        yaw_deg,
        scale,
    }
}

/// The hand-placed framing models, by the view they frame.
const FRAMING: [Framing; 9] = [
    // T01, from the west edge: a big tree at the left of the shot, a rock and
    // a stump under it.
    framing("tree_b", -29.6, 4.2, 200.0, 1.7),
    framing("rock_a", -26.0, 8.4, 150.0, 1.4),
    framing("stump_a", -25.6, 6.1, 0.0, 1.25),
    // T05, from the north-west corner: a big tree at the left.
    framing("tree_b", -28.0, -28.0, 150.0, 1.55),
    // T03, from the north edge: a tree at the top left, over the fort, a
    // stump under it.
    framing("tree_a", 3.6, -29.2, 20.0, 1.75),
    framing("stump_a", 6.4, -25.6, 60.0, 1.2),
    // T11, looking north along the east lip: a big tree at the top left, on
    // the north margin.
    framing("tree_b", 18.0, -29.3, 110.0, 1.6),
    // T04, from the south-east: a tree at the right edge, a stump before it.
    framing("tree_a", 29.5, 12.5, 120.0, 1.6),
    framing("stump_a", 26.5, 14.0, 30.0, 1.2),
];

/// Whether a margin point is where the island steps back out north of the
/// lip: no trees there, so the island-edge view (T11) sees the step's cliff,
/// the void and the station over it.
fn in_clear_sky(p: Vec2) -> bool {
    p.x > ARENA_HALF && (LIP_Z.0 - 12.0..LIP_Z.0).contains(&p.y)
}

/// A model's footprint radius at scale 1 (m) and its blob shadow radius.
fn footprint(model: &str) -> (f32, f32) {
    match model {
        "tree_a" => (TREE_CANOPY, 1.9),
        "tree_b" => (TREE_B_CANOPY, 2.2),
        "rock_a" => (0.95, 1.35),
        "rock_b" => (1.07, 1.45),
        _ => (0.64, 0.9),
    }
}

fn decor(outline: &[EdgeSample], rng: &mut Rng) -> Vec<Decor> {
    let mut placed: Vec<Decor> = FRAMING
        .iter()
        .map(|f| {
            let (foot, shadow) = footprint(f.model);
            Decor {
                model: f.model,
                transform: Transform::from_xyz(f.x, 0.0, f.z)
                    .with_rotation(Quat::from_rotation_y(f.yaw_deg.to_radians()))
                    .with_scale(Vec3::splat(f.scale)),
                shadow: shadow * f.scale,
                radius: foot * f.scale,
            }
        })
        .collect();
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
            if tree && in_clear_sky(p) {
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

/// A round, leafy cartoon bush (T01, T05): a crown puff on top of a ring of
/// smaller ones, so its outline is scalloped like the painted bushes. Each
/// puff shades round and is sunlit on top as the tree crowns are: a
/// yellow-green cap (`GRASS_LIGHT`), lighter flanks (`FOLIAGE_LIGHT`), leaf
/// green below, blended softly over each puff. Faces buried inside a
/// neighbouring puff are dropped. About
/// 1.35 × `size` tall, reaching about 1.3 × `size` from its root.
fn bush(geo: &mut Geo, at: Vec3, size: f32, rng: &mut Rng) {
    bush_of(geo, at, size, 3 + (rng.next_u64() % 2) as usize, rng);
}

fn bush_of(geo: &mut Geo, at: Vec3, size: f32, ring: usize, rng: &mut Rng) {
    let leaf = lin(cartoon::FOLIAGE);
    let flank = lin(cartoon::FOLIAGE_LIGHT);
    let sun = lin(cartoon::GRASS_LIGHT);
    let seed = rng.next_u64() as u32;
    let squash = Vec3::new(1.0, 0.82, 1.0);
    // (centre, radius) of every puff: the crown, then the ring round it, and
    // for the big foreground bushes (`ring` ≥ 6) smaller puffs between the
    // two, so the outline scallops like the painted leaf clumps.
    let mut puffs = vec![(at + Vec3::Y * size * 0.74, size * 0.72)];
    let spin = rng.range(0.0, TAU);
    let small = if ring >= 6 { 0.8 } else { 1.0 };
    for k in 0..ring {
        let a = spin + TAU * k as f32 / ring as f32 + rng.range(-0.3, 0.3);
        let r = size * rng.range(0.5, 0.62) * small;
        let out = size * rng.range(0.55, 0.68);
        puffs.push((at + Vec3::new(a.cos() * out, r * 0.62, a.sin() * out), r));
    }
    if ring >= 6 {
        for k in 0..4 {
            let a = spin + TAU * (k as f32 + 0.5) / 4.0 + rng.range(-0.3, 0.3);
            let r = size * rng.range(0.34, 0.42);
            let out = size * rng.range(0.42, 0.52);
            puffs.push((
                at + Vec3::new(a.cos() * out, size * rng.range(0.78, 0.9), a.sin() * out),
                r,
            ));
        }
    }
    let buried = |p: Vec3, own: usize| {
        puffs
            .iter()
            .enumerate()
            .any(|(j, &(c, r))| j != own && ((p - c) / (squash * r * 0.94)).length_squared() < 1.0)
    };
    for (k, &(c, r)) in puffs.iter().enumerate() {
        let mut shape = rng.fork(k as u64 + 31);
        let tone = rng.range(0.95, 1.06);
        let mut one = Geo::default();
        smooth_blob(
            &mut one,
            1,
            |v| {
                let v = Vec3::new(v.x, v.y.max(-0.55), v.z);
                c + v * r * squash * shape.range(0.9, 1.08)
            },
            |u| u / squash,
            |u| {
                // Sunlit cap, lighter flanks, leaf green, darker underneath.
                let dapple = noise2(u.x * 2.3 + k as f32 * 3.7, u.z * 2.3, seed) * 0.25;
                let h = u.y + dapple;
                let c = if h > 0.3 {
                    mix(flank, sun, 0.75 * smoothstep(0.45, 0.85, h))
                } else {
                    mix(leaf, flank, smoothstep(-0.05, 0.3, h))
                };
                shade(c, tone * (0.78 + 0.22 * smoothstep(-0.6, 0.1, u.y)))
            },
        );
        for t in 0..one.tri_count() {
            let corners = [0, 1, 2].map(|i| Vec3::from_array(one.positions[t * 3 + i]));
            if corners.iter().all(|&p| buried(p, k)) {
                continue;
            }
            geo.positions
                .extend_from_slice(&one.positions[t * 3..t * 3 + 3]);
            geo.normals
                .extend_from_slice(&one.normals[t * 3..t * 3 + 3]);
            geo.colors.extend_from_slice(&one.colors[t * 3..t * 3 + 3]);
        }
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
    while made < 96 && attempts < 60_000 {
        attempts += 1;
        let p = Vec2::new(rng.range(-reach, reach), rng.range(-reach, reach));
        let size = rng.range(0.6, 1.1);
        let foot = size * 1.8;
        let edge = edge_distance(p.x, p.y);
        let to_rim = inside_rim(outline, p);
        if edge < foot + EDGE_CLEARANCE || to_rim < foot + 0.3 {
            continue;
        }
        // The lip stays clean grass over the cliff (T11).
        if edge + to_rim < 4.5 {
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
        let near_rim = to_rim < foot + 2.0;
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
    for _ in 0..235 {
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

/// Tallest bush allowed in an edge-cover patch on the arena floor (m).
pub const EDGE_BUSH_MAX_HEIGHT: f32 = 0.62;
/// Edge-cover patches stay within this distance of the arena's edge (m), so
/// the middle of the arena stays open lawn.
pub const EDGE_COVER_BAND: f32 = 4.5;

/// Low ground cover on the arena floor where a gallery view stands: a few
/// small bushes just inside the arena's edge (never taller than
/// [`EDGE_BUSH_MAX_HEIGHT`], within [`EDGE_COVER_BAND`] of the edge, no
/// collision), flowers and thick tufts. They fill the foot of the shot the way
/// the painted foreground bushes do (T01, T03, T05, T11). (x, z, radius,
/// bushes).
const EDGE_COVER: [(f32, f32, f32, usize); 5] = [
    // T01: the bottom left of the shot from the west edge.
    (-21.6, 11.4, 1.3, 4),
    (-22.5, 8.6, 0.9, 2),
    // T05: the bottom left, in the north-west.
    (-21.5, -17.2, 1.0, 3),
    // T11: the bottom left, on the east edge.
    (22.2, -2.0, 0.9, 3),
    // T03: flowers and tufts only, mid-field at the bottom left of the shot.
    (-2.6, -15.2, 1.2, 0),
];

fn edge_cover(flowers: &mut [Geo], tufts: &mut [Geo], rng: &mut Rng) -> Geo {
    let mut bushes = Geo::default();
    // A bush stands about 1.35 × its size tall.
    let largest = EDGE_BUSH_MAX_HEIGHT / 1.4;
    let petals = [
        (lin(cartoon::GLOVE_WHITE), lin(cartoon::STAR_GOLD)),
        (lin(cartoon::SPELL_GOLD), lin(cartoon::STAR_GOLD)),
        (lin(cartoon::FLOWER_PINK), lin(cartoon::SPELL_GOLD)),
    ];
    for &(x, z, radius, count) in &EDGE_COVER {
        let centre = Vec2::new(x, z);
        let inside = |p: Vec2| p.x.abs() < ARENA_HALF - 0.3 && p.y.abs() < ARENA_HALF - 0.3;
        for k in 0..count {
            let a = TAU * k as f32 / count as f32 + rng.range(-0.5, 0.5);
            let p = centre + Vec2::new(a.cos(), a.sin()) * radius * rng.range(0.2, 0.75);
            let size = largest * rng.range(0.72, 1.0);
            let by_edge = ARENA_HALF - p.x.abs().max(p.y.abs()) <= EDGE_COVER_BAND - 1.0;
            if inside(p) && by_edge {
                bush_of(&mut bushes, Vec3::new(p.x, 0.0, p.y), size, 6, rng);
            }
        }
        for _ in 0..14 {
            let a = rng.range(0.0, TAU);
            let p = centre + Vec2::new(a.cos(), a.sin()) * radius * rng.range(0.3, 1.5);
            if inside(p) {
                tuft(
                    &mut tufts[chunk(p.x, p.y)],
                    Vec3::new(p.x, 0.0, p.y),
                    rng.range(0.16, FLOOR_CLUTTER_MAX_HEIGHT - 0.01),
                    rng,
                );
            }
        }
        let (petal, heart) = petals[(rng.next_u64() % petals.len() as u64) as usize];
        for _ in 0..7 {
            let a = rng.range(0.0, TAU);
            let p = centre + Vec2::new(a.cos(), a.sin()) * radius * rng.range(0.6, 1.6);
            if inside(p) {
                flower(
                    &mut flowers[chunk(p.x, p.y)],
                    Vec3::new(p.x, 0.0, p.y),
                    rng.range(0.09, 0.13),
                    petal,
                    heart,
                    rng,
                );
            }
        }
    }
    bushes
}

// ---------------------------------------------------------------------------
// The station view's knoll (T10)
// ---------------------------------------------------------------------------

/// Where the station view (T10) looks from and at, from the gallery's table:
/// the knoll follows its camera. `None` if the view isn't a fixed camera out
/// over the void.
fn station_view(outline: &[EdgeSample]) -> Option<(Vec3, Vec3)> {
    use crate::scenario::gallery::{Framing as View, views};
    let view = views().into_iter().find(|v| v.id == "T10")?;
    let View::Fixed { eye, look_at } = view.framing else {
        return None;
    };
    (inside_rim(outline, eye.xz()) < -25.0).then_some((eye, look_at))
}

/// The knoll's height (m) above its base plane at a point `(u, v)` in the
/// view's frame (u right, v forward, metres from the camera's foot): a low
/// rounded hill, rising at the left, sloping away ahead.
fn knoll_height(u: f32, v: f32) -> f32 {
    let dome = 1.0 - ((u / 26.0).powi(2) + ((v - 12.0) / 22.0).powi(2));
    let left = smoothstep(0.0, -18.0, u) * 1.6;
    (dome.max(0.0).sqrt() * 2.2 + left * dome.max(0.0)).max(0.0)
}

fn station_view_knoll(outline: &[EdgeSample], rng: &mut Rng) -> Option<Knoll> {
    let (eye, look_at) = station_view(outline)?;
    let ahead = (look_at - eye).with_y(0.0).normalize_or(Vec3::NEG_Z);
    let right = ahead.cross(Vec3::Y);
    // The camera stands on the hill's crest: its foot 1.8 m under the eye,
    // where the hill is about 2.2 m high.
    let base = eye - Vec3::Y * (1.8 + knoll_height(0.0, 0.0));
    let at = |u: f32, v: f32| base + right * u + ahead * v + Vec3::Y * knoll_height(u, v);
    let mut knoll = Knoll::default();
    // The top: a polar grid round the hill's middle, (12 m ahead), out to an
    // oval rim.
    let centre = (0.0, 12.0);
    let (rings, sides) = (5, 20);
    let rim_at = |a: f32| {
        let wobble = 1.0 + 0.08 * (3.0 * a + 1.3).sin() + 0.05 * (5.0 * a + 0.4).sin();
        (26.0 * a.cos() * wobble, 22.0 * a.sin() * wobble)
    };
    let point = |ring: usize, side: usize| {
        let a = TAU * side as f32 / sides as f32;
        let t = ring as f32 / rings as f32;
        let (ru, rv) = rim_at(a);
        at(centre.0 + ru * t, centre.1 + rv * t)
    };
    let grass = lin(cartoon::GRASS);
    for r in 0..rings {
        for sd in 0..sides {
            let n = (sd + 1) % sides;
            let (a, b) = (point(r, sd), point(r, n));
            let (c, d) = (point(r + 1, n), point(r + 1, sd));
            for tri in [[a, c, d], [a, b, c]] {
                let nrm = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
                if nrm.length_squared() < 1e-8 {
                    continue;
                }
                let tri = if nrm.y < 0.0 {
                    [tri[0], tri[2], tri[1]]
                } else {
                    tri
                };
                let nrm = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize();
                knoll.top.tri_raw(tri, [nrm; 3], [grass; 3]);
            }
        }
    }
    // Its cliffs: the island skirt's profile, scaled down, under the rim.
    let rim: Vec<Vec3> = (0..sides).map(|sd| point(rings, sd)).collect();
    let mid = base + ahead * centre.1;
    let dirt = lin(cartoon::CLIFF_DIRT);
    let profile = [
        (0.0, 0.0, 0),
        (-0.5, 0.35, 1),
        (-0.8, -0.05, 2),
        (-3.5, 0.3, 3),
        (-7.0, -0.2, 3),
        (-16.0, -9.0, 4),
    ];
    let ring_at = |k: usize| -> Vec<Vec3> {
        let (dy, out, _) = profile[k];
        rim.iter()
            .map(|&p| {
                let o = (p - mid).with_y(0.0).normalize_or(Vec3::X);
                if k + 1 == profile.len() {
                    mid.with_y(p.y) + (p - mid).with_y(0.0) * 0.25 + Vec3::Y * dy
                } else {
                    p + o * out + Vec3::Y * dy
                }
            })
            .collect()
    };
    for k in 0..profile.len() - 1 {
        let (top, bottom) = (ring_at(k), ring_at(k + 1));
        let color = match profile[k].2.max(profile[k + 1].2) {
            0 | 1 => shade(grass, 0.88),
            2 => lin(cartoon::CLIFF_DIRT_SHADOW),
            3 => shade(dirt, if k % 2 == 0 { 1.0 } else { 0.9 }),
            _ => shade(dirt, 0.8),
        };
        for sd in 0..sides {
            let n = (sd + 1) % sides;
            knoll
                .rock
                .quad(top[sd], bottom[sd], bottom[n], top[n], color);
        }
    }
    // What stands on it: a big tree at the left of the shot and a smaller one
    // beside it with a stump and rocks at their feet, a tree at the right,
    // and bushes along the crest (T10).
    let models: [(&str, f32, f32, f32, f32); 7] = [
        ("tree_b", -10.5, 9.0, 40.0, 1.5),
        ("tree_a", -8.5, 16.5, 200.0, 1.05),
        ("stump_a", -6.2, 11.0, 0.0, 1.3),
        ("rock_b", -9.2, 13.4, 120.0, 1.2),
        ("rock_a", 5.5, 15.0, 60.0, 1.0),
        ("tree_a", 13.0, 10.5, 310.0, 1.35),
        ("tree_a", 15.0, 21.0, 90.0, 1.1),
    ];
    for (model, u, v, yaw, scale) in models {
        let (foot, shadow) = footprint(model);
        knoll.decor.push(Decor {
            model,
            transform: Transform::from_translation(at(u, v))
                .with_rotation(Quat::from_rotation_y(yaw.to_radians()))
                .with_scale(Vec3::splat(scale)),
            shadow: shadow * scale,
            radius: foot * scale,
        });
    }
    for (u, v, size) in [
        (-2.5, 14.0, 1.0),
        (1.5, 16.5, 0.9),
        (-4.0, 18.5, 0.8),
        (7.5, 13.0, 1.1),
        (9.5, 16.0, 0.9),
        (-12.0, 13.5, 1.0),
        (15.5, 14.0, 0.9),
    ] {
        bush(&mut knoll.rock, at(u, v), size, rng);
    }
    for _ in 0..140 {
        let (u, v) = (rng.range(-20.0, 20.0), rng.range(2.0, 28.0));
        let p = at(u, v);
        if knoll_height(u, v) > 0.3 {
            tuft(&mut knoll.grass, p, rng.range(0.25, 0.5), rng);
        }
    }
    Some(knoll)
}

/// Two or three small bushes hugging the foot of every solid arena prop
/// (T01, T05): tucked mostly under the prop's own footprint, no taller than
/// [`PROP_BUSH_MAX_HEIGHT`], so they dress the cover without changing it.
fn prop_bushes(rng: &mut Rng) -> Geo {
    let mut geo = Geo::default();
    for prop in crate::arena::ARENA_PROPS {
        let foot = prop.kind.footprint_radius();
        // A bush reaches about 1.3 × its size from its root and stands
        // about 1.35 × its size tall.
        let largest = 0.42_f32
            .min((PROP_BUSH_REACH + 0.4 * foot) / 1.3)
            .min(0.9 * prop.kind.height().min(PROP_BUSH_MAX_HEIGHT) / 1.35);
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

/// How far (m) below the station's platform its cloud bank floats: under the
/// rock, so the rock and its waterfalls read above the clouds.
const STATION_CLOUD_DROP: f32 = 110.0;

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
        let segments = if c.xz().length() < 90.0 { 10 } else { 8 };
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
    // A bank under the station's rock, on the side facing the arena (T01,
    // T10): anchored to its platform, so the rock and its waterfalls stay
    // clear above it.
    let station = far.station.position;
    let toward = (-station.with_y(0.0)).normalize_or(Vec3::Z);
    let side = toward.cross(Vec3::Y);
    for k in 0..7 {
        let across = (k as f32 / 6.0 * 2.0 - 1.0) * 210.0 + rng.range(-20.0, 20.0);
        let c = station.with_y(0.0)
            + side * across
            + toward * rng.range(120.0, 170.0)
            + Vec3::Y * (station.y - STATION_CLOUD_DROP + rng.range(-20.0, 15.0));
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
            .filter(|s| s.normal.x > 0.99 && (LIP_Z.0..LIP_Z.1).contains(&s.base.z))
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
    fn edge_cover_stays_low_by_the_edge_and_off_the_spawns() {
        let island = generated();
        assert!(island.edge_bushes.tri_count() > 1000);
        let layout = crate::arena::ArenaLayout::default();
        for v in island.edge_bushes.vertices() {
            assert!(
                v.y <= EDGE_BUSH_MAX_HEIGHT + 1e-3,
                "an edge bush too tall at {v}"
            );
            assert!(edge_distance(v.x, v.z) <= 0.0, "inside the arena at {v}");
            let to_edge = ARENA_HALF - v.x.abs().max(v.z.abs());
            assert!(to_edge <= EDGE_COVER_BAND, "an edge bush mid-field at {v}");
            for spawn in [layout.player_spawn, layout.dummy_spawn] {
                assert!(
                    v.xz().distance(spawn.xz()) > 3.0,
                    "a bush on a spawn at {v}"
                );
            }
            for prop in crate::arena::ARENA_PROPS {
                assert!(prop.footprint_distance(v.xz()) > 0.0, "a bush in {prop:?}");
            }
        }
    }

    /// The camera a gallery view renders from, worked out from the view
    /// table alone (for still and aimed views, what the runner computes).
    fn view_camera(id: &str) -> (Transform, f32) {
        use crate::scenario::gallery::{
            Aim, Framing as View, GALLERY_FOV_DEG, framed_camera, views,
        };
        use crate::shared::{EyeHeight, LookAngles};
        let fov = GALLERY_FOV_DEG.to_radians();
        let v = views().into_iter().find(|v| v.id == id).unwrap();
        let height = if v.crouch {
            crate::movement::MovementTuning::default().crouch_eye_height
        } else {
            EyeHeight::default().0
        };
        let eye = v.feet + Vec3::Y * height;
        let aim = match v.aim {
            Aim::Look { yaw, pitch } => eye + LookAngles { yaw, pitch }.forward(),
            Aim::At(point) => point,
            Aim::Knight { up, right } => {
                let feet = v.knight.unwrap().feet;
                let across = (feet - eye).with_y(0.0).normalize_or(Vec3::NEG_Z);
                feet + Vec3::Y * up + across.cross(Vec3::Y) * right
            }
        };
        let camera = match v.framing {
            View::Eye => Transform::from_translation(eye).looking_at(aim, Vec3::Y),
            View::Offset { screen } => framed_camera(eye, aim, screen, fov),
            View::Behind { back } => {
                let fwd = (aim - eye).normalize();
                Transform::from_translation(eye - fwd * back).looking_at(aim, Vec3::Y)
            }
            View::Fixed { eye, look_at } => {
                Transform::from_translation(eye).looking_at(look_at, Vec3::Y)
            }
        };
        (camera, fov)
    }

    /// Where the middle of a tree's crown is.
    fn crown(d: &Decor) -> Vec3 {
        let h = if d.model == "tree_b" { 4.6 } else { 4.3 };
        d.transform.translation + Vec3::Y * h * d.transform.scale.y
    }

    #[test]
    fn the_gallery_views_are_framed_by_trees_as_the_targets_are() {
        let island = generated();
        let trees: Vec<Vec3> = island
            .decor
            .iter()
            .chain(island.knoll.iter().flat_map(|k| k.decor.iter()))
            .filter(|d| is_tree(d.model))
            .map(crown)
            .collect();
        // (view, the screen box a tree's crown must fall in: x range, y
        // range, in half-heights from the centre).
        let wanted: [(&str, (f32, f32), (f32, f32)); 7] = [
            ("T01", (-1.6, -0.8), (0.1, 1.2)),
            ("T03", (-1.6, -0.7), (0.1, 1.2)),
            ("T04", (0.8, 1.6), (0.0, 1.2)),
            ("T05", (-1.6, -0.8), (0.0, 1.2)),
            ("T10", (-1.6, -0.8), (-1.0, 0.7)),
            ("T10", (0.8, 1.6), (-1.0, 0.7)),
            ("T11", (-1.6, -0.6), (0.0, 1.2)),
        ];
        for (id, (x0, x1), (y0, y1)) in wanted {
            let (camera, fov) = view_camera(id);
            let framed = trees.iter().any(|&c| {
                crate::scenario::gallery::screen_point(&camera, fov, c)
                    .is_some_and(|s| (x0..x1).contains(&s.x) && (y0..y1).contains(&s.y))
            });
            assert!(
                framed,
                "{id}: no tree frames the shot at x {x0}..{x1}, y {y0}..{y1}"
            );
        }
        // Low bushes at the foot of the shot, bottom left (T01, T05, T11).
        for id in ["T01", "T05", "T11"] {
            let (camera, fov) = view_camera(id);
            let low = island.edge_bushes.vertices().any(|v| {
                crate::scenario::gallery::screen_point(&camera, fov, v)
                    .is_some_and(|s| s.x < -0.4 && s.x > -1.6 && s.y < -0.4)
            });
            assert!(low, "{id}: no bush at the foot of the shot");
        }
    }

    #[test]
    fn the_knoll_carries_its_trees_under_the_station_view() {
        let island = generated();
        let knoll = island
            .knoll
            .as_ref()
            .expect("the station view has its knoll");
        assert!(knoll.decor.iter().filter(|d| is_tree(d.model)).count() >= 3);
        let (lo, hi) = knoll.top.bounds().unwrap();
        for d in &knoll.decor {
            let p = d.transform.translation;
            assert!(
                p.xz().cmpge(lo.xz()).all() && p.xz().cmple(hi.xz()).all(),
                "{} off the knoll",
                d.model
            );
            assert!(p.y >= lo.y - 1e-3 && p.y <= hi.y + 1e-3);
        }
        // Far out over the void, clear of the island and its clouds' rim.
        let outline = outline();
        assert!(inside_rim(&outline, lo.xz()) < -100.0);
        // Its cliffs hang under its top.
        let (rock_lo, _) = knoll.rock.bounds().unwrap();
        assert!(rock_lo.y < lo.y - 8.0);
    }

    #[test]
    fn the_island_is_deterministic_and_cheap() {
        let a = Island::generate();
        let b = generated();
        assert_eq!(a.ground.positions, b.ground.positions);
        assert_eq!(a.skirt.colors, b.skirt.colors);
        assert_eq!(a.decor, b.decor);
        let tris = a.triangles();
        // Each layer within its share of the budget.
        let sum = |v: &Vec<Geo>| v.iter().map(Geo::tri_count).sum::<usize>();
        let layers = [
            ("tufts", sum(&a.tufts), 45_000),
            ("extra tufts (Plugged in)", sum(&a.dense_tufts), 25_000),
            ("flowers", sum(&a.flowers), 20_000),
            ("bushes", sum(&a.bushes), 36_000),
            ("prop bushes", a.prop_bushes.tri_count(), 6_000),
            ("edge bushes", a.edge_bushes.tri_count(), 10_000),
            (
                "knoll",
                a.knoll.as_ref().map_or(0, |k| {
                    k.top.tri_count() + k.rock.tri_count() + k.grass.tri_count()
                }),
                6_000,
            ),
            ("clouds", sum(&a.clouds), 30_000),
            ("pebbles", a.pebbles.tri_count(), 20_000),
        ];
        for (name, n, cap) in layers {
            assert!(n <= cap, "{name}: {n} triangles, over {cap}");
        }
        assert!(tris < ISLAND_TRIANGLE_BUDGET, "island has {tris} triangles");
        assert_eq!(a.tufts.len(), CHUNKS * CHUNKS);
    }
}
