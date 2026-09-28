//! Frame-sized submission of CPU-prepared Warcraft triangles.
extern crate alloc;
use super::textured::{bytes_of_slice, ensure_buffer};
use alloc::{sync::Arc, vec::Vec};
use trueos::vgpu::{
    self, Buffer, Device, Queue, RenderPipeline, ShaderModule, TimelinePoint, Ui4Surface,
};

pub struct Draw {
    pub vertices: Vec<[f32; 16]>,
    pub state: [f32; 384],
    pub pixels: Arc<[u8]>,
    pub texture_size: [u32; 2],
    pub flags: u32,
    pub clear_rgba8: u32,
}

struct Texture {
    buffer: Buffer,
    pixels: Arc<[u8]>,
    size: [u32; 2],
    used: bool,
}

/// Buffers and textures persist between completed submissions. No framebuffer
/// bytes cross this interface: UI4 supplies the render target.
pub struct Renderer {
    device: Device,
    shader: ShaderModule,
    pipeline: RenderPipeline,
    vertices: Option<(Buffer, usize)>,
    indices: Option<(Buffer, usize)>,
    textures: Vec<Texture>,
    vertex_bytes: Vec<u8>,
    index_words: Vec<u32>,
    pub last_upload_bytes: usize,
    pub last_texture_upload_bytes: usize,
}
impl Renderer {
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
            textures: Vec::new(),
            vertex_bytes: Vec::new(),
            index_words: Vec::new(),
            last_upload_bytes: 0,
            last_texture_upload_bytes: 0,
        })
    }

    fn texture(&mut self, draw: &Draw) -> Result<Buffer, i32> {
        if let Some(t) = self.textures.iter_mut().find(|t| {
            t.size == draw.texture_size
                && (Arc::ptr_eq(&t.pixels, &draw.pixels) || t.pixels == draw.pixels)
        }) {
            t.used = true;
            return Ok(t.buffer);
        }
        let bytes = draw.pixels.len();
        // Evict only textures unused by this entire batch; never invalidate a
        // descriptor already assembled for a later draw in this submission.
        while self.textures.len() >= 64
            || self.textures.iter().map(|t| t.pixels.len()).sum::<usize>() + bytes
                > 16 * 1024 * 1024
        {
            let index = self
                .textures
                .iter()
                .position(|t| !t.used)
                .ok_or(vgpu::ERR_UNSUPPORTED)?;
            self.device.destroy_buffer(self.textures[index].buffer)?;
            self.textures.remove(index);
        }
        let buffer = self
            .device
            .create_buffer(bytes, vgpu::BUFFER_USAGE_MAP_WRITE)?;
        match self.device.write_buffer(buffer, 0, &draw.pixels) {
            Ok(n) if n == bytes => {}
            result => {
                let _ = self.device.destroy_buffer(buffer);
                return Err(result.err().unwrap_or(vgpu::ERR_IO));
            }
        }
        self.last_texture_upload_bytes += bytes;
        self.textures.push(Texture {
            buffer,
            pixels: draw.pixels.clone(),
            size: draw.texture_size,
            used: true,
        });
        Ok(buffer)
    }

    pub fn submit(
        &mut self,
        queue: Queue,
        surface: Ui4Surface,
        draws: &[Draw],
    ) -> Result<TimelinePoint, i32> {
        if draws.is_empty() || draws.len() > vgpu::MAX_PREPARED_RASTER_SUBMIT_DRAWS {
            return Err(vgpu::ERR_UNSUPPORTED);
        }
        self.last_upload_bytes = 0;
        self.last_texture_upload_bytes = 0;
        self.vertex_bytes.clear();
        self.index_words.clear();
        for t in &mut self.textures {
            t.used = draws
                .iter()
                .any(|d| d.texture_size == t.size && Arc::ptr_eq(&d.pixels, &t.pixels));
        }
        let mut batch = vgpu::PreparedRasterBatchV1::default();
        batch.draw_count = draws.len() as u32;
        for (index, draw) in draws.iter().enumerate() {
            if draw.vertices.is_empty()
                || draw.vertices.len() % 3 != 0
                || draw
                    .vertices
                    .iter()
                    .flatten()
                    .chain(draw.state.iter())
                    .any(|v| !v.is_finite())
                || !vgpu::indexed_draw_flags_valid(draw.flags)
                || draw.texture_size.contains(&0)
                || (draw.texture_size[0] as usize)
                    .checked_mul(draw.texture_size[1] as usize)
                    .and_then(|n| n.checked_mul(4))
                    != Some(draw.pixels.len())
            {
                return Err(vgpu::ERR_UNSUPPORTED);
            }
            let texture = self.texture(draw)?;
            let state_offset = self.vertex_bytes.len() as u64;
            self.vertex_bytes
                .extend_from_slice(bytes_of_slice(&draw.state));
            let vertex_offset = self.vertex_bytes.len() as u64;
            self.vertex_bytes
                .extend_from_slice(bytes_of_slice(&draw.vertices));
            let index_offset = (self.index_words.len() * 4) as u64;
            self.index_words.extend(0..draw.vertices.len() as u32);
            batch.draws[index] = vgpu::PreparedRasterDrawV1 {
                vertex_offset,
                state_offset,
                index_offset,
                index_count: draw.vertices.len() as u32,
                texture: texture.raw(),
                texture_width: draw.texture_size[0],
                texture_height: draw.texture_size[1],
                texture_pitch: draw.texture_size[0] * 4,
                sampler_flags: vgpu::SAMPLER_ADDRESS_U_REPEAT | vgpu::SAMPLER_ADDRESS_V_REPEAT,
                flags: draw.flags,
                clear_rgba8: draw.clear_rgba8,
                reserved: 0,
            };
        }
        let vb = ensure_buffer(
            self.device,
            &mut self.vertices,
            self.vertex_bytes.len(),
            vgpu::BUFFER_USAGE_MAP_WRITE | vgpu::BUFFER_USAGE_VERTEX,
        )?;
        let ib = ensure_buffer(
            self.device,
            &mut self.indices,
            self.index_words.len() * 4,
            vgpu::BUFFER_USAGE_MAP_WRITE | vgpu::BUFFER_USAGE_INDEX,
        )?;
        if self.device.write_buffer(vb, 0, &self.vertex_bytes)? != self.vertex_bytes.len()
            || self
                .device
                .write_buffer(ib, 0, bytes_of_slice(&self.index_words))?
                != self.index_words.len() * 4
        {
            return Err(vgpu::ERR_IO);
        }
        self.last_upload_bytes = self.vertex_bytes.len() + self.index_words.len() * 4;
        self.device.submit_ui4_prepared_raster_batch_v1(
            queue,
            surface,
            self.pipeline,
            vb,
            ib,
            batch,
        )
    }
}
impl Drop for Renderer {
    fn drop(&mut self) {
        for t in self.textures.drain(..) {
            let _ = self.device.destroy_buffer(t.buffer);
        }
        for (b, _) in [self.vertices.take(), self.indices.take()]
            .into_iter()
            .flatten()
        {
            let _ = self.device.destroy_buffer(b);
        }
        let _ = self.device.destroy_render_pipeline(self.pipeline);
        let _ = self.device.destroy_shader_module(self.shader);
    }
}
