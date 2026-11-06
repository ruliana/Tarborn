//! The beat detector: watches the simulation tick by tick and names the
//! moves the player actually performed. Bots, metrics and telemetry all use
//! it, so a score always comes from what happened, not from what a grammar
//! intended.

use std::path::Path;

use glam::Vec2;
use serde::{Deserialize, Serialize};
use tarborn_sim::{EnemyKind, Event, HitKind, Intent, Sim};

use crate::ChunkError;

const BUILT_IN_PARAMS: &str = include_str!("../../tarborn/assets/pcg/beats.ron");

/// Tunables for reading beats, from `assets/pcg/beats.ron`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BeatParams {
    pub roll_min_speed: f32,
    pub roll_min_time: f32,
    pub floor_normal_y: f32,
    pub ceiling_normal_y: f32,
    pub wall_normal_x: f32,
    pub stick_min_time: f32,
    pub drop_min_height: f32,
    pub launch_grace: f32,
    pub bounce_min_gap: f32,
    pub fight_window: f32,
    pub fight_min_kills: usize,
    pub lunge_target_reach: f32,
}

impl Default for BeatParams {
    fn default() -> Self {
        ron::from_str(BUILT_IN_PARAMS).expect("built-in assets/pcg/beats.ron is valid")
    }
}

impl BeatParams {
    pub fn load(path: &Path) -> Result<Self, ChunkError> {
        let text = std::fs::read_to_string(path).map_err(|e| ChunkError::Io(e.to_string()))?;
        ron::from_str(&text).map_err(|e| ChunkError::Parse(e.to_string()))
    }
}

/// The kinds of beat the two verbs can produce. These are the grammar's
/// terminals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum BeatKind {
    Roll,
    Jump,
    /// A shrunk jump off a surface with no enemy near.
    Burst,
    StickWall,
    StickCeiling,
    /// Falling from a ledge or a ceiling without jumping.
    Drop,
    /// Rebounding with Jump held in the air.
    Bounce,
    Lunge,
    Pogo,
    Slam,
    GrabThrow,
    /// Killing one or more enemies in one stretch of fighting.
    Fight,
}

impl BeatKind {
    pub const ALL: [BeatKind; 12] = [
        BeatKind::Roll,
        BeatKind::Jump,
        BeatKind::Burst,
        BeatKind::StickWall,
        BeatKind::StickCeiling,
        BeatKind::Drop,
        BeatKind::Bounce,
        BeatKind::Lunge,
        BeatKind::Pogo,
        BeatKind::Slam,
        BeatKind::GrabThrow,
        BeatKind::Fight,
    ];
}

/// One move performed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Beat {
    /// When it started, counting ticks from 0.
    pub tick: u64,
    pub kind: BeatKind,
    /// The enemies involved: the target of a lunge, pogo or throw, or every
    /// kind hit in a fight (sorted, no repeats).
    pub enemies: Vec<EnemyKind>,
    /// Where the blob was.
    pub at: Vec2,
}

/// A stretch that becomes a beat once it has lasted long enough.
#[derive(Clone, Copy, Debug, Default)]
struct Stretch {
    since: Option<u64>,
    emitted: bool,
}

impl Stretch {
    /// Advances the stretch; true on the tick it first lasts `min_ticks`.
    fn update(&mut self, on: bool, tick: u64, min_ticks: u64) -> Option<u64> {
        if !on {
            *self = Self::default();
            return None;
        }
        let since = *self.since.get_or_insert(tick);
        if !self.emitted && tick + 1 - since >= min_ticks {
            self.emitted = true;
            return Some(since);
        }
        None
    }
}

#[derive(Clone, Debug)]
struct Fight {
    start: u64,
    at: Vec2,
    last: u64,
    kinds: Vec<EnemyKind>,
    kills: usize,
}

#[derive(Clone, Copy, Debug)]
struct Air {
    top: f32,
    launched: bool,
    emitted: bool,
}

