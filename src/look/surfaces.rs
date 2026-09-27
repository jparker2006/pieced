//! Surface kinds: how shiny a toon surface is (M2 Amendment B, D47).
//!
//! `art/surfaces.json` names a few kinds (brass, steel, crystal, glass, satin)
//! and tags palette colours with them. Every [`super::ToonMaterial`] carries
//! the whole table (it is part of the global [`super::ToonLight`] block), and
//! the shader looks each fragment's albedo (base × vertex colour, before any
//! detail texture) up in it: a face painted `brass` gets the brass highlight
//! on any model, Blender-built or not, and the shared vertex-coloured material
//! still batches. Colours not in the table use the material's own
//! [`Surface`] (matte unless set with `ToonMaterial::with_surface`).

use bevy::prelude::*;
use serde::Deserialize;
use std::{collections::BTreeMap, sync::LazyLock};

/// Most tagged palette colours the shader looks up.
pub const MAX_SURFACES: usize = 16;
/// Two colours match when every linear channel is within this.
pub const SURFACE_MATCH: f32 = 0.002;

/// A surface's cartoon highlight.
#[derive(Debug, Clone, Copy, PartialEq, Default, Deserialize)]
pub struct Surface {
    /// Strength of the crisp highlight blob (0 = matte).
    pub specular: f32,
    /// Blinn-Phong exponent: bigger is a smaller, crisper blob (the blob is
    /// where N·H^shininess > 0.5).
    pub shininess: f32,
    /// Broad soft gloss under the blob (a fraction of `specular`).
    pub sheen: f32,
    /// How much the highlight takes the surface's own hue (metals), 0..1.
    pub tint: f32,
}

impl Surface {
    /// No highlight.
    pub const MATTE: Self = Self {
        specular: 0.0,
        shininess: 1.0,
        sheen: 0.0,
        tint: 0.0,
    };

    pub const fn new(specular: f32, shininess: f32) -> Self {
        Self {
            specular,
            shininess,
            sheen: 0.0,
            tint: 0.0,
        }
    }

    /// A kind from `art/surfaces.json` (`"brass"`, `"steel"`, `"crystal"`,
    /// `"glass"`, `"satin"`).
    pub fn named(kind: &str) -> Option<Self> {
        SURFACES.kinds.get(kind).copied()
    }

    pub(crate) fn to_vec4(self) -> Vec4 {
        Vec4::new(
            self.specular,
            self.shininess.max(1.0),
            self.sheen,
            self.tint,
        )
    }
}

#[derive(Deserialize)]
struct SurfacesFile {
    kinds: BTreeMap<String, Surface>,
    palette: BTreeMap<String, String>,
}

/// The parsed `art/surfaces.json`.
pub struct Surfaces {
    pub kinds: BTreeMap<String, Surface>,
    /// (palette name, its linear colour, its surface), sorted by name.
    pub tagged: Vec<(String, LinearRgba, Surface)>,
}

pub static SURFACES: LazyLock<Surfaces> = LazyLock::new(|| {
    let file: SurfacesFile = serde_json::from_str(include_str!("../../art/surfaces.json"))
        .expect("art/surfaces.json parses");
    let tagged = file
        .palette
        .iter()
        .map(|(name, kind)| {
            let color = crate::palette::cartoon::by_name(name)
                .unwrap_or_else(|| panic!("art/surfaces.json: '{name}' is not a palette colour"));
            let surface = *file
                .kinds
                .get(kind)
                .unwrap_or_else(|| panic!("art/surfaces.json: '{name}' has unknown kind '{kind}'"));
            (name.clone(), color.to_linear(), surface)
        })
        .collect::<Vec<_>>();
    assert!(
        tagged.len() <= MAX_SURFACES,
        "art/surfaces.json tags {} colours; the shader looks up at most {MAX_SURFACES}",
        tagged.len()
    );
    Surfaces {
        kinds: file.kinds,
        tagged,
    }
});

