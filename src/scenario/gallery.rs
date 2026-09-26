//! `gallery`: the twelve target-board views (docs/M2-SPEC.md → The target board
//! and gallery; gate S1). Each [`GalleryView`] matches one target image,
//! `docs/design/concepts/T01-…png` to `T12-…png` (local only, never committed):
//! where the player stands and aims (or a [`GalleryCamera`] override), the tool,
//! the knight's spot and state, the pieces built for the view, the scripted
//! actions (fire, switch, pause) and the moment to capture. At that moment the
//! runner sets [`GalleryFreeze`], so the knight, the dummy and every effect hold
//! still, and captures `<name>.png` plus a greyscale `<name>-grey.png`.
//!
//! One table ([`views`]) and one [`GalleryRunner`] drive both the native scenario
//! (`pieced --scenario gallery`, evidence in `evidence/gallery-<stamp>/`) and the
//! windowless review render (`tests/gallery_offscreen.rs`). Jake scores the views
//! on the board page: `python3 scripts/board.py <gallery-dir>`.
//!
//! Screen positions are in half-heights from the frame's centre, x right and y
//! up, so they read the same on the 16:9 targets and the 16:10 game: `(0, 0.5)`
//! sits halfway from the centre to the top edge. [`target_screen`] converts a
//! target pixel.
//!
//! The targets paint the knight two to three times closer than the distances
//! Jake approved (T05 "at 10 m" is drawn about 4 m away at the game's 70° FOV).
//! The views keep the approved distances and match where the knight sits in the
//! frame; the distances are one-line changes in the table.

use super::{
    Director, DirectorStatus, ScenarioClock, capture, player_entity, teleport, with_intent,
};
use crate::{
    building::{BuildTarget, Piece, PieceMap, PieceSlot, place_piece},
    combat::Downed,
    dummy::{Dummy, DummyBrain, look_toward},
    menu::{MenuPage, MenuState},
    player::spawn_character,
    render::{CameraFollowSet, MainCamera},
    shared::{
        ActiveTool, AppState, DamageDealt, DamageTarget, EyeHeight, Facing, GalleryFreeze,
        GridCell, Health, LookAngles, PieceKind, Player, PlayerIntent, PreviousFeet, ShotFired,
        WeaponKind,
    },
    tuning::Tuning,
    viewmodel::ViewmodelSet,
};
use bevy::{ecs::message::MessageCursor, prelude::*};
use serde::Serialize;
use serde_json::{Value, json};

pub fn director(name: &str) -> Option<Box<dyn Director>> {
    (name == "gallery").then(|| Box::new(Gallery::new()) as Box<dyn Director>)
}

// ---------------------------------------------------------------------------
// Camera override and freeze plumbing
// ---------------------------------------------------------------------------

/// Scenario-only override of the main camera for views away from the player's
/// eye. Only the gallery (and offscreen review tests) insert it; while present
/// it replaces the first-person eye.
#[derive(Resource, Debug, Clone, Copy)]
pub struct GalleryCamera(pub Transform);

/// Runs after transform propagation (and before frusta and shadow cascades are
/// computed) so the override wins over the first-person camera follow. Every
/// entity under the camera (the viewmodel camera and the guns it draws) follows.
pub fn apply_camera_override(
    over: Option<Res<GalleryCamera>>,
    mut cameras: Query<(&mut Transform, &mut GlobalTransform, Option<&Children>), With<MainCamera>>,
    mut below: Query<(&Transform, &mut GlobalTransform, Option<&Children>), Without<MainCamera>>,
) {
    let Some(over) = over else {
        return;
    };
    for (mut transform, mut global, kids) in &mut cameras {
        *transform = over.0;
        *global = GlobalTransform::from(over.0);
        let mut stack: Vec<(Entity, GlobalTransform)> = kids
            .into_iter()
            .flat_map(|k| k.iter())
            .map(|kid| (kid, *global))
            .collect();
        while let Some((entity, parent)) = stack.pop() {
            if let Ok((local, mut entity_global, children)) = below.get_mut(entity) {
                *entity_global = parent.mul_transform(*local);
                let placed = *entity_global;
                stack.extend(
                    children
                        .into_iter()
                        .flat_map(|c| c.iter())
                        .map(|c| (c, placed)),
                );
            }
        }
    }
}

/// Puts the main camera at the override before the guns and the muzzle point
/// are placed from it (`ViewmodelSet`), so the viewmodel, the bolt's start and
/// the HUD's projections all agree with the gallery's camera.
pub fn place_gallery_camera(
    over: Option<Res<GalleryCamera>>,
    mut cameras: Query<&mut Transform, With<MainCamera>>,
) {
    let Some(over) = over else {
        return;
    };
    for mut transform in &mut cameras {
        *transform = over.0;
    }
}

/// The gallery's client hooks: the knight honors [`GalleryFreeze`]
/// (`knight::freeze_knights`; effects and the dummy honor it on their own), and
/// the [`GalleryCamera`] override is in place before the viewmodel follows the
/// camera. The scenario plugin adds it; offscreen tests add it themselves.
pub struct GalleryPlugin;

impl Plugin for GalleryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, crate::knight::freeze_knights)
            .add_systems(
                PostUpdate,
                place_gallery_camera
                    .after(CameraFollowSet)
                    .before(ViewmodelSet),
            );
    }
}

// ---------------------------------------------------------------------------
// The view table
// ---------------------------------------------------------------------------

