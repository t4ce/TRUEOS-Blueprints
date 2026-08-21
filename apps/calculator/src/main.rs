use std::io::{self, BufWriter, Write};
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    cursor::{Hide, Show},
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
        MouseButton, MouseEvent, MouseEventKind,
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
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
use trueos::calculator_base::{
    CALCULATOR_FUNCTIONS, CALCULATOR_PROTOCOL_VERSION, CalculatorOperation, evaluate,
};
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
use trueos_math::calculator_base::{
    CALCULATOR_FUNCTIONS, CALCULATOR_PROTOCOL_VERSION, CalculatorOperation,
    evaluate_operation as evaluate,
};

const MAX_BUTTONS: usize = 32;
const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(16);
const FRAME_BUFFER_CAPACITY: usize = 128 * 1024;
const BG: Color = Color::Rgb { r: 8, g: 11, b: 18 };
const PANEL: Color = Color::Rgb {
    r: 18,
    g: 24,
    b: 35,
};
const PANEL_ALT: Color = Color::Rgb {
    r: 25,
    g: 34,
    b: 48,
};
const TEXT: Color = Color::Rgb {
    r: 229,
    g: 235,
    b: 244,
};
const DIM: Color = Color::Rgb {
    r: 132,
    g: 147,
    b: 168,
};
const ACCENT: Color = Color::Rgb {
    r: 103,
    g: 211,
    b: 189,
};
const BLUE: Color = Color::Rgb {
    r: 111,
    g: 169,
    b: 242,
};
const WARN: Color = Color::Rgb {
    r: 247,
    g: 190,
    b: 94,
};
const ERROR: Color = Color::Rgb {
    r: 247,
    g: 117,
    b: 131,
};

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

type CalculatorTerminal = BufWriter<io::Stdout>;

fn setup_terminal() -> Result<CalculatorTerminal> {
    enable_raw_mode()?;
    // Buffer a complete frame so TRUEOS only crosses the VM console ABI once.
    let mut stdout = BufWriter::with_capacity(FRAME_BUFFER_CAPACITY, io::stdout());
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture, Hide)?;
    Ok(stdout)
}

fn restore_terminal(terminal: &mut CalculatorTerminal) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal,
        DisableMouseCapture,
        Show,
        ResetColor,
        LeaveAlternateScreen
    )?;
    Ok(())
}

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
        self.draw(terminal)?;

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
                self.draw(terminal)?;
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
}

impl App {
    fn draw(&mut self, out: &mut impl Write) -> io::Result<()> {
        let (width, height) = terminal::size()?;
        self.draw_sized(out, width, height)
    }

    fn draw_sized(&mut self, out: &mut impl Write, width: u16, height: u16) -> io::Result<()> {
        self.hit_boxes = HitBoxes::default();
        let screen = Rect::new(0, 0, width, height);
        queue!(out, SetBackgroundColor(BG), Clear(ClearType::All), Hide)?;

        let header = Rect::new(0, 0, width, height.min(3));
        let io_area = Rect::new(0, 3.min(height), width, height.saturating_sub(3).min(7));
        let controls = Rect::new(0, 10.min(height), width, height.saturating_sub(10).min(3));
        let footer_height = height.saturating_sub(13).min(3);
        let footer = Rect::new(
            0,
            height.saturating_sub(footer_height),
            width,
            footer_height,
        );
        let grid_y = 13.min(height);
        let grid = Rect::new(0, grid_y, width, footer.y.saturating_sub(grid_y));

        self.draw_header(out, header)?;
        self.draw_io(out, io_area)?;
        self.draw_controls(out, controls)?;
        self.draw_button_grid(out, grid)?;
        self.draw_footer(out, footer)?;
        if self.dropdown_open {
            self.draw_dropdown(out, screen)?;
        } else if self.focus == Focus::Input {
            let inner = self.hit_boxes.input.inner();
            let shown_len = self.input.chars().count().min(inner.width as usize) as u16;
            queue!(
                out,
                crossterm::cursor::MoveTo(inner.x + shown_len, inner.y),
                Show
            )?;
        }
        queue!(out, ResetColor, SetAttribute(Attribute::Reset))?;
        out.flush()
    }

    fn draw_header(&self, out: &mut impl Write, area: Rect) -> io::Result<()> {
        draw_panel(out, area, "custom calculator", PANEL)?;
        let selected = self
            .selected_operation()
            .map(|operation| operation.spec().name)
            .unwrap_or("none");
        let text = format!(
            "TRUEOS Calculator  protocol v{}  selected: {}  pad: {}/{}",
            CALCULATOR_PROTOCOL_VERSION,
            selected,
            self.buttons.len(),
            MAX_BUTTONS
        );
        write_at(
            out,
            area.x + 1,
            area.y + 1,
            area.width.saturating_sub(2),
            &text,
            ACCENT,
            PANEL,
            true,
        )
    }

