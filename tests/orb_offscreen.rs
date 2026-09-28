//! Offscreen review of the knight's wand and orb in the real client look
//! (docs/M3-SPEC.md → The orb; The grunt, item 10): runs the look, model,
//! figure and effect plugins with no window, renders into the world target
//! image and writes PNGs: the knight holding his wand at rest, mid wind-up
//! (arm raised, crystal glowing), the orb leaving the wand, the orb flying at
//! the player, and the orb bursting on a wall.
//!
//! It needs a GPU, so it is ignored by default. Run it by hand:
//!
//! ```sh
//! PIECED_LOOK_OUT=/some/dir cargo test --locked --test orb_offscreen -- --ignored --nocapture
//! ```

use avian3d::prelude::PhysicsPlugins;
use bevy::{
    log::LogPlugin,
    prelude::*,
    render::{
        RenderApp, RenderPlugin,
        pipelined_rendering::PipelinedRenderingPlugin,
        render_resource::{CachedPipelineState, PipelineCache},
        settings::{Backends, RenderCreation, WgpuSettings},
        view::screenshot::{Screenshot, save_to_disk},
    },
    time::TimeUpdateStrategy,
    window::ExitCondition,
    winit::WinitPlugin,
};
use pieced::{
    app::{BootPlugin, SimPlugins},
    arena::visuals::{ArenaVisualsPlugin, wand::FigureWand},
    building::{BuildingVisualsPlugin, PieceSlot, clear_pieces, place_piece},
    dummy::{Dummy, look_toward},
    fx::FxPlugin,
    look::LookPlugin,
    models::ModelsPlugin,
    orb::{Orb, Wand},
    player::spawn_character,
    render::{RenderSetupPlugin, WorldTarget},
    scenario::gallery::GalleryCamera,
    shared::{
        AppState, EyeHeight, Facing, GridCell, Health, Player, PlayerIntent, PreviousFeet,
        tick_duration,
    },
    tuning::Tuning,
    viewmodel::ViewmodelPlugin,
};
use std::path::{Path, PathBuf};

fn review_app() -> App {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .build()
            .disable::<WinitPlugin>()
            .disable::<PipelinedRenderingPlugin>()
            .disable::<bevy::audio::AudioPlugin>()
            .disable::<LogPlugin>()
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                close_when_requested: false,
                ..default()
            })
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
                    backends: Some(Backends::METAL),
                    ..default()
                })),
                synchronous_pipeline_compilation: true,
                ..default()
            }),
    )
    .add_plugins(PhysicsPlugins::default())
    .add_plugins(SimPlugins)
    .add_plugins((
        RenderSetupPlugin,
        LookPlugin,
        ModelsPlugin,
        ArenaVisualsPlugin,
        BuildingVisualsPlugin,
        ViewmodelPlugin,
        FxPlugin,
        BootPlugin,
    ))
    // One fixed tick per frame, so the wind-up can be caught mid-way.
    .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()));
    app.finish();
    app.cleanup();
    app
}

