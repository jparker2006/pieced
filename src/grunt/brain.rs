//! The grunt's brain (docs/M3-SPEC.md → The grunt, items 1–7, and Fairness).
//!
//! Three loops, all in [`SimSet::Control`] (chained in this order):
//! - **perception** at `perception_hz`: line of sight from the grunt's eye to
//!   the player's head and chest (rays against [`Layer::World`] and
//!   [`Layer::Piece`]), when it was gained, and the piece blocking it;
//! - **decisions** at `decision_hz`, staggered so at most
//!   [`DECISIONS_PER_TICK`] grunts decide (and so re-plan) in one tick: pick
//!   or keep a standing spot, plan an A* path to it, and choose a mode;
//! - **attack tokens**, then **movement, aim and fire** every fixed tick.
//!
//! Grunts always know where the player is (horde rules, D73), but they only
//! hold `fire` with line of sight to the player, or at the piece blocking it.
//! They write only their own [`PlayerIntent`] and [`LookAngles`], and never
//! touch `select` or `edit_pressed`: they never build or edit.

use super::{
    Grunt, GruntStats, Parked,
    aim::{History, ShotNoise, lead_point},
    nav::{NavGrid, NavNode, PropCircle, Surface, Waypoint, waypoints},
    spots::{SPREAD_HARD, SpotCandidate, SpotParams, ring_candidates, score_spot},
};
use crate::{
    arena::ArenaLayout,
    building::{Piece, PieceMap},
    combat::Downed,
    dummy::look_toward,
    movement::Motor,
    orb::Wand,
    rng::{Rng, SimRng},
    shared::{
        EyeHeight, GalleryFreeze, GameCue, Layer, LookAngles, PieceChange, PieceChanged, PieceKind,
        Player, PlayerIntent, SimTick, TICK_SECONDS,
    },
    tuning::Tuning,
};
use avian3d::prelude::*;
use bevy::prelude::*;

// ---------------------------------------------------------------------------
// Knobs that aren't spec numbers
// ---------------------------------------------------------------------------

/// At most this many grunts decide (and so run A*) in one fixed tick.
pub const DECISIONS_PER_TICK: u32 = 2;
/// A* stops after this many node expansions.
pub const MAX_EXPANSIONS: u32 = 2000;
/// Within this of its spot a grunt has arrived (m).
pub const ARRIVE_RADIUS: f32 = 1.5;
/// A grunt strafes this far either side of its spot (m).
pub const STRAFE_RADIUS: f32 = 1.2;
/// A moving grunt must cover this much ground (m)…
pub const STUCK_DISTANCE: f32 = 0.5;
/// …in this long (s), or it re-plans and a stuck event is counted.
pub const STUCK_SECONDS: f32 = 3.0;
/// Ticks of pushing on the ground without moving before a grunt hops.
pub const BLOCKED_TICKS: u8 = 12;
/// Yaw and pitch turn rates (rad/s): quick, but a turn still reads.
const YAW_RATE: f32 = 12.0;
const PITCH_RATE: f32 = 6.0;
/// Seconds without line of sight before a blocking piece is worth shooting.
const SHOOT_PIECE_AFTER: f32 = 0.5;
/// Seconds without line of sight before a grunt looks for a better spot.
const REPICK_NO_LOS: f32 = 1.2;
/// A grunt reconsiders its spot at least this often (s).
const REPICK_INTERVAL: f32 = 4.0;
/// A failed spot is avoided for this long (s).
const BLACKLIST_SECONDS: f32 = 3.0;
/// Seconds before retrying a shot whose target vanished mid wind-up.
const CANCEL_RETRY: f32 = 0.2;
/// Grunts closer than this push apart while walking (m).
const SEPARATION: f32 = 2.2;
/// How long the player must stand up on the build before grunts follow (s).
const ELEVATED_AFTER: f32 = 0.5;
/// Salt for the grunts' RNG stream.
const GRUNT_SALT: u64 = 0x6_2A47;

fn ticks(seconds: f32) -> u64 {
    (seconds / TICK_SECONDS).round().max(0.0) as u64
}

// ---------------------------------------------------------------------------
// Components and resources
// ---------------------------------------------------------------------------

/// What a grunt is doing, re-chosen by utility at `decision_hz`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum GruntMode {
    /// Walking toward its spot from far away.
    #[default]
    Approach,
    /// At its spot: strafing around it, shooting when it can.
    Strafe,
    /// Walking to a new spot (spread out, or to regain line of sight).
    Reposition,
    /// At its spot with no line of sight: shooting the piece in the way.
    ShootPiece,
}

impl GruntMode {
    pub fn is_travel(self) -> bool {
        matches!(self, Self::Approach | Self::Reposition)
    }
}

/// What a shot is aimed at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShotTarget {
    Player,
    /// The first piece on the eye→player line, and where the line meets it.
    Piece {
        entity: Entity,
        point: Vec3,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Shot {
    target: ShotTarget,
    noise: ShotNoise,
    start_tick: u64,
    saw_windup: bool,
}

/// A standing spot and the node it's on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    pub pos: Vec3,
    pub node: NavNode,
    /// On the player's level, up on the build.
    pub elevated: bool,
}

/// One grunt's controller state. Seeded from [`SimRng`] when it's added;
/// [`GruntBrain::reset`] clears everything but the RNG (for pooled grunts).
#[derive(Component, Debug, Clone)]
pub struct GruntBrain {
    rng: Rng,
    phase: u32,
    mode: GruntMode,
    spot: Option<Spot>,
    spot_tick: u64,
    path_nodes: Vec<NavNode>,
    path: Vec<Waypoint>,
    path_index: usize,
    needs_plan: bool,
    blacklist: Vec<(Vec3, u64)>,
    // Perception.
    los: bool,
    los_gained: Option<u64>,
    los_lost: u64,
    head_only: bool,
    blocker: Option<(Entity, Vec3)>,
    // Decisions.
    next_decision: u64,
    // Strafing.
    strafe_dir: f32,
    strafe_turn: f32,
    // Firing.
    shot: Option<Shot>,
    next_shot: u64,
    token: bool,
    // Stuck detection.
    anchor: Option<(Vec3, u64)>,
    jump_hold: u8,
    /// Ticks spent pushing forward on the ground without moving (a lip at a
    /// ramp's top edge): a short hop gets over it, as a player would.
    blocked: u8,
}

