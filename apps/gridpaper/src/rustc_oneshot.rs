#![no_std]
#![no_main]

const COLUMNS: usize = 39;
const ROWS: usize = 55;
const CELL_BYTES: usize = 13;
const PAGE_BYTES: usize = COLUMNS * ROWS * CELL_BYTES;

const COLOR_BRIGHT_RED: u8 = 10;
const COLOR_BRIGHT_GREEN: u8 = 11;
const COLOR_BRIGHT_YELLOW: u8 = 12;
const COLOR_BRIGHT_BLUE: u8 = 13;
const COLOR_BRIGHT_MAGENTA: u8 = 14;
const COLOR_BRIGHT_CYAN: u8 = 15;
const COLOR_BRIGHT_WHITE: u8 = 16;
const COLOR_TRANSPARENT: u8 = 17;
const STYLE_BOLD: u8 = 1;

unsafe extern "C" {
    fn trueos_cabi_gridpaper_snapshot_submit(
        generation: u64,
        scale_percent: u32,
        raw_ptr: *const u8,
        raw_len: usize,
    ) -> i32;
    fn trueos_cabi_poll_once();
    fn trueos_cabi_sleep_ms(ms: u64);
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut page = [0_u8; PAGE_BYTES];
    draw_rustc_gridpaper(&mut page);

    let status = unsafe {
        trueos_cabi_gridpaper_snapshot_submit(1, 100, page.as_ptr(), page.len())
    };
    if status == 0 {
        write_log(b"rustc one-shot: Gridpaper snapshot submitted\n");
    } else {
        write_log(b"rustc one-shot: Gridpaper snapshot rejected\n");
    }

    loop {
        unsafe {
            trueos_cabi_poll_once();
            trueos_cabi_sleep_ms(16);
        }
    }
}

fn draw_rustc_gridpaper(page: &mut [u8; PAGE_BYTES]) {
    let title = b"RUSTC ONE-SHOT GRIDPAPER";
    let colors = [
        COLOR_BRIGHT_RED,
        COLOR_BRIGHT_YELLOW,
        COLOR_BRIGHT_GREEN,
        COLOR_BRIGHT_CYAN,
        COLOR_BRIGHT_BLUE,
        COLOR_BRIGHT_MAGENTA,
        COLOR_BRIGHT_WHITE,
    ];

    let mut index = 0;
    while index < title.len() {
        let color = colors[index % colors.len()];
        set_ascii_cell(page, 8 + index, 7, title[index], color, STYLE_BOLD);
        index += 1;
    }

    let mut row = 13;
    while row < 44 {
        let mut column = 4;
        while column < 35 {
            let phase = (column + row) % colors.len();
            let glyph = if (column + row) % 3 == 0 {
                b'x'
            } else if (column * 3 + row) % 5 == 0 {
                b'+'
            } else {
                b'.'
            };
            set_ascii_cell(page, column, row, glyph, colors[phase], 0);
            column += 2;
        }
        row += 2;
    }

    let footer = b"compiled inside TRUEOS";
    index = 0;
    while index < footer.len() {
        set_ascii_cell(
            page,
            9 + index,
            49,
            footer[index],
            COLOR_BRIGHT_CYAN,
            0,
        );
        index += 1;
    }
}

fn set_ascii_cell(
    page: &mut [u8; PAGE_BYTES],
    column: usize,
    row: usize,
    glyph: u8,
    foreground: u8,
    style: u8,
) {
    if column >= COLUMNS || row >= ROWS {
        return;
    }
    let offset = (row * COLUMNS + column) * CELL_BYTES;
    unsafe {
        let cell = page.as_mut_ptr().add(offset);
        cell.write(1);
        cell.add(1).write(0);
        cell.add(2).write(foreground);
        cell.add(3).write(COLOR_TRANSPARENT);
        cell.add(4).write(style);
        cell.add(5).write(glyph);
    }
}

fn write_log(message: &[u8]) {
    unsafe {
        trueos_cabi_write(1, message.as_ptr(), message.len());
    }
}
