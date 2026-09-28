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

/// Hides the Milestone 1 backdrop (far terrain, mesas, clouds, boundary cliffs,
/// near trees), which the island slice replaces with cliffs dropping into
/// space, so the far view can be judged as it will be seen.
fn hide_m1_backdrop(app: &mut App) {
    const BACKDROP: [&str; 5] = [
        "Far terrain",
        "Backdrop",
        "Clouds",
        "Boundary cliffs",
        "Near trees",
    ];
    let world = app.world_mut();
    let mut q = world.query::<(&Name, &mut Visibility)>();
    for (name, mut v) in q.iter_mut(world) {
        if BACKDROP.contains(&name.as_str()) {
            *v = Visibility::Hidden;
        }
    }
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

    // T01: the spawn vista (first person, level, facing -Z), first with the
    // Milestone 1 backdrop, then (like everything after) without it.
    view(&mut app, 0.0, eye_looking(SPAWN_EYE, 0.0, 0.0));
    capture(
        &mut app,
        Some(out.join("far-00-spawn-with-m1-backdrop.png")),
    );
    hide_m1_backdrop(&mut app);
    view(&mut app, 0.0, eye_looking(SPAWN_EYE, 0.0, 0.0));
    capture(&mut app, Some(out.join("far-01-spawn-T01.png")));
    // sky_check: the same view five seconds later.
    view(&mut app, 5.0, eye_looking(SPAWN_EYE, 0.0, 0.0));
    capture(&mut app, Some(out.join("far-01b-spawn-5s-later.png")));

    // T10: looking up at the station from the arena's north-east corner, and
    // from a scenario camera 250 m out toward it (where the station fills the
    // frame).
    let station_mid = layout.station.position + Vec3::Y * 70.0;
    let eye = Vec3::new(20.0, 1.6, -20.0);
    view(
        &mut app,
        3.0,
        Transform::from_translation(eye).looking_at(station_mid, Vec3::Y),
    );
    capture(&mut app, Some(out.join("far-02-station-up-T10.png")));
    let toward = (layout.station.position - eye) * Vec3::new(1.0, 0.0, 1.0);
    let out_eye = eye + toward.normalize() * 250.0 + Vec3::Y * 20.0;
    view(
        &mut app,
        3.0,
        Transform::from_translation(out_eye).looking_at(station_mid, Vec3::Y),
    );
    capture(&mut app, Some(out.join("far-02b-station-up-close-T10.png")));
    // A worm's-eye scenario camera well out toward it, below its rock.
    let below_eye = eye + toward.normalize() * 400.0 + Vec3::Y * 40.0;
    view(
        &mut app,
        3.0,
        Transform::from_translation(below_eye).looking_at(station_mid + Vec3::Y * 30.0, Vec3::Y),
    );
    capture(&mut app, Some(out.join("far-02c-station-up-below-T10.png")));

    // T11: at the island's east edge, looking out over the void.
    view(
        &mut app,
        7.0,
        eye_looking(Vec3::new(23.0, 1.6, 4.0), 25.0, 5.0),
    );
    capture(&mut app, Some(out.join("far-03-island-edge-T11.png")));

    // The other directions from spawn, and a high overview.
    for (az, name) in [(90.0, "east"), (180.0, "south"), (270.0, "west")] {
        view(&mut app, 9.0, eye_looking(SPAWN_EYE, az, 3.0));
        capture(&mut app, Some(out.join(format!("far-05-{name}.png"))));
    }
    view(
        &mut app,
        11.0,
        Transform::from_xyz(-60.0, 90.0, 160.0).looking_at(layout.station.position, Vec3::Y),
    );
    capture(&mut app, Some(out.join("far-06-overview.png")));

    assert_pipelines_ok(&app);
    let _ = Path::new(&out);
}

/// The castle's look check (M3 W5, docs/M3-SPEC.md → The castle and the sky):
/// the spawn vista and the look up at the castle, next to C4. It writes, with
/// a greyscale copy of each, into `PIECED_CASTLE_OUT` (default
/// `<tmp>/pieced-castle`):
///
/// - `castle-spawn.png`: from the player spawn, level, facing -Z (the castle
///   upper right, as C4 frames it);
/// - `castle-spawn-t01.png`: the gallery's T01 camera (the west edge, 5° left,
///   5° down);
/// - `castle-up.png`: looking up at the castle from the void 420 m in front of
///   it, below its rock;
/// - `castle-up-t10.png`: the gallery's T10 camera (250 m out, 24° up).
///
/// The sky clock is set so a shooting star is crossing the spawn view.
///
/// ```sh
/// PIECED_CASTLE_OUT=/some/dir cargo test --locked --test far_offscreen castle -- --ignored --nocapture
/// ```
#[test]
#[ignore = "needs a GPU: run by hand to review the castle"]
fn render_the_castle_offscreen() {
    use pieced::far::ShootingStars;
    use pieced::shared::LookAngles;
    let out = std::env::var("PIECED_CASTLE_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("pieced-castle"));
    std::fs::create_dir_all(&out).unwrap();
    let mut app = far_app();
    let mut boot_frames = 0;
    while *app.world().resource::<State<AppState>>().get() == AppState::Boot {
        app.update();
        boot_frames += 1;
        assert!(boot_frames < 900, "Boot never ended");
    }
    frames(&mut app, 10);
    let layout = app.world().resource::<FarLayout>().clone();

    // A moment when a shooting star crosses the spawn view.
    let spawn_view = |d: Vec3| {
        let az = d.x.atan2(-d.z).to_degrees();
        let el = d.y.asin().to_degrees();
        az.abs() < 38.0 && (8.0..32.0).contains(&el)
    };
    let pass = ShootingStars::default()
        .passes(40)
        .into_iter()
        .find(|p| spawn_view(p.direction(0.2)) && spawn_view(p.direction(0.6)))
        .expect("a shooting star crosses the spawn view");
    let seconds = pass.start_s + pass.duration_s as f64 * 0.45;
    println!("sky time {seconds:.2} s (shooting star {})", pass.index);

    let save = |app: &mut App, name: &str, camera: Transform| {
        view(app, seconds, camera);
        let shot = capture(app, Some(out.join(format!("{name}.png"))));
        let grey = shot.try_into_dynamic().unwrap().to_luma8();
        grey.save(out.join(format!("{name}-grey.png"))).unwrap();
    };
    let look = |eye: Vec3, yaw: f32, pitch: f32| {
        Transform::from_translation(eye).with_rotation(
            LookAngles {
                yaw: yaw.to_radians(),
                pitch: pitch.to_radians(),
            }
            .rotation(),
        )
    };
    save(&mut app, "castle-spawn", look(SPAWN_EYE, 0.0, 0.0));
    save(
        &mut app,
        "castle-spawn-t01",
        look(Vec3::new(-19.5, 1.62, 14.0), 5.0, -5.0),
    );
    let station = layout.station.position;
    let away = |deg: f32| {
        let a = deg.to_radians();
        Vec3::new(a.sin(), 0.0, -a.cos())
    };
    let up_eye = (station + away(220.8) * 420.0).with_y(60.0);
    save(
        &mut app,
        "castle-up",
        Transform::from_translation(up_eye).looking_at(station + Vec3::Y * 150.0, Vec3::Y),
    );
    let t10_eye = (station + away(220.8) * 250.0).with_y(70.0);
    save(
        &mut app,
        "castle-up-t10",
        Transform::from_translation(t10_eye).looking_at(
            station.with_y(70.0 + 250.0 * 24f32.to_radians().tan()),
            Vec3::Y,
        ),
    );
    assert_pipelines_ok(&app);
}
