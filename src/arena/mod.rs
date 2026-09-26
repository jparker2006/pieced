//! Arena layout: the collision geometry, spawn points and solid props every
//! gameplay slice relies on. Runs headless. Visuals live in [`visuals`] (client
//! only).
//!
//! **Props (Milestone 2, D29).** A handful of cartoon rocks (crouch cover) and
//! stumps (jumpable) stand in the arena at the fixed spots in [`ARENA_PROPS`].
//! Each is a static World-layer collider, so props block movement and shots,
//! while building ignores them (pieces may intersect a prop, like building
//! through terrain). None is within [`PROP_CLEARANCE`] of either spawn or an
//! initial cover piece, and none is in the dummy's strafe zone
//! ([`in_dummy_strafe_zone`]).

pub mod visuals;

use crate::shared::{ARENA_HALF, Facing, Layer, LookAngles};
use avian3d::prelude::*;
use bevy::prelude::*;

/// Where things start, and the playable bounds (feet positions are clamped inside).
#[derive(Resource, Debug, Clone)]
pub struct ArenaLayout {
    pub player_spawn: Vec3,
    pub player_look: LookAngles,
    pub dummy_spawn: Vec3,
    /// Inclusive min/max of walkable XZ for character feet.
    pub bounds_min: Vec2,
    pub bounds_max: Vec2,
}

impl Default for ArenaLayout {
    fn default() -> Self {
        let inset = 0.5;
        Self {
            player_spawn: Vec3::new(2.0, 0.0, 14.0),
            player_look: LookAngles {
                yaw: Facing::North.yaw(),
                pitch: 0.0,
            },
            dummy_spawn: Vec3::new(2.0, 0.0, -6.0),
            bounds_min: Vec2::splat(-ARENA_HALF + inset),
            bounds_max: Vec2::splat(ARENA_HALF - inset),
        }
    }
}

/// Marks static arena collision (ground, boundary and props).
#[derive(Component, Debug)]
pub struct ArenaCollision;

// ---------------------------------------------------------------------------
// Props (D29)
// ---------------------------------------------------------------------------

/// A kind of solid arena prop, and the Blender model that draws it
/// (`art/blender/assets/props.py`). Collision shapes are fixed here, fitted to
/// the models' sidecar bounds (a test keeps the two in step).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropKind {
    /// A big leaning boulder, 1.25 m: crouch cover.
    RockA,
    /// A rounder 1.05 m boulder with a small buddy rock at its front-left.
    RockB,
    /// A sawn stump, 0.56 m: jumpable.
    StumpA,
}

/// Where a solid prop stands: feet on the ground at `position`, turned `yaw`
/// radians about +Y (0 faces -Z, like its model).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PropPlacement {
    pub kind: PropKind,
    pub position: Vec3,
    pub yaw: f32,
}

/// Props keep at least this far (m, between footprints) from both spawns and
/// every initial cover piece.
pub const PROP_CLEARANCE: f32 = 3.0;

const fn prop(kind: PropKind, x: f32, z: f32, yaw_deg: f32) -> PropPlacement {
    PropPlacement {
        kind,
        position: Vec3::new(x, 0.0, z),
        yaw: yaw_deg * std::f32::consts::PI / 180.0,
    }
}

/// The arena's solid props. Chosen to frame the spawn view (T01) while staying
/// clear of the spawns, the initial cover, the dummy's strafe arc and every
/// path the Milestone 1 tests walk or shoot along (the x = 2 spawn line, the
/// shot fan toward the dummy line at z = 0, the grid cells the building tests
/// use). The first rock sits on the x = -8 grid line, so a wall there is built
/// straight through it.
pub const ARENA_PROPS: [PropPlacement; 8] = [
    prop(PropKind::RockA, -8.0, 9.5, 20.0),
    prop(PropKind::StumpA, 8.0, 11.0, 0.0),
    prop(PropKind::RockB, 9.5, 18.5, -35.0),
    prop(PropKind::RockB, -9.5, 17.0, 70.0),
    prop(PropKind::StumpA, -11.0, 20.0, 40.0),
    prop(PropKind::RockA, -21.0, -20.5, 60.0),
    prop(PropKind::StumpA, -17.0, -21.5, 15.0),
    prop(PropKind::RockB, 21.5, -21.5, -20.0),
];

