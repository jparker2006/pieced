//! Key rebinding (docs/M3-SPEC.md → Main menu and rebinding, D93; D43's
//! default layout) through the input adapter, headless:
//!
//! - every `PlayerIntent` action rebinds, to a key or a trackpad button;
//! - a binding another action uses swaps the two;
//! - "Reset to defaults" restores D43 exactly;
//! - Cmd, Cmd combinations, Esc and the dev keys can't be bound;
//! - bindings persist through `settings.json` and `load_or_default`;
//! - the Controls page's capture goes through the adapter (Esc cancels
//!   without resuming; a Cmd combo is refused);
//! - the on-screen start hints and the start key follow the binding.

use bevy::{
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    time::TimeUpdateStrategy,
    window::{CursorOptions, PrimaryWindow},
};
use pieced::{
    input::{
        Action, BindError, Binding, BindingCapture, Bindings, CaptureOutcome, Captured,
        EditContext, InputAdapterPlugin, apply_capture, bound_buttons_to_intent, captured_input,
    },
    rng::{Rng, SimRng},
    shared::{ActiveTool, AppState, GameMode, PieceKind, PlayerIntent, WeaponKind, tick_duration},
    sim::Sim,
    tuning::Tuning,
    waves::{
        Run, RunPhase,
        ui::{RunUi, WavesUiPlugin},
    },
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// One frame of input through `bindings`: `held` keys and buttons pressed
/// this frame (so they're both held and just pressed).
fn intent_for(bindings: &Bindings, held: &[Binding]) -> PlayerIntent {
    let mut keys = ButtonInput::<KeyCode>::default();
    let mut mouse = ButtonInput::<MouseButton>::default();
    for b in held {
        match *b {
            Binding::Key(k) => keys.press(k),
            Binding::Mouse(m) => mouse.press(m),
        }
    }
    let mut intent = PlayerIntent::default();
    bound_buttons_to_intent(&keys, &mouse, bindings, EditContext::default(), &mut intent);
    intent
}

/// Whether `intent` shows `action` (the start key isn't an intent).
fn does(action: Action, intent: &PlayerIntent) -> bool {
    let tool = |t: ActiveTool| intent.select == Some(t);
    match action {
        Action::MoveForward => intent.move_axis.y > 0.0 && intent.sprint,
        Action::MoveBack => intent.move_axis.y < 0.0,
        Action::MoveLeft => intent.move_axis.x < 0.0,
        Action::MoveRight => intent.move_axis.x > 0.0,
        Action::Jump => intent.jump && intent.jump_pressed,
        Action::Crouch => intent.crouch && intent.crouch_pressed,
        Action::Fire => intent.fire && intent.fire_pressed,
        Action::Aim => intent.ads_held,
        Action::Reload => intent.reload_pressed,
        Action::Rifle => tool(ActiveTool::Weapon(WeaponKind::Rifle)),
        Action::Pump => tool(ActiveTool::Weapon(WeaponKind::Pump)),
        Action::Wall => tool(ActiveTool::Build(PieceKind::Wall)),
        Action::Ramp => tool(ActiveTool::Build(PieceKind::Ramp)),
        Action::Floor => tool(ActiveTool::Build(PieceKind::Floor)),
        Action::Cone => tool(ActiveTool::Build(PieceKind::Cone)),
        Action::Edit => intent.edit_pressed,
        Action::Start => false,
    }
}

fn start_pressed(bindings: &Bindings, held: Binding) -> bool {
    let mut keys = ButtonInput::<KeyCode>::default();
    let mut mouse = ButtonInput::<MouseButton>::default();
    match held {
        Binding::Key(k) => keys.press(k),
        Binding::Mouse(m) => mouse.press(m),
    }
    bindings.just_pressed(Action::Start, &keys, &mouse)
}

/// Keys no default binding uses.
const SPARE: [KeyCode; 17] = [
    KeyCode::KeyK,
    KeyCode::KeyL,
    KeyCode::KeyM,
    KeyCode::KeyN,
    KeyCode::KeyO,
    KeyCode::KeyP,
    KeyCode::KeyU,
    KeyCode::KeyI,
    KeyCode::KeyJ,
    KeyCode::KeyH,
    KeyCode::KeyT,
    KeyCode::KeyY,
    KeyCode::KeyZ,
    KeyCode::KeyX,
    KeyCode::KeyB,
    KeyCode::Digit5,
    KeyCode::Tab,
];

/// D43's layout, spelled out.
const D43: [(Action, Binding); 17] = [
    (Action::MoveForward, Binding::Key(KeyCode::KeyW)),
    (Action::MoveBack, Binding::Key(KeyCode::KeyS)),
    (Action::MoveLeft, Binding::Key(KeyCode::KeyA)),
    (Action::MoveRight, Binding::Key(KeyCode::KeyD)),
    (Action::Jump, Binding::Key(KeyCode::Space)),
    (Action::Crouch, Binding::Key(KeyCode::KeyC)),
    (Action::Fire, Binding::Mouse(MouseButton::Left)),
    (Action::Aim, Binding::Key(KeyCode::ShiftLeft)),
    (Action::Reload, Binding::Key(KeyCode::KeyR)),
    (Action::Rifle, Binding::Key(KeyCode::Digit1)),
    (Action::Pump, Binding::Key(KeyCode::Digit2)),
    (Action::Wall, Binding::Key(KeyCode::KeyQ)),
    (Action::Ramp, Binding::Key(KeyCode::KeyE)),
    (Action::Floor, Binding::Key(KeyCode::KeyF)),
    (Action::Cone, Binding::Key(KeyCode::KeyV)),
    (Action::Edit, Binding::Key(KeyCode::KeyG)),
    (Action::Start, Binding::Key(KeyCode::Enter)),
];

/// The headless simulation plus the real input adapter and the Waves UI,
/// with a window whose cursor the adapter manages.
fn adapter_game(mode: GameMode) -> Sim {
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
    .add_plugins((InputAdapterPlugin, WavesUiPlugin))
    .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()))
    .insert_resource(SimRng(Rng::new(5)))
    .insert_resource(mode)
    .insert_resource(ButtonInput::<KeyCode>::default())
    .insert_resource(ButtonInput::<MouseButton>::default())
    .insert_resource(AccumulatedMouseMotion::default());
    app.finish();
    app.cleanup();
    app.world_mut()
        .spawn((Window::default(), PrimaryWindow, CursorOptions::default()));
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Playing);
    app.update();
    Sim { app }
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

