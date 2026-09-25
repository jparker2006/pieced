//! Slice F — the synthesized sound bank and playback.
//!
//! Every sound is generated in code at startup ([`bank`], [`synth`]) into in-memory
//! WAV bytes and loaded as Bevy [`AudioSource`]s. Playback is driven by the gameplay
//! messages: the player's own sounds are non-spatial; everything happening in the
//! world (the dummy's steps, pieces) is spatial, heard through a
//! [`SpatialListener`] on the main camera. A voice cap keeps the mixer cheap.

pub mod bank;
pub mod synth;

use crate::{
    hud::{FrameStartTick, HitFeedbackStats},
    render::MainCamera,
    shared::{
        DamageDealt, DamageTarget, Eliminated, GameCue, PieceChange, PieceChanged, Player,
        ShotFired, WeaponKind,
    },
    tuning::Tuning,
};
use bevy::{
    audio::{AudioSinkPlayback, SpatialScale, Volume},
    prelude::*,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AudioTuning {
    pub master_volume: f32,
    pub muted: bool,
    /// Most sounds playing at once; lower-priority voices are stolen first.
    pub max_voices: u32,
    /// Distance scale for spatial sounds: full volume within `1 / spatial_scale`
    /// meters, then inverse-square falloff.
    pub spatial_scale: f32,
    pub weapons_volume: f32,
    pub hits_volume: f32,
    pub building_volume: f32,
    pub movement_volume: f32,
    /// Delay from a pump shot to its rack (s).
    pub pump_rack_delay: f32,
}

impl Default for AudioTuning {
    fn default() -> Self {
        Self {
            master_volume: 0.8,
            muted: false,
            max_voices: 24,
            spatial_scale: 0.08,
            weapons_volume: 1.0,
            hits_volume: 1.0,
            building_volume: 1.0,
            movement_volume: 1.0,
            pump_rack_delay: 0.34,
        }
    }
}

impl AudioTuning {
    /// Master gain actually applied (0 when muted).
    pub fn effective_master(&self) -> f32 {
        if self.muted {
            0.0
        } else {
            self.master_volume.clamp(0.0, 1.0)
        }
    }

    fn category_gain(&self, category: SfxCategory) -> f32 {
        match category {
            SfxCategory::Weapons => self.weapons_volume,
            SfxCategory::Hits => self.hits_volume,
            SfxCategory::Building => self.building_volume,
            SfxCategory::Movement => self.movement_volume,
        }
        .max(0.0)
    }
}

// ---------------------------------------------------------------------------
// The sound bank
// ---------------------------------------------------------------------------

/// Every sound effect in the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sfx {
    RifleShot,
    PumpShot,
    PumpRack,
    RifleMagOut,
    RifleMagIn,
    PumpShell,
    WeaponSwitch,
    HitTick,
    HeadshotDing,
    ShieldHit,
    ShieldBreak,
    Elimination,
    PiecePlace,
    PieceCrack,
    PieceBreak,
    Rejected,
    Footstep,
    Jump,
    Land,
    Slide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SfxCategory {
    Weapons,
    Hits,
    Building,
    Movement,
}

impl Sfx {
    pub const ALL: [Sfx; 20] = [
        Sfx::RifleShot,
        Sfx::PumpShot,
        Sfx::PumpRack,
        Sfx::RifleMagOut,
        Sfx::RifleMagIn,
        Sfx::PumpShell,
        Sfx::WeaponSwitch,
        Sfx::HitTick,
        Sfx::HeadshotDing,
        Sfx::ShieldHit,
        Sfx::ShieldBreak,
        Sfx::Elimination,
        Sfx::PiecePlace,
        Sfx::PieceCrack,
        Sfx::PieceBreak,
        Sfx::Rejected,
        Sfx::Footstep,
        Sfx::Jump,
        Sfx::Land,
        Sfx::Slide,
    ];

    /// Renders the sound's samples (mono, [`synth::SAMPLE_RATE`]).
    pub fn synthesize(self) -> Vec<f32> {
        match self {
            Sfx::RifleShot => bank::rifle_shot(),
            Sfx::PumpShot => bank::pump_shot(),
            Sfx::PumpRack => bank::pump_rack(),
            Sfx::RifleMagOut => bank::rifle_mag_out(),
            Sfx::RifleMagIn => bank::rifle_mag_in(),
            Sfx::PumpShell => bank::pump_shell(),
            Sfx::WeaponSwitch => bank::weapon_switch(),
            Sfx::HitTick => bank::hit_tick(),
            Sfx::HeadshotDing => bank::headshot_ding(),
            Sfx::ShieldHit => bank::shield_hit(),
            Sfx::ShieldBreak => bank::shield_break(),
            Sfx::Elimination => bank::elimination(),
            Sfx::PiecePlace => bank::piece_place(),
            Sfx::PieceCrack => bank::piece_crack(),
            Sfx::PieceBreak => bank::piece_break(),
            Sfx::Rejected => bank::rejected(),
            Sfx::Footstep => bank::footstep(),
            Sfx::Jump => bank::jump(),
            Sfx::Land => bank::land(),
            Sfx::Slide => bank::slide(),
        }
    }

    /// The sound as a WAV file.
    pub fn wav(self) -> Vec<u8> {
        synth::encode_wav(&self.synthesize())
    }

    pub fn category(self) -> SfxCategory {
        use Sfx::*;
        match self {
            RifleShot | PumpShot | PumpRack | RifleMagOut | RifleMagIn | PumpShell
            | WeaponSwitch => SfxCategory::Weapons,
            HitTick | HeadshotDing | ShieldHit | ShieldBreak | Elimination => SfxCategory::Hits,
            PiecePlace | PieceCrack | PieceBreak | Rejected => SfxCategory::Building,
            Footstep | Jump | Land | Slide => SfxCategory::Movement,
        }
    }

    /// Mix level before master and category volume.
    pub fn base_volume(self) -> f32 {
        use Sfx::*;
        match self {
            RifleShot => 0.5,
            PumpShot => 0.7,
            PumpRack => 0.4,
            RifleMagOut | RifleMagIn => 0.42,
            PumpShell => 0.42,
            WeaponSwitch => 0.3,
            HitTick => 0.5,
            HeadshotDing => 0.5,
            ShieldHit => 0.42,
            ShieldBreak => 0.6,
            Elimination => 0.6,
            PiecePlace => 0.42,
            PieceCrack => 0.45,
            PieceBreak => 0.6,
            Rejected => 0.35,
            Footstep => 0.22,
            Jump => 0.22,
            Land => 0.32,
            Slide => 0.32,
        }
    }

    /// Voice-stealing priority: higher survives. Hit confirmation matters most.
    pub fn priority(self) -> u8 {
        match self.category() {
            SfxCategory::Hits => 3,
            SfxCategory::Weapons | SfxCategory::Building => 2,
            SfxCategory::Movement => 1,
        }
    }
}

/// Handles to every synthesized sound, indexed by [`Sfx`].
#[derive(Resource, Debug, Clone)]
pub struct SoundBank {
    handles: Vec<Handle<AudioSource>>,
}

impl SoundBank {
    pub fn get(&self, sfx: Sfx) -> Handle<AudioSource> {
        self.handles[sfx as usize].clone()
    }
}

// ---------------------------------------------------------------------------
// Voice management (pure)
// ---------------------------------------------------------------------------

/// What to do with a new sound when the voice cap is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceDecision<T> {
    Play,
    /// Stop this existing voice, then play.
    Steal(T),
    Drop,
}

