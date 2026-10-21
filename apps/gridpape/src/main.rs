#![no_std]
use trueos::{
    ui4_scene::{self, Damage, Error, Frame},
    vgpu::*,
};

#[global_allocator]
static ALLOCATOR: trueos::TrueosAllocator = trueos::TrueosAllocator;
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    trueos::panic_abort("Gridpape panic\n")
}

fn main() {
    if let Err(error) = run() {
        trueos::logl::log(
            trueos::logl::level::ERROR,
            format_args!("Gridpape: failed errno={error}"),
        );
    }
    trueos::vshell::shutdown_current_blueprint("Gridpape closed");
}

fn ui(error: Error) -> i32 {
    match error {
        Error::Busy => ERR_BUSY,
        Error::NotFound => -2,
        _ => ERR_IO,
    }
}

fn run() -> Result<(), i32> {
    let page = ui4_scene::physical_extent(210, 297).map_err(ui)?;
    let output = ui4_scene::output_dimensions().map_err(ui)?;
    let mut frame = Frame::open_immutable(
        (output.0 as i32 - page.0 as i32) / 2,
        (output.1 as i32 - page.1 as i32) / 2,
        page.0,
        page.1,
    )
    .map_err(ui)?;
    frame.set_title("Gridpape").map_err(ui)?;
    let device = Device::open(Capabilities::DEFAULT.union(Capabilities::PRESENT))?;
    let result = (|| {
        let queue = device.create_queue(QueueClass::Render)?;
        let shader =
            device.create_shader_module(SHADER_PACKAGE_CLIP_POSITION3_IMMEDIATE_RGBA_FNV1A64)?;
        let pipeline = device.create_render_pipeline(shader, 12, 0)?;
        draw(&mut frame, page, device, queue, pipeline)?;
        loop {
            match frame.take_resize_event() {
                Ok(Some(event)) => {
                    frame.resize(event.width, event.height).map_err(ui)?;
                    draw(&mut frame, page, device, queue, pipeline)?;
                }
                Ok(None) | Err(Error::Busy) => trueos::vsys::sleep_ms(16),
                Err(Error::NotFound) => return Ok(()),
                Err(error) => return Err(ui(error)),
            }
        }
    })();
    let close = device.close();
    result.and(close)
}

fn draw(
    frame: &mut Frame,
    page: (u32, u32),
    device: Device,
    queue: Queue,
    pipeline: RenderPipeline,
) -> Result<(), i32> {
    let (vertices, indices) = gridpape::geometry(page, (frame.width(), frame.height()));
    let vb = device.create_buffer(vertices.len(), BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_VERTEX)?;
    let ib = device.create_buffer(indices.len(), BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_INDEX)?;
    if device.write_buffer(vb, 0, &vertices)? != vertices.len()
        || device.write_buffer(ib, 0, &indices)? != indices.len()
    {
        return Err(ERR_IO);
    }
    loop {
        match frame.begin_gpu_frame() {
            Err(Error::Busy) => trueos::vsys::sleep_ms(16),
            result => {
                result.map_err(ui)?;
                break;
            }
        }
    }
    let mut batch = IndexedDrawBatchV2 {
        clear_rgba8_srgb: u32::from_le_bytes([255; 4]),
        draw_count: 1,
        ..Default::default()
    };
    batch.draws[0] = IndexedBatchDrawV2 {
        index_count: gridpape::INDEX_COUNT,
        rgba8_srgb: u32::from_le_bytes([190, 190, 190, 255]),
        topology: PRIMITIVE_TOPOLOGY_LINE_LIST,
        ..Default::default()
    };
    loop {
        let surface = device.acquire_ui4_surface(frame.window_id())?;
        match device.submit_ui4_indexed_batch_v2(queue, surface, pipeline, vb, ib, batch) {
            Ok(point) => {
                device.wait(queue, point.value)?;
                break;
            }
            Err(ERR_BUSY) => trueos::vsys::sleep_ms(16),
            Err(error) => return Err(error),
        }
    }
    loop {
        match frame.publish(Damage::full(frame.width(), frame.height())) {
            Err(Error::Busy) => trueos::vsys::sleep_ms(16),
            result => {
                result.map_err(ui)?;
                break;
            }
        }
    }
    device.destroy_buffer(vb)?;
    device.destroy_buffer(ib)
}
