//! The sound designs: magical, cartoony, punchy and short, to match the flat
//! TV-cartoon wizard look (brass-and-crystal guns, spell bolts, brick walls,
//! wooden planks, a goofy armored knight, a grassy island).
//!
//! Each function returns mono samples at
//! [`SAMPLE_RATE`](super::synth::SAMPLE_RATE), mastered by [`Buffer::master`] to
//! the cue's loudness target and never above −1 dBFS. All randomness is seeded,
//! so the bank is byte-identical on every launch. Everything pitched sits on the
//! C-major pentatonic scale, so cues that overlap (a cast, a hit, a chime) never
//! clash.
//!
//! ## Frequency plan
//!
//! Jake plays on MacBook speakers, which barely reproduce anything under about
//! 150 Hz, so every "low" layer starts in the 150–400 Hz range and relies on
//! harmonics for weight. The spectrum is shared out so hit confirmation always
//! cuts through the player's own casts:
//!
//! - casts: zap sweeps and shimmer between 200 Hz and 3 kHz;
//! - hits: a mid "bonk" plus sparkle between 4 and 9 kHz, where no cast has much
//!   energy;
//! - building: brick is dull and noisy (short, damped modes), planks are hollow
//!   and ringing (longer, tonal modes);
//! - movement: soft, short and low in the mix.
//!
//! ## Loudness
//!
//! Every cue has a [`CueSpec`]: a length budget and a loudness target. Loudness
//! is short-term RMS, the RMS of the loudest 50 ms
//! ([`short_term_rms_db`](super::synth::short_term_rms_db), dBFS, where a
//! full-scale sine reads −3). Every cue is mastered to a target between −12 and
//! −10 dBFS, so the bank is consistently loud before mixing. Within that band,
//! clicky cues (tinks, ticks) sit lower and dense ones (whistles, swishes) higher,
//! so peaks land just under the −1 dBFS ceiling without the limiter squashing an
//! attack. How loud each cue plays against the others (quiet footsteps, hits
//! above the guns) is the mix, set in [`Sfx::mix_db`](super::Sfx::mix_db).

use super::synth::{Buffer, Noise, Osc, Svf, ad, attack, decay, note, release};
use std::f32::consts::{PI, TAU};

/// A cue's length budget (s) and loudness target (short-term RMS, dBFS).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CueSpec {
    pub max_seconds: f32,
    pub rms_db: f32,
}

const fn spec(max_seconds: f32, rms_db: f32) -> CueSpec {
    CueSpec {
        max_seconds,
        rms_db,
    }
}

/// The band every loudness target sits in (dBFS short-term RMS).
pub const LOUDNESS_BAND: (f32, f32) = (-12.0, -10.0);

// Weapons.
pub const RIFLE_CAST: CueSpec = spec(0.35, -11.0);
pub const PUMP_CAST: CueSpec = spec(0.60, -10.0);
pub const PUMP_RACK: CueSpec = spec(0.36, -11.0);
pub const RIFLE_MAG_OUT: CueSpec = spec(0.32, -12.0);
pub const RIFLE_MAG_IN: CueSpec = spec(0.32, -12.0);
pub const PUMP_SHELL: CueSpec = spec(0.14, -12.0);
pub const WEAPON_SWITCH: CueSpec = spec(0.25, -11.0);
// Hits.
pub const BODY_HIT: CueSpec = spec(0.11, -11.0);
pub const HEADSHOT: CueSpec = spec(0.55, -11.0);
pub const SHIELD_HIT: CueSpec = spec(0.18, -12.0);
pub const SHIELD_BREAK: CueSpec = spec(0.85, -12.0);
pub const ELIMINATION: CueSpec = spec(0.70, -10.0);
// Building.
pub const BRICK_PLACE: CueSpec = spec(0.30, -10.0);
pub const PLANK_PLACE: CueSpec = spec(0.30, -10.0);
pub const BRICK_CRACK: CueSpec = spec(0.35, -12.0);
pub const PLANK_CRACK: CueSpec = spec(0.40, -12.0);
pub const BRICK_BREAK: CueSpec = spec(0.85, -10.0);
pub const PLANK_BREAK: CueSpec = spec(0.80, -11.0);
pub const REJECTED: CueSpec = spec(0.30, -10.0);
// Movement.
pub const FOOTSTEP: CueSpec = spec(0.18, -12.0);
pub const JUMP: CueSpec = spec(0.24, -11.0);
pub const LAND: CueSpec = spec(0.28, -10.0);
pub const SLIDE: CueSpec = spec(0.60, -11.0);

/// Round-robin takes for the cues that repeat fastest (the rifle fires six times a
/// second; footsteps never stop), so repeats never sound machine-gunned.
pub const RIFLE_VARIANTS: u32 = 3;
pub const FOOTSTEP_VARIANTS: u32 = 3;

