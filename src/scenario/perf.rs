//! Slice G — `perf`: sustained mixed play for the G2 performance gate.
//!
//! A seeded script cycles through representative heavy play — running,
//! sprinting and sliding, jumping, turbo-built 1x1 boxes and ramp rushes,
//! rifle sprays and pump shots at the strafing dummy, gun switching and
//! shooting down pieces — driven only through the player's [`PlayerIntent`]
//! (look included, as `look_delta`). Old player-built pieces are broken once
//! too many stand, so the arena never fills up and the load stays
//! representative. The timed window (default 5 minutes) follows a 10 s warm-up
//! and takes no screenshots; one is captured after it ends.
//!
//! The script itself ([`PerfScript`]) is a pure function of the seed and time,
//! so the intents it asks for are reproducible; the director adds only aiming
//! (toward the dummy or a piece) and steering away from the arena edge, which
//! depend on where things are.
//!
//! [`PlayerIntent`]: crate::shared::PlayerIntent

use super::{Director, DirectorStatus, ScenarioClock, ScenarioRun, capture, with_intent};
use crate::{
    building::{InitialCover, Piece},
    combat::{CombatStats, Downed},
    dummy::{Dummy, look_toward},
    rng::Rng,
    shared::{
        ARENA_HALF, ActiveTool, EyeHeight, LookAngles, PieceChange, PieceChanged, PieceKind,
        Player, WeaponKind,
    },
    telemetry::{self, FrameLog},
    tuning::Tuning,
};
use bevy::{
    ecs::message::{MessageCursor, Messages},
    prelude::*,
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    f32::consts::{FRAC_PI_2, PI, TAU},
};

/// Default timed window (after the warm-up).
pub const DEFAULT_SECONDS: f64 = 300.0;
/// Default warm-up excluded from the frame statistics.
pub const WARMUP_SECONDS: f64 = 10.0;
/// Seed of the scripted session.
pub const SEED: u64 = 0x9E_12F0;
/// When more player-built pieces than this stand, the oldest are broken…
pub const MAX_LIVE_PIECES: usize = 40;
/// …down to this many.
pub const KEEP_LIVE_PIECES: usize = 28;

pub fn director(name: &str) -> Option<Box<dyn Director>> {
    (name == "perf").then(|| Box::new(Perf::new(SEED)) as Box<dyn Director>)
}

// ---------------------------------------------------------------------------
// The script (pure, deterministic)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Activity {
    Run,
    Slide,
    Jump,
    BoxUp,
    RampRush,
    Spray,
    Pump,
    SwitchGuns,
    BreakPieces,
}

impl Activity {
    pub const ALL: [Activity; 9] = [
        Activity::Run,
        Activity::Slide,
        Activity::Jump,
        Activity::BoxUp,
        Activity::RampRush,
        Activity::Spray,
        Activity::Pump,
        Activity::SwitchGuns,
        Activity::BreakPieces,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Activity::Run => "run",
            Activity::Slide => "slide",
            Activity::Jump => "jump",
            Activity::BoxUp => "box_up",
            Activity::RampRush => "ramp_rush",
            Activity::Spray => "spray",
            Activity::Pump => "pump",
            Activity::SwitchGuns => "switch_guns",
            Activity::BreakPieces => "break_pieces",
        }
    }

    fn index(self) -> usize {
        Activity::ALL.iter().position(|a| *a == self).unwrap_or(0)
    }

    /// Duration range in seconds.
    fn duration(self) -> (f64, f64) {
        match self {
            Activity::Run => (3.0, 5.0),
            Activity::Slide => (2.5, 3.5),
            Activity::Jump => (2.5, 3.5),
            Activity::BoxUp => (1.6, 1.6),
            Activity::RampRush => (3.0, 4.0),
            Activity::Spray => (3.0, 4.5),
            Activity::Pump => (3.0, 4.5),
            Activity::SwitchGuns => (2.5, 3.5),
            Activity::BreakPieces => (3.0, 4.0),
        }
    }
}

