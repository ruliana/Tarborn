//! Forgiveness in the Grip verb (Ronie's playtest, 2026-11-07): sticking to
//! walls and ceilings mid-jump, grabbing by touch, dodging late, and
//! directions that follow the screen while stuck to a wall or ceiling.

use glam::Vec2;
use tarborn_sim::{DT, Enemy, Event, Intent, Level, Projectile, Sim, Solid, TarModes};

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn ticks(seconds: f32) -> usize {
    (seconds / DT).round() as usize
}

fn run(sim: &mut Sim, intent: Intent, seconds: f32) -> Vec<Event> {
    (0..ticks(seconds)).flat_map(|_| sim.step(intent)).collect()
}

fn grip(sticky: bool) -> TarModes {
    TarModes {
        sticky,
        bouncy: false,
    }
}

/// A floor, a wall on the right at x = 3 and a ceiling on the left at y = 3.
fn room() -> Level {
    Level {
        solids: vec![
            Solid::rect(v(-100.0, -2.0), v(100.0, 0.0)),
            Solid::rect(v(3.0, 0.0), v(5.0, 20.0)),
            Solid::rect(v(-100.0, 3.0), v(-1.0, 5.0)),
        ],
    }
}

fn settled(level: Level, x: f32) -> Sim {
    let mut sim = Sim::new(level, v(x, 0.6));
    run(&mut sim, Intent::default(), 1.0);
    sim
}

fn on_ceiling(sim: &Sim) -> bool {
    sim.blob.contacts.iter().flatten().any(|n| n.y < -0.5)
}

fn on_right_wall(sim: &Sim) -> bool {
    sim.blob.contacts.iter().flatten().any(|n| n.x < -0.5)
}

/// Jumps straight up under the ceiling holding Jump (so the body is bouncy),
/// pressing Grip `grip_at` ticks into the jump.
fn jump_at_ceiling(grip_at: usize) -> (Sim, Vec<Event>) {
    let mut sim = settled(room(), -3.0);
    let mut events = Vec::new();
    for t in 0..ticks(1.2) {
        events.extend(sim.step(Intent {
            jump_pressed: t == 0,
            jump_held: true,
            modes: grip(t >= grip_at),
            ..Default::default()
        }));
    }
    (sim, events)
}

/// The tick a Grip-less jump bounces off the ceiling.
fn ceiling_bounce_tick() -> usize {
    let (_, events) = jump_at_ceiling(usize::MAX);
    let mut probe = settled(room(), -3.0);
    assert!(events.iter().any(|e| matches!(e, Event::Bounced { .. })));
    (0..ticks(1.0))
        .position(|t| {
            probe
                .step(Intent {
                    jump_pressed: t == 0,
                    jump_held: true,
                    ..Default::default()
                })
                .iter()
                .any(|e| matches!(e, Event::Bounced { .. }))
        })
        .expect("holding Jump bounces off the ceiling")
}

#[test]
fn grip_before_reaching_the_ceiling_sticks() {
    let (sim, events) = jump_at_ceiling(10);
    assert!(on_ceiling(&sim), "{:?}", sim.blob.center());
    assert!(!events.iter().any(|e| matches!(e, Event::Bounced { .. })));
}

#[test]
fn grip_a_moment_after_bouncing_off_the_ceiling_still_sticks() {
    let (sim, _) = jump_at_ceiling(ceiling_bounce_tick() + ticks(0.1));
    assert!(on_ceiling(&sim), "{:?}", sim.blob.center());
}

#[test]
fn grip_long_after_a_bounce_does_not_pull_back() {
    let (sim, _) = jump_at_ceiling(ceiling_bounce_tick() + ticks(0.4));
    assert!(!on_ceiling(&sim), "{:?}", sim.blob.center());
}

#[test]
fn a_burst_into_a_low_ceiling_sticks_right_away() {
    // The ceiling is so close the burst reaches it while the floor's grip is
    // still let go. Only the floor lets go; the ceiling grips.
    let level = Level {
        solids: vec![
            Solid::rect(v(-100.0, -2.0), v(100.0, 0.0)),
            Solid::rect(v(-100.0, 1.6), v(100.0, 3.0)),
        ],
    };
    let mut sim = settled(level, 0.0);
    run(
        &mut sim,
        Intent {
            modes: grip(true),
            ..Default::default()
        },
        0.2,
    );
    sim.step(Intent {
        aim: v(0.0, 1.0),
        jump_pressed: true,
        jump_held: true,
        modes: grip(true),
        ..Default::default()
    });
    run(
        &mut sim,
        Intent {
            aim: v(0.0, 1.0),
            modes: grip(true),
            ..Default::default()
        },
        0.8,
    );
    assert!(on_ceiling(&sim), "{:?}", sim.blob.center());
}

fn tough_dummy(at: Vec2) -> Enemy {
    let mut dummy = Enemy::dummy(at);
    dummy.max_health = 1000.0;
    dummy.health = 1000.0;
    dummy
}

fn grabbed(events: &[Event]) -> bool {
    events.iter().any(|e| matches!(e, Event::Grabbed { .. }))
}

fn flat() -> Level {
    Level {
        solids: vec![Solid::rect(v(-100.0, -2.0), v(100.0, 0.0))],
    }
}

#[test]
fn landing_on_an_enemy_with_grip_held_grabs_it() {
    let mut sim = Sim::new(flat(), v(0.0, 3.0));
    sim.enemies.push(tough_dummy(v(0.0, 0.6)));
    let events = run(
        &mut sim,
        Intent {
            modes: grip(true),
            ..Default::default()
        },
        1.0,
    );
    assert!(grabbed(&events), "{events:?}");
    assert_eq!(sim.held, Some(0));
}

