//! The camera's feel (M4 chunk 5, D108): subtle and trackpad-safe. Every
//! effect is a field-of-view change, a roll or a render-only offset of the
//! drawn camera, applied after it has followed the eye; **the aim never
//! moves** (shots and building trace from [`LookAngles`] and the eye in the
//! fixed step, which nothing here touches). No head bob.
//!
//! - **FOV kick:** up to +[`CameraFeelTuning::fov_kick_deg`] (4°) on a slide
//!   and on a landing from [`CameraFeelTuning::kick_fall_height`] (3 m) or
//!   more, over 250 ms.
//! - **Landing dip:** up to 6 cm, render-only, scaled by the fall.
//! - **Slide tilt:** 2.5° of roll while sliding.
//! - **Damage nudge:** up to 0.5° of roll toward where a hit came from.
//!
//! Everything scales with the **Camera effects** slider
//! (`tuning.feedback.camera_effects`, 0–100%, in Settings; changing it plays
//! a nudge as a preview, [`CameraNudgePreview`]). The death cam
//! (`waves::ui::death`) takes over after an elimination.
//!
//! **Cost.** A few floats per frame and one camera transform and projection
//! write: CPU only, nothing drawn, nothing allocated.

use crate::{
    movement::Motor,
    render::{CameraFollowSet, CurrentFov, MainCamera},
    shared::{DamageDealt, DamageTarget, GameCue, LookAngles, Player},
    tuning::Tuning,
    viewmodel::ViewmodelSet,
    waves::{Run, RunEnd, ui::death::DeathCamSet},
};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Designer numbers for the camera's feel (never persisted; the menu's
/// Camera effects slider is `feedback.camera_effects`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CameraFeelTuning {
    /// FOV kick: its peak (degrees, ≤ 4), rise and total length (s).
    pub fov_kick_deg: f32,
    pub fov_kick_rise: f32,
    pub fov_kick_seconds: f32,
    /// Landings from at least this high kick the FOV (m).
    pub kick_fall_height: f32,
    /// Landing dip: its most (m, ≤ 0.06), the fall that reaches it (m), and
    /// its length (s).
    pub dip_max: f32,
    pub dip_full_height: f32,
    pub dip_seconds: f32,
    /// Slide tilt (degrees of roll, 2–3) and how fast it eases (1/s).
    pub slide_roll_deg: f32,
    pub slide_roll_rate: f32,
    /// Damage nudge: its most (degrees of roll, ≤ 0.5) and length (s).
    pub nudge_deg: f32,
    pub nudge_seconds: f32,
}

impl Default for CameraFeelTuning {
    fn default() -> Self {
        Self {
            fov_kick_deg: 4.0,
            fov_kick_rise: 0.06,
            fov_kick_seconds: 0.25,
            kick_fall_height: 3.0,
            dip_max: 0.06,
            dip_full_height: 3.0,
            dip_seconds: 0.22,
            slide_roll_deg: 2.5,
            slide_roll_rate: 10.0,
            nudge_deg: 0.5,
            nudge_seconds: 0.3,
        }
    }
}

/// Hard limits, whatever the tuning says (D108).
pub const FOV_KICK_MAX_DEG: f32 = 4.0;
pub const DIP_MAX: f32 = 0.06;
pub const SLIDE_ROLL_MAX_DEG: f32 = 3.0;
pub const NUDGE_MAX_DEG: f32 = 0.5;

/// Plays a damage nudge now (the Settings page's preview of the Camera
/// effects slider).
#[derive(Message, Debug, Clone, Copy, Default, PartialEq)]
pub struct CameraNudgePreview;

/// The camera feel's state, and what it applied this frame (for tests).
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct CameraFeel {
    kick_age: f32,
    dip_age: f32,
    dip_depth: f32,
    nudge_age: f32,
    /// +1 rolls clockwise (toward a hit on the right), -1 the other way.
    nudge_side: f32,
    slide_roll: f32,
    /// The FOV before the kick when there is no [`CurrentFov`] (tests).
    base_fov: Option<f32>,
    kicked: bool,
    /// This frame's offsets: FOV (radians), dip (m), roll (radians).
    pub fov: f32,
    pub dip: f32,
    pub roll: f32,
}

