//! Turns player intent into blob actions, with the forgiveness that makes a
//! platformer feel responsive: jump buffering, coyote time and variable jump
//! height.

use glam::Vec2;

use crate::blob::{Blob, TarModes};
use crate::level::Level;

/// What the player wants this tick, already sampled from the input devices.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Intent {
    /// -1 (left) to 1 (right).
    pub roll: f32,
    /// The jump button went down since the previous tick.
    pub jump_pressed: bool,
    /// The jump button is down right now.
    pub jump_held: bool,
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
}

impl Default for FeelParams {
    fn default() -> Self {
        Self {
            jump_buffer: 0.12,
            coyote_time: 0.10,
            jump_speed: 12.0,
            jump_cut: 0.45,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Controller {
    pub params: FeelParams,
    buffer_left: f32,
    coyote_left: f32,
    last_support: Vec2,
    rising: bool,
}

/// Something notable that happened during a step, for sound and effects.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    Jumped { direction: Vec2 },
}

impl Controller {
    pub fn new(params: FeelParams) -> Self {
        Self {
            params,
            ..Default::default()
        }
    }

    pub fn step(
        &mut self,
        blob: &mut Blob,
        level: &Level,
        intent: Intent,
        dt: f32,
    ) -> Option<Event> {
        let mut event = None;

        if let Some(normal) = blob.support_normal(intent.modes) {
            self.coyote_left = self.params.coyote_time;
            self.last_support = normal;
        }
        if intent.jump_pressed {
            self.buffer_left = self.params.jump_buffer;
        }

        if self.buffer_left > 0.0 && self.coyote_left > 0.0 {
            // Jump away from the surface, biased upward so wall jumps arc.
            let direction = (self.last_support + Vec2::Y).normalize_or(Vec2::Y);
            for v in &mut blob.velocities {
                // Ignore any fall speed so a coyote jump is as tall as a normal one.
                v.y = v.y.max(0.0);
            }
            blob.launch(direction * self.params.jump_speed);
            self.buffer_left = 0.0;
            self.coyote_left = 0.0;
            self.rising = true;
            event = Some(Event::Jumped { direction });
        }

        if self.rising {
            if blob.velocity().y <= 0.0 {
                self.rising = false;
            } else if !intent.jump_held {
                blob.cut_rise(self.params.jump_cut);
                self.rising = false;
            }
        }

        blob.step(level, dt, intent.roll, intent.modes);

        self.buffer_left = (self.buffer_left - dt).max(0.0);
        self.coyote_left = (self.coyote_left - dt).max(0.0);
        event
    }
}
