//! The fairness suite (docs/M3-SPEC.md → Fairness, D76; gate W3 in
//! docs/M3-GOAL.md): seeded Waves runs at waves 1–10 with up to eight knights,
//! against a scripted stand-in player who strafes in the open, boxes up, ramps
//! up and holds height, peeks out of cover and runs across the island, firing
//! back with real aim so the waves progress. Everything the player does goes
//! through `PlayerIntent`.
//!
//! An auditor watches every tick from outside the game's own code (its own
//! rays, its own view cone, its own orb tracking) and counts violations of
//! each rule:
//!
//! 1. no orb is released without line of sight from the wand tip to the
//!    player (head or chest), or to the piece it targets, both at its wind-up
//!    start and at its release;
//! 2. no first wind-up at the player sooner than the knight's reaction delay
//!    after it gained line of sight (eye to head or chest);
//! 3. never more than 3 attack-token holders, or winding wands, at once;
//! 4. every wind-up by a knight outside the player's view cone warns (the
//!    wand sound logic runs headless and logs whom it warned about);
//! 5. every hit on the player raises a damage arrow on its tick;
//! 6. no orb passes through a piece: each tick's centre segment of every orb
//!    crosses no piece collider (except the one it stops on);
//! 7. no travelling knight makes less than 0.5 m of progress for over 3 s, and
//!    `GruntNavStats::stuck_events` stays 0;
//! 8. **unfair damage**: an orb hitting the player that broke rule 1, 4 or 6.
//!    Checked on every damaging orb, not only killing ones: the stand-in can't
//!    die (so the later waves are reached), and the would-be deaths of a
//!    normal 100 + 100 player are counted too.
//!
//! `cargo test --test fairness -- --nocapture` prints the summary the W3
//! evidence quotes.

use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use bevy::{ecs::system::SystemState, platform::collections::HashMap, prelude::*};
use pieced::{
    audio::wand::{KNIGHT_CHEST, WandWarningLog, WandWarningTracking, WandWarnings, in_view},
    building::Piece,
    combat::{Downed, Loadout},
    dummy::look_toward,
    grunt::{
        AttackTokens, Grunt, GruntBrain, GruntNavStats, GruntStats, Parked, ShotTarget,
        brain::{ARRIVE_RADIUS, STUCK_DISTANCE, STUCK_SECONDS, sight},
    },
    hud::damage_arrow::{DamageArrowTracking, DamageArrows},
    orb::{Orb, OrbHit, OrbImpact, Wand, wand_tip},
    rng::Rng,
    shared::{
        ActiveTool, DamageDealt, DamageTarget, EyeHeight, GameCue, Health, Layer, LookAngles,
        PieceKind, Player, PlayerIntent, SimSet, SimTick, TICK_SECONDS, WeaponKind,
    },
    sim::Sim,
    tuning::Tuning,
    waves::{Run, RunPhase},
};
use std::{
    f32::consts::{FRAC_PI_2, PI, TAU},
    sync::atomic::{AtomicUsize, Ordering},
};

/// The view the auditor judges "on screen" by: the game's default vertical
/// FOV (70°) at Jake's 16:10 display, to the very edge of the frame (the
/// warning logic itself counts the outer 10% as off-screen, so it is stricter).
const VIEW_ASPECT: f32 = 1.6;
/// Line of sight counts as lost after this many ticks without it: the grunt
/// perceives at 30 Hz, so a one-tick flicker is never "losing" it.
const LOS_DEBOUNCE: u8 = 2;
/// The auditor's LOS runs a tick apart from the brain's 30 Hz perception.
const REACTION_SLACK_TICKS: u64 = 1;

fn ticks(seconds: f32) -> u64 {
    (seconds / TICK_SECONDS).round().max(0.0) as u64
}

// ---------------------------------------------------------------------------
// The audit
// ---------------------------------------------------------------------------

/// What the suite counts. Every `r*` field is a rule violation and must be 0.
#[derive(Debug, Clone, Default)]
struct Counts {
    runs: u32,
    waves_cleared: u32,
    sim_seconds: f32,
    kills: u32,
    knights_landed: u64,
    max_alive: u32,
    windups: u64,
    windups_at_player: u64,
    windups_at_pieces: u64,
    first_windups_checked: u64,
    offscreen_windups: u64,
    warnings_heard: u64,
    orbs_fired: u64,
    orb_segments: u64,
    orbs_on_pieces: u64,
    hits: u64,
    damage_events: u64,
    would_be_deaths: u64,
    max_tokens: usize,
    max_winding: usize,
    /// Info: released orbs whose wand tip was blocked while the eye saw the
    /// player (the orb then stops on the blocker).
    tip_blocked_eye_clear: u64,
    /// Info: wind-ups that started without line of sight from the wand tip
    /// (rule 1 is judged on the orbs they release; a cancelled one releases none).
    windups_without_los: u64,
    /// Info: orbs released on the tick their knight went down.
    orbs_from_downed: u64,
    /// Orbs, wind-ups or hits the auditor couldn't trace (must be 0: every
    /// orb is checked).
    untracked: u64,
    // Violations.
    r1_no_los: u64,
    r2_early_first_windup: u64,
    r3_too_many_shooters: u64,
    r4_unwarned_offscreen: u64,
    r5_no_arrow: u64,
    r6_through_piece: u64,
    r7_stuck: u64,
    r7_stuck_events: u64,
    r8_unfair_hits: u64,
    unfair_deaths: u64,
    examples: Vec<String>,
}

