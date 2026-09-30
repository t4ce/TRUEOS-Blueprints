// trueos-blueprint: features = ["tokio-runtime"]

extern crate alloc;

mod audio;
mod chess;
mod chess_assets;
mod mines;
mod sudoku;
mod tic;

use alloc::{format, vec::Vec};
use audio::Audio;
use chess::Chess;
use chess_assets::ChessAssets;
use cozy_chess::{Color as ChessColor, GameStatus as ChessStatus};
use gamie::minesweeper::{CellStatus as MineCellStatus, Status as MineStatus};
use gamie::tictactoe::{Player as TicPlayer, Status as TicStatus};
use microgames::{Game, Lcg32, NoopEvents, Rgb8, Rotation};
use mines::Minefield;
use sudoku::Sudoku;
use tic::TicTacToe;
use trueos::input;
use trueos::logl::{self, level};
use trueos::ui4_scene::{
    BackgroundLayer, Damage, Error as UiError, Font, FontCanvasRow, Frame, POINTER_BUTTON_PRIMARY,
    SpriteCorner, SpriteQuad, output_dimensions, rgba,
};
use trueos::vgpu::{
    BUFFER_USAGE_INDEX, BUFFER_USAGE_MAP_READ, BUFFER_USAGE_MAP_WRITE, BUFFER_USAGE_VERTEX,
    Capabilities, Device, Queue, QueueClass, RETAINED_MESH_FLAG_DOUBLE_SIDED,
    RETAINED_TOPOLOGY_CUBE_PATCHLIST_1, RETAINED_VERTEX_LAYOUT_CUBE_PATCH_SEED, RetainedDrawRange,
    RetainedFrameSubmit, RetainedFrameSubmitV2, RetainedFrameSubmitV3, RetainedMesh,
    RetainedMeshDescriptor, RetainedTransformSeed,
};
use trueos::{clock, vsys};
use trueos_picasso::cam::{Camera, Projection, Quaternion};

const WIDTH: u32 = 650;
const HEIGHT: u32 = 740;
const COLS: usize = 10;
const ROWS: usize = 24;
const HIDDEN: usize = 4;
const VISIBLE: usize = ROWS - HIDDEN;
const CELL: f32 = 1.08;
const TIC_STEP: f32 = 3.6;
const TIC_X: f32 = -5.9;
const TIC_Y: f32 = 3.6;
const SUD_STEP: f32 = 1.2;
const SUD_X: f32 = -7.15;
const SUD_Y: f32 = 4.8;
const CHESS_STEP: f32 = 1.35;
const CHESS_X: f32 = -7.15;
const CHESS_Y: f32 = 4.725;
const GAME_CUBE_SCALE: f32 = 0.46;
const BORDER_CUBE_SCALE: f32 = GAME_CUBE_SCALE / 4.0;
const BORDER_STEP: f32 = CELL / 4.0;
const MAX_BORDER_SEGMENTS: usize = 600;
const MAX_SEEDS: usize = 8192;
const CUBE_VERTICES: [u8; 12] = [0; 12];
const CUBE_INDICES: [u8; 44 * 4] = [0; 44 * 4];
const CUSTOM_RGB555: u32 = 1 << 15;
const BG: u32 = u32::from_le_bytes([11, 17, 30, 128]);
const MINE_GLYPH_SPRITE: u32 = 1;
const MINE_GLYPHS: &[u8; 10] = b"12345678FX";
const MINE_TILE_WIDTH: usize = microfont::FWIDTH + 4;
const MINE_TILE_HEIGHT: usize = microfont::FHEIGHT + 4;
const MINE_ATLAS_WIDTH: usize = MINE_GLYPHS.len() * MINE_TILE_WIDTH;
const SCENE_WIDTH: f32 = SCENE_HEIGHT * WIDTH as f32 / HEIGHT as f32;
const SCENE_HEIGHT: f32 = 25.0;

type Tetris = Game<COLS, ROWS, HIDDEN>;

#[derive(Clone, Copy, Eq, PartialEq)]
enum Mode {
    Tetris,
    Minesweeper,
    TicTacToe,
    Sudoku,
    Chess,
}

enum Error {
    Ui(&'static str, UiError),
    Gpu(&'static str, i32),
}

struct Layout {
    scale: f32,
    offset_x: f32,
    offset_y: f32,
    scene_width: f32,
    scene_height: f32,
}

impl Layout {
    fn fit(width: u32, height: u32) -> Self {
        let scale = (width as f32 / WIDTH as f32)
            .min(height as f32 / HEIGHT as f32)
            .max(0.01);
        Self {
            scale,
            offset_x: (width as f32 - WIDTH as f32 * scale) * 0.5,
            offset_y: (height as f32 - HEIGHT as f32 * scale) * 0.5,
            scene_width: SCENE_WIDTH * width as f32 / (WIDTH as f32 * scale),
            scene_height: SCENE_HEIGHT * height as f32 / (HEIGHT as f32 * scale),
        }
    }

    fn row<'a>(&self, text: &'a str, x: f32, y: f32, pixels: f32, color: u32) -> FontCanvasRow<'a> {
        FontCanvasRow {
            text,
            x: self.offset_x + x * self.scale,
            y: self.offset_y + y * self.scale,
            font_pixels: pixels * self.scale,
            color_rgba: color,
        }
    }
}

struct HullBoard {
    device: Device,
    queue: Queue,
    vertices: trueos::vgpu::Buffer,
    indices: trueos::vgpu::Buffer,
    mesh: RetainedMesh,
    seeds: trueos::vgpu::Buffer,
    previous_view_projection: [f32; 16],
}

fn main() {
    if let Err(error) = run() {
        match error {
            Error::Ui(stage, cause) => {
                logl::log(level::ERROR, format_args!("Games: {stage}: {cause:?}"))
            }
            Error::Gpu(stage, code) => {
                logl::log(level::ERROR, format_args!("Games: {stage}: GPU {code}"))
            }
        }
    }
}

