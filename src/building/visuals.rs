//! Client-only presentation of building (targets R4-M1, T09):
//!
//! - Pieces are drawn with their Blender models (`art/blender/assets/pieces.py`):
//!   a chunky brick wall and warped plank floors and ramps, toon-shaded with an
//!   ink [`Outline`]. Every piece of one kind and crack stage shares one mesh,
//!   and every piece shares one material, so pieces batch.
//! - Cracking swaps the model (66% HP: cartoon cracks; 33%: bigger cracks,
//!   missing bricks or split planks).
//! - A newly placed piece lands with a [`POP_SECONDS`] squash pop; a hit piece
//!   shudders.
//! - The build ghost is translucent and glowing: blue when the placement is
//!   valid, red when it isn't.
//! - [`PieceDebris`] hands the effects the brick chunk and plank splinter a
//!   broken piece bursts into.
//!
//! Piece meshes are taken from the model library once it has loaded (Boot
//! waits for them); the initial cover gets them like any other piece.

use super::{
    BuildTarget, InitialCover, Piece,
    mesh::{
        BRICK_DEBRIS, PIECE_MODELS, PLANK_DEBRIS, ghost_floor_mesh, ghost_ramp_mesh,
        ghost_wall_mesh, kind_index,
    },
};
use crate::{
    app::BootGate,
    look::{Outline, ToonMaterial, warmup::Warmup, with_outline_normals},
    models::{ModelLibrary, ModelsPlugin},
    palette::cartoon,
    shared::{ActiveTool, CELL_SIZE, DamageDealt, DamageTarget, Facing, LEVEL_HEIGHT, PieceKind, Player},
    tuning::Tuning,
};
use bevy::{light::NotShadowCaster, prelude::*, world_serialization::WorldAsset};
use std::f32::consts::PI;

pub use super::mesh::{model_mesh, model_offset, piece_model};

/// Client-only: piece models, crack stages, the pop, the ghost preview.
pub struct BuildingVisualsPlugin;

impl Plugin for BuildingVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (create_ghost_assets, spawn_ghost).chain())
            .add_systems(
                Update,
                (
                    load_piece_models.run_if(not(resource_exists::<PieceAssets>)),
                    attach_piece_visuals,
                    update_crack_visuals,
                    start_hit_shudder,
                    animate_piece_visuals,
                    update_ghost,
                )
                    .chain(),
            );
    }

    fn finish(&self, app: &mut App) {
        // Boot waits for the piece models only when there are models to wait for.
        if app.is_plugin_added::<ModelsPlugin>() {
            app.init_resource::<BootGate>();
            app.world_mut().resource_mut::<BootGate>().hold(PIECES_GATE);
        }
    }
}

/// The [`BootGate`] key held until the piece models are ready to draw.
pub const PIECES_GATE: &str = "pieces";
/// Seconds a newly placed piece takes to squash, spring up and settle.
pub const POP_SECONDS: f32 = 0.12;
/// Seconds a piece shudders after a hit.
const SHUDDER_SECONDS: f32 = 0.12;
/// East/west walls are lifted this much so their tops never share a plane with
/// north/south walls at a corner (no z-fighting when crack stages differ).
const EW_WALL_LIFT: f32 = 0.004;
/// Emissive strength of the ghost preview (× its own colour).
const GHOST_GLOW: f32 = 0.6;

/// The shared piece meshes (`[kind][stage]`, kind in [`kind_index`] order) and
/// the one material every piece uses.
#[derive(Resource, Debug, Clone)]
pub struct PieceAssets {
    pub meshes: [[Handle<Mesh>; 3]; 3],
    pub material: Handle<ToonMaterial>,
}

impl PieceAssets {
    pub fn mesh(&self, kind: PieceKind, stage: u8) -> &Handle<Mesh> {
        &self.meshes[kind_index(kind)][stage.min(2) as usize]
    }
}

/// The chunks a broken piece bursts into (the effects read this): Blender
/// models with their palette colours, drawn with one lit material.
#[derive(Resource, Debug, Clone)]
pub struct PieceDebris {
    pub brick: Handle<Mesh>,
    pub splinter: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
}

