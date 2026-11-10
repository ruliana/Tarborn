//! The Bevy side of the run loop in [`crate::flow`]: reads the menu keys,
//! pad and pointer, builds stages off the main thread, and draws the start,
//! loading, pause, transition and run-over screens over the world.

use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use tarborn_sim::Sim;

use crate::flow::{BUILT_IN, Flow, MAX_SEED, Mode, Order, RunParams, Screen, Signal};
use crate::rooms::{self, Exit};
use crate::{Game, Room};

const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/run.ron");
/// Pale type on black, as in the Pitch look.
const INK: Color = Color::srgb(0.92, 0.9, 0.86);
const DIM_INK: Color = Color::srgb(0.6, 0.58, 0.55);
const BRIGHT_INK: Color = Color::srgb(1.0, 0.82, 0.4);
/// The most menu lines any screen has.
const MENU_LINES: usize = 10;
/// How far the stick must lean to move a menu cursor.
const STICK_STEP: f32 = 0.6;

#[derive(Resource)]
pub struct RunFlow(pub Flow);

/// Signals for the flow, gathered from input and the simulation.
#[derive(Resource, Default)]
pub struct Signals(pub Vec<Signal>);

#[derive(Resource, Default)]
struct Loading(Option<Task<Built>>);

/// A stage or chunk built off the main thread, or why it couldn't be.
type Built = Result<(Sim, Option<Exit>), String>;

#[derive(Component)]
struct Overlay;

/// The overlay's texts other than the menu lines.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Label {
    Title,
    /// Loading progress, run stats.
    Lines,
    /// What the keys do.
    Help,
}

/// Menu line `i` of the current screen's menu.
#[derive(Component)]
struct MenuLine(usize);

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
    mut stick: Local<IVec2>,
) {
    let flow = &flow.0;
    let pads = |button: GamepadButton| gamepads.iter().any(|pad| pad.just_pressed(button));
    let back = keys.just_pressed(KeyCode::Escape)
        || pads(GamepadButton::Start)
        || pads(GamepadButton::Select);
    if back {
        signals.0.push(Signal::Back);
    }
    if flow.menu().is_empty() {
        return;
    }
    // The stick moves the cursor once each time it leans past the threshold.
    let lean = gamepads
        .iter()
        .map(|pad| pad.left_stick())
        .fold(
            Vec2::ZERO,
            |a, b| if b.length() > a.length() { b } else { a },
        );
    let now = IVec2::new(
        (lean.x > STICK_STEP) as i32 - (lean.x < -STICK_STEP) as i32,
        (lean.y > STICK_STEP) as i32 - (lean.y < -STICK_STEP) as i32,
    );
    let flick = |axis: i32, before: i32| if axis != before { axis } else { 0 };
    let (dx, dy) = (flick(now.x, stick.x), flick(now.y, stick.y));
    *stick = now;
    let moves = [
        (
            Signal::Up,
            [KeyCode::ArrowUp, KeyCode::KeyW],
            GamepadButton::DPadUp,
            dy > 0,
        ),
        (
            Signal::Down,
            [KeyCode::ArrowDown, KeyCode::KeyS],
            GamepadButton::DPadDown,
            dy < 0,
        ),
        (
            Signal::Left,
            [KeyCode::ArrowLeft, KeyCode::KeyA],
            GamepadButton::DPadLeft,
            dx < 0,
        ),
        (
            Signal::Right,
            [KeyCode::ArrowRight, KeyCode::KeyD],
            GamepadButton::DPadRight,
            dx > 0,
        ),
    ];
    for (signal, keys_for, button, flicked) in moves {
        if keys.any_just_pressed(keys_for) || pads(button) || flicked {
            signals.0.push(signal);
        }
    }
    if keys.any_just_pressed([
        KeyCode::Space,
        KeyCode::KeyJ,
        KeyCode::Enter,
        KeyCode::NumpadEnter,
    ]) || pads(GamepadButton::South)
    {
        signals.0.push(Signal::Confirm);
    }
    // The seed can also be typed.
    if flow.screen == Screen::Start {
        let mut seed = flow.seed;
        let digits = [
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
        ];
        for (i, key) in digits.into_iter().enumerate() {
            if keys.just_pressed(key) && seed <= MAX_SEED / 10 {
                seed = seed * 10 + i as u64;
            }
        }
        if keys.just_pressed(KeyCode::Backspace) {
            seed /= 10;
        }
        if seed != flow.seed {
            signals.0.push(Signal::Seed(seed));
        }
    }
}

fn run_flow(
    time: Res<Time>,
    mut flow: ResMut<RunFlow>,
    mut signals: ResMut<Signals>,
    mut game: ResMut<Game>,
    mut loading: ResMut<Loading>,
    mut ratings: Option<ResMut<crate::music::Ratings>>,
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
                    let (mut sim, exit) =
                        rooms::load_stage(&id, params.exit_depth, params.seal_thickness)?;
                    if let Some(health) = health {
                        sim.health = health.clamp(1, sim.max_health);
                    }
                    Ok((sim, Some(exit)))
                });
                loading.0 = Some(task);
            }
            Order::LoadChunk(path) => {
                let params = flow.0.params.clone();
                info!("loading chunk {path}");
                let task = AsyncComputeTaskPool::get().spawn(async move {
                    rooms::load_chunk(&path, params.exit_depth, params.seal_thickness)
                });
                loading.0 = Some(task);
            }
            Order::ScanChunks => flow.0.chunks = rooms::chunk_files(),
            Order::OpenRoom(id) => *game = Game::new(Room::from_id(&id)),
            Order::RandomSeed => signals.0.push(Signal::Seed(random_seed())),
            // The telemetry hook: every screen change is logged.
            Order::Entered(screen) => info!("run: {screen:?}"),
            Order::RateMusic { stage, rating } => {
                info!("run: music of stage {} rated {rating:+}", stage + 1);
                if let Some(ratings) = &mut ratings {
                    ratings.0.push((stage, rating));
                }
            }
        }
    }
}