// ---------------------------------------------------------------------------
// Building blocks
// ---------------------------------------------------------------------------

/// Partials above this are skipped: inaudible on laptop speakers, and near
/// Nyquist they would alias.
const TOP_PARTIAL_HZ: f32 = 16_000.0;

/// A short noise burst through a band-pass, for clicks and clacks.
fn click(buf: &mut Buffer, start: f32, center: f32, q: f32, tau: f32, gain: f32, seed: u64) {
    let mut n = Noise::new(seed);
    let mut f = Svf::default();
    buf.add(start, tau * 8.0, gain, |t| {
        f.band(n.signed(), center, q) * decay(t, tau)
    });
}

/// A high-passed noise snap: the very front of an impact.
fn snap(buf: &mut Buffer, start: f32, cutoff: f32, tau: f32, gain: f32, seed: u64) {
    let mut n = Noise::new(seed);
    let mut f = Svf::default();
    buf.add(start, tau * 8.0, gain, |t| {
        f.high(n.signed(), cutoff) * decay(t, tau)
    });
}

/// Decaying sine partials (pings, bells, wood and stone modes), as
/// `(frequency, amplitude, decay time)`. Each rings for 7 decay times (−61 dB).
fn partials(buf: &mut Buffer, start: f32, modes: &[(f32, f32, f32)], gain: f32) {
    for &(freq, amp, tau) in modes {
        if freq > TOP_PARTIAL_HZ {
            continue;
        }
        let mut o = Osc::default();
        buf.add(start, tau * 7.0, gain * amp, |t| {
            o.sine(freq) * ad(t, 0.0005, tau)
        });
    }
}

/// A crystal ping (glockenspiel-like free-bar modes: 1, 2.76, 5.40).
fn ping(buf: &mut Buffer, start: f32, freq: f32, tau: f32, gain: f32) {
    partials(
        buf,
        start,
        &[
            (freq, 1.0, tau),
            (freq * 2.756, 0.32, tau * 0.45),
            (freq * 5.404, 0.1, tau * 0.25),
        ],
        gain,
    );
}

/// A soft twinkle: a sine with a faint octave, and a detuned twin that beats
/// against it (the shimmer).
fn twinkle(buf: &mut Buffer, start: f32, freq: f32, tau: f32, gain: f32) {
    partials(
        buf,
        start,
        &[
            (freq, 1.0, tau),
            (freq * 1.006, 0.6, tau),
            (freq * 2.0, 0.15, tau * 0.5),
        ],
        gain,
    );
}

/// A glass ping (wine-glass modes), with a detuned twin for a glassy shimmer.
fn glass(buf: &mut Buffer, start: f32, freq: f32, tau: f32, gain: f32) {
    partials(
        buf,
        start,
        &[
            (freq, 1.0, tau),
            (freq * 1.004, 0.45, tau * 0.9),
            (freq * 2.32, 0.5, tau * 0.6),
            (freq * 4.25, 0.25, tau * 0.35),
            (freq * 6.63, 0.12, tau * 0.2),
        ],
        gain,
    );
}

/// A bright bell or chime: a beating prime pair, an octave and bar modes.
fn chime(buf: &mut Buffer, start: f32, freq: f32, tau: f32, gain: f32) {
    partials(
        buf,
        start,
        &[
            (freq, 1.0, tau),
            (freq * 1.003, 0.5, tau * 0.9),
            (freq * 2.0, 0.35, tau * 0.6),
            (freq * 2.756, 0.3, tau * 0.4),
            (freq * 5.404, 0.1, tau * 0.2),
        ],
        gain,
    );
}

/// C-major pentatonic scale degrees (semitones above C).
const PENTATONIC: [f32; 5] = [0.0, 2.0, 4.0, 7.0, 9.0];

/// A seeded pentatonic MIDI note in `lo..=hi`.
fn pentatonic(lo: f32, hi: f32, r: &mut Noise) -> f32 {
    let mut choices = [0.0f32; 64];
    let mut count = 0;
    for octave in 0..11 {
        for d in PENTATONIC {
            let m = 12.0 * octave as f32 + d;
            if (lo..=hi).contains(&m) && count < choices.len() {
                choices[count] = m;
                count += 1;
            }
        }
    }
    let i = ((r.unit() * count as f32) as usize).min(count.saturating_sub(1));
    choices[i]
}

