use anyhow::{Context, Result, ensure};
use glam::Vec2;
use trueos::vmedia::{self, ImageFormat};

use super::{gpu_error, normal_detail::sample_normal};

const PNG: &[u8] = include_bytes!("../../assets/AvatarSample_B-belly-normal.png");
const SIZE: usize = 32;
const ATLAS_SIZE: f32 = 1024.0;
const ORIGIN: Vec2 = Vec2::new(240.0, 712.0);

struct Fold {
    center: Vec2,
    half_width: f32,
    curvature: f32,
    lean: f32,
    width: f32,
    depth: f32,
}

// A short crease above and to one side of the dimple, and a curved fold below it.
const FOLDS: [Fold; 2] = [
    Fold {
        center: Vec2::new(13.7, 15.3),
        half_width: 5.2,
        curvature: 1.25,
        lean: 0.30,
        width: 1.50,
        depth: 0.16,
    },
    Fold {
        center: Vec2::new(16.2, 23.3),
        half_width: 7.4,
        curvature: -2.40,
        lean: 0.25,
        width: 1.35,
        depth: 0.26,
    },
];

pub(super) struct BellyDetail {
    rgba: Vec<u8>,
}

impl BellyDetail {
    pub(super) async fn load() -> Result<Self> {
        let image = vmedia::decode(ImageFormat::Png, PNG)
            .await
            .map_err(gpu_error)
            .context("decode the belly normal crop")?;
        ensure!(
            image.info.width as usize == SIZE
                && image.info.height as usize == SIZE
                && image.info.stride_bytes as usize == SIZE * 4
                && image.rgba.len() == SIZE * SIZE * 4,
            "unexpected belly normal crop extent"
        );
        Ok(Self { rgba: image.rgba })
    }

    pub(super) fn tangent_offset(&self, uv: Vec2) -> Vec2 {
        let local = uv * ATLAS_SIZE - ORIGIN;
        let fade = edge_fade(local);
        if fade == 0.0 {
            return Vec2::ZERO;
        }

        // Fade the local relief into the mesh over four texels. Express the
        // source normal as a slope so the center keeps its authored direction.
        let normal = sample_normal(&self.rgba, SIZE, local / SIZE as f32);
        normal.truncate() * (fade / normal.z.max(0.25))
    }
}

pub(super) fn fold_offset(uv: Vec2) -> Vec2 {
    let local = uv * ATLAS_SIZE - ORIGIN;
    let fade = edge_fade(local);
    if fade == 0.0 {
        return Vec2::ZERO;
    }

    let height = |point: Vec2| {
        FOLDS
            .iter()
            .map(|fold| {
                let x = (point.x - fold.center.x) / fold.half_width;
                let taper = (1.0 - x * x).max(0.0);
                let curve_y = fold.center.y + fold.curvature * x * x + fold.lean * x;
                let distance = (point.y - curve_y) / fold.width;
                -fold.depth * taper * taper * (-0.5 * distance * distance).exp()
            })
            .sum::<f32>()
    };
    let u = Vec2::X * 0.5;
    let v = Vec2::Y * 0.5;
    Vec2::new(
        height(local - u) - height(local + u),
        height(local - v) - height(local + v),
    ) * fade
}

fn edge_fade(local: Vec2) -> f32 {
    let edge = local.min(Vec2::splat(SIZE as f32) - local).min_element();
    let t = ((edge - 1.0) / 4.0).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
