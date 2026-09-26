//! Offscreen render of the target-board gallery (docs/M2-SPEC.md → The target
//! board and gallery): the same twelve view definitions the native `gallery`
//! scenario plays ([`views`]), through the same [`GalleryRunner`], rendered with
//! the real client look and no window. The UI camera draws into an image too, so
//! the shots carry the HUD and the pause menu over the world like a screenshot.
//!
//! It writes `T01-spawn-vista.png` … `T12-pause-menu.png`, a greyscale copy of
//! each, and `gallery.json` (the views and what happened in each) into
//! `PIECED_GALLERY_OUT` (default: `<tmp>/pieced-gallery`). Art still being built
//! in other slices shows as whatever is merged; the composition is the point.
//! Make the board page from the folder with `python3 scripts/board.py <dir>`.
//!
//! It needs a GPU, so it is ignored by default. Run it by hand:
//!
//! ```sh
//! PIECED_GALLERY_OUT=/some/dir cargo test --locked --test gallery_offscreen -- --ignored --nocapture
//! ```

use avian3d::prelude::PhysicsPlugins;
use bevy::{
    camera::RenderTarget,
    log::LogPlugin,
    prelude::*,
    render::{
        RenderApp, RenderPlugin,
        pipelined_rendering::PipelinedRenderingPlugin,
        render_resource::{CachedPipelineState, PipelineCache, TextureFormat},
        settings::{Backends, RenderCreation, WgpuSettings},
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    time::TimeUpdateStrategy,
    window::ExitCondition,
    winit::WinitPlugin,
};
use pieced::{
    app::{BootPlugin, SimPlugins},
    arena::visuals::ArenaVisualsPlugin,
    building::BuildingVisualsPlugin,
    far::FarViewPlugin,
    fx::FxPlugin,
    hud::HudPlugin,
    look::LookPlugin,
    menu::MenuPlugin,
    models::ModelsPlugin,
    player::IntentWriters,
    render::{RenderSetupPlugin, WorldTarget},
    scenario::gallery::{GalleryPlugin, GalleryRunner, views},
    shared::{AppState, tick_duration},
    viewmodel::ViewmodelPlugin,
};
use std::path::{Path, PathBuf};

/// Frames between Boot ending and the first view.
const LEAD_IN: u32 = 30;

/// The gallery being played, and where its captures go.
#[derive(Resource)]
struct Driver {
    runner: GalleryRunner,
    out: PathBuf,
    /// The image every capture reads: the UI composite (world, HUD and menus).
    image: Handle<Image>,
    written: Vec<PathBuf>,
}

fn gallery_app() -> App {
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
        // After ArenaVisualsPlugin: it owns the galaxy's FarHaze.
        FarViewPlugin,
        BuildingVisualsPlugin,
        ViewmodelPlugin,
        FxPlugin,
        HudPlugin,
        MenuPlugin,
        BootPlugin,
        GalleryPlugin,
    ))
    // One fixed tick per frame: the same timeline as a 60 Hz native run.
    .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()))
    .add_systems(PreUpdate, drive.in_set(IntentWriters));
    app.finish();
    app.cleanup();
    app
}

/// The runner's frame, then this frame's captures from the composite image.
fn drive(world: &mut World) {
    if !world.contains_resource::<Driver>() {
        return;
    }
    world.resource_scope(|world, mut driver: Mut<Driver>| {
        driver.runner.frame(world);
        for name in driver.runner.take_captures() {
            let path = driver.out.join(format!("{name}.png"));
            let grey = driver.out.join(format!("{name}-grey.png"));
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(&grey);
            driver.written.extend([path.clone(), grey.clone()]);
            world
                .spawn(Screenshot::image(driver.image.clone()))
                .observe(move |capture: On<ScreenshotCaptured>| {
                    let image = capture.image.clone();
                    let (path, grey) = (path.clone(), grey.clone());
                    std::thread::spawn(move || {
                        let image = image.try_into_dynamic().expect("screenshot converts");
                        image.to_rgb8().save(&path).expect("PNG written");
                        image.to_luma8().save(&grey).expect("grey PNG written");
                    });
                });
        }
    });
}

/// Points the UI camera at an image the size of the world target, so a capture
/// holds the world with the HUD and menus drawn over it.
fn composite_ui(app: &mut App) -> Handle<Image> {
    let size = app.world().resource::<WorldTarget>().size;
    let image = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            size.x,
            size.y,
            TextureFormat::Rgba8Unorm,
            Some(TextureFormat::Rgba8UnormSrgb),
        ));
    let world = app.world_mut();
    let ui = world
        .query_filtered::<Entity, With<IsDefaultUiCamera>>()
        .single(world)
        .expect("the UI camera");
    world
        .entity_mut(ui)
        .insert(RenderTarget::Image(image.clone().into()));
    image
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

fn wait_for_files(app: &mut App, files: &[PathBuf]) {
    for _ in 0..600 {
        if files.iter().all(|f| f.exists()) {
            // Let the writers finish.
            std::thread::sleep(std::time::Duration::from_millis(300));
            return;
        }
        app.update();
    }
    let missing: Vec<&Path> = files
        .iter()
        .filter(|f| !f.exists())
        .map(PathBuf::as_path)
        .collect();
    panic!("captures never written: {missing:?}");
}

#[test]
#[ignore = "needs a GPU: run by hand to review the gallery"]
fn render_the_gallery_offscreen() {
    let out = std::env::var("PIECED_GALLERY_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("pieced-gallery"));
    std::fs::create_dir_all(&out).unwrap();
    let mut app = gallery_app();
    let mut boot = 0;
    while *app.world().resource::<State<AppState>>().get() == AppState::Boot {
        app.update();
        boot += 1;
        assert!(boot < 600, "Boot never ended");
    }
    println!("Boot ended after {boot} frames");
    let image = composite_ui(&mut app);
    app.insert_resource(Driver {
        runner: GalleryRunner::new(views(), LEAD_IN),
        out: out.clone(),
        image,
        written: Vec::new(),
    });
    let mut frames = 0;
    while !app.world().resource::<Driver>().runner.is_done() {
        app.update();
        frames += 1;
        assert!(frames < 20_000, "the gallery never finished");
    }
    let written = app.world().resource::<Driver>().written.clone();
    wait_for_files(&mut app, &written);
    assert_pipelines_ok(&app);

    let driver = app.world().resource::<Driver>();
    let summary = driver.runner.summary();
    std::fs::write(
        out.join("gallery.json"),
        serde_json::to_string_pretty(&summary).unwrap(),
    )
    .unwrap();
    for record in driver.runner.records() {
        println!(
            "{}: shots {} hits {} head {} breaks {} kills {} knight {:?} missed {} rejected {:?}",
            record.id,
            record.shots,
            record.hits,
            record.headshots,
            record.shield_breaks,
            record.kills,
            record.knight_screen,
            record.missed,
            record.rejected
        );
    }
    println!("wrote {} PNGs to {}", written.len(), out.display());
}
