//! Bounded RGBA8 texture draws on the authenticated UI4 sampled-texture path.
//!
//! The current vGPU contract accepts a Float32x3 position followed by a
//! Float32x2 UV, tightly packed at a 20-byte stride.  It samples one RGBA8
//! texture with nearest, repeating addressing.  This is deliberately a small
//! renderer rather than an OpenGL state implementation: callers translate
//! their accepted GL state before calling it.

use trueos::vgpu::{
    self, Buffer, Device, IndexedDraw, Queue, RenderPipeline, ShaderModule, TimelinePoint,
    Ui4Surface,
};

/// A vertex accepted by the vGPU's `CLIP_POSITION3_UV_TEXTURE` shader.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct TexturedVertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
}

const VERTEX_STRIDE: usize = core::mem::size_of::<TexturedVertex>();
const NEAREST_REPEAT: u32 = vgpu::SAMPLER_ADDRESS_U_REPEAT | vgpu::SAMPLER_ADDRESS_V_REPEAT;

/// Real RGBA8 sampled-texture renderer for one indexed triangle-list draw.
///
/// `draw` starts a new UI4 frame with `clear_rgba8_srgb` and opaque depth.
/// `draw_over` continues an already rendered UI4 frame in painter order. The
/// caller must wait for the prior returned timeline point before resizing,
/// re-uploading, destroying this renderer, or reusing its buffers.
pub struct TexturedRenderer {
    device: Device,
    shader: ShaderModule,
    pipeline: RenderPipeline,
    vertices: Option<(Buffer, usize)>,
    indices: Option<(Buffer, usize)>,
    texture: Option<(Buffer, usize)>,
}

impl TexturedRenderer {
    /// Creates the supported nearest-repeat RGBA8 sampled-texture pipeline.
    pub fn new(device: Device) -> Result<Self, i32> {
        let shader =
            device.create_shader_module(vgpu::SHADER_PACKAGE_CLIP_POSITION3_UV_TEXTURE_FNV1A64)?;
        let pipeline = match device.create_render_pipeline(shader, VERTEX_STRIDE as u32, 0) {
            Ok(pipeline) => pipeline,
            Err(error) => {
                let _ = device.destroy_shader_module(shader);
                return Err(error);
            }
        };
        Ok(Self {
            device,
            shader,
            pipeline,
            vertices: None,
            indices: None,
            texture: None,
        })
    }

    /// Draw one indexed triangle list, clearing the UI4 target first.
    pub fn draw(
        &mut self,
        queue: Queue,
        surface: Ui4Surface,
        vertices: &[TexturedVertex],
        indices: &[u32],
        rgba8: &[u8],
        width: u32,
        height: u32,
        clear_rgba8_srgb: u32,
    ) -> Result<TimelinePoint, i32> {
        self.submit(
            queue,
            surface,
            vertices,
            indices,
            rgba8,
            width,
            height,
            clear_rgba8_srgb,
            false,
        )
    }

