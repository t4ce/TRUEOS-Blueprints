// trueos-blueprint: features=["tokio-net-probe"]

use core::error::Error as _;
use core::fmt::Write as _;
use core::time::Duration;
use std::string::String;
use trueos::logl::{self, level};
use trueos::platform;
use trueos::ui2::{self, gfx};
use trueos::{input, runtime, tyche};

const WINDOW_X: i32 = 520;
const WINDOW_Y: i32 = 150;
const WINDOW_WIDTH: u32 = 512;
const WINDOW_HEIGHT: u32 = 320;
const TEX_ID: u32 = 4_776;
const FRAME_MS: u64 = 16;
const FLASH_FRAMES: u8 = 62;
const FLAG_FETCH_TIMEOUT_MS: u64 = 30_000;

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

#[derive(Copy, Clone, Eq, PartialEq)]
enum Flash {
    None,
    Correct,
    Wrong,
}

struct Game {
    answer: usize,
    answer_slot: usize,
    options: [usize; 4],
    svgs: [Option<String>; 4],
    flash: Flash,
    flash_frames: u8,
    selected_slot: Option<usize>,
    score: u32,
}

impl Game {
    fn new(rng: &mut tyche::SoftRng, runtime: &runtime::Runtime) -> Self {
        let mut game = Self {
            answer: 0,
            answer_slot: 0,
            options: [0; 4],
            svgs: [(); 4].map(|_| None),
            flash: Flash::None,
            flash_frames: 0,
            selected_slot: None,
            score: 0,
        };
        game.start_round(rng, runtime);
        game
    }

    fn start_round(&mut self, rng: &mut tyche::SoftRng, runtime: &runtime::Runtime) {
        self.answer = rng.usize_below(COUNTRIES.len());
        self.answer_slot = rng.usize_below(4);
        self.options = [usize::MAX; 4];
        self.svgs = [(); 4].map(|_| None);
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
        let loaded_svgs = runtime.block_on(fetch_round_svgs(self.options));
        for (slot, svg) in loaded_svgs.into_iter().enumerate() {
            let (code, _) = COUNTRIES[self.options[slot]];
            self.svgs[slot] = Some(svg.unwrap_or_else(|| fallback_flag_svg(code)));
        }
    }

