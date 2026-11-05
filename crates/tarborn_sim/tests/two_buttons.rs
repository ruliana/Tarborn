//! The two-button scheme: Jump does jumping, lunging, slamming and bouncing;
//! Grip (sticky) does clinging and, with Jump, coiling.

use glam::Vec2;
use tarborn_sim::{DT, Enemy, Event, HitKind, Intent, Level, Sim, Solid, TarModes};

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

fn high_in_the_air() -> Sim {
    let mut sim = Sim::new(flat_floor(), v(0.0, 8.0));
    run(&mut sim, Intent::default(), 0.2);
    sim
}

#[test]
fn jump_pressed_in_the_air_lunges() {
    let mut sim = high_in_the_air();
    let events = sim.step(press(v(1.0, 0.0)));
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
    // Fall until about 50 ms from the floor.
    while sim.blob.center().y - sim.blob.params.radius > -sim.blob.velocity().y * 0.05 {
        sim.step(Intent::default());
    }
    let mut events = sim.step(press(Vec2::ZERO));
    events.extend(run(
        &mut sim,
        Intent {
            jump_held: true,
            ..Default::default()
        },
        0.2,
    ));
    assert!(
        !events.iter().any(|e| matches!(e, Event::Lunged { .. })),
        "{events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(e, Event::Jumped { .. })),
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

#[test]
fn holding_grip_and_jump_coils_instead_of_bouncing() {
    let mut sim = Sim::new(flat_floor(), v(0.0, 0.6));
    run(&mut sim, Intent::default(), 1.0);
    let events = sim.step(Intent {
        jump_pressed: true,
        jump_held: true,
        modes: GRIP,
        ..Default::default()
    });
    assert_eq!(events, vec![Event::CoilStarted]);
}

#[test]
fn releasing_a_coil_near_an_enemy_lunges_at_it() {
    let mut sim = Sim::new(flat_floor(), v(0.0, 0.6));
    run(&mut sim, Intent::default(), 1.0);
    let mut dummy = Enemy::dummy(v(3.0, 0.6));
    dummy.max_health = 1000.0;
    dummy.health = 1000.0;
    sim.enemies.push(dummy);
    sim.step(Intent {
        jump_pressed: true,
        jump_held: true,
        modes: GRIP,
        ..Default::default()
    });
    run(
        &mut sim,
        Intent {
            jump_held: true,
            modes: GRIP,
            ..Default::default()
        },
        0.6,
    );
    // Release aiming roughly toward the dummy: it is close enough that the
    // launch becomes a lunge at it.
    let mut events = sim.step(Intent {
        aim: v(1.0, 0.3),
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
    let damage = damage.expect("the coil lunge missed");
    assert!(
        damage > sim.combat.lunge_damage * 1.5,
        "a full coil should hit harder: {damage}"
    );
}

#[test]
fn releasing_a_coil_with_no_enemy_near_just_launches() {
    let mut sim = Sim::new(flat_floor(), v(0.0, 0.6));
    run(&mut sim, Intent::default(), 1.0);
    sim.step(Intent {
        jump_pressed: true,
        jump_held: true,
        modes: GRIP,
        ..Default::default()
    });
    run(
        &mut sim,
        Intent {
            jump_held: true,
            modes: GRIP,
            ..Default::default()
        },
        0.3,
    );
    let events = sim.step(Intent {
        modes: GRIP,
        ..Default::default()
    });
    assert!(matches!(events[..], [Event::Launched { .. }]), "{events:?}");
    assert!(!sim.controller.is_lunging());
}
