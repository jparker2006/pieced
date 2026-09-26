//! Offscreen review of the island slice (pieces, props, island, barrier, grid)
//! with the real client look plugins and the Blender models, rendered on the
//! GPU with no window. Views match the targets T01 (spawn vista), T09 (a 1×1
//! fort and the ghost) and T11 (the island edge and barrier), plus close-ups
//! of the crack stages, a break burst and an overview.
//!
//! It needs a GPU, so it is ignored by default. Run it by hand:
//!
//! ```sh
//! PIECED_LOOK_OUT=/some/dir cargo test --locked --test look_island -- --ignored --nocapture
//! ```

use avian3d::prelude::PhysicsPlugins;
use bevy::{
    prelude::*,
    render::{
        RenderApp, RenderPlugin,
        pipelined_rendering::PipelinedRenderingPlugin,
        render_resource::{CachedPipelineState, PipelineCache},
        settings::{Backends, RenderCreation, WgpuSettings},
        view::screenshot::{Screenshot, save_to_disk},
    },
    window::ExitCondition,
    winit::WinitPlugin,
};
use pieced::{
    app::{BootPlugin, SimPlugins},
    arena::visuals::ArenaVisualsPlugin,
    building::{BuildingVisualsPlugin, Piece, PieceSlot, clear_pieces, damage_piece, place_piece},
    far::FarViewPlugin,
    fx::FxPlugin,
    look::LookPlugin,
    models::ModelsPlugin,
    render::{RenderSetupPlugin, WorldTarget},
    scenario::gallery::GalleryCamera,
    shared::{ActiveTool, AppState, Facing, GridCell, LookAngles, PieceKind, Player},
    viewmodel::ViewmodelPlugin,
};
use std::path::{Path, PathBuf};

fn look_app() -> App {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .build()
            .disable::<WinitPlugin>()
            .disable::<PipelinedRenderingPlugin>()
            .disable::<bevy::audio::AudioPlugin>()
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
        FarViewPlugin,
        BuildingVisualsPlugin,
        ViewmodelPlugin,
        FxPlugin,
        BootPlugin,
    ));
    app.finish();
    app.cleanup();
    app
}

fn capture(app: &mut App, path: PathBuf) {
    let _ = std::fs::remove_file(&path);
    let image = app.world().resource::<WorldTarget>().image.clone();
    app.world_mut()
        .spawn(Screenshot::image(image))
        .observe(save_to_disk(path.clone()));
    for _ in 0..30 {
        app.update();
        if path.exists() {
            app.update();
            return;
        }
    }
    panic!("no screenshot at {}", path.display());
}