fn ui_text(sim: &mut Sim, part: RunUi) -> String {
    let world = sim.world_mut();
    let mut q = world.query::<(&RunUi, &Text)>();
    q.iter(world)
        .find(|(p, _)| **p == part)
        .map(|(_, t)| t.0.clone())
        .expect("the text")
}

fn state(sim: &Sim) -> AppState {
    *sim.world().resource::<State<AppState>>().get()
}

// ---------------------------------------------------------------------------
// Rebinding
// ---------------------------------------------------------------------------

#[test]
fn the_default_layout_is_d43() {
    let b = Bindings::default();
    for (action, binding) in D43 {
        assert_eq!(b.get(action), binding, "{action:?}");
        let intent = intent_for(&b, &[binding]);
        assert!(
            does(action, &intent) || action == Action::Start,
            "{action:?} on its D43 key"
        );
    }
    assert!(start_pressed(&b, Binding::Key(KeyCode::Enter)));
    assert!(start_pressed(&b, Binding::Key(KeyCode::NumpadEnter)));
    // Either Shift aims (D40).
    assert!(intent_for(&b, &[Binding::Key(KeyCode::ShiftRight)]).ads_held);
    assert_eq!(b.name(Action::Fire), "Click");
    assert_eq!(b.name(Action::Start), "Enter");
}

