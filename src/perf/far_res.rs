//! `farres=half` (D100): the far layer (the station, far islands, ships, the
//! planet, the sky magic, the horizon glow and the galaxy skybox, all beyond
//! ~150 m) renders through its own camera into a half-resolution image, and
//! the world camera draws that image as a backdrop behind everything nearer.
//!
//! - The far camera ([`FarCamera`], order [`FAR_CAMERA_ORDER`]) sees only
//!   [`FAR_LAYER`], copies the world camera's transform and projection every
//!   frame, and takes the galaxy skybox (see `crate::far`).
//! - Every mesh and halo under a far root ([`crate::far::FarView`],
//!   [`crate::far::MagicPart`], [`crate::far::HorizonGlow`]) moves to
//!   [`FAR_LAYER`] as it appears, so the world camera no longer draws it.
//! - [`FarBackdropMaterial`] is one full-screen triangle at infinite depth in
//!   the world camera's opaque pass: it samples the far image at the pixel's
//!   screen position, and anything nearer hides it (depth test), so on a
//!   tile-based GPU it only shades the sky pixels.
//!
//! Saves: the far layer's fragments at a quarter of the pixels (its ~90k
//! triangles still run their vertex stage once), for one extra 1-texture
//! fetch over the uncovered sky. Off by default; the castle views must still
//! read (A6) before it becomes a default.

use crate::{
    far::{FarView, HorizonGlow, MagicPart},
    look::{Halo, NoOutline},
    render::{FAR_CAMERA_ORDER, MainCamera, WorldTarget},
    tuning::Tuning,
};
use bevy::{
    asset::io::embedded::EmbeddedAssetRegistry,
    camera::{RenderTarget, visibility::NoFrustumCulling, visibility::RenderLayers},
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    light::NotShadowCaster,
    mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology},
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{
        AsBindGroup, Extent3d, RenderPipelineDescriptor, SpecializedMeshPipelineError,
        TextureFormat, TextureUsages,
    },
    shader::ShaderRef,
};
use std::path::{Path, PathBuf};

/// The render layer only the far camera draws.
pub const FAR_LAYER: usize = 2;
/// The far image's size as a fraction of the world target's.
pub const FAR_RES_FRACTION: f32 = 0.5;

pub const BACKDROP_SHADER_PATH: &str = "embedded://pieced/shaders/far_backdrop.wgsl";

/// The far layer's own camera (`farres=half` only).
#[derive(Component, Debug)]
pub struct FarCamera;

/// The world camera's backdrop showing the far image.
#[derive(Component, Debug)]
pub struct FarBackdrop;

/// The far camera's half-resolution image.
#[derive(Resource, Debug, Clone)]
pub struct FarTarget {
    pub image: Handle<Image>,
    pub size: UVec2,
}

/// Draws the far image behind everything the world camera draws.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct FarBackdropMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub image: Handle<Image>,
}

impl Material for FarBackdropMaterial {
    fn vertex_shader() -> ShaderRef {
        BACKDROP_SHADER_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        BACKDROP_SHADER_PATH.into()
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
        descriptor.vertex.buffers = vec![
            layout
                .0
                .get_layout(&[Mesh::ATTRIBUTE_POSITION.at_shader_location(0)])?,
        ];
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

pub struct FarResPlugin;

impl Plugin for FarResPlugin {
    fn build(&self, app: &mut App) {
        if let Some(registry) = app.world().get_resource::<EmbeddedAssetRegistry>() {
            registry.insert_asset(
                PathBuf::new(),
                Path::new("pieced/shaders/far_backdrop.wgsl"),
                include_bytes!("../../assets/shaders/far_backdrop.wgsl").as_slice(),
            );
        }
        app.add_plugins(MaterialPlugin::<FarBackdropMaterial>::default())
            .add_systems(PostStartup, spawn_far_camera.run_if(far_half_res))
            .add_systems(
                PostUpdate,
                (
                    move_to_far_layer.before(TransformSystems::Propagate),
                    (follow_world_camera, resize_far_target)
                        .after(TransformSystems::Propagate)
                        .before(bevy::camera::visibility::VisibilitySystems::UpdateFrusta),
                )
                    .run_if(resource_exists::<FarTarget>),
            );
    }
}

fn far_half_res(tuning: Option<Res<Tuning>>) -> bool {
    tuning.is_some_and(|t| t.perf.far_half_res)
}

fn far_size(world: UVec2) -> UVec2 {
    (world.as_vec2() * FAR_RES_FRACTION)
        .round()
        .as_uvec2()
        .max(UVec2::splat(16))
}

/// One triangle that covers the whole screen in clip space.
pub fn fullscreen_triangle() -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![[-1.0, -1.0, 0.0], [3.0, -1.0, 0.0], [-1.0, 3.0, 0.0]],
    )
    .with_inserted_indices(Indices::U16(vec![0, 1, 2]))
}

