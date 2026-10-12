use std::vec::Vec;

/// Encode straight RGBA8. The demo draws opaque colors, so native
/// premultiplied readback and PNG straight-alpha bytes are identical.
pub(super) fn png_rgba8(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    assert_eq!(pixels.len(), width as usize * height as usize * 4);
    let mut rows = Vec::with_capacity(pixels.len() + height as usize);
    for row in pixels.chunks_exact(width as usize * 4) {
        rows.push(0); // PNG filter: None
        rows.extend_from_slice(row);
    }
    let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&rows, 1);
    let mut png = Vec::new();
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut header = [0u8; 13];
    header[..4].copy_from_slice(&width.to_be_bytes());
    header[4..8].copy_from_slice(&height.to_be_bytes());
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
