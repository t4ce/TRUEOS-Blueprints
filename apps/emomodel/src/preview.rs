use anyhow::{Result, anyhow, ensure};
use std::time::{Duration, Instant};
use trueos::{
    logl,
    ui4_scene::{self, Damage, Error, Frame},
    vgpu::{
        self, Buffer, Capabilities, Device, IndexedBatchDrawV2, IndexedDrawBatchV2, Queue,
        QueueClass, RenderPipeline, TimelinePoint,
    },
    vsys,
};

use crate::mesh::{self, Mesh};

const POSITION_BYTES: usize = size_of::<[f32; 3]>();
// The vGPU surface import can propagate UI4's busy status before mapping.
pub(super) const UI4_IMPORT_BUSY: i32 = -7;

pub(super) fn window_placement() -> Result<(i32, i32, u32, u32)> {
    let (output_width, output_height) = ui4_scene::output_dimensions().map_err(ui_error)?;
    let (width, height) =
        ui4_scene::fit_frame_extent(output_width.min(640), output_height.min(560))
            .map_err(ui_error)?;
    let x = ((output_width - width) / 2) as i32;
    let y = ((output_height - height) / 2) as i32;
    Ok((x, y, width, height))
}

pub fn show(glb: &[u8]) -> Result<()> {
    let (x, y, width, height) = window_placement()?;
    let mesh = mesh::prepare(glb, width, height)?;
    let mut frame = Frame::open_immutable(x, y, width, height).map_err(ui_error)?;
    frame.set_title("EmoModel").map_err(ui_error)?;
    let device =
        Device::open(Capabilities::DEFAULT.union(Capabilities::PRESENT)).map_err(gpu_error)?;
    let shown = (|| {
        let renderer = Renderer::new(device, &mesh)?;
        show_frame(&mut frame, &renderer, &mesh)
    })();
    let released = device.close().map_err(gpu_error);
    shown?;
    released
}

fn show_frame(frame: &mut Frame, renderer: &Renderer, mesh: &Mesh) -> Result<()> {
    let (width, height) = (frame.width(), frame.height());
    let point = renderer.present(frame, mesh)?;
    let triangles = mesh.triangle_count;
    let mut pending_resize = None;
    let mut first_presented = false;

    loop {
        if !first_presented {
            match frame.take_first_presentation() {
                Ok(true) => {
                    first_presented = true;
                    let _ = logl::log_record(
                        logl::level::IMPORTANT,
                        "emomodel",
                        format_args!(
                            "emomodel: native triangle list presented at {width}x{height}; triangles={triangles} gpu_serial={}\n",
                            point.physical_serial,
                        ),
                    );
                }
                Ok(false) => (),
                Err(Error::NotFound) => return Ok(()),
                Err(error) => return Err(ui_error(error)),
            }
        }

        // UI4 emits final extents. Keep its latest request if staging is busy.
        loop {
            match frame.take_resize_event() {
                Ok(Some(event)) => pending_resize = Some((event.width, event.height)),
                Ok(None) => break,
                Err(Error::NotFound) => return Ok(()),
                Err(error) => return Err(ui_error(error)),
            }
        }
        if let Some((width, height)) = pending_resize {
            match frame.resize(width, height) {
                Ok(()) => {
                    pending_resize = None;
                    let point = renderer.present(frame, mesh)?;
                    let _ = logl::log_record(
                        logl::level::IMPORTANT,
                        "emomodel",
                        format_args!(
                            "emomodel: submitted resize frame at {width}x{height}; triangles={triangles} gpu_serial={}\n",
                            point.physical_serial,
                        ),
                    );
                }
                Err(Error::Busy) => (),
                Err(Error::NotFound) => return Ok(()),
                Err(error) => return Err(ui_error(error)),
            }
        }
        vsys::sleep_ms(if pending_resize.is_some() { 16 } else { 50 });
    }
}

struct Renderer {
    device: Device,
    queue: Queue,
    pipeline: RenderPipeline,
    vertex_buffer: Buffer,
    index_buffer: Buffer,
    batch: IndexedDrawBatchV2,
}

