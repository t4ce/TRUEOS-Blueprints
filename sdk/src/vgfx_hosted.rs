extern crate alloc;

use alloc::vec::Vec;

use crate::vcabi;

#[inline]
pub fn upload_texture_rgba_image_now(tex_id: u32, width: u32, height: u32, pixels: &[u8]) -> bool {
    if tex_id == 0 || width == 0 || height == 0 {
        return false;
    }
    unsafe {
        vcabi::trueos_cabi_gfx_upload_texture_rgba_image(
            tex_id,
            width,
            height,
            pixels.as_ptr(),
            pixels.len(),
        ) == 0
    }
}

pub fn ensure_texture_rgba_now(tex_id: u32, width: u32, height: u32, fill_rgba: [u8; 4]) -> bool {
    if matches!(
        crate::vgfx::texture_dimensions(tex_id),
        Some((w, h)) if w == width && h == height
    ) {
        return true;
    }
    let Some(pixel_count) = (width as usize).checked_mul(height as usize) else {
        return false;
    };
    let Some(byte_len) = pixel_count.checked_mul(4) else {
        return false;
    };
    let mut pixels = Vec::with_capacity(byte_len);
    for _ in 0..pixel_count {
        pixels.extend_from_slice(&fill_rgba);
    }
    upload_texture_rgba_image_now(tex_id, width, height, pixels.as_slice())
}
