//! The grunt's aim model (D76, docs/research/bot-ai.md → Human-like aim):
//! it leads a snapshot of the player `lag` seconds old, extrapolated by a noisy
//! velocity estimate over the orb's flight time, plus an error re-rolled per
//! shot that grows with distance. Pure functions; the brain feeds them.

use crate::rng::Rng;
use bevy::prelude::*;
use std::collections::VecDeque;

/// Where to aim so a projectile at `speed` from `shooter` meets a target now
/// at `target` moving at `velocity` (a few fixed-point iterations on the
/// flight time).
pub fn lead_point(shooter: Vec3, target: Vec3, velocity: Vec3, speed: f32) -> Vec3 {
    if speed <= 0.0 {
        return target;
    }
    let mut point = target;
    for _ in 0..4 {
        let t = shooter.distance(point) / speed;
        point = target + velocity * t;
    }
    point
}

/// One shot's rolled aim noise: an error disc sample and velocity-estimate noise.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ShotNoise {
    /// A point in the unit disc: the error direction and fraction.
    pub disc: Vec2,
    /// Multiplier on the estimated speed.
    pub speed_scale: f32,
    /// Added to the velocity estimate (m/s, horizontal).
    pub velocity_jitter: Vec3,
}

impl ShotNoise {
    /// Rolls a shot's noise (uniform in the disc, ±20% speed, ±0.4 m/s jitter).
    pub fn roll(rng: &mut Rng) -> Self {
        let r = rng.next_f32().sqrt();
        let a = rng.range(0.0, std::f32::consts::TAU);
        Self {
            disc: Vec2::new(a.cos(), a.sin()) * r,
            speed_scale: rng.range(0.8, 1.2),
            velocity_jitter: Vec3::new(rng.range(-0.4, 0.4), 0.0, rng.range(-0.4, 0.4)),
        }
    }

    /// The velocity estimate this shot believes.
    pub fn estimate(&self, velocity: Vec3) -> Vec3 {
        velocity * self.speed_scale + self.velocity_jitter
    }

    /// The miss offset, perpendicular to the aim direction `dir`, at
    /// `distance`: radius `error_at_10m × distance / 10`.
    pub fn offset(&self, dir: Vec3, distance: f32, error_at_10m: f32) -> Vec3 {
        let Some(dir) = dir.try_normalize() else {
            return Vec3::ZERO;
        };
        let side = dir.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
        let up = side.cross(dir);
        let radius = error_at_10m * distance / 10.0;
        (side * self.disc.x + up * self.disc.y) * radius
    }
}

/// The player's recent aim points, one per fixed tick (newest last).
#[derive(Debug, Clone, Default)]
pub struct History {
    samples: VecDeque<Vec3>,
}

impl History {
    /// Longest history kept (ticks): 1 s.
    pub const CAPACITY: usize = 60;

    pub fn push(&mut self, point: Vec3) {
        if self.samples.len() == Self::CAPACITY {
            self.samples.pop_front();
        }
        self.samples.push_back(point);
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }

    /// The point `ticks` ticks ago (clamped to the oldest sample).
    pub fn ago(&self, ticks: usize) -> Option<Vec3> {
        let n = self.samples.len();
        if n == 0 {
            return None;
        }
        self.samples.get(n - 1 - ticks.min(n - 1)).copied()
    }

    /// The snapshot `lag` ticks old and its velocity over the 6 ticks before it.
    pub fn snapshot(&self, lag: usize, tick_seconds: f32) -> Option<(Vec3, Vec3)> {
        const SPAN: usize = 6;
        let last = self.samples.len().checked_sub(1)?;
        let (i_now, i_before) = (last - lag.min(last), last - (lag + SPAN).min(last));
        let (now, before) = (self.samples[i_now], self.samples[i_before]);
        let span = i_now - i_before;
        let v = if span > 0 {
            (now - before) / (span as f32 * tick_seconds)
        } else {
            Vec3::ZERO
        };
        Some((now, v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leading_meets_a_moving_target() {
        let shooter = Vec3::ZERO;
        let target = Vec3::new(0.0, 0.0, -15.0);
        let v = Vec3::new(5.0, 0.0, 0.0);
        let p = lead_point(shooter, target, v, 30.0);
        let t = shooter.distance(p) / 30.0;
        assert!((target + v * t).distance(p) < 0.01);
        assert!(p.x > 2.0, "leads to the right: {p}");
        assert_eq!(lead_point(shooter, target, Vec3::ZERO, 30.0), target);
    }

    #[test]
    fn error_scales_with_distance_and_stays_perpendicular() {
        let mut rng = Rng::new(3);
        for _ in 0..200 {
            let n = ShotNoise::roll(&mut rng);
            let dir = Vec3::new(0.3, -0.1, -1.0).normalize();
            let at10 = n.offset(dir, 10.0, 0.35);
            let at20 = n.offset(dir, 20.0, 0.35);
            assert!(at10.length() <= 0.35 + 1e-4);
            assert!((at20.length() - 2.0 * at10.length()).abs() < 1e-4);
            assert!(at10.dot(dir).abs() < 1e-4);
            assert!((0.8..1.2).contains(&n.speed_scale));
        }
    }

    #[test]
    fn history_snapshots_lag_and_estimate_velocity() {
        let mut h = History::default();
        let dt = 1.0 / 60.0;
        for i in 0..100 {
            h.push(Vec3::new(i as f32 * 5.0 * dt, 0.0, 0.0));
        }
        let (p, v) = h.snapshot(11, dt).unwrap();
        assert!((p.x - 88.0 * 5.0 * dt).abs() < 1e-4, "11 ticks old");
        assert!((v.x - 5.0).abs() < 1e-3, "{v}");
        // A fresh history holds still.
        let mut h = History::default();
        h.push(Vec3::ONE);
        assert_eq!(h.snapshot(11, dt), Some((Vec3::ONE, Vec3::ZERO)));
    }
}
