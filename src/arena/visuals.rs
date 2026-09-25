//! Slice D — the arena's look: faceted floor, boundary cliffs, backdrop, sky, sun,
//! fog, palette materials, the target figure and its rim material, and the two
//! quality presets. Client only; gameplay collision lives in [`super`].
//!
//! Cost model (fanless M4, Low Power Mode): all static scenery is merged into a
//! dozen meshes sharing four materials; only the cliffs, near trees, pieces and
//! characters cast shadows, into a single cascade sized to the arena.

mod geo;
mod scenery;
mod sky;
mod target;

pub use scenery::{EDGE_CLEARANCE, FLOOR_CLUTTER_MAX_HEIGHT, sun_direction};
pub use sky::SkyMaterial;
pub use target::{TargetMaterial, TargetRim};

use crate::{
    palette,
    render::{MainCamera, QualityPreset},
    shared::{AppState, Character, EyeHeight, Health, LookAngles, Player, PreviousFeet},
    tuning::Tuning,
};
use bevy::{
    asset::io::embedded::EmbeddedAssetRegistry,
    camera::visibility::{NoFrustumCulling, VisibilitySystems},
    core_pipeline::tonemapping::Tonemapping,
    light::{
        CascadeShadowConfigBuilder, DirectionalLightShadowMap, NotShadowCaster, NotShadowReceiver,
        ShadowFilteringMethod, SimulationLightSystems,
    },
    pbr::{DistanceFog, FogFalloff},
    prelude::*,
};
use std::path::{Path, PathBuf};

pub struct ArenaVisualsPlugin;

impl Plugin for ArenaVisualsPlugin {
    fn build(&self, app: &mut App) {
        register_shaders(app);
        let battery = preset_look(QualityPreset::Battery);
        app.add_plugins((
            MaterialPlugin::<SkyMaterial>::default(),
            MaterialPlugin::<TargetMaterial>::default(),
        ))
        .insert_resource(DirectionalLightShadowMap {
            size: battery.shadow_map,
        })
        .insert_resource(GlobalAmbientLight {
            color: palette::BOUNCE,
            brightness: AMBIENT_BRIGHTNESS,
            affects_lightmapped_meshes: true,
        })
        .add_systems(Startup, (spawn_sun, spawn_scenery))
        .add_systems(Update, apply_quality_preset)
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
                meshes.add(target::figure().into_mesh()),
                meshes.add(sky::dome(sky::SKY_RADIUS, 32, 16).into_mesh()),
            )
        };
        let target = world
            .resource_mut::<Assets<TargetMaterial>>()
            .add(target::target_material());
        let sky = world
            .resource_mut::<Assets<SkyMaterial>>()
            .add(sky_material());
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
    for (path, bytes) in [
        (
            "pieced/shaders/sky.wgsl",
            include_bytes!("../../assets/shaders/sky.wgsl").as_slice(),
        ),
        (
            "pieced/shaders/target_rim.wgsl",
            include_bytes!("../../assets/shaders/target_rim.wgsl").as_slice(),
        ),
    ] {
        registry.insert_asset(PathBuf::new(), Path::new(path), bytes);
    }
}

// ---------------------------------------------------------------------------
// Light, fog and presets
// ---------------------------------------------------------------------------

/// Sun strength (lux) and ambient fill (cd/m²) at the default exposure. The fill is
/// bright on purpose: the inside of a 1x1 box must never go dark.
pub const SUN_ILLUMINANCE: f32 = 3800.0;
/// Bevy scales diffuse ambient by its environment-BRDF fit (~0.45 at our
/// roughness), so this lands near half the albedo in full shadow.
pub const AMBIENT_BRIGHTNESS: f32 = 1150.0;
/// Shadowless cool fill from high above, opposite the sun: sky light that keeps
/// form on the shadow side instead of a flat ambient wash.
pub const SKYLIGHT_ILLUMINANCE: f32 = 1100.0;
/// The single shadow cascade covers this far from the camera: corner to corner
/// across the arena, plus the cliffs.
pub const SHADOW_DISTANCE: f32 = 64.0;

/// What a quality preset changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresetLook {
    pub shadow_map: usize,
    /// Extra grass tufts on the arena floor.
    pub dense_grass: bool,
    /// Sun in-scattering in the fog (costs a shadow lookup per fogged pixel).
    pub sun_scatter: bool,
    pub filtering: ShadowFilteringMethod,
}

pub fn preset_look(preset: QualityPreset) -> PresetLook {
    match preset {
        QualityPreset::Battery => PresetLook {
            shadow_map: 2048,
            dense_grass: false,
            sun_scatter: false,
            filtering: ShadowFilteringMethod::Gaussian,
        },
        QualityPreset::PluggedIn => PresetLook {
            shadow_map: 4096,
            dense_grass: true,
            sun_scatter: true,
            filtering: ShadowFilteringMethod::Gaussian,
        },
    }
}

