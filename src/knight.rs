//! The knight's procedural animation and eyes (docs/M2-SPEC.md → The knight,
//! Amendment B: big cartoon hit reactions, D49).
//!
//! The knight model (`art/blender/assets/knight.py`) hangs its parts from joint
//! pivots. [`KnightAnim`] turns the character's motion and combat events into a
//! [`KnightPose`] every frame, and [`write_pose`] writes that pose onto
//! the pivots found by [`rig_knights`]:
//!
//! - a goofy idle: the torso bobs and rocks side to side, the head counters it,
//!   the arms sway, the robe swings and the hat's floppy tip wobbles;
//! - a run cycle from the velocity: a strong forward lean, boots swinging and
//!   gauntlets pumping, a bouncy stride (the torso rises in each flight and the
//!   body squashes at each footfall), the robe trailing and swaying with the
//!   hips, the cape streaming out and the hat tip flopping back; sideways
//!   strafes side-step without crossing the boots;
//! - an air pose while airborne, a stretch on jump take-off and a squash on landing;
//! - a hit **take**: in two frames the arms fling up (one higher, flailing),
//!   one boot kicks up toward the shooter, the body leans back with the head
//!   tipped forward against it so his wide eyes face the shooter, and he hops
//!   and slides back a little; he holds it for a beat, then drops back with a
//!   bounce. Heavy damage (a close pump blast), several hits in one frame, a
//!   headshot and a shield break make it bigger; while running he
//!   flinches without stopping. Each take flings the other arm high, so under
//!   fire he flails;
//! - a hit wobble spring (he rocks away from the hit) and a nod;
//! - a directional flinch (M4, D105) on top of the take: where the shot
//!   landed ([`HitRegion`]: head, chest, his left or right, a leg) those parts
//!   are kicked away from it on springs: the head snaps back, the torso is
//!   punched back, a side hit twists that shoulder back and flings its arm, a
//!   leg hit kicks the leg out and dips him;
//! - a headshot dents his helmet ([`KnightPose::dented`]) until he goes down
//!   or respawns; going down (or being flung into the void) takes his armor
//!   off ([`KnightPose::armor`]): `fx::armor` flings the pieces, and
//!   [`write_armor`] hides them on him;
//! - a respawn pop-in: scale springs up from zero and overshoots, with a warm
//!   sparkle ([`RespawnSparkle`]) flaring at his chest;
//! - on a headshot the hat pops well clear of his helmet, knocked away from
//!   the hit, cocked over, tipped toward the shooter and swelling, then drops
//!   back onto his head (T06);
//! - eyes: open, blinking every 2–5 s at random, wide after a hit (longer after
//!   a big one), X when eliminated. Each state is a mesh; the pose picks which
//!   one shows.
//!
//! Every pose is visual only: the hop and slide move the model under the
//! figure ([`KnightPose::offset`]), never the character, so its hitboxes stay
//! where the simulation put them.
//!
//! On elimination he shows his X eyes for [`KO_TIME`] in a big take and hides
//! his hat (the elimination effect drops a hat prop in its place), then the
//! figure hides until respawn. The core is pure and seeded, so it is tested
//! headless (`tests/knight.rs`); the systems only gather input and write
//! transforms. The animation runs on `Time` (virtual): pausing virtual time
//! freezes the pose, and so does a kill's hitstop
//! ([`HitstopFrozen`](crate::shared::HitstopFrozen)). The gallery's [`GalleryFreeze`] also freezes it (clock,
//! eyes, springs, the take and the respawn sparkle) without pausing the game:
//! see [`freeze_knights`].
//!
//! Directions in [`KnightInput`] and [`KnightPose`] are in the knight's model
//! space: +Y up, -Z forward, +X to his right. [`write_pose`] converts to
//! the glTF nodes' space under the model's forward fix.

use crate::{
    look::{Halo, ModelDressed, Outline, ToonMaterial, warmup::Warmup},
    models::{MODEL_FORWARD_FIX, ModelParts},
    rng::Rng,
    shared::{FreezableTime, GalleryFreeze},
};
use bevy::{ecs::query::QueryFilter, prelude::*};
use std::f32::consts::{PI, TAU};

/// The model's name in `assets/models/manifest.json`.
pub const KNIGHT_MODEL: &str = "knight";
/// Rim multiplier for the knight's toon material: a strong warm rim so he reads
/// against the purple sky.
pub const KNIGHT_RIM: f32 = 1.3;

// ---------------------------------------------------------------------------
// Tuning (the spec's feel numbers live here; they are presentation only)
// ---------------------------------------------------------------------------