/// Turns ticks of simulation into beats.
#[derive(Clone, Debug)]
pub struct BeatDetector {
    params: BeatParams,
    beats: Vec<Beat>,
    roll: Stretch,
    wall: Stretch,
    ceiling: Stretch,
    air: Option<Air>,
    last_launch: Option<u64>,
    last_bounce: Option<u64>,
    /// Enemy grabbed and not yet let go.
    grabbed: Option<usize>,
    /// When the player last hit or threw each enemy, so kills by other
    /// enemies or hazards don't count as the player's fight.
    touched: Vec<(usize, u64)>,
    fight: Option<Fight>,
}

fn ticks(seconds: f32) -> u64 {
    (seconds * tarborn_sim::TICKS_PER_SECOND).round().max(1.0) as u64
}

impl BeatDetector {
    pub fn new(params: BeatParams) -> Self {
        Self {
            params,
            beats: Vec::new(),
            roll: Stretch::default(),
            wall: Stretch::default(),
            ceiling: Stretch::default(),
            air: None,
            last_launch: None,
            last_bounce: None,
            grabbed: None,
            touched: Vec::new(),
            fight: None,
        }
    }

    fn push(&mut self, tick: u64, kind: BeatKind, enemies: Vec<EnemyKind>, at: Vec2) {
        self.beats.push(Beat {
            tick,
            kind,
            enemies,
            at,
        });
    }

