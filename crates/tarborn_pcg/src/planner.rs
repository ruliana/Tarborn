//! The planner bot: finds input tapes that take the blob to a goal, by
//! searching over macro-actions in the full simulation.
//!
//! The search is Go-Explore style: it keeps an archive of cells (bucketed
//! blob positions), each holding the quickest simulation state found that
//! reaches it, and each iteration expands the most promising cells plus a
//! few at random with every macro-action. Everything is deterministic: the
//! archive is ordered and the random picks come from a seeded generator.

use std::collections::BTreeMap;
use std::path::Path;

use glam::Vec2;
use serde::{Deserialize, Serialize};
use tarborn_sim::Sim;

use crate::beats::{Beat, BeatDetector, BeatKind, BeatParams};
use crate::tape::{Buttons, Tape, play_until};
use crate::{ChunkError, Rng};

const BUILT_IN_PARAMS: &str = include_str!("../../tarborn/assets/pcg/planner.ron");

/// How Jump is used during a macro-action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JumpUse {
    None,
    /// Pressed at the start, let go after `tap_ticks`.
    Tap,
    /// Held the whole time.
    Hold,
}

/// One macro-action: a stick direction and how the buttons are used.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Macro {
    pub dir: Vec2,
    pub jump: JumpUse,
    pub grip: bool,
}

/// Tunables for the planner, from `assets/pcg/planner.ron`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlannerParams {
    pub cell: f32,
    pub beam: usize,
    pub explore: usize,
    pub iterations: usize,
    pub tick_budget: u64,
    pub settle_ticks: u64,
    pub goal_half: Vec2,
    pub goal_contact: bool,
    pub goal_max_speed: f32,
    pub time_weight: f32,
    pub visit_weight: f32,
    pub durations: Vec<u64>,
    pub tap_ticks: u64,
    pub macros: Vec<Macro>,
    pub solutions: usize,
    pub enemy_seeds: u64,
    pub pass_share: f32,
    pub enemy_jitter: f32,
    pub replay_slack: u64,
}

impl Default for PlannerParams {
    fn default() -> Self {
        ron::from_str(BUILT_IN_PARAMS).expect("built-in assets/pcg/planner.ron is valid")
    }
}

impl PlannerParams {
    pub fn load(path: &Path) -> Result<Self, ChunkError> {
        let text = std::fs::read_to_string(path).map_err(|e| ChunkError::Io(e.to_string()))?;
        ron::from_str(&text).map_err(|e| ChunkError::Parse(e.to_string()))
    }
}

/// Where a route ends and what counts as arriving there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Goal {
    /// The middle of the goal region.
    pub at: Vec2,
    /// Half its size: the blob's centre must be inside the box...
    pub half: Vec2,
    /// ...while touching something, so flying past doesn't count...
    pub contact: bool,
    /// ...and moving no faster than this, so rolling across doesn't either.
    pub max_speed: f32,
}

impl Goal {
    pub fn reached(&self, sim: &Sim) -> bool {
        !sim.dead
            && self.distance(sim) == 0.0
            && (!self.contact || sim.blob.touching_count() > 0)
            && sim.blob.velocity().length() <= self.max_speed
    }

    /// How far the blob's centre is from the goal region, 0 inside it.
    pub fn distance(&self, sim: &Sim) -> f32 {
        let d = (sim.blob.center() - self.at).abs() - self.half;
        d.max(Vec2::ZERO).length()
    }

    /// The same rules around another point.
    pub fn moved_to(self, at: Vec2) -> Self {
        Self { at, ..self }
    }
}

/// A tape that reached the goal.
#[derive(Clone, Debug, PartialEq)]
pub struct Solution {
    pub tape: Tape,
    /// Ticks from the start until the blob was within reach of the goal.
    pub ticks: u64,
    /// The beats performed on the way, from replaying the tape.
    pub beats: Vec<Beat>,
    /// Share of enemy variations the tape also works on, from 0 to 1.
    pub pass_rate: f32,
}

impl Solution {
    /// The beat kinds in order: what makes two solutions different.
    pub fn beat_kinds(&self) -> Vec<BeatKind> {
        self.beats.iter().map(|b| b.kind).collect()
    }

