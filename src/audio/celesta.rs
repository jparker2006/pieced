//! The break loop (M4 chunk 3, D119): an original celesta waltz in A minor,
//! synthesized at load on the music thread.
//!
//! There is no openly licensed celesta cue, so the calm slot is composed here:
//! a Hogwarts-flavoured 3/4 ostinato (a rising-and-falling eighth-note figure
//! on the celesta) over a descending chromatic bass (A G♯ G F♯ F E), a soft
//! string pad, a harp on each downbeat, and a slow bell melody doubled by a
//! breathy flute, all in a small hall. It is original: the figure, harmony and
//! melody below are this file's, not any film theme's.
//!
//! **A minor** because the round-start sting that ends every break is in C
//! major (its relative major): as the loop fades under the sting, the pad's
//! A–C–E sits inside the fanfare's C–E–G instead of rubbing against it.
//!
//! The loop is exactly 16 bars (960 000 samples at 44.1 kHz, ≈ 21.8 s at 132
//! bpm). Everything ringing past the end, the reverb included, is folded back
//! onto the start, so the seam is sample-continuous: the loop is the steady
//! state of the piece playing forever.

use super::{
    loudness::integrated_lufs,
    reverb::{Fdn, Room},
    synth::{Noise, SAMPLE_RATE, Svf, db_to_gain, note, soft_limit},
};
use std::f32::consts::{FRAC_PI_2, TAU};

const SR: f32 = SAMPLE_RATE as f32;

/// Samples per eighth note: 132.3 bpm, so every note lands on a whole sample
/// and the loop's length is exact.
pub const EIGHTH: usize = 10_000;
/// Eighths per 3/4 bar.
pub const EIGHTHS_PER_BAR: usize = 6;
pub const BARS: usize = 16;
/// The loop's length in frames.
pub const FRAMES: usize = EIGHTH * EIGHTHS_PER_BAR * BARS;
/// Beats per minute (quarter notes).
pub fn bpm() -> f32 {
    60.0 * SR / (2.0 * EIGHTH as f32)
}

/// The loudness every music slot plays at before its mix (LUFS).
pub const TARGET_LUFS: f32 = -18.0;

/// One bar of the score: the bass (harp, downbeat), the pad's three notes and
/// the celesta's six-eighth figure (MIDI notes).
struct Bar {
    bass: f32,
    pad: [f32; 3],
    figure: [f32; 6],
}

const fn bar(bass: f32, pad: [f32; 3], figure: [f32; 6]) -> Bar {
    Bar { bass, pad, figure }
}

/// The 16 bars. Am, Am/G♯, Am/G, D/F♯, Fmaj7, E7, Am, E7♭9 |
/// Dm, Dm/C, Bø7, E7, Am, F7, E7, E7 (a rising turnaround into bar 1).
const SCORE: [Bar; BARS] = [
    bar(
        57.0,
        [60.0, 64.0, 69.0],
        [76.0, 81.0, 84.0, 88.0, 84.0, 81.0],
    ),
    bar(
        56.0,
        [60.0, 64.0, 69.0],
        [76.0, 80.0, 84.0, 88.0, 84.0, 80.0],
    ),
    bar(
        55.0,
        [60.0, 64.0, 69.0],
        [76.0, 79.0, 84.0, 88.0, 84.0, 79.0],
    ),
    bar(
        54.0,
        [62.0, 66.0, 69.0],
        [78.0, 81.0, 86.0, 90.0, 86.0, 81.0],
    ),
    bar(
        53.0,
        [60.0, 64.0, 69.0],
        [77.0, 81.0, 84.0, 88.0, 84.0, 81.0],
    ),
    bar(
        52.0,
        [59.0, 62.0, 68.0],
        [76.0, 80.0, 83.0, 86.0, 83.0, 80.0],
    ),
    bar(
        57.0,
        [60.0, 64.0, 69.0],
        [76.0, 81.0, 84.0, 88.0, 84.0, 81.0],
    ),
    bar(
        52.0,
        [59.0, 62.0, 68.0],
        [76.0, 80.0, 86.0, 89.0, 86.0, 80.0],
    ),
    bar(
        50.0,
        [62.0, 65.0, 69.0],
        [74.0, 77.0, 81.0, 86.0, 81.0, 77.0],
    ),
    bar(
        48.0,
        [62.0, 65.0, 69.0],
        [74.0, 77.0, 81.0, 84.0, 81.0, 77.0],
    ),
    bar(
        47.0,
        [62.0, 65.0, 69.0],
        [74.0, 77.0, 81.0, 83.0, 81.0, 77.0],
    ),
    bar(
        52.0,
        [62.0, 64.0, 68.0],
        [76.0, 80.0, 83.0, 86.0, 83.0, 80.0],
    ),
    bar(
        57.0,
        [60.0, 64.0, 69.0],
        [76.0, 81.0, 84.0, 88.0, 84.0, 81.0],
    ),
    bar(
        53.0,
        [60.0, 63.0, 69.0],
        [77.0, 81.0, 84.0, 87.0, 84.0, 81.0],
    ),
    bar(
        52.0,
        [59.0, 62.0, 68.0],
        [76.0, 80.0, 83.0, 86.0, 83.0, 80.0],
    ),
    bar(
        52.0,
        [59.0, 64.0, 68.0],
        [76.0, 80.0, 83.0, 86.0, 88.0, 92.0],
    ),
];

