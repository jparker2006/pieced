//! A generated room reverb (M4 chunk 3), baked into samples at load.
//!
//! An 8-line feedback delay network: two allpass diffusers smear the input,
//! eight delay lines of mutually prime lengths feed back through a Householder
//! matrix (lossless mixing), and a one-pole low-pass in every line makes the
//! highs die first, like a real room. Each line's feedback gain sets the decay
//! so the tail falls 60 dB in [`Room::rt60`] seconds.
//!
//! It only ever runs when the sound bank and the break loop are rendered, on
//! their background threads: at play time a reverbed cue costs exactly what a
//! dry one did.

use super::synth::{SAMPLE_RATE, db_to_gain, short_term_rms_db};

const SR: f32 = SAMPLE_RATE as f32;
const LINES: usize = 8;
/// Delay lengths (samples at 44.1 kHz) for a room of size 1: 23–53 ms,
/// mutually prime so the echoes never pile up on one period.
const BASE_DELAYS: [usize; LINES] = [1031, 1327, 1523, 1801, 1949, 2053, 2203, 2341];
/// The input diffusers' lengths (samples) and gain.
const DIFFUSERS: [usize; 2] = [142, 379];
const DIFFUSION: f32 = 0.6;

/// A room: how long it rings, how big it sounds, how dark its tail is, and how
/// much of it a cue gets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Room {
    /// Seconds for the tail to fall 60 dB.
    pub rt60: f32,
    /// Scales the delay lines (1 = 23–53 ms, a mid-sized room).
    pub size: f32,
    /// Where the tail's highs roll off (Hz); lower is a darker, softer room.
    pub damping_hz: f32,
    /// The tail's loudness against the dry cue (short-term RMS, dB).
    pub wet_db: f32,
    /// Seconds of tail rendered past the dry cue.
    pub tail: f32,
    /// Seconds before the room answers (the first reflection): it keeps the
    /// tail from combing against the cue's own ring.
    pub predelay: f32,
}

impl Room {
    /// The player's own guns and handling: tight, so six casts a second never
    /// smear.
    pub const TIGHT: Room = Room {
        rt60: 0.35,
        size: 0.6,
        damping_hz: 5000.0,
        wet_db: -19.0,
        tail: 0.18,
        predelay: 0.006,
    };
    /// Hit confirmation: a short, bright room, well under the hit itself.
    pub const HIT: Room = Room {
        rt60: 0.4,
        size: 0.7,
        damping_hz: 6500.0,
        wet_db: -21.0,
        tail: 0.2,
        predelay: 0.008,
    };
    /// Building and the player's movement: a wooden room.
    pub const WOOD: Room = Room {
        rt60: 0.5,
        size: 0.8,
        damping_hz: 4000.0,
        wet_db: -18.0,
        tail: 0.25,
        predelay: 0.012,
    };
    /// Brick: dense and dry, so a wall's clunk stays short and dull (the
    /// planks ring; the bricks don't).
    pub const STONE: Room = Room {
        rt60: 0.35,
        size: 0.7,
        damping_hz: 2500.0,
        wet_db: -26.0,
        tail: 0.18,
        predelay: 0.008,
    };
    /// The world's sounds (the knights' casts, ships, armor, the void): open
    /// air off the island's stone, so they sit at a distance.
    pub const WORLD: Room = Room {
        rt60: 0.8,
        size: 1.2,
        damping_hz: 3500.0,
        wet_db: -13.0,
        tail: 0.35,
        predelay: 0.02,
    };
    /// The run's beats (jingles, fanfares, stings): a small hall.
    pub const HALL: Room = Room {
        rt60: 1.1,
        size: 1.4,
        damping_hz: 5000.0,
        wet_db: -14.0,
        tail: 0.45,
        predelay: 0.025,
    };
}

/// A one-pole low-pass.
#[derive(Debug, Clone, Copy, Default)]
struct OnePole {
    z: f32,
}

impl OnePole {
    #[inline]
    fn process(&mut self, x: f32, a: f32) -> f32 {
        self.z += a * (x - self.z);
        self.z
    }
}

/// A Schroeder allpass on a ring buffer.
struct Allpass {
    buf: Vec<f32>,
    i: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(1)],
            i: 0,
        }
    }

    #[inline]
    fn process(&mut self, x: f32, g: f32) -> f32 {
        let d = self.buf[self.i];
        let v = x + g * d;
        self.buf[self.i] = v;
        self.i = (self.i + 1) % self.buf.len();
        d - g * v
    }
}

