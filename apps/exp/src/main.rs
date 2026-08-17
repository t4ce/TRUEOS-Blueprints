#![no_std]

use trueos::{platform, vshell, vsys};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResumeMode {
    Exit,
    WaitReentry,
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn write_u32_decimal(mut value: u32, out: &mut [u8]) -> usize {
    let mut scratch = [0u8; 16];
    let mut used = 0;

    if value == 0 {
        out[0] = b'0';
        return 1;
    }

    while value > 0 {
        scratch[used] = b'0' + (value % 10) as u8;
        used += 1;
        value /= 10;
    }

    for i in 0..used {
        out[i] = scratch[used - i - 1];
    }

    used
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn move_cursor(row: u32, col: u32) {
    let mut buf = [0u8; 24];
    let mut n = 0;
    buf[n] = b'\x1b';
    n += 1;
    buf[n] = b'[';
    n += 1;
    n += write_u32_decimal(row, &mut buf[n..]);
    buf[n] = b';';
    n += 1;
    n += write_u32_decimal(col, &mut buf[n..]);
    buf[n] = b'H';
    n += 1;
    let _ = vshell::attached_write(&buf[..n]);
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn draw_border_row(row: u32, cols: u32) {
    // Use single-cell ASCII symbols for deterministic terminal width.
    // Multi-byte Unicode ornaments (like ▴▾ variants) can measure as >1 column
    // in some terminals and cause apparent "bubble" artifacts on resize.
    const TILE_A: &[u8; 1] = b"-";
    const TILE_B: &[u8; 1] = b"=";
    const CHUNK_MAX: usize = 256;
    let mut wrote_cols = 0u32;
    if cols == 0 {
        return;
    }

    while wrote_cols < cols {
        let remaining = cols - wrote_cols;
        let n = core::cmp::min(remaining as usize, CHUNK_MAX);
        let mut chunk = [0u8; CHUNK_MAX];
        let mut i = 0usize;
        while i < n {
            let col = wrote_cols + i as u32;
            chunk[i] = if col % 2 == 0 { TILE_A[0] } else { TILE_B[0] };
            i += 1;
        }
        // konsole_write_row appears to be 0-based and clamps at the frame edge.
        // Use 0-based row/col coordinates to avoid silently dropping whole border rows.
        let _ = vshell::konsole_write_row(
            row.saturating_sub(1),
            wrote_cols,
            &chunk[..n],
        );
        wrote_cols += n as u32;
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn draw_frame(cols: u32, rows: u32) {
    let _ = vshell::attached_write(b"\x1b[2J\x1b[H");
    if cols == 0 || rows == 0 {
        return;
    }

    draw_border_row(1, cols);

    if rows > 1 {
        draw_border_row(rows, cols);
    }

    // Put cursor in content area (row 2, col 1) after border redraw.
    if rows >= 2 {
        move_cursor(2, 1);
    }
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn draw_frame(_: u32, _: u32) {}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn terminal_enter() {
    let size = vshell::konsole_size().unwrap_or(vshell::KonsoleSize {
        cols: 80,
        rows: 24,
    });
    let _ = vshell::konsole_begin_frame(
        size.cols,
        size.rows,
        vshell::KONSOLE_FRAME_TERMINAL_HANDOFF,
    );
    let _ = vshell::attached_write(b"\x1b[?1049h\x1b[?25h");
    draw_frame(size.cols, size.rows);
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn terminal_enter() {}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn terminal_exit(full_exit: bool) {
    // Always restore the terminal frame/cursor before handing control back.
    let _ = vshell::attached_write(b"\x1b[?1049l\x1b[?25h\x1b[2J\x1b[H");
    let _ = vshell::konsole_end_frame();

    // "Exit UI": release terminal handoff so the host shell regains control.
    vshell::leave_terminal_handoff();

    // "Exit full app": terminate blueprint process so it can be relaunched cleanly.
    if full_exit {
        let _ = vshell::shutdown_current_blueprint("exp terminated");
    }
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn terminal_exit(_full_exit: bool) {}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn maybe_redraw_frame(last_cols: &mut u32, last_rows: &mut u32) {
    if let Some(size) = vshell::konsole_size() {
        if size.cols != *last_cols || size.rows != *last_rows {
            draw_frame(size.cols, size.rows);
            *last_cols = size.cols;
            *last_rows = size.rows;
        }
    }
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn maybe_redraw_frame(_last_cols: &mut u32, _last_rows: &mut u32) {}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn wait_for_terminal_reentry() {
    let size = vshell::konsole_size().unwrap_or(vshell::KonsoleSize {
        cols: 80,
        rows: 24,
    });
    let mut shell2 = vshell::Shell2Frontend::attach(size.cols, size.rows).ok();
    let mut last_cols = size.cols;
    let mut last_rows = size.rows;
    loop {
        maybe_redraw_frame(&mut last_cols, &mut last_rows);

        if let Some(byte) = vshell::attached_read_byte() {
            // 0x1f is the host reentry marker; we pause here so vmx can resume this app cleanly.
            if byte == 0x1f {
                return;
            }
            if let Some(frontend) = shell2.as_mut() {
                let _ = frontend.submit_input(&[byte]);
            }
            platform::poll_once();
            vsys::sleep_ms(5);
            continue;
        }
        platform::poll_once();
        vsys::sleep_ms(5);
    }
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn wait_for_terminal_reentry() {}

fn main() {
    loop {
        let mut mode = ResumeMode::Exit;
        let mut last_cols = 0u32;
        let mut last_rows = 0u32;

        terminal_enter();
        if let Some(size) = vshell::konsole_size() {
            last_cols = size.cols;
            last_rows = size.rows;
        }

        'session: loop {
            maybe_redraw_frame(&mut last_cols, &mut last_rows);
            if let Some(byte) = vshell::attached_read_byte() {
                match byte {
                    0x11 => {
                        mode = ResumeMode::Exit;
                        break 'session;
                    }
                    0x12 => {
                        mode = ResumeMode::WaitReentry;
                        break 'session;
                    }
                    b'\r' | b'\n' => {
                        let _ = vshell::attached_write(b"\r\n");
                    }
                    0x08 | 0x7f => {
                        let _ = vshell::attached_write(b"\x08 \x08");
                    }
                    _ if byte.is_ascii_graphic() || byte == b' ' => {
                        let _ = vshell::attached_write(&[byte]);
                    }
                    _ => {}
                }
            } else {
                platform::poll_once();
                vsys::sleep_ms(10);
            }
        }

        terminal_exit(mode == ResumeMode::Exit);
        if mode == ResumeMode::Exit {
            return;
        }

        wait_for_terminal_reentry();
    }
}
