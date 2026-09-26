//! The arena's look: the floating island (its grassy top with the build grid,
//! cliffs, clutter, barrier, and the props and margin models standing on it:
//! [`island`]), the target figure (the knight, in [`target`]), the key light
//! and the dense-grass part of the quality presets. Client only; gameplay
//! collision lives in [`super`]. The sky is the galaxy skybox and far view in
//! [`crate::far`].
//!
//! Everything near is drawn with [`crate::look::ToonMaterial`] (the island's
//! ground with its own toon-lit [`crate::look::GroundMaterial`]); cliffs,
//! props, trees and the figure get ink outlines and blob shadows. There are no
//! shadow maps or fog.
//!
//! Cost model (fanless M4, Low Power Mode): the island's static geometry is a
//! handful of merged meshes sharing four materials; the barrier is drawn only
//! near the camera; models batch per mesh.

mod barrier;
mod geo;
pub mod island;
mod scenery;
mod target;

pub use barrier::{BarrierMaterial, BarrierSide, REVEAL_END, barrier_reveal};
pub use scenery::{
    CLOSE_EDGE_Z, EDGE_CLEARANCE, FLOOR_CLUTTER_MAX_HEIGHT, Island, MIN_MARGIN, edge_distance,
    sun_direction,
};
pub use target::{
    FIGURE_SHADOW_RADIUS, KNIGHT_GATE, TargetFigure, TargetFigurePlugin, animate_knights,
    figure_hidden, pose_target_figures,
};

use crate::{
    look::{FarHaze, LookSettings, ToonLighting},
    palette,
    render::MainCamera,
};
use bevy::{camera::visibility::VisibilitySystems, light::SimulationLightSystems, prelude::*};

pub struct ArenaVisualsPlugin;

impl Plugin for ArenaVisualsPlugin {
    fn build(&self, app: &mut App) {
        // The floating island: its top, cliffs, clutter, barrier and models.
        app.add_plugins(island::IslandPlugin);
        // Only StandardMaterial stragglers (effect debris) still read these.
        app.insert_resource(GlobalAmbientLight {
            color: palette::BOUNCE,
            brightness: AMBIENT_BRIGHTNESS,
            affects_lightmapped_meshes: true,
        })
        // A default haze; the far view sets its own with the galaxy.
        .insert_resource(FarHaze {
            color: palette::FOG,
            start: FAR_HAZE_START,
            density: FAR_HAZE_DENSITY,
        })
        .add_systems(Startup, spawn_key_light)
        .add_systems(Update, (apply_quality_preset, follow_toon_lighting))
        .add_systems(
            PostUpdate,
            crate::scenario::gallery::apply_camera_override
                .after(TransformSystems::Propagate)
                .before(VisibilitySystems::UpdateFrusta)
                .before(SimulationLightSystems::UpdateDirectionalLightCascades),
        )
        // Non-player characters are drawn as the knight.
        .add_plugins(TargetFigurePlugin)
        .add_observer(dress_main_camera);
    }
}

// ---------------------------------------------------------------------------
// Light, haze and presets
// ---------------------------------------------------------------------------

/// Strength (lux) of the shadowless key light that mirrors [`ToonLighting`] for
/// the few `StandardMaterial`s left (effect debris), and the ambient fill
/// (cd/m²) they get. Toon surfaces ignore both.
pub const KEY_ILLUMINANCE: f32 = 3800.0;
pub const AMBIENT_BRIGHTNESS: f32 = 1150.0;
/// Default far-layer haze: none inside the near island (which ends about 40 m
/// out), 50% near 330 m. (`crate::far` replaces it with the galaxy's.)
pub const FAR_HAZE_START: f32 = 40.0;
pub const FAR_HAZE_DENSITY: f32 = 0.0028;

/// Scenery that only shows on the Plugged-in preset (extra grass tufts).
#[derive(Component, Debug)]
pub struct PluggedInOnly;

/// The shadowless key light (see [`KEY_ILLUMINANCE`]).
#[derive(Component, Debug)]
pub struct Sun;

