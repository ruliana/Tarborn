//! Turns player intent into blob actions, with the forgiveness that makes a
//! platformer feel responsive: jump buffering, coyote time and variable jump
//! height.
//!
//! Two verbs, two buttons:
//! - **Grip (hold) shrinks.** The blob gets small and sticky, and charges up
//!   the longer it stays shrunk. Pressing it is a dodge: nothing can hurt
//!   the blob for a moment after, and an attack landing just before the
//!   press is dodged too (see `combat`). Pressed just after a bounce, it
//!   pulls the blob back onto the surface. While gripping a wall or ceiling,
//!   directions follow the screen.
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(default))]
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
    /// How long the blob takes to shrink, once Grip goes down.
    pub shrink_time: f32,
    /// How long the blob can't be hurt after Grip goes down: the dodge.
    pub dodge_time: f32,
    /// An attack that lands this long before Grip goes down is still dodged:
    /// the hurt waits this long for a late press.
    pub dodge_grace: f32,
    /// After a dodge, shrinking again within this long is instant and doesn't
    /// dodge, so tapping Grip can't chain dodges forever.
    pub dodge_cooldown: f32,
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
    /// Grip pressed this long after a bounce off a wall or ceiling undoes it:
    /// the blob is pulled back onto that surface, and sticks.
    pub grip_catch_time: f32,
    /// Speed toward the surface when a late Grip catches a bounce.
    pub grip_catch_speed: f32,
    /// Touching an enemy with Grip held grabs it, from this much further
    /// than a hit reaches.
    pub grab_reach: f32,
    /// On a surface, touching an enemy grabs it only this long after Grip
    /// goes down. In the air, any touch with Grip held grabs.
    pub touch_grab_window: f32,
    /// Gripping a wall, pushing into it climbs it at this share of pushing
    /// up (and pulling away goes down). 0 reads only the stick's up and down.
    pub wall_push_climb: f32,
    /// Directions keep following a wall or ceiling this long after the blob
    /// comes off it, so climbing doesn't drop when rolling lifts it a moment.
    pub surface_memory: f32,
    /// Hitting a wall or ceiling with Grip held keeps this share of the speed
    /// along it, and it can't spring back off. Surfaces whose normal points up
    /// more than `splat_max_normal_y` (floors) don't splat.
    pub splat_slide_keep: f32,
    pub splat_max_normal_y: f32,
    /// For this long after splatting, the soft body can't spring back off.
    pub splat_settle_time: f32,
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
            shrink_time: 0.15,
            dodge_time: 0.25,
            dodge_grace: 0.1,
            dodge_cooldown: 0.45,
            lunge_range: 5.0,
            lunge_aim_angle: 60f32.to_radians(),
            lunge_max_time: 0.4,
            hold_threshold: 0.1,
            aim_assist_angle: 30f32.to_radians(),
            grip_catch_time: 0.15,
            grip_catch_speed: 6.0,
            grab_reach: 0.15,
            touch_grab_window: 0.2,
            wall_push_climb: 1.0,
            surface_memory: 0.15,
            splat_slide_keep: 0.3,
            splat_max_normal_y: 0.5,
            splat_settle_time: 0.15,
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
    /// Time left shrinking.
    shrink_left: f32,
    /// Time left in the shrinking dodge.
    dodge_left: f32,
    /// Time before shrinking dodges again.
    dodge_ready_in: f32,
    /// Seconds Jump has been held without a break.
    jump_hold: f32,
    /// The surface last gripped, and how much longer it is remembered after
    /// the blob comes off it for a moment (rolling peels it off a wall).
    gripped: Option<(Vec2, f32)>,
    /// Gripping a wall or ceiling since the last tick (see `splat`).
    splatted: bool,
    /// Time left in which the blob can't spring off the surface it splatted on.
    settle_left: f32,
    /// Seconds since the blob last gripped a wall or ceiling.
    off_surface_for: f32,
    /// The last bounce: the surface's normal, where the blob was, and seconds since.
    last_bounce: Option<(Vec2, Vec2, f32)>,
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
    /// An enemy attack passed through the blob while it was shrinking.
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
            shrink_left: 0.0,
            dodge_left: 0.0,
            dodge_ready_in: 0.0,
            jump_hold: 0.0,
            last_bounce: None,
            gripped: None,
            splatted: false,
            settle_left: 0.0,
            off_surface_for: 0.0,
        }
    }

    pub fn is_lunging(&self) -> bool {
        self.lunge_left > 0.0
    }

    /// Grip is held: the blob is small and sticky.
    pub fn is_shrunk(&self) -> bool {
        self.shrunk.is_some()
    }

    /// Seconds since Grip went down, or infinity when it is up.
    pub fn shrunk_for(&self) -> f32 {
        self.shrunk.unwrap_or(f32::INFINITY)
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

    /// The blob is mid-shrink: nothing can hurt it.
    pub fn is_dodging(&self) -> bool {
        self.dodge_left > 0.0
    }

    /// Pressing Grip now would dodge: not already shrunk, and not on cooldown.
    pub fn can_dodge(&self) -> bool {
        self.shrunk.is_none() && self.dodge_ready_in <= 0.0
    }

    /// Lunges, slams and the shrinking dodge can't be hurt.
    pub fn invulnerable(&self) -> bool {
        self.is_lunging() || self.slamming || self.is_dodging()
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

        // Grip shrinks. Going from full size to small is the dodge, when ready.
        self.shrink_left = (self.shrink_left - dt).max(0.0);
        self.dodge_left = (self.dodge_left - dt).max(0.0);
        self.dodge_ready_in = (self.dodge_ready_in - dt).max(0.0);
        if let Some((_, _, age)) = &mut self.last_bounce {
            *age += dt;
        }
        if intent.modes.sticky {
            if self.shrunk.is_none() {
                events.push(Event::Shrank);
                self.shrink_left = self.params.shrink_time;
                if self.dodge_ready_in <= 0.0 {
                    self.dodge_left = self.params.dodge_time;
                    self.dodge_ready_in = self.params.dodge_cooldown;
                }
                self.catch_bounce(blob);
            }
            self.shrunk = Some(self.shrunk.unwrap_or(0.0) + dt);
            let size = 0.4 + 0.6 * self.charge();
            let progress = if self.params.shrink_time > 0.0 {
                1.0 - self.shrink_left / self.params.shrink_time
            } else {
                1.0
            };
            blob.scale = 1.0 + (self.params.coil_min_scale - 1.0) * size * progress;
        } else if self.shrunk.take().is_some() {
            self.shrink_left = 0.0;
            self.dodge_left = 0.0;
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
        let roll = self.surface_roll(blob, intent, dt);
        let bounce = blob.step(level, dt, roll, intent.modes);

        if self.slamming && blob.touching_count() > 0 {
            self.slamming = false;
            events.push(Event::Slammed {
                at: blob.center(),
                speed: -impact.y,
            });
        }
        if let Some(normal) = bounce {
            // Only walls and ceilings: a late Grip should catch the wall a
            // blob bounced off, not the floor it bounced on after.
            if normal.y < self.params.splat_max_normal_y {
                self.last_bounce = Some((normal, blob.center(), 0.0));
            }
            events.push(Event::Bounced { normal });
        }
        self.splat(blob, intent.modes, dt);

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

    /// The roll that moves the blob the way the stick points, on screen. On
    /// the floor that is just left and right. Gripping a wall or a ceiling,
    /// rolling one way can move the blob up, down or backwards, so the stick
    /// is read along the surface: up climbs a wall, and pushing into a wall
    /// climbs it too (pulling away goes down it). On a ceiling, left is left.
    fn surface_roll(&mut self, blob: &Blob, intent: Intent, dt: f32) -> f32 {
        if !intent.modes.sticky || self.is_lunging() || self.slamming {
            self.gripped = None;
            return intent.roll;
        }
        self.gripped = match blob.support_normal(intent.modes) {
            Some(normal) => Some((normal, self.params.surface_memory)),
            None => self
                .gripped
                .map(|(normal, left)| (normal, left - dt))
                .filter(|(_, left)| *left > 0.0),
        };
        let Some((normal, _)) = self.gripped else {
            return intent.roll;
        };
        let stick = if intent.aim.length() > 0.1 {
            intent.aim
        } else {
            Vec2::new(intent.roll, 0.0)
        };
        // Where a clockwise roll (roll = 1) carries the blob along this surface.
        let travel = Vec2::new(normal.y, -normal.x);
        let along = stick.dot(travel);
        let into = stick.dot(-normal) * travel.y * self.params.wall_push_climb;
        (along + into).clamp(-1.0, 1.0)
    }

    /// Tar hitting a wall or ceiling with Grip held splats onto it: the soft
    /// body would otherwise spring back off or slide past before its grip
    /// takes hold. On first contact the blob loses its speed into and most of
    /// its speed along the surface (it keeps pressing in, to spread into a
    /// wide grip), and for a moment after it can't spring away from it.
    fn splat(&mut self, blob: &mut Blob, modes: TarModes, dt: f32) {
        self.settle_left = (self.settle_left - dt).max(0.0);
        let normal = blob
            .grip_normal()
            .filter(|n| modes.sticky && n.y < self.params.splat_max_normal_y)
            .filter(|_| !self.is_lunging() && !self.slamming);
        let Some(normal) = normal else {
            // Crawling lifts the blob off for a moment; that is not a new hit.
            self.off_surface_for += dt;
            if self.off_surface_for > self.params.surface_memory {
                self.splatted = false;
            }
            return;
        };
        self.off_surface_for = 0.0;
        let v = blob.velocity();
        if !self.splatted {
            self.splatted = true;
            self.settle_left = self.params.splat_settle_time;
            // Keep pressing into it, so the body spreads into a wide patch.
            let into = v.dot(normal).min(0.0);
            let along = v - normal * v.dot(normal);
            blob.set_velocity(normal * into + along * self.params.splat_slide_keep);
        } else if self.settle_left > 0.0 && v.dot(normal) > 0.0 {
            blob.set_velocity(v - normal * v.dot(normal));
        }
    }

    /// Grip pressed just after a bounce: the player meant to stick, and was a
    /// moment late. Send the blob back to the surface it bounced off.
    fn catch_bounce(&mut self, blob: &mut Blob) {
        let Some((normal, at, age)) = self.last_bounce.take() else {
            return;
        };
        if age > self.params.grip_catch_time || self.is_lunging() || self.slamming {
            return;
        }
        // Fast enough to get back against gravity pulling off the surface.
        let away = (blob.center() - at).dot(normal).max(0.0);
        let pull_off = (blob.params.gravity * blob.gravity_scale)
            .dot(normal)
            .max(0.0);
        let speed = self.params.grip_catch_speed + (2.0 * pull_off * away).sqrt();
        let v = blob.velocity();
        let along = v - normal * v.dot(normal);
        blob.set_velocity(along - normal * speed);
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
        self.gripped = None;
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
        self.gripped = None;
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
