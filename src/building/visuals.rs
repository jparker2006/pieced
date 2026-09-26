//! Client-only presentation of building: shared piece meshes per crack stage, a
//! quick pop when a piece lands, a small shudder when it's hit, and the
//! translucent ghost preview at the player's build target.
//!
//! Pieces are toon-shaded with ink outlines; the ghost is a translucent, glowing
//! toon surface without one. Every mesh is built once at startup; per frame this
//! only swaps handles and writes a few transforms.

use super::{
    BuildTarget, InitialCover, Piece,
    mesh::{
        MeshBuilder, floor_mesh, ghost_floor_mesh, ghost_ramp_mesh, ghost_wall_mesh, ramp_mesh,
        wall_mesh,
    },
};
use crate::{
    look::{Outline, ToonMaterial, with_outline_normals},
    palette,
    shared::{ActiveTool, DamageDealt, DamageTarget, Facing, PieceKind, Player},
    tuning::Tuning,
};
use bevy::{light::NotShadowCaster, prelude::*};

/// Client-only: piece meshes, crack visuals and the ghost preview.
pub struct BuildingVisualsPlugin;

impl Plugin for BuildingVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (create_piece_assets, spawn_ghost).chain())
            .add_systems(
                Update,
                (
                    attach_piece_visuals,
                    update_crack_visuals,
                    start_hit_shudder,
                    animate_piece_visuals,
                    update_ghost,
                )
                    .chain(),
            );
    }
}

/// Seconds a newly placed piece takes to pop into place.
const POP_SECONDS: f32 = 0.11;
/// Seconds a piece shudders after a hit.
const SHUDDER_SECONDS: f32 = 0.12;
/// Base-color multiplier per crack stage (on top of the vertex colors).
const STAGE_TINT: [f32; 3] = [1.0, 0.84, 0.68];
/// East/west walls are lifted this much so their tops never share a plane with
/// north/south walls at a corner (no z-fighting when crack stages differ).
const EW_WALL_LIFT: f32 = 0.004;
/// Emissive strength of the ghost preview (× its own color).
const GHOST_GLOW: f32 = 0.4;

#[derive(Resource)]
struct PieceAssets {
    /// `[kind][stage]`, kind in [`kind_index`] order.
    meshes: [[Handle<Mesh>; 3]; 3],
    materials: [Handle<ToonMaterial>; 3],
    ghost_meshes: [Handle<Mesh>; 3],
    ghost_valid: Handle<ToonMaterial>,
    ghost_invalid: Handle<ToonMaterial>,
}

fn kind_index(kind: PieceKind) -> usize {
    match kind {
        PieceKind::Wall => 0,
        PieceKind::Floor => 1,
        PieceKind::Ramp => 2,
    }
}

/// On a piece: its visual child.
#[derive(Component)]
struct PieceVisualLink(Entity);

/// On a piece's visual child.
#[derive(Component, Default)]
struct PieceVisual {
    stage: u8,
    base: Vec3,
    /// Seconds since placement, while popping in.
    pop: Option<f32>,
    /// Seconds since the latest hit, while shuddering.
    shudder: Option<f32>,
}

#[derive(Component)]
struct Ghost;

fn create_piece_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ToonMaterial>>,
    tuning: Res<Tuning>,
) {
    // Piece meshes carry smooth outline normals for their ink outline.
    let mut add = |m: MeshBuilder| meshes.add(with_outline_normals(m.build()));
    let per_stage = |f: fn(u8) -> MeshBuilder, add: &mut dyn FnMut(MeshBuilder) -> Handle<Mesh>| {
        [add(f(0)), add(f(1)), add(f(2))]
    };
    let wall = per_stage(wall_mesh, &mut add);
    let floor = per_stage(floor_mesh, &mut add);
    let ramp = per_stage(ramp_mesh, &mut add);
    let ghost_meshes = [
        add(ghost_wall_mesh(tuning.building.wall_thickness)),
        add(ghost_floor_mesh(tuning.building.floor_thickness)),
        add(ghost_ramp_mesh()),
    ];
    let piece_material = |tint: f32| ToonMaterial::new(Color::srgb(tint, tint, tint));
    // Translucency comes from the ghost mesh's vertex alpha; the emissive makes
    // it glow on both bands.
    let ghost_material = |color: Color| {
        let c = color.to_srgba();
        let rgb = Color::srgb(c.red, c.green, c.blue);
        ToonMaterial::new(rgb)
            .with_emissive(rgb, GHOST_GLOW)
            .with_alpha(AlphaMode::Blend)
    };
    commands.insert_resource(PieceAssets {
        meshes: [wall, floor, ramp],
        materials: STAGE_TINT.map(|t| materials.add(piece_material(t))),
        ghost_meshes,
        ghost_valid: materials.add(ghost_material(palette::GHOST_VALID)),
        ghost_invalid: materials.add(ghost_material(palette::GHOST_INVALID)),
    });
}

fn spawn_ghost(mut commands: Commands, assets: Res<PieceAssets>) {
    commands.spawn((
        Name::new("Build ghost"),
        Ghost,
        Mesh3d(assets.ghost_meshes[0].clone()),
        MeshMaterial3d(assets.ghost_valid.clone()),
        Transform::default(),
        Visibility::Hidden,
        NotShadowCaster,
    ));
}

