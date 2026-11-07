//! Quality metrics for generated fragments. Each measure is a plain number
//! computed from the notes; a style's targets turn them into a score.
//!
//! - **Tension**: Chew's spiral array. Notes sit on a helix along the line
//!   of fifths; a window's notes form a cloud, and its tension follows
//!   Herremans and Chew: the cloud's diameter, its distance from the
//!   chord-scale's centre (tensile strain), and how far it moved since the
//!   last window (momentum).
//! - **Dissonance**: the roughness of every pair of simultaneous pitched
//!   notes, from an interval-class table, weighted by how long they overlap.
//! - **Voice leading**: the mean cost (semitones moved) between consecutive
//!   comp voicings.
//! - **Syncopation**: Longuet-Higgins and Lee: an onset on a weak eighth
//!   with no onset on the next stronger position.
//! - **Repetition**: the share of melodic n-grams (interval and rhythm)
//!   heard before in the fragment: 0 wanders, 1 is a loop.
//! - **Density**: distinct onsets per beat.
//!
//! Every weight, table and target is a parameter: the method in `music.ron`
//! (`metrics`), the targets in each style file (`targets`).

use crate::score::{Fragment, Note, Role};
use crate::style::{PerLayer, Style, at};
use crate::theory::{Pc, voice_leading_cost};
use serde::{Deserialize, Serialize};

/// How the measures are computed and weighed. The `metrics` section of
/// `music.ron`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct MetricParams {
    /// Roughness of each interval class, 0 (unison) to 11 semitones.
    pub dissonance: [f32; 12],
    /// Spiral array rise per step along the line of fifths (radius is 1).
    pub spiral_rise: f32,
    /// Window length in beats for the tension curve.
    pub window: f32,
    /// Weights of cloud diameter, tensile strain and momentum in tension.
    pub tension_mix: [f32; 3],
    /// Notes per n-gram for repetition.
    pub ngram: usize,
    /// How far outside a target range (in range widths) a measure scores 0.
    pub softness: f32,
    /// Weights of each measure in the score.
    pub weights: Weights,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Weights {
    pub tension: f32,
    pub dissonance: f32,
    pub voice_leading: f32,
    pub syncopation: f32,
    pub repetition: f32,
    pub density: f32,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            tension: 1.0,
            dissonance: 1.0,
            voice_leading: 1.0,
            syncopation: 1.0,
            repetition: 1.0,
            density: 1.0,
        }
    }
}

impl Default for MetricParams {
    fn default() -> Self {
        Self {
            // Roughly after Huron's interval-class consonance ranks.
            dissonance: [
                0.0, 1.0, 0.45, 0.15, 0.1, 0.05, 0.6, 0.0, 0.1, 0.1, 0.35, 0.8,
            ],
            spiral_rise: (2.0f32 / 15.0).sqrt(),
            window: 1.0,
            tension_mix: [1.0, 1.0, 0.5],
            ngram: 3,
            softness: 1.0,
            weights: Weights::default(),
        }
    }
}

/// Where a style wants each measure, per role and intensity layer.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Targets {
    pub bass: RoleTargets,
    pub comp: RoleTargets,
    pub drums: RoleTargets,
    pub lead: RoleTargets,
}

/// Target ranges for one role; a measure left empty isn't scored.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct RoleTargets {
    pub tension: PerLayer<[f32; 2]>,
    pub dissonance: PerLayer<[f32; 2]>,
    pub voice_leading: PerLayer<[f32; 2]>,
    pub syncopation: PerLayer<[f32; 2]>,
    pub repetition: PerLayer<[f32; 2]>,
    pub density: PerLayer<[f32; 2]>,
}

impl Targets {
    pub fn role(&self, role: Role) -> Option<&RoleTargets> {
        match role {
            Role::Bass => Some(&self.bass),
            Role::Comp => Some(&self.comp),
            Role::Drums => Some(&self.drums),
            Role::Lead => Some(&self.lead),
            Role::Stinger => None,
        }
    }
}

/// The raw measures of one fragment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Measures {
    pub tension: f32,
    pub dissonance: f32,
    pub voice_leading: f32,
    pub syncopation: f32,
    pub repetition: f32,
    pub density: f32,
}

impl Measures {
    /// The measures as a feature vector, in a fixed order.
    pub fn vector(&self) -> [f32; 6] {
        [
            self.tension,
            self.dissonance,
            self.voice_leading,
            self.syncopation,
            self.repetition,
            self.density,
        ]
    }
}