/// Vertical field of view the views are composed for: the game's default. The
/// runner sets it, so a player's saved FOV can't reframe the gallery.
pub const GALLERY_FOV_DEG: f32 = 70.0;
/// Target image size, for [`target_screen`].
pub const TARGET_SIZE: Vec2 = Vec2::new(1672.0, 941.0);
/// The player spawn's eye (feet (2, 0, 14)), where the far view is composed from.
pub const SPAWN_EYE: Vec3 = Vec3::new(2.0, 1.62, 14.0);
/// Far-view landmarks as the sky slice lays them out (`far::layout`, composed
/// from the spawn eye as in T01): the station's platform (azimuth 30°, 640 m
/// out, 152 m up; front right), the ringed planet's centre (azimuth 2°, 12° up,
/// 800 m; low centre) and the galaxy core's direction (azimuth -21°, 28° up;
/// upper left). Views aim by them; the headless tests check where they land.
pub const STATION: Vec3 = Vec3::new(322.0, 152.0, -540.3);
pub const PLANET: Vec3 = Vec3::new(29.3, 167.9, -768.0);

/// Unit direction toward the galaxy core.
pub fn galaxy_dir() -> Vec3 {
    let (az, el) = (-21f32.to_radians(), 28f32.to_radians());
    Vec3::new(az.sin() * el.cos(), el.sin(), -az.cos() * el.cos())
}

/// Frames a view settles before its moment: the tool switch, piece pop-ins, the
/// knight's respawn pop and the previous view's effects are all done by then.
pub const SETTLE: u32 = 72;
/// Frame the views that shoot fire on.
pub const FIRE_AT: u32 = SETTLE;
/// Frames the freeze holds after the capture frame before the next view.
pub const HOLD: u32 = 4;
/// A view that never reaches its moment (no shot) captures anyway after this.
pub const VIEW_TIMEOUT: u32 = 360;

/// How the player aims: where the sim's look points, so where shots go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Aim {
    /// Fixed look angles (radians): yaw 0 faces -Z and turns left as it grows;
    /// pitch is positive upward.
    Look { yaw: f32, pitch: f32 },
    /// At a world point.
    At(Vec3),
    /// At the knight, `up` m above his feet and `right` m to his right as the
    /// player sees him. Tracks him while he runs.
    Knight { up: f32, right: f32 },
}

/// How the view is framed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Framing {
    /// The player's eye, looking where they aim: exactly what the game shows.
    Eye,
    /// The player's eye, turned so the aim point sits at `screen` instead of
    /// under the crosshair, for targets that paint the crosshair beside the
    /// knight. Shots still go where the player aims; the bolt flies from the gun
    /// to the knight as in the target.
    Offset { screen: Vec2 },
    /// The player's view pulled `back` m behind their eye, for targets that
    /// frame a build from further off than the player can target it.
    Behind { back: f32 },
    /// A fixed camera away from the player ([`GalleryCamera`]).
    Fixed { eye: Vec3, look_at: Vec3 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KnightMotion {
    /// Standing, turned to face the player (as the dummy does).
    Idle,
    /// Running along `azimuth` (degrees clockwise from -Z, seen from above),
    /// from `lead` m back, so he passes his spot about when the view fires.
    Run { azimuth: f32, lead: f32 },
}

/// Where the knight is and what state he's in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KnightSpot {
    /// His feet at the view's shot (or capture).
    pub feet: Vec3,
    pub motion: KnightMotion,
    pub hp: f32,
    pub shield: f32,
    /// Where the target shows his chest (1 m up), in half-heights. The
    /// composition test holds the captured frame to it.
    pub screen: Vec2,
}

/// A scripted action, `n` frames into the view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Act {
    /// Press and release the primary action (fire the gun).
    Fire,
    /// Switch tools.
    Select(ActiveTool),
    /// Pause the game (the pause menu opens) or resume it.
    Pause(bool),
}

/// When a view freezes and captures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Moment {
    /// This many frames after the view starts.
    Settled(u32),
    /// This many frames (≥ 1) after the frame the player's shot fired. The
    /// capture shows the effects as drawn `n - 1` frames after the shot frame:
    /// `AfterShot(2)` catches a rifle bolt halfway (it reaches its hit point
    /// within 2 frames), `AfterShot(28)` the poof and the hat half a second on.
    AfterShot(u32),
}

/// What the view must show happening, checked headless (`tests/gallery.rs`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Expect {
    /// No shot: a still view.
    Still,
    /// A body hit that neither breaks a shield nor kills.
    Hit,
    Headshot,
    ShieldBreak,
    Kill,
    /// The build ghost shows this slot, valid (blue).
    Ghost(PieceSlot),
    /// The game is paused with the menu open.
    Paused,
}

/// One target-board view.
#[derive(Debug, Clone, PartialEq)]
pub struct GalleryView {
    /// "T01".
    pub id: &'static str,
    /// File stem, shared with the target image: "T01-spawn-vista".
    pub name: &'static str,
    /// What the target shows (the board page's caption).
    pub title: &'static str,
    /// The player's feet.
    pub feet: Vec3,
    pub aim: Aim,
    pub framing: Framing,
    pub tool: ActiveTool,
    /// `None` parks him out of view, behind the player.
    pub knight: Option<KnightSpot>,
    /// Pieces built for this view, removed after it.
    pub pieces: Vec<PieceSlot>,
    /// Scripted actions, in frames after the view starts.
    pub script: Vec<(u32, Act)>,
    pub moment: Moment,
    pub expect: Expect,
}

/// A target pixel (1672 × 941 image) in half-heights from the centre.
pub fn target_screen(px: f32, py: f32) -> Vec2 {
    let half = TARGET_SIZE.y / 2.0;
    Vec2::new((px - TARGET_SIZE.x / 2.0) / half, (half - py) / half)
}