/// One scripted stretch of play.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub activity: Activity,
    pub start: f64,
    pub duration: f64,
    /// ±1: mirrors strafes and turns so repeats differ.
    pub sign: f32,
}

/// Where the script wants the view to go this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LookGoal {
    /// Turn at `yaw_rate` (rad/s) while easing toward `pitch` (rad).
    Sweep { yaw_rate: f32, pitch: f32 },
    /// Yaw relative to the view at the start of the segment, and absolute pitch.
    Pose { yaw: f32, pitch: f32 },
    /// Track the dummy's chest.
    Dummy,
    /// Track the nearest player-built piece (the dummy when there is none).
    Piece,
}

/// The intents the script asks for between two instants.
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptFrame {
    pub activity: Activity,
    pub move_axis: Vec2,
    pub sprint: bool,
    pub jump: bool,
    pub jump_pressed: bool,
    pub crouch: bool,
    pub crouch_pressed: bool,
    pub fire: bool,
    pub fire_pressed: bool,
    pub ads_toggle_pressed: bool,
    pub reload_pressed: bool,
    pub select: Option<ActiveTool>,
    pub look: LookGoal,
}

impl ScriptFrame {
    fn new(activity: Activity, look: LookGoal) -> Self {
        Self {
            activity,
            move_axis: Vec2::ZERO,
            sprint: false,
            jump: false,
            jump_pressed: false,
            crouch: false,
            crouch_pressed: false,
            fire: false,
            fire_pressed: false,
            ads_toggle_pressed: false,
            reload_pressed: false,
            select: None,
            look,
        }
    }
}

/// An event at local time `at` happened in (u0, u].
fn crossed(u0: f64, u: f64, at: f64) -> bool {
    u0 < at && at <= u
}

/// An event repeating every `period` from `first` happened in (u0, u].
fn crossed_every(u0: f64, u: f64, first: f64, period: f64) -> bool {
    if u < first {
        return false;
    }
    let last = first + ((u - first) / period).floor() * period;
    last > u0
}

/// Inside a `hold`-long window that repeats every `period` from `first`.
fn within_every(u: f64, first: f64, period: f64, hold: f64) -> bool {
    u >= first && (u - first) % period < hold
}

/// Smooth 0→1 ramp of `u` over [a, b].
fn ease(u: f64, a: f64, b: f64) -> f32 {
    let x = ((u - a) / (b - a)).clamp(0.0, 1.0) as f32;
    x * x * (3.0 - 2.0 * x)
}

const RIFLE: ActiveTool = ActiveTool::Weapon(WeaponKind::Rifle);
const PUMP: ActiveTool = ActiveTool::Weapon(WeaponKind::Pump);

/// The seeded perf session.
#[derive(Debug, Clone, PartialEq)]
pub struct PerfScript {
    pub seed: u64,
    segments: Vec<Segment>,
}

