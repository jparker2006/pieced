//! Slice A — kinematic first-person movement in the fixed step (see docs/SPEC.md).
//!
//! Every [`Character`] (player, dummy, later bots) is moved here from its own
//! [`PlayerIntent`], once per fixed 60 Hz tick, inside [`SimSet::Movement`]:
//!
//! 1. depenetrate (a piece placed into a character pushes it out);
//! 2. slide / crouch / jump state from intent edges (jump buffer, coyote time);
//! 3. ground acceleration and friction, or reduced air control;
//! 4. avian's move-and-slide with a feet-anchored capsule, plus a step-up pass;
//! 5. ground snapping (so ramps are walked, not bounced down), arena bounds;
//! 6. eye height and hitbox offsets for crouching, and movement cues.
//!
//! `Transform.translation` is the character's feet. The collision capsule's
//! center sits half its height above the feet and only collides with
//! [`Layer::World`] and [`Layer::Piece`], so characters never collide with
//! hitboxes (theirs or anyone else's).

use crate::{
    arena::ArenaLayout,
    player::{BODY_BOTTOM, BODY_TOP, HEAD_CENTER},
    shared::{Character, EyeHeight, GameCue, Hitbox, Layer, LookAngles, PlayerIntent, SimSet},
    tuning::Tuning,
};
use avian3d::{character_controller::move_and_slide::DepenetrationConfig, prelude::*};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MovementTuning {
    pub run_speed: f32,
    pub sprint_speed: f32,
    pub crouch_speed: f32,
    /// Speed at slide entry; decays back toward crouch speed over `slide_duration`.
    pub slide_speed: f32,
    pub slide_duration: f32,
    pub slide_cooldown: f32,
    /// Seconds from rest to full speed on the ground.
    pub accel_time: f32,
    /// Seconds from full speed to rest on the ground.
    pub stop_time: f32,
    pub jump_height: f32,
    pub gravity: f32,
    /// Fraction of ground acceleration available in the air.
    pub air_control: f32,
    pub jump_buffer: f32,
    pub coyote_time: f32,
    pub height: f32,
    pub crouch_height: f32,
    pub radius: f32,
    pub eye_height: f32,
    pub crouch_eye_height: f32,
    pub max_slope_deg: f32,
    pub step_height: f32,
    /// Seconds for the eye to move between standing and crouched height.
    pub crouch_time: f32,
    /// Minimum horizontal speed (m/s) for a crouch press while sprinting to start a slide.
    pub slide_min_speed: f32,
    /// How fast (degrees per second) the move keys can steer a slide.
    pub slide_steer_deg: f32,
    /// Ground distance (m) walked per footstep cue.
    pub footstep_distance: f32,
}

impl Default for MovementTuning {
    fn default() -> Self {
        Self {
            run_speed: 5.5,
            sprint_speed: 7.5,
            crouch_speed: 2.8,
            slide_speed: 9.0,
            slide_duration: 0.8,
            slide_cooldown: 0.5,
            accel_time: 0.1,
            stop_time: 0.06,
            jump_height: 1.2,
            gravity: 20.0,
            air_control: 0.4,
            jump_buffer: 0.1,
            coyote_time: 0.1,
            height: 1.8,
            crouch_height: 1.2,
            radius: 0.35,
            eye_height: 1.62,
            crouch_eye_height: 1.05,
            max_slope_deg: 46.0,
            step_height: 0.3,
            crouch_time: 0.1,
            slide_min_speed: 5.0,
            slide_steer_deg: 60.0,
            footstep_distance: 2.2,
        }
    }
}

impl MovementTuning {
    /// Launch speed that reaches `jump_height` under `gravity`.
    pub fn jump_speed(&self) -> f32 {
        (2.0 * self.gravity.max(0.0) * self.jump_height.max(0.0)).sqrt()
    }

    /// Collision capsule height for the given crouch state.
    pub fn capsule_height(&self, crouched: bool) -> f32 {
        let min = 2.0 * self.radius.max(0.05) + 0.01;
        if crouched {
            self.crouch_height.max(min)
        } else {
            self.height.max(min)
        }
    }
}