#[test]
fn every_action_rebinds_to_a_new_key_and_leaves_the_old_one() {
    for (i, (action, old)) in D43.into_iter().enumerate() {
        let mut b = Bindings::default();
        let new = Binding::Key(SPARE[i]);
        assert_eq!(b.bind(action, new), Ok(None), "{action:?}");
        assert_eq!(b.get(action), new);
        if action == Action::Start {
            assert!(start_pressed(&b, new));
            assert!(!start_pressed(&b, old), "Enter no longer starts");
            continue;
        }
        assert!(
            does(action, &intent_for(&b, &[new])),
            "{action:?} on {new:?}"
        );
        assert!(
            !does(action, &intent_for(&b, &[old])),
            "{action:?} left {old:?}"
        );
    }
}

#[test]
fn actions_bind_to_trackpad_buttons_too() {
    let mut b = Bindings::default();
    // Jump on the secondary click, fire on a key.
    b.bind(Action::Jump, Binding::Mouse(MouseButton::Right))
        .unwrap();
    b.bind(Action::Fire, Binding::Key(KeyCode::KeyK)).unwrap();
    assert!(does(
        Action::Jump,
        &intent_for(&b, &[Binding::Mouse(MouseButton::Right)])
    ));
    assert!(does(
        Action::Fire,
        &intent_for(&b, &[Binding::Key(KeyCode::KeyK)])
    ));
    let click = intent_for(&b, &[Binding::Mouse(MouseButton::Left)]);
    assert_eq!(click, PlayerIntent::default(), "the click is free now");
    assert_eq!(b.name(Action::Jump), "Right click");
}

#[test]
fn a_conflict_swaps_the_two_bindings() {
    let mut b = Bindings::default();
    // Jump onto crouch's C: crouch takes Space.
    assert_eq!(
        b.bind(Action::Jump, Binding::Key(KeyCode::KeyC)),
        Ok(Some(Action::Crouch))
    );
    assert_eq!(b.get(Action::Jump), Binding::Key(KeyCode::KeyC));
    assert_eq!(b.get(Action::Crouch), Binding::Key(KeyCode::Space));
    let c = intent_for(&b, &[Binding::Key(KeyCode::KeyC)]);
    assert!(does(Action::Jump, &c) && !c.crouch);
    // Fire onto Q (the wall): the wall takes the click.
    assert_eq!(
        b.bind(Action::Fire, Binding::Key(KeyCode::KeyQ)),
        Ok(Some(Action::Wall))
    );
    assert_eq!(b.get(Action::Wall), Binding::Mouse(MouseButton::Left));
    // A right-hand twin conflicts with its left key: aim onto Enter's twin.
    assert_eq!(
        b.bind(Action::Aim, Binding::Key(KeyCode::NumpadEnter)),
        Ok(Some(Action::Start))
    );
    assert_eq!(b.get(Action::Start), Binding::Key(KeyCode::ShiftLeft));
    // Every binding is still used exactly once.
    for a in Action::ALL {
        assert_eq!(b.action_for(b.get(a)), Some(a));
    }
    // Rebinding to the same key is a no-op.
    assert_eq!(b.bind(Action::Jump, Binding::Key(KeyCode::KeyC)), Ok(None));
}

#[test]
fn reset_restores_d43_exactly() {
    let mut b = Bindings::default();
    for (i, a) in Action::ALL.into_iter().enumerate() {
        b.bind(a, Binding::Key(SPARE[(i + 3) % SPARE.len()]))
            .unwrap();
    }
    assert_ne!(b, Bindings::default());
    b.reset();
    assert_eq!(b, Bindings::default());
    for (action, binding) in D43 {
        assert_eq!(b.get(action), binding);
    }
}

