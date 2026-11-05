//! Turns player intent into blob actions, with the forgiveness that makes a
//! platformer feel responsive: jump and lunge buffering, coyote time and
//! variable jump height.
//!
//! The move set:
//! - **Jump** off floors (and walls or ceilings while sticky).
//! - **Lunge**: a short, gravity-free burst in the aimed direction. It is the
//!   dodge and the attack in one, and the blob is invulnerable during it. One
//!   lunge in the air, refilled by touching any surface or by
//!   [`Controller::refresh_lunge`] (hitting an enemy).
//! - **Slam**: lunge aimed down while airborne. Drives straight down until it
//!   lands. With bouncy held, the landing rebounds high.

use glam::Vec2;

use crate::blob::{Blob, TarModes};
use crate::level::Level;

/// What the player wants this tick, already sampled from the input devices.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Intent {
    /// -1 (left) to 1 (right).
    pub roll: f32,
    /// Stick direction, including up and down. Its length is at most 1.
    /// Zero means "the way I'm facing".
    pub aim: Vec2,
    /// The jump button went down since the previous tick.
    pub jump_pressed: bool,
    /// The jump button is down right now.
    pub jump_held: bool,
    /// The lunge button went down since the previous tick.
    pub lunge_pressed: bool,
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
    /// A lunge snaps toward a target within this distance...
    pub aim_assist_range: f32,
    /// ...and within this angle (radians) of where it was aimed.
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
            aim_assist_range: 5.0,
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
    /// 1 for right, -1 for left: where an unaimed lunge goes.
    facing: f32,
    lunge_buffer_left: f32,
    lunge_left: f32,
    lunge_direction: Vec2,
    lunge_cooldown_left: f32,
    air_lunge_ready: bool,
    slamming: bool,
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
    /// Landing on an enemy with bouncy held.
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
            slamming: false,
        }
    }

    pub fn is_lunging(&self) -> bool {
        self.lunge_left > 0.0
    }

    pub fn is_slamming(&self) -> bool {
        self.slamming
    }

    /// Lunges and slams can't be hurt.
    pub fn invulnerable(&self) -> bool {
        self.is_lunging() || self.slamming
    }

    /// The direction an unaimed lunge goes: 1 right, -1 left.
    pub fn facing(&self) -> f32 {
        self.facing
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

    /// Advances one tick. `targets` are points a lunge may snap toward.
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
        if intent.jump_pressed {
            self.buffer_left = self.params.jump_buffer;
        }
        if intent.lunge_pressed {
            self.lunge_buffer_left = self.params.lunge_buffer;
        }

        // Lunge or slam.
        let can_lunge = !self.is_lunging()
            && !self.slamming
            && self.lunge_cooldown_left <= 0.0
            && (touching || self.air_lunge_ready);
        if self.lunge_buffer_left > 0.0 && can_lunge {
            self.lunge_buffer_left = 0.0;
            self.lunge_cooldown_left = self.params.lunge_cooldown;
            self.rising = false;
            if !touching {
                self.air_lunge_ready = false;
            }
            blob.unstick();
            if !touching && intent.aim.y < -self.params.slam_aim {
                self.slamming = true;
                let v = blob.velocity();
                blob.set_velocity(Vec2::new(v.x * 0.2, -self.params.slam_speed));
                events.push(Event::SlamStarted);
            } else {
                let mut direction = if intent.aim.length() > 0.3 {
                    intent.aim.normalize()
                } else {
                    Vec2::new(self.facing, 0.0)
                };
                if touching && direction.y < 0.0 {
                    // Can't lunge into the floor: go along it instead.
                    let side = if direction.x.abs() > 0.1 {
                        direction.x.signum()
                    } else {
                        self.facing
                    };
                    direction = Vec2::new(side, 0.0);
                }
                direction = self.assist_aim(blob.center(), direction, targets, touching);
                if direction.x.abs() > 0.1 {
                    self.facing = direction.x.signum();
                }
                self.lunge_left = self.params.lunge_time;
                self.lunge_direction = direction;
                events.push(Event::Lunged { direction });
            }
        }

        // Jump. It cancels a lunge, keeping the lunge's momentum.
        if self.buffer_left > 0.0 && self.coyote_left > 0.0 && !self.slamming {
            // Jump away from the surface, biased upward so wall jumps arc.
            let direction = (self.last_support + Vec2::Y).normalize_or(Vec2::Y);
            if self.is_lunging() {
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
            blob.set_velocity(self.lunge_direction * self.params.lunge_speed);
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

    /// Bends `direction` toward the closest target in the assist cone, so a
    /// roughly aimed lunge still connects.
    fn assist_aim(&self, from: Vec2, direction: Vec2, targets: &[Vec2], touching: bool) -> Vec2 {
        let p = &self.params;
        targets
            .iter()
            .map(|t| *t - from)
            .filter(|d| d.length() <= p.aim_assist_range)
            .filter_map(|d| d.try_normalize().map(|n| (d.length(), n)))
            .filter(|(_, n)| n.angle_to(direction).abs() <= p.aim_assist_angle)
            .filter(|(_, n)| !touching || n.y >= -0.05)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map_or(direction, |(_, n)| n)
    }

    fn end_lunge(&mut self, blob: &mut Blob, keep: f32) {
        self.lunge_left = 0.0;
        blob.gravity_scale = 1.0;
        let v = blob.velocity();
        blob.set_velocity(v * keep);
    }
}
