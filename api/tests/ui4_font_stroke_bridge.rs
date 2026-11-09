use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use trueos::ui4_scene::{Error, Font, FontSpriteRequest, FontSpriteTicket, Frame};

static RESULT: AtomicI32 = AtomicI32::new(0);
static CALLS: AtomicUsize = AtomicUsize::new(0);
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui4_solara_frame_open(_: i32, _: i32, _: u32, _: u32) -> u32 { 42 }
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui4_solara_frame_close(_: u32) -> i32 { 0 }
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_write(_: u32, _: *const u8, _: usize) {}
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_blueprint_shutdown(_: *const u8, _: usize) -> i32 { 0 }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui4_scene_font_sprite_request_v2(
    window: u32, font: u32, scalar: u32, pixels: f32, color: u32,
    stroke: f32, out: *mut u64,
) -> i32 {
    assert_eq!((window, font, scalar, pixels, color, stroke),
               (42, 1, 'Z' as u32, 64.0, 0xff000000, 2.5));
    CALLS.fetch_add(1, Ordering::Relaxed);
    unsafe { out.write(27); }
    RESULT.load(Ordering::Relaxed)
}

#[test]
fn stroke_is_independent_of_texture_height_and_busy_can_be_retried() {
    let mut frame = Frame::open(0, 0, 800, 600).unwrap();
    let request = FontSpriteRequest { font: Font::Default, scalar: 'Z',
                                      font_pixels: 64.0, color_rgba: 0xff000000 };
    RESULT.store(-7, Ordering::Relaxed);
    assert_eq!(frame.request_font_sprite_with_stroke(request, 2.5), Err(Error::Busy));
    RESULT.store(0, Ordering::Relaxed);
    assert_eq!(frame.request_font_sprite_with_stroke(request, 2.5), Ok(FontSpriteTicket(27)));
    for stroke in [-1.0, 8.1, f32::NAN] {
        assert_eq!(frame.request_font_sprite_with_stroke(request, stroke), Err(Error::Invalid));
    }
    let invalid = FontSpriteRequest { font_pixels: f32::INFINITY, ..request };
    assert_eq!(frame.request_font_sprite_with_stroke(invalid, 2.5), Err(Error::Invalid));
    assert_eq!(CALLS.load(Ordering::Relaxed), 2);
}
