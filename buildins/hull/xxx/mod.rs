//! Retained native layout lines. Resize uses GPU viewport mapping of fixed clip coordinates.
mod gpu;
pub use gpu::Lines;

/// Layout-space rectangle: left, top, width, height.
pub type Rect = [f32; 4];

/// Union collinear intervals using exact layout coordinates. Nearby parallel
/// lines and perpendicular crossings remain distinct; no pixel snapping occurs.
fn merged_edges(rectangles: &[Rect]) -> Vec<(bool, f32, f32, f32)> {
    let mut edges = Vec::with_capacity(rectangles.len() * 4);
    for &[x, y, w, h] in rectangles {
        let (left, right) = (x.min(x + w), x.max(x + w));
        let (top, bottom) = (y.min(y + h), y.max(y + h));
        if left < right {
            edges.extend([(false, top, left, right), (false, bottom, left, right)]);
        }
        if top < bottom {
            edges.extend([(true, left, top, bottom), (true, right, top, bottom)]);
        }
    }
    edges.sort_unstable_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.partial_cmp(&b.1).unwrap())
            .then_with(|| a.2.partial_cmp(&b.2).unwrap())
            .then_with(|| a.3.partial_cmp(&b.3).unwrap())
    });
    let mut merged: Vec<(bool, f32, f32, f32)> = Vec::with_capacity(edges.len());
    for edge in edges {
        if let Some(last) = merged.last_mut() {
            if last.0 == edge.0 && last.1 == edge.1 && edge.2 <= last.3 {
                last.3 = last.3.max(edge.3);
                continue;
            }
        }
        merged.push(edge);
    }
    merged
}

fn geometry(width: u32, height: u32, rectangles: &[Rect]) -> (Vec<u8>, Vec<u8>) {
    let edges = merged_edges(rectangles);
    let mut vertices = Vec::with_capacity(edges.len() * 24);
    let mut indices = Vec::with_capacity(edges.len() * 8);
    for (vertical, fixed, start, end) in edges {
        for along in [start, end] {
            let [px, py] = if vertical {
                [fixed, along]
            } else {
                [along, fixed]
            };
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
            [-0.5, -0.6],
            [0.5, -0.6],
            [-0.5, 0.6],
            [-0.5, -0.6],
            [0.5, 0.6],
            [0.5, -0.6],
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
    #[test]
    fn supplied_layout_removes_two_overlapping_horizontal_edges() {
        let rectangles = [
            [0.0, 0.0, 1280.0, 314.0],
            [400.0, 0.0, 480.0, 314.0],
            [432.0, 180.0, 416.0, 102.0],
        ];
        let edges = merged_edges(&rectangles);
        assert_eq!(edges.len(), 10);
        assert!(edges.contains(&(false, 0.0, 0.0, 1280.0)));
        assert!(edges.contains(&(false, 314.0, 0.0, 1280.0)));
        let (vertices, indices) = geometry(1280, 720, &rectangles);
        assert_eq!(vertices.len(), 20 * 12);
        assert_eq!(indices.len(), 20 * 4);
    }

    #[test]
    fn union_preserves_gaps_crossings_and_nearby_parallel_lines() {
        let rectangles = [
            [0.0, 0.0, 10.0, 10.0],
            [5.0, 0.0, 10.0, 10.0],
            [15.0, 0.0, 5.0, 10.0],
            [25.0, 0.0, 5.0, 10.0],
            [0.0, 0.01, 10.0, 10.0],
        ];
        let edges = merged_edges(&rectangles);
        assert!(edges.contains(&(false, 0.0, 0.0, 20.0)));
        assert!(edges.contains(&(false, 0.0, 25.0, 30.0)));
        assert!(edges.contains(&(false, 0.01, 0.0, 10.0)));
        assert!(edges.contains(&(true, 5.0, 0.0, 10.0)));
        assert!(edges.contains(&(true, 10.0, 0.0, 10.01)));
        // Check coverage independently at endpoints and between every pair of
        // endpoints for each supporting line, including formerly empty gaps.
        let raw: Vec<_> = rectangles
            .iter()
            .flat_map(|&[x, y, w, h]| {
                [
                    (false, y, x, x + w),
                    (false, y + h, x, x + w),
                    (true, x, y, y + h),
                    (true, x + w, y, y + h),
                ]
            })
            .collect();
        for &(axis, fixed, _, _) in &raw {
            let mut endpoints: Vec<f32> = raw
                .iter()
                .filter(|e| e.0 == axis && e.1 == fixed)
                .flat_map(|e| [e.2, e.3])
                .collect();
            endpoints.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let samples: Vec<_> = endpoints
                .iter()
                .copied()
                .chain(endpoints.windows(2).map(|p| (p[0] + p[1]) * 0.5))
                .collect();
            for p in samples {
                let covered = |list: &[(bool, f32, f32, f32)]| {
                    list.iter()
                        .any(|e| e.0 == axis && e.1 == fixed && e.2 <= p && p <= e.3)
                };
                assert_eq!(covered(&raw), covered(&edges));
            }
        }
    }

    #[test]
    fn identical_rectangles_and_shared_vertical_edges_are_drawn_once() {
        assert_eq!(merged_edges(&[[0.0, 0.0, 10.0, 10.0]; 3]).len(), 4);
        let edges = merged_edges(&[[0.0, 0.0, 10.0, 10.0], [10.0, 0.0, 10.0, 10.0]]);
        assert_eq!(edges.len(), 5);
        assert_eq!(edges.iter().filter(|e| e.0 && e.1 == 10.0).count(), 1);
    }
}
