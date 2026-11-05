//! Tarborn's Bevy shell. It samples input, advances the simulation at a fixed
//! rate, and draws the result. All gameplay rules live in `tarborn_sim`.

mod audio;
mod visuals;

use bevy::app::RunFixedMainLoopSystems;
use bevy::prelude::*;
use tarborn_sim::combat::SPIT_RADIUS;
use tarborn_sim::enemy::{
    BRUTE_SLAM_RADIUS, BRUTE_WINDUP, GUARD_SWING_OFFSET, GUARD_SWING_RADIUS, SPIT_AIM,
};
use tarborn_sim::{
    ARENA_SPAWN, AttackPhase, DT, Enemy, EnemyKind, Intent, Phase, Sim, State, TICKS_PER_SECOND,
    TarModes,
};
use visuals::{Ink, Look, VisualTuning};

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
        .insert_resource(Game::new(Room::Arena))
        .init_resource::<InputLatch>()
        .add_plugins(audio::SoundPlugin)
        .add_plugins(visuals::VisualsPlugin)
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

/// The rooms to play in. T switches between them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Room {
    /// Waves of every enemy kind.
    Arena,
    /// One of each enemy and some terrain, to try things out.
    Training,
}

impl Room {
    fn spawn(self) -> Vec2 {
        match self {
            Room::Arena => ARENA_SPAWN,
            Room::Training => SPAWN,
        }
    }

    /// How far the camera zooms out: the arena shows most of the room.
    fn zoom(self) -> f32 {
        match self {
            Room::Arena => 1.6,
            Room::Training => 1.0,
        }
    }
}

#[derive(Resource)]
struct Game {
    room: Room,
    sim: Sim,
    /// Blob points before the latest tick, for interpolating between ticks.
    previous: Vec<Vec2>,
    /// Enemy positions before the latest tick.
    previous_enemies: Vec<Vec2>,
    /// Seconds since the player died; the room restarts after a moment.
    dead_for: f32,
}

impl Game {
    fn new(room: Room) -> Self {
        let sim = match room {
            Room::Arena => Sim::arena_room(),
            Room::Training => Sim::training_room(SPAWN),
        };
        Self {
            room,
            previous: sim.blob.positions.clone(),
            previous_enemies: sim.enemies.iter().map(|e| e.position).collect(),
            dead_for: 0.0,
            sim,
        }
    }

    fn enemy_position(&self, index: usize, alpha: f32) -> Vec2 {
        let now = self.sim.enemies[index].position;
        // Enemies that dropped in this tick have no previous position yet.
        let before = self.previous_enemies.get(index).copied().unwrap_or(now);
        before.lerp(now, alpha) * PIXELS_PER_METER
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
    modes: TarModes,
}

fn spawn_camera(mut commands: Commands, game: Res<Game>) {
    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scale: game.room.zoom(),
            ..OrthographicProjection::default_2d()
        }),
        Transform::from_translation((game.room.spawn() * PIXELS_PER_METER).extend(0.0)),
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
    // Two verbs. Grip shrinks (small, sticky, charging). Jump expands: jump,
    // burst when shrunk, lunge at enemies in the air, slam with down, hold to
    // bounce, throw while holding an enemy.
    let jump_keys = [KeyCode::Space, KeyCode::KeyJ];
    let mut jump_pressed = keys.any_just_pressed(jump_keys);
    let mut jump_held = keys.any_pressed(jump_keys);
    let mut modes = TarModes {
        sticky: any_key(&[KeyCode::KeyK, KeyCode::ShiftLeft]),
        bouncy: false,
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
        modes.sticky |= [
            GamepadButton::RightTrigger2,
            GamepadButton::RightTrigger,
            GamepadButton::LeftTrigger2,
            GamepadButton::LeftTrigger,
            GamepadButton::West,
        ]
        .into_iter()
        .any(|b| pad.pressed(b));
    }

    let aim = aim.clamp_length_max(1.0);
    latch.roll = aim.x.clamp(-1.0, 1.0);
    latch.aim = aim;
    latch.jump_pressed |= jump_pressed;
    latch.jump_held = jump_held;
    latch.modes = modes;

    if keys.just_pressed(KeyCode::KeyR) {
        *game = Game::new(game.room);
    }
    if keys.just_pressed(KeyCode::KeyT) {
        let other = match game.room {
            Room::Arena => Room::Training,
            Room::Training => Room::Arena,
        };
        *game = Game::new(other);
    }
}

