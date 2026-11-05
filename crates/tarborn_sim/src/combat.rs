//! How the blob hurts things and gets hurt.
//!
//! Damage comes from the body: lunges, slams, pogo bounces and fast impacts.
//! Every hit freezes the world for a moment (hitstop), knocks the target back,
//! and refreshes the player's lunge so hits can chain.

use glam::Vec2;

use crate::controller::{Event, HitKind, Intent};
use crate::enemy::{
    BRUTE_SLAM_RADIUS, EnemyKind, FEAR_DISTANCE, GUARD_SWING_OFFSET, GUARD_SWING_RADIUS,
    HitOutcome, Senses, State, line_of_sight,
};
use crate::{DT, Sim, TarModes};

#[derive(Clone, Debug, PartialEq)]
pub struct CombatParams {
    pub max_health: u32,
    /// Extra reach beyond the shapes when checking contact. Generous on purpose.
    pub hit_reach: f32,
    pub lunge_damage: f32,
    pub slam_damage: f32,
    pub shockwave_damage: f32,
    pub shockwave_radius: f32,
    pub pogo_damage: f32,
    /// Closing speed above which rolling or bouncing into an enemy hurts it.
    pub impact_speed: f32,
    /// Damage per m/s of closing speed above `impact_speed`.
    pub impact_damage_per_speed: f32,
    pub impact_min_damage: f32,
    pub knockback: f32,
    /// Upward speed after a slam or pogo hit.
    pub pogo_speed: f32,
    pub hitstop: f32,
    pub kill_hitstop: f32,
    pub hit_cooldown: f32,
    pub hurt_invulnerability: f32,
    pub hurt_knockback: Vec2,
    pub pickup_radius: f32,
    /// Speed of a thrown enemy, multiplied up to 2x by the shrink charge.
    pub throw_speed: f32,
    /// Damage a thrown body does: this plus `thrown_damage_per_speed` per m/s.
    pub thrown_damage: f32,
    pub thrown_damage_per_speed: f32,
    /// A thrown body hitting a wall faster than this hurts itself.
    pub splat_speed: f32,
    pub spit_damage: f32,
    pub swing_damage: f32,
    /// A skitter's pounce, to anything it lands on other than the player.
    pub pounce_damage: f32,
    /// A brute's slam, to other enemies.
    pub quake_damage: f32,
}

impl Default for CombatParams {
    fn default() -> Self {
        Self {
            max_health: 5,
            hit_reach: 0.12,
            lunge_damage: 25.0,
            slam_damage: 40.0,
            shockwave_damage: 20.0,
            shockwave_radius: 2.5,
            pogo_damage: 15.0,
            impact_speed: 10.0,
            impact_damage_per_speed: 3.0,
            impact_min_damage: 8.0,
            knockback: 8.0,
            pogo_speed: 14.0,
            hitstop: 0.05,
            kill_hitstop: 0.09,
            hit_cooldown: 0.25,
            hurt_invulnerability: 1.0,
            hurt_knockback: Vec2::new(6.0, 10.0),
            pickup_radius: 0.4,
            throw_speed: 13.0,
            thrown_damage: 10.0,
            thrown_damage_per_speed: 2.0,
            splat_speed: 8.0,
            spit_damage: 15.0,
            swing_damage: 25.0,
            pounce_damage: 15.0,
            quake_damage: 35.0,
        }
    }
}

/// Runs after the player's move each tick: enemy physics, hits, hazards, pickups.
pub(crate) fn resolve(sim: &mut Sim, intent: Intent, events: &mut Vec<Event>) {
    let modes = intent.modes;
    carry(sim, intent, events);
    let slams: Vec<Vec2> = events
        .iter()
        .filter_map(|e| match e {
            Event::Slammed { at, .. } => Some(*at),
            _ => None,
        })
        .collect();

    sim.noises = noises(sim, modes, events);
    perceive_and_think(sim);
    for enemy in &mut sim.enemies {
        enemy.step(&sim.level, DT);
    }
    enemy_attacks(sim, events);
    spawn_projectiles(sim);
    move_projectiles(sim, events);
    thrown_bodies(sim, events);
    eating(sim, events);
    for i in 0..sim.enemies.len() {
        player_hits(sim, i, modes, events);
    }
    for at in slams {
        shockwave(sim, at, events);
    }
    for i in 0..sim.enemies.len() {
        push_blob_out(sim, i);
    }
    hazards(sim, events);
    pickups(sim, events);
}