#[derive(Resource)]
struct GhostAssets {
    meshes: [Handle<Mesh>; 3],
    valid: Handle<ToonMaterial>,
    invalid: Handle<ToonMaterial>,
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

/// The ghost's material for a validity: translucent (the mesh's vertex alpha)
/// and glowing on both toon bands.
pub fn ghost_color(valid: bool) -> Color {
    if valid {
        cartoon::GHOST_BLUE
    } else {
        cartoon::GHOST_RED
    }
}

fn ghost_material(valid: bool) -> ToonMaterial {
    let color = ghost_color(valid);
    ToonMaterial::new(color)
        .with_emissive(color, GHOST_GLOW)
        .with_alpha(AlphaMode::Blend)
}

fn create_ghost_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ToonMaterial>>,
    mut warmup: Warmup,
    tuning: Res<Tuning>,
) {
    let meshes = [
        meshes.add(ghost_wall_mesh(tuning.building.wall_thickness).build()),
        meshes.add(ghost_floor_mesh(tuning.building.floor_thickness).build()),
        meshes.add(ghost_ramp_mesh().build()),
    ];
    let valid = materials.add(ghost_material(true));
    let invalid = materials.add(ghost_material(false));
    warmup.add(meshes[0].clone(), valid.clone());
    commands.insert_resource(GhostAssets {
        meshes,
        valid,
        invalid,
    });
}

fn spawn_ghost(mut commands: Commands, assets: Res<GhostAssets>) {
    commands.spawn((
        Name::new("Build ghost"),
        Ghost,
        Mesh3d(assets.meshes[0].clone()),
        MeshMaterial3d(assets.valid.clone()),
        Transform::default(),
        Visibility::Hidden,
        NotShadowCaster,
    ));
}

/// A plain box the size of a piece's collider, if its model is missing (so a
/// piece is never invisible).
fn fallback_mesh(kind: PieceKind) -> Mesh {
    match kind {
        PieceKind::Wall => Cuboid::new(CELL_SIZE, LEVEL_HEIGHT, 0.2)
            .mesh()
            .build()
            .translated_by(Vec3::Y * LEVEL_HEIGHT / 2.0),
        PieceKind::Floor => Cuboid::new(CELL_SIZE, 0.2, CELL_SIZE).mesh().build(),
        PieceKind::Ramp => Cuboid::new(CELL_SIZE, 0.2, CELL_SIZE)
            .mesh()
            .build()
            .rotated_by(Quat::from_rotation_x((LEVEL_HEIGHT / CELL_SIZE).atan()))
            .translated_by(Vec3::Y * LEVEL_HEIGHT / 2.0),
    }
}

/// Once the model library has loaded: takes each piece model's mesh (with
/// outline normals) and the debris meshes, warms their pipelines behind the
/// loading screen and lets Boot go on.
fn load_piece_models(
    mut commands: Commands,
    library: Option<Res<ModelLibrary>>,
    scenes: Res<Assets<WorldAsset>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut toon: ResMut<Assets<ToonMaterial>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut warmup: Warmup,
) {
    let Some(library) = library.filter(|l| l.is_ready()) else {
        return;
    };
    let model = |name: &str, meshes: &Assets<Mesh>| -> Option<Mesh> {
        let mesh = model_mesh(scenes.get(&library.get(name)?.scene)?, meshes);
        if mesh.is_none() {
            error!("building: model {name} has no mesh");
        }
        mesh
    };
    let kinds = [PieceKind::Wall, PieceKind::Floor, PieceKind::Ramp];
    let piece_meshes = kinds.map(|kind| {
        PIECE_MODELS[kind_index(kind)].map(|name| {
            let mesh = model(name, &meshes).unwrap_or_else(|| fallback_mesh(kind));
            meshes.add(with_outline_normals(mesh))
        })
    });
    let assets = PieceAssets {
        meshes: piece_meshes,
        material: toon.add(ToonMaterial::vertex_colored()),
    };
    let chunk = |mesh: Option<Mesh>| mesh.unwrap_or_else(|| Cuboid::from_length(0.25).mesh().build());
    let brick = chunk(model(BRICK_DEBRIS, &meshes));
    let splinter = chunk(model(PLANK_DEBRIS, &meshes));
    let debris = PieceDebris {
        brick: meshes.add(brick),
        splinter: meshes.add(splinter),
        material: standard.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.9,
            ..default()
        }),
    };
    warmup.add_with(
        assets.meshes[0][0].clone(),
        assets.material.clone(),
        Outline::default(),
    );
    warmup.add(debris.brick.clone(), debris.material.clone());
    warmup.gate().release(PIECES_GATE);
    commands.insert_resource(assets);
    commands.insert_resource(debris);
}

