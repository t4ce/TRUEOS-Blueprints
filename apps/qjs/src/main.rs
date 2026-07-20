use std::{
    cmp,
    io::{self, BufWriter, Write, stdout},
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};

const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const OUTPUT_POLL_INTERVAL: Duration = Duration::from_millis(100);
const FRAME_BUFFER_CAPACITY: usize = 128 * 1024;
const TAB: &str = "  ";
const HISTORY_CAP: usize = 64;

const ACCENT: Color = Color::Rgb(255, 55, 255);
const CYAN: Color = Color::Rgb(96, 210, 255);
const GREEN: Color = Color::Rgb(60, 220, 140);
const RED: Color = Color::Rgb(255, 105, 120);
const MUTED: Color = Color::Rgb(130, 145, 165);
const PANEL: Color = Color::Rgb(45, 52, 65);

type AppTerminal = Terminal<CrosstermBackend<BufWriter<io::Stdout>>>;

fn main() -> Result<()> {
    let mut app = App::new();

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    loop {
        app.begin_terminal_session();
        let result = run_app(&mut app);
        trueos::vshell::leave_terminal_handoff();
        result?;
        trueos::vshell::wait_for_terminal_reentry();
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    {
        app.begin_terminal_session();
        run_app(&mut app)
    }
}

fn run_app(app: &mut App) -> Result<()> {
    let mut terminal = TerminalGuard::enter()?;
    let result = run(&mut terminal.terminal, app);
    terminal.exit()?;
    result
}

fn run(terminal: &mut AppTerminal, app: &mut App) -> Result<()> {
    let mut last_output_poll = Instant::now();
    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            let cursor = draw(frame.buffer_mut(), area, app);
            if let Some(position) = cursor {
                frame.set_cursor_position(position);
            }
        })?;

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
        enable_raw_mode()?;
        let mut output = BufWriter::with_capacity(FRAME_BUFFER_CAPACITY, stdout());
        execute!(output, EnterAlternateScreen, EnableMouseCapture)?;
        output.flush()?;
        let backend = CrosstermBackend::new(output);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;
        Ok(Self {
            terminal,
            active: true,
        })
    }

    fn exit(&mut self) -> Result<()> {
        if self.active {
            disable_raw_mode()?;
            execute!(
                self.terminal.backend_mut(),
                DisableMouseCapture,
                LeaveAlternateScreen
            )?;
            self.terminal.show_cursor()?;
            self.active = false;
        }
        Ok(())
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

        match bridge::eval(source.as_str(), self.eval_mode) {
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
        match bridge::poll() {
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
        bridge::close();
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

fn draw(buffer: &mut Buffer, area: Rect, app: &mut App) -> Option<Position> {
    if area.width < 20 || area.height < 8 {
        Paragraph::new("qjs needs a terminal of at least 20×8")
            .style(Style::default().fg(RED))
            .render(area, buffer);
        return None;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .split(area);
    draw_header(buffer, rows[0], app);

    let body = if rows[1].width >= 96 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(rows[1])
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(rows[1])
    };
    let cursor = draw_editor(buffer, body[0], app);
    draw_output(buffer, body[1], app);
    draw_footer(buffer, rows[2], app);
    if app.show_help {
        draw_help(buffer, area);
        None
    } else {
        Some(cursor)
    }
}

fn draw_header(buffer: &mut Buffer, area: Rect, app: &App) {
    let actual = app
        .last_actual_mode
        .map(|mode| mode.label().to_ascii_lowercase())
        .unwrap_or_else(|| "waiting".to_string());
    let lines = vec![
        Line::from(vec![
            Span::styled(
                " QuickJS scripting workbench ",
                Style::default()
                    .fg(Color::Black)
                    .bg(ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                "persistent VM",
                Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!(
                    "requested {} · last {actual}",
                    app.eval_mode.label().to_ascii_lowercase()
                ),
                Style::default().fg(CYAN),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Runtime ", Style::default().fg(MUTED)),
            Span::raw("shell profile · timers · workers · fetch   "),
            Span::styled(" Modules ", Style::default().fg(MUTED)),
            Span::raw("native import/export · TRUEOS/Node loader"),
        ]),
        Line::from(Span::styled(
            " F1 help · F2 mode · F5/Ctrl-Enter run · Ctrl-R reset · ESC/Ctrl-Q/F10 close TUI",
            Style::default().fg(MUTED),
        )),
    ];
    Paragraph::new(Text::from(lines)).render(area, buffer);
}

fn draw_editor(buffer: &mut Buffer, area: Rect, app: &mut App) -> Position {
    app.ensure_cursor_visible(area);
    let digits = app.lines.len().max(1).to_string().len();
    let visible_rows = area.height.saturating_sub(2) as usize;
    let mut lines = Vec::with_capacity(visible_rows);
    for row in app.row_offset..cmp::min(app.lines.len(), app.row_offset + visible_rows) {
        let number = format!("{:>digits$} ", row + 1);
        let text = app.lines[row]
            .chars()
            .skip(app.col_offset)
            .collect::<String>();
        let style = if row == app.cursor_row {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Gray)
        };
        lines.push(Line::from(vec![
            Span::styled(
                number,
                Style::default().fg(if row == app.cursor_row { ACCENT } else { MUTED }),
            ),
            Span::styled(text, style),
        ]));
    }
    let block = Block::default()
        .title(" Editor / REPL ")
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PANEL));
    Paragraph::new(Text::from(lines))
        .block(block)
        .render(area, buffer);
    Position::new(
        area.x
            .saturating_add(1)
            .saturating_add(digits as u16)
            .saturating_add(1)
            .saturating_add(app.cursor_col.saturating_sub(app.col_offset) as u16),
        area.y
            .saturating_add(1)
            .saturating_add(app.cursor_row.saturating_sub(app.row_offset) as u16),
    )
}

fn draw_output(buffer: &mut Buffer, area: Rect, app: &App) {
    let mut lines = Vec::new();
    for entry in &app.output {
        let (symbol, color) = match entry.kind {
            OutputKind::Source => ("›", CYAN),
            OutputKind::Result => ("⇒", GREEN),
            OutputKind::Error => ("×", RED),
            OutputKind::Print => ("·", Color::Yellow),
            OutputKind::System => ("◆", MUTED),
        };
        lines.push(Line::from(vec![Span::styled(
            format!("{symbol} {} ", entry.label),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )]));
        for text_line in entry.text.lines() {
            lines.push(Line::from(Span::styled(
                format!("  {text_line}"),
                Style::default().fg(if matches!(entry.kind, OutputKind::Error) {
                    RED
                } else {
                    Color::White
                }),
            )));
        }
    }
    let height = area.height.saturating_sub(2) as usize;
    let scroll = lines.len().saturating_sub(height) as u16;
    let block = Block::default()
        .title(format!(" Output · {} entries ", app.output.len()))
        .title_style(Style::default().fg(CYAN).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PANEL));
    Paragraph::new(Text::from(lines))
        .block(block)
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false })
        .render(area, buffer);
}

