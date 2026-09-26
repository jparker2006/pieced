//! Pure pose math for the viewmodel: ADS alignment, the crystal ammo glow, the
//! rifle's crystal-swap reload, the pump's shard reload, rack and spinning
//! rings, the squash-and-stretch firing kick and the weapon-switch
//! lower/raise. No ECS here, so it's easy to test.
//!
//! Offsets are in gun model space (meters, -Z toward the muzzle, +Y up, +X the
//! gun's right) unless a doc says otherwise.

use super::models::{GunSpec, PUMP_RACK_TRAVEL};
use bevy::prelude::*;
use std::f32::consts::{PI, TAU};

/// Hermite ease on 0..=1.
pub fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 0 → 1 → 0 over `t` in 0..=1 (a single soft bump).
pub fn bump(t: f32) -> f32 {
    if (0.0..=1.0).contains(&t) {
        (t * PI).sin()
    } else {
        0.0
    }
}

/// Ease out with a small overshoot past 1 (a cartoon "pop").
pub fn ease_out_back(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    const S: f32 = 1.7;
    1.0 + t * t * ((S + 1.0) * t + S)
}

/// Where `t` is between `a` and `b`, clamped to 0..=1.
fn phase(t: f32, a: f32, b: f32) -> f32 {
    ((t - a) / (b - a)).clamp(0.0, 1.0)
}

/// Rig translation that puts a gun's rear sight on the view axis,
/// `ads_distance` in front of the eye (with the gun's rotation at identity).
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

// ---------------------------------------------------------------------------
// Crystal ammo glow
// ---------------------------------------------------------------------------

/// The glow of an empty crystal (and of a fresh one before it charges).
pub const GLOW_MIN: f32 = 0.25;
/// Seconds a freshly seated rifle crystal takes to charge up.
pub const GLOW_RAMP: f32 = 0.3;

/// Crystal glow for a magazine: 0.25 + 0.75 × (rounds ÷ magazine size).
pub fn ammo_glow(ammo: u32, magazine: u32) -> f32 {
    let fraction = (ammo as f32 / magazine.max(1) as f32).clamp(0.0, 1.0);
    GLOW_MIN + (1.0 - GLOW_MIN) * fraction
}

/// The rifle crystal's glow. `reload` is the reload's progress while one runs;
/// `since_reload` is the seconds since the last reload completed.
///
/// It follows the magazine. Mid-reload the old, dim crystal keeps its glow as it
/// pops out, and the fresh one slides in dark ([`GLOW_MIN`]); the moment the
/// reload completes, the fresh crystal charges up over [`GLOW_RAMP`] with a
/// brief overshoot flash.
pub fn rifle_crystal_glow(ammo: u32, magazine: u32, reload: Option<f32>, since_reload: f32) -> f32 {
    if let Some(p) = reload {
        return if rifle_reload(p).fresh {
            GLOW_MIN
        } else {
            ammo_glow(ammo, magazine)
        };
    }
    let target = ammo_glow(ammo, magazine);
    if since_reload < GLOW_RAMP {
        GLOW_MIN + (target - GLOW_MIN) * ease_out_back(since_reload / GLOW_RAMP)
    } else {
        target
    }
}

/// The pump crystal's glow: it follows the shells loaded, and a shard counts as
/// soon as it merges into the crystal (`shell` is the current shell's progress).
pub fn pump_crystal_glow(ammo: u32, magazine: u32, shell: Option<f32>) -> f32 {
    let merged = shell.is_some_and(|p| p >= SHARD_MERGED) as u32;
    ammo_glow((ammo + merged).min(magazine), magazine)
}

// ---------------------------------------------------------------------------
// Rifle reload: the crystal swap
// ---------------------------------------------------------------------------

/// A crystal's pose relative to its socket.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrystalPose {
    pub offset: Vec3,
    /// Spin about the crystal's own vertical axis (radians).
    pub spin: f32,
    /// Tumble about the gun's axis (radians), while flying.
    pub tumble: f32,
    pub scale: f32,
}

impl CrystalPose {
    pub const SEATED: Self = Self {
        offset: Vec3::ZERO,
        spin: 0.0,
        tumble: 0.0,
        scale: 1.0,
    };

