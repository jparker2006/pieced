//! Pure, frame-rate independent effect math: springs, camera shake, hitstop,
//! slot pools, particle integration and tracer streaks. No ECS, so it's tested
//! directly (`tests/fx.rs`).

use bevy::prelude::*;

/// A critically damped spring pulling a value toward a target. The update is the
/// exact solution for any `dt`, so it never explodes on a long frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    /// Angular frequency (rad/s): higher is snappier. A kick peaks after `1/omega`
    /// seconds and is within ~5% of rest after about `5.5/omega`.
    pub omega: f32,
}

impl Spring {
    pub const fn new(omega: f32) -> Self {
        Self { omega }
    }

    /// Advances `x` (with velocity `v`) toward `target` by `dt` seconds.
    pub fn step(&self, x: &mut Vec3, v: &mut Vec3, target: Vec3, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        let w = self.omega.max(0.01);
        let y = *x - target;
        let e = (-w * dt).exp();
        let k = *v + y * w;
        *x = target + (y + k * dt) * e;
        *v = (*v - k * (w * dt)) * e;
    }

    /// Peak displacement produced by an instant velocity kick `v0` from rest.
    pub fn peak_for_kick(&self, v0: f32) -> f32 {
        v0 / (self.omega * std::f32::consts::E)
    }

    /// The velocity kick that peaks at displacement `peak`.
    pub fn kick_for_peak(&self, peak: f32) -> f32 {
        peak * self.omega * std::f32::consts::E
    }
}

/// Camera shake cap: at most this many degrees per axis, times the tuning's
/// `camera_shake` (0 disables it).
pub const SHAKE_MAX_DEG: f32 = 0.5;
/// Trauma lost per second.
pub const SHAKE_DECAY: f32 = 3.2;

/// Trauma-based camera shake: events add trauma (0..=1), the offset scales with
/// trauma squared and decays in about a third of a second.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Shake {
    pub trauma: f32,
    time: f32,
}

impl Shake {
    pub fn add(&mut self, amount: f32) {
        self.trauma = (self.trauma + amount.max(0.0)).min(1.0);
    }

    pub fn step(&mut self, dt: f32) {
        self.time += dt.max(0.0);
        self.trauma = (self.trauma - SHAKE_DECAY * dt.max(0.0)).max(0.0);
    }

    /// (pitch, yaw, roll) offset in radians for a shake strength setting.
    pub fn angles(&self, strength: f32) -> Vec3 {
        let amp = SHAKE_MAX_DEG.to_radians() * strength.max(0.0) * self.trauma * self.trauma;
        if amp <= 0.0 {
            return Vec3::ZERO;
        }
        let t = self.time;
        // Sums of two sines with weights adding to 1, so each axis stays in ±amp.
        let n =
            |f1: f32, f2: f32, p: f32| 0.6 * (t * f1 + p).sin() + 0.4 * (t * f2 + 2.0 * p).sin();
        Vec3::new(
            amp * n(41.0, 67.0, 0.3),
            amp * n(37.0, 59.0, 1.7),
            amp * 0.5 * n(29.0, 53.0, 4.1),
        )
    }
}

/// Hitstop bookkeeping: freezes game time for a number of rendered frames.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Hitstop {
    frames_left: u32,
    fresh: bool,
}

impl Hitstop {
    /// Starts a hitstop. Returns true when time should be paused now.
    pub fn trigger(&mut self, frames: u32) -> bool {
        if frames == 0 {
            return false;
        }
        self.frames_left = self.frames_left.max(frames);
        self.fresh = true;
        true
    }

    /// Call once at the end of every rendered frame. Returns true on the frame
    /// after which time should resume. The frame that triggered the hitstop has
    /// already advanced, so it doesn't count toward the frozen frames.
    pub fn end_frame(&mut self) -> bool {
        if self.fresh {
            self.fresh = false;
            return false;
        }
        if self.frames_left > 0 {
            self.frames_left -= 1;
            return self.frames_left == 0;
        }
        false
    }

    pub fn active(&self) -> bool {
        self.frames_left > 0
    }
}

