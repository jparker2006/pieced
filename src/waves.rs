//! The Waves mode: runs, waves, the break, score and results (docs/M3-SPEC.md →
//! Waves, D79–D85). Only active in [`GameMode::Waves`](crate::shared::GameMode).
//!
//! Chunk 1 (slice C) builds the minimal run: 3 grunts poof in at seeded points
//! on the island edge, the player's elimination ends the run, a plain results
//! line with a restart key. Chunk 2 grows it into the full wave director.
//!
//! Grunt characters come from a fixed pool of `max_alive` characters created at
//! run start (parked and inactive), reused for every spawn, so a wave spawn
//! never instances a model mid-fight (the knight figure is rigged once).

pub mod ui;

use crate::{
    arena::ArenaLayout,
    building::{self, InitialCover},
    combat::{CombatStats, Downed, GunState, Loadout},
    dummy::look_toward,
    grunt::{self, AttackTokens, GruntBrain, GruntRng, GruntStats, Parked},
    movement::{Knockback, Motor},
    orb::Wand,
    rng::{Rng, SimRng},
    shared::{
        Ads, AppState, Character, EyeHeight, GameCue, GameMode, Health, Layer, LookAngles, Player,
        PlayerIntent, PreviousFeet, SimSet, SimTick, TICK_SECONDS,
    },
    tuning::Tuning,
};
use avian3d::prelude::*;
use bevy::{
    ecs::{message::Messages, query::QueryData},
    prelude::*,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct WavesTuning {
    /// Grunts in wave 1, and how many each later wave adds (D68).
    pub first_wave: u32,
    pub per_wave: u32,
    /// Knights alive (or in a ship's beam) at once (D77).
    pub max_alive: u32,
    /// The break between waves (s), and the shield it refills at its start (D79).
    pub break_seconds: f32,
    pub break_shield: f32,
    /// Shield potions (D80).
    pub potion_chance: f32,
    pub potion_shield: f32,
    pub potion_lifetime: f32,
    pub potion_pickup_radius: f32,
    /// Score (D81).
    pub score_kill: u32,
    pub score_headshot: u32,
    pub score_void: u32,
    pub score_wave: u32,
    /// Per-knight speed jitter, ± this fraction (D83).
    pub speed_jitter: f32,
    /// The death beat: time scale and real seconds (D84).
    pub death_time_scale: f32,
    pub death_seconds: f32,
}

impl Default for WavesTuning {
    fn default() -> Self {
        Self {
            first_wave: 3,
            per_wave: 2,
            max_alive: 8,
            break_seconds: 10.0,
            break_shield: 50.0,
            potion_chance: 0.1,
            potion_shield: 25.0,
            potion_lifetime: 20.0,
            potion_pickup_radius: 1.0,
            score_kill: 100,
            score_headshot: 50,
            score_void: 150,
            score_wave: 250,
            speed_jitter: 0.1,
            death_time_scale: 0.3,
            death_seconds: 1.0,
        }
    }
}

impl WavesTuning {
    /// Grunts in wave `wave` (1-based).
    pub fn wave_size(&self, wave: u32) -> u32 {
        self.first_wave + self.per_wave * wave.max(1).saturating_sub(1)
    }
}

// ---------------------------------------------------------------------------
// The run (chunk 1: one grunt wave, looping)
// ---------------------------------------------------------------------------

/// Seconds from a run's (or wave's) start to its first grunt.
pub const FIRST_SPAWN_DELAY: f32 = 1.0;
/// Seconds between grunts poofing in (chunk 1's stand-in for the beam, D82).
pub const SPAWN_STAGGER: f32 = 0.4;
/// Seconds a downed grunt lies (the KO take, the hat drop) before it goes back
/// to the pool.
pub const RETURN_DELAY: f32 = 1.0;
/// Seconds "Wave n cleared!" shows before the next wave starts (chunk 1: wave
/// 1 again; chunk 2 replaces this with the break).
pub const CLEARED_PAUSE: f32 = 3.0;
/// Grunts poof in on a ring this far inside the playable bounds (m): just
/// inside the island edge.
pub const EDGE_INSET: f32 = 2.0;
/// ... at least this far (horizontally) from the player (m) ...
pub const SPAWN_MIN_PLAYER_DISTANCE: f32 = 12.0;
/// ... and from any other grunt in play (m).
pub const SPAWN_SPACING: f32 = 3.0;
/// Where parked grunts wait: under the island, out of sight and out of play.
pub const PARK_SPOT: Vec3 = Vec3::new(0.0, -60.0, 0.0);
/// Salt for the run-seed stream.
const WAVES_SALT: u64 = 0x3A7E_5EED;

/// Whole ticks covering `seconds`.
fn ticks(seconds: f32) -> u64 {
    (seconds / TICK_SECONDS).round().max(0.0) as u64
}

/// Where the run stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunPhase {
    /// The wave's grunts are poofing in or fighting.
    Fighting,
    /// Every grunt of the wave went down at `tick`; the next wave starts after
    /// [`CLEARED_PAUSE`].
    Cleared { tick: u64 },
    /// The player was eliminated at `tick` (one life per run). The run waits
    /// for a restart ([`RestartRun`]).
    Over { tick: u64 },
}

