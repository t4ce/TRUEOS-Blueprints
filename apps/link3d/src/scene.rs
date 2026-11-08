extern crate alloc;

use alloc::{vec, vec::Vec};
use trueos::hid::InputComboColor;

use crate::roster::Keyboard;

const PALETTE_WIDTH: usize = InputComboColor::COUNT as usize + 1;

pub struct RowGeometry {
    pub vertices: Vec<u8>,
    pub indices: Vec<u8>,
    pub center: [f32; 3],
    pub radius: f32,
    pub origin_radius: f32,
}

/// Expand the prepared GLB mesh only when its device roster changes. Constant
/// UVs select one shared palette texel for each independently colored keyboard.
pub fn prepare_row(
    source_vertices: &[u8],
    source_indices: &[u8],
    keyboards: &[Keyboard],
    radius: f32,
) -> RowGeometry {
    assert_eq!(source_vertices.len() % 24, 0);
    assert_eq!(source_indices.len() % 4, 0);
    let spacing = radius * 2.2;
    let span = keyboards.len().saturating_sub(1) as f32 * spacing;
    let vertex_count = source_vertices.len() / 24;
    let mut row = RowGeometry {
        vertices: Vec::with_capacity(vertex_count * keyboards.len() * 48),
        indices: Vec::with_capacity(source_indices.len() * keyboards.len()),
        center: [0.0, 0.0, span * 0.5],
        radius: radius + span * 0.5,
        origin_radius: radius + span,
    };
    for (index, keyboard) in keyboards.iter().enumerate() {
        let offset_z = index as f32 * spacing;
        let palette_index = (0..InputComboColor::COUNT)
            .find(|&color| InputComboColor::from_index(color).rgba() == keyboard.rgba)
            .unwrap_or(InputComboColor::COUNT);
        let u = (f32::from(palette_index) + 0.5) / PALETTE_WIDTH as f32;
        for source in source_vertices.chunks_exact(24) {
            let start = row.vertices.len();
            row.vertices.extend_from_slice(source);
            if index != 0 {
                row.vertices[start + 8..start + 12]
                    .copy_from_slice(&(read_f32(source, 8) + offset_z).to_le_bytes());
            }
            let normal = [
                read_f32(source, 12),
                read_f32(source, 16),
                read_f32(source, 20),
            ];
            let tangent = if normal[0].abs() > normal[2].abs() {
                [-normal[1], normal[0], 0.0]
            } else {
                [0.0, -normal[2], normal[1]]
            };
            let length = libm::sqrtf(tangent.iter().map(|value| value * value).sum());
            let tangent = if length > 0.0 {
                tangent.map(|value| value / length)
            } else {
                [1.0, 0.0, 0.0]
            };
            for value in [u, 0.5, tangent[0], tangent[1], tangent[2], 1.0] {
                row.vertices.extend_from_slice(&value.to_le_bytes());
            }
        }
        let base = u32::try_from(index * vertex_count).expect("keyboard vertex offset");
        for source in source_indices.chunks_exact(4) {
            let vertex = u32::from_le_bytes(source.try_into().unwrap());
            row.indices
                .extend_from_slice(&(base + vertex).to_le_bytes());
        }
    }
    row
}