impl Default for GruntBrain {
    fn default() -> Self {
        Self::new(Rng::new(0))
    }
}

impl GruntBrain {
    pub fn new(rng: Rng) -> Self {
        Self {
            rng,
            phase: 0,
            mode: GruntMode::Approach,
            spot: None,
            spot_tick: 0,
            path_nodes: Vec::new(),
            path: Vec::new(),
            path_index: 0,
            needs_plan: true,
            blacklist: Vec::new(),
            los: false,
            los_gained: None,
            los_lost: 0,
            head_only: false,
            blocker: None,
            next_decision: 0,
            strafe_dir: 1.0,
            strafe_turn: 0.8,
            shot: None,
            next_shot: 0,
            token: false,
            anchor: None,
            jump_hold: 0,
            blocked: 0,
        }
    }

    /// Forgets everything but its RNG and phase, as if newly spawned. Slice C
    /// calls this when it reactivates a pooled grunt.
    pub fn reset(&mut self) {
        let rng = self.rng.clone();
        let phase = self.phase;
        *self = Self::new(rng);
        self.phase = phase;
    }

    /// Replaces its RNG stream (the wave director seeds each activation from
    /// the run's seed, so the same seed replays the same run).
    pub fn reseed(&mut self, rng: Rng) {
        self.rng = rng;
    }

    pub fn mode(&self) -> GruntMode {
        self.mode
    }

    pub fn spot(&self) -> Option<Spot> {
        self.spot
    }

    /// Line of sight to the player, as last perceived.
    pub fn has_los(&self) -> bool {
        self.los
    }

    /// The piece blocking the line to the player, as last perceived.
    pub fn blocker(&self) -> Option<Entity> {
        self.blocker.map(|b| b.0)
    }

    /// The current shot's target, while holding fire.
    pub fn shot_target(&self) -> Option<ShotTarget> {
        self.shot.map(|s| s.target)
    }

    /// Walking toward a spot it hasn't reached.
    pub fn is_travelling(&self) -> bool {
        self.mode.is_travel() && self.spot.is_some()
    }

    /// The planned waypoints left to walk.
    pub fn remaining_path(&self) -> &[Waypoint] {
        &self.path[self.path_index.min(self.path.len())..]
    }

    fn clear_path(&mut self) {
        self.path.clear();
        self.path_nodes.clear();
        self.path_index = 0;
    }

    fn blacklisted(&self, pos: Vec3) -> bool {
        self.blacklist.iter().any(|(p, _)| p.distance(pos) < 1.5)
    }
}

/// The attack tokens (D76): at most `GruntTuning::max_shooters` grunts hold
/// one, from their fire press until their wand's wind-up ends.
#[derive(Resource, Debug, Default, Clone)]
pub struct AttackTokens {
    holders: Vec<Entity>,
}

impl AttackTokens {
    pub fn holders(&self) -> &[Entity] {
        &self.holders
    }

    pub fn clear(&mut self) {
        self.holders.clear();
    }
}

/// Navigation bookkeeping, for tests and tuning.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct GruntNavStats {
    /// Grunts that failed to make 0.5 m of progress in 3 s toward a goal.
    pub stuck_events: u32,
    pub plans: u64,
    pub failed_plans: u64,
    pub max_expansions: u32,
    pub decisions_this_tick: u32,
    pub max_decisions_per_tick: u32,
}

/// What every grunt knows about the player (horde rules: always its true position).
#[derive(Resource, Debug, Default, Clone)]
pub struct PlayerTrack {
    pub entity: Option<Entity>,
    pub alive: bool,
    pub feet: Vec3,
    pub head: Vec3,
    pub chest: Vec3,
    /// The level the grunts treat the player as standing on (0 = the ground),
    /// once they've stood there for `ELEVATED_AFTER`.
    pub level: i32,
    level_candidate: (i32, u64),
    history: History,
}

/// The grunts' RNG stream, forked from [`SimRng`] at the first spawn. Slice C
/// resets it (`GruntRng::default()`) with a new run's seed.
#[derive(Resource, Debug, Default, Clone)]
pub struct GruntRng {
    rng: Option<Rng>,
    spawned: u32,
}

/// The arena props as circles, for navigation.
#[derive(Resource, Debug, Clone)]
pub struct GruntProps(pub Vec<PropCircle>);

impl Default for GruntProps {
    fn default() -> Self {
        Self(super::nav::arena_prop_circles())
    }
}

// ---------------------------------------------------------------------------
// Pure decision core
// ---------------------------------------------------------------------------

/// What a decision looks at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModeInputs {
    /// Distance to the spot, if it has one.
    pub dist_to_spot: Option<f32>,
    pub los: bool,
    pub no_los_secs: f32,
    /// A piece blocks the line to the player.
    pub blocker: bool,
    /// The player is beyond the range band.
    pub player_far: bool,
}

/// Bonus the current mode keeps, so a grunt commits instead of dithering.
pub const STICKINESS: f32 = 0.15;

