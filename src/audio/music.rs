//! The adaptive score (M4 chunk 3, D107, D116, D119).
//!
//! | Slot | When | Music |
//! |---|---|---|
//! | Menu | the main menu | "Space Fanfare" (whole, looping) |
//! | Break | the 10 s break, the results, Practice | the synthesized celesta waltz ([`celesta`](super::celesta)) |
//! | Combat low | a wave in progress | "Cinematic orchestral adventure" (22-bar loop) |
//! | Combat high | wave ≥ 6, or ≥ 6 knights alive in this wave | "Cinematic Battle Music (Star Wars Style)" (28-bar loop) |
//!
//! Stings: the round-start fanfare on every wave start ([`WaveStarted`], the
//! event chunk 5's banner syncs to), the death sting on the player's
//! elimination (a brass chord from the battle cue bent down and faded:
//! [`death_sting`]), and the new-best fanfare on "NEW BEST!".
//!
//! - **Crossfades:** slots crossfade (equal power) over
//!   [`MusicTuning::crossfade_seconds`]; a slot coming back from silence
//!   restarts from its top.
//! - **Ducking:** a sting ducks the loops while it plays; a big hit (a kill, a
//!   headshot, a shield break, an orb hitting you) ducks them for 150 ms.
//! - **Levels:** the Music and Effects sliders (Settings → Audio) scale the
//!   score and the effects under the master volume; both are exactly silent
//!   at 0.
//! - **Loading:** the music is Ogg Vorbis embedded in the binary
//!   (`assets/music/`, built by `scripts/build-music.sh`). A background thread
//!   decodes it (the menu first), renders the break loop and derives the
//!   death sting while the game boots; each piece is ready the moment it's
//!   done and the main thread only files it. Playback streams the decoded
//!   samples through [`MusicClip`], one pooled voice per piece, so nothing
//!   is spawned or allocated per wave.
//!
//! The [`MusicDirector`] is pure (state in, gains and cues out, fixed ticks),
//! which is what the tests drive.

use super::{
    celesta,
    loudness::integrated_lufs,
    reverb::{Fdn, Room},
    synth::{Svf, db_to_gain, soft_limit},
};
use crate::{
    shared::AppState,
    tuning::Tuning,
    waves::{Run, RunPhase, RunSummary},
};
use bevy::{
    audio::{
        AudioSink, AudioSinkPlayback, AudioSource, ChannelCount, Decodable, PlaybackMode,
        SampleRate, Source, Volume,
    },
    prelude::*,
};
use serde::{Deserialize, Serialize};
use std::{
    f32::consts::FRAC_PI_2,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

/// Every music file's sample rate.
pub const MUSIC_RATE: u32 = 44_100;
/// Every music file is stereo.
pub const MUSIC_CHANNELS: u16 = 2;
/// Loudness of the stings (and the death sting derived at load), LUFS; the
/// loops sit at [`celesta::TARGET_LUFS`].
pub const STING_LUFS: f32 = -16.0;

/// The music files, embedded (the game runs from outside the repo).
pub const MUSIC_FILES: [(&str, &[u8]); 6] = [
    ("menu", include_bytes!("../../assets/music/menu.ogg")),
    (
        "combat_low",
        include_bytes!("../../assets/music/combat_low.ogg"),
    ),
    (
        "combat_high",
        include_bytes!("../../assets/music/combat_high.ogg"),
    ),
    (
        "sting_round",
        include_bytes!("../../assets/music/sting_round.ogg"),
    ),
    (
        "sting_best",
        include_bytes!("../../assets/music/sting_best.ogg"),
    ),
    (
        "death_chord",
        include_bytes!("../../assets/music/death_chord.ogg"),
    ),
];

/// The in-game credit (Settings, under the sliders): every music author, the
/// source and the license. `assets/ASSETS.md` has the full attribution lines.
pub const MUSIC_CREDIT: &str = "Music: humanoide9000 and Sheyvan on freesound.org (CC BY 4.0), \
     trimmed and looped. Celesta waltz and sound effects: made for Pieced.";

/// `assets/music/music.json`: every file's length and loop start (frames),
/// written by `scripts/build-music.sh`.
pub const MUSIC_MANIFEST: &str = include_str!("../../assets/music/music.json");

/// One `music.json` entry.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MusicEntry {
    pub name: String,
    pub frames: usize,
    /// Frame playback loops back to; −1 for a one-shot.
    pub loop_start: i64,
}

