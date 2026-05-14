#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use trueos::input;
use trueos::platform;
use trueos::ui2::{self, gfx};
use trueos_tetris::{Lcg32, NoopEvents, Rotation};

const TEX_ID: u32 = 4_775;
const WIN_W: u32 = 360;
const WIN_H: u32 = 300;
const CLEAR_RGB: u32 = 0x091016;
const BG: [u8; 4] = [0x09, 0x10, 0x16, 0xFF];
const PANEL: [u8; 4] = [0x12, 0x1D, 0x25, 0xFF];
const TEXT: [u8; 4] = [0xD8, 0xE8, 0xF2, 0xFF];
const MUTED: [u8; 4] = [0x55, 0x66, 0x77, 0xFF];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Menu,
    Tetris,
    Snake,
    Bejeweled,
    Minesweeper,
    Chess,
}

struct Arcade {
    mode: Mode,
    rng: Lcg32,
    tetris: trueos_tetris::Game<10, 24, 4>,
    tetris_events: NoopEvents,
    snake: trueos_tetris::snake::Game<18, 14>,
    snake_events: trueos_tetris::snake::NoopEvents,
    jewels: trueos_tetris::bejewled::Game<8, 8>,
    jewel_events: trueos_tetris::bejewled::NoopEvents,
    mines: trueos_tetris::minesweeper::Game<10, 8>,
    mine_events: trueos_tetris::minesweeper::NoopEvents,
    chess: trueos_tetris::chess::Game,
    drop_ms: u32,
    snake_ms: u32,
    cursor_x: usize,
    cursor_y: usize,
}

impl Arcade {
    fn new() -> Self {
        let mut rng = Lcg32::new(0xA7CA_DE01);
        let mut tetris_events = NoopEvents;
        let mut snake_events = trueos_tetris::snake::NoopEvents;
        let mut jewel_events = trueos_tetris::bejewled::NoopEvents;
        Self {
            mode: Mode::Menu,
            tetris: trueos_tetris::Game::new(&mut rng, &mut tetris_events),
            snake: trueos_tetris::snake::Game::new(&mut rng, &mut snake_events),
            jewels: trueos_tetris::bejewled::Game::new(&mut rng, &mut jewel_events),
            mines: trueos_tetris::minesweeper::Game::new(trueos_tetris::minesweeper::Config::new(
                12,
            ))
            .unwrap(),
            chess: trueos_tetris::chess::Game::new(),
            rng,
            tetris_events,
            snake_events,
            jewel_events,
            mine_events: trueos_tetris::minesweeper::NoopEvents,
            drop_ms: 0,
            snake_ms: 0,
            cursor_x: 0,
            cursor_y: 0,
        }
    }

    fn reset_mode(&mut self) {
        match self.mode {
            Mode::Tetris => {
                self.tetris = trueos_tetris::Game::new(&mut self.rng, &mut self.tetris_events);
                self.drop_ms = 0;
            }
            Mode::Snake => self.snake.reset(&mut self.rng, &mut self.snake_events),
            Mode::Bejeweled => self.jewels.reset(&mut self.rng, &mut self.jewel_events),
            Mode::Minesweeper => self.mines.reset(),
            Mode::Chess => self.chess = trueos_tetris::chess::Game::new(),
            Mode::Menu => {}
        }
    }

    fn handle_char(&mut self, ch: char) {
        match ch {
            '1' => self.mode = Mode::Tetris,
            '2' => self.mode = Mode::Snake,
            '3' => self.mode = Mode::Bejeweled,
            '4' => self.mode = Mode::Minesweeper,
            '5' => self.mode = Mode::Chess,
            '0' | 'm' | 'M' => self.mode = Mode::Menu,
            'r' | 'R' => self.reset_mode(),
            'a' | 'h' | 'H' => self.left(),
            'd' | 'l' | 'L' => self.right(),
            'w' | 'k' | 'K' => self.up(),
            's' | 'j' | 'J' => self.down(),
            'z' | 'Z' => {
                if self.mode == Mode::Tetris {
                    let _ = self.tetris.rotate(Rotation::Ccw);
                }
            }
            ' ' => self.primary(),
            _ => {}
        }
    }

    fn left(&mut self) {
        match self.mode {
            Mode::Tetris => {
                let _ = self.tetris.move_left();
            }
            Mode::Snake => {
                let _ = self
                    .snake
                    .set_direction(trueos_tetris::snake::Direction::Left);
            }
            _ => self.cursor_x = self.cursor_x.saturating_sub(1),
        }
    }

