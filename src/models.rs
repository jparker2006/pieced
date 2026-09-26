//! Milestone 2 "Spellbound" model library (client only): loads the Blender-made
//! glTF models listed in `assets/models/manifest.json` during Boot, exposes their
//! named parts and sidecar data (attach points, part bounds), and fixes glTF's
//! +Z-forward convention to Bevy's -Z. See docs/M2-SPEC.md → Asset pipeline and
//! `assets/ASSETS.md` for how models are made (`scripts/build-art.sh`).
//!
//! **Orientation.** Models are authored facing Blender -Y, which the glTF
//! exporter writes as glTF +Z (the glTF convention). Bevy's forward is -Z and its
//! glTF loader converts nothing, so [`spawn_model`] parents the glTF scene under
//! a node turned 180° about +Y ([`MODEL_FORWARD_FIX`]). The model root you get
//! back then faces its own -Z like every Bevy entity: `Transform::looking_to`
//! works on it directly. Sidecar positions and bounds are already in this fixed
//! model space (metres, +Y up, -Z forward, +X right).
//!
//! **Embedding.** The game binary runs from outside the repo (`scripts/play.sh`
//! execs it from the build cache), so like the shaders the models are embedded
//! with `include_bytes!` and loaded from `embedded://pieced/models/…`.
//! [`EMBEDDED_MODELS`] must list every manifest entry; a test enforces it.
//!
//! **Materials.** Models arrive with glTF's `StandardMaterial` (white, rough)
//! multiplied by their `COLOR_0` palette colours. [`ModelSpawned`] is the hook
//! for the look slice to swap in the toon material; this module does no
//! material work.

use crate::app::BootGate;
use bevy::{
    asset::{RecursiveDependencyLoadState, io::embedded::EmbeddedAssetRegistry},
    ecs::system::SystemParam,
    gltf::{GltfAssetLabel, GltfLoaderSettings},
    prelude::*,
    world_serialization::{WorldAsset, WorldAssetRoot, WorldInstanceReady},
};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// The [`BootGate`] key held until every model has loaded.
pub const BOOT_KEY: &str = "models";

/// glTF +Z forward → Bevy -Z forward: a half turn about +Y, (x, y, z) → (-x, y, -z).
pub const MODEL_FORWARD_FIX: Quat = Quat::from_xyzw(0.0, 1.0, 0.0, 0.0);

/// Maps a point from glTF scene space into model space (the sidecar's space).
pub fn gltf_to_model(p: Vec3) -> Vec3 {
    MODEL_FORWARD_FIX * p
}

/// One committed model, compiled into the binary.
pub struct EmbeddedModel {
    pub name: &'static str,
    pub glb: &'static [u8],
    pub sidecar: &'static str,
}

macro_rules! embedded_models {
    ($($name:literal),* $(,)?) => {
        &[$(EmbeddedModel {
            name: $name,
            glb: include_bytes!(concat!("../assets/models/", $name, ".glb")),
            sidecar: include_str!(concat!("../assets/models/", $name, ".json")),
        }),*]
    };
}

/// Every model in `assets/models/manifest.json`. Add a line here when you add an
/// asset (the `models` tests fail until this list and the manifest agree).
pub const EMBEDDED_MODELS: &[EmbeddedModel] = embedded_models![
    "axis_probe",
    "brick_chunk",
    "floor_plank",
    "floor_plank_crack1",
    "floor_plank_crack2",
    "gloves",
    "knight",
    "plank_splinter",
    "pump",
    "ramp_plank",
    "ramp_plank_crack1",
    "ramp_plank_crack2",
    "rifle",
    "rock_a",
    "rock_b",
    "stump_a",
    "tree_a",
    "wall_brick",
    "wall_brick_crack1",
    "wall_brick_crack2",
    // The far view (src/far, art/blender/assets/far.py).
    "far_island_a",
    "far_island_b",
    "far_island_c",
    "planet",
    "ship",
    "station",
];

/// `assets/models/manifest.json`, as compiled in.
pub const MANIFEST_JSON: &str = include_str!("../assets/models/manifest.json");

/// Where embedded models live in the asset server.
pub const EMBEDDED_DIR: &str = "pieced/models";

// ---------------------------------------------------------------------------
// Manifest and sidecar
// ---------------------------------------------------------------------------

/// One `manifest.json` entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ManifestEntry {
    pub name: String,
    pub file: String,
}

pub fn parse_manifest(json: &str) -> Result<Vec<ManifestEntry>, serde_json::Error> {
    serde_json::from_str(json)
}

