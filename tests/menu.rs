//! The main menu (docs/M3-SPEC.md → Main menu and rebinding, D92; M2
//! leftovers item 4) through a headless client: the real simulation, Boot,
//! the input adapter, the menus and the Waves screens, with a window whose
//! cursor the adapter manages. Clicks go in as `Interaction`s, keys through
//! `ButtonInput`; what comes out is the app state, the mode, the arena, the
//! knights and the dummy, and the menus' text and visibility.
//!
//! - Boot ends on the main menu when the game opens there, and in `Playing`
//!   otherwise (`--waves`, `--practice`, scenarios, every older test).
//! - Waves and Practice start from the menu in place, resetting the arena,
//!   the knights, the dummy and the player; Play → controllable is timed.
//! - "Quit to menu" from the pause menu and from the results.
//! - Settings → Controls rebinds through the adapter; hints follow.

use bevy::{
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    time::TimeUpdateStrategy,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use pieced::{
    app::{BootPlugin, BootTarget, GameOptions},
    building::{self, Piece},
    combat::Downed,
    dummy::Dummy,
    grunt::Parked,
    input::{Action, Binding, BindingCapture, InputAdapterPlugin},
    menu::{
        ControlsButton, ControlsStatus, KeyChip, LastPlay, MainMenuButton, MenuPage, MenuState,
        MenuUiPlugin, PauseButton,
    },
    rng::{Rng, SimRng},
    shared::{AppState, GameMode, Health, Player, PlayerIntent, tick_duration},
    sim::Sim,
    tuning::Tuning,
    waves::{
        EndRun, PoolGrunt, RestartRun, Run, RunPhase, RunSeed, RunSummary, WavesClientPlugin,
        modes::{ModeSwitch, StartMode},
        ui::{ResultsButton, WavesUiPlugin},
    },
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The game's client-side flow, headless: built in `mode` (the native game
/// uses Waves, so its pools are warm), booting to `target` if given.
fn client(mode: GameMode, target: Option<AppState>) -> Sim {
    client_with(mode, target, |_| {})
}

/// [`client`], with `extra` applied to the app before it is finished.
fn client_with(mode: GameMode, target: Option<AppState>, extra: impl FnOnce(&mut App)) -> Sim {
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
    .add_plugins((
        InputAdapterPlugin,
        WavesUiPlugin,
        WavesClientPlugin,
        MenuUiPlugin,
        BootPlugin,
        pieced::telemetry::TelemetryPlugin,
    ))
    // The window messages telemetry reads (no `WindowPlugin` headless).
    .add_message::<bevy::window::WindowCreated>()
    .add_message::<bevy::window::WindowOccluded>()
    .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()))
    .insert_resource(SimRng(Rng::new(21)))
    .insert_resource(mode)
    .insert_resource(ButtonInput::<KeyCode>::default())
    .insert_resource(ButtonInput::<MouseButton>::default())
    .insert_resource(AccumulatedMouseMotion::default());
    if let Some(target) = target {
        app.insert_resource(BootTarget(target));
    }
    extra(&mut app);
    app.finish();
    app.cleanup();
    app.world_mut()
        .spawn((Window::default(), PrimaryWindow, CursorOptions::default()));
    let mut sim = Sim { app };
    for _ in 0..3 {
        sim.app.update();
    }
    sim
}

/// The native game without a mode flag: Waves pools, booting to the menu.
fn menu_game() -> Sim {
    let sim = client(GameMode::Waves, Some(AppState::Menu));
    assert_eq!(state(&sim), AppState::Menu);
    sim
}

fn state(sim: &Sim) -> AppState {
    *sim.world().resource::<State<AppState>>().get()
}

fn menu(sim: &Sim) -> MenuState {
    sim.world().resource::<MenuState>().clone()
}

fn updates(sim: &mut Sim, n: usize) {
    for _ in 0..n {
        sim.app.update();
    }
}

/// Clicks the button with component `B == button` (made visible, as the
/// real UI would be) and runs a frame.
fn click<B: Component + PartialEq + Copy>(sim: &mut Sim, button: B) {
    let world = sim.world_mut();
    let mut q = world.query::<(Entity, &B)>();
    let entity = q
        .iter(world)
        .find(|(_, b)| **b == button)
        .map(|(e, _)| e)
        .expect("the button");
    world
        .entity_mut(entity)
        .insert((Interaction::Pressed, InheritedVisibility::VISIBLE));
    sim.app.update();
    sim.world_mut().entity_mut(entity).insert(Interaction::None);
}

fn tap(sim: &mut Sim, key: KeyCode) {
    sim.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key);
    sim.app.update();
    let mut keys = sim.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.release(key);
    keys.clear();
}

