//! App assembly: headless-safe gameplay ([`SimPlugins`]) and the full game
//! ([`ClientPlugins`] on top of Bevy's default plugins).

use crate::{
    arena::{ArenaPlugin, visuals::ArenaVisualsPlugin},
    audio::GameAudioPlugin,
    building::{BuildingPlugin, BuildingVisualsPlugin},
    combat::CombatPlugin,
    dummy::DummyPlugin,
    fx::FxPlugin,
    hud::HudPlugin,
    input::{InputAdapterPlugin, InputProbe, InputProbePlugin},
    menu::MenuPlugin,
    movement::MovementPlugin,
    native::NativeWindowPlugin,
    player::PlayerPlugin,
    render::RenderSetupPlugin,
    rng::{Rng, SimRng},
    scenario::{ScenarioArgs, ScenarioPlugin, ScenarioRun},
    shared::{
        AppState, DamageDealt, Eliminated, GameCue, PieceChanged, PieceHit, ShotFired, SimSet,
        SimTick, tick_duration,
    },
    telemetry::TelemetryPlugin,
    tuning::Tuning,
    viewmodel::ViewmodelPlugin,
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
    window::{MonitorSelection, PresentMode, WindowLevel, WindowMode, WindowResolution},
};
use std::num::NonZero;

/// States, messages, shared resources and fixed-step ordering.
pub struct CorePlugin;

impl Plugin for CorePlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<AppState>()
            .init_resource::<Tuning>()
            .init_resource::<SimTick>()
            .init_resource::<SimRng>()
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
            .add(ArenaVisualsPlugin)
            .add(BuildingVisualsPlugin)
            .add(ViewmodelPlugin)
            .add(FxPlugin)
            .add(GameAudioPlugin)
            .add(HudPlugin)
            .add(MenuPlugin)
            .add(ScenarioPlugin)
            .add(BootPlugin)
            .add(NativeWindowPlugin)
    }
}

/// Moves from `Boot` to `Playing` once the first frame has run.
pub struct BootPlugin;

impl Plugin for BootPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, boot.run_if(in_state(AppState::Boot)));
    }
}

fn boot(mut next: ResMut<NextState<AppState>>) {
    next.set(AppState::Playing);
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
}

impl GameOptions {
    pub fn from_args(args: &[String]) -> Self {
        Self {
            scenario: ScenarioArgs::from_args(args),
            windowed: args.iter().any(|a| a == "--windowed"),
            input_probe: args.iter().any(|a| a == "--input-probe"),
        }
    }
}

/// The full game.
pub fn game_app(options: GameOptions) -> anyhow::Result<App> {
    let tuning = Tuning::load_or_default(&Tuning::settings_path());
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
        present_mode: if tuning.graphics.vsync {
            PresentMode::Fifo
        } else {
            PresentMode::AutoNoVsync
        },
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
    .insert_resource(bevy::winit::WinitSettings::continuous());
    if options.input_probe {
        app.init_resource::<InputProbe>();
    }
    if let Some(run) = scenario {
        app.insert_resource(run);
    }
    Ok(app)
}