fn run() -> Result<(), Error> {
    let (x, y) = output_dimensions()
        .map(|(w, h)| {
            (
                ((w.saturating_sub(WIDTH)) / 2) as i32,
                ((h.saturating_sub(HEIGHT)) / 2) as i32,
            )
        })
        .unwrap_or((80, 60));
    let mut frame =
        Frame::open_layered(x, y, WIDTH, HEIGHT, 60).map_err(|e| Error::Ui("open frame", e))?;
    upload_mine_glyphs(&mut frame)?;
    let mut background = frame
        .background()
        .map_err(|e| Error::Ui("background layer", e))?;
    let mut board = HullBoard::new()?;
    let mut rng = Lcg32::new((clock::monotonic_millis() as u32) ^ 0xC11C_7E75);
    let mut events = NoopEvents;
    let mut game = Tetris::new(&mut rng, &mut events);
    let mut minefield = Minefield::new(clock::monotonic_millis());
    let mut tic = TicTacToe::new();
    let mut sudoku = Sudoku::new(clock::monotonic_millis());
    let mut chess = Chess::new();
    let chess_assets = ChessAssets::new();
    let mut mode = Mode::Tetris;
    let mut paused = false;
    let mut last_tick = clock::monotonic_millis();
    let mut gravity_ms = 0_u64;
    let mut paint = true;
    let mut text = true;
    let mut pending_resize = None;
    let mut resize_foreground_pending = false;
    let mut resize_refresh_pending = false;
    let mut audio = Audio::new();

    loop {
        while let Some(event) = frame
            .take_resize_event()
            .map_err(|e| Error::Ui("resize event", e))?
        {
            pending_resize = Some(event);
        }
        if let Some(event) = pending_resize {
            if (event.width, event.height) == (frame.width(), frame.height()) {
                pending_resize = None;
            } else {
                match frame.resize(event.width, event.height) {
                    Ok(()) => {
                        pending_resize = None;
                        board.previous_view_projection = [0.; 16];
                        // UI4 commits the layered resize after each replacement
                        // layer has published once. Refresh again after commit.
                        resize_foreground_pending = true;
                        resize_refresh_pending = true;
                        paint = true;
                        text = true;
                    }
                    Err(UiError::Busy) => {}
                    Err(error) => return Err(Error::Ui("resize frame", error)),
                }
            }
        }
        let previous_hud = (
            game.level.total_points,
            game.level.current_level,
            game.level.rows_deleted,
            game.is_game_over(),
        );
        let tic_routes = if mode == Mode::TicTacToe {
            let routes = frame
                .input_routes()
                .map_err(|e| Error::Ui("input routes", e))?;
            if tic.sync_routes(&routes) {
                paint = true;
                text = true;
            }
            routes
        } else {
            Vec::new()
        };
        while let Some(event) = frame
            .take_keyboard_event()
            .map_err(|e| Error::Ui("keyboard", e))?
        {
            if event.kind == input::KEYBOARD_OUTPUT_KIND_KEY
                && event.flags & input::KEYBOARD_OUTPUT_FLAG_PRESS != 0
                && matches!(
                    event.key_code,
                    input::KEYBOARD_KEY_F1..=input::KEYBOARD_KEY_F5
                )
            {
                let next = match event.key_code {
                    input::KEYBOARD_KEY_F1 => Mode::Tetris,
                    input::KEYBOARD_KEY_F2 => Mode::Minesweeper,
                    input::KEYBOARD_KEY_F3 => Mode::TicTacToe,
                    input::KEYBOARD_KEY_F4 => Mode::Sudoku,
                    _ => Mode::Chess,
                };
                if select_mode(&mut frame, &mut mode, next)? {
                    paused = false;
                    paint = true;
                    text = true;
                }
            } else if event.kind == input::KEYBOARD_OUTPUT_KIND_KEY
                && event.flags & input::KEYBOARD_OUTPUT_FLAG_PRESS != 0
                && !paused
            {
                match mode {
                    Mode::Tetris if !game.is_game_over() => match event.key_code {
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
                            game.soft_drop(&mut rng, &mut events);
                        }
                        input::KEYBOARD_KEY_SPACE => {
                            game.hard_drop(&mut rng, &mut events);
                        }
                        _ => {}
                    },
                    Mode::Minesweeper if *minefield.status() == MineStatus::Ongoing => {
                        match event.key_code {
                            input::KEYBOARD_KEY_ARROW_LEFT => {
                                minefield.move_cursor(-1, 0);
                                paint = true;
                            }
                            input::KEYBOARD_KEY_ARROW_RIGHT => {
                                minefield.move_cursor(1, 0);
                                paint = true;
                            }
                            input::KEYBOARD_KEY_ARROW_UP => {
                                minefield.move_cursor(0, -1);
                                paint = true;
                            }
                            input::KEYBOARD_KEY_ARROW_DOWN => {
                                minefield.move_cursor(0, 1);
                                paint = true;
                            }
                            input::KEYBOARD_KEY_SPACE | input::KEYBOARD_KEY_ENTER => {
                                if minefield.reveal() {
                                    paint = true;
                                    text = true;
                                }
                            }
                            _ => {}
                        }
                    }
                    Mode::Chess => {
                        let changed = match event.key_code {
                            input::KEYBOARD_KEY_ARROW_LEFT => chess.move_cursor(-1, 0),
                            input::KEYBOARD_KEY_ARROW_RIGHT => chess.move_cursor(1, 0),
                            input::KEYBOARD_KEY_ARROW_UP => chess.move_cursor(0, -1),
                            input::KEYBOARD_KEY_ARROW_DOWN => chess.move_cursor(0, 1),
                            input::KEYBOARD_KEY_SPACE | input::KEYBOARD_KEY_ENTER => {
                                chess.activate()
                            }
                            _ => false,
                        };
                        if changed {
                            paint = true;
                            text = true;
                        }
                    }
                    Mode::Sudoku => {
                        let changed = match event.key_code {
                            input::KEYBOARD_KEY_ARROW_LEFT => sudoku.move_cursor(-1, 0),
                            input::KEYBOARD_KEY_ARROW_RIGHT => sudoku.move_cursor(1, 0),
                            input::KEYBOARD_KEY_ARROW_UP => sudoku.move_cursor(0, -1),
                            input::KEYBOARD_KEY_ARROW_DOWN => sudoku.move_cursor(0, 1),
                            input::KEYBOARD_KEY_BACKSPACE | input::KEYBOARD_KEY_DELETE => {
                                sudoku.erase()
                            }
                            _ => false,
                        };
                        if changed {
                            paint = true;
                            text = true;
                        }
                    }
                    Mode::TicTacToe => {
                        if let Some(player) = tic.keyboard_player(&event, &tic_routes) {
                            let moved = match event.key_code {
                                input::KEYBOARD_KEY_ARROW_LEFT => tic.move_cursor(player, -1, 0),
                                input::KEYBOARD_KEY_ARROW_RIGHT => tic.move_cursor(player, 1, 0),
                                input::KEYBOARD_KEY_ARROW_UP => tic.move_cursor(player, 0, -1),
                                input::KEYBOARD_KEY_ARROW_DOWN => tic.move_cursor(player, 0, 1),
                                input::KEYBOARD_KEY_SPACE | input::KEYBOARD_KEY_ENTER => {
                                    if tic.play_selected(player) {
                                        text = true;
                                        true
                                    } else {
                                        false
                                    }
                                }
                                _ => false,
                            };
                            paint |= moved;
                        }
                    }
                    _ => {}
                }
            } else if event.kind == input::KEYBOARD_OUTPUT_KIND_TEXT {
                match char::from_u32(event.codepoint) {
                    Some(c @ '1'..='9') if mode == Mode::Sudoku && !paused => {
                        if sudoku.set_digit(c as u8 - b'0') {
                            paint = true;
                            text = true;
                        }
                    }
                    Some('0') if mode == Mode::Sudoku && !paused => {
                        if sudoku.erase() {
                            paint = true;
                            text = true;
                        }
                    }
                    Some(c @ '1'..='5') if mode != Mode::Sudoku => {
                        let next = match c {
                            '1' => Mode::Tetris,
                            '2' => Mode::Minesweeper,
                            '3' => Mode::TicTacToe,
                            '4' => Mode::Sudoku,
                            _ => Mode::Chess,
                        };
                        if select_mode(&mut frame, &mut mode, next)? {
                            paused = false;
                            paint = true;
                            text = true;
                        }
                    }
                    Some('r' | 'R') => {
                        match mode {
                            Mode::Tetris => {
                                rng = Lcg32::new((clock::monotonic_millis() as u32) ^ 0xC11C_7E75);
                                game = Tetris::new(&mut rng, &mut events);
                                gravity_ms = 0;
                            }
                            Mode::Minesweeper => {
                                minefield.reset(clock::monotonic_millis());
                            }
                            Mode::TicTacToe => tic.reset(),
                            Mode::Sudoku => sudoku.reset(clock::monotonic_millis()),
                            Mode::Chess => chess.reset(),
                        }
                        paused = false;
                        paint = true;
                        text = true;
                    }
                    Some('p' | 'P') => {
                        paused = !paused;
                        text = true;
                    }
                    Some('f' | 'F') if mode == Mode::Minesweeper && !paused => {
                        if minefield.flag() {
                            paint = true;
                            text = true;
                        }
                    }
                    _ => {}
                }
            }
        }
        while let Some(event) = frame
            .take_pointer_event()
            .map_err(|e| Error::Ui("pointer", e))?
        {
            match mode {
                Mode::Minesweeper => {
                    if let Some((row, col)) =
                        mine_cell_at(event.local_x, event.local_y, frame.width(), frame.height())
                    {
                        if minefield.cursor != (col, row) {
                            minefield.select(row, col);
                            paint = true;
                        }
                        if !paused
                            && *minefield.status() == MineStatus::Ongoing
                            && event.buttons_pressed & POINTER_BUTTON_PRIMARY != 0
                            && minefield.reveal()
                        {
                            paint = true;
                            text = true;
                        }
                    }
                }
                Mode::TicTacToe => {
                    if let Some(player) = tic.pointer_player(event.source)
                        && let Some((row, col)) =
                            tic_cell_at(event.local_x, event.local_y, frame.width(), frame.height())
                    {
                        paint |= tic.select(player, row, col);
                        if !paused
                            && event.buttons_pressed & POINTER_BUTTON_PRIMARY != 0
                            && tic.play(player, row, col)
                        {
                            paint = true;
                            text = true;
                        }
                    }
                }
                Mode::Chess => {
                    if let Some((row, col)) =
                        chess_cell_at(event.local_x, event.local_y, frame.width(), frame.height())
                    {
                        paint |= chess.select_cursor(row, col);
                        if !paused
                            && event.buttons_pressed & POINTER_BUTTON_PRIMARY != 0
                            && chess.activate()
                        {
                            paint = true;
                            text = true;
                        }
                    }
                }
                Mode::Sudoku => {
                    if let Some((row, col)) =
                        sudoku_cell_at(event.local_x, event.local_y, frame.width(), frame.height())
                    {
                        if sudoku.select(row, col) {
                            paint = true;
                        }
                    }
                }
                Mode::Tetris => {}
            }
        }
        while let Some(event) = frame
            .take_dynamic_context_menu_event()
            .map_err(|e| Error::Ui("mine right click", e))?
        {
            if event.closed.is_none() {
                if mode == Mode::Minesweeper
                    && !paused
                    && *minefield.status() == MineStatus::Ongoing
                    && let Some((row, col)) =
                        mine_cell_at(event.local_x, event.local_y, frame.width(), frame.height())
                {
                    minefield.select(row, col);
                    paint = true;
                    if minefield.flag() {
                        text = true;
                    }
                }
                // Cancel the pending dynamic menu before UI4 draws it, then
                // rearm secondary clicks for the next cell.
                frame
                    .clear_context_menu()
                    .map_err(|e| Error::Ui("dismiss mine menu", e))?;
                if mode == Mode::Minesweeper {
                    frame
                        .register_dynamic_context_menu()
                        .map_err(|e| Error::Ui("rearm mine right click", e))?;
                }
            }
        }
        let now = clock::monotonic_millis();
        let elapsed = now.saturating_sub(last_tick).min(100);
        last_tick = now;
        if mode == Mode::Tetris && !paused && !game.is_game_over() {
            gravity_ms += elapsed;
            let interval = u64::from(game.level.level_speed_seconds());
            if gravity_ms >= interval {
                gravity_ms -= interval;
                game.soft_drop(&mut rng, &mut events);
            }
        }
        audio.update(now);
        if game.consume_changed() && mode == Mode::Tetris {
            paint = true;
        }
        text |= mode == Mode::Tetris
            && previous_hud
                != (
                    game.level.total_points,
                    game.level.current_level,
                    game.level.rows_deleted,
                    game.is_game_over(),
                );
        if paint && pending_resize.is_none() {
            match board.render(
                &mut background,
                mode,
                &game,
                &minefield,
                &tic,
                &sudoku,
                &chess,
                &chess_assets,
                frame.width(),
                frame.height(),
            ) {
                Ok(()) => paint = false,
                Err(Error::Ui(_, UiError::Busy)) => {}
                Err(error) => return Err(error),
            }
        }
        if text && !paint && pending_resize.is_none() {
            match present_text(
                &mut frame,
                mode,
                &game,
                &minefield,
                &tic,
                &sudoku,
                &chess,
                &chess_assets,
                paused,
            ) {
                Ok(()) => {
                    resize_foreground_pending = false;
                    if resize_refresh_pending {
                        resize_refresh_pending = false;
                        paint = true;
                        text = true;
                    } else {
                        text = false;
                    }
                }
                Err(Error::Ui(_, UiError::Busy)) => {}
                Err(error) => return Err(error),
            }
        }
        vsys::poll_once();
        vsys::sleep_ms(if pending_resize.is_some() || resize_foreground_pending {
            1
        } else {
            16
        });
    }
}

