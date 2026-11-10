//! Enemy windups stretch so strikes land on the beat, never shorter than
//! their minimum and never by more than the allowed shift.

use glam::Vec2;
use tarborn_sim::{
    AttackPhase, BeatClock, BeatSnap, DT, Enemy, Intent, Level, Sim, Solid, State, TICKS_PER_SECOND,
};

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn flat_floor() -> Level {
    Level {
        solids: vec![Solid::rect(v(-100.0, -2.0), v(100.0, 0.0))],
    }
}

/// One finished windup: who, when it started and struck (ticks), and how
/// long it ran in sim time (hitstop pauses it).
struct Windup {
    enemy: usize,
    start: u64,
    strike: u64,
    active: f32,
}

/// A player standing still among every kind of attacker for `seconds`,
/// kept alive, at `bpm`.
fn fight(snap: BeatSnap, bpm: f32, seconds: f32) -> (Sim, Vec<Windup>) {
    let mut sim = Sim::new(flat_floor(), v(0.0, 0.6));
    sim.clock = BeatClock::new(bpm, 4);
    sim.beat_snap = snap;
    for _ in 0..120 {
        sim.step(Intent::default());
    }
    let mut guard = Enemy::guard(v(1.6, 0.6));
    guard.facing = -1.0;
    let mut spitter = Enemy::spitter(v(-7.0, 0.45));
    spitter.facing = 1.0;
    let mut brute = Enemy::brute(v(-2.5, 0.9));
    brute.facing = 1.0;
    let mut skitter = Enemy::skitter(v(5.0, 0.3));
    skitter.facing = -1.0;
    sim.enemies = vec![guard, spitter, brute, skitter];
    let mut started: Vec<Option<(u64, f32)>> = vec![None; sim.enemies.len()];
    let mut done = Vec::new();
    for _ in 0..(seconds / DT) as usize {
        let before: Vec<State> = sim.enemies.iter().map(|e| e.state).collect();
        let frozen = sim.in_hitstop();
        sim.step(Intent::default());
        if !frozen {
            for s in started.iter_mut().flatten() {
                s.1 += DT;
            }
        }
        sim.health = sim.max_health;
        for (i, e) in sim.enemies.iter().enumerate() {
            let windup = State::Attacking(AttackPhase::Windup);
            if e.state == windup && before[i] != windup {
                started[i] = Some((sim.tick, 0.0));
            }
            if before[i] == windup
                && e.state == State::Attacking(AttackPhase::Strike)
                && let Some((start, active)) = started[i].take()
            {
                done.push(Windup {
                    enemy: i,
                    start,
                    strike: sim.tick,
                    active,
                });
            }
            if e.state != windup
                && before[i] == windup
                && e.state != State::Attacking(AttackPhase::Strike)
            {
                started[i] = None;
            }
        }
    }
    (sim, done)
}

fn snap() -> BeatSnap {
    BeatSnap {
        enabled: true,
        max_shift: 0.15,
        grid: 0.5,
    }
}

/// Share of strikes within a tick of the grid.
fn on_beat(sim: &Sim, windups: &[Windup], grid: f64) -> f32 {
    let tolerance = 1.5 / sim.clock.tick_of_beat(1.0).max(1.0);
    let hits = windups
        .iter()
        .filter(|w| {
            let b = sim.clock.beats_at(w.strike as f64) / grid;
            (b - b.round()).abs() * grid
                <= tolerance.max(1.5 * sim.clock.bpm() as f64 / 60.0 / TICKS_PER_SECOND as f64)
        })
        .count();
    hits as f32 / windups.len().max(1) as f32
}

#[test]
fn no_windup_is_ever_shorter_than_its_minimum() {
    for bpm in [110.0, 123.0, 218.0, 257.0] {
        let (sim, windups) = fight(snap(), bpm, 30.0);
        assert!(windups.len() > 10, "{bpm}: only {} windups", windups.len());
        for w in &windups {
            let kind = sim.enemies[w.enemy].kind;
            let min = Enemy::windup_min(kind).unwrap();
            let took = w.active;
            assert!(w.strike > w.start);
            assert!(
                took + 1e-4 >= min,
                "{bpm}: {kind:?} wound up {took}s < {min}s"
            );
            assert!(
                took <= min + snap().max_shift + 2.0 * DT,
                "{bpm}: {kind:?} wound up {took}s"
            );
        }
    }
}

#[test]
fn snapped_strikes_land_on_the_beat() {
    let s = snap();
    for bpm in [110.0f32, 123.0, 218.0, 257.0] {
        let (sim, snapped) = fight(s, bpm, 30.0);
        let (plain_sim, plain) = fight(BeatSnap::default(), bpm, 30.0);
        let grid = s.grid as f64;
        let share = on_beat(&sim, &snapped, grid);
        let before = on_beat(&plain_sim, &plain, grid);
        // A windup can only reach the grid when the next point is within
        // max_shift: at slow tempos some strikes stay off it.
        let reachable = (s.max_shift / (s.grid * 60.0 / bpm)).min(1.0);
        println!(
            "{bpm} BPM: {:.0}% of strikes on the {}-beat grid (unsnapped {:.0}%, reachable {:.0}%)",
            share * 100.0,
            s.grid,
            before * 100.0,
            reachable * 100.0
        );
        assert!(share >= 0.7 * reachable, "{bpm}: only {share} on the grid");
        assert!(
            share > before,
            "{bpm}: snapping didn't help ({before} -> {share})"
        );
    }
}

#[test]
fn snapping_off_changes_nothing() {
    let (_, a) = fight(BeatSnap::default(), 218.0, 10.0);
    let (_, b) = fight(
        BeatSnap {
            enabled: false,
            ..snap()
        },
        218.0,
        10.0,
    );
    let ticks = |w: &[Windup]| {
        w.iter()
            .map(|w| (w.enemy, w.start, w.strike))
            .collect::<Vec<_>>()
    };
    assert_eq!(ticks(&a), ticks(&b));
}

#[test]
fn extra_time_is_bounded() {
    let clock = BeatClock::new(120.0, 4);
    let s = snap();
    let mut snapped = 0;
    for now in 0..240 {
        if let Some(extra) = s.extra(&clock, now as f64, 0.0, 0.45, TICKS_PER_SECOND) {
            assert!((0.0..=s.max_shift).contains(&extra), "{now}: {extra}");
            snapped += 1;
        }
    }
    // At 120 BPM a beat is 0.5 s, so a 0.15 s window catches 30% of starts.
    assert!(snapped > 50, "{snapped}");
    let off = BeatSnap {
        enabled: false,
        ..s
    };
    assert_eq!(off.extra(&clock, 0.0, 0.0, 0.45, TICKS_PER_SECOND), None);
}
