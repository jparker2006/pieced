//! Blob shadows: a soft dark decal quad on the ground under an entity, instead
//! of shadow maps. The decal is its own top-level entity (so it never inherits
//! the owner's rotation or squash), placed by a downward ray against world and
//! piece colliders, and multiplied onto the ground in a cool violet.
//!
//! Static props cost one ray when they appear; moving characters one ray per
//! frame they move. The decal shrinks and fades as its owner rises (jumps).

use super::outline::NoOutline;
use crate::shared::Layer;
use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use bevy::{
    camera::visibility::{NoFrustumCulling, VisibilitySystems},
    light::NotShadowCaster,
    mesh::MeshTag,
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
};

pub const BLOB_SHADER_PATH: &str = "embedded://pieced/shaders/blob.wgsl";
/// The ray starts this far above the owner's origin (origins sit at the feet
/// or base, a hair above or inside the ground).
pub const PROBE_LIFT: f32 = 0.3;
/// Longest drop a shadow is drawn for.
pub const PROBE_DEPTH: f32 = 30.0;
/// Decals float this far above the surface they lie on.
pub const DECAL_LIFT: f32 = 0.025;
/// Height over which a rising owner's shadow shrinks and fades.
pub const FADE_HEIGHT: f32 = 3.0;

/// Keep a soft blob shadow under this entity. `radius` in meters; `opacity`
/// 0..1 at ground contact.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct BlobShadow {
    pub radius: f32,
    pub opacity: f32,
}

impl BlobShadow {
    pub fn new(radius: f32) -> Self {
        Self {
            radius,
            opacity: 0.55,
        }
    }
}

impl Default for BlobShadow {
    fn default() -> Self {
        Self::new(0.5)
    }
}

/// The decal entity of a [`BlobShadow`].
#[derive(Component, Debug, Clone, Copy)]
pub struct BlobDecal {
    pub owner: Entity,
}

/// On a [`BlobShadow`] owner: its decal.
#[derive(Component, Debug, Clone, Copy)]
pub struct BlobDecalLink(pub Entity);

/// Where the ground was found under a decal's owner (`None`: nothing below).
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
pub struct BlobGround(pub Option<(Vec3, Vec3)>);

/// Multiply-blended decal material; per-decal opacity comes from its `MeshTag`.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct BlobMaterial {
    /// The shadow's tint (what fully covered ground is multiplied by).
    #[uniform(0)]
    pub color: LinearRgba,
    /// x: radius where the soft edge starts (0..1), y: overall strength.
    #[uniform(0)]
    pub params: Vec4,
}

impl Default for BlobMaterial {
    fn default() -> Self {
        Self {
            color: Color::srgb(0.36, 0.3, 0.52).to_linear(),
            params: Vec4::new(0.25, 1.0, 0.0, 0.0),
        }
    }
}

impl Material for BlobMaterial {
    fn fragment_shader() -> ShaderRef {
        BLOB_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Multiply
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }
}

#[derive(Resource, Debug, Clone)]
pub struct BlobAssets {
    pub quad: Handle<Mesh>,
    pub material: Handle<BlobMaterial>,
}

pub(crate) fn create_blob_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<BlobMaterial>>,
) {
    commands.insert_resource(BlobAssets {
        quad: meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(0.5))),
        material: materials.add(BlobMaterial::default()),
    });
}

/// Decal diameter and opacity for an owner `height` meters above the ground.
pub fn blob_footprint(shadow: &BlobShadow, height: f32) -> (f32, f32) {
    let t = (height.max(0.0) / FADE_HEIGHT).clamp(0.0, 1.0);
    let rise = t * t * (3.0 - 2.0 * t);
    (
        2.0 * shadow.radius * (1.0 - 0.5 * rise),
        shadow.opacity.clamp(0.0, 1.0) * (1.0 - 0.75 * rise),
    )
}

/// Opacity in the decal's `MeshTag` (0..255).
pub fn pack_opacity(opacity: f32) -> u32 {
    (opacity.clamp(0.0, 1.0) * 255.0).round() as u32
}

/// The decal's transform for an owner at `owner` over ground `hit` (point,
/// normal).
pub fn decal_transform(shadow: &BlobShadow, owner: Vec3, hit: (Vec3, Vec3)) -> (Transform, f32) {
    let (point, normal) = hit;
    let normal = normal.normalize_or(Vec3::Y);
    let (diameter, opacity) = blob_footprint(shadow, owner.y - point.y);
    (
        Transform::from_translation(point + normal * DECAL_LIFT)
            .with_rotation(Quat::from_rotation_arc(Vec3::Y, normal))
            .with_scale(Vec3::new(diameter, 1.0, diameter)),
        opacity,
    )
}