fn sun_scatter_color(on: bool) -> Color {
    if on {
        Color::srgba(1.0, 0.8, 0.55, 0.06)
    } else {
        Color::NONE
    }
}

pub fn distance_fog(look: &PresetLook) -> DistanceFog {
    DistanceFog {
        color: palette::FOG,
        directional_light_color: sun_scatter_color(look.sun_scatter),
        directional_light_exponent: 14.0,
        // ~4% at the far arena wall, 50% at 300 m, gone by 700 m.
        falloff: FogFalloff::ExponentialSquared { density: 0.0028 },
    }
}

fn sky_material() -> SkyMaterial {
    SkyMaterial {
        zenith: palette::SKY_ZENITH.to_linear(),
        mid: palette::SKY_MID.to_linear(),
        horizon: palette::SKY_HORIZON.to_linear(),
        sun: palette::SUN_DISC.to_linear() * 1.4,
        params: Vec4::new(2.1f32.to_radians().cos(), 48.0, 0.22, 0.6),
    }
}

/// Handles shared by every figure and the sky.
#[derive(Resource, Debug, Clone)]
struct LookAssets {
    figure: Handle<Mesh>,
    target: Handle<TargetMaterial>,
    dome: Handle<Mesh>,
    sky: Handle<SkyMaterial>,
}

/// Scenery that only shows on the Plugged-in preset.
#[derive(Component, Debug)]
pub struct PluggedInOnly;

#[derive(Component, Debug)]
pub struct SkyDome;

#[derive(Component, Debug)]
pub struct Sun;

/// Gives the main camera its look: fog, tonemapping, shadow filtering and the sky.
fn dress_main_camera(
    add: On<Add, MainCamera>,
    mut commands: Commands,
    assets: Res<LookAssets>,
    tuning: Res<Tuning>,
    mut cameras: Query<&mut Camera>,
) {
    let look = preset_look(tuning.graphics.preset);
    if let Ok(mut camera) = cameras.get_mut(add.entity) {
        camera.clear_color = ClearColorConfig::Custom(palette::FOG);
    }
    commands.entity(add.entity).insert((
        Tonemapping::KhronosPbrNeutral,
        distance_fog(&look),
        look.filtering,
    ));
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

fn spawn_sun(mut commands: Commands) {
    let dir = sun_direction();
    let fill = Vec3::new(-dir.x * 0.5, 1.0, -dir.z * 0.5).normalize();
    commands.spawn((
        Name::new("Skylight"),
        DirectionalLight {
            color: palette::SKYLIGHT,
            illuminance: SKYLIGHT_ILLUMINANCE,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_translation(fill * 100.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Name::new("Sun"),
        Sun,
        DirectionalLight {
            color: palette::SUNLIGHT,
            illuminance: SUN_ILLUMINANCE,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_translation(dir * 100.0).looking_at(Vec3::ZERO, Vec3::Y),
        CascadeShadowConfigBuilder {
            num_cascades: 1,
            minimum_distance: 0.1,
            maximum_distance: SHADOW_DISTANCE,
            first_cascade_far_bound: SHADOW_DISTANCE,
            overlap_proportion: 0.2,
        }
        .build(),
    ));
}

/// Applies the current preset live (cheap to run every frame: only writes changes).
pub fn apply_quality_preset(
    tuning: Res<Tuning>,
    mut shadow_map: ResMut<DirectionalLightShadowMap>,
    mut extras: Query<&mut Visibility, With<PluggedInOnly>>,
    mut cameras: Query<(&mut DistanceFog, &mut ShadowFilteringMethod), With<MainCamera>>,
) {
    let look = preset_look(tuning.graphics.preset);
    if shadow_map.size != look.shadow_map {
        shadow_map.size = look.shadow_map;
    }
    let visibility = if look.dense_grass {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut v in &mut extras {
        v.set_if_neq(visibility);
    }
    let scatter = sun_scatter_color(look.sun_scatter);
    for (mut fog, mut filtering) in &mut cameras {
        if fog.directional_light_color != scatter {
            fog.directional_light_color = scatter;
        }
        filtering.set_if_neq(look.filtering);
    }
}

// ---------------------------------------------------------------------------
// Static scenery
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shadows {
    CastAndReceive,
    ReceiveOnly,
    None,
}

fn spawn_scenery(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
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
    let lit = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.92,
        reflectance: 0.2,
        ..default()
    });
    // Grass blades are single triangles seen from both sides; their normals point
    // up so they shade like the ground they grow from.
    let grass = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.95,
        reflectance: 0.1,
        cull_mode: None,
        ..default()
    });
    let clouds = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        fog_enabled: false,
        ..default()
    });
    let mut part =
        |name: &'static str, geo: geo::Geo, material: &Handle<StandardMaterial>, shadows| {
            if geo.is_empty() {
                return None;
            }
            let mut e = commands.spawn((
                Name::new(name),
                Mesh3d(meshes.add(geo.into_mesh())),
                MeshMaterial3d(material.clone()),
                Transform::IDENTITY,
            ));
            if shadows != Shadows::CastAndReceive {
                e.insert(NotShadowCaster);
            }
            if shadows == Shadows::None {
                e.insert(NotShadowReceiver);
            }
            Some(e.id())
        };
    part("Arena ground", s.ground, &lit, Shadows::ReceiveOnly);
    part("Far terrain", s.far_ground, &lit, Shadows::None);
    part("Boundary cliffs", s.cliffs, &lit, Shadows::CastAndReceive);
    part("Near trees", s.near_trees, &lit, Shadows::CastAndReceive);
    part("Backdrop", s.backdrop, &lit, Shadows::None);
    part("Clouds", s.clouds, &clouds, Shadows::None);
    part("Pebbles", s.pebbles, &lit, Shadows::ReceiveOnly);
    for chunk in s.grass {
        part("Grass", chunk, &grass, Shadows::ReceiveOnly);
    }
    let dense = preset_look(tuning.graphics.preset).dense_grass;
    let mut extras = Vec::new();
    for chunk in s.dense_grass {
        extras.extend(part("Dense grass", chunk, &grass, Shadows::ReceiveOnly));
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
        Transform::from_translation(at.translation),
    ));
}