    fn right(&mut self) {
        match self.mode {
            Mode::Tetris => {
                let _ = self.tetris.move_right();
            }
            Mode::Snake => {
                let _ = self
                    .snake
                    .set_direction(trueos_tetris::snake::Direction::Right);
            }
            Mode::Bejeweled => self.cursor_x = (self.cursor_x + 1).min(7),
            Mode::Minesweeper => self.cursor_x = (self.cursor_x + 1).min(9),
            _ => self.cursor_x = self.cursor_x.saturating_add(1).min(7),
        }
    }

    fn up(&mut self) {
        match self.mode {
            Mode::Tetris => {
                let _ = self.tetris.rotate(Rotation::Cw);
            }
            Mode::Snake => {
                let _ = self
                    .snake
                    .set_direction(trueos_tetris::snake::Direction::Up);
            }
            _ => self.cursor_y = self.cursor_y.saturating_sub(1),
        }
    }

    fn down(&mut self) {
        match self.mode {
            Mode::Tetris => {
                let _ = self
                    .tetris
                    .soft_drop(&mut self.rng, &mut self.tetris_events);
            }
            Mode::Snake => {
                let _ = self
                    .snake
                    .set_direction(trueos_tetris::snake::Direction::Down);
            }
            Mode::Bejeweled => self.cursor_y = (self.cursor_y + 1).min(7),
            Mode::Minesweeper => self.cursor_y = (self.cursor_y + 1).min(7),
            _ => self.cursor_y = self.cursor_y.saturating_add(1).min(7),
        }
    }

    fn primary(&mut self) {
        match self.mode {
            Mode::Tetris => {
                let _ = self
                    .tetris
                    .hard_drop(&mut self.rng, &mut self.tetris_events);
            }
            Mode::Bejeweled => {
                let bx = (self.cursor_x + 1).min(7);
                let _ = self.jewels.swap(
                    self.cursor_x,
                    self.cursor_y,
                    bx,
                    self.cursor_y,
                    &mut self.rng,
                    &mut self.jewel_events,
                );
            }
            Mode::Minesweeper => {
                let _ = self.mines.reveal(
                    self.cursor_x.min(9),
                    self.cursor_y.min(7),
                    &mut self.rng,
                    &mut self.mine_events,
                );
            }
            _ => {}
        }
    }

    fn key(&mut self, key_code: u16) {
        match key_code {
            input::KEYBOARD_KEY_ARROW_LEFT => self.left(),
            input::KEYBOARD_KEY_ARROW_RIGHT => self.right(),
            input::KEYBOARD_KEY_ARROW_UP => self.up(),
            input::KEYBOARD_KEY_ARROW_DOWN => self.down(),
            input::KEYBOARD_KEY_SPACE | input::KEYBOARD_KEY_ENTER => self.primary(),
            input::KEYBOARD_KEY_ESCAPE => self.mode = Mode::Menu,
            _ => {}
        }
    }

    fn tick(&mut self, elapsed_ms: u32) {
        match self.mode {
            Mode::Tetris => {
                if self.tetris.is_game_over() {
                    self.reset_mode();
                    return;
                }
                self.drop_ms = self.drop_ms.saturating_add(elapsed_ms);
                while self.drop_ms >= self.tetris.level.level_speed_seconds() {
                    self.drop_ms -= self.tetris.level.level_speed_seconds();
                    let _ = self
                        .tetris
                        .soft_drop(&mut self.rng, &mut self.tetris_events);
                }
            }
            Mode::Snake => {
                self.snake_ms = self.snake_ms.saturating_add(elapsed_ms);
                if self.snake_ms >= 130 {
                    self.snake_ms = 0;
                    let _ = self.snake.tick(&mut self.rng, &mut self.snake_events);
                }
            }
            _ => {}
        }
    }
}

fn push_rect(vertices: &mut Vec<gfx::RgbVertex>, x: u32, y: u32, w: u32, h: u32, color: [u8; 4]) {
    if w == 0 || h == 0 {
        return;
    }
    let x0 = (x as f32 / WIN_W as f32) * 2.0 - 1.0;
    let y0 = (y as f32 / WIN_H as f32) * 2.0 - 1.0;
    let x1 = ((x + w) as f32 / WIN_W as f32) * 2.0 - 1.0;
    let y1 = ((y + h) as f32 / WIN_H as f32) * 2.0 - 1.0;
    let mk = |x: f32, y: f32| gfx::RgbVertex::new(x, y, color);
    vertices.extend_from_slice(&[
        mk(x0, y0),
        mk(x1, y0),
        mk(x1, y1),
        mk(x0, y0),
        mk(x1, y1),
        mk(x0, y1),
    ]);
}

