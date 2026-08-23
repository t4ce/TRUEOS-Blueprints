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
    modal::{ConfirmModal, ConfirmPurpose, InputModal, InputPurpose, Modal, ModalOutcome},
    model::{Board, Card, Lane},
    screen::{clip_text_cells, text_cell_width, Frame, Renderer, Style},
};

const TICK: Duration = Duration::from_millis(16);
const MAX_EVENT_BATCH: usize = 64;
const MENU_WIDTH: u16 = 22;
const MIN_WIDTH: u16 = 72;
const MIN_HEIGHT: u16 = 20;
const CARD_HEIGHT: u16 = 4;
const BUILD_ID: &str = "tboard 0.1.0 ・ RAM board ・ direct crossterm";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    New,
    Edit,
    Detail,
    MoveLeft,
    MoveRight,
    Delete,
    Demo,
    Empty,
    Help,
    Exit,
}

const COMMANDS: [(Command, &str); 10] = [
    (Command::New, "new card"),
    (Command::Edit, "edit title"),
    (Command::Detail, "edit detail"),
    (Command::MoveLeft, "move  ←"),
    (Command::MoveRight, "move  →"),
    (Command::Delete, "delete"),
    (Command::Demo, "demo board"),
    (Command::Empty, "empty board"),
    (Command::Help, "help"),
    (Command::Exit, "exit"),
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
}

#[derive(Clone, Copy, Debug)]
struct Geometry {
    board: Rect,
    menu: Rect,
    status_row: u16,
    hint_row: u16,
}

#[derive(Clone, Copy, Debug)]
enum HitTarget {
    Lane(Lane),
    Card { id: u64, lane: Lane },
    Command(Command),
}

#[derive(Clone, Copy, Debug)]
struct HitRegion {
    rect: Rect,
    target: HitTarget,
}

#[derive(Clone, Copy, Debug)]
struct DragState {
    id: u64,
    origin: Lane,
    target: Lane,
}

pub struct App {
    board: Board,
    selected_lane: Lane,
    selected_card: Option<u64>,
    scroll: [usize; 3],
    modal: Option<Modal>,
    status: String,
    dirty: bool,
    should_exit: bool,
    hits: Vec<HitRegion>,
    drag: Option<DragState>,
}

impl App {
    pub fn new(seed_demo: bool) -> Self {
        let board = Board::new(seed_demo);
        let selected_lane = Lane::Ideas;
        let selected_card = board.first_id(selected_lane);
        Self {
            board,
            selected_lane,
            selected_card,
            scroll: [0; 3],
            modal: None,
            status: if seed_demo {
                "demo board created in RAM".to_owned()
            } else {
                "empty board created in RAM".to_owned()
            },
            dirty: true,
            should_exit: false,
            hits: Vec::new(),
            drag: None,
        }
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
        if self.modal.is_some() {
            self.handle_modal_event(terminal_event);
            return;
        }

        match terminal_event {
            Event::Key(key) if is_action_key(key) => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Resize(_, _) => self.dirty = true,
            _ => {}
        }
    }

    fn handle_modal_event(&mut self, terminal_event: Event) {
        match terminal_event {
            Event::Key(key) if is_action_key(key) => {
                let outcome = self
                    .modal
                    .as_mut()
                    .map(|modal| modal.handle_key(key))
                    .unwrap_or(ModalOutcome::None);
                self.apply_modal_outcome(outcome);
            }
            Event::Paste(text) => {
                if self
                    .modal
                    .as_mut()
                    .is_some_and(|modal| modal.insert_paste(&text))
                {
                    self.dirty = true;
                }
            }
            Event::Resize(_, _) => self.dirty = true,
            _ => {}
        }
    }

