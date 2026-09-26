//! A tiny flat-shaded mesh builder: a triangle soup where every face carries its
//! own normal and vertex colors, so low-poly shapes read as crisp facets with a
//! single shared material. Also holds the deterministic noise the scenery uses.

use bevy::{asset::RenderAssetUsages, mesh::PrimitiveTopology, prelude::*};

/// Linear RGBA used for vertex colors.
pub type Rgba = [f32; 4];

/// Palette color → linear vertex color.
pub fn lin(color: Color) -> Rgba {
    let l = color.to_linear();
    [l.red, l.green, l.blue, l.alpha]
}

/// Scales a color's brightness (linear space), keeping alpha.
pub fn shade(c: Rgba, k: f32) -> Rgba {
    [c[0] * k, c[1] * k, c[2] * k, c[3]]
}

pub fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

/// Triangle soup with per-vertex normals and colors.
#[derive(Debug, Default, Clone)]
pub struct Geo {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<Rgba>,
}

impl Geo {
    pub fn tri_count(&self) -> usize {
        self.positions.len() / 3
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    /// A flat triangle, counter-clockwise when seen from its front.
    /// Degenerate (zero-area) triangles are skipped.
    pub fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3, color: Rgba) {
        let n = (b - a).cross(c - a);
        if n.length_squared() < 1e-12 {
            return;
        }
        let n = n.normalize();
        self.tri_raw([a, b, c], [n, n, n], [color, color, color]);
    }

    /// A triangle with explicit normals and colors (grass blades use this).
    pub fn tri_raw(&mut self, p: [Vec3; 3], n: [Vec3; 3], c: [Rgba; 3]) {
        for i in 0..3 {
            self.positions.push(p[i].to_array());
            self.normals.push(n[i].to_array());
            self.colors.push(c[i]);
        }
    }

    /// A quad `a b c d`, counter-clockwise from its front, split along `a–c`.
    pub fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, color: Rgba) {
        self.tri(a, b, c, color);
        self.tri(a, c, d, color);
    }

    /// Appends another soup transformed by `t` (normals rotated, not scaled).
    pub fn append(&mut self, other: &Geo, t: &Transform) {
        let m = t.to_matrix();
        let normal = t.to_matrix().inverse().transpose();
        self.positions.extend(
            other
                .positions
                .iter()
                .map(|p| m.transform_point3(Vec3::from_array(*p)).to_array()),
        );
        self.normals.extend(other.normals.iter().map(|n| {
            normal
                .transform_vector3(Vec3::from_array(*n))
                .normalize_or_zero()
                .to_array()
        }));
        self.colors.extend_from_slice(&other.colors);
    }

    pub fn vertices(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.positions.iter().map(|p| Vec3::from_array(*p))
    }

    pub fn into_mesh(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
    }

    /// Connects consecutive closed rings of equal length with quads (a lathe/loft).
    /// Rings go bottom to top and wind counter-clockwise seen from above, so faces
    /// point outward. `color(band, side)` colors each quad. A ring may collapse to a
    /// point (all vertices equal) to make a cone tip.
    pub fn loft(&mut self, rings: &[Vec<Vec3>], mut color: impl FnMut(usize, usize) -> Rgba) {
        for band in 0..rings.len().saturating_sub(1) {
            let (lo, hi) = (&rings[band], &rings[band + 1]);
            let n = lo.len().min(hi.len());
            for i in 0..n {
                let j = (i + 1) % n;
                self.quad(lo[i], lo[j], hi[j], hi[i], color(band, i));
            }
        }
    }

    /// Fills a convex ring as a fan. `up` = true faces +Y (top cap).
    pub fn cap(&mut self, ring: &[Vec3], up: bool, color: Rgba) {
        if ring.len() < 3 {
            return;
        }
        let center = ring.iter().copied().sum::<Vec3>() / ring.len() as f32;
        for i in 0..ring.len() {
            let j = (i + 1) % ring.len();
            if up {
                self.tri(center, ring[i], ring[j], color);
            } else {
                self.tri(center, ring[j], ring[i], color);
            }
        }
    }
}

/// A horizontal ring of `sides` points (counter-clockwise from above, first point
/// toward -Z rotated by `phase`), at height `y`, with per-point radius from `radius`.
pub fn ring(
    center: Vec3,
    sides: usize,
    phase: f32,
    y: f32,
    mut radius: impl FnMut(usize) -> f32,
) -> Vec<Vec3> {
    (0..sides)
        .map(|i| {
            // Counter-clockwise seen from +Y means decreasing angle in XZ with -Z first.
            let a = phase - i as f32 * std::f32::consts::TAU / sides as f32;
            let r = radius(i);
            center + Vec3::new(a.sin() * r, y, -a.cos() * r)
        })
        .collect()
}

