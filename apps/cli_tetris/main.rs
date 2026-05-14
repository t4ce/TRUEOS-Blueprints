#![no_std]

use trueos::logl::{self, level};
use trueos::{platform, vshell};
use trueos_tetris::shell::{ShellControl, ShellIo};

struct AttachedShell;

impl ShellIo for AttachedShell {
    fn write_str(&self, s: &str) {
        let _ = vshell::attached_write_str(s);
    }

    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        let _ = vshell::attached_write_fmt(args);
    }
}

fn main() {
    let seed = 0xC11C_7E75;
    let mut app = trueos_tetris::shell::ShellApp::new(seed, 120, 32);
    app.set_terminal_size(120, 32);
    app.set_viewport_top_row(1);

    let io = AttachedShell;
    logl::log(level::INFO, "cli_tetris bp: attached shell mode\n");
    vshell::attached_write_str("\x1b[2J\x1b[H\x1b[?25l");
    app.draw(&io);
    app.finalize_frame();

    loop {
        while let Some(b) = vshell::attached_read_byte() {
            if matches!(app.handle_input_byte(b), ShellControl::Exit) {
                vshell::attached_write_str("\x1b[?25h\x1b[0m\x1b[2J\x1b[H");
                return;
            }
        }

        app.tick(16);
        if app.consume_redraw() {
            app.draw(&io);
            app.finalize_frame();
        }
        platform::sleep_ms(16);
    }
}
