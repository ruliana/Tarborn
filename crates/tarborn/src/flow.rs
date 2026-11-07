//! The run loop around the stages: start menu, loading, playing a stage,
//! pausing, the transition to the next one, and the run-over screen. Pure
//! logic with no Bevy in it, so the whole loop is tested headlessly; the
//! game turns its orders into loading, drawing and sound.
//!
//! Every screen that waits for the player is a menu: Up and Down move the
//! cursor, Confirm (Jump) picks the item under it.

use serde::Deserialize;

/// Built-in run parameters, used when `assets/run.ron` can't be read.
pub const BUILT_IN: &str = include_str!("../assets/run.ron");

/// The largest seed the start menu takes.
pub const MAX_SEED: u64 = 999_999_999_999;

/// Tunables for the run loop, from `assets/run.ron`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct RunParams {
    /// The stages of a run, in order: chunk files in `assets/chunks/stages/`.
    /// Placeholders until stages are assembled from the generated library.
    pub stages: Vec<String>,
    /// The debug rooms on the start menu: (room id, label).
    pub rooms: Vec<(String, String)>,
    /// How far inside the exit socket the blob must get, in metres.
    pub exit_depth: f32,
    /// How thick the walls that close a stage's other openings are, in metres.
    pub seal_thickness: f32,
    /// Carry the blob's health from one stage to the next.
    pub keep_health: bool,
    /// Seconds to fade out after reaching a stage's exit, and to fade in.
    pub fade_out: f32,
    pub fade_in: f32,
    /// The loading screen shows for at least this long.
    pub loading_min: f32,
    /// Seconds between dying and the run-over screen.
    pub death_delay: f32,
    /// The run-over menu ignores Confirm for this long, so a held button
    /// doesn't skip it.
    pub run_over_min: f32,
    /// How dark the start and run-over screens make the world behind them.
    pub dim: f32,
    /// How dark the pause menu makes the world behind it.
    pub pause_dim: f32,
    pub title_size: f32,
    pub text_size: f32,
    /// The exit marker drawn in each stage's exit opening.
    pub exit_marker: ExitMarker,
    pub texts: Texts,
}

impl Default for RunParams {
    fn default() -> Self {
        ron::from_str(BUILT_IN).expect("built-in assets/run.ron is valid")
    }
}

/// How a stage's exit is shown: a frame around the exit region and arrows
/// sliding out through it.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct ExitMarker {
    /// Linear RGB.
    pub color: (f32, f32, f32),
    /// Arrows across the opening.
    pub arrows: u32,
    /// Arrow size, as a share of the opening's width.
    pub arrow_size: f32,
    /// Arrows travel this many metres outward, in a loop, per second.
    pub speed: f32,
    /// How far the arrows travel before looping, in metres.
    pub travel: f32,
    /// Line thickness, in metres, drawn as this many side-by-side lines.
    pub stroke: f32,
    pub strokes: u32,
}

/// Every word the run screens show.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Texts {
    pub title: String,
    pub start: String,
    pub seed: String,
    pub seed_help: String,
    pub menu_help: String,
    pub paused: String,
    pub resume: String,
    pub restart_run: String,
    pub restart_room: String,
    pub quit: String,
    pub loading: String,
    pub cleared: String,
    pub died: String,
    pub stages: String,
    pub time: String,
    pub best_combo: String,
    pub again: String,
}

/// Which screen is up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Start,
    Loading,
    Playing,
    /// The pause menu over a frozen stage or room.
    Paused,
    /// Fading out after the exit, before loading the next stage.
    Transition,
    RunOver,
}

/// What is being played.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    /// A run through the stages.
    Run,
    /// A debug room (training, arena, dressing), outside any run.
    Room(String),
}

/// One line of a menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    /// Start a run with the shown seed.
    Start,
    /// The seed: Left and Right step it, Confirm rolls a random one.
    Seed,
    /// Open debug room `i` of [`RunParams::rooms`].
    Room(usize),
    Resume,
    /// Start the run again from its first stage, or reopen the room.
    Restart,
    /// A new run with the same seed, from the run-over screen.
    Again,
    /// Back to the start menu.
    Quit,
}

