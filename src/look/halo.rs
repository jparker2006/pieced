//! Halos: additive, camera-facing glow billboards with a soft radial texture
//! generated in code. Fake bloom for crystals, stained glass, bolts and
//! waterfalls: one shared quad mesh and one shared material, with each halo's
//! color and intensity packed into its `MeshTag`, so any number of halos
//! batch together.

use super::outline::NoOutline;
use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::{NoFrustumCulling, RenderLayers},
    image::Image,
    light::NotShadowCaster,
    mesh::{MeshTag, MeshVertexBufferLayoutRef},
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{
        AsBindGroup, Extent3d, RenderPipelineDescriptor, SpecializedMeshPipelineError,
        TextureDimension, TextureFormat,
    },
    shader::ShaderRef,
};

pub const HALO_SHADER_PATH: &str = "embedded://pieced/shaders/halo.wgsl";
/// Side of the generated radial texture, in pixels.
pub const HALO_TEXTURE_SIZE: u32 = 64;
/// Largest intensity a halo can carry (it is packed into 8 bits).
pub const HALO_MAX_INTENSITY: f32 = 8.0;

/// A glow halo centered on this entity. Spawn it on its own or on any entity
/// that should glow (a crystal node, a bolt): `commands.spawn((Halo::new(color,
/// size, intensity), Transform::from_translation(p)))`. Change the component
/// to recolor or resize it; despawn the entity to remove it. The halo copies
/// the entity's `RenderLayers` (so it works on the viewmodel layer too).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
#[require(Transform, Visibility)]
pub struct Halo {
    /// Glow color (additive; black shows nothing).
    pub color: Color,
    /// Diameter in meters (times the entity's scale).
    pub size: f32,
    /// Brightness multiplier, 0..[`HALO_MAX_INTENSITY`].
    pub intensity: f32,
}

impl Halo {
    pub fn new(color: Color, size: f32, intensity: f32) -> Self {
        Self {
            color,
            size,
            intensity,
        }
    }
}

/// The billboard entity drawing a [`Halo`] (a child of the halo's entity).
#[derive(Component, Debug, Clone, Copy)]
pub struct HaloSprite {
    pub owner: Entity,
}

/// On a [`Halo`] entity: its sprite.
#[derive(Component, Debug, Clone, Copy)]
pub struct HaloLink(pub Entity);

/// Additive billboard material: one per texture (usually just the shared one).
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct HaloMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub texture: Handle<Image>,
}

impl Material for HaloMaterial {
    fn vertex_shader() -> ShaderRef {
        HALO_SHADER_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        HALO_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
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
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.vertex.buffers = vec![layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
        ])?];
        // Billboards are never back-facing.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// The shared halo quad, material and texture.
#[derive(Resource, Debug, Clone)]
pub struct HaloAssets {
    pub quad: Handle<Mesh>,
    pub material: Handle<HaloMaterial>,
    pub texture: Handle<Image>,
}