fn player_hits(sim: &mut Sim, i: usize, modes: TarModes, events: &mut Vec<Event>) {
    let p = &sim.combat;
    let enemy = &sim.enemies[i];
    if !enemy.alive() || enemy.hit_cooldown > 0.0 || enemy.held || enemy.thrown {
        return;
    }
    let reach = enemy.radius + sim.blob.params.point_radius + p.hit_reach;
    let touching = sim
        .blob
        .positions
        .iter()
        .any(|q| q.distance(enemy.position) < reach);
    if !touching {
        return;
    }
    let center = sim.blob.center();
    let direction = (enemy.position - center).normalize_or(Vec2::X);
    // Only the player's own speed counts: an enemy running into a still
    // blob doesn't hurt itself.
    let closing = sim.blob.velocity().dot(direction) - enemy.velocity.dot(direction).max(0.0);
    let from_above = direction.y < -0.5;

    let kind = if sim.controller.is_slamming() && from_above {
        HitKind::Slam
    } else if sim.controller.is_lunging() && enemy.can_evade() {
        // A skitter sees lunges coming and hops aside; the lunge carries on.
        let at = enemy.position;
        sim.enemies[i].evade(center);
        events.push(Event::Evaded { enemy: i, at });
        return;
    } else if sim.controller.is_lunging() {
        // What the player holds when the lunge arrives decides what it does.
        if modes.sticky && sim.held.is_none() && enemy.grabbable(direction) {
            grab(sim, i, events);
            return;
        } else if sim.controller.jump_is_held() && !modes.sticky {
            HitKind::Pogo
        } else {
            HitKind::Lunge
        }
    } else if modes.bouncy && from_above && closing > 2.0 {
        HitKind::Pogo
    } else if closing > p.impact_speed {
        HitKind::Impact
    } else {
        return;
    };
    let damage = match kind {
        HitKind::Lunge => p.lunge_damage * (1.0 + sim.controller.lunge_power()),
        HitKind::Slam => p.slam_damage,
        HitKind::Pogo => p.pogo_damage,
        HitKind::Impact => {
            p.impact_min_damage + (closing - p.impact_speed) * p.impact_damage_per_speed
        }
        HitKind::Shockwave => p.shockwave_damage,
    };

    sim.controller.cancel_moves(&mut sim.blob);
    let knockback = (direction + Vec2::Y * 0.4).normalize() * p.knockback;
    let p = sim.combat.clone();
    if apply_hit(sim, i, kind, direction, damage, knockback, events) {
        // Blocked: bounced back, and no free lunge.
        sim.blob.set_velocity(-direction * 8.0 + Vec2::Y * 4.0);
        return;
    }

    // The player rebounds: up off the top of things, back off the sides.
    let v = sim.blob.velocity();
    let rebound = match kind {
        HitKind::Slam | HitKind::Pogo => {
            let up = if modes.bouncy {
                p.pogo_speed * 1.25
            } else {
                p.pogo_speed
            };
            Vec2::new(v.x * 0.5 - direction.x * 4.0, up)
        }
        HitKind::Lunge => -direction * 4.0 + Vec2::Y * 6.0,
        _ => v - direction * closing * 1.3,
    };
    sim.blob.set_velocity(rebound);
    sim.controller.refresh_lunge();
}

fn shockwave(sim: &mut Sim, at: Vec2, events: &mut Vec<Event>) {
    for i in 0..sim.enemies.len() {
        let enemy = &sim.enemies[i];
        let offset = enemy.position - at;
        if !enemy.alive()
            || enemy.held
            || enemy.hit_cooldown > 0.0
            || offset.length() > sim.combat.shockwave_radius + enemy.radius
        {
            continue;
        }
        let side = if offset.x.abs() > 0.05 {
            offset.x.signum()
        } else {
            1.0
        };
        let knockback = Vec2::new(side * 0.6, 1.0).normalize() * sim.combat.knockback;
        let damage = sim.combat.shockwave_damage;
        let direction = offset.normalize_or(Vec2::Y);
        apply_hit(
            sim,
            i,
            HitKind::Shockwave,
            direction,
            damage,
            knockback,
            events,
        );
    }
}

