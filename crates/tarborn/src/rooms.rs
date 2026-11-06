//! The hand-built rooms, read from their chunk files in
//! `assets/chunks/rooms/`. A file that is missing or broken falls back to
//! the copy built into the game, so the game always starts.

use std::path::PathBuf;

use bevy::log::warn;
use tarborn_pcg::{Chunk, ChunkParams, Goal};
use tarborn_sim::Sim;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");

const BUILT_IN: [(&str, &str); 3] = [
    (
        "training",
        include_str!("../assets/chunks/rooms/training.ron"),
    ),
    ("arena", include_str!("../assets/chunks/rooms/arena.ron")),
    (
        "dressing",
        include_str!("../assets/chunks/rooms/dressing.ron"),
    ),
];

const BUILT_IN_STAGES: [(&str, &str); 2] = [
    ("intro", include_str!("../assets/chunks/stages/intro.ron")),
    ("climb", include_str!("../assets/chunks/stages/climb.ron")),
];

fn params() -> ChunkParams {
    let path = PathBuf::from(DIR).join("pcg/chunk.ron");
    ChunkParams::load(&path).unwrap_or_else(|e| {
        warn!(
            "{}: {e}; using the built-in chunk parameters",
            path.display()
        );
        ChunkParams::default()
    })
}

/// A fresh simulation of the room `id`.
pub fn load(id: &str) -> Sim {
    let params = params();
    let path = PathBuf::from(DIR).join(format!("chunks/rooms/{id}.ron"));
    match Chunk::load(&path, &params) {
        Ok(chunk) => chunk.sim(),
        Err(e) => {
            warn!("{e}; using the built-in {id} room");
            let (_, text) = BUILT_IN
                .iter()
                .find(|(name, _)| *name == id)
                .unwrap_or_else(|| panic!("no built-in room {id}"));
            Chunk::from_ron(text)
                .expect("built-in room files parse")
                .sim()
        }
    }
}

/// A run stage: its simulation, and the exit region (inside the last
/// socket) that ends it. Read from `assets/chunks/stages/<id>.ron`.
pub fn load_stage(id: &str, exit_depth: f32) -> Result<(Sim, Goal), String> {
    let params = params();
    let path = PathBuf::from(DIR).join(format!("chunks/stages/{id}.ron"));
    let chunk = match Chunk::load(&path, &params) {
        Ok(chunk) => chunk,
        Err(e) => {
            warn!("{e}; trying the built-in {id} stage");
            let text = BUILT_IN_STAGES
                .iter()
                .find(|(name, _)| *name == id)
                .map(|(_, text)| *text)
                .ok_or_else(|| format!("no stage {id}"))?;
            Chunk::from_ron(text).map_err(|e| e.to_string())?
        }
    };
    let exit = chunk
        .sockets
        .last()
        .ok_or_else(|| format!("stage {id} has no exit socket"))?;
    let (at, half) = exit.region(chunk.bounds, exit_depth);
    let goal = Goal {
        at,
        half,
        contact: false,
        max_speed: f32::INFINITY,
    };
    Ok((chunk.sim(), goal))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_room_parses_and_validates() {
        let params = ChunkParams::default();
        for (id, text) in BUILT_IN.into_iter().chain(BUILT_IN_STAGES) {
            let chunk = Chunk::from_ron(text).unwrap_or_else(|e| panic!("{id}: {e}"));
            chunk
                .validate(&params)
                .unwrap_or_else(|e| panic!("{id}: {e}"));
        }
    }
}
