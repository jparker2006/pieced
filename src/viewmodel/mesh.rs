//! A tiny flat-shaded, vertex-colored mesh builder for the gun models and the
//! effect meshes. Every polygon gets its own face normal, so lit meshes read as
//! clean facets. Meshes are built once at startup, never per frame.

use bevy::{
    asset::RenderAssetUsages,
    math::Affine3A,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

/// Accumulates flat-shaded polygons with per-vertex colors.
#[derive(Default, Clone)]
pub struct ModelBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
    xf: Affine3A,
}

/// Linear RGBA of a color with an alpha override.
pub fn linear(color: Color, alpha: f32) -> [f32; 4] {
    let c = color.to_linear();
    [c.red, c.green, c.blue, alpha]
}

/// Scales a color's sRGB channels (k > 1 lightens).
pub fn shade(color: Color, k: f32) -> Color {
    let c = color.to_srgba();
    Color::srgba(
        (c.red * k).clamp(0.0, 1.0),
        (c.green * k).clamp(0.0, 1.0),
        (c.blue * k).clamp(0.0, 1.0),
        c.alpha,
    )
}

impl ModelBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn vertices(&self) -> usize {
        self.positions.len()
    }

    /// Axis-aligned bounds (min, max) of everything added so far.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for p in &self.positions {
            let p = Vec3::from_array(*p);
            min = min.min(p);
            max = max.max(p);
        }
        (min, max)
    }

    /// Runs `f` with `xf` applied on top of the current placement.
    pub fn with(&mut self, xf: Affine3A, f: impl FnOnce(&mut Self)) {
        let saved = self.xf;
        self.xf = saved * xf;
        f(self);
        self.xf = saved;
    }

    /// A convex planar polygon, wound so its normal points away from `inside`.
    pub fn poly(&mut self, points: &[Vec3], inside: Vec3, color: [f32; 4]) {
        if points.len() < 3 {
            return;
        }
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
        let colors = vec![color; points.len()];
        self.push_polygon(points, &colors, if flip { -n } else { n }, flip);
    }

    /// A polygon with one color per vertex and an explicit facing normal
    /// (vertices are reordered so the winding matches `normal`).
    pub fn poly_colored(&mut self, points: &[Vec3], colors: &[[f32; 4]], normal: Vec3) {
        if points.len() < 3 || colors.len() != points.len() {
            return;
        }
        let mut n = Vec3::ZERO;
        for (i, a) in points.iter().enumerate() {
            let b = points[(i + 1) % points.len()];
            n += Vec3::new(
                (a.y - b.y) * (a.z + b.z),
                (a.z - b.z) * (a.x + b.x),
                (a.x - b.x) * (a.y + b.y),
            );
        }
        let flip = n.dot(normal) < 0.0;
        self.push_polygon(points, colors, normal, flip);
    }

    fn push_polygon(&mut self, points: &[Vec3], colors: &[[f32; 4]], n: Vec3, flip: bool) {
        let normal = self.xf.transform_vector3(n).normalize_or_zero();
        let start = self.positions.len() as u32;
        let count = points.len();
        for k in 0..count {
            let i = if flip { count - 1 - k } else { k };
            self.positions
                .push(self.xf.transform_point3(points[i]).to_array());
            self.normals.push(normal.to_array());
            self.colors.push(colors[i]);
        }
        for k in 1..count as u32 - 1 {
            self.indices.extend([start, start + k, start + k + 1]);
        }
    }

    /// A triangle fan around `center` (for star shapes), facing `normal`.
    pub fn fan(&mut self, center: Vec3, ring: &[Vec3], center_color: [f32; 4], rim: [f32; 4]) {
        let normal = {
            let a = ring[0] - center;
            let b = ring[1] - center;
            a.cross(b)
        };
        for i in 0..ring.len() {
            let j = (i + 1) % ring.len();
            self.poly_colored(
                &[center, ring[i], ring[j]],
                &[center_color, rim, rim],
                normal,
            );
        }
    }

    /// An axis-aligned box with chamfered edges (`bevel` = 0 for a plain box).
    /// Bevel faces are a touch lighter, like worn edges catching the light.
    pub fn chamfer_box(&mut self, min: Vec3, max: Vec3, bevel: f32, color: Color) {
        let c = (min + max) / 2.0;
        let h = (max - min) / 2.0;
        let b = bevel.min(h.min_element() * 0.45).max(0.0);
        let face = linear(color, 1.0);
        let edge = linear(shade(color, 1.18), 1.0);
        let at = |a: usize, va: f32, u: usize, vu: f32, v: usize, vv: f32| {
            let mut p = Vec3::ZERO;
            p[a] = va;
            p[u] = vu;
            p[v] = vv;
            c + p
        };
        for a in 0..3 {
            let (u, v) = ((a + 1) % 3, (a + 2) % 3);
            for s in [-1.0, 1.0] {
                let pts = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                    .map(|(su, sv)| at(a, s * h[a], u, su * (h[u] - b), v, sv * (h[v] - b)));
                self.poly(&pts, c, face);
            }
        }
        if b <= 0.0 {
            return;
        }
        for (a, u) in [(0, 1), (0, 2), (1, 2)] {
            let v = 3 - a - u;
            for sa in [-1.0, 1.0] {
                for su in [-1.0, 1.0] {
                    let pts = [
                        at(a, sa * h[a], u, su * (h[u] - b), v, -(h[v] - b)),
                        at(a, sa * h[a], u, su * (h[u] - b), v, h[v] - b),
                        at(a, sa * (h[a] - b), u, su * h[u], v, h[v] - b),
                        at(a, sa * (h[a] - b), u, su * h[u], v, -(h[v] - b)),
                    ];
                    self.poly(&pts, c, edge);
                }
            }
        }
        for sx in [-1.0, 1.0] {
            for sy in [-1.0, 1.0] {
                for sz in [-1.0, 1.0] {
                    let s = Vec3::new(sx, sy, sz);
                    let pts = [
                        c + s * Vec3::new(h.x, h.y - b, h.z - b),
                        c + s * Vec3::new(h.x - b, h.y, h.z - b),
                        c + s * Vec3::new(h.x - b, h.y - b, h.z),
                    ];
                    self.poly(&pts, c, edge);
                }
            }
        }
    }

    /// A plain box (no bevel).
    pub fn cube(&mut self, min: Vec3, max: Vec3, color: Color) {
        self.chamfer_box(min, max, 0.0, color);
    }

    /// A convex profile in the (z, y) plane extruded along x from `x0` to `x1`.
    pub fn prism_x(&mut self, zy: &[Vec2], x0: f32, x1: f32, color: Color) {
        let col = linear(color, 1.0);
        let centroid2 = zy.iter().copied().sum::<Vec2>() / zy.len() as f32;
        let inside = Vec3::new((x0 + x1) / 2.0, centroid2.y, centroid2.x);
        let at = |x: f32, p: Vec2| Vec3::new(x, p.y, p.x);
        let near: Vec<Vec3> = zy.iter().map(|&p| at(x0, p)).collect();
        let far: Vec<Vec3> = zy.iter().map(|&p| at(x1, p)).collect();
        self.poly(&near, inside, col);
        self.poly(&far, inside, col);
        for i in 0..zy.len() {
            let j = (i + 1) % zy.len();
            self.poly(&[near[i], near[j], far[j], far[i]], inside, col);
        }
    }

    /// A regular `sides`-gon prism along z (barrels, tubes, shells), centered at
    /// `(center.x, center.y)`, from `z0` to `z1`.
    pub fn tube_z(
        &mut self,
        center: Vec2,
        radius: f32,
        z0: f32,
        z1: f32,
        sides: usize,
        color: Color,
    ) {
        let col = linear(color, 1.0);
        let sides = sides.max(3);
        // Rotate half a step so an octagon has flat top and sides.
        let offset = std::f32::consts::PI / sides as f32;
        let ring = |z: f32| -> Vec<Vec3> {
            (0..sides)
                .map(|i| {
                    let a = offset + std::f32::consts::TAU * i as f32 / sides as f32;
                    Vec3::new(center.x + radius * a.cos(), center.y + radius * a.sin(), z)
                })
                .collect()
        };
        let inside = Vec3::new(center.x, center.y, (z0 + z1) / 2.0);
        let a = ring(z0);
        let b = ring(z1);
        self.poly(&a, inside, col);
        self.poly(&b, inside, col);
        for i in 0..sides {
            let j = (i + 1) % sides;
            self.poly(&[a[i], a[j], b[j], b[i]], inside, col);
        }
    }

    /// A bar of square section `2r` from `a` to `b`.
    pub fn bar(&mut self, a: Vec3, b: Vec3, r: f32, color: Color) {
        let d = b - a;
        let len = d.length();
        if len < 1e-5 {
            return;
        }
        let z = d / len;
        let x = z.any_orthonormal_vector();
        let y = z.cross(x);
        let xf = Affine3A::from_mat3_translation(Mat3::from_cols(x, y, z), a);
        self.with(xf, |m| {
            m.cube(Vec3::new(-r, -r, 0.0), Vec3::new(r, r, len), color);
        });
    }

    /// A regular icosahedron of radius `r`.
    pub fn icosahedron(&mut self, r: f32, color: [f32; 4]) {
        self.geosphere(r, 0, color);
    }

    /// A geodesic sphere: an icosahedron with each face split `subdivisions`
    /// times (0 = 20 faces, 1 = 80, 2 = 320), for round pops and shells.
    pub fn geosphere(&mut self, r: f32, subdivisions: u32, color: [f32; 4]) {
        let mut faces: Vec<[Vec3; 3]> = icosahedron_faces();
        for _ in 0..subdivisions {
            faces = faces
                .into_iter()
                .flat_map(|[a, b, c]| {
                    let ab = (a + b).normalize();
                    let bc = (b + c).normalize();
                    let ca = (c + a).normalize();
                    [[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]
                })
                .collect();
        }
        for [a, b, c] in faces {
            self.poly(&[a * r, b * r, c * r], Vec3::ZERO, color);
        }
    }
}

