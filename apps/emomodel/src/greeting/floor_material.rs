pub(super) const CLEAR_RGBA: u32 = 0;
pub(super) const SWATCH_ROWS: usize = 4;
const SWATCHES: [[u8; 3]; 3] = [
    [0x87, 0x53, 0xbd], // Hair
    [0xd0, 0xc8, 0xbe], // Mouth
    [0, 0, 0],          // Pupil
];
pub(super) struct Palette {
    size: [usize; 2],
    pixels: Vec<[u8; 4]>,
}

impl Palette {
    pub(super) fn new(width: usize, height: usize) -> Self {
        let mut image = Self {
            size: [width, height],
            pixels: vec![[0; 4]; width * height],
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

    #[cfg(target_os = "trueos")]
    pub(super) fn pupil(&self) -> [f32; 2] {
        self.swatch(2)
    }

    pub(super) fn texel_uv(&self, x: usize, y: usize) -> [f32; 2] {
        [
            (x as f32 + 0.5) / self.size[0] as f32,
            (y as f32 + 0.5) / self.size[1] as f32,
        ]
    }

    pub(super) fn set(&mut self, x: usize, y: usize, [r, g, b]: [u8; 3]) {
        self.set_rgba(x, y, [r, g, b, 255]);
    }

    pub(super) fn set_rgba(&mut self, x: usize, y: usize, rgba: [u8; 4]) {
        self.pixels[y * self.size[0] + x] = rgba;
    }

    #[cfg(test)]
    pub(super) fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        self.pixels[y * self.size[0] + x]
    }

    #[cfg(target_os = "trueos")]
    pub(super) fn png(&self) -> Result<Vec<u8>, png::EncodingError> {
        // The native BMP decoder treats a zero alpha byte as opaque RGBX.
        let [width, height] = self.extent();
        let mut output = Vec::new();
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fastest);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(self.pixels.as_flattened())?;
        writer.finish()?;
        Ok(output)
    }

    #[cfg(target_os = "trueos")]
    pub(super) fn extent(&self) -> [u32; 2] {
        self.size.map(|axis| axis as u32)
    }

    #[cfg(target_os = "trueos")]
    pub(super) fn write_rgba(&self, output: &mut Vec<u8>) {
        output.clear();
        output.extend_from_slice(self.pixels.as_flattened());
    }
}

pub(super) fn shadow_colors() -> [[u8; 4]; 256] {
    std::array::from_fn(|alpha| {
        let linear = 1.0 - alpha as f32 / 255.0;
        let srgb = if linear <= 0.003_130_8 {
            12.92 * linear
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        };
        // Match the old apparent darkness over white with an alpha mask that
        // also composes over any other UI4 background.
        [0, 0, 0, 255 - (255.0 * srgb).round() as u8]
    })
}
