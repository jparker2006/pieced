//! Slice C — weapons, hitscan, damage, health, ADS and reloads.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GunTuning {
    /// Seconds between shots.
    pub fire_interval: f32,
    /// Body damage per bullet or pellet.
    pub damage: f32,
    pub headshot_multiplier: f32,
    /// Full damage up to this distance (m).
    pub falloff_start: f32,
    /// Damage reaches `falloff_min` of full at this distance (m).
    pub falloff_end: f32,
    pub falloff_min: f32,
    pub range: f32,
    pub magazine: u32,
    /// Magazine reload time, or per-shell time when `reload_per_shell`.
    pub reload_time: f32,
    pub reload_per_shell: bool,
    pub base_spread_deg: f32,
    pub bloom_per_shot_deg: f32,
    pub bloom_max_deg: f32,
    pub bloom_recover_delay: f32,
    pub bloom_recover_deg_per_sec: f32,
    pub ads_spread_multiplier: f32,
    /// FOV multiplier while aiming down sights.
    pub ads_zoom: f32,
    pub pellets: u32,
    /// Radius of the fixed pellet pattern (degrees).
    pub pellet_spread_deg: f32,
    /// Damage to building pieces per bullet or pellet.
    pub structure_damage: f32,
}

impl GunTuning {
    pub fn rifle() -> Self {
        Self {
            fire_interval: 1.0 / 6.0,
            damage: 28.0,
            headshot_multiplier: 1.5,
            falloff_start: 25.0,
            falloff_end: 50.0,
            falloff_min: 0.7,
            range: 150.0,
            magazine: 30,
            reload_time: 2.0,
            reload_per_shell: false,
            base_spread_deg: 0.05,
            bloom_per_shot_deg: 0.3,
            bloom_max_deg: 1.8,
            bloom_recover_delay: 0.3,
            bloom_recover_deg_per_sec: 8.0,
            ads_spread_multiplier: 0.4,
            ads_zoom: 0.75,
            pellets: 1,
            pellet_spread_deg: 0.0,
            structure_damage: 28.0,
        }
    }

    pub fn pump() -> Self {
        Self {
            fire_interval: 0.9,
            damage: 10.0,
            headshot_multiplier: 1.5,
            falloff_start: 8.0,
            falloff_end: 15.0,
            falloff_min: 0.3,
            range: 40.0,
            magazine: 5,
            reload_time: 0.5,
            reload_per_shell: true,
            base_spread_deg: 0.0,
            bloom_per_shot_deg: 0.0,
            bloom_max_deg: 0.0,
            bloom_recover_delay: 0.0,
            bloom_recover_deg_per_sec: 0.0,
            ads_spread_multiplier: 0.8,
            ads_zoom: 0.9,
            pellets: 10,
            pellet_spread_deg: 4.5,
            structure_damage: 10.0,
        }
    }
}

impl Default for GunTuning {
    fn default() -> Self {
        Self::rifle()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CombatTuning {
    pub rifle: GunTuning,
    pub pump: GunTuning,
    pub switch_time: f32,
    /// A pump press this early before it's ready still fires when ready.
    pub pump_press_buffer: f32,
    pub max_hp: f32,
    pub max_shield: f32,
    pub aim_friction: bool,
    pub aim_friction_strength: f32,
}

impl Default for CombatTuning {
    fn default() -> Self {
        Self {
            rifle: GunTuning::rifle(),
            pump: GunTuning::pump(),
            switch_time: 0.2,
            pump_press_buffer: 0.15,
            max_hp: 100.0,
            max_shield: 100.0,
            aim_friction: false,
            aim_friction_strength: 0.35,
        }
    }
}

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, _app: &mut App) {}
}