fn count<F: bevy::ecs::query::QueryFilter>(sim: &mut Sim) -> usize {
    let world = sim.world_mut();
    world.query_filtered::<(), F>().iter(world).count()
}

fn cursor(sim: &mut Sim) -> CursorOptions {
    let world = sim.world_mut();
    world
        .query_filtered::<&CursorOptions, With<PrimaryWindow>>()
        .single(world)
        .unwrap()
        .clone()
}

fn pieces(sim: &mut Sim) -> usize {
    count::<With<Piece>>(sim)
}

/// A texts containing `needle`.
fn texts_with(sim: &mut Sim, needle: &str) -> Vec<String> {
    let world = sim.world_mut();
    let mut q = world.query::<&Text>();
    q.iter(world)
        .filter(|t| t.0.contains(needle))
        .map(|t| t.0.clone())
        .collect()
}

/// Everything the menu's idle island promises.
fn assert_idle(sim: &mut Sim) {
    assert!(!sim.world().contains_resource::<Run>(), "no run");
    assert!(!sim.world().contains_resource::<RunSummary>());
    assert_eq!(count::<With<Dummy>>(sim), 0, "no dummy");
    let grunts = count::<With<PoolGrunt>>(sim);
    assert_eq!(
        count::<(With<PoolGrunt>, With<Parked>)>(sim),
        grunts,
        "every knight parked"
    );
    assert_eq!(
        pieces(sim),
        building::initial_cover().len(),
        "initial cover"
    );
    let p = sim.player();
    let spawn = sim
        .world()
        .resource::<pieced::arena::ArenaLayout>()
        .player_spawn;
    assert_eq!(sim.feet(p), spawn, "the player at spawn");
    let combat = sim.world().resource::<Tuning>().combat.clone();
    assert_eq!(
        *sim.get::<Health>(p),
        Health::full(combat.max_hp, combat.max_shield),
        "whole"
    );
    assert!(sim.world().get::<Downed>(p).is_none());
    assert!(sim.world().resource::<ModeSwitch>().is_idle());
}

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------

#[test]
fn boot_opens_the_main_menu_over_an_idle_island() {
    let mut sim = menu_game();
    let m = menu(&sim);
    assert!(m.title_visible(), "the title screen");
    assert!(m.menu_visible(), "so the HUD hides");
    assert!(!m.pause_layer_visible(), "no veil over the island");
    assert_idle(&mut sim);
    // The Waves pools were made during Boot and wait under the island.
    let max = sim.world().resource::<Tuning>().waves.max_alive as usize;
    assert_eq!(count::<With<PoolGrunt>>(&mut sim), max);
    // The cursor is free and the controls do nothing.
    assert_eq!(cursor(&mut sim).grab_mode, CursorGrabMode::None);
    assert!(cursor(&mut sim).visible);
    sim.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    updates(&mut sim, 5);
    assert_eq!(*sim.player_intent(), PlayerIntent::default());
    assert_eq!(state(&sim), AppState::Menu);
    // Esc on the title does nothing; the sim doesn't tick.
    let tick = sim.sim_tick();
    tap(&mut sim, KeyCode::Escape);
    updates(&mut sim, 5);
    assert_eq!(state(&sim), AppState::Menu);
    assert_eq!(sim.sim_tick(), tick, "the menu doesn't simulate");
}

#[test]
fn launch_ready_is_the_clickable_menu() {
    use pieced::telemetry::{BootPhases, LaunchTime};
    let mut sim = menu_game();
    assert!(sim.world().resource::<LaunchTime>().0.is_some(), "timed");
    let phases = sim.world().resource::<BootPhases>().clone();
    assert!(phases.menu_ready_ms.is_some(), "menu_ready");
    assert_eq!(phases.controllable_ms, None, "not play: the menu");
    assert!(phases.phases().iter().any(|(p, _)| p == "menu_ready"));
    // Starting a mode doesn't retime the launch.
    let launch = sim.world().resource::<LaunchTime>().0;
    click(&mut sim, MainMenuButton::Practice);
    updates(&mut sim, 3);
    assert_eq!(sim.world().resource::<LaunchTime>().0, launch);
    // A launch straight into play still times "controllable".
    let direct = client(GameMode::Practice, None);
    let phases = direct.world().resource::<BootPhases>().clone();
    assert!(phases.controllable_ms.is_some() && phases.menu_ready_ms.is_none());
}

