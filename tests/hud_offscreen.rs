//! Offscreen HUD review (slice F): runs the real client look, HUD and menu
//! plugins with no window, points the UI camera at an image so a capture holds
//! the world with the HUD and the menus over it, and writes PNGs to compare with
//! the targets:
//!
//! - `hud-spawn.png` (T01): the spawn view with the rifle, full bars, the hotbar;
//! - `hud-hit.png` and `hud-hit-pop.png` (T05, T06): the knight at about 10 m
//!   taking a shield hit, a body hit and a headshot (cyan, white and gold
//!   numbers), the frame after the hits (squash) and a few frames later;
//! - `hud-pump.png`, `hud-build.png`: the pump's violet crystal and ring, build
//!   mode;
//! - `hud-pause.png` (T12) and `hud-settings.png`: the pause menu and Settings;
//! - `hud-loading.png`: the Boot overlay with the logo.
//!
//! `PIECED_HUD_FONT=<file.ttf>` swaps in another font and `PIECED_HUD_PREFIX`
//! prefixes the file names (to compare fonts).
//! It needs a GPU, so it is ignored by default. Run it by hand:
//!
//! ```sh
//! PIECED_HUD_OUT=/some/dir cargo test --locked --test hud_offscreen -- --ignored --nocapture
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
    app::{BootGate, BootPlugin, SimPlugins},
    arena::visuals::ArenaVisualsPlugin,
    building::BuildingVisualsPlugin,
    dummy::{Dummy, look_toward},
    far::FarViewPlugin,
    fx::FxPlugin,
    hud::HudPlugin,
    look::LookPlugin,
    menu::{MenuPage, MenuPlugin, MenuState},
    models::ModelsPlugin,
    player::HEAD_CENTER,
    render::{RenderSetupPlugin, WorldTarget},
    scenario::{player_entity, set_look},
    shared::{
        ActiveTool, AppState, DamageDealt, DamageTarget, Health, PieceKind, PreviousFeet, SimTick,
        WeaponKind, tick_duration,
    },
    tuning::Tuning,
    viewmodel::ViewmodelPlugin,
};
use std::path::{Path, PathBuf};

fn hud_app() -> App {
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
        BootPlugin,
    ))
    // One fixed tick per frame, like a 60 Hz native run.
    .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()));
    if let Ok(font) = std::env::var("PIECED_HUD_FONT") {
        let bytes = std::fs::read(&font).expect("PIECED_HUD_FONT is readable");
        app.world_mut()
            .resource_mut::<Assets<Font>>()
            .insert(AssetId::default(), Font::from_bytes(bytes))
            .unwrap();
        println!("font: {font}");
    }
    app.finish();
    app.cleanup();
    app
}

/// Points the UI camera at an image the size of the world target.
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

