//! Slice D — the living galaxy sky and the magical far view (client only;
//! docs/M2-SPEC.md → The sky and far view, targets T01, T10, T11).
//!
//! - **Galaxy** ([`galaxy`]): a seeded procedural cubemap generated on a
//!   background thread at launch, shown with Bevy's `Skybox` on the main camera
//!   and turned once every 10 minutes about its own core ([`GalaxySpin`];
//!   `--knobs skyrot=off` freezes it, `sky=off` leaves the sky out).
//! - **Far models** (`art/blender/assets/far.py`): the stained-glass cathedral
//!   station, far islands with waterfalls, ships and the ringed planet, placed by
//!   [`FarLayout`] and drawn with the far material (no outlines). Glass parts
//!   pulse and glow through halos, waterfall strips scroll ([`WaterfallMaterial`]),
//!   ships trail additive streaks, and a soft glow band lifts the horizon.
//! - **Motion** ([`motion`], [`FarMotionPlugin`]): pure transform and parameter
//!   updates on [`SkyClock`], testable headless.
//!
//! Boot waits (gate [`FAR_GATE`]) until the skybox has drawn a few frames and
//! every far model is dressed, so nothing pops in or compiles mid-game.
//! `--knobs far=off` hides the whole far view.

pub mod galaxy;
pub mod layout;
pub mod motion;
pub mod waterfall;

pub use galaxy::{GALAXY_FACE, GALAXY_SEED, GalaxyParams, galaxy_image, generate_galaxy};
pub use layout::{FarLayout, FarPiece, IslandSpec, SPAWN_EYE, ShipSpec};
pub use motion::{
    Bob, FarMotionPlugin, FarMotionSet, GALAXY_PERIOD_S, GLASS_PERIOD_S, GalaxySky, GalaxySpin,
    GlassGlow, GlassPane, GlassPulse, ShipFlight, ShipLoop, SkyClock, galaxy_rotation, glass_level,
};
pub use waterfall::{WaterfallMaterial, sync_waterfall_haze};

use crate::{
    app::BootGate,
    look::{FarHaze, FarMaterial, Halo, LookSettings, ModelDressed, ModelLook, warmup::Warmup},
    models::{ModelLibrary, spawn_model},
    palette::cartoon,
    perf_knobs::PerfKnobs,
    render::MainCamera,
};
use bevy::{
    asset::{RenderAssetUsages, io::embedded::EmbeddedAssetRegistry},
    camera::Exposure,
    core_pipeline::Skybox,
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    prelude::*,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    thread::JoinHandle,
    time::Instant,
};

/// The [`BootGate`] key held until the galaxy is on screen and the far models
/// are dressed.
pub const FAR_GATE: &str = "far";
/// Frames the skybox draws behind the loading overlay before Boot may end (its
/// pipeline compiles on the first).
pub const SKYBOX_WARM_FRAMES: u32 = 3;
/// Never hold Boot longer than this many frames for the far view.
pub const FAR_GATE_TIMEOUT_FRAMES: u32 = 900;

/// The far layer's haze: toward the lit blue of the targets' horizon sky, so
/// distance melts things into the galaxy.
pub fn galaxy_haze() -> FarHaze {
    FarHaze {
        color: Color::srgb_u8(0x48, 0x5E, 0xC2),
        start: 120.0,
        density: 0.0017,
    }
}

/// Skybox brightness that shows the cubemap's texels exactly as authored: the
/// skybox multiplies by the camera's exposure (default EV100 9.7, ≈ 1/975), and
/// both cameras draw without a tone curve.
pub fn galaxy_brightness() -> f32 {
    1.0 / Exposure::default().exposure()
}

pub struct FarViewPlugin;

