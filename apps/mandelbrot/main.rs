#![no_std]
#![no_main]

use core::panic::PanicInfo;
use trueos::{TrueosAllocator, panic_abort};
use trueos::{ui2, vsys};

#[global_allocator]
static GLOBAL_ALLOCATOR: TrueosAllocator = TrueosAllocator;

const UI2_MANDELBROT_TEX_ID: u32 = 4_702;
const UI2_MANDELBROT_RT_W: u32 = 768;
const UI2_MANDELBROT_RT_H: u32 = 512;
const UI2_MANDELBROT_WINDOW_Z: i32 = 31;
const FRAME_MS: u64 = 33;
const TICK_HZ: u64 = 1_000;

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    panic_abort("mandelbrot bp: panic\n")
}

#[unsafe(no_mangle)]
pub extern "C" fn main() {
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
        vsys::log_error("mandelbrot bp: ui2 surface window create failed\n");
        return;
    };

    let _ = surface.id().set_title("Seahorse Valley");
    vsys::sleep_ms(1);

    let mut ticks = 0u64;
    loop {
        if !surface.render_mandelbrot(ticks, TICK_HZ) {
            let _ = surface.id().set_title("Seahorse Valley (unavailable)");
            vsys::log_error("mandelbrot bp: render queue failed\n");
            break;
        }
        ticks = ticks.saturating_add(FRAME_MS);
        vsys::sleep_ms(FRAME_MS);
    }
}
