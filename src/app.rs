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
            .add(crate::profile::FrameProfilePlugin)
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

/// Moves from `Boot` to `Playing` once the first frame has run and every
/// [`BootGate`] hold has been released.
pub struct BootPlugin;

impl Plugin for BootPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BootGate>()
            .add_systems(Update, boot.run_if(in_state(AppState::Boot)));
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

fn boot(gate: Res<BootGate>, mut next: ResMut<NextState<AppState>>) {
    if gate.is_open() {
        next.set(AppState::Playing);
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

/// The mode the native game opens in without a flag (M3: Waves; `--practice`
/// gives the M2 sandbox until the main menu lands in chunk 5).
pub const NATIVE_DEFAULT_MODE: GameMode = GameMode::Waves;

/// The full game.
pub fn game_app(options: GameOptions) -> anyhow::Result<App> {
    let mut tuning = Tuning::load_or_default(&Tuning::settings_path());
    options.apply_graphics_overrides(&mut tuning.graphics);
    let mode = options.game_mode();
    let scenario = options.scenario.map(ScenarioRun::new).transpose()?;
    // Parsed before the window and renderer exist: some knobs shape them.
    let knobs = crate::perf_knobs::PerfKnobs::from_args(&std::env::args().collect::<Vec<_>>());
    let knob = |f: fn(&crate::perf_knobs::PerfKnobs) -> Option<bool>| {
        knobs.as_ref().and_then(f).unwrap_or(false)
    };
    let frame_latency = knobs.as_ref().and_then(|k| k.latency).unwrap_or(1);
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
        desired_maximum_frame_latency: NonZero::new(frame_latency),
        window_level: if scenario.is_some() {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        },
        ..default()
    };
    let mut plugins = DefaultPlugins.build();
    if !knob(|k| k.pipelined) {
        plugins = plugins.disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>();
    }
    let mut app = App::new();
    app.add_plugins(
        plugins
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
    // Waves runs keep the personal best and the run log in `userdata/`.
    .insert_resource(RunStore::user())
    .insert_resource(bevy::winit::WinitSettings::continuous());
    if let Some(seed) = options.seed {
        app.insert_resource(RunSeed(seed));
    }
    if knob(|k| k.gpu) {
        app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin)
            .init_resource::<crate::profile::GpuTiming>();
    }
    if let Some(knobs) = knobs {
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
