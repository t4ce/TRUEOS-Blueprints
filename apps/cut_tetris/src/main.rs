#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use cut_tetris::{Cutter, Game, PieceKind, Rotation, piece_cells, piece_dimensions};
use trueos::input;
use trueos::logl::{self, level};
use trueos::ui4_scene::{Damage, Error as UiError, Frame, rgba};
use trueos::vsys;

const BOARD_WIDTH: usize = 10;
const BOARD_HEIGHT: usize = 20;
const FRAME_WIDTH: u32 = 448;
const FRAME_HEIGHT: u32 = 408;
const FRAME_X: i32 = 520;
const FRAME_Y: i32 = 105;
const MAXIMIZED_CONTENT_SCALE: f32 = 4.0;

const BOARD_X: f32 = 28.0;
const BOARD_Y: f32 = 44.0;
const CELL: f32 = 16.0;
const SIDE_X: f32 = 222.0;
const PLAYABLE_CONTENT_BOTTOM: f32 = BOARD_Y + CELL * BOARD_HEIGHT as f32 + 10.0;

const BG: [u8; 4] = [8, 16, 24, 255];
const PANEL: [u8; 4] = [15, 29, 42, 255];
const GRID: [u8; 4] = [19, 36, 50, 255];
const TEXT: [u8; 4] = [220, 238, 247, 255];
const MUTED: [u8; 4] = [101, 124, 139, 255];
const GOOD: [u8; 4] = [89, 226, 166, 255];
const BAD: [u8; 4] = [255, 103, 112, 255];

const MATERIAL_COLORS: [[u8; 4]; 6] = [
    [45, 111, 149, 255],
    [41, 127, 159, 255],
    [49, 139, 147, 255],
    [53, 118, 171, 255],
    [61, 104, 157, 255],
    [43, 133, 132, 255],
];

fn piece_color(kind: PieceKind) -> [u8; 4] {
    match kind {
        PieceKind::I => [69, 218, 235, 255],
        PieceKind::O => [250, 211, 80, 255],
        PieceKind::T => [187, 111, 239, 255],
        PieceKind::S => [91, 212, 120, 255],
        PieceKind::Z => [244, 91, 105, 255],
        PieceKind::J => [83, 132, 239, 255],
        PieceKind::L => [247, 155, 69, 255],
    }
}

fn main() {
    let Ok(mut frame) = Frame::open_streaming(FRAME_X, FRAME_Y, FRAME_WIDTH, FRAME_HEIGHT) else {
        logl::log(level::ERROR, "cut_tetris: frame create failed");
        return;
    };

    logl::log(
        level::INFO,
        "cut_tetris: arrows move/spin, space or enter cuts, R restarts, Esc exits",
    );
    let mut seed = 0xC07C_7E75;
    let mut game = Game::<BOARD_WIDTH, BOARD_HEIGHT>::new(seed);
    let mut renderer = Renderer::new();
    if let Err(error) = renderer.present(&mut frame, &game) {
        logl::log(
            level::ERROR,
            format_args!("cut_tetris: first frame failed: {error:?}"),
        );
        return;
    }
    let mut repaint_ms = 0_u32;

    'running: loop {
        let mut resized = false;
        loop {
            let resize = match frame.take_resize_event() {
                Ok(Some(resize)) => resize,
                Ok(None) => break,
                Err(error) => {
                    logl::log(
                        level::ERROR,
                        format_args!("cut_tetris: resize event failed: {error:?}"),
                    );
                    break;
                }
            };
            if resize.width == frame.width() && resize.height == frame.height() {
                continue;
            }
            if let Err(error) = frame.resize(resize.width, resize.height) {
                logl::log(
                    level::ERROR,
                    format_args!(
                        "cut_tetris: resize {}x{} -> {}x{} failed: {error:?}",
                        resize.old_width, resize.old_height, resize.width, resize.height
                    ),
                );
                continue;
            }
            renderer.resize(resize.width, resize.height);
            resized = true;
            logl::log(
                level::INFO,
                format_args!(
                    "cut_tetris: resized {}x{} -> {}x{} content_scale={}x",
                    resize.old_width,
                    resize.old_height,
                    resize.width,
                    resize.height,
                    renderer.content_scale() as u32
                ),
            );
        }

        while let Some(event) = input::pop_keyboard_output() {
            if event.kind == input::KEYBOARD_OUTPUT_KIND_KEY {
                match event.key_code {
                    input::KEYBOARD_KEY_ARROW_LEFT => {
                        game.move_left();
                    }
                    input::KEYBOARD_KEY_ARROW_RIGHT => {
                        game.move_right();
                    }
                    input::KEYBOARD_KEY_ARROW_UP => {
                        game.rotate(Rotation::Cw);
                    }
                    input::KEYBOARD_KEY_ARROW_DOWN => {
                        game.rotate(Rotation::Ccw);
                    }
                    input::KEYBOARD_KEY_SPACE | input::KEYBOARD_KEY_ENTER => {
                        if game.is_game_over() {
                            seed = seed.wrapping_add(0x9E37_79B9);
                            game = Game::new(seed);
                        } else {
                            let _ = game.cut();
                        }
                    }
                    input::KEYBOARD_KEY_ESCAPE => break 'running,
                    _ => {}
                }
            } else if event.kind == input::KEYBOARD_OUTPUT_KIND_TEXT {
                let ch = char::from_u32(event.codepoint).unwrap_or('\0');
                match ch {
                    'r' | 'R' => {
                        seed = seed.wrapping_add(0x9E37_79B9);
                        game = Game::new(seed);
                    }
                    'a' | 'A' => {
                        game.move_left();
                    }
                    'd' | 'D' => {
                        game.move_right();
                    }
                    'w' | 'W' => {
                        game.rotate(Rotation::Cw);
                    }
                    's' | 'S' => {
                        game.rotate(Rotation::Ccw);
                    }
                    _ => {}
                }
            }
        }

        game.tick(16);
        repaint_ms = repaint_ms.saturating_add(16);
        if resized || game.consume_changed() || repaint_ms >= 50 {
            repaint_ms = 0;
            if let Err(error) = renderer.present(&mut frame, &game) {
                logl::log(
                    level::ERROR,
                    format_args!("cut_tetris: frame publish failed: {error:?}"),
                );
                break;
            }
        }
        vsys::poll_once();
        vsys::sleep_ms(16);
    }
}

