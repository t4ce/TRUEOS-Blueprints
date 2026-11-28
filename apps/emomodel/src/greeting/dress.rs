use anyhow::{Context, Result, ensure};
use glam::Mat4;

use crate::animation::{Animation, PoseSample};

const GLB: &[u8] = include_bytes!("../../assets/AvatarSample_B-pink-skirt.glb");
const MATERIAL: &str = "N00_001_03_Bottoms_01_CLOTH (Instance)";
pub(super) const PINK: [u8; 3] = [226, 172, 187];

pub(super) struct Dress {
    rig: Animation,
    indices: Vec<u32>,
}

impl Dress {
    pub(super) fn load() -> Result<Self> {
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
        let rig = Animation::procedural(GLB, &[MATERIAL])?;
        let vertices = rig.bind_positions(0).len();
        ensure!(
            primitive.mode() == gltf::mesh::Mode::Triangles
                && !indices.is_empty()
                && indices.len().is_multiple_of(3)
                && indices.iter().all(|&index| (index as usize) < vertices),
            "invalid skirt topology"
        );
        if primitive.material().double_sided() {
            let front_count = indices.len();
            indices.reserve(front_count);
            for triangle in (0..front_count).step_by(3) {
                let [a, b, c] = indices[triangle..][..3].try_into().unwrap();
                indices.extend([a, c, b]);
            }
        }
        Ok(Self { rig, indices })
    }

    pub(super) fn vertex_count(&self) -> usize {
        self.rig.bind_positions(0).len()
    }

    pub(super) fn indices(&self) -> &[u32] {
        &self.indices
    }

    pub(super) fn pose(&mut self, joints: &[Mat4]) -> Result<PoseSample<'_>> {
        self.rig.skin_from_world(joints)
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

        let mut dress = Dress::load().unwrap();
        let mut body = Animation::procedural(HUMANOID_GLB, &[BODY_MATERIAL]).unwrap();
        body.apply_procedural(0, Vec3::ZERO, &[]);
        let rest = dress
            .pose(body.skin().joint_world)
            .unwrap()
            .positions
            .to_vec();
        assert_eq!(rest.len(), dress.vertex_count());
        assert!(dress.indices().len() >= 1_500);
        assert!(
            rest.iter()
                .all(|position| (0.69..1.02).contains(&position[1]))
        );

        let shift = Vec3::new(0.12, 0.09, -0.08);
        body.apply_procedural(0, shift, &[]);
        let posed = dress.pose(body.skin().joint_world).unwrap();
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
