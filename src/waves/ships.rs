//! Ship arrivals (docs/M3-SPEC.md → Ship arrivals, D82): the wave director's
//! knights come down from the station on drop ships.
//!
//! **A sortie** ([`Sortie`]) is one ship's trip, and everything about it is a
//! pure function of its clock (seconds since launch), like M2's far motion:
//!
//! | t (s) | what happens |
//! |---|---|
//! | 0 | it peels off the station's dock ([`station_dock`]) |
//! | 0–4 | it swoops down a spline to hover 12–16 m above its drop point ([`FLIGHT_SECONDS`]) |
//! | 3 | the rune circle lights on the landing spot, 2 s before the first knight lands ([`TELEGRAPH_LEAD`]) |
//! | 4 + 0.4 k | knight k starts down the beam ([`BEAM_GAP`]) |
//! | 5 + 0.4 k | knight k lands with a sparkle poof ([`BEAM_SECONDS`]) |
//! | last landing + 0.5 | the ship flies back up to the station ([`HOLD_SECONDS`], [`DEPART_SECONDS`]) |
//!
//! **Drop points** ([`DROP_POINTS`]): six fixed spots just inside the island
//! edge, on the ring the chunk 1 poofs used ([`super::EDGE_INSET`] inside the
//! bounds), away from the corners' props and the initial cover. A ship's 1–3
//! knights land in lanes [`LANE_SPACING`] apart along the edge.
//!
//! **Pacing** (the director, [`plan_sortie`]): a ship launches whenever a pool
//! slot is free, never letting the knights alive plus those aboard or in a beam
//! exceed `max_alive` (8), at most [`MAX_SHIPS`] in the air, a seeded
//! [`LAUNCH_GAP`] apart. Each takes a seeded 1–3 knights, at most half the
//! wave (rounded up), so every wave of two or more comes on several ships. The
//! drop point (at least [`super::SPAWN_MIN_PLAYER_DISTANCE`] plus a lane from
//! the player), the hover height and the launch offset are seeded too, all
//! from the run's stream, so the same seed flies the same ships.
//!
//! **In the beam** ([`Beaming`]) a knight stays `Parked` and `Downed`: the
//! brain skips it (its intent stays empty), movement leaves it where the beam
//! puts it, and shots and orbs pass through it. On landing it loses all three
//! and fights. The figure still shows in the beam (`arena::visuals::target`).
//!
//! Ships are a fixed array ([`Ships`]); nothing is allocated per arrival.

use super::{EDGE_INSET, PoolGrunt, PoolParts, Run, SPAWN_MIN_PLAYER_DISTANCE, SpawnCheck, ticks};
use crate::{
    arena::ArenaLayout,
    combat::Downed,
    dummy::look_toward,
    far::{
        galaxy::sky_point,
        layout::{SPAWN_EYE, STATION_AZIMUTH, STATION_DISTANCE, STATION_PLATFORM_Y},
    },
    grunt::Parked,
    rng::Rng,
    shared::{ARENA_HALF, GameCue, Player, TICK_SECONDS},
};
use bevy::prelude::*;

/// Ships in the air at once (the pool of sorties and ship models).
pub const MAX_SHIPS: usize = 4;
/// Knights a ship carries at most.
pub const MAX_ABOARD: usize = 3;
/// From the dock to the hover point (s).
pub const FLIGHT_SECONDS: f32 = 4.0;
/// A knight's slide down the beam (s).
pub const BEAM_SECONDS: f32 = 1.0;
/// Between knights starting down the beam (s).
pub const BEAM_GAP: f32 = 0.4;
/// The rune circle lights this long before the first knight lands (s).
pub const TELEGRAPH_LEAD: f32 = 2.0;
/// The ship hovers this long after its last knight lands (s)...
pub const HOLD_SECONDS: f32 = 0.5;
/// ...then flies back up to the station in this long (s).
pub const DEPART_SECONDS: f32 = 3.5;
/// Seeded hover height above the drop point (m).
pub const HOVER_HEIGHT: (f32, f32) = (12.0, 16.0);
/// Seeded time between launches (s).
pub const LAUNCH_GAP: (f32, f32) = (0.6, 1.6);
/// With no drop point free, the director tries again this soon (s).
pub const RETRY_SECONDS: f32 = 0.5;
/// A ship's knights land this far apart along the edge (m): lane 0 on the
/// drop point, lanes 1 and 2 either side.
pub const LANE_SPACING: f32 = 2.2;
/// Lane offsets along the edge (m).
pub const LANES: [f32; MAX_ABOARD] = [0.0, LANE_SPACING, -LANE_SPACING];
/// The beam leaves the hull crystal this far below the ship's origin (m): the
/// `dropship` model's `Beam` attach point.
pub const CRYSTAL_DROP: f32 = 2.9;
/// A knight starts down the beam with its feet this far below the crystal (m).
pub const BEAM_ENTRY: f32 = 2.2;
/// The seeded sideways spread of launch points about the dock (m).
pub const DOCK_SPREAD: f32 = 60.0;

