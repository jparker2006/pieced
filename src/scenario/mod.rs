//! Scenario runner: plays scripted sessions with the real renderer by writing the
//! player's `PlayerIntent`, captures screenshots, and writes an evidence folder
//! (`frames.csv`, `summary.json`, PNGs). Launch with
//! `pieced --scenario <name> [--evidence <dir>] [--seconds <n>] [--warmup <s>]`.
//!
//! Scenario slices add a director in their own file and return it from that file's
//! `director(name)` function.

pub mod fx_check;
pub mod gallery;
pub mod latency;
pub mod perf;
pub mod smoke;
pub mod ttk;

use crate::{
    player::IntentWriters,
    shared::{AppState, LookAngles, Player, PlayerIntent, SimTick},
    telemetry::{self, FrameLog, LaunchTime},
    tuning::Tuning,
};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Instant};

/// Command-line scenario request.
#[derive(Debug, Clone)]
pub struct ScenarioArgs {
    pub name: String,
    pub out: PathBuf,
    pub seconds: Option<f64>,
    pub warmup: Option<f64>,
}

impl ScenarioArgs {
    pub fn from_args(args: &[String]) -> Option<Self> {
        let value = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        let name = value("--scenario")?;
        let out = value("--evidence").map(PathBuf::from).unwrap_or_else(|| {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("evidence")
                .join(format!("{name}-{stamp}"))
        });
        Some(Self {
            name,
            out,
            seconds: value("--seconds").and_then(|v| v.parse().ok()),
            warmup: value("--warmup").and_then(|v| v.parse().ok()),
        })
    }
}

/// Timing given to a director each frame.
#[derive(Debug, Clone, Copy)]
pub struct ScenarioClock {
    /// Frames since the scenario started playing.
    pub frame: u64,
    /// Seconds since the scenario started playing.
    pub seconds: f64,
    /// Fixed simulation ticks so far.
    pub tick: u64,
    /// Requested duration override, if any.
    pub requested_seconds: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectorStatus {
    Running,
    Done,
}

/// A scripted session. Runs every frame in `PreUpdate` (before the fixed step)
/// with full world access; it normally writes the player's intent and look.
pub trait Director: Send + Sync + 'static {
    fn update(&mut self, world: &mut World, clock: &ScenarioClock) -> DirectorStatus;
    /// Seconds excluded from the frame summary.
    fn warmup_seconds(&self) -> f64 {
        0.0
    }
    /// Extra JSON merged into `summary.json` under `"scenario"`.
    fn summary(&mut self, _world: &mut World) -> Value {
        Value::Null
    }
}

/// Returns the director for a scenario name.
pub fn director_for(name: &str) -> Option<Box<dyn Director>> {
    smoke::director(name)
        .or_else(|| gallery::director(name))
        .or_else(|| perf::director(name))
        .or_else(|| ttk::director(name))
        .or_else(|| latency::director(name))
        .or_else(|| fx_check::director(name))
}

/// Present while a scenario controls the game.
#[derive(Resource)]
pub struct ScenarioRun {
    pub name: String,
    pub out: PathBuf,
    pub seconds: Option<f64>,
    pub warmup_override: Option<f64>,
    director: Option<Box<dyn Director>>,
    started: Option<Instant>,
    frame: u64,
    exit_countdown: Option<u32>,
    power_start: telemetry::PowerState,
    screenshots: Vec<String>,
}

impl ScenarioRun {
    pub fn new(args: ScenarioArgs) -> anyhow::Result<Self> {
        let director = director_for(&args.name)
            .ok_or_else(|| anyhow::anyhow!("unknown scenario '{}'", args.name))?;
        std::fs::create_dir_all(&args.out)?;
        Ok(Self {
            name: args.name,
            out: args.out,
            seconds: args.seconds,
            warmup_override: args.warmup,
            director: Some(director),
            started: None,
            frame: 0,
            exit_countdown: None,
            power_start: telemetry::power_state(),
            screenshots: Vec::new(),
        })
    }
}

pub struct ScenarioPlugin;

impl Plugin for ScenarioPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, enable_frame_log).add_systems(
            PreUpdate,
            drive
                .in_set(IntentWriters)
                .run_if(resource_exists::<ScenarioRun>),
        );
    }
}

fn enable_frame_log(mut log: ResMut<FrameLog>, run: Option<Res<ScenarioRun>>) {
    if run.is_some() {
        log.enabled = true;
    }
}

fn drive(world: &mut World) {
    if *world.resource::<State<AppState>>().get() != AppState::Playing {
        return;
    }
    let tick = world.resource::<SimTick>().0;
    let (mut director, clock, countdown) = {
        let mut run = world.resource_mut::<ScenarioRun>();
        let started = *run.started.get_or_insert_with(Instant::now);
        run.frame += 1;
        let clock = ScenarioClock {
            frame: run.frame,
            seconds: started.elapsed().as_secs_f64(),
            tick,
            requested_seconds: run.seconds,
        };
        (run.director.take(), clock, run.exit_countdown)
    };
    if let Some(remaining) = countdown {
        world.resource_mut::<ScenarioRun>().director = director;
        if remaining == 0 {
            finish(world);
        } else {
            world.resource_mut::<ScenarioRun>().exit_countdown = Some(remaining - 1);
        }
        return;
    }
    let status = director
        .as_mut()
        .map(|d| d.update(world, &clock))
        .unwrap_or(DirectorStatus::Done);
    let mut run = world.resource_mut::<ScenarioRun>();
    run.director = director;
    if status == DirectorStatus::Done {
        // Let pending screenshot readbacks land before exiting.
        run.exit_countdown = Some(30);
    }
}