struct Renderer {
    canvas: Canvas,
}

impl Renderer {
    fn new() -> Self {
        Self {
            canvas: Canvas::new(FRAME_WIDTH, FRAME_HEIGHT),
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.canvas.resize(width, height);
    }

    fn content_scale(&self) -> f32 {
        self.canvas.scale
    }

    fn present(
        &mut self,
        frame: &mut Frame,
        game: &Game<BOARD_WIDTH, BOARD_HEIGHT>,
    ) -> Result<(), UiError> {
        let width = frame.width();
        let height = frame.height();
        self.canvas.clear(BG);
        draw_game(&mut self.canvas, game);
        frame.begin(rgba(BG[0], BG[1], BG[2], BG[3]))?;
        frame.write_opaque_rgba8(self.canvas.pixels.as_slice())?;
        frame.publish(Damage::full(width, height))
    }
}

fn draw_game(rects: &mut Canvas, game: &Game<BOARD_WIDTH, BOARD_HEIGHT>) {
    rect(rects, 0.0, 0.0, FRAME_WIDTH as f32, FRAME_HEIGHT as f32, BG);
    rect(
        rects,
        BOARD_X - 4.0,
        BOARD_Y - 4.0,
        CELL * BOARD_WIDTH as f32 + 8.0,
        CELL * BOARD_HEIGHT as f32 + 8.0,
        MUTED,
    );
    rect(
        rects,
        BOARD_X,
        BOARD_Y,
        CELL * BOARD_WIDTH as f32,
        CELL * BOARD_HEIGHT as f32,
        PANEL,
    );

    let target = game.cut_ready().then(|| game.target_cells()).flatten();
    for y in 0..BOARD_HEIGHT {
        for x in 0..BOARD_WIDTH {
            let px = BOARD_X + x as f32 * CELL;
            let py = BOARD_Y + y as f32 * CELL;
            let danger = y + 1 == BOARD_HEIGHT;
            let empty = if danger { [45, 25, 34, 255] } else { GRID };
            rect(rects, px + 1.0, py + 1.0, CELL - 2.0, CELL - 2.0, empty);
            if let Some(band) = game.cell_at(x, y) {
                let mut color = MATERIAL_COLORS[band as usize % MATERIAL_COLORS.len()];
                if target.as_ref().is_some_and(|cells| cells.contains(&(x, y))) {
                    color = piece_color(game.cutter().kind);
                }
                rect(rects, px + 1.0, py + 1.0, CELL - 2.0, CELL - 2.0, color);
                rect(rects, px + 3.0, py + 3.0, CELL - 7.0, 2.0, brighten(color));
            }
        }
    }

    let cutter = game.cutter();
    let (cutter_width, _) = cutter.dimensions();
    rect(
        rects,
        BOARD_X + cutter.x as f32 * CELL,
        BOARD_Y + BOARD_HEIGHT as f32 * CELL + 6.0,
        cutter_width as f32 * CELL,
        4.0,
        if target.is_some() { GOOD } else { BAD },
    );

    draw_text(rects, "CUT TETRIS", 28.0, 15.0, 3.0, TEXT);
    draw_text(rects, "CUTTER", SIDE_X, 47.0, 2.0, MUTED);
    draw_piece(
        rects,
        cutter,
        SIDE_X + 10.0,
        67.0,
        13.0,
        piece_color(cutter.kind),
    );
    draw_text(
        rects,
        if target.is_some() { "FIT" } else { "WAIT" },
        SIDE_X + 76.0,
        82.0,
        2.0,
        if target.is_some() { GOOD } else { BAD },
    );

    draw_text(rects, "NEXT", SIDE_X, 135.0, 2.0, MUTED);
    draw_piece_kind(
        rects,
        game.next_piece(),
        SIDE_X + 10.0,
        155.0,
        13.0,
        piece_color(game.next_piece()),
    );

    draw_text(rects, "SCORE", SIDE_X, 220.0, 2.0, MUTED);
    draw_number(rects, game.score(), SIDE_X, 239.0, 2.0, TEXT);
    draw_text(rects, "CUTS", SIDE_X, 272.0, 2.0, MUTED);
    draw_number(rects, game.cuts(), SIDE_X, 291.0, 2.0, TEXT);
    draw_text(rects, "ROWS", SIDE_X + 92.0, 272.0, 2.0, MUTED);
    draw_number(rects, game.rows_spawned(), SIDE_X + 92.0, 291.0, 2.0, TEXT);

    draw_text(rects, "LIVES", SIDE_X, 314.0, 1.0, MUTED);
    let lives_remaining = game.lives_remaining();
    for life in 0..game.miss_limit() {
        rect(
            rects,
            SIDE_X + 28.0 + life as f32 * 14.0,
            313.0,
            11.0,
            8.0,
            if life < lives_remaining {
                GOOD
            } else {
                [55, 29, 37, 255]
            },
        );
    }

    draw_text(rects, "NEXT ROW", SIDE_X, 329.0, 1.0, MUTED);
    rect(rects, SIDE_X, 341.0, 184.0, 8.0, GRID);
    let progress = game.row_elapsed_ms() as f32 / game.row_interval_ms() as f32;
    rect(
        rects,
        SIDE_X,
        341.0,
        184.0 * progress.clamp(0.0, 1.0),
        8.0,
        BAD,
    );

    draw_text(
        rects,
        "LEFT RIGHT MOVE  UP DOWN SPIN  SPACE CUT",
        28.0,
        385.0,
        1.0,
        MUTED,
    );

    if game.is_game_over() {
        rect(
            rects,
            BOARD_X + 10.0,
            BOARD_Y + 133.0,
            CELL * BOARD_WIDTH as f32 - 20.0,
            58.0,
            [72, 20, 31, 255],
        );
        draw_text(
            rects,
            "GAME OVER",
            BOARD_X + 26.0,
            BOARD_Y + 146.0,
            3.0,
            BAD,
        );
        draw_text(
            rects,
            "ENTER OR R",
            BOARD_X + 45.0,
            BOARD_Y + 176.0,
            1.0,
            TEXT,
        );
    }
}

struct Canvas {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    scale: f32,
    origin_x: f32,
    origin_y: f32,
}

impl Canvas {
    fn new(width: u32, height: u32) -> Self {
        let mut canvas = Self {
            pixels: Vec::new(),
            width: 0,
            height: 0,
            scale: 1.0,
            origin_x: 0.0,
            origin_y: 0.0,
        };
        canvas.resize(width, height);
        canvas
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.scale = if width == FRAME_WIDTH && height == FRAME_HEIGHT {
            1.0
        } else {
            MAXIMIZED_CONTENT_SCALE
        };
        if self.scale == 1.0 {
            self.origin_x = 0.0;
            self.origin_y = 0.0;
        } else {
            // UI4's 2560x1440 maximize target is 192 pixels shorter than the
            // complete 4x decoration canvas. Center horizontally and anchor
            // the cutter at the bottom: title, side panel, and the complete
            // 4x 10x20 play board remain visible; only the help footer clips.
            self.origin_x = (width as f32 - FRAME_WIDTH as f32 * self.scale) * 0.5;
            self.origin_y = height as f32 - PLAYABLE_CONTENT_BOTTOM * self.scale;
        }
        self.pixels.resize(width as usize * height as usize * 4, 0);
    }

