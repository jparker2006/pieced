//! Milestone 2 sound bank: every cue's length, loudness and ceiling, the pitch
//! sweeps that define the rifle zap and the elimination whistle, the brick/plank
//! cue selection, the mix that keeps hits above the player's own casts, and the
//! startup cost.
//!
//! Nothing here plays audio: the cues are rendered to samples and analyzed.
//! `cargo test --test audio -- --nocapture` prints the bank table.

use pieced::{
    audio::{
        Sfx, SfxCategory, bank,
        bank::LOUDNESS_BAND,
        duck_gain, render_bank,
        synth::{PEAK_CEILING, SAMPLE_RATE, Svf, WavInfo, gain_to_db, peak, short_term_rms_db},
    },
    shared::{PieceChange, PieceKind},
};
use std::f32::consts::PI;

const SR: f32 = SAMPLE_RATE as f32;

fn every_take() -> impl Iterator<Item = (Sfx, u32, Vec<f32>)> {
    Sfx::ALL
        .into_iter()
        .flat_map(|sfx| (0..sfx.takes()).map(move |take| (sfx, take, sfx.synthesize_take(take))))
}

fn seconds(samples: &[f32]) -> f32 {
    samples.len() as f32 / SR
}

fn slice(samples: &[f32], from: f32, to: f32) -> &[f32] {
    &samples[(from * SR) as usize..((to * SR) as usize).min(samples.len())]
}

/// Magnitude of the Hann-windowed DFT of `x` at `freq`.
fn dft_magnitude(x: &[f32], freq: f32) -> f32 {
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

/// The strongest frequency in `x` between `lo` and `hi` Hz (10 Hz steps).
fn dominant_frequency(x: &[f32], lo: f32, hi: f32) -> f32 {
    let mut best = (0.0, lo);
    let mut f = lo;
    while f <= hi {
        let m = dft_magnitude(x, f);
        if m > best.0 {
            best = (m, f);
        }
        f += 10.0;
    }
    best.1
}

/// Energy between 4 and 10 kHz (4th-order band edges) at `gain`, over the first
/// 100 ms: the band the hit sparkle owns.
fn sparkle_band_energy(x: &[f32], gain: f32) -> f32 {
    let mut filters = [
        Svf::default(),
        Svf::default(),
        Svf::default(),
        Svf::default(),
    ];
    x.iter()
        .take((0.1 * SR) as usize)
        .map(|s| {
            let [a, b, c, d] = &mut filters;
            let high = b.high(a.high(*s, 4000.0), 4000.0);
            (d.low(c.low(high, 10_000.0), 10_000.0) * gain).powi(2)
        })
        .sum()
}

/// Time (s) from the loudest 5 ms frame until the level falls 30 dB below it.
fn decay_time(x: &[f32]) -> f32 {
    let frame = (0.005 * SR) as usize;
    let env: Vec<f32> = x
        .chunks(frame)
        .map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    let (loudest, top) = env
        .iter()
        .enumerate()
        .fold((0, 0.0f32), |a, (i, e)| if *e > a.1 { (i, *e) } else { a });
    let floor = top * 10f32.powf(-30.0 / 20.0);
    let end = (loudest..env.len())
        .find(|&i| env[i] < floor)
        .unwrap_or(env.len());
    (end - loudest) as f32 * 0.005
}

/// In-game level of a cue (short-term RMS at its mix gain), dBFS.
fn mix_level_db(sfx: Sfx) -> f32 {
    short_term_rms_db(&sfx.synthesize()) + gain_to_db(sfx.base_volume())
}

#[test]
fn every_cue_is_audible_within_its_budget_and_never_clips() {
    for (sfx, take, s) in every_take() {
        let spec = sfx.spec();
        let len = seconds(&s);
        assert!(
            (0.05..=spec.max_seconds).contains(&len),
            "{sfx:?}#{take}: {len:.3} s over its {} s budget",
            spec.max_seconds
        );
        assert!(s.iter().all(|v| v.is_finite()), "{sfx:?}#{take}");
        let p = peak(&s);
        assert!(
            p <= PEAK_CEILING,
            "{sfx:?}#{take} peaks at {:.2} dBFS",
            gain_to_db(p)
        );
        assert!(p >= 0.5, "{sfx:?}#{take} is too quiet: peak {p}");
        assert!(
            short_term_rms_db(&s) > -20.0,
            "{sfx:?}#{take} is near silent"
        );
        // The encoded file clips nowhere either (−1 dBFS is 29 204 of 32 767).
        let wav = pieced::audio::synth::encode_wav(&s);
        let info = WavInfo::parse(&wav).expect("valid WAV");
        assert_eq!(info.frames as usize, s.len());
        let loudest = wav[44..]
            .chunks(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]).unsigned_abs())
            .max()
            .unwrap();
        assert!(loudest <= 29_205, "{sfx:?}#{take}: sample {loudest}");
        assert!(
            s.last().unwrap().abs() < 1e-3,
            "{sfx:?}#{take} ends in a click"
        );
    }
}

