//! Pure pose math for the viewmodel: ADS alignment, the crystal ammo glow, the
//! rifle's crystal-swap reload, the pump's shard reload, rack and spinning
//! rings, the squash-and-stretch firing kick and the weapon-switch
//! put-away and draw. No ECS here, so it's easy to test. Every animation
//! finishes inside its gameplay time (M4, D106): the switch in 0.2 s, the
//! rifle reload in 2.0 s, each pump shell in 0.5 s, the rack before the next
//! pump shot.
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

/// 0 → 1 → 0 over `t` in 0..=1 (a single soft bump), exactly 0 at and
/// beyond both ends.
pub fn bump(t: f32) -> f32 {
    if t > 0.0 && t < 1.0 {
        (t * PI).sin()
    } else {
        0.0
    }
}

/// Ease out with a small overshoot past 1 (a cartoon "pop").
pub fn ease_out_back(t: f32) -> f32 {
    ease_out_back_by(t, 1.7)
}

/// Ease out that overshoots past 1 and settles back exactly on 1 at `t` = 1.
/// `s` sets the overshoot: it peaks at 1 + 4s³ / (27 (s + 1)²) (about 3.7%
/// for `s` = 1, 10% for 1.7); 0 is a plain ease out.
pub fn ease_out_back_by(t: f32, s: f32) -> f32 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    let s = s.max(0.0);
    1.0 + t * t * ((s + 1.0) * t + s)
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
// Keyframes
// ---------------------------------------------------------------------------

/// How a keyframed segment eases into its key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ease {
    /// Soft start and soft landing.
    Smooth,
    /// Starts slow and arrives fast (a push, a slam).
    In,
    /// Leaves fast and lands soft (a reach, a lift).
    Out,
}

impl Ease {
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Ease::Smooth => smoothstep(t),
            Ease::In => t * t,
            Ease::Out => 1.0 - (1.0 - t) * (1.0 - t),
        }
    }
}

/// A keyframed track: `(time, value, ease into it)`, times ascending. Holds the
/// first value before the first key and the last after the last.
pub fn keyframes(t: f32, keys: &[(f32, Vec3, Ease)]) -> Vec3 {
    let Some(&(t0, v0, _)) = keys.first() else {
        return Vec3::ZERO;
    };
    if t <= t0 {
        return v0;
    }
    for pair in keys.windows(2) {
        let (a, va, _) = pair[0];
        let (b, vb, ease) = pair[1];
        if t <= b {
            return va.lerp(vb, ease.apply(phase(t, a, b)));
        }
    }
    keys.last().map_or(Vec3::ZERO, |k| k.1)
}

// ---------------------------------------------------------------------------
// Crystal ammo glow
// ---------------------------------------------------------------------------

/// The glow of an empty crystal.
pub const GLOW_MIN: f32 = 0.25;
/// The spent rifle crystal's light goes out as it flies away, so spent and
/// fresh read apart.
pub const SPENT_GLOW: f32 = 0.05;
/// A fresh rifle crystal glows this much in the glove, before it is seated...
pub const FRESH_GLOW: f32 = 0.7;
/// ...charges to this as it seats (from the slot to the reload's end)...
pub const SEATED_GLOW: f32 = 0.8;
/// ...and flashes up to full over this many seconds once the reload completes.
pub const GLOW_RAMP: f32 = 0.3;
/// How far past full the charge-up flash peaks.
pub const GLOW_FLASH: f32 = 0.4;

/// Crystal glow for a magazine: 0.25 + 0.75 × (rounds ÷ magazine size).
pub fn ammo_glow(ammo: u32, magazine: u32) -> f32 {
    let fraction = (ammo as f32 / magazine.max(1) as f32).clamp(0.0, 1.0);
    GLOW_MIN + (1.0 - GLOW_MIN) * fraction
}

