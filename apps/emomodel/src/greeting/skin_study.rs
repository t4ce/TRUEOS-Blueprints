use glam::Vec3;

#[cfg(test)]
use crate::autobs::{BASE_YAW, camera_back};

pub(super) const SIZE: usize = 64;
#[cfg(test)]
const KEY_ELEVATION: f32 = 28.0_f32.to_radians();
const MAX_ENERGY: f32 = 1.42;
pub(super) const AMBIENT: f32 = 0.22;

#[derive(Clone, Copy)]
pub(super) struct Material {
    roughness: f32,
}

impl Material {
    pub(super) fn at(position: Vec3) -> Self {
        // Sample in bind space once. Shared positions keep their material
        // across UV seams and the roughness follows the vertices during posing.
        let roughness = 0.5
            + 0.48 * noise(position * 19.0, 0x82b9_3ee5)
            + 0.19 * noise(position * 37.0, 0x7ade_e56d);
        Self { roughness }
    }
}

pub(super) struct Lighting {
    key: Vec3,
    half: Vec3,
}

impl Lighting {
    #[cfg(test)]
    pub(super) fn new(angle: i16, view_back: Vec3) -> Self {
        let back = camera_back(BASE_YAW);
        let (rise, run) = KEY_ELEVATION.sin_cos();
        let (side, center) = f32::from(angle).to_radians().sin_cos();
        let key = center * (back * run + Vec3::Y * rise) + side * Vec3::Y.cross(back);
        Self::with_key(key, view_back)
    }

    pub(super) fn with_key(key: Vec3, view_back: Vec3) -> Self {
        Self {
            key,
            half: (key + view_back).normalize_or_zero(),
        }
    }

    pub(super) fn coordinate(&self, normal: Vec3, material: Material) -> f32 {
        energy_coordinate(self.energy(normal, material))
    }

    pub(super) fn energy(&self, normal: Vec3, material: Material) -> f32 {
        let key = normal.dot(self.key);
        let direct = key.clamp(0.0, 1.0);
        let wrap = ((key + 0.21) / 1.21).clamp(0.0, 1.0);
        let diffuse = AMBIENT + 0.73 * (0.56 * direct + 0.44 * wrap);

        let h = normal.dot(self.half).clamp(0.0, 1.0);
        let h2 = h * h;
        let h4 = h2 * h2;
        let h8 = h4 * h4;
        let h16 = h8 * h8;
        let rough = smooth((material.roughness - 0.28) / 0.44);
        let highlight =
            (0.28 * h16 * h16 * (1.0 - rough) + 0.10 * h8 * rough) * smooth((key + 0.015) / 0.315);
        // A scalar energy is enough to retain the moving narrow/broad sheen
        // in the texture lookup used by the existing single-sample renderer.
        diffuse + highlight * (1.15 + 0.65 * direct)
    }
}

#[cfg(any(test, target_os = "trueos"))]
pub(super) fn energy_coordinate(energy: f32) -> f32 {
    texel_center(energy / MAX_ENERGY)
}

#[cfg(test)]
pub(super) fn lookup_rgba() -> Vec<u8> {
    color_lookup_rgba(crate::light::SKIN_SRGB)
}

pub(super) fn color_lookup_rgba(rgb: [u8; 3]) -> Vec<u8> {
    let linear = rgb.map(|channel| ((f32::from(channel) / 255.0 + 0.055) / 1.055).powf(2.4));
    let mut rgba = Vec::with_capacity(SIZE * 4);
    for x in 0..SIZE {
        let energy = projection(x) * MAX_ENERGY;
        let light = light_rgb(energy).to_array();
        let color: [u8; 3] = std::array::from_fn(|channel| {
            let value = encode_srgb(linear[channel] * light[channel]);
            (value.clamp(0.0, 1.0) * 255.0).round() as u8
        });
        rgba.extend(color);
        rgba.push(255);
    }
    rgba
}