/// Something that happened, fed into [`Flow::update`].
#[derive(Clone, Debug, PartialEq)]
pub enum Signal {
    /// Jump or Enter went down: pick the menu item under the cursor.
    Confirm,
    /// Escape or the pad's Start: pause, resume, or go back.
    Back,
    /// Menu cursor moves.
    Up,
    Down,
    Left,
    Right,
    /// The pointer moved over menu item `i`. (A pointer that sits still
    /// while a menu opens under it doesn't move the cursor.)
    Hover(usize),
    /// Menu item `i` was clicked.
    Pick(usize),
    /// Set the seed (typed on the start menu).
    Seed(u64),
    /// The stage asked for has been built.
    Loaded,
    /// The blob reached the stage's exit, with this much health.
    Exit {
        health: u32,
    },
    /// The blob died.
    Died,
    /// The longest combo so far in this stage.
    Combo(u32),
}

/// What the game must do in answer.
#[derive(Clone, Debug, PartialEq)]
pub enum Order {
    /// Build stage `stage` of the run from `seed`, starting with `health`
    /// if it carries over.
    Load {
        stage: usize,
        seed: u64,
        health: Option<u32>,
    },
    /// Open a debug room, fresh.
    OpenRoom(String),
    /// Roll a random seed and send it back as [`Signal::Seed`].
    RandomSeed,
    /// Something to log for telemetry: every screen change.
    Entered(Screen),
}

/// Totals for the run-over screen.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RunStats {
    pub cleared: usize,
    /// Seconds spent playing stages.
    pub time: f32,
    pub best_combo: u32,
    /// Finished every stage, rather than dying.
    pub won: bool,
}

#[derive(Clone, Debug)]
pub struct Flow {
    pub params: RunParams,
    pub screen: Screen,
    pub mode: Mode,
    pub seed: u64,
    /// The stage being played or loaded, from 0.
    pub stage: usize,
    pub stats: RunStats,
    /// Seconds on the current screen.
    pub timer: f32,
    /// The menu item under the cursor.
    pub cursor: usize,
    /// Health carried into the next stage.
    carried: Option<u32>,
    loaded: bool,
    dead: bool,
    /// The play timer when the game was paused, restored on resume.
    paused_at: f32,
}

impl Flow {
    pub fn new(params: RunParams, seed: u64) -> Self {
        Self {
            params,
            screen: Screen::Start,
            mode: Mode::Run,
            seed,
            stage: 0,
            stats: RunStats::default(),
            timer: 0.0,
            cursor: 0,
            carried: None,
            loaded: false,
            dead: false,
            paused_at: 0.0,
        }
    }

    /// 0 shows the game, 1 is black.
    pub fn fade(&self) -> f32 {
        let p = &self.params;
        match self.screen {
            Screen::Transition => (self.timer / p.fade_out.max(1e-3)).min(1.0),
            Screen::Playing if self.mode == Mode::Run => {
                1.0 - (self.timer / p.fade_in.max(1e-3)).min(1.0)
            }
            Screen::Loading => 1.0,
            Screen::Start | Screen::RunOver => p.dim,
            Screen::Paused => p.pause_dim,
            Screen::Playing => 0.0,
        }
    }

    /// True while the simulation should advance.
    pub fn simulating(&self) -> bool {
        self.screen == Screen::Playing
    }

    /// The menu on the current screen, top to bottom; empty when there is
    /// none.
    pub fn menu(&self) -> Vec<Item> {
        match self.screen {
            Screen::Start => [Item::Start, Item::Seed]
                .into_iter()
                .chain((0..self.params.rooms.len()).map(Item::Room))
                .collect(),
            Screen::Paused => vec![Item::Resume, Item::Restart, Item::Quit],
            Screen::RunOver => vec![Item::Again, Item::Quit],
            Screen::Loading | Screen::Playing | Screen::Transition => Vec::new(),
        }
    }

    /// The words for a menu item.
    pub fn label(&self, item: Item) -> String {
        let t = &self.params.texts;
        match item {
            Item::Start => t.start.clone(),
            Item::Seed => format!("{}  < {} >", t.seed, self.seed),
            Item::Room(i) => self.params.rooms[i].1.clone(),
            Item::Resume => t.resume.clone(),
            Item::Restart if self.mode == Mode::Run => t.restart_run.clone(),
            Item::Restart => t.restart_room.clone(),
            Item::Again => t.again.clone(),
            Item::Quit => t.quit.clone(),
        }
    }

