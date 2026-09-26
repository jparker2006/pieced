//! Model library (docs/M2-SPEC.md → Asset pipeline and Loading): the committed
//! glTF models and sidecars agree with each other and with the spec's size and
//! triangle rules, the forward fix maps a model's front to Bevy -Z, and the
//! `ModelsPlugin` loads every model headless, spawns it and finds its parts.

use bevy::{
    ecs::system::RunSystemOnce,
    gltf::GltfPlugin,
    math::Affine3A,
    mesh::{Mesh3d, MeshPlugin},
    prelude::*,
    world_serialization::WorldSerializationPlugin,
};
use pieced::{
    app::BootGate,
    models::{
        self, EMBEDDED_MODELS, MANIFEST_JSON, MODEL_FORWARD_FIX, ModelLibrary, ModelParts,
        ModelSpawned, ModelsPlugin, Sidecar, find_part, gltf_to_model, parse_manifest, spawn_model,
    },
    palette::cartoon,
};
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::PathBuf, time::Duration};

fn models_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/models")
}

fn manifest() -> Vec<models::ManifestEntry> {
    parse_manifest(&fs::read_to_string(models_dir().join("manifest.json")).unwrap()).unwrap()
}

fn sidecar(name: &str) -> Sidecar {
    let path = models_dir().join(format!("{name}.json"));
    Sidecar::parse(&fs::read_to_string(&path).unwrap())
        .unwrap_or_else(|e| panic!("{} does not parse: {e}", path.display()))
}

fn sidecars() -> BTreeMap<String, Sidecar> {
    manifest()
        .into_iter()
        .map(|e| (e.name.clone(), sidecar(&e.name)))
        .collect()
}

/// The JSON chunk of a .glb file.
fn glb_json(name: &str) -> Value {
    let bytes = fs::read(models_dir().join(format!("{name}.glb"))).unwrap();
    assert_eq!(&bytes[0..4], b"glTF", "{name}.glb is not binary glTF");
    let len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    assert_eq!(&bytes[16..20], b"JSON");
    serde_json::from_slice(&bytes[20..20 + len]).unwrap()
}

fn vec3(v: &Value) -> Vec3 {
    let a: Vec<f32> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_f64().unwrap() as f32)
        .collect();
    Vec3::new(a[0], a[1], a[2])
}

/// Each node's transform in glTF scene space (parents composed), by node name.
fn gltf_node_transforms(doc: &Value) -> BTreeMap<String, (usize, Affine3A)> {
    let nodes = doc["nodes"].as_array().unwrap();
    let local = |n: &Value| {
        let t = n.get("translation").map_or(Vec3::ZERO, vec3);
        let s = n.get("scale").map_or(Vec3::ONE, vec3);
        let r = n.get("rotation").map_or(Quat::IDENTITY, |r| {
            let a: Vec<f32> = r
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_f64().unwrap() as f32)
                .collect();
            Quat::from_xyzw(a[0], a[1], a[2], a[3])
        });
        Affine3A::from_scale_rotation_translation(s, r, t)
    };
    let mut out = BTreeMap::new();
    let mut stack: Vec<(usize, Affine3A)> = doc["scenes"][0]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| (i.as_u64().unwrap() as usize, Affine3A::IDENTITY))
        .collect();
    while let Some((i, parent)) = stack.pop() {
        let node = &nodes[i];
        let world = parent * local(node);
        if let Some(name) = node["name"].as_str() {
            out.insert(name.to_string(), (i, world));
        }
        for c in node["children"].as_array().into_iter().flatten() {
            stack.push((c.as_u64().unwrap() as usize, world));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Manifest and sidecars
// ---------------------------------------------------------------------------

#[test]
fn manifest_and_sidecars_parse_and_agree() {
    let entries = manifest();
    assert_eq!(
        fs::read_to_string(models_dir().join("manifest.json")).unwrap(),
        MANIFEST_JSON,
        "the compiled-in manifest is the committed one"
    );
    let mut names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), entries.len(), "duplicate manifest names");
    let mut embedded: Vec<&str> = EMBEDDED_MODELS.iter().map(|m| m.name).collect();
    embedded.sort_unstable();
    assert_eq!(
        embedded, names,
        "models::EMBEDDED_MODELS must list exactly the manifest's models"
    );
    for entry in &entries {
        assert_eq!(entry.file, format!("{}.glb", entry.name));
        let side = sidecar(&entry.name);
        assert_eq!(side.name, entry.name);
        assert!(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(&side.source)
                .is_file(),
            "{}: source script {} is missing",
            entry.name,
            side.source
        );
        let embedded = EMBEDDED_MODELS
            .iter()
            .find(|m| m.name == entry.name)
            .unwrap();
        assert_eq!(Sidecar::parse(embedded.sidecar).unwrap(), side);
        assert!(!side.parts.is_empty(), "{} has no parts", entry.name);
        let total: u32 = side.parts.values().map(|p| p.triangles).sum();
        assert_eq!(total, side.triangles, "{}: part triangles", entry.name);
        for (part, info) in &side.parts {
            let (lo, hi) = (info.bounds.min(), info.bounds.max());
            assert!(lo.cmple(hi).all(), "{}/{part}: inverted bounds", entry.name);
            assert!(
                lo.cmpge(side.bounds.min() - 1e-3).all()
                    && hi.cmple(side.bounds.max() + 1e-3).all(),
                "{}/{part}: part bounds outside the model bounds",
                entry.name
            );
            assert!(!info.colors.is_empty(), "{}/{part}: no colours", entry.name);
            for color in &info.colors {
                assert!(
                    cartoon::by_name(color).is_some(),
                    "{}/{part}: colour {color} is not in the palette",
                    entry.name
                );
            }
        }
    }
}

