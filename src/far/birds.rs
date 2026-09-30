//! Birds circling the castle (M4 chunk 6, D123): a few flocks of small dark
//! silhouettes wheeling round the station's spires, so the far view is never
//! still.
//!
//! Every flock is one entity drawing the same small merged mesh (a loose V of
//! [`BIRDS_PER_FLOCK`] birds, each a pair of swept wings seen from both sides)
//! with one far material, so all the flocks batch into one instanced draw of
//! [`bird_triangles`] triangles. [`fly_birds`] moves them on [`SkyClock`]:
//! round a tilted circle about the castle, banked into the turn, the flock's
//! wings flapping (a vertical scale on its transform). Nothing allocates per
//! frame; it all runs headless.

use super::{FarLayout, FarView, SkyClock};
use crate::look::FarMaterial;
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use std::f32::consts::TAU;

/// Flocks round the castle, and birds in each.
pub const FLOCKS: usize = 4;
pub const BIRDS_PER_FLOCK: usize = 7;
/// A bird's wingspan (m): at the castle's 600+ m, a few pixels across.
pub const WINGSPAN: f32 = 7.0;

/// A flock's circuit round the castle.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct BirdFlock {
    /// The circle's centre (the castle, raised).
    pub centre: Vec3,
    pub radius: f32,
    /// Seconds per lap (negative laps the other way).
    pub period: f32,
    /// Where on the lap it starts (radians).
    pub phase: f32,
    /// The circle's tilt: up and down this much (m) over a lap.
    pub tilt: f32,
    /// Wingbeats per second.
    pub flap_hz: f32,
}

impl BirdFlock {
    /// The flock's transform at `seconds`: on its circle, facing along it,
    /// banked into the turn, wings at their beat.
    pub fn transform(&self, seconds: f64) -> Transform {
        let turns = (seconds / self.period.abs() as f64).rem_euclid(1.0) as f32;
        let dir = self.period.signum();
        let a = self.phase + dir * turns * TAU;
        let (s, c) = a.sin_cos();
        let at = self.centre
            + Vec3::new(
                c * self.radius,
                self.tilt * (a * 1.0 + 0.7).sin(),
                s * self.radius,
            );
        // Heading: the circle's tangent in the direction of travel.
        let heading = Vec3::new(-s, 0.0, c) * dir;
        let bank = Quat::from_rotation_z(-0.35 * dir);
        let rotation = Transform::IDENTITY.looking_to(heading, Vec3::Y).rotation * bank;
        let beat = (seconds as f32 * self.flap_hz * TAU + self.phase * 3.0).sin();
        Transform::from_translation(at)
            .with_rotation(rotation)
            .with_scale(Vec3::new(1.0, 0.35 + 0.65 * beat, 1.0))
    }
}

/// The flocks for `layout`: round the castle at different heights, radii,
/// speeds and directions.
pub fn flocks(layout: &FarLayout) -> [BirdFlock; FLOCKS] {
    let castle = layout.station.position;
    let spec = [
        (170.0, 150.0, 46.0, 0.0, 18.0, 3.1),
        (240.0, 210.0, -58.0, 2.1, 30.0, 2.6),
        (130.0, 270.0, 38.0, 4.0, 12.0, 3.4),
        (300.0, 110.0, -72.0, 5.2, 24.0, 2.8),
    ];
    spec.map(|(radius, up, period, phase, tilt, flap_hz)| BirdFlock {
        centre: castle + Vec3::Y * up,
        radius,
        period,
        phase,
        tilt,
        flap_hz,
    })
}

/// Triangles in one flock's mesh.
pub fn bird_triangles() -> usize {
    BIRDS_PER_FLOCK * 4
}

/// One flock's mesh: a loose V of birds (-Z forward, +Y up), each two swept
/// wings from a body point, both windings so they show from below and above.
/// Wing tips sit above the body, so scaling Y flaps them.
pub fn flock_mesh() -> Mesh {
    let colour = crate::palette::cartoon::FAR_STONE_DARK.to_linear();
    let c = [colour.red, colour.green, colour.blue, 1.0];
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    let half = WINGSPAN * 0.5;
    for b in 0..BIRDS_PER_FLOCK {
        // A V: the leader ahead, the others trailing out to each side.
        let rank = b.div_ceil(2) as f32;
        let side = if b % 2 == 0 { 1.0 } else { -1.0 };
        let body = Vec3::new(
            side * rank * 11.0,
            (b as f32 * 1.7).sin() * 3.0,
            rank * 13.0,
        );
        let nose = body + Vec3::new(0.0, 0.0, -WINGSPAN * 0.25);
        let tail = body + Vec3::new(0.0, 0.0, WINGSPAN * 0.2);
        let left = body + Vec3::new(-half, half * 0.45, WINGSPAN * 0.12);
        let right = body + Vec3::new(half, half * 0.45, WINGSPAN * 0.12);
        let base = positions.len() as u32;
        positions.extend([nose, tail, left, right].map(|p| p.to_array()));
        // Each wing a triangle (nose, tail, tip), front and back.
        indices.extend([base, base + 2, base + 1, base, base + 1, base + 2]);
        indices.extend([base, base + 1, base + 3, base, base + 3, base + 1]);
    }
    let n = positions.len();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; n])
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; n])
    .with_inserted_indices(Indices::U32(indices))
}

/// Marks a flock entity.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Bird;

/// Spawns the flocks: one shared mesh and material for all of them.
pub fn spawn_birds(
    mut commands: Commands,
    layout: Res<FarLayout>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FarMaterial>>,
    roots: Query<Entity, With<FarView>>,
) {
    let root = roots.iter().next();
    let mesh = meshes.add(flock_mesh());
    // Dark against the lit sky, softened by the haze like the castle.
    let material = materials.add(FarMaterial::default().with_haze(0.8));
    for flock in flocks(&layout) {
        let mut e = commands.spawn((
            Name::new("Bird flock"),
            Bird,
            flock,
            flock.transform(0.0),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
        ));
        // Under the far view's root: drawn with the far layer (and hidden
        // with it by `far=off`).
        if let Some(root) = root {
            e.insert(ChildOf(root));
        }
    }
}

/// Flies every flock round its circuit.
pub fn fly_birds(clock: Res<SkyClock>, mut flocks: Query<(&BirdFlock, &mut Transform)>) {
    for (flock, mut transform) in &mut flocks {
        *transform = flock.transform(clock.seconds);
    }
}