/// Hits enemy `i`. Returns true if its shield blocked the hit.
fn apply_hit(
    sim: &mut Sim,
    i: usize,
    kind: HitKind,
    direction: Vec2,
    damage: f32,
    knockback: Vec2,
    events: &mut Vec<Event>,
) -> bool {
    let enemy = &mut sim.enemies[i];
    enemy.hit_cooldown = sim.combat.hit_cooldown;
    let at = enemy.position;
    let (damage, killed) = match enemy.take_hit(kind, direction, damage, knockback) {
        HitOutcome::Blocked => {
            events.push(Event::Blocked { enemy: i, at });
            sim.freeze(sim.combat.hitstop);
            return true;
        }
        HitOutcome::Hurt { damage, killed } => (damage, killed),
    };
    events.push(Event::Hit {
        enemy: i,
        kind,
        damage,
        at,
    });
    if killed {
        events.push(Event::Killed { enemy: i, at });
        sim.pickups.push(at);
        sim.freeze(sim.combat.kill_hitstop);
    } else {
        sim.freeze(sim.combat.hitstop);
    }
    false
}

/// How far each kind of player action can be heard. Sticky movement makes
/// no noise at all, so crawling is the way to sneak up on things.
fn noises(sim: &Sim, modes: TarModes, events: &[Event]) -> Vec<(Vec2, f32)> {
    let at = sim.blob.center();
    let mut out: Vec<(Vec2, f32)> = events
        .iter()
        .filter_map(|e| {
            let radius = match e {
                Event::Jumped { .. } => 3.0,
                Event::Lunged { .. } => 4.0,
                Event::Launched { .. } => 5.0,
                Event::Bounced { .. } => 7.0,
                Event::Hit { .. } | Event::Blocked { .. } | Event::Struck { .. } => 8.0,
                Event::Thrown { .. } => 5.0,
                Event::Slammed { .. } => 10.0,
                _ => return None,
            };
            Some((at, radius))
        })
        .collect();
    let rolling_fast = sim.blob.touching_count() > 0 && sim.blob.velocity().length() > 5.0;
    if rolling_fast && !modes.sticky {
        out.push((at, 3.0));
    }
    out
}

fn perceive_and_think(sim: &mut Sim) {
    let player = sim.blob.center();
    // Where everyone is, before anyone moves.
    let bodies: Vec<(EnemyKind, Vec2, bool)> = sim
        .enemies
        .iter()
        .map(|e| (e.kind, e.position, e.alive() && !e.held))
        .collect();
    for (i, enemy) in sim.enemies.iter_mut().enumerate() {
        if !enemy.alive() {
            continue;
        }
        let closest = |wanted: &dyn Fn(EnemyKind, Vec2) -> bool| {
            bodies
                .iter()
                .enumerate()
                .filter(|(j, (kind, at, ok))| *j != i && *ok && wanted(*kind, *at))
                .map(|(_, (_, at, _))| *at)
                .min_by(|a, b| {
                    a.distance(enemy.position)
                        .total_cmp(&b.distance(enemy.position))
                })
        };
        let prey = closest(&|kind, at| {
            let offset = at - enemy.position;
            enemy.kind.hunts(kind)
                && offset.x * enemy.facing > -0.3
                && offset.length() < enemy.kind.sight_range()
                && offset.y.abs() < 2.5
                && line_of_sight(&sim.level, enemy.position, at)
        });
        let threat = closest(&|kind, at| {
            enemy.kind.fears(kind) && at.distance(enemy.position) < FEAR_DISTANCE
        });
        let offset = player - enemy.position;
        let in_front = offset.x * enemy.facing > -0.3;
        let sees_player = !sim.dead
            && in_front
            && offset.length() < enemy.kind.sight_range()
            && offset.y.abs() < 2.5
            && line_of_sight(&sim.level, enemy.position, player);
        let heard = sim
            .noises
            .iter()
            .filter(|(at, radius)| at.distance(enemy.position) <= *radius)
            .map(|(at, _)| *at)
            .min_by(|a, b| {
                a.distance(enemy.position)
                    .total_cmp(&b.distance(enemy.position))
            });
        let senses = Senses {
            player,
            sees_player,
            heard,
            prey,
            threat,
        };
        enemy.think(senses, &sim.level, DT);
    }
}

