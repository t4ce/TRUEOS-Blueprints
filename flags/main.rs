#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use core::fmt::Write as _;
use core::panic::PanicInfo;
use trueos::{TrueosAllocator, input, panic_abort, tyche};
use trueos::{ui2, vgfx, vsys};

#[global_allocator]
static GLOBAL_ALLOCATOR: TrueosAllocator = TrueosAllocator;

const WINDOW_X: i32 = 520;
const WINDOW_Y: i32 = 150;
const WINDOW_WIDTH: u32 = 512;
const WINDOW_HEIGHT: u32 = 320;
const TEX_ID: u32 = 4_776;
const FRAME_MS: u64 = 16;
const FLASH_FRAMES: u8 = 62;
const FETCH_PENDING: i32 = -8;

const CELL_MARGIN: i32 = 18;
const CELL_GAP: i32 = 14;
const CELL_W: i32 = (WINDOW_WIDTH as i32 - CELL_MARGIN * 2 - CELL_GAP) / 2;
const CELL_H: i32 = (WINDOW_HEIGHT as i32 - CELL_MARGIN * 2 - CELL_GAP) / 2;

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
    ("it", "Italy"),
    ("es", "Spain"),
    ("se", "Sweden"),
    ("no", "Norway"),
    ("pl", "Poland"),
    ("tr", "Turkey"),
    ("ar", "Argentina"),
    ("au", "Australia"),
];

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    panic_abort("flags bp: panic\n")
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum Flash {
    None,
    Correct,
    Wrong,
}

#[derive(Copy, Clone)]
struct FetchSlot {
    op_id: u32,
    done: bool,
}

impl FetchSlot {
    const fn empty() -> Self {
        Self {
            op_id: 0,
            done: true,
        }
    }
}

struct Game {
    answer: usize,
    answer_slot: usize,
    options: [usize; 4],
    fetches: [FetchSlot; 4],
    loading: bool,
    flash: Flash,
    flash_frames: u8,
    selected_slot: Option<usize>,
    score: u32,
}

impl Game {
    fn new(rng: &mut tyche::SoftRng) -> Self {
        let mut game = Self {
            answer: 0,
            answer_slot: 0,
            options: [0; 4],
            fetches: [FetchSlot::empty(); 4],
            loading: false,
            flash: Flash::None,
            flash_frames: 0,
            selected_slot: None,
            score: 0,
        };
        game.start_round(rng);
        game
    }

    fn start_round(&mut self, rng: &mut tyche::SoftRng) {
        for fetch in &self.fetches {
            trueos_flags::discardFlagSVGFetch(fetch.op_id);
        }
        self.answer = rng.usize_below(COUNTRIES.len());
        self.answer_slot = rng.usize_below(4);
        self.options = [usize::MAX; 4];
        self.options[self.answer_slot] = self.answer;

        for slot in 0..4 {
            if self.options[slot] != usize::MAX {
                continue;
            }
            loop {
                let candidate = rng.usize_below(COUNTRIES.len());
                if !self.options.iter().any(|&idx| idx == candidate) {
                    self.options[slot] = candidate;
                    break;
                }
            }
        }

        self.flash = Flash::None;
        self.flash_frames = 0;
        self.selected_slot = None;
        self.start_fetches();
    }

    fn start_fetches(&mut self) {
        self.loading = false;
        self.fetches = [FetchSlot::empty(); 4];
        for slot in 0..4 {
            let (code, _) = COUNTRIES[self.options[slot]];
            if !trueos_flags::getCachedFlagSVG(code).is_empty() {
                continue;
            }
            let op_id = trueos_flags::startFlagSVGFetch(code);
            if op_id != 0 {
                self.fetches[slot] = FetchSlot { op_id, done: false };
                self.loading = true;
            }
        }
    }

    fn poll_fetches(&mut self) -> bool {
        if !self.loading {
            return false;
        }
        let mut all_done = true;
        for fetch in &mut self.fetches {
            if fetch.done {
                continue;
            }
            let rc = trueos_flags::pollFlagSVGFetch(fetch.op_id);
            if rc == FETCH_PENDING {
                all_done = false;
                continue;
            }
            trueos_flags::discardFlagSVGFetch(fetch.op_id);
            fetch.done = true;
            fetch.op_id = 0;
        }
        if all_done {
            self.loading = false;
            return true;
        }
        false
    }