fn draw_digit(
    vertices: &mut Vec<gfx::RgbVertex>,
    digit: u8,
    x: u32,
    y: u32,
    scale: u32,
    color: [u8; 4],
) {
    const MAP: [[u8; 15]; 6] = [
        *b"111101101101111",
        *b"010110010010111",
        *b"111001111100111",
        *b"111001111001111",
        *b"101101111001001",
        *b"111100111001111",
    ];
    let pattern = MAP[digit.min(5) as usize];
    for row in 0..5 {
        for col in 0..3 {
            if pattern[row * 3 + col] == b'1' {
                push_rect(
                    vertices,
                    x + col as u32 * scale,
                    y + row as u32 * scale,
                    scale,
                    scale,
                    color,
                );
            }
        }
    }
}

fn draw_menu(app: &Arcade, vertices: &mut Vec<gfx::RgbVertex>) {
    let colors = [
        [0x55, 0xB8, 0xFF, 0xFF],
        [0x66, 0xD1, 0x8F, 0xFF],
        [0xF0, 0x8A, 0xB8, 0xFF],
        [0xF2, 0xC1, 0x4E, 0xFF],
        [0xC4, 0xA7, 0xFF, 0xFF],
    ];
    let _ = app;
    push_rect(vertices, 0, 0, WIN_W, WIN_H, BG);
    for i in 0..5 {
        let y = 34 + i as u32 * 48;
        push_rect(vertices, 34, y, 292, 34, PANEL);
        draw_digit(vertices, (i + 1) as u8, 48, y + 6, 4, colors[i]);
        push_rect(vertices, 78, y + 11, 220 - i as u32 * 18, 12, colors[i]);
    }
}

fn draw_tetris(app: &Arcade, vertices: &mut Vec<gfx::RgbVertex>) {
    push_rect(vertices, 0, 0, WIN_W, WIN_H, BG);
    let ox = 118;
    let oy = 18;
    let cell = 11;
    push_rect(vertices, ox - 3, oy - 3, 10 * cell + 6, 20 * cell + 6, MUTED);
    for y in 0..20 {
        for x in 0..10 {
            if let Some(c) = app.tetris.cell_view_at(x, y + 4, true) {
                push_rect(
                    vertices,
                    ox + x as u32 * cell,
                    oy + y as u32 * cell,
                    cell - 1,
                    cell - 1,
                    [c.color.r, c.color.g, c.color.b, 0xFF],
                );
            }
        }
    }
}

fn draw_snake(app: &Arcade, vertices: &mut Vec<gfx::RgbVertex>) {
    push_rect(vertices, 0, 0, WIN_W, WIN_H, BG);
    let ox = 72;
    let oy = 62;
    let cell = 12;
    for y in 0..14 {
        for x in 0..18 {
            let color = match app.snake.cell_kind_at(x, y).unwrap() {
                trueos_tetris::snake::CellKind::Empty => [0x12, 0x1A, 0x20, 0xFF],
                trueos_tetris::snake::CellKind::Body => [0x39, 0xC4, 0x78, 0xFF],
                trueos_tetris::snake::CellKind::Head => [0xA8, 0xFF, 0xB8, 0xFF],
                trueos_tetris::snake::CellKind::Food => [0xF6, 0x5B, 0x5B, 0xFF],
            };
            push_rect(
                vertices,
                ox + x as u32 * cell,
                oy + y as u32 * cell,
                cell - 1,
                cell - 1,
                color,
            );
        }
    }
}

fn gem_color(gem: trueos_tetris::bejewled::GemKind) -> [u8; 4] {
    match gem {
        trueos_tetris::bejewled::GemKind::Ruby => [0xE8, 0x4A, 0x5F, 0xFF],
        trueos_tetris::bejewled::GemKind::Sapphire => [0x4A, 0x9D, 0xFF, 0xFF],
        trueos_tetris::bejewled::GemKind::Emerald => [0x48, 0xD4, 0x8A, 0xFF],
        trueos_tetris::bejewled::GemKind::Topaz => [0xF2, 0xC1, 0x4E, 0xFF],
        trueos_tetris::bejewled::GemKind::Diamond => [0xD8, 0xF4, 0xFF, 0xFF],
        trueos_tetris::bejewled::GemKind::Amethyst => [0xB7, 0x78, 0xFF, 0xFF],
    }
}

