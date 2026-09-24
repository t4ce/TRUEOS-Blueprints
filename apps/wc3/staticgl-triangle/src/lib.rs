#![no_std]

//! Small reusable vGPU path for the static OpenGL triangle experiment.
//!
//! The helper owns only its pipeline and buffers. Callers own the device,
//! queue, UI4 frame lifecycle, and acquired surface.

use trueos::vgpu::{
    self, Buffer, Device, IndexedDraw, Queue, RenderPipeline, ShaderModule, TimelinePoint,
    Ui4Surface,
};

/// One clip-space position and linear RGBA vertex color.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Vertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

/// Reusable indexed triangle renderer for an imported UI4 surface.
pub struct TriangleRenderer {
    device: Device,
    pipeline: RenderPipeline,
    shader: ShaderModule,
    vertices: Buffer,
    indices: Buffer,
}

impl TriangleRenderer {
    /// Create the authenticated position-plus-RGBA pipeline and static index buffer.
    pub fn new(device: Device) -> Result<Self, i32> {
        let shader =
            device.create_shader_module(vgpu::SHADER_PACKAGE_CLIP_POSITION3_RGBA_FNV1A64)?;
        let pipeline = device.create_render_pipeline(shader, 28, 0)?;
        let vertices = device.create_buffer(
            3 * core::mem::size_of::<Vertex>(),
            vgpu::BUFFER_USAGE_MAP_WRITE | vgpu::BUFFER_USAGE_VERTEX,
        )?;
        let indices = device.create_buffer(
            3 * core::mem::size_of::<u32>(),
            vgpu::BUFFER_USAGE_MAP_WRITE | vgpu::BUFFER_USAGE_INDEX,
        )?;
        let indices_data = [0u32, 1, 2];
        let index_bytes = unsafe {
            core::slice::from_raw_parts(
                indices_data.as_ptr().cast::<u8>(),
                core::mem::size_of_val(&indices_data),
            )
        };
        if device.write_buffer(indices, 0, &index_bytes)? != index_bytes.len() {
            return Err(vgpu::ERR_IO);
        }
        Ok(Self {
            device,
            pipeline,
            shader,
            vertices,
            indices,
        })
    }

    /// Upload and render three per-vertex colors into the caller's UI4 surface.
    /// The surface is consumed by submission; the caller waits on the returned
    /// timeline point and publishes its frame after the GPU completes.
    pub fn draw(
        &self,
        queue: Queue,
        surface: Ui4Surface,
        vertices: &[Vertex; 3],
        clear_rgba8_srgb: u32,
    ) -> Result<TimelinePoint, i32> {
        // Vertex is repr(C), composed only of f32 arrays, with no padding
        // between its fields on the supported TRUEOS targets.
        let bytes = unsafe {
            core::slice::from_raw_parts(
                vertices.as_ptr().cast::<u8>(),
                core::mem::size_of_val(vertices),
            )
        };
        if self.device.write_buffer(self.vertices, 0, bytes)? != bytes.len() {
            return Err(vgpu::ERR_IO);
        }
        self.device.submit_ui4_indexed(
            queue,
            surface,
            self.pipeline,
            self.vertices,
            self.indices,
            IndexedDraw {
                index_count: 3,
                clear_rgba8_srgb,
                topology: vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
                ..Default::default()
            },
        )
    }

    /// Destroy resources allocated by this helper. Call after submitted work retires.
    pub fn destroy(self) -> Result<(), i32> {
        self.device.destroy_buffer(self.vertices)?;
        self.device.destroy_buffer(self.indices)?;
        self.device.destroy_render_pipeline(self.pipeline)?;
        self.device.destroy_shader_module(self.shader)
    }
}
