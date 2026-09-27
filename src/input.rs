//! Keyboard + trackpad adapter: turns device input into the player's
//! [`PlayerIntent`], and owns cursor lock and pause/focus handling.
//! Disabled while a scenario drives the player.

use crate::{
    scenario::ScenarioRun,
    shared::{ActiveTool, Ads, AppState, PieceKind, Player, PlayerIntent, WeaponKind},
    tuning::Tuning,
};
use bevy::{
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use serde::{Deserialize, Serialize};

/// Look and trackpad feel. Owned by the input adapter.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct LookTuning {
    /// Radians of turn per unit of raw trackpad motion.
    pub sensitivity: f32,
    pub ads_multiplier: f32,
    pub build_multiplier: f32,
    /// Optional acceleration: fast swipes turn more, slow swipes aim finer.
    pub accel_enabled: bool,
    pub accel_exponent: f32,
    /// Motion (units per frame) at which acceleration gain is 1.
    pub accel_reference: f32,
    pub invert_y: bool,
    /// Vertical field of view in degrees (60–90).
    pub fov_deg: f32,
}

impl Default for LookTuning {
    fn default() -> Self {
        Self {
            sensitivity: 0.0035,
            ads_multiplier: 0.7,
            build_multiplier: 1.0,
            accel_enabled: false,
            accel_exponent: 1.35,
            accel_reference: 12.0,
            invert_y: false,
            fov_deg: 70.0,
        }
    }
}

impl LookTuning {
    /// Converts one frame of raw motion into (yaw, pitch) radians.
    pub fn look_delta(&self, raw: Vec2, ads: bool, building: bool) -> Vec2 {
        if raw == Vec2::ZERO {
            return Vec2::ZERO;
        }
        let mut gain = self.sensitivity;
        if ads {
            gain *= self.ads_multiplier;
        } else if building {
            gain *= self.build_multiplier;
        }
        if self.accel_enabled {
            let speed = raw.length() / self.accel_reference.max(0.01);
            gain *= speed.powf(self.accel_exponent - 1.0).clamp(0.4, 3.0);
        }
        let pitch_sign = if self.invert_y { 1.0 } else { -1.0 };
        Vec2::new(-raw.x * gain, pitch_sign * raw.y * gain)
    }
}

pub struct InputAdapterPlugin;

impl Plugin for InputAdapterPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<IgnoreNextLook>()
            .add_systems(
                PreUpdate,
                (pause_controls, cursor_lock)
                    .chain()
                    .after(bevy::input::InputSystems)
                    .run_if(not(resource_exists::<ScenarioRun>)),
            )
            .add_systems(
                PreUpdate,
                device_to_intent
                    .after(cursor_lock)
                    .run_if(in_state(AppState::Playing))
                    .run_if(not(resource_exists::<ScenarioRun>)),
            );
    }
}

/// Set when the cursor is (re)captured so the jump in raw motion is discarded.
#[derive(Resource, Default)]
struct IgnoreNextLook(bool);

fn pause_controls(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
) {
    let focused = window.map(|w| w.focused).unwrap_or(true);
    match state.get() {
        AppState::Playing if keys.just_pressed(KeyCode::Escape) || !focused => {
            next.set(AppState::Paused);
        }
        AppState::Paused if keys.just_pressed(KeyCode::Escape) && focused => {
            next.set(AppState::Playing);
        }
        _ => {}
    }
}

fn cursor_lock(
    state: Res<State<AppState>>,
    cursor: Option<Single<&mut CursorOptions, With<PrimaryWindow>>>,
    mut ignore: ResMut<IgnoreNextLook>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut was_playing: Local<bool>,
) {
    let playing = *state.get() == AppState::Playing;
    if playing != *was_playing {
        // Never carry held keys or buttons across a pause boundary.
        keys.reset_all();
        mouse.reset_all();
        *was_playing = playing;
        ignore.0 = true;
    }
    let Some(mut cursor) = cursor else {
        return;
    };
    let desired = if playing {
        CursorGrabMode::Locked
    } else {
        CursorGrabMode::None
    };
    if cursor.grab_mode != desired {
        cursor.grab_mode = desired;
        ignore.0 = true;
    }
    cursor.visible = !playing;
}

fn device_to_intent(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    tuning: Res<Tuning>,
    mut ignore: ResMut<IgnoreNextLook>,
    player: Option<Single<(&mut PlayerIntent, &ActiveTool, &Ads), With<Player>>>,
) {
    let Some(player) = player else {
        return;
    };
    let (mut intent, tool, ads) = player.into_inner();
    buttons_to_intent(&keys, &mouse, &mut intent);

    if std::mem::take(&mut ignore.0) {
        return;
    }
    intent.look_delta += tuning.look.look_delta(motion.delta, ads.0, tool.is_build());
}

