#![no_std]
#![no_main]

use core::panic::PanicInfo;
use trueos::{TrueosAllocator, panic_abort};
use trueos::{platform, vshell};
use trueos_tetris::shell::{ShellControl, ShellIo};

#[global_allocator]
static GLOBAL_ALLOCATOR: TrueosAllocator = TrueosAllocator;

struct AttachedShell;

impl ShellIo for AttachedShell {
    fn write_str(&self, s: &str) {
        let _ = vshell::attached_write_str(s);
    }

    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        let _ = vshell::attached_write_fmt(args);
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    panic_abort("cli_tetris bp: panic\n")
}

#[unsafe(no_mangle)]
pub extern "C" fn main() {
    let seed = 0xC11C_7E75;
    let mut app = trueos_tetris::shell::ShellApp::new(seed, 120, 32);
    app.set_terminal_size(120, 32);
    app.set_viewport_top_row(1);

    let io = AttachedShell;
    platform::log_info("cli_tetris bp: attached shell mode\n");
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
