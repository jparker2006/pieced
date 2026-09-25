//! Slice E — pooled effects: muzzle flash, tracers, sparks, debris, shield shimmer,
//! elimination burst, camera shake and hitstop.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct FeedbackTuning {
    /// 0 disables camera shake entirely.
    pub camera_shake: f32,
    pub hitstop_on_kill: bool,
    pub hitstop_frames: u32,
    pub viewmodel_sway: bool,
    pub max_debris: u32,
    pub max_particles: u32,
}

impl Default for FeedbackTuning {
    fn default() -> Self {
        Self {
            camera_shake: 0.3,
            hitstop_on_kill: true,
            hitstop_frames: 2,
            viewmodel_sway: true,
            max_debris: 160,
            max_particles: 400,
        }
    }
}

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, _app: &mut App) {}
}
