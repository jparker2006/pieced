//! Slice C — the training dummy: strafing pattern driven through `PlayerIntent`,
//! elimination and respawn.
//!
//! The dummy is an ordinary [`Character`](crate::shared::Character) with the
//! [`Dummy`] marker. Its controller runs in [`SimSet::Control`] and only writes the
//! dummy's [`PlayerIntent`] and [`LookAngles`]; movement applies them like the
//! player's. Combat marks it [`Downed`] when eliminated; [`SimSet::Resolve`]
//! respawns it after `respawn_delay` somewhere away from the player.
//!
//! While the gallery's [`GalleryFreeze`] is set the dummy stops moving: it
//! neither strafes nor respawns, and [`hold_frozen_dummies`] keeps its feet
//! where the freeze found them (mid-jump included).

use crate::{
    arena::ArenaLayout,
    player,
    rng::{Rng, SimRng},
    shared::{
        Character, EyeHeight, GalleryFreeze, GameCue, Health, Layer, LookAngles, Player,
        PlayerIntent, PreviousFeet, SimSet, SimTick, TICK_SECONDS,
    },
    tuning::Tuning,
};
use avian3d::prelude::*;
use bevy::{platform::collections::HashMap, prelude::*};
use serde::{Deserialize, Serialize};

pub use crate::combat::Downed;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DummyTuning {
    pub stand_still: bool,
    /// Fraction of the player's run speed used while strafing (1.0 = same speed).
    pub speed_scale: f32,
    pub respawn_delay: f32,
    pub jump_chance_per_sec: f32,
    pub turn_interval_min: f32,
    pub turn_interval_max: f32,
    /// Respawn at least this far from the player (m).
    pub respawn_min_distance: f32,
}

impl Default for DummyTuning {
    fn default() -> Self {
        Self {
            stand_still: false,
            speed_scale: 1.0,
            respawn_delay: 2.0,
            jump_chance_per_sec: 0.15,
            turn_interval_min: 0.4,
            turn_interval_max: 1.6,
            respawn_min_distance: 12.0,
        }
    }
}

/// Marks the training dummy character.
#[derive(Component, Debug, Default)]
pub struct Dummy;

/// The dummy's controller state: seeded randomness, strafe direction and timers.
#[derive(Component, Debug, Clone)]
pub struct DummyBrain {
    rng: Rng,
    /// +1 strafes right, -1 strafes left (relative to its facing).
    pub strafe: f32,
    /// Seconds until the next direction change.
    pub turn_in: f32,
    /// Seconds the jump button stays held after a jump press.
    jump_hold: f32,
}

impl DummyBrain {
    pub fn new(mut rng: Rng, tuning: &DummyTuning) -> Self {
        let strafe = if rng.chance(0.5) { 1.0 } else { -1.0 };
        let turn_in = next_turn(&mut rng, tuning);
        Self {
            rng,
            strafe,
            turn_in,
            jump_hold: 0.0,
        }
    }
}

fn next_turn(rng: &mut Rng, tuning: &DummyTuning) -> f32 {
    let lo = tuning.turn_interval_min.min(tuning.turn_interval_max);
    let hi = tuning.turn_interval_min.max(tuning.turn_interval_max);
    rng.range(lo, hi)
}

/// Salt for the dummy's controller RNG stream.
const DUMMY_SALT: u64 = 0xD0_33EE;
/// How long a jump press is held.
const JUMP_HOLD: f32 = 0.25;
/// The dummy turns back when a strafe would carry it this close to the bounds.
const EDGE_LOOKAHEAD: f32 = 1.5;

pub struct DummyPlugin;

impl Plugin for DummyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_dummy)
            .add_systems(FixedUpdate, drive_dummies.in_set(SimSet::Control))
            .add_systems(
                FixedUpdate,
                (respawn_dummies, hold_frozen_dummies)
                    .chain()
                    .in_set(SimSet::Resolve),
            );
    }
}

