//! Enemies. For now only the training dummy: a heavy disc that takes hits,
//! gets knocked around, and comes back after it dies.

use glam::Vec2;

use crate::level::Level;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnemyKind {
    /// Never attacks. Respawns where it started a moment after dying.
    Dummy,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Enemy {
    pub kind: EnemyKind,
    pub position: Vec2,
    pub velocity: Vec2,
    pub radius: f32,
    pub health: f32,
    pub max_health: f32,
    /// Time left before it can be hit again, so one lunge is one hit.
    pub hit_cooldown: f32,
    /// While dead, time left until it respawns.
    pub respawn_left: Option<f32>,
    pub home: Vec2,
    pub on_ground: bool,
}

const GRAVITY: f32 = -30.0;
const GROUND_FRICTION: f32 = 8.0;
const RESPAWN_TIME: f32 = 2.0;

impl Enemy {
    pub fn dummy(position: Vec2) -> Self {
        Self {
            kind: EnemyKind::Dummy,
            position,
            velocity: Vec2::ZERO,
            radius: 0.6,
            health: 100.0,
            max_health: 100.0,
            hit_cooldown: 0.0,
            respawn_left: None,
            home: position,
            on_ground: false,
        }
    }

    pub fn alive(&self) -> bool {
        self.respawn_left.is_none()
    }

    /// Applies damage and knockback. Returns true if this killed it.
    pub fn hurt(&mut self, damage: f32, knockback: Vec2) -> bool {
        if !self.alive() {
            return false;
        }
        self.health -= damage;
        self.velocity = knockback;
        if self.health <= 0.0 {
            self.health = 0.0;
            self.respawn_left = Some(RESPAWN_TIME);
            return true;
        }
        false
    }

    pub fn step(&mut self, level: &Level, dt: f32) {
        self.hit_cooldown = (self.hit_cooldown - dt).max(0.0);
        if let Some(left) = &mut self.respawn_left {
            *left -= dt;
            if *left <= 0.0 {
                *self = Self::dummy(self.home);
            }
            return;
        }
        self.velocity.y += GRAVITY * dt;
        self.position += self.velocity * dt;
        self.on_ground = false;
        for solid in &level.solids {
            if let Some((normal, depth)) = solid.penetration(self.position, self.radius) {
                self.position += normal * depth;
                let into = self.velocity.dot(normal);
                if into < 0.0 {
                    self.velocity -= normal * into;
                }
                if normal.y > 0.5 {
                    self.on_ground = true;
                }
            }
        }
        if self.on_ground {
            self.velocity.x *= (1.0 - GROUND_FRICTION * dt).max(0.0);
        }
    }
}