    fn choose(&mut self, slot: usize) -> bool {
        if self.flash != Flash::None || slot >= 4 {
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

    fn tick_flash(&mut self, rng: &mut tyche::SoftRng, runtime: &runtime::Runtime) -> bool {
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
            self.start_round(rng, runtime);
        }
        true
    }

    fn title(&self) -> String {
        let (_, name) = COUNTRIES[self.answer];
        match self.flash {
            Flash::Correct => format!("Flag {} - hit", name),
            Flash::Wrong => format!("Flag {} - miss", name),
            Flash::None => format!("Flag {}", name),
        }
    }
}

async fn fetch_round_svgs(options: [usize; 4]) -> [Option<String>; 4] {
    let mut out = [(); 4].map(|_| None);
    let client = match build_reqwest_client() {
        Ok(client) => client,
        Err(err) => {
            logl::log(
                level::ERROR,
                format_args!("flags bp: reqwest client unavailable err={}\n", err),
            );
            return out;
        }
    };
    for slot in 0..4 {
        let (code, _) = COUNTRIES[options[slot]];
        out[slot] = match fetch_flag_svg(&client, code).await {
            Ok(svg) => {
                logl::log(
                    level::INFO,
                    format_args!(
                        "flags bp: reqwest fetch ok slot={} code={} svg_len={}\n",
                        slot,
                        code,
                        svg.len()
                    ),
                );
                Some(svg)
            }
            Err(err) => {
                logl::log(
                    level::ERROR,
                    format_args!(
                        "flags bp: reqwest fetch failed slot={} code={} err={}\n",
                        slot, code, err
                    ),
                );
                None
            }
        };
    }
    out
}

async fn fetch_flag_svg(client: &reqwest::Client, code: &str) -> Result<String, String> {
    let url = flag_url(code);
    let response = client
        .get(url.as_str())
        .send()
        .await
        .map_err(|err| format!("request {err}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("http {}", status.as_u16()));
    }
    response.text().await.map_err(|err| format!("body {err}"))
}

fn flag_url(code: &str) -> String {
    format!("https://flagcdn.com/{}.svg", code)
}

fn build_reqwest_client() -> Result<reqwest::Client, String> {
    logl::log(
        level::WARN,
        "flags bp: stage reqwest.client.build.insecure_tls\n",
    );
    reqwest::Client::builder()
        .timeout(Duration::from_millis(FLAG_FETCH_TIMEOUT_MS))
        .tls_danger_accept_invalid_certs(true)
        .build()
        .map_err(|err| {
            logl::log(
                level::ERROR,
                format_args!(
                    "flags bp: reqwest insecure builder failed debug={:?}\n",
                    err
                ),
            );
            if let Some(source) = err.source() {
                logl::log(
                    level::ERROR,
                    format_args!("flags bp: reqwest insecure builder source={}\n", source),
                );
            }
            format!("client {err}")
        })
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

fn hit_section(x: i32, y: i32) -> Option<usize> {
    if x < 0 || y < 0 || x >= WINDOW_WIDTH as i32 || y >= WINDOW_HEIGHT as i32 {
        return None;
    }
    let col = ((x as u32).saturating_mul(2) / WINDOW_WIDTH).min(1) as usize;
    let row = ((y as u32).saturating_mul(2) / WINDOW_HEIGHT).min(1) as usize;
    Some(row * 2 + col)
}

fn fallback_flag_svg(code: &str) -> String {
    let svg = match code {
        "ar" => {
            r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg"><rect width="640" height="480" fill="#75aadb"/><rect y="160" width="640" height="160" fill="#fff"/><circle cx="320" cy="240" r="38" fill="#f6b40e"/></svg>"##
        }
        "au" => {
            r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg"><rect width="640" height="480" fill="#012169"/><path d="M0 0h320v240H0z" fill="#012169"/><path d="M0 0l320 240M320 0L0 240" stroke="#fff" stroke-width="38"/><path d="M0 0l320 240M320 0L0 240" stroke="#c8102e" stroke-width="18"/><path d="M160 0v240M0 120h320" stroke="#fff" stroke-width="62"/><path d="M160 0v240M0 120h320" stroke="#c8102e" stroke-width="34"/><circle cx="458" cy="310" r="30" fill="#fff"/><circle cx="540" cy="155" r="22" fill="#fff"/></svg>"##
        }
        "br" => {
            r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg"><rect width="640" height="480" fill="#009b3a"/><path d="M320 72 572 240 320 408 68 240z" fill="#ffdf00"/><circle cx="320" cy="240" r="104" fill="#002776"/><path d="M220 220c76-24 158-18 230 20" stroke="#fff" stroke-width="22" fill="none"/></svg>"##
        }
        "ca" => {
            r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg"><rect width="160" height="480" fill="#d52b1e"/><rect x="160" width="320" height="480" fill="#fff"/><rect x="480" width="160" height="480" fill="#d52b1e"/><path d="M320 104l34 78 82-20-45 72 72 36-86 18 20 88-77-50-77 50 20-88-86-18 72-36-45-72 82 20z" fill="#d52b1e"/></svg>"##
        }
        "de" => {
            r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg"><rect width="640" height="160" fill="#000"/><rect y="160" width="640" height="160" fill="#dd0000"/><rect y="320" width="640" height="160" fill="#ffce00"/></svg>"##
        }
        "es" => {
            r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg"><rect width="640" height="480" fill="#aa151b"/><rect y="120" width="640" height="240" fill="#f1bf00"/><rect x="150" y="190" width="70" height="100" fill="#c60b1e"/></svg>"##
        }
        "fr" => {
            r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg"><rect width="214" height="480" fill="#0055a4"/><rect x="214" width="212" height="480" fill="#fff"/><rect x="426" width="214" height="480" fill="#ef4135"/></svg>"##
        }
        "gb" => {
            r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg"><rect width="640" height="480" fill="#012169"/><path d="M0 0l640 480M640 0L0 480" stroke="#fff" stroke-width="82"/><path d="M0 0l640 480M640 0L0 480" stroke="#c8102e" stroke-width="42"/><path d="M320 0v480M0 240h640" stroke="#fff" stroke-width="136"/><path d="M320 0v480M0 240h640" stroke="#c8102e" stroke-width="82"/></svg>"##
        }
        "in" => {
            r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg"><rect width="640" height="160" fill="#ff9933"/><rect y="160" width="640" height="160" fill="#fff"/><rect y="320" width="640" height="160" fill="#138808"/><circle cx="320" cy="240" r="54" fill="none" stroke="#000080" stroke-width="10"/><g stroke="#000080" stroke-width="4"><path d="M320 186v108M266 240h108M282 202l76 76M358 202l-76 76"/></g></svg>"##
        }
        "it" => {
            r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg"><rect width="214" height="480" fill="#009246"/><rect x="214" width="212" height="480" fill="#fff"/><rect x="426" width="214" height="480" fill="#ce2b37"/></svg>"##
        }
        "jp" => {
            r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg"><rect width="640" height="480" fill="#fff"/><circle cx="320" cy="240" r="128" fill="#bc002d"/></svg>"##
        }
        "kr" => {
            r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg"><rect width="640" height="480" fill="#fff"/><path d="M320 152a88 88 0 0 1 0 176 44 44 0 0 1 0-88 44 44 0 0 0 0-88z" fill="#c60c30"/><path d="M320 328a88 88 0 0 1 0-176 44 44 0 0 1 0 88 44 44 0 0 0 0 88z" fill="#003478"/><g stroke="#111" stroke-width="18"><path d="M132 122l86 50M422 308l86 50M132 358l86-50M422 172l86-50"/></g></svg>"##
        }
        "mx" => {
            r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg"><rect width="214" height="480" fill="#006847"/><rect x="214" width="212" height="480" fill="#fff"/><rect x="426" width="214" height="480" fill="#ce1126"/><circle cx="320" cy="240" r="46" fill="#b38e5d"/></svg>"##
        }
        "no" => {
            r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg"><rect width="640" height="480" fill="#ba0c2f"/><path d="M0 194h640M194 0v480" stroke="#fff" stroke-width="96"/><path d="M0 194h640M194 0v480" stroke="#00205b" stroke-width="56"/></svg>"##
        }
        "pl" => {
            r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg"><rect width="640" height="240" fill="#fff"/><rect y="240" width="640" height="240" fill="#dc143c"/></svg>"##
        }
        "se" => {
            r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg"><rect width="640" height="480" fill="#006aa7"/><path d="M0 190h640M205 0v480" stroke="#fecc00" stroke-width="76"/></svg>"##
        }
        "tr" => {
            r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg"><rect width="640" height="480" fill="#e30a17"/><circle cx="276" cy="240" r="118" fill="#fff"/><circle cx="314" cy="240" r="94" fill="#e30a17"/><path d="M410 178l18 48 52-2-42 32 18 48-44-29-42 32 14-50-42-31 52-3z" fill="#fff"/></svg>"##
        }
        "ua" => {
            r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg"><rect width="640" height="240" fill="#0057b7"/><rect y="240" width="640" height="240" fill="#ffd700"/></svg>"##
        }
        "us" => {
            r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg"><rect width="640" height="480" fill="#b22234"/><g fill="#fff"><rect y="37" width="640" height="37"/><rect y="111" width="640" height="37"/><rect y="185" width="640" height="37"/><rect y="259" width="640" height="37"/><rect y="333" width="640" height="37"/><rect y="407" width="640" height="37"/></g><rect width="276" height="259" fill="#3c3b6e"/><g fill="#fff"><circle cx="46" cy="43" r="10"/><circle cx="92" cy="86" r="10"/><circle cx="138" cy="43" r="10"/><circle cx="184" cy="86" r="10"/><circle cx="230" cy="43" r="10"/><circle cx="46" cy="172" r="10"/><circle cx="138" cy="172" r="10"/><circle cx="230" cy="172" r="10"/></g></svg>"##
        }
        "za" => {
            r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg"><rect width="640" height="240" fill="#de3831"/><rect y="240" width="640" height="240" fill="#002395"/><path d="M0 0l305 240L0 480z" fill="#000"/><path d="M0 42l250 198L0 438" fill="none" stroke="#ffb612" stroke-width="72"/><path d="M0 76l206 164L0 404" fill="none" stroke="#007a4d" stroke-width="92"/><path d="M242 240h398" stroke="#fff" stroke-width="96"/><path d="M242 240h398" stroke="#007a4d" stroke-width="58"/></svg>"##
        }
        _ => {
            r##"<svg viewBox="0 0 640 480" xmlns="http://www.w3.org/2000/svg"><rect width="640" height="480" fill="#1d2430"/><rect x="28" y="28" width="584" height="424" fill="none" stroke="#536173" stroke-width="18"/></svg>"##
        }
    };
    String::from(svg)
}

fn root_svg_attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let bytes = attrs.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if idx < bytes.len() && bytes[idx] == b'/' {
            idx += 1;
            continue;
        }
        let name_start = idx;
        while idx < bytes.len()
            && !bytes[idx].is_ascii_whitespace()
            && bytes[idx] != b'='
            && bytes[idx] != b'/'
        {
            idx += 1;
        }
        if idx == name_start {
            idx += 1;
            continue;
        }
        let attr_name = &attrs[name_start..idx];
        while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if idx >= bytes.len() || bytes[idx] != b'=' {
            continue;
        }
        idx += 1;
        while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if idx >= bytes.len() {
            return None;
        }
        let quote = bytes[idx];
        if quote != b'"' && quote != b'\'' {
            continue;
        }
        idx += 1;
        let value_start = idx;
        while idx < bytes.len() && bytes[idx] != quote {
            idx += 1;
        }
        if attr_name == name {
            return Some(&attrs[value_start..idx]);
        }
        idx = idx.saturating_add(1);
    }
    None
}

fn push_filtered_svg_attrs(out: &mut String, attrs: &str) {
    let bytes = attrs.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        let attr_start = idx;
        while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if idx < bytes.len() && bytes[idx] == b'/' {
            idx += 1;
            continue;
        }
        let name_start = idx;
        while idx < bytes.len()
            && !bytes[idx].is_ascii_whitespace()
            && bytes[idx] != b'='
            && bytes[idx] != b'/'
        {
            idx += 1;
        }
        if idx == name_start {
            idx += 1;
            continue;
        }
        let attr_name = &attrs[name_start..idx];
        while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if idx < bytes.len() && bytes[idx] == b'=' {
            idx += 1;
            while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
                idx += 1;
            }
            if idx < bytes.len() && (bytes[idx] == b'"' || bytes[idx] == b'\'') {
                let quote = bytes[idx];
                idx += 1;
                while idx < bytes.len() && bytes[idx] != quote {
                    idx += 1;
                }
                idx = idx.saturating_add(1);
            } else {
                while idx < bytes.len() && !bytes[idx].is_ascii_whitespace() {
                    idx += 1;
                }
            }
        }
        if !matches!(attr_name, "x" | "y" | "width" | "height") {
            out.push(' ');
            out.push_str(attrs[attr_start..idx].trim());
        }
    }
}

