//! Slice D — the arena's look: terrain, backdrop, sky, sun, fog, palette materials,
//! target rim material and quality presets. Phase 0 ships a plain placeholder.

use crate::shared::{ARENA_HALF, Character, Player};
use bevy::prelude::*;

pub struct ArenaVisualsPlugin;

impl Plugin for ArenaVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, placeholder_scene)
            .add_observer(placeholder_character_mesh);
    }
}

fn placeholder_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let size = 2.0 * ARENA_HALF;
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(size, size))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.47, 0.58, 0.36),
            perceptual_roughness: 0.95,
            ..default()
        })),
    ));
    let marker = materials.add(StandardMaterial {
        base_color: Color::srgb(0.36, 0.45, 0.28),
        perceptual_roughness: 0.95,
        ..default()
    });
    // A faint grid of posts so motion is readable in the placeholder.
    let post = meshes.add(Cuboid::new(0.12, 0.05, 0.12));
    let cells = (size / crate::shared::CELL_SIZE) as i32;
    for i in 0..=cells {
        for j in 0..=cells {
            let x = -ARENA_HALF + i as f32 * crate::shared::CELL_SIZE;
            let z = -ARENA_HALF + j as f32 * crate::shared::CELL_SIZE;
            commands.spawn((
                Mesh3d(post.clone()),
                MeshMaterial3d(marker.clone()),
                Transform::from_xyz(x, 0.025, z),
            ));
        }
    }
    commands.spawn((
        DirectionalLight {
            illuminance: 9000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(20.0, 30.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.85, 0.9, 1.0),
        brightness: 500.0,
        ..default()
    });
}

fn placeholder_character_mesh(
    add: On<Add, Character>,
    players: Query<(), With<Player>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // The player is invisible in first person; everyone else gets a capsule.
    if players.contains(add.entity) {
        return;
    }
    let mesh = meshes.add(Capsule3d::new(0.35, 1.1));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.3, 0.45),
        ..default()
    });
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_xyz(0.0, 0.9, 0.0),
        ChildOf(add.entity),
    ));
}