impl Counts {
    fn violations(&self) -> u64 {
        self.r1_no_los
            + self.r2_early_first_windup
            + self.r3_too_many_shooters
            + self.r4_unwarned_offscreen
            + self.r5_no_arrow
            + self.r6_through_piece
            + self.r7_stuck
            + self.r7_stuck_events
            + self.r8_unfair_hits
            + self.unfair_deaths
            + self.untracked
    }

    fn example(&mut self, text: String) {
        if self.examples.len() < 12 {
            self.examples.push(text);
        }
    }

    fn merge(&mut self, o: &Counts) {
        self.runs += o.runs;
        self.waves_cleared += o.waves_cleared;
        self.sim_seconds += o.sim_seconds;
        self.kills += o.kills;
        self.knights_landed += o.knights_landed;
        self.max_alive = self.max_alive.max(o.max_alive);
        self.windups += o.windups;
        self.windups_at_player += o.windups_at_player;
        self.windups_at_pieces += o.windups_at_pieces;
        self.first_windups_checked += o.first_windups_checked;
        self.offscreen_windups += o.offscreen_windups;
        self.warnings_heard += o.warnings_heard;
        self.orbs_fired += o.orbs_fired;
        self.orb_segments += o.orb_segments;
        self.orbs_on_pieces += o.orbs_on_pieces;
        self.hits += o.hits;
        self.damage_events += o.damage_events;
        self.would_be_deaths += o.would_be_deaths;
        self.max_tokens = self.max_tokens.max(o.max_tokens);
        self.max_winding = self.max_winding.max(o.max_winding);
        self.tip_blocked_eye_clear += o.tip_blocked_eye_clear;
        self.orbs_from_downed += o.orbs_from_downed;
        self.windups_without_los += o.windups_without_los;
        self.untracked += o.untracked;
        self.r1_no_los += o.r1_no_los;
        self.r2_early_first_windup += o.r2_early_first_windup;
        self.r3_too_many_shooters += o.r3_too_many_shooters;
        self.r4_unwarned_offscreen += o.r4_unwarned_offscreen;
        self.r5_no_arrow += o.r5_no_arrow;
        self.r6_through_piece += o.r6_through_piece;
        self.r7_stuck += o.r7_stuck;
        self.r7_stuck_events += o.r7_stuck_events;
        self.r8_unfair_hits += o.r8_unfair_hits;
        self.unfair_deaths += o.unfair_deaths;
        for e in &o.examples {
            self.example(e.clone());
        }
    }
}

/// A wind-up as the auditor saw it start.
#[derive(Debug, Clone, Copy)]
struct Windup {
    tick: u64,
    los: bool,
    offscreen: bool,
    warned: bool,
}

#[derive(Debug, Clone, Default)]
struct KnightAudit {
    /// Tick the eye's line of sight to the player was gained (debounced).
    los_since: Option<u64>,
    los_miss: u8,
    /// No wind-up at the player yet since `los_since`.
    first_pending: bool,
    windup: Option<Windup>,
    /// Stuck watch: where and when it last made 0.5 m of progress.
    anchor: Option<(Vec3, u64)>,
}

/// An orb as the auditor tracks it (by pool slot and launch tick).
#[derive(Debug, Clone, Copy)]
struct OrbAudit {
    launched: u64,
    shooter: Entity,
    /// Rules 1 and 4 held for its wind-up and release.
    los_ok: bool,
    warned_ok: bool,
    through: bool,
    last: Vec3,
}

/// A release this tick: the orb's audit before it has a slot, and where its
/// launch sweep starts (the knight's axis at wand height).
#[derive(Debug, Clone, Copy)]
struct Release {
    los_ok: bool,
    warned_ok: bool,
    axis: Vec3,
}

#[derive(Resource, Default)]
struct Audit {
    c: Counts,
    knights: HashMap<Entity, KnightAudit>,
    orbs: HashMap<Entity, OrbAudit>,
    /// Knights whose wind-up this frame was off-screen: the warning must be
    /// logged by the end of the frame.
    pending_offscreen: Vec<Entity>,
    /// A normal player's health, for the would-be deaths (refilled on each).
    shadow: Health,
}

type Knights<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Transform,
        &'static EyeHeight,
        &'static LookAngles,
        &'static GruntBrain,
        &'static GruntStats,
        &'static Wand,
        Has<Downed>,
    ),
    (With<Grunt>, Without<Parked>),
>;

fn filter_all() -> SpatialQueryFilter {
    SpatialQueryFilter::from_mask([Layer::World, Layer::Piece])
}

