extern crate alloc;

use crate::roster::Keyboard;
use alloc::{vec, vec::Vec};

pub const MAX_KEYBOARDS: usize = 32;
const PALETTE_WIDTH: usize = 256;
const PALETTE_HEIGHT: usize = 2;

/// Prepare a single immutable PBR mesh. U comes from the GPU's compacted
/// original instance index; V selects the key or chassis material row.
pub fn prepare_mesh(source_vertices: &[u8], source_parts: &[u8]) -> Vec<u8> {
    assert_eq!(source_vertices.len() % 24, 0);
    assert_eq!(source_parts.len(), source_vertices.len() / 24);
    let mut vertices = Vec::with_capacity(source_parts.len() * 48);
    for (source, &part) in source_vertices.chunks_exact(24).zip(source_parts) {
        assert!(part <= 1);
        vertices.extend_from_slice(source);
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
        for value in [
            0.0,
            (f32::from(part) + 0.5) / 2.0,
            tangent[0],
            tangent[1],
            tangent[2],
            1.0,
        ] {
            vertices.extend_from_slice(&value.to_le_bytes());
        }
    }
    vertices
}

pub struct RowInstances {
    pub seeds: Vec<u8>,
    pub count: u32,
    pub center: [f32; 3],
    pub radius: f32,
}

/// Roster updates upload only 64-byte transforms and a tiny color atlas.
/// No vertex or index data is duplicated for additional keyboards.
pub fn prepare_row(keyboards: &[Keyboard], radius: f32) -> RowInstances {
    assert!(keyboards.len() <= MAX_KEYBOARDS);
    let spacing = radius * 2.2;
    let span = keyboards.len().saturating_sub(1) as f32 * spacing;
    let mut row = RowInstances {
        seeds: Vec::with_capacity(keyboards.len() * 64),
        count: keyboards.len() as u32,
        center: [0.0, 0.0, span * 0.5],
        radius: radius + span * 0.5,
    };
    for index in 0..keyboards.len() {
        let z = index as f32 * spacing;
        for value in [
            0.0, 0.0, z, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, radius, 0.0, 0.0, z,
        ] {
            row.seeds.extend_from_slice(&value.to_le_bytes());
        }
        row.seeds.extend_from_slice(&0u32.to_le_bytes()); // One full-mesh draw group.
        row.seeds
            .extend_from_slice(&((index as u32) << 16).to_le_bytes());
    }
    row
}

