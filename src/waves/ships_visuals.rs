//! The drop ships' look and sound (client only; docs/M3-SPEC.md → Ship
//! arrivals, D82). Everything here reads [`Ships`] (the simulation's sorties,
//! pure functions of their clocks) and draws it, interpolated between fixed
//! ticks:
//!
//! - **the ship**: the `dropship` model (`art/blender/assets/dropship.py`),
//!   toon-shaded and inked like the knights, facing along its flight and
//!   nosing down as it dives, with its hull crystal glowing violet (a halo on
//!   its `Beam` attach point);
//! - **the beam**: a violet cone of light from the crystal to the ground while
//!   knights come down, fading in and out ([`beam_level`]);
//! - **the rune circle**: a glowing ring of runes on the landing spot, lit 2 s
//!   before the first knight lands and turning slowly ([`Sortie::telegraph`]);
//! - **the landing poof**: a sparkle flash where each knight touches down;
//! - **sounds**: the rising hum as the circle lights, a shimmer as each knight
//!   starts down the beam, and the yelp and slide whistle of a knight knocked
//!   into the void (`GameCue::VoidFall`).
//!
//! Pools: [`MAX_SHIPS`] ship models, beams and circles and three landing
//! flashes per ship, made at startup and hidden while unused; one glow
//! material ([`SpellMaterial`], tinted per entity through its `MeshTag`) and
//! one crystal material. Their pipelines are warmed behind the loading screen.

use super::ships::{CRYSTAL_DROP, MAX_ABOARD, MAX_SHIPS, SeatState, Ships, Sortie};
use crate::{
    app::BootGate,
    audio::{PlayQueue, Sfx, play_queued},
    fx::material::{SpellMaterial, glow_tag},
    look::{Halo, ModelDressed, NoOutline, Outline, ToonMaterial, warmup::Warmup},
    models::{ModelLibrary, ModelParts, spawn_model},
    shared::{AppState, GameCue, SimTick, TICK_SECONDS},
};
use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    mesh::{Indices, MeshTag, PrimitiveTopology},
    prelude::*,
};
use std::f32::consts::TAU;

/// The model's name in `assets/models/manifest.json`.
pub const DROPSHIP_MODEL: &str = "dropship";
/// The [`BootGate`] key held until the ships are dressed and warmed.
pub const SHIPS_GATE: &str = "ships";
/// Never hold Boot for the ships longer than this many frames.
pub const SHIPS_GATE_TIMEOUT: u32 = 600;
/// The beam's radius on the ground (m): wide enough for all three lanes.
pub const BEAM_RADIUS: f32 = 3.3;
/// ...and where it leaves the crystal (m).
pub const BEAM_TOP_RADIUS: f32 = 0.45;
/// The rune circle's radius (m), and how fast it turns (rad/s).
pub const CIRCLE_RADIUS: f32 = 3.4;
pub const CIRCLE_SPIN: f32 = 0.5;
/// Glow tints: the knights' violet magic (never the player's blue, never the
/// orbs' orange).
pub const BEAM_TINT: Color = Color::srgb(0.86, 0.62, 1.0);
pub const CIRCLE_TINT: Color = Color::srgb(1.0, 0.55, 0.95);
pub const CRYSTAL_GLOW: Color = Color::srgb(0.72, 0.38, 1.0);
/// The beam's and circle's peak glow intensities.
pub const BEAM_INTENSITY: f32 = 1.4;
pub const CIRCLE_INTENSITY: f32 = 2.0;
/// The landing flash: its length (s), colour and size (m).
pub const POOF_SECONDS: f32 = 0.45;
pub const POOF_COLOR: Color = Color::srgb(1.0, 0.85, 1.0);
pub const POOF_SIZE: f32 = 2.6;
/// How far the ship noses down (or up) into its dive (fraction of the slope).
pub const DIVE_PITCH: f32 = 0.45;

/// The beam's brightness (0..=1) at `t` seconds into `sortie`: up over 0.15 s
/// as the ship arrives, down over 0.15 s as the last knight lands.
pub fn beam_level(sortie: &Sortie, t: f32) -> f32 {
    if !sortie.beam_on(t) {
        return 0.0;
    }
    let on = super::ships::FLIGHT_SECONDS - 0.15;
    let off = sortie.last_landing() + 0.15;
    ((t - on) / 0.15).min((off - t) / 0.15).clamp(0.0, 1.0)
}

