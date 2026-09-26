//! Dresses Blender models in the look: when [`ModelSpawned`] fires, every mesh
//! below the model root swaps glTF's white `StandardMaterial` for a shared
//! vertex-coloured [`ToonMaterial`] (or [`FarMaterial`] for far models), gains
//! smooth outline normals, and — for the viewmodel — the viewmodel render layer.
//!
//! Put [`ModelLook`] on the root returned by `models::spawn_model` to choose;
//! without it a model is [`ModelLook::Near`]. Outlines still come from putting
//! [`super::Outline`] on the root. Parts that need their own material (glowing
//! crystals, eyes) are re-dressed by their owning slice after this runs: listen
//! for [`ModelDressed`].

use super::{ATTRIBUTE_OUTLINE_NORMAL, FarMaterial, ToonMaterial, with_outline_normals};
use crate::{models::ModelSpawned, render::VIEWMODEL_LAYER};
use bevy::{camera::visibility::RenderLayers, prelude::*};

/// How a spawned model is drawn.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModelLook {
    /// Toon-shaded near object (props, pieces, characters).
    #[default]
    Near,
    /// Toon-shaded, drawn only by the viewmodel camera (guns, gloves).
    Viewmodel,
    /// Unlit, hazed far object (station, ships, far islands, planet).
    Far,
}

/// Sent after a model's meshes have their look materials, so owning slices can
/// override individual parts.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct ModelDressed {
    pub root: Entity,
    pub name: String,
}

/// The shared vertex-coloured materials every model starts from.
#[derive(Resource, Debug, Clone)]
pub struct ModelMaterials {
    pub toon: Handle<ToonMaterial>,
    pub far: Handle<FarMaterial>,
}

impl FromWorld for ModelMaterials {
    fn from_world(world: &mut World) -> Self {
        let toon = world
            .resource_mut::<Assets<ToonMaterial>>()
            .add(ToonMaterial::vertex_colored());
        let far = world
            .resource_mut::<Assets<FarMaterial>>()
            .add(FarMaterial::default());
        Self { toon, far }
    }
}

pub(super) fn dress_spawned_models(
    mut spawned: MessageReader<ModelSpawned>,
    mut dressed: MessageWriter<ModelDressed>,
    looks: Query<&ModelLook>,
    children: Query<&Children>,
    std_meshes: Query<&Mesh3d, With<MeshMaterial3d<StandardMaterial>>>,
    materials: Res<ModelMaterials>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    for event in spawned.read() {
        let look = looks.get(event.root).copied().unwrap_or_default();
        for entity in children.iter_descendants(event.root) {
            let Ok(mesh) = std_meshes.get(entity) else {
                continue;
            };
            let mut e = commands.entity(entity);
            e.remove::<MeshMaterial3d<StandardMaterial>>();
            match look {
                ModelLook::Far => {
                    e.insert(MeshMaterial3d(materials.far.clone()));
                }
                ModelLook::Near | ModelLook::Viewmodel => {
                    e.insert(MeshMaterial3d(materials.toon.clone()));
                    ensure_outline_normals(&mut meshes, &mesh.0);
                }
            }
            if look == ModelLook::Viewmodel {
                e.insert(RenderLayers::layer(VIEWMODEL_LAYER));
            }
        }
        dressed.write(ModelDressed {
            root: event.root,
            name: event.name.clone(),
        });
    }
}

/// glTF meshes are shared between instances, so this runs once per mesh.
fn ensure_outline_normals(meshes: &mut Assets<Mesh>, handle: &Handle<Mesh>) {
    let Some(mesh) = meshes.get(handle) else {
        return;
    };
    if mesh.contains_attribute(ATTRIBUTE_OUTLINE_NORMAL) {
        return;
    }
    let with_normals = with_outline_normals(mesh.clone());
    let _ = meshes.insert(handle.id(), with_normals);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::{Indices, PrimitiveTopology};

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<StandardMaterial>()
            .init_asset::<ToonMaterial>()
            .init_asset::<FarMaterial>()
            .add_message::<ModelSpawned>()
            .add_message::<ModelDressed>()
            .init_resource::<ModelMaterials>()
            .add_systems(Update, dress_spawned_models);
        app
    }

    fn cube() -> Mesh {
        Cuboid::new(1.0, 1.0, 1.0).mesh().build()
    }

    fn triangle() -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, default())
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_POSITION,
                vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 3])
            .with_inserted_indices(Indices::U32(vec![0, 1, 2]))
    }

    /// A root with one glTF-style mesh grandchild, as `spawn_model` produces.
    fn spawn(app: &mut App, look: Option<ModelLook>, mesh: Mesh) -> (Entity, Entity) {
        let world = app.world_mut();
        let mesh = world.resource_mut::<Assets<Mesh>>().add(mesh);
        let white = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let root = world.spawn(Transform::default()).id();
        if let Some(look) = look {
            world.entity_mut(root).insert(look);
        }
        let scene = world.spawn((Transform::default(), ChildOf(root))).id();
        let part = world
            .spawn((Mesh3d(mesh), MeshMaterial3d(white), ChildOf(scene)))
            .id();
        world.write_message(ModelSpawned {
            root,
            name: "test".into(),
        });
        (root, part)
    }

    #[test]
    fn near_models_get_the_shared_toon_material_and_outline_normals() {
        let mut app = app();
        let (_, part) = spawn(&mut app, None, cube());
        app.update();
        let world = app.world();
        let toon = world.resource::<ModelMaterials>().toon.clone();
        let entity = world.entity(part);
        assert!(!entity.contains::<MeshMaterial3d<StandardMaterial>>());
        assert_eq!(
            entity.get::<MeshMaterial3d<ToonMaterial>>().unwrap().0,
            toon
        );
        assert!(!entity.contains::<RenderLayers>());
        let mesh = world
            .resource::<Assets<Mesh>>()
            .get(&entity.get::<Mesh3d>().unwrap().0)
            .unwrap();
        assert!(mesh.contains_attribute(ATTRIBUTE_OUTLINE_NORMAL));
    }

    #[test]
    fn far_models_get_the_far_material_without_outline_normals() {
        let mut app = app();
        let (_, part) = spawn(&mut app, Some(ModelLook::Far), triangle());
        app.update();
        let world = app.world();
        let entity = world.entity(part);
        assert!(entity.contains::<MeshMaterial3d<FarMaterial>>());
        assert!(!entity.contains::<MeshMaterial3d<ToonMaterial>>());
        let mesh = world
            .resource::<Assets<Mesh>>()
            .get(&entity.get::<Mesh3d>().unwrap().0)
            .unwrap();
        assert!(!mesh.contains_attribute(ATTRIBUTE_OUTLINE_NORMAL));
    }

    #[test]
    fn viewmodel_models_move_to_the_viewmodel_layer_and_announce_themselves() {
        let mut app = app();
        let (root, part) = spawn(&mut app, Some(ModelLook::Viewmodel), cube());
        app.update();
        let world = app.world();
        assert_eq!(
            world.entity(part).get::<RenderLayers>(),
            Some(&RenderLayers::layer(VIEWMODEL_LAYER))
        );
        let dressed: Vec<_> = world
            .resource::<Messages<ModelDressed>>()
            .iter_current_update_messages()
            .cloned()
            .collect();
        assert_eq!(
            dressed,
            [ModelDressed {
                root,
                name: "test".into()
            }]
        );
    }
}
