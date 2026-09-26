//! Offscreen review of the spells (docs/M2-SPEC.md → Spells and effects;
//! targets T03–T08): runs the real client look, models, knight, viewmodel and
//! effects with no window, fires real shots through the simulation, holds
//! each moment with the gallery freeze and writes PNGs of the world image:
//! the rifle bolt mid-flight, the pump fan, a shield hit, a body hit, a
//! headshot, a shield break (shards, then the circling stars), the poof with
//! the hat dropping and then lying on the grass, brick chips and splinters off
//! pieces, and a fizzle on the grass. It checks every pipeline compiled.
//!
//! It needs a GPU, so it is ignored by default. Run it by hand:
//!
//! ```sh
//! PIECED_LOOK_OUT=/some/dir cargo test --locked --test spells_offscreen -- --ignored --nocapture
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
    building::{BuildingVisualsPlugin, PieceSlot, clear_pieces, place_piece},
    combat::Downed,
    dummy::{Dummy, look_toward},
    fx::{FxPlugin, spells::SpellTiming},
    look::LookPlugin,
    models::ModelsPlugin,
    render::{RenderSetupPlugin, WorldTarget},
    scenario::{set_look, teleport, with_intent},
    shared::{
        ActiveTool, AppState, Facing, GalleryFreeze, GridCell, Health, Player, PreviousFeet,
        WeaponKind, tick_duration,
    },
    tuning::Tuning,
    viewmodel::ViewmodelPlugin,
};
use std::path::{Path, PathBuf};

fn spells_app() -> App {
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
    // One fixed tick per frame, 60 frames a second: the game's timeline.
    .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()));
    app.finish();
    app.cleanup();
    app
}

fn frames(app: &mut App, n: u32) {
    for _ in 0..n {
        app.update();
    }
}

/// Screenshots the world image (the freeze holds the moment meanwhile).
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

fn entity<C: Component>(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<C>>()
        .single(world)
        .unwrap()
}

/// The knight standing at `feet` with this health, the shot's previous
/// effects gone.
fn knight(app: &mut App, feet: Vec3, hp: f32, shield: f32) {
    let dummy = entity::<Dummy>(app);
    let mut e = app.world_mut().entity_mut(dummy);
    e.remove::<Downed>();
    e.get_mut::<Transform>().unwrap().translation = feet;
    e.get_mut::<PreviousFeet>().unwrap().0 = feet;
    let mut h = e.get_mut::<Health>().unwrap();
    h.hp = hp;
    h.shield = shield;
}

/// The player at `feet` aiming at `at` (the camera is the player's eye).
fn stand(app: &mut App, feet: Vec3, at: Vec3) {
    teleport(app.world_mut(), feet);
    let look = look_toward(at - (feet + Vec3::Y * 1.62));
    set_look(app.world_mut(), look.yaw, look.pitch);
}

fn hold(app: &mut App, tool: ActiveTool) {
    with_intent(app.world_mut(), |i| i.select = Some(tool));
    frames(app, 30);
}

