//! The Waves screens (docs/M3-SPEC.md → HUD and results UI, D65; Waves,
//! D79–D84) through the simulation seam, headless: the real simulation plus
//! the input adapter, the run HUD, the results card and the death beat. Keys
//! go in through `ButtonInput`, button clicks as `Interaction`s; what comes
//! out is the UI's text and visibility, the run's phase and `Time<Virtual>`.

use bevy::{
    ecs::message::Messages,
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    time::TimeUpdateStrategy,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use pieced::{
    audio::{
        Sfx,
        music::{MusicDirector, MusicInput, RunView, Screen, WaveStarted},
        run_beat,
    },
    combat::Downed,
    grunt::Parked,
    hud::{HudPlugin, anchors},
    input::{InputAdapterPlugin, start_key_name},
    render::CurrentFov,
    rng::{Rng, SimRng},
    shared::{
        AppState, DamageDealt, DamageTarget, GalleryFreeze, GameCue, GameMode, Health,
        tick_duration,
    },
    sim::Sim,
    waves::{
        EndRun, PoolGrunt, RestartRun, Run, RunEnd, RunPhase, RunSummary, WavesClientPlugin,
        ui::{
            PauseQuit, ResultsButton, RunUi, WavesUiPlugin,
            banner::{BANNER_SECONDS, BANNER_TOTAL, WaveBanner, covers_crosshair},
            death::{DeathBeat, DeathCam, PULL_SECONDS},
            hud::STRIP_TOP,
            pause_quit,
            results::{BURST_DELAY, COUNT_SECONDS},
        },
    },
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The adaptive score's director, as the game runs it (`audio::music`): it
/// sends [`WaveStarted`] with the round sting, which the wave banner follows.
/// (The audio plugin itself isn't in these apps.)
fn music_director(
    time: Res<Time<Real>>,
    tuning: Res<pieced::tuning::Tuning>,
    state: Res<State<AppState>>,
    summary: Option<Res<RunSummary>>,
    run: Option<Res<Run>>,
    mut director: Local<Option<MusicDirector>>,
    mut started: MessageWriter<WaveStarted>,
) {
    let input = MusicInput {
        screen: Screen::from_state(*state.get()),
        run: summary.map(|s| RunView {
            seed: s.seed,
            phase: s.phase,
            wave: s.wave,
            alive: run.map_or(0, |r| r.alive),
            new_best: s.new_best,
        }),
    };
    let frame = director.get_or_insert_with(MusicDirector::default).step(
        time.delta_secs(),
        &input,
        &tuning.music,
        [0.0; 3],
        false,
    );
    if let Some(wave) = frame.wave_started {
        started.write(WaveStarted { wave });
    }
}

/// A Waves game with the input adapter, the run HUD, the results card and
/// the death beat, over a headless simulation (one fixed tick per update
/// while time runs at 1×).
fn game(seed: u64) -> Sim {
    // `app::headless_app` plus the UI plugins (added before `finish`).
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        bevy::state::app::StatesPlugin,
        AssetPlugin::default(),
        bevy::mesh::MeshPlugin,
        bevy::scene::ScenePlugin,
        avian3d::prelude::PhysicsPlugins::default(),
    ))
    .add_plugins(pieced::app::SimPlugins)
    .add_plugins((InputAdapterPlugin, WavesUiPlugin, WavesClientPlugin))
    .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()))
    .insert_resource(SimRng(Rng::new(seed)))
    .insert_resource(GameMode::Waves)
    .insert_resource(ButtonInput::<KeyCode>::default())
    .insert_resource(ButtonInput::<MouseButton>::default())
    .insert_resource(AccumulatedMouseMotion::default())
    .add_systems(Update, music_director);
    app.finish();
    app.cleanup();
    // A window whose cursor the adapter locks and frees, and the camera the
    // death cam moves.
    app.world_mut()
        .spawn((Window::default(), PrimaryWindow, CursorOptions::default()));
    app.world_mut().spawn((
        pieced::render::MainCamera,
        Transform::from_xyz(0.0, 1.6, 0.0),
    ));
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Playing);
    app.update();
    Sim { app }
}

/// [`game`] with still knights and a player who can't die.
fn frozen(seed: u64) -> Sim {
    let mut sim = game(seed);
    sim.world_mut().insert_resource(GalleryFreeze);
    let p = sim.player();
    let mut health = Health::full(1.0e9, 100.0);
    health.shield = 100.0;
    *sim.world_mut().get_mut::<Health>(p).unwrap() = health;
    sim
}