    pub fn visible(&self) -> bool {
        self.scale > 1e-3
    }

    /// The crystal's transform in model space, given its socket.
    pub fn transform(&self, socket: Vec3) -> Transform {
        Transform::from_translation(socket + self.offset)
            .with_rotation(Quat::from_rotation_z(self.tumble) * Quat::from_rotation_y(self.spin))
            .with_scale(Vec3::splat(self.scale.max(1e-4)))
    }
}

/// Everything the rifle's reload moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RifleReloadPose {
    pub gun: PoseOffset,
    pub crystal: CrystalPose,
    /// 0 = the glass chamber closed, 1 = slid open into the front collar.
    pub chamber_open: f32,
    /// Whether the crystal shown is the fresh one (dark until the reload ends).
    pub fresh: bool,
    /// How far the left glove has left the forend to carry the fresh crystal
    /// in (0 = on the forend, 1 = holding the crystal).
    pub hand: f32,
    /// Where the left glove holds, relative to the crystal socket.
    pub hand_at: Vec3,
}

/// Reload phases (fractions of the reload time).
const GLASS_OPEN: (f32, f32) = (0.06, 0.18);
const POP_OUT: (f32, f32) = (0.14, 0.42);
const SLIDE_IN: (f32, f32) = (0.46, 0.72);
const GLASS_CLOSE: (f32, f32) = (0.72, 0.84);

/// The rifle reload at progress `p` (0..=1 over the whole reload): the gun rolls
/// its chamber toward you, the glass slides open, the dim crystal pops up and
/// spins away, a fresh one slides into the socket, the glass snaps shut, and
/// the gun comes back up.
pub fn rifle_reload(p: f32) -> RifleReloadPose {
    let p = p.clamp(0.0, 1.0);
    let env = smoothstep(p / 0.12) * (1.0 - smoothstep((p - 0.84) / 0.16));
    let mut gun = PoseOffset {
        pos: Vec3::new(-0.035, 0.03, 0.04),
        euler: Vec3::new(0.14, 0.08, 0.36),
    }
    .scaled(env);
    // A little shove as the fresh crystal seats, and a clack as the glass shuts.
    let seat = bump(phase(p, 0.66, 0.78));
    let clack = bump(phase(p, 0.80, 0.88));
    gun.pos.y -= 0.010 * seat;
    gun.euler.x += 0.05 * clack;

    let open = smoothstep(phase(p, GLASS_OPEN.0, GLASS_OPEN.1))
        * (1.0 - smoothstep(phase(p, GLASS_CLOSE.0, GLASS_CLOSE.1)));

    let (crystal, fresh) = if p < POP_OUT.0 {
        // A nervous rattle while the glass opens.
        let r = phase(p, GLASS_OPEN.0, POP_OUT.0);
        (
            CrystalPose {
                offset: Vec3::new(0.0, 0.006 * bump(r), 0.0),
                spin: 0.4 * bump(r),
                ..CrystalPose::SEATED
            },
            false,
        )
    } else if p < POP_OUT.1 {
        // Pops up and spins away to the upper right, shrinking into a sparkle.
        let u = phase(p, POP_OUT.0, POP_OUT.1);
        (
            CrystalPose {
                offset: Vec3::new(0.17 * u, 0.36 * u - 0.24 * u * u, -0.06 * u),
                spin: 3.0 * TAU * (1.0 - (1.0 - u) * (1.0 - u)),
                tumble: -1.2 * u,
                scale: 1.0 - smoothstep(phase(u, 0.55, 1.0)),
            },
            false,
        )
    } else if p < SLIDE_IN.0 {
        (
            CrystalPose {
                scale: 0.0,
                ..CrystalPose::SEATED
            },
            true,
        )
    } else if p < SLIDE_IN.1 {
        // The fresh crystal slides down from above-left into the socket.
        let u = phase(p, SLIDE_IN.0, SLIDE_IN.1);
        let e = smoothstep(u);
        (
            CrystalPose {
                offset: Vec3::new(-0.05, 0.15, 0.02) * (1.0 - e),
                spin: TAU * (1.0 - e),
                tumble: 0.0,
                scale: 0.55 + 0.45 * ease_out_back(phase(u, 0.0, 0.6)),
            },
            true,
        )
    } else {
        (CrystalPose::SEATED, true)
    };
    // The left glove fetches the fresh crystal from above, carries it down
    // into the socket and goes back to the forend.
    let hand = smoothstep(phase(p, HAND_FETCH.0, HAND_FETCH.1))
        * (1.0 - smoothstep(phase(p, HAND_BACK.0, HAND_BACK.1)));
    let hand_at = if p < SLIDE_IN.0 {
        FRESH_FROM
    } else if fresh && crystal.visible() {
        crystal.offset
    } else {
        Vec3::ZERO
    };
    RifleReloadPose {
        gun,
        crystal,
        chamber_open: open,
        fresh,
        hand,
        hand_at,
    }
}