/// The rifle crystal's glow. `reload` is the reload's progress while one runs;
/// `since_reload` is the seconds since the last reload completed.
///
/// It follows the magazine. Mid-reload the old, dim crystal keeps its glow
/// until the glove flicks it out, then goes dark as it flies ([`SPENT_GLOW`]);
/// the fresh one the glove brings up glows ([`FRESH_GLOW`]), starts charging
/// the moment it clicks into the socket, and the moment the reload completes
/// flashes up to full over [`GLOW_RAMP`].
pub fn rifle_crystal_glow(ammo: u32, magazine: u32, reload: Option<f32>, since_reload: f32) -> f32 {
    if let Some(p) = reload {
        return if !rifle_reload(p).fresh {
            let spent = smoothstep(phase(p, RIFLE_POP, RIFLE_POP + 0.1));
            ammo_glow(ammo, magazine) + (SPENT_GLOW - ammo_glow(ammo, magazine)) * spent
        } else if p < RIFLE_SLOT {
            FRESH_GLOW
        } else {
            FRESH_GLOW + (SEATED_GLOW - FRESH_GLOW) * smoothstep(phase(p, RIFLE_SLOT, 1.0))
        };
    }
    let target = ammo_glow(ammo, magazine);
    if since_reload < GLOW_RAMP {
        let u = since_reload / GLOW_RAMP;
        SEATED_GLOW + (target - SEATED_GLOW) * smoothstep(u) + GLOW_FLASH * bump(u)
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
    /// How far the gun is in its reload stance (0 = at the hip, 1 = in
    /// `models::RIFLE_RELOAD`).
    pub stance: f32,
    /// The accents on top of the stance: the flick, the lean toward the
    /// fetching hand, the shove as the crystal seats, the glass's clack.
    pub gun: PoseOffset,
    pub crystal: CrystalPose,
    /// 0 = the glass chamber closed, 1 = slid open into the front collar.
    pub chamber_open: f32,
    /// Whether the crystal shown is the fresh one.
    pub fresh: bool,
    /// How far the left glove has left the forend to work the crystal
    /// (0 = on the forend, 1 = at [`Self::hand_at`]).
    pub hand: f32,
    /// Where the left glove holds, relative to the crystal socket.
    pub hand_at: Vec3,
}

/// The rifle reload's beats (fractions of its 2.0 s): the glass slides open,
/// the glove flicks the dim crystal out, grabs a fresh one from below, and
/// slots it in with a click; the glass shuts.
pub const RIFLE_GLASS_OPEN: f32 = 0.07;
pub const RIFLE_POP: f32 = 0.22;
pub const RIFLE_GRAB: f32 = 0.5;
pub const RIFLE_SLOT: f32 = 0.8;
pub const RIFLE_GLASS_SHUT: f32 = 0.87;

const GLASS_OPEN: (f32, f32) = (RIFLE_GLASS_OPEN, 0.17);
const GLASS_CLOSE: (f32, f32) = (RIFLE_SLOT, RIFLE_GLASS_SHUT);
/// The old crystal's flight after the flick.
const POP_OUT: (f32, f32) = (RIFLE_POP, 0.46);
/// The glove leaves the forend for the chamber, and is back at the end.
const HAND_OUT: (f32, f32) = (0.03, 0.16);
const HAND_BACK: (f32, f32) = (0.84, 0.97);

/// Where the glove reaches down to for the fresh crystal (socket-relative):
/// well below the gun and a little toward you, off the bottom of the screen.
pub const RIFLE_FETCH: Vec3 = Vec3::new(-0.12, -0.22, 0.09);
/// Where the glove lines the fresh crystal up, beside the open socket on your
/// side, before it pushes it home.
const RIFLE_STAGE: Vec3 = Vec3::new(-0.075, -0.025, 0.015);

/// The left glove's path through the reload (socket-relative): under the
/// crystal, a flick up that pops it out, down out of view to fetch a fresh
/// one, up under the socket, and a push home.
const RIFLE_HAND_KEYS: [(f32, Vec3, Ease); 10] = [
    (0.16, Vec3::new(0.0, -0.012, 0.0), Ease::Smooth),
    (0.195, Vec3::new(0.0, -0.03, 0.004), Ease::Out),
    (RIFLE_POP, Vec3::new(0.004, 0.022, -0.004), Ease::In),
    (0.28, Vec3::new(0.012, 0.03, -0.006), Ease::Out),
    (0.46, RIFLE_FETCH, Ease::Smooth),
    (RIFLE_GRAB, Vec3::new(-0.12, -0.232, 0.09), Ease::Out),
    (0.62, RIFLE_STAGE, Ease::Out),
    (0.73, Vec3::new(-0.069, -0.027, 0.015), Ease::Smooth),
    (RIFLE_SLOT, Vec3::ZERO, Ease::In),
    (0.83, Vec3::new(0.0, -0.004, 0.0), Ease::Out),
];

/// The rifle reload at progress `p` (0..=1 over the whole reload): the gun
/// turns its chamber toward you and the glass slides open; the left glove
/// flicks the dim crystal up and out (it spins away), drops out of view,
/// comes back up with a fresh glowing crystal, lines it up beside the socket
/// and pushes it home with a click; the glass snaps shut and the gun comes
/// back up. Everything is back at rest by `p` = 1.
pub fn rifle_reload(p: f32) -> RifleReloadPose {
    let p = p.clamp(0.0, 1.0);
    let stance = smoothstep(p / 0.14) * (1.0 - smoothstep(phase(p, 0.86, 1.0)));
    let mut gun = PoseOffset::default();
    // The flick jerks the muzzle up; the gun leans toward the hand as it
    // fetches; the push from below lifts it, and the glass clacks shut.
    let flick = bump(phase(p, 0.19, 0.3));
    let lean = bump(phase(p, 0.28, 0.72));
    let shove = bump(phase(p, RIFLE_SLOT - 0.01, 0.9));
    let clack = bump(phase(p, RIFLE_GLASS_SHUT - 0.01, 0.93));
    gun.pos.y += 0.006 * flick + 0.012 * shove;
    gun.euler.x += 0.06 * flick - 0.045 * shove + 0.03 * clack;
    gun.euler.z += 0.07 * lean;

    let open = smoothstep(phase(p, GLASS_OPEN.0, GLASS_OPEN.1))
        * (1.0 - smoothstep(phase(p, GLASS_CLOSE.0, GLASS_CLOSE.1)));

    let hand = smoothstep(phase(p, HAND_OUT.0, HAND_OUT.1))
        * (1.0 - smoothstep(phase(p, HAND_BACK.0, HAND_BACK.1)));
    let hand_at = keyframes(p, &RIFLE_HAND_KEYS);

    let fresh = p >= POP_OUT.1;
    let crystal = if p < RIFLE_POP {
        // A nervous rattle as the glass opens and the glove gets under it.
        let r = phase(p, 0.1, RIFLE_POP);
        CrystalPose {
            offset: Vec3::new(0.0, 0.005 * bump(r) * (r * 3.0 * TAU).cos().abs(), 0.0),
            spin: 0.4 * bump(r),
            ..CrystalPose::SEATED
        }
    } else if p < POP_OUT.1 {
        // Flicked up, it spins away to the upper right, shrinking into a
        // sparkle.
        let u = phase(p, POP_OUT.0, POP_OUT.1);
        CrystalPose {
            offset: Vec3::new(0.17 * u, 0.36 * u - 0.24 * u * u, -0.06 * u),
            spin: 3.0 * TAU * (1.0 - (1.0 - u) * (1.0 - u)),
            tumble: -1.2 * u,
            scale: 1.0 - smoothstep(phase(u, 0.55, 1.0)),
        }
    } else if p < RIFLE_GRAB - 0.02 {
        // Gone; the glove is out of view fetching the next.
        CrystalPose {
            scale: 0.0,
            ..CrystalPose::SEATED
        }
    } else if p < RIFLE_SLOT {
        // In the glove: it turns a little as it comes up and squares up to
        // the socket before it goes in.
        let u = phase(p, RIFLE_GRAB - 0.02, RIFLE_SLOT);
        CrystalPose {
            offset: hand_at,
            spin: 0.9 * TAU * (1.0 - smoothstep(u)),
            tumble: 0.3 * (1.0 - smoothstep(u)),
            scale: 0.6 + 0.4 * ease_out_back(phase(p, RIFLE_GRAB - 0.02, RIFLE_GRAB + 0.04)),
        }
    } else {
        // Seated with a click: a little swell as it snaps home.
        CrystalPose {
            scale: 1.0 + 0.12 * bump(phase(p, RIFLE_SLOT, RIFLE_SLOT + 0.07)),
            ..CrystalPose::SEATED
        }
    };
    RifleReloadPose {
        stance,
        gun,
        crystal,
        chamber_open: open,
        fresh,
        hand,
        hand_at,
    }
}

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
    hand_hold_by(t, grip, at, HAND_HOLD_OFFSET)
}

