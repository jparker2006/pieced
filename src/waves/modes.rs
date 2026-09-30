//! The main menu's mode switch (D92, M3 chunk 5): Waves and Practice start
//! **in place**, without reloading anything.
//!
//! - Entering [`AppState::Menu`] lays the island out for the menu ([`go_idle`]):
//!   orbs and potions back in their pools, every grunt parked, the dummy
//!   gone, the player at spawn and whole, no run, and the arena's initial
//!   cover (rebuilt only if play could have changed it).
//! - [`StartMode`] starts a mode from there: Practice spawns the dummy;
//!   Waves makes sure the grunt and potion pools exist (created once, reused
//!   for every run) and starts a fresh run. Then play resumes (`Playing`).
//!
//! A mode can also be started from outside the menu (tests): the arena is
//! reset first.

use super::{
    PersonalBest, PlayerParts, PoolGrunt, PoolParts, Run, RunInbox, RunSeed, RunSummary,
    WAVES_SALT, potion, recycle_pools, reset_cover, ships, spawn_pool_member, summarize,
};
use crate::{
    arena::ArenaLayout,
    combat::CombatStats,
    dummy::Dummy,
    grunt::{AttackTokens, GruntRng, GruntStats},
    movement::VoidFall,
    rng::{Rng, SimRng},
    shared::{AppState, GameMode, Player, SimTick},
    tuning::Tuning,
};
use bevy::{ecs::message::Messages, prelude::*};

/// Starts a mode in place (the main menu's Waves and Practice buttons).
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartMode(pub GameMode);

/// The wave a Waves run starts at (D115): the main menu's 1 / 6 / 10 choice.
/// A run started above wave 1 plays that wave's size, stats and scaling
/// exactly as if it had got there, and never counts toward the personal best.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartWave(u32);

/// The starting waves the menu offers.
pub const START_WAVES: [u32; 3] = [1, 6, 10];

impl Default for StartWave {
    fn default() -> Self {
        Self(1)
    }
}

