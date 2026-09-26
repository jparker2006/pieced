//! The galaxy sky: a seeded procedural cubemap (spiral arms, a dense star
//! field with a few bright sparkles, purple, teal and deep-blue nebula clouds
//! in the palette's galaxy colors), generated on the CPU at load.
//!
//! **Cost.** The smooth fields (nebulae, galaxy light) are evaluated on a
//! half-resolution grid whose samples sit exactly on the face edges, so the
//! bilinear upsample is seamless across faces; stars are splatted at full
//! resolution onto every face they touch. Faces are generated in parallel.
//!
//! **Directions.** A cubemap texel's lookup direction `L` follows the GPU cube
//! layer order (+X, -X, +Y, -Y, +Z, -Z) with `u` right and `v` down in each face.
//! Bevy's skybox shader samples `L = (s.x, s.y, -s.z)` for a sky-space
//! direction `s = rotation⁻¹ · world`, so [`sky_direction`] maps a texel to the
//! sky direction it shows. At `rotation = IDENTITY` sky space is world space.

use crate::palette::cartoon;
use bevy::{
    asset::RenderAssetUsages,
    image::{Image, ImageSampler},
    prelude::*,
    render::render_resource::{
        Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
    },
};
use std::f32::consts::{PI, TAU};

/// Cubemap face size in texels (1024² × 6 × RGBA8 = 24 MB of VRAM).
pub const GALAXY_FACE: u32 = 1024;
/// The galaxy sky's seed.
pub const GALAXY_SEED: u64 = 0x6A1A_C51C;

/// Unit vector toward a sky point from an azimuth (degrees clockwise from north,
/// -Z, toward east, +X) and an elevation (degrees above the horizon).
pub fn sky_point(azimuth_deg: f32, elevation_deg: f32) -> Vec3 {
    let (az, el) = (azimuth_deg.to_radians(), elevation_deg.to_radians());
    Vec3::new(az.sin() * el.cos(), el.sin(), -az.cos() * el.cos())
}

/// What the generator paints and where.
#[derive(Debug, Clone, PartialEq)]
pub struct GalaxyParams {
    pub face: u32,
    pub seed: u64,
    /// Sky-space direction of the galaxy's core (T01: upper left of the spawn view).
    pub center: Vec3,
    /// Disc radius along the major axis, in tangent-plane units (tan of the angle).
    pub radius: f32,
    /// Apparent minor/major axis ratio (cos of the disc's inclination).
    pub flattening: f32,
    /// Tilt of the major axis, radians counter-clockwise from the view's right.
    pub position_angle: f32,
}

impl Default for GalaxyParams {
    fn default() -> Self {
        Self {
            face: GALAXY_FACE,
            seed: GALAXY_SEED,
            center: sky_point(-21.0, 28.0),
            radius: 0.4,
            flattening: 0.74,
            position_angle: 0.2,
        }
    }
}

// ---------------------------------------------------------------------------
// Cube faces
// ---------------------------------------------------------------------------

/// A cube face's lookup direction at `(u, v)` (each -1..1, `v` down).
pub fn face_direction(face: usize, u: f32, v: f32) -> Vec3 {
    match face {
        0 => Vec3::new(1.0, -v, -u),
        1 => Vec3::new(-1.0, -v, u),
        2 => Vec3::new(u, 1.0, v),
        3 => Vec3::new(u, -1.0, -v),
        4 => Vec3::new(u, -v, 1.0),
        _ => Vec3::new(-u, -v, -1.0),
    }
}

/// Inverse of [`face_direction`]: where lookup direction `l` lands on `face`
/// (possibly outside -1..1), or `None` if it points away from that face.
pub fn face_uv(face: usize, l: Vec3) -> Option<Vec2> {
    let (major, u, v) = match face {
        0 => (l.x, -l.z, -l.y),
        1 => (-l.x, l.z, -l.y),
        2 => (l.y, l.x, l.z),
        3 => (-l.y, l.x, -l.z),
        4 => (l.z, l.x, -l.y),
        _ => (-l.z, -l.x, -l.y),
    };
    (major > 1e-6).then(|| Vec2::new(u, v) / major)
}

