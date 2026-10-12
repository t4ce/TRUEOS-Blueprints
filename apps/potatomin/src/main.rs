#![no_std]
extern crate alloc;

use alloc::vec;
use potatomin::{HEIGHT, LINE_COUNT, POINT_COUNT, RGBA_BYTES, WIDTH, geometry, png_rgba8};
use trueos::{
    async_fs,
    logl::{self, level},
    vgpu::*,
};

#[global_allocator]
static ALLOCATOR: trueos::TrueosAllocator = trueos::TrueosAllocator;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    trueos::panic_abort("potatomin panic\n")
}

fn main() {
    let result = run();
    let reason = match result {
        Ok(()) => "potatomin: dots.png and lines.png saved; one-shot complete",
        Err(code) => {
            logl::log(level::ERROR, format_args!("potatomin: failed errno={code}"));
            "potatomin: render or PNG write failed"
        }
    };
    logl::log(level::INFO, reason);
    if !trueos::vshell::shutdown_current_blueprint(reason) {
        logl::log(level::ERROR, "potatomin: Blueprint shutdown request failed");
    }
}

fn run() -> Result<(), i32> {
    let device = Device::open(Capabilities::DEFAULT.union(Capabilities::PRESENT))?;
    let result = render_and_save(device);
    let close = device.close();
    result.and(close)
}

fn render_and_save(device: Device) -> Result<(), i32> {
    let queue = device.create_queue(QueueClass::Render)?;
    let shader =
        device.create_shader_module(SHADER_PACKAGE_CLIP_POSITION3_IMMEDIATE_RGBA_FNV1A64)?;
    let pipeline = device.create_render_pipeline(shader, 12, 0)?;
    let (vertices, indices) = geometry();
    let vertex_buffer =
        device.create_buffer(vertices.len(), BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_VERTEX)?;
    let index_buffer =
        device.create_buffer(indices.len(), BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_INDEX)?;
    let output =
        device.create_buffer(RGBA_BYTES, BUFFER_USAGE_MAP_READ | BUFFER_USAGE_MAP_WRITE)?;
    if device.write_buffer(vertex_buffer, 0, &vertices)? != vertices.len()
        || device.write_buffer(index_buffer, 0, &indices)? != indices.len()
    {
        return Err(ERR_IO);
    }
    let mut pixels = vec![0u8; RGBA_BYTES];
    for (name, topology, color, point_width) in [
        (
            "dots.png",
            PRIMITIVE_TOPOLOGY_POINT_LIST,
            [40, 220, 100, 255],
            8,
        ),
        (
            "lines.png",
            PRIMITIVE_TOPOLOGY_LINE_LIST,
            [240, 65, 65, 255],
            0,
        ),
    ] {
        let mut batch = IndexedDrawBatchV2 {
            clear_rgba8_srgb: u32::from_le_bytes([18, 18, 22, 255]),
            draw_count: 1,
            ..Default::default()
        };
        batch.draws[0] = IndexedBatchDrawV2 {
            index_count: POINT_COUNT,
            first_index: 0,
            base_vertex: 0,
            rgba8_srgb: u32::from_le_bytes(color),
            topology,
            reserved: point_width,
        };
        // Busy means the renderer accepted no work. Retry the same immutable
        // batch; each image has exactly one successful native submission.
        let point = loop {
            match device.submit_offscreen_indexed_batch_v1(
                queue,
                output,
                WIDTH,
                HEIGHT,
                pipeline,
                vertex_buffer,
                index_buffer,
                batch,
            ) {
                Ok(point) => break point,
                Err(ERR_BUSY) => {
                    trueos::vsys::poll_once();
                    trueos::vsys::sleep_ms(16);
                }
                Err(error) => return Err(error),
            }
        };
        device.wait(queue, point.value)?;
        if device.read_buffer(output, 0, &mut pixels)? != pixels.len() {
            return Err(ERR_IO);
        }
        let png = png_rgba8(&pixels);
        async_fs::block_on(async_fs::write_file_typed(
            name.as_bytes(),
            &png,
            async_fs::ContentTypeId::PNG,
        ))?;
        logl::log(
            level::INFO,
            format_args!(
                "potatomin: saved {name} {WIDTH}x{HEIGHT} native_primitives={} png_bytes={} gpu_serial={}",
                if topology == PRIMITIVE_TOPOLOGY_POINT_LIST {
                    POINT_COUNT
                } else {
                    LINE_COUNT
                },
                png.len(),
                point.physical_serial,
            ),
        );
    }
    Ok(())
}