/// The current run (Waves mode only; absent in Practice).
#[derive(Resource, Debug, Clone)]
pub struct Run {
    /// Drives spawn points and speed jitter (D83). A restart draws the next
    /// run's seed from this run's stream.
    pub seed: u64,
    pub wave: u32,
    pub phase: RunPhase,
    /// Grunts in play: poofed in and not downed.
    pub alive: u32,
    /// Grunts of this wave still to poof in.
    pub remaining: u32,
    /// Grunts downed this run.
    pub eliminations: u32,
    /// Placeholder until chunk 2's D81 score: `score_kill` per elimination.
    pub score: u32,
    pub waves_cleared: u32,
    pub started_tick: u64,
    next_spawn_tick: u64,
    rng: Rng,
}

impl Run {
    pub fn new(seed: u64, tick: u64, tuning: &WavesTuning) -> Self {
        let mut run = Self {
            seed,
            wave: 1,
            phase: RunPhase::Fighting,
            alive: 0,
            remaining: 0,
            eliminations: 0,
            score: 0,
            waves_cleared: 0,
            started_tick: tick,
            next_spawn_tick: tick,
            rng: Rng::new(seed),
        };
        run.start_wave(1, tick, tuning);
        run
    }

    fn start_wave(&mut self, wave: u32, tick: u64, tuning: &WavesTuning) {
        self.wave = wave;
        self.remaining = tuning.wave_size(wave);
        self.phase = RunPhase::Fighting;
        self.next_spawn_tick = tick + ticks(FIRST_SPAWN_DELAY);
    }

    pub fn is_over(&self) -> bool {
        matches!(self.phase, RunPhase::Over { .. })
    }

    /// Knights left in the wave: in play plus still to come.
    pub fn left(&self) -> u32 {
        self.alive + self.remaining
    }
}

/// Starts a new run in place ("Go again"): pieces back to the initial cover,
/// orbs gone, grunts parked, the player at spawn at full health, a new seed.
/// Written by the input adapter (Enter on the results line) or by tests.
#[derive(Message, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RestartRun;

/// A member of the grunt pool (`max_alive` characters created at startup and
/// reused for every spawn, so a spawn never instances a knight mid-fight).
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PoolGrunt {
    /// Tick it went down, while it lies before returning to the pool.
    pub downed_at: Option<u64>,
}

/// Run condition: a Waves run exists and the player has been eliminated.
pub fn run_over(run: Option<Res<Run>>) -> bool {
    run.is_some_and(|run| run.is_over())
}

/// Test-only switch: Waves mode without the wave director (no run, no pool),
/// for tests that place their own knights (`Sim::grunt_lab`).
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct NoWaveDirector;

/// The run: grunt pool, wave spawning, the player's death and restart.
pub struct WavesPlugin;

impl Plugin for WavesPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<RestartRun>()
            .add_systems(
                Startup,
                (start_run, register_restart)
                    .run_if(resource_equals(GameMode::Waves))
                    .run_if(not(resource_exists::<NoWaveDirector>)),
            )
            .add_systems(
                FixedUpdate,
                (
                    restart_run
                        .before(SimSet::Control)
                        .run_if(in_state(AppState::Playing)),
                    silence_dead_player.in_set(SimSet::Control),
                    run_director.in_set(SimSet::Resolve),
                )
                    .run_if(resource_exists::<Run>),
            );
    }
}

