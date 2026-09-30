//! Building pieces' look (docs/M2-SPEC.md → Building): the Blender brick wall
//! and plank floor and ramp fit the build grid exactly where Milestone 1's
//! pieces did (so what you see matches the unchanged colliders), the plank
//! cone fits its pyramid, every crack stage and debris chunk loads, and the
//! building visuals give each piece its shared model, swap models as it cracks
//! or is edited, and pop new pieces in. Every edit is drawn from meshes built
//! once at load: editing, like building, creates no assets.

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
            BuildingVisualsPlugin, PieceAssets, PieceDebris, model_mesh, model_offset, piece_model,
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
    p.iter()
        .map(|v| Vec3::from_array(*v))
        .fold((Vec3::MAX, Vec3::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)))
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
            |library: Res<ModelLibrary>,
             scenes: Res<Assets<WorldAsset>>,
             meshes: Res<Assets<Mesh>>| {
                let mut out = Vec::new();
                for kind in [
                    PieceKind::Wall,
                    PieceKind::Floor,
                    PieceKind::Ramp,
                    PieceKind::Cone,
                ] {
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
        assert!(
            mesh.attribute(Mesh::ATTRIBUTE_COLOR).is_some(),
            "{what}: palette colours"
        );
        assert!(mesh.attribute(Mesh::ATTRIBUTE_NORMAL).is_some());
        let (lo, hi) = bounds(&mesh);
        match kind {
            PieceKind::Wall => {
                // 4 m along X, 3 m tall, centred; 0.3 m thick (the collider is 0.2).
                assert!(lo.x >= -2.01 && hi.x <= 2.01, "{what}: {lo}..{hi}");
                assert!(
                    lo.x < -1.95 && hi.x > 1.95,
                    "{what} spans its cell: {lo}..{hi}"
                );
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
                assert!(
                    lo.x >= -2.01 && hi.x <= 2.01 && lo.z >= -2.01 && hi.z <= 2.01,
                    "{what}: {lo}..{hi}"
                );
                assert!(lo.x < -1.95 && hi.x > 1.95 && lo.z < -1.95 && hi.z > 1.95);
                assert!(lo.y >= -0.105 && hi.y <= 0.15, "{what}: {lo}..{hi}");
            }
            PieceKind::Ramp => {
                assert!(
                    lo.x >= -2.01 && hi.x <= 2.01 && lo.z >= -2.02 && hi.z <= 2.02,
                    "{what}: {lo}..{hi}"
                );
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
            PieceKind::Cone => {
                assert!(
                    lo.x >= -2.01 && hi.x <= 2.01 && lo.z >= -2.01 && hi.z <= 2.01,
                    "{what}: {lo}..{hi}"
                );
                assert!(lo.x < -1.9 && hi.x > 1.9 && lo.z < -1.9 && hi.z > 1.9);
                assert!(lo.y >= -0.01 && hi.y <= 1.72, "{what}: {lo}..{hi}");
                // Planks and trims sit on the pyramid (1.5 m), never far above it.
                for p in positions(&mesh) {
                    let pyramid =
                        pieced::building::CONE_HEIGHT * (1.0 - p.x.abs().max(p.z.abs()) / 2.0);
                    assert!(p.y <= pyramid + 0.24, "{what}: {p} floats over the cone");
                }
            }
        }
    }
}

#[test]
fn pieces_get_their_shared_model_crack_and_pop() {
    let mut app = loaded();
    // From here on, frames are exactly 1/60 s: the pop is timed, and a slow real
    // frame (a loaded machine) would otherwise finish it before the first check.
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        Duration::from_secs_f64(1.0 / 60.0),
    ));
    // Boot waited for the models, and the warm-up took over from there.
    assert!(
        !app.world()
            .resource::<BootGate>()
            .held()
            .any(|k| k == "pieces")
    );
    let assets = app.world().resource::<PieceAssets>().clone();
    for kind in [
        PieceKind::Wall,
        PieceKind::Floor,
        PieceKind::Ramp,
        PieceKind::Cone,
    ] {
        let meshes = app.world().resource::<Assets<Mesh>>();
        for stage in 0..3 {
            let mesh = meshes.get(assets.mesh(kind, stage)).expect("loaded");
            assert!(
                mesh.attribute(ATTRIBUTE_OUTLINE_NORMAL).is_some(),
                "ink outlines"
            );
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
    let visual =
        |app: &mut App, piece: Entity| -> (Handle<Mesh>, Handle<ToonMaterial>, Transform, bool) {
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
    // One 60 Hz frame after it was placed, a new wall is still assembling
    // (M4 chunk 5: its brick courses stack up from the ground in 0.15 s).
    assert!(
        transform.scale.y < 0.95,
        "a new wall is still stacking: {}",
        transform.scale
    );
    // The initial cover is simply there, no pop.
    let (mesh, material, transform, _) = visual(&mut app, cover);
    assert_eq!(mesh, *assets.mesh(PieceKind::Ramp, 0));
    assert_eq!(material, assets.material);
    assert_eq!(transform.scale, Vec3::ONE);

    // Cracking swaps the model, stage by stage.
    for stage in [1, 2] {
        app.world_mut()
            .get_mut::<Piece>(placed)
            .unwrap()
            .crack_stage = stage;
        app.update();
        let (mesh, material, ..) = visual(&mut app, placed);
        assert_eq!(mesh, *assets.mesh(PieceKind::Wall, stage));
        assert_eq!(material, assets.material);
    }
    // It stands complete after 0.15 s (nine 60 Hz frames; step ten).
    for _ in 0..10 {
        app.update();
    }
    let (_, _, transform, _) = visual(&mut app, placed);
    assert_eq!(transform.scale, Vec3::ONE);
    let done = pieced::building::juice::assemble_pose(
        PieceKind::Wall,
        pieced::building::juice::ASSEMBLE_SECONDS,
    );
    assert_eq!(done, (Vec3::ZERO, Quat::IDENTITY, Vec3::ONE));
}

/// The whole headless game plus the look, models, building visuals and
/// effects: one fixed tick per update, no GPU.
fn game_with_visuals() -> App {
    use avian3d::prelude::PhysicsPlugins;
    use bevy::time::TimeUpdateStrategy;
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        bevy::state::app::StatesPlugin,
        AssetPlugin::default(),
        MeshPlugin,
        GltfPlugin::default(),
        WorldSerializationPlugin,
        PhysicsPlugins::default(),
    ))
    .init_asset::<Image>()
    .init_asset::<StandardMaterial>()
    .add_plugins(pieced::app::SimPlugins)
    .add_plugins((
        pieced::look::LookPlugin,
        ModelsPlugin,
        BuildingVisualsPlugin,
        pieced::fx::FxPlugin,
    ))
    .insert_resource(TimeUpdateStrategy::ManualDuration(
        pieced::shared::tick_duration(),
    ));
    app.finish();
    app.cleanup();
    app
}