/// Utility-scored mode choice with a stickiness bonus.
pub fn choose_mode(i: &ModeInputs, current: GruntMode) -> GruntMode {
    let near = i.dist_to_spot.is_none_or(|d| d <= 3.0);
    let travel = match i.dist_to_spot {
        Some(d) if d > ARRIVE_RADIUS => 0.7 + 0.3 * (d / 8.0).min(1.0),
        _ => 0.0,
    };
    let strafe = match (near, i.los) {
        (false, _) => 0.0,
        (true, true) => 0.9,
        (true, false) => 0.4,
    };
    let shoot_piece = if near && i.blocker && !i.los && i.no_los_secs >= SHOOT_PIECE_AFTER {
        0.6 + 0.35 * ((i.no_los_secs - SHOOT_PIECE_AFTER) / 1.0).min(1.0)
    } else {
        0.0
    };
    let travel_mode = if i.player_far {
        GruntMode::Approach
    } else {
        GruntMode::Reposition
    };
    let sticky = |mode: GruntMode, score: f32| {
        let same = mode == current || (mode.is_travel() && current.is_travel());
        if same && score > 0.0 {
            score + STICKINESS
        } else {
            score
        }
    };
    [
        (GruntMode::Strafe, sticky(GruntMode::Strafe, strafe)),
        (travel_mode, sticky(travel_mode, travel)),
        (
            GruntMode::ShootPiece,
            sticky(GruntMode::ShootPiece, shoot_piece),
        ),
    ]
    .into_iter()
    .fold((GruntMode::Strafe, f32::MIN), |best, (m, s)| {
        if s > best.1 { (m, s) } else { best }
    })
    .0
}

/// Turns `from` toward `to` by at most the given yaw and pitch steps.
pub fn turn_toward(from: LookAngles, to: LookAngles, max_yaw: f32, max_pitch: f32) -> LookAngles {
    use std::f32::consts::{PI, TAU};
    let dy = (to.yaw - from.yaw + PI).rem_euclid(TAU) - PI;
    LookAngles {
        yaw: (from.yaw + dy.clamp(-max_yaw, max_yaw)).rem_euclid(TAU),
        pitch: from.pitch + (to.pitch - from.pitch).clamp(-max_pitch, max_pitch),
    }
}

/// `PlayerIntent::move_axis` and `sprint` that move at `speed` m/s along the
/// world direction `wish` (length ≤ 1 scales it) for a character looking
/// along `look`. Movement runs at `run` m/s, or `sprint` m/s only when
/// sprinting while moving forward.
pub fn speed_axis(
    wish: Vec3,
    look: &LookAngles,
    speed: f32,
    run: f32,
    sprint: f32,
) -> (Vec2, bool) {
    let len = wish.xz().length().min(1.0);
    if len < 1e-3 || speed <= 0.0 {
        return (Vec2::ZERO, false);
    }
    let (fwd, right) = look.flat_basis();
    let dir = Vec2::new(wish.dot(right), wish.dot(fwd)).normalize_or_zero();
    let target = speed * len;
    if target > run && dir.y > 0.3 && sprint > 0.0 {
        (dir * (target / sprint).min(1.0), true)
    } else {
        (dir * (target / run.max(0.1)).min(1.0), false)
    }
}

// ---------------------------------------------------------------------------
// Line of sight
// ---------------------------------------------------------------------------

/// What a grunt's eye sees of the player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sight {
    /// The head or the chest is in view.
    pub visible: bool,
    /// Only the head is.
    pub head_only: bool,
    /// The first piece on the line to the chest (or the head), when hidden.
    pub blocker: Option<(Entity, Vec3)>,
}

fn ray_hit(spatial: &SpatialQuery, from: Vec3, to: Vec3) -> Option<(Entity, Vec3)> {
    let filter = SpatialQueryFilter::from_mask([Layer::World, Layer::Piece]);
    let d = to - from;
    let len = d.length();
    let dir = Dir3::new(d).ok()?;
    spatial
        .cast_ray(from, dir, (len - 0.05).max(0.0), true, &filter)
        .map(|h| (h.entity, from + *dir * h.distance))
}