fn frames(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

/// Captures the world image with game time paused (the pose holds still).
fn capture(app: &mut App, path: PathBuf) {
    let _ = std::fs::remove_file(&path);
    app.world_mut().resource_mut::<Time<Virtual>>().pause();
    let image = app.world().resource::<WorldTarget>().image.clone();
    app.world_mut()
        .spawn(Screenshot::image(image))
        .observe(save_to_disk(path.clone()));
    for _ in 0..30 {
        app.update();
        if path.exists() {
            app.update();
            app.world_mut().resource_mut::<Time<Virtual>>().unpause();
            println!("wrote {}", path.display());
            return;
        }
    }
    panic!("no screenshot at {}", path.display());
}

fn find<C: Component>(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<C>>()
        .iter(world)
        .next()
        .unwrap()
}

fn teleport(app: &mut App, who: Entity, at: Vec3) {
    let mut e = app.world_mut().entity_mut(who);
    e.get_mut::<Transform>().unwrap().translation = at;
    e.get_mut::<PreviousFeet>().unwrap().0 = at;
}

fn camera(app: &mut App, eye: Vec3, at: Vec3) {
    app.insert_resource(GalleryCamera(
        Transform::from_translation(eye).looking_at(at, Vec3::Y),
    ));
}

fn active_orb(app: &mut App) -> Option<Vec3> {
    let world = app.world_mut();
    world
        .query_filtered::<&Transform, With<Orb>>()
        .iter(world)
        .next()
        .map(|t| t.translation)
}

fn step_until(app: &mut App, what: &str, done: impl Fn(&mut App) -> bool) {
    for _ in 0..600 {
        app.update();
        if done(app) {
            return;
        }
    }
    panic!("{what}: never happened");
}

fn assert_pipelines_ok(app: &App) {
    let render = app.sub_app(RenderApp).world();
    let cache = render.resource::<PipelineCache>();
    let errors: Vec<String> = cache
        .pipelines()
        .filter_map(|p| match &p.state {
            CachedPipelineState::Err(err) => Some(format!("{err}")),
            _ => None,
        })
        .collect();
    assert!(
        errors.is_empty(),
        "pipeline errors:\n{}",
        errors.join("\n\n")
    );
}

fn review(app: &mut App, out: &Path) {
    clear_pieces(app.world_mut());
    app.world_mut().resource_mut::<Tuning>().dummy.stand_still = true;
    let dummy = find::<Dummy>(app);
    teleport(app, dummy, Vec3::new(-16.0, 0.0, -16.0));
    let player = find::<Player>(app);
    let player_feet = Vec3::new(2.0, 0.0, 12.0);
    teleport(app, player, player_feet);
    let eye_height = app.world().get::<EyeHeight>(player).map_or(1.62, |e| e.0);
    let player_eye = player_feet + Vec3::Y * eye_height;

    // The knight, 9 m north of the player, facing him.
    let knight_feet = Vec3::new(2.0, 0.0, 3.0);
    let look = look_toward(player_feet + Vec3::Y * 0.9 - (knight_feet + Vec3::Y * 1.62));
    let knight = {
        let world = app.world_mut();
        let mut commands = world.commands();
        let k = spawn_character(
            &mut commands,
            knight_feet,
            look,
            Health::full(100.0, 0.0),
            (Wand::new(2.5),),
        );
        world.flush();
        k
    };
    step_until(app, "the wand dressed", |app| {
        let world = app.world_mut();
        world
            .query::<&FigureWand>()
            .iter(world)
            .any(|w| w.crystal.is_some())
    });
    frames(app, 20);
    let chest = knight_feet + Vec3::Y * 1.1;
    let three_quarter = knight_feet + Vec3::new(2.2, 1.5, 2.6);

    camera(app, three_quarter, chest);
    frames(app, 10);
    capture(app, out.join("wand-rest.png"));

    // Mid wind-up: the arm raised, the crystal glowing.
    app.world_mut()
        .get_mut::<PlayerIntent>(knight)
        .unwrap()
        .fire = true;
    step_until(app, "wind-up at 80%", |app| {
        app.world()
            .get::<Wand>(knight)
            .and_then(|w| w.windup_progress(0.4))
            .is_some_and(|p| p >= 0.8)
    });
    capture(app, out.join("wand-windup.png"));

    // The release, seen from the side.
    step_until(app, "the orb leaves", |app| active_orb(app).is_some());
    frames(app, 2);
    let mid = (knight_feet + player_feet) / 2.0 + Vec3::Y * 1.2;
    camera(app, mid + Vec3::new(6.5, 0.6, -2.0), mid - Vec3::Z * 1.5);
    frames(app, 1);
    capture(app, out.join("orb-release.png"));

    // Incoming, from the player's eye.
    camera(app, player_eye, knight_feet + Vec3::Y * 1.2);
    step_until(app, "the next orb, 4 m out", |app| {
        active_orb(app).is_some_and(|p| p.distance(player_eye) < 4.5)
    });
    capture(app, out.join("orb-incoming.png"));
    frames(app, 30);

    // A wall in the way: the burst and brick chips.
    // Two cells in front of the player (clear of him).
    let cell = GridCell::containing(player_feet - Vec3::Z * 3.0);
    place_piece(app.world_mut(), PieceSlot::wall(cell, Facing::North)).unwrap();
    let wall_z = cell.min_corner().z;
    camera(
        app,
        Vec3::new(5.5, 2.0, wall_z - 3.5),
        Vec3::new(2.0, 1.0, wall_z),
    );
    step_until(app, "an orb near the wall", |app| {
        active_orb(app).is_some_and(|p| (p.z - wall_z).abs() < 1.2)
    });
    step_until(app, "the orb stops", |app| active_orb(app).is_none());
    frames(app, 3);
    capture(app, out.join("orb-wall.png"));
}

#[test]
#[ignore = "needs a GPU: run by hand to review the wand and orb"]
fn render_the_wand_and_orb_offscreen() {
    let out = std::env::var("PIECED_LOOK_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("pieced-orb"));
    std::fs::create_dir_all(&out).unwrap();
    let mut app = review_app();
    let mut boot = 0;
    while *app.world().resource::<State<AppState>>().get() == AppState::Boot {
        app.update();
        boot += 1;
        assert!(boot < 400, "Boot never ended");
    }
    frames(&mut app, 10);
    review(&mut app, &out);
    assert_pipelines_ok(&app);
    println!("wrote PNGs to {}", out.display());
}
