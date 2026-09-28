//! The grunt's wand and its spell orb (docs/M3-SPEC.md → The orb, D71).
//!
//! Knights carry a [`Wand`] instead of guns: combat's gun step skips any
//! character with one. A grunt holds `PlayerIntent::fire` to wind up; after
//! the wind-up (`GruntTuning::windup`, 0.4 s) the orb leaves the wand tip.
//! Releasing fire before the wind-up ends **cancels** it: no orb, and only a
//! short re-arm ([`REARM`]) instead of the fire interval. The brain lets go
//! the instant it loses line of sight, so no orb is ever fired blind (D76).
//! Orbs are real
//! projectiles stepped in the fixed tick with a sphere cast (never in the render
//! frame): they hit the player's hitboxes (head ×1.5), stop on world geometry
//! and pieces (chipping them through [`PieceHit`]) and pass through knights and
//! any other character that isn't the player. Damage to the player goes through
//! `Health::apply` and emits [`DamageDealt`] with `source` = the grunt, so the
//! HUD's damage arrow and the hit feedback work like any other hit (the
//! player's own hitmarker only shows for the player's own shots).
//!
//! **Where it flies.** The orb leaves the wand tip ([`wand_tip`]: the knight's
//! right gauntlet with the wand raised, [`WAND_TIP`]) and flies along the
//! knight's look direction (the brain aims from his eye; the small parallel
//! offset of the tip is accepted). A launch first sweeps from the knight's
//! own axis out to the tip, so a wand poking through a wall can't fire from
//! its far side (orbs never pass through pieces, D76).
//!
//! **Pool.** Orbs fly on a fixed pool of [`ORB_POOL`] [`OrbSlot`] entities
//! spawned at startup (and topped back up if anything despawns one). An idle
//! slot is parked at [`PARKED`] with no [`Orb`]; a launch claims one (or
//! recycles the orb that has flown farthest) and gives it an `Orb` for its
//! flight, removed when it stops. No entity is spawned per shot. A restart
//! calls [`recycle_all_orbs`] (or despawns every `Orb`; the pool refills).
//!
//! **Messages.** [`GameCue::WandWindup`] when a wind-up starts (the wand glow
//! and the off-screen warning sound key off it), [`GameCue::OrbFired`] on
//! release, and [`OrbImpact`] whenever an orb stops (the burst, chips, fizzle
//! and the player's bonk).
//!
//! Owned by slice B (the orb and the wand).

use crate::{
    building::Piece,
    combat::{Downed, ray_capsule, ray_sphere},
    shared::{
        Character, DamageDealt, DamageTarget, Eliminated, GameCue, Health, Hitbox, Layer,
        LookAngles, PieceHit, PieceKind, Player, PlayerIntent, SimSet, SimTick, TICK_SECONDS,
    },
    tuning::Tuning,
};
use avian3d::prelude::*;
use bevy::{ecs::system::SystemParam, prelude::*};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct OrbTuning {
    /// m/s
    pub speed: f32,
    /// Sphere-cast radius (m).
    pub radius: f32,
    pub damage: f32,
    pub headshot_multiplier: f32,
    pub structure_damage: f32,
    /// Distance before an orb fizzles (m).
    pub range: f32,
}

impl Default for OrbTuning {
    fn default() -> Self {
        Self {
            speed: 30.0,
            radius: 0.15,
            damage: 12.0,
            headshot_multiplier: 1.5,
            structure_damage: 12.0,
            range: 60.0,
        }
    }
}

/// Orbs in the pool: 8 knights × 2 in flight (the spec's floor), plus slack.
pub const ORB_POOL: usize = 24;

/// Where the wand tip is when an orb leaves it, relative to the knight's feet
/// in his yaw frame (x right, y up, -z forward): his right arm raised 70°
/// forward about the shoulder, the wand in his hand pointing 30° above level
/// (`arena::visuals::wand` poses the model to match).
pub const WAND_TIP: Vec3 = Vec3::new(0.266, 1.34, -0.98);

/// After a cancelled wind-up the wand may start another this soon (s): the
/// cancelled wind-up doesn't spend the fire interval.
pub const REARM: f32 = 0.2;

/// Where parked orbs wait, far below the island.
pub const PARKED: Vec3 = Vec3::new(0.0, -500.0, 0.0);

/// Timers land on tick boundaries within this.
const EPS: f32 = 1e-4;

/// A knight's wand: its fire timing. Present on every grunt.
#[derive(Component, Debug, Clone, Copy, PartialEq, Default)]
pub struct Wand {
    /// Seconds between orbs (from `GruntStats::fire_interval`).
    pub fire_interval: f32,
    /// Seconds until the wand may start another wind-up.
    pub cooldown: f32,
    /// Seconds of wind-up left, while winding up.
    pub windup: Option<f32>,
}

