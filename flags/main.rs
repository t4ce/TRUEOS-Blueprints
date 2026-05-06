#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use core::fmt::Write as _;
use core::panic::PanicInfo;
use trueos::{TrueosAllocator, input, panic_abort};
use trueos::{ui2, vgfx, vsys};

#[global_allocator]
static GLOBAL_ALLOCATOR: TrueosAllocator = TrueosAllocator;

const WINDOW_TITLE: &str = "Flags BP";
const WINDOW_X: i32 = 520;
const WINDOW_Y: i32 = 150;
const WINDOW_WIDTH: u32 = 420;
const WINDOW_HEIGHT: u32 = 292;
const TEX_ID: u32 = 4_776;
const BUTTON_Y: i32 = 14;
const BUTTON_W: i32 = 58;
const BUTTON_H: i32 = 34;
const PREV_X: i32 = 18;
const NEXT_X: i32 = WINDOW_WIDTH as i32 - PREV_X - BUTTON_W;

const COUNTRIES: &[(&str, &str)] = &[
    ("us", "United States"),
    ("de", "Germany"),
    ("jp", "Japan"),
    ("br", "Brazil"),
    ("za", "South Africa"),
    ("in", "India"),
    ("ua", "Ukraine"),
    ("ca", "Canada"),
    ("mx", "Mexico"),
    ("kr", "South Korea"),
    ("fr", "France"),
    ("gb", "United Kingdom"),
];

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    panic_abort("flags bp: panic\n")
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

fn button_hit(x: i32, y: i32, bx: i32) -> bool {
    x >= bx && x < bx + BUTTON_W && y >= BUTTON_Y && y < BUTTON_Y + BUTTON_H
}

fn country_title(index: usize) -> String {
    let (code, name) = COUNTRIES[index];
    format!("Flags BP - {} ({})", name, code)
}

fn fallback_flag_svg(code: &str) -> String {
    format!(
        r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg">
<rect width="640" height="480" fill="#1d2430"/>
<rect x="24" y="24" width="592" height="432" fill="none" stroke="#536173" stroke-width="18"/>
<path d="M120 320 L250 170 L350 280 L430 220 L540 330" fill="none" stroke="#83d2ff" stroke-width="34" stroke-linejoin="round"/>
<circle cx="490" cy="135" r="44" fill="#f3c969"/>
<text x="320" y="418" text-anchor="middle" font-family="sans-serif" font-size="58" fill="#e7edf7">{}</text>
</svg>"##,
        code.to_ascii_uppercase()
    )
}

fn push_flag_svg(out: &mut String, flag_svg: &str) {
    let clean = flag_svg
        .find("<svg")
        .map(|idx| &flag_svg[idx + 4..])
        .unwrap_or(flag_svg);
    let _ = write!(
        out,
        r#"<svg x="60" y="76" width="300" height="178"{}"#,
        clean
    );
}

fn compose_svg(index: usize, flag_svg: &str) -> String {
    let (code, name) = COUNTRIES[index];
    let mut out = String::new();
    let _ = write!(
        out,
        r##"<svg width="{w}" height="{h}" viewBox="0 0 {w} {h}" xmlns="http://www.w3.org/2000/svg">
<rect width="{w}" height="{h}" fill="#0b1017"/>
<rect x="0" y="0" width="{w}" height="62" fill="#121a24"/>
<rect x="20" y="68" width="380" height="196" rx="4" fill="#18202b" stroke="#2b3746" stroke-width="2"/>
<rect x="{px}" y="{by}" width="{bw}" height="{bh}" rx="6" fill="#243144" stroke="#5c6f87" stroke-width="2"/>
<path d="M54 25 L40 31 L54 37" fill="none" stroke="#eaf2ff" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/>
<rect x="{nx}" y="{by}" width="{bw}" height="{bh}" rx="6" fill="#243144" stroke="#5c6f87" stroke-width="2"/>
<path d="M366 25 L380 31 L366 37" fill="none" stroke="#eaf2ff" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/>
<text x="210" y="36" text-anchor="middle" font-family="sans-serif" font-size="20" font-weight="700" fill="#eef5ff">{name}</text>
<text x="210" y="56" text-anchor="middle" font-family="sans-serif" font-size="12" fill="#95a5ba">{pos}/{total}  {code}</text>
"##,
        w = WINDOW_WIDTH,
        h = WINDOW_HEIGHT,
        px = PREV_X,
        nx = NEXT_X,
        by = BUTTON_Y,
        bw = BUTTON_W,
        bh = BUTTON_H,
        name = name,
        pos = index + 1,
        total = COUNTRIES.len(),
        code = code.to_ascii_uppercase()
    );
    push_flag_svg(&mut out, flag_svg);
    out.push_str("</svg>");
    out
}