impl PerfScript {
    /// Plans at least `seconds` of play: repeated cycles, each a seeded shuffle
    /// of every activity, so each kind of load recurs about every 30 s.
    pub fn new(seed: u64, seconds: f64) -> Self {
        let mut rng = Rng::new(seed);
        let mut segments = Vec::new();
        let mut t = 0.0;
        while t < seconds.max(0.0) + 1.0 {
            let mut cycle = Activity::ALL;
            for i in (1..cycle.len()).rev() {
                let j = (rng.next_u64() % (i as u64 + 1)) as usize;
                cycle.swap(i, j);
            }
            for activity in cycle {
                let (lo, hi) = activity.duration();
                let duration = lo + (hi - lo) * rng.next_f32() as f64;
                let sign = if rng.chance(0.5) { 1.0 } else { -1.0 };
                segments.push(Segment {
                    activity,
                    start: t,
                    duration,
                    sign,
                });
                t += duration;
            }
        }
        Self { seed, segments }
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// Index of the segment playing at `t` (the last one past the end).
    pub fn segment_index(&self, t: f64) -> usize {
        self.segments
            .partition_point(|s| s.start <= t)
            .saturating_sub(1)
    }

    /// The intents for the frame covering (t_prev, t], in seconds since the
    /// session started. Edges (presses, selections) fire once, in the frame
    /// whose interval contains their scripted instant.
    pub fn frame(&self, t_prev: f64, t: f64) -> ScriptFrame {
        let index = self.segment_index(t);
        let seg = self.segments[index];
        // A segment's first frame sees its t=0 events.
        let local_prev = |seg: &Segment| {
            if t_prev < seg.start {
                -1.0
            } else {
                t_prev - seg.start
            }
        };
        let mut frame = Self::segment_frame(seg, local_prev(&seg), t - seg.start);
        // Keep edges scripted late in any segment this frame has just left.
        for old in &self.segments[self.segment_index(t_prev)..index] {
            let late = Self::segment_frame(*old, local_prev(old), old.duration);
            frame.jump_pressed |= late.jump_pressed;
            frame.crouch_pressed |= late.crouch_pressed;
            frame.fire_pressed |= late.fire_pressed;
            frame.ads_toggle_pressed |= late.ads_toggle_pressed;
            frame.reload_pressed |= late.reload_pressed;
            frame.select = frame.select.or(late.select);
        }
        frame
    }

    fn segment_frame(seg: Segment, u0: f64, u: f64) -> ScriptFrame {
        let s = seg.sign;
        let deg = |d: f32| d.to_radians();
        match seg.activity {
            Activity::Run => {
                let turn = if u % 2.4 < 1.2 { 1.0 } else { -1.0 };
                let mut f = ScriptFrame::new(
                    Activity::Run,
                    LookGoal::Sweep {
                        yaw_rate: s * turn * deg(70.0),
                        pitch: 0.12 * (1.1 * u as f32).sin(),
                    },
                );
                f.move_axis =
                    Vec2::new(0.6 * s * (1.7 * u as f32).sin(), 1.0).clamp_length_max(1.0);
                f.sprint = within_every(u, 0.4, 2.0, 1.3);
                f
            }
            Activity::Slide => {
                let mut f = ScriptFrame::new(
                    Activity::Slide,
                    LookGoal::Sweep {
                        yaw_rate: s * deg(35.0),
                        pitch: -0.05,
                    },
                );
                f.move_axis = Vec2::Y;
                f.sprint = true;
                f.crouch_pressed = crossed_every(u0, u, 0.5, 1.1);
                f.crouch = within_every(u, 0.5, 1.1, 0.75);
                f
            }
            Activity::Jump => {
                let mut f = ScriptFrame::new(
                    Activity::Jump,
                    LookGoal::Sweep {
                        yaw_rate: -s * deg(50.0),
                        pitch: 0.1 * (2.0 * u as f32).sin(),
                    },
                );
                f.move_axis = Vec2::new(0.7 * s, 1.0).clamp_length_max(1.0);
                f.sprint = true;
                f.jump_pressed = crossed_every(u0, u, 0.2, 0.75);
                f.jump = within_every(u, 0.2, 0.75, 0.2);
                f
            }
            Activity::BoxUp => {
                // Q, hold the trigger and swipe 90° three times (turbo walls on
                // all four sides), E and look down (ramp inside), F and look up
                // (floor on top).
                let turned: f32 = (0..3)
                    .map(|k| ease(u, 0.2 + 0.3 * k as f64, 0.35 + 0.3 * k as f64))
                    .sum();
                let pitch = deg(-60.0) * ease(u, 1.0, 1.1) * (1.0 - ease(u, 1.2, 1.3))
                    + deg(65.0) * ease(u, 1.2, 1.3);
                let mut f = ScriptFrame::new(
                    Activity::BoxUp,
                    LookGoal::Pose {
                        yaw: -s * FRAC_PI_2 * turned,
                        pitch,
                    },
                );
                f.fire = (0.05..1.45).contains(&u);
                f.fire_pressed = crossed(u0, u, 0.05);
                f.select = if crossed(u0, u, 0.0) {
                    Some(ActiveTool::Build(PieceKind::Wall))
                } else if crossed(u0, u, 1.0) {
                    Some(ActiveTool::Build(PieceKind::Ramp))
                } else if crossed(u0, u, 1.2) {
                    Some(ActiveTool::Build(PieceKind::Floor))
                } else {
                    None
                };
                f
            }
            Activity::RampRush => {
                let mut f = ScriptFrame::new(
                    Activity::RampRush,
                    LookGoal::Pose {
                        yaw: s * deg(6.0) * (1.3 * u as f32).sin(),
                        pitch: deg(8.0),
                    },
                );
                f.move_axis = Vec2::Y;
                f.sprint = true;
                f.fire = true;
                f.fire_pressed = crossed(u0, u, 0.0);
                f.jump_pressed = crossed_every(u0, u, 0.3, 0.5);
                f.jump = within_every(u, 0.3, 0.5, 0.15);
                f.select = if crossed(u0, u, 0.0) || crossed_every(u0, u, 0.7, 0.7) {
                    Some(ActiveTool::Build(PieceKind::Ramp))
                } else if crossed_every(u0, u, 0.35, 0.7) {
                    Some(ActiveTool::Build(PieceKind::Wall))
                } else {
                    None
                };
                f
            }
            Activity::Spray => {
                let mut f = ScriptFrame::new(Activity::Spray, LookGoal::Dummy);
                let strafe = if (u / 0.8) as i64 % 2 == 0 { s } else { -s };
                f.move_axis = Vec2::new(strafe, 0.0);
                f.select = crossed(u0, u, 0.0).then_some(RIFLE);
                f.ads_toggle_pressed = crossed(u0, u, 0.25);
                f.fire = u >= 0.3 && u < seg.duration - 0.2;
                f.fire_pressed = crossed(u0, u, 0.3);
                f
            }
            Activity::Pump => {
                let mut f = ScriptFrame::new(Activity::Pump, LookGoal::Dummy);
                f.move_axis = Vec2::new(0.3 * s, 1.0).clamp_length_max(1.0);
                f.select = crossed(u0, u, 0.0).then_some(PUMP);
                f.fire_pressed = crossed_every(u0, u, 0.35, 0.95);
                f.fire = within_every(u, 0.35, 0.95, 0.08);
                f
            }
            Activity::SwitchGuns => {
                let mut f = ScriptFrame::new(Activity::SwitchGuns, LookGoal::Dummy);
                f.move_axis = Vec2::new(s, 0.5).clamp_length_max(1.0);
                f.select = if crossed(u0, u, 0.0) || crossed_every(u0, u, 0.9, 0.9) {
                    Some(RIFLE)
                } else if crossed_every(u0, u, 0.45, 0.9) {
                    Some(PUMP)
                } else {
                    None
                };
                f.fire = u >= 0.1;
                f.fire_pressed = crossed_every(u0, u, 0.25, 0.45);
                f
            }
            Activity::BreakPieces => {
                let mut f = ScriptFrame::new(Activity::BreakPieces, LookGoal::Piece);
                f.move_axis = Vec2::new(0.0, -0.3);
                let half = seg.duration / 2.0;
                f.select = if crossed(u0, u, 0.0) {
                    Some(RIFLE)
                } else if crossed(u0, u, half) {
                    Some(PUMP)
                } else {
                    None
                };
                if u < half {
                    f.fire = u >= 0.25;
                    f.fire_pressed = crossed(u0, u, 0.25);
                } else {
                    f.fire_pressed = crossed_every(u0, u, half + 0.3, 0.95);
                    f.fire = within_every(u, half + 0.3, 0.95, 0.08);
                }
                f
            }
        }
    }
}

/// Small constant view sway (radians) layered on every look goal.
pub fn sway(t: f64) -> Vec2 {
    let t = t as f32;
    Vec2::new(0.03 * (TAU * 0.7 * t).sin(), 0.02 * (TAU * 0.53 * t).sin())
}

fn wrap_angle(a: f32) -> f32 {
    (a + PI).rem_euclid(TAU) - PI
}

// ---------------------------------------------------------------------------
// The director
// ---------------------------------------------------------------------------

/// Load counters (player's shots from [`CombatStats`], pieces from messages).
#[derive(Debug, Clone, Copy, Default)]
struct Load {
    shots: u32,
    hits: u32,
    headshots: u32,
    eliminations: u32,
    placed: u32,
    destroyed: u32,
    cleanup_breaks: u32,
}

impl Load {
    fn minus(self, earlier: Load) -> Load {
        Load {
            shots: self.shots.saturating_sub(earlier.shots),
            hits: self.hits.saturating_sub(earlier.hits),
            headshots: self.headshots.saturating_sub(earlier.headshots),
            eliminations: self.eliminations.saturating_sub(earlier.eliminations),
            placed: self.placed.saturating_sub(earlier.placed),
            destroyed: self.destroyed.saturating_sub(earlier.destroyed),
            cleanup_breaks: self.cleanup_breaks.saturating_sub(earlier.cleanup_breaks),
        }
    }