/// The ship's rotation at `t`: facing its heading, nosing into its climb or
/// dive.
pub fn ship_rotation(sortie: &Sortie, t: f32) -> Quat {
    let heading = sortie.heading(t);
    let v = sortie.position(t + 0.05) - sortie.position(t);
    let slope = if v.with_y(0.0).length() > 1e-3 {
        (v.y / v.with_y(0.0).length()).clamp(-1.5, 1.5)
    } else {
        0.0
    };
    let dir = (heading + Vec3::Y * slope * DIVE_PITCH).normalize_or(heading);
    Transform::default().looking_to(dir, Vec3::Y).rotation
}

/// The shared meshes and materials.
#[derive(Resource, Debug, Clone)]
pub struct ShipFxAssets {
    pub glow: Handle<SpellMaterial>,
    pub crystal: Handle<ToonMaterial>,
    pub beam: Handle<Mesh>,
    pub circle: Handle<Mesh>,
}

/// One pooled ship's visual parts, for slot `slot` of [`Ships`].
#[derive(Component, Debug, Clone, Copy)]
pub struct ShipModel {
    pub slot: usize,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct ShipBeam {
    pub slot: usize,
}

#[derive(Component, Debug, Clone, Copy)]
pub struct ShipCircle {
    pub slot: usize,
}

/// A landing flash for seat `seat` of slot `slot`.
#[derive(Component, Debug, Clone, Copy)]
pub struct ShipPoof {
    pub slot: usize,
    pub seat: usize,
}

/// The ship models have been spawned (or can't be).
#[derive(Resource, Debug, Default)]
struct ShipModels {
    spawned: bool,
    dressed: usize,
    warmed: bool,
}

pub struct ShipsVisualsPlugin;

impl Plugin for ShipsVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BootGate>();
        app.world_mut().resource_mut::<BootGate>().hold(SHIPS_GATE);
        app.add_message::<ModelDressed>()
            .add_message::<GameCue>()
            .init_resource::<Ships>()
            .init_resource::<ShipModels>()
            .add_systems(Startup, setup_ship_fx)
            .add_systems(
                Update,
                (
                    spawn_ship_models,
                    dress_ship_models,
                    release_ships_gate,
                    ship_sounds.before(play_queued),
                ),
            )
            .add_systems(PostUpdate, pose_ships.before(TransformSystems::Propagate));
    }
}

// ---------------------------------------------------------------------------
// Meshes
// ---------------------------------------------------------------------------

fn rgba(c: Color, a: f32) -> [f32; 4] {
    let l = c.to_linear();
    [l.red, l.green, l.blue, a]
}

/// A unit beam: an open cone from radius [`BEAM_TOP_RADIUS`]/[`BEAM_RADIUS`]
/// at y = 0 down to radius 1 at y = -1, with a brighter narrow core inside.
/// Vertex alpha scales the glow: bright under the crystal, softer below.
pub fn beam_mesh() -> Mesh {
    let sides = 20;
    let top = BEAM_TOP_RADIUS / BEAM_RADIUS;
    let mut positions = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    let white = Color::srgb(1.0, 0.95, 1.0);
    for (r_top, r_bottom, top_color, bottom_color) in [
        (top, 1.0, rgba(BEAM_TINT, 0.9), rgba(BEAM_TINT, 0.35)),
        (top * 0.5, 0.35, rgba(white, 1.0), rgba(BEAM_TINT, 0.3)),
    ] {
        let base = positions.len() as u32;
        for i in 0..=sides {
            let a = TAU * i as f32 / sides as f32;
            let (s, c) = a.sin_cos();
            positions.push([c * r_top, 0.0, s * r_top]);
            positions.push([c * r_bottom, -1.0, s * r_bottom]);
            // Faint vertical ribs: every other column a little brighter.
            let rib = if i % 2 == 0 { 1.0 } else { 0.7 };
            colors.push([top_color[0], top_color[1], top_color[2], top_color[3] * rib]);
            colors.push([
                bottom_color[0],
                bottom_color[1],
                bottom_color[2],
                bottom_color[3] * rib,
            ]);
        }
        for i in 0..sides {
            let k = base + 2 * i;
            indices.extend_from_slice(&[k, k + 1, k + 2, k + 2, k + 1, k + 3]);
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices))
}

