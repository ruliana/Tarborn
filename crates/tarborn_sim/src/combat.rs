//! How the blob hurts things and gets hurt.
//!
//! Damage comes from the body: lunges, slams, pogo bounces and fast impacts.
//! Every hit freezes the world for a moment (hitstop), knocks the target back,
//! and refreshes the player's lunge so hits can chain.

use glam::Vec2;

use crate::controller::{Event, HitKind};
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
        }
    }
}

/// Runs after the player's move each tick: enemy physics, hits, hazards, pickups.
pub(crate) fn resolve(sim: &mut Sim, modes: TarModes, events: &mut Vec<Event>) {
    let slams: Vec<Vec2> = events
        .iter()
        .filter_map(|e| match e {
            Event::Slammed { at, .. } => Some(*at),
            _ => None,
        })
        .collect();

    for enemy in &mut sim.enemies {
        enemy.step(&sim.level, DT);
    }
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
    if !enemy.alive() || enemy.hit_cooldown > 0.0 {
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
    let closing = (sim.blob.velocity() - enemy.velocity).dot(direction);
    let from_above = direction.y < -0.5;

    let kind = if sim.controller.is_slamming() && from_above {
        HitKind::Slam
    } else if sim.controller.is_lunging() {
        HitKind::Lunge
    } else if modes.bouncy && from_above && closing > 2.0 {
        HitKind::Pogo
    } else if closing > p.impact_speed {
        HitKind::Impact
    } else {
        return;
    };
    let damage = match kind {
        HitKind::Lunge => p.lunge_damage,
        HitKind::Slam => p.slam_damage,
        HitKind::Pogo => p.pogo_damage,
        HitKind::Impact => {
            p.impact_min_damage + (closing - p.impact_speed) * p.impact_damage_per_speed
        }
        HitKind::Shockwave => p.shockwave_damage,
    };

    // The player rebounds: up off the top of things, back off the sides.
    let v = sim.blob.velocity();
    let rebound = match kind {
        HitKind::Slam | HitKind::Pogo => {
            let up = if modes.bouncy {
                p.pogo_speed * 1.25
            } else {
                p.pogo_speed
            };
            Vec2::new(v.x * 0.5, up)
        }
        HitKind::Lunge => -direction * 4.0 + Vec2::Y * 6.0,
        _ => v - direction * closing * 1.3,
    };
    sim.controller.cancel_moves(&mut sim.blob);
    sim.blob.set_velocity(rebound);
    sim.controller.refresh_lunge();

    let knockback = (direction + Vec2::Y * 0.4).normalize() * p.knockback;
    apply_hit(sim, i, kind, damage, knockback, events);
}

fn shockwave(sim: &mut Sim, at: Vec2, events: &mut Vec<Event>) {
    for i in 0..sim.enemies.len() {
        let enemy = &sim.enemies[i];
        let offset = enemy.position - at;
        if !enemy.alive()
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
        apply_hit(sim, i, HitKind::Shockwave, damage, knockback, events);
    }
}

fn apply_hit(
    sim: &mut Sim,
    i: usize,
    kind: HitKind,
    damage: f32,
    knockback: Vec2,
    events: &mut Vec<Event>,
) {
    let enemy = &mut sim.enemies[i];
    enemy.hit_cooldown = sim.combat.hit_cooldown;
    let at = enemy.position;
    let killed = enemy.hurt(damage, knockback);
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
}

/// Enemies are solid to the blob: push any point inside one back out.
fn push_blob_out(sim: &mut Sim, i: usize) {
    let enemy = &sim.enemies[i];
    if !enemy.alive() {
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
