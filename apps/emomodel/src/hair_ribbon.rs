use glam::Vec3;

use crate::hair::PARTICLES;

// Adds about 2.5 pixels at the default 640 × 560 framing.
const GUIDE_STROKE_INCREASE: f32 = 0.008;
pub(super) const TRIANGLES: [[u32; 3]; (PARTICLES - 1) * 2] = ribbon_triangles();

const fn ribbon_triangles() -> [[u32; 3]; (PARTICLES - 1) * 2] {
    let mut triangles = [[0; 3]; (PARTICLES - 1) * 2];
    let mut segment = 0;
    while segment < PARTICLES - 1 {
        let left = (segment * 2) as u32;
        triangles[segment * 2] = [left, left + 2, left + 1];
        triangles[segment * 2 + 1] = [left + 1, left + 2, left + 3];
        segment += 1;
    }
    triangles
}

pub(super) fn vertices(points: &[Vec3; PARTICLES], view_back: Vec3) -> [[Vec3; 2]; PARTICLES] {
    let root_width = (points[0].distance(points[PARTICLES - 1]) * 0.007).clamp(0.003, 0.005);
    std::array::from_fn(|i| {
        let tangent = points[(i + 1).min(PARTICLES - 1)] - points[i.saturating_sub(1)];
        // Face the middle of the observer's shallow orbit to keep the strands thin.
        let side = view_back.cross(tangent).normalize_or_zero();
        let progress = i as f32 / (PARTICLES - 1) as f32;
        let taper = progress * progress;
        let width = root_width * (1.0 - 0.55 * taper * taper) + GUIDE_STROKE_INCREASE;
        let half_width = width * 0.5;
        let offset = side * half_width;
        [points[i] - offset, points[i] + offset]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ribbon_faces_observer_and_tapers_to_tip() {
        let points = std::array::from_fn(|i| Vec3::NEG_Y * (i as f32 / 10.0));
        let [root, .., tip] = vertices(&points, Vec3::Z);
        assert!(root[0].distance(root[1]) > tip[0].distance(tip[1]));
        let verts = vertices(&points, Vec3::Z);
        for triangle in TRIANGLES {
            let [a, b, c] = triangle.map(|i| verts[i as usize / 2][i as usize % 2]);
            assert!((b - a).cross(c - a).dot(Vec3::Z) > 0.0);
        }
    }
}