    fn json(&self) -> Value {
        json!({
            "shots": self.shots,
            "hits": self.hits,
            "headshots": self.headshots,
            "eliminations": self.eliminations,
            "pieces_placed": self.placed,
            "pieces_destroyed": self.destroyed,
            "pieces_broken_by_cleanup": self.cleanup_breaks,
        })
    }
}

struct Perf {
    seed: u64,
    script: Option<PerfScript>,
    warmup: f64,
    seconds: f64,
    t_prev: f64,
    segment: Option<usize>,
    base_yaw: f32,
    pieces: Option<MessageCursor<PieceChanged>>,
    /// Player-built pieces, oldest first.
    live: VecDeque<Entity>,
    max_live: usize,
    placed: u32,
    destroyed: u32,
    cleanup_breaks: u32,
    at_warmup: Option<Load>,
    at_end: Option<Load>,
    activity_seconds: [f64; 9],
    timed_end_ms: Option<f64>,
    load_start: Option<[f64; 3]>,
}

impl Perf {
    fn new(seed: u64) -> Self {
        Self {
            seed,
            script: None,
            warmup: WARMUP_SECONDS,
            seconds: DEFAULT_SECONDS,
            t_prev: 0.0,
            segment: None,
            base_yaw: 0.0,
            pieces: None,
            live: VecDeque::new(),
            max_live: 0,
            placed: 0,
            destroyed: 0,
            cleanup_breaks: 0,
            at_warmup: None,
            at_end: None,
            activity_seconds: [0.0; 9],
            timed_end_ms: None,
            load_start: None,
        }
    }

