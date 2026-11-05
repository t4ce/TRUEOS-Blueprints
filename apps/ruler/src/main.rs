#![no_std]

extern crate alloc;

use alloc::{vec, vec::Vec};
use core::f32::consts::PI;
use ruler::{Interaction, Metrics, Point, Ruler, Scene, View};
use trueos::ui4_scene::{
    self, CursorSource, Damage, Error, Frame, POINTER_BUTTON_PRIMARY, SpriteCorner, SpriteQuad,
    rgba,
};
use trueos::{input, logl, vsys};

const MAX_WIDTH: u32 = 2560;
const MAX_HEIGHT: u32 = 1440;
const ATLAS_SPRITE: u32 = 1;
const ATLAS_COLUMNS: usize = 16;
const CELL_WIDTH: usize = 7;
const CELL_HEIGHT: usize = 9;
const SHIFT_MODIFIERS: u8 = 0x22;

#[derive(Clone, Copy)]
enum GestureKind {
    Rotate { heading: f32, angle: f32 },
    Measure,
}

#[derive(Clone, Copy)]
struct Gesture {
    source: CursorSource,
    /// Screen pixels stay fixed while the backing frame changes its bounds.
    pivot: Point,
    kind: GestureKind,
}

fn main() {
    if let Err(error) = run()
        && error != Error::NotFound
    {
        logl::log(
            logl::level::ERROR,
            format_args!("ruler: stopped error={error:?}"),
        );
    }
}

