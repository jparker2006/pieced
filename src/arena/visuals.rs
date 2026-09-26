//! The arena's look: the floating island (its grassy top with the build grid,
//! cliffs, clutter, barrier, and the props and margin models standing on it:
//! [`island`]), the sky, the target figure, and the dense-grass part of the
//! quality presets. Client only; gameplay collision lives in [`super`].
//!
//! Everything near is drawn with [`ToonMaterial`] (the island's ground with
//! its own toon-lit [`crate::look::GroundMaterial`]); cliffs, props, trees and
//! the figure get ink [`Outline`]s and [`BlobShadow`]s. There are no shadow
//! maps or fog. The far view (galaxy, station, far islands) is the sky
//! slice's.
//!
//! Cost model (fanless M4, Low Power Mode): the island's static geometry is a
//! handful of merged meshes sharing four materials; the barrier is drawn only
//! near the camera; models batch per mesh.

mod barrier;
mod geo;
pub mod island;
mod scenery;
mod sky;
mod target;

pub use barrier::{BarrierMaterial, BarrierSide, REVEAL_END, barrier_reveal};
pub use scenery::{
    CLOSE_EDGE_Z, EDGE_CLEARANCE, FLOOR_CLUTTER_MAX_HEIGHT, Island, MIN_MARGIN, edge_distance,
    sun_direction,
};
pub use sky::SkyMaterial;

use crate::{
    look::{
        BlobShadow, FarHaze, LookSettings, Outline, ToonLighting, ToonMaterial,
        with_outline_normals,
    },
    palette,
    render::MainCamera,
    shared::{AppState, Character, EyeHeight, Health, LookAngles, Player, PreviousFeet},
};
use bevy::{
    asset::io::embedded::EmbeddedAssetRegistry,
    camera::visibility::{NoFrustumCulling, VisibilitySystems},
    light::{NotShadowCaster, NotShadowReceiver, SimulationLightSystems},
    prelude::*,
};
use std::path::{Path, PathBuf};

pub struct ArenaVisualsPlugin;

impl Plugin for ArenaVisualsPlugin {
    fn build(&self, app: &mut App) {
        register_shaders(app);
        app.add_plugins((
            MaterialPlugin::<SkyMaterial>::default(),
            island::IslandPlugin,
        ))
            // Only StandardMaterial stragglers (effect debris) still read these.
            .insert_resource(GlobalAmbientLight {
                color: palette::BOUNCE,
                brightness: AMBIENT_BRIGHTNESS,
                affects_lightmapped_meshes: true,
            })
            // The far terrain melts into the Milestone 1 sky's horizon haze.
            // (The sky slice sets its own haze with the galaxy.)
            .insert_resource(FarHaze {
                color: palette::FOG,
                start: FAR_HAZE_START,
                density: FAR_HAZE_DENSITY,
            })
            .add_systems(Startup, spawn_key_light)
            .add_systems(Update, (apply_quality_preset, follow_toon_lighting))
            .add_systems(
                PostUpdate,
                (
                    pose_target_figures.before(TransformSystems::Propagate),
                    crate::scenario::gallery::apply_camera_override
                        .after(TransformSystems::Propagate)
                        .before(VisibilitySystems::UpdateFrusta)
                        .before(SimulationLightSystems::UpdateDirectionalLightCascades),
                ),
            )
            .add_observer(dress_main_camera)
            .add_observer(spawn_target_figure);
    }

    fn finish(&self, app: &mut App) {
        let world = app.world_mut();
        let (figure, dome) = {
            let mut meshes = world.resource_mut::<Assets<Mesh>>();
            (
                meshes.add(with_outline_normals(target::figure().into_mesh())),
                meshes.add(sky::dome(sky::SKY_RADIUS, 32, 16).into_mesh()),
            )
        };
        let target = world
            .resource_mut::<Assets<ToonMaterial>>()
            .add(target::target_material());
        let lighting = world.resource::<ToonLighting>().clone();
        let sky = world
            .resource_mut::<Assets<SkyMaterial>>()
            .add(sky_material(&lighting));
        world.insert_resource(LookAssets {
            figure,
            target,
            dome,
            sky,
        });
    }
}