/// The bell melody: `(bar, beat, MIDI note, beats)`. It enters in bar 3, after
/// the figure has set the waltz going, and rests in bar 16 for the turnaround.
const MELODY: [(usize, usize, f32, usize); 26] = [
    (2, 0, 88.0, 3),
    (3, 0, 90.0, 2),
    (3, 2, 93.0, 1),
    (4, 0, 93.0, 2),
    (4, 2, 88.0, 1),
    (5, 0, 92.0, 3),
    (6, 0, 93.0, 2),
    (6, 2, 96.0, 1),
    (7, 0, 95.0, 2),
    (7, 2, 92.0, 1),
    (8, 0, 93.0, 3),
    (9, 0, 89.0, 2),
    (9, 2, 93.0, 1),
    (10, 0, 89.0, 2),
    (10, 2, 86.0, 1),
    (11, 0, 92.0, 3),
    (12, 0, 88.0, 1),
    (12, 1, 93.0, 1),
    (12, 2, 96.0, 1),
    (13, 0, 96.0, 2),
    (13, 2, 93.0, 1),
    (14, 0, 95.0, 2),
    (14, 2, 92.0, 1),
    // Bar 16's pickup: two soft bells answering the turnaround.
    (15, 1, 88.0, 1),
    (15, 2, 92.0, 1),
    // The figure's own top note doubled on bar 1's downbeat (wraps round).
    (0, 0, 88.0, 3),
];

/// The dynamics of the 16 bars: two 8-bar phrases, each swelling to its
/// middle and easing off, the second a little fuller, and the turnaround
/// settling back to where bar 1 begins.
const PHRASE: [f32; BARS] = [
    0.8, 0.82, 0.86, 0.9, 0.96, 1.0, 0.95, 0.88, 0.9, 0.96, 1.03, 1.1, 1.06, 1.0, 0.92, 0.84,
];

/// The notes the score uses, as pitch classes (0 = C): A natural minor plus
/// the leading tone G♯, the F♯ of the D/F♯ bar and the E♭ of the F7 bar.
pub fn score_pitch_classes() -> Vec<u8> {
    let mut pcs: Vec<u8> = SCORE
        .iter()
        .flat_map(|b| {
            std::iter::once(b.bass)
                .chain(b.pad)
                .chain(b.figure)
                .collect::<Vec<_>>()
        })
        .chain(MELODY.iter().map(|m| m.2))
        .map(|m| (m as i32).rem_euclid(12) as u8)
        .collect();
    pcs.sort_unstable();
    pcs.dedup();
    pcs
}

/// The celesta figure's note (MIDI) at eighth `i` of the loop (for tests).
pub fn figure_note(i: usize) -> f32 {
    let i = i % (EIGHTHS_PER_BAR * BARS);
    SCORE[i / EIGHTHS_PER_BAR].figure[i % EIGHTHS_PER_BAR]
}

/// A stereo buffer being assembled, with a mono send to the hall (it wraps
/// round: see the module docs).
struct Stereo {
    l: Vec<f32>,
    r: Vec<f32>,
    send: Vec<f32>,
    /// How much of the next voice goes to the hall.
    send_gain: f32,
}

