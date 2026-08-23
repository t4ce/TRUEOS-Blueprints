use std::{
    io::{self, Write},
    time::Duration,
};

use crossterm::{
    event::{
        self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    },
    style::Color,
    terminal,
};

use crate::{
    canvas::Canvas,
    screen::{clip_text_cells, text_cell_width, Frame, Renderer, Style},
};

const TICK: Duration = Duration::from_millis(16);
const MAX_EVENT_BATCH: usize = 64;
const MENU_WIDTH: u16 = 22;
const MIN_WIDTH: u16 = 72;
const MIN_HEIGHT: u16 = 20;
const CANVAS_WIDTH: usize = 96;
const CANVAS_HEIGHT: usize = 64;
const HISTORY_LIMIT: usize = 64;
const BUILD_ID: &str = "tpixel 0.1.0 ・ Braille canvas ・ RAM only";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Pencil,
    Eraser,
    Toggle,
}

impl Tool {
    const fn label(self) -> &'static str {
        match self {
            Self::Pencil => "pencil",
            Self::Eraser => "eraser",
            Self::Toggle => "toggle",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    Pencil,
    Eraser,
    Toggle,
    Brush,
    Undo,
    Redo,
    Invert,
    Clear,
    Demo,
    Help,
}

const COMMANDS: [(Command, &str); 10] = [
    (Command::Pencil, "pencil"),
    (Command::Eraser, "eraser"),
    (Command::Toggle, "toggle"),
    (Command::Brush, "brush size"),
    (Command::Undo, "undo"),
    (Command::Redo, "redo"),
    (Command::Invert, "invert"),
    (Command::Clear, "clear"),
    (Command::Demo, "demo art"),
    (Command::Help, "help"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    x: u16,
    y: u16,
    width: u16,
    height: u16,
}

impl Rect {
    fn right(self) -> u16 {
        self.x.saturating_add(self.width)
    }

    fn bottom(self) -> u16 {
        self.y.saturating_add(self.height)
    }

    fn contains(self, x: u16, y: u16) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    fn inset(self, amount: u16) -> Self {
        Self {
            x: self.x.saturating_add(amount),
            y: self.y.saturating_add(amount),
            width: self.width.saturating_sub(amount.saturating_mul(2)),
            height: self.height.saturating_sub(amount.saturating_mul(2)),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Geometry {
    canvas: Rect,
    menu: Rect,
    status_row: u16,
    hint_row: u16,
}

#[derive(Clone, Copy, Debug)]
struct HitRegion {
    rect: Rect,
    command: Command,
}

#[derive(Clone, Copy, Debug)]
enum StrokeMode {
    Tool(Tool),
    Erase,
}

#[derive(Clone, Copy, Debug)]
struct PanState {
    start_column: u16,
    start_row: u16,
    camera_x: i32,
    camera_y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Modal {
    ConfirmClear,
    Help,
}

pub struct App {
    canvas: Canvas,
    tool: Tool,
    brush: u8,
    cursor_x: i32,
    cursor_y: i32,
    camera_x: i32,
    camera_y: i32,
    history: Vec<Vec<bool>>,
    future: Vec<Vec<bool>>,
    modal: Option<Modal>,
    status: String,
    dirty: bool,
    should_exit: bool,
    hits: Vec<HitRegion>,
    stroke: Option<StrokeMode>,
    pan: Option<PanState>,
}

impl App {
    pub fn new(seed_demo: bool) -> Self {
        let canvas = Canvas::new(CANVAS_WIDTH, CANVAS_HEIGHT, seed_demo);
        let initial_status = if seed_demo {
            "demo art loaded in RAM".to_owned()
        } else {
            "blank 96×64 canvas created in RAM".to_owned()
        };
        let mut app = Self {
            canvas,
            tool: Tool::Pencil,
            brush: 1,
            cursor_x: (CANVAS_WIDTH / 2) as i32,
            cursor_y: (CANVAS_HEIGHT / 2) as i32,
            camera_x: 0,
            camera_y: 0,
            history: Vec::new(),
            future: Vec::new(),
            modal: None,
            status: initial_status.clone(),
            dirty: true,
            should_exit: false,
            hits: Vec::new(),
            stroke: None,
            pan: None,
        };
        app.center_current_terminal();
        app.status = initial_status;
        app
    }

    pub fn run<W: Write>(&mut self, out: &mut W, renderer: &mut Renderer) -> io::Result<()> {
        while !self.should_exit {
            if self.dirty {
                let (width, height) = terminal::size()?;
                let frame = self.draw(width, height);
                renderer.present(out, frame)?;
                self.dirty = false;
            }

            if event::poll(TICK)? {
                for batch_index in 0..MAX_EVENT_BATCH {
                    let terminal_event = event::read()?;
                    self.handle_event(terminal_event);
                    if self.should_exit || batch_index + 1 == MAX_EVENT_BATCH {
                        break;
                    }
                    if !event::poll(Duration::from_millis(0))? {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    fn handle_event(&mut self, terminal_event: Event) {
        if let Some(modal) = self.modal {
            self.handle_modal(modal, terminal_event);
            return;
        }

        match terminal_event {
            Event::Key(key) if is_action_key(key) => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Resize(_, _) => {
                self.keep_cursor_visible();
                self.dirty = true;
            }
            _ => {}
        }
    }

    fn handle_modal(&mut self, modal: Modal, terminal_event: Event) {
        let key = match terminal_event {
            Event::Key(key) => key,
            Event::Resize(_, _) => {
                self.dirty = true;
                return;
            }
            _ => return,
        };
        if !is_action_key(key) {
            return;
        }
        match modal {
            Modal::ConfirmClear => match key.code {
                KeyCode::Enter | KeyCode::Char('y' | 'Y') => {
                    self.checkpoint();
                    self.canvas.clear();
                    self.modal = None;
                    self.status = "canvas cleared".to_owned();
                    self.dirty = true;
                }
                KeyCode::Esc | KeyCode::Char('n' | 'N') => {
                    self.modal = None;
                    self.status = "clear cancelled".to_owned();
                    self.dirty = true;
                }
                _ => {}
            },
            Modal::Help => match key.code {
                KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q' | 'Q') => {
                    self.modal = None;
                    self.status = "help closed".to_owned();
                    self.dirty = true;
                }
                _ => {}
            },
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('q' | 'Q'))
        {
            self.should_exit = true;
            return;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Left => self.pan_by(-8, 0),
                KeyCode::Right => self.pan_by(8, 0),
                KeyCode::Up => self.pan_by(0, -8),
                KeyCode::Down => self.pan_by(0, 8),
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Left => self.move_cursor(-1, 0),
            KeyCode::Right => self.move_cursor(1, 0),
            KeyCode::Up => self.move_cursor(0, -1),
            KeyCode::Down => self.move_cursor(0, 1),
            KeyCode::Home => self.center_current_terminal(),
            KeyCode::Char(' ') => self.apply_keyboard_stamp(),
            KeyCode::Char('p' | 'P') => self.set_tool(Tool::Pencil),
            KeyCode::Char('e' | 'E') => self.set_tool(Tool::Eraser),
            KeyCode::Char('t' | 'T') => self.set_tool(Tool::Toggle),
            KeyCode::Char('[') => self.change_brush(-1),
            KeyCode::Char(']') => self.change_brush(1),
            KeyCode::Char('z' | 'Z') => self.undo(),
            KeyCode::Char('y' | 'Y') => self.redo(),
            KeyCode::Char('i' | 'I') => self.execute(Command::Invert),
            KeyCode::Char('c' | 'C') => self.execute(Command::Clear),
            KeyCode::Char('r' | 'R') => self.execute(Command::Demo),
            KeyCode::Char('?') | KeyCode::F(1) => self.execute(Command::Help),
            KeyCode::Esc => self.should_exit = true,
            KeyCode::Char(ch) if ch.is_ascii_digit() => {
                if let Some(index) = ch.to_digit(10).map(|value| value as usize)
                    && let Some((command, _)) = COMMANDS.get(index)
                {
                    self.execute(*command);
                }
            }
            _ => {}
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(command) = self.hit_command(mouse.column, mouse.row) {
                    self.execute(command);
                    return;
                }
                if let Some((x, y)) = self.pixel_at(mouse.column, mouse.row) {
                    self.cursor_x = x;
                    self.cursor_y = y;
                    self.checkpoint();
                    let mode = StrokeMode::Tool(self.tool);
                    self.stroke = Some(mode);
                    self.apply_mode(mode, x, y);
                    self.dirty = true;
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                if let Some((x, y)) = self.pixel_at(mouse.column, mouse.row) {
                    self.cursor_x = x;
                    self.cursor_y = y;
                    self.checkpoint();
                    self.stroke = Some(StrokeMode::Erase);
                    self.apply_mode(StrokeMode::Erase, x, y);
                    self.dirty = true;
                }
            }
            MouseEventKind::Down(MouseButton::Middle) => {
                self.pan = Some(PanState {
                    start_column: mouse.column,
                    start_row: mouse.row,
                    camera_x: self.camera_x,
                    camera_y: self.camera_y,
                });
            }
            MouseEventKind::Drag(MouseButton::Left)
            | MouseEventKind::Drag(MouseButton::Right) => {
                if let Some(mode) = self.stroke
                    && !matches!(mode, StrokeMode::Tool(Tool::Toggle))
                    && let Some((x, y)) = self.pixel_at(mouse.column, mouse.row)
                {
                    self.cursor_x = x;
                    self.cursor_y = y;
                    self.apply_mode(mode, x, y);
                    self.dirty = true;
                }
            }
            MouseEventKind::Drag(MouseButton::Middle) => {
                if let Some(pan) = self.pan {
                    let dx = mouse.column as i32 - pan.start_column as i32;
                    let dy = mouse.row as i32 - pan.start_row as i32;
                    self.camera_x = pan.camera_x - dx * 2;
                    self.camera_y = pan.camera_y - dy * 4;
                    self.dirty = true;
                }
            }
            MouseEventKind::Up(_) => {
                self.stroke = None;
                self.pan = None;
            }
            MouseEventKind::ScrollUp => self.change_brush(1),
            MouseEventKind::ScrollDown => self.change_brush(-1),
            _ => {}
        }
    }

    fn execute(&mut self, command: Command) {
        match command {
            Command::Pencil => self.set_tool(Tool::Pencil),
            Command::Eraser => self.set_tool(Tool::Eraser),
            Command::Toggle => self.set_tool(Tool::Toggle),
            Command::Brush => self.change_brush(1),
            Command::Undo => self.undo(),
            Command::Redo => self.redo(),
            Command::Invert => {
                self.checkpoint();
                self.canvas.invert();
                self.status = "canvas inverted".to_owned();
                self.dirty = true;
            }
            Command::Clear => {
                self.modal = Some(Modal::ConfirmClear);
                self.dirty = true;
            }
            Command::Demo => {
                self.checkpoint();
                self.canvas.seed_demo();
                self.status = "demo art stamped".to_owned();
                self.dirty = true;
            }
            Command::Help => {
                self.modal = Some(Modal::Help);
                self.dirty = true;
            }
        }
    }

    fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.status = format!("{} selected", tool.label());
        self.dirty = true;
    }

    fn change_brush(&mut self, delta: i8) {
        let next = (self.brush as i8 + delta).clamp(1, 4) as u8;
        self.brush = next;
        self.status = format!("brush {}×{}", self.brush, self.brush);
        self.dirty = true;
    }

    fn move_cursor(&mut self, dx: i32, dy: i32) {
        self.cursor_x = (self.cursor_x + dx).clamp(0, self.canvas.width() as i32 - 1);
        self.cursor_y = (self.cursor_y + dy).clamp(0, self.canvas.height() as i32 - 1);
        self.keep_cursor_visible();
        self.dirty = true;
    }

    fn pan_by(&mut self, dx: i32, dy: i32) {
        self.camera_x += dx;
        self.camera_y += dy;
        self.status = format!("view {},{}", self.camera_x, self.camera_y);
        self.dirty = true;
    }

    fn apply_keyboard_stamp(&mut self) {
        self.checkpoint();
        self.apply_mode(StrokeMode::Tool(self.tool), self.cursor_x, self.cursor_y);
        self.status = format!("{} at {},{}", self.tool.label(), self.cursor_x, self.cursor_y);
        self.dirty = true;
    }

    fn apply_mode(&mut self, mode: StrokeMode, x: i32, y: i32) {
        let size = self.brush as i32;
        let offset = (size - 1) / 2;
        for dy in 0..size {
            for dx in 0..size {
                let px = x + dx - offset;
                let py = y + dy - offset;
                match mode {
                    StrokeMode::Tool(Tool::Pencil) => {
                        self.canvas.set(px, py, true);
                    }
                    StrokeMode::Tool(Tool::Eraser) | StrokeMode::Erase => {
                        self.canvas.set(px, py, false);
                    }
                    StrokeMode::Tool(Tool::Toggle) => {
                        self.canvas.toggle(px, py);
                    }
                }
            }
        }
    }

    fn checkpoint(&mut self) {
        self.history.push(self.canvas.snapshot());
        if self.history.len() > HISTORY_LIMIT {
            self.history.remove(0);
        }
        self.future.clear();
    }

    fn undo(&mut self) {
        let Some(previous) = self.history.pop() else {
            self.status = "nothing to undo".to_owned();
            self.dirty = true;
            return;
        };
        self.future.push(self.canvas.snapshot());
        self.canvas.restore(previous);
        self.status = "undo".to_owned();
        self.dirty = true;
    }

    fn redo(&mut self) {
        let Some(next) = self.future.pop() else {
            self.status = "nothing to redo".to_owned();
            self.dirty = true;
            return;
        };
        self.history.push(self.canvas.snapshot());
        self.canvas.restore(next);
        self.status = "redo".to_owned();
        self.dirty = true;
    }

    fn center_current_terminal(&mut self) {
        let Ok((width, height)) = terminal::size() else {
            return;
        };
        let Some(geometry) = geometry(width, height) else {
            return;
        };
        let inner = geometry.canvas.inset(1);
        let visible_width = inner.width as i32 * 2;
        let visible_height = inner.height as i32 * 4;
        self.camera_x = (self.canvas.width() as i32 - visible_width) / 2;
        self.camera_y = (self.canvas.height() as i32 - visible_height) / 2;
        self.status = "view centered".to_owned();
        self.dirty = true;
    }

    fn keep_cursor_visible(&mut self) {
        let Ok((width, height)) = terminal::size() else {
            return;
        };
        let Some(geometry) = geometry(width, height) else {
            return;
        };
        let inner = geometry.canvas.inset(1);
        let visible_width = (inner.width as i32 * 2).max(1);
        let visible_height = (inner.height as i32 * 4).max(1);
        if self.cursor_x < self.camera_x {
            self.camera_x = self.cursor_x;
        } else if self.cursor_x >= self.camera_x + visible_width {
            self.camera_x = self.cursor_x - visible_width + 1;
        }
        if self.cursor_y < self.camera_y {
            self.camera_y = self.cursor_y;
        } else if self.cursor_y >= self.camera_y + visible_height {
            self.camera_y = self.cursor_y - visible_height + 1;
        }
    }

    fn pixel_at(&self, column: u16, row: u16) -> Option<(i32, i32)> {
        let (width, height) = terminal::size().ok()?;
        let geometry = geometry(width, height)?;
        let inner = geometry.canvas.inset(1);
        if !inner.contains(column, row) {
            return None;
        }
        let sub_x = (self.cursor_x - self.camera_x).rem_euclid(2);
        let sub_y = (self.cursor_y - self.camera_y).rem_euclid(4);
        let x = self.camera_x + (column - inner.x) as i32 * 2 + sub_x;
        let y = self.camera_y + (row - inner.y) as i32 * 4 + sub_y;
        if x < 0
            || y < 0
            || x >= self.canvas.width() as i32
            || y >= self.canvas.height() as i32
        {
            None
        } else {
            Some((x, y))
        }
    }

    fn hit_command(&self, x: u16, y: u16) -> Option<Command> {
        self.hits
            .iter()
            .rev()
            .find(|hit| hit.rect.contains(x, y))
            .map(|hit| hit.command)
    }

    fn draw(&mut self, width: u16, height: u16) -> Frame {
        let mut frame = Frame::new(width, height);
        let mut hits = Vec::new();
        let Some(geometry) = geometry(width, height) else {
            draw_too_small(&mut frame, width, height);
            self.hits.clear();
            return frame;
        };

        draw_title(&mut frame, width);
        draw_canvas(
            &mut frame,
            geometry.canvas,
            &self.canvas,
            self.camera_x,
            self.camera_y,
            self.cursor_x,
            self.cursor_y,
        );
        draw_menu(
            &mut frame,
            geometry.menu,
            self.tool,
            self.brush,
            self.history.len(),
            self.future.len(),
            &mut hits,
        );
        draw_status(
            &mut frame,
            geometry.status_row,
            width,
            &self.status,
            self.tool,
            self.brush,
            self.cursor_x,
            self.cursor_y,
            self.canvas.count_lit(),
        );
        frame.put_str(
            1,
            geometry.hint_row,
            "arrows move dot ・ Space paint ・ LMB draw ・ RMB erase ・ MMB pan ・ [ ] brush ・ ? help",
            Style::new(Color::DarkGrey, Color::Reset),
        );

        if let Some(modal) = self.modal {
            draw_modal(&mut frame, modal);
        }
        self.hits = hits;
        frame
    }
}

fn geometry(width: u16, height: u16) -> Option<Geometry> {
    if width < MIN_WIDTH || height < MIN_HEIGHT {
        return None;
    }
    let menu = Rect {
        x: width.saturating_sub(MENU_WIDTH + 1),
        y: 1,
        width: MENU_WIDTH,
        height: height.saturating_sub(4),
    };
    let canvas = Rect {
        x: 1,
        y: 2,
        width: menu.x.saturating_sub(2),
        height: height.saturating_sub(6),
    };
    Some(Geometry {
        canvas,
        menu,
        status_row: height - 3,
        hint_row: height - 2,
    })
}

#[allow(clippy::too_many_arguments)]
fn draw_canvas(
    frame: &mut Frame,
    rect: Rect,
    canvas: &Canvas,
    camera_x: i32,
    camera_y: i32,
    cursor_x: i32,
    cursor_y: i32,
) {
    draw_box(
        frame,
        rect,
        Style::new(Color::DarkGrey, Color::Reset),
    );
    let title = format!("96×64 ・ view {camera_x},{camera_y}");
    frame.put_str(
        rect.x + 2,
        rect.y,
        &clip_text_cells(&title, rect.width.saturating_sub(4) as usize),
        Style::new(Color::Cyan, Color::Reset).bold(),
    );
    let inner = rect.inset(1);
    for row in 0..inner.height {
        for column in 0..inner.width {
            let base_x = camera_x + column as i32 * 2;
            let base_y = camera_y + row as i32 * 4;
            let mut bits = 0u32;
            for dy in 0..4 {
                for dx in 0..2 {
                    if canvas.get(base_x + dx, base_y + dy) {
                        bits |= braille_bit(dx as usize, dy as usize);
                    }
                }
            }
            let within = base_x < canvas.width() as i32
                && base_y < canvas.height() as i32
                && base_x + 1 >= 0
                && base_y + 3 >= 0;
            let cursor_here = cursor_x >= base_x
                && cursor_x < base_x + 2
                && cursor_y >= base_y
                && cursor_y < base_y + 4;
            let ch = char::from_u32(0x2800 + bits).unwrap_or(' ');
            let style = if cursor_here {
                Style::new(Color::Black, Color::Cyan).bold()
            } else if within {
                Style::new(Color::White, Color::Black)
            } else {
                Style::new(Color::DarkGrey, Color::Black).dim()
            };
            frame.put(inner.x + column, inner.y + row, ch, style);
        }
    }
}

fn braille_bit(dx: usize, dy: usize) -> u32 {
    const BITS: [[u32; 2]; 4] = [
        [1 << 0, 1 << 3],
        [1 << 1, 1 << 4],
        [1 << 2, 1 << 5],
        [1 << 6, 1 << 7],
    ];
    BITS[dy][dx]
}

fn draw_menu(
    frame: &mut Frame,
    rect: Rect,
    tool: Tool,
    brush: u8,
    undo: usize,
    redo: usize,
    hits: &mut Vec<HitRegion>,
) {
    draw_box(
        frame,
        rect,
        Style::new(Color::DarkGrey, Color::Reset),
    );
    frame.put_str(
        rect.x + 2,
        rect.y,
        "☰ PIXEL",
        Style::new(Color::Yellow, Color::Reset).bold(),
    );
    for (index, (command, label)) in COMMANDS.iter().enumerate() {
        let row = rect.y + 2 + index as u16;
        if row >= rect.bottom().saturating_sub(1) {
            break;
        }
        let active = matches!(
            (*command, tool),
            (Command::Pencil, Tool::Pencil)
                | (Command::Eraser, Tool::Eraser)
                | (Command::Toggle, Tool::Toggle)
        );
        let suffix = match command {
            Command::Brush => format!(" {brush}"),
            Command::Undo => format!(" {undo}"),
            Command::Redo => format!(" {redo}"),
            _ => String::new(),
        };
        let line = format!("{index}  {label}{suffix}");
        frame.put_str(
            rect.x + 2,
            row,
            &clip_text_cells(&line, rect.width.saturating_sub(4) as usize),
            if active {
                Style::new(Color::Black, Color::Cyan).bold()
            } else {
                Style::new(Color::White, Color::Reset)
            },
        );
        hits.push(HitRegion {
            rect: Rect {
                x: rect.x + 1,
                y: row,
                width: rect.width.saturating_sub(2),
                height: 1,
            },
            command: *command,
        });
    }
    if rect.height > 15 {
        frame.put_str(
            rect.x + 2,
            rect.bottom().saturating_sub(3),
            "Esc / Ctrl-Q exits",
            Style::new(Color::DarkGrey, Color::Reset),
        );
    }
}

fn draw_title(frame: &mut Frame, width: u16) {
    frame.put_str(
        2,
        0,
        BUILD_ID,
        Style::new(Color::White, Color::Reset).bold(),
    );
    let right = "2×4 pixels per terminal cell";
    let right_width = text_cell_width(right) as u16;
    let left_width = text_cell_width(BUILD_ID) as u16;
    if width > left_width.saturating_add(right_width).saturating_add(6) {
        frame.put_str(
            width - right_width - 1,
            0,
            right,
            Style::new(Color::DarkGrey, Color::Reset),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_status(
    frame: &mut Frame,
    row: u16,
    width: u16,
    status: &str,
    tool: Tool,
    brush: u8,
    cursor_x: i32,
    cursor_y: i32,
    lit: usize,
) {
    frame.fill_rect(0, row, width, 1, ' ', Style::new(Color::Black, Color::White));
    let left = format!(" {status}");
    frame.put_str(
        0,
        row,
        &clip_text_cells(&left, width.saturating_sub(2) as usize),
        Style::new(Color::Black, Color::White),
    );
    let right = format!(" {} ・ {}×{} ・ {},{} ・ {} lit ", tool.label(), brush, brush, cursor_x, cursor_y, lit);
    let right_width = text_cell_width(&right) as u16;
    if right_width < width {
        frame.put_str(
            width - right_width,
            row,
            &right,
            Style::new(Color::Black, Color::White).bold(),
        );
    }
}

fn draw_modal(frame: &mut Frame, modal: Modal) {
    let width = frame.width();
    let height = frame.height();
    let modal_width = width.saturating_sub(10).min(68).max(38);
    let modal_height = match modal {
        Modal::ConfirmClear => 8,
        Modal::Help => 15,
    }
    .min(height.saturating_sub(4));
    let rect = Rect {
        x: width.saturating_sub(modal_width) / 2,
        y: height.saturating_sub(modal_height) / 2,
        width: modal_width,
        height: modal_height,
    };
    let base = Style::new(Color::White, Color::DarkBlue);
    frame.fill_rect(rect.x, rect.y, rect.width, rect.height, ' ', base);
    draw_box(frame, rect, Style::new(Color::Cyan, Color::DarkBlue).bold());
    match modal {
        Modal::ConfirmClear => {
            frame.put_str(rect.x + 2, rect.y, "clear canvas", base.bold());
            frame.put_str(
                rect.x + 2,
                rect.y + 3,
                "clear all 6,144 in-memory pixels?",
                base,
            );
            frame.put_str(
                rect.x + 2,
                rect.bottom().saturating_sub(2),
                "Y / Enter confirm ・ N / Esc cancel",
                base.dim(),
            );
        }
        Modal::Help => {
            frame.put_str(rect.x + 2, rect.y, "tpixel controls", base.bold());
            let lines = [
                "Arrow keys         move the exact logical pixel",
                "Space              apply the current tool",
                "P / E / T          pencil / eraser / toggle",
                "[ / ] or wheel     change brush size",
                "Z / Y              undo / redo",
                "I / C / R          invert / clear / demo art",
                "Left drag          paint with current tool",
                "Right drag         erase",
                "Middle drag        pan the Braille viewport",
                "Ctrl+arrows        pan by eight logical pixels",
                "Home               center the canvas",
                "0..9               run a menu command",
            ];
            for (index, line) in lines.iter().enumerate() {
                let row = rect.y + 2 + index as u16;
                if row < rect.bottom().saturating_sub(2) {
                    frame.put_str(rect.x + 2, row, line, base);
                }
            }
            frame.put_str(
                rect.x + 2,
                rect.bottom().saturating_sub(2),
                "Enter / Esc closes help",
                base.dim(),
            );
        }
    }
}

fn draw_box(frame: &mut Frame, rect: Rect, style: Style) {
    if rect.width < 2 || rect.height < 2 {
        return;
    }
    frame.put(rect.x, rect.y, '┌', style);
    frame.put(rect.right() - 1, rect.y, '┐', style);
    frame.put(rect.x, rect.bottom() - 1, '└', style);
    frame.put(rect.right() - 1, rect.bottom() - 1, '┘', style);
    frame.hline_i32(
        rect.x as i32 + 1,
        rect.y as i32,
        rect.width as i32 - 2,
        '─',
        style,
    );
    frame.hline_i32(
        rect.x as i32 + 1,
        rect.bottom() as i32 - 1,
        rect.width as i32 - 2,
        '─',
        style,
    );
    frame.vline_i32(
        rect.x as i32,
        rect.y as i32 + 1,
        rect.height as i32 - 2,
        '│',
        style,
    );
    frame.vline_i32(
        rect.right() as i32 - 1,
        rect.y as i32 + 1,
        rect.height as i32 - 2,
        '│',
        style,
    );
}

fn draw_too_small(frame: &mut Frame, width: u16, height: u16) {
    let message = format!("tpixel needs at least {MIN_WIDTH}×{MIN_HEIGHT}; terminal is {width}×{height}");
    let x = width.saturating_sub(text_cell_width(&message) as u16) / 2;
    let y = height / 2;
    frame.put_str(
        x,
        y,
        &message,
        Style::new(Color::Yellow, Color::Reset).bold(),
    );
}

fn is_action_key(key: KeyEvent) -> bool {
    matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
}
