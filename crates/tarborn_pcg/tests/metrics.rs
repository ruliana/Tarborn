//! Route metrics respond to the level the way they should.

use glam::Vec2;
use tarborn_pcg::metrics::{Measured, MetricParams, Metrics, entropy, ngram_distance, novelty};
use tarborn_pcg::{BeatKind, BeatParams, Chunk, Goal, Planner, PlannerParams, Shape};

/// A take-off slab, a gap, and a landing slab `landing` metres wide, all
/// floating over nothing: miss the landing, or overshoot it, and the blob
/// falls away. The goal is to come to rest on the landing.
fn gap(landing: f32) -> (Chunk, Goal) {
    let v = Vec2::new;
    let (gap, top) = (3.0, 1.0);
    let land = 6.0 + gap;
    let chunk = Chunk {
        id: format!("gap-{landing}"),
        meta: Default::default(),
        bounds: (v(0.0, -40.0), v(30.0, 10.0)),
        solids: vec![
            Shape::Rect(v(0.0, 0.0), v(6.0, top)),
            Shape::Rect(v(land, 0.0), v(land + landing, top)),
        ],
        hazards: vec![],
        pickups: vec![],
        enemies: vec![],
        sockets: vec![],
        reward_slots: vec![],
        routes: vec![],
        spawn: Some(v(2.0, 1.6)),
        view: None,
        waves: vec![],
    };
    let goal = Goal {
        at: v(land + landing * 0.5, top + 0.5),
        half: v(landing * 0.5, 0.5),
        contact: true,
        max_speed: PlannerParams::default().goal_max_speed,
    };
    (chunk, goal)
}

fn measure(chunk: &Chunk, goal: Goal) -> Measured {
    let planner = Planner::new(PlannerParams::default(), BeatParams::default());
    let metrics = Metrics::new(MetricParams::default(), BeatParams::default());
    let start = planner.settle(&chunk.sim());
    let plan = planner.plan(&start, goal);
    metrics
        .measure(&start, goal, &plan)
        .unwrap_or_else(|| panic!("{}: not reached, closest {:.2}", chunk.id, plan.closest))
}

#[test]
fn difficulty_falls_when_the_landing_gets_wider() {
    let (narrow, at) = gap(1.5);
    let narrow = measure(&narrow, at);
    let (wide, at) = gap(8.0);
    let wide = measure(&wide, at);
    eprintln!(
        "narrow {:.3} {:?} {:?}; wide {:.3} {:?} {:?}",
        narrow.scores.difficulty,
        narrow.easiest.windows,
        narrow.easiest.beats,
        wide.scores.difficulty,
        wide.easiest.windows,
        wide.easiest.beats,
    );
    assert!(
        wide.scores.difficulty < narrow.scores.difficulty,
        "wide {:.3} {:?} vs narrow {:.3} {:?}",
        wide.scores.difficulty,
        wide.easiest.windows,
        narrow.scores.difficulty,
        narrow.easiest.windows,
    );
}

#[test]
fn every_metric_is_filled_for_a_measured_route() {
    let (chunk, at) = gap(4.0);
    let m = measure(&chunk, at);
    let s = m.scores;
    assert!(s.length > 0.0);
    assert!(s.complexity >= 1.0);
    assert!(s.difficulty > 0.0 && s.difficulty <= 1.0);
    assert!(s.diversity >= 0.0);
    assert!(!m.easiest.windows.is_empty());
    assert!(
        m.easiest.tape.changes.len() <= m.timed.iter().map(|t| t.tape.changes.len()).max().unwrap()
    );
}

#[test]
fn entropy_counts_variety() {
    use BeatKind::*;
    assert_eq!(entropy([Jump, Jump, Jump].into_iter()), 0.0);
    assert!((entropy([Jump, Roll].into_iter()) - 1.0).abs() < 1e-6);
    assert!((entropy([Jump, Roll, Slam, Lunge].into_iter()) - 2.0).abs() < 1e-6);
}

#[test]
fn novelty_is_distance_to_the_nearest_routes() {
    use BeatKind::*;
    let set = |k: &[BeatKind]| k.iter().map(|&b| vec![b]).collect();
    let a = set(&[Jump, Roll]);
    let same = set(&[Jump, Roll]);
    let other = set(&[Slam, Lunge]);
    assert_eq!(ngram_distance(&a, &same), 0.0);
    assert_eq!(ngram_distance(&a, &other), 1.0);
    assert_eq!(novelty(&a, &[same.clone(), other.clone()], 1), 0.0);
    assert_eq!(novelty(&a, &[same, other], 2), 0.5);
    assert_eq!(novelty(&a, &[], 3), 1.0);
}
