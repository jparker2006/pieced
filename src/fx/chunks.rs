//! Physics-lite chunks (M4, D105 and D109): rigid bits that fly on a
//! ballistic arc, tumble, bounce on whatever ground is under them, slide to
//! a stop, then shrink and fade away. The knights' armor coming off
//! ([`super::armor`]) uses it; building debris (chunk 5) will too.
//!
//! It is deliberately cheap: gravity, drag, spin, restitution against a
//! ground height the caller supplies for the chunk's position (the island
//! top, a piece's top, or nothing over the void), no avian bodies, no
//! chunk–chunk or chunk–player contact. Pure and seeded, so it is tested
//! directly (`tests/kill_feedback.rs`).

use bevy::prelude::*;

/// Default gravity for chunks (m/s²): a little heavier than the player's, so
/// armor reads as heavy metal.
pub const CHUNK_GRAVITY: f32 = 22.0;
/// Below this downward speed a landing stops bouncing and settles (m/s).
pub const SETTLE_SPEED: f32 = 1.2;
/// Grip while sliding on the ground (1/s) and the spin it scrubs (1/s).
const GROUND_GRIP: f32 = 6.0;
const GROUND_SPIN_GRIP: f32 = 5.0;
/// Substep length (s), so long frames stay stable.
const SUBSTEP: f32 = 1.0 / 120.0;

/// One chunk's state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chunk {
    /// Centre of mass (world).
    pub pos: Vec3,
    pub vel: Vec3,
    pub rot: Quat,
    /// Angular velocity (axis × rad/s).
    pub spin: Vec3,
    pub age: f32,
    /// Seconds until it is gone.
    pub life: f32,
    /// Fraction of life after which it shrinks away.
    pub shrink_start: f32,
    /// Resting half-height above the ground (m).
    pub radius: f32,
    /// Restitution on a landing (0 dead, 1 perfectly bouncy).
    pub restitution: f32,
    pub gravity: f32,
    /// Linear drag (1/s).
    pub drag: f32,
    /// Landings so far.
    pub bounces: u32,
}

impl Default for Chunk {
    fn default() -> Self {
        Self {
            pos: Vec3::ZERO,
            vel: Vec3::ZERO,
            rot: Quat::IDENTITY,
            spin: Vec3::ZERO,
            age: 0.0,
            life: 1.5,
            shrink_start: 0.7,
            radius: 0.08,
            restitution: 0.35,
            gravity: CHUNK_GRAVITY,
            drag: 0.15,
            bounces: 0,
        }
    }
}

/// What a step did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChunkStep {
    /// Still alive after the step.
    pub alive: bool,
    /// It hit the ground for the first time during the step (clatter now).
    pub first_bounce: bool,
}

impl Chunk {
    /// Advances `dt` seconds. `ground(p)` is the height of the ground under
    /// `p` (None over the void: it falls on).
    pub fn step(&mut self, dt: f32, ground: impl Fn(Vec3) -> Option<f32>) -> ChunkStep {
        let mut out = ChunkStep {
            alive: true,
            first_bounce: false,
        };
        if dt <= 0.0 {
            out.alive = self.age < self.life;
            return out;
        }
        self.age += dt;
        if self.age >= self.life {
            out.alive = false;
            return out;
        }
        // The ground is sampled once per step: chunks move little per frame.
        let floor = ground(self.pos).map(|g| g + self.radius);
        let steps = (dt / SUBSTEP).ceil().clamp(1.0, 16.0);
        let h = dt / steps;
        for _ in 0..steps as usize {
            let on_ground = floor.is_some_and(|f| self.pos.y <= f + 1e-4) && self.vel.y <= 0.0;
            if !on_ground {
                self.vel.y -= self.gravity * h;
            }
            self.vel *= (1.0 - self.drag * h).max(0.0);
            self.pos += self.vel * h;
            let turn = self.spin * h;
            if turn != Vec3::ZERO {
                self.rot = (Quat::from_scaled_axis(turn) * self.rot).normalize();
            }
            let Some(floor) = floor else { continue };
            if self.pos.y > floor {
                continue;
            }
            self.pos.y = floor;
            if self.vel.y < 0.0 {
                let impact = -self.vel.y;
                if self.bounces == 0 {
                    out.first_bounce = true;
                }
                self.bounces += 1;
                self.vel.y = if impact > SETTLE_SPEED {
                    impact * self.restitution
                } else {
                    0.0
                };
                self.vel.x *= 0.65;
                self.vel.z *= 0.65;
                // A landing knocks the tumble about.
                self.spin *= 0.6;
            }
            if self.vel.y == 0.0 {
                let grip = (-GROUND_GRIP * h).exp();
                self.vel.x *= grip;
                self.vel.z *= grip;
                self.spin *= (-GROUND_SPIN_GRIP * h).exp();
            }
        }
        out
    }

    /// Scale multiplier: 1 until [`Chunk::shrink_start`], then eased to 0.
    pub fn shrink(&self) -> f32 {
        let t = (self.age / self.life.max(1e-4)).clamp(0.0, 1.0);
        if t <= self.shrink_start {
            return 1.0;
        }
        let u = ((t - self.shrink_start) / (1.0 - self.shrink_start).max(1e-4)).clamp(0.0, 1.0);
        1.0 - u * u * (3.0 - 2.0 * u)
    }

    /// Resting on the ground (bounced, not moving).
    pub fn is_resting(&self) -> bool {
        self.bounces > 0 && self.vel.length() < 0.2
    }
}

/// Caps how many clatter sounds play at once: a start is allowed only while
/// fewer than `voices` started within the last `length` seconds. Fixed size,
/// no allocation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClatterGate {
    starts: [f64; 8],
    voices: usize,
    length: f64,
}

impl ClatterGate {
    /// At most `voices` (up to 8) at once, each lasting `length` seconds.
    pub fn new(voices: u32, length: f32) -> Self {
        Self {
            starts: [f64::NEG_INFINITY; 8],
            voices: (voices as usize).clamp(1, 8),
            length: f64::from(length.max(0.0)),
        }
    }

    /// Voices still sounding at `now`.
    pub fn sounding(&self, now: f64) -> usize {
        self.starts[..self.voices]
            .iter()
            .filter(|s| now - **s < self.length)
            .count()
    }

    /// Takes a voice at `now` if one is free.
    pub fn try_start(&mut self, now: f64) -> bool {
        let length = self.length;
        match self.starts[..self.voices]
            .iter_mut()
            .find(|s| now - **s >= length)
        {
            Some(slot) => {
                *slot = now;
                true
            }
            None => false,
        }
    }
}
