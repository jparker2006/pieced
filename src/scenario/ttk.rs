//! Slice G — `ttk`: standing dummy at 15 m under continuous rifle fire (G6).
//!
//! With the dummy set to stand still, the player holds the rifle trigger on its
//! chest from 15 m until it is eliminated, for several kills (the dummy is
//! re-placed at 15 m after each respawn and the rifle reloaded before each
//! kill). Then one point-blank pump body shot at a full-health dummy at each of
//! a few close ranges, and finally a wall placed between the two soaks
//! continuous rifle fire until it breaks and a bullet reaches the dummy.
//!
//! Times come from simulation ticks (60 per second) carried by the damage,
//! elimination and piece messages; wall-clock times of the frames that
//! observed them are recorded alongside.

use super::{
    Director, DirectorStatus, ScenarioClock, capture, player_entity, set_look, teleport,
    with_intent,
};
use crate::{
    arena::ArenaLayout,
    building::{PieceSlot, place_piece},
    combat::{CombatStats, Downed, Loadout},
    dummy::{Dummy, look_toward},
    shared::{
        ActiveTool, DamageDealt, DamageTarget, Eliminated, EyeHeight, Facing, GridCell, Health,
        PieceChange, PieceChanged, PreviousFeet, ShotFired, SimTick, TICK_SECONDS, WeaponKind,
    },
    telemetry,
    tuning::Tuning,
};
use bevy::{
    ecs::message::{Message, MessageCursor, Messages},
    prelude::*,
};
use serde_json::{Value, json};
use std::time::Instant;

/// Dummy distance for rifle kills and the wall soak (m, feet to feet).
pub const RANGE: f32 = 15.0;
/// Rifle kills measured.
pub const KILLS: usize = 6;
/// Point-blank pump ranges (m).
pub const PUMP_RANGES: [f32; 3] = [1.5, 2.5, 3.5];
/// Aim point above the dummy's feet (m): mid-torso. The TTK contract is about
/// body hits; at 15 m full rifle bloom (1.8°) strays up to ~0.47 m, so aiming
/// here keeps bloom from landing headshots (head sphere starts at 1.42 m).
pub const CHEST: f32 = 0.9;
/// Give up on a step after this long (s).
const TIMEOUT: f64 = 6.0;

const RIFLE: ActiveTool = ActiveTool::Weapon(WeaponKind::Rifle);
const PUMP: ActiveTool = ActiveTool::Weapon(WeaponKind::Pump);

