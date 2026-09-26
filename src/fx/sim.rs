//! Pure, frame-rate independent effect math: springs, camera shake, hitstop,
//! slot pools, particle integration, bolt flight, billboards and the dropped
//! hat. No ECS, so it's tested
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

// ---------------------------------------------------------------------------
// Spells (Milestone 2): bolt flight, billboards, the dropped hat
// ---------------------------------------------------------------------------

/// A bolt lands on its hit point this many rendered frames after the frame
/// its shot registered (the hit frame, frame 0). Gate S6: "the bolt reaches its
/// hit point within 2 frames"; the impact itself shows on frame 0.
pub const BOLT_ARRIVAL_FRAMES: u32 = 2;

/// How far along its path (0..=1) a bolt's head is on the `frame`-th rendered
/// frame of its flight (0 = the hit frame), given where it is on frames 0 and
/// 1 (`early`). Flight counts rendered frames, not seconds, so the head lands
/// on frame [`BOLT_ARRIVAL_FRAMES`] whatever the frame rate or a hitstop does.
pub fn bolt_progress(frame: u32, early: [f32; 2]) -> f32 {
    match frame {
        0 => early[0].clamp(0.0, 1.0),
        1 => early[1].max(early[0]).clamp(0.0, 1.0),
        _ => 1.0,
    }
}

/// The fraction along a straight path (0..=1) whose projection sits `s` of the
/// way across the screen from the path's start to its end, when the start is
/// `z0` and the end `z1` deep along the view (m). Perspective crowds a long
/// shot's far half into a few pixels, so a bolt painted half-way between gun
/// and target (T03) is only a little way down its real path.
pub fn screen_to_path(s: f32, z0: f32, z1: f32) -> f32 {
    let (z0, z1, s) = (z0.max(0.05), z1.max(0.05), s.clamp(0.0, 1.0));
    (s * z0 / (z1 - s * (z1 - z0)).max(1e-4)).clamp(0.0, 1.0)
}

/// A rifle bolt's first two frames (as [`bolt_progress`]'s `early`): its head
/// crosses [`RIFLE_SCREEN_FLIGHT`] of the way across the screen, for a start
/// `z0` and an end `z1` deep along the view.
pub fn rifle_flight(z0: f32, z1: f32) -> [f32; 2] {
    RIFLE_SCREEN_FLIGHT.map(|s| screen_to_path(s, z0, z1))
}

/// How far across the screen, from the gun to the hit, a rifle bolt's head is
/// on the hit frame and the frame after (it lands on the next).
pub const RIFLE_SCREEN_FLIGHT: [f32; 2] = [0.3, 0.6];

/// A pump spark's first two frames: it bursts out of the bell and fans out in
/// front of the gun (T04) whatever the pellet's range, then lands.
pub fn pellet_flight(length: f32) -> [f32; 2] {
    let length = length.max(1e-3);
    [(1.2 / length).min(0.3), (3.2 / length).min(0.62)]
}

/// A camera-facing rotation for a flat card authored facing +Z, rolled by
/// `roll` about the view axis.
pub fn billboard(camera: Quat, roll: f32) -> Quat {
    camera * Quat::from_rotation_z(roll)
}

/// Rotation for a long card authored along +Z with its face on +Y (streaks,
/// ribbons): its length runs along `dir` and it turns about that axis to face
/// `to_camera` as squarely as it can.
pub fn axial_billboard(dir: Vec3, to_camera: Vec3) -> Quat {
    let z = dir.normalize_or(Vec3::NEG_Z);
    let mut y = to_camera - z * to_camera.dot(z);
    if y.length_squared() < 1e-8 {
        y = z.any_orthonormal_vector();
    }
    let y = y.normalize();
    let x = y.cross(z);
    Quat::from_mat3(&Mat3::from_cols(x, y, z)).normalize()
}

/// Size (m) that keeps something at least `min_angle` radians wide on screen
/// at `distance`, and never smaller than `base`.
pub fn apparent_size(base: f32, distance: f32, min_angle: f32) -> f32 {
    base.max(distance.max(0.0) * min_angle)
}

/// Where the `index`-th of `count` dizzy stars sits (relative to the orbit's
/// centre) `t` seconds in: evenly spaced on a flat ellipse, bobbing gently.
pub fn orbit_offset(t: f32, index: usize, count: usize, radius: f32, speed: f32) -> Vec3 {
    let a = speed * t + std::f32::consts::TAU * index as f32 / count.max(1) as f32;
    Vec3::new(
        radius * a.cos(),
        0.03 * (2.0 * a).sin(),
        radius * 0.62 * a.sin(),
    )
}

