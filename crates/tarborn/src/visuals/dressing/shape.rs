//! Pass 1, reading the shape: rasterise the solids, measure how deep each
//! point is, trace the outline that faces open air, and label each stretch
//! of it (floor, slope, wall, ceiling, concave or convex arc) and each corner.
//! It never looks at how the level was built, so slopes, arcs and joints
//! between solids all read the same way.

use bevy::prelude::*;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EdgeClass {
    Floor,
    Slope,
    Wall,
    Ceiling,
    ArcConcave,
    ArcConvex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CornerKind {
    /// The solid pokes out into the open.
    Outer,
    /// The open air pokes into the solid.
    Inner,
}

/// One labelled stretch of outline. Normals point out of the solid.
#[derive(Clone, Debug)]
pub struct Run {
    pub points: Vec<Vec2>,
    pub normals: Vec<Vec2>,
    pub class: EdgeClass,
    pub length: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Corner {
    pub at: Vec2,
    pub kind: CornerKind,
    /// Unit vector from the corner into the solid.
    pub inward: Vec2,
}

/// Numbers for reading the shape (from visuals.ron).
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize)]
pub struct ShapeParams {
    /// Raster cell size, world px.
    pub cell: f32,
    /// Outline smoothing passes (removes raster stairs on slopes).
    pub smoothing: usize,
    /// Normal y at or above which an edge is a floor (below: slope).
    pub floor_min_up: f32,
    /// Normal |y| below which an edge is a wall.
    pub wall_max_y: f32,
    /// Turning, in radians per px over `arc_window`, that makes an arc.
    pub arc_curvature: f32,
    pub arc_window: f32,
    /// Turning over `corner_window` px that makes a corner (radians).
    pub corner_turn: f32,
    pub corner_window: f32,
    /// Strips stop this far from a corner.
    pub corner_gap: f32,
    /// Radius of the circle probed to tell outer from inner corners.
    pub corner_probe: f32,
    /// Shortest run worth dressing.
    pub min_run: f32,
}

pub struct Shape {
    pub origin: Vec2,
    pub cell: f32,
    pub width: usize,
    pub height: usize,
    pub solid: Vec<bool>,
    /// Distance from open air, world px (0 in the open).
    pub depth: Vec<f32>,
    pub runs: Vec<Run>,
    pub corners: Vec<Corner>,
    /// Bounds of the open space.
    pub open_min: Vec2,
    pub open_max: Vec2,
}

fn inside_convex(poly: &[Vec2], p: Vec2) -> bool {
    (0..poly.len()).all(|i| {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        (b - a).perp_dot(p - a) >= 0.0
    })
}

