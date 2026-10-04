//! One full opaque 1:1 quad meets UI4's retained BCS0 copy eligibility.
use trueos::ui4_scene::{Damage, Error, Frame, SpriteCorner, SpriteQuad, rgba};
const MAP_SPRITE: u32 = 1;
fn viewport_quad(width: u32, height: u32) -> SpriteQuad {
    SpriteQuad {
        sprite_id: MAP_SPRITE,
        c0: SpriteCorner {
            x: 0.0,
            y: 0.0,
            u: 0.0,
            v: 0.0,
        },
        c1: SpriteCorner {
            x: width as f32,
            y: 0.0,
            u: 1.0,
            v: 0.0,
        },
        c2: SpriteCorner {
            x: width as f32,
            y: height as f32,
            u: 1.0,
            v: 1.0,
        },
        c3: SpriteCorner {
            x: 0.0,
            y: height as f32,
            u: 0.0,
            v: 1.0,
        },
        color_rgba: rgba(255, 255, 255, 255),
        source_over: false,
    }
}
fn retry(mut operation: impl FnMut() -> Result<(), Error>) -> Result<(), Error> {
    loop {
        match operation() {
            Err(Error::Busy) => {
                trueos::vsys::poll_once();
                trueos::vsys::sleep_ms(1);
            }
            result => return result,
        }
    }
}
pub fn present(frame: &mut Frame, pixels: &image::RgbaImage) -> Result<(), Error> {
    if pixels.dimensions() != (frame.width(), frame.height())
        || pixels.pixels().any(|p| p[3] != 255)
    {
        return Err(Error::Invalid);
    }
    // Same sprite ID replaces the retained source instead of accumulating uploads.
    retry(|| {
        frame.upload_sprite_rgba8(MAP_SPRITE, pixels.width(), pixels.height(), pixels.as_raw())
    })?;
    retry(|| frame.begin_sprite_frame(rgba(230, 230, 230, 255)))?;
    let quad = viewport_quad(frame.width(), frame.height());
    retry(|| frame.draw_sprite_quads(std::slice::from_ref(&quad)))?;
    let damage = Damage::full(frame.width(), frame.height());
    retry(|| frame.publish(damage))
}