#[test]
fn flags_and_scenarios_skip_the_menu() {
    let args =
        |a: &[&str]| GameOptions::from_args(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(args(&["pieced"]).boot_target(), AppState::Menu);
    assert_eq!(
        args(&["pieced", "--waves"]).boot_target(),
        AppState::Playing
    );
    assert_eq!(
        args(&["pieced", "--practice"]).boot_target(),
        AppState::Playing
    );
    let scenario = args(&["pieced", "--scenario", "perf"]);
    assert_eq!(scenario.boot_target(), AppState::Playing);
    assert_eq!(scenario.game_mode(), GameMode::Practice);
    // Boot without a target (every M1/M2 test and scenario) plays Practice
    // straight away, dummy and all.
    let mut sim = client(GameMode::Practice, None);
    assert_eq!(state(&sim), AppState::Playing);
    assert_eq!(count::<With<Dummy>>(&mut sim), 1);
    assert!(!menu(&sim).menu_visible());
}

// ---------------------------------------------------------------------------
// Starting a mode from the menu
// ---------------------------------------------------------------------------

#[test]
fn practice_starts_in_place_with_the_dummy() {
    let mut sim = menu_game();
    click(&mut sim, MainMenuButton::Practice);
    updates(&mut sim, 3);
    assert_eq!(state(&sim), AppState::Playing);
    assert_eq!(*sim.world().resource::<GameMode>(), GameMode::Practice);
    assert_eq!(count::<With<Dummy>>(&mut sim), 1, "the dummy");
    assert!(!sim.world().contains_resource::<Run>(), "no waves");
    assert!(!menu(&sim).menu_visible(), "the menus closed");
    assert_eq!(cursor(&mut sim).grab_mode, CursorGrabMode::Locked);
    // Play → controllable, timed (W8: < 1 s).
    let (ms, mode) = sim.world().resource::<LastPlay>().0.expect("timed");
    assert_eq!(mode, GameMode::Practice);
    assert!(ms < 1000.0, "Play → controllable took {ms} ms");
    // The player plays.
    let before = sim.sim_tick();
    updates(&mut sim, 10);
    assert!(sim.sim_tick() > before);
}

#[test]
fn waves_starts_a_fresh_run_in_place() {
    let mut sim = menu_game();
    click(&mut sim, MainMenuButton::Waves);
    updates(&mut sim, 3);
    assert_eq!(state(&sim), AppState::Playing);
    assert_eq!(*sim.world().resource::<GameMode>(), GameMode::Waves);
    let run = sim.world().resource::<Run>().clone();
    assert_eq!((run.wave, run.score, run.phase), (1, 0, RunPhase::Fighting));
    assert_eq!(count::<With<Dummy>>(&mut sim), 0);
    let (_, mode) = sim.world().resource::<LastPlay>().0.expect("timed");
    assert_eq!(mode, GameMode::Waves);
    // The knights come.
    for _ in 0..600 {
        if count::<(With<PoolGrunt>, Without<Parked>)>(&mut sim) > 0 {
            break;
        }
        sim.tick();
    }
    assert!(count::<(With<PoolGrunt>, Without<Parked>)>(&mut sim) > 0);
}

#[test]
fn switching_modes_resets_the_arena_knights_dummy_and_player() {
    let mut sim = menu_game();
    click(&mut sim, MainMenuButton::Waves);
    updates(&mut sim, 3);
    let first_seed = sim.world().resource::<Run>().seed;
    // Play a while: knights land; knock the cover down; get hurt.
    for _ in 0..240 {
        sim.tick();
    }
    building::clear_pieces(sim.world_mut());
    let p = sim.player();
    sim.world_mut().get_mut::<Health>(p).unwrap().hp = 40.0;
    sim.world_mut()
        .get_mut::<Transform>(p)
        .unwrap()
        .translation
        .x += 5.0;
    assert_eq!(pieces(&mut sim), 0);

    // Quit to menu (straight there: the results path is tested below).
    sim.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Menu);
    updates(&mut sim, 2);
    assert_eq!(state(&sim), AppState::Menu);
    assert_idle(&mut sim);

    // Practice: the dummy, no run, the cover back.
    click(&mut sim, MainMenuButton::Practice);
    updates(&mut sim, 3);
    assert_eq!(count::<With<Dummy>>(&mut sim), 1);
    assert!(!sim.world().contains_resource::<Run>());
    assert_eq!(
        count::<(With<PoolGrunt>, Without<Parked>)>(&mut sim),
        0,
        "no knights in Practice"
    );
    sim.ticks(60);

    // Back to the menu, then Waves: the dummy leaves, a new run starts.
    sim.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Menu);
    updates(&mut sim, 2);
    assert_idle(&mut sim);
    click(&mut sim, MainMenuButton::Waves);
    updates(&mut sim, 3);
    assert_eq!(count::<With<Dummy>>(&mut sim), 0);
    let run = sim.world().resource::<Run>().clone();
    assert_eq!((run.wave, run.score), (1, 0));
    assert_ne!(run.seed, first_seed, "a new seed");
    assert_eq!(sim.world().resource::<ModeSwitch>().starts, 3);
}

