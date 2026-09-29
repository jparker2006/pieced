//! Milestone 4, chunk 3: the adaptive score (docs/M4-SPEC.md → Chunk 3).
//!
//! - The music files: embedded, in the manifest, decoding to their lengths at
//!   their loudness, and looping without a seam as the game plays them.
//! - The synthesized break loop (an A-minor celesta waltz) and the derived
//!   death sting (a brass chord bent down), analysed as audio.
//! - The director, simulated at 60 Hz: the state → slot mapping, crossfade
//!   timing, stings and their ducking, the big-hit duck, the pause dip.
//! - The sliders at 0 are exactly silent; the loading stays off the main
//!   thread and out of the launch; nothing allocates per event over five
//!   simulated minutes of waves.
//!
//! Nothing here plays audio. `PIECED_MUSIC_WAV_DIR=<dir> cargo test --test
//! music -- --ignored write_music_wavs` renders the break loop and the death
//! sting to WAV files for listening and analysis.

use bevy::{
    audio::{AudioSource, Decodable, Source},
    ecs::schedule::{Schedules, SingleThreadedExecutor},
    prelude::*,
    state::app::StatesPlugin,
    time::TimeUpdateStrategy,
};
use pieced::{
    audio::{
        AudioTuning, GameAudioPlugin, SoundBank, SoundBankJob, Voice, celesta,
        loudness::integrated_lufs,
        music::{
            self, LoadMusic, MUSIC_CHANNELS, MUSIC_FILES, MUSIC_MANIFEST, MUSIC_RATE, MusicBank,
            MusicClip, MusicDirector, MusicEntry, MusicFrame, MusicInput, MusicLoader, MusicSlot,
            MusicState, MusicTuning, RunView, Screen, Sting, Track, WaveStarted, death_sting,
            decode_ogg, music_volume, parse_manifest, slot_for,
        },
    },
    fx::kills::KillConfirmed,
    shared::{
        AppState, DamageDealt, DamageTarget, Eliminated, GameCue, PieceChange, PieceChanged,
        PieceKind, Player, ShotFired, WeaponKind,
    },
    tuning::Tuning,
    waves::{Run, RunPhase, RunSummary, WavesTuning},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    f32::consts::PI,
    path::PathBuf,
    sync::OnceLock,
    time::{Duration, Instant},
};

const SR: f32 = MUSIC_RATE as f32;
const DT: f32 = 1.0 / 60.0;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn manifest() -> Vec<MusicEntry> {
    parse_manifest(MUSIC_MANIFEST).expect("music.json parses")
}

fn entry(name: &str) -> MusicEntry {
    manifest()
        .into_iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| panic!("{name} is in music.json"))
}

/// Every music file decoded once (they're big; tests share them).
fn decoded() -> &'static BTreeMap<&'static str, Vec<i16>> {
    static CACHE: OnceLock<BTreeMap<&'static str, Vec<i16>>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let m = manifest();
        MUSIC_FILES
            .iter()
            .map(|(name, bytes)| {
                let e = m.iter().find(|e| e.name == *name).unwrap();
                let pcm = decode_ogg(bytes, e.frames)
                    .unwrap_or_else(|| panic!("{name} decodes to its {} frames", e.frames));
                (*name, pcm)
            })
            .collect()
    })
}

fn break_loop() -> &'static Vec<i16> {
    static CACHE: OnceLock<Vec<i16>> = OnceLock::new();
    CACHE.get_or_init(celesta::render_break_loop)
}

fn to_f32(pcm: &[i16]) -> Vec<f32> {
    pcm.iter().map(|s| *s as f32 / 32_768.0).collect()
}

/// Left + right over 2.
fn mono(stereo: &[f32]) -> Vec<f32> {
    stereo.chunks(2).map(|f| 0.5 * (f[0] + f[1])).collect()
}

fn rms_db(x: &[f32]) -> f32 {
    let ms = x.iter().map(|s| s * s).sum::<f32>() / x.len().max(1) as f32;
    10.0 * ms.max(1e-12).log10()
}

/// Magnitude of the Hann-windowed DFT of `x` at `freq`.
fn magnitude(x: &[f32], freq: f32) -> f32 {
    let n = x.len() as f32;
    let (mut re, mut im) = (0.0f32, 0.0f32);
    for (i, s) in x.iter().enumerate() {
        let w = 0.5 - 0.5 * (2.0 * PI * i as f32 / (n - 1.0)).cos();
        let phase = 2.0 * PI * freq * i as f32 / SR;
        re += s * w * phase.cos();
        im -= s * w * phase.sin();
    }
    re.hypot(im)
}

fn note_hz(midi: f32) -> f32 {
    440.0 * 2f32.powf((midi - 69.0) / 12.0)
}

/// Energy in 1/12-octave bands (100 Hz – 4 kHz) of `x`.
fn semitone_bands(x: &[f32]) -> Vec<f32> {
    (0..64)
        .map(|k| {
            let f = 100.0 * 2f32.powf(k as f32 / 12.0);
            magnitude(x, f).powi(2)
        })
        .collect()
}

/// How far `late`'s spectrum sits below `early`'s (semitones, ≤ 0).
fn spectral_shift(early: &[f32], late: &[f32]) -> i32 {
    let a = semitone_bands(early);
    let b = semitone_bands(late);
    let norm = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
    let (na, nb) = (norm(&a), norm(&b));
    (-14..=2)
        .max_by(|&s1, &s2| {
            let score = |s: i32| {
                (0..64)
                    .filter_map(|k| {
                        let j = k + s;
                        (0..64).contains(&j).then(|| a[k as usize] * b[j as usize])
                    })
                    .sum::<f32>()
                    / (na * nb)
            };
            score(s1).total_cmp(&score(s2))
        })
        .unwrap()
}

// ---------------------------------------------------------------------------
// The music files
// ---------------------------------------------------------------------------

