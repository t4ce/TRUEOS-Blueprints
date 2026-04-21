extern crate alloc;

use alloc::{string::String, vec, vec::Vec};

use crate::vcabi;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct RgbVertex {
    pub x: f32,
    pub y: f32,
    pub color: [u8; 4],
}

impl RgbVertex {
    #[inline]
    pub const fn new(x: f32, y: f32, color: [u8; 4]) -> Self {
        Self { x, y, color }
    }
}

#[inline]
pub fn capture_screenshot_data_url() -> Option<String> {
    let len =
        unsafe { vcabi::trueos_cabi_gfx_capture_screenshot_data_url(core::ptr::null_mut(), 0) };
    if len <= 0 {
        return None;
    }

    let mut bytes = vec![0u8; len as usize];
    let got = unsafe {
        vcabi::trueos_cabi_gfx_capture_screenshot_data_url(bytes.as_mut_ptr(), bytes.len())
    };
    if got <= 0 {
        return None;
    }
    bytes.truncate(got as usize);
    String::from_utf8(bytes).ok()
}

#[inline]
pub fn texture_dimensions(tex_id: u32) -> Option<(u32, u32)> {
    let mut width = 0u32;
    let mut height = 0u32;
    let rc = unsafe {
        vcabi::trueos_cabi_gfx_texture_dimensions(
            tex_id,
            &mut width as *mut u32,
            &mut height as *mut u32,
        )
    };
    if rc == 0 {
        Some((width, height))
    } else {
        None
    }
}

pub fn upload_texture_rgba_image(tex_id: u32, width: u32, height: u32, pixels: &[u8]) -> bool {
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

pub fn ensure_texture_rgba(tex_id: u32, width: u32, height: u32, fill_rgba: [u8; 4]) -> bool {
    if matches!(texture_dimensions(tex_id), Some((w, h)) if w == width && h == height) {
        return true;
    }
    let pixels = vec![fill_rgba; (width as usize).saturating_mul(height as usize)]
        .into_iter()
        .flatten()
        .collect::<Vec<u8>>();
    upload_texture_rgba_image(tex_id, width, height, pixels.as_slice())
}

pub fn render_rgb_triangles_to_texture(
    tex_id: u32,
    width: u32,
    height: u32,
    clear_rgb: u32,
    repaint_window_id: u32,
    vertices: &[RgbVertex],
) -> bool {
    let clear_rgba = [
        ((clear_rgb >> 16) & 0xFF) as u8,
        ((clear_rgb >> 8) & 0xFF) as u8,
        (clear_rgb & 0xFF) as u8,
        0xFF,
    ];
    if !ensure_texture_rgba(tex_id, width.max(1), height.max(1), clear_rgba) {
        return false;
    }

    let bytes = unsafe {
        core::slice::from_raw_parts(
            vertices.as_ptr() as *const u8,
            core::mem::size_of_val(vertices),
        )
    };
    unsafe {
        vcabi::trueos_cabi_gfx_queue_render_rgb_triangles_to_texture(
            tex_id,
            clear_rgb,
            bytes.as_ptr(),
            bytes.len(),
            repaint_window_id,
        ) == 0
    }
}