    fn load(&self, world: &World) -> Load {
        let stats = world.resource::<CombatStats>();
        Load {
            shots: stats.shots,
            hits: stats.hits,
            headshots: stats.headshots,
            eliminations: stats.eliminations,
            placed: self.placed,
            destroyed: self.destroyed,
            cleanup_breaks: self.cleanup_breaks,
        }
    }

    fn track_pieces(&mut self, world: &mut World) {
        let cursor = self
            .pieces
            .get_or_insert_with(|| world.resource::<Messages<PieceChanged>>().get_cursor());
        for change in cursor.read(world.resource::<Messages<PieceChanged>>()) {
            match change.change {
                PieceChange::Placed => {
                    self.placed += 1;
                    self.live.push_back(change.entity);
                }
                PieceChange::Destroyed => self.destroyed += 1,
                PieceChange::Cracked(_) => {}
            }
        }
        self.live.retain(|e| world.get::<Piece>(*e).is_some());
        self.max_live = self.max_live.max(self.live.len());
        // Break the oldest pieces so the arena never fills up. They break
        // through the normal damage path (next fixed tick), debris and all.
        if self.live.len() > MAX_LIVE_PIECES {
            while self.live.len() > KEEP_LIVE_PIECES {
                let Some(old) = self.live.pop_front() else {
                    break;
                };
                crate::building::damage_piece(world, old, 1.0e6);
                self.cleanup_breaks += 1;
            }
        }
    }

