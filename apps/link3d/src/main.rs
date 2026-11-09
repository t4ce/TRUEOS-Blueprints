#![no_std]

extern crate alloc;

mod renderer;

use alloc::vec::Vec;
use core::fmt;
use link3d::{
    OrbitCamera,
    roster::{self, virtual_keyboards},
    scene::{metallic_roughness_bmp, prepare_geometry, prepare_scene},
};
use trueos::{
    clock, hid, input,
    logl::{self, level},
    ui4_scene::{self, Damage, Error as UiError, Frame, POINTER_BUTTON_PRIMARY},
    vgpu::{Capabilities, Device, QueueClass},
    vsys,
};
use trueos_picasso::{Picasso, PicassoError};

include!(concat!(env!("OUT_DIR"), "/prepared_keyboard.rs"));
const SOURCE: &[u8] = include_bytes!("../Keyboard k70 mini pro.glb");
const VERTICES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/keyboard.vertices.bin"));
const INDICES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/keyboard.indices.bin"));
const PARTS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/keyboard.parts.bin"));
const INVENTORY_INTERVAL_MS: u64 = 3_000;

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
        logl::log_record(level::ERROR, "link3d", format_args!("link3d: {error}"));
    }
}

fn run() -> Result<(), Error> {
    // Reuse Picasso-Example's prepared-asset intake: exact source and prepared
    // geometry cross Picasso's database boundary once, before the GPU upload.
    let picasso = Picasso::new()?;
    picasso.put_embedded_asset("keyboard/source.glb", SOURCE)?;
    picasso.put_embedded_asset("keyboard/vertices", VERTICES)?;
    picasso.put_embedded_asset("keyboard/indices", INDICES)?;
    picasso.put_embedded_asset("keyboard/parts", PARTS)?;
    picasso.put_embedded_asset("keyboard/metallic-roughness.bmp", &metallic_roughness_bmp())?;
    let vertices = picasso
        .embedded_asset("keyboard/vertices")?
        .ok_or(Error::Asset)?;
    let indices = picasso
        .embedded_asset("keyboard/indices")?
        .ok_or(Error::Asset)?;
    let parts = picasso
        .embedded_asset("keyboard/parts")?
        .ok_or(Error::Asset)?;
    let metallic_roughness = picasso
        .embedded_asset("keyboard/metallic-roughness.bmp")?
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
    frame.set_escape_key_action(
        trueos::ui4_solara_text::FrameEscapeKeyAction::DeliverToApplication,
    )?;
    frame.set_title("Link3D — drag to orbit · wheel to zoom · Home to reset")?;
    frame.set_primary_activation(true)?;
    let device = Device::open(Capabilities::DEFAULT.union(Capabilities::PRESENT))?;
    let queue = device.create_queue(QueueClass::Render)?;
    let geometry = prepare_geometry(&vertices, &parts, &indices);
    let mut renderer = renderer::Renderer::new(device, &geometry, RADIUS, &metallic_roughness)?;
    let mut legends = link3d::legends::Legends::default();
    let mut feedback = link3d::feedback::Feedback::default();
    let mouse_shape = geometry.mouse;
    drop(geometry.vertices);
    drop(geometry.indices);
    drop(vertices);
    drop(indices);
    drop(parts);
    drop(metallic_roughness);

    logl::log_record(
        level::IMPORTANT,
        "link3d",
        format_args!(
            "link3d: K70 and mouse cuboid prepared once geometry-uploads=1 instance-bytes=64 keyboard-vertices={VERTEX_COUNT} keyboard-indices={INDEX_COUNT} inventory=HID-HUT/virtual-keyboards+all-mice refresh-ms={INVENTORY_INTERVAL_MS} colors=input-combo-palette chassis=gray,metallic:0.5,roughness:0.4 legends=kernel-microfont,A-Z+0-9,black-flat-quads feedback=ui4-routed-physical-hid,red-key-top controls=primary-drag-orbit,wheel-zoom,Home-reset"
        ),
    );
    let mut orbit = OrbitCamera::new(RADIUS);
    let mut previous_view_projection = [0.0; 16];
    let mut pending_resize = None;
    let mut redraws_pending = 1u8;
    let mut keyboards = Vec::new();
    let mut mice = Vec::new();
    let mut inventory_initialized = false;
    let mut next_inventory_at = 0;
    let mut scene_center = [0.0; 3];
    let mut scene_radius = RADIUS;
    let mut captions = Vec::new();
    loop {
        let now = clock::monotonic_millis();
        if feedback.expire(now) {
            redraws_pending = redraws_pending.max(1);
        }
        if legends.poll(&mut frame)? {
            redraws_pending = redraws_pending.max(1);
        }
        if now >= next_inventory_at {
            next_inventory_at = now.saturating_add(INVENTORY_INTERVAL_MS);
            let devices = hid::hid_hut_keyboards();
            let mouse_devices = hid::hid_hut_mice();
            let combos = hid::input_combos();
            let current = virtual_keyboards(&devices, &combos);
            let current_mice = roster::mice(&mouse_devices, &combos);
            let roster_changed = current != keyboards || current_mice != mice;
            if roster_changed {
                let row = prepare_scene(&current, &current_mice, RADIUS, &mouse_shape);
                renderer.update_instances(&row, &current, &current_mice)?;
                feedback.set_roster(&current);
                scene_center = row.center;
                scene_radius = row.radius;
                captions = row.captions;
                orbit.set_scene(scene_center, scene_radius);
                previous_view_projection = [0.0; 16];
                redraws_pending = redraws_pending.max(1);
                keyboards = current;
                mice = current_mice;
            }
            if !inventory_initialized || roster_changed {
                logl::log_record(
                    level::IMPORTANT,
                    "link3d",
                    format_args!(
                        "link3d: inventory refreshed keyboards={} virtual-keyboards={} mice={} combos={}",
                        devices.len(),
                        keyboards.len(),
                        mice.len(),
                        combos.len()
                    ),
                );
                inventory_initialized = true;
            }
        }
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
                orbit.set_scene(scene_center, scene_radius);
                redraws_pending = redraws_pending.max(1);
            }
        }
        while let Some(event) = frame.take_physical_keyboard_event()? {
            if feedback.event(&event, now) {
                redraws_pending = redraws_pending.max(1);
            }
        }
        if feedback.any() && feedback.reconcile_selection(&frame.input_routes()?) {
            redraws_pending = redraws_pending.max(1);
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
            let point = renderer.render(queue, surface, camera)?;
            device.wait(queue, point.value)?;
            legends.draw(
                &mut frame,
                &camera,
                width,
                height,
                keyboards.len(),
                RADIUS,
                &feedback,
                &captions,
            )?;
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