impl Shape {
    /// Reads `solids` (convex, world px). Anything outside their bounds counts
    /// as solid, so the outer faces of the outer walls are never dressed.
    pub fn read(solids: &[Vec<Vec2>], params: &ShapeParams) -> Self {
        let cell = params.cell.max(0.5);
        let (lo, hi) = solids
            .iter()
            .flatten()
            .fold((Vec2::MAX, Vec2::MIN), |(lo, hi), v| {
                (lo.min(*v), hi.max(*v))
            });
        let origin = lo - Vec2::splat(cell * 2.0);
        let width = (((hi.x - lo.x) / cell).ceil() as usize + 4).max(4);
        let height = (((hi.y - lo.y) / cell).ceil() as usize + 4).max(4);
        let boxes: Vec<(Vec2, Vec2)> = solids
            .iter()
            .map(|s| {
                s.iter().fold((Vec2::MAX, Vec2::MIN), |(lo, hi), v| {
                    (lo.min(*v), hi.max(*v))
                })
            })
            .collect();
        let mut solid = vec![false; width * height];
        for y in 0..height {
            for x in 0..width {
                let p = origin + (Vec2::new(x as f32, y as f32) + 0.5) * cell;
                let outside = p.x < lo.x || p.y < lo.y || p.x > hi.x || p.y > hi.y;
                solid[y * width + x] = outside
                    || solids.iter().zip(&boxes).any(|(poly, (a, b))| {
                        p.x >= a.x
                            && p.y >= a.y
                            && p.x <= b.x
                            && p.y <= b.y
                            && inside_convex(poly, p)
                    });
            }
        }

        // Chamfer distance to the nearest open cell.
        let mut depth: Vec<f32> = solid
            .iter()
            .map(|&s| if s { f32::MAX } else { 0.0 })
            .collect();
        let (a, b) = (cell, cell * std::f32::consts::SQRT_2);
        for y in 0..height {
            for x in 0..width {
                let i = y * width + x;
                if !solid[i] {
                    continue;
                }
                let mut v = depth[i];
                if x > 0 {
                    v = v.min(depth[i - 1] + a);
                }
                if y > 0 {
                    v = v.min(depth[i - width] + a);
                    if x > 0 {
                        v = v.min(depth[i - width - 1] + b);
                    }
                    if x + 1 < width {
                        v = v.min(depth[i - width + 1] + b);
                    }
                }
                depth[i] = v;
            }
        }
        for y in (0..height).rev() {
            for x in (0..width).rev() {
                let i = y * width + x;
                if !solid[i] {
                    continue;
                }
                let mut v = depth[i];
                if x + 1 < width {
                    v = v.min(depth[i + 1] + a);
                }
                if y + 1 < height {
                    v = v.min(depth[i + width] + a);
                    if x + 1 < width {
                        v = v.min(depth[i + width + 1] + b);
                    }
                    if x > 0 {
                        v = v.min(depth[i + width - 1] + b);
                    }
                }
                depth[i] = v;
            }
        }
        // Measured from the boundary, not from the nearest open cell centre.
        for d in &mut depth {
            if *d > 0.0 {
                *d = if *d == f32::MAX { 1e6 } else { *d - cell * 0.5 };
            }
        }

        let (mut open_min, mut open_max) = (Vec2::MAX, Vec2::MIN);
        for y in 0..height {
            for x in 0..width {
                if !solid[y * width + x] {
                    let p = origin + (Vec2::new(x as f32, y as f32) + 0.5) * cell;
                    open_min = open_min.min(p);
                    open_max = open_max.max(p);
                }
            }
        }

        let mut shape = Self {
            origin,
            cell,
            width,
            height,
            solid,
            depth,
            runs: Vec::new(),
            corners: Vec::new(),
            open_min,
            open_max,
        };
        for contour in shape.contours(params.smoothing) {
            shape.label(contour, params);
        }
        shape
    }

    pub fn is_solid(&self, p: Vec2) -> bool {
        let q = (p - self.origin) / self.cell;
        if q.x < 0.0 || q.y < 0.0 {
            return true;
        }
        let (x, y) = (q.x as usize, q.y as usize);
        x >= self.width || y >= self.height || self.solid[y * self.width + x]
    }

    pub fn depth_at(&self, p: Vec2) -> f32 {
        let q = (p - self.origin) / self.cell;
        if q.x < 0.0 || q.y < 0.0 {
            return 1e6;
        }
        let (x, y) = (q.x as usize, q.y as usize);
        if x >= self.width || y >= self.height {
            return 1e6;
        }
        self.depth[y * self.width + x]
    }