/// The dropped hat's physics: it pops up off the knight's head, spins like a
/// top, lands on the ground, bounces, wobbles and settles (T08). Pure, so it
/// is tested directly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HatBody {
    /// The hat's pivot (the middle of its brim's base).
    pub pos: Vec3,
    pub vel: Vec3,
    /// Heading about +Y (rad) and its spin rate (rad/s).
    pub yaw: f32,
    pub spin: f32,
    /// Lean (x: about the heading's right axis, y: about its forward axis) and
    /// its rate: a damped wobble kicked by landings.
    pub tilt: Vec2,
    pub tilt_vel: Vec2,
    /// Ground height under the hat and the pivot's height above it at rest.
    pub ground: f32,
    pub rest: f32,
    /// Lean it settles at, so it lies a little askew.
    pub rest_tilt: Vec2,
    pub age: f32,
    pub landed: bool,
    pub settled: bool,
}

/// A cartoon drop: it falls fast, so it is down on the grass (still spinning)
/// by about 0.45 s, the moment T08 shows.
pub const HAT_GRAVITY: f32 = 30.0;
/// Restitution of a landing and the slowest landing that still bounces (m/s).
pub const HAT_RESTITUTION: f32 = 0.1;
pub const HAT_BOUNCE_SPEED: f32 = 1.6;
/// Wobble spring (1/s², 1/s).
const HAT_TILT_K: f32 = 160.0;
const HAT_TILT_C: f32 = 11.0;
/// Spin friction on the grass: viscous (1/s) plus a constant drag (rad/s²),
/// so a spinning hat comes to a stop instead of creeping forever.
const HAT_SPIN_DRAG: f32 = 2.5;
const HAT_SPIN_STOP: f32 = 4.0;

impl HatBody {
    /// A hat launched from `pos` with `vel`, spinning at `spin` rad/s, over
    /// ground at `ground`, resting `rest` above it.
    pub fn launch(pos: Vec3, vel: Vec3, yaw: f32, spin: f32, ground: f32, rest: f32) -> Self {
        Self {
            pos,
            vel,
            yaw,
            spin,
            tilt: Vec2::ZERO,
            tilt_vel: Vec2::new(2.5, -1.5),
            ground,
            rest,
            rest_tilt: Vec2::new(0.1, -0.06),
            age: 0.0,
            landed: false,
            settled: false,
        }
    }

    /// Advances `dt` seconds (sub-stepped, so long frames stay stable).
    pub fn step(&mut self, dt: f32) {
        if self.settled || dt <= 0.0 {
            return;
        }
        let steps = (dt / (1.0 / 240.0)).ceil().clamp(1.0, 64.0);
        let h = dt / steps;
        for _ in 0..steps as usize {
            self.substep(h);
        }
    }

    fn substep(&mut self, h: f32) {
        self.age += h;
        let floor = self.ground + self.rest;
        let on_ground = self.pos.y <= floor + 1e-4 && self.vel.y <= 0.0;
        if !on_ground {
            self.vel.y -= HAT_GRAVITY * h;
            self.spin *= (1.0 - 0.3 * h).max(0.0);
        }
        self.pos += self.vel * h;
        if self.pos.y <= floor {
            self.pos.y = floor;
            if self.vel.y < 0.0 {
                let impact = -self.vel.y;
                self.landed = true;
                self.tilt_vel += Vec2::new(0.45, -0.3) * impact;
                self.vel.y = if impact > HAT_BOUNCE_SPEED {
                    impact * HAT_RESTITUTION
                } else {
                    0.0
                };
                self.vel.x *= 0.55;
                self.vel.z *= 0.55;
            }
            if self.vel.y == 0.0 {
                // Sliding and spinning down on the grass.
                let grip = (-7.0 * h).exp();
                self.vel.x *= grip;
                self.vel.z *= grip;
                let slowed = self.spin.abs() * (-HAT_SPIN_DRAG * h).exp() - HAT_SPIN_STOP * h;
                self.spin = self.spin.signum() * slowed.max(0.0);
            }
        }
        self.yaw = (self.yaw + self.spin * h).rem_euclid(std::f32::consts::TAU);
        let target = if self.landed {
            self.rest_tilt
        } else {
            Vec2::ZERO
        };
        let a = (target - self.tilt) * HAT_TILT_K - self.tilt_vel * HAT_TILT_C;
        self.tilt_vel += a * h;
        self.tilt += self.tilt_vel * h;
        self.tilt = self.tilt.clamp(Vec2::splat(-0.5), Vec2::splat(0.5));
        let still = self.vel.length() < 0.03
            && self.spin.abs() < 0.12
            && self.tilt_vel.length() < 0.05
            && (self.tilt - self.rest_tilt).length() < 0.01;
        if self.landed && self.pos.y <= floor + 1e-4 && still {
            self.settled = true;
            self.vel = Vec3::ZERO;
            self.spin = 0.0;
            self.tilt = self.rest_tilt;
            self.tilt_vel = Vec2::ZERO;
        }
    }

    /// The hat's pose.
    pub fn transform(&self) -> Transform {
        let lean = Quat::from_rotation_x(self.tilt.x) * Quat::from_rotation_z(self.tilt.y);
        Transform::from_translation(self.pos).with_rotation(Quat::from_rotation_y(self.yaw) * lean)
    }
}
