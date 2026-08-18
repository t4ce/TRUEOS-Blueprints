#![no_std]

use trueos::{platform, vshell, vsys};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResumeMode {
    Exit,
    WaitReentry,
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn terminal_enter() {
    let size = vshell::konsole_size().unwrap_or(vshell::KonsoleSize { cols: 80, rows: 24 });
    let _ =
        vshell::konsole_begin_frame(size.cols, size.rows, vshell::KONSOLE_FRAME_TERMINAL_HANDOFF);
    let _ = vshell::attached_write(
        b"\x1b[?1049h\x1b[2J\x1b[H\x1b[?25ltemplate terminal handoff active\r\n\
        Q or Esc: exit app\r\n\
        R: return to Shell2; use vmx tui to reenter\r\n",
    );
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn terminal_enter() {}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn terminal_restore() {
    // Always restore the terminal frame/cursor before handing control back.
    let _ = vshell::attached_write(b"\x1b[?1049l\x1b[?25h\x1b[2J\x1b[H");
    let _ = vshell::konsole_end_frame();
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn terminal_restore() {}

fn main() {
    let mut lease = match vshell::terminal_initial_lease() {
        Ok(lease) => lease,
        Err(_) => {
            let _ = vshell::report_exit_reason("template terminal lease unavailable");
            let _ = vshell::shutdown_current_blueprint("template terminal lease unavailable");
            return;
        }
    };

    loop {
        terminal_enter();
        if lease.acknowledge_ready().is_err() {
            terminal_restore();
            let _ = vshell::report_exit_reason("template terminal lease could not become ready");
            let _ = vshell::shutdown_current_blueprint(
                "template terminal lease could not become ready",
            );
            return;
        }

        let mode = 'session: loop {
            if let Some(byte) = vshell::attached_read_byte() {
                match byte {
                    b'q' | b'Q' | b'\x1b' => break 'session ResumeMode::Exit,
                    b'r' | b'R' => break 'session ResumeMode::WaitReentry,
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
        };

        // The TUI must be completely restored before the lease is returned;
        // Shell2 owns input for the entire parking interval.
        terminal_restore();
        if mode == ResumeMode::Exit {
            match lease.release_to_shell() {
                Ok(_) => {
                    let _ = vshell::shutdown_current_blueprint("template terminal terminated");
                }
                Err(_) => {
                    let _ = vshell::report_exit_reason("template terminal lease release failed");
                    let _ = vshell::shutdown_current_blueprint(
                        "template terminal lease release failed",
                    );
                }
            }
            return;
        }

        let ticket = match lease.release_to_shell() {
            Ok(ticket) => ticket,
            Err(_) => {
                let _ = vshell::report_exit_reason("template terminal lease release failed");
                let _ =
                    vshell::shutdown_current_blueprint("template terminal lease release failed");
                return;
            }
        };
        lease = match ticket.wait_for_reentry() {
            Ok(lease) => lease,
            Err(_) => {
                let _ = vshell::report_exit_reason("template terminal lease reentry failed");
                let _ =
                    vshell::shutdown_current_blueprint("template terminal lease reentry failed");
                return;
            }
        };
    }
}