/// A fixed drop point: the knights' landing spot (lane 0) on the ground, and
/// the edge's direction there (the lanes run along it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropPoint {
    pub at: Vec3,
    pub along: Vec3,
}

impl DropPoint {
    /// Where lane `k`'s knight lands.
    pub fn lane(&self, k: usize) -> Vec3 {
        self.at + self.along * LANES[k.min(MAX_ABOARD - 1)]
    }

    /// Straight out of the island from here (horizontal).
    pub fn outward(&self) -> Vec3 {
        if self.at.x.abs() >= self.at.z.abs() {
            Vec3::X * self.at.x.signum()
        } else {
            Vec3::Z * self.at.z.signum()
        }
    }
}

/// The ring the drop points sit on: [`EDGE_INSET`] inside the playable bounds.
const RING: f32 = ARENA_HALF - 0.5 - EDGE_INSET;

const fn drop_point(x: f32, z: f32, along: Vec3) -> DropPoint {
    DropPoint {
        at: Vec3::new(x, 0.0, z),
        along,
    }
}

/// The six drop points (north -Z, east +X). None is within a lane of a
/// corner's props or the initial cover; the east one is on the close edge,
/// where the cliff drops right behind the barrier.
pub const DROP_POINTS: [DropPoint; 6] = [
    drop_point(-4.0, -RING, Vec3::X),
    drop_point(12.0, -RING, Vec3::X),
    drop_point(RING, -4.0, Vec3::Z),
    drop_point(14.0, RING, Vec3::X),
    drop_point(-RING, 8.0, Vec3::Z),
    drop_point(-RING, -10.0, Vec3::Z),
];

/// The station's launch bay: over its platform, where every sortie starts and
/// ends (far::layout's station, azimuth 30°, 640 m out, 120 m up).
pub fn station_dock() -> Vec3 {
    let dir = sky_point(STATION_AZIMUTH, 0.0);
    Vec3::new(
        SPAWN_EYE.x + dir.x * STATION_DISTANCE,
        STATION_PLATFORM_Y + 30.0,
        SPAWN_EYE.z + dir.z * STATION_DISTANCE,
    )
}

fn bezier(p: [Vec3; 4], u: f32) -> Vec3 {
    let v = 1.0 - u;
    p[0] * (v * v * v) + p[1] * (3.0 * v * v * u) + p[2] * (3.0 * v * u * u) + p[3] * (u * u * u)
}

/// 0 → 1 with zero slope at both ends.
fn smootherstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * x * (x * (x * 6.0 - 15.0) + 10.0)
}

fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Where a knight is in its trip down.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SeatState {
    #[default]
    Aboard,
    Beaming,
    Landed,
}

/// One knight's place on a ship.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Seat {
    pub knight: Option<Entity>,
    /// Where it lands (chosen as it starts down the beam).
    pub spot: Vec3,
    pub state: SeatState,
}

/// Which part of its trip a ship is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShipPhase {
    Approach,
    Hover,
    Depart,
    Done,
}

/// One drop ship's trip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sortie {
    pub launch_tick: u64,
    /// Index into [`DROP_POINTS`].
    pub point: usize,
    pub hover_height: f32,
    /// Sideways offset of its launch from the dock (m).
    pub peel: f32,
    /// Knights aboard (1..=3).
    pub count: usize,
    pub seats: [Seat; MAX_ABOARD],
    /// The run ended: nobody else gets off, the beam and circle go out, and
    /// the ship flies its trip out empty.
    pub cancelled: bool,
}