/// Idle bob: torso rise (m) and rate (Hz), and the goofy side-to-side rock of
/// the torso at half that rate (rad).
pub const IDLE_BOB: f32 = 0.02;
pub const IDLE_HZ: f32 = 1.2;
pub const IDLE_ROCK: f32 = 0.055;
/// Distance covered per full run cycle (m): 5.5 m/s runs at about 3.2 steps/s.
pub const STRIDE: f32 = 1.7;
/// Peak boot swing and gauntlet pump at full run (radians).
pub const LEG_SWING: f32 = 0.8;
pub const ARM_SWING: f32 = 0.95;
/// Boots swinging towards each other move this much less (no crossing).
pub const INWARD_SWING: f32 = 0.22;
/// Run bob (m: the torso rises in each flight), squash at each footfall
/// (scale), lean into the move (rad) and cape trail (rad) at full run.
pub const RUN_BOB: f32 = 0.035;
pub const RUN_SQUASH: f32 = 0.05;
pub const RUN_LEAN: f32 = 0.3;
pub const CAPE_TRAIL: f32 = 0.6;
/// The robe's hem trails behind the run and sways with the hips (rad).
pub const ROBE_TRAIL: f32 = 0.26;
pub const ROBE_SWAY: f32 = 0.12;
/// Speed of a full run (m/s): the run pose's full strength.
pub const RUN_SPEED: f32 = 5.5;
/// Squash spring (scale deviation) and its kicks (1/s).
pub const SQUASH_K: f32 = 170.0;
pub const SQUASH_C: f32 = 9.0;
pub const JUMP_STRETCH: f32 = 2.6;
pub const LAND_SQUASH: f32 = 3.2;
/// Hit wobble spring (lean of the top, rad) and its kick (rad/s).
pub const WOBBLE_K: f32 = 110.0;
pub const WOBBLE_C: f32 = 6.5;
pub const WOBBLE_KICK: f32 = 2.4;
/// The hit take: it snaps in with this time constant (s), so it is nearly full
/// two frames after the hit, holds this long (s; longer for a big one), then
/// springs back to rest with a little bounce.
pub const TAKE_ATTACK: f32 = 0.012;
pub const TAKE_HOLD: f32 = 0.22;
pub const TAKE_HOLD_BIG: f32 = 0.32;
pub const TAKE_K: f32 = 55.0;
pub const TAKE_C: f32 = 8.0;
/// Take strengths: a body hit; each extra hit landing in the same frame adds
/// this, up to a cap; damage past a rifle body hit adds one per this much
/// (a close pump blast), up to a cap; a headshot; a shield break. 1.3 and up
/// counts as big.
pub const TAKE_BODY: f32 = 1.0;
pub const TAKE_PER_HIT: f32 = 0.06;
pub const TAKE_HITS_MAX: f32 = 1.4;
pub const TAKE_HEAVY_FROM: f32 = 30.0;
pub const TAKE_HEAVY_SPAN: f32 = 45.0;
pub const TAKE_HEAVY_MAX: f32 = 1.45;
pub const TAKE_HEADSHOT: f32 = 1.4;
pub const TAKE_SHIELD_BREAK: f32 = 1.55;
pub const TAKE_BIG: f32 = 1.3;
/// While running he flinches without stopping: the take scales down to this
/// at full run.
pub const TAKE_RUNNING: f32 = 0.35;
/// The take's pose at strength 1 (rad): the high and the low arm fling up and
/// out (capped), reaching toward the shooter and flailing; one boot kicks up
/// toward the shooter (capped); the torso and the whole body lean back while
/// the head tips forward against them, so his wide eyes (and the hat's
/// crown) stay turned to the shooter; the robe's hem lags toward the shooter.
pub const FLING_HIGH: f32 = 2.2;
pub const FLING_LOW: f32 = 1.4;
pub const FLING_MAX: f32 = 2.8;
pub const REACH: f32 = 0.3;
pub const FLAIL: f32 = 0.13;
pub const FLAIL_HZ: f32 = 9.0;
pub const KICK: f32 = 0.85;
pub const KICK_MAX: f32 = 1.15;
pub const TAKE_LEAN: f32 = 0.18;
pub const TAKE_TILT: f32 = 0.07;
pub const TAKE_HEAD: f32 = -0.2;
pub const ROBE_TAKE: f32 = 0.3;
/// The knockback, visual only: a little hop (m/s up, m/s² down; scaled by the
/// square root of the strength) and a slide back along the push (m/s, scaled
/// by the strength) that springs back under the character.
pub const HOP_SPEED: f32 = 1.5;
pub const HOP_GRAVITY: f32 = 20.0;
pub const KNOCK_SPEED: f32 = 1.9;
pub const KNOCK_K: f32 = 70.0;
pub const KNOCK_C: f32 = 12.0;
/// The hat's floppy tip: a spring (rad) chasing the idle wobble or the run's
/// trail, kicked by hits, jumps and landings.
pub const TIP_K: f32 = 60.0;
pub const TIP_C: f32 = 4.5;
/// Respawn pop-in spring (scale from 0 to 1, overshooting).
pub const POP_K: f32 = 240.0;
pub const POP_C: f32 = 13.0;
/// Headshot hat pop (T06): launch speed (m/s), gravity (m/s²), restitution. It
/// peaks about 0.44 m clear of the helmet 0.2 s after the hit, then drops back
/// onto his head with a small bounce.
pub const HAT_POP: f32 = 4.4;
pub const HAT_GRAVITY: f32 = 22.0;
pub const HAT_BOUNCE: f32 = 0.35;
/// While popped the hat drifts sideways, away from the side the shot hit
/// (m per m of lift), and cocks over the same way (rad per m of lift, capped),
/// so it reads as knocked off rather than just lifted.
pub const HAT_DRIFT: f32 = 0.45;
pub const HAT_TILT: f32 = 1.3;
pub const HAT_TILT_MAX: f32 = 0.55;
/// It also tips its crown toward whoever shot it (rad per m of lift, capped),
/// so from a low eye it shows its crown and star rather than its brim's
/// underside.
pub const HAT_TIP: f32 = 1.2;
pub const HAT_TIP_MAX: f32 = 0.5;
/// And it swells as it flies, cartoon-style (extra scale per m of lift),
/// back to its own size by the time it lands on his helmet.
pub const HAT_SWELL: f32 = 0.8;
/// The directional flinch (M4, D105): a spring kick on the parts nearest the
/// hit, away from the shot, on top of the take. Its spring (1/s², 1/s: a
/// little bouncy, settled in about 0.4 s) and kicks (rad/s): the head snapping
/// back on a headshot, the torso punched back on a chest hit, the torso
/// twisting the hit shoulder back and that arm flung back on a side hit, the
/// hit leg kicked out from under him on a leg hit (with a dip).
pub const FLINCH_K: f32 = 180.0;
pub const FLINCH_C: f32 = 13.0;
pub const FLINCH_HEAD: f32 = 13.0;
pub const FLINCH_CHEST: f32 = 8.0;
pub const FLINCH_TWIST: f32 = 9.0;
pub const FLINCH_ARM: f32 = 18.0;
pub const FLINCH_LEG: f32 = 15.0;
pub const FLINCH_DIP: f32 = 2.2;
/// Hit regions (model space, feet at the origin): below this height is a
/// leg hit; this far off the middle (m) is a side hit.
pub const FLINCH_LEGS_BELOW: f32 = 0.72;
pub const FLINCH_SIDE: f32 = 0.12;
/// Eyes: blink every 2–5 s for this long; wide this long after a hit (and
/// after a big one).
pub const BLINK_EVERY: (f32, f32) = (2.0, 5.0);
pub const BLINK_TIME: f32 = 0.13;
pub const WIDE_TIME: f32 = 0.3;
pub const WIDE_TIME_BIG: f32 = 0.45;
/// On elimination: X eyes and a slump for this long, then the figure hides.
pub const KO_TIME: f32 = 0.35;
/// The respawn sparkle's life (s).
pub const SPARKLE_TIME: f32 = 0.45;
/// Springs integrate in steps no longer than this (s).
const SUBSTEP: f32 = 1.0 / 240.0;

// ---------------------------------------------------------------------------
// The pure animation core
// ---------------------------------------------------------------------------

/// Which eye mesh shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EyeState {
    #[default]
    Open,
    Blink,
    Wide,
    X,
}

impl EyeState {
    pub const ALL: [EyeState; 4] = [Self::Open, Self::Blink, Self::Wide, Self::X];

    /// The model's part-name prefix for this state (`EyeWideL`, ...).
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Open => "Eye",
            Self::Blink => "EyeBlink",
            Self::Wide => "EyeWide",
            Self::X => "EyeX",
        }
    }
}

/// What the animation reads from the character each frame (model space).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KnightInput {
    /// Velocity in the knight's own frame (m/s).
    pub velocity: Vec3,
    pub grounded: bool,
    /// Eliminated (or dead) and not yet respawned.
    pub downed: bool,
}

impl Default for KnightInput {
    fn default() -> Self {
        Self {
            velocity: Vec3::ZERO,
            grounded: true,
            downed: false,
        }
    }
}

