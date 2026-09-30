//! The knights' gibberish voice (M4 chunk 6, D123): a tiny formant
//! synthesizer and the barks made with it.
//!
//! A knight's voice is a buzzy glottal source (a band-limited sawtooth with a
//! little breath noise) through three resonant formant filters that glide
//! between cartoon vowel shapes, with a few consonant shapes on the front and
//! back of each syllable: an **h** (breath through the vowel's formants), a
//! **w** and a **y** (the vowel gliding in from *oo* or *ee*), an **n** (a
//! nasal hum) and a **b** (a closed lip popping open), and a stopped end (a
//! **p**, **t** or **k**: the voice cut short by a small burst). The formants
//! sit about 15% above an adult's and the pitch around 300–700 Hz, so the
//! knights sound small and goofy, in the Animal-Crossing / Overwatch-bark
//! spirit, but nothing here is sampled or copied: every bark is designed
//! below.
//!
//! | Bark | When | Shape |
//! |---|---|---|
//! | [`hup`] | landing off a drop ship's beam | "hup!": breath, a short rising *uh*, stopped on a *p* |
//! | [`taunt`] | some wind-ups (≤ 1 in 3, on screen only) | "nyah-nyah!", "heh-heh-heh!" or "bla-HAH!" |
//! | [`yelp`] | being hit | "ow!", "eep!", "oof!" or "yip!" |
//! | [`hoo_hah`] | the victory hop | "hoo-HAH!": a low *hoo* and a big rising *hah* |
//! | [`whaaa`] | knocked into the void | "whaaaa…": a *w* into a long *ah* sliding down with a wobble |
//!
//! Each is mastered like every cue ([`Buffer::master`]) to its loudness
//! target; each knight's personal pitch and a yelp's jitter are playback
//! speed ([`super::barks`]).

use super::synth::{Buffer, Noise, Svf, attack, decay, release};

const SR: f32 = super::synth::SAMPLE_RATE as f32;

/// A vowel's first three formants (Hz) and their levels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vowel {
    pub f: [f32; 3],
    pub g: [f32; 3],
}

const fn v(f1: f32, f2: f32, f3: f32) -> Vowel {
    Vowel {
        f: [f1 * 1.15, f2 * 1.15, f3 * 1.15],
        g: [1.0, 0.7, 0.4],
    }
}

/// "ah" (father).
pub const AH: Vowel = v(760.0, 1180.0, 2500.0);
/// "uh" (hup).
pub const UH: Vowel = v(620.0, 1200.0, 2400.0);
/// "a" (cat).
pub const AE: Vowel = v(680.0, 1700.0, 2450.0);
/// "eh" (bed).
pub const EH: Vowel = v(520.0, 1850.0, 2550.0);
/// "ee" (see).
pub const EE: Vowel = v(300.0, 2300.0, 3000.0);
/// "oh".
pub const OH: Vowel = v(480.0, 880.0, 2400.0);
/// "oo" (hoo).
pub const OO: Vowel = v(330.0, 800.0, 2250.0);

impl Vowel {
    fn lerp(self, o: Vowel, x: f32) -> Vowel {
        let m = |a: f32, b: f32| a + (b - a) * x;
        Vowel {
            f: [
                m(self.f[0], o.f[0]),
                m(self.f[1], o.f[1]),
                m(self.f[2], o.f[2]),
            ],
            g: [
                m(self.g[0], o.g[0]),
                m(self.g[1], o.g[1]),
                m(self.g[2], o.g[2]),
            ],
        }
    }
}

/// How a syllable starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Onset {
    /// Straight into the vowel.
    Vowel,
    /// Breath through the vowel's formants first.
    H,
    /// The vowel glides in from "oo".
    W,
    /// The vowel glides in from "ee".
    Y,
    /// A nasal hum first.
    N,
    /// A lip pop.
    B,
}

/// How a syllable ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Coda {
    /// A soft release.
    Open,
    /// Cut short by a burst at this centre (Hz): ~900 a "p", ~2500 a "k",
    /// ~4000 a "t".
    Stop(f32),
    /// Trailing breath through a high band (an "f").
    Breath,
}

/// One syllable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Syllable {
    pub start: f32,
    pub len: f32,
    pub from: Vowel,
    pub to: Vowel,
    /// Pitch at the start and the end (Hz).
    pub f0: (f32, f32),
    pub onset: Onset,
    pub coda: Coda,
    pub gain: f32,
    /// Vibrato depth (fraction of the pitch) at 6 Hz, growing over the syllable.
    pub vibrato: f32,
}

impl Syllable {
    /// A syllable on one vowel.
    pub const fn new(start: f32, len: f32, vowel: Vowel, f0: (f32, f32)) -> Self {
        Self {
            start,
            len,
            from: vowel,
            to: vowel,
            f0,
            onset: Onset::Vowel,
            coda: Coda::Open,
            gain: 1.0,
            vibrato: 0.0,
        }
    }

    pub const fn onset(mut self, onset: Onset) -> Self {
        self.onset = onset;
        self
    }

