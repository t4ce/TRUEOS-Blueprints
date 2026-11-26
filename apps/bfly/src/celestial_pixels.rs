//! Prepare premultiplied RGBA8 backgrounds from the retained artwork.

extern crate alloc;

use alloc::{vec, vec::Vec};

use crate::celestial::{Body, CELESTIAL_TEXTURE_SIDE, Sprite, SpriteRect};

const TEXTURE_SIDE: usize = CELESTIAL_TEXTURE_SIDE as usize;
const MASK_BYTES: usize = TEXTURE_SIDE * TEXTURE_SIDE;
const MOON: &[u8; MASK_BYTES] = include_bytes!("../shaders/trueos/bfly-moon.r8");
const EARTH: &[u8; MASK_BYTES] = include_bytes!("../shaders/trueos/bfly-earth.r8");
const SUN: &[u8; MASK_BYTES * 4] = include_bytes!("../shaders/trueos/bfly-sun.rgba8");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PixelRect {
    fn clipped(rect: SpriteRect, [width, height]: [u32; 2]) -> Option<Self> {
        let clip = |value: i64, limit| value.clamp(0, i64::from(limit)) as u32;
        let x = clip(i64::from(rect.x), width);
        let y = clip(i64::from(rect.y), height);
        let right = clip(i64::from(rect.x) + i64::from(rect.width), width);
        let bottom = clip(i64::from(rect.y) + i64::from(rect.height), height);
        (x < right && y < bottom).then(|| Self {
            x,
            y,
            width: right - x,
            height: bottom - y,
        })
    }

    fn right(self) -> u32 {
        self.x + self.width
    }

    fn bottom(self) -> u32 {
        self.y + self.height
    }

    fn pitch(self) -> usize {
        self.width as usize * 4
    }
}

/// Prepare a complete static stage in content coordinates.
pub fn frame_rgba8(extent: [u32; 2], white_gradient: bool, sprite: Option<Sprite>) -> Vec<u8> {
    let [width, height] = extent;
    let full = PixelRect {
        x: 0,
        y: 0,
        width,
        height,
    };
    let sprite = sprite.and_then(|sprite| SpriteImage::new(extent, sprite));
    render_pixels(extent, full, white_gradient, sprite.as_ref())
}

fn render_pixels(
    [width, height]: [u32; 2],
    rect: PixelRect,
    white_gradient: bool,
    sprite: Option<&SpriteImage>,
) -> Vec<u8> {
    if rect.width == 0 || rect.height == 0 {
        return Vec::new();
    }
    let mut pixels = vec![0u8; rect.pitch() * rect.height as usize];
    for (y, row) in (rect.y..rect.bottom()).zip(pixels.chunks_exact_mut(rect.pitch())) {
        let v = normalized_coordinate(y, height);
        for (x, pixel) in (rect.x..rect.right()).zip(row.chunks_exact_mut(4)) {
            let u = normalized_coordinate(x, width);
            let alpha = gradient_alpha(u, v, x, y);
            let tint = if white_gradient { alpha } else { 0 };
            pixel.copy_from_slice(&[tint, tint, tint, alpha]);
        }
    }

    if let Some(sprite) = sprite {
        let clipped = sprite.clipped;
        let x = (clipped.x - rect.x) as usize * 4;
        let offset = (clipped.y - rect.y) as usize * rect.pitch();
        for (y, row) in
            (clipped.y..clipped.bottom()).zip(pixels[offset..].chunks_exact_mut(rect.pitch()))
        {
            for (x, pixel) in
                (clipped.x..clipped.right()).zip(row[x..x + clipped.pitch()].chunks_exact_mut(4))
            {
                source_over(pixel, sprite.sample(x, y));
            }
        }
    }
    pixels
}

fn normalized_coordinate(coordinate: u32, extent: u32) -> f32 {
    if extent > 1 {
        coordinate as f32 / (extent - 1) as f32
    } else {
        0.5
    }
}

fn gradient_alpha(u: f32, v: f32, x: u32, y: u32) -> u8 {
    let t = (v - u).clamp(0.0, 1.0);
    let ideal = (255.0 * 0.33) * t * t * (3.0 - 2.0 * t);
    let low = 2 * ((x ^ y) & 1) + (y & 1);
    let high = ((x ^ y) & 2) + ((y >> 1) & 1);
    let dither = ((4 * low + high) as f32 + 0.5) / 16.0;
    (ideal + dither) as u8
}

fn round_byte(value: f32) -> u8 {
    let lower = value as u8;
    let fraction = value - f32::from(lower);
    lower + u8::from(fraction > 0.5 || (fraction == 0.5 && lower & 1 != 0))
}