#[test]
fn pressing_grip_while_resting_on_an_enemy_grabs_it() {
    let mut sim = Sim::new(flat(), v(0.0, 3.0));
    sim.enemies.push(tough_dummy(v(0.0, 0.6)));
    let events = run(&mut sim, Intent::default(), 0.8);
    assert!(!grabbed(&events));
    let events = run(
        &mut sim,
        Intent {
            modes: grip(true),
            ..Default::default()
        },
        0.2,
    );
    assert!(grabbed(&events), "{events:?}");
}

#[test]
fn pressing_grip_beside_an_enemy_on_the_ground_grabs_it() {
    let mut sim = settled(flat(), 0.0);
    sim.enemies.push(tough_dummy(v(1.15, 0.6)));
    let events = run(
        &mut sim,
        Intent {
            modes: grip(true),
            ..Default::default()
        },
        0.2,
    );
    assert!(grabbed(&events), "{events:?}");
}

#[test]
fn rolling_shrunk_into_an_enemy_does_not_grab_it() {
    let mut sim = settled(flat(), 0.0);
    sim.enemies.push(tough_dummy(v(3.0, 0.6)));
    let events = run(
        &mut sim,
        Intent {
            roll: 1.0,
            aim: v(1.0, 0.0),
            modes: grip(true),
            ..Default::default()
        },
        2.0,
    );
    assert!(!grabbed(&events), "{events:?}");
}

/// A glob of spit that reaches the resting blob after `seconds`.
fn spit_arriving_in(sim: &mut Sim, seconds: f32) {
    let edge = sim.blob.center().x - sim.blob.params.radius;
    sim.projectiles.push(Projectile {
        position: v(edge - 11.0 * seconds, 0.5),
        velocity: v(11.0, 0.0),
        life: 2.0,
        dodged: false,
        owner: usize::MAX,
    });
}

fn hurt(events: &[Event]) -> bool {
    events.iter().any(|e| matches!(e, Event::PlayerHurt { .. }))
}

fn dodged(events: &[Event]) -> bool {
    events.iter().any(|e| matches!(e, Event::Dodged { .. }))
}

/// Spit hits at 0.3 s; Grip goes down at `grip_at` seconds.
fn spit_with_grip_at(grip_at: f32) -> Vec<Event> {
    let mut sim = settled(flat(), 0.0);
    spit_arriving_in(&mut sim, 0.3);
    let mut events = Vec::new();
    for t in 0..ticks(1.0) {
        let modes = grip(t as f32 * DT >= grip_at);
        events.extend(sim.step(Intent {
            modes,
            ..Default::default()
        }));
    }
    events
}

#[test]
fn grip_pressed_just_before_a_hit_dodges() {
    let events = spit_with_grip_at(0.1);
    assert!(dodged(&events) && !hurt(&events), "{events:?}");
}

#[test]
fn grip_pressed_just_after_a_hit_still_dodges() {
    let events = spit_with_grip_at(0.35);
    assert!(dodged(&events) && !hurt(&events), "{events:?}");
}

#[test]
fn grip_pressed_well_after_a_hit_is_too_late() {
    let events = spit_with_grip_at(0.6);
    assert!(hurt(&events) && !dodged(&events), "{events:?}");
}

#[test]
fn staying_shrunk_does_not_dodge() {
    let events = spit_with_grip_at(-1.0);
    assert!(hurt(&events), "{events:?}");
}

/// Stuck to the right wall, a little off the floor.
fn on_the_wall() -> Sim {
    let mut sim = settled(room(), 1.5);
    let hold = Intent {
        roll: 1.0,
        aim: v(1.0, 0.0),
        modes: grip(true),
        ..Default::default()
    };
    for _ in 0..ticks(1.5) {
        sim.step(hold);
        if on_right_wall(&sim) && sim.blob.center().y > 2.5 {
            break;
        }
    }
    run(
        &mut sim,
        Intent {
            modes: grip(true),
            ..Default::default()
        },
        0.3,
    );
    assert!(on_right_wall(&sim), "{:?}", sim.blob.center());
    sim
}

fn climb(mut sim: Sim, aim: Vec2) -> f32 {
    let from = sim.blob.center().y;
    let intent = Intent {
        roll: aim.x,
        aim,
        modes: grip(true),
        ..Default::default()
    };
    run(&mut sim, intent, 0.5);
    sim.blob.center().y - from
}

#[test]
fn up_climbs_a_wall_and_down_goes_down_it() {
    assert!(climb(on_the_wall(), v(0.0, 1.0)) > 1.0);
    assert!(climb(on_the_wall(), v(0.0, -1.0)) < -0.5);
}

#[test]
fn pushing_into_a_wall_climbs_it_and_pulling_away_goes_down() {
    assert!(climb(on_the_wall(), v(1.0, 0.0)) > 1.0);
    assert!(climb(on_the_wall(), v(-1.0, 0.0)) < -0.5);
}

#[test]
fn left_on_a_ceiling_goes_left() {
    let (mut sim, _) = jump_at_ceiling(10);
    assert!(on_ceiling(&sim));
    let from = sim.blob.center().x;
    run(
        &mut sim,
        Intent {
            roll: -1.0,
            aim: v(-1.0, 0.0),
            modes: grip(true),
            ..Default::default()
        },
        0.4,
    );
    assert!(on_ceiling(&sim));
    assert!(
        sim.blob.center().x < from - 1.0,
        "{from} -> {:?}",
        sim.blob.center()
    );
}
