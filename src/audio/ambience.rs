//! The ambient soundscape (M4 chunk 6, D123): the island is never silent.
//!
//! Three looping beds, synthesized at load on their own thread
//! (`ambience-bank`) and played by one pooled voice each:
//!
//! - **Wind** ([`wind_bed`]): a soft stereo breeze (two decorrelated noises
//!   under a slowly moving low-pass, and a faint whistle). Its level follows
//!   the gusts of the same wind field the grass and trees sway in
//!   ([`crate::look::wind::gust_at`] at the listener).
//! - **The castle** ([`castle_bed`]): far off toward the castle, a soft choir
//!   holding a slow C–Am–F–G "ooh-aah" progression (the knights' formant
//!   voice, [`super::voice`], sung long), a bell tolling twice per loop, and
//!   the paper flutter of its sky lanterns, darkened by distance and bathed in
//!   a hall. It is spatial: heard from the castle's bearing.
//! - **Tension** ([`drone_bed`]): a low drone (D and A with a rubbing E♭ and
//!   a slow pulse) that swells in while a wave is being fought, more with more
//!   knights alive, and fades in the break.
//!
//! And one-shots: now and then a songbird sings from one of the trees near
//! you ([`Sfx::Birdsong`], spatial), less often once the fighting is thick.
//!
//! **Loops** are seamless: each bed renders its loop plus a crossfade's worth
//! more and folds that onto its start (equal power), and every slow
//! modulation completes whole cycles per loop.
//!
//! **Mix** ([`AmbienceDirector`], pure): well under the score and the
//! effects, all on the Effects slider (silent at 0 or muted). While an
//! off-screen wind-up warning is guarded ([`super::barks::WARNING_GUARD`])
//! every bed ducks, so nothing masks it (D76). Stepping the director and the
//! voices allocates nothing.

use super::{
    PlayQueue, Sfx,
    barks::KnightVoices,
    celesta::to_i16,
    loudness::integrated_lufs,
    music::{MUSIC_CHANNELS, MUSIC_RATE, MusicClip, Screen},
    reverb::{Fdn, Room},
    synth::{Noise, Osc, SAMPLE_RATE, Svf, db_to_gain, note, soft_limit},
    voice::{self, Syllable},
};
use crate::{
    arena::visuals::island::IslandLayout,
    far::layout::FarLayout,
    look::wind,
    render::MainCamera,
    rng::Rng,
    shared::AppState,
    tuning::Tuning,
    waves::{Run, RunPhase, RunSummary},
};
use bevy::{
    audio::{AudioSink, AudioSinkPlayback, PlaybackMode, SpatialAudioSink, SpatialScale, Volume},
    prelude::*,
};
use std::{
    f32::consts::{FRAC_PI_2, TAU},
    sync::{Mutex, mpsc},
};

const SR: f32 = SAMPLE_RATE as f32;

/// Every bed's loudness when rendered (LUFS); the mix sets how loud it plays.
pub const BED_LUFS: f32 = -20.0;
/// The beds' mix (dB under their rendered loudness): the wind at full gust,
/// the castle, and the drone at full tension. Together they sit well under
/// the score (−18 LUFS files at −5..−7 dB) and the effects.
pub const WIND_DB: f32 = -9.0;
pub const CASTLE_DB: f32 = -11.0;
pub const DRONE_DB: f32 = -10.0;
/// How far the beds dip while an off-screen warning is guarded.
pub const WARNING_DUCK: f32 = 0.3;
/// The castle bed is heard from this far along the castle's bearing (m):
/// inside the spatial scale's full-volume radius, so only its direction
/// changes, not its level.
pub const CASTLE_EAR_DISTANCE: f32 = 8.0;
/// Seconds between songbirds (seeded, uniform), and how far a singing tree
/// may be (m).
pub const BIRD_GAP: (f32, f32) = (4.0, 11.0);
pub const BIRD_RANGE: f32 = 45.0;

