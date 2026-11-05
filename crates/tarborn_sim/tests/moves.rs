//! Behavioural tests for the lunge and slam.

use glam::Vec2;
use tarborn_sim::{DT, Event, Intent, Level, Sim, Solid, TarModes};

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn ticks(seconds: f32) -> usize {
    (seconds / DT).round() as usize
}

fn run(sim: &mut Sim, intent: Intent, seconds: f32) -> Vec<Event> {
    (0..ticks(seconds)).flat_map(|_| sim.step(intent)).collect()
}

fn flat_floor() -> Level {
    Level {
        solids: vec![Solid::rect(v(-100.0, -2.0), v(100.0, 0.0))],
    }
}

fn settled_on_floor() -> Sim {
    let mut sim = Sim::new(flat_floor(), v(0.0, 0.6));
    run(&mut sim, Intent::default(), 1.5);
    sim
}

/// High in the air with no floor anywhere near.
fn floating() -> Sim {
    let mut level = flat_floor();
    level.solids[0] = Solid::rect(v(-100.0, -102.0), v(100.0, -100.0));
    Sim::new(level, v(0.0, 0.0))
}

fn lunge(sim: &mut Sim, aim: Vec2) -> Vec<Event> {
    sim.step(Intent {
        aim,
        lunge_pressed: true,
        ..Default::default()
    })
}

#[test]
fn lunge_bursts_in_the_aimed_direction() {
    let mut sim = settled_on_floor();
    let start = sim.blob.center();
    let events = lunge(&mut sim, v(1.0, 0.0));
    assert!(matches!(events[..], [Event::Lunged { .. }]), "{events:?}");
    run(&mut sim, Intent::default(), 0.16);
    let moved = sim.blob.center().x - start.x;
    assert!(moved > 1.8, "only lunged {moved}");
}

#[test]
fn unaimed_lunge_goes_the_way_you_face() {
    let mut sim = settled_on_floor();
    run(
        &mut sim,
        Intent {
            roll: -1.0,
            ..Default::default()
        },
        0.05,
    );
    let start = sim.blob.center().x;
    lunge(&mut sim, Vec2::ZERO);
    run(&mut sim, Intent::default(), 0.16);
    assert!(sim.blob.center().x - start < -1.5);
}

#[test]
fn air_lunge_ignores_gravity_while_it_lasts() {
    let mut sim = floating();
    let start = sim.blob.center();
    lunge(&mut sim, v(1.0, 0.0));
    run(&mut sim, Intent::default(), 0.15);
    let end = sim.blob.center();
    assert!(
        (end.y - start.y).abs() < 0.2,
        "drifted {} vertically",
        end.y - start.y
    );
    assert!(end.x - start.x > 1.8);
}

#[test]
fn lunge_makes_the_blob_invulnerable() {
    let mut sim = settled_on_floor();
    assert!(!sim.controller.invulnerable());
    lunge(&mut sim, v(1.0, 0.0));
    assert!(sim.controller.invulnerable());
    run(&mut sim, Intent::default(), 0.2);
    assert!(!sim.controller.invulnerable());
}

#[test]
fn only_one_lunge_in_the_air_until_refreshed() {
    let mut sim = floating();
    lunge(&mut sim, v(1.0, 0.0));
    run(&mut sim, Intent::default(), 0.4);
    let second = lunge(&mut sim, v(1.0, 0.0));
    assert!(second.is_empty(), "lunged twice in the air: {second:?}");

    sim.controller.refresh_lunge();
    let third = lunge(&mut sim, v(1.0, 0.0));
    assert!(matches!(third[..], [Event::Lunged { .. }]), "{third:?}");
}

#[test]
fn lunge_pressed_during_cooldown_is_buffered() {
    let mut sim = settled_on_floor();
    lunge(&mut sim, v(1.0, 0.0));
    run(&mut sim, Intent::default(), 0.22);
    // Pressed 80 ms before the cooldown ends: it should still come out.
    let mut events = lunge(&mut sim, v(-1.0, 0.0));
    events.extend(run(&mut sim, Intent::default(), 0.15));
    assert!(
        events.iter().any(|e| matches!(e, Event::Lunged { .. })),
        "{events:?}"
    );
}

#[test]
fn jump_cancels_a_lunge_and_keeps_its_speed() {
    let mut sim = settled_on_floor();
    lunge(&mut sim, v(1.0, 0.0));
    run(&mut sim, Intent::default(), 0.05);
    sim.step(Intent {
        jump_pressed: true,
        jump_held: true,
        ..Default::default()
    });
    assert!(!sim.controller.is_lunging());
    let vel = sim.blob.velocity();
    assert!(vel.x > 10.0 && vel.y > 8.0, "velocity {vel}");
}

fn drop_and_slam(modes: TarModes) -> (Sim, Vec<Event>) {
    let mut sim = Sim::new(flat_floor(), v(0.0, 4.0));
    run(&mut sim, Intent::default(), 0.1);
    let mut events = sim.step(Intent {
        aim: v(0.0, -1.0),
        lunge_pressed: true,
        modes,
        ..Default::default()
    });
    for _ in 0..ticks(0.5) {
        events.extend(sim.step(Intent {
            modes,
            ..Default::default()
        }));
        if !sim.controller.is_slamming() {
            break;
        }
    }
    (sim, events)
}