/// Axis-aligned bounds in model space.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Bounds {
    pub fn min(&self) -> Vec3 {
        Vec3::from_array(self.min)
    }

    pub fn max(&self) -> Vec3 {
        Vec3::from_array(self.max)
    }

    pub fn size(&self) -> Vec3 {
        self.max() - self.min()
    }

    pub fn center(&self) -> Vec3 {
        (self.min() + self.max()) / 2.0
    }
}

/// A named mesh part.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PartInfo {
    /// The parent node's name (the model root's name for top-level parts).
    pub parent: Option<String>,
    pub bounds: Bounds,
    pub triangles: u32,
    /// Palette colour names used by this part (`art/palette.json`).
    pub colors: Vec<String>,
}

/// A named attach point (a Blender empty), in model space.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AttachPoint {
    pub parent: Option<String>,
    pub position: [f32; 3],
    /// Quaternion x, y, z, w.
    pub rotation: [f32; 4],
}

impl AttachPoint {
    pub fn position(&self) -> Vec3 {
        Vec3::from_array(self.position)
    }

    pub fn transform(&self) -> Transform {
        Transform::from_translation(self.position())
            .with_rotation(Quat::from_array(self.rotation).normalize())
    }
}

/// `assets/models/<name>.json`, written by `art/blender/lib/export.py`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Sidecar {
    pub name: String,
    /// Asset kind (`rock`, `stump`, `tree`, `probe`, …); sets the triangle budget.
    pub kind: String,
    /// The Blender script that builds it.
    pub source: String,
    pub triangles: u32,
    pub triangle_budget: u32,
    pub bounds: Bounds,
    pub parts: BTreeMap<String, PartInfo>,
    pub attach: BTreeMap<String, AttachPoint>,
}

impl Sidecar {
    pub fn parse(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn attach(&self, name: &str) -> Option<&AttachPoint> {
        self.attach.get(name)
    }

    pub fn part(&self, name: &str) -> Option<&PartInfo> {
        self.parts.get(name)
    }
}

// ---------------------------------------------------------------------------
// Library
// ---------------------------------------------------------------------------

/// A loaded (or loading) model.
#[derive(Debug, Clone)]
pub struct Model {
    pub name: String,
    pub file: String,
    pub scene: Handle<WorldAsset>,
    pub sidecar: Sidecar,
}

/// Every model by name. Inserted at `Startup`; [`ModelLibrary::is_ready`] once
/// all scenes have loaded, when the `models` boot gate is released.
#[derive(Resource, Debug, Default)]
pub struct ModelLibrary {
    models: BTreeMap<String, Model>,
    ready: bool,
    failed: Vec<String>,
}

impl ModelLibrary {
    pub fn get(&self, name: &str) -> Option<&Model> {
        self.models.get(name)
    }