impl Sortie {
    pub fn new(launch_tick: u64, point: usize, hover_height: f32, peel: f32, count: usize) -> Self {
        Self {
            launch_tick,
            point: point.min(DROP_POINTS.len() - 1),
            hover_height,
            peel,
            count: count.clamp(1, MAX_ABOARD),
            seats: [Seat::default(); MAX_ABOARD],
            cancelled: false,
        }
    }

    /// Seconds since launch at tick `now`.
    pub fn seconds(&self, now: u64) -> f32 {
        now.saturating_sub(self.launch_tick) as f32 * TICK_SECONDS
    }

    pub fn drop_point(&self) -> DropPoint {
        DROP_POINTS[self.point]
    }

    /// Where it hovers.
    pub fn hover_point(&self) -> Vec3 {
        self.drop_point().at + Vec3::Y * self.hover_height
    }

    /// When knight `k` starts down the beam, and when it lands (s).
    pub fn beam_start(&self, k: usize) -> f32 {
        FLIGHT_SECONDS + BEAM_GAP * k as f32
    }

    pub fn landing(&self, k: usize) -> f32 {
        self.beam_start(k) + BEAM_SECONDS
    }

    pub fn last_landing(&self) -> f32 {
        self.landing(self.count - 1)
    }

    /// When the rune circle lights (s).
    pub fn telegraph_start(&self) -> f32 {
        self.landing(0) - TELEGRAPH_LEAD
    }

    /// When it leaves its hover, and when it's back at the dock (s).
    pub fn depart_start(&self) -> f32 {
        self.last_landing() + HOLD_SECONDS
    }

    pub fn done_at(&self) -> f32 {
        self.depart_start() + DEPART_SECONDS
    }

    /// The tick a time `t` (s since launch) falls on.
    pub fn tick_at(&self, t: f32) -> u64 {
        self.launch_tick + ticks(t)
    }

    pub fn phase(&self, t: f32) -> ShipPhase {
        if t < FLIGHT_SECONDS {
            ShipPhase::Approach
        } else if t < self.depart_start() {
            ShipPhase::Hover
        } else if t < self.done_at() {
            ShipPhase::Depart
        } else {
            ShipPhase::Done
        }
    }

    /// Where it launches from: the dock, shifted sideways by `peel`.
    pub fn origin(&self) -> Vec3 {
        let dock = station_dock();
        let side = dock.with_y(0.0).normalize_or(Vec3::X).cross(Vec3::Y);
        dock + side * self.peel
    }

    /// The approach: off the dock toward the island, dropping, then in over
    /// the edge from the void side, slowing into the hover.
    fn approach(&self) -> [Vec3; 4] {
        let p0 = self.origin();
        let hover = self.hover_point();
        let toward = (hover - p0).with_y(0.0).normalize_or(Vec3::NEG_Z);
        let out = self.drop_point().outward();
        [
            p0,
            p0 + toward * 220.0 - Vec3::Y * 40.0,
            hover + out * 60.0 + Vec3::Y * 30.0,
            hover,
        ]
    }

    /// The climb back: up and out over the edge, then home to the dock.
    fn departure(&self) -> [Vec3; 4] {
        let hover = self.hover_point();
        let p3 = self.origin();
        let out = self.drop_point().outward();
        [
            hover,
            hover + Vec3::Y * 45.0 + out * 20.0,
            p3 + (hover - p3).with_y(0.0).normalize_or(Vec3::Z) * 200.0 + Vec3::Y * 40.0,
            p3,
        ]
    }

    /// The ship's position at `t` (s since launch).
    pub fn position(&self, t: f32) -> Vec3 {
        match self.phase(t) {
            ShipPhase::Approach => bezier(self.approach(), smootherstep(t / FLIGHT_SECONDS)),
            ShipPhase::Hover => {
                // A gentle bob, easing in so it joins the approach smoothly.
                let h = t - FLIGHT_SECONDS;
                let ease = smoothstep(h / 0.6);
                self.hover_point() + Vec3::Y * (0.3 * ease * (h * 2.4).sin())
            }
            ShipPhase::Depart | ShipPhase::Done => {
                let u = ((t - self.depart_start()) / DEPART_SECONDS).clamp(0.0, 1.0);
                bezier(self.departure(), u * u)
            }
        }
    }

