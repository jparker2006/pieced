//! The colour grade every lit look shader ends with (`grade.wgsl`, imported as
//! `pieced::grade::grade`): a vibrance lift, a contrast lift and a slight split
//! tone (cool shadows, warm highlights), in gamma-2 space. It replaces a
//! full-screen pass: a dozen ALU ops per fragment, no extra target, and the
//! cameras keep `Tonemapping::None`. HUD and UI are never graded.
//!
//! [`grade`] mirrors the shader on the CPU; keep the constants in step.

use bevy::prelude::*;

/// Saturation lift, scaled by how muted the colour is (1 - spread), so
/// already-vivid palette colours barely move and never clip, and faded out in
/// the darks (shadows stay rich, not neon).
pub const GRADE_VIBRANCE: f32 = 0.22;
/// Contrast around [`GRADE_PIVOT`], in gamma-2 space.
pub const GRADE_CONTRAST: f32 = 1.1;
pub const GRADE_PIVOT: f32 = 0.45;
/// Added (gamma-2 space) to the darkest tones...
pub const GRADE_COOL: Vec3 = Vec3::new(-0.008, 0.0, 0.018);
/// ...blending to this on the brightest.
pub const GRADE_WARM: Vec3 = Vec3::new(0.024, 0.012, -0.02);

/// The shader's grade: linear rgb in, linear rgb out (clamped to 0..1).
pub fn grade(linear: Vec3) -> Vec3 {
    let mut p = linear.max(Vec3::ZERO).map(f32::sqrt);
    let l = p.dot(Vec3::new(0.2126, 0.7152, 0.0722));
    let spread = p.max_element() - p.min_element();
    let vibrance = GRADE_VIBRANCE * (1.0 - spread).clamp(0.0, 1.0) * smoothstep(0.08, 0.45, l);
    p = Vec3::splat(l) + (p - Vec3::splat(l)) * (1.0 + vibrance);
    p = (p - Vec3::splat(GRADE_PIVOT)) * GRADE_CONTRAST + Vec3::splat(GRADE_PIVOT);
    p += GRADE_COOL.lerp(GRADE_WARM, smoothstep(0.15, 0.85, l));
    let p = p.clamp(Vec3::ZERO, Vec3::ONE);
    p * p
}

pub(crate) fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn srgb(c: Color) -> Vec3 {
        let l = c.to_linear();
        Vec3::new(l.red, l.green, l.blue)
    }

    fn saturation(c: Vec3) -> f32 {
        let s = Color::linear_rgb(c.x, c.y, c.z).to_srgba();
        let (hi, lo) = (
            s.red.max(s.green).max(s.blue),
            s.red.min(s.green).min(s.blue),
        );
        (hi - lo) / hi.max(1e-5)
    }

    #[test]
    fn muted_palette_colours_get_more_vivid() {
        use crate::palette::cartoon;
        for c in [
            cartoon::GRASS,
            cartoon::BRICK,
            cartoon::BRASS,
            cartoon::PLANK,
            cartoon::ROCK,
        ] {
            let before = srgb(c);
            let after = grade(before);
            assert!(
                saturation(after) > saturation(before),
                "{c:?}: {before} -> {after}"
            );
        }
    }

    #[test]
    fn contrast_spreads_tones_and_neutrals_stay_neutral() {
        let dark = grade(Vec3::splat(0.05));
        let light = grade(Vec3::splat(0.6));
        assert!(dark.x < 0.05 && light.y > 0.6, "{dark} {light}");
        // Split tone: shadows lean blue, highlights lean warm, only slightly.
        assert!(dark.z > dark.x && light.x > light.z);
        assert!((dark.z - dark.x) < 0.03 && (light.x - light.z) < 0.1);
    }

    #[test]
    fn nothing_clips_or_goes_negative() {
        use crate::palette::cartoon;
        for (_, c) in cartoon::ALL {
            let g = grade(srgb(*c));
            assert!(g.cmpge(Vec3::ZERO).all() && g.cmple(Vec3::ONE).all());
        }
        assert_eq!(grade(Vec3::ZERO), Vec3::ZERO);
        assert_eq!(grade(Vec3::splat(4.0)), Vec3::ONE);
    }
}
