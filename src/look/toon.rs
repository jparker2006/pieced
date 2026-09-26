//! The toon material: two hard bands against one key light (the shadow band
//! tinted cool violet, never black), a teal fill on the shadow side, a thin rim
//! light and an emissive channel. No PBR light loops: `toon.wgsl` reads only
//! its own uniform, which carries a copy of the global [`ToonLighting`].
//!
//! [`toon_shade`] is a CPU mirror of the shader's color math, used by tests and
//! available to tooling that wants to predict an on-screen color.

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

/// Unit vector toward a light, from an azimuth (degrees clockwise from north,
/// -Z, toward east, +X) and an elevation (degrees above the horizon).
pub fn light_direction(azimuth_deg: f32, elevation_deg: f32) -> Vec3 {
    let (az, el) = (azimuth_deg.to_radians(), elevation_deg.to_radians());
    Vec3::new(az.sin() * el.cos(), el.sin(), -az.cos() * el.cos()).normalize()
}

/// The one global light rig every toon surface agrees on. Change it and a
/// system copies it into every [`ToonMaterial`] (and the far layer's haze has
/// its own [`super::FarHaze`]).
///
/// Defaults follow the M2 targets: a warm key light from the station side
/// (east, from the spawn view), violet shadows and a teal fill from the galaxy
/// side (north-west). The key sits slightly behind the spawn view (azimuth
/// 115°), so faces turned toward a player at spawn read lit, as in T01 and T09.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct ToonLighting {
    /// Unit vector from a surface toward the key light.
    pub key_direction: Vec3,
    /// Multiplies albedo in the lit band. White lands palette colors exactly.
    pub key_color: Color,
    /// Multiplies albedo in the shadow band.
    pub shadow_tint: Color,
    /// Unit vector toward the fill light (only lights the shadow band).
    pub fill_direction: Vec3,
    /// Added to the shadow band as albedo × fill × max(N·F, 0).
    pub fill_color: Color,
    pub rim_color: Color,
    /// Rim brightness at a grazing angle (each material scales it).
    pub rim_strength: f32,
    /// Rim falloff exponent: higher is thinner.
    pub rim_power: f32,
    /// N·L where the bands meet.
    pub band_threshold: f32,
    /// Half-width of the band edge in N·L (anti-aliasing; screen-space
    /// derivatives widen it where needed).
    pub band_softness: f32,
}

impl Default for ToonLighting {
    fn default() -> Self {
        Self {
            key_direction: light_direction(115.0, 45.0),
            key_color: Color::srgb(1.0, 0.98, 0.94),
            shadow_tint: Color::srgb(0.72, 0.66, 0.86),
            fill_direction: light_direction(300.0, 35.0),
            fill_color: Color::srgb(0.2, 0.42, 0.45),
            rim_color: Color::srgb(1.0, 0.93, 0.8),
            rim_strength: 0.3,
            rim_power: 4.0,
            band_threshold: 0.0,
            band_softness: 0.02,
        }
    }
}

/// [`ToonLighting`] as the GPU sees it (linear colors, packed). Every
/// [`ToonMaterial`] carries one; the sync system keeps it current.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToonLight {
    /// xyz toward the key, w band threshold.
    pub key_direction: Vec4,
    /// rgb key color, w band softness.
    pub key_color: Vec4,
    pub shadow_tint: Vec4,
    pub fill_direction: Vec4,
    pub fill_color: Vec4,
    /// rgb rim color, w rim strength.
    pub rim: Vec4,
    pub rim_power: f32,
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
        }
    }
}

impl Default for ToonLight {
    fn default() -> Self {
        Self::from(&ToonLighting::default())
    }
}

/// Our cartoon surface material. Use it for everything near: the world, pieces,
/// props, characters and the viewmodel.
///
/// - The final albedo is `base_color` × the mesh's `COLOR_0` vertex color (when
///   the mesh has one: the mesh pipeline specializes on the vertex layout and
///   sets `VERTEX_COLORS`) × the optional detail texture.
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
}

/// Pipeline key: what changes the compiled shader or pipeline state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ToonKey {
    cull_mode: Option<Face>,
    detail: bool,
    additive: bool,
}

