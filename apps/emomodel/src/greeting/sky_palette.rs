use anyhow::{Context, Result};
use glam::{Vec2, Vec3};
use trueos::{
    vgpu::Device,
    vmedia::{self, ImageFormat, RetainedTexture},
};

use super::sky_light::{
    Key, Levels, SUNRISE_RGB, SkyLight, encode_rgb, linear_rgb, oct_decode, oct_encode,
};
use crate::{
    autobs::{BASE_YAW, camera_back},
    light::{AMBIENT, SKIN_SRGB, diffuse_energy},
    preview::gpu_error,
};

pub(super) const WIDTH: usize = 64;
pub(super) const HEIGHT: usize = 32;
const LUMA: Vec3 = Vec3::new(0.2126, 0.7152, 0.0722);
const TINT: Vec3 = Vec3::new(-0.5549, 0.064, 1.0);
const MIN_TINT: f32 = -0.10;
const MIN_SUNRISE_TINT: f32 = -0.28;
const MIN_KELVIN_TINT: f32 = -0.48;
const MAX_TINT: f32 = 0.36;
const MAX_ENERGY: f32 = 2.15;

pub(super) struct Palette {
    pub(super) base: Vec3,
    key: Key,
    min_tint: f32,
    luma_scale: f32,
    pub(super) rgba: Vec<u8>,
}

impl Palette {
    pub(super) fn new(key: Key) -> Self {
        let base = linear_rgb(SKIN_SRGB);
        let luma_scale = base.dot(LUMA).recip();
        let min_tint = match key {
            Key::Neutral => MIN_TINT,
            Key::Sunrise => MIN_SUNRISE_TINT,
            Key::Kelvin(_) => MIN_KELVIN_TINT,
        };
        // On the warm side, fit the sunlit skin hue at the same luminance.
        // Both axes use the same blue coordinate and meet at neutral skin.
        let chroma = |light: Vec3| {
            let lit = base * light;
            lit - base * (lit.dot(LUMA) * luma_scale)
        };
        let sunlight = chroma(SUNRISE_RGB);
        let mut warm_axis = sunlight / sunlight.z;
        let mut cool_axis = TINT;
        if let Key::Kelvin(_) = key {
            let chroma = chroma(key.color());
            // Fit the selected key's tint. Ease toward the sky axes near
            // neutral, where the blue chroma is too small to define the axis.
            let axis = if chroma.z < 0.0 {
                &mut warm_axis
            } else {
                &mut cool_axis
            };
            let extent = chroma.z.abs().max(0.06);
            let amount = chroma.z / extent;
            *axis = *axis * (1.0 - amount * amount) + chroma * (amount / extent);
        }
        let mut rgba = Vec::with_capacity(WIDTH * HEIGHT * 4);
        for row in 0..HEIGHT {
            let tint = min_tint + (MAX_TINT - min_tint) * row as f32 / (HEIGHT - 1) as f32;
            let axis = if key != Key::Neutral && tint < 0.0 {
                warm_axis
            } else {
                cool_axis
            };
            for column in 0..WIDTH {
                let energy = MAX_ENERGY * column as f32 / (WIDTH - 1) as f32;
                rgba.extend(encode_rgb(base * energy + axis * tint));
                rgba.push(255);
            }
        }
        Self {
            base,
            key,
            min_tint,
            luma_scale,
            rgba,
        }
    }

    pub(super) fn update(&mut self, key: Key) {
        if self.key != key {
            *self = Self::new(key);
        }
    }

    pub(super) fn coordinates(&self, color: Vec3) -> [f32; 2] {
        // The existing textured draw interpolates two values. Store brightness and
        // the warm/cool variation of the sky in those two coordinates.
        let energy = color.dot(LUMA) * self.luma_scale;
        let chroma = color - self.base * energy;
        let tint = if self.key != Key::Neutral {
            chroma.z
        } else {
            chroma.dot(TINT) / TINT.length_squared()
        };
        let u = (energy / MAX_ENERGY).clamp(0.0, 1.0);
        let v = ((tint - self.min_tint) / (MAX_TINT - self.min_tint)).clamp(0.0, 1.0);
        [u * (WIDTH - 1) as f32 + 0.5, v * (HEIGHT - 1) as f32 + 0.5]
    }
}

const NORMAL_SIZE: usize = 64;
const BMP_HEADER: usize = 54;

pub(super) struct NormalPalette {
    texture: RetainedTexture,
    view_back: Vec3,
    levels: Levels,
}

impl NormalPalette {
    pub(super) fn coordinates(normal: Vec3) -> [f32; 2] {
        let back = camera_back(BASE_YAW);
        let right = Vec3::Y.cross(back);
        let projected = Vec3::new(normal.dot(right), normal.dot(back), normal.y);
        // Center the octahedral projection on the observer. The texture's
        // seam runs along normals pointing away from the observer.
        oct_encode(projected)
            .to_array()
            .map(|uv| (uv * (NORMAL_SIZE - 1) as f32 + 0.5) / NORMAL_SIZE as f32)
    }

    pub(super) async fn new(
        device: Device,
        view_back: Vec3,
        levels: Levels,
        sky: &SkyLight,
    ) -> Result<Self> {
        const BYTES: usize = BMP_HEADER + NORMAL_SIZE * NORMAL_SIZE * 3;
        let mut bmp = vec![0; BYTES];
        bmp[..2].copy_from_slice(b"BM");
        for (offset, value) in [
            (2, BYTES),
            (10, BMP_HEADER),
            (14, 40),
            (18, NORMAL_SIZE),
            (22, NORMAL_SIZE),
            (34, BYTES - BMP_HEADER),
        ] {
            bmp[offset..offset + 4].copy_from_slice(&(value as u32).to_le_bytes());
        }
        bmp[26..30].copy_from_slice(&[1, 0, 24, 0]);
        let back = camera_back(BASE_YAW);
        let right = Vec3::Y.cross(back);
        let base = linear_rgb(SKIN_SRGB);
        for (i, pixel) in bmp[BMP_HEADER..].chunks_exact_mut(3).enumerate() {
            let u = (i % NORMAL_SIZE) as f32 / (NORMAL_SIZE - 1) as f32;
            let v = (NORMAL_SIZE - 1 - i / NORMAL_SIZE) as f32 / (NORMAL_SIZE - 1) as f32;
            let projected = oct_decode(Vec2::new(u, v));
            let normal = right * projected.x + back * projected.y + Vec3::Y * projected.z;
            let key = base * diffuse_energy(normal, sky.key_direction());
            let [r, g, b] = encode_rgb(sky.apply(normal, base, key, AMBIENT));
            pixel.copy_from_slice(&[b, g, r]);
        }
        let texture = vmedia::decode_retained(device, ImageFormat::Bmp, &bmp)
            .await
            .map_err(gpu_error)
            .context("retain the sky skin lighting lookup")?;
        Ok(Self {
            texture,
            view_back,
            levels,
        })
    }

    pub(super) async fn update(
        &mut self,
        device: Device,
        view_back: Vec3,
        levels: Levels,
        sky: &SkyLight,
    ) -> Result<()> {
        if self.levels != levels || self.view_back.dot(view_back) < 0.99999 {
            *self = Self::new(device, view_back, levels, sky).await?;
        }
        Ok(())
    }

    pub(super) fn texture(&self) -> &RetainedTexture {
        &self.texture
    }
}
