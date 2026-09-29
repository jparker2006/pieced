//! Input-to-present latency (M4 performance follow-up; M1's G3: median
//! ≤ 33 ms). The probe runs headless on a scripted clock: key presses and
//! trackpad motion go in as Bevy input messages at known OS event ages, the
//! "render world" presents each frame at a known time (on the same frame, or
//! a frame late as under pipelined rendering), and the resolved samples come
//! out in [`LastLatency`] rows and, through the session writer, in
//! `session.json` and the `PIECED_S2` line.

use bevy::{
    input::{
        ButtonState, InputPlugin,
        keyboard::{Key, KeyboardInput},
        mouse::{MouseButton, MouseButtonInput, MouseMotion},
    },
    prelude::*,
};
use pieced::{
    latency::{InputClock, InputKind, InputLatencyPlugin, LastLatency, LatencyProbe, PresentLog},
    telemetry::MainFrame,
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

const MS: u64 = 1_000_000;

/// A headless app with the probe on a scripted clock (`now` in ns).
fn probe_app(now: Arc<AtomicU64>) -> App {
    let mut app = App::new();
    let clock_now = now.clone();
    app.add_plugins((MinimalPlugins, InputPlugin))
        .init_resource::<MainFrame>()
        .add_systems(First, |mut f: ResMut<MainFrame>| f.0 += 1)
        .insert_resource(InputClock {
            now_ns: Arc::new(move || clock_now.load(Ordering::Relaxed)),
            // The OS saw the press 4 ms and the newest look motion 2 ms
            // before the game read them.
            event_age_s: Arc::new(|kind| match kind {
                InputKind::Press => Some(0.004),
                InputKind::Motion => Some(0.002),
            }),
        })
        .add_plugins(InputLatencyPlugin);
    app
}

fn press(app: &mut App) {
    let window = Entity::PLACEHOLDER;
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::Space,
        logical_key: Key::Space,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
}

fn click(app: &mut App) {
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window: Entity::PLACEHOLDER,
    });
}

fn look(app: &mut App) {
    app.world_mut().write_message(MouseMotion {
        delta: Vec2::new(3.0, -1.0),
    });
}

fn frame(app: &App) -> u64 {
    app.world().resource::<MainFrame>().0
}

fn last(app: &App) -> LastLatency {
    *app.world().resource::<LastLatency>()
}

#[test]
fn a_press_is_timed_from_the_os_event_to_its_frames_present() {
    let now = Arc::new(AtomicU64::new(1_000 * MS));
    let mut app = probe_app(now.clone());
    app.update();
    // Frame 2 reads a key press at t = 1000 ms (the OS saw it at 996 ms).
    press(&mut app);
    app.update();
    let consumed = frame(&app);
    assert_eq!(last(&app), LastLatency::default(), "not presented yet");
    // Its render presents it 15 ms later (serial rendering: before the next
    // frame's `Last`).
    app.world()
        .resource::<PresentLog>()
        .record(consumed, 1_015 * MS);
    app.update();
    let l = last(&app);
    assert!((l.press_ms - 19.0).abs() < 1e-3, "{l:?}");
    assert_eq!(l.motion_ms, 0.0);
    // One sample per press: the next row has none.
    app.update();
    assert_eq!(last(&app), LastLatency::default());
    assert_eq!(app.world().resource::<LatencyProbe>().waiting(), (0, 0));
}

#[test]
fn look_motion_and_clicks_are_separate_samples_and_can_resolve_a_frame_late() {
    let now = Arc::new(AtomicU64::new(2_000 * MS));
    let mut app = probe_app(now.clone());
    app.update();
    // Frame 2: motion and a click together.
    look(&mut app);
    click(&mut app);
    app.update();
    let first = frame(&app);
    // Frame 3: motion only, at t = 2016.7 ms.
    now.store(2_016_700_000, Ordering::Relaxed);
    look(&mut app);
    app.update();
    let second = frame(&app);
    // Pipelined: frame 2 is presented only while frame 3 simulated, at
    // 2030 ms, and frame 3 at 2046.7 ms.
    let presents = app.world().resource::<PresentLog>().clone();
    presents.record(first, 2_030 * MS);
    app.update();
    let l = last(&app);
    assert!((l.press_ms - 34.0).abs() < 1e-3, "{l:?}");
    assert!((l.motion_ms - 32.0).abs() < 1e-3, "{l:?}");
    presents.record(second, 2_046_700_000);
    app.update();
    let l = last(&app);
    assert_eq!(l.press_ms, 0.0);
    assert!((l.motion_ms - 32.0).abs() < 1e-3, "{l:?}");
}

#[test]
fn key_repeats_and_releases_are_not_presses_and_idle_frames_are_not_sampled() {
    let now = Arc::new(AtomicU64::new(3_000 * MS));
    let mut app = probe_app(now);
    app.update();
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::KeyW,
        logical_key: Key::Character("w".into()),
        state: ButtonState::Pressed,
        text: None,
        repeat: true,
        window: Entity::PLACEHOLDER,
    });
    app.world_mut().write_message(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Released,
        window: Entity::PLACEHOLDER,
    });
    app.update();
    app.update();
    assert_eq!(app.world().resource::<LatencyProbe>().waiting(), (0, 0));
}

#[test]
fn a_stale_os_reading_gives_no_sample() {
    let now = Arc::new(AtomicU64::new(4_000 * MS));
    let mut app = App::new();
    let clock_now = now.clone();
    app.add_plugins((MinimalPlugins, InputPlugin))
        .init_resource::<MainFrame>()
        .add_systems(First, |mut f: ResMut<MainFrame>| f.0 += 1)
        .insert_resource(InputClock {
            now_ns: Arc::new(move || clock_now.load(Ordering::Relaxed)),
            // The newest key-down the OS knows of is 2 s old: whatever the
            // game read wasn't it.
            event_age_s: Arc::new(|_| Some(2.0)),
        })
        .add_plugins(InputLatencyPlugin);
    app.update();
    press(&mut app);
    app.update();
    assert_eq!(app.world().resource::<LatencyProbe>().waiting(), (0, 0));
}
