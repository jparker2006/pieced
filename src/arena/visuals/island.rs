//! Spawns the floating island (client only): the painted grassy top with the
//! build grid ([`GroundMaterial`]), the cliff skirt, grass tufts, flowers,
//! pebbles and bushes (merged meshes from [`super::scenery`]), the cloud sea
//! (unlit [`FarMaterial`], so the far knob hides it with the rest of the far
//! layer), the barrier, and the Blender models standing on it: the solid
//! arena props (D29, at [`ARENA_PROPS`], whose colliders `arena` spawns
//! headless) and the margin's trees, big rocks and stumps. Models are
//! toon-dressed with ink outlines and blob shadows. Everything is spawned
//! once; nothing here runs per frame.

use super::{
    PluggedInOnly,
    barrier::{self, BarrierMaterial},
    geo::Geo,
    scenery::{ISLAND_TRIANGLE_BUDGET, Island},
};
use crate::{
    app::BootGate,
    arena::ARENA_PROPS,
    look::{
        BlobShadow, FarMaterial, GroundMaterial, Outline, ToonMaterial, preset_look,
        with_outline_normals,
    },
    models::{ModelLibrary, ModelsPlugin, spawn_model},
    tuning::Tuning,
};
use bevy::{asset::io::embedded::EmbeddedAssetRegistry, prelude::*};
use std::path::{Path, PathBuf};

/// The [`BootGate`] key held until the island's models are placed.
pub const ISLAND_GATE: &str = "island";

pub struct IslandPlugin;

impl Plugin for IslandPlugin {
    fn build(&self, app: &mut App) {
        app.world()
            .resource::<EmbeddedAssetRegistry>()
            .insert_asset(
                PathBuf::new(),
                Path::new("pieced/shaders/barrier.wgsl"),
                include_bytes!("../../../assets/shaders/barrier.wgsl").as_slice(),
            );
        app.add_plugins(MaterialPlugin::<BarrierMaterial>::default())
            .add_systems(Startup, (spawn_island, barrier::spawn_barrier))
            .add_systems(
                Update,
                (
                    spawn_island_models.run_if(not(resource_exists::<IslandModelsPlaced>)),
                    barrier::show_barrier_near_camera,
                ),
            );
    }

    fn finish(&self, app: &mut App) {
        // Boot waits for the models only when there are models to wait for.
        if app.is_plugin_added::<ModelsPlugin>() {
            app.init_resource::<BootGate>();
            app.world_mut().resource_mut::<BootGate>().hold(ISLAND_GATE);
        }
    }
}

/// Marks a spawned island model root (arena prop or margin decor).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum IslandModel {
    /// One of the solid arena props (its collider is in `arena`).
    Prop,
    /// Margin scenery beyond the barrier.
    Decor,
}

/// Inserted once the island's models have been spawned.
#[derive(Resource, Debug)]
pub struct IslandModelsPlaced;

/// The generated island, kept for its model placements.
#[derive(Resource, Debug)]
pub struct IslandLayout(pub Island);

fn spawn_part<M: Material>(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    name: &'static str,
    geo: Geo,
    material: &Handle<M>,
    outlined: bool,
) -> Option<Entity> {
    if geo.is_empty() {
        return None;
    }
    let mesh = if outlined {
        with_outline_normals(geo.into_mesh())
    } else {
        geo.into_mesh()
    };
    let mut e = commands.spawn((
        Name::new(name),
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material.clone()),
        Transform::IDENTITY,
    ));
    if outlined {
        e.insert(Outline::default());
    }
    Some(e.id())
}

fn spawn_island(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut toon: ResMut<Assets<ToonMaterial>>,
    mut grounds: ResMut<Assets<GroundMaterial>>,
    mut far: ResMut<Assets<FarMaterial>>,
    tuning: Res<Tuning>,
) {
    let island = Island::generate();
    let triangles = island.triangles();
    info!(
        "island: {triangles} triangles of ground, cliffs, clutter and clouds, {} margin models",
        island.decor.len()
    );
    if triangles > ISLAND_TRIANGLE_BUDGET {
        warn!("island: {triangles} triangles is over its budget of {ISLAND_TRIANGLE_BUDGET}");
    }
    let ground = grounds.add(GroundMaterial::default());
    let cliffs = toon.add(ToonMaterial::vertex_colored());
    // Grass tufts and flowers are single triangles seen from both sides; their
    // normals point up so they shade like the grass they grow from.
    let grass = toon.add(ToonMaterial::vertex_colored().double_sided().with_rim(0.0));
    let pebbles = toon.add(ToonMaterial::vertex_colored().with_rim(0.0));
    // Clouds are unlit and only lightly hazed: their vertex colours carry
    // their soft shading, and they stay white against the galaxy.
    let clouds = far.add(FarMaterial::default().with_haze(0.5));

    let Island {
        ground: top,
        skirt,
        tufts,
        dense_tufts,
        flowers,
        pebbles: stones,
        bushes,
        prop_bushes,
        clouds: cloud_sectors,
        decor,
    } = island;
    let (c, m) = (&mut commands, &mut *meshes);
    spawn_part(c, m, "Island top", top, &ground, false);
    spawn_part(c, m, "Island cliffs", skirt, &cliffs, true);
    spawn_part(c, m, "Bushes", bushes, &cliffs, true);
    spawn_part(c, m, "Prop bushes", prop_bushes, &cliffs, true);
    spawn_part(c, m, "Pebbles", stones, &pebbles, false);
    for chunk in flowers {
        spawn_part(c, m, "Flowers", chunk, &grass, false);
    }
    for chunk in tufts {
        spawn_part(c, m, "Grass tufts", chunk, &grass, false);
    }
    for sector in cloud_sectors {
        spawn_part(c, m, "Clouds", sector, &clouds, false);
    }
    let dense = preset_look(tuning.graphics.preset).dense_grass;
    let mut extras = Vec::new();
    for chunk in dense_tufts {
        extras.extend(spawn_part(c, m, "Dense grass tufts", chunk, &grass, false));
    }
    for e in extras {
        commands.entity(e).insert((
            PluggedInOnly,
            if dense {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            },
        ));
    }
    commands.insert_resource(IslandLayout(Island { decor, ..default() }));
}

/// Places the arena props and the margin's models once the library has
/// loaded them, then lets Boot go on.
fn spawn_island_models(
    mut commands: Commands,
    library: Option<Res<ModelLibrary>>,
    layout: Option<Res<IslandLayout>>,
    gate: Option<ResMut<BootGate>>,
) {
    let (Some(library), Some(layout)) = (library, layout) else {
        return;
    };
    if !library.is_ready() {
        return;
    }
    for prop in ARENA_PROPS {
        let name = prop.kind.model();
        if let Some(root) = spawn_model(&mut commands, &library, name, prop.transform()) {
            let radius = prop.kind.footprint_radius();
            commands.entity(root).insert((
                IslandModel::Prop,
                Outline::default(),
                BlobShadow::new(radius * 1.4),
            ));
        }
    }
    for decor in &layout.0.decor {
        if let Some(root) = spawn_model(&mut commands, &library, decor.model, decor.transform) {
            commands.entity(root).insert((
                IslandModel::Decor,
                Outline::default(),
                BlobShadow::new(decor.shadow),
            ));
        }
    }
    commands.insert_resource(IslandModelsPlaced);
    if let Some(mut gate) = gate {
        gate.release(ISLAND_GATE);
    }
}
