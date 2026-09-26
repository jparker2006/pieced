//! A tiny deterministic synthesizer: noise, oscillators, filters, envelopes, a
//! mastering stage and a 16-bit PCM WAV encoder. Every sound in the game is built
//! from these at startup, so there are no audio asset files and no third-party
//! sounds.

use std::f32::consts::{PI, TAU};

pub const SAMPLE_RATE: u32 = 44_100;
const SR: f32 = SAMPLE_RATE as f32;

/// Loudest sample any cue may reach: −1 dBFS.
pub const PEAK_CEILING: f32 = 0.891_250_9;
/// Where the mastering limiter starts to bend peaks toward [`PEAK_CEILING`]
/// (−3 dBFS). Below it the signal passes untouched.
pub const LIMITER_KNEE: f32 = 0.708;
/// Every cue is high-passed here (Hz) when mastered: laptop speakers can't play
/// below it, and sub-bass would only eat headroom the audible range could use.
pub const MASTER_HIGH_PASS: f32 = 100.0;
/// Window for short-term loudness (s): long enough to span a transient's body,
/// short enough that a long quiet tail doesn't dilute a short, loud cue.
pub const LOUDNESS_WINDOW: f32 = 0.05;

/// Number of samples covering `seconds`.
pub fn samples_for(seconds: f32) -> usize {
    (seconds.max(0.0) * SR).round() as usize
}

/// Seconds at sample index `i`.
#[inline]
pub fn time_of(i: usize) -> f32 {
    i as f32 / SR
}

/// Equal-tempered pitch of a MIDI note number (69 = A4 = 440 Hz).
pub fn note(midi: f32) -> f32 {
    440.0 * 2f32.powf((midi - 69.0) / 12.0)
}

pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub fn gain_to_db(gain: f32) -> f32 {
    20.0 * gain.max(1e-9).log10()
}

/// Loudest absolute sample.
pub fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// Short-term loudness: the RMS of the loudest [`LOUDNESS_WINDOW`] stretch, in dBFS
/// (a full-scale square wave reads 0 dBFS, a full-scale sine −3 dBFS). Cues shorter
/// than the window are measured whole.
pub fn short_term_rms_db(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return gain_to_db(0.0);
    }
    let w = samples_for(LOUDNESS_WINDOW).clamp(1, samples.len());
    // Running sum of squares in f64 so long buffers don't drift.
    let mut sum: f64 = samples[..w].iter().map(|s| (*s as f64).powi(2)).sum();
    let mut best = sum;
    for i in w..samples.len() {
        sum += (samples[i] as f64).powi(2) - (samples[i - w] as f64).powi(2);
        best = best.max(sum);
    }
    gain_to_db((best.max(0.0) / w as f64).sqrt() as f32)
}

/// The mastering limiter's transfer curve: identity below [`LIMITER_KNEE`], then a
/// tanh shoulder that approaches (and never reaches) [`PEAK_CEILING`].
#[inline]
pub fn soft_limit(x: f32) -> f32 {
    let a = x.abs();
    if a <= LIMITER_KNEE {
        x
    } else {
        let room = PEAK_CEILING - LIMITER_KNEE;
        x.signum() * (LIMITER_KNEE + room * ((a - LIMITER_KNEE) / room).tanh())
    }
}

/// Deterministic white noise (xorshift64*), seeded per sound.
#[derive(Debug, Clone)]
pub struct Noise(u64);

impl Noise {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let v = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (v >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform in [-1, 1).
    pub fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

/// Topology-preserving state-variable filter (Simper). Stable while the cutoff
/// moves every sample, which the sweeps below rely on. The prewarped coefficient
/// is cached, so a fixed cutoff costs no `tan` per sample.
#[derive(Debug, Clone, Default)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
    cutoff: f32,
    g: f32,
}

pub struct SvfOut {
    pub low: f32,
    pub band: f32,
    pub high: f32,
}

impl Svf {
    pub fn process(&mut self, x: f32, cutoff: f32, q: f32) -> SvfOut {
        let fc = cutoff.clamp(10.0, SR * 0.45);
        if fc != self.cutoff {
            self.cutoff = fc;
            self.g = (PI * fc / SR).tan();
        }
        let g = self.g;
        let k = 1.0 / q.max(0.05);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        SvfOut {
            low: v2,
            band: v1,
            high: x - k * v1 - v2,
        }
    }

