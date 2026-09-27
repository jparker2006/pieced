//! The toon material (M2 Amendment B, D47): three soft cartoon tones against
//! one key light (a warm lit tone, a mid tone and a cool violet shadow tone,
//! never black), fill from the sky colours plus a teal fill from the galaxy
//! side, a crisp cartoon highlight on shiny surfaces ([`Surface`]), a rim
//! light, baked ambient occlusion from the mesh, an emissive channel and the
//! colour grade ([`super::grade`]). No PBR light loops and no textures needed:
//! `toon.wgsl` reads only its own uniform, which carries a copy of the global
//! [`ToonLighting`] (including the palette's surface table).
//!
//! [`toon_shade`] is a CPU mirror of the shader's colour math, used by tests and
//! available to tooling that wants to predict an on-screen colour.

use super::{
    grade::{grade, smoothstep},
    surfaces::{MAX_SURFACES, Surface, SurfaceTable},
};
use bevy::{
    asset::AssetEvent,
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{
        AsBindGroup, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};

pub const TOON_SHADER_PATH: &str = "embedded://pieced/shaders/toon.wgsl";

/// The camera-fixed highlight light, in view space (x right, y up, z toward
/// the viewer): high above, a little behind and right of the eye, so gleams
/// sit on the upper-front faces of cylinders and edges, as painted. `toon.wgsl` builds
/// it from the view's axes; keep them in step.
pub const VIEW_HIGHLIGHT: Vec3 = Vec3::new(0.3, 0.9, 0.3);

/// The direction the highlight's light comes from, for a camera whose right,
/// up and back axes are the columns of `camera` (world space).
pub fn highlight_light(light: &ToonLight, camera: Mat3) -> Vec3 {
    let rig = (camera * VIEW_HIGHLIGHT).normalize();
    light
        .key_direction
        .truncate()
        .lerp(rig, light.bands.z)
        .normalize_or(rig)
}

/// Unit vector toward a light, from an azimuth (degrees clockwise from north,
/// -Z, toward east, +X) and an elevation (degrees above the horizon).
pub fn light_direction(azimuth_deg: f32, elevation_deg: f32) -> Vec3 {
    let (az, el) = (azimuth_deg.to_radians(), elevation_deg.to_radians());
    Vec3::new(az.sin() * el.cos(), el.sin(), -az.cos() * el.cos()).normalize()
}

/// The one global light rig every toon surface agrees on. Change it and a
/// system copies it into every [`ToonMaterial`] (and the ground's material;
/// the far layer's haze has its own [`super::FarHaze`]).
///
/// Defaults follow the M2 targets: a warm key light from the station side
/// (east, from the spawn view), violet shadows and a teal fill from the galaxy
/// side (north-west), a violet-blue sky fill from above and a green bounce
/// from below. The key sits slightly behind the spawn view (azimuth 115°), so
/// faces turned toward a player at spawn read lit, as in T01 and T09.
///
/// The tones: N·L below `band_threshold` is the shadow tone, between it and
/// `lit_threshold` the mid tone, above it the lit tone; each edge is a soft
/// smoothstep `band_softness` wide.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct ToonLighting {
    /// Unit vector from a surface toward the key light.
    pub key_direction: Vec3,
    /// Multiplies albedo in the lit tone. White lands palette colours as authored
    /// (before the grade).
    pub key_color: Color,
    /// Multiplies albedo in the mid tone.
    pub mid_tint: Color,
    /// Multiplies albedo in the shadow tone.
    pub shadow_tint: Color,
    /// Unit vector toward the fill light (lights the shadow and mid tones).
    pub fill_direction: Vec3,
    /// Added to the shadow tone as albedo × fill × max(N·F, 0) (half in the mid).
    pub fill_color: Color,
    /// Hemisphere fill from above (the galaxy sky), added like the fill.
    pub sky_fill: Color,
    /// Hemisphere fill from below (the grass bounce).
    pub ground_fill: Color,
    pub rim_color: Color,
    /// Rim brightness at a grazing angle (each material scales it).
    pub rim_strength: f32,
    /// Rim falloff exponent: higher is thinner.
    pub rim_power: f32,
    /// N·L where the shadow tone meets the mid tone.
    pub band_threshold: f32,
    /// N·L where the mid tone meets the lit tone.
    pub lit_threshold: f32,
    /// Half-width of each tone edge in N·L (soft, but clearly cartoon;
    /// screen-space derivatives widen it where needed).
    pub band_softness: f32,
    /// A gentle N·L ramp inside the tones (painted gradients on curved parts).
    pub band_gradient: f32,
    /// Colour of the cartoon highlight (metals tint it toward their own hue).
    pub highlight_color: Color,
    /// Where the highlight's light comes from: 0 the key light, 1 a light fixed
    /// to the camera (above, behind and right of the eye: [`VIEW_HIGHLIGHT`]).
    /// Cartoon gleams sit on the top-front edges from any view, as painted,
    /// while diffuse shading still follows the key.
    pub highlight_view_bias: f32,
    /// Edge softness of the highlight blob (in N·H^shininess units).
    pub highlight_softness: f32,
    /// How strongly baked AO (`COLOR_0` alpha) darkens (0 off, 1 full).
    pub ao_strength: f32,
    /// What a fully occluded surface is multiplied by (violet, never black).
    pub ao_tint: Color,
    /// The palette's surface kinds (`art/surfaces.json`).
    pub surfaces: SurfaceTable,
}