/// Gives every piece without a visual its shared mesh (new pieces pop in).
fn attach_piece_visuals(
    mut commands: Commands,
    assets: Option<Res<PieceAssets>>,
    pieces: Query<(Entity, &Piece, Has<InitialCover>), Without<PieceVisualLink>>,
) {
    let Some(assets) = assets else {
        return;
    };
    for (entity, piece, initial) in &pieces {
        let stage = piece.crack_stage.min(2);
        let lift = match (piece.kind, piece.facing) {
            (PieceKind::Wall, Facing::East | Facing::West) => EW_WALL_LIFT,
            _ => 0.0,
        };
        let base = model_offset(piece.kind) + Vec3::Y * lift;
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
                Mesh3d(assets.mesh(piece.kind, stage).clone()),
                MeshMaterial3d(assets.material.clone()),
                Outline::default(),
                Transform::from_translation(base).with_scale(if pop.is_some() {
                    pop_scale(0.0)
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
    assets: Option<Res<PieceAssets>>,
    pieces: Query<(&Piece, &PieceVisualLink), Changed<Piece>>,
    mut visuals: Query<(&mut PieceVisual, &mut Mesh3d)>,
) {
    let Some(assets) = assets else {
        return;
    };
    for (piece, link) in &pieces {
        let stage = piece.crack_stage.min(2);
        if let Ok((mut visual, mut mesh)) = visuals.get_mut(link.0)
            && visual.stage != stage
        {
            visual.stage = stage;
            mesh.0 = assets.mesh(piece.kind, stage).clone();
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

/// A landing piece's scale `t` seconds after placement: squashed flat and
/// wide, springing up past its height, settling at exactly 1 after
/// [`POP_SECONDS`]. Pieces scale about their model origin, which is on the
/// ground for walls and ramps, so they squash onto what they stand on.
pub fn pop_scale(t: f32) -> Vec3 {
    let x = (t / POP_SECONDS).clamp(0.0, 1.0);
    if x >= 1.0 {
        return Vec3::ONE;
    }
    let a = -0.42 * (1.0 - x) * (1.0 - x) * (3.0 * PI * x).cos();
    Vec3::new(1.0 - 0.5 * a, 1.0 + a, 1.0 - 0.5 * a)
}

fn animate_piece_visuals(time: Res<Time>, mut visuals: Query<(&mut PieceVisual, &mut Transform)>) {
    let dt = time.delta_secs();
    for (mut visual, mut transform) in &mut visuals {
        if visual.pop.is_none() && visual.shudder.is_none() {
            continue;
        }
        let mut scale = Vec3::ONE;
        let mut offset = Vec3::ZERO;
        if let Some(t) = visual.pop.as_mut() {
            *t += dt;
            scale = pop_scale(*t);
            if *t >= POP_SECONDS {
                visual.pop = None;
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
        transform.scale = scale;
    }
}

fn update_ghost(
    assets: Option<Res<GhostAssets>>,
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
    let (Some(assets), Some(ghost)) = (assets, ghost) else {
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
    let wanted_mesh = &assets.meshes[kind_index(candidate.slot.kind)];
    if mesh.0 != *wanted_mesh {
        mesh.0 = wanted_mesh.clone();
    }
    let wanted = if candidate.is_valid() {
        &assets.valid
    } else {
        &assets.invalid
    };
    if material.0 != *wanted {
        material.0 = wanted.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pop_squashes_springs_and_settles_in_0_12_s() {
        let start = pop_scale(0.0);
        assert!(start.y < 0.7 && start.x > 1.1, "lands squashed flat: {start}");
        // Springs up past full height partway through.
        let peak = (1..12)
            .map(|k| pop_scale(k as f32 * 0.01).y)
            .fold(0.0, f32::max);
        assert!(peak > 1.1, "overshoots: {peak}");
        assert_eq!(pop_scale(POP_SECONDS), Vec3::ONE);
        assert_eq!(pop_scale(1.0), Vec3::ONE);
        // Continuous: no frame-to-frame jump bigger than a 60 Hz step allows.
        let mut last = pop_scale(0.0);
        for k in 1..=120 {
            let s = pop_scale(k as f32 * 0.001);
            assert!((s - last).length() < 0.05, "jump at {k} ms");
            last = s;
        }
    }

    #[test]
    fn the_ghost_is_blue_when_valid_and_red_when_not() {
        let valid = ghost_material(true);
        let invalid = ghost_material(false);
        let (b, r) = (
            valid.base_color.to_srgba(),
            invalid.base_color.to_srgba(),
        );
        assert!(b.blue > b.red && b.blue > b.green, "valid is blue: {b:?}");
        assert!(r.red > r.blue && r.red > r.green, "invalid is red: {r:?}");
        for m in [&valid, &invalid] {
            assert_eq!(m.alpha_mode, AlphaMode::Blend, "translucent");
            assert!(m.emissive_strength > 0.0, "glowing");
        }
    }
}