/// Gives every piece without a visual its shared mesh (new pieces pop in).
fn attach_piece_visuals(
    mut commands: Commands,
    assets: Res<PieceAssets>,
    pieces: Query<(Entity, &Piece, Has<InitialCover>), Without<PieceVisualLink>>,
) {
    for (entity, piece, initial) in &pieces {
        let stage = piece.crack_stage.min(2);
        let lift = match (piece.kind, piece.facing) {
            (PieceKind::Wall, Facing::East | Facing::West) => EW_WALL_LIFT,
            _ => 0.0,
        };
        let base = Vec3::Y * lift;
        let pop = (!initial).then_some(0.0);
        let child = commands
            .spawn((
                Name::new("Piece visual"),
                PieceVisual {
                    stage,
                    base,
                    pop,
                    shudder: None,
                },
                Mesh3d(assets.meshes[kind_index(piece.kind)][stage as usize].clone()),
                MeshMaterial3d(assets.materials[stage as usize].clone()),
                Outline::default(),
                Transform::from_translation(base).with_scale(if pop.is_some() {
                    Vec3::splat(0.8)
                } else {
                    Vec3::ONE
                }),
                ChildOf(entity),
            ))
            .id();
        commands
            .entity(entity)
            .insert((PieceVisualLink(child), Visibility::default()));
    }
}

fn update_crack_visuals(
    assets: Res<PieceAssets>,
    pieces: Query<(&Piece, &PieceVisualLink), Changed<Piece>>,
    mut visuals: Query<(
        &mut PieceVisual,
        &mut Mesh3d,
        &mut MeshMaterial3d<ToonMaterial>,
    )>,
) {
    for (piece, link) in &pieces {
        let stage = piece.crack_stage.min(2);
        if let Ok((mut visual, mut mesh, mut material)) = visuals.get_mut(link.0)
            && visual.stage != stage
        {
            visual.stage = stage;
            mesh.0 = assets.meshes[kind_index(piece.kind)][stage as usize].clone();
            material.0 = assets.materials[stage as usize].clone();
        }
    }
}

fn start_hit_shudder(
    mut dealt: MessageReader<DamageDealt>,
    links: Query<&PieceVisualLink>,
    mut visuals: Query<&mut PieceVisual>,
) {
    for d in dealt.read() {
        if d.target_kind != DamageTarget::Piece || d.killed {
            continue;
        }
        if let Ok(link) = links.get(d.target)
            && let Ok(mut visual) = visuals.get_mut(link.0)
        {
            visual.shudder = Some(0.0);
        }
    }
}

fn animate_piece_visuals(time: Res<Time>, mut visuals: Query<(&mut PieceVisual, &mut Transform)>) {
    let dt = time.delta_secs();
    for (mut visual, mut transform) in &mut visuals {
        if visual.pop.is_none() && visual.shudder.is_none() {
            continue;
        }
        let mut scale = 1.0;
        let mut offset = Vec3::ZERO;
        if let Some(t) = visual.pop.as_mut() {
            *t += dt;
            let x = (*t / POP_SECONDS).min(1.0);
            // Ease-out-back from 0.8 with a small overshoot.
            let c = 1.9;
            let e = 1.0 + (c + 1.0) * (x - 1.0).powi(3) + c * (x - 1.0).powi(2);
            scale = 0.8 + 0.2 * e;
            if x >= 1.0 {
                visual.pop = None;
                scale = 1.0;
            }
        }
        if let Some(t) = visual.shudder.as_mut() {
            *t += dt;
            let x = (*t / SHUDDER_SECONDS).min(1.0);
            let amp = 0.035 * (1.0 - x) * (1.0 - x);
            offset = Vec3::new((*t * 95.0).sin() * amp, 0.0, (*t * 71.0).cos() * amp * 0.6);
            scale *= 1.0 - 0.02 * (1.0 - x);
            if x >= 1.0 {
                visual.shudder = None;
                offset = Vec3::ZERO;
            }
        }
        transform.translation = visual.base + offset;
        transform.scale = Vec3::splat(scale);
    }
}

fn update_ghost(
    assets: Res<PieceAssets>,
    player: Option<Single<(&ActiveTool, &BuildTarget), With<Player>>>,
    ghost: Option<
        Single<
            (
                &mut Transform,
                &mut Visibility,
                &mut Mesh3d,
                &mut MeshMaterial3d<ToonMaterial>,
            ),
            With<Ghost>,
        >,
    >,
) {
    let Some(ghost) = ghost else {
        return;
    };
    let (mut transform, mut visibility, mut mesh, mut material) = ghost.into_inner();
    let candidate = player.and_then(|p| {
        let (tool, target) = p.into_inner();
        tool.is_build().then_some(target.candidate).flatten()
    });
    let Some(candidate) = candidate else {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    visibility.set_if_neq(Visibility::Inherited);
    transform.set_if_neq(candidate.slot.transform());
    let wanted_mesh = &assets.ghost_meshes[kind_index(candidate.slot.kind)];
    if mesh.0 != *wanted_mesh {
        mesh.0 = wanted_mesh.clone();
    }
    let wanted = if candidate.is_valid() {
        &assets.ghost_valid
    } else {
        &assets.ghost_invalid
    };
    if material.0 != *wanted {
        material.0 = wanted.clone();
    }
}
