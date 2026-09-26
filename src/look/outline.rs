//! Ink outlines behind one component, [`Outline`], with two interchangeable
//! backends (see [`OutlineBackend`]):
//!
//! - **Hull** (default): a child entity draws the same mesh again with
//!   [`InkMaterial`], front faces culled, each vertex pushed out in clip space
//!   along its position-averaged smooth normal ([`ATTRIBUTE_OUTLINE_NORMAL`]),
//!   so the width is a constant number of pixels. It draws in the main opaque
//!   pass: anti-aliased by the camera's MSAA, occluded by everything, one extra
//!   (batched) draw per outlined mesh and no extra passes.
//! - **Mod**: `bevy_mod_outline` 0.13 with `OutlineMode::ExtrudeReal`, using the
//!   same outline-normal attribute. For measurement only (`--knobs outline=mod`).
//!
//! Why the hull is the default (from the crate source and offscreen renders of
//! both, `tests/look_offscreen.rs`): bevy_mod_outline draws after tonemapping in
//! its own passes with its own depth, so geometry without a stencil (the
//! ground, effects, the ghost) doesn't hide outlines; on the arena, outlines of
//! cliff-apron rubble showed through the ground. Its plugin adds a full-screen
//! MSAA write-back blit on every MSAA camera, outlines or not (both of ours);
//! it reads meshes on the CPU (render-world-only meshes panic); it fades per
//! entity, which can't fade a merged scenery mesh; and it ignores the miter, so
//! box edges draw thinner.
//!
//! Width is [`DEFAULT_WIDTH_PX`] at [`REFERENCE_HEIGHT_PX`] and scales with the
//! render target's height, so it stays constant on screen at any render scale.
//! On the hull, each vertex extrudes by width × its outline normal's miter, so
//! hard low-poly edges draw as thick as smooth curves. It fades to zero between
//! [`FADE_START`] and [`FADE_END`] meters (per vertex in the hull shader; per
//! entity for `Mod`), and hull draws beyond the fade are culled on the CPU with
//! an abrupt [`VisibilityRange`].

use super::{
    settings::{LookSettings, OutlineBackend},
    toon::ToonMaterial,
};
use crate::render::{MainCamera, VIEWMODEL_LAYER, WorldTarget};
use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::{RenderLayers, VisibilityRange},
    light::NotShadowCaster,
    mesh::{Indices, MeshVertexBufferLayoutRef, VertexAttributeValues},
    pbr::{MaterialPipeline, MaterialPipelineKey},
    platform::collections::HashMap,
    prelude::*,
    render::render_resource::{
        AsBindGroup, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
};
use bevy_mod_outline::{OutlineMode, OutlineStencil, OutlineVolume};

/// The vertex attribute both backends extrude along: `bevy_mod_outline`'s own,
/// filled by [`with_outline_normals`].
pub use bevy_mod_outline::ATTRIBUTE_OUTLINE_NORMAL;

pub const INK_SHADER_PATH: &str = "embedded://pieced/shaders/ink.wgsl";

/// Outline width at the reference resolution, in pixels.
pub const DEFAULT_WIDTH_PX: f32 = 1.5;
/// Height of the reference render target: the Battery preset's 1.4 MP cap on
/// the 15" Air's default 1710×1107 window (1470×956).
pub const REFERENCE_HEIGHT_PX: f32 = 956.0;
/// Outlines start fading here (meters from the camera)...
pub const FADE_START: f32 = 25.0;
/// ...and are gone here.
pub const FADE_END: f32 = 40.0;

/// Ink outline on a mesh entity (one with `Mesh3d`). Works on any render layer,
/// including the viewmodel's: the outline copies the entity's `RenderLayers`.
///
/// `color: None` derives the ink from the surface: a dark, desaturated version
/// of the base color (per vertex on the hull backend, so vertex-colored meshes
/// get matching ink per part).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Outline {
    pub width_px: f32,
    pub color: Option<Color>,
}

impl Default for Outline {
    fn default() -> Self {
        Self {
            width_px: DEFAULT_WIDTH_PX,
            color: None,
        }
    }
}

impl Outline {
    /// A fixed ink color at the default width.
    pub fn ink(color: Color) -> Self {
        Self {
            color: Some(color),
            ..default()
        }
    }

    pub fn with_width(mut self, width_px: f32) -> Self {
        self.width_px = width_px;
        self
    }
}