    fn enter(&mut self, screen: Screen, orders: &mut Vec<Order>) {
        self.screen = screen;
        self.timer = 0.0;
        self.cursor = 0;
        orders.push(Order::Entered(screen));
    }

    fn load(&mut self, orders: &mut Vec<Order>) {
        self.loaded = false;
        self.dead = false;
        self.enter(Screen::Loading, orders);
        orders.push(Order::Load {
            stage: self.stage,
            seed: self.seed,
            health: self.carried,
        });
    }

    fn start_run(&mut self, orders: &mut Vec<Order>) {
        self.mode = Mode::Run;
        self.stage = 0;
        self.stats = RunStats::default();
        self.carried = None;
        self.load(orders);
    }

    fn open_room(&mut self, id: String, orders: &mut Vec<Order>) {
        self.mode = Mode::Room(id.clone());
        self.dead = false;
        self.enter(Screen::Playing, orders);
        orders.push(Order::OpenRoom(id));
    }

    fn resume(&mut self, orders: &mut Vec<Order>) {
        self.enter(Screen::Playing, orders);
        self.timer = self.paused_at;
    }

    fn quit(&mut self, orders: &mut Vec<Order>) {
        self.mode = Mode::Run;
        self.enter(Screen::Start, orders);
    }

    /// Advances `dt` seconds with the signals that arrived since the last call.
    pub fn update(&mut self, dt: f32, signals: &[Signal]) -> Vec<Order> {
        let mut orders = Vec::new();
        self.timer += dt;
        if self.screen == Screen::Playing && self.mode == Mode::Run && !self.dead {
            self.stats.time += dt;
        }
        for signal in signals {
            self.signal(signal, &mut orders);
        }
        let p = self.params.clone();
        match self.screen {
            Screen::Loading if self.loaded && self.timer >= p.loading_min => {
                self.enter(Screen::Playing, &mut orders);
            }
            Screen::Playing if self.dead && self.timer >= p.death_delay => {
                self.stats.won = false;
                self.enter(Screen::RunOver, &mut orders);
            }
            Screen::Transition if self.timer >= p.fade_out => {
                self.stats.cleared += 1;
                if self.stage + 1 >= p.stages.len() {
                    self.stats.won = true;
                    self.enter(Screen::RunOver, &mut orders);
                } else {
                    self.stage += 1;
                    self.load(&mut orders);
                }
            }
            _ => {}
        }
        orders
    }

    fn signal(&mut self, signal: &Signal, orders: &mut Vec<Order>) {
        let menu = self.menu();
        match (self.screen, signal) {
            (_, Signal::Up) if !menu.is_empty() => {
                self.cursor = (self.cursor + menu.len() - 1) % menu.len();
            }
            (_, Signal::Down) if !menu.is_empty() => {
                self.cursor = (self.cursor + 1) % menu.len();
            }
            (_, Signal::Hover(i)) if *i < menu.len() => self.cursor = *i,
            (_, Signal::Pick(i)) if *i < menu.len() => {
                self.cursor = *i;
                self.choose(menu[*i], orders);
            }
            (_, Signal::Confirm) if !menu.is_empty() => {
                self.choose(menu[self.cursor.min(menu.len() - 1)], orders);
            }
            (Screen::Start, Signal::Left) if menu.get(self.cursor) == Some(&Item::Seed) => {
                self.seed = self.seed.saturating_sub(1);
            }
            (Screen::Start, Signal::Right) if menu.get(self.cursor) == Some(&Item::Seed) => {
                self.seed = (self.seed + 1).min(MAX_SEED);
            }
            (Screen::Start, Signal::Seed(seed)) => self.seed = (*seed).min(MAX_SEED),
            (Screen::Loading, Signal::Loaded) => self.loaded = true,
            (Screen::Playing, Signal::Combo(c)) => {
                self.stats.best_combo = self.stats.best_combo.max(*c);
            }
            (Screen::Playing, Signal::Exit { health }) if self.mode == Mode::Run && !self.dead => {
                self.carried = self.params.keep_health.then_some(*health);
                self.enter(Screen::Transition, orders);
            }
            (Screen::Playing, Signal::Died) if self.mode == Mode::Run && !self.dead => {
                self.dead = true;
                self.timer = 0.0;
            }
            (Screen::Playing, Signal::Back) => {
                self.paused_at = self.timer;
                self.enter(Screen::Paused, orders);
            }
            (Screen::Paused, Signal::Back) => self.resume(orders),
            (Screen::RunOver | Screen::Loading, Signal::Back) => self.quit(orders),
            _ => {}
        }
    }