/// Radial falloff at `r` (0 at the center, 1 at the edge): a soft gaussian
/// core that reaches exactly zero at the edge.
pub fn halo_falloff(r: f32) -> f32 {
    let r = r.clamp(0.0, 1.0);
    let edge = 1.0 - {
        let t = ((r - 0.7) / 0.3).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    (-r * r * 4.0).exp() * edge
}

/// A `size`² single-channel radial texture (row-major, 0..255).
pub fn halo_texture_data(size: u32) -> Vec<u8> {
    let half = size as f32 / 2.0;
    (0..size * size)
        .map(|i| {
            let (x, y) = ((i % size) as f32 + 0.5, (i / size) as f32 + 0.5);
            let r = Vec2::new(x - half, y - half).length() / half;
            (halo_falloff(r) * 255.0).round() as u8
        })
        .collect()
}

pub fn halo_image(size: u32) -> Image {
    Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        halo_texture_data(size),
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// Packs a halo's color (sRGB bytes) and intensity (1/32 steps) into a tag.
pub fn pack_halo(color: Color, intensity: f32) -> u32 {
    let c = color.to_srgba();
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    let i =
        ((intensity.clamp(0.0, HALO_MAX_INTENSITY) / HALO_MAX_INTENSITY) * 255.0).round() as u32;
    byte(c.red) | byte(c.green) << 8 | byte(c.blue) << 16 | i << 24
}

/// Inverse of [`pack_halo`] (as the shader reads it): sRGB color, intensity.
pub fn unpack_halo(tag: u32) -> (Srgba, f32) {
    let byte = |shift: u32| ((tag >> shift) & 0xFF) as f32 / 255.0;
    (
        Srgba::new(byte(0), byte(8), byte(16), 1.0),
        byte(24) * HALO_MAX_INTENSITY,
    )
}

pub(crate) fn create_halo_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<HaloMaterial>>,
) {
    let texture = images.add(halo_image(HALO_TEXTURE_SIZE));
    commands.insert_resource(HaloAssets {
        quad: meshes.add(Rectangle::new(1.0, 1.0)),
        material: materials.add(HaloMaterial {
            texture: texture.clone(),
        }),
        texture,
    });
}

fn sprite_scale(halo: &Halo) -> Vec3 {
    Vec3::splat(halo.size.max(1e-4))
}

pub(crate) fn attach_halo_sprites(
    mut commands: Commands,
    assets: Option<Res<HaloAssets>>,
    settings: Res<super::LookSettings>,
    halos: Query<(Entity, &Halo, Option<&RenderLayers>), Without<HaloLink>>,
) {
    let Some(assets) = assets else {
        return;
    };
    for (entity, halo, layers) in &halos {
        let mut sprite = commands.spawn((
            Name::new("Halo"),
            HaloSprite { owner: entity },
            Mesh3d(assets.quad.clone()),
            MeshMaterial3d(assets.material.clone()),
            MeshTag(pack_halo(halo.color, halo.intensity)),
            Transform::from_scale(sprite_scale(halo)),
            if settings.halos {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            },
            NoFrustumCulling,
            NotShadowCaster,
            NoOutline,
            ChildOf(entity),
        ));
        if let Some(layers) = layers {
            sprite.insert(layers.clone());
        }
        let sprite = sprite.id();
        commands.entity(entity).insert(HaloLink(sprite));
    }
}

pub(crate) fn sync_halo_sprites(
    halos: Query<
        (&Halo, &HaloLink, Option<&RenderLayers>),
        Or<(Changed<Halo>, Changed<RenderLayers>)>,
    >,
    mut sprites: Query<
        (&mut MeshTag, &mut Transform, Option<&mut RenderLayers>),
        (With<HaloSprite>, Without<Halo>),
    >,
    mut commands: Commands,
) {
    for (halo, link, layers) in &halos {
        let Ok((mut tag, mut transform, sprite_layers)) = sprites.get_mut(link.0) else {
            continue;
        };
        tag.set_if_neq(MeshTag(pack_halo(halo.color, halo.intensity)));
        let scale = sprite_scale(halo);
        if transform.scale != scale {
            transform.scale = scale;
        }
        match (layers, sprite_layers) {
            (Some(want), Some(mut have)) => {
                have.set_if_neq(want.clone());
            }
            (Some(want), None) => {
                commands.entity(link.0).insert(want.clone());
            }
            (None, Some(_)) => {
                commands.entity(link.0).remove::<RenderLayers>();
            }
            (None, None) => {}
        }
    }
}

pub(crate) fn apply_halo_setting(
    settings: Res<super::LookSettings>,
    mut sprites: Query<&mut Visibility, With<HaloSprite>>,
) {
    if !settings.is_changed() {
        return;
    }
    let visibility = if settings.halos {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut v in &mut sprites {
        v.set_if_neq(visibility);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radial_texture_is_soft_and_bright_in_the_middle() {
        let size = HALO_TEXTURE_SIZE;
        let data = halo_texture_data(size);
        assert_eq!(data.len(), (size * size) as usize);
        let at = |x: u32, y: u32| data[(y * size + x) as usize];
        let c = size / 2;
        assert!(at(c, c) >= 250, "center {}", at(c, c));
        assert_eq!(at(0, 0), 0, "corners are empty");
        assert_eq!(at(0, c), 0, "edges are empty");
        // Falls off monotonically from the center outward.
        let row: Vec<u8> = (c..size).map(|x| at(x, c)).collect();
        assert!(row.windows(2).all(|w| w[0] >= w[1]), "{row:?}");
        // Soft, not a disc: half-way out it is well below full.
        assert!(at(c + size / 4, c) < 128);
        assert!(at(c + size / 4, c) > 10);
    }

    #[test]
    fn color_and_intensity_round_trip_through_the_tag() {
        let color = Color::srgb(0.3, 0.7, 1.0);
        let (c, i) = unpack_halo(pack_halo(color, 2.5));
        assert!((c.red - 0.3).abs() < 0.003 && (c.green - 0.7).abs() < 0.003);
        assert!((c.blue - 1.0).abs() < 0.003);
        assert!((i - 2.5).abs() < HALO_MAX_INTENSITY / 255.0);
        let (_, clamped) = unpack_halo(pack_halo(color, 100.0));
        assert_eq!(clamped, HALO_MAX_INTENSITY);
        assert_eq!(unpack_halo(pack_halo(Color::BLACK, 0.0)).1, 0.0);
    }

    #[test]
    fn halos_get_a_sprite_that_follows_the_component() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::mesh::MeshPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<HaloMaterial>()
        .init_resource::<super::super::LookSettings>()
        .add_systems(Startup, create_halo_assets)
        .add_systems(Update, (attach_halo_sprites, sync_halo_sprites).chain());
        let owner = app
            .world_mut()
            .spawn((
                Halo::new(Color::srgb(0.4, 0.8, 1.0), 0.5, 1.5),
                RenderLayers::layer(1),
            ))
            .id();
        app.update();
        let link = *app.world().get::<HaloLink>(owner).expect("sprite spawned");
        let sprite = app.world().entity(link.0);
        assert_eq!(sprite.get::<ChildOf>().unwrap().parent(), owner);
        assert_eq!(sprite.get::<Transform>().unwrap().scale, Vec3::splat(0.5));
        assert_eq!(sprite.get::<RenderLayers>(), Some(&RenderLayers::layer(1)));

        app.world_mut().get_mut::<Halo>(owner).unwrap().size = 2.0;
        app.world_mut().get_mut::<Halo>(owner).unwrap().intensity = 3.0;
        app.update();
        let sprite = app.world().entity(link.0);
        assert_eq!(sprite.get::<Transform>().unwrap().scale, Vec3::splat(2.0));
        let (_, intensity) = unpack_halo(sprite.get::<MeshTag>().unwrap().0);
        assert!((intensity - 3.0).abs() < 0.05);
    }
}