impl MusicEntry {
    pub fn loop_start(&self) -> Option<usize> {
        usize::try_from(self.loop_start).ok()
    }
}

pub fn parse_manifest(text: &str) -> Result<Vec<MusicEntry>, serde_json::Error> {
    serde_json::from_str(text)
}

// ---------------------------------------------------------------------------
// Slots, stings and tracks
// ---------------------------------------------------------------------------

/// A looping slot of the score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MusicSlot {
    Menu,
    Break,
    CombatLow,
    CombatHigh,
}

impl MusicSlot {
    pub const ALL: [MusicSlot; 4] = [
        MusicSlot::Menu,
        MusicSlot::Break,
        MusicSlot::CombatLow,
        MusicSlot::CombatHigh,
    ];

    pub fn track(self) -> Track {
        match self {
            MusicSlot::Menu => Track::Menu,
            MusicSlot::Break => Track::Break,
            MusicSlot::CombatLow => Track::CombatLow,
            MusicSlot::CombatHigh => Track::CombatHigh,
        }
    }
}

/// A one-shot over the loops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sting {
    /// Every wave start (synced with chunk 5's banner through [`WaveStarted`]).
    RoundStart,
    /// The player's elimination: a deflating brass fall.
    Death,
    /// "NEW BEST!" on the results.
    NewBest,
}

impl Sting {
    pub const ALL: [Sting; 3] = [Sting::RoundStart, Sting::Death, Sting::NewBest];

    pub fn track(self) -> Track {
        match self {
            Sting::RoundStart => Track::StingRound,
            Sting::Death => Track::StingDeath,
            Sting::NewBest => Track::StingBest,
        }
    }
}

/// Every piece of music, loaded or made at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Track {
    Menu,
    Break,
    CombatLow,
    CombatHigh,
    StingRound,
    StingDeath,
    StingBest,
}

impl Track {
    /// In load order: the menu first (it plays as soon as Boot ends), then
    /// what a run needs first.
    pub const ALL: [Track; 7] = [
        Track::Menu,
        Track::StingRound,
        Track::CombatLow,
        Track::Break,
        Track::CombatHigh,
        Track::StingDeath,
        Track::StingBest,
    ];

    /// The `music.json` entry this track is decoded from (the break loop is
    /// synthesized; the death sting is derived from `death_chord`).
    pub fn file(self) -> Option<&'static str> {
        match self {
            Track::Menu => Some("menu"),
            Track::CombatLow => Some("combat_low"),
            Track::CombatHigh => Some("combat_high"),
            Track::StingRound => Some("sting_round"),
            Track::StingBest => Some("sting_best"),
            Track::StingDeath => Some("death_chord"),
            Track::Break => None,
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

// ---------------------------------------------------------------------------
// Clips: decoded PCM that loops sample-accurately
// ---------------------------------------------------------------------------

/// A piece of music as decoded 16-bit stereo PCM (half the memory of floats).
/// It plays through its own decoder, which loops from `loop_start` without a
/// seam; a one-shot ends.
#[derive(Asset, TypePath, Debug, Clone)]
pub struct MusicClip {
    pcm: Arc<[i16]>,
    loop_start: Option<usize>,
}

impl MusicClip {
    /// `pcm` is interleaved stereo at [`MUSIC_RATE`]; `loop_start` a frame.
    pub fn new(pcm: Vec<i16>, loop_start: Option<usize>) -> Self {
        let frames = pcm.len() / MUSIC_CHANNELS as usize;
        Self {
            pcm: pcm.into(),
            loop_start: loop_start.filter(|&f| f < frames),
        }
    }

    pub fn samples(&self) -> &[i16] {
        &self.pcm
    }

    pub fn frames(&self) -> usize {
        self.pcm.len() / MUSIC_CHANNELS as usize
    }

    pub fn seconds(&self) -> f32 {
        self.frames() as f32 / MUSIC_RATE as f32
    }

    pub fn loop_start(&self) -> Option<usize> {
        self.loop_start
    }

    /// The samples as they play from the top: `n` interleaved samples,
    /// looping round (for tests: the seam as the player hears it).
    pub fn play(&self, n: usize) -> Vec<f32> {
        self.decoder().take(n).collect()
    }
}

/// Streams a [`MusicClip`]: one sample per call, looping in place.
pub struct ClipDecoder {
    pcm: Arc<[i16]>,
    pos: usize,
    loop_at: Option<usize>,
}

impl Iterator for ClipDecoder {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        if self.pos >= self.pcm.len() {
            self.pos = self.loop_at?;
        }
        let s = self.pcm.get(self.pos).copied()?;
        self.pos += 1;
        Some(s as f32 / 32_768.0)
    }
}

impl Source for ClipDecoder {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(MUSIC_CHANNELS).unwrap_or(ChannelCount::MIN)
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(MUSIC_RATE).unwrap_or(SampleRate::MIN)
    }