#[test]
fn every_music_file_is_embedded_and_in_the_manifest() {
    let dir = repo().join("assets/music");
    let on_disk: BTreeSet<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".ogg"))
        .collect();
    let embedded: BTreeSet<String> = MUSIC_FILES
        .iter()
        .map(|(n, _)| format!("{n}.ogg"))
        .collect();
    assert_eq!(on_disk, embedded, "every .ogg is embedded, and only those");
    for (name, bytes) in MUSIC_FILES {
        assert_eq!(
            bytes,
            std::fs::read(dir.join(format!("{name}.ogg"))).unwrap(),
            "{name}: the embedded bytes are the committed file"
        );
        assert!(&bytes[..4] == b"OggS", "{name} is an Ogg file");
        assert!(
            bytes.len() < 1_500_000,
            "{name} stays small: {} bytes",
            bytes.len()
        );
    }
    let names: BTreeSet<String> = manifest().into_iter().map(|e| e.name).collect();
    let files: BTreeSet<String> = MUSIC_FILES.iter().map(|(n, _)| n.to_string()).collect();
    assert_eq!(names, files, "music.json lists exactly the files");
    // Every track has a source: a file, or (the break loop) synthesis.
    for track in Track::ALL {
        match track.file() {
            Some(file) => assert!(files.contains(file), "{track:?}"),
            None => assert_eq!(track, Track::Break),
        }
    }
    let total: usize = MUSIC_FILES.iter().map(|(_, b)| b.len()).sum();
    println!("music: {:.2} MB embedded", total as f64 / 1e6);
    assert!(total < 5_000_000, "the score stays under 5 MB: {total}");
}

#[test]
fn every_music_file_decodes_to_its_length_at_its_loudness() {
    for (name, pcm) in decoded() {
        let e = entry(name);
        assert_eq!(pcm.len(), e.frames * MUSIC_CHANNELS as usize, "{name}");
        // The encoder pads its last block; the game drops the padding.
        let (_, bytes) = MUSIC_FILES.iter().find(|(n, _)| n == name).unwrap();
        let source = AudioSource {
            bytes: (*bytes).into(),
        };
        let decoder = source.decoder();
        assert_eq!(decoder.channels().get(), 2, "{name} is stereo");
        assert_eq!(
            decoder.sample_rate().get(),
            MUSIC_RATE,
            "{name} is 44.1 kHz"
        );
        let total = decoder.count() / 2;
        assert!(
            total >= e.frames && total <= e.frames + 2048,
            "{name}: {total} decoded frames vs {} listed",
            e.frames
        );
        let x = to_f32(pcm);
        let lufs = integrated_lufs(&x, 2, MUSIC_RATE);
        let target = if name.starts_with("sting") || *name == "death_chord" {
            music::STING_LUFS
        } else {
            celesta::TARGET_LUFS
        };
        let peak = x.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        println!(
            "{name:<12} {:6.2} s  {lufs:6.1} LUFS  peak {:5.1} dBFS",
            e.frames as f32 / SR,
            20.0 * peak.log10()
        );
        assert!(
            (lufs - target).abs() <= 1.0,
            "{name}: {lufs:.1} LUFS, target {target}"
        );
        assert!(peak <= 0.9, "{name} peaks under −1 dBFS: {peak}");
    }
}

/// The loop seam as the game plays it: the last frames, then the loop start.
fn check_seam(name: &str, clip: &MusicClip, level_match: bool) {
    let frames = clip.frames();
    let start = clip.loop_start().expect("a loop");
    let played = clip.play(2 * (frames + SR as usize));
    let seam = 2 * frames;
    assert_eq!(
        played[seam..seam + 64],
        to_f32(&clip.samples()[2 * start..2 * start + 64])[..],
        "{name}: after its last frame it plays its loop start"
    );
    // No click: the step across the seam is no bigger than the loop's own
    // largest steps.
    let body = &played[2 * start..seam];
    let mut steps: Vec<f32> = body
        .chunks(2)
        .zip(body.chunks(2).skip(1))
        .map(|(a, b)| (a[0] - b[0]).abs().max((a[1] - b[1]).abs()))
        .collect();
    steps.sort_by(f32::total_cmp);
    let p999 = steps[steps.len() * 999 / 1000];
    let jump = (played[seam] - played[seam - 2])
        .abs()
        .max((played[seam + 1] - played[seam - 1]).abs());
    println!("{name}: seam step {jump:.4}, loop p99.9 step {p999:.4}");
    assert!(
        jump <= p999.max(0.01),
        "{name} clicks at its seam: {jump} > {p999}"
    );
    let window = (0.1 * SR) as usize * 2;
    let before = rms_db(&played[seam - window..seam]);
    let after = rms_db(&played[seam..seam + window]);
    if level_match {
        assert!(
            (before - after).abs() <= 6.0,
            "{name}: {before:.1} dB before its seam, {after:.1} after"
        );
    } else {
        // A whole-piece loop: its ending has faded out by the seam.
        assert!(
            before < -40.0,
            "{name} fades out into its seam: {before:.1} dB"
        );
    }
}

#[test]
fn the_loops_come_round_without_a_seam() {
    for (name, level_match) in [("combat_low", true), ("combat_high", true), ("menu", false)] {
        let e = entry(name);
        let clip = MusicClip::new(decoded()[name].clone(), e.loop_start());
        assert!(clip.loop_start().is_some(), "{name} loops");
        check_seam(name, &clip, level_match);
    }
    for name in ["sting_round", "sting_best", "death_chord"] {
        assert_eq!(entry(name).loop_start(), None, "{name} is a one-shot");
    }
    // A one-shot ends.
    let sting = MusicClip::new(decoded()["sting_round"].clone(), None);
    assert_eq!(sting.play(usize::MAX).len(), sting.samples().len());
    // The loops are long enough not to wear thin: 45+ s of combat each.
    for name in ["combat_low", "combat_high"] {
        let e = entry(name);
        let body = (e.frames - e.loop_start().unwrap()) as f32 / SR;
        assert!(body >= 45.0, "{name} loops every {body:.1} s");
    }
}