/// The sky-space direction a texel shows (see the module docs).
pub fn sky_direction(face: usize, u: f32, v: f32) -> Vec3 {
    let l = face_direction(face, u, v);
    Vec3::new(l.x, l.y, -l.z).normalize()
}

/// Sky direction → the cubemap lookup direction.
fn lookup_of(sky: Vec3) -> Vec3 {
    Vec3::new(sky.x, sky.y, -sky.z)
}

// ---------------------------------------------------------------------------
// Noise
// ---------------------------------------------------------------------------

#[inline]
fn hash3(x: i32, y: i32, z: i32, seed: u32) -> u32 {
    let mut h = seed
        ^ (x as u32).wrapping_mul(0x8DA6_B343)
        ^ (y as u32).wrapping_mul(0xD816_3841)
        ^ (z as u32).wrapping_mul(0xCB1A_B31F);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

#[inline]
fn lattice(x: i32, y: i32, z: i32, seed: u32) -> f32 {
    (hash3(x, y, z, seed) >> 8) as f32 * (2.0 / 16_777_216.0) - 1.0
}

/// Smooth 3D value noise in -1..1.
fn value_noise(p: Vec3, seed: u32) -> f32 {
    let i = p.floor();
    let f = p - i;
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let (x, y, z) = (i.x as i32, i.y as i32, i.z as i32);
    let c = |dx: i32, dy: i32, dz: i32| lattice(x + dx, y + dy, z + dz, seed);
    let x00 = c(0, 0, 0) + (c(1, 0, 0) - c(0, 0, 0)) * u.x;
    let x10 = c(0, 1, 0) + (c(1, 1, 0) - c(0, 1, 0)) * u.x;
    let x01 = c(0, 0, 1) + (c(1, 0, 1) - c(0, 0, 1)) * u.x;
    let x11 = c(0, 1, 1) + (c(1, 1, 1) - c(0, 1, 1)) * u.x;
    let y0 = x00 + (x10 - x00) * u.y;
    let y1 = x01 + (x11 - x01) * u.y;
    y0 + (y1 - y0) * u.z
}

/// Fractal value noise, roughly -1..1.
fn fbm(p: Vec3, seed: u32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut norm, mut q) = (0.0, 1.0, 0.0, p);
    for o in 0..octaves {
        sum += value_noise(q, seed.wrapping_add(o * 0x9E37)) * amp;
        norm += amp;
        amp *= 0.5;
        q = q * 2.03 + Vec3::splat(17.1);
    }
    sum / norm
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A small deterministic generator (SplitMix64) for star placement.
struct Rand(u64);

impl Rand {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / 16_777_216.0
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }

    fn unit_vector(&mut self) -> Vec3 {
        let z = self.range(-1.0, 1.0);
        let a = self.range(0.0, TAU);
        let r = (1.0 - z * z).max(0.0).sqrt();
        Vec3::new(r * a.cos(), r * a.sin(), z)
    }
}

// ---------------------------------------------------------------------------
// The painting
// ---------------------------------------------------------------------------

fn lin(c: Color) -> Vec3 {
    let l = c.to_linear();
    Vec3::new(l.red, l.green, l.blue)
}

fn srgb(r: u8, g: u8, b: u8) -> Vec3 {
    lin(Color::srgb_u8(r, g, b))
}

/// Linear colors used by the painting, from the palette's galaxy colors.
struct Inks {
    space: Vec3,
    deep: Vec3,
    purple: Vec3,
    teal: Vec3,
    pink: Vec3,
    violet: Vec3,
    core: Vec3,
    star_blue: Vec3,
    star_gold: Vec3,
}