    /// Where the view should turn this frame, as a (yaw, pitch) delta.
    fn look_delta(&mut self, world: &mut World, goal: LookGoal, t: f64, dt: f32) -> Vec2 {
        let Some((feet, eye, look)) = world
            .query_filtered::<(&Transform, &EyeHeight, &LookAngles), With<Player>>()
            .iter(world)
            .next()
            .map(|(t, e, l)| (t.translation, t.translation + Vec3::Y * e.0, *l))
        else {
            return Vec2::ZERO;
        };
        let k = 1.0 - (-dt * 14.0).exp();
        let track = |target: Vec3| {
            let want = look_toward(target - eye);
            Vec2::new(
                wrap_angle(want.yaw - look.yaw) * k,
                (want.pitch - look.pitch) * k,
            )
        };
        let idle = Vec2::new(60f32.to_radians() * dt, -look.pitch * k);
        let dummy = dummy_chest(world);
        let base = match goal {
            LookGoal::Sweep { yaw_rate, pitch } => Vec2::new(
                yaw_rate * dt,
                (pitch - look.pitch) * (1.0 - (-dt * 6.0).exp()),
            ),
            LookGoal::Pose { yaw, pitch } => Vec2::new(
                wrap_angle(self.base_yaw + yaw - look.yaw),
                pitch - look.pitch,
            ),
            LookGoal::Dummy => dummy.map_or(idle, track),
            LookGoal::Piece => {
                let mut q =
                    world.query_filtered::<&Transform, (With<Piece>, Without<InitialCover>)>();
                let nearest = q.iter(world).map(|t| t.translation).min_by(|a, b| {
                    a.distance_squared(feet)
                        .total_cmp(&b.distance_squared(feet))
                });
                nearest.or(dummy).map_or(idle, track)
            }
        };
        base + sway(t) - sway(self.t_prev)
    }

    /// Steers the move axis back toward the middle near the arena edge, and
    /// stops short of the dummy when pumping.
    fn steer(world: &mut World, activity: Activity, axis: Vec2) -> Vec2 {
        if axis == Vec2::ZERO {
            return axis;
        }
        let Some((feet, look)) = world
            .query_filtered::<(&Transform, &LookAngles), With<Player>>()
            .iter(world)
            .next()
            .map(|(t, l)| (t.translation, *l))
        else {
            return axis;
        };
        if activity == Activity::Pump
            && dummy_chest(world).is_some_and(|d| d.xz().distance(feet.xz()) < 4.0)
        {
            return Vec2::ZERO;
        }
        let edge = ARENA_HALF - 7.0;
        if feet.x.abs() > edge || feet.z.abs() > edge {
            let home = (-feet).xz().normalize_or_zero();
            let (forward, right) = look.flat_basis();
            return Vec2::new(home.dot(right.xz()), home.dot(forward.xz())).normalize_or_zero();
        }
        axis
    }
}

/// The standing dummy's chest, unless it is down.
fn dummy_chest(world: &mut World) -> Option<Vec3> {
    let mut q = world.query_filtered::<&Transform, (With<Dummy>, Without<Downed>)>();
    q.iter(world).next().map(|t| t.translation + Vec3::Y * 1.1)
}

impl Director for Perf {
    fn update(&mut self, world: &mut World, clock: &ScenarioClock) -> DirectorStatus {
        if self.script.is_none() {
            self.seconds = clock.requested_seconds.unwrap_or(DEFAULT_SECONDS);
            self.warmup = world
                .get_resource::<ScenarioRun>()
                .and_then(|run| run.warmup_override)
                .unwrap_or(WARMUP_SECONDS);
            self.script = Some(PerfScript::new(self.seed, self.warmup + self.seconds));
            if world.contains_resource::<ScenarioRun>() {
                self.load_start = telemetry::load_average();
            }
        }
        let t = clock.seconds;
        let dt = (t - self.t_prev).max(0.0) as f32;
        self.track_pieces(world);

        if self.at_warmup.is_none() && t >= self.warmup {
            self.at_warmup = Some(self.load(world));
        }
        if t >= self.warmup + self.seconds {
            self.timed_end_ms = Some(telemetry::now_ms());
            self.at_end = Some(self.load(world));
            with_intent(world, |i| {
                i.move_axis = Vec2::ZERO;
                i.fire = false;
                i.sprint = false;
                i.crouch = false;
                i.jump = false;
            });
            if world.contains_resource::<ScenarioRun>() {
                capture(world, "perf-end", false);
            }
            return DirectorStatus::Done;
        }

        let Some(script) = self.script.as_ref() else {
            return DirectorStatus::Done;
        };
        let frame = script.frame(self.t_prev, t);
        let index = script.segment_index(t);
        if self.segment != Some(index) {
            self.segment = Some(index);
            self.base_yaw = world
                .query_filtered::<&LookAngles, With<Player>>()
                .iter(world)
                .next()
                .map_or(0.0, |l| l.yaw);
        }
        if t >= self.warmup {
            self.activity_seconds[frame.activity.index()] += dt as f64;
        }

        let look = self.look_delta(world, frame.look, t, dt);
        let axis = Self::steer(world, frame.activity, frame.move_axis);
        with_intent(world, |i| {
            i.move_axis = axis;
            i.sprint = frame.sprint;
            i.jump = frame.jump;
            i.jump_pressed |= frame.jump_pressed;
            i.crouch = frame.crouch;
            i.crouch_pressed |= frame.crouch_pressed;
            i.fire = frame.fire;
            i.fire_pressed |= frame.fire_pressed;
            i.ads_toggle_pressed |= frame.ads_toggle_pressed;
            i.reload_pressed |= frame.reload_pressed;
            if frame.select.is_some() {
                i.select = frame.select;
            }
            i.look_delta += look;
        });
        self.t_prev = t;
        DirectorStatus::Running
    }