impl Default for ToonLighting {
    fn default() -> Self {
        Self {
            key_direction: light_direction(115.0, 45.0),
            key_color: Color::srgb(1.0, 0.97, 0.9),
            mid_tint: Color::srgb(0.86, 0.84, 0.9),
            shadow_tint: Color::srgb(0.6, 0.55, 0.74),
            fill_direction: light_direction(300.0, 35.0),
            fill_color: Color::srgb(0.18, 0.36, 0.4),
            sky_fill: Color::linear_rgb(0.045, 0.045, 0.08),
            ground_fill: Color::linear_rgb(0.035, 0.05, 0.02),
            rim_color: Color::srgb(1.0, 0.93, 0.8),
            rim_strength: 0.5,
            rim_power: 5.0,
            band_threshold: -0.05,
            lit_threshold: 0.38,
            band_softness: 0.06,
            band_gradient: 0.12,
            highlight_color: Color::srgb(1.0, 0.98, 0.9),
            highlight_view_bias: 0.6,
            highlight_softness: 0.1,
            ao_strength: 0.85,
            ao_tint: Color::linear_rgb(0.3, 0.24, 0.42),
            surfaces: SurfaceTable::default(),
        }
    }
}

/// [`ToonLighting`] as the GPU sees it (linear colours, packed). Every
/// [`ToonMaterial`] (and the ground's material) carries one; the sync system
/// keeps it current.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToonLight {
    /// xyz toward the key, w the shadow/mid threshold.
    pub key_direction: Vec4,
    /// rgb lit tone, w band softness.
    pub key_color: Vec4,
    /// rgb shadow tone.
    pub shadow_tint: Vec4,
    pub fill_direction: Vec4,
    pub fill_color: Vec4,
    /// rgb rim color, w rim strength.
    pub rim: Vec4,
    pub rim_power: f32,
    /// rgb mid tone.
    pub mid_tint: Vec4,
    pub sky_fill: Vec4,
    pub ground_fill: Vec4,
    /// rgb full-occlusion multiplier, w AO strength.
    pub ao_tint: Vec4,
    /// rgb highlight colour, w highlight softness.
    pub highlight: Vec4,
    /// x the mid/lit threshold, y the in-tone gradient, z the highlight's
    /// view bias.
    pub bands: Vec4,
    pub surfaces: SurfaceTable,
}

fn rgb(color: Color) -> Vec4 {
    let c = color.to_linear();
    Vec4::new(c.red, c.green, c.blue, 1.0)
}

impl From<&ToonLighting> for ToonLight {
    fn from(l: &ToonLighting) -> Self {
        Self {
            key_direction: l
                .key_direction
                .normalize_or(Vec3::Y)
                .extend(l.band_threshold),
            key_color: rgb(l.key_color).truncate().extend(l.band_softness),
            shadow_tint: rgb(l.shadow_tint),
            fill_direction: l.fill_direction.normalize_or(Vec3::Y).extend(0.0),
            fill_color: rgb(l.fill_color),
            rim: rgb(l.rim_color).truncate().extend(l.rim_strength),
            rim_power: l.rim_power,
            mid_tint: rgb(l.mid_tint),
            sky_fill: rgb(l.sky_fill),
            ground_fill: rgb(l.ground_fill),
            ao_tint: rgb(l.ao_tint).truncate().extend(l.ao_strength),
            highlight: rgb(l.highlight_color)
                .truncate()
                .extend(l.highlight_softness),
            bands: Vec4::new(l.lit_threshold, l.band_gradient, l.highlight_view_bias, 0.0),
            surfaces: l.surfaces,
        }
    }
}

impl Default for ToonLight {
    fn default() -> Self {
        Self::from(&ToonLighting::default())
    }
}

/// What the alpha of a mesh's `COLOR_0` means to a [`ToonMaterial`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VertexAlpha {
    /// Opaque materials read it as baked AO (every Blender model carries it);
    /// blended and additive ones as opacity (building ghosts, the edit grid).
    #[default]
    Auto,
    /// Always baked AO (a see-through Blender part keeps its opacity from
    /// `base_color` and darkens in its crevices).
    Occlusion,
    /// Always opacity (multiplies `base_color`'s alpha); no AO.
    Opacity,
}

