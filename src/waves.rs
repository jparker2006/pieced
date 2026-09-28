//! The Waves mode: runs, waves, the break, potions, score and results
//! (docs/M3-SPEC.md → Waves, D79–D85). Only active in
//! [`GameMode::Waves`](crate::shared::GameMode).
//!
//! **The wave director** (chunk 2, slice A): wave n has
//! [`WavesTuning::wave_size`] knights, at most `max_alive` in play or on their
//! way down; the rest come on drop ships ([`ships`], chunk 3) as pool slots
//! free up. A knight knocked off the island falls into the void ([`void`]) and
//! scores the void bonus. Clearing a wave scores
//! it and starts the break ([`RunPhase::Break`]: +50 shield, 10 s, skippable
//! with [`SkipBreak`]), then the next wave comes, a little tougher
//! ([`GruntStats::for_wave`]). Downed knights may drop a shield potion
//! ([`potion`]). The player's elimination ends the run: the death beat
//! ([`RunPhase::Dying`], slow motion in the client, the survivors hop), then
//! [`RunPhase::Over`] and the results. Each run's end updates the personal best
//! and appends a line to the run log ([`record`]). [`RunSummary`] is the
//! read-only view the HUD and results screen draw from.
//!
//! Grunt characters come from a fixed pool of `max_alive` characters created at
//! run start (parked and inactive), reused for every spawn, so a wave spawn
//! never instances a model mid-fight (the knight figure is rigged once).

pub mod modes;
pub mod potion;
pub mod record;
pub mod ships;
pub mod ships_visuals;
pub mod ui;
pub mod void;

pub use potion::{LivePotion, POTION_POOL, PotionSlot};
pub use record::{
    BestRun, DeathCause, PersonalBest, RunEnd, RunLogLine, RunResults, RunStore, build_commit,
};

use crate::{
    arena::ArenaLayout,
    building::{self, InitialCover},
    combat::{CombatStats, Downed, GunState, Loadout},
    grunt::{self, AttackTokens, GruntBrain, GruntRng, GruntStats, Parked},
    movement::{Knockback, Motor, VoidFall},
    orb::Wand,
    rng::{Rng, SimRng},
    shared::{
        Ads, AppState, Character, DamageDealt, DamageTarget, EyeHeight, GameMode, Health, Layer,
        LookAngles, Player, PlayerIntent, PreviousFeet, SimSet, SimTick, TICK_SECONDS,
    },
    tuning::Tuning,
};
use avian3d::prelude::*;
use bevy::{
    ecs::{message::Messages, query::QueryData, system::SystemParam},
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
// The run
// ---------------------------------------------------------------------------

/// Seconds from a run's (or wave's) start to its first ship's launch.
pub const FIRST_SPAWN_DELAY: f32 = 1.0;
/// Seconds a downed grunt lies (the KO take, the hat drop) before it goes back
/// to the pool.
pub const RETURN_DELAY: f32 = 1.0;
/// Simulated seconds after the player's elimination during which the surviving
/// knights hop (D84's goofy victory hop); then they stand still.
pub const VICTORY_HOP_SECONDS: f32 = 1.2;
/// Grunts land on a ring this far inside the playable bounds (m): just inside
/// the island edge (the drop points, [`ships::DROP_POINTS`]).
pub const EDGE_INSET: f32 = 2.0;
/// ... at least this far (horizontally) from the player (m).
pub const SPAWN_MIN_PLAYER_DISTANCE: f32 = 12.0;
/// Where parked grunts wait: under the island, out of sight and out of play.
pub const PARK_SPOT: Vec3 = Vec3::new(0.0, -60.0, 0.0);
/// Salt for the run-seed stream.
const WAVES_SALT: u64 = 0x3A7E_5EED;
/// Salt for a run's potion-roll stream (separate from spawn points, so the
/// kill count never shifts where knights land).
const POTION_SALT: u64 = 0x9071_0115;

/// Whole ticks covering `seconds`.
pub(crate) fn ticks(seconds: f32) -> u64 {
    (seconds / TICK_SECONDS).round().max(0.0) as u64
}

/// Where the run stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RunPhase {
    /// The wave's grunts are arriving or fighting.
    #[default]
    Fighting,
    /// Every grunt of wave `Run::wave` is down: the break (D79). The next wave
    /// starts at `ends_tick`, or on a [`SkipBreak`].
    Break { ends_tick: u64 },
    /// The player was eliminated: the death beat (D84). The client runs time at
    /// `death_time_scale` until `until` (`death_seconds` of real time), then
    /// the run is [`RunPhase::Over`].
    Dying { until: u64 },
    /// The run has ended (since `tick`): the results. It waits for a
    /// [`RestartRun`] ("Go again").
    Over { tick: u64 },
}

