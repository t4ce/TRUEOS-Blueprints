use super::Rect;
use trueos::vgpu::*;

pub(super) fn render(width: u32, height: u32, rectangles: &[Rect]) -> Result<Vec<u8>, i32> {
    let rgba_bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .filter(|_| width > 0 && height > 0)
        .ok_or(ERR_IO)?;
    if rectangles.len() > u32::MAX as usize / 8
        || rectangles.iter().flatten().any(|v| !v.is_finite())
    {
        return Err(ERR_IO);
    }
    if rectangles.is_empty() {
        return Ok(super::png::png_rgba8(width, height, &vec![255; rgba_bytes]));
    }
    let device = Device::open(Capabilities::DEFAULT.union(Capabilities::PRESENT))?;
    let result = render_device(device, width, height, rectangles, rgba_bytes);
    let close = device.close();
    result.and_then(|png| close.map(|()| png))
}

fn render_device(
    device: Device,
    width: u32,
    height: u32,
    rectangles: &[Rect],
    rgba_bytes: usize,
) -> Result<Vec<u8>, i32> {
    let queue = device.create_queue(QueueClass::Render)?;
    let shader =
        device.create_shader_module(SHADER_PACKAGE_CLIP_POSITION3_IMMEDIATE_RGBA_FNV1A64)?;
    let pipeline = device.create_render_pipeline(shader, 12, 0)?;
    let (vertices, indices) = super::geometry(width, height, rectangles);
    let vertex_buffer =
        device.create_buffer(vertices.len(), BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_VERTEX)?;
    let index_buffer =
        device.create_buffer(indices.len(), BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_INDEX)?;
    let output =
        device.create_buffer(rgba_bytes, BUFFER_USAGE_MAP_READ | BUFFER_USAGE_MAP_WRITE)?;
    if device.write_buffer(vertex_buffer, 0, &vertices)? != vertices.len()
        || device.write_buffer(index_buffer, 0, &indices)? != indices.len()
    {
        return Err(ERR_IO);
    }
    let mut pixels = vec![0u8; rgba_bytes];
    let mut batch = IndexedDrawBatchV2 {
        clear_rgba8_srgb: u32::from_le_bytes([255, 255, 255, 255]),
        draw_count: 1,
        ..Default::default()
    };
    batch.draws[0] = IndexedBatchDrawV2 {
        index_count: (indices.len() / 4) as u32,
        first_index: 0,
        base_vertex: 0,
        rgba8_srgb: u32::from_le_bytes([255, 0, 255, 255]),
        topology: PRIMITIVE_TOPOLOGY_LINE_LIST,
        reserved: 0,
    };
    // Busy means the renderer accepted no work. Retry the same immutable
    // batch; each image has exactly one successful native submission.
    let point = loop {
        match device.submit_offscreen_indexed_batch_v1(
            queue,
            output,
            width,
            height,
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
    Ok(super::png::png_rgba8(width, height, &pixels))
}
