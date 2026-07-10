use std::io::{self, BufWriter};
use std::time::Duration;

use anyhow::Result;
use ratatui::crossterm::{
    cursor::{Hide, Show},
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
        MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use trueos::calculator_base::{
    CALCULATOR_FUNCTIONS, CALCULATOR_PROTOCOL_VERSION, CalculatorFunctionSpec, CalculatorOperation,
    evaluate,
};
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use trueos_math::calculator_base::{
    CALCULATOR_FUNCTIONS, CALCULATOR_PROTOCOL_VERSION, CalculatorFunctionSpec, CalculatorOperation,
    evaluate_operation as evaluate,
};

const MAX_BUTTONS: usize = 32;
const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(16);
const FRAME_BUFFER_CAPACITY: usize = 128 * 1024;
const BG: Color = Color::Rgb(8, 11, 18);
const PANEL: Color = Color::Rgb(18, 24, 35);
const PANEL_ALT: Color = Color::Rgb(25, 34, 48);
const TEXT: Color = Color::Rgb(229, 235, 244);
const DIM: Color = Color::Rgb(132, 147, 168);
const ACCENT: Color = Color::Rgb(103, 211, 189);
const BLUE: Color = Color::Rgb(111, 169, 242);
const WARN: Color = Color::Rgb(247, 190, 94);
const ERROR: Color = Color::Rgb(247, 117, 131);

fn main() -> Result<()> {
    let result = run();

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    trueos::vshell::leave_terminal_handoff();

    result
}

fn run() -> Result<()> {
    let mut terminal = setup_terminal()?;
    let result = App::default().run(&mut terminal);
    restore_terminal(&mut terminal)?;
    result
}

type CalculatorTerminal = Terminal<CrosstermBackend<BufWriter<io::Stdout>>>;

fn setup_terminal() -> Result<CalculatorTerminal> {
    enable_raw_mode()?;
    // Crossterm emits several small writes per changed cell. Buffer a complete
    // Ratatui frame so TRUEOS only has to cross the VM console ABI once.
    let mut stdout = BufWriter::with_capacity(FRAME_BUFFER_CAPACITY, io::stdout());
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture, Hide)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal(terminal: &mut CalculatorTerminal) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        Show,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Focus {
    Input,
    Buttons,
    Add,
    Remove,
    Evaluate,
}

impl Focus {
    const fn next(self) -> Self {
        match self {
            Self::Input => Self::Buttons,
            Self::Buttons => Self::Add,
            Self::Add => Self::Remove,
            Self::Remove => Self::Evaluate,
            Self::Evaluate => Self::Input,
        }
    }

    const fn previous(self) -> Self {
        match self {
            Self::Input => Self::Evaluate,
            Self::Buttons => Self::Input,
            Self::Add => Self::Buttons,
            Self::Remove => Self::Add,
            Self::Evaluate => Self::Remove,
        }
    }
}

#[derive(Debug, Default)]
struct HitBoxes {
    input: Rect,
    add: Rect,
    remove: Rect,
    evaluate: Rect,
    buttons: Vec<Rect>,
    dropdown_items: Vec<(usize, Rect)>,
    dropdown: Rect,
    button_columns: usize,
}

#[derive(Debug)]
struct App {
    input: String,
    output: String,
    status_is_error: bool,
    buttons: Vec<CalculatorOperation>,
    selected_button: usize,
    focus: Focus,
    dropdown_open: bool,
    dropdown_index: usize,
    dropdown_offset: usize,
    hit_boxes: HitBoxes,
    should_quit: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            input: String::from("12, 3"),
            output: String::from("Select an operation, then press ="),
            status_is_error: false,
            buttons: vec![
                CalculatorOperation::Add,
                CalculatorOperation::Subtract,
                CalculatorOperation::Multiply,
                CalculatorOperation::Divide,
            ],
            selected_button: 0,
            focus: Focus::Input,
            dropdown_open: false,
            dropdown_index: 0,
            dropdown_offset: 0,
            hit_boxes: HitBoxes::default(),
            should_quit: false,
        }
    }
}

