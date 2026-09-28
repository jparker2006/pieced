//! App assembly: headless-safe gameplay ([`SimPlugins`]) and the full game
//! ([`ClientPlugins`] on top of Bevy's default plugins).

use crate::{
    arena::{ArenaPlugin, visuals::ArenaVisualsPlugin},
    audio::GameAudioPlugin,
    building::{BuildingPlugin, BuildingVisualsPlugin},
    combat::CombatPlugin,
    dummy::DummyPlugin,
    far::FarViewPlugin,
    fx::FxPlugin,
    grunt::GruntPlugin,
    hud::HudPlugin,
    input::{InputAdapterPlugin, InputProbe, InputProbePlugin},
    look::LookPlugin,
    menu::MenuPlugin,
    models::ModelsPlugin,
    movement::MovementPlugin,
    native::NativeWindowPlugin,
    orb::OrbPlugin,
    player::PlayerPlugin,
    render::RenderSetupPlugin,
    rng::{Rng, SimRng},
    scenario::{ScenarioArgs, ScenarioPlugin, ScenarioRun},
    shared::{
        AppState, DamageDealt, Eliminated, GameCue, GameMode, PieceChanged, PieceHit, ShotFired,
        SimSet, SimTick, tick_duration,
    },
    telemetry::TelemetryPlugin,
    tuning::Tuning,
    viewmodel::ViewmodelPlugin,
    waves::{RunSeed, RunStore, WavesClientPlugin, WavesPlugin, ui::WavesUiPlugin},
};
use avian3d::prelude::*;
use bevy::{
    app::PluginGroupBuilder,
    prelude::*,
    render::{
        RenderPlugin,
        settings::{Backends, RenderCreation, WgpuSettings},
    },
    time::TimeUpdateStrategy,
    window::{MonitorSelection, WindowLevel, WindowMode, WindowResolution},
};
use std::{collections::BTreeSet, num::NonZero};

/// States, messages, shared resources and fixed-step ordering.
pub struct CorePlugin;

impl Plugin for CorePlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<AppState>()
            .init_resource::<Tuning>()
            .init_resource::<SimTick>()
            .init_resource::<SimRng>()
            .init_resource::<GameMode>()
            .insert_resource(Time::<Fixed>::from_duration(tick_duration()))
            .add_message::<DamageDealt>()
            .add_message::<ShotFired>()
            .add_message::<PieceChanged>()
            .add_message::<Eliminated>()
            .add_message::<GameCue>()
            .add_message::<PieceHit>()
            .configure_sets(
                FixedUpdate,
                (
                    SimSet::Control,
                    SimSet::Tool,
                    SimSet::Movement,
                    SimSet::Building,
                    SimSet::Combat,
                    SimSet::Resolve,
                )
                    .chain()
                    .run_if(in_state(AppState::Playing)),
            );
    }
}

/// Everything that simulates the game. Safe to run without a window or GPU.
pub struct SimPlugins;

impl PluginGroup for SimPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(CorePlugin)
            .add(ArenaPlugin)
            .add(PlayerPlugin)
            .add(MovementPlugin)
            .add(BuildingPlugin)
            .add(CombatPlugin)
            .add(DummyPlugin)
            .add(GruntPlugin)
            .add(OrbPlugin)
            .add(WavesPlugin)
    }
}

/// Presentation and devices. Only in the full game.
pub struct ClientPlugins;

impl PluginGroup for ClientPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(TelemetryPlugin)
            .add(InputAdapterPlugin)
            .add(InputProbePlugin)
            .add(RenderSetupPlugin)
            .add(LookPlugin)
            .add(ModelsPlugin)
            .add(ArenaVisualsPlugin)
            .add(FarViewPlugin)
            .add(BuildingVisualsPlugin)
            .add(ViewmodelPlugin)
            .add(FxPlugin)
            .add(GameAudioPlugin)
            .add(HudPlugin)
            .add(MenuPlugin)
            .add(WavesUiPlugin)
            .add(WavesClientPlugin)
            // The drop ships' look and sound (M3 chunk 3).
            .add(crate::waves::ships_visuals::ShipsVisualsPlugin)
            .add(ScenarioPlugin)
            .add(BootPlugin)
            .add(NativeWindowPlugin)
    }
}

