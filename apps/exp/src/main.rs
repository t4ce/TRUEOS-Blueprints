#![no_std]

use trueos::{platform, vshell, vsys};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResumeMode {
    Exit,
    WaitReentry,
}

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
    let _ = vshell::attached_write(
        b"\x1b[?1049h\x1b[2J\x1b[H\x1b[?25lexp: terminal handoff active\r\n\
        Ctrl+Q: exit app\r\n\
        Ctrl+R: suspend and reenter from vmx tui\r\n",
    );
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
fn wait_for_terminal_reentry() {
    let size = vshell::konsole_size().unwrap_or(vshell::KonsoleSize {
        cols: 80,
        rows: 24,
    });
    let mut shell2 = vshell::Shell2Frontend::attach(size.cols, size.rows).ok();
    loop {
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

        terminal_enter();

        'session: loop {
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
