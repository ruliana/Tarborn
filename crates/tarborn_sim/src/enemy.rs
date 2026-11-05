//! Enemies: bodies that move on the ground, and a small state machine that
//! decides what they do and how easy they are to hurt.
//!
//! Every enemy kind shares one vocabulary of [`State`]s but only uses some of
//! them. The state is the whole point: an enemy that hasn't noticed you takes
//! triple damage, a guard with its shield up can't be hurt from the front, an
//! enemy winding up an attack can be interrupted into dizziness, and so on.

use glam::Vec2;

use crate::controller::HitKind;
use crate::level::Level;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnemyKind {
    /// Never moves or attacks. Respawns where it started a moment after dying.
    Dummy,
    /// Small and harmless. Wanders, startles, and runs away.
    Grub,
    /// Patrols. Blocks hits from the front with a shield and swings at you
    /// up close after a visible wind-up.
    Guard,
    /// Keeps its distance and spits tar at you after aiming. Wide open while
    /// it recovers from a shot.
    Spitter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackPhase {
    /// The telegraph. Hitting the enemy now interrupts it.
    Windup,
    /// The hitbox is out.
    Strike,
    /// Open after swinging.
    Recover,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Going about its business. The first hit lands as a surprise.
    Unaware,
    /// Just caught off guard. Frozen for a moment and takes triple damage.
    Surprised,
    /// Heard something and is turning to look.
    Suspicious,
    /// Lost sight of the player and is checking where it last saw them.
    Searching,
    /// Knows where the player is and is coming for them, shield up.
    Defending,
    Attacking(AttackPhase),
    /// Stunned. Takes double damage.
    Dizzy,
    /// Knocked over. Takes double damage.
    Prone,
    Fleeing,
}

/// What an enemy perceives this tick. Worked out by the simulation.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Senses {
    pub player: Vec2,
    /// The player is in front, in range, and not hidden behind level geometry.
    pub sees_player: bool,
    /// The closest noise this enemy can hear, if any.
    pub heard: Option<Vec2>,
}