    fn apply_modal_outcome(&mut self, outcome: ModalOutcome) {
        match outcome {
            ModalOutcome::None => {}
            ModalOutcome::Dirty => self.dirty = true,
            ModalOutcome::Cancel => {
                self.modal = None;
                self.status = "cancelled".to_owned();
                self.dirty = true;
            }
            ModalOutcome::InputSubmitted { purpose, text } => {
                self.modal = None;
                let text = text.trim().to_owned();
                match purpose {
                    InputPurpose::NewCard { lane } => {
                        if text.is_empty() {
                            self.status = "card title may not be empty".to_owned();
                        } else {
                            let id = self.board.add(lane, text);
                            self.selected_lane = lane;
                            self.selected_card = Some(id);
                            self.status = "card created".to_owned();
                        }
                    }
                    InputPurpose::EditTitle { id } => {
                        if text.is_empty() {
                            self.status = "card title may not be empty".to_owned();
                        } else if let Some(card) = self.board.get_mut(id) {
                            card.title = text;
                            self.status = "title updated".to_owned();
                        }
                    }
                    InputPurpose::EditDetail { id } => {
                        if let Some(card) = self.board.get_mut(id) {
                            card.detail = text;
                            self.status = "detail updated".to_owned();
                        }
                    }
                }
                self.normalize_selection();
                self.ensure_selected_visible();
                self.dirty = true;
            }
            ModalOutcome::Confirmed(purpose) => {
                self.modal = None;
                match purpose {
                    ConfirmPurpose::DeleteCard { id } => {
                        if self.board.delete(id) {
                            self.status = "card deleted".to_owned();
                        }
                    }
                    ConfirmPurpose::Reset { demo } => {
                        self.board.reset(demo);
                        self.scroll = [0; 3];
                        self.selected_lane = Lane::Ideas;
                        self.selected_card = self.board.first_id(self.selected_lane);
                        self.status = if demo {
                            "demo board restored".to_owned()
                        } else {
                            "board emptied".to_owned()
                        };
                    }
                }
                self.normalize_selection();
                self.ensure_selected_visible();
                self.dirty = true;
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('q' | 'Q'))
        {
            self.should_exit = true;
            return;
        }

        if key.modifiers.contains(KeyModifiers::SHIFT) {
            match key.code {
                KeyCode::Left => {
                    self.move_selected(self.selected_lane.previous());
                    return;
                }
                KeyCode::Right => {
                    self.move_selected(self.selected_lane.next());
                    return;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.step_card(-1),
            KeyCode::Down | KeyCode::Char('j') => self.step_card(1),
            KeyCode::Left | KeyCode::Char('h') => self.select_lane(self.selected_lane.previous()),
            KeyCode::Right | KeyCode::Char('l') => self.select_lane(self.selected_lane.next()),
            KeyCode::Home => self.select_edge(false),
            KeyCode::End => self.select_edge(true),
            KeyCode::Enter => {
                if self.selected_card.is_some() {
                    self.execute(Command::Edit);
                } else {
                    self.execute(Command::New);
                }
            }
            KeyCode::Char('n' | 'N') => self.execute(Command::New),
            KeyCode::Char('e' | 'E') => self.execute(Command::Edit),
            KeyCode::Char('d' | 'D') => self.execute(Command::Detail),
            KeyCode::Char('x' | 'X') | KeyCode::Delete => self.execute(Command::Delete),
            KeyCode::Char('[') => self.execute(Command::MoveLeft),
            KeyCode::Char(']') | KeyCode::Char(' ') => self.execute(Command::MoveRight),
            KeyCode::Char('?') => self.execute(Command::Help),
            KeyCode::Esc => self.should_exit = true,
            KeyCode::Char(ch) if ch.is_ascii_digit() => {
                if let Some(index) = ch.to_digit(10).map(|value| value as usize) {
                    if let Some((command, _)) = COMMANDS.get(index) {
                        self.execute(*command);
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(target) = self.hit(mouse.column, mouse.row) {
                    match target {
                        HitTarget::Lane(lane) => self.select_lane(lane),
                        HitTarget::Card { id, lane } => {
                            self.selected_lane = lane;
                            self.selected_card = Some(id);
                            self.drag = Some(DragState {
                                id,
                                origin: lane,
                                target: lane,
                            });
                            self.ensure_selected_visible();
                            self.dirty = true;
                        }
                        HitTarget::Command(command) => self.execute(command),
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(lane) = self.lane_at(mouse.column, mouse.row)
                    && let Some(drag) = self.drag.as_mut()
                    && drag.target != lane
                {
                    drag.target = lane;
                    self.dirty = true;
                }
            }
            MouseEventKind::Up(_) => {
                if let Some(drag) = self.drag.take() {
                    if drag.target != drag.origin && self.board.move_to(drag.id, drag.target) {
                        self.selected_lane = drag.target;
                        self.selected_card = Some(drag.id);
                        self.status = format!("card moved to {}", drag.target.title());
                        self.ensure_selected_visible();
                    }
                    self.dirty = true;
                }
            }
            MouseEventKind::ScrollUp => self.step_card(-1),
            MouseEventKind::ScrollDown => self.step_card(1),
            _ => {}
        }
    }

    fn hit(&self, x: u16, y: u16) -> Option<HitTarget> {
        self.hits
            .iter()
            .rev()
            .find(|hit| hit.rect.contains(x, y))
            .map(|hit| hit.target)
    }

    fn lane_at(&self, x: u16, y: u16) -> Option<Lane> {
        self.hits.iter().rev().find_map(|hit| match hit.target {
            HitTarget::Lane(lane) if hit.rect.contains(x, y) => Some(lane),
            _ => None,
        })
    }

    fn select_lane(&mut self, lane: Lane) {
        self.selected_lane = lane;
        if !self
            .selected_card
            .and_then(|id| self.board.get(id))
            .is_some_and(|card| card.lane == lane)
        {
            self.selected_card = self.board.first_id(lane);
        }
        self.ensure_selected_visible();
        self.status = format!("{} selected", lane.title());
        self.dirty = true;
    }

    fn step_card(&mut self, delta: isize) {
        let ids: Vec<u64> = self
            .board
            .cards_in(self.selected_lane)
            .into_iter()
            .map(|card| card.id)
            .collect();
        if ids.is_empty() {
            self.selected_card = None;
            self.dirty = true;
            return;
        }
        let current = self
            .selected_card
            .and_then(|id| ids.iter().position(|candidate| *candidate == id))
            .unwrap_or(0);
        let next = if delta < 0 {
            current.saturating_sub(delta.unsigned_abs())
        } else {
            (current + delta as usize).min(ids.len() - 1)
        };
        self.selected_card = Some(ids[next]);
        self.ensure_selected_visible();
        self.dirty = true;
    }

    fn select_edge(&mut self, last: bool) {
        let ids: Vec<u64> = self
            .board
            .cards_in(self.selected_lane)
            .into_iter()
            .map(|card| card.id)
            .collect();
        self.selected_card = if last {
            ids.last().copied()
        } else {
            ids.first().copied()
        };
        self.ensure_selected_visible();
        self.dirty = true;
    }

    fn move_selected(&mut self, lane: Lane) {
        let Some(id) = self.selected_card else {
            self.status = "no card selected".to_owned();
            self.dirty = true;
            return;
        };
        if self.board.move_to(id, lane) {
            self.selected_lane = lane;
            self.selected_card = Some(id);
            self.status = format!("card moved to {}", lane.title());
            self.ensure_selected_visible();
        } else {
            self.status = "card already at board edge".to_owned();
        }
        self.dirty = true;
    }

    fn execute(&mut self, command: Command) {
        match command {
            Command::New => {
                self.modal = Some(Modal::Input(InputModal::new(
                    "new card",
                    format!("title for {}", self.selected_lane.title()),
                    "",
                    InputPurpose::NewCard {
                        lane: self.selected_lane,
                    },
                )));
            }
            Command::Edit => {
                let Some(card) = self.current_card().cloned() else {
                    self.status = "no card selected".to_owned();
                    self.dirty = true;
                    return;
                };
                self.modal = Some(Modal::Input(InputModal::new(
                    "edit title",
                    "card title",
                    card.title,
                    InputPurpose::EditTitle { id: card.id },
                )));
            }
            Command::Detail => {
                let Some(card) = self.current_card().cloned() else {
                    self.status = "no card selected".to_owned();
                    self.dirty = true;
                    return;
                };
                self.modal = Some(Modal::Input(InputModal::new(
                    "edit detail",
                    "single-line detail",
                    card.detail,
                    InputPurpose::EditDetail { id: card.id },
                )));
            }
            Command::MoveLeft => self.move_selected(self.selected_lane.previous()),
            Command::MoveRight => self.move_selected(self.selected_lane.next()),
            Command::Delete => {
                let Some(card) = self.current_card().cloned() else {
                    self.status = "no card selected".to_owned();
                    self.dirty = true;
                    return;
                };
                self.modal = Some(Modal::Confirm(ConfirmModal {
                    title: "delete card".to_owned(),
                    question: format!("delete ‘{}’?", card.title),
                    purpose: ConfirmPurpose::DeleteCard { id: card.id },
                }));
            }
            Command::Demo => {
                self.modal = Some(Modal::Confirm(ConfirmModal {
                    title: "reset board".to_owned(),
                    question: "replace everything with the demo board?".to_owned(),
                    purpose: ConfirmPurpose::Reset { demo: true },
                }));
            }
            Command::Empty => {
                self.modal = Some(Modal::Confirm(ConfirmModal {
                    title: "empty board".to_owned(),
                    question: "delete every card in this RAM session?".to_owned(),
                    purpose: ConfirmPurpose::Reset { demo: false },
                }));
            }
            Command::Help => self.modal = Some(Modal::Help),
            Command::Exit => self.should_exit = true,
        }
        self.dirty = true;
    }

    fn current_card(&self) -> Option<&Card> {
        self.selected_card.and_then(|id| self.board.get(id))
    }

    fn normalize_selection(&mut self) {
        if self
            .selected_card
            .and_then(|id| self.board.get(id))
            .is_some_and(|card| card.lane == self.selected_lane)
        {
            return;
        }
        self.selected_card = self.board.first_id(self.selected_lane);
    }

    fn ensure_selected_visible(&mut self) {
        let Ok((width, height)) = terminal::size() else {
            return;
        };
        let Some(geometry) = geometry(width, height) else {
            return;
        };
        let lanes = lane_rects(geometry.board);
        let lane = self.selected_lane;
        let ids: Vec<u64> = self
            .board
            .cards_in(lane)
            .into_iter()
            .map(|card| card.id)
            .collect();
        let Some(id) = self.selected_card else {
            self.scroll[lane.index()] = 0;
            return;
        };
        let Some(index) = ids.iter().position(|candidate| *candidate == id) else {
            return;
        };
        let slots = visible_card_slots(lanes[lane.index()]).max(1);
        let scroll = &mut self.scroll[lane.index()];
        if index < *scroll {
            *scroll = index;
        } else if index >= *scroll + slots {
            *scroll = index + 1 - slots;
        }
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
        let lanes = lane_rects(geometry.board);
        for lane in Lane::ALL {
            let rect = lanes[lane.index()];
            let cards: Vec<Card> = self.board.cards_in(lane).into_iter().cloned().collect();
            draw_lane(
                &mut frame,
                rect,
                lane,
                &cards,
                self.scroll[lane.index()],
                self.selected_lane,
                self.selected_card,
                self.drag,
                &mut hits,
            );
        }
        draw_menu(&mut frame, geometry.menu, &mut hits);
        draw_status(
            &mut frame,
            geometry.status_row,
            width,
            &self.status,
            self.current_card(),
        );
        frame.put_str(
            1,
            geometry.hint_row,
            "↑↓ cards ・ ←→ lanes ・ Shift+←→ move ・ Enter edit ・ n new ・ drag cards",
            Style::new(Color::DarkGrey, Color::Reset),
        );

        if let Some(modal) = self.modal.as_ref() {
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
    let board = Rect {
        x: 1,
        y: 2,
        width: menu.x.saturating_sub(2),
        height: height.saturating_sub(6),
    };
    Some(Geometry {
        board,
        menu,
        status_row: height - 3,
        hint_row: height - 2,
    })
}

fn lane_rects(board: Rect) -> [Rect; 3] {
    let gaps = 2u16;
    let available = board.width.saturating_sub(gaps);
    let base = available / 3;
    let remainder = available % 3;
    let widths = [
        base + if remainder > 0 { 1 } else { 0 },
        base + if remainder > 1 { 1 } else { 0 },
        base,
    ];
    let first = Rect {
        x: board.x,
        y: board.y,
        width: widths[0],
        height: board.height,
    };
    let second = Rect {
        x: first.right() + 1,
        y: board.y,
        width: widths[1],
        height: board.height,
    };
    let third = Rect {
        x: second.right() + 1,
        y: board.y,
        width: widths[2],
        height: board.height,
    };
    [first, second, third]
}

fn visible_card_slots(lane: Rect) -> usize {
    lane.height.saturating_sub(4) as usize / CARD_HEIGHT as usize
}

#[allow(clippy::too_many_arguments)]
fn draw_lane(
    frame: &mut Frame,
    rect: Rect,
    lane: Lane,
    cards: &[Card],
    scroll: usize,
    selected_lane: Lane,
    selected_card: Option<u64>,
    drag: Option<DragState>,
    hits: &mut Vec<HitRegion>,
) {
    let lane_selected = selected_lane == lane;
    let drag_target = drag.is_some_and(|state| state.target == lane && state.target != state.origin);
    let border = if drag_target {
        Style::new(Color::Yellow, Color::Reset).bold()
    } else if lane_selected {
        Style::new(Color::Cyan, Color::Reset).bold()
    } else {
        Style::new(Color::DarkGrey, Color::Reset)
    };
    draw_box(frame, rect, border);
    let count = format!("{}  {}", lane.title(), cards.len());
    let label = clip_text_cells(&count, rect.width.saturating_sub(4) as usize);
    frame.put_str(rect.x + 2, rect.y, &label, border);
    hits.push(HitRegion {
        rect,
        target: HitTarget::Lane(lane),
    });

    let slots = visible_card_slots(rect);
    let start_y = rect.y + 2;
    for (slot, card) in cards.iter().skip(scroll).take(slots).enumerate() {
        let card_rect = Rect {
            x: rect.x + 1,
            y: start_y + slot as u16 * CARD_HEIGHT,
            width: rect.width.saturating_sub(2),
            height: CARD_HEIGHT,
        };
        let selected = selected_card == Some(card.id);
        draw_card(frame, card_rect, card, selected);
        hits.push(HitRegion {
            rect: card_rect,
            target: HitTarget::Card { id: card.id, lane },
        });
    }

    if scroll > 0 {
        frame.put_str(
            rect.right().saturating_sub(4),
            rect.y + 1,
            "↑…",
            Style::new(Color::DarkGrey, Color::Reset),
        );
    }
    if scroll + slots < cards.len() {
        frame.put_str(
            rect.right().saturating_sub(4),
            rect.bottom().saturating_sub(2),
            "↓…",
            Style::new(Color::DarkGrey, Color::Reset),
        );
    }
}

fn draw_card(frame: &mut Frame, rect: Rect, card: &Card, selected: bool) {
    let style = if selected {
        Style::new(Color::Black, Color::Cyan).bold()
    } else {
        Style::new(Color::White, Color::DarkBlue)
    };
    frame.fill_rect(rect.x, rect.y, rect.width, rect.height, ' ', style);
    let title = clip_text_cells(&card.title, rect.width.saturating_sub(4) as usize);
    let detail = if card.detail.is_empty() {
        "no detail".to_owned()
    } else {
        clip_text_cells(&card.detail, rect.width.saturating_sub(4) as usize)
    };
    frame.put_str(rect.x + 1, rect.y, "┌", style);
    frame.put_str(rect.right().saturating_sub(1), rect.y, "┐", style);
    frame.put_str(rect.x + 2, rect.y + 1, &title, style);
    frame.put_str(
        rect.x + 2,
        rect.y + 2,
        &detail,
        if selected { style } else { style.dim() },
    );
    frame.put_str(rect.x + 1, rect.bottom().saturating_sub(1), "└", style);
    frame.put_str(
        rect.right().saturating_sub(1),
        rect.bottom().saturating_sub(1),
        "┘",
        style,
    );
}

fn draw_menu(frame: &mut Frame, rect: Rect, hits: &mut Vec<HitRegion>) {
    draw_box(
        frame,
        rect,
        Style::new(Color::DarkGrey, Color::Reset),
    );
    frame.put_str(
        rect.x + 2,
        rect.y,
        "☰ BOARD",
        Style::new(Color::Yellow, Color::Reset).bold(),
    );
    for (index, (command, label)) in COMMANDS.iter().enumerate() {
        let row = rect.y + 2 + index as u16;
        if row >= rect.bottom().saturating_sub(1) {
            break;
        }
        let line = format!("{index}  {label}");
        frame.put_str(
            rect.x + 2,
            row,
            &clip_text_cells(&line, rect.width.saturating_sub(4) as usize),
            Style::new(Color::White, Color::Reset),
        );
        hits.push(HitRegion {
            rect: Rect {
                x: rect.x + 1,
                y: row,
                width: rect.width.saturating_sub(2),
                height: 1,
            },
            target: HitTarget::Command(*command),
        });
    }
    let ram = "session: RAM only";
    if rect.height > 15 {
        frame.put_str(
            rect.x + 2,
            rect.bottom().saturating_sub(3),
            &clip_text_cells(ram, rect.width.saturating_sub(4) as usize),
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
    let right = "three lanes ・ zero services";
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

fn draw_status(
    frame: &mut Frame,
    row: u16,
    width: u16,
    status: &str,
    selected: Option<&Card>,
) {
    frame.fill_rect(0, row, width, 1, ' ', Style::new(Color::Black, Color::White));
    let left = format!(" {status}");
    frame.put_str(
        0,
        row,
        &clip_text_cells(&left, width.saturating_sub(2) as usize),
        Style::new(Color::Black, Color::White),
    );
    if let Some(card) = selected {
        let right = format!(" #{} ・ {} ", card.id, card.lane.title());
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
}

fn draw_modal(frame: &mut Frame, modal: &Modal) {
    let width = frame.width();
    let height = frame.height();
    let modal_width = width.saturating_sub(10).min(68).max(34);
    let modal_height = match modal {
        Modal::Help => 14,
        _ => 8,
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
        Modal::Input(input) => {
            frame.put_str(rect.x + 2, rect.y, &input.title, base.bold());
            frame.put_str(
                rect.x + 2,
                rect.y + 2,
                &clip_text_cells(&input.prompt, rect.width.saturating_sub(4) as usize),
                base,
            );
            let field = Rect {
                x: rect.x + 2,
                y: rect.y + 4,
                width: rect.width.saturating_sub(4),
                height: 1,
            };
            frame.fill_rect(
                field.x,
                field.y,
                field.width,
                1,
                ' ',
                Style::new(Color::Black, Color::White),
            );
            let visible = clip_text_cells(&input.text, field.width as usize);
            frame.put_str(
                field.x,
                field.y,
                &visible,
                Style::new(Color::Black, Color::White),
            );
            let cursor_x = input
                .text
                .chars()
                .take(input.cursor)
                .map(|ch| crate::screen::terminal_cell_width(ch) as usize)
                .sum::<usize>()
                .min(field.width.saturating_sub(1) as usize) as u16;
            let cursor_ch = input.text.chars().nth(input.cursor).unwrap_or(' ');
            frame.put_display_char(
                field.x + cursor_x,
                field.y,
                cursor_ch,
                Style::new(Color::White, Color::Black).underline(),
            );
            frame.put_str(
                rect.x + 2,
                rect.bottom().saturating_sub(2),
                "Enter accept ・ Esc cancel",
                base.dim(),
            );
        }
        Modal::Confirm(confirm) => {
            frame.put_str(rect.x + 2, rect.y, &confirm.title, base.bold());
            frame.put_str(
                rect.x + 2,
                rect.y + 3,
                &clip_text_cells(&confirm.question, rect.width.saturating_sub(4) as usize),
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
            frame.put_str(rect.x + 2, rect.y, "tboard controls", base.bold());
            let lines = [
                "↑ ↓ or j k       select cards",
                "← → or h l       select lanes",
                "Shift+← / →      move selected card",
                "[ / ] or Space   move card left / right",
                "Enter / e        edit title",
                "n / d / x        new / detail / delete",
                "LMB drag         move a card across lanes",
                "0..9             run the numbered menu item",
                "Ctrl-Q / Esc     exit",
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
    let message = format!("tboard needs at least {MIN_WIDTH}×{MIN_HEIGHT}; terminal is {width}×{height}");
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
