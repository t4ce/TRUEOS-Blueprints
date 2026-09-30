//! Compact Cube UI dialogs, migrated from Cubes' `cube_interface` raster model.
//! The same generated MicroFont glyphs, icons, themes and layered cell painting
//! produce each panel. Games composites twelve precomputed panels in UI4.
use alloc::vec::Vec;

#[path = "dialog_style.rs"]
mod style;

pub const WIDTH: usize = 150;
pub const HEIGHT: usize = 100;
pub const ATLAS_COLUMNS: usize = 4;
pub const ATLAS_ROWS: usize = 3;
pub const SCALE: usize = 2;
pub const ATLAS_WIDTH: usize = WIDTH * ATLAS_COLUMNS * SCALE;
pub const ATLAS_HEIGHT: usize = HEIGHT * ATLAS_ROWS * SCALE;

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum Widget {
    Checkbox,
    Button,
}

pub struct Definition {
    pub title: &'static str,
    pub text: &'static str,
    pub button: &'static str,
    pub theme: usize,
    pub checkbox: bool,
    pub enabled: bool,
}

const CHECK_RECT: [i32; 4] = [15, 44, 120, 18];
const BUTTON_RECT: [i32; 4] = [15, 70, 120, 20];

pub fn hit(def: &Definition, x: f32, y: f32) -> Option<Widget> {
    let contains = |[left, top, width, height]: [i32; 4]| {
        x >= left as f32
            && y >= top as f32
            && x < (left + width) as f32
            && y < (top + height) as f32
    };
    if def.enabled && def.checkbox && contains(CHECK_RECT) {
        Some(Widget::Checkbox)
    } else if def.enabled && contains(BUTTON_RECT) {
        Some(Widget::Button)
    } else {
        None
    }
}

struct Canvas {
    colors: Vec<u32>,
    heights: Vec<u8>,
}

impl Canvas {
    fn new(color: u32) -> Self {
        Self {
            colors: alloc::vec![color; WIDTH * HEIGHT],
            heights: alloc::vec![0; WIDTH * HEIGHT],
        }
    }

    fn put(&mut self, x: i32, y: i32, z: u8, color: u32) {
        if x >= 0 && y >= 0 && (x as usize) < WIDTH && (y as usize) < HEIGHT {
            let index = y as usize * WIDTH + x as usize;
            if z >= self.heights[index] {
                self.colors[index] = color;
                self.heights[index] = z;
            }
        }
    }

    fn rect(&mut self, [x, y, w, h]: [i32; 4], z: u8, color: u32, clip: i32) {
        for dy in 0..h {
            for dx in 0..w {
                if dx.min(w - 1 - dx) + dy.min(h - 1 - dy) >= clip {
                    self.put(x + dx, y + dy, z, color);
                }
            }
        }
    }

    fn label(&mut self, text: &str, x: i32, y: i32, z: u8, color: u32) {
        for (i, c) in text.bytes().enumerate() {
            let glyph = style::GLYPHS[usize::from(c.clamp(32, 126) - 32)];
            for bit in 0..64 {
                if glyph >> (63 - bit) & 1 != 0 {
                    self.put(
                        x + i as i32 * 6 + bit % 6 + i32::from(c == b'q'),
                        y + bit / 6,
                        z,
                        color,
                    );
                }
            }
        }
    }

    fn icon(&mut self, id: usize, x: i32, y: i32, z: u8, color: u32) {
        for (dy, row) in style::ICONS[id].iter().enumerate() {
            for dx in 0..7 {
                if row >> (6 - dx) & 1 != 0 {
                    self.put(x + dx, y + dy as i32, z, color);
                }
            }
        }
    }

    fn button(&mut self, rect: [i32; 4], text: &str, fill: u32, ink: u32, edge: u32, lift: u8) {
        self.rect(rect, 1, edge, 1);
        self.rect(rect, 2 + lift, fill, 1);
        let [x, y, w, h] = rect;
        self.label(
            text,
            x + (w - text.len() as i32 * 6) / 2,
            y + (h - 11) / 2,
            3 + lift,
            ink,
        );
    }