/// The 20 faces of a unit icosahedron.
fn icosahedron_faces() -> Vec<[Vec3; 3]> {
    {
        let t = (1.0 + 5f32.sqrt()) / 2.0;
        let v = [
            Vec3::new(-1.0, t, 0.0),
            Vec3::new(1.0, t, 0.0),
            Vec3::new(-1.0, -t, 0.0),
            Vec3::new(1.0, -t, 0.0),
            Vec3::new(0.0, -1.0, t),
            Vec3::new(0.0, 1.0, t),
            Vec3::new(0.0, -1.0, -t),
            Vec3::new(0.0, 1.0, -t),
            Vec3::new(t, 0.0, -1.0),
            Vec3::new(t, 0.0, 1.0),
            Vec3::new(-t, 0.0, -1.0),
            Vec3::new(-t, 0.0, 1.0),
        ]
        .map(|p| p.normalize());
        const F: [[usize; 3]; 20] = [
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
        F.iter().map(|f| [v[f[0]], v[f[1]], v[f[2]]]).collect()
    }
}

impl ModelBuilder {
    /// A flat ring (annulus) in the XZ plane, facing +Y.
    pub fn ring_xz(&mut self, inner: f32, outer: f32, segments: usize, color: [f32; 4]) {
        let segments = segments.max(3);
        for i in 0..segments {
            let a0 = std::f32::consts::TAU * i as f32 / segments as f32;
            let a1 = std::f32::consts::TAU * (i + 1) as f32 / segments as f32;
            let p = |r: f32, a: f32| Vec3::new(r * a.cos(), 0.0, r * a.sin());
            let quad = [p(inner, a0), p(outer, a0), p(outer, a1), p(inner, a1)];
            self.poly(&quad, Vec3::NEG_Y, color);
        }
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
