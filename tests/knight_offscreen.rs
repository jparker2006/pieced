//! Offscreen review of the knight in the real client look (docs/M2-SPEC.md → The
//! knight; targets T05, T06, T07): runs the look, model and figure plugins with
//! no window, renders into the world target image and writes PNGs of the knight
//! at about 10 m, close up, from three-quarters, running, just hit, headshot and
//! eliminated. The player stands where the camera is, so the dummy turns to face
//! it, as in play.
//!
//! It needs a GPU, so it is ignored by default. Run it by hand:
//!
//! ```sh
//! PIECED_LOOK_OUT=/some/dir cargo test --locked --test knight_offscreen -- --ignored --nocapture
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
    arena::ArenaLayout,
    arena::visuals::{ArenaVisualsPlugin, TargetFigure},
    building::BuildingVisualsPlugin,
    combat::Downed,
    dummy::Dummy,
    fx::FxPlugin,
    knight::KnightRig,
    look::{BlobDecal, LookPlugin},
    models::ModelsPlugin,
    movement::Motor,
    render::{RenderSetupPlugin, WorldTarget},
    scenario::gallery::GalleryCamera,
    shared::{
        AppState, Character, DamageDealt, DamageTarget, EyeHeight, Player, PreviousFeet, SimTick,
    },
    tuning::Tuning,
    viewmodel::ViewmodelPlugin,
};
use std::path::{Path, PathBuf};

fn knight_app() -> App {
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

fn capture(app: &mut App, path: PathBuf) {
    let _ = std::fs::remove_file(&path);
    let image = app.world().resource::<WorldTarget>().image.clone();
    app.world_mut()
        .spawn(Screenshot::image(image))
        .observe(save_to_disk(path.clone()));
    for _ in 0..30 {
        app.update();
        if path.exists() {
            app.update();
            return;
        }
    }
    panic!("no screenshot at {}", path.display());
}

fn dummy(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<Dummy>>()
        .single(world)
        .unwrap()
}

fn feet(app: &App, e: Entity) -> Vec3 {
    app.world().get::<Transform>(e).unwrap().translation
}

/// Moves a character (and its interpolation start) to `at`.
fn teleport(app: &mut App, who: Entity, at: Vec3) {
    let mut e = app.world_mut().entity_mut(who);
    e.get_mut::<Transform>().unwrap().translation = at;
    e.get_mut::<PreviousFeet>().unwrap().0 = at;
}

fn set_still(app: &mut App, still: bool) {
    app.world_mut().resource_mut::<Tuning>().dummy.stand_still = still;
}

/// Puts the player at `eye` (so the dummy faces it) and the camera there,
/// looking at `at`.
fn view(app: &mut App, eye: Vec3, at: Vec3) {
    let player = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<Player>>()
            .single(world)
            .unwrap()
    };
    let height = app.world().get::<EyeHeight>(player).map_or(1.6, |e| e.0);
    teleport(app, player, eye - Vec3::Y * height);
    app.insert_resource(GalleryCamera(
        Transform::from_translation(eye).looking_at(at, Vec3::Y),
    ));
}