/// (meshes, toon materials, ink materials, standard materials, images)
fn asset_counts(app: &App) -> [usize; 5] {
    let w = app.world();
    [
        w.resource::<Assets<Mesh>>().len(),
        w.resource::<Assets<ToonMaterial>>().len(),
        w.resource::<Assets<pieced::look::InkMaterial>>().len(),
        w.resource::<Assets<StandardMaterial>>().len(),
        w.resource::<Assets<Image>>().len(),
    ]
}

#[test]
fn building_cracking_and_breaking_fifty_pieces_creates_no_assets() {
    use pieced::{
        building::{clear_pieces, damage_piece, place_piece},
        shared::AppState,
    };
    let mut app = game_with_visuals();
    update_until(&mut app, "the piece models", |app| {
        app.world().contains_resource::<PieceAssets>()
    });
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Playing);
    for _ in 0..10 {
        app.update();
    }
    clear_pieces(app.world_mut());
    for _ in 0..5 {
        app.update();
    }
    let before = asset_counts(&app);

    // 50 pieces: walls, floors and ramps over a 5 × 4 patch of cells.
    let mut pieces = Vec::new();
    for x in 1..6 {
        for z in 1..5 {
            let c = GridCell::new(x, z, 0);
            for slot in [
                PieceSlot::wall(c, Facing::North),
                PieceSlot::floor(GridCell::new(x, z, 1)),
            ] {
                pieces.push(place_piece(app.world_mut(), slot).expect("placed"));
            }
        }
    }
    for x in 7..12 {
        for z in 1..3 {
            let slot = PieceSlot::ramp(GridCell::new(x, z, 0), Facing::North);
            pieces.push(place_piece(app.world_mut(), slot).expect("placed"));
        }
    }
    assert_eq!(pieces.len(), 50);
    for _ in 0..12 {
        app.update();
    }
    // Crack every piece twice, then break them all.
    for fraction in [0.4, 0.3] {
        for &p in &pieces {
            let max = app.world().get::<Piece>(p).unwrap().max_hp;
            damage_piece(app.world_mut(), p, max * fraction);
        }
        for _ in 0..3 {
            app.update();
        }
    }
    assert!(
        pieces
            .iter()
            .all(|&p| app.world().get::<Piece>(p).unwrap().crack_stage == 2)
    );
    for &p in &pieces {
        damage_piece(app.world_mut(), p, 10_000.0);
    }
    for _ in 0..90 {
        app.update();
    }
    assert!(pieces.iter().all(|&p| app.world().get_entity(p).is_err()));
    assert_eq!(
        asset_counts(&app),
        before,
        "placing, cracking and breaking pieces must reuse shared meshes, materials and images"
    );
}

