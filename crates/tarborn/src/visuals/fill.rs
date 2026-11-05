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
