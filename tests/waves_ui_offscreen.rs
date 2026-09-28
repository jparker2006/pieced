//! Offscreen review of the Waves screens (M3 chunk 2B): runs the real client
//! look, HUD, menu and Waves UI plugins with no window, in a Waves run, and
//! writes PNGs to compare with T01/T12:
//!
//! - `waves-fight.png`: the run HUD along the top, knights in, a shield
//!   potion bobbing in front of the player;
//! - `waves-potion.png`: the potion drunk (sparkle burst, "+25");
//! - `waves-break.png`: "WAVE 1 CLEARED!" and the countdown;
//! - `waves-death.png`, `waves-death-late.png`: mid death beat and on the
//!   grass (the view dropping, the vignette closing in);
//! - `waves-results.png`: the results card with "NEW BEST!".
//!
//! It needs a GPU, so it is ignored by default. Run it by hand:
//!
//! ```sh
//! PIECED_HUD_OUT=/some/dir cargo test --locked --test waves_ui_offscreen -- --ignored --nocapture
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
    combat::Downed,
    dummy::look_toward,
    far::FarViewPlugin,
    fx::FxPlugin,
    grunt::Parked,
    hud::HudPlugin,
    look::LookPlugin,
    menu::MenuPlugin,
    models::ModelsPlugin,
    render::{RenderSetupPlugin, WorldTarget},
    scenario::{player_entity, set_look},
    shared::{
        AppState, DamageDealt, DamageTarget, GalleryFreeze, GameMode, Health, PreviousFeet,
        SimTick, tick_duration,
    },
    viewmodel::ViewmodelPlugin,
    waves::{
        LivePotion, PoolGrunt, PotionSlot, Run, RunPhase, WavesClientPlugin, ui::WavesUiPlugin,
    },
};
use std::path::PathBuf;

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
        FarViewPlugin,
        BuildingVisualsPlugin,
        ViewmodelPlugin,
        FxPlugin,
        HudPlugin,
        MenuPlugin,
        WavesUiPlugin,
        WavesClientPlugin,
        BootPlugin,
    ))
    .insert_resource(GameMode::Waves)
    .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()));
    app.finish();
    app.cleanup();
    app
}

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

fn frames(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

fn capture(app: &mut App, image: &Handle<Image>, path: PathBuf) {
    let _ = std::fs::remove_file(&path);
    app.world_mut()
        .spawn(Screenshot::image(image.clone()))
        .observe(save_to_disk(path.clone()));
    for _ in 0..30 {
        app.update();
        if path.exists() {
            std::thread::sleep(std::time::Duration::from_millis(200));
            println!("wrote {}", path.display());
            return;
        }
    }
    panic!("no screenshot at {}", path.display());
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

fn in_play(world: &mut World) -> Vec<Entity> {
    world
        .query_filtered::<Entity, (With<PoolGrunt>, Without<Parked>, Without<Downed>)>()
        .iter(world)
        .collect()
}

fn kill_all(world: &mut World) {
    let player = player_entity(world).unwrap();
    let tick = world.resource::<SimTick>().0;
    for g in in_play(world) {
        let at = world.get::<Transform>(g).unwrap().translation;
        world.get_mut::<Health>(g).unwrap().hp = 0.0;
        world.entity_mut(g).insert(Downed { tick });
        world.write_message(DamageDealt {
            source: Some(player),
            target: g,
            target_kind: DamageTarget::Character,
            amount: 80.0,
            to_shield: 0.0,
            headshot: false,
            shield_broke: false,
            killed: true,
            point: at + Vec3::Y,
            normal: Vec3::Z,
            tick,
        });
    }
}

#[test]
#[ignore = "needs a GPU: run by hand to review the Waves screens"]
fn render_the_waves_screens_offscreen() {
    let out = std::env::var("PIECED_HUD_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("pieced-waves-ui"));
    std::fs::create_dir_all(&out).unwrap();
    let mut app = app();
    app.update();
    let image = composite_ui(&mut app);
    let mut boot = 0;
    while *app.world().resource::<State<AppState>>().get() == AppState::Boot {
        app.update();
        boot += 1;
        assert!(boot < 600, "Boot never ended");
    }
    // Knights hold still where they land; the player can't die (yet).
    {
        let world = app.world_mut();
        world.insert_resource(GalleryFreeze);
        let player = player_entity(world).unwrap();
        *world.get_mut::<Health>(player).unwrap() = Health::full(1.0e9, 100.0);
    }
    frames(&mut app, 150);

    // A potion on the grass 3 m ahead, the view on it and a knight.
    let (player, feet) = {
        let world = app.world_mut();
        let player = player_entity(world).unwrap();
        let feet = world.get::<Transform>(player).unwrap().translation;
        (player, feet)
    };
    let knight = in_play(app.world_mut()).first().copied();
    let ahead = {
        let world = app.world_mut();
        let target = knight
            .map(|k| world.get::<Transform>(k).unwrap().translation)
            .unwrap_or(feet + Vec3::new(0.0, 0.0, -12.0));
        let dir = (target - feet).with_y(0.0).normalize_or(Vec3::NEG_Z);
        let look = look_toward(target + Vec3::Y * 0.6 - (feet + Vec3::Y * 1.62));
        set_look(world, look.yaw, look.pitch - 0.12);
        feet + dir * 3.0 + dir.cross(Vec3::Y) * 0.6
    };
    {
        let world = app.world_mut();
        let now = world.resource::<SimTick>().0;
        let slot = world
            .query_filtered::<Entity, With<PotionSlot>>()
            .iter(world)
            .next()
            .unwrap();
        world.get_mut::<PotionSlot>(slot).unwrap().live = Some(LivePotion {
            at: ahead,
            dropped: now,
            expires: now + 60 * 60,
        });
        world.get_mut::<Transform>(slot).unwrap().translation = ahead;
        *world.get_mut::<Visibility>(slot).unwrap() = Visibility::Inherited;
        world.resource_mut::<Run>().score = 1_250;
    }
    frames(&mut app, 40);
    capture(&mut app, &image, out.join("waves-fight.png"));

    // Walk onto the potion (the burst and the "+25"), with some shield gone.
    {
        let world = app.world_mut();
        world.get_mut::<Health>(player).unwrap().shield = 40.0;
        world.get_mut::<Transform>(player).unwrap().translation = ahead;
        if let Some(mut p) = world.get_mut::<PreviousFeet>(player) {
            p.0 = ahead;
        }
    }
    frames(&mut app, 8);
    capture(&mut app, &image, out.join("waves-potion.png"));

    // The wave cleared: the break's banner.
    kill_all(app.world_mut());
    for _ in 0..600 {
        app.update();
        kill_all(app.world_mut());
        if matches!(app.world().resource::<Run>().phase, RunPhase::Break { .. }) {
            break;
        }
    }
    frames(&mut app, 30);
    capture(&mut app, &image, out.join("waves-break.png"));

    // The player's elimination: mid death beat.
    app.world_mut().resource_mut::<Run>().wave = 7;
    {
        let world = app.world_mut();
        world.get_mut::<Health>(player).unwrap().apply(1.0e12);
    }
    frames(&mut app, 30);
    capture(&mut app, &image, out.join("waves-death.png"));
    frames(&mut app, 12);
    capture(&mut app, &image, out.join("waves-death-late.png"));

    // The results.
    for _ in 0..300 {
        app.update();
        if app.world().resource::<Run>().is_over() {
            break;
        }
    }
    frames(&mut app, 40);
    capture(&mut app, &image, out.join("waves-results.png"));
    assert_pipelines_ok(&app);
    println!("wrote PNGs to {}", out.display());
}
