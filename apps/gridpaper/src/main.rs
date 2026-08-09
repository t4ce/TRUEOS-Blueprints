// trueos-blueprint: features=["ui4-scene"]
#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use gridpaper::{Cell, CellStyle, Color, GridPaper, GridPaperConfig, PublishMode, SnapshotCadence};
use trueos::{
    clock, env,
    logl::{self, level},
    replication,
    ui4_scene::{
        CloseRequest, Damage, Error as Ui4Error, Font, Frame, SceneTextRow, output_dimensions, rgba,
    },
    vshell, vsys,
};

const CHECKPOINT_VERSION: u64 = 1;
const FRAME_MARGIN: u32 = 40;
const FRAME_CASCADE_PIXELS: u32 = 56;
const FRAME_CASCADE_STEPS: u32 = 4;
const FRAME_PADDING: f32 = 20.0;
const CELL_PIXELS: f32 = 18.0;
const PAPER_COLOR: u32 = rgba(248, 250, 252, 255);
const TEXT_COLOR_COUNT: usize = 17;

const ACTIVE_TEXT_COLORS: [Color; TEXT_COLOR_COUNT] = [
    Color::Default,
    Color::Black,
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    Color::White,
    Color::BrightBlack,
    Color::BrightRed,
    Color::BrightGreen,
    Color::BrightYellow,
    Color::BrightBlue,
    Color::BrightMagenta,
    Color::BrightCyan,
    Color::BrightWhite,
];

const UNICODE_WAVES: [[&str; TEXT_COLOR_COUNT]; 3] = [
    [
        "α", "β", "γ", "δ", "λ", "π", "Σ", "Ω", "∞", "∫", "√", "≈", "≠", "≤", "≥", "±", "∂",
    ],
    [
        "Ж", "Я", "Д", "Ф", "Ю", "♪", "♫", "▲", "△", "◆", "◇", "●", "○", "♠", "♣", "♥", "♦",
    ],
    [
        "←", "↖", "↑", "↗", "→", "↘", "↓", "↙", "⇐", "⇑", "⇒", "⇓", "⇔", "⊕", "⊗", "⊙", "⊥",
    ],
];
const ASCII_DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
const ASCII_LETTERS: [&str; 7] = ["a", "b", "c", "d", "e", "f", "g"];
const ASCII_SPECIMEN_ROWS: [(usize, usize); 3] = [(14, 17), (31, 34), (48, 51)];
const ASCII_SPECIMEN_STYLES: [CellStyle; ASCII_SPECIMEN_ROWS.len()] =
    [CellStyle::NONE, CellStyle::BOLD, CellStyle::ITALIC];
const ASCII_SPECIMEN_COLOR_PHASES: [usize; ASCII_SPECIMEN_ROWS.len()] = [0, 5, 10];
const WAVE_BASE_ROWS: [usize; UNICODE_WAVES.len()] = [5, 22, 39];
const WAVE_ROW_OFFSETS: [usize; TEXT_COLOR_COUNT] =
    [0, 1, 3, 5, 6, 5, 3, 1, 0, 1, 3, 5, 6, 5, 3, 1, 0];

#[derive(Clone, Copy)]
struct GridSize {
    columns: usize,
    rows: usize,
}

impl GridSize {
    const FULL: Self = Self {
        columns: gridpaper::COLUMNS,
        rows: gridpaper::ROWS,
    };
}