fn frames(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

fn look_from(app: &mut App, eye: Vec3, at: Vec3) {
    app.insert_resource(GalleryCamera(
        Transform::from_translation(eye).looking_at(at, Vec3::Y),
    ));
    frames(app, 4);
}

/// First person: the player stands at `feet` looking `yaw`/`pitch` (radians).
fn first_person(app: &mut App, feet: Vec3, yaw: f32, pitch: f32, tool: ActiveTool) {
    app.world_mut().remove_resource::<GalleryCamera>();
    let world = app.world_mut();
    let mut q =
        world.query_filtered::<(&mut Transform, &mut LookAngles, &mut ActiveTool), With<Player>>();
    let (mut t, mut look, mut active) = q.single_mut(world).unwrap();
    t.translation = feet;
    look.yaw = yaw;
    look.pitch = pitch;
    *active = tool;
    frames(app, 20);
}

fn boot(app: &mut App) {
    let mut boot_frames = 0;
    while *app.world().resource::<State<AppState>>().get() == AppState::Boot {
        app.update();
        boot_frames += 1;
        assert!(boot_frames < 600, "Boot never ended");
    }
    println!("Boot ended after {boot_frames} frames");
    frames(app, 10);
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

fn place(app: &mut App, slot: PieceSlot) -> Entity {
    place_piece(app.world_mut(), slot).unwrap_or_else(|e| panic!("{slot:?}: {e:?}"))
}

fn crack(app: &mut App, piece: Entity, stage: u8) {
    let max = app.world().get::<Piece>(piece).unwrap().max_hp;
    let fraction = [1.0, 0.6, 0.3][stage as usize];
    damage_piece(app.world_mut(), piece, max * (1.0 - fraction));
}

fn review(app: &mut App, out: &Path) {
    let rifle = ActiveTool::Weapon(pieced::shared::WeaponKind::Rifle);

    // T01: the spawn vista, first person from the spawn looking north, and
    // turned a little left toward the nearest rock.
    first_person(app, Vec3::new(2.0, 0.0, 14.0), 0.0, -0.06, rifle);
    capture(app, out.join("island-01-spawn-vista.png"));
    first_person(app, Vec3::new(2.0, 0.0, 14.0), 0.42, -0.08, rifle);
    capture(app, out.join("island-01b-spawn-left.png"));

    // T09: a 1×1 fort (three brick walls, a plank roof, a ramp inside) with a
    // valid wall ghost beside it, on the next cell's edge.
    clear_pieces(app.world_mut());
    let c = GridCell::new(5, 6, 0); // x -4..0, z 0..4
    for f in [Facing::West, Facing::North, Facing::East] {
        place(app, PieceSlot::wall(c, f));
    }
    place(app, PieceSlot::floor(GridCell::new(5, 6, 1)));
    place(app, PieceSlot::ramp(c, Facing::North));
    frames(app, 30);
    first_person(
        app,
        Vec3::new(1.2, 0.0, 7.6),
        0.5,
        0.02,
        ActiveTool::Build(PieceKind::Wall),
    );
    capture(app, out.join("island-02-fort-ghost.png"));

    // The invalid ghost: aiming at a wall that's already there (occupied).
    place(app, PieceSlot::wall(GridCell::new(5, 7, 0), Facing::North));
    first_person(
        app,
        Vec3::new(-2.0, 0.0, 7.0),
        0.0,
        -0.05,
        ActiveTool::Build(PieceKind::Wall),
    );
    capture(app, out.join("island-03-ghost-invalid.png"));

    // Crack stages (100%, 60%, 30% from left to right): walls, floors, ramps.
    clear_pieces(app.world_mut());
    first_person(app, Vec3::new(2.0, 0.0, 14.0), 0.0, 0.0, rifle);
    for (i, stage) in [0u8, 1, 2].into_iter().enumerate() {
        let x = 2 + 2 * i as i32;
        let wall = place(app, PieceSlot::wall(GridCell::new(x, 3, 0), Facing::South));
        let floor = place(app, PieceSlot::floor(GridCell::new(x, 5, 0)));
        let ramp = place(app, PieceSlot::ramp(GridCell::new(x, 7, 0), Facing::North));
        frames(app, 2);
        for p in [wall, floor, ramp] {
            crack(app, p, stage);
        }
    }
    frames(app, 30);
    look_from(app, Vec3::new(-6.0, 7.0, 17.0), Vec3::new(-6.0, 0.5, 0.0));
    capture(app, out.join("island-04-crack-stages.png"));
    look_from(app, Vec3::new(-6.0, 2.0, -1.0), Vec3::new(-6.0, 1.4, -8.0));
    capture(app, out.join("island-05-walls-close.png"));

    // A break burst: bricks and dust.
    let wall = place(app, PieceSlot::wall(GridCell::new(5, 9, 0), Facing::North));
    frames(app, 10);
    look_from(app, Vec3::new(1.5, 1.8, 19.0), Vec3::new(-2.0, 1.0, 12.0));
    damage_piece(app.world_mut(), wall, 10_000.0);
    frames(app, 7);
    capture(app, out.join("island-06-break-burst.png"));

    // T11: the island edge on the east side, the barrier close by.
    clear_pieces(app.world_mut());
    frames(app, 5);
    first_person(app, Vec3::new(21.6, 0.0, 10.0), -0.62, -0.14, rifle);
    capture(app, out.join("island-07-island-edge.png"));
    // Touching the barrier.
    first_person(app, Vec3::new(23.4, 0.0, 4.0), -1.25, -0.05, rifle);
    capture(app, out.join("island-08-barrier-touch.png"));

    // Props, blob shadows and base clumps.
    look_from(app, Vec3::new(-5.0, 1.7, 12.8), Vec3::new(-8.0, 0.4, 9.5));
    capture(app, out.join("island-09-props.png"));

    // Overview: the island top, margin trees, cliffs dropping into space.
    look_from(app, Vec3::new(52.0, 30.0, 58.0), Vec3::new(0.0, -6.0, 0.0));
    capture(app, out.join("island-10-overview.png"));
    look_from(app, Vec3::new(40.0, -2.0, 30.0), Vec3::new(20.0, -8.0, 6.0));
    capture(app, out.join("island-11-cliffs.png"));
    // The margin from inside: trees, bushes, rocks beyond the edge.
    look_from(
        app,
        Vec3::new(-18.0, 2.2, 16.0),
        Vec3::new(-30.0, 1.0, 26.0),
    );
    capture(app, out.join("island-12-margin.png"));
}

#[test]
#[ignore = "needs a GPU: run by hand to review the island slice"]
fn render_the_island_offscreen() {
    let out = std::env::var("PIECED_LOOK_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("pieced-look"));
    std::fs::create_dir_all(&out).unwrap();
    let mut app = look_app();
    boot(&mut app);
    review(&mut app, &out);
    assert_pipelines_ok(&app);
    println!("wrote PNGs to {}", out.display());
}