/// The current run (Waves mode only; absent in Practice).
#[derive(Resource, Debug, Clone)]
pub struct Run {
    /// Drives spawn points, speed jitter and potion rolls (D83). A restart
    /// draws the next run's seed from this run's stream.
    pub seed: u64,
    pub wave: u32,
    pub phase: RunPhase,
    /// Grunts in play: landed and not downed.
    pub alive: u32,
    /// Grunts of this wave still to land (including those aboard a ship or in
    /// its beam).
    pub remaining: u32,
    /// Of `remaining`, those aboard a ship or in its beam: `alive + aboard`
    /// never exceeds `max_alive`.
    pub aboard: u32,
    /// Knights downed this run.
    pub eliminations: u32,
    /// Knights downed by a headshot (the +50s in the score).
    pub headshot_kills: u32,
    /// Knights knocked into the void (the +150s).
    pub void_kills: u32,
    /// D81: 100 per knight, +50 headshot kill, +150 void, +250 × n per wave.
    pub score: u32,
    pub waves_cleared: u32,
    pub started_tick: u64,
    /// Tick the run ended (the player's elimination, or a quit).
    pub ended_tick: Option<u64>,
    pub ended: Option<RunEnd>,
    /// This run beat the personal best (set when the run ends).
    pub new_best: bool,
    next_spawn_tick: u64,
    /// Survivors stop moving from this tick (after the victory hop).
    freeze_tick: Option<u64>,
    /// The run just ended and its record isn't written yet.
    record_pending: bool,
    rng: Rng,
    potion_rng: Rng,
}

