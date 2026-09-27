//! The grunt's wand and its spell orb (docs/M3-SPEC.md → The orb, D71).
//!
//! Knights carry a [`Wand`] instead of guns: combat's gun step skips any
//! character with one. A grunt holds `PlayerIntent::fire` to wind up; after
//! [`OrbTuning`]'s wind-up (from `GruntTuning::windup`) the orb leaves the wand
//! tip along the character's look direction at that moment. Orbs are real
//! projectiles stepped in the fixed tick with a sphere cast (never in the render
//! frame): they hit the player's hitboxes (head ×1.5), stop on world geometry
//! and pieces (chipping them through [`PieceHit`](crate::shared::PieceHit)) and
//! pass through knights. Damage to the player goes through `Health::apply` and
//! emits [`DamageDealt`](crate::shared::DamageDealt) with `source` = the grunt,
//! so the HUD's damage arrow and the hit feedback work like any other hit.
//!
//! Cues: [`GameCue::WandWindup`](crate::shared::GameCue::WandWindup) when a
//! wind-up starts (the off-screen warning and the wand glow key off it) and
//! [`GameCue::OrbFired`](crate::shared::GameCue::OrbFired) on release.
//!
//! Owned by slice B (the orb and the wand).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct OrbTuning {
    /// m/s
    pub speed: f32,
    /// Sphere-cast radius (m).
    pub radius: f32,
    pub damage: f32,
    pub headshot_multiplier: f32,
    pub structure_damage: f32,
    /// Distance before an orb fizzles (m).
    pub range: f32,
}

impl Default for OrbTuning {
    fn default() -> Self {
        Self {
            speed: 30.0,
            radius: 0.15,
            damage: 12.0,
            headshot_multiplier: 1.5,
            structure_damage: 12.0,
            range: 60.0,
        }
    }
}

/// A knight's wand: its fire timing. Present on every grunt.
#[derive(Component, Debug, Clone, Copy, PartialEq, Default)]
pub struct Wand {
    /// Seconds between orbs (from `GruntStats::fire_interval`).
    pub fire_interval: f32,
    /// Seconds until the wand may start another wind-up.
    pub cooldown: f32,
    /// Seconds of wind-up left, while winding up.
    pub windup: Option<f32>,
}

impl Wand {
    pub fn new(fire_interval: f32) -> Self {
        Self {
            fire_interval,
            ..default()
        }
    }

    /// Winding up or about to fire: the grunt holds an attack token.
    pub fn is_winding(&self) -> bool {
        self.windup.is_some()
    }
}

/// A spell orb in flight.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Orb {
    pub shooter: Entity,
    pub velocity: Vec3,
    /// Metres travelled so far.
    pub travelled: f32,
}

/// The wand and orbs. Slice B fills it in; until then it only registers types.
pub struct OrbPlugin;

impl Plugin for OrbPlugin {
    fn build(&self, _app: &mut App) {}
}
