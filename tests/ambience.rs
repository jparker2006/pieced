//! The ambient soundscape (M4 chunk 6, D123): the three looping beds are
//! seamless, at their loudness and rendered off the main thread; the mix
//! follows the wind, swells the drone in a fight, ducks under an off-screen
//! warning and is silent muted.

use pieced::{
    audio::{
        AudioTuning,
        ambience::{
            AmbienceDirector, AmbienceInput, AmbienceLoader, BED_LUFS, Bed, WARNING_DUCK,
            tension_for,
        },
        loudness::integrated_lufs,
        music::Screen,
        synth::SAMPLE_RATE,
    },
    look::wind::{gust, gust_at},
};

fn to_f32(pcm: &[i16]) -> Vec<f32> {
    pcm.iter().map(|s| *s as f32 / 32_768.0).collect()
}

#[test]
fn every_bed_is_a_seamless_loop_at_its_loudness() {
    for bed in Bed::ALL {
        let start = std::time::Instant::now();
        let pcm = bed.render();
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(pcm.len(), bed.frames() * 2, "{bed:?} is exactly its loop");
        let x = to_f32(&pcm);
        let lufs = integrated_lufs(&x, 2, SAMPLE_RATE);
        assert!((lufs - BED_LUFS).abs() < 1.0, "{bed:?}: {lufs:.1} LUFS");
        assert!(
            x.iter().all(|s| s.abs() < 0.9),
            "{bed:?} stays under the ceiling"
        );
        // The seam: stepping from the last frame to the first is no bigger a
        // jump than the loop's own sample-to-sample steps, and the level
        // carries across it.
        for ch in 0..2 {
            let s: Vec<f32> = x.iter().skip(ch).step_by(2).copied().collect();
            let mut steps: Vec<f32> = s.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
            steps.sort_by(f32::total_cmp);
            let p999 = steps[steps.len() * 999 / 1000];
            let seam = (s[0] - s[s.len() - 1]).abs();
            assert!(seam <= p999, "{bed:?} ch{ch}: seam {seam} vs p99.9 {p999}");
            let n = (0.05 * SAMPLE_RATE as f32) as usize;
            let rms = |w: &[f32]| (w.iter().map(|v| v * v).sum::<f32>() / w.len() as f32).sqrt();
            let (end, head) = (rms(&s[s.len() - n..]), rms(&s[..n]));
            assert!(
                (20.0 * (end / head).log10()).abs() < 6.0,
                "{bed:?} ch{ch}: {end} then {head}"
            );
        }
        println!(
            "{bed:?}: {:.1} s, {lufs:.1} LUFS, rendered in {ms:.0} ms",
            bed.seconds()
        );
    }
}

#[test]
fn the_beds_render_on_their_own_thread() {
    let loader = AmbienceLoader::spawn().expect("thread starts");
    assert_eq!(loader.thread_name().as_deref(), Some("ambience-bank"));
}

fn playing(gust: f32) -> AmbienceInput {
    AmbienceInput {
        screen: Screen::Playing,
        fighting: false,
        alive: 0,
        gust,
        warning: false,
        effects: AudioTuning::default().effects_gain(),
    }
}

fn fight(alive: u32) -> AmbienceInput {
    AmbienceInput {
        fighting: true,
        alive,
        ..playing(0.6)
    }
}

fn settle(d: &mut AmbienceDirector, input: &AmbienceInput, seconds: f32) -> [f32; 3] {
    let mut g = [0.0; 3];
    for _ in 0..(seconds * 60.0) as u32 {
        g = d.step(1.0 / 60.0, input);
    }
    g
}

#[test]
fn the_wind_bed_gusts_with_the_wind_field() {
    let mut d = AmbienceDirector::default();
    let calm = settle(&mut d, &playing(0.0), 3.0)[0];
    let gusty = settle(&mut d, &playing(1.0), 3.0)[0];
    assert!(gusty > calm * 2.5, "{calm} -> {gusty}");
    // The bed reads the sway's own field: at the centre it is the gust.
    assert_eq!(gust_at(40.0, bevy::math::Vec2::ZERO), gust(40.0));
}

#[test]
fn the_drone_swells_in_a_fight_and_fades_in_the_break() {
    let mut d = AmbienceDirector::default();
    let calm = settle(&mut d, &playing(0.5), 3.0)[2];
    assert_eq!(calm, 0.0, "no drone outside a fight");
    let few = settle(&mut d, &fight(1), 6.0)[2];
    let many = settle(&mut d, &fight(8), 6.0)[2];
    assert!(few > 0.0 && many > few * 1.3, "{few} -> {many}");
    assert!(tension_for(true, 20) <= 1.0);
    let after = settle(&mut d, &playing(0.5), 8.0)[2];
    assert!(after < many * 0.05, "fades in the break: {after}");
}

#[test]
fn every_bed_ducks_under_an_off_screen_warning() {
    let mut d = AmbienceDirector::default();
    let before = settle(&mut d, &fight(6), 6.0);
    let warned = AmbienceInput {
        warning: true,
        ..fight(6)
    };
    // Two frames in, every bed is well down; held, it sits at the duck.
    let mut now = [0.0; 3];
    for _ in 0..2 {
        now = d.step(1.0 / 60.0, &warned);
    }
    for i in 0..3 {
        assert!(
            now[i] <= before[i] * (WARNING_DUCK + 0.45),
            "bed {i}: {before:?} -> {now:?}"
        );
    }
    let held = settle(&mut d, &warned, 0.5);
    for i in 0..3 {
        assert!(held[i] <= before[i] * (WARNING_DUCK + 0.02), "bed {i} held");
    }
}

#[test]
fn the_ambience_is_silent_muted_at_zero_effects_and_while_loading() {
    let tuning = AudioTuning {
        muted: true,
        ..AudioTuning::default()
    };
    for effects in [tuning.effects_gain(), 0.0] {
        let mut d = AmbienceDirector::default();
        let g = settle(
            &mut d,
            &AmbienceInput {
                effects,
                ..fight(6)
            },
            5.0,
        );
        assert_eq!(g, [0.0; 3]);
    }
    let mut d = AmbienceDirector::default();
    let g = settle(
        &mut d,
        &AmbienceInput {
            screen: Screen::Boot,
            ..fight(6)
        },
        5.0,
    );
    assert!(g.iter().all(|v| *v < 1e-6), "{g:?}");
}
