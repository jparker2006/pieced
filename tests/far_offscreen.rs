//! Offscreen review of the far view (slice D): runs the real client look,
//! models, arena visuals, viewmodel and far-view plugins with no window,
//! renders into the world target and writes PNGs to compare with the targets
//! (T01 spawn vista, T10 station up, T11 island edge), plus a `sky_check` pair
//! five seconds apart. It checks that every pipeline compiled, that Boot waited
//! for the galaxy and the far models, and that the generated cubemap's faces
//! land where the layout says (the galaxy's core is at the centre of a view
//! aimed at it).
//!
//! It needs a GPU, so it is ignored by default. Run it by hand:
//!
//! ```sh
//! PIECED_LOOK_OUT=/some/dir cargo test --locked --test far_offscreen -- --ignored --nocapture
//! ```

use avian3d::prelude::PhysicsPlugins;
use bevy::{
    prelude::*,
    render::{
        RenderApp, RenderPlugin,
        pipelined_rendering::PipelinedRenderingPlugin,
        render_resource::{CachedPipelineState, PipelineCache},
        settings::{Backends, RenderCreation, WgpuSettings},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::ExitCondition,
    winit::WinitPlugin,
};
use pieced::{
    app::{BootPlugin, SimPlugins},
    arena::visuals::ArenaVisualsPlugin,
    building::BuildingVisualsPlugin,
    far::{FarBoot, FarLayout, FarViewPlugin, GalaxySky, GalaxyTexture, SPAWN_EYE, SkyClock},
    look::LookPlugin,
    models::ModelsPlugin,
    render::{MainCamera, RenderSetupPlugin, WorldTarget},
    scenario::gallery::GalleryCamera,
    shared::AppState,
    viewmodel::ViewmodelPlugin,
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

fn far_app() -> App {
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
        BootPlugin,
    ));
    app.finish();
    app.cleanup();
    app
}