fn push_flag(out: &mut String, flag_svg: &str, x: i32, y: i32, w: i32, h: i32) {
    let Some(svg_start) = flag_svg.find("<svg") else {
        return;
    };
    let tail = &flag_svg[svg_start + 4..];
    let Some(head_end) = tail.find('>') else {
        return;
    };
    let attrs = &tail[..head_end];
    let mut body = &tail[head_end + 1..];
    if let Some(end) = body.rfind("</svg>") {
        body = &body[..end];
    }

    let _ = write!(
        out,
        r#"<svg x="{}" y="{}" width="{}" height="{}""#,
        x, y, w, h
    );
    if root_svg_attr(attrs, "viewBox").is_none()
        && let (Some(svg_w), Some(svg_h)) = (
            root_svg_attr(attrs, "width"),
            root_svg_attr(attrs, "height"),
        )
    {
        let _ = write!(out, r#" viewBox="0 0 {} {}""#, svg_w, svg_h);
    }
    push_filtered_svg_attrs(out, attrs);
    out.push('>');
    out.push_str(body);
    out.push_str("</svg>");
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
        let fallback;
        let svg = match game.svgs[slot].as_deref() {
            Some(svg) => svg,
            None => {
                fallback = fallback_flag_svg(code);
                fallback.as_str()
            }
        };
        push_flag(&mut out, svg, x + 10, y + 10, w - 20, h - 20);
    }

    out.push_str("</svg>");
    out
}

