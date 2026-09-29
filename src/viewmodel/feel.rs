//! Weapon feel (M4 chunk 2, D106): the weighty ADS blend, the idle breathing
//! sway, the render-only camera kick, and the presentation cues the audio
//! hooks its layered sounds on. Pure math and plain data, so it's tested
//! directly; `super` wires it into the rig.
//!
//! None of it is gameplay: the aim ray, bloom, recoil, ADS, reload and switch
//! timings are M1's (D117). The camera kick turns the rendered camera only,
//! never `LookAngles` or the eye the shots come from.

use super::anim::{bump, ease_out_back_by, smoothstep};
use crate::shared::WeaponKind;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;

/// Designer feel numbers for the guns in the hands. Never persisted
/// (`Tuning::weapons` is `#[serde(skip)]`): play-test tuning must reach
/// Jake's game. D117 allows the orchestrator ±50% on these.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct WeaponFeelTuning {
    /// Peak render-only camera kick per rifle shot (degrees, ≤ 0.3).
    pub kick_rifle_deg: f32,
    /// Peak render-only camera kick per pump shot (degrees, ≤ 0.3).
    pub kick_pump_deg: f32,
    /// Seconds from a shot until its kick has fully recovered (≤ 0.12).
    pub kick_seconds: f32,
    /// The kick's scale while aiming down sights.
    pub kick_ads_scale: f32,
    /// How far the ADS blend overshoots and settles (ease-out-back strength;
    /// 0 = none). It never lengthens the blend.
    pub ads_overshoot: f32,
    /// The roll (radians) the gun swings through going in and out of ADS.
    pub ads_swing: f32,
    /// Seconds per idle breath.
    pub breath_seconds: f32,
    /// Idle breathing sway scale (0 = off; the M1 sway switch turns it off too).
    pub breath: f32,
}

impl Default for WeaponFeelTuning {
    fn default() -> Self {
        Self {
            kick_rifle_deg: 0.12,
            kick_pump_deg: 0.3,
            kick_seconds: 0.11,
            kick_ads_scale: 0.7,
            ads_overshoot: 1.0,
            ads_swing: 0.05,
            breath_seconds: 4.2,
            breath: 1.0,
        }
    }
}

/// The hard ceilings D106 puts on the camera kick, whatever the tuning says.
pub const KICK_MAX_DEG: f32 = 0.3;
pub const KICK_MAX_SECONDS: f32 = 0.12;

// ---------------------------------------------------------------------------
// ADS with weight
// ---------------------------------------------------------------------------

/// The viewmodel's aim-down-sights blend: it runs `from` → `to` over the ADS
/// time and lands exactly on the ADS (or hip) pose at the end of it. The pose
/// overshoots a touch and settles on the way ([`AdsBlend::pose`]); the FOV,
/// sway calming and bob use the plain blend ([`AdsBlend::plain`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdsBlend {
    from: f32,
    to: f32,
    /// Progress 0..=1 through the current blend.
    t: f32,
}

impl Default for AdsBlend {
    fn default() -> Self {
        Self {
            from: 0.0,
            to: 0.0,
            t: 1.0,
        }
    }
}

impl AdsBlend {
    /// Sets the target (0 hip, 1 ADS) and advances by `dt` over a blend of
    /// `seconds`. Returns `Some(true)` on the frame aiming starts, `Some(false)`
    /// on the frame it ends, `None` otherwise. A reversal mid-blend starts from
    /// where the gun is (its pose with `overshoot`), so the gun never jumps.
    pub fn step(&mut self, target: f32, dt: f32, seconds: f32, overshoot: f32) -> Option<bool> {
        let mut changed = None;
        if (target - self.to).abs() > 1e-6 {
            self.from = self.pose(overshoot);
            self.to = target;
            self.t = 0.0;
            changed = Some(target > 0.5);
        }
        self.t = (self.t + dt / seconds.max(1e-3)).min(1.0);
        changed
    }

    /// The plain eased blend, 0..=1.
    pub fn plain(&self) -> f32 {
        self.from + (self.to - self.from) * smoothstep(self.t)
    }

    /// The pose blend: like [`Self::plain`] but overshooting by `overshoot`
    /// (ease-out-back strength) before it settles on the target.
    pub fn pose(&self, overshoot: f32) -> f32 {
        self.from + (self.to - self.from) * ease_out_back_by(self.t, overshoot)
    }

