//! The knight's procedural animation and eyes (docs/M2-SPEC.md → The knight).
//!
//! The knight model (`art/blender/assets/knight.py`) hangs its parts from joint
//! pivots. [`KnightAnim`] turns the character's motion and combat events into a
//! [`KnightPose`] every frame, and [`apply_knight_poses`] writes that pose onto
//! the pivots found by [`rig_knights`]:
//!
//! - an idle bob (torso, head and hat breathe and wobble);
//! - a run cycle from the velocity: boots swing, gauntlets pump, the torso bobs
//!   and leans into the move, the cape trails and flutters; sideways strafes
//!   side-step without crossing the boots;
//! - an air pose while airborne, a stretch on jump take-off and a squash on landing;
//! - a hit wobble spring (he rocks away from the hit) and a nod;
//! - a respawn pop-in: scale springs up from zero and overshoots, with a warm
//!   sparkle ([`RespawnSparkle`]) flaring at his chest;
//! - the hat bounces up off his helmet on headshots;
//! - eyes: open, blinking every 2–5 s at random, wide for 0.3 s after a hit, X
//!   when eliminated. Each state is a mesh; the pose picks which one shows.
//!
//! On elimination he shows his X eyes for [`KO_TIME`] and hides his hat (the
//! elimination effect drops a hat prop in its place), then the figure hides until
//! respawn. The core is pure and seeded, so it is tested headless
//! (`tests/knight.rs`); the systems only gather input and write transforms.
//! The animation runs on `Time` (virtual): pausing virtual time freezes the pose.
//! The gallery's [`GalleryFreeze`] also freezes it (clock, eyes, springs and the
//! respawn sparkle) without pausing the game: see [`freeze_knights`].
//!
//! Directions in [`KnightInput`] and [`KnightPose`] are in the knight's model
//! space: +Y up, -Z forward, +X to his right. [`apply_knight_poses`] converts to
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
pub const KNIGHT_RIM: f32 = 2.0;

// ---------------------------------------------------------------------------
// Tuning (the spec's feel numbers live here; they are presentation only)
// ---------------------------------------------------------------------------

/// Idle bob: torso rise (m) and rate (Hz).
pub const IDLE_BOB: f32 = 0.014;
pub const IDLE_HZ: f32 = 1.1;
/// Distance covered per full run cycle (m): 5.5 m/s runs at about 3.2 steps/s.
pub const STRIDE: f32 = 1.7;
/// Peak boot swing and gauntlet pump at full run (radians).
pub const LEG_SWING: f32 = 0.62;
pub const ARM_SWING: f32 = 0.7;
/// Boots swinging towards each other move this much less (no crossing).
pub const INWARD_SWING: f32 = 0.3;
/// Run bob (m), lean into the move (rad) and cape trail (rad) at full run.
pub const RUN_BOB: f32 = 0.035;
pub const RUN_LEAN: f32 = 0.14;
pub const CAPE_TRAIL: f32 = 0.5;
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
/// Respawn pop-in spring (scale from 0 to 1, overshooting).
pub const POP_K: f32 = 240.0;
pub const POP_C: f32 = 13.0;
/// Headshot hat bounce: launch speed (m/s), gravity (m/s²), restitution.
pub const HAT_POP: f32 = 2.3;
pub const HAT_GRAVITY: f32 = 24.0;
pub const HAT_BOUNCE: f32 = 0.35;
/// Eyes: blink every 2–5 s for this long; wide this long after a hit.
pub const BLINK_EVERY: (f32, f32) = (2.0, 5.0);
pub const BLINK_TIME: f32 = 0.13;
pub const WIDE_TIME: f32 = 0.3;
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
    /// direction matters).
    Hit {
        push: Vec3,
        headshot: bool,
    },
}

/// The pose: offsets from the model's rest pose, in model space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KnightPose {
    /// Whole-model scale about the feet (squash, stretch, respawn pop).
    pub scale: Vec3,
    /// Whole-model lean about the feet (the hit wobble).
    pub tilt: Quat,
    /// Torso (upper body) offset and rotation about the hips.
    pub torso_offset: Vec3,
    pub torso: Quat,
    /// Boots and gauntlets about the hips and shoulders: `[left, right]`.
    pub legs: [Quat; 2],
    pub arms: [Quat; 2],
    pub head: Quat,
    pub cape: Quat,
    pub hat_offset: Vec3,
    pub hat: Quat,
    pub hat_visible: bool,
    pub eyes: EyeState,
    /// False once an elimination's KO beat is over, until respawn.
    pub visible: bool,
}

