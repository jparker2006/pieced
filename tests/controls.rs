//! Controls (docs/SPEC.md → Controls, decisions D40–D42) end to end: real key
//! and trackpad-button states go through the keyboard adapter's mapping
//! (`input::buttons_to_intent`) into the player's `PlayerIntent`, then through
//! the headless simulation, one fixed 60 Hz tick per frame.
//!
//! - D40: holding Shift aims down sights; releasing stops. Never a toggle.
//! - D41: holding W sprints (no sprint key); aiming stops the sprint; C while
//!   sprinting slides. Strafing and backpedalling run, like Fortnite.
//! - D42: the secondary (right) click does nothing.

use bevy::prelude::*;
use pieced::{
    input::buttons_to_intent,
    movement::{Motor, MovementTuning},
    shared::{
        ActiveTool, Ads, Facing, GameCue, PieceChange, PieceChanged, PieceKind, PlayerIntent,
        ShotFired, TICK_SECONDS, WeaponKind,
    },
    sim::Sim,
    tuning::Tuning,
};

const RIFLE: ActiveTool = ActiveTool::Weapon(WeaponKind::Rifle);

/// A headless game played through the keyboard and trackpad buttons.
struct Pad {
    sim: Sim,
    player: Entity,
    keys: ButtonInput<KeyCode>,
    mouse: ButtonInput<MouseButton>,
}

impl Pad {
    fn new() -> Self {
        let mut sim = Sim::new();
        sim.tuning_mut().dummy.stand_still = true;
        sim.record::<GameCue>();
        sim.record::<ShotFired>();
        sim.record::<PieceChanged>();
        sim.ticks(5);
        let player = sim.player();
        let mut pad = Self {
            sim,
            player,
            keys: ButtonInput::default(),
            mouse: ButtonInput::default(),
        };
        // An open lane the movement tests also use.
        pad.place(Vec3::new(-20.0, 0.0, 20.0), Facing::North);
        pad
    }

    /// Puts the player somewhere with nothing held and lets it settle.
    fn place(&mut self, feet: Vec3, facing: Facing) {
        self.keys.reset_all();
        self.mouse.reset_all();
        self.sim
            .world_mut()
            .get_mut::<Transform>(self.player)
            .unwrap()
            .translation = feet;
        self.sim.set_look(self.player, facing.yaw(), 0.0);
        *self.sim.intent(self.player) = PlayerIntent::default();
        self.frames(20);
    }

    fn press(&mut self, key: KeyCode) {
        self.keys.press(key);
    }

    fn release(&mut self, key: KeyCode) {
        self.keys.release(key);
    }

    /// One rendered frame: the adapter maps the buttons, then one fixed tick.
    fn frame(&mut self) {
        let mut intent = self.sim.intent(self.player);
        buttons_to_intent(&self.keys, &self.mouse, &mut intent);
        self.sim.tick();
        self.keys.clear();
        self.mouse.clear();
    }

    fn frames(&mut self, n: u32) {
        for _ in 0..n {
            self.frame();
        }
    }

    /// Presses `key` and plays one frame, where its press edge lands. The key
    /// stays held until released.
    fn tap(&mut self, key: KeyCode) {
        self.press(key);
        self.frame();
    }

    fn movement(&self) -> MovementTuning {
        self.sim.world().resource::<Tuning>().movement.clone()
    }

    fn ads(&self) -> bool {
        self.sim.get::<Ads>(self.player).0
    }

    fn motor(&self) -> &Motor {
        self.sim.get::<Motor>(self.player)
    }

    fn tool(&self) -> ActiveTool {
        *self.sim.get::<ActiveTool>(self.player)
    }

    /// Average horizontal speed (m/s) over the next `n` frames.
    fn speed(&mut self, n: u32) -> f32 {
        let before = self.sim.feet(self.player);
        self.frames(n);
        let after = self.sim.feet(self.player);
        Vec2::new(after.x - before.x, after.z - before.z).length() / (n as f32 * TICK_SECONDS)
    }

    fn shots(&self) -> usize {
        self.sim
            .recorded::<ShotFired>()
            .iter()
            .filter(|s| s.shooter == self.player)
            .count()
    }

