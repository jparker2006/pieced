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
// Handling (M4, D106).
pub const WEAPON_DRAW: CueSpec = spec(0.40, -11.0);
pub const ADS_IN: CueSpec = spec(0.22, -12.0);
pub const ADS_OUT: CueSpec = spec(0.20, -12.0);
// The reload and rack beats (M4, D106), cued by the viewmodel's animation.
pub const CHAMBER_OPEN: CueSpec = spec(0.2, -12.0);
pub const CRYSTAL_GRAB: CueSpec = spec(0.14, -12.0);
pub const CRYSTAL_SLOT: CueSpec = spec(0.26, -11.0);
pub const CHAMBER_SHUT: CueSpec = spec(0.22, -11.0);
pub const CRYSTAL_CHARGE: CueSpec = spec(0.42, -11.0);
pub const RACK_PULL: CueSpec = spec(0.32, -11.0);
pub const RACK_CLACK: CueSpec = spec(0.26, -10.0);
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
// The knights' wand and orb (M3).
pub const ORB_CAST: CueSpec = spec(0.50, -11.0);
pub const ORB_WHOOSH: CueSpec = spec(0.45, -11.0);
pub const ORB_BONK: CueSpec = spec(0.40, -10.0);
pub const WAND_WARNING: CueSpec = spec(0.40, -11.0);
// The run's beats (M3 chunk 2).
pub const POTION_GULP: CueSpec = spec(0.50, -11.0);
pub const WAVE_CLEARED: CueSpec = spec(0.90, -11.0);
pub const WAVE_START: CueSpec = spec(0.55, -12.0);
pub const NEW_BEST: CueSpec = spec(1.00, -10.0);
// Ship arrivals and the void (M3 chunk 3).
pub const SHIP_HUM: CueSpec = spec(1.0, -11.0);
pub const SHIP_BEAM: CueSpec = spec(1.0, -11.0);
pub const VOID_YELP: CueSpec = spec(1.0, -10.0);
// Kill feedback (M4 chunk 1).
pub const KILL_CONFIRM: CueSpec = spec(0.55, -11.0);
pub const HELMET_DING: CueSpec = spec(0.65, -11.0);
pub const ARMOR_CLATTER: CueSpec = spec(0.36, -12.0);
pub const MULTI_KILL: CueSpec = spec(0.95, -11.0);

/// Clatter takes (round-robin) and multi-kill sting levels (double .. rampage).
pub const CLATTER_VARIANTS: u32 = 3;
pub const MULTI_KILL_LEVELS: u32 = 4;

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

/// Rifle cast, layered (M4, D106): the brass **mechanism** clicking, the
/// **magic body** (a bright "zap" that falls from ~2.7 kHz to ~200 Hz over a
/// small punch), and a **tail** (a soft twinkling shimmer and a breath of
/// air; the tight room is baked on top). It fires six times a second, so the
/// body is over in ~80 ms, and everything stays under 3.5 kHz except the soft
/// shimmer (the hit sparkle owns the top). `variant` picks one of
/// [`RIFLE_VARIANTS`] round-robin takes.
pub fn rifle_shot(variant: u32) -> Vec<f32> {
    let v = variant % RIFLE_VARIANTS;
    let top = [2600.0, 2760.0, 2460.0][v as usize];
    let mut b = Buffer::new(0.33);
    // Transient: the mechanism.
    click(
        &mut b,
        0.0,
        2300.0 + 150.0 * v as f32,
        3.0,
        0.0007,
        0.4,
        13 + v as u64,
    );
    partials(
        &mut b,
        0.0004,
        &[
            (1870.0 + 40.0 * v as f32, 1.0, 0.004),
            (3150.0, 0.5, 0.0025),
        ],
        0.22,
    );
    // Body: the zap and its punch.
    snap(&mut b, 0.0, 3500.0, 0.0012, 0.35, 11 + v as u64);
    let mut o = Osc::default();
    b.add(0.0, 0.3, 1.0, |t| {
        let freq = 190.0 + top * decay(t, 0.03);
        o.soft_square(freq, 1.0 + 3.0 * decay(t, 0.025)) * ad(t, 0.0006, 0.04)
    });
    thump(&mut b, 0.0, 110.0, 230.0, 0.012, 0.028, 0.45);
    // Tail: the shimmer and a breath of air.
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
    noise_bp(
        &mut b,
        0.01,
        0.2,
        0.1,
        14 + v as u64,
        1.3,
        |t| 1600.0 * 2f32.powf(-t / 0.08),
        |t| ad(t, 0.004, 0.045),
    );
    b.master(RIFLE_CAST.rms_db)
}