#[test]
fn cmd_esc_and_the_dev_keys_are_refused() {
    let mut b = Bindings::default();
    for key in [KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::Meta] {
        assert_eq!(b.bind(Action::Jump, Binding::Key(key)), Err(BindError::Cmd));
    }
    assert!(matches!(
        b.bind(Action::Jump, Binding::Key(KeyCode::Escape)),
        Err(BindError::Reserved(_))
    ));
    for key in [KeyCode::F3, KeyCode::F4] {
        assert!(matches!(
            b.bind(Action::Edit, Binding::Key(key)),
            Err(BindError::Reserved(_))
        ));
    }
    assert_eq!(
        b.bind(Action::Jump, Binding::Key(KeyCode::CapsLock)),
        Err(BindError::Unsupported)
    );
    assert_eq!(b, Bindings::default(), "a refusal changes nothing");

    // The capture: Cmd held with K is a Cmd combo, refused; Cmd alone too.
    let mut keys = ButtonInput::<KeyCode>::default();
    let mouse = ButtonInput::<MouseButton>::default();
    keys.press(KeyCode::SuperLeft);
    keys.clear();
    keys.press(KeyCode::KeyK);
    let captured = captured_input(&keys, &mouse).unwrap();
    assert_eq!(captured, Captured::CmdCombo(Binding::Key(KeyCode::KeyK)));
    assert_eq!(
        apply_capture(&mut b, Action::Jump, captured),
        CaptureOutcome::Refused(Action::Jump, BindError::Cmd)
    );
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::SuperRight);
    let captured = captured_input(&keys, &mouse).unwrap();
    assert!(matches!(captured, Captured::CmdCombo(_)));
    // Cmd-click too.
    let mut clicks = ButtonInput::<MouseButton>::default();
    clicks.press(MouseButton::Left);
    let mut held = ButtonInput::<KeyCode>::default();
    held.press(KeyCode::SuperLeft);
    held.clear();
    assert!(matches!(
        captured_input(&held, &clicks),
        Some(Captured::CmdCombo(Binding::Mouse(MouseButton::Left)))
    ));
    // Esc cancels.
    let mut esc = ButtonInput::<KeyCode>::default();
    esc.press(KeyCode::Escape);
    assert_eq!(captured_input(&esc, &mouse), Some(Captured::Cancel));
    assert_eq!(b, Bindings::default());
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

#[test]
fn bindings_persist_through_settings_json_and_load_or_default() {
    let dir = std::env::temp_dir().join(format!("pieced-bindings-{}", std::process::id()));
    let path = dir.join("settings.json");
    let mut t = Tuning::default();
    t.bindings
        .bind(Action::Jump, Binding::Key(KeyCode::KeyK))
        .unwrap();
    t.bindings
        .bind(Action::Fire, Binding::Mouse(MouseButton::Right))
        .unwrap();
    t.bindings
        .bind(Action::Aim, Binding::Key(KeyCode::ShiftRight))
        .unwrap();
    t.save(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"bindings\""), "a bindings section");
    assert!(text.contains("\"jump\": \"KeyK\""), "{text}");
    assert!(text.contains("\"fire\": \"MouseRight\""));
    let loaded = Tuning::load_or_default(&path);
    assert_eq!(loaded.bindings, t.bindings, "load_or_default keeps them");
    assert_eq!(loaded, t);

    // A partial or hand-edited section: known entries apply, the rest (and
    // anything unbindable) fall back to D43.
    std::fs::write(
        &path,
        r#"{"bindings":{"jump":"KeyK","crouch":"Escape","edit":"Nonsense","warp":"KeyL"}}"#,
    )
    .unwrap();
    let partial = Tuning::load_or_default(&path);
    assert_eq!(
        partial.bindings.get(Action::Jump),
        Binding::Key(KeyCode::KeyK)
    );
    assert_eq!(
        partial.bindings.get(Action::Crouch),
        Binding::Key(KeyCode::KeyC)
    );
    assert_eq!(
        partial.bindings.get(Action::Edit),
        Binding::Key(KeyCode::KeyG)
    );
    // An older file without the section loads D43.
    std::fs::write(&path, r#"{"look":{"fov_deg":80.0}}"#).unwrap();
    assert_eq!(Tuning::load_or_default(&path).bindings, Bindings::default());
    let _ = std::fs::remove_dir_all(dir);
}

// ---------------------------------------------------------------------------
// Through the adapter
// ---------------------------------------------------------------------------

