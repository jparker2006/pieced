//! The target look: a chunky low-poly training-dummy figure for every non-player
//! character, with a bullseye plate and a fresnel rim light so it pops against any
//! background and still reads in greyscale. The figure fits the gameplay hitboxes
//! (body capsule r 0.33 from y 0.05 to 1.45, head sphere r 0.2 at y 1.62).

use super::geo::{Geo, Rgba, blob, lin, mix, ring, shade};
use crate::palette;
use bevy::{
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
};

pub const TARGET_SHADER_PATH: &str = "embedded://pieced/shaders/target_rim.wgsl";

/// The dummy material: `StandardMaterial` lighting plus a fresnel rim.
pub type TargetMaterial = ExtendedMaterial<StandardMaterial, TargetRim>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct TargetRim {
    #[uniform(100)]
    pub color: LinearRgba,
    /// x: rim exponent, y: rim strength, z: self-light lift (keeps the shadow side
    /// saturated), w: unused.
    #[uniform(100)]
    pub params: Vec4,
}

impl Default for TargetRim {
    fn default() -> Self {
        Self {
            color: palette::TARGET_RIM.to_linear(),
            params: Vec4::new(2.0, 1.5, 0.12, 0.0),
        }
    }
}

impl MaterialExtension for TargetRim {
    fn fragment_shader() -> ShaderRef {
        TARGET_SHADER_PATH.into()
    }
}

pub fn target_material() -> TargetMaterial {
    ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.55,
            reflectance: 0.35,
            ..default()
        },
        extension: TargetRim::default(),
    }
}

/// Octagonal prism/loft through `(y, radius_x, radius_z)` stations, face toward -Z.
fn lathe(geo: &mut Geo, center: Vec3, sides: usize, stations: &[(f32, f32, f32)], color: Rgba) {
    let phase = std::f32::consts::PI / sides as f32;
    let rings: Vec<Vec<Vec3>> = stations
        .iter()
        .map(|&(y, rx, rz)| {
            ring(center, sides, phase, y, |_| 1.0)
                .into_iter()
                .map(|p| {
                    let off = p - center;
                    center + Vec3::new(off.x * rx, off.y, off.z * rz)
                })
                .collect()
        })
        .collect();
    let mut k = 0;
    geo.loft(&rings, |_, _| {
        k += 1;
        // Alternate facets slightly so the flat shading reads even in flat light.
        shade(color, if k % 2 == 0 { 1.0 } else { 0.94 })
    });
    geo.cap(rings.last().expect("stations"), true, shade(color, 1.05));
    geo.cap(&rings[0], false, shade(color, 0.8));
}

/// A flat disc facing -Z (or +Z when `back`), `thickness` deep.
fn plate(geo: &mut Geo, center: Vec3, radius: f32, thickness: f32, back: bool, color: Rgba) {
    let sides = 12;
    let dir = if back { 1.0 } else { -1.0 };
    let pts: Vec<Vec3> = (0..sides)
        .map(|i| {
            let a = std::f32::consts::TAU * i as f32 / sides as f32;
            Vec3::new(a.cos() * radius, a.sin() * radius, 0.0)
        })
        .collect();
    let front: Vec<Vec3> = pts
        .iter()
        .map(|p| center + *p + Vec3::Z * dir * thickness * 0.5)
        .collect();
    let rear: Vec<Vec3> = pts
        .iter()
        .map(|p| center + *p - Vec3::Z * dir * thickness * 0.5)
        .collect();
    let c_front = center + Vec3::Z * dir * thickness * 0.5;
    for i in 0..sides {
        let j = (i + 1) % sides;
        // Front face must face along `dir`.
        let (a, b) = if back {
            (front[i], front[j])
        } else {
            (front[j], front[i])
        };
        geo.tri(c_front, a, b, color);
        let (r0, r1, f0, f1) = if back {
            (rear[i], rear[j], front[i], front[j])
        } else {
            (rear[j], rear[i], front[j], front[i])
        };
        geo.quad(r0, r1, f1, f0, shade(color, 0.85));
    }
}

