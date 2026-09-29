//! Input-to-present latency for the session log (M4 performance follow-up;
//! M1's G3 asks for a median ≤ 33 ms, and pipelined rendering is the default
//! only while it stays there).
//!
//! **What is measured.** For each frame that consumed input, the time from
//! the OS input event to the moment that frame was submitted and presented
//! (`present` is called at the end of [`RenderSystems::Render`]). Two kinds,
//! logged separately:
//!
//! - **press**: a key or mouse button going down (fire, build, jump...). One
//!   event per frame, so its time is exact.
//! - **motion**: trackpad or mouse look. Many events arrive per frame; the
//!   sample uses the newest one, so it is the freshest look data's latency
//!   (the batch's oldest event is at most one frame older).
//!
//! **Where the event time comes from.** On macOS,
//! `CGEventSourceSecondsSinceLastEventType` (HID system state) says how long
//! ago the newest event of a type entered the system, read the moment the
//! game sees the input (`PreUpdate`, after Bevy's input systems). Elsewhere
//! the time the game saw it; tests script both clocks ([`InputClock`]).
//!
//! **Limits.** Like M1's G3 probe, it excludes the GPU's execution of the
//! frame and the display's scanout: with vsync the frame shows at the first
//! vblank after its GPU work, so input-to-photon is this plus the GPU time
//! left after present, rounded up to the next refresh. Under pipelined
//! rendering the frame that consumed the input is presented by the render
//! thread while the next frame simulates; this measures that directly.
//!
//! **How it rides the log.** The render world writes each frame's present
//! time into a fixed ring ([`PresentLog`]). The main world keeps the frames
//! that consumed input in small fixed queues and resolves each once its
//! present is known (one or two frames later), at most one sample per kind
//! per frame row ([`LastLatency`] → `press_latency_ms`, `motion_latency_ms`
//! in `frames.csv`). Nothing allocates per frame.

use crate::{profile::now_ns, telemetry::MainFrame};
use bevy::{
    input::{
        ButtonState, InputSystems,
        keyboard::KeyboardInput,
        mouse::{AccumulatedMouseMotion, MouseButtonInput},
    },
    prelude::*,
    render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems},
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Presents remembered (frames).
pub const PRESENT_RING: usize = 64;
/// Frames waiting for their present, per kind.
pub const PENDING: usize = 16;
/// An event older than this when the game saw it isn't the one it consumed
/// (a stale reading): no sample.
pub const MAX_EVENT_AGE_S: f64 = 0.25;

/// The kinds of input sampled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputKind {
    Press,
    Motion,
}

/// The probe's clocks: now (ns since process start, the clock the present
/// log uses too) and how long ago (s) the newest OS event of a kind
/// happened, when the OS says. The game uses [`now_ns`] and
/// [`os_event_age`]; tests script both. Insert it before the plugin to
/// replace it (the render world gets the same one).
#[derive(Resource, Clone)]
pub struct InputClock {
    pub now_ns: Arc<dyn Fn() -> u64 + Send + Sync>,
    pub event_age_s: Arc<dyn Fn(InputKind) -> Option<f64> + Send + Sync>,
}

impl Default for InputClock {
    fn default() -> Self {
        Self {
            now_ns: Arc::new(now_ns),
            event_age_s: Arc::new(os_event_age),
        }
    }
}

#[cfg(target_os = "macos")]
mod os {
    // CoreGraphics' `CGEventSourceSecondsSinceLastEventType`: plain values in
    // and out, callable from any thread.
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        safe fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
    }
    /// `kCGEventSourceStateHIDSystemState`: events as the hardware posted them.
    const HID_SYSTEM_STATE: i32 = 1;
    /// keyDown, leftMouseDown, rightMouseDown, otherMouseDown.
    pub const PRESS: [u32; 4] = [10, 1, 3, 25];
    /// mouseMoved, leftMouseDragged, rightMouseDragged, otherMouseDragged.
    pub const MOTION: [u32; 4] = [5, 6, 7, 27];

    pub fn newest(types: &[u32]) -> Option<f64> {
        types
            .iter()
            .map(|t| CGEventSourceSecondsSinceLastEventType(HID_SYSTEM_STATE, *t))
            .filter(|s| s.is_finite() && *s >= 0.0)
            .min_by(f64::total_cmp)
    }
}

