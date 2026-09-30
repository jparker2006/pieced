//! The knights' personality in sound (M4 chunk 6, D123): the barks judged by
//! analysis (pitch contours, voicing, length), their rate limits and voice
//! cap, the fairness rule (a warning always wins over a bark), the knights'
//! spatial footsteps, and the pan fix for every spatial sound.
//!
//! Nothing plays aloud. `PIECED_BARK_WAVS=<dir> cargo test --test voice --
//! --nocapture render_the_barks` writes every bark take as a WAV to listen to.

use bevy::prelude::*;
use pieced::{
    audio::{
        Sfx,
        barks::{
            Bark, BarkGate, BarkLog, GLOBAL_GAP, KNIGHT_GAP, KnightVoiceTracking, MAX_BARKS,
            TAUNT_EVERY, TAUNT_GAP, WARNING_GUARD, personal_pitch, step_for, taunt_turn,
            yelp_jitter,
        },
        spatial::{listener, speaker_gains},
        synth::{SAMPLE_RATE, encode_wav, short_term_rms_db},
    },
    grunt::Grunt,
    shared::{PieceKind, TICK_SECONDS},
    sim::Sim,
    waves::Run,
};

const SR: f32 = SAMPLE_RATE as f32;

/// The pitch (Hz) of `x` by autocorrelation between 150 and 1200 Hz, and how
/// periodic it is (0..1: the normalized peak).
fn pitch(x: &[f32]) -> (f32, f32) {
    let energy: f32 = x.iter().map(|s| s * s).sum();
    if energy < 1e-6 {
        return (0.0, 0.0);
    }
    let (lo, hi) = ((SR / 1200.0) as usize, (SR / 150.0) as usize);
    let mut rs = Vec::new();
    for lag in lo..hi.min(x.len() / 2) {
        let c: f32 = x.iter().zip(&x[lag..]).map(|(a, b)| a * b).sum();
        let norm = (x[..x.len() - lag].iter().map(|s| s * s).sum::<f32>()
            * x[lag..].iter().map(|s| s * s).sum::<f32>())
        .sqrt()
        .max(1e-9);
        rs.push((c / norm, lag));
    }
    // The shortest lag nearly as periodic as the best (not a subharmonic).
    let top = rs.iter().map(|r| r.0).fold(0.0f32, f32::max);
    let best = rs
        .iter()
        .enumerate()
        .find(|(i, r)| {
            r.0 >= 0.93 * top
                && rs.get(i + 1).is_none_or(|n| n.0 <= r.0)
                && (*i == 0 || rs[i - 1].0 <= r.0)
        })
        .map_or((0.0, lo), |(_, r)| *r);
    (SR / best.1 as f32, best.0)
}

/// Pitch over time: (seconds, Hz, periodicity) for 30 ms windows every 20 ms
/// that carry at least a tenth of the loudest window's energy.
fn contour(x: &[f32]) -> Vec<(f32, f32, f32)> {
    let win = (0.03 * SR) as usize;
    let hop = (0.02 * SR) as usize;
    let rms = |w: &[f32]| (w.iter().map(|s| s * s).sum::<f32>() / w.len() as f32).sqrt();
    let loudest = x.windows(win).step_by(hop).map(rms).fold(0.0f32, f32::max);
    x.windows(win)
        .step_by(hop)
        .enumerate()
        .filter(|(_, w)| rms(w) > 0.3 * loudest)
        .map(|(i, w)| {
            let (f, p) = pitch(w);
            (i as f32 * 0.02, f, p)
        })
        .collect()
}

/// The dry bark (no room), as designed.
fn dry(sfx: Sfx, take: u32) -> Vec<f32> {
    sfx.dry_take(take)
}

fn voiced(c: &[(f32, f32, f32)]) -> Vec<(f32, f32)> {
    c.iter()
        .filter(|(_, _, p)| *p > 0.6)
        .map(|(t, f, _)| (*t, *f))
        .collect()
}