/// One-off moments the animation reacts to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KnightEvent {
    Jump,
    /// Touched down at `speed` m/s (downwards).
    Land {
        speed: f32,
    },
    /// Hit by a shot pushing along `push` (model space; only its horizontal
    /// direction matters). The hits of one frame make one take, bigger the
    /// more of them land.
    Hit {
        push: Vec3,
        headshot: bool,
    },
    /// The damage a hit this frame dealt: heavy hits (a close pump blast)
    /// make bigger takes (send it with the frame's `Hit`s).
    Damage {
        amount: f32,
    },
    /// The hit this frame broke his shield: the biggest take (send it with
    /// the frame's `Hit`s).
    ShieldBreak,
    /// Where a hit landed (M4): the parts there flinch away from the shot,
    /// along `push` (model space, horizontal). Send it with the frame's `Hit`s.
    Flinch {
        region: HitRegion,
        push: Vec3,
    },
    /// His armor comes off now (flung into the void: it comes apart on the
    /// fall). An elimination sheds it too.
    ShedArmor,
}

/// Where a hit landed on him, for the directional flinch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HitRegion {
    Head,
    Chest,
    /// His own left (model -X) and right (+X).
    Left,
    Right,
    /// Below the hips; `left` is his own left leg.
    Legs {
        left: bool,
    },
}

impl HitRegion {
    /// The region of a hit at `local` (model space: feet at the origin, +X
    /// his right, -Z his front). Headshots are the head whatever the point.
    pub fn classify(local: Vec3, headshot: bool) -> Self {
        if headshot {
            Self::Head
        } else if local.y < FLINCH_LEGS_BELOW {
            Self::Legs {
                left: local.x < 0.0,
            }
        } else if local.x < -FLINCH_SIDE {
            Self::Left
        } else if local.x > FLINCH_SIDE {
            Self::Right
        } else {
            Self::Chest
        }
    }
}

/// The pose: offsets from the model's rest pose, in model space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KnightPose {
    /// Whole-model scale about the feet (squash, stretch, respawn pop).
    pub scale: Vec3,
    /// Whole-model lean about the feet (the hit wobble and the take's lean).
    pub tilt: Quat,
    /// Whole-model offset from the character's feet: the take's hop and slide
    /// back. Visual only; the character (and its hitboxes) never moves.
    pub offset: Vec3,
    /// Torso (upper body) offset and rotation about the hips.
    pub torso_offset: Vec3,
    pub torso: Quat,
    /// Boots and gauntlets about the hips and shoulders: `[left, right]`.
    pub legs: [Quat; 2],
    pub arms: [Quat; 2],
    pub head: Quat,
    pub cape: Quat,
    /// The robe about the waist.
    pub robe: Quat,
    pub hat_offset: Vec3,
    pub hat: Quat,
    /// The hat's floppy tip about its bend.
    pub hat_tip: Quat,
    /// Hat scale about its pivot (1 on his head).
    pub hat_scale: f32,
    pub hat_visible: bool,
    pub eyes: EyeState,
    /// False once an elimination's KO beat is over, until respawn.
    pub visible: bool,
    /// How far into a hit take he is: 0 at rest, the take's strength at its
    /// peak (a little below 0 as it bounces back).
    pub take: f32,
    /// A headshot has dented his helmet (M4): the dented helmet shows until
    /// he is eliminated or respawns.
    pub dented: bool,
    /// His armor (helmet, gauntlets, boots) is on him: false once it has come
    /// off (an elimination, or flung into the void), until respawn.
    pub armor: bool,
}

impl KnightPose {
    pub const REST: Self = Self {
        scale: Vec3::ONE,
        tilt: Quat::IDENTITY,
        offset: Vec3::ZERO,
        torso_offset: Vec3::ZERO,
        torso: Quat::IDENTITY,
        legs: [Quat::IDENTITY; 2],
        arms: [Quat::IDENTITY; 2],
        head: Quat::IDENTITY,
        cape: Quat::IDENTITY,
        robe: Quat::IDENTITY,
        hat_offset: Vec3::ZERO,
        hat: Quat::IDENTITY,
        hat_tip: Quat::IDENTITY,
        hat_scale: 1.0,
        hat_visible: true,
        eyes: EyeState::Open,
        visible: true,
        take: 0.0,
        dented: false,
        armor: true,
    };
}

/// A damped spring: `x'' = k (target - x) - c x'`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Spring<T> {
    x: T,
    v: T,
}

impl<T> Spring<T>
where
    T: Copy
        + std::ops::Add<Output = T>
        + std::ops::Sub<Output = T>
        + std::ops::Mul<f32, Output = T>,
{
    fn step(&mut self, target: T, k: f32, c: f32, dt: f32) {
        let a = (target - self.x) * k - self.v * c;
        self.v = self.v + a * dt;
        self.x = self.x + self.v * dt;
    }
}

fn smoothstep(lo: f32, hi: f32, x: f32) -> f32 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Rotation that moves the bottom of something hanging below its pivot along
/// `-dir` (and its top along `dir`) by `angle`.
fn swing(dir: Vec3, angle: f32) -> Quat {
    let axis = Vec3::Y.cross(Vec3::new(dir.x, 0.0, dir.z));
    axis.try_normalize()
        .map_or(Quat::IDENTITY, |axis| Quat::from_axis_angle(axis, angle))
}

/// The hits that landed since the last step, made into one take by it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct PendingHit {
    /// Sum of the hits' horizontal push directions (model x, z).
    push: Vec2,
    hits: u32,
    damage: f32,
    headshot: bool,
    shield_break: bool,
}

/// The directional flinch's springs (angles, rad) and the push they flinch
/// along (model x, z).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Flinch {
    push: Vec2,
    /// The torso leaning back along the push, and twisting the hit shoulder back.
    lean: Spring<f32>,
    twist: Spring<f32>,
    /// The head snapping back.
    head: Spring<f32>,
    /// Each arm and leg flung back along the push: `[left, right]`.
    arms: [Spring<f32>; 2],
    legs: [Spring<f32>; 2],
}

impl Flinch {
    /// Adds a velocity kick, capped so a pump's many pellets don't pile up.
    fn kick(spring: &mut Spring<f32>, v: f32) {
        let cap = v.abs() * 1.5;
        spring.v = (spring.v + v).clamp(-cap, cap);
    }