/// How long ago the newest OS event of a kind happened (macOS), else none.
pub fn os_event_age(kind: InputKind) -> Option<f64> {
    #[cfg(target_os = "macos")]
    {
        match kind {
            InputKind::Press => os::newest(&os::PRESS),
            InputKind::Motion => os::newest(&os::MOTION),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = kind;
        None
    }
}

/// Each recent frame's present time (ns since process start), by main-world
/// frame number: written by the render world, read by the main world (the
/// same `Arc` in both), lock-free.
#[derive(Resource, Debug, Clone, Default)]
pub struct PresentLog(Arc<PresentSlots>);

#[derive(Debug)]
pub struct PresentSlots {
    frames: [AtomicU64; PRESENT_RING],
    times: [AtomicU64; PRESENT_RING],
}

impl Default for PresentSlots {
    fn default() -> Self {
        Self {
            frames: std::array::from_fn(|_| AtomicU64::new(0)),
            times: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}

impl PresentLog {
    /// Records that `frame` was presented at `t_ns`.
    pub fn record(&self, frame: u64, t_ns: u64) {
        let i = (frame % PRESENT_RING as u64) as usize;
        // The time first, then the frame number that makes it valid.
        self.0.times[i].store(t_ns, Ordering::Relaxed);
        self.0.frames[i].store(frame, Ordering::Release);
    }

    /// When `frame` was presented, while it's remembered.
    pub fn presented(&self, frame: u64) -> Option<u64> {
        let i = (frame % PRESENT_RING as u64) as usize;
        (frame != 0 && self.0.frames[i].load(Ordering::Acquire) == frame)
            .then(|| self.0.times[i].load(Ordering::Relaxed))
    }
}

/// A frame that consumed input, waiting for its present.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Pending {
    frame: u64,
    arrival_ns: u64,
}

/// A fixed queue of [`Pending`] (oldest first); a full queue drops its oldest.
#[derive(Debug, Clone, Default)]
struct PendingQueue {
    items: [Pending; PENDING],
    head: usize,
    len: usize,
}

impl PendingQueue {
    fn push(&mut self, item: Pending) {
        if self.len == PENDING {
            self.pop();
        }
        self.items[(self.head + self.len) % PENDING] = item;
        self.len += 1;
    }

    fn front(&self) -> Option<Pending> {
        (self.len > 0).then(|| self.items[self.head])
    }

    fn pop(&mut self) {
        if self.len > 0 {
            self.head = (self.head + 1) % PENDING;
            self.len -= 1;
        }
    }
}

/// The main world's side: frames waiting for their present, per kind.
#[derive(Resource, Debug, Default)]
pub struct LatencyProbe {
    press: PendingQueue,
    motion: PendingQueue,
}

impl LatencyProbe {
    /// Frames still waiting for their present (press, motion).
    pub fn waiting(&self) -> (usize, usize) {
        (self.press.len, self.motion.len)
    }
}

/// The samples resolved this frame (ms; 0 = none), for the session log's row.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub struct LastLatency {
    pub press_ms: f32,
    pub motion_ms: f32,
}

/// The latency systems in `Last`; the session log's row reads
/// [`LastLatency`] after them.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LatencySystems;

/// Samples input-to-present latency (see the module docs). Needs Bevy's
/// input plugin and [`MainFrame`] (from [`crate::telemetry::TelemetryPlugin`]).
/// Without a render world nothing is presented, so nothing resolves.
pub struct InputLatencyPlugin;

impl Plugin for InputLatencyPlugin {
    fn build(&self, app: &mut App) {
        let presents = PresentLog::default();
        let clock = app
            .world()
            .get_resource::<InputClock>()
            .cloned()
            .unwrap_or_default();
        app.insert_resource(presents.clone())
            .insert_resource(clock.clone())
            .init_resource::<LatencyProbe>()
            .init_resource::<LastLatency>()
            .add_systems(PreUpdate, note_input.after(InputSystems))
            .add_systems(Last, resolve_latency.in_set(LatencySystems));
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .insert_resource(presents)
                .insert_resource(clock)
                .init_resource::<PresentFrame>()
                .add_systems(ExtractSchedule, extract_present_frame)
                .add_systems(
                    Render,
                    record_present
                        .after(RenderSystems::Render)
                        .before(RenderSystems::Cleanup),
                );
        }
    }
}

/// The main-world frame the render world is drawing.
#[derive(Resource, Debug, Default, Clone, Copy)]
struct PresentFrame(u64);

fn extract_present_frame(mut frame: ResMut<PresentFrame>, main: Extract<Option<Res<MainFrame>>>) {
    frame.0 = main.as_deref().map_or(0, |f| f.0);
}

fn record_present(frame: Res<PresentFrame>, log: Res<PresentLog>, clock: Res<InputClock>) {
    log.record(frame.0, (clock.now_ns)());
}

