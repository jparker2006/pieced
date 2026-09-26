//! Slice F — the synthesized sound bank and playback.
//!
//! Every sound is generated in code at startup ([`bank`], [`synth`]) into in-memory
//! WAV bytes and loaded as Bevy [`AudioSource`]s. Playback is driven by the gameplay
//! messages: the player's own sounds are non-spatial; everything happening in the
//! world (the dummy's steps, pieces) is spatial, heard through a
//! [`SpatialListener`] on the main camera. A voice cap keeps the mixer cheap.
//!
//! Milestone 2 ("Spellbound") replaced the bank with magical, cartoony cues: spell
//! zaps, bonks and sparkles, brick clunks and plank thocks, glassy shield crashes
//! and a poof-and-slide-whistle elimination. Every cue is mastered to a documented
//! loudness target (see [`bank`]) and mixed by [`Sfx::mix_db`]; hit confirmation
//! sits above the player's own casts and ducks them on the frame it lands.

pub mod bank;
pub mod synth;

use crate::{
    building::Piece,
    hud::{FrameStartTick, HitFeedbackStats},
    render::MainCamera,
    shared::{
        DamageDealt, DamageTarget, Eliminated, GameCue, PieceChange, PieceChanged, PieceKind,
        Player, ShotFired, WeaponKind,
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
    /// Gain on the player's own weapon sounds that start on the same frame as one
    /// of their hit confirmations, so the hit cuts through (0.7 ≈ −3 dB).
    pub hit_duck: f32,
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
            hit_duck: 0.7,
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
    /// Rifle spell: a bright falling zap with a sparkle shimmer.
    RifleShot,
    /// Pump spell: "whoomp-zap" with a chime burst.
    PumpShot,
    /// Gold-ring whirr and metallic tick.
    PumpRack,
    /// Crystal clink as the dim crystal pops out.
    RifleMagOut,
    /// Rising hum as the fresh crystal slots in.
    RifleMagIn,
    /// Crystal-shard tink.
    PumpShell,
    /// Soft magical swish.
    WeaponSwitch,
    /// Body hit: bonk plus sparkle.
    HitTick,
    /// Headshot: bonk plus a bright ding.
    HeadshotDing,
    /// Glassy tick.
    ShieldHit,
    /// Glass crash plus chime.
    ShieldBreak,
    /// Cartoon poof plus a slide whistle going down.
    Elimination,
    /// Wall placed: brick clunk.
    BrickPlace,
    /// Floor or ramp placed: wooden thock.
    PlankPlace,
    BrickCrack,
    PlankCrack,
    /// Wall broken: brick crumble.
    BrickBreak,
    /// Floor or ramp broken: wood splinter.
    PlankBreak,
    /// Invalid placement: soft cartoon bwomp.
    Rejected,
    /// Soft grass step.
    Footstep,
    /// Light "boing".
    Jump,
    /// Soft thud.
    Land,
    /// Swish.
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
    pub const ALL: [Sfx; 23] = [
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
        Sfx::BrickPlace,
        Sfx::PlankPlace,
        Sfx::BrickCrack,
        Sfx::PlankCrack,
        Sfx::BrickBreak,
        Sfx::PlankBreak,
        Sfx::Rejected,
        Sfx::Footstep,
        Sfx::Jump,
        Sfx::Land,
        Sfx::Slide,
    ];

    /// The cue for a piece event: walls are brick (clunk, crack, crumble); floors
    /// and ramps are wooden planks (thock, crack, splinter).
    pub fn for_piece(kind: PieceKind, change: PieceChange) -> Sfx {
        let brick = matches!(kind, PieceKind::Wall);
        match (change, brick) {
            (PieceChange::Placed, true) => Sfx::BrickPlace,
            (PieceChange::Placed, false) => Sfx::PlankPlace,
            (PieceChange::Cracked(_), true) => Sfx::BrickCrack,
            (PieceChange::Cracked(_), false) => Sfx::PlankCrack,
            (PieceChange::Destroyed, true) => Sfx::BrickBreak,
            (PieceChange::Destroyed, false) => Sfx::PlankBreak,
        }
    }

    /// Renders the sound's first take (mono, [`synth::SAMPLE_RATE`]).
    pub fn synthesize(self) -> Vec<f32> {
        self.synthesize_take(0)
    }

    /// Renders round-robin take `take` (wrapping at [`Sfx::takes`]).
    pub fn synthesize_take(self, take: u32) -> Vec<f32> {
        match self {
            Sfx::RifleShot => bank::rifle_shot(take),
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
            Sfx::BrickPlace => bank::brick_place(),
            Sfx::PlankPlace => bank::plank_place(),
            Sfx::BrickCrack => bank::brick_crack(),
            Sfx::PlankCrack => bank::plank_crack(),
            Sfx::BrickBreak => bank::brick_break(),
            Sfx::PlankBreak => bank::plank_break(),
            Sfx::Rejected => bank::rejected(),
            Sfx::Footstep => bank::footstep(take),
            Sfx::Jump => bank::jump(),
            Sfx::Land => bank::land(),
            Sfx::Slide => bank::slide(),
        }
    }

    /// Round-robin takes in the bank (the fastest-repeating cues get several).
    pub fn takes(self) -> u32 {
        match self {
            Sfx::RifleShot => bank::RIFLE_VARIANTS,
            Sfx::Footstep => bank::FOOTSTEP_VARIANTS,
            _ => 1,
        }
    }

    /// The sound's first take as a WAV file.
    pub fn wav(self) -> Vec<u8> {
        synth::encode_wav(&self.synthesize())
    }

    /// Length budget and loudness target (see [`bank`]).
    pub fn spec(self) -> bank::CueSpec {
        use Sfx::*;
        match self {
            RifleShot => bank::RIFLE_CAST,
            PumpShot => bank::PUMP_CAST,
            PumpRack => bank::PUMP_RACK,
            RifleMagOut => bank::RIFLE_MAG_OUT,
            RifleMagIn => bank::RIFLE_MAG_IN,
            PumpShell => bank::PUMP_SHELL,
            WeaponSwitch => bank::WEAPON_SWITCH,
            HitTick => bank::BODY_HIT,
            HeadshotDing => bank::HEADSHOT,
            ShieldHit => bank::SHIELD_HIT,
            ShieldBreak => bank::SHIELD_BREAK,
            Elimination => bank::ELIMINATION,
            BrickPlace => bank::BRICK_PLACE,
            PlankPlace => bank::PLANK_PLACE,
            BrickCrack => bank::BRICK_CRACK,
            PlankCrack => bank::PLANK_CRACK,
            BrickBreak => bank::BRICK_BREAK,
            PlankBreak => bank::PLANK_BREAK,
            Rejected => bank::REJECTED,
            Footstep => bank::FOOTSTEP,
            Jump => bank::JUMP,
            Land => bank::LAND,
            Slide => bank::SLIDE,
        }
    }

    pub fn category(self) -> SfxCategory {
        use Sfx::*;
        match self {
            RifleShot | PumpShot | PumpRack | RifleMagOut | RifleMagIn | PumpShell
            | WeaponSwitch => SfxCategory::Weapons,
            HitTick | HeadshotDing | ShieldHit | ShieldBreak | Elimination => SfxCategory::Hits,
            BrickPlace | PlankPlace | BrickCrack | PlankCrack | BrickBreak | PlankBreak
            | Rejected => SfxCategory::Building,
            Footstep | Jump | Land | Slide => SfxCategory::Movement,
        }
    }

    /// The mix: how loud the cue plays in game (short-term RMS, dBFS) before master
    /// volume, category volume and distance. Hit confirmation sits 4–5 dB above
    /// the rifle (which fires six times a second) and 1–2 dB above the pump, and
    /// the player's own casts also dip by `hit_duck` on the frame a hit lands;
    /// building sits with the rifle; handling sounds and movement sit well under.
    ///
    /// | Cues | Mix (dBFS) |
    /// |---|---|
    /// | headshot, shield break, elimination | −13 |
    /// | body hit, shield hit | −14 |
    /// | pump cast | −15 |
    /// | piece breaks | −16 |
    /// | rifle cast, piece places and cracks | −18 |
    /// | pump rack, reload, rejected | −20 |
    /// | pump shell | −21 |
    /// | weapon switch | −22 |
    /// | land, slide | −22 |
    /// | jump | −26 |
    /// | footstep | −27 |
    pub fn mix_db(self) -> f32 {
        use Sfx::*;
        match self {
            HeadshotDing | ShieldBreak | Elimination => -13.0,
            HitTick | ShieldHit => -14.0,
            PumpShot => -15.0,
            BrickBreak | PlankBreak => -16.0,
            RifleShot | BrickPlace | PlankPlace | BrickCrack | PlankCrack => -18.0,
            PumpRack | RifleMagOut | RifleMagIn | Rejected => -20.0,
            PumpShell => -21.0,
            WeaponSwitch | Land | Slide => -22.0,
            Jump => -26.0,
            Footstep => -27.0,
        }
    }

    /// Mix gain before master and category volume: what takes the cue from its
    /// mastered loudness to its [`Sfx::mix_db`].
    pub fn base_volume(self) -> f32 {
        synth::db_to_gain(self.mix_db() - self.spec().rms_db).min(1.0)
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

/// Gain for a sound starting this frame: the player's own weapon sounds dip to
/// `duck` when one of the player's hit confirmations starts on the same frame
/// (hitscan hits land with their shot), so the bonk and sparkle cut through.
pub fn duck_gain(sfx: Sfx, own: bool, own_hit_this_frame: bool, duck: f32) -> f32 {
    if own && own_hit_this_frame && sfx.category() == SfxCategory::Weapons {
        duck.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// Every cue's takes as WAV files, indexed by [`Sfx`] and then take. This is all
/// the synthesis work done at startup.
pub fn render_bank() -> Vec<Vec<Vec<u8>>> {
    Sfx::ALL
        .iter()
        .map(|sfx| {
            (0..sfx.takes())
                .map(|take| synth::encode_wav(&sfx.synthesize_take(take)))
                .collect()
        })
        .collect()
}

/// Handles to every synthesized sound, indexed by [`Sfx`] and then take.
#[derive(Resource, Debug, Clone)]
pub struct SoundBank {
    handles: Vec<Vec<Handle<AudioSource>>>,
}

impl SoundBank {
    pub fn get(&self, sfx: Sfx) -> Handle<AudioSource> {
        self.take(sfx, 0)
    }

    /// Round-robin take `take` of `sfx` (wraps).
    pub fn take(&self, sfx: Sfx, take: u32) -> Handle<AudioSource> {
        let takes = &self.handles[sfx as usize];
        takes[take as usize % takes.len()].clone()
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
    /// Round-robin take.
    take: u32,
    /// Real time (s) at which to start.
    when: f64,
}

#[derive(Resource, Default)]
struct PlayQueue {
    pending: Vec<PlayRequest>,
    /// Counter for deterministic pitch variation.
    variation: u32,
    /// Next round-robin take per [`Sfx`].
    takes: [u32; Sfx::ALL.len()],
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
        let take = self.takes[sfx as usize];
        self.takes[sfx as usize] = (take + 1) % sfx.takes().max(1);
        self.pending.push(PlayRequest {
            sfx,
            at,
            gain,
            speed,
            take,
            when: now + delay,
        });
    }
}

fn build_sound_bank(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let handles = render_bank()
        .into_iter()
        .map(|takes| {
            takes
                .into_iter()
                .map(|wav| sources.add(AudioSource { bytes: wav.into() }))
                .collect()
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
    pieces: Query<&Piece>,
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
                // A quiet, quick knock in the piece's material so shooting it has
                // weight. A piece this shot destroyed is already gone; its break
                // sound covers it.
                if let Ok(piece) = pieces.get(hit.target) {
                    piece_knock = true;
                    let knock = Sfx::for_piece(piece.kind, PieceChange::Placed);
                    queue.push_with(knock, Some(hit.point), 0.35, 0.0, now);
                }
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
        queue.push(
            Sfx::for_piece(change.kind, change.change),
            Some(change.center),
            now,
        );
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
    let own_hit = due
        .iter()
        .any(|r| r.at.is_none() && r.sfx.category() == SfxCategory::Hits);
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
        let duck = duck_gain(request.sfx, request.at.is_none(), own_hit, audio.hit_duck);
        let level = request.sfx.base_volume()
            * audio.category_gain(request.sfx.category())
            * request.gain
            * duck;
        let mut settings = PlaybackSettings::DESPAWN
            .with_volume(Volume::Linear(level * master))
            .with_speed(request.speed);
        let mut entity = commands.spawn((
            Name::new("Sfx"),
            AudioPlayer::new(bank.take(request.sfx, request.take)),
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
