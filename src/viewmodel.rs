//! The first-person guns and gloves, drawn by a separate viewmodel camera
//! (docs/M2-SPEC.md → Guns and gloves).
//!
//! A second 3D camera (a child of the main camera) renders only
//! [`VIEWMODEL_LAYER`] into the same [`WorldTarget`] after the world, with its
//! own depth and a fixed FOV, so the guns never clip into walls and don't warp
//! when the world FOV zooms.
//!
//! The rifle, the pump and the white cartoon gloves are Blender models
//! (`art/blender/assets/guns.py`, `gloves.py`), toon-shaded and inked by the
//! look module. Each gun sits on a "kick" node that squashes and stretches on
//! every shot (pivoting on the right hand); the gloves hold the gun at its
//! `GripR` / `GripL` attach points. Named parts are animated here:
//!
//! - both crystals glow with the magazine ([`CrystalGlow`]: 0.25 + 0.75 × the
//!   magazine fraction) and turn slowly;
//! - rifle reload: the glass `Chamber` slides open, the dim `Crystal` pops up
//!   and spins away, a fresh one slides in, and its glow charges up the moment
//!   the reload completes;
//! - pump: the left glove pushes a violet `Shard` in through the `Rings` for
//!   each shell; the `PumpGrip` racks after every shot and the rings whirr.
//!
//! Every frame (PostUpdate, after the camera follows the eye) the rig's pose is
//! composed from the hip/ADS pose, look and movement sway, walk bob, spring
//! recoil, reload and switch animations, and the muzzle's world position is
//! published in [`MuzzlePoint`] for tracers.

pub mod anim;
pub mod mesh;
pub mod models;

use crate::{
    app::BootGate,
    combat::Loadout,
    fx::sim::{FxRng, Spring},
    look::{
        InheritedOutline, ModelDressed, ModelLook, NoOutline, Outline, OutlineHull, ToonMaterial,
        warmup::Warmup, with_outline_normals,
    },
    models::{MODEL_FORWARD_FIX, ModelLibrary, ModelParts, spawn_model},
    movement::Motor,
    palette::cartoon,
    render::{
        CameraFollowSet, CurrentFov, MainCamera, VIEWMODEL_CAMERA_ORDER, VIEWMODEL_LAYER,
        WorldTarget,
    },
    shared::{ActiveTool, Ads, GameCue, LookAngles, PieceKind, Player, ShotFired, WeaponKind},
    tuning::Tuning,
};
use anim::{
    CrystalPose, LOWERED, PUMP_RELOAD_STANCE, PUMP_SQUASH, PUMP_SQUASH_PEAK, PoseOffset,
    RACK_START, RIFLE_SQUASH, RIFLE_SQUASH_PEAK, RING_RACK_KICK, RING_SHARD_KICK, RingSpin,
    SHARD_MERGED, SHARD_START, Squash, ads_translation, chamber_transform, euler, hand_hold,
    pump_crystal_glow, pump_grip_offset, pump_rack, pump_shard, rifle_crystal_glow, rifle_reload,
    smoothstep, squash_transform, switch_phase,
};
use bevy::{
    camera::{ClearColorConfig, RenderTarget, visibility::RenderLayers},
    light::{NotShadowCaster, NotShadowReceiver},
    prelude::*,
};
use models::{BLUEPRINT, GLOVES_MODEL, GunSpec, PUMP, RIFLE, gun_model, gun_spec};

/// Viewmodel poses are written in this PostUpdate set: after the main camera
/// follows the eye (and after camera shake), before transform propagation.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ViewmodelSet;

/// World-space position of the held gun's muzzle as it appears on screen this
/// frame (projected from the viewmodel FOV into the world camera), or `None`
/// when no gun is up. Tracers start here.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct MuzzlePoint(pub Option<Vec3>);

/// The viewmodel camera.
#[derive(Component, Debug)]
pub struct ViewmodelCamera;

/// The `BootGate` key held until the gun and glove models are attached, dressed
/// and registered for pipeline warm-up.
pub const VIEWMODEL_GATE: &str = "viewmodel";

/// How brightly each gun's crystal glows (see [`anim::ammo_glow`]): 0.25 when
/// empty, 1 when full, a brief flash above 1 as a fresh rifle crystal charges.
/// Updated every frame from the player's loadout.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct CrystalGlow {
    pub rifle: f32,
    pub pump: f32,
}

impl Default for CrystalGlow {
    fn default() -> Self {
        Self {
            rifle: 1.0,
            pump: 1.0,
        }
    }
}

impl CrystalGlow {
    pub fn of(&self, kind: WeaponKind) -> f32 {
        match kind {
            WeaponKind::Rifle => self.rifle,
            WeaponKind::Pump => self.pump,
        }
    }
}

/// Emissive strength of a crystal at glow 1 (crystal color × this is added).
pub const CRYSTAL_EMISSIVE: f32 = 1.5;
/// The rifle's glass chamber glows with its crystal, fainter.
pub const GLASS_EMISSIVE: f32 = 0.55;
/// The glass chamber's opacity.
pub const GLASS_ALPHA: f32 = 0.42;
/// Idle spin of a seated crystal (rad/s): it's alive in there.
pub const CRYSTAL_IDLE_SPIN: f32 = 0.7;

/// Tracks the crystal glow from the player's loadout. Needs only the
/// simulation, so it also runs in headless tests.
pub struct CrystalGlowPlugin;

impl Plugin for CrystalGlowPlugin {
    fn build(&self, app: &mut App) {
        Self::install(app);
    }
}

impl CrystalGlowPlugin {
    /// Adds the glow tracking to an app that is already running (a test's
    /// headless simulation, where plugins can no longer be added).
    pub fn install(app: &mut App) {
        app.init_resource::<CrystalGlow>()
            .init_resource::<GlowTracker>()
            .add_systems(Update, track_crystal_glow);
    }
}

