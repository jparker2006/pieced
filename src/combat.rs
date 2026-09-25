//! Slice C — weapons, hitscan, damage, health, ADS and reloads.
//!
//! Everything here runs in [`SimSet::Combat`] for every [`Character`], driven only
//! by [`PlayerIntent`] and [`ActiveTool`]. Presentation reads the public state:
//! [`Loadout`] (ammo, reload, bloom, switch lock), [`Ads`], [`Downed`] and the
//! player's [`CombatStats`], plus the [`ShotFired`], [`DamageDealt`],
//! [`Eliminated`] and [`GameCue`] messages.
//!
//! **Hitbox lag.** avian syncs collider `Position`s and its query BVH in the
//! physics step (`FixedPostUpdate`), *after* `FixedUpdate`. A character moved this
//! tick (or teleported between ticks) still has last tick's hitbox positions when
//! combat runs, so a plain `SpatialQuery` ray against `Body`/`Head` would test
//! stale hitboxes. Hitscan therefore uses `SpatialQuery` only for `World | Piece`
//! and ray-tests each character's hitbox colliders at their *current*
//! `Transform` (owner transform × hitbox local transform).
//!
//! **Analytic hitboxes.** parry casts rays against capsules with GJK, which
//! misses about 0.3% of rays aimed straight through the capsule's axis (measured
//! on the body hitbox). Capsule and ball hitboxes are therefore ray-tested
//! analytically here ([`ray_capsule`], [`ray_sphere`]); other shapes fall back to
//! avian.

use crate::{
    rng::{Rng, SimRng},
    shared::{
        ActiveTool, Ads, Character, DamageDealt, DamageTarget, Eliminated, EyeHeight, GameCue,
        Health, Hitbox, Layer, LookAngles, PieceHit, Player, PlayerIntent, ShotFired, ShotTrace,
        SimSet, SimTick, TICK_SECONDS, WeaponKind, eye_ray,
    },
    tuning::Tuning,
};
use avian3d::prelude::*;
use bevy::{platform::collections::HashMap, prelude::*};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GunTuning {
    /// Seconds between shots.
    pub fire_interval: f32,
    /// Body damage per bullet or pellet.
    pub damage: f32,
    pub headshot_multiplier: f32,
    /// Full damage up to this distance (m).
    pub falloff_start: f32,
    /// Damage reaches `falloff_min` of full at this distance (m).
    pub falloff_end: f32,
    pub falloff_min: f32,
    pub range: f32,
    pub magazine: u32,
    /// Magazine reload time, or per-shell time when `reload_per_shell`.
    pub reload_time: f32,
    pub reload_per_shell: bool,
    pub base_spread_deg: f32,
    pub bloom_per_shot_deg: f32,
    pub bloom_max_deg: f32,
    pub bloom_recover_delay: f32,
    pub bloom_recover_deg_per_sec: f32,
    pub ads_spread_multiplier: f32,
    /// FOV multiplier while aiming down sights.
    pub ads_zoom: f32,
    pub pellets: u32,
    /// Radius of the fixed pellet pattern (degrees).
    pub pellet_spread_deg: f32,
    /// Damage to building pieces per bullet or pellet.
    pub structure_damage: f32,
}

impl GunTuning {
    pub fn rifle() -> Self {
        Self {
            fire_interval: 1.0 / 6.0,
            damage: 28.0,
            headshot_multiplier: 1.5,
            falloff_start: 25.0,
            falloff_end: 50.0,
            falloff_min: 0.7,
            range: 150.0,
            magazine: 30,
            reload_time: 2.0,
            reload_per_shell: false,
            base_spread_deg: 0.05,
            bloom_per_shot_deg: 0.3,
            bloom_max_deg: 1.8,
            bloom_recover_delay: 0.3,
            bloom_recover_deg_per_sec: 8.0,
            ads_spread_multiplier: 0.4,
            ads_zoom: 0.75,
            pellets: 1,
            pellet_spread_deg: 0.0,
            structure_damage: 28.0,
        }
    }