/// Fixed-size slot allocation for pooled effect entities. When every slot under
/// the live limit is busy, the oldest one is recycled so new effects always show.
#[derive(Debug, Clone, Default)]
pub struct SlotPool {
    live: Vec<bool>,
    born: Vec<u64>,
    counter: u64,
}

impl SlotPool {
    pub fn new(capacity: usize) -> Self {
        Self {
            live: vec![false; capacity],
            born: vec![0; capacity],
            counter: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.live.len()
    }

    /// Takes a slot among the first `limit` (clamped to the capacity).
    pub fn alloc(&mut self, limit: usize) -> Option<usize> {
        let limit = limit.min(self.live.len());
        if limit == 0 {
            return None;
        }
        self.counter += 1;
        let slot = (0..limit)
            .find(|&i| !self.live[i])
            .unwrap_or_else(|| (0..limit).min_by_key(|&i| self.born[i]).unwrap_or_default());
        self.live[slot] = true;
        self.born[slot] = self.counter;
        Some(slot)
    }

    pub fn free(&mut self, slot: usize) {
        if let Some(live) = self.live.get_mut(slot) {
            *live = false;
        }
    }

    pub fn is_live(&self, slot: usize) -> bool {
        self.live.get(slot).copied().unwrap_or(false)
    }

    pub fn live_count(&self) -> usize {
        self.live.iter().filter(|l| **l).count()
    }
}

/// One simulated effect particle or debris chunk.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Particle {
    pub pos: Vec3,
    pub vel: Vec3,
    pub rot: Quat,
    /// Angular velocity (rad/s, axis × speed).
    pub spin: Vec3,
    pub age: f32,
    pub life: f32,
    /// Base scale.
    pub size: Vec3,
    pub gravity: f32,
    /// Linear drag per second.
    pub drag: f32,
    /// Restitution off the ground plane; `None` falls through it.
    pub bounce: Option<f32>,
    pub ground: f32,
    /// Resting half-height above the ground.
    pub radius: f32,
    /// Fraction of life after which it shrinks away.
    pub shrink_start: f32,
    /// > 0 stretches the particle along its velocity (sparks): extra length per m/s.
    pub stretch: f32,
    /// Scale multiplier at birth, easing to 1 over the first 60 ms (pops).
    pub birth_scale: f32,
    /// Scale multiplier reached at the end of life, eased out (rings, pops).
    pub grow: f32,
}

impl Default for Particle {
    fn default() -> Self {
        Self {
            pos: Vec3::ZERO,
            vel: Vec3::ZERO,
            rot: Quat::IDENTITY,
            spin: Vec3::ZERO,
            age: 0.0,
            life: 0.5,
            size: Vec3::ONE,
            gravity: 0.0,
            drag: 0.0,
            bounce: None,
            ground: 0.0,
            radius: 0.05,
            shrink_start: 0.6,
            stretch: 0.0,
            birth_scale: 1.0,
            grow: 1.0,
        }
    }
}

/// Below this downward speed a landing chunk stops bouncing and settles.
const SETTLE_SPEED: f32 = 1.1;

impl Particle {
    /// Advances the particle; returns false once it has expired.
    pub fn step(&mut self, dt: f32) -> bool {
        if dt <= 0.0 {
            return self.age < self.life;
        }
        self.age += dt;
        if self.age >= self.life {
            return false;
        }
        self.vel.y -= self.gravity * dt;
        self.vel *= (1.0 - self.drag * dt).max(0.0);
        self.pos += self.vel * dt;
        let turn = self.spin * dt;
        if turn != Vec3::ZERO {
            self.rot = (Quat::from_scaled_axis(turn) * self.rot).normalize();
        }
        if let Some(restitution) = self.bounce {
            let floor = self.ground + self.radius;
            if self.pos.y <= floor {
                self.pos.y = floor;
                if self.vel.y < 0.0 {
                    let impact = -self.vel.y;
                    self.vel.y = if impact > SETTLE_SPEED {
                        impact * restitution
                    } else {
                        0.0
                    };
                    self.vel.x *= 0.6;
                    self.vel.z *= 0.6;
                    self.spin *= 0.55;
                }
                // Scrub while in contact.
                let scrub = (1.0 - 7.0 * dt).max(0.0);
                if self.vel.y == 0.0 {
                    self.vel.x *= scrub;
                    self.vel.z *= scrub;
                    self.spin *= scrub;
                }
            }
        }
        true
    }