/// The training-dummy figure, feet at the origin, facing -Z.
pub fn figure() -> Geo {
    // A touch deeper than the pure hue: in greyscale the body reads darker than
    // the mid-grey world while the rim and bullseye read lighter.
    let body = mix(lin(palette::TARGET), lin(palette::TARGET_DARK), 0.3);
    let dark = lin(palette::TARGET_DARK);
    let light = lin(palette::TARGET_LIGHT);
    let mut g = Geo::default();
    for side in [-1.0f32, 1.0] {
        let x = side * 0.13;
        // Feet: chunky wedges, toes forward.
        lathe(
            &mut g,
            Vec3::new(x, 0.0, -0.03),
            4,
            &[(0.0, 0.11, 0.18), (0.1, 0.1, 0.15)],
            dark,
        );
        // Legs.
        lathe(
            &mut g,
            Vec3::new(x, 0.0, 0.0),
            6,
            &[(0.08, 0.095, 0.095), (0.62, 0.11, 0.11)],
            dark,
        );
        // Arms hang close to the body, inside the hitbox silhouette.
        lathe(
            &mut g,
            Vec3::new(side * 0.275, 0.0, 0.0),
            5,
            &[(0.84, 0.055, 0.055), (1.22, 0.068, 0.068)],
            dark,
        );
        blob(
            &mut g,
            0,
            |v| Vec3::new(side * 0.28, 0.8, 0.0) + v * 0.068,
            |_| body,
        );
    }
    // Pelvis and torso: a beveled octagonal barrel, widest at the chest.
    lathe(
        &mut g,
        Vec3::ZERO,
        8,
        &[(0.56, 0.22, 0.2), (0.74, 0.24, 0.22)],
        dark,
    );
    lathe(
        &mut g,
        Vec3::ZERO,
        8,
        &[
            (0.72, 0.25, 0.23),
            (0.98, 0.29, 0.26),
            (1.18, 0.3, 0.27),
            (1.33, 0.25, 0.22),
            (1.43, 0.15, 0.13),
        ],
        body,
    );
    // Neck and faceted head.
    lathe(
        &mut g,
        Vec3::ZERO,
        6,
        &[(1.4, 0.08, 0.08), (1.5, 0.075, 0.075)],
        dark,
    );
    blob(
        &mut g,
        1,
        |v| Vec3::new(0.0, 1.62, 0.0) + v * 0.2,
        |n| shade(body, 0.96 + 0.08 * n.y.max(0.0)),
    );
    // Visor band so facing reads at a glance.
    lathe(
        &mut g,
        Vec3::new(0.0, 0.0, -0.12),
        4,
        &[(1.6, 0.16, 0.12), (1.66, 0.16, 0.12)],
        dark,
    );
    // Bullseye plates on chest and back: light / dark / light rings.
    for back in [false, true] {
        let s = if back { 1.0 } else { -1.0 };
        plate(
            &mut g,
            Vec3::new(0.0, 1.08, s * 0.26),
            0.2,
            0.07,
            back,
            light,
        );
        plate(
            &mut g,
            Vec3::new(0.0, 1.08, s * 0.298),
            0.135,
            0.006,
            back,
            dark,
        );
        plate(
            &mut g,
            Vec3::new(0.0, 1.08, s * 0.303),
            0.065,
            0.006,
            back,
            light,
        );
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::{BODY_BOTTOM, BODY_RADIUS, BODY_TOP, HEAD_CENTER, HEAD_RADIUS};

    /// Distance from a point to the body capsule's core segment.
    fn capsule_distance(p: Vec3) -> f32 {
        let lo = BODY_BOTTOM + BODY_RADIUS;
        let hi = BODY_TOP - BODY_RADIUS;
        let y = p.y.clamp(lo, hi);
        p.distance(Vec3::new(0.0, y, 0.0))
    }

    #[test]
    fn figure_lines_up_with_the_hitboxes() {
        let g = figure();
        let (lo, hi) = g.bounds().unwrap();
        // Top of the head matches the head sphere; feet on the ground.
        assert!(
            (hi.y - (HEAD_CENTER + HEAD_RADIUS)).abs() < 0.03,
            "top {}",
            hi.y
        );
        assert!(lo.y >= -0.01 && lo.y < 0.05, "bottom {}", lo.y);
        // Every vertex sits inside the hitboxes, give or take a few centimeters.
        for v in g.vertices() {
            let in_body = capsule_distance(v) <= BODY_RADIUS + 0.05;
            let in_head = v.distance(Vec3::Y * HEAD_CENTER) <= HEAD_RADIUS + 0.05;
            // Feet may poke slightly out of the capsule's rounded bottom.
            let foot = v.y < 0.12 && v.xz().length() < 0.36;
            assert!(in_body || in_head || foot, "vertex outside hitboxes: {v}");
        }
        // And the silhouette fills them: wide enough to read as the body.
        assert!(hi.x > 0.3 && lo.x < -0.3);
    }

    #[test]
    fn bullseye_faces_front_and_back() {
        let g = figure();
        let light = lin(palette::TARGET_LIGHT);
        let mut front = 0;
        let mut back = 0;
        for (i, n) in g.normals.chunks(3).enumerate() {
            if g.colors[i * 3] == light {
                let n = Vec3::from_array(n[0]);
                if n.z < -0.99 {
                    front += 1;
                }
                if n.z > 0.99 {
                    back += 1;
                }
            }
        }
        assert!(front >= 24 && back >= 24, "front {front}, back {back}");
    }
}
