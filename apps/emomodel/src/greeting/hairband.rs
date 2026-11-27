use std::ops::Range;

use anyhow::{Context, Result, ensure};

use crate::{
    cutout::Cutout,
    mesh::{HAIR_MATERIAL, is_hairband_triangle},
};

#[cfg(target_os = "trueos")]
use crate::mesh::Mesh;
#[cfg(target_os = "trueos")]
use trueos::vmedia::{self, ImageFormat};

pub(super) const WIDTH: usize = 215;
pub(super) const HEIGHT: usize = 268;
const PNG: &[u8] = include_bytes!("../../assets/AvatarSample_B-hairband.png");
const CUTOUT: &[u8] = include_bytes!("../../assets/AvatarSample_B-hairband-cutout.bin");

#[cfg(target_os = "trueos")]
pub(super) struct Hairband {
    cutout: Cutout,
    rgba: Vec<u8>,
}

#[cfg(target_os = "trueos")]
impl Hairband {
    pub(super) async fn load(glb: &[u8], mesh: &Mesh) -> Result<Self> {
        let cutout = cutout(glb, mesh.material_vertices[2].clone())?;
        let image = vmedia::decode(ImageFormat::Png, PNG)
            .await
            .map_err(super::gpu_error)
            .context("decode the hairband texture")?;
        ensure!(
            image.info.width as usize == WIDTH
                && image.info.height as usize == HEIGHT
                && image.info.stride_bytes as usize == WIDTH * 4
                && image.rgba.len() == WIDTH * HEIGHT * 4,
            "unexpected hairband texture extent"
        );
        let mut rgba = image.rgba;
        // The cutout mesh supplies coverage for both depth-writing renderers.
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
        Ok(Self { cutout, rgba })
    }

    pub(super) fn len(&self) -> usize {
        self.cutout.len()
    }

    pub(super) fn indices(&self) -> &[u32] {
        self.cutout.indices()
    }

    pub(super) fn vertices<'a>(
        &'a self,
        positions: &'a [[f32; 3]],
    ) -> impl Iterator<Item = ([f32; 3], [f32; 2])> + 'a {
        self.cutout.vertices(positions)
    }

    pub(super) fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}

fn cutout(glb: &[u8], vertices: Range<usize>) -> Result<Cutout> {
    let asset = gltf::Gltf::from_slice(glb).context("parse the hairband mesh")?;
    let blob = asset
        .blob
        .as_deref()
        .context("hair mesh has no BIN chunk")?;
    let primitive = asset
        .meshes()
        .flat_map(|mesh| mesh.primitives())
        .find(|primitive| primitive.material().name() == Some(HAIR_MATERIAL))
        .context("missing hair material for the hairband")?;
    ensure!(
        primitive
            .get(&gltf::Semantic::Positions)
            .is_some_and(|positions| positions.count() == vertices.len()),
        "hairband vertex range differs from the hair mesh"
    );
    let reader = primitive
        .reader(|buffer| matches!(buffer.source(), gltf::buffer::Source::Bin).then_some(blob));
    let indices: Vec<_> = reader
        .read_indices()
        .context("missing hair mesh indices")?
        .into_u32()
        .collect();
    let (triangles, extra) = indices.as_chunks::<3>();
    ensure!(extra.is_empty(), "invalid hair mesh triangle indices");
    let selected = triangles
        .iter()
        .enumerate()
        .filter(|&(index, _)| is_hairband_triangle(index))
        .map(|(_, &triangle)| triangle);
    Cutout::read(CUTOUT, selected, vertices.start).context("read the hairband cutout")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HUMANOID_GLB, animation::Animation, mesh::PREVIEW_MATERIALS};

    #[test]
    fn authored_hairband_cutout_follows_the_skinned_hair() {
        let mut avatar = Animation::nod(HUMANOID_GLB, &PREVIEW_MATERIALS, None).unwrap();
        let first_hair = avatar.bind_positions(0).len() + avatar.bind_positions(1).len();
        let hair_vertices = first_hair..first_hair + avatar.bind_positions(2).len();
        let band = cutout(HUMANOID_GLB, hair_vertices).unwrap();
        let before: Vec<_> = band.vertices(avatar.skin().positions).collect();
        avatar.apply(1.5);
        let after: Vec<_> = band.vertices(avatar.skin().positions).collect();

        assert_eq!(before.len(), band.len());
        assert!(band.indices().len() > 213 * 3);
        assert!(
            before
                .iter()
                .zip(after)
                .all(|(&(_, original_uv), (_, posed_uv))| original_uv == posed_uv)
        );
        assert!(band.vertices(avatar.skin().positions).zip(before).any(
            |((posed, _), (original, _))| {
                glam::Vec3::from_array(posed).distance(glam::Vec3::from_array(original)) > 0.005
            }
        ));
        let size = |offset| u32::from_be_bytes(PNG[offset..offset + 4].try_into().unwrap());
        assert_eq!([size(16) as usize, size(20) as usize], [WIDTH, HEIGHT]);
    }
}