    /// The ship's horizontal heading at `t` (it faces along its flight, and
    /// holds its arrival heading while it hovers).
    pub fn heading(&self, t: f32) -> Vec3 {
        let arrival = || {
            let p = self.approach();
            (p[3] - p[2]).with_y(0.0).normalize_or(Vec3::NEG_Z)
        };
        let along = |a: f32, b: f32| {
            (self.position(b) - self.position(a))
                .with_y(0.0)
                .try_normalize()
        };
        match self.phase(t) {
            ShipPhase::Approach => along(t, (t + 0.05).min(FLIGHT_SECONDS))
                .filter(|_| t < FLIGHT_SECONDS - 0.05)
                .unwrap_or_else(arrival),
            ShipPhase::Hover => arrival(),
            ShipPhase::Depart | ShipPhase::Done => along(t, t + 0.05).unwrap_or_else(arrival),
        }
    }

    /// The beam is on (s since launch): from the ship's arrival until just
    /// after its last knight lands.
    pub fn beam_on(&self, t: f32) -> bool {
        !self.cancelled && (FLIGHT_SECONDS - 0.15..self.last_landing() + 0.15).contains(&t)
    }

    /// The rune circle's glow (0..=1): up over a quarter second from
    /// [`Sortie::telegraph_start`], held, and fading after the last landing.
    pub fn telegraph(&self, t: f32) -> f32 {
        if self.cancelled {
            return 0.0;
        }
        let start = self.telegraph_start();
        let end = self.last_landing();
        if t < start || t > end + 0.4 {
            0.0
        } else if t <= end {
            smoothstep((t - start) / 0.25)
        } else {
            1.0 - smoothstep((t - end) / 0.4)
        }
    }

    /// Where knight `k`'s feet are in the beam at `t`, sliding from the
    /// crystal down to its `spot`; `None` outside its slide.
    pub fn in_beam(&self, k: usize, spot: Vec3, t: f32) -> Option<Vec3> {
        let start = self.beam_start(k);
        if !(start..self.landing(k)).contains(&t) {
            return None;
        }
        let u = (t - start) / BEAM_SECONDS;
        let top = self.hover_point() - Vec3::Y * (CRYSTAL_DROP + BEAM_ENTRY);
        // Down the beam, easing in and landing softly, drifting out to its lane.
        let down = smoothstep(u);
        let drift = smoothstep((u * 1.4).min(1.0));
        let x = top.x + (spot.x - top.x) * drift;
        let z = top.z + (spot.z - top.z) * drift;
        let y = top.y + (spot.y - top.y) * down;
        Some(Vec3::new(x, y, z))
    }

    /// Knights aboard or in the beam.
    pub fn unlanded(&self) -> usize {
        self.seats[..self.count]
            .iter()
            .filter(|s| s.knight.is_some() && s.state != SeatState::Landed)
            .count()
    }
}

/// The drop ships in flight: a fixed pool of [`MAX_SHIPS`] slots.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct Ships {
    pub slots: [Option<Sortie>; MAX_SHIPS],
}

impl Ships {
    pub fn live(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    pub fn free_slot(&self) -> Option<usize> {
        self.slots.iter().position(Option::is_none)
    }

    /// A ship in the air is using drop point `point`.
    pub fn uses(&self, point: usize) -> bool {
        self.slots.iter().flatten().any(|s| s.point == point)
    }

    /// Knights aboard a ship or in a beam.
    pub fn unlanded(&self) -> usize {
        self.slots.iter().flatten().map(Sortie::unlanded).sum()
    }

    pub fn reset(&mut self) {
        self.slots = [None; MAX_SHIPS];
    }
}

/// On a knight sliding down a ship's beam: `Parked` and `Downed` with it, so it
/// can't be hit and doesn't act, but its figure shows.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Beaming {
    /// Its ship's slot in [`Ships`], and its seat.
    pub ship: usize,
    pub seat: usize,
}

/// A planned launch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SortiePlan {
    pub point: usize,
    pub count: usize,
    pub hover_height: f32,
    pub peel: f32,
    /// Seconds until the next launch may go.
    pub gap: f32,
}