    /// Draw one indexed triangle list over the current UI4 frame.
    ///
    /// This uses the vGPU's `LOAD_COLOR` path: it preserves prior colour but
    /// does not supply a shared GL depth buffer. It is therefore suitable for
    /// painter-order opaque UI draws only (there is no alpha blending) after the caller has waited
    /// for the preceding UI4 submission.
    pub fn draw_over(
        &mut self,
        queue: Queue,
        surface: Ui4Surface,
        vertices: &[TexturedVertex],
        indices: &[u32],
        rgba8: &[u8],
        width: u32,
        height: u32,
    ) -> Result<TimelinePoint, i32> {
        self.submit(
            queue, surface, vertices, indices, rgba8, width, height, 0, true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn submit(
        &mut self,
        queue: Queue,
        surface: Ui4Surface,
        vertices: &[TexturedVertex],
        indices: &[u32],
        rgba8: &[u8],
        width: u32,
        height: u32,
        clear_rgba8_srgb: u32,
        load_color: bool,
    ) -> Result<TimelinePoint, i32> {
        let texture_bytes = texture_byte_len(width, height)?;
        if rgba8.len() != texture_bytes
            || indices.len() < 3
            || indices.len() % 3 != 0
            || vertices.is_empty()
            || !vertices.iter().all(vertex_is_finite)
            || indices
                .iter()
                .any(|&index| index as usize >= vertices.len())
        {
            return Err(vgpu::ERR_UNSUPPORTED);
        }
        let vertex_bytes = bytes_of_slice(vertices);
        let index_bytes = bytes_of_slice(indices);
        let vertex_buffer = self.ensure_vertices(vertex_bytes.len())?;
        let index_buffer = self.ensure_indices(index_bytes.len())?;
        let texture_buffer = self.ensure_texture(texture_bytes)?;
        if self.device.write_buffer(vertex_buffer, 0, vertex_bytes)? != vertex_bytes.len()
            || self.device.write_buffer(index_buffer, 0, index_bytes)? != index_bytes.len()
            || self.device.write_buffer(texture_buffer, 0, rgba8)? != rgba8.len()
        {
            return Err(vgpu::ERR_IO);
        }
        self.device.submit_ui4_indexed(
            queue,
            surface,
            self.pipeline,
            vertex_buffer,
            index_buffer,
            IndexedDraw {
                index_count: indices.len() as u32,
                clear_rgba8_srgb,
                topology: vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
                sampled_texture: texture_buffer.raw(),
                texture_width: width,
                texture_height: height,
                texture_pitch: width * 4,
                sampler_flags: NEAREST_REPEAT,
                texture_reserved: if load_color {
                    vgpu::INDEXED_DRAW_LOAD_COLOR
                } else {
                    0
                },
                ..Default::default()
            },
        )
    }

    fn ensure_vertices(&mut self, bytes: usize) -> Result<Buffer, i32> {
        ensure_buffer(
            self.device,
            &mut self.vertices,
            bytes,
            vgpu::BUFFER_USAGE_MAP_WRITE | vgpu::BUFFER_USAGE_VERTEX,
        )
    }

    fn ensure_indices(&mut self, bytes: usize) -> Result<Buffer, i32> {
        ensure_buffer(
            self.device,
            &mut self.indices,
            bytes,
            vgpu::BUFFER_USAGE_MAP_WRITE | vgpu::BUFFER_USAGE_INDEX,
        )
    }

    fn ensure_texture(&mut self, bytes: usize) -> Result<Buffer, i32> {
        ensure_buffer(
            self.device,
            &mut self.texture,
            bytes,
            vgpu::BUFFER_USAGE_MAP_WRITE,
        )
    }

    /// Releases the vGPU resources after the caller has waited for its work.
    pub fn destroy(mut self) -> Result<(), i32> {
        if let Some((buffer, _)) = self.texture.take() {
            self.device.destroy_buffer(buffer)?;
        }
        if let Some((buffer, _)) = self.indices.take() {
            self.device.destroy_buffer(buffer)?;
        }
        if let Some((buffer, _)) = self.vertices.take() {
            self.device.destroy_buffer(buffer)?;
        }
        self.device.destroy_render_pipeline(self.pipeline)?;
        self.device.destroy_shader_module(self.shader)
    }
}

fn ensure_buffer(
    device: Device,
    slot: &mut Option<(Buffer, usize)>,
    bytes: usize,
    usage: u32,
) -> Result<Buffer, i32> {
    if let Some((buffer, capacity)) = *slot
        && capacity >= bytes
    {
        return Ok(buffer);
    }
    if let Some((buffer, _)) = slot.take() {
        device.destroy_buffer(buffer)?;
    }
    let buffer = device.create_buffer(bytes, usage)?;
    *slot = Some((buffer, bytes));
    Ok(buffer)
}

fn texture_byte_len(width: u32, height: u32) -> Result<usize, i32> {
    let pitch = width.checked_mul(4).ok_or(vgpu::ERR_UNSUPPORTED)?;
    if width == 0 || height == 0 {
        return Err(vgpu::ERR_UNSUPPORTED);
    }
    usize::try_from(u64::from(pitch) * u64::from(height)).map_err(|_| vgpu::ERR_UNSUPPORTED)
}

fn vertex_is_finite(vertex: &TexturedVertex) -> bool {
    vertex
        .position
        .iter()
        .chain(vertex.uv.iter())
        .all(|value| value.is_finite())
}

fn bytes_of_slice<T>(values: &[T]) -> &[u8] {
    // `TexturedVertex` and `u32` are plain C-layout scalar values; the vGPU
    // receives their contiguous representation without an allocation.
    unsafe {
        core::slice::from_raw_parts(values.as_ptr().cast::<u8>(), core::mem::size_of_val(values))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_and_uv_have_the_authenticated_20_byte_layout() {
        let vertex = TexturedVertex {
            position: [1.0, -2.0, 3.5],
            uv: [0.25, 0.75],
        };
        let bytes = bytes_of_slice(core::slice::from_ref(&vertex));
        assert_eq!(VERTEX_STRIDE, 20);
        assert_eq!(bytes.len(), 20);
        assert_eq!(&bytes[0..4], &1.0f32.to_le_bytes());
        assert_eq!(&bytes[12..16], &0.25f32.to_le_bytes());
        assert_eq!(&bytes[16..20], &0.75f32.to_le_bytes());
    }

    #[test]
    fn rgba8_texture_bytes_require_nonzero_nonoverflowing_shape() {
        assert_eq!(texture_byte_len(3, 2), Ok(24));
        assert_eq!(texture_byte_len(0, 2), Err(vgpu::ERR_UNSUPPORTED));
        assert_eq!(texture_byte_len(u32::MAX, 1), Err(vgpu::ERR_UNSUPPORTED));
    }
}