/// What happened when the player hit an enemy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HitOutcome {
    /// The shield took it. No damage.
    Blocked,
    Hurt {
        damage: f32,
        killed: bool,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Enemy {
    pub kind: EnemyKind,
    pub state: State,
    /// Seconds spent in the current state.
    pub state_time: f32,
    /// 1 faces right, -1 faces left.
    pub facing: f32,
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
    /// Where it last saw the player, or the noise it is checking.
    pub target: Option<Vec2>,
    /// Seconds since it last saw the player.
    pub unseen_for: f32,
    /// The current attack already hit someone.
    pub struck: bool,
    /// Horizontal speed it wants to move at.
    walk: f32,
    /// How long the player has been behind it while it is alert.
    turn_wait: f32,
    /// Set for one tick when it fires: the direction of the shot.
    pub fire: Option<Vec2>,
    /// The player is holding it. It can't act, and the simulation moves it.
    pub held: bool,
    /// Flying after a throw: it hurts whatever it hits.
    pub thrown: bool,
    /// Fastest speed into a surface during the last step.
    pub impact: f32,
}

/// Per-kind numbers.
struct Stats {
    radius: f32,
    health: f32,
    walk_speed: f32,
    run_speed: f32,
    respawn: f32,
}

impl EnemyKind {
    fn stats(self) -> Stats {
        match self {
            EnemyKind::Dummy => Stats {
                radius: 0.6,
                health: 100.0,
                walk_speed: 0.0,
                run_speed: 0.0,
                respawn: 2.0,
            },
            EnemyKind::Grub => Stats {
                radius: 0.35,
                health: 30.0,
                walk_speed: 0.8,
                run_speed: 4.5,
                respawn: 4.0,
            },
            EnemyKind::Guard => Stats {
                radius: 0.6,
                health: 80.0,
                walk_speed: 1.5,
                run_speed: 3.0,
                respawn: 5.0,
            },
            EnemyKind::Spitter => Stats {
                radius: 0.45,
                health: 40.0,
                walk_speed: 1.0,
                run_speed: 2.5,
                respawn: 5.0,
            },
        }
    }

    /// How far it can see in front of itself.
    pub fn sight_range(self) -> f32 {
        match self {
            EnemyKind::Dummy => 0.0,
            EnemyKind::Grub => 4.0,
            EnemyKind::Guard => 8.0,
            EnemyKind::Spitter => 10.0,
        }
    }
}

const GRAVITY: f32 = -30.0;
const GROUND_FRICTION: f32 = 8.0;
const WALK_ACCEL: f32 = 30.0;

pub const GUARD_REACH: f32 = 1.6;
pub const GUARD_WINDUP: f32 = 0.45;
pub const GUARD_STRIKE: f32 = 0.15;
pub const GUARD_RECOVER: f32 = 0.5;
/// Centre of the guard's swing, ahead of it.
pub const GUARD_SWING_OFFSET: f32 = 1.0;
pub const GUARD_SWING_RADIUS: f32 = 0.75;
const PATROL_RANGE: f32 = 3.0;
const SURPRISE_TIME: f32 = 0.6;
const DIZZY_TIME: f32 = 1.5;
const PRONE_TIME: f32 = 1.2;
const SUSPICIOUS_TIME: f32 = 3.0;
const SEARCH_TIME: f32 = 4.0;
const LOSE_TRACK_TIME: f32 = 1.5;
const FLEE_TIME: f32 = 3.0;
/// How long it takes to react to a noise, and to turn around when the player
/// gets behind it. Both are windows for the player.
const REACTION_TIME: f32 = 0.3;
const TURN_TIME: f32 = 0.4;
/// Spotting the player closer than this startles an unaware enemy.
const STARTLE_DISTANCE: f32 = 3.0;
pub const SPIT_AIM: f32 = 0.6;
const SPIT_RECOVER: f32 = 0.8;
/// A spitter backs away from a player closer than this.
const SPIT_COMFORT: f32 = 3.0;
const BACK_OFF_TIME: f32 = 1.2;
/// An unaware spitter looks the other way this often.
const LOOK_AROUND: f32 = 2.5;

impl Enemy {
    pub fn new(kind: EnemyKind, position: Vec2) -> Self {
        let stats = kind.stats();
        Self {
            kind,
            state: State::Unaware,
            state_time: 0.0,
            facing: 1.0,
            position,
            velocity: Vec2::ZERO,
            radius: stats.radius,
            health: stats.health,
            max_health: stats.health,
            hit_cooldown: 0.0,
            respawn_left: None,
            home: position,
            on_ground: false,
            target: None,
            unseen_for: 0.0,
            struck: false,
            walk: 0.0,
            turn_wait: 0.0,
            fire: None,
            held: false,
            thrown: false,
            impact: 0.0,
        }
    }

    pub fn dummy(position: Vec2) -> Self {
        Self::new(EnemyKind::Dummy, position)
    }

    pub fn spitter(position: Vec2) -> Self {
        Self::new(EnemyKind::Spitter, position)
    }

    pub fn grub(position: Vec2) -> Self {
        Self::new(EnemyKind::Grub, position)
    }

    pub fn guard(position: Vec2) -> Self {
        Self::new(EnemyKind::Guard, position)
    }

    pub fn alive(&self) -> bool {
        self.respawn_left.is_none()
    }

    /// Whether a grab from `direction` (the way the lunge travels) can latch on.
    /// A guard's shield stops grabs from the front too.
    pub fn grabbable(&self, direction: Vec2) -> bool {
        let shield_up = matches!(
            self.state,
            State::Defending | State::Attacking(AttackPhase::Windup)
        );
        let from_front = direction.x * self.facing < -0.2;
        !(self.kind == EnemyKind::Guard && shield_up && from_front)
    }

    /// True while the guard's swing can hurt.
    pub fn striking(&self) -> bool {
        self.state == State::Attacking(AttackPhase::Strike)
    }

    fn set_state(&mut self, state: State) {
        if self.state != state {
            self.state = state;
            self.state_time = 0.0;
            self.struck = false;
        }
    }

    fn face(&mut self, point: Vec2) {
        let dx = point.x - self.position.x;
        if dx.abs() > 0.05 {
            self.facing = dx.signum();
        }
    }

    /// Takes a hit from the player. `direction` is the way the hit travels
    /// (from the player toward the enemy). The state decides the damage and
    /// what state it goes to next.
    pub fn take_hit(
        &mut self,
        kind: HitKind,
        direction: Vec2,
        damage: f32,
        knockback: Vec2,
    ) -> HitOutcome {
        if !self.alive() {
            return HitOutcome::Hurt {
                damage: 0.0,
                killed: false,
            };
        }
        // The hit comes from the side the enemy is facing when it travels against its facing.
        let from_front = direction.x * self.facing < -0.2;
        let from_above = matches!(kind, HitKind::Slam | HitKind::Pogo | HitKind::Shockwave);
        let heavy = from_above || kind == HitKind::Impact;

        let (multiplier, next) = match self.kind {
            EnemyKind::Dummy => (1.0, self.state),
            EnemyKind::Grub => match self.state {
                State::Unaware | State::Surprised => (3.0, State::Fleeing),
                State::Prone => (2.0, State::Prone),
                _ if heavy => (1.0, State::Prone),
                _ => (1.0, State::Fleeing),
            },
            EnemyKind::Guard => match self.state {
                State::Defending | State::Attacking(AttackPhase::Windup)
                    if from_front && !from_above =>
                {
                    self.velocity = Vec2::new(direction.x.signum() * 2.0, 0.0);
                    return HitOutcome::Blocked;
                }
                State::Unaware | State::Surprised => (3.0, State::Surprised),
                State::Suspicious if self.state_time < REACTION_TIME => (3.0, State::Surprised),
                State::Suspicious if !from_front => (1.5, State::Surprised),
                State::Attacking(AttackPhase::Windup) => (1.5, State::Dizzy),
                State::Attacking(AttackPhase::Recover) => (2.0, State::Defending),
                State::Dizzy | State::Prone if from_above => (2.0, State::Prone),
                State::Dizzy | State::Prone => (2.0, self.state),
                _ if from_above => (1.0, State::Dizzy),
                _ => (1.0, State::Defending),
            },
            EnemyKind::Spitter => match self.state {
                State::Unaware | State::Surprised => (3.0, State::Surprised),
                State::Suspicious if self.state_time < REACTION_TIME => (3.0, State::Surprised),
                State::Attacking(AttackPhase::Recover) => (2.0, State::Fleeing),
                State::Dizzy | State::Prone if from_above => (2.0, State::Prone),
                State::Dizzy | State::Prone => (2.0, self.state),
                _ if from_above => (1.0, State::Dizzy),
                _ => (1.0, State::Fleeing),
            },
        };

        let damage = damage * multiplier;
        self.health -= damage;
        self.velocity = knockback;
        if self.health <= 0.0 {
            self.health = 0.0;
            self.respawn_left = Some(self.kind.stats().respawn);
            return HitOutcome::Hurt {
                damage,
                killed: true,
            };
        }
        // Being hit always tells it where you are.
        self.target = Some(self.position - direction);
        self.unseen_for = 0.0;
        if next != self.state || matches!(next, State::Dizzy | State::Prone) {
            self.face(self.position - direction);
            self.state = next;
            self.state_time = 0.0;
        }
        HitOutcome::Hurt {
            damage,
            killed: false,
        }
    }

    /// Decides what to do this tick.
    pub fn think(&mut self, senses: Senses, level: &Level, dt: f32) {
        if !self.alive() {
            return;
        }
        if self.held || self.thrown {
            // Helpless: stays dizzy until it lands and recovers.
            self.walk = 0.0;
            if self.state != State::Prone {
                self.state = State::Dizzy;
                self.state_time = 0.0;
            }
            return;
        }
        self.state_time += dt;
        if senses.sees_player {
            self.target = Some(senses.player);
            self.unseen_for = 0.0;
        } else {
            self.unseen_for += dt;
        }
        self.walk = 0.0;
        match self.kind {
            EnemyKind::Dummy => {}
            EnemyKind::Grub => self.think_grub(senses),
            EnemyKind::Guard => self.think_guard(senses, dt),
            EnemyKind::Spitter => self.think_spitter(senses),
        }
        // Don't walk off ledges or into walls.
        if self.walk != 0.0 && !self.can_walk(level, self.walk.signum()) {
            self.walk = 0.0;
            if self.state == State::Unaware {
                self.facing = -self.facing;
            }
        }
    }

    fn think_grub(&mut self, senses: Senses) {
        let stats = self.kind.stats();
        let near = senses.player.distance(self.position) < 2.5;
        match self.state {
            State::Unaware => {
                self.patrol(stats.walk_speed, 2.0);
                if senses.sees_player || senses.heard.is_some() {
                    self.set_state(if near && senses.sees_player {
                        State::Surprised
                    } else {
                        State::Fleeing
                    });
                }
            }
            State::Surprised if self.state_time > SURPRISE_TIME => self.set_state(State::Fleeing),
            State::Prone if self.state_time > PRONE_TIME => self.set_state(State::Fleeing),
            State::Dizzy if self.state_time > DIZZY_TIME => self.set_state(State::Fleeing),
            State::Fleeing => {
                if let Some(threat) = self.target.or(senses.heard) {
                    self.facing = (self.position.x - threat.x).signum();
                    if self.facing == 0.0 {
                        self.facing = 1.0;
                    }
                }
                self.walk = self.facing * stats.run_speed;
                if self.state_time > FLEE_TIME && self.unseen_for > FLEE_TIME {
                    self.target = None;
                    self.set_state(State::Unaware);
                }
            }
            _ => {}
        }
    }

    fn think_guard(&mut self, senses: Senses, dt: f32) {
        let stats = self.kind.stats();
        let t = self.state_time;
        match self.state {
            State::Unaware => {
                self.patrol(stats.walk_speed, PATROL_RANGE);
                if senses.sees_player {
                    let close = senses.player.distance(self.position) < STARTLE_DISTANCE;
                    self.set_state(if close {
                        State::Surprised
                    } else {
                        State::Defending
                    });
                } else if let Some(noise) = senses.heard {
                    self.target = Some(noise);
                    self.set_state(State::Suspicious);
                }
            }
            State::Suspicious => {
                if let Some(noise) = senses.heard {
                    self.target = Some(noise);
                }
                if let Some(target) = self.target
                    && t > REACTION_TIME
                {
                    self.face(target);
                    if (target.x - self.position.x).abs() > 0.5 && t > 0.4 {
                        self.walk = self.facing * stats.walk_speed;
                    }
                }
                if senses.sees_player {
                    self.set_state(State::Defending);
                } else if t > SUSPICIOUS_TIME {
                    self.target = None;
                    self.set_state(State::Unaware);
                }
            }
            State::Surprised if t > SURPRISE_TIME => self.set_state(State::Defending),
            State::Dizzy if t > DIZZY_TIME => self.set_state(State::Defending),
            State::Prone if t > PRONE_TIME => self.set_state(State::Dizzy),
            State::Defending => {
                if let Some(target) = self.target {
                    self.turn_slowly(target, dt);
                    let gap = (target - self.position).length();
                    if senses.sees_player && gap < GUARD_REACH {
                        self.set_state(State::Attacking(AttackPhase::Windup));
                    } else if gap > GUARD_REACH * 0.8 {
                        self.walk = self.facing * stats.run_speed;
                    }
                }
                if self.unseen_for > LOSE_TRACK_TIME {
                    self.set_state(State::Searching);
                }
            }
            State::Attacking(AttackPhase::Windup) if t > GUARD_WINDUP => {
                self.set_state(State::Attacking(AttackPhase::Strike));
                // A small step into the swing.
                self.velocity.x = self.facing * 3.0;
            }
            State::Attacking(AttackPhase::Strike) if t > GUARD_STRIKE => {
                self.set_state(State::Attacking(AttackPhase::Recover));
            }
            State::Attacking(AttackPhase::Recover) if t > GUARD_RECOVER => {
                self.set_state(State::Defending);
            }
            State::Searching => {
                if senses.sees_player {
                    self.set_state(State::Defending);
                } else if let Some(target) = self.target {
                    if (target.x - self.position.x).abs() > 0.5 {
                        self.face(target);
                        self.walk = self.facing * stats.walk_speed;
                    } else {
                        // Look both ways.
                        self.facing = if ((t * 1.25) as u32).is_multiple_of(2) {
                            1.0
                        } else {
                            -1.0
                        };
                    }
                }
                if t > SEARCH_TIME {
                    self.target = None;
                    self.set_state(State::Unaware);
                }
            }
            _ => {}
        }
    }

    fn think_spitter(&mut self, senses: Senses) {
        let stats = self.kind.stats();
        let t = self.state_time;
        let close = senses.player.distance(self.position) < SPIT_COMFORT;
        let attack_or_back_off = |e: &mut Self| {
            e.set_state(if close {
                State::Fleeing
            } else {
                State::Attacking(AttackPhase::Windup)
            });
        };
        match self.state {
            State::Unaware => {
                if t > LOOK_AROUND {
                    self.facing = -self.facing;
                    self.state_time = 0.0;
                }
                if senses.sees_player {
                    if close {
                        self.set_state(State::Surprised);
                    } else {
                        self.set_state(State::Attacking(AttackPhase::Windup));
                    }
                } else if let Some(noise) = senses.heard {
                    self.target = Some(noise);
                    self.set_state(State::Suspicious);
                }
            }
            State::Suspicious => {
                if let Some(noise) = senses.heard {
                    self.target = Some(noise);
                }
                if let Some(target) = self.target
                    && t > REACTION_TIME
                {
                    self.face(target);
                }
                if senses.sees_player {
                    attack_or_back_off(self);
                } else if t > SUSPICIOUS_TIME {
                    self.target = None;
                    self.set_state(State::Unaware);
                }
            }
            State::Surprised if t > SURPRISE_TIME => self.set_state(State::Fleeing),
            State::Dizzy if t > DIZZY_TIME => self.set_state(State::Fleeing),
            State::Prone if t > PRONE_TIME => self.set_state(State::Dizzy),
            State::Fleeing => {
                // Back away while still facing the player.
                if let Some(target) = self.target {
                    self.face(target);
                    self.walk = -self.facing * stats.run_speed;
                }
                if t > BACK_OFF_TIME || !close && t > 0.3 {
                    if senses.sees_player {
                        self.set_state(State::Attacking(AttackPhase::Windup));
                    } else {
                        self.set_state(State::Searching);
                    }
                }
            }
            State::Attacking(AttackPhase::Windup) => {
                if let Some(target) = self.target {
                    self.face(target);
                    if t > SPIT_AIM {
                        self.fire = (target - self.position).try_normalize();
                        self.set_state(State::Attacking(AttackPhase::Strike));
                    }
                }
            }
            State::Attacking(AttackPhase::Strike) if t > 0.05 => {
                self.set_state(State::Attacking(AttackPhase::Recover));
            }
            State::Attacking(AttackPhase::Recover) if t > SPIT_RECOVER => {
                if senses.sees_player {
                    attack_or_back_off(self);
                } else {
                    self.set_state(State::Searching);
                }
            }
            State::Searching => {
                if let Some(target) = self.target {
                    self.face(target);
                }
                if senses.sees_player {
                    attack_or_back_off(self);
                } else if t > SEARCH_TIME {
                    self.target = None;
                    self.set_state(State::Unaware);
                }
            }
            _ => {}
        }
    }

    /// Turns to face `point`, but only once it has been behind for a moment.
    fn turn_slowly(&mut self, point: Vec2, dt: f32) {
        let behind = (point.x - self.position.x) * self.facing < -0.05;
        if behind {
            self.turn_wait += dt;
            if self.turn_wait > TURN_TIME {
                self.facing = -self.facing;
                self.turn_wait = 0.0;
            }
        } else {
            self.turn_wait = 0.0;
        }
    }

    /// Walks back and forth around home.
    fn patrol(&mut self, speed: f32, range: f32) {
        let offset = self.position.x - self.home.x;
        if offset > range {
            self.facing = -1.0;
        } else if offset < -range {
            self.facing = 1.0;
        }
        self.walk = self.facing * speed;
    }

    fn can_walk(&self, level: &Level, direction: f32) -> bool {
        let r = self.radius;
        let ahead = self.position + Vec2::new(direction * (r + 0.25), 0.0);
        let below = self.position + Vec2::new(direction * (r + 0.25), -(r + 0.4));
        let solid_at = |p: Vec2| {
            level
                .solids
                .iter()
                .any(|s| s.penetration(p, 0.05).is_some())
        };
        !solid_at(ahead) && solid_at(below)
    }

    /// Physics: gravity, walking, and collision with the level.
    pub fn step(&mut self, level: &Level, dt: f32) {
        self.hit_cooldown = (self.hit_cooldown - dt).max(0.0);
        if let Some(left) = &mut self.respawn_left {
            *left -= dt;
            if *left <= 0.0 {
                *self = Self::new(self.kind, self.home);
            }
            return;
        }
        if self.held {
            return;
        }
        let can_move = !matches!(
            self.state,
            State::Surprised | State::Dizzy | State::Prone | State::Attacking(_)
        );
        if self.thrown {
            // A thrown body slides freely until it slows down or hits something.
        } else if self.on_ground && can_move {
            let change = (self.walk - self.velocity.x).clamp(-WALK_ACCEL * dt, WALK_ACCEL * dt);
            self.velocity.x += change;
        } else if self.on_ground {
            self.velocity.x *= (1.0 - GROUND_FRICTION * dt).max(0.0);
        }
        self.velocity.y += GRAVITY * dt;
        self.position += self.velocity * dt;
        self.on_ground = false;
        self.impact = 0.0;
        for solid in &level.solids {
            if let Some((normal, depth)) = solid.penetration(self.position, self.radius) {
                self.position += normal * depth;
                let into = self.velocity.dot(normal);
                if into < 0.0 {
                    self.velocity -= normal * into;
                    self.impact = self.impact.max(-into);
                }
                if normal.y > 0.5 {
                    self.on_ground = true;
                }
            }
        }
    }
}

/// True if nothing in the level blocks the straight line from `a` to `b`.
pub fn line_of_sight(level: &Level, a: Vec2, b: Vec2) -> bool {
    let steps = (a.distance(b) / 0.25).ceil().max(1.0) as usize;
    (1..steps).all(|i| {
        let p = a.lerp(b, i as f32 / steps as f32);
        level.solids.iter().all(|s| s.penetration(p, 0.0).is_none())
    })
}