/// Outline width in target pixels for an outline of `width_px` (at the
/// reference resolution) seen from `distance` meters on a target
/// `target_height_px` tall. Mirrors `ink.wgsl`.
pub fn outline_width_px(width_px: f32, distance: f32, target_height_px: f32) -> f32 {
    let t = ((distance - FADE_START) / (FADE_END - FADE_START)).clamp(0.0, 1.0);
    let fade = 1.0 - t * t * (3.0 - 2.0 * t);
    width_px * (target_height_px / REFERENCE_HEIGHT_PX) * fade
}

/// How much of the color's saturation the ink keeps.
pub const INK_SATURATION: f32 = 0.6;
/// Ink = desaturated color × this...
pub const INK_DARKNESS: f32 = 0.1;
/// ...but never brighter than this linear luminance (white gloves get dark
/// grey ink, not mid grey).
pub const INK_MAX_LUMINANCE: f32 = 0.02;

fn luminance(c: Vec3) -> f32 {
    c.dot(Vec3::new(0.2126, 0.7152, 0.0722))
}

/// The default ink for a surface color: dark and desaturated, never pure black.
/// Mirrors `ink.wgsl`.
pub fn ink_color(base: Color) -> Color {
    let l = base.to_linear();
    let c = Vec3::new(l.red, l.green, l.blue);
    let desat = Vec3::splat(luminance(c)).lerp(c, INK_SATURATION);
    let k = INK_DARKNESS.min(INK_MAX_LUMINANCE / luminance(desat).max(1e-5));
    let ink = desat * k;
    Color::linear_rgb(ink.x, ink.y, ink.z)
}

// ---------------------------------------------------------------------------
// Smooth outline normals
// ---------------------------------------------------------------------------

/// Positions closer than this (meters) count as the same point.
const WELD: f32 = 1e-4;
/// Longest miter an outline normal may carry (see [`smooth_outline_normals`]).
pub const MAX_MITER: f32 = 2.0;

/// Outline normals averaged by vertex *position*: every vertex at the same
/// point gets the same direction, the angle-weighted mean of the face normals
/// around that point. Unlike `Mesh::compute_smooth_normals`, this welds split
/// vertices (hard edges, flat shading), so an extruded hull stays closed at
/// every corner.
///
/// The vector's *length* is the miter: 1 / the smallest cosine between that
/// direction and the faces meeting there (at most [`MAX_MITER`]). Extruding by
/// width × length pushes every adjacent face out by the same width, so a hard
/// 90° edge draws as thick a line as a smooth curve (√2 on a box edge, √3 on a
/// box corner, 1 on smooth surfaces). The hull shader uses it; bevy_mod_outline
/// ignores the length.
///
/// `normals` (optional) only orients faces whose winding is ambiguous;
/// `indices` is `None` for non-indexed triangle lists.
pub fn smooth_outline_normals(
    positions: &[[f32; 3]],
    normals: Option<&[[f32; 3]]>,
    indices: Option<&[u32]>,
) -> Vec<[f32; 3]> {
    let key = |p: [f32; 3]| {
        let q = |v: f32| (v / WELD).round() as i64;
        (q(p[0]), q(p[1]), q(p[2]))
    };
    let tri_count = indices.map_or(positions.len(), <[u32]>::len) / 3;
    let corner =
        |t: usize, k: usize| -> usize { indices.map_or(t * 3 + k, |ix| ix[t * 3 + k] as usize) };
    // Every valid triangle's corners and oriented unit face normal.
    let faces: Vec<([usize; 3], Vec3)> = (0..tri_count)
        .filter_map(|t| {
            let ids = [corner(t, 0), corner(t, 1), corner(t, 2)];
            if ids.iter().any(|&i| i >= positions.len()) {
                return None;
            }
            let p = ids.map(|i| Vec3::from_array(positions[i]));
            let face = (p[1] - p[0]).cross(p[2] - p[0]);
            if face.length_squared() < 1e-14 {
                return None;
            }
            let mut face = face.normalize();
            if let Some(normals) = normals {
                let hint: Vec3 = ids.iter().map(|&i| Vec3::from_array(normals[i])).sum();
                if hint.dot(face) < 0.0 {
                    face = -face;
                }
            }
            Some((ids, face))
        })
        .collect();
    let mut sums: HashMap<(i64, i64, i64), Vec3> = HashMap::default();
    for (ids, face) in &faces {
        let p = ids.map(|i| Vec3::from_array(positions[i]));
        for k in 0..3 {
            let angle = (p[(k + 1) % 3] - p[k]).angle_between(p[(k + 2) % 3] - p[k]);
            if angle.is_finite() {
                *sums.entry(key(positions[ids[k]])).or_default() += *face * angle;
            }
        }
    }
    let directions: HashMap<(i64, i64, i64), Vec3> = sums
        .into_iter()
        .filter_map(|(k, sum)| sum.try_normalize().map(|n| (k, n)))
        .collect();
    let mut min_cos: HashMap<(i64, i64, i64), f32> = HashMap::default();
    for (ids, face) in &faces {
        for &i in ids {
            let k = key(positions[i]);
            if let Some(n) = directions.get(&k) {
                let c = min_cos.entry(k).or_insert(1.0);
                *c = c.min(n.dot(*face));
            }
        }
    }
    positions
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let k = key(*p);
            match directions.get(&k) {
                Some(n) => {
                    let cos = min_cos.get(&k).copied().unwrap_or(1.0);
                    let miter = (1.0 / cos.max(1.0 / MAX_MITER)).clamp(1.0, MAX_MITER);
                    (*n * miter).to_array()
                }
                None => normals
                    .map_or(Vec3::Y, |n| Vec3::from_array(n[i]))
                    .normalize_or(Vec3::Y)
                    .to_array(),
            }
        })
        .collect()
}

