//! Client-only presentation of building (targets R4-M1, T09):
//!
//! - Pieces are drawn with their Blender models (`art/blender/assets/pieces.py`):
//!   a chunky brick wall and warped plank floors and ramps, toon-shaded with an
//!   ink [`Outline`]. Every piece of one kind and crack stage shares one mesh,
//!   and every piece shares one material, so pieces batch.
//! - Cracking swaps the model (66% HP: cartoon cracks; 33%: bigger cracks,
//!   missing bricks or split planks).
//! - An edited piece (D44) swaps to its edit's mesh at once: walls and floors
//!   are composed from the Blender tile sets (kept tiles plus frame boards
//!   round the opening), half ramps and cone roofs are one model turned to
//!   fit. Every edit's meshes are built once when the models load and warmed
//!   up, so editing never creates an asset.
//! - [`super::edit_grid`] draws the edit grid over the piece being edited.
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
    BuildTarget, InitialCover, Piece, PieceEdit,
    edit::{self},
    mesh::{
        BRICK_DEBRIS, CONE_ROOF_MODELS, KINDS, PIECE_MODELS, PLANK_DEBRIS, TILE_MODELS,
        compose_floor, compose_wall, cone_roof, ghost_cone_mesh, ghost_floor_mesh, ghost_ramp_mesh,
        ghost_wall_mesh, kind_index, model_part_meshes, ramp_half,
    },
};
use crate::{
    app::BootGate,
    look::{Outline, ToonMaterial, warmup::Warmup, with_outline_normals},
    models::{ModelLibrary, ModelsPlugin},
    palette::cartoon,
    shared::{
        ActiveTool, CELL_SIZE, DamageDealt, DamageTarget, Facing, LEVEL_HEIGHT, PieceKind, Player,
    },
    tuning::Tuning,
};
use bevy::{
    light::NotShadowCaster, platform::collections::HashMap, prelude::*,
    world_serialization::WorldAsset,
};
use std::{collections::BTreeMap, f32::consts::PI};

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
                    update_piece_meshes,
                    start_hit_shudder,
                    animate_piece_visuals,
                    update_ghost,
                )
                    .chain(),
            );
        super::edit_grid::build(app);
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

/// The shared piece meshes (`[kind][stage]`, kind in [`kind_index`] order),
/// every valid edit's meshes, and the one material every piece uses.
#[derive(Resource, Debug, Clone)]
pub struct PieceAssets {
    pub meshes: [[Handle<Mesh>; 3]; 4],
    pub edits: HashMap<(PieceKind, PieceEdit), EditVisual>,
    pub material: Handle<ToonMaterial>,
}

/// How an edited piece is drawn: a mesh per crack stage, turned about the
/// piece's +Y (half ramps and cone roofs are one model turned to fit).
#[derive(Debug, Clone, PartialEq)]
pub struct EditVisual {
    pub meshes: [Handle<Mesh>; 3],
    pub turn: Quat,
}

impl PieceAssets {
    pub fn mesh(&self, kind: PieceKind, stage: u8) -> &Handle<Mesh> {
        &self.meshes[kind_index(kind)][stage.min(2) as usize]
    }