// ---------------------------------------------------------------------------
// The break loop: an original celesta waltz
// ---------------------------------------------------------------------------

#[test]
fn the_break_loop_is_a_seamless_a_minor_celesta_waltz() {
    let pcm = break_loop();
    assert_eq!(pcm.len(), celesta::FRAMES * 2, "exactly 16 bars");
    assert!((celesta::bpm() - 132.3).abs() < 0.1, "{}", celesta::bpm());
    let x = to_f32(pcm);
    let lufs = integrated_lufs(&x, 2, MUSIC_RATE);
    assert!((lufs - celesta::TARGET_LUFS).abs() < 0.5, "{lufs:.2} LUFS");
    let peak = x.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak <= 0.9, "peaks under −1 dBFS: {peak}");
    check_seam("break", &MusicClip::new(pcm.clone(), Some(0)), true);
    let m = mono(&x);

    // In key: the score's pitch classes (A minor, its leading tone G♯, the F♯
    // and E♭ colours) hold nearly all the pitched energy.
    let allowed = celesta::score_pitch_classes();
    assert_eq!(
        allowed,
        vec![0, 2, 3, 4, 5, 6, 7, 8, 9, 11],
        "C D E♭ E F F♯ G G♯ A B"
    );
    let mut chroma = [0.0f32; 12];
    for w in m.chunks(8192).take(100).step_by(3) {
        for midi in 57..96 {
            chroma[midi % 12] += magnitude(w, note_hz(midi as f32)).powi(2);
        }
    }
    let total: f32 = chroma.iter().sum();
    let inside: f32 = allowed.iter().map(|&pc| chroma[pc as usize]).sum();
    println!(
        "chroma C..B: {:?}",
        chroma.map(|c| (100.0 * c / total).round())
    );
    assert!(
        inside / total > 0.97,
        "{:.1}% in key",
        100.0 * inside / total
    );
    // A minor, not C major: A and E outweigh C and G.
    assert!(chroma[9] + chroma[4] > chroma[0] + chroma[7], "{chroma:?}");

    // The celesta figure plays its notes: 20–110 ms after each eighth's
    // onset, the written note rings louder than the semitones either side.
    let eighth = celesta::EIGHTH;
    let mut right = 0;
    let checked = 96;
    for i in 0..checked {
        let at = i * eighth + (0.02 * SR) as usize;
        let w = &m[at..at + (0.09 * SR) as usize];
        let f = note_hz(celesta::figure_note(i));
        let here = magnitude(w, f);
        if here > magnitude(w, f * 2f32.powf(1.0 / 12.0))
            && here > magnitude(w, f / 2f32.powf(1.0 / 12.0))
        {
            right += 1;
        }
    }
    assert!(
        right * 10 >= checked * 9,
        "{right}/{checked} figure notes in tune"
    );

    // Three beats to the bar: the level swells on every downbeat (the figure's
    // accent and the harp), so bar-long lags line up better than 4/4 ones.
    let env: Vec<f32> = m
        .chunks(eighth / 4)
        .map(|c| c.iter().map(|s| s * s).sum::<f32>())
        .collect();
    let mean = env.iter().sum::<f32>() / env.len() as f32;
    let ac = |lag: usize| -> f32 {
        (0..env.len() - lag)
            .map(|i| (env[i] - mean) * (env[i + lag] - mean))
            .sum()
    };
    let (three, four) = (ac(4 * 6), ac(4 * 8));
    println!("waltz: autocorrelation at a 3/4 bar {three:.3e}, a 4/4 bar {four:.3e}");
    assert!(three > four, "a waltz, not a march");
}

#[test]
fn the_break_loop_renders_the_same_every_launch() {
    assert_eq!(celesta::render_break_loop(), *break_loop());
}

// ---------------------------------------------------------------------------
// The death sting: a brass chord deflating
// ---------------------------------------------------------------------------

#[test]
fn the_death_sting_is_the_battle_chord_falling_away() {
    let chord = &decoded()["death_chord"];
    let sting = death_sting(chord);
    let secs = sting.len() as f32 / 2.0 / SR;
    assert!((1.0..=3.0).contains(&secs), "{secs:.2} s");
    let x = mono(&to_f32(&sting));
    // It starts as the chord itself (the stab is untouched).
    let src = mono(&to_f32(chord));
    let n = (0.1 * SR) as usize;
    let (a, b) = (
        &x[(0.05 * SR) as usize..][..n],
        &src[(0.05 * SR) as usize..][..n],
    );
    let dot: f32 = a.iter().zip(b).map(|(p, q)| p * q).sum();
    let norm = (a.iter().map(|p| p * p).sum::<f32>() * b.iter().map(|q| q * q).sum::<f32>()).sqrt();
    assert!(
        dot / norm > 0.9,
        "the stab is the chord: correlation {:.2}",
        dot / norm
    );
    // The pitch falls: late in the fall the spectrum sits several semitones
    // below the stab.
    let w = (0.12 * SR) as usize;
    let stab = &x[(0.08 * SR) as usize..][..w];
    let at = |fraction: f32| ((music::DEATH_STAB + fraction * music::DEATH_FALL) * SR) as usize;
    let mid = &x[at(0.6)..][..w];
    let late = &x[at(0.85)..][..w];
    let (s_mid, s_late) = (spectral_shift(stab, mid), spectral_shift(stab, late));
    println!("death sting: spectrum {s_mid} semitones down at 60% of the fall, {s_late} at 85%");
    assert!(
        s_mid <= -2 && s_late <= -4 && s_late <= s_mid,
        "{s_mid}, {s_late}"
    );
    // It fades as it falls and ends in silence.
    let level = |t: f32| rms_db(&x[(t * SR) as usize..][..(0.05 * SR) as usize]);
    assert!(
        level(1.6) < level(0.2) - 10.0,
        "{} vs {}",
        level(1.6),
        level(0.2)
    );
    assert!(
        rms_db(&x[x.len() - (0.05 * SR) as usize..]) < -50.0,
        "ends silent"
    );
    let lufs = integrated_lufs(&to_f32(&sting), 2, MUSIC_RATE);
    assert!((lufs - music::STING_LUFS).abs() < 0.5, "{lufs:.1} LUFS");
}