/// A unit rune circle, flat on y = 0: an outer ring, an inner ring, and a
/// band of rune marks between them (diamonds and bars), with a small star at
/// the centre.
pub fn circle_mesh() -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let gold = Color::srgb(1.0, 0.82, 0.45);
    let mut ring = |r0: f32, r1: f32, sides: u32, color: [f32; 4]| {
        let base = positions.len() as u32;
        for i in 0..=sides {
            let a = TAU * i as f32 / sides as f32;
            let (s, c) = a.sin_cos();
            positions.push([c * r0, 0.0, s * r0]);
            positions.push([c * r1, 0.0, s * r1]);
            colors.push(color);
            colors.push(color);
        }
        for i in 0..sides {
            let k = base + 2 * i;
            indices.extend_from_slice(&[k, k + 1, k + 2, k + 2, k + 1, k + 3]);
        }
    };
    ring(0.9, 1.0, 40, rgba(CIRCLE_TINT, 1.0));
    ring(0.62, 0.67, 32, rgba(CIRCLE_TINT, 0.8));
    ring(0.3, 0.33, 20, rgba(gold, 0.7));
    // Runes: twelve marks round the band, alternating diamonds and bars.
    for i in 0..12 {
        let a = TAU * i as f32 / 12.0;
        let (s, c) = a.sin_cos();
        let radial = Vec3::new(c, 0.0, s);
        let tangent = Vec3::new(-s, 0.0, c);
        let mid = radial * 0.785;
        let color = rgba(gold, 0.95);
        let base = positions.len() as u32;
        let quad = if i % 2 == 0 {
            [
                mid + radial * 0.08,
                mid + tangent * 0.04,
                mid - radial * 0.08,
                mid - tangent * 0.04,
            ]
        } else {
            [
                mid + radial * 0.07 + tangent * 0.015,
                mid - radial * 0.07 + tangent * 0.015,
                mid - radial * 0.07 - tangent * 0.015,
                mid + radial * 0.07 - tangent * 0.015,
            ]
        };
        for p in quad {
            positions.push(p.to_array());
            colors.push(color);
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    // A four-point star in the middle.
    let base = positions.len() as u32;
    positions.push([0.0, 0.0, 0.0]);
    colors.push(rgba(Color::WHITE, 1.0));
    for i in 0..8 {
        let a = TAU * i as f32 / 8.0;
        let r = if i % 2 == 0 { 0.2 } else { 0.06 };
        positions.push([a.cos() * r, 0.0, a.sin() * r]);
        colors.push(rgba(CIRCLE_TINT, 0.6));
    }
    for i in 0..8 {
        indices.extend_from_slice(&[base, base + 1 + i, base + 1 + (i + 1) % 8]);
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices))
}

// ---------------------------------------------------------------------------
// Setup and warm-up
// ---------------------------------------------------------------------------

