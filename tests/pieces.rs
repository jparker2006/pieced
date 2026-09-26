//! Building pieces' look (docs/M2-SPEC.md → Building): the Blender brick wall
//! and plank floor and ramp fit the build grid exactly where Milestone 1's
//! pieces did (so what you see matches the unchanged colliders), every crack
//! stage and debris chunk loads, and the building visuals give each piece its
//! shared model, swap models as it cracks, and pop new pieces in.

use bevy::{
    ecs::system::RunSystemOnce,
    gltf::GltfPlugin,
    mesh::{MeshPlugin, VertexAttributeValues},
    prelude::*,
    world_serialization::{WorldAsset, WorldSerializationPlugin},
};
use pieced::{
    app::BootGate,
    building::{
        InitialCover, Piece, PieceSlot,
        visuals::{
            BuildingVisualsPlugin, POP_SECONDS, PieceAssets, PieceDebris, model_mesh,
            model_offset, piece_model, pop_scale,
        },
    },
    look::{ATTRIBUTE_OUTLINE_NORMAL, Outline, ToonMaterial, warmup::WarmupState},
    models::{ModelLibrary, ModelsPlugin},
    shared::{DamageDealt, Facing, GridCell, PieceKind},
    tuning::Tuning,
};
use std::time::Duration;

