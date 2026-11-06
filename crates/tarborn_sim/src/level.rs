//! Static level geometry: a set of convex polygons the blob collides with.

use glam::Vec2;

/// A convex, solid polygon. Vertices are counter-clockwise.
#[derive(Clone, Debug, PartialEq)]
pub struct Solid {
    pub vertices: Vec<Vec2>,
}

impl Solid {
    /// Builds a solid from counter-clockwise vertices.
    ///
    /// # Panics
    /// If there are fewer than three vertices or the winding is not counter-clockwise.
    pub fn new(vertices: Vec<Vec2>) -> Self {
        assert!(vertices.len() >= 3, "a solid needs at least three vertices");
        let solid = Self { vertices };
        assert!(
            solid.signed_area() > 0.0,
            "solid vertices must be counter-clockwise"
        );
        solid
    }

    /// Axis-aligned box from its minimum and maximum corners.
    pub fn rect(min: Vec2, max: Vec2) -> Self {
        Self::new(vec![
            min,
            Vec2::new(max.x, min.y),
            max,
            Vec2::new(min.x, max.y),
        ])
    }

    /// A convex solid from vertices in either winding.
    pub fn convex(mut vertices: Vec<Vec2>) -> Self {
        let area: f32 = (0..vertices.len())
            .map(|i| vertices[i].perp_dot(vertices[(i + 1) % vertices.len()]))
            .sum();
        if area < 0.0 {
            vertices.reverse();
        }
        Self::new(vertices)
    }

    fn signed_area(&self) -> f32 {
        let n = self.vertices.len();
        (0..n)
            .map(|i| self.vertices[i].perp_dot(self.vertices[(i + 1) % n]))
            .sum::<f32>()
            * 0.5
    }

    /// If `point` (a disc of `radius`) overlaps this solid, returns the outward
    /// surface normal and the penetration depth along it.
    ///
    /// Uses the separating axis of least penetration, which is exact for points
    /// that are inside the polygon or near a face. Near a convex corner it
    /// slightly over-pushes, which is harmless for a soft body.
    pub fn penetration(&self, point: Vec2, radius: f32) -> Option<(Vec2, f32)> {
        let n = self.vertices.len();
        let mut best: Option<(Vec2, f32)> = None;
        for i in 0..n {
            let a = self.vertices[i];
            let b = self.vertices[(i + 1) % n];
            // Outward normal of a CCW edge is the edge direction rotated clockwise.
            let edge = b - a;
            let normal = Vec2::new(edge.y, -edge.x).normalize();
            let distance = (point - a).dot(normal);
            if distance >= radius {
                return None; // Separating axis found: no overlap.
            }
            let depth = radius - distance;
            if best.is_none_or(|(_, d)| depth < d) {
                best = Some((normal, depth));
            }
        }
        best
    }
}

/// Points on a circle from angle `from` to `to` (radians), both included.
pub fn arc_points(center: Vec2, radius: f32, from: f32, to: f32, segments: usize) -> Vec<Vec2> {
    let segments = segments.max(1);
    (0..=segments)
        .map(|i| {
            let a = from + (to - from) * i as f32 / segments as f32;
            center + Vec2::from_angle(a) * radius
        })
        .collect()
}

/// A concave rounded corner: the solid between `corner` and a quarter arc
/// around `center`, built as a fan of triangles (solids must be convex).
pub fn fillet(
    corner: Vec2,
    center: Vec2,
    radius: f32,
    from: f32,
    to: f32,
    segments: usize,
) -> Vec<Solid> {
    let arc = arc_points(center, radius, from, to, segments);
    arc.windows(2)
        .map(|w| Solid::convex(vec![corner, w[0], w[1]]))
        .collect()
}

/// A bridge from `left` to `right` whose top is flat at `top` and whose
/// underside is an arch: `end_depth` thick at the ends, `crown_depth` in the
/// middle. The top is one slab so rolling across it meets no seams; the arch
/// below it is cut into convex slices.
pub fn arch(
    left: f32,
    right: f32,
    top: f32,
    end_depth: f32,
    crown_depth: f32,
    segments: usize,
) -> Vec<Solid> {
    let v = Vec2::new;
    let slab = crown_depth.min(end_depth);
    let mut solids = vec![Solid::rect(v(left, top - slab), v(right, top))];
    // Circle through both lower ends and the crown.
    let half = (right - left) * 0.5;
    let mid = left + half;
    let (end_y, crown_y) = (top - end_depth, top - crown_depth);
    let rise = crown_y - end_y;
    if rise <= 1e-4 {
        return solids;
    }
    let radius = (half * half + rise * rise) / (2.0 * rise);
    let center = v(mid, crown_y - radius);
    let half_angle = (half / radius).asin();
    let arc = arc_points(
        center,
        radius,
        std::f32::consts::FRAC_PI_2 + half_angle,
        std::f32::consts::FRAC_PI_2 - half_angle,
        segments,
    );
    let base = top - slab;
    for w in arc.windows(2) {
        let (a, b) = (w[0], w[1]);
        if base - a.y.max(b.y) > 1e-4 || base - a.y.min(b.y) > 1e-4 {
            solids.push(Solid::convex(vec![a, b, v(b.x, base), v(a.x, base)]));
        }
    }
    solids
}