impl Default for CameraFeel {
    fn default() -> Self {
        Self {
            kick_age: f32::MAX,
            dip_age: f32::MAX,
            dip_depth: 0.0,
            nudge_age: f32::MAX,
            nudge_side: 1.0,
            slide_roll: 0.0,
            base_fov: None,
            kicked: false,
            fov: 0.0,
            dip: 0.0,
            roll: 0.0,
        }
    }
}

/// A pulse's envelope `age` seconds in: up over `rise`, back down by
/// `length` (0 outside).
pub fn pulse(age: f32, rise: f32, length: f32) -> f32 {
    if !(0.0..length).contains(&age) {
        return 0.0;
    }
    if age < rise {
        let x = age / rise.max(1e-4);
        x * x * (3.0 - 2.0 * x)
    } else {
        let x = (age - rise) / (length - rise).max(1e-4);
        let y = 1.0 - x;
        y * y * (3.0 - 2.0 * y)
    }
}

/// The height a landing at `speed` (m/s, downward) fell from under
/// `gravity`.
pub fn fall_height(speed: f32, gravity: f32) -> f32 {
    speed * speed / (2.0 * gravity.max(1e-3))
}

pub struct CameraFeelPlugin;

impl Plugin for CameraFeelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraFeel>()
            .add_message::<CameraNudgePreview>()
            .add_message::<GameCue>()
            .add_message::<DamageDealt>()
            .add_systems(Update, read_camera_events)
            .add_systems(
                PostUpdate,
                apply_camera_feel
                    .after(CameraFollowSet)
                    .before(ViewmodelSet)
                    .before(DeathCamSet)
                    .before(TransformSystems::Propagate),
            );
    }
}

/// Starts kicks, dips and nudges from the player's slides, landings and
/// hits.
fn read_camera_events(
    tuning: Res<Tuning>,
    mut cues: MessageReader<GameCue>,
    mut hits: MessageReader<DamageDealt>,
    mut previews: MessageReader<CameraNudgePreview>,
    player: Option<Single<(Entity, &Transform, &LookAngles), With<Player>>>,
    sources: Query<&Transform, Without<Player>>,
    mut feel: ResMut<CameraFeel>,
) {
    let t = &tuning.camera;
    let me = player.as_ref().map(|p| p.0);
    for cue in cues.read() {
        match *cue {
            GameCue::SlideStart { who } if Some(who) == me => feel.kick_age = 0.0,
            GameCue::Land { who, speed } if Some(who) == me => {
                let height = fall_height(speed, tuning.movement.gravity);
                if height >= t.kick_fall_height {
                    feel.kick_age = 0.0;
                }
                let depth = (height / t.dip_full_height.max(0.1)).min(1.0) * t.dip_max;
                if depth > 0.005 {
                    feel.dip_age = 0.0;
                    feel.dip_depth = depth;
                }
            }
            _ => {}
        }
    }
    for hit in hits.read() {
        if Some(hit.target) != me || hit.target_kind != DamageTarget::Character || hit.amount <= 0.0
        {
            continue;
        }
        let Some((_, body, look)) = player.as_deref() else {
            continue;
        };
        let from = hit
            .source
            .and_then(|s| sources.get(s).ok())
            .map_or(hit.point, |t| t.translation);
        let right = look.rotation() * Vec3::X;
        let side = (from - body.translation).dot(right);
        feel.nudge_side = if side >= 0.0 { 1.0 } else { -1.0 };
        feel.nudge_age = 0.0;
    }
    for _ in previews.read() {
        feel.nudge_side = -feel.nudge_side;
        feel.nudge_age = 0.0;
    }
}

