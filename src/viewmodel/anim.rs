//! Pure pose math for the viewmodel: ADS alignment, reload keyframes, the pump
//! rack and the weapon-switch lower/raise. No ECS here, so it's easy to test.

use super::models::{GunSpec, PUMP_LOADING_PORT, PUMP_RACK_TRAVEL};
use bevy::prelude::*;

/// Hermite ease on 0..=1.
pub fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 0 → 1 → 0 over `t` in 0..=1 (a single soft bump).
pub fn bump(t: f32) -> f32 {
    if (0.0..=1.0).contains(&t) {
        (t * std::f32::consts::PI).sin()
    } else {
        0.0
    }
}

/// Rig translation that puts a gun's sight point on the view axis, `ads_distance`
/// in front of the eye (with the gun's rotation at identity).
pub fn ads_translation(spec: &GunSpec) -> Vec3 {
    Vec3::new(0.0, 0.0, -spec.ads_distance) - spec.sight
}

/// Rotation from (pitch, yaw, roll) radians: yaw about +Y, then pitch about +X,
/// then roll about +Z (positive pitch raises the muzzle, positive yaw swings it
/// left, positive roll tips the top to the left).
pub fn euler(e: Vec3) -> Quat {
    Quat::from_euler(EulerRot::YXZ, e.y, e.x, e.z)
}

/// A pose offset: translation plus (pitch, yaw, roll).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PoseOffset {
    pub pos: Vec3,
    pub euler: Vec3,
}

impl PoseOffset {
    pub fn scaled(self, k: f32) -> Self {
        Self {
            pos: self.pos * k,
            euler: self.euler * k,
        }
    }
}

impl std::ops::Add for PoseOffset {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self {
            pos: self.pos + rhs.pos,
            euler: self.euler + rhs.euler,
        }
    }
}

/// Everything the rifle's magazine reload moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RifleReloadPose {
    pub gun: PoseOffset,
    /// Magazine offset from its seat (rifle model space).
    pub mag: PoseOffset,
    pub mag_visible: bool,
}

/// Rifle reload at progress `p` (0..=1 over the whole reload): the gun dips and
/// tilts its magwell toward you, the old magazine drops away, a fresh one rises,
/// is slapped home, and the gun comes back up.
pub fn rifle_reload(p: f32) -> RifleReloadPose {
    let p = p.clamp(0.0, 1.0);
    let env = smoothstep(p / 0.12) * (1.0 - smoothstep((p - 0.84) / 0.16));
    let mut gun = PoseOffset {
        pos: Vec3::new(-0.025, -0.05, 0.03),
        euler: Vec3::new(0.14, 0.16, -0.50),
    }
    .scaled(env);
    // The slap: a quick upward bump as the magazine seats.
    let slap = bump((p - 0.70) / 0.10);
    gun.pos.y += 0.014 * slap;
    gun.euler.x += 0.05 * slap;

    let (mag, mag_visible) = if p < 0.14 {
        (PoseOffset::default(), true)
    } else if p < 0.32 {
        let u = (p - 0.14) / 0.18;
        (
            PoseOffset {
                pos: Vec3::new(0.01 * u, -0.30 * u * u, 0.03 * u),
                euler: Vec3::new(0.5 * u * u, 0.0, 0.2 * u),
            },
            true,
        )
    } else if p < 0.46 {
        (PoseOffset::default(), false)
    } else if p < 0.70 {
        let u = smoothstep((p - 0.46) / 0.24);
        let from = PoseOffset {
            pos: Vec3::new(0.02, -0.22, 0.04),
            euler: Vec3::new(-0.35, 0.0, 0.15),
        };
        let to = PoseOffset {
            pos: Vec3::new(0.0, -0.014, 0.0),
            euler: Vec3::ZERO,
        };
        (from.scaled(1.0 - u) + to.scaled(u), true)
    } else if p < 0.74 {
        let u = (p - 0.70) / 0.04;
        (
            PoseOffset {
                pos: Vec3::new(0.0, -0.014 * (1.0 - u), 0.0),
                euler: Vec3::ZERO,
            },
            true,
        )
    } else {
        (PoseOffset::default(), true)
    };
    RifleReloadPose {
        gun,
        mag,
        mag_visible,
    }
}

/// The pump's reload stance (applied with a blend while reloading).
pub const PUMP_RELOAD_STANCE: PoseOffset = PoseOffset {
    pos: Vec3::new(-0.035, 0.03, 0.01),
    euler: Vec3::new(0.18, 0.20, -0.80),
};

/// One shell going in at per-shell progress `p` (0..=1): the shell's position
/// in pump model space (pointing along the magazine tube), whether it shows,
/// and a small push on the gun as it seats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShellPose {
    pub pos: Vec3,
    pub tilt: f32,
    pub visible: bool,
    pub gun_push: f32,
}

pub fn pump_shell(p: f32) -> ShellPose {
    let p = p.clamp(0.0, 1.0);
    let below = PUMP_LOADING_PORT + Vec3::new(0.05, -0.09, 0.07);
    let into_tube = PUMP_LOADING_PORT + Vec3::new(0.0, 0.03, -0.09);
    let push = 0.008 * bump((p - 0.58) / 0.2);
    if p < 0.12 {
        ShellPose {
            pos: below,
            tilt: 0.6,
            visible: false,
            gun_push: push,
        }
    } else if p < 0.55 {
        let u = smoothstep((p - 0.12) / 0.43);
        ShellPose {
            pos: below.lerp(PUMP_LOADING_PORT, u),
            tilt: 0.6 * (1.0 - u),
            visible: true,
            gun_push: push,
        }
    } else if p < 0.80 {
        let u = smoothstep((p - 0.55) / 0.25);
        ShellPose {
            pos: PUMP_LOADING_PORT.lerp(into_tube, u),
            tilt: 0.0,
            visible: true,
            gun_push: push,
        }
    } else {
        ShellPose {
            pos: into_tube,
            tilt: 0.0,
            visible: false,
            gun_push: push,
        }
    }
}

/// Seconds after a pump shot when the rack starts, reaches the back, and is done.
pub const RACK_START: f32 = 0.14;
pub const RACK_BACK: f32 = 0.30;
pub const RACK_DONE: f32 = 0.47;

/// Forend travel (0 = forward, 1 = fully back) `age` seconds after a pump shot.
pub fn pump_rack(age: f32) -> f32 {
    if age < RACK_START || age >= RACK_DONE {
        0.0
    } else if age < RACK_BACK {
        smoothstep((age - RACK_START) / (RACK_BACK - RACK_START))
    } else {
        1.0 - smoothstep((age - RACK_BACK) / (RACK_DONE - RACK_BACK))
    }
}

/// Forend offset along the gun for a rack fraction.
pub fn forend_offset(rack: f32) -> Vec3 {
    Vec3::new(0.0, 0.0, PUMP_RACK_TRAVEL * rack)
}

/// The fully lowered pose used for switching and for build mode.
pub const LOWERED: PoseOffset = PoseOffset {
    pos: Vec3::new(0.03, -0.30, 0.08),
    euler: Vec3::new(-0.7, -0.1, -0.35),
};

/// Weapon switch at progress `t` (0..=1 over the switch time): the old item
/// lowers during the first half, the new one rises during the second.
/// Returns (show the previous item, how lowered 0..=1).
pub fn switch_phase(t: f32) -> (bool, f32) {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        (true, smoothstep(t / 0.5))
    } else {
        (false, 1.0 - smoothstep((t - 0.5) / 0.5))
    }
}
