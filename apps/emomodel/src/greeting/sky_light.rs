use glam::{Vec2, Vec3};

const MAX_LEVEL: u8 = 9;
const KEY_ELEVATION: f32 = 28.0_f32.to_radians();
const KEY_YAW_OFFSET: f32 = 26.0_f32.to_radians();
const SKY_YAW_OFFSET: f32 = 13.0_f32.to_radians();
// The Long path preset (4× attenuation) in studies/sky/sunlight.js.
// Normalize to the same luminance as a white key.
pub(super) const SUNRISE_RGB: Vec3 = Vec3::new(1.5232292, 0.8938354, 0.51056594);
pub(super) const MIN_KELVIN: u16 = 2200;
pub(super) const MAX_KELVIN: u16 = 9000;
pub(super) const DEFAULT_KELVIN: u16 = 6500;
const KELVIN_STEP: u16 = 400;

// Planck radiance integrated at 380–780 nm with the CIE 1931 2° observer
// from studies/sky/sunlight.js, then normalized to Y=1 in linear sRGB.
// CIE data DOI 10.25039/CIE.DS.xvudnb9b. Planck's second radiation constant
// is 1.438776877e-2 m·K (NIST CODATA).
const KELVIN_RGB: [Vec3; ((MAX_KELVIN - MIN_KELVIN) / KELVIN_STEP + 1) as usize] = [
    Vec3::new(2.3142305, 0.7035976, 0.0652889),
    Vec3::new(1.9987690, 0.7872835, 0.1654361),
    Vec3::new(1.7708951, 0.8442775, 0.2720207),
    Vec3::new(1.6002590, 0.8842068, 0.3790635),
    Vec3::new(1.4687570, 0.9128087, 0.4830533),
    Vec3::new(1.3650263, 0.9336553, 0.5820718),
    Vec3::new(1.2816158, 0.9490563, 0.6751843),
    Vec3::new(1.2134549, 0.9605537, 0.7620514),
    Vec3::new(1.1569826, 0.9692047, 0.8426870),
    Vec3::new(1.1096311, 0.9757509, 0.9173089),
    Vec3::new(1.0695069, 0.9807221, 0.9862462),
    Vec3::new(1.0351865, 0.9845028, 1.0498821),
    Vec3::new(1.0055819, 0.9873760, 1.1086178),
    Vec3::new(0.9798495, 0.9895523, 1.1628517),
    Vec3::new(0.9573270, 0.9911901, 1.2129661),
    Vec3::new(0.9374880, 0.9924095, 1.2593206),
    Vec3::new(0.9199104, 0.9933025, 1.3022484),
    Vec3::new(0.9042521, 0.9939398, 1.3420557),
];

const IRRADIANCE_BYTES: usize = 9 * 3 * size_of::<f32>();
const REFLECTION_SIZE: usize = 65;
const REFLECTION_BYTES: usize = REFLECTION_SIZE * REFLECTION_SIZE * 3;
const SRGB_OFFSET: usize = IRRADIANCE_BYTES + REFLECTION_BYTES;
const LINEAR_RGB_STEPS: usize = 4096;
// Generated with tools/build_sky_lighting.py from the 2K study. The file stores
// nine little-endian f32 RGB harmonics, a Y-up octahedral RGB8 reflection grid,
// and the linear-to-sRGB lookup table.
const SKY_DATA: &[u8; SRGB_OFFSET + LINEAR_RGB_STEPS + 1] =
    include_bytes!("../../assets/sky-lighting.bin");
const IRRADIANCE: [Vec3; 9] = read_irradiance();

const fn read_irradiance() -> [Vec3; 9] {
    let mut coefficients = [Vec3::ZERO; 9];
    let mut index = 0;
    while index < coefficients.len() {
        let offset = index * 3 * size_of::<f32>();
        coefficients[index] =
            Vec3::new(read_f32(offset), read_f32(offset + 4), read_f32(offset + 8));
        index += 1;
    }
    coefficients
}

