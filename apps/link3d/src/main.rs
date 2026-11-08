#![no_std]

use core::fmt;
use link3d::OrbitCamera;
use trueos::{
    input,
    logl::{self, level},
    ui4_scene::{self, Damage, Error as UiError, Frame, POINTER_BUTTON_PRIMARY},
    vgpu::{
        self, Buffer, Capabilities, Device, QueueClass, RetainedFrameSubmit,
        RetainedMeshDescriptor, RetainedTransformSeed,
    },
    vsys,
};
use trueos_picasso::{Picasso, PicassoError};

include!(concat!(env!("OUT_DIR"), "/prepared_keyboard.rs"));
const SOURCE: &[u8] = include_bytes!("../Keyboard OP.glb");
const VERTICES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/keyboard.vertices.bin"));
const INDICES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/keyboard.indices.bin"));

#[global_allocator]
static ALLOCATOR: trueos::TrueosAllocator = trueos::TrueosAllocator;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo<'_>) -> ! {
    trueos::panic_abort("link3d panic\n")
}

fn main() {
    if let Err(error) = run()
        && !matches!(error, Error::Ui(UiError::NotFound))
    {
        logl::log(level::ERROR, format_args!("link3d: {error}"));
    }
}

fn upload(device: Device, bytes: &[u8], usage: u32) -> Result<Buffer, Error> {
    let buffer = device.create_buffer(bytes.len(), usage | vgpu::BUFFER_USAGE_MAP_WRITE)?;
    if device.write_buffer(buffer, 0, bytes)? != bytes.len() {
        return Err(Error::Gpu(vgpu::ERR_IO));
    }
    Ok(buffer)
}