    /// Closed outlines between solid and open cells (marching squares over
    /// cell centres), smoothed.
    fn contours(&self, smoothing: usize) -> Vec<Vec<Vec2>> {
        let (w, h) = (self.width, self.height);
        let s = |x: usize, y: usize| self.solid[y * w + x];
        // Edge keys: horizontal edge from (x,y) to (x+1,y) and vertical from (x,y) to (x,y+1).
        let hk = |x: usize, y: usize| (y * w + x) * 2;
        let vk = |x: usize, y: usize| (y * w + x) * 2 + 1;
        let mut segments: Vec<(usize, usize)> = Vec::new();
        for y in 0..h - 1 {
            for x in 0..w - 1 {
                let case = s(x, y) as u8
                    | (s(x + 1, y) as u8) << 1
                    | (s(x + 1, y + 1) as u8) << 2
                    | (s(x, y + 1) as u8) << 3;
                let (bottom, right, top, left) = (hk(x, y), vk(x + 1, y), hk(x, y + 1), vk(x, y));
                match case {
                    1 | 14 => segments.push((left, bottom)),
                    2 | 13 => segments.push((bottom, right)),
                    3 | 12 => segments.push((left, right)),
                    4 | 11 => segments.push((right, top)),
                    6 | 9 => segments.push((bottom, top)),
                    7 | 8 => segments.push((left, top)),
                    5 => {
                        segments.push((left, bottom));
                        segments.push((right, top));
                    }
                    10 => {
                        segments.push((bottom, right));
                        segments.push((left, top));
                    }
                    _ => {}
                }
            }
        }
        let point = |key: usize| {
            let i = key / 2;
            let (x, y) = ((i % w) as f32, (i / w) as f32);
            let c = if key.is_multiple_of(2) {
                Vec2::new(x + 1.0, y + 0.5)
            } else {
                Vec2::new(x + 0.5, y + 1.0)
            };
            self.origin + c * self.cell
        };
        let mut by_key: HashMap<usize, Vec<usize>> = HashMap::new();
        for (i, (a, b)) in segments.iter().enumerate() {
            by_key.entry(*a).or_default().push(i);
            by_key.entry(*b).or_default().push(i);
        }
        let mut used = vec![false; segments.len()];
        let mut loops = Vec::new();
        for start in 0..segments.len() {
            if used[start] {
                continue;
            }
            used[start] = true;
            let (first, mut key) = segments[start];
            let mut keys = vec![first];
            loop {
                keys.push(key);
                let next = by_key
                    .get(&key)
                    .and_then(|list| list.iter().copied().find(|&j| !used[j]));
                let Some(j) = next else { break };
                used[j] = true;
                let (a, b) = segments[j];
                key = if a == key { b } else { a };
                if key == first {
                    break;
                }
            }
            if keys.len() < 4 {
                continue;
            }
            let mut pts: Vec<Vec2> = keys.iter().map(|&k| point(k)).collect();
            let n = pts.len();
            for _ in 0..smoothing {
                pts = (0..n)
                    .map(|i| pts[(i + n - 1) % n] * 0.25 + pts[i] * 0.5 + pts[(i + 1) % n] * 0.25)
                    .collect();
            }
            loops.push(pts);
        }
        loops
    }