/// [`hand_hold`] with the glove's grip point `offset` from what it holds
/// (the pump's shards are small, so the glove holds them lower down, at
/// [`SHARD_HOLD_OFFSET`], to keep them in view above its fingers).
pub fn hand_hold_by(t: f32, grip: Transform, at: Vec3, offset: Vec3) -> Transform {
    let t = smoothstep(t);
    let hold = Quat::from_rotation_y(HAND_HOLD_YAW)
        * Quat::from_rotation_z(HAND_HOLD_ROLL)
        * grip.rotation;
    let target = at + offset;
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

/// Shell progress at which the glove's push lands: the shard is in the rings.
pub const SHARD_PUSH: f32 = 0.46;
/// Shell progress at which a shard has merged into the crystal (it counts).
pub const SHARD_MERGED: f32 = 0.66;

/// Where a shard is lined up before the push: above the rings, a little
/// toward you.
pub const SHARD_START: Vec3 = Vec3::new(-0.03, 0.13, 0.01);
/// Where the glove picks up each shard: down on your side of the gun.
pub const SHARD_FETCH: Vec3 = Vec3::new(-0.11, -0.03, 0.09);
/// The glove's grip point below a shard it holds (lower than for the rifle's
/// crystal: the shard is small, and shows above the fingertips).
pub const SHARD_HOLD_OFFSET: Vec3 = Vec3::new(0.0, -0.088, 0.01);
/// A shard shows this big in the fingertips (it shrinks to its real size as
/// it's pushed into the rings), so the glove's load reads at a glance.
pub const SHARD_HELD_SCALE: f32 = 1.8;

/// One shard at per-shell progress `p` (0..=1): its pose relative to the
/// crystal socket, and where the left glove holds (relative to the socket).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShardPose {
    pub crystal: CrystalPose,
    pub hand_at: Vec3,
}

