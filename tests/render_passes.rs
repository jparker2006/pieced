//! The frame's full-screen passes are pinned (M4 performance follow-up).
//!
//! Play-test 2 put the UI pass at 2.17 ms and post at 2.35 ms against 0.5 and
//! 1.0: the UI camera composited the 3D image as a full-screen image node at
//! Retina size, cleared a depth texture it never used, and copied the result
//! into the window, and both 3D cameras copied into the world image. Now a
//! frame runs FXAA and one copy at the render size and **one** pass at the
//! window's size (the HUD over the 3D image). These tests build the real
//! cameras headless and read [`pieced::render::full_screen_passes`], so a new
//! full-screen pass (or a second one at window size) fails here first.

use bevy::{
    anti_alias::fxaa::Fxaa, camera::CameraOutputMode, input::InputPlugin, prelude::*,
    time::TimeUpdateStrategy, world_serialization::WorldSerializationPlugin,
};
use pieced::{
    look::LookSettings,
    render::{
        FullScreenPass, MainCamera, PassSize, RenderSetupPlugin, UiComposite,
        full_screen_passes,
    },
    shared::tick_duration,
    tuning::Tuning,
    viewmodel::ViewmodelCamera,
};

fn camera_app(configure: impl FnOnce(&mut Tuning)) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        bevy::state::app::StatesPlugin,
        AssetPlugin::default(),
        bevy::mesh::MeshPlugin,
        bevy::gltf::GltfPlugin::default(),
        WorldSerializationPlugin,
        avian3d::prelude::PhysicsPlugins::default(),
        InputPlugin,
    ))
    .init_asset::<Image>()
    .init_asset::<StandardMaterial>()
    .add_plugins(pieced::app::SimPlugins)
    .add_plugins((
        RenderSetupPlugin,
        pieced::look::LookPlugin,
        pieced::models::ModelsPlugin,
        pieced::viewmodel::ViewmodelPlugin,
        pieced::perf::PerfPlugin,
    ))
    .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()));
    configure(&mut app.world_mut().resource_mut::<Tuning>());
    app.finish();
    app.cleanup();
    for _ in 0..600 {
        app.update();
        let world = app.world_mut();
        let has_viewmodel = world
            .query_filtered::<(), With<ViewmodelCamera>>()
            .iter(world)
            .next()
            .is_some();
        if has_viewmodel {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    for _ in 0..3 {
        app.update();
    }
    app
}

fn names(passes: &[FullScreenPass]) -> Vec<(&'static str, PassSize)> {
    passes.iter().map(|p| (p.name, p.size)).collect()
}

#[test]
fn a_battery_frame_runs_fxaa_one_copy_and_one_window_size_pass() {
    let mut app = camera_app(|_| {});
    assert!(app.world().resource::<LookSettings>().fxaa);
    let passes = full_screen_passes(app.world_mut());
    assert_eq!(
        names(&passes),
        [
            ("fxaa", PassSize::Render),
            ("copy_to_output", PassSize::Render),
            ("ui_composite", PassSize::Window),
        ],
        "the full-screen passes changed: {passes:?}"
    );
    let window = passes.iter().filter(|p| p.size == PassSize::Window).count();
    assert_eq!(window, 1, "one pass at the window's (Retina) size");
    // The world camera skips its copy (the viewmodel camera writes the
    // shared image), and the UI camera runs the composite schedule.
    let world = app.world_mut();
    let main = world
        .query_filtered::<&Camera, With<MainCamera>>()
        .single(world)
        .unwrap();
    assert!(matches!(main.output_mode, CameraOutputMode::Skip));
    let ui = world
        .query_filtered::<(&Camera, &bevy::render::camera::CameraRenderGraph), With<IsDefaultUiCamera>>()
        .single(world)
        .unwrap();
    assert!(matches!(ui.0.output_mode, CameraOutputMode::Skip));
    assert_eq!(
        ui.1.0,
        bevy::ecs::schedule::ScheduleLabel::intern(&UiComposite)
    );
    // FXAA runs once, on the render-size image of the last 3D camera.
    let fxaa: Vec<bool> = world
        .query::<&Fxaa>()
        .iter(world)
        .map(|f| f.enabled)
        .collect();
    assert_eq!(fxaa, [true]);
}

#[test]
fn plugged_in_keeps_msaa_without_fxaa_and_the_same_single_copy() {
    let mut app = camera_app(|t| t.graphics.preset = pieced::render::QualityPreset::PluggedIn);
    let passes = full_screen_passes(app.world_mut());
    assert_eq!(
        names(&passes),
        [
            ("copy_to_output", PassSize::Render),
            ("ui_composite", PassSize::Window),
        ]
    );
}

#[test]
fn with_the_viewmodel_camera_off_the_world_camera_writes_its_own_image() {
    let mut app = camera_app(|_| {});
    {
        let world = app.world_mut();
        let mut viewmodel = world
            .query_filtered::<&mut Camera, With<ViewmodelCamera>>()
            .single_mut(world)
            .unwrap();
        viewmodel.is_active = false;
    }
    app.update();
    let passes = full_screen_passes(app.world_mut());
    assert_eq!(
        names(&passes),
        [
            ("copy_to_output", PassSize::Render),
            ("ui_composite", PassSize::Window),
        ],
        "the death beat turns the gun camera off: the world still reaches the screen"
    );
    // And back on: the world camera skips its copy again.
    {
        let world = app.world_mut();
        let mut viewmodel = world
            .query_filtered::<&mut Camera, With<ViewmodelCamera>>()
            .single_mut(world)
            .unwrap();
        viewmodel.is_active = true;
    }
    app.update();
    assert_eq!(full_screen_passes(app.world_mut()).len(), 3);
}

#[test]
fn a_split_msaa_setup_copies_from_each_camera() {
    // `--knobs msaa=4,vmmsaa=1`: the cameras no longer share a main texture,
    // so both must write the world image (the viewmodel blends over it).
    let mut app = camera_app(|_| {});
    app.insert_resource(pieced::perf_knobs::PerfKnobs::parse("msaa=4,vmmsaa=1"));
    for _ in 0..3 {
        app.update();
    }
    let passes = full_screen_passes(app.world_mut());
    let copies = passes.iter().filter(|p| p.name == "copy_to_output").count();
    assert_eq!(copies, 2, "{passes:?}");
    let world = app.world_mut();
    let viewmodel = world
        .query_filtered::<&Camera, With<ViewmodelCamera>>()
        .single(world)
        .unwrap();
    assert!(
        matches!(
            viewmodel.output_mode,
            CameraOutputMode::Write {
                blend_state: None,
                ..
            }
        ),
        "Bevy's default: blend over the world camera's copy"
    );
}
