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
        EndRun, PoolGrunt, Run, RunPhase, RunSummary, WavesClientPlugin,
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
    ))
    .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()))
    .insert_resource(SimRng(Rng::new(21)))
    .insert_resource(mode)
    .insert_resource(ButtonInput::<KeyCode>::default())
    .insert_resource(ButtonInput::<MouseButton>::default())
    .insert_resource(AccumulatedMouseMotion::default());
    if let Some(target) = target {
        app.insert_resource(BootTarget(target));
    }
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
    assert_eq!(pieces(sim), building::initial_cover().len(), "initial cover");
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
fn flags_and_scenarios_skip_the_menu() {
    let args = |a: &[&str]| {
        GameOptions::from_args(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    };
    assert_eq!(args(&["pieced"]).boot_target(), AppState::Menu);
    assert_eq!(args(&["pieced", "--waves"]).boot_target(), AppState::Playing);
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
    sim.world_mut().get_mut::<Transform>(p).unwrap().translation.x += 5.0;
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