impl PropKind {
    pub const ALL: [PropKind; 3] = [PropKind::RockA, PropKind::RockB, PropKind::StumpA];

    /// The model that draws it (`assets/models/<name>.glb`).
    pub fn model(self) -> &'static str {
        match self {
            PropKind::RockA => "rock_a",
            PropKind::RockB => "rock_b",
            PropKind::StumpA => "stump_a",
        }
    }

    pub fn is_rock(self) -> bool {
        matches!(self, PropKind::RockA | PropKind::RockB)
    }

    /// Height of its top above the ground (m).
    pub fn height(self) -> f32 {
        match self {
            PropKind::RockA => 1.25,
            PropKind::RockB => 1.05,
            PropKind::StumpA => 0.56,
        }
    }

    /// The convex pieces of its collision shape, in its local space (feet at
    /// the origin, facing -Z): each a point cloud to hull.
    pub fn hulls(self) -> Vec<Vec<Vec3>> {
        match self {
            // Bounds x ±0.95, z ±0.75, 1.25 tall (rock_a.json).
            PropKind::RockA => vec![rock_hull(Vec3::ZERO, Vec3::new(0.95, 1.25, 0.75), 2.5)],
            // The main rock (x ±0.725, z ±0.65, 1.05 tall) and its buddy
            // (0.7 × 0.62 m, 0.5 tall) at (-0.72, -0.42) (rock_b.json).
            PropKind::RockB => vec![
                rock_hull(Vec3::ZERO, Vec3::new(0.725, 1.05, 0.65), 2.2),
                rock_hull(
                    Vec3::new(-0.72, 0.0, -0.42),
                    Vec3::new(0.35, 0.5, 0.31),
                    2.3,
                ),
            ],
            // A cylinder of radius 0.5 m, 0.56 m tall (the bark; the root
            // flare is visual only).
            PropKind::StumpA => vec![cylinder_points(0.5, 0.56, 16)],
        }
    }

    /// Its collider, in local space.
    pub fn collider(self) -> Collider {
        let mut hulls: Vec<Collider> = self
            .hulls()
            .into_iter()
            .filter_map(Collider::convex_hull)
            .collect();
        match hulls.len() {
            0 => Collider::cylinder(0.5, self.height()),
            1 => hulls.remove(0),
            _ => Collider::compound(
                hulls
                    .into_iter()
                    .map(|hull| (Vec3::ZERO, Quat::IDENTITY, hull))
                    .collect(),
            ),
        }
    }

    /// Radius of a circle around its origin that contains its whole footprint.
    pub fn footprint_radius(self) -> f32 {
        self.hulls()
            .iter()
            .flatten()
            .map(|p| p.xz().length())
            .fold(0.0, f32::max)
    }
}

impl PropPlacement {
    /// Local → world.
    pub fn transform(&self) -> Transform {
        Transform::from_translation(self.position).with_rotation(Quat::from_rotation_y(self.yaw))
    }

    /// Its collision hull points in world space (for clearance checks).
    pub fn world_points(&self) -> Vec<Vec3> {
        let t = self.transform();
        self.kind
            .hulls()
            .iter()
            .flatten()
            .map(|p| t.transform_point(*p))
            .collect()
    }

    /// Horizontal distance from `point` to the prop's footprint (0 inside its
    /// bounding circle).
    pub fn footprint_distance(&self, point: Vec2) -> f32 {
        (point.distance(self.position.xz()) - self.kind.footprint_radius()).max(0.0)
    }
}

