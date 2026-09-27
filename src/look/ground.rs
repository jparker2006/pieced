//! The island's ground: toon-lit grass (the same two bands, violet shadow and
//! teal fill as [`super::ToonMaterial`], no rim) with a faint glowing build
//! grid drawn in world space by the shader, so the grid costs no geometry and
//! stays crisp at any distance (M2-SPEC → Building: "Build grid").
//!
//! The grass is painted (D48, targets T01, T09, T11): a small detail texture
//! generated here at startup ([`grass_detail_pixels`], no image files) holds
//! blade strokes, light dabs and two soft tileable noise fields. The shader
//! samples it in world space twice at blade scale (turned against each other so
//! the tile never shows) and twice at patch scale, for darker and lighter,
//! yellower patches of lawn. Mipmaps and anisotropic filtering fade the strokes
//! into the average grass colour with distance, so nothing shimmers.
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
use crate::{
    palette::cartoon,
    rng::Rng,
    shared::{ARENA_HALF, CELL_SIZE},
};
use bevy::{
    asset::{RenderAssetUsages, io::embedded::EmbeddedAssetRegistry, uuid_handle},
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{
        AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
        TextureDimension, TextureFormat,
    },
    shader::ShaderRef,
};
use std::path::{Path, PathBuf};

pub const GROUND_SHADER_PATH: &str = "embedded://pieced/shaders/ground.wgsl";

/// The painted-grass detail texture ([`grass_detail_image`]), inserted at
/// startup under this fixed handle so [`GroundMaterial::default`] can name it.
pub const GRASS_DETAIL: Handle<Image> = uuid_handle!("6b1f3c2e-9a47-4d0e-8c55-2f7e1a9b4d31");
/// Side of the detail texture's top mip (pixels).
pub const GRASS_DETAIL_SIZE: usize = 512;
/// World size (m) of one blade-stroke tile: about 170 texels per metre.
pub const GRASS_STROKE_TILE: f32 = 3.0;
/// World size (m) of one patch-noise tile (patches a few metres across).
pub const GRASS_PATCH_TILE: f32 = 26.0;

/// Toon-lit ground with the world-space build grid.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[uniform(0, GroundUniform)]
#[bind_group_data(GroundKey)]
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
    /// The painted-grass detail texture ([`GRASS_DETAIL`]); `None` draws the
    /// plain vertex-coloured grass.
    #[texture(1)]
    #[sampler(2)]
    pub detail: Option<Handle<Image>>,
    /// Colour of the darker lawn patches (the vertex colours are tinted
    /// toward it by the ratio to [`cartoon::GRASS`]) and how far (0..1).
    pub patch_dark: Color,
    pub patch_dark_amount: f32,
    /// Colour of the lighter, yellower patches and the light dabs, and how far.
    pub patch_light: Color,
    pub patch_light_amount: f32,
    /// How strongly the dark blade strokes darken the grass (0..1).
    pub stroke_strength: f32,
    /// Brightness of a full-strength blade stroke (× the grass colour).
    pub stroke_shade: f32,
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
            grid_strength: 0.26,
            glow_strength: 0.08,
            cell: CELL_SIZE,
            origin: Vec2::splat(-ARENA_HALF),
            grid_min: Vec2::splat(-ARENA_HALF),
            grid_max: Vec2::splat(ARENA_HALF),
            line_half_width: 0.016,
            glow_width: 0.12,
            fade_start: 12.0,
            fade_end: 40.0,
            detail: Some(GRASS_DETAIL),
            patch_dark: cartoon::GRASS_SHADOW,
            patch_dark_amount: 0.7,
            patch_light: cartoon::GRASS_LIGHT,
            patch_light_amount: 1.0,
            stroke_strength: 0.9,
            stroke_shade: 0.52,
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

    /// Flat vertex-coloured grass: no painted detail.
    pub fn without_detail(mut self) -> Self {
        self.detail = None;
        self
    }
}

/// Pipeline key: whether the painted detail is sampled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroundKey {
    detail: bool,
}

