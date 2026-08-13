use std::sync::Mutex;

use trueos::ui4_scene::{Frame, SpriteCorner, SpriteQuad, rgba};
use v::bp_abi::TrueosUi4SpriteQuad;

const WINDOW_ID: u32 = 0x51A4;

static DRAWN_QUADS: Mutex<Vec<TrueosUi4SpriteQuad>> = Mutex::new(Vec::new());
static UPLOADED_SPRITE: Mutex<Option<(u32, u32, u32, Vec<u8>)>> = Mutex::new(None);

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_write(_stream: u32, _bytes: *const u8, _len: usize) {}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_blueprint_shutdown(_data_ptr: *const u8, _data_len: usize) -> i32 {
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui4_solara_frame_open(
    _x: i32,
    _y: i32,
    width: u32,
    height: u32,
) -> u32 {
    assert_eq!((width, height), (320, 200));
    WINDOW_ID
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui4_scene_sprite_upload_rgba8(
    window_id: u32,
    sprite_id: u32,
    width: u32,
    height: u32,
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    assert_eq!(window_id, WINDOW_ID);
    let bytes = unsafe { std::slice::from_raw_parts(data_ptr, data_len) };
    *UPLOADED_SPRITE.lock().unwrap() = Some((sprite_id, width, height, bytes.to_vec()));
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui4_scene_sprite_quads(
    window_id: u32,
    quads: *const TrueosUi4SpriteQuad,
    quad_count: usize,
) -> i32 {
    assert_eq!(window_id, WINDOW_ID);
    let quads = unsafe { std::slice::from_raw_parts(quads, quad_count) };
    DRAWN_QUADS.lock().unwrap().extend_from_slice(quads);
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui4_solara_frame_close(window_id: u32) -> i32 {
    assert_eq!(window_id, WINDOW_ID);
    0
}

fn axis_aligned_quad(sprite_id: u32, width: f32, height: f32, color_rgba: u32) -> SpriteQuad {
    let sampled = sprite_id != 0;
    SpriteQuad {
        sprite_id,
        c0: SpriteCorner::default(),
        c1: SpriteCorner {
            x: width,
            u: f32::from(sampled),
            ..SpriteCorner::default()
        },
        c2: SpriteCorner {
            x: width,
            y: height,
            u: f32::from(sampled),
            v: f32::from(sampled),
        },
        c3: SpriteCorner {
            y: height,
            v: f32::from(sampled),
            ..SpriteCorner::default()
        },
        color_rgba,
        source_over: true,
    }
}

#[test]
fn public_bridge_preserves_straight_alpha_and_dom_paint_order() {
    DRAWN_QUADS.lock().unwrap().clear();
    *UPLOADED_SPRITE.lock().unwrap() = None;

    let mut frame = Frame::open(10, 20, 320, 200).unwrap();
    let border_rgba = [0x18, 0x45, 0x73, 0x80];
    frame.upload_sprite_rgba8(27, 1, 1, &border_rgba).unwrap();

    let background = axis_aligned_quad(0, 320.0, 200.0, rgba(250, 249, 247, 255));
    // Picasso V0 lowers a rounded edge into bounded solid spans. The retained
    // upload above independently locks down the straight-alpha sprite fallback.
    let border = axis_aligned_quad(0, 300.0, 1.0, rgba(24, 69, 115, 128));
    let font = frame.font_canvas_quad((640, 400), (32, 48)).unwrap();
    frame
        .draw_sprite_quads(&[background, border, font])
        .unwrap();

    assert_eq!(
        UPLOADED_SPRITE.lock().unwrap().as_ref(),
        Some(&(27, 1, 1, border_rgba.to_vec())),
        "the safe facade must transport straight-alpha sprite bytes unchanged",
    );

    let raw = DRAWN_QUADS.lock().unwrap();
    assert_eq!(raw.len(), 3);
    assert_eq!(
        raw.iter().map(|quad| quad.sprite_id).collect::<Vec<_>>(),
        [0, 0, u32::MAX],
        "DOM paint order must survive the public SpriteQuad lowering",
    );
    assert_eq!(
        raw.iter().map(|quad| quad.flags).collect::<Vec<_>>(),
        [1, 1, 1],
        "background, blended border, and premultiplied font canvas all use source-over",
    );
    assert_eq!(raw[0].color_rgba, u32::from_le_bytes([250, 249, 247, 255]));
    assert_eq!(raw[1].color_rgba, u32::from_le_bytes([24, 69, 115, 128]));
    assert_eq!((raw[2].c0_x, raw[2].c0_y), (0.0, 0.0));
    assert_eq!((raw[2].c2_x, raw[2].c2_y), (320.0, 200.0));
    assert_eq!((raw[2].c0_u, raw[2].c0_v), (0.05, 0.12));
    assert_eq!((raw[2].c2_u, raw[2].c2_v), (0.55, 0.62));
}
