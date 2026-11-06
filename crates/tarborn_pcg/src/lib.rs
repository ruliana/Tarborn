//! Engine-free level generation for Tarborn.
//!
//! Levels are data: a [`Chunk`] is one self-contained piece of level stored
//! as a RON file under `assets/chunks/<set>/<id>.ron`, and loads into the
//! simulation as [`tarborn_sim::Level`] solids plus enemy spawns. Bots,
//! metrics, the offline chunk generator and the runtime assembler all build
//! on this crate, and none of it depends on Bevy.

pub mod beats;
pub mod chunk;
pub mod metrics;
pub mod planner;
pub mod tape;

pub use chunk::{
    Chunk, ChunkError, ChunkParams, EnemySlot, Meta, Route, Scores, Shape, Side, Socket,
    SocketKind, solid_bounds,
};

pub use beats::{Beat, BeatDetector, BeatKind, BeatParams};
pub use planner::{Goal, Plan, Planner, PlannerParams, Solution, vary_enemies};
pub use tape::{Buttons, Change, Run, Tape, play, play_until};

use std::path::PathBuf;

/// The game's asset folder, where chunk files and generation parameters live.
pub fn assets_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../tarborn/assets"))
}

/// The folder of chunk files for one set, such as `rooms`.
pub fn chunk_dir(set: &str) -> PathBuf {
    assets_dir().join("chunks").join(set)
}

/// A small, seeded, deterministic random number generator (SplitMix64), so
/// every search and generator run can be repeated exactly.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform in 0..n. `n` must be above 0.
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}
