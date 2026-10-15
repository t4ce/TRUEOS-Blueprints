mod ansi;
mod arena;
mod codex;
#[cfg(test)]
mod compat_tests;
mod gallery;
mod stress;
mod ui;

use crossterm::{
    cursor::{Hide, SetCursorStyle, Show},
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind,
        KeyModifiers,
    },
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{Terminal, TerminalOptions, Viewport, backend::CrosstermBackend};
use std::{
    cell::Cell,
    io::{self, Write},
    rc::Rc,
    time::{Duration, Instant},
};

struct Counted<W> {
    inner: W,
    bytes: Rc<Cell<u64>>,
}
impl<W: Write> Write for Counted<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let count = self.inner.write(bytes)?;
        self.bytes
            .set(self.bytes.get().saturating_add(count as u64));
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
type LiveTerminal = Terminal<CrosstermBackend<Counted<io::Stdout>>>;

fn draw_measured(
    terminal: &mut LiveTerminal,
    app: &mut ui::App,
    bytes: &Cell<u64>,
    epoch: Instant,
) -> io::Result<()> {
    let before = bytes.get();
    let started = Instant::now();
    terminal.draw(|frame| ui::render(frame, app))?;
    let draw_micros = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
    app.metrics.frames = app.metrics.frames.saturating_add(1);
    app.metrics.bytes = bytes.get();
    app.metrics.last_frame_bytes = bytes.get().saturating_sub(before);
    app.metrics.draw_micros = draw_micros;
    app.metrics.peak_draw_micros = app.metrics.peak_draw_micros.max(draw_micros);
    app.metrics.elapsed_ms = epoch.elapsed().as_millis().min(u64::MAX as u128) as u64;
    if draw_micros > app.tick_millis() * 1000 {
        app.metrics.slow_frames += 1;
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ratatest: {error}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help") {
        println!(
            "ratatest [--stress] [--load 1|2|3] [--case ID] [--tour] [--inline]\n         --self-check | --stress-check [frames-per-size]\n\n1..5 sections  n/p cases  s stress tour  +/- load  Space freeze\nLoad 1/2/3 targets 10/20/40 fps in Arena; q quits."
        );
        return Ok(());
    }
    if let Some(index) = args.iter().position(|a| a == "--stress-check") {
        let frames = args
            .get(index + 1)
            .map(|a| a.parse::<usize>())
            .transpose()
            .map_err(|_| io::Error::other("--stress-check expects a frame count"))?
            .unwrap_or(600);
        return stress::run(frames);
    }
    if args.iter().any(|a| a == "--self-check") {
        return ui::self_check();
    }
    let inline = args.iter().any(|a| a == "--inline");
    let mut app = ui::App::new();
    if args.iter().any(|a| a == "--stress") {
        app.start_stress();
    }
    app.tour = args.iter().any(|a| a == "--tour");
    if let Some(index) = args.iter().position(|a| a == "--load") {
        app.load = args
            .get(index + 1)
            .and_then(|a| a.parse::<u8>().ok())
            .filter(|n| (1..=3).contains(n))
            .ok_or_else(|| io::Error::other("--load expects 1, 2 or 3"))?;
    }
    if let Some(index) = args.iter().position(|a| a == "--case") {
        let id = args
            .get(index + 1)
            .ok_or_else(|| io::Error::other("--case requires a case id"))?;
        app.case = ui::cases()
            .iter()
            .position(|c| c.id == *id)
            .ok_or_else(|| {
                io::Error::other(format!("unknown case {id}; use --self-check for case ids"))
            })?;
        app.home = false;
    }
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    let mut lease = trueos::vshell::terminal_initial_lease()
        .map_err(|error| io::Error::other(error.to_string()))?;
    let bytes = Rc::new(Cell::new(0));
    let epoch = Instant::now();
    loop {
        let result = (|| -> io::Result<bool> {
            let _guard = ScreenGuard::enter(inline)?;
            let viewport = if inline {
                Viewport::Inline(18)
            } else {
                Viewport::Fullscreen
            };
            let mut terminal = Terminal::with_options(
                CrosstermBackend::new(Counted {
                    inner: io::stdout(),
                    bytes: bytes.clone(),
                }),
                TerminalOptions { viewport },
            )?;
            terminal.clear()?;
            draw_measured(&mut terminal, &mut app, &bytes, epoch)?;
            #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
            lease
                .acknowledge_ready()
                .map_err(|error| io::Error::other(error.to_string()))?;
            interact(&mut terminal, &mut app, &bytes, epoch)
        })();
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        let ticket = lease
            .release_to_shell()
            .map_err(|error| io::Error::other(error.to_string()))?;
        let park = result?;
        #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
        if park {
            lease = ticket
                .wait_for_reentry()
                .map_err(|error| io::Error::other(error.to_string()))?;
            continue;
        }
        #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
        let _ = park;
        return Ok(());
    }
}