pub(crate) fn spawn_blob_decals(
    mut commands: Commands,
    assets: Option<Res<BlobAssets>>,
    owners: Query<Entity, (With<BlobShadow>, Without<BlobDecalLink>)>,
) {
    let Some(assets) = assets else {
        return;
    };
    for owner in &owners {
        let decal = commands
            .spawn((
                Name::new("Blob shadow"),
                BlobDecal { owner },
                BlobGround::default(),
                Mesh3d(assets.quad.clone()),
                MeshMaterial3d(assets.material.clone()),
                MeshTag(0),
                Transform::from_scale(Vec3::ZERO),
                Visibility::Hidden,
                NoFrustumCulling,
                NotShadowCaster,
                NoOutline,
            ))
            .id();
        commands.entity(owner).insert(BlobDecalLink(decal));
    }
}

pub(crate) fn despawn_blob_decal(
    remove: On<Remove, BlobShadow>,
    links: Query<&BlobDecalLink>,
    mut commands: Commands,
) {
    if let Ok(link) = links.get(remove.entity) {
        commands.entity(link.0).try_despawn();
        commands.entity(remove.entity).try_remove::<BlobDecalLink>();
    }
}

/// Finds the ground under owners that moved (or are new). Runs after transform
/// propagation.
pub(crate) fn probe_blob_ground(
    spatial: SpatialQuery,
    owners: Query<
        (Entity, &GlobalTransform, &BlobDecalLink),
        Or<(
            Changed<GlobalTransform>,
            Added<BlobDecalLink>,
            Changed<BlobShadow>,
        )>,
    >,
    mut grounds: Query<&mut BlobGround>,
) {
    let filter = SpatialQueryFilter::from_mask([Layer::World, Layer::Piece]);
    for (owner, transform, link) in &owners {
        let Ok(mut ground) = grounds.get_mut(link.0) else {
            continue;
        };
        let origin = transform.translation() + Vec3::Y * PROBE_LIFT;
        // Not solid: a ray starting inside a prop's own collider reports where
        // it leaves it (its base), not a hit at the ray origin.
        let hit = spatial
            .cast_ray(
                origin,
                Dir3::NEG_Y,
                PROBE_DEPTH,
                false,
                &filter.clone().with_excluded_entities([owner]),
            )
            .map(|hit| (origin - Vec3::Y * hit.distance, hit.normal));
        ground.set_if_neq(BlobGround(hit));
    }
}

/// Places, scales and shows each decal. Runs after transform propagation and
/// writes the decal's `GlobalTransform` itself (decals have no parent), before
/// visibility is checked.
pub(crate) fn place_blob_decals(
    settings: Res<super::LookSettings>,
    owners: Query<(&BlobShadow, &GlobalTransform, &InheritedVisibility), Without<BlobDecal>>,
    mut decals: Query<
        (
            &BlobDecal,
            &BlobGround,
            &mut Transform,
            &mut GlobalTransform,
            &mut MeshTag,
            &mut Visibility,
        ),
        Without<BlobShadow>,
    >,
) {
    for (decal, ground, mut transform, mut global, mut tag, mut visibility) in &mut decals {
        let Ok((shadow, owner, owner_visible)) = owners.get(decal.owner) else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        let show = settings.blobs && owner_visible.get() && ground.0.is_some();
        visibility.set_if_neq(if show {
            Visibility::Visible
        } else {
            Visibility::Hidden
        });
        let Some(hit) = ground.0 else {
            continue;
        };
        let (placed, opacity) = decal_transform(shadow, owner.translation(), hit);
        if *transform != placed {
            *transform = placed;
            *global = GlobalTransform::from(placed);
        }
        tag.set_if_neq(MeshTag(pack_opacity(opacity)));
    }
}