fn start_run(
    mut commands: Commands,
    tuning: Res<Tuning>,
    sim_rng: Res<SimRng>,
    tick: Res<SimTick>,
) {
    let seed = sim_rng.0.clone().fork(WAVES_SALT).next_u64();
    commands.insert_resource(Run::new(seed, tick.0, &tuning.waves));
    let stats = GruntStats::for_wave(1, &tuning.grunt);
    for _ in 0..tuning.waves.max_alive {
        spawn_pool_member(&mut commands, stats);
    }
}

/// Registers the restart's reset up front, so the first "Go again" allocates
/// nothing.
fn register_restart(world: &mut World) {
    world.register_system_cached(reset_characters);
}

/// Creates one parked pool grunt (a full grunt, brain included).
fn spawn_pool_member(commands: &mut Commands, stats: GruntStats) -> Entity {
    let grunt = grunt::spawn_grunt(commands, PARK_SPOT, LookAngles::default(), stats);
    commands
        .entity(grunt)
        .insert((PoolGrunt::default(), Parked, Downed { tick: 0 }));
    grunt
}

/// A pool grunt's parts the run resets.
#[derive(QueryData)]
#[query_data(mutable)]
struct PoolParts {
    entity: Entity,
    slot: &'static mut PoolGrunt,
    transform: &'static mut Transform,
    previous: &'static mut PreviousFeet,
    look: &'static mut LookAngles,
    health: &'static mut Health,
    stats: &'static mut GruntStats,
    wand: &'static mut Wand,
    brain: &'static mut GruntBrain,
    intent: &'static mut PlayerIntent,
    motor: &'static mut Motor,
    parked: Has<Parked>,
    downed: Has<Downed>,
}

impl PoolPartsItem<'_, '_> {
    /// In play: poofed in, not downed, not frozen.
    fn in_play(&self) -> bool {
        !self.parked && !self.downed
    }

    /// Waiting in the pool, free for the next spawn.
    fn free(&self) -> bool {
        self.parked && self.downed && self.slot.downed_at.is_none()
    }

    /// Back to the pool: under the island, inert, hidden (downed), restored.
    fn park(&mut self, commands: &mut Commands, tick: u64) {
        self.transform.translation = PARK_SPOT;
        self.previous.0 = PARK_SPOT;
        *self.intent = PlayerIntent::default();
        *self.motor = Motor::default();
        self.health.reset();
        self.wand.windup = None;
        self.wand.cooldown = 0.0;
        self.slot.downed_at = None;
        commands
            .entity(self.entity)
            .insert((Parked, Downed { tick }))
            .remove::<Knockback>();
    }
}

/// Once the run is over, the player's controls do nothing (the results line's
/// Enter is read by the input adapter, not through `PlayerIntent`).
fn silence_dead_player(run: Res<Run>, mut players: Query<&mut PlayerIntent, With<Player>>) {
    if !run.is_over() {
        return;
    }
    for mut intent in &mut players {
        if *intent != PlayerIntent::default() {
            *intent = PlayerIntent::default();
        }
    }
}

