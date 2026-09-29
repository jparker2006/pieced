//! Offscreen cost of a full wave, for bisecting GPU regressions: the whole
//! client (`ClientPlugins` without the native window), the UI camera drawing
//! into a Retina-sized image like the 15" Air's window, Waves at wave 6 with
//! a player who can't die, looking at the fight. Over 300 frames it prints
//! the CPU time per frame, the GPU wait after each frame's submit (≈ its GPU
//! time), the visible meshes per layer, the visible UI nodes, the entities,
//! and (where the build has it) the GPU time per pass.
//!
//! `PIECED_PROBE_LEVERS=farres,dynres,overdraw` turns the M4 levers on
//! (any subset), and `PIECED_PROBE_SHOT=<file.png>` saves the 3D image at the
//! end, for comparing a lever's look; `PIECED_PROBE_UI_SHOT=<file.png>` saves
//! the finished Retina frame (the HUD composited over the 3D image).
//!
//! It needs a GPU, so it is ignored by default:
//!
//! ```sh
//! scripts/cargo.sh test --locked --test wave_cost_offscreen -- --ignored --nocapture
//! ```

use avian3d::prelude::PhysicsPlugins;
use bevy::{
    camera::{RenderTarget, visibility::RenderLayers},
    log::LogPlugin,
    prelude::*,
    render::{
        RenderApp, RenderPlugin,
        pipelined_rendering::PipelinedRenderingPlugin,
        render_resource::TextureFormat,
        renderer::RenderDevice,
        settings::{Backends, RenderCreation, WgpuSettings},
    },
    time::TimeUpdateStrategy,
    window::ExitCondition,
    winit::WinitPlugin,
};
use pieced::{
    app::{ClientPlugins, SimPlugins},
    audio::GameAudioPlugin,
    native::NativeWindowPlugin,
    shared::{AppState, GameMode, Health, Player, tick_duration},
    waves::Run,
};
use std::time::Instant;

fn wave_app() -> App {
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
    .add_plugins(
        ClientPlugins
            .build()
            .disable::<NativeWindowPlugin>()
            .disable::<GameAudioPlugin>(),
    )
    .insert_resource(GameMode::Waves)
    .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()));
    app.finish();
    app.cleanup();
    let levers = std::env::var("PIECED_PROBE_LEVERS").unwrap_or_default();
    {
        let mut tuning = app.world_mut().resource_mut::<pieced::tuning::Tuning>();
        tuning.perf.far_half_res = levers.contains("farres");
        tuning.perf.dynres = levers.contains("dynres");
        tuning.perf.overdraw_cap = levers.contains("overdraw");
    }
    app
}

/// The UI camera draws into an image the size of the Air's Retina window.
fn retina_ui(app: &mut App) -> Handle<Image> {
    let image = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            3420,
            2214,
            TextureFormat::Bgra8Unorm,
            Some(TextureFormat::Bgra8UnormSrgb),
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

fn wait_gpu(app: &App) -> f64 {
    let device = app.sub_app(RenderApp).world().resource::<RenderDevice>();
    let t = Instant::now();
    let _ = device
        .wgpu_device()
        .poll(bevy::render::render_resource::PollType::wait_indefinitely());
    t.elapsed().as_secs_f64() * 1000.0
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len().max(1) as f64
}

fn p95(v: &[f64]) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    s.get(((s.len() as f64 * 0.95) as usize).min(s.len().saturating_sub(1)))
        .copied()
        .unwrap_or(0.0)
}