/// Embeds the WGSL under `assets/shaders/` in the binary (no working-directory
/// dependency). Equivalent to `embedded_asset!`, with a fixed asset path because
/// that macro's path rule assumes the shader sits beside the Rust source.
fn register_shaders(app: &mut App) {
    let registry = app.world().resource::<EmbeddedAssetRegistry>();
    registry.insert_asset(
        PathBuf::new(),
        Path::new("pieced/shaders/sky.wgsl"),
        include_bytes!("../../assets/shaders/sky.wgsl").as_slice(),
    );
}

// ---------------------------------------------------------------------------
// Light, haze and presets
// ---------------------------------------------------------------------------

/// Strength (lux) of the shadowless key light that mirrors [`ToonLighting`] for
/// the few `StandardMaterial`s left (effect debris), and the ambient fill
/// (cd/m²) they get. Toon surfaces ignore both.
pub const KEY_ILLUMINANCE: f32 = 3800.0;
pub const AMBIENT_BRIGHTNESS: f32 = 1150.0;
/// Far-layer haze: none inside the near island (which ends about 40 m out),
/// 50% near 330 m. (The sky slice sets its own haze with the galaxy.)
pub const FAR_HAZE_START: f32 = 40.0;
pub const FAR_HAZE_DENSITY: f32 = 0.0028;

fn sky_material(lighting: &ToonLighting) -> SkyMaterial {
    SkyMaterial {
        zenith: palette::SKY_ZENITH.to_linear(),
        mid: palette::SKY_MID.to_linear(),
        horizon: palette::SKY_HORIZON.to_linear(),
        sun: palette::SUN_DISC.to_linear() * 1.4,
        haze: palette::FOG.to_linear(),
        sun_direction: lighting.key_direction.normalize_or(Vec3::Y).extend(0.0),
        params: Vec4::new(2.1f32.to_radians().cos(), 48.0, 0.22, 0.6),
    }
}

/// Handles shared by every figure and the sky.
#[derive(Resource, Debug, Clone)]
struct LookAssets {
    figure: Handle<Mesh>,
    target: Handle<ToonMaterial>,
    dome: Handle<Mesh>,
    sky: Handle<SkyMaterial>,
}

/// Scenery that only shows on the Plugged-in preset (extra grass tufts).
#[derive(Component, Debug)]
pub struct PluggedInOnly;

#[derive(Component, Debug)]
pub struct SkyDome;

/// The shadowless key light (see [`KEY_ILLUMINANCE`]).
#[derive(Component, Debug)]
pub struct Sun;