/// The glb holds what its sidecar says: the same named parts and attach points,
/// the same triangle count, and part bounds that match the mesh data.
#[test]
fn every_glb_matches_its_sidecar() {
    for (name, side) in sidecars() {
        let doc = glb_json(&name);
        let nodes = gltf_node_transforms(&doc);
        assert!(
            nodes.contains_key(&name),
            "{name}: no root node named {name}"
        );
        let mut triangles = 0;
        for (part, info) in &side.parts {
            let (i, world) = nodes
                .get(part)
                .unwrap_or_else(|| panic!("{name}: part {part} is not a glTF node"));
            let mesh = doc["nodes"][*i]["mesh"]
                .as_u64()
                .unwrap_or_else(|| panic!("{name}/{part} has no mesh"))
                as usize;
            let mut lo = Vec3::splat(f32::MAX);
            let mut hi = Vec3::splat(f32::MIN);
            for prim in doc["meshes"][mesh]["primitives"].as_array().unwrap() {
                let attrs = &prim["attributes"];
                assert!(
                    attrs.get("COLOR_0").is_some(),
                    "{name}/{part}: no COLOR_0 (palette colours)"
                );
                assert!(attrs.get("NORMAL").is_some(), "{name}/{part}: no normals");
                let pos = &doc["accessors"][attrs["POSITION"].as_u64().unwrap() as usize];
                let indices = &doc["accessors"][prim["indices"].as_u64().unwrap() as usize];
                triangles += indices["count"].as_u64().unwrap() / 3;
                let (pmin, pmax) = (vec3(&pos["min"]), vec3(&pos["max"]));
                // Transform the accessor box's corners into model space.
                for k in 0..8 {
                    let c = Vec3::new(
                        if k & 1 == 0 { pmin.x } else { pmax.x },
                        if k & 2 == 0 { pmin.y } else { pmax.y },
                        if k & 4 == 0 { pmin.z } else { pmax.z },
                    );
                    let p = gltf_to_model(world.transform_point3(c));
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
            }
            let (_, rotation, _) = world.to_scale_rotation_translation();
            if rotation.angle_between(Quat::IDENTITY) < 1e-4 {
                assert!(
                    lo.abs_diff_eq(info.bounds.min(), 2e-3)
                        && hi.abs_diff_eq(info.bounds.max(), 2e-3),
                    "{name}/{part}: glb bounds {lo}..{hi} vs sidecar {:?}",
                    info.bounds
                );
            }
        }
        assert_eq!(triangles, side.triangles as u64, "{name}: glb triangles");
        for (attach, point) in &side.attach {
            let (_, world) = nodes
                .get(attach)
                .unwrap_or_else(|| panic!("{name}: attach point {attach} is not a glTF node"));
            let p = gltf_to_model(world.translation.into());
            assert!(
                p.abs_diff_eq(point.position(), 1e-3),
                "{name}/{attach}: glb {p} vs sidecar {:?}",
                point.position
            );
        }
    }
}

/// The spec's triangle budgets per asset kind (docs/M2-SPEC.md, worst case,
/// before outlines). `probe` is the orientation fixture.
fn spec_budget(kind: &str) -> u32 {
    match kind {
        "gun" => 6000,
        "gloves" => 2000,
        "knight" => 8000,
        "wall" => 600,
        "floor" | "ramp" => 400,
        "tree" | "far_island" => 1500,
        "rock" | "stump" => 300,
        "station" => 25000,
        "ship" => 500,
        "probe" => 100,
        other => panic!("unknown asset kind {other}: add its budget from the spec"),
    }
}

#[test]
fn every_model_is_within_its_triangle_budget() {
    for (name, side) in sidecars() {
        assert_eq!(
            side.triangle_budget,
            spec_budget(&side.kind),
            "{name}: budget differs from the spec"
        );
        assert!(
            side.triangles <= side.triangle_budget,
            "{name}: {} triangles, budget {}",
            side.triangles,
            side.triangle_budget
        );
    }
}

/// D29: rocks are 1.0–1.4 m tall (crouch cover), stumps at most 0.7 m
/// (jumpable); both stand on the ground and are wide enough to hide behind or
/// stand on. Trees stand on the ground, taller than a wall.
#[test]
fn props_meet_the_d29_size_rules() {
    let all = sidecars();
    let count = |k: &str| all.values().filter(|s| s.kind == k).count();
    assert!(count("rock") >= 2, "at least two rock models");
    assert!(count("stump") >= 1, "at least one stump model");
    assert!(count("tree") >= 1, "at least one tree model");
    for side in all.values() {
        let size = side.bounds.size();
        let ground = side.bounds.min[1];
        match side.kind.as_str() {
            "rock" => {
                assert!(
                    (1.0..=1.4).contains(&size.y),
                    "{}: rock height {} m",
                    side.name,
                    size.y
                );
                assert!(
                    size.x.min(size.z) >= 0.9,
                    "{}: a rock must be wide enough to hide a crouched player ({size})",
                    side.name
                );
                assert!(
                    size.x.max(size.z) <= 2.5,
                    "{}: rock too big ({size})",
                    side.name
                );
            }
            "stump" => {
                assert!(size.y <= 0.7, "{}: stump height {} m", side.name, size.y);
                assert!(size.y >= 0.3, "{}: stump too low to read", side.name);
                assert!(
                    size.x.min(size.z) >= 0.6,
                    "{}: stump top must be wide enough to stand on ({size})",
                    side.name
                );
            }
            "tree" => assert!(
                size.y > 3.0,
                "{}: trees are taller than a wall ({size})",
                side.name
            ),
            _ => continue,
        }
        assert!(
            ground.abs() <= 0.01,
            "{}: stands on the ground (min y {ground})",
            side.name
        );
        assert!(
            side.bounds.center().x.abs() < 0.5 && side.bounds.center().z.abs() < 0.5,
            "{}: pivot near the footprint's centre",
            side.name
        );
    }
}

// ---------------------------------------------------------------------------
// Orientation
// ---------------------------------------------------------------------------

/// `axis_probe` has an empty named `Forward` 1 m in front of its front face.
/// In the glb it lies along glTF +Z (models are authored facing glTF forward);
/// the forward fix puts it on Bevy -Z, which is what `Transform::forward` means.
#[test]
fn forward_fix_maps_model_front_to_bevy_minus_z() {
    let doc = glb_json("axis_probe");
    let (_, forward) = gltf_node_transforms(&doc)["Forward"];
    let in_gltf: Vec3 = forward.translation.into();
    assert!(
        in_gltf.z > 1.0 && in_gltf.x.abs() < 1e-4,
        "authored facing glTF +Z: {in_gltf}"
    );

    let in_model = gltf_to_model(in_gltf);
    let side = sidecar("axis_probe");
    assert!(
        in_model.abs_diff_eq(side.attach("Forward").unwrap().position(), 1e-4),
        "sidecar agrees with the glb after the fix"
    );
    let dir = Vec3::new(in_model.x, 0.0, in_model.z).normalize();
    assert!(
        dir.dot(Vec3::NEG_Z) > 0.9999,
        "model front is Bevy -Z: {dir}"
    );
    assert!(dir.dot(*Transform::IDENTITY.forward()) > 0.9999);
    // 1 m beyond the front face (the probe body's front at -Z).
    let front_face = side.part("Body").unwrap().bounds.min[2];
    assert!(front_face < -0.2 + 1e-3, "the nose is on the front");
    assert!((in_model.z - (-0.2 - 1.0)).abs() < 1e-4);

    // A model root pointed somewhere with Bevy's own API puts the model's front there.
    let root = Transform::from_xyz(3.0, 0.0, -2.0).looking_to(Vec3::X, Vec3::Y);
    let world = root.transform_point(in_model);
    assert!(
        world.abs_diff_eq(Vec3::new(3.0 + 1.2, 0.2, -2.0), 1e-4),
        "{world}"
    );
    // The fix is exactly a half turn about +Y.
    assert!(
        (MODEL_FORWARD_FIX * Vec3::new(1.0, 2.0, 3.0))
            .abs_diff_eq(Vec3::new(-1.0, 2.0, -3.0), 1e-6)
    );
}

// ---------------------------------------------------------------------------
// find_part
// ---------------------------------------------------------------------------

#[test]
fn find_part_searches_descendants_by_name() {
    let mut world = World::new();
    let root = world.spawn(Name::new("Hat")).id(); // the root itself never matches
    let scene = world.spawn((Name::new("scene"), ChildOf(root))).id();
    let knight = world.spawn((Name::new("knight"), ChildOf(scene))).id();
    let torso = world.spawn((Name::new("Torso"), ChildOf(knight))).id();
    let helmet = world.spawn((Name::new("Helmet"), ChildOf(torso))).id();
    let hat = world.spawn((Name::new("Hat"), ChildOf(helmet))).id();
    world.spawn((Name::new("Hat.Toon"), ChildOf(hat))); // a mesh primitive child
    world.spawn(ChildOf(helmet)); // unnamed
    let elsewhere = world.spawn(Name::new("Elsewhere")).id();
    world.spawn((Name::new("Cape"), ChildOf(elsewhere)));

    let found = world
        .run_system_once(
            move |children: Query<&Children>, names: Query<&Name>, parts: ModelParts| {
                (
                    find_part(root, "Hat", &children, &names),
                    find_part(root, "Helmet", &children, &names),
                    find_part(root, "Cape", &children, &names),
                    find_part(root, "Ha", &children, &names),
                    parts.find(root, "Torso"),
                    parts.find(helmet, "Hat"),
                    parts.find(hat, "Hat"),
                )
            },
        )
        .unwrap();
    assert_eq!(found.0, Some(hat), "nested part, not the root");
    assert_eq!(found.1, Some(helmet));
    assert_eq!(found.2, None, "only the root's own descendants");
    assert_eq!(found.3, None, "exact names only");
    assert_eq!(found.4, Some(torso));
    assert_eq!(found.5, Some(hat));
    assert_eq!(found.6, None);
}

// ---------------------------------------------------------------------------
// Loading and spawning, headless
// ---------------------------------------------------------------------------

#[derive(Resource, Default)]
struct Seen(Vec<ModelSpawned>);

fn record(mut reader: MessageReader<ModelSpawned>, mut seen: ResMut<Seen>) {
    seen.0.extend(reader.read().cloned());
}

fn loader_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        TransformPlugin,
        MeshPlugin,
        GltfPlugin::default(),
        WorldSerializationPlugin,
        ModelsPlugin,
    ))
    .init_resource::<Seen>()
    .add_systems(Update, record);
    app.finish();
    app.cleanup();
    app
}