    fn draw_io(&mut self, out: &mut impl Write, area: Rect) -> io::Result<()> {
        let input = Rect::new(area.x, area.y, area.width, area.height.min(3));
        let output = Rect::new(
            area.x,
            area.y.saturating_add(input.height),
            area.width,
            area.height.saturating_sub(input.height),
        );
        self.hit_boxes.input = input;
        let (input_fg, input_bg) = if self.focus == Focus::Input {
            (Color::Black, ACCENT)
        } else {
            (TEXT, PANEL_ALT)
        };
        draw_panel(out, input, "Input · comma or space separated", input_bg)?;
        let input_width = input.width.saturating_sub(2) as usize;
        let shown = tail_chars(&self.input, input_width);
        write_at(
            out,
            input.x + 1,
            input.y + 1,
            input.width.saturating_sub(2),
            &shown,
            input_fg,
            input_bg,
            false,
        )?;

        draw_panel(out, output, "Output", PANEL)?;
        let color = if self.status_is_error { ERROR } else { TEXT };
        draw_wrapped(out, output.inner(), &self.output, color, PANEL)
    }

    fn draw_controls(&mut self, out: &mut impl Write, area: Rect) -> io::Result<()> {
        let first = area.width.saturating_mul(30) / 100;
        let second = area.width.saturating_mul(30) / 100;
        let controls = [
            Rect::new(area.x, area.y, first, area.height),
            Rect::new(area.x + first, area.y, second, area.height),
            Rect::new(
                area.x + first + second,
                area.y,
                area.width.saturating_sub(first + second),
                area.height,
            ),
        ];
        self.hit_boxes.add = controls[0];
        self.hit_boxes.remove = controls[1];
        self.hit_boxes.evaluate = controls[2];
        draw_action(
            out,
            controls[0],
            "+ Add function",
            self.focus == Focus::Add,
            BLUE,
        )?;
        draw_action(
            out,
            controls[1],
            "- Remove selected",
            self.focus == Focus::Remove,
            WARN,
        )?;
        draw_action(
            out,
            controls[2],
            "= Evaluate",
            self.focus == Focus::Evaluate,
            ACCENT,
        )
    }

    fn draw_button_grid(&mut self, out: &mut impl Write, area: Rect) -> io::Result<()> {
        let columns = grid_columns(area.width.saturating_sub(2), self.buttons.len());
        self.hit_boxes.button_columns = columns;
        let rows = if columns == 0 {
            0
        } else {
            self.buttons.len().div_ceil(columns)
        };
        draw_panel(
            out,
            area,
            &format!("Custom buttons · layout hint {columns} columns x {rows} rows"),
            PANEL,
        )?;
        let inner = area.inner();
        self.hit_boxes.buttons.reserve(self.buttons.len());
        if self.buttons.is_empty() {
            return write_centered(
                out,
                inner,
                "No custom buttons. Choose Add function; = remains available.",
                DIM,
                PANEL,
                false,
            );
        }

        for row in 0..rows {
            let start = row * columns;
            let end = (start + columns).min(self.buttons.len());
            let cell_width = inner.width / columns as u16;
            for (offset, operation) in self.buttons[start..end].iter().enumerate() {
                let index = start + offset;
                let x = inner.x + offset as u16 * cell_width;
                let width = if offset + 1 == columns || index + 1 == self.buttons.len() {
                    inner.x + inner.width - x
                } else {
                    cell_width
                };
                let row_y = inner.y + row as u16 * 3;
                let cell = Rect::new(
                    x,
                    row_y,
                    width,
                    3.min(inner.y.saturating_add(inner.height).saturating_sub(row_y)),
                );
                self.hit_boxes.buttons.push(cell);
                draw_action(
                    out,
                    cell,
                    operation.spec().label,
                    self.focus == Focus::Buttons && index == self.selected_button,
                    BLUE,
                )?;
            }
        }
        Ok(())
    }

    fn draw_footer(&self, out: &mut impl Write, area: Rect) -> io::Result<()> {
        draw_panel(out, area, "controls", PANEL)?;
        write_centered(
            out,
            area.inner(),
            "Mouse: click controls/buttons · Keyboard: Tab focus, arrows move, Enter activate, Del remove, Esc quit",
            DIM,
            PANEL,
            false,
        )
    }