    fn choose(&mut self, slot: usize) -> bool {
        if self.loading || self.flash != Flash::None || slot >= 4 {
            return false;
        }
        self.selected_slot = Some(slot);
        self.flash_frames = FLASH_FRAMES;
        if slot == self.answer_slot {
            self.flash = Flash::Correct;
            self.score = self.score.saturating_add(1);
        } else {
            self.flash = Flash::Wrong;
        }
        true
    }

    fn tick_flash(&mut self, rng: &mut tyche::SoftRng) -> bool {
        if self.flash == Flash::None {
            return false;
        }
        if self.flash_frames > 0 {
            self.flash_frames -= 1;
        }
        if self.flash_frames != 0 {
            return false;
        }
        let was_correct = self.flash == Flash::Correct;
        self.flash = Flash::None;
        self.selected_slot = None;
        if was_correct {
            self.start_round(rng);
        }
        true
    }

    fn title(&self) -> String {
        let (_, name) = COUNTRIES[self.answer];
        match self.flash {
            Flash::Correct => format!("Flag {} - hit", name),
            Flash::Wrong => format!("Flag {} - miss", name),
            Flash::None if self.loading => format!("Flag {} - loading", name),
            Flash::None => format!("Flag {}", name),
        }
    }
}

fn open_window() -> Option<ui2::SurfaceWindow> {
    ui2::SurfaceWindow::create(
        "Flag loading",
        ui2::Rect {
            x: WINDOW_X,
            y: WINDOW_Y,
            width: WINDOW_WIDTH,
            height: WINDOW_HEIGHT,
        },
        TEX_ID,
    )
}

fn cell_rect(slot: usize) -> (i32, i32, i32, i32) {
    let col = (slot % 2) as i32;
    let row = (slot / 2) as i32;
    let x = CELL_MARGIN + col * (CELL_W + CELL_GAP);
    let y = CELL_MARGIN + row * (CELL_H + CELL_GAP);
    (x, y, CELL_W, CELL_H)
}

fn hit_slot(x: i32, y: i32) -> Option<usize> {
    for slot in 0..4 {
        let (cx, cy, cw, ch) = cell_rect(slot);
        if x >= cx && x < cx + cw && y >= cy && y < cy + ch {
            return Some(slot);
        }
    }
    None
}

fn fallback_flag_svg(code: &str) -> String {
    format!(
        r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg">
<rect width="640" height="480" fill="#1d2430"/>
<rect x="28" y="28" width="584" height="424" fill="none" stroke="#536173" stroke-width="18"/>
<path d="M100 330 L230 170 L338 282 L430 215 L548 330" fill="none" stroke="#83d2ff" stroke-width="34" stroke-linejoin="round"/>
<circle cx="492" cy="136" r="46" fill="#f3c969"/>
<text x="320" y="418" text-anchor="middle" font-family="sans-serif" font-size="64" fill="#e7edf7">{}</text>
</svg>"##,
        code.to_ascii_uppercase()
    )
}

fn flag_svg_for(code: &str) -> String {
    let svg = trueos_flags::getCachedFlagSVG(code);
    if svg.is_empty() {
        fallback_flag_svg(code)
    } else {
        svg
    }
}

fn push_flag(out: &mut String, flag_svg: &str, x: i32, y: i32, w: i32, h: i32) {
    let clean = flag_svg
        .find("<svg")
        .map(|idx| &flag_svg[idx + 4..])
        .unwrap_or(flag_svg);
    let _ = write!(
        out,
        r#"<svg x="{}" y="{}" width="{}" height="{}"{}"#,
        x, y, w, h, clean
    );
}

fn border_color(game: &Game, slot: usize) -> &'static str {
    match (game.flash, game.selected_slot) {
        (Flash::Correct, Some(selected)) if selected == slot => "#6AF0A1",
        (Flash::Wrong, Some(selected)) if selected == slot => "#FF6B7A",
        (Flash::Wrong, _) if game.answer_slot == slot => "#6AF0A1",
        _ => "#2B3746",
    }
}