/// The table as the shader sees it: keys (linear rgb, w 1) and params
/// (specular, shininess, sheen, tint), plus the count in use.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceTable {
    pub keys: [Vec4; MAX_SURFACES],
    pub params: [Vec4; MAX_SURFACES],
    pub count: u32,
}

impl Default for SurfaceTable {
    fn default() -> Self {
        let mut table = Self {
            keys: [Vec4::ZERO; MAX_SURFACES],
            params: [Vec4::ZERO; MAX_SURFACES],
            count: 0,
        };
        for (i, (_, color, surface)) in SURFACES.tagged.iter().enumerate() {
            table.keys[i] = Vec4::new(color.red, color.green, color.blue, 1.0);
            table.params[i] = surface.to_vec4();
            table.count = i as u32 + 1;
        }
        table
    }
}

impl SurfaceTable {
    /// The surface for a linear albedo: the tagged palette colour it matches,
    /// or `fallback`. Mirrors the shader's lookup.
    pub fn lookup(&self, albedo: Vec3, fallback: Vec4) -> Vec4 {
        (0..self.count as usize)
            .find(|&i| (albedo - self.keys[i].truncate()).abs().max_element() < SURFACE_MATCH)
            .map_or(fallback, |i| self.params[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::cartoon;

    fn linear(c: Color) -> Vec3 {
        let l = c.to_linear();
        Vec3::new(l.red, l.green, l.blue)
    }

    #[test]
    fn metal_and_crystal_are_shiny_and_wood_grass_cloth_are_not() {
        let table = SurfaceTable::default();
        let matte = Surface::MATTE.to_vec4();
        let spec = |c: Color| table.lookup(linear(c), matte).x;
        for shiny in [
            cartoon::BRASS,
            cartoon::GOLD_RINGS,
            cartoon::KNIGHT_STEEL,
            cartoon::GUN_IRON,
            cartoon::CRYSTAL_BLUE,
            cartoon::CRYSTAL_VIOLET,
        ] {
            assert!(spec(shiny) > 0.5, "{shiny:?}");
        }
        for barely in [cartoon::KNIGHT_PURPLE, cartoon::GLOVE_WHITE] {
            let s = spec(barely);
            assert!(s > 0.0 && s < 0.3, "{barely:?}: {s}");
        }
        for matte_colour in [
            cartoon::GRASS,
            cartoon::PLANK,
            cartoon::BRICK,
            cartoon::STOCK,
            cartoon::ROCK,
            cartoon::FOLIAGE,
        ] {
            assert_eq!(spec(matte_colour), 0.0, "{matte_colour:?}");
        }
        // Crystal glints are crisper than brass.
        let shininess = |c: Color| table.lookup(linear(c), matte).y;
        assert!(shininess(cartoon::CRYSTAL_BLUE) > shininess(cartoon::BRASS));
    }

    #[test]
    fn no_untagged_palette_colour_is_mistaken_for_a_tagged_one() {
        let table = SurfaceTable::default();
        let tagged: Vec<&str> = SURFACES.tagged.iter().map(|(n, ..)| n.as_str()).collect();
        let sentinel = Vec4::new(-1.0, 0.0, 0.0, 0.0);
        for (name, color) in cartoon::ALL {
            let found = table.lookup(linear(*color), sentinel);
            if tagged.contains(name) {
                assert_ne!(found, sentinel, "{name} should be found");
            } else {
                assert_eq!(found, sentinel, "{name} collides with a tagged colour");
            }
        }
        assert_eq!(table.count as usize, SURFACES.tagged.len());
        assert!(Surface::named("brass").is_some() && Surface::named("velvet").is_none());
    }

    #[test]
    fn a_16_bit_vertex_colour_still_matches() {
        // Blender exports COLOR_0 as unorm16: the key must survive that rounding.
        let table = SurfaceTable::default();
        let c = linear(cartoon::BRASS);
        let q = (c * 65535.0).round() / 65535.0 + Vec3::splat(0.4 / 65535.0);
        assert!(table.lookup(q, Vec4::ZERO).x > 0.5);
    }
}