#[test]
fn every_edit_has_shared_meshes_and_edited_pieces_show_them() {
    use pieced::building::{PieceEdit, edit::valid_edits};
    let mut app = loaded();
    let assets = app.world().resource::<PieceAssets>().clone();
    let meshes = app.world().resource::<Assets<Mesh>>();
    let mut count = 0;
    for kind in [
        PieceKind::Wall,
        PieceKind::Floor,
        PieceKind::Ramp,
        PieceKind::Cone,
    ] {
        for e in valid_edits(kind) {
            let v = assets
                .edits
                .get(&(kind, e))
                .unwrap_or_else(|| panic!("{kind:?} {e:?} has no meshes"));
            for (stage, handle) in v.meshes.iter().enumerate() {
                let mesh = meshes.get(handle).expect("loaded");
                assert!(mesh.attribute(ATTRIBUTE_OUTLINE_NORMAL).is_some());
                assert!(mesh.attribute(Mesh::ATTRIBUTE_COLOR).is_some());
                let (lo, hi) = bounds(mesh);
                let what = format!("{kind:?} {e:?} stage {stage}");
                // Composed in the full piece's model space, inside its cell
                // (frame boards may stand a little proud of a wall).
                assert!(
                    lo.x >= -2.02 && hi.x <= 2.02 && lo.z >= -2.02 && hi.z <= 2.02,
                    "{what}: {lo}..{hi}"
                );
                // No edited piece draws much more than the full piece.
                let tris = mesh.indices().map_or(0, |i| i.len() / 3);
                assert!(tris <= 1100, "{what}: {tris} triangles");
            }
            count += 1;
        }
    }
    assert_eq!(count, 22 + 14 + 8 + 14);

    // A piece swaps to its edit's mesh (turned, for ramps and cones) and back.
    let slot = PieceSlot::cone(GridCell::new(5, 5, 0));
    let tuning = Tuning::default().building;
    let piece = app
        .world_mut()
        .spawn((
            Piece {
                kind: PieceKind::Cone,
                cell: slot.cell,
                facing: slot.facing,
                hp: tuning.max_hp(PieceKind::Cone),
                max_hp: tuning.max_hp(PieceKind::Cone),
                crack_stage: 0,
            },
            PieceEdit::FULL,
            InitialCover,
            slot.transform(),
        ))
        .id();
    app.update();
    let visual = |app: &mut App| -> (Handle<Mesh>, Quat) {
        let child = app.world().get::<Children>(piece).expect("a visual child")[0];
        let e = app.world().entity(child);
        (
            e.get::<Mesh3d>().unwrap().0.clone(),
            e.get::<Transform>().unwrap().rotation,
        )
    };
    assert_eq!(visual(&mut app).0, *assets.mesh(PieceKind::Cone, 0));
    for e in [
        PieceEdit::of(&[0]),
        PieceEdit::of(&[3]),
        PieceEdit::of(&[1, 3]),
    ] {
        *app.world_mut().get_mut::<PieceEdit>(piece).unwrap() = e;
        app.update();
        let (mesh, turn) = visual(&mut app);
        let (want, want_turn) = assets.visual(PieceKind::Cone, e, 0);
        assert_eq!((mesh, turn), (want.clone(), want_turn), "{e:?}");
    }
    *app.world_mut().get_mut::<PieceEdit>(piece).unwrap() = PieceEdit::FULL;
    app.update();
    assert_eq!(
        visual(&mut app),
        (assets.mesh(PieceKind::Cone, 0).clone(), Quat::IDENTITY)
    );
}