#[test]
fn a_game_started_in_practice_makes_the_waves_pools_on_demand() {
    // `--practice`, then Quit to menu, then Waves.
    let mut sim = client(GameMode::Practice, None);
    assert_eq!(count::<With<PoolGrunt>>(&mut sim), 0);
    sim.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Menu);
    updates(&mut sim, 2);
    assert_idle(&mut sim);
    sim.world_mut().write_message(StartMode(GameMode::Waves));
    updates(&mut sim, 3);
    let max = sim.world().resource::<Tuning>().waves.max_alive as usize;
    assert_eq!(count::<With<PoolGrunt>>(&mut sim), max);
    assert!(sim.world().contains_resource::<Run>());
    // The first menu run keeps `--seed` semantics: the game's seed stream.
    for _ in 0..600 {
        if count::<(With<PoolGrunt>, Without<Parked>)>(&mut sim) > 0 {
            break;
        }
        sim.tick();
    }
    assert!(count::<(With<PoolGrunt>, Without<Parked>)>(&mut sim) > 0);
}

/// The seeds of: the first menu run, its Go again, then a menu run after
/// quitting to the menu.
fn seed_sequence(run_seed: u64) -> [u64; 3] {
    let mut sim = menu_game();
    sim.world_mut().insert_resource(RunSeed(run_seed));
    click(&mut sim, MainMenuButton::Waves);
    updates(&mut sim, 3);
    let first = sim.world().resource::<Run>().seed;
    sim.world_mut().write_message(RestartRun);
    updates(&mut sim, 2);
    let again = sim.world().resource::<Run>().seed;
    sim.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Menu);
    updates(&mut sim, 2);
    click(&mut sim, MainMenuButton::Waves);
    updates(&mut sim, 3);
    [first, again, sim.world().resource::<Run>().seed]
}

