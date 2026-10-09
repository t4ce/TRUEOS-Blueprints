//! Opaque compact-window logo, decoded once from bytes embedded in the Blueprint.
pub const SIZE: u32 = 128;
const JPEG: &[u8] = include_bytes!("logo.jpg");
pub fn decode() -> image::ImageResult<image::RgbaImage> {
    image::load_from_memory_with_format(JPEG, image::ImageFormat::Jpeg)
        .map(|image| image.to_rgba8())
}
#[cfg(test)]
mod tests {
    #[test]
    fn embedded_logo_is_small_opaque_and_exactly_the_compact_frame_size() {
        let image = super::decode().unwrap();
        assert_eq!(image.dimensions(), (super::SIZE, super::SIZE));
        assert!(image.pixels().all(|pixel| pixel[3] == 255));
        assert!(super::JPEG.len() < 8192);
    }
}