#[test]
fn editing_and_resetting_fifty_times_creates_no_assets() {
    use pieced::{
        building::{
            PieceEdit, clear_pieces, damage_piece, edit::valid_edits, edit_piece, place_piece,
        },
        shared::AppState,
    };
    let mut app = game_with_visuals();
    update_until(&mut app, "the piece models", |app| {
        app.world().contains_resource::<PieceAssets>()
    });
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Playing);
    for _ in 0..10 {
        app.update();
    }
    clear_pieces(app.world_mut());
    for _ in 0..5 {
        app.update();
    }
    let before = asset_counts(&app);

    // One of each kind, including cones placed (and broken) along the way.
    let slots = [
        PieceSlot::wall(GridCell::new(3, 3, 0), Facing::North),
        PieceSlot::floor(GridCell::new(5, 3, 1)),
        PieceSlot::ramp(GridCell::new(7, 3, 0), Facing::East),
        PieceSlot::cone(GridCell::new(9, 3, 0)),
    ];
    let pieces: Vec<(PieceKind, Entity)> = slots
        .iter()
        .map(|s| (s.kind, place_piece(app.world_mut(), *s).expect("placed")))
        .collect();
    for _ in 0..12 {
        app.update();
    }
    let mut edits = 0;
    'outer: for round in 0..4 {
        for &(kind, piece) in &pieces {
            for e in valid_edits(kind).into_iter().skip(round).step_by(4) {
                assert!(edit_piece(app.world_mut(), piece, e));
                app.update();
                assert!(edit_piece(app.world_mut(), piece, PieceEdit::FULL));
                app.update();
                edits += 1;
                if edits >= 50 {
                    break 'outer;
                }
            }
            // Crack it a little each round, so edits land on every stage.
            let max = app.world().get::<Piece>(piece).unwrap().max_hp;
            damage_piece(app.world_mut(), piece, max * 0.2);
            app.update();
        }
    }
    assert_eq!(edits, 50);
    let cone = place_piece(app.world_mut(), PieceSlot::cone(GridCell::new(9, 5, 0))).unwrap();
    for _ in 0..3 {
        app.update();
    }
    damage_piece(app.world_mut(), cone, 10_000.0);
    for _ in 0..90 {
        app.update();
    }
    assert_eq!(
        asset_counts(&app),
        before,
        "editing and resetting must reuse the meshes built at load"
    );
}