// ---------------------------------------------------------------------------
// The director
// ---------------------------------------------------------------------------

const STINGS: [f32; 3] = [4.5, 2.4, 10.1];

fn view(seed: u64, phase: RunPhase, wave: u32, alive: u32) -> RunView {
    RunView {
        seed,
        phase,
        wave,
        alive,
        new_best: false,
    }
}

fn play(run: Option<RunView>) -> MusicInput {
    MusicInput {
        screen: Screen::Playing,
        run,
    }
}

fn menu() -> MusicInput {
    MusicInput {
        screen: Screen::Menu,
        run: None,
    }
}

/// Steps `seconds` at 60 Hz, returning every frame.
fn run_for(
    d: &mut MusicDirector,
    input: MusicInput,
    seconds: f32,
    stings: [f32; 3],
) -> Vec<MusicFrame> {
    let t = MusicTuning::default();
    (0..(seconds / DT).round() as usize)
        .map(|_| d.step(DT, &input, &t, stings, false))
        .collect()
}

fn slot_gain(frame: &MusicFrame, slot: MusicSlot) -> f32 {
    frame.gains[MusicSlot::ALL.iter().position(|s| *s == slot).unwrap()]
}

fn mix(slot: MusicSlot) -> f32 {
    let t = MusicTuning::default();
    10f32.powf(t.slot_db[MusicSlot::ALL.iter().position(|s| *s == slot).unwrap()] / 20.0)
}

#[test]
fn every_state_maps_to_its_slot() {
    let fighting = |wave, alive| play(Some(view(1, RunPhase::Fighting, wave, alive)));
    assert_eq!(
        slot_for(&MusicInput::default(), false),
        None,
        "boot is silent"
    );
    assert_eq!(slot_for(&menu(), false), Some(MusicSlot::Menu));
    assert_eq!(
        slot_for(&play(None), false),
        Some(MusicSlot::Break),
        "practice is calm"
    );
    assert_eq!(slot_for(&fighting(1, 2), false), Some(MusicSlot::CombatLow));
    assert_eq!(slot_for(&fighting(1, 2), true), Some(MusicSlot::CombatHigh));
    let at = |phase| play(Some(view(1, phase, 3, 0)));
    assert_eq!(
        slot_for(&at(RunPhase::Break { ends_tick: 9 }), false),
        Some(MusicSlot::Break)
    );
    assert_eq!(
        slot_for(&at(RunPhase::Dying { until: 9 }), false),
        None,
        "the death beat"
    );
    assert_eq!(
        slot_for(&at(RunPhase::Over { tick: 9 }), false),
        Some(MusicSlot::Break),
        "results"
    );
    let mut paused = fighting(2, 1);
    paused.screen = Screen::Paused;
    assert_eq!(
        slot_for(&paused, false),
        Some(MusicSlot::CombatLow),
        "a pause keeps the slot"
    );

    // Through the director: combat goes high from wave 6, or once 6 knights
    // are alive, for the rest of that wave.
    let t = MusicTuning::default();
    assert_eq!((t.high_wave, t.high_alive), (6, 6));
    let mut d = MusicDirector::default();
    let target =
        |d: &mut MusicDirector, input| run_for(d, input, 0.1, STINGS).last().unwrap().target;
    assert_eq!(target(&mut d, fighting(3, 5)), Some(MusicSlot::CombatLow));
    assert_eq!(target(&mut d, fighting(3, 6)), Some(MusicSlot::CombatHigh));
    assert_eq!(
        target(&mut d, fighting(3, 1)),
        Some(MusicSlot::CombatHigh),
        "latched for the wave"
    );
    let brk = play(Some(view(1, RunPhase::Break { ends_tick: 9 }, 3, 0)));
    assert_eq!(target(&mut d, brk), Some(MusicSlot::Break));
    assert_eq!(
        target(&mut d, fighting(4, 2)),
        Some(MusicSlot::CombatLow),
        "a new wave starts low"
    );
    assert_eq!(target(&mut d, fighting(6, 0)), Some(MusicSlot::CombatHigh));
    assert_eq!(target(&mut d, fighting(9, 0)), Some(MusicSlot::CombatHigh));
}

#[test]
fn slots_crossfade_in_the_crossfade_time_at_equal_power() {
    let t = MusicTuning::default();
    assert!(
        (1.0..=2.0).contains(&t.crossfade_seconds),
        "1–2 s: {}",
        t.crossfade_seconds
    );
    let mut d = MusicDirector::default();
    // No stings loaded here, so nothing ducks the fade.
    let frames = run_for(&mut d, menu(), 3.0, [0.0; 3]);
    let first = frames[0];
    assert!(first.restart[0], "the menu starts from its top");
    assert!(
        (slot_gain(frames.last().unwrap(), MusicSlot::Menu) - mix(MusicSlot::Menu)).abs() < 1e-6
    );
    // Into combat.
    let fight = play(Some(view(5, RunPhase::Fighting, 1, 0)));
    let frames = run_for(&mut d, fight, 3.0, [0.0; 3]);
    assert!(frames[0].restart[2], "combat starts from its top");
    let full_at = frames
        .iter()
        .position(|f| (slot_gain(f, MusicSlot::CombatLow) - mix(MusicSlot::CombatLow)).abs() < 1e-6)
        .unwrap();
    let gone_at = frames
        .iter()
        .position(|f| slot_gain(f, MusicSlot::Menu) == 0.0)
        .unwrap();
    let expect = (t.crossfade_seconds / DT).round() as usize - 1;
    assert!(
        full_at.abs_diff(expect) <= 1,
        "in at frame {full_at}, expected {expect}"
    );
    assert!(
        gone_at.abs_diff(expect) <= 1,
        "out at frame {gone_at}, expected {expect}"
    );
    // Halfway, both sit at −3 dB (equal power).
    let half = &frames[expect / 2];
    let (a, b) = (
        slot_gain(half, MusicSlot::Menu) / mix(MusicSlot::Menu),
        slot_gain(half, MusicSlot::CombatLow) / mix(MusicSlot::CombatLow),
    );
    assert!(
        (a * a + b * b - 1.0).abs() < 0.03,
        "equal power: {a:.3}² + {b:.3}²"
    );
    assert!((a - b).abs() < 0.03, "{a} vs {b}");
    // The menu's voice stops once it's silent; combat's plays on.
    let last = frames.last().unwrap();
    assert_eq!(last.voices, [false, false, true, false]);
    assert!(
        frames[1..].iter().all(|f| f.restart == [false; 4]),
        "one restart only"
    );
}