fn main() {
    let grid_size = match requested_grid_size() {
        Ok(size) => size,
        Err(error) => {
            logl::log(level::ERROR, error);
            return;
        }
    };
    let start_ms = clock::monotonic_millis();
    let config = GridPaperConfig {
        cadence: SnapshotCadence::EveryEditsOrMillis {
            edits: 32,
            millis: 16,
        },
        publish_mode: PublishMode::PreserveIncrementalEdits,
        initial_time_ms: start_ms,
    };
    let mut page = GridPaper::new(config);
    initialize_unicode_demo(&mut page, start_ms);

    let mut frame = match open_frame(grid_size) {
        Ok(frame) => frame,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("gridpaper: UI4 frame open failed error={error:?}"),
            );
            return;
        }
    };
    if let Err(error) = present_page(&mut frame, &page, grid_size) {
        logl::log(
            level::ERROR,
            format_args!("gridpaper: initial UI4 publish failed error={error:?}"),
        );
        return;
    }

    logl::log(
        level::INFO,
        format_args!(
            "gridpaper: direct UI4 frame submitted window={} grid={}x{} font=NotoSansSc animations=ignored renderer=blueprint-font-canvas",
            frame.window_id(),
            grid_size.columns,
            grid_size.rows,
        ),
    );

    let mut input = [0_u8; 64];
    loop {
        if let Some(prepare) = replication::poll_prepare_pause() {
            if let Err(error) = recreate_after_pause(prepare, &mut frame, &page, grid_size) {
                logl::log(
                    level::WARN,
                    format_args!("gridpaper: replication UI4 recreate failed error={error:?}"),
                );
            }
            continue;
        }

        if frame.take_first_presentation().unwrap_or(false) {
            logl::log(
                level::INFO,
                format_args!(
                    "gridpaper: direct UI4 frame visible window={}",
                    frame.window_id()
                ),
            );
        }
        if let Err(error) = drain_ui4_pointer_input(&mut frame) {
            logl::log(
                level::WARN,
                format_args!("gridpaper: UI4 pointer drain failed error={error:?}"),
            );
        }

        let read = vshell::read(&mut input);
        if read == 0 {
            vsys::poll_once();
            vsys::sleep_ms(16);
            continue;
        }
        let command = trim_ascii(&input[..read]);
        let update = match command {
            b"quit" => {
                let _ = frame.close(CloseRequest::default());
                return;
            }
            b"snapshot" => true,
            b"clear" => {
                let now_ms = clock::monotonic_millis();
                let mut edit = page.edit(now_ms);
                edit.raw_mut().fill(0);
                let _ = edit.finish();
                let _ = page.publish(now_ms);
                true
            }
            bytes => match core::str::from_utf8(bytes) {
                Ok(text) => match Cell::new(text, Color::BrightBlue, Color::White, CellStyle::BOLD)
                {
                    Ok(cell) => {
                        let now_ms = clock::monotonic_millis();
                        let mut edit = page.edit(now_ms);
                        let _ = edit.set_cell(0, 0, cell);
                        let _ = edit.finish();
                        let _ = page.publish(now_ms);
                        true
                    }
                    Err(error) => {
                        logl::log(level::WARN, format_args!("gridpaper: {error}"));
                        false
                    }
                },
                Err(_) => {
                    logl::log(level::WARN, format_args!("gridpaper: invalid UTF-8"));
                    false
                }
            },
        };
        if update && let Err(error) = present_page(&mut frame, &page, grid_size) {
            logl::log(
                level::WARN,
                format_args!("gridpaper: direct UI4 publish failed error={error:?}"),
            );
        }
    }
}

fn recreate_after_pause(
    prepare: replication::PreparePause,
    frame: &mut Frame,
    page: &GridPaper,
    grid_size: GridSize,
) -> Result<(), Ui4Error> {
    let replacement = open_frame(grid_size)?;
    let old_frame = core::mem::replace(frame, replacement);
    let _ = old_frame.close(CloseRequest::default());
    present_page(frame, page, grid_size)?;
    let resume = replication::ready(prepare, CHECKPOINT_VERSION);
    logl::log(
        level::INFO,
        format_args!(
            "gridpaper: PreparePause operation={} reason={:?}; direct UI4 frame rebuilt resume={:?}",
            prepare.operation(),
            prepare.reason,
            resume,
        ),
    );
    Ok(())
}

fn open_frame(grid_size: GridSize) -> Result<Frame, Ui4Error> {
    let (output_width, output_height) = output_dimensions().unwrap_or((2_560, 1_440));
    let natural_width = FRAME_PADDING * 2.0 + grid_size.columns as f32 * CELL_PIXELS;
    let natural_height = FRAME_PADDING * 2.0 + grid_size.rows as f32 * CELL_PIXELS;
    let available_width = output_width.saturating_sub(FRAME_MARGIN * 2).max(1) as f32;
    let available_height = output_height.saturating_sub(FRAME_MARGIN * 2).max(1) as f32;
    let scale = (available_width / natural_width)
        .min(available_height / natural_height)
        .min(1.0);
    let width = (natural_width * scale).max(1.0) as u32;
    let height = (natural_height * scale).max(1.0) as u32;
    let offset = instance_cascade_offset();
    let x = output_width
        .saturating_sub(width)
        .saturating_div(2)
        .saturating_add(offset)
        .min(output_width.saturating_sub(width)) as i32;
    let y = output_height
        .saturating_sub(height)
        .saturating_div(2)
        .saturating_add(offset)
        .min(output_height.saturating_sub(height)) as i32;
    Frame::open_immutable(x, y, width, height)
}