fn run(sim: &Sim) -> Run {
    sim.world().resource::<Run>().clone()
}

fn summary(sim: &Sim) -> RunSummary {
    sim.world().resource::<RunSummary>().clone()
}

fn text(sim: &mut Sim, part: RunUi) -> String {
    let world = sim.world_mut();
    let mut q = world.query::<(&RunUi, &Text)>();
    let found: Vec<String> = q
        .iter(world)
        .filter(|(p, _)| **p == part)
        .map(|(_, t)| t.0.clone())
        .collect();
    assert_eq!(found.len(), 1, "one {part:?} text");
    found[0].clone()
}

fn shown(sim: &mut Sim, part: RunUi) -> bool {
    let world = sim.world_mut();
    let mut q = world.query::<(&RunUi, &Visibility)>();
    let found: Vec<Visibility> = q
        .iter(world)
        .filter(|(p, _)| **p == part)
        .map(|(_, v)| *v)
        .collect();
    assert_eq!(found.len(), 1, "one {part:?} node");
    found[0] != Visibility::Hidden
}

fn press(sim: &mut Sim, key: KeyCode) {
    sim.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key);
    sim.app.update();
    let mut keys = sim.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.release(key);
    keys.clear();
}

fn click(sim: &mut Sim, button: ResultsButton) {
    let world = sim.world_mut();
    let mut q = world.query::<(Entity, &ResultsButton)>();
    let entity = q
        .iter(world)
        .find(|(_, b)| **b == button)
        .map(|(e, _)| e)
        .expect("the button");
    world.entity_mut(entity).insert(Interaction::Pressed);
    sim.app.update();
    sim.world_mut().entity_mut(entity).insert(Interaction::None);
}

fn in_play(sim: &mut Sim) -> Vec<Entity> {
    let mut q = sim
        .world_mut()
        .query_filtered::<Entity, (With<PoolGrunt>, Without<Parked>, Without<Downed>)>();
    q.iter(sim.world()).collect()
}

/// Downs `grunt` the way combat does (a killing hit from the player).
fn kill(sim: &mut Sim, grunt: Entity) {
    let player = sim.player();
    let tick = sim.sim_tick();
    let at = sim.feet(grunt);
    let hp = sim.get::<Health>(grunt).hp;
    sim.world_mut().get_mut::<Health>(grunt).unwrap().hp = 0.0;
    sim.world_mut().entity_mut(grunt).insert(Downed { tick });
    sim.world_mut().write_message(DamageDealt {
        source: Some(player),
        target: grunt,
        target_kind: DamageTarget::Character,
        amount: hp,
        to_shield: 0.0,
        headshot: false,
        shield_broke: false,
        killed: true,
        point: at + Vec3::Y,
        normal: Vec3::Z,
        tick,
    });
}

/// Downs the wave's knights as they arrive until the break.
fn clear_wave(sim: &mut Sim) {
    for _ in 0..20_000 {
        for g in in_play(sim) {
            kill(sim, g);
        }
        sim.tick();
        if matches!(run(sim).phase, RunPhase::Break { .. }) {
            return;
        }
    }
    panic!("the wave never cleared");
}

fn eliminate(sim: &mut Sim) {
    let player = sim.player();
    sim.world_mut()
        .get_mut::<Health>(player)
        .unwrap()
        .apply(1.0e12);
    sim.tick();
    assert!(run(sim).is_ended());
}

/// Updates until the results are up; returns how many updates that took.
fn until_over(sim: &mut Sim) -> u32 {
    for n in 1..2_000 {
        sim.app.update();
        if run(sim).is_over() {
            return n;
        }
    }
    panic!("the results never came");
}

/// Updates until the results card is up (after the death cam's hold);
/// returns how many updates that took.
fn until_results(sim: &mut Sim) -> u32 {
    for n in 1..3_000 {
        sim.app.update();
        if shown(sim, RunUi::Results) {
            return n;
        }
    }
    panic!("the results card never came");
}

fn banner(sim: &Sim) -> WaveBanner {
    *sim.world().resource::<WaveBanner>()
}

/// The banner's face text ("WAVE 7") and its ribbon ("17 KNIGHTS INCOMING").
fn banner_text(sim: &mut Sim) -> (String, String) {
    (
        text(sim, RunUi::WaveBannerFace),
        text(sim, RunUi::WaveBannerKnights),
    )
}

