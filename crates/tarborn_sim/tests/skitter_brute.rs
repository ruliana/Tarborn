//! The skitter (fast, dodges lunges) and the brute (heavy, quakes the
//! ground), and how enemies treat each other.

use glam::Vec2;
use tarborn_sim::{
    AttackPhase, DT, Enemy, EnemyKind, Event, HitKind, Intent, Level, Sim, Solid, State, TarModes,
};

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn ticks(seconds: f32) -> usize {
    (seconds / DT).round() as usize
}

fn run(sim: &mut Sim, intent: Intent, seconds: f32) -> Vec<Event> {
    (0..ticks(seconds)).flat_map(|_| sim.step(intent)).collect()
}

const GRIP: TarModes = TarModes {
    sticky: true,
    bouncy: false,
};

fn grip() -> Intent {
    Intent {
        modes: GRIP,
        ..Default::default()
    }
}

fn flat_floor() -> Level {
    Level {
        solids: vec![Solid::rect(v(-100.0, -2.0), v(100.0, 0.0))],
    }
}

fn hurt(events: &[Event]) -> bool {
    events.iter().any(|e| matches!(e, Event::PlayerHurt { .. }))
}

/// The player falling from above, with `enemy` on the floor to the right.
fn above(enemy: Enemy) -> Sim {
    let mut sim = Sim::new(flat_floor(), v(0.0, 3.0));
    sim.enemies.push(enemy);
    run(&mut sim, Intent::default(), 0.15);
    sim
}

fn settled_with(enemies: Vec<Enemy>) -> Sim {
    let mut sim = Sim::new(flat_floor(), v(0.0, 0.6));
    run(&mut sim, Intent::default(), 1.0);
    sim.enemies = enemies;
    sim
}

fn facing(mut enemy: Enemy, direction: f32) -> Enemy {
    enemy.facing = direction;
    enemy
}

/// Lunges at enemy 0 holding `hold`, and returns what happened.
fn lunge(sim: &mut Sim, hold: Intent) -> Vec<Event> {
    let mut events = sim.step(Intent {
        jump_pressed: true,
        ..hold
    });
    assert!(
        events.iter().any(|e| matches!(e, Event::Lunged { .. })),
        "{events:?}"
    );
    events.extend(run(sim, hold, 0.4));
    events
}

fn hit_on(events: &[Event], enemy: usize) -> Option<HitKind> {
    events.iter().find_map(|e| match e {
        Event::Hit { enemy: i, kind, .. } if *i == enemy => Some(*kind),
        _ => None,
    })
}

#[test]
fn a_skitter_hops_out_of_the_way_of_a_lunge() {
    let mut sim = above(Enemy::skitter(v(2.0, 0.3)));
    let events = lunge(&mut sim, Intent::default());
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Evaded { enemy: 0, .. })),
        "{events:?}"
    );
    assert_eq!(hit_on(&events, 0), None, "{events:?}");
    assert_eq!(sim.enemies[0].health, sim.enemies[0].max_health);
}

#[test]
fn a_skitter_hops_out_of_the_way_of_a_grab() {
    let mut sim = above(Enemy::skitter(v(2.0, 0.3)));
    let events = lunge(&mut sim, grip());
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Evaded { enemy: 0, .. })),
        "{events:?}"
    );
    assert_eq!(sim.held, None);
}

#[test]
fn a_slam_lands_on_a_skitter() {
    let mut sim = Sim::new(flat_floor(), v(0.0, 3.0));
    sim.enemies.push(Enemy::skitter(v(0.0, 0.3)));
    run(&mut sim, Intent::default(), 0.1);
    let mut events = sim.step(Intent {
        aim: v(0.0, -1.0),
        jump_pressed: true,
        jump_held: true,
        ..Default::default()
    });
    events.extend(run(&mut sim, Intent::default(), 0.4));
    let kind = hit_on(&events, 0);
    assert!(
        matches!(kind, Some(HitKind::Slam | HitKind::Shockwave)),
        "{events:?}"
    );
}

#[test]
fn a_skitter_after_its_pounce_can_be_lunged() {
    let mut skitter = Enemy::skitter(v(2.0, 0.3));
    skitter.state = State::Attacking(AttackPhase::Recover);
    let mut sim = above(skitter);
    let events = lunge(&mut sim, Intent::default());
    assert_eq!(hit_on(&events, 0), Some(HitKind::Lunge), "{events:?}");
}

