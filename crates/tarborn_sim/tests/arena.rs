//! The arena room: waves, enemies that stay dead, ambient grubs, combos.

use glam::Vec2;
use tarborn_sim::{
    ARENA_SPAWN, DT, Enemy, EnemyKind, Event, Intent, Level, Phase, Sim, Solid, arena::WAVE_BREAK,
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

fn wave_started(events: &[Event]) -> Option<usize> {
    events.iter().find_map(|e| match e {
        Event::WaveStarted { wave } => Some(*wave),
        _ => None,
    })
}

/// Kills every enemy that isn't an ambient grub.
fn kill_wave(sim: &mut Sim) {
    for enemy in &mut sim.enemies {
        if enemy.kind != EnemyKind::Grub {
            enemy.die();
        }
    }
}

fn arena_at_wave_one() -> (Sim, Vec<Event>) {
    let mut sim = Sim::arena_room();
    let events = run(&mut sim, Intent::default(), 1.6);
    (sim, events)
}

#[test]
fn the_first_wave_drops_in_after_a_moment() {
    let (sim, events) = arena_at_wave_one();
    assert_eq!(wave_started(&events), Some(0), "{events:?}");
    let guards = sim
        .enemies
        .iter()
        .filter(|e| e.kind == EnemyKind::Guard)
        .count();
    assert_eq!(guards, 2);
}

#[test]
fn the_player_starts_on_the_middle_platform() {
    let mut sim = Sim::arena_room();
    run(&mut sim, Intent::default(), 1.0);
    let at = sim.blob.center();
    assert!(at.y > 8.0 && at.x.abs() < 3.0, "at {at}");
    assert!(!sim.dead);
}

#[test]
fn every_wave_lands_on_solid_ground() {
    let mut sim = Sim::arena_room();
    let waves = sim.arena.as_ref().unwrap().waves.len();
    run(&mut sim, Intent::default(), 1.6);
    for wave in 0..waves {
        let first = sim.enemies.len() - sim.arena.as_ref().unwrap().waves[wave].len();
        // Each one should touch down within a moment (skitters hop about, so
        // check over the whole moment rather than at its end).
        let count = sim.enemies.len() - first;
        let mut landed = vec![false; count];
        for _ in 0..ticks(0.8) {
            sim.step(Intent::default());
            for (k, enemy) in sim.enemies[first..first + count].iter().enumerate() {
                landed[k] |= enemy.on_ground && enemy.position.y > 0.0;
            }
        }
        for (k, enemy) in sim.enemies[first..first + count].iter().enumerate() {
            assert!(
                landed[k],
                "wave {wave}: {:?} at {}",
                enemy.kind, enemy.position
            );
        }
        kill_wave(&mut sim);
        run(&mut sim, Intent::default(), WAVE_BREAK + 0.1);
    }
}

#[test]
fn clearing_a_wave_brings_the_next_after_a_break() {
    let (mut sim, _) = arena_at_wave_one();
    kill_wave(&mut sim);
    let early = run(&mut sim, Intent::default(), WAVE_BREAK - 0.3);
    assert_eq!(wave_started(&early), None);
    assert!(matches!(sim.arena.as_ref().unwrap().phase, Phase::Break(_)));
    let events = run(&mut sim, Intent::default(), 0.5);
    assert_eq!(wave_started(&events), Some(1));
}

#[test]
fn arena_enemies_stay_dead() {
    let (mut sim, _) = arena_at_wave_one();
    let guard = sim
        .enemies
        .iter()
        .position(|e| e.kind == EnemyKind::Guard)
        .unwrap();
    sim.enemies[guard].die();
    run(&mut sim, Intent::default(), 10.0);
    assert!(!sim.enemies[guard].alive());
}

#[test]
fn clearing_every_wave_clears_the_arena() {
    let mut sim = Sim::arena_room();
    let waves = sim.arena.as_ref().unwrap().waves.len();
    run(&mut sim, Intent::default(), 1.6);
    let mut cleared = false;
    for _ in 0..waves {
        kill_wave(&mut sim);
        let events = run(&mut sim, Intent::default(), WAVE_BREAK + 0.2);
        cleared |= events.contains(&Event::ArenaCleared);
    }
    assert!(cleared);
    assert_eq!(sim.arena.as_ref().unwrap().phase, Phase::Cleared);
}

#[test]
fn there_are_always_grubs_to_throw() {
    let (mut sim, _) = arena_at_wave_one();
    let grubs = |sim: &Sim| {
        sim.enemies
            .iter()
            .filter(|e| e.kind == EnemyKind::Grub && e.alive())
            .count()
    };
    assert_eq!(grubs(&sim), 2);
    for enemy in &mut sim.enemies {
        if enemy.kind == EnemyKind::Grub {
            enemy.die();
        }
    }
    run(&mut sim, Intent::default(), 5.0);
    assert_eq!(grubs(&sim), 2);
    assert_eq!(sim.arena.as_ref().unwrap().phase, Phase::Fighting);
}

#[test]
fn hits_chain_into_a_combo_that_drops_after_a_pause() {
    let mut sim = Sim::new(
        Level {
            solids: vec![Solid::rect(v(-100.0, -2.0), v(100.0, 0.0))],
        },
        v(0.0, 3.0),
    );
    let mut dummy = Enemy::dummy(v(2.0, 0.6));
    dummy.max_health = 1000.0;
    dummy.health = 1000.0;
    sim.enemies.push(dummy);
    run(&mut sim, Intent::default(), 0.15);
    let press = Intent {
        jump_pressed: true,
        ..Default::default()
    };
    // Lunge, bounce off the hit, and lunge again from the air.
    for _ in 0..2 {
        sim.step(press);
        run(&mut sim, Intent::default(), 0.45);
    }
    assert_eq!(sim.combo.count, 2, "{:?}", sim.combo);
    run(&mut sim, Intent::default(), 2.5);
    assert_eq!(sim.combo.count, 0);
    assert_eq!(sim.combo.best, 2);
}

#[test]
fn getting_hurt_breaks_the_combo() {
    let mut sim = Sim::arena_room();
    sim.combo.count = 5;
    sim.combo.left = 2.0;
    sim.blob.set_velocity(Vec2::ZERO);
    // Drop the player onto the spikes.
    for p in &mut sim.blob.positions {
        *p -= v(0.0, ARENA_SPAWN.y - 1.0);
    }
    let events = run(&mut sim, Intent::default(), 0.5);
    assert!(events.iter().any(|e| matches!(e, Event::PlayerHurt { .. })));
    assert_eq!(sim.combo.count, 0);
}

#[test]
fn a_minute_of_button_mashing_in_the_arena_does_not_break_anything() {
    let mut sim = Sim::arena_room();
    // A small deterministic pseudo-random input stream.
    let mut seed: u32 = 12345;
    let mut next = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        seed >> 16
    };
    let mut intent = Intent::default();
    for tick in 0..ticks(60.0) {
        if tick % 15 == 0 {
            let r = next();
            intent.roll = [(-1.0), 0.0, 1.0][(r % 3) as usize];
            intent.aim = Vec2::new(intent.roll, [(-1.0), 0.0, 1.0][(r / 3 % 3) as usize]);
            intent.jump_held = r / 9 % 2 == 0;
            intent.modes.sticky = r / 18 % 3 == 0;
        }
        intent.jump_pressed = next() % 20 == 0;
        sim.step(intent);
        if sim.dead {
            sim = Sim::arena_room();
        }
        for enemy in &sim.enemies {
            assert!(enemy.position.is_finite(), "{enemy:?}");
        }
        let at = sim.blob.center();
        assert!(
            at.is_finite() && at.x.abs() < 20.0 && at.y > -0.5 && at.y < 16.0,
            "left the room: {at}"
        );
    }
}
