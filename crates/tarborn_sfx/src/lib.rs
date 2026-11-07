//! Procedural sound effects for Tarborn, built from numbers instead of
//! recordings.
//!
//! The model follows sfxr (DrPetter, 2007) and its descendants (jsfxr, bfxr,
//! ChipTone, rFXGen): every sound is a short synthesized voice described by a
//! handful of parameters (waveform, pitch slide, envelope, filters). We extend
//! that with **layers**: a sound is several voices mixed together, each with
//! its own delay, so one effect can stack a sharp transient, a pitched body and
//! a noisy tail. That layering is how impact sounds are usually built by hand,
//! and it is what a single sfxr voice cannot do.
//!
//! Everything here is plain Rust with no engine dependency. Rendering is
//! deterministic: the same [`Sfx`] always produces the same samples, so the
//! game renders each sound once and varies pitch and volume at playback.

pub mod wav;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::f32::consts::{PI, TAU};

/// The rate everything is rendered at.
pub const SAMPLE_RATE: u32 = 44_100;

/// The basic oscillator shape of a layer.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum Wave {
    #[default]
    Sine,
    Triangle,
    /// Pulse wave; `duty` sets the high fraction (0.5 is a square).
    Square,
    Saw,
    /// White noise, resampled at the layer's frequency like sfxr: high
    /// frequencies hiss, low ones crackle and rumble.
    Noise,
}

/// One synthesized voice. Fields left out of a sound file take these defaults.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Layer {
    pub wave: Wave,
    /// Linear gain of this layer in the mix.
    pub gain: f32,
    /// Seconds of silence before the layer starts.
    pub delay: f32,

    /// Envelope: seconds rising from 0 to full.
    pub attack: f32,
    /// Seconds held at full (plus `punch`).
    pub hold: f32,
    /// Seconds falling from full to 0.
    pub decay: f32,
    /// Shape of the fall: 1 is linear, above 1 drops fast then lingers
    /// (percussive), below 1 stays loud and cuts off late.
    pub decay_curve: f32,
    /// Extra gain at the start of the hold, fading to 0 by its end. sfxr's
    /// "punch": makes the front of a sound pop.
    pub punch: f32,

    /// Starting pitch in Hz (for noise, the resample rate).
    pub freq: f32,
    /// Pitch at the end of the slide. Equal to `freq` (or 0) means no slide.
    pub freq_end: f32,
    /// Seconds to glide from `freq` to `freq_end`, exponentially (so it sounds
    /// even in pitch). 0 means the whole layer.
    pub slide_time: f32,
    /// After this many seconds, jump pitch by `arp_ratio` (sfxr's "change",
    /// the classic coin-pickup step). 0 disables.
    pub arp_time: f32,
    pub arp_ratio: f32,
    /// Vibrato depth in semitones and rate in Hz.
    pub vibrato_depth: f32,
    pub vibrato_rate: f32,

    /// Pulse width for `Square`, at start and end of the layer.
    pub duty: f32,
    pub duty_end: f32,

    /// Low-pass cutoff in Hz at start and end (swept exponentially). 0 is off.
    pub lowpass: f32,
    pub lowpass_end: f32,
    /// High-pass cutoff in Hz at start and end. 0 is off.
    pub highpass: f32,
    pub highpass_end: f32,
    /// Filter resonance, 0 (flat) to just under 1 (whistling).
    pub resonance: f32,

    /// Soft-clipping drive. 0 is clean; higher adds grit and loudness.
    pub drive: f32,
    /// Random seed for `Noise`, so two noise layers can differ.
    pub seed: u32,
}

impl Default for Layer {
    fn default() -> Self {
        Self {
            wave: Wave::Sine,
            gain: 1.0,
            delay: 0.0,
            attack: 0.0,
            hold: 0.0,
            decay: 0.2,
            decay_curve: 1.0,
            punch: 0.0,
            freq: 440.0,
            freq_end: 0.0,
            slide_time: 0.0,
            arp_time: 0.0,
            arp_ratio: 1.0,
            vibrato_depth: 0.0,
            vibrato_rate: 0.0,
            duty: 0.5,
            duty_end: -1.0,
            lowpass: 0.0,
            lowpass_end: -1.0,
            highpass: 0.0,
            highpass_end: -1.0,
            resonance: 0.0,
            drive: 0.0,
            seed: 1,
        }
    }
}

impl Layer {
    /// Seconds from the layer's start (after its delay) to silence.
    pub fn length(&self) -> f32 {
        self.attack.max(0.0) + self.hold.max(0.0) + self.decay.max(0.0)
    }

