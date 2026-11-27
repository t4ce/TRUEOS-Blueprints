use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec3};

use crate::mesh::BODY_MATERIAL;

const FINGERS: [&str; 5] = ["Thumb", "Index", "Middle", "Ring", "Little"];
const FINGERTIP_CONTACT: f32 = 0.018;
const TIP_PAD_DEPTH: f32 = 0.002;

pub(super) struct HandProbe {
    vertices: [usize; 2 * FINGERS.len()],
}

impl HandProbe {
    pub(super) fn new(glb: &[u8], body_vertex_offset: usize) -> Result<Self> {
        let asset = gltf::Gltf::from_slice(glb).context("parse hand contact rig")?;
        let blob = asset
            .blob
            .as_deref()
            .context("missing hand contact rig BIN")?;
        let embedded = |buffer: gltf::Buffer<'_>| {
            matches!(buffer.source(), gltf::buffer::Source::Bin).then_some(blob)
        };
        let (node, primitive) = asset
            .nodes()
            .find_map(|node| {
                let primitive = node
                    .mesh()?
                    .primitives()
                    .find(|part| part.material().name() == Some(BODY_MATERIAL))?;
                Some((node, primitive))
            })
            .context("missing skinned body for hand contact")?;
        let skin = node.skin().context("missing hand contact rig skin")?;
        let bind_origins: Vec<_> = skin
            .reader(embedded)
            .read_inverse_bind_matrices()
            .context("missing hand contact inverse bind matrices")?
            .map(|matrix| {
                Mat4::from_cols_array_2d(&matrix)
                    .inverse()
                    .w_axis
                    .truncate()
            })
            .collect();
        let reader = primitive.reader(embedded);
        let positions: Vec<_> = reader
            .read_positions()
            .context("missing body positions")?
            .map(Vec3::from_array)
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
            positions.len() == joints.len()
                && positions.len() == weights.len()
                && bind_origins.len() == skin.joints().len(),
            "inconsistent hand contact attributes"
        );
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

        let mut vertices = [0; 2 * FINGERS.len()];
        for (hand, side) in vertices.chunks_exact_mut(FINGERS.len()).zip(['L', 'R']) {
            for (point, finger) in hand.iter_mut().zip(FINGERS) {
                let joint = |segment| {
                    let name = format!("J_Bip_{side}_{finger}{segment}");
                    skin.joints()
                        .position(|joint| joint.name() == Some(name.as_str()))
                        .with_context(|| format!("missing joint {name}"))
                };
                let distal = joint(3)?;
                let origin = bind_origins[distal];
                let direction = (origin - bind_origins[joint(2)?])
                    .try_normalize()
                    .context("invalid fingertip bind direction")?;
                let candidates = positions
                    .iter()
                    .zip(&joints)
                    .zip(&weights)
                    .zip(&visible)
                    .enumerate()
                    .filter_map(|(index, (((&position, joints), weights), &drawn))| {
                        let weight: f32 = joints
                            .iter()
                            .zip(weights)
                            .filter_map(|(&joint, &weight)| {
                                (usize::from(joint) == distal).then_some(weight)
                            })
                            .sum();
                        (drawn && weight >= 0.7).then_some((
                            index,
                            position.y,
                            (position - origin).dot(direction),
                        ))
                    });
                let tip = candidates
                    .clone()
                    .map(|(_, _, distance)| distance)
                    .max_by(f32::total_cmp)
                    .context("missing fingertip vertices")?;
                // The palms face down in the bind pose. Use the skin pad just
                // behind the tip rather than the nail or the distal joint origin.
                let (index, _, _) = candidates
                    .filter(|&(_, _, distance)| tip - distance <= TIP_PAD_DEPTH)
                    .min_by(|(_, a, _), (_, b, _)| a.total_cmp(b))
                    .context("missing fingertip pad")?;
                *point = body_vertex_offset
                    .checked_add(index)
                    .context("hand contact offset overflow")?;
            }
        }
        Ok(Self { vertices })
    }

    pub(super) fn sample(&self, positions: &[[f32; 3]]) -> [(Vec3, f32); 2 * FINGERS.len()] {
        self.vertices
            .map(|index| (Vec3::from_array(positions[index]), FINGERTIP_CONTACT))
    }

    pub(super) fn right_index_vertex(&self) -> usize {
        self.vertices[FINGERS.len() + 1]
    }
}