/// Captures on the very next rendered frame (no extra updates first).
fn capture_now(app: &mut App, image: &Handle<Image>, path: PathBuf) {
    capture(app, image, path);
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

fn set_tool(app: &mut App, tool: ActiveTool) {
    let world = app.world_mut();
    let mut q = world.query_filtered::<&mut ActiveTool, With<pieced::shared::Player>>();
    *q.single_mut(world).unwrap() = tool;
}

fn feet_of(world: &World, e: Entity) -> Vec3 {
    world.get::<Transform>(e).unwrap().translation
}

/// Stands the knight `range` m in front of the player and looks at its chest.
fn knight_in_front(app: &mut App, range: f32) -> Vec3 {
    let world = app.world_mut();
    world.resource_mut::<Tuning>().dummy.stand_still = true;
    let player = player_entity(world).unwrap();
    let feet = feet_of(world, player);
    let spot = feet + Vec3::new(1.2, 0.0, -range);
    let dummy = world
        .query_filtered::<Entity, With<Dummy>>()
        .iter(world)
        .next()
        .unwrap();
    world.get_mut::<Transform>(dummy).unwrap().translation = spot;
    if let Some(mut p) = world.get_mut::<PreviousFeet>(dummy) {
        p.0 = spot;
    }
    let look = look_toward(spot + Vec3::Y * 1.2 - (feet + Vec3::Y * 1.62));
    set_look(world, look.yaw, look.pitch);
    spot
}

/// The HUD sees the player's hits as `DamageDealt` messages: write three
/// (shield, body, headshot) on this tick.
fn hit_knight(app: &mut App, spot: Vec3) {
    let world = app.world_mut();
    let player = player_entity(world).unwrap();
    let dummy = world
        .query_filtered::<Entity, With<Dummy>>()
        .iter(world)
        .next()
        .unwrap();
    let tick = world.resource::<SimTick>().0 + 1;
    let hit = |amount: f32, to_shield: f32, headshot: bool, height: f32| DamageDealt {
        source: Some(player),
        target: dummy,
        target_kind: DamageTarget::Character,
        amount,
        to_shield,
        headshot,
        shield_broke: false,
        killed: false,
        point: spot + Vec3::Y * height,
        normal: Vec3::Z,
        tick,
    };
    world.write_message(hit(28.0, 28.0, false, 1.0));
    world.write_message(hit(24.0, 0.0, false, 0.8));
    world.write_message(hit(48.0, 0.0, true, HEAD_CENTER));
}

fn review(app: &mut App, image: &Handle<Image>, out: &Path, prefix: &str) {
    set_tool(app, ActiveTool::Weapon(WeaponKind::Rifle));
    frames(app, 20);
    capture(app, image, out.join(format!("{prefix}hud-spawn.png")));

    // A hit on the knight: the player's bars take a knock too.
    let spot = knight_in_front(app, 10.0);
    {
        let world = app.world_mut();
        let player = player_entity(world).unwrap();
        world.get_mut::<Health>(player).unwrap().apply(35.0);
    }
    frames(app, 30);
    hit_knight(app, spot);
    frames(app, 1);
    capture_now(app, image, out.join(format!("{prefix}hud-hit-pop.png")));
    frames(app, 6);
    capture(app, image, out.join(format!("{prefix}hud-hit.png")));

    set_tool(app, ActiveTool::Weapon(WeaponKind::Pump));
    frames(app, 40);
    capture(app, image, out.join(format!("{prefix}hud-pump.png")));
    set_tool(app, ActiveTool::Build(PieceKind::Wall));
    frames(app, 30);
    capture(app, image, out.join(format!("{prefix}hud-build.png")));
    set_tool(app, ActiveTool::Weapon(WeaponKind::Rifle));

    // The pause menu over the spawn view, then Settings.
    {
        let mut menu = app.world_mut().resource_mut::<MenuState>();
        menu.menu_open = true;
        menu.page = MenuPage::Main;
    }
    frames(app, 10);
    capture(app, image, out.join(format!("{prefix}hud-pause.png")));
    app.world_mut().resource_mut::<MenuState>().page = MenuPage::Settings;
    frames(app, 10);
    capture(app, image, out.join(format!("{prefix}hud-settings.png")));
}

#[test]
#[ignore = "needs a GPU: run by hand to review the HUD"]
fn render_the_hud_offscreen() {
    let out = std::env::var("PIECED_HUD_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("pieced-hud"));
    std::fs::create_dir_all(&out).unwrap();
    let mut app = hud_app();
    // Hold Boot while the loading overlay (with the logo) is captured.
    app.world_mut()
        .resource_mut::<BootGate>()
        .hold("hud-review");
    app.update();
    let image = composite_ui(&mut app);
    frames(&mut app, 5);
    capture(&mut app, &image, out.join("hud-loading.png"));
    app.world_mut()
        .resource_mut::<BootGate>()
        .release("hud-review");
    let mut boot = 0;
    while *app.world().resource::<State<AppState>>().get() == AppState::Boot {
        app.update();
        boot += 1;
        assert!(boot < 600, "Boot never ended");
    }
    println!("Boot ended after {boot} frames");
    let prefix = std::env::var("PIECED_HUD_PREFIX").unwrap_or_default();
    review(&mut app, &image, &out, &prefix);
    assert_pipelines_ok(&app);
    println!("wrote PNGs to {}", out.display());
}