    /// Envelope gain `t` seconds into the layer.
    pub fn envelope(&self, t: f32) -> f32 {
        let (a, h, d) = (
            self.attack.max(0.0),
            self.hold.max(0.0),
            self.decay.max(0.0),
        );
        if t < 0.0 {
            0.0
        } else if t < a {
            t / a
        } else if t < a + h {
            1.0 + self.punch * (1.0 - (t - a) / h)
        } else if t < a + h + d {
            let u = (t - a - h) / d;
            // With no hold, the punch rides on the start of the decay instead.
            let punch = if h <= 0.0 {
                self.punch * (1.0 - u)
            } else {
                0.0
            };
            (1.0 + punch) * (1.0 - u).powf(self.decay_curve.max(0.01))
        } else {
            0.0
        }
    }

    /// Instantaneous pitch in Hz `t` seconds into the layer.
    pub fn pitch(&self, t: f32) -> f32 {
        let mut f = self.freq.max(1.0);
        if self.freq_end > 0.0 && self.freq_end != self.freq {
            let span = if self.slide_time > 0.0 {
                self.slide_time
            } else {
                self.length().max(1e-3)
            };
            let u = (t / span).clamp(0.0, 1.0);
            f *= (self.freq_end.max(1.0) / f).powf(u);
        }
        if self.arp_time > 0.0 && t >= self.arp_time {
            f *= self.arp_ratio.max(0.01);
        }
        if self.vibrato_depth != 0.0 {
            let semis = self.vibrato_depth * (TAU * self.vibrato_rate * t).sin();
            f *= (semis / 12.0).exp2();
        }
        f
    }

    /// Renders this layer alone (without its delay) at `rate` samples per second.
    pub fn render(&self, rate: u32) -> Vec<f32> {
        let rate_f = rate as f32;
        let len = self.length();
        let n = (len * rate_f).ceil() as usize;
        let mut out = Vec::with_capacity(n);
        let mut phase = 0.0f32;
        let mut rng = Rng::new(self.seed);
        let mut held_noise = rng.bipolar();
        let mut low = Svf::default();
        let mut high = Svf::default();
        let drive_norm = if self.drive > 0.0 {
            (1.0 + self.drive).tanh()
        } else {
            1.0
        };
        for i in 0..n {
            let t = i as f32 / rate_f;
            let u = if len > 0.0 { t / len } else { 0.0 };
            let f = self.pitch(t);
            let duty = lerp(self.duty, end_or(self.duty_end, self.duty), u).clamp(0.01, 0.99);
            let sample = match self.wave {
                Wave::Sine => (TAU * phase).sin(),
                Wave::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
                Wave::Square => {
                    if phase < duty {
                        1.0
                    } else {
                        -1.0
                    }
                }
                Wave::Saw => 2.0 * phase - 1.0,
                Wave::Noise => held_noise,
            };
            phase += f / rate_f;
            while phase >= 1.0 {
                phase -= 1.0;
                held_noise = rng.bipolar();
            }
            let mut s = sample;
            if self.lowpass > 0.0 {
                let cutoff = exp_lerp(self.lowpass, end_or(self.lowpass_end, self.lowpass), u);
                s = low.process(s, cutoff, self.resonance, rate_f).low;
            }
            if self.highpass > 0.0 {
                let cutoff = exp_lerp(self.highpass, end_or(self.highpass_end, self.highpass), u);
                s = high.process(s, cutoff, self.resonance, rate_f).high;
            }
            if self.drive > 0.0 {
                s = (s * (1.0 + self.drive)).tanh() / drive_norm;
            }
            out.push(s * self.envelope(t) * self.gain);
        }
        out
    }
}

/// A complete sound effect: layers mixed together, plus how it varies each
/// time it plays.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Sfx {
    pub layers: Vec<Layer>,
    /// Linear gain of the whole sound.
    pub volume: f32,
    /// Soft limit on the mix, so stacked layers never clip harshly.
    pub limit: bool,

    // Playback, applied by the game each time the sound plays.
    /// Random pitch spread per play, in semitones either way. Keeps repeated
    /// sounds from feeling machine-gunned.
    pub pitch_jitter: f32,
    /// Random volume spread per play, as a fraction either way.
    pub volume_jitter: f32,
    /// Volume multiplier at intensity 0 and at intensity 1.
    pub intensity_volume: [f32; 2],
    /// Pitch multiplier at intensity 0 and at intensity 1. Heavier hits
    /// usually play lower.
    pub intensity_pitch: [f32; 2],
    /// Minimum seconds between two plays of this sound. Extra triggers inside
    /// the window are dropped.
    pub cooldown: f32,
    /// How many copies may sound at once. Extra triggers are dropped.
    pub max_voices: u32,
}

