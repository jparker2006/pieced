//! The knights' personality in sound (M4 chunk 6, D123): voice barks and
//! spatial footsteps.
//!
//! **Barks** ([`voice`](super::voice) designs them):
//!
//! - a "hup!" as a knight lands off a drop ship's beam;
//! - a short taunt on some wind-ups: only a knight **on screen**, only on
//!   every [`TAUNT_EVERY`]th of his wind-ups (so at most 1 in 3), and at most
//!   one taunt every [`TAUNT_GAP`] seconds across all knights;
//! - a yelp when he's hit (four takes, pitch-jittered);
//! - a "hoo-HAH!" from the survivors as the victory hop starts;
//! - a "whaaaa…" as he's flung into the void (after the existing yip).
//!
//! Each knight has his own pitch ([`personal_pitch`]). The [`BarkGate`]
//! limits them: one knight barks at most every [`KNIGHT_GAP`] s, any two
//! barks start at least [`GLOBAL_GAP`] s apart, and at most [`MAX_BARKS`]
//! sound at once.
//!
//! **Fairness (D76).** The off-screen wind-up warning must stay clearly
//! audible, so a warning always wins: when one starts, every bark sounding
//! is cut and every bark still queued dropped, and no bark may start for
//! [`WARNING_GUARD`] s (longer than the warning itself). Taunts never come
//! from off-screen knights, whose wind-ups are the warning's.
//!
//! **Footsteps.** A knight's boots sound on the run clip's footfalls (the
//! animation's run phase crossing each multiple of π, see
//! [`KnightAnim::run_phase`]; without a figure, the same phase integrated
//! from his speed), panned and attenuated by where he is. The boot sounds
//! of grass, of wood on a floor or ramp, or of brick on a wall top (one
//! short ray down per step). Only the [`STEP_KNIGHTS`] nearest knights within
//! [`STEP_RANGE`] are heard, so a crowd never floods the mixer.
//!
//! Nothing here allocates per frame once the knights have been seen: the
//! gate is fixed-size and the per-knight state is a map that only grows
//! with the (pooled) knights.

use super::{PlayQueue, Sfx, SfxCategory, bank, play_queued, voice, wand};
use crate::{
    arena::visuals::TargetFigure,
    building::Piece,
    combat::Downed,
    grunt::{Grunt, Parked},
    knight::{KnightAnim, STRIDE},
    movement::Motor,
    render::{CurrentFov, MainCamera},
    shared::{
        DamageDealt, DamageTarget, EyeHeight, GameCue, Layer, LookAngles, PieceKind, Player,
        SimTick,
    },
    waves::{Run, ships::Beaming},
};
use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use bevy::{platform::collections::HashMap, prelude::*, window::PrimaryWindow};
use std::f32::consts::{PI, TAU};

/// Most barks sounding at once.
pub const MAX_BARKS: usize = 3;
/// Any two barks start at least this far apart (s).
pub const GLOBAL_GAP: f64 = 0.15;
/// One knight barks at most this often (s).
pub const KNIGHT_GAP: f64 = 1.0;
/// A knight taunts on at most every this-many-th wind-up of his.
pub const TAUNT_EVERY: u32 = 3;
/// At most one taunt this often across all knights (s).
pub const TAUNT_GAP: f64 = 3.0;
/// After an off-screen warning starts, no bark for this long (s): the
/// warning's own length (0.34 s) with a margin.
pub const WARNING_GUARD: f64 = 0.55;
/// The void's "whaaa" follows the yip (s).
pub const WHAAA_DELAY: f64 = 0.12;
/// Barks come from the knight's head (m above his feet).
pub const HEAD: f32 = 1.4;
/// Knights heard stepping: the nearest few within this range (m).
pub const STEP_KNIGHTS: usize = 3;
pub const STEP_RANGE: f32 = 26.0;
/// A knight is heard stepping above this speed (m/s).
pub const STEP_SPEED: f32 = 1.0;

/// A knight's bark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bark {
    Hup,
    Taunt,
    Yelp,
    HooHah,
    Whaaa,
}

impl Bark {
    pub const ALL: [Bark; 5] = [
        Bark::Hup,
        Bark::Taunt,
        Bark::Yelp,
        Bark::HooHah,
        Bark::Whaaa,
    ];

    pub fn sfx(self) -> Sfx {
        match self {
            Bark::Hup => Sfx::BarkHup,
            Bark::Taunt => Sfx::BarkTaunt,
            Bark::Yelp => Sfx::BarkYelp,
            Bark::HooHah => Sfx::BarkHooHah,
            Bark::Whaaa => Sfx::BarkWhaaa,
        }
    }