fn present(window: &ui2::SurfaceWindow, game: &Game) {
    let title = game.title();
    let _ = window.id().set_title(title.as_str());
    let svg = compose_svg(game);
    let rc = gfx::upload_svg_to_texture(TEX_ID, svg.as_bytes());
    if rc == 0 {
        let _ = window.id().request_repaint();
    } else {
        logl::log(
            level::ERROR,
            format_args!("flags bp: svg upload failed rc={}\n", rc),
        );
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
        let cursor_id = event.slot_id.max(1);
        let Ok((screen_x, screen_y)) =
            input::cursor_pos(cursor_id).or_else(|_| input::cursor_pos(1))
        else {
            continue;
        };
        let local_x = screen_x - info.content.x;
        let local_y = screen_y - info.content.y;
        if local_x < 0
            || local_y < 0
            || local_x >= info.content.width as i32
            || local_y >= info.content.height as i32
        {
            continue;
        }
        let surface_x =
            (local_x as i64 * WINDOW_WIDTH as i64 / info.content.width.max(1) as i64) as i32;
        let surface_y =
            (local_y as i64 * WINDOW_HEIGHT as i64 / info.content.height.max(1) as i64) as i32;
        if let Some(slot) = hit_section(surface_x, surface_y) {
            changed |= game.choose(slot);
        }
    }
    changed
}