fn select_mode(frame: &mut Frame, mode: &mut Mode, next: Mode) -> Result<bool, Error> {
    if *mode == next {
        return Ok(false);
    }
    frame
        .clear_context_menu()
        .map_err(|e| Error::Ui("clear game menu", e))?;
    if next == Mode::Minesweeper {
        frame
            .register_dynamic_context_menu()
            .map_err(|e| Error::Ui("mine menu", e))?;
    }
    *mode = next;
    Ok(true)
}

fn chess_cell_at(local_x: i32, local_y: i32, width: u32, height: u32) -> Option<(usize, usize)> {
    let layout = Layout::fit(width, height);
    let world_x = (local_x as f32 / width as f32 - 0.5) * layout.scene_width;
    let world_y = (0.5 - local_y as f32 / height as f32) * layout.scene_height;
    let col = (world_x - (CHESS_X - CHESS_STEP * 0.5)) / CHESS_STEP;
    let row = ((CHESS_Y + CHESS_STEP * 0.5) - world_y) / CHESS_STEP;
    if col >= 0.0 && col < 8.0 && row >= 0.0 && row < 8.0 {
        Some((row as usize, col as usize))
    } else {
        None
    }
}

fn sudoku_cell_at(local_x: i32, local_y: i32, width: u32, height: u32) -> Option<(usize, usize)> {
    let layout = Layout::fit(width, height);
    let world_x = (local_x as f32 / width as f32 - 0.5) * layout.scene_width;
    let world_y = (0.5 - local_y as f32 / height as f32) * layout.scene_height;
    let col = (world_x - (SUD_X - SUD_STEP * 0.5)) / SUD_STEP;
    let row = ((SUD_Y + SUD_STEP * 0.5) - world_y) / SUD_STEP;
    if col >= 0.0 && col < 9.0 && row >= 0.0 && row < 9.0 {
        Some((row as usize, col as usize))
    } else {
        None
    }
}