fn run() -> Result<(), Error> {
    let mut output = ui4_scene::output_dimensions()?;
    let mut physical = ui4_scene::physical_extent(1000, 1000)?;
    let metrics = physical_metrics(physical)?;
    let mut limits = bounded_extent(output);
    let mut ruler = Ruler::new(metrics, limits.0, limits.1);
    let mut view = ruler.view();
    let mut frame = Frame::open(
        (output.0 as i32 - view.width as i32) / 2,
        (output.1 as i32 - view.height as i32) / 2,
        view.width,
        view.height,
    )?;
    frame.set_title("Ruler")?;
    frame.set_primary_activation(true)?;
    upload_atlas(&mut frame)?;
    logl::log(
        logl::level::INFO,
        format_args!(
            "ruler: ready pixels_per_mm={:.4},{:.4} physical_scale=output-metrics controls=primary-handles secondary-drag wheel-size U-units M/P-mode A-angle-units Home-reset arrows-rotate Shift-snap",
            metrics.px_per_mm_x, metrics.px_per_mm_y,
        ),
    );

    let mut gesture: Option<Gesture> = None;
    let mut dirty = true;
    let mut display_check = 0u32;
    loop {
        // No event handling or resize can acquire a second buffer while paint
        // is retrying publication of the current completed lease.
        if dirty {
            let pivot = match gesture {
                Some(drag) => drag.pivot,
                None => screen_pivot(&frame, &view)?,
            };
            paint(&mut frame, &ruler, &mut view, pivot)?;
            dirty = false;
        }

        match frame.take_resize_event() {
            Ok(Some(event)) => {
                limits = bounded_extent((event.width.min(output.0), event.height.min(output.1)));
                ruler.fit_limits(limits.0, limits.1);
                dirty = true;
            }
            Ok(None) | Err(Error::Busy) => {}
            Err(error) => return Err(error),
        }

        while let Some(event) = frame.take_keyboard_event()? {
            let snap = event.modifiers & SHIFT_MODIFIERS != 0;
            if event.kind == input::KEYBOARD_OUTPUT_KIND_TEXT {
                let changed = match char::from_u32(event.codepoint) {
                    Some('u' | 'U') => {
                        ruler.cycle_unit();
                        true
                    }
                    Some('m' | 'M' | 'p' | 'P') => {
                        ruler.toggle_mode();
                        true
                    }
                    Some('a' | 'A') => {
                        ruler.toggle_angle_unit();
                        true
                    }
                    Some('+' | '=') => ruler.scroll(1),
                    Some('-' | '_') => ruler.scroll(-1),
                    _ => false,
                };
                if changed {
                    gesture = None;
                    dirty = true;
                }
            } else if event.kind == input::KEYBOARD_OUTPUT_KIND_KEY
                && event.flags & input::KEYBOARD_OUTPUT_FLAG_PRESS != 0
            {
                match event.key_code {
                    input::KEYBOARD_KEY_HOME => {
                        ruler.reset();
                        gesture = None;
                        dirty = true;
                    }
                    input::KEYBOARD_KEY_ARROW_LEFT | input::KEYBOARD_KEY_ARROW_RIGHT => {
                        let direction = if event.key_code == input::KEYBOARD_KEY_ARROW_LEFT {
                            -1.0
                        } else {
                            1.0
                        };
                        let step = if snap { PI / 12.0 } else { PI / 180.0 };
                        ruler.set_angle(ruler.angle + direction * step, snap);
                        gesture = None;
                        dirty = true;
                    }
                    _ => {}
                }
            }
        }

        while let Some(event) = frame.take_pointer_event()? {
            let local = Point::new(event.local_x as f32, event.local_y as f32);
            if event.wheel != 0 {
                dirty |= ruler.scroll(event.wheel);
            }
            if event.buttons_pressed & POINTER_BUTTON_PRIMARY != 0 && gesture.is_none() {
                let pivot = screen_pivot(&frame, &view)?;
                let kind = match ruler.press(&view, local) {
                    Interaction::None => None,
                    Interaction::Changed => {
                        dirty = true;
                        None
                    }
                    Interaction::Rotate => Some(GestureKind::Rotate {
                        heading: ruler.pointer_angle(&view, local),
                        angle: ruler.angle,
                    }),
                    Interaction::Measure => {
                        dirty = true;
                        Some(GestureKind::Measure)
                    }
                };
                if let Some(kind) = kind {
                    gesture = Some(Gesture {
                        source: event.source,
                        pivot,
                        kind,
                    });
                }
            }
            if let Some(drag) = gesture
                && drag.source == event.source
            {
                // The event's global position is authoritative. Frame-local
                // coordinates can belong to the previous size during a drag.
                let point = Point::new(
                    view.center.x + event.x as f32 - drag.pivot.x,
                    view.center.y + event.y as f32 - drag.pivot.y,
                );
                if event.buttons_down & POINTER_BUTTON_PRIMARY != 0
                    || event.buttons_released & POINTER_BUTTON_PRIMARY != 0
                {
                    let heading = ruler.pointer_angle(&view, point);
                    let snap = match frame.keyboard_state() {
                        Ok(Some(keys)) => keys.modifiers & SHIFT_MODIFIERS != 0,
                        Ok(None) | Err(Error::Busy) => false,
                        Err(error) => return Err(error),
                    };
                    match drag.kind {
                        GestureKind::Rotate {
                            heading: start_heading,
                            angle,
                        } => {
                            let delta = heading - start_heading;
                            let delta = libm::atan2f(libm::sinf(delta), libm::cosf(delta));
                            ruler.set_angle(angle + delta, snap);
                        }
                        GestureKind::Measure => ruler.set_measure(heading, snap),
                    }
                    dirty = true;
                }
                if event.buttons_down & POINTER_BUTTON_PRIMARY == 0 {
                    gesture = None;
                }
            }
        }

        // Output density can change without a window resize. A long physical
        // sample minimizes integer conversion error and keeps both axes exact.
        display_check += 1;
        if display_check >= 120 {
            display_check = 0;
            let next_output = ui4_scene::output_dimensions()?;
            let next_physical = ui4_scene::physical_extent(1000, 1000)?;
            if next_output != output || next_physical != physical {
                output = next_output;
                physical = next_physical;
                limits = bounded_extent(output);
                ruler.refresh_metrics(physical_metrics(physical)?);
                ruler.fit_limits(limits.0, limits.1);
                gesture = None;
                dirty = true;
            }
        }
        vsys::poll_once();
        vsys::sleep_ms(if dirty { 1 } else { 16 });
    }
}