fn tick(
    mut game: ResMut<Game>,
    mut latch: ResMut<InputLatch>,
    mut sound: ResMut<audio::SoundCues>,
) {
    let intent = Intent {
        roll: latch.roll,
        aim: latch.aim,
        jump_pressed: latch.jump_pressed,
        jump_held: latch.jump_held,
        lunge_pressed: false,
        modes: latch.modes,
    };
    latch.jump_pressed = false;
    let game = &mut *game;
    if game.sim.dead {
        game.dead_for += DT;
        if game.dead_for > RESTART_DELAY {
            *game = Game::new(game.room);
        }
        return;
    }
    game.previous.clone_from(&game.sim.blob.positions);
    game.previous_enemies.clear();
    game.previous_enemies
        .extend(game.sim.enemies.iter().map(|e| e.position));
    let events = game.sim.step(intent);
    sound.observe(&game.sim, &events);
}

fn follow_camera(
    game: Res<Game>,
    fixed: Res<Time<Fixed>>,
    time: Res<Time>,
    camera: Single<(&mut Transform, &mut Projection), With<Camera2d>>,
) {
    let (mut camera, mut projection) = camera.into_inner();
    if let Projection::Orthographic(ortho) = &mut *projection
        && ortho.scale != game.room.zoom()
    {
        ortho.scale = game.room.zoom();
    }
    let outline = game.blob_outline(fixed.overstep_fraction());
    let center = outline.iter().copied().sum::<Vec2>() / outline.len() as f32;
    let target = center.extend(camera.translation.z);
    let smoothing = 1.0 - (-6.0 * time.delta_secs()).exp();
    camera.translation = camera.translation.lerp(target, smoothing);
}

