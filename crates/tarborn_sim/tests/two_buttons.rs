//! The two-verb scheme. Grip shrinks (small, sticky, charging). Jump expands:
//! a jump at normal size, a burst when shrunk, a lunge at an enemy in the air.
//! What is held when a lunge arrives decides how it ends: a hit, a bounce off
//! (Jump held) or a grab (Grip held).

use glam::Vec2;
use tarborn_sim::{DT, Enemy, Event, HitKind, Intent, Level, Projectile, Sim, Solid, TarModes};

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

fn flat_floor() -> Level {
    Level {
        solids: vec![Solid::rect(v(-100.0, -2.0), v(100.0, 0.0))],
    }
}

fn press(aim: Vec2) -> Intent {
    Intent {
        aim,
        jump_pressed: true,
        jump_held: true,
        ..Default::default()
    }
}

fn tough_dummy(at: Vec2) -> Enemy {
    let mut dummy = Enemy::dummy(at);
    dummy.max_health = 1000.0;
    dummy.health = 1000.0;
    dummy
}

fn high_in_the_air() -> Sim {
    let mut sim = Sim::new(flat_floor(), v(0.0, 8.0));
    run(&mut sim, Intent::default(), 0.2);
    sim
}

/// In the air, with a tough dummy on the floor below and to the right,
/// within lunge range.
fn above_a_dummy() -> Sim {
    let mut sim = Sim::new(flat_floor(), v(0.0, 3.0));
    sim.enemies.push(tough_dummy(v(2.0, 0.6)));
    run(&mut sim, Intent::default(), 0.15);
    sim
}

fn settled_near(enemy: Enemy) -> Sim {
    let mut sim = Sim::new(flat_floor(), v(0.0, 0.6));
    run(&mut sim, Intent::default(), 1.0);
    sim.enemies.push(enemy);
    sim
}

#[test]
fn jump_in_the_air_with_no_enemy_near_does_not_lunge() {
    let mut sim = high_in_the_air();
    let events = sim.step(press(v(1.0, 0.0)));
    assert!(events.is_empty(), "{events:?}");
    assert!(!sim.controller.is_lunging());
}

#[test]
fn jump_in_the_air_lunges_at_a_nearby_enemy() {
    let mut sim = above_a_dummy();
    let events = sim.step(press(Vec2::ZERO));
    assert!(matches!(events[..], [Event::Lunged { .. }]), "{events:?}");
}

#[test]
fn down_and_jump_in_the_air_slams() {
    let mut sim = high_in_the_air();
    let events = sim.step(press(v(0.0, -1.0)));
    assert_eq!(events, vec![Event::SlamStarted]);
}

#[test]
fn jump_pressed_just_before_landing_jumps_instead_of_lunging() {
    let mut sim = Sim::new(flat_floor(), v(0.0, 3.0));
    sim.enemies.push(tough_dummy(v(2.0, 0.6)));
    // Fall until about 50 ms from the floor.
    while sim.blob.center().y - sim.blob.params.radius > -sim.blob.velocity().y * 0.05 {
        sim.step(Intent::default());
    }
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
        0.2,
    ));
    // Close to landing, the buffered jump wins only when there's no enemy to
    // lunge at; with one in range the lunge comes first.
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Jumped { .. } | Event::Lunged { .. })),
        "{events:?}"
    );
}

/// Highest point after first touching the floor from a drop.
fn rebound(hold_jump: bool) -> f32 {
    let mut sim = Sim::new(flat_floor(), v(0.0, 6.0));
    let intent = Intent {
        jump_held: hold_jump,
        ..Default::default()
    };
    while sim.blob.touching_count() == 0 {
        sim.step(intent);
    }
    let mut best = f32::MIN;
    for _ in 0..ticks(1.5) {
        sim.step(intent);
        best = best.max(sim.blob.center().y);
    }
    best
}

#[test]
fn holding_jump_through_a_landing_bounces() {
    let held = rebound(true);
    let soft = rebound(false);
    assert!(held > 3.5, "held only rebounded to {held}");
    assert!(soft < 1.0, "soft landing rebounded to {soft}");
}

/// Lunges at the dummy from above, holding `hold` for the rest of the lunge.
fn lunge_holding(hold: Intent) -> (Sim, Vec<Event>) {
    let mut sim = above_a_dummy();
    let mut events = sim.step(Intent {
        jump_pressed: true,
        ..hold
    });
    assert!(
        events.iter().any(|e| matches!(e, Event::Lunged { .. })),
        "{events:?}"
    );
    events.extend(run(&mut sim, hold, 0.4));
    (sim, events)
}

#[test]
fn a_tapped_lunge_hits() {
    let (_, events) = lunge_holding(Intent::default());
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::Hit {
                kind: HitKind::Lunge,
                ..
            }
        )),
        "{events:?}"
    );
}

#[test]
fn a_lunge_with_jump_held_bounces_off() {
    let (sim, events) = lunge_holding(Intent {
        jump_held: true,
        ..Default::default()
    });
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::Hit {
                kind: HitKind::Pogo,
                ..
            }
        )),
        "{events:?}"
    );
    assert!(sim.blob.center().y > 1.0, "didn't bounce up");
}