fn tic_cell_at(local_x: i32, local_y: i32, width: u32, height: u32) -> Option<(usize, usize)> {
    let layout = Layout::fit(width, height);
    let world_x = (local_x as f32 / width as f32 - 0.5) * layout.scene_width;
    let world_y = (0.5 - local_y as f32 / height as f32) * layout.scene_height;
    let col = (world_x - (TIC_X - TIC_STEP * 0.5)) / TIC_STEP;
    let row = ((TIC_Y + TIC_STEP * 0.5) - world_y) / TIC_STEP;
    if col >= 0.0 && col < 3.0 && row >= 0.0 && row < 3.0 {
        Some((row as usize, col as usize))
    } else {
        None
    }
}

fn mine_cell_at(local_x: i32, local_y: i32, width: u32, height: u32) -> Option<(usize, usize)> {
    let layout = Layout::fit(width, height);
    let world_x = (local_x as f32 / width as f32 - 0.5) * layout.scene_width;
    let world_y = (0.5 - local_y as f32 / height as f32) * layout.scene_height;
    let col = (world_x - (-7.1 - CELL * 0.5)) / CELL;
    let row = (10.25 + CELL * 0.5 - world_y) / CELL;
    if col >= 0.0 && col < mines::COLS as f32 && row >= 0.0 && row < mines::ROWS as f32 {
        Some((row as usize, col as usize))
    } else {
        None
    }
}

impl HullBoard {
    fn new() -> Result<Self, Error> {
        let device = Device::open(Capabilities::DEFAULT.union(Capabilities::PRESENT))
            .map_err(|c| Error::Gpu("open device", c))?;
        let queue = device
            .create_queue(QueueClass::Render)
            .map_err(|c| Error::Gpu("render queue", c))?;
        let vertices = device
            .create_buffer(
                CUBE_VERTICES.len(),
                BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_VERTEX,
            )
            .map_err(|c| Error::Gpu("vertices", c))?;
        let indices = device
            .create_buffer(
                CUBE_INDICES.len(),
                BUFFER_USAGE_MAP_WRITE | BUFFER_USAGE_INDEX,
            )
            .map_err(|c| Error::Gpu("indices", c))?;
        write_exact(device, vertices, &CUBE_VERTICES)?;
        write_exact(device, indices, &CUBE_INDICES)?;
        let mesh = device
            .create_retained_mesh(
                vertices,
                indices,
                RetainedMeshDescriptor {
                    vertex_count: 1,
                    index_count: 44,
                    vertex_layout: RETAINED_VERTEX_LAYOUT_CUBE_PATCH_SEED,
                    topology: RETAINED_TOPOLOGY_CUBE_PATCHLIST_1 | RETAINED_MESH_FLAG_DOUBLE_SIDED,
                    ..RetainedMeshDescriptor::default()
                },
            )
            .map_err(|c| Error::Gpu("cube patch mesh", c))?;
        let seeds = device
            .create_buffer(
                MAX_SEEDS * 64,
                BUFFER_USAGE_MAP_READ | BUFFER_USAGE_MAP_WRITE,
            )
            .map_err(|c| Error::Gpu("seed buffer", c))?;
        Ok(Self {
            device,
            queue,
            vertices,
            indices,
            mesh,
            seeds,
            previous_view_projection: [0.; 16],
        })
    }

