//! Slice F — crosshair, bars, ammo, hotbar, hitmarkers, damage numbers, piece HP,
//! combat readout and the performance overlay.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct HudTuning {
    pub show_bloom: bool,
    pub crosshair_scale: f32,
    pub damage_numbers: bool,
    pub perf_overlay: bool,
}

impl Default for HudTuning {
    fn default() -> Self {
        Self {
            show_bloom: true,
            crosshair_scale: 1.0,
            damage_numbers: true,
            perf_overlay: false,
        }
    }
}

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, _app: &mut App) {}
}