/// Line of sight from `eye` to the player's `head` and `chest`.
pub fn sight(
    spatial: &SpatialQuery,
    eye: Vec3,
    head: Vec3,
    chest: Vec3,
    is_piece: impl Fn(Entity) -> bool,
) -> Sight {
    let chest_hit = ray_hit(spatial, eye, chest);
    let head_hit = ray_hit(spatial, eye, head);
    let visible = chest_hit.is_none() || head_hit.is_none();
    let blocker = if visible {
        None
    } else {
        chest_hit
            .filter(|h| is_piece(h.0))
            .or(head_hit.filter(|h| is_piece(h.0)))
    };
    Sight {
        visible,
        head_only: chest_hit.is_some() && head_hit.is_none(),
        blocker,
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

pub(super) fn seed_brain(
    add: On<Add, GruntBrain>,
    sim_rng: Res<SimRng>,
    mut grunt_rng: ResMut<GruntRng>,
    mut brains: Query<&mut GruntBrain>,
) {
    let Ok(mut brain) = brains.get_mut(add.entity) else {
        return;
    };
    let state = &mut *grunt_rng;
    let stream = state
        .rng
        .get_or_insert_with(|| sim_rng.0.clone().fork(GRUNT_SALT));
    brain.rng = stream.fork(state.spawned as u64);
    brain.phase = state.spawned;
    brain.strafe_dir = if brain.rng.chance(0.5) { 1.0 } else { -1.0 };
    brain.strafe_turn = brain.rng.range(0.4, 1.6);
    state.spawned = state.spawned.wrapping_add(1);
}

type ActiveGrunt = (With<Grunt>, Without<Parked>, Without<Downed>);

pub(super) fn track_player(
    tick: Res<SimTick>,
    map: Res<PieceMap>,
    props: Res<GruntProps>,
    mut track: ResMut<PlayerTrack>,
    players: Query<
        (Entity, &Transform, &EyeHeight, Option<&Motor>, Has<Downed>),
        (With<Player>, Without<Grunt>),
    >,
) {
    let Some((entity, transform, eye, motor, downed)) = players.iter().next() else {
        track.entity = None;
        track.alive = false;
        return;
    };
    let feet = transform.translation;
    track.entity = Some(entity);
    track.alive = !downed;
    track.feet = feet;
    track.head = feet + Vec3::Y * eye.0;
    track.chest = feet + Vec3::Y * (eye.0 * 0.7);
    let chest = track.chest;
    track.history.push(chest);
    if motor.is_none_or(|m| m.grounded) {
        let nav = NavGrid::new(&map, &props.0);
        let level = nav.localize(feet).map_or(0, |n| n.standing_level(feet));
        if level != track.level_candidate.0 {
            track.level_candidate = (level, tick.0);
        }
    }
    if tick.0.saturating_sub(track.level_candidate.1) >= ticks(ELEVATED_AFTER) {
        track.level = track.level_candidate.0;
    }
}

/// Drops any path a placed, broken or edited piece has cut.
pub(super) fn watch_pieces(
    map: Res<PieceMap>,
    props: Res<GruntProps>,
    mut changed: MessageReader<PieceChanged>,
    mut cues: MessageReader<GameCue>,
    mut grunts: Query<&mut GruntBrain, ActiveGrunt>,
) {
    let structural = changed
        .read()
        .any(|c| matches!(c.change, PieceChange::Placed | PieceChange::Destroyed));
    let edited = cues
        .read()
        .any(|c| matches!(c, GameCue::PieceEdited { .. }));
    if !(structural || edited) {
        return;
    }
    let nav = NavGrid::new(&map, &props.0);
    for mut brain in &mut grunts {
        if brain.path.is_empty() {
            continue;
        }
        let from = brain
            .path_index
            .min(brain.path_nodes.len().saturating_sub(1));
        if !nav.path_valid(&brain.path_nodes[from..]) {
            brain.needs_plan = true;
        }
    }
}

pub(super) fn perceive(
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    spatial: SpatialQuery,
    track: Res<PlayerTrack>,
    pieces: Query<(), With<Piece>>,
    mut grunts: Query<(&Transform, &EyeHeight, &mut GruntBrain), ActiveGrunt>,
) {
    let interval = ticks(1.0 / tuning.grunt.perception_hz.max(1.0)).max(1);
    for (transform, eye, mut brain) in &mut grunts {
        if !(tick.0 + brain.phase as u64).is_multiple_of(interval) {
            continue;
        }
        let seen = if track.alive {
            let from = transform.translation + Vec3::Y * eye.0;
            sight(&spatial, from, track.head, track.chest, |e| {
                pieces.contains(e)
            })
        } else {
            Sight {
                visible: false,
                head_only: false,
                blocker: None,
            }
        };
        if seen.visible && !brain.los {
            brain.los_gained = Some(tick.0);
        } else if !seen.visible && brain.los {
            brain.los_lost = tick.0;
        }
        if !seen.visible {
            brain.los_gained = None;
        }
        brain.los = seen.visible;
        brain.head_only = seen.head_only;
        brain.blocker = seen.blocker;
    }
}

/// Everything a decision reads.
struct DecisionWorld<'a, 'w, 's> {
    tick: u64,
    tuning: &'a Tuning,
    layout: &'a ArenaLayout,
    nav: NavGrid<'a>,
    track: &'a PlayerTrack,
    spatial: &'a SpatialQuery<'w, 's>,
    is_piece: &'a dyn Fn(Entity) -> bool,
}

pub(super) fn decide(
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    layout: Res<ArenaLayout>,
    map: Res<PieceMap>,
    props: Res<GruntProps>,
    track: Res<PlayerTrack>,
    spatial: SpatialQuery,
    pieces: Query<(), With<Piece>>,
    mut stats: ResMut<GruntNavStats>,
    mut grunts: Query<(Entity, &Transform, &mut GruntBrain), ActiveGrunt>,
) {
    stats.decisions_this_tick = 0;
    if track.entity.is_none() {
        return;
    }
    let tick = tick.0;
    // Who decides this tick: grunts needing a path first, then the most overdue.
    let mut due: Vec<(bool, u64, Entity)> = grunts
        .iter()
        .filter(|(_, _, b)| b.needs_plan || b.next_decision <= tick)
        .map(|(e, _, b)| (!b.needs_plan, b.next_decision, e))
        .collect();
    if due.is_empty() {
        return;
    }
    due.sort();
    let mut others: Vec<(Entity, Vec3)> = grunts
        .iter()
        .map(|(e, t, b)| (e, b.spot.map_or(t.translation, |s| s.pos)))
        .collect();
    let is_piece = |e: Entity| pieces.contains(e);
    let world = DecisionWorld {
        tick,
        tuning: &tuning,
        layout: &layout,
        nav: NavGrid::new(&map, &props.0),
        track: &track,
        spatial: &spatial,
        is_piece: &is_piece,
    };
    for &(_, _, entity) in due.iter().take(DECISIONS_PER_TICK as usize) {
        let Ok((_, transform, mut brain)) = grunts.get_mut(entity) else {
            continue;
        };
        let feet = transform.translation;
        let rivals: Vec<Vec3> = others
            .iter()
            .filter(|(e, _)| *e != entity)
            .map(|(_, p)| *p)
            .collect();
        let crowded_by_elder = brain.spot.is_some_and(|s| {
            others
                .iter()
                .any(|(e, p)| *e < entity && p.xz().distance(s.pos.xz()) < SPREAD_HARD - 0.5)
        });
        decide_one(
            &world,
            &mut brain,
            feet,
            &rivals,
            crowded_by_elder,
            &mut stats,
        );
        if let Some(slot) = others.iter_mut().find(|(e, _)| *e == entity) {
            slot.1 = brain.spot.map_or(feet, |s| s.pos);
        }
        stats.decisions_this_tick += 1;
    }
    stats.max_decisions_per_tick = stats.max_decisions_per_tick.max(stats.decisions_this_tick);
}

fn decide_one(
    w: &DecisionWorld,
    brain: &mut GruntBrain,
    feet: Vec3,
    rivals: &[Vec3],
    crowded_by_elder: bool,
    stats: &mut GruntNavStats,
) {
    let tick = w.tick;
    let t = &w.tuning.grunt;
    let player = w.track.feet;
    brain.next_decision = tick + ticks(1.0 / t.decision_hz.max(0.5)).max(1);
    brain.blacklist.retain(|(_, until)| *until > tick);

    // 1. Keep the spot, or pick a better one.
    let no_los_secs = if brain.los {
        0.0
    } else {
        tick.saturating_sub(brain.los_lost) as f32 * TICK_SECONDS
    };
    let repick = match brain.spot {
        None => true,
        Some(spot) => {
            let d = spot.pos.xz().distance(player.xz());
            let out_of_band = if spot.elevated {
                w.track.level == 0
                    || spot.node.standing_level(spot.pos) != w.track.level
                    || d > t.range_max + 2.0
                    || d < 2.0
            } else {
                d < t.range_min - 2.0 || d > t.range_max + 2.0
            };
            let since = tick.saturating_sub(brain.spot_tick) as f32 * TICK_SECONDS;
            out_of_band
                || crowded_by_elder
                || (!brain.los && no_los_secs > REPICK_NO_LOS && since > 1.5)
                || since > REPICK_INTERVAL
                || (w.track.level > 0 && !spot.elevated && since > 1.0)
        }
    };
    if repick {
        let new = pick_spot(w, brain, feet, rivals);
        brain.spot_tick = tick;
        let moved = match (brain.spot, new) {
            (Some(a), Some(b)) => a.pos.distance(b.pos) > 0.5,
            (None, None) => false,
            _ => true,
        };
        if moved {
            brain.spot = new;
            brain.clear_path();
            brain.needs_plan = new.is_some();
        }
    }

    // 2. Plan (or re-plan) the path to it.
    if !brain.needs_plan && !brain.path.is_empty() {
        let from = brain
            .path_index
            .min(brain.path_nodes.len().saturating_sub(1));
        let valid = w.nav.path_valid(&brain.path_nodes[from..]);
        let here = w.nav.localize(feet);
        let expected = &brain.path_nodes[from..(from + 2).min(brain.path_nodes.len())];
        let on_track = here.is_some_and(|h| {
            expected.contains(&h) || (h.is_ground() && expected.iter().all(NavNode::is_ground))
        });
        if !valid || !on_track {
            brain.needs_plan = true;
        }
    }
    if brain.needs_plan
        && let Some(spot) = brain.spot
    {
        brain.needs_plan = false;
        let start = w.nav.localize(feet);
        let plan = start.map(|s| w.nav.plan(s, feet, spot.node, MAX_EXPANSIONS));
        stats.plans += 1;
        match plan.as_ref().and_then(|p| p.path.as_ref()) {
            Some(path) => {
                brain.path = waypoints(path, spot.pos);
                brain.path_nodes = path.clone();
                brain.path_index = 0;
                brain.anchor = None;
            }
            None => {
                stats.failed_plans += 1;
                brain
                    .blacklist
                    .push((spot.pos, tick + ticks(BLACKLIST_SECONDS)));
                brain.spot = None;
                brain.clear_path();
                // Try another spot soon, without hogging the per-tick budget.
                brain.next_decision = tick + 4;
            }
        }
        if let Some(p) = plan {
            stats.max_expansions = stats.max_expansions.max(p.expansions);
        }
    } else {
        brain.needs_plan = false;
    }

    // 3. Choose a mode.
    let inputs = ModeInputs {
        dist_to_spot: brain.spot.map(|s| s.pos.distance(feet)),
        los: brain.los,
        no_los_secs,
        blocker: brain.blocker.is_some(),
        player_far: feet.xz().distance(player.xz()) > t.range_max + 2.0,
    };
    brain.mode = choose_mode(&inputs, brain.mode);
}

/// Scores candidate spots and returns the best (line of sight checked last,
/// for the best few only).
fn pick_spot(
    w: &DecisionWorld,
    brain: &mut GruntBrain,
    feet: Vec3,
    rivals: &[Vec3],
) -> Option<Spot> {
    let t = &w.tuning.grunt;
    let params = SpotParams::new(t.range_min, t.range_max);
    let player = w.track.feet;
    let rotation = brain.rng.range(0.0, std::f32::consts::TAU);
    let margin = 1.5;
    let (lo, hi) = (
        w.layout.bounds_min + Vec2::splat(margin),
        w.layout.bounds_max - Vec2::splat(margin),
    );
    let mut candidates: Vec<(Spot, f32)> = Vec::new();
    let mut consider = |brain: &GruntBrain, spot: Spot| {
        if brain.blacklisted(spot.pos) {
            return;
        }
        let c = SpotCandidate {
            pos: spot.pos,
            elevated: spot.elevated,
        };
        let mut score = score_spot(c, player, rivals, feet, brain.spot.map(|s| s.pos), &params);
        // A ramp is the way up, not the goal: the player's floors score higher.
        if spot.elevated && matches!(spot.node.surface, Surface::Ramp(_)) {
            score -= params.height_bonus * 0.6;
        }
        candidates.push((spot, score));
    };
    for pos in ring_candidates(player, rotation, 16, &params) {
        let inside = pos.x >= lo.x && pos.x <= hi.x && pos.z >= lo.y && pos.z <= hi.y;
        if !inside || !ground_spot_clear(&w.nav, pos, player) {
            continue;
        }
        if let Some(node) = w.nav.localize(pos).filter(NavNode::is_ground) {
            consider(
                brain,
                Spot {
                    pos,
                    node,
                    elevated: false,
                },
            );
        }
    }
    // Keep the current ground spot in the running (it earns the keep bonus).
    if let Some(s) = brain.spot
        && !s.elevated
        && w.nav.node_exists(s.node)
        && ground_spot_clear(&w.nav, s.pos, player)
    {
        consider(brain, s);
    }
    // Up on the build: the player's floors, and the ramps up to them.
    let level = w.track.level;
    if level >= 1 {
        for entry in w.nav.map.iter() {
            let cell = entry.slot.cell;
            let node = match entry.slot.kind {
                PieceKind::Floor if cell.level == level => {
                    w.nav.surface_node(cell.x, cell.z, level)
                }
                PieceKind::Ramp if cell.level == level - 1 => {
                    w.nav.ramp_node(cell.x, cell.z, cell.level)
                }
                _ => None,
            };
            let Some(node) = node else { continue };
            let pos = node.center();
            if pos.xz().distance(player.xz()) > t.range_max + 2.0 {
                continue;
            }
            consider(
                brain,
                Spot {
                    pos,
                    node,
                    elevated: true,
                },
            );
        }
    }
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
    candidates
        .into_iter()
        .take(6)
        .map(|(spot, score)| {
            let eye = spot.pos + Vec3::Y * 1.62;
            let seen = sight(w.spatial, eye, w.track.head, w.track.chest, w.is_piece).visible;
            (spot, score + if seen { params.los_bonus } else { 0.0 })
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(spot, _)| spot)
}

/// A ground spot a grunt can stand and strafe on: clear of props, and a clear
/// strafe line across the direction to the player.
fn ground_spot_clear(nav: &NavGrid, pos: Vec3, player: Vec3) -> bool {
    if !nav.clear_of_props(pos.xz(), STRAFE_RADIUS + 0.5) {
        return false;
    }
    let to_player = (player - pos).with_y(0.0).normalize_or(Vec3::NEG_Z);
    let side = to_player.cross(Vec3::Y) * (STRAFE_RADIUS + 0.1);
    nav.ground_segment_clear(pos - side, pos + side, 0.4)
}

pub(super) fn allocate_tokens(
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    track: Res<PlayerTrack>,
    mut tokens: ResMut<AttackTokens>,
    mut grunts: Query<
        (
            Entity,
            &Transform,
            &mut GruntBrain,
            &GruntStats,
            Option<&Wand>,
            Has<Downed>,
            Has<Parked>,
        ),
        With<Grunt>,
    >,
) {
    let tick = tick.0;
    tokens.holders.retain(|&e| {
        grunts
            .get(e)
            .is_ok_and(|(_, _, brain, _, wand, downed, parked)| {
                !downed && !parked && (brain.shot.is_some() || wand.is_some_and(Wand::is_winding))
            })
    });
    for (entity, _, mut brain, ..) in &mut grunts {
        let held = tokens.holders.contains(&entity);
        if brain.token != held {
            brain.token = held;
        }
    }
    if !track.alive {
        return;
    }
    let max = tuning.grunt.max_shooters as usize;
    if tokens.holders.len() >= max {
        return;
    }
    let fire_range = tuning.orb.range * 0.8;
    let mut requests: Vec<(u8, u32, Entity)> = Vec::new();
    for (entity, transform, brain, stats, wand, downed, parked) in &grunts {
        if downed || parked || brain.token || brain.shot.is_some() || tick < brain.next_shot {
            continue;
        }
        if wand.is_some_and(|w| w.cooldown > 0.0 || w.is_winding()) {
            continue;
        }
        let dist = transform.translation.distance(track.feet);
        let reacted = brain
            .los_gained
            .is_some_and(|t| tick >= t + ticks(stats.reaction));
        let priority = if brain.los && reacted && dist <= fire_range {
            0
        } else if brain.mode == GruntMode::ShootPiece
            && brain
                .blocker
                .is_some_and(|(_, p)| p.distance(transform.translation) <= fire_range)
        {
            1
        } else {
            continue;
        };
        requests.push((priority, (dist * 100.0) as u32, entity));
    }
    requests.sort();
    for (_, _, entity) in requests {
        if tokens.holders.len() >= max {
            break;
        }
        tokens.holders.push(entity);
        if let Ok((_, _, mut brain, ..)) = grunts.get_mut(entity) {
            brain.token = true;
        }
    }
}

pub(super) fn drive(
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    map: Res<PieceMap>,
    props: Res<GruntProps>,
    track: Res<PlayerTrack>,
    spatial: SpatialQuery,
    freeze: Option<Res<GalleryFreeze>>,
    pieces: Query<(), With<Piece>>,
    mut nav_stats: ResMut<GruntNavStats>,
    mut grunts: Query<
        (
            Entity,
            &Transform,
            &EyeHeight,
            &mut LookAngles,
            &mut PlayerIntent,
            &mut GruntBrain,
            &GruntStats,
            Option<&Wand>,
            Has<Downed>,
            Has<Parked>,
            Option<&Motor>,
        ),
        With<Grunt>,
    >,
) {
    let tick = tick.0;
    let nav = NavGrid::new(&map, &props.0);
    let run = tuning.movement.run_speed;
    let sprint = tuning.movement.sprint_speed;
    let positions: Vec<(Entity, Vec3)> = grunts
        .iter()
        .filter(|g| !g.8 && !g.9)
        .map(|g| (g.0, g.1.translation))
        .collect();
    let is_piece = |e: Entity| pieces.contains(e);
    for (
        entity,
        transform,
        eye,
        mut look,
        mut intent,
        mut brain,
        stats,
        wand,
        downed,
        parked,
        motor,
    ) in &mut grunts
    {
        if downed || parked || freeze.is_some() || !track.alive {
            brain.shot = None;
            brain.anchor = None;
            let idle = PlayerIntent::default();
            if *intent != idle {
                *intent = idle;
            }
            continue;
        }
        let brain = &mut *brain;
        let feet = transform.translation;
        let eye_pos = feet + Vec3::Y * eye.0;
        let dist_to_player = feet.distance(track.feet);
        let travel = brain.is_travelling();

        // Where to go.
        let mut wish = if travel {
            follow_path(brain, &nav, feet, tick)
        } else {
            strafe(brain, &nav, feet, track.feet)
        };
        let walking = wish.length_squared() > 1e-4;
        if travel && walking && feet.y < 0.5 {
            let mut push = Vec3::ZERO;
            for (other, pos) in &positions {
                let d = (feet - *pos).with_y(0.0);
                let len = d.length();
                if *other != entity && len < SEPARATION && len > 1e-3 {
                    push += d / len * (SEPARATION - len) / SEPARATION;
                }
            }
            wish = (wish + push * 0.8).clamp_length_max(1.0);
        }

        // Firing: hold the wand while the target stays in sight.
        let verify = |target: ShotTarget| -> Option<ShotTarget> {
            let seen = sight(&spatial, eye_pos, track.head, track.chest, is_piece);
            match target {
                ShotTarget::Player => seen.visible.then_some(ShotTarget::Player),
                // Whichever piece now blocks the line is the target.
                ShotTarget::Piece { .. } => match seen.blocker {
                    Some((entity, point)) if !seen.visible => {
                        Some(ShotTarget::Piece { entity, point })
                    }
                    _ => None,
                },
            }
        };
        let winding = wand.is_some_and(Wand::is_winding);
        let mut fire = false;
        let mut pressed = false;
        match brain.shot {
            None if brain.token => {
                let target = if brain.los {
                    Some(ShotTarget::Player)
                } else if brain.mode == GruntMode::ShootPiece {
                    brain
                        .blocker
                        .map(|(entity, point)| ShotTarget::Piece { entity, point })
                } else {
                    None
                };
                if let Some(target) = target.and_then(verify) {
                    brain.shot = Some(Shot {
                        target,
                        noise: ShotNoise::roll(&mut brain.rng),
                        start_tick: tick,
                        saw_windup: winding,
                    });
                    fire = true;
                    pressed = true;
                }
            }
            None => {}
            Some(mut shot) => {
                shot.saw_windup |= winding;
                let windup_ticks = ticks(tuning.grunt.windup);
                let done = if shot.saw_windup {
                    !winding
                } else {
                    tick >= shot.start_tick + windup_ticks + 6
                };
                match verify(shot.target) {
                    None => {
                        brain.shot = None;
                        brain.next_shot = tick + ticks(CANCEL_RETRY);
                    }
                    Some(_) if done => {
                        brain.shot = None;
                        brain.next_shot = shot.start_tick + ticks(stats.fire_interval);
                    }
                    Some(target) => {
                        shot.target = target;
                        brain.shot = Some(shot);
                        fire = true;
                    }
                }
            }
        }

        // Where to look: the aim point while shooting, the player while
        // strafing, else the way it walks.
        let aim = match brain.shot {
            Some(shot) => Some(shot_aim(
                &shot,
                stats,
                &tuning,
                &track,
                eye_pos,
                brain.head_only,
            )),
            None if !travel || (brain.los && dist_to_player <= tuning.grunt.range_max) => {
                Some(if brain.head_only {
                    track.head
                } else {
                    track.chest
                })
            }
            None => None,
        };
        let desired = match aim {
            Some(point) => look_toward(point - eye_pos),
            None if walking => LookAngles {
                yaw: look_toward(wish.with_y(0.0)).yaw,
                pitch: 0.0,
            },
            None => *look,
        };
        let turned = turn_toward(
            *look,
            desired,
            YAW_RATE * TICK_SECONDS,
            PITCH_RATE * TICK_SECONDS,
        );
        if *look != turned {
            *look = turned;
        }

        // Stuck: a grunt walking to its spot must keep making ground.
        let goal_far = brain
            .spot
            .is_some_and(|s| s.pos.distance(feet) > ARRIVE_RADIUS);
        let mut jump = false;
        match brain.anchor {
            Some((at, since)) if travel && goal_far && at.distance(feet) < STUCK_DISTANCE => {
                if tick.saturating_sub(since) >= ticks(STUCK_SECONDS) {
                    nav_stats.stuck_events += 1;
                    brain.needs_plan = true;
                    brain.anchor = Some((feet, tick));
                    brain.jump_hold = 15;
                    jump = true;
                }
            }
            _ => brain.anchor = Some((feet, tick)),
        }
        // Blocked by a lip (a floor's edge standing proud of a ramp's top):
        // pushing on the ground for BLOCKED_TICKS without moving, it hops.
        let pushing = travel && goal_far && wish.length_squared() > 0.25;
        let stalled = motor.is_some_and(|m| {
            m.grounded && Vec2::new(m.velocity.x, m.velocity.z).length() < 0.3 * stats.speed
        });
        if pushing && stalled && brain.jump_hold == 0 {
            brain.blocked = brain.blocked.saturating_add(1);
            if brain.blocked >= BLOCKED_TICKS {
                brain.blocked = 0;
                brain.jump_hold = 15;
                jump = true;
            }
        } else {
            brain.blocked = 0;
        }

        let speed = if travel {
            stats.speed
        } else {
            stats.speed.min(run)
        };
        let (axis, sprinting) = speed_axis(wish, &look, speed, run, sprint);
        let next = PlayerIntent {
            move_axis: axis,
            sprint: sprinting,
            jump: brain.jump_hold > 0,
            jump_pressed: jump || intent.jump_pressed,
            fire,
            fire_pressed: pressed || intent.fire_pressed,
            ..default()
        };
        brain.jump_hold = brain.jump_hold.saturating_sub(1);
        if *intent != next {
            *intent = next;
        }
    }
}

/// The point a shot aims at this tick: the lagged, led player plus the shot's
/// error, or the blocking piece.
fn shot_aim(
    shot: &Shot,
    stats: &GruntStats,
    tuning: &Tuning,
    track: &PlayerTrack,
    eye: Vec3,
    head_only: bool,
) -> Vec3 {
    let point = match shot.target {
        ShotTarget::Player => {
            let lag = ticks(stats.lag) as usize;
            let (snapshot, velocity) = track
                .history
                .snapshot(lag, TICK_SECONDS)
                .unwrap_or((track.chest, Vec3::ZERO));
            let snapshot = if head_only {
                snapshot + (track.head - track.chest)
            } else {
                snapshot
            };
            lead_point(
                eye,
                snapshot,
                shot.noise.estimate(velocity),
                tuning.orb.speed,
            )
        }
        ShotTarget::Piece { point, .. } => point,
    };
    let d = point - eye;
    point + shot.noise.offset(d, d.length(), stats.error_at_10m)
}

/// Walks the path; returns the wish direction (unit, or zero when there).
fn follow_path(brain: &mut GruntBrain, nav: &NavGrid, feet: Vec3, tick: u64) -> Vec3 {
    let Some(spot) = brain.spot else {
        return Vec3::ZERO;
    };
    let len = brain.path.len();
    while brain.path_index < len {
        let wp = brain.path[brain.path_index];
        let last = brain.path_index + 1 == len;
        let reach = if last { 0.4 } else { 0.6 };
        let flat = (wp.pos - feet).xz().length();
        if flat < reach && (wp.pos.y - feet.y).abs() < 1.6 {
            brain.path_index += 1;
        } else {
            break;
        }
    }
    // String-pull across open ground, every 4 ticks, staggered by the
    // brain's phase so the knights of a wave don't all pull on one tick.
    if (tick + u64::from(brain.phase)).is_multiple_of(4) && feet.y < 0.5 {
        while brain.path_index + 1 < len {
            let (a, b) = (
                brain.path[brain.path_index],
                brain.path[brain.path_index + 1],
            );
            if a.node.is_ground()
                && b.node.is_ground()
                && nav.ground_segment_clear(feet, b.pos, 0.4)
            {
                brain.path_index += 1;
            } else {
                break;
            }
        }
    }
    let target = if brain.path_index < len {
        brain.path[brain.path_index].pos
    } else if len > 0
        || (feet.y < 0.5
            && spot.node.surface == Surface::Ground
            && nav.ground_segment_clear(feet, spot.pos, 0.4))
    {
        spot.pos
    } else {
        // No path yet and no straight way there: wait for the plan.
        return Vec3::ZERO;
    };
    let d = (target - feet).with_y(0.0);
    if d.length() < 0.05 {
        Vec3::ZERO
    } else {
        d.normalize()
    }
}

/// Strafes across the line to the player around the spot (like the dummy's
/// strafe). Returns the wish direction.
fn strafe(brain: &mut GruntBrain, nav: &NavGrid, feet: Vec3, player: Vec3) -> Vec3 {
    let Some(spot) = brain.spot else {
        return Vec3::ZERO;
    };
    brain.strafe_turn -= TICK_SECONDS;
    if brain.strafe_turn <= 0.0 {
        brain.strafe_dir = -brain.strafe_dir;
        brain.strafe_turn = brain.rng.range(0.4, 1.6);
    }
    let forward = (player - feet).with_y(0.0).normalize_or(Vec3::NEG_Z);
    let right = forward.cross(Vec3::Y);
    let offset = (feet - spot.pos).with_y(0.0);
    let along = offset.dot(right);
    let blocked =
        |dir: f32| feet.y < 0.5 && !nav.ground_segment_clear(feet, feet + right * dir * 0.8, 0.35);
    if along * brain.strafe_dir > STRAFE_RADIUS || blocked(brain.strafe_dir) {
        brain.strafe_dir = -brain.strafe_dir;
        brain.strafe_turn = brain.rng.range(0.4, 1.6);
    }
    let radial = offset - right * along;
    (right * brain.strafe_dir - radial * 0.5).clamp_length_max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> ModeInputs {
        ModeInputs {
            dist_to_spot: Some(0.5),
            los: true,
            no_los_secs: 0.0,
            blocker: false,
            player_far: false,
        }
    }

    #[test]
    fn far_from_the_spot_it_travels_then_strafes_on_arrival() {
        let far = ModeInputs {
            dist_to_spot: Some(20.0),
            player_far: true,
            ..inputs()
        };
        assert_eq!(choose_mode(&far, GruntMode::Strafe), GruntMode::Approach);
        let near = ModeInputs {
            dist_to_spot: Some(6.0),
            ..inputs()
        };
        assert_eq!(choose_mode(&near, GruntMode::Strafe), GruntMode::Reposition);
        assert_eq!(
            choose_mode(&inputs(), GruntMode::Approach),
            GruntMode::Strafe
        );
    }

    #[test]
    fn stickiness_keeps_the_current_mode_near_the_boundary() {
        let edge = ModeInputs {
            dist_to_spot: Some(2.0),
            ..inputs()
        };
        assert_eq!(choose_mode(&edge, GruntMode::Strafe), GruntMode::Strafe);
        assert_eq!(
            choose_mode(&edge, GruntMode::Reposition),
            GruntMode::Reposition
        );
    }

    #[test]
    fn a_blocking_piece_is_shot_once_sight_has_been_lost_a_while() {
        let hidden = ModeInputs {
            los: false,
            blocker: true,
            no_los_secs: 0.2,
            ..inputs()
        };
        assert_eq!(choose_mode(&hidden, GruntMode::Strafe), GruntMode::Strafe);
        let longer = ModeInputs {
            no_los_secs: 1.0,
            ..hidden
        };
        assert_eq!(
            choose_mode(&longer, GruntMode::Strafe),
            GruntMode::ShootPiece
        );
        let no_piece = ModeInputs {
            blocker: false,
            ..longer
        };
        assert_eq!(
            choose_mode(&no_piece, GruntMode::ShootPiece),
            GruntMode::Strafe
        );
    }

    #[test]
    fn turning_is_rate_limited_and_wraps() {
        let from = LookAngles {
            yaw: 0.1,
            pitch: 0.0,
        };
        let to = LookAngles {
            yaw: std::f32::consts::TAU - 0.1,
            pitch: 0.5,
        };
        let t = turn_toward(from, to, 0.05, 0.1);
        assert!(
            (t.yaw - 0.05).abs() < 1e-5,
            "the short way round: {}",
            t.yaw
        );
        assert!((t.pitch - 0.1).abs() < 1e-5);
        assert_eq!(turn_toward(from, to, 1.0, 1.0), to);
    }

    #[test]
    fn speed_axis_runs_scales_and_sprints_only_forward() {
        let look = LookAngles::default();
        let fwd = look.forward();
        let (a, s) = speed_axis(fwd, &look, 4.5, 5.5, 7.5);
        assert!(!s && (a.length() - 4.5 / 5.5).abs() < 1e-4 && a.y > 0.0);
        let (a, s) = speed_axis(fwd, &look, 6.5, 5.5, 7.5);
        assert!(s && (a.length() - 6.5 / 7.5).abs() < 1e-4);
        let side = look.flat_basis().1;
        let (a, s) = speed_axis(side, &look, 6.5, 5.5, 7.5);
        assert!(!s && (a.length() - 1.0).abs() < 1e-4, "no sprint sideways");
        assert_eq!(
            speed_axis(Vec3::ZERO, &look, 6.5, 5.5, 7.5),
            (Vec2::ZERO, false)
        );
    }
}