/// Moves from `Boot` to [`BootTarget`] (`Playing` unless the game opens on
/// the main menu) once the first frame has run and every [`BootGate`] hold
/// has been released.
pub struct BootPlugin;

impl Plugin for BootPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BootGate>()
            .init_resource::<BootTarget>()
            .add_systems(Update, boot.run_if(in_state(AppState::Boot)));
    }
}

/// Where `Boot` hands over: `Playing` (scenarios, `--waves`, `--practice`,
/// tests) or the main menu (the native game without a mode flag, D92).
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootTarget(pub AppState);

impl Default for BootTarget {
    fn default() -> Self {
        Self(AppState::Playing)
    }
}

/// Work that must finish before play starts (model loading, pipeline warm-up).
/// An owner calls [`BootGate::hold`] in `Startup` and [`BootGate::release`] once
/// its work is done; `Boot` hands over to `Playing` when nothing is held.
#[derive(Resource, Debug, Default)]
pub struct BootGate {
    held: BTreeSet<&'static str>,
}

impl BootGate {
    pub fn hold(&mut self, key: &'static str) {
        self.held.insert(key);
    }

    pub fn release(&mut self, key: &'static str) {
        self.held.remove(key);
    }

    pub fn is_open(&self) -> bool {
        self.held.is_empty()
    }

    /// What is still being waited on, for launch telemetry and logs.
    pub fn held(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.held.iter().copied()
    }
}

fn boot(
    gate: Res<BootGate>,
    target: Option<Res<BootTarget>>,
    mut next: ResMut<NextState<AppState>>,
) {
    if gate.is_open() {
        next.set(target.map_or(AppState::Playing, |t| t.0));
    }
}

/// A windowless app with the full simulation, stepped one fixed tick per `update()`.
pub fn headless_app(seed: u64) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        bevy::state::app::StatesPlugin,
        AssetPlugin::default(),
        bevy::mesh::MeshPlugin,
        bevy::scene::ScenePlugin,
        PhysicsPlugins::default(),
    ))
    .add_plugins(SimPlugins)
    .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()))
    .insert_resource(SimRng(Rng::new(seed)));
    app.finish();
    app.cleanup();
    app
}

/// Options for the full game.
#[derive(Debug, Clone, Default)]
pub struct GameOptions {
    pub scenario: Option<ScenarioArgs>,
    pub windowed: bool,
    pub input_probe: bool,
    /// `--no-vsync`: present without vsync, paced by the frame cap.
    pub no_vsync: bool,
    /// `--frame-cap N`: frame cap used without vsync (0 = uncapped).
    pub frame_cap: Option<u32>,
    /// `--waves` / `--practice`: the game mode. Scenarios always run Practice.
    pub mode: Option<GameMode>,
    /// `--seed N`: the first Waves run's seed (D83: replay a run).
    pub seed: Option<u64>,
}

impl GameOptions {
    pub fn from_args(args: &[String]) -> Self {
        Self {
            scenario: ScenarioArgs::from_args(args),
            windowed: args.iter().any(|a| a == "--windowed"),
            input_probe: args.iter().any(|a| a == "--input-probe"),
            no_vsync: args.iter().any(|a| a == "--no-vsync"),
            frame_cap: args
                .iter()
                .position(|a| a == "--frame-cap")
                .and_then(|i| args.get(i + 1))
                .and_then(|v| v.parse().ok()),
            mode: if args.iter().any(|a| a == "--practice") {
                Some(GameMode::Practice)
            } else if args.iter().any(|a| a == "--waves") {
                Some(GameMode::Waves)
            } else {
                None
            },
            seed: args
                .iter()
                .position(|a| a == "--seed")
                .and_then(|i| args.get(i + 1))
                .and_then(|v| v.parse().ok()),
        }
    }

    /// The mode the native game starts in: scenarios always run Practice (the
    /// M1/M2 gate runs drive the dummy); otherwise the flag, else the default.
    pub fn game_mode(&self) -> GameMode {
        if self.scenario.is_some() {
            GameMode::Practice
        } else {
            self.mode.unwrap_or(NATIVE_DEFAULT_MODE)
        }
    }

