//! The planner bot reaches listed points in the training room and the arena
//! within its tick budget, and its plans replay and repeat exactly.

use glam::Vec2;
use tarborn_pcg::{
    BeatParams, Chunk, ChunkParams, Planner, PlannerParams, chunk_dir, play_until, vary_enemies,
};
use tarborn_sim::Sim;

fn room(id: &str) -> Sim {
    Chunk::load(
        &chunk_dir("rooms").join(format!("{id}.ron")),
        &ChunkParams::default(),
    )
    .unwrap()
    .sim()
}

fn planner() -> Planner {
    Planner::new(PlannerParams::default(), BeatParams::default())
}

/// Plans to each goal and checks the quickest solution: within budget, and
/// replaying its tape from the start really gets there.
fn reaches_all(id: &str, goals: &[Vec2]) {
    let planner = planner();
    let p = &planner.params;
    let start = room(id);
    let mut missed = Vec::new();
    for &goal in goals {
        let plan = planner.plan(&start, goal);
        let Some(best) = plan.quickest() else {
            missed.push(format!("{goal} (closest {:.2} m)", plan.closest));
            continue;
        };
        assert!(
            best.ticks <= p.tick_budget,
            "{id} {goal}: {} ticks",
            best.ticks
        );
        let mut sim = start.clone();
        play_until(&mut sim, &best.tape, best.ticks, &planner.beats, |_| false);
        let off = sim.blob.center().distance(goal);
        assert!(
            off <= p.goal_radius,
            "{id} {goal}: replay ends {off:.2} m away"
        );
        assert!(!best.beats.is_empty(), "{id} {goal}: no beats");
    }
    assert!(missed.is_empty(), "{id}: missed {missed:?}");
}

#[test]
fn reaches_every_target_in_the_training_room() {
    let v = Vec2::new;
    reaches_all(
        "training",
        &[
            v(5.0, 1.0),  // up the slope, past the dummy
            v(-3.5, 8.6), // on top of the pillar
            v(-9.0, 6.0), // on the floating ledge
            v(16.0, 4.8), // on the plateau
            v(0.0, 11.3), // under the ceiling
        ],
    );
}

#[test]
fn reaches_every_target_in_the_arena() {
    let v = Vec2::new;
    reaches_all(
        "arena",
        &[
            v(-11.0, 6.2), // left ledge
            v(11.0, 6.2),  // right ledge
            v(-17.0, 0.6), // left floor
            v(19.0, 3.2),  // on the right step
            v(0.0, 15.0),  // under the ceiling
        ],
    );
}

#[test]
fn keeps_distinct_solutions() {
    let plan = planner().plan(&room("training"), Vec2::new(-3.5, 8.6));
    let kinds: Vec<_> = plan.solutions.iter().map(|s| s.beat_kinds()).collect();
    assert!(kinds.len() >= 2, "{kinds:?}");
    for (i, a) in kinds.iter().enumerate() {
        assert!(!kinds[i + 1..].contains(a), "repeated {a:?}");
    }
    for pair in plan.solutions.windows(2) {
        assert!(pair[0].ticks <= pair[1].ticks, "not quickest first");
    }
}

#[test]
fn planning_is_deterministic() {
    let goal = Vec2::new(-9.0, 6.0);
    let a = planner().plan(&room("training"), goal);
    let b = planner().plan(&room("training"), goal);
    assert_eq!(a, b);
}

#[test]
fn gives_up_on_a_goal_inside_a_wall() {
    let params = PlannerParams {
        iterations: 5,
        ..PlannerParams::default()
    };
    let planner = Planner::new(params, BeatParams::default());
    let plan = planner.plan(&room("training"), Vec2::new(0.0, -1.0));
    assert!(!plan.reached());
    assert_eq!(plan.iterations, 5);
}

#[test]
fn enemy_variations_move_enemies_a_little() {
    let base = room("training");
    let mut same = base.clone();
    vary_enemies(&mut same, 0, 0.6);
    assert_eq!(same.enemies, base.enemies);
    let mut varied = base.clone();
    vary_enemies(&mut varied, 3, 0.6);
    assert_ne!(varied.enemies, base.enemies);
    for (a, b) in varied.enemies.iter().zip(&base.enemies) {
        assert!((a.position.x - b.position.x).abs() <= 0.6);
        assert_eq!(a.position.y, b.position.y);
    }
}
