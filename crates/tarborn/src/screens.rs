//! The Bevy side of the run loop in [`crate::flow`]: reads the screen
//! keys, builds stages off the main thread, and draws the start, loading,
//! transition and run-over screens over the world.

use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use tarborn_pcg::Goal;
use tarborn_sim::Sim;

use crate::flow::{BUILT_IN, Flow, Mode, Order, RunParams, Screen, Signal};
use crate::{Game, Room, rooms};

const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/run.ron");
/// Pale type on black, as in the Pitch look.
const INK: Color = Color::srgb(0.92, 0.9, 0.86);
const DIM_INK: Color = Color::srgb(0.6, 0.58, 0.55);
/// Seeds are typed as decimal digits; longer ones stop taking digits.
const MAX_SEED: u64 = 999_999_999_999;

#[derive(Resource)]
pub struct RunFlow(pub Flow);

/// Signals for the flow, gathered from input and the simulation.
#[derive(Resource, Default)]
pub struct Signals(pub Vec<Signal>);

#[derive(Resource, Default)]
struct Loading(Option<Task<Result<(Sim, Goal), String>>>);

#[derive(Component)]
struct Overlay;

#[derive(Component)]
struct Title;

#[derive(Component)]
struct Lines;

pub struct ScreensPlugin;

impl Plugin for ScreensPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(RunFlow(Flow::new(load_params(), random_seed())))
            .init_resource::<Signals>()
            .init_resource::<Loading>()
            .add_systems(Startup, spawn_ui)
            .add_systems(
                Update,
                (screen_input, run_flow, poll_loading, update_ui)
                    .chain()
                    .before(crate::follow_camera),
            );
    }
}

fn load_params() -> RunParams {
    std::fs::read_to_string(PATH)
        .map_err(|e| e.to_string())
        .and_then(|text| ron::from_str(&text).map_err(|e| e.to_string()))
        .unwrap_or_else(|e| {
            warn!("{PATH}: {e}; using the built-in run parameters");
            ron::from_str(BUILT_IN).expect("built-in run.ron is valid")
        })
}

fn random_seed() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64);
    tarborn_pcg::Rng::new(nanos).next_u64() % 1_000_000
}

fn screen_input(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    flow: Res<RunFlow>,
    mut signals: ResMut<Signals>,
) {
    let flow = &flow.0;
    let jump = keys.any_just_pressed([KeyCode::Space, KeyCode::KeyJ])
        || gamepads
            .iter()
            .any(|pad| pad.just_pressed(GamepadButton::South));
    let back = keys.just_pressed(KeyCode::Escape)
        || gamepads
            .iter()
            .any(|pad| pad.just_pressed(GamepadButton::Select));
    if back {
        signals.0.push(Signal::Back);
    }
    match flow.screen {
        Screen::Start => {
            for (key, room) in [
                (KeyCode::F1, "training"),
                (KeyCode::F2, "arena"),
                (KeyCode::F3, "dressing"),
                (KeyCode::F4, "dark"),
            ] {
                if keys.just_pressed(key) {
                    signals.0.push(Signal::Room(room.to_string()));
                }
            }
            let mut seed = flow.seed;
            for (i, key) in [
                KeyCode::Digit0,
                KeyCode::Digit1,
                KeyCode::Digit2,
                KeyCode::Digit3,
                KeyCode::Digit4,
                KeyCode::Digit5,
                KeyCode::Digit6,
                KeyCode::Digit7,
                KeyCode::Digit8,
                KeyCode::Digit9,
            ]
            .into_iter()
            .enumerate()
            {
                if keys.just_pressed(key) && seed <= MAX_SEED / 10 {
                    seed = seed * 10 + i as u64;
                }
            }
            if keys.just_pressed(KeyCode::Backspace) {
                seed /= 10;
            }
            if keys.just_pressed(KeyCode::KeyR) {
                seed = random_seed();
            }
            for pad in &gamepads {
                if pad.just_pressed(GamepadButton::DPadUp) {
                    seed = (seed + 1).min(MAX_SEED);
                }
                if pad.just_pressed(GamepadButton::DPadDown) {
                    seed = seed.saturating_sub(1);
                }
            }
            if seed != flow.seed {
                signals.0.push(Signal::Seed(seed));
            }
            if jump {
                signals.0.push(Signal::Jump);
            }
        }
        Screen::RunOver if jump => signals.0.push(Signal::Jump),
        _ => {}
    }
}