    fn rgba(&self, x: usize, y: usize) -> [u8; 4] {
        let index = y * WIDTH + x;
        let color = self.colors[index];
        let z = self.heights[index] as i32;
        let left = if x > 0 {
            self.heights[index - 1] as i32
        } else {
            z
        };
        let top = if y > 0 {
            self.heights[index - WIDTH] as i32
        } else {
            z
        };
        let shade = (255 + (z - left) * 15 + (z - top) * 12).clamp(176, 255);
        let channel = |shift| (((color >> shift) & 255_u32) as i32 * shade / 255) as u8;
        [channel(16), channel(8), channel(0), 255]
    }
}

fn tone(color: u32, amount: f32) -> u32 {
    (0..3)
        .map(|a| ((((color >> (a * 8)) & 255) as f32 * amount).min(255.) as u32) << (a * 8))
        .sum()
}

fn raster(def: &Definition, checked: bool, hover: Option<Widget>, selected: bool) -> Canvas {
    let t = style::THEMES[def.theme];
    let mut c = Canvas::new(t[0]);
    c.rect([1, 1, WIDTH as i32 - 2, HEIGHT as i32 - 2], 0, t[1], 1);
    c.rect([1, 1, WIDTH as i32 - 2, 16], 1, t[2], 1);
    c.label(def.title, 6, 4, 2, t[6]);
    c.label(def.text, 7, 24, 1, if def.enabled { t[3] } else { t[4] });
    if def.checkbox {
        let lift = u8::from(hover == Some(Widget::Checkbox));
        let fill = if checked { t[5] } else { t[7] };
        c.button(
            CHECK_RECT,
            "",
            tone(fill, if lift != 0 { 1.15 } else { 1.0 }),
            t[3],
            t[0],
            lift,
        );
        c.rect([20, 49, 9, 9], 3 + lift, t[3], 0);
        c.rect([21, 50, 7, 7], 3 + lift, t[1], 0);
        if checked {
            c.icon(1, 21, 50, 4 + lift, t[5]);
        }
        c.label(
            "NEW GAME",
            34,
            48,
            3 + lift,
            if checked { t[6] } else { t[3] },
        );
    }
    let lift = u8::from(hover == Some(Widget::Button));
    let fill = if def.enabled { t[5] } else { t[11] };
    c.button(
        BUTTON_RECT,
        def.button,
        tone(fill, if lift != 0 { 1.15 } else { 1.0 }),
        if def.enabled { t[6] } else { t[4] },
        t[0],
        lift,
    );
    if selected {
        let gold = t[9];
        c.rect([0, 0, WIDTH as i32, 2], 5, gold, 0);
        c.rect([0, HEIGHT as i32 - 2, WIDTH as i32, 2], 5, gold, 0);
    }
    c
}

pub fn atlas(
    definitions: &[Definition; 12],
    checked: &[bool; 5],
    hover: Option<(usize, Widget)>,
    selected: usize,
) -> Vec<u8> {
    let mut bytes = alloc::vec![0_u8; ATLAS_WIDTH * ATLAS_HEIGHT * 4];
    for (i, def) in definitions.iter().enumerate() {
        let canvas = raster(
            def,
            checked.get(i).copied().unwrap_or(false),
            hover.and_then(|(index, widget)| (index == i).then_some(widget)),
            selected == i,
        );
        let x0 = (i % ATLAS_COLUMNS) * WIDTH * SCALE;
        let y0 = (i / ATLAS_COLUMNS) * HEIGHT * SCALE;
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let pixel = canvas.rgba(x, y);
                for sy in 0..SCALE {
                    for sx in 0..SCALE {
                        let at = ((y0 + y * SCALE + sy) * ATLAS_WIDTH + x0 + x * SCALE + sx) * 4;
                        bytes[at..at + 4].copy_from_slice(&pixel);
                    }
                }
            }
        }
    }
    bytes
}
