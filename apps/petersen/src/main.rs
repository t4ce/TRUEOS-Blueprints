#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use core::panic::PanicInfo;
use trueos::ui2::{self, gfx};
use trueos::platform;
use trueos::{panic_abort, TrueosAllocator};

#[global_allocator]
static GLOBAL_ALLOCATOR: TrueosAllocator = TrueosAllocator;

const WINDOW_TITLE: &str = "Petersen BP";
const WINDOW_X: i32 = 640;
const WINDOW_Y: i32 = 140;
const WINDOW_WIDTH: u32 = 280;
const WINDOW_HEIGHT: u32 = 280;
const TEX_ID: u32 = 4_760;
const BG: [u8; 4] = [0xF3, 0xF3, 0xF3, 0xFF];
const EDGE: [u8; 4] = [0x15, 0x15, 0x15, 0xFF];
const OUTLINE: [u8; 4] = [0x10, 0x10, 0x10, 0xFF];
const RED: [u8; 4] = [0xFF, 0x25, 0x1E, 0xFF];
const BLUE: [u8; 4] = [0x2E, 0x73, 0xFF, 0xFF];
const GREEN: [u8; 4] = [0x12, 0xF0, 0x22, 0xFF];

#[derive(Copy, Clone)]
struct Point {
    x: i32,
    y: i32,
}

impl Point {
    const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    panic_abort("petersen bp: panic\n")
}

fn put_pixel(pixels: &mut [u8], width: u32, height: u32, x: i32, y: i32, rgba: [u8; 4]) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    let offset = ((y as usize) * (width as usize) + (x as usize)) * 4;
    if let Some(px) = pixels.get_mut(offset..offset + 4) {
        px.copy_from_slice(&rgba);
    }
}

fn draw_disc(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    center: Point,
    radius: i32,
    rgba: [u8; 4],
) {
    let r2 = radius * radius;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy <= r2 {
                put_pixel(pixels, width, height, center.x + dx, center.y + dy, rgba);
            }
        }
    }
}

fn draw_line(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    a: Point,
    b: Point,
    radius: i32,
    rgba: [u8; 4],
) {
    let mut x0 = a.x;
    let mut y0 = a.y;
    let dx = (b.x - x0).abs();
    let sx = if x0 < b.x { 1 } else { -1 };
    let dy = -(b.y - y0).abs();
    let sy = if y0 < b.y { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        draw_disc(pixels, width, height, Point::new(x0, y0), radius, rgba);
        if x0 == b.x && y0 == b.y {
            break;
        }
        let e2 = err * 2;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

fn render_petersen(width: u32, height: u32) -> Vec<u8> {
    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    for px in pixels.chunks_exact_mut(4) {
        px.copy_from_slice(&BG);
    }

    let outer = [
        Point::new(140, 22),
        Point::new(253, 104),
        Point::new(210, 236),
        Point::new(70, 236),
        Point::new(27, 104),
    ];
    let inner = [
        Point::new(140, 76),
        Point::new(191, 114),
        Point::new(171, 174),
        Point::new(109, 174),
        Point::new(89, 114),
    ];

    for idx in 0..5 {
        draw_line(pixels.as_mut_slice(), width, height, outer[idx], outer[(idx + 1) % 5], 2, EDGE);
        draw_line(pixels.as_mut_slice(), width, height, inner[idx], inner[(idx + 2) % 5], 2, BLUE);
        draw_line(pixels.as_mut_slice(), width, height, outer[idx], inner[idx], 2, GREEN);
    }

    for point in outer {
        draw_disc(pixels.as_mut_slice(), width, height, point, 9, OUTLINE);
        draw_disc(pixels.as_mut_slice(), width, height, point, 7, RED);
    }
    for point in inner {
        draw_disc(pixels.as_mut_slice(), width, height, point, 8, OUTLINE);
        draw_disc(pixels.as_mut_slice(), width, height, point, 6, BLUE);
    }

    pixels
}

#[unsafe(no_mangle)]
pub extern "C" fn main() {
    let Some(window) = ui2::SurfaceWindow::create(
        WINDOW_TITLE,
        ui2::Rect {
            x: WINDOW_X,
            y: WINDOW_Y,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        },
        TEX_ID,
    ) else {
        platform::log_error("petersen bp: window create failed\n");
        return;
    };

    let pixels = render_petersen(WINDOW_WIDTH, WINDOW_HEIGHT);
    let ok = gfx::upload_texture_rgba_image_now(TEX_ID, WINDOW_WIDTH, WINDOW_HEIGHT, &pixels);
    if !ok {
        platform::log_error("petersen bp: texture upload failed\n");
        return;
    }

    let _ = window.id().request_repaint();
    loop {
        platform::poll_once();
    }
}
