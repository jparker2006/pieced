//! Slice D — the arena's look: faceted floor, boundary cliffs, backdrop, the
//! target figure, and the dense-grass part of the quality presets. Client
//! only; gameplay collision lives in [`super`]. The sky is the galaxy skybox in
//! [`crate::far`].
//!
//! Since Milestone 2 everything near is drawn with [`ToonMaterial`] (cliffs,
//! near trees and the figure also get ink [`Outline`]s), everything far with
//! [`FarMaterial`], and there are no shadow maps or fog: the figure gets a
//! [`BlobShadow`] and distance haze lives in the far material. This is still
//! the Milestone 1 scenery; the Phase 2 slices replace the art itself.
//!
//! Cost model (fanless M4, Low Power Mode): all static scenery is merged into a
//! dozen meshes sharing five materials; only the cliffs and near trees add an
//! outline hull draw.

mod geo;
mod scenery;
mod target;

pub use scenery::{EDGE_CLEARANCE, FLOOR_CLUTTER_MAX_HEIGHT, sun_direction};

use crate::{
    look::{
        BlobShadow, FarHaze, FarMaterial, LookSettings, Outline, ToonLighting, ToonMaterial,
        preset_look, with_outline_normals,
    },
    palette,
    render::MainCamera,
    shared::{AppState, Character, EyeHeight, Health, LookAngles, Player, PreviousFeet},
    tuning::Tuning,
};
use bevy::{camera::visibility::VisibilitySystems, light::SimulationLightSystems, prelude::*};

pub struct ArenaVisualsPlugin;

impl Plugin for ArenaVisualsPlugin {
    fn build(&self, app: &mut App) {
        // Only StandardMaterial stragglers (effect debris) still read these.
        app.insert_resource(GlobalAmbientLight {
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
        .add_systems(Startup, (spawn_key_light, spawn_scenery))
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
        let figure = world
            .resource_mut::<Assets<Mesh>>()
            .add(with_outline_normals(target::figure().into_mesh()));
        let target = world
            .resource_mut::<Assets<ToonMaterial>>()
            .add(target::target_material());
        world.insert_resource(LookAssets { figure, target });
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
/// Far-layer haze for the Milestone 1 scenery: none inside the near ground
/// (which ends 40 m out), 50% near 330 m, like the old distance fog.
pub const FAR_HAZE_START: f32 = 40.0;
pub const FAR_HAZE_DENSITY: f32 = 0.0028;

/// Handles shared by every figure.
#[derive(Resource, Debug, Clone)]
struct LookAssets {
    figure: Handle<Mesh>,
    target: Handle<ToonMaterial>,
}

/// Scenery that only shows on the Plugged-in preset.
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

// ---------------------------------------------------------------------------
// Static scenery
// ---------------------------------------------------------------------------

fn spawn_part<M: Material>(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    name: &'static str,
    geo: geo::Geo,
    material: &Handle<M>,
    outlined: bool,
) -> Option<Entity> {
    if geo.is_empty() {
        return None;
    }
    let mesh = if outlined {
        with_outline_normals(geo.into_mesh())
    } else {
        geo.into_mesh()
    };
    let mut e = commands.spawn((
        Name::new(name),
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material.clone()),
        Transform::IDENTITY,
    ));
    if outlined {
        e.insert(Outline::default());
    }
    Some(e.id())
}

fn spawn_scenery(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut toon: ResMut<Assets<ToonMaterial>>,
    mut far: ResMut<Assets<FarMaterial>>,
    tuning: Res<Tuning>,
) {
    let s = scenery::Scenery::generate();
    let triangles: usize = [
        &s.ground,
        &s.far_ground,
        &s.cliffs,
        &s.near_trees,
        &s.backdrop,
        &s.clouds,
        &s.pebbles,
    ]
    .iter()
    .map(|g| g.tri_count())
    .sum::<usize>()
        + s.grass.iter().map(geo::Geo::tri_count).sum::<usize>();
    info!("arena scenery: {triangles} triangles (Battery preset)");
    // Near: toon-shaded. The ground gets no rim (it would wash the far floor).
    let props = toon.add(ToonMaterial::vertex_colored());
    let ground = toon.add(ToonMaterial::vertex_colored().with_rim(0.0));
    // Grass blades are single triangles seen from both sides; their normals
    // point up so they shade like the ground they grow from.
    let grass = toon.add(ToonMaterial::vertex_colored().double_sided().with_rim(0.0));
    // Far: unlit and hazed. Clouds keep their baked shading and skip the haze.
    let distant = far.add(FarMaterial::default());
    let clouds = far.add(FarMaterial::default().with_haze(0.0));

    let (commands, meshes) = (&mut commands, &mut *meshes);
    spawn_part(commands, meshes, "Arena ground", s.ground, &ground, false);
    spawn_part(
        commands,
        meshes,
        "Far terrain",
        s.far_ground,
        &distant,
        false,
    );
    spawn_part(commands, meshes, "Boundary cliffs", s.cliffs, &props, true);
    spawn_part(commands, meshes, "Near trees", s.near_trees, &props, true);
    spawn_part(commands, meshes, "Backdrop", s.backdrop, &distant, false);
    spawn_part(commands, meshes, "Clouds", s.clouds, &clouds, false);
    spawn_part(commands, meshes, "Pebbles", s.pebbles, &ground, false);
    for chunk in s.grass {
        spawn_part(commands, meshes, "Grass", chunk, &grass, false);
    }
    let dense = preset_look(tuning.graphics.preset).dense_grass;
    let mut extras = Vec::new();
    for chunk in s.dense_grass {
        extras.extend(spawn_part(
            commands,
            meshes,
            "Dense grass",
            chunk,
            &grass,
            false,
        ));
    }
    for e in extras {
        commands.entity(e).insert((
            PluggedInOnly,
            if dense {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            },
        ));
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
    fn the_key_light_follows_the_toon_lighting() {
        let lighting = ToonLighting::default();
        let light = key_light_transform(&lighting);
        // A directional light shines along its forward (-Z).
        assert!((light.forward().as_vec3() + lighting.key_direction.normalize()).length() < 1e-4);
    }
}
