#![no_std]
extern crate alloc;
use alloc::{format, vec, vec::Vec};
use gridpape::Editor;
use trueos::ui4_scene::{SpriteCorner, SpriteQuad};
use trueos::ui4_winit::{SpriteBackend, SpriteCommand};
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
    // Consume the one-shot runscript before any UI4 window can become visible.
    let scale = match trueos::async_fs::block_on(trueos::async_fs::read_file(b"vFile:launch")) {
        Ok(bytes) => {
            let script = core::str::from_utf8(&bytes).map_err(|_| -22)?;
            gridpape::script_scale(script).map_err(|message| {
                trueos::logl::log(trueos::logl::level::ERROR, message);
                -22
            })?
        }
        Err(trueos::async_fs::ERR_NOT_FOUND) => 1,
        Err(error) => return Err(error),
    };
    let page = ui4_scene::physical_extent(210, 297).map_err(ui)?;
    // Warm only small complete atlases. Large sizes remain strictly demand-only.
    if scale <= 4 {
        let _ = glyph_pixels(scale, '?')?;
    }
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
        let mut editor = Editor::new(scale);
        let mut sprites = vec![GlyphSlot::default(); editor.cells.len()];
        let mut dirty = true;
        let mut visible = true;
        let mut blink = trueos::clock::Instant::now();
        loop {
            match frame.take_resize_event() {
                Ok(Some(event)) => {
                    frame.resize(event.width, event.height).map_err(ui)?;
                    dirty = true;
                }
                Ok(None) | Err(Error::Busy) => {}
                Err(Error::NotFound) => return Ok(()),
                Err(error) => return Err(ui(error)),
            }
            let mut input = false;
            while let Some(event) = frame.take_keyboard_event().map_err(ui)? {
                use trueos::input::*;
                if event.kind == KEYBOARD_OUTPUT_KIND_TEXT {
                    if let Some(ch) = char::from_u32(event.codepoint) {
                        editor.type_char(ch);
                        input = true;
                    }
                } else if event.kind == KEYBOARD_OUTPUT_KIND_KEY
                    && event.flags & KEYBOARD_OUTPUT_FLAG_PRESS != 0
                {
                    // Space is delivered as text by the keyboard translator.
                    if event.key_code != KEYBOARD_KEY_SPACE {
                        editor.key(event.key_code);
                        input = true;
                    }
                }
            }
            while let Some(event) = frame.take_pointer_event().map_err(ui)? {
                if event.buttons_pressed & ui4_scene::POINTER_BUTTON_PRIMARY != 0 {
                    input |= editor.click(event.local_x, event.local_y, page, scale);
                }
            }
            if input {
                visible = true;
                blink = trueos::clock::Instant::now();
                dirty = true;
            }
            if blink.elapsed().as_millis() >= 500 {
                visible = !visible;
                blink = trueos::clock::Instant::now();
                dirty = true;
            }
            if dirty {
                draw(
                    &mut frame,
                    page,
                    scale,
                    device,
                    queue,
                    pipeline,
                    &editor,
                    &mut sprites,
                    visible,
                )?;
                dirty = false;
            }
            trueos::vsys::sleep_ms(16);
        }
    })();
    let close = device.close();
    result.and(close)
}

fn draw(
    frame: &mut Frame,
    page: (u32, u32),
    scale: u32,
    device: Device,
    queue: Queue,
    pipeline: RenderPipeline,
    editor: &Editor,
    sprites: &mut [GlyphSlot],
    cursor_visible: bool,
) -> Result<(), i32> {
    for (index, cell) in editor.cells.iter().enumerate() {
        if let Some(ch) = cell.character
            && sprites[index].character != Some(ch)
        {
            let bytes = glyph_pixels(scale, ch)?;
            if bytes.len() < 8 {
                return Err(ERR_IO);
            }
            let width = u32::from_le_bytes(bytes[..4].try_into().unwrap());
            let height = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
            frame
                .upload_sprite_rgba8(index as u32 + 1, width, height, &bytes[8..])
                .map_err(ui)?;
            sprites[index] = GlyphSlot {
                character: Some(ch),
                width,
                height,
            };
        }
    }
    let (vertices, indices) = gridpape::geometry(page, (frame.width(), frame.height()), scale);
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
        index_count: (indices.len() / 4) as u32,
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
    let mut commands = Vec::with_capacity(editor.cells.len() + 1);
    for (index, cell) in editor.cells.iter().enumerate() {
        if cell.character.is_none() {
            continue;
        }
        let glyph = sprites[index];
        let (x, y, w, h) = editor.bounds(index, page, scale);
        // Centre the em box; crop its white border inside the grid lines.
        let left = x + w.saturating_sub(glyph.width) / 2;
        let top = y + h.saturating_sub(glyph.height) / 2;
        let inset = 1;
        if let Some(quad) = clipped_quad(
            index as u32 + 1,
            left,
            top,
            glyph.width,
            glyph.height,
            inset,
            frame.width(),
            frame.height(),
            u32::MAX,
        ) {
            commands.push(SpriteCommand {
                quad,
                backend: SpriteBackend::Bcs0,
            });
        }
    }
    if cursor_visible {
        let (x, y, w, h) = editor.bounds(editor.cursor, page, scale);
        if let Some(quad) = clipped_quad(
            0,
            x + w / 2,
            y + 2,
            1,
            h.saturating_sub(4),
            0,
            frame.width(),
            frame.height(),
            u32::from_le_bytes([0, 0, 0, 255]),
        ) {
            commands.push(SpriteCommand {
                quad,
                backend: SpriteBackend::Bcs0,
            });
        }
    }
    if !commands.is_empty() {
        loop {
            match frame.draw_sprite_commands(&commands) {
                Err(Error::Busy) => trueos::vsys::sleep_ms(16),
                result => {
                    result.map_err(ui)?;
                    break;
                }
            }
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

#[derive(Clone, Copy, Default)]
struct GlyphSlot {
    character: Option<char>,
    width: u32,
    height: u32,
}
fn glyph_pixels(scale: u32, ch: char) -> Result<Vec<u8>, i32> {
    let path = format!("vFile:microfont-grid/{scale}/{:x}", ch as u32);
    trueos::async_fs::block_on(trueos::async_fs::read_file(path.as_bytes()))
}
fn clipped_quad(
    id: u32,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    inset: u32,
    vw: u32,
    vh: u32,
    color: u32,
) -> Option<SpriteQuad> {
    if w <= 2 * inset || h <= 2 * inset || x + inset >= vw || y + inset >= vh {
        return None;
    }
    let left = x + inset;
    let top = y + inset;
    let right = (x + w - inset).min(vw);
    let bottom = (y + h - inset).min(vh);
    let corner = |px: u32, py: u32| SpriteCorner {
        x: px as f32,
        y: py as f32,
        u: (px - x) as f32 / w as f32,
        v: (py - y) as f32 / h as f32,
    };
    Some(SpriteQuad {
        sprite_id: id,
        c0: corner(left, top),
        c1: corner(right, top),
        c2: corner(right, bottom),
        c3: corner(left, bottom),
        color_rgba: color,
        source_over: false,
    })
}
