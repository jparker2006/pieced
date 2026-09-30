//! The knights' wand and orb sounds (docs/M3-SPEC.md → The orb, Fairness):
//!
//! - a flame "fwoom" at the wand tip when a knight's orb leaves it
//!   ([`Sfx::OrbCast`], spatial);
//! - a doppler whoosh when an orb flies past within [`WHOOSH_RADIUS`] of the
//!   player's head without hitting him ([`Sfx::OrbWhoosh`], at the point of
//!   closest approach);
//! - a crunchy bonk when one hits the player ([`Sfx::OrbBonk`], his own);
//! - **the off-screen warning** (D76): when a knight outside the player's view
//!   ([`in_view`]) starts a wind-up, a short rising charge plays from his
//!   direction at the start of the 0.4 s wind-up ([`Sfx::WandWarning`]). It is
//!   placed [`WARNING_DISTANCE`] from the listener along the knight's bearing,
//!   so it is always loud enough to hear, and still tells you where to turn.
//!   [`WandWarnings`] counts wind-ups and warnings for the fairness gate.
//!
//! An orb chipping a piece knocks it quietly in its material, like the
//! player's own shots.

use super::{PlayQueue, Sfx, play_queued};
use crate::{
    orb::{Orb, OrbHit, OrbImpact, Wand, wand_tip},
    render::{CurrentFov, MainCamera},
    shared::{EyeHeight, GameCue, LookAngles, PieceChange, Player},
};
use bevy::{platform::collections::HashMap, prelude::*, window::PrimaryWindow};

/// An orb passing this close to the player's head whooshes (m).
pub const WHOOSH_RADIUS: f32 = 3.0;
/// Closer than this it is a hit, and bonks instead (m).
pub const WHOOSH_MIN: f32 = 0.55;
/// The whoosh starts this long before the orb's closest approach (s), so the
/// swell peaks as it passes.
pub const WHOOSH_LEAD: f32 = 0.2;
/// The off-screen warning plays this far from the listener, along the
/// knight's bearing (m): close enough to be loud, far enough to be placed.
pub const WARNING_DISTANCE: f32 = 4.0;
/// A knight counts as on screen only well inside the frame: this fraction of
/// the half-FOV in each direction (at the very edge he is easy to miss).
pub const VIEW_MARGIN: f32 = 0.9;
/// Where on a knight the view check looks (his chest, above his feet).
pub const KNIGHT_CHEST: f32 = 1.2;

/// Whether `point` shows on screen for a camera at `eye` turned by
/// `rotation` (looking along its -Z) with vertical FOV `vfov` (radians) and
/// `aspect` (width / height), within `margin` of each half-FOV.
pub fn in_view(
    eye: Vec3,
    rotation: Quat,
    vfov: f32,
    aspect: f32,
    point: Vec3,
    margin: f32,
) -> bool {
    let v = rotation.inverse() * (point - eye);
    let depth = -v.z;
    if depth <= 0.1 {
        return false;
    }
    let tan_v = (vfov * 0.5).tan() * margin;
    let tan_h = (vfov * 0.5).tan() * aspect * margin;
    v.x.abs() / depth <= tan_h && v.y.abs() / depth <= tan_v
}

/// When an orb at `pos` moving at `velocity` passes closest to `ear`: the
/// time from now (s) and the miss distance (m).
pub fn closest_approach(pos: Vec3, velocity: Vec3, ear: Vec3) -> (f32, f32) {
    let v2 = velocity.length_squared();
    if v2 < 1e-6 {
        return (0.0, pos.distance(ear));
    }
    let t = -(pos - ear).dot(velocity) / v2;
    (t, (pos + velocity * t).distance(ear))
}

/// Wind-ups heard, for the fairness gate: every off-screen one must warn.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WandWarnings {
    pub windups: u32,
    pub offscreen: u32,
    pub warned: u32,
}

/// The knights whose off-screen wind-up warned since the log was last
/// cleared, in order. Only the headless fairness tests add it
/// ([`WandWarningTracking`]), to match each warning to its wind-up; the game
/// never does.
#[derive(Resource, Debug, Default, Clone)]
pub struct WandWarningLog(pub Vec<Entity>);

/// The wand sounds' decisions without the sound: for headless tests, which
/// have no sound bank or audio output (the queued sounds are dropped).
pub struct WandWarningTracking;

impl WandWarningTracking {
    /// Adds the wand-sound logic, [`WandWarnings`] and a [`WandWarningLog`] to
    /// a headless app (a test's simulation). Not for an app with
    /// `GameAudioPlugin`, which already runs it.
    pub fn install(app: &mut App) {
        app.init_resource::<PlayQueue>()
            .init_resource::<WandWarnings>()
            .init_resource::<WandWarningLog>()
            .add_systems(Update, (queue_wand_sounds, play_queued).chain());
    }
}