fn update_until(app: &mut App, what: &str, done: impl Fn(&mut App) -> bool) {
    for _ in 0..2000 {
        app.update();
        if done(app) {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn models_load_headless_and_spawn_with_their_parts() {
    let mut app = loader_app();
    app.update();
    assert!(
        app.world()
            .resource::<BootGate>()
            .held()
            .any(|k| k == "models"),
        "the models boot gate is held while loading"
    );
    update_until(&mut app, "models to load", |app| {
        app.world().resource::<ModelLibrary>().is_ready()
    });
    let library = app.world().resource::<ModelLibrary>();
    assert!(
        library.failed().is_empty(),
        "failed: {:?}",
        library.failed()
    );
    assert!(app.world().resource::<BootGate>().is_open());
    let names: Vec<String> = library.names().map(str::to_string).collect();
    assert_eq!(names.len(), manifest().len());

    // Spawn every model; the probe turned to face +X.
    let probe_at = Transform::from_xyz(3.0, 0.0, -2.0).looking_to(Vec3::X, Vec3::Y);
    let roots: BTreeMap<String, Entity> = app
        .world_mut()
        .run_system_once(move |mut commands: Commands, library: Res<ModelLibrary>| {
            let names: Vec<String> = library.names().map(str::to_string).collect();
            let mut roots = BTreeMap::new();
            for (k, name) in names.iter().enumerate() {
                let at = if name == "axis_probe" {
                    probe_at
                } else {
                    Transform::from_xyz(10.0 * k as f32, 0.0, 10.0)
                };
                roots.insert(
                    name.clone(),
                    spawn_model(&mut commands, &library, name, at).unwrap(),
                );
            }
            assert!(
                spawn_model(
                    &mut commands,
                    &library,
                    "no_such_model",
                    Transform::IDENTITY
                )
                .is_none()
            );
            roots
        })
        .unwrap();
    update_until(&mut app, "ModelSpawned for every model", |app| {
        app.world().resource::<Seen>().0.len() >= roots.len()
    });
    for _ in 0..3 {
        app.update(); // propagate transforms
    }
    let seen = &app.world().resource::<Seen>().0;
    for (name, root) in &roots {
        assert_eq!(
            seen.iter()
                .filter(|m| m.root == *root && &m.name == name)
                .count(),
            1,
            "exactly one ModelSpawned for {name}"
        );
    }

    let sidecars = sidecars();
    let found = app
        .world_mut()
        .run_system_once(
            move |parts: ModelParts,
                  globals: Query<&GlobalTransform>,
                  meshes_q: Query<&Mesh3d>,
                  children: Query<&Children>,
                  meshes: Res<Assets<Mesh>>| {
                let mut report = Vec::new();
                for (name, root) in &roots {
                    let side = &sidecars[name];
                    for part in side.parts.keys() {
                        let entity = parts.find(*root, part);
                        let colored = entity.is_some_and(|e| {
                            children.iter_descendants(e).chain([e]).any(|d| {
                                meshes_q.get(d).is_ok_and(|m| {
                                    meshes.get(&m.0).is_some_and(|m| {
                                        m.attribute(Mesh::ATTRIBUTE_COLOR).is_some()
                                    })
                                })
                            })
                        });
                        report.push((format!("{name}/{part}"), entity.is_some(), colored));
                    }
                    for attach in side.attach.keys() {
                        let entity = parts.find(*root, attach);
                        report.push((format!("{name}/{attach}"), entity.is_some(), true));
                    }
                }
                let forward = parts
                    .find(roots["axis_probe"], "Forward")
                    .map(|e| globals.get(e).unwrap().translation());
                (report, forward)
            },
        )
        .unwrap();
    for (what, exists, colored) in &found.0 {
        assert!(exists, "{what} is not in the spawned hierarchy");
        assert!(colored, "{what} has no vertex colours after loading");
    }
    let forward = found.1.expect("axis_probe has a Forward node");
    assert!(
        forward.abs_diff_eq(Vec3::new(3.0 + 1.2, 0.2, -2.0), 1e-3),
        "a model turned to face +X has its Forward point 1.2 m along +X: {forward}"
    );
}