/// The first piece the segment `a`→`b` crosses (a ray down the orb's centre).
fn piece_on_segment(spatial: &SpatialQuery, a: Vec3, b: Vec3) -> Option<Entity> {
    let d = b - a;
    let len = d.length();
    let dir = Dir3::new(d).ok()?;
    spatial
        .cast_ray(
            a,
            dir,
            len,
            true,
            &SpatialQueryFilter::from_mask(Layer::Piece),
        )
        .map(|h| h.entity)
}

/// Rule 1: line of sight from the wand tip to what the shot targets.
fn wand_sees(
    spatial: &SpatialQuery,
    is_piece: &dyn Fn(Entity) -> bool,
    tip: Vec3,
    target: Option<ShotTarget>,
    head: Vec3,
    chest: Vec3,
) -> bool {
    match target {
        Some(ShotTarget::Piece { point, .. }) => {
            let d = point - tip;
            let Ok(dir) = Dir3::new(d) else {
                return false;
            };
            spatial
                .cast_ray(tip, dir, d.length() + 0.3, true, &filter_all())
                .is_some_and(|h| is_piece(h.entity))
        }
        _ => sight(spatial, tip, head, chest, is_piece).visible,
    }
}

#[allow(clippy::too_many_arguments)]
fn audit_fixed(
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    tokens: Res<AttackTokens>,
    nav: Res<GruntNavStats>,
    spatial: SpatialQuery,
    pieces: Query<(), With<Piece>>,
    player: Single<(Entity, &Transform, &EyeHeight, &LookAngles), With<Player>>,
    knights: Knights,
    orbs: Query<(Entity, &Orb, &Transform)>,
    mut cues: MessageReader<GameCue>,
    mut impacts: MessageReader<OrbImpact>,
    mut audit: ResMut<Audit>,
) {
    let now = tick.0;
    let audit = &mut *audit;
    let is_piece = |e: Entity| pieces.contains(e);
    let (me, player_tf, player_eye, player_look) = *player;
    let feet = player_tf.translation;
    let eye = feet + Vec3::Y * player_eye.0;
    let head = eye;
    let chest = feet + Vec3::Y * (player_eye.0 * 0.7);
    let vfov = tuning.look.fov_deg.to_radians();

    let mut windups = Vec::new();
    let mut fired = Vec::new();
    for cue in cues.read() {
        match *cue {
            GameCue::WandWindup { who } if who != me => windups.push(who),
            GameCue::OrbFired { who } if who != me => fired.push(who),
            _ => {}
        }
    }

    // Knights back in the pool forget their audit (the pool reuses them). A
    // knight downed this tick is still looked up: his wand may release on
    // the tick he goes down.
    let landed: Vec<Entity> = knights.iter().map(|k| k.0).collect();
    audit.knights.retain(|e, _| landed.contains(e));
    let alive = knights.iter().filter(|k| !k.7).count() as u32;
    audit.c.max_alive = audit.c.max_alive.max(alive);

    // Line of sight (rule 2) and progress (rule 7), every tick.
    for (entity, tf, keye, _, brain, _, _, downed) in &knights {
        if downed {
            continue;
        }
        let k = audit.knights.entry(entity).or_insert_with(|| {
            audit.c.knights_landed += 1;
            KnightAudit::default()
        });
        let from = tf.translation + Vec3::Y * keye.0;
        if sight(&spatial, from, head, chest, is_piece).visible {
            k.los_miss = 0;
            if k.los_since.is_none() {
                k.los_since = Some(now);
                k.first_pending = true;
            }
        } else {
            k.los_miss = k.los_miss.saturating_add(1);
            if k.los_miss >= LOS_DEBOUNCE {
                k.los_since = None;
            }
        }
        let at = tf.translation;
        let goal_far = brain
            .spot()
            .is_some_and(|s| s.pos.distance(at) > ARRIVE_RADIUS);
        if brain.is_travelling() && goal_far {
            match k.anchor {
                Some((a, since)) if a.distance(at) < STUCK_DISTANCE => {
                    if now.saturating_sub(since) > ticks(STUCK_SECONDS) {
                        audit.c.r7_stuck += 1;
                        audit.c.example(format!(
                            "r7 stuck: knight {entity} at {at:.1} since tick {since} (now {now})"
                        ));
                        k.anchor = Some((at, now));
                    }
                }
                _ => k.anchor = Some((at, now)),
            }
        } else {
            k.anchor = None;
        }
    }

    // Wind-up starts: rules 1 (at the start), 2 and 4.
    for who in windups {
        audit.c.windups += 1;
        let Ok((_, tf, keye, look, brain, stats, _, _)) = knights.get(who) else {
            audit.c.untracked += 1;
            audit
                .c
                .example(format!("windup by an unknown knight {who} at {now}"));
            continue;
        };
        let target = brain.shot_target();
        let tip = wand_tip(tf.translation, look.yaw);
        let los = wand_sees(&spatial, &is_piece, tip, target, head, chest);
        let chest_k = tf.translation + Vec3::Y * KNIGHT_CHEST;
        let offscreen = !in_view(eye, player_look.rotation(), vfov, VIEW_ASPECT, chest_k, 1.0);
        let k = audit.knights.entry(who).or_default();
        match target {
            Some(ShotTarget::Player) => {
                audit.c.windups_at_player += 1;
                if std::mem::take(&mut k.first_pending) {
                    audit.c.first_windups_checked += 1;
                    let reaction = ticks(stats.reaction);
                    let early = k
                        .los_since
                        .is_none_or(|since| now < since + reaction - REACTION_SLACK_TICKS);
                    if early {
                        audit.c.r2_early_first_windup += 1;
                        audit.c.example(format!(
                            "r2 early: knight {who} wound up at {now}, LOS since {:?}, reaction {reaction} ticks",
                            k.los_since
                        ));
                    }
                }
            }
            Some(ShotTarget::Piece { .. }) => audit.c.windups_at_pieces += 1,
            None => audit
                .c
                .example(format!("windup with no shot by {who} at {now}")),
        }
        if !los {
            audit.c.windups_without_los += 1;
            let eye_k = tf.translation + Vec3::Y * keye.0;
            let eye_sees = sight(&spatial, eye_k, head, chest, is_piece).visible;
            audit.c.example(format!(
                "windup without wand LOS: knight {who} at {now}, target {target:?}, eye sees {eye_sees}"
            ));
        }
        k.windup = Some(Windup {
            tick: now,
            los,
            offscreen,
            warned: false,
        });
        if offscreen {
            audit.c.offscreen_windups += 1;
            audit.pending_offscreen.push(who);
        }
    }

    // Releases: rule 1 at release, with the wind-up's rules 1 and 4.
    let mut releases: Vec<(Entity, Release)> = Vec::new();
    for who in fired {
        audit.c.orbs_fired += 1;
        let Ok((_, tf, keye, look, brain, _, _, downed)) = knights.get(who) else {
            audit.c.untracked += 1;
            audit
                .c
                .example(format!("an orb from an unknown knight {who} at {now}"));
            continue;
        };
        audit.c.orbs_from_downed += u64::from(downed);
        let target = brain.shot_target();
        let tip = wand_tip(tf.translation, look.yaw);
        let los_release = wand_sees(&spatial, &is_piece, tip, target, head, chest);
        let windup = audit.knights.get(&who).and_then(|k| k.windup);
        let los_windup = windup.is_some_and(|w| w.los);
        let warned_ok = windup.is_some_and(|w| !w.offscreen || w.warned);
        let los_ok = los_release && los_windup;
        if !los_ok {
            audit.c.r1_no_los += 1;
            let eye_k = tf.translation + Vec3::Y * keye.0;
            let eye_sees = sight(&spatial, eye_k, head, chest, is_piece).visible;
            if eye_sees && matches!(target, Some(ShotTarget::Player)) {
                audit.c.tip_blocked_eye_clear += 1;
            }
            audit.c.example(format!(
                "r1: orb from {who} at {now}: LOS at wind-up {los_windup} (tick {:?}), at release {los_release}, target {target:?}, eye sees {eye_sees}",
                windup.map(|w| w.tick)
            ));
        }
        let axis = Vec3::new(tf.translation.x, tip.y, tf.translation.z);
        releases.push((
            who,
            Release {
                los_ok,
                warned_ok,
                axis,
            },
        ));
    }
    let release_of = |who: Entity| releases.iter().find(|r| r.0 == who).map(|r| r.1);

    // Impacts: rule 6 on the last segment, then rule 8 for hits.
    let od = &tuning.orb;
    for impact in impacts.read() {
        let existing = audit
            .orbs
            .get(&impact.orb)
            .copied()
            .filter(|o| o.launched < now && o.shooter == impact.shooter);
        let mut orb = match existing {
            Some(o) => {
                audit.orbs.remove(&impact.orb);
                o
            }
            None => {
                let Some(r) = release_of(impact.shooter) else {
                    audit.c.untracked += 1;
                    audit.c.example(format!(
                        "impact from an untracked orb (shooter {}) at {now}",
                        impact.shooter
                    ));
                    continue;
                };
                OrbAudit {
                    launched: now,
                    shooter: impact.shooter,
                    los_ok: r.los_ok,
                    warned_ok: r.warned_ok,
                    through: false,
                    last: r.axis,
                }
            }
        };
        audit.c.orb_segments += 1;
        if let Some(crossed) = piece_on_segment(&spatial, orb.last, impact.point) {
            let stopped_on_it = matches!(impact.hit, OrbHit::Piece(_))
                && impact.target == Some(crossed)
                && orb.last.distance(impact.point) < 1e-3;
            if !stopped_on_it {
                orb.through = true;
                audit.c.r6_through_piece += 1;
                audit.c.example(format!(
                    "r6: orb from {} crossed piece {crossed} before stopping ({:?}) at {now}",
                    impact.shooter, impact.hit
                ));
            }
        }
        match impact.hit {
            OrbHit::Piece(_) => audit.c.orbs_on_pieces += 1,
            OrbHit::Player { head } if impact.target == Some(me) => {
                audit.c.hits += 1;
                let unfair = !orb.los_ok || !orb.warned_ok || orb.through;
                if unfair {
                    audit.c.r8_unfair_hits += 1;
                    audit.c.example(format!(
                        "r8 unfair hit from {} at {now}: los {} warned {} through {}",
                        orb.shooter, orb.los_ok, orb.warned_ok, orb.through
                    ));
                }
                let amount = od.damage * if head { od.headshot_multiplier } else { 1.0 };
                audit.shadow.apply(amount);
                if audit.shadow.is_dead() {
                    audit.c.would_be_deaths += 1;
                    if unfair {
                        audit.c.unfair_deaths += 1;
                    }
                    audit.shadow.reset();
                }
            }
            _ => {}
        }
    }

    // Orbs still flying: rule 6 on this tick's segment.
    for (slot, orb, tf) in &orbs {
        let pos = tf.translation;
        let tracked = audit
            .orbs
            .get(&slot)
            .copied()
            .filter(|o| o.launched == orb.launched && o.shooter == orb.shooter);
        let mut entry = match tracked {
            Some(o) => o,
            None if orb.launched == now => match release_of(orb.shooter) {
                Some(r) => OrbAudit {
                    launched: now,
                    shooter: orb.shooter,
                    los_ok: r.los_ok,
                    warned_ok: r.warned_ok,
                    through: false,
                    last: r.axis,
                },
                None => {
                    audit.c.untracked += 1;
                    audit
                        .c
                        .example(format!("orb launched at {now} without a release"));
                    continue;
                }
            },
            None => {
                audit.c.untracked += 1;
                audit.c.example(format!("untracked orb in flight at {now}"));
                continue;
            }
        };
        audit.c.orb_segments += 1;
        if !entry.through
            && let Some(crossed) = piece_on_segment(&spatial, entry.last, pos)
        {
            entry.through = true;
            audit.c.r6_through_piece += 1;
            audit.c.example(format!(
                "r6: orb from {} flew through piece {crossed} at {now}",
                entry.shooter
            ));
        }
        entry.last = pos;
        audit.orbs.insert(slot, entry);
    }
    let flying: Vec<Entity> = orbs.iter().map(|o| o.0).collect();
    audit.orbs.retain(|e, _| flying.contains(e));

    // Rule 3: tokens and winding wands.
    let winding = knights.iter().filter(|k| !k.7 && k.6.is_winding()).count();
    let holders = tokens.holders().len();
    let max = tuning.grunt.max_shooters as usize;
    audit.c.max_tokens = audit.c.max_tokens.max(holders);
    audit.c.max_winding = audit.c.max_winding.max(winding);
    if holders > max || winding > max {
        audit.c.r3_too_many_shooters += 1;
        audit.c.example(format!(
            "r3: {holders} token holders, {winding} winding at {now}"
        ));
    }
    audit.c.r7_stuck_events = nav.stuck_events as u64;
}