impl Run {
    pub fn new(seed: u64, tick: u64, tuning: &WavesTuning) -> Self {
        let rng = Rng::new(seed);
        let potion_rng = rng.clone().fork(POTION_SALT);
        let mut run = Self {
            seed,
            wave: 1,
            phase: RunPhase::Fighting,
            alive: 0,
            remaining: 0,
            aboard: 0,
            eliminations: 0,
            headshot_kills: 0,
            void_kills: 0,
            score: 0,
            waves_cleared: 0,
            started_tick: tick,
            ended_tick: None,
            ended: None,
            new_best: false,
            next_spawn_tick: tick,
            freeze_tick: None,
            record_pending: false,
            rng,
            potion_rng,
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

    /// The results screen is up: the run has ended and the death beat is over.
    pub fn is_over(&self) -> bool {
        matches!(self.phase, RunPhase::Over { .. })
    }

    /// The run has ended: the death beat or the results.
    pub fn is_ended(&self) -> bool {
        matches!(self.phase, RunPhase::Dying { .. } | RunPhase::Over { .. })
    }

    /// Knights left in the wave: in play plus still to come.
    pub fn left(&self) -> u32 {
        self.alive + self.remaining
    }

    /// Seconds of the break left (`None` outside a break).
    pub fn break_seconds_left(&self, now: u64) -> Option<f32> {
        match self.phase {
            RunPhase::Break { ends_tick } => {
                Some(ends_tick.saturating_sub(now) as f32 * TICK_SECONDS)
            }
            _ => None,
        }
    }

    /// Simulated seconds from the start to the end (or `now`).
    pub fn run_seconds(&self, now: u64) -> f32 {
        self.ended_tick
            .unwrap_or(now)
            .saturating_sub(self.started_tick) as f32
            * TICK_SECONDS
    }

    /// The results-screen numbers, with the player's shooting from `stats`.
    pub fn results(&self, now: u64, stats: &CombatStats) -> RunResults {
        RunResults {
            seed: self.seed,
            wave: self.wave,
            score: self.score,
            eliminations: self.eliminations,
            accuracy: stats.accuracy(),
            headshots: stats.headshots,
            run_seconds: self.run_seconds(now),
        }
    }

    /// Ends the run at `now`: an elimination starts the death beat; a quit goes
    /// straight to the results.
    fn end(&mut self, now: u64, how: RunEnd, tuning: &WavesTuning) {
        self.ended_tick = Some(now);
        self.ended = Some(how);
        self.record_pending = true;
        match how {
            RunEnd::Eliminated => {
                let beat = tuning.death_seconds * tuning.death_time_scale;
                self.phase = RunPhase::Dying {
                    until: now + ticks(beat).max(1),
                };
                self.freeze_tick = Some(now + ticks(VICTORY_HOP_SECONDS));
            }
            RunEnd::Quit => {
                self.phase = RunPhase::Over { tick: now };
                self.freeze_tick = Some(now);
            }
        }
    }

    /// Whether the survivors are doing the victory hop at `now`.
    pub fn hopping(&self, now: u64) -> bool {
        self.ended == Some(RunEnd::Eliminated) && self.freeze_tick.is_some_and(|f| now < f)
    }
}

/// Starts a new run in place ("Go again"): pieces back to the initial cover,
/// orbs and potions gone, grunts parked, the player at spawn at full health, a
/// score of 0, a new seed. Written by the input adapter (Enter on the results)
/// or by tests. Works in any phase.
#[derive(Message, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RestartRun;

/// Ends the break now (D79: Enter, rebindable). Ignored outside a break.
#[derive(Message, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SkipBreak;

/// Ends the run now and goes to the results (quitting from the pause menu,
/// D84). Recorded like a death, with [`RunEnd::Quit`]. Ignored once ended.
#[derive(Message, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EndRun;

/// A knight was knocked off the island (D78, chunk 3): worth the void bonus
/// (`score_void`) on top of the kill. The sender also marks the knight
/// [`Downed`] (which counts the elimination and the 100); the message may come
/// on the same tick or any tick before the knight returns to the pool. A
/// void knight drops no potion.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoidKill {
    pub knight: Entity,
}

/// The first run's seed, fixed from the command line (`--seed <n>`, D83).
/// Later runs ("Go again") draw new seeds from it.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunSeed(pub u64);

/// A member of the grunt pool (`max_alive` characters created at startup and
/// reused for every spawn, so a spawn never instances a knight mid-fight).
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PoolGrunt {
    /// Tick it went down, while it lies before returning to the pool.
    pub downed_at: Option<u64>,
    /// The wave it came for.
    pub wave: u32,
    /// Knocked into the void ([`VoidKill`]) before its down was counted.
    pub void_kill: bool,
    /// Taken for a ship: aboard or in its beam, not yet landed.
    pub aboard: bool,
}

/// Run condition: a Waves run exists and it has ended (the death beat or the
/// results): the player's controls do nothing, and Enter goes again.
pub fn run_over(run: Option<Res<Run>>) -> bool {
    run.is_some_and(|run| run.is_ended())
}

/// Run condition: a Waves run is in its break (Enter skips it).
pub fn in_break(run: Option<Res<Run>>) -> bool {
    run.is_some_and(|run| matches!(run.phase, RunPhase::Break { .. }))
}

/// Test-only switch: Waves mode without the wave director (no run, no pool),
/// for tests that place their own knights (`Sim::grunt_lab`).
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct NoWaveDirector;

/// Everything the HUD and the results screen show (slice B reads this; it's
/// rewritten after the director every fixed tick).
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct RunSummary {
    pub seed: u64,
    /// The wave being fought (during a break: the wave just cleared).
    pub wave: u32,
    /// Knights left in the wave: in play plus still to come.
    pub knights_left: u32,
    pub score: u32,
    pub phase: RunPhase,
    /// The break's countdown (s), while in a break.
    pub break_seconds_left: Option<f32>,
    /// The saved personal best (after this run's end: including this run).
    pub best: Option<BestRun>,
    /// This run is a new personal best (known once the run ends).
    pub new_best: bool,
    /// The results-screen numbers (live during the run, final once ended).
    pub results: RunResults,
}

/// The run: grunt pool, waves, break, potions, score, the death beat, records
/// and restart.
pub struct WavesPlugin;

impl Plugin for WavesPlugin {
    fn build(&self, app: &mut App) {
        let waves =
            resource_equals(GameMode::Waves).and_then(not(resource_exists::<NoWaveDirector>));
        app.add_message::<RestartRun>()
            .add_message::<SkipBreak>()
            .add_message::<EndRun>()
            .add_message::<VoidKill>()
            .init_resource::<RunStore>()
            .init_resource::<PersonalBest>()
            .init_resource::<RunInbox>()
            .init_resource::<ships::Ships>()
            .init_resource::<potion::PendingPotions>()
            .add_systems(
                Startup,
                (
                    load_best,
                    (start_run, register_restart, potion::spawn_potion_pool).run_if(waves),
                )
                    .chain(),
            )
            .add_systems(
                FixedUpdate,
                (
                    restart_run
                        .before(SimSet::Control)
                        .run_if(in_state(AppState::Playing)),
                    silence_dead_player.in_set(SimSet::Control),
                    victory_hop.in_set(SimSet::Tool),
                    (
                        read_run_messages,
                        run_director,
                        ships::step_ships,
                        record_run_end,
                        potion::step_potions,
                        update_summary,
                    )
                        .chain()
                        .in_set(SimSet::Resolve),
                )
                    .run_if(resource_exists::<Run>),
            );
        void::build(app);
        // The main menu's mode switch (chunk 5).
        modes::build(app);
    }
}

/// Presentation-side run effects (client only): the death beat (slow motion,
/// the view dropping to the grass, the vignette; [`ui::death`]).
pub struct WavesClientPlugin;

impl Plugin for WavesClientPlugin {
    fn build(&self, app: &mut App) {
        ui::death::build(app);
    }
}

fn load_best(store: Res<RunStore>, mut best: ResMut<PersonalBest>) {
    best.0 = store.load_best();
}

fn start_run(
    mut commands: Commands,
    tuning: Res<Tuning>,
    sim_rng: Res<SimRng>,
    fixed: Option<Res<RunSeed>>,
    best: Res<PersonalBest>,
    tick: Res<SimTick>,
) {
    let seed = fixed.map_or_else(|| sim_rng.0.clone().fork(WAVES_SALT).next_u64(), |s| s.0);
    let run = Run::new(seed, tick.0, &tuning.waves);
    commands.insert_resource(summarize(&run, tick.0, &CombatStats::default(), &best));
    commands.insert_resource(run);
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
    /// In play: landed, not downed, not frozen.
    fn in_play(&self) -> bool {
        !self.parked && !self.downed
    }

    /// Waiting in the pool, free for the next ship.
    fn free(&self) -> bool {
        self.parked && self.downed && self.slot.downed_at.is_none() && !self.slot.aboard
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
        *self.slot = PoolGrunt::default();
        commands
            .entity(self.entity)
            .insert((Parked, Downed { tick }))
            .remove::<(Knockback, VoidFall, ships::Beaming)>();
    }
}

/// What the director needs from this tick's messages.
#[derive(Resource, Debug)]
struct RunInbox {
    /// Knights the player downed with a headshot this tick.
    headshot_kills: Vec<Entity>,
    /// Knights knocked into the void this tick.
    void_kills: Vec<Entity>,
    skip_break: bool,
    end_requested: bool,
    /// The player's latest hit (tick), for the death cause.
    player_hit_tick: Option<u64>,
    /// What killed the player.
    cause: Option<DeathCause>,
}

impl Default for RunInbox {
    fn default() -> Self {
        Self {
            headshot_kills: Vec::with_capacity(16),
            void_kills: Vec::with_capacity(16),
            skip_break: false,
            end_requested: false,
            player_hit_tick: None,
            cause: None,
        }
    }
}

impl RunInbox {
    fn reset(&mut self) {
        self.headshot_kills.clear();
        self.void_kills.clear();
        self.skip_break = false;
        self.end_requested = false;
        self.player_hit_tick = None;
        self.cause = None;
    }
}

/// Once the run has ended, the player's controls do nothing (the results'
/// Enter is read by the input adapter, not through `PlayerIntent`).
fn silence_dead_player(run: Res<Run>, mut players: Query<&mut PlayerIntent, With<Player>>) {
    if !run.is_ended() {
        return;
    }
    for mut intent in &mut players {
        if *intent != PlayerIntent::default() {
            *intent = PlayerIntent::default();
        }
    }
}

/// D84's goofy victory hop: after the player's elimination the surviving
/// knights jump whenever they land, for [`VICTORY_HOP_SECONDS`]. Runs after
/// the brains (which idle once the player is down), before movement.
fn victory_hop(
    run: Res<Run>,
    tick: Res<SimTick>,
    mut grunts: Query<
        (&mut PlayerIntent, &Motor),
        (With<PoolGrunt>, Without<Parked>, Without<Downed>),
    >,
) {
    if !run.hopping(tick.0) {
        return;
    }
    for (mut intent, motor) in &mut grunts {
        intent.jump = true;
        if motor.grounded {
            intent.jump_pressed = true;
        }
    }
}

/// Reads this tick's damage, void, skip and quit messages into the inbox.
fn read_run_messages(
    mut inbox: ResMut<RunInbox>,
    mut damage: MessageReader<DamageDealt>,
    mut void: MessageReader<VoidKill>,
    mut skip: MessageReader<SkipBreak>,
    mut end: MessageReader<EndRun>,
    players: Query<Entity, With<Player>>,
    sources: Query<(&Transform, Option<&PoolGrunt>)>,
) {
    let player = players.iter().next();
    for hit in damage.read() {
        if hit.target_kind != DamageTarget::Character || hit.amount <= 0.0 {
            continue;
        }
        if Some(hit.target) == player {
            if hit.killed && inbox.cause.is_none() {
                let source = hit.source.and_then(|s| sources.get(s).ok());
                inbox.cause = Some(DeathCause {
                    source_wave: source.and_then(|(_, slot)| slot.map(|s| s.wave)),
                    source_position: source.map(|(t, _)| t.translation.to_array()),
                    seconds_since_previous_hit: inbox
                        .player_hit_tick
                        .map(|t| hit.tick.saturating_sub(t) as f32 * TICK_SECONDS),
                });
            }
            inbox.player_hit_tick = Some(hit.tick);
        } else if hit.killed && hit.headshot && player.is_some() && hit.source == player {
            inbox.headshot_kills.push(hit.target);
        }
    }
    inbox.void_kills.extend(void.read().map(|v| v.knight));
    inbox.skip_break |= skip.read().count() > 0;
    inbox.end_requested |= end.read().count() > 0;
}

/// The spawn-point checks: a knight's capsule must touch no world geometry or
/// piece.
#[derive(SystemParam)]
struct SpawnCheck<'w, 's> {
    spatial: SpatialQuery<'w, 's>,
    collider_of: Query<'w, 's, &'static ColliderOf>,
    characters: Query<'w, 's, (), With<Character>>,
}

impl SpawnCheck<'_, '_> {
    fn is_clear(&self, spot: Vec3) -> bool {
        // A capsule from 0.05 m to 1.95 m above the feet must touch no world
        // geometry or piece (characters' own colliders don't count).
        let blockers = SpatialQueryFilter::from_mask([Layer::World, Layer::Piece]);
        let body = Collider::capsule(0.4, 1.1);
        let mut clear = true;
        self.spatial.shape_intersections_callback(
            &body,
            spot + Vec3::Y * 1.0,
            Quat::IDENTITY,
            &blockers,
            |e| {
                clear &= self
                    .collider_of
                    .get(e)
                    .is_ok_and(|c| self.characters.contains(c.body));
                clear
            },
        );
        clear
    }
}

/// The wave loop, after combat each tick: the player's death (or a quit) ends
/// the run; downed grunts score, may drop a potion, and return to the pool;
/// drop ships launch with the wave's knights as slots free up ([`ships`] then
/// flies them in and lands them); a cleared wave starts the break; the break
/// ends into the next wave.
#[allow(clippy::too_many_arguments)]
fn run_director(
    mut commands: Commands,
    mut run: ResMut<Run>,
    mut inbox: ResMut<RunInbox>,
    mut drops: ResMut<potion::PendingPotions>,
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    layout: Res<ArenaLayout>,
    spawn_check: SpawnCheck,
    mut players: Query<(Entity, &Transform, &mut Health, Has<Downed>), With<Player>>,
    mut grunts: Query<PoolParts, (With<PoolGrunt>, Without<Player>)>,
    mut ships: ResMut<ships::Ships>,
) {
    let now = tick.0;
    let t = &tuning.waves;
    let run = &mut *run;
    let inbox = &mut *inbox;

    // The player's elimination ends the run (one life, D84); so does a quit.
    if !run.is_ended() {
        if let Some((entity, _, health, downed)) = players.iter().next()
            && (downed || health.is_dead())
        {
            if !downed {
                commands.entity(entity).insert(Downed { tick: now });
            }
            run.end(now, RunEnd::Eliminated, t);
        } else if inbox.end_requested {
            run.end(now, RunEnd::Quit, t);
        }
    }
    inbox.end_requested = false;
    let live = !run.is_ended();

    // Void knock-offs (chunk 3): the bonus now if the down already counted,
    // else when it does.
    for knight in inbox.void_kills.drain(..) {
        let Ok(mut grunt) = grunts.get_mut(knight) else {
            continue;
        };
        if grunt.parked {
            continue;
        }
        if grunt.slot.downed_at.is_some() {
            if live {
                run.score += t.score_void;
                run.void_kills += 1;
            }
        } else {
            grunt.slot.void_kill = true;
        }
    }

    // Downed grunts count once, lie for a beat, then go back to the pool.
    for mut grunt in &mut grunts {
        if grunt.parked || !grunt.downed {
            continue;
        }
        match grunt.slot.downed_at {
            None => {
                grunt.slot.downed_at = Some(now);
                if !live {
                    continue;
                }
                run.eliminations += 1;
                run.score += t.score_kill;
                if inbox.headshot_kills.contains(&grunt.entity) {
                    run.score += t.score_headshot;
                    run.headshot_kills += 1;
                }
                if grunt.slot.void_kill {
                    run.score += t.score_void;
                    run.void_kills += 1;
                } else if run.potion_rng.chance(t.potion_chance) {
                    drops.0.push(grunt.transform.translation);
                }
            }
            Some(at) if now.saturating_sub(at) >= ticks(RETURN_DELAY) => {
                grunt.park(&mut commands, now);
            }
            Some(_) => {}
        }
    }
    inbox.headshot_kills.clear();
    run.alive = grunts.iter().filter(|g| !g.parked && !g.downed).count() as u32;

    // After the victory hop the survivors stop where they stand (frozen by
    // `Parked` once they're on the ground, so none hangs mid-jump).
    if run.freeze_tick.is_some_and(|f| now >= f) {
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

    let skip = std::mem::take(&mut inbox.skip_break);
    match run.phase {
        RunPhase::Dying { until } => {
            if now >= until {
                run.phase = RunPhase::Over { tick: now };
            }
        }
        RunPhase::Over { .. } => {}
        RunPhase::Break { ends_tick } => {
            if skip || now >= ends_tick {
                let next = run.wave + 1;
                run.start_wave(next, now, t);
            }
        }
        RunPhase::Fighting => {
            if run.remaining == 0 && run.alive == 0 {
                // Wave cleared: score it, refill the shield, take a breath (D79).
                run.waves_cleared += 1;
                run.score += t.score_wave * run.wave;
                run.phase = RunPhase::Break {
                    ends_tick: now + ticks(t.break_seconds),
                };
                for (_, _, mut health, downed) in &mut players {
                    if !downed && !health.is_dead() {
                        health.shield = (health.shield + t.break_shield).min(health.max_shield);
                    }
                }
                return;
            }
            // A ship launches whenever a slot is free: knights alive plus
            // those aboard or in a beam stay within `max_alive` (D77, D82).
            let unassigned = run.remaining.saturating_sub(run.aboard);
            let room = t.max_alive.saturating_sub(run.alive + run.aboard);
            if unassigned == 0 || room == 0 || now < run.next_spawn_tick {
                return;
            }
            let Some(slot) = ships.free_slot() else {
                return;
            };
            let player_feet = players
                .iter()
                .next()
                .map_or(layout.player_spawn, |p| p.1.translation);
            let max_count = ships::max_aboard(t.wave_size(run.wave), unassigned, room);
            let Some(plan) =
                ships::plan_sortie(&mut run.rng, &ships, player_feet, max_count, |p| {
                    spawn_check.is_clear(p)
                })
            else {
                run.next_spawn_tick = now + ticks(ships::RETRY_SECONDS);
                return;
            };
            let mut sortie =
                ships::Sortie::new(now, plan.point, plan.hover_height, plan.peel, plan.count);
            // Take its knights from the pool: readied for this wave, still
            // parked under the island until each starts down the beam.
            let mut taken = 0;
            for mut grunt in &mut grunts {
                if taken == sortie.count {
                    break;
                }
                if !grunt.free() {
                    continue;
                }
                let mut stats = GruntStats::for_wave(run.wave, &tuning.grunt);
                let jitter = t.speed_jitter.abs();
                stats.speed = (stats.speed * (1.0 + run.rng.range(-jitter, jitter)))
                    .min(tuning.grunt.speed_cap);
                *grunt.health = Health::full(stats.hp, 0.0);
                *grunt.stats = stats;
                *grunt.wand = Wand::new(stats.fire_interval);
                *grunt.intent = PlayerIntent::default();
                *grunt.motor = Motor::default();
                *grunt.slot = PoolGrunt {
                    downed_at: None,
                    wave: run.wave,
                    void_kill: false,
                    aboard: true,
                };
                // A fresh brain, seeded from the run so the same seed replays the run.
                grunt.brain.reset();
                grunt
                    .brain
                    .reseed(Rng::new(run.seed).fork(now ^ grunt.entity.to_bits().rotate_left(32)));
                commands.entity(grunt.entity).remove::<Knockback>();
                sortie.seats[taken].knight = Some(grunt.entity);
                taken += 1;
            }
            if taken == 0 {
                return;
            }
            sortie.count = taken;
            ships.slots[slot] = Some(sortie);
            run.aboard += taken as u32;
            run.next_spawn_tick = now + ticks(plan.gap);
        }
    }
}

/// At a run's end: compare with the personal best (saving it when beaten) and
/// append the run's line to the run log (D81, D89).
fn record_run_end(
    mut run: ResMut<Run>,
    tick: Res<SimTick>,
    inbox: Res<RunInbox>,
    stats: Res<CombatStats>,
    store: Res<RunStore>,
    mut best: ResMut<PersonalBest>,
) {
    if !run.record_pending {
        return;
    }
    run.record_pending = false;
    let results = run.results(tick.0, &stats);
    let this = BestRun::from(&results);
    run.new_best = best.0.as_ref().is_none_or(|b| this.beats(b));
    if run.new_best {
        if let Err(e) = store.save_best(&this) {
            warn!("waves: couldn't save the best run: {e}");
        }
        best.0 = Some(this);
    }
    let how = run.ended.unwrap_or_default();
    let cause = (how == RunEnd::Eliminated)
        .then(|| inbox.cause.clone())
        .flatten();
    if let Err(e) = store.append_run(&RunLogLine::new(&results, how, cause)) {
        warn!("waves: couldn't append to the run log: {e}");
    }
}

fn summarize(run: &Run, now: u64, stats: &CombatStats, best: &PersonalBest) -> RunSummary {
    RunSummary {
        seed: run.seed,
        wave: run.wave,
        knights_left: run.left(),
        score: run.score,
        phase: run.phase,
        break_seconds_left: run.break_seconds_left(now),
        best: best.0.clone(),
        new_best: run.new_best,
        results: run.results(now, stats),
    }
}

/// Rewrites [`RunSummary`] after the director.
fn update_summary(
    run: Res<Run>,
    tick: Res<SimTick>,
    stats: Res<CombatStats>,
    best: Res<PersonalBest>,
    mut summary: ResMut<RunSummary>,
) {
    let now = summarize(&run, tick.0, &stats, &best);
    if *summary != now {
        *summary = now;
    }
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
    recycle_pools(world);
    if let Err(e) = world.run_system_cached(reset_characters) {
        error!("waves: restart failed: {e}");
        return;
    }
    // After the player is back at spawn, which is off the cover.
    reset_cover(world);
}

/// Orbs in flight and potions on the ground vanish back into their pools.
fn recycle_pools(world: &mut World) {
    crate::orb::recycle_all_orbs(world);
    potion::recycle_all_potions(world);
}

/// The grid back to the arena's initial cover.
fn reset_cover(world: &mut World) {
    building::clear_pieces(world);
    for slot in building::initial_cover() {
        if let Ok(piece) = building::place_piece(world, slot) {
            world.entity_mut(piece).insert(InitialCover);
        }
    }
}

/// The player's parts a restart (or a mode switch) puts back.
#[derive(QueryData)]
#[query_data(mutable)]
struct PlayerParts {
    entity: Entity,
    transform: &'static mut Transform,
    previous: &'static mut PreviousFeet,
    look: &'static mut LookAngles,
    health: &'static mut Health,
    intent: &'static mut PlayerIntent,
    motor: &'static mut Motor,
    eye: &'static mut EyeHeight,
    loadout: &'static mut Loadout,
    ads: &'static mut Ads,
    edit: Option<&'static mut building::EditMode>,
}

impl PlayerPartsItem<'_, '_> {
    /// Back at spawn, whole, guns full, not editing.
    fn respawn(&mut self, commands: &mut Commands, layout: &ArenaLayout, tuning: &Tuning) {
        self.transform.translation = layout.player_spawn;
        self.previous.0 = layout.player_spawn;
        *self.look = layout.player_look;
        self.health.reset();
        *self.intent = PlayerIntent::default();
        *self.motor = Motor::default();
        self.eye.0 = tuning.movement.eye_height;
        self.loadout.rifle = GunState::new(&tuning.combat.rifle);
        self.loadout.pump = GunState::new(&tuning.combat.pump);
        self.loadout.switch_remaining = 0.0;
        self.loadout.pump_buffer = None;
        self.ads.0 = false;
        if let Some(edit) = self.edit.as_mut() {
            **edit = building::EditMode::default();
        }
        commands.entity(self.entity).remove::<(Downed, Knockback)>();
    }
}

/// Parks every grunt, puts the player back at spawn whole, and starts a new run
/// with the next seed.
#[allow(clippy::too_many_arguments)]
fn reset_characters(
    mut commands: Commands,
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    layout: Res<ArenaLayout>,
    mut run: ResMut<Run>,
    mut inbox: ResMut<RunInbox>,
    mut stats: ResMut<CombatStats>,
    mut grunt_rng: ResMut<GruntRng>,
    mut tokens: ResMut<AttackTokens>,
    mut ships: ResMut<ships::Ships>,
    mut grunts: Query<PoolParts, (With<PoolGrunt>, Without<Player>)>,
    mut players: Query<PlayerParts, With<Player>>,
) {
    let now = tick.0;
    for mut grunt in &mut grunts {
        grunt.park(&mut commands, now);
    }
    for mut player in &mut players {
        player.respawn(&mut commands, &layout, &tuning);
    }
    stats.reset();
    inbox.reset();
    *grunt_rng = GruntRng::default();
    tokens.clear();
    ships.reset();
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

    #[test]
    fn the_death_beat_lasts_a_real_second_of_slow_motion() {
        let t = WavesTuning::default();
        let mut run = Run::new(1, 100, &t);
        run.end(100, RunEnd::Eliminated, &t);
        // 1 s of real time at 0.3× is 0.3 s of game time: 18 ticks.
        assert_eq!(run.phase, RunPhase::Dying { until: 118 });
        assert!(run.is_ended() && !run.is_over());
        assert!(run.hopping(110));
        let mut quit = Run::new(1, 100, &t);
        quit.end(160, RunEnd::Quit, &t);
        assert_eq!(quit.phase, RunPhase::Over { tick: 160 });
        assert!(!quit.hopping(160));
        assert_eq!(quit.run_seconds(1000), 1.0);
    }
}