    fn hit(&mut self, region: HitRegion, push: Vec2) {
        self.push = push.normalize_or(Vec2::Y);
        let pz = self.push.y;
        match region {
            HitRegion::Head => Self::kick(&mut self.head, FLINCH_HEAD),
            HitRegion::Chest => {
                Self::kick(&mut self.lean, FLINCH_CHEST);
                Self::kick(&mut self.head, FLINCH_HEAD * 0.3);
            }
            HitRegion::Left | HitRegion::Right => {
                // The hit shoulder (x = s) goes back along the push.
                let (s, i) = if region == HitRegion::Left {
                    (-1.0, 0)
                } else {
                    (1.0, 1)
                };
                let twist = -s * pz.clamp(-1.0, 1.0);
                Self::kick(&mut self.twist, FLINCH_TWIST * twist);
                Self::kick(&mut self.arms[i], FLINCH_ARM);
                Self::kick(&mut self.lean, FLINCH_CHEST * 0.4);
            }
            HitRegion::Legs { left } => {
                Self::kick(&mut self.legs[usize::from(!left)], FLINCH_LEG);
                Self::kick(&mut self.lean, -FLINCH_CHEST * 0.35);
            }
        }
    }

    fn step(&mut self, h: f32) {
        for s in [&mut self.lean, &mut self.twist, &mut self.head]
            .into_iter()
            .chain(self.arms.iter_mut())
            .chain(self.legs.iter_mut())
        {
            s.step(0.0, FLINCH_K, FLINCH_C, h);
        }
    }
}

/// One knight's animation state. Put it on the figure entity; see the module docs.
#[derive(Component, Debug, Clone)]
pub struct KnightAnim {
    /// Seconds animated.
    pub time: f32,
    /// Run-cycle phase (radians).
    phase: f32,
    /// Smoothed horizontal speed (m/s) and move direction (model space).
    speed: f32,
    dir: Vec3,
    /// 0 grounded .. 1 airborne, smoothed.
    air: f32,
    squash: Spring<f32>,
    wobble: Spring<Vec2>,
    nod: Spring<f32>,
    /// The take: its level (a spring once the hold is over), its strength, the
    /// hold left (s), which arm is high (eased toward ±1, +1 his right) and the
    /// push it leans away from (model x, z).
    take: Spring<f32>,
    take_goal: f32,
    take_hold: f32,
    take_side: f32,
    take_side_goal: f32,
    take_push: Vec2,
    pending: Option<PendingHit>,
    /// The knockback: hop height (m) and speed, and the slide (model x, z).
    hop: f32,
    hop_speed: f32,
    knock: Spring<Vec2>,
    /// The hat tip's flop (rad about model X and Z).
    tip: Spring<Vec2>,
    hat_height: f32,
    hat_speed: f32,
    hat_spin: Spring<f32>,
    /// Which way (±1 along his X) the last headshot knocked the hat.
    hat_side: f32,
    /// Respawn pop-in scale, while it settles.
    pop: Option<Spring<f32>>,
    blink_in: f32,
    blink_left: f32,
    wide_left: f32,
    downed: bool,
    ko_time: f32,
    /// The directional flinch (M4).
    flinch: Flinch,
    /// A headshot dented the helmet; the armor has come off (M4).
    dented: bool,
    armor_off: bool,
    /// Held by the gallery ([`GalleryFreeze`]): steps advance no time.
    frozen: bool,
    rng: Rng,
}

impl KnightAnim {
    pub fn new(seed: u64) -> Self {
        let mut rng = Rng::new(seed ^ 0x4B_4E_49_47_48_54);
        let blink_in = rng.range(BLINK_EVERY.0, BLINK_EVERY.1);
        let side = if rng.chance(0.5) { 1.0 } else { -1.0 };
        Self {
            time: 0.0,
            phase: 0.0,
            speed: 0.0,
            dir: Vec3::NEG_Z,
            air: 0.0,
            squash: Spring::default(),
            wobble: Spring::default(),
            nod: Spring::default(),
            take: Spring::default(),
            take_goal: 0.0,
            take_hold: 0.0,
            take_side: side,
            take_side_goal: side,
            take_push: Vec2::Y,
            pending: None,
            hop: 0.0,
            hop_speed: 0.0,
            knock: Spring::default(),
            tip: Spring::default(),
            hat_height: 0.0,
            hat_speed: 0.0,
            hat_spin: Spring::default(),
            hat_side: 1.0,
            pop: None,
            blink_in,
            blink_left: 0.0,
            wide_left: 0.0,
            downed: false,
            ko_time: 0.0,
            flinch: Flinch::default(),
            dented: false,
            armor_off: false,
            frozen: false,
            rng,
        }
    }

    /// A headshot has dented his helmet (until elimination or respawn).
    pub fn is_dented(&self) -> bool {
        self.dented
    }

    /// His armor has come off (until respawn).
    pub fn armor_is_off(&self) -> bool {
        self.armor_off
    }