fn setup_ship_fx(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut spell: ResMut<Assets<SpellMaterial>>,
    mut toon: ResMut<Assets<ToonMaterial>>,
    mut warmup: Warmup,
) {
    let assets = ShipFxAssets {
        glow: spell.add(SpellMaterial::default()),
        crystal: toon.add(
            ToonMaterial::vertex_colored()
                .with_emissive(CRYSTAL_GLOW, 1.6)
                .with_rim(1.0),
        ),
        beam: meshes.add(beam_mesh()),
        circle: meshes.add(circle_mesh()),
    };
    for slot in 0..MAX_SHIPS {
        commands.spawn((
            Name::new("Drop ship beam"),
            ShipBeam { slot },
            Mesh3d(assets.beam.clone()),
            MeshMaterial3d(assets.glow.clone()),
            glow_tag(BEAM_TINT, 0.0),
            Transform::default(),
            Visibility::Hidden,
            NotShadowCaster,
            NoOutline,
        ));
        commands.spawn((
            Name::new("Drop ship rune circle"),
            ShipCircle { slot },
            Mesh3d(assets.circle.clone()),
            MeshMaterial3d(assets.glow.clone()),
            glow_tag(CIRCLE_TINT, 0.0),
            Halo::new(CIRCLE_TINT, CIRCLE_RADIUS * 1.2, 0.0),
            Transform::default(),
            Visibility::Hidden,
            NotShadowCaster,
            NoOutline,
        ));
        for seat in 0..MAX_ABOARD {
            commands.spawn((
                Name::new("Drop ship landing poof"),
                ShipPoof { slot, seat },
                Halo::new(POOF_COLOR, POOF_SIZE, 0.0),
                Transform::default(),
                Visibility::Hidden,
            ));
        }
    }
    // The beam's and circle's glow, compiled behind the loading screen.
    warmup.add_with(
        assets.beam.clone(),
        assets.glow.clone(),
        glow_tag(BEAM_TINT, 1.0),
    );
    warmup.add_with(
        assets.circle.clone(),
        assets.glow.clone(),
        glow_tag(CIRCLE_TINT, 1.0),
    );
    commands.insert_resource(assets);
}

/// Spawns the pooled ship models (hidden) once the model library exists.
fn spawn_ship_models(
    mut commands: Commands,
    library: Option<Res<ModelLibrary>>,
    mut state: ResMut<ShipModels>,
) {
    let Some(library) = library else {
        return;
    };
    if state.spawned {
        return;
    }
    state.spawned = true;
    for slot in 0..MAX_SHIPS {
        let Some(model) = spawn_model(&mut commands, &library, DROPSHIP_MODEL, Transform::IDENTITY)
        else {
            warn!("drop ships: no `{DROPSHIP_MODEL}` model in the library");
            return;
        };
        commands.entity(model).insert((
            ShipModel { slot },
            Outline::default(),
            Visibility::Hidden,
            NotShadowCaster,
        ));
    }
}

/// Once a ship is dressed: the crystal's glow material and a halo under the
/// hull; the first one also warms its meshes behind the loading screen.
fn dress_ship_models(
    mut commands: Commands,
    mut dressed: MessageReader<ModelDressed>,
    parts: ModelParts,
    children: Query<&Children>,
    meshes: Query<(&Mesh3d, Option<&MeshMaterial3d<ToonMaterial>>)>,
    assets: Option<Res<ShipFxAssets>>,
    mut state: ResMut<ShipModels>,
    mut warmup: Warmup,
) {
    let Some(assets) = assets else {
        dressed.clear();
        return;
    };
    for event in dressed.read() {
        if event.name != DROPSHIP_MODEL {
            continue;
        }
        let crystal = parts.find(event.root, "Crystal").and_then(|node| {
            std::iter::once(node)
                .chain(children.iter_descendants(node))
                .find(|e| meshes.contains(*e))
        });
        if let Some(crystal) = crystal {
            commands
                .entity(crystal)
                .insert(MeshMaterial3d(assets.crystal.clone()));
        }
        if let Some(beam) = parts.find(event.root, "Beam") {
            commands
                .entity(beam)
                .insert(Halo::new(CRYSTAL_GLOW, 3.0, 1.2));
        }
        if !state.warmed {
            state.warmed = true;
            for e in children.iter_descendants(event.root) {
                let Ok((mesh, material)) = meshes.get(e) else {
                    continue;
                };
                let material = if Some(e) == crystal {
                    assets.crystal.clone()
                } else if let Some(m) = material {
                    m.0.clone()
                } else {
                    continue;
                };
                warmup.add_with(mesh.0.clone(), material, Outline::default());
            }
        }
        state.dressed += 1;
    }
}

