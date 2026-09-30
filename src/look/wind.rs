//! The wind field (M4 chunk 6, D123): one slowly varying breeze that the
//! grass, flowers, bushes and tree canopies sway in (a vertex offset in
//! `toon.wgsl` and the ink hull's `ink.wgsl`) and that the ambience's wind bed
//! gusts with ([`crate::audio::ambience`]).
//!
//! The breeze blows along [`WIND_DIR`]. Its strength is a sum of slow sines
//! ([`gust`], 0..1), and each gust travels across the island at
//! [`GUST_SPEED`], so a gust is seen rolling over the grass. A sway is a bend
//! with the wind (stronger in a gust) plus a small flutter across it, both
//! scaled by the material's amplitude and each vertex's weight: 0 where a
//! blade, stem or trunk meets the ground, 1 at its tip ([`SwayWeight`]).
//!
//! It is presentation only: the offset is applied on the GPU to what's drawn,
//! never to a collider, a hitbox or anything the simulation reads.
//! [`sway_offset`] is the CPU mirror of the shader's `sway_offset`; keep
//! them in step.

use bevy::prelude::*;

/// Where the wind blows toward (xz, unit): from the south-west, across the
/// spawn view.
pub const WIND_DIR: Vec2 = Vec2::new(0.8, -0.6);
/// How fast a gust rolls across the island (m/s).
pub const GUST_SPEED: f32 = 7.0;

/// How a toon material's vertices weigh their sway (the `sway` uniform's w).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SwayWeight {
    /// No sway.
    #[default]
    Off,
    /// By height in the model's own space: 0 below `from`, 1 above `to` (m).
    /// Tree canopies: the trunk stands, the crown moves.
    Height,
    /// Baked into the mesh: the vertex normal's length minus 1 (merged
    /// scenery: blade tips carry 1, roots 0). Normals are normalized for
    /// shading, so the length is free to carry it.
    Baked,
}

impl SwayWeight {
    pub fn code(self) -> f32 {
        match self {
            SwayWeight::Off => 0.0,
            SwayWeight::Height => 1.0,
            SwayWeight::Baked => 2.0,
        }
    }
}

/// A material's sway: its amplitude (m at full weight in a full gust), the
/// height band for [`SwayWeight::Height`], and the weighting.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Sway {
    pub amplitude: f32,
    pub from: f32,
    pub to: f32,
    pub weight: SwayWeight,
}

impl Sway {
    pub const OFF: Sway = Sway {
        amplitude: 0.0,
        from: 0.0,
        to: 1.0,
        weight: SwayWeight::Off,
    };

    /// Merged scenery with baked weights.
    pub const fn baked(amplitude: f32) -> Sway {
        Sway {
            amplitude,
            from: 0.0,
            to: 1.0,
            weight: SwayWeight::Baked,
        }
    }

    /// A model swaying above `from` m, fully by `to` m.
    pub const fn height(amplitude: f32, from: f32, to: f32) -> Sway {
        Sway {
            amplitude,
            from,
            to,
            weight: SwayWeight::Height,
        }
    }

    /// The uniform: x amplitude, y from, z to, w the weighting's code.
    pub fn to_vec4(self) -> Vec4 {
        Vec4::new(self.amplitude, self.from, self.to, self.weight.code())
    }

    pub fn is_on(self) -> bool {
        self.weight != SwayWeight::Off && self.amplitude > 0.0
    }
}

/// The breeze's strength at time `t` (s), 0..1: slow, uneven swells.
pub fn gust(t: f32) -> f32 {
    let s = 0.5
        + 0.26 * (0.23 * t).sin()
        + 0.15 * (0.61 * t + 1.7).sin()
        + 0.09 * (1.37 * t + 0.4).sin();
    s.clamp(0.0, 1.0)
}

/// The breeze's strength at `p` (world xz) at time `t`: gusts roll along the
/// wind at [`GUST_SPEED`].
pub fn gust_at(t: f32, p: Vec2) -> f32 {
    gust(t - p.dot(WIND_DIR) / GUST_SPEED)
}

/// The sway offset (m, world) at `t` for a vertex at `world` whose sway phase
/// comes from `anchor` (its own position for merged scenery, the model's
/// origin for a tree, so the crown moves as one), at `weight` with
/// `amplitude`. Mirrors `sway_offset` in `toon.wgsl` and `ink.wgsl`.
pub fn sway_offset(t: f32, anchor: Vec3, weight: f32, amplitude: f32) -> Vec3 {
    let g = gust_at(t, anchor.xz());
    let phase = anchor.x * 0.73 + anchor.z * 1.19;
    let bend = amplitude * weight * (0.3 + 0.7 * g) * (0.65 + 0.35 * (1.9 * t + phase).sin());
    let flutter = amplitude * weight * (0.15 + 0.25 * g) * (3.7 * t + phase * 1.7).sin();
    let along = Vec3::new(WIND_DIR.x, 0.0, WIND_DIR.y);
    let across = Vec3::new(-WIND_DIR.y, 0.0, WIND_DIR.x);
    // A bent tip dips a little.
    along * bend + across * flutter - Vec3::Y * 0.2 * bend.abs() * weight
}

/// The sway weight at local height `y` for a height band.
pub fn height_weight(y: f32, from: f32, to: f32) -> f32 {
    let x = ((y - from) / (to - from).max(1e-4)).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gusts_stay_in_range_and_vary() {
        let (mut lo, mut hi) = (1.0f32, 0.0f32);
        for i in 0..6000 {
            let g = gust(i as f32 * 0.1);
            assert!((0.0..=1.0).contains(&g));
            lo = lo.min(g);
            hi = hi.max(g);
        }
        assert!(hi - lo > 0.6, "{lo}..{hi}");
    }

    #[test]
    fn roots_never_move_and_tips_bend_with_the_wind() {
        for i in 0..200 {
            let t = i as f32 * 0.37;
            let p = Vec3::new(i as f32, 0.0, -(i as f32) * 0.5);
            assert_eq!(sway_offset(t, p, 0.0, 0.1), Vec3::ZERO);
            let o = sway_offset(t, p, 1.0, 0.1);
            assert!(o.length() <= 0.1 * 1.3, "{o}");
            assert!(o.xz().dot(WIND_DIR) > 0.0, "bends downwind");
        }
    }
}