/// The left glove leaves the forend to fetch the fresh crystal as the old one
/// flies, and is back once the crystal is seated.
const HAND_FETCH: (f32, f32) = (0.30, 0.46);
const HAND_BACK: (f32, f32) = (0.72, 0.86);
/// Where the fresh crystal comes from (relative to its socket).
const FRESH_FROM: Vec3 = Vec3::new(-0.05, 0.15, 0.02);

/// The left glove's forearm direction in its grip frame (`gloves.py`'s
/// `FOREARM_L` in Bevy axes): down, and out toward the side facing you.
pub const GLOVE_L_FOREARM: Vec3 = Vec3::new(-0.422, -0.9045, 0.0603);
/// Holding a crystal, the glove rolls this far about the gun's axis (fingers
/// curling up round the crystal from below, on the side facing you)...
pub const HAND_HOLD_ROLL: f32 = -0.6;
/// ...and turns this far about the vertical so its forearm reaches back
/// toward you instead of out sideways.
pub const HAND_HOLD_YAW: f32 = 0.45;
/// The glove's grip point sits this far from the crystal it holds: below and
/// a little behind it, so the crystal shows above the fingertips.
pub const HAND_HOLD_OFFSET: Vec3 = Vec3::new(0.0, -0.055, 0.022);
/// How far the glove swings out on your side (-X) on its way, clearing the gun.
pub const HAND_SWING_OUT: f32 = 0.09;

/// The left glove leaving its `grip` to hold a crystal at `at` (both in gun
/// model space): at `t` = 0 it is on its grip, at 1 it holds the crystal from
/// below on your side ([`HAND_HOLD_OFFSET`]) with its forearm reaching back
/// toward you. On the way it swings out on your side, clear of the gun.
pub fn hand_hold(t: f32, grip: Transform, at: Vec3) -> Transform {
    let t = smoothstep(t);
    let hold = Quat::from_rotation_y(HAND_HOLD_YAW)
        * Quat::from_rotation_z(HAND_HOLD_ROLL)
        * grip.rotation;
    let target = at + HAND_HOLD_OFFSET;
    let path = grip.translation.lerp(target, t) + Vec3::new(-HAND_SWING_OUT * bump(t), 0.0, 0.0);
    Transform::from_translation(path).with_rotation(grip.rotation.slerp(hold, t))
}

/// How short the glass chamber gets when fully open (a fraction of its length).
pub const CHAMBER_OPEN_LENGTH: f32 = 0.12;

/// The glass chamber's transform (model space): it slides open by shrinking
/// toward the front collar, keeping its front end in place. `rest` is its rest
/// transform and `half_length` half its length along the barrel.
pub fn chamber_transform(rest: Transform, half_length: f32, open: f32) -> Transform {
    let s = 1.0 - (1.0 - CHAMBER_OPEN_LENGTH) * open.clamp(0.0, 1.0);
    let mut t = rest;
    t.scale.z *= s;
    // The front end (−Z) stays put: move the centre forward by what was lost.
    t.translation.z -= half_length * (1.0 - s);
    t
}

// ---------------------------------------------------------------------------
// Pump reload: crystal shards into the rings
// ---------------------------------------------------------------------------

/// The pump's reload stance (applied with a blend while reloading): the gun
/// turns broadside and a little away, its crystal cradle rolled toward you.
pub const PUMP_RELOAD_STANCE: PoseOffset = PoseOffset {
    pos: Vec3::new(-0.04, 0.03, -0.14),
    euler: Vec3::new(0.12, 0.75, 0.25),
};

