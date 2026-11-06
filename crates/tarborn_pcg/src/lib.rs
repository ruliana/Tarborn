//! Engine-free level generation for Tarborn.
//!
//! Levels are data: a [`Chunk`] is one self-contained piece of level stored
//! as a RON file under `assets/chunks/<set>/<id>.ron`, and loads into the
//! simulation as [`tarborn_sim::Level`] solids plus enemy spawns. Bots,
//! metrics, the offline chunk generator and the runtime assembler all build
//! on this crate, and none of it depends on Bevy.

pub mod beats;
pub mod chunk;
pub mod tape;

pub use chunk::{
    Chunk, ChunkError, ChunkParams, EnemySlot, Meta, Route, Scores, Shape, Side, Socket,
    SocketKind, solid_bounds,
};

pub use beats::{Beat, BeatDetector, BeatKind, BeatParams};
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