    /// The first Waves run's seed: `--seed N` replays run N exactly;
    /// otherwise a fresh seed from OS entropy and the clock, so every launch
    /// plays new runs (the fixed `SimRng` would repeat the same first run).
    /// Later runs (the menu's, Go again) draw their seeds from this one.
    pub fn run_seed(&self) -> u64 {
        self.seed.unwrap_or_else(entropy_seed)
    }

    /// Where the native game goes after `Boot`: the main menu, unless a
    /// scenario or `--waves`/`--practice` starts play straight away.
    pub fn boot_target(&self) -> AppState {
        if self.scenario.is_some() || self.mode.is_some() {
            AppState::Playing
        } else {
            AppState::Menu
        }
    }

    /// Applies the command-line graphics overrides (vsync A/B runs).
    pub fn apply_graphics_overrides(&self, graphics: &mut crate::render::GraphicsTuning) {
        if self.no_vsync {
            graphics.vsync = false;
        }
        if let Some(cap) = self.frame_cap {
            graphics.frame_cap = cap;
        }
    }
}

/// A seed nobody chose: the standard library's per-process OS randomness
/// (`RandomState`), mixed with the wall clock and a call counter.
pub fn entropy_seed() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    h.write_u128(now);
    h.write_u64(CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    h.write_u32(std::process::id());
    h.finish()
}

/// The mode the native game is built in without a flag. It opens on the main
/// menu (chunk 5), which starts Waves or Practice in place; Waves at startup
/// means the grunt pool and the potions are created and warmed during Boot.
pub const NATIVE_DEFAULT_MODE: GameMode = GameMode::Waves;

/// The full game.
pub fn game_app(options: GameOptions) -> anyhow::Result<App> {
    let mut tuning = Tuning::load_or_default(&Tuning::settings_path());
    options.apply_graphics_overrides(&mut tuning.graphics);
    let mode = options.game_mode();
    let boot_target = BootTarget(options.boot_target());
    let run_seed = RunSeed(options.run_seed());
    let scenario = options.scenario.map(ScenarioRun::new).transpose()?;
    let windowed = options.windowed || !tuning.graphics.fullscreen;
    let window = Window {
        title: "Pieced".into(),
        mode: if windowed {
            WindowMode::Windowed
        } else {
            WindowMode::BorderlessFullscreen(MonitorSelection::Primary)
        },
        resolution: WindowResolution::new(1280, 800),
        present_mode: crate::render::present_mode_for(&tuning.graphics),
        desired_maximum_frame_latency: NonZero::new(1),
        window_level: if scenario.is_some() {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        },
        ..default()
    };
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .build()
            .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>()
            .set(WindowPlugin {
                primary_window: Some(window),
                ..default()
            })
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
                    backends: Some(Backends::METAL),
                    ..default()
                })),
                ..default()
            }),
    )
    .add_plugins(PhysicsPlugins::default())
    .add_plugins(SimPlugins)
    .add_plugins(ClientPlugins)
    .insert_resource(tuning)
    .insert_resource(mode)
    .insert_resource(boot_target)
    // Waves runs keep the personal best and the run log in `userdata/`.
    .insert_resource(RunStore::user())
    .insert_resource(bevy::winit::WinitSettings::continuous());
    // Always a run seed in the native game: `--seed`, or fresh entropy.
    // Headless tests don't come through here, so they stay deterministic.
    app.insert_resource(run_seed);
    if let Some(knobs) =
        crate::perf_knobs::PerfKnobs::from_args(&std::env::args().collect::<Vec<_>>())
    {
        app.insert_resource(knobs)
            .add_plugins(crate::perf_knobs::PerfKnobsPlugin);
    }
    if options.input_probe {
        app.init_resource::<InputProbe>();
    }
    // Launch kind always; the session frame log only outside scenarios.
    app.add_plugins(crate::session::SessionPlugin::native(scenario.is_none()));
    if let Some(run) = scenario {
        app.insert_resource(run);
    }
    Ok(app)
}

#[cfg(test)]
mod tests {
    use super::BootGate;

    #[test]
    fn boot_gate_opens_only_when_every_hold_is_released() {
        let mut gate = BootGate::default();
        assert!(gate.is_open());
        gate.hold("models");
        gate.hold("warmup");
        gate.release("models");
        assert!(!gate.is_open());
        assert_eq!(gate.held().collect::<Vec<_>>(), ["warmup"]);
        gate.release("warmup");
        assert!(gate.is_open());
    }
}