impl Wand {
    pub fn new(fire_interval: f32) -> Self {
        Self {
            fire_interval,
            ..default()
        }
    }

    /// Winding up or about to fire: the grunt holds an attack token.
    pub fn is_winding(&self) -> bool {
        self.windup.is_some()
    }

    /// How far through a wind-up of `total` seconds the wand is (0..=1), or
    /// `None` when not winding up.
    pub fn windup_progress(&self, total: f32) -> Option<f32> {
        self.windup
            .map(|left| (1.0 - left / total.max(EPS)).clamp(0.0, 1.0))
    }
}

/// A pool slot for an orb: always there, parked at [`PARKED`] while idle. It
/// carries an [`Orb`] only while that orb is in flight.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OrbSlot;

/// A spell orb in flight (on an [`OrbSlot`]; removed when it stops, so a
/// restart may despawn every `Orb` without touching the pool).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Orb {
    pub shooter: Entity,
    pub velocity: Vec3,
    /// Metres travelled so far.
    pub travelled: f32,
    /// Its position at the start of this tick, for render interpolation. The
    /// entity's `Transform.translation` is the authoritative position.
    pub previous: Vec3,
    /// The tick it was launched on.
    pub launched: u64,
}

/// What an orb stopped on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrbHit {
    /// The player's body or head hitbox.
    Player { head: bool },
    /// A building piece (its kind, for the chips).
    Piece(PieceKind),
    /// World geometry: it fizzles.
    World,
    /// It flew its full range.
    Expired,
}

/// Emitted when an orb stops, for its burst, chips, fizzle and sounds.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct OrbImpact {
    pub orb: Entity,
    pub shooter: Entity,
    pub hit: OrbHit,
    /// The entity hit (the player, a piece or a world collider).
    pub target: Option<Entity>,
    /// Where the orb's centre stopped, and the surface normal there.
    pub point: Vec3,
    pub normal: Vec3,
    pub velocity: Vec3,
    pub tick: u64,
}

/// A wand release waiting for its orb (filled by the wand step, drained by the
/// orb step in the same tick). Its capacity is reused, never reallocated in play.
#[derive(Resource, Default)]
struct Launches(Vec<Launch>);

#[derive(Debug, Clone, Copy)]
struct Launch {
    shooter: Entity,
    /// The knight's axis at wand height (the launch sweep's start), the wand
    /// tip and his aim direction.
    axis: Vec3,
    tip: Vec3,
    forward: Vec3,
}

/// The wand and orbs.
pub struct OrbPlugin;

impl Plugin for OrbPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<OrbImpact>()
            .insert_resource(Launches(Vec::with_capacity(ORB_POOL)))
            .add_systems(Startup, fill_pool)
            .add_systems(
                FixedUpdate,
                (fill_pool, wand_step, orb_step)
                    .chain()
                    .in_set(SimSet::Combat),
            );
    }
}

/// The wand tip for a knight standing at `feet` facing `yaw`.
pub fn wand_tip(feet: Vec3, yaw: f32) -> Vec3 {
    feet + Quat::from_rotation_y(yaw) * WAND_TIP
}

/// Spawns pool slots until there are [`ORB_POOL`] (all of them at startup; a
/// replacement if anything ever despawns one).
fn fill_pool(mut commands: Commands, slots: Query<(), With<OrbSlot>>) {
    for _ in slots.iter().count()..ORB_POOL {
        commands.spawn((
            Name::new("Orb"),
            OrbSlot,
            Transform::from_translation(PARKED),
        ));
    }
}

fn wand_step(
    tuning: Res<Tuning>,
    mut launches: ResMut<Launches>,
    mut cues: MessageWriter<GameCue>,
    mut wands: Query<(
        Entity,
        &mut Wand,
        &PlayerIntent,
        &Transform,
        &LookAngles,
        Has<Downed>,
    )>,
) {
    let dt = TICK_SECONDS;
    for (entity, mut wand, intent, transform, look, downed) in &mut wands {
        // Clamp so an idle wand can't bank shots, while a held trigger keeps
        // its sub-tick remainder (an exact average interval), like the guns.
        wand.cooldown = (wand.cooldown - dt).max(-dt);
        if downed {
            wand.windup = None;
            continue;
        }
        let held = intent.fire || intent.fire_pressed;
        if let Some(left) = wand.windup.as_mut() {
            if !held {
                // Let go: the wind-up is cancelled, and the interval with it.
                wand.windup = None;
                wand.cooldown = REARM;
                continue;
            }
            *left -= dt;
            if *left <= EPS {
                wand.windup = None;
                let feet = transform.translation;
                let tip = wand_tip(feet, look.yaw);
                launches.0.push(Launch {
                    shooter: entity,
                    axis: Vec3::new(feet.x, tip.y, feet.z),
                    tip,
                    forward: look.forward(),
                });
            }
        } else if held && wand.cooldown <= EPS {
            // The interval runs from one wind-up start to the next, so it is
            // also the release-to-release interval.
            wand.cooldown = if wand.cooldown > -dt + EPS {
                wand.cooldown + wand.fire_interval
            } else {
                wand.fire_interval
            };
            wand.windup = Some(tuning.grunt.windup);
            cues.write(GameCue::WandWindup { who: entity });
        }
    }
}