/// Adds [`ATTRIBUTE_OUTLINE_NORMAL`] to a triangle-list mesh. Call it before
/// adding the mesh to `Assets<Mesh>` (our builders make render-world-only
/// meshes, whose data is gone after upload). The mesh also keeps its main-world
/// copy: `bevy_mod_outline` reads it on the CPU every frame (and panics on a
/// render-world-only mesh); for our outlined meshes that is a few MB in all.
/// Meshes without positions or with other topologies are returned as-is.
pub fn with_outline_normals(mut mesh: Mesh) -> Mesh {
    if mesh.primitive_topology() != bevy::mesh::PrimitiveTopology::TriangleList {
        return mesh;
    }
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return mesh;
    };
    let normals = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(VertexAttributeValues::Float32x3(n)) => Some(n.as_slice()),
        _ => None,
    };
    let indices: Option<Vec<u32>> = mesh.indices().map(|ix| match ix {
        Indices::U16(v) => v.iter().map(|&i| i as u32).collect(),
        Indices::U32(v) => v.clone(),
    });
    let outline = smooth_outline_normals(positions, normals, indices.as_deref());
    mesh.insert_attribute(ATTRIBUTE_OUTLINE_NORMAL, outline);
    mesh.asset_usage |= RenderAssetUsages::MAIN_WORLD;
    mesh
}

// ---------------------------------------------------------------------------
// Hull backend
// ---------------------------------------------------------------------------

/// The hull's unlit ink material (front faces culled, vertices extruded in the
/// vertex shader). One is shared per (width, ink) combination, so hulls batch.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone, PartialEq)]
#[uniform(0, InkUniform)]
pub struct InkMaterial {
    pub width_px: f32,
    /// The ink itself (`fixed`), or the base color the ink is derived from
    /// (times the vertex color) when not `fixed`.
    pub color: Color,
    pub fixed: bool,
}

#[derive(Clone, Copy, Default, ShaderType)]
pub struct InkUniform {
    /// rgb: ink or base color; w: 1 fixed, 0 derived.
    color: Vec4,
    /// x: width px at the reference height, y: fade start, z: fade end,
    /// w: reference height px.
    params: Vec4,
    /// x: saturation kept, y: darkness, z: max luminance.
    derive: Vec4,
}

impl From<&InkMaterial> for InkUniform {
    fn from(m: &InkMaterial) -> Self {
        let c = m.color.to_linear();
        Self {
            color: Vec4::new(c.red, c.green, c.blue, if m.fixed { 1.0 } else { 0.0 }),
            params: Vec4::new(m.width_px, FADE_START, FADE_END, REFERENCE_HEIGHT_PX),
            derive: Vec4::new(INK_SATURATION, INK_DARKNESS, INK_MAX_LUMINANCE, 0.0),
        }
    }
}

impl Material for InkMaterial {
    fn vertex_shader() -> ShaderRef {
        INK_SHADER_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        INK_SHADER_PATH.into()
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let mut attributes = vec![Mesh::ATTRIBUTE_POSITION.at_shader_location(0)];
        // Meshes without outline normals fall back to their own normals (which
        // crack at hard edges; prefer `with_outline_normals`).
        if layout.0.contains(ATTRIBUTE_OUTLINE_NORMAL) {
            attributes.push(ATTRIBUTE_OUTLINE_NORMAL.at_shader_location(1));
        } else {
            attributes.push(Mesh::ATTRIBUTE_NORMAL.at_shader_location(1));
        }
        let colors = layout.0.contains(Mesh::ATTRIBUTE_COLOR);
        if colors {
            attributes.push(Mesh::ATTRIBUTE_COLOR.at_shader_location(5));
        }
        descriptor.vertex.buffers = vec![layout.0.get_layout(&attributes)?];
        if colors {
            descriptor
                .vertex
                .shader_defs
                .push("INK_VERTEX_COLORS".into());
        }
        descriptor.primitive.cull_mode = Some(Face::Front);
        Ok(())
    }
}

