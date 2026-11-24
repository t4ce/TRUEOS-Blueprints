//! Native outline reference for the standalone butterfly renderer.
#![no_std]

mod app_shader_packages;

use bfly::geometry;
use core::fmt;
use trueos::logl::{self, level};
use trueos::ui4_scene::{Damage, Error as Ui4Error, Frame};
use trueos::vgpu::{self, Buffer, Capabilities, Device, Queue, QueueClass, RenderPipeline};
use trueos::vsys;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;
const RETRY_MS: u64 = 16;
// Surface import can return raw UI4 statuses as well as vGPU errors.
const UI4_NOT_FOUND: i32 = -3;
const UI4_BUSY: i32 = -7;

#[global_allocator]
static ALLOCATOR: trueos::TrueosAllocator = trueos::TrueosAllocator;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    trueos::panic_abort("bfly panic\n")
}

enum Error {
    Ui4(&'static str, Ui4Error),
    Vgpu(&'static str, i32),
}

impl fmt::Display for Error {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ui4(stage, code) => write!(output, "UI4 {stage} failed ({code:?})"),
            Self::Vgpu(stage, code) => write!(output, "vGPU {stage} failed ({code})"),
        }
    }
}

struct OwnedDevice(Device);

impl Drop for OwnedDevice {
    fn drop(&mut self) {
        let _ = self.0.close();
    }
}

struct OutlineRenderer {
    device: OwnedDevice,
    queue: Queue,
    pipeline: RenderPipeline,
    vertices: Buffer,
    indices: Buffer,
}

impl OutlineRenderer {
    fn open() -> Result<Self, Error> {
        let device = OwnedDevice(
            Device::open(Capabilities::DEFAULT.union(Capabilities::PRESENT))
                .map_err(|code| Error::Vgpu("open", code))?,
        );
        let gpu = device.0;
        let queue = gpu
            .create_queue(QueueClass::Render)
            .map_err(|code| Error::Vgpu("create queue", code))?;
        let shader = gpu
            .create_shader_module(vgpu::SHADER_PACKAGE_CLIP_POSITION3_IMMEDIATE_RGBA_FNV1A64)
            .map_err(|code| Error::Vgpu("create shader", code))?;
        let pipeline = gpu
            .create_render_pipeline(shader, geometry::STRIDE as u32, 0)
            .map_err(|code| Error::Vgpu("create pipeline", code))?;
        let vertices = gpu
            .create_buffer(
                geometry::VERTEX_BYTES,
                vgpu::BUFFER_USAGE_VERTEX | vgpu::BUFFER_USAGE_MAP_WRITE,
            )
            .map_err(|code| Error::Vgpu("create vertex buffer", code))?;
        let indices = gpu
            .create_buffer(
                geometry::INDEX_BYTES.len(),
                vgpu::BUFFER_USAGE_INDEX | vgpu::BUFFER_USAGE_MAP_WRITE,
            )
            .map_err(|code| Error::Vgpu("create index buffer", code))?;
        write_exact(gpu, indices, &geometry::INDEX_BYTES)
            .map_err(|code| Error::Vgpu("upload indices", code))?;
        Ok(Self {
            device,
            queue,
            pipeline,
            vertices,
            indices,
        })
    }

    fn draw(&self, frame: &mut Frame) -> Result<(), Error> {
        let gpu = self.device.0;
        let (width, height) = (frame.width(), frame.height());
        write_exact(gpu, self.vertices, &geometry::vertex_bytes(width, height))
            .map_err(|code| Error::Vgpu("upload vertices", code))?;

        let point = loop {
            retry_ui4("begin frame", || frame.begin_gpu_frame())?;

            // A failed import can leave the frame's write lease in place.
            // Retry import against that lease instead of beginning a new frame.
            let surface = loop {
                match gpu.acquire_ui4_surface(frame.window_id()) {
                    Ok(surface) => break surface,
                    Err(UI4_BUSY | vgpu::ERR_BUSY) => vsys::sleep_ms(RETRY_MS),
                    Err(UI4_NOT_FOUND) => {
                        return Err(Error::Ui4("import surface", Ui4Error::NotFound));
                    }
                    Err(code) => return Err(Error::Vgpu("import surface", code)),
                }
            };
            match gpu.submit_ui4_indexed_batch_v2(
                self.queue,
                surface,
                self.pipeline,
                self.vertices,
                self.indices,
                geometry::batch(),
            ) {
                Ok(point) => break point,
                // The consumed surface import is dropped on submission failure.
                Err(vgpu::ERR_BUSY) => vsys::sleep_ms(RETRY_MS),
                Err(UI4_NOT_FOUND) => return Err(Error::Ui4("submit frame", Ui4Error::NotFound)),
                Err(code) => return Err(Error::Vgpu("submit frame", code)),
            }
        };
        gpu.wait(self.queue, point.value)
            .map_err(|code| Error::Vgpu("wait for frame", code))?;
        retry_ui4("publish frame", || {
            frame.publish(Damage::full(width, height))
        })
    }
}

fn write_exact(device: Device, buffer: Buffer, bytes: &[u8]) -> Result<(), i32> {
    if device.write_buffer(buffer, 0, bytes)? == bytes.len() {
        Ok(())
    } else {
        Err(vgpu::ERR_IO)
    }
}

fn retry_ui4<T>(
    stage: &'static str,
    mut action: impl FnMut() -> Result<T, Ui4Error>,
) -> Result<T, Error> {
    loop {
        match action() {
            Err(Ui4Error::Busy) => vsys::sleep_ms(RETRY_MS),
            result => return result.map_err(|code| Error::Ui4(stage, code)),
        }
    }
}

fn main() {
    match run() {
        Ok(()) | Err(Error::Ui4(_, Ui4Error::NotFound)) => {}
        Err(error) => {
            logl::log(level::ERROR, format_args!("bfly {error}"));
            let _ = trueos::vshell::shutdown_current_blueprint("bfly renderer failed");
        }
    }
}

fn run() -> Result<(), Error> {
    let mut frame = Frame::open_streaming(96, 72, WIDTH, HEIGHT)
        .map_err(|code| Error::Ui4("open window", code))?;
    frame
        .set_title("bfly")
        .map_err(|code| Error::Ui4("set title", code))?;
    let renderer = OutlineRenderer::open()?;
    renderer.draw(&mut frame)?;
    logl::log(level::INFO, "bfly presented the native outline reference");

    loop {
        let mut pending_size = None;
        while let Some(event) = frame
            .take_resize_event()
            .map_err(|code| Error::Ui4("read resize event", code))?
        {
            pending_size = Some((event.width, event.height));
        }
        if let Some((width, height)) = pending_size
            && (width, height) != (frame.width(), frame.height())
        {
            retry_ui4("resize window", || frame.resize(width, height))?;
            renderer.draw(&mut frame)?;
        }
        vsys::sleep_ms(RETRY_MS);
    }
}
