use anyhow::{Context, Result, ensure};
use glam::Vec2;
use trueos::vmedia::{self, ImageFormat};

use super::gpu_error;

const PNG: &[u8] = include_bytes!("../../assets/AvatarSample_B-ear-height.png");
const WIDTH: usize = 454;
const HEIGHT: usize = 283;
const ATLAS_SIZE: f32 = 4096.0;
const ORIGIN: Vec2 = Vec2::new(20.0, 11.0);
const NEUTRAL: f32 = 128.0;
const DIFFERENCE_RADIUS: f32 = 4.0;
const SLOPE_GAIN: f32 = 0.018;
const MAX_SLOPE: f32 = 0.24;

pub(super) struct EarDetail {
    rgba: Vec<u8>,
}

impl EarDetail {
    pub(super) async fn load() -> Result<Self> {
        let image = vmedia::decode(ImageFormat::Png, PNG)
            .await
            .map_err(gpu_error)
            .context("decode shared ear height")?;
        ensure!(
            image.info.width as usize == WIDTH
                && image.info.height as usize == HEIGHT
                && image.info.stride_bytes as usize == WIDTH * 4
                && image.rgba.len() == WIDTH * HEIGHT * 4,
            "unexpected ear height extent"
        );
        Ok(Self { rgba: image.rgba })
    }

    pub(super) fn tangent_offset(&self, uv: Vec2) -> Vec2 {
        // The two front islands mirror around U=0.25 in the face atlas.
        let (u, handedness) = if uv.x > 0.25 {
            (0.5 - uv.x, -1.0)
        } else {
            (uv.x, 1.0)
        };
        let local = Vec2::new(u, uv.y) * ATLAS_SIZE - ORIGIN;
        if local.min_element() < 0.0 || local.x >= WIDTH as f32 || local.y >= HEIGHT as f32 {
            return Vec2::ZERO;
        }

        let pixel = local - Vec2::splat(0.5);
        let step_u = Vec2::X * DIFFERENCE_RADIUS;
        let step_v = Vec2::Y * DIFFERENCE_RADIUS;
        let du = self.sample(pixel + step_u) - self.sample(pixel - step_u);
        let dv = self.sample(pixel + step_v) - self.sample(pixel - step_v);
        // The normal leans away from increasing height. Mirroring reverses U alone.
        (Vec2::new(du * handedness, dv) * -SLOPE_GAIN).clamp_length_max(MAX_SLOPE)
    }

    fn sample(&self, pixel: Vec2) -> f32 {
        let lower = pixel.floor();
        let blend = pixel - lower;
        let x = lower.x as i32;
        let y = lower.y as i32;
        let read = |x: i32, y: i32| {
            if (0..WIDTH as i32).contains(&x) && (0..HEIGHT as i32).contains(&y) {
                f32::from(self.rgba[(y as usize * WIDTH + x as usize) * 4])
            } else {
                NEUTRAL
            }
        };
        let cross = |y| {
            let left = read(x, y);
            left + (read(x + 1, y) - left) * blend.x
        };
        let top = cross(y);
        top + (cross(y + 1) - top) * blend.y
    }
}