impl Inks {
    fn new() -> Self {
        Self {
            space: srgb(0x20, 0x2D, 0x76),
            deep: lin(cartoon::GALAXY_DEEP),
            purple: lin(cartoon::GALAXY_PURPLE),
            teal: lin(cartoon::GALAXY_TEAL),
            pink: lin(cartoon::GLASS_PINK),
            violet: lin(cartoon::GLASS_VIOLET),
            core: srgb(0xFF, 0xF6, 0xEE),
            star_blue: srgb(0xCF, 0xE4, 0xFF),
            star_gold: lin(cartoon::STAR_GOLD),
        }
    }
}

/// The galaxy disc's frame: sky direction ↔ disc coordinates.
#[derive(Debug, Clone, Copy)]
struct Disc {
    center: Vec3,
    right: Vec3,
    up: Vec3,
    radius: f32,
    flattening: f32,
    cos_pa: f32,
    sin_pa: f32,
}

impl Disc {
    fn new(p: &GalaxyParams) -> Self {
        let center = p.center.normalize();
        let right = center.cross(Vec3::Y).normalize();
        let up = right.cross(center);
        Self {
            center,
            right,
            up,
            radius: p.radius,
            flattening: p.flattening,
            cos_pa: p.position_angle.cos(),
            sin_pa: p.position_angle.sin(),
        }
    }

    /// Disc coordinates (1 = the disc's edge) of a sky direction, if it is in
    /// front of the galaxy.
    fn coords(&self, s: Vec3) -> Option<Vec2> {
        let d = s.dot(self.center);
        if d < 0.2 {
            return None;
        }
        let t = Vec2::new(s.dot(self.right), s.dot(self.up)) / d;
        let r = Vec2::new(
            t.x * self.cos_pa + t.y * self.sin_pa,
            -t.x * self.sin_pa + t.y * self.cos_pa,
        );
        Some(Vec2::new(r.x, r.y / self.flattening) / self.radius)
    }

    /// Inverse of [`Disc::coords`].
    fn direction(&self, q: Vec2) -> Vec3 {
        let r = Vec2::new(q.x, q.y * self.flattening) * self.radius;
        let t = Vec2::new(
            r.x * self.cos_pa - r.y * self.sin_pa,
            r.x * self.sin_pa + r.y * self.cos_pa,
        );
        (self.center + self.right * t.x + self.up * t.y).normalize()
    }
}

/// Arm pitch: how tightly the two arms wind.
const ARM_WIND: f32 = 3.7;

/// Spiral-arm light at disc coordinates `q`: (arm strength 0..1, which arm is
/// nearest, radius, signed angular distance to that arm's spine).
fn arms(q: Vec2, warp: f32) -> (f32, usize, f32, f32) {
    let r = q.length();
    let theta = q.y.atan2(q.x) + warp;
    let mut best = (0.0f32, 0usize, 0.0f32);
    for arm in 0..2 {
        let phase = 0.6 + arm as f32 * PI + r.max(0.03).ln() * ARM_WIND;
        let d = (theta - phase + PI).rem_euclid(TAU) - PI;
        let width = 0.3 + 0.24 * r;
        let a = (-(d * d) / (2.0 * width * width)).exp();
        if a > best.0 {
            best = (a, arm, d);
        }
    }
    (best.0, best.1, r, best.2)
}