fn draw_footer(buffer: &mut Buffer, area: Rect, app: &App) {
    let text = vec![
        Line::from(vec![
            Span::styled(
                " STATUS ",
                Style::default()
                    .fg(Color::Black)
                    .bg(CYAN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(app.status.as_str(), Style::default().fg(Color::White)),
        ]),
        Line::from(Span::styled(
            " Ctrl-N new · Ctrl-↑/↓ history · Ctrl-L clear · commands :help :reset :clear :quit",
            Style::default().fg(MUTED),
        )),
    ];
    Paragraph::new(Text::from(text)).render(area, buffer);
}

fn draw_help(buffer: &mut Buffer, area: Rect) {
    let width = area.width.saturating_sub(8).min(84).max(20);
    let height = area.height.saturating_sub(4).min(21).max(8);
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    Clear.render(popup, buffer);
    let help = Text::from(vec![
        Line::from(Span::styled(
            "QuickJS workbench",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("F5 / Ctrl-Enter   evaluate the complete editor buffer"),
        Line::from("F2                cycle Auto / Script / Module"),
        Line::from("Ctrl-R            discard and recreate the persistent VM"),
        Line::from("Ctrl-N            clear the editor; Ctrl-↑/↓ recalls history"),
        Line::from("ESC / Ctrl-Q/F10  close TUI; the VM and JS state stay alive"),
        Line::from(""),
        Line::from(Span::styled(
            "Natural modules",
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        )),
        Line::from("import { readFile } from 'fs';"),
        Line::from("import * as events from 'node:events';"),
        Line::from("const module = await import('/path/module.mjs');  // Module mode"),
        Line::from(""),
        Line::from("Auto selects Module for static import/export; otherwise Script keeps"),
        Line::from("global declarations between evaluations. F2 overrides detection."),
        Line::from(""),
        Line::from(Span::styled(
            "Press F1 or ? to close help · ESC returns to the VMX shell",
            Style::default().fg(MUTED),
        )),
    ]);
    Paragraph::new(help)
        .block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ACCENT)),
        )
        .wrap(Wrap { trim: false })
        .render(popup, buffer);
}

mod bridge {
    use super::EvalMode;

    pub struct EvalResult {
        pub ok: bool,
        pub mode: EvalMode,
        pub eval_count: u64,
        pub text: String,
    }

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    pub fn eval(source: &str, mode: EvalMode) -> Result<EvalResult, String> {
        let native_mode = match mode {
            EvalMode::Auto => trueos::vshell::QjsWorkbenchMode::Auto,
            EvalMode::Script => trueos::vshell::QjsWorkbenchMode::Script,
            EvalMode::Module => trueos::vshell::QjsWorkbenchMode::Module,
        };
        let result = trueos::vshell::qjs_workbench_eval(source, native_mode)?;
        Ok(EvalResult {
            ok: result.ok,
            mode: match result.mode {
                trueos::vshell::QjsWorkbenchMode::Module => EvalMode::Module,
                _ => EvalMode::Script,
            },
            eval_count: result.eval_count,
            text: result.text,
        })
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    pub fn eval(_source: &str, mode: EvalMode) -> Result<EvalResult, String> {
        Err(format!(
            "QuickJS execution is available inside TRUEOS (requested {})",
            mode.label()
        ))
    }

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    pub fn poll() -> Result<String, String> {
        trueos::vshell::qjs_workbench_poll()
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    pub fn poll() -> Result<String, String> {
        Ok(String::new())
    }

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    pub fn close() {
        trueos::vshell::qjs_workbench_close();
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    pub fn close() {}
}
