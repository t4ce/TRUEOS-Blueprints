#![no_std]
#![no_main]

extern crate alloc;

use alloc::{format, string::String, vec, vec::Vec};
use core::panic::PanicInfo;
use trueos::panic_abort;
use trueos::platform;
use trueos::ui2::{self, gfx};

// Axis settings - change these to adjust the visible range of the plot.
const AXIS_X_MIN: f64 = -2.0;
const AXIS_X_MAX: f64 = 2.0;
const AXIS_Y_MIN: f64 = -1.0;
const AXIS_Y_MAX: f64 = 1.0;
const TICK_STEP: f64 = 0.5;

const WINDOW_TITLE: &str = "Chart BP";
const WINDOW_X: i32 = 140;
const WINDOW_Y: i32 = 100;
const WINDOW_WIDTH: u32 = 480;
const WINDOW_HEIGHT: u32 = 320;
const TEX_ID: u32 = 4_721;

const BG_RGBA: [u8; 4] = [0x18, 0x1C, 0x24, 0xFF];
const AXIS_RGBA: [u8; 4] = [0x88, 0x98, 0xAA, 0xFF];
const TICK_RGBA: [u8; 4] = [0x55, 0x66, 0x77, 0xFF];
const GRID_RGBA: [u8; 4] = [0x28, 0x2E, 0x38, 0xFF];
const SINE_RGBA: [u8; 4] = [0x7F, 0xD1, 0xAE, 0xFF];
const LABEL_RGBA: [u8; 4] = [0xAA, 0xBB, 0xCC, 0xFF];

const MARGIN_LEFT: usize = 40;
const MARGIN_BOTTOM: usize = 20;
const MARGIN_TOP: usize = 8;
const MARGIN_RIGHT: usize = 8;
const LABEL_W: usize = 3;
const LABEL_H: usize = 5;
const LABEL_SCALE: usize = 2;
const LABEL_ADVANCE: usize = (LABEL_W + 1) * LABEL_SCALE;
const LABEL_LINE_HEIGHT: usize = LABEL_H * LABEL_SCALE;

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    panic_abort("chart bp: panic\n")
}

fn fill_rect(
    dst: &mut [u8],
    dst_w: usize,
    dst_h: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    rgba: [u8; 4],
) {
    let ey = y.saturating_add(h).min(dst_h);
    let ex = x.saturating_add(w).min(dst_w);
    for row in y.min(dst_h)..ey {
        for col in x.min(dst_w)..ex {
            let i = (row * dst_w + col) * 4;
            dst[i] = rgba[0];
            dst[i + 1] = rgba[1];
            dst[i + 2] = rgba[2];
            dst[i + 3] = rgba[3];
        }
    }
}

fn put_pixel(dst: &mut [u8], dst_w: usize, dst_h: usize, x: usize, y: usize, rgba: [u8; 4]) {
    if x < dst_w && y < dst_h {
        let i = (y * dst_w + x) * 4;
        dst[i] = rgba[0];
        dst[i + 1] = rgba[1];
        dst[i + 2] = rgba[2];
        dst[i + 3] = rgba[3];
    }
}

fn glyph_bits(ch: char) -> [u8; LABEL_H] {
    match ch {
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        '.' => [0b000, 0b000, 0b000, 0b000, 0b010],
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        _ => [0; LABEL_H],
    }
}

fn measure_text(text: &str) -> usize {
    text.chars().count().saturating_mul(LABEL_ADVANCE).max(1)
}

fn render_text(
    dst: &mut [u8],
    dst_w: usize,
    dst_h: usize,
    x: usize,
    y: usize,
    text: &str,
    rgba: [u8; 4],
) {
    let mut pen_x = x;
    for ch in text.chars() {
        let bits = glyph_bits(ch);
        for (row, mask) in bits.iter().enumerate() {
            for col in 0..LABEL_W {
                if (mask & (1 << (LABEL_W - 1 - col))) != 0 {
                    fill_rect(
                        dst,
                        dst_w,
                        dst_h,
                        pen_x + col * LABEL_SCALE,
                        y + row * LABEL_SCALE,
                        LABEL_SCALE,
                        LABEL_SCALE,
                        rgba,
                    );
                }
            }
        }
        pen_x = pen_x.saturating_add(LABEL_ADVANCE);
    }
}

