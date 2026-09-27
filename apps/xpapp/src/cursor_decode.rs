//! Decoder for the DIB payload of a Win32 `RT_CURSOR` resource.
//!
//! A cursor resource starts with two little-endian hotspot words; the bytes
//! passed here start immediately after those words.  The bitmap has an XOR
//! plane followed by an AND plane, and its stored height includes both planes.
//! This module deliberately rejects the classic XOR-composite pixels: their
//! result depends on the destination framebuffer and cannot truthfully be
//! registered as a static RGBA sprite.

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedCursor {
    pub width: u32,
    pub height: u32,
    pub hotspot_x: u16,
    pub hotspot_y: u16,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorDecodeError {
    Header,
    Dimensions,
    Format,
    Truncated,
    XorComposite,
    Allocation,
}

const BI_RGB: u32 = 0;
/// Win32 cursors may be 256 pixels per axis; larger DIBs are not cursor
/// assets for this compatibility boundary and would make a registration-sized
/// allocation unbounded.
const MAX_CURSOR_EDGE: u32 = 256;

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    bytes.get(offset..offset.checked_add(2)?)
        .map(|value| u16::from_le_bytes([value[0], value[1]]))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    bytes.get(offset..offset.checked_add(4)?)
        .map(|value| u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn i32_at(bytes: &[u8], offset: usize) -> Option<i32> {
    u32_at(bytes, offset).map(|value| value as i32)
}

fn stride(width: usize, bits_per_pixel: usize) -> Option<usize> {
    width
        .checked_mul(bits_per_pixel)?
        .checked_add(31)?
        .checked_div(32)?
        .checked_mul(4)
}

/// Decode a resource DIB into a top-down, non-premultiplied RGBA sprite.
///
/// `width` and `height` are the selected `RT_GROUP_CURSOR` visible dimensions;
/// they are validated against the DIB rather than trusted.  Only uncompressed
/// 1/4/8/24/32 bpp DIBs are accepted because those preserve a direct static
/// RGBA interpretation.  A classic `AND=1, XOR!=0` pixel is rejected instead
/// of silently compositing it against an invented background.
pub fn decode_cursor_dib(
    width: u32,
    height: u32,
    hotspot_x: u16,
    hotspot_y: u16,
    dib: &[u8],
) -> Result<DecodedCursor, CursorDecodeError> {
    const HEADER_BYTES: usize = 40;
    if dib.len() < HEADER_BYTES || u32_at(dib, 0) != Some(HEADER_BYTES as u32) {
        return Err(CursorDecodeError::Header);
    }
    let dib_width = i32_at(dib, 4).ok_or(CursorDecodeError::Header)?;
    let dib_height = i32_at(dib, 8).ok_or(CursorDecodeError::Header)?;
    let planes = u16_at(dib, 12).ok_or(CursorDecodeError::Header)?;
    let bits_per_pixel = u16_at(dib, 14).ok_or(CursorDecodeError::Header)?;
    let compression = u32_at(dib, 16).ok_or(CursorDecodeError::Header)?;
    let colors_used = u32_at(dib, 32).ok_or(CursorDecodeError::Header)?;
    if width == 0
        || height == 0
        || width > MAX_CURSOR_EDGE
        || height > MAX_CURSOR_EDGE
        || u32::from(hotspot_x) >= width
        || u32::from(hotspot_y) >= height
        || dib_width != i32::try_from(width).map_err(|_| CursorDecodeError::Dimensions)?
        || dib_height
            != i32::try_from(height.checked_mul(2).ok_or(CursorDecodeError::Dimensions)?)
                .map_err(|_| CursorDecodeError::Dimensions)?
        || planes != 1
    {
        return Err(CursorDecodeError::Dimensions);
    }
    if compression != BI_RGB || !matches!(bits_per_pixel, 1 | 4 | 8 | 24 | 32) {
        return Err(CursorDecodeError::Format);
    }

    let width = usize::try_from(width).map_err(|_| CursorDecodeError::Dimensions)?;
    let height = usize::try_from(height).map_err(|_| CursorDecodeError::Dimensions)?;
    let bpp = usize::from(bits_per_pixel);
    let palette_entries = if bpp <= 8 {
        let default = 1usize << bpp;
        if colors_used == 0 {
            default
        } else {
            let specified = usize::try_from(colors_used).map_err(|_| CursorDecodeError::Format)?;
            if specified > default {
                return Err(CursorDecodeError::Format);
            }
            specified
        }
    } else {
        0
    };
    let palette_end = HEADER_BYTES
        .checked_add(palette_entries.checked_mul(4).ok_or(CursorDecodeError::Format)?)
        .ok_or(CursorDecodeError::Format)?;
    if palette_end > dib.len() {
        return Err(CursorDecodeError::Truncated);
    }
    let xor_stride = stride(width, bpp).ok_or(CursorDecodeError::Format)?;
    let and_stride = stride(width, 1).ok_or(CursorDecodeError::Format)?;
    let xor_bytes = xor_stride.checked_mul(height).ok_or(CursorDecodeError::Format)?;
    let and_start = palette_end.checked_add(xor_bytes).ok_or(CursorDecodeError::Format)?;
    let and_bytes = and_stride.checked_mul(height).ok_or(CursorDecodeError::Format)?;
    let end = and_start.checked_add(and_bytes).ok_or(CursorDecodeError::Format)?;
    if end > dib.len() {
        return Err(CursorDecodeError::Truncated);
    }
    let rgba_len = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(CursorDecodeError::Allocation)?;
    let mut rgba = Vec::new();
    rgba.try_reserve_exact(rgba_len)
        .map_err(|_| CursorDecodeError::Allocation)?;
    rgba.resize(rgba_len, 0);

    // In an alpha cursor, all 32-bit alpha values are meaningful, including
    // zero.  Falling back to the AND plane per pixel would turn deliberately
    // transparent pixels opaque merely because a neighbouring pixel used
    // alpha blending.
    let has_alpha_plane = bpp == 32
        && (0..height).any(|row| {
            let row_start = palette_end + row * xor_stride;
            (0..width).any(|column| dib[row_start + column * 4 + 3] != 0)
        });

    for y in 0..height {
        // DIB scanlines are bottom-up because a resource cursor has a positive
        // height. The AND plane follows the same ordering.
        let source_y = height - 1 - y;
        let xor_row = palette_end + source_y * xor_stride;
        let and_row = and_start + source_y * and_stride;
        for x in 0..width {
            let (red, green, blue, source_alpha) = match bpp {
                32 => {
                    let offset = xor_row + x * 4;
                    (dib[offset + 2], dib[offset + 1], dib[offset], dib[offset + 3])
                }
                24 => {
                    let offset = xor_row + x * 3;
                    (dib[offset + 2], dib[offset + 1], dib[offset], 0)
                }
                8 => palette_pixel(dib, palette_end, dib[xor_row + x] as usize)?,
                4 => {
                    let packed = dib[xor_row + x / 2];
                    let index = if x & 1 == 0 { packed >> 4 } else { packed & 0x0f };
                    palette_pixel(dib, palette_end, usize::from(index))?
                }
                1 => {
                    let index = (dib[xor_row + x / 8] >> (7 - (x & 7))) & 1;
                    palette_pixel(dib, palette_end, usize::from(index))?
                }
                _ => return Err(CursorDecodeError::Format),
            };
            let and_bit = (dib[and_row + x / 8] >> (7 - (x & 7))) & 1 != 0;
            let output = (y * width + x) * 4;
            rgba[output] = red;
            rgba[output + 1] = green;
            rgba[output + 2] = blue;
            rgba[output + 3] = if has_alpha_plane {
                source_alpha
            } else if !and_bit {
                255
            } else if red == 0 && green == 0 && blue == 0 {
                0
            } else {
                return Err(CursorDecodeError::XorComposite);
            };
        }
    }
    Ok(DecodedCursor { width: width as u32, height: height as u32, hotspot_x, hotspot_y, rgba })
}

fn palette_pixel(
    dib: &[u8],
    palette_end: usize,
    index: usize,
) -> Result<(u8, u8, u8, u8), CursorDecodeError> {
    let offset = 40usize
        .checked_add(index.checked_mul(4).ok_or(CursorDecodeError::Format)?)
        .ok_or(CursorDecodeError::Format)?;
    if offset.checked_add(4).ok_or(CursorDecodeError::Format)? > palette_end {
        return Err(CursorDecodeError::Format);
    }
    Ok((dib[offset + 2], dib[offset + 1], dib[offset], 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(width: i32, visible_height: i32, bpp: u16) -> Vec<u8> {
        let mut bytes = vec![0; 40];
        bytes[0..4].copy_from_slice(&40u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&width.to_le_bytes());
        bytes[8..12].copy_from_slice(&(visible_height * 2).to_le_bytes());
        bytes[12..14].copy_from_slice(&1u16.to_le_bytes());
        bytes[14..16].copy_from_slice(&bpp.to_le_bytes());
        bytes
    }

    #[test]
    fn decodes_bottom_up_32bpp_and_honors_hotspot() {
        let mut dib = header(2, 2, 32);
        // Stored bottom row: blue then green. Stored top row: red then white.
        dib.extend_from_slice(&[255, 0, 0, 0, 0, 255, 0, 0]);
        dib.extend_from_slice(&[0, 0, 255, 0, 255, 255, 255, 0]);
        dib.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
        let cursor = decode_cursor_dib(2, 2, 1, 1, &dib).unwrap();
        assert_eq!((cursor.hotspot_x, cursor.hotspot_y), (1, 1));
        assert_eq!(cursor.rgba, vec![255, 0, 0, 255, 255, 255, 255, 255, 0, 0, 255, 255, 0, 255, 0, 255]);
    }

    #[test]
    fn alpha_cursor_preserves_zero_alpha_for_the_entire_image() {
        let mut dib = header(2, 1, 32);
        dib.extend_from_slice(&[1, 2, 3, 0, 4, 5, 6, 128]);
        dib.extend_from_slice(&[0, 0, 0, 0]);
        let cursor = decode_cursor_dib(2, 1, 0, 0, &dib).unwrap();
        assert_eq!(cursor.rgba, vec![3, 2, 1, 0, 6, 5, 4, 128]);
    }

    #[test]
    fn decodes_palette_and_and_mask_transparency() {
        let mut dib = header(2, 1, 1);
        // palette: index zero black, index one red (BGRA records).
        dib.extend_from_slice(&[0, 0, 0, 0, 0, 0, 255, 0]);
        dib.extend_from_slice(&[0b1000_0000, 0, 0, 0]);
        // x=1 has AND=1 and black XOR, the defined transparent case.
        dib.extend_from_slice(&[0b0100_0000, 0, 0, 0]);
        let cursor = decode_cursor_dib(2, 1, 0, 0, &dib).unwrap();
        assert_eq!(cursor.rgba, vec![255, 0, 0, 255, 0, 0, 0, 0]);
    }

    #[test]
    fn rejects_xor_composite_pixels_without_a_background() {
        let mut dib = header(1, 1, 1);
        dib.extend_from_slice(&[0, 0, 0, 0, 0, 0, 255, 0]);
        dib.extend_from_slice(&[0b1000_0000, 0, 0, 0]);
        dib.extend_from_slice(&[0b1000_0000, 0, 0, 0]);
        assert_eq!(decode_cursor_dib(1, 1, 0, 0, &dib), Err(CursorDecodeError::XorComposite));
    }

    #[test]
    fn rejects_non_cursor_height() {
        let mut dib = header(1, 1, 32);
        dib[8..12].copy_from_slice(&1i32.to_le_bytes());
        assert_eq!(decode_cursor_dib(1, 1, 0, 0, &dib), Err(CursorDecodeError::Dimensions));
    }

    #[test]
    fn rejects_out_of_bounds_hotspot_and_oversized_cursor() {
        let dib = header(1, 1, 32);
        assert_eq!(decode_cursor_dib(1, 1, 1, 0, &dib), Err(CursorDecodeError::Dimensions));
        let dib = header(257, 1, 32);
        assert_eq!(decode_cursor_dib(257, 1, 0, 0, &dib), Err(CursorDecodeError::Dimensions));
    }
}
