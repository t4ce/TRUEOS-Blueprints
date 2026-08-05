#![no_std]

mod config;

use trueos::input::{KEYBOARD_KEY_ESCAPE, KEYBOARD_OUTPUT_FLAG_PRESS, KEYBOARD_OUTPUT_KIND_KEY};
use trueos::ui4_scene::{
    CloseRequest, Damage, Error, Font, FontCanvasRow, Frame, MenuEntry, output_dimensions, rgba,
    worker_slot,
};
use trueos::{logl, vsys};

/// What the context menu acts on.
struct Canvas {
    /// Set once FontKernel has handed back the warm buffer.
    stamped: bool,
    /// Set by the "clear" row. The stamp stays warm; it is simply not composed.
    cleared: bool,
}

impl Canvas {
    const fn new() -> Self {
        Self {
            stamped: false,
            cleared: false,
        }
    }

    fn clear(&mut self) {
        self.cleared = true;
        logl::log(logl::level::INFO, "grid: context menu clear; frame empty");
    }

    const fn shows_stamp(&self) -> bool {
        self.stamped && !self.cleared
    }
}

/// The frame's whole menu: one row, one handler.
const MENU: [MenuEntry<'static, Canvas>; 1] = [MenuEntry::new("clear", Canvas::clear)];

fn main() {
    // The hypervisor gives every live instance its own worker lane, so an
    // instance can place itself without being told who it is. Lanes start
    // above the reserved BSP/UI lanes, so the lowest observed lane is treated
    // as tile zero.
    const FIRST_VM_LANE: u32 = 2;
    let lane = worker_slot();
    let index = lane.saturating_sub(FIRST_VM_LANE);

    let output = output_dimensions().unwrap_or(config::FALLBACK_OUTPUT);
    let tile = config::tile_for(index, output);
    let id = index.saturating_add(1);

    logl::log(
        logl::level::INFO,
        format_args!(
            "grid: placing id={id} lane={lane} tile={}x{}@{},{} wall={}x{} output={}x{}",
            tile.width,
            tile.height,
            tile.x,
            tile.y,
            config::COLUMNS,
            config::ROWS,
            output.0,
            output.1
        ),
    );

    let mut width = tile.width;
    let mut height = tile.height;

    let Ok(mut frame) = Frame::open_streaming(tile.x, tile.y, width, height) else {
        logl::log(logl::level::ERROR, "grid: frame open failed");
        return;
    };
    logl::log(
        logl::level::INFO,
        format_args!(
            "grid: frame open id={id} window={} extent={}x{}@{},{} buffering=streaming-triple",
            frame.window_id(),
            width,
            height,
            tile.x,
            tile.y
        ),
    );

    // Pixels onto a plane before anything else is attempted.
    if let Err(error) = publish_empty(&mut frame, width, height) {
        fail("first publish", error);
        return;
    }
    let mut presented = false;

    // The frame owns the secondary-click gesture over its own pixels.
    if let Err(error) = frame.register_context_menu(&MENU) {
        fail("context menu register", error);
        return;
    }

    let mut canvas = Canvas::new();

    loop {
        vsys::poll_once();

        loop {
            match frame.take_resize_event() {
                Ok(Some(event)) => {
                    if event.width != width || event.height != height {
                        if let Err(error) = frame.resize(event.width, event.height) {
                            fail("resize", error);
                            return;
                        }
                        width = event.width;
                        height = event.height;
                        canvas.stamped = false;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    fail("resize event", error);
                    return;
                }
            }
        }

        if let Err(error) = frame.pump_context_menu(&MENU, &mut canvas) {
            fail("context menu pump", error);
            return;
        }

        loop {
            let event = match frame.take_keyboard_event() {
                Ok(Some(event)) => event,
                Ok(None) => break,
                Err(error) => {
                    fail("keyboard event", error);
                    return;
                }
            };
            if event.kind != KEYBOARD_OUTPUT_KIND_KEY
                || event.flags & KEYBOARD_OUTPUT_FLAG_PRESS == 0
            {
                continue;
            }
            if event.key_code == KEYBOARD_KEY_ESCAPE {
                if let Err(error) = frame.close(CloseRequest::default()) {
                    fail("frame close", error);
                }
                return;
            }
        }

        if !canvas.stamped {
            match stamp_id(&mut frame, id, width, height) {
                Ok(()) => {
                    canvas.stamped = true;
                    logl::log(
                        logl::level::INFO,
                        format_args!(
                            "grid: id stamp retained id={id} canvas={width}x{height} font=inconsolata color=white claim=released stamps=1"
                        ),
                    );
                }
                // Every executor in the pool is busy; ask again next iteration.
                Err(Error::Busy) => {}
                Err(error) => {
                    fail("id stamp", error);
                    return;
                }
            }
        }

        let published = if canvas.shows_stamp() {
            publish_stamp(&mut frame, width, height)
        } else {
            publish_empty(&mut frame, width, height)
        };
        if let Err(error) = published {
            fail("frame publish", error);
            return;
        }

        if !presented && let Ok(true) = frame.take_first_presentation() {
            presented = true;
            logl::log(
                logl::level::INFO,
                format_args!(
                    "grid: first presentation id={id} window={} extent={}x{}",
                    frame.window_id(),
                    width,
                    height
                ),
            );
        }
    }
}

/// Stamp this instance's id in white across the top of the tile.
fn stamp_id(frame: &mut Frame, id: u32, width: u32, height: u32) -> Result<(), Error> {
    let mut digits = [0u8; 10];
    let text = format_u32(id, &mut digits);

    let font_pixels = (height as f32 * config::ID_HEIGHT_FRACTION)
        .clamp(config::ID_MIN_PIXELS, config::ID_MAX_PIXELS);
    let advance = text.len() as f32 * config::INCONSOLATA_ADVANCE_EM * font_pixels;
    // `SceneOrigin` positioning puts the baseline one em below the given y, so
    // the row position is the top-left of the em box.
    let row = FontCanvasRow {
        text,
        x: (width as f32 - advance) * 0.5,
        y: config::ID_TOP_INSET_PX,
        font_pixels,
        color_rgba: rgba(
            config::ID_RGBA.0,
            config::ID_RGBA.1,
            config::ID_RGBA.2,
            config::ID_RGBA.3,
        ),
    };
    frame.retain_font_canvas(
        Font::Inconsolata,
        (width, height),
        core::slice::from_ref(&row),
    )
}

/// Render `value` into `buffer`, returning the populated decimal slice.
fn format_u32(value: u32, buffer: &mut [u8; 10]) -> &str {
    if value == 0 {
        buffer[0] = b'0';
        return core::str::from_utf8(&buffer[..1]).expect("ascii digit");
    }
    let mut digits = 0usize;
    let mut remaining = value;
    while remaining != 0 {
        buffer[digits] = b'0' + (remaining % 10) as u8;
        remaining /= 10;
        digits += 1;
    }
    buffer[..digits].reverse();
    core::str::from_utf8(&buffer[..digits]).expect("ascii digits")
}

/// Publish one opaque clear-colour frame. `Busy` is ordinary backpressure and
/// is waited out; skipping it is how a frame never reaches a plane at all.
fn publish_empty(frame: &mut Frame, width: u32, height: u32) -> Result<(), Error> {
    let clear = clear_rgba();
    loop {
        match frame.begin(clear) {
            Ok(()) => break,
            Err(Error::Busy) => yield_once(),
            Err(error) => return Err(error),
        }
    }
    loop {
        match frame.publish(Damage::full(width, height)) {
            Ok(()) => return Ok(()),
            Err(Error::Busy) => yield_once(),
            Err(error) => return Err(error),
        }
    }
}

/// Compose the warm ID canvas over the clear colour and publish it.
fn publish_stamp(frame: &mut Frame, width: u32, height: u32) -> Result<(), Error> {
    let clear = clear_rgba();
    loop {
        match frame.present_font_canvas_view((width, height), (0, 0), clear) {
            Ok(()) => return Ok(()),
            Err(Error::Busy) => yield_once(),
            Err(error) => return Err(error),
        }
    }
}

fn clear_rgba() -> u32 {
    rgba(
        config::CLEAR_RGBA.0,
        config::CLEAR_RGBA.1,
        config::CLEAR_RGBA.2,
        config::CLEAR_RGBA.3,
    )
}

fn yield_once() {
    vsys::poll_once();
    vsys::sleep_ms(1);
}

fn fail(stage: &str, error: Error) {
    logl::log(
        logl::level::ERROR,
        format_args!("grid: {stage} failed: {error:?}"),
    );
}
