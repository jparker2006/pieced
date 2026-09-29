//! Offscreen review of the first-person guns: runs the real client look,
//! model and viewmodel plugins with no window, renders into the world target
//! image, checks every pipeline compiled and Boot's warm-up finished, and
//! writes PNGs of the rifle and pump at the hip, aiming down sights, firing and
//! reloading, for comparison with targets T01, T02 and T04, and the M4
//! animation poses (D106: the ADS swing, the reload's flick, grab and slot,
//! the draw, the heavy rack and its clack, the shard push) for V2, V3 and V6.
//! Time runs in fixed 60 Hz ticks and is frozen while a screenshot is taken,
//! so a pose mid-animation is caught exactly.
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
    time::TimeUpdateStrategy,
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
    shared::{ActiveTool, Ads, AppState, Player, PlayerIntent, WeaponKind, tick_duration},
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
    ))
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

/// Stops (or restarts) the clock: every frame advances one 60 Hz tick of game
/// and animation time, or none while frozen, so a pose mid-animation holds
/// exactly while its screenshot is in flight.
fn freeze(app: &mut App, frozen: bool) {
    let step = if frozen {
        std::time::Duration::ZERO
    } else {
        tick_duration()
    };
    app.world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(step));
}

/// Writes the frame as it stands (the clock frozen) to `path`.
fn shoot(app: &mut App, path: PathBuf) {
    let _ = std::fs::remove_file(&path);
    let image = app.world().resource::<WorldTarget>().image.clone();
    freeze(app, true);
    app.world_mut()
        .spawn(Screenshot::image(image))
        .observe(save_to_disk(path.clone()));
    for _ in 0..30 {
        app.update();
        if path.exists() {
            app.update();
            freeze(app, false);
            return;
        }
    }
    panic!("no screenshot at {}", path.display());
}

/// Runs `setup` on the player every frame for 0.75 s of game time so a pose
/// settles.
fn settle(app: &mut App, setup: impl Fn(&mut World)) {
    for _ in 0..45 {
        setup(app.world_mut());
        app.update();
    }
    setup(app.world_mut());
}

/// Settles a pose, then holds it for the screenshot.
fn capture_with(app: &mut App, path: PathBuf, setup: impl Fn(&mut World)) {
    settle(app, setup);
    shoot(app, path);
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
        // Aiming follows the held Shift (Amendment A): hold it in the intent
        // too, or the next tick puts the gun back at the hip.
        player_mut::<PlayerIntent>(world, |i| i.ads_held = ads);
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

/// Pulls the trigger for one tick.
fn pull_trigger(app: &mut App) {
    player_mut::<PlayerIntent>(app.world_mut(), |i| {
        i.fire = true;
        i.fire_pressed = true;
    });
    frames(app, 1);
    player_mut::<PlayerIntent>(app.world_mut(), |i| {
        i.fire = false;
        i.fire_pressed = false;
    });
}

fn review(app: &mut App, out: &Path) {
    let rifle = ActiveTool::Weapon(WeaponKind::Rifle);
    let pump = ActiveTool::Weapon(WeaponKind::Pump);
    capture_with(app, out.join("vm-01-rifle-hip.png"), hold(rifle, false));
    capture_with(app, out.join("vm-02-rifle-ads.png"), hold(rifle, true));
    // Mid-ADS: the gun swinging up with weight, 4 frames into the 0.12 s blend.
    settle(app, hold(rifle, false));
    hold(rifle, true)(app.world_mut());
    frames(app, 5);
    shoot(app, out.join("vm-03-rifle-ads-swing.png"));
    capture_with(
        app,
        out.join("vm-04-rifle-reload-pop.png"),
        reloading(WeaponKind::Rifle, 0.28, 8),
    );
    capture_with(
        app,
        out.join("vm-05-rifle-reload-grab.png"),
        reloading(WeaponKind::Rifle, 0.6, 8),
    );
    capture_with(
        app,
        out.join("vm-06-rifle-reload-slot.png"),
        reloading(WeaponKind::Rifle, 0.79, 8),
    );
    // Finish the reload so the rifle is full again.
    player_mut::<Loadout>(app.world_mut(), |l| {
        l.rifle.ammo = 30;
        l.rifle.reload = None;
    });
    frames(app, 30);
    // The draw: switch to the pump and catch it rising in, overshooting.
    hold(pump, false)(app.world_mut());
    frames(app, 6);
    shoot(app, out.join("vm-07-pump-draw.png"));
    capture_with(app, out.join("vm-08-pump-hip.png"), hold(pump, false));
    capture_with(app, out.join("vm-09-pump-ads.png"), hold(pump, true));
    settle(app, hold(pump, false));
    // Fire the pump and catch the rack fully back, then the clack's jolt.
    pull_trigger(app);
    frames(app, 18);
    shoot(app, out.join("vm-10-pump-rack.png"));
    frames(app, 10);
    shoot(app, out.join("vm-11-pump-clack.png"));
    frames(app, 40);
    capture_with(
        app,
        out.join("vm-12-pump-reload-lift.png"),
        reloading(WeaponKind::Pump, 0.3, 2),
    );
    capture_with(
        app,
        out.join("vm-13-pump-reload-push.png"),
        reloading(WeaponKind::Pump, 0.47, 2),
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