/// A twinkle of `count` pings on pentatonic notes between MIDI `lo` and `hi`,
/// spread over `span` seconds and fading as it goes. `soft` uses [`twinkle`]
/// instead of the brighter crystal [`ping`].
#[allow(clippy::too_many_arguments)]
fn sparkle(
    buf: &mut Buffer,
    start: f32,
    span: f32,
    count: u32,
    (lo, hi): (f32, f32),
    tau: f32,
    gain: f32,
    soft: bool,
    seed: u64,
) {
    let mut r = Noise::new(seed);
    for i in 0..count {
        let slot = (i as f32 + r.range(0.0, 0.7)) / count as f32;
        let freq = note(pentatonic(lo, hi, &mut r));
        let amp = gain * r.range(0.6, 1.0) * (1.0 - 0.45 * i as f32 / count as f32);
        let tau = tau * r.range(0.75, 1.2);
        if soft {
            twinkle(buf, start + span * slot, freq, tau, amp);
        } else {
            ping(buf, start + span * slot, freq, tau, amp);
        }
    }
}

/// Low-passed noise with an envelope `env(t)` and cutoff `cutoff(t)`.
fn noise_lp(
    buf: &mut Buffer,
    start: f32,
    length: f32,
    gain: f32,
    seed: u64,
    cutoff: impl Fn(f32) -> f32,
    env: impl Fn(f32) -> f32,
) {
    let mut n = Noise::new(seed);
    let mut f = Svf::default();
    buf.add(start, length, gain, |t| {
        f.low(n.signed(), cutoff(t)) * env(t)
    });
}

/// Band-passed noise with a moving center.
#[allow(clippy::too_many_arguments)]
fn noise_bp(
    buf: &mut Buffer,
    start: f32,
    length: f32,
    gain: f32,
    seed: u64,
    q: f32,
    center: impl Fn(f32) -> f32,
    env: impl Fn(f32) -> f32,
) {
    let mut n = Noise::new(seed);
    let mut f = Svf::default();
    buf.add(start, length, gain, |t| {
        f.band(n.signed(), center(t), q) * env(t)
    });
}

/// A sine whose pitch falls from `hi` to `lo` with time constant `sweep`.
fn thump(buf: &mut Buffer, start: f32, lo: f32, hi: f32, sweep: f32, tau: f32, gain: f32) {
    let mut o = Osc::default();
    buf.add(start, tau * 7.0, gain, |t| {
        o.sine(lo + (hi - lo) * decay(t, sweep)) * ad(t, 0.0008, tau)
    });
}

/// Sparse crackle: short band-passed ticks at seeded random times.
fn crackle(buf: &mut Buffer, start: f32, span: f32, count: u32, gain: f32, center: f32, seed: u64) {
    let mut r = Noise::new(seed);
    for i in 0..count {
        // Denser early, thinning out.
        let at = start + span * r.unit().powf(1.6);
        let amp = gain * r.range(0.35, 1.0) * (1.0 - i as f32 / (count as f32 * 1.4));
        click(
            buf,
            at,
            center * r.range(0.7, 1.4),
            2.5,
            r.range(0.0006, 0.0018),
            amp,
            seed ^ ((i as u64 + 1) * 7919),
        );
    }
}

/// A cartoon "bonk": a hollow wood-block knock whose pitch drops fast, scaled by
/// `pitch` (1 = about 1000 → 480 Hz).
fn bonk(buf: &mut Buffer, start: f32, pitch: f32, gain: f32, seed: u64) {
    let mut o = Osc::default();
    buf.add(start, 0.2, gain, |t| {
        o.sine(pitch * (480.0 + 520.0 * decay(t, 0.016))) * ad(t, 0.0006, 0.026)
    });
    let mut o2 = Osc::default();
    buf.add(start, 0.07, gain * 0.35, |t| {
        o2.sine(pitch * 2.4 * (500.0 + 320.0 * decay(t, 0.008))) * ad(t, 0.0004, 0.009)
    });
    click(buf, start, 1800.0 * pitch, 1.5, 0.0009, gain * 0.5, seed);
}

// ---------------------------------------------------------------------------
// Weapons
// ---------------------------------------------------------------------------

/// Rifle cast: a bright "zap" that falls from ~2.7 kHz to ~200 Hz, a small punch,
/// and a soft twinkling shimmer. It fires six times a second, so the body is over
/// in ~80 ms and the shimmer stays soft and under 3.2 kHz (the hit sparkle owns
/// the top). `variant` picks one of [`RIFLE_VARIANTS`] round-robin takes.
pub fn rifle_shot(variant: u32) -> Vec<f32> {
    let v = variant % RIFLE_VARIANTS;
    let top = [2600.0, 2760.0, 2460.0][v as usize];
    let mut b = Buffer::new(0.33);
    snap(&mut b, 0.0, 3500.0, 0.0012, 0.35, 11 + v as u64);
    let mut o = Osc::default();
    b.add(0.0, 0.3, 1.0, |t| {
        let freq = 190.0 + top * decay(t, 0.03);
        o.soft_square(freq, 1.0 + 3.0 * decay(t, 0.025)) * ad(t, 0.0006, 0.04)
    });
    thump(&mut b, 0.0, 110.0, 230.0, 0.012, 0.028, 0.45);
    sparkle(
        &mut b,
        0.02,
        0.1,
        5,
        (91.0, 103.0),
        0.018,
        0.2,
        true,
        12 + 7 * v as u64,
    );
    b.master(RIFLE_CAST.rms_db)
}

