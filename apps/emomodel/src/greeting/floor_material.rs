// These all-white clear values are represented exactly by the native clear shader.
pub(super) const CLEAR_RGB: [u8; 3] = [255; 3];
pub(super) const CLEAR_RGBA: u32 =
    u32::from_le_bytes([CLEAR_RGB[0], CLEAR_RGB[1], CLEAR_RGB[2], 255]);
pub(super) const SWATCH_ROWS: usize = 4;
const SWATCHES: [[u8; 3]; 3] = [
    [0x87, 0x53, 0xbd], // Hair
    [0xd0, 0xc8, 0xbe], // Mouth
    [0x2d, 0x23, 0x35], // Iris
];
const HEADER: usize = 54;

pub(super) struct Palette {
    size: [usize; 2],
    bmp: Vec<u8>,
}

impl Palette {
    pub(super) fn new(width: usize, height: usize) -> Self {
        // Atlas widths are multiples of four, so BMP rows need no padding.
        let mut bmp = vec![CLEAR_RGB[0]; HEADER + width * height * 3];
        bmp[..HEADER].fill(0);
        bmp[..2].copy_from_slice(b"BM");
        for (offset, value) in [
            (2, bmp.len() as u32),
            (10, HEADER as u32),
            (14, 40),
            (18, width as u32),
            (22, height as u32),
            (34, (width * height * 3) as u32),
        ] {
            bmp[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bmp[26..30].copy_from_slice(&[1, 0, 24, 0]);
        let mut image = Self {
            size: [width, height],
            bmp,
        };
        for (slot, color) in SWATCHES.into_iter().enumerate() {
            for y in 0..SWATCH_ROWS {
                for x in slot * 4..(slot + 1) * 4 {
                    image.set(x, y, color);
                }
            }
        }
        image
    }

    pub(super) fn swatch(&self, slot: usize) -> [f32; 2] {
        self.texel_uv(slot * 4 + 2, SWATCH_ROWS / 2)
    }

    pub(super) fn texel_uv(&self, x: usize, y: usize) -> [f32; 2] {
        [
            (x as f32 + 0.5) / self.size[0] as f32,
            (y as f32 + 0.5) / self.size[1] as f32,
        ]
    }

    fn pixel_offset(&self, x: usize, y: usize) -> usize {
        // Positive-height BMP rows are stored bottom first.
        HEADER + 3 * ((self.size[1] - 1 - y) * self.size[0] + x)
    }

    pub(super) fn set(&mut self, x: usize, y: usize, [r, g, b]: [u8; 3]) {
        let offset = self.pixel_offset(x, y);
        self.bmp[offset..offset + 3].copy_from_slice(&[b, g, r]);
    }

    #[cfg(test)]
    pub(super) fn pixel(&self, x: usize, y: usize) -> [u8; 3] {
        let offset = self.pixel_offset(x, y);
        [self.bmp[offset + 2], self.bmp[offset + 1], self.bmp[offset]]
    }

    pub(super) fn bmp(&self) -> &[u8] {
        &self.bmp
    }

    #[cfg(target_os = "trueos")]
    pub(super) fn extent(&self) -> [u32; 2] {
        self.size.map(|axis| axis as u32)
    }

    #[cfg(target_os = "trueos")]
    pub(super) fn write_rgba(&self, output: &mut Vec<u8>) {
        output.clear();
        output.reserve(self.size[0] * self.size[1] * 4);
        for row in self.bmp[HEADER..].chunks_exact(3 * self.size[0]).rev() {
            output.extend(
                row.chunks_exact(3)
                    .flat_map(|bgr| [bgr[2], bgr[1], bgr[0], 255]),
            );
        }
    }
}

pub(super) fn shadow_colors() -> [[u8; 3]; 256] {
    std::array::from_fn(|alpha| {
        let linear = 1.0 - alpha as f32 / 255.0;
        let srgb = if linear <= 0.003_130_8 {
            12.92 * linear
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        };
        [(255.0 * srgb).round() as u8; 3]
    })
}