fn speed(sim: &Sim) -> f32 {
    sim.world().resource::<Time<Virtual>>().relative_speed()
}

fn cursor(sim: &mut Sim) -> CursorOptions {
    let world = sim.world_mut();
    let mut q = world.query_filtered::<&CursorOptions, With<PrimaryWindow>>();
    q.single(world).unwrap().clone()
}

// ---------------------------------------------------------------------------
// The run HUD and the break
// ---------------------------------------------------------------------------

#[test]
fn the_hud_follows_the_run_through_a_wave_clear() {
    let mut sim = frozen(3);
    sim.tick();
    assert!(shown(&mut sim, RunUi::Hud));
    assert_eq!(text(&mut sim, RunUi::Wave), "1");
    assert_eq!(text(&mut sim, RunUi::Knights), "3", "3 + 0 knights to come");
    assert_eq!(text(&mut sim, RunUi::Score), "0");
    // The new wave's banner shows as the run starts (the break's banner
    // doesn't).
    assert!(shown(&mut sim, RunUi::WaveBanner));
    assert_eq!(
        banner_text(&mut sim),
        ("WAVE 1".into(), "3 KNIGHTS INCOMING".into())
    );
    assert!(!shown(&mut sim, RunUi::Banner));
    assert!(!shown(&mut sim, RunUi::Countdown));

    // One knight down: the knights left and the score follow, and it pops.
    for _ in 0..600 {
        if !in_play(&mut sim).is_empty() {
            break;
        }
        sim.tick();
    }
    let first = in_play(&mut sim)[0];
    kill(&mut sim, first);
    sim.tick();
    assert_eq!(text(&mut sim, RunUi::Knights), "2");
    assert_eq!(text(&mut sim, RunUi::Score), "100");
    let pop = {
        let world = sim.world_mut();
        let mut q = world.query::<(&RunUi, &UiTransform)>();
        q.iter(world)
            .find(|(p, _)| **p == RunUi::ScoreFrame)
            .map(|(_, t)| t.scale)
            .unwrap()
    };
    assert_ne!(pop, Vec2::ONE, "the score pops when it ticks up");

    // The wave cleared: 3 × 100 + 250 × 1.
    clear_wave(&mut sim);
    sim.tick();
    let s = summary(&sim);
    assert_eq!(text(&mut sim, RunUi::Score), format!("{}", s.score));
    assert_eq!(s.score, 550);
    assert_eq!(text(&mut sim, RunUi::Knights), "0");
    assert_eq!(text(&mut sim, RunUi::Wave), "1");
    assert!(shown(&mut sim, RunUi::Banner));
    assert_eq!(text(&mut sim, RunUi::BannerTitle), "WAVE 1 CLEARED!");
    assert!(shown(&mut sim, RunUi::Countdown) && shown(&mut sim, RunUi::Hint));
    assert_eq!(text(&mut sim, RunUi::CountdownValue), "10");
    assert_eq!(text(&mut sim, RunUi::HintKey), start_key_name());
    assert_eq!(start_key_name(), "Enter");
    // Three seconds later it reads 7.
    sim.run_seconds(3.0);
    assert_eq!(text(&mut sim, RunUi::CountdownValue), "7");
    assert!(!shown(&mut sim, RunUi::Results));
}

#[test]
fn a_big_score_prints_with_separators() {
    let mut sim = frozen(4);
    sim.world_mut().resource_mut::<Run>().score = 12_450;
    sim.tick();
    assert_eq!(text(&mut sim, RunUi::Score), "12,450");
}

#[test]
fn enter_skips_the_break_and_the_next_wave_is_announced() {
    let mut sim = frozen(5);
    // Enter mid-fight does nothing.
    press(&mut sim, KeyCode::Enter);
    assert_eq!(run(&sim).phase, RunPhase::Fighting);
    assert_eq!(run(&sim).wave, 1);
    clear_wave(&mut sim);
    press(&mut sim, KeyCode::Enter);
    let r = run(&sim);
    assert_eq!(r.phase, RunPhase::Fighting, "Enter skipped the break");
    assert_eq!(r.wave, 2);
    sim.tick();
    assert_eq!(text(&mut sim, RunUi::Wave), "2");
    assert_eq!(text(&mut sim, RunUi::Knights), "5");
    assert!(shown(&mut sim, RunUi::WaveBanner));
    assert_eq!(
        banner_text(&mut sim),
        ("WAVE 2".into(), "5 KNIGHTS INCOMING".into())
    );
    assert!(!shown(&mut sim, RunUi::Banner));
    assert!(!shown(&mut sim, RunUi::Countdown));
    // The banner goes into the counter after a moment.
    sim.run_seconds(2.0);
    assert!(!shown(&mut sim, RunUi::WaveBanner));
    // The numpad's Enter skips too.
    clear_wave(&mut sim);
    press(&mut sim, KeyCode::NumpadEnter);
    assert_eq!(run(&sim).wave, 3);
}