#[test]
fn launches_get_fresh_seeds_and_seed_replays_exactly() {
    let args =
        |a: &[&str]| GameOptions::from_args(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    // Without `--seed`, every launch draws a new first seed (entropy)...
    let seeds: Vec<u64> = (0..4).map(|_| args(&["pieced"]).run_seed()).collect();
    for (i, a) in seeds.iter().enumerate() {
        for b in &seeds[i + 1..] {
            assert_ne!(a, b, "two launches, one seed: {seeds:?}");
        }
    }
    // ...and `--seed N` always replays N.
    assert_eq!(args(&["pieced", "--seed", "42"]).run_seed(), 42);
    assert_eq!(args(&["pieced", "--seed", "42"]).run_seed(), 42);

    // The run seed drives every run: the menu's first run is exactly it, Go
    // again and later menu runs get new seeds, and the same run seed replays
    // the same sequence.
    let replay = seed_sequence(42);
    assert_eq!(replay[0], 42);
    assert!(replay[1] != 42 && replay[2] != 42 && replay[1] != replay[2]);
    assert_eq!(seed_sequence(42), replay, "`--seed` replays exactly");
    let other = seed_sequence(args(&["pieced"]).run_seed());
    assert_ne!(other, replay);
}

// ---------------------------------------------------------------------------
// Quit to menu
// ---------------------------------------------------------------------------

#[test]
fn quit_to_menu_from_the_pause_menu() {
    let mut sim = menu_game();
    click(&mut sim, MainMenuButton::Practice);
    updates(&mut sim, 3);
    tap(&mut sim, KeyCode::Escape);
    updates(&mut sim, 2);
    assert_eq!(state(&sim), AppState::Paused);
    assert!(menu(&sim).pause_layer_visible() && !menu(&sim).title);
    click(&mut sim, PauseButton::Quit);
    updates(&mut sim, 2);
    assert_eq!(state(&sim), AppState::Menu, "Practice quits straight out");
    assert!(menu(&sim).title_visible());
    assert_idle(&mut sim);
}

#[test]
fn quitting_a_run_shows_its_results_then_quit_to_menu() {
    let mut sim = menu_game();
    click(&mut sim, MainMenuButton::Waves);
    updates(&mut sim, 3);
    sim.ticks(30);
    // Pause → Quit to menu mid-run: the run ends and its results show (D84).
    tap(&mut sim, KeyCode::Escape);
    updates(&mut sim, 2);
    assert_eq!(state(&sim), AppState::Paused);
    click(&mut sim, PauseButton::Quit);
    updates(&mut sim, 3);
    assert_eq!(state(&sim), AppState::Playing);
    assert!(sim.world().resource::<Run>().is_over(), "the results");
    // The results card's Quit to menu.
    click(&mut sim, ResultsButton::Quit);
    updates(&mut sim, 2);
    assert_eq!(state(&sim), AppState::Menu);
    assert!(menu(&sim).title_visible());
    assert_idle(&mut sim);
    assert_eq!(cursor(&mut sim).grab_mode, CursorGrabMode::None);
    // And again from the top.
    click(&mut sim, MainMenuButton::Waves);
    updates(&mut sim, 3);
    assert_eq!(state(&sim), AppState::Playing);
    assert_eq!(sim.world().resource::<Run>().phase, RunPhase::Fighting);
}

#[test]
fn a_death_ends_on_the_results_and_quit_goes_to_the_menu() {
    let mut sim = menu_game();
    click(&mut sim, MainMenuButton::Waves);
    updates(&mut sim, 3);
    sim.world_mut().write_message(EndRun);
    updates(&mut sim, 3);
    assert!(sim.world().resource::<Run>().is_over());
    click(&mut sim, ResultsButton::Quit);
    updates(&mut sim, 2);
    assert_eq!(state(&sim), AppState::Menu);
    let player = sim.player();
    assert!(sim.world().get::<Player>(player).is_some());
}

// ---------------------------------------------------------------------------
// Settings and Controls from the menu
// ---------------------------------------------------------------------------

#[test]
fn settings_and_controls_open_over_the_menu_and_esc_steps_back() {
    let mut sim = menu_game();
    click(&mut sim, MainMenuButton::Settings);
    assert_eq!(menu(&sim).page, MenuPage::Settings);
    assert!(menu(&sim).pause_layer_visible() && !menu(&sim).title_visible());
    click(&mut sim, PauseButton::Controls);
    assert_eq!(menu(&sim).page, MenuPage::Controls);
    tap(&mut sim, KeyCode::Escape);
    assert_eq!(menu(&sim).page, MenuPage::Settings);
    tap(&mut sim, KeyCode::Escape);
    assert_eq!(menu(&sim).page, MenuPage::Main);
    assert!(menu(&sim).title_visible());
    assert_eq!(state(&sim), AppState::Menu);
}

#[test]
fn the_controls_page_rebinds_swaps_cancels_and_resets() {
    let mut sim = menu_game();
    click(&mut sim, MainMenuButton::Settings);
    click(&mut sim, PauseButton::Controls);
    let chip = |sim: &mut Sim, action: Action| {
        let world = sim.world_mut();
        let mut q = world.query::<(&KeyChip, &Text)>();
        q.iter(world)
            .find(|(c, _)| c.0 == action)
            .map(|(_, t)| t.0.clone())
            .unwrap()
    };
    let status = |sim: &mut Sim| {
        let world = sim.world_mut();
        let mut q = world.query_filtered::<&Text, With<ControlsStatus>>();
        q.single(world).unwrap().0.clone()
    };
    updates(&mut sim, 1);
    assert_eq!(chip(&mut sim, Action::Jump), "Space");
    assert_eq!(chip(&mut sim, Action::Fire), "Click");

    // Click Jump's chip, press C (crouch's): they swap.
    click(&mut sim, ControlsButton::Bind(Action::Jump));
    assert_eq!(
        sim.world().resource::<BindingCapture>().waiting,
        Some(Action::Jump)
    );
    updates(&mut sim, 1);
    assert_eq!(chip(&mut sim, Action::Jump), "Press a key");
    tap(&mut sim, KeyCode::KeyC);
    updates(&mut sim, 1);
    assert_eq!(chip(&mut sim, Action::Jump), "C");
    assert_eq!(chip(&mut sim, Action::Crouch), "Space");
    assert!(status(&mut sim).contains("Swapped"), "{}", status(&mut sim));
    let b = sim.world().resource::<Tuning>().bindings.clone();
    assert_eq!(b.get(Action::Crouch), Binding::Key(KeyCode::Space));

    // A click binds the mouse button.
    click(&mut sim, ControlsButton::Bind(Action::Edit));
    sim.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    updates(&mut sim, 1);
    sim.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .reset_all();
    updates(&mut sim, 1);
    assert_eq!(chip(&mut sim, Action::Edit), "Right click");

    // Esc cancels the capture and stays on the page.
    click(&mut sim, ControlsButton::Bind(Action::Wall));
    tap(&mut sim, KeyCode::Escape);
    updates(&mut sim, 1);
    assert_eq!(menu(&sim).page, MenuPage::Controls, "Esc only cancelled");
    assert_eq!(chip(&mut sim, Action::Wall), "Q");
    assert!(status(&mut sim).contains("unchanged"));

    // Cmd-Q can't be bound.
    sim.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::SuperLeft);
    updates(&mut sim, 1);
    sim.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    click(&mut sim, ControlsButton::Bind(Action::Ramp));
    tap(&mut sim, KeyCode::KeyK);
    sim.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    updates(&mut sim, 1);
    assert_eq!(chip(&mut sim, Action::Ramp), "E");
    assert!(status(&mut sim).contains("Cmd"), "{}", status(&mut sim));

    // Reset to defaults: D43 again.
    click(&mut sim, ControlsButton::Reset);
    updates(&mut sim, 1);
    assert_eq!(
        sim.world().resource::<Tuning>().bindings,
        pieced::input::Bindings::default()
    );
    assert_eq!(chip(&mut sim, Action::Jump), "Space");
    assert_eq!(chip(&mut sim, Action::Edit), "G");

    // Leaving the page drops a pending capture.
    click(&mut sim, ControlsButton::Bind(Action::Cone));
    click(&mut sim, ControlsButton::Back);
    assert_eq!(menu(&sim).page, MenuPage::Settings);
    updates(&mut sim, 1);
    assert_eq!(sim.world().resource::<BindingCapture>().waiting, None);
}