fn poll_loading(
    mut loading: ResMut<Loading>,
    mut game: ResMut<Game>,
    mut signals: ResMut<Signals>,
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
            error!("can't load: {e}");
            signals.0.push(Signal::Failed(e));
        }
    }
}

fn spawn_ui(mut commands: Commands, flow: Res<RunFlow>) {
    let p = &flow.0.params;
    let font = |size: f32| TextFont {
        font_size: bevy::text::FontSize::Px(size),
        ..default()
    };
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
                Label::Title,
                Text::new(""),
                font(p.title_size),
                TextColor(INK),
            ));
            root.spawn((
                Label::Lines,
                Text::new(""),
                font(p.text_size),
                TextColor(DIM_INK),
                TextLayout::justify(Justify::Center),
            ));
            root.spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(p.text_size * 0.4),
                ..default()
            })
            .with_children(|menu| {
                for i in 0..MENU_LINES {
                    menu.spawn((
                        MenuLine(i),
                        Text::new(""),
                        font(p.text_size * 1.25),
                        TextColor(DIM_INK),
                        Node {
                            padding: UiRect::axes(Val::Px(p.text_size), Val::Px(2.0)),
                            ..default()
                        },
                    ))
                    .observe(
                        move |_: On<PointerMove>,
                              flow: Res<RunFlow>,
                              mut signals: ResMut<Signals>| {
                            let first = flow.0.menu_first(MENU_LINES);
                            signals.0.push(Signal::Hover(first + i));
                        },
                    )
                    .observe(
                        move |_: On<PointerClick>,
                              flow: Res<RunFlow>,
                              mut signals: ResMut<Signals>| {
                            let first = flow.0.menu_first(MENU_LINES);
                            signals.0.push(Signal::Pick(first + i));
                        },
                    );
                }
            });
            root.spawn((
                Label::Help,
                Text::new(""),
                font(p.text_size * 0.75),
                TextColor(DIM_INK),
                TextLayout::justify(Justify::Center),
            ));
        });
}

fn update_ui(
    flow: Res<RunFlow>,
    mut overlay: Single<&mut BackgroundColor, With<Overlay>>,
    mut labels: Query<(&Label, &mut Text), Without<MenuLine>>,
    mut menu: Query<(&MenuLine, &mut Text, &mut TextColor, &mut Visibility), Without<Label>>,
) {
    let flow = &flow.0;
    let t = &flow.params.texts;
    overlay.0 = Color::BLACK.with_alpha(flow.fade());
    let (head, body) = match flow.screen {
        Screen::Start => (t.title.clone(), flow.error.clone().unwrap_or_default()),
        Screen::Chunks => (t.chunks.clone(), flow.error.clone().unwrap_or_default()),
        Screen::Paused => (t.paused.clone(), String::new()),
        Screen::Loading => (
            String::new(),
            match flow.mode {
                Mode::Chunk(i) => format!(
                    "{} {}",
                    t.loading,
                    flow.chunks.get(i).map_or("", String::as_str)
                ),
                _ => format!(
                    "{} {} / {}",
                    t.loading,
                    flow.stage + 1,
                    flow.params.stages.len()
                ),
            },
        ),
        Screen::RunOver => {
            let s = flow.stats;
            let total = s.time as u32;
            (
                if s.won { &t.cleared } else { &t.died }.clone(),
                format!(
                    "{} {}\n{} {}:{:02}\n{} {}\n{} {}",
                    t.seed,
                    flow.seed,
                    t.time,
                    total / 60,
                    total % 60,
                    t.stages,
                    s.cleared,
                    t.best_combo,
                    s.best_combo,
                ),
            )
        }
        Screen::Playing | Screen::Transition => (String::new(), String::new()),
    };
    let items = flow.menu();
    let tip = match items.get(flow.cursor) {
        Some(crate::flow::Item::Seed) => format!("{}\n{}", t.seed_help, t.menu_help),
        Some(_) => t.menu_help.clone(),
        None => String::new(),
    };
    for (label, mut text) in &mut labels {
        let new = match label {
            Label::Title => head.clone(),
            Label::Lines => body.clone(),
            Label::Help => tip.clone(),
        };
        set(&mut text, new);
    }
    // A long menu shows the lines around the cursor, with marks for the rest.
    let first = flow.menu_first(MENU_LINES);
    let more_above = first > 0;
    let more_below = first + MENU_LINES < items.len();
    for (line, mut text, mut color, mut shown) in &mut menu {
        let index = first + line.0;
        let Some(&item) = items.get(index) else {
            shown.set_if_neq(Visibility::Hidden);
            continue;
        };
        shown.set_if_neq(Visibility::Inherited);
        let chosen = index == flow.cursor;
        let edge = (line.0 == 0 && more_above) || (line.0 == MENU_LINES - 1 && more_below);
        let label = flow.label(item);
        set(
            &mut text,
            match (chosen, edge) {
                (true, _) => format!("> {label} <"),
                (false, true) => "...".to_string(),
                (false, false) => label,
            },
        );
        color.set_if_neq(TextColor(if chosen { BRIGHT_INK } else { DIM_INK }));
    }
}

/// Changes a text only when it differs, so Bevy doesn't re-lay it out.
fn set(text: &mut Mut<Text>, new: String) {
    if text.0 != new {
        text.0 = new;
    }
}

/// True when the game is in a debug room rather than a run.
pub fn in_room(flow: &Flow) -> bool {
    matches!(flow.mode, Mode::Room(_))
}
