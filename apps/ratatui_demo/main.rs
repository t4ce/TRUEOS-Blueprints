use core::fmt::Write;
use ratatui_core::{
    backend::{Backend, ClearType, WindowSize},
    buffer::{Buffer, Cell},
    layout::{Constraint, Layout, Position, Rect, Size},
    style::{Color, Modifier, Style},
    terminal::{Frame, Terminal},
    text::{Line, Span},
    widgets::Widget,
};
use ratatui_widgets::{block::Block, borders::Borders, gauge::Gauge, paragraph::Paragraph};
use trueos::{
    logl::{self, level},
    vshell,
};

const WIDTH: u16 = 48;
const HEIGHT: u16 = 12;
const RESERVED_TOP_ROWS: u32 = 2;

fn main() {
    logl::log(level::INFO, format_args!("ratatui_demo: start"));

    match run_probe() {
        Ok(()) => logl::log(level::INFO, format_args!("ratatui_demo: done")),
        Err(stage) => logl::log(
            level::ERROR,
            format_args!("ratatui_demo: failed stage={}", stage),
        ),
    }
}

fn run_probe() -> Result<(), &'static str> {
    let backend = TrueOsKonsoleBackend::new(WIDTH, HEIGHT, ShellKonsoleSink::default());
    let mut terminal = Terminal::new(backend).map_err(|_| "terminal.new")?;
    let completed = terminal
        .draw(render_shell_model)
        .map_err(|_| "terminal.draw")?;
    validate_render(completed.buffer)
}

fn render_shell_model(frame: &mut Frame<'_>) {
    let area = frame.area();
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(5),
        Constraint::Length(3),
    ])
    .split(area);

    Paragraph::new(Line::from(vec![
        Span::styled(
            "TRUEOS",
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" kernel shell model", Style::new().fg(Color::LightGreen)),
    ]))
    .block(
        Block::new()
            .title("ratatui")
            .borders(Borders::ALL)
            .style(Style::new().fg(Color::Gray)),
    )
    .render(chunks[0], frame.buffer_mut());

    Paragraph::new(vec![
        Line::from(vec![
            Span::styled("prompt  ", Style::new().fg(Color::DarkGray)),
            Span::styled("/ > apps", Style::new().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("select  ", Style::new().fg(Color::DarkGray)),
            Span::styled("ratatui_demo", Style::new().fg(Color::LightCyan)),
        ]),
        Line::from(vec![
            Span::styled("render  ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                "Terminal<TrueOsKonsoleBackend>",
                Style::new().fg(Color::White).add_modifier(Modifier::ITALIC),
            ),
        ]),
    ])
    .block(
        Block::new()
            .title("shell")
            .borders(Borders::ALL)
            .style(Style::new().fg(Color::LightBlue)),
    )
    .render(chunks[1], frame.buffer_mut());

    Gauge::default()
        .block(
            Block::new()
                .title("model fit")
                .borders(Borders::ALL)
                .style(Style::new().fg(Color::Magenta)),
        )
        .gauge_style(
            Style::new()
                .fg(Color::Black)
                .bg(Color::LightGreen)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )
        .ratio(0.72)
        .label("ratatui vendored")
        .render(chunks[2], frame.buffer_mut());

    frame.set_cursor_position(Position::new(2, 4));
}

fn validate_render(buffer: &Buffer) -> Result<(), &'static str> {
    let rendered = buffer_text(buffer);
    if !rendered.contains("TRUEOS kernel shell model") {
        return Err("title.text");
    }
    if !rendered.contains("Terminal<TrueOsKonsoleBackend>") {
        return Err("body.text");
    }
    if !rendered.contains("ratatui vendored") {
        return Err("gauge.label");
    }
    Ok(())
}

fn buffer_text(buffer: &Buffer) -> String {
    let mut rendered = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            rendered.push_str(buffer[(x, y)].symbol());
        }
        rendered.push('\n');
    }
    rendered
}

trait KonsoleSink {
    fn begin_frame(&mut self, size: Size, cursor: Option<Position>);
    fn write_row(&mut self, y: u16, row: &str);
    fn end_frame(&mut self);
}

#[derive(Default)]
struct ShellKonsoleSink {
    cursor: Option<Position>,
}