impl Default for Sfx {
    fn default() -> Self {
        Self {
            layers: Vec::new(),
            volume: 1.0,
            limit: true,
            pitch_jitter: 0.0,
            volume_jitter: 0.0,
            intensity_volume: [1.0, 1.0],
            intensity_pitch: [1.0, 1.0],
            cooldown: 0.0,
            max_voices: 4,
        }
    }
}

impl Sfx {
    /// Seconds until the last layer falls silent.
    pub fn length(&self) -> f32 {
        self.layers
            .iter()
            .map(|l| l.delay.max(0.0) + l.length())
            .fold(0.0, f32::max)
    }

    /// Renders the whole sound as mono samples in -1..=1.
    pub fn render(&self, rate: u32) -> Vec<f32> {
        let mut mix = self.mix(rate);
        for m in &mut mix {
            *m = if self.limit {
                soft_limit(*m)
            } else {
                m.clamp(-1.0, 1.0)
            };
        }
        mix
    }

    /// The layers mixed at `volume`, before limiting. Peaks above 1 here are
    /// what the limiter squashes.
    pub fn mix(&self, rate: u32) -> Vec<f32> {
        let n = (self.length() * rate as f32).ceil() as usize;
        let mut mix = vec![0.0f32; n];
        for layer in &self.layers {
            let start = (layer.delay.max(0.0) * rate as f32).round() as usize;
            for (i, s) in layer.render(rate).into_iter().enumerate() {
                if let Some(m) = mix.get_mut(start + i) {
                    *m += s * self.volume;
                }
            }
        }
        mix
    }

    /// Playback volume and pitch multipliers for one play at `intensity`
    /// (0..=1), given two uniform random numbers in 0..1.
    pub fn variation(&self, intensity: f32, r1: f32, r2: f32) -> (f32, f32) {
        let k = intensity.clamp(0.0, 1.0);
        let volume = lerp(self.intensity_volume[0], self.intensity_volume[1], k)
            * (1.0 + self.volume_jitter * (2.0 * r1 - 1.0));
        let semis = self.pitch_jitter * (2.0 * r2 - 1.0);
        let pitch =
            lerp(self.intensity_pitch[0], self.intensity_pitch[1], k) * (semis / 12.0).exp2();
        (volume.max(0.0), pitch.max(0.05))
    }
}

/// A named set of sounds, as read from a sound file. Other top-level fields
/// in the file (such as the game's trigger tuning) are ignored here.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Library {
    pub sounds: BTreeMap<String, Sfx>,
}

impl Library {
    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
            .from_str(text)
    }
}

/// Above ±0.6 the signal bends smoothly toward ±1 instead of clipping.
pub fn soft_limit(x: f32) -> f32 {
    const KNEE: f32 = 0.6;
    let a = x.abs();
    if a <= KNEE {
        x
    } else {
        let over = (a - KNEE) / (1.0 - KNEE);
        x.signum() * (KNEE + (1.0 - KNEE) * over.tanh())
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Interpolates evenly in pitch (log frequency) rather than in Hz.
fn exp_lerp(a: f32, b: f32, t: f32) -> f32 {
    let (a, b) = (a.max(1.0), b.max(1.0));
    a * (b / a).powf(t)
}

/// `end` when it is set (not negative), else `start`.
fn end_or(end: f32, start: f32) -> f32 {
    if end >= 0.0 { end } else { start }
}

/// Small deterministic PRNG (xorshift32) so noise is reproducible.
struct Rng(u32);

impl Rng {
    fn new(seed: u32) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9) | 1)
    }

    fn bipolar(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Zero-delay-feedback state variable filter (Simper/Cytomic). Stable at any
/// cutoff and resonance, so sweeps can't blow up.
#[derive(Default)]
struct Svf {
    ic1: f32,
    ic2: f32,
}

struct SvfOut {
    low: f32,
    high: f32,
}

impl Svf {
    fn process(&mut self, x: f32, cutoff: f32, resonance: f32, rate: f32) -> SvfOut {
        let fc = cutoff.clamp(10.0, rate * 0.49);
        let g = (PI * fc / rate).tan();
        let k = 2.0 - 2.0 * resonance.clamp(0.0, 0.98);
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
            high: x - k * v1 - v2,
        }
    }
}