fn draw_bejeweled(app: &Arcade, vertices: &mut Vec<gfx::RgbVertex>) {
    push_rect(vertices, 0, 0, WIN_W, WIN_H, BG);
    let ox = 98;
    let oy = 42;
    let cell = 22;
    for y in 0..8 {
        for x in 0..8 {
            let mut color = gem_color(app.jewels.gem_at(x, y).unwrap());
            if app.cursor_x == x && app.cursor_y == y {
                color = TEXT;
            }
            push_rect(
                vertices,
                ox + x as u32 * cell,
                oy + y as u32 * cell,
                cell - 3,
                cell - 3,
                color,
            );
        }
    }
}

fn draw_mines(app: &Arcade, vertices: &mut Vec<gfx::RgbVertex>) {
    push_rect(vertices, 0, 0, WIN_W, WIN_H, BG);
    let ox = 80;
    let oy = 52;
    let cell = 20;
    for y in 0..8 {
        for x in 0..10 {
            let view = app.mines.cell_view_at(x, y).unwrap();
            let color = if app.cursor_x == x && app.cursor_y == y {
                TEXT
            } else if view.exploded {
                [0xFF, 0x55, 0x55, 0xFF]
            } else if view.is_revealed {
                [0x80, 0x92, 0xA0, 0xFF]
            } else if view.is_flagged {
                [0xF2, 0xC1, 0x4E, 0xFF]
            } else {
                PANEL
            };
            push_rect(
                vertices,
                ox + x as u32 * cell,
                oy + y as u32 * cell,
                cell - 2,
                cell - 2,
                color,
            );
        }
    }
}

fn draw_chess(app: &Arcade, vertices: &mut Vec<gfx::RgbVertex>) {
    push_rect(vertices, 0, 0, WIN_W, WIN_H, BG);
    let ox = 100;
    let oy = 50;
    let cell = 22;
    for rank in 0..8 {
        for file in 0..8 {
            let light = ((file + rank) & 1) == 0;
            push_rect(
                vertices,
                ox + file as u32 * cell,
                oy + rank as u32 * cell,
                cell,
                cell,
                if light {
                    [0xB8, 0xC7, 0xB0, 0xFF]
                } else {
                    [0x5E, 0x75, 0x68, 0xFF]
                },
            );
            let sq = trueos_tetris::chess::Square::new(file as u8, 7 - rank as u8).unwrap();
            if let Some(piece) = app.chess.piece_at(sq) {
                let color = match piece.color {
                    trueos_tetris::chess::Color::White => [0xF6, 0xF1, 0xD8, 0xFF],
                    trueos_tetris::chess::Color::Black => [0x18, 0x1D, 0x24, 0xFF],
                };
                push_rect(
                    vertices,
                    ox + file as u32 * cell + 5,
                    oy + rank as u32 * cell + 5,
                    cell - 10,
                    cell - 10,
                    color,
                );
            }
        }
    }
}

fn render(app: &Arcade, window: &ui2::SurfaceWindow) {
    let mut vertices = Vec::with_capacity(4096);
    match app.mode {
        Mode::Menu => draw_menu(app, &mut vertices),
        Mode::Tetris => draw_tetris(app, &mut vertices),
        Mode::Snake => draw_snake(app, &mut vertices),
        Mode::Bejeweled => draw_bejeweled(app, &mut vertices),
        Mode::Minesweeper => draw_mines(app, &mut vertices),
        Mode::Chess => draw_chess(app, &mut vertices),
    }
    let _ = window.render_rgb_triangles(CLEAR_RGB, vertices.as_slice());
}

fn drain_input(app: &mut Arcade) {
    while let Some(event) = input::pop_keyboard_output() {
        if event.kind == input::KEYBOARD_OUTPUT_KIND_TEXT {
            let len = (event.utf8_len as usize).min(event.utf8.len());
            if len != 0 {
                if let Ok(text) = core::str::from_utf8(&event.utf8[..len]) {
                    for ch in text.chars() {
                        app.handle_char(ch);
                    }
                }
            } else if let Some(ch) = char::from_u32(event.codepoint) {
                app.handle_char(ch);
            }
        } else if event.kind == input::KEYBOARD_OUTPUT_KIND_KEY {
            app.key(event.key_code);
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn main() {
    let Some(window) = ui2::SurfaceWindow::create(
        "TRUEOS Arcade",
        ui2::Rect {
            x: 530,
            y: 120,
            width: WIN_W,
            height: WIN_H,
        },
        TEX_ID,
    ) else {
        trueos::globalog::log_with_level(
            trueos::globalog::level::ERROR,
            "full_tetris bp: surface window create failed\n",
        );
        return;
    };

    let mut app = Arcade::new();
    loop {
        drain_input(&mut app);
        app.tick(16);
        render(&app, &window);
        platform::sleep_ms(16);
    }
}