/// Shell progress at which a shard has merged into the crystal (it counts).
pub const SHARD_MERGED: f32 = 0.66;

/// Where a shard starts: above the rings, a little toward you.
pub const SHARD_START: Vec3 = Vec3::new(-0.03, 0.13, 0.01);

/// One shard at per-shell progress `p` (0..=1): its pose relative to the
/// crystal socket, and where the left glove holds (relative to the socket).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShardPose {
    pub crystal: CrystalPose,
    pub hand_at: Vec3,
}

/// Each shell: the left glove carries a violet shard down in through the rings,
/// where it spins and melts into the crystal, and goes back up for the next.
pub fn pump_shard(p: f32) -> ShardPose {
    let p = p.clamp(0.0, 1.0);
    let hidden = CrystalPose {
        scale: 0.0,
        ..CrystalPose::SEATED
    };
    // Carrying the shard in, then back up for the next one.
    let hand_at = if p < 0.12 {
        SHARD_START
    } else if p < 0.52 {
        SHARD_START * (1.0 - smoothstep(phase(p, 0.12, 0.52)))
    } else {
        SHARD_START * smoothstep(phase(p, 0.56, 0.86))
    };
    let crystal = if p < 0.12 {
        hidden
    } else if p < 0.52 {
        let u = smoothstep(phase(p, 0.12, 0.52));
        CrystalPose {
            offset: SHARD_START * (1.0 - u),
            spin: 1.5 * TAU * u,
            tumble: 0.0,
            scale: 0.7 + 0.3 * ease_out_back(phase(p, 0.12, 0.3)),
        }
    } else if p < SHARD_MERGED + 0.06 {
        // Melts into the crystal.
        let u = phase(p, 0.52, SHARD_MERGED + 0.06);
        CrystalPose {
            offset: Vec3::ZERO,
            spin: 1.5 * TAU + 2.0 * u,
            tumble: 0.0,
            scale: 1.0 - smoothstep(u),
        }
    } else {
        hidden
    };
    ShardPose { crystal, hand_at }
}

/// Seconds after a pump shot when the rack starts, reaches the back, and is done.
pub const RACK_START: f32 = 0.14;
pub const RACK_BACK: f32 = 0.30;
pub const RACK_DONE: f32 = 0.47;

/// Pump grip travel (0 = forward, 1 = fully back) `age` seconds after a shot.
pub fn pump_rack(age: f32) -> f32 {
    if !(RACK_START..RACK_DONE).contains(&age) {
        0.0
    } else if age < RACK_BACK {
        smoothstep((age - RACK_START) / (RACK_BACK - RACK_START))
    } else {
        1.0 - smoothstep((age - RACK_BACK) / (RACK_DONE - RACK_BACK))
    }
}

/// The pump grip's offset along the gun (model +Z is back) for a rack fraction.
pub fn pump_grip_offset(rack: f32) -> Vec3 {
    Vec3::new(0.0, 0.0, PUMP_RACK_TRAVEL * rack)
}

// ---------------------------------------------------------------------------
// The pump's rings
// ---------------------------------------------------------------------------

/// The rings' slow magical idle spin (rad/s).
pub const RING_IDLE_SPEED: f32 = 0.8;
/// Spin added when the pump racks...
pub const RING_RACK_KICK: f32 = 17.0;
/// ...and when a shard merges into the crystal.
pub const RING_SHARD_KICK: f32 = 7.0;
/// How fast extra spin bleeds back to the idle speed (1/s).
pub const RING_DRAG: f32 = 3.0;

/// The pump's gold rings spin about the gun's axis: slowly at rest, whirring on
/// every rack and twitching as each shard goes in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RingSpin {
    pub angle: f32,
    pub speed: f32,
}

impl Default for RingSpin {
    fn default() -> Self {
        Self {
            angle: 0.0,
            speed: RING_IDLE_SPEED,
        }
    }
}

impl RingSpin {
    pub fn kick(&mut self, speed: f32) {
        self.speed += speed;
    }

