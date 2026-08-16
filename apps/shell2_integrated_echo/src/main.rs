#![no_std]

use trueos::{platform, vshell, vsys};

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
    let _ = vshell::attached_write(b"exp> ");
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn terminal_enter() {}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn terminal_exit() {
    let _ = vshell::konsole_end_frame();
    vshell::leave_terminal_handoff();
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn terminal_exit() {}

fn main() {
    terminal_enter();

    loop {
        while let Some(byte) = vshell::attached_read_byte() {
            match byte {
                b'\x03' | 0x1b => {
                    terminal_exit();
                    return;
                }
                b'\r' | b'\n' => {
                    let _ = vshell::attached_write(b"\r\n");
                    let _ = vshell::attached_write(b"exp> ");
                }
                _ => {
                    let _ = vshell::attached_write(&[byte]);
                }
            }
        }

        platform::poll_once();
        vsys::sleep_ms(10);
    }
}