/// Lets Boot finish once every ship is dressed (and its warm-up registered),
/// or there's nothing to wait for.
fn release_ships_gate(
    mut gate: ResMut<BootGate>,
    library: Option<Res<ModelLibrary>>,
    state: Res<ShipModels>,
    mut frames: Local<u32>,
) {
    if !gate.held().any(|k| k == SHIPS_GATE) {
        return;
    }
    *frames += 1;
    let done = match library {
        None => *frames > 2,
        Some(library) => {
            let missing = library.failed().iter().any(|f| f == DROPSHIP_MODEL)
                || (library.is_ready() && library.get(DROPSHIP_MODEL).is_none());
            library.is_ready() && (missing || state.dressed >= MAX_SHIPS)
        }
    };
    if done || *frames > SHIPS_GATE_TIMEOUT {
        if !done {
            warn!("drop ships: the models weren't dressed in time; not holding Boot");
        }
        gate.release(SHIPS_GATE);
    }
}

// ---------------------------------------------------------------------------
// Per frame
// ---------------------------------------------------------------------------

/// Seconds into each slot's sortie this frame (between fixed ticks).
fn clock(ships: &Ships, tick: u64, alpha: f32) -> [Option<(Sortie, f32)>; MAX_SHIPS] {
    std::array::from_fn(|slot| {
        ships.slots[slot].map(|s| (s, s.seconds(tick) + alpha * TICK_SECONDS))
    })
}

#[allow(clippy::type_complexity)]
fn pose_ships(
    ships: Option<Res<Ships>>,
    tick: Option<Res<SimTick>>,
    fixed: Res<Time<Fixed>>,
    time: Res<Time>,
    state: Res<State<AppState>>,
    mut models: Query<
        (&ShipModel, &mut Transform, &mut Visibility),
        (Without<ShipBeam>, Without<ShipCircle>, Without<ShipPoof>),
    >,
    mut beams: Query<
        (&ShipBeam, &mut Transform, &mut Visibility, &mut MeshTag),
        (Without<ShipCircle>, Without<ShipPoof>),
    >,
    mut circles: Query<
        (
            &ShipCircle,
            &mut Transform,
            &mut Visibility,
            &mut MeshTag,
            &mut Halo,
        ),
        Without<ShipPoof>,
    >,
    mut poofs: Query<(&ShipPoof, &mut Transform, &mut Visibility, &mut Halo)>,
) {
    let (Some(ships), Some(tick)) = (ships, tick) else {
        return;
    };
    let alpha = if *state.get() == AppState::Playing {
        fixed.overstep_fraction().clamp(0.0, 1.0)
    } else {
        0.0
    };
    let now = clock(&ships, tick.0, alpha);
    let spin = time.elapsed_secs_wrapped() * CIRCLE_SPIN;

    for (ship, mut transform, mut visibility) in &mut models {
        match now[ship.slot] {
            Some((sortie, t)) => {
                transform.translation = sortie.position(t);
                transform.rotation = ship_rotation(&sortie, t);
                visibility.set_if_neq(Visibility::Inherited);
            }
            None => {
                visibility.set_if_neq(Visibility::Hidden);
            }
        }
    }
    for (beam, mut transform, mut visibility, mut tag) in &mut beams {
        let level = now[beam.slot].map_or(0.0, |(s, t)| beam_level(&s, t));
        let Some((sortie, t)) = now[beam.slot].filter(|_| level > 0.0) else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        let crystal = sortie.position(t) - Vec3::Y * CRYSTAL_DROP;
        let ground = sortie.drop_point().at.y;
        let height = (crystal.y - ground).max(0.1);
        transform.translation = crystal;
        transform.rotation = Quat::from_rotation_y(-spin * 0.6);
        transform.scale = Vec3::new(BEAM_RADIUS, height, BEAM_RADIUS);
        let want = glow_tag(BEAM_TINT, BEAM_INTENSITY * level);
        if *tag != want {
            *tag = want;
        }
        visibility.set_if_neq(Visibility::Inherited);
    }
    for (circle, mut transform, mut visibility, mut tag, mut halo) in &mut circles {
        let level = now[circle.slot].map_or(0.0, |(s, t)| s.telegraph(t));
        let Some((sortie, _)) = now[circle.slot].filter(|_| level > 0.0) else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        transform.translation = sortie.drop_point().at + Vec3::Y * 0.04;
        transform.rotation = Quat::from_rotation_y(spin);
        transform.scale = Vec3::splat(CIRCLE_RADIUS * (0.6 + 0.4 * level));
        let want = glow_tag(CIRCLE_TINT, CIRCLE_INTENSITY * level);
        if *tag != want {
            *tag = want;
        }
        let want_halo = Halo::new(CIRCLE_TINT, CIRCLE_RADIUS * 1.2, 0.6 * level);
        if *halo != want_halo {
            *halo = want_halo;
        }
        visibility.set_if_neq(Visibility::Inherited);
    }
    for (poof, mut transform, mut visibility, mut halo) in &mut poofs {
        let flash = now[poof.slot].and_then(|(sortie, t)| {
            let seat = sortie.seats[poof.seat];
            let since = t - sortie.landing(poof.seat);
            (poof.seat < sortie.count
                && seat.state == SeatState::Landed
                && (0.0..POOF_SECONDS).contains(&since))
            .then_some((seat.spot, since / POOF_SECONDS))
        });
        let Some((spot, k)) = flash else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        transform.translation = spot + Vec3::Y * 0.9;
        let fade = (1.0 - k) * (1.0 - k);
        let want = Halo::new(POOF_COLOR, POOF_SIZE * (0.6 + 0.8 * k), 2.2 * fade);
        if *halo != want {
            *halo = want;
        }
        visibility.set_if_neq(Visibility::Inherited);
    }
}

