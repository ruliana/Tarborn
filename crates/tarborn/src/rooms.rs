//! The hand-built rooms, read from their chunk files in
//! `assets/chunks/rooms/`. A file that is missing or broken falls back to
//! the copy built into the game, so the game always starts.

use std::path::PathBuf;

use bevy::log::warn;
use bevy::math::Vec2;
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

/// The way out of a stage.
#[derive(Clone, Copy, Debug)]
pub struct Exit {
    /// Reaching this region, just inside the exit opening, ends the stage.
    pub goal: Goal,
    /// Unit vector pointing out of the stage through the opening.
    pub outward: Vec2,
}

/// A run stage: its simulation, with every opening walled off, and its exit
/// (inside the last socket). Read from `assets/chunks/stages/<id>.ron`.
pub fn load_stage(id: &str, exit_depth: f32, seal: f32) -> Result<(Sim, Exit), String> {
    let (sim, exit) = load_chunk(&format!("stages/{id}"), exit_depth, seal)?;
    let exit = exit.ok_or_else(|| format!("stage {id} has no exit socket"))?;
    Ok((sim, exit))
}

/// Any chunk file, `assets/chunks/<path>.ron`, played on its own: its
/// simulation with every opening walled off, and its exit (inside the last
/// socket) if it has sockets. Built-in stages stand in for missing files.
pub fn load_chunk(path: &str, exit_depth: f32, seal: f32) -> Result<(Sim, Option<Exit>), String> {
    let params = params();
    let file = PathBuf::from(DIR).join(format!("chunks/{path}.ron"));
    let chunk = match Chunk::load(&file, &params) {
        Ok(chunk) => chunk,
        Err(e) => {
            let text = path
                .strip_prefix("stages/")
                .and_then(|id| BUILT_IN_STAGES.iter().find(|(name, _)| *name == id))
                .map(|(_, text)| *text)
                .ok_or_else(|| e.to_string())?;
            warn!("{e}; using the built-in {path}");
            Chunk::from_ron(text).map_err(|e| e.to_string())?
        }
    };
    let exit = chunk.sockets.last().map(|socket| {
        let (at, half) = socket.region(chunk.bounds, exit_depth);
        Exit {
            goal: Goal {
                at,
                half,
                contact: false,
                max_speed: f32::INFINITY,
            },
            outward: -socket.inward(),
        }
    });
    // A chunk on its own joins onto nothing, so its entrance is sealed. The
    // exit is sealed too, just past the exit region, so the blob can't
    // leave while the screen fades.
    Ok((chunk.sealed(seal).sim(), exit))
}

/// Every chunk file that can be played on its own: `assets/chunks/<set>/
/// <id>.ron` for every set but the hand-built rooms, as `<set>/<id>`,
/// sorted. Sets nested deeper are listed too.
pub fn chunk_files() -> Vec<String> {
    let root = PathBuf::from(DIR).join("chunks");
    let mut found = Vec::new();
    let mut dirs = vec![root.clone()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for path in entries.flatten().map(|e| e.path()) {
            if path.is_dir() {
                if path != root.join("rooms") {
                    dirs.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "ron")
                && let Ok(rel) = path.with_extension("").strip_prefix(&root)
            {
                found.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    found.sort();
    found
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

    #[test]
    fn every_stage_is_walled_in_and_exits_on_the_right() {
        for (id, text) in BUILT_IN_STAGES {
            let chunk = Chunk::from_ron(text).unwrap();
            let (sim, exit) = load_stage(id, 1.0, 1.0).unwrap();
            assert_eq!(
                sim.level.solids.len(),
                chunk.solids.len() + chunk.sockets.len(),
                "{id}: one plug per opening"
            );
            assert_eq!(exit.outward, Vec2::X, "{id}");
        }
    }

    #[test]
    fn the_chunk_list_has_the_stages_but_not_the_rooms() {
        let files = chunk_files();
        assert!(files.contains(&"stages/intro".to_string()), "{files:?}");
        assert!(files.contains(&"stages/climb".to_string()), "{files:?}");
        assert!(files.iter().all(|f| !f.starts_with("rooms/")), "{files:?}");
        for file in files {
            load_chunk(&file, 1.0, 1.0).unwrap_or_else(|e| panic!("{file}: {e}"));
        }
        assert!(load_chunk("stages/nowhere", 1.0, 1.0).is_err());
    }
}