#[test]
fn the_adapter_captures_a_key_for_the_controls_page() {
    let mut sim = adapter_game(GameMode::Practice);
    sim.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Paused);
    sim.app.update();
    assert_eq!(state(&sim), AppState::Paused);

    // Bind jump to K.
    sim.world_mut().resource_mut::<BindingCapture>().waiting = Some(Action::Jump);
    tap(&mut sim, KeyCode::KeyK);
    let capture = sim.world().resource::<BindingCapture>().clone();
    assert_eq!(capture.waiting, None);
    assert_eq!(
        capture.last,
        Some(CaptureOutcome::Bound {
            action: Action::Jump,
            binding: Binding::Key(KeyCode::KeyK),
            swapped: None,
        })
    );
    let bindings = sim.world().resource::<Tuning>().bindings.clone();
    assert_eq!(bindings.get(Action::Jump), Binding::Key(KeyCode::KeyK));

    // Esc cancels the capture and doesn't also resume play.
    sim.world_mut().resource_mut::<BindingCapture>().waiting = Some(Action::Crouch);
    tap(&mut sim, KeyCode::Escape);
    sim.app.update();
    assert_eq!(state(&sim), AppState::Paused, "Esc only cancelled");
    assert_eq!(
        sim.world().resource::<BindingCapture>().last,
        Some(CaptureOutcome::Cancelled(Action::Crouch))
    );
    assert_eq!(sim.world().resource::<Tuning>().bindings, bindings);

    // Cmd-L (Cmd already held) is refused; the binding stays.
    sim.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::SuperLeft);
    sim.app.update();
    sim.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    sim.world_mut().resource_mut::<BindingCapture>().waiting = Some(Action::Crouch);
    tap(&mut sim, KeyCode::KeyL);
    sim.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    let last = sim.world().resource::<BindingCapture>().last;
    assert!(
        matches!(
            last,
            Some(CaptureOutcome::Refused(Action::Crouch, BindError::Cmd))
        ),
        "{last:?}"
    );
    assert_eq!(sim.world().resource::<Tuning>().bindings, bindings);

    // With no capture waiting, Esc resumes as always.
    tap(&mut sim, KeyCode::Escape);
    sim.app.update();
    assert_eq!(state(&sim), AppState::Playing);
}

#[test]
fn the_game_follows_the_bindings() {
    let mut sim = adapter_game(GameMode::Practice);
    sim.tuning_mut()
        .bindings
        .bind(Action::Jump, Binding::Key(KeyCode::KeyK))
        .unwrap();
    // The adapter reads the rebound key into the player's intent.
    sim.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyK);
    sim.app.update();
    assert!(sim.player_intent().jump, "K jumps");
    sim.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    sim.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    sim.app.update();
    assert!(!sim.player_intent().jump, "Space no longer jumps");
    sim.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
}

#[test]
fn the_start_key_and_its_hints_follow_the_binding() {
    let mut sim = adapter_game(GameMode::Waves);
    sim.tuning_mut()
        .bindings
        .bind(Action::Start, Binding::Key(KeyCode::KeyK))
        .unwrap();
    // A break (as if wave 1 were cleared).
    let now = sim.sim_tick();
    {
        let mut run = sim.world_mut().resource_mut::<Run>();
        run.remaining = 0;
        run.phase = RunPhase::Break {
            ends_tick: now + 600,
        };
    }
    sim.tick();
    sim.tick();
    assert_eq!(ui_text(&mut sim, RunUi::HintKey), "K", "Press K to start");
    // Enter doesn't skip it any more; K does.
    tap(&mut sim, KeyCode::Enter);
    sim.tick();
    assert!(matches!(
        sim.world().resource::<Run>().phase,
        RunPhase::Break { .. }
    ));
    tap(&mut sim, KeyCode::KeyK);
    sim.tick();
    let run = sim.world().resource::<Run>();
    assert_eq!((run.wave, run.phase), (2, RunPhase::Fighting));
    // The results' hint names it too.
    sim.world_mut().write_message(pieced::waves::EndRun);
    sim.tick();
    sim.tick();
    assert_eq!(ui_text(&mut sim, RunUi::ResultsHint), "K  go again");
}
