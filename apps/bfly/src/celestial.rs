//! Appearance and placement of the retained background sprite.

pub(crate) const CELESTIAL_TEXTURE_SIDE: u32 = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Body {
    Moon = 1,
    Earth = 2,
    Sun = 3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sprite {
    pub body: Body,
    pub opacity: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Wake {
    Sunrise,
    Twilight,
    Universe,
}

impl Wake {
    pub fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Sunrise,
            1 => Self::Twilight,
            _ => Self::Universe,
        }
    }

    fn weights(self) -> [f32; 3] {
        let mut weights = [0.0; 3];
        weights[self as usize] = 1.0;
        weights
    }
}

const WAKE_FADE_NS: u64 = 4_000_000_000;

#[derive(Clone, Copy)]
pub struct WakeFade {
    from: [f32; 3],
    target: Wake,
    started_ns: u64,
}

impl WakeFade {
    pub fn new(wake: Wake) -> Self {
        Self {
            from: wake.weights(),
            target: wake,
            started_ns: 0,
        }
    }

    pub fn retarget(&mut self, target: Wake, now_ns: u64) {
        if target != self.target {
            let now_ns = now_ns.max(self.started_ns);
            self.from = self.weights_at(now_ns);
            self.target = target;
            self.started_ns = now_ns;
        }
    }

    pub fn weights_at(self, now_ns: u64) -> [f32; 3] {
        let elapsed = now_ns.saturating_sub(self.started_ns);
        if elapsed >= WAKE_FADE_NS {
            return self.target.weights();
        }
        let t = elapsed as f32 / WAKE_FADE_NS as f32;
        let blend = t * t * (3.0 - 2.0 * t);
        let mut weights = self.from.map(|weight| weight * (1.0 - blend));
        weights[self.target as usize] += blend;
        weights
    }
}