fn spawn_dummy(
    mut commands: Commands,
    layout: Res<ArenaLayout>,
    tuning: Res<Tuning>,
    sim_rng: Res<SimRng>,
) {
    let to_player = layout.player_spawn - layout.dummy_spawn;
    let look = look_toward(to_player);
    let brain = DummyBrain::new(sim_rng.0.clone().fork(DUMMY_SALT), &tuning.dummy);
    player::spawn_character(
        &mut commands,
        layout.dummy_spawn,
        look,
        Health::full(tuning.combat.max_hp, tuning.combat.max_shield),
        (Dummy, brain),
    );
}

/// Look angles that aim along `dir`.
pub fn look_toward(dir: Vec3) -> LookAngles {
    let Some(d) = dir.try_normalize() else {
        return LookAngles::default();
    };
    LookAngles {
        yaw: (-d.x).atan2(-d.z).rem_euclid(std::f32::consts::TAU),
        pitch: d
            .y
            .clamp(-1.0, 1.0)
            .asin()
            .clamp(-LookAngles::PITCH_LIMIT, LookAngles::PITCH_LIMIT),
    }
}

fn outside_by(point: Vec2, min: Vec2, max: Vec2) -> f32 {
    let below = (min - point).max(Vec2::ZERO);
    let above = (point - max).max(Vec2::ZERO);
    (below + above).length()
}

fn drive_dummies(
    tuning: Res<Tuning>,
    layout: Res<ArenaLayout>,
    freeze: Option<Res<GalleryFreeze>>,
    players: Query<(&Transform, &EyeHeight), (With<Player>, Without<Dummy>)>,
    mut dummies: Query<
        (
            &Transform,
            &EyeHeight,
            &mut LookAngles,
            &mut PlayerIntent,
            &mut DummyBrain,
            Has<Downed>,
        ),
        With<Dummy>,
    >,
) {
    let dt = TICK_SECONDS;
    let dt_tuning = &tuning.dummy;
    let player = players.iter().next();
    for (transform, eye, mut look, mut intent, mut brain, downed) in &mut dummies {
        if downed || freeze.is_some() {
            *intent = PlayerIntent::default();
            continue;
        }
        if let Some((ptf, peye)) = player {
            let from = transform.translation + Vec3::Y * eye.0;
            let to = ptf.translation + Vec3::Y * peye.0;
            let new_look = look_toward(to - from);
            if *look != new_look {
                *look = new_look;
            }
        }
        if dt_tuning.stand_still {
            intent.move_axis = Vec2::ZERO;
            intent.jump = false;
            intent.jump_pressed = false;
            intent.sprint = false;
            continue;
        }
        let brain = &mut *brain;

        // Random direction changes.
        brain.turn_in -= dt;
        if brain.turn_in <= 0.0 {
            brain.strafe = -brain.strafe;
            brain.turn_in = next_turn(&mut brain.rng, dt_tuning);
        }

        // Turn back before strafing out of the arena.
        let (_, right) = look.flat_basis();
        let feet = transform.translation.xz();
        let ahead = |s: f32| {
            outside_by(
                feet + right.xz() * s * EDGE_LOOKAHEAD,
                layout.bounds_min,
                layout.bounds_max,
            )
        };
        if ahead(brain.strafe) > 0.0 && ahead(-brain.strafe) < ahead(brain.strafe) {
            brain.strafe = -brain.strafe;
            brain.turn_in = next_turn(&mut brain.rng, dt_tuning);
        }

        // Occasional jumps (held briefly after the press).
        if brain.jump_hold > 0.0 {
            brain.jump_hold -= dt;
        } else if brain.rng.chance(dt_tuning.jump_chance_per_sec * dt) {
            brain.jump_hold = JUMP_HOLD;
            intent.jump_pressed = true;
        }
        intent.jump = brain.jump_hold > 0.0;

        intent.sprint = false;
        intent.move_axis = Vec2::new(brain.strafe * dt_tuning.speed_scale.clamp(0.0, 1.0), 0.0);
    }
}