    /// Splits one closed outline into labelled runs and corners.
    fn label(&mut self, pts: Vec<Vec2>, p: &ShapeParams) {
        let n = pts.len();
        if n < 8 {
            return;
        }
        let step = {
            let total: f32 = (0..n).map(|i| pts[i].distance(pts[(i + 1) % n])).sum();
            (total / n as f32).max(1e-3)
        };
        let at = |i: isize| pts[i.rem_euclid(n as isize) as usize];
        let tangent = |i: isize| (at(i + 1) - at(i - 1)).normalize_or_zero();
        // Outward normals: perpendicular to the tangent, on the open side.
        let normals: Vec<Vec2> = (0..n as isize)
            .map(|i| {
                let t = tangent(i);
                let mut nrm = t.perp();
                if self.is_solid(at(i) + nrm * self.cell * 1.5)
                    && !self.is_solid(at(i) - nrm * self.cell * 1.5)
                {
                    nrm = -nrm;
                }
                nrm
            })
            .collect();
        let turn = |i: isize, k: isize| {
            let a = tangent(i - k);
            let b = tangent(i + k);
            a.perp_dot(b).atan2(a.dot(b))
        };
        let kc = ((p.corner_window / step) * 0.5).round().max(1.0) as isize;
        let ka = ((p.arc_window / step) * 0.5).round().max(2.0) as isize;

        // Corners: sharp turns. A square corner turns as much over a whole
        // window around it, so candidates are ranked by how sharp they are
        // right at the point too, and a corner keeps others out of its window.
        let sharp: Vec<f32> = (0..n as isize).map(|i| turn(i, kc).abs()).collect();
        let mut candidates: Vec<(f32, usize)> = (0..n)
            .filter(|&i| sharp[i] >= p.corner_turn)
            .map(|i| (sharp[i] + turn(i as isize, 1).abs(), i))
            .collect();
        candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut is_corner = vec![false; n];
        let mut taken: Vec<usize> = Vec::new();
        for (_, i) in candidates {
            let clear = taken.iter().all(|&j| {
                let d = i.abs_diff(j);
                d.min(n - d) as isize > 2 * kc
            });
            if clear {
                taken.push(i);
                is_corner[i] = true;
                let c = pts[i];
                let mut solid = 0;
                let mut inward = Vec2::ZERO;
                const SAMPLES: usize = 32;
                for k in 0..SAMPLES {
                    let d = Vec2::from_angle(k as f32 / SAMPLES as f32 * std::f32::consts::TAU);
                    if self.is_solid(c + d * p.corner_probe) {
                        solid += 1;
                        inward += d;
                    }
                }
                let f = solid as f32 / SAMPLES as f32;
                let kind = if f < 0.42 {
                    Some(CornerKind::Outer)
                } else if f > 0.58 {
                    Some(CornerKind::Inner)
                } else {
                    None
                };
                if let Some(kind) = kind {
                    self.corners.push(Corner {
                        at: c,
                        kind,
                        inward: inward.normalize_or_zero(),
                    });
                }
            }
        }

        // Points near a corner are left bare; the rest get a class.
        let gap = (p.corner_gap / step).round() as isize;
        let mut class: Vec<Option<EdgeClass>> = vec![None; n];
        for i in 0..n {
            let near_corner =
                (-gap..=gap).any(|d| is_corner[(i as isize + d).rem_euclid(n as isize) as usize]);
            if near_corner {
                continue;
            }
            let curvature = turn(i as isize, ka) / (2.0 * ka as f32 * step);
            let arc_clear =
                (-ka..=ka).all(|d| !is_corner[(i as isize + d).rem_euclid(n as isize) as usize]);
            let nrm = normals[i];
            class[i] = Some(if arc_clear && curvature.abs() > p.arc_curvature {
                // Turning towards the open side means the open side is inside the curve.
                let toward_open =
                    (tangent(i as isize + ka) - tangent(i as isize - ka)).dot(nrm) > 0.0;
                if toward_open {
                    EdgeClass::ArcConcave
                } else {
                    EdgeClass::ArcConvex
                }
            } else if nrm.y >= p.floor_min_up {
                EdgeClass::Floor
            } else if nrm.y.abs() < p.wall_max_y {
                EdgeClass::Wall
            } else if nrm.y > 0.0 {
                EdgeClass::Slope
            } else {
                EdgeClass::Ceiling
            });
        }

        // Cut into runs where the class changes, starting at a change so a
        // run never wraps around the loop's start.
        let start = (0..n).find(|&i| class[i] != class[(i + n - 1) % n]);
        let Some(start) = start else {
            if let Some(c) = class[0] {
                let mut pts2 = pts.clone();
                pts2.push(pts[0]);
                let mut nrm2 = normals.clone();
                nrm2.push(normals[0]);
                self.push_run(pts2, nrm2, c, p.min_run);
            }
            return;
        };
        let mut i = 0;
        while i < n {
            let j0 = (start + i) % n;
            let c = class[j0];
            let mut run_pts = vec![pts[j0]];
            let mut run_nrm = vec![normals[j0]];
            i += 1;
            while i < n && class[(start + i) % n] == c {
                let j = (start + i) % n;
                run_pts.push(pts[j]);
                run_nrm.push(normals[j]);
                i += 1;
            }
            if let Some(c) = c {
                self.push_run(run_pts, run_nrm, c, p.min_run);
            }
        }
    }

    fn push_run(&mut self, points: Vec<Vec2>, normals: Vec<Vec2>, class: EdgeClass, min: f32) {
        let length: f32 = points.windows(2).map(|w| w[0].distance(w[1])).sum();
        if length >= min && points.len() >= 2 {
            self.runs.push(Run {
                points,
                normals,
                class,
                length,
            });
        }
    }
}