/// Plans the next launch, drawing from the run's `rng`: a drop point not in
/// use by a ship in the air, far enough from the `player` and clear
/// (`is_clear`), and 1..=`max_count` knights. `None` when no drop point
/// qualifies (the director tries again after [`RETRY_SECONDS`]).
pub fn plan_sortie(
    rng: &mut Rng,
    ships: &Ships,
    player: Vec3,
    max_count: usize,
    is_clear: impl Fn(Vec3) -> bool,
) -> Option<SortiePlan> {
    let mut candidates = [0usize; DROP_POINTS.len()];
    let mut n = 0;
    for (i, point) in DROP_POINTS.iter().enumerate() {
        let far = point.at.xz().distance(player.xz()) >= SPAWN_MIN_PLAYER_DISTANCE + LANE_SPACING;
        if far && !ships.uses(i) && is_clear(point.at) {
            candidates[n] = i;
            n += 1;
        }
    }
    if n == 0 || max_count == 0 {
        return None;
    }
    let point = candidates[(rng.next_u64() % n as u64) as usize];
    let max_count = max_count.min(MAX_ABOARD);
    let count = 1 + (rng.next_u64() % max_count as u64) as usize;
    let hover_height = rng.range(HOVER_HEIGHT.0, HOVER_HEIGHT.1);
    let peel = rng.range(-DOCK_SPREAD, DOCK_SPREAD);
    let gap = rng.range(LAUNCH_GAP.0, LAUNCH_GAP.1);
    Some(SortiePlan {
        point,
        count,
        hover_height,
        peel,
        gap,
    })
}

/// How many knights the next ship may take: at most [`MAX_ABOARD`], the
/// wave's knights not yet on a ship, the free alive slots, and half the wave
/// (rounded up), so a wave of two or more comes on several ships.
pub fn max_aboard(wave_size: u32, unassigned: u32, room: u32) -> usize {
    let half = wave_size.div_ceil(2).max(1);
    (MAX_ABOARD as u32).min(unassigned).min(room).min(half) as usize
}

/// Where knight `seat` of a ship at drop point `point` lands: its own lane if
/// that's clear and far enough from the player, else the first lane that is,
/// else (all blocked by pieces) its own lane anyway.
fn landing_spot(
    point: DropPoint,
    seat: usize,
    player: Vec3,
    is_clear: impl Fn(Vec3) -> bool,
) -> Vec3 {
    let ok = |p: Vec3| p.xz().distance(player.xz()) >= SPAWN_MIN_PLAYER_DISTANCE && is_clear(p);
    let own = point.lane(seat);
    if ok(own) {
        return own;
    }
    (0..MAX_ABOARD)
        .map(|k| point.lane(k))
        .find(|&p| ok(p))
        .unwrap_or(own)
}