/// What the viewmodel can hold up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Rifle,
    Pump,
    Blueprint,
}

impl Item {
    fn of(tool: ActiveTool) -> Self {
        match tool {
            ActiveTool::Weapon(WeaponKind::Rifle) => Item::Rifle,
            ActiveTool::Weapon(WeaponKind::Pump) => Item::Pump,
            ActiveTool::Build(_) => Item::Blueprint,
        }
    }

    fn gun(self) -> Option<WeaponKind> {
        match self {
            Item::Rifle => Some(WeaponKind::Rifle),
            Item::Pump => Some(WeaponKind::Pump),
            Item::Blueprint => None,
        }
    }
}

/// Role of each animated viewmodel entity.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum VmPart {
    Rig,
    Item(Item),
    /// The squash-and-stretch node holding a gun model.
    Kick(WeaponKind),
    Flash(WeaponKind),
    Mini(PieceKind),
}

/// What a spawned viewmodel model root is.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmModel {
    Gun(WeaponKind),
    Gloves(WeaponKind),
}

/// Seconds to ease into and out of aim-down-sights.
const ADS_SECONDS: f32 = 0.12;
/// Sprint "gun away" pose.
const SPRINT_POSE: PoseOffset = PoseOffset {
    pos: Vec3::new(-0.025, -0.035, 0.03),
    euler: Vec3::new(-0.16, 0.40, 0.20),
};
/// Slide lean.
const SLIDE_POSE: PoseOffset = PoseOffset {
    pos: Vec3::new(-0.01, -0.02, 0.0),
    euler: Vec3::new(0.0, 0.05, 0.22),
};
/// Recoil springs: the rifle kick is small and quick, the pump kick big and slower.
pub const RIFLE_KICK: Spring = Spring::new(42.0);
pub const PUMP_KICK: Spring = Spring::new(19.0);
const SWAY: Spring = Spring::new(13.0);
/// Walk bob: one left-right cycle per this many meters walked.
const BOB_STRIDE: f32 = 2.8;

#[derive(Resource, Debug)]
struct ViewmodelState {
    tool: Option<ActiveTool>,
    prev: Option<ActiveTool>,
    /// Switch progress 0..=1 (1 = done).
    switch_t: f32,
    ads: f32,
    kick: Spring,
    kick_pos: Vec3,
    kick_pos_v: Vec3,
    kick_rot: Vec3,
    kick_rot_v: Vec3,
    squash: Squash,
    squash_x: f32,
    squash_v: f32,
    sway_pos: Vec3,
    sway_pos_v: Vec3,
    sway_rot: Vec3,
    sway_rot_v: Vec3,
    bob_phase: f32,
    bob_amount: f32,
    sprint: f32,
    slide: f32,
    pump_reload: f32,
    last_look: Option<(f32, f32)>,
    rack_age: f32,
    rings: RingSpin,
    /// Idle crystal spin angle.
    crystal_spin: f32,
    last_shell: Option<f32>,
    flash_frames: u8,
    flash_kind: WeaponKind,
    flash_roll: f32,
    flash_scale: f32,
    /// Frames left drawing the flash invisibly small so its pipeline is ready.
    prewarm: u32,
    rng: FxRng,
    /// This frame's part poses, for [`animate_gun_parts`].
    shown: Option<WeaponKind>,
    rifle_reload: Option<anim::RifleReloadPose>,
    pump_loading: f32,
    shard: Option<anim::ShardPose>,
    rack: f32,
}

impl Default for ViewmodelState {
    fn default() -> Self {
        Self {
            tool: None,
            prev: None,
            switch_t: 1.0,
            ads: 0.0,
            kick: RIFLE_KICK,
            kick_pos: Vec3::ZERO,
            kick_pos_v: Vec3::ZERO,
            kick_rot: Vec3::ZERO,
            kick_rot_v: Vec3::ZERO,
            squash: RIFLE_SQUASH,
            squash_x: 0.0,
            squash_v: 0.0,
            sway_pos: Vec3::ZERO,
            sway_pos_v: Vec3::ZERO,
            sway_rot: Vec3::ZERO,
            sway_rot_v: Vec3::ZERO,
            bob_phase: 0.0,
            bob_amount: 0.0,
            sprint: 0.0,
            slide: 0.0,
            pump_reload: 0.0,
            last_look: None,
            rack_age: 10.0,
            rings: RingSpin::default(),
            crystal_spin: 0.0,
            last_shell: None,
            flash_frames: 0,
            flash_kind: WeaponKind::Rifle,
            flash_roll: 0.0,
            flash_scale: 1.0,
            prewarm: 120,
            rng: FxRng::new(0x51DE),
            shown: None,
            rifle_reload: None,
            pump_loading: 0.0,
            shard: None,
            rack: 0.0,
        }
    }
}

/// A model part that is animated: its entity (the glTF node) and its rest
/// transform in model space.
#[derive(Debug, Clone, Copy)]
struct AnimPart {
    entity: Entity,
    rest: Transform,
}

/// One gun's animated parts, found after its models are dressed.
#[derive(Debug, Default, Clone)]
struct GunParts {
    crystal: Option<AnimPart>,
    chamber: Option<AnimPart>,
    rings: Option<AnimPart>,
    pump_grip: Option<AnimPart>,
    shard: Option<AnimPart>,
    glove_r: Option<Entity>,
    glove_l: Option<Entity>,
    gun_ready: bool,
    gloves_ready: bool,
}

/// Half the rifle's glass chamber length along the barrel (it is 0.218 m).
const CHAMBER_HALF_LENGTH: f32 = 0.109;