fn bounded_extent((width, height): (u32, u32)) -> (u32, u32) {
    (width.clamp(1, MAX_WIDTH), height.clamp(1, MAX_HEIGHT))
}

fn physical_metrics((width, height): (u32, u32)) -> Result<Metrics, Error> {
    Metrics::new(width as f32 / 1000.0, height as f32 / 1000.0).ok_or(Error::Ui4)
}

fn screen_pivot(frame: &Frame, view: &View) -> Result<Point, Error> {
    let (x, y) = retry(|| frame.position())?;
    Ok(Point::new(
        x as f32 + view.center.x,
        y as f32 + view.center.y,
    ))
}

fn paint(frame: &mut Frame, ruler: &Ruler, previous: &mut View, pivot: Point) -> Result<(), Error> {
    let view = ruler.view();
    if view.width > MAX_WIDTH || view.height > MAX_HEIGHT {
        return Err(Error::Invalid);
    }
    if (frame.width(), frame.height()) != (view.width, view.height) {
        retry(|| frame.resize(view.width, view.height))?;
    }
    let x = libm::roundf(pivot.x - view.center.x) as i32;
    let y = libm::roundf(pivot.y - view.center.y) as i32;
    retry(|| frame.set_position(x, y))?;
    let scene = ruler.scene(&view);
    let quads = scene_quads(&scene)?;
    loop {
        retry(|| frame.begin_sprite_frame(rgba(0, 0, 0, 0)))?;
        match frame.draw_sprite_quads(&quads) {
            // Draw admission Busy cancels the lease. Start a fresh frame;
            // uploaded sources remain retained by this window.
            Err(Error::Busy) => {
                yield_briefly();
                continue;
            }
            Err(error) => return Err(error),
            Ok(()) => {}
        }
        // Publication Busy retains the completed lease. Retry only publish.
        retry(|| frame.publish(Damage::full(view.width, view.height)))?;
        *previous = view;
        return Ok(());
    }
}

fn retry<T>(mut operation: impl FnMut() -> Result<T, Error>) -> Result<T, Error> {
    loop {
        match operation() {
            Err(Error::Busy) => yield_briefly(),
            result => return result,
        }
    }
}

fn yield_briefly() {
    vsys::poll_once();
    vsys::sleep_ms(1);
}

fn scene_quads(scene: &Scene) -> Result<Vec<SpriteQuad>, Error> {
    let mut quads = Vec::with_capacity(scene.quads.len() + scene.labels.len() * 6);
    for quad in &scene.quads {
        quads.push(SpriteQuad {
            sprite_id: 0,
            c0: corner(quad.corners[0], 0.0, 0.0),
            c1: corner(quad.corners[1], 0.0, 0.0),
            c2: corner(quad.corners[2], 0.0, 0.0),
            c3: corner(quad.corners[3], 0.0, 0.0),
            color_rgba: quad.color,
            source_over: true,
        });
    }
    let atlas_width = (ATLAS_COLUMNS * CELL_WIDTH) as f32;
    let atlas_height = atlas_height() as f32;
    for label in &scene.labels {
        let scale = label.font_pixels / CELL_HEIGHT as f32;
        let count = label.text.chars().count();
        if count == 0 {
            continue;
        }
        let visible_width = (count as f32 * 6.0 - 1.0) * scale;
        let left = label.center.x - visible_width * 0.5;
        let top = label.center.y - label.font_pixels * 0.5;
        for (offset, scalar) in label.text.chars().enumerate() {
            if scalar == ' ' {
                continue;
            }
            let Some(slot) = GLYPHS.iter().position(|(ch, _)| *ch == scalar) else {
                logl::log(
                    logl::level::ERROR,
                    format_args!("ruler: unsupported label glyph={scalar:?}"),
                );
                return Err(Error::Font);
            };
            let x = left + offset as f32 * 6.0 * scale - scale;
            let u0 = (slot % ATLAS_COLUMNS * CELL_WIDTH) as f32 / atlas_width;
            let v0 = (slot / ATLAS_COLUMNS * CELL_HEIGHT) as f32 / atlas_height;
            let u1 = u0 + CELL_WIDTH as f32 / atlas_width;
            let v1 = v0 + CELL_HEIGHT as f32 / atlas_height;
            let right = x + CELL_WIDTH as f32 * scale;
            let bottom = top + label.font_pixels;
            // Only label centers follow the physical ruler transform. Glyph
            // corners stay in screen coordinates at every ruler orientation.
            quads.push(SpriteQuad {
                sprite_id: ATLAS_SPRITE,
                c0: corner(Point::new(x, top), u0, v0),
                c1: corner(Point::new(right, top), u1, v0),
                c2: corner(Point::new(right, bottom), u1, v1),
                c3: corner(Point::new(x, bottom), u0, v1),
                color_rgba: label.color,
                source_over: true,
            });
        }
    }
    Ok(quads)
}