/// On a hull child: the outlined entity it belongs to.
#[derive(Component, Debug, Clone, Copy)]
pub struct OutlineHull {
    pub owner: Entity,
}

/// On an outlined entity: its hull child.
#[derive(Component, Debug, Clone, Copy)]
pub struct OutlineHullLink(pub Entity);

/// Shared ink materials, keyed by (width bits, color bits, fixed).
#[derive(Resource, Debug, Default)]
pub(crate) struct InkMaterials(HashMap<(u32, [u32; 4], bool), Handle<InkMaterial>>);

impl InkMaterials {
    fn get(
        &mut self,
        assets: &mut Assets<InkMaterial>,
        material: InkMaterial,
    ) -> Handle<InkMaterial> {
        let c = material.color.to_linear();
        let key = (
            material.width_px.to_bits(),
            [c.red, c.green, c.blue, c.alpha].map(f32::to_bits),
            material.fixed,
        );
        self.0
            .entry(key)
            .or_insert_with(|| assets.add(material))
            .clone()
    }
}

/// The hull material an outlined entity should use.
fn ink_for(
    outline: &Outline,
    surface: Option<&MeshMaterial3d<ToonMaterial>>,
    toon: &Assets<ToonMaterial>,
) -> InkMaterial {
    match outline.color {
        Some(color) => InkMaterial {
            width_px: outline.width_px,
            color,
            fixed: true,
        },
        None => InkMaterial {
            width_px: outline.width_px,
            color: surface
                .and_then(|m| toon.get(&m.0))
                .map_or(Color::WHITE, |m| m.base_color),
            fixed: false,
        },
    }
}

fn hull_visibility(backend: OutlineBackend) -> Visibility {
    if backend == OutlineBackend::Hull {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    }
}

/// Hulls are culled past the fade (their width is zero there anyway).
fn hull_range() -> VisibilityRange {
    VisibilityRange::abrupt(0.0, FADE_END + 2.0)
}

pub(crate) fn spawn_outline_hulls(
    mut commands: Commands,
    settings: Res<LookSettings>,
    mut inks: ResMut<InkMaterials>,
    mut ink_assets: ResMut<Assets<InkMaterial>>,
    toon: Res<Assets<ToonMaterial>>,
    outlined: Query<
        (
            Entity,
            &Outline,
            &Mesh3d,
            Option<&RenderLayers>,
            Option<&MeshMaterial3d<ToonMaterial>>,
        ),
        Without<OutlineHullLink>,
    >,
) {
    for (entity, outline, mesh, layers, surface) in &outlined {
        let ink = inks.get(&mut ink_assets, ink_for(outline, surface, &toon));
        let mut hull = commands.spawn((
            Name::new("Outline hull"),
            OutlineHull { owner: entity },
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(ink),
            Transform::IDENTITY,
            hull_visibility(settings.outline),
            hull_range(),
            NotShadowCaster,
            NoOutline,
            ChildOf(entity),
        ));
        if let Some(layers) = layers {
            hull.insert(layers.clone());
        }
        let hull = hull.id();
        commands.entity(entity).insert(OutlineHullLink(hull));
    }
}

/// Keeps each hull on its owner's mesh, ink and render layers.
pub(crate) fn sync_outline_hulls(
    mut inks: ResMut<InkMaterials>,
    mut ink_assets: ResMut<Assets<InkMaterial>>,
    toon: Res<Assets<ToonMaterial>>,
    owners: Query<
        (
            &Outline,
            &Mesh3d,
            Option<&RenderLayers>,
            Option<&MeshMaterial3d<ToonMaterial>>,
            &OutlineHullLink,
        ),
        Or<(
            Changed<Outline>,
            Changed<Mesh3d>,
            Changed<RenderLayers>,
            Changed<MeshMaterial3d<ToonMaterial>>,
        )>,
    >,
    mut hulls: Query<
        (
            &mut Mesh3d,
            &mut MeshMaterial3d<InkMaterial>,
            Option<&mut RenderLayers>,
        ),
        (With<OutlineHull>, Without<Outline>),
    >,
    mut commands: Commands,
) {
    for (outline, mesh, layers, surface, link) in &owners {
        let Ok((mut hull_mesh, mut hull_ink, hull_layers)) = hulls.get_mut(link.0) else {
            continue;
        };
        if hull_mesh.0 != mesh.0 {
            hull_mesh.0 = mesh.0.clone();
        }
        let ink = inks.get(&mut ink_assets, ink_for(outline, surface, &toon));
        if hull_ink.0 != ink {
            hull_ink.0 = ink;
        }
        match (layers, hull_layers) {
            (Some(want), Some(mut have)) => {
                have.set_if_neq(want.clone());
            }
            (Some(want), None) => {
                commands.entity(link.0).insert(want.clone());
            }
            (None, Some(_)) => {
                commands.entity(link.0).remove::<RenderLayers>();
            }
            (None, None) => {}
        }
    }
}