/// The viewmodel's Blender models: whether they are attached, their parts, and
/// the per-gun materials that glow.
#[derive(Resource, Debug, Default)]
pub struct ViewmodelModels {
    /// The model library exists, so models are expected (and Boot waits).
    wanted: bool,
    spawned: bool,
    released: bool,
    rifle: GunParts,
    pump: GunParts,
    rifle_crystal: Option<Handle<ToonMaterial>>,
    pump_crystal: Option<Handle<ToonMaterial>>,
    glass: Option<Handle<ToonMaterial>>,
}

impl ViewmodelModels {
    fn parts(&self, kind: WeaponKind) -> &GunParts {
        match kind {
            WeaponKind::Rifle => &self.rifle,
            WeaponKind::Pump => &self.pump,
        }
    }

    fn parts_mut(&mut self, kind: WeaponKind) -> &mut GunParts {
        match kind {
            WeaponKind::Rifle => &mut self.rifle,
            WeaponKind::Pump => &mut self.pump,
        }
    }

    /// Every gun and glove model is attached and its parts found.
    pub fn is_ready(&self) -> bool {
        [&self.rifle, &self.pump]
            .iter()
            .all(|p| p.gun_ready && p.gloves_ready)
    }

    /// The material a gun's crystal glows with (once its model is dressed).
    pub fn crystal_material(&self, kind: WeaponKind) -> Option<&Handle<ToonMaterial>> {
        match kind {
            WeaponKind::Rifle => self.rifle_crystal.as_ref(),
            WeaponKind::Pump => self.pump_crystal.as_ref(),
        }
    }
}

pub struct ViewmodelPlugin;

impl Plugin for ViewmodelPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(CrystalGlowPlugin)
            .init_resource::<MuzzlePoint>()
            .init_resource::<ViewmodelState>()
            .init_resource::<ViewmodelModels>()
            .init_resource::<BootGate>()
            .configure_sets(
                PostUpdate,
                ViewmodelSet
                    .after(CameraFollowSet)
                    .before(TransformSystems::Propagate),
            )
            .add_systems(PostStartup, spawn_viewmodel)
            .add_systems(Update, (attach_models, configure_models).chain())
            .add_systems(
                PostUpdate,
                (animate_viewmodel, animate_gun_parts)
                    .chain()
                    .in_set(ViewmodelSet),
            );
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_viewmodel(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut toon: ResMut<Assets<ToonMaterial>>,
    target: Option<Res<WorldTarget>>,
    main: Option<Single<Entity, With<MainCamera>>>,
    tuning: Res<Tuning>,
    library: Option<Res<ModelLibrary>>,
    mut vm_models: ResMut<ViewmodelModels>,
    mut gate: ResMut<BootGate>,
) {
    let (Some(target), Some(main)) = (target, main) else {
        return;
    };
    if library.is_some() {
        // The gun and glove models load with the library; Boot waits for them.
        vm_models.wanted = true;
        gate.hold(VIEWMODEL_GATE);
    }
    let layer = RenderLayers::layer(VIEWMODEL_LAYER);
    // The blueprint tablet: toon-shaded vertex colors with ink outlines. The
    // muzzle flash stays an unlit additive effect.
    let tablet_mat = toon.add(ToonMaterial::vertex_colored());
    let flash_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        ..default()
    });

    let camera = commands
        .spawn((
            Name::new("Viewmodel camera"),
            ViewmodelCamera,
            Camera3d::default(),
            RenderTarget::Image(target.image.clone().into()),
            Camera {
                order: VIEWMODEL_CAMERA_ORDER,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            Projection::Perspective(PerspectiveProjection {
                fov: tuning.feedback.viewmodel_fov_deg.to_radians(),
                near: 0.01,
                far: 5.0,
                ..default()
            }),
            Msaa::Sample4,
            layer.clone(),
            Transform::IDENTITY,
            ChildOf(*main),
        ))
        .id();

    let rig = commands
        .spawn((
            Name::new("Viewmodel rig"),
            VmPart::Rig,
            Transform::from_translation(RIFLE.hip),
            Visibility::Visible,
            ChildOf(camera),
        ))
        .id();

    let item = |commands: &mut Commands, it: Item| {
        commands
            .spawn((
                Name::new(format!("Viewmodel {it:?}")),
                VmPart::Item(it),
                Transform::IDENTITY,
                Visibility::Hidden,
                ChildOf(rig),
            ))
            .id()
    };
    let flash = meshes.add(models::muzzle_flash().build());
    for (it, kind) in [
        (Item::Rifle, WeaponKind::Rifle),
        (Item::Pump, WeaponKind::Pump),
    ] {
        let parent = item(&mut commands, it);
        let kick = commands
            .spawn((
                Name::new(format!("Viewmodel {kind:?} kick")),
                VmPart::Kick(kind),
                Transform::IDENTITY,
                Visibility::Inherited,
                ChildOf(parent),
            ))
            .id();
        commands.spawn((
            Name::new(format!("Viewmodel {kind:?} flash")),
            VmPart::Flash(kind),
            Mesh3d(flash.clone()),
            MeshMaterial3d(flash_mat.clone()),
            Transform::from_translation(gun_spec(kind).muzzle),
            Visibility::Hidden,
            layer.clone(),
            NotShadowCaster,
            NotShadowReceiver,
            ChildOf(kick),
        ));
    }

    let blueprint_item = item(&mut commands, Item::Blueprint);
    let mut tablet_part = |commands: &mut Commands, mesh: Mesh, transform: Transform| {
        commands
            .spawn((
                Mesh3d(meshes.add(with_outline_normals(mesh))),
                MeshMaterial3d(tablet_mat.clone()),
                Outline::default(),
                transform,
                Visibility::Inherited,
                layer.clone(),
                NotShadowCaster,
                NotShadowReceiver,
                ChildOf(blueprint_item),
            ))
            .id()
    };
    tablet_part(
        &mut commands,
        models::blueprint().build(),
        Transform::IDENTITY,
    );
    for kind in [PieceKind::Wall, PieceKind::Floor, PieceKind::Ramp] {
        let mini = tablet_part(
            &mut commands,
            models::mini_piece(kind).build(),
            Transform::from_xyz(-0.01, 0.0036, 0.0),
        );
        commands
            .entity(mini)
            .insert((VmPart::Mini(kind), Visibility::Hidden));
    }
}

