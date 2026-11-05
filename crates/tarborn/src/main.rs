//! Tarborn's Bevy shell. It samples input, advances the simulation at a fixed
//! rate, and draws the result. All gameplay rules live in `tarborn_sim`.

use bevy::app::RunFixedMainLoopSystems;
use bevy::prelude::*;
use tarborn_sim::{DT, Intent, Sim, TICKS_PER_SECOND, TarModes};

const PIXELS_PER_METER: f32 = 48.0;
const SPAWN: Vec2 = Vec2::new(0.0, 2.0);
const STICK_DEADZONE: f32 = 0.2;
const RESTART_DELAY: f32 = 1.0;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Tarborn".into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(Time::<Fixed>::from_hz(TICKS_PER_SECOND as f64))
        .insert_resource(ClearColor(Color::srgb(0.07, 0.06, 0.08)))
        .insert_resource(Game::new())
        .init_resource::<InputLatch>()
        .add_systems(Startup, spawn_camera)
        // Sample input after Bevy has updated devices this frame but before the
        // fixed ticks run, so a press is simulated in the same frame it happened.
        .add_systems(
            RunFixedMainLoop,
            read_input.in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
        )
        .add_systems(FixedUpdate, tick)
        .add_systems(Update, (follow_camera, draw).chain())
        .run();
}

#[derive(Resource)]
struct Game {
    sim: Sim,
    /// Blob points before the latest tick, for interpolating between ticks.
    previous: Vec<Vec2>,
    /// Enemy positions before the latest tick.
    previous_enemies: Vec<Vec2>,
    /// Seconds since the player died; the room restarts after a moment.
    dead_for: f32,
}

impl Game {
    fn new() -> Self {
        let sim = Sim::training_room(SPAWN);
        Self {
            previous: sim.blob.positions.clone(),
            previous_enemies: sim.enemies.iter().map(|e| e.position).collect(),
            dead_for: 0.0,
            sim,
        }
    }

    fn enemy_position(&self, index: usize, alpha: f32) -> Vec2 {
        self.previous_enemies[index].lerp(self.sim.enemies[index].position, alpha)
            * PIXELS_PER_METER
    }

    /// Blob outline in world pixels, interpolated `alpha` of the way from the
    /// previous tick to the current one so motion is smooth at any frame rate.
    fn blob_outline(&self, alpha: f32) -> Vec<Vec2> {
        self.previous
            .iter()
            .zip(&self.sim.blob.positions)
            .map(|(a, b)| a.lerp(*b, alpha) * PIXELS_PER_METER)
            .collect()
    }
}

/// Input gathered every frame and consumed by the fixed tick. A jump press is
/// latched until a tick consumes it, so no press is ever lost between ticks.
#[derive(Resource, Default)]
struct InputLatch {
    roll: f32,
    aim: Vec2,
    jump_pressed: bool,
    jump_held: bool,
    lunge_pressed: bool,
    modes: TarModes,
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        Transform::from_translation((SPAWN * PIXELS_PER_METER).extend(0.0)),
    ));
}