/// What a slot's sounds have already played for.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Heard {
    launch: Option<u64>,
    hum: bool,
    beams: [bool; MAX_ABOARD],
}

/// The ships' and the void's sounds.
fn ship_sounds(
    time: Res<Time<Real>>,
    ships: Option<Res<Ships>>,
    tick: Option<Res<SimTick>>,
    queue: Option<ResMut<PlayQueue>>,
    transforms: Query<&Transform>,
    mut cues: MessageReader<GameCue>,
    mut heard: Local<[Heard; MAX_SHIPS]>,
) {
    let (Some(ships), Some(tick), Some(mut queue)) = (ships, tick, queue) else {
        cues.clear();
        return;
    };
    let now = time.elapsed_secs_f64();
    for (slot, entry) in ships.slots.iter().enumerate() {
        let Some(sortie) = entry else {
            heard[slot] = Heard::default();
            continue;
        };
        let memory = &mut heard[slot];
        if memory.launch != Some(sortie.launch_tick) {
            *memory = Heard {
                launch: Some(sortie.launch_tick),
                ..default()
            };
        }
        let t = sortie.seconds(tick.0);
        if !memory.hum && sortie.telegraph(t) > 0.0 {
            memory.hum = true;
            queue.push(Sfx::ShipHum, Some(sortie.drop_point().at + Vec3::Y), now);
        }
        for k in 0..sortie.count {
            if !memory.beams[k] && sortie.seats[k].state != SeatState::Aboard && !sortie.cancelled {
                memory.beams[k] = true;
                let crystal = sortie.hover_point() - Vec3::Y * CRYSTAL_DROP;
                queue.push(Sfx::ShipBeam, Some(crystal), now);
            }
        }
    }
    for cue in cues.read() {
        if let GameCue::VoidFall { who } = *cue
            && let Ok(t) = transforms.get(who)
        {
            queue.push(Sfx::VoidYelp, Some(t.translation + Vec3::Y), now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::waves::ships::DROP_POINTS;

    #[test]
    fn the_beam_fades_in_as_the_ship_arrives_and_out_after_the_last_landing() {
        let s = Sortie::new(0, 1, 13.0, 0.0, 2);
        assert_eq!(beam_level(&s, 3.0), 0.0);
        assert_eq!(beam_level(&s, 4.5), 1.0);
        assert!(beam_level(&s, s.last_landing() + 0.1) < 0.5);
        assert_eq!(beam_level(&s, s.last_landing() + 0.2), 0.0);
    }

    #[test]
    fn the_meshes_are_small_and_every_drop_point_has_a_circle() {
        let count = |m: &Mesh| match m.indices() {
            Some(Indices::U32(i)) => i.len() / 3,
            _ => 0,
        };
        assert!(count(&beam_mesh()) <= 100);
        assert!(count(&circle_mesh()) <= 300);
        assert_eq!(DROP_POINTS.len(), 6);
    }
}