    pub fn pump() -> Self {
        Self {
            fire_interval: 0.9,
            damage: 10.0,
            headshot_multiplier: 1.5,
            falloff_start: 8.0,
            falloff_end: 15.0,
            falloff_min: 0.3,
            range: 40.0,
            magazine: 5,
            reload_time: 0.5,
            reload_per_shell: true,
            base_spread_deg: 0.0,
            bloom_per_shot_deg: 0.0,
            bloom_max_deg: 0.0,
            bloom_recover_delay: 0.0,
            bloom_recover_deg_per_sec: 0.0,
            ads_spread_multiplier: 0.8,
            ads_zoom: 0.9,
            pellets: 10,
            pellet_spread_deg: 4.5,
            structure_damage: 10.0,
        }
    }

    /// Damage multiplier at `distance` meters: 1 up to `falloff_start`, then linear
    /// down to `falloff_min` at `falloff_end`, and `falloff_min` beyond.
    pub fn falloff(&self, distance: f32) -> f32 {
        if distance <= self.falloff_start {
            1.0
        } else if distance >= self.falloff_end || self.falloff_end <= self.falloff_start {
            self.falloff_min
        } else {
            let t = (distance - self.falloff_start) / (self.falloff_end - self.falloff_start);
            1.0 + (self.falloff_min - 1.0) * t
        }
    }

    /// Damage one bullet or pellet does to a character at `distance`.
    pub fn damage_at(&self, distance: f32, headshot: bool) -> f32 {
        let head = if headshot {
            self.headshot_multiplier
        } else {
            1.0
        };
        self.damage * head * self.falloff(distance)
    }

    /// The pump's fixed pellet pattern as angular offsets in degrees
    /// (x = right, y = up), before any ADS tightening. Identical every shot:
    /// one center pellet, an inner ring at half radius and an outer ring at
    /// `pellet_spread_deg`.
    pub fn pellet_pattern(&self) -> Vec<Vec2> {
        let n = self.pellets.max(1) as usize;
        let mut out = vec![Vec2::ZERO];
        if n == 1 {
            return out;
        }
        let rest = n - 1;
        let inner = rest / 3;
        let outer = rest - inner;
        let r = self.pellet_spread_deg;
        for i in 0..inner {
            // A triangle pointing up.
            let a = std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * i as f32 / inner as f32;
            out.push(Vec2::new(a.cos(), a.sin()) * r * 0.5);
        }
        for i in 0..outer {
            let a = std::f32::consts::TAU * i as f32 / outer as f32;
            out.push(Vec2::new(a.cos(), a.sin()) * r);
        }
        out
    }

    /// Hits needed to take `hp` of structure off a piece.
    pub fn shots_to_break(&self, hp: f32) -> u32 {
        if self.structure_damage <= 0.0 {
            return u32::MAX;
        }
        (hp / self.structure_damage).ceil() as u32
    }
}