pub fn director(name: &str) -> Option<Box<dyn Director>> {
    (name == "ttk").then(|| Box::new(Ttk::default()) as Box<dyn Director>)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Step {
    Start,
    /// Get the rifle ready and the dummy standing at 15 m.
    RifleSetup,
    RifleFire,
    PumpSetup,
    PumpShot,
    WallSetup,
    WallFire,
    Done,
}

struct Cursor<M: Message>(Option<MessageCursor<M>>);

impl<M: Message + Clone> Cursor<M> {
    fn read(&mut self, world: &World) -> Vec<M> {
        let messages = world.resource::<Messages<M>>();
        let cursor = self.0.get_or_insert_with(|| messages.get_cursor());
        cursor.read(messages).cloned().collect()
    }
}

impl<M: Message> Default for Cursor<M> {
    fn default() -> Self {
        Self(None)
    }
}

#[derive(Default)]
struct Messages4 {
    damage: Cursor<DamageDealt>,
    kills: Cursor<Eliminated>,
    shots: Cursor<ShotFired>,
    pieces: Cursor<PieceChanged>,
}

#[derive(Debug, Clone, Default)]
struct RifleKill {
    trigger_tick: u64,
    trigger_at: Option<Instant>,
    first_damage_tick: Option<u64>,
    first_damage_seen: Option<Instant>,
    kill_tick: Option<u64>,
    kill_seen: Option<Instant>,
    shots: u32,
    hits: u32,
    headshots: u32,
    readout_ttk_s: Option<f32>,
    start_total_health: f32,
    range_m: f32,
}

#[derive(Debug, Clone, Default)]
struct PumpShot {
    range_m: f32,
    start_total_health: f32,
    damage: f32,
    pellets_fired: usize,
    pellets_hit: usize,
    headshot: bool,
    killed: bool,
    health_after: (f32, f32),
}

#[derive(Debug, Clone, Default)]
struct WallSoak {
    slot: String,
    wall_center: Vec3,
    placed: bool,
    placement_error: Option<String>,
    trigger_tick: u64,
    first_hit_tick: Option<u64>,
    break_tick: Option<u64>,
    dummy_hit_tick: Option<u64>,
    hits_on_wall: u32,
    damage_to_dummy_before_break: f32,
    wall: Option<Entity>,
}

#[derive(Default)]
struct Ttk {
    step: Option<Step>,
    /// Seconds (scenario clock) when the current step began.
    step_started: f64,
    /// Frames spent in the current step.
    frames_in_step: u32,
    messages: Messages4,
    player_feet: Vec3,
    kills: Vec<RifleKill>,
    pumps: Vec<PumpShot>,
    wall: WallSoak,
    notes: Vec<String>,
    captured_rifle: bool,
}

/// Screenshot for the evidence folder (skipped in headless runs).
fn snapshot(world: &mut World, name: &str) {
    if world.contains_resource::<super::ScenarioRun>() {
        capture(world, name, false);
    }
}

fn dummy_entity(world: &mut World) -> Option<Entity> {
    world
        .query_filtered::<Entity, With<Dummy>>()
        .iter(world)
        .next()
}

fn place_character(world: &mut World, entity: Entity, feet: Vec3) {
    if let Some(mut t) = world.get_mut::<Transform>(entity) {
        t.translation = feet;
    }
    if let Some(mut p) = world.get_mut::<PreviousFeet>(entity) {
        p.0 = feet;
    }
}

/// Aims the player at the dummy's chest.
fn aim_at_chest(world: &mut World, dummy: Entity) {
    let Some(player) = player_entity(world) else {
        return;
    };
    let (Some(pt), Some(eye), Some(dt)) = (
        world.get::<Transform>(player),
        world.get::<EyeHeight>(player),
        world.get::<Transform>(dummy),
    ) else {
        return;
    };
    let from = pt.translation + Vec3::Y * eye.0;
    let to = dt.translation + Vec3::Y * CHEST;
    let look = look_toward(to - from);
    set_look(world, look.yaw, look.pitch);
}

fn tool(world: &mut World) -> Option<ActiveTool> {
    let player = player_entity(world)?;
    world.get::<ActiveTool>(player).copied()
}

fn loadout(world: &mut World) -> Option<Loadout> {
    let player = player_entity(world)?;
    world.get::<Loadout>(player).cloned()
}

fn total_health(world: &World, e: Entity) -> f32 {
    world.get::<Health>(e).map_or(0.0, |h| h.total())
}

fn secs(ticks: u64) -> f64 {
    ticks as f64 * TICK_SECONDS as f64
}

impl Ttk {
    fn go(&mut self, step: Step, clock: &ScenarioClock) {
        self.step = Some(step);
        self.step_started = clock.seconds;
        self.frames_in_step = 0;
    }

    fn step_time(&self, clock: &ScenarioClock) -> f64 {
        clock.seconds - self.step_started
    }

    /// Makes `want` the held tool with a full magazine; true once ready.
    fn ready_gun(&mut self, world: &mut World, want: ActiveTool, clock: &ScenarioClock) -> bool {
        if tool(world) != Some(want) {
            with_intent(world, |i| i.select = Some(want));
            return false;
        }
        let Some(lo) = loadout(world) else {
            return false;
        };
        if lo.is_switching() {
            return false;
        }
        let ActiveTool::Weapon(kind) = want else {
            return true;
        };
        let magazine = world.resource::<Tuning>().combat.gun(kind).magazine;
        let gun = lo.gun(kind);
        if gun.is_reloading() {
            return false;
        }
        if gun.ammo < magazine {
            with_intent(world, |i| i.reload_pressed = true);
            return false;
        }
        // Let the fire-rate cooldown and switch lock settle.
        self.step_time(clock) > 0.3 && gun.ready()
    }

    /// Puts a full-health dummy at `range` in front of the player; false while
    /// it is still down.
    fn stand_dummy(&mut self, world: &mut World, range: f32) -> Option<Entity> {
        let dummy = dummy_entity(world)?;
        if world.get::<Downed>(dummy).is_some() {
            return None;
        }
        let feet = self.player_feet + Vec3::NEG_Z * range;
        place_character(world, dummy, feet);
        teleport(world, self.player_feet);
        Some(dummy)
    }
}

impl Director for Ttk {
    fn update(&mut self, world: &mut World, clock: &ScenarioClock) -> DirectorStatus {
        let tick = world.resource::<SimTick>().0;
        let player = player_entity(world);
        let dummy = dummy_entity(world);
        let damage = self.messages.damage.read(world);
        let kills = self.messages.kills.read(world);
        let shots = self.messages.shots.read(world);
        let pieces = self.messages.pieces.read(world);
        let now = Instant::now();
        self.frames_in_step += 1;
        let (Some(player), Some(dummy)) = (player, dummy) else {
            return DirectorStatus::Running;
        };
        let step = *self.step.get_or_insert(Step::Start);
        let timed_out = self.step_time(clock) > TIMEOUT;

        match step {
            Step::Start => {
                world.resource_mut::<Tuning>().dummy.stand_still = true;
                self.player_feet = world.resource::<ArenaLayout>().player_spawn;
                teleport(world, self.player_feet);
                // Let the first frames (shader warm-up) pass.
                if clock.seconds > 2.0 {
                    self.go(Step::RifleSetup, clock);
                }
            }
            Step::RifleSetup => {
                let gun_ready = self.ready_gun(world, RIFLE, clock);
                let Some(dummy) = self.stand_dummy(world, RANGE) else {
                    return DirectorStatus::Running;
                };
                aim_at_chest(world, dummy);
                if gun_ready && self.frames_in_step > 20 {
                    if !self.captured_rifle {
                        snapshot(world, "ttk-rifle-aim");
                        self.captured_rifle = true;
                    }
                    let range_m = world
                        .get::<Transform>(dummy)
                        .map_or(0.0, |t| t.translation.distance(self.player_feet));
                    self.kills.push(RifleKill {
                        // The trigger is consumed by the next fixed tick.
                        trigger_tick: tick + 1,
                        trigger_at: Some(now),
                        start_total_health: total_health(world, dummy),
                        range_m,
                        ..default()
                    });
                    with_intent(world, |i| {
                        i.fire = true;
                        i.fire_pressed = true;
                    });
                    self.go(Step::RifleFire, clock);
                } else if timed_out {
                    self.notes.push("rifle setup timed out".into());
                    self.go(Step::PumpSetup, clock);
                }
            }
            Step::RifleFire => {
                aim_at_chest(world, dummy);
                with_intent(world, |i| i.fire = true);
                let kill = self.kills.last_mut().expect("a kill in progress");
                kill.shots += shots.iter().filter(|s| s.shooter == player).count() as u32;
                for d in damage
                    .iter()
                    .filter(|d| d.target == dummy && d.source == Some(player))
                {
                    kill.hits += 1;
                    kill.headshots += d.headshot as u32;
                    if kill.first_damage_tick.is_none() {
                        kill.first_damage_tick = Some(d.tick);
                        kill.first_damage_seen = Some(now);
                    }
                }
                if let Some(e) = kills.iter().find(|e| e.victim == dummy) {
                    kill.kill_tick = Some(e.tick);
                    kill.kill_seen = Some(now);
                    kill.readout_ttk_s = world.resource::<CombatStats>().last_ttk;
                    with_intent(world, |i| i.fire = false);
                    let next = if self.kills.len() >= KILLS {
                        Step::PumpSetup
                    } else {
                        Step::RifleSetup
                    };
                    self.go(next, clock);
                } else if timed_out {
                    with_intent(world, |i| i.fire = false);
                    self.notes
                        .push(format!("rifle kill {} timed out", self.kills.len()));
                    let next = if self.kills.len() >= KILLS {
                        Step::PumpSetup
                    } else {
                        Step::RifleSetup
                    };
                    self.go(next, clock);
                }
            }
            Step::PumpSetup => {
                let shot = self.pumps.len();
                if shot >= PUMP_RANGES.len() {
                    self.go(Step::WallSetup, clock);
                    return DirectorStatus::Running;
                }
                let range = PUMP_RANGES[shot];
                let gun_ready = self.ready_gun(world, PUMP, clock);
                let Some(dummy) = self.stand_dummy(world, range) else {
                    return DirectorStatus::Running;
                };
                if let Some(mut h) = world.get_mut::<Health>(dummy) {
                    h.reset();
                }
                aim_at_chest(world, dummy);
                if gun_ready && self.frames_in_step > 30 {
                    self.pumps.push(PumpShot {
                        range_m: range,
                        start_total_health: total_health(world, dummy),
                        ..default()
                    });
                    with_intent(world, |i| {
                        i.fire = true;
                        i.fire_pressed = true;
                    });
                    self.go(Step::PumpShot, clock);
                } else if timed_out {
                    self.notes.push(format!("pump setup {shot} timed out"));
                    self.go(Step::WallSetup, clock);
                }
            }
            Step::PumpShot => {
                with_intent(world, |i| i.fire = false);
                let pump = self.pumps.last_mut().expect("a pump shot in progress");
                for s in shots.iter().filter(|s| s.shooter == player) {
                    pump.pellets_fired += s.traces.len();
                    pump.pellets_hit += s.traces.iter().filter(|t| t.hit == Some(dummy)).count();
                }
                for d in damage
                    .iter()
                    .filter(|d| d.target == dummy && d.source == Some(player))
                {
                    pump.damage += d.amount;
                    pump.headshot |= d.headshot;
                    pump.killed |= d.killed;
                }
                pump.killed |= kills.iter().any(|e| e.victim == dummy);
                if self.frames_in_step > 30 {
                    pump.health_after = world
                        .get::<Health>(dummy)
                        .map_or((0.0, 0.0), |h| (h.hp, h.shield));
                    self.go(Step::PumpSetup, clock);
                }
            }
            Step::WallSetup => {
                let gun_ready = self.ready_gun(world, RIFLE, clock);
                let Some(dummy) = self.stand_dummy(world, RANGE) else {
                    return DirectorStatus::Running;
                };
                if let Some(mut h) = world.get_mut::<Health>(dummy) {
                    h.reset();
                }
                aim_at_chest(world, dummy);
                if self.wall.wall.is_none() && self.wall.placement_error.is_none() {
                    // The north edge of the cell 60% of the way to the dummy.
                    let dummy_feet = self.player_feet + Vec3::NEG_Z * RANGE;
                    let cell = GridCell::containing(self.player_feet.lerp(dummy_feet, 0.6));
                    let slot = PieceSlot::wall(cell, Facing::North);
                    self.wall.slot = format!("{slot:?}");
                    self.wall.wall_center = slot.center();
                    match place_piece(world, slot) {
                        Ok(e) => {
                            self.wall.wall = Some(e);
                            self.wall.placed = true;
                        }
                        Err(why) => {
                            self.wall.placement_error = Some(format!("{why:?}"));
                            self.notes.push(format!("wall not placed: {why:?}"));
                        }
                    }
                }
                if self.wall.placement_error.is_some() {
                    self.go(Step::Done, clock);
                } else if gun_ready && self.frames_in_step > 20 {
                    snapshot(world, "ttk-wall");
                    self.wall.trigger_tick = tick + 1;
                    with_intent(world, |i| {
                        i.fire = true;
                        i.fire_pressed = true;
                    });
                    self.go(Step::WallFire, clock);
                } else if timed_out {
                    self.notes.push("wall setup timed out".into());
                    self.go(Step::Done, clock);
                }
            }
            Step::WallFire => {
                aim_at_chest(world, dummy);
                with_intent(world, |i| i.fire = true);
                let w = &mut self.wall;
                let wall = w.wall;
                for d in &damage {
                    if d.target_kind == DamageTarget::Piece && Some(d.target) == wall {
                        w.hits_on_wall += 1;
                        w.first_hit_tick.get_or_insert(d.tick);
                    }
                    if d.target == dummy && d.source == Some(player) {
                        if w.break_tick.is_none() {
                            w.damage_to_dummy_before_break += d.amount;
                        }
                        w.dummy_hit_tick.get_or_insert(d.tick);
                    }
                }
                for p in &pieces {
                    if Some(p.entity) == wall && p.change == PieceChange::Destroyed {
                        w.break_tick.get_or_insert(p.tick);
                    }
                }
                if w.dummy_hit_tick.is_some() || timed_out {
                    with_intent(world, |i| i.fire = false);
                    if timed_out {
                        self.notes.push("wall soak timed out".into());
                    }
                    self.go(Step::Done, clock);
                }
            }
            Step::Done => {
                with_intent(world, |i| i.fire = false);
                if self.frames_in_step > 10 {
                    return DirectorStatus::Done;
                }
            }
        }
        DirectorStatus::Running
    }

    fn warmup_seconds(&self) -> f64 {
        2.0
    }

    fn summary(&mut self, world: &mut World) -> Value {
        let kills: Vec<Value> = self
            .kills
            .iter()
            .enumerate()
            .map(|(i, k)| {
                let ttk = k
                    .first_damage_tick
                    .zip(k.kill_tick)
                    .map(|(a, b)| secs(b - a));
                json!({
                    "kill": i + 1,
                    "range_m": k.range_m,
                    "start_total_health": k.start_total_health,
                    "ttk_first_damage_to_kill_s": ttk,
                    "combat_stats_last_ttk_s": k.readout_ttk_s,
                    "trigger_to_kill_s": k.kill_tick.map(|b| secs(b.saturating_sub(k.trigger_tick))),
                    "wall_clock_first_damage_to_kill_s": k.first_damage_seen.zip(k.kill_seen).map(|(a, b)| (b - a).as_secs_f64()),
                    "wall_clock_trigger_to_kill_s": k.trigger_at.zip(k.kill_seen).map(|(a, b)| (b - a).as_secs_f64()),
                    "shots": k.shots,
                    "hits": k.hits,
                    "headshots": k.headshots,
                    "killed": k.kill_tick.is_some(),
                })
            })
            .collect();
        // A kill that never happened counts as out of range.
        let ttks: Vec<f64> = self
            .kills
            .iter()
            .map(|k| {
                k.first_damage_tick
                    .zip(k.kill_tick)
                    .map_or(f64::INFINITY, |(a, b)| secs(b - a))
            })
            .collect();
        let finite: Vec<f64> = ttks.iter().copied().filter(|t| t.is_finite()).collect();
        let stats = telemetry::latency_stats(&finite);
        let pumps: Vec<Value> = self
            .pumps
            .iter()
            .map(|p| {
                json!({
                    "range_m": p.range_m,
                    "start_total_health": p.start_total_health,
                    "damage": p.damage,
                    "pellets_fired": p.pellets_fired,
                    "pellets_hit_dummy": p.pellets_hit,
                    "any_headshot_pellet": p.headshot,
                    "killed": p.killed,
                    "survived": !p.killed,
                    "health_after": {"hp": p.health_after.0, "shield": p.health_after.1},
                })
            })
            .collect();
        let pump_kills: Vec<bool> = self
            .pumps
            .iter()
            .filter(|p| p.start_total_health >= 200.0 - 1e-3 && p.damage > 0.0)
            .map(|p| p.killed)
            .collect();
        let w = &self.wall;
        let soak = w.first_hit_tick.zip(w.break_tick).map(|(a, b)| secs(b - a));
        let gate = telemetry::evaluate_g6(&ttks, &pump_kills, soak);
        let tuning = world.resource::<Tuning>();
        json!({
            "measures": "G6 time to kill. A standing dummy (stand_still) at 15 m, full 100 HP + 100 shield, under continuous rifle fire aimed at its chest; one point-blank pump body shot per range on a full-health dummy; a wall placed between the player and the dummy under continuous rifle fire. Times are simulation ticks (1/60 s) from the damage, elimination and piece messages; wall-clock times of the observing frames are included for reference.",
            "setup": {
                "player_feet": [self.player_feet.x, self.player_feet.y, self.player_feet.z],
                "range_m": RANGE,
                "aim": format!("dummy chest, {CHEST} m above its feet"),
                "dummy_stand_still": tuning.dummy.stand_still,
                "rifle": {"damage": tuning.combat.rifle.damage, "fire_interval_s": tuning.combat.rifle.fire_interval, "structure_damage": tuning.combat.rifle.structure_damage},
                "wall_hp": tuning.building.wall_hp,
            },
            "rifle_kills": kills,
            "rifle_ttk_s": stats,
            "pump_shots": pumps,
            "wall_soak": {
                "slot": w.slot,
                "wall_center": [w.wall_center.x, w.wall_center.y, w.wall_center.z],
                "placed": w.placed,
                "placement_error": w.placement_error,
                "hits_on_wall": w.hits_on_wall,
                "first_hit_to_break_s": soak,
                "trigger_to_break_s": w.break_tick.map(|b| secs(b.saturating_sub(w.trigger_tick))),
                "first_hit_to_first_dummy_hit_s": w.first_hit_tick.zip(w.dummy_hit_tick).map(|(a, b)| secs(b - a)),
                "damage_to_dummy_before_break": w.damage_to_dummy_before_break,
            },
            "g6_thresholds": {
                "rifle_ttk_s_min": telemetry::g6::RIFLE_TTK_MIN_S,
                "rifle_ttk_s_max": telemetry::g6::RIFLE_TTK_MAX_S,
                "min_rifle_kills": telemetry::g6::MIN_RIFLE_KILLS,
                "pump_kills_from_full_allowed": 0,
                "wall_soak_s_min": telemetry::g6::WALL_SOAK_MIN_S,
            },
            "gate": gate,
            "notes": self.notes,
        })
    }
}