impl Plugin for FarViewPlugin {
    fn build(&self, app: &mut App) {
        register_shaders(app);
        app.add_plugins((
            MaterialPlugin::<WaterfallMaterial>::default(),
            FarMotionPlugin,
        ))
        // The sky slice owns the far haze (the galaxy's colours). Added after the
        // arena visuals in `ClientPlugins`, so it replaces their placeholder.
        .insert_resource(galaxy_haze())
        .init_resource::<FarLayout>()
        .init_resource::<FarBoot>()
        .init_resource::<BootGate>()
        .add_systems(
            Startup,
            (
                start_galaxy,
                insert_galaxy_spin,
                (
                    spawn_far_view,
                    create_far_assets,
                    spawn_horizon_glow,
                    warm_far_variants,
                )
                    .chain(),
            ),
        )
        .add_systems(
            Update,
            (
                finish_galaxy,
                attach_galaxy_skybox,
                attach_far_models,
                apply_far_view_setting,
                sync_waterfall_haze,
                release_far_gate,
            )
                .chain()
                .before(FarMotionSet),
        )
        .add_systems(PostUpdate, dress_far_models);
    }
}

fn register_shaders(app: &mut App) {
    let registry = app.world().resource::<EmbeddedAssetRegistry>();
    registry.insert_asset(
        PathBuf::new(),
        Path::new("pieced/shaders/waterfall.wgsl"),
        include_bytes!("../../assets/shaders/waterfall.wgsl").as_slice(),
    );
}

// ---------------------------------------------------------------------------
// Galaxy
// ---------------------------------------------------------------------------

/// The galaxy cubemap once generated, and how long generation took.
#[derive(Resource, Debug, Clone)]
pub struct GalaxyTexture {
    pub image: Handle<Image>,
    pub millis: f64,
}

#[derive(Resource)]
struct GalaxyJob {
    handle: Option<JoinHandle<Vec<u8>>>,
    params: GalaxyParams,
    started: Instant,
}

/// Progress of the far view through Boot.
#[derive(Resource, Debug, Default)]
pub struct FarBoot {
    /// Frames the galaxy skybox has been on the main camera.
    pub skybox_frames: u32,
    /// Far models spawned (set once the model library is ready).
    pub models_spawned: Option<usize>,
    /// Far models dressed so far.
    pub models_dressed: usize,
    frames: u32,
    released: bool,
}

fn sky_wanted(knobs: Option<&PerfKnobs>) -> bool {
    knobs.and_then(|k| k.sky) != Some(false)
}

fn start_galaxy(mut commands: Commands, mut gate: ResMut<BootGate>, layout: Res<FarLayout>) {
    gate.hold(FAR_GATE);
    let params = GalaxyParams {
        center: layout.galaxy,
        ..default()
    };
    let job_params = params.clone();
    let handle = std::thread::Builder::new()
        .name("galaxy".into())
        .spawn(move || generate_galaxy(&job_params))
        .expect("spawn the galaxy thread");
    commands.insert_resource(GalaxyJob {
        handle: Some(handle),
        params,
        started: Instant::now(),
    });
}

fn insert_galaxy_spin(mut commands: Commands, layout: Res<FarLayout>) {
    commands.insert_resource(GalaxySpin {
        axis: layout.galaxy,
        period_s: GALAXY_PERIOD_S,
    });
}

fn finish_galaxy(
    mut commands: Commands,
    job: Option<ResMut<GalaxyJob>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(mut job) = job else { return };
    if !job.handle.as_ref().is_some_and(JoinHandle::is_finished) {
        return;
    }
    let data = job
        .handle
        .take()
        .unwrap()
        .join()
        .expect("galaxy generation panicked");
    let millis = job.started.elapsed().as_secs_f64() * 1000.0;
    info!(
        "far: galaxy {n}² × 6 generated in {millis:.0} ms",
        n = job.params.face
    );
    let image = images.add(galaxy_image(data, job.params.face));
    commands.insert_resource(GalaxyTexture { image, millis });
    commands.remove_resource::<GalaxyJob>();
}

