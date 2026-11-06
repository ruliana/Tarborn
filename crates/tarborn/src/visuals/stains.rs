//! Tar stains. Hits splat tar onto the enemy that was hit and onto the
//! nearest wall or floor, so a fight leaves a trail. Enemy stains stay until
//! the enemy dies; wall stains fade. Visual only for now.

use bevy::prelude::*;
use serde::Deserialize;
use tarborn_sim::{Event, Sim};

use super::face::{Pose, hash};
use super::fill::Fill;
use super::tuning::{Rgb, srgb};
use crate::PIXELS_PER_METER;

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct StainLook {
    pub color: Rgb,
    /// A glossy dot on each splat, mixed this much toward `shine`.
    pub shine: Rgb,
    pub shine_mix: f32,
    /// Splat radius on an enemy, as a fraction of its radius: a base, more
    /// per point of damage, and a cap.
    pub enemy_size: f32,
    pub enemy_per_damage: f32,
    pub enemy_max_size: f32,
    /// Splat centres stay this close to the enemy's centre (fraction of radius).
    pub enemy_reach: f32,
    /// Oldest splats go once an enemy has this many.
    pub enemy_max: usize,
    /// Small drops around each splat, and their size relative to it.
    pub droplets: u32,
    pub droplet_size: f32,
    /// Walls closer than this to a hit get splatted (metres).
    pub wall_reach: f32,
    /// Wall splat length (px): base, per point of damage, cap.
    pub wall_size: f32,
    pub wall_per_damage: f32,
    pub wall_max_size: f32,
    /// Thickness of a wall splat relative to its length.
    pub wall_flat: f32,
    pub wall_max: usize,
    /// Seconds a wall splat stays fully dark, then seconds to fade out.
    pub wall_hold: f32,
    pub wall_fade: f32,
    /// Wall splats thrown around when an enemy dies.
    pub kill_burst: u32,
    /// A slam's floor splat, px.
    pub slam_size: f32,
}

#[derive(Clone, Copy, Debug)]
struct Splat {
    /// Offset from the enemy's centre in radii, x toward its facing side.
    local: Vec2,
    /// Radius in radii.
    size: f32,
    seed: f32,
}

#[derive(Clone, Copy, Debug)]
struct WallSplat {
    at: Vec2,
    tangent: Vec2,
    size: f32,
    born: f32,
    seed: f32,
}

#[derive(Resource, Default)]
pub struct Stains {
    enemies: Vec<Vec<Splat>>,
    walls: Vec<WallSplat>,
    last_tick: u64,
    count: u32,
}

impl Stains {
    /// Turns this frame's events into splats.
    pub fn observe(&mut self, sim: &Sim, events: &[Event], now: f32, look: &StainLook) {
        // A restart (or a new room) wipes the slate.
        if sim.tick < self.last_tick {
            self.enemies.clear();
            self.walls.clear();
        }
        self.last_tick = sim.tick;
        self.enemies.resize(sim.enemies.len(), Vec::new());
        for (i, enemy) in sim.enemies.iter().enumerate() {
            if !enemy.alive() {
                self.enemies[i].clear();
            }
        }
        // Enemies tar hit this step: only their deaths splatter.
        let mut tarred = Vec::new();
        for event in events {
            match *event {
                // The blob's own hits, and hits by a body it threw (coated in
                // tar). Other enemies' swings leave nothing.
                Event::Hit {
                    enemy, damage, at, ..
                }
                | Event::Struck { enemy, damage, at }
                    if matches!(event, Event::Hit { .. })
                        || thrown_body_near(sim, at).is_some() =>
                {
                    tarred.push(enemy);
                    // The splat lands on the side the tar came from.
                    let from = match event {
                        Event::Hit { .. } => sim.blob.center(),
                        _ => thrown_body_near(sim, at).unwrap_or(at),
                    };
                    self.splat_enemy(sim, enemy, from, damage, look);
                    let size =
                        (look.wall_size + look.wall_per_damage * damage).min(look.wall_max_size);
                    self.splat_wall(sim, at, size, now, look);
                }
                Event::Killed { enemy, at } if tarred.contains(&enemy) => {
                    for k in 0..look.kill_burst {
                        let dir = Vec2::from_angle(
                            hash(self.next_seed() + k as f32) * std::f32::consts::TAU,
                        );
                        let probe = at + dir * look.wall_reach * 0.5;
                        self.splat_wall(sim, probe, look.wall_max_size, now, look);
                    }
                }
                Event::Slammed { at, .. } => {
                    self.splat_wall(sim, at, look.slam_size, now, look);
                }
                _ => {}
            }
        }
        // Old wall splats go once fully faded, or when there are too many.
        let life = look.wall_hold + look.wall_fade;
        self.walls.retain(|w| now - w.born < life);
        if self.walls.len() > look.wall_max {
            let extra = self.walls.len() - look.wall_max;
            self.walls.drain(..extra);
        }
    }

