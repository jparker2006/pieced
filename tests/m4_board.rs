//! Offscreen captures of Milestone 4's target board (docs/M4-SPEC.md → The
//! target board, D102, D112): the whole client (`ClientPlugins` without the
//! native window and audio) with no window, the UI composited over the world,
//! in a Waves run. Each view is scripted through intents, `teleport`,
//! `set_look`, the knights' own brains and the gallery freeze, and written as
//! `<id>.png` plus a greyscale copy `<id>-grey.png`, to compare with
//! `docs/design/concepts/M4-V1` … `M4-V8`:
//!
//! - `V7-wave-banner`: a wave starting (wave 7): the brass-and-crystal
//!   "WAVE 7" banner (M4 chunk 5) over the island, the drop ships swooping
//!   down from the castle, from the spawn looking north;
//! - `V1-spawn-midwave`: the same wave a few seconds on, the ships hovering
//!   and knights running in at the player from 7–12 m;
//! - `V5-knight-windup`: a knight about 7 m out, facing the player, deep in
//!   his wand wind-up, others behind him;
//! - `V2-rifle-ads`: aiming down the rifle's sights, a body hit on a knight at
//!   about 9 m, the castle to the left;
//! - `V4-headshot-kill`: a body kill, then a headshot kill within the chain
//!   (the gold X, "DOUBLE!", the helmet popping off), the castle behind;
//! - `V3-pump-blast`: the pump killing a knight at 3 m, the castle to the right;
//! - `V6-boxup-fight`: boxed up in four brick walls under a floor, looking
//!   out of a wide window at knights firing orbs at the walls;
//! - `V8-main-menu`: the main menu over its orbit;
//! - `X-knight-close`: an art-review extra, a knight at 3 m, still.
//!
//! Output goes to `PIECED_BOARD_OUT` (default `<tmp>/pieced-board`).
//! `PIECED_BOARD_ONLY=V1,V5` captures only the listed views (the script still
//! plays in order; the others just aren't written).
//!
//! It needs a GPU, so it is ignored by default. Run it by hand:
//!
//! ```sh
//! PIECED_BOARD_OUT=/some/dir scripts/cargo.sh test --locked --test m4_board -- --ignored --nocapture
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
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    time::TimeUpdateStrategy,
    window::ExitCondition,
    winit::WinitPlugin,
};
use pieced::{
    app::{ClientPlugins, SimPlugins},
    audio::GameAudioPlugin,
    building::{PieceEdit, PieceSlot, edit_piece, place_piece},
    combat::Downed,
    dummy::look_toward,
    grunt::{GruntStats, Parked},
    native::NativeWindowPlugin,
    orb::Wand,
    player::HEAD_CENTER,
    render::WorldTarget,
    scenario::{player_entity, set_look, teleport},
    shared::{
        ActiveTool, AppState, EyeHeight, Facing, GalleryFreeze, GameMode, GridCell, Health,
        LookAngles, PlayerIntent, PreviousFeet, WeaponKind, tick_duration,
    },
    waves::{PoolGrunt, Run, ships::Ships},
};
use std::path::{Path, PathBuf};

fn board_app() -> App {
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
    app
}

/// Where the captures go, which to write, and what's still being written.
struct Board {
    out: PathBuf,
    only: Vec<String>,
    image: Handle<Image>,
    written: Vec<PathBuf>,
}

impl Board {
    fn wants(&self, id: &str) -> bool {
        self.only.is_empty() || self.only.iter().any(|o| id.starts_with(o.as_str()))
    }
}

/// Points the UI camera at an image the size of the world target, so a capture
/// holds the world with the HUD and menus drawn over it.
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