    /// The swing through the blend: 0 at both ends, +1 at its height going in,
    /// -1 coming out.
    pub fn swing(&self) -> f32 {
        (self.to - self.from) * bump(self.t)
    }

    /// Whether the blend has landed on its target.
    pub fn settled(&self) -> bool {
        self.t >= 1.0
    }

    pub fn target(&self) -> f32 {
        self.to
    }
}

// ---------------------------------------------------------------------------
// Idle breathing
// ---------------------------------------------------------------------------

/// The idle breathing sway at breath phase `phase` (radians): a slow rise and
/// fall with a lazy figure-of-eight drift. Millimetres and fractions of a
/// degree: the gun is alive in your hands, never swimming.
pub fn breath(phase: f32) -> (Vec3, Vec3) {
    let pos = Vec3::new(
        0.0010 * (0.5 * phase).sin(),
        0.0018 * phase.sin(),
        0.0006 * (phase + 1.2).sin(),
    );
    let rot = Vec3::new(
        0.0050 * (phase + 0.8).sin(),
        0.0030 * (0.5 * phase + 0.3).sin(),
        0.0025 * (0.5 * phase).sin(),
    );
    (pos, rot)
}

/// Advances a breath phase by `dt` for a breath of `seconds`.
pub fn breathe(phase: f32, dt: f32, seconds: f32) -> f32 {
    // Two breaths per 4π, so the half-rate drift loops too.
    (phase + dt * TAU / seconds.max(0.5)).rem_euclid(2.0 * TAU)
}

// ---------------------------------------------------------------------------
// Camera kick
// ---------------------------------------------------------------------------

/// Fraction of a kick spent rising to its peak (one frame at 60 fps).
const KICK_RISE: f32 = 0.15;

/// The shape of one kick over its life `u` (0..=1): a snap up to 1, then a
/// smooth recovery to exactly 0 at `u` = 1.
pub fn kick_shape(u: f32) -> f32 {
    if !(0.0..1.0).contains(&u) {
        0.0
    } else if u < KICK_RISE {
        (u / KICK_RISE * std::f32::consts::FRAC_PI_2).sin()
    } else {
        1.0 - smoothstep((u - KICK_RISE) / (1.0 - KICK_RISE))
    }
}

/// How many overlapping kicks are tracked (more than a rifle burst ever
/// overlaps in [`KICK_MAX_SECONDS`]).
const KICK_SLOTS: usize = 4;

/// The render-only camera kick: each shot snaps the rendered camera up by at
/// most its peak and recovers within the kick time. Overlapping kicks take
/// the largest, never the sum, so it never exceeds one shot's peak and is
/// gone the kick time after the last shot. Fixed size, allocates nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraKick {
    /// (age s, peak rad, direction (pitch, yaw) unit).
    slots: [(f32, f32, Vec2); KICK_SLOTS],
    next: usize,
}

impl Default for CameraKick {
    fn default() -> Self {
        Self {
            slots: [(f32::INFINITY, 0.0, Vec2::X); KICK_SLOTS],
            next: 0,
        }
    }
}

impl CameraKick {
    /// A shot: kicks by `peak` radians (clamped to [`KICK_MAX_DEG`]) along
    /// `dir` (x pitch up, y yaw; normalized).
    pub fn add(&mut self, peak: f32, dir: Vec2) {
        let peak = peak.clamp(0.0, KICK_MAX_DEG.to_radians());
        let dir = dir.try_normalize().unwrap_or(Vec2::X);
        self.slots[self.next] = (0.0, peak, dir);
        self.next = (self.next + 1) % KICK_SLOTS;
    }

    pub fn step(&mut self, dt: f32) {
        for slot in &mut self.slots {
            slot.0 += dt.max(0.0);
        }
    }

    /// The (pitch, yaw) offset in radians for a kick lasting `seconds`
    /// (clamped to [`KICK_MAX_SECONDS`]): the strongest live kick.
    pub fn angles(&self, seconds: f32) -> Vec2 {
        let seconds = seconds.clamp(1e-3, KICK_MAX_SECONDS);
        let mut best = Vec2::ZERO;
        for &(age, peak, dir) in &self.slots {
            let v = dir * peak * kick_shape(age / seconds);
            if v.length_squared() > best.length_squared() {
                best = v;
            }
        }
        best
    }
}

