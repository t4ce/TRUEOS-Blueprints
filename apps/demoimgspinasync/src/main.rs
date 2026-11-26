use std::f32::consts::TAU;
use trueos::input::{KEYBOARD_KEY_SPACE, KEYBOARD_OUTPUT_FLAG_PRESS, KEYBOARD_OUTPUT_KIND_KEY};
use trueos::ui4_scene::{Damage, Error, Frame, output_dimensions};
use trueos::ui4_winit::{SceneTarget, SpriteBackend, SpriteCommand, SpriteCorner, SpriteQuad};
use trueos::{clock, logl, vsys};

const SIDE: u32 = 512;
const HZ: u32 = 30;
const SECOND_NS: u64 = 1_000_000_000;
const FRAME_NS: u64 = SECOND_NS / HZ as u64;
const REVOLUTION_NS: u64 = 120 * SECOND_NS;
const SOURCE_ID: u32 = 1;
const TEXTURE_SIDE: usize = 512;
const SUN: &[u8; TEXTURE_SIDE * TEXTURE_SIDE * 4] = include_bytes!(env!("DEMO_SUN_TEXTURE"));
const TEXTURE_CENTER: f32 = (TEXTURE_SIDE - 1) as f32 * 0.5;
const TEXTURE_PIXELS_PER_OUTPUT_PIXEL: f32 = 1.10;

fn main() {
    if let Err(error) = run()
        && error != Error::NotFound
    {
        logl::log(
            logl::level::ERROR,
            format_args!("demoimgspinasync stopped error={error:?}"),
        );
    }
}

fn run() -> Result<(), Error> {
    let (output_width, _) = output_dimensions()?;
    let x = 2032.min(output_width.saturating_sub(SIDE)) as i32;
    let mut frame = Frame::open_visual(x, 72, SIDE, SIDE, HZ)?;
    let mut scene = SceneTarget::for_window(frame.window_id(), SIDE, SIDE)?;
    let mut pixels = vec![0; SIDE as usize * SIDE as usize * 4];
    let copy = viewport_copy();
    let mut next = clock::monotonic_nanos();
    let mut report_from = next;
    let mut last_pose_at = next;
    let mut rotation_ns = 0;
    let mut spinning = true;
    let mut published = 0;
    let mut max_prepare_us = 0;
    let mut max_submit_us = 0;

    logl::log(
        logl::level::IMPORTANT,
        "demoimgspinasync ready=512x512 hz=30",
    );
    loop {
        let now = clock::monotonic_nanos();
        if now < next {
            vsys::sleep_ms((next - now).div_ceil(1_000_000));
            continue;
        }
        next += FRAME_NS;
        if spinning {
            rotation_ns = (rotation_ns + now - last_pose_at) % REVOLUTION_NS;
        }
        last_pose_at = now;
        while let Some(event) = frame.take_keyboard_event()? {
            if event.kind == KEYBOARD_OUTPUT_KIND_KEY
                && event.flags & KEYBOARD_OUTPUT_FLAG_PRESS != 0
                && event.key_code == KEYBOARD_KEY_SPACE
            {
                spinning = !spinning;
                logl::log(
                    logl::level::IMPORTANT,
                    format_args!("demoimgspinasync spinning={spinning}"),
                );
            }
        }
        if spinning {
            let prepare_at = clock::monotonic_nanos();
            let radians = TAU * rotation_ns as f32 / REVOLUTION_NS as f32;
            render(&mut pixels, radians);
            let submit_at = clock::monotonic_nanos();
            max_prepare_us = max_prepare_us.max((submit_at - prepare_at) / 1000);
            scene.upload_sprite_rgba8(SOURCE_ID, SIDE, SIDE, &pixels)?;
            loop {
                wait_ready(|| frame.begin_visual_gpu_frame())?;
                match scene.draw_sprite_commands(std::slice::from_ref(&copy)) {
                    Ok(()) => break,
                    Err(Error::Busy) => vsys::sleep_ms(1),
                    Err(error) => return Err(error),
                }
            }
            wait_ready(|| frame.publish(Damage::full(SIDE, SIDE)))?;
            published += 1;
            max_submit_us = max_submit_us.max((clock::monotonic_nanos() - submit_at) / 1000);
        }
        let now = clock::monotonic_nanos();
        let elapsed = now - report_from;
        if elapsed >= 5 * SECOND_NS {
            let fps = f64::from(published) * SECOND_NS as f64 / elapsed as f64;
            logl::log(
                logl::level::INFO,
                format_args!(
                    "demoimgspinasync fps={fps:.1} frames={published} prepare_max_us={max_prepare_us} submit_max_us={max_submit_us} spinning={spinning}"
                ),
            );
            report_from = now;
            published = 0;
            max_prepare_us = 0;
            max_submit_us = 0;
        }
        if next <= now {
            next = now + FRAME_NS;
        }
    }
}

fn wait_ready(mut operation: impl FnMut() -> Result<(), Error>) -> Result<(), Error> {
    loop {
        match operation() {
            Err(Error::Busy) => vsys::sleep_ms(1),
            outcome => return outcome,
        }
    }
}

fn viewport_copy() -> SpriteCommand {
    let side = SIDE as f32;
    let corner = |x, y, u, v| SpriteCorner { x, y, u, v };
    SpriteCommand {
        quad: SpriteQuad {
            sprite_id: SOURCE_ID,
            c0: corner(0.0, 0.0, 0.0, 0.0),
            c1: corner(side, 0.0, 1.0, 0.0),
            c2: corner(side, side, 1.0, 1.0),
            c3: corner(0.0, side, 0.0, 1.0),
            color_rgba: u32::MAX,
            source_over: false,
        },
        backend: SpriteBackend::Bcs0Overwrite,
    }
}

fn render(pixels: &mut [u8], radians: f32) {
    let (sin, cos) = radians.sin_cos();
    let pitch = SIDE as usize * 4;
    let center = (SIDE - 1) as f32 * 0.5;
    for (y, row) in pixels.chunks_exact_mut(pitch).enumerate() {
        let dy = (y as f32 - center) * TEXTURE_PIXELS_PER_OUTPUT_PIXEL;
        let base_x = TEXTURE_CENTER - dy * sin;
        let base_y = TEXTURE_CENTER + dy * cos;
        for (x, pixel) in row.chunks_exact_mut(4).enumerate() {
            let dx = (x as f32 - center) * TEXTURE_PIXELS_PER_OUTPUT_PIXEL;
            pixel.copy_from_slice(&sample_sun(base_x + dx * cos, base_y + dx * sin));
        }
    }
}

fn sample_sun(x: f32, y: f32) -> [u8; 4] {
    let last_texel = TEXTURE_SIDE - 1;
    if x < 0.0 || y < 0.0 || x > last_texel as f32 || y > last_texel as f32 {
        return [0; 4];
    }
    let (left, top) = (x as usize, y as usize);
    let (right, bottom) = ((left + 1).min(last_texel), (top + 1).min(last_texel));
    let (tx, ty) = (x - left as f32, y - top as f32);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let texel = |x, y, channel| f32::from(SUN[(y * TEXTURE_SIDE + x) * 4 + channel]);
    std::array::from_fn(|channel| {
        let upper = lerp(texel(left, top, channel), texel(right, top, channel), tx);
        let lower = lerp(
            texel(left, bottom, channel),
            texel(right, bottom, channel),
            tx,
        );
        (lerp(upper, lower, ty) + 0.5) as u8
    })
}