/// Per-character movement state, attached to every [`Character`] when it spawns.
/// Other slices may read it (sway, FOV kick, audio, bots); only movement writes it.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct Motor {
    /// Velocity after the latest tick (m/s). On the ground only the horizontal part carries over.
    pub velocity: Vec3,
    /// Standing on walkable ground at the end of the latest tick.
    pub grounded: bool,
    /// Normal of the ground under the character (up when airborne).
    pub ground_normal: Vec3,
    /// The collision capsule is the crouched one (also true while sliding).
    pub crouched: bool,
    pub sliding: bool,
    /// Sprint is requested and allowed (held, moving forward, not crouched).
    pub sprinting: bool,
    /// Seconds since the current slide started.
    pub slide_time: f32,
    /// Seconds until another slide may start.
    pub slide_cooldown: f32,
    /// Seconds since the character last stood on the ground.
    pub air_time: f32,
    slide_dir: Vec3,
    /// Seconds since an unconsumed jump press, if one is buffered.
    jump_buffer_age: Option<f32>,
    /// Jumped since last grounded (no coyote jump after a real jump).
    jumped: bool,
    /// Ground distance walked since the last footstep.
    stride: f32,
}

impl Default for Motor {
    fn default() -> Self {
        Self {
            velocity: Vec3::ZERO,
            grounded: false,
            ground_normal: Vec3::Y,
            crouched: false,
            sliding: false,
            sprinting: false,
            slide_time: 0.0,
            slide_cooldown: 0.0,
            air_time: 0.0,
            slide_dir: Vec3::NEG_Z,
            jump_buffer_age: None,
            jumped: false,
            stride: 0.0,
        }
    }
}

pub struct MovementPlugin;

impl Plugin for MovementPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(attach_motor).add_systems(
            FixedUpdate,
            (move_characters, place_hitboxes)
                .chain()
                .in_set(SimSet::Movement),
        );
    }
}

fn attach_motor(add: On<Add, Character>, mut commands: Commands) {
    commands.entity(add.entity).insert(Motor::default());
}

/// Small slack for comparing accumulated tick times against tuning windows.
const TIME_EPS: f32 = 1e-4;
/// How far below the feet an airborne character looks for ground to land on.
const LANDING_PROBE: f32 = 0.06;
/// Upward speed above which an airborne character is rising and can't land.
const RISING_SPEED: f32 = 0.1;
/// Contacts deeper than this are ignored by depenetration (avian's default is
/// 0.5 m, too shallow for a ramp placed over a character).
const MAX_DEPENETRATION: f32 = 1.0;
/// How far past a ledge's edge, and from how high above it, the ground check
/// probes for the ledge's top face.
const EDGE_PROBE_INSET: f32 = 0.03;
const EDGE_PROBE_HEIGHT: f32 = 0.05;
/// An edge supports the capsule only if it touches at least this fraction of the
/// radius below the bottom sphere's center (contact within ~60° of straight down).
const EDGE_SUPPORT: f32 = 0.5;
/// Feet below this height have fallen out of the world and are put back.
const KILL_HEIGHT: f32 = -5.0;
/// Landings slower than this (settling at spawn, tiny drops) make no cue.
const MIN_LAND_CUE_SPEED: f32 = 1.0;

/// Standing and crouched capsules, rebuilt only when their tuning changes.
#[derive(Default)]
struct Capsules {
    key: (f32, f32, f32),
    standing: Option<Collider>,
    crouched: Option<Collider>,
}

impl Capsules {
    fn refresh(&mut self, t: &MovementTuning) {
        let key = (t.radius, t.capsule_height(false), t.capsule_height(true));
        if self.standing.is_none() || self.key != key {
            let radius = t.radius.max(0.05);
            self.standing = Some(Collider::capsule(radius, key.1 - 2.0 * radius));
            self.crouched = Some(Collider::capsule(radius, key.2 - 2.0 * radius));
            self.key = key;
        }
    }

    fn get(&self, crouched: bool) -> &Collider {
        let shape = if crouched {
            &self.crouched
        } else {
            &self.standing
        };
        shape.as_ref().expect("capsules refreshed before use")
    }
}

/// Walkable ground found below a character.
struct Ground {
    /// How far the character can drop onto it (keeping skin width).
    drop: f32,
    /// The walkable surface's normal.
    normal: Vec3,
    /// Height of the contact point.
    contact_y: f32,
}

/// Collision queries for one tick, shared by every character.
struct Mover<'a, 'w, 's> {
    mas: &'a MoveAndSlide<'w, 's>,
    filter: SpatialQueryFilter,
    config: MoveAndSlideConfig,
    depenetration: DepenetrationConfig,
    skin: f32,
    radius: f32,
    walkable_cos: f32,
}

