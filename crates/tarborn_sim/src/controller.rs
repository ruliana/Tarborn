//! Turns player intent into blob actions, with the forgiveness that makes a
//! platformer feel responsive: jump buffering, coyote time and variable jump
//! height.
//!
//! Two verbs, two buttons:
//! - **Grip (hold) shrinks.** The blob gets small and sticky, makes no noise,
//!   dodges some attacks, and charges up the longer it stays shrunk.
//! - **Jump (press) expands**, the instant it is pressed:
//!   - at normal size on a surface: a jump (hold for height);
//!   - shrunk on a surface: a burst, a lunge at a nearby enemy or, with none
//!     near, a launch off the surface, stronger with more charge;
//!   - in the air: a lunge at a nearby enemy, or with down held, a slam.
//! - **Holding Jump in the air** keeps the body bouncy.
//!
//! A **lunge** is a short, gravity-free dash at an enemy, invulnerable while
//! it lasts. What the player holds when it arrives decides what happens (see
//! `combat`): nothing for a hit, Jump to bounce off, Grip to grab. One lunge
//! per air time, refilled by touching a surface or by hitting something.

use glam::Vec2;

use crate::blob::{Blob, TarModes};
use crate::level::Level;

/// What the player wants this tick, already sampled from the input devices.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Intent {
    /// -1 (left) to 1 (right).
    pub roll: f32,
    /// Stick direction, including up and down. Its length is at most 1.
    pub aim: Vec2,
    /// The jump button went down since the previous tick.
    pub jump_pressed: bool,
    /// The jump button is down right now.
    pub jump_held: bool,
    /// Asks for an aimed dash directly, whether or not an enemy is near.
    /// The two-button controls never set it; it is for tests and scripts.
    pub lunge_pressed: bool,
    /// `sticky` is Grip. `bouncy` is worked out from Jump by the simulation,
    /// but can be forced.
    pub modes: TarModes,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FeelParams {
    /// A jump pressed this long before landing still happens on landing.
    pub jump_buffer: f32,
    /// A jump pressed this long after leaving a surface still happens.
    pub coyote_time: f32,
    pub jump_speed: f32,
    /// Fraction of upward speed kept when jump is released early.
    pub jump_cut: f32,
    pub lunge_speed: f32,
    /// How long a lunge drives the blob, ignoring gravity.
    pub lunge_time: f32,
    /// Fraction of lunge speed kept when it ends, so lunges carry momentum.
    pub lunge_exit_keep: f32,
    /// Time from the start of one lunge until the next can start.
    pub lunge_cooldown: f32,
    /// A lunge pressed this long before it is allowed still happens.
    pub lunge_buffer: f32,
    /// Aim more downward than this (aim.y below -this) makes an air lunge a slam.
    pub slam_aim: f32,
    pub slam_speed: f32,
    /// Staying shrunk this long reaches full charge.
    pub coil_time: f32,
    /// Launch and lunge speed at full charge, as a multiple of normal.
    pub coil_max_power: f32,
    /// How small the blob gets at full charge. It drops most of the way there
    /// as soon as Grip goes down.
    pub coil_min_scale: f32,
    /// Jump lunges at enemies within this distance...
    pub lunge_range: f32,
    /// ...and, when the stick is pushed, within this angle (radians) of it.
    pub lunge_aim_angle: f32,
    /// Longest a lunge can last while it closes in on a far enemy.
    pub lunge_max_time: f32,
    /// Jump held at least this long when a lunge arrives counts as "held",
    /// so a quick tap still hits rather than bounces.
    pub hold_threshold: f32,
    /// A forced (scripted) dash snaps toward targets within this angle.
    pub aim_assist_angle: f32,
}

