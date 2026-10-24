use super::Rect;
use trueos::ui4_winit::{Error, SceneTarget};
use trueos::vgpu::*;

/// Owns GPU resources until the renderer is dropped, not until a frame is submitted.
pub struct Quads {
    device: Device,
    queue: Queue,
    shader: ShaderModule,
    pipeline: RenderPipeline,
    mesh: Option<(Buffer, Buffer, u32)>,
}

fn ui_error(code: i32) -> Error {
    if code == ERR_BUSY {
        Error::Busy
    } else {
        Error::Unknown(code)
    }
}

impl Quads {
    pub fn new() -> Result<Self, i32> {
        let device = Device::open(Capabilities::DEFAULT.union(Capabilities::PRESENT))?;
        let result = (|| {
            let queue = device.create_queue(QueueClass::Render)?;
            let shader = device
                .create_shader_module(SHADER_PACKAGE_CLIP_POSITION3_IMMEDIATE_RGBA_FNV1A64)?;
            let pipeline = device.create_render_pipeline(shader, 12, 0)?;
            Ok(Self {
                device,
                queue,
                shader,
                pipeline,
                mesh: None,
            })
        })();
        if result.is_err() {
            let _ = device.close();
        }
        result
    }

    /// Upload the visible clipped rectangles when layout, scroll or extent changes.
    /// Width/height are the frame viewport, never the full document's height.
    pub fn set_layout(&mut self, width: u32, height: u32, rectangles: &[Rect]) -> Result<(), i32> {
        if width == 0
            || height == 0
            || rectangles.len() > u32::MAX as usize / 6
            || rectangles
                .iter()
                .any(|&[x, y, w, h]| ![x, y, w, h, x + w, y + h].iter().all(|v| v.is_finite()))
        {
            return Err(ERR_IO);
        }
        let (vertices, indices) = super::geometry(width, height, rectangles);
        let replacement = if indices.is_empty() {
            None
        } else {
            let vb = self
                .device
                .create_buffer(vertices.len(), BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_VERTEX)?;
            let uploaded = (|| {
                let ib = self
                    .device
                    .create_buffer(indices.len(), BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_INDEX)?;
                let result = (|| {
                    if self.device.write_buffer(vb, 0, &vertices)? != vertices.len()
                        || self.device.write_buffer(ib, 0, &indices)? != indices.len()
                    {
                        return Err(ERR_IO);
                    }
                    Ok((vb, ib, (indices.len() / 4) as u32))
                })();
                if result.is_err() {
                    let _ = self.device.destroy_buffer(ib);
                }
                result
            })();
            if uploaded.is_err() {
                let _ = self.device.destroy_buffer(vb);
            }
            Some(uploaded?)
        };
        if let Some((vertices, indices, _)) = self.mesh.take() {
            let _ = self.device.destroy_buffer(indices);
            let _ = self.device.destroy_buffer(vertices);
        }
        self.mesh = replacement;
        Ok(())
    }

    /// Draw into the existing UI4 frame. No geometry creation or uploads occur here.
    /// The acquired surface carries the current viewport dimensions to the GPU.
    pub fn draw(&self, target: &mut SceneTarget) -> Result<(), Error> {
        let Some((vertices, indices, count)) = self.mesh else {
            return target.begin(u32::from_le_bytes([255; 4]));
        };
        target.begin_gpu_frame()?;
        let surface = self
            .device
            .acquire_ui4_surface(target.render_target())
            .map_err(ui_error)?;
        let mut batch = IndexedDrawBatchV2 {
            clear_rgba8_srgb: u32::from_le_bytes([255; 4]),
            draw_count: 1,
            ..Default::default()
        };
        batch.draws[0] = IndexedBatchDrawV2 {
            index_count: count,
            rgba8_srgb: u32::from_le_bytes(super::DEFAULT_COLOR),
            topology: PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
            ..Default::default()
        };
        let point = self
            .device
            .submit_ui4_indexed_batch_v2(
                self.queue,
                surface,
                self.pipeline,
                vertices,
                indices,
                batch,
            )
            .map_err(ui_error)?;
        self.device.wait(self.queue, point.value).map_err(ui_error)
    }
}

impl Drop for Quads {
    fn drop(&mut self) {
        // Draws retire before returning; resources are safe to release at teardown.
        if let Some((vertices, indices, _)) = self.mesh.take() {
            let _ = self.device.destroy_buffer(indices);
            let _ = self.device.destroy_buffer(vertices);
        }
        let _ = self.device.destroy_render_pipeline(self.pipeline);
        let _ = self.device.destroy_shader_module(self.shader);
        let _ = self.device.close();
    }
}