fn source_over(pixel: &mut [u8], source: [f32; 4]) {
    let gradient_scale = 1.0 - source[3] / 255.0;
    for (destination, component) in pixel.iter_mut().zip(source) {
        *destination = round_byte(component + f32::from(*destination) * gradient_scale);
    }
}

enum SpriteTexture {
    Mask(&'static [u8; MASK_BYTES]),
    Rgba(&'static [u8; MASK_BYTES * 4]),
}

struct SpriteImage {
    texture: SpriteTexture,
    rect: SpriteRect,
    clipped: PixelRect,
    scale: [f32; 2],
    opacity: f32,
}

impl SpriteImage {
    fn new([width, height]: [u32; 2], sprite: Sprite) -> Option<Self> {
        let rect = SpriteRect::for_body(sprite.body, width, height);
        let clipped = PixelRect::clipped(rect, [width, height])?;
        let texture = match sprite.body {
            Body::Moon => SpriteTexture::Mask(MOON),
            Body::Earth => SpriteTexture::Mask(EARTH),
            Body::Sun => SpriteTexture::Rgba(SUN),
        };
        Some(Self {
            texture,
            rect,
            clipped,
            scale: [
                CELESTIAL_TEXTURE_SIDE as f32 / rect.width as f32,
                CELESTIAL_TEXTURE_SIDE as f32 / rect.height as f32,
            ],
            opacity: sprite.opacity.clamp(0.0, 1.0),
        })
    }

    fn sample(&self, x: u32, y: u32) -> [f32; 4] {
        let x = (i64::from(x) - i64::from(self.rect.x)) as usize;
        let y = (i64::from(y) - i64::from(self.rect.y)) as usize;
        match self.texture {
            SpriteTexture::Mask(mask) => [self.mask_alpha(mask, x, y) * self.opacity; 4],
            SpriteTexture::Rgba(rgba) => {
                // The Sun always uses its native pitch, even in clipped windows.
                let offset = (y * TEXTURE_SIDE + x) * 4;
                core::array::from_fn(|channel| f32::from(rgba[offset + channel]) * self.opacity)
            }
        }
    }

    fn mask_alpha(&self, mask: &[u8; MASK_BYTES], x: usize, y: usize) -> f32 {
        let last_texel = TEXTURE_SIDE - 1;
        let u = ((x as f32 + 0.5) * self.scale[0] - 0.5).clamp(0.0, last_texel as f32);
        let v = ((y as f32 + 0.5) * self.scale[1] - 0.5).clamp(0.0, last_texel as f32);
        let x0 = u as usize;
        let y0 = v as usize;
        let x1 = (x0 + 1).min(last_texel);
        let y1 = (y0 + 1).min(last_texel);
        let tx = u - x0 as f32;
        let ty = v - y0 as f32;
        let row_sample = |row| {
            let left = f32::from(mask[row * TEXTURE_SIDE + x0]);
            let right = f32::from(mask[row * TEXTURE_SIDE + x1]);
            left + (right - left) * tx
        };
        let upper = row_sample(y0);
        let lower = row_sample(y1);
        upper + (lower - upper) * ty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_has_stable_alpha_and_premultiplied_tint() {
        let [width, height] = [19, 13];
        let black = frame_rgba8([width, height], false, None);
        let white = frame_rgba8([width, height], true, None);
        assert_eq!(black.len(), (width * height * 4) as usize);
        for (black, white) in black.chunks_exact(4).zip(white.chunks_exact(4)) {
            assert_eq!(black, &[0, 0, 0, white[3]]);
            assert_eq!(white, &[white[3]; 4]);
        }
        assert_eq!(
            &white[(width as usize - 1) * 4..width as usize * 4],
            &[0; 4]
        );
        let bottom_left = ((height - 1) * width * 4) as usize;
        assert_eq!(white[bottom_left + 3], 84);
    }

    #[test]
    fn sun_is_static_and_retains_native_pixel_pitch_when_clipped() {
        let sprite = Sprite {
            body: Body::Sun,
            opacity: 1.0,
        };
        let extent = [96, 72];
        let sun = SpriteImage::new(extent, sprite).unwrap();
        assert_eq!(
            sun.clipped,
            PixelRect {
                x: 0,
                y: 0,
                width: 96,
                height: 72
            }
        );
        let center = (256 * TEXTURE_SIDE + 256) * 4;
        assert_eq!(
            sun.sample(48, 36),
            core::array::from_fn(|i| f32::from(SUN[center + i]))
        );
    }

    #[test]
    fn translucent_sun_color_is_composited_as_premultiplied_rgba() {
        let mut gradient = [0, 0, 0, 64];
        source_over(&mut gradient, [128.0, 64.0, 15.0, 128.0]);
        assert_eq!(gradient, [128, 64, 15, 160]);
    }
}