/// The nebula clouds in one sky direction, linear rgb.
fn nebula(s: Vec3, inks: &Inks, seed: u32) -> Vec3 {
    // Nebula clouds, swirled by a slow warp: deep blue everywhere, purple
    // banks, teal wisps, darker lanes between.
    let swirl = fbm(s * 1.6 + Vec3::splat(7.0), seed ^ 0x3C, 3);
    let w = s * 2.3 + Vec3::new(swirl, -swirl, swirl * 0.5) * 0.9;
    let n1 = fbm(w, seed, 5);
    let n2 = fbm(w * 1.35 + Vec3::splat(40.0), seed ^ 0x51, 5);
    let n3 = fbm(w * 2.4 + Vec3::splat(-23.0), seed ^ 0xA7, 4);
    let mut c = inks.space;
    c = c.lerp(inks.deep * 0.74, smoothstep(-0.3, 0.45, n1) * 0.85);
    c = c.lerp(inks.purple * 0.86, smoothstep(0.02, 0.55, n2) * 0.62);
    let wisp = smoothstep(0.18, 0.55, n3) * smoothstep(-0.15, 0.3, n1);
    c += inks.teal * wisp * 0.12;
    c * (0.74 + 0.26 * smoothstep(-0.5, 0.1, n1.max(n2)))
}

/// The galaxy's own light in one sky direction (zero away from it), linear rgb.
fn galaxy_light(s: Vec3, disc: &Disc, inks: &Inks, seed: u32) -> Vec3 {
    let Some(q) = disc.coords(s) else {
        return Vec3::ZERO;
    };
    // A wide purple glow; the arms, bulge and core only near the disc.
    let r0 = q.length();
    let glow = (-(r0 * r0) / 0.45).exp();
    let mut c = inks.purple * glow * 0.22;
    if r0 > 1.45 {
        return c;
    }
    let warp = 0.45 * fbm(s * 11.0, seed ^ 0x1234, 3);
    let (arm, which, r, d) = arms(q, warp);
    // Filaments running along the arms, and clumps of star-forming knots.
    let filaments = 0.62 + 0.38 * (d * 11.0 + 3.0 * fbm(s * 20.0, seed ^ 0x55, 2)).cos();
    let clump = 0.6 + 0.55 * fbm(s * 38.0, seed ^ 0x77, 3).max(-0.4);
    let envelope = smoothstep(1.25, 0.3, r) * smoothstep(0.02, 0.16, r);
    let light = arm * envelope * clump * filaments;
    // Arm colours: pink-violet inside; one arm turns teal outside, the other
    // purple (T01).
    let tint = smoothstep(0.25, 0.85, r + 0.2 * fbm(s * 7.0, seed ^ 0x99, 2));
    let outer = if which == 0 { inks.teal } else { inks.violet };
    let arm_color = inks.pink.lerp(inks.violet, 0.5).lerp(outer, tint);
    let bulge = (-(r * r) / 0.02).exp();
    let core = (-(r * r) / 0.0018).exp();
    c += arm_color * light * 1.45;
    c += inks.pink.lerp(inks.core, 0.4) * bulge * 0.75;
    c += inks.core * core * 1.8;
    c
}

/// How strongly stars crowd into the galaxy at `s` (0..1).
fn star_crowding(s: Vec3, disc: &Disc) -> f32 {
    disc.coords(s).map_or(0.0, |q| {
        let (arm, _, r, _) = arms(q, 0.0);
        (arm * smoothstep(1.2, 0.2, r) + (-(r * r) / 0.05).exp()).min(1.0)
    })
}

/// One star to splat: a gaussian dot, optionally with a four-point sparkle.
#[derive(Debug, Clone, Copy)]
struct Star {
    sky: Vec3,
    color: Vec3,
    sigma: f32,
    flare: f32,
}