impl App {
    fn run(mut self, terminal: &mut CalculatorTerminal) -> Result<()> {
        terminal.draw(|frame| self.draw(frame))?;

        while !self.should_quit {
            if !event::poll(INPUT_POLL_INTERVAL)? {
                continue;
            }

            let mut redraw = false;
            loop {
                match event::read()? {
                    Event::Key(key) if key.is_press() => {
                        self.handle_key(key);
                        redraw = true;
                    }
                    Event::Mouse(mouse) => {
                        self.handle_mouse(mouse);
                        redraw = true;
                    }
                    Event::Resize(_, _) => redraw = true,
                    _ => {}
                }

                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }

            if redraw && !self.should_quit {
                terminal.draw(|frame| self.draw(frame))?;
            }
        }
        Ok(())
    }

    fn selected_operation(&self) -> Option<CalculatorOperation> {
        self.buttons.get(self.selected_button).copied()
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if self.dropdown_open {
            self.handle_dropdown_key(key);
            return;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        match key.code {
            KeyCode::Esc => self.should_quit = true,
            KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.focus = self.focus.previous();
            }
            KeyCode::Tab => self.focus = self.focus.next(),
            KeyCode::Char('a') if self.focus != Focus::Input => self.open_dropdown(),
            KeyCode::Delete if self.focus == Focus::Buttons => self.remove_selected_button(),
            KeyCode::Left if self.focus == Focus::Buttons => self.select_previous_button(),
            KeyCode::Right if self.focus == Focus::Buttons => self.select_next_button(),
            KeyCode::Up if self.focus == Focus::Buttons => self.move_button_row(-1),
            KeyCode::Down if self.focus == Focus::Buttons => self.move_button_row(1),
            KeyCode::Enter => self.activate_focus(),
            KeyCode::Backspace if self.focus == Focus::Input => {
                self.input.pop();
            }
            KeyCode::Delete if self.focus == Focus::Input => self.input.clear(),
            KeyCode::Char(character)
                if self.focus == Focus::Input
                    && !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.input.push(character);
            }
            _ => {}
        }
    }

    fn activate_focus(&mut self) {
        match self.focus {
            Focus::Input | Focus::Evaluate => self.evaluate_selected(),
            Focus::Buttons => self.describe_selected(),
            Focus::Add => self.open_dropdown(),
            Focus::Remove => self.remove_selected_button(),
        }
    }

    fn handle_dropdown_key(&mut self, key: KeyEvent) {
        let last = CALCULATOR_FUNCTIONS.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc => self.dropdown_open = false,
            KeyCode::Up => self.dropdown_index = self.dropdown_index.saturating_sub(1),
            KeyCode::Down => self.dropdown_index = (self.dropdown_index + 1).min(last),
            KeyCode::PageUp => self.dropdown_index = self.dropdown_index.saturating_sub(10),
            KeyCode::PageDown => self.dropdown_index = (self.dropdown_index + 10).min(last),
            KeyCode::Home => self.dropdown_index = 0,
            KeyCode::End => self.dropdown_index = last,
            KeyCode::Enter => self.add_dropdown_selection(),
            _ => {}
        }
        self.keep_dropdown_selection_visible();
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        let column = mouse.column;
        let row = mouse.row;
        if self.dropdown_open {
            match mouse.kind {
                MouseEventKind::ScrollUp => {
                    self.dropdown_index = self.dropdown_index.saturating_sub(1)
                }
                MouseEventKind::ScrollDown => {
                    self.dropdown_index =
                        (self.dropdown_index + 1).min(CALCULATOR_FUNCTIONS.len().saturating_sub(1));
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some((index, _)) = self
                        .hit_boxes
                        .dropdown_items
                        .iter()
                        .find(|(_, area)| contains(*area, column, row))
                    {
                        self.dropdown_index = *index;
                        self.add_dropdown_selection();
                    } else if !contains(self.hit_boxes.dropdown, column, row) {
                        self.dropdown_open = false;
                    }
                }
                _ => {}
            }
            self.keep_dropdown_selection_visible();
            return;
        }

        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            return;
        }
        if contains(self.hit_boxes.input, column, row) {
            self.focus = Focus::Input;
        } else if contains(self.hit_boxes.add, column, row) {
            self.focus = Focus::Add;
            self.open_dropdown();
        } else if contains(self.hit_boxes.remove, column, row) {
            self.focus = Focus::Remove;
            self.remove_selected_button();
        } else if contains(self.hit_boxes.evaluate, column, row) {
            self.focus = Focus::Evaluate;
            self.evaluate_selected();
        } else if let Some(index) = self
            .hit_boxes
            .buttons
            .iter()
            .position(|area| contains(*area, column, row))
        {
            self.focus = Focus::Buttons;
            self.selected_button = index;
            self.describe_selected();
        }
    }

    fn open_dropdown(&mut self) {
        self.dropdown_open = true;
        self.dropdown_index = self
            .selected_operation()
            .map(|operation| operation as usize)
            .unwrap_or(0);
        self.keep_dropdown_selection_visible();
    }

    fn add_dropdown_selection(&mut self) {
        let operation = CALCULATOR_FUNCTIONS[self.dropdown_index].operation;
        if self.buttons.len() >= MAX_BUTTONS {
            self.set_error("The calculator pad has reached its 32-button soft cap");
            self.dropdown_open = false;
            return;
        }
        if self.buttons.contains(&operation) {
            self.set_error("That function is already on the calculator pad");
            self.dropdown_open = false;
            return;
        }
        self.buttons.push(operation);
        self.selected_button = self.buttons.len() - 1;
        self.focus = Focus::Buttons;
        self.dropdown_open = false;
        self.describe_selected();
    }

    fn remove_selected_button(&mut self) {
        if self.buttons.is_empty() {
            self.set_error("There is no custom button to remove");
            return;
        }
        let removed = self.buttons.remove(self.selected_button);
        self.selected_button = self
            .selected_button
            .min(self.buttons.len().saturating_sub(1));
        self.output = format!(
            "Removed {}. Use Add function to put it back.",
            removed.spec().name
        );
        self.status_is_error = false;
    }

    fn evaluate_selected(&mut self) {
        let Some(operation) = self.selected_operation() else {
            self.set_error("Add and select a function before evaluating");
            return;
        };
        let spec = operation.spec();
        let arguments = match parse_arguments(&self.input) {
            Ok(arguments) => arguments,
            Err(message) => {
                self.set_error(message);
                return;
            }
        };
        if arguments.len() != spec.arity as usize {
            self.set_error(format!(
                "{} expects {} value(s): {}. Received {}.",
                spec.name,
                spec.arity,
                spec.arguments,
                arguments.len()
            ));
            return;
        }
        match evaluate(operation, &arguments) {
            Ok(value) => {
                self.output = format!(
                    "{}({}) = {}",
                    spec.name,
                    self.input.trim(),
                    format_value(value)
                );
                self.status_is_error = false;
            }
            Err(error) => self.set_error(format!("Evaluation failed: {error:?}")),
        }
    }

    fn describe_selected(&mut self) {
        if let Some(operation) = self.selected_operation() {
            let spec = operation.spec();
            self.output = format!(
                "{} [{}] expects {} value(s): {}",
                spec.name, spec.category, spec.arity, spec.arguments
            );
            self.status_is_error = false;
        }
    }

    fn set_error(&mut self, message: impl Into<String>) {
        self.output = message.into();
        self.status_is_error = true;
    }

    fn select_previous_button(&mut self) {
        self.selected_button = self.selected_button.saturating_sub(1);
    }

    fn select_next_button(&mut self) {
        self.selected_button = (self.selected_button + 1).min(self.buttons.len().saturating_sub(1));
    }

    fn move_button_row(&mut self, direction: isize) {
        let columns = self.hit_boxes.button_columns;
        if columns == 0 {
            return;
        }
        if direction < 0 {
            self.selected_button = self.selected_button.saturating_sub(columns);
        } else {
            self.selected_button = (self.selected_button + columns).min(self.buttons.len() - 1);
        }
    }

    fn keep_dropdown_selection_visible(&mut self) {
        let visible = self.hit_boxes.dropdown_items.len().max(12);
        if self.dropdown_index < self.dropdown_offset {
            self.dropdown_offset = self.dropdown_index;
        } else if self.dropdown_index >= self.dropdown_offset + visible {
            self.dropdown_offset = self.dropdown_index + 1 - visible;
        }
    }

    fn draw(&mut self, frame: &mut Frame<'_>) {
        self.hit_boxes = HitBoxes::default();
        let area = frame.area();
        frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

        let root = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(7),
                Constraint::Length(3),
                Constraint::Min(6),
                Constraint::Length(3),
            ])
            .split(area);
        self.draw_header(frame, root[0]);
        self.draw_io(frame, root[1]);
        self.draw_controls(frame, root[2]);
        self.draw_button_grid(frame, root[3]);
        self.draw_footer(frame, root[4]);
        if self.dropdown_open {
            self.draw_dropdown(frame, area);
        }
    }

    fn draw_header(&self, frame: &mut Frame<'_>, area: Rect) {
        let selected = self
            .selected_operation()
            .map(|operation| operation.spec().name)
            .unwrap_or("none");
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    "TRUEOS Calculator",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(
                        "  protocol v{}  selected: {}  pad: {}/{}",
                        CALCULATOR_PROTOCOL_VERSION,
                        selected,
                        self.buttons.len(),
                        MAX_BUTTONS
                    ),
                    Style::default().fg(DIM),
                ),
            ]))
            .block(panel("custom calculator"))
            .style(Style::default().bg(PANEL)),
            area,
        );
    }

    fn draw_io(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Length(4)])
            .split(area);
        self.hit_boxes.input = rows[0];

        let input_width = rows[0].width.saturating_sub(2) as usize;
        let start = self.input.len().saturating_sub(input_width);
        let shown = self.input.get(start..).unwrap_or(self.input.as_str());
        let input_style = if self.focus == Focus::Input {
            Style::default().fg(Color::Black).bg(ACCENT)
        } else {
            Style::default().fg(TEXT).bg(PANEL_ALT)
        };
        frame.render_widget(
            Paragraph::new(shown)
                .block(panel("Input · comma or space separated"))
                .style(input_style),
            rows[0],
        );
        if self.focus == Focus::Input && !self.dropdown_open {
            frame.set_cursor_position((
                rows[0].x + 1 + shown.len().min(input_width) as u16,
                rows[0].y + 1,
            ));
        }

        let output_color = if self.status_is_error { ERROR } else { TEXT };
        frame.render_widget(
            Paragraph::new(self.output.as_str())
                .block(panel("Output"))
                .style(Style::default().fg(output_color).bg(PANEL))
                .wrap(Wrap { trim: true }),
            rows[1],
        );
    }

    fn draw_controls(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let controls = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(30),
                Constraint::Percentage(30),
                Constraint::Percentage(40),
            ])
            .split(area);
        self.hit_boxes.add = controls[0];
        self.hit_boxes.remove = controls[1];
        self.hit_boxes.evaluate = controls[2];
        draw_action(
            frame,
            controls[0],
            "+ Add function",
            self.focus == Focus::Add,
            BLUE,
        );
        draw_action(
            frame,
            controls[1],
            "- Remove selected",
            self.focus == Focus::Remove,
            WARN,
        );
        draw_action(
            frame,
            controls[2],
            "= Evaluate",
            self.focus == Focus::Evaluate,
            ACCENT,
        );
    }

    fn draw_button_grid(&mut self, frame: &mut Frame<'_>, area: Rect) {
        let columns = grid_columns(area.width.saturating_sub(2), self.buttons.len());
        self.hit_boxes.button_columns = columns;
        let rows = if columns == 0 {
            0
        } else {
            self.buttons.len().div_ceil(columns)
        };
        let title = format!(
            "Custom buttons · layout hint {} columns x {} rows",
            columns, rows
        );
        let block = panel(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        self.hit_boxes.buttons.reserve(self.buttons.len());
        if self.buttons.is_empty() {
            frame.render_widget(
                Paragraph::new("No custom buttons. Choose Add function; = remains available.")
                    .alignment(Alignment::Center)
                    .style(Style::default().fg(DIM)),
                inner,
            );
            return;
        }

        let row_areas = Layout::default()
            .direction(Direction::Vertical)
            .constraints((0..rows).map(|_| Constraint::Length(3)))
            .split(inner);
        for (row_index, row_area) in row_areas.iter().enumerate() {
            let start = row_index * columns;
            let end = (start + columns).min(self.buttons.len());
            if start >= end {
                break;
            }
            let cells = Layout::default()
                .direction(Direction::Horizontal)
                .constraints((start..end).map(|_| Constraint::Ratio(1, columns as u32)))
                .split(*row_area);
            for (offset, operation) in self.buttons[start..end].iter().enumerate() {
                let index = start + offset;
                let cell = cells[offset];
                self.hit_boxes.buttons.push(cell);
                let selected = self.focus == Focus::Buttons && index == self.selected_button;
                draw_action(frame, cell, operation.spec().label, selected, BLUE);
            }
        }
    }

    fn draw_footer(&self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(
            Paragraph::new(
                "Mouse: click controls/buttons · Keyboard: Tab focus, arrows move, Enter activate, Del remove, Esc quit",
            )
            .block(panel("controls"))
            .style(Style::default().fg(DIM).bg(PANEL))
            .alignment(Alignment::Center),
            area,
        );
    }

    fn draw_dropdown(&mut self, frame: &mut Frame<'_>, screen: Rect) {
        let width = screen.width.saturating_sub(4).clamp(30, 76);
        let height = screen.height.saturating_sub(4).clamp(8, 24);
        let area = centered_rect(screen, width, height);
        self.hit_boxes.dropdown = area;
        frame.render_widget(Clear, area);

        let inner_height = height.saturating_sub(2) as usize;
        if self.dropdown_index < self.dropdown_offset {
            self.dropdown_offset = self.dropdown_index;
        }
        if self.dropdown_index >= self.dropdown_offset + inner_height {
            self.dropdown_offset = self.dropdown_index + 1 - inner_height;
        }
        let end = (self.dropdown_offset + inner_height).min(CALCULATOR_FUNCTIONS.len());
        let visible = &CALCULATOR_FUNCTIONS[self.dropdown_offset..end];
        let items = visible.iter().map(dropdown_item).collect::<Vec<_>>();
        let list = List::new(items)
            .block(panel(format!(
                "Add function · {} operations · Enter/click to add",
                CALCULATOR_FUNCTIONS.len()
            )))
            .style(Style::default().fg(TEXT).bg(PANEL))
            .highlight_style(Style::default().fg(Color::Black).bg(ACCENT))
            .highlight_symbol("› ");
        let mut state = ListState::default();
        state.select(Some(self.dropdown_index - self.dropdown_offset));
        frame.render_stateful_widget(list, area, &mut state);

        let inner = Rect::new(
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );
        self.hit_boxes.dropdown_items = (self.dropdown_offset..end)
            .enumerate()
            .map(|(row, index)| {
                (
                    index,
                    Rect::new(inner.x, inner.y + row as u16, inner.width, 1),
                )
            })
            .collect();
    }
}