    pub fn sidecar(&self, name: &str) -> Option<&Sidecar> {
        self.models.get(name).map(|m| &m.sidecar)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.models.keys().map(String::as_str)
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    /// Models that failed to load (logged as errors; boot still proceeds).
    pub fn failed(&self) -> &[String] {
        &self.failed
    }
}

/// Marks a model root made by [`spawn_model`].
#[derive(Component, Debug, Clone)]
pub struct ModelRoot {
    pub name: String,
}

/// Marks the child that holds the glTF scene and the forward fix.
#[derive(Component, Debug)]
pub struct ModelScene;

/// Sent once a spawned model's glTF hierarchy exists (its parts can be found).
/// The look slice uses it to swap in the toon material.
#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub struct ModelSpawned {
    pub root: Entity,
    pub name: String,
}

/// Spawns a model: a root entity at `transform` (facing its -Z like any Bevy
/// entity) with the glTF scene under a [`MODEL_FORWARD_FIX`] child. Returns the
/// root, or `None` for an unknown name. [`ModelSpawned`] follows once the scene
/// has been instantiated.
pub fn spawn_model(
    commands: &mut Commands,
    library: &ModelLibrary,
    name: &str,
    transform: Transform,
) -> Option<Entity> {
    let model = library.get(name)?;
    let root = commands
        .spawn((
            Name::new(format!("Model {name}")),
            ModelRoot {
                name: name.to_string(),
            },
            transform,
            Visibility::default(),
        ))
        .id();
    commands.spawn((
        Name::new(format!("Model {name} scene")),
        ModelScene,
        WorldAssetRoot(model.scene.clone()),
        Transform::from_rotation(MODEL_FORWARD_FIX),
        ChildOf(root),
    ));
    Some(root)
}

/// Finds the first descendant of `root` named `part` (depth-first). glTF nodes
/// keep their Blender names (`Hat`, `Muzzle`, …); mesh primitives are separate
/// children named `<mesh>.<material>`, so an exact name finds the node.
pub fn find_part(
    root: Entity,
    part: &str,
    children: &Query<&Children>,
    names: &Query<&Name>,
) -> Option<Entity> {
    children
        .iter_descendants_depth_first(root)
        .find(|&e| names.get(e).is_ok_and(|n| n.as_str() == part))
}

/// System parameter for looking up named model parts.
#[derive(SystemParam)]
pub struct ModelParts<'w, 's> {
    children: Query<'w, 's, &'static Children>,
    names: Query<'w, 's, &'static Name>,
}

impl ModelParts<'_, '_> {
    pub fn find(&self, root: Entity, part: &str) -> Option<Entity> {
        find_part(root, part, &self.children, &self.names)
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

/// Loads every model during Boot. Needs `AssetPlugin`, `GltfPlugin` and
/// `WorldSerializationPlugin` (all in `DefaultPlugins`); no renderer.
pub struct ModelsPlugin;

impl Plugin for ModelsPlugin {
    fn build(&self, app: &mut App) {
        register_embedded(app);
        app.init_resource::<BootGate>()
            .add_message::<ModelSpawned>()
            .add_systems(Startup, start_loading)
            .add_systems(Update, track_loading)
            .add_observer(announce_spawned);
    }
}

fn register_embedded(app: &mut App) {
    let registry = app.world().resource::<EmbeddedAssetRegistry>();
    for model in EMBEDDED_MODELS {
        let path = format!("{EMBEDDED_DIR}/{}.glb", model.name);
        registry.insert_asset(PathBuf::new(), Path::new(&path), model.glb);
    }
}

/// When loading began, for the log line.
#[derive(Resource)]
struct LoadStarted(std::time::Instant);

fn start_loading(mut commands: Commands, assets: Res<AssetServer>, mut gate: ResMut<BootGate>) {
    let manifest = parse_manifest(MANIFEST_JSON).expect("assets/models/manifest.json parses");
    let mut library = ModelLibrary::default();
    for entry in manifest {
        let Some(embedded) = EMBEDDED_MODELS.iter().find(|m| m.name == entry.name) else {
            error!("models: {} is in the manifest but not embedded", entry.name);
            library.failed.push(entry.name);
            continue;
        };
        let sidecar = match Sidecar::parse(embedded.sidecar) {
            Ok(s) => s,
            Err(e) => {
                error!("models: {}.json does not parse: {e}", entry.name);
                library.failed.push(entry.name);
                continue;
            }
        };
        let path = format!("embedded://{EMBEDDED_DIR}/{}", entry.file);
        let scene = assets
            .load_builder()
            .with_settings(|s: &mut GltfLoaderSettings| {
                s.load_cameras = false;
                s.load_lights = false;
                s.load_animations = false;
            })
            .load(GltfAssetLabel::Scene(0).from_asset(path));
        library.models.insert(
            entry.name.clone(),
            Model {
                name: entry.name,
                file: entry.file,
                scene,
                sidecar,
            },
        );
    }
    gate.hold(BOOT_KEY);
    commands.insert_resource(library);
    commands.insert_resource(LoadStarted(std::time::Instant::now()));
}

fn track_loading(
    library: Option<ResMut<ModelLibrary>>,
    assets: Res<AssetServer>,
    started: Option<Res<LoadStarted>>,
    mut gate: ResMut<BootGate>,
) {
    let Some(mut library) = library else { return };
    if library.ready {
        return;
    }
    let mut pending = 0;
    let mut newly_failed = Vec::new();
    for model in library.models.values() {
        match assets.recursive_dependency_load_state(&model.scene) {
            RecursiveDependencyLoadState::Loaded => {}
            RecursiveDependencyLoadState::Failed(err) => {
                if !library.failed.contains(&model.name) {
                    error!("models: {} failed to load: {err}", model.name);
                    newly_failed.push(model.name.clone());
                }
            }
            _ => pending += 1,
        }
    }
    library.failed.extend(newly_failed);
    if pending == 0 {
        library.ready = true;
        gate.release(BOOT_KEY);
        let ms = started.map_or(0.0, |s| s.0.elapsed().as_secs_f64() * 1000.0);
        info!(
            "models: {} loaded in {ms:.0} ms ({} failed)",
            library.models.len() - library.failed.len(),
            library.failed.len()
        );
    }
}

fn announce_spawned(
    ready: On<WorldInstanceReady>,
    scenes: Query<&ChildOf, With<ModelScene>>,
    roots: Query<&ModelRoot>,
    mut spawned: MessageWriter<ModelSpawned>,
) {
    let Ok(child_of) = scenes.get(ready.entity) else {
        return;
    };
    let root = child_of.parent();
    if let Ok(model) = roots.get(root) {
        spawned.write(ModelSpawned {
            root,
            name: model.name.clone(),
        });
    }
}