/// Our cartoon surface material. Use it for everything near: the world, pieces,
/// props, characters and the viewmodel.
///
/// - The final albedo is `base_color` × the mesh's `COLOR_0` vertex color (when
///   the mesh has one: the mesh pipeline specializes on the vertex layout and
///   sets `VERTEX_COLORS`) × the optional detail texture.
/// - `COLOR_0`'s alpha is baked AO (see [`VertexAlpha`]).
/// - The highlight comes from the palette's surface table when the albedo is a
///   tagged palette colour, otherwise from `surface`.
/// - `AlphaMode::Opaque`, `Blend` (ghosts) and `Add` are supported.
/// - Leave `lighting` at its default; the global [`ToonLighting`] overwrites it.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[uniform(0, ToonUniform)]
#[bind_group_data(ToonKey)]
pub struct ToonMaterial {
    pub base_color: Color,
    /// Added after lighting: emissive × `emissive_strength` (crystals, glass,
    /// spells, ghosts).
    pub emissive: Color,
    pub emissive_strength: f32,
    /// Multiplier on the global rim strength (0 = no rim).
    pub rim: f32,
    /// The highlight for albedos the palette's surface table doesn't tag.
    pub surface: Surface,
    /// Multiplier on the global AO strength (0 ignores the baked AO).
    pub occlusion: f32,
    pub vertex_alpha: VertexAlpha,
    /// Optional detail texture (mortar lines, plank grain), multiplied in using
    /// the mesh's UV_0 × `detail_scale`. Meshes without UVs ignore it.
    #[texture(1)]
    #[sampler(2)]
    pub detail: Option<Handle<Image>>,
    pub detail_scale: Vec2,
    /// 0 = texture ignored, 1 = fully multiplied in.
    pub detail_strength: f32,
    pub alpha_mode: AlphaMode,
    /// `Some(Face::Back)` normally; `None` for thin double-sided sheets.
    pub cull_mode: Option<Face>,
    pub depth_bias: f32,
    /// Managed copy of [`ToonLighting`]; don't set by hand.
    pub lighting: ToonLight,
}

impl Default for ToonMaterial {
    fn default() -> Self {
        Self {
            base_color: Color::WHITE,
            emissive: Color::BLACK,
            emissive_strength: 0.0,
            rim: 1.0,
            surface: Surface::MATTE,
            occlusion: 1.0,
            vertex_alpha: VertexAlpha::Auto,
            detail: None,
            detail_scale: Vec2::ONE,
            detail_strength: 1.0,
            alpha_mode: AlphaMode::Opaque,
            cull_mode: Some(Face::Back),
            depth_bias: 0.0,
            lighting: ToonLight::default(),
        }
    }
}

impl ToonMaterial {
    /// A solid-colored toon surface (vertex colors, if any, multiply in).
    pub fn new(base_color: Color) -> Self {
        Self {
            base_color,
            ..default()
        }
    }

    /// White base: the mesh's vertex colors are the whole palette.
    pub fn vertex_colored() -> Self {
        Self::default()
    }

    pub fn with_emissive(mut self, emissive: Color, strength: f32) -> Self {
        self.emissive = emissive;
        self.emissive_strength = strength;
        self
    }

    pub fn with_alpha(mut self, alpha_mode: AlphaMode) -> Self {
        self.alpha_mode = alpha_mode;
        self
    }

    pub fn with_rim(mut self, rim: f32) -> Self {
        self.rim = rim;
        self
    }

    /// The highlight for albedos the palette doesn't tag (e.g.
    /// `Surface::named("crystal").unwrap()`).
    pub fn with_surface(mut self, surface: Surface) -> Self {
        self.surface = surface;
        self
    }

    /// Shorthand for a plain highlight: strength and Blinn-Phong exponent.
    pub fn with_specular(mut self, specular: f32, shininess: f32) -> Self {
        self.surface = Surface {
            specular,
            shininess,
            ..self.surface
        };
        self
    }

    /// Scales the baked AO's effect (0 ignores it: eyes deep in a visor).
    pub fn with_occlusion(mut self, occlusion: f32) -> Self {
        self.occlusion = occlusion;
        self
    }

    pub fn with_vertex_alpha(mut self, vertex_alpha: VertexAlpha) -> Self {
        self.vertex_alpha = vertex_alpha;
        self
    }

    pub fn double_sided(mut self) -> Self {
        self.cull_mode = None;
        self
    }

    pub fn with_detail(mut self, texture: Handle<Image>, scale: Vec2, strength: f32) -> Self {
        self.detail = Some(texture);
        self.detail_scale = scale;
        self.detail_strength = strength;
        self
    }

    /// Whether `COLOR_0`'s alpha is opacity (else baked AO) for this material.
    pub fn vertex_alpha_is_opacity(&self) -> bool {
        match self.vertex_alpha {
            VertexAlpha::Auto => self.alpha_mode != AlphaMode::Opaque,
            VertexAlpha::Occlusion => false,
            VertexAlpha::Opacity => true,
        }
    }
}

/// Pipeline key: what changes the compiled shader or pipeline state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ToonKey {
    cull_mode: Option<Face>,
    detail: bool,
    additive: bool,
    vertex_opacity: bool,
}

impl From<&ToonMaterial> for ToonKey {
    fn from(m: &ToonMaterial) -> Self {
        Self {
            cull_mode: m.cull_mode,
            detail: m.detail.is_some(),
            additive: m.alpha_mode == AlphaMode::Add,
            vertex_opacity: m.vertex_alpha_is_opacity(),
        }
    }
}