/// Fires, then holds the moment `after` frames past the frame the shot lands
/// (0 = that very frame) and captures it.
fn shoot_and_capture(app: &mut App, after: u32, out: &Path, name: &str) {
    with_intent(app.world_mut(), |i| {
        i.fire = true;
        i.fire_pressed = true;
    });
    app.update();
    with_intent(app.world_mut(), |i| i.fire = false);
    frames(app, after);
    app.world_mut().insert_resource(GalleryFreeze);
    capture(app, out.join(format!("{name}.png")));
    app.world_mut().remove_resource::<GalleryFreeze>();
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

const RIFLE: ActiveTool = ActiveTool::Weapon(WeaponKind::Rifle);
const PUMP: ActiveTool = ActiveTool::Weapon(WeaponKind::Pump);

fn review(app: &mut App, out: &Path) {
    app.world_mut().resource_mut::<Tuning>().dummy.stand_still = true;
    app.world_mut()
        .resource_mut::<Tuning>()
        .feedback
        .camera_shake = 0.0;
    let me = Vec3::new(2.0, 0.0, 12.0);
    let chest = |feet: Vec3| feet + Vec3::Y * 1.05;

    // T03: the bolt in flight at the knight 14 m out, one frame after the hit.
    let far = Vec3::new(-1.0, 0.0, -2.0);
    knight(app, far, 100.0, 0.0);
    stand(app, me, chest(far));
    frames(app, 40);
    shoot_and_capture(app, 1, out, "spell-T03-bolt");
    frames(app, 40);

    // T04: the pump fan at 5 m.
    let near = Vec3::new(2.4, 0.0, 7.0);
    knight(app, near, 100.0, 0.0);
    stand(app, me, chest(near) + Vec3::X * 0.4);
    hold(app, PUMP);
    frames(app, 30);
    shoot_and_capture(app, 1, out, "spell-T04-pump-fan");
    frames(app, 60);
    hold(app, RIFLE);

    // A shield hit (the hex shimmer) and a body hit (T05) at 8 m.
    let mid = Vec3::new(2.0, 0.0, 4.0);
    knight(app, mid, 100.0, 100.0);
    stand(app, me, chest(mid));
    frames(app, 40);
    shoot_and_capture(app, 1, out, "spell-shield-hit");
    frames(app, 40);
    knight(app, mid, 100.0, 0.0);
    frames(app, 20);
    shoot_and_capture(app, 2, out, "spell-T05-body-hit");
    frames(app, 40);

    // T06: a headshot.
    knight(app, mid, 100.0, 0.0);
    stand(app, me, mid + Vec3::Y * 1.62);
    frames(app, 40);
    shoot_and_capture(app, 6, out, "spell-T06-headshot");
    frames(app, 40);

    // T07: a shield break: the glass bursting off, then the stars.
    knight(app, mid, 100.0, 10.0);
    stand(app, me, chest(mid));
    frames(app, 40);
    shoot_and_capture(app, 8, out, "spell-T07-shield-break");
    frames(app, 22);
    app.world_mut().insert_resource(GalleryFreeze);
    capture(app, out.join("spell-T07-stars.png"));
    app.world_mut().remove_resource::<GalleryFreeze>();
    frames(app, 60);

    // T08: the elimination poof, the hat dropping (as the gallery catches it)
    // and then lying on the grass.
    app.world_mut().resource_mut::<Tuning>().dummy.respawn_delay = 30.0;
    knight(app, mid, 20.0, 0.0);
    stand(app, me, chest(mid));
    frames(app, 40);
    shoot_and_capture(app, 27, out, "spell-T08-poof");
    frames(app, 60);
    app.world_mut().insert_resource(GalleryFreeze);
    capture(app, out.join("spell-T08-hat-on-grass.png"));
    app.world_mut().remove_resource::<GalleryFreeze>();
    knight(app, Vec3::new(-18.0, 0.0, -18.0), 100.0, 0.0);
    frames(app, 30);

    // Piece hits: brick chips off a wall, splinters off a floor.
    let wall = place_piece(
        app.world_mut(),
        PieceSlot::wall(GridCell::new(6, 7, 0), Facing::North),
    )
    .ok();
    assert!(wall.is_some(), "the wall was placed");
    let wall_center = PieceSlot::wall(GridCell::new(6, 7, 0), Facing::North).center();
    stand(
        app,
        Vec3::new(2.5, 0.0, 6.0),
        wall_center + Vec3::new(0.3, -0.2, 0.0),
    );
    frames(app, 40);
    shoot_and_capture(app, 3, out, "spell-brick-chips");
    frames(app, 40);
    clear_pieces(app.world_mut());
    let floor = PieceSlot::floor(GridCell::new(6, 7, 0));
    place_piece(app.world_mut(), floor).expect("floor placed");
    stand(
        app,
        Vec3::new(2.5, 0.0, 6.0),
        floor.center() + Vec3::Y * 0.1,
    );
    frames(app, 40);
    shoot_and_capture(app, 3, out, "spell-wood-splinters");
    frames(app, 40);
    clear_pieces(app.world_mut());

    // A miss that fizzles on the grass.
    stand(app, me, me + Vec3::new(1.0, 0.0, -5.0));
    frames(app, 40);
    shoot_and_capture(app, 2, out, "spell-miss-fizzle");

    let timing = app.world().resource::<SpellTiming>();
    println!(
        "hits {} (impact same frame {}), bolts {} (landed within 2 frames {})",
        timing.character_hits,
        timing.impacts_same_frame,
        timing.bolts_fired,
        timing.bolts_within_two_frames
    );
    // (Bolts caught mid-flight are held there by the freeze, so only the
    // impacts are checked here; tests/spells.rs times the bolts.)
    assert_eq!(timing.impacts_same_frame, timing.character_hits);
}

#[test]
#[ignore = "needs a GPU: run by hand to review the spells"]
fn render_the_spells_offscreen() {
    let out = std::env::var("PIECED_LOOK_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("pieced-spells"));
    std::fs::create_dir_all(&out).unwrap();
    let mut app = spells_app();
    let mut boot = 0;
    while *app.world().resource::<State<AppState>>().get() == AppState::Boot {
        app.update();
        boot += 1;
        assert!(boot < 600, "Boot never ended");
    }
    println!("Boot ended after {boot} frames");
    let player = entity::<Player>(&mut app);
    assert!(app.world().get::<Transform>(player).is_some());
    frames(&mut app, 10);
    review(&mut app, &out);
    assert_pipelines_ok(&app);
    println!("wrote PNGs to {}", out.display());
}