/// Pump cast: "whoomp-zap". An air push over a falling thump, a fat detuned zap,
/// then a strummed violet chime burst (A6, C7, E7, A7).
pub fn pump_shot() -> Vec<f32> {
    let mut low = Buffer::new(0.55);
    snap(&mut low, 0.0, 2200.0, 0.002, 0.5, 21);
    noise_lp(
        &mut low,
        0.0,
        0.35,
        0.9,
        22,
        |t| 200.0 + 1800.0 * decay(t, 0.03),
        |t| ad(t, 0.003, 0.05),
    );
    thump(&mut low, 0.0, 110.0, 260.0, 0.035, 0.07, 1.1);
    for (detune, phase) in [(0.985, 0.0), (1.015, 0.37)] {
        let mut o = Osc::at(phase);
        low.add(0.0, 0.4, 0.4, |t| {
            let freq = (170.0 + 1500.0 * decay(t, 0.03)) * detune;
            o.soft_square(freq, 1.0 + 2.5 * decay(t, 0.04)) * ad(t, 0.001, 0.055)
        });
    }
    low.saturate(1.8);
    let mut b = low;
    for (i, m) in [93.0, 96.0, 100.0, 105.0].into_iter().enumerate() {
        chime(&mut b, 0.012 + 0.009 * i as f32, note(m), 0.1, 0.26);
    }
    sparkle(&mut b, 0.02, 0.12, 5, (96.0, 108.0), 0.028, 0.14, false, 23);
    b.master(PUMP_CAST.rms_db)
}

/// Pump rack: the gold rings spin up with a rising, fluttering whirr and lock
/// with a metallic tick.
pub fn pump_rack() -> Vec<f32> {
    let mut b = Buffer::new(0.34);
    let spin = 0.2;
    let (mut o1, mut o2, mut lfo) = (Osc::default(), Osc::default(), Osc::default());
    b.add(0.0, spin, 0.5, |t| {
        let x = t / spin;
        let freq = 520.0 * 2f32.powf(1.3 * x);
        let flutter = (0.5 + 0.5 * lfo.sine(16.0 + 32.0 * x)).powi(2);
        let tone = o1.sine(freq) + 0.4 * o2.sine(freq * 2.756);
        tone * flutter * attack(t, 0.03) * release(t, spin, 0.03)
    });
    let mut n = Noise::new(31);
    let mut f = Svf::default();
    let mut lfo2 = Osc::default();
    b.add(0.0, spin, 0.35, |t| {
        let x = t / spin;
        let flutter = (0.5 + 0.5 * lfo2.sine(16.0 + 32.0 * x)).powi(2);
        f.band(n.signed(), 1400.0 * 2f32.powf(1.5 * x), 2.0)
            * flutter
            * attack(t, 0.03)
            * release(t, spin, 0.03)
    });
    let tick = 0.205;
    click(&mut b, tick, 4200.0, 2.0, 0.0012, 0.8, 32);
    partials(
        &mut b,
        tick,
        &[
            (2960.0, 1.0, 0.018),
            (2960.0 * 2.756, 0.4, 0.01),
            (2960.0 * 5.404, 0.15, 0.006),
        ],
        0.55,
    );
    b.master(PUMP_RACK.rms_db)
}

/// Rifle reload, part 1: the dim crystal pops out with a clink, a bounce and a
/// little power-down sigh.
pub fn rifle_mag_out() -> Vec<f32> {
    let mut b = Buffer::new(0.3);
    let mut o = Osc::default();
    b.add(0.0, 0.06, 0.5, |t| {
        o.sine(380.0 + 900.0 * (1.0 - decay(t, 0.006))) * ad(t, 0.0005, 0.008)
    });
    click(&mut b, 0.0, 2400.0, 2.0, 0.001, 0.4, 41);
    glass(&mut b, 0.012, note(100.0), 0.04, 0.8);
    glass(&mut b, 0.07, note(103.0), 0.03, 0.45);
    let mut o2 = Osc::default();
    b.add(0.02, 0.26, 0.2, |t| {
        o2.sine(620.0 * decay(t, 0.25).max(0.4)) * ad(t, 0.01, 0.035)
    });
    b.master(RIFLE_MAG_OUT.rms_db)
}