    fn total_duration(&self) -> Option<Duration> {
        match self.loop_at {
            Some(_) => None,
            None => Some(Duration::from_secs_f64(
                self.pcm.len() as f64 / (MUSIC_RATE as f64 * MUSIC_CHANNELS as f64),
            )),
        }
    }
}

impl Decodable for MusicClip {
    type Decoder = ClipDecoder;

    fn decoder(&self) -> ClipDecoder {
        ClipDecoder {
            pcm: self.pcm.clone(),
            pos: 0,
            loop_at: self.loop_start.map(|f| f * MUSIC_CHANNELS as usize),
        }
    }
}

// ---------------------------------------------------------------------------
// Loading (background thread)
// ---------------------------------------------------------------------------

/// Decodes an embedded Ogg Vorbis file to 16-bit stereo PCM, exactly `frames`
/// long (the encoder pads the last block; the padding is dropped). `None` if
/// it isn't 44.1 kHz stereo or is shorter than `frames`.
pub fn decode_ogg(bytes: &[u8], frames: usize) -> Option<Vec<i16>> {
    let source = AudioSource {
        bytes: Arc::from(bytes),
    };
    let decoder =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| source.decoder())).ok()?;
    if decoder.channels().get() != MUSIC_CHANNELS || decoder.sample_rate().get() != MUSIC_RATE {
        return None;
    }
    let n = frames * MUSIC_CHANNELS as usize;
    let mut pcm = Vec::with_capacity(n);
    pcm.extend(decoder.take(n).map(celesta::to_i16));
    (pcm.len() == n).then_some(pcm)
}

/// Decodes (or synthesizes) one track. Runs on the music thread.
pub fn render_track(track: Track, manifest: &[MusicEntry]) -> Option<MusicClip> {
    if track == Track::Break {
        return Some(MusicClip::new(celesta::render_break_loop(), Some(0)));
    }
    let name = track.file()?;
    let entry = manifest.iter().find(|e| e.name == name)?;
    let bytes = MUSIC_FILES.iter().find(|(n, _)| *n == name)?.1;
    let pcm = decode_ogg(bytes, entry.frames)?;
    Some(match track {
        Track::StingDeath => MusicClip::new(death_sting(&pcm), None),
        _ => MusicClip::new(pcm, entry.loop_start()),
    })
}

/// Makes every track in [`Track::ALL`] order, handing each over as it's done.
pub fn render_all(mut deliver: impl FnMut(Track, MusicClip)) {
    let Ok(manifest) = parse_manifest(MUSIC_MANIFEST) else {
        return;
    };
    for track in Track::ALL {
        if let Some(clip) = render_track(track, &manifest) {
            deliver(track, clip);
        }
    }
}