/// Picks the voice to steal for a new sound of `priority`: the oldest voice of the
/// lowest priority, if that priority is not higher than the new sound's.
/// `voices` are `(id, priority, started_seconds)`.
pub fn choose_voice<T: Copy>(
    voices: &[(T, u8, f64)],
    max_voices: usize,
    priority: u8,
) -> VoiceDecision<T> {
    if voices.len() < max_voices.max(1) {
        return VoiceDecision::Play;
    }
    let victim = voices
        .iter()
        .min_by(|a, b| a.1.cmp(&b.1).then(a.2.total_cmp(&b.2)));
    match victim {
        Some(&(id, p, _)) if p <= priority => VoiceDecision::Steal(id),
        _ => VoiceDecision::Drop,
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayQueue>()
            .add_systems(Startup, build_sound_bank)
            .add_observer(attach_listener)
            .add_systems(
                Update,
                (
                    queue_combat_sounds,
                    queue_piece_sounds,
                    queue_cue_sounds,
                    play_queued,
                    apply_live_volume,
                )
                    .chain(),
            );
    }
}

/// A playing sound effect.
#[derive(Component, Debug, Clone, Copy)]
struct Voice {
    priority: u8,
    started: f64,
    /// Volume before master volume (so master changes apply live).
    level: f32,
}

#[derive(Debug, Clone, Copy)]
struct PlayRequest {
    sfx: Sfx,
    /// World position for spatial sounds; `None` plays it as the player's own.
    at: Option<Vec3>,
    gain: f32,
    speed: f32,
    /// Real time (s) at which to start.
    when: f64,
}

#[derive(Resource, Default)]
struct PlayQueue {
    pending: Vec<PlayRequest>,
    /// Counter for deterministic pitch variation.
    variation: u32,
}