type PlayerParts<'w, 's> = Query<
    'w,
    's,
    (&'static Transform, &'static mut Health, Has<Downed>),
    (With<Player>, With<Character>, Without<Orb>),
>;

/// What one sweep hit first.
#[derive(Debug, Clone, Copy)]
struct Sweep {
    hit: OrbHit,
    target: Entity,
    distance: f32,
    normal: Vec3,
}

/// Everything a sweep needs to look at.
#[derive(SystemParam)]
struct Scene<'w, 's> {
    spatial: SpatialQuery<'w, 's>,
    colliders: Query<'w, 's, &'static ColliderOf>,
    characters: Query<'w, 's, (), With<Character>>,
    layers: Query<'w, 's, &'static CollisionLayers>,
    pieces: Query<'w, 's, &'static Piece>,
    hitboxes: Query<'w, 's, (&'static Hitbox, &'static Collider, &'static Transform), Without<Orb>>,
}

impl Scene<'_, '_> {
    /// The first thing a sphere of `radius` moving from `origin` along `dir`
    /// (unit) for `length` touches: world and pieces through avian, the
    /// player's hitboxes at their current transforms (as combat does, since
    /// avian's copies lag a tick). Knights and any other character are
    /// ignored: orbs pass through them.
    fn sweep(
        &self,
        origin: Vec3,
        dir: Vec3,
        length: f32,
        radius: f32,
        players: &PlayerParts,
    ) -> Option<Sweep> {
        let dir3 = Dir3::new(dir).ok()?;
        let filter = SpatialQueryFilter::from_mask([Layer::World, Layer::Piece]);
        // A character's own colliders (movement capsules) never stop an orb.
        let not_character = |e: Entity| {
            self.colliders
                .get(e)
                .map_or(true, |c| !self.characters.contains(c.body))
        };
        let config = ShapeCastConfig::from_max_distance(length);
        let hit = self.spatial.cast_shape_predicate(
            &Collider::sphere(radius.max(1e-3)),
            origin,
            Quat::IDENTITY,
            dir3,
            &config,
            &filter,
            &not_character,
        );
        let mut best = hit.map(|h| {
            let piece = self
                .layers
                .get(h.entity)
                .is_ok_and(|l| l.memberships.has_all(Layer::Piece));
            let hit = match (piece, self.pieces.get(h.entity)) {
                (true, Ok(p)) => OrbHit::Piece(p.kind),
                (true, Err(_)) => OrbHit::Piece(PieceKind::Wall),
                (false, _) => OrbHit::World,
            };
            Sweep {
                hit,
                target: h.entity,
                distance: h.distance,
                normal: h.normal1,
            }
        });
        for (hitbox, collider, local) in &self.hitboxes {
            let Ok((owner, health, downed)) = players.get(hitbox.owner) else {
                continue;
            };
            if downed || health.is_dead() {
                continue;
            }
            let pose = owner.mul_transform(*local);
            let max = best.as_ref().map_or(length, |b| b.distance);
            let shape = collider.shape_scaled();
            // A sphere sweep against a ball or capsule is a ray against the
            // same shape grown by the sphere's radius.
            let found = if let Some(ball) = shape.as_ball() {
                ray_sphere(origin, dir, pose.translation, ball.radius + radius, max)
            } else if let Some(capsule) = shape.as_capsule() {
                let a = pose.translation + pose.rotation * capsule.segment.a;
                let b = pose.translation + pose.rotation * capsule.segment.b;
                ray_capsule(origin, dir, a, b, capsule.radius + radius, max)
            } else {
                None
            };
            if let Some((distance, normal)) = found
                && distance < max
            {
                best = Some(Sweep {
                    hit: OrbHit::Player { head: hitbox.head },
                    target: hitbox.owner,
                    distance,
                    normal,
                });
            }
        }
        best
    }
}

/// Where an orb's hits go: damage and eliminations, piece chips, impacts.
#[derive(SystemParam)]
struct Outcomes<'w, 's> {
    commands: Commands<'w, 's>,
    cues: MessageWriter<'w, GameCue>,
    damage: MessageWriter<'w, DamageDealt>,
    piece_hits: MessageWriter<'w, PieceHit>,
    eliminated: MessageWriter<'w, Eliminated>,
    impacts: MessageWriter<'w, OrbImpact>,
}

impl Outcomes<'_, '_> {
    /// Applies what `sweep` hit (the orb's centre now at `at`) and reports
    /// the impact.
    #[allow(clippy::too_many_arguments)]
    fn resolve(
        &mut self,
        tick: u64,
        ot: &OrbTuning,
        entity: Entity,
        orb: &Orb,
        sweep: Sweep,
        at: Vec3,
        players: &mut PlayerParts,
    ) {
        let contact = at - sweep.normal * ot.radius;
        match sweep.hit {
            OrbHit::Player { head } => {
                if let Ok((player_tf, mut health, _)) = players.get_mut(sweep.target) {
                    let amount = ot.damage * if head { ot.headshot_multiplier } else { 1.0 };
                    let split = health.apply(amount);
                    let applied = split.to_shield + split.to_hp;
                    if applied > 0.0 {
                        self.damage.write(DamageDealt {
                            source: Some(orb.shooter),
                            target: sweep.target,
                            target_kind: DamageTarget::Character,
                            amount: applied,
                            to_shield: split.to_shield,
                            headshot: head,
                            shield_broke: split.shield_broke,
                            killed: split.killed,
                            point: contact,
                            normal: sweep.normal,
                            tick,
                        });
                    }
                    if split.killed {
                        self.eliminated.write(Eliminated {
                            victim: sweep.target,
                            by: Some(orb.shooter),
                            position: player_tf.translation,
                            tick,
                        });
                        self.commands.entity(sweep.target).insert(Downed { tick });
                    }
                }
            }
            OrbHit::Piece(_) => {
                self.piece_hits.write(PieceHit {
                    piece: sweep.target,
                    amount: ot.structure_damage,
                    source: Some(orb.shooter),
                    point: contact,
                    normal: sweep.normal,
                });
            }
            OrbHit::World | OrbHit::Expired => {}
        }
        self.impacts.write(OrbImpact {
            orb: entity,
            shooter: orb.shooter,
            hit: sweep.hit,
            target: Some(sweep.target),
            point: at,
            normal: sweep.normal,
            velocity: orb.velocity,
            tick,
        });
    }