    /// Reads one tick: the simulation after the step, the input it got and
    /// the events it produced.
    pub fn observe(&mut self, tick: u64, sim: &Sim, intent: Intent, events: &[Event]) {
        let p = self.params.clone();
        let at = sim.blob.center();
        let kind_of = |i: usize| sim.enemies.get(i).map(|e| e.kind);

        for event in events {
            match *event {
                Event::Jumped { .. } => {
                    self.last_launch = Some(tick);
                    self.push(tick, BeatKind::Jump, vec![], at);
                }
                Event::Launched { .. } => {
                    self.last_launch = Some(tick);
                    self.push(tick, BeatKind::Burst, vec![], at);
                }
                Event::Lunged { direction } => {
                    self.last_launch = Some(tick);
                    let reach = sim.controller.params.lunge_range * p.lunge_target_reach;
                    let target = sim
                        .enemies
                        .iter()
                        .filter(|e| e.alive() && !e.held && e.position.distance(at) <= reach)
                        .min_by(|a, b| {
                            let score = |e: &tarborn_sim::Enemy| {
                                let to = e.position - at;
                                to.length() * (2.0 - to.normalize_or_zero().dot(direction))
                            };
                            score(a).total_cmp(&score(b))
                        })
                        .map(|e| e.kind);
                    self.push(tick, BeatKind::Lunge, target.into_iter().collect(), at);
                }
                Event::SlamStarted => self.last_launch = Some(tick),
                Event::Slammed { .. } => self.push(tick, BeatKind::Slam, vec![], at),
                Event::Bounced { .. } => {
                    let gap = ticks(p.bounce_min_gap);
                    if self.last_bounce.is_none_or(|t| tick >= t + gap) {
                        self.push(tick, BeatKind::Bounce, vec![], at);
                    }
                    self.last_bounce = Some(tick);
                    self.last_launch = Some(tick);
                }
                Event::Hit { enemy, kind, .. } => {
                    if kind == HitKind::Pogo {
                        self.push(
                            tick,
                            BeatKind::Pogo,
                            kind_of(enemy).into_iter().collect(),
                            at,
                        );
                    }
                    self.touch(enemy, tick);
                    self.fight_hit(tick, at, kind_of(enemy), false);
                }
                // A body the player threw hitting another enemy is the player's hit.
                Event::Struck { enemy, .. } if sim.enemies.iter().any(|e| e.thrown) => {
                    self.touch(enemy, tick);
                    self.fight_hit(tick, at, kind_of(enemy), false);
                }
                Event::Killed { enemy, .. } => {
                    let window = ticks(p.fight_window);
                    if self
                        .touched
                        .iter()
                        .any(|&(e, t)| e == enemy && tick <= t + window)
                    {
                        self.fight_hit(tick, at, kind_of(enemy), true);
                    }
                }
                Event::Grabbed { enemy } => self.grabbed = Some(enemy),
                Event::Thrown { enemy, .. } => {
                    self.touch(enemy, tick);
                    if self.grabbed.take() == Some(enemy) {
                        self.push(
                            tick,
                            BeatKind::GrabThrow,
                            kind_of(enemy).into_iter().collect(),
                            at,
                        );
                    }
                }
                Event::Dropped { .. } => self.grabbed = None,
                _ => {}
            }
        }
        if let Some(fight) = &self.fight
            && tick > fight.last + ticks(p.fight_window)
        {
            self.close_fight();
        }

        // Surfaces under the blob this tick.
        let normals: Vec<Vec2> = sim.blob.contacts.iter().flatten().copied().collect();
        let floor = normals.iter().any(|n| n.y > p.floor_normal_y);
        let wall = normals.iter().any(|n| n.x.abs() > p.wall_normal_x);
        let ceiling = normals.iter().any(|n| n.y < p.ceiling_normal_y);
        let sticky = intent.modes.sticky;

        let speed = sim.blob.velocity().x.abs();
        if let Some(since) = self.roll.update(
            floor && speed >= p.roll_min_speed,
            tick,
            ticks(p.roll_min_time),
        ) {
            self.push(since, BeatKind::Roll, vec![], at);
        }
        let stick = ticks(p.stick_min_time);
        if let Some(since) = self.wall.update(sticky && wall && !floor, tick, stick) {
            self.push(since, BeatKind::StickWall, vec![], at);
        }
        if let Some(since) = self
            .ceiling
            .update(sticky && ceiling && !floor, tick, stick)
        {
            self.push(since, BeatKind::StickCeiling, vec![], at);
        }

        // Drops: airborne without having been launched, falling far enough.
        if normals.is_empty() {
            let recent = self
                .last_launch
                .is_some_and(|t| tick <= t + ticks(p.launch_grace));
            let air = self.air.get_or_insert(Air {
                top: at.y,
                launched: recent,
                emitted: false,
            });
            air.launched |= recent;
            air.top = if air.launched {
                air.top
            } else {
                air.top.max(at.y)
            };
            if !air.launched && !air.emitted && air.top - at.y >= p.drop_min_height {
                air.emitted = true;
                self.push(tick, BeatKind::Drop, vec![], at);
            }
        } else {
            self.air = None;
        }
    }

    fn touch(&mut self, enemy: usize, tick: u64) {
        self.touched.retain(|&(e, _)| e != enemy);
        self.touched.push((enemy, tick));
    }

    fn fight_hit(&mut self, tick: u64, at: Vec2, kind: Option<EnemyKind>, killed: bool) {
        let fight = self.fight.get_or_insert(Fight {
            start: tick,
            at,
            last: tick,
            kinds: Vec::new(),
            kills: 0,
        });
        fight.last = tick;
        if let Some(kind) = kind
            && !fight.kinds.contains(&kind)
        {
            fight.kinds.push(kind);
        }
        if killed {
            fight.kills += 1;
        }
    }

    fn close_fight(&mut self) {
        if let Some(mut fight) = self.fight.take()
            && fight.kills >= self.params.fight_min_kills
        {
            fight.kinds.sort_by_key(|k| *k as u8);
            self.push(fight.start, BeatKind::Fight, fight.kinds, fight.at);
        }
    }

    /// Ends the run: closes any fight in progress and returns every beat,
    /// in the order they started.
    pub fn finish(mut self) -> Vec<Beat> {
        self.close_fight();
        self.beats.sort_by_key(|b| b.tick);
        self.beats
    }
}
