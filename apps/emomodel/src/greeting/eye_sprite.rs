use anyhow::{Context, Result, ensure};
use glam::{Vec2, Vec3};
use trueos::vmedia::{self, ImageFormat};

use super::gpu_error;
use crate::EyeStyle;

const PNG: &[u8] = include_bytes!("../../assets/lily-eyes.png");
const SOURCE_SIZE: usize = 512;
const STYLES: usize = 3;
pub(super) const SIZE: usize = 128;
const SAMPLES: usize = SOURCE_SIZE / SIZE;
const RIM: u32 = 48;
const RIM_ALPHA: u8 = 160;
const PUPIL_RIM: u32 = 24;
const IRIS_DEPTH: f32 = 0.000_35;
const PUPIL_DEPTH: f32 = 0.000_425;
pub(super) const VERTEX_COUNT: u32 = 2 * (1 + RIM);
pub(super) const INDEX_COUNT: usize = 2 * RIM as usize * 3;
pub(super) const PUPIL_VERTEX_COUNT: u32 = 2 * (1 + PUPIL_RIM);
pub(super) const PUPIL_INDEX_COUNT: usize = 2 * PUPIL_RIM as usize * 3;

// Pupil center and radii in each source atlas cell, in EyeStyle order.
const PUPILS: [([f32; 2], [f32; 2]); STYLES] = [
    ([255.5, 246.0], [40.0, 63.0]),
    ([257.5, 242.5], [58.0, 75.0]),
    ([256.0, 233.5], [16.0, 58.0]),
];

// Center, screen-left, screen-right, top and bottom in the Face position
// accessor. Both eyes use its skinned vertices to follow their eye joints.
const ANCHORS: [[usize; 5]; 2] = [
    [1239, 1252, 1292, 1198, 1223],
    [1109, 1226, 1190, 1131, 1255],
];

pub(super) struct EyeSprite {
    rgba: Vec<u8>,
    center: Vec2,
    half_extent: Vec2,
    rim: [Vec2; RIM as usize],
    pupil_center: Vec2,
    pupil_rim: [Vec2; PUPIL_RIM as usize],
}

struct EyePlane {
    center: Vec3,
    right: Vec3,
    down: Vec3,
    front: Vec3,
}

impl EyePlane {
    fn project(&self, offset: Vec2, depth: f32) -> Vec3 {
        self.center + self.right * offset.x + self.down * offset.y + self.front * depth
    }
}

impl EyeSprite {
    pub(super) async fn load(style: EyeStyle) -> Result<Self> {
        let image = vmedia::decode(ImageFormat::Png, PNG)
            .await
            .map_err(gpu_error)
            .context("decode the Lily eye atlas")?;
        let stride = SOURCE_SIZE * STYLES * 4;
        ensure!(
            image.info.width as usize == SOURCE_SIZE * STYLES
                && image.info.height as usize == SOURCE_SIZE
                && image.info.stride_bytes as usize == stride
                && image.rgba.len() == stride * SOURCE_SIZE,
            "unexpected Lily eye atlas extent"
        );
        Self::prepare(&image.rgba, style as usize, stride)
    }