/// While set, no knight in play may cast (checked every frame), so no stray
/// orb bursts over the player's view in the shooting views.
static HUSHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn set_hushed(on: bool) {
    HUSHED.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Keeps the player alive with full bars (the knights keep shooting at him),
/// and the knights quiet while [`HUSHED`].
fn heal(app: &mut App) {
    if HUSHED.load(std::sync::atomic::Ordering::Relaxed) {
        let world = app.world_mut();
        let mut q = world.query_filtered::<(&mut GruntStats, &mut Wand), With<PoolGrunt>>();
        for (mut stats, mut wand) in q.iter_mut(world) {
            stats.reaction = 1.0e6;
            stats.fire_interval = 1.0e6;
            wand.windup = None;
            wand.cooldown = 1.0e6;
        }
    }
    if let Some(p) = player_entity(app.world_mut())
        && let Some(mut health) = app.world_mut().get_mut::<Health>(p)
    {
        let mut full = Health::full(100.0, 100.0);
        full.shield = 100.0;
        *health = full;
    }
}

fn frames(app: &mut App, n: usize) {
    for _ in 0..n {
        heal(app);
        app.update();
    }
}

/// Captures the next rendered frame as `<name>.png` and `<name>-grey.png`.
/// The gallery freeze holds knights, effects and the dummy while it renders.
fn capture(app: &mut App, board: &mut Board, name: &str) {
    capture_with(app, board, name, true);
}

/// As [`capture`]; without the freeze, the knights' brains keep acting in the
/// captured frame (a wand wind-up would cancel under the freeze).
fn capture_with(app: &mut App, board: &mut Board, name: &str, freeze: bool) {
    if !board.wants(name) {
        return;
    }
    let path = board.out.join(format!("{name}.png"));
    let grey = board.out.join(format!("{name}-grey.png"));
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&grey);
    board.written.extend([path.clone(), grey.clone()]);
    if freeze {
        app.world_mut().insert_resource(GalleryFreeze);
    }
    app.world_mut()
        .spawn(Screenshot::image(board.image.clone()))
        .observe(move |capture: On<ScreenshotCaptured>| {
            let image = capture.image.clone();
            let (path, grey) = (path.clone(), grey.clone());
            std::thread::spawn(move || {
                let image = image.try_into_dynamic().expect("screenshot converts");
                image.to_rgb8().save(&path).expect("PNG written");
                image.to_luma8().save(&grey).expect("grey PNG written");
            });
        });
    frames(app, 3);
    app.world_mut().remove_resource::<GalleryFreeze>();
    println!("captured {name}");
}

