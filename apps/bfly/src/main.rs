//! Animated Bézier wings through the retained GPU tessellation path.

mod app_shader_packages;
mod celestial_scene;

use bfly::flock::{FRAME_PERIOD_NS, Flock, Snapshot};
use bfly::geometry::{
    FILL_INDEX_BYTES, FILL_PATCH_COUNT, FILL_VERTEX_BYTES, FILL_VERTEX_COUNT, LAYERED_INDEX_BYTES,
    LAYERED_VERTEX_BYTES, LAYERED_VERTEX_COUNT, PATCH_COUNT, VERTEX_COUNT,
};
use celestial_scene::{BACKGROUND_HZ, CelestialScene};
use core::fmt;
use trueos::clock;
use trueos::input::{
    KEYBOARD_KEY_F1, KEYBOARD_KEY_F2, KEYBOARD_KEY_F3, KEYBOARD_KEY_F4, KEYBOARD_KEY_F5,
    KEYBOARD_OUTPUT_FLAG_PRESS, KEYBOARD_OUTPUT_KIND_KEY,
};
use trueos::logl::{self, level};
use trueos::ui4_scene::{Damage, Error as Ui4Error, Frame, POINTER_BUTTON_PRIMARY};
use trueos::vgpu::{
    self, Buffer, Capabilities, Device, Queue, QueueClass, RetainedCamera, RetainedFrameSubmit,
    RetainedMesh, RetainedMeshDescriptor, RetainedTransformSeed,
};
use trueos::vsys;

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
const FRAME_RATE: u32 = 30;
const DRAW_INTERVAL_NS: u64 = 1_000_000_000 / FRAME_RATE as u64;
const RETRY_MS: u64 = 1;
const FLIGHT_PERIOD_SECONDS: u64 = 12;
// Each of the seven splits swaps twice during one cycle.
const SWAP_PERIOD_NS: u64 = 420_000_000_000;
// Surface import can return raw UI4 statuses as well as vGPU errors.
const UI4_NOT_FOUND: i32 = -3;
const UI4_BUSY: i32 = -7;

enum Error {
    Ui4(&'static str, Ui4Error),
    Vgpu(&'static str, i32),
    BackgroundWorker(trueos::worker::SpawnError),
}

impl fmt::Display for Error {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ui4(stage, code) => write!(output, "UI4 {stage} failed ({code:?})"),
            Self::Vgpu(stage, code) => write!(output, "vGPU {stage} failed ({code})"),
            Self::BackgroundWorker(code) => write!(output, "background start failed ({code})"),
        }
    }
}

struct OwnedDevice(Device);

impl Drop for OwnedDevice {
    fn drop(&mut self) {
        let _ = self.0.close();
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum RenderMode {
    Monochrome,
    Smoke,
    Cloud,
    Fill,
    Layered,
}

impl RenderMode {
    fn has_fill(self) -> bool {
        matches!(self, Self::Fill | Self::Layered)
    }

    fn describe(self, frame: &mut Frame) -> Result<(), Error> {
        let (title, mode) = match self {
            Self::Monochrome => ("bfly - monochrome", "monochrome"),
            Self::Smoke => ("bfly - smoke", "smoke"),
            Self::Cloud => ("bfly - cloud", "cloud"),
            Self::Fill => ("bfly - fill", "fill"),
            Self::Layered => ("bfly - layered", "layered"),
        };
        frame
            .set_title(title)
            .map_err(|code| Error::Ui4("set title", code))?;
        logl::log(level::IMPORTANT, format_args!("bfly render mode={mode}"));
        Ok(())
    }
}

struct Renderer {
    device: OwnedDevice,
    queue: Queue,
    meshes: [Option<RetainedMesh>; 5],
    vertices: Buffer,
    indices: Buffer,
}

impl Renderer {
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
                LAYERED_VERTEX_BYTES.len(),
                vgpu::BUFFER_USAGE_VERTEX | vgpu::BUFFER_USAGE_MAP_WRITE,
            )
            .map_err(|code| Error::Vgpu("create vertex buffer", code))?;
        let indices = gpu
            .create_buffer(
                LAYERED_INDEX_BYTES.len(),
                vgpu::BUFFER_USAGE_INDEX | vgpu::BUFFER_USAGE_MAP_WRITE,
            )
            .map_err(|code| Error::Vgpu("create index buffer", code))?;
        write_exact(gpu, vertices, &LAYERED_VERTEX_BYTES)
            .map_err(|code| Error::Vgpu("upload vertices", code))?;
        write_exact(gpu, indices, &LAYERED_INDEX_BYTES)
            .map_err(|code| Error::Vgpu("upload indices", code))?;
        let mut renderer = Self {
            device,
            queue,
            meshes: [None; 5],
            vertices,
            indices,
        };
        renderer.mesh_for(RenderMode::Smoke)?;
        logl::log(
            level::IMPORTANT,
            format_args!(
                "bfly retained patches={PATCH_COUNT} controls={VERTEX_COUNT} fill-patches={FILL_PATCH_COUNT} fill-controls={FILL_VERTEX_COUNT} target-hz={FRAME_RATE}"
            ),
        );
        Ok(renderer)
    }