pub(super) fn light_rgb(energy: f32) -> Vec3 {
    let lit = ((energy - AMBIENT) / 0.73).clamp(0.0, 1.0);
    Vec3::splat(energy) + lit * Vec3::new(0.041, 0.0, -0.035)
}

fn projection(texel: usize) -> f32 {
    texel as f32 / (SIZE - 1) as f32
}

fn texel_center(value: f32) -> f32 {
    (value.clamp(0.0, 1.0) * (SIZE - 1) as f32 + 0.5) / SIZE as f32
}

fn encode_srgb(channel: f32) -> f32 {
    if channel <= 0.003_130_8 {
        12.92 * channel
    } else {
        1.055 * channel.powf(1.0 / 2.4) - 0.055
    }
}

fn smooth(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn noise(position: Vec3, seed: u32) -> f32 {
    let floor = position.floor();
    let [x, y, z] = floor.to_array().map(|component| component as i32);
    let [u, v, w] = (position - floor).to_array().map(smooth);
    let layer = |z| {
        let top = lerp(hash(x, y, z, seed), hash(x + 1, y, z, seed), u);
        let bottom = lerp(hash(x, y + 1, z, seed), hash(x + 1, y + 1, z, seed), u);
        lerp(top, bottom, v)
    };
    lerp(layer(z), layer(z + 1), w) - 0.5
}

fn lerp(a: f32, b: f32, weight: f32) -> f32 {
    a + (b - a) * weight
}

fn hash(x: i32, y: i32, z: i32, seed: u32) -> f32 {
    let mut hash = seed
        ^ (x as u32).wrapping_mul(0x9e37_79b1)
        ^ (y as u32).wrapping_mul(0x85eb_ca77)
        ^ (z as u32).wrapping_mul(0xc2b2_ae3d);
    hash = (hash ^ (hash >> 16)).wrapping_mul(0x7feb_352d);
    hash = (hash ^ (hash >> 15)).wrapping_mul(0x846c_a68b);
    ((hash ^ (hash >> 16)) >> 8) as f32 / 16_777_215.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autobs::view_back;

    #[test]
    fn roughness_responds_to_the_single_key() {
        let view = view_back(BASE_YAW);
        let left = Lighting::new(-55, view);
        let right = Lighting::new(55, view);
        let smooth = Material { roughness: 0.3 };
        let rough = Material { roughness: 0.7 };
        assert!(left.coordinate(left.half, smooth) > left.coordinate(left.half, rough));
        assert!(left.coordinate(left.half, smooth) > right.coordinate(left.half, smooth));
        for lighting in [left, right] {
            for normal in [Vec3::X, Vec3::Y, Vec3::Z, -Vec3::X, -Vec3::Y, -Vec3::Z] {
                let u = lighting.coordinate(normal, smooth);
                assert!((texel_center(0.0)..=texel_center(1.0)).contains(&u));
            }
        }
    }

    #[test]
    fn bind_space_roughness_varies_and_is_continuous() {
        let point = Vec3::new(0.0, 1.0 / 19.0, -1.0 / 37.0);
        let left = Material::at(point - Vec3::splat(0.000_001));
        let right = Material::at(point + Vec3::splat(0.000_001));
        assert!((left.roughness - right.roughness).abs() < 0.0001);
        let distinct = Material::at(point + Vec3::X * 0.07);
        assert!((distinct.roughness - left.roughness).abs() > 0.005);
    }

    #[test]
    fn lookup_is_an_opaque_single_row_with_increasing_brightness() {
        let rgba = lookup_rgba();
        assert_eq!(rgba.len(), SIZE * 4);
        assert!(rgba.chunks_exact(4).all(|pixel| pixel[3] == 255));
        let pixel = |x: usize| &rgba[4 * x..][..3];
        let dark = pixel(15);
        let light = pixel(45);
        assert!(dark.iter().zip(light).all(|(dark, light)| dark < light));
        assert_eq!(pixel(0), [0; 3]);
    }
}
