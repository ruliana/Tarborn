//! The offline search that fills the music library: quality-diversity
//! (MAP-Elites) over the grammar's proposals.
//!
//! Two archives per style:
//! - **Tunes**, binned by tempo and by form (the sections' lengths and the
//!   opening chord's quality). Each cell keeps the tune whose parts score
//!   best on average.
//! - **Parts**, per tune section, role and intensity layer, binned by
//!   density and syncopation. Each kept tune keeps up to `variants` parts
//!   from different cells, best first, so the director can vary a tune
//!   from run to run without leaving the library.
//!
//! Only proposals that break no rule and score at least `min_score` enter.
//! The grammar is deterministic, so the library stores seeds (the recipe)
//! plus the scores they earned; [`realize`] regenerates the notes and a
//! test checks every stored score still holds.

use crate::grammar::{Tune, part, stinger, tune, validate};
use crate::metrics::{MetricParams, score};
use crate::rng::Rng;
use crate::score::{Fragment, Role};
use crate::style::Style;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Search effort and archive shape. The `search` section of `music.ron`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct SearchParams {
    pub seed: u32,
    /// Tunes proposed per style.
    pub tune_candidates: u32,
    /// Tempo bins across the style's tempo range.
    pub tempo_bins: u32,
    /// Candidates per part when judging a tune, and when filling a kept one.
    pub quick_candidates: u32,
    pub part_candidates: u32,
    /// Density bins over `density_range` (onsets per beat), syncopation
    /// bins over `syncopation_range`.
    pub density_bins: u32,
    pub density_range: [f32; 2],
    pub syncopation_bins: u32,
    pub syncopation_range: [f32; 2],
    /// Most variants kept per part.
    pub variants: usize,
    /// Least score a proposal needs to enter.
    pub min_score: f32,
}

impl Default for SearchParams {
    fn default() -> Self {
        Self {
            seed: 1,
            tune_candidates: 120,
            tempo_bins: 3,
            quick_candidates: 4,
            part_candidates: 24,
            density_bins: 4,
            density_range: [0.0, 2.0],
            syncopation_bins: 3,
            syncopation_range: [0.0, 3.0],
            variants: 3,
            min_score: 0.8,
        }
    }
}

/// One kept part proposal.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Variant {
    pub seed: u32,
    pub score: f32,
    /// The archive cell it won: density bin, syncopation bin.
    pub cell: (u32, u32),
}

/// The kept variants of one section × role × layer.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LibraryPart {
    pub section: usize,
    pub role: Role,
    pub layer: u8,
    pub variants: Vec<Variant>,
}

/// One tune of the library.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LibraryTune {
    pub style: String,
    pub seed: u32,
    /// The archive cell it won: tempo bin and form.
    pub cell: (u32, String),
    pub bpm: f32,
    pub key: String,
    pub minor: bool,
    pub form: Vec<String>,
    /// Mean score of its best parts.
    pub score: f32,
    pub parts: Vec<LibraryPart>,
    pub stinger_seed: u32,
}

fn form_cell(t: &Tune) -> String {
    let bars: Vec<String> = t.sections.iter().map(|s| s.bars.to_string()).collect();
    let opening = t.sections[0].chords[0].chord.quality.suffix();
    format!("{}:{opening}", bars.join("+"))
}

fn bin(value: f32, [lo, hi]: [f32; 2], bins: u32) -> u32 {
    let u = ((value - lo) / (hi - lo).max(1e-6)).clamp(0.0, 0.9999);
    (u * bins.max(1) as f32) as u32
}

/// The seed a part proposal is generated from.
fn part_seed(tune_seed: u32, section: usize, role: Role, layer: u8, k: u32) -> u32 {
    let salt = (section as u32) << 24 | (role as u32) << 16 | (layer as u32) << 8 | k;
    Rng::new(tune_seed ^ salt.wrapping_mul(0x9E37_79B9)).next_u32()
}

/// The part roles the library holds.
pub const ROLES: [Role; 4] = [Role::Bass, Role::Comp, Role::Drums, Role::Lead];

/// What a part search works within: the style, the tune and its seed.
pub struct PartContext<'a> {
    pub style: &'a Style,
    pub tune: &'a Tune,
    pub tune_seed: u32,
    pub metrics: &'a MetricParams,
    pub params: &'a SearchParams,
}

