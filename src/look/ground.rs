//! The island's ground: toon-lit grass (the same two bands, violet shadow and
//! teal fill as [`super::ToonMaterial`], no rim) with a faint glowing build
//! grid drawn in world space by the shader, so the grid costs no geometry and
//! stays crisp at any distance (M2-SPEC → Building: "Build grid").
//!
//! The grid's lines run every [`GroundMaterial::cell`] metres from
//! [`GroundMaterial::origin`], only inside the build area
//! ([`GroundMaterial::grid_min`]..[`GroundMaterial::grid_max`]). They are
//! anti-aliased with screen-space derivatives: a line thinner than a pixel
//! dims instead of shimmering, and the grid fades out with distance from the
//! camera. [`grid_line_intensity`] mirrors the shader's line math on the CPU.
//!
//! Put it on a mesh with normals and (optionally) `COLOR_0` vertex colours,
//! which multiply the base colour (grass patches).

use super::{
    ToonLight, ToonLighting,
    toon::{GlobalBlock, sync_global_block},
    warmup::Warmup,
};
use crate::shared::{ARENA_HALF, CELL_SIZE};
use bevy::{
    asset::io::embedded::EmbeddedAssetRegistry,
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
};
use std::path::{Path, PathBuf};

pub const GROUND_SHADER_PATH: &str = "embedded://pieced/shaders/ground.wgsl";

/// Toon-lit ground with the world-space build grid.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[uniform(0, GroundUniform)]
pub struct GroundMaterial {
    /// Multiplies the mesh's vertex colours.
    pub base_color: Color,
    /// The grid lines' colour.
    pub grid_color: Color,
    /// How strongly lines replace the grass near the camera (0 = no grid).
    pub grid_strength: f32,
    /// Extra additive glow around each line.
    pub glow_strength: f32,
    /// Grid pitch (m).
    pub cell: f32,
    /// A point where two grid lines cross (XZ).
    pub origin: Vec2,
    /// Lines are drawn only inside this XZ rectangle.
    pub grid_min: Vec2,
    pub grid_max: Vec2,
    /// Half-width of a line's bright core (m).
    pub line_half_width: f32,
    /// Falloff distance of the glow around a line (m).
    pub glow_width: f32,
    /// Lines fade out between these distances from the camera (m).
    pub fade_start: f32,
    pub fade_end: f32,
    /// Managed copy of [`ToonLighting`]; don't set by hand.
    pub lighting: ToonLight,
}

impl Default for GroundMaterial {
    /// The build grid over the arena: 4 m cells aligned with the build grid,
    /// in the palette's pale grid-line colour.
    fn default() -> Self {
        Self {
            base_color: Color::WHITE,
            grid_color: crate::palette::cartoon::GRID_LINE,
            grid_strength: 0.5,
            glow_strength: 0.13,
            cell: CELL_SIZE,
            origin: Vec2::splat(-ARENA_HALF),
            grid_min: Vec2::splat(-ARENA_HALF),
            grid_max: Vec2::splat(ARENA_HALF),
            line_half_width: 0.024,
            glow_width: 0.16,
            fade_start: 22.0,
            fade_end: 52.0,
            lighting: ToonLight::default(),
        }
    }
}

impl GroundMaterial {
    /// Plain toon-lit ground without a grid (the island margin, if wanted).
    pub fn without_grid(mut self) -> Self {
        self.grid_strength = 0.0;
        self.glow_strength = 0.0;
        self
    }
}

#[derive(Clone, Copy, Default, ShaderType)]
pub struct GroundUniform {
    base_color: Vec4,
    /// rgb: line colour, w: line strength.
    grid_color: Vec4,
    key_direction: Vec4,
    key_color: Vec4,
    shadow_tint: Vec4,
    fill_direction: Vec4,
    fill_color: Vec4,
    /// x: cell, yz: origin, w: line half-width.
    grid: Vec4,
    /// xy: min, zw: max.
    bounds: Vec4,
    /// x: glow width, y: glow strength, z: fade start, w: fade end.
    params: Vec4,
}

impl From<&GroundMaterial> for GroundUniform {
    fn from(m: &GroundMaterial) -> Self {
        let base = m.base_color.to_linear();
        let grid = m.grid_color.to_linear();
        let l = &m.lighting;
        Self {
            base_color: Vec4::new(base.red, base.green, base.blue, 1.0),
            grid_color: Vec4::new(grid.red, grid.green, grid.blue, m.grid_strength),
            key_direction: l.key_direction,
            key_color: l.key_color,
            shadow_tint: l.shadow_tint,
            fill_direction: l.fill_direction,
            fill_color: l.fill_color,
            grid: Vec4::new(m.cell, m.origin.x, m.origin.y, m.line_half_width),
            bounds: Vec4::new(m.grid_min.x, m.grid_min.y, m.grid_max.x, m.grid_max.y),
            params: Vec4::new(m.glow_width, m.glow_strength, m.fade_start, m.fade_end),
        }
    }
}

impl Material for GroundMaterial {
    fn fragment_shader() -> ShaderRef {
        GROUND_SHADER_PATH.into()
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }
}