#[derive(Clone, Copy, ShaderType)]
pub struct ToonUniform {
    base_color: Vec4,
    /// rgb: emissive × strength; w: AO strength (global × material).
    emissive: Vec4,
    key_direction: Vec4,
    key_color: Vec4,
    shadow_tint: Vec4,
    fill_direction: Vec4,
    fill_color: Vec4,
    rim_color: Vec4,
    /// x: rim power, y: detail strength, zw: detail UV scale.
    params: Vec4,
    /// rgb: mid tone; w: surface table size.
    mid_tint: Vec4,
    sky_fill: Vec4,
    ground_fill: Vec4,
    ao_tint: Vec4,
    /// rgb: highlight colour; w: highlight softness.
    highlight: Vec4,
    /// x: mid/lit threshold, y: in-tone gradient, z: highlight view bias.
    bands: Vec4,
    /// This material's own surface: specular, shininess, sheen, tint.
    surface: Vec4,
    surface_keys: [Vec4; MAX_SURFACES],
    surface_params: [Vec4; MAX_SURFACES],
}

impl Default for ToonUniform {
    fn default() -> Self {
        Self::from(&ToonMaterial::default())
    }
}

impl From<&ToonMaterial> for ToonUniform {
    fn from(m: &ToonMaterial) -> Self {
        let base = m.base_color.to_linear();
        let emissive = m.emissive.to_linear() * m.emissive_strength;
        let l = &m.lighting;
        let ao = if m.vertex_alpha_is_opacity() {
            0.0
        } else {
            (l.ao_tint.w * m.occlusion).clamp(0.0, 1.0)
        };
        Self {
            base_color: Vec4::new(base.red, base.green, base.blue, base.alpha),
            emissive: Vec4::new(emissive.red, emissive.green, emissive.blue, ao),
            key_direction: l.key_direction,
            key_color: l.key_color,
            shadow_tint: l.shadow_tint,
            fill_direction: l.fill_direction,
            fill_color: l.fill_color,
            rim_color: l.rim.truncate().extend(l.rim.w * m.rim),
            params: Vec4::new(
                l.rim_power,
                m.detail_strength,
                m.detail_scale.x,
                m.detail_scale.y,
            ),
            mid_tint: l.mid_tint.truncate().extend(l.surfaces.count as f32),
            sky_fill: l.sky_fill,
            ground_fill: l.ground_fill,
            ao_tint: l.ao_tint,
            highlight: l.highlight,
            bands: l.bands,
            surface: m.surface.to_vec4(),
            surface_keys: l.surfaces.keys,
            surface_params: l.surfaces.params,
        }
    }
}