    fn next_seed(&mut self) -> f32 {
        self.count = self.count.wrapping_add(1);
        self.count as f32 * 0.618
    }

    fn splat_enemy(&mut self, sim: &Sim, index: usize, from: Vec2, damage: f32, look: &StainLook) {
        let (Some(enemy), Some(list)) = (sim.enemies.get(index), self.enemies.get_mut(index))
        else {
            return;
        };
        if !enemy.alive() || enemy.radius <= 0.0 {
            return;
        }
        self.count = self.count.wrapping_add(1);
        let seed = self.count as f32 * 0.618;
        // Somewhere on the near side, scattered a little.
        let toward = (from - enemy.position).normalize_or(Vec2::Y);
        let angle = (hash(seed) - 0.5) * 1.6;
        let mut local = Vec2::from_angle(angle).rotate(toward)
            * look.enemy_reach
            * (0.5 + 0.5 * hash(seed * 3.7));
        local.x *= enemy.facing;
        let size = (look.enemy_size + look.enemy_per_damage * damage).min(look.enemy_max_size);
        list.push(Splat { local, size, seed });
        if list.len() > look.enemy_max {
            list.remove(0);
        }
    }

    fn splat_wall(&mut self, sim: &Sim, at: Vec2, size: f32, now: f32, look: &StainLook) {
        let Some((point, normal)) = nearest_surface(sim, at, look.wall_reach) else {
            return;
        };
        let seed = self.next_seed();
        self.walls.push(WallSplat {
            at: point * PIXELS_PER_METER,
            tangent: normal.perp(),
            size,
            born: now,
            seed,
        });
    }

    /// Splats on enemy number `index`, posed with its body.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_enemy(
        &self,
        fill: &mut Fill,
        index: usize,
        center: Vec2,
        r: f32,
        facing: f32,
        pose: &Pose,
        look: &StainLook,
    ) {
        let Some(list) = self.enemies.get(index) else {
            return;
        };
        let tar = srgb(look.color);
        let shine = tar.mix(&srgb(look.shine), look.shine_mix);
        for s in list {
            let at = center + Vec2::new(s.local.x * facing, s.local.y) * r;
            let radius = s.size * r;
            fill.circle(pose.apply(at), radius, 12, tar);
            for k in 0..look.droplets {
                let angle = hash(s.seed + k as f32 * 3.1) * std::f32::consts::TAU;
                let reach = radius * (1.2 + 0.6 * hash(s.seed * 1.7 + k as f32));
                let drop = at + Vec2::from_angle(angle) * reach;
                fill.circle(pose.apply(drop), radius * look.droplet_size, 6, tar);
            }
            let glint = at + Vec2::new(-0.3, 0.35) * radius;
            fill.circle(pose.apply(glint), radius * 0.25, 6, shine);
        }
    }

    pub fn draw_walls(&self, fill: &mut Fill, now: f32, look: &StainLook) {
        let tar = srgb(look.color);
        let shine = srgb(look.shine);
        for w in &self.walls {
            let age = now - w.born;
            let alpha = if age < look.wall_hold {
                1.0
            } else {
                1.0 - ((age - look.wall_hold) / look.wall_fade.max(1e-3)).clamp(0.0, 1.0)
            };
            let color = tar.with_alpha(alpha);
            let angle = w.tangent.to_angle();
            let half = w.size * 0.5;
            fill.ellipse(
                w.at,
                Vec2::new(half, half * look.wall_flat),
                angle,
                14,
                color,
            );
            // Drops along the surface.
            for k in 0..3 {
                let side = if hash(w.seed + k as f32) > 0.5 {
                    1.0
                } else {
                    -1.0
                };
                let along = side * half * (1.1 + 0.6 * hash(w.seed * 2.3 + k as f32));
                let r = half * look.wall_flat * (0.4 + 0.4 * hash(w.seed * 3.1 + k as f32));
                fill.circle(w.at + w.tangent * along, r, 8, color);
            }
            let glint = w.at + Vec2::new(-0.2, 0.3) * half * look.wall_flat;
            fill.circle(
                glint,
                half * look.wall_flat * 0.25,
                6,
                tar.mix(&shine, look.shine_mix).with_alpha(alpha),
            );
        }
    }
}