/// A rock-shaped point cloud: a rounded superellipsoid (the Blender boulder's
/// shape) with a flat base, fitted to `half` = (half width, height, half
/// depth) and centred on `base` (on the ground).
fn rock_hull(base: Vec3, half: Vec3, exponent: f32) -> Vec<Vec3> {
    // Directions on a Fibonacci sphere, like `shapes.fibonacci_sphere`.
    let n = 40;
    let golden = std::f32::consts::PI * (3.0 - 5f32.sqrt());
    // The boulder's unit shape spans -0.45 (its flat base) to its crown.
    let floor = -0.45;
    let unit: Vec<Vec3> = (0..n)
        .map(|i| {
            let y = 1.0 - 2.0 * (i as f32 + 0.5) / n as f32;
            let r = (1.0 - y * y).max(0.0).sqrt();
            let a = golden * i as f32;
            let d = Vec3::new(a.cos() * r, y, a.sin() * r);
            let s =
                (d.x.abs().powf(exponent) + d.y.abs().powf(exponent) + d.z.abs().powf(exponent))
                    .powf(1.0 / exponent);
            let p = d / s;
            p.with_y(p.y.max(floor))
        })
        .collect();
    // Fit to the model's box, like `shapes.fit_to_box`.
    let lo = unit.iter().copied().fold(Vec3::MAX, Vec3::min);
    let hi = unit.iter().copied().fold(Vec3::MIN, Vec3::max);
    let size = (hi - lo).max(Vec3::splat(1e-4));
    unit.into_iter()
        .map(|p| {
            let t = (p - lo) / size;
            base + Vec3::new(
                (t.x * 2.0 - 1.0) * half.x,
                t.y * half.y,
                (t.z * 2.0 - 1.0) * half.z,
            )
        })
        .collect()
}

fn cylinder_points(radius: f32, height: f32, sides: usize) -> Vec<Vec3> {
    (0..sides)
        .flat_map(|i| {
            let a = std::f32::consts::TAU * i as f32 / sides as f32;
            let p = Vec3::new(a.cos() * radius, 0.0, a.sin() * radius);
            [p, p + Vec3::Y * height]
        })
        .collect()
}

/// Whether a ground point is where the training dummy strafes. The dummy
/// strafes sideways while facing the player, so from the spawns it sweeps arcs
/// around the player's spawn at the spawns' distance (20 m). Over minutes it
/// drifts outward (up to about 36 m in 5 simulated minutes), and the cover
/// walls deflect it inward (to about 11 m). All of that stays north of
/// z = 8 (the dummy's spawn + 14 m), except a strip down the west edge.
pub fn in_dummy_strafe_zone(layout: &ArenaLayout, point: Vec2) -> bool {
    let centre = layout.player_spawn.xz();
    let radius = layout.dummy_spawn.xz().distance(centre);
    let r = point.distance(centre);
    let arc = (radius - 9.0..=radius + 18.0).contains(&r);
    let north = point.y <= layout.dummy_spawn.z + 14.0;
    let west_edge = point.x <= layout.bounds_min.x + 5.0;
    arc && (north || west_edge)
}

/// Height of the invisible boundary walls.
pub const BOUNDARY_HEIGHT: f32 = 40.0;

pub struct ArenaPlugin;

impl Plugin for ArenaPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ArenaLayout>()
            .add_systems(Startup, spawn_arena_collision);
    }
}

/// On a prop's collider entity.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ArenaProp(pub PropPlacement);

fn spawn_arena_collision(mut commands: Commands) {
    let world = CollisionLayers::new(Layer::World, LayerMask::ALL);
    // Ground: top surface at y = 0, extending well past the arena.
    let ground = 2.0 * ARENA_HALF + 40.0;
    commands.spawn((
        Name::new("Ground collision"),
        ArenaCollision,
        RigidBody::Static,
        Collider::cuboid(ground, 1.0, ground),
        world,
        Transform::from_xyz(0.0, -0.5, 0.0),
    ));
    // Boundary walls just outside the arena edge.
    let thickness = 1.0;
    let length = 2.0 * ARENA_HALF + 2.0 * thickness;
    for (x, z, sx, sz) in [
        (0.0, -ARENA_HALF - thickness / 2.0, length, thickness),
        (0.0, ARENA_HALF + thickness / 2.0, length, thickness),
        (-ARENA_HALF - thickness / 2.0, 0.0, thickness, length),
        (ARENA_HALF + thickness / 2.0, 0.0, thickness, length),
    ] {
        commands.spawn((
            Name::new("Boundary collision"),
            ArenaCollision,
            RigidBody::Static,
            Collider::cuboid(sx, BOUNDARY_HEIGHT, sz),
            world,
            Transform::from_xyz(x, BOUNDARY_HEIGHT / 2.0, z),
        ));
    }
    // The solid props (D29).
    for placement in ARENA_PROPS {
        commands.spawn((
            Name::new(format!("Prop {}", placement.kind.model())),
            ArenaCollision,
            ArenaProp(placement),
            RigidBody::Static,
            placement.kind.collider(),
            world,
            placement.transform(),
        ));
    }
}