    fn warmup_seconds(&self) -> f64 {
        self.warmup
    }

    fn summary(&mut self, world: &mut World) -> Value {
        let window = world
            .get_resource::<FrameLog>()
            .map_or_else(Vec::new, |log| {
                telemetry::intervals_in_window(&log.rows, self.warmup * 1000.0, self.timed_end_ms)
            });
        let frames = telemetry::summarize(&window);
        let gate = telemetry::evaluate_g2(&frames);
        let end = self.at_end.unwrap_or_else(|| self.load(world));
        let start = self.at_warmup.unwrap_or_default();
        let activities: serde_json::Map<String, Value> = Activity::ALL
            .iter()
            .map(|a| {
                (
                    a.name().to_string(),
                    json!(self.activity_seconds[a.index()]),
                )
            })
            .collect();
        let tuning = world.resource::<Tuning>();
        json!({
            "measures": "Frame pacing of sustained scripted heavy play (G2). Frame intervals are app wall-clock time between successive frame ends (main schedule `Last`), after the warm-up and up to the end of the timed window; the final screenshot and exit frames are excluded.",
            "seed": self.seed,
            "warmup_seconds": self.warmup,
            "timed_seconds": self.seconds,
            "dummy_stand_still": tuning.dummy.stand_still,
            "frames": frames,
            "load_timed_window": end.minus(start).json(),
            "load_total": end.json(),
            "activity_seconds_timed_window": activities,
            "max_live_player_pieces": self.max_live,
            "piece_cleanup_rule": format!("the oldest player-built pieces are broken when more than {MAX_LIVE_PIECES} stand, down to {KEEP_LIVE_PIECES}"),
            "g2_thresholds": {
                "mean_interval_ms_min": telemetry::g2::MEAN_MIN_MS,
                "mean_interval_ms_max": telemetry::g2::MEAN_MAX_MS,
                "hitch_ms": telemetry::g2::HITCH_MS,
                "max_frames_over_hitch": telemetry::g2::MAX_HITCHES,
                "fast_frame_ms": telemetry::g2::FAST_MS,
                "min_pct_frames_under_fast": telemetry::g2::MIN_PCT_FAST,
            },
            "gate": gate,
            "run_conditions": telemetry::run_conditions_json(world, self.load_start),
            "gate_note": "Counts as the G2 gate only when power_start and power_end show battery power with Low Power Mode on, with the Battery preset, the window visible (no occluded time), no other heavy process running, a release build and a 300 s timed window.",
        })
    }
}