    pub fn passes(&self, params: &PlannerParams) -> bool {
        self.pass_rate >= params.pass_share
    }
}

/// What the search found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    /// Distinct solutions, quickest first.
    pub solutions: Vec<Solution>,
    /// Iterations run and cells reached, for reports.
    pub iterations: usize,
    pub cells: usize,
    /// The closest the search got to the goal.
    pub closest: f32,
}

impl Plan {
    pub fn reached(&self) -> bool {
        !self.solutions.is_empty()
    }

    pub fn quickest(&self) -> Option<&Solution> {
        self.solutions.first()
    }
}

#[derive(Clone)]
struct Node {
    sim: Sim,
    tape: Tape,
    tick: u64,
    /// What was held on the last tick, so a new Tap can let go first.
    last: Buttons,
    visits: u32,
    /// Reads beats as the node is extended, so a solution's beats are known
    /// the moment it reaches the goal.
    detector: BeatDetector,
}

type Cell = (i32, i32, bool);

fn cell_of(sim: &Sim, size: f32) -> Cell {
    let c = sim.blob.center();
    (
        (c.x / size).floor() as i32,
        (c.y / size).floor() as i32,
        sim.blob.touching_count() > 0,
    )
}

/// Moves each enemy sideways by up to `jitter` metres, picks its facing at
/// random and staggers its state clock. Seed 0 leaves the enemies as they
/// are.
pub fn vary_enemies(sim: &mut Sim, seed: u64, jitter: f32) {
    if seed == 0 {
        return;
    }
    let mut rng = Rng::new(seed);
    for enemy in &mut sim.enemies {
        let dx = (rng.unit() * 2.0 - 1.0) * jitter;
        enemy.position.x += dx;
        enemy.home.x += dx;
        enemy.facing = if rng.unit() < 0.5 { -1.0 } else { 1.0 };
        enemy.state_time = rng.unit();
    }
}

/// Plans routes from a starting simulation to goals.
pub struct Planner {
    pub params: PlannerParams,
    pub beats: BeatParams,
}

impl Planner {
    pub fn new(params: PlannerParams, beats: BeatParams) -> Self {
        Self { params, beats }
    }

    /// `sim` after `settle_ticks` with nothing held, so plans start from a
    /// blob at rest rather than one still dropping in at its spawn.
    pub fn settle(&self, sim: &Sim) -> Sim {
        let mut sim = sim.clone();
        for _ in 0..self.params.settle_ticks {
            sim.step(crate::tape::intent(Buttons::NONE, Buttons::NONE));
        }
        sim
    }

    /// A goal at `at`, with the arrival rules from the parameters.
    pub fn goal(&self, at: Vec2) -> Goal {
        Goal {
            at,
            half: self.params.goal_half,
            contact: self.params.goal_contact,
            max_speed: self.params.goal_max_speed,
        }
    }

