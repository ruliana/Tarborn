//! The hand-built rooms, ported to chunk files, rebuild and play exactly like
//! the rooms built in code, and the simulation is deterministic.

use glam::Vec2;
use tarborn_pcg::{
    Chunk, ChunkError, ChunkParams, Route, Shape, Side, Socket, SocketKind, chunk_dir,
};
use tarborn_sim::{ARENA_SPAWN, Intent, Sim, Solid, TarModes};

const SPAWN: Vec2 = Vec2::new(0.0, 2.0);
/// Ticks each comparison runs for: 30 seconds.
const TICKS: u64 = 3600;

fn room(id: &str) -> Chunk {
    let path = chunk_dir("rooms").join(format!("{id}.ron"));
    Chunk::load(&path, &ChunkParams::default()).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn rooms() -> [(&'static str, Sim); 3] {
    [
        ("training", Sim::training_room(SPAWN)),
        ("arena", Sim::arena_room()),
        ("dressing", Sim::dressing_room(SPAWN)),
    ]
}

/// A busy scripted input: rolls both ways, jumps, holds, grips and aims down.
fn scripted(t: u64) -> Intent {
    let roll = ((t / 40) % 3) as f32 - 1.0;
    Intent {
        roll,
        aim: Vec2::new(roll, if t % 170 < 85 { -1.0 } else { 0.0 }),
        jump_pressed: t.is_multiple_of(90),
        jump_held: t % 90 < 30,
        lunge_pressed: false,
        modes: TarModes {
            sticky: t % 200 < 50,
            bouncy: false,
        },
    }
}

/// Everything about the world that can differ between two runs.
fn snapshot(sim: &Sim) -> String {
    format!(
        "{:?} {:?} {:?} {:?} {} {} {:?}",
        sim.blob.positions,
        sim.blob.velocities,
        sim.enemies,
        sim.projectiles,
        sim.health,
        sim.dead,
        sim.arena,
    )
}

#[test]
fn every_room_file_loads_and_validates() {
    for (id, _) in rooms() {
        let chunk = room(id);
        assert_eq!(chunk.id, id);
    }
}

#[test]
fn room_files_rebuild_the_hand_built_rooms() {
    for (id, built) in rooms() {
        let loaded = room(id).sim();
        same_solids(&loaded.level.solids, &built.level.solids, id);
        assert_eq!(loaded.hazards, built.hazards, "{id}: hazards differ");
        assert_eq!(loaded.pickups, built.pickups, "{id}: pickups differ");
        assert_eq!(loaded.enemies, built.enemies, "{id}: enemies differ");
        assert_eq!(loaded.view, built.view, "{id}: view differs");
        assert_eq!(loaded.arena, built.arena, "{id}: waves differ");
        assert_eq!(
            loaded.blob.positions, built.blob.positions,
            "{id}: spawn differs"
        );
    }
    assert_eq!(room("arena").spawn, Some(ARENA_SPAWN));
}

/// Solids built from sines and cosines (the dressing room's arcs) can differ
/// in the last bits between machines, because `sin` and `cos` are not
/// correctly rounded everywhere. The file is the source of truth, so the
/// code-built room only has to match it closely.
fn same_solids(a: &[Solid], b: &[Solid], id: &str) {
    const TOLERANCE: f32 = 1e-5;
    assert_eq!(a.len(), b.len(), "{id}: solid count differs");
    for (i, (a, b)) in a.iter().zip(b).enumerate() {
        assert_eq!(
            a.vertices.len(),
            b.vertices.len(),
            "{id}: solid {i} differs"
        );
        for (p, q) in a.vertices.iter().zip(&b.vertices) {
            assert!(p.distance(*q) < TOLERANCE, "{id}: solid {i}: {p} vs {q}");
        }
    }
}

#[test]
fn ported_rooms_play_identically_tick_by_tick() {
    // Only the rooms made of exact numbers: see `same_solids`.
    for (id, mut built) in rooms().into_iter().filter(|(id, _)| *id != "dressing") {
        let mut loaded = room(id).sim();
        for t in 0..TICKS {
            let a = built.step(scripted(t));
            let b = loaded.step(scripted(t));
            assert_eq!(a, b, "{id}: events differ at tick {t}");
            assert_eq!(
                snapshot(&built),
                snapshot(&loaded),
                "{id}: state differs at tick {t}"
            );
        }
    }
}

#[test]
fn the_same_tape_twice_gives_the_same_state_every_tick() {
    for (id, _) in rooms() {
        let mut a = room(id).sim();
        let mut b = room(id).sim();
        for t in 0..TICKS {
            a.step(scripted(t));
            b.step(scripted(t));
            assert_eq!(snapshot(&a), snapshot(&b), "{id}: diverged at tick {t}");
        }
    }
}

#[test]
fn chunks_round_trip_through_ron() {
    for (id, _) in rooms() {
        let chunk = room(id);
        let again = Chunk::from_ron(&chunk.to_ron()).unwrap();
        assert_eq!(again, chunk, "{id}");
    }
}

/// A 10 x 6 m box open on the left and right at floor level.
fn corridor() -> Chunk {
    let v = Vec2::new;
    Chunk {
        id: "corridor".into(),
        meta: Default::default(),
        bounds: (v(0.0, 0.0), v(10.0, 6.0)),
        solids: vec![
            Shape::Rect(v(0.0, 0.0), v(10.0, 1.0)),
            Shape::Rect(v(0.0, 5.0), v(10.0, 6.0)),
        ],
        hazards: vec![],
        pickups: vec![],
        enemies: vec![],
        sockets: vec![
            Socket {
                side: Side::Left,
                at: 2.5,
                width: 2.0,
                kind: SocketKind::Walk,
            },
            Socket {
                side: Side::Right,
                at: 2.5,
                width: 2.0,
                kind: SocketKind::Walk,
            },
        ],
        reward_slots: vec![v(5.0, 2.0)],
        routes: vec![Route {
            from: 0,
            to: 1,
            scores: None,
        }],
        spawn: None,
        view: None,
        waves: vec![],
    }
}

fn problems(chunk: &Chunk) -> Vec<String> {
    match chunk.validate(&ChunkParams::default()) {
        Ok(()) => Vec::new(),
        Err(ChunkError::Invalid(p)) => p,
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn a_well_formed_chunk_validates_and_spawns_inside_its_entry() {
    let chunk = corridor();
    assert!(problems(&chunk).is_empty(), "{:?}", problems(&chunk));
    let sim = chunk.sim();
    let c = sim.blob.center();
    assert!(c.x > 0.0 && c.x < 2.0 && c.y > 1.0 && c.y < 5.0, "{c}");
}

#[test]
fn validation_catches_bad_chunks() {
    let v = Vec2::new;
    let mut narrow = corridor();
    narrow.sockets[0].width = 0.5;
    assert!(problems(&narrow).iter().any(|p| p.contains("narrower")));

    let mut off_grid = corridor();
    off_grid.sockets[1].at = 2.3;
    assert!(
        problems(&off_grid)
            .iter()
            .any(|p| p.contains("off the grid"))
    );

    let mut blocked = corridor();
    blocked.solids.push(Shape::Rect(v(0.0, 1.0), v(1.0, 5.0)));
    assert!(problems(&blocked).iter().any(|p| p.contains("blocked")));

    let mut concave = corridor();
    concave.solids.push(Shape::Poly(vec![
        v(4.0, 2.0),
        v(6.0, 2.0),
        v(5.0, 2.5),
        v(6.0, 4.0),
        v(4.0, 4.0),
    ]));
    assert!(problems(&concave).iter().any(|p| p.contains("not convex")));

    let mut dangling = corridor();
    dangling.routes.push(Route {
        from: 0,
        to: 7,
        scores: None,
    });
    assert!(
        problems(&dangling)
            .iter()
            .any(|p| p.contains("doesn't exist"))
    );

    let mut buried = corridor();
    buried.reward_slots.push(v(5.0, 0.5));
    assert!(
        problems(&buried)
            .iter()
            .any(|p| p.contains("inside a solid"))
    );
}

#[test]
fn every_stage_file_loads_and_validates() {
    let dir = chunk_dir("stages");
    let mut count = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let chunk = Chunk::load(&path, &ChunkParams::default())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(
            chunk.sockets.len() >= 2,
            "{}: needs an entry and an exit",
            chunk.id
        );
        count += 1;
    }
    assert!(count > 0);
}

#[test]
fn a_socket_region_sits_just_inside_its_opening() {
    let chunk = corridor();
    let (mid, half) = chunk.sockets[1].region(chunk.bounds, 1.0);
    assert_eq!(mid, Vec2::new(9.5, 2.5));
    assert_eq!(half, Vec2::new(0.5, 1.0));
}