impl PlayQueue {
    fn push(&mut self, sfx: Sfx, at: Option<Vec3>, now: f64) {
        self.push_with(sfx, at, 1.0, 0.0, now);
    }

    fn push_with(&mut self, sfx: Sfx, at: Option<Vec3>, gain: f32, delay: f64, now: f64) {
        // Small, deterministic pitch variation so repeats never sound machine-gunned.
        self.variation = self.variation.wrapping_add(1);
        let h = self.variation.wrapping_mul(2_654_435_761) >> 16;
        let spread = match sfx.category() {
            SfxCategory::Movement => 0.08,
            SfxCategory::Weapons | SfxCategory::Building => 0.03,
            SfxCategory::Hits => 0.0,
        };
        let speed = 1.0 + spread * ((h % 1000) as f32 / 500.0 - 1.0);
        self.pending.push(PlayRequest {
            sfx,
            at,
            gain,
            speed,
            when: now + delay,
        });
    }
}

fn build_sound_bank(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let handles = Sfx::ALL
        .iter()
        .enumerate()
        .map(|(i, sfx)| {
            debug_assert_eq!(*sfx as usize, i);
            sources.add(AudioSource {
                bytes: sfx.wav().into(),
            })
        })
        .collect();
    commands.insert_resource(SoundBank { handles });
}

fn attach_listener(add: On<Add, MainCamera>, mut commands: Commands) {
    commands
        .entity(add.entity)
        .insert(SpatialListener::new(0.25));
}

fn queue_combat_sounds(
    time: Res<Time<Real>>,
    tuning: Res<Tuning>,
    player: Option<Single<Entity, With<Player>>>,
    start_tick: Option<Res<FrameStartTick>>,
    mut stats: Option<ResMut<HitFeedbackStats>>,
    mut shots: MessageReader<ShotFired>,
    mut damage: MessageReader<DamageDealt>,
    mut eliminated: MessageReader<Eliminated>,
    mut queue: ResMut<PlayQueue>,
) {
    let now = time.elapsed_secs_f64();
    let player = player.map(|p| *p);
    let mine = |e: Option<Entity>| e.is_some() && e == player;
    for shot in shots.read() {
        let at = (!mine(Some(shot.shooter))).then_some(shot.origin);
        match shot.weapon {
            WeaponKind::Rifle => queue.push(Sfx::RifleShot, at, now),
            WeaponKind::Pump => {
                queue.push(Sfx::PumpShot, at, now);
                let delay = tuning.audio.pump_rack_delay.max(0.0) as f64;
                queue.push_with(Sfx::PumpRack, at, 1.0, delay, now);
            }
        }
    }
    let frame_start = start_tick.map(|t| t.0).unwrap_or(0);
    let mut piece_knock = false;
    for hit in damage.read() {
        if !mine(hit.source) {
            continue;
        }
        match hit.target_kind {
            DamageTarget::Character => {
                let base = if hit.headshot {
                    Sfx::HeadshotDing
                } else if hit.to_shield > 0.0 && !hit.shield_broke {
                    Sfx::ShieldHit
                } else {
                    Sfx::HitTick
                };
                queue.push(base, None, now);
                if hit.shield_broke {
                    queue.push(Sfx::ShieldBreak, None, now);
                }
                if let Some(stats) = stats.as_mut()
                    && hit.tick > frame_start
                {
                    stats.sounds_same_frame += 1;
                }
            }
            DamageTarget::Piece if !piece_knock => {
                // A quiet, quick knock so shooting a wall has weight.
                piece_knock = true;
                queue.push_with(Sfx::PiecePlace, Some(hit.point), 0.35, 0.0, now);
            }
            DamageTarget::Piece => {}
        }
    }
    for kill in eliminated.read() {
        if mine(kill.by) {
            queue.push(Sfx::Elimination, None, now);
        }
    }
}

fn queue_piece_sounds(
    time: Res<Time<Real>>,
    mut changes: MessageReader<PieceChanged>,
    mut queue: ResMut<PlayQueue>,
) {
    let now = time.elapsed_secs_f64();
    for change in changes.read() {
        let sfx = match change.change {
            PieceChange::Placed => Sfx::PiecePlace,
            PieceChange::Cracked(_) => Sfx::PieceCrack,
            PieceChange::Destroyed => Sfx::PieceBreak,
        };
        queue.push(sfx, Some(change.center), now);
    }
}