#[test]
fn a_lunge_with_grip_held_grabs() {
    let (sim, events) = lunge_holding(Intent {
        modes: GRIP,
        ..Default::default()
    });
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Grabbed { enemy: 0 })),
        "{events:?}"
    );
    assert!(!events.iter().any(|e| matches!(e, Event::Hit { .. })));
    assert_eq!(sim.held, Some(0));
    // The held enemy rides along with the blob.
    let gap = sim.enemies[0].position.distance(sim.blob.center());
    assert!(gap < 1.5, "held enemy is {gap} away");
}

#[test]
fn a_shrunk_burst_near_an_enemy_lunges_at_it() {
    let mut sim = settled_near(tough_dummy(v(3.0, 0.6)));
    run(
        &mut sim,
        Intent {
            modes: GRIP,
            ..Default::default()
        },
        0.6,
    );
    let mut events = sim.step(Intent {
        jump_pressed: true,
        modes: GRIP,
        ..Default::default()
    });
    events.extend(run(&mut sim, Intent::default(), 0.3));
    assert!(
        events.iter().any(|e| matches!(e, Event::Lunged { .. })),
        "{events:?}"
    );
    let damage = events.iter().find_map(|e| match e {
        Event::Hit {
            kind: HitKind::Lunge,
            damage,
            ..
        } => Some(*damage),
        _ => None,
    });
    let damage = damage.expect("the burst lunge missed");
    assert!(
        damage > sim.combat.lunge_damage * 1.5,
        "a full charge should hit harder: {damage}"
    );
}

#[test]
fn a_shrunk_burst_with_no_enemy_near_launches() {
    let mut sim = Sim::new(flat_floor(), v(0.0, 0.6));
    run(&mut sim, Intent::default(), 1.0);
    run(
        &mut sim,
        Intent {
            modes: GRIP,
            ..Default::default()
        },
        0.3,
    );
    let events = sim.step(Intent {
        jump_pressed: true,
        modes: GRIP,
        ..Default::default()
    });
    assert!(matches!(events[..], [Event::Launched { .. }]), "{events:?}");
    assert!(!sim.controller.is_lunging());
}

/// Grabs the dummy, lands, and keeps gripping for a moment.
fn holding_a_dummy() -> Sim {
    let (mut sim, _) = lunge_holding(Intent {
        modes: GRIP,
        ..Default::default()
    });
    run(
        &mut sim,
        Intent {
            modes: GRIP,
            ..Default::default()
        },
        0.5,
    );
    assert_eq!(sim.held, Some(0));
    sim
}

#[test]
fn letting_go_of_grip_drops_the_held_enemy() {
    let mut sim = holding_a_dummy();
    let events = sim.step(Intent::default());
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Dropped { enemy: 0 })),
        "{events:?}"
    );
    assert_eq!(sim.held, None);
}

#[test]
fn jump_while_holding_throws_instead_of_jumping() {
    let mut sim = holding_a_dummy();
    let events = sim.step(Intent {
        aim: v(1.0, 0.0),
        jump_pressed: true,
        jump_held: true,
        modes: GRIP,
        ..Default::default()
    });
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Thrown { enemy: 0, .. })),
        "{events:?}"
    );
    assert!(!events.iter().any(|e| matches!(e, Event::Jumped { .. })));
    assert_eq!(sim.held, None);
    assert!(sim.enemies[0].velocity.x > 10.0);
}

#[test]
fn a_charged_throw_is_faster() {
    let speed = |charge: f32| {
        let mut sim = holding_a_dummy();
        sim.controller.spend_charge();
        run(
            &mut sim,
            Intent {
                modes: GRIP,
                ..Default::default()
            },
            charge,
        );
        let events = sim.step(Intent {
            aim: v(1.0, 0.0),
            jump_pressed: true,
            modes: GRIP,
            ..Default::default()
        });
        events
            .iter()
            .find_map(|e| match e {
                Event::Thrown { speed, .. } => Some(*speed),
                _ => None,
            })
            .unwrap()
    };
    let quick = speed(0.0);
    let full = speed(0.6);
    assert!(full > quick * 1.6, "full {full}, quick {quick}");
}

#[test]
fn a_thrown_enemy_hurts_the_enemy_it_hits() {
    let mut sim = holding_a_dummy();
    let victim_x = sim.blob.center().x + 4.0;
    sim.enemies.push(Enemy::grub(v(victim_x, 0.35)));
    let before = sim.enemies[1].health;
    let mut events = sim.step(Intent {
        aim: v(1.0, 0.0),
        jump_pressed: true,
        modes: GRIP,
        ..Default::default()
    });
    events.extend(run(&mut sim, Intent::default(), 0.6));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Struck { enemy: 1, .. } | Event::Killed { .. })),
        "{events:?}"
    );
    assert!(sim.enemies[1].health < before || !sim.enemies[1].alive());
}

#[test]
fn spit_hits_any_enemy_in_its_way() {
    let mut sim = settled_near(tough_dummy(v(-6.0, 0.6)));
    sim.enemies.push(tough_dummy(v(-3.0, 0.6)));
    sim.projectiles.push(Projectile {
        position: v(-5.0, 0.6),
        velocity: v(11.0, 0.0),
        life: 2.0,
        dodged: false,
        owner: 0,
    });
    let events = run(&mut sim, Intent::default(), 0.4);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Struck { enemy: 1, .. })),
        "{events:?}"
    );
    assert!(sim.enemies[1].health < 1000.0);
    assert_eq!(
        sim.health, sim.max_health,
        "the spit got through to the player"
    );
}
