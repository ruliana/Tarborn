//! Engine-free simulation core for Tarborn.
//!
//! Everything that decides how the tar blob moves lives here, with no Bevy
//! dependency, so it can be unit tested headlessly and stays deterministic.
//! The game crate only feeds it input and draws the result.

pub mod blob;
pub mod controller;
pub mod level;

pub use blob::{Blob, BlobParams, TarModes};
pub use controller::{Controller, Event, FeelParams, Intent};
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
    pub tick: u64,
}

impl Sim {
    pub fn new(level: Level, spawn: Vec2) -> Self {
        Self {
            level,
            blob: Blob::new(spawn, BlobParams::default()),
            controller: Controller::new(FeelParams::default()),
            tick: 0,
        }
    }

    /// Advances one fixed tick.
    pub fn step(&mut self, intent: Intent) -> Option<Event> {
        self.tick += 1;
        self.controller
            .step(&mut self.blob, &self.level, intent, DT)
    }
}