/// Once the model library has loaded, puts the gun models on their kick nodes
/// and a pair of gloves on each gun's item.
fn attach_models(
    mut commands: Commands,
    library: Option<Res<ModelLibrary>>,
    mut vm_models: ResMut<ViewmodelModels>,
    nodes: Query<(Entity, &VmPart, &ChildOf)>,
    mut gate: ResMut<BootGate>,
) {
    let Some(library) = library else { return };
    if !vm_models.wanted || vm_models.spawned || !library.is_ready() {
        return;
    }
    vm_models.spawned = true;
    let mut missing = Vec::new();
    for kind in [WeaponKind::Rifle, WeaponKind::Pump] {
        let Some((kick, item)) = nodes.iter().find_map(|(e, part, child_of)| {
            (*part == VmPart::Kick(kind)).then_some((e, child_of.parent()))
        }) else {
            continue;
        };
        for (name, parent, role) in [
            (gun_model(kind), kick, VmModel::Gun(kind)),
            (GLOVES_MODEL, item, VmModel::Gloves(kind)),
        ] {
            let failed = library.failed().iter().any(|f| f == name);
            match spawn_model(&mut commands, &library, name, Transform::IDENTITY) {
                Some(root) if !failed => {
                    commands.entity(root).insert((
                        role,
                        ModelLook::Viewmodel,
                        Outline::default(),
                        ChildOf(parent),
                    ));
                }
                _ => missing.push(name),
            }
        }
    }
    if !missing.is_empty() {
        error!("viewmodel: models missing: {missing:?}; the guns will be incomplete");
        vm_models.released = true;
        gate.release(VIEWMODEL_GATE);
    }
}

/// Maps a model-space transform to a glTF part node's local transform: the
/// node's glTF parent is the asset's root node (at the origin), under the model
/// root's forward-fix node, a half turn that is its own inverse.
pub fn model_to_node(t: Transform) -> Transform {
    Transform::from_rotation(MODEL_FORWARD_FIX) * t
}

/// The inverse of [`model_to_node`] (the same half turn).
fn node_to_model(t: Transform) -> Transform {
    Transform::from_rotation(MODEL_FORWARD_FIX) * t
}

/// After the look has dressed a viewmodel model: finds its animated parts,
/// gives the crystals and the glass their own materials, puts the gloves on
/// their grips, and (once everything is in) registers warm-up draws and lets
/// Boot go.
#[allow(clippy::too_many_arguments)]
fn configure_models(
    mut dressed: MessageReader<ModelDressed>,
    roles: Query<&VmModel>,
    parts: ModelParts,
    children: Query<&Children>,
    mut transforms: Query<&mut Transform>,
    // A part's own meshes, not the ink hulls the look hangs under them.
    mesh_entities: Query<&Mesh3d, Without<OutlineHull>>,
    mut toon: ResMut<Assets<ToonMaterial>>,
    mut vm_models: ResMut<ViewmodelModels>,
    mut commands: Commands,
    mut warmup: Warmup,
) {
    let layer = RenderLayers::layer(VIEWMODEL_LAYER);
    for event in dressed.read() {
        let Ok(&role) = roles.get(event.root) else {
            continue;
        };
        let meshes_below = |node: Entity| -> Vec<Entity> {
            std::iter::once(node)
                .chain(children.iter_descendants(node))
                .filter(|e| mesh_entities.contains(*e))
                .collect()
        };
        match role {
            VmModel::Gun(kind) => {
                let anim_part = |name: &str| {
                    parts.find(event.root, name).map(|entity| AnimPart {
                        entity,
                        rest: node_to_model(transforms.get(entity).copied().unwrap_or_default()),
                    })
                };
                let mut found = GunParts {
                    crystal: anim_part("Crystal"),
                    shard: anim_part("Shard"),
                    rings: anim_part("Rings"),
                    pump_grip: anim_part("PumpGrip"),
                    chamber: anim_part("Chamber"),
                    ..default()
                };
                let color = match kind {
                    WeaponKind::Rifle => cartoon::CRYSTAL_BLUE,
                    WeaponKind::Pump => cartoon::CRYSTAL_VIOLET,
                };
                let crystal_mat =
                    toon.add(ToonMaterial::vertex_colored().with_emissive(color, CRYSTAL_EMISSIVE));
                for part in [found.crystal, found.shard].into_iter().flatten() {
                    for mesh in meshes_below(part.entity) {
                        commands
                            .entity(mesh)
                            .insert(MeshMaterial3d(crystal_mat.clone()));
                    }
                }
                if let Some(crystal) = found.crystal
                    && let Some(m) = meshes_below(crystal.entity)
                        .first()
                        .and_then(|e| mesh_entities.get(*e).ok())
                {
                    warmup.add_with(
                        m.0.clone(),
                        crystal_mat.clone(),
                        (layer.clone(), Outline::default()),
                    );
                }
                if let Some(shard) = found.shard {
                    commands.entity(shard.entity).insert(Visibility::Hidden);
                }
                if let Some(chamber) = found.chamber {
                    let glass = toon.add(
                        ToonMaterial::new(cartoon::GLASS_CYAN.with_alpha(GLASS_ALPHA))
                            .with_emissive(cartoon::CRYSTAL_BLUE, GLASS_EMISSIVE)
                            .with_alpha(AlphaMode::Blend),
                    );
                    for mesh in meshes_below(chamber.entity) {
                        // Glass is see-through: an ink hull behind it would
                        // show through as a dark tube.
                        commands
                            .entity(mesh)
                            .insert((MeshMaterial3d(glass.clone()), NoOutline))
                            .remove::<(Outline, InheritedOutline)>();
                        if let Ok(m) = mesh_entities.get(mesh) {
                            warmup.add_with(m.0.clone(), glass.clone(), layer.clone());
                        }
                    }
                    vm_models.glass = Some(glass);
                }
                match kind {
                    WeaponKind::Rifle => vm_models.rifle_crystal = Some(crystal_mat),
                    WeaponKind::Pump => vm_models.pump_crystal = Some(crystal_mat),
                }
                let slot = vm_models.parts_mut(kind);
                found.gun_ready = true;
                found.glove_r = slot.glove_r;
                found.glove_l = slot.glove_l;
                found.gloves_ready = slot.gloves_ready;
                *slot = found;
            }
            VmModel::Gloves(kind) => {
                let spec = gun_spec(kind);
                let slot = vm_models.parts_mut(kind);
                slot.glove_r = parts.find(event.root, "GloveR");
                slot.glove_l = parts.find(event.root, "GloveL");
                for (glove, grip) in [(slot.glove_r, spec.grip_r), (slot.glove_l, spec.grip_l)] {
                    if let Some(glove) = glove
                        && let Ok(mut t) = transforms.get_mut(glove)
                    {
                        *t = model_to_node(grip);
                    }
                }
                slot.gloves_ready = true;
            }
        }
    }
    if vm_models.wanted && !vm_models.released && vm_models.is_ready() {
        vm_models.released = true;
        warmup.gate().release(VIEWMODEL_GATE);
    }
}