fn parse_arguments(input: &str) -> Result<Vec<f64>, &'static str> {
    let values = input
        .split(|character: char| character == ',' || character == ';' || character.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Input contains a value that is not a Rust f64")?;
    if values.is_empty() {
        Err("Enter at least one numeric value")
    } else {
        Ok(values)
    }
}

fn format_value(value: f64) -> String {
    if !value.is_finite() || value == 0.0 {
        return value.to_string();
    }
    let magnitude = value.abs();
    if !(1e-9..1e12).contains(&magnitude) {
        format!("{value:.12e}")
    } else {
        let formatted = format!("{value:.12}");
        formatted
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

fn dropdown_item(spec: &CalculatorFunctionSpec) -> ListItem<'static> {
    ListItem::new(Line::from(vec![
        Span::styled(format!("{:<12}", spec.label), Style::default().fg(ACCENT)),
        Span::styled(format!("{:<22}", spec.name), Style::default().fg(TEXT)),
        Span::styled(
            format!("{} · {}", spec.category, spec.arguments),
            Style::default().fg(DIM),
        ),
    ]))
}

fn draw_action(
    frame: &mut Frame<'_>,
    area: Rect,
    label: impl Into<String>,
    selected: bool,
    accent: Color,
) {
    let style = if selected {
        Style::default()
            .fg(Color::Black)
            .bg(accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(TEXT).bg(PANEL_ALT)
    };
    frame.render_widget(
        Paragraph::new(label.into())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(accent)),
            )
            .alignment(Alignment::Center)
            .style(style),
        area,
    );
}

