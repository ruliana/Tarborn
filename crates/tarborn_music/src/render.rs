//! Offline rendering: a fragment through instrument patches to samples.
//!
//! A [`Patch`] is a `tarborn_sfx` sound written at one pitch, plus how it
//! follows the notes it plays: it is retuned to each note, its filters can
//! track pitch, and sustained instruments can hold for the note's length.
//! Rendering is deterministic: the seed only drives humanisation (timing and
//! velocity jitter), so the same inputs and seed give identical samples.

use crate::score::{Fragment, midi_to_hz};
use crate::time::beats_to_seconds;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use tarborn_sfx::{Sfx, soft_limit};

/// One instrument.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Patch {
    /// The sound at the patch's root pitch.
    pub sound: Sfx,
    /// Whether notes retune the sound. Drums usually do not.
    pub pitched: bool,
    /// MIDI pitch the sound is written at.
    pub root: f32,
    /// How much filter cutoffs follow the note: 0 fixed, 1 fully with pitch.
    pub filter_tracking: f32,
    /// Hold each layer for the note's length (organ, bowed or held notes)
    /// instead of ringing out the written envelope (plucks, piano, drums).
    pub gate: bool,
    /// Linear gain of this instrument in the mix.
    pub gain: f32,
}

impl Default for Patch {
    fn default() -> Self {
        Self {
            sound: Sfx::default(),
            pitched: true,
            root: 69.0,
            filter_tracking: 1.0,
            gate: false,
            gain: 1.0,
        }
    }
}

impl Patch {
    /// The sound retuned to `pitch` and, for gated patches, held for
    /// `seconds`.
    pub fn voiced(&self, pitch: f32, seconds: f32, a4: f32) -> Sfx {
        let mut sfx = self.sound.clone();
        let ratio = if self.pitched {
            midi_to_hz(pitch, a4) / midi_to_hz(self.root, a4)
        } else {
            1.0
        };
        let filter_ratio = ratio.powf(self.filter_tracking);
        for layer in &mut sfx.layers {
            layer.freq *= ratio;
            if layer.freq_end > 0.0 {
                layer.freq_end *= ratio;
            }
            for cutoff in [
                &mut layer.lowpass,
                &mut layer.lowpass_end,
                &mut layer.highpass,
                &mut layer.highpass_end,
            ] {
                if *cutoff > 0.0 {
                    *cutoff *= filter_ratio;
                }
            }
            if self.gate {
                let held = seconds - layer.delay.max(0.0) - layer.attack.max(0.0);
                layer.hold = layer.hold.max(held);
            }
        }
        sfx
    }
}

/// Named instruments, as read from a patch file.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PatchBank {
    pub patches: BTreeMap<String, Patch>,
}

/// How a fragment is played. Read from `assets/music.ron`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct RenderSettings {
    pub sample_rate: u32,
    pub bpm: f32,
    /// Share of each beat taken by its first eighth note; 0.5 is straight.
    pub swing: f32,
    /// Tuning reference in Hz.
    pub a4: f32,
    /// Linear gain of the whole mix.
    pub master_volume: f32,
    /// Note gain is `velocity ^ velocity_curve`.
    pub velocity_curve: f32,
    /// Most a note may land early or late, in beats.
    pub timing_jitter: f32,
    /// Most a note's velocity may move, as a fraction either way.
    pub velocity_jitter: f32,
    /// Seconds rendered past the fragment's end so the last notes ring out.
    pub tail: f32,
    /// Soft-limit the mix instead of hard clipping.
    pub limit: bool,
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            sample_rate: tarborn_sfx::SAMPLE_RATE,
            bpm: 120.0,
            swing: 0.5,
            a4: 440.0,
            master_volume: 1.0,
            velocity_curve: 1.0,
            timing_jitter: 0.0,
            velocity_jitter: 0.0,
            tail: 0.0,
            limit: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderError {
    /// A note names a patch the bank does not have.
    UnknownPatch(String),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::UnknownPatch(name) => write!(f, "no patch named {name:?}"),
        }
    }
}

impl std::error::Error for RenderError {}

/// Renders `fragment` as mono samples in -1..=1.
pub fn render(
    fragment: &Fragment,
    bank: &PatchBank,
    settings: &RenderSettings,
    seed: u32,
) -> Result<Vec<f32>, RenderError> {
    let rate = settings.sample_rate.max(1);
    let to_secs = |beats: f32| beats_to_seconds(beats, settings.bpm, settings.swing);
    let total = to_secs(fragment.beats()) + settings.tail.max(0.0);
    let mut mix = vec![0.0f32; (total * rate as f32).ceil() as usize];
    let mut rng = Rng::new(seed);
    for note in &fragment.notes {
        let patch = bank
            .patches
            .get(&note.patch)
            .ok_or_else(|| RenderError::UnknownPatch(note.patch.clone()))?;
        // Draw both numbers for every note so one setting never shifts the
        // random stream of the other.
        let (r_time, r_vel) = (rng.bipolar(), rng.bipolar());
        let start_beats = (note.start + settings.timing_jitter * r_time).max(0.0);
        let start = to_secs(start_beats);
        let seconds = (to_secs(start_beats + note.length.max(0.0)) - start).max(0.0);
        let velocity = (note.velocity * (1.0 + settings.velocity_jitter * r_vel)).clamp(0.0, 1.0);
        let gain = velocity.powf(settings.velocity_curve.max(0.01)) * patch.gain;
        let samples = patch
            .voiced(note.pitch as f32, seconds, settings.a4)
            .render(rate);
        let offset = (start * rate as f32).round() as usize;
        for (m, s) in mix.iter_mut().skip(offset).zip(samples) {
            *m += s * gain;
        }
    }
    for m in &mut mix {
        let v = *m * settings.master_volume;
        *m = if settings.limit {
            soft_limit(v)
        } else {
            v.clamp(-1.0, 1.0)
        };
    }
    Ok(mix)
}

/// Small deterministic PRNG (xorshift32).
struct Rng(u32);

impl Rng {
    fn new(seed: u32) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9) | 1)
    }

    /// Uniform in -1..=1.
    fn bipolar(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}
