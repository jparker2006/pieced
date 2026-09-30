//! The death beat (D84), presentation only: while the run is
//! [`RunPhase::Dying`] the client runs virtual time at `death_time_scale`
//! (0.3×) for `death_seconds` of real time, the gun lowers out of view and a
//! soft vignette closes in. The surviving knights' victory hop is the
//! simulation's (`waves::victory_hop`), danced with chunk 4's clip.
//!
//! **The death cam** (M4 chunk 5, D108; it replaces D84's drop to the grass):
//! the camera pulls out of the player's eye to a third-person view over
//! [`PULL_SECONDS`], turning to frame the knight that got you (the killing
//! orb's source, [`DeathCam::killer`]), and holds on him while the knights
//! hop; the results come up once the hop is over
//! ([`super::results::results_ready`]). It only moves the rendered camera:
//! the player's look, eye and every aim ray stay where they were.
//!
//! Time runs at exactly 1.0 whenever the beat isn't playing: in any other
//! phase, after a restart, and while paused (menus never run slow).

use super::RunUi;
use crate::{
    render::{CameraFollowSet, MainCamera},
    shared::{AppState, Character, DamageDealt, EyeHeight, LookAngles, Player},
    tuning::Tuning,
    viewmodel::{ViewmodelCamera, ViewmodelSet},
    waves::{Run, RunEnd, RunPhase},
};
use bevy::prelude::*;

/// Real seconds the vignette takes to close in.
pub const DROP_SECONDS: f32 = 0.85;
/// Real seconds the death cam takes to pull out to third person.
pub const PULL_SECONDS: f32 = 0.6;
/// Where the death cam ends up: this far back from the player along the
/// line from the killer, this high over the player's feet, this far to the
/// side (m).
pub const CAM_BACK: f32 = 3.4;
pub const CAM_UP: f32 = 2.4;
pub const CAM_SIDE: f32 = 1.0;
/// Where on the killer it looks: his chest (m over his feet).
pub const KILLER_CHEST: f32 = 1.1;
/// The vignette's strength once the drop is done.
pub const VIGNETTE_ALPHA: f32 = 0.92;

pub(crate) fn build(app: &mut App) {
    app.init_resource::<DeathBeat>()
        .init_resource::<DeathCam>()
        .add_message::<DamageDealt>()
        .add_systems(Startup, spawn_vignette)
        .add_systems(Update, (death_time, death_look, find_killer).chain())
        .add_systems(
            PostUpdate,
            death_cam
                .in_set(DeathCamSet)
                .after(CameraFollowSet)
                .before(ViewmodelSet)
                .before(TransformSystems::Propagate),
        );
}

/// The virtual-time speed for the run's `phase`: `death_time_scale` during
/// the death beat while playing, exactly 1.0 otherwise.
pub fn death_time_scale(phase: Option<RunPhase>, playing: bool, tuning: &Tuning) -> f32 {
    match phase {
        Some(RunPhase::Dying { .. }) if playing => tuning.waves.death_time_scale.clamp(0.05, 1.0),
        _ => 1.0,
    }
}

/// How far the view has dropped (0 standing, 1 on the grass) `seconds` after
/// the elimination: a quick ease-in fall with a little bounce at the bottom.
pub fn drop_progress(seconds: f32) -> f32 {
    let t = (seconds / DROP_SECONDS).clamp(0.0, 1.0);
    if t < 0.8 {
        let x = t / 0.8;
        x * x * (1.6 - 0.6 * x)
    } else {
        // The bump as the head meets the grass.
        let x = (t - 0.8) / 0.2;
        1.0 - 0.06 * (x * std::f32::consts::PI).sin()
    }
}

/// Where the death cam moves the camera (PostUpdate, after the camera has
/// followed the eye). The camera feel ([`crate::fx::camera`]) runs before it.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeathCamSet;

/// The death cam's state (see the module docs).
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct DeathCam {
    /// The knight whose orb (or hit) killed the player, once known.
    pub killer: Option<Entity>,
    /// Real seconds since the player went down, while the cam plays.
    pub since: Option<f32>,
    /// Where the camera is this frame (for tests and the board).
    pub pose: Option<Transform>,
}

