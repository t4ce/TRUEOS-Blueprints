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
    if rc == 0 { Some((width, height)) } else { None }
}

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

pub fn upload_texture_rgba_image_async(
    tex_id: u32,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> bool {
    if tex_id == 0 || width == 0 || height == 0 {
        return false;
    }
    unsafe {
        vcabi::trueos_cabi_gfx_upload_texture_rgba_image_async(
            tex_id,
            width,
            height,
            pixels.as_ptr(),
            pixels.len(),
        ) == 0
    }
}

pub fn ensure_texture_rgba_now(tex_id: u32, width: u32, height: u32, fill_rgba: [u8; 4]) -> bool {
    if matches!(texture_dimensions(tex_id), Some((w, h)) if w == width && h == height) {
        return true;
    }
    let Some(pixels) = solid_rgba_pixels(width, height, fill_rgba) else {
        return false;
    };
    upload_texture_rgba_image_now(tex_id, width, height, pixels.as_slice())
}

#[inline]
pub fn upload_png_to_texture(tex_id: u32, png: &[u8]) -> i32 {
    if png.is_empty() {
        return -1;
    }
    unsafe { vcabi::trueos_cabi_gfx_upload_texture_png(tex_id, png.as_ptr(), png.len()) }
}

#[inline]
pub fn upload_png_to_texture_async(tex_id: u32, png: &[u8]) -> i32 {
    if png.is_empty() {
        return -1;
    }
    unsafe { vcabi::trueos_cabi_gfx_upload_texture_png_async(tex_id, png.as_ptr(), png.len()) }
}

#[inline]
pub fn probe_upload_png_to_texture_async(tex_id: u32) -> i32 {
    unsafe { vcabi::trueos_cabi_gfx_upload_texture_png_async(tex_id, core::ptr::null(), 0) }
}

#[inline]
pub fn upload_jpeg_to_texture(tex_id: u32, jpeg: &[u8]) -> i32 {
    if jpeg.is_empty() {
        return -1;
    }
    unsafe { vcabi::trueos_cabi_gfx_upload_texture_jpeg(tex_id, jpeg.as_ptr(), jpeg.len()) }
}

#[inline]
pub fn upload_jpeg_to_texture_async(tex_id: u32, jpeg: &[u8]) -> i32 {
    if jpeg.is_empty() {
        return -1;
    }
    unsafe { vcabi::trueos_cabi_gfx_upload_texture_jpeg_async(tex_id, jpeg.as_ptr(), jpeg.len()) }
}

#[inline]
pub fn probe_upload_jpeg_to_texture_async(tex_id: u32) -> i32 {
    unsafe { vcabi::trueos_cabi_gfx_upload_texture_jpeg_async(tex_id, core::ptr::null(), 0) }
}

#[inline]
pub fn upload_svg_to_texture(tex_id: u32, svg: &[u8]) -> i32 {
    if svg.is_empty() {
        return -1;
    }
    unsafe { vcabi::trueos_cabi_gfx_upload_texture_svg(tex_id, svg.as_ptr(), svg.len()) }
}

#[inline]
pub fn upload_svg_to_texture_async(tex_id: u32, svg: &[u8]) -> i32 {
    if svg.is_empty() {
        return -1;
    }
    unsafe { vcabi::trueos_cabi_gfx_upload_texture_svg_async(tex_id, svg.as_ptr(), svg.len()) }
}

#[inline]
pub fn probe_upload_svg_to_texture_async(tex_id: u32) -> i32 {
    unsafe { vcabi::trueos_cabi_gfx_upload_texture_svg_async(tex_id, core::ptr::null(), 0) }
}

#[inline]
pub fn texture_status(tex_id: u32) -> i32 {
    unsafe { vcabi::trueos_cabi_gfx_texture_status(tex_id) }
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
    let width = width.max(1);
    let height = height.max(1);
    if !matches!(texture_dimensions(tex_id), Some((w, h)) if w == width && h == height) {
        let Some(pixels) = solid_rgba_pixels(width, height, clear_rgba) else {
            return false;
        };
        if !upload_texture_rgba_image_async(tex_id, width, height, pixels.as_slice()) {
            return false;
        }
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

pub fn render_tex_triangles_to_texture(
    target_tex_id: u32,
    source_tex_id: u32,
    clear_rgb: u32,
    repaint_window_id: u32,
    vertices: &[u8],
) -> bool {
    if target_tex_id == 0 || source_tex_id == 0 {
        return false;
    }
    if vertices.is_empty() {
        return true;
    }
    unsafe {
        vcabi::trueos_cabi_gfx_queue_render_tex_triangles_to_texture(
            target_tex_id,
            source_tex_id,
            clear_rgb,
            vertices.as_ptr(),
            vertices.len(),
            repaint_window_id,
        ) == 0
    }
}

pub fn render_mandelbrot_to_texture(
    tex_id: u32,
    ticks: u64,
    tick_hz: u64,
    repaint_window_id: u32,
) -> bool {
    if tex_id == 0 {
        return false;
    }
    unsafe {
        vcabi::trueos_cabi_gfx_queue_render_mandelbrot_to_texture(
            tex_id,
            ticks,
            tick_hz,
            repaint_window_id,
        ) == 0
    }
}

fn solid_rgba_pixels(width: u32, height: u32, rgba: [u8; 4]) -> Option<Vec<u8>> {
    let pixel_count = (width as usize).checked_mul(height as usize)?;
    let byte_len = pixel_count.checked_mul(4)?;
    let mut pixels = Vec::with_capacity(byte_len);
    for _ in 0..pixel_count {
        pixels.extend_from_slice(&rgba);
    }
    Some(pixels)
}