impl KnightPose {
    pub const REST: Self = Self {
        scale: Vec3::ONE,
        tilt: Quat::IDENTITY,
        torso_offset: Vec3::ZERO,
        torso: Quat::IDENTITY,
        legs: [Quat::IDENTITY; 2],
        arms: [Quat::IDENTITY; 2],
        head: Quat::IDENTITY,
        cape: Quat::IDENTITY,
        hat_offset: Vec3::ZERO,
        hat: Quat::IDENTITY,
        hat_visible: true,
        eyes: EyeState::Open,
        visible: true,
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
    hat_height: f32,
    hat_speed: f32,
    hat_spin: Spring<f32>,
    /// Respawn pop-in scale, while it settles.
    pop: Option<Spring<f32>>,
    blink_in: f32,
    blink_left: f32,
    wide_left: f32,
    downed: bool,
    ko_time: f32,
    /// Held by the gallery ([`GalleryFreeze`]): steps advance no time.
    frozen: bool,
    rng: Rng,
}

impl KnightAnim {
    pub fn new(seed: u64) -> Self {
        let mut rng = Rng::new(seed ^ 0x4B_4E_49_47_48_54);
        let blink_in = rng.range(BLINK_EVERY.0, BLINK_EVERY.1);
        Self {
            time: 0.0,
            phase: 0.0,
            speed: 0.0,
            dir: Vec3::NEG_Z,
            air: 0.0,
            squash: Spring::default(),
            wobble: Spring::default(),
            nod: Spring::default(),
            hat_height: 0.0,
            hat_speed: 0.0,
            hat_spin: Spring::default(),
            pop: None,
            blink_in,
            blink_left: 0.0,
            wide_left: 0.0,
            downed: false,
            ko_time: 0.0,
            frozen: false,
            rng,
        }
    }

    /// Holds the animation clock: while frozen, [`KnightAnim::step`] advances no
    /// time, so the pose, eyes (blink, wide) and springs stay exactly as they
    /// are. Events and elimination still register and play out once thawed.
    /// Driven by [`freeze_knights`] from the gallery's [`GalleryFreeze`].
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

    /// Reacts to a one-off moment (applied on the next [`KnightAnim::step`]).
    pub fn event(&mut self, event: KnightEvent) {
        if self.downed {
            return;
        }
        match event {
            KnightEvent::Jump => self.squash.v += JUMP_STRETCH,
            KnightEvent::Land { speed } => {
                self.squash.v -= LAND_SQUASH * (speed.abs() / 9.0).clamp(0.35, 1.0);
            }
            KnightEvent::Hit { push, headshot } => {
                let push = Vec2::new(push.x, push.z).normalize_or_zero();
                let kick = if headshot { 1.35 } else { 1.0 };
                self.wobble.v += push * WOBBLE_KICK * kick;
                self.nod.v += if headshot { 9.0 } else { 4.0 };
                self.wide_left = WIDE_TIME;
                self.blink_left = 0.0;
                if headshot {
                    self.hat_speed = self.hat_speed.max(0.0) + HAT_POP;
                    let spin = if self.rng.chance(0.5) { 1.0 } else { -1.0 };
                    self.hat_spin.v += spin * 7.0;
                }
            }
        }
    }