/// Picks a respawn spot inside the bounds, at least `min_distance` (horizontal)
/// from `player`, where `is_clear` holds. Falls back to the bounds corner
/// farthest from the player.
pub fn pick_respawn_spot(
    rng: &mut Rng,
    layout: &ArenaLayout,
    player: Vec3,
    min_distance: f32,
    is_clear: impl Fn(Vec3) -> bool,
) -> Vec3 {
    let margin = 1.0;
    let min = layout.bounds_min + Vec2::splat(margin);
    let max = layout.bounds_max - Vec2::splat(margin);
    for _ in 0..64 {
        let spot = Vec3::new(rng.range(min.x, max.x), 0.0, rng.range(min.y, max.y));
        if spot.xz().distance(player.xz()) >= min_distance && is_clear(spot) {
            return spot;
        }
    }
    [
        Vec2::new(min.x, min.y),
        Vec2::new(min.x, max.y),
        Vec2::new(max.x, min.y),
        Vec2::new(max.x, max.y),
    ]
    .into_iter()
    .max_by(|a, b| {
        a.distance_squared(player.xz())
            .total_cmp(&b.distance_squared(player.xz()))
    })
    .map(|c| Vec3::new(c.x, 0.0, c.y))
    .unwrap_or(layout.dummy_spawn)
}

fn respawn_dummies(
    mut commands: Commands,
    freeze: Option<Res<GalleryFreeze>>,
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    layout: Res<ArenaLayout>,
    spatial: SpatialQuery,
    collider_of: Query<&ColliderOf>,
    characters: Query<(), With<Character>>,
    players: Query<&Transform, (With<Player>, Without<Dummy>)>,
    mut dummies: Query<
        (
            Entity,
            &mut Transform,
            &mut PreviousFeet,
            &mut Health,
            &mut DummyBrain,
            &mut PlayerIntent,
            &Downed,
        ),
        With<Dummy>,
    >,
    mut cues: MessageWriter<GameCue>,
) {
    if freeze.is_some() {
        return;
    }
    let delay_ticks = (tuning.dummy.respawn_delay / TICK_SECONDS).round().max(0.0) as u64;
    let player = players
        .iter()
        .next()
        .map_or(layout.player_spawn, |t| t.translation);
    let blockers = SpatialQueryFilter::from_mask([Layer::World, Layer::Piece]);
    let body = Collider::capsule(0.35, 1.1);
    for (entity, mut transform, mut prev, mut health, mut brain, mut intent, downed) in &mut dummies
    {
        if tick.0.saturating_sub(downed.tick) < delay_ticks {
            continue;
        }
        let spot = pick_respawn_spot(
            &mut brain.rng,
            &layout,
            player,
            tuning.dummy.respawn_min_distance,
            |spot| {
                // A capsule from 0.1 m to 1.9 m above the feet must touch no world
                // geometry or piece (characters' own colliders don't count).
                let center = spot + Vec3::Y * 1.0;
                let mut clear = true;
                spatial.shape_intersections_callback(
                    &body,
                    center,
                    Quat::IDENTITY,
                    &blockers,
                    |e| {
                        let owned = collider_of
                            .get(e)
                            .is_ok_and(|c| characters.contains(c.body));
                        clear &= owned;
                        clear
                    },
                );
                clear
            },
        );
        transform.translation = spot;
        prev.0 = spot;
        health.reset();
        *intent = PlayerIntent::default();
        brain.turn_in = next_turn(&mut brain.rng, &tuning.dummy);
        brain.jump_hold = 0.0;
        commands.entity(entity).remove::<Downed>();
        cues.write(GameCue::Respawned { who: entity });
    }
}

/// While [`GalleryFreeze`] is set, puts every dummy's feet back where the freeze
/// found them (their position at the start of the first frozen tick) after
/// movement has run, so the figure holds still for the capture, mid-jump
/// included. Its velocity is left to movement; it resumes when thawed.
pub fn hold_frozen_dummies(
    freeze: Option<Res<GalleryFreeze>>,
    mut pins: Local<HashMap<Entity, Vec3>>,
    mut dummies: Query<(Entity, &mut Transform, &PreviousFeet), With<Dummy>>,
) {
    if freeze.is_none() {
        pins.clear();
        return;
    }
    for (entity, mut transform, previous) in &mut dummies {
        let pin = *pins.entry(entity).or_insert(previous.0);
        if transform.translation != pin {
            transform.translation = pin;
        }
    }
}
