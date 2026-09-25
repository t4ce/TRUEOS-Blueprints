#![no_std]

//! RGB vertex triangle on TRUEOS' authenticated immediate-color vGPU path.
//!
//! The current single indexed-draw shader uses one fixed green fragment color.
//! Subdivide the three vertex colors into a bounded set of flat-color triangles
//! and submit them together through the supported immediate-RGBA batch path.

use trueos::vgpu::{
    self, Buffer, Device, IndexedBatchDrawV2, IndexedDrawBatchV2, Queue, RenderPipeline,
    ShaderModule, TimelinePoint, Ui4Surface,
};

const SUBDIVISIONS: usize = 12;
const TRIANGLE_COUNT: usize = SUBDIVISIONS * SUBDIVISIONS;
const VERTEX_COUNT: usize = TRIANGLE_COUNT * 3;

/// One clip-space position and linear RGBA vertex color.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Vertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

/// Reusable RGB triangle renderer for an acquired UI4 surface.
pub struct TriangleRenderer {
    device: Device,
    pipeline: RenderPipeline,
    shader: ShaderModule,
    vertices: Buffer,
    indices: Buffer,
}

impl TriangleRenderer {
    pub fn new(device: Device) -> Result<Self, i32> {
        let shader = device
            .create_shader_module(vgpu::SHADER_PACKAGE_CLIP_POSITION3_IMMEDIATE_RGBA_FNV1A64)?;
        let pipeline = device.create_render_pipeline(shader, 12, 0)?;
        let vertices = device.create_buffer(
            VERTEX_COUNT * core::mem::size_of::<[f32; 3]>(),
            vgpu::BUFFER_USAGE_MAP_WRITE | vgpu::BUFFER_USAGE_VERTEX,
        )?;
        let indices = device.create_buffer(
            VERTEX_COUNT * core::mem::size_of::<u32>(),
            vgpu::BUFFER_USAGE_MAP_WRITE | vgpu::BUFFER_USAGE_INDEX,
        )?;
        Ok(Self {
            device,
            pipeline,
            shader,
            vertices,
            indices,
        })
    }

    /// Render the three supplied vertex colors as a smooth-looking RGB triangle.
    /// The vGPU's existing batched draw path supplies one color per small triangle.
    pub fn draw(
        &self,
        queue: Queue,
        surface: Ui4Surface,
        vertices: &[Vertex; 3],
        clear_rgba8_srgb: u32,
    ) -> Result<TimelinePoint, i32> {
        let mut positions = [[0f32; 3]; VERTEX_COUNT];
        let mut indices = [0u32; VERTEX_COUNT];
        let mut batch = IndexedDrawBatchV2 {
            clear_rgba8_srgb,
            ..Default::default()
        };
        let mut triangle = 0usize;
        for row in 0..SUBDIVISIONS {
            for column in 0..(SUBDIVISIONS - row) {
                self.add_triangle(
                    vertices,
                    &mut positions,
                    &mut indices,
                    &mut batch,
                    &mut triangle,
                    [(row, column), (row + 1, column), (row, column + 1)],
                );
                if row + column + 1 < SUBDIVISIONS {
                    self.add_triangle(
                        vertices,
                        &mut positions,
                        &mut indices,
                        &mut batch,
                        &mut triangle,
                        [(row + 1, column), (row + 1, column + 1), (row, column + 1)],
                    );
                }
            }
        }
        debug_assert_eq!(triangle, TRIANGLE_COUNT);
        batch.draw_count = triangle as u32;
        let position_bytes = unsafe {
            core::slice::from_raw_parts(
                positions.as_ptr().cast::<u8>(),
                core::mem::size_of_val(&positions),
            )
        };
        let index_bytes = unsafe {
            core::slice::from_raw_parts(
                indices.as_ptr().cast::<u8>(),
                core::mem::size_of_val(&indices),
            )
        };
        if self.device.write_buffer(self.vertices, 0, position_bytes)? != position_bytes.len()
            || self.device.write_buffer(self.indices, 0, index_bytes)? != index_bytes.len()
        {
            return Err(vgpu::ERR_IO);
        }
        self.device.submit_ui4_indexed_batch_v2(
            queue,
            surface,
            self.pipeline,
            self.vertices,
            self.indices,
            batch,
        )
    }

    fn add_triangle(
        &self,
        source: &[Vertex; 3],
        positions: &mut [[f32; 3]; VERTEX_COUNT],
        indices: &mut [u32; VERTEX_COUNT],
        batch: &mut IndexedDrawBatchV2,
        triangle: &mut usize,
        corners: [(usize, usize); 3],
    ) {
        let mut color = [0f32; 4];
        let base = *triangle * 3;
        for (corner, &(row, column)) in corners.iter().enumerate() {
            let weights = [
                (SUBDIVISIONS - row - column) as f32 / SUBDIVISIONS as f32,
                row as f32 / SUBDIVISIONS as f32,
                column as f32 / SUBDIVISIONS as f32,
            ];
            let destination = &mut positions[base + corner];
            for source_vertex in 0..3 {
                for axis in 0..3 {
                    destination[axis] +=
                        source[source_vertex].position[axis] * weights[source_vertex];
                }
                for channel in 0..4 {
                    color[channel] +=
                        source[source_vertex].color[channel] * weights[source_vertex] / 3.0;
                }
            }
            indices[base + corner] = (base + corner) as u32;
        }
        batch.draws[*triangle] = IndexedBatchDrawV2 {
            index_count: 3,
            first_index: base as u32,
            rgba8_srgb: rgba8(color),
            topology: vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
            ..Default::default()
        };
        *triangle += 1;
    }

    pub fn destroy(self) -> Result<(), i32> {
        self.device.destroy_buffer(self.vertices)?;
        self.device.destroy_buffer(self.indices)?;
        self.device.destroy_render_pipeline(self.pipeline)?;
        self.device.destroy_shader_module(self.shader)
    }
}

fn rgba8(color: [f32; 4]) -> u32 {
    u32::from_le_bytes(color.map(|channel| (channel.clamp(0.0, 1.0) * 255.0 + 0.5) as u8))
}