fn data_to_px_x(val: f64, plot_w: usize) -> f64 {
    let frac = (val - AXIS_X_MIN) / (AXIS_X_MAX - AXIS_X_MIN);
    frac * (plot_w as f64)
}

fn data_to_px_y(val: f64, plot_h: usize) -> f64 {
    let frac = (val - AXIS_Y_MIN) / (AXIS_Y_MAX - AXIS_Y_MIN);
    (1.0 - frac) * (plot_h as f64)
}

fn format_tick(v: f64) -> String {
    let rounded = libm::round(v * 10.0) / 10.0;
    if libm::fabs(rounded - libm::round(rounded)) < 1e-9 {
        format!("{}", rounded as i32)
    } else {
        format!("{:.1}", rounded)
    }
}

fn compose_chart(w: u32, h: u32) -> Vec<u8> {
    let dst_w = w as usize;
    let dst_h = h as usize;
    let mut buf = vec![0u8; dst_w * dst_h * 4];

    fill_rect(&mut buf, dst_w, dst_h, 0, 0, dst_w, dst_h, BG_RGBA);

    let plot_x0 = MARGIN_LEFT;
    let plot_y0 = MARGIN_TOP;
    let plot_w = dst_w.saturating_sub(MARGIN_LEFT + MARGIN_RIGHT).max(1);
    let plot_h = dst_h.saturating_sub(MARGIN_TOP + MARGIN_BOTTOM).max(1);

    let mut v = AXIS_X_MIN;
    while v <= AXIS_X_MAX + TICK_STEP * 0.01 {
        if libm::fabs(v) > 1e-9 {
            let px = data_to_px_x(v, plot_w);
            let ix = plot_x0 + (px as usize).min(plot_w);
            if ix > plot_x0 && ix < plot_x0 + plot_w {
                for row in plot_y0..plot_y0 + plot_h {
                    put_pixel(&mut buf, dst_w, dst_h, ix, row, GRID_RGBA);
                }
            }
        }
        v += TICK_STEP;
    }

    let mut v = AXIS_Y_MIN;
    while v <= AXIS_Y_MAX + TICK_STEP * 0.01 {
        if libm::fabs(v) > 1e-9 {
            let py = data_to_px_y(v, plot_h);
            let iy = plot_y0 + (py as usize).min(plot_h);
            if iy > plot_y0 && iy < plot_y0 + plot_h {
                for col in plot_x0..plot_x0 + plot_w {
                    put_pixel(&mut buf, dst_w, dst_h, col, iy, GRID_RGBA);
                }
            }
        }
        v += TICK_STEP;
    }

    let y0_px = data_to_px_y(0.0, plot_h);
    let iy0 = plot_y0 + (y0_px as usize).min(plot_h);
    if iy0 >= plot_y0 && iy0 < plot_y0 + plot_h {
        for col in plot_x0..plot_x0 + plot_w {
            put_pixel(&mut buf, dst_w, dst_h, col, iy0, AXIS_RGBA);
        }
    }

    let x0_px = data_to_px_x(0.0, plot_w);
    let ix0 = plot_x0 + (x0_px as usize).min(plot_w);
    if ix0 >= plot_x0 && ix0 < plot_x0 + plot_w {
        for row in plot_y0..plot_y0 + plot_h {
            put_pixel(&mut buf, dst_w, dst_h, ix0, row, AXIS_RGBA);
        }
    }

    let tick_len = 4usize;
    let axis_y_px = plot_y0 + (data_to_px_y(0.0, plot_h) as usize).min(plot_h);
    let label_y = plot_y0 + plot_h + 2;
    let mut v = AXIS_X_MIN;
    while v <= AXIS_X_MAX + TICK_STEP * 0.01 {
        if libm::fabs(v) > 1e-9 {
            let px = data_to_px_x(v, plot_w);
            let ix = plot_x0 + (px as usize).min(plot_w);
            if ix > plot_x0 && ix < plot_x0 + plot_w {
                let ty = if axis_y_px >= plot_y0 && axis_y_px < plot_y0 + plot_h {
                    axis_y_px
                } else {
                    plot_y0 + plot_h - 1
                };
                for dy in 0..tick_len {
                    put_pixel(&mut buf, dst_w, dst_h, ix, ty + dy, TICK_RGBA);
                }
                let label = format_tick(v);
                let tw = measure_text(label.as_str());
                let lx = ix.saturating_sub(tw / 2);
                if label_y + LABEL_LINE_HEIGHT <= dst_h {
                    render_text(&mut buf, dst_w, dst_h, lx, label_y, label.as_str(), LABEL_RGBA);
                }
            }
        }
        v += TICK_STEP;
    }

    let axis_x_px = plot_x0 + (data_to_px_x(0.0, plot_w) as usize).min(plot_w);
    let mut v = AXIS_Y_MIN;
    while v <= AXIS_Y_MAX + TICK_STEP * 0.01 {
        if libm::fabs(v) > 1e-9 {
            let py = data_to_px_y(v, plot_h);
            let iy = plot_y0 + (py as usize).min(plot_h);
            if iy >= plot_y0 && iy < plot_y0 + plot_h {
                let tx = if axis_x_px >= plot_x0 && axis_x_px < plot_x0 + plot_w {
                    axis_x_px.saturating_sub(tick_len)
                } else {
                    plot_x0
                };
                for dx in 0..tick_len {
                    put_pixel(&mut buf, dst_w, dst_h, tx + dx, iy, TICK_RGBA);
                }
                let label = format_tick(v);
                let tw = measure_text(label.as_str());
                let lx = plot_x0.saturating_sub(tw + 3);
                let ly = iy.saturating_sub(LABEL_LINE_HEIGHT / 2);
                render_text(&mut buf, dst_w, dst_h, lx, ly, label.as_str(), LABEL_RGBA);
            }
        }
        v += TICK_STEP;
    }

    let mut prev_y: Option<usize> = None;
    for px_col in 0..plot_w {
        let data_x = AXIS_X_MIN + (px_col as f64 / plot_w as f64) * (AXIS_X_MAX - AXIS_X_MIN);
        let data_y = libm::sin(data_x * core::f64::consts::PI);
        let py = data_to_px_y(data_y, plot_h);
        let iy = (py as isize).clamp(0, plot_h as isize - 1) as usize;
        let screen_x = plot_x0 + px_col;
        let screen_y = plot_y0 + iy;

        if let Some(prev) = prev_y {
            let from = prev.min(iy);
            let to = prev.max(iy);
            for y in from..=to {
                put_pixel(&mut buf, dst_w, dst_h, screen_x, plot_y0 + y, SINE_RGBA);
            }
        } else {
            put_pixel(&mut buf, dst_w, dst_h, screen_x, screen_y, SINE_RGBA);
        }
        prev_y = Some(iy);
    }

    buf
}

fn open_window() -> Option<ui2::SurfaceWindow> {
    ui2::SurfaceWindow::create(
        WINDOW_TITLE,
        ui2::Rect {
            x: WINDOW_X,
            y: WINDOW_Y,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        },
        TEX_ID,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn main() {
    let Some(window) = open_window() else {
        platform::log_error("chart bp: window create failed\n");
        return;
    };

    let pixels = compose_chart(WINDOW_WIDTH, WINDOW_HEIGHT);
    if !gfx::upload_texture_rgba_image_now(TEX_ID, WINDOW_WIDTH, WINDOW_HEIGHT, pixels.as_slice()) {
        platform::log_error("chart bp: texture upload failed\n");
        return;
    }
    let _ = window.id().request_repaint();
    platform::log_info("chart bp: rendered sine axis chart\n");

    loop {
        platform::poll_once();
    }
}