fn run() -> Result<(), Error> {
    // Reuse Picasso-Example's prepared-asset intake: exact source and prepared
    // geometry cross Picasso's database boundary once, before the GPU upload.
    let picasso = Picasso::new()?;
    picasso.put_embedded_asset("keyboard/source.glb", SOURCE)?;
    picasso.put_embedded_asset("keyboard/vertices", VERTICES)?;
    picasso.put_embedded_asset("keyboard/indices", INDICES)?;
    let vertices = picasso
        .embedded_asset("keyboard/vertices")?
        .ok_or(Error::Asset)?;
    let indices = picasso
        .embedded_asset("keyboard/indices")?
        .ok_or(Error::Asset)?;

    let (width, height) = (800, 600);
    let (x, y) = ui4_scene::output_dimensions()
        .map(|(w, h)| {
            (
                (w.saturating_sub(width) / 2) as i32,
                (h.saturating_sub(height) / 2) as i32,
            )
        })
        .unwrap_or((80, 64));
    let mut frame = Frame::open_streaming(x, y, width, height)?;
    frame.set_title("Link3D — drag to orbit · wheel to zoom · Home to reset")?;
    frame.set_primary_activation(true)?;
    let device = Device::open(Capabilities::DEFAULT.union(Capabilities::PRESENT))?;
    let queue = device.create_queue(QueueClass::Render)?;
    let vertex_buffer = upload(device, &vertices, vgpu::BUFFER_USAGE_VERTEX)?;
    let index_buffer = upload(device, &indices, vgpu::BUFFER_USAGE_INDEX)?;
    let mesh = device.create_retained_mesh(
        vertex_buffer,
        index_buffer,
        RetainedMeshDescriptor {
            vertex_count: VERTEX_COUNT,
            index_count: INDEX_COUNT,
            vertex_layout: vgpu::RETAINED_VERTEX_LAYOUT_POS_NORMAL,
            topology: vgpu::PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
            ..RetainedMeshDescriptor::default()
        },
    )?;
    drop(vertices);
    drop(indices);

    logl::log(
        level::IMPORTANT,
        format_args!(
            "link3d: keyboard loaded once vertices={VERTEX_COUNT} indices={INDEX_COUNT} origin=0,0,0 radius={RADIUS:.3} controls=primary-drag-orbit,wheel-zoom,Home-reset"
        ),
    );
    let mut orbit = OrbitCamera::new(RADIUS);
    let mut previous_view_projection = [0.0; 16];
    let mut pending_resize = None;
    let mut redraws_pending = 1u8;
    loop {
        while let Some(event) = frame.take_resize_event()? {
            pending_resize = Some((event.width.max(1), event.height.max(1)));
        }
        if let Some((width, height)) = pending_resize {
            match frame.resize(width, height) {
                Ok(()) => {
                    pending_resize = None;
                    // Refresh again so a transient resize frame cannot persist
                    // while the camera stays still after maximize/restore.
                    redraws_pending = 2;
                }
                Err(UiError::Busy) => {
                    vsys::sleep_ms(1);
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
        }
        while let Some(event) = frame.take_pointer_event()? {
            // The selecting click must not consume its preceding pointer delta.
            if event.buttons_down & POINTER_BUTTON_PRIMARY != 0
                && event.buttons_pressed & POINTER_BUTTON_PRIMARY == 0
                && (event.dx != 0 || event.dy != 0)
            {
                orbit.orbit(event.dx, event.dy);
                redraws_pending = redraws_pending.max(1);
            }
            if event.wheel != 0 {
                orbit.zoom(event.wheel);
                redraws_pending = redraws_pending.max(1);
            }
        }
        while let Some(event) = frame.take_keyboard_event()? {
            if event.kind == input::KEYBOARD_OUTPUT_KIND_KEY
                && event.flags & input::KEYBOARD_OUTPUT_FLAG_PRESS != 0
                && event.key_code == input::KEYBOARD_KEY_HOME
            {
                orbit = OrbitCamera::new(RADIUS);
                redraws_pending = redraws_pending.max(1);
            }
        }
        if redraws_pending != 0 {
            match frame.begin_gpu_frame() {
                Ok(()) => {}
                Err(UiError::Busy) => {
                    vsys::sleep_ms(1);
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
            let surface = device.acquire_ui4_surface(frame.window_id())?;
            let extent = surface.info();
            let (width, height) = (extent.width, extent.height);
            let camera = orbit.retained(width, height, previous_view_projection);
            let mut submit = RetainedFrameSubmit {
                camera,
                clear_rgba8_srgb: u32::from_le_bytes([128, 128, 128, 0]),
                seed_count: 1,
                ..RetainedFrameSubmit::default()
            };
            // One static keyboard at the world origin, with its authored size.
            submit.seeds[0] = RetainedTransformSeed {
                scale: [1.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
                local_radius: RADIUS,
                ..RetainedTransformSeed::default()
            };
            let point = device.submit_retained_frame(
                queue,
                surface,
                mesh,
                vertex_buffer,
                index_buffer,
                submit,
            )?;
            device.wait(queue, point.value)?;
            loop {
                match frame.publish(Damage::full(width, height)) {
                    Ok(()) => break,
                    Err(UiError::Busy) => {
                        vsys::poll_once();
                        vsys::sleep_ms(1);
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            previous_view_projection = camera.view_projection;
            redraws_pending -= 1;
        }
        vsys::poll_once();
        vsys::sleep_ms(16);
    }
}

enum Error {
    Ui(UiError),
    Gpu(i32),
    Picasso(PicassoError),
    Asset,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ui(error) => write!(f, "UI4: {error:?}"),
            Self::Gpu(code) => write!(f, "GPU: {code}"),
            Self::Picasso(error) => write!(f, "Picasso: {error:?}"),
            Self::Asset => f.write_str("missing prepared keyboard asset"),
        }
    }
}

impl From<UiError> for Error {
    fn from(error: UiError) -> Self {
        Self::Ui(error)
    }
}
impl From<i32> for Error {
    fn from(code: i32) -> Self {
        Self::Gpu(code)
    }
}
impl From<PicassoError> for Error {
    fn from(error: PicassoError) -> Self {
        Self::Picasso(error)
    }
}