    fn clear(&mut self, color: [u8; 4]) {
        for pixel in self.pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&color);
        }
    }

    fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: [u8; 4]) {
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let x = self.origin_x + x * self.scale;
        let y = self.origin_y + y * self.scale;
        let width = width * self.scale;
        let height = height * self.scale;
        let left = (x.max(0.0) as usize).min(self.width as usize);
        let top = (y.max(0.0) as usize).min(self.height as usize);
        let right = ((x + width).max(0.0) as usize).min(self.width as usize);
        let bottom = ((y + height).max(0.0) as usize).min(self.height as usize);
        if left >= right || top >= bottom {
            return;
        }
        for pixel_y in top..bottom {
            let start = (pixel_y * self.width as usize + left) * 4;
            let end = (pixel_y * self.width as usize + right) * 4;
            for pixel in self.pixels[start..end].chunks_exact_mut(4) {
                pixel.copy_from_slice(&color);
            }
        }
    }
}

fn rect(canvas: &mut Canvas, x: f32, y: f32, width: f32, height: f32, color: [u8; 4]) {
    canvas.rect(x, y, width, height, color);
}

fn brighten(color: [u8; 4]) -> [u8; 4] {
    [
        color[0].saturating_add(38),
        color[1].saturating_add(38),
        color[2].saturating_add(38),
        255,
    ]
}