fn instance_cascade_offset() -> u32 {
    let instance_name = env::var("TRUEOS_APP_INSTANCE_NAME").ok();
    let instance_index = instance_name
        .as_deref()
        .and_then(|name| name.rsplit_once('_'))
        .and_then(|(_, suffix)| suffix.parse::<u32>().ok())
        .unwrap_or(0);
    instance_index % FRAME_CASCADE_STEPS * FRAME_CASCADE_PIXELS
}

fn drain_ui4_pointer_input(frame: &mut Frame) -> Result<(), Ui4Error> {
    while frame.take_pointer_event()?.is_some() {}
    while frame.take_pan_event()?.is_some() {}
    Ok(())
}

fn present_page(frame: &mut Frame, page: &GridPaper, grid_size: GridSize) -> Result<(), Ui4Error> {
    let scale = ((frame.width() as f32 - FRAME_PADDING * 2.0) / grid_size.columns as f32)
        .min((frame.height() as f32 - FRAME_PADDING * 2.0) / grid_size.rows as f32);
    let origin_x = (frame.width() as f32 - grid_size.columns as f32 * scale) * 0.5;
    let origin_y = (frame.height() as f32 - grid_size.rows as f32 * scale) * 0.5;
    let snapshot = page.snapshot();
    let mut cells = Vec::with_capacity(grid_size.columns * grid_size.rows);
    for row in 0..grid_size.rows {
        for column in 0..grid_size.columns {
            cells.push(snapshot.cell(column, row).unwrap_or_else(|_| Cell::blank()));
        }
    }

    let mut rows_by_color: [Vec<SceneTextRow<'_>>; TEXT_COLOR_COUNT] =
        core::array::from_fn(|_| Vec::new());

    for row in 0..grid_size.rows {
        for column in 0..grid_size.columns {
            let cell = &cells[row * grid_size.columns + column];
            let x = origin_x + column as f32 * scale;
            let y = origin_y + row as f32 * scale;
            let color_index = cell.foreground() as usize;
            let rows = rows_by_color
                .get_mut(color_index)
                .expect("validated Gridpaper foreground color");
            if !cell.primary().is_empty() {
                rows.push(SceneTextRow {
                    text: cell.primary(),
                    x: x + scale * 0.22,
                    y: y + scale * 0.08,
                    font_pixels: scale * 0.72,
                });
            }
            if let Some(upper) = cell.upper() {
                rows.push(SceneTextRow {
                    text: upper,
                    x: x + scale * 0.62,
                    y,
                    font_pixels: scale * 0.42,
                });
            }
        }
    }

    retry_busy(|| frame.begin(PAPER_COLOR))?;
    for (color, rows) in ACTIVE_TEXT_COLORS.iter().copied().zip(rows_by_color.iter()) {
        if rows.is_empty() {
            continue;
        }
        retry_busy(|| {
            frame.stamp_text_scene(
                Font::NotoSansSc,
                (frame.width(), frame.height()),
                color_rgba(color),
                rows,
            )
        })?;
    }
    retry_busy(|| frame.publish(Damage::full(frame.width(), frame.height())))
}

fn color_rgba(color: Color) -> u32 {
    let (red, green, blue) = match color {
        Color::Default => (26, 38, 54),
        Color::Black => (0, 0, 0),
        Color::Red => (190, 52, 52),
        Color::Green => (42, 132, 82),
        Color::Yellow => (168, 118, 20),
        Color::Blue => (45, 96, 190),
        Color::Magenta => (160, 54, 154),
        Color::Cyan => (20, 130, 146),
        Color::White => (246, 248, 250),
        Color::BrightBlack => (94, 108, 124),
        Color::BrightRed => (232, 69, 69),
        Color::BrightGreen => (52, 173, 102),
        Color::BrightYellow => (224, 170, 42),
        Color::BrightBlue => (78, 130, 238),
        Color::BrightMagenta => (212, 76, 204),
        Color::BrightCyan => (44, 188, 207),
        Color::BrightWhite => (255, 255, 255),
        Color::Transparent => (0, 0, 0),
    };
    rgba(
        red,
        green,
        blue,
        if color == Color::Transparent { 0 } else { 255 },
    )
}

fn initialize_unicode_demo(page: &mut GridPaper, now_ms: u64) {
    let mut edit = page.edit(now_ms);
    edit.raw_mut().fill(0);
    for (wave_index, glyphs) in UNICODE_WAVES.iter().enumerate() {
        for (selector, glyph) in glyphs.iter().enumerate() {
            let column = 2 + selector * 2;
            let row = WAVE_BASE_ROWS[wave_index] + WAVE_ROW_OFFSETS[selector];
            let style = match wave_index {
                0 => CellStyle::NONE,
                1 => CellStyle::BOLD,
                _ => match selector % 4 {
                    0 => CellStyle::UNDERLINE,
                    1 => CellStyle::STRIKEOUT,
                    2 => CellStyle::ITALIC,
                    _ => CellStyle::BOLD.union(CellStyle::UNDERLINE),
                },
            };
            let cell = Cell::new(
                glyph,
                ACTIVE_TEXT_COLORS[selector],
                Color::Transparent,
                style,
            )
            .expect("static Unicode demo glyph fits one cell");
            edit.set_cell(column, row, cell)
                .expect("static Unicode demo coordinate is in bounds");
        }
    }
    for (specimen, ((digit_row, letter_row), style)) in ASCII_SPECIMEN_ROWS
        .iter()
        .copied()
        .zip(ASCII_SPECIMEN_STYLES.iter().copied())
        .enumerate()
    {
        let color_phase = ASCII_SPECIMEN_COLOR_PHASES[specimen];
        for (index, glyph) in ASCII_DIGITS.iter().enumerate() {
            let cell = Cell::new(
                glyph,
                ACTIVE_TEXT_COLORS[(index + color_phase) % ACTIVE_TEXT_COLORS.len()],
                Color::Transparent,
                style,
            )
            .expect("static ASCII digit fits one cell");
            edit.set_cell(9 + index * 2, digit_row, cell)
                .expect("static ASCII digit coordinate is in bounds");
        }
        for (index, glyph) in ASCII_LETTERS.iter().enumerate() {
            let cell = Cell::new(
                glyph,
                ACTIVE_TEXT_COLORS[(index + 10 + color_phase) % ACTIVE_TEXT_COLORS.len()],
                Color::Transparent,
                style,
            )
            .expect("static ASCII letter fits one cell");
            edit.set_cell(12 + index * 2, letter_row, cell)
                .expect("static ASCII letter coordinate is in bounds");
        }
    }
    edit.set_cell(
        18,
        11,
        Cell::with_upper(
            "x",
            "²",
            Color::BrightBlue,
            Color::Transparent,
            CellStyle::NONE,
        )
        .expect("static x-squared demo fits one cell"),
    )
    .expect("static x-squared demo coordinate is in bounds");
    let _ = edit.finish();
    let _ = page.publish(now_ms);
}

fn requested_grid_size() -> Result<GridSize, &'static str> {
    let mut args = env::args().skip(1);
    let Some(first) = args.next() else {
        return Ok(GridSize::FULL);
    };
    let Some((columns, rows)) = first
        .split_once('x')
        .or_else(|| first.split_once('X'))
        .or_else(|| first.split_once("by"))
    else {
        return Err("gridpaper: expected grid size as COLUMNSxROWS");
    };
    if args.next().is_some() {
        return Err("gridpaper: expected one grid size, for example 12x20");
    }
    let columns = columns
        .parse::<usize>()
        .map_err(|_| "gridpaper: grid columns must be a positive integer")?;
    let rows = rows
        .parse::<usize>()
        .map_err(|_| "gridpaper: grid rows must be a positive integer")?;
    if columns == 0 || columns > gridpaper::COLUMNS || rows == 0 || rows > gridpaper::ROWS {
        return Err("gridpaper: grid size must be within 1x1 and 39x55");
    }
    Ok(GridSize { columns, rows })
}

fn retry_busy(mut operation: impl FnMut() -> Result<(), Ui4Error>) -> Result<(), Ui4Error> {
    loop {
        match operation() {
            Ok(()) => return Ok(()),
            Err(Ui4Error::Busy) => {
                vsys::poll_once();
                vsys::sleep_ms(1);
            }
            Err(error) => return Err(error),
        }
    }
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}
