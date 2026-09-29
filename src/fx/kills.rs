//! Kill confirmation (docs/M4-SPEC.md → Chunk 1, D105): the moment a knight
//! goes down to the player becomes one [`KillConfirmed`] message, on the
//! frame the kill registers, that every kill effect keys off:
//!
//! - the "X" kill hitmarker, the score popups and the multi-kill callouts
//!   (`hud::kills`), the kill chime and the callout stings (`audio`);
//! - the physical death, armor flying off ([`super::armor`]);
//! - **hitstop:** [`FeedbackTuning::hitstop_frames`](super::FeedbackTuning)
//!   rendered frames (2) after the kill frame hold [`HitstopFrozen`], so every
//!   presentation clock read through `FreezableTime` (effects, the knights'
//!   animation, the viewmodel, the HUD's animations) stands still. `Time`, the
//!   fixed gameplay step and input never pause: aim, TTK and every timer are
//!   untouched (D117).
//!
//! **Multi-kills:** a kill within [`KillFeelTuning::chain_seconds`] (1.5 s, in
//! simulation ticks) of the chain's last kill extends the chain, and
//! [`KillConfirmed::chain`] counts it: 2 "Double!", 3 "Triple!", 4 "Quad!",
//! 5 and more "Rampage!" ([`Callout`]). Visual and sound only; the score is
//! the wave director's (`waves`).
//!
//! It all runs headless (no assets), so `tests/kill_feedback.rs` drives it
//! through the simulation.

use super::sim::Hitstop;
use crate::{
    movement::VoidFall,
    shared::{DamageDealt, DamageTarget, Eliminated, HitstopFrozen, Player, TICK_SECONDS},
    tuning::Tuning,
};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// The kill-feedback feel numbers (M4 chunk 1). Designer tuning, never
/// persisted (`Tuning::kills` is `#[serde(skip)]`), so a saved settings file
/// can't freeze them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct KillFeelTuning {
    /// A kill within this long (s) of the chain's last kill extends it.
    pub chain_seconds: f32,
    /// Score popups: life (s) and rise over it (px).
    pub popup_seconds: f32,
    pub popup_rise: f32,
    /// Multi-kill callouts: how long one stays up (s).
    pub callout_seconds: f32,
    /// Armor off a downed knight: its life (s) before it has shrunk away, and
    /// how many pieces can be out at once (the pool).
    pub armor_seconds: f32,
    pub max_armor: u32,
    /// Armor clatter voices at once.
    pub clatter_voices: u32,
    /// Armor chips off a body hit: at least and at most.
    pub chips: (u32, u32),
}

impl Default for KillFeelTuning {
    fn default() -> Self {
        Self {
            chain_seconds: 1.5,
            popup_seconds: 0.8,
            popup_rise: 70.0,
            callout_seconds: 1.1,
            armor_seconds: 1.5,
            max_armor: 48,
            clatter_voices: 4,
            chips: (2, 4),
        }
    }
}

/// The player downed a knight, on the frame it registered.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct KillConfirmed {
    pub victim: Entity,
    /// The victim's feet when it went down.
    pub at: Vec3,
    /// The killing hit was a headshot.
    pub headshot: bool,
    /// Knocked into the void (the kill counted as it fell).
    pub void: bool,
    /// Kills in the current chain, this one included (1 = a single kill).
    pub chain: u32,
    /// Simulation tick of the kill.
    pub tick: u64,
}

/// A multi-kill callout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Callout {
    Double,
    Triple,
    Quad,
    Rampage,
}

impl Callout {
    /// The callout for a chain of `kills` (none for a single kill).
    pub fn for_chain(kills: u32) -> Option<Self> {
        match kills {
            0 | 1 => None,
            2 => Some(Self::Double),
            3 => Some(Self::Triple),
            4 => Some(Self::Quad),
            _ => Some(Self::Rampage),
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            Self::Double => "DOUBLE!",
            Self::Triple => "TRIPLE!",
            Self::Quad => "QUAD!",
            Self::Rampage => "RAMPAGE!",
        }
    }

    /// 0 for a double .. 3 for a rampage (the sting's take).
    pub fn level(self) -> u32 {
        self as u32
    }
}