#[test]
fn every_cue_meets_its_documented_loudness_target() {
    println!(
        "{:<13} {:>5} {:>7} {:>7} {:>7}",
        "cue", "len", "rms", "peak", "mix"
    );
    for (sfx, take, s) in every_take() {
        let target = sfx.spec().rms_db;
        assert!(
            (LOUDNESS_BAND.0..=LOUDNESS_BAND.1).contains(&target),
            "{sfx:?}: target {target} outside the documented band"
        );
        let rms = short_term_rms_db(&s);
        assert!(
            (rms - target).abs() <= 0.5,
            "{sfx:?}#{take}: {rms:.2} dBFS vs target {target}"
        );
        if take == 0 {
            println!(
                "{:<13} {:>5.2} {:>7.1} {:>7.1} {:>7.1}",
                format!("{sfx:?}"),
                seconds(&s),
                rms,
                gain_to_db(peak(&s)),
                mix_level_db(sfx)
            );
        }
    }
}

#[test]
fn the_bank_is_deterministic_and_its_takes_differ() {
    // The bank is indexed by `Sfx as usize`.
    for (i, sfx) in Sfx::ALL.into_iter().enumerate() {
        assert_eq!(sfx as usize, i, "{sfx:?}");
    }
    assert_eq!(render_bank(), render_bank());
    for sfx in [Sfx::RifleShot, Sfx::Footstep] {
        assert!(sfx.takes() >= 3, "{sfx:?} round-robins");
        let takes: Vec<Vec<f32>> = (0..sfx.takes()).map(|t| sfx.synthesize_take(t)).collect();
        for (i, a) in takes.iter().enumerate() {
            for b in &takes[i + 1..] {
                assert_ne!(a, b, "{sfx:?}: takes must differ");
            }
        }
        // Takes wrap around.
        assert_eq!(sfx.synthesize_take(sfx.takes()), takes[0]);
    }
}

#[test]
fn the_rifle_zap_sweeps_down_fast() {
    for take in 0..Sfx::RifleShot.takes() {
        let zap = Sfx::RifleShot.synthesize_take(take);
        let windows = [
            (0.002, 0.012),
            (0.012, 0.024),
            (0.024, 0.040),
            (0.040, 0.060),
        ];
        let pitch: Vec<f32> = windows
            .iter()
            .map(|&(a, b)| dominant_frequency(slice(&zap, a, b), 150.0, 4000.0))
            .collect();
        assert!(
            pitch.windows(2).all(|w| w[1] < w[0]),
            "take {take}: pitch must fall every window: {pitch:?}"
        );
        assert!(pitch[0] > 1800.0, "take {take} starts bright: {pitch:?}");
        assert!(
            pitch[3] < pitch[0] / 2.5,
            "take {take} falls over an octave in 50 ms: {pitch:?}"
        );
    }
}

#[test]
fn the_elimination_slide_whistle_glides_down() {
    let cue = Sfx::Elimination.synthesize();
    let start = bank::WHISTLE_START;
    // After the poof has died away, every 80 ms along the glide.
    let times: Vec<f32> = (0..6).map(|i| start + 0.04 + 0.08 * i as f32).collect();
    let pitch: Vec<f32> = times
        .iter()
        .map(|&t| dominant_frequency(slice(&cue, t, t + 0.023), 200.0, 2500.0))
        .collect();
    assert!(
        pitch.windows(2).all(|w| w[1] < w[0]),
        "the whistle falls the whole way: {pitch:?}"
    );
    assert!(
        (pitch[0] / bank::WHISTLE_FROM_HZ - 1.0).abs() < 0.1,
        "starts near {} Hz: {pitch:?}",
        bank::WHISTLE_FROM_HZ
    );
    let octaves = (pitch[0] / pitch[5]).log2();
    assert!(
        octaves > 1.4,
        "falls well over an octave, not {octaves:.2}: {pitch:?}"
    );
}

#[test]
fn walls_clunk_and_floors_and_ramps_thock() {
    use PieceChange::*;
    for change in [Placed, Cracked(1), Cracked(2), Destroyed] {
        let (brick, plank) = match change {
            Placed => (Sfx::BrickPlace, Sfx::PlankPlace),
            Cracked(_) => (Sfx::BrickCrack, Sfx::PlankCrack),
            Destroyed => (Sfx::BrickBreak, Sfx::PlankBreak),
        };
        assert_eq!(Sfx::for_piece(PieceKind::Wall, change), brick, "{change:?}");
        assert_eq!(
            Sfx::for_piece(PieceKind::Floor, change),
            plank,
            "{change:?}"
        );
        assert_eq!(Sfx::for_piece(PieceKind::Ramp, change), plank, "{change:?}");
    }
    // The clunk is dull and short; the thock is hollow and rings on.
    let clunk = decay_time(&Sfx::BrickPlace.synthesize());
    let thock = decay_time(&Sfx::PlankPlace.synthesize());
    assert!(
        thock > 1.3 * clunk,
        "the thock rings longer: {thock:.3} s vs {clunk:.3} s"
    );
    for sfx in [
        Sfx::BrickPlace,
        Sfx::PlankPlace,
        Sfx::BrickBreak,
        Sfx::PlankBreak,
    ] {
        assert_eq!(sfx.category(), SfxCategory::Building);
    }
}

