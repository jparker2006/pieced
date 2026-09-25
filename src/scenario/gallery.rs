//! Slice D — `gallery`: the eight fixed art-review views, each written with a
//! greyscale copy. Views are driven only through the player's intent, `set_look`
//! and `teleport`; the two high vantage shots use [`GalleryCamera`], a
//! scenario-only camera override that nothing else inserts.
//!
//! Building and shooting come from other slices: the intents are scripted here, so
//! the box, ramp, ADS, hit-effect, debris and HUD views fill in once those merge.

use super::{Director, DirectorStatus, ScenarioClock, capture, set_look, teleport, with_intent};
use crate::{
    arena::ArenaLayout,
    dummy::Dummy,
    player::spawn_character,
    render::MainCamera,
    shared::{
        ActiveTool, Ads, EyeHeight, Facing, Health, LookAngles, PieceChange, PieceChanged,
        PieceKind, Player, ShotFired, WeaponKind,
    },
};
use bevy::{ecs::message::MessageCursor, prelude::*};
use serde_json::{Value, json};

pub fn director(name: &str) -> Option<Box<dyn Director>> {
    (name == "gallery").then(|| Box::new(Gallery::new()) as Box<dyn Director>)
}

/// Scenario-only override of the main camera for the high vantage shots. Only the
/// gallery inserts it; while present it replaces the first-person eye.
#[derive(Resource, Debug, Clone, Copy)]
pub struct GalleryCamera(pub Transform);

/// Runs after transform propagation (and before frusta and shadow cascades are
/// computed) so the override wins over the first-person camera follow.
pub fn apply_camera_override(
    over: Option<Res<GalleryCamera>>,
    mut cameras: Query<(&mut Transform, &mut GlobalTransform, Option<&Children>), With<MainCamera>>,
    mut children: Query<(&Transform, &mut GlobalTransform), Without<MainCamera>>,
) {
    let Some(over) = over else {
        return;
    };
    for (mut transform, mut global, kids) in &mut cameras {
        *transform = over.0;
        *global = GlobalTransform::from(over.0);
        for kid in kids.into_iter().flat_map(|k| k.iter()) {
            if let Ok((local, mut kid_global)) = children.get_mut(kid) {
                *kid_global = global.mul_transform(*local);
            }
        }
    }
}

/// Where the player aims every frame during a shot.
#[derive(Debug, Clone, Copy)]
enum Aim {
    /// The dummy, at this height above its feet.
    Dummy(f32),
    Point(Vec3),
}