fn panel<'a>(title: impl Into<Line<'a>>) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(58, 76, 99)))
        .title(title)
}

fn grid_columns(width: u16, button_count: usize) -> usize {
    if button_count == 0 {
        return 0;
    }
    ((width as usize) / 12).clamp(1, 8).min(button_count)
}

const fn contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x && column < area.x + area.width && row >= area.y && row < area.y + area.height
}

fn centered_rect(screen: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        screen.x + screen.width.saturating_sub(width) / 2,
        screen.y + screen.height.saturating_sub(height) / 2,
        width.min(screen.width),
        height.min(screen.height),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    #[test]
    fn defaults_are_the_four_basic_operations() {
        let app = App::default();
        assert_eq!(
            app.buttons,
            [
                CalculatorOperation::Add,
                CalculatorOperation::Subtract,
                CalculatorOperation::Multiply,
                CalculatorOperation::Divide,
            ]
        );
    }

    #[test]
    fn parses_comma_space_and_semicolon_arguments() {
        assert_eq!(parse_arguments("1, 2; 3 4").unwrap(), [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn evaluate_button_cannot_be_removed() {
        let mut app = App::default();
        for _ in 0..4 {
            app.remove_selected_button();
        }
        assert!(app.buttons.is_empty());
        app.evaluate_selected();
        assert!(app.output.contains("Add and select"));
    }

    #[test]
    fn rendered_layout_records_mouse_targets() {
        let mut app = App::default();
        let backend = TestBackend::new(100, 32);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| app.draw(frame)).unwrap();
        assert_eq!(app.hit_boxes.buttons.len(), 4);
        assert!(app.hit_boxes.input.width > 0);
        assert!(app.hit_boxes.evaluate.width > 0);

        app.open_dropdown();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        assert!(!app.hit_boxes.dropdown_items.is_empty());
        assert!(app.hit_boxes.dropdown.width > 0);
    }

    #[test]
    fn selected_function_evaluates_through_the_shared_api() {
        let mut app = App {
            input: String::from("20, 22"),
            ..App::default()
        };
        app.evaluate_selected();
        assert!(app.output.ends_with("= 42"));
    }
}