/// Pump cast, layered (M4, D106): a **low whoomp** (an air push over a deep
/// falling thump and a fat detuned zap), a **chime** (a strummed violet burst:
/// A6, C7, E7, A7) and a **tail** (the blast's air falling away; the tight
/// room is baked on top).
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
    thump(&mut low, 0.0, 95.0, 240.0, 0.045, 0.085, 1.2);
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
    // Tail: the air of the blast sweeping down as it dies.
    noise_bp(
        &mut b,
        0.03,
        0.45,
        0.22,
        24,
        1.1,
        |t| 1400.0 * 2f32.powf(-t / 0.15),
        |t| ad(t, 0.02, 0.11),
    );
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

/// A gun coming up (M4, D106): a cloth swish, the brass seating in the glove
/// with a clack, and the crystal waking with a rising hum and a glint.
pub fn weapon_draw() -> Vec<f32> {
    let mut b = Buffer::new(0.38);
    // Transient-to-come: the swish of the draw.
    noise_bp(
        &mut b,
        0.0,
        0.14,
        0.7,
        75,
        1.0,
        |t| 600.0 * 2f32.powf(2.0 * (t / 0.13).min(1.0)),
        |t| (PI * (t / 0.14).min(1.0)).sin().powi(2),
    );
    // Body: the brass seats.
    let seat = 0.12;
    click(&mut b, seat, 2800.0, 2.5, 0.0009, 0.7, 76);
    partials(
        &mut b,
        seat,
        &[
            (1420.0, 1.0, 0.012),
            (2330.0, 0.6, 0.008),
            (3050.0, 0.3, 0.005),
        ],
        0.4,
    );
    thump(&mut b, seat, 170.0, 300.0, 0.008, 0.02, 0.5);
    // Tail: the crystal wakes, a hum rising an octave, and a glint.
    let (mut o1, mut o2) = (Osc::default(), Osc::default());
    b.add(seat + 0.01, 0.22, 0.28, |t| {
        let x = (t / 0.16).min(1.0);
        let f = note(64.0) * 2f32.powf(x);
        (o1.sine(f) + 0.35 * o2.sine(2.0 * f)) * attack(t, 0.03) * release(t, 0.22, 0.07)
    });
    twinkle(&mut b, seat + 0.14, note(100.0), 0.03, 0.25);
    b.master(WEAPON_DRAW.rms_db)
}

/// Aiming down sights: the gun shifts in the gloves (a short leathery swish
/// and a soft brass tick) and a faint rising glint.
pub fn ads_in() -> Vec<f32> {
    let mut b = Buffer::new(0.2);
    noise_lp(
        &mut b,
        0.0,
        0.1,
        0.8,
        77,
        |t| 900.0 + 900.0 * (t / 0.08).min(1.0),
        |t| (PI * (t / 0.09).min(1.0)).sin(),
    );
    click(&mut b, 0.055, 1900.0, 2.0, 0.0008, 0.45, 78);
    partials(
        &mut b,
        0.055,
        &[(1650.0, 1.0, 0.008), (2700.0, 0.4, 0.005)],
        0.25,
    );
    let mut o = Osc::default();
    b.add(0.06, 0.12, 0.12, |t| {
        o.sine(note(96.0) * 2f32.powf(0.3 * t / 0.1)) * ad(t, 0.01, 0.04)
    });
    b.master(ADS_IN.rms_db)
}

/// Leaving the sights: the shift, softer and falling, with no glint.
pub fn ads_out() -> Vec<f32> {
    let mut b = Buffer::new(0.18);
    noise_lp(
        &mut b,
        0.0,
        0.09,
        0.8,
        79,
        |t| 1700.0 - 900.0 * (t / 0.08).min(1.0),
        |t| (PI * (t / 0.08).min(1.0)).sin(),
    );
    click(&mut b, 0.045, 1600.0, 2.0, 0.0008, 0.35, 80);
    partials(&mut b, 0.045, &[(1450.0, 1.0, 0.007)], 0.2);
    b.master(ADS_OUT.rms_db)
}