    /// How long it can sound (s): its design's budget plus its room.
    pub fn seconds(self) -> f64 {
        let sfx = self.sfx();
        f64::from(sfx.spec().max_seconds + sfx.room().tail)
    }
}

/// A knight's own pitch (playback speed, 0.9..1.12), fixed for the entity:
/// each knight sounds like himself.
pub fn personal_pitch(knight: Entity) -> f32 {
    let h = (knight.index_u32().wrapping_add(7)).wrapping_mul(2_654_435_761) >> 12;
    0.9 + 0.22 * ((h % 1000) as f32 / 999.0)
}

/// A yelp's extra pitch jitter for its `n`th play (±6%).
pub fn yelp_jitter(n: u32) -> f32 {
    let h = n.wrapping_mul(2_246_822_519).wrapping_add(0x9E37) >> 13;
    1.0 + 0.06 * ((h % 1001) as f32 / 500.0 - 1.0)
}

/// Whether a knight's `n`th wind-up (from 1) may taunt: every
/// [`TAUNT_EVERY`]th, three times in four (seeded by the knight).
pub fn taunt_turn(knight: Entity, n: u32) -> bool {
    n > 0
        && n.is_multiple_of(TAUNT_EVERY)
        && (knight.index_u32() ^ n).wrapping_mul(0x9E37_79B9) >> 30 != 0
}

/// A bark sounding (or cut).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarkVoice {
    pub knight: Entity,
    pub bark: Bark,
    pub start: f64,
    pub end: f64,
}

/// The barks' rate limits and voice cap, pure: fixed-size, no allocation.
#[derive(Debug, Clone, PartialEq)]
pub struct BarkGate {
    voices: [Option<BarkVoice>; MAX_BARKS],
    last_any: f64,
    last_taunt: f64,
    warning_until: f64,
}

impl Default for BarkGate {
    fn default() -> Self {
        Self {
            voices: [None; MAX_BARKS],
            last_any: f64::MIN,
            last_taunt: f64::MIN,
            warning_until: f64::MIN,
        }
    }
}

impl BarkGate {
    /// Barks sounding at `now`.
    pub fn sounding(&self, now: f64) -> usize {
        self.voices
            .iter()
            .flatten()
            .filter(|v| v.start <= now && now < v.end)
            .count()
    }

    /// Whether an off-screen warning is inside its guard at `now`.
    pub fn guarding(&self, now: f64) -> bool {
        now < self.warning_until
    }

    /// Asks for `bark` from `knight` (who last barked at `knight_last`) at
    /// `now`; on yes it is booked and sounds from `now` (plus `delay`).
    pub fn try_bark(
        &mut self,
        now: f64,
        knight: Entity,
        knight_last: f64,
        bark: Bark,
        delay: f64,
    ) -> Option<BarkVoice> {
        if self.guarding(now)
            || now - self.last_any < GLOBAL_GAP
            || now - knight_last < KNIGHT_GAP
            || (bark == Bark::Taunt && now - self.last_taunt < TAUNT_GAP)
        {
            return None;
        }
        let slot = self
            .voices
            .iter()
            .position(|v| v.is_none_or(|v| v.end <= now))?;
        let voice = BarkVoice {
            knight,
            bark,
            start: now + delay,
            end: now + delay + bark.seconds(),
        };
        self.voices[slot] = Some(voice);
        self.last_any = now;
        if bark == Bark::Taunt {
            self.last_taunt = now;
        }
        Some(voice)
    }

    /// An off-screen warning starts at `now`: every bark sounding or waiting
    /// is cut, and none may start for [`WARNING_GUARD`]. Calls `cut` for each
    /// bark cut short.
    pub fn warning(&mut self, now: f64, mut cut: impl FnMut(BarkVoice)) {
        self.warning_until = now + WARNING_GUARD;
        for slot in &mut self.voices {
            if let Some(v) = slot.take()
                && v.end > now
            {
                cut(BarkVoice {
                    end: v.end.min(now).max(v.start.min(now)),
                    ..v
                });
            }
        }
    }
}

/// What the barks remember about one knight.
#[derive(Debug, Clone, Copy, PartialEq)]
struct KnightVoice {
    last_bark: f64,
    windups: u32,
    beaming: bool,
    /// Run phase (radians) and the footfall it last sounded.
    phase: f32,
    speed: f32,
    yelps: u32,
}

