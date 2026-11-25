//! Appearance and placement of the retained background sprite.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Body {
    Moon = 1,
    Earth = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Sprite {
    pub body: Body,
    pub opacity: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpriteRect {
    pub x: u32,
    pub y: u32,
    pub size: u32,
}

impl SpriteRect {
    pub fn top_right(width: u32, height: u32) -> Self {
        let shortest = width.min(height);
        let margin = (shortest / 12).min(32);
        let size = (shortest - 2 * margin).min(512);
        Self {
            x: width - margin - size,
            y: margin,
            size,
        }
    }
}

pub fn sprite_for([r, g, b]: [u8; 3]) -> Option<Sprite> {
    let hi = r.max(g).max(b);
    let lo = r.min(g).min(b);
    let chroma = f32::from(hi - lo);
    let color_strength = ease(10.0, 51.0, chroma);
    if color_strength == 0.0 {
        return None;
    }
    let [r, g, b] = [r, g, b].map(f32::from);
    let hue = 60.0
        * if g == f32::from(hi) {
            2.0 + (b - r) / chroma
        } else if b == f32::from(hi) {
            4.0 + (r - g) / chroma
        } else {
            (g - b) / chroma + if g < b { 6.0 } else { 0.0 }
        };
    let lightness = (f32::from(hi) + f32::from(lo)) / 510.0;
    let dark_blue = band(211.0, 263.0, hue) * (1.0 - ease(0.38, 0.50, lightness));
    let light_blue = band(165.0, 250.0, hue) * ease(0.50, 0.63, lightness);
    let green = band(97.0, 173.0, hue);
    let (body, strength) = if dark_blue > 0.0 {
        (Body::Earth, 0.92 * dark_blue)
    } else {
        (Body::Moon, (0.50 * green).max(0.70 * light_blue))
    };
    let opacity = (255.0 * strength * color_strength + 0.5) as u8;
    (opacity > 0).then_some(Sprite { body, opacity })
}

fn band(low: f32, high: f32, value: f32) -> f32 {
    ease(low, low + 12.0, value) * (1.0 - ease(high - 12.0, high, value))
}

fn ease(low: f32, high: f32, value: f32) -> f32 {
    let t = ((value - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooses_a_body_from_the_display_hue_and_lightness() {
        assert_eq!(sprite_for([255; 3]), None);
        assert_eq!(sprite_for([173, 64, 255]), None);
        assert_eq!(sprite_for([0, 128, 0]).unwrap().body, Body::Moon);
        assert_eq!(sprite_for([128, 217, 255]).unwrap().body, Body::Moon);
        assert_eq!(sprite_for([0, 32, 153]).unwrap().body, Body::Earth);
    }

    #[test]
    fn blue_fades_through_zero_between_moon_and_earth() {
        let dark = sprite_for([0, 0, 192]).unwrap();
        let fading_dark = sprite_for([0, 0, 225]).unwrap();
        assert!(dark.opacity > fading_dark.opacity);
        assert_eq!(sprite_for([0, 0, 255]), None);
        let fading_light = sprite_for([30, 30, 255]).unwrap();
        let light = sprite_for([100, 100, 255]).unwrap();
        assert!(fading_light.opacity < light.opacity);
        assert_eq!(light.body, Body::Moon);
    }

    #[test]
    fn sprite_uses_native_pixels_and_fits_smaller_windows() {
        assert_eq!(
            SpriteRect::top_right(1920, 1080),
            SpriteRect {
                x: 1376,
                y: 32,
                size: 512
            }
        );
        for (w, h) in [(3, 2), (200, 1000), (1000, 200)] {
            let rect = SpriteRect::top_right(w, h);
            assert!(rect.size > 0 && rect.x + rect.size <= w && rect.y + rect.size <= h);
        }
    }
}