/// Blob systems in order: spawn, probe, place.
pub(crate) fn add_blob_systems(app: &mut App) {
    app.add_systems(
        PostUpdate,
        spawn_blob_decals.before(TransformSystems::Propagate),
    )
    .add_systems(
        PostUpdate,
        (probe_blob_ground, place_blob_decals)
            .chain()
            .after(TransformSystems::Propagate)
            .before(VisibilitySystems::CheckVisibility),
    )
    .add_observer(despawn_blob_decal);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_shrinks_and_fades_as_the_owner_rises() {
        let shadow = BlobShadow::new(0.5);
        let (d0, o0) = blob_footprint(&shadow, 0.0);
        assert_eq!(d0, 1.0);
        assert_eq!(o0, shadow.opacity);
        let (d1, o1) = blob_footprint(&shadow, 1.2);
        assert!(d1 < d0 && o1 < o0);
        let (d3, o3) = blob_footprint(&shadow, FADE_HEIGHT);
        assert!((d3 - 0.5).abs() < 1e-5 && o3 > 0.0 && o3 < o1);
        assert_eq!(blob_footprint(&shadow, 50.0), (d3, o3));
        // Sinking into the ground doesn't grow it.
        assert_eq!(blob_footprint(&shadow, -0.3), (d0, o0));
    }

    #[test]
    fn decal_lies_on_the_surface_below() {
        let shadow = BlobShadow::new(0.4);
        // Standing on a floor piece three meters up.
        let floor = (Vec3::new(4.0, 3.0, -2.0), Vec3::Y);
        let (t, opacity) = decal_transform(&shadow, Vec3::new(4.0, 3.0, -2.0), floor);
        assert!((t.translation - Vec3::new(4.0, 3.0 + DECAL_LIFT, -2.0)).length() < 1e-5);
        assert_eq!(t.scale, Vec3::new(0.8, 1.0, 0.8));
        assert_eq!(opacity, shadow.opacity);
        // On a ramp the decal tilts with the surface.
        let n = Vec3::new(0.0, 1.0, 1.0).normalize();
        let (t, _) = decal_transform(
            &shadow,
            Vec3::new(0.0, 1.5, 0.0),
            (Vec3::new(0.0, 1.5, 0.0), n),
        );
        assert!((t.rotation * Vec3::Y - n).length() < 1e-5);
        assert_eq!(pack_opacity(1.0), 255);
        assert_eq!(pack_opacity(0.0), 0);
    }

    fn blob_app() -> App {
        let mut app = App::new();
        // No transform propagation: the test writes owners' GlobalTransforms.
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::mesh::MeshPlugin,
        ))
        .init_asset::<BlobMaterial>()
        .init_resource::<super::super::LookSettings>()
        .add_systems(Startup, create_blob_assets)
        .add_systems(Update, (spawn_blob_decals, place_blob_decals).chain())
        .add_observer(despawn_blob_decal);
        app
    }

    #[test]
    fn decal_follows_a_moving_owner_and_hides_with_it() {
        let mut app = blob_app();
        let owner = app
            .world_mut()
            .spawn((
                BlobShadow::new(0.5),
                Transform::from_xyz(1.0, 0.0, 2.0),
                Visibility::Visible,
            ))
            .id();
        app.update();
        let decal = app.world().get::<BlobDecalLink>(owner).unwrap().0;
        // No ground found yet: hidden.
        assert_eq!(
            app.world().get::<Visibility>(decal),
            Some(&Visibility::Hidden)
        );

        // The probe (physics) found flat ground at y = 0.
        app.world_mut().get_mut::<BlobGround>(decal).unwrap().0 =
            Some((Vec3::new(1.0, 0.0, 2.0), Vec3::Y));
        app.world_mut()
            .entity_mut(owner)
            .insert(InheritedVisibility::VISIBLE);
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(decal),
            Some(&Visibility::Visible)
        );
        let t = *app.world().get::<Transform>(decal).unwrap();
        assert!((t.translation.xz() - Vec2::new(1.0, 2.0)).length() < 1e-5);

        // A static prop: same ground, nothing changes. A jump: smaller, fainter.
        *app.world_mut().get_mut::<GlobalTransform>(owner).unwrap() =
            GlobalTransform::from_xyz(1.0, 1.5, 2.0);
        let before = app.world().get::<MeshTag>(decal).unwrap().0;
        app.update();
        let after = app.world().get::<MeshTag>(decal).unwrap().0;
        assert!(after < before, "fainter in the air: {before} → {after}");

        // Owner hidden (a downed character): shadow hidden.
        app.world_mut()
            .entity_mut(owner)
            .insert(InheritedVisibility::HIDDEN);
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(decal),
            Some(&Visibility::Hidden)
        );

        // Owner gone: decal gone.
        app.world_mut().entity_mut(owner).despawn();
        app.update();
        assert!(app.world().get_entity(decal).is_err());
    }
}
