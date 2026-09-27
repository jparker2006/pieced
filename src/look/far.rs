//! The far layer: an unlit material that hazes toward one global haze color
//! with distance, for everything beyond the cartoon foreground (the station,
//! ships, far islands, the planet, distant terrain). Never outlined. Glow comes
//! from [`super::Halo`] billboards, not bloom. Opaque far models darken by
//! their baked AO ([`FAR_AO_STRENGTH`]) and everything but additive glows ends
//! with the same colour grade as the toon surfaces ([`super::grade`]).

use super::toon::GlobalBlock;
use bevy::{
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
};

pub const FAR_SHADER_PATH: &str = "embedded://pieced/shaders/far.wgsl";

/// Distance haze shared by every [`FarMaterial`]:
/// `haze = 1 - exp(-((d - start) × density)²)` past `start` meters.
/// Defaults to a violet-blue galaxy haze.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct FarHaze {
    pub color: Color,
    pub start: f32,
    pub density: f32,
}

impl Default for FarHaze {
    fn default() -> Self {
        Self {
            color: Color::srgb(0.42, 0.38, 0.72),
            start: 100.0,
            density: 0.003,
        }
    }
}

impl FarHaze {
    /// Haze fraction (0..1) at `distance` meters. Mirrors `far.wgsl`.
    pub fn amount(&self, distance: f32) -> f32 {
        let d = (distance - self.start).max(0.0) * self.density;
        1.0 - (-d * d).exp()
    }
}

/// [`FarHaze`] as the GPU sees it; managed like [`super::ToonLight`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FarHazeBlock {
    /// Linear rgb, w unused.
    pub color: Vec4,
    /// x start, y density.
    pub params: Vec4,
}

impl From<&FarHaze> for FarHazeBlock {
    fn from(h: &FarHaze) -> Self {
        let c = h.color.to_linear();
        Self {
            color: Vec4::new(c.red, c.green, c.blue, 1.0),
            params: Vec4::new(h.start, h.density, 0.0, 0.0),
        }
    }
}

impl Default for FarHazeBlock {
    fn default() -> Self {
        Self::from(&FarHaze::default())
    }
}

/// Unlit, hazed surface for the far layer. Albedo is `base_color` × the mesh's
/// vertex colors; `emissive` is added before the haze (so distant glows fade
/// into the sky too). `haze` scales the global haze for this material (0 keeps
/// it crisp, e.g. clouds or a planet that should read against the sky).
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[uniform(0, FarUniform)]
pub struct FarMaterial {
    pub base_color: Color,
    pub emissive: Color,
    pub emissive_strength: f32,
    pub haze: f32,
    pub alpha_mode: AlphaMode,
    /// Managed copy of [`FarHaze`]; don't set by hand.
    pub haze_block: FarHazeBlock,
}

impl Default for FarMaterial {
    fn default() -> Self {
        Self {
            base_color: Color::WHITE,
            emissive: Color::BLACK,
            emissive_strength: 0.0,
            haze: 1.0,
            alpha_mode: AlphaMode::Opaque,
            haze_block: FarHazeBlock::default(),
        }
    }
}

impl FarMaterial {
    pub fn new(base_color: Color) -> Self {
        Self {
            base_color,
            ..default()
        }
    }

    pub fn with_haze(mut self, haze: f32) -> Self {
        self.haze = haze;
        self
    }

    pub fn with_emissive(mut self, emissive: Color, strength: f32) -> Self {
        self.emissive = emissive;
        self.emissive_strength = strength;
        self
    }
}

/// How strongly an opaque far model's baked AO (`COLOR_0` alpha, see
/// `lib/ao.py`) darkens it: gentler than near toon surfaces, since distance
/// haze flattens everything anyway.
pub const FAR_AO_STRENGTH: f32 = 0.6;