/// Unit direction along an azimuth (degrees clockwise from -Z, seen from above).
pub fn azimuth_dir(azimuth_deg: f32) -> Vec3 {
    let a = azimuth_deg.to_radians();
    Vec3::new(a.sin(), 0.0, -a.cos())
}

/// A point `distance` m from `from` along an azimuth, on the ground.
fn out(from: Vec3, azimuth_deg: f32, distance: f32) -> Vec3 {
    (from + azimuth_dir(azimuth_deg) * distance).with_y(0.0)
}

fn deg(d: f32) -> f32 {
    d.to_radians()
}

fn wall(x: i32, z: i32, level: i32, facing: Facing) -> PieceSlot {
    PieceSlot::wall(GridCell::new(x, z, level), facing)
}

fn ramp(x: i32, z: i32, level: i32, facing: Facing) -> PieceSlot {
    PieceSlot::ramp(GridCell::new(x, z, level), facing)
}

fn floor(x: i32, z: i32, level: i32) -> PieceSlot {
    PieceSlot::floor(GridCell::new(x, z, level))
}

const RIFLE: ActiveTool = ActiveTool::Weapon(WeaponKind::Rifle);
const PUMP: ActiveTool = ActiveTool::Weapon(WeaponKind::Pump);

/// The twelve views, T01 to T12, each matched to its target's composition.
pub fn views() -> Vec<GalleryView> {
    use Facing::*;
    let spawn = SPAWN_EYE.with_y(0.0);
    let chest = Aim::Knight {
        up: 1.0,
        right: 0.0,
    };
    let fire = vec![(FIRE_AT, Act::Fire)];

    // T03: across the arena toward the station (upper left), the knight 20 m
    // out running to the left and a little toward us.
    let t03_feet = Vec3::new(-14.0, 0.0, 14.0);
    let t03_knight = out(t03_feet, 36.0, 20.0);
    // T04: a built platform on the left, the knight 5 m out in front of it,
    // the station upper right.
    let t04_feet = Vec3::new(-1.0, 0.0, 18.5);
    let t04_knight = out(t04_feet, -21.0, 5.0);
    // T05: the knight 10 m out on open ground, a wall close on the right.
    let t05_feet = Vec3::new(4.0, 0.0, 19.0);
    let t05_knight = out(t05_feet, 6.0, 10.0);
    // T06: the knight dead ahead at 12 m, the station upper left.
    let t06_feet = Vec3::new(-10.0, 0.0, 12.0);
    let t06_knight = out(t06_feet, 52.0, 12.0);
    // T07: the knight 8 m out, a wall very close on the left.
    let t07_feet = Vec3::new(-2.0, 0.0, 10.0);
    let t07_knight = out(t07_feet, 12.0, 8.0);
    // T08: the knight 8 m out beside the south-east ramp, roofed and walled.
    let t08_feet = Vec3::new(19.0, 0.0, 16.0);
    let t08_knight = out(t08_feet, -12.0, 8.0);
    // T10: out over the void toward the station, low, looking up at it.
    let t10_eye = (SPAWN_EYE + azimuth_dir(30.0) * 250.0).with_y(15.0);

    vec![
        GalleryView {
            id: "T01",
            name: "T01-spawn-vista",
            title: "Spawn vista",
            feet: spawn,
            aim: Aim::Look {
                yaw: 0.0,
                pitch: deg(-5.0),
            },
            framing: Framing::Eye,
            tool: RIFLE,
            knight: Some(KnightSpot {
                feet: Vec3::new(2.0, 0.0, -6.0),
                motion: KnightMotion::Idle,
                hp: 100.0,
                shield: 100.0,
                screen: target_screen(835.0, 440.0),
            }),
            pieces: vec![],
            script: vec![],
            moment: Moment::Settled(SETTLE),
            expect: Expect::Still,
        },
        GalleryView {
            id: "T02",
            name: "T02-rifle-idle",
            title: "Rifle idle, gun tilted toward the viewer",
            feet: Vec3::new(0.0, 0.0, 4.3),
            aim: Aim::Look {
                yaw: 0.0,
                pitch: deg(-2.0),
            },
            framing: Framing::Eye,
            tool: RIFLE,
            knight: None,
            // The brick wall at the left, seen along its face.
            pieces: vec![wall(5, 5, 0, West)],
            script: vec![],
            moment: Moment::Settled(SETTLE),
            expect: Expect::Still,
        },
        GalleryView {
            id: "T03",
            name: "T03-rifle-bolt",
            title: "Rifle bolt in flight; the knight running at about 20 m",
            feet: t03_feet,
            aim: chest,
            framing: Framing::Offset {
                screen: target_screen(590.0, 490.0),
            },
            tool: RIFLE,
            knight: Some(KnightSpot {
                feet: t03_knight,
                motion: KnightMotion::Run {
                    azimuth: -60.0,
                    lead: 6.3,
                },
                hp: 100.0,
                shield: 0.0,
                screen: target_screen(590.0, 490.0),
            }),
            // A wall with a ramp against it at the left edge; a wall end and a
            // raised floor at the right.
            pieces: vec![
                wall(2, 7, 0, East),
                ramp(2, 7, 0, East),
                wall(4, 9, 0, South),
                wall(5, 9, 0, East),
                floor(5, 9, 1),
            ],
            script: fire.clone(),
            moment: Moment::AfterShot(2),
            expect: Expect::Hit,
        },
        GalleryView {
            id: "T04",
            name: "T04-pump-fan",
            title: "Pump spark fan; the knight at about 5 m",
            feet: t04_feet,
            // Some pellets catch his side; the rest fan past him.
            aim: Aim::Knight {
                up: 1.0,
                right: 0.4,
            },
            framing: Framing::Offset {
                screen: Vec2::new(-0.39, -0.105),
            },
            tool: PUMP,
            knight: Some(KnightSpot {
                feet: t04_knight,
                motion: KnightMotion::Idle,
                hp: 100.0,
                shield: 0.0,
                screen: target_screen(600.0, 520.0),
            }),
            // Two walls round a ramp, floored over: the platform on the left.
            pieces: vec![
                wall(4, 9, 0, West),
                wall(4, 9, 0, North),
                ramp(4, 9, 0, North),
                floor(4, 9, 1),
            ],
            script: fire.clone(),
            moment: Moment::AfterShot(2),
            expect: Expect::Hit,
        },
        GalleryView {
            id: "T05",
            name: "T05-knight-hit",
            title: "Knight body hit at about 10 m: wide eyes, damage number",
            feet: t05_feet,
            aim: chest,
            // The target paints him higher; a level-ish camera keeps its sky.
            framing: Framing::Offset {
                screen: Vec2::new(-0.26, 0.0),
            },
            tool: RIFLE,
            knight: Some(KnightSpot {
                feet: t05_knight,
                motion: KnightMotion::Idle,
                hp: 100.0,
                shield: 0.0,
                screen: target_screen(715.0, 430.0),
            }),
            // A wall with a ramp up to it, close on the right.
            pieces: vec![wall(8, 9, 0, South), ramp(8, 10, 0, North)],
            script: fire.clone(),
            moment: Moment::AfterShot(3),
            expect: Expect::Hit,
        },
        GalleryView {
            id: "T06",
            name: "T06-headshot",
            title: "Headshot at about 12 m: hat bouncing, gold number",
            feet: t06_feet,
            aim: Aim::Knight {
                up: 1.62,
                right: 0.0,
            },
            framing: Framing::Eye,
            tool: RIFLE,
            knight: Some(KnightSpot {
                feet: t06_knight,
                motion: KnightMotion::Idle,
                hp: 100.0,
                shield: 0.0,
                screen: target_screen(790.0, 500.0),
            }),
            // A ramp against a wall on the left, a wall end close on the right.
            pieces: vec![
                wall(3, 7, 0, North),
                ramp(3, 7, 0, North),
                wall(4, 9, 0, East),
            ],
            script: fire.clone(),
            moment: Moment::AfterShot(7),
            expect: Expect::Headshot,
        },
        GalleryView {
            id: "T07",
            name: "T07-shield-break",
            title: "Shield break at about 8 m, stars",
            feet: t07_feet,
            aim: chest,
            framing: Framing::Offset {
                screen: Vec2::new(-0.25, 0.0),
            },
            tool: RIFLE,
            knight: Some(KnightSpot {
                feet: t07_knight,
                motion: KnightMotion::Idle,
                hp: 100.0,
                shield: 10.0,
                screen: target_screen(720.0, 440.0),
            }),
            // A wall very close on the left.
            pieces: vec![wall(5, 7, 0, West)],
            script: fire.clone(),
            moment: Moment::AfterShot(9),
            expect: Expect::ShieldBreak,
        },
        GalleryView {
            id: "T08",
            name: "T08-elimination",
            title: "Elimination poof at about 8 m, hat spinning on the grass",
            feet: t08_feet,
            aim: chest,
            framing: Framing::Offset {
                screen: Vec2::new(-0.37, 0.05),
            },
            tool: RIFLE,
            knight: Some(KnightSpot {
                feet: t08_knight,
                motion: KnightMotion::Idle,
                hp: 20.0,
                shield: 0.0,
                screen: target_screen(660.0, 400.0),
            }),
            // The south-east ramp (initial cover), walled and roofed: the
            // structure on the left.
            pieces: vec![wall(9, 8, 0, West), floor(9, 8, 1)],
            script: fire,
            moment: Moment::AfterShot(28),
            expect: Expect::Kill,
        },
        GalleryView {
            id: "T09",
            name: "T09-fort",
            title: "A 1×1 fort (3 brick walls, a ramp inside, a floor on top) with a blue ghost wall beside it; wall slot selected",
            // Just short of a grid line, so the ghost goes on the next one: the
            // fort's front line, beside it.
            feet: Vec3::new(5.0, 0.0, 12.3),
            aim: Aim::Look {
                yaw: deg(3.0),
                pitch: deg(-4.0),
            },
            // The ghost can't go further than the next grid line, so the view
            // steps back a little to show the fort and the ghost side by side.
            framing: Framing::Behind { back: 1.2 },
            tool: ActiveTool::Build(PieceKind::Wall),
            knight: None,
            pieces: vec![
                wall(6, 7, 0, West),
                wall(6, 7, 0, North),
                wall(6, 7, 0, East),
                ramp(6, 7, 0, North),
                floor(6, 7, 1),
            ],
            script: vec![],
            moment: Moment::Settled(SETTLE),
            expect: Expect::Ghost(wall(7, 8, 0, North)),
        },
        GalleryView {
            id: "T10",
            name: "T10-station-up",
            title: "Looking up at the station",
            feet: spawn,
            aim: Aim::Look {
                yaw: deg(-30.0),
                pitch: deg(20.0),
            },
            framing: Framing::Fixed {
                eye: t10_eye,
                look_at: STATION + Vec3::Y * 45.0,
            },
            tool: RIFLE,
            knight: None,
            pieces: vec![],
            script: vec![],
            moment: Moment::Settled(SETTLE),
            expect: Expect::Still,
        },
        GalleryView {
            id: "T11",
            name: "T11-island-edge",
            title: "The island edge and barrier",
            // Beside the east barrier, looking north along the edge: the land
            // on the left, the east cover walls at the far left.
            feet: Vec3::new(22.5, 0.0, 14.0),
            aim: Aim::Look {
                yaw: deg(-15.0),
                pitch: deg(-8.0),
            },
            framing: Framing::Eye,
            tool: RIFLE,
            knight: None,
            pieces: vec![],
            script: vec![],
            moment: Moment::Settled(SETTLE),
            expect: Expect::Still,
        },
        GalleryView {
            id: "T12",
            name: "T12-pause-menu",
            title: "The pause menu",
            feet: spawn,
            aim: Aim::Look {
                yaw: 0.0,
                pitch: deg(4.0),
            },
            framing: Framing::Eye,
            tool: RIFLE,
            knight: None,
            pieces: vec![],
            script: vec![(24, Act::Pause(true))],
            moment: Moment::Settled(SETTLE),
            expect: Expect::Paused,
        },
    ]
}