impl Default for GunTuning {
    fn default() -> Self {
        Self::rifle()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct CombatTuning {
    pub rifle: GunTuning,
    pub pump: GunTuning,
    pub switch_time: f32,
    /// A pump press this early before it's ready still fires when ready.
    pub pump_press_buffer: f32,
    pub max_hp: f32,
    pub max_shield: f32,
    pub aim_friction: bool,
    pub aim_friction_strength: f32,
}

impl Default for CombatTuning {
    fn default() -> Self {
        Self {
            rifle: GunTuning::rifle(),
            pump: GunTuning::pump(),
            switch_time: 0.2,
            pump_press_buffer: 0.15,
            max_hp: 100.0,
            max_shield: 100.0,
            aim_friction: false,
            aim_friction_strength: 0.35,
        }
    }
}

impl CombatTuning {
    pub fn gun(&self, kind: WeaponKind) -> &GunTuning {
        match kind {
            WeaponKind::Rifle => &self.rifle,
            WeaponKind::Pump => &self.pump,
        }
    }
}

// ---------------------------------------------------------------------------
// Per-character state
// ---------------------------------------------------------------------------

/// Tolerance for timers that should land exactly on a tick boundary.
const EPS: f32 = 1e-4;

/// One gun's live state.
#[derive(Debug, Clone, PartialEq)]
pub struct GunState {
    /// Rounds (rifle) or shells (pump) loaded.
    pub ammo: u32,
    /// Seconds left in the current reload step (the whole magazine for the
    /// rifle, the next shell for the pump). `None` when not reloading.
    pub reload: Option<f32>,
    /// Seconds until the gun can fire again (≤ 0 means ready).
    pub cooldown: f32,
    /// Extra spread from sustained fire, in degrees.
    pub bloom_deg: f32,
    /// Seconds since this gun last fired.
    pub since_shot: f32,
}

impl GunState {
    pub fn new(tuning: &GunTuning) -> Self {
        Self {
            ammo: tuning.magazine,
            reload: None,
            cooldown: 0.0,
            bloom_deg: 0.0,
            since_shot: 1.0e6,
        }
    }

    pub fn is_reloading(&self) -> bool {
        self.reload.is_some()
    }

    /// Progress of the current reload step in 0..=1 (for HUD and viewmodel).
    pub fn reload_progress(&self, tuning: &GunTuning) -> Option<f32> {
        self.reload
            .map(|left| (1.0 - left / tuning.reload_time.max(EPS)).clamp(0.0, 1.0))
    }

    /// Whether the fire-rate cooldown has elapsed.
    pub fn ready(&self) -> bool {
        self.cooldown <= EPS
    }

    /// Spread cone half-angle (degrees) the next shot will use. The pump's pattern
    /// is fixed and only tightened by ADS; see [`GunTuning::pellet_pattern`].
    pub fn spread_deg(&self, tuning: &GunTuning, ads: bool) -> f32 {
        let ads_mult = if ads {
            tuning.ads_spread_multiplier
        } else {
            1.0
        };
        (tuning.base_spread_deg + self.bloom_deg) * ads_mult
    }

    fn tick(&mut self, tuning: &GunTuning, dt: f32) {
        // Clamp so an idle gun can't bank shots, while a held trigger keeps its
        // sub-tick remainder (exact average fire rate).
        self.cooldown = (self.cooldown - dt).max(-dt);
        self.since_shot = (self.since_shot + dt).min(1.0e6);
        if self.since_shot >= tuning.bloom_recover_delay - EPS {
            self.bloom_deg = (self.bloom_deg - tuning.bloom_recover_deg_per_sec * dt).max(0.0);
        }
    }

    fn on_fire(&mut self, tuning: &GunTuning, dt: f32) {
        self.ammo = self.ammo.saturating_sub(1);
        self.cooldown = if self.cooldown > -dt + EPS {
            self.cooldown + tuning.fire_interval
        } else {
            tuning.fire_interval
        };
        self.bloom_deg = (self.bloom_deg + tuning.bloom_per_shot_deg).min(tuning.bloom_max_deg);
        self.since_shot = 0.0;
    }
}

/// Every character's guns: ammo, reloads, cooldowns, bloom, the weapon-switch
/// lock and the pump's early-press buffer. Added to each [`Character`] on spawn.
#[derive(Component, Debug, Clone)]
pub struct Loadout {
    pub rifle: GunState,
    pub pump: GunState,
    /// Seconds left before the newly selected tool can fire.
    pub switch_remaining: f32,
    /// A buffered pump press: seconds of buffer window left.
    pub pump_buffer: Option<f32>,
    tool: ActiveTool,
    rng: Rng,
}

impl Loadout {
    pub fn new(tuning: &CombatTuning, tool: ActiveTool, rng: Rng) -> Self {
        Self {
            rifle: GunState::new(&tuning.rifle),
            pump: GunState::new(&tuning.pump),
            switch_remaining: 0.0,
            pump_buffer: None,
            tool,
            rng,
        }
    }

    pub fn gun(&self, kind: WeaponKind) -> &GunState {
        match kind {
            WeaponKind::Rifle => &self.rifle,
            WeaponKind::Pump => &self.pump,
        }
    }

    pub fn gun_mut(&mut self, kind: WeaponKind) -> &mut GunState {
        match kind {
            WeaponKind::Rifle => &mut self.rifle,
            WeaponKind::Pump => &mut self.pump,
        }
    }

    pub fn is_switching(&self) -> bool {
        self.switch_remaining > EPS
    }
}

/// Marks a character that has been eliminated and can't be hit. The dummy module
/// removes it on respawn; visuals read it for the downed look.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Downed {
    /// Simulation tick of the elimination.
    pub tick: u64,
}

/// The player's combat readout for the HUD.
#[derive(Resource, Debug, Default, Clone)]
pub struct CombatStats {
    /// Trigger pulls (one per pump shot, not per pellet).
    pub shots: u32,
    /// Shots that damaged a character.
    pub hits: u32,
    /// Shots with at least one head hit.
    pub headshots: u32,
    pub eliminations: u32,
    /// Seconds from first damage to elimination on the latest target killed.
    pub last_ttk: Option<f32>,
    first_damage: HashMap<Entity, u64>,
}

impl CombatStats {
    /// Hits per shot, 0..=1.
    pub fn accuracy(&self) -> f32 {
        if self.shots == 0 {
            0.0
        } else {
            self.hits as f32 / self.shots as f32
        }
    }