fn run_flow(
    time: Res<Time>,
    mut flow: ResMut<RunFlow>,
    mut signals: ResMut<Signals>,
    mut game: ResMut<Game>,
    mut loading: ResMut<Loading>,
) {
    let incoming = std::mem::take(&mut signals.0);
    let orders = flow.0.update(time.delta_secs(), &incoming);
    for order in orders {
        match order {
            Order::Load {
                stage,
                seed,
                health,
            } => {
                let params = flow.0.params.clone();
                let id = params.stages[stage].clone();
                info!("loading stage {} ({id}) of seed {seed}", stage + 1);
                // Stages are placeholders for now and ignore the seed; the
                // assembler will build them from it.
                let task = AsyncComputeTaskPool::get().spawn(async move {
                    let (mut sim, exit) = rooms::load_stage(&id, params.exit_depth)?;
                    if let Some(health) = health {
                        sim.health = health.clamp(1, sim.max_health);
                    }
                    Ok((sim, exit))
                });
                loading.0 = Some(task);
            }
            Order::OpenRoom(id) => *game = Game::new(Room::from_id(&id)),
            // The telemetry hook: every screen change is logged.
            Order::Entered(screen) => info!("run: {screen:?}"),
        }
    }
}

fn poll_loading(
    mut loading: ResMut<Loading>,
    mut game: ResMut<Game>,
    mut signals: ResMut<Signals>,
    mut flow: ResMut<RunFlow>,
) {
    let Some(task) = &mut loading.0 else {
        return;
    };
    let Some(result) = block_on(poll_once(task)) else {
        return;
    };
    loading.0 = None;
    match result {
        Ok((sim, exit)) => {
            *game = Game::stage(sim, exit);
            signals.0.push(Signal::Loaded);
        }
        Err(e) => {
            error!("can't load the stage: {e}");
            flow.0.update(0.0, &[Signal::Back]);
        }
    }
}

fn spawn_ui(mut commands: Commands, flow: Res<RunFlow>) {
    let p = &flow.0.params;
    commands
        .spawn((
            Overlay,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Px(p.text_size * 0.8),
                ..default()
            },
            BackgroundColor(Color::BLACK.with_alpha(0.0)),
            GlobalZIndex(100),
        ))
        .with_children(|root| {
            root.spawn((
                Title,
                Text::new(""),
                TextFont {
                    font_size: bevy::text::FontSize::Px(p.title_size),
                    ..default()
                },
                TextColor(INK),
            ));
            root.spawn((
                Lines,
                Text::new(""),
                TextFont {
                    font_size: bevy::text::FontSize::Px(p.text_size),
                    ..default()
                },
                TextColor(DIM_INK),
                TextLayout::justify(Justify::Center),
            ));
        });
}

fn update_ui(
    flow: Res<RunFlow>,
    mut overlay: Single<&mut BackgroundColor, With<Overlay>>,
    mut title: Single<&mut Text, (With<Title>, Without<Lines>)>,
    mut lines: Single<&mut Text, (With<Lines>, Without<Title>)>,
) {
    let flow = &flow.0;
    let t = &flow.params.texts;
    overlay.0 = Color::BLACK.with_alpha(flow.fade());
    let (head, body) = match flow.screen {
        Screen::Start => (
            t.title.clone(),
            format!(
                "{}\n\n{} {}\n{}\n\n{}",
                t.start, t.seed, flow.seed, t.seed_help, t.rooms
            ),
        ),
        Screen::Loading => (
            String::new(),
            format!(
                "{} {} / {}",
                t.loading,
                flow.stage + 1,
                flow.params.stages.len()
            ),
        ),
        Screen::RunOver => {
            let s = flow.stats;
            let total = s.time as u32;
            (
                if s.won { &t.cleared } else { &t.died }.clone(),
                format!(
                    "{} {}\n{} {}:{:02}\n{} {}\n{} {}\n\n{}",
                    t.seed,
                    flow.seed,
                    t.time,
                    total / 60,
                    total % 60,
                    t.stages,
                    s.cleared,
                    t.best_combo,
                    s.best_combo,
                    t.again
                ),
            )
        }
        Screen::Playing | Screen::Transition => (String::new(), String::new()),
    };
    if title.0 != head {
        title.0 = head;
    }
    if lines.0 != body {
        lines.0 = body;
    }
}

/// True when the game is in a debug room rather than a run.
pub fn in_room(flow: &Flow) -> bool {
    matches!(flow.mode, Mode::Room(_))
}