impl From<&ToonMaterial> for ToonKey {
    fn from(m: &ToonMaterial) -> Self {
        Self {
            cull_mode: m.cull_mode,
            detail: m.detail.is_some(),
            additive: m.alpha_mode == AlphaMode::Add,
        }
    }
}

#[derive(Clone, Copy, Default, ShaderType)]
pub struct ToonUniform {
    base_color: Vec4,
    emissive: Vec4,
    key_direction: Vec4,
    key_color: Vec4,
    shadow_tint: Vec4,
    fill_direction: Vec4,
    fill_color: Vec4,
    rim_color: Vec4,
    /// x: rim power, y: detail strength, zw: detail UV scale.
    params: Vec4,
}

impl From<&ToonMaterial> for ToonUniform {
    fn from(m: &ToonMaterial) -> Self {
        let base = m.base_color.to_linear();
        let emissive = m.emissive.to_linear() * m.emissive_strength;
        let l = &m.lighting;
        Self {
            base_color: Vec4::new(base.red, base.green, base.blue, base.alpha),
            emissive: Vec4::new(emissive.red, emissive.green, emissive.blue, 0.0),
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

/// CPU mirror of `toon.wgsl`'s color math (no textures, no tonemapping: the
/// cameras use `Tonemapping::None`). `albedo` is linear; `normal` and
/// `to_eye` are unit vectors; `rim` is the material's rim multiplier.
pub fn toon_shade(
    albedo: LinearRgba,
    normal: Vec3,
    to_eye: Vec3,
    light: &ToonLight,
    rim: f32,
) -> LinearRgba {
    let a = Vec3::new(albedo.red, albedo.green, albedo.blue);
    let ndl = normal.dot(light.key_direction.truncate());
    let (t, w) = (light.key_direction.w, light.key_color.w.max(1e-4));
    let lit = smoothstep(t - w, t + w, ndl);
    let fill = normal.dot(light.fill_direction.truncate()).max(0.0);
    let shadow = a * (light.shadow_tint.truncate() + light.fill_color.truncate() * fill);
    let bright = a * light.key_color.truncate();
    let mut c = shadow.lerp(bright, lit);
    let grazing = (1.0 - normal.dot(to_eye).clamp(0.0, 1.0)).powf(light.rim_power);
    c += light.rim.truncate() * grazing * light.rim.w * rim * (0.5 + 0.5 * lit);
    LinearRgba::new(c.x, c.y, c.z, albedo.alpha)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lum(c: LinearRgba) -> f32 {
        0.2126 * c.red + 0.7152 * c.green + 0.0722 * c.blue
    }

    #[test]
    fn a_white_key_lands_palette_colors_exactly() {
        let lighting = ToonLighting {
            key_color: Color::WHITE,
            ..default()
        };
        let light = ToonLight::from(&lighting);
        let brick = Color::srgb(0.78, 0.35, 0.2).to_linear();
        let n = light.key_direction.truncate();
        // Seen head-on, so no rim.
        let c = toon_shade(brick, n, n, &light, 1.0);
        let out = Color::from(c).to_srgba();
        assert!((out.red - 0.78).abs() < 1e-3, "{out:?}");
        assert!((out.green - 0.35).abs() < 1e-3, "{out:?}");
        assert!((out.blue - 0.2).abs() < 1e-3, "{out:?}");
    }

    #[test]
    fn shadows_are_violet_and_never_black() {
        let light = ToonLight::default();
        let away = -light.key_direction.truncate();
        for base in [
            Color::srgb(0.78, 0.35, 0.2),
            Color::srgb(0.45, 0.72, 0.3),
            Color::srgb(0.2, 0.2, 0.22),
            Color::WHITE,
        ] {
            let albedo = base.to_linear();
            let lit = toon_shade(albedo, -away, -away, &light, 0.0);
            let dark = toon_shade(albedo, away, away, &light, 0.0);
            assert!(
                lum(dark) < lum(lit) * 0.8,
                "{base:?}: shadow must read darker"
            );
            assert!(
                lum(dark) > lum(lit) * 0.25,
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
        // Faces turned toward a player at spawn (+Z) sit in the lit band.
        assert!(lighting.key_direction.dot(Vec3::Z) > lighting.band_threshold + 0.1);
        // The fill comes from the opposite (galaxy) side.
        assert!(lighting.fill_direction.dot(lighting.key_direction) < 0.0);
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
