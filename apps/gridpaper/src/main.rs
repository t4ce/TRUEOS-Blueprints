#![no_std]

use gridpaper::{Cell, CellStyle, Color, GridPaper, GridPaperConfig, PublishMode, SnapshotCadence};
use trueos::{
    clock,
    logl::{self, level},
    vshell,
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

    let title = Cell::new(
        "gridpaper",
        Color::BrightBlue,
        Color::White,
        CellStyle::BOLD | CellStyle::UNDERLINE,
    )
    .expect("static gridpaper title fits a cell");
    {
        let mut edit = page.edit(start_ms);
        edit.set_cell(0, 0, title)
            .expect("static gridpaper coordinate is valid");
    }
    let _ = page.publish(start_ms);

    logl::log(
        level::INFO,
        format_args!(
            "gridpaper: ready cells={} page_bytes={} double_buffer_bytes={} scale={}% generation={}",
            gridpaper::CELL_COUNT,
            gridpaper::PAGE_BYTES,
            gridpaper::DOUBLE_BUFFER_BYTES,
            page.scale_percent(),
            page.generation(),
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
                logl::log(
                    level::INFO,
                    format_args!("gridpaper: snapshot generation={}", event.generation()),
                );
            }
            b"clear" => {
                let mut edit = page.edit(clock::monotonic_millis());
                edit.raw_mut().fill(0);
                let event = edit.finish();
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
                        let event = edit.finish();
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
