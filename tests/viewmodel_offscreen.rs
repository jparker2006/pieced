//! Offscreen review of the first-person guns: runs the real client look,
//! model and viewmodel plugins with no window, renders into the world target
//! image, checks every pipeline compiled and Boot's warm-up finished, and
//! writes PNGs of the rifle and pump at the hip, aiming down sights, firing and
//! reloading, for comparison with targets T01, T02 and T04.
//!
//! It needs a GPU, so it is ignored by default. Run it by hand:
//!
//! ```sh
//! PIECED_LOOK_OUT=/some/dir cargo test --locked --test viewmodel_offscreen -- --ignored --nocapture
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
    building::BuildingVisualsPlugin,
    combat::Loadout,
    fx::FxPlugin,
    look::LookPlugin,
    models::ModelsPlugin,
    render::{RenderSetupPlugin, WorldTarget},
    shared::{ActiveTool, Ads, AppState, Player, PlayerIntent, WeaponKind},
    viewmodel::{ViewmodelModels, ViewmodelPlugin},
};
use std::path::{Path, PathBuf};

fn app() -> App {
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

/// Runs frames for at least `seconds` of real time (the viewmodel's eases run
/// on real time, and offscreen frames are fast).
fn settle(app: &mut App, seconds: f32, setup: &impl Fn(&mut World)) {
    let start = std::time::Instant::now();
    while start.elapsed().as_secs_f32() < seconds {
        setup(app.world_mut());
        app.update();
    }
}

/// Runs `setup` on the player every frame for half a second and then until the
/// screenshot is written, so a pose settles and stays frozen while the capture
/// is in flight.
fn capture_with(app: &mut App, path: PathBuf, setup: impl Fn(&mut World)) {
    let _ = std::fs::remove_file(&path);
    let image = app.world().resource::<WorldTarget>().image.clone();
    settle(app, 0.5, &setup);
    setup(app.world_mut());
    app.world_mut()
        .spawn(Screenshot::image(image))
        .observe(save_to_disk(path.clone()));
    for _ in 0..30 {
        setup(app.world_mut());
        app.update();
        if path.exists() {
            app.update();
            return;
        }
    }
    panic!("no screenshot at {}", path.display());
}

fn player_mut<T: Component<Mutability = bevy::ecs::component::Mutable>>(
    world: &mut World,
    f: impl FnOnce(&mut T),
) {
    let mut q = world.query_filtered::<&mut T, With<Player>>();
    let mut c = q.single_mut(world).unwrap();
    f(&mut c);
}

fn hold(tool: ActiveTool, ads: bool) -> impl Fn(&mut World) {
    move |world: &mut World| {
        player_mut::<ActiveTool>(world, |t| *t = tool);
        player_mut::<Ads>(world, |a| a.0 = ads);
    }
}

/// Holds the gun `kind` mid-reload at progress `p` (and `ammo` loaded).
fn reloading(kind: WeaponKind, p: f32, ammo: u32) -> impl Fn(&mut World) {
    move |world: &mut World| {
        hold(ActiveTool::Weapon(kind), false)(world);
        let time = match kind {
            WeaponKind::Rifle => 2.0,
            WeaponKind::Pump => 0.5,
        };
        player_mut::<Loadout>(world, |l| {
            let gun = l.gun_mut(kind);
            gun.ammo = ammo;
            gun.reload = Some(time * (1.0 - p));
        });
    }
}

fn pipelines_ok(app: &App) {
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
    let rifle = ActiveTool::Weapon(WeaponKind::Rifle);
    let pump = ActiveTool::Weapon(WeaponKind::Pump);
    capture_with(app, out.join("vm-01-rifle-hip.png"), hold(rifle, false));
    frames(app, 20);
    capture_with(app, out.join("vm-02-rifle-ads.png"), hold(rifle, true));
    capture_with(app, out.join("vm-03-rifle-half-mag.png"), |w| {
        hold(rifle, false)(w);
        player_mut::<Ads>(w, |a| a.0 = false);
        player_mut::<Loadout>(w, |l| l.rifle.ammo = 8);
    });
    frames(app, 10);
    capture_with(
        app,
        out.join("vm-04-rifle-reload-pop.png"),
        reloading(WeaponKind::Rifle, 0.27, 8),
    );
    capture_with(
        app,
        out.join("vm-05-rifle-reload-slide.png"),
        reloading(WeaponKind::Rifle, 0.6, 8),
    );
    // Finish the reload so the rifle is full again.
    frames(app, 5);
    player_mut::<Loadout>(app.world_mut(), |l| {
        l.rifle.ammo = 30;
        l.rifle.reload = None;
    });
    frames(app, 30);
    capture_with(app, out.join("vm-06-pump-hip.png"), hold(pump, false));
    frames(app, 20);
    capture_with(app, out.join("vm-07-pump-ads.png"), hold(pump, true));
    player_mut::<Ads>(app.world_mut(), |a| a.0 = false);
    frames(app, 20);
    // Fire the pump and catch the rack.
    player_mut::<PlayerIntent>(app.world_mut(), |i| {
        i.fire = true;
        i.fire_pressed = true;
    });
    frames(app, 2);
    player_mut::<PlayerIntent>(app.world_mut(), |i| {
        i.fire = false;
        i.fire_pressed = false;
    });
    let start = std::time::Instant::now();
    while start.elapsed().as_secs_f32() < 0.24 {
        app.update();
    }
    let image = app.world().resource::<WorldTarget>().image.clone();
    let path = out.join("vm-08-pump-rack.png");
    let _ = std::fs::remove_file(&path);
    app.world_mut()
        .spawn(Screenshot::image(image))
        .observe(save_to_disk(path.clone()));
    frames(app, 6);
    frames(app, 30);
    capture_with(
        app,
        out.join("vm-09-pump-reload.png"),
        reloading(WeaponKind::Pump, 0.32, 2),
    );
}

#[test]
#[ignore = "needs a GPU: run by hand to review the viewmodel"]
fn render_the_viewmodel_offscreen() {
    let out = std::env::var("PIECED_LOOK_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("pieced-look"));
    std::fs::create_dir_all(&out).unwrap();
    let mut app = app();
    let mut boot_frames = 0;
    while *app.world().resource::<State<AppState>>().get() == AppState::Boot {
        app.update();
        boot_frames += 1;
        assert!(boot_frames < 600, "Boot never ended");
    }
    println!("Boot ended after {boot_frames} frames");
    assert!(
        app.world().resource::<ViewmodelModels>().is_ready(),
        "the guns and gloves are attached before play starts"
    );
    frames(&mut app, 10);
    review(&mut app, &out);
    pipelines_ok(&app);
    println!("wrote PNGs to {}", out.display());
}