    fn mesh_for(&mut self, mode: RenderMode) -> Result<RetainedMesh, Error> {
        if let Some(mesh) = self.meshes[mode as usize] {
            return Ok(mesh);
        }
        let vertex_layout = match mode {
            RenderMode::Monochrome => vgpu::RETAINED_VERTEX_LAYOUT_BFLY_MIXED_TRAIL_FLOCK,
            RenderMode::Smoke => vgpu::RETAINED_VERTEX_LAYOUT_BFLY_MIXED_SMOKE_FLOCK,
            RenderMode::Cloud => vgpu::RETAINED_VERTEX_LAYOUT_BFLY_MIXED_CLOUD_TRAIL_FLOCK,
            RenderMode::Fill => vgpu::RETAINED_VERTEX_LAYOUT_BFLY_OUTLINED_INSET_FLOCK,
            RenderMode::Layered => vgpu::RETAINED_VERTEX_LAYOUT_BFLY_LAYERED_FLOCK,
        };
        let (vertex_offset, index_offset, vertex_count) = match mode {
            RenderMode::Fill => (0, 0, FILL_VERTEX_COUNT),
            RenderMode::Layered => (0, 0, LAYERED_VERTEX_COUNT),
            _ => (
                FILL_VERTEX_BYTES.len(),
                FILL_INDEX_BYTES.len(),
                VERTEX_COUNT,
            ),
        };
        let mesh = self
            .device
            .0
            .create_retained_mesh(
                self.vertices,
                self.indices,
                RetainedMeshDescriptor {
                    vertex_offset: vertex_offset as u64,
                    index_offset: index_offset as u64,
                    vertex_count: vertex_count as u32,
                    index_count: vertex_count as u32,
                    vertex_layout,
                    topology: vgpu::RETAINED_TOPOLOGY_BFLY_PATCHLIST_4,
                    ..RetainedMeshDescriptor::default()
                },
            )
            .map_err(|code| Error::Vgpu("create retained wing patches", code))?;
        self.meshes[mode as usize] = Some(mesh);
        Ok(mesh)
    }