impl Stereo {
    fn new(frames: usize) -> Self {
        Self {
            l: vec![0.0; frames],
            r: vec![0.0; frames],
            send: vec![0.0; frames],
            send_gain: 0.0,
        }
    }

    /// Adds `f(t)` from sample `start` for `len` samples at `pan` (−1 left,
    /// +1 right, equal power), and `send_gain` of it to the hall.
    fn add(&mut self, start: usize, len: usize, pan: f32, mut f: impl FnMut(f32) -> f32) {
        let angle = (pan.clamp(-1.0, 1.0) + 1.0) * 0.25 * std::f32::consts::PI;
        let (gl, gr) = (angle.cos(), angle.sin());
        let end = (start + len).min(self.l.len());
        for i in start..end {
            let s = f((i - start) as f32 / SR);
            self.l[i] += gl * s;
            self.r[i] += gr * s;
            self.send[i] += self.send_gain * s;
        }
    }
}

/// A quadrature oscillator (a rotating phasor): a sine per sample for two
/// multiplies, so hundreds of long bell notes stay cheap to render.
#[derive(Clone, Copy)]
struct Phasor {
    re: f32,
    im: f32,
    c: f32,
    s: f32,
}

impl Phasor {
    fn new(freq: f32) -> Self {
        let w = TAU * freq / SR;
        Self {
            re: 1.0,
            im: 0.0,
            c: w.cos(),
            s: w.sin(),
        }
    }

    #[inline]
    fn next(&mut self) -> f32 {
        let out = self.im;
        let re = self.re * self.c - self.im * self.s;
        self.im = self.re * self.s + self.im * self.c;
        self.re = re;
        out
    }
}

/// A celesta note: a hammered steel bar over a wooden resonator. The
/// fundamental dominates and rings (shorter as the pitch rises); a soft
/// octave and a faint fourth harmonic fade fast; a brief phase-modulated
/// "tink" at 3.5× gives the attack its glassy sparkle; a felt hammer thump
/// sits under it. `ring` scales how long it sings (the figure is played
/// lightly damped, the melody let ring).
fn celesta(out: &mut Stereo, start: usize, midi: f32, vel: f32, ring: f32, pan: f32, seed: u64) {
    let f = note(midi);
    let tau = ring * (0.55 * (440.0 / f).sqrt()).clamp(0.2, 0.9);
    let len = ((tau * 6.0).min(2.5) * SR) as usize;
    let (mut p1, mut p2, mut p4) = (Phasor::new(f), Phasor::new(2.0 * f), Phasor::new(4.0 * f));
    let mut phase = 0.0f32;
    let mut mod_phase = 0.0f32;
    let mut n = Noise::new(seed);
    let mut lp = Svf::default();
    out.add(start, len, pan, |t| {
        let attack = (t / 0.0015).min(1.0);
        let body = p1.next() * (-t / tau).exp()
            + 0.16 * p2.next() * (-t / (tau * 0.35)).exp()
            + 0.05 * p4.next() * (-t / 0.12).exp();
        let tink = if t < 0.08 {
            let index = 1.4 * (-t / 0.012).exp();
            phase += TAU * f / SR;
            mod_phase += TAU * 3.5 * f / SR;
            0.35 * (phase + index * mod_phase.sin()).sin() * (-t / 0.02).exp()
        } else {
            0.0
        };
        let thump = if t < 0.02 {
            0.12 * lp.low(n.signed(), 1800.0) * (-t / 0.004).exp()
        } else {
            0.0
        };
        vel * attack * (body + tink + thump)
    });
}

/// A breathy flute: a sine with a touch of second harmonic, a soft 70 ms
/// swell, delayed vibrato and a whisper of band-passed breath.
fn flute(out: &mut Stereo, start: usize, midi: f32, len: usize, vel: f32, pan: f32, seed: u64) {
    let f = note(midi);
    let secs = len as f32 / SR;
    let mut phase = 0.0f32;
    let mut vib = 0.0f32;
    let mut n = Noise::new(seed);
    let mut bp = Svf::default();
    out.add(start, len + (0.25 * SR) as usize, pan, |t| {
        let swell = (t / 0.07).min(1.0);
        let release = if t > secs {
            (-(t - secs) / 0.06).exp()
        } else {
            1.0
        };
        let depth = 0.004 * ((t - 0.25) / 0.3).clamp(0.0, 1.0);
        vib += TAU * 5.2 / SR;
        phase += TAU * f * (1.0 + depth * vib.sin()) / SR;
        let tone = phase.sin() + 0.12 * (2.0 * phase).sin();
        let breath = 0.05 * bp.band(n.signed(), 2.2 * f, 1.5);
        vel * swell * release * (tone + breath)
    });
}