/// Attacks that are out this tick: swings, pounces and quakes.
fn enemy_attacks(sim: &mut Sim, events: &mut Vec<Event>) {
    for i in 0..sim.enemies.len() {
        let enemy = &sim.enemies[i];
        if !enemy.alive() || !enemy.striking() {
            continue;
        }
        match enemy.kind {
            EnemyKind::Skitter => {
                pounce(sim, i, events);
                continue;
            }
            EnemyKind::Brute => {
                quake(sim, i, events);
                continue;
            }
            _ => {}
        }
        let center = enemy.position + Vec2::X * enemy.facing * GUARD_SWING_OFFSET;
        // The swing hits anything in it, friend or not.
        let facing = enemy.facing;
        for j in 0..sim.enemies.len() {
            let other = &sim.enemies[j];
            if j != i
                && other.alive()
                && other.hit_cooldown <= 0.0
                && other.position.distance(center) < GUARD_SWING_RADIUS + other.radius
            {
                let direction = Vec2::new(facing, 0.0);
                let knockback = Vec2::new(facing * 6.0, 4.0);
                let damage = sim.combat.swing_damage;
                strike(sim, j, direction, damage, knockback, events);
            }
        }
        if sim.enemies[i].struck {
            continue;
        }
        let reach = GUARD_SWING_RADIUS + sim.blob.params.point_radius;
        let hit = sim
            .blob
            .positions
            .iter()
            .any(|q| q.distance(center) < reach);
        if hit && sim.player_shrunk() {
            sim.enemies[i].struck = true;
            events.push(Event::Dodged {
                at: sim.blob.center(),
            });
        } else if hit && !sim.player_invulnerable() {
            sim.enemies[i].struck = true;
            let away = sim.enemies[i].facing;
            hurt_player(sim, away, events);
        }
    }
}

/// A pouncing skitter bites whatever it lands on. Shrinking doesn't help:
/// it bites low.
fn pounce(sim: &mut Sim, i: usize, events: &mut Vec<Event>) {
    let (at, radius, facing) = {
        let e = &sim.enemies[i];
        (e.position, e.radius, e.facing)
    };
    for j in 0..sim.enemies.len() {
        let other = &sim.enemies[j];
        if j != i
            && other.alive()
            && other.hit_cooldown <= 0.0
            && other.position.distance(at) < radius + other.radius + 0.1
        {
            let direction = (other.position - at).normalize_or(Vec2::X * facing);
            let damage = sim.combat.pounce_damage;
            strike(
                sim,
                j,
                direction,
                damage,
                direction * 4.0 + Vec2::Y * 2.0,
                events,
            );
        }
    }
    if sim.enemies[i].struck {
        return;
    }
    let reach = radius + sim.blob.params.point_radius + sim.combat.hit_reach;
    let hit = sim.blob.positions.iter().any(|q| q.distance(at) < reach);
    if hit && !sim.player_invulnerable() {
        sim.enemies[i].struck = true;
        hurt_player(sim, facing, events);
    }
}

/// A brute's slam hits everything on the ground around it, once. Shrinking
/// doesn't dodge it; being in the air does.
fn quake(sim: &mut Sim, i: usize, events: &mut Vec<Event>) {
    if sim.enemies[i].struck {
        return;
    }
    sim.enemies[i].struck = true;
    let at = sim.enemies[i].position;
    events.push(Event::Quake { enemy: i, at });
    for j in 0..sim.enemies.len() {
        let other = &sim.enemies[j];
        if j != i
            && other.alive()
            && other.on_ground
            && other.position.distance(at) < BRUTE_SLAM_RADIUS + other.radius
        {
            let side = if other.position.x >= at.x { 1.0 } else { -1.0 };
            let damage = sim.combat.quake_damage;
            let knockback = Vec2::new(side * 4.0, 7.0);
            strike_as(
                sim,
                j,
                HitKind::Shockwave,
                Vec2::new(side, -0.5),
                damage,
                knockback,
                events,
            );
        }
    }
    let center = sim.blob.center();
    let grounded = sim.blob.touching_count() > 0;
    if grounded && center.distance(at) < BRUTE_SLAM_RADIUS + sim.blob.params.radius {
        let away = if center.x >= at.x { 1.0 } else { -1.0 };
        hurt_player(sim, away, events);
    }
}