/// Gives the main camera its sky. (`look` sets its tonemapping and MSAA.)
fn dress_main_camera(
    add: On<Add, MainCamera>,
    mut commands: Commands,
    assets: Res<LookAssets>,
    mut cameras: Query<&mut Camera>,
) {
    if let Ok(mut camera) = cameras.get_mut(add.entity) {
        camera.clear_color = ClearColorConfig::Custom(palette::FOG);
    }
    commands.spawn((
        Name::new("Sky dome"),
        SkyDome,
        Mesh3d(assets.dome.clone()),
        MeshMaterial3d(assets.sky.clone()),
        Transform::IDENTITY,
        NoFrustumCulling,
        NotShadowCaster,
        NotShadowReceiver,
        ChildOf(add.entity),
    ));
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

/// Keeps the key light and the sky's sun on the global toon lighting.
fn follow_toon_lighting(
    lighting: Res<ToonLighting>,
    assets: Option<Res<LookAssets>>,
    mut skies: ResMut<Assets<SkyMaterial>>,
    mut lights: Query<(&mut DirectionalLight, &mut Transform), With<Sun>>,
) {
    if !lighting.is_changed() {
        return;
    }
    for (mut light, mut transform) in &mut lights {
        light.color = lighting.key_color;
        *transform = key_light_transform(&lighting);
    }
    if let Some(assets) = assets
        && let Some(mut sky) = skies.get_mut(&assets.sky)
    {
        sky.sun_direction = lighting.key_direction.normalize_or(Vec3::Y).extend(0.0);
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

// ---------------------------------------------------------------------------
// Target figures
// ---------------------------------------------------------------------------

/// The visible figure of a non-player character. A top-level entity that follows
/// its owner with render interpolation, faces its look direction and hides while
/// the owner is down.
#[derive(Component, Debug)]
pub struct TargetFigure {
    pub owner: Entity,
}

/// Blob shadow under a standing figure (a little wider than the body capsule).
pub const FIGURE_SHADOW_RADIUS: f32 = 0.45;

fn spawn_target_figure(
    add: On<Add, Character>,
    players: Query<(), With<Player>>,
    owners: Query<&Transform>,
    assets: Res<LookAssets>,
    mut commands: Commands,
) {
    // The player is invisible in first person.
    if players.contains(add.entity) {
        return;
    }
    let at = owners.get(add.entity).copied().unwrap_or_default();
    commands.spawn((
        Name::new("Target figure"),
        TargetFigure { owner: add.entity },
        Mesh3d(assets.figure.clone()),
        MeshMaterial3d(assets.target.clone()),
        Outline::default(),
        BlobShadow::new(FIGURE_SHADOW_RADIUS),
        Transform::from_translation(at.translation),
    ));
}

/// Whether a character's figure should be hidden: dead, or downed awaiting respawn.
pub fn figure_hidden(health: Option<&Health>, downed: bool) -> bool {
    downed || health.is_some_and(Health::is_dead)
}

pub fn pose_target_figures(
    mut commands: Commands,
    fixed: Res<Time<Fixed>>,
    state: Res<State<AppState>>,
    owners: Query<
        (
            &Transform,
            Option<&PreviousFeet>,
            Option<&LookAngles>,
            Option<&EyeHeight>,
            Option<&Health>,
            Has<crate::combat::Downed>,
        ),
        (With<Character>, Without<TargetFigure>),
    >,
    mut figures: Query<(Entity, &TargetFigure, &mut Transform, &mut Visibility)>,
) {
    let alpha = if *state.get() == AppState::Playing {
        fixed.overstep_fraction().clamp(0.0, 1.0)
    } else {
        1.0
    };
    for (entity, figure, mut transform, mut visibility) in &mut figures {
        let Ok((owner, previous, look, eye, health, downed)) = owners.get(figure.owner) else {
            commands.entity(entity).despawn();
            continue;
        };
        let feet = previous.map_or(owner.translation, |p| p.0.lerp(owner.translation, alpha));
        let yaw = look.map_or(0.0, |l| l.yaw);
        let crouch = eye.map_or(1.0, |e| (e.0 / EyeHeight::default().0).clamp(0.6, 1.0));
        transform.translation = feet;
        transform.rotation = Quat::from_rotation_y(yaw);
        transform.scale = Vec3::new(1.0, crouch, 1.0);
        visibility.set_if_neq(if figure_hidden(health, downed) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
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
    fn figure_hides_when_owner_is_down() {
        let mut health = Health::default();
        assert!(!figure_hidden(Some(&health), false));
        assert!(figure_hidden(Some(&health), true));
        health.apply(1000.0);
        assert!(figure_hidden(Some(&health), false));
        assert!(!figure_hidden(None, false));
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
    fn the_sky_sun_follows_the_key_light() {
        let lighting = ToonLighting::default();
        let sky = sky_material(&lighting);
        assert!(
            (sky.sun_direction.truncate() - lighting.key_direction.normalize()).length() < 1e-5
        );
        let light = key_light_transform(&lighting);
        // A directional light shines along its forward (-Z).
        assert!((light.forward().as_vec3() + lighting.key_direction.normalize()).length() < 1e-4);
    }
}