#[test]
fn stings_play_on_their_beats_and_duck_the_loops() {
    let t = MusicTuning::default();
    let mut d = MusicDirector::default();
    // A break, then wave 2 starts.
    let brk = play(Some(view(9, RunPhase::Break { ends_tick: 0 }, 1, 0)));
    run_for(&mut d, brk, 3.0, STINGS);
    let wave2 = play(Some(view(9, RunPhase::Fighting, 2, 0)));
    let frames = run_for(&mut d, wave2, 7.0, STINGS);
    assert_eq!(
        frames[0].sting,
        Some(Sting::RoundStart),
        "the round sting on the wave's frame"
    );
    assert_eq!(
        frames[0].wave_started,
        Some(2),
        "with the wave-start event (the banner's sync)"
    );
    assert!(
        frames[1..]
            .iter()
            .all(|f| f.sting.is_none() && f.wave_started.is_none())
    );
    // The loops dip under it within its attack, hold while it plays, and come
    // back after its release.
    let ducked_by = ((t.sting_attack / DT).ceil() as usize) + 1;
    assert!(
        (frames[ducked_by].duck - t.sting_duck).abs() < 1e-4,
        "{}",
        frames[ducked_by].duck
    );
    let playing_until = (STINGS[0] / DT) as usize - 2;
    assert!(
        frames[ducked_by..playing_until]
            .iter()
            .all(|f| (f.duck - t.sting_duck).abs() < 1e-4)
    );
    let back = ((STINGS[0] + t.sting_release) / DT).ceil() as usize + 2;
    assert!(
        (frames[back].duck - 1.0).abs() < 1e-6,
        "{}",
        frames[back].duck
    );
    // The duck is on the loops' gains.
    let low = slot_gain(&frames[ducked_by + 100], MusicSlot::CombatLow);
    assert!(
        (low - mix(MusicSlot::CombatLow) * t.sting_duck).abs() < 1e-4,
        "{low}"
    );

    // The player's elimination: the death sting, and the loops fall away.
    let dying = play(Some(view(9, RunPhase::Dying { until: 0 }, 2, 3)));
    let frames = run_for(&mut d, dying, 3.0, STINGS);
    assert_eq!(frames[0].sting, Some(Sting::Death));
    assert_eq!(frames[0].target, None);
    assert!(frames.last().unwrap().gains.iter().all(|g| *g == 0.0));
    // The results with a new best.
    let mut over = view(9, RunPhase::Over { tick: 0 }, 2, 0);
    over.new_best = true;
    let frames = run_for(&mut d, play(Some(over)), 1.0, STINGS);
    assert_eq!(frames[0].sting, Some(Sting::NewBest));
    assert_eq!(frames[0].target, Some(MusicSlot::Break));
    // "Go again": a new run's first wave starts with the round sting.
    let again = play(Some(view(10, RunPhase::Fighting, 1, 0)));
    let frames = run_for(&mut d, again, 0.5, STINGS);
    assert_eq!(frames[0].sting, Some(Sting::RoundStart));
    assert_eq!(frames[0].wave_started, Some(1));
    // A results screen without a new best stays quiet; so does a quit.
    let mut d = MusicDirector::default();
    run_for(
        &mut d,
        play(Some(view(3, RunPhase::Fighting, 4, 0))),
        0.5,
        STINGS,
    );
    let frames = run_for(
        &mut d,
        play(Some(view(3, RunPhase::Over { tick: 0 }, 4, 0))),
        0.5,
        STINGS,
    );
    assert!(frames.iter().all(|f| f.sting.is_none()));
    // A sting that isn't loaded yet neither plays nor ducks.
    let mut d = MusicDirector::default();
    let frames = run_for(
        &mut d,
        play(Some(view(4, RunPhase::Fighting, 1, 0))),
        1.0,
        [0.0; 3],
    );
    assert_eq!(frames[0].sting, None);
    assert_eq!(frames[0].wave_started, Some(1), "the wave still started");
    assert!(frames.iter().all(|f| f.duck == 1.0));
}

#[test]
fn a_big_hit_ducks_the_music_for_150_ms() {
    let t = MusicTuning::default();
    assert!((t.hit_duck_seconds - 0.15).abs() < 1e-6);
    let mut d = MusicDirector::default();
    let fight = play(Some(view(2, RunPhase::Fighting, 1, 0)));
    run_for(&mut d, fight, 6.0, STINGS);
    let mut ducks = vec![d.step(DT, &fight, &t, STINGS, true).duck];
    for _ in 0..40 {
        ducks.push(d.step(DT, &fight, &t, STINGS, false).duck);
    }
    let held = (0.15 / DT) as usize;
    assert!(
        ducks[..held].iter().all(|g| (g - t.hit_duck).abs() < 1e-6),
        "{ducks:?}"
    );
    let back = ((0.15 + t.hit_release) / DT).ceil() as usize + 1;
    assert!(
        (ducks[back] - 1.0).abs() < 1e-6,
        "back by {back}: {ducks:?}"
    );
    assert!(ducks[held + 1] > t.hit_duck, "releasing after 150 ms");
}

