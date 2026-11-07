//! Instrument patches, generated instead of handwritten.
//!
//! Each instrument in `assets/music/instruments.ron` gives a *genome*: for
//! every synth layer, a range for each parameter that matters (left out
//! means the synth default), plus *targets*: the range each sound
//! descriptor (attack, decay, brightness, noisiness) must land in. The
//! search samples candidates from the genome with a seeded generator,
//! measures each one rendered at the instrument's root, and keeps the
//! candidate that best fits the targets. The result is written to
//! `assets/music/patches.ron` by `cargo run --release -p tarborn_music --bin
//! patches`.
//!
//! Ranges with a positive minimum are sampled evenly in log space (so a
//! 0.01..1 s decay is as likely to be short as long); ranges starting at 0
//! or below are sampled evenly.

use crate::analysis::{AnalysisParams, Descriptors, describe};
use crate::render::{Patch, PatchBank};
use crate::score::midi_to_hz;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tarborn_sfx::{Layer, Sfx, Wave};

/// A parameter range, inclusive.
pub type Span = [f32; 2];

/// Ranges for one synth layer. Frequencies are ratios: `freq` and the
/// filters are relative to the instrument's root pitch, `freq_end` and the
/// filter ends relative to their start.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct LayerGenes {
    /// Waves to choose from; empty means sine.
    pub waves: Vec<Wave>,
    pub gain: Option<Span>,
    pub delay: Option<Span>,
    pub attack: Option<Span>,
    pub hold: Option<Span>,
    pub decay: Option<Span>,
    pub decay_curve: Option<Span>,
    pub punch: Option<Span>,
    pub freq: Option<Span>,
    pub freq_end: Option<Span>,
    pub slide_time: Option<Span>,
    pub vibrato_depth: Option<Span>,
    pub vibrato_rate: Option<Span>,
    pub duty: Option<Span>,
    pub duty_end: Option<Span>,
    pub lowpass: Option<Span>,
    pub lowpass_end: Option<Span>,
    pub highpass: Option<Span>,
    pub highpass_end: Option<Span>,
    pub resonance: Option<Span>,
    pub drive: Option<Span>,
}

/// Where each descriptor must land.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Targets {
    pub attack: Span,
    pub decay: Span,
    pub centroid: Span,
    pub noisiness: Span,
}

/// One instrument to generate.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Instrument {
    /// What it is for, in words (bass, ride, player kit accent...).
    pub role: String,
    pub pitched: bool,
    /// MIDI pitch the genome's ratios are relative to and the patch is
    /// written at.
    pub root: f32,
    pub filter_tracking: f32,
    pub gate: bool,
    pub gain: f32,
    /// Seconds a gated instrument is held when measured.
    pub measure_length: f32,
    pub layers: Vec<LayerGenes>,
    pub targets: Targets,
}

impl Default for Instrument {
    fn default() -> Self {
        Self {
            role: String::new(),
            pitched: true,
            root: 69.0,
            filter_tracking: 1.0,
            gate: false,
            gain: 1.0,
            measure_length: 0.25,
            layers: Vec::new(),
            targets: Targets {
                attack: [0.0, 1.0],
                decay: [0.0, 10.0],
                centroid: [20.0, 20_000.0],
                noisiness: [0.0, 1.0],
            },
        }
    }
}

/// How hard the search looks.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct SearchParams {
    pub seed: u32,
    /// Candidates tried per instrument.
    pub candidates: u32,
    /// Weight of being near the middle of each target range, against being
    /// inside it at all (which always wins).
    pub centre_weight: f32,
    /// Seconds added to times before taking logs, so 0 s attacks compare.
    pub time_floor: f32,
    pub sample_rate: u32,
}

impl Default for SearchParams {
    fn default() -> Self {
        Self {
            seed: 1,
            candidates: 200,
            centre_weight: 0.05,
            time_floor: 0.001,
            sample_rate: tarborn_sfx::SAMPLE_RATE,
        }
    }
}

/// The genome file, `assets/music/instruments.ron`.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct InstrumentFile {
    pub analysis: AnalysisParams,
    pub search: SearchParams,
    pub instruments: BTreeMap<String, Instrument>,
}

/// The winning candidate for one instrument.
#[derive(Clone, Debug)]
pub struct Generated {
    pub patch: Patch,
    pub descriptors: Descriptors,
    /// How far outside the targets it lands; 0 is inside all of them.
    pub miss: f32,
    pub score: f32,
}

/// Small deterministic PRNG (xorshift32).
struct Rng(u32);

impl Rng {
    fn new(seed: u32) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B | 1)
    }

    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    fn span(&mut self, [lo, hi]: Span) -> f32 {
        let u = self.unit();
        if lo > 0.0 && hi > lo {
            lo * (hi / lo).powf(u)
        } else {
            lo + (hi - lo) * u
        }
    }
}

