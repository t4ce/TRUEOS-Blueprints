//! Potatomin-style native line rendering, isolated from Hull's network and layout code.
mod gpu;
mod png;

/// Layout-space rectangle: left, top, width, height.
pub type Rect = [f32; 4];

/// Render opaque magenta box outlines on white and return encoded PNG bytes.
/// This demo renders layout boxes only, not SVG text or arbitrary SVG content.
pub fn lines_png(width: u32, height: u32, rectangles: &[Rect]) -> Result<Vec<u8>, i32> {
    gpu::render(width, height, rectangles)
}

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
    fn png_uses_requested_size_and_preserves_rows() {
        let pixels: Vec<u8> = (0..24).collect();
        let encoded = png::png_rgba8(3, 2, &pixels);
        assert_eq!(&encoded[16..24], &[0, 0, 0, 3, 0, 0, 0, 2]);
        let size = u32::from_be_bytes(encoded[33..37].try_into().unwrap()) as usize;
        let rows = miniz_oxide::inflate::decompress_to_vec_zlib(&encoded[41..41 + size]).unwrap();
        assert_eq!(
            rows,
            [
                vec![0],
                pixels[..12].to_vec(),
                vec![0],
                pixels[12..].to_vec()
            ]
            .concat()
        );
    }
}
