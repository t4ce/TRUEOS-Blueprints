#![no_std]

use trueos::logl::{self, level};
use trueos::{platform, ui2};

const UI2_MANDELBROT_TEX_ID: u32 = 4_702;
const UI2_MANDELBROT_RT_W: u32 = 768;
const UI2_MANDELBROT_RT_H: u32 = 512;
const UI2_MANDELBROT_WINDOW_Z: i32 = 31;
const FRAME_MS: u64 = 33;
const TICK_HZ: u64 = 1_000;

fn main() {
    let Some(surface) = ui2::SurfaceWindow::create_with_options(
        "Demo Mandelbrot",
        ui2::Rect {
            x: 10,
            y: 10,
            width: UI2_MANDELBROT_RT_W,
            height: UI2_MANDELBROT_RT_H,
        },
        ui2::CreateOptions {
            z: UI2_MANDELBROT_WINDOW_Z,
            alpha: 128,
        },
        UI2_MANDELBROT_TEX_ID,
        false,
    ) else {
        logl::log(level::ERROR, "mandelbrot bp: ui2 surface window create failed\n");
        return;
    };

    let _ = surface.id().set_title("Seahorse Valley");
    platform::sleep_ms(1);

    let mut ticks = 0u64;
    loop {
        if !surface.render_mandelbrot(ticks, TICK_HZ) {
            let _ = surface.id().set_title("Seahorse Valley (unavailable)");
            logl::log(level::ERROR, "mandelbrot bp: render queue failed\n");
            break;
        }
        ticks = ticks.saturating_add(FRAME_MS);
        platform::sleep_ms(FRAME_MS);
    }
}