/// The music thread: started when the plugin builds, it hands each track to
/// the main thread as soon as it's ready.
#[derive(Resource)]
pub struct MusicLoader {
    rx: Mutex<mpsc::Receiver<(Track, MusicClip)>>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl MusicLoader {
    /// Starts the music thread (named `music-bank`).
    pub fn spawn() -> Option<Self> {
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("music-bank".into())
            .spawn(move || {
                render_all(|track, clip| {
                    let _ = tx.send((track, clip));
                })
            })
            .ok()?;
        Some(Self {
            rx: Mutex::new(rx),
            thread: Mutex::new(Some(thread)),
        })
    }

    /// The thread's name (it's never the main thread).
    pub fn thread_name(&self) -> Option<String> {
        let thread = self.thread.lock().ok()?;
        thread.as_ref()?.thread().name().map(str::to_owned)
    }

    /// Waits for the thread to finish (tests, and the budget probes, which
    /// want every track filed before they measure).
    pub fn wait(&self) {
        if let Some(thread) = self.thread.lock().ok().and_then(|mut t| t.take()) {
            let _ = thread.join();
        }
    }
}

/// Files everything the music thread has finished: the clip as an asset, and
/// its pooled voice (paused and silent until the director starts it).
pub(super) fn receive_music(
    mut commands: Commands,
    loader: Option<Res<MusicLoader>>,
    mut clips: ResMut<Assets<MusicClip>>,
    mut bank: ResMut<MusicBank>,
    mut voices: ResMut<MusicVoices>,
) {
    let Some(loader) = loader else {
        return;
    };
    let Ok(rx) = loader.rx.lock() else {
        return;
    };
    loop {
        match rx.try_recv() {
            Ok((track, clip)) => {
                bank.seconds[track.index()] = clip.seconds();
                let handle = clips.add(clip);
                let voice = commands
                    .spawn((
                        Name::new("Music"),
                        MusicVoice { track },
                        AudioPlayer::<MusicClip>(handle.clone()),
                        PlaybackSettings {
                            mode: PlaybackMode::Once,
                            paused: true,
                            volume: Volume::SILENT,
                            ..PlaybackSettings::ONCE
                        },
                    ))
                    .id();
                bank.clips[track.index()] = Some(handle);
                voices.entities[track.index()] = Some(voice);
            }
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => {
                commands.remove_resource::<MusicLoader>();
                break;
            }
        }
    }
}

/// Insert before adding the audio plugin to load the music in a headless app
/// (the game loads it whenever it has an audio output).
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct LoadMusic;

/// Every loaded track.
#[derive(Resource, Debug, Default)]
pub struct MusicBank {
    clips: [Option<Handle<MusicClip>>; 7],
    seconds: [f32; 7],
}

impl MusicBank {
    pub fn get(&self, track: Track) -> Option<&Handle<MusicClip>> {
        self.clips[track.index()].as_ref()
    }

    pub fn is_loaded(&self, track: Track) -> bool {
        self.clips[track.index()].is_some()
    }

    /// The track's length (0 until it's loaded).
    pub fn seconds(&self, track: Track) -> f32 {
        self.seconds[track.index()]
    }

    /// Every track is loaded.
    pub fn is_complete(&self) -> bool {
        self.clips.iter().all(Option::is_some)
    }
}

/// A pooled music voice: one per track, made when the track loads.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MusicVoice {
    pub track: Track,
}

/// The pooled voices, and which are playing.
#[derive(Resource, Debug, Default)]
pub struct MusicVoices {
    entities: [Option<Entity>; 7],
    playing: [bool; 7],
}

impl MusicVoices {
    pub fn is_playing(&self, track: Track) -> bool {
        self.playing[track.index()]
    }
}

// ---------------------------------------------------------------------------
// The death sting (derived at load)
// ---------------------------------------------------------------------------

/// The stab before the fall (s).
pub const DEATH_STAB: f32 = 0.30;
/// The fall (s): the chord sags and darkens as it fades.
pub const DEATH_FALL: f32 = 1.6;
/// How far it falls (semitones).
pub const DEATH_SEMITONES: f32 = 8.0;