#[test]
fn a_pause_dips_the_music_and_freezes_the_run_beats() {
    let t = MusicTuning::default();
    let mut d = MusicDirector::default();
    let run = view(2, RunPhase::Fighting, 1, 0);
    run_for(&mut d, play(Some(run)), 7.0, STINGS);
    let paused = MusicInput {
        screen: Screen::Paused,
        run: Some(run),
    };
    let frames = run_for(&mut d, paused, 1.0, STINGS);
    let low = slot_gain(frames.last().unwrap(), MusicSlot::CombatLow);
    assert!(
        (low - mix(MusicSlot::CombatLow) * t.pause_gain).abs() < 1e-4,
        "{low}"
    );
    assert!(frames.iter().all(|f| f.sting.is_none()));
    let frames = run_for(&mut d, play(Some(run)), 1.0, STINGS);
    assert!(
        frames.iter().all(|f| f.sting.is_none()),
        "resuming isn't a wave start"
    );
    assert!(
        (slot_gain(frames.last().unwrap(), MusicSlot::CombatLow) - mix(MusicSlot::CombatLow)).abs()
            < 1e-4
    );
}

// ---------------------------------------------------------------------------
// The sliders
// ---------------------------------------------------------------------------

#[test]
fn the_music_and_effects_sliders_are_silent_at_exactly_zero() {
    let mut audio = AudioTuning::default();
    assert!(music_volume(&audio) > 0.0 && audio.effects_gain() > 0.0);
    audio.music_volume = 0.0;
    assert_eq!(music_volume(&audio), 0.0);
    assert!(audio.effects_gain() > 0.0, "the sliders are independent");
    // Every gain the score would set is exactly zero.
    let mut d = MusicDirector::default();
    let t = MusicTuning::default();
    for input in [menu(), play(Some(view(1, RunPhase::Fighting, 7, 8)))] {
        for _ in 0..200 {
            let f = d.step(DT, &input, &t, STINGS, false);
            for g in f.gains.iter().chain([&f.sting_gain]) {
                assert_eq!(g * music_volume(&audio), 0.0);
            }
        }
    }
    audio.music_volume = 1.0;
    audio.effects_volume = 0.0;
    assert_eq!(audio.effects_gain(), 0.0);
    assert!(music_volume(&audio) > 0.0);
    // Master and mute silence both.
    let mut audio = AudioTuning {
        master_volume: 0.0,
        ..default()
    };
    assert_eq!((music_volume(&audio), audio.effects_gain()), (0.0, 0.0));
    audio.master_volume = 1.0;
    audio.muted = true;
    assert_eq!((music_volume(&audio), audio.effects_gain()), (0.0, 0.0));
}

#[test]
fn the_music_and_effects_sliders_are_menu_settings_that_persist() {
    use pieced::menu::{Setting, SettingKind};
    let dir = std::env::temp_dir().join(format!("pieced-music-{}", std::process::id()));
    let path = dir.join("settings.json");
    let mut t = Tuning::default();
    for (setting, value) in [(Setting::Music, 0.35), (Setting::Effects, 0.0)] {
        assert!(Setting::ALL.contains(&setting));
        assert!(matches!(setting.kind(), SettingKind::Slider { min, .. } if min == 0.0));
        setting.set(&mut t, value);
    }
    assert_eq!(t.audio.music_volume, 0.35);
    assert_eq!(t.audio.effects_volume, 0.0);
    // The designer section is never saved: a changed file can't freeze it.
    t.music.crossfade_seconds = 9.0;
    t.save(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        !text.contains("crossfade_seconds"),
        "the score's tuning isn't persisted"
    );
    let loaded = Tuning::load_or_default(&path);
    assert_eq!(loaded.audio.music_volume, 0.35);
    assert_eq!(loaded.audio.effects_volume, 0.0);
    assert_eq!(loaded.music, MusicTuning::default());
    let _ = std::fs::remove_dir_all(dir);
}

// ---------------------------------------------------------------------------
// Loading: off the main thread, out of the launch
// ---------------------------------------------------------------------------

fn audio_app(load_music: bool) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin, AssetPlugin::default()))
        .init_state::<AppState>()
        .init_resource::<Tuning>()
        .init_asset::<AudioSource>()
        .add_message::<DamageDealt>()
        .add_message::<ShotFired>()
        .add_message::<PieceChanged>()
        .add_message::<Eliminated>()
        .add_message::<GameCue>()
        .add_message::<pieced::orb::OrbImpact>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            DT,
        )));
    if load_music {
        app.insert_resource(LoadMusic);
    }
    app
}

#[test]
fn synthesis_and_decoding_run_off_the_main_thread_and_never_hold_up_launch() {
    let mut app = audio_app(true);
    // The main thread's share of the launch: building the plugin and the
    // first frames. The bank's synthesis, its rooms, the decoding and the
    // celesta all run on their own threads meanwhile.
    let start = Instant::now();
    app.add_plugins(GameAudioPlugin);
    app.finish();
    app.cleanup();
    for _ in 0..3 {
        app.update();
    }
    let main_ms = start.elapsed().as_secs_f64() * 1000.0;
    let world = app.world();
    let bank_thread = world.resource::<SoundBankJob>().thread_name();
    let music_thread = world.resource::<MusicLoader>().thread_name();
    assert_eq!(bank_thread.as_deref(), Some("sound-bank"));
    assert_eq!(music_thread.as_deref(), Some("music-bank"));
    assert_ne!(std::thread::current().name(), Some("sound-bank"));
    println!("launch: {main_ms:.1} ms on the main thread for the audio plugin and 3 frames");
    assert!(main_ms < 150.0, "audio added {main_ms:.0} ms to the launch");

    // Everything arrives and is filed without the main thread doing the work.
    let start = Instant::now();
    world.resource::<SoundBankJob>().wait();
    world.resource::<MusicLoader>().wait();
    let background_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    app.update();
    let filing_ms = start.elapsed().as_secs_f64() * 1000.0;
    println!(
        "background work finished {background_ms:.0} ms later; filing it took {filing_ms:.1} ms"
    );
    assert!(
        filing_ms < 150.0,
        "filing the finished work took {filing_ms:.0} ms"
    );
    let world = app.world();
    assert!(world.contains_resource::<SoundBank>());
    assert!(
        world.resource::<MusicBank>().is_complete(),
        "every track loaded"
    );
    assert!(
        !world.contains_resource::<MusicLoader>(),
        "the music thread is done"
    );
    for track in Track::ALL {
        assert!(
            world.resource::<MusicBank>().seconds(track) > 1.0,
            "{track:?}"
        );
    }
    let clips = world.resource::<Assets<MusicClip>>();
    let bank = world.resource::<MusicBank>();
    let menu = clips.get(bank.get(Track::Menu).unwrap()).unwrap();
    assert_eq!(menu.loop_start(), Some(0));
    let brk = clips.get(bank.get(Track::Break).unwrap()).unwrap();
    assert_eq!(brk.frames(), celesta::FRAMES);
}