// ---------------------------------------------------------------------------
// Presentation cues
// ---------------------------------------------------------------------------

/// A beat in a gun's animation, for layered sounds (chunk 3 owns the sounds)
/// and any other presentation. Emitted by the viewmodel, on the frame the
/// beat shows, for the player's own held gun only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WeaponBeat {
    /// The gun starts rising in after a switch.
    Draw,
    /// Aim down sights starts (the gun swings up to the eye)...
    AdsIn,
    /// ...and ends.
    AdsOut,
    /// Rifle reload: the glass chamber slides open.
    ChamberOpen,
    /// Rifle reload: the glove flicks the dim crystal out.
    CrystalPop,
    /// Rifle reload: the glove grabs a fresh crystal from below.
    CrystalGrab,
    /// Rifle reload: the fresh crystal clicks into the socket.
    CrystalSlot,
    /// Rifle reload: the glass chamber snaps shut.
    ChamberShut,
    /// Rifle reload complete: the crystal charges up and flashes.
    CrystalCharged,
    /// Pump reload: the glove pushes a shard into the rings (every shell).
    ShardPush,
    /// Pump: the rack is pulled back (the rings whirr up)...
    RackPull,
    /// ...and slammed home: the clack.
    RackClack,
}

/// A weapon presentation cue: `beat` of `weapon`'s animation happened this
/// frame. Registered by `ViewmodelPlugin`; readers elsewhere may
/// `add_message::<WeaponCue>()` too (it's idempotent).
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeaponCue {
    pub weapon: WeaponKind,
    pub beat: WeaponBeat,
}

/// The rifle reload's beats at their fractions of the reload (the charge-up
/// comes with the reload's completion, `GameCue::ReloadDone`).
pub const RIFLE_RELOAD_BEATS: [(f32, WeaponBeat); 5] = [
    (super::anim::RIFLE_GLASS_OPEN, WeaponBeat::ChamberOpen),
    (super::anim::RIFLE_POP, WeaponBeat::CrystalPop),
    (super::anim::RIFLE_GRAB, WeaponBeat::CrystalGrab),
    (super::anim::RIFLE_SLOT, WeaponBeat::CrystalSlot),
    (super::anim::RIFLE_GLASS_SHUT, WeaponBeat::ChamberShut),
];

/// Each pump shell's beat at its fraction of the shell.
pub const SHELL_BEATS: [(f32, WeaponBeat); 1] = [(super::anim::SHARD_PUSH, WeaponBeat::ShardPush)];

/// The rack's beats, in seconds after a pump shot.
pub const RACK_BEATS: [(f32, WeaponBeat); 2] = [
    (super::anim::RACK_START, WeaponBeat::RackPull),
    (super::anim::RACK_DONE, WeaponBeat::RackClack),
];

/// The beats crossed going from `last` to `now` along a track (`last` is
/// exclusive, `now` inclusive). `None` for `last` means the track was not
/// running last frame: nothing before `now` counts, so a track first seen
/// halfway through never fires its earlier beats all at once.
pub fn crossed(
    beats: &[(f32, WeaponBeat)],
    last: Option<f32>,
    now: f32,
) -> impl Iterator<Item = WeaponBeat> + '_ {
    let last = last.unwrap_or(now);
    beats
        .iter()
        .filter(move |(at, _)| last < *at && *at <= now)
        .map(|(_, beat)| *beat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kick_shape_snaps_up_and_is_gone_at_the_end() {
        assert_eq!(kick_shape(0.0), 0.0);
        assert!((kick_shape(KICK_RISE) - 1.0).abs() < 1e-6);
        assert_eq!(kick_shape(1.0), 0.0);
        assert!((0..=100).all(|i| (0.0..=1.0).contains(&kick_shape(i as f32 / 100.0))));
    }

    #[test]
    fn ads_blend_reverses_without_a_jump() {
        let mut ads = AdsBlend::default();
        assert_eq!(ads.step(1.0, 0.05, 0.12, 1.0), Some(true));
        let mid = ads.pose(1.0);
        assert_eq!(ads.step(0.0, 0.0, 0.12, 1.0), Some(false));
        assert!((ads.pose(1.0) - mid).abs() < 1e-6);
    }
}