/// Rifle reload, part 2: a hum rising an octave (C4 to C5) as the fresh crystal
/// slides in, then it seats with a click and a bright crystal chord. It plays when
/// the reload completes, so the rise is kept short and the click lands ~0.12 s
/// after the gun is ready.
pub fn rifle_mag_in() -> Vec<f32> {
    let mut b = Buffer::new(0.3);
    let rise = 0.12;
    let (mut o1, mut o2, mut o3, mut trem) = (
        Osc::default(),
        Osc::default(),
        Osc::default(),
        Osc::default(),
    );
    b.add(0.0, rise + 0.03, 0.55, |t| {
        let x = (t / rise).min(1.0);
        let freq = note(60.0) * 2f32.powf(x.powf(1.5));
        let shimmer = 1.0 + 0.2 * trem.sine(9.0 + 14.0 * x);
        let tone = o1.sine(freq) + 0.45 * o2.sine(2.0 * freq) + 0.25 * o3.sine(3.0 * freq);
        tone * shimmer * (0.25 + 0.75 * x) * attack(t, 0.02) * release(t, rise + 0.03, 0.035)
    });
    click(&mut b, rise, 2000.0, 1.8, 0.0015, 0.8, 52);
    thump(&mut b, rise, 180.0, 300.0, 0.008, 0.018, 0.5);
    glass(&mut b, rise, note(96.0), 0.04, 0.6);
    glass(&mut b, rise + 0.003, note(103.0), 0.03, 0.4);
    sparkle(
        &mut b,
        rise + 0.02,
        0.06,
        3,
        (100.0, 108.0),
        0.02,
        0.2,
        false,
        53,
    );
    b.master(RIFLE_MAG_IN.rms_db)
}

/// One crystal shard pushed into the pump: a small, bright "tink".
pub fn pump_shell() -> Vec<f32> {
    let mut b = Buffer::new(0.12);
    click(&mut b, 0.0, 3000.0, 2.0, 0.0008, 0.5, 61);
    ping(&mut b, 0.001, note(110.0), 0.014, 1.0);
    ping(&mut b, 0.004, note(103.0), 0.01, 0.35);
    b.master(PUMP_SHELL.rms_db)
}

/// A soft magical swish with a faint glint.
pub fn weapon_switch() -> Vec<f32> {
    let mut b = Buffer::new(0.22);
    noise_bp(
        &mut b,
        0.0,
        0.18,
        1.0,
        71,
        1.1,
        |t| 700.0 * 2f32.powf(2.3 * (t / 0.16).min(1.0)),
        |t| (PI * (t / 0.17).min(1.0)).sin().powi(2),
    );
    sparkle(&mut b, 0.07, 0.08, 3, (98.0, 107.0), 0.02, 0.12, true, 72);
    b.master(WEAPON_SWITCH.rms_db)
}

// ---------------------------------------------------------------------------
// Hits
// ---------------------------------------------------------------------------

/// Body hit: a cartoon bonk plus a bright sparkle (E8, A8, G8) above the guns, so
/// hit confirmation cuts through the player's own casts.
pub fn hit_tick() -> Vec<f32> {
    let mut b = Buffer::new(0.105);
    bonk(&mut b, 0.0, 1.0, 0.8, 81);
    ping(&mut b, 0.004, note(112.0), 0.022, 0.6);
    ping(&mut b, 0.018, note(117.0), 0.016, 0.5);
    ping(&mut b, 0.03, note(115.0), 0.012, 0.3);
    b.master(BODY_HIT.rms_db)
}

/// Headshot: the bonk plus a bright, shimmering bell "ding" on C7.
pub fn headshot_ding() -> Vec<f32> {
    let mut b = Buffer::new(0.5);
    bonk(&mut b, 0.0, 1.1, 0.75, 91);
    chime(&mut b, 0.006, note(96.0), 0.13, 0.8);
    sparkle(&mut b, 0.02, 0.1, 3, (103.0, 112.0), 0.03, 0.2, false, 92);
    b.master(HEADSHOT.rms_db)
}

/// Shield hit: a thin, glassy tick on C8.
pub fn shield_hit() -> Vec<f32> {
    let mut b = Buffer::new(0.16);
    snap(&mut b, 0.0, 5000.0, 0.0007, 0.5, 101);
    glass(&mut b, 0.0, note(108.0), 0.02, 1.0);
    b.master(SHIELD_HIT.rms_db)
}