    fn choose(&mut self, item: Item, orders: &mut Vec<Order>) {
        if self.screen == Screen::RunOver && self.timer < self.params.run_over_min {
            return;
        }
        match item {
            Item::Start | Item::Again => self.start_run(orders),
            Item::Seed => orders.push(Order::RandomSeed),
            Item::Room(i) => {
                let id = self.params.rooms[i].0.clone();
                self.open_room(id, orders);
            }
            Item::Resume => self.resume(orders),
            Item::Restart => match self.mode.clone() {
                Mode::Run => self.start_run(orders),
                Mode::Room(id) => self.open_room(id, orders),
            },
            Item::Quit => self.quit(orders),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICK: f32 = 1.0 / 120.0;

    fn flow() -> Flow {
        let params = RunParams {
            stages: vec!["a".into(), "b".into(), "c".into()],
            ..RunParams::default()
        };
        Flow::new(params, 42)
    }

    /// Runs `seconds` of ticks with no signals, collecting the orders.
    fn wait(flow: &mut Flow, seconds: f32) -> Vec<Order> {
        let mut orders = Vec::new();
        let mut t = 0.0;
        while t < seconds {
            orders.extend(flow.update(TICK, &[]));
            t += TICK;
        }
        orders
    }

    fn loads(orders: &[Order]) -> Vec<(usize, Option<u32>)> {
        orders
            .iter()
            .filter_map(|o| match o {
                Order::Load { stage, health, .. } => Some((*stage, *health)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_whole_run_from_start_through_every_stage_to_run_over() {
        let mut f = flow();
        let p = f.params.clone();
        assert_eq!(f.screen, Screen::Start);
        let orders = f.update(TICK, &[Signal::Confirm]);
        assert_eq!(f.screen, Screen::Loading);
        assert_eq!(loads(&orders), [(0, None)]);
        assert_eq!(f.fade(), 1.0);

        for stage in 0..3 {
            // Not playing until both loaded and shown long enough.
            wait(&mut f, p.loading_min);
            assert_eq!(f.screen, Screen::Loading, "stage {stage}");
            f.update(TICK, &[Signal::Loaded]);
            wait(&mut f, p.loading_min);
            assert_eq!(f.screen, Screen::Playing, "stage {stage}");
            assert!(f.simulating());
            assert!(f.fade() > 0.0, "fades in");
            wait(&mut f, p.fade_in + 0.1);
            assert_eq!(f.fade(), 0.0);
            f.update(TICK, &[Signal::Combo(stage as u32 + 4)]);

            f.update(
                TICK,
                &[Signal::Exit {
                    health: 3 - stage as u32,
                }],
            );
            assert_eq!(f.screen, Screen::Transition);
            assert!(!f.simulating(), "input frozen during the fade");
            let orders = wait(&mut f, p.fade_out + 0.05);
            if stage < 2 {
                assert_eq!(f.screen, Screen::Loading);
                assert_eq!(loads(&orders), [(stage + 1, Some(3 - stage as u32))]);
            }
        }
        assert_eq!(f.screen, Screen::RunOver);
        assert!(f.stats.won);
        assert_eq!(f.stats.cleared, 3);
        assert_eq!(f.stats.best_combo, 6);
        assert!(f.stats.time > 0.0);

        // A held Jump doesn't skip the screen; a press after a moment does.
        f.update(TICK, &[Signal::Confirm]);
        assert_eq!(f.screen, Screen::RunOver);
        wait(&mut f, p.run_over_min);
        f.update(TICK, &[Signal::Down, Signal::Confirm]);
        assert_eq!(f.screen, Screen::Start);
    }

    #[test]
    fn dying_ends_the_run_after_a_moment() {
        let mut f = flow();
        let p = f.params.clone();
        f.update(TICK, &[Signal::Confirm]);
        f.update(TICK, &[Signal::Loaded]);
        wait(&mut f, p.loading_min + 0.1);
        assert_eq!(f.screen, Screen::Playing);
        f.update(TICK, &[Signal::Died]);
        assert_eq!(f.screen, Screen::Playing);
        // Reaching the exit while dead doesn't count.
        f.update(TICK, &[Signal::Exit { health: 0 }]);
        assert_eq!(f.screen, Screen::Playing);
        wait(&mut f, p.death_delay + 0.05);
        assert_eq!(f.screen, Screen::RunOver);
        assert!(!f.stats.won);
        assert_eq!(f.stats.cleared, 0);
    }

    #[test]
    fn health_carries_only_when_asked() {
        let mut f = flow();
        let p = f.params.clone();
        f.params.keep_health = false;
        f.update(TICK, &[Signal::Confirm]);
        f.update(TICK, &[Signal::Loaded]);
        wait(&mut f, p.loading_min + 0.1);
        f.update(TICK, &[Signal::Exit { health: 1 }]);
        let orders = wait(&mut f, p.fade_out + 0.05);
        assert_eq!(loads(&orders), [(1, None)]);
    }

    #[test]
    fn a_new_run_starts_fresh_with_the_chosen_seed() {
        let mut f = flow();
        f.update(TICK, &[Signal::Seed(7)]);
        let orders = f.update(TICK, &[Signal::Confirm]);
        assert!(
            orders
                .iter()
                .any(|o| matches!(o, Order::Load { seed: 7, .. }))
        );
        f.update(TICK, &[Signal::Back]);
        assert_eq!(f.screen, Screen::Start, "Escape while loading");
        let orders = f.update(TICK, &[Signal::Confirm]);
        assert_eq!(loads(&orders), [(0, None)]);
        assert_eq!(f.stats, RunStats::default());
    }

    #[test]
    fn debug_rooms_open_from_the_start_screen_and_escape_back() {
        let mut f = flow();
        let p = f.params.clone();
        // Down past Start and Seed, to the second room.
        f.update(TICK, &[Signal::Down, Signal::Down, Signal::Down]);
        assert_eq!(f.menu()[f.cursor], Item::Room(1));
        assert_eq!(p.rooms[1].0, "arena");
        let orders = f.update(TICK, &[Signal::Confirm]);
        assert_eq!(f.screen, Screen::Playing);
        assert_eq!(f.mode, Mode::Room("arena".into()));
        assert!(orders.contains(&Order::OpenRoom("arena".into())));
        assert_eq!(f.fade(), 0.0);
        // Rooms restart on death by themselves: no run over.
        f.update(TICK, &[Signal::Died]);
        wait(&mut f, p.death_delay + 0.1);
        assert_eq!(f.screen, Screen::Playing);
        // Escape pauses; Restart reopens the room; Quit goes to the start.
        f.update(TICK, &[Signal::Back]);
        assert_eq!(f.screen, Screen::Paused);
        let orders = f.update(TICK, &[Signal::Down, Signal::Confirm]);
        assert_eq!(f.screen, Screen::Playing);
        assert!(orders.contains(&Order::OpenRoom("arena".into())));
        f.update(TICK, &[Signal::Back, Signal::Up, Signal::Confirm]);
        assert_eq!(f.screen, Screen::Start);
        assert_eq!(f.mode, Mode::Run);
    }

    #[test]
    fn every_screen_change_is_reported() {
        let mut f = flow();
        let p = f.params.clone();
        let mut seen = Vec::new();
        let mut collect = |orders: Vec<Order>| {
            seen.extend(orders.into_iter().filter_map(|o| match o {
                Order::Entered(s) => Some(s),
                _ => None,
            }));
        };
        collect(f.update(TICK, &[Signal::Confirm]));
        collect(f.update(TICK, &[Signal::Loaded]));
        collect(wait(&mut f, p.loading_min + 0.1));
        collect(f.update(TICK, &[Signal::Exit { health: 3 }]));
        collect(wait(&mut f, p.fade_out + 0.05));
        assert_eq!(
            seen,
            [
                Screen::Loading,
                Screen::Playing,
                Screen::Transition,
                Screen::Loading
            ]
        );
    }

    fn playing() -> Flow {
        let mut f = flow();
        let p = f.params.clone();
        f.update(TICK, &[Signal::Confirm]);
        f.update(TICK, &[Signal::Loaded]);
        wait(&mut f, p.loading_min + 0.1);
        assert_eq!(f.screen, Screen::Playing);
        f
    }

    #[test]
    fn escape_pauses_and_resumes_where_it_left_off() {
        let mut f = playing();
        let p = f.params.clone();
        wait(&mut f, p.fade_in * 0.5);
        let (timer, fade) = (f.timer, f.fade());
        f.update(0.0, &[Signal::Back]);
        let time = f.stats.time;
        assert_eq!(f.screen, Screen::Paused);
        assert!(!f.simulating());
        assert_eq!(f.fade(), p.pause_dim);
        assert_eq!(f.menu(), [Item::Resume, Item::Restart, Item::Quit]);
        wait(&mut f, 2.0);
        assert_eq!(f.stats.time, time, "the clock stops while paused");
        f.update(0.0, &[Signal::Back]);
        assert_eq!(f.screen, Screen::Playing);
        assert_eq!((f.timer, f.fade()), (timer, fade), "the fade-in picks up");
        // Resume from the menu too.
        f.update(0.0, &[Signal::Back, Signal::Confirm]);
        assert_eq!(f.screen, Screen::Playing);
    }

    #[test]
    fn restart_starts_the_run_over_from_the_first_stage() {
        let mut f = playing();
        let p = f.params.clone();
        f.update(TICK, &[Signal::Combo(5), Signal::Exit { health: 1 }]);
        wait(&mut f, p.fade_out + 0.05);
        assert_eq!(f.stage, 1);
        f.update(TICK, &[Signal::Loaded]);
        wait(&mut f, p.loading_min + 0.1);
        f.update(TICK, &[Signal::Back, Signal::Down]);
        assert_eq!(f.menu()[f.cursor], Item::Restart);
        let orders = f.update(TICK, &[Signal::Confirm]);
        assert_eq!(f.screen, Screen::Loading);
        assert_eq!(loads(&orders), [(0, None)], "fresh health too");
        assert_eq!(f.stats, RunStats::default());
    }

    #[test]
    fn menus_wrap_and_answer_the_pointer() {
        let mut f = flow();
        let n = f.menu().len();
        assert_eq!(n, 2 + f.params.rooms.len());
        f.update(TICK, &[Signal::Up]);
        assert_eq!(f.cursor, n - 1);
        f.update(TICK, &[Signal::Down]);
        assert_eq!(f.cursor, 0);
        f.update(TICK, &[Signal::Hover(1)]);
        assert_eq!(f.menu()[f.cursor], Item::Seed);
        f.update(TICK, &[Signal::Hover(99)]);
        assert_eq!(f.cursor, 1, "off the menu is ignored");
        let orders = f.update(TICK, &[Signal::Pick(0)]);
        assert_eq!(loads(&orders), [(0, None)]);
    }

    #[test]
    fn the_seed_line_steps_and_rolls() {
        let mut f = flow();
        f.update(TICK, &[Signal::Right]);
        assert_eq!(f.seed, 42, "only on the seed line");
        f.update(TICK, &[Signal::Down, Signal::Right, Signal::Right]);
        assert_eq!(f.seed, 44);
        f.update(TICK, &[Signal::Left]);
        assert_eq!(f.seed, 43);
        assert!(f.label(Item::Seed).contains("43"));
        let orders = f.update(TICK, &[Signal::Confirm]);
        assert_eq!(orders, [Order::RandomSeed]);
        assert_eq!(f.screen, Screen::Start);
        f.update(TICK, &[Signal::Seed(u64::MAX)]);
        assert_eq!(f.seed, MAX_SEED);
    }

    #[test]
    fn run_over_offers_another_go_or_the_title() {
        let mut f = playing();
        let p = f.params.clone();
        f.update(TICK, &[Signal::Died]);
        wait(&mut f, p.death_delay + 0.05);
        assert_eq!(f.screen, Screen::RunOver);
        assert_eq!(f.menu(), [Item::Again, Item::Quit]);
        wait(&mut f, p.run_over_min);
        let orders = f.update(TICK, &[Signal::Confirm]);
        assert_eq!(loads(&orders), [(0, None)]);
    }
}
