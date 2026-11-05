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
    #[should_panic]
    fn clockwise_solid_is_rejected() {
        Solid::new(vec![Vec2::ZERO, Vec2::Y, Vec2::X]);
    }
}