fn draw_piece_kind(rects: &mut Canvas, kind: PieceKind, x: f32, y: f32, cell: f32, color: [u8; 4]) {
    draw_piece(
        rects,
        Cutter {
            kind,
            rotation: 0,
            x: 0,
        },
        x,
        y,
        cell,
        color,
    );
}

fn draw_piece(rects: &mut Canvas, cutter: Cutter, x: f32, y: f32, cell: f32, color: [u8; 4]) {
    let (_, height) = piece_dimensions(cutter.kind, cutter.rotation);
    for (cell_x, cell_y) in piece_cells(cutter.kind, cutter.rotation) {
        rect(
            rects,
            x + cell_x as f32 * cell,
            y + (height - 1 - cell_y as usize) as f32 * cell,
            cell - 2.0,
            cell - 2.0,
            color,
        );
    }
}

fn draw_number(rects: &mut Canvas, mut value: u32, x: f32, y: f32, scale: f32, color: [u8; 4]) {
    let mut digits = [0_u8; 10];
    let mut len = 0_usize;
    loop {
        digits[len] = (value % 10) as u8;
        len += 1;
        value /= 10;
        if value == 0 || len == digits.len() {
            break;
        }
    }
    for index in 0..len {
        let digit = digits[len - index - 1];
        draw_glyph(
            rects,
            b'0' + digit,
            x + index as f32 * scale * 4.0,
            y,
            scale,
            color,
        );
    }
}

fn draw_text(rects: &mut Canvas, text: &str, x: f32, y: f32, scale: f32, color: [u8; 4]) {
    for (index, byte) in text.bytes().enumerate() {
        draw_glyph(rects, byte, x + index as f32 * scale * 4.0, y, scale, color);
    }
}

fn draw_glyph(rects: &mut Canvas, byte: u8, x: f32, y: f32, scale: f32, color: [u8; 4]) {
    let rows = glyph(byte);
    for (row, bits) in rows.into_iter().enumerate() {
        for column in 0..3 {
            if bits & (1 << (2 - column)) != 0 {
                rect(
                    rects,
                    x + column as f32 * scale,
                    y + row as f32 * scale,
                    scale,
                    scale,
                    color,
                );
            }
        }
    }
}

fn glyph(byte: u8) -> [u8; 5] {
    match byte.to_ascii_uppercase() {
        b'0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        b'1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        b'2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        b'3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        b'4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        b'5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        b'6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        b'7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        b'8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        b'9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        b'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        b'C' => [0b111, 0b100, 0b100, 0b100, 0b111],
        b'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        b'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        b'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        b'G' => [0b111, 0b100, 0b101, 0b101, 0b111],
        b'H' => [0b101, 0b101, 0b111, 0b101, 0b101],
        b'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
        b'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        b'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        b'N' => [0b101, 0b111, 0b111, 0b111, 0b101],
        b'O' => [0b111, 0b101, 0b101, 0b101, 0b111],
        b'P' => [0b110, 0b101, 0b110, 0b100, 0b100],
        b'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        b'S' => [0b111, 0b100, 0b111, 0b001, 0b111],
        b'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        b'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
        b'V' => [0b101, 0b101, 0b101, 0b101, 0b010],
        b'W' => [0b101, 0b101, 0b111, 0b111, 0b101],
        b'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        b'Y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        _ => [0; 5],
    }
}