impl Mover<'_, '_, '_> {
    fn walkable(&self, normal: Vec3) -> bool {
        normal.y >= self.walkable_cos
    }

    fn depenetrate(&self, shape: &Collider, center: Vec3) -> Vec3 {
        self.mas.depenetrate(
            shape,
            center,
            Quat::IDENTITY,
            &self.depenetration,
            &self.filter,
        )
    }

    /// Distance the shape can safely move along `movement` (keeping skin width)
    /// and the normal of what stops it, or `None` if the path is clear.
    fn cast(&self, shape: &Collider, center: Vec3, movement: Vec3) -> Option<(f32, Vec3)> {
        self.mas
            .cast_move(
                shape,
                center,
                Quat::IDENTITY,
                movement,
                self.skin,
                &self.filter,
            )
            .map(|hit| (hit.distance, hit.normal1))
    }

    /// Casts the shape down by up to `distance` looking for walkable ground.
    /// The rounded bottom of the capsule touches ledges and steps on their edge,
    /// where the contact normal is tilted; that counts as standing on the
    /// walkable top face.
    fn ground(&self, shape: &Collider, center: Vec3, half: f32, distance: f32) -> Option<Ground> {
        let hit = self.mas.cast_move(
            shape,
            center,
            Quat::IDENTITY,
            Vec3::NEG_Y * distance,
            self.skin,
            &self.filter,
        )?;
        let ground = |normal| Ground {
            drop: hit.distance,
            normal,
            contact_y: hit.point1.y,
        };
        if self.walkable(hit.normal1) {
            return Some(ground(hit.normal1));
        }
        // Only an edge well under the bottom sphere holds the character up.
        let sphere_center = center.y - hit.distance - (half - self.radius);
        if sphere_center - hit.point1.y < EDGE_SUPPORT * self.radius {
            return None;
        }
        let outward = flat(hit.point1 - center).normalize_or_zero();
        if outward == Vec3::ZERO {
            return None;
        }
        let origin = hit.point1 + outward * EDGE_PROBE_INSET + Vec3::Y * EDGE_PROBE_HEIGHT;
        let face = self.mas.spatial_query.cast_ray_predicate(
            origin,
            Dir3::NEG_Y,
            2.0 * EDGE_PROBE_HEIGHT,
            true,
            &self.filter,
            // Solid colliders only, like move-and-slide itself (no sensors).
            &|entity| self.mas.colliders.contains(entity),
        )?;
        (face.distance > 0.0 && self.walkable(face.normal)).then(|| ground(face.normal))
    }

    /// Move and slide. Returns the new center, the slid velocity, and whether a
    /// steep wall-like surface (a step candidate) was hit.
    fn slide(
        &self,
        shape: &Collider,
        center: Vec3,
        velocity: Vec3,
        dt: std::time::Duration,
        ground: Option<Vec3>,
    ) -> (Vec3, Vec3, bool) {
        let mut config = self.config.clone();
        if let Some(normal) = ground.and_then(|n| Dir3::new(n).ok()) {
            config.planes.push(normal);
        }
        let mut hit_steep = false;
        let walkable_cos = self.walkable_cos;
        let on_ground = ground.is_some();
        let out = self.mas.move_and_slide(
            shape,
            center,
            Quat::IDENTITY,
            velocity,
            dt,
            &config,
            &self.filter,
            |hit| {
                let n = hit.normal.as_vec3();
                let v = *hit.velocity;
                if on_ground && n.y >= walkable_cos && v.dot(n) < 0.0 {
                    // Running onto a ramp (or off one onto flat ground): follow
                    // the new slope at full horizontal speed instead of losing
                    // the part of the velocity that pointed into it.
                    let h = flat(v);
                    *hit.velocity = h - Vec3::Y * (h.x * n.x + h.z * n.z) / n.y;
                } else if n.y < walkable_cos && n.y > -0.3 {
                    hit_steep = true;
                    // On the ground, anything too steep to walk is a wall: the
                    // rounded capsule must not ride up ledges (stepping does that).
                    if on_ground
                        && n.y > 0.0
                        && let Ok(wall) = Dir3::new(flat(n))
                    {
                        *hit.normal = wall;
                    }
                }
                MoveAndSlideHitResponse::Accept
            },
        );
        (out.position, out.projected_velocity, hit_steep)
    }

    /// Classic step-up: lift by up to `step_height`, move horizontally, drop back
    /// down onto walkable ground. Returns the stepped center and velocity if it
    /// gets further than `plain_center` did.
    fn try_step(
        &self,
        shape: &Collider,
        center: Vec3,
        feet_y: f32,
        horizontal: Vec3,
        dt: f32,
        step_height: f32,
        plain_center: Vec3,
    ) -> Option<(Vec3, Vec3)> {
        let lift = self
            .cast(shape, center, Vec3::Y * step_height)
            .map_or(step_height, |(d, _)| d);
        if lift < 0.05 {
            return None;
        }
        // Sweep straight across (no sliding, so the raised capsule can't ride up
        // an edge taller than the step), then settle onto what is below.
        let raised = center + Vec3::Y * lift;
        let motion = horizontal * dt;
        let travel = self
            .cast(shape, raised, motion)
            .map_or(motion.length(), |(d, _)| d);
        let moved = raised + motion.normalize_or_zero() * travel;
        let landing = self.ground(shape, moved, center.y - feet_y, lift + 0.05)?;
        if landing.contact_y > feet_y + step_height + self.skin {
            return None;
        }
        let stepped = moved - Vec3::Y * landing.drop;
        let plain = flat(plain_center - center).length();
        let gained = flat(stepped - center).length();
        (gained > plain + 0.01).then_some((stepped, horizontal))
    }
}

