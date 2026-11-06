//! Input tapes: what a player (or a bot) does, as a list of changes to the
//! two buttons and the direction, and a headless runner that plays a tape
//! in the simulation.

use glam::Vec2;
use serde::{Deserialize, Serialize};
use tarborn_sim::{Event, Intent, Sim, TarModes};

use crate::beats::{Beat, BeatDetector, BeatParams};

/// The whole controller at one moment: two buttons and a direction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Buttons {
    pub jump: bool,
    pub grip: bool,
    /// Stick direction; longer than 1 is clamped.
    pub dir: Vec2,
}

impl Buttons {
    pub const NONE: Self = Self {
        jump: false,
        grip: false,
        dir: Vec2::ZERO,
    };

    pub fn dir(dir: Vec2) -> Self {
        Self { dir, ..Self::NONE }
    }

    pub fn with_jump(mut self) -> Self {
        self.jump = true;
        self
    }

    pub fn with_grip(mut self) -> Self {
        self.grip = true;
        self
    }
}

/// From `tick` on, the controller reads `buttons`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub tick: u64,
    pub buttons: Buttons,
}

/// A plan of input: changes sorted by tick. Before the first change nothing
/// is held.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Tape {
    pub changes: Vec<Change>,
}

impl Tape {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a change at `tick`, replacing any change already at that tick.
    pub fn at(mut self, tick: u64, buttons: Buttons) -> Self {
        self.set(tick, buttons);
        self
    }

    /// Adds a change at `tick`, keeping the list sorted.
    pub fn set(&mut self, tick: u64, buttons: Buttons) {
        match self.changes.binary_search_by_key(&tick, |c| c.tick) {
            Ok(i) => self.changes[i].buttons = buttons,
            Err(i) => self.changes.insert(i, Change { tick, buttons }),
        }
    }

    /// The tick of the last change, or 0.
    pub fn last_tick(&self) -> u64 {
        self.changes.last().map_or(0, |c| c.tick)
    }

    /// What is held at `tick`.
    pub fn buttons_at(&self, tick: u64) -> Buttons {
        match self.changes.binary_search_by_key(&tick, |c| c.tick) {
            Ok(i) => self.changes[i].buttons,
            Err(0) => Buttons::NONE,
            Err(i) => self.changes[i - 1].buttons,
        }
    }

    /// The simulation input for `tick`, the same way the game reads a
    /// controller: Jump counts as pressed on the tick it goes down.
    pub fn intent(&self, tick: u64) -> Intent {
        let now = self.buttons_at(tick);
        let before = if tick == 0 {
            Buttons::NONE
        } else {
            self.buttons_at(tick - 1)
        };
        intent(now, before)
    }
}

/// The simulation input for `now`, given what was held the tick before.
pub fn intent(now: Buttons, before: Buttons) -> Intent {
    let aim = now.dir.clamp_length_max(1.0);
    Intent {
        roll: aim.x.clamp(-1.0, 1.0),
        aim,
        jump_pressed: now.jump && !before.jump,
        jump_held: now.jump,
        lunge_pressed: false,
        modes: TarModes {
            sticky: now.grip,
            bouncy: false,
        },
    }
}

/// What happened while a tape played.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    /// Ticks simulated.
    pub ticks: u64,
    /// Every simulation event, with the tick it happened on (counting from 0).
    pub events: Vec<(u64, Event)>,
    /// The beats performed, in order.
    pub beats: Vec<Beat>,
    pub died: bool,
}

/// Plays `tape` for `ticks` ticks (stopping early if the blob dies) and
/// reports the beats performed. `sim` is left in its final state.
pub fn play(sim: &mut Sim, tape: &Tape, ticks: u64, params: &BeatParams) -> Run {
    play_until(sim, tape, ticks, params, |_| false)
}

/// Like [`play`], but also stops once `done` says so after a tick.
pub fn play_until(
    sim: &mut Sim,
    tape: &Tape,
    ticks: u64,
    params: &BeatParams,
    mut done: impl FnMut(&Sim) -> bool,
) -> Run {
    let mut detector = BeatDetector::new(params.clone());
    let mut run = Run::default();
    for tick in 0..ticks {
        let intent = tape.intent(tick);
        let events = sim.step(intent);
        detector.observe(tick, sim, intent, &events);
        run.events.extend(events.into_iter().map(|e| (tick, e)));
        run.ticks = tick + 1;
        if sim.dead {
            run.died = true;
            break;
        }
        if done(sim) {
            break;
        }
    }
    run.beats = detector.finish();
    run
}
