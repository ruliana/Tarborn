//! Behavioural tests for enemy states, sight and hearing.

use glam::Vec2;
use tarborn_sim::{
    AttackPhase, DT, Enemy, Event, HitKind, Intent, Level, Sim, Solid, State, TarModes,
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

const STICKY: TarModes = TarModes {
    sticky: true,
    bouncy: false,
};
const BOUNCY: TarModes = TarModes {
    sticky: false,
    bouncy: true,
};

fn flat_floor() -> Level {
    Level {
        solids: vec![Solid::rect(v(-100.0, -2.0), v(100.0, 0.0))],
    }
}

/// Player settled at x = 0, and a guard at `x` facing `facing`.
fn with_guard(x: f32, facing: f32) -> Sim {
    let mut sim = Sim::new(flat_floor(), v(0.0, 0.6));
    run(&mut sim, Intent::default(), 1.0);
    let mut guard = Enemy::guard(v(x, 0.6));
    guard.facing = facing;
    sim.enemies.push(guard);
    sim
}

fn lunge(aim: Vec2) -> Intent {
    Intent {
        aim,
        lunge_pressed: true,
        ..Default::default()
    }
}

fn hits(events: &[Event]) -> Vec<(HitKind, f32)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Hit { kind, damage, .. } => Some((*kind, *damage)),
            _ => None,
        })
        .collect()
}

#[test]
fn an_unaware_guard_patrols_around_its_post() {
    let mut sim = with_guard(20.0, 1.0);
    let mut xs = Vec::new();
    for _ in 0..ticks(8.0) {
        sim.step(Intent::default());
        xs.push(sim.enemies[0].position.x);
    }
    let (lo, hi) = xs
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(*x), hi.max(*x)));
    assert!(hi - lo > 4.0, "barely moved: {lo}..{hi}");
    assert!(lo > 16.0 && hi < 24.0, "wandered off: {lo}..{hi}");
    assert_eq!(sim.enemies[0].state, State::Unaware);
}

#[test]
fn a_guard_does_not_walk_off_a_ledge() {
    let level = Level {
        solids: vec![Solid::rect(v(-1.5, -2.0), v(1.5, 0.0))],
    };
    let mut sim = Sim::new(level, v(30.0, 0.6));
    sim.enemies.push(Enemy::guard(v(0.0, 0.6)));
    run(&mut sim, Intent::default(), 6.0);
    assert!(sim.enemies[0].position.y > 0.0, "fell off");
}

#[test]
fn a_guard_sees_the_player_in_front() {
    let mut sim = with_guard(5.0, -1.0);
    run(&mut sim, Intent::default(), 0.1);
    assert_eq!(sim.enemies[0].state, State::Defending);
}

#[test]
fn a_guard_does_not_see_behind_itself() {
    let mut sim = with_guard(5.0, 1.0);
    // Stand still for a moment: no noise, out of sight.
    sim.enemies[0].home = v(8.0, 0.6);
    run(&mut sim, Intent::default(), 0.3);
    assert_eq!(sim.enemies[0].state, State::Unaware);
}

#[test]
fn a_guard_cannot_see_through_walls() {
    let mut sim = with_guard(5.0, -1.0);
    sim.level.solids.push(Solid::rect(v(2.0, 0.0), v(2.5, 4.0)));
    run(&mut sim, Intent::default(), 0.3);
    assert_ne!(sim.enemies[0].state, State::Defending);
}

#[test]
fn a_bounce_behind_a_guard_makes_it_suspicious() {
    let mut sim = Sim::new(flat_floor(), v(0.0, 3.0));
    let mut guard = Enemy::guard(v(4.0, 0.6));
    guard.facing = 1.0;
    sim.enemies.push(guard);
    run(
        &mut sim,
        Intent {
            modes: BOUNCY,
            ..Default::default()
        },
        0.8,
    );
    let guard = &sim.enemies[0];
    assert!(
        matches!(guard.state, State::Suspicious | State::Defending),
        "{:?}",
        guard.state
    );
    assert_eq!(guard.facing, -1.0, "did not turn toward the noise");
}

#[test]
fn crawling_with_sticky_is_silent() {
    let mut sim = with_guard(6.0, 1.0);
    sim.enemies[0].home = v(9.0, 0.6);
    run(
        &mut sim,
        Intent {
            roll: 1.0,
            modes: STICKY,
            ..Default::default()
        },
        1.0,
    );
    assert!(sim.noises.is_empty(), "{:?}", sim.noises);
    assert_eq!(sim.enemies[0].state, State::Unaware);
}

#[test]
fn a_surprise_hit_does_triple_damage() {
    let mut sim = with_guard(2.1, 1.0);
    sim.enemies[0].home = v(4.0, 0.6);
    let mut events = sim.step(lunge(v(1.0, 0.0)));
    events.extend(run(&mut sim, Intent::default(), 0.3));
    let lunge_damage = sim.combat.lunge_damage;
    assert_eq!(
        hits(&events),
        vec![(HitKind::Lunge, lunge_damage * 3.0)],
        "{events:?} {:?}",
        sim.enemies[0]
    );
    assert_eq!(sim.enemies[0].state, State::Surprised);
}

/// A guard that has seen the player and has its shield up, facing left.
fn alert_guard(x: f32) -> Sim {
    let mut sim = with_guard(x, -1.0);
    sim.enemies[0].state = State::Defending;
    sim.enemies[0].target = Some(sim.blob.center());
    sim
}

#[test]
fn spotting_the_player_up_close_startles_a_guard() {
    let mut sim = with_guard(2.5, -1.0);
    run(&mut sim, Intent::default(), 0.05);
    assert_eq!(sim.enemies[0].state, State::Surprised);
}