#[test]
fn a_drunk_potion_pops_a_cyan_plus_25() {
    let mut sim = frozen(6);
    assert!(!shown(&mut sim, RunUi::PotionNumber));
    let player = sim.player();
    sim.world_mut().write_message(GameCue::PotionPicked {
        who: player,
        potion: player,
        at: Vec3::ZERO,
    });
    sim.app.update();
    assert!(shown(&mut sim, RunUi::PotionNumber));
    assert_eq!(text(&mut sim, RunUi::PotionNumber), "+25");
    sim.run_seconds(1.2);
    assert!(!shown(&mut sim, RunUi::PotionNumber), "it fades away");
}

// ---------------------------------------------------------------------------
// The death beat
// ---------------------------------------------------------------------------

#[test]
fn the_death_beat_runs_at_0_3_for_a_real_second_then_exactly_1() {
    let mut sim = game(7);
    sim.ticks(30);
    assert_eq!(speed(&sim), 1.0);
    eliminate(&mut sim);
    sim.app.update();
    assert_eq!(speed(&sim), 0.3, "slow motion during the beat");
    assert!(matches!(run(&sim).phase, RunPhase::Dying { .. }));
    // The view is dropping and the vignette closing in.
    assert!(sim.world().resource::<DeathBeat>().progress() > 0.0);
    assert!(shown(&mut sim, RunUi::Vignette));

    // Pausing runs the menus at full speed; resuming slows it again.
    sim.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Paused);
    sim.app.update();
    sim.app.update();
    assert_eq!(speed(&sim), 1.0, "paused");
    sim.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Playing);
    sim.app.update();
    sim.app.update();
    assert_eq!(speed(&sim), 0.3, "back in the beat");

    // About a second of real time (60 updates at 1/60 s) to the run's end.
    let updates = until_over(&mut sim);
    assert!(
        (50..=66).contains(&updates),
        "the beat took {updates} real frames"
    );
    sim.app.update();
    assert_eq!(speed(&sim), 1.0, "exactly 1.0 after the beat");
    // The death cam holds while the knights hop, then the results.
    assert!(
        !shown(&mut sim, RunUi::Results),
        "the death cam holds first"
    );
    let hold = until_results(&mut sim);
    assert!(
        (30..=70).contains(&hold),
        "the results came {hold} frames after the beat"
    );
    let beat = *sim.world().resource::<DeathBeat>();
    assert!(
        (beat.progress() - 1.0).abs() < 1e-4,
        "the vignette closed in"
    );

    // Go again: time and the view are back to normal.
    press(&mut sim, KeyCode::Enter);
    sim.app.update();
    assert_eq!(speed(&sim), 1.0);
    assert_eq!(run(&sim).phase, RunPhase::Fighting);
    assert_eq!(sim.world().resource::<DeathBeat>().progress(), 0.0);
    assert!(!shown(&mut sim, RunUi::Vignette));
}

#[test]
fn a_restart_during_the_beat_restores_time_at_once() {
    let mut sim = game(8);
    sim.ticks(10);
    eliminate(&mut sim);
    sim.app.update();
    assert_eq!(speed(&sim), 0.3);
    // Enter can't skip the beat...
    press(&mut sim, KeyCode::Enter);
    assert!(matches!(run(&sim).phase, RunPhase::Dying { .. }));
    // ...but a restart (from anywhere) puts time back to exactly 1.0.
    sim.world_mut().write_message(RestartRun);
    for _ in 0..8 {
        sim.app.update();
    }
    assert_eq!(run(&sim).phase, RunPhase::Fighting);
    assert_eq!(speed(&sim), 1.0);
}

// ---------------------------------------------------------------------------
// The results
// ---------------------------------------------------------------------------