fn hit(app: &mut App, target: Entity, headshot: bool) {
    let at = feet(app, target);
    app.world_mut().write_message(DamageDealt {
        source: None,
        target,
        target_kind: DamageTarget::Character,
        amount: if headshot { 48.0 } else { 24.0 },
        to_shield: 0.0,
        headshot,
        shield_broke: false,
        killed: false,
        point: at + Vec3::Y * if headshot { 1.7 } else { 1.0 },
        normal: Vec3::Z,
        tick: 0,
    });
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

/// Where the dummy's blob shadow sits relative to its feet (horizontal m).
fn shadow_offset(app: &mut App, dummy: Entity) -> f32 {
    let feet = feet(app, dummy);
    let world = app.world_mut();
    let figure = world
        .query::<(Entity, &TargetFigure)>()
        .iter(world)
        .find(|(_, f)| f.owner == dummy)
        .map(|(e, _)| e)
        .unwrap();
    let decal = world
        .query::<(&BlobDecal, &GlobalTransform)>()
        .iter(world)
        .find(|(d, _)| d.owner == figure)
        .map(|(_, t)| t.translation())
        .expect("the figure has a blob shadow");
    println!("shadow at {decal:.3}, feet at {feet:.3}");
    (decal.xz() - feet.xz()).length()
}

fn review(app: &mut App, out: &Path) {
    let dummy = dummy(app);
    set_still(app, true);
    frames(app, 20);
    // Views are laid out from the dummy towards the player's spawn (clear ground).
    let spot = feet(app, dummy);
    let spawn = app.world().resource::<ArenaLayout>().player_spawn;
    let back = (spawn - spot).with_y(0.0).normalize();
    let side = Vec3::Y.cross(back);
    let at = |ahead: f32, right: f32, up: f32| spot + back * ahead + side * right + Vec3::Y * up;
    let rigged = {
        let world = app.world_mut();
        world.query::<&KnightRig>().iter(world).count()
    };
    assert_eq!(rigged, 1, "the dummy wears the rigged knight");

    // T05-like: about 10 m out, a little off-axis, from eye height.
    let chest = spot + Vec3::Y * 1.0;
    view(app, at(9.7, 2.5, 1.6), chest);
    frames(app, 20);
    capture(app, out.join("knight-10m.png"));
    let off = shadow_offset(app, dummy);
    assert!(off < 0.05, "blob shadow {off:.3} m off the feet");

    view(app, at(3.2, 0.4, 1.6), spot + Vec3::Y * 1.05);
    frames(app, 20);
    capture(app, out.join("knight-close.png"));

    // Three-quarters: the player in front, the camera off to his left.
    view(app, at(6.0, 0.0, 1.6), chest);
    frames(app, 10);
    app.insert_resource(GalleryCamera(
        Transform::from_translation(at(2.4, -2.6, 1.5)).looking_at(chest, Vec3::Y),
    ));
    frames(app, 10);
    capture(app, out.join("knight-3q.png"));

    // A body hit (T05) and a headshot (T06), close.
    view(app, at(4.0, 0.8, 1.6), chest + Vec3::Y * 0.2);
    frames(app, 30);
    hit(app, dummy, false);
    frames(app, 4);
    capture(app, out.join("knight-hit.png"));
    frames(app, 60);
    hit(app, dummy, true);
    frames(app, 5);
    capture(app, out.join("knight-headshot.png"));
    frames(app, 60);

    // Eliminated: X eyes, hat gone (the effect drops its own).
    let tick = app.world().resource::<SimTick>().0;
    app.world_mut().entity_mut(dummy).insert(Downed { tick });
    frames(app, 3);
    capture(app, out.join("knight-ko.png"));
    app.world_mut().entity_mut(dummy).remove::<Downed>();
    frames(app, 90);

    // Running: the dummy strafes (no jumps), seen from about 7 m.
    app.world_mut()
        .resource_mut::<Tuning>()
        .dummy
        .jump_chance_per_sec = 0.0;
    set_still(app, false);
    view(app, at(7.0, 1.5, 1.6), chest);
    frames(app, 40);
    for k in 0..3 {
        let (y, grounded) = {
            let e = app.world().entity(dummy);
            (
                e.get::<Transform>().unwrap().translation.y,
                e.get::<Motor>().is_some_and(|m| m.grounded),
            )
        };
        println!("run frame {k}: feet y {y:.3}, grounded {grounded}");
        capture(app, out.join(format!("knight-run{k}.png")));
        frames(app, 5);
    }
}

#[test]
#[ignore = "needs a GPU: run by hand to review the knight"]
fn render_the_knight_offscreen() {
    let out = std::env::var("PIECED_LOOK_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("pieced-knight"));
    std::fs::create_dir_all(&out).unwrap();
    let mut app = knight_app();
    let mut boot = 0;
    while *app.world().resource::<State<AppState>>().get() == AppState::Boot {
        app.update();
        boot += 1;
        assert!(boot < 400, "Boot never ended");
    }
    println!("Boot ended after {boot} frames");
    frames(&mut app, 10);
    let characters = {
        let world = app.world_mut();
        world.query::<&Character>().iter(world).count()
    };
    assert_eq!(characters, 2);
    review(&mut app, &out);
    assert_pipelines_ok(&app);
    println!("wrote PNGs to {}", out.display());
}