/// Position on the spiral array of a pitch class.
fn spiral(pc: Pc, rise: f32) -> [f32; 3] {
    // Steps along the line of fifths, centred on C: F is -1, G is 1.
    let k = ((pc.index() as i32 * 7) % 12 + 5).rem_euclid(12) - 5;
    let a = k as f32 * std::f32::consts::FRAC_PI_2;
    [a.sin(), a.cos(), k as f32 * rise]
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn centre(points: &[([f32; 3], f32)]) -> [f32; 3] {
    let total: f32 = points.iter().map(|p| p.1).sum::<f32>().max(1e-6);
    let mut c = [0.0; 3];
    for (p, w) in points {
        for i in 0..3 {
            c[i] += p[i] * w / total;
        }
    }
    c
}

fn pitched<'a>(f: &'a Fragment, style: &'a Style) -> impl Iterator<Item = &'a Note> {
    let d = &style.drums;
    f.notes
        .iter()
        .filter(move |n| n.patch != d.ride && n.patch != d.hat && n.patch != d.brushes)
}

/// Mean tension over the fragment's windows.
pub fn tension(f: &Fragment, style: &Style, p: &MetricParams) -> f32 {
    let window = p.window.max(0.25);
    let windows = (f.beats() / window).ceil() as usize;
    let mut last: Option<[f32; 3]> = None;
    let (mut sum, mut count) = (0.0, 0);
    for w in 0..windows {
        let (t0, t1) = (w as f32 * window, (w + 1) as f32 * window);
        let cloud: Vec<([f32; 3], f32)> = pitched(f, style)
            .filter(|n| n.start < t1 && n.start + n.length > t0)
            .map(|n| {
                let overlap = (n.start + n.length).min(t1) - n.start.max(t0);
                (spiral(Pc::of(n.pitch), p.spiral_rise), overlap * n.velocity)
            })
            .collect();
        if cloud.is_empty() {
            continue;
        }
        let c = centre(&cloud);
        let diameter = cloud
            .iter()
            .flat_map(|a| cloud.iter().map(move |b| dist(a.0, b.0)))
            .fold(0.0, f32::max);
        let strain = f.chord_at(t0).map_or(0.0, |span| {
            let scale: Vec<([f32; 3], f32)> = span
                .scale()
                .pcs()
                .into_iter()
                .map(|pc| (spiral(pc, p.spiral_rise), 1.0))
                .collect();
            dist(c, centre(&scale))
        });
        let momentum = last.map_or(0.0, |l| dist(l, c));
        last = Some(c);
        let [a, b, m] = p.tension_mix;
        sum += a * diameter + b * strain + m * momentum;
        count += 1;
    }
    if count == 0 { 0.0 } else { sum / count as f32 }
}

/// Duration-weighted roughness of simultaneous pitched notes.
pub fn dissonance(f: &Fragment, style: &Style, p: &MetricParams) -> f32 {
    let notes: Vec<&Note> = pitched(f, style).collect();
    let (mut rough, mut time) = (0.0, 0.0);
    for (i, a) in notes.iter().enumerate() {
        for b in &notes[i + 1..] {
            let overlap = (a.start + a.length).min(b.start + b.length) - a.start.max(b.start);
            if overlap <= 0.0 {
                continue;
            }
            let ic = (a.pitch as i32 - b.pitch as i32).unsigned_abs() as usize % 12;
            rough += p.dissonance[ic] * overlap;
            time += overlap;
        }
    }
    if time > 0.0 { rough / time } else { 0.0 }
}

/// Mean voice-leading cost between consecutive chords (notes struck
/// together on one patch).
pub fn voice_leading(f: &Fragment, style: &Style) -> f32 {
    let mut chords: Vec<(f32, &str, Vec<u8>)> = Vec::new();
    for n in pitched(f, style).filter(|n| n.patch != style.bass.patch) {
        match chords.iter_mut().find(|c| c.0 == n.start && c.1 == n.patch) {
            Some(c) => c.2.push(n.pitch),
            None => chords.push((n.start, &n.patch, vec![n.pitch])),
        }
    }
    let first = chords.first().map(|c| c.1);
    let mut voicings: Vec<(f32, Vec<u8>)> = chords
        .into_iter()
        .filter(|c| Some(c.1) == first && c.2.len() > 1)
        .map(|(t, _, mut v)| {
            v.sort_unstable();
            (t, v)
        })
        .collect();
    voicings.sort_by(|a, b| a.0.total_cmp(&b.0));
    let costs: Vec<u32> = voicings
        .windows(2)
        .map(|w| voice_leading_cost(&w[0].1, &w[1].1))
        .collect();
    if costs.is_empty() {
        0.0
    } else {
        costs.iter().sum::<u32>() as f32 / costs.len() as f32
    }
}

/// Longuet-Higgins and Lee syncopation per bar, over an eighth-note grid.
pub fn syncopation(f: &Fragment) -> f32 {
    let slots = (f.beats_per_bar * 2) as usize;
    // Metrical weight of each eighth: the downbeat heaviest, then the
    // half bar, the beats, the offbeats.
    let weight = |s: usize| -> i32 {
        if s == 0 {
            0
        } else if s * 2 == slots {
            -1
        } else if s.is_multiple_of(2) {
            -2
        } else {
            -3
        }
    };
    let mut total = 0;
    for bar in 0..f.bars as usize {
        let mut onset = vec![false; slots];
        for n in &f.notes {
            let s = (n.start * 2.0).round() as usize;
            if s / slots == bar {
                onset[s % slots] = true;
            }
        }
        for s in 0..slots {
            if !onset[s] {
                continue;
            }
            // The next stronger position, if silent, is syncopated over.
            if let Some(next) = (s + 1..slots).find(|&t| weight(t) > weight(s))
                && !onset[next]
            {
                total += weight(next) - weight(s);
            }
        }
    }
    total as f32 / f.bars.max(1) as f32
}

/// The share of melodic n-grams already heard earlier in the fragment.
/// Only the line that plays most (the first patch) counts.
pub fn repetition(f: &Fragment, p: &MetricParams) -> f32 {
    let Some(first) = f.notes.first().map(|n| n.patch.as_str()) else {
        return 0.0;
    };
    let mut line: Vec<&Note> = f.notes.iter().filter(|n| n.patch == first).collect();
    line.sort_by(|a, b| a.start.total_cmp(&b.start));
    line.dedup_by(|b, a| a.start == b.start);
    let n = p.ngram.max(2);
    if line.len() <= n {
        return 0.0;
    }
    let grams: Vec<Vec<(i32, i32)>> = line
        .windows(n)
        .map(|w| {
            w.windows(2)
                .map(|p| {
                    (
                        p[1].pitch as i32 - p[0].pitch as i32,
                        ((p[1].start - p[0].start) * 2.0).round() as i32,
                    )
                })
                .collect()
        })
        .collect();
    let repeats = (1..grams.len())
        .filter(|&i| grams[..i].contains(&grams[i]))
        .count();
    repeats as f32 / (grams.len() - 1) as f32
}

/// Distinct onsets per beat.
pub fn density(f: &Fragment) -> f32 {
    let mut starts: Vec<i32> = f
        .notes
        .iter()
        .map(|n| (n.start * 2.0).round() as i32)
        .collect();
    starts.sort_unstable();
    starts.dedup();
    starts.len() as f32 / f.beats().max(1.0)
}

pub fn measure(f: &Fragment, style: &Style, p: &MetricParams) -> Measures {
    Measures {
        tension: tension(f, style, p),
        dissonance: dissonance(f, style, p),
        voice_leading: voice_leading(f, style),
        syncopation: syncopation(f),
        repetition: repetition(f, p),
        density: density(f),
    }
}

/// How well `value` sits in `[lo, hi]`: 1 inside, falling linearly to 0 at
/// `softness` range widths outside.
pub fn fit(value: f32, [lo, hi]: [f32; 2], softness: f32) -> f32 {
    let width = (hi - lo).abs().max(1e-3) * softness.max(1e-3);
    let out = (lo - value).max(0.0) + (value - hi).max(0.0);
    (1.0 - out / width).clamp(0.0, 1.0)
}

/// A score and its parts, each 0 (far off) to 1 (on target).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Score {
    pub total: f32,
    pub measures: Measures,
    pub fits: Measures,
}