// ---------------------------------------------------------------------------
// Screen geometry
// ---------------------------------------------------------------------------

/// Where `point` lands on screen for a camera at `camera` with vertical field of
/// view `fov` (radians), in half-heights from the centre (x right, y up).
/// `None` behind the camera.
pub fn screen_point(camera: &Transform, fov: f32, point: Vec3) -> Option<Vec2> {
    let local = camera.rotation.inverse() * (point - camera.translation);
    if local.z > -1e-4 {
        return None;
    }
    let t = (fov * 0.5).tan();
    Some(Vec2::new(local.x, local.y) / (-local.z * t))
}

/// A camera at `eye` with no roll, turned so `point` lands at `screen`.
pub fn framed_camera(eye: Vec3, point: Vec3, screen: Vec2, fov: f32) -> Transform {
    let d = (point - eye).normalize_or(Vec3::NEG_Z);
    // The camera-space direction that shows at `screen`.
    let t = (fov * 0.5).tan();
    let c = Vec3::new(screen.x * t, screen.y * t, -1.0).normalize();
    // Pitch first: the X rotation must give `c` the world direction's height.
    // (y cos p + z sin p... with z < 0): c.y cos p - c.z sin p = d.y.
    let (a, b) = (c.y, -c.z);
    let r = (a * a + b * b).sqrt().max(1e-6);
    let phi = b.atan2(a);
    let pitch = phi - (d.y / r).clamp(-1.0, 1.0).acos();
    let pitched = Quat::from_rotation_x(pitch) * c;
    // Then yaw, turning the pitched direction's heading onto the world one's.
    let heading = |v: Vec3| (-v.x).atan2(-v.z);
    let yaw = heading(d) - heading(pitched);
    Transform::from_translation(eye).with_rotation(Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0))
}