/// Maps held keys and trackpad buttons onto `intent` (docs/SPEC.md → Controls):
///
/// - W A S D move. Holding W sprints (D41: sprint by default, no sprint key);
///   movement decides when that applies (forward, standing, not aiming).
/// - Holding either Shift aims down sights (D40: a hold, never a toggle).
/// - Space jumps; C crouches, and a C press while sprinting slides.
/// - The physical click fires or places; R reloads; 1 / 2 and Q / E / F pick tools.
/// - The secondary (right) click and V do nothing (D40, D42).
///
/// Held fields are overwritten; press edges are OR-ed in, so they stay latched
/// until a fixed tick consumes them. Look is handled separately.
pub fn buttons_to_intent(
    keys: &ButtonInput<KeyCode>,
    mouse: &ButtonInput<MouseButton>,
    intent: &mut PlayerIntent,
) {
    let axis =
        |neg: KeyCode, pos: KeyCode| (keys.pressed(pos) as i32 - keys.pressed(neg) as i32) as f32;
    let raw = Vec2::new(
        axis(KeyCode::KeyA, KeyCode::KeyD),
        axis(KeyCode::KeyS, KeyCode::KeyW),
    );
    intent.move_axis = raw.clamp_length_max(1.0);

    intent.jump = keys.pressed(KeyCode::Space);
    intent.jump_pressed |= keys.just_pressed(KeyCode::Space);
    intent.sprint = keys.pressed(KeyCode::KeyW);
    intent.crouch = keys.pressed(KeyCode::KeyC);
    intent.crouch_pressed |= keys.just_pressed(KeyCode::KeyC);
    intent.fire = mouse.pressed(MouseButton::Left);
    intent.fire_pressed |= mouse.just_pressed(MouseButton::Left);
    intent.ads_held = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    intent.reload_pressed |= keys.just_pressed(KeyCode::KeyR);

    let selections = [
        (KeyCode::Digit1, ActiveTool::Weapon(WeaponKind::Rifle)),
        (KeyCode::Digit2, ActiveTool::Weapon(WeaponKind::Pump)),
        (KeyCode::KeyQ, ActiveTool::Build(PieceKind::Wall)),
        (KeyCode::KeyE, ActiveTool::Build(PieceKind::Ramp)),
        (KeyCode::KeyF, ActiveTool::Build(PieceKind::Floor)),
    ];
    for (key, tool) in selections {
        if keys.just_pressed(key) {
            intent.select = Some(tool);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter_world() -> (World, Entity) {
        let mut world = World::new();
        world.insert_resource(ButtonInput::<KeyCode>::default());
        world.insert_resource(ButtonInput::<MouseButton>::default());
        world.insert_resource(AccumulatedMouseMotion::default());
        world.insert_resource(Tuning::default());
        world.insert_resource(IgnoreNextLook(false));
        let player = world
            .spawn((
                Player,
                PlayerIntent::default(),
                ActiveTool::default(),
                Ads(false),
            ))
            .id();
        (world, player)
    }

    fn run_adapter(world: &mut World) {
        use bevy::ecs::system::RunSystemOnce;
        world.run_system_once(device_to_intent).unwrap();
    }

    #[test]
    fn keys_and_trackpad_map_to_intent() {
        use bevy::ecs::system::RunSystemOnce as _;
        let (mut world, player) = adapter_world();
        // Held W + physical click: moving forward, sprinting, and firing on the press.
        world
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        world.resource_mut::<AccumulatedMouseMotion>().delta = Vec2::new(12.0, 0.0);
        run_adapter(&mut world);
        let intent = world.get::<PlayerIntent>(player).unwrap().clone();
        assert_eq!(intent.move_axis, Vec2::Y);
        assert!(intent.sprint, "holding W sprints");
        assert!(intent.fire && intent.fire_pressed);
        assert!(!intent.ads_held);
        assert!(intent.look_delta.x < 0.0, "swiping right turns right");
        // Piece and gun keys select tools.
        for (key, tool) in [
            (KeyCode::KeyQ, ActiveTool::Build(PieceKind::Wall)),
            (KeyCode::KeyE, ActiveTool::Build(PieceKind::Ramp)),
            (KeyCode::KeyF, ActiveTool::Build(PieceKind::Floor)),
            (KeyCode::Digit2, ActiveTool::Weapon(WeaponKind::Pump)),
            (KeyCode::Digit1, ActiveTool::Weapon(WeaponKind::Rifle)),
        ] {
            world.resource_mut::<ButtonInput<KeyCode>>().clear();
            world.resource_mut::<ButtonInput<KeyCode>>().press(key);
            world.run_system_once(device_to_intent).unwrap();
            assert_eq!(
                world.get::<PlayerIntent>(player).unwrap().select,
                Some(tool)
            );
            world.resource_mut::<ButtonInput<KeyCode>>().release(key);
        }
    }

    #[test]
    fn either_shift_holds_aim_and_only_w_sprints() {
        let mut keys = ButtonInput::<KeyCode>::default();
        let mouse = ButtonInput::<MouseButton>::default();
        let mut intent = PlayerIntent::default();
        for shift in [KeyCode::ShiftLeft, KeyCode::ShiftRight] {
            keys.press(shift);
            buttons_to_intent(&keys, &mouse, &mut intent);
            assert!(intent.ads_held, "{shift:?} held aims");
            assert!(!intent.sprint, "Shift is not a sprint key");
            keys.clear();
            buttons_to_intent(&keys, &mouse, &mut intent);
            assert!(intent.ads_held, "still held on the next frame");
            keys.release(shift);
            buttons_to_intent(&keys, &mouse, &mut intent);
            assert!(!intent.ads_held, "releasing {shift:?} stops aiming");
        }
        // Strafing and backpedalling don't ask for a sprint; forward diagonals do.
        for (held, sprint) in [
            (&[KeyCode::KeyA][..], false),
            (&[KeyCode::KeyD][..], false),
            (&[KeyCode::KeyS][..], false),
            (&[KeyCode::KeyW, KeyCode::KeyD][..], true),
            (&[KeyCode::KeyW][..], true),
        ] {
            keys.reset_all();
            for key in held {
                keys.press(*key);
            }
            buttons_to_intent(&keys, &mouse, &mut intent);
            assert_eq!(intent.sprint, sprint, "{held:?}");
        }
    }

    #[test]
    fn right_click_and_v_do_nothing() {
        let mut keys = ButtonInput::<KeyCode>::default();
        let mut mouse = ButtonInput::<MouseButton>::default();
        let mut intent = PlayerIntent::default();
        mouse.press(MouseButton::Right);
        keys.press(KeyCode::KeyV);
        buttons_to_intent(&keys, &mouse, &mut intent);
        assert_eq!(intent, PlayerIntent::default());
    }

    #[test]
    fn look_delta_directions_and_multipliers() {
        let look = LookTuning::default();
        // Swiping right turns right (negative yaw), swiping down looks down.
        let d = look.look_delta(Vec2::new(10.0, 10.0), false, false);
        assert!(d.x < 0.0 && d.y < 0.0);
        let ads = look.look_delta(Vec2::new(10.0, 0.0), true, false);
        assert!((ads.x / d.x - look.ads_multiplier).abs() < 1e-5);
        assert_eq!(look.look_delta(Vec2::ZERO, false, false), Vec2::ZERO);
    }
}

/// `--input-probe`: logs, once per second, whether trackpad motion and clicks keep
/// registering while movement keys are held (the Phase 0 trackpad check).
#[derive(Resource, Debug, Default)]
pub struct InputProbe {
    frames_moving: u32,
    motion_frames_moving: u32,
    clicks_moving: u32,
    motion_frames_still: u32,
    last_report: f64,
}

pub struct InputProbePlugin;

impl Plugin for InputProbePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PreUpdate,
            probe_input
                .after(bevy::input::InputSystems)
                .run_if(resource_exists::<InputProbe>),
        );
    }
}

fn probe_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    time: Res<Time<Real>>,
    mut probe: ResMut<InputProbe>,
) {
    let moving = [KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD]
        .iter()
        .any(|k| keys.pressed(*k));
    let moved = motion.delta != Vec2::ZERO;
    if moving {
        probe.frames_moving += 1;
        probe.motion_frames_moving += moved as u32;
        probe.clicks_moving += mouse.just_pressed(MouseButton::Left) as u32;
    } else {
        probe.motion_frames_still += moved as u32;
    }
    let now = time.elapsed_secs_f64();
    if now - probe.last_report >= 1.0 {
        probe.last_report = now;
        println!(
            "PIECED_PROBE t={now:.0}s frames_with_wasd={} look_motion_frames_with_wasd={} clicks_with_wasd={} look_motion_frames_without_wasd={}",
            probe.frames_moving,
            probe.motion_frames_moving,
            probe.clicks_moving,
            probe.motion_frames_still
        );
    }
}