/// The wave loop, after combat each tick: the player's death ends the run;
/// downed grunts count and return to the pool; grunts poof in at seeded edge
/// points; a wave with every grunt down is cleared and (chunk 1) starts again.
fn run_director(
    mut commands: Commands,
    mut run: ResMut<Run>,
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    layout: Res<ArenaLayout>,
    spatial: SpatialQuery,
    collider_of: Query<&ColliderOf>,
    characters: Query<(), With<Character>>,
    players: Query<(Entity, &Transform, &Health, Has<Downed>), With<Player>>,
    mut grunts: Query<PoolParts, (With<PoolGrunt>, Without<Player>)>,
    mut cues: MessageWriter<GameCue>,
) {
    let now = tick.0;
    let t = &tuning.waves;
    let run = &mut *run;
    let player = players.iter().next();

    // The player's elimination ends the run (one life, D84).
    if !run.is_over()
        && let Some((entity, _, health, downed)) = player
        && (downed || health.is_dead())
    {
        if !downed {
            commands.entity(entity).insert(Downed { tick: now });
        }
        run.phase = RunPhase::Over { tick: now };
    }

    // Downed grunts count once, lie for a beat, then go back to the pool.
    for mut grunt in &mut grunts {
        if grunt.parked || !grunt.downed {
            continue;
        }
        match grunt.slot.downed_at {
            None => {
                grunt.slot.downed_at = Some(now);
                run.eliminations += 1;
                run.score += t.score_kill;
            }
            Some(at) if now.saturating_sub(at) >= ticks(RETURN_DELAY) => {
                grunt.park(&mut commands, now);
            }
            Some(_) => {}
        }
    }
    run.alive = grunts.iter().filter(|g| !g.parked && !g.downed).count() as u32;

    match run.phase {
        RunPhase::Over { tick: over } => {
            // After the death beat the survivors stop where they stand (frozen
            // by `Parked` once they're on the ground, so none hangs mid-jump).
            if now.saturating_sub(over) >= ticks(t.death_seconds) {
                for mut grunt in &mut grunts {
                    if !grunt.in_play() {
                        continue;
                    }
                    *grunt.intent = PlayerIntent::default();
                    if grunt.motor.grounded {
                        commands.entity(grunt.entity).insert(Parked);
                    }
                }
            }
        }
        RunPhase::Cleared { tick: cleared } => {
            if now.saturating_sub(cleared) >= ticks(CLEARED_PAUSE) {
                // Chunk 1 loops wave 1; chunk 2's director counts up.
                let wave = run.wave;
                run.start_wave(wave, now, t);
            }
        }
        RunPhase::Fighting => {
            if run.remaining == 0 && run.alive == 0 {
                run.phase = RunPhase::Cleared { tick: now };
                run.waves_cleared += 1;
                run.score += t.score_wave * run.wave;
                return;
            }
            if run.remaining == 0 || now < run.next_spawn_tick || run.alive >= t.max_alive {
                return;
            }
            let player_feet = player.map_or(layout.player_spawn, |p| p.1.translation);
            let others: Vec<Vec3> = grunts
                .iter()
                .filter(|g| !g.parked)
                .map(|g| g.transform.translation)
                .collect();
            let blockers = SpatialQueryFilter::from_mask([Layer::World, Layer::Piece]);
            let body = Collider::capsule(0.4, 1.1);
            let is_clear = |spot: Vec3| {
                // A capsule from 0.05 m to 1.95 m above the feet must touch no
                // world geometry or piece (characters' own colliders don't count).
                let mut clear = true;
                spatial.shape_intersections_callback(
                    &body,
                    spot + Vec3::Y * 1.0,
                    Quat::IDENTITY,
                    &blockers,
                    |e| {
                        clear &= collider_of
                            .get(e)
                            .is_ok_and(|c| characters.contains(c.body));
                        clear
                    },
                );
                clear
            };
            let Some(mut grunt) = grunts.iter_mut().find(|g| g.free()) else {
                return;
            };
            let spot = edge_spot(&mut run.rng, &layout, player_feet, &others, is_clear);
            let mut stats = GruntStats::for_wave(run.wave, &tuning.grunt);
            let jitter = t.speed_jitter.abs();
            stats.speed =
                (stats.speed * (1.0 + run.rng.range(-jitter, jitter))).min(tuning.grunt.speed_cap);

            grunt.transform.translation = spot;
            grunt.previous.0 = spot;
            *grunt.look = look_toward((player_feet - spot).with_y(0.0));
            *grunt.health = Health::full(stats.hp, 0.0);
            *grunt.stats = stats;
            *grunt.wand = Wand::new(stats.fire_interval);
            *grunt.intent = PlayerIntent::default();
            *grunt.motor = Motor::default();
            grunt.slot.downed_at = None;
            // A fresh brain, seeded from the run so the same seed replays the run.
            grunt.brain.reset();
            grunt
                .brain
                .reseed(Rng::new(run.seed).fork(now ^ grunt.entity.to_bits().rotate_left(32)));
            commands
                .entity(grunt.entity)
                .remove::<(Parked, Downed, Knockback)>();
            // The knight pops in with its sparkle (chunk 1's poof).
            cues.write(GameCue::Respawned { who: grunt.entity });

            run.remaining -= 1;
            run.alive += 1;
            run.next_spawn_tick = now + ticks(SPAWN_STAGGER);
        }
    }
}