fn interact(
    terminal: &mut LiveTerminal,
    app: &mut ui::App,
    bytes: &Cell<u64>,
    epoch: Instant,
) -> io::Result<bool> {
    let mut last_tick = Instant::now();
    let mut last_tour = Instant::now();
    let mut dirty = true;
    loop {
        if dirty {
            draw_measured(terminal, app, bytes, epoch)?;
            dirty = false;
        }
        let cadence = Duration::from_millis(app.tick_millis());
        let wait = if app.paused {
            Duration::from_millis(80)
        } else {
            cadence
                .saturating_sub(last_tick.elapsed())
                .min(Duration::from_millis(80))
        };
        if event::poll(wait)? {
            let event = event::read()?;
            app.record_event(&event);
            dirty = true;
            match event {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('q'))
                    {
                        return Ok(false);
                    }
                    if key.code == KeyCode::F(10) {
                        return Ok(true);
                    }
                    if app.editing {
                        match key.code {
                            KeyCode::Esc => app.editing = false,
                            KeyCode::Enter => {
                                if app.submissions.len() == 64 {
                                    app.submissions.remove(0);
                                }
                                app.submissions.push(std::mem::take(&mut app.input));
                                app.editing = false;
                            }
                            KeyCode::Backspace => {
                                app.input.pop();
                            }
                            KeyCode::Char(ch) => {
                                if app.input.len() < 4096 {
                                    app.input.push(ch);
                                }
                            }
                            _ => {}
                        }
                    } else {
                        match key.code {
                            KeyCode::Char('q') => return Ok(false),
                            KeyCode::Esc | KeyCode::Char('h') => app.home = !app.home,
                            KeyCode::Tab | KeyCode::Right | KeyCode::Char('n') => {
                                app.stress_tour = false;
                                app.next(1);
                            }
                            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('p') => {
                                app.stress_tour = false;
                                app.next(-1);
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                if app.home {
                                    app.next(1);
                                } else {
                                    app.selected = app.selected.wrapping_add(1);
                                }
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                if app.home {
                                    app.next(-1);
                                } else {
                                    app.selected = app.selected.saturating_sub(1);
                                }
                            }
                            KeyCode::PageDown => app.scroll = app.scroll.saturating_add(5),
                            KeyCode::PageUp => app.scroll = app.scroll.saturating_sub(5),
                            KeyCode::Enter => app.home = false,
                            KeyCode::Char(' ') => app.paused = !app.paused,
                            KeyCode::Char('t') => {
                                app.stress_tour = false;
                                app.tour = !app.tour;
                                app.home = false;
                                last_tour = Instant::now();
                            }
                            KeyCode::Char('s') => {
                                if app.stress_tour {
                                    app.stress_tour = false;
                                } else {
                                    app.start_stress();
                                }
                                last_tour = Instant::now();
                                last_tick = Instant::now();
                            }
                            KeyCode::Char('+') | KeyCode::Char('=') => {
                                app.load = (app.load + 1).min(3)
                            }
                            KeyCode::Char('-') => app.load = app.load.saturating_sub(1).max(1),
                            KeyCode::Char('o') => app.popup = !app.popup,
                            KeyCode::Char('i') => {
                                app.editing = true;
                                app.home = false;
                            }
                            KeyCode::Char('r') => {
                                app.tick = 0;
                                app.scroll = 0;
                                terminal.clear()?;
                            }
                            KeyCode::Char('a') if !app.home => {
                                if let ui::Kind::Ansi(index) = ui::cases()[app.case].kind {
                                    ansi::run(terminal.backend_mut(), index)?;
                                    app.probe_runs += 1;
                                    terminal.clear()?;
                                }
                            }
                            KeyCode::Char(ch @ '1'..='5') => {
                                app.select_group(ch as usize - '1' as usize)
                            }
                            _ => {}
                        }
                    }
                }
                Event::Paste(text) => {
                    app.input.extend(
                        text.chars()
                            .take(4096 - app.input.chars().count().min(4096)),
                    );
                }
                Event::Resize(_, _) => {
                    terminal.clear()?;
                }
                _ => {}
            }
        }
        if last_tick.elapsed() >= cadence {
            if !app.paused {
                app.tick = app.tick.wrapping_add(1);
                dirty = true;
            }
            last_tick = Instant::now();
        }
        if app.paused || app.editing {
            last_tour = Instant::now();
        }
        if app.stress_tour
            && !app.paused
            && !app.editing
            && last_tour.elapsed() >= Duration::from_secs(6)
        {
            app.advance_stress();
            last_tour = Instant::now();
            dirty = true;
        } else if app.tour
            && !app.paused
            && !app.editing
            && last_tour.elapsed() >= Duration::from_secs(3)
        {
            app.home = false;
            app.next(1);
            last_tour = Instant::now();
            dirty = true;
        }
    }
}

/// Restores the modes enabled by this app even when setup or rendering fails.
struct ScreenGuard {
    inline: bool,
}
impl ScreenGuard {
    fn enter(inline: bool) -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let guard = Self { inline };
        if !inline {
            execute!(io::stdout(), EnterAlternateScreen)?;
        }
        execute!(io::stdout(), EnableBracketedPaste, Hide)?;
        Ok(guard)
    }
}
impl Drop for ScreenGuard {
    fn drop(&mut self) {
        let mut out = io::stdout();
        let _ = write!(out, "{}", ansi::RESTORE);
        let _ = execute!(
            out,
            DisableBracketedPaste,
            SetCursorStyle::DefaultUserShape,
            Show
        );
        if !self.inline {
            let _ = execute!(out, LeaveAlternateScreen);
        }
        let _ = out.flush();
        let _ = terminal::disable_raw_mode();
    }
}