/// End of each frame: rule 4 (the warning was logged this frame) and rule 5
/// (an arrow for each hit, raised on its tick).
fn audit_last(
    mut damage: MessageReader<DamageDealt>,
    arrows: Res<DamageArrows>,
    player: Single<Entity, With<Player>>,
    grunts: Query<(), With<Grunt>>,
    warnings: Res<WandWarnings>,
    mut log: ResMut<WandWarningLog>,
    mut audit: ResMut<Audit>,
) {
    let me = *player;
    let audit = &mut *audit;
    for hit in damage.read() {
        if hit.target != me || hit.target_kind != DamageTarget::Character || hit.amount <= 0.0 {
            continue;
        }
        if !hit.source.is_some_and(|s| grunts.contains(s)) {
            continue;
        }
        audit.c.damage_events += 1;
        let raised = arrows
            .live()
            .any(|a| a.tick == hit.tick && a.source == hit.source);
        if !raised {
            audit.c.r5_no_arrow += 1;
            audit.c.example(format!(
                "r5: no arrow for the hit from {:?} at {}",
                hit.source, hit.tick
            ));
        }
    }
    for who in std::mem::take(&mut audit.pending_offscreen) {
        if log.0.contains(&who) {
            if let Some(w) = audit.knights.get_mut(&who).and_then(|k| k.windup.as_mut()) {
                w.warned = true;
            }
        } else {
            audit.c.r4_unwarned_offscreen += 1;
            audit
                .c
                .example(format!("r4: off-screen wind-up by {who} not warned"));
        }
    }
    log.0.clear();
    audit.c.warnings_heard = warnings.warned as u64;
}