/// Where the knight parks for views without him: behind the player.
fn parking(feet: Vec3, facing: Vec3) -> Vec3 {
    let back = Vec3::new(-facing.x, 0.0, -facing.z).normalize_or(Vec3::Z);
    let spot = feet + back * 7.0;
    let limit = crate::shared::ARENA_HALF - 1.5;
    Vec3::new(
        spot.x.clamp(-limit, limit),
        0.0,
        spot.z.clamp(-limit, limit),
    )
}

// ---------------------------------------------------------------------------
// The runner
// ---------------------------------------------------------------------------

/// What happened in one view, for the summary and the headless tests.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ViewRecord {
    pub id: &'static str,
    pub name: &'static str,
    pub title: &'static str,
    pub shots: u32,
    pub hits: u32,
    pub headshots: u32,
    pub shield_breaks: u32,
    pub kills: u32,
    /// Pieces the view asked for that placement rejected.
    pub rejected: Vec<String>,
    /// View frame the freeze started on.
    pub moment_frame: Option<u32>,
    /// The moment never came (no shot); captured at the timeout instead.
    pub missed: bool,
    /// Where the knight's feet were at the moment, and where his chest (1 m
    /// up) showed, in half-heights (`None` behind the camera).
    pub knight_feet: Option<[f32; 3]>,
    pub knight_screen: Option<[f32; 2]>,
    /// The build ghost at the moment: the slot and whether it was valid.
    #[serde(skip)]
    pub ghost: Option<(PieceSlot, bool)>,
    pub paused: bool,
    /// The camera at the moment.
    #[serde(skip)]
    pub camera: Option<Transform>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Setup,
    Live,
    /// Frozen since this view frame.
    Frozen(u32),
}

/// Plays the views in order, one call to [`GalleryRunner::frame`] per rendered
/// frame. The host takes the captures it asks for ([`GalleryRunner::take_captures`]):
/// the native scenario screenshots the window, the offscreen test the world
/// image.
pub struct GalleryRunner {
    views: Vec<GalleryView>,
    lead_in: u32,
    frame: u32,
    index: usize,
    view_frame: u32,
    phase: Phase,
    next_act: usize,
    release_fire: bool,
    /// Runner frame the current view's shot fired on.
    shot_frame: Option<u32>,
    built: Vec<Entity>,
    captures: Vec<&'static str>,
    records: Vec<ViewRecord>,
    started: bool,
    shots: MessageCursor<ShotFired>,
    damage: MessageCursor<DamageDealt>,
}

