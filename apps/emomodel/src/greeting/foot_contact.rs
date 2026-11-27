use anyhow::{Context, Result, ensure};
use glam::Vec3;

use crate::mesh::BODY_MATERIAL;

// The settled Greeting pose holds the raised-heel foot's toe about 13 mm
// above the floor. A separate toe allowance keeps that contact visible.
const TOE_CONTACT: f32 = 0.022;
const HEEL_CONTACT: f32 = 0.018;

pub(super) struct FootProbe {
    vertices: [[usize; 2]; 2],
}

#[derive(Clone, Copy)]
pub(super) struct FootContact {
    heel: Vec3,
    toe: Vec3,
}

impl FootProbe {
    pub(super) fn new(glb: &[u8], body_vertex_offset: usize) -> Result<Self> {
        let asset = gltf::Gltf::from_slice(glb).context("parse foot contact rig")?;
        let blob = asset
            .blob
            .as_deref()
            .context("missing foot contact rig BIN")?;
        let (node, primitive) = asset
            .nodes()
            .find_map(|node| {
                let primitive = node
                    .mesh()?
                    .primitives()
                    .find(|part| part.material().name() == Some(BODY_MATERIAL))?;
                Some((node, primitive))
            })
            .context("missing skinned body for foot contact")?;
        let skin = node.skin().context("missing foot contact rig skin")?;
        let reader = primitive
            .reader(|buffer| matches!(buffer.source(), gltf::buffer::Source::Bin).then_some(blob));
        let positions: Vec<_> = reader
            .read_positions()
            .context("missing body positions")?
            .collect();
        let joints: Vec<_> = reader
            .read_joints(0)
            .context("missing body joints")?
            .into_u16()
            .collect();
        let weights: Vec<_> = reader
            .read_weights(0)
            .context("missing body weights")?
            .into_f32()
            .collect();
        ensure!(
            positions.len() == joints.len() && positions.len() == weights.len(),
            "inconsistent foot contact attributes"
        );
        // The body mesh also contains a separately indexed shoe section. Use
        // only vertices actually drawn by the bare skin primitive.
        let mut visible = vec![false; positions.len()];
        for index in reader
            .read_indices()
            .context("missing body indices")?
            .into_u32()
        {
            *visible
                .get_mut(index as usize)
                .context("invalid body index")? = true;
        }
        let mut vertices = [[0; 2]; 2];
        for (foot, (heel_name, toe_name)) in vertices.iter_mut().zip([
            ("J_Bip_L_Foot", "J_Bip_L_ToeBase"),
            ("J_Bip_R_Foot", "J_Bip_R_ToeBase"),
        ]) {
            for (point, (joint_name, towards)) in
                foot.iter_mut().zip([(heel_name, 1.0), (toe_name, -1.0)])
            {
                let joint = skin
                    .joints()
                    .position(|joint| joint.name() == Some(joint_name))
                    .with_context(|| format!("missing joint {joint_name}"))?;
                let candidates = positions
                    .iter()
                    .zip(&joints)
                    .zip(&weights)
                    .zip(&visible)
                    .enumerate()
                    .filter_map(|(i, (((&position, joints), weights), &drawn))| {
                        let weight: f32 = joints
                            .iter()
                            .zip(weights)
                            .filter_map(|(&index, &weight)| {
                                (usize::from(index) == joint).then_some(weight)
                            })
                            .sum();
                        (drawn && weight >= 0.7).then_some((i, position))
                    });
                // +Z points toward the heel in the authored bind pose. Pick a
                // low sole vertex near each end, away from the ankle and arch.
                let end = candidates
                    .clone()
                    .map(|(_, p)| p[2] * towards)
                    .max_by(f32::total_cmp)
                    .context("missing foot sole")?;
                let (index, _) = candidates
                    .filter(|(_, p)| end - p[2] * towards <= 0.046)
                    .min_by(|(_, a), (_, b)| a[1].total_cmp(&b[1]))
                    .context("missing foot contact vertex")?;
                *point = body_vertex_offset
                    .checked_add(index)
                    .context("foot contact offset overflow")?;
            }
        }
        Ok(Self { vertices })
    }

    pub(super) fn sample(&self, positions: &[[f32; 3]]) -> [FootContact; 2] {
        self.vertices.map(|[heel, toe]| FootContact {
            heel: Vec3::from_array(positions[heel]),
            toe: Vec3::from_array(positions[toe]),
        })
    }
}

impl FootContact {
    pub(super) fn points(&self) -> [(Vec3, f32); 2] {
        [(self.heel, HEEL_CONTACT), (self.toe, TOE_CONTACT)]
    }
}