pub(super) fn build(app: &mut App) {
    app.init_resource::<WandWarnings>()
        .add_systems(Update, queue_wand_sounds.before(play_queued));
}

/// The listener: the camera as it rendered last, else the player's eye.
struct View {
    eye: Vec3,
    rotation: Quat,
    vfov: f32,
    aspect: f32,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn queue_wand_sounds(
    time: Res<Time<Real>>,
    fov: Option<Res<CurrentFov>>,
    camera: Option<Single<&GlobalTransform, With<MainCamera>>>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    player: Option<Single<(Entity, &Transform, &LookAngles, Option<&EyeHeight>), With<Player>>>,
    knights: Query<(&Transform, &LookAngles), With<Wand>>,
    orbs: Query<(Entity, &Orb, &Transform)>,
    mut cues: MessageReader<GameCue>,
    mut impacts: MessageReader<OrbImpact>,
    mut queue: ResMut<PlayQueue>,
    mut warnings: ResMut<WandWarnings>,
    mut log: Option<ResMut<WandWarningLog>>,
    mut whooshed: Local<HashMap<Entity, u64>>,
) {
    let now = time.elapsed_secs_f64();
    let Some(player) = player else {
        cues.clear();
        impacts.clear();
        return;
    };
    let (me, feet, look, eye) = *player;
    let player_eye = feet.translation + Vec3::Y * eye.map_or(EyeHeight::default().0, |e| e.0);
    let aspect = window.map_or(1.6, |w| w.width() / w.height().max(1.0));
    let vfov = fov.map_or(70f32.to_radians(), |f| f.0);
    let view = match camera {
        Some(c) => View {
            eye: c.translation(),
            rotation: c.rotation(),
            vfov,
            aspect,
        },
        None => View {
            eye: player_eye,
            rotation: look.rotation(),
            vfov,
            aspect,
        },
    };

    for cue in cues.read() {
        match *cue {
            GameCue::WandWindup { who } if who != me => {
                let Ok((knight, _)) = knights.get(who) else {
                    continue;
                };
                warnings.windups += 1;
                let chest = knight.translation + Vec3::Y * KNIGHT_CHEST;
                if in_view(
                    view.eye,
                    view.rotation,
                    view.vfov,
                    view.aspect,
                    chest,
                    VIEW_MARGIN,
                ) {
                    continue;
                }
                warnings.offscreen += 1;
                let bearing = (chest - view.eye).normalize_or(Vec3::NEG_Z);
                let at = view.eye + bearing * WARNING_DISTANCE.min(chest.distance(view.eye));
                queue.push(Sfx::WandWarning, Some(at), now);
                warnings.warned += 1;
                if let Some(log) = log.as_mut() {
                    log.0.push(who);
                }
            }
            GameCue::OrbFired { who } if who != me => {
                if let Ok((knight, look)) = knights.get(who) {
                    queue.push(
                        Sfx::OrbCast,
                        Some(wand_tip(knight.translation, look.yaw)),
                        now,
                    );
                }
            }
            _ => {}
        }
    }

    for impact in impacts.read() {
        match impact.hit {
            OrbHit::Player { .. } if impact.target == Some(me) => {
                queue.push(Sfx::OrbBonk, None, now);
            }
            OrbHit::Piece(kind) => {
                let knock = Sfx::for_piece(kind, PieceChange::Placed);
                queue.push_with(knock, Some(impact.point), 0.35, 0.0, now);
            }
            _ => {}
        }
    }

    // Whooshes: an orb about to fly past the player's head.
    for (entity, orb, transform) in &orbs {
        if orb.shooter == me {
            continue;
        }
        if whooshed.get(&entity) == Some(&orb.launched) {
            continue;
        }
        let (t, miss) = closest_approach(transform.translation, orb.velocity, player_eye);
        if (0.0..=WHOOSH_LEAD).contains(&t) && (WHOOSH_MIN..=WHOOSH_RADIUS).contains(&miss) {
            whooshed.insert(entity, orb.launched);
            let at = transform.translation + orb.velocity * t;
            queue.push(Sfx::OrbWhoosh, Some(at), now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closest_approach_finds_the_pass() {
        let (t, miss) = closest_approach(Vec3::new(-30.0, 2.0, 0.0), Vec3::X * 30.0, Vec3::ZERO);
        assert!((t - 1.0).abs() < 1e-5 && (miss - 2.0).abs() < 1e-5);
        let (t, _) = closest_approach(Vec3::new(5.0, 0.0, 0.0), Vec3::X * 30.0, Vec3::ZERO);
        assert!(t < 0.0, "already past");
    }
}