    fn placed(&self) -> usize {
        self.sim
            .recorded::<PieceChanged>()
            .iter()
            .filter(|c| c.change == PieceChange::Placed)
            .count()
    }
}

fn assert_close(actual: f32, expected: f32, tolerance: f32, what: &str) {
    assert!(
        (actual - expected).abs() <= expected.abs() * tolerance,
        "{what}: {actual} not within {:.0}% of {expected}",
        tolerance * 100.0
    );
}

#[test]
fn holding_shift_aims_and_releasing_stops() {
    let mut pad = Pad::new();
    assert!(!pad.ads());
    for shift in [KeyCode::ShiftLeft, KeyCode::ShiftRight] {
        pad.tap(shift);
        assert!(pad.ads(), "{shift:?} pressed: aiming");
        pad.frames(60);
        assert!(pad.ads(), "{shift:?} held for a second: still aiming");
        pad.release(shift);
        pad.frame();
        assert!(!pad.ads(), "{shift:?} released: not aiming");
        pad.frames(30);
        assert!(!pad.ads(), "a hold, not a toggle");
    }
    // V used to toggle ADS; it does nothing now.
    pad.tap(KeyCode::KeyV);
    pad.frames(10);
    assert!(!pad.ads(), "V doesn't aim");
}

#[test]
fn ads_is_refused_in_build_mode_and_resumes_on_a_gun_while_shift_is_held() {
    let mut pad = Pad::new();
    pad.press(KeyCode::ShiftLeft);
    pad.frames(5);
    assert!(pad.ads());

    // Q picks the wall: build mode refuses ADS for as long as it lasts.
    pad.tap(KeyCode::KeyQ);
    pad.release(KeyCode::KeyQ);
    assert_eq!(pad.tool(), ActiveTool::Build(PieceKind::Wall));
    assert!(!pad.ads(), "no ADS in build mode");
    pad.frames(60);
    assert!(!pad.ads(), "Shift still held, still no ADS in build mode");

    // Back to the rifle with Shift still held: aiming again, no new press.
    pad.tap(KeyCode::Digit1);
    pad.release(KeyCode::Digit1);
    assert_eq!(pad.tool(), RIFLE);
    assert!(pad.ads(), "ADS resumes on the gun while Shift is held");

    // The same after a rifle reload: refused while reloading, back after it.
    pad.frames(20);
    pad.mouse.press(MouseButton::Left);
    pad.frame();
    pad.mouse.release(MouseButton::Left);
    assert_eq!(pad.shots(), 1);
    pad.tap(KeyCode::KeyR);
    pad.release(KeyCode::KeyR);
    assert!(!pad.ads(), "no ADS while the rifle reloads");
    pad.frames(150);
    assert!(
        pad.ads(),
        "ADS resumes after the reload while Shift is held"
    );
    pad.release(KeyCode::ShiftLeft);
    pad.frame();
    assert!(!pad.ads());
}

#[test]
fn holding_w_sprints_and_aiming_stops_the_sprint() {
    let mut pad = Pad::new();
    let t = pad.movement();
    let (run, sprint) = (t.run_speed, t.sprint_speed);

    // Holding W alone sprints: no sprint key.
    pad.press(KeyCode::KeyW);
    pad.frames(30);
    assert_close(pad.speed(30), sprint, 0.03, "holding W");
    assert!(pad.motor().sprinting);

    // Holding Shift aims and drops to run speed; letting go sprints again.
    pad.press(KeyCode::ShiftLeft);
    pad.frames(30);
    assert!(pad.ads());
    assert!(!pad.motor().sprinting, "aiming stops the sprint");
    assert_close(pad.speed(30), run, 0.03, "W while aiming");
    pad.release(KeyCode::ShiftLeft);
    pad.frames(30);
    assert!(
        pad.motor().sprinting,
        "sprinting again after letting go of Shift"
    );
    assert_close(pad.speed(30), sprint, 0.03, "W after aiming");

    // Forward diagonals sprint; strafing and backpedalling run (like Fortnite).
    for (keys, expected, what) in [
        (&[KeyCode::KeyW, KeyCode::KeyD][..], sprint, "W+D"),
        (&[KeyCode::KeyD][..], run, "D (strafe)"),
        (&[KeyCode::KeyA][..], run, "A (strafe)"),
        (&[KeyCode::KeyS][..], run, "S (backpedal)"),
    ] {
        pad.place(Vec3::new(-18.0, 0.0, 12.0), Facing::North);
        for key in keys {
            pad.press(*key);
        }
        pad.frames(20);
        assert_close(pad.speed(20), expected, 0.03, what);
        assert_eq!(pad.motor().sprinting, expected == sprint, "{what}");
    }

    // Shift does nothing to movement in build mode (there is no ADS there).
    pad.place(Vec3::new(-20.0, 0.0, 20.0), Facing::North);
    pad.tap(KeyCode::KeyE);
    pad.release(KeyCode::KeyE);
    pad.press(KeyCode::ShiftLeft);
    pad.press(KeyCode::KeyW);
    pad.frames(30);
    assert!(!pad.ads());
    assert_close(pad.speed(30), sprint, 0.03, "W + Shift in build mode");
}

