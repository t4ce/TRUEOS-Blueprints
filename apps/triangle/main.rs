#![no_std]
#![no_main]

use core::panic::PanicInfo;
use trueos::{TrueosAllocator, panic_abort};
use trueos::{ui2, vgfx, vsys};

#[global_allocator]
static GLOBAL_ALLOCATOR: TrueosAllocator = TrueosAllocator;

const WINDOW_TITLE: &str = "Triangle BP";
const WINDOW_X: i32 = 220;
const WINDOW_Y: i32 = 160;
const WINDOW_WIDTH: u32 = 384;
const WINDOW_HEIGHT: u32 = 240;
const TRIANGLE_TEX_ID: u32 = 4_700;
const CLEAR_RGB: u32 = 0x10141A;
const FRAME_MS: u64 = 33;
const STEP_COS: f32 = 0.9992001;
const STEP_SIN: f32 = 0.039989334;

#[derive(Copy, Clone)]
struct Point {
    x: f32,
    y: f32,
    color: [u8; 4],
}

impl Point {
    const fn new(x: f32, y: f32, color: [u8; 4]) -> Self {
        Self { x, y, color }
    }

    fn rotate_step(&mut self) {
        let next_x = (self.x * STEP_COS) - (self.y * STEP_SIN);
        let next_y = (self.x * STEP_SIN) + (self.y * STEP_COS);
        self.x = next_x;
        self.y = next_y;
    }

    const fn vertex(self) -> vgfx::RgbVertex {
        vgfx::RgbVertex::new(self.x, self.y, self.color)
    }
}

fn open_triangle_window() -> Option<ui2::SurfaceWindow> {
    ui2::SurfaceWindow::create(
        WINDOW_TITLE,
        ui2::Rect {
            x: WINDOW_X,
            y: WINDOW_Y,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        },
        TRIANGLE_TEX_ID,
    )
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    panic_abort("triangle bp: panic\n")
}

#[unsafe(no_mangle)]
pub extern "C" fn main() {
    let Some(window) = open_triangle_window() else {
        vsys::log_error("triangle bp: ui2 surface window create failed\n");
        return;
    };

    let mut points = [
        Point::new(0.0, -0.65, [0xFF, 0x52, 0x52, 0xFF]),
        Point::new(-0.7, 0.55, [0x40, 0xE3, 0x92, 0xFF]),
        Point::new(0.7, 0.55, [0x5A, 0x9C, 0xFF, 0xFF]),
    ];

    loop {
        let vertices = [points[0].vertex(), points[1].vertex(), points[2].vertex()];
        if !window.render_rgb_triangles(CLEAR_RGB, &vertices) {
            vsys::log_error("triangle bp: render failed\n");
            break;
        }
        for point in &mut points {
            point.rotate_step();
        }
        vsys::sleep_ms(FRAME_MS);
    }
}