    /// Current render scale: base size, eased in from birth and shrunk away at
    /// the end, stretched along velocity when requested.
    pub fn scale(&self) -> Vec3 {
        let t = (self.age / self.life.max(1e-4)).clamp(0.0, 1.0);
        let end = if t <= self.shrink_start {
            1.0
        } else {
            1.0 - smooth((t - self.shrink_start) / (1.0 - self.shrink_start).max(1e-4))
        };
        let birth = if self.birth_scale != 1.0 {
            let u = (self.age / 0.06).clamp(0.0, 1.0);
            self.birth_scale + (1.0 - self.birth_scale) * (1.0 - (1.0 - u) * (1.0 - u))
        } else {
            1.0
        };
        let grow = if self.grow != 1.0 {
            1.0 + (self.grow - 1.0) * (1.0 - (1.0 - t) * (1.0 - t))
        } else {
            1.0
        };
        let mut s = self.size * end * birth * grow;
        if self.stretch > 0.0 {
            s.z *= 1.0 + self.vel.length() * self.stretch;
        }
        s
    }

    /// Rotation to render with: stretched particles face along their velocity.
    pub fn render_rotation(&self) -> Quat {
        if self.stretch > 0.0 {
            let dir = self.vel.normalize_or_zero();
            if dir != Vec3::ZERO {
                return Quat::from_rotation_arc(Vec3::Z, dir);
            }
        }
        self.rot
    }

    pub fn is_resting(&self) -> bool {
        self.bounce.is_some()
            && self.pos.y <= self.ground + self.radius + 1e-3
            && self.vel.length() < 0.2
    }
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How far the head of a tracer gets ahead of its birth on the first frame (s).
pub const TRACER_LEAD: f32 = 0.014;

/// A tracer streak along a path of `length` meters, `age` seconds after the shot:
/// the (tail, head) distances from the muzzle, or `None` once it has faded. The
/// head reaches the end at half the lifetime; the visible streak is never longer
/// than `max_streak`.
pub fn tracer_segment(age: f32, life: f32, length: f32, max_streak: f32) -> Option<(f32, f32)> {
    if age >= life || length <= 0.0 {
        return None;
    }
    let head_t = ((age + TRACER_LEAD) / (life * 0.5)).min(1.0);
    let tail_t = ((age - life * 0.2) / (life * 0.8)).clamp(0.0, 1.0);
    let head = head_t * length;
    let tail = (tail_t * length).max(head - max_streak).max(0.0);
    (head - tail > 1e-3).then_some((tail, head))
}

/// Index into `steps` fade materials for normalized age `t` (0 = fresh).
pub fn fade_step(t: f32, steps: usize) -> usize {
    if steps == 0 {
        return 0;
    }
    ((t.clamp(0.0, 1.0) * steps as f32) as usize).min(steps - 1)
}

/// A tiny deterministic hash-based random stream for effect variety (not
/// gameplay): the same seed always gives the same burst.
#[derive(Debug, Clone, Copy)]
pub struct FxRng(u64);

impl FxRng {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    pub fn next_u32(&mut self) -> u32 {
        // splitmix64
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 32) as u32
    }

    /// Uniform in 0..1.
    pub fn f(&mut self) -> f32 {
        self.next_u32() as f32 / (u32::MAX as f32 + 1.0)
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f()
    }

    /// Uniform direction on the unit sphere.
    pub fn dir(&mut self) -> Vec3 {
        let z = self.range(-1.0, 1.0);
        let a = self.range(0.0, std::f32::consts::TAU);
        let r = (1.0 - z * z).max(0.0).sqrt();
        Vec3::new(r * a.cos(), r * a.sin(), z)
    }

    /// A direction within `spread` (0 = exactly `axis`, 1 = hemisphere) of `axis`.
    pub fn cone(&mut self, axis: Vec3, spread: f32) -> Vec3 {
        let axis = axis.normalize_or(Vec3::Y);
        (axis + self.dir() * spread).normalize_or(axis)
    }

    pub fn pick(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u32() as usize) % n
        }
    }
}
