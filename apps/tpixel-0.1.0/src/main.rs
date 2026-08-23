mod app;
mod canvas;
mod screen;

use std::{
    env,
    error::Error,
    io::{self, stdout, BufWriter},
};

use app::App;
use crossterm::{
    cursor::{Hide, Show},
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    style::ResetColor,
    terminal::{
        self, DisableLineWrap, EnableLineWrap, EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use screen::Renderer;

const FRAME_OUTPUT_BUFFER_CAPACITY: usize = 64 * 1024;

fn main() {
    if let Err(error) = run() {
        eprintln!("tpixel: {error}");
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let config = Config::from_environment();
    let mut terminal_guard = TerminalGuard::enter()?;
    let mut app = App::new(config.seed_demo);
    let mut renderer = Renderer::default();
    let mut output = BufWriter::with_capacity(FRAME_OUTPUT_BUFFER_CAPACITY, stdout());

    let result = app.run(&mut output, &mut renderer);
    terminal_guard.restore()?;
    result?;
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct Config {
    seed_demo: bool,
}

impl Config {
    fn from_environment() -> Self {
        let mut seed_demo = launch_seed_demo().unwrap_or(true);
        for argument in env::args().skip(1) {
            match argument.as_str() {
                "--demo" => seed_demo = true,
                "--empty" => seed_demo = false,
                _ => {}
            }
        }
        Self { seed_demo }
    }
}

#[cfg(feature = "trueos")]
fn launch_seed_demo() -> Option<bool> {
    let bytes = trueos::async_fs::block_on(trueos::async_fs::read_file(b"vFile:launch")).ok()?;
    let script = String::from_utf8(bytes).ok()?;
    let mut result = None;
    for line in script.lines().map(str::trim) {
        match line {
            "seed demo" => result = Some(true),
            "seed empty" => result = Some(false),
            _ => {}
        }
    }
    result
}

#[cfg(not(feature = "trueos"))]
fn launch_seed_demo() -> Option<bool> {
    None
}

struct TerminalGuard {
    active: bool,
}

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        if let Err(error) = execute!(
            stdout(),
            EnterAlternateScreen,
            DisableLineWrap,
            EnableMouseCapture,
            Hide
        ) {
            let _ = execute!(
                stdout(),
                ResetColor,
                Show,
                DisableMouseCapture,
                EnableLineWrap,
                LeaveAlternateScreen
            );
            let _ = terminal::disable_raw_mode();
            return Err(error);
        }
        Ok(Self { active: true })
    }

    fn restore(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        let screen_result = execute!(
            stdout(),
            ResetColor,
            Show,
            DisableMouseCapture,
            EnableLineWrap,
            LeaveAlternateScreen
        );
        let raw_result = terminal::disable_raw_mode();
        self.active = false;
        screen_result.and(raw_result)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}