impl GalleryRunner {
    /// `lead_in` frames pass before the first view (pipelines settle).
    pub fn new(views: Vec<GalleryView>, lead_in: u32) -> Self {
        Self {
            views,
            lead_in,
            frame: 0,
            index: 0,
            view_frame: 0,
            phase: Phase::Setup,
            next_act: 0,
            release_fire: false,
            shot_frame: None,
            built: Vec::new(),
            captures: Vec::new(),
            records: Vec::new(),
            started: false,
            shots: MessageCursor::default(),
            damage: MessageCursor::default(),
        }
    }

    pub fn views(&self) -> &[GalleryView] {
        &self.views
    }

    pub fn records(&self) -> &[ViewRecord] {
        &self.records
    }

    /// File stems to capture now, in the order asked.
    pub fn take_captures(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.captures)
    }

    pub fn is_done(&self) -> bool {
        self.index >= self.views.len()
    }

    /// Advances one rendered frame. Runs before the fixed step (the scenario's
    /// `PreUpdate`). Returns false once every view is captured.
    pub fn frame(&mut self, world: &mut World) -> bool {
        self.frame += 1;
        if std::mem::take(&mut self.release_fire) {
            with_intent(world, |i| i.fire = false);
        }
        self.watch(world);
        if self.frame <= self.lead_in {
            return true;
        }
        if !self.started {
            self.started = true;
            self.start(world);
        }
        let Some(view) = self.views.get(self.index).cloned() else {
            return false;
        };
        match self.phase {
            Phase::Setup => {
                self.setup(world, &view);
                self.phase = Phase::Live;
            }
            Phase::Live => {
                self.view_frame += 1;
                self.run_script(world, &view);
                self.drive(world, &view);
                let reached = match view.moment {
                    Moment::Settled(n) => self.view_frame >= n,
                    Moment::AfterShot(n) => self.shot_frame.is_some_and(|f| self.frame >= f + n),
                };
                if reached || self.view_frame >= VIEW_TIMEOUT {
                    world.insert_resource(GalleryFreeze);
                    self.record_moment(world, !reached);
                    self.phase = Phase::Frozen(self.view_frame);
                }
            }
            Phase::Frozen(at) => {
                self.view_frame += 1;
                if self.view_frame == at + 1 {
                    self.captures.push(view.name);
                }
                if self.view_frame > at + 1 + HOLD {
                    self.teardown(world);
                    self.index += 1;
                    self.phase = Phase::Setup;
                }
            }
        }
        true
    }

    /// Once, before the first view: the gallery's FOV, and the dummy becomes a
    /// puppet (its strafing brain comes off) so each view places and poses it.
    fn start(&mut self, world: &mut World) {
        {
            let mut tuning = world.resource_mut::<Tuning>();
            tuning.look.fov_deg = GALLERY_FOV_DEG;
            tuning.hud.damage_numbers = true;
            tuning.hud.perf_overlay = false;
            tuning.dummy.stand_still = true;
            // Shake would only tilt the frame; the freeze holds everything else.
            tuning.feedback.camera_shake = 0.0;
        }
        let dummies: Vec<Entity> = world
            .query_filtered::<Entity, With<Dummy>>()
            .iter(world)
            .collect();
        if dummies.is_empty() {
            let feet = crate::arena::ArenaLayout::default().dummy_spawn;
            let mut commands = world.commands();
            spawn_character(
                &mut commands,
                feet,
                LookAngles::default(),
                Health::default(),
                Dummy,
            );
            world.flush();
        }
        for dummy in dummies {
            world.entity_mut(dummy).remove::<DummyBrain>();
        }
    }

    fn record(&mut self) -> Option<&mut ViewRecord> {
        self.records.get_mut(self.index)
    }

    /// Counts the current view's shots and what they did.
    fn watch(&mut self, world: &mut World) {
        let player = player_entity(world);
        let dummy = dummy_entity(world);
        let fired = self
            .shots
            .read(world.resource::<Messages<ShotFired>>())
            .filter(|s| Some(s.shooter) == player)
            .count() as u32;
        let hits: Vec<DamageDealt> = self
            .damage
            .read(world.resource::<Messages<DamageDealt>>())
            .filter(|d| d.target_kind == DamageTarget::Character && Some(d.target) == dummy)
            .cloned()
            .collect();
        if self.phase == Phase::Setup || self.records.len() <= self.index {
            return;
        }
        if fired > 0 && self.shot_frame.is_none() {
            // Messages from a fixed tick are read on the next frame.
            self.shot_frame = Some(self.frame - 1);
        }
        let Some(record) = self.record() else {
            return;
        };
        record.shots += fired;
        for hit in hits {
            record.hits += 1;
            record.headshots += u32::from(hit.headshot);
            record.shield_breaks += u32::from(hit.shield_broke);
            record.kills += u32::from(hit.killed);
        }
    }

    fn setup(&mut self, world: &mut World, view: &GalleryView) {
        world.remove_resource::<GalleryFreeze>();
        self.view_frame = 0;
        self.next_act = 0;
        self.shot_frame = None;
        self.records.push(ViewRecord {
            id: view.id,
            name: view.name,
            title: view.title,
            ..default()
        });
        with_intent(world, |i| {
            *i = PlayerIntent {
                select: Some(view.tool),
                ..default()
            };
        });
        teleport(world, view.feet);
        self.place_knight(world, view);
        for slot in &view.pieces {
            match place_piece(world, *slot) {
                Ok(piece) => self.built.push(piece),
                Err(why) => {
                    let text = format!("{slot:?}: {why:?}");
                    if let Some(record) = self.record() {
                        record.rejected.push(text);
                    }
                }
            }
        }
        match view.framing {
            Framing::Fixed { eye, look_at } => world.insert_resource(GalleryCamera(
                Transform::from_translation(eye).looking_at(look_at, Vec3::Y),
            )),
            _ => {
                world.remove_resource::<GalleryCamera>();
            }
        }
        self.drive(world, view);
    }

    /// Puts the knight on his spot (or out of view), alive, with the view's
    /// health and shield.
    fn place_knight(&mut self, world: &mut World, view: &GalleryView) {
        let Some(dummy) = dummy_entity(world) else {
            return;
        };
        let (feet, hp, shield) = match view.knight {
            Some(spot) => {
                let start = match spot.motion {
                    KnightMotion::Idle => spot.feet,
                    KnightMotion::Run { azimuth, lead } => spot.feet - azimuth_dir(azimuth) * lead,
                };
                (start, spot.hp, spot.shield)
            }
            None => {
                let facing = match view.aim {
                    Aim::Look { yaw, pitch } => LookAngles { yaw, pitch }.forward(),
                    _ => Vec3::NEG_Z,
                };
                (parking(view.feet, facing), 100.0, 100.0)
            }
        };
        let mut entity = world.entity_mut(dummy);
        entity.remove::<Downed>();
        if let Some(mut health) = entity.get_mut::<Health>() {
            health.hp = hp.min(health.max_hp);
            health.shield = shield.min(health.max_shield);
        }
        if let Some(mut intent) = entity.get_mut::<PlayerIntent>() {
            *intent = PlayerIntent::default();
        }
        if let Some(mut t) = entity.get_mut::<Transform>() {
            t.translation = feet;
        }
        if let Some(mut p) = entity.get_mut::<PreviousFeet>() {
            p.0 = feet;
        }
    }

    fn run_script(&mut self, world: &mut World, view: &GalleryView) {
        while let Some(&(at, act)) = view.script.get(self.next_act) {
            if at > self.view_frame {
                break;
            }
            self.next_act += 1;
            match act {
                Act::Fire => {
                    with_intent(world, |i| {
                        i.fire = true;
                        i.fire_pressed = true;
                    });
                    self.release_fire = true;
                    // One press per frame.
                    break;
                }
                Act::Select(tool) => with_intent(world, |i| i.select = Some(tool)),
                Act::Pause(paused) => set_paused(world, paused),
            }
        }
    }

    /// Every live frame: the player's aim, the knight puppet and the camera.
    fn drive(&mut self, world: &mut World, view: &GalleryView) {
        let knight = dummy_entity(world);
        let knight_feet = knight.and_then(|k| world.get::<Transform>(k).map(|t| t.translation));
        let Some(eye) = player_eye(world) else {
            return;
        };
        // The knight faces the player, or runs.
        if let (Some(knight), Some(feet)) = (knight, knight_feet) {
            let (look, axis) = match view.knight.map(|k| k.motion) {
                Some(KnightMotion::Run { azimuth, .. }) => (
                    LookAngles {
                        yaw: deg(-azimuth).rem_euclid(std::f32::consts::TAU),
                        pitch: 0.0,
                    },
                    Vec2::Y,
                ),
                _ => {
                    let knight_eye = feet + Vec3::Y * EyeHeight::default().0;
                    (look_toward(eye - knight_eye), Vec2::ZERO)
                }
            };
            let mut entity = world.entity_mut(knight);
            if let Some(mut l) = entity.get_mut::<LookAngles>() {
                l.set_if_neq(look);
            }
            if let Some(mut intent) = entity.get_mut::<PlayerIntent>() {
                intent.move_axis = axis;
            }
        }
        // The player's aim.
        let aim_point = match view.aim {
            Aim::Look { .. } => None,
            Aim::At(point) => Some(point),
            Aim::Knight { up, right } => knight_feet.map(|feet| {
                let across = (feet - eye).with_y(0.0).normalize_or(Vec3::NEG_Z);
                let side = across.cross(Vec3::Y);
                feet + Vec3::Y * up + side * right
            }),
        };
        let look = match (view.aim, aim_point) {
            (Aim::Look { yaw, pitch }, _) => LookAngles { yaw, pitch },
            (_, Some(point)) => look_toward(point - eye),
            _ => return,
        };
        if let Some(player) = player_entity(world)
            && let Some(mut l) = world.get_mut::<LookAngles>(player)
        {
            l.set_if_neq(look);
        }
        // The camera.
        match view.framing {
            Framing::Offset { screen } => {
                let point = aim_point.unwrap_or(eye + look.forward());
                let fov = world.resource::<Tuning>().look.fov_deg.to_radians();
                world.insert_resource(GalleryCamera(framed_camera(eye, point, screen, fov)));
            }
            Framing::Behind { back } => world.insert_resource(GalleryCamera(
                Transform::from_translation(eye - look.forward() * back)
                    .with_rotation(look.rotation()),
            )),
            Framing::Eye | Framing::Fixed { .. } => {}
        }
    }

    fn record_moment(&mut self, world: &mut World, missed: bool) {
        let frame = self.view_frame;
        let camera = view_camera(world);
        let fov = world.resource::<Tuning>().look.fov_deg.to_radians();
        let feet = dummy_entity(world)
            .and_then(|k| world.get::<Transform>(k))
            .map(|t| t.translation);
        let knight = feet
            .zip(camera)
            .and_then(|(feet, camera)| screen_point(&camera, fov, feet + Vec3::Y));
        let ghost = player_entity(world)
            .and_then(|p| world.get::<BuildTarget>(p))
            .and_then(|t| t.candidate)
            .map(|c| (c.slot, c.is_valid()));
        let paused = *world.resource::<State<AppState>>().get() == AppState::Paused;
        if let Some(record) = self.record() {
            record.moment_frame = Some(frame);
            record.missed = missed;
            record.knight_feet = feet.map(|f| f.to_array());
            record.knight_screen = knight.map(|s| [s.x, s.y]);
            record.ghost = ghost;
            record.paused = paused;
            record.camera = camera;
        }
    }

    /// After the capture: thaw, resume, and take the view's pieces down.
    fn teardown(&mut self, world: &mut World) {
        world.remove_resource::<GalleryFreeze>();
        world.remove_resource::<GalleryCamera>();
        if *world.resource::<State<AppState>>().get() == AppState::Paused {
            set_paused(world, false);
        }
        with_intent(world, |i| i.fire = false);
        for piece in self.built.drain(..) {
            let Ok(entity) = world.get_entity(piece) else {
                continue;
            };
            if let Some(slot) = entity.get::<Piece>().map(Piece::slot) {
                world.resource_mut::<PieceMap>().remove(slot.key(), piece);
            }
            world.despawn(piece);
        }
    }

    /// The scenario summary: one entry per view.
    pub fn summary(&self) -> Value {
        let views: Vec<Value> = self
            .records
            .iter()
            .map(|r| {
                let mut v = serde_json::to_value(r).unwrap_or(Value::Null);
                if let Value::Object(map) = &mut v {
                    map.insert("file".into(), json!(format!("{}.png", r.name)));
                    map.insert("grey".into(), json!(format!("{}-grey.png", r.name)));
                }
                v
            })
            .collect();
        json!({
            "views": views,
            "captured": self.records.iter().filter(|r| r.moment_frame.is_some()).count(),
            "missed": self.records.iter().filter(|r| r.missed).map(|r| r.id).collect::<Vec<_>>(),
        })
    }
}