/// Everything static the blob can touch.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Level {
    pub solids: Vec<Solid>,
}

impl Level {
    /// A small hand-built test room: floor, walls, a ceiling, a slope and a ledge.
    /// Units are metres; the blob is about one metre across.
    pub fn test_room() -> Self {
        let v = Vec2::new;
        Self {
            solids: vec![
                Solid::rect(v(-20.0, -2.0), v(20.0, 0.0)),   // floor
                Solid::rect(v(-21.0, -2.0), v(-20.0, 12.0)), // left wall
                Solid::rect(v(20.0, -2.0), v(21.0, 12.0)),   // right wall
                Solid::rect(v(-21.0, 12.0), v(21.0, 13.0)),  // ceiling
                Solid::new(vec![v(4.0, 0.0), v(12.0, 0.0), v(12.0, 4.0)]), // slope
                Solid::rect(v(12.0, 0.0), v(20.0, 4.0)),     // plateau after the slope
                Solid::rect(v(-12.0, 5.0), v(-6.0, 5.5)),    // floating ledge
                Solid::rect(v(-4.0, 0.0), v(-3.0, 8.0)),     // tall pillar to climb
            ],
        }
    }
}

impl Level {
    /// A room for trying the level dressing: a ramp, quarter-pipe corners, an
    /// arched bridge, a dome and a ledge. Units are metres.
    pub fn dressing_room() -> Self {
        use std::f32::consts::PI;
        let v = Vec2::new;
        let mut solids = vec![
            Solid::rect(v(-20.0, -2.0), v(20.0, 0.0)),   // floor
            Solid::rect(v(-21.0, -2.0), v(-20.0, 15.0)), // left wall
            Solid::rect(v(20.0, -2.0), v(21.0, 15.0)),   // right wall
            Solid::rect(v(-21.0, 14.0), v(21.0, 15.0)),  // ceiling
            Solid::new(vec![v(2.0, 0.0), v(8.0, 0.0), v(8.0, 2.5)]), // ramp
            Solid::rect(v(8.0, 0.0), v(20.0, 2.5)),      // plateau
            Solid::rect(v(-14.0, 4.5), v(-9.0, 4.9)),    // ledge
        ];
        // Quarter-pipes where the floor meets the left wall and the plateau the right.
        solids.extend(fillet(v(-20.0, 0.0), v(-17.5, 2.5), 2.5, PI, 1.5 * PI, 10));
        solids.extend(fillet(
            v(20.0, 2.5),
            v(18.5, 4.0),
            1.5,
            1.5 * PI,
            2.0 * PI,
            8,
        ));
        // A rounded corner where the ceiling meets the left wall.
        solids.extend(fillet(
            v(-20.0, 14.0),
            v(-17.0, 11.0),
            3.0,
            0.5 * PI,
            PI,
            12,
        ));
        // An arched bridge and a dome on the plateau.
        solids.extend(arch(-4.0, 4.0, 7.5, 1.6, 0.5, 12));
        solids.push(Solid::convex(arc_points(v(14.0, 2.5), 1.6, 0.0, PI, 14)));
        Self { solids }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_above_box_does_not_penetrate() {
        let s = Solid::rect(Vec2::ZERO, Vec2::ONE);
        assert!(s.penetration(Vec2::new(0.5, 1.5), 0.1).is_none());
    }

    #[test]
    fn point_resting_into_top_face_is_pushed_up() {
        let s = Solid::rect(Vec2::ZERO, Vec2::ONE);
        let (n, d) = s.penetration(Vec2::new(0.5, 1.05), 0.1).unwrap();
        assert!((n - Vec2::Y).length() < 1e-6);
        assert!((d - 0.05).abs() < 1e-6);
    }

    #[test]
    fn dressing_room_builds_convex_solids() {
        let level = Level::dressing_room();
        assert!(level.solids.len() > 30);
    }

    #[test]
    fn arch_top_is_one_flat_slab() {
        let solids = arch(-4.0, 4.0, 7.5, 1.6, 0.5, 12);
        let top = solids
            .iter()
            .filter(|s| s.vertices.iter().any(|v| (v.y - 7.5).abs() < 1e-5))
            .count();
        assert_eq!(top, 1);
    }

    #[test]
    #[should_panic]
    fn clockwise_solid_is_rejected() {
        Solid::new(vec![Vec2::ZERO, Vec2::Y, Vec2::X]);
    }
}