fn stars(params: &GalaxyParams, disc: &Disc, inks: &Inks) -> Vec<Star> {
    let mut rand = Rand(params.seed ^ 0xC0FF_EE00_5747_5253);
    let scale = (params.face as f32 / 1024.0).powi(2);
    let mut out = Vec::new();
    // The field: many faint stars, a few bright ones.
    let faint = (18_000.0 * scale) as usize;
    for _ in 0..faint {
        let sky = rand.unit_vector();
        let m = rand.f32();
        let brightness = 0.12 + 0.9 * m * m * m;
        let color = inks
            .star_blue
            .lerp(Vec3::ONE, rand.f32())
            .lerp(inks.star_gold, (rand.f32() - 0.85).max(0.0) * 4.0);
        out.push(Star {
            sky,
            color: color * brightness,
            sigma: rand.range(0.55, 0.85),
            flare: 0.0,
        });
    }
    let bright = (160.0 * scale.sqrt()) as usize;
    for i in 0..bright {
        let sky = rand.unit_vector();
        let color = if rand.f32() < 0.25 {
            inks.star_gold
        } else {
            inks.star_blue.lerp(Vec3::ONE, 0.6)
        };
        out.push(Star {
            sky,
            color: color * rand.range(0.9, 1.8),
            sigma: rand.range(0.9, 1.5),
            flare: if i % 4 == 0 {
                rand.range(6.0, 13.0)
            } else {
                0.0
            },
        });
    }
    // Star dust crowding the spiral arms.
    let dust = (34_000.0 * scale) as usize;
    let mut placed = 0;
    let mut tries = 0;
    while placed < dust && tries < dust * 12 {
        tries += 1;
        let q = Vec2::new(rand.range(-1.2, 1.2), rand.range(-1.2, 1.2));
        let sky = disc.direction(q);
        if rand.f32() > star_crowding(sky, disc) {
            continue;
        }
        placed += 1;
        let r = q.length();
        let (_, which, _, _) = arms(q, 0.0);
        let tint = if which == 0 { inks.teal } else { inks.pink };
        let color = inks.core.lerp(tint, (0.35 + r).min(1.0) * 0.85);
        let m = rand.f32();
        out.push(Star {
            sky,
            color: color * (0.12 + 0.55 * m * m),
            sigma: rand.range(0.5, 0.75),
            flare: 0.0,
        });
    }
    out
}

/// Linear → sRGB bytes through a table (the conversion is the hot loop's tail).
struct SrgbTable(Vec<u8>);

impl SrgbTable {
    const STEPS: usize = 4096;

    fn new() -> Self {
        Self(
            (0..=Self::STEPS)
                .map(|i| {
                    let c = i as f32 / Self::STEPS as f32;
                    let s = if c <= 0.003_130_8 {
                        c * 12.92
                    } else {
                        1.055 * c.powf(1.0 / 2.4) - 0.055
                    };
                    (s * 255.0).round().clamp(0.0, 255.0) as u8
                })
                .collect(),
        )
    }

    fn get(&self, c: f32) -> u8 {
        self.0[(c.clamp(0.0, 1.0) * Self::STEPS as f32) as usize]
    }
}

/// Paints one face into `out` (size² RGBA8, sRGB).
fn paint_face(
    face: usize,
    params: &GalaxyParams,
    disc: &Disc,
    inks: &Inks,
    stars: &[Star],
    table: &SrgbTable,
    out: &mut [u8],
) {
    let n = params.face as usize;
    let seed = params.seed as u32 ^ (params.seed >> 32) as u32;
    // The soft nebulae on a quarter-resolution grid, the galaxy's finer light on
    // a half-resolution one (only where it is); both grids' outer samples lie on
    // the face edges, so neighbouring faces meet without a seam.
    let nebula_grid = Grid::sample(face, (n / 4).max(1), |s| nebula(s, inks, seed));
    let galaxy_grid = Grid::sample(face, (n / 2).max(1), |s| galaxy_light(s, disc, inks, seed));
    let mut px = vec![Vec3::ZERO; n * n];
    for y in 0..n {
        let fy = (y as f32 + 0.5) / n as f32;
        for x in 0..n {
            let fx = (x as f32 + 0.5) / n as f32;
            px[y * n + x] = nebula_grid.at(fx, fy) + galaxy_grid.at(fx, fy);
        }
    }
    // Stars: every star whose splat reaches this face (with a margin, so stars on
    // an edge appear on both faces).
    let half = n as f32 / 2.0;
    for star in stars {
        let Some(uv) = face_uv(face, lookup_of(star.sky)) else {
            continue;
        };
        let reach = (3.0 * star.sigma).max(star.flare) + 1.0;
        let cx = (uv.x + 1.0) * half - 0.5;
        let cy = (uv.y + 1.0) * half - 0.5;
        if cx < -reach || cy < -reach || cx > n as f32 + reach || cy > n as f32 + reach {
            continue;
        }
        splat(&mut px, n, cx, cy, star);
    }
    for (i, c) in px.iter().enumerate() {
        out[i * 4] = table.get(c.x);
        out[i * 4 + 1] = table.get(c.y);
        out[i * 4 + 2] = table.get(c.z);
        out[i * 4 + 3] = 255;
    }
}