pub(crate) fn remove_outline_hull(
    remove: On<Remove, Outline>,
    links: Query<&OutlineHullLink>,
    mut commands: Commands,
) {
    if let Ok(link) = links.get(remove.entity) {
        commands.entity(link.0).try_despawn();
        commands
            .entity(remove.entity)
            .try_remove::<(OutlineHullLink, OutlineVolume, OutlineStencil, OutlineMode)>();
    }
}

/// On a mesh entity whose [`Outline`] was copied from a mesh-less ancestor.
#[derive(Component, Debug)]
pub struct InheritedOutline;

/// Keeps a mesh out of outline propagation (hulls, halos, blob decals, and
/// anything else that must never be inked).
#[derive(Component, Debug, Default)]
pub struct NoOutline;

/// An [`Outline`] on an entity without a mesh (a glTF model root, say) outlines
/// every mesh below it. Meshes with their own `Outline` keep it.
pub(crate) fn propagate_outlines(
    roots: Query<(Entity, &Outline), Without<Mesh3d>>,
    children: Query<&Children>,
    meshes: Query<
        (Option<&Outline>, Has<InheritedOutline>),
        (With<Mesh3d>, Without<OutlineHull>, Without<NoOutline>),
    >,
    mut commands: Commands,
) {
    for (root, outline) in &roots {
        for entity in children.iter_descendants(root) {
            match meshes.get(entity) {
                Ok((None, _)) => {
                    commands.entity(entity).insert((*outline, InheritedOutline));
                }
                Ok((Some(current), true)) if current != outline => {
                    commands.entity(entity).insert(*outline);
                }
                _ => {}
            }
        }
    }
}

/// Shows hulls only on the hull backend; adds or removes `bevy_mod_outline`'s
/// components for the mod backend.
pub(crate) fn apply_outline_backend(
    settings: Res<LookSettings>,
    mut last: Local<Option<OutlineBackend>>,
    mut hulls: Query<&mut Visibility, With<OutlineHull>>,
    outlined: Query<Entity, With<Outline>>,
    fresh: Query<Entity, (With<Outline>, Without<OutlineVolume>)>,
    mut commands: Commands,
) {
    let backend = settings.outline;
    if *last != Some(backend) {
        for mut visibility in &mut hulls {
            visibility.set_if_neq(hull_visibility(backend));
        }
        if backend != OutlineBackend::Mod {
            for entity in &outlined {
                commands
                    .entity(entity)
                    .try_remove::<(OutlineVolume, OutlineStencil, OutlineMode)>();
            }
        }
        *last = Some(backend);
    }
    if backend == OutlineBackend::Mod {
        for entity in &fresh {
            commands.entity(entity).insert((
                OutlineVolume::default(),
                OutlineStencil::default(),
                OutlineMode::ExtrudeReal,
            ));
        }
    }
}

