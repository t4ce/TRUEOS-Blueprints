use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec2};
#[cfg(target_os = "trueos")]
use trueos::vmedia::{self, ImageFormat};

use crate::animation::{Animation, PoseSample};

use super::{normal_detail, skirt_heart::SkirtHeart};

const GLB: &[u8] = include_bytes!("../../assets/AvatarSample_B-pink-skirt.glb");
const NORMAL_PNG: &[u8] = include_bytes!("../../assets/AvatarSample_B-pink-skirt-normal.png");
const NORMAL_SIZE: usize = 128;
const MATERIAL: &str = "N00_001_03_Bottoms_01_CLOTH (Instance)";
pub(super) const PINK: [u8; 3] = [226, 172, 187];

pub(super) struct Dress {
    rig: Animation,
    indices: Vec<u32>,
    heart: SkirtHeart,
}

impl Dress {
    #[cfg(target_os = "trueos")]
    pub(super) async fn load() -> Result<Self> {
        let image = vmedia::decode(ImageFormat::Png, NORMAL_PNG)
            .await
            .map_err(super::gpu_error)
            .context("decode the skirt normal map")?;
        ensure!(
            [image.info.width, image.info.height] == [NORMAL_SIZE as u32; 2]
                && image.info.stride_bytes == (NORMAL_SIZE * 4) as u32,
            "unexpected skirt normal map extent"
        );
        Self::from_normal_map(&image.rgba)
    }

    fn from_normal_map(normal_map: &[u8]) -> Result<Self> {
        let glb = gltf::Gltf::from_slice(GLB).context("parse the original AvatarSample_B skirt")?;
        let primitive = glb
            .meshes()
            .flat_map(|mesh| mesh.primitives())
            .find(|primitive| primitive.material().name() == Some(MATERIAL))
            .context("missing AvatarSample_B skirt material")?;
        let reader = primitive.reader(|_| glb.blob.as_deref());
        let mut indices: Vec<_> = reader
            .read_indices()
            .context("missing skirt triangle indices")?
            .into_u32()
            .collect();
        let mut rig = Animation::procedural(GLB, &[MATERIAL])?;
        let vertices = rig.bind_positions(0).len();
        ensure!(
            primitive.mode() == gltf::mesh::Mode::Triangles
                && !indices.is_empty()
                && indices.len().is_multiple_of(3)
                && indices.iter().all(|&index| (index as usize) < vertices),
            "invalid skirt topology"
        );
        let positions: Vec<_> = rig
            .bind_positions(0)
            .map(|point| point.to_array())
            .collect();
        let heart = SkirtHeart::new(&positions, &indices)?;
        if primitive.material().double_sided() {
            let front_count = indices.len();
            indices.reserve(front_count);
            for triangle in (0..front_count).step_by(3) {
                let [a, b, c] = indices[triangle..][..3].try_into().unwrap();
                indices.extend([a, c, b]);
            }
        }
        let normals = normal_detail::bake(GLB, MATERIAL, Some((normal_map, NORMAL_SIZE)), |_| {
            Vec2::ZERO
        })?;
        rig.apply_bind_normals(0, &normals)?;
        Ok(Self {
            rig,
            indices,
            heart,
        })
    }

    pub(super) fn vertex_count(&self) -> usize {
        self.rig.bind_positions(0).len()
    }

    pub(super) fn indices(&self) -> &[u32] {
        &self.indices
    }

    pub(super) fn heart(&self) -> &SkirtHeart {
        &self.heart
    }

    pub(super) fn pose(&mut self, joints: &[Mat4]) -> Result<(PoseSample<'_>, &SkirtHeart)> {
        Ok((self.rig.skin_from_world(joints)?, &self.heart))
    }
}

#[cfg(test)]
fn read_normal_map() -> Result<Vec<u8>> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(NORMAL_PNG))
        .read_info()
        .context("read the skirt normal map")?;
    let mut pixels = vec![
        0;
        reader
            .output_buffer_size()
            .context("invalid skirt normal map size")?
    ];
    let info = reader
        .next_frame(&mut pixels)
        .context("decode the skirt normal map")?;
    ensure!(
        [info.width, info.height] == [NORMAL_SIZE as u32; 2]
            && info.bit_depth == png::BitDepth::Eight,
        "unexpected skirt normal map extent"
    );
    pixels.truncate(info.buffer_size());
    match info.color_type {
        png::ColorType::Rgba => Ok(pixels),
        png::ColorType::Rgb => Ok(pixels
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect()),
        _ => anyhow::bail!("expected RGB skirt normal map"),
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;
    use crate::{HUMANOID_GLB, mesh::BODY_MATERIAL};

    #[test]
    fn extracted_skirt_skins_with_the_embedded_avatar_pose() {
        let asset = gltf::Gltf::from_slice(GLB).unwrap();
        assert_eq!(asset.textures().count(), 0);
        assert_eq!(asset.meshes().flat_map(|mesh| mesh.primitives()).count(), 1);
        assert!(PINK[0] > PINK[2] && PINK[2] > PINK[1]);

        let mut dress = Dress::from_normal_map(&read_normal_map().unwrap()).unwrap();
        let mut body = Animation::procedural(HUMANOID_GLB, &[BODY_MATERIAL]).unwrap();
        body.apply_procedural(0, Vec3::ZERO, &[]);
        let world = body.skin().joint_world;
        let mut original = Animation::procedural(GLB, &[MATERIAL]).unwrap();
        let original = original.skin_from_world(world).unwrap();
        let (textured, _) = dress.pose(world).unwrap();
        assert_eq!(textured.positions, original.positions);
        let detailed = textured
            .normals
            .iter()
            .zip(original.normals)
            .filter(|&(&a, &b)| Vec3::from_array(a).dot(Vec3::from_array(b)) < 0.9998)
            .count();
        assert!(
            detailed > textured.normals.len() / 20,
            "normal detail sampled at {detailed} vertices"
        );
        let rest = textured.positions.to_vec();
        assert_eq!(rest.len(), dress.vertex_count());
        assert!(dress.indices().len() >= 1_500);
        assert!(
            rest.iter()
                .all(|position| (0.69..1.02).contains(&position[1]))
        );

        let shift = Vec3::new(0.12, 0.09, -0.08);
        body.apply_procedural(0, shift, &[]);
        let (posed, _) = dress.pose(body.skin().joint_world).unwrap();
        for (&position, &original) in posed.positions.iter().zip(&rest) {
            assert!(
                (Vec3::from_array(position) - Vec3::from_array(original) - shift).length() < 0.0001
            );
        }
        assert!(
            posed
                .normals
                .iter()
                .all(|&normal| (Vec3::from_array(normal).length() - 1.0).abs() < 0.0001)
        );
    }
}