/// Field samples over one face on an (m + 1)² grid, corners on the face edges.
struct Grid {
    m: usize,
    samples: Vec<Vec3>,
}

impl Grid {
    fn sample(face: usize, m: usize, f: impl Fn(Vec3) -> Vec3) -> Self {
        let mut samples = Vec::with_capacity((m + 1) * (m + 1));
        for gy in 0..=m {
            for gx in 0..=m {
                let u = -1.0 + 2.0 * gx as f32 / m as f32;
                let v = -1.0 + 2.0 * gy as f32 / m as f32;
                samples.push(f(sky_direction(face, u, v)));
            }
        }
        Self { m, samples }
    }

    /// Bilinear value at face fraction (fx, fy), each 0..1.
    fn at(&self, fx: f32, fy: f32) -> Vec3 {
        let m = self.m;
        let (gxf, gyf) = (fx * m as f32, fy * m as f32);
        let gx = (gxf as usize).min(m - 1);
        let gy = (gyf as usize).min(m - 1);
        let (tx, ty) = (gxf - gx as f32, gyf - gy as f32);
        let at = |i: usize, j: usize| self.samples[j * (m + 1) + i];
        let top = at(gx, gy).lerp(at(gx + 1, gy), tx);
        let bottom = at(gx, gy + 1).lerp(at(gx + 1, gy + 1), tx);
        top.lerp(bottom, ty)
    }
}

fn splat(px: &mut [Vec3], n: usize, cx: f32, cy: f32, star: &Star) {
    let r = (3.0 * star.sigma).ceil() as i32;
    let inv = 1.0 / (2.0 * star.sigma * star.sigma);
    let (ix, iy) = (cx.round() as i32, cy.round() as i32);
    let mut add = |x: i32, y: i32, w: f32| {
        if x >= 0 && y >= 0 && (x as usize) < n && (y as usize) < n {
            px[y as usize * n + x as usize] += star.color * w;
        }
    };
    for y in iy - r..=iy + r {
        for x in ix - r..=ix + r {
            let (dx, dy) = (x as f32 - cx, y as f32 - cy);
            add(x, y, (-(dx * dx + dy * dy) * inv).exp());
        }
    }
    if star.flare > 0.0 {
        // A thin four-point sparkle along the face axes.
        let len = star.flare.ceil() as i32;
        for k in -len..=len {
            if k == 0 {
                continue;
            }
            let fall = (1.0 - k.unsigned_abs() as f32 / star.flare).max(0.0);
            let w = 0.55 * fall * fall;
            add(ix + k, iy, w);
            add(ix, iy + k, w);
        }
    }
}

/// Generates the six faces (+X, -X, +Y, -Y, +Z, -Z), each `face`² RGBA8 sRGB,
/// back to back. Deterministic for a given `params`; faces run in parallel.
pub fn generate_galaxy(params: &GalaxyParams) -> Vec<u8> {
    let n = params.face as usize;
    let disc = Disc::new(params);
    let inks = Inks::new();
    let table = SrgbTable::new();
    let stars = stars(params, &disc, &inks);
    let mut data = vec![0u8; n * n * 4 * 6];
    std::thread::scope(|scope| {
        for (face, out) in data.chunks_mut(n * n * 4).enumerate() {
            let (params, disc, inks, stars, table) = (params, &disc, &inks, &stars, &table);
            scope.spawn(move || paint_face(face, params, disc, inks, stars, table, out));
        }
    });
    data
}

