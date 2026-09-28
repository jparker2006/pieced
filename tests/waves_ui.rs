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
    audio::{Sfx, run_beat},
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
            PauseQuit, ResultsButton, RunUi, WavesUiPlugin, death::DeathBeat, hud::STRIP_TOP,
            pause_quit,
        },
    },
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
    .insert_resource(AccumulatedMouseMotion::default());
    app.finish();
    app.cleanup();
    // A window whose cursor the adapter locks and frees.
    app.world_mut()
        .spawn((Window::default(), PrimaryWindow, CursorOptions::default()));
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
    // The new wave's title shows as the run starts.
    assert!(shown(&mut sim, RunUi::Banner));
    assert_eq!(text(&mut sim, RunUi::BannerTitle), "WAVE 1");
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
    assert!(shown(&mut sim, RunUi::Banner));
    assert_eq!(text(&mut sim, RunUi::BannerTitle), "WAVE 2");
    assert!(!shown(&mut sim, RunUi::Countdown));
    // The title goes after a moment.
    sim.run_seconds(2.0);
    assert!(!shown(&mut sim, RunUi::Banner));
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

    // About a second of real time (60 updates at 1/60 s) to the results.
    let updates = until_over(&mut sim);
    assert!(
        (50..=66).contains(&updates),
        "the beat took {updates} real frames"
    );
    sim.app.update();
    assert_eq!(speed(&sim), 1.0, "exactly 1.0 after the beat");
    assert!(shown(&mut sim, RunUi::Results));
    let beat = *sim.world().resource::<DeathBeat>();
    assert!((beat.progress() - 1.0).abs() < 1e-4, "on the grass");

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
    until_over(&mut sim);
    sim.app.update();

    let s = summary(&sim);
    assert!(s.new_best, "the first run is a best");
    assert!(shown(&mut sim, RunUi::Results));
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
    until_over(&mut sim);
    sim.app.update();
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
    assert!(!sim.world().contains_resource::<Run>(), "the run is put away");
}

#[test]
fn enter_on_the_results_goes_again() {
    let mut sim = game(10);
    sim.ticks(5);
    eliminate(&mut sim);
    until_over(&mut sim);
    sim.app.update();
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
    assert!(shown(&mut sim, RunUi::Results));
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
