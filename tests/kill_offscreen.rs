//! Offscreen review of M4 chunk 1's hit feedback and kills (docs/M4-SPEC.md
//! → Chunk 1; targets M4-V2, V3, V4): the real client look, HUD, Waves HUD,
//! effects and knights with no window, the UI composited over the world, in a
//! Waves run. It writes PNGs of:
//!
//! - `kill-body-hit.png`: a rifle body hit at about 8 m (sparks, armor chips,
//!   the flinch), two frames on;
//! - `kill-body.png`: that knight shot dead (the white X, "+100", armor
//!   flying, the poof);
//! - `kill-headshot.png` and `kill-headshot-late.png`: a second knight
//!   headshot dead within the chain (the gold X, "DOUBLE!", "+100" and "+50
//!   HEADSHOT", the dented helmet popping off), and armor on the grass later;
//! - `kill-dent.png`: a third knight's dented helmet after a headshot, close.
//!
//! It needs a GPU, so it is ignored by default. Run it by hand:
//!
//! ```sh
//! PIECED_LOOK_OUT=/some/dir scripts/cargo.sh test --locked --test kill_offscreen -- --ignored --nocapture
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
    grunt::{GruntStats, Parked},
    hud::HudPlugin,
    look::LookPlugin,
    menu::MenuPlugin,
    models::ModelsPlugin,
    player::HEAD_CENTER,
    render::{RenderSetupPlugin, WorldTarget},
    scenario::{player_entity, set_look},
    shared::{
        ActiveTool, AppState, EyeHeight, GameMode, Health, PlayerIntent, PreviousFeet, WeaponKind,
        tick_duration,
    },
    viewmodel::ViewmodelPlugin,
    waves::{PoolGrunt, ui::WavesUiPlugin},
};
use std::path::{Path, PathBuf};

fn kill_app() -> App {
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
        BootPlugin,
    ))
    .insert_resource(GameMode::Waves)
    .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()));
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

fn in_play(app: &mut App) -> Vec<Entity> {
    let world = app.world_mut();
    let mut v: Vec<Entity> = world
        .query_filtered::<Entity, (With<PoolGrunt>, Without<Parked>, Without<Downed>)>()
        .iter(world)
        .collect();
    v.sort();
    v
}

fn feet(app: &App, e: Entity) -> Vec3 {
    app.world().get::<Transform>(e).unwrap().translation
}

/// Stands a knight at `at`, still, with `hp` health and no shield.
fn stand(app: &mut App, knight: Entity, at: Vec3, hp: f32) {
    let mut e = app.world_mut().entity_mut(knight);
    e.get_mut::<Transform>().unwrap().translation = at;
    e.get_mut::<PreviousFeet>().unwrap().0 = at;
    e.get_mut::<GruntStats>().unwrap().speed = 0.0;
    let mut health = e.get_mut::<Health>().unwrap();
    health.hp = hp;
    health.shield = 0.0;
}

fn aim(app: &mut App, point: Vec3) {
    let world = app.world_mut();
    let player = player_entity(world).unwrap();
    let eye = world.get::<Transform>(player).unwrap().translation
        + Vec3::Y * world.get::<EyeHeight>(player).unwrap().0;
    let look = look_toward(point - eye);
    set_look(world, look.yaw, look.pitch);
}

fn intent(app: &mut App, f: impl FnOnce(&mut PlayerIntent)) {
    let world = app.world_mut();
    let player = player_entity(world).unwrap();
    f(&mut world.get_mut::<PlayerIntent>(player).unwrap());
}

fn fire(app: &mut App) {
    intent(app, |i| {
        i.fire = true;
        i.fire_pressed = true;
    });
    app.update();
    intent(app, |i| {
        i.fire = false;
        i.fire_pressed = false;
    });
}

fn review(app: &mut App, image: &Handle<Image>, out: &Path) {
    // An unbeatable player with the rifle.
    {
        let world = app.world_mut();
        let player = player_entity(world).unwrap();
        let mut health = Health::full(1.0e9, 100.0);
        health.shield = 100.0;
        *world.get_mut::<Health>(player).unwrap() = health;
    }
    intent(app, |i| {
        i.select = Some(ActiveTool::Weapon(WeaponKind::Rifle))
    });
    // Wait for wave 1's three knights.
    let mut knights = Vec::new();
    for _ in 0..3000 {
        app.update();
        knights = in_play(app);
        if knights.len() >= 3 {
            break;
        }
    }
    assert!(knights.len() >= 3, "wave 1 landed: {}", knights.len());
    let player = player_entity(app.world_mut()).unwrap();
    let me = feet(app, player);
    // Out in the open toward the middle, looking along -Z.
    let base = Vec3::new(me.x, 0.0, me.z);
    let a = base + Vec3::new(-1.2, 0.0, -8.0);
    let b = base + Vec3::new(1.4, 0.0, -7.0);
    let c = base + Vec3::new(4.0, 0.0, -9.0);
    stand(app, knights[0], a, 100.0);
    stand(app, knights[1], b, 100.0);
    stand(app, knights[2], c, 100.0);
    aim(app, a + Vec3::Y * 1.0);
    frames(app, 40);
    for (k, at) in [(knights[0], a), (knights[1], b), (knights[2], c)] {
        stand(app, k, at, 100.0);
    }

    // V2: a body hit.
    aim(app, a + Vec3::Y * 1.0);
    fire(app);
    frames(app, 2);
    capture(app, image, out.join("kill-body-hit.png"));

    // A body kill, then a headshot kill within the chain (DOUBLE!).
    stand(app, knights[0], a, 1.0);
    frames(app, 12);
    aim(app, a + Vec3::Y * 1.0);
    fire(app);
    frames(app, 3);
    capture(app, image, out.join("kill-body.png"));
    stand(app, knights[1], b, 1.0);
    aim(app, b + Vec3::Y * HEAD_CENTER);
    frames(app, 8);
    aim(app, b + Vec3::Y * HEAD_CENTER);
    fire(app);
    frames(app, 4);
    capture(app, image, out.join("kill-headshot.png"));
    frames(app, 20);
    capture(app, image, out.join("kill-headshot-late.png"));

    // A dented helmet close up: a headshot that doesn't kill.
    frames(app, 60);
    let c2 = me + Vec3::new(0.4, 0.0, -3.4);
    stand(app, knights[2], c2, 500.0);
    frames(app, 20);
    stand(app, knights[2], c2, 500.0);
    aim(app, c2 + Vec3::Y * HEAD_CENTER);
    fire(app);
    frames(app, 40);
    stand(app, knights[2], c2, 500.0);
    aim(app, c2 + Vec3::Y * 1.35);
    frames(app, 3);
    capture(app, image, out.join("kill-dent.png"));
}

#[test]
#[ignore = "needs a GPU: run by hand to review the kill feedback"]
fn render_the_kill_feedback_offscreen() {
    let out = std::env::var("PIECED_LOOK_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("pieced-kills"));
    std::fs::create_dir_all(&out).unwrap();
    let mut app = kill_app();
    app.update();
    let image = composite_ui(&mut app);
    let mut boot = 0;
    while *app.world().resource::<State<AppState>>().get() == AppState::Boot {
        app.update();
        boot += 1;
        assert!(boot < 600, "Boot never ended");
    }
    println!("Boot ended after {boot} frames");
    review(&mut app, &image, &out);
    assert_pipelines_ok(&app);
    println!("wrote PNGs to {}", out.display());
}