#[derive(Clone, Copy, Default, ShaderType)]
pub struct FarUniform {
    base_color: Vec4,
    /// rgb emissive × strength, w AO strength (0: vertex alpha is opacity).
    emissive: Vec4,
    /// rgb haze color, w this material's haze amount.
    haze_color: Vec4,
    /// x start, y density, z grade amount (0 for additive glows).
    haze: Vec4,
}

impl From<&FarMaterial> for FarUniform {
    fn from(m: &FarMaterial) -> Self {
        let b = m.base_color.to_linear();
        let e = m.emissive.to_linear() * m.emissive_strength;
        let opaque = m.alpha_mode == AlphaMode::Opaque;
        let additive = matches!(
            m.alpha_mode,
            AlphaMode::Add | AlphaMode::Premultiplied | AlphaMode::Multiply
        );
        let ao = if opaque { FAR_AO_STRENGTH } else { 0.0 };
        let graded = if additive { 0.0 } else { 1.0 };
        Self {
            base_color: Vec4::new(b.red, b.green, b.blue, b.alpha),
            emissive: Vec4::new(e.red, e.green, e.blue, ao),
            haze_color: m.haze_block.color.truncate().extend(m.haze),
            haze: m
                .haze_block
                .params
                .truncate()
                .truncate()
                .extend(graded)
                .extend(0.0),
        }
    }
}

impl Material for FarMaterial {
    fn fragment_shader() -> ShaderRef {
        FAR_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }
}

impl GlobalBlock<FarHaze> for FarMaterial {
    type Block = FarHazeBlock;
    fn block_of(global: &FarHaze) -> FarHazeBlock {
        FarHazeBlock::from(global)
    }
    fn block(&self) -> FarHazeBlock {
        self.haze_block
    }
    fn set_block(&mut self, block: FarHazeBlock) {
        self.haze_block = block;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::look::toon::sync_global_block;

    #[test]
    fn haze_rises_smoothly_with_distance() {
        let haze = FarHaze::default();
        assert_eq!(haze.amount(0.0), 0.0);
        assert_eq!(haze.amount(haze.start), 0.0);
        let mut last = 0.0;
        for d in (110..900).step_by(50) {
            let a = haze.amount(d as f32);
            assert!(a > last && a < 1.0, "haze at {d} m: {a}");
            last = a;
        }
        assert!(haze.amount(900.0) > 0.95);
    }

    #[test]
    fn haze_reaches_every_far_material() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<FarMaterial>()
            .init_resource::<FarHaze>()
            .add_systems(PostUpdate, sync_global_block::<FarHaze, FarMaterial>);
        let clouds = app
            .world_mut()
            .resource_mut::<Assets<FarMaterial>>()
            .add(FarMaterial::default().with_haze(0.0));
        app.world_mut().resource_mut::<FarHaze>().color = Color::srgb(0.9, 0.8, 0.6);
        app.update();
        let expected = FarHazeBlock::from(app.world().resource::<FarHaze>());
        let material = app
            .world()
            .resource::<Assets<FarMaterial>>()
            .get(&clouds)
            .unwrap()
            .clone();
        assert_eq!(material.haze_block, expected);
        // The per-material amount survives the sync.
        assert_eq!(FarUniform::from(&material).haze_color.w, 0.0);
    }

    #[test]
    fn opaque_far_models_use_baked_ao_and_glows_stay_ungraded() {
        let solid = FarUniform::from(&FarMaterial::default());
        assert_eq!(solid.emissive.w, FAR_AO_STRENGTH);
        assert_eq!(solid.haze.z, 1.0);
        let glow = FarUniform::from(&FarMaterial {
            alpha_mode: AlphaMode::Add,
            ..default()
        });
        assert_eq!(glow.emissive.w, 0.0, "vertex alpha stays opacity");
        assert_eq!(glow.haze.z, 0.0, "additive glows are not graded");
        // Haze start and density are untouched by the packing.
        let haze = FarHazeBlock::default().params;
        assert_eq!(solid.haze.x, haze.x);
        assert_eq!(solid.haze.y, haze.y);
    }
}