/// Shield break: glass crashes into shards over a whump, then a rising cyan
/// chime (G6, C7, E7, G7).
pub fn shield_break() -> Vec<f32> {
    let mut b = Buffer::new(0.8);
    snap(&mut b, 0.0, 3000.0, 0.05, 0.7, 111);
    let mut r = Noise::new(112);
    for i in 0..34 {
        let at = 0.22 * r.unit().powi(2);
        let freq = r.range(2600.0, 9000.0);
        let tau = r.range(0.012, 0.045);
        let amp = r.range(0.3, 1.0) * (1.0 - i as f32 / 60.0);
        partials(
            &mut b,
            at,
            &[(freq, amp, tau), (freq * 1.51, amp * 0.4, tau * 0.7)],
            0.5,
        );
    }
    thump(&mut b, 0.0, 90.0, 220.0, 0.03, 0.05, 0.6);
    for (i, m) in [91.0, 96.0, 100.0, 103.0].into_iter().enumerate() {
        chime(&mut b, 0.1 + 0.055 * i as f32, note(m), 0.1, 0.4);
    }
    b.master(SHIELD_BREAK.rms_db)
}

/// When the elimination's slide whistle starts (s) and how long it glides.
pub const WHISTLE_START: f32 = 0.06;
pub const WHISTLE_GLIDE: f32 = 0.42;
/// The whistle's first pitch (G6) and how many octaves it falls.
pub const WHISTLE_FROM_HZ: f32 = 1568.0;
pub const WHISTLE_OCTAVES: f32 = 1.8;

/// Elimination: a cartoon smoke "poof" (a noise burst with a fast decay) and a
/// short slide whistle going down.
pub fn elimination() -> Vec<f32> {
    let mut b = Buffer::new(0.62);
    noise_lp(
        &mut b,
        0.0,
        0.3,
        1.0,
        121,
        |t| 400.0 + 2600.0 * decay(t, 0.025),
        |t| ad(t, 0.002, 0.04),
    );
    thump(&mut b, 0.0, 110.0, 220.0, 0.02, 0.035, 0.5);
    let len = 0.5;
    let (mut o1, mut o2, mut vib) = (Osc::default(), Osc::default(), Osc::default());
    let mut n = Noise::new(122);
    let mut bp = Svf::default();
    b.add(WHISTLE_START, len, 0.5, |t| {
        let x = (t / WHISTLE_GLIDE).min(1.0);
        let freq = WHISTLE_FROM_HZ
            * 2f32.powf(-WHISTLE_OCTAVES * x.powf(1.4))
            * (1.0 + 0.012 * vib.sine(7.0));
        let tone = o1.sine(freq) + 0.08 * o2.sine(2.0 * freq);
        let breath = 0.15 * bp.band(n.signed(), freq, 6.0);
        (tone + breath) * attack(t, 0.02) * release(t, len, 0.07)
    });
    b.master(ELIMINATION.rms_db)
}

// ---------------------------------------------------------------------------
// Building
// ---------------------------------------------------------------------------

/// Stone modes for brick: heavily damped.
const BRICK_MODES: [(f32, f32, f32); 4] = [
    (330.0, 1.0, 0.022),
    (790.0, 0.6, 0.013),
    (1330.0, 0.35, 0.008),
    (2150.0, 0.2, 0.005),
];

/// Wood modes for planks above the fundamental: a hollow, ringing "o".
const PLANK_MODES: [(f32, f32, f32); 3] = [
    (700.0, 0.7, 0.03),
    (1330.0, 0.4, 0.018),
    (2150.0, 0.22, 0.01),
];

fn scaled(modes: &[(f32, f32, f32)], k: f32, tau_k: f32) -> Vec<(f32, f32, f32)> {
    modes
        .iter()
        .map(|&(f, a, tau)| (f * k, a, tau * tau_k))
        .collect()
}

/// Brick wall placed: a heavy, dull "clunk" with a little grit.
pub fn brick_place() -> Vec<f32> {
    let mut b = Buffer::new(0.28);
    thump(&mut b, 0.0, 130.0, 250.0, 0.012, 0.025, 0.8);
    // The dull, noisy "unk" of the body.
    noise_bp(
        &mut b,
        0.0,
        0.2,
        2.2,
        131,
        3.0,
        |_| 360.0,
        |t| ad(t, 0.001, 0.02),
    );
    partials(&mut b, 0.0, &BRICK_MODES, 1.0);
    noise_lp(
        &mut b,
        0.0,
        0.08,
        0.35,
        132,
        |t| 700.0 + 1800.0 * decay(t, 0.006),
        |t| ad(t, 0.0005, 0.012),
    );
    crackle(&mut b, 0.002, 0.04, 8, 0.15, 2600.0, 133);
    b.saturate(1.4);
    // Brick is dull: no ring, no sheen.
    b.low_pass(2400.0);
    b.master(BRICK_PLACE.rms_db)
}