fn read_input(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut latch: ResMut<InputLatch>,
    mut game: ResMut<Game>,
) {
    let any_key = |codes: &[KeyCode]| keys.any_pressed(codes.iter().copied());
    let mut aim = Vec2::ZERO;
    if any_key(&[KeyCode::ArrowLeft, KeyCode::KeyA]) {
        aim.x -= 1.0;
    }
    if any_key(&[KeyCode::ArrowRight, KeyCode::KeyD]) {
        aim.x += 1.0;
    }
    if any_key(&[KeyCode::ArrowUp, KeyCode::KeyW]) {
        aim.y += 1.0;
    }
    if any_key(&[KeyCode::ArrowDown, KeyCode::KeyS]) {
        aim.y -= 1.0;
    }
    let jump_keys = [KeyCode::Space];
    let mut jump_pressed = keys.any_just_pressed(jump_keys);
    let mut jump_held = keys.any_pressed(jump_keys);
    let mut lunge_pressed = keys.any_just_pressed([KeyCode::KeyJ, KeyCode::KeyX]);
    let mut modes = TarModes {
        sticky: any_key(&[KeyCode::KeyK, KeyCode::ShiftLeft]),
        bouncy: any_key(&[KeyCode::KeyL]),
    };

    for pad in &gamepads {
        let stick = pad.left_stick();
        if stick.length() > STICK_DEADZONE {
            aim += stick;
        }
        for (button, dir) in [
            (GamepadButton::DPadLeft, Vec2::NEG_X),
            (GamepadButton::DPadRight, Vec2::X),
            (GamepadButton::DPadUp, Vec2::Y),
            (GamepadButton::DPadDown, Vec2::NEG_Y),
        ] {
            if pad.pressed(button) {
                aim += dir;
            }
        }
        jump_pressed |= pad.just_pressed(GamepadButton::South);
        jump_held |= pad.pressed(GamepadButton::South);
        lunge_pressed |= pad.just_pressed(GamepadButton::West);
        modes.sticky |=
            pad.pressed(GamepadButton::RightTrigger2) || pad.pressed(GamepadButton::RightTrigger);
        modes.bouncy |=
            pad.pressed(GamepadButton::LeftTrigger2) || pad.pressed(GamepadButton::LeftTrigger);
    }

    let aim = aim.clamp_length_max(1.0);
    latch.roll = aim.x.clamp(-1.0, 1.0);
    latch.aim = aim;
    latch.jump_pressed |= jump_pressed;
    latch.jump_held = jump_held;
    latch.lunge_pressed |= lunge_pressed;
    latch.modes = modes;

    if keys.just_pressed(KeyCode::KeyR) {
        *game = Game::new();
    }
}

fn tick(mut game: ResMut<Game>, mut latch: ResMut<InputLatch>) {
    let intent = Intent {
        roll: latch.roll,
        aim: latch.aim,
        jump_pressed: latch.jump_pressed,
        jump_held: latch.jump_held,
        lunge_pressed: latch.lunge_pressed,
        modes: latch.modes,
    };
    latch.jump_pressed = false;
    latch.lunge_pressed = false;
    let game = &mut *game;
    if game.sim.dead {
        game.dead_for += DT;
        if game.dead_for > RESTART_DELAY {
            *game = Game::new();
        }
        return;
    }
    game.previous.clone_from(&game.sim.blob.positions);
    game.previous_enemies.clear();
    game.previous_enemies
        .extend(game.sim.enemies.iter().map(|e| e.position));
    game.sim.step(intent);
}

fn follow_camera(
    game: Res<Game>,
    fixed: Res<Time<Fixed>>,
    time: Res<Time>,
    mut camera: Single<&mut Transform, With<Camera2d>>,
) {
    let outline = game.blob_outline(fixed.overstep_fraction());
    let center = outline.iter().copied().sum::<Vec2>() / outline.len() as f32;
    let target = center.extend(camera.translation.z);
    let smoothing = 1.0 - (-6.0 * time.delta_secs()).exp();
    camera.translation = camera.translation.lerp(target, smoothing);
}