    /// Advances by `dt` seconds (exact, so frame-rate independent).
    pub fn step(&mut self, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        let extra = self.speed - RING_IDLE_SPEED;
        let decay = (-RING_DRAG * dt).exp();
        self.angle =
            (self.angle + RING_IDLE_SPEED * dt + extra * (1.0 - decay) / RING_DRAG).rem_euclid(TAU);
        self.speed = RING_IDLE_SPEED + extra * decay;
    }

    /// The rings' transform (model space), given their rest transform.
    pub fn transform(&self, rest: Transform) -> Transform {
        rest.with_rotation(Quat::from_rotation_z(self.angle) * rest.rotation)
    }
}

// ---------------------------------------------------------------------------
// Squash-and-stretch firing kick
// ---------------------------------------------------------------------------

/// A bouncy (under-damped) spring for the cartoon squash on every shot: the gun
/// squashes along the barrel, stretches past rest, and settles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Squash {
    /// Angular frequency (rad/s).
    pub omega: f32,
    /// Damping ratio (< 1 bounces).
    pub zeta: f32,
}

/// The rifle's squash is quick and small; the pump's big and slower.
pub const RIFLE_SQUASH: Squash = Squash {
    omega: 42.0,
    zeta: 0.3,
};
pub const PUMP_SQUASH: Squash = Squash {
    omega: 26.0,
    zeta: 0.28,
};
/// Peak squash per shot (fraction of the gun's length).
pub const RIFLE_SQUASH_PEAK: f32 = 0.07;
pub const PUMP_SQUASH_PEAK: f32 = 0.15;

impl Squash {
    /// The velocity kick whose first squash peaks at `peak`, from rest.
    pub fn kick_for_peak(&self, peak: f32) -> f32 {
        let z = self.zeta.clamp(0.0, 0.99);
        let wd = self.omega * (1.0 - z * z).sqrt();
        let tp = (wd / (z * self.omega)).atan() / wd;
        let gain = (-z * self.omega * tp).exp() * (wd * tp).sin() / wd;
        peak / gain
    }

    /// Advances squash `x` (velocity `v`) by `dt`, in small fixed substeps so
    /// the bounce does not depend on the frame rate.
    pub fn step(&self, x: &mut f32, v: &mut f32, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        let steps = (dt / (1.0 / 480.0)).ceil().clamp(1.0, 64.0) as usize;
        let h = dt / steps as f32;
        for _ in 0..steps {
            let a = -self.omega * self.omega * *x - 2.0 * self.zeta * self.omega * *v;
            *v += a * h;
            *x += *v * h;
        }
    }
}

/// Scale for a squash amount `x` (> 0 squashes along the barrel, < 0 stretches),
/// bulging sideways so the volume stays about the same.
pub fn squash_scale(x: f32) -> Vec3 {
    let along = (1.0 - x).max(0.5);
    let across = 1.0 / along.sqrt();
    Vec3::new(across, across, along)
}

/// The squash as a transform about `pivot` (the right hand, so the gun squashes
/// into the grip rather than sliding out of it).
pub fn squash_transform(x: f32, pivot: Vec3) -> Transform {
    let scale = squash_scale(x);
    Transform::from_translation(pivot - pivot * scale).with_scale(scale)
}

// ---------------------------------------------------------------------------
// Switching
// ---------------------------------------------------------------------------

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ease_out_back_overshoots_then_lands() {
        assert_eq!(ease_out_back(0.0), 0.0);
        assert!((ease_out_back(1.0) - 1.0).abs() < 1e-6);
        assert!((1..100).any(|i| ease_out_back(i as f32 / 100.0) > 1.02));
    }

    #[test]
    fn squash_kick_peaks_where_asked() {
        for (s, peak) in [
            (RIFLE_SQUASH, RIFLE_SQUASH_PEAK),
            (PUMP_SQUASH, PUMP_SQUASH_PEAK),
        ] {
            let (mut x, mut v) = (0.0, s.kick_for_peak(peak));
            let mut max = 0.0f32;
            for _ in 0..120 {
                s.step(&mut x, &mut v, 1.0 / 240.0);
                max = max.max(x);
            }
            assert!((max - peak).abs() < 0.004, "peak {max} vs {peak}");
        }
    }
}