fn corner(point: Point, u: f32, v: f32) -> SpriteCorner {
    SpriteCorner {
        x: point.x,
        y: point.y,
        u,
        v,
    }
}

fn atlas_height() -> usize {
    GLYPHS.len().div_ceil(ATLAS_COLUMNS) * CELL_HEIGHT
}

fn upload_atlas(frame: &mut Frame) -> Result<(), Error> {
    let width = ATLAS_COLUMNS * CELL_WIDTH;
    let height = atlas_height();
    let mut pixels = vec![255u8; width * height * 4];
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[3] = 0;
    }
    for (slot, (_, rows)) in GLYPHS.iter().enumerate() {
        let left = slot % ATLAS_COLUMNS * CELL_WIDTH + 1;
        let top = slot / ATLAS_COLUMNS * CELL_HEIGHT + 1;
        for (y, row) in rows.iter().enumerate() {
            for x in 0..5 {
                let offset = ((top + y) * width + left + x) * 4;
                pixels[offset + 3] = if row & (1 << (4 - x)) != 0 { 255 } else { 0 };
            }
        }
    }
    // A single uncoloured retained atlas serves every size and colour. The
    // native sampler scales coverage; source-over applies each label's ink.
    retry(|| frame.upload_sprite_rgba8(ATLAS_SPRITE, width as u32, height as u32, &pixels))
}

// Deliberately bounded alphabet for all ruler labels, including true lowercase
// units and the pi/degree symbols. Each tile has transparent sampling gutters.
const GLYPHS: &[(char, [u8; 7])] = &[
    ('0', [14, 17, 19, 21, 25, 17, 14]),
    ('1', [4, 12, 4, 4, 4, 4, 14]),
    ('2', [14, 17, 1, 2, 4, 8, 31]),
    ('3', [30, 1, 1, 14, 1, 1, 30]),
    ('4', [2, 6, 10, 18, 31, 2, 2]),
    ('5', [31, 16, 16, 30, 1, 1, 30]),
    ('6', [6, 8, 16, 30, 17, 17, 14]),
    ('7', [31, 1, 2, 4, 8, 8, 8]),
    ('8', [14, 17, 17, 14, 17, 17, 14]),
    ('9', [14, 17, 17, 15, 1, 2, 12]),
    ('R', [30, 17, 17, 30, 20, 18, 17]),
    ('U', [17, 17, 17, 17, 17, 17, 14]),
    ('L', [16, 16, 16, 16, 16, 16, 31]),
    ('E', [31, 16, 16, 30, 16, 16, 31]),
    ('r', [0, 0, 22, 25, 16, 16, 16]),
    ('c', [0, 0, 14, 17, 16, 17, 14]),
    ('m', [0, 0, 26, 21, 21, 21, 21]),
    ('i', [4, 0, 12, 4, 4, 4, 14]),
    ('n', [0, 0, 30, 17, 17, 17, 17]),
    ('-', [0, 0, 0, 31, 0, 0, 0]),
    ('+', [0, 4, 4, 31, 4, 4, 0]),
    ('.', [0, 0, 0, 0, 0, 12, 12]),
    ('/', [1, 2, 2, 4, 8, 8, 16]),
    ('°', [6, 9, 9, 6, 0, 0, 0]),
    ('π', [0, 0, 31, 10, 10, 10, 11]),
];
