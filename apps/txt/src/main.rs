use std::{
    cmp, env, fs,
    io::{self, BufWriter, Write, stdout},
    path::PathBuf,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Paragraph, Widget},
};

const TAB_WIDTH: usize = 4;
const WHEEL_STEP: usize = 3;
const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(100);
const FRAME_BUFFER_CAPACITY: usize = 128 * 1024;

type EditorTerminal = Terminal<CrosstermBackend<BufWriter<io::Stdout>>>;

fn main() -> Result<()> {
    let result = run_app();

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    trueos::vshell::leave_terminal_handoff();

    result
}

fn run_app() -> Result<()> {
    let file = parse_file_arg()?;
    let mut app = App::open(file)?;
    let mut terminal = TerminalGuard::enter()?;
    let result = run(&mut terminal.terminal, &mut app);
    terminal.exit()?;
    result
}

fn parse_file_arg() -> Result<Option<PathBuf>> {
    let mut args = env::args_os();
    let _program = args.next();
    let file = args.next().map(PathBuf::from);
    if args.next().is_some() {
        bail!("txt accepts one file path; use `txt [FILE]`");
    }
    Ok(file)
}

fn run(terminal: &mut EditorTerminal, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            draw_editor(frame.buffer_mut(), area, app);
            if area.width != 0 && area.height != 0 {
                frame.set_cursor_position(app.cursor_position(area));
            }
        })?;

        if app.should_quit {
            return Ok(());
        }

        if event::poll(INPUT_POLL_INTERVAL)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => app.handle_key(key)?,
                Event::Mouse(mouse) => {
                    let size = terminal.size()?;
                    app.handle_mouse(mouse, Rect::new(0, 0, size.width, size.height));
                }
                Event::Paste(text) => app.insert_str(&text),
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }
}

