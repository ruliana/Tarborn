//! Behavioural tests for hitting things and getting hurt.

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

fn hits(events: &[Event]) -> Vec<HitKind> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Hit { kind, .. } => Some(*kind),
            _ => None,
        })
        .collect()
}

const BOUNCY: TarModes = TarModes {
    sticky: false,
    bouncy: true,
};

fn flat_floor() -> Level {
    Level {
        solids: vec![Solid::rect(v(-100.0, -2.0), v(100.0, 0.0))],
    }
}

/// The blob settled on a floor with a dummy `gap` metres to its right.
fn facing_dummy(gap: f32) -> Sim {
    let mut sim = Sim::new(flat_floor(), v(0.0, 0.6));
    sim.enemies.push(Enemy::dummy(v(0.5 + 0.6 + gap, 0.6)));
    run(&mut sim, Intent::default(), 1.0);
    sim
}

fn lunge(aim: Vec2) -> Intent {
    Intent {
        aim,
        lunge_pressed: true,
        ..Default::default()
    }
}

#[test]
fn lunge_into_a_dummy_hits_it_once() {
    let mut sim = facing_dummy(1.0);
    let mut events = sim.step(lunge(v(1.0, 0.0)));
    events.extend(run(&mut sim, Intent::default(), 0.5));
    assert_eq!(hits(&events), vec![HitKind::Lunge]);
    let dummy = &sim.enemies[0];
    assert_eq!(dummy.health, dummy.max_health - sim.combat.lunge_damage);
}

#[test]
fn a_hit_knocks_the_dummy_away() {
    let mut sim = facing_dummy(1.0);
    let start = sim.enemies[0].position.x;
    sim.step(lunge(v(1.0, 0.0)));
    run(&mut sim, Intent::default(), 0.5);
    assert!(sim.enemies[0].position.x > start + 0.5);
}

#[test]
fn hitting_refreshes_the_air_lunge() {
    // In the air, next to a dummy standing on a narrow post.
    let level = Level {
        solids: vec![
            Solid::rect(v(-100.0, -102.0), v(100.0, -100.0)),
            Solid::rect(v(2.3, -10.0), v(2.7, -0.6)),
        ],
    };
    let mut sim = Sim::new(level, v(0.0, 0.0));
    sim.enemies.push(Enemy::dummy(v(2.5, 0.0)));
    let mut events = sim.step(lunge(v(1.0, 0.0)));
    events.extend(run(&mut sim, Intent::default(), 0.3));
    assert_eq!(hits(&events), vec![HitKind::Lunge], "first lunge missed");
    assert!(sim.blob.touching_count() == 0, "landed already");
    let second = sim.step(lunge(v(-1.0, 0.0)));
    assert!(
        second.iter().any(|e| matches!(e, Event::Lunged { .. })),
        "no second air lunge: {second:?}"
    );
}

#[test]
fn rolling_gently_into_a_dummy_does_not_hurt_it() {
    let mut sim = facing_dummy(0.5);
    let events = run(
        &mut sim,
        Intent {
            roll: 1.0,
            ..Default::default()
        },
        1.0,
    );
    assert!(hits(&events).is_empty(), "{events:?}");
}

/// Running into an enemy doesn't hurt it, from any run-up, in any mode.
#[test]
fn rolling_into_a_dummy_from_any_run_up_does_not_hurt_it() {
    for modes in [TarModes::default(), BOUNCY] {
        for gap in [0.5, 1.0, 2.0, 3.0, 4.0, 6.0, 10.0, 25.0] {
            let mut sim = facing_dummy(gap);
            let mut events = Vec::new();
            let mut closest = f32::INFINITY;
            for _ in 0..ticks(6.0) {
                events.extend(sim.step(Intent {
                    roll: 1.0,
                    modes,
                    ..Default::default()
                }));
                closest = closest.min(sim.blob.center().distance(sim.enemies[0].position));
            }
            assert!(
                closest < 1.3,
                "{modes:?} gap={gap}: never reached the dummy"
            );
            assert!(hits(&events).is_empty(), "{modes:?} gap={gap}: {events:?}");
        }
    }
}