    /// Headshots per hit, 0..=1.
    pub fn headshot_rate(&self) -> f32 {
        if self.hits == 0 {
            0.0
        } else {
            self.headshots as f32 / self.hits as f32
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CombatStats>()
            .init_resource::<ShotQueue>()
            .add_observer(attach_loadout)
            .add_systems(
                FixedUpdate,
                (weapon_step, resolve_shots).chain().in_set(SimSet::Combat),
            );
    }
}

/// Salt for per-character weapon RNG streams.
const LOADOUT_SALT: u64 = 0xC0_4BA7;

fn attach_loadout(
    add: On<Add, Character>,
    tools: Query<(&ActiveTool, Has<Player>)>,
    tuning: Res<Tuning>,
    sim_rng: Res<SimRng>,
    mut commands: Commands,
) {
    let (tool, is_player) = tools
        .get(add.entity)
        .map(|(t, p)| (*t, p))
        .unwrap_or_default();
    // Fork from a clone so the stream depends on the seed and the character's
    // role, not on the order characters happen to spawn in.
    let salt = if is_player { 1 } else { add.entity.to_bits() };
    let rng = sim_rng.0.clone().fork(LOADOUT_SALT ^ salt);
    commands
        .entity(add.entity)
        .insert(Loadout::new(&tuning.combat, tool, rng));
}

struct PendingShot {
    shooter: Entity,
    weapon: WeaponKind,
    origin: Vec3,
    dirs: Vec<Dir3>,
}

#[derive(Resource, Default)]
struct ShotQueue(Vec<PendingShot>);

/// Direction `offset_deg` (x right, y up) away from the look direction.
pub fn offset_dir(look: &LookAngles, offset_deg: Vec2) -> Dir3 {
    let rot = look.rotation();
    let fwd = rot * Vec3::NEG_Z;
    let right = rot * Vec3::X;
    let up = rot * Vec3::Y;
    let v = fwd + right * offset_deg.x.to_radians().tan() + up * offset_deg.y.to_radians().tan();
    Dir3::new(v).unwrap_or(Dir3::new(fwd).unwrap_or(Dir3::NEG_Z))
}

fn weapon_step(
    tuning: Res<Tuning>,
    mut queue: ResMut<ShotQueue>,
    mut cues: MessageWriter<GameCue>,
    mut q: Query<
        (
            Entity,
            &mut Loadout,
            &mut Ads,
            &PlayerIntent,
            &ActiveTool,
            &Transform,
            &EyeHeight,
            &LookAngles,
            Has<Downed>,
        ),
        With<Character>,
    >,
) {
    let dt = TICK_SECONDS;
    let ct = &tuning.combat;
    for (entity, mut loadout, mut ads, intent, tool, transform, eye, look, downed) in &mut q {
        let lo = &mut *loadout;
        if downed {
            lo.pump_buffer = None;
            continue;
        }

        // Timers.
        lo.switch_remaining = (lo.switch_remaining - dt).max(0.0);
        lo.rifle.tick(&ct.rifle, dt);
        lo.pump.tick(&ct.pump, dt);

        // Tool changes start the switch lock and cancel the old gun's reload.
        if *tool != lo.tool {
            if let ActiveTool::Weapon(prev) = lo.tool {
                lo.gun_mut(prev).reload = None;
            }
            lo.tool = *tool;
            lo.switch_remaining = ct.switch_time;
            lo.pump_buffer = None;
        }

        let mut want_ads = ads.0;
        if intent.ads_toggle_pressed {
            want_ads = !want_ads;
        }

        if let ActiveTool::Weapon(kind) = *tool {
            let gt = ct.gun(kind);
            let switching = lo.is_switching();

            // Reload progress.
            let gun = lo.gun_mut(kind);
            if let Some(left) = gun.reload.as_mut() {
                *left -= dt;
                if *left <= EPS {
                    if gt.reload_per_shell {
                        gun.ammo = (gun.ammo + 1).min(gt.magazine);
                        cues.write(GameCue::ReloadShell { who: entity });
                        if gun.ammo < gt.magazine {
                            *left += gt.reload_time;
                        } else {
                            gun.reload = None;
                            cues.write(GameCue::ReloadDone {
                                who: entity,
                                weapon: kind,
                            });
                        }
                    } else {
                        gun.ammo = gt.magazine;
                        gun.reload = None;
                        cues.write(GameCue::ReloadDone {
                            who: entity,
                            weapon: kind,
                        });
                    }
                }
            }

            // Manual reload.
            if intent.reload_pressed && !switching && gun.reload.is_none() && gun.ammo < gt.magazine
            {
                gun.reload = Some(gt.reload_time);
                cues.write(GameCue::ReloadStart {
                    who: entity,
                    weapon: kind,
                });
            }

            // Trigger: the rifle is full-auto while held; the pump is semi-auto with
            // an early-press buffer.
            let wants_fire = match kind {
                WeaponKind::Rifle => intent.fire || intent.fire_pressed,
                WeaponKind::Pump => {
                    if intent.fire_pressed {
                        lo.pump_buffer = Some(ct.pump_press_buffer);
                    }
                    lo.pump_buffer.is_some_and(|left| left >= -EPS)
                }
            };
            let gun = lo.gun_mut(kind);
            // Pump firing interrupts its shell-by-shell reload once a shell is in
            // (the `ammo > 0` check); the rifle's magazine reload can't be interrupted.
            let reload_blocks = kind == WeaponKind::Rifle && gun.is_reloading();
            let can_fire = !switching && gun.ammo > 0 && gun.ready() && !reload_blocks;
            if wants_fire && can_fire {
                gun.reload = None;
                let spread = gun.spread_deg(gt, want_ads);
                gun.on_fire(gt, dt);
                let (origin, _) = eye_ray(transform, eye, look);
                let dirs = match kind {
                    WeaponKind::Rifle => {
                        // Uniform over the spread disc.
                        let r = spread * lo.rng.next_f32().sqrt();
                        let theta = lo.rng.range(0.0, std::f32::consts::TAU);
                        vec![offset_dir(look, Vec2::new(theta.cos(), theta.sin()) * r)]
                    }
                    WeaponKind::Pump => {
                        let tighten = if want_ads {
                            gt.ads_spread_multiplier
                        } else {
                            1.0
                        };
                        lo.pump_buffer = None;
                        gt.pellet_pattern()
                            .into_iter()
                            .map(|o| offset_dir(look, o * tighten))
                            .collect()
                    }
                };
                queue.0.push(PendingShot {
                    shooter: entity,
                    weapon: kind,
                    origin,
                    dirs,
                });
            } else if let Some(left) = lo.pump_buffer.as_mut() {
                *left -= dt;
                if *left < -EPS {
                    lo.pump_buffer = None;
                }
            }

            // Auto-reload on empty.
            let switching = lo.is_switching();
            let gun = lo.gun_mut(kind);
            if gun.ammo == 0 && gun.reload.is_none() && !switching {
                gun.reload = Some(gt.reload_time);
                cues.write(GameCue::ReloadStart {
                    who: entity,
                    weapon: kind,
                });
            }
        }

        // ADS: off in build mode, while sprinting and while the rifle reloads.
        let sprinting = intent.sprint && intent.move_axis != Vec2::ZERO;
        let rifle_reloading =
            *tool == ActiveTool::Weapon(WeaponKind::Rifle) && lo.rifle.is_reloading();
        if tool.is_build() || sprinting || rifle_reloading {
            want_ads = false;
        }
        if want_ads != ads.0 {
            ads.0 = want_ads;
            cues.write(GameCue::AdsChanged {
                who: entity,
                ads: want_ads,
            });
        }
    }
}

/// What one ray hit first.
#[derive(Clone, Copy)]
enum HitKind {
    World,
    Piece,
    Character { head: bool },
}

struct RayHit {
    entity: Entity,
    kind: HitKind,
    distance: f32,
    normal: Vec3,
}

struct TargetDamage {
    target: Entity,
    amount: f32,
    headshot: bool,
    point: Vec3,
    normal: Vec3,
}

type CharacterQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Transform,
        &'static mut Health,
        Has<Downed>,
        Has<Player>,
    ),
    With<Character>,
>;

fn resolve_shots(
    mut commands: Commands,
    mut queue: ResMut<ShotQueue>,
    tuning: Res<Tuning>,
    tick: Res<SimTick>,
    spatial: SpatialQuery,
    collider_of: Query<&ColliderOf>,
    layers: Query<&CollisionLayers>,
    hitboxes: Query<(&Hitbox, &Collider, &Transform)>,
    mut characters: CharacterQuery,
    mut stats: ResMut<CombatStats>,
    mut shots: MessageWriter<ShotFired>,
    mut damage: MessageWriter<DamageDealt>,
    mut piece_hits: MessageWriter<PieceHit>,
    mut eliminated: MessageWriter<Eliminated>,
) {
    let tick = tick.0;
    let static_filter = SpatialQueryFilter::from_mask([Layer::World, Layer::Piece]);
    for shot in queue.0.drain(..) {
        let gt = tuning.combat.gun(shot.weapon);
        let mut traces = Vec::with_capacity(shot.dirs.len());
        let mut to_characters: Vec<TargetDamage> = Vec::new();
        let mut to_pieces: Vec<TargetDamage> = Vec::new();

        for dir in &shot.dirs {
            let hit = trace_ray(
                shot.shooter,
                shot.origin,
                *dir,
                gt.range,
                &spatial,
                &static_filter,
                &collider_of,
                &layers,
                &hitboxes,
                &characters,
            );
            let Some(hit) = hit else {
                traces.push(ShotTrace {
                    end: shot.origin + dir.as_vec3() * gt.range,
                    normal: Vec3::ZERO,
                    hit: None,
                });
                continue;
            };
            let point = shot.origin + dir.as_vec3() * hit.distance;
            traces.push(ShotTrace {
                end: point,
                normal: hit.normal,
                hit: Some(hit.entity),
            });
            let (bucket, amount, headshot) = match hit.kind {
                HitKind::World => continue,
                HitKind::Piece => (&mut to_pieces, gt.structure_damage, false),
                HitKind::Character { head } => {
                    (&mut to_characters, gt.damage_at(hit.distance, head), head)
                }
            };
            if let Some(acc) = bucket.iter_mut().find(|a| a.target == hit.entity) {
                acc.amount += amount;
                acc.headshot |= headshot;
            } else {
                bucket.push(TargetDamage {
                    target: hit.entity,
                    amount,
                    headshot,
                    point,
                    normal: hit.normal,
                });
            }
        }

        let shooter_is_player = characters
            .get(shot.shooter)
            .is_ok_and(|(_, _, _, player)| player);

        let mut shot_hit = false;
        let mut shot_headshot = false;
        for acc in to_characters {
            let Ok((transform, mut health, _, _)) = characters.get_mut(acc.target) else {
                continue;
            };
            let split = health.apply(acc.amount);
            let applied = split.to_shield + split.to_hp;
            if applied <= 0.0 {
                continue;
            }
            shot_hit = true;
            shot_headshot |= acc.headshot;
            damage.write(DamageDealt {
                source: Some(shot.shooter),
                target: acc.target,
                target_kind: DamageTarget::Character,
                amount: applied,
                to_shield: split.to_shield,
                headshot: acc.headshot,
                shield_broke: split.shield_broke,
                killed: split.killed,
                point: acc.point,
                normal: acc.normal,
                tick,
            });
            if shooter_is_player {
                stats.first_damage.entry(acc.target).or_insert(tick);
            }
            if split.killed {
                eliminated.write(Eliminated {
                    victim: acc.target,
                    by: Some(shot.shooter),
                    position: transform.translation,
                    tick,
                });
                commands.entity(acc.target).insert(Downed { tick });
                if shooter_is_player {
                    stats.eliminations += 1;
                }
                if let Some(first) = stats.first_damage.remove(&acc.target)
                    && shooter_is_player
                {
                    stats.last_ttk = Some((tick - first) as f32 * TICK_SECONDS);
                }
            }
        }
        for acc in to_pieces {
            piece_hits.write(PieceHit {
                piece: acc.target,
                amount: acc.amount,
                source: Some(shot.shooter),
                point: acc.point,
                normal: acc.normal,
            });
        }

        if shooter_is_player {
            stats.shots += 1;
            stats.hits += u32::from(shot_hit);
            stats.headshots += u32::from(shot_headshot);
        }
        shots.write(ShotFired {
            shooter: shot.shooter,
            weapon: shot.weapon,
            origin: shot.origin,
            traces,
            tick,
        });
    }
}

/// First thing a ray hits: static world and pieces through avian's query, and
/// character hitboxes at their current transforms (see the module docs).
fn trace_ray(
    shooter: Entity,
    origin: Vec3,
    dir: Dir3,
    range: f32,
    spatial: &SpatialQuery,
    static_filter: &SpatialQueryFilter,
    collider_of: &Query<&ColliderOf>,
    layers: &Query<&CollisionLayers>,
    hitboxes: &Query<(&Hitbox, &Collider, &Transform)>,
    characters: &CharacterQuery,
) -> Option<RayHit> {
    // Colliders owned by a character (e.g. a movement capsule) never stop bullets;
    // characters are hit only through their hitboxes.
    let not_character = |e: Entity| {
        collider_of
            .get(e)
            .map_or(true, |c| !characters.contains(c.body))
    };
    let mut best = spatial
        .cast_ray_predicate(origin, dir, range, true, static_filter, &not_character)
        .map(|h| {
            let piece = layers
                .get(h.entity)
                .is_ok_and(|l| l.memberships.has_all(Layer::Piece));
            RayHit {
                entity: h.entity,
                kind: if piece {
                    HitKind::Piece
                } else {
                    HitKind::World
                },
                distance: h.distance,
                normal: h.normal,
            }
        });

    for (hitbox, collider, local) in hitboxes {
        if hitbox.owner == shooter {
            continue;
        }
        let Ok((owner_tf, health, downed, _)) = characters.get(hitbox.owner) else {
            continue;
        };
        if downed || health.is_dead() {
            continue;
        }
        let world = owner_tf.mul_transform(*local);
        let max = best.as_ref().map_or(range, |b| b.distance);
        if let Some((distance, normal)) = hitbox_ray(collider, &world, origin, dir.as_vec3(), max)
            && distance < max
        {
            best = Some(RayHit {
                entity: hitbox.owner,
                kind: HitKind::Character { head: hitbox.head },
                distance,
                normal,
            });
        }
    }
    best
}

/// Ray test against one hitbox collider placed at `pose`: analytic for balls and
/// capsules (the hitbox shapes), avian's cast for anything else.
fn hitbox_ray(
    collider: &Collider,
    pose: &Transform,
    origin: Vec3,
    dir: Vec3,
    max: f32,
) -> Option<(f32, Vec3)> {
    let shape = collider.shape_scaled();
    if let Some(ball) = shape.as_ball() {
        return ray_sphere(origin, dir, pose.translation, ball.radius, max);
    }
    if let Some(capsule) = shape.as_capsule() {
        let a = pose.translation + pose.rotation * capsule.segment.a;
        let b = pose.translation + pose.rotation * capsule.segment.b;
        return ray_capsule(origin, dir, a, b, capsule.radius, max);
    }
    collider.cast_ray(pose.translation, pose.rotation, origin, dir, max, true)
}

/// First intersection of a ray (unit `dir`) with a solid sphere within `max`:
/// `(distance, outward normal)`. A ray starting inside hits at distance 0.
pub fn ray_sphere(
    origin: Vec3,
    dir: Vec3,
    center: Vec3,
    radius: f32,
    max: f32,
) -> Option<(f32, Vec3)> {
    let m = origin - center;
    let c = m.length_squared() - radius * radius;
    if c <= 0.0 {
        return Some((0.0, -dir));
    }
    let b = m.dot(dir);
    if b > 0.0 {
        return None;
    }
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let t = -b - disc.sqrt();
    if t > max {
        return None;
    }
    let normal = (origin + dir * t - center).normalize_or(-dir);
    Some((t.max(0.0), normal))
}

/// First intersection of a ray (unit `dir`) with a solid capsule (segment `a`–`b`,
/// `radius`) within `max`: `(distance, outward normal)`. A ray starting inside
/// hits at distance 0.
pub fn ray_capsule(
    origin: Vec3,
    dir: Vec3,
    a: Vec3,
    b: Vec3,
    radius: f32,
    max: f32,
) -> Option<(f32, Vec3)> {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 <= 1e-12 {
        return ray_sphere(origin, dir, a, radius, max);
    }
    let ao = origin - a;
    let closest = a + ab * (ao.dot(ab) / len2).clamp(0.0, 1.0);
    if origin.distance_squared(closest) <= radius * radius {
        return Some((0.0, -dir));
    }
    let axis = ab / len2.sqrt();
    let len = len2.sqrt();
    let mut best: Option<(f32, Vec3)> = None;

    // Side of the cylinder between the caps.
    let d_perp = dir - axis * dir.dot(axis);
    let o_perp = ao - axis * ao.dot(axis);
    let qa = d_perp.length_squared();
    if qa > 1e-12 {
        let qb = o_perp.dot(d_perp);
        let qc = o_perp.length_squared() - radius * radius;
        let disc = qb * qb - qa * qc;
        if disc >= 0.0 {
            let t = (-qb - disc.sqrt()) / qa;
            if (0.0..=max).contains(&t) {
                let p = origin + dir * t;
                let s = (p - a).dot(axis);
                if (0.0..=len).contains(&s) {
                    best = Some((t, (p - (a + axis * s)).normalize_or(-dir)));
                }
            }
        }
    }
    // Hemispherical caps.
    for center in [a, b] {
        if let Some((t, n)) = ray_sphere(origin, dir, center, radius, max)
            && best.is_none_or(|(bt, _)| t < bt)
        {
            best = Some((t, n));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analytic_capsule_matches_geometry() {
        let (a, b, r) = (Vec3::new(0.0, 0.38, 0.0), Vec3::new(0.0, 1.12, 0.0), 0.33);
        // Straight through the side.
        let hit = ray_capsule(Vec3::new(0.0, 0.75, 10.0), Vec3::NEG_Z, a, b, r, 50.0);
        let (t, n) = hit.unwrap();
        assert!((t - (10.0 - r)).abs() < 1e-5 && n.abs_diff_eq(Vec3::Z, 1e-5));
        // Down onto the top cap.
        let (t, n) = ray_capsule(Vec3::new(0.0, 5.0, 0.0), Vec3::NEG_Y, a, b, r, 50.0).unwrap();
        assert!((t - (5.0 - 1.12 - r)).abs() < 1e-5 && n.abs_diff_eq(Vec3::Y, 1e-5));
        // Just past the side misses; out of range misses; inside hits at 0.
        assert!(ray_capsule(Vec3::new(0.34, 0.75, 10.0), Vec3::NEG_Z, a, b, r, 50.0).is_none());
        assert!(ray_capsule(Vec3::new(0.0, 0.75, 10.0), Vec3::NEG_Z, a, b, r, 9.0).is_none());
        assert_eq!(
            ray_capsule(Vec3::new(0.0, 0.75, 0.1), Vec3::NEG_Z, a, b, r, 50.0).map(|h| h.0),
            Some(0.0)
        );
    }

    #[test]
    fn analytic_capsule_never_misses_rays_through_its_axis() {
        // parry's GJK capsule cast misses a few of these; the analytic test must not.
        let origin = Vec3::new(2.0, 1.62, 14.0);
        for i in 0..2000 {
            let x = -8.0 + i as f32 * 0.009;
            let (a, b) = (Vec3::new(x, 0.38, 0.0), Vec3::new(x, 1.12, 0.0));
            for h in [0.6, 0.8, 1.0, 1.1, 1.2] {
                let dir = (Vec3::new(x, h, 0.0) - origin).normalize();
                assert!(
                    ray_capsule(origin, dir, a, b, 0.33, 60.0).is_some(),
                    "x {x} h {h}"
                );
            }
        }
    }
}