    /// Holds the animation clock: while frozen, [`KnightAnim::step`] advances no
    /// time, so the pose, eyes (blink, wide), take and springs stay exactly as
    /// they are. Events and elimination still register and play out once
    /// thawed. Driven by [`freeze_knights`] from the gallery's [`GalleryFreeze`].
    pub fn set_frozen(&mut self, frozen: bool) {
        self.frozen = frozen;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// Eliminated and not yet back (as of the last [`KnightAnim::step`]).
    pub fn is_downed(&self) -> bool {
        self.downed
    }

    /// Reacts to a one-off moment (applied on the next [`KnightAnim::step`]
    /// that isn't frozen).
    pub fn event(&mut self, event: KnightEvent) {
        if self.downed {
            return;
        }
        match event {
            KnightEvent::Jump => {
                self.squash.v += JUMP_STRETCH;
                self.tip.v.x -= 4.0;
            }
            KnightEvent::Land { speed } => {
                let k = (speed.abs() / 9.0).clamp(0.35, 1.0);
                self.squash.v -= LAND_SQUASH * k;
                self.tip.v.x += 6.0 * k;
            }
            KnightEvent::Hit { push, headshot } => {
                let hit = self.pending.get_or_insert_default();
                hit.push += Vec2::new(push.x, push.z).normalize_or_zero();
                hit.hits += 1;
                hit.headshot |= headshot;
            }
            KnightEvent::Damage { amount } => {
                self.pending.get_or_insert_default().damage += amount.max(0.0);
            }
            KnightEvent::ShieldBreak => self.pending.get_or_insert_default().shield_break = true,
            KnightEvent::Flinch { region, push } => {
                self.flinch.hit(region, Vec2::new(push.x, push.z));
                if matches!(region, HitRegion::Legs { .. }) {
                    self.squash.v -= FLINCH_DIP;
                }
            }
            KnightEvent::ShedArmor => self.armor_off = true,
        }
    }

    /// Makes the frame's hits into a take.
    fn take_hit(&mut self, hit: PendingHit) {
        let push = hit.push.normalize_or(Vec2::Y);
        let stacked =
            (TAKE_BODY + TAKE_PER_HIT * hit.hits.saturating_sub(1) as f32).min(TAKE_HITS_MAX);
        let heavy = (TAKE_BODY + (hit.damage - TAKE_HEAVY_FROM).max(0.0) / TAKE_HEAVY_SPAN)
            .min(TAKE_HEAVY_MAX);
        let mut strength = stacked.max(heavy);
        if hit.headshot {
            strength = strength.max(TAKE_HEADSHOT);
        }
        if hit.shield_break {
            strength = strength.max(TAKE_SHIELD_BREAK);
        }
        let big = strength >= TAKE_BIG;
        // Running, he flinches and keeps going.
        let run = smoothstep(0.4, 2.5, self.speed);
        let strength = strength * (1.0 + (TAKE_RUNNING - 1.0) * run);

        // Each take flings the other arm high: under fire he flails (easing
        // across mid-take; a fresh take starts on its side at once).
        self.take_side_goal = -self.take_side_goal;
        if self.take.x < 0.1 {
            self.take_side = self.take_side_goal;
        }
        self.take_goal = if self.take_hold > 0.0 {
            self.take_goal.max(strength)
        } else {
            strength
        };
        self.take_hold = if big { TAKE_HOLD_BIG } else { TAKE_HOLD };
        self.take_push = push;
        if self.air < 0.5 && self.hop <= 0.0 {
            self.hop_speed = HOP_SPEED * strength.sqrt();
        }
        let want = KNOCK_SPEED * strength;
        let along = self.knock.v.dot(push);
        if along < want {
            self.knock.v += push * (want - along);
        }
        let kick = if big { 1.35 } else { 1.0 };
        self.wobble.v += push * WOBBLE_KICK * kick;
        self.nod.v += if big { 4.0 } else { 2.0 };
        self.tip.v += Vec2::new(7.0, -5.0 * self.take_side_goal) * kick;
        self.wide_left = if big { WIDE_TIME_BIG } else { WIDE_TIME };
        self.blink_left = 0.0;
        if hit.headshot {
            self.dented = true;
            self.hat_speed = self.hat_speed.max(0.0) + HAT_POP;
            let coin = if self.rng.chance(0.5) { 1.0 } else { -1.0 };
            // Knocked away from the side the shot struck (the push carries
            // it; a dead-centre hit picks a side at random), and always spun
            // the same way for that side, so it turns side-on as it flies and
            // reads as a hat, not a brim.
            self.hat_side = if push.x.abs() > 0.08 {
                push.x.signum()
            } else {
                coin
            };
            self.hat_spin.v -= self.hat_side * 7.0;
        }
    }

    /// Advances `dt` seconds with this frame's `input` and returns the pose
    /// (no time at all while frozen: see [`KnightAnim::set_frozen`]).
    pub fn step(&mut self, dt: f32, input: &KnightInput) -> KnightPose {
        let dt = if self.frozen { 0.0 } else { dt.clamp(0.0, 0.1) };
        if input.downed && !self.downed {
            self.downed = true;
            self.ko_time = 0.0;
            if let Some(hit) = self.pending.take() {
                self.take_push = hit.push.normalize_or(Vec2::Y);
                // The killing headshot dents the helmet that flies off.
                self.dented |= hit.headshot;
            }
            // His armor comes apart (`fx::armor` flings it off).
            self.armor_off = true;
            self.squash.v -= LAND_SQUASH * 0.8;
            self.hat_height = 0.0;
            self.hat_speed = 0.0;
            // A last, biggest take while the X eyes show.
            self.take_goal = TAKE_SHIELD_BREAK;
            self.take_hold = KO_TIME;
        } else if !input.downed && self.downed {
            self.respawn();
        }
        if !self.frozen
            && let Some(hit) = self.pending.take()
        {
            self.take_hit(hit);
        }
        self.time += dt;
        if self.downed {
            self.ko_time += dt;
        }

        // Eyes.
        self.wide_left = (self.wide_left - dt).max(0.0);
        self.blink_left = (self.blink_left - dt).max(0.0);
        self.blink_in -= dt;
        if self.blink_in <= 0.0 {
            self.blink_in += self.rng.range(BLINK_EVERY.0, BLINK_EVERY.1);
            if self.wide_left <= 0.0 {
                self.blink_left = BLINK_TIME;
            }
        }

        // Motion, smoothed so the pose never snaps.
        let flat = Vec3::new(input.velocity.x, 0.0, input.velocity.z);
        let target_speed = if self.downed { 0.0 } else { flat.length() };
        self.speed += (target_speed - self.speed) * (1.0 - (-12.0 * dt).exp());
        if let Some(d) = flat.try_normalize().filter(|_| target_speed > 0.3) {
            self.dir = self.dir.lerp(d, 1.0 - (-14.0 * dt).exp()).normalize_or(d);
        }
        let airborne = !input.grounded && !self.downed;
        self.air += (f32::from(u8::from(airborne)) - self.air) * (1.0 - (-14.0 * dt).exp());
        self.phase = (self.phase + self.speed / STRIDE * TAU * dt).rem_euclid(TAU);

        // The take: snap in, hold, then spring back.
        self.take_side += (self.take_side_goal - self.take_side) * (1.0 - (-dt / 0.05).exp());
        if self.take_hold > 0.0 {
            self.take.x += (self.take_goal - self.take.x) * (1.0 - (-dt / TAKE_ATTACK).exp());
            self.take.v = 0.0;
            self.take_hold -= dt;
        }

        // What the hat tip chases: a lazy wobble standing, trailing back
        // (and bouncing with the stride) running.
        let run = smoothstep(0.4, 2.5, self.speed);
        let t = self.time;
        let tip_goal = Vec2::new(
            (1.0 - run) * 0.1 * (TAU * 0.9 * t).sin()
                + run * (0.32 + 0.12 * (2.0 * self.phase).cos()),
            (1.0 - run) * 0.14 * (TAU * 0.65 * t + 1.0).sin() - 0.1 * run,
        );

        // Springs.
        let steps = (dt / SUBSTEP).ceil().max(1.0);
        let h = dt / steps;
        let released = self.take_hold <= 0.0;
        for _ in 0..steps as usize {
            self.squash.step(0.0, SQUASH_K, SQUASH_C, h);
            self.wobble.step(Vec2::ZERO, WOBBLE_K, WOBBLE_C, h);
            self.nod.step(0.0, WOBBLE_K * 1.6, WOBBLE_C * 1.2, h);
            self.hat_spin.step(0.0, 90.0, 7.0, h);
            self.knock.step(Vec2::ZERO, KNOCK_K, KNOCK_C, h);
            self.tip.step(tip_goal, TIP_K, TIP_C, h);
            self.flinch.step(h);
            if released {
                self.take.step(0.0, TAKE_K, TAKE_C, h);
            }
            if let Some(pop) = &mut self.pop {
                pop.step(1.0, POP_K, POP_C, h);
            }
            self.hat_speed -= HAT_GRAVITY * h;
            self.hat_height += self.hat_speed * h;
            if self.hat_height < 0.0 {
                self.hat_height = 0.0;
                self.hat_speed = if self.hat_speed < -0.4 {
                    -self.hat_speed * HAT_BOUNCE
                } else {
                    0.0
                };
            }
            if self.hop > 0.0 || self.hop_speed > 0.0 {
                self.hop_speed -= HOP_GRAVITY * h;
                self.hop += self.hop_speed * h;
                if self.hop <= 0.0 {
                    self.hop = 0.0;
                    self.hop_speed = 0.0;
                }
            }
        }
        if released && self.take.x.abs() < 1e-3 && self.take.v.abs() < 1e-2 {
            self.take = Spring::default();
            self.take_goal = 0.0;
        }
        if self
            .pop
            .is_some_and(|p| (p.x - 1.0).abs() < 1e-3 && p.v.abs() < 1e-2)
        {
            self.pop = None;
        }
        self.pose()
    }

    fn respawn(&mut self) {
        self.downed = false;
        self.ko_time = 0.0;
        self.squash = Spring::default();
        self.wobble = Spring::default();
        self.nod = Spring::default();
        self.take = Spring::default();
        self.take_goal = 0.0;
        self.take_hold = 0.0;
        self.pending = None;
        self.hop = 0.0;
        self.hop_speed = 0.0;
        self.knock = Spring::default();
        self.tip = Spring::default();
        self.hat_spin = Spring::default();
        self.hat_height = 0.0;
        self.hat_speed = 0.0;
        self.speed = 0.0;
        self.air = 0.0;
        self.wide_left = 0.0;
        self.blink_left = 0.0;
        self.flinch = Flinch::default();
        self.dented = false;
        self.armor_off = false;
        self.pop = Some(Spring { x: 0.0, v: 0.0 });
    }

    fn pose(&self) -> KnightPose {
        let t = self.time;
        let run = smoothstep(0.4, 2.5, self.speed);
        let idle = 1.0 - run;
        let stride = run * (self.speed / RUN_SPEED).min(1.0);
        let air = self.air;
        let d = self.dir;
        let s = self.phase.sin();
        // 0 at each footfall (the boots pass each other), 1 mid-flight.
        let flight = s.abs();
        let idle_wave = (TAU * IDLE_HZ * t).sin();
        let rock = (PI * IDLE_HZ * t).sin();

        // The take.
        let take = self.take.x;
        let r = take.max(0.0);
        let push = Vec3::new(self.take_push.x, 0.0, self.take_push.y);
        let side = self.take_side;
        let flail = FLAIL * r.min(1.0) * (TAU * FLAIL_HZ * t).sin();

        // Boots: swing in the move's plane, left and right in opposite phase;
        // a boot swinging in towards the other swings less, so they never
        // cross. In a take the high side's boot kicks up toward the shooter.
        let mut legs = [Quat::IDENTITY; 2];
        for (i, lat) in [(0, -1.0f32), (1, 1.0)] {
            let sign = if i == 0 { 1.0 } else { -1.0 };
            let mut angle = sign * LEG_SWING * stride * s;
            // Positive angles move the boot along -d.
            if -d.x * angle * lat < 0.0 {
                angle *= INWARD_SWING;
            }
            let air_pose = Quat::from_rotation_x(if i == 0 { 0.32 } else { -0.22 });
            let base = swing(d, angle).slerp(air_pose, air);
            let high = 0.5 + 0.5 * side * lat;
            let kick = (KICK * r * high).min(KICK_MAX) - 0.12 * r * (1.0 - high);
            let out = (0.25 * high + 0.08) * r;
            legs[i] = swing(push, kick) * swing(Vec3::X * -lat, out) * base;
        }
        // Gauntlets pump against the boot on their side, held a little out
        // while running; flail outwards in the air; fling up in a take.
        let mut arms = [Quat::IDENTITY; 2];
        for (i, lat) in [(0, -1.0f32), (1, 1.0)] {
            let sign = if i == 0 { -1.0 } else { 1.0 };
            let out = Vec3::X * -lat;
            let pump = swing(d, sign * ARM_SWING * stride * s);
            let spread = swing(out, 0.16 * run + 0.05 * idle * (0.5 + 0.5 * idle_wave));
            let air_pose = Quat::from_rotation_z(lat * 0.62) * Quat::from_rotation_x(0.25);
            let base = (pump * spread).slerp(air_pose, air);
            let high = 0.5 + 0.5 * side * lat;
            let fling = (r * (FLING_LOW + (FLING_HIGH - FLING_LOW) * high)).min(FLING_MAX)
                + sign * flail
                + 0.4 * take.min(0.0);
            arms[i] = swing(push, REACH * r) * swing(out, fling) * base;
        }

        // Torso: bob and rock when idle, rise in each flight and lean into the
        // run, lean back and turn in a take.
        let bob = IDLE_BOB * idle * idle_wave + RUN_BOB * run * (2.0 * flight - 1.0);
        let torso_offset = Vec3::Y * bob;
        let torso = swing(push, TAKE_LEAN * r)
            * Quat::from_rotation_y(0.18 * r * side)
            * swing(d, RUN_LEAN * stride)
            * Quat::from_rotation_y(0.07 * run * s)
            * Quat::from_rotation_z(IDLE_ROCK * idle * rock);
        let head = swing(push, TAKE_HEAD * r)
            * Quat::from_rotation_z(-0.07 * idle * rock)
            * Quat::from_rotation_x(
                0.05 * run * (2.0 * self.phase + 0.6).sin()
                    + 0.03 * idle * (TAU * IDLE_HZ * t - 0.8).sin()
                    + self.nod.x,
            );

        // Cape: trails behind the move (never swinging forward into the legs),
        // flutters with the stride, lifts in the air and flies up in a take.
        let back = (-d.z).max(-0.15);
        let sideways = -d.x;
        let trail = CAPE_TRAIL * stride;
        let flutter = 0.08 * run * (2.0 * self.phase).sin() + 0.035 * idle * (PI * t).sin();
        let cape = Quat::from_rotation_x(-(trail * back + flutter + 0.25 * air + 0.45 * r))
            * Quat::from_rotation_z(0.6 * trail * sideways);
        // Robe: its hem trails the run and sways with the hips, swings gently
        // standing, and lags toward the shooter as a take knocks him back.
        let robe = swing(push, ROBE_TAKE * r)
            * swing(d, ROBE_TRAIL * stride)
            * swing(d.cross(Vec3::Y), ROBE_SWAY * run * s)
            * swing(Vec3::X, 0.045 * idle * rock);

        // Hat: wobbles with the bob; a headshot knocks it up, off to one side
        // and cocked over, and it drops straight back onto his helmet.
        let lift = self.hat_height;
        let hat_side = self.hat_side;
        let hat_offset = Vec3::new(hat_side * HAT_DRIFT * lift, lift, 0.0);
        let cock = hat_side * (HAT_TILT * lift).min(HAT_TILT_MAX);
        let hat = Quat::from_rotation_z(0.06 * idle * idle_wave + self.hat_spin.x * 0.12 - cock)
            * Quat::from_rotation_y(self.hat_spin.x)
            * Quat::from_rotation_x(-(HAT_TIP * lift).min(HAT_TIP_MAX));
        let tip = self.tip.x.clamp(Vec2::splat(-0.8), Vec2::splat(0.8));
        let hat_tip = Quat::from_rotation_x(tip.x) * Quat::from_rotation_z(tip.y);

        // Whole body: squash and stretch (and a squash at each footfall), the
        // hit wobble and the take's lean, the respawn pop, the knockback.
        let sq = self.squash.x.clamp(-0.4, 0.4);
        let breathe = 0.01 * idle * (TAU * IDLE_HZ * t + 0.5).sin();
        let footfall = RUN_SQUASH * run * (2.0 * flight - 1.0);
        let pop = self.pop.map_or(1.0, |p| p.x.max(0.0));
        let girth = 1.0 - 0.5 * sq - 0.5 * footfall;
        let scale = Vec3::new(girth, 1.0 + sq + breathe + footfall, girth) * pop;
        let lean = self.wobble.x;
        let wobble = if lean.length() > 1e-5 {
            swing(Vec3::new(lean.x, 0.0, lean.y), lean.length().min(0.6))
        } else {
            Quat::IDENTITY
        };
        let tilt = swing(push, TAKE_TILT * r) * wobble;
        let offset = Vec3::new(self.knock.x.x, self.hop, self.knock.x.y);

        let eyes = if self.downed {
            EyeState::X
        } else if self.wide_left > 0.0 {
            EyeState::Wide
        } else if self.blink_left > 0.0 {
            EyeState::Blink
        } else {
            EyeState::Open
        };
        // The directional flinch, on top: the parts nearest the hit knocked
        // back along the shot (a hand or boot hanging below its pivot swings
        // its far end along the push; the torso and head lean their tops).
        let f = &self.flinch;
        let fp = Vec3::new(f.push.x, 0.0, f.push.y);
        let torso = swing(fp, f.lean.x) * Quat::from_rotation_y(f.twist.x) * torso;
        let head = swing(fp, f.head.x) * head;
        for i in 0..2 {
            arms[i] = swing(fp, -f.arms[i].x) * arms[i];
            legs[i] = swing(fp, -f.legs[i].x) * legs[i];
        }

        KnightPose {
            scale,
            tilt,
            offset,
            torso_offset,
            torso,
            legs,
            arms,
            head,
            cape,
            robe,
            hat_offset,
            hat,
            hat_tip,
            hat_scale: 1.0 + HAT_SWELL * lift,
            hat_visible: !self.downed && !self.armor_off,
            eyes,
            visible: !(self.downed && self.ko_time >= KO_TIME),
            take,
            dented: self.dented,
            armor: !self.armor_off,
        }
    }
}

// ---------------------------------------------------------------------------
// ECS: rig the spawned model, apply poses
// ---------------------------------------------------------------------------

/// The pivots and parts the pose drives, in [`KnightRig`]'s order. `Hat` stays
/// last: effects find the hat on a rig as its last joint.
pub const JOINTS: [&str; 11] = [
    "Torso",
    "PivotLegL",
    "PivotLegR",
    "PivotArmL",
    "PivotArmR",
    "PivotHead",
    "PivotCape",
    "PivotRobe",
    "PivotHatTip",
    "PivotHat",
    "Hat",
];

/// On a knight figure: the model root and the entities the pose drives, with
/// their rest transforms. Added by [`rig_knights`] once the model is dressed.
#[derive(Component, Debug, Clone)]
pub struct KnightRig {
    pub model: Entity,
    /// [`JOINTS`] and their rest transforms.
    pub joints: [(Entity, Transform); JOINTS.len()],
    /// Eye nodes by [`EyeState::ALL`] order, `[left, right]`.
    pub eyes: [[Entity; 2]; 4],
}

/// On a knight figure: its model root (a child), before and after rigging.
#[derive(Component, Debug, Clone, Copy)]
pub struct KnightModel(pub Entity);

/// The knight's shared toon material (vertex colours, a strong warm rim).
#[derive(Resource, Debug, Clone)]
pub struct KnightAssets {
    pub material: Handle<ToonMaterial>,
}

pub fn create_knight_assets(mut commands: Commands, mut materials: ResMut<Assets<ToonMaterial>>) {
    commands.insert_resource(KnightAssets {
        material: materials.add(ToonMaterial::vertex_colored().with_rim(KNIGHT_RIM)),
    });
}

/// glTF nodes sit under the model's forward fix: a model-space rotation or
/// offset becomes `F⁻¹ q F` / `F⁻¹ v` for them.
fn to_node_rotation(q: Quat) -> Quat {
    MODEL_FORWARD_FIX.inverse() * q * MODEL_FORWARD_FIX
}

fn to_node_offset(v: Vec3) -> Vec3 {
    MODEL_FORWARD_FIX.inverse() * v
}

/// Finds a dressed knight's pivots and eyes, gives every mesh the knight's
/// material, shows only the open eyes, and warms the knight's draws up.
#[allow(clippy::too_many_arguments)]
pub fn rig_knights(
    mut dressed: MessageReader<ModelDressed>,
    parts: ModelParts,
    parents: Query<&ChildOf>,
    figures: Query<(), (With<KnightModel>, Without<KnightRig>)>,
    transforms: Query<&Transform>,
    children: Query<&Children>,
    toon_meshes: Query<&Mesh3d, With<MeshMaterial3d<ToonMaterial>>>,
    assets: Option<Res<KnightAssets>>,
    mut warmup: Warmup,
    mut warmed: Local<bool>,
    mut commands: Commands,
) {
    for event in dressed.read() {
        if event.name != KNIGHT_MODEL {
            continue;
        }
        let Ok(figure) = parents.get(event.root).map(ChildOf::parent) else {
            continue;
        };
        if !figures.contains(figure) {
            continue;
        }
        let find = |name: &str| parts.find(event.root, name);
        let joints: Option<Vec<(Entity, Transform)>> = JOINTS
            .iter()
            .map(|n| find(n).map(|e| (e, transforms.get(e).copied().unwrap_or_default())))
            .collect();
        let eyes: Option<Vec<[Entity; 2]>> = EyeState::ALL
            .iter()
            .map(|s| {
                Some([
                    find(&format!("{}L", s.prefix()))?,
                    find(&format!("{}R", s.prefix()))?,
                ])
            })
            .collect();
        let (Some(joints), Some(eyes)) = (joints, eyes) else {
            error!("knight: the model is missing named parts; not animating it");
            continue;
        };
        for (state, pair) in EyeState::ALL.iter().zip(&eyes) {
            for &eye in pair {
                commands.entity(eye).insert(if *state == EyeState::Open {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                });
            }
        }
        if let Some(assets) = &assets {
            let mut sample = None;
            for entity in children.iter_descendants(event.root) {
                if let Ok(mesh) = toon_meshes.get(entity) {
                    commands
                        .entity(entity)
                        .insert(MeshMaterial3d(assets.material.clone()));
                    sample.get_or_insert(mesh.0.clone());
                }
            }
            // The knight's material and the glTF vertex layout, with and
            // without its ink hull, compiled behind the loading screen.
            if let Some(mesh) = sample.filter(|_| !*warmed) {
                warmup.add_with(mesh, assets.material.clone(), Outline::default());
                *warmed = true;
            }
        }
        commands.entity(figure).insert(KnightRig {
            model: event.root,
            joints: joints.try_into().expect("JOINTS"),
            eyes: eyes.try_into().expect("four eye states"),
        });
    }
}

/// Writes a pose onto a rigged knight: the model root, its pivots and eyes.
pub fn write_pose<F1: QueryFilter, F2: QueryFilter>(
    pose: &KnightPose,
    rig: &KnightRig,
    transforms: &mut Query<&mut Transform, F1>,
    visibility: &mut Query<&mut Visibility, F2>,
) {
    if let Ok(mut t) = transforms.get_mut(rig.model) {
        // The model root is in model space already (the forward fix is below it).
        t.translation = pose.offset;
        t.scale = pose.scale;
        t.rotation = pose.tilt;
    }
    let [
        torso,
        leg_l,
        leg_r,
        arm_l,
        arm_r,
        head,
        cape,
        robe,
        hat_tip,
        hat_pivot,
        hat,
    ] = rig.joints;
    let mut set = |(entity, rest): (Entity, Transform), offset: Vec3, rotation: Quat| {
        if let Ok(mut t) = transforms.get_mut(entity) {
            t.translation = rest.translation + to_node_offset(offset);
            t.rotation = to_node_rotation(rotation) * rest.rotation;
        }
    };
    set(torso, pose.torso_offset, pose.torso);
    set(leg_l, Vec3::ZERO, pose.legs[0]);
    set(leg_r, Vec3::ZERO, pose.legs[1]);
    set(arm_l, Vec3::ZERO, pose.arms[0]);
    set(arm_r, Vec3::ZERO, pose.arms[1]);
    set(head, Vec3::ZERO, pose.head);
    set(cape, Vec3::ZERO, pose.cape);
    set(robe, Vec3::ZERO, pose.robe);
    set(hat_tip, Vec3::ZERO, pose.hat_tip);
    set(hat_pivot, pose.hat_offset, pose.hat);
    if let Ok(mut t) = transforms.get_mut(hat_pivot.0) {
        t.scale = hat_pivot.1.scale * pose.hat_scale;
    }
    let show = |v: bool| {
        if v {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        }
    };
    if let Ok(mut v) = visibility.get_mut(hat.0) {
        v.set_if_neq(show(pose.hat_visible));
    }
    for (state, pair) in EyeState::ALL.iter().zip(&rig.eyes) {
        for &eye in pair {
            if let Ok(mut v) = visibility.get_mut(eye) {
                v.set_if_neq(show(*state == pose.eyes));
            }
        }
    }
}

/// The armor that comes off a knight when he goes down (M4, D105): its parts'
/// names in the model, in [`KnightArmorRig`]'s order. The helmet comes first.
pub const ARMOR_PARTS: [&str; 5] = ["Helmet", "GauntletL", "GauntletR", "BootL", "BootR"];

/// On a rigged knight figure: his armor parts (added by `fx::armor` once the
/// knight is rigged).
#[derive(Component, Debug, Clone)]
pub struct KnightArmorRig {
    /// Each [`ARMOR_PARTS`] node (where the part is).
    pub nodes: [Entity; ARMOR_PARTS.len()],
    /// What to hide when the part comes off: the helmet's own mesh (so his
    /// eyes stay), the limbs' nodes.
    pub shown: [Entity; ARMOR_PARTS.len()],
    /// The dented helmet, a hidden sibling of the helmet's mesh until a
    /// headshot (when the dented model exists).
    pub dent: Option<Entity>,
}

/// Shows a knight's armor as `pose` says: on or off, the helmet dented or not.
pub fn write_armor<F: QueryFilter>(
    pose: &KnightPose,
    rig: &KnightArmorRig,
    visibility: &mut Query<&mut Visibility, F>,
) {
    let show = |v: bool| {
        if v {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        }
    };
    let dented = pose.dented && rig.dent.is_some();
    for (i, &entity) in rig.shown.iter().enumerate() {
        let on = pose.armor && (i != 0 || !dented);
        if let Ok(mut v) = visibility.get_mut(entity) {
            v.set_if_neq(show(on));
        }
    }
    if let Some(dent) = rig.dent
        && let Ok(mut v) = visibility.get_mut(dent)
    {
        v.set_if_neq(show(pose.armor && dented));
    }
}

/// A respawn sparkle: a glow at the knight's chest that flares and fades over
/// [`SPARKLE_TIME`], then despawns.
#[derive(Component, Debug, Clone, Copy)]
pub struct RespawnSparkle {
    /// Seconds left.
    pub left: f32,
}

/// The sparkle's glow `t` seconds into its life: bright and small, then wide and gone.
pub fn sparkle_halo(t: f32) -> Halo {
    let k = (1.0 - t / SPARKLE_TIME).clamp(0.0, 1.0);
    Halo::new(
        Color::srgb(1.0, 0.9, 0.55),
        0.9 + 1.5 * (1.0 - k),
        1.6 * k * k,
    )
}

/// Freezes every knight's animation clock while the gallery's [`GalleryFreeze`]
/// is set, and thaws it after. Runs in `Update`, before the knights animate in
/// `PostUpdate`, so the freeze holds from the frame it is set. Registered by
/// `scenario::gallery::GalleryPlugin`.
pub fn freeze_knights(freeze: Option<Res<GalleryFreeze>>, mut knights: Query<&mut KnightAnim>) {
    let frozen = freeze.is_some();
    for mut anim in &mut knights {
        if anim.is_frozen() != frozen {
            anim.set_frozen(frozen);
        }
    }
}

pub fn fade_sparkles(
    time: FreezableTime,
    mut sparkles: Query<(Entity, &mut RespawnSparkle, &mut Halo)>,
    mut commands: Commands,
) {
    for (entity, mut sparkle, mut halo) in &mut sparkles {
        sparkle.left -= time.delta_secs();
        if sparkle.left <= 0.0 {
            commands.entity(entity).despawn();
        } else {
            *halo = sparkle_halo(SPARKLE_TIME - sparkle.left);
        }
    }
}
