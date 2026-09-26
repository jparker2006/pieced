//! Offscreen look review: runs the real client look plugins with no window,
//! renders into the world target image, checks that every render pipeline
//! compiled (the WGSL of every look material, on this GPU), that Boot's
//! pipeline warm-up finished quickly, and writes PNGs of a few views for review:
//! first with the default hull outlines, then with `--knobs outline=mod`
//! (bevy_mod_outline) for comparison.
//!
//! It needs a GPU, so it is ignored by default. Run it by hand:
//!
//! ```sh
//! PIECED_LOOK_OUT=/some/dir cargo test --locked --test look_offscreen -- --ignored --nocapture
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
    window::ExitCondition,
    winit::WinitPlugin,
};
use pieced::{
    app::{BootPlugin, SimPlugins},
    arena::visuals::ArenaVisualsPlugin,
    building::{BuildingVisualsPlugin, Piece},
    fx::FxPlugin,
    look::{Halo, LookPlugin, LookSettings, OutlineBackend},
    models::ModelsPlugin,
    perf_knobs::PerfKnobs,
    render::{RenderSetupPlugin, WorldTarget},
    scenario::gallery::GalleryCamera,
    shared::{ActiveTool, AppState, Character, PieceKind, Player},
    viewmodel::ViewmodelPlugin,
};
use std::path::{Path, PathBuf};

fn look_app(knobs: Option<&str>, log: bool) -> App {
    let mut app = App::new();
    if let Some(knobs) = knobs {
        // Before the plugins: `LookPlugin` decides at build time whether to add
        // bevy_mod_outline.
        app.insert_resource(PerfKnobs::parse(knobs));
    }
    let mut plugins = DefaultPlugins
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
        });
    if !log {
        plugins = plugins.disable::<LogPlugin>();
    }
    app.add_plugins(plugins)
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
            // Let the writer finish.
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

/// Runs Boot and asserts the warm-up let go quickly.
fn boot(app: &mut App) {
    let mut boot_frames = 0;
    while *app.world().resource::<State<AppState>>().get() == AppState::Boot {
        app.update();
        boot_frames += 1;
        assert!(boot_frames < 120, "Boot never ended");
    }
    println!("Boot ended after {boot_frames} frames");
    assert!(
        boot_frames <= 20,
        "warm-up held Boot for {boot_frames} frames"
    );
    frames(app, 10);
}

/// Every pipeline created so far compiled on this GPU.
fn assert_pipelines_ok(app: &App) {
    let render = app.sub_app(RenderApp).world();
    let cache = render.resource::<PipelineCache>();
    let mut errors = Vec::new();
    let (mut ok, mut pending) = (0, 0);
    for pipeline in cache.pipelines() {
        match &pipeline.state {
            CachedPipelineState::Err(err) => errors.push(format!("{err}")),
            CachedPipelineState::Ok(_) => ok += 1,
            _ => pending += 1,
        }
    }
    println!(
        "pipelines: {ok} ok, {pending} pending, {} failed",
        errors.len()
    );
    assert!(
        errors.is_empty(),
        "pipeline errors:\n{}",
        errors.join("\n\n")
    );
}

fn dummy_position(app: &mut App) -> Vec3 {
    let world = app.world_mut();
    let mut q = world.query_filtered::<&Transform, (With<Character>, Without<Player>)>();
    q.iter(world).next().map(|t| t.translation).unwrap()
}

fn nearest_piece(app: &mut App, to: Vec3) -> Vec3 {
    let world = app.world_mut();
    let mut q = world.query_filtered::<&GlobalTransform, With<Piece>>();
    q.iter(world)
        .map(|t| t.translation())
        .min_by(|a, b| a.distance(to).total_cmp(&b.distance(to)))
        .unwrap()
}

/// The review views: spawn (first person), the dummy close up, a piece group
/// from its lit and shadow sides, an overview, and the build ghost.
fn review(app: &mut App, out: &Path, prefix: &str, all: bool) {
    let dummy = dummy_position(app);
    // A halo to review, beside the dummy's head.
    app.world_mut().spawn((
        Halo::new(Color::srgb(0.35, 0.8, 1.0), 0.9, 1.6),
        Transform::from_translation(dummy + Vec3::new(1.2, 1.6, 0.0)),
    ));
    frames(app, 3);
    capture(app, out.join(format!("{prefix}01-spawn.png")));

    look_from(
        app,
        dummy + Vec3::new(-2.2, 1.6, 3.4),
        dummy + Vec3::new(0.3, 0.9, 0.0),
    );
    capture(app, out.join(format!("{prefix}02-dummy.png")));

    let piece = nearest_piece(app, Vec3::new(2.0, 0.0, 14.0));
    look_from(app, piece + Vec3::new(6.0, 2.5, 7.0), piece + Vec3::Y * 1.5);
    capture(app, out.join(format!("{prefix}03-pieces-lit-side.png")));
    if !all {
        return;
    }
    look_from(
        app,
        piece + Vec3::new(-5.0, 3.0, -6.0),
        piece + Vec3::Y * 1.5,
    );
    capture(app, out.join(format!("{prefix}04-pieces-shadow-side.png")));

    look_from(
        app,
        Vec3::new(20.0, 16.0, 22.0),
        Vec3::new(-10.0, 0.0, -10.0),
    );
    capture(app, out.join(format!("{prefix}05-overview.png")));

    // First person again with a build tool: the ghost.
    app.world_mut().remove_resource::<GalleryCamera>();
    {
        let world = app.world_mut();
        let mut q = world.query_filtered::<&mut ActiveTool, With<Player>>();
        *q.single_mut(world).unwrap() = ActiveTool::Build(PieceKind::Floor);
    }
    frames(app, 30);
    capture(app, out.join(format!("{prefix}06-ghost.png")));
}

#[test]
#[ignore = "needs a GPU: run by hand to review the look"]
fn render_the_look_offscreen() {
    let out = std::env::var("PIECED_LOOK_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("pieced-look"));
    std::fs::create_dir_all(&out).unwrap();

    // The default look: hull outlines.
    let start = std::time::Instant::now();
    let mut app = look_app(None, true);
    boot(&mut app);
    // Debug build, offscreen; with a warm Metal shader cache this is the
    // pipeline compile + warm-up share of launch-to-controllable.
    println!(
        "app build to Playing: {:.2} s",
        start.elapsed().as_secs_f32()
    );
    assert_eq!(
        app.world().resource::<LookSettings>().outline,
        OutlineBackend::Hull
    );
    review(&mut app, &out, "", true);
    assert_pipelines_ok(&app);
    drop(app);

    // The same views with bevy_mod_outline, for comparison.
    let mut app = look_app(Some("outline=mod"), false);
    boot(&mut app);
    assert_eq!(
        app.world().resource::<LookSettings>().outline,
        OutlineBackend::Mod
    );
    review(&mut app, &out, "mod-", false);
    assert_pipelines_ok(&app);
    println!("wrote PNGs to {}", out.display());
}