#[test]
fn hits_cut_through_the_players_own_casts() {
    let hits: Vec<Sfx> = Sfx::ALL
        .into_iter()
        .filter(|s| s.category() == SfxCategory::Hits)
        .collect();
    assert_eq!(hits.len(), 5);
    for &hit in &hits {
        assert!(
            mix_level_db(hit) >= mix_level_db(Sfx::RifleShot) + 3.5,
            "{hit:?} sits well above the rifle"
        );
        assert!(
            mix_level_db(hit) >= mix_level_db(Sfx::PumpShot) + 0.5,
            "{hit:?} sits above the pump"
        );
    }
    // A hit landing with its own shot dips that shot.
    let duck = pieced::tuning::Tuning::default().audio.hit_duck;
    assert!(duck < 0.8 && duck > 0.4, "duck {duck}");
    // In the sparkle band the hit dominates the ducked cast it lands with.
    for hit in [Sfx::HitTick, Sfx::HeadshotDing, Sfx::ShieldHit] {
        let h = sparkle_band_energy(&hit.synthesize(), hit.base_volume());
        for gun in [Sfx::RifleShot, Sfx::PumpShot] {
            let g = sparkle_band_energy(&gun.synthesize(), gun.base_volume() * duck);
            let margin = 10.0 * (h / g).log10();
            assert!(
                margin >= 6.0,
                "{hit:?} over {gun:?} at 4-10 kHz: {margin:.1} dB"
            );
        }
    }
}

#[test]
fn the_mix_keeps_movement_soft_and_weapons_under_hits() {
    for sfx in Sfx::ALL {
        let v = sfx.base_volume();
        assert!(v > 0.0 && v <= 1.0, "{sfx:?}: {v}");
        assert!(
            (mix_level_db(sfx) - sfx.mix_db()).abs() <= 0.5,
            "{sfx:?} plays at its documented mix level"
        );
    }
    let loudest = |cat: SfxCategory| {
        Sfx::ALL
            .into_iter()
            .filter(|s| s.category() == cat)
            .map(Sfx::mix_db)
            .fold(f32::MIN, f32::max)
    };
    let quietest = |cat: SfxCategory| {
        Sfx::ALL
            .into_iter()
            .filter(|s| s.category() == cat)
            .map(Sfx::mix_db)
            .fold(f32::MAX, f32::min)
    };
    assert!(quietest(SfxCategory::Hits) > loudest(SfxCategory::Weapons));
    assert!(quietest(SfxCategory::Hits) > loudest(SfxCategory::Building));
    assert!(loudest(SfxCategory::Movement) <= quietest(SfxCategory::Weapons));
    assert!(Sfx::Footstep.mix_db() <= -25.0, "footsteps stay soft");
}

#[test]
fn only_the_players_own_weapons_duck_and_only_on_a_hit_frame() {
    assert_eq!(duck_gain(Sfx::RifleShot, true, true, 0.7), 0.7);
    assert_eq!(duck_gain(Sfx::PumpShot, true, true, 0.7), 0.7);
    assert_eq!(duck_gain(Sfx::RifleShot, true, false, 0.7), 1.0);
    assert_eq!(
        duck_gain(Sfx::RifleShot, false, true, 0.7),
        1.0,
        "others' shots"
    );
    assert_eq!(
        duck_gain(Sfx::HitTick, true, true, 0.7),
        1.0,
        "the hit itself"
    );
    assert_eq!(duck_gain(Sfx::Footstep, true, true, 0.7), 1.0);
    assert_eq!(
        duck_gain(Sfx::RifleShot, true, true, 1.7),
        1.0,
        "never boosts"
    );
}

#[test]
fn the_bank_builds_fast_enough_for_launch() {
    // Warm up once (page faults, lazy statics), then time a full build.
    let _ = render_bank();
    let start = std::time::Instant::now();
    let bank = render_bank();
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    println!("sound bank build: {ms:.1} ms");
    assert_eq!(bank.len(), Sfx::ALL.len());
    // Launch must stay under 5 s; the budget for the bank is ~150 ms in release.
    // Tests run unoptimized and in parallel with other suites, so the bound here
    // is generous and only catches a real regression.
    assert!(ms < 750.0, "the bank took {ms:.0} ms");
}