    /// Flies `orb` (on slot `entity`) one tick: true while it is still in
    /// flight; false once it has stopped (and been reported).
    #[allow(clippy::too_many_arguments)]
    fn fly(
        &mut self,
        tick: u64,
        ot: &OrbTuning,
        scene: &Scene,
        entity: Entity,
        orb: &mut Orb,
        transform: &mut Transform,
        players: &mut PlayerParts,
    ) -> bool {
        let pos = transform.translation;
        orb.previous = pos;
        let speed = orb.velocity.length();
        let dir = orb.velocity / speed.max(1e-6);
        let step = (speed * TICK_SECONDS).min((ot.range - orb.travelled).max(0.0));
        if let Some(sweep) = scene.sweep(pos, dir, step, ot.radius, players) {
            orb.travelled += sweep.distance;
            let at = pos + dir * sweep.distance;
            self.resolve(tick, ot, entity, orb, sweep, at, players);
            return false;
        }
        transform.translation = pos + dir * step;
        orb.travelled += step;
        if orb.travelled < ot.range - EPS {
            return true;
        }
        self.impacts.write(OrbImpact {
            orb: entity,
            shooter: orb.shooter,
            hit: OrbHit::Expired,
            target: None,
            point: transform.translation,
            normal: -dir,
            velocity: orb.velocity,
            tick,
        });
        false
    }
}

type Slots<'w, 's> = Query<
    'w,
    's,
    (Entity, Option<&'static mut Orb>, &'static mut Transform),
    (With<OrbSlot>, Without<Character>, Without<Hitbox>),
>;

/// Moves every orb in flight one tick and resolves what it touches, then
/// launches this tick's orbs (each flies its first tick at once).
fn orb_step(
    tuning: Res<Tuning>,
    tick: Res<SimTick>,
    mut launches: ResMut<Launches>,
    scene: Scene,
    mut players: PlayerParts,
    mut slots: Slots,
    mut out: Outcomes,
) {
    let tick = tick.0;
    let ot = &tuning.orb;

    for (entity, orb, mut transform) in &mut slots {
        let Some(mut orb) = orb else {
            continue;
        };
        if !out.fly(
            tick,
            ot,
            &scene,
            entity,
            &mut orb,
            &mut transform,
            &mut players,
        ) {
            transform.translation = PARKED;
            out.commands.entity(entity).remove::<Orb>();
        }
    }

    // Launches: claim an idle slot, or recycle the orb that has flown
    // farthest. Slots claimed this tick (their `Orb` is still a command
    // away) are skipped.
    let mut claimed = [Entity::PLACEHOLDER; ORB_POOL];
    for (n, launch) in launches.0.drain(..).enumerate() {
        let taken = |e: &Entity| claimed[..n.min(ORB_POOL)].contains(e);
        let idle = slots
            .iter()
            .find(|(e, orb, _)| orb.is_none() && !taken(e))
            .map(|(e, _, _)| (e, false));
        let pick = idle.or_else(|| {
            slots
                .iter()
                .filter(|(e, orb, _)| orb.is_some() && !taken(e))
                .max_by(|a, b| {
                    let far = |o: &Option<&Orb>| o.map_or(0.0, |o| o.travelled);
                    far(&a.1).total_cmp(&far(&b.1))
                })
                .map(|(e, _, _)| (e, true))
        });
        let Some((entity, recycled)) = pick else {
            continue;
        };
        if n < ORB_POOL {
            claimed[n] = entity;
        }
        let Ok((_, _, mut transform)) = slots.get_mut(entity) else {
            continue;
        };
        let mut orb = Orb {
            shooter: launch.shooter,
            velocity: launch.forward.normalize_or(Vec3::NEG_Z) * ot.speed,
            travelled: 0.0,
            previous: launch.tip,
            launched: tick,
        };
        transform.translation = launch.tip;
        out.cues.write(GameCue::OrbFired {
            who: launch.shooter,
        });
        // The wand pokes out ahead of the knight: sweep out to the tip from
        // his axis, so an orb never starts on the far side of a wall.
        let reach = launch.tip - launch.axis;
        let length = reach.length();
        let blocked = (length > 1e-4)
            .then(|| scene.sweep(launch.axis, reach / length, length, ot.radius, &players))
            .flatten();
        let flying = match blocked {
            Some(sweep) => {
                let at = launch.axis + reach / length * sweep.distance;
                orb.previous = at;
                out.resolve(tick, ot, entity, &orb, sweep, at, &mut players);
                false
            }
            None => out.fly(
                tick,
                ot,
                &scene,
                entity,
                &mut orb,
                &mut transform,
                &mut players,
            ),
        };
        if flying {
            out.commands.entity(entity).insert(orb);
        } else {
            transform.translation = PARKED;
            if recycled {
                out.commands.entity(entity).remove::<Orb>();
            }
        }
    }
}

/// Stops every orb in flight and returns it to the pool (a run restart
/// clears the sky; the pool's slots stay). Despawning every [`Orb`] entity
/// works too: the pool refills its slots on the next tick.
pub fn recycle_all_orbs(world: &mut World) {
    let flying: Vec<Entity> = world
        .query_filtered::<Entity, With<Orb>>()
        .iter(world)
        .collect();
    for entity in flying {
        let mut e = world.entity_mut(entity);
        e.remove::<Orb>();
        if let Some(mut t) = e.get_mut::<Transform>() {
            t.translation = PARKED;
        }
    }
    world.resource_mut::<Launches>().0.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windup_progress_runs_from_zero_to_one() {
        let mut w = Wand::new(1.5);
        assert_eq!(w.windup_progress(0.4), None);
        w.windup = Some(0.4);
        assert_eq!(w.windup_progress(0.4), Some(0.0));
        w.windup = Some(0.1);
        assert!((w.windup_progress(0.4).unwrap() - 0.75).abs() < 1e-5);
    }

    #[test]
    fn the_wand_tip_turns_with_the_knight() {
        let feet = Vec3::new(1.0, 0.0, 2.0);
        let ahead = wand_tip(feet, 0.0);
        assert!(ahead.z < feet.z - 0.5 && ahead.x > feet.x, "{ahead}");
        // Facing +X (yaw -90°), the tip is ahead along +X and to his right (+Z).
        let east = wand_tip(feet, -std::f32::consts::FRAC_PI_2);
        assert!(east.x > feet.x + 0.5 && east.z > feet.z, "{east}");
        assert!((ahead.y - WAND_TIP.y).abs() < 1e-6);
    }
}