fn main() {
    let Some(window) = open_window() else {
        logl::log(level::ERROR, "flags bp: surface window create failed\n");
        return;
    };
    let _ = window.id().set_resize_maintain_aspect(true);
    let _ = window.id().set_content_preserve_scale(true);
    let _ = window.id().set_vertical_scrollbar_visible(false);
    let _ = window.id().set_horizontal_scrollbar_visible(false);

    let runtime = match runtime::current_thread_net().build() {
        Ok(rt) => rt,
        Err(err) => {
            logl::log(
                level::ERROR,
                format_args!("flags bp: runtime build failed: {}\n", err),
            );
            return;
        }
    };
    logl::log(
        level::WARN,
        "flags bp: stage reqwest.client.build.worker_fetch\n",
    );

    let mut rng = tyche::SoftRng::new();
    let mut game = Game::new(&mut rng, &runtime);
    let mut cursor_seq = {
        let (_, next_seq, _) = input::read_cursor_events_since(0, 64);
        next_seq
    };
    let mut last_buttons = 0u32;

    present(&window, &game);
    logl::log(level::INFO, "flags bp: quiz ready\n");

    loop {
        let mut changed = false;
        changed |= handle_keyboard(&mut game);
        changed |= handle_cursor(&window, &mut cursor_seq, &mut last_buttons, &mut game);
        changed |= game.tick_flash(&mut rng, &runtime);
        if changed {
            present(&window, &game);
        }
        platform::poll_once();
        platform::sleep_ms(FRAME_MS);
    }
}
