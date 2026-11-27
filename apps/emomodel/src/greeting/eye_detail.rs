use anyhow::{Context, Result, ensure};
use gltf::Semantic;
use trueos::vmedia::{self, ImageFormat};

use super::gpu_error;
use crate::{cutout::Cutout, mesh::FACE_MATERIAL};

pub(super) const WIDTH: usize = 435;
pub(super) const HEIGHT: usize = 78;
const SOURCE_WIDTH: usize = 1738;
const SOURCE_HEIGHT: usize = 309;
const PNG: &[u8] = include_bytes!("../../assets/AvatarSample_B-eyebrows-and-lashes.png");
const CUTOUT: &[u8] = include_bytes!("../../assets/AvatarSample_B-eye-cutout.bin");
const EYE_MATERIAL: &str = "N00_000_00_EyeHighlight_00_EYE (Instance)";

pub(super) struct EyeDetail {
    cutout: Cutout,
    rgba: Vec<u8>,
}

impl EyeDetail {
    pub(super) async fn load(glb: &[u8]) -> Result<Self> {
        let cutout = cutout(glb)?;
        let image = vmedia::decode(ImageFormat::Png, PNG)
            .await
            .map_err(gpu_error)
            .context("decode the eyebrow and lash atlas")?;
        ensure!(
            image.info.width as usize == SOURCE_WIDTH
                && image.info.height as usize == SOURCE_HEIGHT
                && image.info.stride_bytes as usize == SOURCE_WIDTH * 4
                && image.rgba.len() == SOURCE_WIDTH * SOURCE_HEIGHT * 4,
            "unexpected eyebrow and lash atlas extent"
        );
        let rgba = downsample(&image.rgba);
        Ok(Self { cutout, rgba })
    }

    pub(super) fn len(&self) -> usize {
        self.cutout.len()
    }

    pub(super) fn indices(&self) -> &[u32] {
        self.cutout.indices()
    }

    pub(super) fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    pub(super) fn vertices<'a>(
        &'a self,
        positions: &'a [[f32; 3]],
    ) -> impl Iterator<Item = ([f32; 3], [f32; 2])> + 'a {
        self.cutout.vertices(positions)
    }
}

fn cutout(glb: &[u8]) -> Result<Cutout> {
    let avatar = gltf::Gltf::from_slice(glb).context("parse the eye-detail avatar")?;
    let blob = avatar.blob.as_deref().context("avatar has no BIN chunk")?;
    let (face, overlay) = avatar
        .nodes()
        .filter(|node| node.skin().is_some())
        .find_map(|node| {
            let mesh = node.mesh()?;
            let by_material = |name| {
                mesh.primitives()
                    .find(|part| part.material().name() == Some(name))
            };
            Some((by_material(FACE_MATERIAL)?, by_material(EYE_MATERIAL)?))
        })
        .context("missing skinned face and eye-detail geometry")?;
    for semantic in [
        Semantic::Positions,
        Semantic::Joints(0),
        Semantic::Weights(0),
    ] {
        let face = face.get(&semantic).context("missing face skin attribute")?;
        ensure!(
            overlay.get(&semantic).map(|attr| attr.index()) == Some(face.index()),
            "eye detail must share the skinned face vertices"
        );
    }
    ensure!(
        overlay.mode() == gltf::mesh::Mode::Triangles,
        "eye detail requires source triangles"
    );
    let source_count = face
        .get(&Semantic::Positions)
        .context("missing face positions")?
        .count();
    let reader = overlay
        .reader(|buffer| matches!(buffer.source(), gltf::buffer::Source::Bin).then_some(blob));
    let source_indices: Vec<_> = reader
        .read_indices()
        .context("missing eye-detail triangle indices")?
        .into_u32()
        .collect();
    let (triangles, extra) = source_indices.as_chunks::<3>();
    ensure!(
        !triangles.is_empty()
            && extra.is_empty()
            && source_indices
                .iter()
                .all(|&index| (index as usize) < source_count),
        "invalid eye-detail source triangles"
    );
    Cutout::read(CUTOUT, triangles.iter().copied(), 0).context("read the eye cutout")
}

fn downsample(source: &[u8]) -> Vec<u8> {
    let mut pixels = vec![[0; 4]; WIDTH * HEIGHT];
    // In integer subpixel units an input texel spans WIDTH by HEIGHT, and an
    // output texel spans SOURCE_WIDTH by SOURCE_HEIGHT.
    let area = (SOURCE_WIDTH * SOURCE_HEIGHT) as u64;
    for y in 0..HEIGHT {
        let top = y * SOURCE_HEIGHT;
        let bottom = top + SOURCE_HEIGHT;
        for x in 0..WIDTH {
            let left = x * SOURCE_WIDTH;
            let right = left + SOURCE_WIDTH;
            let mut total_alpha = 0;
            let mut total_rgb = [0; 3];
            for sy in top / HEIGHT..bottom.div_ceil(HEIGHT) {
                let height = bottom.min((sy + 1) * HEIGHT) - top.max(sy * HEIGHT);
                for sx in left / WIDTH..right.div_ceil(WIDTH) {
                    let width = right.min((sx + 1) * WIDTH) - left.max(sx * WIDTH);
                    let index = (sy * SOURCE_WIDTH + sx) * 4;
                    let weighted_alpha = (width * height) as u64 * u64::from(source[index + 3]);
                    total_alpha += weighted_alpha;
                    for (channel, total) in total_rgb.iter_mut().enumerate() {
                        *total += weighted_alpha * u64::from(source[index + channel]);
                    }
                }
            }
            let alpha = ((total_alpha + area / 2) / area) as u8;
            if alpha != 0 {
                let [r, g, b] = total_rgb.map(|sum| ((sum + total_alpha / 2) / total_alpha) as u8);
                pixels[y * WIDTH + x] = [r, g, b, alpha];
            }
        }
    }

    // Extend nearby RGB through a single transparent texel so texture filtering
    // keeps the original lash colors at the edges.
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let index = y * WIDTH + x;
            if pixels[index][3] == 0 {
                let neighbors = y.saturating_sub(1)..=(y + 1).min(HEIGHT - 1);
                let [r, g, b, _] = neighbors
                    .flat_map(|row| {
                        (x.saturating_sub(1)..=(x + 1).min(WIDTH - 1)).map(move |x| row * WIDTH + x)
                    })
                    .map(|index| pixels[index])
                    .filter(|rgba| rgba[3] != 0)
                    .max_by_key(|rgba| rgba[3])
                    .unwrap_or([0; 4]);
                pixels[index] = [r, g, b, 0];
            }
        }
    }
    // The cutout triangles define coverage for both native draw paths.
    for pixel in &mut pixels {
        pixel[3] = 255;
    }
    pixels.into_flattened()
}