/// Scores a fragment against its style's targets for its role and layer.
pub fn score(f: &Fragment, style: &Style, p: &MetricParams) -> Score {
    let m = measure(f, style, p);
    let Some(t) = style.targets.role(f.role) else {
        return Score {
            total: 1.0,
            measures: m,
            fits: Measures::default(),
        };
    };
    let w = &p.weights;
    let mut fits = Measures::default();
    let (mut sum, mut weight) = (0.0, 0.0);
    let mut add = |value: f32, target: &PerLayer<[f32; 2]>, w: f32, slot: &mut f32| {
        if target.is_empty() {
            *slot = 1.0;
            return;
        }
        let k = fit(value, at(target, f.layer), p.softness);
        *slot = k;
        sum += k * w;
        weight += w;
    };
    add(m.tension, &t.tension, w.tension, &mut fits.tension);
    add(
        m.dissonance,
        &t.dissonance,
        w.dissonance,
        &mut fits.dissonance,
    );
    add(
        m.voice_leading,
        &t.voice_leading,
        w.voice_leading,
        &mut fits.voice_leading,
    );
    add(
        m.syncopation,
        &t.syncopation,
        w.syncopation,
        &mut fits.syncopation,
    );
    add(
        m.repetition,
        &t.repetition,
        w.repetition,
        &mut fits.repetition,
    );
    add(m.density, &t.density, w.density, &mut fits.density);
    Score {
        total: if weight > 0.0 { sum / weight } else { 1.0 },
        measures: m,
        fits,
    }
}