fn read_f32(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

/// One 24-bit BMP row: the canonical combo palette followed by neutral gray.
pub fn palette_bmp() -> Vec<u8> {
    let row_stride = (PALETTE_WIDTH * 3 + 3) & !3;
    let mut bmp = vec![0; 54 + row_stride];
    bmp[..2].copy_from_slice(b"BM");
    for (offset, value) in [
        (2, (54 + row_stride) as u32),
        (10, 54),
        (14, 40),
        (18, PALETTE_WIDTH as u32),
        (22, 1),
        (34, row_stride as u32),
    ] {
        bmp[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
    bmp[28..30].copy_from_slice(&24u16.to_le_bytes());
    for index in 0..PALETTE_WIDTH {
        let [r, g, b, _] = if index < InputComboColor::COUNT as usize {
            InputComboColor::from_index(index as u8).rgba()
        } else {
            [128, 128, 128, 255]
        };
        bmp[54 + index * 3..57 + index * 3].copy_from_slice(&[b, g, r]);
    }
    bmp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> (Vec<u8>, Vec<u8>) {
        let vertices = [
            [1.0f32, 0.0, 0.0, 0.0, 1.0, 0.0],
            [0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            [0.0, 0.0, 1.0, 1.0, 0.0, 0.0],
        ]
        .into_iter()
        .flatten()
        .flat_map(f32::to_le_bytes)
        .collect();
        let indices = [2u32, 0, 1]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        (vertices, indices)
    }

    fn keyboard(index: u8) -> Keyboard {
        Keyboard {
            endpoint: [0, u32::from(index) + 1, 0],
            combo_id: u32::from(index) + 1,
            rgba: InputComboColor::from_index(index).rgba(),
        }
    }

    #[test]
    fn empty_roster_has_no_geometry_and_keeps_valid_origin_bounds() {
        let (vertices, indices) = source();
        let row = prepare_row(&vertices, &indices, &[], 2.0);
        assert!(row.vertices.is_empty() && row.indices.is_empty());
        assert_eq!(row.center, [0.0; 3]);
        assert_eq!(row.radius, 2.0);
        assert_eq!(row.origin_radius, 2.0);
    }

    #[test]
    fn thirty_two_meshes_preserve_authored_geometry_winding_and_valid_tangents() {
        let (vertices, indices) = source();
        let keyboards: Vec<_> = (0..32).map(keyboard).collect();
        let row = prepare_row(&vertices, &indices, &keyboards, 2.0);
        assert_eq!(row.vertices.len(), 32 * 3 * 48);
        assert_eq!(row.indices.len(), 32 * indices.len());
        for keyboard_index in 0..32 {
            for vertex_index in 0..3 {
                let source = &vertices[vertex_index * 24..(vertex_index + 1) * 24];
                let start = (keyboard_index * 3 + vertex_index) * 48;
                let vertex = &row.vertices[start..start + 48];
                assert_eq!(&vertex[..8], &source[..8]);
                assert_eq!(&vertex[12..24], &source[12..24]);
                assert_eq!(
                    read_f32(vertex, 8),
                    read_f32(source, 8) + keyboard_index as f32 * 4.4
                );
                let dot: f32 = (0..3)
                    .map(|axis| read_f32(vertex, 12 + axis * 4) * read_f32(vertex, 32 + axis * 4))
                    .sum();
                let length: f32 = (0..3)
                    .map(|axis| {
                        let value = read_f32(vertex, 32 + axis * 4);
                        value * value
                    })
                    .sum();
                assert!(dot.abs() < 0.000001 && (length - 1.0).abs() < 0.000001);
                assert_eq!(read_f32(vertex, 44), 1.0);
            }
            for (corner, source_index) in [2u32, 0, 1].into_iter().enumerate() {
                let start = (keyboard_index * 3 + corner) * 4;
                assert_eq!(
                    u32::from_le_bytes(row.indices[start..start + 4].try_into().unwrap()),
                    keyboard_index as u32 * 3 + source_index
                );
            }
        }
        assert_eq!(&row.vertices[..24], &vertices[..24]);
        let span = 31.0 * 4.4;
        assert_eq!(row.center, [0.0, 0.0, span * 0.5]);
        assert_eq!(row.radius, 2.0 + span * 0.5);
        assert_eq!(row.origin_radius, 2.0 + span);
        for index in 0..32 {
            let z = read_f32(&row.vertices, index * 3 * 48 + 8);
            assert!((z - row.center[2]).abs() + 2.0 <= row.radius + 0.0001);
            assert!(z + 2.0 <= row.origin_radius + 0.0001);
            if index != 0 {
                let previous_z = read_f32(&row.vertices, (index - 1) * 3 * 48 + 8);
                assert!(z - previous_z > 4.0);
            }
        }
    }

    #[test]
    fn each_keyboard_selects_its_central_palette_texel_including_neutral() {
        let (vertices, indices) = source();
        let mut keyboards: Vec<_> = (0..InputComboColor::COUNT).map(keyboard).collect();
        keyboards.push(Keyboard {
            rgba: [128, 128, 128, 255],
            ..keyboard(0)
        });
        let row = prepare_row(&vertices, &indices, &keyboards, 2.0);
        for (index, vertex) in row.vertices.chunks_exact(48).enumerate() {
            assert_eq!(read_f32(vertex, 24), ((index / 3) as f32 + 0.5) / 17.0);
            assert_eq!(read_f32(vertex, 28), 0.5);
        }
    }

    #[test]
    fn bmp_header_pixels_and_padding_match_shared_palette() {
        let bmp = palette_bmp();
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(bmp.len(), 106);
        for (offset, value) in [(2, 106u32), (10, 54), (14, 40), (18, 17), (22, 1), (34, 52)] {
            assert_eq!(
                u32::from_le_bytes(bmp[offset..offset + 4].try_into().unwrap()),
                value
            );
        }
        assert_eq!(&bmp[26..30], &[1, 0, 24, 0]);
        for color in 0..InputComboColor::COUNT {
            let [r, g, b, _] = InputComboColor::from_index(color).rgba();
            let start = 54 + usize::from(color) * 3;
            assert_eq!(&bmp[start..start + 3], &[b, g, r]);
        }
        assert_eq!(&bmp[102..106], &[128, 128, 128, 0]);
    }
}
