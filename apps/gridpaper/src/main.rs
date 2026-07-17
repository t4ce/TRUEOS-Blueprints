// trueos-blueprint: features=["gridpaper"]
#![no_std]

use gridpaper::{Cell, CellStyle, Color, GridPaper, GridPaperConfig, PublishMode, SnapshotCadence};
use trueos::{
    clock,
    logl::{self, level},
    rng, vshell,
};

fn main() {
    let start_ms = clock::monotonic_millis();
    let mut page = GridPaper::new(GridPaperConfig {
        cadence: SnapshotCadence::EveryEditsOrMillis {
            edits: 32,
            millis: 16,
        },
        publish_mode: PublishMode::PreserveIncrementalEdits,
        initial_time_ms: start_ms,
    });

    let (noise_seed, x_cells) = seed_perlin_page(&mut page, start_ms);
    submit_to_kernel(&page);

    logl::log(
        level::INFO,
        format_args!(
            "gridpaper: ready cells={} page_bytes={} double_buffer_bytes={} scale={}% generation={} perlin_seed=0x{:08x} x_cells={} o_cells={}",
            gridpaper::CELL_COUNT,
            gridpaper::PAGE_BYTES,
            gridpaper::DOUBLE_BUFFER_BYTES,
            page.scale_percent(),
            page.generation(),
            noise_seed,
            x_cells,
            gridpaper::CELL_COUNT - x_cells,
        ),
    );
    vshell::write(b"gridpaper: type UTF-8 for cell (0,0), `snapshot`, `clear`, or `quit`\n");

    let mut input = [0_u8; gridpaper::CELL_TEXT_CAPACITY + 2];
    loop {
        let read = vshell::read_blocking(&mut input);
        let command = trim_ascii(&input[..read]);
        match command {
            b"quit" => break,
            b"snapshot" => {
                let event = page.publish(clock::monotonic_millis());
                submit_to_kernel(&page);
                logl::log(
                    level::INFO,
                    format_args!("gridpaper: snapshot generation={}", event.generation()),
                );
            }
            b"clear" => {
                {
                    let mut edit = page.edit(clock::monotonic_millis());
                    edit.raw_mut().fill(0);
                    let _ = edit.finish();
                }
                let event = page.publish(clock::monotonic_millis());
                submit_to_kernel(&page);
                logl::log(
                    level::INFO,
                    format_args!("gridpaper: cleared generation={}", event.generation()),
                );
            }
            bytes => match core::str::from_utf8(bytes) {
                Ok(text) => match Cell::new(text, Color::BrightBlue, Color::White, CellStyle::BOLD)
                {
                    Ok(cell) => {
                        let mut edit = page.edit(clock::monotonic_millis());
                        let _ = edit.set_cell(0, 0, cell);
                        let _ = edit.finish();
                        let event = page.publish(clock::monotonic_millis());
                        submit_to_kernel(&page);
                        logl::log(
                            level::INFO,
                            format_args!(
                                "gridpaper: staged {:?} generation={}",
                                text,
                                event.generation(),
                            ),
                        );
                    }
                    Err(error) => {
                        logl::log(level::WARN, format_args!("gridpaper: {error}"));
                    }
                },
                Err(_) => logl::log(level::WARN, format_args!("gridpaper: invalid UTF-8")),
            },
        }
    }
    if let Err(error) = trueos::gridpaper::close() {
        logl::log(
            level::WARN,
            format_args!("gridpaper: kernel close failed: {error:?}"),
        );
    }
}

/// Fill the complete page in one edit and cross the kernel boundary once.
fn seed_perlin_page(page: &mut GridPaper, now_ms: u64) -> (u32, usize) {
    let seed = rng::u32();
    let x_cell = Cell::new("x", Color::BrightBlue, Color::White, CellStyle::NONE)
        .expect("single ASCII x fits a cell");
    let o_cell = Cell::new("o", Color::BrightCyan, Color::White, CellStyle::NONE)
        .expect("single ASCII o fits a cell");
    let offset_x = (seed & 0xff) as f32 / 83.0;
    let offset_y = ((seed >> 8) & 0xff) as f32 / 79.0;
    let mut x_cells = 0usize;

    {
        let mut edit = page.edit(now_ms);
        for row in 0..gridpaper::ROWS {
            for column in 0..gridpaper::COLUMNS {
                let x = column as f32 * 0.19 + offset_x;
                let y = row as f32 * 0.17 + offset_y;
                let cell = if perlin_2d(x, y, seed) >= 0.0 {
                    x_cells += 1;
                    x_cell
                } else {
                    o_cell
                };
                edit.set_cell(column, row, cell)
                    .expect("gridpaper startup coordinates are in bounds");
            }
        }
        let _ = edit.finish();
    }
    let _ = page.publish(now_ms);
    (seed, x_cells)
}

fn perlin_2d(x: f32, y: f32, seed: u32) -> f32 {
    let x0 = x as i32;
    let y0 = y as i32;
    let local_x = x - x0 as f32;
    let local_y = y - y0 as f32;
    let u = perlin_fade(local_x);
    let v = perlin_fade(local_y);

    let top = lerp(
        gradient_dot(noise_hash(x0, y0, seed), local_x, local_y),
        gradient_dot(noise_hash(x0 + 1, y0, seed), local_x - 1.0, local_y),
        u,
    );
    let bottom = lerp(
        gradient_dot(noise_hash(x0, y0 + 1, seed), local_x, local_y - 1.0),
        gradient_dot(
            noise_hash(x0 + 1, y0 + 1, seed),
            local_x - 1.0,
            local_y - 1.0,
        ),
        u,
    );
    lerp(top, bottom, v)
}

fn noise_hash(x: i32, y: i32, seed: u32) -> u32 {
    let mut value =
        seed ^ (x as u32).wrapping_mul(0x9e37_79b1) ^ (y as u32).wrapping_mul(0x85eb_ca77);
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^ (value >> 16)
}

fn gradient_dot(hash: u32, x: f32, y: f32) -> f32 {
    match hash & 7 {
        0 => x + y,
        1 => x - y,
        2 => -x + y,
        3 => -x - y,
        4 => x,
        5 => -x,
        6 => y,
        _ => -y,
    }
}

fn perlin_fade(value: f32) -> f32 {
    value * value * value * (value * (value * 6.0 - 15.0) + 10.0)
}

fn lerp(from: f32, to: f32, amount: f32) -> f32 {
    from + (to - from) * amount
}

fn submit_to_kernel(page: &GridPaper) {
    let snapshot = page.snapshot();
    if let Err(error) = trueos::gridpaper::submit_snapshot(
        snapshot.generation(),
        snapshot.scale_percent(),
        snapshot.raw(),
    ) {
        logl::log(
            level::WARN,
            format_args!("gridpaper: kernel snapshot submit failed: {error:?}"),
        );
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
