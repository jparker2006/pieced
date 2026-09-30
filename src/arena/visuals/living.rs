//! A living world (M4 chunk 6, D123): the island's scenery moves.
//!
//! - **Sway:** grass tufts, flowers, bushes and the trees' crowns sway in the
//!   wind field ([`crate::look::wind`]). It is a vertex offset in the toon
//!   material and its ink hull, weighted per vertex (baked into the merged
//!   scenery's normals, by model height for the trees). The grass and the
//!   cliffs-and-bushes materials carry their sway in their uniform; the trees
//!   swap the models' shared material for one swaying copy ([`TreeSway`]),
//!   so they still batch. No new draws, no per-frame CPU work, and nothing a
//!   collider, hitbox or the aim ray reads ever moves.
//! - **Clouds:** the cloud sea's sectors turn slowly round the island (a full
//!   lap every [`CLOUD_LAP_S`], so they wrap for ever) and breathe up and
//!   down; the clouds under the station and the far islands only bob.
//!   [`CloudDrift::transform`] is a pure function of the sky clock.

use crate::{
    far::SkyClock,
    look::{ModelDressed, ToonMaterial, wind::Sway},
};
use bevy::prelude::*;
use std::f32::consts::TAU;

/// Tuft and flower sway at the tip (m, in a full gust).
pub const GRASS_SWAY: f32 = 0.075;
/// Bush crowns' sway (m).
pub const BUSH_SWAY: f32 = 0.05;
/// Tree crowns' sway (m at the crown's top), rising from the trunk's fork
/// ([`TREE_SWAY_FROM`] m up the model) to the crown ([`TREE_SWAY_TO`]).
pub const TREE_SWAY: f32 = 0.16;
pub const TREE_SWAY_FROM: f32 = 2.3;
pub const TREE_SWAY_TO: f32 = 5.5;
/// Seconds for the cloud sea to lap the island once.
pub const CLOUD_LAP_S: f32 = 900.0;
/// The clouds' breathing: up and down this far (m) over this long (s).
pub const CLOUD_BOB_M: f32 = 1.6;
pub const CLOUD_BOB_S: f32 = 23.0;

/// How a cloud mesh moves.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub enum CloudDrift {
    /// A sector of the sea: laps the island and bobs (`phase` in radians).
    Sea { phase: f32 },
    /// Anchored under the station and the far islands: bobs only.
    Anchored { phase: f32 },
}

impl CloudDrift {
    /// Where the mesh (authored in world space) sits at `seconds`.
    pub fn transform(&self, seconds: f64) -> Transform {
        let (phase, lap) = match *self {
            CloudDrift::Sea { phase } => (phase, true),
            CloudDrift::Anchored { phase } => (phase, false),
        };
        let bob_turns = (seconds / CLOUD_BOB_S as f64).rem_euclid(1.0) as f32;
        let y = CLOUD_BOB_M * (bob_turns * TAU + phase).sin();
        let yaw = if lap {
            -((seconds / CLOUD_LAP_S as f64).rem_euclid(1.0) as f32) * TAU
        } else {
            0.0
        };
        Transform::from_translation(Vec3::Y * y).with_rotation(Quat::from_rotation_y(yaw))
    }
}

/// Moves the clouds on the sky clock.
pub fn drift_clouds(
    clock: Option<Res<SkyClock>>,
    mut clouds: Query<(&CloudDrift, &mut Transform)>,
) {
    let Some(clock) = clock else {
        return;
    };
    for (drift, mut transform) in &mut clouds {
        *transform = drift.transform(clock.seconds);
    }
}

/// The trees' swaying copy of the models' shared toon material.
#[derive(Resource, Debug, Clone)]
pub struct TreeSway(pub Handle<ToonMaterial>);

impl FromWorld for TreeSway {
    fn from_world(world: &mut World) -> Self {
        let material = ToonMaterial::vertex_colored().with_sway(tree_sway());
        Self(world.resource_mut::<Assets<ToonMaterial>>().add(material))
    }
}

/// The trees' sway.
pub fn tree_sway() -> Sway {
    Sway::height(TREE_SWAY, TREE_SWAY_FROM, TREE_SWAY_TO)
}

/// Gives each tree model the swaying material as it's dressed.
pub fn sway_trees(
    mut dressed: MessageReader<ModelDressed>,
    tree: Option<Res<TreeSway>>,
    children: Query<&Children>,
    mut meshes: Query<&mut MeshMaterial3d<ToonMaterial>>,
) {
    let Some(tree) = tree else {
        dressed.clear();
        return;
    };
    for event in dressed.read() {
        if !event.name.starts_with("tree_") {
            continue;
        }
        for e in children.iter_descendants(event.root) {
            if let Ok(mut material) = meshes.get_mut(e)
                && material.0 != tree.0
            {
                material.0 = tree.0.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sea_laps_and_the_anchored_clouds_only_bob() {
        let sea = CloudDrift::Sea { phase: 0.3 };
        let a = sea.transform(0.0);
        let b = sea.transform(60.0);
        assert!(
            a.rotation.angle_between(b.rotation) > 0.3,
            "turned in a minute"
        );
        let lap = sea.transform(CLOUD_LAP_S as f64);
        assert!(
            a.rotation.angle_between(lap.rotation) < 1e-3,
            "wraps after a lap"
        );
        let anchored = CloudDrift::Anchored { phase: 1.0 };
        for t in [0.0, 10.0, 100.0] {
            let tr = anchored.transform(t);
            assert_eq!(tr.rotation, Quat::IDENTITY);
            assert!(tr.translation.y.abs() <= CLOUD_BOB_M);
        }
    }
}