fn attach_galaxy_skybox(
    mut commands: Commands,
    texture: Option<Res<GalaxyTexture>>,
    knobs: Option<Res<PerfKnobs>>,
    spin: Option<Res<GalaxySpin>>,
    clock: Res<SkyClock>,
    cameras: Query<Entity, (With<MainCamera>, Without<GalaxySky>)>,
    with_sky: Query<(), (With<MainCamera>, With<GalaxySky>)>,
    mut boot: ResMut<FarBoot>,
) {
    if !with_sky.is_empty() {
        boot.skybox_frames += 1;
    }
    let Some(texture) = texture else { return };
    if !sky_wanted(knobs.as_deref()) {
        return;
    }
    let rotation = spin.map_or(Quat::IDENTITY, |s| galaxy_rotation(&s, clock.seconds));
    for camera in &cameras {
        commands.entity(camera).insert((
            Skybox {
                image: Some(texture.image.clone()),
                brightness: galaxy_brightness(),
                rotation,
            },
            GalaxySky,
        ));
    }
}

fn release_far_gate(
    mut gate: ResMut<BootGate>,
    mut boot: ResMut<FarBoot>,
    knobs: Option<Res<PerfKnobs>>,
    library: Option<Res<ModelLibrary>>,
) {
    if boot.released {
        return;
    }
    boot.frames += 1;
    let sky_done = boot.skybox_frames >= SKYBOX_WARM_FRAMES || !sky_wanted(knobs.as_deref());
    let models_done = match boot.models_spawned {
        Some(n) => boot.models_dressed >= n,
        // No model library in this app (or it failed): nothing to wait for.
        None => library
            .as_ref()
            .is_none_or(|l| l.is_ready() && l.names().next().is_none()),
    };
    let timed_out = boot.frames >= FAR_GATE_TIMEOUT_FRAMES;
    if (sky_done && models_done) || timed_out {
        if timed_out {
            warn!(
                "far: Boot released after {} frames without everything ready \
                 (skybox frames {}, models {}/{:?})",
                boot.frames, boot.skybox_frames, boot.models_dressed, boot.models_spawned
            );
        }
        boot.released = true;
        gate.release(FAR_GATE);
    }
}

// ---------------------------------------------------------------------------
// The far view's entities
// ---------------------------------------------------------------------------

/// The root of the far view (hidden by `--knobs far=off`).
#[derive(Component, Debug)]
pub struct FarView;

/// The station's frame: ships fly in it.
#[derive(Component, Debug)]
pub struct FarStation;

/// What a far anchor shows: a model placed so its `anchor` attach point sits at
/// the anchor's origin.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct FarModelSlot {
    pub model: &'static str,
    pub anchor: &'static str,
    pub scale: f32,
    pub kind: FarKind,
    pub waterfall: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FarKind {
    Station,
    Island,
    Ship,
    Planet,
}

/// On a spawned far model root.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct FarModel {
    pub kind: FarKind,
    pub waterfall: bool,
}

/// The soft glow band on the horizon.
#[derive(Component, Debug)]
pub struct HorizonGlow;

/// Spawns the far view's anchors from [`FarLayout`]: the station frame with its
/// ships, the planet and the islands. Pure (no assets), so tests can run it.
pub fn spawn_far_view(mut commands: Commands, layout: Res<FarLayout>) {
    let root = commands
        .spawn((
            Name::new("Far view"),
            FarView,
            Transform::IDENTITY,
            Visibility::default(),
        ))
        .id();
    let station = commands
        .spawn((
            Name::new("Far station"),
            FarStation,
            layout.station_frame(),
            Visibility::default(),
            slot(&layout.station, FarKind::Station, true),
            ChildOf(root),
        ))
        .id();
    for (i, spec) in layout.ships.iter().enumerate() {
        let path = Arc::new(ShipLoop::new(spec.points.clone(), spec.lap_s));
        for &start in &spec.starts {
            let flight = ShipFlight {
                path: path.clone(),
                start,
                max_bank: 0.45,
            };
            commands.spawn((
                Name::new(format!("Far ship {i}")),
                flight.transform(0.0),
                flight,
                Visibility::default(),
                FarModelSlot {
                    model: "ship",
                    anchor: "Center",
                    scale: spec.scale,
                    kind: FarKind::Ship,
                    waterfall: false,
                },
                ChildOf(station),
            ));
        }
    }
    commands.spawn((
        Name::new("Far planet"),
        Transform::from_translation(layout.planet.position)
            .with_rotation(Quat::from_rotation_y(layout.planet.yaw)),
        Visibility::default(),
        slot(&layout.planet, FarKind::Planet, false),
        ChildOf(root),
    ));
    for (i, island) in layout.islands.iter().enumerate() {
        commands.spawn((
            Name::new(format!("Far island {i}")),
            Transform::from_translation(island.piece.position)
                .with_rotation(Quat::from_rotation_y(island.piece.yaw)),
            Visibility::default(),
            Bob {
                base: island.piece.position,
                amplitude: island.bob_amplitude,
                period: island.bob_period,
                phase: island.bob_phase,
            },
            slot(&island.piece, FarKind::Island, island.waterfall),
            ChildOf(root),
        ));
    }
}

