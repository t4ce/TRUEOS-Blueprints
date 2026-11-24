//! Animated Bézier outlines through the retained GPU tessellation path.
#![no_std]

mod app_shader_packages;

use bfly::geometry::{INDEX_BYTES, PATCH_COUNT, VERTEX_BYTES, VERTEX_COUNT};
use core::fmt;
use trueos::clock;
use trueos::logl::{self, level};
use trueos::ui4_scene::{Damage, Error as Ui4Error, Frame};
use trueos::vgpu::{
    self, Buffer, Capabilities, Device, Queue, QueueClass, RetainedCamera, RetainedFrameSubmit,
    RetainedMesh, RetainedMeshDescriptor, RetainedTransformSeed,
};
use trueos::vsys;

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const FRAME_RATE: u32 = 30;
const RETRY_MS: u64 = 1;
// Matches the shader's flight lap and spans six complete wingbeats.
const FLIGHT_PERIOD_SECONDS: u64 = 12;
const FLIGHT_PERIOD_NS: u64 = FLIGHT_PERIOD_SECONDS * 1_000_000_000;
const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];
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
    mesh: RetainedMesh,
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
        let vertices = gpu
            .create_buffer(
                VERTEX_BYTES.len(),
                vgpu::BUFFER_USAGE_VERTEX | vgpu::BUFFER_USAGE_MAP_WRITE,
            )
            .map_err(|code| Error::Vgpu("create vertex buffer", code))?;
        let indices = gpu
            .create_buffer(
                INDEX_BYTES.len(),
                vgpu::BUFFER_USAGE_INDEX | vgpu::BUFFER_USAGE_MAP_WRITE,
            )
            .map_err(|code| Error::Vgpu("create index buffer", code))?;
        write_exact(gpu, vertices, &VERTEX_BYTES)
            .map_err(|code| Error::Vgpu("upload vertices", code))?;
        write_exact(gpu, indices, &INDEX_BYTES)
            .map_err(|code| Error::Vgpu("upload indices", code))?;
        let mesh = gpu
            .create_retained_mesh(
                vertices,
                indices,
                RetainedMeshDescriptor {
                    vertex_count: VERTEX_COUNT as u32,
                    index_count: VERTEX_COUNT as u32,
                    vertex_layout: vgpu::RETAINED_VERTEX_LAYOUT_BFLY_PATCHES,
                    topology: vgpu::RETAINED_TOPOLOGY_BFLY_PATCHLIST_4,
                    ..RetainedMeshDescriptor::default()
                },
            )
            .map_err(|code| Error::Vgpu("create retained wing patches", code))?;
        logl::log(
            level::IMPORTANT,
            format_args!(
                "bfly retained patches={PATCH_COUNT} controls={VERTEX_COUNT} target-hz={FRAME_RATE}"
            ),
        );
        Ok(Self {
            device,
            queue,
            mesh,
            vertices,
            indices,
        })
    }

    fn draw(&self, frame: &mut Frame, started: u64) -> Result<(), Error> {
        let gpu = self.device.0;
        let (width, height) = (frame.width(), frame.height());

        let point = loop {
            retry_ui4("begin frame", || frame.begin_visual_gpu_frame())?;

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
            let elapsed = clock::monotonic_nanos().saturating_sub(started) % FLIGHT_PERIOD_NS;
            let time = elapsed as f32 * 1e-9;
            match gpu.submit_retained_frame(
                self.queue,
                surface,
                self.mesh,
                self.vertices,
                self.indices,
                frame_params(width, height, time),
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

fn frame_params(width: u32, height: u32, time: f32) -> RetainedFrameSubmit {
    let mut submit = RetainedFrameSubmit {
        clear_rgba8_srgb: u32::from_le_bytes([8, 10, 12, 255]),
        seed_count: 1,
        camera: RetainedCamera {
            view: IDENTITY,
            projection: IDENTITY,
            view_projection: IDENTITY,
            inverse_view_projection: IDENTITY,
            previous_view_projection: IDENTITY,
            position_near: [0.0, 0.0, 0.0, 0.25],
            forward_far: [0.0, 0.0, -1.0, 2048.0],
            jitter_frame: [
                width as f32,
                height as f32,
                time,
                FLIGHT_PERIOD_SECONDS as f32,
            ],
        },
        ..RetainedFrameSubmit::default()
    };
    submit.seeds[0] = RetainedTransformSeed {
        scale: [1.0; 3],
        rotation: [0.0, 0.0, 0.0, 1.0],
        ..RetainedTransformSeed::default()
    };
    submit
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
    let renderer = OutlineRenderer::open()?;
    let mut frame = Frame::open_visual(96, 72, WIDTH, HEIGHT, FRAME_RATE)
        .map_err(|code| Error::Ui4("open window", code))?;
    frame
        .set_title("bfly")
        .map_err(|code| Error::Ui4("set title", code))?;
    let started = clock::monotonic_nanos();
    let mut frame_count = 0u32;
    let mut measure_from = started;
    let mut presented = false;
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
        }
        renderer.draw(&mut frame, started)?;
        if !presented
            && frame
                .take_first_presentation()
                .map_err(|code| Error::Ui4("read first presentation", code))?
        {
            presented = true;
            logl::log(
                level::IMPORTANT,
                "bfly tessellated outline reached the display",
            );
        }
        frame_count += 1;
        let now = clock::monotonic_nanos();
        let elapsed = now.saturating_sub(measure_from);
        if elapsed >= 5_000_000_000 {
            let fps = (frame_count as f64 * 1_000_000_000.0) / elapsed as f64;
            logl::log(
                level::INFO,
                format_args!("bfly tessellated outline fps={fps:.1} frames={frame_count}"),
            );
            measure_from = now;
            frame_count = 0;
        }
    }
}