fn set_paused(world: &mut World, paused: bool) {
    world.resource_mut::<NextState<AppState>>().set(if paused {
        AppState::Paused
    } else {
        AppState::Playing
    });
    if let Some(mut menu) = world.get_resource_mut::<MenuState>() {
        menu.menu_open = paused;
        menu.page = MenuPage::Main;
    }
}

fn dummy_entity(world: &mut World) -> Option<Entity> {
    world
        .query_filtered::<Entity, With<Dummy>>()
        .iter(world)
        .next()
}

fn player_eye(world: &mut World) -> Option<Vec3> {
    world
        .query_filtered::<(&Transform, Option<&EyeHeight>), With<Player>>()
        .iter(world)
        .next()
        .map(|(t, eye)| t.translation + Vec3::Y * eye.map_or(EyeHeight::default().0, |e| e.0))
}

/// The camera the view renders from: the override, or the player's eye.
pub fn view_camera(world: &mut World) -> Option<Transform> {
    if let Some(over) = world.get_resource::<GalleryCamera>() {
        return Some(over.0);
    }
    let eye = player_eye(world)?;
    let look = world
        .query_filtered::<&LookAngles, With<Player>>()
        .iter(world)
        .next()
        .copied()?;
    Some(Transform::from_translation(eye).with_rotation(look.rotation()))
}