pub struct Appearance {
    pub sprite: Option<Sprite>,
    pub white_gradient: bool,
    pub wake: Wake,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpriteRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl SpriteRect {
    pub const EMPTY: Self = Self {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    };

    pub fn for_body(body: Body, width: u32, height: u32) -> Self {
        if body == Body::Sun {
            let half = i64::from(CELESTIAL_TEXTURE_SIDE / 2);
            return Self {
                x: (i64::from(width / 2) - half) as i32,
                y: (i64::from(height / 2) - half) as i32,
                width: CELESTIAL_TEXTURE_SIDE,
                height: CELESTIAL_TEXTURE_SIDE,
            };
        }

        let shortest = width.min(height);
        let margin = (shortest / 12).min(32);
        let native_size = if body == Body::Moon {
            341
        } else {
            CELESTIAL_TEXTURE_SIDE
        };
        let size = (shortest - 2 * margin).min(native_size);
        Self {
            x: if body == Body::Moon {
                margin
            } else {
                width - margin - size
            } as i32,
            y: margin as i32,
            width: size,
            height: size,
        }
    }
}

pub fn appearance_for([r, g, b]: [f32; 3]) -> Appearance {
    let hi = r.max(g).max(b);
    let lo = r.min(g).min(b);
    let chroma = hi - lo;
    let color_strength = ease(10.0, 51.0, chroma);
    let hue = if color_strength == 0.0 {
        0.0
    } else if g == hi {
        60.0 * (2.0 + (b - r) / chroma)
    } else if b == hi {
        60.0 * (4.0 + (r - g) / chroma)
    } else {
        60.0 * ((g - b) / chroma + if g < b { 6.0 } else { 0.0 })
    };
    let lightness = (hi + lo) / 510.0;
    let earth = color_strength * band(211.0, 263.0, hue) * (1.0 - ease(0.38, 0.50, lightness));
    let light_blue = color_strength * band(165.0, 250.0, hue) * ease(0.50, 0.63, lightness);
    let green = color_strength * band(97.0, 173.0, hue);
    let moon = green.max(light_blue);

    // Swap sprites at zero opacity so there is no hard cut between masks
    // in different positions. Keep the original Moon and Earth peak alpha.
    const HANDOFF: f32 = 0.10;
    let themed = earth.max(moon);
    let (body, opacity) = if themed <= HANDOFF {
        (Body::Sun, 1.0 - ease(0.0, HANDOFF, themed))
    } else if earth > moon {
        (
            Body::Earth,
            0.92 * earth * ease(HANDOFF, 2.0 * HANDOFF, earth),
        )
    } else {
        (
            Body::Moon,
            (0.50 * green).max(0.70 * light_blue) * ease(HANDOFF, 2.0 * HANDOFF, moon),
        )
    };
    let sprite = (opacity > 0.0).then_some(Sprite { body, opacity });
    let wake = match body {
        Body::Moon => Wake::Sunrise,
        Body::Earth => Wake::Twilight,
        Body::Sun => Wake::Universe,
    };
    Appearance {
        sprite,
        white_gradient: green > HANDOFF,
        wake,
    }
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
    fn backdrop_routes_sprites_and_wake_profiles() {
        let purple = appearance_for([173.0, 64.0, 255.0]);
        assert_eq!(
            purple.sprite,
            Some(Sprite {
                body: Body::Sun,
                opacity: 1.0
            })
        );
        assert_eq!(purple.wake, Wake::Universe);
        assert!(!purple.white_gradient);

        let green = appearance_for([0.0, 128.0, 0.0]);
        assert_eq!(green.sprite.unwrap().body, Body::Moon);
        assert_eq!(green.wake, Wake::Sunrise);
        assert!(green.white_gradient);

        let light_blue = appearance_for([128.0, 217.0, 255.0]);
        assert_eq!(light_blue.sprite.unwrap().body, Body::Moon);
        assert_eq!(light_blue.wake, Wake::Sunrise);
        assert!(!light_blue.white_gradient);

        let deep_blue = appearance_for([0.0, 32.0, 153.0]);
        assert_eq!(deep_blue.sprite.unwrap().body, Body::Earth);
        assert_eq!(deep_blue.wake, Wake::Twilight);
        assert!(!deep_blue.white_gradient);
    }

    #[test]
    fn wake_fades_from_the_stage_handoff_for_four_seconds() {
        let started = 3_000_000_000;
        let mut fade = WakeFade::new(Wake::Universe);
        assert_eq!(fade.weights_at(0), [0.0, 0.0, 1.0]);
        fade.retarget(Wake::Sunrise, started);
        assert_eq!(fade.weights_at(started), [0.0, 0.0, 1.0]);
        fade.retarget(Wake::Sunrise, started + 1_000_000_000);
        assert_eq!(fade.weights_at(started + 2_000_000_000), [0.5, 0.0, 0.5]);
        assert_eq!(fade.weights_at(started + 4_000_000_000), [1.0, 0.0, 0.0]);
    }

    #[test]
    fn retargeting_retains_the_current_mix() {
        let mut fade = WakeFade::new(Wake::Sunrise);
        fade.retarget(Wake::Twilight, 0);
        fade.retarget(Wake::Universe, 2_000_000_000);
        assert_eq!(fade.weights_at(2_000_000_000), [0.5, 0.5, 0.0]);
        assert_eq!(fade.weights_at(4_000_000_000), [0.25, 0.25, 0.5]);
        assert_eq!(fade.weights_at(6_000_000_000), [0.0, 0.0, 1.0]);
    }

    #[test]
    fn sprite_fades_preserve_sub_byte_color_changes() {
        let first = appearance_for([30.0, 30.0, 255.0]).sprite.unwrap();
        let later = appearance_for([30.1, 30.1, 255.0]).sprite.unwrap();
        assert_eq!(first.body, Body::Moon);
        assert_eq!(later.body, Body::Moon);
        assert!(later.opacity > first.opacity);
        assert!(later.opacity - first.opacity < 1.0 / 255.0);
    }

    #[test]
    fn places_moon_earth_and_sun_at_their_native_scale() {
        assert_eq!(
            SpriteRect::for_body(Body::Moon, 1920, 1080),
            SpriteRect {
                x: 32,
                y: 32,
                width: 341,
                height: 341,
            }
        );
        assert_eq!(
            SpriteRect::for_body(Body::Earth, 1920, 1080),
            SpriteRect {
                x: 1376,
                y: 32,
                width: 512,
                height: 512,
            }
        );
        assert_eq!(
            SpriteRect::for_body(Body::Sun, 1920, 1080),
            SpriteRect {
                x: 704,
                y: 284,
                width: 512,
                height: 512,
            }
        );
        for body in [Body::Moon, Body::Earth] {
            for (w, h) in [(3, 2), (200, 1000), (1000, 200)] {
                let rect = SpriteRect::for_body(body, w, h);
                assert!(
                    rect.width > 0
                        && rect.x as u32 + rect.width <= w
                        && rect.y as u32 + rect.height <= h
                );
            }
        }
    }
}