/// Plank floor or ramp placed: a hollow wooden "thock".
pub fn plank_place() -> Vec<f32> {
    let mut b = Buffer::new(0.28);
    click(&mut b, 0.0, 2800.0, 1.6, 0.0008, 0.6, 141);
    let mut o = Osc::default();
    b.add(0.0, 0.28, 1.0, |t| {
        o.sine(280.0 + 45.0 * decay(t, 0.006)) * ad(t, 0.0006, 0.042)
    });
    partials(&mut b, 0.0, &PLANK_MODES, 0.8);
    thump(&mut b, 0.0, 140.0, 200.0, 0.01, 0.025, 0.4);
    b.saturate(1.2);
    b.master(PLANK_PLACE.rms_db)
}

/// Brick cracks: a gritty fracture and a trickle of dust.
pub fn brick_crack() -> Vec<f32> {
    let mut b = Buffer::new(0.32);
    snap(&mut b, 0.0, 1200.0, 0.0025, 0.55, 151);
    crackle(&mut b, 0.0, 0.09, 26, 0.7, 2000.0, 152);
    thump(&mut b, 0.0, 170.0, 260.0, 0.012, 0.025, 0.4);
    noise_bp(
        &mut b,
        0.01,
        0.3,
        0.25,
        153,
        0.8,
        |_| 1600.0,
        |t| ad(t, 0.01, 0.06) * release(t, 0.3, 0.05),
    );
    crackle(&mut b, 0.08, 0.2, 10, 0.3, 3200.0, 154);
    b.master(BRICK_CRACK.rms_db)
}

/// Planks crack: a woody snap and a short creak.
pub fn plank_crack() -> Vec<f32> {
    let mut b = Buffer::new(0.38);
    click(&mut b, 0.0, 2200.0, 1.2, 0.002, 1.0, 161);
    partials(
        &mut b,
        0.0,
        &[(410.0, 0.5, 0.02), (980.0, 0.35, 0.012)],
        0.6,
    );
    let mut o = Osc::default();
    let mut f = Svf::default();
    b.add(0.015, 0.36, 0.7, |t| {
        let pitch = 160.0 + 40.0 * (t * 9.0 * TAU).sin() + 90.0 * t;
        f.band(o.saw(pitch, 12), 900.0, 4.0) * ad(t, 0.02, 0.08) * release(t, 0.36, 0.05)
    });
    crackle(&mut b, 0.0, 0.2, 14, 0.6, 2600.0, 162);
    b.master(PLANK_CRACK.rms_db)
}

/// Brick wall breaks: a fracture, then chunks crumble and tumble over a rumble.
pub fn brick_break() -> Vec<f32> {
    let mut b = Buffer::new(0.8);
    snap(&mut b, 0.0, 1000.0, 0.004, 0.8, 171);
    thump(&mut b, 0.0, 120.0, 230.0, 0.03, 0.07, 1.0);
    noise_lp(
        &mut b,
        0.0,
        0.75,
        0.8,
        172,
        |t| 480.0 + 900.0 * decay(t, 0.08),
        |t| ad(t, 0.004, 0.14) * release(t, 0.75, 0.1),
    );
    let mut r = Noise::new(173);
    for i in 0..16 {
        let at = 0.02 + 0.55 * r.unit().powf(1.5);
        let k = r.range(0.7, 1.6);
        let amp = r.range(0.35, 0.8) * (1.0 - i as f32 / 24.0);
        partials(&mut b, at, &scaled(&BRICK_MODES[..3], k, 0.55), amp);
        click(
            &mut b,
            at,
            1800.0 * k,
            1.5,
            0.0008,
            amp * 0.4,
            1730 + i as u64,
        );
    }
    crackle(&mut b, 0.0, 0.5, 36, 0.5, 2200.0, 174);
    b.saturate(1.3);
    b.master(BRICK_BREAK.rms_db)
}

/// Planks break: a big splintering snap, then boards knock as they fall.
pub fn plank_break() -> Vec<f32> {
    let mut b = Buffer::new(0.75);
    snap(&mut b, 0.0, 1800.0, 0.003, 1.0, 181);
    crackle(&mut b, 0.0, 0.06, 20, 1.0, 3000.0, 182);
    thump(&mut b, 0.0, 90.0, 180.0, 0.02, 0.05, 0.8);
    let mut r = Noise::new(183);
    for i in 0..7 {
        let at = 0.04 + 0.35 * r.unit();
        let k = r.range(0.8, 1.3);
        let amp = r.range(0.35, 0.7);
        let mut modes = vec![(280.0 * k, 1.0, 0.035)];
        modes.extend(scaled(&PLANK_MODES, k, 0.7));
        partials(&mut b, at, &modes, amp);
        click(
            &mut b,
            at,
            2800.0 * k,
            1.6,
            0.0008,
            amp * 0.5,
            1830 + i as u64,
        );
    }
    crackle(&mut b, 0.02, 0.45, 30, 0.55, 2600.0, 184);
    b.master(PLANK_BREAK.rms_db)
}