impl Default for FeelParams {
    fn default() -> Self {
        Self {
            jump_buffer: 0.12,
            coyote_time: 0.10,
            jump_speed: 12.0,
            jump_cut: 0.45,
            lunge_speed: 15.0,
            lunge_time: 0.16,
            lunge_exit_keep: 0.5,
            lunge_cooldown: 0.3,
            lunge_buffer: 0.12,
            slam_aim: 0.5,
            slam_speed: 24.0,
            coil_time: 0.6,
            coil_max_power: 2.0,
            coil_min_scale: 0.65,
            lunge_range: 5.0,
            lunge_aim_angle: 60f32.to_radians(),
            lunge_max_time: 0.4,
            hold_threshold: 0.1,
            aim_assist_angle: 30f32.to_radians(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Controller {
    pub params: FeelParams,
    buffer_left: f32,
    coyote_left: f32,
    last_support: Vec2,
    rising: bool,
    /// 1 for right, -1 for left.
    facing: f32,
    lunge_buffer_left: f32,
    lunge_left: f32,
    lunge_direction: Vec2,
    lunge_cooldown_left: f32,
    air_lunge_ready: bool,
    lunge_speed: f32,
    lunge_power: f32,
    slamming: bool,
    /// Seconds shrunk, while Grip is held.
    shrunk: Option<f32>,
    /// Seconds Jump has been held without a break.
    jump_hold: f32,
}

impl Default for Controller {
    fn default() -> Self {
        Self::new(FeelParams::default())
    }
}

/// Something notable that happened during a step, for effects and gameplay.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    Jumped {
        direction: Vec2,
    },
    Lunged {
        direction: Vec2,
    },
    SlamStarted,
    /// Grip went down: the blob shrank.
    Shrank,
    /// A shrunk blob expanded off a surface with no enemy near. `power` is
    /// 0 right after shrinking, 1 after a full charge.
    Launched {
        direction: Vec2,
        power: f32,
    },
    /// A slam hit the ground at `at` going `speed` metres per second.
    Slammed {
        at: Vec2,
        speed: f32,
    },
    /// A bouncy blob rebounded off a surface with this normal.
    Bounced {
        normal: Vec2,
    },
    /// The player hit enemy number `enemy`.
    Hit {
        enemy: usize,
        kind: HitKind,
        damage: f32,
        at: Vec2,
    },
    Killed {
        enemy: usize,
        at: Vec2,
    },
    /// A lunge with Grip held latched onto enemy number `enemy`.
    Grabbed {
        enemy: usize,
    },
    /// The held enemy was thrown (Jump) or dropped (Grip released).
    Thrown {
        enemy: usize,
        speed: f32,
    },
    Dropped {
        enemy: usize,
    },
    /// Something other than the player hurt enemy number `enemy`: a thrown
    /// body, spit, or another enemy's swing.
    Struck {
        enemy: usize,
        damage: f32,
        at: Vec2,
    },
    /// An enemy attack passed through the shrunk blob.
    Dodged {
        at: Vec2,
    },
    /// A skitter hopped out of the way of a lunge.
    Evaded {
        enemy: usize,
        at: Vec2,
    },
    /// A brute's ground slam went off.
    Quake {
        enemy: usize,
        at: Vec2,
    },
    /// A brute ate a grub.
    Eaten {
        enemy: usize,
        prey: usize,
    },
    /// An arena wave dropped in (counting from 0).
    WaveStarted {
        wave: usize,
    },
    /// The last arena wave is down.
    ArenaCleared,
    /// A guard's shield stopped the hit.
    Blocked {
        enemy: usize,
        at: Vec2,
    },
    PlayerHurt {
        health: u32,
    },
    PlayerDied,
    Healed {
        health: u32,
    },
}

/// How the player hit something. Each has its own damage and rebound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKind {
    Lunge,
    /// A slam landing directly on an enemy.
    Slam,
    /// Caught in a slam's shockwave without being touched.
    Shockwave,
    /// Landing on an enemy with Jump held, or reaching it in a lunge with
    /// Jump still held: bounce off it.
    Pogo,
    /// Rolling or bouncing into an enemy fast.
    Impact,
}

impl Controller {
    pub fn new(params: FeelParams) -> Self {
        Self {
            params,
            buffer_left: 0.0,
            coyote_left: 0.0,
            last_support: Vec2::Y,
            rising: false,
            facing: 1.0,
            lunge_buffer_left: 0.0,
            lunge_left: 0.0,
            lunge_direction: Vec2::X,
            lunge_cooldown_left: 0.0,
            air_lunge_ready: true,
            lunge_speed: 0.0,
            lunge_power: 0.0,
            slamming: false,
            shrunk: None,
            jump_hold: 0.0,
        }
    }