/// Searches one part: up to `variants` passing proposals from different
/// density × syncopation cells, best first. None when the style rests in
/// this part (every proposal is silent, such as the lead in the calm layer).
pub fn search_part(
    cx: &PartContext,
    section: usize,
    role: Role,
    layer: u8,
    candidates: u32,
) -> Option<Vec<Variant>> {
    let PartContext {
        style,
        tune: t,
        tune_seed,
        metrics,
        params: p,
    } = *cx;
    let mut cells: BTreeMap<(u32, u32), Variant> = BTreeMap::new();
    let mut silent = true;
    for k in 0..candidates.max(1) {
        let seed = part_seed(tune_seed, section, role, layer, k);
        let f = part(style, t, section, role, layer, seed);
        if f.notes.is_empty() {
            continue;
        }
        silent = false;
        if !validate(style, &f).is_empty() {
            continue;
        }
        let s = score(&f, style, metrics);
        if s.total < p.min_score {
            continue;
        }
        let cell = (
            bin(s.measures.density, p.density_range, p.density_bins),
            bin(
                s.measures.syncopation,
                p.syncopation_range,
                p.syncopation_bins,
            ),
        );
        let better = cells.get(&cell).is_none_or(|v| s.total > v.score);
        if better {
            cells.insert(
                cell,
                Variant {
                    seed,
                    score: s.total,
                    cell,
                },
            );
        }
    }
    let mut kept: Vec<Variant> = cells.into_values().collect();
    kept.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.seed.cmp(&b.seed)));
    kept.truncate(p.variants.max(1));
    (!silent).then_some(kept)
}

/// Every part of a tune, searched with `candidates` proposals each. None if
/// some part has no passing proposal.
fn search_parts(
    style: &Style,
    t: &Tune,
    tune_seed: u32,
    candidates: u32,
    metrics: &MetricParams,
    p: &SearchParams,
) -> Option<(Vec<LibraryPart>, f32)> {
    let cx = PartContext {
        style,
        tune: t,
        tune_seed,
        metrics,
        params: p,
    };
    let mut parts = Vec::new();
    let (mut sum, mut n) = (0.0, 0);
    for section in 0..t.sections.len() {
        for role in ROLES {
            for layer in 0..style.layers.max(1) {
                let Some(variants) = search_part(&cx, section, role, layer, candidates) else {
                    continue;
                };
                let best = variants.first()?;
                sum += best.score;
                n += 1;
                parts.push(LibraryPart {
                    section,
                    role,
                    layer,
                    variants,
                });
            }
        }
    }
    Some((parts, sum / n.max(1) as f32))
}

/// Fills a style's library.
pub fn compose(style: &Style, metrics: &MetricParams, p: &SearchParams) -> Vec<LibraryTune> {
    let mut rng = Rng::new(
        p.seed
            ^ style
                .name
                .bytes()
                .fold(0u32, |h, b| h.rotate_left(5) ^ b as u32),
    );
    let mut cells: BTreeMap<(u32, String), (u32, f32)> = BTreeMap::new();
    for _ in 0..p.tune_candidates.max(1) {
        let seed = rng.next_u32();
        let t = tune(style, seed);
        let cell = (bin(t.bpm, style.tempo, p.tempo_bins), form_cell(&t));
        let Some((_, quality)) = search_parts(style, &t, seed, p.quick_candidates, metrics, p)
        else {
            continue;
        };
        if cells.get(&cell).is_none_or(|&(_, q)| quality > q) {
            cells.insert(cell, (seed, quality));
        }
    }
    let mut library = Vec::new();
    for (cell, (seed, _)) in cells {
        let t = tune(style, seed);
        let Some((parts, quality)) = search_parts(style, &t, seed, p.part_candidates, metrics, p)
        else {
            continue;
        };
        library.push(LibraryTune {
            style: style.name.clone(),
            seed,
            cell,
            bpm: t.bpm,
            key: t.key.to_string(),
            minor: t.minor,
            form: t
                .form
                .iter()
                .map(|&s| t.sections[s].label.clone())
                .collect(),
            score: quality,
            parts,
            stinger_seed: Rng::new(seed ^ 0x5717_6E12).next_u32(),
        });
    }
    library
}

/// A library tune with its notes regenerated.
pub struct Realized {
    pub tune: Tune,
    /// Per section × role × layer that plays, its variants' fragments, best
    /// first. Parts the style leaves silent are absent.
    pub parts: Vec<(LibraryPart, Vec<Fragment>)>,
    pub stinger: Fragment,
}

pub fn realize(style: &Style, entry: &LibraryTune) -> Realized {
    let t = tune(style, entry.seed);
    let parts = entry
        .parts
        .iter()
        .map(|lp| {
            let fs = lp
                .variants
                .iter()
                .map(|v| part(style, &t, lp.section, lp.role, lp.layer, v.seed))
                .collect();
            (lp.clone(), fs)
        })
        .collect();
    let stinger = stinger(style, &t, entry.stinger_seed);
    Realized {
        tune: t,
        parts,
        stinger,
    }
}