/// Steps every ship after the director: knights start down their beams, slide
/// down, and land (then they're in play); finished trips free their slot. When
/// the run has ended, ships drop nobody else: their unlanded knights go back
/// to the pool.
pub(super) fn step_ships(
    mut commands: Commands,
    mut run: ResMut<Run>,
    mut ships: ResMut<Ships>,
    tick: Res<crate::shared::SimTick>,
    layout: Res<ArenaLayout>,
    spawn_check: SpawnCheck,
    players: Query<&Transform, With<Player>>,
    mut grunts: Query<PoolParts, (With<PoolGrunt>, Without<Player>)>,
    mut cues: MessageWriter<GameCue>,
) {
    let now = tick.0;
    let player = players
        .iter()
        .next()
        .map_or(layout.player_spawn, |p| p.translation);
    let ended = run.is_ended();
    for (slot, entry) in ships.slots.iter_mut().enumerate() {
        let Some(sortie) = entry.as_mut() else {
            continue;
        };
        let t = sortie.seconds(now);
        if ended && !sortie.cancelled {
            sortie.cancelled = true;
            for seat in sortie.seats.iter_mut() {
                if seat.state == SeatState::Landed {
                    continue;
                }
                if let Some(mut grunt) = seat.knight.and_then(|k| grunts.get_mut(k).ok()) {
                    grunt.park(&mut commands, now);
                    commands.entity(grunt.entity).remove::<Beaming>();
                }
                seat.knight = None;
            }
            run.aboard = 0;
        }
        if !sortie.cancelled {
            let point = sortie.drop_point();
            for k in 0..sortie.count {
                let seat = sortie.seats[k];
                let Some(knight) = seat.knight else {
                    continue;
                };
                let Ok(mut grunt) = grunts.get_mut(knight) else {
                    continue;
                };
                match seat.state {
                    SeatState::Aboard if now >= sortie.tick_at(sortie.beam_start(k)) => {
                        let spot = landing_spot(point, k, player, |p| spawn_check.is_clear(p));
                        sortie.seats[k].spot = spot;
                        sortie.seats[k].state = SeatState::Beaming;
                        let at = sortie.in_beam(k, spot, t).unwrap_or(sortie.hover_point());
                        grunt.transform.translation = at;
                        grunt.previous.0 = at;
                        *grunt.look = look_toward((player - spot).with_y(0.0));
                        commands.entity(knight).insert(Beaming {
                            ship: slot,
                            seat: k,
                        });
                    }
                    SeatState::Beaming if now >= sortie.tick_at(sortie.landing(k)) => {
                        // Touchdown: in play, with a sparkle poof.
                        let spot = seat.spot;
                        grunt.transform.translation = spot;
                        grunt.previous.0 = spot;
                        *grunt.look = look_toward((player - spot).with_y(0.0));
                        grunt.slot.aboard = false;
                        commands
                            .entity(knight)
                            .remove::<(Parked, Downed, Beaming)>();
                        cues.write(GameCue::Respawned { who: knight });
                        sortie.seats[k].state = SeatState::Landed;
                        run.aboard = run.aboard.saturating_sub(1);
                        run.remaining = run.remaining.saturating_sub(1);
                        run.alive += 1;
                    }
                    SeatState::Beaming => {
                        if let Some(at) = sortie.in_beam(k, seat.spot, t) {
                            grunt.transform.translation = at;
                        }
                    }
                    _ => {}
                }
            }
        }
        if sortie.phase(t) == ShipPhase::Done {
            *entry = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sortie_hovers_at_four_seconds_and_drops_on_schedule() {
        let s = Sortie::new(0, 2, 14.0, 10.0, 3);
        assert_eq!(s.position(FLIGHT_SECONDS), s.hover_point());
        assert!(s.position(0.0).distance(s.hover_point()) > 500.0);
        assert!(s.position(FLIGHT_SECONDS - 0.5).distance(s.hover_point()) > 0.5);
        assert!((s.landing(0) - s.telegraph_start() - TELEGRAPH_LEAD).abs() < 1e-5);
        assert!((s.landing(2) - 5.8).abs() < 1e-5);
        assert_eq!(s.phase(s.done_at() + 0.1), ShipPhase::Done);
        // Back home at the end.
        assert!(s.position(s.done_at()).distance(s.origin()) < 1e-2);
    }

    #[test]
    fn the_beam_carries_a_knight_from_the_crystal_to_its_lane() {
        let s = Sortie::new(0, 0, 13.0, 0.0, 2);
        let spot = s.drop_point().lane(1);
        let top = s.in_beam(1, spot, s.beam_start(1)).unwrap();
        assert!(top.y > 7.0, "starts up under the ship: {top}");
        let near_end = s.in_beam(1, spot, s.landing(1) - 0.01).unwrap();
        assert!(near_end.distance(spot) < 0.05);
        assert_eq!(s.in_beam(1, spot, s.landing(1)), None);
        assert_eq!(s.in_beam(1, spot, s.beam_start(1) - 0.01), None);
    }

    #[test]
    fn drop_points_sit_just_inside_the_edge() {
        let layout = ArenaLayout::default();
        for p in DROP_POINTS {
            for k in 0..MAX_ABOARD {
                let lane = p.lane(k);
                let edge =
                    (layout.bounds_max.x - lane.x.abs()).min(layout.bounds_max.y - lane.z.abs());
                assert!((edge - EDGE_INSET).abs() < 1e-4, "{lane}: {edge}");
            }
        }
    }

    #[test]
    fn a_wave_splits_across_ships() {
        assert_eq!(max_aboard(3, 3, 8), 2);
        assert_eq!(max_aboard(11, 11, 8), 3);
        assert_eq!(max_aboard(11, 1, 8), 1);
        assert_eq!(max_aboard(11, 11, 0), 0);
        assert_eq!(max_aboard(1, 1, 8), 1);
    }
}
