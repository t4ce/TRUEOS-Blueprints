//! Transparent compact-window logo, decoded once from bytes embedded in the Blueprint.
pub const SIZE: u32 = 128;
const PNG: &[u8] = include_bytes!("logo.png");
pub fn decode() -> image::ImageResult<image::RgbaImage> {
    image::load_from_memory_with_format(PNG, image::ImageFormat::Png).map(|image| image.to_rgba8())
}
#[cfg(test)]
mod tests {
    #[test]
    fn embedded_logo_is_transparent_and_exactly_the_compact_frame_size() {
        let image = super::decode().unwrap();
        assert_eq!(image.dimensions(), (super::SIZE, super::SIZE));
        assert!(image.pixels().any(|pixel| pixel[3] == 0));
        assert!(image.pixels().any(|pixel| pixel[3] > 0 && pixel[3] < 255));
        assert!(image.pixels().any(|pixel| pixel[3] == 255));
        assert!(super::PNG.len() < 32768);
    }
}