/// How far the pull-out is `seconds` after the elimination (0 at the eye, 1
/// in third person): an ease in and out.
pub fn pull_progress(seconds: f32) -> f32 {
    let t = (seconds / PULL_SECONDS).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The death cam's third-person pose: back from the player's `feet` along
/// the line from the `killer`'s feet, up and to the side, looking at his
/// chest. `ahead` is the player's facing, used when there is no killer.
pub fn death_cam_pose(feet: Vec3, eye: Vec3, ahead: Vec3, killer: Option<Vec3>) -> Transform {
    let flat = ahead.with_y(0.0).normalize_or(Vec3::NEG_Z);
    let (toward, target) = match killer {
        Some(k) => (
            (k - feet).with_y(0.0).normalize_or(flat),
            k + Vec3::Y * KILLER_CHEST,
        ),
        None => (flat, eye + flat * 6.0 - Vec3::Y * 0.6),
    };
    let side = toward.cross(Vec3::Y).normalize_or(Vec3::X);
    let at = feet - toward * CAM_BACK + Vec3::Y * CAM_UP + side * CAM_SIDE;
    Transform::from_translation(at).looking_at(target, Vec3::Y)
}

/// The drop's state: real seconds since the player went down (while the run
/// is ended by an elimination).
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct DeathBeat {
    pub since: Option<f32>,
}

impl DeathBeat {
    /// The drop's progress now (0 when no drop is playing).
    pub fn progress(&self) -> f32 {
        self.since.map_or(0.0, drop_progress)
    }
}

/// D84's slow motion: 0.3× during the beat, exactly 1.0 otherwise.
fn death_time(
    run: Option<Res<Run>>,
    state: Res<State<AppState>>,
    tuning: Res<Tuning>,
    mut time: ResMut<Time<Virtual>>,
) {
    let speed = death_time_scale(
        run.map(|r| r.phase),
        *state.get() == AppState::Playing,
        &tuning,
    );
    if time.relative_speed() != speed {
        time.set_relative_speed(speed);
    }
}

/// The drop's clock, the vignette, and the gun out of view.
fn death_look(
    time: Res<Time<Real>>,
    run: Option<Res<Run>>,
    state: Res<State<AppState>>,
    mut beat: ResMut<DeathBeat>,
    mut vignette: Query<(&RunUi, &mut BackgroundGradient, &mut Visibility)>,
    mut viewmodel: Query<&mut Camera, With<ViewmodelCamera>>,
) {
    let (fallen, ended) = run.as_deref().map_or((false, false), |r| {
        (r.ended == Some(RunEnd::Eliminated), r.is_ended())
    });
    beat.since = if fallen {
        Some(beat.since.map_or(0.0, |s| s + time.delta_secs()))
    } else {
        None
    };
    let p = beat.progress();
    let booting = *state.get() == AppState::Boot;
    for (part, mut gradient, mut v) in &mut vignette {
        if *part != RunUi::Vignette {
            continue;
        }
        v.set_if_neq(if p > 0.0 || booting {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        // It closes in quickly, then settles as the head meets the grass.
        let alpha = VIGNETTE_ALPHA * p.clamp(0.0, 1.0).sqrt();
        let current = match gradient.0.first() {
            Some(Gradient::Radial(r)) => r.stops.last().map(|s| s.color.alpha()),
            _ => None,
        };
        if current != Some(alpha) {
            set_vignette_alpha(&mut gradient, alpha);
        }
    }
    // The gun drops out of view with the player (and is back for the next
    // run). While paused the pause blur owns the 3D cameras.
    if *state.get() == AppState::Paused {
        return;
    }
    for mut camera in &mut viewmodel {
        let active = !ended;
        if camera.is_active != active {
            camera.is_active = active;
        }
    }
}

/// Remembers who killed the player: the source of the killing hit (the
/// orb's caster). The death cam forgets it when a new run starts.
fn find_killer(
    mut hits: MessageReader<DamageDealt>,
    player: Option<Single<Entity, With<Player>>>,
    mut cam: ResMut<DeathCam>,
) {
    let me = player.map(|p| *p);
    for hit in hits.read() {
        if Some(hit.target) == me && hit.killed && hit.source.is_some() && hit.source != me {
            cam.killer = hit.source;
        }
    }
}

/// The death cam (see the module docs): pulls out of the eye to third
/// person and turns to frame the killer, then holds.
#[allow(clippy::type_complexity)]
fn death_cam(
    time: Res<Time<Real>>,
    run: Option<Res<Run>>,
    mut cam: ResMut<DeathCam>,
    player: Option<Single<(&Transform, &EyeHeight, &LookAngles), (With<Player>, Without<MainCamera>)>>,
    bodies: Query<&Transform, (With<Character>, Without<MainCamera>, Without<Player>)>,
    mut camera: Option<Single<&mut Transform, With<MainCamera>>>,
) {
    let fallen = run
        .as_deref()
        .is_some_and(|r| r.ended == Some(RunEnd::Eliminated));
    if !fallen {
        if cam.since.is_some() || cam.pose.is_some() {
            cam.since = None;
            cam.pose = None;
            cam.killer = None;
        }
        return;
    }
    let Some(player) = player else {
        return;
    };
    let (body, eye_height, look) = player.into_inner();
    let since = cam.since.map_or(0.0, |s| s + time.delta_secs());
    cam.since = Some(since);
    let feet = body.translation;
    let eye = feet + Vec3::Y * eye_height.0;
    // The killer, while he is still on the island (not back in the pool).
    let killer = cam
        .killer
        .and_then(|k| bodies.get(k).ok())
        .map(|t| t.translation)
        .filter(|k| k.y > -10.0);
    let start = Transform::from_translation(eye).with_rotation(look.rotation());
    let end = death_cam_pose(feet, eye, look.forward(), killer);
    let k = pull_progress(since);
    let pose = Transform {
        translation: start.translation.lerp(end.translation, k),
        rotation: start.rotation.slerp(end.rotation, k),
        scale: Vec3::ONE,
    };
    cam.pose = Some(pose);
    if let Some(camera) = camera.as_mut() {
        camera.translation = pose.translation;
        camera.rotation = pose.rotation;
    }
}

/// The vignette's colour at its edge (a dusk-violet ink) and where, as a
/// percentage of the way to the corners, it starts to darken.
pub const VIGNETTE_INK: Color = Color::srgb(0.086, 0.03, 0.14);
pub const VIGNETTE_CLEAR: f32 = 30.0;

/// The vignette's gradient at `alpha` (0..=1).
pub fn vignette_gradient(alpha: f32) -> RadialGradient {
    RadialGradient::new(
        UiPosition::CENTER,
        RadialGradientShape::FarthestCorner,
        vec![
            ColorStop::percent(VIGNETTE_INK.with_alpha(0.0), VIGNETTE_CLEAR),
            ColorStop::percent(VIGNETTE_INK.with_alpha(0.6 * alpha), 68.0),
            ColorStop::percent(VIGNETTE_INK.with_alpha(alpha), 100.0),
        ],
    )
}

/// Sets the vignette's stops to `alpha` in place (no allocation).
fn set_vignette_alpha(gradient: &mut BackgroundGradient, alpha: f32) {
    let want = vignette_gradient(alpha);
    if let Some(Gradient::Radial(radial)) = gradient.0.first_mut() {
        for (stop, want) in radial.stops.iter_mut().zip(&want.stops) {
            stop.color = want.color;
        }
    }
}

fn spawn_vignette(mut commands: Commands) {
    commands.spawn((
        Name::new("Death vignette"),
        RunUi::Vignette,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        BackgroundGradient(vec![vignette_gradient(0.0).into()]),
        GlobalZIndex(8),
        // Drawn (clear) behind the loading screen, so its pipeline is
        // compiled before the first death.
        Visibility::Inherited,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_motion_only_while_dying_and_playing() {
        let t = Tuning::default();
        let dying = Some(RunPhase::Dying { until: 10 });
        assert_eq!(death_time_scale(dying, true, &t), 0.3);
        assert_eq!(death_time_scale(dying, false, &t), 1.0, "paused");
        assert_eq!(death_time_scale(Some(RunPhase::Fighting), true, &t), 1.0);
        assert_eq!(
            death_time_scale(Some(RunPhase::Over { tick: 1 }), true, &t),
            1.0
        );
        assert_eq!(death_time_scale(None, true, &t), 1.0);
    }

    #[test]
    fn the_view_falls_to_the_grass_and_stays() {
        assert_eq!(drop_progress(0.0), 0.0);
        let mid = drop_progress(DROP_SECONDS * 0.4);
        assert!((0.1..0.9).contains(&mid), "{mid}");
        assert!((drop_progress(DROP_SECONDS) - 1.0).abs() < 1e-5);
        assert_eq!(drop_progress(30.0), drop_progress(DROP_SECONDS));
        // Monotonic until the bump at the bottom.
        let mut last = 0.0;
        for i in 0..=80 {
            let p = drop_progress(DROP_SECONDS * 0.8 * i as f32 / 80.0);
            assert!(p >= last - 1e-6);
            last = p;
        }
    }

    #[test]
    fn the_death_cam_pulls_back_and_up_and_looks_at_the_killer() {
        let feet = Vec3::new(2.0, 0.0, 14.0);
        let eye = feet + Vec3::Y * 1.6;
        let killer = Vec3::new(2.0, 0.0, 4.0);
        let pose = death_cam_pose(feet, eye, Vec3::X, Some(killer));
        let to_killer = (killer + Vec3::Y * KILLER_CHEST - pose.translation).normalize();
        assert!(pose.forward().dot(to_killer) > 0.9999, "framed on his chest");
        assert!(pose.translation.z > feet.z + 3.0, "behind the player");
        assert!(pose.translation.y > 2.0, "above");
        // No killer: behind and above, looking where the player looked.
        let lone = death_cam_pose(feet, eye, Vec3::NEG_Z, None);
        assert!(lone.forward().dot(Vec3::NEG_Z) > 0.8);
        assert_eq!(pull_progress(0.0), 0.0);
        assert_eq!(pull_progress(PULL_SECONDS), 1.0);
        assert!((pull_progress(PULL_SECONDS / 2.0) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn the_vignette_is_clear_in_the_middle_and_dark_at_the_corners() {
        let g = vignette_gradient(0.8);
        assert_eq!(g.stops.first().unwrap().color.alpha(), 0.0);
        assert!((g.stops.last().unwrap().color.alpha() - 0.8).abs() < 1e-5);
        let mut bg = BackgroundGradient(vec![vignette_gradient(0.0).into()]);
        set_vignette_alpha(&mut bg, 0.5);
        let Some(Gradient::Radial(r)) = bg.0.first() else {
            panic!("radial")
        };
        assert!((r.stops.last().unwrap().color.alpha() - 0.5).abs() < 1e-5);
    }
}
