use anyhow::{Context, Result, ensure};
use trueos::vgpu::{
    self, Buffer, Device, IndexedDraw, Queue, QueueClass, RenderPipeline, Ui4Surface,
};

use crate::preview::gpu_error;

pub(super) const VERTEX_BYTES: usize = 5 * size_of::<f32>();

pub(super) struct ReelDraw {
    device: Device,
    queue: Queue,
    pipeline: RenderPipeline,
    vertices: Buffer,
    indices: Buffer,
    texture: Buffer,
    vertex_capacity: usize,
    index_capacity: usize,
    texture_capacity: usize,
    required_vertex_bytes: usize,
    draw: IndexedDraw,
}

impl ReelDraw {
    pub(super) fn new(
        device: Device,
        vertex_capacity: usize,
        index_capacity: usize,
        texture_capacity: usize,
    ) -> Result<Self> {
        ensure!(
            vertex_capacity >= VERTEX_BYTES
                && vertex_capacity.is_multiple_of(VERTEX_BYTES)
                && index_capacity >= 3 * size_of::<u32>()
                && index_capacity.is_multiple_of(size_of::<u32>())
                && texture_capacity >= 4
                && texture_capacity.is_multiple_of(4),
            "invalid reel staging capacity"
        );
        let queue = device.create_queue(QueueClass::Render).map_err(gpu_error)?;
        let shader = device
            .create_shader_module(vgpu::SHADER_PACKAGE_CLIP_POSITION3_UV_TEXTURE_FNV1A64)
            .map_err(gpu_error)
            .context("select the native textured indexed shader")?;
        let pipeline = device.create_render_pipeline(shader, VERTEX_BYTES as u32, 0);
        // The pipeline keeps its admitted shader digest. The module handle is
        // only needed to create it.
        let released = device.destroy_shader_module(shader).map_err(gpu_error);
        let pipeline = pipeline
            .map_err(gpu_error)
            .context("create reel pipeline")?;
        released?;
        let staging = |capacity, usage| {
            device
                .create_buffer(capacity, usage | vgpu::BUFFER_USAGE_MAP_WRITE)
                .map_err(gpu_error)
        };
        let vertices = staging(vertex_capacity, vgpu::BUFFER_USAGE_VERTEX)
            .context("allocate reel vertex staging")?;
        let indices = staging(index_capacity, vgpu::BUFFER_USAGE_INDEX)
            .context("allocate reel index staging")?;
        let texture = staging(texture_capacity, 0).context("allocate reel texture staging")?;
        let draw = IndexedDraw {
            topology: vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
            sampled_texture: texture.raw(),
            sampler_flags: vgpu::SAMPLER_ADDRESS_U_REPEAT | vgpu::SAMPLER_ADDRESS_V_REPEAT,
            // The native default clears color and depth and uses an opaque
            // depth-tested draw. Both the contact patches and the avatar use it.
            ..Default::default()
        };
        Ok(Self {
            device,
            queue,
            pipeline,
            vertices,
            indices,
            texture,
            vertex_capacity,
            index_capacity,
            texture_capacity,
            required_vertex_bytes: 0,
            draw,
        })
    }

    pub(super) fn set_indices(&mut self, indices: &[u8]) -> std::result::Result<(), i32> {
        let count = indices.len() / size_of::<u32>();
        if count == 0 || !indices.len().is_multiple_of(3 * size_of::<u32>()) {
            return Err(vgpu::ERR_UNSUPPORTED);
        }
        let count = u32::try_from(count).map_err(|_| vgpu::ERR_UNSUPPORTED)?;
        let (words, _) = indices.as_chunks::<{ size_of::<u32>() }>();
        let required_vertices =
            words.iter().copied().map(u32::from_le_bytes).max().unwrap() as usize;
        let required_vertex_bytes = required_vertices
            .checked_add(1)
            .and_then(|vertices| vertices.checked_mul(VERTEX_BYTES))
            .filter(|&bytes| bytes <= self.vertex_capacity)
            .ok_or(vgpu::ERR_UNSUPPORTED)?;
        self.write(self.indices, self.index_capacity, indices)?;
        self.required_vertex_bytes = required_vertex_bytes;
        self.draw.index_count = count;
        Ok(())
    }

    pub(super) fn set_texture(
        &mut self,
        rgba: &[u8],
        [width, height]: [u32; 2],
    ) -> std::result::Result<(), i32> {
        let pitch = width.checked_mul(4).ok_or(vgpu::ERR_UNSUPPORTED)?;
        let expected = (pitch as usize)
            .checked_mul(height as usize)
            .ok_or(vgpu::ERR_UNSUPPORTED)?;
        if expected == 0 || expected != rgba.len() {
            return Err(vgpu::ERR_UNSUPPORTED);
        }
        self.write(self.texture, self.texture_capacity, rgba)?;
        self.draw.texture_width = width;
        self.draw.texture_height = height;
        self.draw.texture_pitch = pitch;
        Ok(())
    }

    pub(super) fn submit(
        &mut self,
        surface: Ui4Surface,
        vertices: &[u8],
        indices: &[u8],
        rgba: &[u8],
        extent: [u32; 2],
        clear_rgba: u32,
    ) -> std::result::Result<(), i32> {
        self.set_indices(indices)?;
        self.set_texture(rgba, extent)?;
        self.submit_prepared(surface, vertices, clear_rgba)
    }

    pub(super) fn submit_prepared(
        &self,
        surface: Ui4Surface,
        vertices: &[u8],
        clear_rgba: u32,
    ) -> std::result::Result<(), i32> {
        if vertices.len() < self.required_vertex_bytes
            || !vertices.len().is_multiple_of(VERTEX_BYTES)
            || self.draw.index_count == 0
            || self.draw.texture_width == 0
        {
            return Err(vgpu::ERR_UNSUPPORTED);
        }
        self.write(self.vertices, self.vertex_capacity, vertices)?;
        self.device
            .submit_ui4_indexed(
                self.queue,
                surface,
                self.pipeline,
                self.vertices,
                self.indices,
                IndexedDraw {
                    clear_rgba8_srgb: clear_rgba,
                    ..self.draw
                },
            )
            .map(|_| ())
    }

    fn write(&self, buffer: Buffer, capacity: usize, data: &[u8]) -> std::result::Result<(), i32> {
        if data.len() > capacity {
            return Err(vgpu::ERR_UNSUPPORTED);
        }
        if self.device.write_buffer(buffer, 0, data)? != data.len() {
            return Err(vgpu::ERR_IO);
        }
        Ok(())
    }
}