#[test]
fn down_lunge_in_the_air_slams_into_the_ground() {
    let (sim, events) = drop_and_slam(TarModes::default());
    assert_eq!(events.first(), Some(&Event::SlamStarted));
    let slammed = events.iter().find_map(|e| match e {
        Event::Slammed { speed, .. } => Some(*speed),
        _ => None,
    });
    let speed = slammed.expect("never landed the slam");
    assert!(speed > 20.0, "slam landed at only {speed}");
    assert!(!sim.controller.is_slamming());
}

#[test]
fn down_lunge_on_the_ground_lunges_sideways() {
    let mut sim = settled_on_floor();
    let events = lunge(&mut sim, v(0.0, -1.0));
    assert!(matches!(events[..], [Event::Lunged { direction }] if direction.y == 0.0));
}

#[test]
fn bouncy_slam_rebounds_higher_than_it_started() {
    let bouncy = TarModes {
        bouncy: true,
        ..Default::default()
    };
    let (mut sim, events) = drop_and_slam(bouncy);
    assert!(
        events.iter().any(|e| matches!(e, Event::Bounced { .. })),
        "{events:?}"
    );
    let mut best = f32::MIN;
    for _ in 0..ticks(1.5) {
        sim.step(Intent {
            modes: bouncy,
            ..Default::default()
        });
        best = best.max(sim.blob.center().y);
    }
    assert!(best > 4.5, "rebounded only to {best}");
}

const STICKY: TarModes = TarModes {
    sticky: true,
    bouncy: false,
};

fn sticky(jump_pressed: bool, jump_held: bool) -> Intent {
    Intent {
        jump_pressed,
        jump_held,
        modes: STICKY,
        ..Default::default()
    }
}

/// Clings to a wall, shrinks for `hold` seconds from an empty charge, then
/// presses Jump; returns the events and the speed right after the burst.
fn burst_off_wall(hold: f32) -> (Vec<Event>, f32, Sim) {
    let level = Level {
        solids: vec![Solid::rect(v(0.0, -50.0), v(1.0, 50.0))],
    };
    let mut sim = Sim::new(level, v(-0.52, 0.0));
    run(&mut sim, sticky(false, false), 0.5);
    sim.controller.spend_charge();
    let mut events = run(&mut sim, sticky(false, false), hold);
    events.extend(sim.step(sticky(true, true)));
    let speed = sim.blob.velocity().length();
    (events, speed, sim)
}

#[test]
fn grip_shrinks_and_jump_bursts_instead_of_jumping() {
    let mut sim = settled_on_floor();
    let events = sim.step(sticky(false, false));
    assert_eq!(events, vec![Event::Shrank]);
    let (events, _, _) = burst_off_wall(0.2);
    assert!(!events.iter().any(|e| matches!(e, Event::Jumped { .. })));
    assert!(events.iter().any(|e| matches!(e, Event::Launched { .. })));
}

#[test]
fn gripping_shrinks_the_blob() {
    let mut sim = settled_on_floor();
    let before = sim.blob.area();
    run(&mut sim, sticky(false, false), 0.6);
    assert!(sim.controller.is_shrunk());
    let ratio = sim.blob.area() / before;
    assert!(ratio < 0.6, "only shrank to {ratio}");
    // Letting go of Grip grows it back.
    run(&mut sim, Intent::default(), 0.3);
    assert!(!sim.controller.is_shrunk());
    let ratio = sim.blob.area() / before;
    assert!(ratio > 0.9, "stayed small: {ratio}");
}

#[test]
fn a_longer_shrink_bursts_faster() {
    let (_, tap, _) = burst_off_wall(0.0);
    let (_, full, _) = burst_off_wall(0.6);
    assert!(tap > 8.0, "a quick burst barely moved: {tap}");
    assert!(full > tap * 1.6, "full {full}, tap {tap}");
}

#[test]
fn burst_launches_away_from_the_wall_and_up() {
    let (events, _, sim) = burst_off_wall(0.3);
    let direction = events
        .iter()
        .find_map(|e| match e {
            Event::Launched { direction, .. } => Some(*direction),
            _ => None,
        })
        .unwrap();
    assert!(direction.x < -0.5 && direction.y > 0.2, "{direction}");
    assert!(sim.blob.velocity().x < -5.0);
}

#[test]
fn burst_from_the_ceiling_launches_down() {
    let level = Level {
        solids: vec![Solid::rect(v(-50.0, 0.0), v(50.0, 1.0))],
    };
    let mut sim = Sim::new(level, v(0.0, -0.52));
    run(&mut sim, sticky(false, false), 0.5);
    sim.step(sticky(true, true));
    assert!(sim.blob.velocity().y < -10.0, "{}", sim.blob.velocity());
}

#[test]
fn burst_follows_the_aim_when_it_points_away_from_the_surface() {
    let mut sim = Sim::new(flat_floor(), v(0.0, 0.6));
    run(&mut sim, sticky(false, false), 1.0);
    sim.step(Intent {
        aim: v(1.0, 0.3),
        ..sticky(true, true)
    });
    let vel = sim.blob.velocity();
    assert!(vel.x > 10.0 && vel.x > vel.y * 1.5, "{vel}");
}

#[test]
fn plain_jump_is_still_instant_without_sticky() {
    let mut sim = settled_on_floor();
    let events = sim.step(Intent {
        jump_pressed: true,
        jump_held: true,
        ..Default::default()
    });
    assert!(matches!(events[..], [Event::Jumped { .. }]), "{events:?}");
}
