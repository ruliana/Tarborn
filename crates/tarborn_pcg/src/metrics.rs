//! Route metrics, all measured from what the bots actually did.
//!
//! - **Length**: ticks the easiest solution takes.
//! - **Complexity**: beats in the shortest required chain, after dropping
//!   every input the route still works without.
//! - **Difficulty**: how precisely each required input must be timed. Each
//!   one is moved earlier and later until the route fails; narrow windows
//!   are strict. Scored on the easiest solution, so the game stays forgiving.
//! - **Diversity**: entropy of the beat kinds the solutions use.
//! - **Novelty**: how different the route's beat n-grams are from its
//!   nearest neighbours in a library.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use glam::Vec2;
use serde::{Deserialize, Serialize};
use tarborn_sim::Sim;

use crate::ChunkError;
use crate::beats::{BeatKind, BeatParams};
use crate::chunk::Scores;
use crate::planner::{Goal, Plan, Solution};
use crate::tape::{Tape, play_until};

const BUILT_IN_PARAMS: &str = include_str!("../../tarborn/assets/pcg/metrics.ron");

/// Points in a hand-built room for the report to measure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Probe {
    pub room: String,
    pub goals: Vec<Vec2>,
}

/// Tunables for scoring, from `assets/pcg/metrics.ron`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MetricParams {
    pub window_step: u64,
    pub max_shift: u64,
    pub window_scale: f32,
    pub max_weight: f32,
    pub mean_weight: f32,
    pub replay_slack: u64,
    pub ngram: usize,
    pub nearest: usize,
    pub probes: Vec<Probe>,
}

impl Default for MetricParams {
    fn default() -> Self {
        ron::from_str(BUILT_IN_PARAMS).expect("built-in assets/pcg/metrics.ron is valid")
    }
}

impl MetricParams {
    pub fn load(path: &Path) -> Result<Self, ChunkError> {
        let text = std::fs::read_to_string(path).map_err(|e| ChunkError::Io(e.to_string()))?;
        ron::from_str(&text).map_err(|e| ChunkError::Parse(e.to_string()))
    }
}

/// One solution, stripped to the inputs it needs, with their windows.
#[derive(Clone, Debug, PartialEq)]
pub struct Timed {
    /// The solution with every skippable input removed.
    pub tape: Tape,
    pub ticks: u64,
    pub beats: Vec<BeatKind>,
    /// The window of each required input, in seconds.
    pub windows: Vec<f32>,
    pub difficulty: f32,
}

/// Everything measured on one route.
#[derive(Clone, Debug, PartialEq)]
pub struct Measured {
    pub goal: Vec2,
    pub scores: Scores,
    /// The easiest solution, stripped and timed.
    pub easiest: Timed,
    /// Every solution, stripped and timed, easiest first.
    pub timed: Vec<Timed>,
    pub pass_rate: f32,
}

impl Measured {
    /// The route's beat n-grams, pooled over its solutions.
    pub fn ngrams(&self, n: usize) -> BTreeSet<Vec<BeatKind>> {
        let mut set = BTreeSet::new();
        for t in &self.timed {
            for size in 1..=n.max(1) {
                for w in t.beats.windows(size) {
                    set.insert(w.to_vec());
                }
            }
        }
        set
    }
}

/// Scores routes using replays in the full simulation.
pub struct Metrics {
    pub params: MetricParams,
    pub beats: BeatParams,
}

impl Metrics {
    pub fn new(params: MetricParams, beats: BeatParams) -> Self {
        Self { params, beats }
    }

    /// Plays `tape` from `start`; the tick it reached the goal, if it did.
    fn reaches(&self, start: &Sim, goal: Goal, tape: &Tape, ticks: u64) -> Option<u64> {
        let mut sim = start.clone();
        let run = play_until(&mut sim, tape, ticks, &self.beats, |s| goal.reached(s));
        goal.reached(&sim).then_some(run.ticks)
    }

    /// Measures a planned route. `None` if the plan found nothing.
    /// Novelty is left at 0: it needs the other routes, see [`fill_novelty`].
    pub fn measure(&self, start: &Sim, goal: Goal, plan: &Plan) -> Option<Measured> {
        let mut timed: Vec<Timed> = plan
            .solutions
            .iter()
            .filter_map(|s| self.time(start, goal, s))
            .collect();
        timed.sort_by(|a, b| {
            a.difficulty
                .total_cmp(&b.difficulty)
                .then(a.ticks.cmp(&b.ticks))
        });
        let easiest = timed.first()?.clone();
        let complexity = timed.iter().map(|t| t.beats.len()).min().unwrap_or(0);
        let pass_rate = plan
            .solutions
            .iter()
            .map(|s| s.pass_rate)
            .fold(0.0, f32::max);
        Some(Measured {
            goal: goal.at,
            scores: Scores {
                diversity: entropy(timed.iter().flat_map(|t| t.beats.iter().copied())),
                complexity: complexity as f32,
                difficulty: easiest.difficulty,
                novelty: 0.0,
                length: easiest.ticks as f32,
            },
            easiest,
            timed,
            pass_rate,
        })
    }

