//! The run loop around the stages: start screen, loading, playing a stage,
//! the transition to the next one, and the run-over screen. Pure logic with
//! no Bevy in it, so the whole loop is tested headlessly; the game turns
//! its orders into loading, drawing and sound.

use serde::Deserialize;

/// Built-in run parameters, used when `assets/run.ron` can't be read.
pub const BUILT_IN: &str = include_str!("../assets/run.ron");

/// Tunables for the run loop, from `assets/run.ron`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct RunParams {
    /// The stages of a run, in order: chunk files in `assets/chunks/stages/`.
    /// Placeholders until stages are assembled from the generated library.
    pub stages: Vec<String>,
    /// How far inside the exit socket the blob must get, in metres.
    pub exit_depth: f32,
    /// Carry the blob's health from one stage to the next.
    pub keep_health: bool,
    /// Seconds to fade out after reaching a stage's exit, and to fade in.
    pub fade_out: f32,
    pub fade_in: f32,
    /// The loading screen shows for at least this long.
    pub loading_min: f32,
    /// Seconds between dying and the run-over screen.
    pub death_delay: f32,
    /// The run-over screen ignores Jump for this long, so a held button
    /// doesn't skip it.
    pub run_over_min: f32,
    /// How dark the start and run-over screens make the world behind them.
    pub dim: f32,
    pub title_size: f32,
    pub text_size: f32,
    pub texts: Texts,
}

impl Default for RunParams {
    fn default() -> Self {
        ron::from_str(BUILT_IN).expect("built-in assets/run.ron is valid")
    }
}

/// Every word the run screens show.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Texts {
    pub title: String,
    pub start: String,
    pub seed: String,
    pub seed_help: String,
    pub rooms: String,
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

/// Something that happened, fed into [`Flow::update`].
#[derive(Clone, Debug, PartialEq)]
pub enum Signal {
    /// Jump went down.
    Jump,
    /// Escape: leave a debug room or a run, back to the start screen.
    Back,
    /// Pick a debug room from the start screen.
    Room(String),
    /// Change the seed on the start screen.
    Seed(u64),
    /// The stage asked for has been built.
    Loaded,
    /// The blob reached the stage's exit, with this much health.
    Exit { health: u32 },
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
    /// Open a debug room.
    OpenRoom(String),
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
    /// Health carried into the next stage.
    carried: Option<u32>,
    loaded: bool,
    dead: bool,
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
            carried: None,
            loaded: false,
            dead: false,
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
            Screen::Playing => 0.0,
        }
    }

    /// True while the simulation should advance.
    pub fn simulating(&self) -> bool {
        self.screen == Screen::Playing
    }

    fn enter(&mut self, screen: Screen, orders: &mut Vec<Order>) {
        self.screen = screen;
        self.timer = 0.0;
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
        match (self.screen, signal) {
            (Screen::Start, Signal::Jump) => {
                self.mode = Mode::Run;
                self.stage = 0;
                self.stats = RunStats::default();
                self.carried = None;
                self.load(orders);
            }
            (Screen::Start, Signal::Seed(seed)) => self.seed = *seed,
            (Screen::Start, Signal::Room(id)) => {
                self.mode = Mode::Room(id.clone());
                self.dead = false;
                self.enter(Screen::Playing, orders);
                orders.push(Order::OpenRoom(id.clone()));
            }
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
            (Screen::RunOver, Signal::Jump) if self.timer >= self.params.run_over_min => {
                self.enter(Screen::Start, orders);
            }
            (Screen::Playing | Screen::RunOver | Screen::Loading, Signal::Back) => {
                self.mode = Mode::Run;
                self.enter(Screen::Start, orders);
            }
            _ => {}
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
        let orders = f.update(TICK, &[Signal::Jump]);
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
        f.update(TICK, &[Signal::Jump]);
        assert_eq!(f.screen, Screen::RunOver);
        wait(&mut f, p.run_over_min);
        f.update(TICK, &[Signal::Jump]);
        assert_eq!(f.screen, Screen::Start);
    }

    #[test]
    fn dying_ends_the_run_after_a_moment() {
        let mut f = flow();
        let p = f.params.clone();
        f.update(TICK, &[Signal::Jump]);
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
        f.update(TICK, &[Signal::Jump]);
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
        let orders = f.update(TICK, &[Signal::Jump]);
        assert!(
            orders
                .iter()
                .any(|o| matches!(o, Order::Load { seed: 7, .. }))
        );
        f.update(TICK, &[Signal::Back]);
        assert_eq!(f.screen, Screen::Start);
        let orders = f.update(TICK, &[Signal::Jump]);
        assert_eq!(loads(&orders), [(0, None)]);
        assert_eq!(f.stats, RunStats::default());
    }

    #[test]
    fn debug_rooms_open_from_the_start_screen_and_escape_back() {
        let mut f = flow();
        let p = f.params.clone();
        let orders = f.update(TICK, &[Signal::Room("arena".into())]);
        assert_eq!(f.screen, Screen::Playing);
        assert_eq!(f.mode, Mode::Room("arena".into()));
        assert!(orders.contains(&Order::OpenRoom("arena".into())));
        assert_eq!(f.fade(), 0.0);
        // Rooms restart on death by themselves: no run over.
        f.update(TICK, &[Signal::Died]);
        wait(&mut f, p.death_delay + 0.1);
        assert_eq!(f.screen, Screen::Playing);
        f.update(TICK, &[Signal::Back]);
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
        collect(f.update(TICK, &[Signal::Jump]));
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
}