/// Whether a character's figure should be hidden.
// TODO(slice C): also hide while the dummy carries its "downed" marker, e.g.
// `downed: bool` from `Has<Downed>` in `pose_target_figures`, once that lands.
pub fn figure_hidden(health: Option<&Health>) -> bool {
    health.is_some_and(Health::is_dead)
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
        let Ok((owner, previous, look, eye, health)) = owners.get(figure.owner) else {
            commands.entity(entity).despawn();
            continue;
        };
        let feet = previous.map_or(owner.translation, |p| p.0.lerp(owner.translation, alpha));
        let yaw = look.map_or(0.0, |l| l.yaw);
        let crouch = eye.map_or(1.0, |e| (e.0 / EyeHeight::default().0).clamp(0.6, 1.0));
        transform.translation = feet;
        transform.rotation = Quat::from_rotation_y(yaw);
        transform.scale = Vec3::new(1.0, crouch, 1.0);
        visibility.set_if_neq(if figure_hidden(health) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_trade_quality_for_headroom() {
        let battery = preset_look(QualityPreset::Battery);
        let plugged = preset_look(QualityPreset::PluggedIn);
        assert_eq!(battery.shadow_map, 2048);
        assert_eq!(plugged.shadow_map, 4096);
        assert!(!battery.dense_grass && plugged.dense_grass);
        assert!(!battery.sun_scatter && plugged.sun_scatter);
        assert_eq!(QualityPreset::default(), QualityPreset::Battery);
    }

    fn preset_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Tuning>()
            .init_resource::<DirectionalLightShadowMap>()
            .add_systems(Update, apply_quality_preset);
        app.world_mut()
            .spawn((PluggedInOnly, Visibility::Inherited));
        app.world_mut().spawn((
            MainCamera,
            distance_fog(&preset_look(QualityPreset::PluggedIn)),
            ShadowFilteringMethod::Hardware2x2,
        ));
        app
    }

    #[test]
    fn preset_switches_live() {
        let mut app = preset_app();
        app.update();
        let world = app.world_mut();
        assert_eq!(world.resource::<DirectionalLightShadowMap>().size, 2048);
        let hidden = *world
            .query_filtered::<&Visibility, With<PluggedInOnly>>()
            .single(world)
            .unwrap();
        assert_eq!(hidden, Visibility::Hidden);
        let fog = world.query::<&DistanceFog>().single(world).unwrap();
        assert_eq!(fog.directional_light_color, Color::NONE);

        world.resource_mut::<Tuning>().graphics.preset = QualityPreset::PluggedIn;
        app.update();
        let world = app.world_mut();
        assert_eq!(world.resource::<DirectionalLightShadowMap>().size, 4096);
        let shown = *world
            .query_filtered::<&Visibility, With<PluggedInOnly>>()
            .single(world)
            .unwrap();
        assert_eq!(shown, Visibility::Inherited);
        let fog = world.query::<&DistanceFog>().single(world).unwrap();
        assert!(fog.directional_light_color.alpha() > 0.0);
    }

    #[test]
    fn figure_hides_when_owner_is_down() {
        let mut health = Health::default();
        assert!(!figure_hidden(Some(&health)));
        health.apply(1000.0);
        assert!(figure_hidden(Some(&health)));
        assert!(!figure_hidden(None));
    }

    #[test]
    fn sun_is_warm_late_afternoon() {
        let sun = sun_direction();
        let elevation = sun.y.asin().to_degrees();
        assert!((20.0..40.0).contains(&elevation), "elevation {elevation}");
        // From the spawn looking north, the sun sits to the west (left).
        assert!(sun.x < -0.5);
    }
}
