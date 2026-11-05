//! The arena: one room, waves of enemies that mix every kind, and grubs
//! always around to throw or to feed the brutes. It is the place to judge
//! whether the moves chain into fun fights.

use glam::Vec2;

use crate::controller::Event;
use crate::enemy::{Enemy, EnemyKind};
use crate::level::{Level, Solid};

/// Where the player starts: on the high middle platform.
pub const ARENA_SPAWN: Vec2 = Vec2::new(0.0, 9.0);
/// Seconds between clearing a wave and the next one dropping in.
pub const WAVE_BREAK: f32 = 2.5;
/// Grubs kept alive in the arena at all times.
const AMBIENT_GRUBS: [Vec2; 2] = [Vec2::new(-17.0, 0.35), Vec2::new(17.0, 0.35)];

/// One enemy of a wave: what, and where it drops in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drop {
    pub kind: EnemyKind,
    pub at: Vec2,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// A wave is in the room.
    Fighting,
    /// Cleared a wave; the next one comes after this many seconds.
    Break(f32),
    /// Every wave is done.
    Cleared,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Arena {
    pub waves: Vec<Vec<Drop>>,
    /// The wave in play (or coming next during a break), from 0.
    pub wave: usize,
    pub phase: Phase,
    /// Indices into `Sim::enemies` of the current wave.
    members: Vec<usize>,
}

impl Arena {
    pub fn new(waves: Vec<Vec<Drop>>) -> Self {
        Self {
            waves,
            wave: 0,
            // The first wave drops in after a short pause too.
            phase: Phase::Break(1.5),
            members: Vec::new(),
        }
    }

    /// The waves in the arena room, mixing every kind more and more.
    pub fn standard() -> Self {
        use EnemyKind::*;
        let d = |kind, x: f32, y: f32| Drop {
            kind,
            at: Vec2::new(x, y),
        };
        Self::new(vec![
            // Shields: flank, slam, or throw a grub.
            vec![d(Guard, -10.0, 2.0), d(Guard, 10.0, 2.0)],
            // Ranged from the ledges, and a skitter that won't take a lunge.
            vec![
                d(Spitter, -11.0, 6.5),
                d(Spitter, 11.0, 6.5),
                d(Skitter, -6.0, 2.0),
            ],
            // A skitter pack, and guards that hate skitters.
            vec![
                d(Skitter, -15.0, 2.0),
                d(Skitter, 6.0, 2.0),
                d(Skitter, 15.0, 2.0),
                d(Guard, -12.0, 2.0),
                d(Guard, 12.0, 2.0),
            ],
            // A brute, with company to get caught in its slams.
            vec![
                d(Brute, 12.0, 2.5),
                d(Guard, -10.0, 2.0),
                d(Spitter, -11.0, 6.5),
            ],
            // Everything.
            vec![
                d(Brute, -12.0, 2.5),
                d(Brute, 12.0, 2.5),
                d(Skitter, -6.0, 2.0),
                d(Skitter, 6.0, 2.0),
                d(Spitter, 11.0, 6.5),
                d(Guard, 0.0, 10.0),
            ],
        ])
    }

    /// Seconds left in a break, if in one.
    pub fn break_left(&self) -> Option<f32> {
        match self.phase {
            Phase::Break(left) => Some(left),
            _ => None,
        }
    }

    /// Advances waves: starts the next one after a break, and starts a break
    /// once every enemy of the current wave is dead.
    pub(crate) fn step(&mut self, enemies: &mut Vec<Enemy>, dt: f32, events: &mut Vec<Event>) {
        keep_grubs(enemies);
        match self.phase {
            Phase::Fighting => {
                let alive = self.members.iter().any(|&i| enemies[i].alive());
                if !alive {
                    self.members.clear();
                    self.wave += 1;
                    if self.wave >= self.waves.len() {
                        self.phase = Phase::Cleared;
                        events.push(Event::ArenaCleared);
                    } else {
                        self.phase = Phase::Break(WAVE_BREAK);
                    }
                }
            }
            Phase::Break(left) => {
                let left = left - dt;
                if left > 0.0 {
                    self.phase = Phase::Break(left);
                    return;
                }
                for drop in &self.waves[self.wave] {
                    let mut enemy = Enemy::new(drop.kind, drop.at);
                    enemy.respawns = false;
                    // Face the middle of the room.
                    enemy.facing = if drop.at.x > 0.0 { -1.0 } else { 1.0 };
                    self.members.push(enemies.len());
                    enemies.push(enemy);
                }
                self.phase = Phase::Fighting;
                events.push(Event::WaveStarted { wave: self.wave });
            }
            Phase::Cleared => {}
        }
    }
}

/// Keeps the ambient grubs in the room: they respawn as usual, so they only
/// need adding once.
fn keep_grubs(enemies: &mut Vec<Enemy>) {
    if !enemies.iter().any(|e| e.kind == EnemyKind::Grub) {
        enemies.extend(AMBIENT_GRUBS.iter().map(|&at| Enemy::grub(at)));
    }
}

impl Level {
    /// A wide room with ledges, a high middle platform, walls to climb and a
    /// strip of spikes under the middle.
    pub fn arena() -> Self {
        let v = Vec2::new;
        Self {
            solids: vec![
                Solid::rect(v(-20.0, -2.0), v(20.0, 0.0)),   // floor
                Solid::rect(v(-21.0, -2.0), v(-20.0, 17.0)), // left wall
                Solid::rect(v(20.0, -2.0), v(21.0, 17.0)),   // right wall
                Solid::rect(v(-21.0, 16.0), v(21.0, 17.0)),  // ceiling
                Solid::rect(v(-14.0, 5.0), v(-8.0, 5.5)),    // left ledge
                Solid::rect(v(8.0, 5.0), v(14.0, 5.5)),      // right ledge
                Solid::rect(v(-3.0, 8.0), v(3.0, 8.5)),      // high middle platform
                Solid::rect(v(-20.0, 0.0), v(-18.0, 2.5)),   // left step
                Solid::rect(v(18.0, 0.0), v(20.0, 2.5)),     // right step
            ],
        }
    }
}

/// The spikes under the middle platform.
pub fn arena_hazards() -> Vec<Solid> {
    vec![Solid::rect(Vec2::new(-1.5, 0.0), Vec2::new(1.5, 0.3))]
}