fn spec_of(item: Item) -> &'static GunSpec {
    match item {
        Item::Rifle => &RIFLE,
        Item::Pump => &PUMP,
        Item::Blueprint => &BLUEPRINT,
    }
}

/// Moves `x` toward `target` by at most `step`.
fn approach(x: f32, target: f32, step: f32) -> f32 {
    if x < target {
        (x + step).min(target)
    } else {
        (x - step).max(target)
    }
}

fn wrap_angle(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

fn flag(b: bool) -> f32 {
    if b { 1.0 } else { 0.0 }
}

/// Seconds since the rifle's last reload completed (for the charge-up).
#[derive(Resource, Debug)]
struct GlowTracker {
    was_reloading: bool,
    since_reload: f32,
}

impl Default for GlowTracker {
    fn default() -> Self {
        Self {
            was_reloading: false,
            since_reload: 1.0e6,
        }
    }
}

fn track_crystal_glow(
    time: Res<Time>,
    tuning: Res<Tuning>,
    player: Option<Single<&Loadout, With<Player>>>,
    mut tracker: ResMut<GlowTracker>,
    mut glow: ResMut<CrystalGlow>,
) {
    let Some(loadout) = player else { return };
    let combat = &tuning.combat;
    let reloading = loadout.rifle.is_reloading();
    if tracker.was_reloading && !reloading {
        tracker.since_reload = 0.0;
    } else {
        tracker.since_reload = (tracker.since_reload + time.delta_secs()).min(1.0e6);
    }
    tracker.was_reloading = reloading;
    let next = CrystalGlow {
        rifle: rifle_crystal_glow(
            loadout.rifle.ammo,
            combat.rifle.magazine,
            loadout.rifle.reload_progress(&combat.rifle),
            tracker.since_reload,
        ),
        pump: pump_crystal_glow(
            loadout.pump.ammo,
            combat.pump.magazine,
            loadout.pump.reload_progress(&combat.pump),
        ),
    };
    if *glow != next {
        *glow = next;
    }
}

#[allow(clippy::too_many_arguments)]
fn animate_viewmodel(
    time: Res<Time>,
    tuning: Res<Tuning>,
    fov: Res<CurrentFov>,
    mut state: ResMut<ViewmodelState>,
    mut muzzle: ResMut<MuzzlePoint>,
    mut shots: MessageReader<ShotFired>,
    mut cues: MessageReader<GameCue>,
    player: Option<
        Single<
            (
                Entity,
                &ActiveTool,
                &Ads,
                &Loadout,
                &LookAngles,
                Option<&Motor>,
            ),
            With<Player>,
        >,
    >,
    main_camera: Option<Single<&Transform, (With<MainCamera>, Without<VmPart>)>>,
    mut vm_camera: Option<Single<&mut Projection, With<ViewmodelCamera>>>,
    mut parts: Query<(&VmPart, &mut Transform, &mut Visibility)>,
) {
    let dt = time.delta_secs();
    let Some(player) = player else {
        for (part, _, mut vis) in &mut parts {
            if *part == VmPart::Rig {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
        muzzle.0 = None;
        state.shown = None;
        shots.clear();
        cues.clear();
        return;
    };
    let (me, tool, ads, loadout, look, motor) = player.into_inner();
    let st = &mut *state;
    let combat = &tuning.combat;
    let feedback = &tuning.feedback;

    // Tool changes start a lower/raise (not between two build pieces).
    if st.tool != Some(*tool) {
        match st.tool {
            Some(old) if !(old.is_build() && tool.is_build()) => {
                st.prev = Some(old);
                st.switch_t = 0.0;
            }
            None => st.switch_t = 1.0,
            _ => {}
        }
        st.tool = Some(*tool);
    }
    st.switch_t = (st.switch_t + dt / combat.switch_time.max(0.05)).min(1.0);
    let (show_prev, lowered) = match st.prev {
        Some(_) if st.switch_t < 1.0 => switch_phase(st.switch_t),
        _ => (false, 0.0),
    };
    let shown_tool = if show_prev {
        st.prev.unwrap_or(*tool)
    } else {
        *tool
    };
    let shown = Item::of(shown_tool);
    let spec = spec_of(shown);
    let gun = shown.gun();

    // ADS eases in over ~0.12 s.
    let ads_target = flag(ads.0 && !tool.is_build());
    st.ads = approach(st.ads, ads_target, dt / ADS_SECONDS);
    let ads_e = smoothstep(st.ads);
    let calm = 1.0 - 0.85 * ads_e;

    // Shots: spring kick, squash, muzzle flash, pump rack.
    for shot in shots.read() {
        if shot.shooter != me {
            continue;
        }
        let rng = &mut st.rng;
        let (spring, pos_peak, rot_peak, squash, squash_peak) = match shot.weapon {
            WeaponKind::Rifle => (
                RIFLE_KICK,
                Vec3::new(rng.range(-0.002, 0.002), 0.003, 0.018),
                Vec3::new(0.030, rng.range(-0.008, 0.008), rng.range(-0.012, 0.012)),
                RIFLE_SQUASH,
                RIFLE_SQUASH_PEAK,
            ),
            WeaponKind::Pump => (
                PUMP_KICK,
                Vec3::new(0.0, 0.010, 0.075),
                Vec3::new(0.19, rng.range(-0.02, 0.02), rng.range(-0.05, -0.02)),
                PUMP_SQUASH,
                PUMP_SQUASH_PEAK,
            ),
        };
        if shot.weapon == WeaponKind::Pump {
            st.rack_age = 0.0;
        }
        let rot_scale = 1.0 - 0.55 * ads_e;
        let pos_scale = Vec3::new(1.0, 1.0, 1.0 - 0.3 * ads_e);
        st.kick = spring;
        st.kick_pos_v += Vec3::new(
            spring.kick_for_peak(pos_peak.x),
            spring.kick_for_peak(pos_peak.y),
            spring.kick_for_peak(pos_peak.z),
        ) * pos_scale;
        st.kick_rot_v += Vec3::new(
            spring.kick_for_peak(rot_peak.x),
            spring.kick_for_peak(rot_peak.y),
            spring.kick_for_peak(rot_peak.z),
        ) * rot_scale;
        // In ADS the squash is gentler, so the sights stay readable.
        st.squash = squash;
        st.squash_v += squash.kick_for_peak(squash_peak * (1.0 - 0.5 * ads_e));
        st.flash_frames = 2;
        st.flash_kind = shot.weapon;
        st.flash_roll = st.rng.range(0.0, std::f32::consts::TAU);
        st.flash_scale = st.rng.range(0.85, 1.15);
    }
    for cue in cues.read() {
        match *cue {
            GameCue::Land { who, speed } if who == me => {
                let s = (speed / 8.0).clamp(0.0, 1.0);
                st.kick_pos_v.y -= st.kick.kick_for_peak(0.02 * s);
                st.kick_rot_v.x -= st.kick.kick_for_peak(0.04 * s);
            }
            GameCue::Jump { who } if who == me => {
                st.kick_pos_v.y -= st.kick.kick_for_peak(0.008);
            }
            _ => {}
        }
    }
    let kick = st.kick;
    kick.step(&mut st.kick_pos, &mut st.kick_pos_v, Vec3::ZERO, dt);
    kick.step(&mut st.kick_rot, &mut st.kick_rot_v, Vec3::ZERO, dt);
    let squash = st.squash;
    squash.step(&mut st.squash_x, &mut st.squash_v, dt);

    // Sway from look and movement.
    let (dyaw, dpitch) = match st.last_look {
        Some((yaw, pitch)) => (wrap_angle(look.yaw - yaw), look.pitch - pitch),
        None => (0.0, 0.0),
    };
    st.last_look = Some((look.yaw, look.pitch));
    let sway_on = feedback.viewmodel_sway;
    let (mut sway_pos, mut sway_rot) = (Vec3::ZERO, Vec3::ZERO);
    let (speed, grounded, sliding, sprinting) = motor
        .map(|m| {
            (
                m.velocity.with_y(0.0).length(),
                m.grounded,
                m.sliding,
                m.sprinting,
            )
        })
        .unwrap_or((0.0, true, false, false));
    if sway_on && dt > 1e-4 {
        let rate = (Vec2::new(dyaw, dpitch) / dt).clamp_length_max(9.0);
        sway_pos += Vec3::new(rate.x * 0.0045, -rate.y * 0.0045, 0.0);
        sway_rot += Vec3::new(-rate.y * 0.012, -rate.x * 0.010, rate.x * 0.018);
        if let Some(m) = motor {
            let (fwd, right) = look.flat_basis();
            let vr = m.velocity.dot(right);
            let vf = m.velocity.dot(fwd);
            sway_pos += Vec3::new(
                -vr * 0.0022,
                (-m.velocity.y * 0.004).clamp(-0.025, 0.025),
                vf * 0.0018,
            );
            sway_rot.z -= vr * 0.010;
        }
        sway_pos = sway_pos.clamp_length_max(0.04);
    }
    SWAY.step(&mut st.sway_pos, &mut st.sway_pos_v, sway_pos, dt);
    SWAY.step(&mut st.sway_rot, &mut st.sway_rot_v, sway_rot, dt);

    // Walk bob on the gun only (never the camera).
    let bob_target = if sway_on && grounded && !sliding {
        (speed / 5.5).min(1.4)
    } else {
        0.0
    };
    st.bob_amount += (bob_target - st.bob_amount) * (1.0 - (-10.0 * dt).exp());
    if grounded {
        st.bob_phase = (st.bob_phase + dt * speed * std::f32::consts::TAU / BOB_STRIDE)
            % std::f32::consts::TAU;
    }
    let a = st.bob_amount * (1.0 - 0.9 * ads_e);
    let phase = st.bob_phase;
    let bob = PoseOffset {
        pos: Vec3::new(phase.sin() * 0.006 * a, -phase.sin().abs() * 0.009 * a, 0.0),
        euler: Vec3::new(0.0, 0.0, phase.sin() * 0.012 * a),
    };

    // Sprint and slide stances.
    let since_shot = gun.map_or(10.0, |k| loadout.gun(k).since_shot);
    let sprint_target = sprinting && grounded && !sliding && ads_e < 0.01 && since_shot > 0.3;
    st.sprint = approach(st.sprint, flag(sprint_target), dt / 0.16);
    st.slide = approach(st.slide, flag(sliding), dt / 0.12);

    // Reloads, the pump rack and the rings.
    let mut reload = PoseOffset::default();
    let pump_reloading = loadout.pump.is_reloading() && shown == Item::Pump;
    st.pump_reload = approach(
        st.pump_reload,
        flag(pump_reloading),
        dt / if pump_reloading { 0.12 } else { 0.16 },
    );
    let prev_rack_age = st.rack_age;
    st.rack_age += dt;
    if prev_rack_age < RACK_START && st.rack_age >= RACK_START {
        st.rings.kick(RING_RACK_KICK);
    }
    let shell = loadout.pump.reload_progress(&combat.pump);
    if let (Some(p), Some(last)) = (shell, st.last_shell)
        && last < SHARD_MERGED
        && p >= SHARD_MERGED
    {
        st.rings.kick(RING_SHARD_KICK);
    }
    st.last_shell = shell;
    st.rings.step(dt);
    st.crystal_spin = (st.crystal_spin + CRYSTAL_IDLE_SPIN * dt) % std::f32::consts::TAU;
    st.rack = if shown == Item::Pump {
        pump_rack(st.rack_age)
    } else {
        0.0
    };
    st.rifle_reload = None;
    st.shard = None;
    match shown {
        Item::Rifle => {
            if let Some(p) = loadout.rifle.reload_progress(&combat.rifle) {
                let pose = rifle_reload(p);
                reload = pose.gun;
                st.rifle_reload = Some(pose);
            }
        }
        Item::Pump => {
            reload = PUMP_RELOAD_STANCE.scaled(smoothstep(st.pump_reload));
            if pump_reloading && let Some(p) = shell {
                st.shard = Some(pump_shard(p));
            }
            reload = reload
                + PoseOffset {
                    pos: Vec3::new(0.0, -0.006, 0.02),
                    euler: Vec3::new(0.06, 0.03, -0.09),
                }
                .scaled(st.rack);
        }
        Item::Blueprint => {}
    }
    st.pump_loading = smoothstep(st.pump_reload);
    st.shown = gun;

    // Compose the rig pose.
    let hip = PoseOffset {
        pos: spec.hip,
        euler: spec.hip_euler,
    };
    let base = if gun.is_some() {
        hip.scaled(1.0 - ads_e)
            + PoseOffset {
                pos: ads_translation(spec),
                euler: Vec3::ZERO,
            }
            .scaled(ads_e)
    } else {
        hip
    };
    let sway = PoseOffset {
        pos: st.sway_pos,
        euler: st.sway_rot,
    };
    let kick_pose = PoseOffset {
        pos: st.kick_pos,
        euler: st.kick_rot,
    };
    let offset = sway.scaled(calm)
        + bob
        + SPRINT_POSE.scaled(smoothstep(st.sprint))
        + SLIDE_POSE.scaled(smoothstep(st.slide) * calm)
        + reload.scaled(1.0 - 0.5 * ads_e)
        + kick_pose
        + LOWERED.scaled(lowered);
    let rig_tf = Transform {
        translation: base.pos + offset.pos,
        rotation: euler(base.euler + offset.euler),
        scale: Vec3::ONE,
    };

    // Viewmodel FOV tightens a little in ADS.
    let fov_vm = feedback.viewmodel_fov_deg.clamp(40.0, 90.0).to_radians() * (1.0 - 0.1 * ads_e);
    if let Some(projection) = vm_camera.as_mut()
        && let Projection::Perspective(p) = projection.as_mut()
        && (p.fov - fov_vm).abs() > 1e-5
    {
        p.fov = fov_vm;
    }

    // Where the muzzle appears on screen, placed in the world at the same depth.
    let squash_tf = squash_transform(st.squash_x, spec.grip_r.translation);
    muzzle.0 = match (gun, main_camera) {
        (Some(_), Some(cam)) if lowered < 0.5 => {
            let m = rig_tf.transform_point(squash_tf.transform_point(spec.muzzle));
            let s = (fov.0 * 0.5).tan() / (fov_vm * 0.5).tan();
            Some(cam.transform_point(Vec3::new(m.x * s, m.y * s, m.z)))
        }
        _ => None,
    };

    let flash_on = st.flash_frames > 0 && gun == Some(st.flash_kind) && lowered < 0.5;
    let flash_size = st.flash_scale
        * if st.flash_frames >= 2 { 1.0 } else { 0.6 }
        * if st.flash_kind == WeaponKind::Pump {
            1.9
        } else {
            1.0
        };
    let build_kind = match tool {
        ActiveTool::Build(kind) => Some(*kind),
        _ => None,
    };
    for (part, mut tf, mut vis) in &mut parts {
        let want = match *part {
            VmPart::Rig => {
                *tf = rig_tf;
                true
            }
            VmPart::Item(it) => it == shown,
            VmPart::Kick(kind) => {
                if Some(kind) == gun {
                    tf.set_if_neq(squash_tf);
                }
                true
            }
            VmPart::Flash(kind) => {
                let on = flash_on && kind == st.flash_kind;
                if on {
                    tf.rotation = Quat::from_rotation_z(st.flash_roll);
                    tf.scale = Vec3::splat(flash_size);
                } else if st.prewarm > 0 {
                    tf.scale = Vec3::splat(1e-4);
                }
                on || st.prewarm > 0
            }
            VmPart::Mini(kind) => Some(kind) == build_kind,
        };
        let v = match (want, *part) {
            (false, _) => Visibility::Hidden,
            (true, VmPart::Rig) => Visibility::Visible,
            (true, _) => Visibility::Inherited,
        };
        vis.set_if_neq(v);
    }
    st.flash_frames = st.flash_frames.saturating_sub(1);
    st.prewarm = st.prewarm.saturating_sub(1);
}

/// Poses the shown gun's named parts and gloves, and sets the crystals' glow.
fn animate_gun_parts(
    state: Res<ViewmodelState>,
    vm_models: Res<ViewmodelModels>,
    glow: Res<CrystalGlow>,
    mut toon: ResMut<Assets<ToonMaterial>>,
    mut transforms: Query<&mut Transform, (Without<VmPart>, Without<MainCamera>)>,
    mut visibility: Query<&mut Visibility, Without<VmPart>>,
) {
    // Crystal glow, written only when it changes.
    for (handle, level, color) in [
        (&vm_models.rifle_crystal, glow.rifle, cartoon::CRYSTAL_BLUE),
        (&vm_models.pump_crystal, glow.pump, cartoon::CRYSTAL_VIOLET),
    ] {
        set_emissive(&mut toon, handle.as_ref(), color, level * CRYSTAL_EMISSIVE);
    }
    set_emissive(
        &mut toon,
        vm_models.glass.as_ref(),
        cartoon::CRYSTAL_BLUE,
        glow.rifle * GLASS_EMISSIVE,
    );

    let Some(kind) = state.shown else { return };
    let spec = gun_spec(kind);
    let parts = vm_models.parts(kind);
    let mut place = |part: Option<AnimPart>, model: Transform| {
        if let Some(part) = part
            && let Ok(mut t) = transforms.get_mut(part.entity)
        {
            t.set_if_neq(model_to_node(model));
        }
    };
    let mut show = |part: Option<AnimPart>, visible: bool| {
        if let Some(part) = part
            && let Ok(mut v) = visibility.get_mut(part.entity)
        {
            v.set_if_neq(if visible {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    };
    let idle = CrystalPose {
        spin: state.crystal_spin,
        ..CrystalPose::SEATED
    };
    let squash = squash_transform(state.squash_x, spec.grip_r.translation);
    let mut glove_l = spec.grip_l;
    match kind {
        WeaponKind::Rifle => {
            let (crystal, open) = match state.rifle_reload {
                Some(r) => (r.crystal, r.chamber_open),
                None => (idle, 0.0),
            };
            place(parts.crystal, crystal.transform(spec.socket));
            show(parts.crystal, crystal.visible());
            // The left glove fetches the fresh crystal and carries it in.
            if let Some(r) = state.rifle_reload
                && r.hand > 0.0
            {
                glove_l = hand_hold(r.hand, glove_l, spec.socket + r.hand_at);
            }
            if let Some(chamber) = parts.chamber {
                place(
                    Some(chamber),
                    chamber_transform(chamber.rest, CHAMBER_HALF_LENGTH, open),
                );
            }
        }
        WeaponKind::Pump => {
            place(parts.crystal, idle.transform(spec.socket));
            if let Some(rings) = parts.rings {
                place(Some(rings), state.rings.transform(rings.rest));
            }
            let grip = pump_grip_offset(state.rack);
            if let Some(g) = parts.pump_grip {
                place(Some(g), g.rest.with_translation(g.rest.translation + grip));
            }
            glove_l.translation += grip;
            // While loading, the left glove carries each shard down through
            // the rings.
            if state.pump_loading > 0.0 {
                let at = state.shard.map_or(SHARD_START, |s| s.hand_at);
                glove_l = hand_hold(state.pump_loading, glove_l, spec.socket + at);
            }
            let shard = state.shard.map(|s| s.crystal);
            if let Some(pose) = shard {
                place(parts.shard, pose.transform(spec.socket));
            }
            show(parts.shard, shard.is_some_and(|p| p.visible()));
        }
    }
    // The gloves stay unsquashed, but the left one follows its grip as the gun
    // squashes into the right hand.
    glove_l.translation = squash.transform_point(glove_l.translation);
    for (glove, grip) in [(parts.glove_r, spec.grip_r), (parts.glove_l, glove_l)] {
        if let Some(glove) = glove
            && let Ok(mut t) = transforms.get_mut(glove)
        {
            t.set_if_neq(model_to_node(grip));
        }
    }
}

fn set_emissive(
    toon: &mut Assets<ToonMaterial>,
    handle: Option<&Handle<ToonMaterial>>,
    color: Color,
    strength: f32,
) {
    let Some(handle) = handle else { return };
    let unchanged = toon
        .get(handle)
        .is_some_and(|m| (m.emissive_strength - strength).abs() < 1e-3 && m.emissive == color);
    if unchanged {
        return;
    }
    if let Some(mut m) = toon.get_mut(handle) {
        m.emissive = color;
        m.emissive_strength = strength;
    }
}
