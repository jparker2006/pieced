//! Piece geometry: flat-shaded, vertex-colored meshes modeled as crafted wooden
//! panels (frame, planks, bevels), built once at startup and shared by every piece.
//! Each mesh is authored in its piece's local space (see `PieceSlot::transform`).

use crate::{
    palette,
    shared::{CELL_SIZE, LEVEL_HEIGHT},
};
use bevy::{
    asset::RenderAssetUsages,
    math::Affine3A,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

/// Accumulates flat-shaded polygons with per-vertex colors.
#[derive(Default)]
pub(crate) struct MeshBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
    /// Placement applied to everything added (local → mesh space).
    xf: Affine3A,
}

fn linear(color: Color, alpha: f32) -> [f32; 4] {
    let c = color.to_linear();
    [c.red, c.green, c.blue, alpha]
}

/// Scales a color's sRGB channels (k > 1 lightens).
pub(crate) fn shade(color: Color, k: f32) -> Color {
    let c = color.to_srgba();
    Color::srgba(
        (c.red * k).clamp(0.0, 1.0),
        (c.green * k).clamp(0.0, 1.0),
        (c.blue * k).clamp(0.0, 1.0),
        c.alpha,
    )
}

impl MeshBuilder {
    #[cfg(test)]
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    /// Runs `f` with `xf` applied on top of the current placement.
    pub fn with(&mut self, xf: Affine3A, f: impl FnOnce(&mut Self)) {
        let saved = self.xf;
        self.xf = saved * xf;
        f(self);
        self.xf = saved;
    }

    /// A convex polygon, wound so its normal points away from `inside`.
    pub fn poly(&mut self, points: &[Vec3], inside: Vec3, color: [f32; 4]) {
        if points.len() < 3 {
            return;
        }
        // Newell's method: robust normal for any planar polygon.
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
        let normal = self
            .xf
            .transform_vector3(if flip { -n } else { n })
            .normalize_or_zero();
        let start = self.positions.len() as u32;
        let count = points.len();
        for k in 0..count {
            let p = if flip {
                points[count - 1 - k]
            } else {
                points[k]
            };
            self.positions.push(self.xf.transform_point3(p).to_array());
            self.normals.push(normal.to_array());
            self.colors.push(color);
        }
        for k in 1..count as u32 - 1 {
            self.indices.extend([start, start + k, start + k + 1]);
        }
    }

    /// An axis-aligned box with chamfered edges (`bevel` = 0 for a plain box).
    /// Bevel faces get a slightly lighter tone, like worn edges catching light.
    pub fn chamfer_box(&mut self, min: Vec3, max: Vec3, bevel: f32, color: Color) {
        let c = (min + max) / 2.0;
        let h = (max - min) / 2.0;
        let b = bevel.min(h.min_element() * 0.45).max(0.0);
        let face = linear(color, 1.0);
        let edge = linear(shade(color, 1.07), 1.0);
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

    /// A convex polygon given in the (z, y) plane, extruded along x from `x0` to `x1`.
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

    /// A thin bar of square section `2r` from `a` to `b`.
    pub fn bar(&mut self, a: Vec3, b: Vec3, r: f32, color: [f32; 4]) {
        let d = b - a;
        let len = d.length();
        if len < 1e-4 {
            return;
        }
        let z = d / len;
        let x = z.any_orthonormal_vector();
        let y = z.cross(x);
        let xf = Affine3A::from_mat3_translation(Mat3::from_cols(x, y, z), a);
        self.with(xf, |m| {
            let c = Vec3::new(0.0, 0.0, len / 2.0);
            let h = Vec3::new(r, r, len / 2.0 + r);
            for a in 0..3 {
                let (u, v) = ((a + 1) % 3, (a + 2) % 3);
                for s in [-1.0, 1.0] {
                    let pts =
                        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(su, sv)| {
                            let mut p = Vec3::ZERO;
                            p[a] = s * h[a];
                            p[u] = su * h[u];
                            p[v] = sv * h[v];
                            c + p
                        });
                    m.poly(&pts, c, color);
                }
            }
        });
    }

