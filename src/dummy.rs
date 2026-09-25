//! Slice C — the training dummy: strafing pattern driven through `PlayerIntent`,
//! elimination and respawn.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DummyTuning {
    pub stand_still: bool,
    /// Fraction of the player's run speed used while strafing (1.0 = same speed).
    pub speed_scale: f32,
    pub respawn_delay: f32,
    pub jump_chance_per_sec: f32,
    pub turn_interval_min: f32,
    pub turn_interval_max: f32,
    /// Respawn at least this far from the player (m).
    pub respawn_min_distance: f32,
}

impl Default for DummyTuning {
    fn default() -> Self {
        Self {
            stand_still: false,
            speed_scale: 1.0,
            respawn_delay: 2.0,
            jump_chance_per_sec: 0.15,
            turn_interval_min: 0.4,
            turn_interval_max: 1.6,
            respawn_min_distance: 12.0,
        }
    }
}

/// Marks the training dummy character.
#[derive(Component, Debug, Default)]
pub struct Dummy;

pub struct DummyPlugin;

impl Plugin for DummyPlugin {
    fn build(&self, _app: &mut App) {}
}