impl StartWave {
    /// A starting wave (at least 1).
    pub fn new(wave: u32) -> Self {
        Self(wave.max(1))
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

/// Salt for the seeds of runs started from the menu.
const MENU_SALT: u64 = 0x3E_7015;

/// Where the island stands between modes.
#[derive(Resource, Debug, Default)]
pub struct ModeSwitch {
    /// Laid out for the menu (see [`go_idle`]).
    idle: bool,
    /// Play may have changed the pieces since the last cover reset.
    dirty: bool,
    /// Seeds for Waves runs started from the menu (after the first).
    seeds: Option<Rng>,
    /// Modes started so far.
    pub starts: u32,
}

impl ModeSwitch {
    /// The island is laid out for the menu: no run, no dummy.
    pub fn is_idle(&self) -> bool {
        self.idle
    }
}

pub(super) fn build(app: &mut App) {
    app.add_message::<StartMode>()
        .init_resource::<ModeSwitch>()
        .init_resource::<StartWave>()
        .add_systems(Startup, register_mode_systems)
        .add_systems(OnEnter(AppState::Menu), go_idle)
        .add_systems(OnEnter(AppState::Playing), mark_dirty)
        // Before the state transition, so play starts on the same frame.
        .add_systems(PreUpdate, start_requested_mode);
}

/// Registers the switch's systems up front, so the first switch allocates
/// no system state.
fn register_mode_systems(world: &mut World) {
    world.register_system_cached(idle_characters);
    world.register_system_cached(crate::dummy::spawn_dummy);
    world.register_system_cached(spawn_pool);
    world.register_system_cached(potion::spawn_potion_pool);
    world.register_system_cached(super::reset_characters);
}

fn mark_dirty(mut switch: ResMut<ModeSwitch>) {
    switch.dirty = true;
    switch.idle = false;
}

/// Lays the island out for the menu: pools recycled, grunts parked, the
/// dummy gone, the player at spawn, no run, the initial cover.
pub fn go_idle(world: &mut World) {
    recycle_pools(world);
    if let Err(e) = world.run_system_cached(idle_characters) {
        error!("modes: couldn't reset the characters: {e}");
    }
    if world.resource::<ModeSwitch>().dirty {
        reset_cover(world);
    }
    world.remove_resource::<Run>();
    world.remove_resource::<RunSummary>();
    let mut switch = world.resource_mut::<ModeSwitch>();
    switch.idle = true;
    switch.dirty = false;
}

#[allow(clippy::too_many_arguments)]
fn idle_characters(
    mut commands: Commands,
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    layout: Res<ArenaLayout>,
    mut inbox: ResMut<RunInbox>,
    mut stats: ResMut<CombatStats>,
    mut grunt_rng: ResMut<GruntRng>,
    mut tokens: ResMut<AttackTokens>,
    mut ships: ResMut<ships::Ships>,
    mut grunts: Query<PoolParts, (With<PoolGrunt>, Without<Player>)>,
    mut players: Query<PlayerParts, With<Player>>,
    dummies: Query<Entity, With<Dummy>>,
) {
    for mut grunt in &mut grunts {
        grunt.park(&mut commands, tick.0);
    }
    for mut player in &mut players {
        player.respawn(&mut commands, &layout, &tuning);
        commands.entity(player.entity).remove::<VoidFall>();
    }
    for dummy in &dummies {
        commands.entity(dummy).despawn();
    }
    stats.reset();
    inbox.reset();
    *grunt_rng = GruntRng::default();
    tokens.clear();
    ships.reset();
}

fn start_requested_mode(world: &mut World) {
    let requested = world
        .get_resource_mut::<Messages<StartMode>>()
        .and_then(|mut m| m.drain().last());
    if let Some(StartMode(mode)) = requested {
        start(world, mode);
    }
}

/// Starts `mode` in place and resumes play.
pub fn start(world: &mut World, mode: GameMode) {
    if !world.resource::<ModeSwitch>().idle {
        go_idle(world);
    }
    match mode {
        GameMode::Practice => {
            if let Err(e) = world.run_system_cached(crate::dummy::spawn_dummy) {
                error!("modes: couldn't spawn the dummy: {e}");
            }
        }
        GameMode::Waves => {
            ensure_pools(world);
            begin_run(world);
        }
    }
    world.insert_resource(mode);
    let mut switch = world.resource_mut::<ModeSwitch>();
    switch.idle = false;
    switch.starts += 1;
    world
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Playing);
}

/// The grunt pool and the potion pool, created once (the native game makes
/// them during Boot; a game started in Practice makes them on its first run).
fn ensure_pools(world: &mut World) {
    let grunts = world
        .query_filtered::<(), With<PoolGrunt>>()
        .iter(world)
        .count();
    if grunts == 0
        && let Err(e) = world.run_system_cached(spawn_pool)
    {
        error!("modes: couldn't create the grunt pool: {e}");
    }
    let potions = world
        .query_filtered::<(), With<potion::PotionSlot>>()
        .iter(world)
        .count();
    if potions == 0
        && let Err(e) = world.run_system_cached(potion::spawn_potion_pool)
    {
        error!("modes: couldn't create the potions: {e}");
    }
}

fn spawn_pool(mut commands: Commands, tuning: Res<Tuning>) {
    let stats = GruntStats::for_wave(1, &tuning.grunt);
    for _ in 0..tuning.waves.max_alive {
        spawn_pool_member(&mut commands, stats);
    }
}

/// A fresh run: the first from `--seed` (or the game's seed, like a run
/// started at launch), later ones from the menu's own seed stream.
fn begin_run(world: &mut World) {
    let next = world
        .resource_mut::<ModeSwitch>()
        .seeds
        .as_mut()
        .map(Rng::next_u64);
    let seed = match next {
        Some(seed) => seed,
        None => {
            let first = world.get_resource::<RunSeed>().map_or_else(
                || {
                    world
                        .resource::<SimRng>()
                        .0
                        .clone()
                        .fork(WAVES_SALT)
                        .next_u64()
                },
                |s| s.0,
            );
            world.resource_mut::<ModeSwitch>().seeds = Some(Rng::new(first).fork(MENU_SALT));
            first
        }
    };
    let now = world.resource::<SimTick>().0;
    let start = world
        .get_resource::<StartWave>()
        .copied()
        .unwrap_or_default();
    let run = Run::starting_at(seed, now, start.get(), &world.resource::<Tuning>().waves);
    let summary = summarize(
        &run,
        now,
        &CombatStats::default(),
        world.resource::<PersonalBest>(),
    );
    world.insert_resource(summary);
    world.insert_resource(run);
}