/// When the event the game just saw happened: `now` minus the OS's age of
/// the newest event of that kind, or `now` when the OS doesn't say. `None`
/// for a stale reading.
pub fn arrival_ns(now_ns: u64, age_s: Option<f64>) -> Option<u64> {
    match age_s {
        None => Some(now_ns),
        Some(age) if age <= MAX_EVENT_AGE_S => Some(now_ns.saturating_sub((age * 1e9) as u64)),
        Some(_) => None,
    }
}

/// `PreUpdate`: queues this frame if it consumed a press or look motion.
pub fn note_input(
    frame: Option<Res<MainFrame>>,
    clock: Res<InputClock>,
    mut probe: ResMut<LatencyProbe>,
    mut keys: MessageReader<KeyboardInput>,
    mut buttons: MessageReader<MouseButtonInput>,
    motion: Option<Res<AccumulatedMouseMotion>>,
) {
    let pressed_key = keys
        .read()
        .any(|k| k.state == ButtonState::Pressed && !k.repeat);
    let pressed_button = buttons.read().any(|b| b.state == ButtonState::Pressed);
    let pressed = pressed_key || pressed_button;
    let moved = motion.is_some_and(|m| m.delta != Vec2::ZERO);
    let Some(frame) = frame.map(|f| f.0) else {
        return;
    };
    if !(pressed || moved) {
        return;
    }
    let now = (clock.now_ns)();
    if pressed && let Some(arrival_ns) = arrival_ns(now, (clock.event_age_s)(InputKind::Press)) {
        probe.press.push(Pending { frame, arrival_ns });
    }
    if moved && let Some(arrival_ns) = arrival_ns(now, (clock.event_age_s)(InputKind::Motion)) {
        probe.motion.push(Pending { frame, arrival_ns });
    }
}

/// Resolves the oldest waiting frame whose present is known (ms; 0 = none).
fn resolve(queue: &mut PendingQueue, presents: &PresentLog, current: u64) -> f32 {
    while let Some(p) = queue.front() {
        // Forgotten by the ring (never presented, e.g. no renderer): drop.
        if current.saturating_sub(p.frame) >= PRESENT_RING as u64 {
            queue.pop();
            continue;
        }
        let Some(t) = presents.presented(p.frame) else {
            return 0.0;
        };
        queue.pop();
        return (t.saturating_sub(p.arrival_ns) as f64 * 1e-6) as f32;
    }
    0.0
}

/// `Last`: this frame's resolved samples into [`LastLatency`].
pub fn resolve_latency(
    frame: Option<Res<MainFrame>>,
    presents: Res<PresentLog>,
    mut probe: ResMut<LatencyProbe>,
    mut last: ResMut<LastLatency>,
) {
    let current = frame.map_or(0, |f| f.0);
    let probe = &mut *probe;
    *last = LastLatency {
        press_ms: resolve(&mut probe.press, &presents, current),
        motion_ms: resolve(&mut probe.motion, &presents, current),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_present_ring_remembers_recent_frames_only() {
        let log = PresentLog::default();
        assert_eq!(log.presented(5), None);
        log.record(5, 1_000);
        assert_eq!(log.presented(5), Some(1_000));
        log.record(5 + PRESENT_RING as u64, 2_000);
        assert_eq!(log.presented(5), None, "overwritten");
        assert_eq!(log.presented(0), None, "frame 0 is never a frame");
    }

    #[test]
    fn a_full_queue_drops_its_oldest() {
        let mut q = PendingQueue::default();
        for f in 1..=(PENDING as u64 + 3) {
            q.push(Pending {
                frame: f,
                arrival_ns: f,
            });
        }
        assert_eq!(q.front().map(|p| p.frame), Some(4));
        for _ in 0..PENDING {
            q.pop();
        }
        assert_eq!(q.front(), None);
    }

    #[test]
    fn arrival_is_now_minus_the_os_age_unless_stale() {
        assert_eq!(arrival_ns(50_000_000, None), Some(50_000_000));
        assert_eq!(arrival_ns(50_000_000, Some(0.004)), Some(46_000_000));
        assert_eq!(arrival_ns(50_000_000, Some(1.0)), None);
    }

    #[test]
    fn frames_that_were_never_presented_are_dropped() {
        let mut q = PendingQueue::default();
        q.push(Pending {
            frame: 3,
            arrival_ns: 0,
        });
        let log = PresentLog::default();
        assert_eq!(resolve(&mut q, &log, 10), 0.0);
        assert_eq!(q.len, 1, "still waiting");
        assert_eq!(resolve(&mut q, &log, 3 + PRESENT_RING as u64), 0.0);
        assert_eq!(q.len, 0, "given up");
    }
}