struct TerminalGuard {
    terminal: EditorTerminal,
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

struct App {
    lines: Vec<String>,
    cursor_x: usize,
    cursor_y: usize,
    preferred_x: usize,
    row_offset: usize,
    col_offset: usize,
    selection: Option<Selection>,
    drag_mode: Option<SelectionMode>,
    file: Option<PathBuf>,
    dirty: bool,
    should_quit: bool,
    quit_warning: bool,
    message: String,
}

impl App {
    fn open(file: Option<PathBuf>) -> Result<Self> {
        let (lines, message) = match file.as_ref() {
            Some(path) if path.exists() => {
                let content = fs::read_to_string(path)
                    .with_context(|| format!("failed to read {}", path.display()))?;
                (split_lines(&content), format!("Opened {}", path.display()))
            }
            Some(path) => (vec![String::new()], format!("New file: {}", path.display())),
            None => (vec![String::new()], "New buffer".to_string()),
        };

        Ok(Self {
            lines,
            cursor_x: 0,
            cursor_y: 0,
            preferred_x: 0,
            row_offset: 0,
            col_offset: 0,
            selection: None,
            drag_mode: None,
            file,
            dirty: false,
            should_quit: false,
            quit_warning: false,
            message,
        })
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        let is_quit = key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('q');
        if !is_quit {
            self.quit_warning = false;
        }

        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('q')) => self.quit(),
            (KeyModifiers::CONTROL, KeyCode::Char('s')) => self.save()?,
            (KeyModifiers::CONTROL, KeyCode::Char('a')) => self.move_to_line_start(),
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => self.move_to_line_end(),
            (_, KeyCode::Esc) => {
                self.clear_selection();
                self.message.clear();
            }
            (_, KeyCode::Up) => self.move_up(1),
            (_, KeyCode::Down) => self.move_down(1),
            (_, KeyCode::Left) => self.move_left(),
            (_, KeyCode::Right) => self.move_right(),
            (_, KeyCode::PageUp) => self.move_up(16),
            (_, KeyCode::PageDown) => self.move_down(16),
            (_, KeyCode::Home) => self.move_to_line_start(),
            (_, KeyCode::End) => self.move_to_line_end(),
            (_, KeyCode::Backspace) => self.backspace(),
            (_, KeyCode::Delete) => self.delete(),
            (_, KeyCode::Enter) => self.insert_newline(),
            (_, KeyCode::Tab) => self.insert_str(&" ".repeat(TAB_WIDTH)),
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(ch)) => self.insert_char(ch),
            _ => {}
        }
        Ok(())
    }

    fn handle_mouse(&mut self, mouse: MouseEvent, area: Rect) {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(position) = self.position_from_mouse(area, mouse.column, mouse.row) else {
                    return;
                };
                let mode = selection_mode_from_modifiers(mouse.modifiers);
                self.set_cursor(position);
                self.selection = Some(Selection {
                    anchor: position,
                    head: position,
                    mode,
                });
                self.drag_mode = Some(mode);
                self.message = match mode {
                    SelectionMode::Normal => String::new(),
                    SelectionMode::Rect => "Rect selection".to_string(),
                };
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some(position) = self.position_from_mouse(area, mouse.column, mouse.row) else {
                    return;
                };
                self.set_cursor(position);
                if let Some(selection) = self.selection.as_mut() {
                    selection.head = position;
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.drag_mode = None;
                if !self.has_selection() {
                    self.clear_selection();
                }
            }
            MouseEventKind::ScrollUp => self.scroll_up(WHEEL_STEP),
            MouseEventKind::ScrollDown => self.scroll_down(WHEEL_STEP),
            _ => {}
        }
    }

    fn quit(&mut self) {
        if self.dirty && !self.quit_warning {
            self.quit_warning = true;
            self.message = "Unsaved changes. Press Ctrl-Q again to quit.".to_string();
        } else {
            self.should_quit = true;
        }
    }

    fn save(&mut self) -> Result<()> {
        let Some(path) = self.file.as_ref() else {
            self.message = "No file path. Start with: txt <file>".to_string();
            return Ok(());
        };

        let mut content = self.lines.join("\n");
        content.push('\n');
        fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))?;
        self.dirty = false;
        self.message = format!("Saved {}", path.display());
        Ok(())
    }

    fn insert_str(&mut self, text: &str) {
        self.delete_selection();
        for ch in text.chars() {
            match ch {
                '\n' => self.insert_newline(),
                '\r' => {}
                '\t' => self.insert_str(&" ".repeat(TAB_WIDTH)),
                ch if !ch.is_control() => self.insert_char(ch),
                _ => {}
            }
        }
    }

    fn insert_char(&mut self, ch: char) {
        self.delete_selection();
        self.ensure_line_width(self.cursor_y, self.cursor_x);
        let byte = char_to_byte_index(&self.lines[self.cursor_y], self.cursor_x);
        self.lines[self.cursor_y].insert(byte, ch);
        self.cursor_x += 1;
        self.preferred_x = self.cursor_x;
        self.mark_dirty();
    }

    fn insert_newline(&mut self) {
        self.delete_selection();
        self.ensure_line_width(self.cursor_y, self.cursor_x);
        let byte = char_to_byte_index(&self.lines[self.cursor_y], self.cursor_x);
        let tail = self.lines[self.cursor_y].split_off(byte);
        self.lines.insert(self.cursor_y + 1, tail);
        self.cursor_y += 1;
        self.cursor_x = 0;
        self.preferred_x = 0;
        self.mark_dirty();
    }

    fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }

        let current_len = line_len(&self.lines[self.cursor_y]);
        if self.cursor_x > current_len {
            self.cursor_x -= 1;
            self.preferred_x = self.cursor_x;
        } else if self.cursor_x > 0 {
            let line = &mut self.lines[self.cursor_y];
            let end = char_to_byte_index(line, self.cursor_x);
            let start = char_to_byte_index(line, self.cursor_x - 1);
            line.replace_range(start..end, "");
            self.cursor_x -= 1;
            self.preferred_x = self.cursor_x;
            self.mark_dirty();
        } else if self.cursor_y > 0 {
            let current = self.lines.remove(self.cursor_y);
            self.cursor_y -= 1;
            self.cursor_x = line_len(&self.lines[self.cursor_y]);
            self.lines[self.cursor_y].push_str(&current);
            self.preferred_x = self.cursor_x;
            self.mark_dirty();
        }
    }

    fn delete(&mut self) {
        if self.delete_selection() {
            return;
        }

        let current_len = line_len(&self.lines[self.cursor_y]);
        if self.cursor_x < current_len {
            let line = &mut self.lines[self.cursor_y];
            let start = char_to_byte_index(line, self.cursor_x);
            let end = char_to_byte_index(line, self.cursor_x + 1);
            line.replace_range(start..end, "");
            self.mark_dirty();
        } else if self.cursor_x == current_len && self.cursor_y + 1 < self.lines.len() {
            let next = self.lines.remove(self.cursor_y + 1);
            self.lines[self.cursor_y].push_str(&next);
            self.mark_dirty();
        }
    }

    fn move_up(&mut self, count: usize) {
        self.clear_selection();
        self.cursor_y = self.cursor_y.saturating_sub(count);
        self.cursor_x = self.preferred_x;
    }

    fn move_down(&mut self, count: usize) {
        self.clear_selection();
        self.cursor_y = cmp::min(self.cursor_y + count, self.lines.len().saturating_sub(1));
        self.cursor_x = self.preferred_x;
    }

    fn move_left(&mut self) {
        self.clear_selection();
        if self.cursor_x > 0 {
            self.cursor_x -= 1;
        } else if self.cursor_y > 0 {
            self.cursor_y -= 1;
            self.cursor_x = line_len(&self.lines[self.cursor_y]);
        }
        self.preferred_x = self.cursor_x;
    }

    fn move_right(&mut self) {
        self.clear_selection();
        self.cursor_x += 1;
        self.preferred_x = self.cursor_x;
    }

    fn move_to_line_start(&mut self) {
        self.clear_selection();
        self.cursor_x = 0;
        self.preferred_x = 0;
    }

    fn move_to_line_end(&mut self) {
        self.clear_selection();
        self.cursor_x = line_len(&self.lines[self.cursor_y]);
        self.preferred_x = self.cursor_x;
    }

    fn scroll_up(&mut self, count: usize) {
        self.row_offset = self.row_offset.saturating_sub(count);
    }

    fn scroll_down(&mut self, count: usize) {
        self.row_offset = cmp::min(self.row_offset + count, self.lines.len().saturating_sub(1));
    }

    fn set_cursor(&mut self, position: TextPosition) {
        let position = self.clamp_position(position);
        self.cursor_x = position.x;
        self.cursor_y = position.y;
        self.preferred_x = position.x;
    }

    fn position_from_mouse(&self, area: Rect, column: u16, row: u16) -> Option<TextPosition> {
        let editor = editor_area(area);
        if editor.is_empty()
            || row < editor.y
            || row >= editor.bottom()
            || column < editor.x
            || column >= editor.right()
        {
            return None;
        }

        let gutter_width = self.gutter_width();
        let y = self.row_offset + (row - editor.y) as usize;
        let x = if column < editor.x + gutter_width {
            0
        } else {
            self.col_offset + (column - editor.x - gutter_width) as usize
        };
        Some(self.clamp_position(TextPosition { x, y }))
    }

    fn clamp_position(&self, position: TextPosition) -> TextPosition {
        TextPosition {
            x: position.x,
            y: cmp::min(position.y, self.lines.len().saturating_sub(1)),
        }
    }

    fn ensure_line_width(&mut self, y: usize, width: usize) {
        let missing = width.saturating_sub(line_len(&self.lines[y]));
        if missing != 0 {
            self.lines[y].push_str(&" ".repeat(missing));
        }
    }

    fn has_selection(&self) -> bool {
        self.selection
            .is_some_and(|selection| selection.has_cells())
    }

    fn clear_selection(&mut self) {
        self.selection = None;
        self.drag_mode = None;
    }

    fn delete_selection(&mut self) -> bool {
        let Some(selection) = self.selection.take() else {
            return false;
        };
        if !selection.has_cells() {
            return false;
        }

        match selection.mode {
            SelectionMode::Normal => self.delete_normal_selection(selection),
            SelectionMode::Rect => self.delete_rect_selection(selection),
        }
        self.mark_dirty();
        true
    }

    fn delete_normal_selection(&mut self, selection: Selection) {
        let (start, end) = selection.ordered();
        if start.y == end.y {
            let line = &mut self.lines[start.y];
            let start_byte = char_to_byte_index(line, start.x);
            let end_byte = char_to_byte_index(line, end.x);
            line.replace_range(start_byte..end_byte, "");
        } else {
            let start_byte = char_to_byte_index(&self.lines[start.y], start.x);
            let end_byte = char_to_byte_index(&self.lines[end.y], end.x);
            let prefix = self.lines[start.y][..start_byte].to_string();
            let suffix = self.lines[end.y][end_byte..].to_string();
            self.lines[start.y] = format!("{prefix}{suffix}");
            self.lines.drain(start.y + 1..=end.y);
        }
        self.set_cursor(start);
    }

    fn delete_rect_selection(&mut self, selection: Selection) {
        let (start_y, end_y) = ordered_pair(selection.anchor.y, selection.head.y);
        let (start_x, end_x) = ordered_pair(selection.anchor.x, selection.head.x);
        for y in start_y..=end_y {
            let current_len = line_len(&self.lines[y]);
            if start_x >= current_len {
                continue;
            }
            let start_byte = char_to_byte_index(&self.lines[y], start_x);
            let end_byte = char_to_byte_index(&self.lines[y], cmp::min(end_x, current_len));
            self.lines[y].replace_range(start_byte..end_byte, "");
        }
        self.set_cursor(TextPosition {
            x: start_x,
            y: start_y,
        });
    }

    fn is_selected(&self, y: usize, x: usize) -> bool {
        self.selection
            .is_some_and(|selection| selection.contains(y, x))
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.quit_warning = false;
    }

    fn cursor_position(&mut self, area: Rect) -> Position {
        let editor = editor_area(area);
        self.keep_cursor_visible(editor);
        let x = editor
            .x
            .saturating_add(self.gutter_width())
            .saturating_add(self.cursor_x.saturating_sub(self.col_offset) as u16);
        let y = editor
            .y
            .saturating_add(self.cursor_y.saturating_sub(self.row_offset) as u16);
        Position::new(
            x.min(editor.right().saturating_sub(1)),
            y.min(editor.bottom().saturating_sub(1)),
        )
    }

    fn keep_cursor_visible(&mut self, editor: Rect) {
        let height = editor.height as usize;
        let width = editor.width.saturating_sub(self.gutter_width()) as usize;
        if height == 0 || width == 0 {
            return;
        }

        if self.cursor_y < self.row_offset {
            self.row_offset = self.cursor_y;
        } else if self.cursor_y >= self.row_offset + height {
            self.row_offset = self.cursor_y + 1 - height;
        }

        if self.cursor_x < self.col_offset {
            self.col_offset = self.cursor_x;
        } else if self.cursor_x >= self.col_offset + width {
            self.col_offset = self.cursor_x + 1 - width;
        }
    }

    fn gutter_width(&self) -> u16 {
        self.lines.len().max(1).to_string().len() as u16 + 2
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TextPosition {
    x: usize,
    y: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelectionMode {
    Normal,
    Rect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Selection {
    anchor: TextPosition,
    head: TextPosition,
    mode: SelectionMode,
}

impl Selection {
    fn has_cells(self) -> bool {
        match self.mode {
            SelectionMode::Normal => self.anchor != self.head,
            SelectionMode::Rect => self.anchor.x != self.head.x,
        }
    }

    fn ordered(self) -> (TextPosition, TextPosition) {
        if position_before_or_equal(self.anchor, self.head) {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    fn contains(self, y: usize, x: usize) -> bool {
        if !self.has_cells() {
            return false;
        }
        match self.mode {
            SelectionMode::Normal => {
                let (start, end) = self.ordered();
                position_before_or_equal(start, TextPosition { x, y })
                    && position_before(TextPosition { x, y }, end)
            }
            SelectionMode::Rect => {
                let (start_y, end_y) = ordered_pair(self.anchor.y, self.head.y);
                let (start_x, end_x) = ordered_pair(self.anchor.x, self.head.x);
                (start_y..=end_y).contains(&y) && (start_x..end_x).contains(&x)
            }
        }
    }
}

fn draw_editor(buf: &mut Buffer, area: Rect, app: &mut App) {
    let editor = editor_area(area);
    app.keep_cursor_visible(editor);
    Block::default().borders(Borders::BOTTOM).render(area, buf);
    draw_rows(buf, editor, app);
    draw_status(buf, area, app);
}

fn draw_rows(buf: &mut Buffer, area: Rect, app: &App) {
    if area.is_empty() {
        return;
    }

    let gutter_width = app.gutter_width();
    let text_width = area.width.saturating_sub(gutter_width) as usize;
    let selected_style = Style::default().add_modifier(Modifier::REVERSED);
    for visual_row in 0..area.height {
        let y = area.y + visual_row;
        let line_index = app.row_offset + visual_row as usize;
        clear_line(buf, area.x, y, area.width);
        if line_index >= app.lines.len() {
            continue;
        }

        let line_number = format!(
            "{:>width$} ",
            line_index + 1,
            width = gutter_width.saturating_sub(2) as usize
        );
        buf.set_string(area.x, y, line_number, Style::default());
        buf.set_string(area.x + gutter_width - 1, y, "|", Style::default());

        let visible = app.lines[line_index]
            .chars()
            .skip(app.col_offset)
            .take(text_width)
            .collect::<Vec<_>>();
        for column in 0..text_width {
            let buffer_x = area.x + gutter_width + column as u16;
            let text_x = app.col_offset + column;
            let ch = visible.get(column).copied().unwrap_or(' ');
            let style = if app.is_selected(line_index, text_x) {
                selected_style
            } else {
                Style::default()
            };
            buf[(buffer_x, y)].set_char(ch).set_style(style);
        }
    }
}

fn draw_status(buf: &mut Buffer, area: Rect, app: &App) {
    if area.height == 0 {
        return;
    }

    let y = area.bottom().saturating_sub(1);
    let dirty = if app.dirty { "modified" } else { "saved" };
    let name = app
        .file
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "[No Name]".to_string());
    let mut status = format!(
        " {name} | {dirty} | Ln {}, Col {} ",
        app.cursor_y + 1,
        app.cursor_x + 1
    );
    if !app.message.is_empty() {
        status.push_str("| ");
        status.push_str(&app.message);
        status.push(' ');
    }

    let help = " Ctrl-S save | Ctrl-Q quit ";
    let width = area.width as usize;
    let used = status.chars().count() + help.chars().count();
    if used < width {
        status.push_str(&" ".repeat(width - used));
        status.push_str(help);
    }
    Paragraph::new(visible_slice(&status, width))
        .style(Style::default().add_modifier(Modifier::REVERSED))
        .render(Rect::new(area.x, y, area.width, 1), buf);
}

fn editor_area(area: Rect) -> Rect {
    Rect {
        height: area.height.saturating_sub(1),
        ..area
    }
}

fn clear_line(buf: &mut Buffer, x: u16, y: u16, width: u16) {
    for col in 0..width {
        buf[(x + col, y)].reset();
    }
}

fn split_lines(content: &str) -> Vec<String> {
    let mut lines = content
        .split('\n')
        .map(|line| line.trim_end_matches('\r').to_string())
        .collect::<Vec<_>>();
    if content.ends_with('\n') && lines.len() > 1 {
        lines.pop();
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn visible_slice(text: &str, width: usize) -> String {
    text.chars().take(width).collect()
}

fn line_len(line: &str) -> usize {
    line.chars().count()
}

fn char_to_byte_index(line: &str, char_index: usize) -> usize {
    line.char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(line.len())
}

fn ordered_pair(a: usize, b: usize) -> (usize, usize) {
    if a <= b { (a, b) } else { (b, a) }
}

fn position_before(a: TextPosition, b: TextPosition) -> bool {
    a.y < b.y || (a.y == b.y && a.x < b.x)
}

fn position_before_or_equal(a: TextPosition, b: TextPosition) -> bool {
    a == b || position_before(a, b)
}

fn selection_mode_from_modifiers(modifiers: KeyModifiers) -> SelectionMode {
    if modifiers.contains(KeyModifiers::ALT) {
        SelectionMode::Rect
    } else {
        SelectionMode::Normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_lines_normalizes_line_endings_and_final_newline() {
        assert_eq!(split_lines("one\r\ntwo\n"), vec!["one", "two"]);
        assert_eq!(split_lines(""), vec![""]);
    }

    #[test]
    fn unicode_edits_use_character_columns() {
        let mut app = App::open(None).unwrap();
        app.insert_str("a🦀b");
        assert_eq!(app.lines, vec!["a🦀b"]);
        assert_eq!(app.cursor_x, 3);
        app.backspace();
        assert_eq!(app.lines, vec!["a🦀"]);
        app.backspace();
        assert_eq!(app.lines, vec!["a"]);
    }

    #[test]
    fn dirty_buffer_requires_two_quit_commands() {
        let mut app = App::open(None).unwrap();
        app.insert_char('x');
        app.quit();
        assert!(!app.should_quit);
        app.quit();
        assert!(app.should_quit);
    }

    #[test]
    fn rectangular_delete_affects_each_selected_row() {
        let mut app = App::open(None).unwrap();
        app.lines = vec!["abcd".into(), "wxyz".into()];
        app.selection = Some(Selection {
            anchor: TextPosition { x: 1, y: 0 },
            head: TextPosition { x: 3, y: 1 },
            mode: SelectionMode::Rect,
        });
        assert!(app.delete_selection());
        assert_eq!(app.lines, vec!["ad", "wz"]);
    }
}