    fn draw(
        &self,
        mesh: RetainedMesh,
        mode: RenderMode,
        reset_history: bool,
        frame: &mut Frame,
        started: u64,
        flock: &mut Flock,
    ) -> Result<(), Error> {
        let gpu = self.device.0;
        let (width, height) = (frame.width(), frame.height());

        let mut params = None;
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
            let params = *params.get_or_insert_with(|| {
                let now = clock::monotonic_nanos();
                let events = flock.tick(now);
                if events.spawned != 0 || events.promoted != 0 || events.departed != 0 {
                    logl::log(
                        level::INFO,
                        format_args!(
                            "bfly flock spawned={} promoted={} departed={} population={}",
                            events.spawned,
                            events.promoted,
                            events.departed,
                            flock.population(),
                        ),
                    );
                }
                frame_params(
                    width,
                    height,
                    now.saturating_sub(started),
                    flock.snapshot(),
                    mode,
                    reset_history,
                )
            });
            match gpu.submit_retained_frame(
                self.queue,
                surface,
                mesh,
                self.vertices,
                self.indices,
                params,
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

fn frame_params(
    width: u32,
    height: u32,
    elapsed_ns: u64,
    snapshot: Snapshot,
    mode: RenderMode,
    reset_history: bool,
) -> RetainedFrameSubmit {
    // The flock shader reads its instance records from the retained camera lanes.
    let mut camera = RetainedCamera {
        jitter_frame: [
            width as f32,
            height as f32,
            (elapsed_ns % FRAME_PERIOD_NS) as f32 * 1e-9,
            FLIGHT_PERIOD_SECONDS as f32,
        ],
        ..RetainedCamera::default()
    };
    let records = &snapshot.records;
    camera.view.copy_from_slice(&records[..16]);
    camera.projection.copy_from_slice(&records[16..32]);
    camera.view_projection.copy_from_slice(&records[32..48]);
    camera
        .inverse_view_projection
        .copy_from_slice(&records[48..64]);
    camera.position_near.copy_from_slice(&records[64..68]);
    camera.forward_far.copy_from_slice(&records[68..72]);
    camera.previous_view_projection[..8].copy_from_slice(&records[72..]);
    camera.previous_view_projection[8..12].copy_from_slice(&snapshot.motion);
    camera.previous_view_projection[12] = (elapsed_ns % SWAP_PERIOD_NS) as f32 * 1e-9;
    camera.previous_view_projection[13] = if matches!(mode, RenderMode::Smoke | RenderMode::Cloud) {
        1.0
    } else {
        0.0
    };
    if reset_history {
        camera.previous_view_projection[14] = 1.0;
    }
    let mut submit = RetainedFrameSubmit {
        clear_rgba8_srgb: 0,
        seed_count: 1,
        camera,
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

fn wait_for_draw(next_draw: &mut u64) {
    let remaining_ns = next_draw.saturating_sub(clock::monotonic_nanos());
    if remaining_ns != 0 {
        vsys::sleep_ms(remaining_ns.div_ceil(1_000_000));
    }
    *next_draw = next_draw.saturating_add(DRAW_INTERVAL_NS);
    let now = clock::monotonic_nanos();
    if *next_draw <= now {
        *next_draw = now.saturating_add(DRAW_INTERVAL_NS);
    }
}

fn run() -> Result<(), Error> {
    let mut renderer = Renderer::open()?;
    let mut frame = Frame::open_layered(96, 72, WIDTH, HEIGHT, BACKGROUND_HZ)
        .map_err(|code| Error::Ui4("open window", code))?;
    let mut mode = RenderMode::Smoke;
    let mut previous_mode = mode;
    let mut mesh = renderer.mesh_for(mode)?;
    mode.describe(&mut frame)?;
    frame
        .set_primary_activation(true)
        .map_err(|code| Error::Ui4("enable primary activation", code))?;
    let background = CelestialScene::start(&frame)?;
    let started = clock::monotonic_nanos();
    let mut flock = Flock::new(started);
    let mut frame_count = 0u32;
    let mut measure_from = started;
    let mut presented = false;
    let mut next_draw = started;
    let mut pending_size = None;
    loop {
        wait_for_draw(&mut next_draw);
        while let Some(event) = frame
            .take_pointer_event()
            .map_err(|code| Error::Ui4("read pointer event", code))?
        {
            if event.buttons_pressed & POINTER_BUTTON_PRIMARY != 0 {
                flock.click();
            }
        }
        let mut next_mode = mode;
        while let Some(event) = frame
            .take_keyboard_event()
            .map_err(|code| Error::Ui4("read keyboard event", code))?
        {
            if event.kind == KEYBOARD_OUTPUT_KIND_KEY
                && event.flags & KEYBOARD_OUTPUT_FLAG_PRESS != 0
            {
                match event.key_code {
                    KEYBOARD_KEY_F1 => next_mode = RenderMode::Monochrome,
                    KEYBOARD_KEY_F2 => next_mode = RenderMode::Smoke,
                    KEYBOARD_KEY_F3 => next_mode = RenderMode::Cloud,
                    KEYBOARD_KEY_F4 => next_mode = RenderMode::Fill,
                    KEYBOARD_KEY_F5 => next_mode = RenderMode::Layered,
                    _ => {}
                }
            }
        }
        if next_mode != mode {
            mesh = renderer.mesh_for(next_mode)?;
            mode = next_mode;
            mode.describe(&mut frame)?;
        }
        while let Some(event) = frame
            .take_resize_event()
            .map_err(|code| Error::Ui4("read resize event", code))?
        {
            pending_size = Some((event.width, event.height));
        }
        if let Some((width, height)) = pending_size
            && ((width, height) == (frame.width(), frame.height())
                || background.try_resize(&mut frame, width, height)?)
        {
            pending_size = None;
        }
        let reset_history = previous_mode.has_fill() && !mode.has_fill();
        renderer.draw(mesh, mode, reset_history, &mut frame, started, &mut flock)?;
        previous_mode = mode;
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
