//! The static world, generated in code: arena floor, boundary cliffs, the plateau
//! and far hills, mesas, trees, clouds, grass tufts and pebbles. Everything here is
//! pure, deterministic CPU work that ends in a handful of merged meshes, so it can
//! be tested headless and costs almost nothing per frame.

use super::geo::{Geo, Rgba, blob, fbm2, lin, mix, noise2, ring, shade, smoothstep};
use crate::{palette, rng::Rng, shared::ARENA_HALF};
use bevy::prelude::*;
use std::f32::consts::TAU;

/// Rocks and props keep at least this far outside the playable square, so walls
/// built on the arena edge never clip into scenery.
pub const EDGE_CLEARANCE: f32 = 0.35;
/// Width of the flat apron just outside the arena before the cliffs rise.
const APRON: f32 = 1.0;
/// Tallest grass or pebble allowed on the playable floor (m).
pub const FLOOR_CLUTTER_MAX_HEIGHT: f32 = 0.26;

/// Direction toward the sun (unit). Warm late afternoon from the west-southwest.
pub fn sun_direction() -> Vec3 {
    let elevation = 36f32.to_radians();
    // Azimuth measured clockwise from north (-Z) toward east (+X).
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

/// Height of the plateau that sits behind the cliffs. Lower toward the sun (west),
/// so the late sun still reaches most of the arena and the sunset view stays open.
fn plateau_height(x: f32, z: f32) -> f32 {
    let toward_sun = Vec2::new(x, z)
        .normalize_or_zero()
        .dot(sun_direction().xz().normalize());
    let bias = 1.0 - 0.45 * toward_sun.max(0.0);
    (3.3 + 3.0 * fbm2(x * 0.045 + 3.1, z * 0.045 - 7.7, 11)) * bias
}

/// Terrain height. Exactly 0 on the playable floor and on a thin apron around it.
pub fn height(x: f32, z: f32) -> f32 {
    let d = edge_distance(x, z);
    if d <= APRON {
        return 0.0;
    }
    let rise = smoothstep(APRON, 5.5, d);
    let roll = (fbm2(x * 0.035, z * 0.035, 12) - 0.5) * 4.0 * smoothstep(6.0, 24.0, d);
    let hills =
        smoothstep(30.0, 170.0, d) * fbm2(x * 0.0065 + 40.0, z * 0.0065, 13).powf(1.6) * 95.0;
    plateau_height(x, z) * rise + roll + hills
}

/// Soft mask (0..1) of the sandy clearings on the arena floor.
pub fn sand_mask(x: f32, z: f32) -> f32 {
    smoothstep(0.68, 0.8, fbm2(x * 0.08 - 3.0, z * 0.08 + 5.0, 23))
}

/// Everything static, as merged triangle soups grouped by how they render.
#[derive(Debug, Default)]
pub struct Scenery {
    /// Arena floor and the near plateau: receives shadows.
    pub ground: Geo,
    /// Distant terrain out to the horizon: fogged, no shadows.
    pub far_ground: Geo,
    /// Boundary cliffs and rocks: cast and receive shadows.
    pub cliffs: Geo,
    /// Trees on the plateau right behind the cliffs: cast shadows.
    pub near_trees: Geo,
    /// Trees, boulders and mesas further out: no shadows.
    pub backdrop: Geo,
    /// Unlit, fog-free clouds with baked facet shading.
    pub clouds: Geo,
    /// Pebbles on the arena floor.
    pub pebbles: Geo,
    /// Grass tufts on the arena floor, one soup per quadrant (Battery preset).
    pub grass: Vec<Geo>,
    /// Extra grass for the Plugged-in preset, one soup per quadrant.
    pub dense_grass: Vec<Geo>,
}

impl Scenery {
    pub fn generate() -> Self {
        let mut rng = Rng::new(0x5EED_A7E4);
        let mut s = Scenery::default();
        terrain(&mut s, &mut rng.fork(1));
        cliffs(&mut s.cliffs, &mut rng.fork(2));
        trees(&mut s, &mut rng.fork(3));
        mesas(&mut s.backdrop, &mut rng.fork(4));
        boulders(&mut s.backdrop, &mut rng.fork(5));
        clouds(&mut s.clouds, &mut rng.fork(6));
        pebbles(&mut s.pebbles, &mut rng.fork(7));
        s.grass = grass(1500, &mut rng.fork(8));
        s.dense_grass = grass(2800, &mut rng.fork(9));
        s
    }
}

// ---------------------------------------------------------------------------
// Terrain: three nested grids (2 m, 8 m, 40 m). Vertices on a grid's outer border
// follow the next coarser grid's edge, so there are no T-junction cracks.
// ---------------------------------------------------------------------------

struct Level {
    half: f32,
    step: f32,
    hole: f32,
}

const LEVELS: [Level; 3] = [
    Level {
        half: 40.0,
        step: 2.0,
        hole: 0.0,
    },
    Level {
        half: 200.0,
        step: 8.0,
        hole: 40.0,
    },
    Level {
        half: 800.0,
        step: 40.0,
        hole: 200.0,
    },
];

fn level_height(level: usize, x: f32, z: f32) -> f32 {
    let l = &LEVELS[level];
    let Some(coarse) = LEVELS.get(level + 1) else {
        return height(x, z);
    };
    let on_x = (x.abs() - l.half).abs() < 1e-3;
    let on_z = (z.abs() - l.half).abs() < 1e-3;
    if on_x == on_z {
        // Interior vertex, or a corner (corners coincide with coarse vertices).
        return height(x, z);
    }
    // Interpolate along the coarse edge this border vertex lies on.
    let along = if on_x { z } else { x };
    let a = (along / coarse.step).floor() * coarse.step;
    let b = a + coarse.step;
    let t = (along - a) / coarse.step;
    let (ha, hb) = if on_x {
        (height(x, a), height(x, b))
    } else {
        (height(a, z), height(b, z))
    };
    ha + (hb - ha) * t
}

fn terrain(s: &mut Scenery, rng: &mut Rng) {
    for (index, level) in LEVELS.iter().enumerate() {
        let cells = (2.0 * level.half / level.step).round() as i32;
        for i in 0..cells {
            for j in 0..cells {
                let x0 = -level.half + i as f32 * level.step;
                let z0 = -level.half + j as f32 * level.step;
                let (x1, z1) = (x0 + level.step, z0 + level.step);
                let inside_hole = level.hole > 0.0
                    && x0 >= -level.hole
                    && x1 <= level.hole
                    && z0 >= -level.hole
                    && z1 <= level.hole;
                if inside_hole {
                    continue;
                }
                let p = |x: f32, z: f32| Vec3::new(x, level_height(index, x, z), z);
                let (a, b, c, d) = (p(x0, z0), p(x0, z1), p(x1, z1), p(x1, z0));
                // Counter-clockwise from above: (x0,z0) → (x0,z1) → (x1,z1) → (x1,z0).
                let tris = if noise2(i as f32 * 7.31, j as f32 * 3.17, 77 + index as u32) < 0.5 {
                    [[a, b, c], [a, c, d]]
                } else {
                    [[a, b, d], [b, c, d]]
                };
                let target = if index == 0 {
                    &mut s.ground
                } else {
                    &mut s.far_ground
                };
                for [p0, p1, p2] in tris {
                    let n = (p1 - p0).cross(p2 - p0).normalize_or_zero();
                    if n == Vec3::ZERO {
                        continue;
                    }
                    // Colors follow the smooth patch field per vertex; one jitter
                    // per face keeps the facets readable without hard stair-steps.
                    let class = ground_class((p0 + p1 + p2) / 3.0, n);
                    let k = jitter(rng, if index == 0 { 0.03 } else { 0.06 });
                    let c = [p0, p1, p2].map(|p| shade(ground_color(p, class), k));
                    target.tri_raw([p0, p1, p2], [n, n, n], c);
                }
            }
        }
    }
}

fn jitter(rng: &mut Rng, amount: f32) -> f32 {
    1.0 + (rng.next_f32() - 0.5) * 2.0 * amount
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ground {
    Floor,
    Rock,
    HillFace,
    Apron,
    Plateau,
}

fn ground_class(c: Vec3, n: Vec3) -> Ground {
    let d = edge_distance(c.x, c.z);
    if d <= 0.0 {
        Ground::Floor
    } else if n.y < 0.74 && d > 30.0 {
        Ground::HillFace
    } else if n.y < 0.74 {
        Ground::Rock
    } else if d < APRON + 0.6 {
        Ground::Apron
    } else {
        Ground::Plateau
    }
}

fn ground_color(p: Vec3, class: Ground) -> Rgba {
    let (x, z) = (p.x, p.z);
    let grass = lin(palette::GRASS);
    let grass_dark = lin(palette::GRASS_DARK);
    let olive = lin(palette::OLIVE);
    let sand = lin(palette::SAND);
    let dirt = lin(palette::DIRT);
    match class {
        Ground::Floor => {
            // Playable floor: gentle patches, worn clearings, a dusty band at the edge.
            let patches = fbm2(x * 0.09, z * 0.09, 21);
            let mut col = mix(grass, grass_dark, smoothstep(0.4, 0.75, patches) * 0.7);
            col = mix(
                col,
                olive,
                smoothstep(0.55, 0.8, fbm2(x * 0.05 + 9.0, z * 0.05, 22)) * 0.4,
            );
            col = mix(col, dirt, sand_mask(x, z) * 0.55);
            let rim = x.abs().max(z.abs());
            mix(
                col,
                dirt,
                smoothstep(ARENA_HALF - 1.6, ARENA_HALF, rim) * 0.6,
            )
        }
        Ground::HillFace => mix(
            mix(grass_dark, olive, 0.6),
            sand,
            0.1 + 0.15 * fbm2(x * 0.02, z * 0.02, 27),
        ),
        Ground::Rock => mix(
            lin(palette::ROCK),
            lin(palette::ROCK_WARM),
            fbm2(x * 0.2, z * 0.2, 24),
        ),
        Ground::Apron => mix(dirt, sand, 0.3),
        Ground::Plateau => {
            // Plateau and hills: sage, olive and darker grass, drier far away.
            let d = edge_distance(x, z);
            let tone = fbm2(x * 0.04, z * 0.04, 25);
            let mut col = mix(grass_dark, grass, smoothstep(0.3, 0.7, tone));
            col = mix(
                col,
                olive,
                smoothstep(0.5, 0.8, fbm2(x * 0.02, z * 0.02, 26)) * 0.6,
            );
            col = mix(col, lin(palette::SAGE), smoothstep(80.0, 400.0, d) * 0.45);
            mix(col, sand, smoothstep(30.0, 80.0, p.y) * 0.15)
        }
    }
}

// ---------------------------------------------------------------------------
// Boundary cliffs: broad, overlapping rock masses with stepped strata ledges,
// behind a rubble apron. The skyline dips into grassy saddles and rises again.
// ---------------------------------------------------------------------------

/// A broad faceted rock mass from y = -0.8 up to `top`, with two strata ledges
/// and a beveled, slightly tilted top. Neighbors overlap into one cliff wall.
fn rock_mass(geo: &mut Geo, base: Vec3, radius: f32, top: f32, grassy: bool, rng: &mut Rng) {
    let sides = 6 + (rng.next_u64() % 3) as usize;
    let phase = rng.range(0.0, TAU);
    let radii: Vec<f32> = (0..sides).map(|_| radius * rng.range(0.8, 1.15)).collect();
    let h = (top - base.y).max(1.0);
    let lean = Vec3::new(rng.range(-1.0, 1.0), 0.0, rng.range(-1.0, 1.0)) * 0.06 * radius;
    // (height fraction, radius scale): ledges where the radius steps in.
    // Ledge heights vary per mass so strata don't line up like masonry.
    let (l1, l2) = (rng.range(0.22, 0.42), rng.range(0.55, 0.78));
    let (s1, s2) = (rng.range(0.9, 0.95), rng.range(0.82, 0.88));
    let tall = [
        (0.0, 1.05),
        (l1, 1.0),
        (l1 + 0.05, s1),
        (l2, s1 - 0.02),
        (l2 + 0.05, s2),
        (0.93, s2 - 0.04),
        (1.0, 0.6),
    ];
    let short = [
        (0.0, 1.05),
        (l2 - 0.1, 0.98),
        (l2 - 0.04, s1 - 0.02),
        (0.9, s2),
        (1.0, 0.62),
    ];
    let profile: &[(f32, f32)] = if h > 4.5 { &tall } else { &short };
    let mut rings: Vec<Vec<Vec3>> = profile
        .iter()
        .enumerate()
        .map(|(k, &(f, s))| {
            let wobble = rng.range(0.97, 1.03);
            ring(
                base + lean * f,
                sides,
                phase + k as f32 * 0.03,
                h * f,
                |i| radii[i] * s * wobble,
            )
        })
        .collect();
    let tilt = Vec2::new(rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)) * 0.08;
    if let Some(top_ring) = rings.last_mut() {
        for p in top_ring.iter_mut() {
            p.y += Vec2::new(p.x - base.x, p.z - base.z).dot(tilt);
        }
    }
    let rock = lin(palette::ROCK);
    let warm = mix(lin(palette::ROCK_WARM), lin(palette::SAND), 0.2);
    let light = lin(palette::ROCK_LIGHT);
    let moss = mix(
        lin(palette::GRASS_DARK),
        lin(palette::OLIVE),
        rng.next_f32(),
    );
    let body = mix(rock, warm, rng.range(0.0, 0.8));
    let alt = mix(body, light, 0.3);
    let bands = rings.len() - 1;
    let mut face = rng.fork(17);
    geo.loft(&rings, |band, _| {
        let c = if band + 1 == bands {
            if grassy { mix(alt, moss, 0.5) } else { alt }
        } else if band % 2 == 1 {
            // Ledge bands: a lighter stratum.
            mix(body, light, 0.45)
        } else if band >= 2 {
            alt
        } else {
            shade(body, 0.95)
        };
        shade(c, jitter(&mut face, 0.06))
    });
    let cap = if grassy { moss } else { mix(light, moss, 0.2) };
    geo.cap(
        rings.last().expect("profile"),
        true,
        shade(cap, jitter(rng, 0.05)),
    );
}

/// Pushes a finished rock outward until every vertex clears the arena edge.
fn keep_clear(rock: &mut Geo, outward: Vec3) {
    for _ in 0..40 {
        let min = rock
            .vertices()
            .map(|v| edge_distance(v.x, v.z))
            .fold(f32::MAX, f32::min);
        if min >= EDGE_CLEARANCE {
            return;
        }
        let shift = outward * (EDGE_CLEARANCE - min + 0.05);
        for p in &mut rock.positions {
            p[0] += shift.x;
            p[2] += shift.z;
        }
    }
}

/// A squat, faceted boulder partly sunk into the ground.
fn boulder(geo: &mut Geo, at: Vec3, r: f32, rng: &mut Rng) {
    let squash = rng.range(0.55, 0.85);
    let mut shape = rng.fork(3);
    let tone = rng.next_f32();
    let base = mix(
        lin(palette::ROCK),
        mix(lin(palette::ROCK_WARM), lin(palette::SAND), 0.2),
        rng.next_f32(),
    );
    let mut face = rng.fork(4);
    blob(
        geo,
        0,
        |v| {
            at + Vec3::new(
                v.x * r * shape.range(0.8, 1.2),
                (v.y * squash * shape.range(0.85, 1.1) + 0.3) * r,
                v.z * r * shape.range(0.8, 1.2),
            )
        },
        |n| {
            let c = mix(base, lin(palette::ROCK_LIGHT), tone * 0.5);
            shade(c, (0.9 + 0.12 * n.y) * jitter(&mut face, 0.05))
        },
    );
}

fn cliffs(geo: &mut Geo, rng: &mut Rng) {
    // Each side: a point on the edge `along` its tangent, pushed out by `out`.
    let sides = [
        (Vec3::NEG_Z, Vec3::X),
        (Vec3::X, Vec3::Z),
        (Vec3::Z, Vec3::NEG_X),
        (Vec3::NEG_X, Vec3::NEG_Z),
    ];
    for (outward, tangent) in sides {
        // Main cliff: broad overlapping masses with a varied skyline.
        let mut t = -ARENA_HALF - 6.0 + rng.range(0.0, 2.0);
        while t < ARENA_HALF + 6.0 {
            let out = rng.range(4.2, 5.6);
            let pos = outward * (ARENA_HALF + out) + tangent * t;
            let plateau = plateau_height(pos.x, pos.z);
            let skyline = fbm2(pos.x * 0.04 + 17.0, pos.z * 0.04 - 3.0, 42);
            let top = if skyline < 0.38 {
                // Grassy saddle: the rock stays under the plateau lip.
                plateau * rng.range(0.55, 0.8) + 0.5
            } else {
                plateau + 0.4 + (skyline - 0.38) * 9.0 + rng.range(0.0, 1.0)
            };
            let mut rock = Geo::default();
            rock_mass(
                &mut rock,
                Vec3::new(pos.x, -0.8, pos.z),
                rng.range(3.4, 4.8),
                top,
                rng.chance(0.75),
                rng,
            );
            keep_clear(&mut rock, outward);
            geo.append(&rock, &Transform::IDENTITY);
            t += rng.range(3.2, 4.6);
        }
        // Rubble apron: squat boulders with gaps, in front of the cliff.
        let mut t = -ARENA_HALF - 3.0 + rng.range(0.0, 2.0);
        while t < ARENA_HALF + 3.0 {
            let step = rng.range(2.2, 5.0);
            if rng.chance(0.7) {
                let out = rng.range(1.2, 2.4);
                let pos = outward * (ARENA_HALF + out) + tangent * t;
                let r = rng.range(0.6, 1.4);
                let mut rock = Geo::default();
                boulder(&mut rock, Vec3::new(pos.x, -0.25 * r, pos.z), r, rng);
                keep_clear(&mut rock, outward);
                geo.append(&rock, &Transform::IDENTITY);
            }
            t += step;
        }
        // Low stones on the apron: the visible edge of the playable floor.
        let mut t = -ARENA_HALF + rng.range(0.0, 3.0);
        while t < ARENA_HALF {
            let out = rng.range(0.6, 1.2);
            let pos = outward * (ARENA_HALF + out) + tangent * t;
            let r = rng.range(0.2, 0.45);
            let mut stone = Geo::default();
            boulder(&mut stone, Vec3::new(pos.x, -0.15 * r, pos.z), r, rng);
            keep_clear(&mut stone, outward);
            geo.append(&stone, &Transform::IDENTITY);
            t += rng.range(1.5, 4.5);
        }
    }
}

// ---------------------------------------------------------------------------
// Trees and boulders
// ---------------------------------------------------------------------------

fn tree(geo: &mut Geo, base: Vec3, scale: f32, detail: u32, rng: &mut Rng) {
    let trunk_h = rng.range(1.5, 2.3) * scale;
    let trunk_r = rng.range(0.2, 0.28) * scale;
    let phase = rng.range(0.0, TAU);
    let trunk = lin(palette::TRUNK);
    let trunk_rings = [
        ring(base, 5, phase, -0.6, |_| trunk_r * 1.2),
        ring(base, 5, phase, trunk_h, |_| trunk_r * 0.75),
    ];
    geo.loft(&trunk_rings, |_, i| {
        shade(trunk, 0.9 + 0.1 * (i % 2) as f32)
    });
    let foliage = [
        lin(palette::FOLIAGE),
        lin(palette::GRASS_DARK),
        lin(palette::OLIVE),
        lin(palette::FOLIAGE_LIGHT),
    ];
    let leaf = foliage[(rng.next_u64() % foliage.len() as u64) as usize];
    let top = base + Vec3::Y * trunk_h;
    if rng.chance(0.4) {
        // Pine: three stacked faceted cones.
        let mut y = trunk_h * 0.55;
        let mut r = rng.range(1.5, 1.9) * scale;
        for tier in 0..3 {
            let h = r * rng.range(1.25, 1.5);
            let sides = 6;
            let p = phase + tier as f32 * 0.5;
            let skirt = ring(base, sides, p, y, |_| r);
            let tip = vec![base + Vec3::Y * (y + h); sides];
            let tint = shade(leaf, 0.92 + 0.06 * tier as f32);
            let mut face = rng.fork(tier);
            geo.loft(&[skirt.clone(), tip], |_, _| {
                shade(tint, jitter(&mut face, 0.06))
            });
            geo.cap(&skirt, false, shade(tint, 0.8));
            y += h * 0.45;
            r *= 0.72;
        }
    } else {
        // Broadleaf: a few chunky jittered blobs.
        let blobs = 2 + (rng.next_u64() % 2) as usize;
        for k in 0..blobs {
            let r = rng.range(1.2, 1.7) * scale * if k == 0 { 1.0 } else { 0.8 };
            let offset = if k == 0 {
                Vec3::new(0.0, r * 0.7, 0.0)
            } else {
                Vec3::new(
                    rng.range(-0.9, 0.9) * scale,
                    rng.range(0.6, 1.4) * scale,
                    rng.range(-0.9, 0.9) * scale,
                )
            };
            let center = top + offset;
            let mut shape_rng = rng.fork(k as u64 + 11);
            let mut face_rng = rng.fork(k as u64 + 21);
            blob(
                geo,
                detail,
                |v| center + v * r * Vec3::new(1.0, 0.86, 1.0) * shape_rng.range(0.88, 1.12),
                |n| {
                    let lift = 0.94 + 0.12 * n.y.max(0.0);
                    shade(leaf, lift * jitter(&mut face_rng, 0.05))
                },
            );
        }
    }
}

fn trees(s: &mut Scenery, rng: &mut Rng) {
    let mut placed: Vec<Vec2> = Vec::new();
    let sun = sun_direction().xz().normalize();
    // (min edge distance, max edge distance, count, spacing, scale range, detail, near?)
    let rings = [
        (6.5, 16.0, 70, 3.6, (0.9, 1.25), 1, true),
        (16.0, 110.0, 150, 5.0, (1.0, 1.4), 0, false),
        (110.0, 320.0, 130, 9.0, (1.6, 2.4), 0, false),
    ];
    for (dmin, dmax, count, spacing, (smin, smax), detail, near) in rings {
        let mut made = 0;
        let mut attempts = 0;
        while made < count && attempts < count * 60 {
            attempts += 1;
            let reach = ARENA_HALF + dmax;
            let (x, z) = (rng.range(-reach, reach), rng.range(-reach, reach));
            let d = edge_distance(x, z);
            if d < dmin || d > dmax {
                continue;
            }
            // Forest patches, thinner on the sun side near the arena.
            let patch = fbm2(x * 0.03, z * 0.03, 51);
            let sunward = Vec2::new(x, z).normalize_or_zero().dot(sun).max(0.0);
            let keep =
                smoothstep(0.35, 0.6, patch) * (1.0 - if near { 0.7 * sunward } else { 0.0 });
            if rng.next_f32() > keep {
                continue;
            }
            let p = Vec2::new(x, z);
            if placed
                .iter()
                .any(|q| q.distance_squared(p) < spacing * spacing)
            {
                continue;
            }
            placed.push(p);
            let base = Vec3::new(x, height(x, z), z);
            let scale = rng.range(smin, smax);
            let target = if near {
                &mut s.near_trees
            } else {
                &mut s.backdrop
            };
            tree(target, base, scale, detail, rng);
            made += 1;
        }
    }
}

fn boulders(geo: &mut Geo, rng: &mut Rng) {
    let mut made = 0;
    while made < 40 {
        let reach = ARENA_HALF + 60.0;
        let (x, z) = (rng.range(-reach, reach), rng.range(-reach, reach));
        let d = edge_distance(x, z);
        if !(7.0..60.0).contains(&d) {
            continue;
        }
        let r = rng.range(0.8, 2.4);
        let base = Vec3::new(x, height(x, z) + r * 0.2, z);
        let mut shape = rng.fork(made);
        let tone = rng.next_f32();
        blob(
            geo,
            0,
            |v| {
                base + v
                    * r
                    * Vec3::new(
                        shape.range(0.8, 1.2),
                        shape.range(0.5, 0.8),
                        shape.range(0.8, 1.2),
                    )
            },
            |n| {
                shade(
                    mix(lin(palette::ROCK), lin(palette::ROCK_LIGHT), tone),
                    0.9 + 0.1 * n.y,
                )
            },
        );
        made += 1;
    }
}

// ---------------------------------------------------------------------------
// Mesas: layered flat-topped buttes on the horizon.
// ---------------------------------------------------------------------------

fn mesas(geo: &mut Geo, rng: &mut Rng) {
    // (azimuth degrees clockwise from north, distance, radius, height)
    let spots = [
        (228.0, 330.0, 58.0, 70.0),
        (252.0, 420.0, 70.0, 95.0),
        (275.0, 360.0, 44.0, 58.0),
        (300.0, 470.0, 60.0, 80.0),
        (345.0, 440.0, 52.0, 66.0),
        (20.0, 500.0, 75.0, 90.0),
        (95.0, 420.0, 56.0, 62.0),
        (160.0, 460.0, 64.0, 74.0),
    ];
    let sand = lin(palette::SAND);
    let mesa = lin(palette::MESA);
    let mesa_dark = lin(palette::MESA_DARK);
    for (az, dist, radius, h) in spots {
        let a = f32::to_radians(az);
        let center = Vec3::new(a.sin() * dist, 0.0, -a.cos() * dist);
        let sides = 8 + (rng.next_u64() % 3) as usize;
        let phase = rng.range(0.0, TAU);
        let radii: Vec<f32> = (0..sides).map(|_| radius * rng.range(0.85, 1.12)).collect();
        let ground = height(center.x, center.z) - 25.0;
        let profile = [
            (ground, 1.35),
            (h * 0.22, 1.08),
            (h * 0.3, 0.97),
            (h * 0.55, 0.95),
            (h * 0.6, 0.91),
            (h * 0.86, 0.9),
            (h, 0.86),
        ];
        let rings: Vec<Vec<Vec3>> = profile
            .iter()
            .enumerate()
            .map(|(k, &(y, s))| ring(center, sides, phase + k as f32 * 0.02, y, |i| radii[i] * s))
            .collect();
        let bands = [
            mix(sand, mesa, 0.3),
            mesa_dark,
            mix(mesa, sand, 0.4),
            mesa_dark,
            mesa,
            mix(mesa, sand, 0.5),
        ];
        let mut face = rng.fork(9);
        geo.loft(&rings, |band, _| {
            shade(bands[band.min(bands.len() - 1)], jitter(&mut face, 0.05))
        });
        geo.cap(
            rings.last().expect("profile"),
            true,
            mix(lin(palette::OLIVE), sand, 0.4),
        );
    }
}

// ---------------------------------------------------------------------------
// Clouds: puffy faceted clusters with baked sun-side shading (unlit, fog-free).
// ---------------------------------------------------------------------------

fn clouds(geo: &mut Geo, rng: &mut Rng) {
    let sun = sun_direction();
    let lit = lin(palette::CLOUD);
    let shadow = lin(palette::CLOUD_SHADE);
    // (azimuth degrees, distance, height)
    let spots = [
        (205.0, 420.0, 150.0),
        (238.0, 520.0, 185.0),
        (262.0, 380.0, 125.0),
        (290.0, 470.0, 165.0),
        (320.0, 400.0, 150.0),
        (350.0, 520.0, 190.0),
        (15.0, 430.0, 140.0),
        (48.0, 500.0, 175.0),
        (80.0, 380.0, 130.0),
        (118.0, 470.0, 160.0),
        (150.0, 420.0, 180.0),
        (178.0, 520.0, 145.0),
    ];
    for (az, dist, h) in spots {
        let a = f32::to_radians(az + rng.range(-6.0, 6.0));
        let center = Vec3::new(a.sin() * dist, h, -a.cos() * dist);
        let along = Vec3::new(a.cos(), 0.0, a.sin());
        let puffs = 4 + (rng.next_u64() % 4) as usize;
        let length = rng.range(55.0, 100.0);
        for k in 0..puffs {
            let t = k as f32 / (puffs - 1) as f32 - 0.5;
            let bulge = 1.0 - (2.0 * t).abs() * 0.5;
            let size = Vec3::new(
                rng.range(17.0, 26.0),
                rng.range(9.0, 14.0),
                rng.range(13.0, 19.0),
            ) * bulge;
            let offset = along * t * length
                + Vec3::new(0.0, rng.range(-1.0, 4.0) * bulge, rng.range(-5.0, 5.0));
            let c = center + offset;
            let mut shape = rng.fork(k as u64);
            blob(
                geo,
                1,
                |v| {
                    // Flatten the underside.
                    let v = Vec3::new(v.x, v.y.max(-0.35), v.z);
                    c + v * size * shape.range(0.93, 1.07)
                },
                |n| {
                    let light = 0.55 * n.dot(sun) + 0.45 * n.y;
                    mix(shadow, lit, smoothstep(-0.35, 0.55, light))
                },
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Floor clutter: grass tufts and pebbles (low, sparse, never in the way).
// ---------------------------------------------------------------------------

fn quadrant(x: f32, z: f32) -> usize {
    (x >= 0.0) as usize + 2 * (z >= 0.0) as usize
}

fn grass_tuft(geo: &mut Geo, at: Vec3, scale: f32, rng: &mut Rng) {
    let blades = 3 + (rng.next_u64() % 3) as usize;
    let root = lin(palette::GRASS_DARK);
    let tip_base = mix(
        lin(palette::GRASS),
        lin(palette::SAND),
        rng.range(0.1, 0.35),
    );
    let tip = shade(tip_base, rng.range(1.0, 1.12));
    let up = Vec3::Y;
    for _ in 0..blades {
        let a = rng.range(0.0, TAU);
        let facing = Vec3::new(a.cos(), 0.0, a.sin());
        let side = facing.cross(up);
        let base = at + Vec3::new(rng.range(-0.07, 0.07), 0.0, rng.range(-0.07, 0.07)) * scale;
        let w = rng.range(0.035, 0.055) * scale;
        let h = rng.range(0.1, 0.2) * scale;
        let lean = facing * rng.range(0.02, 0.07) * scale;
        let top = base + lean + Vec3::Y * h;
        geo.tri_raw(
            [base - side * w * 0.5, base + side * w * 0.5, top],
            [up, up, up],
            [shade(root, 0.85), shade(root, 0.85), tip],
        );
    }
}

fn grass(count: usize, rng: &mut Rng) -> Vec<Geo> {
    let mut chunks = vec![Geo::default(); 4];
    let mut made = 0;
    let mut attempts = 0;
    let reach = ARENA_HALF - 0.3;
    while made < count && attempts < count * 40 {
        attempts += 1;
        let (x, z) = (rng.range(-reach, reach), rng.range(-reach, reach));
        let rim = x.abs().max(z.abs());
        let edge = smoothstep(12.0, 23.0, rim);
        let patch = smoothstep(0.45, 0.72, fbm2(x * 0.12, z * 0.12, 31));
        let density = (0.08 + 0.55 * edge + 0.5 * patch) * (1.0 - sand_mask(x, z));
        if rng.next_f32() > density {
            continue;
        }
        let scale = rng.range(0.85, 1.25);
        grass_tuft(
            &mut chunks[quadrant(x, z)],
            Vec3::new(x, 0.0, z),
            scale,
            rng,
        );
        made += 1;
    }
    chunks
}

fn pebbles(geo: &mut Geo, rng: &mut Rng) {
    let mut made = 0;
    let reach = ARENA_HALF - 0.2;
    while made < 260 {
        let (x, z) = (rng.range(-reach, reach), rng.range(-reach, reach));
        let rim = x.abs().max(z.abs());
        let density = 0.15 + 0.6 * smoothstep(19.0, 23.5, rim) + 0.5 * sand_mask(x, z);
        if rng.next_f32() > density {
            continue;
        }
        let r = rng.range(0.05, 0.15);
        let squash = rng.range(0.45, 0.7);
        let tone = rng.next_f32();
        let mut shape = rng.fork(made);
        let color = mix(lin(palette::ROCK), lin(palette::ROCK_LIGHT), tone);
        blob(
            geo,
            0,
            |v| {
                Vec3::new(
                    x + v.x * r * shape.range(0.8, 1.2),
                    (v.y * squash + squash * 0.4) * r,
                    z + v.z * r * shape.range(0.8, 1.2),
                )
            },
            |n| shade(color, 0.9 + 0.12 * n.y),
        );
        made += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generated() -> &'static Scenery {
        static SCENERY: std::sync::OnceLock<Scenery> = std::sync::OnceLock::new();
        SCENERY.get_or_init(Scenery::generate)
    }

    #[test]
    fn playable_floor_is_flat_at_zero() {
        for i in 0..=96 {
            for j in 0..=96 {
                let x = -ARENA_HALF + i as f32 * 0.5;
                let z = -ARENA_HALF + j as f32 * 0.5;
                assert_eq!(height(x, z), 0.0, "floor not flat at ({x}, {z})");
            }
        }
        for v in generated().ground.vertices() {
            if edge_distance(v.x, v.z) <= 0.0 {
                assert_eq!(v.y, 0.0, "floor vertex off the collision plane: {v}");
            }
        }
    }

    #[test]
    fn rocks_and_trees_stay_outside_the_playable_edge() {
        let s = generated();
        for geo in [&s.cliffs, &s.near_trees, &s.backdrop] {
            for v in geo.vertices() {
                assert!(
                    edge_distance(v.x, v.z) >= EDGE_CLEARANCE - 1e-3,
                    "scenery intrudes at {v}"
                );
            }
        }
    }

    #[test]
    fn floor_clutter_is_low_and_inside() {
        let s = generated();
        for geo in s.grass.iter().chain(&s.dense_grass).chain([&s.pebbles]) {
            for v in geo.vertices() {
                assert!(v.y <= FLOOR_CLUTTER_MAX_HEIGHT, "clutter too tall: {v}");
                assert!(v.x.abs() <= ARENA_HALF && v.z.abs() <= ARENA_HALF);
            }
        }
    }

    #[test]
    fn scenery_is_deterministic_and_within_budget() {
        let a = Scenery::generate();
        let b = generated();
        assert_eq!(a.ground.positions, b.ground.positions);
        assert_eq!(a.cliffs.colors, b.cliffs.colors);
        let total: usize = [
            &a.ground,
            &a.far_ground,
            &a.cliffs,
            &a.near_trees,
            &a.backdrop,
            &a.clouds,
            &a.pebbles,
        ]
        .iter()
        .map(|g| g.tri_count())
        .sum::<usize>()
            + a.grass
                .iter()
                .chain(&a.dense_grass)
                .map(Geo::tri_count)
                .sum::<usize>();
        assert!(total < 150_000, "scenery has {total} triangles");
        assert_eq!(a.grass.len(), 4);
        assert!(a.grass.iter().all(|g| !g.is_empty()));
    }

    #[test]
    fn terrain_grid_has_no_cracks_between_levels() {
        // A fine border vertex must lie on the coarse edge it touches.
        for level in 0..LEVELS.len() - 1 {
            let l = &LEVELS[level];
            let coarse = &LEVELS[level + 1];
            let mut along = -l.half;
            while along <= l.half {
                let fine = level_height(level, l.half, along);
                let a = (along / coarse.step).floor() * coarse.step;
                let t = (along - a) / coarse.step;
                let expect = level_height(level + 1, l.half, a) * (1.0 - t)
                    + level_height(level + 1, l.half, a + coarse.step) * t;
                assert!(
                    (fine - expect).abs() < 1e-3,
                    "crack at level {level}, {along}"
                );
                along += l.step;
            }
        }
    }
}
