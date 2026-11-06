//! Measures routes in the hand-built rooms and prints their metrics:
//!
//! ```sh
//! cargo run --release -p tarborn_pcg --bin report            # every probe
//! cargo run --release -p tarborn_pcg --bin report -- arena   # one room
//! ```
//!
//! The rooms and goal points are the `probes` in `assets/pcg/metrics.ron`.

use std::time::Instant;

use tarborn_pcg::metrics::{MetricParams, Metrics, fill_novelty};
use tarborn_pcg::{BeatParams, Chunk, ChunkParams, Planner, PlannerParams, assets_dir, chunk_dir};

fn main() {
    let pcg = assets_dir().join("pcg");
    let or_default = |e| {
        eprintln!("{e}; using the built-in parameters");
    };
    let chunk_params = ChunkParams::load(&pcg.join("chunk.ron")).unwrap_or_else(|e| {
        or_default(e);
        ChunkParams::default()
    });
    let beats = BeatParams::load(&pcg.join("beats.ron")).unwrap_or_else(|e| {
        or_default(e);
        BeatParams::default()
    });
    let planner_params = PlannerParams::load(&pcg.join("planner.ron")).unwrap_or_else(|e| {
        or_default(e);
        PlannerParams::default()
    });
    let metric_params = MetricParams::load(&pcg.join("metrics.ron")).unwrap_or_else(|e| {
        or_default(e);
        MetricParams::default()
    });
    let only: Vec<String> = std::env::args().skip(1).collect();

    let planner = Planner::new(planner_params.clone(), beats.clone());
    let metrics = Metrics::new(metric_params.clone(), beats);
    let mut routes = Vec::new();
    let mut misses = Vec::new();
    let clock = Instant::now();
    for probe in &metric_params.probes {
        if !only.is_empty() && !only.contains(&probe.room) {
            continue;
        }
        let path = chunk_dir("rooms").join(format!("{}.ron", probe.room));
        let chunk = match Chunk::load(&path, &chunk_params) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        };
        let start = planner.settle(&chunk.sim());
        for &goal in &probe.goals {
            let goal = planner.goal(goal);
            let plan = planner.plan(&start, goal);
            match metrics.measure(&start, goal, &plan) {
                Some(m) => routes.push((probe.room.clone(), m)),
                None => misses.push(format!(
                    "{} {}: not reached (closest {:.2} m)",
                    probe.room, goal.at, plan.closest
                )),
            }
        }
    }
    let mut measured: Vec<_> = routes.iter().map(|(_, m)| m.clone()).collect();
    fill_novelty(&mut measured, &metric_params);

    println!(
        "{:<9} {:>14}  {:>6} {:>5} {:>5} {:>5} {:>5} {:>5}  easiest route",
        "room", "goal", "length", "cmplx", "diff", "div", "novel", "pass"
    );
    for ((room, _), m) in routes.iter().zip(&measured) {
        let s = m.scores;
        let beats: Vec<String> = m.easiest.beats.iter().map(|b| format!("{b:?}")).collect();
        println!(
            "{:<9} {:>14}  {:>5.2}s {:>5} {:>5.2} {:>5.2} {:>5.2} {:>5.2}  {}",
            room,
            format!("({:.1}, {:.1})", m.goal.x, m.goal.y),
            s.length / tarborn_sim::TICKS_PER_SECOND,
            s.complexity,
            s.difficulty,
            s.diversity,
            s.novelty,
            m.pass_rate,
            beats.join(" > "),
        );
    }
    for miss in &misses {
        println!("{miss}");
    }
    eprintln!(
        "measured {} routes in {:.1}s",
        routes.len(),
        clock.elapsed().as_secs_f32()
    );
}