impl From<&GroundMaterial> for GroundKey {
    fn from(m: &GroundMaterial) -> Self {
        Self {
            detail: m.detail.is_some(),
        }
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
    /// rgb: dark patch colour ÷ grass (linear), w: amount.
    patch_dark: Vec4,
    /// rgb: light patch colour ÷ grass (linear), w: amount.
    patch_light: Vec4,
    /// x: stroke tile (m), y: patch tile (m), z: stroke strength, w: stroke shade.
    detail: Vec4,
}

/// `color` ÷ the palette grass, per linear channel: what the shader multiplies
/// the (grass-coloured) vertex colours by to reach `color`.
fn grass_ratio(color: Color) -> Vec3 {
    let c = color.to_linear();
    let g = cartoon::GRASS.to_linear();
    Vec3::new(
        c.red / g.red.max(1e-4),
        c.green / g.green.max(1e-4),
        c.blue / g.blue.max(1e-4),
    )
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
            patch_dark: grass_ratio(m.patch_dark).extend(m.patch_dark_amount),
            patch_light: grass_ratio(m.patch_light).extend(m.patch_light_amount),
            detail: Vec4::new(
                GRASS_STROKE_TILE,
                GRASS_PATCH_TILE,
                m.stroke_strength,
                m.stroke_shade,
            ),
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

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if key.bind_group_data.detail
            && let Some(fragment) = descriptor.fragment.as_mut()
        {
            fragment.shader_defs.push("GROUND_DETAIL".into());
        }
        Ok(())
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

// ---------------------------------------------------------------------------
// The painted-grass detail texture
// ---------------------------------------------------------------------------

/// Tileable smooth value noise in [0, 1): a lattice of `period` × `period`
/// cells over the unit square, wrapping at its edges.
fn tile_noise(u: f32, v: f32, period: u32, seed: u32) -> f32 {
    let hash = |x: u32, y: u32| {
        let mut h = (x % period).wrapping_mul(0x8DA6_B343)
            ^ (y % period).wrapping_mul(0xD816_3841)
            ^ seed.wrapping_mul(0xCB1A_B31F);
        h ^= h >> 13;
        h = h.wrapping_mul(0x5BD1_E995);
        h ^= h >> 15;
        (h & 0x00FF_FFFF) as f32 / 0x0100_0000 as f32
    };
    let (x, y) = (u * period as f32, v * period as f32);
    let (xi, yi) = (x.floor(), y.floor());
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (fx, fy) = (s(x - xi), s(y - yi));
    let (xi, yi) = (xi as i64 as u32, yi as i64 as u32);
    let a = hash(xi, yi) + (hash(xi + 1, yi) - hash(xi, yi)) * fx;
    let b = hash(xi, yi + 1) + (hash(xi + 1, yi + 1) - hash(xi, yi + 1)) * fx;
    a + (b - a) * fy
}

/// Three octaves of [`tile_noise`], normalised to about [0, 1).
fn tile_fbm(u: f32, v: f32, period: u32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut norm = 0.0;
    let mut amp = 1.0;
    for octave in 0..3 {
        sum += amp * tile_noise(u, v, period << octave, seed.wrapping_add(octave * 131));
        norm += amp;
        amp *= 0.5;
    }
    sum / norm
}

/// Stamps a tapered stroke (a painted blade or dab) into `buf` (side `size`,
/// wrapping at the edges so the tile is seamless): from `root` (width
/// `width` px) to `tip` (a point), anti-aliased, kept at the maximum coverage.
fn stamp(buf: &mut [f32], size: usize, root: Vec2, tip: Vec2, width: f32, strength: f32) {
    let lo = root.min(tip) - Vec2::splat(width + 2.0);
    let hi = root.max(tip) + Vec2::splat(width + 2.0);
    let axis = tip - root;
    let len2 = axis.length_squared().max(1e-6);
    for y in lo.y.floor() as i32..=hi.y.ceil() as i32 {
        for x in lo.x.floor() as i32..=hi.x.ceil() as i32 {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let t = ((p - root).dot(axis) / len2).clamp(0.0, 1.0);
            let half = 0.5 * width * (1.0 - t).powf(0.8);
            let d = p.distance(root + axis * t);
            let cover = (half + 0.5 - d).clamp(0.0, 1.0) * strength;
            if cover > 0.0 {
                let (wx, wy) = (x.rem_euclid(size as i32), y.rem_euclid(size as i32));
                let i = wy as usize * size + wx as usize;
                buf[i] = buf[i].max(cover);
            }
        }
    }
}

/// The detail texture's top mip, `size` × `size` RGBA8 (row-major, top row
/// first), tiling seamlessly on both axes:
///
/// - R: dark blade strokes, clumps of three to six tapered blades fanning up
///   out of one root, plus loose single blades (like the painted lawn marks in
///   T01 and T09);
/// - G: short light dabs (sunlit blade tips);
/// - B: soft noise for the darker patches;
/// - A: soft noise, another seed, for the lighter, yellower patches.
///
/// Deterministic: the same pixels every launch.
pub fn grass_detail_pixels(size: usize) -> Vec<[u8; 4]> {
    let mut rng = Rng::new(0x6A55_D37A);
    let px_per_m = size as f32 / GRASS_STROKE_TILE;
    let area = GRASS_STROKE_TILE * GRASS_STROKE_TILE;
    let mut dark = vec![0.0f32; size * size];
    let mut light = vec![0.0f32; size * size];
    // Blade clumps, about 5 per square metre: chunky enough (3-4 cm blades,
    // 12-22 cm clumps) to read at play distance, like the painted marks.
    for _ in 0..(5.0 * area) as usize {
        let root = Vec2::new(rng.range(0.0, size as f32), rng.range(0.0, size as f32));
        let blades = 3 + (rng.next_u64() % 4) as usize;
        let lean = rng.range(-0.5, 0.5);
        let height = rng.range(0.12, 0.22) * px_per_m;
        for b in 0..blades {
            let f = if blades > 1 {
                b as f32 / (blades - 1) as f32 - 0.5
            } else {
                0.0
            };
            let a = lean + f * rng.range(1.6, 2.2) + rng.range(-0.15, 0.15);
            let h = height * rng.range(0.65, 1.0) * (1.0 - 0.35 * f.abs());
            let tip = root + Vec2::new(a.sin(), -a.cos()) * h;
            let from = root + Vec2::new(f * height * 0.25, 0.0);
            stamp(
                &mut dark,
                size,
                from,
                tip,
                rng.range(5.0, 7.0),
                rng.range(0.8, 1.0),
            );
        }
    }
    // Loose single blades, about 6 per square metre.
    for _ in 0..(6.0 * area) as usize {
        let root = Vec2::new(rng.range(0.0, size as f32), rng.range(0.0, size as f32));
        let a = rng.range(-0.7, 0.7);
        let h = rng.range(0.06, 0.12) * px_per_m;
        let tip = root + Vec2::new(a.sin(), -a.cos()) * h;
        stamp(
            &mut dark,
            size,
            root,
            tip,
            rng.range(4.0, 5.5),
            rng.range(0.6, 0.9),
        );
    }
    // Light dabs, about 10 per square metre.
    for _ in 0..(10.0 * area) as usize {
        let root = Vec2::new(rng.range(0.0, size as f32), rng.range(0.0, size as f32));
        let a = rng.range(-0.9, 0.9);
        let h = rng.range(0.06, 0.12) * px_per_m;
        let tip = root + Vec2::new(a.sin(), -a.cos()) * h;
        stamp(
            &mut light,
            size,
            root,
            tip,
            rng.range(5.0, 7.0),
            rng.range(0.7, 1.0),
        );
    }
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    (0..size * size)
        .map(|i| {
            let (x, y) = (i % size, i / size);
            let (u, v) = (
                (x as f32 + 0.5) / size as f32,
                (y as f32 + 0.5) / size as f32,
            );
            [
                byte(dark[i]),
                byte(light[i] * (1.0 - dark[i])),
                byte(tile_fbm(u, v, 3, 17)),
                byte(tile_fbm(u, v, 4, 29)),
            ]
        })
        .collect()
}

/// Every mip level below `top` (side `size`), each the 2 × 2 box average of the
/// one above, down to 1 × 1, concatenated after the top level.
fn with_mips(top: Vec<[u8; 4]>, size: usize) -> (Vec<u8>, u32) {
    let mut data: Vec<u8> = top.iter().flatten().copied().collect();
    let mut level = top;
    let mut side = size;
    let mut count = 1;
    while side > 1 {
        let half = side / 2;
        level = (0..half * half)
            .map(|i| {
                let (x, y) = (i % half * 2, i / half * 2);
                let texel = |dx: usize, dy: usize| level[(y + dy) * side + x + dx];
                let mut out = [0u8; 4];
                for (c, o) in out.iter_mut().enumerate() {
                    let sum: u32 = [(0, 0), (1, 0), (0, 1), (1, 1)]
                        .iter()
                        .map(|&(dx, dy)| texel(dx, dy)[c] as u32)
                        .sum();
                    *o = ((sum + 2) / 4) as u8;
                }
                out
            })
            .collect();
        data.extend(level.iter().flatten());
        side = half;
        count += 1;
    }
    (data, count)
}

/// The painted-grass detail texture, with its whole mip chain, repeating and
/// anisotropically filtered (see [`grass_detail_pixels`]).
pub fn grass_detail_image() -> Image {
    let size = GRASS_DETAIL_SIZE;
    let (data, mips) = with_mips(grass_detail_pixels(size), size);
    let mut image = Image::new_uninit(
        Extent3d {
            width: size as u32,
            height: size as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = mips;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        label: Some("grass detail".into()),
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 4,
        ..default()
    });
    image
}

fn insert_grass_detail(mut images: ResMut<Assets<Image>>) {
    let _ = images.insert(&GRASS_DETAIL, grass_detail_image());
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
        .add_systems(PreStartup, insert_grass_detail)
        .add_systems(Startup, warm_ground)
        .add_systems(
            PostUpdate,
            sync_global_block::<ToonLighting, GroundMaterial>,
        );
}

/// The ground's pipeline (the builder vertex layout without outline normals,
/// with the painted detail).
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
    fn the_grass_detail_tiles_seamlessly_and_is_deterministic() {
        let size = 128;
        let a = grass_detail_pixels(size);
        assert_eq!(a, grass_detail_pixels(size), "the same pixels every launch");
        // The soft patch fields (B, A) wrap: across the seam they change no
        // more than between any two neighbouring texels.
        for c in [2, 3] {
            let step = |x0: usize, y0: usize, x1: usize, y1: usize| {
                (a[y0 * size + x0][c] as i32 - a[y1 * size + x1][c] as i32).abs()
            };
            let inner = (0..size)
                .map(|y| step(size / 2, y, size / 2 + 1, y))
                .max()
                .unwrap();
            let seam_x = (0..size).map(|y| step(size - 1, y, 0, y)).max().unwrap();
            let seam_y = (0..size).map(|x| step(x, size - 1, x, 0)).max().unwrap();
            assert!(seam_x <= inner + 3 && seam_y <= inner + 3, "channel {c}");
            // And they vary enough to make patches.
            let (lo, hi) = a
                .iter()
                .fold((255, 0), |(lo, hi), p| (lo.min(p[c]), hi.max(p[c])));
            assert!(hi - lo > 90, "channel {c}: {lo}..{hi}");
        }
    }

    #[test]
    fn the_grass_has_blade_strokes_and_light_dabs() {
        let size = GRASS_DETAIL_SIZE;
        let px = grass_detail_pixels(size);
        let share = |c: usize| px.iter().filter(|p| p[c] > 128).count() as f32 / px.len() as f32;
        let (dark, light) = (share(0), share(1));
        // Busy enough to read as painted lawn, sparse enough to stay grass.
        assert!((0.03..0.25).contains(&dark), "dark strokes cover {dark}");
        assert!((0.01..0.2).contains(&light), "light dabs cover {light}");
        // A dab never sits on a stroke.
        assert!(px.iter().all(|p| p[0] < 250 || p[1] < 10));
    }

    #[test]
    fn the_detail_mips_average_down_to_one_texel() {
        let image = grass_detail_image();
        let size = GRASS_DETAIL_SIZE;
        assert_eq!(image.texture_descriptor.mip_level_count, 10);
        let data = image.data.as_ref().unwrap();
        let texels: usize = (0..10).map(|l| (size >> l) * (size >> l)).sum();
        assert_eq!(data.len(), texels * 4);
        // The last mip is the whole tile's average (the far-away grass).
        let top = &data[..size * size * 4];
        let last = &data[data.len() - 4..];
        for c in 0..4 {
            let mean = top
                .iter()
                .skip(c)
                .step_by(4)
                .map(|&v| v as f32)
                .sum::<f32>()
                / (size * size) as f32;
            assert!((last[c] as f32 - mean).abs() < 3.0, "channel {c}");
        }
        // Far away, strokes darken the grass only slightly on average.
        assert!(last[0] < 70, "far strokes {}", last[0]);
    }

    #[test]
    fn the_default_ground_is_painted_and_patches_stay_green() {
        let m = GroundMaterial::default();
        assert_eq!(m.detail, Some(GRASS_DETAIL));
        assert!(GroundKey::from(&m).detail);
        assert!(!GroundKey::from(&m.clone().without_detail()).detail);
        let u = GroundUniform::from(&m);
        // Dark patches are darker, light patches lighter and yellower, and
        // both stay green.
        let (dark, light) = (u.patch_dark.truncate(), u.patch_light.truncate());
        assert!(dark.max_element() < 1.0, "{dark}");
        assert!(light.y > 1.0 && light.x > light.z, "{light}");
        let g = cartoon::GRASS.to_linear();
        for k in [dark, light] {
            let c = Vec3::new(g.red, g.green, g.blue) * k;
            assert!(c.y > c.x && c.y > c.z, "{c}");
        }
        assert!((0.0..1.0).contains(&m.stroke_shade));
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