    pub fn is_lunging(&self) -> bool {
        self.lunge_left > 0.0
    }

    /// Grip is held: the blob is small and sticky.
    pub fn is_shrunk(&self) -> bool {
        self.shrunk.is_some()
    }

    /// 0 just after shrinking, 1 after a full charge.
    pub fn charge(&self) -> f32 {
        self.shrunk
            .map_or(0.0, |t| (t / self.params.coil_time).min(1.0))
    }

    /// Spends the charge (after a burst or a throw); the blob stays shrunk.
    pub fn spend_charge(&mut self) {
        if self.shrunk.is_some() {
            self.shrunk = Some(0.0);
        }
    }

    /// True when Jump has been held long enough to count as a hold, not a tap.
    pub fn jump_is_held(&self) -> bool {
        self.jump_hold >= self.params.hold_threshold
    }

    pub fn is_slamming(&self) -> bool {
        self.slamming
    }

    /// Lunges and slams can't be hurt.
    pub fn invulnerable(&self) -> bool {
        self.is_lunging() || self.slamming
    }

    /// 1 for right, -1 for left.
    pub fn facing(&self) -> f32 {
        self.facing
    }

    /// Extra damage share for the current lunge: 0 for a plain lunge, up to 1
    /// for one burst from a full charge.
    pub fn lunge_power(&self) -> f32 {
        self.lunge_power
    }

    /// Makes the next lunge available right away, even in the air. Hitting an
    /// enemy calls this, which is what lets lunges chain.
    pub fn refresh_lunge(&mut self) {
        self.air_lunge_ready = true;
        self.lunge_cooldown_left = 0.0;
    }

    /// Ends any lunge or slam right away, e.g. when it hits something.
    pub fn cancel_moves(&mut self, blob: &mut Blob) {
        if self.is_lunging() {
            self.end_lunge(blob, 1.0);
        }
        self.slamming = false;
    }

    /// Advances one tick. `targets` are the enemies a lunge can go for.
    pub fn step(
        &mut self,
        blob: &mut Blob,
        level: &Level,
        intent: Intent,
        targets: &[Vec2],
        dt: f32,
    ) -> Vec<Event> {
        let mut events = Vec::new();
        let touching = blob.touching_count() > 0;

        if intent.roll.abs() > 0.1 {
            self.facing = intent.roll.signum();
        }
        if let Some(normal) = blob.support_normal(intent.modes) {
            self.coyote_left = self.params.coyote_time;
            self.last_support = normal;
        }
        if touching && !self.is_lunging() {
            self.air_lunge_ready = true;
        }
        self.jump_hold = if intent.jump_held && !intent.jump_pressed {
            self.jump_hold + dt
        } else {
            0.0
        };

        // Grip shrinks.
        if intent.modes.sticky {
            if self.shrunk.is_none() {
                events.push(Event::Shrank);
            }
            self.shrunk = Some(self.shrunk.unwrap_or(0.0) + dt);
            let size = 0.4 + 0.6 * self.charge();
            blob.scale = 1.0 + (self.params.coil_min_scale - 1.0) * size;
        } else if self.shrunk.take().is_some() {
            blob.scale = 1.0;
        }

        // Jump expands.
        let on_surface = touching || self.coyote_left > 0.0;
        let free = !self.is_lunging() && !self.slamming;
        if intent.jump_pressed {
            if on_surface {
                self.buffer_left = self.params.jump_buffer;
            } else if free && intent.aim.y < -self.params.slam_aim {
                self.start_slam(blob, &mut events);
            } else if let Some(target) = self.pick_target(blob.center(), intent.aim, targets)
                && free
                && self.air_lunge_ready
                && self.lunge_cooldown_left <= 0.0
            {
                self.air_lunge_ready = false;
                self.lunge_at(blob, target, 0.0, &mut events);
            } else {
                // Maybe about to land: keep it as a buffered jump.
                self.buffer_left = self.params.jump_buffer;
            }
        }
        if intent.lunge_pressed {
            self.lunge_buffer_left = self.params.lunge_buffer;
        }
        self.forced_lunge(blob, intent, targets, touching, &mut events);

        // A jump (buffered or not) on a surface: a burst if shrunk, else a jump.
        if self.buffer_left > 0.0 && self.coyote_left > 0.0 && !self.slamming {
            if self.is_shrunk() {
                self.burst(blob, intent.aim, targets, &mut events);
            } else {
                self.jump(blob, &mut events);
            }
        }

        if self.rising {
            if blob.velocity().y <= 0.0 {
                self.rising = false;
            } else if !intent.jump_held {
                blob.cut_rise(self.params.jump_cut);
                self.rising = false;
            }
        }

        if self.is_lunging() {
            blob.gravity_scale = 0.0;
            blob.set_velocity(self.lunge_direction * self.lunge_speed);
        }

        let impact = blob.velocity();
        let bounce = blob.step(level, dt, intent.roll, intent.modes);

        if self.slamming && blob.touching_count() > 0 {
            self.slamming = false;
            events.push(Event::Slammed {
                at: blob.center(),
                speed: -impact.y,
            });
        }
        if let Some(normal) = bounce {
            events.push(Event::Bounced { normal });
        }

        if self.is_lunging() {
            self.lunge_left -= dt;
            if self.lunge_left <= 0.0 {
                let keep = self.params.lunge_exit_keep;
                self.end_lunge(blob, keep);
            }
        }

        self.buffer_left = (self.buffer_left - dt).max(0.0);
        self.coyote_left = (self.coyote_left - dt).max(0.0);
        self.lunge_buffer_left = (self.lunge_buffer_left - dt).max(0.0);
        self.lunge_cooldown_left = (self.lunge_cooldown_left - dt).max(0.0);
        events
    }