/// Ground acceleration: brake sideways and opposing velocity at the stop rate,
/// accelerate along the wish direction at the acceleration rate.
fn ground_accel(h: Vec3, wish_dir: Vec3, target: f32, accel: f32, decel: f32, dt: f32) -> Vec3 {
    if wish_dir == Vec3::ZERO || target <= 0.0 {
        return h.move_towards(Vec3::ZERO, decel * dt);
    }
    let along = h.dot(wish_dir);
    let side = (h - wish_dir * along).move_towards(Vec3::ZERO, decel * dt);
    let mut along = along;
    let mut remaining = dt;
    if along < 0.0 {
        let to_stop = -along / decel;
        if to_stop >= remaining {
            along += decel * remaining;
            remaining = 0.0;
        } else {
            along = 0.0;
            remaining -= to_stop;
        }
    }
    if along < target {
        along = (along + accel * remaining).min(target);
    } else if along > target {
        along = (along - decel * remaining).max(target);
    }
    side + wish_dir * along
}

/// Air control: accelerate toward the wish direction without friction, never
/// gaining speed beyond the larger of the current speed and the target.
fn air_accel(h: Vec3, wish_dir: Vec3, target: f32, accel: f32, dt: f32) -> Vec3 {
    if wish_dir == Vec3::ZERO {
        return h;
    }
    let along = h.dot(wish_dir);
    let add = (target - along).clamp(0.0, accel * dt);
    (h + wish_dir * add).clamp_length_max(h.length().max(target))
}