    /// Strips a solution to the inputs it needs, then times each one.
    pub fn time(&self, start: &Sim, goal: Goal, solution: &Solution) -> Option<Timed> {
        let p = &self.params;
        let limit = solution.ticks + p.replay_slack;
        let mut tape = solution.tape.clone();
        let mut ticks = self.reaches(start, goal, &tape, limit)?;
        // Drop every change the route works without, last first.
        let mut i = tape.changes.len();
        while i > 0 {
            i -= 1;
            let mut shorter = tape.clone();
            shorter.changes.remove(i);
            if let Some(t) = self.reaches(start, goal, &shorter, limit) {
                tape = shorter;
                ticks = t;
            }
        }
        let windows: Vec<f32> = (0..tape.changes.len())
            .map(|i| self.window(start, goal, &tape, i, limit))
            .collect();
        let strictness: Vec<f32> = windows
            .iter()
            .map(|w| (-w / p.window_scale).exp())
            .collect();
        let max = strictness.iter().copied().fold(0.0, f32::max);
        let mean = if strictness.is_empty() {
            0.0
        } else {
            strictness.iter().sum::<f32>() / strictness.len() as f32
        };
        let mut sim = start.clone();
        let beats = play_until(&mut sim, &tape, ticks, &self.beats, |s| goal.reached(s))
            .beats
            .into_iter()
            .map(|b| b.kind)
            .collect();
        Some(Timed {
            tape,
            ticks,
            beats,
            windows,
            difficulty: p.max_weight * max + p.mean_weight * mean,
        })
    }

    /// The widest window, in seconds, that change `i` can move in and the
    /// route still works. A side cut short by a neighbouring change or by
    /// the start of the tape, rather than by the route failing, is taken to
    /// be as wide as the other side: those limits say nothing about the level.
    fn window(&self, start: &Sim, goal: Goal, tape: &Tape, i: usize, limit: u64) -> f32 {
        let p = &self.params;
        let step = p.window_step.max(1);
        let tick = tape.changes[i].tick;
        let floor = if i == 0 {
            0
        } else {
            tape.changes[i - 1].tick + 1
        };
        let ceiling = tape.changes.get(i + 1).map_or(u64::MAX, |c| c.tick - 1);
        let works = |to: u64| {
            let mut moved = tape.clone();
            moved.changes[i].tick = to;
            self.reaches(start, goal, &moved, limit + p.max_shift)
                .is_some()
        };
        // How far one side goes, and whether a failure stopped it.
        let side = |earlier: bool| {
            let mut shift = 0;
            loop {
                let next = shift + step;
                if next > p.max_shift {
                    return (shift, true);
                }
                let to = if earlier {
                    match tick.checked_sub(next) {
                        Some(t) if t >= floor => t,
                        _ => return (shift, false),
                    }
                } else {
                    let t = tick + next;
                    if t > ceiling {
                        return (shift, false);
                    }
                    t
                };
                if !works(to) {
                    return (shift, true);
                }
                shift = next;
            }
        };
        let (mut early, early_real) = side(true);
        let (mut late, late_real) = side(false);
        match (early_real, late_real) {
            (false, true) => early = early.max(late),
            (true, false) => late = late.max(early),
            _ => {}
        }
        (early + late + 1) as f32 / tarborn_sim::TICKS_PER_SECOND
    }
}

/// Shannon entropy, in bits, of the kinds' frequencies.
pub fn entropy(kinds: impl Iterator<Item = BeatKind>) -> f32 {
    let mut counts: BTreeMap<BeatKind, usize> = BTreeMap::new();
    let mut total = 0;
    for k in kinds {
        *counts.entry(k).or_default() += 1;
        total += 1;
    }
    if total == 0 {
        return 0.0;
    }
    counts
        .values()
        .map(|&c| {
            let p = c as f32 / total as f32;
            -p * p.log2()
        })
        .sum::<f32>()
        .max(0.0)
}

/// 1 minus the Jaccard similarity of two n-gram sets: 0 alike, 1 nothing shared.
pub fn ngram_distance(a: &BTreeSet<Vec<BeatKind>>, b: &BTreeSet<Vec<BeatKind>>) -> f32 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    1.0 - a.intersection(b).count() as f32 / union as f32
}

/// Novelty of `route` against a library: the mean n-gram distance to its
/// `nearest` closest routes. A route with no library is fully novel.
pub fn novelty(
    route: &BTreeSet<Vec<BeatKind>>,
    library: &[BTreeSet<Vec<BeatKind>>],
    nearest: usize,
) -> f32 {
    if library.is_empty() {
        return 1.0;
    }
    let mut d: Vec<f32> = library.iter().map(|l| ngram_distance(route, l)).collect();
    d.sort_by(f32::total_cmp);
    let k = nearest.clamp(1, d.len());
    d[..k].iter().sum::<f32>() / k as f32
}

/// Sets each route's novelty against all the others.
pub fn fill_novelty(routes: &mut [Measured], params: &MetricParams) {
    let grams: Vec<_> = routes.iter().map(|r| r.ngrams(params.ngram)).collect();
    for (i, route) in routes.iter_mut().enumerate() {
        let others: Vec<_> = grams
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, g)| g.clone())
            .collect();
        route.scores.novelty = novelty(&grams[i], &others, params.nearest);
    }
}
