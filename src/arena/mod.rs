//! Arena layout: the collision geometry and spawn points every gameplay slice
//! relies on. Runs headless. Visuals live in [`visuals`] (client only).

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

/// Marks static arena collision (ground and boundary).
#[derive(Component, Debug)]
pub struct ArenaCollision;

/// Height of the invisible boundary walls.
pub const BOUNDARY_HEIGHT: f32 = 40.0;

pub struct ArenaPlugin;

impl Plugin for ArenaPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ArenaLayout>()
            .add_systems(Startup, spawn_arena_collision);
    }
}

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
}
