use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec3};
use trueos::vgpu::{self, Device, RetainedDrawRange, RetainedMesh, RetainedMeshDescriptor};

use super::{DRAW_GROUPS, HAIR_UV, Staging, VERTEX_BYTES, gpu_error, write_geometry};
use crate::{
    hair::{Hair, PARTICLES},
    hair_body::BodyBack,
    hair_ribbon,
    idle_wind::{IDLE_FRAMES, acceleration},
};

pub(super) struct IdleBase {
    pub(super) vertices: Vec<u8>,
    pub(super) indices: Vec<u8>,
    pub(super) ranges: [RetainedDrawRange; DRAW_GROUPS],
}

pub(super) struct IdleHair<'a> {
    pub(super) hair: &'a mut Hair,
    pub(super) head: Mat4,
    pub(super) body: &'a BodyBack,
    pub(super) view_back: Vec3,
}

pub(super) struct Idle {
    meshes: Vec<RetainedMesh>,
    frames_per_mesh: usize,
    base_ranges: [RetainedDrawRange; DRAW_GROUPS],
    hair_ranges: Vec<RetainedDrawRange>,
    geometry_bytes: usize,
}

impl Idle {
    pub(super) fn bake(
        device: Device,
        staging: Staging,
        base: IdleBase,
        state: IdleHair<'_>,
        mut visit: impl FnMut(Vec3),
    ) -> Result<Self> {
        let IdleHair {
            hair,
            head,
            body,
            view_back,
        } = state;
        let strand_vertices = (PARTICLES * 2) as u32;
        let strand_count = u32::try_from(hair.strands().len())?;
        ensure!(strand_count > 0, "missing idle hair strands");
        let hair_indices: Vec<_> = (0..strand_count)
            .flat_map(|strand| {
                hair_ribbon::TRIANGLES
                    .into_iter()
                    .flatten()
                    .map(move |index| strand * strand_vertices + index)
            })
            .collect();
        let frame_vertex_bytes = (strand_count * strand_vertices) as usize * VERTEX_BYTES;
        let frame_index_bytes = hair_indices.len() * size_of::<u32>();
        let free_vertex_bytes = staging
            .vertex_capacity
            .checked_sub(base.vertices.len())
            .context("idle avatar vertices exceed staging capacity")?;
        let free_index_bytes = staging
            .index_capacity
            .checked_sub(base.indices.len())
            .context("idle avatar indices exceed staging capacity")?;
        let frame_limit =
            (free_vertex_bytes / frame_vertex_bytes).min(free_index_bytes / frame_index_bytes);
        ensure!(frame_limit > 0, "idle hair frame exceeds staging capacity");

        // Store the static avatar and floor once in each staging-sized batch.
        let mesh_count = IDLE_FRAMES.div_ceil(frame_limit);
        let frames_per_mesh = IDLE_FRAMES.div_ceil(mesh_count);
        let mut vertices =
            Vec::with_capacity(base.vertices.len() + frame_vertex_bytes * frames_per_mesh);
        let mut indices =
            Vec::with_capacity(base.indices.len() + frame_index_bytes * frames_per_mesh);
        let mut meshes = Vec::with_capacity(mesh_count);
        let mut hair_ranges = Vec::with_capacity(IDLE_FRAMES);
        let mut geometry_bytes = 0;
        let index_count = u32::try_from(hair_indices.len())?;
        let normal = view_back.to_array();

        for start in (0..IDLE_FRAMES).step_by(frames_per_mesh) {
            let end = (start + frames_per_mesh).min(IDLE_FRAMES);
            vertices.clear();
            vertices.extend_from_slice(&base.vertices);
            indices.clear();
            indices.extend_from_slice(&base.indices);

            for frame in start..end {
                // The visible handoff from the gesture keeps its exact last hair pose.
                if frame != 0 {
                    hair.step(head, acceleration(frame), body);
                }
                let first_index = u32::try_from(indices.len() / size_of::<u32>())?;
                let base_vertex = u32::try_from(vertices.len() / VERTEX_BYTES)?;
                for points in hair.strands() {
                    for point in hair_ribbon::vertices(points, view_back)
                        .into_iter()
                        .flatten()
                    {
                        visit(point);
                        for component in point.to_array().into_iter().chain(normal).chain(HAIR_UV) {
                            vertices.extend_from_slice(&component.to_le_bytes());
                        }
                    }
                }
                for &index in &hair_indices {
                    indices.extend_from_slice(&(base_vertex + index).to_le_bytes());
                }
                hair_ranges.push(RetainedDrawRange {
                    first_index,
                    index_count,
                });
            }

            write_geometry(device, staging.vertices, &vertices)
                .context("upload idle wind vertices")?;
            write_geometry(device, staging.indices, &indices)
                .context("upload idle wind indices")?;
            meshes.push(
                device
                    .create_retained_mesh(
                        staging.vertices,
                        staging.indices,
                        RetainedMeshDescriptor {
                            vertex_count: u32::try_from(vertices.len() / VERTEX_BYTES)?,
                            index_count: u32::try_from(indices.len() / size_of::<u32>())?,
                            vertex_layout: vgpu::RETAINED_VERTEX_LAYOUT_POS_NORMAL_UV,
                            topology: vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
                            ..Default::default()
                        },
                    )
                    .map_err(gpu_error)
                    .with_context(|| {
                        format!("retain idle wind frames {start} through {}", end - 1)
                    })?,
            );
            geometry_bytes += vertices.len() + indices.len();
        }

        Ok(Self {
            meshes,
            frames_per_mesh,
            base_ranges: base.ranges,
            hair_ranges,
            geometry_bytes,
        })
    }

    pub(super) fn draw(
        &self,
        frame: usize,
    ) -> (
        RetainedMesh,
        &[RetainedDrawRange; DRAW_GROUPS],
        RetainedDrawRange,
    ) {
        (
            self.meshes[frame / self.frames_per_mesh],
            &self.base_ranges,
            self.hair_ranges[frame],
        )
    }

    pub(super) fn geometry_bytes(&self) -> usize {
        self.geometry_bytes
    }

    pub(super) fn mesh_count(&self) -> usize {
        self.meshes.len()
    }
}