    fn jump(&mut self, blob: &mut Blob, events: &mut Vec<Event>) {
        // Jump away from the surface, biased upward so wall jumps arc.
        let direction = (self.last_support + Vec2::Y).normalize_or(Vec2::Y);
        if self.is_lunging() {
            // Jump cancels a lunge, keeping its momentum.
            self.end_lunge(blob, 1.0);
        }
        for v in &mut blob.velocities {
            // Ignore any fall speed so a coyote jump is as tall as a normal one.
            v.y = v.y.max(0.0);
        }
        blob.launch(direction * self.params.jump_speed);
        self.buffer_left = 0.0;
        self.coyote_left = 0.0;
        self.rising = true;
        events.push(Event::Jumped { direction });
    }

    /// A shrunk blob expanding off a surface: a lunge at a nearby enemy, or a
    /// launch, both stronger with more charge.
    fn burst(&mut self, blob: &mut Blob, aim: Vec2, targets: &[Vec2], events: &mut Vec<Event>) {
        let power = self.charge();
        self.spend_charge();
        self.buffer_left = 0.0;
        self.coyote_left = 0.0;
        self.rising = false;
        blob.unstick();
        if let Some(target) = self.pick_target(blob.center(), aim, targets)
            && !self.is_lunging()
        {
            self.lunge_at(blob, target, power, events);
            return;
        }
        let normal = self.last_support;
        // Aim away from or along the surface to pick the direction; otherwise
        // leap off it, arcing upward from walls.
        let direction = match aim.try_normalize() {
            Some(a) if a.dot(normal) > -0.2 => a,
            _ => (normal + Vec2::Y * 0.5).normalize_or(normal),
        };
        let speed = self.params.jump_speed * (1.0 + (self.params.coil_max_power - 1.0) * power);
        blob.set_velocity(direction * speed);
        events.push(Event::Launched { direction, power });
    }

    fn start_slam(&mut self, blob: &mut Blob, events: &mut Vec<Event>) {
        self.slamming = true;
        self.rising = false;
        blob.unstick();
        let v = blob.velocity();
        blob.set_velocity(Vec2::new(v.x * 0.2, -self.params.slam_speed));
        events.push(Event::SlamStarted);
    }

