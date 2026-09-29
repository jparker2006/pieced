//! Offscreen check of the per-pass GPU timing (`src/gpu_timing.rs`) on the
//! real GPU: the client look with no window, the UI camera drawing into an
//! image (so every camera writes its marks), a busy view of the arena and the
//! far layer. It times every frame, then only bare frames, and prints each
//! pass's mean and the timing's own cost (mean full total minus mean bare).
//!
//! It needs a GPU, so it is ignored by default. Run it by hand:
//!
//! ```sh
//! scripts/cargo.sh test --locked --test gpu_timing_offscreen -- --ignored --nocapture
//! ```

use avian3d::prelude::PhysicsPlugins;
use bevy::{
    camera::RenderTarget,
    prelude::*,
    render::{
        RenderPlugin,
        pipelined_rendering::PipelinedRenderingPlugin,
        render_resource::{TextureFormat, TextureUsages},
        settings::{Backends, RenderCreation, WgpuSettings},
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
    gpu_timing::{GpuPass, GpuResults, GpuSample, GpuTimingPlugin},
    hud::HudPlugin,
    look::LookPlugin,
    models::ModelsPlugin,
    render::{RenderSetupPlugin, WorldTarget},
    shared::{AppState, tick_duration},
    telemetry::MainFrame,
    tuning::Tuning,
    viewmodel::ViewmodelPlugin,
};

fn probe_app() -> App {
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
        HudPlugin,
        BootPlugin,
        GpuTimingPlugin,
    ))
    .init_resource::<MainFrame>()
    .add_systems(First, |mut f: ResMut<MainFrame>| f.0 += 1)
    .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()));
    app.finish();
    app.cleanup();
    app
}

/// Points the UI camera at an image (there is no window), readable by the
/// timing's last mark.
fn composite_ui(app: &mut App) {
    let size = app.world().resource::<WorldTarget>().size;
    let mut image = Image::new_target_texture(
        size.x,
        size.y,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    );
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let image = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    let world = app.world_mut();
    let ui = world
        .query_filtered::<Entity, With<IsDefaultUiCamera>>()
        .single(world)
        .expect("the UI camera");
    world
        .entity_mut(ui)
        .insert(RenderTarget::Image(image.into()));
}

fn collect(app: &mut App, frames: usize) -> Vec<GpuSample> {
    let mut out = Vec::new();
    for _ in 0..frames {
        app.update();
        if let Some(s) = app.world().resource::<GpuResults>().take() {
            out.push(s);
        }
    }
    out
}

fn mean(values: impl Iterator<Item = f32>) -> Option<f32> {
    let (n, sum) = values.fold((0u32, 0.0f32), |(n, s), v| (n + 1, s + v));
    (n > 0).then(|| sum / n as f32)
}

#[test]
#[ignore = "needs a GPU: run by hand to check the GPU timing"]
fn every_kind_of_mark_is_valid_on_this_gpu() {
    use bevy::render::renderer::{RenderDevice, RenderQueue};
    let app = probe_app();
    let render = app.sub_app(bevy::render::RenderApp).world();
    let device = render.resource::<RenderDevice>();
    let queue = render.resource::<RenderQueue>();
    let gaps = pieced::gpu_timing::self_test(device.wgpu_device(), queue).unwrap();
    println!("gaps between marks (ms): {gaps:?}");
    assert_eq!(gaps.len(), 8, "nine marks came back");
}

#[test]
#[ignore = "needs a GPU: run by hand to check the GPU timing"]
fn gpu_timing_reports_every_pass_on_this_gpu() {
    let mut app = probe_app();
    let mut boot = 0;
    while *app.world().resource::<State<AppState>>().get() == AppState::Boot {
        app.update();
        boot += 1;
        assert!(boot < 600, "Boot never ended");
    }
    composite_ui(&mut app);
    for _ in 0..30 {
        app.update();
    }

    let set = |app: &mut App, every: u32, bare_every: u32| {
        let mut t = app.world_mut().resource_mut::<Tuning>();
        t.perf.gpu_every = every;
        t.perf.gpu_bare_every = bare_every;
    };
    set(&mut app, 1, 0);
    let _ = collect(&mut app, 10);
    let full = collect(&mut app, 240);
    set(&mut app, 1, 1);
    let _ = collect(&mut app, 10);
    let bare = collect(&mut app, 240);

    assert!(
        full.len() > 100,
        "only {} full samples came back",
        full.len()
    );
    assert!(
        bare.len() > 100,
        "only {} bare samples came back",
        bare.len()
    );
    assert!(full.iter().all(|s| s.full) && bare.iter().all(|s| !s.full));
    for pass in GpuPass::ALL {
        let m = mean(full.iter().filter_map(|s| s.pass(pass)));
        let n = full.iter().filter(|s| s.pass(pass).is_some()).count();
        println!("{:9} mean {:?} ms over {n} samples", pass.name(), m);
    }
    let full_total = mean(full.iter().filter_map(|s| s.total_ms)).unwrap();
    let bare_total = mean(bare.iter().filter_map(|s| s.total_ms)).unwrap();
    println!(
        "total: full {full_total:.3} ms, bare {bare_total:.3} ms, timing cost {:.3} ms",
        full_total - bare_total
    );
    let world = mean(full.iter().filter_map(|s| s.pass(GpuPass::World))).unwrap();
    assert!(world > 0.0, "the world pass took no GPU time");
    for pass in [GpuPass::Effects, GpuPass::Ui, GpuPass::Post] {
        assert!(
            full.iter().any(|s| s.pass(pass).is_some()),
            "{} never measured",
            pass.name()
        );
    }
    assert!(full_total >= world, "the total covers the world pass");
}
