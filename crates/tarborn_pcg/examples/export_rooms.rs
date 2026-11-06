//! Writes the hand-built rooms in `tarborn_sim` out as chunk files:
//! `cargo run -p tarborn_pcg --example export_rooms`.
//!
//! The files in `assets/chunks/rooms/` were made this way. The tests check
//! the files still rebuild exactly the hand-built rooms.

use glam::Vec2;
use tarborn_pcg::{Chunk, chunk_dir, solid_bounds};
use tarborn_sim::{ARENA_SPAWN, Sim};

fn main() {
    let spawn = Vec2::new(0.0, 2.0);
    let rooms = [
        ("training", Sim::training_room(spawn), spawn),
        ("arena", Sim::arena_room(), ARENA_SPAWN),
        ("dressing", Sim::dressing_room(spawn), spawn),
    ];
    let dir = chunk_dir("rooms");
    std::fs::create_dir_all(&dir).expect("can create the rooms folder");
    for (id, sim, spawn) in rooms {
        let chunk = Chunk::from_sim(id, &sim, spawn, solid_bounds(&sim.level));
        let path = dir.join(format!("{id}.ron"));
        std::fs::write(&path, chunk.to_ron()).expect("can write the chunk");
        println!("wrote {}", path.display());
    }
}