/// A looping bed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bed {
    Wind,
    Castle,
    Drone,
}

impl Bed {
    pub const ALL: [Bed; 3] = [Bed::Wind, Bed::Castle, Bed::Drone];

    /// The loop's length (s).
    pub fn seconds(self) -> f32 {
        match self {
            Bed::Wind => 12.0,
            Bed::Castle => 16.0,
            Bed::Drone => 12.0,
        }
    }

    pub fn frames(self) -> usize {
        (self.seconds() * SR) as usize
    }

    /// Renders the loop: interleaved stereo 16-bit PCM, exactly
    /// [`Bed::frames`] long, at [`BED_LUFS`].
    pub fn render(self) -> Vec<i16> {
        match self {
            Bed::Wind => wind_bed(),
            Bed::Castle => castle_bed(),
            Bed::Drone => drone_bed(),
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering (the ambience thread)
// ---------------------------------------------------------------------------

/// Crossfade (s) folded from past the loop's end onto its start.
const FOLD: f32 = 1.5;

/// Folds `l`/`r` (longer than `frames` by at least the fold) into a seamless
/// loop of `frames`: the part past the end fades out over the start while the
/// start fades in (equal power), so the last sample runs straight into the
/// first. Sounds already ringing at the end (the hall's tail) carry on.
fn fold_loop(l: &[f32], r: &[f32], frames: usize) -> (Vec<f32>, Vec<f32>) {
    let fold = ((FOLD * SR) as usize).min(l.len() - frames);
    let mut ol = l[..frames].to_vec();
    let mut or = r[..frames].to_vec();
    for i in 0..fold {
        let x = i as f32 / fold as f32;
        let (fin, fout) = ((x * FRAC_PI_2).sin(), (x * FRAC_PI_2).cos());
        ol[i] = ol[i] * fin + l[frames + i] * fout;
        or[i] = or[i] * fin + r[frames + i] * fout;
    }
    (ol, or)
}

/// Loudness-normalizes to [`BED_LUFS`], soft-limits and interleaves.
fn master_bed(l: &[f32], r: &[f32]) -> Vec<i16> {
    let interleaved: Vec<f32> = l.iter().zip(r).flat_map(|(a, b)| [*a, *b]).collect();
    let gain = db_to_gain(BED_LUFS - integrated_lufs(&interleaved, 2, SAMPLE_RATE));
    interleaved
        .iter()
        .map(|s| to_i16(soft_limit(s * gain)))
        .collect()
}

/// A modulation that completes `cycles` whole cycles per `loop_s`.
fn lfo(t: f32, cycles: f32, loop_s: f32, phase: f32) -> f32 {
    (TAU * cycles * t / loop_s + phase).sin()
}

/// The wind bed: two decorrelated pink-ish noises (left and right) under a
/// low-pass that breathes between ~350 and ~900 Hz, a high airy hiss, and a
/// faint whistle wandering around 800 Hz.
pub fn wind_bed() -> Vec<i16> {
    let bed = Bed::Wind;
    let len = bed.seconds();
    let n = bed.frames() + (FOLD * SR) as usize;
    let mut l = vec![0.0f32; n];
    let mut r = vec![0.0f32; n];
    for (ch, out) in [&mut l, &mut r].into_iter().enumerate() {
        let mut noise = Noise::new(0xA1 + ch as u64);
        let mut lp1 = Svf::default();
        let mut lp2 = Svf::default();
        let mut air = Svf::default();
        let mut whistle = Svf::default();
        let mut pink = 0.0f32;
        let phase = ch as f32 * 1.3;
        for (i, s) in out.iter_mut().enumerate() {
            let t = i as f32 / SR;
            let w = noise.signed();
            // A leaky integrator tilts white noise toward pink/brown.
            pink = 0.985 * pink + 0.12 * w;
            let cutoff = 600.0 + 260.0 * lfo(t, 3.0, len, phase) + 90.0 * lfo(t, 7.0, len, 0.5);
            let body = lp2.low(lp1.low(pink, cutoff), cutoff * 1.4);
            let swell = 0.75 + 0.25 * lfo(t, 2.0, len, phase + 0.8);
            let hiss = 0.05 * air.band(w, 3500.0, 0.7) * (0.6 + 0.4 * lfo(t, 5.0, len, phase));
            let tone = 790.0 + 140.0 * lfo(t, 1.0, len, phase) + 40.0 * lfo(t, 4.0, len, 0.0);
            let whistling = 0.9 * whistle.band(w, tone, 14.0) / 14.0
                * (0.5 + 0.5 * lfo(t, 3.0, len, phase + 2.0)).powi(2);
            *s = body * swell + hiss + whistling;
        }
    }
    let (l, r) = fold_loop(&l, &r, bed.frames());
    master_bed(&l, &r)
}

/// A soft sung pad chord: `midis`, "ooh" opening to "aah", at `start` for
/// `secs` with a slow swell, panned across the stereo field.
fn choir_chord(l: &mut [f32], r: &mut [f32], start: f32, secs: f32, midis: &[f32], seed: u64) {
    for (k, &m) in midis.iter().enumerate() {
        let mut b = super::synth::Buffer::new(secs + 0.2);
        // Two slightly detuned singers per part: a chorus.
        for (j, detune) in [0.997f32, 1.004].into_iter().enumerate() {
            let f = note(m) * detune;
            let s = Syllable::new(0.02 * j as f32, secs, voice::OO, (f, f * 1.001))
                .to(voice::AH)
                .vibrato(0.012);
            voice::sing(&mut b, &s, seed + 17 * k as u64 + j as u64);
        }
        let pan = (k as f32 / (midis.len().max(2) - 1) as f32) * 1.2 - 0.6;
        let (gl, gr) = (((1.0 - pan) * 0.5).sqrt(), ((1.0 + pan) * 0.5).sqrt());
        let s0 = (start * SR) as usize;
        for (i, x) in b.samples.iter().enumerate() {
            let t = i as f32 / SR;
            // A slow swell in and out.
            let env = (t / (0.45 * secs)).min(1.0).powf(1.5) * ((secs + 0.2 - t) / 1.2).clamp(0.0, 1.0);
            let y = x * env * 0.12;
            if let (Some(a), Some(b)) = (l.get_mut(s0 + i), r.get_mut(s0 + i)) {
                *a += y * gl;
                *b += y * gr;
            }
        }
    }
}

/// A distant church bell: inharmonic partials (hum, prime, tierce, quint,
/// nominal) with long decays.
fn bell(l: &mut [f32], r: &mut [f32], start: f32, f: f32, gain: f32) {
    let partials = [
        (0.5, 0.5, 3.5),
        (1.0, 1.0, 2.6),
        (1.19, 0.45, 2.0),
        (1.5, 0.35, 1.6),
        (2.0, 0.6, 1.3),
        (2.66, 0.2, 0.8),
    ];
    let s0 = (start * SR) as usize;
    let len = (4.0 * SR) as usize;
    for &(ratio, amp, tau) in &partials {
        let mut o = Osc::default();
        for i in 0..len {
            let t = i as f32 / SR;
            let env = (t / 0.004).min(1.0) * (-t / tau).exp();
            let y = gain * amp * o.sine(f * ratio) * env;
            if let (Some(a), Some(b)) = (l.get_mut(s0 + i), r.get_mut(s0 + i)) {
                *a += y * 0.9;
                *b += y;
            }
        }
    }
}

/// The castle bed: the choir, the bells and the lanterns' flutter, darkened
/// by distance and bathed in a big hall.
pub fn castle_bed() -> Vec<i16> {
    let bed = Bed::Castle;
    let frames = bed.frames();
    let n = frames + (5.0 * SR) as usize;
    let mut l = vec![0.0f32; n];
    let mut r = vec![0.0f32; n];
    // C – Am – F – G, four seconds each (the break loop is in A minor and the
    // stings in C: this sits between them).
    let chords: [[f32; 4]; 4] = [
        [60.0, 64.0, 67.0, 72.0],
        [57.0, 60.0, 64.0, 69.0],
        [53.0, 57.0, 60.0, 65.0],
        [55.0, 59.0, 62.0, 67.0],
    ];
    for (i, chord) in chords.iter().enumerate() {
        choir_chord(&mut l, &mut r, 4.0 * i as f32, 4.6, chord, 1000 + 50 * i as u64);
    }
    bell(&mut l, &mut r, 1.0, note(55.0), 0.05);
    bell(&mut l, &mut r, 9.0, note(48.0), 0.06);
    // The sky lanterns: a soft paper flutter of tiny crackles.
    let mut rng = Noise::new(0x1A7);
    let mut crackle = Svf::default();
    let mut next = 0usize;
    let mut level = 0.0f32;
    for i in 0..frames {
        if i >= next {
            level = rng.range(0.2, 1.0);
            next = i + (rng.range(0.01, 0.09) * SR) as usize;
        }
        let pop = crackle.band(rng.signed(), 1800.0, 1.2) * level * (-(((next - i) as f32) / (0.004 * SR))).exp();
        l[i] += 0.02 * pop;
        r[i] += 0.02 * pop * 0.8;
    }
    // Distance: the highs go; then the hall.
    for ch in [&mut l, &mut r] {
        let mut lp = Svf::default();
        for s in ch.iter_mut() {
            *s = lp.low(*s, 2600.0);
        }
    }
    let hall = Room {
        rt60: 3.0,
        size: 1.8,
        damping_hz: 3000.0,
        wet_db: 0.0,
        tail: 0.0,
        predelay: 0.0,
    };
    let mut fdn = Fdn::new(&hall);
    let wet = db_to_gain(-9.0);
    // Everything past the loop folds onto its start (as the break loop does),
    // and the hall runs over the loop twice, keeping the second pass, so it
    // is already ringing from the end as the start comes round.
    let mut ml = vec![0.0f32; frames];
    let mut mr = vec![0.0f32; frames];
    for i in 0..n {
        ml[i % frames] += l[i];
        mr[i % frames] += r[i];
    }
    let mut wl = vec![0.0f32; frames];
    let mut wr = vec![0.0f32; frames];
    for pass in 0..2 {
        for i in 0..frames {
            let (a, b) = fdn.process(0.5 * (ml[i] + mr[i]));
            if pass == 1 {
                wl[i] = a;
                wr[i] = b;
            }
        }
    }
    for i in 0..frames {
        ml[i] = 0.55 * ml[i] + wet * wl[i];
        mr[i] = 0.55 * mr[i] + wet * wr[i];
    }
    dc_block_loop(&mut ml);
    dc_block_loop(&mut mr);
    master_bed(&ml, &mr)
}

/// A circular DC block (two passes, keeping the second).
fn dc_block_loop(ch: &mut [f32]) {
    let mut hp = Svf::default();
    for pass in 0..2 {
        for s in ch.iter_mut() {
            let y = hp.high(*s, 60.0);
            if pass == 1 {
                *s = y;
            }
        }
    }
}

/// The tension drone: D2 and A2 on detuned saws through a low-pass that
/// breathes, a quiet E♭3 rubbing against the D, and a slow pulse.
pub fn drone_bed() -> Vec<i16> {
    let bed = Bed::Drone;
    let len = bed.seconds();
    let n = bed.frames() + (FOLD * SR) as usize;
    let mut l = vec![0.0f32; n];
    let mut r = vec![0.0f32; n];
    let voices: [(f32, f32, f32); 5] = [
        (note(38.0), 0.998, -0.4),
        (note(38.0), 1.003, 0.4),
        (note(45.0), 1.001, -0.2),
        (note(45.0), 0.997, 0.3),
        (note(51.0), 1.0, 0.0),
    ];
    for (k, &(f, detune, pan)) in voices.iter().enumerate() {
        let mut o = Osc::at(k as f32 * 0.21);
        let mut lp = Svf::default();
        let amp = if k == 4 { 0.25 } else { 0.5 };
        let (gl, gr) = (((1.0 - pan) * 0.5).sqrt(), ((1.0 + pan) * 0.5).sqrt());
        for i in 0..n {
            let t = i as f32 / SR;
            let cutoff = 420.0 + 180.0 * lfo(t, 2.0, len, k as f32);
            let pulse = 0.7 + 0.3 * lfo(t, 6.0, len, 0.0);
            // The E♭ swells in and out, the rub coming and going.
            let rub = if k == 4 {
                (0.5 + 0.5 * lfo(t, 1.0, len, -FRAC_PI_2)).powi(2)
            } else {
                1.0
            };
            let y = amp * lp.low(o.saw(f * detune, 10), cutoff) * pulse * rub;
            l[i] += y * gl;
            r[i] += y * gr;
        }
    }
    let (mut l, mut r) = fold_loop(&l, &r, bed.frames());
    dc_block_loop(&mut l);
    dc_block_loop(&mut r);
    master_bed(&l, &r)
}

/// Renders every bed in [`Bed::ALL`] order, handing each over when done.
pub fn render_all(mut deliver: impl FnMut(Bed, MusicClip)) {
    for bed in Bed::ALL {
        deliver(bed, MusicClip::new(bed.render(), Some(0)));
    }
}

/// The ambience thread (named `ambience-bank`).
#[derive(Resource)]
pub struct AmbienceLoader {
    rx: Mutex<mpsc::Receiver<(Bed, MusicClip)>>,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl AmbienceLoader {
    pub fn spawn() -> Option<Self> {
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("ambience-bank".into())
            .spawn(move || {
                render_all(|bed, clip| {
                    let _ = tx.send((bed, clip));
                })
            })
            .ok()?;
        Some(Self {
            rx: Mutex::new(rx),
            thread: Mutex::new(Some(thread)),
        })
    }

    pub fn thread_name(&self) -> Option<String> {
        let thread = self.thread.lock().ok()?;
        thread.as_ref()?.thread().name().map(str::to_owned)
    }
}

// ---------------------------------------------------------------------------
// The mix (pure)
// ---------------------------------------------------------------------------

/// What the ambience reads each frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AmbienceInput {
    pub screen: Screen,
    /// A wave is being fought, with this many knights alive.
    pub fighting: bool,
    pub alive: u32,
    /// The breeze at the listener (0..1).
    pub gust: f32,
    /// An off-screen warning is guarded.
    pub warning: bool,
    /// Master × Effects (0 when muted).
    pub effects: f32,
}

/// The beds' gains ([`Bed::ALL`] order), before nothing else: the Effects
/// slider and mute are in.
pub type AmbienceGains = [f32; 3];

/// Smooths the beds' levels toward their targets. Fixed-size: stepping it
/// never allocates.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AmbienceDirector {
    level: [f32; 3],
    tension: f32,
    duck: f32,
}

/// How full the tension drone is for `alive` knights in a fight.
pub fn tension_for(fighting: bool, alive: u32) -> f32 {
    if fighting {
        (0.35 + alive as f32 / 10.0).min(1.0)
    } else {
        0.0
    }
}

impl AmbienceDirector {
    /// The drone's tension now (0..1).
    pub fn tension(&self) -> f32 {
        self.tension
    }

    pub fn step(&mut self, dt: f32, input: &AmbienceInput) -> AmbienceGains {
        let on = match input.screen {
            Screen::Boot => 0.0,
            Screen::Menu | Screen::Playing => 1.0,
            Screen::Paused => 0.5,
        };
        let playing = matches!(input.screen, Screen::Playing | Screen::Paused);
        // The drone swells in over ~2 s and falls away over ~4 s.
        let want = if playing {
            tension_for(input.fighting, input.alive)
        } else {
            0.0
        };
        let rate = if want > self.tension { 0.5 } else { 0.25 };
        self.tension += (want - self.tension).clamp(-rate * dt, rate * dt);
        // The warning's duck: fast in, eased out.
        let duck_to = if input.warning { 1.0 } else { 0.0 };
        let k = if input.warning { 40.0 } else { 3.0 };
        self.duck += (duck_to - self.duck) * (1.0 - (-k * dt).exp());
        let duck = 1.0 - (1.0 - WARNING_DUCK) * self.duck;
        let targets = [
            db_to_gain(WIND_DB) * (0.3 + 0.7 * input.gust.clamp(0.0, 1.0)),
            db_to_gain(CASTLE_DB),
            db_to_gain(DRONE_DB) * self.tension,
        ];
        let ease = 1.0 - (-4.0 * dt).exp();
        let effects = input.effects.clamp(0.0, 1.0);
        let mut out = [0.0; 3];
        for i in 0..3 {
            self.level[i] += (targets[i] * on - self.level[i]) * ease;
            out[i] = if effects > 0.0 {
                self.level[i] * duck * effects
            } else {
                0.0
            };
        }
        out
    }
}

// ---------------------------------------------------------------------------
// The game side
// ---------------------------------------------------------------------------

/// One pooled voice per bed, made when its loop arrives.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmbienceVoice(pub Bed);

/// The ambience's state.
#[derive(Resource, Debug)]
pub struct Ambience {
    pub director: AmbienceDirector,
    pub last: AmbienceGains,
    voices: [Option<Entity>; 3],
    next_bird: f64,
    rng: Rng,
    /// The castle's direction from the arena (unit).
    castle: Vec3,
}

impl Default for Ambience {
    fn default() -> Self {
        let castle = FarLayout::default().station.position;
        Self {
            director: AmbienceDirector::default(),
            last: [0.0; 3],
            voices: [None; 3],
            next_bird: 3.0,
            rng: Rng::new(0xB1DD),
            castle: castle.normalize_or(Vec3::NEG_Z),
        }
    }
}

pub(super) fn build(app: &mut App) {
    if app.is_plugin_added::<bevy::audio::AudioPlugin>()
        && let Some(loader) = AmbienceLoader::spawn()
    {
        app.insert_resource(loader);
    }
    app.init_resource::<Ambience>().add_systems(
        Update,
        (receive_ambience, drive_ambience)
            .chain()
            .after(super::barks::queue_knight_voices)
            .before(super::play_queued),
    );
}

/// Files each loop as it arrives, with its pooled voice (playing silently
/// until the director raises it).
fn receive_ambience(
    mut commands: Commands,
    loader: Option<Res<AmbienceLoader>>,
    mut clips: ResMut<Assets<MusicClip>>,
    mut ambience: ResMut<Ambience>,
) {
    let Some(loader) = loader else {
        return;
    };
    let Ok(rx) = loader.rx.lock() else {
        return;
    };
    loop {
        match rx.try_recv() {
            Ok((bed, clip)) => {
                let handle = clips.add(clip);
                let mut settings = PlaybackSettings {
                    mode: PlaybackMode::Once,
                    volume: Volume::SILENT,
                    ..PlaybackSettings::ONCE
                };
                let mut voice = commands.spawn((
                    Name::new("Ambience"),
                    AmbienceVoice(bed),
                    AudioPlayer::<MusicClip>(handle),
                    Transform::default(),
                ));
                if bed == Bed::Castle {
                    settings = settings
                        .with_spatial(true)
                        .with_spatial_scale(SpatialScale::new(0.08));
                }
                voice.insert(settings);
                ambience.voices[bed as usize] = Some(voice.id());
            }
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => {
                commands.remove_resource::<AmbienceLoader>();
                break;
            }
        }
    }
}

/// Steps the mix, sets the beds' levels, places the castle bed on the
/// castle's bearing, and lets a songbird sing now and then.
#[allow(clippy::too_many_arguments)]
fn drive_ambience(
    real: Res<Time<Real>>,
    time: Res<Time>,
    tuning: Res<Tuning>,
    state: Option<Res<State<AppState>>>,
    summary: Option<Res<RunSummary>>,
    run: Option<Res<Run>>,
    barks: Option<Res<KnightVoices>>,
    camera: Option<Single<&GlobalTransform, With<MainCamera>>>,
    layout: Option<Res<IslandLayout>>,
    mut ambience: ResMut<Ambience>,
    mut queue: ResMut<PlayQueue>,
    mut sinks: Query<(
        &AmbienceVoice,
        &mut Transform,
        Option<&mut AudioSink>,
        Option<&mut SpatialAudioSink>,
    )>,
) {
    let now = real.elapsed_secs_f64();
    let ears = camera.map_or(Vec3::new(2.0, 1.6, 14.0), |c| c.translation());
    let screen = state.map_or(Screen::Boot, |s| Screen::from_state(*s.get()));
    let fighting = summary
        .as_ref()
        .is_some_and(|s| matches!(s.phase, RunPhase::Fighting));
    let warning = barks.is_some_and(|b| b.gate.guarding(now));
    let input = AmbienceInput {
        screen,
        fighting,
        alive: run.map_or(0, |r| r.alive),
        gust: wind::gust_at(time.elapsed_secs_wrapped(), ears.xz()),
        warning,
        effects: tuning.audio.effects_gain(),
    };
    let ambience = &mut *ambience;
    let gains = ambience.director.step(real.delta_secs(), &input);
    ambience.last = gains;
    for (voice, mut transform, sink, spatial) in &mut sinks {
        let gain = Volume::Linear(gains[voice.0 as usize]);
        if let Some(mut sink) = sink {
            sink.set_volume(gain);
        }
        if let Some(mut sink) = spatial {
            sink.set_volume(gain);
        }
        if voice.0 == Bed::Castle {
            let at = ears + ambience.castle * CASTLE_EAR_DISTANCE;
            if transform.translation.distance_squared(at) > 1e-4 {
                transform.translation = at;
            }
        }
    }
    // Songbirds: from a tree near you, less often when the fight is thick.
    if now < ambience.next_bird {
        return;
    }
    let gap = ambience.rng.range(BIRD_GAP.0, BIRD_GAP.1) as f64;
    ambience.next_bird = now + gap * (1.0 + 2.0 * ambience.director.tension() as f64);
    if !matches!(screen, Screen::Menu | Screen::Playing) || warning || input.effects <= 0.0 {
        return;
    }
    let Some(layout) = layout else {
        return;
    };
    // Reservoir-pick one tree in range (no allocation).
    let mut pick = None;
    let mut seen = 0u32;
    for d in &layout.0.decor {
        if !d.model.starts_with("tree_") {
            continue;
        }
        let p = d.transform.translation;
        if p.distance(ears) > BIRD_RANGE {
            continue;
        }
        seen += 1;
        if ambience.rng.range(0.0, 1.0) * seen as f32 <= 1.0 {
            pick = Some(p + Vec3::Y * 4.5 * d.transform.scale.y);
        }
    }
    if let Some(at) = pick {
        queue.push(Sfx::Birdsong, Some(at), now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn music_clip_layout_matches() {
        assert_eq!(MUSIC_CHANNELS, 2);
        assert_eq!(MUSIC_RATE, SAMPLE_RATE);
    }
}