/// A brute that touches a grub eats it.
fn eating(sim: &mut Sim, events: &mut Vec<Event>) {
    for i in 0..sim.enemies.len() {
        let brute = &sim.enemies[i];
        let busy = matches!(
            brute.state,
            State::Attacking(_) | State::Dizzy | State::Prone
        );
        if brute.kind != EnemyKind::Brute || !brute.alive() || brute.held || busy {
            continue;
        }
        let (at, reach) = (brute.position, brute.radius + 0.2);
        let meal = (0..sim.enemies.len()).find(|&j| {
            let e = &sim.enemies[j];
            e.kind == EnemyKind::Grub
                && e.alive()
                && !e.held
                && e.position.distance(at) < reach + e.radius
        });
        if let Some(j) = meal {
            sim.enemies[j].die();
            sim.enemies[i].eat();
            events.push(Event::Eaten { enemy: i, prey: j });
        }
    }
}

/// Enemies are solid to the blob: push any point inside one back out.
fn push_blob_out(sim: &mut Sim, i: usize) {
    let enemy = &sim.enemies[i];
    if !enemy.alive() || enemy.held || enemy.state == State::Dodging {
        return;
    }
    let reach = enemy.radius + sim.blob.params.point_radius;
    for (q, v) in sim
        .blob
        .positions
        .iter_mut()
        .zip(sim.blob.velocities.iter_mut())
    {
        let offset = *q - enemy.position;
        let distance = offset.length();
        if distance >= reach {
            continue;
        }
        let normal = offset.try_normalize().unwrap_or(Vec2::Y);
        *q = enemy.position + normal * reach;
        let into = (*v - enemy.velocity).dot(normal);
        if into < 0.0 {
            *v -= normal * into;
        }
    }
}

fn hazards(sim: &mut Sim, events: &mut Vec<Event>) {
    if sim.player_invulnerable() {
        return;
    }
    let radius = sim.blob.params.point_radius;
    let Some(hazard) = sim.hazards.iter().find(|h| {
        sim.blob
            .positions
            .iter()
            .any(|q| h.penetration(*q, radius).is_some())
    }) else {
        return;
    };
    let middle = hazard.vertices.iter().copied().sum::<Vec2>() / hazard.vertices.len() as f32;
    let away = if sim.blob.center().x >= middle.x {
        1.0
    } else {
        -1.0
    };
    hurt_player(sim, away, events);
}

/// Takes one health pip and knocks the player toward `away` (-1 or 1).
pub(crate) fn hurt_player(sim: &mut Sim, away: f32, events: &mut Vec<Event>) {
    if sim.player_invulnerable() || sim.dead {
        return;
    }
    sim.health = sim.health.saturating_sub(1);
    sim.hurt_left = sim.combat.hurt_invulnerability;
    let k = sim.combat.hurt_knockback;
    sim.controller.cancel_moves(&mut sim.blob);
    sim.blob.unstick();
    sim.blob.set_velocity(Vec2::new(k.x * away, k.y));
    events.push(Event::PlayerHurt { health: sim.health });
    if sim.health == 0 {
        sim.dead = true;
        events.push(Event::PlayerDied);
    }
}

fn pickups(sim: &mut Sim, events: &mut Vec<Event>) {
    if sim.health >= sim.max_health {
        return;
    }
    let center = sim.blob.center();
    let reach = sim.combat.pickup_radius + sim.blob.params.radius;
    if let Some(index) = sim.pickups.iter().position(|p| p.distance(center) < reach) {
        sim.pickups.remove(index);
        sim.health += 1;
        events.push(Event::Healed { health: sim.health });
    }
}

/// A glob of spit flying in a straight line.
#[derive(Clone, Debug, PartialEq)]
pub struct Projectile {
    pub position: Vec2,
    pub velocity: Vec2,
    /// Seconds left before it fizzles out.
    pub life: f32,
    /// Already passed through a shrunk player, so it doesn't count twice.
    pub dodged: bool,
    /// The enemy that fired it, which it can't hit.
    pub owner: usize,
}