/// The multi-kill chain (pure): counts kills that come within a window of
/// the chain's last kill, in simulation ticks.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KillChain {
    pub count: u32,
    pub last_tick: Option<u64>,
}

impl KillChain {
    /// Registers a kill at `tick`; returns the chain's length with it.
    pub fn kill(&mut self, tick: u64, window_ticks: u64) -> u32 {
        let extends = self
            .last_tick
            .is_some_and(|last| tick.saturating_sub(last) <= window_ticks);
        self.count = if extends { self.count + 1 } else { 1 };
        self.last_tick = Some(tick);
        self.count
    }
}

/// Whole ticks in `seconds`.
pub fn ticks_in(seconds: f32) -> u64 {
    (seconds.max(0.0) / TICK_SECONDS).round() as u64
}

/// The hitstop's frame bookkeeping (see the module docs).
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct HitstopClock(pub Hitstop);

/// Evidence for the same-frame rule: kill feedback that showed on the frame
/// its kill registered. Each consumer counts itself.
#[derive(Resource, Debug, Default, Clone, Serialize)]
pub struct KillFeedbackStats {
    pub kills: u32,
    pub hitstops: u32,
    pub markers_same_frame: u32,
    pub sounds_same_frame: u32,
    pub popups_same_frame: u32,
    pub armor_same_frame: u32,
    pub callouts: u32,
}

/// Where kills are confirmed each frame (in `Update`, right after the fixed
/// step). The HUD, audio and effects that react run after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct KillFeedbackSet;

/// Kill confirmation, the chain and the hitstop. Headless-safe; added by
/// `FxPlugin` (and by tests on their own).
pub struct KillFeedbackPlugin;

impl Plugin for KillFeedbackPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<KillConfirmed>()
            .add_message::<Eliminated>()
            .add_message::<DamageDealt>()
            .init_resource::<KillChain>()
            .init_resource::<HitstopClock>()
            .init_resource::<HitstopFrozen>()
            .init_resource::<KillFeedbackStats>()
            .add_systems(Update, confirm_kills.in_set(KillFeedbackSet))
            .add_systems(Last, end_hitstop_frame);
    }
}

/// Reads this frame's eliminations: the player's become [`KillConfirmed`]s,
/// extend the chain and start the hitstop.
#[allow(clippy::too_many_arguments)]
fn confirm_kills(
    tuning: Res<Tuning>,
    player: Option<Single<Entity, With<Player>>>,
    fallers: Query<(), With<VoidFall>>,
    mut damage: MessageReader<DamageDealt>,
    mut eliminated: MessageReader<Eliminated>,
    mut confirmed: MessageWriter<KillConfirmed>,
    mut chain: ResMut<KillChain>,
    mut hitstop: ResMut<HitstopClock>,
    mut stats: ResMut<KillFeedbackStats>,
    mut headshots: Local<Vec<Entity>>,
) {
    let me = player.map(|p| *p);
    headshots.clear();
    for hit in damage.read() {
        if hit.killed
            && hit.headshot
            && hit.target_kind == DamageTarget::Character
            && me.is_some()
            && hit.source == me
        {
            headshots.push(hit.target);
        }
    }
    let feel = &tuning.kills;
    let feedback = &tuning.feedback;
    for kill in eliminated.read() {
        if me.is_none() || kill.by != me || Some(kill.victim) == me {
            continue;
        }
        let count = chain.kill(kill.tick, ticks_in(feel.chain_seconds));
        confirmed.write(KillConfirmed {
            victim: kill.victim,
            at: kill.position,
            headshot: headshots.contains(&kill.victim),
            void: fallers.contains(kill.victim),
            chain: count,
            tick: kill.tick,
        });
        stats.kills += 1;
        if Callout::for_chain(count).is_some() {
            stats.callouts += 1;
        }
        if feedback.hitstop_on_kill && hitstop.0.trigger(feedback.hitstop_frames) {
            stats.hitstops += 1;
        }
    }
}

/// Ends each rendered frame: the frames after a kill's own frame hold.
fn end_hitstop_frame(mut hitstop: ResMut<HitstopClock>, mut frozen: ResMut<HitstopFrozen>) {
    hitstop.0.end_frame();
    let hold = hitstop.0.holding();
    if frozen.0 != hold {
        frozen.0 = hold;
    }
}