fn draw(
    game: Res<Game>,
    latch: Res<InputLatch>,
    fixed: Res<Time<Fixed>>,
    time: Res<Time>,
    camera: Single<&Transform, With<Camera2d>>,
    window: Single<&Window>,
    mut gizmos: Gizmos,
) {
    let alpha = fixed.overstep_fraction();
    let sim = &game.sim;
    let ground = Color::srgb(0.55, 0.5, 0.45);
    for solid in &game.sim.level.solids {
        let first = solid.vertices[0];
        gizmos.linestrip_2d(
            solid
                .vertices
                .iter()
                .chain(std::iter::once(&first))
                .map(|v| *v * PIXELS_PER_METER),
            ground,
        );
    }

    // Spikes: a zigzag along the top of each hazard.
    let spike = Color::srgb(0.95, 0.25, 0.25);
    for hazard in &sim.hazards {
        let (min, max) = hazard
            .vertices
            .iter()
            .fold((Vec2::MAX, Vec2::MIN), |(lo, hi), v| {
                (lo.min(*v), hi.max(*v))
            });
        let teeth = ((max.x - min.x) / 0.3).ceil().max(1.0) as usize;
        let width = (max.x - min.x) / teeth as f32;
        let points = (0..=teeth * 2).map(|i| {
            let x = min.x + width * i as f32 * 0.5;
            let y = if i % 2 == 1 { max.y } else { min.y };
            Vec2::new(x, y) * PIXELS_PER_METER
        });
        gizmos.linestrip_2d(points, spike);
    }

    // Tar drops that heal.
    for pickup in &sim.pickups {
        gizmos.circle_2d(
            *pickup * PIXELS_PER_METER,
            0.2 * PIXELS_PER_METER,
            Color::srgb(0.95, 0.75, 0.3),
        );
    }

    // Enemies: a circle with a cross, and a health bar above while hurt.
    for (i, enemy) in sim.enemies.iter().enumerate() {
        if !enemy.alive() {
            continue;
        }
        let at = game.enemy_position(i, alpha);
        let r = enemy.radius * PIXELS_PER_METER;
        let flash = enemy.hit_cooldown > 0.0;
        let color = if flash {
            Color::WHITE
        } else {
            Color::srgb(0.75, 0.55, 0.85)
        };
        gizmos.circle_2d(at, r, color);
        gizmos.line_2d(at - Vec2::X * r * 0.5, at + Vec2::X * r * 0.5, color);
        gizmos.line_2d(at - Vec2::Y * r * 0.5, at + Vec2::Y * r * 0.5, color);
        if enemy.health < enemy.max_health {
            let left = at + Vec2::new(-r, r + 8.0);
            let full = 2.0 * r;
            let filled = full * enemy.health / enemy.max_health;
            gizmos.line_2d(left, left + Vec2::X * full, Color::srgb(0.3, 0.3, 0.3));
            gizmos.line_2d(left, left + Vec2::X * filled, Color::srgb(0.9, 0.3, 0.3));
        }
    }

    // Health pips in the top-left corner of the screen.
    let corner = camera.translation.truncate()
        + Vec2::new(-window.width() * 0.5 + 24.0, window.height() * 0.5 - 24.0);
    for i in 0..sim.max_health {
        let at = corner + Vec2::X * (i as f32 * 22.0);
        let color = if i < sim.health {
            Color::srgb(0.95, 0.75, 0.3)
        } else {
            Color::srgb(0.3, 0.3, 0.3)
        };
        gizmos.circle_2d(at, 8.0, color);
    }

    let modes = latch.modes;
    // Blink while invulnerable after being hurt; white while lunging.
    let blink = ((time.elapsed_secs() * 16.0) as u32).is_multiple_of(2);
    let tar = if sim.dead {
        Color::srgb(0.4, 0.4, 0.4)
    } else if sim.controller.invulnerable() {
        Color::WHITE
    } else if sim.controller.is_coiling() {
        // Green deepening to red as the coil charges.
        let k = sim.controller.coil_power();
        Color::srgb(0.45 + 0.5 * k, 0.85 - 0.6 * k, 0.3)
    } else if sim.player_invulnerable() && blink {
        Color::srgb(0.95, 0.3, 0.3)
    } else if modes.sticky {
        Color::srgb(0.45, 0.85, 0.3)
    } else if modes.bouncy {
        Color::srgb(0.4, 0.7, 1.0)
    } else {
        Color::srgb(0.95, 0.75, 0.3)
    };
    let outline = game.blob_outline(alpha);
    gizmos.linestrip_2d(
        outline.iter().copied().chain(std::iter::once(outline[0])),
        tar,
    );
    for point in &outline {
        gizmos.circle_2d(*point, 2.0, tar);
    }
}
