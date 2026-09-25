//! Slice B — the build grid: piece map, targeting, placement, damage and destruction.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct BuildTuning {
    pub wall_hp: f32,
    pub floor_hp: f32,
    pub ramp_hp: f32,
    /// Crack stage 1 at or below this HP fraction.
    pub crack_stage_1: f32,
    /// Crack stage 2 at or below this HP fraction.
    pub crack_stage_2: f32,
    /// Seconds a destroyed piece's spot stays locked.
    pub rebuild_lock: f32,
    /// Seconds between placements while the primary action is held.
    pub turbo_interval: f32,
    pub wall_thickness: f32,
    pub floor_thickness: f32,
    /// Piece key selects *and* places in one press.
    pub builder_pro: bool,
}

impl Default for BuildTuning {
    fn default() -> Self {
        Self {
            wall_hp: 200.0,
            floor_hp: 170.0,
            ramp_hp: 170.0,
            crack_stage_1: 0.66,
            crack_stage_2: 0.33,
            rebuild_lock: 0.15,
            turbo_interval: 0.05,
            wall_thickness: 0.2,
            floor_thickness: 0.2,
            builder_pro: false,
        }
    }
}

pub struct BuildingPlugin;

impl Plugin for BuildingPlugin {
    fn build(&self, _app: &mut App) {}
}

/// Client-only: piece meshes, crack visuals and the ghost preview.
pub struct BuildingVisualsPlugin;

impl Plugin for BuildingVisualsPlugin {
    fn build(&self, _app: &mut App) {}
}