#[test]
fn the_results_card_shows_every_field_and_new_best_only_when_earned() {
    let mut sim = frozen(9);
    clear_wave(&mut sim);
    press(&mut sim, KeyCode::Enter);
    // Down one knight of wave 2, then die.
    for _ in 0..600 {
        if !in_play(&mut sim).is_empty() {
            break;
        }
        sim.tick();
    }
    let g = in_play(&mut sim)[0];
    kill(&mut sim, g);
    sim.tick();
    let p = sim.player();
    *sim.world_mut().get_mut::<Health>(p).unwrap() = Health::full(100.0, 0.0);
    eliminate(&mut sim);
    assert!(!shown(&mut sim, RunUi::Results), "not during the beat");
    until_results(&mut sim);
    // The numbers count up from 0, then "NEW BEST!" bursts.
    assert_eq!(text(&mut sim, RunUi::ResultsScore), "0");
    assert!(!shown(&mut sim, RunUi::NewBest), "not before the count");
    sim.run_seconds(COUNT_SECONDS * 0.4);
    let mid: u32 = text(&mut sim, RunUi::ResultsScore)
        .replace(',', "")
        .parse()
        .unwrap();
    assert!((1..650).contains(&mid), "counting: {mid}");
    assert!(!shown(&mut sim, RunUi::NewBest));
    sim.run_seconds(BURST_DELAY);

    let s = summary(&sim);
    assert!(s.new_best, "the first run is a best");
    assert!(shown(&mut sim, RunUi::Results));
    assert!(!shown(&mut sim, RunUi::StartedAt), "a ranked run");
    assert!(
        !shown(&mut sim, RunUi::Hud),
        "the strip gives way to the card"
    );
    assert!(shown(&mut sim, RunUi::NewBest));
    assert_eq!(text(&mut sim, RunUi::ResultsWave), "WAVE 2");
    assert_eq!(s.results.score, 3 * 100 + 250 + 100);
    assert_eq!(text(&mut sim, RunUi::ResultsScore), "650");
    assert_eq!(text(&mut sim, RunUi::ResultsEliminations), "4");
    assert_eq!(text(&mut sim, RunUi::ResultsAccuracy), "0%");
    assert_eq!(text(&mut sim, RunUi::ResultsHeadshots), "0");
    let secs = s.results.run_seconds.floor() as u32;
    assert_eq!(
        text(&mut sim, RunUi::ResultsTime),
        format!("{}:{:02}", secs / 60, secs % 60)
    );
    assert_eq!(text(&mut sim, RunUi::BestWave), "2");
    assert_eq!(text(&mut sim, RunUi::BestScore), "650");
    assert_eq!(text(&mut sim, RunUi::BestEliminations), "4");
    assert_eq!(text(&mut sim, RunUi::Seed), format!("Seed {}", s.seed));
    assert!(text(&mut sim, RunUi::ResultsHint).contains("Enter"));
    // The cursor is free for the buttons.
    let c = cursor(&mut sim);
    assert_eq!(c.grab_mode, CursorGrabMode::None);
    assert!(c.visible);

    // Go again: a fresh run, the HUD back at wave 1 and 0 points, the cursor
    // locked again.
    click(&mut sim, ResultsButton::GoAgain);
    sim.tick();
    let r = run(&sim);
    assert_eq!((r.wave, r.score, r.phase), (1, 0, RunPhase::Fighting));
    assert_ne!(r.seed, s.seed, "a new seed");
    assert!(!shown(&mut sim, RunUi::Results));
    assert!(shown(&mut sim, RunUi::Hud));
    assert_eq!(text(&mut sim, RunUi::Wave), "1");
    assert_eq!(text(&mut sim, RunUi::Score), "0");
    sim.tick();
    assert_eq!(cursor(&mut sim).grab_mode, CursorGrabMode::Locked);

    // A worse run: no NEW BEST, the best run still beside it.
    let p = sim.player();
    *sim.world_mut().get_mut::<Health>(p).unwrap() = Health::full(100.0, 0.0);
    eliminate(&mut sim);
    until_results(&mut sim);
    sim.run_seconds(BURST_DELAY + 0.2);
    assert!(!summary(&sim).new_best);
    assert!(shown(&mut sim, RunUi::Results));
    assert!(!shown(&mut sim, RunUi::NewBest));
    assert_eq!(text(&mut sim, RunUi::ResultsWave), "WAVE 1");
    assert_eq!(text(&mut sim, RunUi::ResultsScore), "0");
    assert_eq!(text(&mut sim, RunUi::BestWave), "2");
    assert_eq!(text(&mut sim, RunUi::BestScore), "650");

    // Quit to menu on the card goes to the main menu (chunk 5), not out of
    // the game.
    sim.world_mut().resource_mut::<Messages<AppExit>>().clear();
    click(&mut sim, ResultsButton::Quit);
    sim.app.update();
    assert_eq!(
        *sim.world().resource::<State<AppState>>().get(),
        AppState::Menu,
        "Quit to menu"
    );
    assert_eq!(sim.world().resource::<Messages<AppExit>>().len(), 0);
    assert!(
        !sim.world().contains_resource::<Run>(),
        "the run is put away"
    );
}