#[test]
fn render_the_barks() {
    let out = std::env::var_os("PIECED_BARK_WAVS").map(std::path::PathBuf::from);
    println!(
        "{:<16} {:>5} {:>6}  pitch contour (Hz, voiced windows)",
        "bark", "len", "rms"
    );
    for sfx in [
        Sfx::BarkHup,
        Sfx::BarkTaunt,
        Sfx::BarkYelp,
        Sfx::BarkHooHah,
        Sfx::BarkWhaaa,
        Sfx::KnightStepGrass,
        Sfx::KnightStepWood,
        Sfx::KnightStepBrick,
        Sfx::Birdsong,
    ] {
        for take in 0..sfx.takes() {
            let played = sfx.synthesize_take(take);
            let c = voiced(&contour(&dry(sfx, take)));
            let shown: Vec<String> = c.iter().map(|(_, f)| format!("{f:.0}")).collect();
            println!(
                "{:<16} {:>5.2} {:>6.1}  {}",
                format!("{sfx:?}#{take}"),
                played.len() as f32 / SR,
                short_term_rms_db(&played),
                shown.join(" ")
            );
            if let Some(dir) = &out {
                std::fs::create_dir_all(dir).unwrap();
                std::fs::write(dir.join(format!("{sfx:?}-{take}.wav")), encode_wav(&played))
                    .unwrap();
            }
        }
    }
}

#[test]
fn every_bark_is_a_voice_with_its_designed_melody() {
    // Every bark is voiced (clearly periodic for most of it) in a cartoon
    // register: 220–950 Hz.
    for (sfx, takes) in [
        (Sfx::BarkHup, 1),
        (Sfx::BarkTaunt, 3),
        (Sfx::BarkYelp, 4),
        (Sfx::BarkHooHah, 1),
        (Sfx::BarkWhaaa, 1),
    ] {
        for take in 0..takes {
            let all = contour(&dry(sfx, take));
            let v = voiced(&all);
            assert!(
                v.len() * 2 >= all.len(),
                "{sfx:?}#{take}: only {} of {} windows voiced",
                v.len(),
                all.len()
            );
            for (t, f) in &v {
                assert!((200.0..=950.0).contains(f), "{sfx:?}#{take} at {t}: {f} Hz");
            }
        }
    }
    let first_last = |sfx: Sfx, take: u32| {
        let v = voiced(&contour(&dry(sfx, take)));
        (v.first().unwrap().1, v.last().unwrap().1)
    };
    // "Hup!" rises.
    let (a, b) = first_last(Sfx::BarkHup, 0);
    assert!(b > a * 1.1, "hup rises: {a} -> {b}");
    // "Whaaaa…" slides down most of an octave.
    let (a, b) = first_last(Sfx::BarkWhaaa, 0);
    assert!(b < a * 0.62, "whaaa falls: {a} -> {b}");
    // "Ow!" falls; "eep!" and "yip!" rise.
    let (a, b) = first_last(Sfx::BarkYelp, 0);
    assert!(b < a * 0.85, "ow falls: {a} -> {b}");
    for take in [1, 3] {
        let (a, b) = first_last(Sfx::BarkYelp, take);
        assert!(b > a, "yelp {take} rises: {a} -> {b}");
    }
    // "Hoo-HAH!": the "hah" sits well above the "hoo".
    let v = voiced(&contour(&dry(Sfx::BarkHooHah, 0)));
    let hoo = v
        .iter()
        .filter(|(t, _)| *t < 0.2)
        .map(|(_, f)| *f)
        .fold(0.0, f32::max);
    let hah = v
        .iter()
        .filter(|(t, _)| *t > 0.3)
        .map(|(_, f)| *f)
        .fold(0.0, f32::max);
    assert!(hah > hoo * 1.25, "hoo {hoo} vs hah {hah}");
    // "Nyah-nyah!": the second syllable is lower (the playground third).
    let v = voiced(&contour(&dry(Sfx::BarkTaunt, 0)));
    let one = v
        .iter()
        .filter(|(t, _)| *t < 0.15)
        .map(|(_, f)| *f)
        .sum::<f32>()
        / v.iter().filter(|(t, _)| *t < 0.15).count().max(1) as f32;
    let two = v
        .iter()
        .filter(|(t, _)| *t > 0.26)
        .map(|(_, f)| *f)
        .sum::<f32>()
        / v.iter().filter(|(t, _)| *t > 0.26).count().max(1) as f32;
    assert!(two < one * 0.92, "nyah {one} then nyah {two}");
}