/// The death sting: a sustained brass C7 chord from the battle cue (freesound
/// 685841, cut by `build-music.sh`) held for a stab, then bent down
/// [`DEATH_SEMITONES`] over [`DEATH_FALL`] (a tape slowing down, easing in),
/// darkening through a closing low-pass and fading, in a small hall: an
/// orchestra deflating, not a comic trombone. Stereo PCM in and out.
pub fn death_sting(chord: &[i16]) -> Vec<i16> {
    let sr = MUSIC_RATE as f32;
    let frames = chord.len() / 2;
    let at = |i: isize, c: usize| -> f32 {
        let i = i.clamp(0, frames as isize - 1) as usize;
        chord[2 * i + c] as f32 / 32_768.0
    };
    let out_frames = ((DEATH_STAB + DEATH_FALL + 0.05) * sr) as usize;
    let tail = (0.5 * sr) as usize;
    let mut l = vec![0.0f32; out_frames + tail];
    let mut r = vec![0.0f32; out_frames + tail];
    let mut lp = [Svf::default(), Svf::default()];
    let mut pos = 0.0f64;
    for n in 0..out_frames {
        let t = n as f32 / sr;
        let u = ((t - DEATH_STAB) / DEATH_FALL).clamp(0.0, 1.0);
        let rate = 2f32.powf(-DEATH_SEMITONES * u.powf(1.7) / 12.0);
        let gain = (1.0 - u).powf(1.3) * ((out_frames - n) as f32 / (0.02 * sr)).min(1.0);
        let cutoff = 9000.0 * (700.0f32 / 9000.0).powf(u);
        // Catmull–Rom interpolation between source frames.
        let i = pos.floor() as isize;
        let f = (pos - i as f64) as f32;
        for (c, out) in [&mut l, &mut r].into_iter().enumerate() {
            let (p0, p1, p2, p3) = (at(i - 1, c), at(i, c), at(i + 1, c), at(i + 2, c));
            let s = p1
                + 0.5
                    * f
                    * (p2 - p0
                        + f * (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3
                            + f * (3.0 * (p1 - p2) + p3 - p0)));
            out[n] = gain * lp[c].low(s, cutoff);
        }
        pos += rate as f64;
    }
    // A small hall so the fall dies away instead of stopping.
    let room = Room {
        rt60: 1.4,
        size: 1.5,
        damping_hz: 3000.0,
        wet_db: 0.0,
        tail: 0.0,
        predelay: 0.0,
    };
    let mut fdn = Fdn::new(&room);
    let wet = db_to_gain(-15.0);
    for n in 0..l.len() {
        let (wl, wr) = fdn.process(0.5 * (l[n] + r[n]));
        l[n] += wet * wl;
        r[n] += wet * wr;
    }
    let fade = (0.05 * sr) as usize;
    let len = l.len();
    for k in 0..fade {
        let g = k as f32 / fade as f32;
        l[len - 1 - k] *= g;
        r[len - 1 - k] *= g;
    }
    let interleaved: Vec<f32> = l.iter().zip(&r).flat_map(|(a, b)| [*a, *b]).collect();
    let gain = db_to_gain(STING_LUFS - integrated_lufs(&interleaved, 2, MUSIC_RATE));
    interleaved
        .into_iter()
        .map(|s| celesta::to_i16(soft_limit(s * gain)))
        .collect()
}

// ---------------------------------------------------------------------------
// Tuning
// ---------------------------------------------------------------------------

/// The score's designer numbers (a `#[serde(skip)]` section of [`Tuning`]:
/// never persisted, so play-test tuning always reaches Jake's game). The
/// Music and Effects sliders are menu settings in `AudioTuning`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MusicTuning {
    /// Seconds for one slot to crossfade into the next (equal power).
    pub crossfade_seconds: f32,
    /// Combat goes high from this wave on...
    pub high_wave: u32,
    /// ...or once this many knights are alive at once (for the rest of that
    /// wave, so it never flips back and forth).
    pub high_alive: u32,
    /// Gain on the loops while a sting plays (0.35 ≈ −9 dB).
    pub sting_duck: f32,
    /// Seconds for the loops to dip under a sting, and to come back after it.
    pub sting_attack: f32,
    pub sting_release: f32,
    /// Gain on the loops for a big hit (0.55 ≈ −5 dB), held this long (s),
    /// then released over `hit_release` (s).
    pub hit_duck: f32,
    pub hit_duck_seconds: f32,
    pub hit_release: f32,
    /// Gain on the loops while paused.
    pub pause_gain: f32,
    /// Each slot's mix level (dB), in [`MusicSlot::ALL`] order: the files are
    /// all −18 LUFS; the score sits under the effects.
    pub slot_db: [f32; 4],
    /// The stings' mix level (dB): the files are −16 LUFS.
    pub sting_db: f32,
}

impl Default for MusicTuning {
    fn default() -> Self {
        Self {
            crossfade_seconds: 1.5,
            high_wave: 6,
            high_alive: 6,
            sting_duck: 0.35,
            sting_attack: 0.12,
            sting_release: 0.9,
            hit_duck: 0.55,
            hit_duck_seconds: 0.15,
            hit_release: 0.12,
            pause_gain: 0.45,
            slot_db: [-4.0, -5.0, -7.0, -7.0],
            sting_db: -5.0,
        }
    }
}

// ---------------------------------------------------------------------------
// The director (pure)
// ---------------------------------------------------------------------------

/// What the player is looking at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Screen {
    /// Loading: silence.
    #[default]
    Boot,
    Menu,
    Playing,
    Paused,
}

impl Screen {
    pub fn from_state(state: AppState) -> Self {
        match state {
            AppState::Boot => Screen::Boot,
            AppState::Menu => Screen::Menu,
            AppState::Playing => Screen::Playing,
            AppState::Paused => Screen::Paused,
        }
    }
}

