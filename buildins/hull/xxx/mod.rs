//! Translucent layout quads. The caller supplies clipped viewport coordinates.
mod gpu;
pub use gpu::Quads;

/// Layout-space rectangle: left, top, width, height.
pub type Rect = [f32; 4];
/// Straight-alpha magenta. Deliberately preserve overlap for the diagnostic view.
const DEFAULT_COLOR: [u8; 4] = [255, 0, 255, 33];

fn geometry(width: u32, height: u32, rectangles: &[Rect]) -> (Vec<u8>, Vec<u8>) {
    let mut vertices = Vec::with_capacity(rectangles.len() * 48);
    let mut indices = Vec::with_capacity(rectangles.len() * 24);
    for &[x, y, w, h] in rectangles {
        if w == 0.0 || h == 0.0 {
            continue;
        }
        let base = (vertices.len() / 12) as u32;
        for [px, py] in [[x, y], [x + w, y], [x + w, y + h], [x, y + h]] {
            for value in [
                2.0 * px / width as f32 - 1.0,
                1.0 - 2.0 * py / height as f32,
                0.0,
            ] {
                vertices.extend_from_slice(&value.to_le_bytes());
            }
        }
        // Two triangles tile the quad; their shared diagonal is not a second layer.
        for index in [0, 1, 2, 0, 2, 3] {
            indices.extend_from_slice(&(base + index).to_le_bytes());
        }
    }
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quad_has_four_vertices_and_two_triangles() {
        let (vertices, indices) = geometry(100, 50, &[[25.0, 10.0, 50.0, 30.0]]);
        assert_eq!(vertices.len(), 48);
        let decoded: Vec<u32> = indices
            .chunks_exact(4)
            .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
            .collect();
        assert_eq!(decoded, [0, 1, 2, 0, 2, 3]);
        let expected = [[-0.5, 0.6], [0.5, 0.6], [0.5, -0.6], [-0.5, -0.6]];
        for (point, expected) in vertices.chunks_exact(12).zip(expected) {
            for axis in 0..2 {
                let value = f32::from_le_bytes(point[axis * 4..axis * 4 + 4].try_into().unwrap());
                assert!((value - expected[axis]).abs() < 1e-6);
            }
        }
    }
    #[test]
    fn duplicate_boxes_remain_separate_and_degenerate_boxes_are_skipped() {
        let (vertices, indices) = geometry(
            100,
            50,
            &[
                [0.0, 0.0, 20.0, 20.0],
                [0.0, 0.0, 20.0, 20.0],
                [10.0, 0.0, 0.0, 20.0],
            ],
        );
        assert_eq!(vertices.len(), 96);
        assert_eq!(&vertices[..48], &vertices[48..]);
        let decoded: Vec<u32> = indices
            .chunks_exact(4)
            .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
            .collect();
        assert_eq!(decoded, [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]);
        assert_eq!(DEFAULT_COLOR[3], 33);
    }
}