/// The blob's state as a color, or `None` in its resting state. The line look
/// colors the outline with it; the filled looks show it as a rim on the tar.
fn tar_state_color(game: &Game, latch: &InputLatch, now: f32) -> Option<Color> {
    let sim = &game.sim;
    // Blink while invulnerable after being hurt; white while lunging.
    let blink = ((now * 16.0) as u32).is_multiple_of(2);
    if sim.dead {
        Some(Color::srgb(0.4, 0.4, 0.4))
    } else if sim.controller.invulnerable() {
        Some(Color::WHITE)
    } else if sim.controller.is_shrunk() {
        // Green deepening to red as the shrink charges.
        let k = sim.controller.charge();
        Some(Color::srgb(0.45 + 0.5 * k, 0.85 - 0.6 * k, 0.3))
    } else if sim.player_invulnerable() && blink {
        Some(Color::srgb(0.95, 0.3, 0.3))
    } else if latch.modes.sticky {
        Some(Color::srgb(0.45, 0.85, 0.3))
    } else if latch.jump_held {
        Some(Color::srgb(0.4, 0.7, 1.0))
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    game: Res<Game>,
    latch: Res<InputLatch>,
    fixed: Res<Time<Fixed>>,
    time: Res<Time>,
    camera: Single<&Transform, With<Camera2d>>,
    window: Single<&Window>,
    look: Res<Look>,
    tuning: Res<VisualTuning>,
    mut gizmos: Gizmos,
) {
    let alpha = fixed.overstep_fraction();
    let sim = &game.sim;
    let ink = Ink::new(*look, &tuning);
    // The filled looks draw the level, tar, pickups and shots as meshes;
    // gizmos keep only outlines, marks and the HUD there.
    let lines = !look.filled();
    let ground = Color::srgb(0.55, 0.5, 0.45);
    for solid in game.sim.level.solids.iter().filter(|_| lines) {
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
    for hazard in sim.hazards.iter().filter(|_| lines) {
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
    for pickup in sim.pickups.iter().filter(|_| lines) {
        gizmos.circle_2d(
            *pickup * PIXELS_PER_METER,
            0.2 * PIXELS_PER_METER,
            Color::srgb(0.95, 0.75, 0.3),
        );
    }

    for (i, enemy) in sim.enemies.iter().enumerate() {
        if enemy.alive() {
            draw_enemy(
                &mut gizmos,
                enemy,
                game.enemy_position(i, alpha),
                time.elapsed_secs(),
                ink,
            );
            // A tar ring on an enemy that is held or flying after a throw.
            if lines && (enemy.held || enemy.thrown) {
                gizmos.circle_2d(
                    game.enemy_position(i, alpha) * PIXELS_PER_METER,
                    (enemy.radius + 0.12) * PIXELS_PER_METER,
                    Color::srgb(0.45, 0.85, 0.3),
                );
            }
        }
    }

    // Spit in flight.
    for shot in sim.projectiles.iter().filter(|_| lines) {
        gizmos.circle_2d(
            shot.position * PIXELS_PER_METER,
            SPIT_RADIUS * PIXELS_PER_METER,
            Color::srgb(1.0, 0.45, 0.2),
        );
    }

    // The HUD is laid out in screen pixels, scaled by the zoom.
    let zoom = game.room.zoom();
    let half = Vec2::new(window.width(), window.height()) * 0.5 * zoom;
    let view = camera.translation.truncate();
    // Health pips in the top-left corner of the screen.
    let corner = view + Vec2::new(-half.x, half.y) + Vec2::new(24.0, -24.0) * zoom;
    for i in 0..sim.max_health {
        let at = corner + Vec2::X * (i as f32 * 22.0 * zoom);
        let color = if i < sim.health {
            Color::srgb(0.95, 0.75, 0.3)
        } else {
            Color::srgb(0.3, 0.3, 0.3)
        };
        gizmos.circle_2d(at, 8.0 * zoom, ink.apply(color));
    }
    draw_hud(&mut gizmos, sim, view, half, zoom, ink);

    if !lines {
        return;
    }
    let tar =
        tar_state_color(&game, &latch, time.elapsed_secs()).unwrap_or(Color::srgb(0.95, 0.75, 0.3));
    let outline = game.blob_outline(alpha);
    gizmos.linestrip_2d(
        outline.iter().copied().chain(std::iter::once(outline[0])),
        tar,
    );
    for point in &outline {
        gizmos.circle_2d(*point, 2.0, tar);
    }
}

/// Enemies are plain shapes whose colour and marks show their state, so the
/// player can read how vulnerable each one is at a glance.
fn draw_enemy(gizmos: &mut Gizmos, enemy: &Enemy, at: Vec2, now: f32, ink: Ink) {
    let r = enemy.radius * PIXELS_PER_METER;
    let facing = Vec2::X * enemy.facing;
    let base = match enemy.kind {
        EnemyKind::Dummy => ink.apply(Color::srgb(0.6, 0.6, 0.65)),
        EnemyKind::Grub => ink.apply(Color::srgb(0.75, 0.55, 0.85)),
        EnemyKind::Guard => ink.apply(Color::srgb(0.45, 0.6, 0.95)),
        EnemyKind::Spitter => ink.apply(Color::srgb(0.95, 0.6, 0.35)),
        EnemyKind::Skitter => ink.apply(Color::srgb(0.95, 0.9, 0.4)),
        EnemyKind::Brute => ink.apply(Color::srgb(0.75, 0.35, 0.3)),
    };
    let flash = ((now * 20.0) as u32).is_multiple_of(2);
    let color = match enemy.state {
        _ if enemy.hit_cooldown > 0.0 => ink.apply(Color::WHITE),
        State::Attacking(AttackPhase::Windup) if flash => ink.apply(Color::srgb(1.0, 0.3, 0.2)),
        State::Attacking(AttackPhase::Strike) => ink.apply(Color::srgb(1.0, 0.3, 0.2)),
        State::Attacking(AttackPhase::Recover) | State::Dizzy | State::Prone => {
            ink.apply(Color::srgb(0.5, 0.5, 0.5))
        }
        State::Fleeing => ink.apply(Color::srgb(0.4, 0.9, 0.4)),
        _ => base,
    };

    // Body.
    match (enemy.kind, enemy.state) {
        (_, State::Prone) => {
            gizmos.rect_2d(
                Isometry2d::from_translation(at - Vec2::Y * r * 0.5),
                Vec2::new(r * 2.4, r),
                color,
            );
        }
        (EnemyKind::Guard, _) => {
            gizmos.rect_2d(
                Isometry2d::from_translation(at),
                Vec2::new(r * 1.6, r * 2.0),
                color,
            );
        }
        (EnemyKind::Dummy, _) => {
            gizmos.circle_2d(at, r, color);
            gizmos.line_2d(at - Vec2::X * r * 0.5, at + Vec2::X * r * 0.5, color);
            gizmos.line_2d(at - Vec2::Y * r * 0.5, at + Vec2::Y * r * 0.5, color);
        }
        (EnemyKind::Grub, _) => {
            gizmos.circle_2d(at, r, color);
        }
        (EnemyKind::Spitter, _) => {
            let corners = [Vec2::X, Vec2::Y, Vec2::NEG_X, Vec2::NEG_Y, Vec2::X];
            gizmos.linestrip_2d(corners.iter().map(|c| at + *c * r * 1.2), color);
        }
        (EnemyKind::Skitter, _) => {
            // A triangle pointing the way it faces.
            let tip = at + facing * r * 1.3;
            let back_up = at - facing * r + Vec2::Y * r;
            let back_down = at - facing * r - Vec2::Y * r;
            gizmos.linestrip_2d([tip, back_up, back_down, tip], color);
        }
        (EnemyKind::Brute, _) => {
            // Raised up during the wind-up, so the slam is easy to read.
            let lift = if enemy.state == State::Attacking(AttackPhase::Windup) {
                (enemy.state_time / BRUTE_WINDUP).min(1.0) * r * 0.4
            } else {
                0.0
            };
            gizmos.rect_2d(
                Isometry2d::from_translation(at + Vec2::Y * lift),
                Vec2::splat(r * 2.0),
                color,
            );
        }
    }
    // The reach of a brute's slam: drawn faintly as it winds up, solid as it lands.
    if enemy.kind == EnemyKind::Brute {
        let radius = BRUTE_SLAM_RADIUS * PIXELS_PER_METER;
        let ground = at - Vec2::Y * r;
        match enemy.state {
            State::Attacking(AttackPhase::Windup) => {
                let k = (enemy.state_time / BRUTE_WINDUP).min(1.0);
                gizmos.line_2d(
                    ground - Vec2::X * radius * k,
                    ground + Vec2::X * radius * k,
                    ink.apply(Color::srgba(1.0, 0.3, 0.2, 0.6)),
                );
            }
            State::Attacking(AttackPhase::Strike) => {
                gizmos.circle_2d(at, radius, ink.apply(Color::srgb(1.0, 0.3, 0.2)));
            }
            _ => {}
        }
    }
    // A spitter's aim line while it winds up, so you can see the shot coming.
    if enemy.kind == EnemyKind::Spitter
        && enemy.state == State::Attacking(AttackPhase::Windup)
        && let Some(target) = enemy.target
    {
        let to = target * PIXELS_PER_METER;
        let k = (enemy.state_time / SPIT_AIM).min(1.0);
        gizmos.line_2d(
            at,
            at.lerp(to, k),
            ink.apply(Color::srgba(1.0, 0.4, 0.2, 0.5)),
        );
    }
    if enemy.kind != EnemyKind::Dummy && enemy.state != State::Prone {
        // An eye on the side it faces.
        gizmos.circle_2d(at + facing * r * 0.45 + Vec2::Y * r * 0.3, 2.5, color);
    }

    // Guard shield and swing.
    if enemy.kind == EnemyKind::Guard {
        let shield_up = matches!(
            enemy.state,
            State::Defending | State::Attacking(AttackPhase::Windup)
        );
        if shield_up {
            let x = at + facing * (r * 0.8 + 6.0);
            gizmos.line_2d(
                x - Vec2::Y * r,
                x + Vec2::Y * r,
                ink.apply(Color::srgb(0.4, 0.95, 1.0)),
            );
        }
        if enemy.striking() {
            let center = at + facing * GUARD_SWING_OFFSET * PIXELS_PER_METER;
            gizmos.circle_2d(
                center,
                GUARD_SWING_RADIUS * PIXELS_PER_METER,
                ink.apply(Color::srgb(1.0, 0.3, 0.2)),
            );
        }
    }

    // A mark above the head for the states that matter.
    let head = at + Vec2::Y * (r + 14.0);
    match enemy.state {
        State::Surprised => {
            let white = ink.apply(Color::WHITE);
            gizmos.line_2d(head, head + Vec2::Y * 12.0, white);
            gizmos.circle_2d(head - Vec2::Y * 5.0, 1.5, white);
        }
        // Skitters are always searching, so it isn't marked on them.
        State::Searching if enemy.kind != EnemyKind::Skitter => {
            gizmos.circle_2d(
                head + Vec2::Y * 4.0,
                6.0,
                ink.apply(Color::srgb(1.0, 0.6, 0.2)),
            );
        }
        State::Dodging => {
            gizmos.line_2d(
                head - Vec2::X * 8.0,
                head + Vec2::X * 8.0,
                ink.apply(Color::WHITE),
            );
        }
        State::Dizzy => {
            for k in 0..3 {
                let angle = now * 6.0 + k as f32 * std::f32::consts::TAU / 3.0;
                gizmos.circle_2d(
                    head + Vec2::new(angle.cos() * 10.0, angle.sin() * 3.0),
                    2.5,
                    ink.apply(Color::srgb(1.0, 0.9, 0.3)),
                );
            }
        }
        _ => {}
    }

    if enemy.health < enemy.max_health {
        let left = at + Vec2::new(-r, r + 6.0);
        let full = 2.0 * r;
        let filled = full * enemy.health / enemy.max_health;
        gizmos.line_2d(
            left,
            left + Vec2::X * full,
            ink.apply(Color::srgb(0.3, 0.3, 0.3)),
        );
        gizmos.line_2d(
            left,
            left + Vec2::X * filled,
            ink.apply(Color::srgb(0.9, 0.3, 0.3)),
        );
    }
}

/// Wave and combo counters, drawn with line digits so no font is needed.
fn draw_hud(gizmos: &mut Gizmos, sim: &Sim, center: Vec2, half: Vec2, z: f32, ink: Ink) {
    let top_right = center + half - Vec2::splat(24.0 * z);
    // Combo: the chain under way, in the top middle, and the best one beside it.
    let top = center + Vec2::new(0.0, half.y - 30.0 * z);
    if sim.combo.count >= 2 {
        let fade = (sim.combo.left / tarborn_sim::COMBO_WINDOW).clamp(0.3, 1.0);
        let color = ink.apply(Color::srgba(1.0, 0.85, 0.3, fade));
        draw_number(gizmos, sim.combo.count, top, 18.0 * z, color);
        // The time left in the chain, as a shrinking bar.
        let width = 60.0 * z * sim.combo.left / tarborn_sim::COMBO_WINDOW;
        let y = top.y - 22.0 * z;
        gizmos.line_2d(
            Vec2::new(top.x - width * 0.5, y),
            Vec2::new(top.x + width * 0.5, y),
            color,
        );
    }
    if sim.combo.best >= 2 {
        let grey = ink.apply(Color::srgb(0.5, 0.5, 0.5));
        let at = top + Vec2::new(90.0, 4.0) * z;
        draw_number(gizmos, sim.combo.best, at, 10.0 * z, grey);
    }

    let Some(arena) = &sim.arena else {
        return;
    };
    // Waves: a box per wave, filled in once it is beaten.
    let total = arena.waves.len();
    for i in 0..total {
        let at = top_right - Vec2::X * ((total - 1 - i) as f32 * 20.0 * z);
        let done = i < arena.wave || arena.phase == Phase::Cleared;
        let current = i == arena.wave && arena.phase == Phase::Fighting;
        let color = if done {
            ink.apply(Color::srgb(0.45, 0.85, 0.3))
        } else if current {
            ink.apply(Color::srgb(1.0, 0.85, 0.3))
        } else {
            ink.apply(Color::srgb(0.35, 0.35, 0.35))
        };
        let box_at = Isometry2d::from_translation(at);
        gizmos.rect_2d(box_at, Vec2::splat(12.0 * z), color);
        if done {
            gizmos.rect_2d(box_at, Vec2::splat(6.0 * z), color);
        }
    }
    // Between waves, the number of the one coming, big in the middle.
    let middle = center + Vec2::Y * 120.0 * z;
    match arena.phase {
        Phase::Break(left) if left < tarborn_sim::arena::WAVE_BREAK - 0.3 || arena.wave == 0 => {
            let pulse = 0.5 + 0.5 * (left * 6.0).sin().abs();
            let color = ink.apply(Color::srgba(1.0, 0.85, 0.3, pulse));
            draw_number(gizmos, arena.wave as u32 + 1, middle, 40.0 * z, color);
        }
        Phase::Cleared => {
            // A big tick.
            let green = ink.apply(Color::srgb(0.45, 0.85, 0.3));
            let points = [
                Vec2::new(-40.0, 0.0),
                Vec2::new(-10.0, -30.0),
                Vec2::new(45.0, 35.0),
            ];
            gizmos.linestrip_2d(points.map(|p| middle + p * z), green);
        }
        _ => {}
    }
}

/// Draws `value` centered on `at`, in seven-segment digits `height` tall.
fn draw_number(gizmos: &mut Gizmos, value: u32, at: Vec2, height: f32, color: Color) {
    let digits: Vec<u32> = value
        .to_string()
        .chars()
        .filter_map(|c| c.to_digit(10))
        .collect();
    let w = height * 0.5;
    let gap = height * 0.25;
    let total = digits.len() as f32 * (w + gap) - gap;
    for (i, digit) in digits.iter().enumerate() {
        let left = at.x - total * 0.5 + i as f32 * (w + gap);
        draw_digit(gizmos, *digit, Vec2::new(left, at.y), w, height, color);
    }
}

fn draw_digit(gizmos: &mut Gizmos, digit: u32, left_mid: Vec2, w: f32, h: f32, color: Color) {
    // Segments: top, top-right, bottom-right, bottom, bottom-left, top-left, middle.
    const SEGMENTS: [u8; 10] = [
        0b0111111, 0b0000110, 0b1011011, 0b1001111, 0b1100110, 0b1101101, 0b1111101, 0b0000111,
        0b1111111, 0b1101111,
    ];
    let (l, r) = (left_mid.x, left_mid.x + w);
    let (t, m, b) = (left_mid.y + h * 0.5, left_mid.y, left_mid.y - h * 0.5);
    let lines = [
        (Vec2::new(l, t), Vec2::new(r, t)),
        (Vec2::new(r, t), Vec2::new(r, m)),
        (Vec2::new(r, m), Vec2::new(r, b)),
        (Vec2::new(l, b), Vec2::new(r, b)),
        (Vec2::new(l, m), Vec2::new(l, b)),
        (Vec2::new(l, t), Vec2::new(l, m)),
        (Vec2::new(l, m), Vec2::new(r, m)),
    ];
    let mask = SEGMENTS[digit as usize % 10];
    for (k, (a, b)) in lines.iter().enumerate() {
        if mask & (1 << k) != 0 {
            gizmos.line_2d(*a, *b, color);
        }
    }
}
