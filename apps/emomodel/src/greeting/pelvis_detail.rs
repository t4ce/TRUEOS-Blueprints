use anyhow::{Context, Result, ensure};
use glam::{Vec2, Vec3};
use trueos::vmedia::{self, ImageFormat};

use super::{gpu_error, normal_detail::sample_normal};

const PNG: &[u8] = include_bytes!("../../assets/AvatarSample_B-pelvis-normal.png");
const SIZE: usize = 24;
const ATLAS_SIZE: f32 = 1024.0;
const ORIGIN: Vec2 = Vec2::new(244.0, 781.0);

// Each group is one bind position with identical skin weights. The first
// two indices are the distinct shading normals, followed by any UV copy.
const FRONT_SEAM: &[&[usize]] = &[
    &[2, 103, 2262],
    &[1175, 1395, 3057],
    &[988, 2239],
    &[1146, 3060],
    &[61, 1009, 2234],
    &[1128, 2377, 3068],
    &[55, 1016, 401],
    &[1114, 1615, 2380],
];

pub(super) fn smooth_front_seam(normals: &mut [[f32; 3]]) {
    for group in FRONT_SEAM {
        let a = Vec3::from_array(normals[group[0]]);
        let b = Vec3::from_array(normals[group[1]]);
        let blended = (a + b).normalize().to_array();
        for &index in *group {
            normals[index] = blended;
        }
    }
}

pub(super) struct PelvisDetail {
    rgba: Vec<u8>,
}

impl PelvisDetail {
    pub(super) async fn load() -> Result<Self> {
        let image = vmedia::decode(ImageFormat::Png, PNG)
            .await
            .map_err(gpu_error)
            .context("decode the pelvis normal crop")?;
        ensure!(
            image.info.width as usize == SIZE
                && image.info.height as usize == SIZE
                && image.info.stride_bytes as usize == SIZE * 4
                && image.rgba.len() == SIZE * SIZE * 4,
            "unexpected pelvis normal crop extent"
        );
        Ok(Self { rgba: image.rgba })
    }

    pub(super) fn tangent_offset(&self, uv: Vec2) -> Vec2 {
        let local = uv * ATLAS_SIZE - ORIGIN;
        let edge = local.min(Vec2::splat(SIZE as f32) - local).min_element();
        // Keep the authored detail inside the continuous midline island.
        let t = ((edge - 2.0) / 4.0).clamp(0.0, 1.0);
        if t == 0.0 {
            return Vec2::ZERO;
        }
        let normal = sample_normal(&self.rgba, SIZE, local / SIZE as f32);
        let fade = t * t * (3.0 - 2.0 * t);
        normal.truncate() * (fade / normal.z.max(0.25))
    }
}
