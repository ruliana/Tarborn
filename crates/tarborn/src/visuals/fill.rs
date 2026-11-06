//! A batch of flat-colored triangles, rebuilt into one mesh.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

#[derive(Default)]
pub struct Fill {
    positions: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl Fill {
    /// A convex polygon, as a fan from its first vertex.
    pub fn polygon(&mut self, points: &[Vec2], color: Color) {
        if points.len() < 3 {
            return;
        }
        let base = self.positions.len() as u32;
        let c = color.to_linear().to_f32_array();
        for p in points {
            self.positions.push([p.x, p.y, 0.0]);
            self.colors.push(c);
        }
        for k in 1..points.len() as u32 - 1 {
            self.indices.extend([base, base + k, base + k + 1]);
        }
    }

    pub fn rect(&mut self, center: Vec2, size: Vec2, color: Color) {
        let h = size * 0.5;
        self.polygon(
            &[
                center - h,
                center + Vec2::new(h.x, -h.y),
                center + h,
                center + Vec2::new(-h.x, h.y),
            ],
            color,
        );
    }

    /// A straight band of `width` from `a` to `b`.
    pub fn segment(&mut self, a: Vec2, b: Vec2, width: f32, color: Color) {
        let side = (b - a).normalize_or_zero().perp() * width * 0.5;
        self.polygon(&[a - side, b - side, b + side, a + side], color);
    }

    pub fn circle(&mut self, center: Vec2, radius: f32, segments: usize, color: Color) {
        let points: Vec<Vec2> = (0..segments.max(3))
            .map(|k| {
                center
                    + Vec2::from_angle(k as f32 / segments as f32 * std::f32::consts::TAU) * radius
            })
            .collect();
        self.polygon(&points, color);
    }

    /// A convex polygon with one color per vertex (glows, fades).
    pub fn polygon_colors(&mut self, points: &[Vec2], colors: &[Color]) {
        if points.len() < 3 || colors.len() != points.len() {
            return;
        }
        let base = self.positions.len() as u32;
        for (p, c) in points.iter().zip(colors) {
            self.positions.push([p.x, p.y, 0.0]);
            self.colors.push(c.to_linear().to_f32_array());
        }
        for k in 1..points.len() as u32 - 1 {
            self.indices.extend([base, base + k, base + k + 1]);
        }
    }

    /// A soft disc: `inner` at the centre fading to `outer` at the rim.
    pub fn glow(&mut self, center: Vec2, radius: f32, segments: usize, inner: Color, outer: Color) {
        let segments = segments.max(3) as u32;
        let base = self.positions.len() as u32;
        self.positions.push([center.x, center.y, 0.0]);
        self.colors.push(inner.to_linear().to_f32_array());
        let o = outer.to_linear().to_f32_array();
        for k in 0..segments {
            let p = center
                + Vec2::from_angle(k as f32 / segments as f32 * std::f32::consts::TAU) * radius;
            self.positions.push([p.x, p.y, 0.0]);
            self.colors.push(o);
        }
        for k in 0..segments {
            self.indices
                .extend([base, base + 1 + k, base + 1 + (k + 1) % segments]);
        }
    }

    pub fn ellipse(
        &mut self,
        center: Vec2,
        radii: Vec2,
        angle: f32,
        segments: usize,
        color: Color,
    ) {
        let rot = Vec2::from_angle(angle);
        let points: Vec<Vec2> = (0..segments.max(3))
            .map(|k| {
                let a =
                    Vec2::from_angle(k as f32 / segments as f32 * std::f32::consts::TAU) * radii;
                center + rot.rotate(a)
            })
            .collect();
        self.polygon(&points, color);
    }

    /// A stroke along `points`, `width` wide, with round-ish joints.
    pub fn polyline(&mut self, points: &[Vec2], width: f32, color: Color) {
        for w in points.windows(2) {
            self.segment(w[0], w[1], width, color);
        }
        if width > 1.5 {
            for p in points.iter().skip(1).take(points.len().saturating_sub(2)) {
                self.circle(*p, width * 0.5, 6, color);
            }
        }
    }

    /// A stroke that narrows from `from` to `to` width along its length.
    pub fn tapered(&mut self, points: &[Vec2], from: f32, to: f32, color: Color) {
        let n = points.len().max(2) - 1;
        for (i, w) in points.windows(2).enumerate() {
            let (a, b) = (w[0], w[1]);
            let side = (b - a).normalize_or_zero().perp();
            let wa = from + (to - from) * i as f32 / n as f32;
            let wb = from + (to - from) * (i + 1) as f32 / n as f32;
            self.polygon(
                &[
                    a - side * wa * 0.5,
                    b - side * wb * 0.5,
                    b + side * wb * 0.5,
                    a + side * wa * 0.5,
                ],
                color,
            );
        }
    }

    /// A quadratic curve from `a` to `b` bent towards `control`.
    pub fn curve(a: Vec2, control: Vec2, b: Vec2, steps: usize) -> Vec<Vec2> {
        (0..=steps.max(1))
            .map(|i| {
                let t = i as f32 / steps.max(1) as f32;
                a * (1.0 - t) * (1.0 - t) + control * 2.0 * t * (1.0 - t) + b * t * t
            })
            .collect()
    }

    pub fn into_mesh(mut self) -> Mesh {
        if self.indices.is_empty() {
            // An empty mesh is not drawable; one invisible triangle keeps it valid.
            self.positions = vec![[0.0; 3]; 3];
            self.colors = vec![[0.0; 4]; 3];
            self.indices = vec![0, 1, 2];
        }
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}