#[test]
fn enter_on_the_results_goes_again() {
    let mut sim = game(10);
    sim.ticks(5);
    eliminate(&mut sim);
    until_results(&mut sim);
    let seed = run(&sim).seed;
    press(&mut sim, KeyCode::Enter);
    sim.tick();
    let r = run(&sim);
    assert_eq!((r.wave, r.score, r.phase), (1, 0, RunPhase::Fighting));
    assert_ne!(r.seed, seed);
    assert!(!shown(&mut sim, RunUi::Results));
}

#[test]
fn pause_quit_mid_run_shows_the_results_then_quits() {
    let mut sim = frozen(11);
    clear_wave(&mut sim);
    assert_eq!(pause_quit(Some(&run(&sim))), PauseQuit::EndRun);
    // What the pause menu's Quit does mid-run: end the run and resume.
    sim.world_mut().write_message(EndRun);
    sim.tick();
    sim.app.update();
    let r = run(&sim);
    assert!(r.is_over(), "straight to the results");
    assert_eq!(r.ended, Some(RunEnd::Quit));
    assert!(shown(&mut sim, RunUi::Results), "no death cam after a quit");
    sim.run_seconds(COUNT_SECONDS + 0.1);
    assert_eq!(text(&mut sim, RunUi::ResultsWave), "WAVE 1");
    assert_eq!(text(&mut sim, RunUi::ResultsScore), "550");
    // A quit isn't a death: no slow motion, no drop, no vignette.
    assert_eq!(speed(&sim), 1.0);
    assert_eq!(sim.world().resource::<DeathBeat>().progress(), 0.0);
    assert!(!shown(&mut sim, RunUi::Vignette));
    // A second quit (from the pause menu over the results) goes to the main
    // menu (chunk 5).
    assert_eq!(pause_quit(Some(&run(&sim))), PauseQuit::ToMenu);
}

// ---------------------------------------------------------------------------
// Layout and sound beats
// ---------------------------------------------------------------------------

#[test]
fn the_run_hud_leaves_the_milestone_anchors_where_they_were() {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        bevy::state::app::StatesPlugin,
        AssetPlugin::default(),
        bevy::mesh::MeshPlugin,
        bevy::scene::ScenePlugin,
        avian3d::prelude::PhysicsPlugins::default(),
        bevy::input::InputPlugin,
    ))
    .add_plugins(pieced::app::SimPlugins)
    .add_plugins((HudPlugin, WavesUiPlugin))
    .insert_resource(GameMode::Waves)
    .init_resource::<CurrentFov>()
    .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()));
    app.finish();
    app.cleanup();
    app.update();
    let world = app.world_mut();
    let mut q = world.query::<(&Name, &Node)>();
    let mut node = |name: &str| {
        q.iter(world)
            .find(|(n, _)| n.as_str() == name)
            .map(|(_, n)| n.clone())
            .unwrap_or_else(|| panic!("no node {name:?}"))
    };
    let status = node("Status");
    assert_eq!(
        (status.left, status.bottom),
        (px(anchors::STATUS.0), px(anchors::STATUS.1))
    );
    let ammo = node("Ammo");
    assert_eq!(
        (ammo.right, ammo.bottom),
        (px(anchors::AMMO.0), px(anchors::AMMO.1))
    );
    let hotbar = node("Hotbar");
    assert_eq!(hotbar.bottom, px(anchors::HOTBAR_BOTTOM));
    let readout = node("Combat readout");
    assert_eq!(
        (readout.right, readout.top),
        (px(anchors::READOUT.0), px(anchors::READOUT.1))
    );
    let perf = node("Performance overlay");
    assert_eq!(
        (perf.left, perf.top),
        (px(anchors::PERF.0), px(anchors::PERF.1))
    );
    // The run strip sits top centre, between the performance overlay and the
    // readout, and the banner well below it.
    let strip = node("Run HUD");
    assert_eq!(strip.top, px(STRIP_TOP));
    assert_eq!(strip.justify_content, JustifyContent::Center);
    assert_eq!(strip.position_type, PositionType::Absolute);
}