    fn prepare(source: &[u8], cell: usize, stride: usize) -> Result<Self> {
        let mut rgba = Vec::with_capacity(SIZE * SIZE * 4);
        let mut low = Vec2::splat(SIZE as f32);
        let mut high = Vec2::ZERO;
        for y in 0..SIZE {
            for x in 0..SIZE {
                let mut alpha = 0_u32;
                let mut color = [0_u32; 3];
                let start = (y * stride + x * 4) * SAMPLES + cell * SOURCE_SIZE * 4;
                for row in 0..SAMPLES {
                    let offset = start + row * stride;
                    for pixel in source[offset..offset + SAMPLES * 4].chunks_exact(4) {
                        let a = u32::from(pixel[3]);
                        alpha += a;
                        for (channel, sum) in color.iter_mut().enumerate() {
                            *sum += u32::from(pixel[channel]) * a;
                        }
                    }
                }
                let a = (alpha / (SAMPLES * SAMPLES) as u32) as u8;
                if a >= RIM_ALPHA {
                    let point = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    low = low.min(point);
                    high = high.max(point);
                }
                let rgb = color.map(|sum| ((sum + alpha / 2) / alpha.max(1)) as u8);
                rgba.extend_from_slice(&[rgb[0], rgb[1], rgb[2], a]);
            }
        }
        let center = (low + high) * 0.5;
        let half_extent = (high - low) * 0.5;
        let opaque = |point: Vec2| {
            let [x, y] = point.to_array();
            (0.0..SIZE as f32).contains(&x)
                && (0.0..SIZE as f32).contains(&y)
                && rgba[(y as usize * SIZE + x as usize) * 4 + 3] >= RIM_ALPHA
        };
        ensure!(
            half_extent.min_element() > 0.0 && opaque(center),
            "Lily eye sprite has no opaque center"
        );
        // Both native passes use opaque depth testing. The fan clips to the
        // iris silhouette so the transparent gutters reveal the authored sclera.
        let rim = std::array::from_fn(|i| {
            let angle = std::f32::consts::TAU * i as f32 / RIM as f32;
            let direction = Vec2::from_angle(angle) * half_extent;
            let mut inside = 0.0;
            let mut outside = 1.5;
            for _ in 0..8 {
                let distance = (inside + outside) * 0.5;
                if opaque(center + direction * distance) {
                    inside = distance;
                } else {
                    outside = distance;
                }
            }
            center + direction * inside
        });
        for alpha in rgba[3..].iter_mut().step_by(4) {
            *alpha = 255;
        }
        let (pupil_center, pupil_radii) = PUPILS[cell];
        let pupil_center = Vec2::from_array(pupil_center) / SAMPLES as f32;
        let pupil_radii = Vec2::from_array(pupil_radii) / SAMPLES as f32;
        let pupil_rim = std::array::from_fn(|i| {
            let angle = std::f32::consts::TAU * i as f32 / PUPIL_RIM as f32;
            Vec2::from_angle(angle) * pupil_radii
        });
        Ok(Self {
            rgba,
            center,
            half_extent,
            rim,
            pupil_center,
            pupil_rim,
        })
    }

    pub(super) fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    pub(super) fn indices(base: u32) -> impl Iterator<Item = [u32; 3]> {
        fan_indices(base, RIM)
    }

    pub(super) fn pupil_indices(base: u32) -> impl Iterator<Item = [u32; 3]> {
        fan_indices(base, PUPIL_RIM)
    }

    pub(super) fn vertices<'a>(
        &'a self,
        positions: &'a [[f32; 3]],
    ) -> impl Iterator<Item = (Vec3, [f32; 2])> + 'a {
        self.planes(positions).flat_map(move |eye| {
            std::iter::once(self.center)
                .chain(self.rim)
                .map(move |pixel| {
                    let offset = (pixel - self.center) / self.half_extent;
                    (eye.project(offset, IRIS_DEPTH), pixel.to_array())
                })
        })
    }

    pub(super) fn pupil_vertices<'a>(
        &'a self,
        positions: &'a [[f32; 3]],
        area: f32,
    ) -> impl Iterator<Item = Vec3> + 'a {
        let scale = area.clamp(1.0, 4.0).sqrt();
        self.planes(positions).flat_map(move |eye| {
            std::iter::once(Vec2::ZERO)
                .chain(self.pupil_rim)
                .map(move |radial| {
                    let pixel = self.pupil_center + radial * scale;
                    let offset = (pixel - self.center) / self.half_extent;
                    eye.project(offset, PUPIL_DEPTH)
                })
        })
    }

    fn planes<'a>(&'a self, positions: &'a [[f32; 3]]) -> impl Iterator<Item = EyePlane> + 'a {
        ANCHORS.into_iter().enumerate().map(move |(eye, anchors)| {
            let [center, left, right, top, bottom] =
                anchors.map(|index| Vec3::from_array(positions[index]));
            let down = (bottom - top) * 0.5;
            let right = (right - left) * (0.5 * self.half_extent.x / self.half_extent.y);
            let front = down.cross(right).normalize_or_zero();
            let mirror = if eye == 0 { 1.0 } else { -1.0 };
            EyePlane {
                center,
                right: right * mirror,
                down,
                front,
            }
        })
    }
}

fn fan_indices(base: u32, rim: u32) -> impl Iterator<Item = [u32; 3]> {
    (0..2).flat_map(move |eye| {
        let center = base + eye * (1 + rim);
        (0..rim).map(move |i| {
            let a = center + 1 + i;
            let b = center + 1 + (i + 1) % rim;
            if eye == 0 {
                [center, b, a]
            } else {
                [center, a, b]
            }
        })
    })
}
