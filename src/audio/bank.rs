//! The sound designs. Each function returns mono samples at
//! [`synth::SAMPLE_RATE`](super::synth::SAMPLE_RATE), normalized and click-free.
//! All randomness is seeded, so the bank is byte-identical on every launch.

use super::synth::{Buffer, Noise, Osc, Svf, ad, attack, decay};

/// A short noise burst through a band-pass, for clicks and clacks.
fn click(buf: &mut Buffer, start: f32, center: f32, q: f32, tau: f32, gain: f32, seed: u64) {
    let mut n = Noise::new(seed);
    let mut f = Svf::default();
    buf.add(start, tau * 8.0, gain, |t| {
        f.band(n.signed(), center, q) * decay(t, tau)
    });
}

/// Decaying sine partials (metallic pings, bells, wood modes).
fn partials(buf: &mut Buffer, start: f32, modes: &[(f32, f32, f32)], gain: f32) {
    for &(freq, amp, tau) in modes {
        let mut o = Osc::default();
        buf.add(start, tau * 7.0, gain * amp, |t| {
            o.sine(freq) * ad(t, 0.0005, tau)
        });
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
    buf.add(start, length, gain, |t| f.low(n.signed(), cutoff(t)) * env(t));
}

/// Band-passed noise with a moving center.
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

// ---------------------------------------------------------------------------
// Weapons
// ---------------------------------------------------------------------------

/// Punchy, bassy rifle crack.
pub fn rifle_shot() -> Vec<f32> {
    let mut b = Buffer::new(0.42);
    // Supersonic crack: a very short, bright transient.
    let mut n = Noise::new(11);
    let mut hp = Svf::default();
    b.add(0.0, 0.02, 0.9, |t| {
        hp.high(n.signed(), 2500.0) * decay(t, 0.0022)
    });
    // Body: noise through a closing low-pass.
    noise_lp(
        &mut b,
        0.0,
        0.3,
        0.95,
        12,
        |t| 300.0 + 5200.0 * decay(t, 0.018),
        |t| ad(t, 0.0005, 0.045),
    );
    // Bass thump that gives the shot weight.
    thump(&mut b, 0.0, 52.0, 170.0, 0.025, 0.065, 1.1);
    // Short room tail.
    noise_lp(
        &mut b,
        0.008,
        0.4,
        0.22,
        13,
        |_| 900.0,
        |t| ad(t, 0.01, 0.12),
    );
    b.saturate(1.8);
    b.finish(0.92)
}

/// Big pump boom.
pub fn pump_shot() -> Vec<f32> {
    let mut b = Buffer::new(0.85);
    let mut n = Noise::new(21);
    let mut hp = Svf::default();
    b.add(0.0, 0.03, 0.8, |t| {
        hp.high(n.signed(), 1800.0) * decay(t, 0.003)
    });
    noise_lp(
        &mut b,
        0.0,
        0.6,
        1.0,
        22,
        |t| 220.0 + 3000.0 * decay(t, 0.045),
        |t| ad(t, 0.0008, 0.085),
    );
    thump(&mut b, 0.0, 38.0, 130.0, 0.045, 0.16, 1.35);
    noise_lp(
        &mut b,
        0.01,
        0.8,
        0.35,
        23,
        |_| 520.0,
        |t| ad(t, 0.02, 0.26),
    );
    b.saturate(2.2);
    b.finish(0.95)
}

/// "Chk-chk": the pump rack after a shot (or a finished reload).
pub fn pump_rack() -> Vec<f32> {
    let mut b = Buffer::new(0.42);
    // Back...
    click(&mut b, 0.0, 2300.0, 1.8, 0.006, 0.9, 31);
    partials(
        &mut b,
        0.0,
        &[
            (1850.0, 0.5, 0.018),
            (3100.0, 0.35, 0.012),
            (4700.0, 0.2, 0.008),
        ],
        0.8,
    );
    thump(&mut b, 0.0, 180.0, 260.0, 0.01, 0.018, 0.5);
    // ...slide...
    noise_bp(
        &mut b,
        0.02,
        0.16,
        0.18,
        32,
        1.2,
        |t| 1200.0 + 6000.0 * t,
        |t| ad(t, 0.03, 0.06),
    );
    // ...and forward, a touch higher and harder.
    click(&mut b, 0.19, 2700.0, 1.8, 0.006, 1.0, 33);
    partials(
        &mut b,
        0.19,
        &[
            (2150.0, 0.55, 0.02),
            (3500.0, 0.4, 0.013),
            (5300.0, 0.2, 0.008),
        ],
        0.9,
    );
    thump(&mut b, 0.19, 200.0, 300.0, 0.01, 0.02, 0.6);
    b.finish(0.85)
}

/// Rifle magazine release and pull.
pub fn rifle_mag_out() -> Vec<f32> {
    let mut b = Buffer::new(0.36);
    click(&mut b, 0.0, 2800.0, 2.5, 0.004, 0.8, 41);
    partials(
        &mut b,
        0.0,
        &[(2600.0, 0.4, 0.012), (4100.0, 0.25, 0.008)],
        0.7,
    );
    noise_bp(
        &mut b,
        0.03,
        0.18,
        0.35,
        42,
        1.4,
        |t| 1500.0 + 2500.0 * t,
        |t| ad(t, 0.04, 0.05),
    );
    thump(&mut b, 0.2, 140.0, 220.0, 0.01, 0.03, 0.3);
    click(&mut b, 0.2, 1200.0, 1.5, 0.006, 0.35, 43);
    b.finish(0.8)
}

/// Rifle magazine seated, then the bolt released.
pub fn rifle_mag_in() -> Vec<f32> {
    let mut b = Buffer::new(0.42);
    noise_bp(
        &mut b,
        0.0,
        0.1,
        0.3,
        51,
        1.4,
        |t| 2600.0 - 8000.0 * t,
        |t| ad(t, 0.05, 0.03),
    );
    // Seat.
    click(&mut b, 0.09, 1500.0, 1.6, 0.007, 1.0, 52);
    thump(&mut b, 0.09, 150.0, 230.0, 0.012, 0.03, 0.7);
    partials(
        &mut b,
        0.09,
        &[(1400.0, 0.5, 0.02), (2300.0, 0.35, 0.014)],
        0.6,
    );
    // Bolt release: bright and snappy.
    click(&mut b, 0.25, 3000.0, 2.0, 0.004, 0.9, 53);
    partials(
        &mut b,
        0.25,
        &[
            (3000.0, 0.45, 0.018),
            (4400.0, 0.3, 0.012),
            (6200.0, 0.15, 0.006),
        ],
        0.8,
    );
    b.finish(0.85)
}

/// One pump shell pushed into the tube.
pub fn pump_shell() -> Vec<f32> {
    let mut b = Buffer::new(0.2);
    click(&mut b, 0.0, 1600.0, 3.0, 0.006, 0.9, 61);
    partials(
        &mut b,
        0.0,
        &[(900.0, 0.5, 0.025), (2400.0, 0.3, 0.012)],
        0.7,
    );
    thump(&mut b, 0.0, 170.0, 240.0, 0.01, 0.025, 0.4);
    // The shell's plastic tuck a moment later.
    click(&mut b, 0.05, 2200.0, 2.0, 0.004, 0.45, 62);
    b.finish(0.8)
}

/// Mechanical draw: a cloth rustle and a click.
pub fn weapon_switch() -> Vec<f32> {
    let mut b = Buffer::new(0.22);
    noise_bp(
        &mut b,
        0.0,
        0.12,
        0.35,
        71,
        0.9,
        |t| 900.0 + 5000.0 * t,
        |t| ad(t, 0.035, 0.03),
    );
    click(&mut b, 0.085, 2600.0, 2.2, 0.004, 0.9, 72);
    partials(
        &mut b,
        0.085,
        &[(2200.0, 0.4, 0.012), (3400.0, 0.3, 0.008)],
        0.7,
    );
    b.finish(0.8)
}

// ---------------------------------------------------------------------------
// Hits
// ---------------------------------------------------------------------------

/// Crisp hitmarker tick: high and short, nothing like a gunshot.
pub fn hit_tick() -> Vec<f32> {
    let mut b = Buffer::new(0.09);
    partials(
        &mut b,
        0.0,
        &[
            (2650.0, 1.0, 0.016),
            (3975.0, 0.45, 0.011),
            (5300.0, 0.2, 0.007),
        ],
        1.0,
    );
    let mut n = Noise::new(81);
    let mut hp = Svf::default();
    b.add(0.0, 0.01, 0.5, |t| {
        hp.high(n.signed(), 4000.0) * decay(t, 0.0008)
    });
    b.finish(0.85)
}

/// Headshot: a bright bell ding over the tick.
pub fn headshot_ding() -> Vec<f32> {
    let mut b = Buffer::new(0.62);
    let f = 1760.0;
    partials(
        &mut b,
        0.0,
        &[
            (f, 1.0, 0.16),
            (f * 2.0, 0.45, 0.1),
            (f * 2.76, 0.4, 0.07),
            (f * 4.07, 0.22, 0.045),
            (f * 5.4, 0.12, 0.03),
        ],
        1.0,
    );
    partials(
        &mut b,
        0.0,
        &[(2650.0, 0.6, 0.014), (3975.0, 0.3, 0.009)],
        0.8,
    );
    b.finish(0.85)
}

/// Shield hit: glassy and bright.
pub fn shield_hit() -> Vec<f32> {
    let mut b = Buffer::new(0.24);
    partials(
        &mut b,
        0.0,
        &[
            (3150.0, 0.8, 0.05),
            (4520.0, 0.6, 0.04),
            (6080.0, 0.45, 0.03),
            (7900.0, 0.3, 0.02),
        ],
        1.0,
    );
    // A second, detuned layer gives the glassy beating.
    partials(
        &mut b,
        0.004,
        &[(3190.0, 0.4, 0.045), (4585.0, 0.3, 0.035)],
        0.8,
    );
    let mut n = Noise::new(91);
    let mut hp = Svf::default();
    b.add(0.0, 0.02, 0.45, |t| {
        hp.high(n.signed(), 5000.0) * decay(t, 0.0015)
    });
    b.finish(0.8)
}

/// Shield break: a shatter of glass shards over a low whump.
pub fn shield_break() -> Vec<f32> {
    let mut b = Buffer::new(0.75);
    let mut r = Noise::new(101);
    for i in 0..36 {
        let at = 0.26 * r.unit().powi(2);
        let f = r.range(2400.0, 8200.0);
        let tau = r.range(0.018, 0.06);
        let amp = r.range(0.3, 1.0) * (1.0 - i as f32 / 60.0);
        partials(
            &mut b,
            at,
            &[(f, amp, tau), (f * 1.51, amp * 0.4, tau * 0.7)],
            0.5,
        );
    }
    let mut n = Noise::new(102);
    let mut hp = Svf::default();
    b.add(0.0, 0.5, 0.55, |t| {
        hp.high(n.signed(), 3200.0) * ad(t, 0.001, 0.07)
    });
    thump(&mut b, 0.0, 70.0, 150.0, 0.04, 0.1, 0.7);
    // A falling glassy sweep ties the shards together.
    let mut o = Osc::default();
    b.add(0.0, 0.4, 0.18, |t| {
        o.sine(5200.0 - 3600.0 * (t / 0.35).min(1.0)) * ad(t, 0.005, 0.08)
    });
    b.finish(0.85)
}

/// Elimination: a satisfying pop and a rising two-note chime.
pub fn elimination() -> Vec<f32> {
    let mut b = Buffer::new(0.75);
    let mut o = Osc::default();
    b.add(0.0, 0.25, 0.9, |t| {
        o.sine(250.0 + 550.0 * (1.0 - decay(t, 0.012))) * ad(t, 0.001, 0.05)
    });
    let mut n = Noise::new(111);
    let mut f = Svf::default();
    b.add(0.0, 0.05, 0.4, |t| {
        f.band(n.signed(), 1800.0, 1.0) * decay(t, 0.006)
    });
    for (start, note) in [(0.05, 880.0), (0.13, 1318.5)] {
        partials(
            &mut b,
            start,
            &[
                (note, 0.8, 0.2),
                (note * 2.0, 0.35, 0.12),
                (note * 3.0, 0.15, 0.07),
            ],
            0.8,
        );
    }
    b.finish(0.85)
}

// ---------------------------------------------------------------------------
// Building
// ---------------------------------------------------------------------------

/// Wooden panel thunk.
pub fn piece_place() -> Vec<f32> {
    let mut b = Buffer::new(0.32);
    thump(&mut b, 0.0, 105.0, 170.0, 0.02, 0.05, 0.9);
    partials(
        &mut b,
        0.0,
        &[
            (420.0, 0.7, 0.035),
            (690.0, 0.5, 0.025),
            (1130.0, 0.3, 0.014),
        ],
        0.8,
    );
    noise_lp(
        &mut b,
        0.0,
        0.08,
        0.55,
        121,
        |_| 1600.0,
        |t| ad(t, 0.0005, 0.01),
    );
    b.saturate(1.3);
    b.finish(0.85)
}

/// Creak and crackle as a piece cracks further.
pub fn piece_crack() -> Vec<f32> {
    let mut b = Buffer::new(0.48);
    let mut o = Osc::default();
    let mut f = Svf::default();
    b.add(0.0, 0.45, 0.8, |t| {
        let pitch = 175.0 + 45.0 * (t * 7.0 * std::f32::consts::TAU).sin() + 60.0 * t;
        f.band(o.saw(pitch, 12), 950.0, 4.0) * ad(t, 0.03, 0.14)
    });
    crackle(&mut b, 0.0, 0.3, 22, 0.9, 2800.0, 131);
    b.finish(0.8)
}

/// Chunky wood crunch when a piece breaks.
pub fn piece_break() -> Vec<f32> {
    let mut b = Buffer::new(0.8);
    let mut r = Noise::new(141);
    for _ in 0..7 {
        let at = 0.22 * r.unit();
        let k = r.range(0.7, 1.35);
        partials(
            &mut b,
            at,
            &[
                (420.0 * k, 0.7, 0.03),
                (690.0 * k, 0.5, 0.02),
                (1130.0 * k, 0.3, 0.012),
            ],
            r.range(0.35, 0.7),
        );
    }
    noise_lp(
        &mut b,
        0.0,
        0.5,
        0.85,
        142,
        |t| 400.0 + 2200.0 * decay(t, 0.05),
        |t| ad(t, 0.001, 0.06),
    );
    thump(&mut b, 0.0, 55.0, 110.0, 0.03, 0.12, 1.0);
    crackle(&mut b, 0.04, 0.5, 30, 0.7, 2400.0, 143);
    b.saturate(1.5);
    b.finish(0.9)
}

/// Placement rejected: a soft falling two-note blip.
pub fn rejected() -> Vec<f32> {
    let mut b = Buffer::new(0.2);
    for (start, freq) in [(0.0, 330.0), (0.085, 247.0)] {
        let mut o = Osc::default();
        let mut f = Svf::default();
        b.add(start, 0.08, 0.8, |t| {
            let square = o.sine(freq).signum();
            f.low(square, 1400.0) * attack(t, 0.004) * decay(t, 0.03)
        });
    }
    b.finish(0.6)
}

// ---------------------------------------------------------------------------
// Movement
// ---------------------------------------------------------------------------

pub fn footstep() -> Vec<f32> {
    let mut b = Buffer::new(0.14);
    noise_lp(
        &mut b,
        0.0,
        0.14,
        0.9,
        151,
        |_| 650.0,
        |t| ad(t, 0.002, 0.028),
    );
    thump(&mut b, 0.0, 80.0, 110.0, 0.01, 0.03, 0.6);
    let mut n = Noise::new(152);
    let mut hp = Svf::default();
    b.add(0.0, 0.05, 0.12, |t| {
        hp.high(n.signed(), 3200.0) * ad(t, 0.001, 0.01)
    });
    b.finish(0.8)
}

pub fn jump() -> Vec<f32> {
    let mut b = Buffer::new(0.24);
    noise_lp(
        &mut b,
        0.0,
        0.1,
        0.6,
        161,
        |_| 600.0,
        |t| ad(t, 0.002, 0.02),
    );
    noise_bp(
        &mut b,
        0.0,
        0.22,
        0.6,
        162,
        0.8,
        |t| 500.0 + 4500.0 * t,
        |t| ad(t, 0.03, 0.06),
    );
    b.finish(0.75)
}

pub fn land() -> Vec<f32> {
    let mut b = Buffer::new(0.3);
    noise_lp(
        &mut b,
        0.0,
        0.25,
        1.0,
        171,
        |_| 520.0,
        |t| ad(t, 0.002, 0.05),
    );
    thump(&mut b, 0.0, 62.0, 95.0, 0.015, 0.07, 1.0);
    let mut n = Noise::new(172);
    let mut hp = Svf::default();
    b.add(0.0, 0.1, 0.18, |t| {
        hp.high(n.signed(), 2600.0) * ad(t, 0.001, 0.02)
    });
    b.saturate(1.3);
    b.finish(0.85)
}

pub fn slide() -> Vec<f32> {
    let mut b = Buffer::new(0.75);
    noise_bp(
        &mut b,
        0.0,
        0.75,
        0.9,
        181,
        0.8,
        |t| 2100.0 - 1500.0 * (t / 0.6).min(1.0),
        |t| ad(t, 0.03, 0.28),
    );
    crackle(&mut b, 0.0, 0.5, 40, 0.25, 3000.0, 182);
    b.finish(0.75)
}