// ---------------------------------------------------------------------------
// The stand-in player
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Behaviour {
    /// Strafes side to side in the open, firing bursts.
    Strafe,
    /// Builds a 1×1 box with a ramp inside, waits, climbs the ramp and holds
    /// the height, then drops off the far side.
    Box,
    /// Ramp-rushes toward the middle, holds the top firing down, walks off.
    Ramp,
    /// Puts up a wall toward the nearest knight and peeks out to shoot.
    Peek,
    /// Sprints across the island (knights behind him: off-screen wind-ups).
    Run,
}

impl Behaviour {
    const ALL: [Behaviour; 5] = [
        Behaviour::Strafe,
        Behaviour::Box,
        Behaviour::Peek,
        Behaviour::Ramp,
        Behaviour::Run,
    ];

    fn ticks(self) -> u64 {
        match self {
            Behaviour::Strafe => 300,
            Behaviour::Box => 360,
            Behaviour::Ramp => 360,
            Behaviour::Peek => 330,
            Behaviour::Run => 240,
        }
    }
}

type Eyes = SystemState<(
    SpatialQuery<'static, 'static>,
    Query<
        'static,
        'static,
        (Entity, &'static Transform),
        (With<Grunt>, Without<Parked>, Without<Downed>),
    >,
)>;

struct StandIn {
    rng: Rng,
    order: usize,
    behaviour: Behaviour,
    since: u64,
    yaw: f32,
    dest: Vec3,
    strafe: f32,
    /// Where he stood a second ago, to notice he's walled in.
    stall: (Vec3, u64),
    eyes: Eyes,
}

/// The shortest signed turn from `from` to `to` (radians).
fn turn(from: f32, to: f32) -> f32 {
    (to - from + PI).rem_euclid(TAU) - PI
}

fn snap_to_facing(yaw: f32) -> f32 {
    ((yaw / FRAC_PI_2).round() * FRAC_PI_2).rem_euclid(TAU)
}

impl StandIn {
    fn new(sim: &mut Sim, seed: u64) -> Self {
        let mut rng = Rng::new(seed ^ 0x57A2_D1A7);
        let order = (rng.next_u64() % Behaviour::ALL.len() as u64) as usize;
        Self {
            rng,
            order,
            behaviour: Behaviour::Strafe,
            since: sim.sim_tick(),
            yaw: 0.0,
            dest: Vec3::ZERO,
            strafe: 1.0,
            stall: (Vec3::ZERO, 0),
            eyes: SystemState::new(sim.world_mut()),
        }
    }

    /// The nearest knight in play (any), and the nearest he can see.
    fn knights(&mut self, sim: &Sim, eye: Vec3) -> (Option<Vec3>, Option<Vec3>) {
        let (spatial, knights) = self.eyes.get(sim.world()).expect("spatial query");
        let mut all: Vec<Vec3> = knights.iter().map(|(_, t)| t.translation).collect();
        all.sort_by(|a, b| a.distance(eye).total_cmp(&b.distance(eye)));
        let visible = all.iter().copied().find(|k| {
            let chest = *k + Vec3::Y * 1.1;
            let d = chest - eye;
            Dir3::new(d).is_ok_and(|dir| {
                spatial
                    .cast_ray(eye, dir, d.length(), true, &filter_all())
                    .is_none()
            })
        });
        (all.first().copied(), visible)
    }

    fn start(&mut self, b: Behaviour, now: u64, feet: Vec3, look: LookAngles, near: Option<Vec3>) {
        self.behaviour = b;
        self.since = now;
        self.strafe = if self.rng.chance(0.5) { 1.0 } else { -1.0 };
        match b {
            Behaviour::Box => self.yaw = snap_to_facing(look.yaw),
            Behaviour::Ramp => self.yaw = snap_to_facing(look_toward(-feet.with_y(0.0)).yaw),
            Behaviour::Peek => {
                let toward = near.map_or(-feet, |k| k - feet).with_y(0.0);
                self.yaw = snap_to_facing(look_toward(toward).yaw);
            }
            Behaviour::Run => {
                let across = -feet.with_y(0.0);
                self.dest = if across.length() > 6.0 {
                    (across * 0.8).clamp(Vec3::splat(-17.0), Vec3::splat(17.0))
                } else {
                    let a = self.rng.range(0.0, TAU);
                    Vec3::new(a.cos() * 15.0, 0.0, a.sin() * 15.0)
                };
                self.yaw = look_toward(self.dest - feet).yaw;
            }
            Behaviour::Strafe => {}
        }
    }

    /// Writes this tick's intent.
    fn drive(&mut self, sim: &mut Sim) {
        let now = sim.sim_tick();
        let p = sim.player();
        let feet = sim.feet(p);
        let eye = feet + Vec3::Y * sim.get::<EyeHeight>(p).0;
        let look = *sim.get::<LookAngles>(p);
        let (near, visible) = self.knights(sim, eye);
        if now >= self.since + self.behaviour.ticks() {
            self.order += 1;
            let next = Behaviour::ALL[self.order % Behaviour::ALL.len()];
            self.start(next, now, feet, look, near);
        }
        let k = now - self.since;
        let tool = *sim.get::<ActiveTool>(p);
        let ammo = sim.get::<Loadout>(p).rifle.ammo;
        let mut i = PlayerIntent::default();
        let rifle = ActiveTool::Weapon(WeaponKind::Rifle);
        let face = |yaw: f32, pitch: f32| Vec2::new(turn(look.yaw, yaw), pitch - look.pitch);
        // Aim at the nearest knight in sight and fire in bursts (0.5 s on, 0.5 s off).
        let engage = |i: &mut PlayerIntent| {
            if tool != rifle {
                i.select = Some(rifle);
            }
            if let Some(knight) = visible {
                let want = look_toward(knight + Vec3::Y * 1.0 - eye);
                i.look_delta = face(want.yaw, want.pitch);
                i.fire = k % 60 < 30;
            }
            if ammo == 0 {
                i.reload_pressed = true;
            }
        };
        match self.behaviour {
            Behaviour::Strafe => {
                if k.is_multiple_of(36) && k > 0 {
                    self.strafe = -self.strafe;
                }
                engage(&mut i);
                i.move_axis = Vec2::new(self.strafe, 0.0);
            }
            Behaviour::Box => match k {
                0 => {
                    i.select = Some(ActiveTool::Build(PieceKind::Wall));
                    i.look_delta = face(self.yaw, 0.0);
                }
                3 | 7 | 11 | 15 | 20 => i.fire_pressed = true,
                5 | 9 | 13 => i.look_delta = Vec2::new(-FRAC_PI_2, 0.0),
                17 => {
                    i.select = Some(ActiveTool::Build(PieceKind::Ramp));
                    i.look_delta = face(self.yaw, (-60f32).to_radians());
                }
                22 => {
                    i.select = Some(rifle);
                    i.look_delta = face(self.yaw, 0.0);
                }
                1..150 => {}
                150..200 | 300.. => {
                    i.look_delta = face(self.yaw, 0.0);
                    i.move_axis = Vec2::Y;
                }
                _ => engage(&mut i),
            },
            Behaviour::Ramp => match k {
                0 => {
                    i.select = Some(ActiveTool::Build(PieceKind::Ramp));
                    i.look_delta = face(self.yaw, 8f32.to_radians());
                }
                1..150 => {
                    i.fire = true;
                    i.move_axis = Vec2::Y;
                    i.sprint = true;
                }
                300.. => {
                    i.look_delta = face(self.yaw, 0.0);
                    i.move_axis = Vec2::Y;
                }
                _ => engage(&mut i),
            },
            Behaviour::Peek => match k {
                0 => {
                    i.select = Some(ActiveTool::Build(PieceKind::Wall));
                    i.look_delta = face(self.yaw, 0.0);
                }
                3 => i.fire_pressed = true,
                5 => i.select = Some(rifle),
                1..6 => {}
                _ => {
                    let c = (k - 6) % 96;
                    if (24..60).contains(&c) {
                        engage(&mut i);
                    } else {
                        i.look_delta = face(self.yaw, 0.0);
                        i.move_axis = match c {
                            0..24 => Vec2::X,
                            60..84 => -Vec2::X,
                            _ => Vec2::ZERO,
                        };
                    }
                }
            },
            Behaviour::Run => {
                if tool != rifle {
                    i.select = Some(rifle);
                }
                let to = self.dest - feet;
                if to.xz().length() > 1.5 {
                    i.look_delta = face(look_toward(to.with_y(0.0)).yaw, 0.0);
                    i.move_axis = Vec2::Y;
                    i.sprint = true;
                    // Walled in (his own box, a knight's corner): shoot his way out.
                    if now >= self.stall.1 + 60 && feet.distance(self.stall.0) < 0.3 {
                        i.fire = true;
                    }
                }
            }
        }
        if now >= self.stall.1 + 60 {
            self.stall = (feet, now);
        }
        *sim.player_intent() = i;
    }
}

// ---------------------------------------------------------------------------
// Runs
// ---------------------------------------------------------------------------

/// One seeded run starting at `wave`, until that wave is cleared or
/// `max_seconds` of simulated play.
fn fairness_run(seed: u64, wave: u32, max_seconds: f32) -> Counts {
    let mut sim = Sim::waves(seed);
    let tick = sim.sim_tick();
    let waves = sim.world().resource::<Tuning>().waves.clone();
    sim.world_mut()
        .resource_mut::<Run>()
        .start_at_wave(wave, tick, &waves);
    // The stand-in can't die, so every wave is reached; the audit keeps a
    // normal player's health on the side for the would-be deaths.
    let p = sim.player();
    let mut health = Health::full(1.0e9, 100.0);
    health.shield = 100.0;
    *sim.world_mut().get_mut::<Health>(p).unwrap() = health;
    DamageArrowTracking::install(&mut sim.app);
    WandWarningTracking::install(&mut sim.app);
    let max_hp = sim.world().resource::<Tuning>().combat.max_hp;
    let max_shield = sim.world().resource::<Tuning>().combat.max_shield;
    sim.app
        .insert_resource(Audit {
            shadow: Health::full(max_hp, max_shield),
            ..default()
        })
        .add_systems(
            FixedUpdate,
            audit_fixed.after(SimSet::Combat).before(SimSet::Resolve),
        )
        .add_systems(Last, audit_last);
    let mut player = StandIn::new(&mut sim, seed);
    let start = sim.sim_tick();
    let limit = ticks(max_seconds);
    let mut cleared = false;
    while sim.sim_tick() - start < limit {
        let run = sim.world().resource::<Run>();
        if run.wave > wave || matches!(run.phase, RunPhase::Break { .. }) {
            cleared = true;
            break;
        }
        assert!(!run.is_ended(), "seed {seed}: the stand-in can't die");
        player.drive(&mut sim);
        sim.tick();
    }
    let run = sim.world().resource::<Run>().clone();
    let mut c = sim.world_mut().resource_mut::<Audit>().c.clone();
    c.runs = 1;
    c.waves_cleared = u32::from(cleared);
    c.kills = run.eliminations;
    c.sim_seconds = (sim.sim_tick() - start) as f32 * TICK_SECONDS;
    println!(
        "  seed {seed:>2} wave {wave:>2}: {} in {:>5.1} s, {:>2} kills, {:>4} orbs, {:>3} hits, {} would-be deaths, {} violations",
        if cleared { "cleared" } else { "timed out" },
        c.sim_seconds,
        c.kills,
        c.orbs_fired,
        c.hits,
        c.would_be_deaths,
        c.violations(),
    );
    c
}

/// Runs `jobs` (seed, wave) on up to four threads and merges their counts.
fn run_suite(jobs: &[(u64, u32)], max_seconds: f32) -> Counts {
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism()
        .map_or(2, |n| n.get())
        .clamp(1, 4);
    let results: Vec<Counts> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let n = next.fetch_add(1, Ordering::SeqCst);
                        let Some(&(seed, wave)) = jobs.get(n) else {
                            break;
                        };
                        mine.push(fairness_run(seed, wave, max_seconds));
                    }
                    mine
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("a fairness run panicked"))
            .collect()
    });
    let mut total = Counts::default();
    for c in &results {
        total.merge(c);
    }
    total
}