    /// Searches for up to `solutions` distinct tapes that take the blob in
    /// `start` to `goal`, then checks each against enemy variations.
    pub fn plan(&self, start: &Sim, goal: Goal) -> Plan {
        let p = &self.params;
        let mut rng = Rng::new(0x7a7b_0123);
        let mut archive: BTreeMap<Cell, Node> = BTreeMap::new();
        archive.insert(
            cell_of(start, p.cell),
            Node {
                sim: start.clone(),
                tape: Tape::new(),
                tick: 0,
                last: Buttons::NONE,
                visits: 0,
                detector: BeatDetector::new(self.beats.clone()),
            },
        );
        let mut plan = Plan {
            closest: goal.distance(start),
            ..Plan::default()
        };
        let mut found: Vec<Solution> = Vec::new();
        let score = |n: &Node| {
            goal.distance(&n.sim) + p.time_weight * n.tick as f32 + p.visit_weight * n.visits as f32
        };

        for iteration in 0..p.iterations {
            plan.iterations = iteration + 1;
            // Pick the cells to expand.
            let mut ranked: Vec<(f32, Cell)> =
                archive.iter().map(|(c, n)| (score(n), *c)).collect();
            ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            let mut picks: Vec<Cell> = ranked.iter().take(p.beam).map(|(_, c)| *c).collect();
            let rest: Vec<Cell> = ranked.iter().skip(p.beam).map(|(_, c)| *c).collect();
            for _ in 0..p.explore.min(rest.len()) {
                let c = rest[rng.below(rest.len())];
                if !picks.contains(&c) {
                    picks.push(c);
                }
            }

            for cell in picks {
                let Some(parent) = archive.get_mut(&cell) else {
                    continue;
                };
                parent.visits += 1;
                let parent = parent.clone();
                if parent.tick >= p.tick_budget {
                    continue;
                }
                for m in &p.macros {
                    for &duration in &p.durations {
                        let mut node = parent.clone();
                        node.visits = 0;
                        let reached = self.extend(&mut node, *m, duration, goal);
                        if node.sim.dead {
                            continue;
                        }
                        plan.closest = plan.closest.min(goal.distance(&node.sim));
                        if reached {
                            add_distinct(
                                &mut found,
                                Solution {
                                    tape: node.tape,
                                    ticks: node.tick,
                                    beats: node.detector.finish(),
                                    pass_rate: 0.0,
                                },
                            );
                            continue;
                        }
                        let key = cell_of(&node.sim, p.cell);
                        let better = archive.get(&key).is_none_or(|old| node.tick < old.tick);
                        if better {
                            archive.insert(key, node);
                        }
                    }
                }
            }
            plan.cells = archive.len();
            if found.len() >= p.solutions {
                break;
            }
        }

        found.sort_by_key(|s| s.ticks);
        found.truncate(p.solutions);
        plan.solutions = found
            .into_iter()
            .map(|s| self.verify(start, goal, s))
            .collect();
        plan
    }

    /// Runs one macro-action on `node`, a tick at a time. True if the blob
    /// came within reach of the goal; the tape then ends at that tick.
    fn extend(&self, node: &mut Node, m: Macro, duration: u64, goal: Goal) -> bool {
        let p = &self.params;
        let base = Buttons {
            jump: false,
            grip: m.grip,
            dir: m.dir,
        };
        // A tap after Jump was held needs one tick let go to count as a press.
        let release = m.jump == JumpUse::Tap && node.last.jump;
        for k in 0..duration {
            if node.tick >= p.tick_budget {
                break;
            }
            let jump = match m.jump {
                JumpUse::None => false,
                JumpUse::Hold => true,
                JumpUse::Tap => {
                    let k = k as i64 - release as i64;
                    k >= 0 && (k as u64) < p.tap_ticks
                }
            };
            let buttons = Buttons { jump, ..base };
            if buttons != node.last || node.tape.changes.is_empty() {
                node.tape.set(node.tick, buttons);
            }
            let intent = crate::tape::intent(buttons, node.last);
            let events = node.sim.step(intent);
            node.detector.observe(node.tick, &node.sim, intent, &events);
            node.last = buttons;
            node.tick += 1;
            if node.sim.dead {
                return false;
            }
            if goal.reached(&node.sim) {
                return true;
            }
        }
        false
    }

    /// Replays a solution on every enemy variation, for its pass rate.
    fn verify(&self, start: &Sim, goal: Goal, mut solution: Solution) -> Solution {
        let p = &self.params;
        let reach = |sim: &Sim| goal.reached(sim);
        let seeds = p.enemy_seeds.max(1);
        let mut passed = 0;
        for seed in 0..seeds {
            let mut sim = start.clone();
            vary_enemies(&mut sim, seed, p.enemy_jitter);
            let ticks = solution.ticks + p.replay_slack;
            play_until(&mut sim, &solution.tape, ticks, &self.beats, reach);
            if reach(&sim) {
                passed += 1;
            }
        }
        solution.pass_rate = passed as f32 / seeds as f32;
        solution
    }
}

/// Keeps the quickest solution for each distinct beat sequence.
fn add_distinct(found: &mut Vec<Solution>, solution: Solution) {
    let kinds = solution.beat_kinds();
    match found.iter_mut().find(|s| s.beat_kinds() == kinds) {
        Some(old) if solution.ticks < old.ticks => *old = solution,
        Some(_) => {}
        None => found.push(solution),
    }
}