/// A closed icosphere as shared vertices plus triangle indices (outward CCW).
pub fn icosphere(subdivisions: u32) -> (Vec<Vec3>, Vec<[usize; 3]>) {
    let t = (1.0 + 5f32.sqrt()) / 2.0;
    let mut v: Vec<Vec3> = [
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
    let mut f: Vec<[usize; 3]> = vec![
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
        let mut cache = std::collections::HashMap::new();
        let mut mid = |a: usize, b: usize, v: &mut Vec<Vec3>| {
            let key = (a.min(b), a.max(b));
            *cache.entry(key).or_insert_with(|| {
                v.push(((v[a] + v[b]) * 0.5).normalize());
                v.len() - 1
            })
        };
        let mut next = Vec::with_capacity(f.len() * 4);
        for [a, b, c] in f {
            let ab = mid(a, b, &mut v);
            let bc = mid(b, c, &mut v);
            let ca = mid(c, a, &mut v);
            next.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
        }
        f = next;
    }
    (v, f)
}

/// Emits an icosphere-based blob: unit sphere vertices mapped through `shape`
/// (which may jitter and scale them), faces colored by `color(face_normal)`.
pub fn blob(
    geo: &mut Geo,
    subdivisions: u32,
    mut shape: impl FnMut(Vec3) -> Vec3,
    mut color: impl FnMut(Vec3) -> Rgba,
) {
    let (verts, faces) = icosphere(subdivisions);
    let verts: Vec<Vec3> = verts.into_iter().map(&mut shape).collect();
    for [a, b, c] in faces {
        let (a, b, c) = (verts[a], verts[b], verts[c]);
        let n = (b - a).cross(c - a).normalize_or_zero();
        geo.tri(a, b, c, color(n));
    }
}

// ---------------------------------------------------------------------------
// Deterministic noise
// ---------------------------------------------------------------------------

fn hash2(x: i32, z: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
        ^ (z as u32).wrapping_mul(0xD816_3841)
        ^ seed.wrapping_mul(0xCB1A_B31F);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5BD1_E995);
    h ^= h >> 15;
    (h & 0x00FF_FFFF) as f32 / 0x0100_0000 as f32
}

/// Smooth value noise in [0, 1).
pub fn noise2(x: f32, z: f32, seed: u32) -> f32 {
    let (xi, zi) = (x.floor(), z.floor());
    let (fx, fz) = (x - xi, z - zi);
    let (xi, zi) = (xi as i32, zi as i32);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (u, v) = (s(fx), s(fz));
    let a = hash2(xi, zi, seed);
    let b = hash2(xi + 1, zi, seed);
    let c = hash2(xi, zi + 1, seed);
    let d = hash2(xi + 1, zi + 1, seed);
    let ab = a + (b - a) * u;
    let cd = c + (d - c) * u;
    ab + (cd - ab) * v
}

/// Three octaves of value noise, normalized to [0, 1).
pub fn fbm2(x: f32, z: f32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut freq = 1.0;
    let mut norm = 0.0;
    for octave in 0..3 {
        sum += amp * noise2(x * freq, z * freq, seed.wrapping_add(octave * 101));
        norm += amp;
        amp *= 0.5;
        freq *= 2.03;
    }
    sum / norm
}

pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_point_outward_and_are_flat() {
        let mut g = Geo::default();
        blob(&mut g, 1, |v| v * 2.0, |_| [1.0; 4]);
        assert_eq!(g.tri_count(), 80);
        for (i, n) in g.normals.chunks(3).enumerate() {
            assert_eq!(n[0], n[1]);
            assert_eq!(n[1], n[2]);
            let centroid = (0..3)
                .map(|k| Vec3::from_array(g.positions[i * 3 + k]))
                .sum::<Vec3>()
                / 3.0;
            assert!(
                centroid.dot(Vec3::from_array(n[0])) > 0.0,
                "face {i} inward"
            );
        }
    }

    #[test]
    fn loft_rings_face_outward() {
        let mut g = Geo::default();
        let rings = vec![
            ring(Vec3::ZERO, 6, 0.0, 0.0, |_| 1.0),
            ring(Vec3::ZERO, 6, 0.0, 1.0, |_| 1.0),
        ];
        g.loft(&rings, |_, _| [1.0; 4]);
        g.cap(&rings[1], true, [1.0; 4]);
        for (i, n) in g.normals.chunks(3).enumerate() {
            let p = Vec3::from_array(g.positions[i * 3]);
            let n = Vec3::from_array(n[0]);
            if n.y.abs() > 0.9 {
                assert!(n.y > 0.0, "top cap faces up");
            } else {
                assert!(Vec3::new(p.x, 0.0, p.z).dot(n) > 0.0, "side {i} faces out");
            }
        }
    }

    #[test]
    fn noise_is_deterministic_and_bounded() {
        for i in 0..200 {
            let (x, z) = (i as f32 * 0.37, i as f32 * -0.61);
            let a = fbm2(x, z, 9);
            assert_eq!(a, fbm2(x, z, 9));
            assert!((0.0..1.0).contains(&a));
        }
    }
}