/// Mod backend: width, ink and distance fade per entity, written only on change.
pub(crate) fn update_mod_outlines(
    settings: Res<LookSettings>,
    target: Option<Res<WorldTarget>>,
    camera: Option<Single<&GlobalTransform, With<MainCamera>>>,
    toon: Res<Assets<ToonMaterial>>,
    mut outlined: Query<(
        &Outline,
        &GlobalTransform,
        Option<&RenderLayers>,
        Option<&MeshMaterial3d<ToonMaterial>>,
        &mut OutlineVolume,
    )>,
) {
    if settings.outline != OutlineBackend::Mod {
        return;
    }
    let height = target.map_or(REFERENCE_HEIGHT_PX, |t| t.size.y as f32);
    let eye = camera.map(|c| c.translation());
    for (outline, transform, layers, surface, mut volume) in &mut outlined {
        let viewmodel = layers.is_some_and(|l| l.intersects(&RenderLayers::layer(VIEWMODEL_LAYER)));
        let distance = match (viewmodel, eye) {
            (false, Some(eye)) => eye.distance(transform.translation()),
            _ => 0.0,
        };
        let width = outline_width_px(outline.width_px, distance, height);
        let colour = outline.color.unwrap_or_else(|| {
            ink_color(
                surface
                    .and_then(|m| toon.get(&m.0))
                    .map_or(Color::WHITE, |m| m.base_color),
            )
        });
        let visible = width > 0.01;
        if volume.visible != visible
            || (volume.width - width).abs() > 0.01
            || volume.colour != colour
        {
            *volume = OutlineVolume {
                visible,
                width,
                colour,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat-shaded cube: 6 faces × 4 split vertices, each face with its own
    /// normal, like every mesh our builders produce.
    fn flat_cube() -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<u32>) {
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut indices = Vec::new();
        for axis in 0..3 {
            for sign in [-1.0f32, 1.0] {
                let mut n = Vec3::ZERO;
                n[axis] = sign;
                let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
                let start = positions.len() as u32;
                for (su, sv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                    let mut p = n;
                    p[u] = su;
                    p[v] = sv;
                    positions.push(p.to_array());
                    normals.push(n.to_array());
                }
                // Wind counter-clockwise seen from outside: quad q0 q1 q2 q3,
                // split along q0–q2.
                let (a, b, c, d) = (start, start + 1, start + 2, start + 3);
                let at = |i: u32| Vec3::from_array(positions[i as usize]);
                let face = (at(b) - at(a)).cross(at(c) - at(a));
                let q = if face.dot(n) > 0.0 {
                    [a, b, c, d]
                } else {
                    [a, d, c, b]
                };
                indices.extend([q[0], q[1], q[2], q[0], q[2], q[3]]);
            }
        }
        (positions, normals, indices)
    }

    #[test]
    fn cube_corners_share_one_averaged_normal() {
        let (positions, normals, indices) = flat_cube();
        assert_eq!(positions.len(), 24);
        let outline = smooth_outline_normals(&positions, Some(&normals), Some(&indices));
        let mut corners: HashMap<[i32; 3], Vec3> = HashMap::default();
        for (p, n) in positions.iter().zip(&outline) {
            let n = Vec3::from_array(*n);
            let key = p.map(|v| v as i32);
            let expected = Vec3::from_array(*p).normalize();
            assert!(
                n.normalize().distance(expected) < 1e-5,
                "corner {p:?}: {n} instead of {expected}"
            );
            // Three perpendicular faces meet there: a √3 miter.
            assert!(
                (n.length() - 3f32.sqrt()).abs() < 1e-4,
                "miter {}",
                n.length()
            );
            if let Some(previous) = corners.insert(key, n) {
                assert_eq!(previous, n, "split vertices at {p:?} disagree");
            }
        }
        assert_eq!(corners.len(), 8, "8 corners, each with one normal");
    }

    #[test]
    fn welding_ignores_triangulation_and_indexing() {
        // The same cube as a non-indexed soup, with each face split along the
        // other diagonal: corner angles differ per triangle, the average doesn't.
        let (positions, normals, indices) = flat_cube();
        let mut soup_p = Vec::new();
        let mut soup_n = Vec::new();
        for quad in indices.chunks(6) {
            let (a, b, c, d) = (quad[0], quad[1], quad[2], quad[5]);
            for i in [a, b, d, b, c, d] {
                soup_p.push(positions[i as usize]);
                soup_n.push(normals[i as usize]);
            }
        }
        let outline = smooth_outline_normals(&soup_p, Some(&soup_n), None);
        let indexed = smooth_outline_normals(&positions, Some(&normals), Some(&indices));
        for (p, n) in soup_p.iter().zip(&outline) {
            let expected = Vec3::from_array(*p).normalize() * 3f32.sqrt();
            assert!(Vec3::from_array(*n).distance(expected) < 1e-4);
        }
        assert!(
            indexed
                .iter()
                .all(|n| (Vec3::from_array(*n).length() - 3f32.sqrt()).abs() < 1e-4)
        );
    }

    #[test]
    fn nearly_equal_positions_are_welded() {
        let (mut positions, normals, indices) = flat_cube();
        // Float noise from composing transforms: well under the weld size.
        for (i, p) in positions.iter_mut().enumerate() {
            p[0] += if i % 2 == 0 { 2e-6 } else { -2e-6 };
        }
        let outline = smooth_outline_normals(&positions, Some(&normals), Some(&indices));
        for (p, n) in positions.iter().zip(&outline) {
            let expected = Vec3::new(p[0].signum(), p[1].signum(), p[2].signum()).normalize();
            assert!(Vec3::from_array(*n).normalize().distance(expected) < 1e-4);
        }
    }

    #[test]
    fn miter_keeps_line_width_even_on_hard_edges() {
        // A long flat slab seen edge-on: along the long edges only two faces
        // meet (a 90° edge), so the miter is √2; on a smooth sphere it is ~1.
        let (positions, normals, indices) = flat_cube();
        let stretched: Vec<[f32; 3]> = positions.iter().map(|p| [p[0] * 4.0, p[1], p[2]]).collect();
        let outline = smooth_outline_normals(&stretched, Some(&normals), Some(&indices));
        assert!(outline.iter().all(|n| {
            let len = Vec3::from_array(*n).length();
            (1.0..=MAX_MITER).contains(&len)
        }));
        let mut sphere = Sphere::new(1.0).mesh().ico(3).unwrap();
        sphere.remove_attribute(Mesh::ATTRIBUTE_UV_0);
        let sphere = with_outline_normals(sphere);
        let Some(VertexAttributeValues::Float32x3(round)) =
            sphere.attribute(ATTRIBUTE_OUTLINE_NORMAL)
        else {
            panic!("no outline normals");
        };
        assert!(round.iter().all(|n| Vec3::from_array(*n).length() < 1.05));
        // A thin wedge can't produce a spike longer than the cap.
        let wedge = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 0.05, 0.0],
        ];
        for n in smooth_outline_normals(&wedge, None, None) {
            assert!(Vec3::from_array(n).length() <= MAX_MITER + 1e-5);
        }
    }

    #[test]
    fn mesh_gets_the_outline_attribute() {
        let (positions, normals, indices) = flat_cube();
        let mesh = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_indices(Indices::U32(indices));
        let mesh = with_outline_normals(mesh);
        let Some(VertexAttributeValues::Float32x3(outline)) =
            mesh.attribute(ATTRIBUTE_OUTLINE_NORMAL)
        else {
            panic!("no outline normals");
        };
        assert_eq!(outline.len(), 24);
        // bevy_mod_outline needs the CPU copy.
        assert!(mesh.asset_usage.contains(RenderAssetUsages::MAIN_WORLD));
        // The regular (flat) normals are untouched.
        let Some(VertexAttributeValues::Float32x3(flat)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            panic!("no normals");
        };
        assert!(
            flat.iter()
                .all(|n| n.iter().filter(|v| **v != 0.0).count() == 1)
        );
    }

    #[test]
    fn width_is_constant_near_and_fades_between_25_and_40_m() {
        let w = |d: f32| outline_width_px(DEFAULT_WIDTH_PX, d, REFERENCE_HEIGHT_PX);
        assert_eq!(w(0.5), DEFAULT_WIDTH_PX);
        assert_eq!(w(10.0), DEFAULT_WIDTH_PX);
        assert_eq!(w(FADE_START), DEFAULT_WIDTH_PX);
        assert!((w(32.5) - DEFAULT_WIDTH_PX * 0.5).abs() < 1e-5);
        assert!(w(30.0) > w(35.0));
        assert_eq!(w(FADE_END), 0.0);
        assert_eq!(w(80.0), 0.0);
        // Constant on screen: twice the pixels on a target twice as tall.
        let hi = outline_width_px(DEFAULT_WIDTH_PX, 10.0, REFERENCE_HEIGHT_PX * 2.0);
        assert!((hi - 2.0 * DEFAULT_WIDTH_PX).abs() < 1e-5);
        // Hulls are culled only after they have faded out.
        assert!(hull_range().is_visible_at_all(FADE_END - 0.1));
        assert!(hull_range().is_culled(FADE_END + 5.0));
    }

    #[test]
    fn ink_is_a_dark_desaturated_version_of_the_surface() {
        let brick = Color::srgb(0.78, 0.35, 0.2);
        let ink = ink_color(brick).to_srgba();
        let base = brick.to_srgba();
        assert!(ink.red < base.red * 0.5 && ink.green < base.green * 0.6);
        assert!(
            ink.red > ink.green && ink.green > ink.blue,
            "keeps the hue: {ink:?}"
        );
        let spread = |c: Srgba| c.red.max(c.green).max(c.blue) - c.red.min(c.green).min(c.blue);
        assert!(spread(ink) < spread(base), "less saturated");
        // White gloves get dark grey ink, not mid grey.
        let white = ink_color(Color::WHITE).to_srgba();
        assert!(white.red < 0.25 && white.red > 0.05, "{white:?}");
        let black = ink_color(Color::BLACK).to_srgba();
        assert!(black.red <= 0.01);
    }

    #[test]
    fn ink_materials_are_shared() {
        let mut assets = Assets::<InkMaterial>::default();
        let mut inks = InkMaterials::default();
        let a = inks.get(
            &mut assets,
            ink_for(&Outline::default(), None, &Assets::default()),
        );
        let b = inks.get(
            &mut assets,
            ink_for(&Outline::default(), None, &Assets::default()),
        );
        let c = inks.get(
            &mut assets,
            ink_for(&Outline::ink(Color::BLACK), None, &Assets::default()),
        );
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(assets.len(), 2);
    }

    fn hull_app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::mesh::MeshPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<InkMaterial>()
        .init_asset::<ToonMaterial>()
        .init_resource::<InkMaterials>()
        .init_resource::<LookSettings>()
        .add_systems(
            Update,
            (
                apply_outline_backend,
                spawn_outline_hulls,
                sync_outline_hulls,
            )
                .chain(),
        )
        .add_observer(remove_outline_hull);
        app
    }

    fn hull_of(app: &App, owner: Entity) -> Entity {
        app.world()
            .get::<OutlineHullLink>(owner)
            .expect("hull spawned")
            .0
    }

    #[test]
    fn hulls_follow_their_owner_on_any_layer() {
        let mut app = hull_app();
        let (mesh_a, mesh_b) = {
            let mut meshes = app.world_mut().resource_mut::<Assets<Mesh>>();
            (
                meshes.add(Cuboid::default()),
                meshes.add(Cuboid::new(2.0, 1.0, 1.0)),
            )
        };
        let brick = app
            .world_mut()
            .resource_mut::<Assets<ToonMaterial>>()
            .add(ToonMaterial::new(Color::srgb(0.78, 0.35, 0.2)));
        // A viewmodel part: its outline must render on the viewmodel layer.
        let gun = app
            .world_mut()
            .spawn((
                Mesh3d(mesh_a.clone()),
                MeshMaterial3d(brick.clone()),
                Outline::default(),
                RenderLayers::layer(VIEWMODEL_LAYER),
            ))
            .id();
        app.update();
        let hull = hull_of(&app, gun);
        let world = app.world();
        assert_eq!(world.get::<ChildOf>(hull).unwrap().parent(), gun);
        assert_eq!(world.get::<Mesh3d>(hull).unwrap().0, mesh_a);
        assert_eq!(
            world.get::<RenderLayers>(hull),
            Some(&RenderLayers::layer(VIEWMODEL_LAYER))
        );
        assert_eq!(world.get::<Visibility>(hull), Some(&Visibility::Inherited));
        let ink = world
            .get::<MeshMaterial3d<InkMaterial>>(hull)
            .unwrap()
            .0
            .clone();
        let ink = world.resource::<Assets<InkMaterial>>().get(&ink).unwrap();
        assert!(!ink.fixed, "ink derived from the surface");
        assert_eq!(ink.color, Color::srgb(0.78, 0.35, 0.2));

        // A crack stage swaps the mesh: the hull follows.
        app.world_mut().get_mut::<Mesh3d>(gun).unwrap().0 = mesh_b.clone();
        app.update();
        assert_eq!(app.world().get::<Mesh3d>(hull).unwrap().0, mesh_b);

        // Outlines off: hidden, not despawned (switching back is free).
        app.world_mut().resource_mut::<LookSettings>().outline = OutlineBackend::Off;
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(hull),
            Some(&Visibility::Hidden)
        );
        app.world_mut().resource_mut::<LookSettings>().outline = OutlineBackend::Hull;
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(hull),
            Some(&Visibility::Inherited)
        );

        // Removing the outline removes the hull.
        app.world_mut().entity_mut(gun).remove::<Outline>();
        app.update();
        assert!(app.world().get_entity(hull).is_err());
        assert!(app.world().get::<OutlineHullLink>(gun).is_none());
    }

    #[test]
    fn mod_backend_swaps_hulls_for_bevy_mod_outline_components() {
        let mut app = hull_app();
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::default());
        let piece = app
            .world_mut()
            .spawn((Mesh3d(mesh), Outline::ink(Color::BLACK)))
            .id();
        app.world_mut().resource_mut::<LookSettings>().outline = OutlineBackend::Mod;
        app.update();
        let hull = hull_of(&app, piece);
        assert_eq!(
            app.world().get::<Visibility>(hull),
            Some(&Visibility::Hidden)
        );
        assert!(app.world().get::<OutlineVolume>(piece).is_some());
        assert!(matches!(
            app.world().get::<OutlineMode>(piece),
            Some(OutlineMode::ExtrudeReal)
        ));
        app.world_mut().resource_mut::<LookSettings>().outline = OutlineBackend::Hull;
        app.update();
        assert!(app.world().get::<OutlineVolume>(piece).is_none());
        assert_eq!(
            app.world().get::<Visibility>(hull),
            Some(&Visibility::Inherited)
        );
    }
}