/// Placement rejected: a soft, muted cartoon "bwomp" that bends down while a
/// low-pass opens and closes on it.
pub fn rejected() -> Vec<f32> {
    let mut b = Buffer::new(0.26);
    let len = 0.22;
    let (mut o1, mut o2) = (Osc::default(), Osc::at(0.25));
    let mut f = Svf::default();
    b.add(0.0, len, 1.0, |t| {
        let pitch = 200.0 + 130.0 * decay(t, 0.05);
        let src = o1.soft_square(pitch, 1.6) + 0.5 * o2.soft_square(pitch * 1.01, 1.6);
        let cutoff = 300.0 + 1500.0 * (PI * (t / 0.17).min(1.0)).sin();
        f.low(src, cutoff) * attack(t, 0.012) * release(t, len, 0.06) * (0.6 + 0.4 * decay(t, 0.08))
    });
    b.master(REJECTED.rms_db)
}

// ---------------------------------------------------------------------------
// Movement
// ---------------------------------------------------------------------------

/// A soft step on grass: a muffled thud and a crinkly blade rustle. `variant`
/// picks one of [`FOOTSTEP_VARIANTS`] round-robin takes.
pub fn footstep(variant: u32) -> Vec<f32> {
    let v = variant % FOOTSTEP_VARIANTS;
    let k = [1.0, 1.1, 0.9][v as usize];
    let seed = 191 + 10 * v as u64;
    let mut b = Buffer::new(0.16);
    noise_bp(
        &mut b,
        0.0,
        0.15,
        1.4,
        seed,
        0.8,
        |_| 320.0 * k,
        |t| ad(t, 0.003, 0.022),
    );
    thump(&mut b, 0.0, 150.0 * k, 220.0 * k, 0.01, 0.02, 0.35);
    noise_bp(
        &mut b,
        0.002,
        0.13,
        0.35,
        seed + 1,
        0.8,
        |t| k * (3600.0 - 12_000.0 * t),
        |t| ad(t, 0.004, 0.026),
    );
    crackle(&mut b, 0.002, 0.07, 12, 0.28, 3800.0 * k, seed + 2);
    b.master(FOOTSTEP.rms_db)
}

/// Jump: a grass scuff and a light "boing" rising an octave with a spring wobble
/// that settles fast. Subtle, not silly.
pub fn jump() -> Vec<f32> {
    let mut b = Buffer::new(0.22);
    noise_bp(
        &mut b,
        0.0,
        0.1,
        0.35,
        201,
        0.9,
        |_| 3500.0,
        |t| ad(t, 0.002, 0.018),
    );
    noise_lp(
        &mut b,
        0.0,
        0.1,
        0.4,
        202,
        |_| 400.0,
        |t| ad(t, 0.002, 0.02),
    );
    let (mut o, mut wobble) = (Osc::default(), Osc::default());
    b.add(0.005, 0.21, 0.45, |t| {
        let freq = 290.0
            * 2f32.powf(1.0 - decay(t, 0.03))
            * (1.0 + 0.05 * decay(t, 0.06) * wobble.sine(24.0));
        o.sine(freq) * ad(t, 0.004, 0.032)
    });
    b.master(JUMP.rms_db)
}

/// Landing: a soft thud on turf with a little grass crush.
pub fn land() -> Vec<f32> {
    let mut b = Buffer::new(0.26);
    noise_lp(
        &mut b,
        0.0,
        0.26,
        1.0,
        211,
        |t| 550.0 + 1000.0 * decay(t, 0.012),
        |t| ad(t, 0.002, 0.036),
    );
    thump(&mut b, 0.0, 120.0, 240.0, 0.015, 0.035, 0.7);
    noise_bp(
        &mut b,
        0.0,
        0.12,
        0.3,
        212,
        0.9,
        |_| 3200.0,
        |t| ad(t, 0.003, 0.025),
    );
    crackle(&mut b, 0.0, 0.06, 10, 0.25, 3600.0, 213);
    b.saturate(1.3);
    b.master(LAND.rms_db)
}

/// Slide: a falling grass-and-air swish.
pub fn slide() -> Vec<f32> {
    let mut b = Buffer::new(0.55);
    noise_bp(
        &mut b,
        0.0,
        0.55,
        0.9,
        221,
        0.9,
        |t| 2600.0 * 2f32.powf(-1.5 * (t / 0.45).min(1.0)),
        |t| ad(t, 0.035, 0.16) * release(t, 0.55, 0.08),
    );
    noise_lp(
        &mut b,
        0.0,
        0.5,
        0.35,
        222,
        |_| 300.0,
        |t| ad(t, 0.02, 0.15) * release(t, 0.5, 0.08),
    );
    crackle(&mut b, 0.01, 0.4, 28, 0.2, 3600.0, 223);
    b.master(SLIDE.rms_db)
}
