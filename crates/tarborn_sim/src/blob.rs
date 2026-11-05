//! The tar blob: a ring of point masses held together by position-based
//! constraints (edge lengths, enclosed area and a weak pull toward its rest
//! shape), colliding with the level.
//!
//! The step is fully deterministic for a given fixed `dt`: no randomness, no
//! wall-clock time, no hash-map iteration order.

use glam::Vec2;

use crate::level::Level;

/// Which tar properties the player is holding. They can be combined.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TarModes {
    /// Grip any surface: walls, ceilings, and (later) objects.
    pub sticky: bool,
    /// No friction: slide down slopes and through gaps.
    pub slick: bool,
    /// Dense: falls harder, holds its shape, and (later) pushes things around.
    pub heavy: bool,
}

/// Tuning knobs. Units are metres, seconds and "per second squared".
#[derive(Clone, Debug, PartialEq)]
pub struct BlobParams {
    pub point_count: usize,
    pub radius: f32,
    /// Collision radius of each perimeter point.
    pub point_radius: f32,
    /// Extra distance within which a point still counts as touching a surface.
    pub contact_slop: f32,
    pub gravity: Vec2,
    pub heavy_gravity_scale: f32,
    pub solver_iterations: usize,
    /// 0..1 stiffness of the perimeter edges, per solver iteration.
    pub edge_stiffness: f32,
    /// 0..1 stiffness of the enclosed-area (pressure) constraint, per iteration.
    pub area_stiffness: f32,
    /// 0..1 pull toward the rest shape, per iteration. Low values feel gooey.
    pub shape_stiffness: f32,
    pub heavy_shape_stiffness: f32,
    /// Tangential acceleration that spins the blob, which rolls it on contact.
    pub roll_accel: f32,
    /// Direct horizontal acceleration while airborne.
    pub air_accel: f32,
    /// Fraction of a touching point's sliding removed per solver iteration.
    pub friction: f32,
    pub grip_friction: f32,
    pub slick_friction: f32,
    /// While sticky, a point that was touching is pulled back onto the surface
    /// if it has drifted less than this far off it.
    pub grip_range: f32,
    /// 0..1 fraction of that drift undone per solver iteration.
    pub grip_pull: f32,
    /// Solver inverse mass of a gripping point (1 is a normal point). Lower
    /// values let the body hang from its grip more firmly.
    pub grip_inv_mass: f32,
    /// How long grip is released after a jump, so sticky jumps can leave.
    pub unstick_time: f32,
    /// Fraction of velocity removed per second, everywhere.
    pub linear_damping: f32,
    pub max_point_speed: f32,
}