fn slot(piece: &FarPiece, kind: FarKind, waterfall: bool) -> FarModelSlot {
    FarModelSlot {
        model: piece.model,
        anchor: piece.anchor,
        scale: piece.scale,
        kind,
        waterfall,
    }
}

/// Once the model library is ready, puts each far model under its anchor.
fn attach_far_models(
    mut commands: Commands,
    library: Option<Res<ModelLibrary>>,
    slots: Query<(Entity, &FarModelSlot)>,
    mut boot: ResMut<FarBoot>,
) {
    let Some(library) = library else { return };
    if boot.models_spawned.is_some() || !library.is_ready() {
        return;
    }
    let mut spawned = 0;
    for (anchor, slot) in &slots {
        let offset = library
            .sidecar(slot.model)
            .and_then(|s| s.attach(slot.anchor))
            .map_or(Vec3::ZERO, |a| a.position());
        let transform =
            Transform::from_translation(-offset * slot.scale).with_scale(Vec3::splat(slot.scale));
        let Some(root) = spawn_model(&mut commands, &library, slot.model, transform) else {
            error!("far: model {} is missing", slot.model);
            continue;
        };
        commands.entity(root).insert((
            ModelLook::Far,
            FarModel {
                kind: slot.kind,
                waterfall: slot.waterfall,
            },
            ChildOf(anchor),
        ));
        spawned += 1;
    }
    boot.models_spawned = Some(spawned);
    info!("far: {spawned} far models placed");
}

