#![no_std]
extern crate alloc;
use alloc::vec::Vec;

pub const COLUMNS: u32 = 39;
pub const ROWS: u32 = 55;
pub const INDEX_COUNT: u32 = (COLUMNS + ROWS + 2) * 2;

/// Match Gridpaper's centered 195 x 275 mm grid on a full 210 x 297 mm page.
/// Keep physical sizing on resize: extra area is white, smaller windows clip.
/// Each independent segment is two vertices and two indices, never adjacency.
pub fn geometry(page: (u32, u32), viewport: (u32, u32)) -> (Vec<u8>, Vec<u8>) {
    let mut vertices = Vec::with_capacity(INDEX_COUNT as usize * 12);
    let mut indices = Vec::with_capacity(INDEX_COUNT as usize * 4);
    let mut vertex = |x_mm: f32, y_mm: f32| {
        // Snap to pixel centers for crisp single-sample one-pixel lines.
        let x = (x_mm * page.0 as f32 / 210.0) as u32 as f32 + 0.5;
        let y = (y_mm * page.1 as f32 / 297.0) as u32 as f32 + 0.5;
        for value in [
            2.0 * x / viewport.0 as f32 - 1.0,
            1.0 - 2.0 * y / viewport.1 as f32,
            0.0,
        ] {
            vertices.extend_from_slice(&value.to_le_bytes());
        }
        indices.extend_from_slice(&((indices.len() / 4) as u32).to_le_bytes());
    };
    for column in 0..=COLUMNS {
        let x = 7.5 + column as f32 * 5.0;
        vertex(x, 11.0);
        vertex(x, 286.0);
    }
    for row in 0..=ROWS {
        let y = 11.0 + row as f32 * 5.0;
        vertex(7.5, y);
        vertex(202.5, y);
    }
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn centered_five_mm_grid_has_only_independent_line_pairs() {
        let (vertices, indices) = geometry((840, 1188), (840, 1188));
        assert_eq!(INDEX_COUNT, 192);
        assert_eq!(vertices.len(), 192 * 12);
        assert_eq!(indices.len(), 192 * 4);
        let pixel = |n: usize, axis: usize| {
            let start = n * 12 + axis * 4;
            let clip = f32::from_le_bytes(vertices[start..start + 4].try_into().unwrap());
            if axis == 0 {
                (clip + 1.0) * 420.0
            } else {
                (1.0 - clip) * 594.0
            }
        };
        assert!((pixel(0, 0) - 30.5).abs() < 0.001);
        assert!((pixel(0, 1) - 44.5).abs() < 0.001);
        for column in 0..39 {
            assert!((pixel((column + 1) * 2, 0) - pixel(column * 2, 0) - 20.0).abs() < 0.001);
        }
        for (n, bytes) in indices.chunks_exact(4).enumerate() {
            assert_eq!(u32::from_le_bytes(bytes.try_into().unwrap()), n as u32);
        }
    }
}