impl Default for KnightVoice {
    fn default() -> Self {
        Self {
            last_bark: f64::MIN,
            windups: 0,
            beaming: false,
            phase: 0.0,
            speed: 0.0,
            yelps: 0,
        }
    }
}

/// The barks' state (the gate and each knight's memory).
#[derive(Resource, Debug, Default)]
pub struct KnightVoices {
    pub gate: BarkGate,
    knights: HashMap<Entity, KnightVoice>,
    hopping: bool,
    /// Survivors still to "hoo-hah" in this hop, nearest first.
    hop_queue: [Option<Entity>; MAX_BARKS],
    warned: u32,
    steps: u32,
}

/// Every bark decided and every step heard, for headless tests (the game
/// never adds it).
#[derive(Resource, Debug, Default, Clone)]
pub struct BarkLog {
    pub barks: Vec<BarkVoice>,
    /// Barks cut by a warning (with their cut end).
    pub cut: Vec<BarkVoice>,
    /// Off-screen warnings' start times.
    pub warnings: Vec<f64>,
    /// Knight steps: (time, knight, where, sound).
    pub steps: Vec<(f64, Entity, Vec3, Sfx)>,
}

/// The barks and steps without the sound, for headless tests.
pub struct KnightVoiceTracking;

impl KnightVoiceTracking {
    /// Adds the barks' logic and a [`BarkLog`] to a headless app (with the
    /// wand-sound logic it defers to, if the test hasn't added it).
    pub fn install(app: &mut App) {
        if !app.world().contains_resource::<wand::WandWarnings>() {
            wand::WandWarningTracking::install(app);
        }
        app.init_resource::<KnightVoices>()
            .init_resource::<BarkLog>()
            .add_systems(
                Update,
                queue_knight_voices
                    .after(wand::queue_wand_sounds)
                    .before(play_queued),
            );
    }
}

pub(super) fn build(app: &mut App) {
    app.init_resource::<KnightVoices>().add_systems(
        Update,
        queue_knight_voices
            .after(wand::queue_wand_sounds)
            .before(play_queued),
    );
}

/// The ears: the camera as it rendered last, else the player's eye.
struct Ears {
    eye: Vec3,
    rotation: Quat,
    vfov: f32,
    aspect: f32,
}

type KnightQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Transform,
        Option<&'static Motor>,
        Has<Beaming>,
        Has<Parked>,
        Has<Downed>,
    ),
    With<Grunt>,
>;

