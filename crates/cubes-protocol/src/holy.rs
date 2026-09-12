//! Sparse 48x48 RGBA-frame asset shared by CubeSrv and Cubes.

pub const MAGIC: &[u8; 4] = b"HFX1";
pub const VERSION: u8 = 1;
pub const WIDTH: u8 = 48;
pub const HEIGHT: u8 = 48;
pub const PERIOD_MS: u16 = 100;
pub const HEADER: usize = 12;
pub const MAX_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pixel {
    pub x: u8,
    pub y: u8,
    pub palette: u8,
}

#[derive(Clone, Copy)]
pub struct Sequence<'a> {
    bytes: &'a [u8],
    frames: u8,
    palette: u8,
    offsets: usize,
    data: usize,
}

impl<'a> Sequence<'a> {
    pub fn parse(bytes: &'a [u8]) -> Option<Self> {
        if bytes.len() < HEADER || bytes.len() > MAX_BYTES || &bytes[..4] != MAGIC
            || bytes[4] != VERSION || bytes[5] != WIDTH || bytes[6] != HEIGHT
            || bytes[7] == 0 || u16::from_le_bytes(bytes[8..10].try_into().ok()?) != PERIOD_MS
            || bytes[10] == 0 || bytes[11] != 0
        {
            return None;
        }
        let frames = bytes[7];
        let palette = bytes[10];
        let offsets = HEADER.checked_add(palette as usize * 3)?;
        let data = offsets.checked_add((frames as usize + 1) * 4)?;
        if data > bytes.len() || read_u32(bytes, offsets)? != 0 {
            return None;
        }
        let payload_len = bytes.len() - data;
        let mut previous = 0usize;
        for frame in 0..=frames as usize {
            let offset = read_u32(bytes, offsets + frame * 4)? as usize;
            if offset < previous || offset > payload_len || offset % 3 != 0 {
                return None;
            }
            previous = offset;
        }
        if previous != payload_len {
            return None;
        }
        let sequence = Self { bytes, frames, palette, offsets, data };
        for frame in 0..frames {
            if sequence.pixels(frame).any(|pixel| pixel.x >= WIDTH || pixel.y >= HEIGHT
                || pixel.palette >= palette)
            {
                return None;
            }
        }
        Some(sequence)
    }

    pub const fn frame_count(self) -> u8 { self.frames }
    pub const fn palette_count(self) -> u8 { self.palette }
    pub fn palette(self, index: u8) -> Option<[u8; 3]> {
        if index >= self.palette { return None; }
        let start = HEADER + index as usize * 3;
        Some(self.bytes[start..start + 3].try_into().unwrap())
    }
    pub fn frame(self, index: u8) -> Option<&'a [u8]> {
        if index >= self.frames { return None; }
        let start = read_u32(self.bytes, self.offsets + index as usize * 4)? as usize;
        let end = read_u32(self.bytes, self.offsets + (index as usize + 1) * 4)? as usize;
        Some(&self.bytes[self.data + start..self.data + end])
    }
    pub fn pixels(self, index: u8) -> impl Iterator<Item = Pixel> + 'a {
        self.frame(index).unwrap_or_default().chunks_exact(3).map(|record| Pixel {
            x: record[0], y: record[1], palette: record[2],
        })
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?))
}

pub fn decode_frame(bytes: &[u8]) -> Option<impl Iterator<Item = Pixel> + '_> {
    if bytes.len() % 3 != 0 { return None; }
    let pixels = bytes.chunks_exact(3).map(|record| Pixel {
        x: record[0], y: record[1], palette: record[2],
    });
    bytes.chunks_exact(3).all(|record| record[0] < WIDTH && record[1] < HEIGHT)
        .then_some(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{vec, vec::Vec};

    #[test]
    fn sparse_sequence_is_bounded_and_indexed() {
        let mut bytes = b"HFX1\x01\x30\x30\x02\x64\x00\x02\x00".to_vec();
        bytes.extend_from_slice(&[10, 20, 30, 40, 50, 60]);
        bytes.extend_from_slice(&[0u32, 6, 9].into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>());
        bytes.extend_from_slice(&[1, 2, 0, 47, 47, 1, 3, 4, 1]);
        let sequence = Sequence::parse(&bytes).unwrap();
        assert_eq!(sequence.frame_count(), 2);
        assert_eq!(sequence.palette(1), Some([40, 50, 60]));
        assert_eq!(sequence.pixels(0).collect::<Vec<_>>(), vec![
            Pixel { x: 1, y: 2, palette: 0 }, Pixel { x: 47, y: 47, palette: 1 },
        ]);
        assert_eq!(sequence.pixels(1).collect::<Vec<_>>(), vec![Pixel { x: 3, y: 4, palette: 1 }]);
    }
}