fn print_summary(title: &str, c: &Counts) {
    println!("{title}");
    println!(
        "  runs {} (waves cleared {}), {:.0} s simulated, {} knights landed (max {} at once), {} kills",
        c.runs, c.waves_cleared, c.sim_seconds, c.knights_landed, c.max_alive, c.kills
    );
    println!(
        "  wind-ups {} (at the player {}, at pieces {}; first wind-ups checked {}), off-screen {} (warnings logged {})",
        c.windups,
        c.windups_at_player,
        c.windups_at_pieces,
        c.first_windups_checked,
        c.offscreen_windups,
        c.warnings_heard
    );
    println!(
        "  orbs fired {}, on pieces {}, segments checked {}, hits on the player {} (damage events {}), would-be deaths {}",
        c.orbs_fired, c.orbs_on_pieces, c.orb_segments, c.hits, c.damage_events, c.would_be_deaths
    );
    println!(
        "  peak token holders {}, peak winding wands {}; wind-ups without wand LOS {}; tip blocked while the eye saw {}; orbs from knights downed that tick {}; untraced {}",
        c.max_tokens,
        c.max_winding,
        c.windups_without_los,
        c.tip_blocked_eye_clear,
        c.orbs_from_downed,
        c.untracked
    );
    println!(
        "  violations: 1 no-LOS {} | 2 early first wind-up {} | 3 >3 shooters {} | 4 unwarned off-screen {} | 5 no arrow {} | 6 through a piece {} | 7 stuck {} (stuck events {}) | 8 unfair hits {}, unfair deaths {}",
        c.r1_no_los,
        c.r2_early_first_windup,
        c.r3_too_many_shooters,
        c.r4_unwarned_offscreen,
        c.r5_no_arrow,
        c.r6_through_piece,
        c.r7_stuck,
        c.r7_stuck_events,
        c.r8_unfair_hits,
        c.unfair_deaths
    );
    for e in &c.examples {
        println!("    {e}");
    }
}

fn assert_fair(c: &Counts) {
    assert!(c.orbs_fired > 0 && c.hits > 0, "the knights fought: {c:?}");
    assert_eq!(c.hits, c.damage_events, "every orb hit is one damage event");
    assert_eq!(c.violations(), 0, "every rule holds: {c:#?}");
}

/// W3: 20 seeded runs, two at each of waves 1–10.
#[test]
fn twenty_seeded_waves_hold_every_fairness_rule() {
    let jobs: Vec<(u64, u32)> = (1..=20u64)
        .map(|s| (s, ((s - 1) % 10) as u32 + 1))
        .collect();
    let c = run_suite(&jobs, 150.0);
    print_summary("Fairness suite (W3): 20 seeded runs, waves 1-10", &c);
    assert_fair(&c);
    assert_eq!(c.runs, 20);
    assert_eq!(c.max_alive, 8, "the later waves fill the island");
}