/// Draws one candidate sound from `instrument`'s genome.
pub fn sample(instrument: &Instrument, rng_seed: u32) -> Sfx {
    let mut rng = Rng::new(rng_seed);
    let root_hz = midi_to_hz(instrument.root, 440.0);
    let layers = instrument
        .layers
        .iter()
        .map(|g| {
            let mut l = Layer::default();
            // Draw every range, used or not, so editing one doesn't reshuffle
            // the others.
            let mut draw = |span: Option<Span>, default: f32| {
                let v = rng.span(span.unwrap_or([0.0, 0.0]));
                if span.is_some() { v } else { default }
            };
            let wave_pick = draw(Some([0.0, 1.0]), 0.0);
            l.gain = draw(g.gain, l.gain);
            l.delay = draw(g.delay, l.delay);
            l.attack = draw(g.attack, l.attack);
            l.hold = draw(g.hold, l.hold);
            l.decay = draw(g.decay, l.decay);
            l.decay_curve = draw(g.decay_curve, l.decay_curve);
            l.punch = draw(g.punch, l.punch);
            l.freq = draw(g.freq, 1.0) * root_hz;
            l.freq_end = g.freq_end.map_or(0.0, |_| l.freq) * draw(g.freq_end, 1.0);
            l.slide_time = draw(g.slide_time, l.slide_time);
            l.vibrato_depth = draw(g.vibrato_depth, l.vibrato_depth);
            l.vibrato_rate = draw(g.vibrato_rate, l.vibrato_rate);
            l.duty = draw(g.duty, l.duty);
            l.duty_end = if g.duty_end.is_some() {
                l.duty * draw(g.duty_end, 1.0)
            } else {
                draw(None, l.duty_end)
            };
            l.lowpass = g.lowpass.map_or(0.0, |_| root_hz) * draw(g.lowpass, 1.0);
            l.lowpass_end = if g.lowpass_end.is_some() {
                l.lowpass * draw(g.lowpass_end, 1.0)
            } else {
                draw(None, l.lowpass_end)
            };
            l.highpass = g.highpass.map_or(0.0, |_| root_hz) * draw(g.highpass, 1.0);
            l.highpass_end = if g.highpass_end.is_some() {
                l.highpass * draw(g.highpass_end, 1.0)
            } else {
                draw(None, l.highpass_end)
            };
            l.resonance = draw(g.resonance, l.resonance);
            l.drive = draw(g.drive, l.drive);
            l.seed = rng.next_u32();
            l.wave = if g.waves.is_empty() {
                Wave::Sine
            } else {
                let i = ((wave_pick * g.waves.len() as f32) as usize).min(g.waves.len() - 1);
                g.waves[i]
            };
            l
        })
        .collect();
    Sfx {
        layers,
        ..Sfx::default()
    }
}

impl Instrument {
    /// The patch for a candidate sound.
    pub fn patch(&self, sound: Sfx) -> Patch {
        Patch {
            sound,
            pitched: self.pitched,
            root: self.root,
            filter_tracking: self.filter_tracking,
            gate: self.gate,
            gain: self.gain,
        }
    }

    /// Measures a patch as the search does: one note at the root.
    pub fn measure(&self, patch: &Patch, analysis: &AnalysisParams, rate: u32) -> Descriptors {
        let samples = patch
            .voiced(self.root, self.measure_length, 440.0)
            .render(rate);
        describe(&samples, rate, analysis)
    }

    /// How far `d` lands outside the targets (0 is inside all), and how far
    /// from their middles, each in widths of the range.
    pub fn fit(&self, d: &Descriptors, floor: f32) -> (f32, f32) {
        let t = &self.targets;
        let log = |[lo, hi]: Span, v: f32, floor: f32| {
            let f = |x: f32| (x.max(0.0) + floor).ln();
            (f(lo), f(hi), f(v))
        };
        let parts = [
            log(t.attack, d.attack, floor),
            log(t.decay, d.decay, floor),
            log(t.centroid, d.centroid, 1.0),
            (t.noisiness[0], t.noisiness[1], d.noisiness),
        ];
        let (mut miss, mut off_centre) = (0.0, 0.0);
        for (lo, hi, v) in parts {
            let width = (hi - lo).abs().max(1e-3);
            miss += ((lo - v).max(0.0) + (v - hi).max(0.0)) / width;
            off_centre += (v - (lo + hi) / 2.0).abs() / width;
        }
        (miss, off_centre)
    }
}

/// Searches for the best patch for one instrument.
pub fn generate(
    instrument: &Instrument,
    analysis: &AnalysisParams,
    search: &SearchParams,
    seed: u32,
) -> Generated {
    let mut best: Option<Generated> = None;
    let mut seeds = Rng::new(seed);
    for _ in 0..search.candidates.max(1) {
        let patch = instrument.patch(sample(instrument, seeds.next_u32()));
        let descriptors = instrument.measure(&patch, analysis, search.sample_rate);
        let (miss, off_centre) = instrument.fit(&descriptors, search.time_floor);
        let score = miss + search.centre_weight * off_centre;
        if best.as_ref().is_none_or(|b| score < b.score) {
            best = Some(Generated {
                patch,
                descriptors,
                miss,
                score,
            });
        }
    }
    best.expect("at least one candidate")
}

/// Generates every instrument in the file. Each instrument's seed derives
/// from the file seed and its name, so adding one doesn't change the rest.
pub fn generate_all(file: &InstrumentFile) -> BTreeMap<String, Generated> {
    file.instruments
        .iter()
        .map(|(name, instrument)| {
            let seed = name.bytes().fold(file.search.seed ^ 0x811C_9DC5, |h, b| {
                (h ^ b as u32).wrapping_mul(0x0100_0193)
            });
            let generated = generate(instrument, &file.analysis, &file.search, seed);
            (name.clone(), generated)
        })
        .collect()
}

/// The generated patches as a bank.
pub fn bank(generated: &BTreeMap<String, Generated>) -> PatchBank {
    PatchBank {
        patches: generated
            .iter()
            .map(|(name, g)| (name.clone(), g.patch.clone()))
            .collect(),
    }
}