/// Decides the barks and steps this frame and queues them.
#[allow(clippy::too_many_arguments)]
pub(crate) fn queue_knight_voices(
    (real, time): (Res<Time<Real>>, Res<Time>),
    (fov, camera, window): (
        Option<Res<CurrentFov>>,
        Option<Single<&GlobalTransform, With<MainCamera>>>,
        Option<Single<&Window, With<PrimaryWindow>>>,
    ),
    player: Option<Single<(&Transform, &LookAngles, Option<&EyeHeight>), With<Player>>>,
    knights: KnightQuery,
    figures: Query<(&TargetFigure, &KnightAnim)>,
    pieces: Query<&Piece>,
    spatial: Option<SpatialQuery>,
    (run, tick, warnings): (
        Option<Res<Run>>,
        Option<Res<SimTick>>,
        Option<Res<wand::WandWarnings>>,
    ),
    mut damage: MessageReader<DamageDealt>,
    mut cues: MessageReader<GameCue>,
    mut queue: ResMut<PlayQueue>,
    mut voices: ResMut<KnightVoices>,
    mut log: Option<ResMut<BarkLog>>,
) {
    let now = real.elapsed_secs_f64();
    let dt = time.delta_secs();
    let voices = &mut *voices;
    let Some(player) = player else {
        damage.clear();
        cues.clear();
        return;
    };
    let (feet, look, eye) = *player;
    let player_eye = feet.translation + Vec3::Y * eye.map_or(EyeHeight::default().0, |e| e.0);
    let ears = match camera {
        Some(c) => Ears {
            eye: c.translation(),
            rotation: c.rotation(),
            vfov: fov.map_or(70f32.to_radians(), |f| f.0),
            aspect: window.map_or(1.6, |w| w.width() / w.height().max(1.0)),
        },
        None => Ears {
            eye: player_eye,
            rotation: look.rotation(),
            vfov: fov.map_or(70f32.to_radians(), |f| f.0),
            aspect: window.map_or(1.6, |w| w.width() / w.height().max(1.0)),
        },
    };

    // 1. A warning wins: cut every bark, drop the queued ones, hold them off.
    let warned = warnings.map_or(0, |w| w.warned);
    if warned != voices.warned {
        voices.warned = warned;
        voices.gate.warning(now, |v| {
            if let Some(log) = log.as_mut() {
                log.cut.push(v);
            }
        });
        queue
            .pending
            .retain(|r| r.sfx.category() != SfxCategory::Voice);
        queue.cut_voices = true;
        if let Some(log) = log.as_mut() {
            log.warnings.push(now);
        }
    }

    let head = |t: &Transform| t.translation + Vec3::Y * HEAD;
    let bark = |voices: &mut KnightVoices,
                queue: &mut PlayQueue,
                log: &mut Option<ResMut<BarkLog>>,
                knight: Entity,
                at: Vec3,
                b: Bark,
                delay: f64| {
        let state = voices.knights.entry(knight).or_default();
        let Some(v) = voices.gate.try_bark(now, knight, state.last_bark, b, delay) else {
            return;
        };
        state.last_bark = now;
        let mut speed = personal_pitch(knight);
        let take = match b {
            Bark::Yelp => {
                state.yelps = state.yelps.wrapping_add(1);
                speed *= yelp_jitter(state.yelps);
                state.yelps % voice::YELP_TAKES
            }
            Bark::Taunt => state.windups / TAUNT_EVERY % voice::TAUNT_TAKES,
            _ => 0,
        };
        queue.push_voice(b.sfx(), Some(at), speed, take, delay, now);
        if let Some(log) = log.as_mut() {
            log.barks.push(v);
        }
    };

    // 2. Hits: a yelp.
    for hit in damage.read() {
        if hit.target_kind != DamageTarget::Character || hit.amount <= 0.0 {
            continue;
        }
        if let Ok((k, t, ..)) = knights.get(hit.target) {
            bark(voices, &mut queue, &mut log, k, head(t), Bark::Yelp, 0.0);
        }
    }

    // 3. Wind-ups (a taunt, on screen only) and void falls (a whaaa).
    for cue in cues.read() {
        match *cue {
            GameCue::WandWindup { who } => {
                let Ok((k, t, ..)) = knights.get(who) else {
                    continue;
                };
                let n = {
                    let state = voices.knights.entry(k).or_default();
                    state.windups = state.windups.wrapping_add(1);
                    state.windups
                };
                let chest = t.translation + Vec3::Y * wand::KNIGHT_CHEST;
                let seen = wand::in_view(
                    ears.eye,
                    ears.rotation,
                    ears.vfov,
                    ears.aspect,
                    chest,
                    wand::VIEW_MARGIN,
                );
                if seen && taunt_turn(k, n) {
                    bark(voices, &mut queue, &mut log, k, head(t), Bark::Taunt, 0.0);
                }
            }
            GameCue::VoidFall { who } => {
                if let Ok((k, t, ..)) = knights.get(who) {
                    bark(
                        voices,
                        &mut queue,
                        &mut log,
                        k,
                        head(t),
                        Bark::Whaaa,
                        WHAAA_DELAY,
                    );
                }
            }
            _ => {}
        }
    }

    // 4. Beam landings: a hup. The victory hop: the nearest survivors'
    //    hoo-hah, one after another.
    for (k, t, _, beaming, parked, _) in &knights {
        let state = voices.knights.entry(k).or_default();
        let landed = state.beaming && !beaming && !parked;
        state.beaming = beaming;
        if landed {
            bark(voices, &mut queue, &mut log, k, head(t), Bark::Hup, 0.0);
        }
    }
    let hopping = run
        .as_deref()
        .zip(tick.as_deref())
        .is_some_and(|(run, tick)| run.hopping(tick.0));
    if hopping && !voices.hopping {
        let mut nearest: [Option<(f32, Entity)>; MAX_BARKS] = [None; MAX_BARKS];
        for (k, t, _, beaming, parked, downed) in &knights {
            if beaming || parked || downed {
                continue;
            }
            insert_nearest(&mut nearest, t.translation.distance(ears.eye), k);
        }
        voices.hop_queue = nearest.map(|n| n.map(|(_, k)| k));
    }
    voices.hopping = hopping;
    if hopping {
        for i in 0..MAX_BARKS {
            let Some(k) = voices.hop_queue[i] else {
                continue;
            };
            if let Ok((_, t, ..)) = knights.get(k) {
                let before = voices.gate.sounding(now);
                bark(voices, &mut queue, &mut log, k, head(t), Bark::HooHah, 0.0);
                if voices.gate.sounding(now) == before {
                    // Not yet (the gaps): try again next frame.
                    break;
                }
            }
            voices.hop_queue[i] = None;
        }
    }

    // 5. Footsteps: the nearest knights' boots on the run clip's footfalls.
    let anims = |owner: Entity| {
        figures
            .iter()
            .find(|(f, _)| f.owner == owner)
            .map(|(_, a)| a.run_phase())
    };
    let mut nearest: [Option<(f32, Entity)>; STEP_KNIGHTS] = [None; STEP_KNIGHTS];
    for (k, t, motor, beaming, parked, downed) in &knights {
        let state = voices.knights.entry(k).or_default();
        let moving = motor.filter(|m| m.grounded && !beaming && !parked && !downed);
        let target = moving.map_or(0.0, |m| m.velocity.with_y(0.0).length());
        state.speed += (target - state.speed) * (1.0 - (-12.0 * dt).exp());
        let phase = match anims(k) {
            Some(p) => p,
            None => (state.phase + state.speed / STRIDE * TAU * dt).rem_euclid(TAU),
        };
        // Footfalls counted across the wrap (phase only grows).
        let wrapped = phase + 0.5 < state.phase;
        let fall = (phase / PI).floor() as i64;
        let prev = (state.phase / PI).floor() as i64;
        let crossed = wrapped || fall != prev;
        state.phase = phase;
        let d = t.translation.distance(ears.eye);
        if crossed && moving.is_some() && state.speed > STEP_SPEED && d < STEP_RANGE {
            insert_nearest(&mut nearest, d, k);
        }
    }
    for (_, k) in nearest.into_iter().flatten() {
        let Ok((_, t, ..)) = knights.get(k) else {
            continue;
        };
        let at = t.translation;
        let sfx = step_sound(spatial.as_ref(), &pieces, at);
        voices.steps = voices.steps.wrapping_add(1);
        queue.push_voice(sfx, Some(at), personal_pitch(k), voices.steps, 0.0, now);
        if let Some(log) = log.as_mut() {
            log.steps.push((now, k, at, sfx));
        }
    }
}