    pub fn low(&mut self, x: f32, cutoff: f32) -> f32 {
        self.process(x, cutoff, std::f32::consts::FRAC_1_SQRT_2).low
    }

    pub fn high(&mut self, x: f32, cutoff: f32) -> f32 {
        self.process(x, cutoff, std::f32::consts::FRAC_1_SQRT_2)
            .high
    }

    pub fn band(&mut self, x: f32, cutoff: f32, q: f32) -> f32 {
        self.process(x, cutoff, q).band
    }
}

/// Phase-accumulating oscillator (for pitch sweeps).
#[derive(Debug, Clone, Default)]
pub struct Osc {
    phase: f32,
}

impl Osc {
    /// Starts at a phase in cycles (0..1).
    pub fn at(phase: f32) -> Self {
        Self {
            phase: phase.rem_euclid(1.0),
        }
    }

    #[inline]
    fn advance(&mut self, freq: f32) {
        self.phase = (self.phase + freq / SR).fract();
    }

    pub fn sine(&mut self, freq: f32) -> f32 {
        let out = (self.phase * TAU).sin();
        self.advance(freq);
        out
    }

    /// A sine pushed toward a square by `drive` (≥ 0.1), level-matched. High drive
    /// is buzzy and bright ("zap"); low drive is round.
    pub fn soft_square(&mut self, freq: f32, drive: f32) -> f32 {
        let d = drive.max(0.1);
        let out = (d * (self.phase * TAU).sin()).tanh() / d.tanh();
        self.advance(freq);
        out
    }