/// A harp pluck (Karplus–Strong): a noise-excited delay line whose averaging
/// feedback darkens and dies like a plucked gut string.
fn harp(out: &mut Stereo, start: usize, midi: f32, vel: f32, seed: u64) {
    let f = note(midi);
    let period = SR / f;
    let n0 = period.floor() as usize;
    let frac = period - n0 as f32;
    let len = (1.6 * SR) as usize;
    let mut line = vec![0.0f32; n0 + 2];
    let mut noise = Noise::new(seed);
    let mut lp = Svf::default();
    for v in &mut line {
        *v = lp.low(noise.signed(), 2.5 * f.max(200.0));
    }
    let mut pos = 0usize;
    let size = line.len();
    let mut prev = 0.0f32;
    out.add(start, len, 0.0, |t| {
        // Fractional delay by linear interpolation keeps the pitch true.
        let a = line[(pos + size - n0) % size];
        let b = line[(pos + size - n0 - 1) % size];
        let y = a + (b - a) * frac;
        let fb = 0.996 * 0.5 * (y + prev);
        prev = y;
        line[pos] = fb;
        pos = (pos + 1) % size;
        vel * y * (t / 0.002).min(1.0)
    });
}

/// A band-limited sawtooth (polyBLEP), for the string pad.
#[derive(Clone, Copy, Default)]
struct Saw {
    phase: f32,
}

impl Saw {
    #[inline]
    fn next(&mut self, freq: f32) -> f32 {
        let dt = freq / SR;
        let t = self.phase;
        let mut v = 2.0 * t - 1.0;
        if t < dt {
            let x = t / dt;
            v -= x + x - x * x - 1.0;
        } else if t > 1.0 - dt {
            let x = (t - 1.0) / dt;
            v -= x * x + x + x + 1.0;
        }
        self.phase = (t + dt).fract();
        v
    }
}

/// The string pad: each chord note is three detuned saws (the section), dark
/// through a low-pass, swelling in over 0.4 s and releasing over 0.9 s into
/// the next chord.
fn strings(out: &mut Stereo, start: usize, midi: f32, len: usize, vel: f32, pan: f32) {
    let f = note(midi);
    let secs = len as f32 / SR;
    let mut saws = [Saw { phase: 0.0 }, Saw { phase: 0.33 }, Saw { phase: 0.71 }];
    let detune = [0.9965, 1.0, 1.0036];
    let mut lp = Svf::default();
    let mut lfo = 0.0f32;
    out.add(start, len + (1.2 * SR) as usize, pan, |t| {
        lfo += TAU * 0.23 / SR;
        let wobble = 1.0 + 0.0015 * lfo.sin();
        let s: f32 = saws
            .iter_mut()
            .zip(detune)
            .map(|(saw, d)| saw.next(f * d * wobble))
            .sum();
        let swell = ((t / 0.4).min(1.0) * FRAC_PI_2).sin();
        let release = if t > secs {
            (-(t - secs) / 0.35).exp()
        } else {
            1.0
        };
        vel * swell * release * lp.low(s, 1300.0)
    });
}