/// Rifle reload: the glass chamber slides open (a glassy scrape up, a tick,
/// a faint ring).
pub fn chamber_open() -> Vec<f32> {
    let mut b = Buffer::new(0.19);
    click(&mut b, 0.0, 2600.0, 2.0, 0.0008, 0.5, 131);
    noise_bp(
        &mut b,
        0.004,
        0.07,
        0.6,
        132,
        4.0,
        |t| 1900.0 + 1600.0 * (t / 0.06).min(1.0),
        |t| (PI * (t / 0.07).min(1.0)).sin(),
    );
    glass(&mut b, 0.06, note(98.0), 0.03, 0.35);
    b.master(CHAMBER_OPEN.rms_db)
}

/// Rifle reload: the glove closes on a fresh crystal (a soft leathery thud
/// and a small crystal tick).
pub fn crystal_grab() -> Vec<f32> {
    let mut b = Buffer::new(0.13);
    noise_lp(
        &mut b,
        0.0,
        0.05,
        1.0,
        133,
        |_| 700.0,
        |t| ad(t, 0.004, 0.012),
    );
    thump(&mut b, 0.0, 160.0, 240.0, 0.006, 0.014, 0.5);
    ping(&mut b, 0.012, note(103.0), 0.012, 0.3);
    b.master(CRYSTAL_GRAB.rms_db)
}

/// Rifle reload: the fresh crystal clicks into its socket (a click, a seat
/// thump and a bright glass pair).
pub fn crystal_slot() -> Vec<f32> {
    let mut b = Buffer::new(0.25);
    click(&mut b, 0.0, 2000.0, 1.8, 0.0015, 0.8, 134);
    thump(&mut b, 0.0, 180.0, 300.0, 0.008, 0.018, 0.5);
    glass(&mut b, 0.0, note(96.0), 0.04, 0.6);
    glass(&mut b, 0.004, note(103.0), 0.03, 0.4);
    b.master(CRYSTAL_SLOT.rms_db)
}

/// Rifle reload: the glass chamber snaps shut (a sharp snap over a low
/// glass knock).
pub fn chamber_shut() -> Vec<f32> {
    let mut b = Buffer::new(0.21);
    click(&mut b, 0.0, 3200.0, 2.2, 0.001, 0.9, 135);
    thump(&mut b, 0.0, 200.0, 360.0, 0.006, 0.016, 0.45);
    glass(&mut b, 0.002, note(91.0), 0.045, 0.55);
    b.master(CHAMBER_SHUT.rms_db)
}

/// Rifle reload done: the crystal charges (a hum rising an octave, C4 to C5)
/// and flashes (a bright chord and a sparkle).
pub fn crystal_charge() -> Vec<f32> {
    let mut b = Buffer::new(0.4);
    let rise = 0.18;
    let (mut o1, mut o2, mut trem) = (Osc::default(), Osc::default(), Osc::default());
    b.add(0.0, rise + 0.03, 0.5, |t| {
        let x = (t / rise).min(1.0);
        let f = note(60.0) * 2f32.powf(x.powf(1.4));
        let shimmer = 1.0 + 0.2 * trem.sine(9.0 + 16.0 * x);
        (o1.sine(f) + 0.4 * o2.sine(2.0 * f))
            * shimmer
            * (0.3 + 0.7 * x)
            * attack(t, 0.02)
            * release(t, rise + 0.03, 0.03)
    });
    chime(&mut b, rise, note(96.0), 0.09, 0.45);
    chime(&mut b, rise + 0.006, note(100.0), 0.08, 0.3);
    sparkle(
        &mut b,
        rise + 0.02,
        0.1,
        4,
        (100.0, 108.0),
        0.02,
        0.18,
        false,
        136,
    );
    b.master(CRYSTAL_CHARGE.rms_db)
}

