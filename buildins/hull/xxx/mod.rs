//! Retained native layout lines. Resize uses GPU viewport mapping of fixed clip coordinates.
mod gpu;
pub use gpu::Lines;

/// Layout-space rectangle: left, top, width, height.
pub type Rect = [f32; 4];

fn geometry(width: u32, height: u32, rectangles: &[Rect]) -> (Vec<u8>, Vec<u8>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for &[x, y, w, h] in rectangles {
        for [px, py] in [
            [x, y],
            [x + w, y],
            [x + w, y],
            [x + w, y + h],
            [x + w, y + h],
            [x, y + h],
            [x, y + h],
            [x, y],
        ] {
            let index = (vertices.len() / 12) as u32;
            for value in [
                2.0 * px / width as f32 - 1.0,
                1.0 - 2.0 * py / height as f32,
                0.0,
            ] {
                vertices.extend_from_slice(&value.to_le_bytes());
            }
            indices.extend_from_slice(&index.to_le_bytes());
        }
    }
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangle_forms_four_connected_edges_in_clip_space() {
        let (vertices, indices) = geometry(100, 50, &[[25.0, 10.0, 50.0, 30.0]]);
        let points: Vec<[f32; 2]> = vertices
            .chunks_exact(12)
            .map(|v| {
                [
                    f32::from_le_bytes(v[0..4].try_into().unwrap()),
                    f32::from_le_bytes(v[4..8].try_into().unwrap()),
                ]
            })
            .collect();
        let expected = [
            [-0.5, 0.6],
            [0.5, 0.6],
            [0.5, 0.6],
            [0.5, -0.6],
            [0.5, -0.6],
            [-0.5, -0.6],
            [-0.5, -0.6],
            [-0.5, 0.6],
        ];
        for (actual, expected) in points.iter().zip(expected) {
            for axis in 0..2 {
                assert!((actual[axis] - expected[axis]).abs() < 1e-6);
            }
        }
        assert_eq!(points.len(), 8);
        assert_eq!(
            indices,
            (0u32..8).flat_map(u32::to_le_bytes).collect::<Vec<_>>()
        );
    }
    #[test]
    fn retained_vertex_preserves_relative_position_across_viewports() {
        let (vertices, _) = geometry(1280, 720, &[[320.0, 180.0, 100.0, 50.0]]);
        let clip_x = f32::from_le_bytes(vertices[..4].try_into().unwrap());
        let clip_y = f32::from_le_bytes(vertices[4..8].try_into().unwrap());
        for (width, height) in [(1280.0, 720.0), (640.0, 360.0), (1920.0, 500.0)] {
            // Fixed GPU viewport transform applied to the same retained vertex bytes.
            let x = (clip_x + 1.0) * width * 0.5;
            let y = (1.0 - clip_y) * height * 0.5;
            assert_eq!(x / width, 0.25);
            assert_eq!(y / height, 0.25);
        }
    }
}