impl Default for BlobParams {
    fn default() -> Self {
        Self {
            point_count: 20,
            radius: 0.5,
            point_radius: 0.06,
            contact_slop: 0.03,
            gravity: Vec2::new(0.0, -30.0),
            heavy_gravity_scale: 2.2,
            solver_iterations: 8,
            edge_stiffness: 0.9,
            area_stiffness: 0.5,
            shape_stiffness: 0.03,
            heavy_shape_stiffness: 0.12,
            roll_accel: 120.0,
            air_accel: 14.0,
            friction: 0.3,
            grip_friction: 0.5,
            slick_friction: 0.0,
            grip_range: 0.15,
            grip_pull: 0.1,
            grip_inv_mass: 1.0,
            unstick_time: 0.15,
            linear_damping: 0.3,
            max_point_speed: 20.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Blob {
    pub params: BlobParams,
    pub positions: Vec<Vec2>,
    pub velocities: Vec<Vec2>,
    /// Surface normal under each point from the last step, if it was touching.
    pub contacts: Vec<Option<Vec2>>,
    rest_offsets: Vec<Vec2>,
    rest_edge: f32,
    rest_area: f32,
    scratch: Vec<Vec2>,
    inv_mass: Vec<f32>,
    held: Vec<Option<Vec2>>,
    unstick_left: f32,
}

impl Blob {
    pub fn new(center: Vec2, params: BlobParams) -> Self {
        let n = params.point_count;
        assert!(n >= 6, "a blob needs at least six points");
        let rest_offsets: Vec<Vec2> = (0..n)
            .map(|i| {
                let angle = i as f32 / n as f32 * std::f32::consts::TAU;
                Vec2::from_angle(angle) * params.radius
            })
            .collect();
        let rest_edge = rest_offsets[0].distance(rest_offsets[1]);
        let rest_area = polygon_area(&rest_offsets);
        Self {
            positions: rest_offsets.iter().map(|&o| center + o).collect(),
            velocities: vec![Vec2::ZERO; n],
            contacts: vec![None; n],
            rest_offsets,
            rest_edge,
            rest_area,
            scratch: vec![Vec2::ZERO; n],
            inv_mass: vec![1.0; n],
            held: vec![None; n],
            unstick_left: 0.0,
            params,
        }
    }

    pub fn center(&self) -> Vec2 {
        self.positions.iter().copied().sum::<Vec2>() / self.positions.len() as f32
    }

    pub fn velocity(&self) -> Vec2 {
        self.velocities.iter().copied().sum::<Vec2>() / self.velocities.len() as f32
    }

    pub fn area(&self) -> f32 {
        polygon_area(&self.positions)
    }

    pub fn rest_area(&self) -> f32 {
        self.rest_area
    }

    pub fn touching_count(&self) -> usize {
        self.contacts.iter().flatten().count()
    }

    /// The averaged normal of the surfaces the blob can jump off right now.
    /// Floors always count; walls and ceilings only while sticky.
    pub fn support_normal(&self, modes: TarModes) -> Option<Vec2> {
        let sum: Vec2 = self
            .contacts
            .iter()
            .flatten()
            .filter(|n| modes.sticky || n.y > 0.3)
            .copied()
            .sum();
        sum.try_normalize()
    }

    /// Adds `velocity` to every point and briefly releases any grip, e.g. for a jump.
    pub fn launch(&mut self, velocity: Vec2) {
        for v in &mut self.velocities {
            *v += velocity;
        }
        self.unstick_left = self.params.unstick_time;
    }

    /// Removes part of the upward velocity, for variable jump height.
    pub fn cut_rise(&mut self, keep: f32) {
        for v in &mut self.velocities {
            if v.y > 0.0 {
                v.y *= keep;
            }
        }
    }

    /// Advances the simulation by `dt` seconds. `roll` is -1 (left) to 1 (right).
    pub fn step(&mut self, level: &Level, dt: f32, roll: f32, modes: TarModes) {
        let p = &self.params;
        let n = self.positions.len();
        let roll = roll.clamp(-1.0, 1.0);
        let gravity = p.gravity
            * if modes.heavy {
                p.heavy_gravity_scale
            } else {
                1.0
            };
        let airborne = self.touching_count() == 0;
        let center = self.center();
        let damping = (1.0 - p.linear_damping * dt).max(0.0);
        self.unstick_left = (self.unstick_left - dt).max(0.0);
        let gripping = modes.sticky && self.unstick_left <= 0.0;

        // Points gripping a surface are "heavier" for the solver, so the rest of
        // the body hangs from them instead of dragging them along.
        for i in 0..n {
            self.held[i] = if gripping { self.contacts[i] } else { None };
            self.inv_mass[i] = if self.held[i].is_some() {
                p.grip_inv_mass
            } else {
                1.0
            };
        }

        // 1. Integrate forces and predict new positions.
        for i in 0..n {
            let mut accel = gravity;
            let radial = self.positions[i] - center;
            // Clockwise tangent: spinning clockwise rolls the blob to the right.
            let tangent = Vec2::new(radial.y, -radial.x).normalize_or_zero();
            accel += tangent * roll * p.roll_accel;
            if airborne {
                accel.x += roll * p.air_accel;
            }
            let v =
                ((self.velocities[i] + accel * dt) * damping).clamp_length_max(p.max_point_speed);
            self.velocities[i] = v;
            self.scratch[i] = self.positions[i] + v * dt;
        }

        // 2. Solve constraints on the predicted positions.
        let shape_k = if modes.heavy {
            p.heavy_shape_stiffness
        } else {
            p.shape_stiffness
        };
        let friction = if modes.slick {
            p.slick_friction
        } else if gripping {
            p.grip_friction
        } else {
            p.friction
        };
        let surface = Surface {
            radius: p.point_radius,
            slop: p.contact_slop,
            grip_range: p.grip_range,
            grip_pull: p.grip_pull,
            friction,
        };
        self.contacts.fill(None);
        for _ in 0..p.solver_iterations.max(1) {
            solve_edges(
                &mut self.scratch,
                &self.inv_mass,
                self.rest_edge,
                p.edge_stiffness,
            );
            solve_area(
                &mut self.scratch,
                &self.inv_mass,
                self.rest_area,
                p.area_stiffness,
            );
            solve_shape(
                &mut self.scratch,
                &self.inv_mass,
                &self.rest_offsets,
                shape_k,
            );
            surface.solve(
                &mut self.scratch,
                &self.positions,
                &self.held,
                &mut self.contacts,
                level,
            );
        }

        // 3. Derive velocities from the corrected positions.
        for i in 0..n {
            let mut v = (self.scratch[i] - self.positions[i]) / dt;
            if let Some(normal) = self.contacts[i] {
                // Remove any velocity into the surface. (Tar never bounces.)
                v -= normal * v.dot(normal).min(0.0);
            }
            self.velocities[i] = v;
            self.positions[i] = self.scratch[i];
        }
    }
}

fn polygon_area(points: &[Vec2]) -> f32 {
    let n = points.len();
    (0..n)
        .map(|i| points[i].perp_dot(points[(i + 1) % n]))
        .sum::<f32>()
        * 0.5
}

fn solve_edges(points: &mut [Vec2], inv_mass: &[f32], rest: f32, k: f32) {
    let n = points.len();
    for i in 0..n {
        let j = (i + 1) % n;
        let w = inv_mass[i] + inv_mass[j];
        let delta = points[j] - points[i];
        let len = delta.length();
        if len <= f32::EPSILON || w <= 0.0 {
            continue;
        }
        let correction = delta * ((len - rest) / len) * k / w;
        points[i] += correction * inv_mass[i];
        points[j] -= correction * inv_mass[j];
    }
}

/// Pushes every point along the area gradient so the enclosed area returns
/// to `rest`. This is the "pressure" that makes the blob a blob, not a rope.
fn solve_area(points: &mut [Vec2], inv_mass: &[f32], rest: f32, k: f32) {
    let n = points.len();
    let area = polygon_area(points);
    // dA/dp_i = ½ · perp(p_{i+1} − p_{i−1}). All gradients come from the same
    // configuration, so compute them before moving anything.
    let gradients: Vec<Vec2> = (0..n)
        .map(|i| {
            let prev = points[(i + n - 1) % n];
            let next = points[(i + 1) % n];
            Vec2::new(next.y - prev.y, prev.x - next.x) * 0.5
        })
        .collect();
    let denominator: f32 = gradients
        .iter()
        .zip(inv_mass)
        .map(|(g, w)| g.length_squared() * w)
        .sum();
    if denominator <= f32::EPSILON {
        return;
    }
    let lambda = (rest - area) / denominator * k;
    for ((point, g), w) in points.iter_mut().zip(gradients).zip(inv_mass) {
        *point += g * lambda * *w;
    }
}

/// Weak 2D shape matching: find the best-fit rotation of the rest shape onto
/// the current points and pull each point a little toward it. This keeps the
/// blob from folding inside out without making it rigid.
fn solve_shape(points: &mut [Vec2], inv_mass: &[f32], rest_offsets: &[Vec2], k: f32) {
    if k <= 0.0 {
        return;
    }
    let total: f32 = inv_mass.iter().map(|w| 1.0 / w).sum();
    let center = points
        .iter()
        .zip(inv_mass)
        .map(|(p, w)| *p / *w)
        .sum::<Vec2>()
        / total;
    let (mut sin, mut cos) = (0.0, 0.0);
    for (p, q) in points.iter().zip(rest_offsets) {
        let r = *p - center;
        sin += q.perp_dot(r);
        cos += q.dot(r);
    }
    let rotation = Vec2::new(cos, sin).normalize_or(Vec2::X);
    for ((p, q), w) in points.iter_mut().zip(rest_offsets).zip(inv_mass) {
        let goal = center + rotation.rotate(*q);
        *p += (goal - *p) * (k * w).min(1.0);
    }
}

/// Contact handling against the level: push-out, positional friction, and grip.
struct Surface {
    radius: f32,
    slop: f32,
    grip_range: f32,
    grip_pull: f32,
    friction: f32,
}

impl Surface {
    fn solve(
        &self,
        points: &mut [Vec2],
        start: &[Vec2],
        held: &[Option<Vec2>],
        contacts: &mut [Option<Vec2>],
        level: &Level,
    ) {
        let reach = self.radius + self.slop.max(self.grip_range);
        for i in 0..points.len() {
            for solid in &level.solids {
                let Some((normal, depth)) = solid.penetration(points[i], reach) else {
                    continue;
                };
                // depth = reach - distance, so `radius - distance` is:
                let overlap = depth - (reach - self.radius);
                let gripping = held[i].is_some();
                let touching = overlap > -self.slop;
                if !(touching || gripping) {
                    continue;
                }
                contacts[i] = Some(normal);
                if overlap > 0.0 {
                    points[i] += normal * overlap;
                } else if gripping {
                    // Tar is tacky, not glued: pull back part of the way so a
                    // strong enough roll can peel the trailing edge off.
                    points[i] += normal * overlap * self.grip_pull;
                }
                // Positional friction: undo part of this step's sliding along the surface.
                let moved = points[i] - start[i];
                let sliding = moved - normal * moved.dot(normal);
                points[i] -= sliding * self.friction;
            }
        }
    }
}