/// Pump rack, pulled: the gold rings whirr up as the grip slides back (a
/// rising, fluttering spin over a metal slide).
pub fn rack_pull() -> Vec<f32> {
    let mut b = Buffer::new(0.31);
    let spin = 0.28;
    let (mut o1, mut o2, mut lfo) = (Osc::default(), Osc::default(), Osc::default());
    b.add(0.0, spin, 0.5, |t| {
        let x = t / spin;
        let freq = 480.0 * 2f32.powf(1.4 * x);
        let flutter = (0.5 + 0.5 * lfo.sine(14.0 + 34.0 * x)).powi(2);
        (o1.sine(freq) + 0.4 * o2.sine(freq * 2.756))
            * flutter
            * attack(t, 0.03)
            * release(t, spin, 0.04)
    });
    noise_bp(
        &mut b,
        0.0,
        0.12,
        0.45,
        137,
        2.0,
        |t| 1300.0 + 700.0 * (t / 0.1).min(1.0),
        |t| ad(t, 0.005, 0.04),
    );
    click(&mut b, 0.0, 2200.0, 2.0, 0.001, 0.4, 138);
    b.master(RACK_PULL.rms_db)
}

/// Pump rack, slammed home: a heavy brass clack (a hard click, a low knock
/// and short metal modes) and the lock's tick.
pub fn rack_clack() -> Vec<f32> {
    let mut b = Buffer::new(0.25);
    click(&mut b, 0.0, 2500.0, 1.6, 0.0014, 1.0, 139);
    thump(&mut b, 0.0, 140.0, 280.0, 0.01, 0.03, 0.9);
    partials(
        &mut b,
        0.0,
        &[
            (1180.0, 1.0, 0.03),
            (1960.0, 0.6, 0.02),
            (2950.0, 0.35, 0.012),
        ],
        0.45,
    );
    click(&mut b, 0.03, 4200.0, 2.0, 0.0012, 0.5, 140);
    b.master(RACK_CLACK.rms_db)
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
    snap(&mut b, 0.0, 1500.0, 0.002, 0.4, 160);
    click(&mut b, 0.0, 2200.0, 1.2, 0.002, 0.9, 161);
    partials(
        &mut b,
        0.0,
        &[(410.0, 0.5, 0.02), (980.0, 0.35, 0.012)],
        0.9,
    );
    let mut o = Osc::default();
    let mut f = Svf::default();
    b.add(0.015, 0.36, 0.5, |t| {
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
        1.5,
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

// ---------------------------------------------------------------------------
// The knights' wand and orb (M3)
// ---------------------------------------------------------------------------
//
// Fire, not frost: the orb sounds sit low and warm (a breathy flame roar and
// soft crackle, 200 Hz – 2 kHz) so they never mask the player's bright zaps
// and hit sparkles, and read at once as "incoming".

/// Orb cast: a "fwoom" — a flame catching (band noise swelling up from
/// 300 Hz), a falling warm tone and a few embers crackling off.
pub fn orb_cast() -> Vec<f32> {
    let mut b = Buffer::new(0.46);
    noise_bp(
        &mut b,
        0.0,
        0.44,
        1.0,
        301,
        0.9,
        |t| 320.0 + 900.0 * (t / 0.09).min(1.0) * decay((t - 0.09).max(0.0), 0.2),
        |t| attack(t, 0.035) * decay(t, 0.13) * release(t, 0.44, 0.08),
    );
    let (mut o1, mut o2) = (Osc::default(), Osc::at(0.3));
    b.add(0.0, 0.4, 0.55, |t| {
        let freq = 190.0 + 260.0 * decay(t, 0.08);
        (o1.soft_square(freq, 1.6) + 0.4 * o2.sine(freq * 2.01))
            * attack(t, 0.02)
            * decay(t, 0.1)
            * release(t, 0.4, 0.06)
    });
    crackle(&mut b, 0.05, 0.3, 12, 0.3, 2400.0, 302);
    b.saturate(1.4);
    b.master(ORB_CAST.rms_db)
}

/// Orb passing close: a doppler whoosh — a breathy roar that swells, peaks as
/// the orb goes by and drops in pitch as it leaves, with a low flame tone
/// bending down with it.
pub fn orb_whoosh() -> Vec<f32> {
    let len = 0.42;
    let peak = 0.2;
    let mut b = Buffer::new(len);
    // Approach (higher) to recede (lower): about a fifth down, eased around the pass.
    let pitch = |t: f32| 2f32.powf(-0.7 / (1.0 + (-(t - peak) / 0.035).exp()));
    let swell = |t: f32| {
        let x = (t - peak) / if t < peak { 0.09 } else { 0.07 };
        (-x * x).exp()
    };
    noise_bp(
        &mut b,
        0.0,
        len,
        1.0,
        311,
        1.4,
        |t| 1500.0 * pitch(t),
        |t| swell(t) * release(t, len, 0.05),
    );
    noise_lp(
        &mut b,
        0.0,
        len,
        0.6,
        312,
        |t| 700.0 * pitch(t),
        |t| swell(t) * release(t, len, 0.05),
    );
    let mut o = Osc::default();
    b.add(0.0, len, 0.35, |t| {
        o.sine(420.0 * pitch(t)) * swell(t) * release(t, len, 0.05)
    });
    b.master(ORB_WHOOSH.rms_db)
}

/// Orb hitting the player: a crunchy cartoon "bonk" — a low, hollow knock with
/// a gritty crunch of embers and a short hiss of flame.
pub fn orb_bonk() -> Vec<f32> {
    let mut b = Buffer::new(0.36);
    bonk(&mut b, 0.0, 0.62, 1.0, 321);
    thump(&mut b, 0.0, 150.0, 330.0, 0.02, 0.05, 0.8);
    crackle(&mut b, 0.004, 0.08, 22, 0.55, 1900.0, 322);
    noise_lp(
        &mut b,
        0.01,
        0.3,
        0.45,
        323,
        |t| 3000.0 * decay(t, 0.08) + 500.0,
        |t| ad(t, 0.004, 0.07) * release(t, 0.3, 0.05),
    );
    b.saturate(2.2);
    b.master(ORB_BONK.rms_db)
}

/// A knight winding up off-screen: a short rising charge (a warm, fluttering
/// tone climbing over the wind-up) the player can locate, done by the time
/// the orb leaves.
pub fn wand_warning() -> Vec<f32> {
    let len = 0.34;
    let mut b = Buffer::new(len);
    let (mut o1, mut o2, mut lfo) = (Osc::default(), Osc::at(0.2), Osc::default());
    b.add(0.0, len, 0.7, |t| {
        let x = t / len;
        let freq = note(67.0) * 2f32.powf(1.1 * x);
        let flutter = 0.65 + 0.35 * lfo.sine(18.0 + 20.0 * x);
        (o1.soft_square(freq, 1.3) + 0.35 * o2.sine(freq * 1.5))
            * flutter
            * attack(t, 0.03)
            * release(t, len, 0.05)
    });
    noise_bp(
        &mut b,
        0.0,
        len,
        0.35,
        331,
        1.2,
        |t| 700.0 * 2f32.powf(1.3 * t / len),
        |t| attack(t, 0.06) * release(t, len, 0.05),
    );
    b.master(WAND_WARNING.rms_db)
}

// ---------------------------------------------------------------------------
// The run's beats (M3 chunk 2)
// ---------------------------------------------------------------------------
//
// Bright, happy and pentatonic like the hits, but longer and rounder: they
// reward, they don't confirm. Each is heard alone (a potion, a cleared wave,
// the next wave, a new best), so they may sit loud without masking a hit.

/// Drinking a potion: two glassy "gulp" bloops (a sine bending up from low)
/// and a cyan glass chime on E6 and A6.
pub fn potion_gulp() -> Vec<f32> {
    let mut b = Buffer::new(0.5);
    for (i, at) in [0.0f32, 0.09].into_iter().enumerate() {
        let mut o = Osc::default();
        let from = if i == 0 { 260.0 } else { 300.0 };
        b.add(at, 0.09, 0.9, |t| {
            let bend = 1.0 - decay(t, 0.025);
            o.sine(from + 420.0 * bend) * ad(t, 0.004, 0.03) * release(t, 0.09, 0.02)
        });
        click(&mut b, at, 900.0, 2.0, 0.002, 0.35, 401 + i as u64);
    }
    glass(&mut b, 0.17, note(88.0), 0.09, 0.55);
    glass(&mut b, 0.22, note(93.0), 0.08, 0.45);
    sparkle(&mut b, 0.2, 0.18, 4, (100.0, 110.0), 0.03, 0.18, true, 402);
    b.master(POTION_GULP.rms_db)
}

/// A wave cleared: a quick rising arpeggio of chimes (C5 E5 G5 C6 E6) with a
/// soft sparkle on top.
pub fn wave_cleared() -> Vec<f32> {
    let mut b = Buffer::new(0.9);
    for (i, m) in [72.0, 76.0, 79.0, 84.0, 88.0].into_iter().enumerate() {
        let last = i == 4;
        chime(
            &mut b,
            0.075 * i as f32,
            note(m),
            if last { 0.2 } else { 0.1 },
            if last { 0.8 } else { 0.55 },
        );
    }
    sparkle(&mut b, 0.3, 0.35, 6, (96.0, 108.0), 0.05, 0.22, true, 411);
    b.master(WAVE_CLEARED.rms_db)
}

/// The next wave: a short two-note horn call (G4 then C5) with a little
/// breath, warm rather than bright so it reads as "here they come".
pub fn wave_start() -> Vec<f32> {
    let mut b = Buffer::new(0.55);
    for (i, (m, len)) in [(67.0, 0.13), (72.0, 0.34)].into_iter().enumerate() {
        let at = 0.14 * i as f32;
        let (mut o1, mut o2, mut vib) = (Osc::default(), Osc::at(0.25), Osc::default());
        let freq = note(m);
        b.add(at, len, 0.6, |t| {
            let f = freq * (1.0 + 0.006 * vib.sine(6.0) * (t / len));
            (o1.soft_square(f, 1.8) + 0.3 * o2.sine(f * 2.0))
                * attack(t, 0.02)
                * release(t, len, 0.06)
        });
        // A bright ping on each note's onset gives the call its bite.
        chime(&mut b, at, freq * 2.0, 0.04, 0.5);
        noise_bp(
            &mut b,
            at,
            len,
            0.12,
            421 + i as u64,
            3.0,
            move |_| freq * 2.0,
            move |t| attack(t, 0.02) * release(t, len, 0.06),
        );
    }
    b.master(WAVE_START.rms_db)
}

/// "NEW BEST!": a fanfare arpeggio (C5 E5 G5 C6), a held bright chord and a
/// shower of sparkles.
pub fn new_best() -> Vec<f32> {
    let mut b = Buffer::new(0.98);
    for (i, m) in [72.0, 76.0, 79.0].into_iter().enumerate() {
        let at = 0.09 * i as f32;
        let mut o = Osc::default();
        let freq = note(m);
        b.add(at, 0.14, 0.4, |t| {
            o.soft_square(freq, 1.4) * attack(t, 0.008) * release(t, 0.14, 0.05)
        });
        chime(&mut b, at, freq * 2.0, 0.08, 0.35);
    }
    let hold = 0.27;
    for (k, m) in [84.0, 88.0, 91.0].into_iter().enumerate() {
        let (mut o1, mut o2) = (Osc::default(), Osc::at(0.1 * k as f32));
        let freq = note(m);
        b.add(hold, 0.7, 0.28, |t| {
            (o1.soft_square(freq, 1.2) + 0.25 * o2.sine(freq * 2.0))
                * attack(t, 0.01)
                * decay(t, 0.3)
                * release(t, 0.7, 0.1)
        });
    }
    chime(&mut b, hold, note(96.0), 0.25, 0.5);
    sparkle(
        &mut b,
        hold + 0.05,
        0.6,
        10,
        (98.0, 112.0),
        0.06,
        0.25,
        false,
        431,
    );
    b.master(NEW_BEST.rms_db)
}

// ---------------------------------------------------------------------------
// Ship arrivals and the void (M3 chunk 3)
// ---------------------------------------------------------------------------

/// The rune circle lighting under a drop ship: a warm hum swelling over the
/// second before the beam comes on (two detuned voices climbing a fifth, a
/// quickening flutter and a rising breath of shimmer); the beam's shimmer
/// takes over for the second after, so the landing is heard coming for 2 s.
pub fn ship_hum() -> Vec<f32> {
    let len = 0.98;
    let mut b = Buffer::new(len);
    let (mut o1, mut o2, mut o3, mut lfo) =
        (Osc::default(), Osc::at(0.37), Osc::at(0.61), Osc::default());
    b.add(0.0, len, 0.6, |t| {
        let x = t / len;
        let freq = note(55.0) * 2f32.powf(7.0 / 12.0 * x * x);
        let flutter = 0.75 + 0.25 * lfo.sine(4.0 + 10.0 * x);
        (o1.soft_square(freq, 1.2)
            + 0.7 * o2.soft_square(freq * 1.004, 1.2)
            + 0.3 * o3.sine(freq * 2.0))
            * flutter
            * (0.25 + 0.75 * x.powf(1.3))
            * attack(t, 0.15)
            * release(t, len, 0.1)
    });
    noise_bp(
        &mut b,
        0.0,
        len,
        0.3,
        341,
        2.0,
        |t| 900.0 * 2f32.powf(1.2 * t / len),
        |t| (t / len).powf(1.6) * release(t, len, 0.1),
    );
    b.master(SHIP_HUM.rms_db)
}

/// A knight sliding down the beam: a bright glide falling an octave with a
/// shimmer of pentatonic twinkles and an airy swish.
pub fn ship_beam() -> Vec<f32> {
    let len = 0.98;
    let mut b = Buffer::new(len);
    let (mut o1, mut o2) = (Osc::default(), Osc::at(0.25));
    b.add(0.0, len, 0.5, |t| {
        let x = t / len;
        let freq = note(84.0) * 2f32.powf(-x);
        (o1.sine(freq) + 0.35 * o2.sine(freq * 1.5))
            * attack(t, 0.05)
            * (1.0 - 0.6 * x)
            * release(t, len, 0.15)
    });
    sparkle(&mut b, 0.02, 0.7, 7, (84.0, 100.0), 0.12, 0.35, true, 342);
    noise_bp(
        &mut b,
        0.0,
        len,
        0.4,
        343,
        1.5,
        |t| 3000.0 * 2f32.powf(-1.2 * t / len),
        |t| attack(t, 0.08) * release(t, len, 0.2),
    );
    b.master(SHIP_BEAM.rms_db)
}

/// A knight knocked off the island: a cartoon "yip!" and a long slide whistle
/// falling away into the void.
pub fn void_yelp() -> Vec<f32> {
    let len = 0.98;
    let mut b = Buffer::new(len);
    // The yip: a quick nasal chirp upward.
    let (mut y1, mut y2) = (Osc::default(), Osc::at(0.3));
    b.add(0.0, 0.16, 0.7, |t| {
        let freq = 620.0 + 520.0 * (t / 0.08).min(1.0);
        (y1.soft_square(freq, 1.8) + 0.4 * y2.sine(freq * 2.0))
            * attack(t, 0.01)
            * release(t, 0.16, 0.05)
    });
    // The fall: a slide whistle down two octaves, with vibrato.
    let fall = 0.82;
    let (mut o1, mut o2, mut vib) = (Osc::default(), Osc::default(), Osc::default());
    let mut n = Noise::new(344);
    let mut bp = Svf::default();
    b.add(0.14, fall, 0.5, |t| {
        let x = t / fall;
        let freq = 1500.0 * 2f32.powf(-2.2 * x.powf(1.2)) * (1.0 + 0.015 * vib.sine(6.5));
        let tone = o1.sine(freq) + 0.08 * o2.sine(2.0 * freq);
        let breath = 0.15 * bp.band(n.signed(), freq, 6.0);
        (tone + breath) * attack(t, 0.02) * (1.0 - 0.5 * x) * release(t, fall, 0.12)
    });
    b.master(VOID_YELP.rms_db)
}

// ---------------------------------------------------------------------------
// Kill feedback (M4 chunk 1, D105)
// ---------------------------------------------------------------------------

/// The kill confirm, over the hit sound: a low bonk, then a bright
/// "cha-ching": a metal click with a chime on E7, and a ringing coin chime on
/// C8 with a shimmer of coins. Distinct from every hit: longer, higher and
/// two-stepped.
pub fn kill_confirm() -> Vec<f32> {
    let mut b = Buffer::new(0.52);
    bonk(&mut b, 0.0, 0.8, 0.6, 501);
    // "Cha".
    click(&mut b, 0.012, 3800.0, 2.0, 0.0012, 0.6, 502);
    chime(&mut b, 0.012, note(100.0), 0.05, 0.55);
    // "Ching".
    let ching = 0.075;
    click(&mut b, ching, 5200.0, 2.5, 0.001, 0.5, 503);
    chime(&mut b, ching, note(108.0), 0.16, 0.8);
    chime(&mut b, ching + 0.004, note(103.0), 0.12, 0.35);
    sparkle(
        &mut b,
        ching + 0.02,
        0.18,
        6,
        (103.0, 115.0),
        0.03,
        0.22,
        false,
        504,
    );
    b.master(KILL_CONFIRM.rms_db)
}

/// A headshot rings the helmet: a struck steel shell (a clank, then
/// inharmonic bell partials on A6) that rings out bright over the headshot's
/// bonk.
pub fn helmet_ding() -> Vec<f32> {
    let mut b = Buffer::new(0.62);
    snap(&mut b, 0.0, 4000.0, 0.0008, 0.6, 511);
    click(&mut b, 0.0, 2600.0, 3.0, 0.0015, 0.7, 512);
    let f = note(93.0);
    partials(
        &mut b,
        0.001,
        &[
            (f, 1.0, 0.16),
            (f * 1.007, 0.5, 0.14),
            (f * 2.32, 0.55, 0.09),
            (f * 4.25, 0.3, 0.05),
            (f * 5.4, 0.22, 0.035),
            (f * 6.9, 0.12, 0.02),
        ],
        0.9,
    );
    b.master(HELMET_DING.rms_db)
}

/// Armor landing on the grass: two or three quick metallic clanks as it
/// bounces and settles. `variant` picks one of [`CLATTER_VARIANTS`] takes.
pub fn armor_clatter(variant: u32) -> Vec<f32> {
    let v = variant % CLATTER_VARIANTS;
    let mut b = Buffer::new(0.34);
    let mut r = Noise::new(520 + u64::from(v));
    let hits = [(0.0, 1.0), (0.11, 0.55), (0.19, 0.3)];
    let count = if v == 1 { 2 } else { 3 };
    for (k, &(at, amp)) in hits.iter().take(count).enumerate() {
        let at = (at + r.range(-0.015, 0.015)).max(0.0);
        let f = r.range(900.0, 1500.0) * [1.0, 1.12, 0.9][v as usize];
        snap(&mut b, at, 3000.0, 0.0008, 0.5 * amp, 530 + k as u64);
        partials(
            &mut b,
            at,
            &[
                (f, 1.0, 0.025),
                (f * 2.41, 0.6, 0.018),
                (f * 3.87, 0.35, 0.012),
                (f * 5.2, 0.2, 0.008),
            ],
            amp,
        );
        thump(&mut b, at, 160.0, 320.0, 0.006, 0.012, 0.35 * amp);
    }
    b.master(ARMOR_CLATTER.rms_db)
}

/// The multi-kill sting: a quick rising fanfare of `level + 2` notes up the
/// pentatonic (C6 E6 G6 C7 E7) that lands on a held chime, bigger and
/// brighter each level ("Double!" .. "Rampage!").
pub fn multi_kill(level: u32) -> Vec<f32> {
    let level = level % MULTI_KILL_LEVELS;
    let notes = [84.0, 88.0, 91.0, 96.0, 100.0];
    let count = level as usize + 2;
    let step = 0.065;
    let mut b = Buffer::new(0.9);
    for (i, &m) in notes.iter().take(count).enumerate() {
        let at = step * i as f32;
        let freq = note(m);
        let mut o = Osc::default();
        let last = i + 1 == count;
        let len = if last { 0.5 } else { 0.1 };
        b.add(at, len, 0.35, |t| {
            o.soft_square(freq, 1.3) * attack(t, 0.006) * decay(t, 0.12) * release(t, len, 0.05)
        });
        chime(&mut b, at, freq * 2.0, if last { 0.2 } else { 0.06 }, 0.35);
    }
    let top = step * (count - 1) as f32;
    if level >= 2 {
        thump(&mut b, 0.0, 110.0, 240.0, 0.03, 0.06, 0.5);
    }
    sparkle(
        &mut b,
        top + 0.02,
        0.35,
        4 + 2 * level,
        (100.0, 115.0),
        0.05,
        0.2,
        false,
        540 + u64::from(level),
    );
    b.master(MULTI_KILL.rms_db)
}