#[test]
fn barks_sit_under_the_guns_and_over_the_steps() {
    for sfx in [
        Sfx::BarkHup,
        Sfx::BarkTaunt,
        Sfx::BarkYelp,
        Sfx::BarkHooHah,
        Sfx::BarkWhaaa,
    ] {
        assert!(
            sfx.mix_db() < Sfx::RifleShot.mix_db(),
            "{sfx:?} under the rifle"
        );
        assert!(
            sfx.mix_db() < Sfx::WandWarning.mix_db() - 5.0,
            "{sfx:?} well under the warning"
        );
        assert!(
            sfx.mix_db() > Sfx::KnightStepGrass.mix_db(),
            "{sfx:?} over the steps"
        );
        assert!(sfx.priority() < Sfx::WandWarning.priority());
    }
    // Steps give way first.
    assert!(Sfx::KnightStepGrass.priority() < Sfx::Footstep.priority());
}

// ---------------------------------------------------------------------------
// The gate: rate limits, the voice cap, the warning
// ---------------------------------------------------------------------------

fn e(i: u32) -> Entity {
    Entity::from_raw_u32(i).unwrap()
}

#[test]
fn barks_are_rate_limited_per_knight_and_globally() {
    let mut gate = BarkGate::default();
    let never = f64::MIN;
    assert!(gate.try_bark(10.0, e(1), never, Bark::Yelp, 0.0).is_some());
    // Another knight, too soon after any bark.
    assert!(
        gate.try_bark(10.0 + GLOBAL_GAP * 0.5, e(2), never, Bark::Yelp, 0.0)
            .is_none()
    );
    assert!(
        gate.try_bark(10.0 + GLOBAL_GAP, e(2), never, Bark::Yelp, 0.0)
            .is_some()
    );
    // The same knight, too soon after his last.
    assert!(gate.try_bark(10.5, e(1), 10.0, Bark::Yelp, 0.0).is_none());
    assert!(
        gate.try_bark(10.0 + KNIGHT_GAP, e(1), 10.0, Bark::Yelp, 0.0)
            .is_some()
    );
    // Taunts: one per TAUNT_GAP across all knights.
    let mut gate = BarkGate::default();
    assert!(gate.try_bark(0.0, e(1), never, Bark::Taunt, 0.0).is_some());
    assert!(
        gate.try_bark(TAUNT_GAP * 0.5, e(2), never, Bark::Taunt, 0.0)
            .is_none()
    );
    assert!(
        gate.try_bark(TAUNT_GAP, e(3), never, Bark::Taunt, 0.0)
            .is_some()
    );
}

#[test]
fn at_most_three_barks_sound_at_once() {
    let mut gate = BarkGate::default();
    let mut t = 0.0;
    let mut booked = 0;
    for k in 0..10 {
        if gate.try_bark(t, e(k), f64::MIN, Bark::Whaaa, 0.0).is_some() {
            booked += 1;
        }
        assert!(gate.sounding(t) <= MAX_BARKS);
        t += GLOBAL_GAP;
    }
    assert_eq!(booked, MAX_BARKS, "the cap holds while they ring");
    // Once they have rung out, more may start.
    let later = t + Bark::Whaaa.seconds();
    assert!(
        gate.try_bark(later, e(20), f64::MIN, Bark::Yelp, 0.0)
            .is_some()
    );
}