#[test]
fn the_pause_menu_hints_show_the_bound_keys() {
    let mut sim = menu_game();
    click(&mut sim, MainMenuButton::Practice);
    updates(&mut sim, 3);
    updates(&mut sim, 1);
    assert_eq!(
        texts_with(&mut sim, "(hold) aim"),
        ["W sprint   Shift (hold) aim   C slide   R reload"]
    );
    {
        let mut tuning = sim.tuning_mut();
        tuning
            .bindings
            .bind(Action::Aim, Binding::Key(KeyCode::KeyZ))
            .unwrap();
        tuning
            .bindings
            .bind(Action::Edit, Binding::Key(KeyCode::KeyT))
            .unwrap();
    }
    updates(&mut sim, 1);
    assert_eq!(
        texts_with(&mut sim, "(hold) aim"),
        ["W sprint   Z (hold) aim   C slide   R reload"]
    );
    assert_eq!(
        texts_with(&mut sim, "wall ramp floor cone"),
        ["Q E F V wall ramp floor cone   T edit   R reset an edit"]
    );
}

#[test]
fn the_best_run_shows_under_waves() {
    let mut sim = menu_game();
    updates(&mut sim, 1);
    assert_eq!(texts_with(&mut sim, "NO RUNS YET").len(), 1);
    sim.world_mut()
        .resource_mut::<pieced::waves::PersonalBest>()
        .0 = Some(pieced::waves::BestRun {
        wave: 4,
        score: 2_350,
        seed: 9,
        eliminations: 12,
        run_seconds: 200.0,
    });
    updates(&mut sim, 1);
    assert_eq!(texts_with(&mut sim, "NO RUNS YET").len(), 0);
    assert_eq!(texts_with(&mut sim, "BEST  WAVE 4 \u{b7} 2,350").len(), 1);
}

// ---------------------------------------------------------------------------
// M4 chunk 5: start at wave, menu motion, the pause blur, loading, previews
// ---------------------------------------------------------------------------

#[test]
fn the_waves_button_starts_at_the_chosen_wave() {
    use pieced::{menu::StartWaveChoice, waves::modes::StartWave};
    let mut sim = menu_game();
    assert_eq!(sim.world().resource::<StartWave>().get(), 1);
    click(&mut sim, StartWaveChoice(6));
    assert_eq!(state(&sim), AppState::Menu, "choosing doesn't start");
    assert_eq!(sim.world().resource::<StartWave>().get(), 6);
    click(&mut sim, MainMenuButton::Waves);
    updates(&mut sim, 3);
    let run = sim.world().resource::<Run>().clone();
    assert_eq!((run.wave, run.start_wave), (6, 6));
    assert_eq!(
        sim.world().resource::<RunSummary>().knights_left,
        13,
        "wave 6's 13 knights"
    );
    // Back on the menu, 10.
    sim.world_mut().write_message(EndRun);
    updates(&mut sim, 3);
    click(&mut sim, ResultsButton::Quit);
    updates(&mut sim, 2);
    click(&mut sim, StartWaveChoice(10));
    click(&mut sim, MainMenuButton::Waves);
    updates(&mut sim, 3);
    assert_eq!(sim.world().resource::<Run>().wave, 10);
}

