//! Sound descriptors measured from rendered audio: how fast a sound starts,
//! how long it rings, how bright and how noisy it is.
//!
//! These are what instrument patches are generated against: each role in
//! `assets/music/instruments.ron` gives a target range per descriptor, and
//! the search keeps the candidate that lands inside them.

use serde::{Deserialize, Serialize};
use std::f32::consts::PI;

/// How the descriptors are measured. Read from `instruments.ron`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct AnalysisParams {
    /// Seconds per envelope frame.
    pub frame: f32,
    /// Share of the peak that marks the onset, and the end of the attack.
    pub onset_level: f32,
    pub attack_level: f32,
    /// dB below the peak at which the sound counts as decayed.
    pub decay_db: f32,
    /// Spectrum window in samples (a power of two), taken from the peak.
    pub window: usize,
    /// Strongest spectrum bins counted as tonal; the share of power outside
    /// them is the noisiness.
    pub tonal_bins: usize,
}

impl Default for AnalysisParams {
    fn default() -> Self {
        Self {
            frame: 0.001,
            onset_level: 0.05,
            attack_level: 0.9,
            decay_db: -30.0,
            window: 4096,
            tonal_bins: 32,
        }
    }
}

/// What a sound measures as.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Descriptors {
    /// Seconds from onset to near peak.
    pub attack: f32,
    /// Seconds from peak until it stays below `decay_db`.
    pub decay: f32,
    /// Power-weighted spectral centroid in Hz: the brightness.
    pub centroid: f32,
    /// Share of power outside the strongest `tonal_bins` bins: 0 for a few
    /// pure partials, near 1 for noise, however it is filtered.
    pub noisiness: f32,
}

/// Measures `samples` at `rate`.
pub fn describe(samples: &[f32], rate: u32, p: &AnalysisParams) -> Descriptors {
    let hop = ((p.frame * rate as f32) as usize).max(1);
    let env: Vec<f32> = samples
        .chunks(hop)
        .map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt())
        .collect();
    let Some((peak_i, &peak)) = env.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)) else {
        return Descriptors::default();
    };
    if peak <= 0.0 {
        return Descriptors::default();
    }
    let secs = |frames: usize| (frames * hop) as f32 / rate as f32;
    let onset = env
        .iter()
        .position(|&e| e >= peak * p.onset_level)
        .unwrap_or(0);
    let attacked = env
        .iter()
        .position(|&e| e >= peak * p.attack_level)
        .unwrap_or(peak_i);
    let floor = peak * 10f32.powf(p.decay_db / 20.0);
    let last_loud = env.iter().rposition(|&e| e >= floor).unwrap_or(peak_i);

    let (centroid, noisiness) = spectrum(samples, peak_i * hop, rate, p);
    Descriptors {
        attack: secs(attacked.saturating_sub(onset)),
        decay: secs(last_loud.saturating_sub(peak_i)),
        centroid,
        noisiness,
    }
}

/// Centroid and noisiness of a Hann-windowed spectrum starting at `from`.
fn spectrum(samples: &[f32], from: usize, rate: u32, p: &AnalysisParams) -> (f32, f32) {
    let n = p.window.next_power_of_two().max(64);
    let mut re: Vec<f32> = (0..n)
        .map(|i| {
            let s = samples.get(from + i).copied().unwrap_or(0.0);
            s * 0.5 * (1.0 - (2.0 * PI * i as f32 / n as f32).cos())
        })
        .collect();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    let power: Vec<f64> = (1..n / 2)
        .map(|k| (re[k] as f64).powi(2) + (im[k] as f64).powi(2))
        .collect();
    let total: f64 = power.iter().sum();
    if total <= 1e-20 {
        return (0.0, 0.0);
    }
    let bin = rate as f64 / n as f64;
    // Power-weighted, so window leakage far from the partials barely counts.
    let centroid = power
        .iter()
        .enumerate()
        .map(|(k, p)| (k + 1) as f64 * bin * p)
        .sum::<f64>()
        / total;
    let mut sorted = power.clone();
    sorted.sort_unstable_by(|a, b| b.total_cmp(a));
    let tonal: f64 = sorted.iter().take(p.tonal_bins).sum();
    (
        centroid as f32,
        (1.0 - tonal / total).clamp(0.0, 1.0) as f32,
    )
}

/// In-place iterative radix-2 FFT. `re.len()` must be a power of two.
pub fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    debug_assert!(n.is_power_of_two() && im.len() == n);
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -2.0 * std::f64::consts::PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (angle * k as f64).sin_cos();
                let (c, s) = (c as f32, s as f32);
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * c - im[b] * s;
                let ti = re[b] * s + im[b] * c;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}
