//! Engine-free simulation core for Tarborn.
//!
//! Everything that decides how the game plays lives here, with no Bevy
//! dependency, so it can be unit tested headlessly and stays deterministic.
//! The game crate only feeds it input and draws the result.

pub mod blob;
pub mod combat;
pub mod controller;
pub mod enemy;
pub mod level;

pub use blob::{Blob, BlobParams, TarModes};
pub use combat::{CombatParams, Projectile};
pub use controller::{Controller, Event, FeelParams, HitKind, Intent};
pub use enemy::{AttackPhase, Enemy, EnemyKind, State};
pub use level::{Level, Solid};

use glam::Vec2;

/// The fixed simulation rate. Gameplay always advances in steps of this size,
/// regardless of the display's frame rate.
pub const TICKS_PER_SECOND: f32 = 120.0;
pub const DT: f32 = 1.0 / TICKS_PER_SECOND;

/// The whole simulated world.
#[derive(Clone, Debug)]
pub struct Sim {
    pub level: Level,
    pub blob: Blob,
    pub controller: Controller,
    pub combat: CombatParams,
    pub enemies: Vec<Enemy>,
    /// Regions that hurt the player on touch, such as spikes.
    pub hazards: Vec<Solid>,
    /// Tar drops that heal one health pip.
    pub pickups: Vec<Vec2>,
    /// Enemy shots in flight.
    pub projectiles: Vec<Projectile>,
    /// Noises the player made last tick: where, and how far they carry.
    pub noises: Vec<(Vec2, f32)>,
    /// The enemy held in the blob's grip, if any.
    pub held: Option<usize>,
    pub health: u32,
    pub max_health: u32,
    pub dead: bool,
    pub tick: u64,
    /// Time left with the world frozen after a hit, so contact feels solid.
    hitstop_left: f32,
    /// Time left invulnerable after being hurt.
    pub(crate) hurt_left: f32,
    /// Presses that arrived during hitstop, replayed when it ends.
    pending_jump: bool,
    pending_lunge: bool,
}

impl Sim {
    pub fn new(level: Level, spawn: Vec2) -> Self {
        let combat = CombatParams::default();
        Self {
            level,
            blob: Blob::new(spawn, BlobParams::default()),
            controller: Controller::new(FeelParams::default()),
            enemies: Vec::new(),
            hazards: Vec::new(),
            pickups: Vec::new(),
            noises: Vec::new(),
            projectiles: Vec::new(),
            held: None,
            health: combat.max_health,
            max_health: combat.max_health,
            combat,
            dead: false,
            tick: 0,
            hitstop_left: 0.0,
            hurt_left: 0.0,
            pending_jump: false,
            pending_lunge: false,
        }
    }

    /// The test room with a dummy, grubs, guards, a spitter and spikes.
    pub fn training_room(spawn: Vec2) -> Self {
        let v = Vec2::new;
        let mut sim = Self::new(Level::test_room(), spawn);
        sim.enemies = vec![
            Enemy::guard(v(-8.0, 0.6)),
            Enemy::grub(v(-9.0, 5.85)),
            Enemy::dummy(v(6.0, 0.6)),
            Enemy::guard(v(16.0, 4.6)),
            Enemy::grub(v(2.5, 0.35)),
            Enemy::spitter(v(-10.5, 5.95)),
        ];
        sim.hazards = vec![Solid::rect(v(-19.0, 0.0), v(-15.0, 0.3))];
        sim
    }

    /// True while frozen by a hit.
    pub fn in_hitstop(&self) -> bool {
        self.hitstop_left > 0.0
    }

    /// While the blob is shrunk (Grip held), some attacks pass it by: spit,
    /// and the guard's swing.
    pub fn player_shrunk(&self) -> bool {
        self.controller.is_shrunk()
    }

    /// The player can't be hurt while lunging, slamming, or just after a hit.
    pub fn player_invulnerable(&self) -> bool {
        self.controller.invulnerable() || self.hurt_left > 0.0
    }

    /// Advances one fixed tick.
    pub fn step(&mut self, mut intent: Intent) -> Vec<Event> {
        self.tick += 1;
        if self.dead {
            return Vec::new();
        }
        if self.hitstop_left > 0.0 {
            self.hitstop_left -= DT;
            self.pending_jump |= intent.jump_pressed;
            self.pending_lunge |= intent.lunge_pressed;
            return Vec::new();
        }
        intent.jump_pressed |= std::mem::take(&mut self.pending_jump);
        // Holding jump keeps the body springy, unless gripping (then it shrinks).
        intent.modes.bouncy |= intent.jump_held && !intent.modes.sticky;
        intent.lunge_pressed |= std::mem::take(&mut self.pending_lunge);

        let mut events = Vec::new();
        // While holding an enemy, Jump throws it instead of jumping.
        if self.held.is_some() && intent.jump_pressed {
            intent.jump_pressed = false;
            combat::throw(self, intent.aim, &mut events);
        }
        let targets: Vec<Vec2> = self
            .enemies
            .iter()
            .filter(|e| e.alive() && !e.held && !e.thrown)
            .map(|e| e.position)
            .collect();
        events.extend(
            self.controller
                .step(&mut self.blob, &self.level, intent, &targets, DT),
        );
        self.hurt_left = (self.hurt_left - DT).max(0.0);
        combat::resolve(self, intent, &mut events);
        events
    }

    pub(crate) fn freeze(&mut self, seconds: f32) {
        self.hitstop_left = self.hitstop_left.max(seconds);
    }
}