#[test]
fn a_skitter_pounces_on_the_player_and_shrinking_does_not_help() {
    for hold in [Intent::default(), grip()] {
        let mut sim = settled_with(vec![facing(Enemy::skitter(v(4.0, 0.3)), -1.0)]);
        let events = run(&mut sim, hold, 2.5);
        assert!(
            events.iter().any(|e| matches!(e, Event::PlayerHurt { .. })),
            "never bit: {:?}",
            sim.enemies[0].state
        );
    }
}

#[test]
fn a_skitter_hunts_grubs() {
    let mut sim = settled_with(vec![
        facing(Enemy::skitter(v(20.0, 0.3)), 1.0),
        Enemy::grub(v(23.5, 0.35)),
    ]);
    let events = run(&mut sim, Intent::default(), 3.0);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Struck { enemy: 1, .. })),
        "{events:?}"
    );
}

#[test]
fn a_skitter_runs_from_a_brute() {
    let mut sim = settled_with(vec![
        Enemy::skitter(v(22.0, 0.3)),
        facing(Enemy::brute(v(20.0, 0.9)), -1.0),
    ]);
    run(&mut sim, Intent::default(), 0.5);
    assert_eq!(sim.enemies[0].state, State::Fleeing);
    run(&mut sim, Intent::default(), 0.5);
    let gap = sim.enemies[0].position.x - sim.enemies[1].position.x;
    assert!(gap > 4.0, "only {gap} away");
}

#[test]
fn guards_attack_skitters_on_sight() {
    let mut sim = settled_with(vec![
        facing(Enemy::guard(v(20.0, 0.6)), 1.0),
        Enemy::skitter(v(23.0, 0.3)),
    ]);
    let events = run(&mut sim, Intent::default(), 3.0);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Struck { enemy: 1, .. })),
        "{events:?}"
    );
}

/// A brute 2 m to the right of a settled player, already after them.
fn facing_a_brute() -> Sim {
    settled_with(vec![facing(Enemy::brute(v(2.4, 0.9)), -1.0)])
}

#[test]
fn a_brute_quake_hurts_a_player_on_the_ground_even_shrunk() {
    let mut sim = facing_a_brute();
    let events = run(&mut sim, grip(), 1.5);
    assert!(
        events.iter().any(|e| matches!(e, Event::Quake { .. })),
        "{events:?}"
    );
    assert!(hurt(&events), "{events:?}");
}

#[test]
fn jumping_over_a_brute_quake_is_safe() {
    let mut sim = facing_a_brute();
    while sim.enemies[0].state != State::Attacking(AttackPhase::Windup) {
        sim.step(Intent::default());
    }
    // Jump near the end of the wind-up.
    run(&mut sim, Intent::default(), 0.7);
    let mut events = sim.step(Intent {
        jump_pressed: true,
        jump_held: true,
        ..Default::default()
    });
    events.extend(run(
        &mut sim,
        Intent {
            jump_held: true,
            ..Default::default()
        },
        0.3,
    ));
    assert!(
        events.iter().any(|e| matches!(e, Event::Quake { .. })),
        "{events:?}"
    );
    assert!(!hurt(&events), "{events:?}");
}

#[test]
fn a_brute_quake_hurts_other_enemies() {
    let mut sim = facing_a_brute();
    sim.enemies.push(Enemy::dummy(v(-1.0, 0.6)));
    sim.enemies.push(Enemy::dummy(v(4.0, 0.6)));
    let events = run(&mut sim, Intent::default(), 1.5);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Struck { enemy: 2, .. })),
        "{events:?}"
    );
}

#[test]
fn a_brute_is_too_heavy_to_grab_or_knock_back() {
    let mut brute = Enemy::brute(v(2.4, 0.9));
    brute.state = State::Attacking(AttackPhase::Recover);
    let mut sim = above(brute);
    let start = sim.enemies[0].position.x;
    let events = lunge(&mut sim, grip());
    assert!(!events.iter().any(|e| matches!(e, Event::Grabbed { .. })));
    assert_eq!(hit_on(&events, 0), Some(HitKind::Lunge), "{events:?}");
    let moved = (sim.enemies[0].position.x - start).abs();
    assert!(moved < 0.5, "knocked {moved}");
}

#[test]
fn a_brute_eats_grubs_to_heal() {
    let mut brute = Enemy::brute(v(20.0, 0.9));
    brute.health = 100.0;
    let mut sim = settled_with(vec![brute, Enemy::grub(v(21.1, 0.35))]);
    let events = run(&mut sim, Intent::default(), 0.5);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Eaten { enemy: 0, prey: 1 })),
        "{events:?}"
    );
    assert!(!sim.enemies[1].alive());
    assert!(sim.enemies[0].health > 100.0);
    assert_eq!(sim.enemies[0].kind, EnemyKind::Brute);
}