    fn render(
        &mut self,
        background: &mut BackgroundLayer,
        mode: Mode,
        game: &Tetris,
        minefield: &Minefield,
        tic: &TicTacToe,
        sudoku: &Sudoku,
        chess: &Chess,
        chess_assets: &ChessAssets,
        width: u32,
        height: u32,
    ) -> Result<(), Error> {
        let layout = Layout::fit(width, height);
        let mut bytes = Vec::with_capacity(1200 * 64);
        match mode {
            Mode::Tetris => {
                for row in 0..VISIBLE {
                    for col in 0..COLS {
                        let Some(cell) = game.cell_view_at(col, row + HIDDEN, false) else {
                            continue;
                        };
                        append_cube(
                            &mut bytes,
                            [-7.1 + col as f32 * CELL, 10.25 - row as f32 * CELL, 0.0],
                            GAME_CUBE_SCALE,
                            rgb555(cell.color),
                        );
                    }
                }
            }
            Mode::Minesweeper => {
                for row in 0..mines::ROWS {
                    for col in 0..mines::COLS {
                        let cell = minefield.cell(row, col);
                        let (color, scale) = match cell.status() {
                            MineCellStatus::Flagged => (Rgb8::new(245, 166, 63), GAME_CUBE_SCALE),
                            MineCellStatus::Exploded => (Rgb8::new(255, 67, 75), GAME_CUBE_SCALE),
                            MineCellStatus::Revealed => {
                                if cell.adjacent_mine_count() == 0 {
                                    (Rgb8::new(31, 62, 79), 0.38)
                                } else {
                                    (Rgb8::new(40, 83, 105), 0.40)
                                }
                            }
                            MineCellStatus::Hidden
                                if *minefield.status() != MineStatus::Ongoing && cell.is_mine() =>
                            {
                                (Rgb8::new(180, 61, 77), GAME_CUBE_SCALE)
                            }
                            MineCellStatus::Hidden => (Rgb8::new(60, 112, 150), GAME_CUBE_SCALE),
                        };
                        append_cube(
                            &mut bytes,
                            [-7.1 + col as f32 * CELL, 10.25 - row as f32 * CELL, 0.0],
                            scale,
                            rgb555(color),
                        );
                    }
                }
                let (col, row) = minefield.cursor;
                let center_x = -7.1 + col as f32 * CELL;
                let center_y = 10.25 - row as f32 * CELL;
                for dx in [-0.43, 0.43] {
                    for dy in [-0.43, 0.43] {
                        append_cube(
                            &mut bytes,
                            [center_x + dx, center_y + dy, 0.10],
                            BORDER_CUBE_SCALE,
                            rgb555(Rgb8::new(238, 244, 190)),
                        );
                    }
                }
            }
            Mode::Chess => {
                for row in 0..8 {
                    for col in 0..8 {
                        let square = Chess::square(row, col);
                        let x = CHESS_X + col as f32 * CHESS_STEP;
                        let y = CHESS_Y - row as f32 * CHESS_STEP;
                        let tile = if (row + col) % 2 == 0 {
                            Rgb8::new(98, 129, 144)
                        } else {
                            Rgb8::new(32, 62, 82)
                        };
                        append_cube(&mut bytes, [x, y, -0.17], 0.57, rgb555(tile));
                        if chess.legal_at(row, col) {
                            for dx in [-0.52, 0.52] {
                                for dy in [-0.52, 0.52] {
                                    append_cube(
                                        &mut bytes,
                                        [x + dx, y + dy, 0.26],
                                        BORDER_CUBE_SCALE,
                                        rgb555(Rgb8::new(110, 236, 153)),
                                    );
                                }
                            }
                        }
                        if let Some(piece) = chess.board().piece_on(square) {
                            let side = chess.board().color_on(square).unwrap();
                            if let Some(asset) = chess_assets.piece(piece as usize) {
                                for cube in asset {
                                    let color = chess_tint(cube.color, side);
                                    append_cube(
                                        &mut bytes,
                                        [
                                            x + cube.center[0] * CHESS_STEP * 0.70,
                                            y + cube.center[1] * CHESS_STEP * 0.70,
                                            0.67 + cube.center[2] * CHESS_STEP * 0.70,
                                        ],
                                        cube.scale * CHESS_STEP * 0.70,
                                        rgb555(color),
                                    );
                                }
                            } else {
                                let color = chess_tint(Rgb8::new(190, 200, 205), side);
                                for (r, bits) in
                                    CHESS_GLYPHS[piece as usize].into_iter().enumerate()
                                {
                                    for c in 0..3 {
                                        if bits & (1 << (2 - c)) != 0 {
                                            append_cube(
                                                &mut bytes,
                                                [
                                                    x + (c as f32 - 1.0) * 0.25,
                                                    y + (2.0 - r as f32) * 0.22,
                                                    0.50,
                                                ],
                                                0.105,
                                                rgb555(color),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if let Some(selected) = chess.selected {
                    let col = selected.file() as usize;
                    let row = 7 - selected.rank() as usize;
                    append_chess_corners(&mut bytes, row, col, Rgb8::new(255, 201, 94));
                }
                append_chess_corners(
                    &mut bytes,
                    chess.cursor.1,
                    chess.cursor.0,
                    Rgb8::new(240, 247, 188),
                );
            }
            Mode::Sudoku => {
                let violations = sudoku.violations();
                for row in 0..9 {
                    for col in 0..9 {
                        let index = row * 9 + col;
                        let x = SUD_X + col as f32 * SUD_STEP;
                        let y = SUD_Y - row as f32 * SUD_STEP;
                        let box_light = ((row / 3) + (col / 3)) % 2 == 0;
                        let tile = if box_light {
                            Rgb8::new(38, 68, 84)
                        } else {
                            Rgb8::new(27, 51, 69)
                        };
                        append_cube(&mut bytes, [x, y, -0.10], 0.49, rgb555(tile));
                        let digit = sudoku.game().digit(index);
                        if digit == 0 {
                            continue;
                        }
                        let color = if violations.get(index) {
                            Rgb8::new(255, 83, 96)
                        } else if sudoku.game().frozen().get(index) {
                            Rgb8::new(167, 225, 244)
                        } else {
                            Rgb8::new(255, 191, 116)
                        };
                        let glyph = SUD_DIGITS[(digit - 1) as usize];
                        for (r, bits) in glyph.into_iter().enumerate() {
                            for c in 0..3 {
                                if bits & (1 << (2 - c)) != 0 {
                                    append_cube(
                                        &mut bytes,
                                        [
                                            x + (c as f32 - 1.0) * 0.22,
                                            y + (2.0 - r as f32) * 0.20,
                                            0.47,
                                        ],
                                        0.085,
                                        rgb555(color),
                                    );
                                }
                            }
                        }
                    }
                }
                for n in [3.0_f32, 6.0] {
                    let x = SUD_X + (n - 0.5) * SUD_STEP;
                    let y = SUD_Y - (n - 0.5) * SUD_STEP;
                    for i in 0..=36 {
                        let delta = -5.4 + i as f32 * 0.3;
                        append_cube(
                            &mut bytes,
                            [x, delta, 0.18],
                            BORDER_CUBE_SCALE,
                            rgb555(Rgb8::new(101, 161, 187)),
                        );
                        append_cube(
                            &mut bytes,
                            [SUD_X + 4.0 * SUD_STEP + delta, y, 0.18],
                            BORDER_CUBE_SCALE,
                            rgb555(Rgb8::new(101, 161, 187)),
                        );
                    }
                }
                let x = SUD_X + sudoku.cursor.0 as f32 * SUD_STEP;
                let y = SUD_Y - sudoku.cursor.1 as f32 * SUD_STEP;
                for dx in [-0.48, 0.48] {
                    for dy in [-0.48, 0.48] {
                        append_cube(
                            &mut bytes,
                            [x + dx, y + dy, 0.48],
                            BORDER_CUBE_SCALE,
                            rgb555(Rgb8::new(255, 239, 149)),
                        );
                    }
                }
            }
            Mode::TicTacToe => {
                for row in 0..3 {
                    for col in 0..3 {
                        let x = TIC_X + col as f32 * TIC_STEP;
                        let y = TIC_Y - row as f32 * TIC_STEP;
                        append_cube(
                            &mut bytes,
                            [x, y, -0.16],
                            1.48,
                            rgb555(Rgb8::new(32, 59, 77)),
                        );
                        let mark = tic.game().get(row, col);
                        let color = match mark {
                            Some(TicPlayer::Player0) => Rgb8::new(89, 221, 242),
                            Some(TicPlayer::Player1) => Rgb8::new(255, 166, 87),
                            None => continue,
                        };
                        for (dx, dy) in tic_mark_points(mark.unwrap()) {
                            append_cube(
                                &mut bytes,
                                [x + dx, y + dy, 0.35],
                                GAME_CUBE_SCALE,
                                rgb555(color),
                            );
                        }
                    }
                }
                for seat in tic.seats().iter().flatten().filter(|seat| seat.selected) {
                    let x = TIC_X + seat.cell.0 as f32 * TIC_STEP;
                    let y = TIC_Y - seat.cell.1 as f32 * TIC_STEP;
                    let [r, g, b, _] = seat.color_rgba.to_le_bytes();
                    let color = rgb555(Rgb8::new(r, g, b));
                    for dx in [-1.37, 1.37] {
                        for dy in [-1.37, 1.37] {
                            append_cube(
                                &mut bytes,
                                [x + dx, y + dy, 0.44],
                                BORDER_CUBE_SCALE,
                                color,
                            );
                        }
                    }
                }
            }
        }
        // The outer border follows the whole UI4 window on maximize/restore.
        // The inner border encloses the fixed 10 x 20 playfield.
        let outer_x = layout.scene_width * 0.5 - 0.35;
        let outer_y = layout.scene_height * 0.5 - 0.35;
        append_border(
            &mut bytes,
            -outer_x,
            outer_x,
            -outer_y,
            outer_y,
            rgb555(Rgb8::new(82, 143, 177)),
        );
        append_border(
            &mut bytes,
            -7.95,
            3.47,
            -11.1,
            11.1,
            rgb555(Rgb8::new(105, 205, 235)),
        );
        if bytes.len() / 64 > MAX_SEEDS {
            return Err(Error::Gpu("cube border seed budget", trueos::vgpu::ERR_IO));
        }
        write_exact(self.device, self.seeds, &bytes)?;
        background
            .begin_gpu_frame()
            .map_err(|e| Error::Ui("begin board", e))?;
        let surface = self
            .device
            .acquire_ui4_surface(background.render_target())
            .map_err(|c| Error::Gpu("board surface", c))?;
        let mut camera = Camera {
            position: [0., 0., 35.],
            rotation: Quaternion::IDENTITY,
            projection: Projection::Orthographic {
                xmag: layout.scene_width,
                ymag: layout.scene_height,
                znear: 0.1,
                zfar: 100.,
            },
        }
        .retained(width, height, self.previous_view_projection);
        if self.previous_view_projection == [0.; 16] {
            camera.previous_view_projection = camera.view_projection;
        }
        let point = self
            .device
            .submit_retained_frame_v3(
                self.queue,
                surface,
                self.mesh,
                self.vertices,
                self.indices,
                RetainedFrameSubmitV3 {
                    frame: RetainedFrameSubmitV2 {
                        frame: RetainedFrameSubmit {
                            camera,
                            clear_rgba8_srgb: BG,
                            ..RetainedFrameSubmit::default()
                        },
                        ..RetainedFrameSubmitV2::default()
                    },
                    seed_buffer: self.seeds.raw(),
                    seed_count: (bytes.len() / 64) as u32,
                    draw_count: 1,
                    draws: [
                        RetainedDrawRange {
                            first_index: 0,
                            index_count: 44,
                        },
                        RetainedDrawRange::default(),
                        RetainedDrawRange::default(),
                        RetainedDrawRange::default(),
                    ],
                    ..RetainedFrameSubmitV3::default()
                },
            )
            .map_err(|c| Error::Gpu("hull shader submit", c))?;
        self.device
            .wait(self.queue, point.value)
            .map_err(|c| Error::Gpu("board completion", c))?;
        background
            .publish(Damage::full(width, height))
            .map_err(|e| Error::Ui("publish board", e))?;
        self.previous_view_projection = camera.view_projection;
        Ok(())
    }
}

const CHESS_GLYPHS: [[u8; 5]; 6] = [
    [0b110, 0b101, 0b110, 0b100, 0b100], // P
    [0b101, 0b111, 0b111, 0b111, 0b101], // N
    [0b110, 0b101, 0b110, 0b101, 0b110], // B
    [0b110, 0b101, 0b110, 0b101, 0b101], // R
    [0b111, 0b101, 0b101, 0b111, 0b001], // Q
    [0b101, 0b110, 0b100, 0b110, 0b101], // K
];

fn chess_tint(color: Rgb8, side: ChessColor) -> Rgb8 {
    let mix = |v: u8, base: u16, weight: u16| (base + u16::from(v) * weight / 255).min(255) as u8;
    match side {
        ChessColor::White => Rgb8::new(
            mix(color.r, 142, 105),
            mix(color.g, 138, 103),
            mix(color.b, 125, 98),
        ),
        ChessColor::Black => Rgb8::new(
            mix(color.r, 24, 77),
            mix(color.g, 42, 93),
            mix(color.b, 61, 106),
        ),
    }
}

fn append_chess_corners(bytes: &mut Vec<u8>, row: usize, col: usize, color: Rgb8) {
    let x = CHESS_X + col as f32 * CHESS_STEP;
    let y = CHESS_Y - row as f32 * CHESS_STEP;
    for dx in [-0.56, 0.56] {
        for dy in [-0.56, 0.56] {
            append_cube(
                bytes,
                [x + dx, y + dy, 0.50],
                BORDER_CUBE_SCALE,
                rgb555(color),
            );
        }
    }
}

const SUD_DIGITS: [[u8; 5]; 9] = [
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

fn tic_mark_points(player: TicPlayer) -> &'static [(f32, f32)] {
    const X: &[(f32, f32)] = &[
        (-0.82, -0.82),
        (-0.82, 0.82),
        (0.0, 0.0),
        (0.82, -0.82),
        (0.82, 0.82),
    ];
    const O: &[(f32, f32)] = &[
        (-0.82, -0.82),
        (-0.82, 0.0),
        (-0.82, 0.82),
        (0.0, -0.82),
        (0.0, 0.82),
        (0.82, -0.82),
        (0.82, 0.0),
        (0.82, 0.82),
    ];
    match player {
        TicPlayer::Player0 => X,
        TicPlayer::Player1 => O,
    }
}

fn append_border(bytes: &mut Vec<u8>, left: f32, right: f32, bottom: f32, top: f32, color: u32) {
    let across = (((right - left) / BORDER_STEP) as usize + 1).clamp(1, MAX_BORDER_SEGMENTS);
    let down = (((top - bottom) / BORDER_STEP) as usize + 1).clamp(1, MAX_BORDER_SEGMENTS);
    for i in (0..=across).step_by(2) {
        let x = left + (right - left) * i as f32 / across as f32;
        append_cube(bytes, [x, top, 0.03], BORDER_CUBE_SCALE, color);
        append_cube(bytes, [x, bottom, 0.03], BORDER_CUBE_SCALE, color);
    }
    for i in (1..down).step_by(2) {
        let y = bottom + (top - bottom) * i as f32 / down as f32;
        append_cube(bytes, [left, y, 0.03], BORDER_CUBE_SCALE, color);
        append_cube(bytes, [right, y, 0.03], BORDER_CUBE_SCALE, color);
    }
}

fn append_cube(bytes: &mut Vec<u8>, center: [f32; 3], scale: f32, color: u32) {
    encode_seed(
        RetainedTransformSeed {
            translation: center,
            previous_translation: center,
            scale: [scale; 3],
            rotation: [0., 0., 0., 1.],
            local_radius: 1.74,
            draw_group: 0,
            flags: ((bytes.len() / 64) as u32) << 16 | color,
        },
        bytes,
    );
}

fn rgb555(color: Rgb8) -> u32 {
    let quantize = |channel: u8| (u32::from(channel) * 31 + 127) / 255;
    CUSTOM_RGB555 | quantize(color.r) | (quantize(color.g) << 5) | (quantize(color.b) << 10)
}

fn upload_mine_glyphs(frame: &mut Frame) -> Result<(), Error> {
    let mut atlas = alloc::vec![0_u8; MINE_ATLAS_WIDTH * MINE_TILE_HEIGHT * 4];
    for (index, &glyph) in MINE_GLYPHS.iter().enumerate() {
        let mut mask = [0_u8; microfont::FWIDTH * microfont::FHEIGHT];
        microfont::stamp_bytes(
            &mut mask,
            microfont::FWIDTH,
            microfont::FHEIGHT,
            0,
            0,
            core::slice::from_ref(&glyph),
            1_u8,
        )
        .map_err(|_| Error::Ui("stamp mine glyph", UiError::Invalid))?;
        for y in 0..microfont::FHEIGHT {
            for x in 0..microfont::FWIDTH {
                if mask[y * microfont::FWIDTH + x] == 0 {
                    continue;
                }
                let px = index * MINE_TILE_WIDTH + 2 + x;
                let py = 2 + y;
                let offset = (py * MINE_ATLAS_WIDTH + px) * 4;
                atlas[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
    }
    frame
        .upload_sprite_rgba8(
            MINE_GLYPH_SPRITE,
            MINE_ATLAS_WIDTH as u32,
            MINE_TILE_HEIGHT as u32,
            &atlas,
        )
        .map_err(|e| Error::Ui("upload mine glyphs", e))
}

fn mine_glyph_quad(
    layout: &Layout,
    glyph: u8,
    col: usize,
    row: usize,
    color: u32,
    glyph_scale: f32,
) -> SpriteQuad {
    let index = MINE_GLYPHS
        .iter()
        .position(|&candidate| candidate == glyph)
        .unwrap();
    let u0 = index as f32 / MINE_GLYPHS.len() as f32;
    let u1 = (index + 1) as f32 / MINE_GLYPHS.len() as f32;
    let center_x = layout.offset_x + (114.8 + col as f32 * 31.97) * layout.scale;
    let center_y = layout.offset_y + (66.6 + row as f32 * 31.97) * layout.scale;
    let half_width = MINE_TILE_WIDTH as f32 * glyph_scale * 0.5;
    let half_height = MINE_TILE_HEIGHT as f32 * glyph_scale * 0.5;
    let x0 = center_x - half_width;
    let y0 = center_y - half_height;
    let x1 = center_x + half_width;
    let y1 = center_y + half_height;
    SpriteQuad {
        sprite_id: MINE_GLYPH_SPRITE,
        c0: SpriteCorner {
            x: x0,
            y: y0,
            u: u0,
            v: 0.,
        },
        c1: SpriteCorner {
            x: x1,
            y: y0,
            u: u1,
            v: 0.,
        },
        c2: SpriteCorner {
            x: x1,
            y: y1,
            u: u1,
            v: 1.,
        },
        c3: SpriteCorner {
            x: x0,
            y: y1,
            u: u0,
            v: 1.,
        },
        color_rgba: color,
        source_over: true,
    }
}

fn present_text(
    frame: &mut Frame,
    mode: Mode,
    game: &Tetris,
    minefield: &Minefield,
    tic: &TicTacToe,
    sudoku: &Sudoku,
    chess: &Chess,
    chess_assets: &ChessAssets,
    paused: bool,
) -> Result<(), Error> {
    let score = format!("SCORE  {}", game.level.total_points);
    let level_text = format!("LEVEL  {}", game.level.current_level);
    let rows_text = format!("LINES  {}", game.level.rows_deleted);
    let mines_text = format!("MINES  {}", mines::MINES);
    let flags_text = format!("FLAGS  {}", minefield.flag_count());
    let clear_text = format!("CLEAR  {}", minefield.revealed_count());
    let sudoku_filled = format!("FILLED  {}/81", sudoku.game().board().filled());
    let sudoku_errors = format!("ERRORS  {}", sudoku.violations().count());
    let chess_assets_text = format!("ASSETS  {}/6", chess_assets.loaded());
    let white = rgba(230, 240, 250, 255);
    let muted = rgba(150, 172, 194, 255);
    let selected = rgba(160, 233, 246, 255);
    let layout = Layout::fit(frame.width(), frame.height());
    let mut rows = Vec::with_capacity(20);
    let mut glyph_quads = Vec::with_capacity(200);
    let glyph_scale =
        if output_dimensions().is_ok_and(|extent| extent == (frame.width(), frame.height())) {
            2.
        } else {
            1.
        };
    for (label, x, y, active) in [
        ("1 TETRIS", 462., 28., mode == Mode::Tetris),
        ("2 MINES", 462., 57., mode == Mode::Minesweeper),
        ("3 TIC", 556., 28., mode == Mode::TicTacToe),
        ("4 SUDOKU", 556., 57., mode == Mode::Sudoku),
        ("5 CHESS", 462., 86., mode == Mode::Chess),
    ] {
        rows.push(layout.row(label, x, y, 17., if active { selected } else { muted }));
    }
    match mode {
        Mode::Tetris => {
            let status = if game.is_game_over() {
                "GAME OVER"
            } else if paused {
                "PAUSED"
            } else {
                "PLAYING"
            };
            rows.extend([
                layout.row(&score, 462., 114., 21., white),
                layout.row(&level_text, 462., 155., 21., white),
                layout.row(&rows_text, 462., 196., 21., white),
                layout.row(status, 462., 255., 22., white),
                layout.row("LEFT / RIGHT", 462., 340., 19., muted),
                layout.row("MOVE", 462., 365., 18., muted),
                layout.row("UP  ROTATE", 462., 414., 19., muted),
                layout.row("DOWN  DROP", 462., 452., 19., muted),
                layout.row("SPACE  FALL", 462., 490., 19., muted),
                layout.row("P  PAUSE", 462., 565., 19., muted),
                layout.row("R  RESTART", 462., 603., 19., muted),
            ]);
        }
        Mode::Minesweeper => {
            let status = if paused {
                "PAUSED"
            } else {
                match minefield.status() {
                    MineStatus::Ongoing => "SWEEPING",
                    MineStatus::Exploded => "BOOM",
                    MineStatus::Finished => "CLEARED",
                }
            };
            rows.extend([
                layout.row(&mines_text, 462., 114., 21., white),
                layout.row(&flags_text, 462., 155., 21., white),
                layout.row(&clear_text, 462., 196., 21., white),
                layout.row(status, 462., 255., 22., white),
                layout.row("CLICK  REVEAL", 462., 340., 18., muted),
                layout.row("RIGHT  FLAG", 462., 379., 18., muted),
                layout.row("ARROWS MOVE", 462., 432., 18., muted),
                layout.row("SPACE OPEN", 462., 470., 18., muted),
                layout.row("F  FLAG", 462., 508., 18., muted),
                layout.row("P  PAUSE", 462., 565., 19., muted),
                layout.row("R  RESTART", 462., 603., 19., muted),
            ]);
            const DIGITS: [&str; 9] = ["", "1", "2", "3", "4", "5", "6", "7", "8"];
            for row in 0..mines::ROWS {
                for col in 0..mines::COLS {
                    let cell = minefield.cell(row, col);
                    let label = match cell.status() {
                        MineCellStatus::Revealed => DIGITS[cell.adjacent_mine_count()],
                        MineCellStatus::Flagged => "F",
                        MineCellStatus::Exploded => "X",
                        MineCellStatus::Hidden
                            if *minefield.status() != MineStatus::Ongoing && cell.is_mine() =>
                        {
                            "X"
                        }
                        MineCellStatus::Hidden => "",
                    };
                    if label.is_empty() {
                        continue;
                    }
                    let color = match cell.status() {
                        MineCellStatus::Flagged => rgba(32, 31, 43, 255),
                        MineCellStatus::Exploded => white,
                        _ => mine_number_color(cell.adjacent_mine_count()),
                    };
                    glyph_quads.push(mine_glyph_quad(
                        &layout,
                        label.as_bytes()[0],
                        col,
                        row,
                        color,
                        glyph_scale,
                    ));
                }
            }
        }
        Mode::Chess => {
            let turn = match chess.side() {
                ChessColor::White => "WHITE TURN",
                ChessColor::Black => "BLACK TURN",
            };
            let status = if paused {
                "PAUSED"
            } else {
                match chess.status() {
                    ChessStatus::Ongoing => turn,
                    ChessStatus::Drawn => "DRAW",
                    ChessStatus::Won if chess.side() == ChessColor::White => "BLACK WINS",
                    ChessStatus::Won => "WHITE WINS",
                }
            };
            rows.extend([
                layout.row(status, 462., 144., 21., white),
                layout.row(&chess_assets_text, 462., 195., 19., muted),
                layout.row("CLICK PIECE", 462., 320., 18., muted),
                layout.row("CLICK TARGET", 462., 358., 18., muted),
                layout.row("GREEN LEGAL", 462., 398., 18., muted),
                layout.row("ARROWS MOVE", 462., 455., 18., muted),
                layout.row("SPACE SELECT", 462., 493., 18., muted),
                layout.row("F1-F5 MODES", 462., 531., 18., muted),
                layout.row("P PAUSE", 462., 586., 19., muted),
                layout.row("R RESTART", 462., 624., 19., muted),
            ]);
        }
        Mode::Sudoku => {
            let status = if paused {
                "PAUSED"
            } else if sudoku.game().is_complete() {
                "SOLVED"
            } else {
                "EASY"
            };
            rows.extend([
                layout.row(&sudoku_filled, 462., 114., 20., white),
                layout.row(&sudoku_errors, 462., 155., 20., white),
                layout.row(status, 462., 233., 22., white),
                layout.row("HOVER SELECT", 462., 326., 18., muted),
                layout.row("ARROWS MOVE", 462., 365., 18., muted),
                layout.row("1-9  ENTER", 462., 414., 18., muted),
                layout.row("0 / DEL ERASE", 462., 453., 18., muted),
                layout.row("F1-F4  MODES", 462., 508., 18., muted),
                layout.row("P  PAUSE", 462., 565., 19., muted),
                layout.row("R  NEW PUZZLE", 462., 603., 19., muted),
            ]);
        }
        Mode::TicTacToe => {
            let status = if paused {
                "PAUSED"
            } else {
                match tic.game().status() {
                    TicStatus::Ongoing => match tic.game().next_player() {
                        TicPlayer::Player0 => "P1 TURN",
                        TicPlayer::Player1 => "P2 TURN",
                    },
                    TicStatus::Draw => "DRAW",
                    TicStatus::Win(TicPlayer::Player0) => "P1 WINS",
                    TicStatus::Win(TicPlayer::Player1) => "P2 WINS",
                }
            };
            let p1 = if tic.seats()[0].is_some_and(|seat| seat.selected) {
                "P1 X  READY"
            } else {
                "P1 X  WAIT"
            };
            let p2 = if tic.seats()[1].is_some_and(|seat| seat.selected) {
                "P2 O  READY"
            } else {
                "P2 O  WAIT"
            };
            rows.extend([
                layout.row(p1, 462., 114., 20., white),
                layout.row(p2, 462., 155., 20., white),
                layout.row(status, 462., 233., 22., white),
                layout.row("2 CURSORS", 462., 327., 19., muted),
                layout.row("EACH SELECTS", 462., 355., 18., muted),
                layout.row("CLICK TO PLAY", 462., 414., 18., muted),
                layout.row("ARROWS MOVE", 462., 469., 18., muted),
                layout.row("SPACE / ENTER", 462., 497., 18., muted),
                layout.row("P  PAUSE", 462., 565., 19., muted),
                layout.row("R  RESTART", 462., 603., 19., muted),
            ]);
        }
    }
    let canvas = (frame.width(), frame.height());
    frame
        .retain_font_canvas(Font::Inconsolata, canvas, &rows)
        .map_err(|e| Error::Ui("font canvas", e))?;
    frame
        .begin_sprite_frame(rgba(0, 0, 0, 0))
        .map_err(|e| Error::Ui("begin text", e))?;
    let mut quads = Vec::with_capacity(glyph_quads.len() + 1);
    quads.push(
        frame
            .font_canvas_quad(canvas, (0, 0))
            .map_err(|e| Error::Ui("font canvas quad", e))?,
    );
    quads.extend(glyph_quads);
    frame
        .draw_sprite_quads(&quads)
        .map_err(|e| Error::Ui("draw text and mine glyphs", e))?;
    frame
        .publish(Damage::full(canvas.0, canvas.1))
        .map_err(|e| Error::Ui("publish text", e))
}

fn mine_number_color(number: usize) -> u32 {
    match number {
        1 => rgba(154, 205, 255, 255),
        2 => rgba(138, 234, 172, 255),
        3 => rgba(255, 175, 154, 255),
        4 => rgba(194, 178, 255, 255),
        5 => rgba(255, 219, 142, 255),
        _ => rgba(242, 226, 244, 255),
    }
}

fn write_exact(device: Device, buffer: trueos::vgpu::Buffer, bytes: &[u8]) -> Result<(), Error> {
    if device
        .write_buffer(buffer, 0, bytes)
        .map_err(|c| Error::Gpu("upload", c))?
        != bytes.len()
    {
        return Err(Error::Gpu("short upload", trueos::vgpu::ERR_IO));
    }
    Ok(())
}

fn encode_seed(seed: RetainedTransformSeed, bytes: &mut Vec<u8>) {
    for value in seed
        .translation
        .into_iter()
        .chain(seed.scale)
        .chain(seed.rotation)
        .chain([seed.local_radius])
        .chain(seed.previous_translation)
    {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&seed.draw_group.to_le_bytes());
    bytes.extend_from_slice(&seed.flags.to_le_bytes());
}
