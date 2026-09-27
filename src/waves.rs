//! The Waves mode: runs, waves, the break, score and results (docs/M3-SPEC.md →
//! Waves, D79–D85). Only active in [`GameMode::Waves`](crate::shared::GameMode).
//!
//! Chunk 1 (slice C) builds the minimal run: 3 grunts poof in at seeded points
//! on the island edge, the player's elimination ends the run, a plain results
//! line with a restart key. Chunk 2 grows it into the full wave director.
//!
//! Grunt characters come from a fixed pool of `max_alive` characters created at
//! run start (parked and inactive), reused for every spawn, so a wave spawn
//! never instances a model mid-fight (the knight figure is rigged once).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct WavesTuning {
    /// Grunts in wave 1, and how many each later wave adds (D68).
    pub first_wave: u32,
    pub per_wave: u32,
    /// Knights alive (or in a ship's beam) at once (D77).
    pub max_alive: u32,
    /// The break between waves (s), and the shield it refills at its start (D79).
    pub break_seconds: f32,
    pub break_shield: f32,
    /// Shield potions (D80).
    pub potion_chance: f32,
    pub potion_shield: f32,
    pub potion_lifetime: f32,
    pub potion_pickup_radius: f32,
    /// Score (D81).
    pub score_kill: u32,
    pub score_headshot: u32,
    pub score_void: u32,
    pub score_wave: u32,
    /// Per-knight speed jitter, ± this fraction (D83).
    pub speed_jitter: f32,
    /// The death beat: time scale and real seconds (D84).
    pub death_time_scale: f32,
    pub death_seconds: f32,
}

impl Default for WavesTuning {
    fn default() -> Self {
        Self {
            first_wave: 3,
            per_wave: 2,
            max_alive: 8,
            break_seconds: 10.0,
            break_shield: 50.0,
            potion_chance: 0.1,
            potion_shield: 25.0,
            potion_lifetime: 20.0,
            potion_pickup_radius: 1.0,
            score_kill: 100,
            score_headshot: 50,
            score_void: 150,
            score_wave: 250,
            speed_jitter: 0.1,
            death_time_scale: 0.3,
            death_seconds: 1.0,
        }
    }
}

impl WavesTuning {
    /// Grunts in wave `wave` (1-based).
    pub fn wave_size(&self, wave: u32) -> u32 {
        self.first_wave + self.per_wave * wave.max(1).saturating_sub(1)
    }
}

/// The run. Slice C fills it in; until then it only registers types.
pub struct WavesPlugin;

impl Plugin for WavesPlugin {
    fn build(&self, _app: &mut App) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_sizes_follow_d68() {
        let t = WavesTuning::default();
        assert_eq!(t.wave_size(1), 3);
        assert_eq!(t.wave_size(2), 5);
        assert_eq!(t.wave_size(20), 41);
        let through_20: u32 = (1..=20).map(|w| t.wave_size(w)).sum();
        assert_eq!(through_20, 440);
    }
}