    /// A tapering crack drawn just above a plane (origin, in-plane axes u and v,
    /// outward normal n) through 2D points in that plane.
    pub fn crack(&mut self, origin: Vec3, u: Vec3, v: Vec3, n: Vec3, pts: &[Vec2], width: f32) {
        let col = linear(CRACK, 1.0);
        let lift = n * 0.004;
        let map = |p: Vec2| origin + u * p.x + v * p.y + lift;
        let segments = pts.len().saturating_sub(1).max(1) as f32;
        for (i, w) in pts.windows(2).enumerate() {
            let (a, b) = (w[0], w[1]);
            let dir = (b - a).normalize_or_zero();
            let perp = Vec2::new(-dir.y, dir.x);
            let wa = width * (1.0 - 0.7 * i as f32 / segments) / 2.0;
            let wb = width * (1.0 - 0.7 * (i as f32 + 1.0) / segments) / 2.0;
            let quad = [
                map(a - perp * wa),
                map(b - perp * wb),
                map(b + perp * wb),
                map(a + perp * wa),
            ];
            self.poly(&quad, origin - n, col);
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

const CRACK: Color = Color::srgb(0.24, 0.14, 0.07);

fn plank_color(i: usize) -> Color {
    [
        palette::WOOD,
        palette::WOOD_LIGHT,
        shade(palette::WOOD, 1.05),
        shade(palette::WOOD_LIGHT, 0.95),
        palette::WOOD,
        palette::WOOD_LIGHT,
    ][i % 6]
}

fn rot_z(angle: f32, pivot: Vec3) -> Affine3A {
    Affine3A::from_translation(pivot)
        * Affine3A::from_rotation_z(angle)
        * Affine3A::from_translation(-pivot)
}

fn rot_y(angle: f32, pivot: Vec3) -> Affine3A {
    Affine3A::from_translation(pivot)
        * Affine3A::from_rotation_y(angle)
        * Affine3A::from_translation(-pivot)
}

fn v2(points: &[(f32, f32)]) -> Vec<Vec2> {
    points.iter().map(|&(x, y)| Vec2::new(x, y)).collect()
}

// ---------------------------------------------------------------------------
// Wall: centered on its origin, 4 m along X, 3 m tall, thin along Z.
// ---------------------------------------------------------------------------

const HALF_W: f32 = CELL_SIZE / 2.0;
const HALF_H: f32 = LEVEL_HEIGHT / 2.0;

pub(crate) fn wall_mesh(stage: u8) -> MeshBuilder {
    let mut m = MeshBuilder::default();
    let post_w = 0.26;
    let post_t = 0.12;
    let rail_h = 0.24;
    let rail_t = 0.11;
    let plank_t = 0.085;
    let inner_x = HALF_W - post_w;
    let inner_y = HALF_H - rail_h;
    // Frame: posts and rails, proud of the planks.
    for s in [-1.0, 1.0] {
        let (x0, x1) = if s < 0.0 {
            (-HALF_W, -inner_x)
        } else {
            (inner_x, HALF_W)
        };
        m.chamfer_box(
            Vec3::new(x0, -HALF_H, -post_t),
            Vec3::new(x1, HALF_H, post_t),
            0.04,
            palette::WOOD_DARK,
        );
        let (y0, y1) = if s < 0.0 {
            (-HALF_H, -inner_y)
        } else {
            (inner_y, HALF_H)
        };
        m.chamfer_box(
            Vec3::new(-inner_x - 0.01, y0, -rail_t),
            Vec3::new(inner_x + 0.01, y1, rail_t),
            0.035,
            palette::WOOD_DARK,
        );
    }
    // Dark core, seen through the plank grooves.
    m.chamfer_box(
        Vec3::new(-inner_x, -inner_y, -0.03),
        Vec3::new(inner_x, inner_y, 0.03),
        0.0,
        palette::WOOD_TRIM,
    );
    // Four horizontal planks.
    let pitch = 2.0 * inner_y / 4.0;
    let gap = 0.045;
    for i in 0..4 {
        let y0 = -inner_y + i as f32 * pitch + gap / 2.0;
        let y1 = y0 + pitch - gap;
        let color = plank_color(i);
        let broken = stage >= 2 && i == 1;
        let askew = stage >= 2 && i == 2;
        if broken {
            // Snapped plank: two stubs with the core showing between.
            for (x0, x1, tilt) in [
                (-inner_x - 0.02, -0.42, -0.035),
                (0.36, inner_x + 0.02, 0.03),
            ] {
                let pivot = Vec3::new(if tilt < 0.0 { x0 } else { x1 }, (y0 + y1) / 2.0, 0.0);
                m.with(rot_z(tilt, pivot), |m| {
                    m.chamfer_box(
                        Vec3::new(x0, y0, -plank_t),
                        Vec3::new(x1, y1, plank_t),
                        0.03,
                        color,
                    );
                });
            }
        } else if askew {
            let pivot = Vec3::new(-inner_x, (y0 + y1) / 2.0, 0.0);
            m.with(rot_z(-0.025, pivot), |m| {
                m.chamfer_box(
                    Vec3::new(-inner_x - 0.02, y0, -plank_t + 0.01),
                    Vec3::new(inner_x + 0.02, y1, plank_t - 0.01),
                    0.03,
                    color,
                );
            });
        } else {
            m.chamfer_box(
                Vec3::new(-inner_x - 0.02, y0, -plank_t),
                Vec3::new(inner_x + 0.02, y1, plank_t),
                0.03,
                color,
            );
        }
    }
    // A diagonal brace on each face.
    let diag = Vec2::new(2.0 * inner_x, 2.0 * inner_y);
    let len = diag.length();
    let angle = diag.y.atan2(diag.x);
    for s in [-1.0f32, 1.0] {
        let keep = if stage >= 2 && s > 0.0 { 0.56 } else { 1.0 };
        let z0 = s * 0.075;
        let z1 = s * 0.104;
        m.with(Affine3A::from_rotation_z(angle), |m| {
            m.chamfer_box(
                Vec3::new(-len / 2.0, -0.1, z0.min(z1)),
                Vec3::new(-len / 2.0 + len * keep, 0.1, z0.max(z1)),
                0.02,
                shade(palette::WOOD_DARK, 1.04),
            );
        });
    }
    // Cracks on both faces, just above the planks.
    if stage >= 1 {
        let mut cracks = vec![
            v2(&[(-1.3, 0.95), (-0.95, 0.6), (-1.05, 0.2), (-0.7, -0.15)]),
            v2(&[(1.1, -0.95), (0.85, -0.55), (1.0, -0.2)]),
            v2(&[(0.25, 1.1), (0.05, 0.85), (0.2, 0.55)]),
        ];
        if stage >= 2 {
            cracks.extend([
                v2(&[
                    (-0.2, 0.3),
                    (0.15, -0.05),
                    (-0.05, -0.45),
                    (0.3, -0.85),
                    (0.15, -1.1),
                ]),
                v2(&[(1.55, 0.9), (1.2, 0.55), (1.35, 0.2), (1.05, -0.1)]),
                v2(&[(-1.6, -0.5), (-1.2, -0.8), (-0.8, -0.75), (-0.5, -1.05)]),
                v2(&[(-0.6, 1.1), (-0.35, 0.8), (-0.55, 0.5)]),
            ]);
        }
        for s in [-1.0f32, 1.0] {
            let n = Vec3::Z * s;
            let u = Vec3::X * s;
            for crack in &cracks {
                m.crack(n * plank_t, u, Vec3::Y, n, crack, 0.05);
            }
        }
    }
    m
}

// ---------------------------------------------------------------------------
// Floor: centered on its origin, 4 × 4 m, planks running along local Z.
// ---------------------------------------------------------------------------

pub(crate) fn floor_mesh(stage: u8) -> MeshBuilder {
    let mut m = MeshBuilder::default();
    let beam = 0.22;
    let inner = HALF_W - beam;
    let (bottom, top) = (-0.1, 0.1);
    // Frame beams (sides along X span the full width).
    for s in [-1.0, 1.0] {
        let (z0, z1) = if s < 0.0 {
            (-HALF_W, -inner)
        } else {
            (inner, HALF_W)
        };
        m.chamfer_box(
            Vec3::new(-HALF_W, bottom, z0),
            Vec3::new(HALF_W, top + 0.015, z1),
            0.04,
            palette::WOOD_DARK,
        );
        m.chamfer_box(
            Vec3::new(z0, bottom, -inner),
            Vec3::new(z1, top + 0.015, inner),
            0.04,
            palette::WOOD_DARK,
        );
    }
    // Core under the planks (its underside is the floor's ceiling face).
    m.chamfer_box(
        Vec3::new(-inner, bottom, -inner),
        Vec3::new(inner, -0.05, inner),
        0.0,
        palette::WOOD_TRIM,
    );
    let count = 5;
    let pitch = 2.0 * inner / count as f32;
    let gap = 0.04;
    for i in 0..count {
        let x0 = -inner + i as f32 * pitch + gap / 2.0;
        let x1 = x0 + pitch - gap;
        let color = plank_color(i + 1);
        if stage >= 2 && i == 2 {
            for (z0, z1) in [(-inner - 0.02, -0.3), (0.55, inner + 0.02)] {
                m.chamfer_box(
                    Vec3::new(x0, -0.05, z0),
                    Vec3::new(x1, top - 0.012, z1),
                    0.028,
                    shade(color, 0.96),
                );
            }
        } else if stage >= 2 && i == 4 {
            m.with(rot_y(0.02, Vec3::new(x0, 0.0, -inner)), |m| {
                m.chamfer_box(
                    Vec3::new(x0, -0.05, -inner - 0.02),
                    Vec3::new(x1, top - 0.006, inner + 0.02),
                    0.028,
                    color,
                );
            });
        } else {
            m.chamfer_box(
                Vec3::new(x0, -0.05, -inner - 0.02),
                Vec3::new(x1, top, inner + 0.02),
                0.028,
                color,
            );
        }
    }
    if stage >= 1 {
        let mut cracks = vec![
            v2(&[(-1.2, -1.3), (-0.8, -0.9), (-0.95, -0.4), (-0.6, 0.0)]),
            v2(&[(1.2, 1.1), (0.8, 0.75), (0.95, 0.35)]),
        ];
        if stage >= 2 {
            cracks.extend([
                v2(&[(0.4, -1.4), (0.1, -1.0), (0.3, -0.6), (-0.05, -0.2)]),
                v2(&[(-1.4, 0.8), (-1.0, 1.1), (-0.6, 0.95), (-0.3, 1.35)]),
                v2(&[(1.4, -0.6), (1.05, -0.3), (1.2, 0.05)]),
            ]);
        }
        for crack in &cracks {
            m.crack(Vec3::Y * top, Vec3::X, Vec3::NEG_Z, Vec3::Y, crack, 0.055);
        }
    }
    m
}

// ---------------------------------------------------------------------------
// Ramp: base center at the origin, rising 3 m over 4 m toward local -Z.
// ---------------------------------------------------------------------------

/// Maps slope space (x across, y = t along the slope normal, z = s down the
/// slope) onto the ramp's walking surface.
fn slope_frame() -> Affine3A {
    let run = CELL_SIZE;
    let rise = LEVEL_HEIGHT;
    let len = (run * run + rise * rise).sqrt();
    let down = Vec3::new(0.0, -rise / len, run / len);
    let normal = Vec3::new(0.0, run / len, rise / len);
    Affine3A::from_mat3_translation(
        Mat3::from_cols(Vec3::X, normal, down),
        Vec3::new(0.0, rise / 2.0, 0.0),
    )
}

pub(crate) fn ramp_mesh(stage: u8) -> MeshBuilder {
    let mut m = MeshBuilder::default();
    let run = CELL_SIZE;
    let rise = LEVEL_HEIGHT;
    let slope_len = (run * run + rise * rise).sqrt();
    let half_len = slope_len / 2.0;
    let side = 0.22;
    let inner = HALF_W - side;
    let tread_t = 0.12;
    // Deck: treads across the slope, cleats, and a dark sub-deck.
    m.with(slope_frame(), |m| {
        let s_top = -half_len + 0.08;
        m.chamfer_box(
            Vec3::new(-inner, -tread_t - 0.05, s_top + 0.06),
            Vec3::new(inner, -tread_t + 0.005, half_len - 0.08),
            0.0,
            palette::WOOD_TRIM,
        );
        let count = 6;
        let pitch = (half_len - s_top) / count as f32;
        let gap = 0.045;
        for i in 0..count {
            let s0 = s_top + i as f32 * pitch + gap / 2.0;
            let s1 = s0 + pitch - gap;
            let color = plank_color(i);
            if stage >= 2 && i == 2 {
                for (x0, x1) in [(-inner - 0.02, -0.45), (0.5, inner + 0.02)] {
                    m.chamfer_box(
                        Vec3::new(x0, -tread_t, s0),
                        Vec3::new(x1, -0.012, s1),
                        0.026,
                        shade(color, 0.96),
                    );
                }
            } else if stage >= 2 && i == 4 {
                m.with(
                    Affine3A::from_translation(Vec3::new(0.0, -0.01, 0.0))
                        * Affine3A::from_rotation_y(0.025),
                    |m| {
                        m.chamfer_box(
                            Vec3::new(-inner - 0.02, -tread_t, s0),
                            Vec3::new(inner + 0.02, 0.0, s1),
                            0.026,
                            color,
                        );
                    },
                );
            } else {
                m.chamfer_box(
                    Vec3::new(-inner - 0.02, -tread_t, s0),
                    Vec3::new(inner + 0.02, 0.0, s1),
                    0.026,
                    color,
                );
                if i % 2 == 1 {
                    let c = (s0 + s1) / 2.0;
                    m.chamfer_box(
                        Vec3::new(-inner + 0.15, -0.01, c - 0.05),
                        Vec3::new(inner - 0.15, 0.035, c + 0.05),
                        0.012,
                        palette::WOOD_DARK,
                    );
                }
            }
        }
        if stage >= 1 {
            let mut cracks = vec![
                v2(&[(-1.2, 1.8), (-0.8, 1.3), (-1.0, 0.8), (-0.6, 0.4)]),
                v2(&[(1.1, -1.6), (0.8, -1.2), (1.0, -0.8)]),
            ];
            if stage >= 2 {
                cracks.extend([
                    v2(&[(0.2, 2.2), (0.5, 1.7), (0.25, 1.2), (0.55, 0.7)]),
                    v2(&[(-1.4, -0.4), (-1.0, -0.8), (-1.2, -1.3), (-0.8, -1.9)]),
                    v2(&[(1.3, 0.6), (0.9, 0.3), (1.1, -0.1)]),
                ]);
            }
            for crack in &cracks {
                // Plane coordinates: x across, y down the slope.
                m.crack(Vec3::ZERO, Vec3::X, Vec3::Z, Vec3::Y, crack, 0.055);
            }
        }
    });
    // Side stringers: a band along the slope edge, clipped to the cell.
    let slope_y = |z: f32| (HALF_W - z) / run * rise;
    let band = 0.4;
    let low_z = HALF_W - band / (rise / run);
    let stringer = v2(&[
        (HALF_W, 0.0),
        (HALF_W, 0.05),
        (-HALF_W, rise + 0.05),
        (-HALF_W, rise - band),
        (low_z, 0.0),
    ]);
    // Recessed side panel under the stringer.
    let panel = v2(&[
        (low_z, 0.0),
        (-HALF_W + side, 0.0),
        (-HALF_W + side, slope_y(-HALF_W + side) - band),
    ]);
    for s in [-1.0f32, 1.0] {
        let (x0, x1) = (s * HALF_W, s * inner);
        m.prism_x(&stringer, x0.min(x1), x0.max(x1), palette::WOOD_DARK);
        let (p0, p1) = (s * (HALF_W - 0.04), s * (inner + 0.03));
        m.prism_x(&panel, p0.min(p1), p0.max(p1), palette::WOOD_TRIM);
        // High-end post and ground beam.
        let (bx0, bx1) = if s < 0.0 {
            (-HALF_W, -inner + 0.02)
        } else {
            (inner - 0.02, HALF_W)
        };
        m.chamfer_box(
            Vec3::new(bx0, 0.0, -HALF_W),
            Vec3::new(bx1, rise - band + 0.02, -HALF_W + side + 0.02),
            0.035,
            palette::WOOD_DARK,
        );
        m.chamfer_box(
            Vec3::new(bx0, 0.0, -HALF_W + side),
            Vec3::new(bx1, 0.2, low_z - 0.05),
            0.035,
            shade(palette::WOOD_DARK, 0.95),
        );
    }
    // Back panel at the high end, up under the deck: core plus three planks.
    let back_top = rise - 0.22;
    m.chamfer_box(
        Vec3::new(-inner, 0.0, -HALF_W + 0.03),
        Vec3::new(inner, back_top, -HALF_W + 0.1),
        0.0,
        palette::WOOD_TRIM,
    );
    let pitch = back_top / 3.0;
    for i in 0..3 {
        let y0 = i as f32 * pitch + 0.02;
        m.chamfer_box(
            Vec3::new(-inner - 0.01, y0, -HALF_W + 0.005),
            Vec3::new(inner + 0.01, y0 + pitch - 0.04, -HALF_W + 0.07),
            0.025,
            shade(plank_color(i + 3), 0.9),
        );
    }
    // Underside (seen when the ramp is built above head height).
    m.poly(
        &[
            Vec3::new(-HALF_W, 0.002, -HALF_W),
            Vec3::new(HALF_W, 0.002, -HALF_W),
            Vec3::new(HALF_W, 0.002, HALF_W),
            Vec3::new(-HALF_W, 0.002, HALF_W),
        ],
        Vec3::new(0.0, 1.0, 0.0),
        linear(palette::WOOD_TRIM, 1.0),
    );
    m
}

// ---------------------------------------------------------------------------
// Ghost previews: the piece's volume, slightly enlarged, with bright edges.
// ---------------------------------------------------------------------------

const GHOST_FACE_ALPHA: f32 = 0.26;
const GHOST_EDGE_ALPHA: f32 = 0.8;
const GHOST_EDGE: f32 = 0.035;

fn ghost_convex(m: &mut MeshBuilder, corners: &[Vec3], faces: &[&[usize]]) {
    let white = |a: f32| [1.0, 1.0, 1.0, a];
    let center = corners.iter().copied().sum::<Vec3>() / corners.len() as f32;
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for face in faces {
        let pts: Vec<Vec3> = face.iter().map(|&i| corners[i]).collect();
        m.poly(&pts, center, white(GHOST_FACE_ALPHA));
        for k in 0..face.len() {
            let (a, b) = (face[k], face[(k + 1) % face.len()]);
            let e = (a.min(b), a.max(b));
            if !edges.contains(&e) {
                edges.push(e);
            }
        }
    }
    for (a, b) in edges {
        m.bar(corners[a], corners[b], GHOST_EDGE, white(GHOST_EDGE_ALPHA));
    }
}

fn ghost_box(half: Vec3, offset: Vec3) -> MeshBuilder {
    let mut m = MeshBuilder::default();
    let corners: Vec<Vec3> = (0..8)
        .map(|i| {
            offset
                + half
                    * Vec3::new(
                        if i & 1 == 0 { -1.0 } else { 1.0 },
                        if i & 2 == 0 { -1.0 } else { 1.0 },
                        if i & 4 == 0 { -1.0 } else { 1.0 },
                    )
        })
        .collect();
    let faces: [&[usize]; 6] = [
        &[0, 1, 3, 2],
        &[4, 5, 7, 6],
        &[0, 1, 5, 4],
        &[2, 3, 7, 6],
        &[0, 2, 6, 4],
        &[1, 3, 7, 5],
    ];
    ghost_convex(&mut m, &corners, &faces);
    m
}

pub(crate) fn ghost_wall_mesh(thickness: f32) -> MeshBuilder {
    ghost_box(
        Vec3::new(HALF_W + 0.02, HALF_H + 0.02, thickness / 2.0 + 0.05),
        Vec3::ZERO,
    )
}

pub(crate) fn ghost_floor_mesh(thickness: f32) -> MeshBuilder {
    ghost_box(
        Vec3::new(HALF_W + 0.02, thickness / 2.0 + 0.04, HALF_W + 0.02),
        Vec3::ZERO,
    )
}

pub(crate) fn ghost_ramp_mesh() -> MeshBuilder {
    let mut m = MeshBuilder::default();
    let w = HALF_W + 0.02;
    let top = LEVEL_HEIGHT + 0.04;
    let corners = [
        Vec3::new(-w, -0.02, w),
        Vec3::new(w, -0.02, w),
        Vec3::new(-w, -0.02, -w),
        Vec3::new(w, -0.02, -w),
        Vec3::new(-w, top, -w),
        Vec3::new(w, top, -w),
    ];
    let faces: [&[usize]; 5] = [
        &[0, 1, 3, 2],
        &[0, 1, 5, 4],
        &[2, 3, 5, 4],
        &[0, 2, 4],
        &[1, 3, 5],
    ];
    ghost_convex(&mut m, &corners, &faces);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn piece_meshes_stay_within_budget_and_are_consistent() {
        for stage in 0..3 {
            for (name, m) in [
                ("wall", wall_mesh(stage)),
                ("floor", floor_mesh(stage)),
                ("ramp", ramp_mesh(stage)),
            ] {
                let tris = m.triangles();
                assert!(tris > 100, "{name} stage {stage}: {tris} triangles");
                assert!(tris < 1200, "{name} stage {stage}: {tris} triangles");
                assert_eq!(m.positions.len(), m.normals.len());
                assert_eq!(m.positions.len(), m.colors.len());
                assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()));
                assert!(
                    m.normals
                        .iter()
                        .all(|n| (Vec3::from_array(*n).length() - 1.0).abs() < 1e-3),
                    "{name}: unit normals"
                );
            }
        }
    }

    #[test]
    fn meshes_stay_inside_their_cell() {
        let check = |name: &str, m: &MeshBuilder, min: Vec3, max: Vec3| {
            for p in &m.positions {
                let p = Vec3::from_array(*p);
                assert!(
                    p.cmpge(min - 0.001).all() && p.cmple(max + 0.001).all(),
                    "{name}: vertex {p} outside {min}..{max}"
                );
            }
        };
        for stage in 0..3 {
            check(
                "wall",
                &wall_mesh(stage),
                Vec3::new(-HALF_W, -HALF_H, -0.13),
                Vec3::new(HALF_W, HALF_H, 0.13),
            );
            check(
                "floor",
                &floor_mesh(stage),
                Vec3::new(-HALF_W, -0.1, -HALF_W),
                Vec3::new(HALF_W, 0.12, HALF_W),
            );
            check(
                "ramp",
                &ramp_mesh(stage),
                Vec3::new(-HALF_W, -0.13, -HALF_W - 0.02),
                Vec3::new(HALF_W, LEVEL_HEIGHT + 0.06, HALF_W + 0.1),
            );
        }
    }
}