impl GlobalBlock<ToonLighting> for GroundMaterial {
    type Block = ToonLight;
    fn block_of(global: &ToonLighting) -> ToonLight {
        ToonLight::from(global)
    }
    fn block(&self) -> ToonLight {
        self.lighting
    }
    fn set_block(&mut self, block: ToonLight) {
        self.lighting = block;
    }
}

/// Distance (m) from `coord` to the nearest grid line of pitch `cell`.
pub fn grid_line_distance(coord: f32, cell: f32) -> f32 {
    let f = coord / cell;
    (f - f.round()).abs() * cell
}

/// CPU mirror of the shader's line coverage (0..1) at a ground point, for a
/// pixel footprint of `pixel` metres, before the distance fade: the bright
/// core only (no glow).
pub fn grid_line_intensity(m: &GroundMaterial, point: Vec2, pixel: f32) -> f32 {
    let inside = point.cmpge(m.grid_min - m.line_half_width).all()
        && point.cmple(m.grid_max + m.line_half_width).all();
    if !inside || m.grid_strength <= 0.0 {
        return 0.0;
    }
    let p = point - m.origin;
    let d = grid_line_distance(p.x, m.cell).min(grid_line_distance(p.y, m.cell));
    let w = m.line_half_width.max(pixel);
    let coverage = 1.0 - smoothstep(w - pixel * 0.5, w + pixel * 0.5, d);
    coverage * (m.line_half_width / w)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0).max(1e-6)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Registers the ground material, its shader, the lighting sync and its
/// warm-up draw. Called from `LookPlugin`.
pub(super) fn add_ground(app: &mut App) {
    app.world()
        .resource::<EmbeddedAssetRegistry>()
        .insert_asset(
            PathBuf::new(),
            Path::new("pieced/shaders/ground.wgsl"),
            include_bytes!("../../assets/shaders/ground.wgsl").as_slice(),
        );
    app.add_plugins(MaterialPlugin::<GroundMaterial>::default())
        .add_systems(Startup, warm_ground)
        .add_systems(
            PostUpdate,
            sync_global_block::<ToonLighting, GroundMaterial>,
        );
}

/// The ground's pipeline (the builder vertex layout without outline normals).
fn warm_ground(
    mut warmup: Warmup,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GroundMaterial>>,
) {
    let mesh = meshes.add(super::builder_layout_cube(false));
    warmup.add(mesh, materials.add(GroundMaterial::default()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_follow_the_build_grid_inside_the_arena_only() {
        let m = GroundMaterial::default();
        let px = 0.005;
        // On grid lines (every 4 m from the arena corner), including its edges.
        for p in [
            Vec2::new(-ARENA_HALF, 3.0),
            Vec2::new(0.0, 1.3),
            Vec2::new(4.0, -9.1),
            Vec2::new(7.3, 8.0),
            Vec2::new(ARENA_HALF, ARENA_HALF),
        ] {
            assert!(grid_line_intensity(&m, p, px) > 0.9, "on a line at {p}");
        }
        // Mid-cell there is no line.
        for p in [Vec2::new(2.0, 2.0), Vec2::new(-10.0, 14.0)] {
            assert_eq!(grid_line_intensity(&m, p, px), 0.0, "{p}");
        }
        // Outside the build area there is none either.
        assert_eq!(grid_line_intensity(&m, Vec2::new(28.0, 0.0), px), 0.0);
        assert_eq!(
            grid_line_intensity(&m, Vec2::new(0.0, -ARENA_HALF - 3.0), px),
            0.0
        );
        // Without a grid, nothing.
        let plain = GroundMaterial::default().without_grid();
        assert_eq!(grid_line_intensity(&plain, Vec2::new(0.0, 1.0), px), 0.0);
    }

    #[test]
    fn far_lines_dim_instead_of_aliasing() {
        let m = GroundMaterial::default();
        let on = Vec2::new(4.0, 1.0);
        let near = grid_line_intensity(&m, on, 0.004);
        // Far away a pixel covers several centimetres: the line is dimmer but
        // never brighter, and never vanishes abruptly.
        let far = grid_line_intensity(&m, on, 0.2);
        assert!(far < near && far > 0.05, "near {near}, far {far}");
        assert!((grid_line_distance(5.9, 4.0) - 1.9).abs() < 1e-5);
        assert!((grid_line_distance(-24.0 + 8.02, 4.0) - 0.02).abs() < 1e-4);
    }

    #[test]
    fn lighting_reaches_the_ground_material() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<GroundMaterial>()
            .insert_resource(ToonLighting {
                key_color: Color::srgb(1.0, 0.5, 0.2),
                ..default()
            })
            .add_systems(
                PostUpdate,
                sync_global_block::<ToonLighting, GroundMaterial>,
            );
        let handle = app
            .world_mut()
            .resource_mut::<Assets<GroundMaterial>>()
            .add(GroundMaterial::default());
        app.update();
        app.update();
        let expected = ToonLight::from(app.world().resource::<ToonLighting>());
        let lighting = app
            .world()
            .resource::<Assets<GroundMaterial>>()
            .get(&handle)
            .unwrap()
            .lighting;
        assert_eq!(lighting, expected);
    }
}
