#![no_std]
extern crate alloc;

use alloc::vec::Vec;

pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 720;
pub const POINT_COUNT: u32 = 100;
pub const LINE_COUNT: u32 = POINT_COUNT / 2;
pub const RGBA_BYTES: usize = WIDTH as usize * HEIGHT as usize * 4;

/// A regular 10 by 10 grid, with a 64-pixel inset. Adjacent pairs on each
/// row form 50 disjoint horizontal lines using the exact same 100 vertices.
pub fn geometry() -> (Vec<u8>, Vec<u8>) {
    let mut vertices = Vec::with_capacity(POINT_COUNT as usize * 12);
    let mut indices = Vec::with_capacity(POINT_COUNT as usize * 4);
    for ordinal in 0..POINT_COUNT {
        let x = 64.0 + (ordinal % 10) as f32 * (WIDTH as f32 - 128.0) / 9.0;
        let y = 64.0 + (ordinal / 10) as f32 * (HEIGHT as f32 - 128.0) / 9.0;
        for component in [
            2.0 * x / WIDTH as f32 - 1.0,
            1.0 - 2.0 * y / HEIGHT as f32,
            0.0,
        ] {
            vertices.extend_from_slice(&component.to_le_bytes());
        }
        indices.extend_from_slice(&ordinal.to_le_bytes());
    }
    (vertices, indices)
}

/// Encode straight RGBA8. The demo draws opaque colors, so native
/// premultiplied readback and PNG straight-alpha bytes are identical.
pub fn png_rgba8(pixels: &[u8]) -> Vec<u8> {
    assert_eq!(pixels.len(), RGBA_BYTES);
    let mut rows = Vec::with_capacity(pixels.len() + HEIGHT as usize);
    for row in pixels.chunks_exact(WIDTH as usize * 4) {
        rows.push(0); // PNG filter: None
        rows.extend_from_slice(row);
    }
    let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&rows, 1);
    let mut png = Vec::new();
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut header = [0u8; 13];
    header[..4].copy_from_slice(&WIDTH.to_be_bytes());
    header[4..8].copy_from_slice(&HEIGHT.to_be_bytes());
    header[8] = 8;
    header[9] = 6; // RGBA
    chunk(&mut png, b"IHDR", &header);
    chunk(&mut png, b"IDAT", &compressed);
    chunk(&mut png, b"IEND", &[]);
    png
}

fn chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    png.extend_from_slice(kind);
    png.extend_from_slice(data);
    let mut crc = !0u32;
    for byte in kind.iter().chain(data) {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    png.extend_from_slice(&(!crc).to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn png_preserves_all_rgba_channels_and_row_order() {
        let mut pixels = vec![0u8; RGBA_BYTES];
        for (ordinal, pixel) in pixels.chunks_exact_mut(4).enumerate() {
            pixel.copy_from_slice(&[(ordinal % 251) as u8, (ordinal / 1280 % 253) as u8, 17, 255]);
        }
        let png = png_rgba8(&pixels);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&png[16..24], &[0, 0, 5, 0, 0, 0, 2, 208]);
        assert_eq!(&png[24..29], &[8, 6, 0, 0, 0]);
        let length = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
        assert_eq!(&png[37..41], b"IDAT");
        let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&png[41..41 + length]).unwrap();
        assert_eq!(raw.len(), RGBA_BYTES + HEIGHT as usize);
        for (row, expected) in raw
            .chunks_exact(WIDTH as usize * 4 + 1)
            .zip(pixels.chunks_exact(WIDTH as usize * 4))
        {
            assert_eq!(row[0], 0);
            assert_eq!(&row[1..], expected);
        }
        // Known PNG IEND CRC independently checks the CRC implementation.
        assert_eq!(
            &png[png.len() - 12..],
            &[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82]
        );
    }

    #[test]
    fn grid_has_100_points_and_50_disjoint_equal_length_lines() {
        let (vertices, indices) = geometry();
        let points: Vec<[f32; 3]> = vertices
            .chunks_exact(12)
            .map(|v| {
                [
                    f32::from_le_bytes(v[..4].try_into().unwrap()),
                    f32::from_le_bytes(v[4..8].try_into().unwrap()),
                    f32::from_le_bytes(v[8..12].try_into().unwrap()),
                ]
            })
            .collect();
        assert_eq!(points.len(), 100);
        assert_eq!(indices.len() / 8, 50);
        let line_length = points[1][0] - points[0][0];
        for (ordinal, pair) in points.chunks_exact(2).enumerate() {
            assert!((pair[1][0] - pair[0][0] - line_length).abs() < 1e-6);
            assert_eq!(pair[0][1], pair[1][1]);
            assert!(
                pair.iter()
                    .all(|p| p[0].abs() < 1.0 && p[1].abs() < 1.0 && p[2] == 0.0)
            );
            let index = ordinal * 8;
            assert_eq!(
                u32::from_le_bytes(indices[index..index + 4].try_into().unwrap()),
                ordinal as u32 * 2
            );
        }
    }
}
