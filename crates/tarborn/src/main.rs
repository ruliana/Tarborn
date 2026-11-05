//! Tarborn's Bevy shell. It samples input, advances the simulation at a fixed
//! rate, and draws the result. All gameplay rules live in `tarborn_sim`.

use bevy::app::RunFixedMainLoopSystems;
use bevy::prelude::*;
use tarborn_sim::{Intent, Level, Sim, TICKS_PER_SECOND, TarModes};

const PIXELS_PER_METER: f32 = 48.0;
const SPAWN: Vec2 = Vec2::new(0.0, 2.0);
const STICK_DEADZONE: f32 = 0.2;

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
}

impl Game {
    fn new() -> Self {
        let sim = Sim::new(Level::test_room(), SPAWN);
        Self {
            previous: sim.blob.positions.clone(),
            sim,
        }
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
    game.previous.clone_from(&game.sim.blob.positions);
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

fn draw(game: Res<Game>, latch: Res<InputLatch>, fixed: Res<Time<Fixed>>, mut gizmos: Gizmos) {
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

    let modes = latch.modes;
    let tar = if game.sim.controller.invulnerable() {
        Color::WHITE
    } else if modes.sticky {
        Color::srgb(0.45, 0.85, 0.3)
    } else if modes.bouncy {
        Color::srgb(0.4, 0.7, 1.0)
    } else {
        Color::srgb(0.95, 0.75, 0.3)
    };
    let outline = game.blob_outline(fixed.overstep_fraction());
    gizmos.linestrip_2d(
        outline.iter().copied().chain(std::iter::once(outline[0])),
        tar,
    );
    for point in &outline {
        gizmos.circle_2d(*point, 2.0, tar);
    }
}