fn app() -> App {
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
    .init_asset::<Image>()
    .init_asset::<StandardMaterial>()
    .init_asset::<ToonMaterial>()
    .init_resource::<BootGate>()
    .init_resource::<WarmupState>()
    .init_resource::<Tuning>()
    .add_message::<DamageDealt>()
    .add_plugins(BuildingVisualsPlugin);
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

fn loaded() -> App {
    let mut app = app();
    update_until(&mut app, "the piece models", |app| {
        app.world().contains_resource::<PieceAssets>()
    });
    app
}

fn bounds(mesh: &Mesh) -> (Vec3, Vec3) {
    let Some(VertexAttributeValues::Float32x3(p)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
        panic!("positions");
    };
    p.iter().map(|v| Vec3::from_array(*v)).fold(
        (Vec3::MAX, Vec3::MIN),
        |(lo, hi), v| (lo.min(v), hi.max(v)),
    )
}

fn positions(mesh: &Mesh) -> Vec<Vec3> {
    let Some(VertexAttributeValues::Float32x3(p)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
        panic!("positions");
    };
    p.iter().map(|v| Vec3::from_array(*v)).collect()
}

#[test]
fn piece_models_fit_the_build_grid_in_every_crack_stage() {
    let mut app = app();
    update_until(&mut app, "models to load", |app| {
        app.world().resource::<ModelLibrary>().is_ready()
    });
    let found = app
        .world_mut()
        .run_system_once(
            |library: Res<ModelLibrary>, scenes: Res<Assets<WorldAsset>>, meshes: Res<Assets<Mesh>>| {
                let mut out = Vec::new();
                for kind in [PieceKind::Wall, PieceKind::Floor, PieceKind::Ramp] {
                    for stage in 0..3 {
                        let name = piece_model(kind, stage);
                        let scene = scenes.get(&library.get(name).unwrap().scene).unwrap();
                        let mesh = model_mesh(scene, &meshes).expect("a mesh");
                        let mesh = mesh.translated_by(model_offset(kind));
                        out.push((kind, stage, mesh));
                    }
                }
                out
            },
        )
        .unwrap();
    for (kind, stage, mesh) in found {
        let what = format!("{kind:?} stage {stage}");
        assert!(mesh.attribute(Mesh::ATTRIBUTE_COLOR).is_some(), "{what}: palette colours");
        assert!(mesh.attribute(Mesh::ATTRIBUTE_NORMAL).is_some());
        let (lo, hi) = bounds(&mesh);
        match kind {
            PieceKind::Wall => {
                // 4 m along X, 3 m tall, centred; 0.3 m thick (the collider is 0.2).
                assert!(lo.x >= -2.01 && hi.x <= 2.01, "{what}: {lo}..{hi}");
                assert!(lo.x < -1.95 && hi.x > 1.95, "{what} spans its cell: {lo}..{hi}");
                assert!(lo.y >= -1.51 && hi.y <= 1.51, "{what}: {lo}..{hi}");
                assert!(lo.y < -1.49, "{what} stands on its base: {lo}");
                // (At 33% HP two bricks hang half out of the wall.)
                let depth = if stage < 2 { 0.16 } else { 0.25 };
                assert!(lo.z >= -depth && hi.z <= depth, "{what}: {lo}..{hi}");
                if stage < 2 {
                    assert!(hi.y > 1.44, "{what} reaches its top: {hi}");
                }
            }
            PieceKind::Floor => {
                assert!(lo.x >= -2.01 && hi.x <= 2.01 && lo.z >= -2.01 && hi.z <= 2.01);
                assert!(lo.x < -1.95 && hi.x > 1.95 && lo.z < -1.95 && hi.z > 1.95);
                assert!(lo.y >= -0.105 && hi.y <= 0.15, "{what}: {lo}..{hi}");
            }
            PieceKind::Ramp => {
                assert!(lo.x >= -2.01 && hi.x <= 2.01 && lo.z >= -2.02 && hi.z <= 2.02);
                assert!(lo.y >= -0.1 && hi.y <= 3.12, "{what}: {lo}..{hi}");
                // It rises toward local -Z (the forward fix is applied).
                for p in positions(&mesh) {
                    if p.y > 2.6 {
                        assert!(p.z < -1.2, "{what}: high point {p} not at the -Z end");
                    }
                }
                // Plank tops sit on the collider's slope, never far above it.
                for p in positions(&mesh) {
                    let surface = pieced::building::ramp_surface_height(p.z);
                    assert!(p.y <= surface + 0.12, "{what}: {p} floats over the slope");
                }
            }
        }
    }
}

#[test]
fn pieces_get_their_shared_model_crack_and_pop() {
    let mut app = loaded();
    // Boot waited for the models, and the warm-up took over from there.
    assert!(
        !app.world()
            .resource::<BootGate>()
            .held()
            .any(|k| k == "pieces")
    );
    let assets = app.world().resource::<PieceAssets>().clone();
    for kind in [PieceKind::Wall, PieceKind::Floor, PieceKind::Ramp] {
        let meshes = app.world().resource::<Assets<Mesh>>();
        for stage in 0..3 {
            let mesh = meshes.get(assets.mesh(kind, stage)).expect("loaded");
            assert!(mesh.attribute(ATTRIBUTE_OUTLINE_NORMAL).is_some(), "ink outlines");
        }
    }
    assert!(app.world().contains_resource::<PieceDebris>());

    let slot = PieceSlot::wall(GridCell::new(5, 5, 0), Facing::North);
    let tuning = Tuning::default().building;
    let piece = |kind: PieceKind| Piece {
        kind,
        cell: slot.cell,
        facing: slot.facing,
        hp: tuning.max_hp(kind),
        max_hp: tuning.max_hp(kind),
        crack_stage: 0,
    };
    let placed = app
        .world_mut()
        .spawn((piece(PieceKind::Wall), slot.transform()))
        .id();
    let cover = app
        .world_mut()
        .spawn((piece(PieceKind::Ramp), InitialCover, slot.transform()))
        .id();
    app.update();
    let visual = |app: &mut App, piece: Entity| -> (Handle<Mesh>, Handle<ToonMaterial>, Transform, bool) {
        let child = app.world().get::<Children>(piece).expect("a visual child")[0];
        let e = app.world().entity(child);
        (
            e.get::<Mesh3d>().unwrap().0.clone(),
            e.get::<MeshMaterial3d<ToonMaterial>>().unwrap().0.clone(),
            *e.get::<Transform>().unwrap(),
            e.contains::<Outline>(),
        )
    };
    let (mesh, material, transform, outlined) = visual(&mut app, placed);
    assert_eq!(mesh, *assets.mesh(PieceKind::Wall, 0));
    assert_eq!(material, assets.material, "every piece shares one material");
    assert!(outlined);
    assert_eq!(transform.translation, model_offset(PieceKind::Wall));
    assert!(transform.scale.y < 0.8, "a new piece lands squashed: {}", transform.scale);
    // The initial cover is simply there, no pop.
    let (mesh, material, transform, _) = visual(&mut app, cover);
    assert_eq!(mesh, *assets.mesh(PieceKind::Ramp, 0));
    assert_eq!(material, assets.material);
    assert_eq!(transform.scale, Vec3::ONE);

    // Cracking swaps the model, stage by stage.
    for stage in [1, 2] {
        app.world_mut().get_mut::<Piece>(placed).unwrap().crack_stage = stage;
        app.update();
        let (mesh, material, ..) = visual(&mut app, placed);
        assert_eq!(mesh, *assets.mesh(PieceKind::Wall, stage));
        assert_eq!(material, assets.material);
    }
    // The pop settles to full size after 0.12 s.
    std::thread::sleep(Duration::from_secs_f32(POP_SECONDS + 0.05));
    app.update();
    app.update();
    let (_, _, transform, _) = visual(&mut app, placed);
    assert_eq!(transform.scale, Vec3::ONE);
    assert_eq!(pop_scale(POP_SECONDS), Vec3::ONE);
}