#[test]
fn the_run_beats_sound_on_clear_next_wave_and_new_best() {
    let base = RunSummary {
        wave: 3,
        ..default()
    };
    let with = |phase, new_best| RunSummary {
        phase,
        new_best,
        ..base.clone()
    };
    let brk = RunPhase::Break { ends_tick: 600 };
    let over = RunPhase::Over { tick: 9 };
    let dying = RunPhase::Dying { until: 9 };
    assert_eq!(
        run_beat(RunPhase::Fighting, 3, &with(brk, false)),
        Some(Sfx::WaveCleared)
    );
    let mut next = with(RunPhase::Fighting, false);
    next.wave = 4;
    assert_eq!(run_beat(brk, 3, &next), Some(Sfx::WaveStart));
    assert_eq!(run_beat(dying, 3, &with(over, true)), Some(Sfx::NewBest));
    assert_eq!(run_beat(dying, 3, &with(over, false)), None);
    assert_eq!(
        run_beat(RunPhase::Fighting, 3, &with(RunPhase::Fighting, false)),
        None
    );
    // "Go again" (results → wave 1) is silent.
    let mut again = with(RunPhase::Fighting, false);
    again.wave = 1;
    assert_eq!(run_beat(over, 3, &again), None);
    for sfx in [
        Sfx::PotionGulp,
        Sfx::WaveCleared,
        Sfx::WaveStart,
        Sfx::NewBest,
    ] {
        assert!(Sfx::ALL.contains(&sfx));
    }
}

// ---------------------------------------------------------------------------
// M4 chunk 5: the wave banner, the death cam, start at wave on the results
// ---------------------------------------------------------------------------

#[test]
fn the_wave_banner_starts_with_the_round_sting_and_lands_in_the_counter() {
    let mut sim = frozen(21);
    sim.record::<WaveStarted>();
    // The run's first wave: the sting and the banner on the same frame.
    let b = banner(&sim);
    assert_eq!(b.showing.map(|(w, _)| w), Some(1));
    assert!(b.showing.unwrap().1 < 1e-6, "started this very frame");
    assert_eq!(b.knights, 3);
    // Up over the island for its 1.5 s...
    sim.run_seconds(BANNER_SECONDS - 0.05);
    assert!(banner(&sim).visible());
    assert!(shown(&mut sim, RunUi::WaveBanner));
    // ...then it shrinks into the counter, which pops as it lands.
    sim.run_seconds(BANNER_TOTAL - BANNER_SECONDS + 0.1);
    assert!(!shown(&mut sim, RunUi::WaveBanner));
    assert!(banner(&sim).landed.is_some());
    let pop = {
        let world = sim.world_mut();
        let mut q = world.query::<(&RunUi, &UiTransform)>();
        q.iter(world)
            .find(|(p, _)| **p == RunUi::WaveFrame)
            .map(|(_, t)| t.scale)
            .unwrap()
    };
    assert_ne!(pop, Vec2::ONE, "the counter pops");

    // The next wave: every WaveStarted starts a banner that same frame, with
    // the wave's real size.
    clear_wave(&mut sim);
    sim.clear_recorded::<WaveStarted>();
    let shown_before = banner(&sim).shown;
    press(&mut sim, KeyCode::Enter);
    for _ in 0..3 {
        let starts = sim.recorded::<WaveStarted>().len();
        let b = banner(&sim);
        if starts > 0 {
            assert_eq!(b.shown, shown_before + 1);
            assert_eq!(b.showing.map(|(w, _)| w), Some(2));
            break;
        }
        assert_eq!(b.shown, shown_before, "no banner without the sting");
        sim.app.update();
    }
    assert_eq!(sim.recorded::<WaveStarted>(), [WaveStarted { wave: 2 }]);
    assert_eq!(banner(&sim).knights, 5);
}