/// The run as the score sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunView {
    pub seed: u64,
    pub phase: RunPhase,
    pub wave: u32,
    /// Knights in play (landed, not downed).
    pub alive: u32,
    pub new_best: bool,
}

/// Everything the director reads each frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MusicInput {
    pub screen: Screen,
    /// The Waves run (none in Practice or on the menu).
    pub run: Option<RunView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Fighting,
    Break,
    Dying,
    Over,
}

fn phase(p: RunPhase) -> Phase {
    match p {
        RunPhase::Fighting => Phase::Fighting,
        RunPhase::Break { .. } => Phase::Break,
        RunPhase::Dying { .. } => Phase::Dying,
        RunPhase::Over { .. } => Phase::Over,
    }
}

/// The slot for what's on screen (`high`: the combat is intense).
pub fn slot_for(input: &MusicInput, high: bool) -> Option<MusicSlot> {
    match input.screen {
        Screen::Boot => None,
        Screen::Menu => Some(MusicSlot::Menu),
        Screen::Playing | Screen::Paused => match input.run {
            // Practice: the calm loop.
            None => Some(MusicSlot::Break),
            Some(run) => match phase(run.phase) {
                Phase::Fighting if high => Some(MusicSlot::CombatHigh),
                Phase::Fighting => Some(MusicSlot::CombatLow),
                Phase::Break | Phase::Over => Some(MusicSlot::Break),
                // The death beat: the loops fall away under the death sting.
                Phase::Dying => None,
            },
        },
    }
}

/// What the director asks for this frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MusicFrame {
    /// Each slot's gain before the volume sliders ([`MusicSlot::ALL`] order):
    /// the crossfade, the slot's mix, the ducks and the pause dip.
    pub gains: [f32; 4],
    /// Which slots have a voice playing (a slot fading out keeps its voice
    /// until it's silent).
    pub voices: [bool; 4],
    /// Slots starting from silence this frame: play them from the top.
    pub restart: [bool; 4],
    /// A sting to start now.
    pub sting: Option<Sting>,
    /// The stings' gain before the volume sliders.
    pub sting_gain: f32,
    /// A wave started this frame (its number).
    pub wave_started: Option<u32>,
    /// The loops' combined duck (1 = none).
    pub duck: f32,
    /// The slot the score is heading to.
    pub target: Option<MusicSlot>,
}

/// The adaptive score's state machine: slot choice, crossfades, stings and
/// ducking. Fixed-size, so stepping it never allocates.
#[derive(Debug, Clone, PartialEq)]
pub struct MusicDirector {
    /// Crossfade position per slot (0 silent .. 1 full), linear in time.
    level: [f32; 4],
    voice: [bool; 4],
    /// The last run state seen in play: `(seed, phase, wave)`.
    prev: Option<(u64, Phase, u32)>,
    /// `(seed, wave)` that went high on its knights alive.
    high: Option<(u64, u32)>,
    sting_left: f32,
    sting_env: f32,
    hit_left: f32,
    hit_env: f32,
    pause_env: f32,
}

impl Default for MusicDirector {
    fn default() -> Self {
        Self {
            level: [0.0; 4],
            voice: [false; 4],
            prev: None,
            high: None,
            sting_left: 0.0,
            sting_env: 1.0,
            hit_left: 0.0,
            hit_env: 1.0,
            pause_env: 1.0,
        }
    }
}

/// Moves `x` toward `target` by at most `step`.
fn approach(x: f32, target: f32, step: f32) -> f32 {
    if x < target {
        (x + step).min(target)
    } else {
        (x - step).max(target)
    }
}