fn read_f32(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

/// Column = original instance index; rows = keys/chassis. Colors come from
/// the HUT combo palette, with neutral gray for unused columns.
pub fn palette_bmp(keyboards: &[Keyboard]) -> Vec<u8> {
    assert!(keyboards.len() <= MAX_KEYBOARDS);
    material_bmp(Some(keyboards))
}

pub fn metallic_roughness_bmp() -> Vec<u8> {
    material_bmp(None)
}

fn material_bmp(keyboards: Option<&[Keyboard]>) -> Vec<u8> {
    let stride = PALETTE_WIDTH * 3;
    let mut bmp = vec![0; 54 + stride * PALETTE_HEIGHT];
    bmp[..2].copy_from_slice(b"BM");
    for (offset, value) in [
        (2, (54 + stride * PALETTE_HEIGHT) as u32),
        (10, 54),
        (14, 40),
        (18, PALETTE_WIDTH as u32),
        (22, PALETTE_HEIGHT as u32),
        (34, (stride * PALETTE_HEIGHT) as u32),
    ] {
        bmp[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
    bmp[28..30].copy_from_slice(&24u16.to_le_bytes());
    for row in 0..PALETTE_HEIGHT {
        for column in 0..PALETTE_WIDTH {
            let [r, g, b, _] = match keyboards {
                Some(keyboards) if row == 0 => keyboards
                    .get(column)
                    .map_or([128, 128, 128, 255], |keyboard| keyboard.rgba),
                Some(_) => [128, 128, 128, 255],
                None if row == 0 => [255, 255, 0, 255],
                None => [255, 102, 255, 255],
            };
            // Positive-height BMP storage is bottom-up; UV row 0 is keys.
            let at = 54 + (PALETTE_HEIGHT - 1 - row) * stride + column * 3;
            bmp[at..at + 3].copy_from_slice(&[b, g, r]);
        }
    }
    bmp
}

#[cfg(test)]
mod tests {
    use super::*;
    use trueos::hid::InputComboColor;

    fn keyboard(index: u8) -> Keyboard {
        Keyboard {
            endpoint: [1, index as u32, 0],
            combo_id: index as u32 + 1,
            rgba: InputComboColor::from_index(index % InputComboColor::COUNT).rgba(),
        }
    }
    fn pixel(bmp: &[u8], column: usize, row: usize) -> [u8; 3] {
        let at = 54 + (1 - row) * 768 + column * 3;
        [bmp[at + 2], bmp[at + 1], bmp[at]]
    }

    #[test]
    fn k70_geometry_is_prepared_once_with_chassis_material_and_no_duplication() {
        let source = include_bytes!(concat!(env!("OUT_DIR"), "/keyboard.vertices.bin"));
        let parts = include_bytes!(concat!(env!("OUT_DIR"), "/keyboard.parts.bin"));
        let mesh = prepare_mesh(source, parts);
        assert_eq!(parts.len(), 36152);
        assert_eq!(parts.iter().filter(|&&part| part == 1).count(), 88);
        assert_eq!(mesh.len(), parts.len() * 48);
        for ((vertex, source), &part) in mesh
            .chunks_exact(48)
            .zip(source.chunks_exact(24))
            .zip(parts)
        {
            assert_eq!(&vertex[..24], source);
            assert_eq!(read_f32(vertex, 28), if part == 1 { 0.75 } else { 0.25 });
            let normal: [f32; 3] = core::array::from_fn(|i| read_f32(vertex, 12 + i * 4));
            let tangent: [f32; 3] = core::array::from_fn(|i| read_f32(vertex, 32 + i * 4));
            assert!(
                normal
                    .iter()
                    .zip(tangent)
                    .map(|(a, b)| a * b)
                    .sum::<f32>()
                    .abs()
                    < 0.0001
            );
        }
        for count in [0, 1, 2, 32] {
            let keyboards: Vec<_> = (0..count).map(keyboard).collect();
            let row = prepare_row(&keyboards, 20.0);
            assert_eq!(row.seeds.len(), count as usize * 64);
            assert_eq!(row.count, count as u32);
            for (index, seed) in row.seeds.chunks_exact(64).enumerate() {
                let z = index as f32 * 44.0;
                assert_eq!(read_f32(seed, 8), z);
                assert_eq!(read_f32(seed, 52), z);
                assert_eq!(read_f32(seed, 40), 20.0);
                assert_eq!(
                    u32::from_le_bytes(seed[60..64].try_into().unwrap()),
                    (index as u32) << 16
                );
                assert!((z - row.center[2]).abs() + 20.0 <= row.radius + 0.001);
            }
        }
    }

    #[test]
    fn compaction_keeps_exact_combo_colors_and_chassis_properties() {
        let keyboards: Vec<_> = (0..32).map(keyboard).collect();
        let colors = palette_bmp(&keyboards);
        let properties = metallic_roughness_bmp();
        assert_eq!(colors.len(), 1590);
        assert_eq!(&colors[..2], b"BM");
        assert_eq!(u32::from_le_bytes(colors[18..22].try_into().unwrap()), 256);
        assert_eq!(u32::from_le_bytes(colors[22..26].try_into().unwrap()), 2);
        // VF fetches the GPU compaction row's low byte as R8_UNORM.
        // Even when preceding boards are culled, the original index selects
        // its own exact 8-bit color rather than the compacted draw slot.
        for original in [0usize, 7, 16, 31] {
            let u = original as f32 / 255.0;
            let texel = libm::floorf(u * 256.0) as usize;
            assert_eq!(texel, original);
            assert_eq!(pixel(&colors, texel, 0), keyboards[original].rgba[..3]);
            assert_eq!(pixel(&colors, texel, 1), [128; 3]);
            assert_eq!(pixel(&properties, texel, 0), [255, 255, 0]);
            assert_eq!(pixel(&properties, texel, 1), [255, 102, 255]);
        }
        assert_eq!(pixel(&palette_bmp(&[]), 0, 0), [128; 3]);
    }
}