fn queue_cue_sounds(
    time: Res<Time<Real>>,
    player: Option<Single<Entity, With<Player>>>,
    transforms: Query<&Transform>,
    mut cues: MessageReader<GameCue>,
    mut queue: ResMut<PlayQueue>,
) {
    let now = time.elapsed_secs_f64();
    let player = player.map(|p| *p);
    for cue in cues.read() {
        let (who, sfx) = match *cue {
            GameCue::Jump { who } => (who, Sfx::Jump),
            GameCue::Land { who, speed } => {
                if speed < 2.0 {
                    continue;
                }
                (who, Sfx::Land)
            }
            GameCue::Footstep { who } => (who, Sfx::Footstep),
            GameCue::SlideStart { who } => (who, Sfx::Slide),
            GameCue::ReloadStart { who, weapon } => match weapon {
                WeaponKind::Rifle => (who, Sfx::RifleMagOut),
                // The pump's reload is heard shell by shell.
                WeaponKind::Pump => continue,
            },
            GameCue::ReloadShell { who } => (who, Sfx::PumpShell),
            GameCue::ReloadDone { who, weapon } => match weapon {
                WeaponKind::Rifle => (who, Sfx::RifleMagIn),
                WeaponKind::Pump => (who, Sfx::PumpRack),
            },
            GameCue::WeaponSwitch { who, .. } => (who, Sfx::WeaponSwitch),
            GameCue::PlacementRejected { who } => (who, Sfx::Rejected),
            GameCue::AdsChanged { .. } | GameCue::Respawned { .. } => continue,
        };
        let own = Some(who) == player;
        if !own && sfx.category() != SfxCategory::Movement {
            // Other characters are heard moving; their gear stays quiet for now.
            continue;
        }
        let at = if own {
            None
        } else {
            match transforms.get(who) {
                Ok(t) => Some(t.translation),
                Err(_) => continue,
            }
        };
        queue.push(sfx, at, now);
    }
}

fn play_queued(
    mut commands: Commands,
    time: Res<Time<Real>>,
    tuning: Res<Tuning>,
    bank: Option<Res<SoundBank>>,
    mut queue: ResMut<PlayQueue>,
    voices: Query<(Entity, &Voice)>,
) {
    let Some(bank) = bank else {
        queue.pending.clear();
        return;
    };
    let now = time.elapsed_secs_f64();
    let audio = &tuning.audio;
    let master = audio.effective_master();
    let mut due = Vec::new();
    queue.pending.retain(|r| {
        if r.when <= now {
            due.push(*r);
            false
        } else {
            true
        }
    });
    if master <= 0.0 || due.is_empty() {
        return;
    }
    let mut active: Vec<(Entity, u8, f64)> = voices
        .iter()
        .map(|(e, v)| (e, v.priority, v.started))
        .collect();
    // One of each non-spatial sound per frame is enough (two hits on one frame
    // shouldn't double the volume).
    let mut played_own: Vec<Sfx> = Vec::new();
    for request in due {
        if request.at.is_none() {
            if played_own.contains(&request.sfx) {
                continue;
            }
            played_own.push(request.sfx);
        }
        let priority = request.sfx.priority();
        match choose_voice(&active, audio.max_voices as usize, priority) {
            VoiceDecision::Play => {}
            VoiceDecision::Steal(victim) => {
                commands.entity(victim).despawn();
                active.retain(|v| v.0 != victim);
            }
            VoiceDecision::Drop => continue,
        }
        let level =
            request.sfx.base_volume() * audio.category_gain(request.sfx.category()) * request.gain;
        let mut settings = PlaybackSettings::DESPAWN
            .with_volume(Volume::Linear(level * master))
            .with_speed(request.speed);
        let mut entity = commands.spawn((
            Name::new("Sfx"),
            AudioPlayer::new(bank.get(request.sfx)),
            Voice {
                priority,
                started: now,
                level,
            },
        ));
        if let Some(at) = request.at {
            settings = settings
                .with_spatial(true)
                .with_spatial_scale(SpatialScale::new(audio.spatial_scale.max(0.001)));
            entity.insert(Transform::from_translation(at));
        }
        entity.insert(settings);
        active.push((entity.id(), priority, now));
    }
}

/// Master volume and mute apply to sounds already playing, too.
fn apply_live_volume(
    tuning: Res<Tuning>,
    mut last: Local<Option<f32>>,
    mut sinks: Query<(
        &Voice,
        Option<&mut AudioSink>,
        Option<&mut SpatialAudioSink>,
    )>,
) {
    let master = tuning.audio.effective_master();
    if *last == Some(master) {
        return;
    }
    *last = Some(master);
    for (voice, sink, spatial) in &mut sinks {
        let volume = Volume::Linear(voice.level * master);
        if let Some(mut sink) = sink {
            sink.set_volume(volume);
        }
        if let Some(mut sink) = spatial {
            sink.set_volume(volume);
        }
    }
}
