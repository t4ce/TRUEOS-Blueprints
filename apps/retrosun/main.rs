#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use core::fmt::Write as _;
use core::panic::PanicInfo;

use trueos::panic_abort;
use trueos::platform;
use trueos::ui2::{self, gfx};

const WINDOW_TITLE: &str = "RETRO SUN";
const WINDOW_X: i32 = 120;
const WINDOW_Y: i32 = 42;
const WINDOW_WIDTH: u32 = 1600;
const WINDOW_HEIGHT: u32 = 900;
const TEX_ID: u32 = 4_888;
const FRAME_MS: u64 = 50;

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    panic_abort("retrosun bp: panic\n")
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
        platform::log_error("retrosun bp: window create failed\n");
        return;
    };

    let window_id = window.id();
    let _ = window_id.set_resize_maintain_aspect(true);
    let _ = window_id.set_content_preserve_scale(true);
    let _ = window_id.set_vertical_scrollbar_visible(false);
    let _ = window_id.set_horizontal_scrollbar_visible(false);
    let _ = window_id.set_title("RETRO SUN // slow wave");

    let mut frame = 0u32;
    loop {
        let svg = compose_svg(frame);
        let rc = gfx::upload_svg_to_texture(TEX_ID, svg.as_bytes());
        if rc != 0 {
            platform::log_errorf(format_args!("retrosun bp: svg upload failed rc={}\n", rc));
            break;
        }
        let _ = window_id.request_repaint();
        frame = frame.wrapping_add(1);
        platform::poll_once();
        platform::sleep_ms(FRAME_MS);
    }
}

fn compose_svg(frame: u32) -> String {
    let t = frame as f32 * 0.037;
    let glow = 0.72 + wave(t * 0.72, 0.16);
    let sun_y = 382.0 + wave(t * 0.29, 10.0);
    let sun_r = 258.0 + wave(t * 0.21, 5.5);
    let horizon = 598.0 + wave(t * 0.38, 7.0);
    let scan_phase = (frame % 20) as f32 * 2.0;

    let mut out = String::new();
    let _ = write!(
        out,
        r##"<svg width="{w}" height="{h}" viewBox="0 0 {w} {h}" xmlns="http://www.w3.org/2000/svg">
<defs>
  <linearGradient id="sky" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0%" stop-color="#060415"/>
    <stop offset="32%" stop-color="#15103A"/>
    <stop offset="64%" stop-color="#3B124D"/>
    <stop offset="100%" stop-color="#10071D"/>
  </linearGradient>
  <radialGradient id="sun" cx="50%" cy="47%" r="60%">
    <stop offset="0%" stop-color="#FFF7A8"/>
    <stop offset="42%" stop-color="#FFB43F"/>
    <stop offset="72%" stop-color="#FF4D8D"/>
    <stop offset="100%" stop-color="#8D2BFF"/>
  </radialGradient>
  <linearGradient id="ground" x1="0" y1="0" x2="0" y2="1">
    <stop offset="0%" stop-color="#14072B"/>
    <stop offset="100%" stop-color="#05030B"/>
  </linearGradient>
  <linearGradient id="wire" x1="0" y1="0" x2="1" y2="0">
    <stop offset="0%" stop-color="#10F7FF" stop-opacity="0.20"/>
    <stop offset="50%" stop-color="#FF4DFF" stop-opacity="0.94"/>
    <stop offset="100%" stop-color="#10F7FF" stop-opacity="0.20"/>
  </linearGradient>
</defs>
<rect width="{w}" height="{h}" fill="url(#sky)"/>
"##,
        w = WINDOW_WIDTH,
        h = WINDOW_HEIGHT
    );

    push_starfield(&mut out, t);
    push_neon_halo(&mut out, sun_y, sun_r, glow);
    push_sun(&mut out, sun_y, sun_r, t);
    push_mountains(&mut out, horizon, t);
    push_grid(&mut out, horizon, t);
    push_foreground_waves(&mut out, t);
    push_scanlines(&mut out, scan_phase);

    out.push_str("</svg>");
    out
}

