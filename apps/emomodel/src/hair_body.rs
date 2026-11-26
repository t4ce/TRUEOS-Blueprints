use glam::{UVec2, Vec2, Vec3};

const BINS_PER_AXIS: usize = 40;

/// Rear surface of the skinned body, sampled along +Z in the avatar's world space.
/// The grid only rejects distant triangles. Depth comes from the actual surface.
#[derive(Default)]
pub(super) struct BodyBack {
    minimum: Vec2,
    maximum: Vec2,
    bin_scale: Vec2,
    triangles: Vec<ProjectedTriangle>,
    bins: Vec<Vec<usize>>,
}

struct ProjectedTriangle {
    origin: Vec3,
    edge_ab: Vec3,
    edge_ac: Vec3,
    inverse_area: f32,
    minimum: Vec2,
    maximum: Vec2,
}

impl BodyBack {
    pub(super) fn update(&mut self, positions: &[[f32; 3]], body_indices: &[u32]) {
        self.triangles.clear();
        self.minimum = Vec2::splat(f32::INFINITY);
        self.maximum = Vec2::splat(f32::NEG_INFINITY);
        for triangle in body_indices.as_chunks::<3>().0 {
            let points = triangle.map(|index| Vec3::from_array(positions[index as usize]));
            if let Some(triangle) = ProjectedTriangle::new(points) {
                self.minimum = self.minimum.min(triangle.minimum);
                self.maximum = self.maximum.max(triangle.maximum);
                self.triangles.push(triangle);
            }
        }
        if self.triangles.is_empty() {
            return;
        }

        self.bin_scale = Vec2::splat(BINS_PER_AXIS as f32) / (self.maximum - self.minimum);
        self.bins
            .resize_with(BINS_PER_AXIS * BINS_PER_AXIS, Vec::new);
        for bin in &mut self.bins {
            bin.clear();
        }
        for (index, triangle) in self.triangles.iter().enumerate() {
            let low = self.bin(triangle.minimum);
            let high = self.bin(triangle.maximum);
            for y in low.y as usize..=high.y as usize {
                let row = y * BINS_PER_AXIS;
                for bin in &mut self.bins[row + low.x as usize..=row + high.x as usize] {
                    bin.push(index);
                }
            }
        }
    }

    pub(super) fn depth_at(&self, x: f32, y: f32) -> f32 {
        let point = Vec2::new(x, y);
        if self.triangles.is_empty()
            || point.cmplt(self.minimum).any()
            || point.cmpgt(self.maximum).any()
        {
            return f32::NEG_INFINITY;
        }
        let bin = self.bin(point);
        self.bins[bin.y as usize * BINS_PER_AXIS + bin.x as usize]
            .iter()
            .filter_map(|&index| self.triangles[index].depth_at(point))
            .fold(f32::NEG_INFINITY, f32::max)
    }

    fn bin(&self, point: Vec2) -> UVec2 {
        ((point - self.minimum) * self.bin_scale)
            .as_uvec2()
            .min(UVec2::splat(BINS_PER_AXIS as u32 - 1))
    }
}

impl ProjectedTriangle {
    fn new([a, b, c]: [Vec3; 3]) -> Option<Self> {
        let edge_ab = b - a;
        let edge_ac = c - a;
        let area = edge_ab.truncate().perp_dot(edge_ac.truncate());
        if !area.is_finite() || area.abs() < 1.0e-9 {
            return None;
        }
        Some(Self {
            origin: a,
            edge_ab,
            edge_ac,
            inverse_area: area.recip(),
            minimum: a.min(b).min(c).truncate(),
            maximum: a.max(b).max(c).truncate(),
        })
    }

    fn depth_at(&self, point: Vec2) -> Option<f32> {
        let offset = point - self.origin.truncate();
        let b = offset.perp_dot(self.edge_ac.truncate()) * self.inverse_area;
        let c = self.edge_ab.truncate().perp_dot(offset) * self.inverse_area;
        let a = 1.0 - b - c;
        (a.min(b).min(c) >= -1.0e-5)
            .then_some(self.origin.z + b * self.edge_ab.z + c * self.edge_ac.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_the_rearmost_triangle_at_the_exact_query_position() {
        let positions = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 1.0, 2.0],
            [1.0, 0.0, 2.0],
            [0.0, 0.0, 3.0],
        ];
        let mut body = BodyBack::default();
        assert_eq!(body.depth_at(0.25, 0.25), f32::NEG_INFINITY);
        body.update(&positions, &[0, 1, 2, 3, 4, 5, 3, 6, 4]);
        assert_eq!(body.depth_at(0.25, 0.25), 1.5);
        assert_eq!(body.depth_at(1.0, 0.0), 2.0);
        assert_eq!(body.depth_at(0.75, 0.55), f32::NEG_INFINITY);
        body.update(&positions, &[0, 1, 2]);
        assert_eq!(body.depth_at(0.25, 0.25), 0.0);
        body.update(&[], &[]);
        assert_eq!(body.depth_at(0.25, 0.25), f32::NEG_INFINITY);
    }
}