const fn read_f32(offset: usize) -> f32 {
    f32::from_le_bytes([
        SKY_DATA[offset],
        SKY_DATA[offset + 1],
        SKY_DATA[offset + 2],
        SKY_DATA[offset + 3],
    ])
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Key {
    Neutral,
    Sunrise,
    Kelvin(u16),
}

impl Key {
    pub(super) fn color(self) -> Vec3 {
        match self {
            Self::Neutral => Vec3::ONE,
            Self::Sunrise => SUNRISE_RGB,
            Self::Kelvin(temperature) => {
                let offset = temperature.clamp(MIN_KELVIN, MAX_KELVIN) - MIN_KELVIN;
                let left = usize::from(offset / KELVIN_STEP);
                let right = (left + 1).min(KELVIN_RGB.len() - 1);
                let fraction = f32::from(offset % KELVIN_STEP) / f32::from(KELVIN_STEP);
                KELVIN_RGB[left].lerp(KELVIN_RGB[right], fraction)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Levels {
    pub sky: u8,
    pub sheen: u8,
    pub key: Key,
    pub firefly: bool,
}

impl Default for Levels {
    fn default() -> Self {
        Self {
            sky: 5,
            sheen: 5,
            key: Key::Neutral,
            firefly: true,
        }
    }
}

pub(super) struct SkyLight {
    pub(super) key: Key,
    pub(super) firefly: bool,
    key_tint: Vec3,
    view_back: Vec3,
    key_direction: Vec3,
    sky_cos: f32,
    sky_sin: f32,
    sky_strength: f32,
    reflection_strength: f32,
}

impl SkyLight {
    pub(super) fn new(view_back: Vec3, levels: Levels) -> Self {
        let view_back = view_back.try_normalize().unwrap_or(-Vec3::Z);
        let camera_yaw = (-view_back.x).atan2(-view_back.z);
        // At the study's 22° view, the key is at 48° and the lookup rotates −35°.
        let (key_sin, key_cos) = (camera_yaw + KEY_YAW_OFFSET).sin_cos();
        let (key_up, key_horizontal) = KEY_ELEVATION.sin_cos();
        let key_direction = Vec3::new(-key_sin * key_horizontal, key_up, -key_cos * key_horizontal);
        let (sky_sin, sky_cos) = (-camera_yaw - SKY_YAW_OFFSET).sin_cos();
        let sky_strength = f32::from(levels.sky.min(MAX_LEVEL)) / f32::from(MAX_LEVEL);
        let sheen_strength = f32::from(levels.sheen.min(MAX_LEVEL)) / f32::from(MAX_LEVEL);
        Self {
            key: levels.key,
            firefly: levels.firefly,
            key_tint: levels.key.color() - Vec3::ONE,
            view_back,
            key_direction,
            sky_cos,
            sky_sin,
            sky_strength,
            reflection_strength: 3.8 * sky_strength * sheen_strength,
        }
    }

    pub(super) fn key_direction(&self) -> Vec3 {
        self.key_direction
    }

    pub(super) fn apply(
        &self,
        normal: Vec3,
        base_linear: Vec3,
        key_with_ambient: Vec3,
        ambient: f32,
    ) -> Vec3 {
        let color = self.key_light(base_linear, key_with_ambient, ambient);
        self.environment(normal, base_linear, color)
    }

    pub(super) fn key_light(&self, base: Vec3, key_with_ambient: Vec3, ambient: f32) -> Vec3 {
        if self.key == Key::Neutral {
            key_with_ambient
        } else {
            key_with_ambient + self.key_tint * (key_with_ambient - base * ambient)
        }
    }

    pub(super) fn environment(&self, normal: Vec3, base_linear: Vec3, mut color: Vec3) -> Vec3 {
        if self.sky_strength == 0.0 {
            return color;
        }

        let normal = normal.normalize_or_zero();
        let sky_diffuse = irradiance(self.sky_space(normal)).max(Vec3::ZERO);
        let diffuse = (2.2 * sky_diffuse - Vec3::splat(0.12)) * self.sky_strength;
        color += base_linear * diffuse;

        if self.reflection_strength > 0.0 {
            let facing = normal.dot(self.view_back);
            let grazing = 1.0 - facing.clamp(0.0, 1.0);
            let grazing_squared = grazing * grazing;
            let fresnel = 0.035 + 0.62 * grazing_squared * grazing_squared;
            let reflected = (2.0 * facing) * normal - self.view_back;
            let sky_reflection = reflection(self.sky_space(reflected));
            color += sky_reflection * (self.reflection_strength * fresnel);
        }
        color
    }

    fn sky_space(&self, direction: Vec3) -> Vec3 {
        Vec3::new(
            self.sky_cos * direction.x + self.sky_sin * direction.z,
            direction.y,
            self.sky_cos * direction.z - self.sky_sin * direction.x,
        )
    }
}

pub(super) fn linear_rgb(channels: [u8; 3]) -> Vec3 {
    let linear = channels.map(|channel| {
        let channel = f32::from(channel) / 255.0;
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    });
    Vec3::from_array(linear)
}

pub(super) fn encode_rgb(linear: Vec3) -> [u8; 3] {
    linear.to_array().map(|channel| {
        let step = (channel.clamp(0.0, 1.0) * LINEAR_RGB_STEPS as f32 + 0.5) as usize;
        SKY_DATA[SRGB_OFFSET + step]
    })
}

fn irradiance(normal: Vec3) -> Vec3 {
    let Vec3 { x, y, z } = normal;
    IRRADIANCE[0] * 0.282095
        + 0.488603 * (IRRADIANCE[1] * x + IRRADIANCE[2] * y + IRRADIANCE[3] * z)
        + 1.092548 * (IRRADIANCE[4] * (x * y) + IRRADIANCE[5] * (y * z) + IRRADIANCE[7] * (x * z))
        + (0.315392 * (3.0 * y * y - 1.0)) * IRRADIANCE[6]
        + (0.546274 * (x * x - z * z)) * IRRADIANCE[8]
}

pub(super) fn oct_encode(direction: Vec3) -> Vec2 {
    let inv_sum = 1.0 / direction.abs().element_sum().max(f32::MIN_POSITIVE);
    let u = direction.x * inv_sum;
    let v = direction.z * inv_sum;
    let (u, v) = if direction.y < 0.0 {
        ((1.0 - v.abs()).copysign(u), (1.0 - u.abs()).copysign(v))
    } else {
        (u, v)
    };

    (Vec2::new(u, v) * 0.5 + Vec2::splat(0.5)).clamp(Vec2::ZERO, Vec2::ONE)
}

pub(super) fn oct_decode(uv: Vec2) -> Vec3 {
    let uv = uv * 2.0 - Vec2::ONE;
    let up = 1.0 - uv.abs().element_sum();
    let fold = up.min(0.0);
    let x = uv.x - fold.copysign(uv.x);
    let z = uv.y - fold.copysign(uv.y);
    Vec3::new(x, up, z).normalize()
}

fn reflection(direction: Vec3) -> Vec3 {
    let last = REFLECTION_SIZE - 1;
    let Vec2 { x, y } = oct_encode(direction) * last as f32;
    let x0 = x as usize;
    let y0 = y as usize;
    let x1 = (x0 + 1).min(last);
    let y1 = (y0 + 1).min(last);
    let x_mix = x - x0 as f32;
    let y_mix = y - y0 as f32;
    let top = reflection_texel(x0, y0).lerp(reflection_texel(x1, y0), x_mix);
    let bottom = reflection_texel(x0, y1).lerp(reflection_texel(x1, y1), x_mix);
    top.lerp(bottom, y_mix) * (1.0 / 255.0)
}

fn reflection_texel(x: usize, y: usize) -> Vec3 {
    let i = IRRADIANCE_BYTES + (y * REFLECTION_SIZE + x) * 3;
    Vec3::new(
        SKY_DATA[i].into(),
        SKY_DATA[i + 1].into(),
        SKY_DATA[i + 2].into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_modes_tint_only_direct_light_even_at_zero_sky() {
        let base = linear_rgb([218, 171, 147]);
        let ambient = 0.22;
        let fill = base * ambient;
        let direct = base * 0.73;
        let normal = -Vec3::Z;
        assert_eq!(Levels::default().key, Key::Neutral);
        for sky in [0, Levels::default().sky] {
            let make = |key| {
                SkyLight::new(
                    normal,
                    Levels {
                        sky,
                        key,
                        ..Levels::default()
                    },
                )
            };
            let baseline = make(Key::Neutral);
            let shade = |light: &SkyLight| light.apply(normal, base, fill + direct, ambient);
            for key in [
                Key::Sunrise,
                Key::Kelvin(MIN_KELVIN),
                Key::Kelvin(MAX_KELVIN),
            ] {
                let enabled = make(key);
                let delta = (key.color() - Vec3::ONE) * direct;
                assert_eq!(enabled.key_direction(), baseline.key_direction());
                assert_eq!(
                    enabled.apply(normal, base, fill, ambient),
                    baseline.apply(normal, base, fill, ambient),
                );
                assert!((shade(&enabled) - shade(&baseline)).abs_diff_eq(delta, 0.000_001));
            }
        }
    }

    #[test]
    fn kelvin_curve_keeps_luminance_while_shifting_from_warm_to_cool() {
        let warm = Key::Kelvin(MIN_KELVIN).color();
        let middle = Key::Kelvin(DEFAULT_KELVIN).color();
        let cool = Key::Kelvin(MAX_KELVIN).color();
        let luminance = Vec3::new(0.212639, 0.7151687, 0.0721923);
        for rgb in [warm, middle, cool] {
            assert!((rgb.dot(luminance) - 1.0).abs() < 0.000_001);
            assert!(rgb.min_element() > 0.0);
        }
        assert!(warm.x > middle.x && middle.x > cool.x);
        assert!(warm.z < middle.z && middle.z < cool.z);
        assert_eq!(Key::Kelvin(0).color(), warm);
        assert_eq!(Key::Kelvin(u16::MAX).color(), cool);
    }
}
