//! Behavioural tests for how the blob feels. Each one describes something a
//! player would notice, run headlessly on the deterministic simulation.

use glam::Vec2;
use tarborn_sim::{DT, Event, Intent, Level, Sim, Solid, TarModes};

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn ticks(seconds: f32) -> usize {
    (seconds / DT).round() as usize
}

fn run(sim: &mut Sim, intent: Intent, seconds: f32) {
    for _ in 0..ticks(seconds) {
        sim.step(intent);
    }
}

fn idle() -> Intent {
    Intent::default()
}

fn with_modes(modes: TarModes) -> Intent {
    Intent {
        modes,
        ..Default::default()
    }
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

fn settled_on_floor() -> Sim {
    let mut sim = Sim::new(flat_floor(), v(0.0, 0.6));
    run(&mut sim, idle(), 1.5);
    sim
}

fn press_jump(sim: &mut Sim) {
    sim.step(Intent {
        jump_pressed: true,
        jump_held: true,
        ..Default::default()
    });
}

#[test]
fn settles_on_the_floor_without_sinking_or_deflating() {
    let mut sim = Sim::new(flat_floor(), v(0.0, 2.0));
    run(&mut sim, idle(), 2.0);
    let blob = &sim.blob;
    assert!(blob.touching_count() > 0, "should be resting on the floor");
    let c = blob.center();
    assert!((0.25..0.6).contains(&c.y), "center height {}", c.y);
    for p in &blob.positions {
        assert!(p.y > 0.0, "point sank into the floor: {p}");
    }
    let ratio = blob.area() / blob.rest_area();
    assert!((0.85..1.15).contains(&ratio), "area ratio {ratio}");
    assert!(
        blob.velocity().length() < 0.2,
        "still moving: {}",
        blob.velocity()
    );
}

#[test]
fn rolls_in_the_direction_held() {
    let mut sim = settled_on_floor();
    let start = sim.blob.center().x;
    run(
        &mut sim,
        Intent {
            roll: 1.0,
            ..Default::default()
        },
        1.0,
    );
    assert!(
        sim.blob.center().x - start > 2.0,
        "moved {}",
        sim.blob.center().x - start
    );

    let mut sim = settled_on_floor();
    run(
        &mut sim,
        Intent {
            roll: -1.0,
            ..Default::default()
        },
        1.0,
    );
    assert!(sim.blob.center().x - start < -2.0);
}

#[test]
fn jump_leaves_the_ground() {
    let mut sim = settled_on_floor();
    press_jump(&mut sim);
    run(
        &mut sim,
        Intent {
            jump_held: true,
            ..Default::default()
        },
        0.3,
    );
    assert!(
        sim.blob.center().y > 1.5,
        "only reached {}",
        sim.blob.center().y
    );
}

#[test]
fn holding_jump_goes_higher_than_tapping() {
    let apex = |hold: bool| {
        let mut sim = settled_on_floor();
        press_jump(&mut sim);
        let mut best = f32::MIN;
        for _ in 0..ticks(1.0) {
            sim.step(Intent {
                jump_held: hold,
                ..Default::default()
            });
            best = best.max(sim.blob.center().y);
        }
        best
    };
    let (held, tapped) = (apex(true), apex(false));
    assert!(held > tapped + 0.5, "held {held}, tapped {tapped}");
}

/// Ticks until a blob dropped from `height` first touches the floor.
fn landing_tick(height: f32) -> usize {
    let mut sim = Sim::new(flat_floor(), v(0.0, height));
    (1..ticks(3.0))
        .find(|_| {
            sim.step(idle());
            sim.blob.touching_count() > 0
        })
        .expect("never landed")
}

fn jumped_after_landing(press_ticks_early: usize) -> bool {
    let land = landing_tick(3.0);
    let mut sim = Sim::new(flat_floor(), v(0.0, 3.0));
    let mut jumped = false;
    for tick in 1..=land + ticks(0.3) {
        let pressed = tick + press_ticks_early == land;
        let events = sim.step(Intent {
            jump_pressed: pressed,
            jump_held: tick + press_ticks_early >= land,
            ..Default::default()
        });
        jumped |= events.iter().any(|e| matches!(e, Event::Jumped { .. }));
    }
    // Holding Jump through the landing bounces, so look for the jump itself.
    jumped
}

#[test]
fn jump_pressed_just_before_landing_is_buffered() {
    assert!(
        jumped_after_landing(ticks(0.06)),
        "a 60 ms early press should still jump"
    );
}

#[test]
fn jump_pressed_long_before_landing_is_ignored() {
    assert!(
        !jumped_after_landing(ticks(0.3)),
        "a 300 ms early press should not jump"
    );
}

/// Settles on a floor, removes the floor, waits, then presses jump.
fn jumped_after_floor_vanished(wait: f32) -> bool {
    let mut sim = settled_on_floor();
    sim.level = Level::default();
    run(&mut sim, idle(), wait);
    press_jump(&mut sim);
    sim.blob.velocity().y > 5.0
}

#[test]
fn coyote_time_allows_a_late_jump() {
    assert!(jumped_after_floor_vanished(0.05));
}

#[test]
fn no_jump_long_after_leaving_the_ground() {
    assert!(!jumped_after_floor_vanished(0.3));
}

fn ceiling() -> Level {
    Level {
        solids: vec![Solid::rect(v(-50.0, 0.0), v(50.0, 1.0))],
    }
}

#[test]
fn sticky_clings_to_the_ceiling() {
    let mut sim = Sim::new(ceiling(), v(0.0, -0.52));
    run(&mut sim, with_modes(STICKY), 2.0);
    assert!(
        sim.blob.center().y > -1.0,
        "fell to {}",
        sim.blob.center().y
    );
}

#[test]
fn not_sticky_falls_from_the_ceiling() {
    let mut sim = Sim::new(ceiling(), v(0.0, -0.52));
    run(&mut sim, idle(), 1.0);
    assert!(
        sim.blob.center().y < -3.0,
        "only at {}",
        sim.blob.center().y
    );
}

fn right_wall() -> Level {
    Level {
        solids: vec![Solid::rect(v(0.0, -50.0), v(1.0, 50.0))],
    }
}

#[test]
fn sticky_holds_onto_a_wall() {
    let mut sim = Sim::new(right_wall(), v(-0.52, 0.0));
    run(
        &mut sim,
        Intent {
            roll: 0.0,
            ..with_modes(STICKY)
        },
        2.0,
    );
    assert!(
        sim.blob.center().y > -0.5,
        "slid to {}",
        sim.blob.center().y
    );
}

#[test]
fn not_sticky_slides_off_a_wall() {
    let mut sim = Sim::new(right_wall(), v(-0.52, 0.0));
    run(&mut sim, idle(), 1.0);
    assert!(sim.blob.center().y < -3.0);
}

#[test]
fn sticky_rolling_climbs_a_wall() {
    let mut sim = Sim::new(right_wall(), v(-0.52, 0.0));
    run(&mut sim, with_modes(STICKY), 0.5);
    let start = sim.blob.center().y;
    run(
        &mut sim,
        Intent {
            roll: 1.0,
            ..with_modes(STICKY)
        },
        1.0,
    );
    let climbed = sim.blob.center().y - start;
    assert!(climbed > 2.0, "only climbed {climbed}");
}

#[test]
fn sticky_rolling_the_other_way_crawls_down_a_wall() {
    let mut sim = Sim::new(right_wall(), v(-0.52, 0.0));
    run(&mut sim, with_modes(STICKY), 0.5);
    let start = sim.blob.center();
    run(
        &mut sim,
        Intent {
            roll: -1.0,
            ..with_modes(STICKY)
        },
        0.5,
    );
    let end = sim.blob.center();
    assert!(
        end.y < start.y - 0.5,
        "did not crawl down: {start} -> {end}"
    );
    assert!(end.x > -1.0, "let go of the wall: {end}");
}

fn slope() -> Level {
    // A 30° ramp rising to the right.
    let h = 20.0 * 30f32.to_radians().tan();
    Level {
        solids: vec![Solid::new(vec![v(-10.0, 0.0), v(10.0, 0.0), v(10.0, h)])],
    }
}

fn slide_distance(modes: TarModes) -> f32 {
    let mut sim = Sim::new(slope(), v(4.0, 8.5));
    run(&mut sim, with_modes(modes), 0.5); // land on the ramp
    let start = sim.blob.center();
    run(&mut sim, with_modes(modes), 1.0);
    start.distance(sim.blob.center())
}

#[test]
fn bouncy_slides_down_slopes_and_sticky_does_not() {
    let bouncy = slide_distance(BOUNCY);
    let sticky = slide_distance(STICKY);
    assert!(bouncy > 2.0, "bouncy only slid {bouncy}");
    assert!(sticky < 0.5, "sticky slid {sticky}");
}

/// Highest the blob's center gets after first touching the floor.
fn rebound_height(modes: TarModes, drop_from: f32) -> f32 {
    let mut sim = Sim::new(flat_floor(), v(0.0, drop_from));
    while sim.blob.touching_count() == 0 {
        sim.step(with_modes(modes));
    }
    let mut best = f32::MIN;
    for _ in 0..ticks(1.5) {
        sim.step(with_modes(modes));
        best = best.max(sim.blob.center().y);
    }
    best
}

#[test]
fn bouncy_rebounds_from_a_drop_and_plain_tar_does_not() {
    let bouncy = rebound_height(BOUNCY, 6.0);
    let plain = rebound_height(TarModes::default(), 6.0);
    assert!(bouncy > 3.5, "bouncy only rebounded to {bouncy}");
    assert!(plain < 1.0, "plain tar rebounded to {plain}");
}

#[test]
fn bouncy_can_still_roll() {
    let mut sim = settled_on_floor();
    let start = sim.blob.center().x;
    run(
        &mut sim,
        Intent {
            roll: 1.0,
            ..with_modes(BOUNCY)
        },
        1.0,
    );
    let moved = sim.blob.center().x - start;
    assert!(moved > 1.5, "only rolled {moved}");
}

#[test]
fn bouncy_comes_to_rest_eventually() {
    let mut sim = Sim::new(flat_floor(), v(0.0, 3.0));
    run(&mut sim, with_modes(BOUNCY), 5.0);
    assert!(sim.blob.touching_count() > 0, "still in the air");
    assert!(sim.blob.velocity().length() < 0.5, "still moving");
}

#[test]
fn survives_a_hard_landing() {
    let mut sim = Sim::new(flat_floor(), v(0.0, 12.0));
    run(&mut sim, idle(), 3.0);
    let ratio = sim.blob.area() / sim.blob.rest_area();
    assert!(ratio > 0.75, "crushed to {ratio} of its area");
    assert!(
        sim.blob.positions.iter().all(|p| p.y > 0.0),
        "fell through the floor"
    );
}

#[test]
fn simulation_is_deterministic() {
    let script = |sim: &mut Sim| {
        for t in 0..ticks(3.0) {
            let roll = ((t / 40) % 3) as f32 - 1.0;
            sim.step(Intent {
                roll,
                aim: v(roll, if t % 170 < 85 { -1.0 } else { 0.0 }),
                jump_pressed: t % 90 == 0,
                jump_held: t % 90 < 30,
                lunge_pressed: t % 70 == 35,
                modes: TarModes {
                    sticky: t % 200 < 50,
                    bouncy: t % 300 > 250,
                },
            });
        }
    };
    let mut a = Sim::new(Level::test_room(), v(0.0, 3.0));
    let mut b = a.clone();
    script(&mut a);
    script(&mut b);
    assert_eq!(a.blob.positions, b.blob.positions);
}

#[test]
fn sticky_rolling_climbs_out_of_a_floor_wall_corner() {
    let level = Level {
        solids: vec![
            Solid::rect(v(-50.0, -2.0), v(50.0, 0.0)),
            Solid::rect(v(5.0, -2.0), v(6.0, 50.0)),
        ],
    };
    let mut sim = Sim::new(level, v(3.0, 0.6));
    run(&mut sim, idle(), 1.0);
    run(
        &mut sim,
        Intent {
            roll: 1.0,
            ..with_modes(STICKY)
        },
        2.5,
    );
    let c = sim.blob.center();
    assert!(c.y > 2.5, "wedged in the corner at {c}");
}

#[test]
fn sticky_rolling_climbs_out_of_a_corner_from_any_run_up() {
    // Hitting the wall at speed presses the blob flat against it; it must
    // still be able to roll up, wherever the run-up started.
    let mut stuck = Vec::new();
    for k in 0..20 {
        let x = 1.0 + k as f32 * 0.17;
        let level = Level {
            solids: vec![
                Solid::rect(v(-50.0, -2.0), v(50.0, 0.0)),
                Solid::rect(v(5.0, -2.0), v(6.0, 50.0)),
            ],
        };
        let mut sim = Sim::new(level, v(x, 0.6));
        run(&mut sim, idle(), 1.0);
        run(
            &mut sim,
            Intent {
                roll: 1.0,
                ..with_modes(STICKY)
            },
            2.5,
        );
        if sim.blob.center().y < 2.5 {
            stuck.push(x);
        }
    }
    assert!(stuck.is_empty(), "wedged starting from {stuck:?}");
}

#[test]
fn sticky_rolling_crawls_along_a_ceiling() {
    let mut sim = Sim::new(ceiling(), v(0.0, -0.52));
    run(&mut sim, with_modes(STICKY), 0.5);
    run(
        &mut sim,
        Intent {
            roll: 1.0,
            ..with_modes(STICKY)
        },
        1.5,
    );
    let c = sim.blob.center();
    assert!(c.y > -1.0, "fell off the ceiling: {c}");
    assert!(c.x.abs() > 1.0, "didn't move along it: {c}");
}