fn wait_for_files(app: &mut App, files: &[PathBuf]) {
    for _ in 0..600 {
        if files.iter().all(|f| f.exists()) {
            std::thread::sleep(std::time::Duration::from_millis(400));
            return;
        }
        app.update();
    }
    let missing: Vec<&Path> = files
        .iter()
        .filter(|f| !f.exists())
        .map(PathBuf::as_path)
        .collect();
    panic!("captures never written: {missing:?}");
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

// ---------------------------------------------------------------------------
// Scripting helpers
// ---------------------------------------------------------------------------

/// Unit vector along the ground at `azimuth` degrees clockwise from north
/// (-Z) toward east (+X), as `far::layout` measures bearings.
fn bearing(azimuth_deg: f32) -> Vec3 {
    let a = azimuth_deg.to_radians();
    Vec3::new(a.sin(), 0.0, -a.cos())
}

fn player(app: &mut App) -> Entity {
    player_entity(app.world_mut()).expect("the player")
}

fn eye(app: &mut App) -> Vec3 {
    let p = player(app);
    let world = app.world();
    world.get::<Transform>(p).unwrap().translation + Vec3::Y * world.get::<EyeHeight>(p).unwrap().0
}

/// Looks along `azimuth` (degrees, clockwise from north) and `pitch` (degrees up).
fn look(app: &mut App, azimuth_deg: f32, pitch_deg: f32) {
    let dir = bearing(azimuth_deg);
    let l = look_toward(dir);
    set_look(app.world_mut(), l.yaw, pitch_deg.to_radians());
}

fn aim(app: &mut App, point: Vec3) {
    let from = eye(app);
    let l = look_toward(point - from);
    set_look(app.world_mut(), l.yaw, l.pitch);
}

fn intent(app: &mut App, f: impl FnOnce(&mut PlayerIntent)) {
    let p = player(app);
    f(&mut app.world_mut().get_mut::<PlayerIntent>(p).unwrap());
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

fn select(app: &mut App, weapon: WeaponKind) {
    intent(app, |i| i.select = Some(ActiveTool::Weapon(weapon)));
    frames(app, 40);
}

/// The player at `at` with full bars, standing still.
fn put_player(app: &mut App, at: Vec3) {
    heal(app);
    teleport(app.world_mut(), at);
}

/// Holds (or lets go of) the aim key, as the input adapter reads it.
fn hold_aim(app: &mut App, on: bool) {
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    if on {
        keys.press(KeyCode::ShiftLeft);
    } else {
        keys.release(KeyCode::ShiftLeft);
    }
}

/// The knights in play (landed, up), in a stable order.
fn in_play(app: &mut App) -> Vec<Entity> {
    let world = app.world_mut();
    let mut v: Vec<Entity> = world
        .query_filtered::<Entity, (With<PoolGrunt>, Without<Parked>, Without<Downed>)>()
        .iter(world)
        .collect();
    v.sort();
    v
}

/// Runs until at least `n` knights are in play (at most `limit` frames).
fn knights(app: &mut App, n: usize, limit: usize) -> Vec<Entity> {
    let mut k = in_play(app);
    for _ in 0..limit {
        if k.len() >= n {
            break;
        }
        heal(app);
        app.update();
        k = in_play(app);
    }
    assert!(k.len() >= n, "only {} knights in play", k.len());
    k
}

/// Stands a knight at `at` facing the player, held still (no moving, no
/// firing), with `hp` health and no shield.
/// Stops every knight in play from casting (so no stray orb bursts over the
/// player's view), and clears the orbs already flying.
fn hush(app: &mut App) {
    set_hushed(true);
    frames(app, 90);
}

fn stand(app: &mut App, knight: Entity, at: Vec3, hp: f32) {
    let to = eye(app) - (at + Vec3::Y * 1.5);
    let mut e = app.world_mut().entity_mut(knight);
    e.get_mut::<Transform>().unwrap().translation = at;
    e.get_mut::<PreviousFeet>().unwrap().0 = at;
    let facing = look_toward(to);
    *e.get_mut::<LookAngles>().unwrap() = facing;
    let mut stats = e.get_mut::<GruntStats>().unwrap();
    stats.speed = 0.0;
    stats.reaction = 1.0e6;
    stats.fire_interval = 1.0e6;
    let mut health = e.get_mut::<Health>().unwrap();
    health.hp = hp;
    health.shield = 0.0;
}

/// Puts a knight back in the fight at `at` (its wave's stats, running).
fn release(app: &mut App, knight: Entity, at: Vec3, wave: u32) {
    let stats = GruntStats::for_wave(
        wave,
        &app.world().resource::<pieced::tuning::Tuning>().grunt,
    );
    let mut e = app.world_mut().entity_mut(knight);
    e.get_mut::<Transform>().unwrap().translation = at;
    e.get_mut::<PreviousFeet>().unwrap().0 = at;
    *e.get_mut::<GruntStats>().unwrap() = stats;
}

fn set_wave(app: &mut App, wave: u32) {
    let mut run = app.world_mut().resource_mut::<Run>();
    run.wave = wave;
    run.remaining = run.remaining.max(10 + wave);
}

// ---------------------------------------------------------------------------
// The board
// ---------------------------------------------------------------------------

/// The spawn's feet (cell (6, 9)'s centre).
const SPAWN: Vec3 = Vec3::new(2.0, 0.0, 14.0);

fn review(app: &mut App, board: &mut Board) {
    put_player(app, SPAWN);
    intent(app, |i| {
        i.select = Some(ActiveTool::Weapon(WeaponKind::Rifle))
    });
    set_wave(app, 7);
    let me = SPAWN;

    // V7: the wave's ships swoop down from the castle.
    look(app, 10.0, 9.0);
    let mut ship_frames = 0;
    for _ in 0..600 {
        heal(app);
        app.update();
        if app.world().resource::<Ships>().live() >= 2 {
            ship_frames += 1;
            if ship_frames > 150 {
                break;
            }
        }
    }
    put_player(app, SPAWN);
    look(app, 10.0, 9.0);
    // The round sting (the audio isn't in this app) starts the banner; it
    // is at rest over the island 0.6 s in.
    app.world_mut()
        .write_message(pieced::audio::music::WaveStarted { wave: 7 });
    for _ in 0..36 {
        put_player(app, SPAWN);
        look(app, 10.0, 9.0);
        heal(app);
        app.update();
    }
    capture(app, board, "V7-wave-banner");

    // V1: mid-wave, ships hovering, knights running in at 6–10 m.
    let k = knights(app, 3, 1800);
    put_player(app, SPAWN);
    look(app, 4.0, 3.0);
    let spots = [
        me + bearing(-16.0) * 8.5,
        me + bearing(2.0) * 6.5,
        me + bearing(17.0) * 9.0,
    ];
    for (knight, at) in k.iter().zip(spots) {
        release(app, *knight, at, 7);
    }
    frames(app, 12);
    put_player(app, SPAWN);
    look(app, 4.0, 3.0);
    frames(app, 1);
    capture(app, board, "V1-spawn-midwave");

    // V5: a knight about 6 m out, winding up his wand at the player (his own
    // brain fires; he just can't walk), others behind him.
    let k = knights(app, 3, 1800);
    put_player(app, SPAWN);
    let a = me + bearing(-6.0) * 4.6;
    stand(app, k[0], a, 500.0);
    let wave = GruntStats::for_wave(7, &app.world().resource::<pieced::tuning::Tuning>().grunt);
    {
        let mut stats = app.world_mut().get_mut::<GruntStats>(k[0]).unwrap();
        stats.reaction = wave.reaction;
        stats.fire_interval = wave.fire_interval;
    }
    release(app, k[1], me + bearing(14.0) * 15.0, 7);
    release(app, k[2], me + bearing(-1.0) * 18.0, 7);
    look(app, 8.0, 1.0);
    let total = app
        .world()
        .resource::<pieced::tuning::Tuning>()
        .grunt
        .windup;
    let mut wound = false;
    for _ in 0..900 {
        put_player(app, SPAWN);
        look(app, 8.0, 1.0);
        if let Some(mut t) = app.world_mut().get_mut::<Transform>(k[0]) {
            t.translation = a;
        }
        app.update();
        let progress = app
            .world()
            .get::<Wand>(k[0])
            .and_then(|w| w.windup_progress(total));
        // Deep in it (M4 chunk 4): the wind-up clip's planted thrust.
        if progress.is_some_and(|p| p >= 0.85) {
            wound = true;
            break;
        }
    }
    println!("V5: wound up {wound}");
    {
        use pieced::knight::{KnightAnim, KnightClip};
        let world = app.world_mut();
        for (figure, anim) in world
            .query::<(&pieced::arena::visuals::TargetFigure, &KnightAnim)>()
            .iter(world)
        {
            if figure.owner == k[0] {
                let mix = anim.clip_mix();
                println!(
                    "V5: clips {}, wind-up clip weight {:.2} at {:.3} s",
                    anim.clips_enabled(),
                    mix.weight(KnightClip::WindUp),
                    mix.time(KnightClip::WindUp)
                );
            }
        }
    }
    capture_with(app, board, "V5-knight-windup", false);

    // V2: ADS on the rifle, a body hit at about 9 m, the castle to the left.
    let k = knights(app, 3, 1800);
    hush(app);
    put_player(app, SPAWN);
    let a = me + bearing(50.0) * 8.5;
    stand(app, k[0], a, 500.0);
    aim(app, a + Vec3::Y * 1.0);
    hold_aim(app, true);
    frames(app, 40);
    stand(app, k[0], a, 500.0);
    aim(app, a + Vec3::Y * 1.0);
    fire(app);
    frames(app, 2);
    capture(app, board, "V2-rifle-ads");
    hold_aim(app, false);

    // V4: a body kill, then a headshot kill within the chain (DOUBLE!), the
    // second knight caught as his helmet pops, the castle behind.
    let k = knights(app, 3, 1800);
    hush(app);
    put_player(app, SPAWN);
    let a = me + bearing(62.0) * 9.0;
    let b = me + bearing(40.0) * 5.0;
    stand(app, k[0], a, 1.0);
    stand(app, k[1], b, 40.0);
    frames(app, 30);
    stand(app, k[0], a, 1.0);
    stand(app, k[1], b, 40.0);
    aim(app, a + Vec3::Y * 1.0);
    frames(app, 2);
    fire(app);
    frames(app, 40);
    stand(app, k[1], b, 40.0);
    aim(app, b + Vec3::Y * HEAD_CENTER);
    frames(app, 2);
    aim(app, b + Vec3::Y * HEAD_CENTER);
    fire(app);
    frames(app, 18);
    capture(app, board, "V4-headshot-kill");

    // V3: the pump blasts a knight at 3 m (a heavy hit, not a kill: he's
    // flung back flailing), the castle to the right.
    select(app, WeaponKind::Pump);
    let k = knights(app, 1, 1800);
    hush(app);
    put_player(app, SPAWN);
    let a = me + bearing(-10.0) * 2.8;
    stand(app, k[0], a, 500.0);
    frames(app, 20);
    stand(app, k[0], a, 500.0);
    aim(app, a + Vec3::Y * 1.1);
    frames(app, 1);
    fire(app);
    frames(app, 6);
    capture(app, board, "V3-pump-blast");

    // X: a knight at 3 m, still, for close review of the model.
    let k = knights(app, 1, 1800);
    put_player(app, SPAWN);
    let a = me + bearing(20.0) * 3.0;
    stand(app, k[0], a, 500.0);
    frames(app, 40);
    stand(app, k[0], a, 500.0);
    aim(app, a + Vec3::Y * 1.05);
    frames(app, 2);
    capture(app, board, "X-knight-close");

    // V6: boxed up, looking out of a window at knights firing orbs.
    set_hushed(false);
    put_player(app, SPAWN);
    let c = GridCell::new(6, 9, 0);
    let mut front = None;
    for slot in [
        PieceSlot::wall(c, Facing::North),
        PieceSlot::wall(c, Facing::East),
        PieceSlot::wall(c, Facing::West),
        PieceSlot::wall(c, Facing::South),
        PieceSlot::floor(GridCell::new(6, 9, 1)),
    ] {
        match place_piece(app.world_mut(), slot) {
            Ok(e) if slot == PieceSlot::wall(c, Facing::North) => front = Some(e),
            Ok(_) => {}
            Err(why) => println!("V6: {slot:?} rejected: {why:?}"),
        }
    }
    if let Some(front) = front {
        assert!(edit_piece(
            app.world_mut(),
            front,
            PieceEdit::of(&[3, 4, 5])
        ));
    }
    let k = knights(app, 3, 1800);
    for (i, knight) in k.iter().take(3).enumerate() {
        let at = me + bearing(-18.0 + 18.0 * i as f32) * (10.0 + i as f32 * 2.0);
        release(app, *knight, at, 7);
    }
    for _ in 0..150 {
        put_player(app, SPAWN);
        look(app, 0.0, 8.0);
        app.update();
    }
    capture(app, board, "V6-boxup-fight");

    // V8: the main menu's hero shot. (The hats the board's kills left on the
    // lawn go: a player's menu opens on a fresh island.)
    {
        let world = app.world_mut();
        let hats: Vec<Entity> = world
            .query_filtered::<Entity, With<pieced::fx::hat::HatProp>>()
            .iter(world)
            .collect();
        for hat in hats {
            world.entity_mut(hat).insert(Visibility::Hidden);
        }
    }
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Menu);
    frames(app, 120);
    capture(app, board, "V8-main-menu");
}

#[test]
#[ignore = "needs a GPU: run by hand to capture the M4 target board"]
fn capture_the_m4_board_offscreen() {
    let out = std::env::var("PIECED_BOARD_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("pieced-board"));
    std::fs::create_dir_all(&out).unwrap();
    let only: Vec<String> = std::env::var("PIECED_BOARD_ONLY")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let mut app = board_app();
    let mut boot = 0;
    while *app.world().resource::<State<AppState>>().get() == AppState::Boot {
        app.update();
        boot += 1;
        assert!(boot < 900, "Boot never ended");
    }
    println!("Boot ended after {boot} frames");
    let image = composite_ui(&mut app);
    frames(&mut app, 10);
    let mut board = Board {
        out: out.clone(),
        only,
        image,
        written: Vec::new(),
    };
    review(&mut app, &mut board);
    let written = board.written.clone();
    wait_for_files(&mut app, &written);
    assert_pipelines_ok(&app);
    println!("wrote {} PNGs to {}", written.len(), out.display());
}