#[test]
#[ignore = "needs a GPU: run by hand to measure a full wave"]
fn a_full_wave_costs() {
    let t = Instant::now();
    let bank = pieced::audio::render_bank();
    println!(
        "WAVE_SOUNDS {} cues synthesized in {:.0} ms",
        bank.len(),
        t.elapsed().as_secs_f64() * 1000.0
    );
    let mut app = wave_app();
    let mut boot = 0;
    let started = Instant::now();
    while *app.world().resource::<State<AppState>>().get() == AppState::Boot {
        let t = Instant::now();
        app.update();
        let held: Vec<&str> = app
            .world()
            .resource::<pieced::app::BootGate>()
            .held()
            .collect();
        if std::env::var_os("PIECED_PROBE_BOOT").is_some() {
            println!(
                "BOOT_FRAME {boot} at {:.0} ms took {:.1} ms, held {held:?}",
                started.elapsed().as_secs_f64() * 1000.0,
                t.elapsed().as_secs_f64() * 1000.0
            );
        }
        boot += 1;
        assert!(boot < 900, "Boot never ended");
    }
    println!(
        "WAVE_BOOT {} frames, {:.0} ms; {}",
        boot,
        started.elapsed().as_secs_f64() * 1000.0,
        app.world()
            .resource::<pieced::telemetry::BootPhases>()
            .line()
    );
    let ui_image = retina_ui(&mut app);
    {
        let world = app.world_mut();
        let player = world
            .query_filtered::<Entity, With<Player>>()
            .single(world)
            .unwrap();
        let mut health = Health::full(1.0e9, 100.0);
        health.shield = 100.0;
        *world.get_mut::<Health>(player).unwrap() = health;
        let mut run = world.resource_mut::<Run>();
        run.wave = 6;
        run.remaining = 14;
    }
    // Until the wave is fully landed (8 alive), at most ~40 s.
    let mut settle = 0;
    while app.world().resource::<Run>().alive < 8 && settle < 2400 {
        app.update();
        let _ = wait_gpu(&app);
        settle += 1;
    }
    for _ in 0..60 {
        app.update();
    }
    let _ = wait_gpu(&app);
    let (mut cpu, mut gpu) = (Vec::new(), Vec::new());
    for _ in 0..300 {
        let t = Instant::now();
        app.update();
        cpu.push(t.elapsed().as_secs_f64() * 1000.0);
        gpu.push(wait_gpu(&app));
    }
    // The GPU time per pass (src/gpu_timing.rs), every frame for 240 frames.
    app.world_mut()
        .resource_mut::<pieced::tuning::Tuning>()
        .perf
        .gpu_every = 1;
    app.world_mut()
        .resource_mut::<pieced::tuning::Tuning>()
        .perf
        .gpu_bare_every = 0;
    let mut samples = Vec::new();
    for _ in 0..240 {
        app.update();
        if let Some(s) = app
            .world()
            .resource::<pieced::gpu_timing::GpuResults>()
            .take()
        {
            if samples.len() < 3 {
                println!("WAVE_MARKS {:?}", pieced::gpu_timing::debug_last_marks());
            }
            samples.push(s);
        }
    }
    let pass = |p: pieced::gpu_timing::GpuPass| {
        let v: Vec<f64> = samples
            .iter()
            .filter_map(|s| s.pass(p))
            .map(f64::from)
            .collect();
        format!("{}={:.2}/{:.2}", p.name(), mean(&v), p95(&v))
    };
    let totals: Vec<f64> = samples
        .iter()
        .filter_map(|s| s.total_ms)
        .map(f64::from)
        .collect();
    println!(
        "WAVE_GPU mean/p95 ms: {} {} {} {} {} total={:.2}/{:.2} ({} samples)",
        pass(pieced::gpu_timing::GpuPass::World),
        pass(pieced::gpu_timing::GpuPass::Far),
        pass(pieced::gpu_timing::GpuPass::Effects),
        pass(pieced::gpu_timing::GpuPass::Ui),
        pass(pieced::gpu_timing::GpuPass::Post),
        mean(&totals),
        p95(&totals),
        samples.len()
    );
    let world = app.world_mut();
    let mut layers = [0usize; 4];
    for (vis, layer) in world
        .query_filtered::<(&ViewVisibility, Option<&RenderLayers>), With<Mesh3d>>()
        .iter(world)
    {
        if vis.get() {
            let i = layer.and_then(|l| l.iter().next()).unwrap_or(0).min(3);
            layers[i] += 1;
        }
    }
    let ui = world
        .query::<(&Node, &InheritedVisibility)>()
        .iter(world)
        .filter(|(_, v)| v.get())
        .count();
    let entities = world.query::<Entity>().iter(world).count();
    let far_cameras = world
        .query_filtered::<(), With<pieced::perf::far_res::FarCamera>>()
        .iter(world)
        .count();
    let scale = world.resource::<pieced::perf::DynamicResolution>().scale;
    println!("WAVE_LEVERS far_cameras={far_cameras} dynres_scale={scale}");
    let alive = world.resource::<Run>().alive;
    println!(
        "WAVE_COST alive={alive} settle={settle} cpu_mean={:.2} gpu_wait_mean={:.2} gpu_wait_p95={:.2} \
         visible_meshes world={} viewmodel={} other={} ui_nodes={ui} entities={entities}",
        mean(&cpu),
        mean(&gpu),
        p95(&gpu),
        layers[0],
        layers[1],
        layers[2] + layers[3],
    );
    let world_image = app
        .world()
        .resource::<pieced::render::WorldTarget>()
        .image
        .clone();
    for (var, image) in [
        ("PIECED_PROBE_SHOT", world_image),
        ("PIECED_PROBE_UI_SHOT", ui_image),
    ] {
        let Ok(path) = std::env::var(var) else {
            continue;
        };
        use bevy::render::view::screenshot::{Screenshot, save_to_disk};
        let path = std::path::PathBuf::from(path);
        let _ = std::fs::remove_file(&path);
        app.world_mut()
            .spawn(Screenshot::image(image))
            .observe(save_to_disk(path.clone()));
        for _ in 0..60 {
            app.update();
            if path.exists() {
                std::thread::sleep(std::time::Duration::from_millis(300));
                break;
            }
        }
        assert!(path.exists(), "no screenshot at {}", path.display());
    }
}