    /// Advances `dt` seconds with this frame's `input` and returns the pose
    /// (no time at all while frozen: see [`KnightAnim::set_frozen`]).
    pub fn step(&mut self, dt: f32, input: &KnightInput) -> KnightPose {
        let dt = if self.frozen { 0.0 } else { dt.clamp(0.0, 0.1) };
        if input.downed && !self.downed {
            self.downed = true;
            self.ko_time = 0.0;
            self.squash.v -= LAND_SQUASH * 0.8;
            self.hat_height = 0.0;
            self.hat_speed = 0.0;
        } else if !input.downed && self.downed {
            self.respawn();
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

        // Springs.
        let steps = (dt / SUBSTEP).ceil().max(1.0);
        let h = dt / steps;
        for _ in 0..steps as usize {
            self.squash.step(0.0, SQUASH_K, SQUASH_C, h);
            self.wobble.step(Vec2::ZERO, WOBBLE_K, WOBBLE_C, h);
            self.nod.step(0.0, WOBBLE_K * 1.6, WOBBLE_C * 1.2, h);
            self.hat_spin.step(0.0, 90.0, 7.0, h);
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
        self.hat_spin = Spring::default();
        self.hat_height = 0.0;
        self.hat_speed = 0.0;
        self.speed = 0.0;
        self.air = 0.0;
        self.wide_left = 0.0;
        self.blink_left = 0.0;
        self.pop = Some(Spring { x: 0.0, v: 0.0 });
    }

    fn pose(&self) -> KnightPose {
        let t = self.time;
        let run = smoothstep(0.4, 2.5, self.speed);
        let idle = 1.0 - run;
        let strength = (self.speed / RUN_SPEED).min(1.3);
        let air = self.air;
        let d = self.dir;
        let s = self.phase.sin();
        let idle_wave = (TAU * IDLE_HZ * t).sin();

        // Boots: swing in the move's plane, left and right in opposite phase;
        // a boot swinging in towards the other swings less, so they never cross.
        let mut legs = [Quat::IDENTITY; 2];
        for (i, side) in [(0, -1.0f32), (1, 1.0)] {
            let sign = if i == 0 { 1.0 } else { -1.0 };
            let mut angle = sign * LEG_SWING * run * strength.min(1.0) * s;
            // Positive angles move the boot along -d.
            if -d.x * angle * side < 0.0 {
                angle *= INWARD_SWING;
            }
            let air_pose = Quat::from_rotation_x(if i == 0 { 0.32 } else { -0.22 });
            legs[i] = swing(d, angle).slerp(air_pose, air);
        }
        // Gauntlets pump against the boot on their side; flail outwards in the air.
        let mut arms = [Quat::IDENTITY; 2];
        for (i, side) in [(0, -1.0f32), (1, 1.0)] {
            let sign = if i == 0 { -1.0 } else { 1.0 };
            let pump = swing(d, sign * ARM_SWING * run * strength.min(1.0) * s);
            let sway = Quat::from_rotation_z(side * 0.035 * idle * idle_wave);
            let air_pose = Quat::from_rotation_z(side * 0.62) * Quat::from_rotation_x(0.25);
            arms[i] = (pump * sway).slerp(air_pose, air);
        }

        // Torso: breathe when idle, bounce twice a stride and lean into the run.
        let bob = IDLE_BOB * idle * idle_wave + RUN_BOB * run * (2.0 * self.phase).cos();
        let torso_offset = Vec3::Y * bob;
        let torso =
            swing(d, RUN_LEAN * run * strength.min(1.0)) * Quat::from_rotation_y(0.07 * run * s);
        let head = Quat::from_rotation_x(
            0.05 * run * (2.0 * self.phase + 0.6).sin()
                + 0.03 * idle * (TAU * IDLE_HZ * t - 0.8).sin()
                + self.nod.x,
        );

        // Cape: trails behind the move (never swinging forward into the legs),
        // flutters with the stride and lifts in the air.
        let back = (-d.z).max(-0.15);
        let sideways = -d.x;
        let trail = CAPE_TRAIL * strength.min(1.0) * run;
        let flutter = 0.07 * run * (2.0 * self.phase).sin() + 0.035 * idle * (PI * t).sin();
        let cape = Quat::from_rotation_x(-(trail * back + flutter + 0.25 * air))
            * Quat::from_rotation_z(0.6 * trail * sideways);

        // Hat: wobbles with the bob, bounces on headshots.
        let hat_offset = Vec3::Y * self.hat_height;
        let hat = Quat::from_rotation_z(0.06 * idle * idle_wave + self.hat_spin.x * 0.12)
            * Quat::from_rotation_y(self.hat_spin.x)
            * Quat::from_rotation_x(-0.6 * self.hat_height.min(0.15));

        // Whole body: squash and stretch, the hit wobble, the respawn pop.
        let sq = self.squash.x.clamp(-0.4, 0.4);
        let breathe = 0.008 * idle * (TAU * IDLE_HZ * t + 0.5).sin();
        let pop = self.pop.map_or(1.0, |p| p.x.max(0.0));
        let scale = Vec3::new(1.0 - 0.5 * sq, 1.0 + sq + breathe, 1.0 - 0.5 * sq) * pop;
        let lean = self.wobble.x;
        let tilt = if lean.length() > 1e-5 {
            swing(Vec3::new(lean.x, 0.0, lean.y), lean.length().min(0.6))
        } else {
            Quat::IDENTITY
        };

        let eyes = if self.downed {
            EyeState::X
        } else if self.wide_left > 0.0 {
            EyeState::Wide
        } else if self.blink_left > 0.0 {
            EyeState::Blink
        } else {
            EyeState::Open
        };
        KnightPose {
            scale,
            tilt,
            torso_offset,
            torso,
            legs,
            arms,
            head,
            cape,
            hat_offset,
            hat,
            hat_visible: !self.downed,
            eyes,
            visible: !(self.downed && self.ko_time >= KO_TIME),
        }
    }
}

// ---------------------------------------------------------------------------
// ECS: rig the spawned model, apply poses
// ---------------------------------------------------------------------------

/// The pivots and parts the pose drives, in [`KnightRig`]'s order.
pub const JOINTS: [&str; 9] = [
    "Torso",
    "PivotLegL",
    "PivotLegR",
    "PivotArmL",
    "PivotArmR",
    "PivotHead",
    "PivotCape",
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
    set(hat_pivot, pose.hat_offset, pose.hat);
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