impl Material for ToonMaterial {
    fn fragment_shader() -> ShaderRef {
        TOON_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    fn depth_bias(&self) -> f32 {
        self.depth_bias
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
        descriptor.primitive.cull_mode = key.bind_group_data.cull_mode;
        if let Some(fragment) = descriptor.fragment.as_mut() {
            if key.bind_group_data.detail {
                fragment.shader_defs.push("TOON_DETAIL".into());
            }
            if key.bind_group_data.additive {
                fragment.shader_defs.push("TOON_ADDITIVE".into());
            }
            if key.bind_group_data.vertex_opacity {
                fragment.shader_defs.push("TOON_VERTEX_OPACITY".into());
            }
        }
        Ok(())
    }
}

/// A global block that a system copies into every asset of one material type
/// whenever the global changes (or an asset is added or replaced).
pub(crate) trait GlobalBlock<G: Resource>: Asset {
    type Block: PartialEq + Copy + Send + Sync + 'static;
    fn block_of(global: &G) -> Self::Block;
    fn block(&self) -> Self::Block;
    fn set_block(&mut self, block: Self::Block);
}

impl GlobalBlock<ToonLighting> for ToonMaterial {
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

/// Copies the global `G` into every `M` that disagrees with it. Only touches
/// (and so only re-uploads) materials whose block actually differs.
pub(crate) fn sync_global_block<G: Resource, M: GlobalBlock<G>>(
    global: Res<G>,
    mut events: MessageReader<AssetEvent<M>>,
    mut assets: ResMut<Assets<M>>,
) {
    let block = M::block_of(&global);
    let stale: Vec<AssetId<M>> = if global.is_changed() {
        events.clear();
        assets
            .iter()
            .filter(|(_, m)| m.block() != block)
            .map(|(id, _)| id)
            .collect()
    } else {
        events
            .read()
            .filter_map(|event| match event {
                AssetEvent::Added { id } | AssetEvent::Modified { id } => Some(*id),
                _ => None,
            })
            .filter(|id| assets.get(*id).is_some_and(|m| m.block() != block))
            .collect()
    };
    for id in stale {
        if let Some(mut asset) = assets.get_mut(id) {
            asset.set_block(block);
        }
    }
}

/// One surface sample for [`toon_shade_sample`]: what the shader knows about a
/// fragment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToonSample {
    /// Linear albedo (base × vertex colour).
    pub albedo: LinearRgba,
    /// Unit surface normal.
    pub normal: Vec3,
    /// Unit vector toward the eye.
    pub to_eye: Vec3,
    /// Baked AO (`COLOR_0` alpha): 1 open, 0 enclosed.
    pub ao: f32,
    /// The material's rim multiplier.
    pub rim: f32,
    /// The material's AO multiplier.
    pub occlusion: f32,
    /// The material's own surface (used when the palette doesn't tag the albedo).
    pub surface: Surface,
    /// The camera's right, up and back axes (world space) as columns;
    /// identity is a camera looking along -Z.
    pub camera: Mat3,
}

impl ToonSample {
    pub fn new(albedo: LinearRgba, normal: Vec3, to_eye: Vec3) -> Self {
        Self {
            albedo,
            normal,
            to_eye,
            ao: 1.0,
            rim: 1.0,
            occlusion: 1.0,
            surface: Surface::MATTE,
            camera: Mat3::IDENTITY,
        }
    }
}

/// CPU mirror of `toon.wgsl`'s colour math before the grade (no textures, no
/// emissive, no screen-space band widening).
pub fn toon_shade_linear(s: &ToonSample, light: &ToonLight) -> Vec3 {
    let a = Vec3::new(s.albedo.red, s.albedo.green, s.albedo.blue);
    let surface = light.surfaces.lookup(a, s.surface.to_vec4());
    let (n, v) = (s.normal, s.to_eye);
    let key = light.key_direction.truncate();
    let ndl = n.dot(key);
    let w = light.key_color.w.max(1e-4);
    let (t_mid, t_lit) = (light.key_direction.w, light.bands.x);
    let to_mid = smoothstep(t_mid - w, t_mid + w, ndl);
    let to_lit = smoothstep(t_lit - w, t_lit + w, ndl);
    let hemi = light
        .ground_fill
        .truncate()
        .lerp(light.sky_fill.truncate(), n.y * 0.5 + 0.5);
    let fill = light.fill_color.truncate() * n.dot(light.fill_direction.truncate()).max(0.0);
    let shadow = light.shadow_tint.truncate() + fill + hemi;
    let mid = light.mid_tint.truncate() + (fill + hemi) * 0.5;
    let tone = shadow
        .lerp(mid, to_mid)
        .lerp(light.key_color.truncate(), to_lit)
        * (1.0 + light.bands.y * (ndl - t_lit)).max(0.0);
    let k = (light.ao_tint.w * s.occlusion).clamp(0.0, 1.0);
    let open = 1.0 - k * (1.0 - s.ao.clamp(0.0, 1.0));
    let occ = light.ao_tint.truncate().lerp(Vec3::ONE, open);
    let mut c = a * tone * occ;
    // The cartoon highlight: a crisp blob plus a broad sheen, lit side only.
    let h = (highlight_light(light, s.camera) + v).normalize_or(n);
    let ndh = n.dot(h).max(0.0);
    let blob_w = light.highlight.w.max(1e-4);
    let blob = smoothstep(0.5 - blob_w, 0.5 + blob_w, ndh.powf(surface.y.max(1.0)));
    let broad = ndh.powf((surface.y * 0.2).max(1.0));
    let hue = a / a.max_element().max(1e-4);
    let highlight = light.highlight.truncate().lerp(hue, surface.w);
    c += highlight * surface.x * (blob + surface.z * broad) * to_mid * open;
    let grazing = (1.0 - n.dot(v).clamp(0.0, 1.0)).powf(light.rim_power);
    c += light.rim.truncate() * grazing * light.rim.w * s.rim * (0.4 + 0.6 * to_mid) * open;
    c
}

/// CPU mirror of `toon.wgsl`: [`toon_shade_linear`] then the grade.
pub fn toon_shade_sample(s: &ToonSample, light: &ToonLight) -> LinearRgba {
    let c = grade(toon_shade_linear(s, light));
    LinearRgba::new(c.x, c.y, c.z, s.albedo.alpha)
}

/// [`toon_shade_sample`] for an open (AO 1), otherwise matte surface with the
/// material rim multiplier `rim`. `albedo` is linear; `normal` and `to_eye`
/// are unit vectors.
pub fn toon_shade(
    albedo: LinearRgba,
    normal: Vec3,
    to_eye: Vec3,
    light: &ToonLight,
    rim: f32,
) -> LinearRgba {
    toon_shade_sample(
        &ToonSample {
            rim,
            ..ToonSample::new(albedo, normal, to_eye)
        },
        light,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::cartoon;

    fn lum(c: LinearRgba) -> f32 {
        0.2126 * c.red + 0.7152 * c.green + 0.0722 * c.blue
    }

    fn lum3(c: Vec3) -> f32 {
        c.dot(Vec3::new(0.2126, 0.7152, 0.0722))
    }

    /// A unit normal whose N·L with the default key is `ndl`, seen head-on.
    fn facing(light: &ToonLight, ndl: f32) -> Vec3 {
        let key = light.key_direction.truncate();
        let side = key.cross(Vec3::Y).normalize();
        (key * ndl + side * (1.0 - ndl * ndl).max(0.0).sqrt()).normalize()
    }

    #[test]
    fn a_white_key_lands_palette_colors_as_authored_before_the_grade() {
        let lighting = ToonLighting {
            key_color: Color::WHITE,
            band_gradient: 0.0,
            ..default()
        };
        let light = ToonLight::from(&lighting);
        let brick = cartoon::BRICK.to_linear();
        let n = light.key_direction.truncate();
        // Seen head-on (no rim), fully open, matte.
        let c = toon_shade_linear(&ToonSample::new(brick, n, n), &light);
        let out = Color::linear_rgb(c.x, c.y, c.z).to_srgba();
        let want = cartoon::BRICK.to_srgba();
        assert!((out.red - want.red).abs() < 1e-3, "{out:?}");
        assert!((out.green - want.green).abs() < 1e-3, "{out:?}");
        assert!((out.blue - want.blue).abs() < 1e-3, "{out:?}");
        // On screen it is the graded palette colour.
        let graded = toon_shade(brick, n, n, &light, 1.0);
        assert_eq!(Vec3::new(graded.red, graded.green, graded.blue), grade(c));
    }

    #[test]
    fn three_tones_step_up_with_the_light() {
        let light = ToonLight::default();
        let (t_mid, t_lit) = (light.key_direction.w, light.bands.x);
        for base in [cartoon::BRICK, cartoon::GRASS, cartoon::PLANK] {
            let albedo = base.to_linear();
            let at = |ndl: f32| {
                let n = facing(&light, ndl);
                lum(toon_shade(albedo, n, n, &light, 0.0))
            };
            // From the shadow edge on, brightness never drops as a face turns
            // toward the key. (Deeper in shadow the galaxy fill, from the
            // other side, lights faces turned away from the key.)
            let mut last = 0.0;
            for i in 0..=30 {
                let l = at(t_mid - 0.2 + i as f32 * 0.04);
                assert!(l >= last - 1e-4, "{base:?} at step {i}");
                last = l;
            }
            let (shadow, mid, lit) = (at(t_mid - 0.3), at((t_mid + t_lit) / 2.0), at(t_lit + 0.3));
            assert!(
                mid > shadow * 1.25,
                "{base:?}: mid {mid} vs shadow {shadow}"
            );
            assert!(lit > mid * 1.12, "{base:?}: lit {lit} vs mid {mid}");
            // The edges are soft but clearly cartoon: most of each step happens
            // within a narrow band of N·L.
            let edge = at(t_lit + 0.08) - at(t_lit - 0.08);
            assert!(edge > (lit - mid) * 0.5, "{base:?}: soft edge {edge}");
        }
    }

    #[test]
    fn shadows_are_violet_and_never_black() {
        let light = ToonLight::default();
        let away = -light.key_direction.truncate();
        for base in [
            cartoon::BRICK,
            cartoon::GRASS,
            Color::srgb(0.2, 0.2, 0.22),
            Color::WHITE,
        ] {
            let albedo = base.to_linear();
            let lit = toon_shade(albedo, -away, -away, &light, 0.0);
            let dark = toon_shade(albedo, away, away, &light, 0.0);
            assert!(
                lum(dark) < lum(lit) * 0.6,
                "{base:?}: shadow must read darker"
            );
            assert!(
                lum(dark) > lum(lit) * 0.12,
                "{base:?}: shadow must not go black"
            );
        }
        // White in shadow leans blue-violet.
        let white = toon_shade(LinearRgba::WHITE, away, away, &light, 0.0);
        assert!(
            white.blue > white.red && white.blue > white.green,
            "{white:?}"
        );
    }

    #[test]
    fn default_key_is_warm_high_and_lights_the_spawn_view() {
        let lighting = ToonLighting::default();
        let key = lighting.key_color.to_srgba();
        assert!(key.red >= key.green && key.green >= key.blue, "warm key");
        assert!(lighting.key_direction.y > 0.5, "key high in the sky");
        // East: the station side of the spawn view (looking north, -Z).
        assert!(lighting.key_direction.x > 0.5);
        // Faces turned toward a player at spawn (+Z) sit at least in the mid tone.
        assert!(lighting.key_direction.dot(Vec3::Z) > lighting.band_threshold + 0.1);
        // Faces toward the key (east) and tops are in the lit tone.
        assert!(lighting.key_direction.dot(Vec3::X) > lighting.lit_threshold + 0.1);
        assert!(lighting.key_direction.dot(Vec3::Y) > lighting.lit_threshold + 0.1);
        // The fill comes from the opposite (galaxy) side.
        assert!(lighting.fill_direction.dot(lighting.key_direction) < 0.0);
        // The sky fill is cool, the ground bounce green.
        let (sky, ground) = (
            lighting.sky_fill.to_linear(),
            lighting.ground_fill.to_linear(),
        );
        assert!(sky.blue > sky.red && ground.green > ground.blue);
    }

    #[test]
    fn metal_and_crystal_get_a_crisp_highlight_and_grass_does_not() {
        let light = ToonLight::default();
        let key = highlight_light(&light, Mat3::IDENTITY);
        let eye = Vec3::new(0.3, 0.2, 1.0).normalize();
        let mirror = (key + eye).normalize();
        let shine = |c: Color, n: Vec3| {
            let s = ToonSample::new(c.to_linear(), n, eye);
            let with = toon_shade_linear(&s, &light);
            let without = toon_shade_linear(
                &s,
                &ToonLight {
                    surfaces: SurfaceTable {
                        count: 0,
                        ..light.surfaces
                    },
                    ..light
                },
            );
            lum3(with - without)
        };
        // At the mirror angle brass and crystal flash; grass and wood don't.
        assert!(shine(cartoon::BRASS, mirror) > 0.25);
        assert!(shine(cartoon::CRYSTAL_BLUE, mirror) > 0.25);
        assert!(shine(cartoon::KNIGHT_STEEL, mirror) > 0.2);
        assert!(shine(cartoon::GRASS, mirror).abs() < 1e-6);
        assert!(shine(cartoon::PLANK, mirror).abs() < 1e-6);
        // Crystal's glint is a smaller blob than brass: 12° off the mirror
        // angle brass still shines, crystal is down to its sheen.
        let off = Quat::from_axis_angle(key.cross(eye).normalize(), 12f32.to_radians()) * mirror;
        assert!(shine(cartoon::BRASS, off) > shine(cartoon::CRYSTAL_BLUE, off));
        // A brass highlight is golden, not white.
        let s = ToonSample::new(cartoon::BRASS.to_linear(), mirror, eye);
        let c = toon_shade_linear(&s, &light);
        assert!(c.x > c.y && c.y > c.z, "{c}");
        // No highlight on the shadow side.
        assert!(shine(cartoon::BRASS, -mirror).abs() < 1e-6);
        // The gleam follows the camera: seen from the other side (camera
        // turned 180°, eye along its new back axis), the same top-front
        // relation still shines.
        let turned = Mat3::from_rotation_y(std::f32::consts::PI);
        let eye2 = turned * eye;
        let key2 = highlight_light(&light, turned);
        let mirror2 = (key2 + eye2).normalize();
        let s2 = ToonSample {
            camera: turned,
            ..ToonSample::new(cartoon::BRASS.to_linear(), mirror2, eye2)
        };
        let plain2 = toon_shade_linear(
            &ToonSample {
                albedo: cartoon::PLANK.to_linear(),
                ..s2
            },
            &light,
        );
        assert!(lum3(toon_shade_linear(&s2, &light)) > lum3(plain2));
        // A material's own surface lights untagged colours.
        let own = ToonSample {
            surface: Surface::new(1.0, 30.0),
            ..ToonSample::new(cartoon::PLANK.to_linear(), mirror, eye)
        };
        let plain = ToonSample::new(cartoon::PLANK.to_linear(), mirror, eye);
        assert!(lum3(toon_shade_linear(&own, &light) - toon_shade_linear(&plain, &light)) > 0.25);
    }

    #[test]
    fn baked_ao_darkens_toward_violet_and_can_be_ignored() {
        let light = ToonLight::default();
        let n = light.key_direction.truncate();
        let albedo = cartoon::ROCK.to_linear();
        let open = toon_shade_linear(&ToonSample::new(albedo, n, n), &light);
        let crease = ToonSample {
            ao: 0.2,
            ..ToonSample::new(albedo, n, n)
        };
        let dark = toon_shade_linear(&crease, &light);
        assert!(lum3(dark) < lum3(open) * 0.6, "{dark} vs {open}");
        assert!(lum3(dark) > lum3(open) * 0.25, "never black");
        // Occluded rock leans violet relative to open rock.
        assert!(dark.z / dark.x > open.z / open.x);
        // A material can opt out (eyes deep in a visor).
        let ignored = toon_shade_linear(
            &ToonSample {
                occlusion: 0.0,
                ..crease
            },
            &light,
        );
        assert!((ignored - open).abs().max_element() < 1e-6);
    }

    #[test]
    fn rim_brightens_grazing_faces_only() {
        let light = ToonLight::default();
        let n = Vec3::Y;
        let base = LinearRgba::rgb(0.2, 0.2, 0.2);
        let head_on = toon_shade(base, n, n, &light, 1.0);
        let grazing = toon_shade(base, n, Vec3::new(1.0, 0.05, 0.0).normalize(), &light, 1.0);
        assert!(lum(grazing) > lum(head_on) + 0.05);
        let no_rim = toon_shade(base, n, Vec3::new(1.0, 0.05, 0.0).normalize(), &light, 0.0);
        assert!((lum(no_rim) - lum(head_on)).abs() < 1e-5);
    }

    fn lighting_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>()
            .init_asset::<ToonMaterial>()
            .init_resource::<ToonLighting>()
            .add_systems(PostUpdate, sync_global_block::<ToonLighting, ToonMaterial>);
        app
    }

    #[test]
    fn lighting_reaches_every_toon_material() {
        let mut app = lighting_app();
        let custom = ToonLighting {
            key_color: Color::srgb(1.0, 0.5, 0.25),
            ..default()
        };
        app.insert_resource(custom.clone());
        let a = app
            .world_mut()
            .resource_mut::<Assets<ToonMaterial>>()
            .add(ToonMaterial::vertex_colored());
        app.update();
        let expected = ToonLight::from(&custom);
        assert_eq!(
            app.world()
                .resource::<Assets<ToonMaterial>>()
                .get(&a)
                .unwrap()
                .lighting,
            expected
        );

        // A material added later picks up the current lighting.
        let b = app
            .world_mut()
            .resource_mut::<Assets<ToonMaterial>>()
            .add(ToonMaterial::new(Color::srgb(0.3, 0.6, 0.2)));
        app.update();
        app.update();
        let mats = app.world().resource::<Assets<ToonMaterial>>();
        assert_eq!(mats.get(&b).unwrap().lighting, expected);

        // Changing the global updates every material.
        app.world_mut().resource_mut::<ToonLighting>().shadow_tint = Color::srgb(0.5, 0.4, 0.9);
        app.update();
        let expected = ToonLight::from(app.world().resource::<ToonLighting>());
        let mats = app.world().resource::<Assets<ToonMaterial>>();
        for handle in [&a, &b] {
            assert_eq!(mats.get(handle).unwrap().lighting, expected);
        }

        // A material replaced with a fresh default gets fixed up again.
        let stale = ToonMaterial::vertex_colored();
        assert_ne!(stale.lighting, expected);
        *app.world_mut()
            .resource_mut::<Assets<ToonMaterial>>()
            .get_mut(&a)
            .unwrap() = stale;
        app.update();
        app.update();
        let mats = app.world().resource::<Assets<ToonMaterial>>();
        assert_eq!(mats.get(&a).unwrap().lighting, expected);
    }

    #[test]
    fn unchanged_lighting_leaves_materials_untouched() {
        let mut app = lighting_app();
        app.world_mut()
            .resource_mut::<Assets<ToonMaterial>>()
            .add(ToonMaterial::vertex_colored());
        app.update();
        app.update();
        // Nothing changed: no Modified events are produced by the sync.
        let mut cursor = app
            .world()
            .resource::<Messages<AssetEvent<ToonMaterial>>>()
            .get_cursor();
        app.update();
        let events = app.world().resource::<Messages<AssetEvent<ToonMaterial>>>();
        assert!(
            cursor
                .read(events)
                .all(|e| !matches!(e, AssetEvent::Modified { .. })),
            "sync must not rewrite materials that already match"
        );
    }

    #[test]
    fn material_key_tracks_pipeline_state() {
        let ghost =
            ToonMaterial::new(Color::srgba(0.5, 0.8, 1.0, 0.4)).with_alpha(AlphaMode::Blend);
        let key = ToonKey::from(&ghost);
        assert_eq!(key.cull_mode, Some(Face::Back));
        assert!(!key.detail && !key.additive);
        // Blended: vertex alpha is opacity (ghost lines), so no AO.
        assert!(key.vertex_opacity);
        assert_eq!(ToonUniform::from(&ghost).emissive.w, 0.0);
        let glass = ghost.clone().with_vertex_alpha(VertexAlpha::Occlusion);
        assert!(!ToonKey::from(&glass).vertex_opacity);
        let solid = ToonMaterial::vertex_colored();
        assert!(!ToonKey::from(&solid).vertex_opacity);
        let ao = ToonUniform::from(&solid).emissive.w;
        assert!(ao > 0.5, "opaque models darken by their baked AO: {ao}");
        let eyes = ToonMaterial::vertex_colored().with_occlusion(0.0);
        assert_eq!(ToonUniform::from(&eyes).emissive.w, 0.0);
        let shiny = ToonMaterial::new(Color::WHITE).with_specular(0.8, 40.0);
        assert_eq!(
            ToonUniform::from(&shiny).surface,
            Vec4::new(0.8, 40.0, 0.0, 0.0)
        );
        // Every material carries the palette's surface table.
        let u = ToonUniform::from(&solid);
        assert_eq!(u.mid_tint.w as u32, solid.lighting.surfaces.count);
        assert!(solid.lighting.surfaces.count > 0);
        let grass = ToonMaterial::vertex_colored().double_sided();
        assert_eq!(ToonKey::from(&grass).cull_mode, None);
        let glow = ToonMaterial::default().with_alpha(AlphaMode::Add);
        assert!(ToonKey::from(&glow).additive);
        let detailed =
            ToonMaterial::default().with_detail(Handle::default(), Vec2::splat(4.0), 0.6);
        assert!(ToonKey::from(&detailed).detail);
        let uniform = ToonUniform::from(&detailed);
        assert_eq!(uniform.params.z, 4.0);
        assert_eq!(uniform.params.y, 0.6);
    }
}