    pub const fn coda(mut self, coda: Coda) -> Self {
        self.coda = coda;
        self
    }

    pub const fn to(mut self, to: Vowel) -> Self {
        self.to = to;
        self
    }

    pub const fn gain(mut self, gain: f32) -> Self {
        self.gain = gain;
        self
    }

    pub const fn vibrato(mut self, depth: f32) -> Self {
        self.vibrato = depth;
        self
    }
}

/// PolyBLEP correction for a naive sawtooth's jump (phase `t` in cycles, step
/// `dt` in cycles per sample).
fn blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

/// Smooth 0..1 ease.
fn ease(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Renders `syllable` into `buf` (seeded breath noise).
pub fn sing(buf: &mut Buffer, s: &Syllable, seed: u64) {
    // The onset's own length, before the vowel is fully open.
    let pre = match s.onset {
        Onset::H => 0.045,
        Onset::N => 0.05,
        Onset::B => 0.012,
        Onset::W | Onset::Y => 0.0,
        Onset::Vowel => 0.0,
    };
    let glide = match s.onset {
        Onset::W | Onset::Y => 0.07,
        _ => 0.0,
    };
    let stop_at = s.len;
    let tail = match s.coda {
        Coda::Open => 0.0,
        Coda::Stop(_) => 0.03,
        Coda::Breath => 0.07,
    };
    let total = pre + s.len + tail;
    let mut noise = Noise::new(seed);
    let mut filters = [Svf::default(), Svf::default(), Svf::default()];
    let mut nasal = Svf::default();
    let mut burst = Svf::default();
    let mut phase = 0.0f32;
    let mut vib = 0.0f32;
    let start = s.start;
    buf.add(start, total, s.gain, |t| {
        // Time into the vowel.
        let tv = t - pre;
        let x = (tv / s.len).clamp(0.0, 1.0);
        let mut vowel = s.from.lerp(s.to, ease(x));
        if glide > 0.0 && tv < glide {
            let from = if s.onset == Onset::W { OO } else { EE };
            vowel = from.lerp(vowel, ease(tv.max(0.0) / glide));
        }
        // Pitch: a glide between the ends (exponential), with vibrato.
        let f0 = s.f0.0 * (s.f0.1 / s.f0.0).powf(x);
        vib += 6.0 / SR;
        let f0 = f0 * (1.0 + s.vibrato * x * (vib * std::f32::consts::TAU).sin());
        let dt = f0 / SR;
        phase += dt;
        if phase >= 1.0 {
            phase -= 1.0;
        }
        let saw = 2.0 * phase - 1.0 - blep(phase, dt);
        let breath = noise.signed();
        // The voice's envelope over the vowel, and the onset's.
        let voiced = match s.onset {
            Onset::H => attack(tv, 0.02),
            Onset::B => {
                if tv < 0.0 {
                    0.0
                } else {
                    attack(tv, 0.004)
                }
            }
            Onset::N => attack(tv, 0.015),
            _ => attack(t, 0.012),
        } * match s.coda {
            Coda::Open => release(tv, stop_at, 0.05),
            Coda::Stop(_) | Coda::Breath => release(tv, stop_at, 0.012),
        };
        // The "h": breath that fades as the voice comes in.
        let aspiration = match s.onset {
            Onset::H => attack(t, 0.008) * (1.0 - ease(t / (pre + 0.025))),
            _ => 0.0,
        };
        let source = saw * voiced + breath * (0.06 * voiced + 1.2 * aspiration);
        let mut out = 0.0;
        for (k, f) in filters.iter_mut().enumerate() {
            let q = vowel.f[k] / [120.0, 150.0, 210.0][k];
            out += vowel.g[k] * f.band(source, vowel.f[k], q) / q;
        }
        // The nasal hum: the buzz through a low, narrow band, with lips closed.
        if s.onset == Onset::N && tv < 0.01 {
            let hum = nasal.band(saw, 280.0, 3.0) / 3.0 * attack(t, 0.01);
            out = out * attack(tv.max(0.0), 0.01) + 0.8 * hum;
        }
        // The lip pop.
        if s.onset == Onset::B && (0.0..0.008).contains(&t) {
            out += 0.5 * burst.band(breath, 600.0, 1.5) * decay(t, 0.002);
        }
        // The stopped end: a short burst after the voice cuts.
        match s.coda {
            Coda::Stop(centre) if tv >= stop_at => {
                out += 0.45 * burst.band(breath, centre, 1.8) * decay(tv - stop_at, 0.004);
            }
            Coda::Breath if tv >= stop_at - 0.02 => {
                out += 0.3
                    * burst.band(breath, 3200.0, 1.2)
                    * attack(tv - stop_at + 0.02, 0.01)
                    * release(tv, stop_at + tail, 0.04);
            }
            _ => {}
        }
        out
    });
}

/// Sings `syllables` into a buffer `seconds` long, mastered to `rms_db`.
pub fn phrase(seconds: f32, syllables: &[Syllable], seed: u64, rms_db: f32) -> Vec<f32> {
    let mut b = Buffer::new(seconds);
    for (i, s) in syllables.iter().enumerate() {
        sing(&mut b, s, seed.wrapping_add(i as u64 * 131));
    }
    // A touch of grit, like a cartoon voice actor pushing it.
    b.saturate(1.15);
    b.master(rms_db)
}

/// Taunts in the bank: "nyah-nyah!", "heh-heh-heh!", "bla-HAH!".
pub const TAUNT_TAKES: u32 = 3;
/// Yelps in the bank: "ow!", "eep!", "oof!", "yip!".
pub const YELP_TAKES: u32 = 4;

/// "Hup!": a breath, a short rising *uh*, stopped on a *p*.
pub fn hup(rms_db: f32) -> Vec<f32> {
    let s = Syllable::new(0.0, 0.11, UH, (300.0, 400.0))
        .onset(Onset::H)
        .coda(Coda::Stop(900.0));
    phrase(0.22, &[s], 601, rms_db)
}

/// A short taunt, take `take`: "nyah-nyah!" (the playground minor third,
/// down), a snickering "heh-heh-heh!", or a raspberry-ish "bla-HAH!".
pub fn taunt(take: u32, rms_db: f32) -> Vec<f32> {
    match take % TAUNT_TAKES {
        0 => phrase(
            0.62,
            &[
                Syllable::new(0.0, 0.17, AE, (470.0, 450.0))
                    .onset(Onset::N)
                    .to(AH),
                Syllable::new(0.25, 0.26, AE, (395.0, 370.0))
                    .onset(Onset::N)
                    .to(AH)
                    .vibrato(0.03),
            ],
            611,
            rms_db,
        ),
        1 => phrase(
            0.5,
            &[
                Syllable::new(0.0, 0.08, EH, (430.0, 410.0)).onset(Onset::H),
                Syllable::new(0.13, 0.08, EH, (410.0, 390.0))
                    .onset(Onset::H)
                    .gain(0.9),
                Syllable::new(0.26, 0.12, EH, (390.0, 340.0))
                    .onset(Onset::H)
                    .gain(0.85),
            ],
            621,
            rms_db,
        ),
        _ => phrase(
            0.56,
            &[
                Syllable::new(0.0, 0.12, AH, (340.0, 300.0)).onset(Onset::B),
                Syllable::new(0.2, 0.22, AH, (430.0, 540.0))
                    .onset(Onset::H)
                    .coda(Coda::Stop(2500.0))
                    .gain(1.1),
            ],
            631,
            rms_db,
        ),
    }
}

/// A yelp, take `take`: "ow!" (ah gliding to oo, pitch falling), "eep!"
/// (high, rising, stopped), "oof!" (low, winded, a breath at the end) or
/// "yip!" (very high and quick).
pub fn yelp(take: u32, rms_db: f32) -> Vec<f32> {
    let (len, s) = match take % YELP_TAKES {
        0 => (0.28, Syllable::new(0.0, 0.2, AH, (560.0, 400.0)).to(OO)),
        1 => (
            0.2,
            Syllable::new(0.0, 0.1, EE, (640.0, 760.0)).coda(Coda::Stop(900.0)),
        ),
        2 => (
            0.28,
            Syllable::new(0.0, 0.13, OH, (300.0, 240.0))
                .onset(Onset::H)
                .to(OO)
                .coda(Coda::Breath),
        ),
        _ => (
            0.16,
            Syllable::new(0.0, 0.08, EE, (700.0, 860.0))
                .onset(Onset::Y)
                .coda(Coda::Stop(4000.0)),
        ),
    };
    phrase(len, &[s], 641 + u64::from(take % YELP_TAKES), rms_db)
}

/// "Hoo-HAH!": a low *hoo*, then a big rising *hah* with a wobble.
pub fn hoo_hah(rms_db: f32) -> Vec<f32> {
    phrase(
        0.7,
        &[
            Syllable::new(0.0, 0.17, OO, (320.0, 350.0)).onset(Onset::H),
            Syllable::new(0.27, 0.3, AH, (440.0, 560.0))
                .onset(Onset::H)
                .vibrato(0.035)
                .gain(1.15),
        ],
        651,
        rms_db,
    )
}

/// "Whaaaa…": a *w* into a long *ah* sliding down an octave, the wobble
/// growing, the voice fading as he falls away.
pub fn whaaa(rms_db: f32) -> Vec<f32> {
    let mut b = Buffer::new(1.0);
    let s = Syllable::new(0.0, 0.92, AH, (560.0, 250.0))
        .onset(Onset::W)
        .to(OH)
        .vibrato(0.05);
    sing(&mut b, &s, 661);
    // Falling away: quieter toward the end.
    let n = b.samples.len() as f32;
    for (i, x) in b.samples.iter_mut().enumerate() {
        *x *= 1.0 - 0.55 * (i as f32 / n);
    }
    b.saturate(1.15);
    b.master(rms_db)
}