/// Keeps the `N` nearest in `list` (nearest first).
fn insert_nearest<const N: usize>(list: &mut [Option<(f32, Entity)>; N], d: f32, k: Entity) {
    let Some(i) = list.iter().position(|e| e.is_none_or(|(e, _)| d < e)) else {
        return;
    };
    for j in (i + 1..N).rev() {
        list[j] = list[j - 1];
    }
    list[i] = Some((d, k));
}

/// The step for what's under a knight's feet: a piece's top (a wall's brick,
/// a floor's or ramp's planks), else the island's grass.
pub fn step_sound(spatial: Option<&SpatialQuery>, pieces: &Query<&Piece>, feet: Vec3) -> Sfx {
    let kind = spatial.and_then(|spatial| {
        let filter = SpatialQueryFilter::from_mask([Layer::Piece]);
        let hit = spatial.cast_ray(feet + Vec3::Y * 0.3, Dir3::NEG_Y, 0.7, true, &filter)?;
        pieces.get(hit.entity).ok().map(|p| p.kind)
    });
    step_for(kind)
}

/// The step sound on a piece of `kind` (or the island's grass).
pub fn step_for(kind: Option<PieceKind>) -> Sfx {
    match kind {
        None => Sfx::KnightStepGrass,
        Some(PieceKind::Wall) => Sfx::KnightStepBrick,
        Some(_) => Sfx::KnightStepWood,
    }
}

/// The surface a step sound is on (for the bank).
pub fn step_surface(sfx: Sfx) -> Option<bank::StepSurface> {
    match sfx {
        Sfx::KnightStepGrass => Some(bank::StepSurface::Grass),
        Sfx::KnightStepWood => Some(bank::StepSurface::Wood),
        Sfx::KnightStepBrick => Some(bank::StepSurface::Brick),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(i: u32) -> Entity {
        Entity::from_raw_u32(i).unwrap()
    }

    #[test]
    fn nearest_keeps_the_closest_in_order() {
        let mut list: [Option<(f32, Entity)>; 3] = [None; 3];
        for (d, i) in [(5.0, 1), (2.0, 2), (9.0, 3), (1.0, 4), (3.0, 5)] {
            insert_nearest(&mut list, d, e(i));
        }
        let got: Vec<u32> = list.iter().flatten().map(|(_, k)| k.index_u32()).collect();
        assert_eq!(got, [4, 2, 5]);
    }
}
