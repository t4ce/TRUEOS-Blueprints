use anyhow::{Context, Result, ensure};
use trueos::{
    vgpu::Device,
    vmedia::{self, ImageFormat, RetainedTexture},
};

use super::{MATERIALS, gpu_error, report};
use crate::mesh::Mesh;

const SIZE: u32 = 4096;
const FACE: &[u8] = include_bytes!("../../assets/AvatarSample_B-face-base-color-4096.png");
const BODY: &[u8] = include_bytes!("../../assets/AvatarSample_B-body-base-color-4096.png");

pub(super) fn apply_coordinates(
    glb: &[u8],
    mesh: &Mesh,
    coordinates: &mut [Option<[f32; 2]>],
) -> Result<()> {
    let asset = gltf::Gltf::from_slice(glb).context("parse base-color mesh")?;
    let blob = asset.blob.as_deref().context("mesh has no BIN chunk")?;
    // The face and mouth have opaque coverage in atlas #0. The iris uses blend.
    for section in [0, 1, 3] {
        let material = MATERIALS[section];
        let primitive = asset
            .meshes()
            .flat_map(|mesh| mesh.primitives())
            .find(|primitive| primitive.material().name() == Some(material))
            .with_context(|| format!("missing base-color material {material}"))?;
        let reader = primitive
            .reader(|buffer| matches!(buffer.source(), gltf::buffer::Source::Bin).then_some(blob));
        let uv = reader
            .read_tex_coords(0)
            .with_context(|| format!("missing TEXCOORD_0 for {material}"))?
            .into_f32();
        let destination = &mut coordinates[mesh.material_vertices[section].clone()];
        ensure!(uv.len() == destination.len(), "base-color UV count changed");
        for (out, uv) in destination.iter_mut().zip(uv) {
            ensure!(
                uv.into_iter().all(f32::is_finite),
                "nonfinite base-color UV"
            );
            *out = Some(uv);
        }
    }
    Ok(())
}

pub(super) async fn retain(device: Device) -> Result<[RetainedTexture; 2]> {
    let face = retain_atlas(device, FACE).await.context("face atlas #0")?;
    let body = retain_atlas(device, BODY).await.context("body atlas #3")?;
    report(format_args!(
        "skin base color atlases=2 extent={SIZE}x{SIZE} sampling=fragment rgba_bytes={}",
        u64::from(SIZE) * u64::from(SIZE) * 8,
    ));
    Ok([face, body])
}

async fn retain_atlas(device: Device, png: &[u8]) -> Result<RetainedTexture> {
    let texture = vmedia::decode_retained(device, ImageFormat::Png, png)
        .await
        .map_err(gpu_error)
        .context("retain the original base-color PNG")?;
    let info = texture.info();
    ensure!(
        info.width == SIZE && info.height == SIZE && info.stride_bytes == SIZE * 4,
        "unexpected base-color atlas extent"
    );
    Ok(texture)
}