impl Run {
    /// The point and outward normal `s` px along the run.
    pub fn at(&self, s: f32) -> (Vec2, Vec2) {
        let mut acc = 0.0;
        for k in 1..self.points.len() {
            let (a, b) = (self.points[k - 1], self.points[k]);
            let len = a.distance(b);
            if acc + len >= s && len > 0.0 {
                let t = (s - acc) / len;
                let n = self.normals[k - 1]
                    .lerp(self.normals[k], t)
                    .normalize_or_zero();
                return (a.lerp(b, t), n);
            }
            acc += len;
        }
        (*self.points.last().unwrap(), *self.normals.last().unwrap())
    }

    /// Calls `f(point, normal, s)` every `spacing` px, starting half a step in.
    pub fn walk(&self, spacing: f32, mut f: impl FnMut(Vec2, Vec2, f32)) {
        let spacing = spacing.max(0.5);
        let mut s = spacing * 0.5;
        while s < self.length {
            let (p, n) = self.at(s);
            f(p, n, s);
            s += spacing;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> ShapeParams {
        ShapeParams {
            cell: 2.0,
            smoothing: 3,
            floor_min_up: 0.97,
            wall_max_y: 0.7,
            arc_curvature: 0.006,
            arc_window: 40.0,
            corner_turn: 0.6,
            corner_window: 10.0,
            corner_gap: 6.0,
            corner_probe: 10.0,
            min_run: 8.0,
        }
    }

    fn rect(a: Vec2, b: Vec2) -> Vec<Vec2> {
        vec![a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)]
    }

    /// A box room with a floor, two walls, a ceiling and a ledge.
    fn room() -> Vec<Vec<Vec2>> {
        let v = Vec2::new;
        vec![
            rect(v(0.0, 0.0), v(400.0, 20.0)),
            rect(v(0.0, 0.0), v(20.0, 300.0)),
            rect(v(380.0, 0.0), v(400.0, 300.0)),
            rect(v(0.0, 280.0), v(400.0, 300.0)),
            rect(v(150.0, 120.0), v(250.0, 140.0)),
        ]
    }

    #[test]
    fn a_box_room_has_every_straight_class_and_both_corner_kinds() {
        let shape = Shape::read(&room(), &params());
        let has = |c| shape.runs.iter().any(|r| r.class == c);
        assert!(has(EdgeClass::Floor));
        assert!(has(EdgeClass::Wall));
        assert!(has(EdgeClass::Ceiling));
        assert!(!has(EdgeClass::ArcConcave) && !has(EdgeClass::ArcConvex));
        let inner = shape
            .corners
            .iter()
            .filter(|c| c.kind == CornerKind::Inner)
            .count();
        let outer = shape
            .corners
            .iter()
            .filter(|c| c.kind == CornerKind::Outer)
            .count();
        assert_eq!(inner, 4, "the room's four corners");
        assert_eq!(outer, 4, "the ledge's four corners");
    }

    #[test]
    fn floor_normals_point_up_into_the_room() {
        let shape = Shape::read(&room(), &params());
        let floor = shape
            .runs
            .iter()
            .find(|r| r.class == EdgeClass::Floor && r.points[0].y < 50.0)
            .unwrap();
        assert!(floor.normals.iter().all(|n| n.y > 0.9));
    }

    #[test]
    fn a_dome_reads_as_a_convex_arc() {
        let mut solids = room();
        let dome: Vec<Vec2> = (0..=16)
            .map(|i| {
                Vec2::new(100.0, 20.0)
                    + Vec2::from_angle(i as f32 / 16.0 * std::f32::consts::PI) * 50.0
            })
            .collect();
        solids.push(dome);
        let shape = Shape::read(&solids, &params());
        assert!(
            shape
                .runs
                .iter()
                .any(|r| r.class == EdgeClass::ArcConvex && r.length > 60.0)
        );
    }

    #[test]
    fn depth_grows_into_solids() {
        let shape = Shape::read(&room(), &params());
        assert!(shape.depth_at(Vec2::new(200.0, 200.0)) == 0.0);
        let near = shape.depth_at(Vec2::new(200.0, 17.0));
        let deep = shape.depth_at(Vec2::new(200.0, 5.0));
        assert!(near < deep, "{near} < {deep}");
    }
}