/// Turns a horizontal direction toward another by at most `max_angle` radians.
fn steer(dir: Vec3, toward: Vec3, max_angle: f32) -> Vec3 {
    use std::f32::consts::{PI, TAU};
    let from = dir.z.atan2(dir.x);
    let to = toward.z.atan2(toward.x);
    let delta = (to - from + PI).rem_euclid(TAU) - PI;
    let angle = from + delta.clamp(-max_angle, max_angle);
    Vec3::new(angle.cos(), 0.0, angle.sin())
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

fn move_characters(
    time: Res<Time>,
    tuning: Res<Tuning>,
    layout: Res<ArenaLayout>,
    mas: MoveAndSlide,
    mut capsules: Local<Capsules>,
    mut characters: Query<
        (
            Entity,
            &PlayerIntent,
            &LookAngles,
            &mut Transform,
            &mut Motor,
            &mut EyeHeight,
        ),
        With<Character>,
    >,
    mut cues: MessageWriter<GameCue>,
) {
    let t = &tuning.movement;
    let dt_duration = time.delta();
    let dt = dt_duration.as_secs_f32();
    if dt <= 0.0 {
        return;
    }
    capsules.refresh(t);
    let mover = Mover {
        mas: &mas,
        filter: SpatialQueryFilter::from_mask([Layer::World, Layer::Piece]),
        config: MoveAndSlideConfig {
            penetration_rejection_threshold: MAX_DEPENETRATION,
            ..default()
        },
        depenetration: DepenetrationConfig {
            penetration_rejection_threshold: MAX_DEPENETRATION,
            ..default()
        },
        skin: MoveAndSlideConfig::default().skin_width * mas.length_unit.0,
        radius: t.radius.max(0.05),
        walkable_cos: t.max_slope_deg.to_radians().cos(),
    };
    let accel_time = t.accel_time.max(1e-3);
    let decel = t.run_speed.max(0.1) / t.stop_time.max(1e-3);

    for (entity, intent, look, mut transform, mut motor, mut eye) in &mut characters {
        let mut feet = transform.translation;
        if !feet.is_finite() || feet.y < KILL_HEIGHT {
            feet = Vec3::new(
                if feet.x.is_finite() { feet.x } else { 0.0 },
                1.0,
                if feet.z.is_finite() { feet.z } else { 0.0 },
            );
            motor.velocity = Vec3::ZERO;
        }

        // 1. Push out of anything that overlaps us (e.g. a piece placed on top of us).
        let half = t.capsule_height(motor.crouched) / 2.0;
        feet += mover.depenetrate(capsules.get(motor.crouched), feet + Vec3::Y * half);

        // 2. Timers and latched presses.
        if motor.grounded {
            motor.air_time = 0.0;
            motor.jumped = false;
        } else {
            motor.air_time += dt;
        }
        motor.slide_cooldown = (motor.slide_cooldown - dt).max(0.0);
        motor.jump_buffer_age = if intent.jump_pressed {
            Some(0.0)
        } else {
            motor
                .jump_buffer_age
                .map(|age| age + dt)
                .filter(|age| *age <= t.jump_buffer + TIME_EPS)
        };

        // 3. What the controller wants.
        let axis = intent.move_axis.clamp_length_max(1.0);
        let (forward, right) = look.flat_basis();
        let wish = forward * axis.y + right * axis.x;
        let wish_len = wish.length().min(1.0);
        let wish_dir = if wish_len > 1e-3 {
            wish.normalize()
        } else {
            Vec3::ZERO
        };
        let mut horizontal = flat(motor.velocity);
        let sprint_intent = intent.sprint && axis.y > 0.1;

        // 4. Slide entry: crouch pressed while sprinting on the ground.
        if intent.crouch_pressed
            && motor.grounded
            && !motor.sliding
            && motor.slide_cooldown <= 0.0
            && sprint_intent
            && horizontal.length() >= t.slide_min_speed
        {
            motor.sliding = true;
            motor.slide_time = 0.0;
            motor.slide_dir = horizontal.normalize();
            cues.write(GameCue::SlideStart { who: entity });
        }

        // 5. Crouch: shrinking is always allowed, standing needs headroom.
        let want_crouch = intent.crouch || motor.sliding;
        if want_crouch {
            motor.crouched = true;
        } else if motor.crouched {
            let crouch_half = t.capsule_height(true) / 2.0;
            let rise = t.capsule_height(false) - t.capsule_height(true);
            let blocked = mover
                .cast(
                    capsules.get(true),
                    feet + Vec3::Y * crouch_half,
                    Vec3::Y * rise,
                )
                .is_some();
            if !blocked {
                motor.crouched = false;
            }
        }

        // 6. Jump from the ground, or within coyote time of leaving it.
        let can_jump =
            motor.grounded || (!motor.jumped && motor.air_time <= t.coyote_time + TIME_EPS);
        let mut vertical = motor.velocity.y;
        let jumped_now = motor.jump_buffer_age.is_some() && can_jump;
        if jumped_now {
            vertical = t.jump_speed();
            motor.jumped = true;
            motor.jump_buffer_age = None;
            if motor.sliding {
                motor.sliding = false;
                motor.slide_cooldown = t.slide_cooldown;
            }
            cues.write(GameCue::Jump { who: entity });
        }
        let on_ground = motor.grounded && !jumped_now;

        // 7. Horizontal velocity.
        let mut slide_speed = 0.0;
        if motor.sliding && !on_ground {
            motor.sliding = false;
            motor.slide_cooldown = t.slide_cooldown;
        }
        if motor.sliding {
            let duration = t.slide_duration.max(1e-3);
            let progress = (motor.slide_time / duration).min(1.0);
            slide_speed = t.slide_speed + (t.crouch_speed - t.slide_speed) * progress;
            motor.slide_time += dt;
            if wish_dir != Vec3::ZERO {
                motor.slide_dir = steer(
                    motor.slide_dir,
                    wish_dir,
                    t.slide_steer_deg.to_radians() * dt,
                );
            }
            horizontal = motor.slide_dir * slide_speed;
            if motor.slide_time >= duration - TIME_EPS {
                motor.sliding = false;
                motor.slide_cooldown = t.slide_cooldown;
            }
        } else {
            let full = if motor.crouched {
                t.crouch_speed
            } else if sprint_intent {
                t.sprint_speed
            } else {
                t.run_speed
            };
            let accel = full / accel_time;
            let target = full * wish_len;
            horizontal = if on_ground {
                ground_accel(horizontal, wish_dir, target, accel, decel, dt)
            } else {
                air_accel(horizontal, wish_dir, target, accel * t.air_control, dt)
            };
        }
        motor.sprinting = sprint_intent && !motor.crouched;

        // 8. Vertical velocity: follow the ground plane, or fall (midpoint
        //    integration so the jump apex matches `jump_height` exactly).
        let (velocity, move_velocity) = if on_ground {
            let n = motor.ground_normal;
            let along_plane = if n.y > 1e-3 {
                -(horizontal.x * n.x + horizontal.z * n.z) / n.y
            } else {
                0.0
            };
            let v = horizontal + Vec3::Y * along_plane;
            (v, v)
        } else {
            let end = vertical - t.gravity * dt;
            (
                horizontal + Vec3::Y * end,
                horizontal + Vec3::Y * (vertical + end) / 2.0,
            )
        };

        // 9. Move and slide, stepping up small ledges when blocked on the ground.
        let shape = capsules.get(motor.crouched);
        let half = t.capsule_height(motor.crouched) / 2.0;
        let center = feet + Vec3::Y * half;
        let ground = on_ground.then_some(motor.ground_normal);
        let (mut new_center, mut slid, hit_steep) =
            mover.slide(shape, center, move_velocity, dt_duration, ground);
        if on_ground
            && hit_steep
            && horizontal.length_squared() > 1e-4
            && let Some((stepped, stepped_velocity)) = mover.try_step(
                shape,
                center,
                feet.y,
                horizontal,
                dt,
                t.step_height,
                new_center,
            )
        {
            new_center = stepped;
            slid = stepped_velocity;
        }
        // Carry over what collisions took away from the move velocity (landing,
        // bumping a ceiling, sliding along a wall) onto the end-of-tick velocity.
        let mut next_velocity = if on_ground {
            flat(slid)
        } else {
            velocity + (slid - move_velocity)
        };
        if motor.sliding {
            // A slide that runs into a wall ends; one that glances off follows it.
            if next_velocity.length() < 0.5 * slide_speed {
                motor.sliding = false;
                motor.slide_cooldown = t.slide_cooldown;
            } else {
                motor.slide_dir = flat(next_velocity).normalize_or(motor.slide_dir);
            }
        }

        // 10. Stay on the ground (snap down ramps and steps) or land.
        let was_grounded = motor.grounded;
        let mut grounded = false;
        let mut ground_normal = Vec3::Y;
        if !jumped_now {
            let probe = if on_ground {
                t.step_height.max(LANDING_PROBE)
            } else {
                LANDING_PROBE
            };
            let rising = !on_ground && next_velocity.y > RISING_SPEED;
            if !rising && let Some(ground) = mover.ground(shape, new_center, half, probe) {
                new_center.y -= ground.drop;
                grounded = true;
                ground_normal = ground.normal;
            }
        }
        if grounded {
            let impact = (-velocity.y).max(0.0);
            if !was_grounded && impact >= MIN_LAND_CUE_SPEED {
                cues.write(GameCue::Land {
                    who: entity,
                    speed: impact,
                });
            }
            next_velocity.y = 0.0;
        }

        // 11. Arena bounds.
        let mut new_feet = new_center - Vec3::Y * half;
        let clamped_x = new_feet.x.clamp(layout.bounds_min.x, layout.bounds_max.x);
        let clamped_z = new_feet.z.clamp(layout.bounds_min.y, layout.bounds_max.y);
        if clamped_x != new_feet.x {
            next_velocity.x = 0.0;
        }
        if clamped_z != new_feet.z {
            next_velocity.z = 0.0;
        }
        new_feet.x = clamped_x;
        new_feet.z = clamped_z;

        // 12. Footsteps on the ground (crouching and sliding are quiet).
        if grounded && on_ground && !motor.crouched {
            motor.stride += flat(new_feet - feet).length();
            let step = t.footstep_distance.max(0.1);
            if motor.stride >= step {
                motor.stride -= step;
                cues.write(GameCue::Footstep { who: entity });
            }
        }

        // 13. Eye height eases between standing and crouched over `crouch_time`.
        let target_eye = if motor.crouched {
            t.crouch_eye_height
        } else {
            t.eye_height
        };
        let eye_rate = (t.eye_height - t.crouch_eye_height).abs() / t.crouch_time.max(1e-3);
        let next_eye = eye.0 + (target_eye - eye.0).clamp(-eye_rate * dt, eye_rate * dt);
        if next_eye != eye.0 {
            eye.0 = next_eye;
        }

        motor.velocity = next_velocity;
        motor.grounded = grounded;
        motor.ground_normal = ground_normal;
        if transform.translation != new_feet {
            transform.translation = new_feet;
        }
    }
}

/// Lowers each character's hitboxes by however far its eye has dropped from
/// standing height, so crouching characters are shot where they appear.
fn place_hitboxes(
    tuning: Res<Tuning>,
    characters: Query<&EyeHeight, With<Character>>,
    mut hitboxes: Query<(&Hitbox, &mut Transform), Without<Character>>,
) {
    let body_center = (BODY_BOTTOM + BODY_TOP) / 2.0;
    for (hitbox, mut transform) in &mut hitboxes {
        let Ok(eye) = characters.get(hitbox.owner) else {
            continue;
        };
        let drop = (tuning.movement.eye_height - eye.0).max(0.0);
        let standing = if hitbox.head {
            HEAD_CENTER
        } else {
            body_center
        };
        let y = standing - drop;
        if (transform.translation.y - y).abs() > 1e-5 {
            transform.translation.y = y;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn ticks_until(
        mut v: Vec3,
        mut step: impl FnMut(Vec3) -> Vec3,
        done: impl Fn(Vec3) -> bool,
    ) -> u32 {
        for n in 1..=120 {
            v = step(v);
            if done(v) {
                return n;
            }
        }
        u32::MAX
    }

    #[test]
    fn ground_accel_reaches_target_and_stops_crisply() {
        let accel = 5.5 / 0.1;
        let decel = 5.5 / 0.06;
        let up = ticks_until(
            Vec3::ZERO,
            |v| ground_accel(v, Vec3::NEG_Z, 5.5, accel, decel, DT),
            |v| v.length() >= 5.5 - 1e-4,
        );
        assert_eq!(up, 6, "0.1 s to full speed");
        let down = ticks_until(
            Vec3::NEG_Z * 5.5,
            |v| ground_accel(v, Vec3::ZERO, 0.0, accel, decel, DT),
            |v| v.length() < 1e-4,
        );
        assert_eq!(down, 4, "about 0.06 s to stop");
    }

    #[test]
    fn reversing_brakes_then_accelerates() {
        let accel = 5.5 / 0.1;
        let decel = 5.5 / 0.06;
        let n = ticks_until(
            Vec3::X * 5.5,
            |v| ground_accel(v, Vec3::NEG_X, 5.5, accel, decel, DT),
            |v| v.x <= -5.5 + 1e-4,
        );
        assert!((9..=11).contains(&n), "reverse took {n} ticks");
    }

    #[test]
    fn air_control_redirects_without_gaining_speed() {
        let h = Vec3::NEG_Z * 7.5;
        let mut v = h;
        for _ in 0..120 {
            v = air_accel(v, Vec3::X, 5.5, 0.4 * 55.0, DT);
            assert!(v.length() <= 7.5 + 1e-4);
        }
        assert!(v.x > 5.0, "turned toward the wish direction: {v}");
        assert_eq!(
            air_accel(h, Vec3::ZERO, 5.5, 22.0, DT),
            h,
            "no air friction"
        );
    }

    #[test]
    fn steering_is_rate_limited_and_stays_flat() {
        let d = steer(Vec3::NEG_Z, Vec3::Z, 0.1);
        assert!((d.length() - 1.0).abs() < 1e-5 && d.y == 0.0);
        assert!((d.angle_between(Vec3::NEG_Z) - 0.1).abs() < 1e-4);
    }

    #[test]
    fn jump_speed_matches_height() {
        let t = MovementTuning::default();
        let v = t.jump_speed();
        assert!((v * v / (2.0 * t.gravity) - t.jump_height).abs() < 1e-5);
    }
}