fn load_flag_svg(index: usize) -> String {
    let (code, _) = COUNTRIES[index];
    let svg = trueos_flags::getFlagSVG(code);
    if svg.is_empty() {
        fallback_flag_svg(code)
    } else {
        svg
    }
}

fn render(window: &ui2::SurfaceWindow, index: usize) {
    let title = country_title(index);
    let _ = window.id().set_title(title.as_str());
    let flag = load_flag_svg(index);
    let svg = compose_svg(index, flag.as_str());
    let rc = vgfx::upload_svg_to_texture(TEX_ID, svg.as_bytes());
    if rc == 0 {
        let _ = window.id().request_repaint();
    } else {
        vsys::log_errorf(format_args!("flags bp: svg upload failed rc={}\n", rc));
    }
}

fn previous(index: &mut usize) {
    *index = if *index == 0 {
        COUNTRIES.len() - 1
    } else {
        *index - 1
    };
}

fn next(index: &mut usize) {
    *index = (*index + 1) % COUNTRIES.len();
}

fn handle_keyboard(index: &mut usize) -> bool {
    let mut changed = false;
    while let Some(event) = input::pop_keyboard_output() {
        if event.kind != input::KEYBOARD_OUTPUT_KIND_KEY {
            continue;
        }
        match event.key_code {
            input::KEYBOARD_KEY_ARROW_LEFT => {
                previous(index);
                changed = true;
            }
            input::KEYBOARD_KEY_ARROW_RIGHT
            | input::KEYBOARD_KEY_SPACE
            | input::KEYBOARD_KEY_ENTER => {
                next(index);
                changed = true;
            }
            _ => {}
        }
    }
    changed
}

fn handle_cursor(
    window: &ui2::SurfaceWindow,
    read_seq: &mut u64,
    last_buttons: &mut u32,
    index: &mut usize,
) -> bool {
    let (events, next_seq, _) = input::read_cursor_events_since(*read_seq, 32);
    *read_seq = next_seq;
    let Some(info) = window.id().info() else {
        return false;
    };

    let mut changed = false;
    for event in events {
        let pressed = (event.buttons_down & 1) != 0 && (*last_buttons & 1) == 0;
        *last_buttons = event.buttons_down;
        if !pressed {
            continue;
        }
        let local_x = event.x as i32 - info.content.x;
        let local_y = event.y as i32 - info.content.y;
        if button_hit(local_x, local_y, PREV_X) {
            previous(index);
            changed = true;
        } else if button_hit(local_x, local_y, NEXT_X) {
            next(index);
            changed = true;
        }
    }
    changed
}

#[unsafe(no_mangle)]
pub extern "C" fn main() {
    let Some(window) = open_window() else {
        vsys::log_error("flags bp: surface window create failed\n");
        return;
    };

    let mut index = 0usize;
    let mut cursor_seq = {
        let (_, next_seq, _) = input::read_cursor_events_since(0, 64);
        next_seq
    };
    let mut last_buttons = 0u32;

    render(&window, index);
    vsys::log_info("flags bp: ready\n");

    loop {
        let changed = handle_keyboard(&mut index)
            || handle_cursor(&window, &mut cursor_seq, &mut last_buttons, &mut index);
        if changed {
            render(&window, index);
        }
        vsys::sleep_ms(16);
    }
}
