//! The grunt: the first wave knight (docs/M3-SPEC.md → The grunt, D71–D78).
//!
//! A grunt is an ordinary [`Character`](crate::shared::Character) with the
//! [`Grunt`] marker, a [`Wand`](crate::orb::Wand) instead of guns, and
//! `Health::full(stats.hp, 0)`. Its brain (chunk 1, slice A) writes only its own
//! [`PlayerIntent`](crate::shared::PlayerIntent) and
//! [`LookAngles`](crate::shared::LookAngles): movement, the wand and building
//! apply them exactly as for the player (the "virtual controller" rule,
//! docs/research/bot-ai.md). Every non-player character is drawn as the knight
//! (`arena::visuals::target`), so grunts get the M2 knight for free.
//!
//! Speed: movement caps a character at run speed (5.5 m/s) or, sprinting
//! forward, sprint speed (7.5 m/s). A grunt reaches its [`GruntStats::speed`] by
//! scaling `move_axis` and, above run speed, holding `sprint` while moving
//! forward.
//!
//! Owned by slice A (the brain). The orchestrator owns [`GruntTuning`]'s spec
//! defaults and [`GruntStats::for_wave`] (the scaling contract).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Marks a wave knight of the grunt type.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct Grunt;

/// Spec defaults (docs/M3-SPEC.md → The grunt). Presentation-free; every
/// number here is tunable by up to ±50% from Jake's play-tests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GruntTuning {
    /// Wave-1 health (no shield).
    pub hp: f32,
    /// Wave-1 move speed (m/s).
    pub speed: f32,
    /// Move speed never scales past this (below the player's 7.5 m/s sprint).
    pub speed_cap: f32,
    /// Wave-1 seconds between orbs (the wind-up is part of it).
    pub fire_interval: f32,
    /// Wand wind-up before each orb (s).
    pub windup: f32,
    /// Preferred standing distance from the player (m).
    pub range_min: f32,
    pub range_max: f32,
    /// Per-wave growth, as a fraction per wave after the first.
    pub hp_per_wave: f32,
    pub speed_per_wave: f32,
    pub fire_rate_per_wave: f32,
    /// Every curve stops growing at this wave.
    pub scaling_cap_wave: u32,
    /// Aim, "Normal" tier (wave 1) and "Hard" tier (reached at `aim_hard_wave`).
    pub reaction_normal: f32,
    pub reaction_hard: f32,
    pub lag_normal: f32,
    pub lag_hard: f32,
    /// Aim error at 10 m (m).
    pub error_normal: f32,
    pub error_hard: f32,
    pub aim_hard_wave: u32,
    /// At most this many grunts hold attack tokens (wind up or fire) at once.
    pub max_shooters: u32,
    pub decision_hz: f32,
    pub perception_hz: f32,
    /// Knockback travel (m) from a point-blank pump hit with every pellet landing.
    pub pump_knockback: f32,
}

impl Default for GruntTuning {
    fn default() -> Self {
        Self {
            hp: 100.0,
            speed: 4.5,
            speed_cap: 6.5,
            fire_interval: 1.5,
            windup: 0.4,
            range_min: 10.0,
            range_max: 18.0,
            hp_per_wave: 0.04,
            speed_per_wave: 0.015,
            fire_rate_per_wave: 0.02,
            scaling_cap_wave: 25,
            reaction_normal: 0.33,
            reaction_hard: 0.26,
            lag_normal: 0.18,
            lag_hard: 0.14,
            error_normal: 0.35,
            error_hard: 0.20,
            aim_hard_wave: 15,
            max_shooters: 3,
            decision_hz: 8.0,
            perception_hz: 30.0,
            pump_knockback: 4.0,
        }
    }
}

/// One grunt's numbers for a wave (D74–D76), before the per-knight ±10% speed
/// jitter the wave director applies.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct GruntStats {
    pub hp: f32,
    /// m/s
    pub speed: f32,
    /// Seconds between orbs.
    pub fire_interval: f32,
    /// Seconds from gaining line of sight to the first wind-up.
    pub reaction: f32,
    /// Age of the player snapshot the aim leads from (s).
    pub lag: f32,
    /// Aim error at 10 m (m).
    pub error_at_10m: f32,
}

impl GruntStats {
    /// Stats for wave `wave` (1-based; 0 counts as 1).
    pub fn for_wave(wave: u32, t: &GruntTuning) -> Self {
        let n = wave.clamp(1, t.scaling_cap_wave.max(1)) as f32 - 1.0;
        let aim = if t.aim_hard_wave > 1 {
            (n / (t.aim_hard_wave as f32 - 1.0)).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let lerp = |a: f32, b: f32| a + (b - a) * aim;
        Self {
            hp: t.hp * (1.0 + t.hp_per_wave * n),
            speed: (t.speed * (1.0 + t.speed_per_wave * n)).min(t.speed_cap),
            fire_interval: t.fire_interval / (1.0 + t.fire_rate_per_wave * n),
            reaction: lerp(t.reaction_normal, t.reaction_hard),
            lag: lerp(t.lag_normal, t.lag_hard),
            error_at_10m: lerp(t.error_normal, t.error_hard),
        }
    }
}

/// Grunt behaviour. Slice A fills it in; until then it only registers types.
pub struct GruntPlugin;

impl Plugin for GruntPlugin {
    fn build(&self, _app: &mut App) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_one_matches_the_spec() {
        let t = GruntTuning::default();
        let s = GruntStats::for_wave(1, &t);
        assert_eq!(s.hp, 100.0);
        assert_eq!(s.speed, 4.5);
        assert_eq!(s.fire_interval, 1.5);
        assert_eq!(s.reaction, 0.33);
        assert_eq!(GruntStats::for_wave(0, &t), s);
    }

    #[test]
    fn scaling_curves_and_caps() {
        let t = GruntTuning::default();
        let w10 = GruntStats::for_wave(10, &t);
        assert!((w10.hp - 136.0).abs() < 1e-3);
        assert!((w10.speed - 4.5 * 1.135).abs() < 1e-3);
        let w25 = GruntStats::for_wave(25, &t);
        assert!((w25.hp - 196.0).abs() < 1e-3);
        assert!(w25.speed <= t.speed_cap && w25.speed < 7.5);
        assert_eq!(GruntStats::for_wave(40, &t), w25);
        let w15 = GruntStats::for_wave(15, &t);
        assert!((w15.reaction - t.reaction_hard).abs() < 1e-6);
        assert!((w15.error_at_10m - t.error_hard).abs() < 1e-6);
    }
}
