//! The spitter (ranged enemy) and dodging by shrinking.

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

fn flat_floor() -> Level {
    Level {
        solids: vec![Solid::rect(v(-100.0, -2.0), v(100.0, 0.0))],
    }
}

/// Player settled at x = 0, a spitter at `x` facing the player.
fn with_spitter(x: f32) -> Sim {
    let mut sim = Sim::new(flat_floor(), v(0.0, 0.6));
    run(&mut sim, Intent::default(), 1.0);
    let mut spitter = Enemy::spitter(v(x, 0.45));
    spitter.facing = -x.signum();
    sim.enemies.push(spitter);
    sim
}

fn hurt(events: &[Event]) -> bool {
    events.iter().any(|e| matches!(e, Event::PlayerHurt { .. }))
}

#[test]
fn a_spitter_aims_then_shoots_the_player() {
    let mut sim = with_spitter(7.0);
    let mut aimed_at = None;
    let mut hurt_at = None;
    for t in 0..ticks(3.0) {
        let events = sim.step(Intent::default());
        if aimed_at.is_none() && sim.enemies[0].state == State::Attacking(AttackPhase::Windup) {
            aimed_at = Some(t);
        }
        if hurt(&events) {
            hurt_at = Some(t);
            break;
        }
    }
    let aimed = aimed_at.expect("never aimed");
    let hit = hurt_at.expect("never hit the player");
    let warning = (hit - aimed) as f32 * DT;
    assert!(warning > 0.9, "only {warning}s from aiming to being hit");
}

#[test]
fn shrinking_makes_spit_miss() {
    let mut sim = with_spitter(7.0);
    // Wait until it fires, holding Grip: the blob stays shrunk.
    while sim.projectiles.is_empty() {
        sim.step(Intent {
            modes: STICKY,
            ..Default::default()
        });
    }
    let events = run(
        &mut sim,
        Intent {
            modes: STICKY,
            ..Default::default()
        },
        0.7,
    );
    assert!(
        events.iter().any(|e| matches!(e, Event::Dodged { .. })),
        "{events:?}"
    );
    assert!(!hurt(&events));
}

#[test]
fn shrinking_makes_a_guard_swing_miss() {
    let mut sim = Sim::new(flat_floor(), v(0.0, 0.6));
    run(&mut sim, Intent::default(), 1.0);
    let mut guard = Enemy::guard(v(1.6, 0.6));
    guard.facing = -1.0;
    guard.state = State::Defending;
    guard.target = Some(sim.blob.center());
    sim.enemies.push(guard);
    let events = run(
        &mut sim,
        Intent {
            modes: STICKY,
            ..Default::default()
        },
        1.2,
    );
    assert!(
        events.iter().any(|e| matches!(e, Event::Dodged { .. })),
        "{events:?}"
    );
    assert!(!hurt(&events));
}

#[test]
fn shrink_dodge_then_launch_into_the_opening() {
    // The combo: shrink under the shot, then expand (Jump) to burst at the
    // spitter while it recovers, for double damage.
    let mut sim = with_spitter(4.5);
    while sim.projectiles.is_empty() {
        sim.step(Intent {
            modes: STICKY,
            ..Default::default()
        });
    }
    run(
        &mut sim,
        Intent {
            modes: STICKY,
            ..Default::default()
        },
        0.4,
    );
    assert_eq!(sim.enemies[0].state, State::Attacking(AttackPhase::Recover));
    let toward = (sim.enemies[0].position - sim.blob.center()).normalize();
    let mut events = sim.step(Intent {
        aim: toward,
        jump_pressed: true,
        modes: STICKY,
        ..Default::default()
    });
    events.extend(run(&mut sim, Intent::default(), 0.4));
    let hit = events.iter().find_map(|e| match e {
        Event::Hit { kind, damage, .. } => Some((*kind, *damage)),
        _ => None,
    });
    let (kind, damage) = hit.unwrap_or_else(|| {
        panic!(
            "the burst missed: {events:?} {:?} {:?}",
            sim.blob.center(),
            sim.enemies[0].position
        )
    });
    assert_eq!(kind, HitKind::Lunge, "{events:?}");
    let base = sim.combat.lunge_damage;
    assert!(damage >= base * 2.0, "no recovery bonus: {damage}");
}

#[test]
fn a_spitter_backs_away_from_a_close_player() {
    let mut sim = with_spitter(2.5);
    run(&mut sim, Intent::default(), 1.5);
    let gap = sim.enemies[0].position.x - sim.blob.center().x;
    assert!(gap > 3.0, "stayed {gap} away");
}

#[test]
fn spit_stops_at_walls() {
    let mut sim = with_spitter(7.0);
    sim.level.solids.push(Solid::rect(v(3.0, 0.0), v(3.5, 1.0)));
    // The wall is low: the spitter still sees over it... but its shots go
    // straight at the player and hit the wall.
    let events = run(&mut sim, Intent::default(), 3.0);
    assert!(!hurt(&events), "shot went through the wall");
}

#[test]
fn lunging_through_spit_is_safe() {
    let mut sim = with_spitter(7.0);
    while sim.projectiles.is_empty() {
        sim.step(Intent::default());
    }
    // Wait until the shot is close, then lunge at the spitter.
    while sim.projectiles[0].position.x > 1.8 {
        sim.step(Intent::default());
    }
    let mut events = sim.step(Intent {
        aim: v(1.0, 0.0),
        lunge_pressed: true,
        ..Default::default()
    });
    events.extend(run(&mut sim, Intent::default(), 0.15));
    assert!(!hurt(&events), "{events:?}");
}
