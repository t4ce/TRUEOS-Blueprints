//! Fixed GL state and guest arrays uploaded for native GPU VS/PS execution.
extern crate alloc;
use alloc::vec::Vec;

use super::textured::{bytes_of_slice, ensure_buffer};
use trueos::vgpu::{
    self, Buffer, Device, Queue, RenderPipeline, ShaderModule, TimelinePoint, Ui4Surface,
};
// Bound both buffer handles and staging bytes. The broker also keeps a resident
// GPU copy; leave quota headroom for that copy, alignment, geometry and depth.
const TEXTURE_CACHE_BYTES: usize = 16 * 1024 * 1024;
const TEXTURE_CACHE_ENTRIES: usize = 64;
struct CachedTexture {
    buffer: Buffer,
    shape: [u32; 2],
    pixels: Vec<u8>,
}

#[derive(Default)]
pub struct TextureUploads {
    pub hits: u64,
    pub uploads: u64,
    pub bytes: u64,
}

pub struct FixedRenderer {
    device: Device,
    shader: ShaderModule,
    pipeline: RenderPipeline,
    vertices: Option<(Buffer, usize)>,
    indices: Option<(Buffer, usize)>,
    texture: Option<(Buffer, usize)>,
    textures: Vec<CachedTexture>,
    texture_bytes: usize,
    uploads: TextureUploads,
}
impl FixedRenderer {
    pub fn new(device: Device) -> Result<Self, i32> {
        let shader = device.create_shader_module(vgpu::SHADER_PACKAGE_WC3_FIXED_FNV1A64)?;
        let pipeline = match device.create_render_pipeline(shader, 64, 0) {
            Ok(p) => p,
            Err(e) => {
                let _ = device.destroy_shader_module(shader);
                return Err(e);
            }
        };
        Ok(Self {
            device,
            shader,
            pipeline,
            vertices: None,
            indices: None,
            texture: None,
            textures: Vec::new(),
            texture_bytes: 0,
            uploads: TextureUploads::default(),
        })
    }
    pub fn take_texture_uploads(&mut self) -> TextureUploads {
        core::mem::take(&mut self.uploads)
    }

    // Draw submission is synchronous; callers retire the preceding draw before
    // reusing this renderer. Never rewrite a cached buffer: that would invalidate
    // the broker's resident sampled texture even when the bytes are unchanged.
    fn sampled_texture(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<Buffer, i32> {
        let shape = [width, height];
        if let Some(index) = self.textures.iter().position(|entry|
            entry.shape == shape && entry.pixels == rgba)
        {
            self.uploads.hits += 1;
            let entry = self.textures.remove(index);
            let buffer = entry.buffer;
            self.textures.push(entry);
            return Ok(buffer);
        }
        self.uploads.uploads += 1;
        self.uploads.bytes += rgba.len() as u64;
        if rgba.len() > TEXTURE_CACHE_BYTES {
            let buffer = ensure_buffer(self.device, &mut self.texture, rgba.len(),
                vgpu::BUFFER_USAGE_MAP_WRITE)?;
            if self.device.write_buffer(buffer, 0, rgba)? != rgba.len() {
                return Err(vgpu::ERR_IO);
            }
            return Ok(buffer);
        }
        while self.textures.len() >= TEXTURE_CACHE_ENTRIES
            || self.texture_bytes + rgba.len() > TEXTURE_CACHE_BYTES
        {
            // Keep the entry owned if destruction fails.
            self.device.destroy_buffer(self.textures[0].buffer)?;
            let entry = self.textures.remove(0);
            self.texture_bytes -= entry.pixels.len();
        }
        let buffer = self.device.create_buffer(rgba.len(), vgpu::BUFFER_USAGE_MAP_WRITE)?;
        match self.device.write_buffer(buffer, 0, rgba) {
            Ok(n) if n == rgba.len() => {},
            result => {
                let _ = self.device.destroy_buffer(buffer);
                return Err(result.err().unwrap_or(vgpu::ERR_IO));
            }
        }
        self.texture_bytes += rgba.len();
        self.textures.push(CachedTexture { buffer, shape, pixels: rgba.to_vec() });
        Ok(buffer)
    }

    pub fn draw(
        &mut self,
        queue: Queue,
        surface: Ui4Surface,
        vertices: &[[f32; 16]],
        indices: &[u32],
        state: &[f32; 384],
        rgba: &[u8],
        width: u32,
        height: u32,
        flags: u32,
    ) -> Result<TimelinePoint, i32> {
        if !vgpu::indexed_draw_flags_valid(flags)
            || vertices.is_empty()
            || indices.len() % 3 != 0
            || indices.is_empty()
            || indices.iter().any(|i| *i as usize >= vertices.len())
            || vertices
                .iter()
                .flatten()
                .chain(state.iter())
                .any(|v| !v.is_finite())
            || width == 0
            || height == 0
            || width.checked_mul(4).is_none()
            || (width as usize)
                .checked_mul(height as usize)
                .and_then(|n| n.checked_mul(4))
                != Some(rgba.len())
        {
            return Err(vgpu::ERR_UNSUPPORTED);
        }
        let vb = bytes_of_slice(vertices);
        let ib = bytes_of_slice(indices);
        let vertex = ensure_buffer(
            self.device,
            &mut self.vertices,
            vb.len() + vgpu::WC3_FIXED_STATE_BYTES,
            vgpu::BUFFER_USAGE_MAP_WRITE | vgpu::BUFFER_USAGE_VERTEX,
        )?;
        let index = ensure_buffer(
            self.device,
            &mut self.indices,
            ib.len(),
            vgpu::BUFFER_USAGE_MAP_WRITE | vgpu::BUFFER_USAGE_INDEX,
        )?;
        let texture = self.sampled_texture(rgba, width, height)?;
        for (buffer, offset, data) in [
            (vertex, 0, bytes_of_slice(state)),
            (vertex, vgpu::WC3_FIXED_STATE_BYTES, vb),
            (index, 0, ib),
        ] {
            if self.device.write_buffer(buffer, offset, data)? != data.len() {
                return Err(vgpu::ERR_IO);
            }
        }
        self.device.submit_ui4_indexed(
            queue,
            surface,
            self.pipeline,
            vertex,
            index,
            vgpu::IndexedDraw {
                vertex_offset: vgpu::WC3_FIXED_STATE_BYTES as u64,
                index_count: indices.len() as u32,
                topology: vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
                sampled_texture: texture.raw(),
                texture_width: width,
                texture_height: height,
                texture_pitch: width * 4,
                sampler_flags: vgpu::SAMPLER_ADDRESS_U_REPEAT | vgpu::SAMPLER_ADDRESS_V_REPEAT,
                texture_reserved: flags,
                ..Default::default()
            },
        )
    }
    pub fn destroy(mut self) -> Result<(), i32> {
        for entry in self.textures.drain(..) {
            self.device.destroy_buffer(entry.buffer)?;
        }
        for buffer in [
            self.vertices.take(),
            self.indices.take(),
            self.texture.take(),
        ]
        .into_iter()
        .flatten()
        {
            self.device.destroy_buffer(buffer.0)?;
        }
        self.device.destroy_render_pipeline(self.pipeline)?;
        self.device.destroy_shader_module(self.shader)
    }
}