#[test]
fn sliding_into_a_dummy_fast_hurts_it() {
    let mut sim = facing_dummy(1.5);
    sim.blob.set_velocity(v(18.0, 0.0));
    let events = run(
        &mut sim,
        Intent {
            modes: BOUNCY,
            ..Default::default()
        },
        0.3,
    );
    assert_eq!(
        hits(&events),
        vec![HitKind::Impact],
        "{events:?} v={}",
        sim.blob.velocity()
    );
}

#[test]
fn lunge_aim_assist_finds_a_dummy_slightly_off_line() {
    let mut sim = Sim::new(flat_floor(), v(0.0, 0.6));
    sim.enemies.push(Enemy::dummy(v(3.0, 2.0)));
    run(&mut sim, Intent::default(), 0.3);
    // The dummy is about 25° above straight ahead and still falling.
    let mut events = sim.step(lunge(v(1.0, 0.0)));
    events.extend(run(&mut sim, Intent::default(), 0.3));
    assert!(hits(&events).contains(&HitKind::Lunge), "{events:?}");
}

#[test]
fn slam_shockwave_hits_nearby_enemies_without_touching_them() {
    let mut sim = Sim::new(flat_floor(), v(0.0, 4.0));
    sim.enemies.push(Enemy::dummy(v(2.0, 0.6)));
    run(&mut sim, Intent::default(), 0.1);
    let mut events = sim.step(lunge(v(0.0, -1.0)));
    events.extend(run(&mut sim, Intent::default(), 0.6));
    assert_eq!(hits(&events), vec![HitKind::Shockwave]);
}

#[test]
fn slam_onto_an_enemy_bounces_the_player_up() {
    let mut sim = Sim::new(flat_floor(), v(0.0, 4.0));
    sim.enemies.push(Enemy::dummy(v(0.0, 0.6)));
    run(&mut sim, Intent::default(), 0.1);
    let mut events = sim.step(lunge(v(0.0, -1.0)));
    let mut rose = false;
    for _ in 0..ticks(0.6) {
        events.extend(sim.step(Intent::default()));
        rose |= hits(&events).contains(&HitKind::Slam) && sim.blob.velocity().y > 8.0;
    }
    assert!(hits(&events).contains(&HitKind::Slam), "{events:?}");
    assert!(rose, "did not bounce off the enemy");
}

#[test]
fn bouncy_landing_on_an_enemy_pogos() {
    let mut sim = Sim::new(flat_floor(), v(0.0, 4.0));
    sim.enemies.push(Enemy::dummy(v(0.0, 0.6)));
    let events = run(
        &mut sim,
        Intent {
            modes: BOUNCY,
            ..Default::default()
        },
        1.0,
    );
    assert!(hits(&events).contains(&HitKind::Pogo), "{events:?}");
}

#[test]
fn hits_freeze_the_world_briefly_without_losing_input() {
    let mut sim = facing_dummy(1.0);
    sim.step(lunge(v(1.0, 0.0)));
    while !sim.in_hitstop() {
        sim.step(Intent::default());
    }
    let frozen = sim.blob.positions.clone();
    // Jump pressed during hitstop.
    sim.step(Intent {
        jump_pressed: true,
        jump_held: true,
        ..Default::default()
    });
    assert_eq!(sim.blob.positions, frozen, "moved during hitstop");
    let mut jumped = false;
    for _ in 0..ticks(0.2) {
        jumped |= sim
            .step(Intent {
                jump_held: true,
                ..Default::default()
            })
            .iter()
            .any(|e| matches!(e, Event::Jumped { .. }));
    }
    assert!(jumped, "the jump pressed during hitstop was lost");
}