#[test]
fn a_warning_cuts_every_bark_and_holds_them_off() {
    let mut gate = BarkGate::default();
    let never = f64::MIN;
    gate.try_bark(0.0, e(1), never, Bark::Taunt, 0.0).unwrap();
    gate.try_bark(0.2, e(2), never, Bark::Whaaa, 0.1).unwrap();
    let mut cut = Vec::new();
    gate.warning(0.3, |v| cut.push(v));
    assert_eq!(cut.len(), 2, "both barks are cut");
    for v in &cut {
        assert!(v.end <= 0.3 + 1e-9, "{v:?} ends by the warning");
    }
    assert_eq!(gate.sounding(0.3), 0);
    for dt in [0.0, 0.1, WARNING_GUARD - 0.01] {
        assert!(
            gate.try_bark(0.3 + dt, e(3), never, Bark::Yelp, 0.0)
                .is_none(),
            "no bark {dt} s into the warning"
        );
    }
    assert!(
        gate.try_bark(0.3 + WARNING_GUARD, e(3), never, Bark::Yelp, 0.0)
            .is_some()
    );
    // The guard outlasts the warning cue itself.
    let warning = Sfx::WandWarning.spec().max_seconds as f64;
    assert!(WARNING_GUARD > warning);
}

#[test]
fn a_knight_taunts_on_at_most_one_wind_up_in_three() {
    for k in 0..40 {
        let taunts = (1..=300).filter(|&n| taunt_turn(e(k), n)).count();
        assert!(taunts <= 300 / TAUNT_EVERY as usize, "knight {k}: {taunts}");
        assert!(!taunt_turn(e(k), 1) && !taunt_turn(e(k), 2));
    }
    let any = (0..40)
        .map(|k| (1..=30).filter(|&n| taunt_turn(e(k), n)).count())
        .sum::<usize>();
    assert!(any > 100, "they do taunt: {any}");
}

#[test]
fn each_knight_has_his_own_pitch_and_yelps_vary() {
    let pitches: Vec<f32> = (0..30).map(|k| personal_pitch(e(k))).collect();
    for p in &pitches {
        assert!((0.9..=1.12).contains(p), "{p}");
    }
    let spread = pitches.iter().fold(f32::MIN, |a, b| a.max(*b))
        - pitches.iter().fold(f32::MAX, |a, b| a.min(*b));
    assert!(spread > 0.15, "knights sound different: {spread}");
    assert_eq!(personal_pitch(e(5)), personal_pitch(e(5)));
    let j: Vec<f32> = (0..500).map(yelp_jitter).collect();
    assert!(j.iter().all(|x| (0.94..=1.06).contains(x)));
    assert!(j.iter().any(|x| *x < 0.97) && j.iter().any(|x| *x > 1.03));
}

// ---------------------------------------------------------------------------
// Spatial: the pan, the steps
// ---------------------------------------------------------------------------

#[test]
fn spatial_sounds_pan_to_the_side_they_come_from() {
    // A camera at the origin looking down -Z: +X is its right.
    let camera = GlobalTransform::from(Transform::IDENTITY);
    let ears = listener();
    for d in [2.0f32, 6.0, 15.0, 30.0] {
        let (l, r) = speaker_gains(&camera, &ears, 0.08, Vec3::new(d, 0.0, -1.0));
        assert!(r > l * 1.5, "right side at {d} m: L {l} R {r}");
        let (l, r) = speaker_gains(&camera, &ears, 0.08, Vec3::new(-d, 0.0, -1.0));
        assert!(l > r * 1.5, "left side at {d} m: L {l} R {r}");
        let (l, r) = speaker_gains(&camera, &ears, 0.08, Vec3::new(0.0, 0.0, -d));
        assert!((l - r).abs() < 1e-4, "ahead is centred");
    }
    // Turned around, the sides swap with the view.
    let back = GlobalTransform::from(Transform::from_rotation(Quat::from_rotation_y(
        std::f32::consts::PI,
    )));
    let (l, r) = speaker_gains(&back, &ears, 0.08, Vec3::new(5.0, 0.0, 0.0));
    assert!(l > r, "+X is now on the left");
    // Far away it is quieter (full level within 1 / scale = 12.5 m).
    let near = speaker_gains(&camera, &ears, 0.08, Vec3::new(4.0, 0.0, 0.0)).1;
    let far = speaker_gains(&camera, &ears, 0.08, Vec3::new(40.0, 0.0, 0.0)).1;
    assert!(far < near * 0.2, "{near} vs {far}");
    // Bevy's own listener, through rodio's formula, pans the wrong way: the
    // bug the swapped ears fix.
    let bevy = bevy::audio::SpatialListener::new(0.25);
    let (l, r) = speaker_gains(&camera, &bevy, 0.08, Vec3::new(6.0, 0.0, 0.0));
    assert!(l > r, "rodio's reversed term: L {l} R {r}");
}