fn button_transform<B: Component + PartialEq + Copy>(sim: &mut Sim, button: B) -> UiTransform {
    let world = sim.world_mut();
    let mut q = world.query::<(&B, &UiTransform)>();
    q.iter(world)
        .find(|(b, _)| **b == button)
        .map(|(_, t)| *t)
        .expect("the button has a transform")
}

#[test]
fn menu_buttons_slide_in_within_0_2_s_and_pop_on_hover() {
    use pieced::menu::motion::{ENTER_SECONDS, HOVER_REST};
    let mut sim = menu_game();
    click(&mut sim, MainMenuButton::Practice);
    updates(&mut sim, 3);
    tap(&mut sim, KeyCode::Escape);
    updates(&mut sim, 1);
    assert_eq!(state(&sim), AppState::Paused);
    // Mid-slide: off to the left and small.
    let quit = button_transform(&mut sim, PauseButton::Quit);
    let x = match quit.translation.x {
        Val::Px(x) => x,
        other => panic!("{other:?}"),
    };
    assert!(x < -10.0, "sliding in: {x}");
    assert!(quit.scale.x < 1.0);
    // All in place within 0.2 s.
    updates(&mut sim, (ENTER_SECONDS * 60.0).ceil() as usize);
    for b in [
        PauseButton::Resume,
        PauseButton::Settings,
        PauseButton::Quit,
    ] {
        assert_eq!(
            button_transform(&mut sim, b),
            UiTransform::IDENTITY,
            "{b:?}"
        );
    }
    // A hover pops it, then it rests a little big; leaving puts it back.
    let resume = {
        let world = sim.world_mut();
        let mut q = world.query::<(Entity, &PauseButton)>();
        q.iter(world)
            .find(|(_, b)| **b == PauseButton::Resume)
            .map(|(e, _)| e)
            .unwrap()
    };
    sim.world_mut()
        .entity_mut(resume)
        .insert(Interaction::Hovered);
    updates(&mut sim, 4);
    assert!(button_transform(&mut sim, PauseButton::Resume).scale.x > HOVER_REST);
    updates(&mut sim, 20);
    assert_eq!(
        button_transform(&mut sim, PauseButton::Resume).scale,
        Vec2::splat(HOVER_REST)
    );
    sim.world_mut().entity_mut(resume).insert(Interaction::None);
    updates(&mut sim, 1);
    assert_eq!(
        button_transform(&mut sim, PauseButton::Resume),
        UiTransform::IDENTITY
    );
}

#[test]
fn pausing_blurs_the_last_frame_once_and_stops_the_3d_passes() {
    use pieced::{
        menu::blur::{BLUR_FRAMES, BlurCamera, PauseBlur},
        render::{MainCamera, PassSize, WorldTarget, full_screen_passes},
        viewmodel::ViewmodelCamera,
    };
    let mut sim = client_with(GameMode::Waves, Some(AppState::Menu), |app| {
        app.init_asset::<Image>();
    });
    // The world's cameras and image, as the renderer makes them.
    let image = sim
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::default());
    sim.world_mut().insert_resource(WorldTarget {
        image,
        size: UVec2::new(1470, 956),
    });
    sim.world_mut()
        .spawn((MainCamera, Camera3d::default(), Transform::default()));
    sim.world_mut()
        .spawn((ViewmodelCamera, Camera3d::default(), Transform::default()));
    updates(&mut sim, 2);
    fn active_3d(sim: &mut Sim) -> usize {
        let world = sim.world_mut();
        let mut q = world.query_filtered::<&Camera, With<Camera3d>>();
        q.iter(world).filter(|c| c.is_active).count()
    }
    fn blur_active(sim: &mut Sim) -> bool {
        let world = sim.world_mut();
        let mut q = world.query_filtered::<&Camera, With<BlurCamera>>();
        q.iter(world).any(|c| c.is_active)
    }
    fn render_passes(sim: &mut Sim) -> usize {
        // The 3D cameras' passes (the blur camera is asleep when this runs).
        full_screen_passes(sim.world_mut())
            .into_iter()
            .filter(|p| {
                matches!(
                    p.name,
                    "copy_to_output" | "tonemapping" | "fxaa" | "sharpening"
                )
            })
            .count()
    }
    assert_eq!(state(&sim), AppState::Menu);
    assert!(!blur_active(&mut sim), "the blur camera sleeps after Boot");
    click(&mut sim, MainMenuButton::Waves);
    updates(&mut sim, 3);
    assert_eq!(active_3d(&mut sim), 2);
    assert!(render_passes(&mut sim) > 0, "playing draws the world");

    // Pause: the last frame is blurred once, then the 3D cameras stop.
    tap(&mut sim, KeyCode::Escape);
    updates(&mut sim, 1);
    assert_eq!(state(&sim), AppState::Paused);
    assert_eq!(sim.world().resource::<PauseBlur>().blurs, 1);
    let mut blur_frames = u32::from(blur_active(&mut sim));
    for _ in 0..5 {
        updates(&mut sim, 1);
        blur_frames += u32::from(blur_active(&mut sim));
    }
    assert!((1..=BLUR_FRAMES).contains(&blur_frames), "{blur_frames}");
    assert_eq!(active_3d(&mut sim), 0, "the world stops rendering");
    assert_eq!(render_passes(&mut sim), 0, "no 3D passes while paused");
    assert_eq!(sim.world().resource::<PauseBlur>().blurs, 1, "blurred once");

    // Settings preview live: the world draws again behind the card.
    click(&mut sim, PauseButton::Settings);
    updates(&mut sim, 1);
    assert_eq!(active_3d(&mut sim), 2);
    click(&mut sim, PauseButton::Back);
    updates(&mut sim, 3);
    assert_eq!(active_3d(&mut sim), 0);
    assert_eq!(sim.world().resource::<PauseBlur>().blurs, 2);

    // Resume: everything back.
    tap(&mut sim, KeyCode::Escape);
    updates(&mut sim, 2);
    assert_eq!(state(&sim), AppState::Playing);
    assert_eq!(active_3d(&mut sim), 2);
    assert!(!blur_active(&mut sim));
}

