#![no_std]

extern crate alloc;

mod audio;
mod mines;

use alloc::{format, vec::Vec};
use audio::Audio;
use gamie::minesweeper::{CellStatus as MineCellStatus, Status as MineStatus};
use microgames::{Game, Lcg32, NoopEvents, Rgb8, Rotation};
use mines::Minefield;
use trueos::input;
use trueos::logl::{self, level};
use trueos::ui4_scene::{
    BackgroundLayer, Damage, Error as UiError, Font, FontCanvasRow, Frame, MenuEntry,
    POINTER_BUTTON_PRIMARY, output_dimensions, rgba,
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
const GAME_CUBE_SCALE: f32 = 0.46;
const BORDER_CUBE_SCALE: f32 = GAME_CUBE_SCALE / 4.0;
const BORDER_STEP: f32 = CELL / 4.0;
const MAX_BORDER_SEGMENTS: usize = 600;
const MAX_SEEDS: usize = 4096;
const CUBE_VERTICES: [u8; 12] = [0; 12];
const CUBE_INDICES: [u8; 44 * 4] = [0; 44 * 4];
const CUSTOM_RGB555: u32 = 1 << 15;
const BG: u32 = u32::from_le_bytes([11, 17, 30, 128]);
const SCENE_WIDTH: f32 = SCENE_HEIGHT * WIDTH as f32 / HEIGHT as f32;
const SCENE_HEIGHT: f32 = 25.0;

type Tetris = Game<COLS, ROWS, HIDDEN>;

#[derive(Clone, Copy, Eq, PartialEq)]
enum Mode {
    Tetris,
    Minesweeper,
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
    let mut background = frame
        .background()
        .map_err(|e| Error::Ui("background layer", e))?;
    let mut board = HullBoard::new()?;
    let mut rng = Lcg32::new((clock::monotonic_millis() as u32) ^ 0xC11C_7E75);
    let mut events = NoopEvents;
    let mut game = Tetris::new(&mut rng, &mut events);
    let mut minefield = Minefield::new(clock::monotonic_millis());
    let mut mode = Mode::Tetris;
    let mut paused = false;
    let mut last_tick = clock::monotonic_millis();
    let mut gravity_ms = 0_u64;
    let mut paint = true;
    let mut text = true;
    let mut pending_resize = None;
    let mut audio = Audio::new();
    let mut menu_target: Option<(u64, usize, usize)> = None;

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
        while let Some(event) = frame
            .take_keyboard_event()
            .map_err(|e| Error::Ui("keyboard", e))?
        {
            if event.kind == input::KEYBOARD_OUTPUT_KIND_KEY
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
                    _ => {}
                }
            } else if event.kind == input::KEYBOARD_OUTPUT_KIND_TEXT {
                match char::from_u32(event.codepoint) {
                    Some('1') if mode != Mode::Tetris => {
                        frame
                            .clear_context_menu()
                            .map_err(|e| Error::Ui("clear mine menu", e))?;
                        menu_target = None;
                        mode = Mode::Tetris;
                        paused = false;
                        paint = true;
                        text = true;
                    }
                    Some('2') if mode != Mode::Minesweeper => {
                        frame
                            .register_dynamic_context_menu()
                            .map_err(|e| Error::Ui("mine menu", e))?;
                        mode = Mode::Minesweeper;
                        paused = false;
                        paint = true;
                        text = true;
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
            if mode != Mode::Minesweeper || paused || *minefield.status() != MineStatus::Ongoing {
                continue;
            }
            let buttons = event.buttons_pressed & POINTER_BUTTON_PRIMARY;
            if buttons == 0 {
                continue;
            }
            if let Some((row, col)) =
                mine_cell_at(event.local_x, event.local_y, frame.width(), frame.height())
            {
                minefield.select(row, col);
                paint = true;
                if minefield.reveal() {
                    text = true;
                }
            }
        }
        while let Some(event) = frame
            .take_dynamic_context_menu_event()
            .map_err(|e| Error::Ui("mine menu event", e))?
        {
            let entries = [
                MenuEntry::new("Flag / unflag", menu_flag),
                MenuEntry::new("Reveal", menu_reveal),
            ];
            if event.closed.is_none() {
                if mode == Mode::Minesweeper
                    && !paused
                    && *minefield.status() == MineStatus::Ongoing
                {
                    if let Some((row, col)) =
                        mine_cell_at(event.local_x, event.local_y, frame.width(), frame.height())
                    {
                        if frame
                            .resolve_context_menu(event.serial, &entries)
                            .map_err(|e| Error::Ui("resolve mine menu", e))?
                        {
                            menu_target = Some((event.serial, row, col));
                        }
                    } else {
                        let empty = [MenuEntry::<Minefield>::disabled("Minefield cells only")];
                        let _ = frame
                            .resolve_context_menu(event.serial, &empty)
                            .map_err(|e| Error::Ui("resolve empty menu", e))?;
                    }
                } else {
                    let unavailable = [MenuEntry::<Minefield>::disabled("Game paused or finished")];
                    let _ = frame
                        .resolve_context_menu(event.serial, &unavailable)
                        .map_err(|e| Error::Ui("resolve unavailable menu", e))?;
                }
            } else if let Some((serial, row, col)) = menu_target {
                if serial == event.serial {
                    menu_target = None;
                    if mode == Mode::Minesweeper
                        && !paused
                        && *minefield.status() == MineStatus::Ongoing
                    {
                        minefield.select(row, col);
                        event.dispatch(&entries, &mut minefield);
                        paint = true;
                        text = true;
                    }
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
        let playing = !paused
            && match mode {
                Mode::Tetris => !game.is_game_over(),
                Mode::Minesweeper => *minefield.status() == MineStatus::Ongoing,
            };
        audio.update(playing, now);
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
        if paint {
            match board.render(
                &mut background,
                mode,
                &game,
                &minefield,
                frame.width(),
                frame.height(),
            ) {
                Ok(()) => paint = false,
                Err(Error::Ui(_, UiError::Busy)) => {}
                Err(error) => return Err(error),
            }
        }
        if text {
            match present_text(&mut frame, mode, &game, &minefield, paused) {
                Ok(()) => text = false,
                Err(Error::Ui(_, UiError::Busy)) => {}
                Err(error) => return Err(error),
            }
        }
        vsys::poll_once();
        vsys::sleep_ms(16);
    }
}

fn menu_flag(minefield: &mut Minefield) {
    let _ = minefield.flag();
}

fn menu_reveal(minefield: &mut Minefield) {
    let _ = minefield.reveal();
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
        let camera = Camera {
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

fn append_border(bytes: &mut Vec<u8>, left: f32, right: f32, bottom: f32, top: f32, color: u32) {
    let across = (((right - left) / BORDER_STEP) as usize + 1).clamp(1, MAX_BORDER_SEGMENTS);
    let down = (((top - bottom) / BORDER_STEP) as usize + 1).clamp(1, MAX_BORDER_SEGMENTS);
    for i in 0..=across {
        let x = left + (right - left) * i as f32 / across as f32;
        append_cube(bytes, [x, top, 0.03], BORDER_CUBE_SCALE, color);
        append_cube(bytes, [x, bottom, 0.03], BORDER_CUBE_SCALE, color);
    }
    for i in 1..down {
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

fn present_text(
    frame: &mut Frame,
    mode: Mode,
    game: &Tetris,
    minefield: &Minefield,
    paused: bool,
) -> Result<(), Error> {
    let score = format!("SCORE  {}", game.level.total_points);
    let level_text = format!("LEVEL  {}", game.level.current_level);
    let rows_text = format!("LINES  {}", game.level.rows_deleted);
    let mines_text = format!("MINES  {}", mines::MINES);
    let flags_text = format!("FLAGS  {}", minefield.flag_count());
    let clear_text = format!("CLEAR  {}", minefield.revealed_count());
    let white = rgba(230, 240, 250, 255);
    let muted = rgba(150, 172, 194, 255);
    let selected = rgba(160, 233, 246, 255);
    let layout = Layout::fit(frame.width(), frame.height());
    let mut rows = Vec::with_capacity(220);
    rows.push(layout.row(
        "1  TETRIS",
        462.,
        28.,
        19.,
        if mode == Mode::Tetris {
            selected
        } else {
            muted
        },
    ));
    rows.push(layout.row(
        "2  MINES",
        462.,
        57.,
        19.,
        if mode == Mode::Minesweeper {
            selected
        } else {
            muted
        },
    ));
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
                layout.row("RIGHT  MENU", 462., 379., 18., muted),
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
                    rows.push(layout.row(
                        label,
                        109. + col as f32 * 31.97,
                        53. + row as f32 * 31.97,
                        23.,
                        color,
                    ));
                }
            }
        }
    }
    let canvas = (frame.width(), frame.height());
    frame
        .retain_font_canvas(Font::Inconsolata, canvas, &rows)
        .map_err(|e| Error::Ui("font canvas", e))?;
    frame
        .begin_sprite_frame(rgba(0, 0, 0, 0))
        .map_err(|e| Error::Ui("begin text", e))?;
    frame
        .draw_font_canvas_view(canvas, (0, 0))
        .map_err(|e| Error::Ui("draw text", e))?;
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