#[test]
fn killing_drops_tar_that_heals() {
    let mut sim = facing_dummy(1.0);
    sim.enemies[0].health = 10.0;
    sim.health = 3;
    let mut events = sim.step(lunge(v(1.0, 0.0)));
    events.extend(run(&mut sim, Intent::default(), 0.3));
    assert!(events.iter().any(|e| matches!(e, Event::Killed { .. })));
    assert_eq!(sim.pickups.len(), 1);
    // Roll over to the drop.
    let events = run(
        &mut sim,
        Intent {
            roll: 1.0,
            ..Default::default()
        },
        1.5,
    );
    assert!(
        events.iter().any(|e| matches!(e, Event::Healed { .. })),
        "never picked it up"
    );
    assert_eq!(sim.health, 4);
}

#[test]
fn dummies_come_back_after_dying() {
    let mut sim = facing_dummy(1.0);
    sim.enemies[0].health = 1.0;
    sim.step(lunge(v(1.0, 0.0)));
    run(&mut sim, Intent::default(), 0.3);
    assert!(!sim.enemies[0].alive());
    run(&mut sim, Intent::default(), 2.5);
    assert!(sim.enemies[0].alive());
    assert_eq!(sim.enemies[0].health, sim.enemies[0].max_health);
}

fn on_spikes() -> Sim {
    let mut sim = Sim::new(flat_floor(), v(0.0, 0.6));
    sim.hazards.push(Solid::rect(v(-1.0, 0.0), v(1.0, 0.3)));
    sim
}

#[test]
fn spikes_hurt_once_then_grant_invulnerability() {
    let mut sim = on_spikes();
    let events = run(&mut sim, Intent::default(), 0.5);
    let hurts = events
        .iter()
        .filter(|e| matches!(e, Event::PlayerHurt { .. }))
        .count();
    assert_eq!(hurts, 1);
    assert_eq!(sim.health, sim.max_health - 1);
    assert!(sim.player_invulnerable());
}

#[test]
fn lunging_through_spikes_is_safe() {
    let mut sim = Sim::new(flat_floor(), v(-3.0, 0.6));
    run(&mut sim, Intent::default(), 1.0);
    sim.hazards.push(Solid::rect(v(-1.0, 0.0), v(-0.5, 0.3)));
    let mut events = sim.step(lunge(v(1.0, 0.0)));
    events.extend(run(&mut sim, Intent::default(), 0.15));
    assert!(
        !events.iter().any(|e| matches!(e, Event::PlayerHurt { .. })),
        "{events:?}"
    );
}

#[test]
fn running_out_of_health_kills_the_player() {
    let mut sim = Sim::new(flat_floor(), v(0.0, 0.6));
    sim.hazards.push(Solid::rect(v(-100.0, 0.0), v(100.0, 0.3)));
    let events = run(&mut sim, Intent::default(), 10.0);
    assert!(events.contains(&Event::PlayerDied));
    assert!(sim.dead);
    assert_eq!(sim.health, 0);
}

#[test]
fn training_room_is_deterministic() {
    let script = |sim: &mut Sim| {
        for t in 0..ticks(4.0) {
            let roll = ((t / 50) % 3) as f32 - 1.0;
            sim.step(Intent {
                roll,
                aim: v(roll, if t % 130 < 60 { -1.0 } else { 0.3 }),
                jump_pressed: t % 80 == 0,
                jump_held: t % 80 < 30,
                lunge_pressed: t % 45 == 20,
                modes: TarModes {
                    sticky: false,
                    bouncy: t % 300 > 200,
                },
            });
        }
    };
    let mut a = Sim::training_room(v(-6.0, 2.0));
    let mut b = a.clone();
    script(&mut a);
    script(&mut b);
    assert_eq!(a.blob.positions, b.blob.positions);
    assert_eq!(a.enemies, b.enemies);
}