fn push_starfield(out: &mut String, t: f32) {
    let mut seed = 0xA751_9E3Du32;
    for idx in 0..86 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let x = (seed % WINDOW_WIDTH) as f32;
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let base_y = (seed % 430) as f32 + 14.0;
        let y = wrap(base_y + t * (1.0 + (idx % 5) as f32 * 0.16), 12.0, 455.0);
        let r = 0.8 + ((seed >> 13) % 4) as f32 * 0.36;
        let a = 0.28 + wave(t * 1.7 + idx as f32 * 0.67, 0.34);
        let color = if idx % 3 == 0 { "#70F8FF" } else { "#FFE7FF" };
        let _ = write!(
            out,
            r##"<circle cx="{:.1}" cy="{:.1}" r="{:.2}" fill="{}" opacity="{:.2}"/>"##,
            x,
            y,
            r,
            color,
            a.clamp(0.12, 0.86)
        );
    }
}

fn push_neon_halo(out: &mut String, sun_y: f32, sun_r: f32, glow: f32) {
    for idx in 0..8 {
        let r = sun_r + 34.0 + idx as f32 * 31.0;
        let opacity = (0.18 - idx as f32 * 0.016) * glow;
        let stroke = if idx % 2 == 0 { "#FF48D4" } else { "#36F8FF" };
        let _ = write!(
            out,
            r##"<circle cx="800" cy="{:.1}" r="{:.1}" fill="none" stroke="{}" stroke-width="{:.1}" opacity="{:.3}"/>"##,
            sun_y,
            r,
            stroke,
            16.0 + idx as f32 * 5.0,
            opacity.max(0.015)
        );
    }
}