/// Gives the main camera its clear color. (`look` sets its tonemapping and
/// MSAA; `far` gives it the galaxy skybox.)
fn dress_main_camera(add: On<Add, MainCamera>, mut cameras: Query<&mut Camera>) {
    if let Ok(mut camera) = cameras.get_mut(add.entity) {
        camera.clear_color = ClearColorConfig::Custom(palette::FOG);
    }
}

fn key_light_transform(lighting: &ToonLighting) -> Transform {
    let toward = lighting.key_direction.normalize_or(Vec3::Y);
    Transform::from_translation(toward * 100.0).looking_at(Vec3::ZERO, Vec3::Y)
}

fn spawn_key_light(mut commands: Commands, lighting: Res<ToonLighting>) {
    commands.spawn((
        Name::new("Key light"),
        Sun,
        DirectionalLight {
            color: lighting.key_color,
            illuminance: KEY_ILLUMINANCE,
            shadow_maps_enabled: false,
            ..default()
        },
        key_light_transform(&lighting),
    ));
}

/// Keeps the key light on the global toon lighting.
fn follow_toon_lighting(
    lighting: Res<ToonLighting>,
    mut lights: Query<(&mut DirectionalLight, &mut Transform), With<Sun>>,
) {
    if !lighting.is_changed() {
        return;
    }
    for (mut light, mut transform) in &mut lights {
        light.color = lighting.key_color;
        *transform = key_light_transform(&lighting);
    }
}

/// Applies the preset's dense grass live (cheap: only writes changes).
pub fn apply_quality_preset(
    settings: Res<LookSettings>,
    mut extras: Query<&mut Visibility, With<PluggedInOnly>>,
) {
    let visibility = if settings.dense_grass {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut v in &mut extras {
        v.set_if_neq(visibility);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::QualityPreset;

    fn preset_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<LookSettings>()
            .add_systems(Update, apply_quality_preset);
        app.world_mut()
            .spawn((PluggedInOnly, Visibility::Inherited));
        app
    }

    #[test]
    fn preset_switches_dense_grass_live() {
        let mut app = preset_app();
        app.update();
        let world = app.world_mut();
        let hidden = *world
            .query_filtered::<&Visibility, With<PluggedInOnly>>()
            .single(world)
            .unwrap();
        assert_eq!(hidden, Visibility::Hidden);

        *world.resource_mut::<LookSettings>() =
            crate::look::resolve_look(QualityPreset::PluggedIn, None, false);
        app.update();
        let world = app.world_mut();
        let shown = *world
            .query_filtered::<&Visibility, With<PluggedInOnly>>()
            .single(world)
            .unwrap();
        assert_eq!(shown, Visibility::Inherited);
    }

    #[test]
    fn sun_is_warm_late_afternoon() {
        let sun = sun_direction();
        let elevation = sun.y.asin().to_degrees();
        assert!((20.0..40.0).contains(&elevation), "elevation {elevation}");
        // From the spawn looking north, the sun sits to the west (left).
        assert!(sun.x < -0.5);
    }

    #[test]
    fn far_haze_starts_where_the_near_ground_ends() {
        let haze = FarHaze {
            color: palette::FOG,
            start: FAR_HAZE_START,
            density: FAR_HAZE_DENSITY,
        };
        assert_eq!(haze.amount(40.0), 0.0);
        assert!(
            haze.amount(60.0) < 0.01,
            "a clean seam at the ground's edge"
        );
        let half = haze.amount(330.0);
        assert!((0.4..0.6).contains(&half), "{half}");
        assert!(haze.amount(900.0) > 0.99);
    }

    #[test]
    fn the_key_light_follows_the_toon_lighting() {
        let lighting = ToonLighting::default();
        let light = key_light_transform(&lighting);
        // A directional light shines along its forward (-Z).
        assert!((light.forward().as_vec3() + lighting.key_direction.normalize()).length() < 1e-4);
    }
}