fn apply_far_view_setting(
    settings: Res<LookSettings>,
    mut roots: Query<&mut Visibility, With<FarView>>,
) {
    let wanted = if settings.far {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut v in &mut roots {
        if settings.is_changed() || v.is_added() {
            v.set_if_neq(wanted);
        }
    }
}

// ---------------------------------------------------------------------------
// Materials and dressing
// ---------------------------------------------------------------------------

/// The far view's materials and meshes.
#[derive(Resource, Debug, Clone)]
pub struct FarAssets {
    pub station: Handle<FarMaterial>,
    pub island: Handle<FarMaterial>,
    pub planet: Handle<FarMaterial>,
    pub ship: Handle<FarMaterial>,
    /// Stained glass, one per pulse phase group ([`glass_group`]).
    pub glass: Vec<Handle<FarMaterial>>,
    /// Engine streaks and the horizon band: additive (alpha 0), unhazed.
    pub trail: Handle<FarMaterial>,
    pub horizon: Handle<FarMaterial>,
    pub waterfall: Handle<WaterfallMaterial>,
    pub horizon_mesh: Handle<Mesh>,
}

/// Pulse phases (turns) of the glass groups: the sails ripple out of step with
/// the great window; the little windows breathe on their own.
pub const GLASS_PHASES: [f32; 4] = [0.0, 0.33, 0.66, 0.5];

/// Which glass group a `Glass*` / `Glow*` part belongs to.
pub fn glass_group(part: &str) -> usize {
    let name = part
        .strip_prefix("Glass")
        .or_else(|| part.strip_prefix("Glow"))
        .unwrap_or(part);
    match name {
        "SailLeftInner" | "SailRightOuter" => 0,
        "Nave" => 1,
        "SailLeftOuter" | "SailRightInner" => 2,
        _ => 3,
    }
}

/// The stained-glass materials, one per phase group, and how each pulses:
/// brightness lifts the glass in its own vertex colors, a pale violet emissive
/// washes over it.
pub fn glass_panes(far: &mut Assets<FarMaterial>) -> Vec<GlassPane> {
    GLASS_PHASES
        .iter()
        .enumerate()
        .map(|(i, &phase)| {
            let windows = i == 3;
            GlassPane {
                material: far.add(FarMaterial::default().with_haze(0.18)),
                phase,
                brightness: if windows { (1.0, 1.25) } else { (1.0, 1.45) },
                emissive: cartoon::GLASS_VIOLET,
                strength: if windows { (0.02, 0.1) } else { (0.03, 0.22) },
            }
        })
        .collect()
}

fn additive(color: Color) -> FarMaterial {
    let c = color.to_linear();
    FarMaterial {
        // Alpha 0 turns the far material's premultiplied blend into a pure add.
        base_color: Color::linear_rgba(c.red, c.green, c.blue, 0.0),
        alpha_mode: AlphaMode::Add,
        haze: 0.0,
        ..default()
    }
}

fn create_far_assets(
    mut commands: Commands,
    mut far: ResMut<Assets<FarMaterial>>,
    mut falls: ResMut<Assets<WaterfallMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut pulse: ResMut<GlassPulse>,
    haze: Res<FarHaze>,
    layout: Res<FarLayout>,
) {
    pulse.panes = glass_panes(&mut far);
    let glass = pulse.panes.iter().map(|p| p.material.clone()).collect();
    let assets = FarAssets {
        station: far.add(FarMaterial::default().with_haze(0.5)),
        island: far.add(FarMaterial::default()),
        planet: far.add(FarMaterial::default().with_haze(0.3)),
        ship: far.add(FarMaterial::default().with_haze(0.45)),
        glass,
        trail: far.add(additive(Color::linear_rgb(1.1, 1.1, 1.2))),
        horizon: far.add(additive(Color::WHITE)),
        waterfall: falls.add(WaterfallMaterial::new(
            cartoon::WATERFALL_BLUE,
            Color::srgb(0.9, 0.97, 1.0),
            &haze,
            0.8,
        )),
        horizon_mesh: meshes.add(horizon_mesh(layout.horizon_radius)),
    };
    commands.insert_resource(assets);
}

/// Additive glow color of the horizon band at full strength (linear).
pub fn horizon_glow_color() -> LinearRgba {
    let c = cartoon::GALAXY_DEEP.to_linear();
    LinearRgba::rgb(c.red * 0.2, c.green * 0.2, c.blue * 0.2)
}

/// An open cylinder around the arena, facing in, glowing brightest at the
/// horizon and fading out above and below (vertex colors, additive).
pub fn horizon_mesh(radius: f32) -> Mesh {
    let segments = 48;
    // (height m, glow 0..1)
    let rings = [
        (-420.0, 0.0),
        (-150.0, 0.45),
        (-10.0, 1.0),
        (80.0, 0.85),
        (250.0, 0.35),
        (560.0, 0.0),
    ];
    let glow = horizon_glow_color();
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut colors = Vec::new();
    for &(y, k) in &rings {
        for i in 0..=segments {
            let a = i as f32 / segments as f32 * std::f32::consts::TAU;
            let (s, c) = a.sin_cos();
            positions.push([c * radius, y, s * radius]);
            normals.push([-c, 0.0, -s]);
            colors.push([glow.red * k, glow.green * k, glow.blue * k, 1.0]);
        }
    }
    let row = segments as u32 + 1;
    let mut indices = Vec::new();
    for j in 0..rings.len() as u32 - 1 {
        for i in 0..segments as u32 {
            let (a, b) = (j * row + i, j * row + i + 1);
            let (c, d) = (a + row, b + row);
            indices.extend([a, b, d, a, d, c]);
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices))
}

fn spawn_horizon_glow(
    mut commands: Commands,
    assets: Res<FarAssets>,
    roots: Query<Entity, With<FarView>>,
) {
    let mut e = commands.spawn((
        Name::new("Horizon glow"),
        HorizonGlow,
        Mesh3d(assets.horizon_mesh.clone()),
        MeshMaterial3d(assets.horizon.clone()),
        Transform::IDENTITY,
    ));
    if let Some(root) = roots.iter().next() {
        e.insert(ChildOf(root));
    }
}

/// A small cube with the far models' vertex layout (position, normal, COLOR_0).
fn far_layout_cube() -> Mesh {
    let mut mesh = Mesh::from(Cuboid::default());
    mesh.remove_attribute(Mesh::ATTRIBUTE_UV_0);
    let count = mesh.count_vertices();
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_COLOR,
        VertexAttributeValues::Float32x4(vec![[1.0; 4]; count]),
    );
    mesh
}