impl MusicDirector {
    /// Advances `dt` seconds of real time. `sting_seconds` are the stings'
    /// lengths in [`Sting::ALL`] order (0 while not loaded: nothing plays or
    /// ducks); `big_hit` is a big hit's sound starting this frame.
    pub fn step(
        &mut self,
        dt: f32,
        input: &MusicInput,
        t: &MusicTuning,
        sting_seconds: [f32; 3],
        big_hit: bool,
    ) -> MusicFrame {
        let dt = dt.max(0.0);
        // The run's beats: only while it's being played (a pause freezes it;
        // the menu has no run).
        let mut sting = None;
        let mut wave_started = None;
        match input.screen {
            Screen::Playing => {
                let now = input.run.map(|r| (r.seed, phase(r.phase), r.wave));
                if let (Some(run), Some((seed, now_phase, wave))) = (input.run, now) {
                    let prev = self.prev.filter(|p| p.0 == seed);
                    let was = prev.map(|p| p.1);
                    if now_phase == Phase::Fighting && was != Some(Phase::Fighting) {
                        wave_started = Some(wave);
                        sting = Some(Sting::RoundStart);
                    }
                    if now_phase == Phase::Dying
                        && matches!(was, Some(Phase::Fighting | Phase::Break))
                    {
                        sting = Some(Sting::Death);
                    }
                    if now_phase == Phase::Over
                        && run.new_best
                        && was.is_some_and(|w| w != Phase::Over)
                    {
                        sting = Some(Sting::NewBest);
                    }
                    if now_phase == Phase::Fighting && run.alive >= t.high_alive {
                        self.high = Some((seed, wave));
                    }
                }
                self.prev = now;
            }
            Screen::Menu => self.prev = None,
            Screen::Boot | Screen::Paused => {}
        }
        let high = input
            .run
            .is_some_and(|r| r.wave >= t.high_wave || self.high == Some((r.seed, r.wave)));
        let target = slot_for(input, high);

        // Stings and the ducks.
        if let Some(s) = sting {
            let len = sting_seconds[Sting::ALL.iter().position(|x| *x == s).unwrap_or(0)];
            if len > 0.0 {
                self.sting_left = len;
            } else {
                sting = None;
            }
        } else {
            self.sting_left = (self.sting_left - dt).max(0.0);
        }
        let depth = (1.0 - t.sting_duck).max(0.0);
        let sting_target = if self.sting_left > 0.0 {
            t.sting_duck
        } else {
            1.0
        };
        let rate = if sting_target < self.sting_env {
            t.sting_attack
        } else {
            t.sting_release
        };
        self.sting_env = approach(self.sting_env, sting_target, depth * dt / rate.max(1e-3));
        if big_hit {
            self.hit_left = t.hit_duck_seconds;
            self.hit_env = t.hit_duck;
        } else if self.hit_left > 0.0 {
            self.hit_left -= dt;
        }
        if self.hit_left <= 0.0 {
            let depth = (1.0 - t.hit_duck).max(0.0);
            self.hit_env = approach(self.hit_env, 1.0, depth * dt / t.hit_release.max(1e-3));
        }
        let pause_target = if input.screen == Screen::Paused {
            t.pause_gain
        } else {
            1.0
        };
        self.pause_env = approach(self.pause_env, pause_target, dt / 0.25);
        let duck = self.sting_env.min(self.hit_env);

        // The crossfade.
        let step = dt / t.crossfade_seconds.max(0.01);
        let mut frame = MusicFrame {
            sting,
            sting_gain: db_to_gain(t.sting_db),
            wave_started,
            duck,
            target,
            ..default()
        };
        for (i, slot) in MusicSlot::ALL.into_iter().enumerate() {
            let wanted = target == Some(slot);
            if wanted && !self.voice[i] {
                self.voice[i] = true;
                frame.restart[i] = true;
            }
            self.level[i] = approach(self.level[i], if wanted { 1.0 } else { 0.0 }, step);
            if !wanted && self.level[i] <= 0.0 {
                self.voice[i] = false;
            }
            frame.voices[i] = self.voice[i];
            frame.gains[i] = (self.level[i] * FRAC_PI_2).sin()
                * db_to_gain(t.slot_db[i])
                * duck
                * self.pause_env;
        }
        frame
    }
}

/// The score's volume: master × Music (0 when muted or either is 0).
pub fn music_volume(audio: &super::AudioTuning) -> f32 {
    audio.effective_master() * audio.music_volume.clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// A wave just started (its number): the round sting plays with it, and
/// chunk 5's banner can sync to it.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaveStarted {
    pub wave: u32,
}

/// Cues from the effects to the score: a big hit's sound started this frame.
#[derive(Resource, Debug, Default)]
pub struct MusicCues {
    pub big_hit: bool,
}

/// The director and what it asked for last frame (for the session log and
/// tests).
#[derive(Resource, Debug, Default)]
pub struct MusicState {
    pub director: MusicDirector,
    pub last: MusicFrame,
}