#[test]
fn c_while_holding_w_slides() {
    let mut pad = Pad::new();
    let t = pad.movement();
    let slides = |pad: &Pad| {
        pad.sim
            .recorded::<GameCue>()
            .iter()
            .filter(|c| matches!(c, GameCue::SlideStart { who } if *who == pad.player))
            .count()
    };

    // Holding W, press C: a slide with its burst of speed.
    pad.press(KeyCode::KeyW);
    pad.frames(30);
    pad.tap(KeyCode::KeyC);
    assert_eq!(slides(&pad), 1);
    assert!(pad.motor().sliding);
    assert!(
        pad.speed(3) > t.sprint_speed,
        "the slide is faster than a sprint"
    );
    pad.release(KeyCode::KeyC);

    // Strafing (no W), C is just a crouch.
    pad.place(Vec3::new(-20.0, 0.0, 16.0), Facing::North);
    pad.press(KeyCode::KeyD);
    pad.frames(30);
    pad.tap(KeyCode::KeyC);
    assert!(!pad.motor().sliding, "no slide without W");
    assert!(pad.motor().crouched);
    assert_eq!(slides(&pad), 1);
    pad.release(KeyCode::KeyC);
    pad.release(KeyCode::KeyD);

    // Holding W while aiming isn't a sprint, so C crouches instead of sliding.
    pad.place(Vec3::new(-20.0, 0.0, 16.0), Facing::North);
    pad.press(KeyCode::KeyW);
    pad.press(KeyCode::ShiftLeft);
    pad.frames(30);
    pad.tap(KeyCode::KeyC);
    assert!(!pad.motor().sliding, "no slide while aiming");
    assert_eq!(slides(&pad), 1);
}

#[test]
fn the_right_click_does_nothing() {
    let mut pad = Pad::new();
    let before = pad.sim.get::<PlayerIntent>(pad.player).clone();

    // With a gun: no ADS, no shot.
    pad.mouse.press(MouseButton::Right);
    pad.frame();
    assert_eq!(*pad.sim.get::<PlayerIntent>(pad.player), before);
    pad.frames(30);
    pad.mouse.release(MouseButton::Right);
    pad.frame();
    assert!(!pad.ads(), "the right click doesn't aim");
    assert_eq!(pad.shots(), 0, "the right click doesn't fire");

    // In build mode: no piece, and the tool stays put.
    pad.tap(KeyCode::KeyQ);
    pad.release(KeyCode::KeyQ);
    pad.frames(15);
    pad.mouse.press(MouseButton::Right);
    pad.frames(30);
    pad.mouse.release(MouseButton::Right);
    pad.frame();
    assert_eq!(pad.placed(), 0, "the right click doesn't build");
    assert_eq!(pad.tool(), ActiveTool::Build(PieceKind::Wall));

    // The physical click still places and fires.
    pad.mouse.press(MouseButton::Left);
    pad.frame();
    pad.mouse.release(MouseButton::Left);
    pad.frame();
    assert_eq!(pad.placed(), 1, "the physical click places");
    pad.tap(KeyCode::Digit1);
    pad.release(KeyCode::Digit1);
    pad.frames(20);
    pad.mouse.press(MouseButton::Left);
    pad.frame();
    pad.mouse.release(MouseButton::Left);
    pad.frame();
    assert_eq!(pad.shots(), 1, "the physical click fires");
}
