//! The spell shapes, built once at startup (never per event).
//!
//! **Glow shapes** are drawn with the additive [`super::material::SpellMaterial`]:
//! their `COLOR_0` is the glow colour (linear) and the vertex alpha its weight,
//! so a star can be white-hot in the middle and blue at the tips in one mesh.
//! Flat cards lie in the XY plane facing +Z (a billboard turns +Z toward the
//! camera); long shapes (streaks, ribbons) lie in the XZ plane along +Z with
//! their face on +Y, for axial billboarding around their length.
//!
//! **Solid shapes** (cloud puffs, brick chips, wood splinters, gold stars) are
//! toon-shaded with vertex colours.
//!
//! Every builder here is deterministic (seeded), and sizes are unit sizes that
//! the effects scale.

use crate::{
    fx::sim::FxRng,
    palette::cartoon,
    viewmodel::mesh::{ModelBuilder, linear, shade},
};
use bevy::{
    asset::RenderAssetUsages,
    math::Affine3A,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use std::f32::consts::{PI, TAU};

// ---------------------------------------------------------------------------
// Glow colours (sRGB)
// ---------------------------------------------------------------------------

/// The rifle's blue spell (T03, T05): a white-hot core fading to these.
pub const BOLT_CORE: Color = Color::srgb(0.86, 0.97, 1.0);
pub const BOLT_BLUE: Color = Color::srgb(0.32, 0.78, 1.0);
pub const BOLT_DEEP: Color = Color::srgb(0.16, 0.48, 1.0);
/// Headshot gold (T06).
pub const GOLD_CORE: Color = Color::srgb(1.0, 0.97, 0.78);
pub const GOLD: Color = Color::srgb(1.0, 0.84, 0.28);
pub const GOLD_DEEP: Color = Color::srgb(1.0, 0.56, 0.12);
/// The pump's violet (T04).
pub const VIOLET_CORE: Color = Color::srgb(1.0, 0.86, 1.0);
pub const VIOLET: Color = Color::srgb(0.84, 0.36, 1.0);
pub const VIOLET_DEEP: Color = Color::srgb(0.55, 0.18, 0.95);
/// Shield glass (T07).
pub const GLASS_EDGE: Color = Color::srgb(0.8, 0.97, 1.0);
pub const GLASS: Color = Color::srgb(0.36, 0.78, 1.0);

fn lin(color: Color, alpha: f32) -> [f32; 4] {
    linear(color, alpha)
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    let (a, b) = (a.to_linear(), b.to_linear());
    Color::LinearRgba(LinearRgba::new(
        a.red + (b.red - a.red) * t,
        a.green + (b.green - a.green) * t,
        a.blue + (b.blue - a.blue) * t,
        1.0,
    ))
}

// ---------------------------------------------------------------------------
// Glow shapes
// ---------------------------------------------------------------------------

/// A star as a triangle fan: `tips` alternate with `valleys`. Each tip has its
/// own colour, so a burst can alternate violet and gold.
struct StarSpec {
    points: usize,
    /// Tip radius per point (0..=0.5).
    tip: Vec<f32>,
    /// Valley radius.
    valley: f32,
    core: [f32; 4],
    tip_colors: Vec<[f32; 4]>,
    valley_color: [f32; 4],
    twist: f32,
}

fn star_fan(m: &mut ModelBuilder, s: &StarSpec) {
    let n = s.points;
    let mut ring = Vec::with_capacity(n * 2);
    let mut colors = Vec::with_capacity(n * 2);
    for i in 0..n * 2 {
        let a = s.twist + PI * i as f32 / n as f32;
        let (r, c) = if i % 2 == 0 {
            (s.tip[i / 2], s.tip_colors[(i / 2) % s.tip_colors.len()])
        } else {
            (s.valley, s.valley_color)
        };
        ring.push(Vec3::new(r * a.cos(), r * a.sin(), 0.0));
        colors.push(c);
    }
    for i in 0..ring.len() {
        let j = (i + 1) % ring.len();
        m.poly_colored(
            &[Vec3::ZERO, ring[i], ring[j]],
            &[s.core, colors[i], colors[j]],
            Vec3::Z,
        );
    }
}

/// The bolt's head (T03): a four-point sparkle with short diagonal points,
/// white in the middle, `tip` at the points. Radius 0.5.
pub fn sparkle(tip: Color, core: Color) -> Mesh {
    let mut m = ModelBuilder::new();
    let long = 0.5;
    let short = 0.2;
    star_fan(
        &mut m,
        &StarSpec {
            points: 8,
            tip: (0..8)
                .map(|i| if i % 2 == 0 { long } else { short })
                .collect(),
            valley: 0.075,
            core: lin(Color::WHITE, 1.0),
            tip_colors: vec![lin(tip, 0.55), lin(tip, 0.45)],
            valley_color: lin(core, 0.95),
            twist: PI / 2.0,
        },
    );
    m.build()
}

/// The rifle bolt's head (T03): a big jagged starburst, saturated blue from
/// its white-hot middle out to deep blue tips, under a crisp white four-point
/// sparkle. Radius 0.5.
pub fn bolt_head() -> Mesh {
    let mut rng = FxRng::new(0xB017);
    let mut m = ModelBuilder::new();
    let points = 12;
    star_fan(
        &mut m,
        &StarSpec {
            points,
            tip: (0..points)
                .map(|i| {
                    if i % 2 == 0 {
                        rng.range(0.42, 0.5)
                    } else {
                        rng.range(0.25, 0.36)
                    }
                })
                .collect(),
            valley: 0.1,
            core: lin(BOLT_CORE, 1.0),
            tip_colors: vec![lin(BOLT_BLUE, 0.85), lin(BOLT_DEEP, 0.8)],
            valley_color: lin(BOLT_BLUE, 0.95),
            twist: rng.range(0.0, TAU),
        },
    );
    m.with(Affine3A::from_translation(Vec3::Z * 0.004), |m| {
        star_fan(
            m,
            &StarSpec {
                points: 4,
                tip: vec![0.34; 4],
                valley: 0.05,
                core: lin(Color::WHITE, 1.0),
                tip_colors: vec![lin(BOLT_CORE, 0.9)],
                valley_color: lin(Color::WHITE, 1.0),
                twist: PI / 2.0,
            },
        );
    });
    m.build()
}

/// A jagged starburst (impacts, T03 and T05): `points` spikes of seeded,
/// uneven length. `tips` cycle around the spikes. Its body is `core` all the
/// way into the middle (saturated, not washed white), with a small white-hot
/// star at its heart. Radius 0.5.
pub fn starburst(points: usize, seed: u64, core: Color, tips: &[Color], valley: f32) -> Mesh {
    let mut rng = FxRng::new(seed);
    let mut m = ModelBuilder::new();
    star_fan(
        &mut m,
        &StarSpec {
            points,
            tip: (0..points).map(|_| rng.range(0.3, 0.5)).collect(),
            valley,
            core: lin(mix(core, Color::WHITE, 0.35), 1.0),
            tip_colors: tips.iter().map(|c| lin(*c, 0.75)).collect(),
            valley_color: lin(core, 0.9),
            twist: rng.range(0.0, TAU),
        },
    );
    white_heart(&mut m, 0.13);
    m.build()
}

/// A small white-hot six-point star (radius `r`) just in front of a burst.
fn white_heart(m: &mut ModelBuilder, r: f32) {
    m.with(Affine3A::from_translation(Vec3::Z * 0.003), |m| {
        star_fan(
            m,
            &StarSpec {
                points: 6,
                tip: vec![r; 6],
                valley: r * 0.45,
                core: lin(Color::WHITE, 1.0),
                tip_colors: vec![lin(Color::WHITE, 0.0)],
                valley_color: lin(Color::WHITE, 0.6),
                twist: 0.3,
            },
        );
    });
}

/// A spark: a teardrop in the XZ plane (face +Y), its round head at z = 0 and
/// its tail thinning to nothing at z = 1. Width 1 at the head. Its middle is
/// `core`, its body `body`, fading to `tail` down the tail (T04's teardrops).
pub fn streak(core: Color, body: Color, tail: Color) -> Mesh {
    let mut m = ModelBuilder::new();
    let center = Vec3::new(0.0, 0.0, 0.08);
    // (x, z, alpha, tail blend)
    let rim = [
        (0.0, -0.16, 0.9, 0.0),
        (0.32, -0.11, 0.9, 0.0),
        (0.5, 0.03, 0.85, 0.1),
        (0.36, 0.24, 0.7, 0.35),
        (0.13, 0.58, 0.4, 0.8),
        (0.0, 1.0, 0.0, 1.0),
        (-0.13, 0.58, 0.4, 0.8),
        (-0.36, 0.24, 0.7, 0.35),
        (-0.5, 0.03, 0.85, 0.1),
        (-0.32, -0.11, 0.9, 0.0),
    ];
    let pts: Vec<Vec3> = rim.iter().map(|&(x, z, ..)| Vec3::new(x, 0.0, z)).collect();
    let cols: Vec<[f32; 4]> = rim
        .iter()
        .map(|&(_, _, a, t)| lin(mix(body, tail, t), a))
        .collect();
    for i in 0..pts.len() {
        let j = (i + 1) % pts.len();
        m.poly_colored(
            &[center, pts[i], pts[j]],
            &[lin(core, 1.0), cols[i], cols[j]],
            Vec3::Y,
        );
    }
    m.build()
}

/// A bolt's trail: a soft beam along +Z from 0 (the muzzle) to 1 (the head),
/// in the XZ plane (face +Y). It tapers to a quarter width at the muzzle end,
/// is brightest along its middle and fades out at its edges and toward the
/// muzzle. Width 1 at the head end.
pub fn ribbon(core: Color, edge: Color) -> Mesh {
    let mut m = ModelBuilder::new();
    let steps = 6;
    let row = |k: usize| {
        let z = k as f32 / steps as f32;
        let w = 0.5 * (0.25 + 0.75 * z);
        let a = 0.2 + 0.8 * z;
        (
            [
                Vec3::new(-w, 0.0, z),
                Vec3::new(0.0, 0.0, z),
                Vec3::new(w, 0.0, z),
            ],
            [lin(edge, 0.0), lin(core, a), lin(edge, 0.0)],
        )
    };
    for k in 0..steps {
        let (p0, c0) = row(k);
        let (p1, c1) = row(k + 1);
        for s in 0..2 {
            m.poly_colored(
                &[p0[s], p0[s + 1], p1[s + 1], p1[s]],
                &[c0[s], c0[s + 1], c1[s + 1], c1[s]],
                Vec3::Y,
            );
        }
    }
    m.build()
}

/// A soft ring in the XY plane (face +Z), brightest at radius 0.45, fading to
/// nothing at 0.38 and 0.5.
pub fn ring(color: Color) -> Mesh {
    let mut m = ModelBuilder::new();
    let radii = [(0.38, 0.0), (0.45, 1.0), (0.5, 0.0)];
    let n = 40;
    for i in 0..n {
        let (a0, a1) = (TAU * i as f32 / n as f32, TAU * (i + 1) as f32 / n as f32);
        let p = |r: f32, a: f32| Vec3::new(r * a.cos(), r * a.sin(), 0.0);
        for w in radii.windows(2) {
            let ((r0, al0), (r1, al1)) = (w[0], w[1]);
            m.poly_colored(
                &[p(r0, a0), p(r1, a0), p(r1, a1), p(r0, a1)],
                &[
                    lin(color, al0),
                    lin(color, al1),
                    lin(color, al1),
                    lin(color, al0),
                ],
                Vec3::Z,
            );
        }
    }
    m.build()
}

/// Three curved speed lines circling in the XZ plane (face +Y) at radius 0.5:
/// the swirl around a spinning hat (T08).
pub fn swirl(color: Color) -> Mesh {
    let mut m = ModelBuilder::new();
    let arcs = 3;
    let steps = 10;
    for k in 0..arcs {
        let start = TAU * k as f32 / arcs as f32;
        let span = 1.7;
        for s in 0..steps {
            let t0 = s as f32 / steps as f32;
            let t1 = (s + 1) as f32 / steps as f32;
            let (a0, a1) = (start + span * t0, start + span * t1);
            // Thick and bright at the head (t = 1), thin at the tail.
            let w0 = 0.004 + 0.018 * t0;
            let w1 = 0.004 + 0.018 * t1;
            let p = |r: f32, a: f32| Vec3::new(r * a.cos(), 0.0, r * a.sin());
            m.poly_colored(
                &[
                    p(0.5 - w0, a0),
                    p(0.5 + w0, a0),
                    p(0.5 + w1, a1),
                    p(0.5 - w1, a1),
                ],
                &[
                    lin(color, t0 * 0.9),
                    lin(color, t0 * 0.9),
                    lin(color, t1 * 0.9),
                    lin(color, t1 * 0.9),
                ],
                Vec3::Y,
            );
        }
    }
    m.build()
}

/// Vertices and faces of a geodesic unit sphere (`subdivisions` = 0 is the
/// icosahedron).
pub fn icosphere(subdivisions: u32) -> (Vec<Vec3>, Vec<[usize; 3]>) {
    let t = (1.0 + 5f32.sqrt()) / 2.0;
    let mut verts: Vec<Vec3> = [
        (-1.0, t, 0.0),
        (1.0, t, 0.0),
        (-1.0, -t, 0.0),
        (1.0, -t, 0.0),
        (0.0, -1.0, t),
        (0.0, 1.0, t),
        (0.0, -1.0, -t),
        (0.0, 1.0, -t),
        (t, 0.0, -1.0),
        (t, 0.0, 1.0),
        (-t, 0.0, -1.0),
        (-t, 0.0, 1.0),
    ]
    .iter()
    .map(|&(x, y, z)| Vec3::new(x, y, z).normalize())
    .collect();
    let mut faces: Vec<[usize; 3]> = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    for _ in 0..subdivisions {
        let mut mid = std::collections::HashMap::new();
        let mut midpoint = |a: usize, b: usize, verts: &mut Vec<Vec3>| -> usize {
            *mid.entry((a.min(b), a.max(b))).or_insert_with(|| {
                verts.push(((verts[a] + verts[b]) * 0.5).normalize());
                verts.len() - 1
            })
        };
        let mut next = Vec::with_capacity(faces.len() * 4);
        for [a, b, c] in faces {
            let ab = midpoint(a, b, &mut verts);
            let bc = midpoint(b, c, &mut verts);
            let ca = midpoint(c, a, &mut verts);
            next.extend([[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]);
        }
        faces = next;
    }
    (verts, faces)
}

/// The shield shimmer (T07's glass, as a hit): a unit sphere of glowing hex
/// (and twelve pentagon) cells, the dual of a geodesic sphere. Each cell is dim
/// inside with a bright rim, so the shell reads as a lattice of glass hexes.
pub fn hex_shell(rim: Color, fill: Color) -> Mesh {
    let (verts, faces) = icosphere(1);
    let mut m = ModelBuilder::new();
    for (i, &n) in verts.iter().enumerate() {
        let mut ring: Vec<Vec3> = faces
            .iter()
            .filter(|f| f.contains(&i))
            .map(|f| ((verts[f[0]] + verts[f[1]] + verts[f[2]]) / 3.0).normalize())
            .collect();
        let u = n.any_orthonormal_vector();
        let w = n.cross(u);
        ring.sort_by(|a, b| {
            let aa = (*a - n).dot(w).atan2((*a - n).dot(u));
            let bb = (*b - n).dot(w).atan2((*b - n).dot(u));
            aa.total_cmp(&bb)
        });
        let inner: Vec<Vec3> = ring.iter().map(|p| n.lerp(*p, 0.78)).collect();
        let (c_in, c_mid, c_rim) = (lin(fill, 0.05), lin(fill, 0.14), lin(rim, 0.85));
        for k in 0..ring.len() {
            let j = (k + 1) % ring.len();
            m.poly_colored(&[n, inner[k], inner[j]], &[c_in, c_mid, c_mid], n);
            m.poly_colored(
                &[inner[k], ring[k], ring[j], inner[j]],
                &[c_mid, c_rim, c_rim, c_mid],
                n,
            );
        }
    }
    m.build()
}

/// A big curved pane of broken shield glass (T07), about 1 across: a jagged
/// outline bent like a piece of a shell, bright at its edges.
pub fn glass_pane(seed: u64) -> Mesh {
    let mut rng = FxRng::new(seed);
    let mut m = ModelBuilder::new();
    let n = 6 + rng.pick(2);
    let bend = |p: Vec2| Vec3::new(p.x, p.y, -0.35 * (p.x * p.x + p.y * p.y));
    let outline: Vec<Vec2> = (0..n)
        .map(|i| {
            let a = TAU * (i as f32 + rng.range(-0.25, 0.25)) / n as f32;
            let r = rng.range(0.32, 0.55);
            Vec2::new(r * a.cos() * 0.8, r * a.sin())
        })
        .collect();
    let center = bend(Vec2::ZERO);
    let inner: Vec<Vec3> = outline.iter().map(|p| bend(*p * 0.72)).collect();
    let outer: Vec<Vec3> = outline.iter().map(|p| bend(*p)).collect();
    let (c0, c1, c2) = (lin(GLASS, 0.18), lin(GLASS, 0.3), lin(GLASS_EDGE, 1.0));
    for k in 0..n {
        let j = (k + 1) % n;
        m.poly_colored(&[center, inner[k], inner[j]], &[c0, c1, c1], Vec3::Z);
        m.poly_colored(
            &[inner[k], outer[k], outer[j], inner[j]],
            &[c1, c2, c2, c1],
            Vec3::Z,
        );
    }
    m.build()
}

/// A small sliver of glass, about 1 long: a thin triangle, bright at its edges.
pub fn glass_chip(seed: u64) -> Mesh {
    let mut rng = FxRng::new(seed);
    let mut m = ModelBuilder::new();
    let tri = [
        Vec3::new(-0.3, -0.5, 0.0),
        Vec3::new(0.3, rng.range(-0.45, -0.2), 0.0),
        Vec3::new(rng.range(-0.1, 0.15), 0.5, 0.0),
    ];
    let c = tri.iter().copied().sum::<Vec3>() / 3.0;
    for k in 0..3 {
        let j = (k + 1) % 3;
        m.poly_colored(
            &[c, tri[k], tri[j]],
            &[lin(GLASS, 0.3), lin(GLASS_EDGE, 1.0), lin(GLASS_EDGE, 1.0)],
            Vec3::Z,
        );
    }
    m.build()
}

/// Spikes in a cone around -Z from the origin, as crossed pairs of thin cards
/// so they read from any side: bright at the base, gone at the tip.
fn cone_spikes(
    m: &mut ModelBuilder,
    rng: &mut FxRng,
    count: usize,
    spread: f32,
    length: (f32, f32),
    width: f32,
    colors: &[Color],
) {
    for i in 0..count {
        let az = TAU * (i as f32 + rng.range(-0.3, 0.3)) / count as f32;
        let tilt = spread * rng.range(0.35, 1.0);
        let dir = Quat::from_axis_angle(Vec3::new(az.cos(), az.sin(), 0.0), tilt) * Vec3::NEG_Z;
        let len = rng.range(length.0, length.1);
        let color = colors[i % colors.len()];
        let side = dir.any_orthonormal_vector();
        for s in [side, dir.cross(side)] {
            let w = s * width * 0.5;
            let base = dir * 0.01;
            let mid = dir * (len * 0.35);
            let tip = dir * len;
            m.poly_colored(
                &[base, mid + w, tip, mid - w],
                &[
                    lin(mix(color, Color::WHITE, 0.5), 0.95),
                    lin(color, 0.85),
                    lin(color, 0.0),
                    lin(color, 0.85),
                ],
                s.cross(dir),
            );
        }
    }
}

/// The rifle's muzzle burst at `MuzzleTip` (viewmodel space, the barrel along
/// -Z): a big jagged cyan star facing back at the eye (T03's "pow"), a crisp
/// sparkle over it, and a spray of blue spikes out of the barrel.
pub fn rifle_muzzle_burst() -> Mesh {
    let mut m = ModelBuilder::new();
    let mut rng = FxRng::new(0xB1A5);
    let scale = |m: &mut ModelBuilder, k: f32, f: &dyn Fn(&mut ModelBuilder)| {
        m.with(Affine3A::from_scale(Vec3::splat(k)), f);
    };
    scale(&mut m, 0.11, &|m| {
        star_fan(
            m,
            &StarSpec {
                points: 13,
                tip: vec![
                    0.5, 0.3, 0.44, 0.27, 0.5, 0.33, 0.41, 0.26, 0.48, 0.31, 0.45, 0.28, 0.47,
                ],
                valley: 0.12,
                core: lin(BOLT_CORE, 1.0),
                tip_colors: vec![lin(BOLT_BLUE, 0.85), lin(BOLT_DEEP, 0.8)],
                valley_color: lin(BOLT_BLUE, 0.95),
                twist: 0.2,
            },
        );
    });
    scale(&mut m, 0.075, &|m| {
        m.with(Affine3A::from_translation(Vec3::Z * 0.02), |m| {
            star_fan(
                m,
                &StarSpec {
                    points: 4,
                    tip: vec![0.5; 4],
                    valley: 0.08,
                    core: lin(Color::WHITE, 1.0),
                    tip_colors: vec![lin(BOLT_CORE, 0.8)],
                    valley_color: lin(Color::WHITE, 1.0),
                    twist: PI / 4.0,
                },
            );
        });
    });
    cone_spikes(
        &mut m,
        &mut rng,
        9,
        0.45,
        (0.06, 0.14),
        0.02,
        &[BOLT_BLUE, BOLT_CORE, BOLT_DEEP],
    );
    m.build()
}

/// The pump's fan flash at the bell muzzle (T04): a white-pink core burst and
/// a wide cone of violet and gold spikes.
pub fn pump_muzzle_fan() -> Mesh {
    let mut m = ModelBuilder::new();
    let mut rng = FxRng::new(0xFA11);
    m.with(Affine3A::from_scale(Vec3::splat(0.13)), |m| {
        star_fan(
            m,
            &StarSpec {
                points: 14,
                tip: (0..14)
                    .map(|i| if i % 2 == 0 { 0.5 } else { 0.32 })
                    .collect(),
                valley: 0.15,
                core: lin(VIOLET_CORE, 1.0),
                tip_colors: vec![lin(VIOLET, 0.85), lin(GOLD, 0.85)],
                valley_color: lin(VIOLET, 0.95),
                twist: 0.1,
            },
        );
    });
    cone_spikes(
        &mut m,
        &mut rng,
        26,
        0.8,
        (0.08, 0.2),
        0.026,
        &[VIOLET, GOLD, VIOLET_DEEP, GOLD_DEEP, VIOLET],
    );
    m.build()
}

/// A crackling lightning arc for a gun's crystal chamber (T02, T04), in
/// chamber space: the gun's axis along Z, the glass about 0.05 m in radius.
/// Beside one end of the crystal (+Z), it zig-zags round the inside of the
/// glass through about half a turn, from the wall, bowing in toward the
/// crystal and back out to the wall, with a short fork off its middle. Two
/// crossed ribbons, so it reads from any side: `core` along the middle, fading
/// to `edge` and to nothing at the sides, dimmer toward both ends. Seeded, so
/// each seed is one arc shape; the game rolls, flips and swaps them.
pub fn lightning_arc(seed: u64, core: Color, edge: Color) -> Mesh {
    let mut rng = FxRng::new(seed);
    let mut m = ModelBuilder::new();
    let steps = 11;
    let span = rng.range(2.0, 2.9);
    let a0 = rng.range(-0.4, 0.4) - span * 0.5;
    let z0 = rng.range(0.035, 0.06);
    let drift = rng.range(-0.02, 0.02);
    let point = |t: f32, rng: &mut FxRng| -> Vec3 {
        let a = a0 + span * t;
        // Hugging the glass at the ends, bowing in toward the crystal between.
        let r = 0.046 - 0.012 * (t * PI).sin() + rng.range(-0.005, 0.005);
        let z = z0 + drift * t + rng.range(-0.008, 0.008);
        Vec3::new(r * a.cos(), r * a.sin(), z)
    };
    let mut main: Vec<Vec3> = (0..=steps)
        .map(|i| point(i as f32 / steps as f32, &mut rng))
        .collect();
    // Pin the ends onto the glass (no jitter there).
    let (first, last) = (main[0], main[steps]);
    main[0] = first.truncate().normalize().extend(0.0) * 0.048 + Vec3::Z * first.z;
    main[steps] = last.truncate().normalize().extend(0.0) * 0.048 + Vec3::Z * last.z;
    // A short fork off the middle, toward the crystal's end.
    let k = steps / 2 + rng.pick(3) - 1;
    let fork: Vec<Vec3> = (0..4)
        .map(|j| {
            let f = j as f32 / 3.0;
            main[k]
                + Vec3::new(rng.range(-0.004, 0.004), rng.range(-0.004, 0.004), 0.0)
                + Vec3::new(0.0, 0.0, -0.022 * f)
                + main[k].truncate().normalize().extend(0.0) * (0.006 * f)
        })
        .collect();
    let bolt = |m: &mut ModelBuilder, pts: &[Vec3], width: f32| {
        let n = pts.len();
        for i in 0..n - 1 {
            let (p, q) = (pts[i], pts[i + 1]);
            let t = (q - p).normalize_or(Vec3::Z);
            let radial = ((p + q) * 0.5).with_z(0.0).normalize_or(Vec3::X);
            // Thin at both ends, full in the middle.
            let w = |j: usize| width * (0.35 + 0.65 * ((j as f32 / (n - 1) as f32) * PI).sin());
            let fade = |j: usize| {
                let e = (j as f32 / (n - 1) as f32 * PI).sin();
                0.35 + 0.65 * e
            };
            for side in [t.cross(radial).normalize_or(Vec3::Z), radial] {
                let (wp, wq) = (side * w(i), side * w(i + 1));
                let (ap, aq) = (fade(i), fade(i + 1));
                let (cp, cq) = (lin(core, ap), lin(core, aq));
                let (ep, eq) = (lin(edge, 0.0), lin(edge, 0.0));
                let normal = t.cross(side).normalize_or(Vec3::Y);
                m.poly_colored(&[p - wp, p, q, q - wq], &[ep, cp, cq, eq], normal);
                m.poly_colored(&[p, p + wp, q + wq, q], &[cp, ep, eq, cq], normal);
            }
        }
    };
    bolt(&mut m, &main, 0.0085);
    bolt(&mut m, &fork, 0.0055);
    m.build()
}

/// A sparkle mote that reads from any side: three [`sparkle`] cards crossed
/// on the three axes. Radius 0.5.
pub fn sparkle_cross(tip: Color, core: Color) -> Mesh {
    let mut m = ModelBuilder::new();
    for rot in [
        Affine3A::IDENTITY,
        Affine3A::from_rotation_y(PI / 2.0),
        Affine3A::from_rotation_x(PI / 2.0),
    ] {
        m.with(rot, |m| {
            star_fan(
                m,
                &StarSpec {
                    points: 4,
                    tip: vec![0.5; 4],
                    valley: 0.09,
                    core: lin(Color::WHITE, 1.0),
                    tip_colors: vec![lin(tip, 0.6)],
                    valley_color: lin(core, 0.95),
                    twist: PI / 4.0,
                },
            );
        });
    }
    m.build()
}

// ---------------------------------------------------------------------------
// Solid (toon) shapes
// ---------------------------------------------------------------------------

/// A small mesh builder with smooth normals (for round, puffy shapes).
#[derive(Default)]
struct SmoothBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl SmoothBuilder {
    /// A UV sphere at `center`, radius `r` (squashed by `squash` in y), coloured
    /// from `top` to `bottom` by world height between `y0` and `y1`.
    #[allow(clippy::too_many_arguments)]
    fn sphere(
        &mut self,
        center: Vec3,
        r: f32,
        squash: f32,
        top: Color,
        bottom: Color,
        y0: f32,
        y1: f32,
    ) {
        let (seg, rings) = (12u32, 8u32);
        let start = self.positions.len() as u32;
        for j in 0..=rings {
            let v = PI * j as f32 / rings as f32;
            for i in 0..=seg {
                let u = TAU * i as f32 / seg as f32;
                let n = Vec3::new(v.sin() * u.cos(), v.cos(), v.sin() * u.sin());
                let p = center + Vec3::new(n.x * r, n.y * r * squash, n.z * r);
                let t = ((p.y - y0) / (y1 - y0).max(1e-4)).clamp(0.0, 1.0);
                let normal = Vec3::new(n.x, n.y / squash.max(0.1), n.z).normalize();
                self.positions.push(p.to_array());
                self.normals.push(normal.to_array());
                self.colors.push(lin(mix(bottom, top, t), 1.0));
            }
        }
        let w = seg + 1;
        for j in 0..rings {
            for i in 0..seg {
                let a = start + j * w + i;
                let b = a + w;
                self.indices.extend([a, a + 1, b, b, a + 1, b + 1]);
            }
        }
    }

    fn build(self) -> Mesh {
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

/// Cloud white and its lavender underside (T08).
pub const CLOUD_TOP: Color = Color::srgb(1.0, 0.99, 1.0);
pub const CLOUD_BOTTOM: Color = Color::srgb(0.84, 0.82, 0.98);

/// The elimination poof (T08): a big puffy cartoon cloud of round lobes,
/// about 1.9 m wide and 1.5 m tall at scale 1, centred on the origin.
pub fn poof_cloud(seed: u64) -> Mesh {
    let mut rng = FxRng::new(seed);
    let mut b = SmoothBuilder::default();
    let (y0, y1) = (-0.7, 0.75);
    // A big middle lobe, a crown of lobes around it and a few on top.
    b.sphere(
        Vec3::new(0.0, 0.05, 0.0),
        0.55,
        0.95,
        CLOUD_TOP,
        CLOUD_BOTTOM,
        y0,
        y1,
    );
    let around = 7;
    for i in 0..around {
        let a = TAU * i as f32 / around as f32 + rng.range(-0.2, 0.2);
        let d = rng.range(0.48, 0.62);
        let y = rng.range(-0.28, 0.1);
        let r = rng.range(0.3, 0.4);
        b.sphere(
            Vec3::new(d * a.cos(), y, d * a.sin()),
            r,
            0.9,
            CLOUD_TOP,
            CLOUD_BOTTOM,
            y0,
            y1,
        );
    }
    for i in 0..3 {
        let a = TAU * i as f32 / 3.0 + 0.5;
        b.sphere(
            Vec3::new(0.26 * a.cos(), 0.45 + rng.range(0.0, 0.12), 0.26 * a.sin()),
            rng.range(0.26, 0.33),
            1.0,
            CLOUD_TOP,
            CLOUD_BOTTOM,
            y0,
            y1,
        );
    }
    b.build()
}

/// A small puff (misses, the poof's little clouds): three lobes, radius about
/// 0.5 at scale 1.
pub fn puff(seed: u64) -> Mesh {
    let mut rng = FxRng::new(seed);
    let mut b = SmoothBuilder::default();
    b.sphere(Vec3::ZERO, 0.36, 0.95, CLOUD_TOP, CLOUD_BOTTOM, -0.4, 0.4);
    for _ in 0..2 {
        let a = rng.range(0.0, TAU);
        b.sphere(
            Vec3::new(0.24 * a.cos(), rng.range(-0.1, 0.12), 0.24 * a.sin()),
            rng.range(0.2, 0.26),
            1.0,
            CLOUD_TOP,
            CLOUD_BOTTOM,
            -0.4,
            0.4,
        );
    }
    b.build()
}

/// A chip of brick (piece hits on walls): a chunky bevelled block, 1 × 0.6 ×
/// 0.7 at scale 1, in brick red with a mortar face.
pub fn brick_chip() -> Mesh {
    let mut m = ModelBuilder::new();
    m.chamfer_box(
        Vec3::new(-0.5, -0.3, -0.35),
        Vec3::new(0.5, 0.3, 0.35),
        0.08,
        cartoon::BRICK,
    );
    m.cube(
        Vec3::new(-0.46, 0.3, -0.31),
        Vec3::new(0.46, 0.36, 0.31),
        cartoon::MORTAR,
    );
    m.build()
}

/// A wood splinter (piece hits on floors and ramps): a long thin wedge along
/// z, 1 long at scale 1, in plank brown with a pale split face.
pub fn wood_splinter() -> Mesh {
    let mut m = ModelBuilder::new();
    let zy = [
        Vec2::new(-0.5, -0.1),
        Vec2::new(0.5, -0.06),
        Vec2::new(0.5, 0.02),
        Vec2::new(-0.2, 0.12),
        Vec2::new(-0.5, 0.06),
    ];
    m.prism_x(&zy, -0.14, 0.14, cartoon::PLANK);
    m.prism_x(
        &[
            Vec2::new(-0.45, 0.07),
            Vec2::new(-0.15, 0.125),
            Vec2::new(0.3, 0.05),
        ],
        -0.1,
        0.1,
        shade(cartoon::STUMP_RINGS, 1.1),
    );
    m.build()
}

/// A chunky cartoon star (T07's dizzy stars, T08's poof stars): a five-point
/// star of radius 0.5 in the XY plane, 0.16 thick, gold with darker sides.
pub fn gold_star() -> Mesh {
    let mut m = ModelBuilder::new();
    let (outer, inner, half): (f32, f32, f32) = (0.5, 0.22, 0.08);
    let pts: Vec<Vec2> = (0..10)
        .map(|i| {
            let a = PI / 2.0 + PI * i as f32 / 5.0;
            let r = if i % 2 == 0 { outer } else { inner };
            Vec2::new(r * a.cos(), r * a.sin())
        })
        .collect();
    let face = lin(cartoon::STAR_GOLD, 1.0);
    let side = lin(shade(cartoon::GOLD_RINGS, 1.15), 1.0);
    for z in [half, -half] {
        let normal = Vec3::Z * z.signum();
        for i in 0..10 {
            let j = (i + 1) % 10;
            // Slightly domed: the centre stands proud of the points.
            m.poly_colored(
                &[
                    Vec3::new(0.0, 0.0, z * 1.6),
                    pts[i].extend(z),
                    pts[j].extend(z),
                ],
                &[face, face, face],
                normal,
            );
        }
    }
    for i in 0..10 {
        let j = (i + 1) % 10;
        let (a, b) = (pts[i], pts[j]);
        let out = ((a + b) * 0.5).normalize_or(Vec2::Y).extend(0.0);
        m.poly_colored(
            &[
                a.extend(half),
                b.extend(half),
                b.extend(-half),
                a.extend(-half),
            ],
            &[side; 4],
            out,
        );
    }
    m.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    fn vertex_count(mesh: &Mesh) -> usize {
        mesh.count_vertices()
    }

    #[test]
    fn every_shape_builds_with_the_builder_layout() {
        let meshes = [
            sparkle(BOLT_BLUE, BOLT_CORE),
            starburst(11, 1, BOLT_CORE, &[BOLT_BLUE], 0.15),
            streak(VIOLET_CORE, VIOLET, VIOLET_DEEP),
            ribbon(BOLT_CORE, BOLT_BLUE),
            ring(GLASS),
            swirl(Color::WHITE),
            hex_shell(GLASS_EDGE, GLASS),
            glass_pane(3),
            glass_chip(4),
            rifle_muzzle_burst(),
            pump_muzzle_fan(),
            bolt_head(),
            lightning_arc(7, BOLT_CORE, BOLT_BLUE),
            sparkle_cross(VIOLET, VIOLET_CORE),
            poof_cloud(5),
            puff(6),
            brick_chip(),
            wood_splinter(),
            gold_star(),
        ];
        for mesh in &meshes {
            assert!(vertex_count(mesh) >= 3);
            for attr in [
                Mesh::ATTRIBUTE_POSITION,
                Mesh::ATTRIBUTE_NORMAL,
                Mesh::ATTRIBUTE_COLOR,
            ] {
                assert!(mesh.attribute(attr).is_some(), "{attr:?} missing");
            }
            // Every glow and solid shape shares one vertex layout, so one
            // warm-up draw per material covers them all.
            assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_none());
        }
    }

    #[test]
    fn stars_are_white_hot_in_the_middle_and_coloured_at_the_tips() {
        let mesh = sparkle(BOLT_BLUE, BOLT_CORE);
        let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("no colours");
        };
        let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("no positions");
        };
        let (mut center, mut tip) = (None, None);
        for (p, c) in pos.iter().zip(colors) {
            let r = Vec3::from_array(*p).length();
            if r < 1e-4 {
                center = Some(*c);
            }
            if r > 0.49 {
                tip = Some(*c);
            }
        }
        let (center, tip) = (center.unwrap(), tip.unwrap());
        assert_eq!(center, [1.0, 1.0, 1.0, 1.0]);
        assert!(tip[2] > tip[0] && tip[3] < 1.0, "blue, softer tips {tip:?}");
    }

    #[test]
    fn hex_shell_has_twelve_pentagons_and_hexes() {
        let (verts, faces) = icosphere(1);
        assert_eq!((verts.len(), faces.len()), (42, 80));
        let valence: Vec<usize> = (0..verts.len())
            .map(|i| faces.iter().filter(|f| f.contains(&i)).count())
            .collect();
        assert_eq!(valence.iter().filter(|&&v| v == 5).count(), 12);
        assert_eq!(valence.iter().filter(|&&v| v == 6).count(), 30);
    }
}