    /// The mesh and turn a piece of `kind` with `edit` shows at crack `stage`
    /// (the full piece's model when unedited, or if the edit has no mesh).
    pub fn visual(&self, kind: PieceKind, edit: PieceEdit, stage: u8) -> (&Handle<Mesh>, Quat) {
        match self.edits.get(&(kind, edit)) {
            Some(v) if edit.is_edited() => (&v.meshes[stage.min(2) as usize], v.turn),
            _ => (self.mesh(kind, stage), Quat::IDENTITY),
        }
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
    meshes: [Handle<Mesh>; 4],
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
    edit: PieceEdit,
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
        meshes.add(ghost_cone_mesh().build()),
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

/// Adds three staged meshes (with outline normals) as assets.
trait StagedMeshes {
    fn into_staged_handles(self, meshes: &mut Assets<Mesh>) -> [Handle<Mesh>; 3];
}

impl StagedMeshes for Vec<Mesh> {
    fn into_staged_handles(self, meshes: &mut Assets<Mesh>) -> [Handle<Mesh>; 3] {
        let mut it = self.into_iter();
        std::array::from_fn(|_| {
            it.next()
                .map(|m| meshes.add(with_outline_normals(m)))
                .unwrap_or_default()
        })
    }
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
        PieceKind::Cone => ghost_cone_mesh().build(),
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
    let piece_meshes = KINDS.map(|kind| {
        PIECE_MODELS[kind_index(kind)].map(|name| {
            let mesh = model(name, &meshes).unwrap_or_else(|| fallback_mesh(kind));
            meshes.add(with_outline_normals(mesh))
        })
    });
    let parts = |name: &str, meshes: &Assets<Mesh>| -> BTreeMap<String, Mesh> {
        library
            .get(name)
            .and_then(|m| scenes.get(&m.scene))
            .map(|scene| model_part_meshes(scene, meshes))
            .unwrap_or_default()
    };
    let mut edits = HashMap::default();
    // Walls and floors: every valid edit composed from its tile set, per stage.
    for (kind, compose) in [
        (PieceKind::Wall, compose_wall as fn(_, &_) -> _),
        (PieceKind::Floor, compose_floor),
    ] {
        let sets = TILE_MODELS[kind_index(kind)].map(|name| parts(name, &meshes));
        for e in edit::valid_edits(kind) {
            let staged: Option<Vec<Mesh>> = sets.iter().map(|set| compose(e, set)).collect();
            let Some(staged) = staged else {
                error!("building: no mesh for {kind:?} edit {e:?}");
                continue;
            };
            let handles = staged.into_staged_handles(&mut meshes);
            edits.insert(
                (kind, e),
                EditVisual {
                    meshes: handles,
                    turn: Quat::IDENTITY,
                },
            );
        }
    }
    // Half ramps: the tile set's two halves, turned.
    let halves = TILE_MODELS[kind_index(PieceKind::Ramp)].map(|name| parts(name, &meshes));
    let mut half_handles: BTreeMap<&str, [Handle<Mesh>; 3]> = BTreeMap::new();
    for side in ["Left", "Right"] {
        let staged: Option<Vec<Mesh>> = halves.iter().map(|set| set.get(side).cloned()).collect();
        match staged {
            Some(staged) => {
                half_handles.insert(side, staged.into_staged_handles(&mut meshes));
            }
            None => error!("building: ramp tile part {side} is missing"),
        }
    }
    for e in edit::valid_edits(PieceKind::Ramp) {
        if let Some((side, turn)) = ramp_half(e)
            && let Some(handles) = half_handles.get(side)
        {
            let meshes = handles.clone();
            edits.insert((PieceKind::Ramp, e), EditVisual { meshes, turn });
        }
    }
    // Cone roofs: four models, turned.
    let roofs: Vec<Option<[Handle<Mesh>; 3]>> = CONE_ROOF_MODELS
        .iter()
        .map(|names| {
            let staged: Option<Vec<Mesh>> = names.iter().map(|n| model(n, &meshes)).collect();
            staged.map(|s| s.into_staged_handles(&mut meshes))
        })
        .collect();
    for e in edit::valid_edits(PieceKind::Cone) {
        if let Some((roof, turn)) = cone_roof(e)
            && let Some(Some(handles)) = roofs.get(roof as usize)
        {
            let meshes = handles.clone();
            edits.insert((PieceKind::Cone, e), EditVisual { meshes, turn });
        }
    }
    let assets = PieceAssets {
        meshes: piece_meshes,
        edits,
        material: toon.add(ToonMaterial::vertex_colored()),
    };
    let chunk =
        |mesh: Option<Mesh>| mesh.unwrap_or_else(|| Cuboid::from_length(0.25).mesh().build());
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
    // Every piece and edit mesh is drawn once behind the loading screen, so
    // the first cone or edit of a match never waits on an upload.
    let mut warm: Vec<Handle<Mesh>> = assets.meshes.iter().flatten().cloned().collect();
    for v in assets.edits.values() {
        for m in &v.meshes {
            if !warm.contains(m) {
                warm.push(m.clone());
            }
        }
    }
    for mesh in warm {
        warmup.add_with(mesh, assets.material.clone(), Outline::default());
    }
    warmup.add(debris.brick.clone(), debris.material.clone());
    warmup.gate().release(PIECES_GATE);
    commands.insert_resource(assets);
    commands.insert_resource(debris);
}

/// Gives every piece without a visual its shared mesh (new pieces pop in).
fn attach_piece_visuals(
    mut commands: Commands,
    assets: Option<Res<PieceAssets>>,
    pieces: Query<
        (Entity, &Piece, Option<&PieceEdit>, Has<InitialCover>),
        Without<PieceVisualLink>,
    >,
) {
    let Some(assets) = assets else {
        return;
    };
    for (entity, piece, edit, initial) in &pieces {
        let stage = piece.crack_stage.min(2);
        let edit = edit.copied().unwrap_or_default();
        let (mesh, turn) = assets.visual(piece.kind, edit, stage);
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
                    edit,
                    base,
                    pop,
                    shudder: None,
                },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(assets.material.clone()),
                Outline::default(),
                Transform::from_translation(base)
                    .with_rotation(turn)
                    .with_scale(if pop.is_some() {
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

/// Swaps a piece's mesh when it cracks further or is edited (an edit lands
/// with a little shudder).
fn update_piece_meshes(
    assets: Option<Res<PieceAssets>>,
    pieces: Query<
        (&Piece, Option<&PieceEdit>, &PieceVisualLink),
        Or<(Changed<Piece>, Changed<PieceEdit>)>,
    >,
    mut visuals: Query<(&mut PieceVisual, &mut Mesh3d, &mut Transform)>,
) {
    let Some(assets) = assets else {
        return;
    };
    for (piece, edit, link) in &pieces {
        let stage = piece.crack_stage.min(2);
        let edit = edit.copied().unwrap_or_default();
        let Ok((mut visual, mut mesh, mut transform)) = visuals.get_mut(link.0) else {
            continue;
        };
        if visual.stage == stage && visual.edit == edit {
            continue;
        }
        if visual.edit != edit {
            visual.shudder = Some(0.0);
        }
        visual.stage = stage;
        visual.edit = edit;
        let (wanted, turn) = assets.visual(piece.kind, edit, stage);
        if mesh.0 != *wanted {
            mesh.0 = wanted.clone();
        }
        transform.rotation = turn;
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
        assert!(
            start.y < 0.7 && start.x > 1.1,
            "lands squashed flat: {start}"
        );
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
        let (b, r) = (valid.base_color.to_srgba(), invalid.base_color.to_srgba());
        assert!(b.blue > b.red && b.blue > b.green, "valid is blue: {b:?}");
        assert!(r.red > r.blue && r.red > r.green, "invalid is red: {r:?}");
        for m in [&valid, &invalid] {
            assert_eq!(m.alpha_mode, AlphaMode::Blend, "translucent");
            assert!(m.emissive_strength > 0.0, "glowing");
        }
    }
}