/// Applies the kick, dip, tilt and nudge to the drawn camera (see the
/// module docs); never to the look.
#[allow(clippy::type_complexity)]
fn apply_camera_feel(
    time: Res<Time<Real>>,
    tuning: Res<Tuning>,
    run: Option<Res<Run>>,
    current: Option<Res<CurrentFov>>,
    player: Option<Single<&Motor, With<Player>>>,
    mut feel: ResMut<CameraFeel>,
    mut camera: Option<Single<(&mut Transform, Option<&mut Projection>), With<MainCamera>>>,
) {
    let dt = time.delta_secs();
    let t = &tuning.camera;
    let amount = tuning.feedback.camera_effects.clamp(0.0, 1.0);
    let feel = &mut *feel;
    for age in [&mut feel.kick_age, &mut feel.dip_age, &mut feel.nudge_age] {
        if *age < 1.0e6 {
            *age += dt;
        }
    }
    let sliding = player.is_some_and(|m| m.sliding);
    let target = if sliding {
        t.slide_roll_deg.min(SLIDE_ROLL_MAX_DEG).to_radians()
    } else {
        0.0
    };
    let blend = 1.0 - (-t.slide_roll_rate * dt).exp();
    feel.slide_roll += (target - feel.slide_roll) * blend;
    if feel.slide_roll.abs() < 1e-5 && target == 0.0 {
        feel.slide_roll = 0.0;
    }
    // After an elimination the death cam has the camera.
    let dead = run.is_some_and(|r| r.ended == Some(RunEnd::Eliminated));
    let on = if dead { 0.0 } else { amount };
    feel.fov = on
        * t.fov_kick_deg.min(FOV_KICK_MAX_DEG).to_radians()
        * pulse(feel.kick_age, t.fov_kick_rise, t.fov_kick_seconds);
    feel.dip = on * feel.dip_depth.min(DIP_MAX) * pulse(feel.dip_age, 0.05, t.dip_seconds);
    // A slide leans left; a nudge rolls toward the hit (clockwise for the
    // right: a negative turn about the view axis).
    feel.roll = on
        * (feel.slide_roll
            - feel.nudge_side
                * t.nudge_deg.min(NUDGE_MAX_DEG).to_radians()
                * pulse(feel.nudge_age, 0.04, t.nudge_seconds));
    let Some(camera) = camera.as_mut() else {
        return;
    };
    let (transform, projection) = &mut **camera;
    if feel.dip != 0.0 {
        transform.translation.y -= feel.dip;
    }
    if feel.roll != 0.0 {
        transform.rotation *= Quat::from_rotation_z(feel.roll);
    }
    if let Some(Projection::Perspective(p)) = projection.as_deref_mut() {
        let kicking = feel.fov != 0.0;
        if kicking || feel.kicked {
            let base = match current.as_deref() {
                Some(c) => c.0,
                None => *feel.base_fov.get_or_insert(p.fov),
            };
            p.fov = base + feel.fov;
        }
        if !kicking && current.is_none() {
            feel.base_fov = None;
        }
        feel.kicked = kicking;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pulse_rises_and_falls_inside_its_length() {
        assert_eq!(pulse(-0.1, 0.06, 0.25), 0.0);
        assert_eq!(pulse(0.0, 0.06, 0.25), 0.0);
        assert!((pulse(0.06, 0.06, 0.25) - 1.0).abs() < 1e-5);
        assert!(pulse(0.2, 0.06, 0.25) < 0.2);
        assert_eq!(pulse(0.25, 0.06, 0.25), 0.0);
    }

    #[test]
    fn the_defaults_sit_inside_d108() {
        let t = CameraFeelTuning::default();
        assert!(t.fov_kick_deg <= FOV_KICK_MAX_DEG && t.fov_kick_seconds <= 0.25);
        assert!(t.dip_max <= DIP_MAX);
        assert!((2.0..=3.0).contains(&t.slide_roll_deg));
        assert!(t.nudge_deg <= NUDGE_MAX_DEG);
        assert!((fall_height(20.0f32.sqrt() * 6.0f32.sqrt(), 20.0) - 3.0).abs() < 1e-4);
    }
}