// ---------------------------------------------------------------------------
// Five simulated minutes of waves allocate nothing per event
// ---------------------------------------------------------------------------

struct CountingAlloc;

static ALLOCATIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

thread_local! {
    static ARMED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

// SAFETY: forwards every call to the system allocator unchanged.
unsafe impl std::alloc::GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        if ARMED.with(std::cell::Cell::get) {
            ALLOCATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        unsafe { std::alloc::System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, size: usize) -> *mut u8 {
        if ARMED.with(std::cell::Cell::get) {
            ALLOCATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        unsafe { std::alloc::System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

fn allocations_in(f: impl FnOnce()) -> u64 {
    let before = ALLOCATIONS.load(std::sync::atomic::Ordering::Relaxed);
    ARMED.with(|a| a.set(true));
    f();
    ARMED.with(|a| a.set(false));
    ALLOCATIONS.load(std::sync::atomic::Ordering::Relaxed) - before
}

/// A scripted run: 40 s waves (knights landing up to eight at once), 10 s
/// breaks, a death every third wave then the results (a new best every other
/// time) and "Go again"; the player firing, hitting, getting kills, building
/// and running the whole time.
#[derive(Resource, Default)]
struct Script {
    frame: u64,
    seed: u64,
}

#[allow(clippy::too_many_arguments)]
fn drive_script(
    mut script: ResMut<Script>,
    mut summary: ResMut<RunSummary>,
    mut run: ResMut<Run>,
    player: Single<Entity, With<Player>>,
    mut shots: MessageWriter<ShotFired>,
    mut damage: MessageWriter<DamageDealt>,
    mut pieces: MessageWriter<PieceChanged>,
    mut cues: MessageWriter<GameCue>,
    mut kills: MessageWriter<KillConfirmed>,
) {
    script.frame += 1;
    let f = script.frame;
    let cycle = 60 * 60; // one wave and its break, frames
    let in_cycle = f % cycle;
    let wave_of_run = (f / cycle) % 3 + 1;
    if in_cycle == 0 && wave_of_run == 1 {
        script.seed += 1;
    }
    summary.seed = script.seed;
    summary.wave = wave_of_run as u32 + (script.seed % 2) as u32 * 5;
    let fighting = in_cycle < 40 * 60;
    summary.phase = if fighting {
        RunPhase::Fighting
    } else if wave_of_run == 3 && in_cycle < 43 * 60 {
        RunPhase::Dying { until: 0 }
    } else if wave_of_run == 3 {
        RunPhase::Over { tick: 0 }
    } else {
        RunPhase::Break { ends_tick: 0 }
    };
    summary.new_best = script.seed.is_multiple_of(2);
    run.alive = ((in_cycle / 300) % 9) as u32;
    let who = *player;
    if fighting {
        if f.is_multiple_of(10) {
            shots.write(ShotFired {
                shooter: who,
                weapon: if f.is_multiple_of(70) {
                    WeaponKind::Pump
                } else {
                    WeaponKind::Rifle
                },
                origin: Vec3::ZERO,
                traces: Vec::new(),
                tick: f,
            });
        }
        if f.is_multiple_of(20) {
            damage.write(DamageDealt {
                source: Some(who),
                target: who,
                target_kind: DamageTarget::Character,
                amount: 10.0,
                headshot: f.is_multiple_of(60),
                to_shield: 0.0,
                shield_broke: f.is_multiple_of(240),
                killed: false,
                point: Vec3::ZERO,
                normal: Vec3::Y,
                tick: f,
            });
        }
        if f.is_multiple_of(90) {
            kills.write(KillConfirmed {
                victim: who,
                at: Vec3::ZERO,
                headshot: false,
                void: false,
                chain: 1 + (f / 90 % 3) as u32,
                tick: f,
            });
        }
    }
    if f.is_multiple_of(120) {
        pieces.write(PieceChanged {
            entity: who,
            kind: PieceKind::Wall,
            change: PieceChange::Placed,
            center: Vec3::new(4.0, 1.5, 0.0),
            tick: f,
        });
    }
    if f % 20 == 5 {
        cues.write(GameCue::Footstep { who });
    }
}

fn scripted_app(audio: bool) -> App {
    let mut app = audio_app(audio);
    if audio {
        app.add_plugins(GameAudioPlugin);
    } else {
        app.init_resource::<pieced::audio::music::MusicState>();
    }
    app.add_message::<KillConfirmed>()
        .init_resource::<Script>()
        .insert_resource(RunSummary::default())
        .insert_resource(Run::new(1, 0, &WavesTuning::default()))
        .add_systems(
            Update,
            drive_script.before(pieced::fx::kills::KillFeedbackSet),
        );
    app.world_mut().spawn((Player, Transform::default()));
    app.finish();
    app.cleanup();
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Playing);
    if audio {
        app.world().resource::<SoundBankJob>().wait();
        app.world().resource::<MusicLoader>().wait();
    }
    for (_, schedule) in app.world_mut().resource_mut::<Schedules>().iter_mut() {
        schedule.set_executor(SingleThreadedExecutor::new());
    }
    app
}

#[test]
fn five_minutes_of_waves_allocate_nothing_per_event() {
    let mut plain = scripted_app(false);
    let mut audio = scripted_app(true);
    // Warm up one full script cycle (every sound, sting and slot once), so
    // pools and scratch lists have grown.
    for _ in 0..3 * 60 * 60 {
        plain.update();
        audio.update();
    }
    assert!(audio.world().resource::<MusicBank>().is_complete());
    let base = allocations_in(|| {
        for _ in 0..5 * 60 * 60 {
            plain.update();
        }
    });
    let started = std::cell::Cell::new(0u32);
    let with = allocations_in(|| {
        for _ in 0..5 * 60 * 60 {
            audio.update();
            let messages = audio.world().resource::<Messages<WaveStarted>>();
            started.set(started.get() + messages.iter_current_update_messages().count() as u32);
        }
    });
    let world = audio.world_mut();
    let voices = world.query::<&Voice>().iter(world).count();
    let state = world.resource::<MusicState>();
    println!(
        "5 min of waves: {base} allocations without audio, {with} with; {} waves started; {voices} voices",
        started.get()
    );
    assert!(
        started.get() >= 5,
        "the script started waves: {}",
        started.get()
    );
    assert!(voices > 0, "sounds played");
    assert!(state.last.voices.iter().any(|v| *v), "music played");
    assert!(
        with <= base,
        "the audio allocated {} times over 5 minutes",
        with.saturating_sub(base)
    );
}

// ---------------------------------------------------------------------------
// The gun's handling plays on the viewmodel's beats (chunk 2's WeaponCue)
// ---------------------------------------------------------------------------

fn sounding(app: &mut App) -> Vec<pieced::audio::Sfx> {
    let world = app.world_mut();
    let mut sfx: Vec<_> = world
        .query::<&Voice>()
        .iter(world)
        .map(Voice::sfx)
        .collect();
    sfx.sort_by_key(|s| *s as usize);
    sfx
}

fn handling_app(beats: bool) -> (App, Entity) {
    let mut app = audio_app(false);
    app.add_plugins(GameAudioPlugin);
    if beats {
        app.init_resource::<pieced::audio::WeaponBeatSounds>();
    }
    let player = app.world_mut().spawn((Player, Transform::default())).id();
    app.finish();
    app.cleanup();
    app.world().resource::<SoundBankJob>().wait();
    app.update();
    assert!(app.world().contains_resource::<SoundBank>());
    (app, player)
}

#[test]
fn the_guns_handling_plays_on_the_viewmodels_beats() {
    use pieced::{
        audio::{Sfx, beat_sound},
        viewmodel::{WeaponBeat, WeaponCue},
    };
    let beats = [
        WeaponBeat::Draw,
        WeaponBeat::AdsIn,
        WeaponBeat::AdsOut,
        WeaponBeat::ChamberOpen,
        WeaponBeat::CrystalPop,
        WeaponBeat::CrystalGrab,
        WeaponBeat::CrystalSlot,
        WeaponBeat::ChamberShut,
        WeaponBeat::CrystalCharged,
        WeaponBeat::ShardPush,
        WeaponBeat::RackPull,
        WeaponBeat::RackClack,
    ];
    // Every beat has its own layered sound.
    let sounds: BTreeSet<usize> = beats.iter().map(|b| beat_sound(*b) as usize).collect();
    assert_eq!(sounds.len(), beats.len());

    // With the viewmodel: each beat sounds on the frame it's announced.
    let (mut app, player) = handling_app(true);
    for beat in beats {
        app.world_mut().write_message(WeaponCue {
            weapon: WeaponKind::Rifle,
            beat,
        });
        app.update();
        assert!(
            sounding(&mut app).contains(&beat_sound(beat)),
            "{beat:?} plays {:?}",
            beat_sound(beat)
        );
    }
    // ...and the old reload cues stay quiet, so nothing doubles.
    let before = sounding(&mut app).len();
    for cue in [
        GameCue::ReloadStart {
            who: player,
            weapon: WeaponKind::Rifle,
        },
        GameCue::ReloadDone {
            who: player,
            weapon: WeaponKind::Pump,
        },
        GameCue::AdsChanged {
            who: player,
            ads: true,
        },
    ] {
        app.world_mut().write_message(cue);
    }
    app.update();
    assert_eq!(
        sounding(&mut app).len(),
        before,
        "the beats own the handling sounds"
    );

    // Without the viewmodel (headless), the reload cues still sound, and
    // stray beats are ignored.
    let (mut app, player) = handling_app(false);
    app.world_mut().write_message(WeaponCue {
        weapon: WeaponKind::Pump,
        beat: WeaponBeat::RackClack,
    });
    app.world_mut().write_message(GameCue::ReloadStart {
        who: player,
        weapon: WeaponKind::Rifle,
    });
    app.update();
    assert_eq!(sounding(&mut app), vec![Sfx::RifleMagOut]);
}

/// Renders the break loop and the death sting to WAV files for listening and
/// offline analysis (`PIECED_MUSIC_WAV_DIR`).
#[test]
#[ignore = "writes WAV files: run by hand"]
fn write_music_wavs() {
    let Some(dir) = std::env::var_os("PIECED_MUSIC_WAV_DIR").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let write = |name: &str, pcm: &[i16]| {
        let mut b = Vec::new();
        let data = (pcm.len() * 2) as u32;
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&MUSIC_RATE.to_le_bytes());
        b.extend_from_slice(&(MUSIC_RATE * 4).to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data.to_le_bytes());
        for s in pcm {
            b.extend_from_slice(&s.to_le_bytes());
        }
        std::fs::write(dir.join(name), b).unwrap();
    };
    write("break.wav", break_loop());
    write("death_sting.wav", &death_sting(&decoded()["death_chord"]));
}