fn finish(world: &mut World) {
    let mut director = world.resource_mut::<ScenarioRun>().director.take();
    let extra = director
        .as_mut()
        .map(|d| d.summary(world))
        .unwrap_or(Value::Null);
    let warmup = {
        let run = world.resource::<ScenarioRun>();
        run.warmup_override
            .or_else(|| director.as_ref().map(|d| d.warmup_seconds()))
            .unwrap_or(0.0)
    };
    let rows = world.resource::<FrameLog>().rows.clone();
    let intervals = telemetry::intervals_after_warmup(&rows, warmup * 1000.0);
    let summary = telemetry::summarize(&intervals);
    let launch_ms = world
        .resource::<LaunchTime>()
        .0
        .map(|d| d.as_secs_f64() * 1000.0);
    let graphics = telemetry::tuning_graphics(world.resource::<Tuning>());
    let run = world.resource::<ScenarioRun>();
    let out = run.out.clone();
    let doc = json!({
        "scenario_name": run.name,
        "commit": telemetry::git_commit(),
        "build": if cfg!(debug_assertions) { "debug" } else { "release" },
        "power_start": run.power_start,
        "power_end": telemetry::power_state(),
        "graphics": graphics,
        "launch_to_controllable_ms": launch_ms,
        "warmup_seconds": warmup,
        "frames": summary,
        "screenshots": run.screenshots,
        "scenario": extra,
    });
    if let Err(e) = telemetry::write_csv(&rows, &out.join("frames.csv")) {
        eprintln!("frame log not written: {e}");
    }
    match serde_json::to_string_pretty(&doc) {
        Ok(text) => {
            if let Err(e) = std::fs::write(out.join("summary.json"), text) {
                eprintln!("summary not written: {e}");
            }
        }
        Err(e) => eprintln!("summary not serialized: {e}"),
    }
    println!("PIECED_SCENARIO_DONE {}", out.display());
    world.write_message(AppExit::Success);
}

// ---------------------------------------------------------------------------
// Helpers for directors
// ---------------------------------------------------------------------------

pub fn player_entity(world: &mut World) -> Option<Entity> {
    world
        .query_filtered::<Entity, With<Player>>()
        .iter(world)
        .next()
}

/// Mutates the player's intent.
pub fn with_intent(world: &mut World, f: impl FnOnce(&mut PlayerIntent)) {
    if let Some(player) = player_entity(world)
        && let Some(mut intent) = world.get_mut::<PlayerIntent>(player)
    {
        f(&mut intent);
    }
}

/// Sets the player's absolute look direction.
pub fn set_look(world: &mut World, yaw: f32, pitch: f32) {
    if let Some(player) = player_entity(world)
        && let Some(mut look) = world.get_mut::<LookAngles>(player)
    {
        look.yaw = yaw;
        look.pitch = pitch.clamp(-LookAngles::PITCH_LIMIT, LookAngles::PITCH_LIMIT);
    }
}

/// Teleports the player's feet (also resets interpolation).
pub fn teleport(world: &mut World, feet: Vec3) {
    if let Some(player) = player_entity(world) {
        if let Some(mut t) = world.get_mut::<Transform>(player) {
            t.translation = feet;
        }
        if let Some(mut p) = world.get_mut::<crate::shared::PreviousFeet>(player) {
            p.0 = feet;
        }
    }
}

/// Captures the window (world + UI) to `<evidence>/<name>.png`, plus a greyscale
/// copy `<name>-grey.png` when requested.
pub fn capture(world: &mut World, name: &str, grey_copy: bool) {
    let out = world.resource::<ScenarioRun>().out.clone();
    world
        .resource_mut::<ScenarioRun>()
        .screenshots
        .push(format!("{name}.png"));
    let path = out.join(format!("{name}.png"));
    let grey = grey_copy.then(|| out.join(format!("{name}-grey.png")));
    world
        .spawn(Screenshot::primary_window())
        .observe(move |capture: On<ScreenshotCaptured>| {
            let image = capture.image.clone();
            let path = path.clone();
            let grey = grey.clone();
            std::thread::spawn(move || {
                let Ok(image) = image.try_into_dynamic() else {
                    eprintln!("screenshot conversion failed: {}", path.display());
                    return;
                };
                if let Err(e) = image.to_rgb8().save(&path) {
                    eprintln!("screenshot not saved to {}: {e}", path.display());
                }
                if let Some(grey) = grey
                    && let Err(e) = image.to_luma8().save(&grey)
                {
                    eprintln!("greyscale copy not saved to {}: {e}", grey.display());
                }
            });
        });
}