impl KonsoleSink for ShellKonsoleSink {
    fn begin_frame(&mut self, size: Size, cursor: Option<Position>) {
        self.cursor = cursor;
        let status = vshell::konsole_begin_frame(
            u32::from(size.width),
            u32::from(size.height),
            RESERVED_TOP_ROWS,
        );
        if status != 0 {
            logl::log(
                level::ERROR,
                format_args!("ratatui_demo: konsole_begin_frame failed status={}", status),
            );
        }
    }

    fn write_row(&mut self, y: u16, row: &str) {
        let status = vshell::konsole_write_row(u32::from(y), 0, row.as_bytes());
        if status != 0 {
            logl::log(
                level::ERROR,
                format_args!(
                    "ratatui_demo: konsole_write_row failed row={} status={}",
                    y, status
                ),
            );
        }
    }

    fn end_frame(&mut self) {
        if let Some(cursor) = self.cursor {
            let _ = vshell::konsole_set_cursor(u32::from(cursor.y), u32::from(cursor.x), true);
        } else {
            let _ = vshell::konsole_set_cursor(0, 0, false);
        }
        let status = vshell::konsole_end_frame();
        if status != 0 {
            logl::log(
                level::ERROR,
                format_args!("ratatui_demo: konsole_end_frame failed status={}", status),
            );
        }
    }
}

struct TrueOsKonsoleBackend<S> {
    buffer: Buffer,
    cursor_position: Position,
    cursor_visible: bool,
    sink: S,
}

impl<S> TrueOsKonsoleBackend<S> {
    fn new(width: u16, height: u16, sink: S) -> Self {
        Self {
            buffer: Buffer::empty(Rect::new(0, 0, width, height)),
            cursor_position: Position::ORIGIN,
            cursor_visible: false,
            sink,
        }
    }
}

impl<S: KonsoleSink> TrueOsKonsoleBackend<S> {
    fn emit_frame(&mut self) {
        let cursor = self.cursor_visible.then_some(self.cursor_position);
        self.sink.begin_frame(self.buffer.area.as_size(), cursor);

        for y in 0..self.buffer.area.height {
            let row = self.styled_row(y);
            self.sink.write_row(y, row.as_str());
        }

        self.sink.end_frame();
    }

    fn styled_row(&self, y: u16) -> String {
        let mut row = String::new();
        let mut current = CellStyle::default();
        row.push_str("\x1b[0m");

        for x in 0..self.buffer.area.width {
            let cell = &self.buffer[(x, y)];
            let next = CellStyle::from_cell(cell);
            if next != current {
                emit_sgr(&mut row, next);
                current = next;
            }
            row.push_str(cell.symbol());
        }

        row.push_str("\x1b[0m");
        row
    }