    /// Band-limited-ish saw built from a few harmonics (no aliasing at the
    /// frequencies used here).
    pub fn saw(&mut self, freq: f32, harmonics: u32) -> f32 {
        let p = self.phase * TAU;
        let mut out = 0.0;
        for h in 1..=harmonics.max(1) {
            out += (p * h as f32).sin() / h as f32;
        }
        self.advance(freq);
        out * 0.6
    }
}

/// Exponential decay with time constant `tau` seconds.
#[inline]
pub fn decay(t: f32, tau: f32) -> f32 {
    if t < 0.0 { 0.0 } else { (-t / tau).exp() }
}

/// Linear attack over `a` seconds.
#[inline]
pub fn attack(t: f32, a: f32) -> f32 {
    if t <= 0.0 {
        0.0
    } else if a <= 0.0 {
        1.0
    } else {
        (t / a).min(1.0)
    }
}

/// A linear attack over `a` seconds, then exponential decay with time constant
/// `tau`. (The decay holds at 1 during the attack: `decay` is 0 before time 0, so
/// the attack would otherwise be silent and then jump to full level.)
#[inline]
pub fn ad(t: f32, a: f32, tau: f32) -> f32 {
    attack(t, a) * decay((t - a).max(0.0), tau)
}

/// 1 until `end - release`, then a linear fade to 0 at `end`.
#[inline]
pub fn release(t: f32, end: f32, release: f32) -> f32 {
    if t >= end {
        0.0
    } else if release <= 0.0 {
        1.0
    } else {
        ((end - t) / release).min(1.0)
    }
}

/// A mono buffer being assembled from layers.
#[derive(Debug, Clone)]
pub struct Buffer {
    pub samples: Vec<f32>,
}

impl Buffer {
    pub fn new(seconds: f32) -> Self {
        Self {
            samples: vec![0.0; samples_for(seconds)],
        }
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Adds `f(t)` for every sample from `start` seconds for `length` seconds, with
    /// `t` measured from `start`.
    pub fn add(&mut self, start: f32, length: f32, gain: f32, mut f: impl FnMut(f32) -> f32) {
        let s0 = samples_for(start);
        let n = samples_for(length);
        let end = (s0 + n).min(self.samples.len());
        for i in s0..end {
            self.samples[i] += gain * f(time_of(i - s0));
        }
    }

    /// Mixes another buffer in at `gain` (from the start; extra length is cut).
    pub fn mix(&mut self, other: &Buffer, gain: f32) {
        for (s, o) in self.samples.iter_mut().zip(&other.samples) {
            *s += gain * o;
        }
    }

    /// Soft saturation for punch (tanh with drive, level-compensated).
    pub fn saturate(&mut self, drive: f32) {
        let norm = drive.tanh();
        for s in &mut self.samples {
            *s = (*s * drive).tanh() / norm;
        }
    }

    /// Low-passes at `cutoff` Hz (12 dB/octave).
    pub fn low_pass(&mut self, cutoff: f32) {
        let mut lp = Svf::default();
        for s in &mut self.samples {
            *s = lp.low(*s, cutoff);
        }
    }

    /// High-passes at `cutoff` Hz (12 dB/octave). Also removes DC, so tails settle
    /// on zero.
    pub fn high_pass(&mut self, cutoff: f32) {
        let mut hp = Svf::default();
        for s in &mut self.samples {
            *s = hp.high(*s, cutoff);
        }
    }

    /// Masters the cue to a short-term loudness of `rms_db` dBFS (see
    /// [`short_term_rms_db`]): high-passes at [`MASTER_HIGH_PASS`], sets the gain,
    /// bends any peak above [`LIMITER_KNEE`] under [`PEAK_CEILING`], makes up the
    /// loudness the limiter took, and fades the tail so the cue never ends in a
    /// click.
    pub fn master(mut self, rms_db: f32) -> Vec<f32> {
        self.high_pass(MASTER_HIGH_PASS);
        let dry = std::mem::take(&mut self.samples);
        let measured = short_term_rms_db(&dry);
        let mut gain = db_to_gain(rms_db - measured);
        let mut out = Vec::new();
        // The limiter only ever lowers loudness, so a few make-up passes converge.
        for _ in 0..4 {
            out = dry.iter().map(|s| soft_limit(s * gain)).collect();
            let short_by = rms_db - short_term_rms_db(&out);
            if short_by.abs() < 0.05 {
                break;
            }
            gain *= db_to_gain(short_by);
        }
        self.samples = out;
        self.fade_out();
        self.samples
    }

    fn fade_out(&mut self) {
        let fade = samples_for(0.006).min(self.samples.len());
        let n = self.samples.len();
        for j in 0..fade {
            self.samples[n - 1 - j] *= j as f32 / fade as f32;
        }
    }
}

/// Encodes mono samples in [-1, 1] as a 16-bit PCM WAV file.
pub fn encode_wav(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVE");
    b.extend_from_slice(b"fmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    b.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    b.extend_from_slice(&2u16.to_le_bytes()); // block align
    b.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        b.extend_from_slice(&v.to_le_bytes());
    }
    b
}

/// The header fields of a canonical PCM WAV file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavInfo {
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    /// Samples per channel.
    pub frames: u32,
}

impl WavInfo {
    /// Parses and validates a canonical 44-byte-header PCM WAV. `None` if any
    /// field is inconsistent with the data that follows.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let u32_at = |o: usize| Some(u32::from_le_bytes(bytes.get(o..o + 4)?.try_into().ok()?));
        let u16_at = |o: usize| Some(u16::from_le_bytes(bytes.get(o..o + 2)?.try_into().ok()?));
        if bytes.get(0..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
            return None;
        }
        if bytes.get(12..16)? != b"fmt " || u32_at(16)? != 16 || u16_at(20)? != 1 {
            return None;
        }
        let channels = u16_at(22)?;
        let sample_rate = u32_at(24)?;
        let byte_rate = u32_at(28)?;
        let block_align = u16_at(32)?;
        let bits = u16_at(34)?;
        let block = channels as u32 * bits as u32 / 8;
        if block == 0 || block_align as u32 != block || byte_rate != sample_rate * block {
            return None;
        }
        if bytes.get(36..40)? != b"data" {
            return None;
        }
        let data_len = u32_at(40)?;
        if u32_at(4)? != 36 + data_len || bytes.len() != 44 + data_len as usize {
            return None;
        }
        Some(Self {
            channels,
            sample_rate,
            bits_per_sample: bits,
            frames: data_len / block,
        })
    }

    pub fn seconds(&self) -> f32 {
        self.frames as f32 / self.sample_rate as f32
    }
}