fn spawn_far_camera(
    mut commands: Commands,
    world: Option<Res<WorldTarget>>,
    main: Query<(&Camera, &Projection, &Transform), With<MainCamera>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FarBackdropMaterial>>,
) {
    let (Some(world), Ok((camera, projection, transform))) = (world, main.single()) else {
        return;
    };
    let size = far_size(world.size);
    let mut image = Image::new_target_texture(
        size.x,
        size.y,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    );
    // COPY_SRC for the GPU timing's end-of-camera mark.
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let image = images.add(image);
    commands.insert_resource(FarTarget {
        image: image.clone(),
        size,
    });
    commands.spawn((
        Name::new("Far camera"),
        FarCamera,
        Camera3d::default(),
        RenderTarget::Image(image.clone().into()),
        Camera {
            order: FAR_CAMERA_ORDER,
            clear_color: camera.clear_color,
            ..default()
        },
        projection.clone(),
        *transform,
        RenderLayers::layer(FAR_LAYER),
        Msaa::Off,
        Tonemapping::None,
        DebandDither::Disabled,
    ));
    commands.spawn((
        Name::new("Far backdrop"),
        FarBackdrop,
        Mesh3d(meshes.add(fullscreen_triangle())),
        MeshMaterial3d(materials.add(FarBackdropMaterial { image })),
        Transform::IDENTITY,
        NoFrustumCulling,
        NotShadowCaster,
        NoOutline,
    ));
    info!(
        "farres=half: the far layer renders at {}×{}",
        size.x, size.y
    );
}

/// Whether `entity` is, or hangs under, a far root.
fn under_far_root(
    entity: Entity,
    parents: &Query<&ChildOf>,
    roots: &Query<(), Or<(With<FarView>, With<MagicPart>, With<HorizonGlow>)>>,
) -> bool {
    let mut at = entity;
    for _ in 0..32 {
        if roots.contains(at) {
            return true;
        }
        match parents.get(at) {
            Ok(parent) => at = parent.parent(),
            Err(_) => return false,
        }
    }
    false
}

/// Moves new far meshes and halos (the halo's sprite copies its owner's
/// layers) onto [`FAR_LAYER`].
fn move_to_far_layer(
    mut commands: Commands,
    added: Query<
        Entity,
        (
            Or<(With<Mesh3d>, With<Halo>)>,
            Or<(Added<Mesh3d>, Added<Halo>, Added<ChildOf>)>,
            Without<FarBackdrop>,
        ),
    >,
    parents: Query<&ChildOf>,
    roots: Query<(), Or<(With<FarView>, With<MagicPart>, With<HorizonGlow>)>>,
    layers: Query<&RenderLayers>,
) {
    let far = RenderLayers::layer(FAR_LAYER);
    for entity in &added {
        if layers.get(entity).is_ok_and(|l| *l == far) {
            continue;
        }
        if under_far_root(entity, &parents, &roots) {
            commands.entity(entity).insert(far.clone());
        }
    }
}

/// The far camera sees what the world camera sees (after camera shake and
/// ADS zoom): its transform, global transform and projection are copied.
fn follow_world_camera(
    main: Query<
        (&Transform, &GlobalTransform, &Projection),
        (With<MainCamera>, Without<FarCamera>),
    >,
    mut far: Query<(&mut Transform, &mut GlobalTransform, &mut Projection), With<FarCamera>>,
) {
    let (Ok((t, g, p)), Ok((mut ft, mut fg, mut fp))) = (main.single(), far.single_mut()) else {
        return;
    };
    if *ft != *t {
        *ft = *t;
    }
    if *fg != *g {
        *fg = *g;
    }
    if let (Projection::Perspective(a), Projection::Perspective(b)) = (p, &*fp)
        && (a.fov != b.fov || a.aspect_ratio != b.aspect_ratio)
    {
        *fp = p.clone();
    }
}

fn resize_far_target(
    world: Option<Res<WorldTarget>>,
    mut far: ResMut<FarTarget>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(world) = world else { return };
    let size = far_size(world.size);
    if size == far.size {
        return;
    }
    if let Some(mut image) = images.get_mut(&far.image) {
        image.resize(Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        });
        far.size = size;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_far_image_is_half_the_world_image() {
        assert_eq!(far_size(UVec2::new(1470, 956)), UVec2::new(735, 478));
        assert_eq!(far_size(UVec2::new(10, 10)), UVec2::splat(16));
    }

    #[test]
    fn far_meshes_and_halos_move_to_the_far_layer() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Assets<Mesh>>()
            .add_systems(Update, move_to_far_layer);
        let mesh = Mesh3d(Handle::default());
        let root = app.world_mut().spawn(FarView).id();
        let island = app.world_mut().spawn(ChildOf(root)).id();
        let far_mesh = app.world_mut().spawn((mesh.clone(), ChildOf(island))).id();
        let far_glow = app
            .world_mut()
            .spawn((Halo::new(Color::WHITE, 1.0, 1.0), ChildOf(island)))
            .id();
        let near_mesh = app.world_mut().spawn(mesh.clone()).id();
        let magic = app.world_mut().spawn(MagicPart::Ring).id();
        let rune = app.world_mut().spawn((mesh, ChildOf(magic))).id();
        app.update();
        let far = RenderLayers::layer(FAR_LAYER);
        let layer = |e| app.world().get::<RenderLayers>(e).cloned();
        assert_eq!(layer(far_mesh), Some(far.clone()));
        assert_eq!(layer(far_glow), Some(far.clone()));
        assert_eq!(layer(rune), Some(far));
        assert_eq!(layer(near_mesh), None);
        assert_eq!(layer(island), None, "only meshes and halos move");
    }
}