    fn clear_cells(&mut self, clear_type: ClearType) {
        let area = self.buffer.area;
        match clear_type {
            ClearType::All => self.buffer.reset(),
            ClearType::AfterCursor => {
                let index = self
                    .buffer
                    .index_of(self.cursor_position.x, self.cursor_position.y);
                for cell in &mut self.buffer.content[index..] {
                    cell.reset();
                }
            }
            ClearType::BeforeCursor => {
                let index = self
                    .buffer
                    .index_of(self.cursor_position.x, self.cursor_position.y);
                for cell in &mut self.buffer.content[..=index] {
                    cell.reset();
                }
            }
            ClearType::CurrentLine => {
                let start = self.buffer.index_of(0, self.cursor_position.y);
                let end = self
                    .buffer
                    .index_of(area.width.saturating_sub(1), self.cursor_position.y);
                for cell in &mut self.buffer.content[start..=end] {
                    cell.reset();
                }
            }
            ClearType::UntilNewLine => {
                let start = self
                    .buffer
                    .index_of(self.cursor_position.x, self.cursor_position.y);
                let end = self
                    .buffer
                    .index_of(area.width.saturating_sub(1), self.cursor_position.y);
                for cell in &mut self.buffer.content[start..=end] {
                    cell.reset();
                }
            }
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct CellStyle {
    fg: Color,
    bg: Color,
    modifier: Modifier,
}

impl Default for CellStyle {
    fn default() -> Self {
        Self {
            fg: Color::Reset,
            bg: Color::Reset,
            modifier: Modifier::empty(),
        }
    }
}

impl CellStyle {
    fn from_cell(cell: &Cell) -> Self {
        Self {
            fg: cell.fg,
            bg: cell.bg,
            modifier: cell.modifier,
        }
    }
}

fn emit_sgr(out: &mut String, style: CellStyle) {
    out.push_str("\x1b[0");
    emit_modifier_codes(out, style.modifier);
    emit_color_code(out, style.fg, true);
    emit_color_code(out, style.bg, false);
    out.push('m');
}

fn emit_modifier_codes(out: &mut String, modifier: Modifier) {
    if modifier.contains(Modifier::BOLD) {
        out.push_str(";1");
    }
    if modifier.contains(Modifier::DIM) {
        out.push_str(";2");
    }
    if modifier.contains(Modifier::ITALIC) {
        out.push_str(";3");
    }
    if modifier.contains(Modifier::UNDERLINED) {
        out.push_str(";4");
    }
    if modifier.contains(Modifier::SLOW_BLINK) {
        out.push_str(";5");
    }
    if modifier.contains(Modifier::RAPID_BLINK) {
        out.push_str(";6");
    }
    if modifier.contains(Modifier::REVERSED) {
        out.push_str(";7");
    }
    if modifier.contains(Modifier::HIDDEN) {
        out.push_str(";8");
    }
    if modifier.contains(Modifier::CROSSED_OUT) {
        out.push_str(";9");
    }
}

fn emit_color_code(out: &mut String, color: Color, foreground: bool) {
    let base = if foreground { 30 } else { 40 };
    let bright_base = if foreground { 90 } else { 100 };
    match color {
        Color::Reset => {}
        Color::Black => {
            let _ = write!(out, ";{}", base);
        }
        Color::Red => {
            let _ = write!(out, ";{}", base + 1);
        }
        Color::Green => {
            let _ = write!(out, ";{}", base + 2);
        }
        Color::Yellow => {
            let _ = write!(out, ";{}", base + 3);
        }
        Color::Blue => {
            let _ = write!(out, ";{}", base + 4);
        }
        Color::Magenta => {
            let _ = write!(out, ";{}", base + 5);
        }
        Color::Cyan => {
            let _ = write!(out, ";{}", base + 6);
        }
        Color::Gray => {
            let _ = write!(out, ";{}", base + 7);
        }
        Color::DarkGray => {
            let _ = write!(out, ";{}", bright_base);
        }
        Color::LightRed => {
            let _ = write!(out, ";{}", bright_base + 1);
        }
        Color::LightGreen => {
            let _ = write!(out, ";{}", bright_base + 2);
        }
        Color::LightYellow => {
            let _ = write!(out, ";{}", bright_base + 3);
        }
        Color::LightBlue => {
            let _ = write!(out, ";{}", bright_base + 4);
        }
        Color::LightMagenta => {
            let _ = write!(out, ";{}", bright_base + 5);
        }
        Color::LightCyan => {
            let _ = write!(out, ";{}", bright_base + 6);
        }
        Color::White => {
            let _ = write!(out, ";{}", bright_base + 7);
        }
        Color::Indexed(index) => {
            let selector = if foreground { 38 } else { 48 };
            let _ = write!(out, ";{};5;{}", selector, index);
        }
        Color::Rgb(r, g, b) => {
            let selector = if foreground { 38 } else { 48 };
            let _ = write!(out, ";{};2;{};{};{}", selector, r, g, b);
        }
    }
}

impl<S: KonsoleSink> Backend for TrueOsKonsoleBackend<S> {
    type Error = core::convert::Infallible;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        for (x, y, cell) in content {
            if x < self.buffer.area.width && y < self.buffer.area.height {
                self.buffer[(x, y)] = cell.clone();
            }
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.cursor_visible = false;
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.cursor_visible = true;
        Ok(())
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        Ok(self.cursor_position)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        self.cursor_position = position.into();
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.clear_cells(ClearType::All);
        Ok(())
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        self.clear_cells(clear_type);
        Ok(())
    }

    fn size(&self) -> Result<Size, Self::Error> {
        Ok(self.buffer.area.as_size())
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        let size = self.buffer.area.as_size();
        Ok(WindowSize {
            columns_rows: size,
            pixels: Size::ZERO,
        })
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        self.emit_frame();
        Ok(())
    }
}
