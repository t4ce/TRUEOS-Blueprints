#![no_std]

extern crate alloc;

use alloc::{format, vec::Vec};
use microgames::{Game, Lcg32, NoopEvents, Rgb8, Rotation};
use trueos::input;
use trueos::logl::{self, level};
use trueos::ui4_scene::{
    BackgroundLayer, Damage, Error as UiError, Font, FontCanvasRow, Frame, output_dimensions, rgba,
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
const MAX_SEEDS: usize = COLS * VISIBLE;
const CUBE_VERTICES: [u8; 12] = [0; 12];
const CUBE_INDICES: [u8; 44 * 4] = [0; 44 * 4];
const CUSTOM_RGB555: u32 = 1 << 15;
const BG: u32 = u32::from_le_bytes([11, 17, 30, 255]);

type Tetris = Game<COLS, ROWS, HIDDEN>;

enum Error {
    Ui(&'static str, UiError),
    Gpu(&'static str, i32),
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
    let mut paused = false;
    let mut last_tick = clock::monotonic_millis();
    let mut gravity_ms = 0_u64;
    let mut paint = true;
    let mut text = true;

    loop {
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
            {
                match event.key_code {
                    input::KEYBOARD_KEY_ARROW_LEFT if !paused && !game.is_game_over() => {
                        game.move_left();
                    }
                    input::KEYBOARD_KEY_ARROW_RIGHT if !paused && !game.is_game_over() => {
                        game.move_right();
                    }
                    input::KEYBOARD_KEY_ARROW_UP if !paused && !game.is_game_over() => {
                        game.rotate(Rotation::Cw);
                    }
                    input::KEYBOARD_KEY_ARROW_DOWN if !paused && !game.is_game_over() => {
                        game.soft_drop(&mut rng, &mut events);
                    }
                    input::KEYBOARD_KEY_SPACE if !paused && !game.is_game_over() => {
                        game.hard_drop(&mut rng, &mut events);
                    }
                    _ => {}
                }
            } else if event.kind == input::KEYBOARD_OUTPUT_KIND_TEXT {
                match char::from_u32(event.codepoint) {
                    Some('r' | 'R') => {
                        rng = Lcg32::new((clock::monotonic_millis() as u32) ^ 0xC11C_7E75);
                        game = Tetris::new(&mut rng, &mut events);
                        gravity_ms = 0;
                        paused = false;
                        paint = true;
                        text = true;
                    }
                    Some('p' | 'P') => {
                        paused = !paused;
                        text = true;
                    }
                    _ => {}
                }
            }
        }
        let now = clock::monotonic_millis();
        let elapsed = now.saturating_sub(last_tick).min(100);
        last_tick = now;
        if !paused && !game.is_game_over() {
            gravity_ms += elapsed;
            let interval = u64::from(game.level.level_speed_seconds());
            if gravity_ms >= interval {
                gravity_ms -= interval;
                game.soft_drop(&mut rng, &mut events);
            }
        }
        if game.consume_changed() {
            paint = true;
        }
        text |= previous_hud
            != (
                game.level.total_points,
                game.level.current_level,
                game.level.rows_deleted,
                game.is_game_over(),
            );
        if paint {
            match board.render(&mut background, &game, frame.width(), frame.height()) {
                Ok(()) => paint = false,
                Err(Error::Ui(_, UiError::Busy)) => {}
                Err(error) => return Err(error),
            }
        }
        if text {
            present_text(&mut frame, &game, paused)?;
            text = false;
        }
        vsys::poll_once();
        vsys::sleep_ms(16);
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
        game: &Tetris,
        width: u32,
        height: u32,
    ) -> Result<(), Error> {
        let mut bytes = Vec::with_capacity(MAX_SEEDS * 64);
        for row in 0..VISIBLE {
            for col in 0..COLS {
                let Some(cell) = game.cell_view_at(col, row + HIDDEN, false) else {
                    continue;
                };
                let color = rgb555(cell.color);
                let seed = RetainedTransformSeed {
                    translation: [-7.1 + col as f32 * CELL, 10.25 - row as f32 * CELL, 0.],
                    previous_translation: [-7.1 + col as f32 * CELL, 10.25 - row as f32 * CELL, 0.],
                    scale: [0.46, 0.46, 0.46],
                    rotation: [0., 0., 0., 1.],
                    local_radius: 1.74,
                    draw_group: 0,
                    flags: ((bytes.len() / 64) as u32) << 16 | color,
                };
                encode_seed(seed, &mut bytes);
            }
        }
        // A fresh game can have every active cell in its four hidden rows.
        // V3 still needs one valid patch seed to clear and present that frame.
        if bytes.is_empty() {
            encode_seed(
                RetainedTransformSeed {
                    translation: [0., 0., 100.],
                    previous_translation: [0., 0., 100.],
                    scale: [0.0001; 3],
                    rotation: [0., 0., 0., 1.],
                    local_radius: 1.74,
                    draw_group: 0,
                    flags: CUSTOM_RGB555,
                },
                &mut bytes,
            );
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
                xmag: 11.9,
                ymag: 13.55,
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

fn rgb555(color: Rgb8) -> u32 {
    let quantize = |channel: u8| (u32::from(channel) * 31 + 127) / 255;
    CUSTOM_RGB555 | quantize(color.r) | (quantize(color.g) << 5) | (quantize(color.b) << 10)
}

fn present_text(frame: &mut Frame, game: &Tetris, paused: bool) -> Result<(), Error> {
    let score = format!("SCORE  {}", game.level.total_points);
    let level_text = format!("LEVEL  {}", game.level.current_level);
    let rows_text = format!("LINES  {}", game.level.rows_deleted);
    let status = if game.is_game_over() {
        "GAME OVER"
    } else if paused {
        "PAUSED"
    } else {
        "PLAYING"
    };
    let white = rgba(230, 240, 250, 255);
    let muted = rgba(150, 172, 194, 255);
    let rows = [
        FontCanvasRow {
            text: "GAMES / TETRIS",
            x: 44.,
            y: 25.,
            font_pixels: 27.,
            color_rgba: white,
        },
        FontCanvasRow {
            text: &score,
            x: 462.,
            y: 114.,
            font_pixels: 17.,
            color_rgba: white,
        },
        FontCanvasRow {
            text: &level_text,
            x: 462.,
            y: 150.,
            font_pixels: 17.,
            color_rgba: white,
        },
        FontCanvasRow {
            text: &rows_text,
            x: 462.,
            y: 186.,
            font_pixels: 17.,
            color_rgba: white,
        },
        FontCanvasRow {
            text: status,
            x: 462.,
            y: 238.,
            font_pixels: 18.,
            color_rgba: white,
        },
        FontCanvasRow {
            text: "LEFT / RIGHT",
            x: 462.,
            y: 327.,
            font_pixels: 15.,
            color_rgba: muted,
        },
        FontCanvasRow {
            text: "MOVE",
            x: 462.,
            y: 348.,
            font_pixels: 14.,
            color_rgba: muted,
        },
        FontCanvasRow {
            text: "UP  ROTATE",
            x: 462.,
            y: 395.,
            font_pixels: 15.,
            color_rgba: muted,
        },
        FontCanvasRow {
            text: "DOWN  DROP",
            x: 462.,
            y: 427.,
            font_pixels: 15.,
            color_rgba: muted,
        },
        FontCanvasRow {
            text: "SPACE  FALL",
            x: 462.,
            y: 459.,
            font_pixels: 15.,
            color_rgba: muted,
        },
        FontCanvasRow {
            text: "P  PAUSE",
            x: 462.,
            y: 523.,
            font_pixels: 15.,
            color_rgba: muted,
        },
        FontCanvasRow {
            text: "R  RESTART",
            x: 462.,
            y: 555.,
            font_pixels: 15.,
            color_rgba: muted,
        },
    ];
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
