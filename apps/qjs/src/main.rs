use std::{
    cmp,
    io::{self, BufWriter, Write, stdout},
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    execute, queue,
    style::{
        Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
    },
    terminal::{
        self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};

const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const OUTPUT_POLL_INTERVAL: Duration = Duration::from_millis(100);
const FRAME_BUFFER_CAPACITY: usize = 128 * 1024;
const TAB: &str = "  ";
const HISTORY_CAP: usize = 64;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
const CHILD_WORKER_ARGUMENT: &str = trueos_qjs::child_worker::ARGUMENT;

const BACKGROUND: Color = Color::Rgb { r: 8, g: 11, b: 18 };
const ACCENT: Color = Color::Rgb {
    r: 255,
    g: 55,
    b: 255,
};
const CYAN: Color = Color::Rgb {
    r: 96,
    g: 210,
    b: 255,
};
const GREEN: Color = Color::Rgb {
    r: 60,
    g: 220,
    b: 140,
};
const RED: Color = Color::Rgb {
    r: 255,
    g: 105,
    b: 120,
};
const MUTED: Color = Color::Rgb {
    r: 130,
    g: 145,
    b: 165,
};
const PANEL: Color = Color::Rgb {
    r: 45,
    g: 52,
    b: 65,
};

type AppTerminal = BufWriter<io::Stdout>;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Rect {
    x: u16,
    y: u16,
    width: u16,
    height: u16,
}

impl Rect {
    const fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    const fn inner(self) -> Self {
        Self::new(
            self.x.saturating_add(1),
            self.y.saturating_add(1),
            self.width.saturating_sub(2),
            self.height.saturating_sub(2),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Position {
    x: u16,
    y: u16,
}

impl Position {
    const fn new(x: u16, y: u16) -> Self {
        Self { x, y }
    }
}

fn main() -> Result<()> {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    if trueos::env::args().any(|arg| arg == CHILD_WORKER_ARGUMENT) {
        // A VMX child runs the same qjs.bp archive but must never acquire the
        // terminal lease. Its first parent->child frame is the Worker source.
        return trueos_qjs::child_worker::run().map_err(anyhow::Error::msg);
    }

    let mut app = App::new();

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        let mut lease = trueos::vshell::terminal_initial_lease()
            .map_err(|error| terminal_lease_error("initial terminal lease", error))?;
        loop {
            app.begin_terminal_session();
            let session = run_app(&mut app, || {
                lease
                    .acknowledge_ready()
                    .map_err(|error| terminal_lease_error("terminal-ready acknowledgement", error))
            });

            // `run_app` has already restored raw mode, mouse capture, and the
            // alternate screen. Only then may Shell2 take the terminal lease.
            let ticket = lease
                .release_to_shell()
                .map_err(|error| terminal_lease_error("terminal lease release", error))?;
            session?;
            lease = ticket
                .wait_for_reentry()
                .map_err(|error| terminal_lease_error("terminal lease reentry", error))?;
        }
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    {
        app.begin_terminal_session();
        run_app(&mut app, || Ok(()))
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn terminal_lease_error(action: &str, error: trueos::vshell::TerminalLeaseError) -> anyhow::Error {
    anyhow::anyhow!("{action}: {error}")
}

fn run_app<F>(app: &mut App, acknowledge_ready: F) -> Result<()>
where
    F: FnMut() -> Result<()>,
{
    let mut terminal = TerminalGuard::enter()?;
    let result = run(&mut terminal.terminal, app, acknowledge_ready);
    terminal.exit()?;
    result
}

fn run<F>(terminal: &mut AppTerminal, app: &mut App, mut acknowledge_ready: F) -> Result<()>
where
    F: FnMut() -> Result<()>,
{
    let mut last_output_poll = Instant::now();
    let mut terminal_ready = false;
    loop {
        let (width, height) = terminal::size()?;
        let cursor = draw(terminal, Rect::new(0, 0, width, height), app)?;
        match cursor {
            Some(position) => queue!(terminal, MoveTo(position.x, position.y), Show)?,
            None => queue!(terminal, Hide)?,
        }
        queue!(terminal, ResetColor, SetAttribute(Attribute::Reset))?;
        terminal.flush()?;
        if !terminal_ready {
            acknowledge_ready()?;
            terminal_ready = true;
        }

        if app.should_quit {
            return Ok(());
        }

        if event::poll(INPUT_POLL_INTERVAL)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => app.handle_key(key),
                Event::Paste(text) => app.insert_text(text.as_str()),
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        if last_output_poll.elapsed() >= OUTPUT_POLL_INTERVAL {
            app.poll_output();
            last_output_poll = Instant::now();
        }
    }
}

struct TerminalGuard {
    terminal: AppTerminal,
    active: bool,
}

impl TerminalGuard {
    fn enter() -> Result<Self> {
        let mut terminal = BufWriter::with_capacity(FRAME_BUFFER_CAPACITY, stdout());
        if let Err(error) = enable_raw_mode() {
            let _ = Self::restore_terminal(&mut terminal);
            return Err(error.into());
        }
        if let Err(error) = execute!(terminal, EnterAlternateScreen, EnableMouseCapture, Hide) {
            let _ = Self::restore_terminal(&mut terminal);
            return Err(error.into());
        }
        if let Err(error) = execute!(terminal, Clear(ClearType::All)) {
            let _ = Self::restore_terminal(&mut terminal);
            return Err(error.into());
        }
        Ok(Self {
            terminal,
            active: true,
        })
    }

    fn exit(&mut self) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        // Make Drop idempotent even if one cleanup operation fails. The helper
        // still attempts every terminal and raw-mode restoration step.
        self.active = false;
        Self::restore_terminal(&mut self.terminal).map_err(Into::into)
    }

    fn restore_terminal(terminal: &mut AppTerminal) -> io::Result<()> {
        let mut first_error = None;
        if let Err(error) = execute!(
            terminal,
            DisableMouseCapture,
            Show,
            ResetColor,
            LeaveAlternateScreen
        ) {
            first_error = Some(error);
        }
        if let Err(error) = terminal.flush() {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
        if let Err(error) = disable_raw_mode() {
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.exit();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EvalMode {
    Auto,
    Script,
    Module,
}

impl EvalMode {
    fn next(self) -> Self {
        match self {
            Self::Auto => Self::Script,
            Self::Script => Self::Module,
            Self::Module => Self::Auto,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Auto => "AUTO",
            Self::Script => "SCRIPT",
            Self::Module => "MODULE",
        }
    }
}

enum OutputKind {
    Source,
    Result,
    Error,
    Print,
    System,
}

struct OutputEntry {
    kind: OutputKind,
    label: String,
    text: String,
}

struct App {
    /// Persistent VM ownership belongs to this Blueprint process, not Shell2.
    vm: trueos_qjs::workbench::Workbench,
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
    row_offset: usize,
    col_offset: usize,
    output: Vec<OutputEntry>,
    history: Vec<String>,
    history_index: Option<usize>,
    eval_mode: EvalMode,
    last_actual_mode: Option<EvalMode>,
    status: String,
    show_help: bool,
    should_quit: bool,
}

impl App {
    fn new() -> Self {
        Self {
            vm: trueos_qjs::workbench::Workbench::new(),
            lines: vec!["1 + 1".to_string()],
            cursor_row: 0,
            cursor_col: 5,
            row_offset: 0,
            col_offset: 0,
            output: vec![OutputEntry {
                kind: OutputKind::System,
                label: "READY".to_string(),
                text: "Persistent QuickJS VM ready. Native import/export syntax uses the TRUEOS/Node loader."
                    .to_string(),
            }],
            history: Vec::new(),
            history_index: None,
            eval_mode: EvalMode::Auto,
            last_actual_mode: None,
            status: "Ctrl-Enter or F5 evaluates the editor".to_string(),
            show_help: false,
            should_quit: false,
        }
    }

    fn begin_terminal_session(&mut self) {
        self.show_help = false;
        self.should_quit = false;
        self.status = "QuickJS VM attached · ESC or :quit returns to the VMX shell".to_string();
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if self.show_help {
            match key.code {
                KeyCode::Esc => self.should_quit = true,
                KeyCode::F(1) | KeyCode::Char('?') => self.show_help = false,
                _ => {}
            }
            return;
        }

        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc | KeyCode::F(10) if !control => self.should_quit = true,
            KeyCode::Char('q') if control => self.should_quit = true,
            KeyCode::F(1) | KeyCode::Char('?') if control => self.show_help = true,
            KeyCode::F(2) => {
                self.eval_mode = self.eval_mode.next();
                self.status = format!("Evaluation mode: {}", self.eval_mode.label());
            }
            KeyCode::F(5) => self.evaluate(),
            KeyCode::Enter if control => self.evaluate(),
            KeyCode::Char('l') if control => self.clear_output(),
            KeyCode::Char('r') if control => self.reset_vm(),
            KeyCode::Char('n') if control => self.new_buffer(),
            KeyCode::Up if control => self.recall_history(true),
            KeyCode::Down if control => self.recall_history(false),
            KeyCode::Char('a') if control => self.cursor_col = 0,
            KeyCode::Char('e') if control => self.cursor_col = self.current_line_chars(),
            KeyCode::Up => self.move_vertical(-1),
            KeyCode::Down => self.move_vertical(1),
            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::PageUp => self.move_vertical(-12),
            KeyCode::PageDown => self.move_vertical(12),
            KeyCode::Home => self.cursor_col = 0,
            KeyCode::End => self.cursor_col = self.current_line_chars(),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Enter => self.insert_newline(),
            KeyCode::Tab => self.insert_text(TAB),
            KeyCode::Char(ch) if !control && !key.modifiers.contains(KeyModifiers::ALT) => {
                self.insert_char(ch)
            }
            _ => {}
        }
    }

    fn source(&self) -> String {
        self.lines.join("\n")
    }

    fn evaluate(&mut self) {
        let source = self.source();
        let trimmed = source.trim();
        if trimmed.is_empty() {
            self.status = "Nothing to evaluate".to_string();
            return;
        }
        if self.handle_command(trimmed) {
            return;
        }

        if self.history.last().is_none_or(|last| last != &source) {
            if self.history.len() == HISTORY_CAP {
                self.history.remove(0);
            }
            self.history.push(source.clone());
        }
        self.history_index = None;
        self.output.push(OutputEntry {
            kind: OutputKind::Source,
            label: "SOURCE".to_string(),
            text: source.clone(),
        });

        match bridge::eval(&mut self.vm, source.as_str(), self.eval_mode) {
            Ok(result) => {
                self.last_actual_mode = Some(result.mode);
                let label = format!(
                    "#{:04} {}",
                    result.eval_count,
                    result.mode.label().to_ascii_lowercase()
                );
                self.output.push(OutputEntry {
                    kind: if result.ok {
                        OutputKind::Result
                    } else {
                        OutputKind::Error
                    },
                    label,
                    text: result.text,
                });
                self.status = if result.ok {
                    "Evaluation complete".to_string()
                } else {
                    "QuickJS exception".to_string()
                };
            }
            Err(error) => {
                self.output.push(OutputEntry {
                    kind: OutputKind::Error,
                    label: "BRIDGE".to_string(),
                    text: error,
                });
                self.status = "Evaluation bridge failed".to_string();
            }
        }
        self.trim_output();
        self.poll_output();
    }

    fn handle_command(&mut self, command: &str) -> bool {
        match command {
            ":quit" | ".quit" | ":q" | "quit" | "Quit" => self.should_quit = true,
            ":help" | ".help" => self.show_help = true,
            ":reset" | ".reset" => self.reset_vm(),
            ":clear" | ".clear" => self.clear_output(),
            ":mode auto" => self.set_mode(EvalMode::Auto),
            ":mode script" => self.set_mode(EvalMode::Script),
            ":mode module" => self.set_mode(EvalMode::Module),
            _ if command.starts_with(':') || command.starts_with('.') => {
                self.status = "Unknown command; open F1 help".to_string();
            }
            _ => return false,
        }
        true
    }

    fn set_mode(&mut self, mode: EvalMode) {
        self.eval_mode = mode;
        self.status = format!("Evaluation mode: {}", mode.label());
    }

    fn poll_output(&mut self) {
        match bridge::poll(&mut self.vm) {
            Ok(output) if !output.is_empty() => {
                for line in output.lines() {
                    self.output.push(OutputEntry {
                        kind: OutputKind::Print,
                        label: "PRINT".to_string(),
                        text: line.to_string(),
                    });
                }
                self.trim_output();
            }
            Ok(_) => {}
            Err(error) => self.status = error,
        }
    }

    fn reset_vm(&mut self) {
        bridge::close(&mut self.vm);
        self.last_actual_mode = None;
        self.output.push(OutputEntry {
            kind: OutputKind::System,
            label: "RESET".to_string(),
            text: "VM, globals, timers, workers, and pending jobs discarded".to_string(),
        });
        self.status = "QuickJS VM reset".to_string();
        self.trim_output();
    }

    fn clear_output(&mut self) {
        self.output.clear();
        self.status = "Output cleared".to_string();
    }

    fn new_buffer(&mut self) {
        self.lines.clear();
        self.lines.push(String::new());
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.row_offset = 0;
        self.col_offset = 0;
        self.history_index = None;
        self.status = "New editor buffer".to_string();
    }

    fn recall_history(&mut self, older: bool) {
        if self.history.is_empty() {
            self.status = "History is empty".to_string();
            return;
        }
        let index = match (self.history_index, older) {
            (None, true) => self.history.len() - 1,
            (Some(index), true) => index.saturating_sub(1),
            (Some(index), false) if index + 1 < self.history.len() => index + 1,
            (_, false) => {
                self.new_buffer();
                return;
            }
        };
        self.history_index = Some(index);
        self.lines = split_lines(self.history[index].as_str());
        self.cursor_row = self.lines.len().saturating_sub(1);
        self.cursor_col = self.current_line_chars();
        self.status = format!("History {}/{}", index + 1, self.history.len());
    }

    fn trim_output(&mut self) {
        const CAP: usize = 256;
        if self.output.len() > CAP {
            self.output.drain(..self.output.len() - CAP);
        }
    }

    fn current_line_chars(&self) -> usize {
        self.lines[self.cursor_row].chars().count()
    }

    fn move_vertical(&mut self, delta: isize) {
        self.cursor_row = self
            .cursor_row
            .saturating_add_signed(delta)
            .min(self.lines.len().saturating_sub(1));
        self.cursor_col = self.cursor_col.min(self.current_line_chars());
    }

    fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.current_line_chars();
        }
    }

    fn move_right(&mut self) {
        if self.cursor_col < self.current_line_chars() {
            self.cursor_col += 1;
        } else if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
    }

    fn insert_char(&mut self, ch: char) {
        let byte = char_to_byte(&self.lines[self.cursor_row], self.cursor_col);
        self.lines[self.cursor_row].insert(byte, ch);
        self.cursor_col += 1;
        self.history_index = None;
    }

    fn insert_text(&mut self, text: &str) {
        for ch in text.chars() {
            if ch == '\n' {
                self.insert_newline();
            } else if ch != '\r' {
                self.insert_char(ch);
            }
        }
    }

    fn insert_newline(&mut self) {
        let byte = char_to_byte(&self.lines[self.cursor_row], self.cursor_col);
        let tail = self.lines[self.cursor_row].split_off(byte);
        self.cursor_row += 1;
        self.lines.insert(self.cursor_row, tail);
        self.cursor_col = 0;
        self.history_index = None;
    }

    fn backspace(&mut self) {
        if self.cursor_col > 0 {
            let end = char_to_byte(&self.lines[self.cursor_row], self.cursor_col);
            let start = char_to_byte(&self.lines[self.cursor_row], self.cursor_col - 1);
            self.lines[self.cursor_row].replace_range(start..end, "");
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            let tail = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            self.cursor_col = self.current_line_chars();
            self.lines[self.cursor_row].push_str(tail.as_str());
        }
        self.history_index = None;
    }

    fn delete(&mut self) {
        if self.cursor_col < self.current_line_chars() {
            let start = char_to_byte(&self.lines[self.cursor_row], self.cursor_col);
            let end = char_to_byte(&self.lines[self.cursor_row], self.cursor_col + 1);
            self.lines[self.cursor_row].replace_range(start..end, "");
        } else if self.cursor_row + 1 < self.lines.len() {
            let next = self.lines.remove(self.cursor_row + 1);
            self.lines[self.cursor_row].push_str(next.as_str());
        }
        self.history_index = None;
    }

    fn ensure_cursor_visible(&mut self, area: Rect) {
        let height = area.height.saturating_sub(2) as usize;
        let gutter = self.lines.len().max(1).to_string().len() + 2;
        let width = area.width.saturating_sub(2) as usize;
        let text_width = width.saturating_sub(gutter).max(1);
        if self.cursor_row < self.row_offset {
            self.row_offset = self.cursor_row;
        } else if self.cursor_row >= self.row_offset.saturating_add(height.max(1)) {
            self.row_offset = self.cursor_row + 1 - height.max(1);
        }
        if self.cursor_col < self.col_offset {
            self.col_offset = self.cursor_col;
        } else if self.cursor_col >= self.col_offset.saturating_add(text_width) {
            self.col_offset = self.cursor_col + 1 - text_width;
        }
    }
}

fn char_to_byte(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

fn split_lines(text: &str) -> Vec<String> {
    let mut lines = text.split('\n').map(str::to_string).collect::<Vec<_>>();
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn draw(out: &mut impl Write, area: Rect, app: &mut App) -> io::Result<Option<Position>> {
    queue!(
        out,
        SetBackgroundColor(BACKGROUND),
        Clear(ClearType::All),
        Hide
    )?;
    if area.width < 20 || area.height < 8 {
        write_at(
            out,
            area.x,
            area.y,
            area.width,
            "qjs needs a terminal of at least 20×8",
            RED,
            BACKGROUND,
            false,
        )?;
        return Ok(None);
    }

    let header = Rect::new(area.x, area.y, area.width, 3);
    let footer = Rect::new(area.x, area.y + area.height - 2, area.width, 2);
    let body = Rect::new(area.x, area.y + 3, area.width, area.height - 5);
    draw_header(out, header, app)?;

    let (editor, output) = if body.width >= 96 {
        let editor_width = body.width.saturating_mul(58) / 100;
        (
            Rect::new(body.x, body.y, editor_width, body.height),
            Rect::new(
                body.x + editor_width,
                body.y,
                body.width - editor_width,
                body.height,
            ),
        )
    } else {
        let editor_height = body.height.saturating_mul(55) / 100;
        (
            Rect::new(body.x, body.y, body.width, editor_height),
            Rect::new(
                body.x,
                body.y + editor_height,
                body.width,
                body.height - editor_height,
            ),
        )
    };
    let cursor = draw_editor(out, editor, app)?;
    draw_output(out, output, app)?;
    draw_footer(out, footer, app)?;
    if app.show_help {
        draw_help(out, area)?;
        Ok(None)
    } else {
        Ok(Some(cursor))
    }
}

fn draw_header(out: &mut impl Write, area: Rect, app: &App) -> io::Result<()> {
    let actual = app
        .last_actual_mode
        .map(|mode| mode.label().to_ascii_lowercase())
        .unwrap_or_else(|| "waiting".to_string());
    fill_rect(out, area, BACKGROUND)?;
    write_at(
        out,
        area.x,
        area.y,
        area.width,
        " QuickJS scripting workbench ",
        Color::Black,
        ACCENT,
        true,
    )?;
    write_at(
        out,
        area.x + 29,
        area.y,
        area.width.saturating_sub(29),
        &format!(
            "persistent VM  requested {} · last {actual}",
            app.eval_mode.label().to_ascii_lowercase()
        ),
        GREEN,
        BACKGROUND,
        true,
    )?;
    write_at(
        out,
        area.x,
        area.y + 1,
        area.width,
        " Runtime  shell profile · timers · workers · fetch    Modules  native import/export · TRUEOS/Node loader",
        MUTED,
        BACKGROUND,
        false,
    )?;
    write_at(
        out,
        area.x,
        area.y + 2,
        area.width,
        " F1 help · F2 mode · F5/Ctrl-Enter run · Ctrl-R reset · ESC/Ctrl-Q/F10 close TUI",
        MUTED,
        BACKGROUND,
        false,
    )
}

fn draw_editor(out: &mut impl Write, area: Rect, app: &mut App) -> io::Result<Position> {
    app.ensure_cursor_visible(area);
    draw_panel(out, area, " Editor / REPL ", ACCENT)?;
    let inner = area.inner();
    let digits = app.lines.len().max(1).to_string().len();
    let visible_rows = inner.height as usize;
    for (screen_row, row) in
        (app.row_offset..cmp::min(app.lines.len(), app.row_offset + visible_rows)).enumerate()
    {
        let number = format!("{:>digits$} ", row + 1);
        let number_color = if row == app.cursor_row { ACCENT } else { MUTED };
        write_at(
            out,
            inner.x,
            inner.y + screen_row as u16,
            inner.width,
            &number,
            number_color,
            BACKGROUND,
            false,
        )?;
        let text: String = app.lines[row].chars().skip(app.col_offset).collect();
        write_at(
            out,
            inner.x + digits as u16 + 1,
            inner.y + screen_row as u16,
            inner.width.saturating_sub(digits as u16 + 1),
            &text,
            if row == app.cursor_row {
                Color::White
            } else {
                Color::Grey
            },
            BACKGROUND,
            false,
        )?;
    }
    Ok(Position::new(
        inner
            .x
            .saturating_add(digits as u16)
            .saturating_add(1)
            .saturating_add(app.cursor_col.saturating_sub(app.col_offset) as u16),
        inner
            .y
            .saturating_add(app.cursor_row.saturating_sub(app.row_offset) as u16),
    ))
}

fn draw_output(out: &mut impl Write, area: Rect, app: &App) -> io::Result<()> {
    draw_panel(
        out,
        area,
        &format!(" Output · {} entries ", app.output.len()),
        CYAN,
    )?;
    let inner = area.inner();
    let mut lines: Vec<(String, Color, bool)> = Vec::new();
    for entry in &app.output {
        let (symbol, color) = match entry.kind {
            OutputKind::Source => ("›", CYAN),
            OutputKind::Result => ("⇒", GREEN),
            OutputKind::Error => ("×", RED),
            OutputKind::Print => ("·", Color::Yellow),
            OutputKind::System => ("◆", MUTED),
        };
        lines.push((format!("{symbol} {} ", entry.label), color, true));
        let text_color = if matches!(entry.kind, OutputKind::Error) {
            RED
        } else {
            Color::White
        };
        for text_line in entry.text.lines() {
            push_wrapped_lines(
                &mut lines,
                &format!("  {text_line}"),
                inner.width as usize,
                text_color,
            );
        }
    }
    let start = lines.len().saturating_sub(inner.height as usize);
    for (row, (text, color, bold)) in lines[start..].iter().enumerate() {
        write_at(
            out,
            inner.x,
            inner.y + row as u16,
            inner.width,
            text,
            *color,
            BACKGROUND,
            *bold,
        )?;
    }
    Ok(())
}

fn draw_footer(out: &mut impl Write, area: Rect, app: &App) -> io::Result<()> {
    fill_rect(out, area, BACKGROUND)?;
    write_at(
        out,
        area.x,
        area.y,
        area.width.min(8),
        " STATUS ",
        Color::Black,
        CYAN,
        true,
    )?;
    write_at(
        out,
        area.x + 9,
        area.y,
        area.width.saturating_sub(9),
        &app.status,
        Color::White,
        BACKGROUND,
        false,
    )?;
    write_at(
        out,
        area.x,
        area.y + 1,
        area.width,
        " Ctrl-N new · Ctrl-↑/↓ history · Ctrl-L clear · commands :help :reset :clear :quit",
        MUTED,
        BACKGROUND,
        false,
    )
}

fn draw_help(out: &mut impl Write, area: Rect) -> io::Result<()> {
    let width = area.width.saturating_sub(8).min(84).max(20).min(area.width);
    let height = area
        .height
        .saturating_sub(4)
        .min(21)
        .max(8)
        .min(area.height);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    draw_panel(out, popup, " Help ", ACCENT)?;
    let help = [
        "QuickJS workbench",
        "",
        "F5 / Ctrl-Enter   evaluate the complete editor buffer",
        "F2                cycle Auto / Script / Module",
        "Ctrl-R            discard and recreate the persistent VM",
        "Ctrl-N            clear the editor; Ctrl-↑/↓ recalls history",
        "ESC / Ctrl-Q/F10  close TUI; the VM and JS state stay alive",
        "",
        "Natural modules",
        "import { readFile } from 'fs';",
        "import * as events from 'node:events';",
        "const module = await import('/path/module.mjs');  // Module mode",
        "",
        "Auto selects Module for static import/export; otherwise Script keeps",
        "global declarations between evaluations. F2 overrides detection.",
        "",
        "Press F1 or ? to close help · ESC returns to the VMX shell",
    ];
    let inner = popup.inner();
    for (row, line) in help.iter().take(inner.height as usize).enumerate() {
        let color = match *line {
            "QuickJS workbench" => ACCENT,
            "Natural modules" => CYAN,
            line if line.starts_with("Press F1") => MUTED,
            _ => Color::White,
        };
        write_at(
            out,
            inner.x,
            inner.y + row as u16,
            inner.width,
            line,
            color,
            BACKGROUND,
            matches!(*line, "QuickJS workbench" | "Natural modules"),
        )?;
    }
    Ok(())
}

fn draw_panel(out: &mut impl Write, area: Rect, title: &str, accent: Color) -> io::Result<()> {
    draw_box(out, area, PANEL, BACKGROUND)?;
    if area.width > 4 && area.height > 0 {
        write_at(
            out,
            area.x + 2,
            area.y,
            area.width.saturating_sub(4),
            title,
            accent,
            BACKGROUND,
            true,
        )?;
    }
    Ok(())
}

fn draw_box(out: &mut impl Write, area: Rect, border: Color, background: Color) -> io::Result<()> {
    if area.width == 0 || area.height == 0 {
        return Ok(());
    }
    fill_rect(out, area, background)?;
    if area.width < 2 || area.height < 2 {
        return Ok(());
    }
    let horizontal = "─".repeat(area.width.saturating_sub(2) as usize);
    queue!(
        out,
        SetForegroundColor(border),
        SetBackgroundColor(background),
        MoveTo(area.x, area.y),
        Print("┌"),
        Print(&horizontal),
        Print("┐"),
        MoveTo(area.x, area.y + area.height - 1),
        Print("└"),
        Print(&horizontal),
        Print("┘")
    )?;
    for row in area.y + 1..area.y + area.height - 1 {
        queue!(
            out,
            MoveTo(area.x, row),
            Print("│"),
            MoveTo(area.x + area.width - 1, row),
            Print("│")
        )?;
    }
    Ok(())
}

fn fill_rect(out: &mut impl Write, area: Rect, background: Color) -> io::Result<()> {
    if area.width == 0 || area.height == 0 {
        return Ok(());
    }
    let blank = " ".repeat(area.width as usize);
    queue!(out, SetBackgroundColor(background))?;
    for row in area.y..area.y.saturating_add(area.height) {
        queue!(out, MoveTo(area.x, row), Print(&blank))?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_at(
    out: &mut impl Write,
    x: u16,
    y: u16,
    width: u16,
    text: &str,
    foreground: Color,
    background: Color,
    bold: bool,
) -> io::Result<()> {
    if width == 0 {
        return Ok(());
    }
    let clipped: String = text.chars().take(width as usize).collect();
    queue!(
        out,
        SetForegroundColor(foreground),
        SetBackgroundColor(background),
        SetAttribute(if bold {
            Attribute::Bold
        } else {
            Attribute::NormalIntensity
        }),
        MoveTo(x, y),
        Print(clipped),
        SetAttribute(Attribute::NormalIntensity)
    )
}

fn push_wrapped_lines(
    lines: &mut Vec<(String, Color, bool)>,
    text: &str,
    width: usize,
    color: Color,
) {
    if width == 0 {
        return;
    }
    let mut remaining = text;
    while !remaining.is_empty() {
        let take = remaining.chars().count().min(width);
        let byte = remaining
            .char_indices()
            .nth(take)
            .map(|(index, _)| index)
            .unwrap_or(remaining.len());
        lines.push((remaining[..byte].to_string(), color, false));
        remaining = &remaining[byte..];
    }
    if text.is_empty() {
        lines.push((String::new(), color, false));
    }
}

mod bridge {
    use super::EvalMode;
    use trueos_qjs::workbench::{EvalMode as RuntimeEvalMode, Workbench};

    pub struct EvalResult {
        pub ok: bool,
        pub mode: EvalMode,
        pub eval_count: u64,
        pub text: String,
    }

    pub fn eval(vm: &mut Workbench, source: &str, mode: EvalMode) -> Result<EvalResult, String> {
        let result = vm.eval(source, runtime_mode(mode))?;
        Ok(EvalResult {
            ok: result.ok,
            mode: app_mode(result.mode),
            eval_count: result.eval_count,
            text: result.text,
        })
    }

    pub fn poll(vm: &mut Workbench) -> Result<String, String> {
        Ok(vm.poll())
    }

    pub fn close(vm: &mut Workbench) {
        vm.close();
    }

    fn runtime_mode(mode: EvalMode) -> RuntimeEvalMode {
        match mode {
            EvalMode::Auto => RuntimeEvalMode::Auto,
            EvalMode::Script => RuntimeEvalMode::Script,
            EvalMode::Module => RuntimeEvalMode::Module,
        }
    }

    fn app_mode(mode: RuntimeEvalMode) -> EvalMode {
        match mode {
            RuntimeEvalMode::Auto | RuntimeEvalMode::Script => EvalMode::Script,
            RuntimeEvalMode::Module => EvalMode::Module,
        }
    }
}