pub(super) fn build(app: &mut App) {
    if app.is_plugin_added::<bevy::audio::AudioPlugin>() {
        use bevy::audio::AddAudioSource;
        app.add_audio_source::<MusicClip>();
    } else {
        app.init_asset::<MusicClip>();
    }
    // The music thread starts with the game's audio output (or when a headless
    // test asks for it with [`LoadMusic`]).
    let wanted = app.is_plugin_added::<bevy::audio::AudioPlugin>()
        || app.world().contains_resource::<LoadMusic>();
    if wanted && let Some(loader) = MusicLoader::spawn() {
        app.insert_resource(loader);
    }
    app.init_resource::<MusicBank>()
        .init_resource::<MusicVoices>()
        .init_resource::<MusicCues>()
        .init_resource::<MusicState>()
        .add_message::<WaveStarted>();
}

/// Reads the game into a [`MusicInput`].
fn music_input(
    state: Option<&State<AppState>>,
    summary: Option<&RunSummary>,
    run: Option<&Run>,
) -> MusicInput {
    MusicInput {
        screen: state.map_or(Screen::Boot, |s| Screen::from_state(*s.get())),
        run: summary.map(|s| RunView {
            seed: s.seed,
            phase: s.phase,
            wave: s.wave,
            alive: run.map_or(0, |r| r.alive),
            new_best: s.new_best,
        }),
    }
}

type VoiceQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut PlaybackSettings,
        Option<&'static mut AudioSink>,
    ),
    With<MusicVoice>,
>;

/// Plays, restarts, re-levels or silences one pooled voice. `gain: None`
/// silences and pauses it; a restart (or a voice that wasn't playing) plays
/// it from the top by giving it a fresh sink.
fn control(
    voices: &mut MusicVoices,
    players: &mut VoiceQuery,
    commands: &mut Commands,
    track: Track,
    gain: Option<f32>,
    restart: bool,
) {
    let i = track.index();
    let Some(entity) = voices.entities[i] else {
        return;
    };
    let Ok((mut settings, sink)) = players.get_mut(entity) else {
        return;
    };
    match gain {
        Some(gain) if restart || !voices.playing[i] => {
            settings.paused = false;
            settings.volume = Volume::Linear(gain);
            commands.entity(entity).remove::<AudioSink>();
            voices.playing[i] = true;
        }
        Some(gain) => {
            if let Some(mut sink) = sink {
                sink.set_volume(Volume::Linear(gain));
            }
        }
        None if voices.playing[i] => {
            if let Some(mut sink) = sink {
                sink.set_volume(Volume::SILENT);
                sink.pause();
            }
            settings.paused = true;
            settings.volume = Volume::SILENT;
            voices.playing[i] = false;
        }
        None => {}
    }
}

/// Steps the director and applies it to the pooled voices. Runs after the
/// effects, so a big hit ducks the score on its own frame.
#[allow(clippy::too_many_arguments)]
pub(super) fn drive_music(
    time: Res<Time<Real>>,
    tuning: Res<Tuning>,
    state: Option<Res<State<AppState>>>,
    summary: Option<Res<RunSummary>>,
    run: Option<Res<Run>>,
    bank: Res<MusicBank>,
    mut voices: ResMut<MusicVoices>,
    mut music: ResMut<MusicState>,
    mut cues: ResMut<MusicCues>,
    mut players: VoiceQuery,
    mut commands: Commands,
    mut started: MessageWriter<WaveStarted>,
) {
    let input = music_input(state.as_deref(), summary.as_deref(), run.as_deref());
    let lengths = Sting::ALL.map(|s| bank.seconds(s.track()));
    let big_hit = std::mem::take(&mut cues.big_hit);
    let frame = music
        .director
        .step(time.delta_secs(), &input, &tuning.music, lengths, big_hit);
    let volume = music_volume(&tuning.audio);
    let (voices, players, commands) = (&mut *voices, &mut players, &mut commands);
    for (i, slot) in MusicSlot::ALL.into_iter().enumerate() {
        let gain = frame.voices[i].then_some(frame.gains[i] * volume);
        control(
            voices,
            players,
            commands,
            slot.track(),
            gain,
            frame.restart[i],
        );
    }
    let sting_gain = frame.sting_gain * volume;
    for sting in Sting::ALL {
        let track = sting.track();
        if frame.sting == Some(sting) {
            control(voices, players, commands, track, Some(sting_gain), true);
        } else if frame.sting.is_some() {
            // A new sting cuts the last one off.
            control(voices, players, commands, track, None, false);
        } else if voices.is_playing(track) {
            control(voices, players, commands, track, Some(sting_gain), false);
        }
    }
    if let Some(wave) = frame.wave_started {
        started.write(WaveStarted { wave });
    }
    music.last = frame;
}