// ---------------------------------------------------------------------------
// The native scenario
// ---------------------------------------------------------------------------

/// Frames before the first view, so pipelines and models settle.
const NATIVE_LEAD_IN: u32 = 150;

struct Gallery {
    runner: GalleryRunner,
}

impl Gallery {
    fn new() -> Self {
        Self {
            runner: GalleryRunner::new(views(), NATIVE_LEAD_IN),
        }
    }
}

impl Director for Gallery {
    fn update(&mut self, world: &mut World, _clock: &ScenarioClock) -> DirectorStatus {
        let running = self.runner.frame(world);
        for name in self.runner.take_captures() {
            capture(world, name, true);
        }
        if running {
            DirectorStatus::Running
        } else {
            DirectorStatus::Done
        }
    }

    fn warmup_seconds(&self) -> f64 {
        NATIVE_LEAD_IN as f64 / 60.0
    }

    fn runs_while_paused(&self) -> bool {
        true
    }

    fn summary(&mut self, _world: &mut World) -> Value {
        self.runner.summary()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn framed_camera_puts_the_point_where_asked() {
        let fov = GALLERY_FOV_DEG.to_radians();
        let eye = Vec3::new(1.0, 1.62, 4.0);
        for (point, screen) in [
            (Vec3::new(-3.0, 1.0, -6.0), Vec2::new(-0.3, 0.1)),
            (Vec3::new(9.0, 0.2, -2.0), Vec2::new(0.6, -0.4)),
            (Vec3::new(1.0, 40.0, -30.0), Vec2::new(0.0, 0.2)),
        ] {
            let camera = framed_camera(eye, point, screen, fov);
            let got = screen_point(&camera, fov, point).unwrap();
            assert!(got.distance(screen) < 1e-3, "{got} != {screen}");
            // No roll: the camera's right vector stays level.
            assert!((camera.rotation * Vec3::X).y.abs() < 1e-4);
        }
    }

    #[test]
    fn target_pixels_convert_to_half_heights() {
        assert_eq!(target_screen(836.0, 470.5), Vec2::ZERO);
        assert_eq!(target_screen(836.0, 0.0), Vec2::new(0.0, 1.0));
        let right = target_screen(1672.0, 941.0);
        assert!((right.x - 1672.0 / 941.0).abs() < 1e-5 && right.y == -1.0);
    }
}