/// Warms the far view's own pipeline variants: waterfalls and additive far
/// surfaces (opaque far surfaces and halos are warmed by `look`).
fn warm_far_variants(mut warmup: Warmup, mut meshes: ResMut<Assets<Mesh>>, assets: Res<FarAssets>) {
    let cube = meshes.add(far_layout_cube());
    warmup.add(cube.clone(), assets.waterfall.clone());
    warmup.add(cube, assets.trail.clone());
}

/// Gives far models their part materials and halos once the look has dressed
/// them: glass, waterfalls, engine streaks, per-kind haze, and halos at the
/// `Glow*`, `Mist*` and `Engine` attach points.
fn dress_far_models(
    mut commands: Commands,
    mut dressed: MessageReader<ModelDressed>,
    models: Query<&FarModel>,
    children: Query<&Children>,
    names: Query<&Name>,
    far_meshes: Query<(), With<MeshMaterial3d<FarMaterial>>>,
    assets: Option<Res<FarAssets>>,
    library: Option<Res<ModelLibrary>>,
    mut boot: ResMut<FarBoot>,
) {
    let Some(assets) = assets else { return };
    for event in dressed.read() {
        let Ok(model) = models.get(event.root) else {
            continue;
        };
        boot.models_dressed += 1;
        for node in children.iter_descendants(event.root) {
            let Ok(name) = names.get(node) else { continue };
            let part = name.as_str();
            if part.contains('.') {
                continue; // a mesh primitive: dressed through its node
            }
            let dress = part_dress(model, part, &assets);
            if dress == PartDress::Hide {
                commands.entity(node).insert(Visibility::Hidden);
                continue;
            }
            for &mesh in children.get(node).into_iter().flatten() {
                if !far_meshes.contains(mesh) {
                    continue;
                }
                let mut e = commands.entity(mesh);
                match &dress {
                    PartDress::Far(material) => {
                        e.insert(MeshMaterial3d(material.clone()));
                    }
                    PartDress::Waterfall => {
                        e.remove::<MeshMaterial3d<FarMaterial>>()
                            .insert(MeshMaterial3d(assets.waterfall.clone()));
                    }
                    PartDress::Hide => {}
                }
            }
        }
        if let Some(side) = library.as_ref().and_then(|l| l.sidecar(&event.name)) {
            for (point, attach) in &side.attach {
                if let Some((halo, glow)) = attach_halo(model, point) {
                    let mut e = commands.spawn((
                        Name::new(format!("Far halo {point}")),
                        halo,
                        Transform::from_translation(attach.position()),
                        ChildOf(event.root),
                    ));
                    if let Some(glow) = glow {
                        e.insert(glow);
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum PartDress {
    Far(Handle<FarMaterial>),
    Waterfall,
    Hide,
}

fn part_dress(model: &FarModel, part: &str, assets: &FarAssets) -> PartDress {
    if part.starts_with("Waterfall") {
        return if model.waterfall {
            PartDress::Waterfall
        } else {
            PartDress::Hide
        };
    }
    match model.kind {
        FarKind::Station if part.starts_with("Glass") => {
            PartDress::Far(assets.glass[glass_group(part)].clone())
        }
        FarKind::Station => PartDress::Far(assets.station.clone()),
        FarKind::Island => PartDress::Far(assets.island.clone()),
        FarKind::Ship if part == "Trail" => PartDress::Far(assets.trail.clone()),
        FarKind::Ship => PartDress::Far(assets.ship.clone()),
        FarKind::Planet => PartDress::Far(assets.planet.clone()),
    }
}

/// The halo an attach point carries (sizes in model metres).
fn attach_halo(model: &FarModel, point: &str) -> Option<(Halo, Option<GlassGlow>)> {
    let glow = |phase: f32, low: f32, high: f32| GlassGlow { phase, low, high };
    if let Some(sail) = point.strip_prefix("Glow") {
        let phase = GLASS_PHASES[glass_group(point)];
        let (size, color) = match sail {
            "Nave" => (70.0, cartoon::GLASS_YELLOW),
            s if s.ends_with("Inner") => (120.0, cartoon::GLASS_VIOLET),
            _ => (100.0, cartoon::GLASS_CYAN),
        };
        return Some((Halo::new(color, size, 0.3), Some(glow(phase, 0.18, 0.55))));
    }
    if point.starts_with("Mist") {
        if !model.waterfall {
            return None;
        }
        let size = if model.kind == FarKind::Station {
            40.0
        } else {
            16.0
        };
        return Some((Halo::new(cartoon::WATERFALL_BLUE, size, 0.9), None));
    }
    if point == "Engine" {
        return Some((Halo::new(cartoon::CRYSTAL_BLUE, 9.0, 2.2), None));
    }
    if point == "Center" && model.kind == FarKind::Planet {
        // A thin atmosphere so the planet stands off the sky.
        return Some((Halo::new(cartoon::GLASS_VIOLET, 250.0, 0.3), None));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glass_parts_and_their_glows_share_a_phase_group() {
        for suffix in [
            "Nave",
            "SailLeftInner",
            "SailLeftOuter",
            "SailRightInner",
            "SailRightOuter",
        ] {
            assert_eq!(
                glass_group(&format!("Glass{suffix}")),
                glass_group(&format!("Glow{suffix}"))
            );
        }
        assert_eq!(glass_group("GlassWindows"), 3);
        assert_ne!(glass_group("GlassNave"), glass_group("GlassSailLeftInner"));
    }

    #[test]
    fn the_horizon_band_faces_inward_and_glows_at_the_horizon() {
        let mesh = horizon_mesh(1000.0);
        let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions")
        };
        let Some(Indices::U32(idx)) = mesh.indices() else {
            panic!("indices")
        };
        for tri in idx.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|k| Vec3::from_array(pos[tri[k] as usize]));
            let n = (b - a).cross(c - a);
            let centre = (a + b + c) / 3.0;
            if n.length() > 1e-3 {
                assert!(n.dot(Vec3::new(centre.x, 0.0, centre.z)) < 0.0, "faces in");
            }
        }
        let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("colors")
        };
        let brightest = pos
            .iter()
            .zip(colors)
            .max_by(|a, b| a.1[2].total_cmp(&b.1[2]))
            .unwrap();
        assert!(brightest.0[1].abs() < 20.0, "peak at the horizon");
    }

    #[test]
    fn additive_far_materials_output_alpha_zero() {
        let m = additive(Color::WHITE);
        assert_eq!(m.base_color.alpha(), 0.0);
        assert_eq!(m.haze, 0.0);
        assert_eq!(m.alpha_mode, AlphaMode::Add);
    }
}