fn compose_svg(game: &Game) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        r##"<svg width="{w}" height="{h}" viewBox="0 0 {w} {h}" xmlns="http://www.w3.org/2000/svg">
<rect width="{w}" height="{h}" fill="#090E14"/>
"##,
        w = WINDOW_WIDTH,
        h = WINDOW_HEIGHT
    );

    for slot in 0..4 {
        let (x, y, w, h) = cell_rect(slot);
        let stroke = border_color(game, slot);
        let _ = write!(
            out,
            r##"<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="5" fill="#111821" stroke="{stroke}" stroke-width="3"/>
"##,
            x = x,
            y = y,
            w = w,
            h = h,
            stroke = stroke
        );
        let (code, _) = COUNTRIES[game.options[slot]];
        let svg = flag_svg_for(code);
        push_flag(&mut out, svg.as_str(), x + 10, y + 10, w - 20, h - 20);
    }

    if game.loading {
        out.push_str(
            r##"<rect x="136" y="128" width="240" height="64" rx="7" fill="#101821" opacity="0.92" stroke="#536173" stroke-width="2"/>
<text x="256" y="168" text-anchor="middle" font-family="sans-serif" font-size="24" font-weight="700" fill="#EAF2FF">loading</text>
"##,
        );
    }

    out.push_str("</svg>");
    out
}

fn present(window: &ui2::SurfaceWindow, game: &Game) {
    let title = game.title();
    let _ = window.id().set_title(title.as_str());
    let svg = compose_svg(game);
    let rc = vgfx::upload_svg_to_texture(TEX_ID, svg.as_bytes());
    if rc == 0 {
        let _ = window.id().request_repaint();
    } else {
        vsys::log_errorf(format_args!("flags bp: svg upload failed rc={}\n", rc));
    }
}

fn handle_keyboard(game: &mut Game) -> bool {
    let mut changed = false;
    while let Some(event) = input::pop_keyboard_output() {
        if event.kind != input::KEYBOARD_OUTPUT_KIND_KEY {
            continue;
        }
        let slot = match event.key_code {
            input::KEYBOARD_KEY_ARROW_LEFT => Some(0),
            input::KEYBOARD_KEY_ARROW_UP => Some(1),
            input::KEYBOARD_KEY_ARROW_DOWN => Some(2),
            input::KEYBOARD_KEY_ARROW_RIGHT
            | input::KEYBOARD_KEY_SPACE
            | input::KEYBOARD_KEY_ENTER => Some(3),
            _ => None,
        };
        if let Some(slot) = slot {
            changed |= game.choose(slot);
        }
    }
    changed
}

fn handle_cursor(
    window: &ui2::SurfaceWindow,
    read_seq: &mut u64,
    last_buttons: &mut u32,
    game: &mut Game,
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
        if let Some(slot) = hit_slot(local_x, local_y) {
            changed |= game.choose(slot);
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
    let _ = window.id().set_resize_maintain_aspect(true);
    let _ = window.id().set_content_preserve_scale(true);
    let _ = window.id().set_vertical_scrollbar_visible(false);
    let _ = window.id().set_horizontal_scrollbar_visible(false);

    let mut rng = tyche::SoftRng::new();
    let mut game = Game::new(&mut rng);
    let mut cursor_seq = {
        let (_, next_seq, _) = input::read_cursor_events_since(0, 64);
        next_seq
    };
    let mut last_buttons = 0u32;

    present(&window, &game);
    vsys::log_info("flags bp: quiz ready\n");

    loop {
        let mut changed = false;
        changed |= game.poll_fetches();
        changed |= handle_keyboard(&mut game);
        changed |= handle_cursor(&window, &mut cursor_seq, &mut last_buttons, &mut game);
        changed |= game.tick_flash(&mut rng);
        if changed {
            present(&window, &game);
        }
        vsys::poll_once();
        vsys::sleep_ms(FRAME_MS);
    }
}