#[test]
fn a_knight_steps_on_grass_wood_or_brick() {
    assert_eq!(step_for(None), Sfx::KnightStepGrass);
    assert_eq!(step_for(Some(PieceKind::Floor)), Sfx::KnightStepWood);
    assert_eq!(step_for(Some(PieceKind::Ramp)), Sfx::KnightStepWood);
    assert_eq!(step_for(Some(PieceKind::Wall)), Sfx::KnightStepBrick);
}

/// A waves run with the knights' voices tracked, for `seconds`.
fn voiced_run(seed: u64, seconds: f32) -> (Sim, BarkLog) {
    let mut sim = Sim::waves(seed);
    KnightVoiceTracking::install(&mut sim.app);
    let mut feet: Vec<(f64, Entity, Vec3)> = Vec::new();
    let ticks = (seconds / TICK_SECONDS) as u32;
    let mut log = BarkLog::default();
    for _ in 0..ticks {
        sim.tick();
        let now = sim.world().resource::<Time<Real>>().elapsed_secs_f64();
        let mut q = sim
            .world_mut()
            .query_filtered::<(Entity, &Transform), With<Grunt>>();
        let w = sim.world();
        feet.extend(q.iter(w).map(|(e, t)| (now, e, t.translation)));
        let mut l = sim.world_mut().resource_mut::<BarkLog>();
        log.barks.append(&mut l.barks);
        log.cut.append(&mut l.cut);
        log.warnings.append(&mut l.warnings);
        log.steps.append(&mut l.steps);
    }
    // Every step is heard from its knight's feet on that frame.
    for (t, k, at, _) in &log.steps {
        let there = feet
            .iter()
            .find(|(ft, fk, _)| ft == t && fk == k)
            .map(|(_, _, p)| *p)
            .expect("the knight exists");
        assert!(at.distance(there) < 1e-3, "step at {at} vs feet {there}");
    }
    (sim, log)
}

#[test]
fn knights_step_where_they_are_and_hup_as_they_land() {
    let (sim, log) = voiced_run(3, 25.0);
    let run = sim.world().resource::<Run>();
    println!(
        "wave {}: {} steps, {} barks ({:?}), {} warnings, {} cut",
        run.wave,
        log.steps.len(),
        log.barks.len(),
        log.barks.iter().map(|b| b.bark).collect::<Vec<_>>(),
        log.warnings.len(),
        log.cut.len()
    );
    assert!(log.steps.len() > 20, "knights are heard running");
    assert!(
        log.barks.iter().any(|b| b.bark == Bark::Hup),
        "a landing hup"
    );
    // Steps: at most three knights per footfall frame, grass on the island.
    let mut per_frame = std::collections::HashMap::<u64, usize>::new();
    for (t, ..) in &log.steps {
        *per_frame.entry(t.to_bits()).or_default() += 1;
    }
    assert!(per_frame.values().all(|n| *n <= 3));
    assert!(log.steps.iter().all(|s| matches!(
        s.3,
        Sfx::KnightStepGrass | Sfx::KnightStepWood | Sfx::KnightStepBrick
    )));
    // Every bark keeps the gate's limits.
    for (i, a) in log.barks.iter().enumerate() {
        for b in &log.barks[i + 1..] {
            assert!(b.start - a.start >= GLOBAL_GAP - 1e-6 - 0.2, "{a:?} {b:?}");
        }
    }
}
