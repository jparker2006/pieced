//! Slice D — the arena's look: faceted floor, boundary cliffs, backdrop, the
//! target figure, and the dense-grass part of the quality presets. Client
//! only; gameplay collision lives in [`super`]. The sky is the galaxy skybox in
//! [`crate::far`].
//!
//! Since Milestone 2 everything near is drawn with [`ToonMaterial`] (cliffs,
//! near trees and the figure also get ink [`Outline`]s), everything far with
//! [`FarMaterial`], and there are no shadow maps or fog: the figure gets a
//! blob shadow and distance haze lives in the far material. This is still
//! the Milestone 1 scenery; the Phase 2 slices replace the art itself. The
//! figure is the knight, in [`target`].
//!
//! Cost model (fanless M4, Low Power Mode): all static scenery is merged into a
//! dozen meshes sharing five materials; only the cliffs and near trees add an
//! outline hull draw.

mod geo;
mod scenery;
mod target;

pub use scenery::{EDGE_CLEARANCE, FLOOR_CLUTTER_MAX_HEIGHT, sun_direction};
pub use target::{
    FIGURE_SHADOW_RADIUS, KNIGHT_GATE, TargetFigure, TargetFigurePlugin, animate_knights,
    figure_hidden, pose_target_figures,
};

use crate::{
    look::{
        FarHaze, FarMaterial, LookSettings, Outline, ToonLighting, ToonMaterial, preset_look,
        with_outline_normals,
    },
    palette,
    render::MainCamera,
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
        // (The far view sets its own haze with the galaxy.)
        .insert_resource(FarHaze {
            color: palette::FOG,
            start: FAR_HAZE_START,
            density: FAR_HAZE_DENSITY,
        })
        .add_systems(Startup, (spawn_key_light, spawn_scenery))
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
/// Far-layer haze for the Milestone 1 scenery: none inside the near ground
/// (which ends 40 m out), 50% near 330 m, like the old distance fog.
pub const FAR_HAZE_START: f32 = 40.0;
pub const FAR_HAZE_DENSITY: f32 = 0.0028;

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