pub const SPIT_SPEED: f32 = 11.0;
pub const SPIT_RADIUS: f32 = 0.15;
const SPIT_LIFE: f32 = 3.0;

fn spawn_projectiles(sim: &mut Sim) {
    for (owner, enemy) in sim.enemies.iter_mut().enumerate() {
        if let Some(direction) = enemy.fire.take() {
            sim.projectiles.push(Projectile {
                position: enemy.position + direction * (enemy.radius + SPIT_RADIUS + 0.05),
                velocity: direction * SPIT_SPEED,
                life: SPIT_LIFE,
                dodged: false,
                owner,
            });
        }
    }
}

fn move_projectiles(sim: &mut Sim, events: &mut Vec<Event>) {
    let mut i = 0;
    while i < sim.projectiles.len() {
        let shot = &mut sim.projectiles[i];
        shot.position += shot.velocity * DT;
        shot.life -= DT;
        let at = shot.position;
        let blocked = shot.life <= 0.0
            || sim
                .level
                .solids
                .iter()
                .any(|s| s.penetration(at, SPIT_RADIUS).is_some());
        if blocked {
            sim.projectiles.remove(i);
            continue;
        }
        // Spit hits other enemies too, including one the player is holding.
        let owner = sim.projectiles[i].owner;
        let victim = (0..sim.enemies.len()).find(|&j| {
            let e = &sim.enemies[j];
            j != owner && e.alive() && e.position.distance(at) < e.radius + SPIT_RADIUS
        });
        if let Some(j) = victim {
            let direction = sim.projectiles[i].velocity.normalize_or(Vec2::X);
            sim.projectiles.remove(i);
            let damage = sim.combat.spit_damage;
            strike(
                sim,
                j,
                direction,
                damage,
                direction * 4.0 + Vec2::Y * 2.0,
                events,
            );
            continue;
        }
        let reach = SPIT_RADIUS + sim.blob.params.point_radius;
        let touching = sim.blob.center().distance(at) < sim.blob.params.radius
            || sim.blob.positions.iter().any(|q| q.distance(at) < reach);
        if touching && sim.player_shrunk() {
            if !sim.projectiles[i].dodged {
                sim.projectiles[i].dodged = true;
                events.push(Event::Dodged { at });
            }
        } else if touching && !sim.player_invulnerable() && !sim.projectiles[i].dodged {
            let away = sim.projectiles[i].velocity.x.signum();
            sim.projectiles.remove(i);
            hurt_player(sim, away, events);
            continue;
        }
        i += 1;
    }
}

/// Latches onto enemy `i`: the lunge stops and the enemy is held.
fn grab(sim: &mut Sim, i: usize, events: &mut Vec<Event>) {
    sim.controller.cancel_moves(&mut sim.blob);
    let v = sim.blob.velocity();
    sim.blob.set_velocity(v * 0.2);
    sim.controller.refresh_lunge();
    let enemy = &mut sim.enemies[i];
    enemy.held = true;
    enemy.thrown = false;
    enemy.velocity = Vec2::ZERO;
    sim.held = Some(i);
    events.push(Event::Grabbed { enemy: i });
    sim.freeze(sim.combat.hitstop);
}

/// Keeps the held enemy in front of the blob, and drops it when Grip lets go.
fn carry(sim: &mut Sim, intent: Intent, events: &mut Vec<Event>) {
    let Some(i) = sim.held else {
        return;
    };
    let enemy = &mut sim.enemies[i];
    if !enemy.alive() {
        enemy.held = false;
        sim.held = None;
        return;
    }
    if !intent.modes.sticky {
        enemy.held = false;
        enemy.velocity = sim.blob.velocity();
        sim.held = None;
        events.push(Event::Dropped { enemy: i });
        return;
    }
    let reach = sim.blob.params.radius * sim.blob.scale + enemy.radius;
    enemy.position = sim.blob.center() + Vec2::new(sim.controller.facing() * reach * 0.8, 0.25);
    enemy.velocity = sim.blob.velocity();
}