/// The glove's loop through one shell (socket-relative): it picks a shard up
/// on your side, swings it up over the rings, pushes it down in hard, holds
/// a beat, and drops back down for the next. It ends where it started, so
/// shells chain without a seam.
const SHARD_HAND_KEYS: [(f32, Vec3, Ease); 6] = [
    (0.06, SHARD_FETCH, Ease::Smooth),
    (0.3, SHARD_START, Ease::Out),
    (SHARD_PUSH, Vec3::ZERO, Ease::In),
    (0.5, Vec3::new(0.0, -0.012, 0.0), Ease::Out),
    (0.56, Vec3::new(0.0, -0.006, 0.0), Ease::Smooth),
    (0.94, SHARD_FETCH, Ease::Smooth),
];

/// Each shell: the left glove picks up a violet shard, lifts it over the
/// rings and pushes it down in (the gun dips with the push), where it spins
/// and melts into the crystal; then the glove drops back for the next.
pub fn pump_shard(p: f32) -> ShardPose {
    let p = p.clamp(0.0, 1.0);
    let hand_at = keyframes(p, &SHARD_HAND_KEYS);
    let hidden = CrystalPose {
        scale: 0.0,
        ..CrystalPose::SEATED
    };
    let crystal = if p < 0.03 {
        hidden
    } else if p < SHARD_PUSH {
        // In the glove: it pops into being as the glove closes on it and
        // spins up as it rises.
        let u = phase(p, 0.03, SHARD_PUSH);
        CrystalPose {
            offset: hand_at,
            spin: 1.5 * TAU * smoothstep(u),
            tumble: 0.0,
            scale: (0.7 + 0.3 * ease_out_back(phase(p, 0.03, 0.2)))
                * (1.0 + (SHARD_HELD_SCALE - 1.0) * (1.0 - smoothstep(phase(p, 0.3, SHARD_PUSH)))),
        }
    } else if p < SHARD_MERGED + 0.06 {
        // Pushed home, it melts into the crystal.
        let u = phase(p, SHARD_PUSH, SHARD_MERGED + 0.06);
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

/// The dip the gun takes as each shard is pushed in (added to the stance).
pub fn shard_push_dip(p: f32) -> PoseOffset {
    let k = bump(phase(p, SHARD_PUSH - 0.08, SHARD_PUSH + 0.14));
    PoseOffset {
        pos: Vec3::new(0.0, -0.012, 0.0),
        euler: Vec3::new(-0.05, 0.0, 0.03),
    }
    .scaled(k)
}

/// Seconds after a pump shot when the rack starts, reaches the back, starts
/// forward again, and slams home (the clack). A heavy, deliberate rack:
/// a strong pull, a beat at the back, and a fast slam forward, all done long
/// before the next shot (0.9 s).
pub const RACK_START: f32 = 0.13;
pub const RACK_BACK: f32 = 0.3;
pub const RACK_HOLD: f32 = 0.34;
pub const RACK_DONE: f32 = 0.43;
/// How long the gun jolts after the clack.
pub const RACK_JOLT: f32 = 0.14;

/// Pump grip travel (0 = forward, 1 = fully back) `age` seconds after a shot.
pub fn pump_rack(age: f32) -> f32 {
    if !(RACK_START..RACK_DONE).contains(&age) {
        0.0
    } else if age < RACK_BACK {
        smoothstep((age - RACK_START) / (RACK_BACK - RACK_START))
    } else if age < RACK_HOLD {
        1.0
    } else {
        // Slammed forward: accelerates all the way home.
        let u = (age - RACK_HOLD) / (RACK_DONE - RACK_HOLD);
        1.0 - u * u
    }
}

/// The whole gun's motion during the rack, `age` seconds after a shot: it
/// tips and rolls toward the pull, then jolts forward with the clack.
pub fn rack_pose(age: f32) -> PoseOffset {
    let pull = PoseOffset {
        pos: Vec3::new(0.008, 0.004, 0.03),
        euler: Vec3::new(0.2, 0.08, -0.24),
    }
    .scaled(pump_rack(age));
    let jolt = PoseOffset {
        pos: Vec3::new(0.0, 0.004, -0.03),
        euler: Vec3::new(-0.09, -0.02, 0.06),
    }
    .scaled(bump(phase(age, RACK_DONE, RACK_DONE + RACK_JOLT)));
    pull + jolt
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
pub const RING_RACK_KICK: f32 = 22.0;
/// ...and when a shard merges into the crystal.
pub const RING_SHARD_KICK: f32 = 7.0;
/// How fast extra spin bleeds back to the idle speed (1/s).
pub const RING_DRAG: f32 = 3.4;

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
// Switching: put away, then draw
// ---------------------------------------------------------------------------

/// The fully lowered pose used for switching and for build mode.
pub const LOWERED: PoseOffset = PoseOffset {
    pos: Vec3::new(0.03, -0.30, 0.08),
    euler: Vec3::new(-0.7, -0.1, -0.35),
};

/// The fraction of the switch spent putting the old item away; the draw
/// takes the rest.
pub const DRAW_SPLIT: f32 = 0.4;
/// How far the draw overshoots as it comes up (the ease-out-back strength).
pub const DRAW_OVERSHOOT: f32 = 1.4;

/// Weapon switch at progress `t` (0..=1 over the switch time): the old item
/// drops away quickly, then the new one rises in from below, overshoots a
/// touch (muzzle up) and settles exactly on its pose at `t` = 1.
/// Returns (show the previous item, how lowered: 1 = fully down, 0 = up,
/// below 0 while the draw overshoots).
pub fn switch_phase(t: f32) -> (bool, f32) {
    let t = t.clamp(0.0, 1.0);
    if t < DRAW_SPLIT {
        let u = t / DRAW_SPLIT;
        (true, u * u * (3.0 - 2.0 * u))
    } else {
        let u = (t - DRAW_SPLIT) / (1.0 - DRAW_SPLIT);
        (false, 1.0 - ease_out_back_by(u, DRAW_OVERSHOOT))
    }
}

/// The draw's flourish on top of the rise (`lowered` from [`switch_phase`]
/// while drawing): the gun rolls in as it comes up, leading with the muzzle.
pub fn draw_twist(t: f32) -> PoseOffset {
    let t = t.clamp(0.0, 1.0);
    if t < DRAW_SPLIT {
        return PoseOffset::default();
    }
    let u = (t - DRAW_SPLIT) / (1.0 - DRAW_SPLIT);
    PoseOffset {
        pos: Vec3::ZERO,
        euler: Vec3::new(0.0, 0.06, 0.12),
    }
    .scaled(bump(u) * (1.0 - u))
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