fn push_sun(out: &mut String, sun_y: f32, sun_r: f32, t: f32) {
    let _ =
        write!(out, r##"<circle cx="800" cy="{:.1}" r="{:.1}" fill="url(#sun)"/>"##, sun_y, sun_r);

    for band in 0..15 {
        let y = sun_y - sun_r + 34.0 + band as f32 * 31.0 + wave(t * 1.15 + band as f32, 8.0);
        let h = 11.0 + (band % 3) as f32 * 5.0;
        let cut_w = chord_width(sun_y, sun_r, y + h * 0.5) + 26.0;
        let x = 800.0 - cut_w * 0.5;
        let alpha = 0.82 - band as f32 * 0.025;
        let _ = write!(
            out,
            r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="#10071D" opacity="{:.2}"/>"##,
            x,
            y,
            cut_w,
            h,
            alpha.clamp(0.34, 0.88)
        );
    }

    for idx in 0..5 {
        let y = sun_y + sun_r - 96.0 + idx as f32 * 19.0 + wave(t + idx as f32, 4.0);
        let _ = write!(
            out,
            r##"<path d="M{:.1} {:.1} C650 {:.1} 720 {:.1} 800 {:.1} C890 {:.1} 980 {:.1} {:.1} {:.1}" fill="none" stroke="#FFE066" stroke-width="{:.1}" opacity="{:.2}"/>"##,
            800.0 - sun_r + 24.0,
            y,
            y - 14.0,
            y + 13.0,
            y,
            y - 13.0,
            y + 15.0,
            800.0 + sun_r - 24.0,
            y,
            2.0 + idx as f32,
            0.20
        );
    }
}

fn push_mountains(out: &mut String, horizon: f32, t: f32) {
    let back_y = horizon - 62.0 + wave(t * 0.35, 9.0);
    let front_y = horizon - 34.0 + wave(t * 0.51, 7.0);
    let _ = write!(
        out,
        r##"<path d="M0 {hy:.1} L130 {a:.1} L245 {b:.1} L360 {c:.1} L520 {d:.1} L650 {e:.1} L780 {f:.1} L930 {g:.1} L1090 {d:.1} L1210 {h:.1} L1390 {i:.1} L1600 {j:.1} L1600 900 L0 900 Z" fill="#090A1A" opacity="0.88"/>"##,
        hy = back_y,
        a = back_y - 54.0,
        b = back_y - 18.0,
        c = back_y - 86.0,
        d = back_y - 24.0,
        e = back_y - 76.0,
        f = back_y - 20.0,
        g = back_y - 92.0,
        h = back_y - 44.0,
        i = back_y - 100.0,
        j = back_y - 28.0
    );
    let _ = write!(
        out,
        r##"<path d="M0 {hy:.1} L180 {a:.1} L335 {b:.1} L500 {c:.1} L680 {d:.1} L810 {e:.1} L1000 {f:.1} L1180 {g:.1} L1355 {h:.1} L1600 {i:.1} L1600 900 L0 900 Z" fill="#050711"/>"##,
        hy = front_y,
        a = front_y - 44.0,
        b = front_y - 12.0,
        c = front_y - 72.0,
        d = front_y - 20.0,
        e = front_y - 56.0,
        f = front_y - 16.0,
        g = front_y - 70.0,
        h = front_y - 18.0,
        i = front_y - 62.0
    );
}

fn push_grid(out: &mut String, horizon: f32, t: f32) {
    let _ = write!(
        out,
        r##"<rect x="0" y="{:.1}" width="1600" height="{:.1}" fill="url(#ground)"/>"##,
        horizon,
        900.0 - horizon
    );
    let van_x = 800.0 + wave(t * 0.18, 22.0);

    for i in -18..=18 {
        let x = 800.0 + i as f32 * 92.0;
        let _ = write!(
            out,
            r##"<path d="M{:.1} 900 L{:.1} {:.1}" stroke="url(#wire)" stroke-width="{:.1}" opacity="{:.2}"/>"##,
            x,
            van_x + i as f32 * 7.0,
            horizon,
            if i == 0 { 3.2 } else { 1.7 },
            0.68
        );
    }

    for row in 0..22 {
        let p = row as f32 / 22.0;
        let eased = p * p;
        let y = horizon + eased * (900.0 - horizon) + ((t * 18.0 + row as f32 * 9.0) % 9.0);
        let width = 1.2 + p * 3.8;
        let opacity = 0.24 + p * 0.64;
        let _ = write!(
            out,
            r##"<path d="M0 {:.1} C360 {:.1} 460 {:.1} 800 {:.1} C1140 {:.1} 1240 {:.1} 1600 {:.1}" fill="none" stroke="#FF3DF2" stroke-width="{:.1}" opacity="{:.2}"/>"##,
            y,
            y - wave(t + row as f32, 10.0),
            y + wave(t * 0.8 + row as f32, 7.0),
            y,
            y - wave(t * 0.9 + row as f32, 8.0),
            y + wave(t + row as f32, 6.0),
            y,
            width,
            opacity
        );
    }
}

fn push_foreground_waves(out: &mut String, t: f32) {
    for band in 0..8 {
        let y = 760.0 + band as f32 * 18.0 + wave(t * 0.65 + band as f32, 10.0);
        let color = if band % 2 == 0 { "#2AFBFF" } else { "#FF4AF6" };
        let _ = write!(
            out,
            r##"<path d="M-30 {:.1} C160 {:.1} 300 {:.1} 500 {:.1} C700 {:.1} 910 {:.1} 1110 {:.1} C1300 {:.1} 1440 {:.1} 1630 {:.1}" fill="none" stroke="{}" stroke-width="{:.1}" opacity="{:.2}"/>"##,
            y,
            y - 24.0 + wave(t + band as f32, 10.0),
            y + 18.0,
            y - 4.0,
            y - 18.0,
            y + 20.0,
            y + 2.0,
            y - 20.0,
            y + 18.0,
            y,
            color,
            2.0 + band as f32 * 0.34,
            0.20 + band as f32 * 0.035
        );
    }
}

fn push_scanlines(out: &mut String, phase: f32) {
    let mut y = phase - 40.0;
    while y < WINDOW_HEIGHT as f32 {
        let _ = write!(
            out,
            r##"<rect x="0" y="{:.1}" width="1600" height="2" fill="#000" opacity="0.22"/>"##,
            y
        );
        y += 8.0;
    }
    out.push_str(
        r##"<rect x="0" y="0" width="1600" height="900" fill="none" stroke="#FF65F7" stroke-width="5" opacity="0.75"/>
<rect x="10" y="10" width="1580" height="880" fill="none" stroke="#38F8FF" stroke-width="2" opacity="0.52"/>
"##,
    );
}

fn chord_width(cy: f32, r: f32, y: f32) -> f32 {
    let dy = (y - cy).abs();
    if dy >= r {
        0.0
    } else {
        2.0 * libm::sqrtf(r * r - dy * dy)
    }
}

fn wave(x: f32, amp: f32) -> f32 {
    libm::sinf(x) * amp
}

fn wrap(value: f32, min: f32, max: f32) -> f32 {
    let span = max - min;
    if span <= 0.0 {
        return min;
    }
    min + libm::fmodf(value - min + span * 8.0, span)
}