/// Throws the held enemy along the aim (or forward and a little up), harder
/// with more shrink charge.
pub(crate) fn throw(sim: &mut Sim, aim: Vec2, events: &mut Vec<Event>) {
    let Some(i) = sim.held.take() else {
        return;
    };
    let facing = sim.controller.facing();
    let direction = aim
        .try_normalize()
        .filter(|_| aim.length() > 0.3)
        .unwrap_or(Vec2::new(facing, 0.4).normalize());
    let power = sim.controller.charge();
    sim.controller.spend_charge();
    let max = sim.controller.params.coil_max_power;
    let speed = sim.combat.throw_speed * (1.0 + (max - 1.0) * power);
    let reach = sim.blob.params.radius * sim.blob.scale;
    let enemy = &mut sim.enemies[i];
    enemy.held = false;
    enemy.thrown = true;
    enemy.hit_cooldown = 0.3;
    enemy.position = sim.blob.center() + direction * (reach + enemy.radius + 0.05);
    enemy.velocity = direction * speed;
    let v = sim.blob.velocity();
    sim.blob.set_velocity(v - direction * 2.0);
    events.push(Event::Thrown { enemy: i, speed });
}

/// A thrown enemy hurts the first other enemy it hits, and itself when it
/// hits a wall hard. It stops being a projectile once it slows down.
fn thrown_bodies(sim: &mut Sim, events: &mut Vec<Event>) {
    for i in 0..sim.enemies.len() {
        let body = &sim.enemies[i];
        if !body.alive() || !body.thrown {
            continue;
        }
        let speed = body.velocity.length();
        if body.impact > sim.combat.splat_speed {
            let damage = body.impact * sim.combat.thrown_damage_per_speed;
            let direction = -body.velocity.normalize_or(Vec2::Y);
            sim.enemies[i].thrown = false;
            strike(sim, i, direction, damage, Vec2::Y * 2.0, events);
            continue;
        }
        if speed < 4.0 && body.on_ground {
            sim.enemies[i].thrown = false;
            continue;
        }
        let (at, radius, velocity) = (body.position, body.radius, body.velocity);
        let victim = (0..sim.enemies.len()).find(|&j| {
            let e = &sim.enemies[j];
            j != i && e.alive() && !e.held && e.position.distance(at) < e.radius + radius
        });
        let Some(j) = victim else {
            continue;
        };
        let direction = (sim.enemies[j].position - at).normalize_or(velocity.normalize_or(Vec2::X));
        let damage = sim.combat.thrown_damage + speed * sim.combat.thrown_damage_per_speed;
        let knockback = direction * speed * 0.5 + Vec2::Y * 3.0;
        strike(sim, j, direction, damage, knockback, events);
        // The thrown body bounces off and takes half.
        let rebound = -direction * speed * 0.3 + Vec2::Y * 2.0;
        let body = &mut sim.enemies[i];
        body.thrown = false;
        body.velocity = rebound;
        strike(sim, i, -direction, damage * 0.5, rebound, events);
    }
}

/// Damage to enemy `j` from something other than the player.
fn strike(
    sim: &mut Sim,
    j: usize,
    direction: Vec2,
    damage: f32,
    knockback: Vec2,
    events: &mut Vec<Event>,
) {
    strike_as(
        sim,
        j,
        HitKind::Impact,
        direction,
        damage,
        knockback,
        events,
    );
}

fn strike_as(
    sim: &mut Sim,
    j: usize,
    kind: HitKind,
    direction: Vec2,
    damage: f32,
    knockback: Vec2,
    events: &mut Vec<Event>,
) {
    let enemy = &mut sim.enemies[j];
    enemy.hit_cooldown = sim.combat.hit_cooldown;
    let at = enemy.position;
    let knockback = if enemy.held { Vec2::ZERO } else { knockback };
    match enemy.take_hit(kind, direction, damage, knockback) {
        HitOutcome::Blocked => events.push(Event::Blocked { enemy: j, at }),
        HitOutcome::Hurt { damage, killed } => {
            events.push(Event::Struck {
                enemy: j,
                damage,
                at,
            });
            if killed {
                events.push(Event::Killed { enemy: j, at });
                sim.pickups.push(at);
                if sim.held == Some(j) {
                    sim.held = None;
                    sim.enemies[j].held = false;
                }
            }
            sim.freeze(sim.combat.hitstop * 0.6);
        }
    }
}
