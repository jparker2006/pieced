//! Integrated loudness (ITU-R BS.1770-4, LUFS): K-weighting, 400 ms blocks
//! with 75% overlap, the absolute (−70 LUFS) and relative (−10 LU) gates.
//!
//! The music files are normalized by `scripts/build-music.sh` with ffmpeg's
//! meter; the synthesized break loop is normalized with this one at load, so
//! every slot plays at the same loudness, and the tests check both.

/// A biquad (direct form I).
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Biquad {
    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// The two K-weighting stages for `rate` (the BS.1770 prefilters, derived for
/// any sample rate).
fn k_weighting(rate: f64) -> [Biquad; 2] {
    use std::f64::consts::PI;
    // Stage 1: a high shelf (+4 dB above ~1.7 kHz, the head's acoustics).
    let (g, f0, q) = (
        3.999_843_853_973_347,
        1_681.974_450_955_533,
        0.707_175_236_955_419_6,
    );
    let k = (PI * f0 / rate).tan();
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.499_666_774_154_541_6);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Biquad {
        b: [
            (vh + vb * k / q + k * k) / a0,
            2.0 * (k * k - vh) / a0,
            (vh - vb * k / q + k * k) / a0,
        ],
        a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        x: [0.0; 2],
        y: [0.0; 2],
    };
    // Stage 2: the RLB high-pass (~38 Hz).
    let (f0, q) = (38.135_470_876_024_44, 0.500_327_037_323_877_3);
    let k = (PI * f0 / rate).tan();
    let a0 = 1.0 + k / q + k * k;
    let high = Biquad {
        b: [1.0, -2.0, 1.0],
        a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        x: [0.0; 2],
        y: [0.0; 2],
    };
    [shelf, high]
}

/// Integrated loudness (LUFS) of interleaved samples with `channels` channels
/// at `rate` Hz (mono or stereo; every channel weighs 1). −70 or lower for
/// silence or audio shorter than one 400 ms block.
pub fn integrated_lufs(samples: &[f32], channels: usize, rate: u32) -> f32 {
    let channels = channels.max(1);
    let frames = samples.len() / channels;
    let step = (rate as usize / 10).max(1); // 100 ms
    let block = step * 4; // 400 ms
    if frames < block {
        return -70.0;
    }
    // Squared K-weighted samples summed over channels, per 100 ms step.
    let mut filters: Vec<[Biquad; 2]> = (0..channels).map(|_| k_weighting(rate as f64)).collect();
    let mut steps = vec![0.0f64; frames / step];
    for (f, frame) in samples.chunks_exact(channels).enumerate() {
        let Some(slot) = steps.get_mut(f / step) else {
            break;
        };
        for (c, &s) in frame.iter().enumerate() {
            let [shelf, high] = &mut filters[c];
            let y = high.process(shelf.process(s as f64));
            *slot += y * y;
        }
    }
    let blocks: Vec<f64> = steps
        .windows(4)
        .map(|w| w.iter().sum::<f64>() / block as f64)
        .collect();
    let loudness = |z: f64| -0.691 + 10.0 * z.max(1e-20).log10();
    let above: Vec<f64> = blocks
        .iter()
        .copied()
        .filter(|&z| loudness(z) > -70.0)
        .collect();
    if above.is_empty() {
        return -70.0;
    }
    let relative = loudness(above.iter().sum::<f64>() / above.len() as f64) - 10.0;
    let gated: Vec<f64> = above
        .into_iter()
        .filter(|&z| loudness(z) > relative)
        .collect();
    loudness(gated.iter().sum::<f64>() / gated.len().max(1) as f64) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_scale_1k_sine_reads_minus_3_lufs_per_channel_pair() {
        // BS.1770: a 0 dBFS 1 kHz sine in one channel reads −3.01 LUFS; in both
        // channels, 0 LUFS.
        let rate = 48_000;
        let mono: Vec<f32> = (0..rate * 2)
            .map(|i| (std::f32::consts::TAU * 1000.0 * i as f32 / rate as f32).sin())
            .collect();
        let l = integrated_lufs(&mono, 1, rate as u32);
        assert!((l + 3.01).abs() < 0.1, "{l}");
        let stereo: Vec<f32> = mono.iter().flat_map(|&s| [s, s]).collect();
        let l = integrated_lufs(&stereo, 2, rate as u32);
        assert!(l.abs() < 0.1, "{l}");
        assert_eq!(integrated_lufs(&vec![0.0; 96_000], 2, 48_000), -70.0);
    }
}
