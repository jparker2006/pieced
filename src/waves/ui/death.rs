//! The death beat (D84), presentation only: while the run is
//! [`RunPhase::Dying`] the client runs virtual time at `death_time_scale`
//! (0.3×) for `death_seconds` of real time, the view drops to the grass with a
//! slight roll, the gun lowers out of view and a soft vignette closes in. The
//! surviving knights' victory hop is the simulation's (`waves::victory_hop`).
//!
//! Time runs at exactly 1.0 whenever the beat isn't playing: in any other
//! phase, after a restart, and while paused (menus never run slow).

use super::RunUi;
use crate::{
    render::{CameraFollowSet, MainCamera},
    shared::AppState,
    tuning::Tuning,
    viewmodel::{ViewmodelCamera, ViewmodelSet},
    waves::{Run, RunEnd, RunPhase},
};
use bevy::prelude::*;

/// How far above the feet the eye ends up on the grass (m).
pub const GRASS_EYE: f32 = 0.32;
/// The view's roll on the grass (radians) and how far it tips up.
pub const DROP_ROLL: f32 = 0.42;
pub const DROP_PITCH: f32 = 0.12;
/// Real seconds the drop takes.
pub const DROP_SECONDS: f32 = 0.85;
/// The vignette's strength once the drop is done.
pub const VIGNETTE_ALPHA: f32 = 0.92;

pub(crate) fn build(app: &mut App) {
    app.init_resource::<DeathBeat>()
        .add_systems(Startup, spawn_vignette)
        .add_systems(Update, (death_time, death_look).chain())
        .add_systems(
            PostUpdate,
            drop_view
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
    // The gun drops out of view with the player (and is back for the next run).
    for mut camera in &mut viewmodel {
        let active = !ended;
        if camera.is_active != active {
            camera.is_active = active;
        }
    }
}

/// Lowers and rolls the view toward the grass over the beat.
fn drop_view(
    beat: Res<DeathBeat>,
    mut camera: Option<Single<&mut Transform, With<MainCamera>>>,
    eye: Option<Single<&crate::shared::EyeHeight, With<crate::shared::Player>>>,
) {
    let p = beat.progress();
    if p <= 0.0 {
        return;
    }
    let (Some(camera), Some(eye)) = (camera.as_mut(), eye) else {
        return;
    };
    let fall = (eye.0 - GRASS_EYE).max(0.0) * p;
    camera.translation.y -= fall;
    camera.rotation *= Quat::from_rotation_z(DROP_ROLL * p) * Quat::from_rotation_x(DROP_PITCH * p);
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