/// The network's state for one room.
pub struct Fdn {
    lines: [Vec<f32>; LINES],
    pos: [usize; LINES],
    gains: [f32; LINES],
    damp: [OnePole; LINES],
    damp_a: f32,
    diffusers: [Allpass; 2],
}

impl Fdn {
    pub fn new(room: &Room) -> Self {
        let size = room.size.clamp(0.1, 4.0);
        let lens = BASE_DELAYS.map(|d| ((d as f32 * size) as usize).max(8));
        let rt60 = room.rt60.max(0.05);
        // Each pass through a line of `len` samples loses 60 dB · len / (rt60 · SR).
        let gains = lens.map(|len| db_to_gain(-60.0 * len as f32 / (rt60 * SR)));
        let damp_a = 1.0 - (-std::f32::consts::TAU * room.damping_hz.max(100.0) / SR).exp();
        Self {
            lines: lens.map(|len| vec![0.0; len]),
            pos: [0; LINES],
            gains,
            damp: [OnePole::default(); LINES],
            damp_a,
            diffusers: [
                Allpass::new((DIFFUSERS[0] as f32 * size) as usize),
                Allpass::new((DIFFUSERS[1] as f32 * size) as usize),
            ],
        }
    }

    /// One sample in, the tail out as a stereo pair (two decorrelated taps).
    #[inline]
    pub fn process(&mut self, x: f32) -> (f32, f32) {
        let x = self.diffusers[0].process(x, DIFFUSION);
        let x = self.diffusers[1].process(x, DIFFUSION);
        let mut outs = [0.0f32; LINES];
        let mut sum = 0.0;
        for (k, out) in outs.iter_mut().enumerate() {
            let line = &self.lines[k];
            let y = line[self.pos[k]];
            let y = self.damp[k].process(y, self.damp_a) * self.gains[k];
            *out = y;
            sum += y;
        }
        // Householder feedback: I - (2 / N) · 1 1ᵀ, energy-preserving.
        let reflect = sum * (2.0 / LINES as f32);
        let (mut left, mut right) = (0.0, 0.0);
        for (k, &y) in outs.iter().enumerate() {
            let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
            let line = &mut self.lines[k];
            line[self.pos[k]] = y - reflect + sign * x;
            self.pos[k] = (self.pos[k] + 1) % line.len();
            if k < LINES / 2 {
                left += sign * y;
            } else {
                right += sign * y;
            }
            if k % 4 < 2 {
                right += 0.5 * y;
            } else {
                left += 0.5 * y;
            }
        }
        (left, right)
    }
}

/// The room's tail for `input` (mono), `extra` samples longer than it.
pub fn tail_mono(input: &[f32], room: &Room, extra: usize) -> Vec<f32> {
    let mut fdn = Fdn::new(room);
    (0..input.len() + extra)
        .map(|i| {
            let (l, r) = fdn.process(input.get(i).copied().unwrap_or(0.0));
            0.5 * (l + r)
        })
        .collect()
}

/// Bakes the room into a mono cue: the dry cue with `room.tail` seconds of
/// silence added, plus its tail at `room.wet_db` under the dry cue's
/// short-term loudness. The result still needs mastering.
pub fn bake(dry: &[f32], room: &Room) -> Vec<f32> {
    let extra = super::synth::samples_for(room.tail);
    let pre = super::synth::samples_for(room.predelay);
    let wet = tail_mono(dry, room, extra);
    let dry_db = short_term_rms_db(dry);
    let wet_db = short_term_rms_db(&wet);
    let g = db_to_gain(dry_db + room.wet_db - wet_db);
    let mut out: Vec<f32> = dry.to_vec();
    out.resize(dry.len() + extra, 0.0);
    for (o, w) in out.iter_mut().skip(pre).zip(&wet) {
        *o += g * w;
    }
    out
}

/// Seconds for an impulse's tail to fall `drop_db` below its loudest 10 ms
/// (for tests: a room's measured decay against its `rt60`).
pub fn measured_decay(room: &Room, drop_db: f32) -> f32 {
    let mut impulse = vec![0.0f32; 1];
    impulse[0] = 1.0;
    let tail = tail_mono(&impulse, room, (room.rt60 * 2.0 * SR) as usize);
    let frame = (0.01 * SR) as usize;
    let env: Vec<f32> = tail
        .chunks(frame)
        .map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    let (top_i, top) = env
        .iter()
        .enumerate()
        .fold((0, 0.0f32), |a, (i, e)| if *e > a.1 { (i, *e) } else { a });
    let floor = top * db_to_gain(-drop_db);
    let end = (top_i..env.len())
        .find(|&i| env[i] < floor)
        .unwrap_or(env.len());
    (end - top_i) as f32 * 0.01
}