/// Where a body the blob threw (coated in tar) is, if one is near `at`.
fn thrown_body_near(sim: &Sim, at: Vec2) -> Option<Vec2> {
    const NEAR: f32 = 1.5;
    sim.enemies
        .iter()
        .find(|e| e.thrown && e.position.distance(at) < NEAR + e.radius)
        .map(|e| e.position)
}

/// The closest point on any solid's outline within `reach` of `at`, and the
/// direction from it toward `at` (metres).
fn nearest_surface(sim: &Sim, at: Vec2, reach: f32) -> Option<(Vec2, Vec2)> {
    let mut best: Option<(f32, Vec2)> = None;
    for solid in &sim.level.solids {
        let v = &solid.vertices;
        for i in 0..v.len() {
            let (a, b) = (v[i], v[(i + 1) % v.len()]);
            let ab = b - a;
            let t = ((at - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
            let p = a + ab * t;
            let d = p.distance(at);
            if d <= reach && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, p));
            }
        }
    }
    let (_, point) = best?;
    let normal = (at - point).try_normalize().unwrap_or(Vec2::Y);
    Some((point, normal))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tarborn_sim::HitKind;

    fn look() -> StainLook {
        super::super::VisualTuning::default().stains
    }

    #[test]
    fn the_blob_s_hits_stain_the_enemy_and_the_floor() {
        let sim = Sim::training_room(Vec2::new(0.0, 2.0));
        let enemy = &sim.enemies[0];
        let mut stains = Stains::default();
        let hit = Event::Hit {
            enemy: 0,
            kind: HitKind::Lunge,
            damage: 25.0,
            at: enemy.position,
        };
        stains.observe(&sim, &[hit], 0.0, &look());
        assert_eq!(stains.enemies[0].len(), 1);
        // The guard stands on the floor, so the floor under it gets tar too.
        assert_eq!(stains.walls.len(), 1);
        // On the side the blob hit from, inside the body.
        let splat = stains.enemies[0][0];
        assert!(splat.local.length() <= look().enemy_reach + 1e-4);
    }

    #[test]
    fn other_enemies_swings_leave_no_tar() {
        let sim = Sim::training_room(Vec2::new(0.0, 2.0));
        let mut stains = Stains::default();
        let struck = Event::Struck {
            enemy: 0,
            damage: 25.0,
            at: sim.enemies[0].position,
        };
        stains.observe(&sim, &[struck], 0.0, &look());
        assert!(stains.enemies[0].is_empty());
        assert!(stains.walls.is_empty());
    }

    #[test]
    fn wall_stains_fade_away() {
        let sim = Sim::training_room(Vec2::new(0.0, 2.0));
        let look = look();
        let mut stains = Stains::default();
        let slam = Event::Slammed {
            at: Vec2::new(0.0, 0.1),
            speed: 10.0,
        };
        stains.observe(&sim, &[slam], 0.0, &look);
        assert_eq!(stains.walls.len(), 1);
        stains.observe(&sim, &[], look.wall_hold + look.wall_fade + 0.1, &look);
        assert!(stains.walls.is_empty());
    }
}
