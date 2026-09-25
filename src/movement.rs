//! Slice A — kinematic first-person movement in the fixed step (see docs/SPEC.md).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MovementTuning {
    pub run_speed: f32,
    pub sprint_speed: f32,
    pub crouch_speed: f32,
    /// Speed at slide entry; decays back toward crouch speed over `slide_duration`.
    pub slide_speed: f32,
    pub slide_duration: f32,
    pub slide_cooldown: f32,
    /// Seconds from rest to full speed on the ground.
    pub accel_time: f32,
    /// Seconds from full speed to rest on the ground.
    pub stop_time: f32,
    pub jump_height: f32,
    pub gravity: f32,
    /// Fraction of ground acceleration available in the air.
    pub air_control: f32,
    pub jump_buffer: f32,
    pub coyote_time: f32,
    pub height: f32,
    pub crouch_height: f32,
    pub radius: f32,
    pub eye_height: f32,
    pub crouch_eye_height: f32,
    pub max_slope_deg: f32,
    pub step_height: f32,
}

impl Default for MovementTuning {
    fn default() -> Self {
        Self {
            run_speed: 5.5,
            sprint_speed: 7.5,
            crouch_speed: 2.8,
            slide_speed: 9.0,
            slide_duration: 0.8,
            slide_cooldown: 0.5,
            accel_time: 0.1,
            stop_time: 0.06,
            jump_height: 1.2,
            gravity: 20.0,
            air_control: 0.4,
            jump_buffer: 0.1,
            coyote_time: 0.1,
            height: 1.8,
            crouch_height: 1.2,
            radius: 0.35,
            eye_height: 1.62,
            crouch_eye_height: 1.05,
            max_slope_deg: 46.0,
            step_height: 0.3,
        }
    }
}

pub struct MovementPlugin;

impl Plugin for MovementPlugin {
    fn build(&self, _app: &mut App) {}
}