#[derive(Debug, Clone, Copy)]
enum Act {
    Camera(Option<Transform>),
    Teleport(Vec3),
    Look(f32, f32),
    Track(Option<Aim>),
    Select(ActiveTool),
    /// Press and release the primary action (fire / place).
    Tap,
    Hold(bool),
    Move(Vec2),
    /// Toggle ADS if it isn't already in the wanted state.
    Ads(bool),
    Capture(&'static str),
    /// Capture a few frames after the next piece breaks (falls back to `Capture`).
    CaptureOnBreak(&'static str),
}

struct Shot {
    script: Vec<(f64, Act)>,
    /// Seconds after the last action before the next shot starts.
    tail: f64,
}

/// Seconds before the first shot, so pipelines finish compiling.
const WARMUP: f64 = 2.5;
const EYE: f32 = 1.62;

fn vantage(eye: Vec3, target: Vec3) -> Transform {
    Transform::from_translation(eye).looking_at(target, Vec3::Y)
}

fn shots() -> Vec<Shot> {
    use Act::*;
    let wall = ActiveTool::Build(PieceKind::Wall);
    let floor = ActiveTool::Build(PieceKind::Floor);
    let ramp = ActiveTool::Build(PieceKind::Ramp);
    let rifle = ActiveTool::Weapon(WeaponKind::Rifle);
    let pump = ActiveTool::Weapon(WeaponKind::Pump);
    let sun = crate::arena::visuals::sun_direction();
    let sunward = Vec3::new(sun.x, 0.0, sun.z).normalize();
    let sunset_eye = Vec3::new(14.0, 11.0, 12.0);
    vec![
        // 1. Arena overview from a high vantage, sun behind the camera.
        Shot {
            script: vec![
                (
                    0.0,
                    Camera(Some(vantage(
                        Vec3::new(-30.0, 26.0, 38.0),
                        Vec3::new(2.0, 0.0, -6.0),
                    ))),
                ),
                (1.2, Capture("01-overview")),
            ],
            tail: 0.1,
        },
        // 2. Looking into the sun over the western cliffs: sky, glow, mesas.
        Shot {
            script: vec![
                (
                    0.0,
                    Camera(Some(vantage(
                        sunset_eye,
                        sunset_eye + sunward * 100.0 + Vec3::Y * 17.0,
                    ))),
                ),
                (1.2, Capture("02-sunset")),
            ],
            tail: 0.1,
        },
        // 3. Four walls and a roof around the player, then look into a corner.
        Shot {
            script: vec![
                (0.0, Camera(None)),
                (0.0, Teleport(Vec3::new(-14.0, 0.0, 10.0))),
                (0.0, Look(Facing::North.yaw(), 0.0)),
                (0.1, Select(wall)),
                (0.4, Tap),
                (0.55, Look(Facing::East.yaw(), 0.0)),
                (0.7, Tap),
                (0.85, Look(Facing::South.yaw(), 0.0)),
                (1.0, Tap),
                (1.15, Look(Facing::West.yaw(), 0.0)),
                (1.3, Tap),
                (1.45, Select(floor)),
                (1.5, Look(Facing::North.yaw(), 1.35)),
                (1.75, Tap),
                (1.9, Look(Facing::North.yaw() - 0.6, 0.18)),
                (2.9, Capture("03-box")),
            ],
            tail: 0.1,
        },
        // 4. Build a ramp, run up it and look out over the arena.
        Shot {
            script: vec![
                (0.0, Teleport(Vec3::new(14.0, 0.0, 18.0))),
                (0.0, Look(Facing::North.yaw(), -0.3)),
                (0.1, Select(ramp)),
                (0.45, Tap),
                (0.6, Look(Facing::North.yaw(), 0.0)),
                (0.6, Move(Vec2::Y)),
                (1.55, Move(Vec2::ZERO)),
                (1.6, Track(Some(Aim::Point(Vec3::new(-4.0, 1.0, -8.0))))),
                (2.6, Capture("04-ramp-top")),
            ],
            tail: 0.1,
        },
        // 5. Rifle aimed down sights at the dummy.
        Shot {
            script: vec![
                (0.0, Track(None)),
                (0.0, Teleport(Vec3::new(2.0, 0.0, 6.0))),
                (0.0, Select(rifle)),
                (0.05, Track(Some(Aim::Dummy(1.1)))),
                (0.4, Ads(true)),
                (1.4, Capture("05-rifle-ads")),
                (1.5, Ads(false)),
            ],
            tail: 0.2,
        },
        // 6. Point-blank pump shot on the dummy, caught mid-effect.
        Shot {
            script: vec![
                (0.0, Teleport(Vec3::new(2.0, 0.0, -1.0))),
                (0.0, Select(pump)),
                (0.05, Track(Some(Aim::Dummy(0.95)))),
                (1.1, Tap),
                (1.16, Capture("06-pump-hit")),
            ],
            tail: 0.6,
        },
        // 8 (run before 7 so the dummy is still standing). A rifle burst, HUD up.
        Shot {
            script: vec![
                (0.0, Teleport(Vec3::new(-6.0, 0.0, 4.0))),
                (0.0, Select(rifle)),
                (0.05, Track(Some(Aim::Dummy(1.05)))),
                (1.0, Hold(true)),
                (1.3, Capture("08-hud-mid-fight")),
                (1.36, Hold(false)),
            ],
            tail: 0.3,
        },
        // 7. Wall up, pump it down, catch the debris.
        Shot {
            script: vec![
                (0.0, Track(None)),
                (0.0, Teleport(Vec3::new(-14.0, 0.0, -14.0))),
                (0.0, Look(Facing::West.yaw(), 0.0)),
                (0.1, Select(wall)),
                (0.4, Tap),
                (0.6, Select(pump)),
                (0.7, Track(Some(Aim::Point(Vec3::new(-16.0, 1.4, -14.0))))),
                (1.1, CaptureOnBreak("07-piece-debris")),
                (1.1, Tap),
                (2.1, Tap),
                (3.1, Tap),
                (3.5, Capture("07-piece-debris")),
            ],
            tail: 0.3,
        },
    ]
}

struct Gallery {
    shots: Vec<Shot>,
    shot: usize,
    shot_start: Option<f64>,
    next_act: usize,
    aim: Option<Aim>,
    release_fire: bool,
    captured: Vec<&'static str>,
    on_break: Option<&'static str>,
    break_countdown: Option<u32>,
    display_dummy: Option<bool>,
    piece_cursor: MessageCursor<PieceChanged>,
    shot_cursor: MessageCursor<ShotFired>,
    pieces_placed: u32,
    pieces_broken: u32,
    shots_fired: u32,
    /// Simulation tick of the last applied action: at most one action per tick,
    /// so a frame hitch can't collapse "look, tap, look, tap" into a single tap.
    last_act_tick: Option<u64>,
}

impl Gallery {
    fn new() -> Self {
        Self {
            shots: shots(),
            shot: 0,
            shot_start: None,
            next_act: 0,
            aim: None,
            release_fire: false,
            captured: Vec::new(),
            on_break: None,
            break_countdown: None,
            display_dummy: None,
            piece_cursor: MessageCursor::default(),
            shot_cursor: MessageCursor::default(),
            pieces_placed: 0,
            pieces_broken: 0,
            shots_fired: 0,
            last_act_tick: None,
        }
    }

    fn capture_once(&mut self, world: &mut World, name: &'static str) {
        if !self.captured.contains(&name) {
            capture(world, name, true);
            self.captured.push(name);
        }
    }

    fn act(&mut self, world: &mut World, act: Act) {
        match act {
            Act::Camera(Some(t)) => world.insert_resource(GalleryCamera(t)),
            Act::Camera(None) => {
                world.remove_resource::<GalleryCamera>();
            }
            Act::Teleport(feet) => {
                with_intent(world, |i| {
                    i.move_axis = Vec2::ZERO;
                    i.fire = false;
                });
                teleport(world, feet);
            }
            Act::Look(yaw, pitch) => set_look(world, yaw, pitch),
            Act::Track(aim) => self.aim = aim,
            Act::Select(tool) => with_intent(world, |i| i.select = Some(tool)),
            Act::Tap => {
                with_intent(world, |i| {
                    i.fire = true;
                    i.fire_pressed = true;
                });
                self.release_fire = true;
            }
            Act::Hold(on) => with_intent(world, |i| {
                i.fire = on;
                i.fire_pressed |= on;
            }),
            Act::Move(axis) => with_intent(world, |i| i.move_axis = axis),
            Act::Ads(want) => {
                if player_ads(world) != want {
                    with_intent(world, |i| i.ads_toggle_pressed = true);
                }
            }
            Act::Capture(name) => {
                self.on_break = None;
                self.break_countdown = None;
                self.capture_once(world, name);
            }
            Act::CaptureOnBreak(name) => self.on_break = Some(name),
        }
    }

    /// Reads gameplay messages: counts for the summary, and piece breaks for the
    /// debris capture.
    fn watch(&mut self, world: &mut World) {
        let mut broke = false;
        for change in self
            .piece_cursor
            .read(world.resource::<Messages<PieceChanged>>())
        {
            match change.change {
                PieceChange::Placed => self.pieces_placed += 1,
                PieceChange::Destroyed => {
                    self.pieces_broken += 1;
                    broke = true;
                }
                PieceChange::Cracked(_) => {}
            }
        }
        self.shots_fired += self
            .shot_cursor
            .read(world.resource::<Messages<ShotFired>>())
            .count() as u32;
        if broke && self.on_break.is_some() && self.break_countdown.is_none() {
            // A few frames in, the debris is flying.
            self.break_countdown = Some(4);
        }
        match self.break_countdown {
            Some(0) => {
                self.break_countdown = None;
                if let Some(name) = self.on_break.take() {
                    self.capture_once(world, name);
                }
            }
            Some(n) => self.break_countdown = Some(n - 1),
            None => {}
        }
    }

    /// Before the first shot: make sure a target stands in the arena. If slice C's
    /// dummy isn't spawned (not merged yet), a static stand-in takes its place.
    fn ensure_dummy(&mut self, world: &mut World) {
        if self.display_dummy.is_some() {
            return;
        }
        let exists = world
            .query_filtered::<(), With<Dummy>>()
            .iter(world)
            .next()
            .is_some();
        self.display_dummy = Some(!exists);
        if exists {
            return;
        }
        let feet = world.resource::<ArenaLayout>().dummy_spawn;
        let look = LookAngles {
            yaw: Facing::South.yaw(),
            pitch: 0.0,
        };
        let mut commands = world.commands();
        spawn_character(&mut commands, feet, look, Health::default(), Dummy);
        world.flush();
    }

    fn track(&self, world: &mut World) {
        let Some(aim) = self.aim else {
            return;
        };
        let target = match aim {
            Aim::Point(p) => p,
            Aim::Dummy(h) => dummy_feet(world) + Vec3::Y * h,
        };
        let Some(eye) = player_eye(world) else {
            return;
        };
        let d = (target - eye).normalize_or_zero();
        if d == Vec3::ZERO {
            return;
        }
        let yaw = (-d.x).atan2(-d.z);
        set_look(world, yaw, d.y.clamp(-1.0, 1.0).asin());
    }
}

fn dummy_feet(world: &mut World) -> Vec3 {
    world
        .query_filtered::<&Transform, With<Dummy>>()
        .iter(world)
        .next()
        .map(|t| t.translation)
        .unwrap_or_else(|| world.resource::<ArenaLayout>().dummy_spawn)
}

fn player_eye(world: &mut World) -> Option<Vec3> {
    world
        .query_filtered::<(&Transform, Option<&EyeHeight>), With<Player>>()
        .iter(world)
        .next()
        .map(|(t, eye)| t.translation + Vec3::Y * eye.map_or(EYE, |e| e.0))
}

fn player_ads(world: &mut World) -> bool {
    world
        .query_filtered::<&Ads, With<Player>>()
        .iter(world)
        .next()
        .is_some_and(|a| a.0)
}

impl Director for Gallery {
    fn update(&mut self, world: &mut World, clock: &ScenarioClock) -> DirectorStatus {
        if self.release_fire {
            with_intent(world, |i| i.fire = false);
            self.release_fire = false;
        }
        self.watch(world);
        // Checked every frame from 0.5 s on: a long pipeline-compile stall can jump
        // the clock straight past the warmup.
        if clock.seconds > 0.5 {
            self.ensure_dummy(world);
        }
        if clock.seconds < WARMUP {
            return DirectorStatus::Running;
        }
        let Some(shot) = self.shots.get(self.shot) else {
            return DirectorStatus::Done;
        };
        let start = *self.shot_start.get_or_insert(clock.seconds);
        let t = clock.seconds - start;
        let mut due = Vec::new();
        if self.last_act_tick != Some(clock.tick)
            && let Some(&(at, act)) = shot.script.get(self.next_act)
            && t >= at
        {
            due.push(act);
            self.next_act += 1;
            self.last_act_tick = Some(clock.tick);
        }
        let finished = self.next_act >= shot.script.len()
            && t >= shot.script.last().map_or(0.0, |(at, _)| *at) + shot.tail;
        for act in due {
            self.act(world, act);
        }
        self.track(world);
        if finished {
            self.shot += 1;
            self.shot_start = None;
            self.next_act = 0;
        }
        DirectorStatus::Running
    }

    fn warmup_seconds(&self) -> f64 {
        WARMUP
    }

    fn summary(&mut self, _world: &mut World) -> Value {
        json!({
            "views": self.captured,
            "pieces_placed": self.pieces_placed,
            "pieces_broken": self.pieces_broken,
            "shots_fired": self.shots_fired,
            "display_dummy_spawned": self.display_dummy.unwrap_or(false),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gallery_scripts_all_eight_views_with_settle_time() {
        let mut names = Vec::new();
        for shot in shots() {
            let mut last_setup = 0.0;
            for (at, act) in &shot.script {
                match act {
                    Act::Capture(name) => {
                        names.push(*name);
                        assert!(at - last_setup >= 0.9, "{name} captured before settling");
                    }
                    Act::CaptureOnBreak(_)
                    | Act::Ads(false)
                    | Act::Hold(_)
                    | Act::Tap
                    | Act::Select(_) => {}
                    _ => last_setup = *at,
                }
            }
            // Scripts are in time order.
            assert!(shot.script.windows(2).all(|w| w[0].0 <= w[1].0));
        }
        names.sort();
        names.dedup();
        assert_eq!(
            names,
            [
                "01-overview",
                "02-sunset",
                "03-box",
                "04-ramp-top",
                "05-rifle-ads",
                "06-pump-hit",
                "07-piece-debris",
                "08-hud-mid-fight"
            ]
        );
    }

    #[test]
    fn camera_override_only_from_the_gallery() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_systems(Update, apply_camera_override);
        let cam = app
            .world_mut()
            .spawn((MainCamera, Transform::from_xyz(1.0, 2.0, 3.0)))
            .id();
        app.update();
        let t = app.world().get::<Transform>(cam).unwrap().translation;
        assert_eq!(t, Vec3::new(1.0, 2.0, 3.0));
        app.world_mut()
            .insert_resource(GalleryCamera(Transform::from_xyz(0.0, 30.0, 0.0)));
        app.update();
        let g = app
            .world()
            .get::<GlobalTransform>(cam)
            .unwrap()
            .translation();
        assert_eq!(g, Vec3::new(0.0, 30.0, 0.0));
    }
}