impl Renderer {
    fn new(device: Device, mesh: &Mesh) -> Result<Self> {
        let queue = device.create_queue(QueueClass::Render).map_err(gpu_error)?;
        // This selects TRUEOS's existing flat-color pipeline.
        let shader = device
            .create_shader_module(vgpu::SHADER_PACKAGE_CLIP_POSITION3_IMMEDIATE_RGBA_FNV1A64)
            .map_err(gpu_error)?;
        let pipeline = device
            .create_render_pipeline(shader, POSITION_BYTES as u32, 0)
            .map_err(gpu_error)?;
        let index_count = u32::try_from(mesh.indices.len())?;
        let vertex_buffer = device
            .create_buffer(
                mesh.vertex_count() * POSITION_BYTES,
                vgpu::BUFFER_USAGE_VERTEX | vgpu::BUFFER_USAGE_MAP_WRITE,
            )
            .map_err(gpu_error)?;
        let indices: Vec<_> = mesh
            .indices
            .iter()
            .copied()
            .flat_map(u32::to_le_bytes)
            .collect();
        let index_buffer = device
            .create_buffer(
                indices.len(),
                vgpu::BUFFER_USAGE_INDEX | vgpu::BUFFER_USAGE_MAP_WRITE,
            )
            .map_err(gpu_error)?;
        write_geometry(device, index_buffer, &indices)?;
        let mut batch = IndexedDrawBatchV2 {
            clear_rgba8_srgb: u32::from_le_bytes([249, 249, 248, 255]),
            draw_count: 1,
            ..Default::default()
        };
        batch.draws[0] = IndexedBatchDrawV2 {
            index_count,
            rgba8_srgb: u32::from_le_bytes([30, 34, 39, 255]),
            topology: vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
            ..Default::default()
        };
        Ok(Self {
            device,
            queue,
            pipeline,
            vertex_buffer,
            index_buffer,
            batch,
        })
    }

    fn present(&self, frame: &mut Frame, mesh: &Mesh) -> Result<TimelinePoint> {
        let (width, height) = (frame.width(), frame.height());
        let vertices: Vec<_> = mesh
            .projected_vertices(width, height)
            .flatten()
            .flat_map(f32::to_le_bytes)
            .collect();
        write_geometry(self.device, self.vertex_buffer, &vertices)?;

        let deadline = Instant::now() + Duration::from_secs(2);
        let point = loop {
            match frame.begin_gpu_frame() {
                Ok(()) => (),
                Err(Error::Busy) if Instant::now() < deadline => {
                    vsys::sleep_ms(16);
                    continue;
                }
                Err(error) => return Err(ui_error(error)),
            }
            let surface = loop {
                match self.device.acquire_ui4_surface(frame.window_id()) {
                    Ok(surface) => break surface,
                    Err(vgpu::ERR_BUSY | UI4_IMPORT_BUSY) if Instant::now() < deadline => {
                        vsys::sleep_ms(16);
                    }
                    Err(code) => return Err(gpu_error(code)),
                }
            };
            match self.device.submit_ui4_indexed_batch_v2(
                self.queue,
                surface,
                self.pipeline,
                self.vertex_buffer,
                self.index_buffer,
                self.batch,
            ) {
                Ok(point) => break point,
                Err(vgpu::ERR_BUSY) if Instant::now() < deadline => vsys::sleep_ms(16),
                Err(code) => return Err(gpu_error(code)),
            }
        };

        let damage = Damage::full(width, height);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match frame.publish(damage) {
                Ok(()) => return Ok(point),
                Err(Error::Busy) if Instant::now() < deadline => vsys::sleep_ms(16),
                Err(error) => return Err(ui_error(error)),
            }
        }
    }
}

pub(super) fn write_geometry(device: Device, buffer: Buffer, bytes: &[u8]) -> Result<()> {
    let written = device.write_buffer(buffer, 0, bytes).map_err(gpu_error)?;
    ensure!(written == bytes.len(), "short EmoModel geometry upload");
    Ok(())
}

pub(super) fn ui_error(error: Error) -> anyhow::Error {
    anyhow!("EmoModel UI4 frame: {error:?}")
}

pub(super) fn gpu_error(code: i32) -> anyhow::Error {
    anyhow!("EmoModel vGPU: {code}")
}