/// Renders the break loop: stereo interleaved PCM, exactly [`FRAMES`] long,
/// loudness-normalized to [`TARGET_LUFS`] and peaking under −1 dBFS.
pub fn render_break_loop() -> Vec<i16> {
    let beat = 2 * EIGHTH;
    let bar_len = EIGHTH * EIGHTHS_PER_BAR;
    // Render with 5 s of room for everything that rings past the end.
    let extra = (5.0 * SR) as usize;
    let total = FRAMES + extra;
    let mut out = Stereo::new(total);
    let mut seed = 1u64;
    let mut humanize = Noise::new(0xCE1E57A);
    for (b, bar) in SCORE.iter().enumerate() {
        let at = b * bar_len;
        let arc = PHRASE[b];
        // The figure, lightly accented on the downbeat and humanized by a few
        // milliseconds, as a player would.
        out.send_gain = 0.9;
        for (k, &m) in bar.figure.iter().enumerate() {
            let vel = match k {
                0 => 0.30,
                3 => 0.24,
                _ => 0.20,
            } * humanize.range(0.9, 1.05);
            let jitter = (humanize.range(-0.004, 0.004) * SR) as isize;
            let start = (at + k * EIGHTH) as isize + if k == 0 { 0 } else { jitter };
            seed += 1;
            celesta(
                &mut out,
                start.max(0) as usize,
                m,
                vel * arc,
                0.85,
                -0.3,
                seed,
            );
        }
        // Harp on the downbeat, with its octave a beat later, softly.
        out.send_gain = 0.3;
        seed += 1;
        harp(&mut out, at, bar.bass, 0.55 * arc, seed);
        harp(&mut out, at + beat, bar.bass + 12.0, 0.2 * arc, seed + 7);
        // The pad.
        out.send_gain = 0.6;
        for (k, &m) in bar.pad.iter().enumerate() {
            let pan = [-0.5, 0.0, 0.5][k];
            strings(&mut out, at, m, bar_len, 0.07 * arc, pan);
        }
        // The cellos hold the bass an octave down, under the harp.
        strings(&mut out, at, bar.bass - 12.0, bar_len, 0.06 * arc, 0.0);
    }
    for &(b, beat_i, m, beats) in &MELODY {
        let start = b * bar_len + beat_i * beat;
        let arc = PHRASE[b];
        seed += 1;
        out.send_gain = 1.4;
        celesta(&mut out, start, m, 0.34 * arc, 1.8, 0.25, seed);
        out.send_gain = 0.9;
        flute(
            &mut out,
            start,
            m - 12.0,
            beats * beat - EIGHTH / 2,
            0.09 * arc,
            0.4,
            seed + 3,
        );
    }
    // A small hall on the send. It runs over the loop twice, keeping the second
    // pass: the hall is then already ringing from the loop's end as its first
    // bar plays, exactly as it will when it comes round.
    let hall = Room {
        rt60: 2.3,
        size: 1.7,
        damping_hz: 4200.0,
        wet_db: 0.0,
        tail: 0.0,
        predelay: 0.0,
    };
    let mut fdn = Fdn::new(&hall);
    let wet_gain = db_to_gain(-17.0);
    let mut folded_send = vec![0.0f32; FRAMES];
    for (i, s) in out.send.iter().enumerate() {
        folded_send[i % FRAMES] += s;
    }
    let mut wet_l = vec![0.0f32; FRAMES];
    let mut wet_r = vec![0.0f32; FRAMES];
    for pass in 0..2 {
        for (i, &x) in folded_send.iter().enumerate() {
            let (l, r) = fdn.process(x);
            if pass == 1 {
                wet_l[i] = l;
                wet_r[i] = r;
            }
        }
    }
    // Fold everything past the end onto the start.
    let mut l = vec![0.0f32; FRAMES];
    let mut r = vec![0.0f32; FRAMES];
    for i in 0..total {
        l[i % FRAMES] += out.l[i];
        r[i % FRAMES] += out.r[i];
    }
    for i in 0..FRAMES {
        l[i] += wet_gain * wet_l[i];
        r[i] += wet_gain * wet_r[i];
    }
    master(&mut l, &mut r);
    l.iter()
        .zip(&r)
        .flat_map(|(a, b)| [to_i16(*a), to_i16(*b)])
        .collect()
}

/// A DC-blocking high-pass (the loop is circular, so it runs two passes and
/// keeps the second), the loudness target, and the soft limiter.
fn master(l: &mut [f32], r: &mut [f32]) {
    for ch in [&mut *l, &mut *r] {
        let mut hp = Svf::default();
        for pass in 0..2 {
            for s in ch.iter_mut() {
                let y = hp.high(*s, 70.0);
                if pass == 1 {
                    *s = y;
                }
            }
        }
    }
    let interleaved: Vec<f32> = l.iter().zip(r.iter()).flat_map(|(a, b)| [*a, *b]).collect();
    let gain = db_to_gain(TARGET_LUFS - integrated_lufs(&interleaved, 2, SAMPLE_RATE));
    for s in l.iter_mut().chain(r.iter_mut()) {
        *s = soft_limit(*s * gain);
    }
}

#[inline]
pub(crate) fn to_i16(s: f32) -> i16 {
    (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
}