fn frames(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

/// Renders the world target and returns it (and saves it when `path` is given).
fn capture(app: &mut App, path: Option<PathBuf>) -> Image {
    let image = app.world().resource::<WorldTarget>().image.clone();
    let got: Arc<Mutex<Option<Image>>> = Arc::default();
    let sink = got.clone();
    app.world_mut()
        .spawn(Screenshot::image(image))
        .observe(move |shot: On<ScreenshotCaptured>| {
            *sink.lock().unwrap() = Some(shot.image.clone());
        });
    for _ in 0..30 {
        app.update();
        if let Some(image) = got.lock().unwrap().take() {
            if let Some(path) = path {
                let _ = std::fs::remove_file(&path);
                image
                    .clone()
                    .try_into_dynamic()
                    .unwrap()
                    .to_rgb8()
                    .save(&path)
                    .unwrap();
                println!("wrote {}", path.display());
            }
            return image;
        }
    }
    panic!("no screenshot");
}

/// Freezes the far view at `seconds` and aims the main camera.
fn view(app: &mut App, seconds: f64, camera: Transform) {
    *app.world_mut().resource_mut::<SkyClock>() = SkyClock {
        seconds,
        paused: true,
    };
    app.insert_resource(GalleryCamera(camera));
    frames(app, 4);
}

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

/// Mean sRGB luma (0..255) of a square patch centred at (fx, fy) of the image.
fn patch_luma(image: &Image, fx: f32, fy: f32) -> f32 {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let data = image.data.as_ref().unwrap();
    let (cx, cy) = ((w as f32 * fx) as usize, (h as f32 * fy) as usize);
    let r = 6;
    let mut sum = 0.0;
    let mut n = 0.0;
    for y in cy.saturating_sub(r)..(cy + r).min(h) {
        for x in cx.saturating_sub(r)..(cx + r).min(w) {
            let i = (y * w + x) * 4;
            sum += (data[i] as f32 + data[i + 1] as f32 + data[i + 2] as f32) / 3.0;
            n += 1.0;
        }
    }
    sum / n
}

fn out_dir() -> PathBuf {
    std::env::var("PIECED_LOOK_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("pieced-look"))
}

fn eye_looking(eye: Vec3, azimuth_deg: f32, pitch_deg: f32) -> Transform {
    let (az, el) = (azimuth_deg.to_radians(), pitch_deg.to_radians());
    let dir = Vec3::new(az.sin() * el.cos(), el.sin(), -az.cos() * el.cos());
    Transform::from_translation(eye).looking_to(dir, Vec3::Y)
}

#[test]
#[ignore = "needs a GPU: run by hand to review the far view"]
fn render_the_far_view_offscreen() {
    let out = out_dir();
    std::fs::create_dir_all(&out).unwrap();
    let start = std::time::Instant::now();
    let mut app = far_app();
    let mut boot_frames = 0;
    while *app.world().resource::<State<AppState>>().get() == AppState::Boot {
        app.update();
        boot_frames += 1;
        assert!(boot_frames < 900, "Boot never ended");
    }
    let galaxy_ms = app.world().resource::<GalaxyTexture>().millis;
    println!(
        "Boot ended after {boot_frames} frames ({:.2} s); galaxy generated in {galaxy_ms:.0} ms",
        start.elapsed().as_secs_f32()
    );
    {
        let boot = app.world().resource::<FarBoot>();
        assert!(boot.skybox_frames >= pieced::far::SKYBOX_WARM_FRAMES);
        let spawned = boot.models_spawned.expect("far models placed");
        assert!(spawned >= 20, "{spawned} far models");
        assert_eq!(
            boot.models_dressed, spawned,
            "every far model dressed during Boot"
        );
    }
    let world = app.world_mut();
    let mut q = world.query_filtered::<(), (With<MainCamera>, With<GalaxySky>)>();
    assert_eq!(q.iter(world).count(), 1, "the main camera shows the galaxy");
    frames(&mut app, 10);

    let layout = app.world().resource::<FarLayout>().clone();

    // The cubemap's faces land where the layout says: aimed at the galaxy's
    // core (sky time 0), the centre of the view is its brightest part.
    let at_core = Transform::from_translation(SPAWN_EYE).looking_to(layout.galaxy, Vec3::Y);
    view(&mut app, 0.0, at_core);
    let shot = capture(&mut app, Some(out.join("far-04-galaxy.png")));
    let (core, edge) = (patch_luma(&shot, 0.5, 0.5), patch_luma(&shot, 0.1, 0.9));
    println!("galaxy core luma {core:.0}, view corner {edge:.0}");
    assert!(
        core > 200.0 && core > edge + 90.0,
        "core {core} vs corner {edge}"
    );

    // T01: the spawn vista (first person, level, facing -Z).
    view(&mut app, 0.0, eye_looking(SPAWN_EYE, 0.0, 0.0));
    capture(&mut app, Some(out.join("far-01-spawn-T01.png")));
    // sky_check: the same view five seconds later.
    view(&mut app, 5.0, eye_looking(SPAWN_EYE, 0.0, 0.0));
    capture(&mut app, Some(out.join("far-01b-spawn-5s-later.png")));

    // T10: looking up at the station from the arena's north-east.
    let eye = Vec3::new(16.0, 1.6, -16.0);
    let station_top = layout.station.position + Vec3::Y * 90.0;
    view(
        &mut app,
        3.0,
        Transform::from_translation(eye).looking_at(station_top, Vec3::Y),
    );
    capture(&mut app, Some(out.join("far-02-station-up-T10.png")));

    // T11: at the island's east edge, looking out over the void.
    view(
        &mut app,
        7.0,
        eye_looking(Vec3::new(23.0, 1.6, 4.0), 22.0, 6.0),
    );
    capture(&mut app, Some(out.join("far-03-island-edge-T11.png")));

    // Behind the spawn, and a high overview of the whole far layout.
    view(&mut app, 9.0, eye_looking(SPAWN_EYE, 180.0, 4.0));
    capture(&mut app, Some(out.join("far-05-behind.png")));
    view(
        &mut app,
        11.0,
        Transform::from_xyz(-60.0, 90.0, 160.0).looking_at(layout.station.position, Vec3::Y),
    );
    capture(&mut app, Some(out.join("far-06-overview.png")));

    assert_pipelines_ok(&app);
    let _ = Path::new(&out);
}