/// Wraps generated faces as a cube texture for `Skybox` (render world only: the
/// CPU copy is dropped after upload).
pub fn galaxy_image(data: Vec<u8>, face: u32) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: face,
            height: face,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image.sampler = ImageSampler::linear();
    image
}

/// Samples generated faces (nearest texel) in a sky direction, as sRGB bytes.
/// For tests and review renders.
pub fn sample_galaxy(data: &[u8], face: u32, sky: Vec3) -> [u8; 3] {
    let l = lookup_of(sky.normalize());
    let a = l.abs();
    let f = if a.x >= a.y && a.x >= a.z {
        if l.x > 0.0 { 0 } else { 1 }
    } else if a.y >= a.z {
        if l.y > 0.0 { 2 } else { 3 }
    } else if l.z > 0.0 {
        4
    } else {
        5
    };
    let uv = face_uv(f, l).unwrap_or(Vec2::ZERO);
    let n = face as usize;
    let x = (((uv.x + 1.0) * 0.5 * n as f32) as usize).min(n - 1);
    let y = (((uv.y + 1.0) * 0.5 * n as f32) as usize).min(n - 1);
    let i = (f * n * n + y * n + x) * 4;
    [data[i], data[i + 1], data[i + 2]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_round_trip_and_cover_the_sphere() {
        for face in 0..6 {
            for (u, v) in [(0.0, 0.0), (0.5, -0.25), (-0.9, 0.8)] {
                let l = face_direction(face, u, v);
                let back = face_uv(face, l).unwrap();
                assert!((back - Vec2::new(u, v)).length() < 1e-5, "face {face}");
                // Only its own face sees it head on.
                for other in 0..6 {
                    if other != face
                        && let Some(o) = face_uv(other, l)
                    {
                        assert!(o.x.abs() > 1.0 - 1e-5 || o.y.abs() > 1.0 - 1e-5);
                    }
                }
            }
        }
        // Face centres are the six axes (GPU layer order +X, -X, +Y, -Y, +Z, -Z).
        let axes = [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z];
        for (face, axis) in axes.iter().enumerate() {
            assert_eq!(face_direction(face, 0.0, 0.0), *axis);
        }
        // Shared edges: +X's left edge is +Z's right edge, both upright.
        assert_eq!(face_direction(0, -1.0, 0.3), face_direction(4, 1.0, 0.3));
        // `v` runs down: the top row of a side face looks up.
        assert!(face_direction(4, 0.0, -1.0).y > 0.0);
    }

    #[test]
    fn disc_coordinates_invert() {
        let disc = Disc::new(&GalaxyParams::default());
        for q in [Vec2::ZERO, Vec2::new(0.5, 0.2), Vec2::new(-0.8, 0.6)] {
            let s = disc.direction(q);
            let back = disc.coords(s).unwrap();
            assert!((back - q).length() < 1e-4, "{q} -> {back}");
        }
        assert!(disc.coords(-disc.center).is_none());
    }

    #[test]
    fn the_core_is_the_brightest_thing_in_its_neighbourhood() {
        let params = GalaxyParams {
            face: 96,
            ..default()
        };
        let data = generate_galaxy(&params);
        let luma = |c: [u8; 3]| c.iter().map(|&v| v as u32).sum::<u32>();
        let core = luma(sample_galaxy(&data, params.face, params.center));
        let away = luma(sample_galaxy(&data, params.face, -params.center));
        assert!(core > 600, "core {core}");
        assert!(core > away + 250, "core {core} vs far sky {away}");
    }
}