    /// The closest enemy in lunge range, within the aim cone if the stick is
    /// pushed.
    fn pick_target(&self, from: Vec2, aim: Vec2, targets: &[Vec2]) -> Option<Vec2> {
        let aim = aim.try_normalize().filter(|_| aim.length() > 0.3);
        targets
            .iter()
            .copied()
            .filter(|t| t.distance(from) <= self.params.lunge_range)
            .filter(|t| {
                aim.is_none_or(|a| (*t - from).angle_to(a).abs() <= self.params.lunge_aim_angle)
            })
            .min_by(|a, b| a.distance(from).total_cmp(&b.distance(from)))
    }

    /// Dashes at `target`, for as long as it takes to get there (capped).
    fn lunge_at(&mut self, blob: &mut Blob, target: Vec2, power: f32, events: &mut Vec<Event>) {
        let offset = target - blob.center();
        let direction = offset.normalize_or(Vec2::new(self.facing, 0.0));
        let speed = self.params.lunge_speed * (1.0 + (self.params.coil_max_power - 1.0) * power);
        let time = (offset.length() / speed + 0.06)
            .clamp(self.params.lunge_time, self.params.lunge_max_time);
        self.start_lunge(blob, direction, speed, time, power, events);
    }

    fn start_lunge(
        &mut self,
        blob: &mut Blob,
        direction: Vec2,
        speed: f32,
        time: f32,
        power: f32,
        events: &mut Vec<Event>,
    ) {
        self.lunge_left = time;
        self.lunge_direction = direction;
        self.lunge_speed = speed;
        self.lunge_power = power;
        self.lunge_cooldown_left = self.params.lunge_cooldown;
        self.rising = false;
        if direction.x.abs() > 0.1 {
            self.facing = direction.x.signum();
        }
        blob.unstick();
        blob.set_velocity(direction * speed);
        events.push(Event::Lunged { direction });
    }

    /// The scripted dash from `Intent::lunge_pressed`: aimed, with a little
    /// aim assist, and a slam when aimed down in the air.
    fn forced_lunge(
        &mut self,
        blob: &mut Blob,
        intent: Intent,
        targets: &[Vec2],
        touching: bool,
        events: &mut Vec<Event>,
    ) {
        let ready = !self.is_lunging()
            && !self.slamming
            && self.lunge_cooldown_left <= 0.0
            && (touching || self.air_lunge_ready);
        if self.lunge_buffer_left <= 0.0 || !ready {
            return;
        }
        self.lunge_buffer_left = 0.0;
        if !touching {
            self.air_lunge_ready = false;
        }
        if !touching && intent.aim.y < -self.params.slam_aim {
            self.lunge_cooldown_left = self.params.lunge_cooldown;
            self.start_slam(blob, events);
            return;
        }
        let mut direction = if intent.aim.length() > 0.3 {
            intent.aim.normalize()
        } else {
            Vec2::new(self.facing, 0.0)
        };
        if touching && direction.y < 0.0 {
            // Can't dash into the floor: go along it instead.
            let side = if direction.x.abs() > 0.1 {
                direction.x.signum()
            } else {
                self.facing
            };
            direction = Vec2::new(side, 0.0);
        }
        let from = blob.center();
        let assisted = targets
            .iter()
            .map(|t| *t - from)
            .filter(|d| d.length() <= self.params.lunge_range)
            .filter_map(|d| d.try_normalize().map(|n| (d.length(), n)))
            .filter(|(_, n)| n.angle_to(direction).abs() <= self.params.aim_assist_angle)
            .filter(|(_, n)| !touching || n.y >= -0.05)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, n)| n);
        let direction = assisted.unwrap_or(direction);
        let (speed, time) = (self.params.lunge_speed, self.params.lunge_time);
        self.start_lunge(blob, direction, speed, time, 0.0, events);
    }

    fn end_lunge(&mut self, blob: &mut Blob, keep: f32) {
        self.lunge_left = 0.0;
        blob.gravity_scale = 1.0;
        let v = blob.velocity();
        blob.set_velocity(v * keep);
    }
}