#[test]
fn a_defending_guard_blocks_hits_from_the_front() {
    let mut sim = alert_guard(2.4);
    let mut events = sim.step(lunge(v(1.0, 0.0)));
    events.extend(run(&mut sim, Intent::default(), 0.2));
    assert!(
        events.iter().any(|e| matches!(e, Event::Blocked { .. })),
        "{events:?}"
    );
    assert!(hits(&events).is_empty());
    assert_eq!(sim.enemies[0].health, sim.enemies[0].max_health);
    assert!(sim.blob.velocity().x < 0.0, "not bounced back");
}

#[test]
fn a_slam_breaks_a_guards_defence() {
    let mut sim = alert_guard(2.0);
    // Teleport above the guard and slam onto it.
    for p in &mut sim.blob.positions {
        *p += v(2.0, 3.0);
    }
    sim.step(Intent::default());
    let mut events = sim.step(lunge(v(0.0, -1.0)));
    events.extend(run(&mut sim, Intent::default(), 0.4));
    assert!(
        hits(&events).iter().any(|(k, _)| *k == HitKind::Slam),
        "{events:?}"
    );
    assert_eq!(sim.enemies[0].state, State::Dizzy);
}

#[test]
fn a_guard_winds_up_before_it_swings() {
    let mut sim = alert_guard(1.6);
    let mut windup_at = None;
    let mut hurt_at = None;
    for t in 0..ticks(2.0) {
        let events = sim.step(Intent::default());
        if windup_at.is_none() && sim.enemies[0].state == State::Attacking(AttackPhase::Windup) {
            windup_at = Some(t);
        }
        if events.iter().any(|e| matches!(e, Event::PlayerHurt { .. })) {
            hurt_at = Some(t);
            break;
        }
    }
    let windup = windup_at.expect("never wound up");
    let hurt = hurt_at.expect("never hit the player");
    let warning = (hurt - windup) as f32 * DT;
    assert!(warning >= 0.4, "only {warning}s of warning");
    assert_eq!(sim.health, sim.max_health - 1);
}

#[test]
fn hitting_a_guard_during_its_windup_makes_it_dizzy() {
    let mut sim = alert_guard(1.6);
    while sim.enemies[0].state != State::Attacking(AttackPhase::Windup) {
        sim.step(Intent::default());
    }
    // The shield covers the front even mid-windup, so hit it from behind.
    sim.enemies[0].facing = 1.0;
    let mut events = sim.step(lunge(v(1.0, 0.0)));
    events.extend(run(&mut sim, Intent::default(), 0.1));
    assert!(!hits(&events).is_empty(), "{events:?}");
    assert_eq!(sim.enemies[0].state, State::Dizzy);
}

#[test]
fn a_guard_is_open_while_recovering_from_a_swing() {
    let mut sim = alert_guard(1.6);
    while sim.enemies[0].state != State::Attacking(AttackPhase::Recover) {
        sim.step(Intent::default());
    }
    // The swing knocked the player back; come back in with a lunge from the front.
    sim.controller.refresh_lunge();
    let start = sim.blob.center();
    let toward = (sim.enemies[0].position - start).normalize();
    let mut events = sim.step(lunge(toward));
    events.extend(run(&mut sim, Intent::default(), 0.3));
    assert!(!hits(&events).is_empty(), "{events:?}");
    assert!(!events.iter().any(|e| matches!(e, Event::Blocked { .. })));
}

#[test]
fn a_guard_turns_around_only_after_a_moment() {
    let mut sim = alert_guard(4.0);
    sim.enemies[0].facing = 1.0; // player is behind it
    run(&mut sim, Intent::default(), 0.2);
    assert_eq!(sim.enemies[0].facing, 1.0, "turned instantly");
    run(&mut sim, Intent::default(), 0.4);
    assert_eq!(sim.enemies[0].facing, -1.0, "never turned");
}

#[test]
fn a_guard_searches_after_losing_sight_then_gives_up() {
    let mut sim = alert_guard(6.0);
    // Vanish: move the player far away behind a wall.
    sim.level
        .solids
        .push(Solid::rect(v(-3.0, 0.0), v(-2.5, 6.0)));
    for p in &mut sim.blob.positions {
        *p += v(-10.0, 0.0);
    }
    let mut saw_searching = false;
    for _ in 0..ticks(10.0) {
        sim.step(Intent::default());
        saw_searching |= sim.enemies[0].state == State::Searching;
    }
    assert!(saw_searching);
    assert_eq!(sim.enemies[0].state, State::Unaware);
}

#[test]
fn a_grub_runs_away_from_the_player() {
    let mut sim = Sim::new(flat_floor(), v(0.0, 0.6));
    run(&mut sim, Intent::default(), 1.0);
    let mut grub = Enemy::grub(v(2.0, 0.35));
    grub.facing = -1.0;
    sim.enemies.push(grub);
    let events = run(&mut sim, Intent::default(), 2.0);
    let grub = &sim.enemies[0];
    assert!(grub.position.x > 5.0, "only got to {}", grub.position.x);
    assert!(!events.iter().any(|e| matches!(e, Event::PlayerHurt { .. })));
}

#[test]
fn a_slammed_grub_is_knocked_prone() {
    let mut sim = Sim::new(flat_floor(), v(0.0, 4.0));
    let mut grub = Enemy::grub(v(1.5, 0.35));
    grub.max_health = 1000.0;
    grub.health = 1000.0;
    grub.state = State::Fleeing;
    sim.enemies.push(grub);
    run(&mut sim, Intent::default(), 0.1);
    let mut events = sim.step(lunge(v(0.0, -1.0)));
    events.extend(run(&mut sim, Intent::default(), 0.5));
    assert!(!hits(&events).is_empty(), "{events:?}");
    assert_eq!(sim.enemies[0].state, State::Prone);
}