/// A seeded spawn point on the ring [`EDGE_INSET`] inside the bounds, at least
/// [`SPAWN_MIN_PLAYER_DISTANCE`] from `player` and [`SPAWN_SPACING`] from each
/// of `others`, where `is_clear` holds. Falls back to the ring corner farthest
/// from the player.
pub fn edge_spot(
    rng: &mut Rng,
    layout: &ArenaLayout,
    player: Vec3,
    others: &[Vec3],
    is_clear: impl Fn(Vec3) -> bool,
) -> Vec3 {
    let min = layout.bounds_min + Vec2::splat(EDGE_INSET);
    let max = layout.bounds_max - Vec2::splat(EDGE_INSET);
    let size = (max - min).max(Vec2::ZERO);
    let perimeter = 2.0 * (size.x + size.y);
    let on_ring = |d: f32| {
        let p = if d < size.x {
            Vec2::new(min.x + d, min.y)
        } else if d < size.x + size.y {
            Vec2::new(max.x, min.y + d - size.x)
        } else if d < 2.0 * size.x + size.y {
            Vec2::new(max.x - (d - size.x - size.y), max.y)
        } else {
            Vec2::new(min.x, max.y - (d - 2.0 * size.x - size.y))
        };
        Vec3::new(p.x, 0.0, p.y)
    };
    for _ in 0..64 {
        let spot = on_ring(rng.range(0.0, perimeter));
        let far = spot.xz().distance(player.xz()) >= SPAWN_MIN_PLAYER_DISTANCE;
        let spaced = others
            .iter()
            .all(|o| o.xz().distance(spot.xz()) >= SPAWN_SPACING);
        if far && spaced && is_clear(spot) {
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
    .map_or(PARK_SPOT, |c| Vec3::new(c.x, 0.0, c.y))
}

/// Takes pending [`RestartRun`] requests at the start of a fixed tick and
/// restarts the run in place: no reload, nothing respawned from scratch.
fn restart_run(world: &mut World) {
    let requested = world
        .get_resource_mut::<Messages<RestartRun>>()
        .is_some_and(|mut m| m.drain().count() > 0);
    if !requested {
        return;
    }
    // Orbs in flight vanish back into their fixed pool.
    crate::orb::recycle_all_orbs(world);
    if let Err(e) = world.run_system_cached(reset_characters) {
        error!("waves: restart failed: {e}");
        return;
    }
    // The grid back to the arena's initial cover (after the player is back at
    // spawn, which is off the cover).
    building::clear_pieces(world);
    for slot in building::initial_cover() {
        if let Ok(piece) = building::place_piece(world, slot) {
            world.entity_mut(piece).insert(InitialCover);
        }
    }
}

/// Parks every grunt, puts the player back at spawn whole, and starts a new run
/// with the next seed.
fn reset_characters(
    mut commands: Commands,
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    layout: Res<ArenaLayout>,
    mut run: ResMut<Run>,
    mut stats: ResMut<CombatStats>,
    mut grunt_rng: ResMut<GruntRng>,
    mut tokens: ResMut<AttackTokens>,
    mut grunts: Query<PoolParts, (With<PoolGrunt>, Without<Player>)>,
    mut players: Query<
        (
            Entity,
            &mut Transform,
            &mut PreviousFeet,
            &mut LookAngles,
            &mut Health,
            &mut PlayerIntent,
            &mut Motor,
            &mut EyeHeight,
            &mut Loadout,
            &mut Ads,
            Option<&mut building::EditMode>,
        ),
        With<Player>,
    >,
) {
    let now = tick.0;
    for mut grunt in &mut grunts {
        grunt.park(&mut commands, now);
    }
    for (
        entity,
        mut tf,
        mut prev,
        mut look,
        mut health,
        mut intent,
        mut motor,
        mut eye,
        mut lo,
        mut ads,
        edit,
    ) in &mut players
    {
        tf.translation = layout.player_spawn;
        prev.0 = layout.player_spawn;
        *look = layout.player_look;
        health.reset();
        *intent = PlayerIntent::default();
        *motor = Motor::default();
        eye.0 = tuning.movement.eye_height;
        lo.rifle = GunState::new(&tuning.combat.rifle);
        lo.pump = GunState::new(&tuning.combat.pump);
        lo.switch_remaining = 0.0;
        lo.pump_buffer = None;
        ads.0 = false;
        if let Some(mut edit) = edit {
            *edit = building::EditMode::default();
        }
        commands.entity(entity).remove::<(Downed, Knockback)>();
    }
    stats.reset();
    *grunt_rng = GruntRng::default();
    tokens.clear();
    let seed = run.rng.next_u64();
    *run = Run::new(seed, now, &tuning.waves);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_sizes_follow_d68() {
        let t = WavesTuning::default();
        assert_eq!(t.wave_size(1), 3);
        assert_eq!(t.wave_size(2), 5);
        assert_eq!(t.wave_size(20), 41);
        let through_20: u32 = (1..=20).map(|w| t.wave_size(w)).sum();
        assert_eq!(through_20, 440);
    }
}