    fn draw_dropdown(&mut self, out: &mut impl Write, screen: Rect) -> io::Result<()> {
        let width = screen
            .width
            .saturating_sub(4)
            .clamp(30, 76)
            .min(screen.width);
        let height = screen
            .height
            .saturating_sub(4)
            .clamp(8, 24)
            .min(screen.height);
        let area = centered_rect(screen, width, height);
        self.hit_boxes.dropdown = area;
        draw_panel(
            out,
            area,
            &format!(
                "Add function · {} operations · Enter/click to add",
                CALCULATOR_FUNCTIONS.len()
            ),
            PANEL,
        )?;

        let inner = area.inner();
        let inner_height = inner.height as usize;
        if self.dropdown_index < self.dropdown_offset {
            self.dropdown_offset = self.dropdown_index;
        }
        if inner_height > 0 && self.dropdown_index >= self.dropdown_offset + inner_height {
            self.dropdown_offset = self.dropdown_index + 1 - inner_height;
        }
        let end = (self.dropdown_offset + inner_height).min(CALCULATOR_FUNCTIONS.len());
        self.hit_boxes.dropdown_items.clear();
        for (row, index) in (self.dropdown_offset..end).enumerate() {
            let spec = &CALCULATOR_FUNCTIONS[index];
            let selected = index == self.dropdown_index;
            let fg = if selected { Color::Black } else { TEXT };
            let bg = if selected { ACCENT } else { PANEL };
            let line = format!(
                "{} {:<12} {:<22} {} · {}",
                if selected { '›' } else { ' ' },
                spec.label,
                spec.name,
                spec.category,
                spec.arguments
            );
            let item = Rect::new(inner.x, inner.y + row as u16, inner.width, 1);
            fill_rect(out, item, bg)?;
            write_at(out, item.x, item.y, item.width, &line, fg, bg, selected)?;
            self.hit_boxes.dropdown_items.push((index, item));
        }
        Ok(())
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

fn draw_action(
    out: &mut impl Write,
    area: Rect,
    label: &str,
    selected: bool,
    accent: Color,
) -> io::Result<()> {
    let (fg, bg) = if selected {
        (Color::Black, accent)
    } else {
        (TEXT, PANEL_ALT)
    };
    draw_box(out, area, accent, bg)?;
    write_centered(out, area.inner(), label, fg, bg, selected)
}

fn draw_panel(out: &mut impl Write, area: Rect, title: &str, background: Color) -> io::Result<()> {
    draw_box(
        out,
        area,
        Color::Rgb {
            r: 58,
            g: 76,
            b: 99,
        },
        background,
    )?;
    if area.width > 4 && area.height > 0 {
        write_at(
            out,
            area.x + 2,
            area.y,
            area.width.saturating_sub(4),
            title,
            DIM,
            background,
            false,
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
        crossterm::cursor::MoveTo(area.x, area.y),
        Print("┌"),
        Print(&horizontal),
        Print("┐"),
        crossterm::cursor::MoveTo(area.x, area.y + area.height - 1),
        Print("└"),
        Print(&horizontal),
        Print("┘")
    )?;
    for row in area.y + 1..area.y + area.height - 1 {
        queue!(
            out,
            crossterm::cursor::MoveTo(area.x, row),
            Print("│"),
            crossterm::cursor::MoveTo(area.x + area.width - 1, row),
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
        queue!(out, crossterm::cursor::MoveTo(area.x, row), Print(&blank))?;
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
        crossterm::cursor::MoveTo(x, y),
        Print(clipped),
        SetAttribute(Attribute::NormalIntensity)
    )
}

fn write_centered(
    out: &mut impl Write,
    area: Rect,
    text: &str,
    foreground: Color,
    background: Color,
    bold: bool,
) -> io::Result<()> {
    if area.width == 0 || area.height == 0 {
        return Ok(());
    }
    let len = text.chars().count().min(area.width as usize) as u16;
    write_at(
        out,
        area.x + area.width.saturating_sub(len) / 2,
        area.y + area.height.saturating_sub(1) / 2,
        len,
        text,
        foreground,
        background,
        bold,
    )
}

fn draw_wrapped(
    out: &mut impl Write,
    area: Rect,
    text: &str,
    foreground: Color,
    background: Color,
) -> io::Result<()> {
    if area.width == 0 || area.height == 0 {
        return Ok(());
    }
    let mut words = text.split_whitespace().peekable();
    for row in 0..area.height {
        let mut line = String::new();
        while let Some(word) = words.peek().copied() {
            let separator = usize::from(!line.is_empty());
            if line.chars().count() + separator + word.chars().count() > area.width as usize {
                break;
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
            words.next();
        }
        if line.is_empty() {
            if let Some(word) = words.next() {
                line.extend(word.chars().take(area.width as usize));
            }
        }
        write_at(
            out,
            area.x,
            area.y + row,
            area.width,
            &line,
            foreground,
            background,
            false,
        )?;
        if words.peek().is_none() {
            break;
        }
    }
    Ok(())
}

fn tail_chars(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    text.chars().skip(count.saturating_sub(limit)).collect()
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
        let mut output = Vec::new();

        app.draw_sized(&mut output, 100, 32).unwrap();
        assert_eq!(app.hit_boxes.buttons.len(), 4);
        assert!(app.hit_boxes.input.width > 0);
        assert!(app.hit_boxes.evaluate.width > 0);
        assert!(!output.is_empty());

        app.open_dropdown();
        app.draw_sized(&mut output, 100, 32).unwrap();
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