#[test]
fn the_loading_screen_fills_and_its_tips_name_the_bound_keys() {
    use pieced::{
        app::BootGate,
        menu::loading::{LoadingProgress, LoadingUi},
    };
    let mut sim = client_with(GameMode::Waves, Some(AppState::Menu), |app| {
        let mut gate = app.world_mut().resource_mut::<BootGate>();
        gate.hold("test-a");
        gate.hold("test-b");
        app.world_mut()
            .resource_mut::<Tuning>()
            .bindings
            .bind(Action::Aim, Binding::Key(KeyCode::KeyZ))
            .unwrap();
    });
    assert_eq!(state(&sim), AppState::Boot);
    fn tip(sim: &mut Sim) -> Option<String> {
        let world = sim.world_mut();
        let mut q = world.query::<(&LoadingUi, &Text)>();
        q.iter(world)
            .find(|(p, _)| **p == LoadingUi::Tip)
            .map(|(_, t)| t.0.clone())
    }
    assert_eq!(tip(&mut sim).as_deref(), Some("Hold Z to aim down sights"));
    let before = sim.world().resource::<LoadingProgress>().fraction;
    sim.world_mut().resource_mut::<BootGate>().release("test-a");
    updates(&mut sim, 2);
    let after = sim.world().resource::<LoadingProgress>().fraction;
    assert!(after > before + 0.2, "{before} -> {after}");
    // The tips rotate.
    updates(&mut sim, (2.7 * 60.0) as usize);
    assert_eq!(
        tip(&mut sim).as_deref(),
        Some("Pump knights off the edge for +150")
    );
    sim.world_mut().resource_mut::<BootGate>().release("test-b");
    updates(&mut sim, 3);
    assert_eq!(state(&sim), AppState::Menu);
    assert_eq!(tip(&mut sim), None, "gone with the loading screen");
}

#[test]
fn settings_preview_audio_and_the_camera_nudge_live() {
    use pieced::menu::{Setting, preview::SettingsPreview};
    let mut sim = menu_game();
    click(&mut sim, MainMenuButton::Settings);
    updates(&mut sim, 1);
    fn set(sim: &mut Sim, s: Setting, v: f32) {
        s.set(&mut sim.world_mut().resource_mut::<Tuning>(), v);
        sim.app.update();
    }
    set(&mut sim, Setting::CameraEffects, 0.5);
    assert_eq!(sim.world().resource::<SettingsPreview>().nudges, 1);
    set(&mut sim, Setting::Effects, 0.4);
    assert_eq!(sim.world().resource::<SettingsPreview>().samples, 1);
    // Dragging on: at most one sample per quarter second.
    set(&mut sim, Setting::Effects, 0.45);
    assert_eq!(sim.world().resource::<SettingsPreview>().samples, 1);
    updates(&mut sim, 16);
    set(&mut sim, Setting::Volume, 0.7);
    assert_eq!(sim.world().resource::<SettingsPreview>().samples, 2);
    // The slider reads 0–100%.
    assert_eq!(
        Setting::CameraEffects.display(sim.world().resource::<Tuning>()),
        "50%"
    );
}