#[test]
fn the_wave_banner_never_covers_the_crosshair() {
    // Every window the Air might show, logical px.
    for screen in [
        Vec2::new(1280.0, 800.0),
        Vec2::new(1470.0, 956.0),
        Vec2::new(1710.0, 1107.0),
        Vec2::new(1440.0, 900.0),
    ] {
        let covered = (0..=(BANNER_TOTAL * 1000.0) as u32)
            .filter(|ms| covers_crosshair(*ms as f32 / 1000.0, screen))
            .count() as f32
            / 1000.0;
        assert!(covered <= 0.3, "{screen}: covered for {covered} s");
        assert_eq!(covered, 0.0, "{screen}: it stays in the top third");
    }
}

#[test]
fn the_death_cam_pulls_out_and_frames_the_knight_that_got_you() {
    let mut sim = frozen(22);
    for _ in 0..900 {
        if !in_play(&mut sim).is_empty() {
            break;
        }
        sim.tick();
    }
    let knight = in_play(&mut sim)[0];
    let player = sim.player();
    let look_before = *sim.get::<pieced::shared::LookAngles>(player);
    // His orb kills the player.
    *sim.world_mut().get_mut::<Health>(player).unwrap() = Health::full(10.0, 0.0);
    sim.world_mut()
        .get_mut::<Health>(player)
        .unwrap()
        .apply(1.0e6);
    let tick = sim.sim_tick();
    let point = sim.feet(player) + Vec3::Y;
    sim.world_mut().write_message(DamageDealt {
        source: Some(knight),
        target: player,
        target_kind: DamageTarget::Character,
        amount: 10.0,
        to_shield: 0.0,
        headshot: false,
        shield_broke: false,
        killed: true,
        point,
        normal: Vec3::Z,
        tick,
    });
    sim.tick();
    assert!(run(&sim).is_ended());
    let eye = sim.feet(player) + Vec3::Y * 1.6;
    sim.app.update();
    let cam = *sim.world().resource::<DeathCam>();
    assert_eq!(cam.killer, Some(knight), "the orb's source");
    // Pulled out to third person over 0.6 s of real time...
    for _ in 0..((PULL_SECONDS * 60.0) as u32 + 2) {
        sim.app.update();
    }
    let camera = {
        let world = sim.world_mut();
        let mut q = world.query_filtered::<&Transform, With<pieced::render::MainCamera>>();
        *q.single(world).unwrap()
    };
    assert!(
        camera.translation.distance(eye) > 3.0,
        "pulled out: {}",
        camera.translation
    );
    // ...facing the knight who did it.
    let at = sim.feet(knight) + Vec3::Y * 1.1;
    let to_knight = (at - camera.translation).normalize();
    assert!(
        camera.forward().dot(to_knight) > 0.995,
        "frames the killer ({})",
        camera.forward().dot(to_knight)
    );
    // The player's aim never moved.
    assert_eq!(*sim.get::<pieced::shared::LookAngles>(player), look_before);
    // Go again puts the camera back in the eye.
    until_results(&mut sim);
    press(&mut sim, KeyCode::Enter);
    sim.app.update();
    assert_eq!(sim.world().resource::<DeathCam>().pose, None);
}

#[test]
fn a_start_at_wave_run_is_marked_on_the_results() {
    let mut sim = frozen(23);
    // To the main menu, pick wave 10, start Waves.
    sim.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Menu);
    sim.app.update();
    sim.app.update();
    sim.world_mut()
        .insert_resource(pieced::waves::modes::StartWave::new(10));
    sim.world_mut()
        .write_message(pieced::waves::modes::StartMode(GameMode::Waves));
    sim.app.update();
    sim.app.update();
    assert_eq!(run(&sim).start_wave, 10);
    assert_eq!(
        banner_text(&mut sim),
        ("WAVE 10".into(), "21 KNIGHTS INCOMING".into())
    );
    sim.world_mut().write_message(EndRun);
    sim.tick();
    until_results(&mut sim);
    sim.run_seconds(BURST_DELAY + 0.1);
    assert!(shown(&mut sim, RunUi::StartedAt));
    assert_eq!(
        text(&mut sim, RunUi::StartedAt),
        "STARTED AT WAVE 10 \u{b7} NOT RANKED"
    );
    assert!(!shown(&mut sim, RunUi::NewBest), "never a best");
    assert!(!summary(&sim).new_best);
}
