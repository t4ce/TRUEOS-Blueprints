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
    layout::{GraphLayout, LayoutEdge, LayoutNode, NodeKind, Rect},
    menu::{entries_for, Command, MenuSection, MenuState},
    modal::{ConfirmModal, ConfirmPurpose, InputModal, InputPurpose, Modal, ModalOutcome},
    model::{display_bytes, input_bytes, parse_user_bytes, DbSnapshot, Selection},
    screen::{clip_text_cells, terminal_cell_width, text_cell_width, Frame, Renderer, Style},
    store::Store,
};

const TICK: Duration = Duration::from_millis(16);
const MAX_EVENT_BATCH: usize = 64;
const MENU_WIDTH: u16 = 24;
const MIN_WIDTH: u16 = 72;
const MIN_HEIGHT: u16 = 20;
const PAN_X: i32 = 4;
const PAN_Y: i32 = 2;
const BUILD_ID: &str = "tredb 0.1.0 ・ redb 4.2 no_std ・ RAM only";

#[derive(Clone, Debug)]
enum HitTarget {
    Selection(Selection),
    Section(MenuSection),
    Command(Command),
}

#[derive(Clone, Debug)]
struct HitRegion {
    rect: Rect,
    target: HitTarget,
}

#[derive(Clone, Copy, Debug)]
struct PanDrag {
    start_column: u16,
    start_row: u16,
    camera_x: i32,
    camera_y: i32,
}

#[derive(Clone, Copy, Debug)]
struct Geometry {
    canvas: Rect,
    menu: Rect,
    selection_row: u16,
    status_row: u16,
}

pub struct App {
    store: Store,
    snapshot: DbSnapshot,
    graph: GraphLayout,
    selection: Selection,
    menu: MenuState,
    modal: Option<Modal>,
    camera_x: i32,
    camera_y: i32,
    spacing: usize,
    show_values: bool,
    status: String,
    dirty: bool,
    should_exit: bool,
    hits: Vec<HitRegion>,
    pan_drag: Option<PanDrag>,
}

impl App {
    pub fn new(seed_demo: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let store = Store::new(seed_demo)?;
        let snapshot = store.snapshot()?;
        let graph = GraphLayout::build(&snapshot, true, 1);
        let mut app = Self {
            store,
            snapshot,
            graph,
            selection: Selection::Database,
            menu: MenuState::default(),
            modal: None,
            camera_x: 0,
            camera_y: 0,
            spacing: 1,
            show_values: true,
            status: if seed_demo {
                "demo database created in RAM".to_owned()
            } else {
                "empty database created in RAM".to_owned()
            },
            dirty: true,
            should_exit: false,
            hits: Vec::new(),
            pan_drag: None,
        };
        app.center_current_terminal();
        Ok(app)
    }

    pub fn run<W: Write>(
        &mut self,
        out: &mut W,
        renderer: &mut Renderer,
    ) -> io::Result<()> {
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

    fn rebuild_graph(&mut self) {
        self.graph = GraphLayout::build(&self.snapshot, self.show_values, self.spacing);
        self.dirty = true;
    }

    fn refresh_snapshot(&mut self, message: impl Into<String>) {
        match self.store.snapshot() {
            Ok(snapshot) => {
                self.snapshot = snapshot;
                self.snapshot.normalize_selection(&mut self.selection);
                self.rebuild_graph();
                self.status = message.into();
            }
            Err(error) => {
                self.status = format!("refresh failed: {error}");
                self.dirty = true;
            }
        }
    }

    fn center_current_terminal(&mut self) {
        if let Ok((width, height)) = terminal::size() {
            self.center_for_size(width, height);
        }
    }

    fn center_for_size(&mut self, width: u16, height: u16) {
        let Some(geometry) = geometry(width, height) else {
            return;
        };
        self.camera_x = geometry.canvas.width as i32 / 2 - self.graph.bounds.center_x();
        self.camera_y = geometry.canvas.height as i32 / 2 - self.graph.bounds.center_y();
        self.dirty = true;
    }

    fn handle_event(&mut self, terminal_event: Event) {
        if self.modal.is_some() {
            self.handle_modal_event(terminal_event);
            return;
        }

        match terminal_event {
            Event::Key(key) if is_action_key(key) => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Resize(_, _) => {
                self.dirty = true;
            }
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

    fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('q' | 'Q'))
        {
            self.should_exit = true;
            return;
        }

        match key.code {
            KeyCode::Esc => self.should_exit = true,
            KeyCode::Tab => {
                self.menu.cycle(key.modifiers.contains(KeyModifiers::SHIFT));
                self.dirty = true;
            }
            KeyCode::Left | KeyCode::Char('a' | 'A') => {
                self.camera_x += PAN_X;
                self.dirty = true;
            }
            KeyCode::Right | KeyCode::Char('d' | 'D') => {
                self.camera_x -= PAN_X;
                self.dirty = true;
            }
            KeyCode::Up | KeyCode::Char('w' | 'W') => {
                self.camera_y += PAN_Y;
                self.dirty = true;
            }
            KeyCode::Down | KeyCode::Char('s' | 'S') => {
                self.camera_y -= PAN_Y;
                self.dirty = true;
            }
            KeyCode::Home => self.center_current_terminal(),
            KeyCode::Char('r' | 'R') => self.refresh_snapshot("snapshot refreshed"),
            KeyCode::Char('h' | 'H' | '?') => {
                self.modal = Some(Modal::Help);
                self.dirty = true;
            }
            KeyCode::Enter => self.execute_default_action(),
            KeyCode::Delete => self.execute_command(Command::Delete),
            KeyCode::Char(digit @ '0'..='9') => {
                if let Some(command) = self.menu.command_for_digit(&self.selection, digit) {
                    self.execute_command(command);
                }
            }
            _ => {}
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let target = self
                    .hits
                    .iter()
                    .rev()
                    .find(|hit| hit.rect.contains(mouse.column as i32, mouse.row as i32))
                    .map(|hit| hit.target.clone());
                if let Some(target) = target {
                    match target {
                        HitTarget::Selection(selection) => {
                            self.selection = selection;
                            self.menu.section = MenuSection::Action;
                            self.dirty = true;
                        }
                        HitTarget::Section(section) => {
                            self.menu.section = section;
                            self.dirty = true;
                        }
                        HitTarget::Command(command) => self.execute_command(command),
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Middle) => {
                self.pan_drag = Some(PanDrag {
                    start_column: mouse.column,
                    start_row: mouse.row,
                    camera_x: self.camera_x,
                    camera_y: self.camera_y,
                });
            }
            MouseEventKind::Drag(MouseButton::Middle) => {
                if let Some(drag) = self.pan_drag {
                    self.camera_x = drag.camera_x + mouse.column as i32 - drag.start_column as i32;
                    self.camera_y = drag.camera_y + mouse.row as i32 - drag.start_row as i32;
                    self.dirty = true;
                }
            }
            MouseEventKind::Up(MouseButton::Middle) => self.pan_drag = None,
            MouseEventKind::ScrollUp => {
                self.camera_y += 3;
                self.dirty = true;
            }
            MouseEventKind::ScrollDown => {
                self.camera_y -= 3;
                self.dirty = true;
            }
            _ => {}
        }
    }

    fn execute_default_action(&mut self) {
        let command = match &self.selection {
            Selection::Database => Command::NewTable,
            Selection::Table { .. } => Command::NewRow,
            Selection::Row { .. } => Command::EditValue,
        };
        self.execute_command(command);
    }

    fn execute_command(&mut self, command: Command) {
        match command {
            Command::Refresh => self.refresh_snapshot("snapshot refreshed"),
            Command::ResetDemo => {
                self.modal = Some(Modal::Confirm(ConfirmModal {
                    title: "Reset database".to_owned(),
                    question: "Discard RAM state and restore the demo database?".to_owned(),
                    purpose: ConfirmPurpose::Reset { demo: true },
                }));
                self.dirty = true;
            }
            Command::ResetEmpty => {
                self.modal = Some(Modal::Confirm(ConfirmModal {
                    title: "Reset database".to_owned(),
                    question: "Discard RAM state and create an empty database?".to_owned(),
                    purpose: ConfirmPurpose::Reset { demo: false },
                }));
                self.dirty = true;
            }
            Command::Center => self.center_current_terminal(),
            Command::CycleSpacing => {
                self.spacing = (self.spacing + 1) % 4;
                self.rebuild_graph();
                self.center_current_terminal();
                self.status = format!("graph spacing {}", self.spacing + 1);
            }
            Command::ToggleValues => {
                self.show_values = !self.show_values;
                self.rebuild_graph();
                self.status = if self.show_values {
                    "row values shown".to_owned()
                } else {
                    "row values replaced by byte counts".to_owned()
                };
            }
            Command::Help => {
                self.modal = Some(Modal::Help);
                self.dirty = true;
            }
            Command::NewTable => {
                self.modal = Some(Modal::Input(InputModal::new(
                    "New table",
                    "table name",
                    "",
                    InputPurpose::NewTable,
                )));
                self.dirty = true;
            }
            Command::NewRow => self.open_new_row_modal(),
            Command::EditKey => self.open_edit_key_modal(),
            Command::EditValue => self.open_edit_value_modal(),
            Command::Delete => self.open_delete_confirmation(),
            Command::Exit => self.should_exit = true,
        }
    }

    fn open_new_row_modal(&mut self) {
        let Some(table) = self.selection.table_name().map(str::to_owned) else {
            self.status = "select a table first".to_owned();
            self.dirty = true;
            return;
        };
        self.modal = Some(Modal::Input(InputModal::new(
            "New row",
            format!("key for {table}"),
            "",
            InputPurpose::NewRowKey { table },
        )));
        self.dirty = true;
    }

    fn open_edit_key_modal(&mut self) {
        let Some((table, key, value)) = self.selected_row_data() else {
            self.status = "select a row first".to_owned();
            self.dirty = true;
            return;
        };
        self.modal = Some(Modal::Input(InputModal::new(
            "Edit row key",
            format!("new key for {table}"),
            input_bytes(&key),
            InputPurpose::EditKey {
                table,
                old_key: key,
                value,
            },
        )));
        self.dirty = true;
    }

    fn open_edit_value_modal(&mut self) {
        let Some((table, key, value)) = self.selected_row_data() else {
            self.status = "select a row first".to_owned();
            self.dirty = true;
            return;
        };
        self.modal = Some(Modal::Input(InputModal::new(
            "Edit row value",
            format!("value for {}", display_bytes(&key)),
            input_bytes(&value),
            InputPurpose::EditValue { table, key },
        )));
        self.dirty = true;
    }

    fn open_delete_confirmation(&mut self) {
        match &self.selection {
            Selection::Database => {
                self.status = "the database root is reset, not deleted".to_owned();
                self.dirty = true;
            }
            Selection::Table { table } => {
                self.modal = Some(Modal::Confirm(ConfirmModal {
                    title: "Delete table".to_owned(),
                    question: format!("Delete table {table} and all of its rows?"),
                    purpose: ConfirmPurpose::DeleteTable {
                        table: table.clone(),
                    },
                }));
                self.dirty = true;
            }
            Selection::Row { table, key } => {
                self.modal = Some(Modal::Confirm(ConfirmModal {
                    title: "Delete row".to_owned(),
                    question: format!("Delete key {} from {table}?", display_bytes(key)),
                    purpose: ConfirmPurpose::DeleteRow {
                        table: table.clone(),
                        key: key.clone(),
                    },
                }));
                self.dirty = true;
            }
        }
    }

    fn selected_row_data(&self) -> Option<(String, Vec<u8>, Vec<u8>)> {
        let Selection::Row { table, key } = &self.selection else {
            return None;
        };
        let row = self
            .snapshot
            .table(table)?
            .rows
            .iter()
            .find(|row| row.key.as_slice() == key.as_slice())?;
        Some((table.clone(), key.clone(), row.value.clone()))
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
                self.submit_input(purpose, text);
            }
            ModalOutcome::Confirmed(purpose) => self.confirm(purpose),
        }
    }

    fn submit_input(&mut self, purpose: InputPurpose, text: String) {
        match purpose {
            InputPurpose::NewTable => match self.store.create_table(&text) {
                Ok(table) => {
                    self.selection = Selection::Table {
                        table: table.clone(),
                    };
                    self.modal = None;
                    self.refresh_snapshot(format!("created table {table}"));
                }
                Err(error) => self.modal_error(error.to_string()),
            },
            InputPurpose::NewRowKey { table } => match parse_user_bytes(&text) {
                Ok(key) if !key.is_empty() => {
                    self.modal = Some(Modal::Input(InputModal::new(
                        "New row",
                        format!("value for {}", display_bytes(&key)),
                        "",
                        InputPurpose::NewRowValue { table, key },
                    )));
                    self.dirty = true;
                }
                Ok(_) => self.modal_error("row key cannot be empty"),
                Err(error) => self.modal_error(error.to_string()),
            },
            InputPurpose::NewRowValue { table, key } => match parse_user_bytes(&text) {
                Ok(value) => match self.store.upsert(&table, &key, &value) {
                    Ok(()) => {
                        self.selection = Selection::Row {
                            table: table.clone(),
                            key: key.clone(),
                        };
                        self.modal = None;
                        self.refresh_snapshot(format!("inserted row in {table}"));
                    }
                    Err(error) => self.modal_error(error.to_string()),
                },
                Err(error) => self.modal_error(error.to_string()),
            },
            InputPurpose::EditKey {
                table,
                old_key,
                value,
            } => match parse_user_bytes(&text) {
                Ok(new_key) if !new_key.is_empty() => {
                    match self
                        .store
                        .replace_key(&table, &old_key, &new_key, &value)
                    {
                        Ok(()) => {
                            self.selection = Selection::Row {
                                table: table.clone(),
                                key: new_key,
                            };
                            self.modal = None;
                            self.refresh_snapshot(format!("updated key in {table}"));
                        }
                        Err(error) => self.modal_error(error.to_string()),
                    }
                }
                Ok(_) => self.modal_error("row key cannot be empty"),
                Err(error) => self.modal_error(error.to_string()),
            },
            InputPurpose::EditValue { table, key } => match parse_user_bytes(&text) {
                Ok(value) => match self.store.upsert(&table, &key, &value) {
                    Ok(()) => {
                        self.selection = Selection::Row {
                            table: table.clone(),
                            key,
                        };
                        self.modal = None;
                        self.refresh_snapshot(format!("updated value in {table}"));
                    }
                    Err(error) => self.modal_error(error.to_string()),
                },
                Err(error) => self.modal_error(error.to_string()),
            },
        }
    }

    fn confirm(&mut self, purpose: ConfirmPurpose) {
        match purpose {
            ConfirmPurpose::DeleteTable { table } => match self.store.delete_table(&table) {
                Ok(_) => {
                    self.selection = Selection::Database;
                    self.modal = None;
                    self.refresh_snapshot(format!("deleted table {table}"));
                }
                Err(error) => self.modal_error(error.to_string()),
            },
            ConfirmPurpose::DeleteRow { table, key } => {
                match self.store.delete_row(&table, &key) {
                    Ok(_) => {
                        self.selection = Selection::Table {
                            table: table.clone(),
                        };
                        self.modal = None;
                        self.refresh_snapshot(format!("deleted row from {table}"));
                    }
                    Err(error) => self.modal_error(error.to_string()),
                }
            }
            ConfirmPurpose::Reset { demo } => match self.store.reset(demo) {
                Ok(()) => {
                    self.selection = Selection::Database;
                    self.modal = None;
                    self.refresh_snapshot(if demo {
                        "demo database restored"
                    } else {
                        "empty database created"
                    });
                    self.center_current_terminal();
                }
                Err(error) => self.modal_error(error.to_string()),
            },
        }
    }

    fn modal_error(&mut self, message: impl Into<String>) {
        self.status = format!("error: {}", message.into());
        self.dirty = true;
    }

    fn draw(&mut self, width: u16, height: u16) -> Frame {
        let mut frame = Frame::new(width, height);
        self.hits.clear();

        if width < MIN_WIDTH || height < MIN_HEIGHT {
            let message = format!("tredb needs at least {MIN_WIDTH}×{MIN_HEIGHT}; now {width}×{height}");
            let x = width.saturating_sub(text_cell_width(&message) as u16) / 2;
            let y = height / 2;
            frame.put_str(x, y, &message, Style::new(Color::Yellow, Color::Reset).bold());
            return frame;
        }

        let geometry = geometry(width, height).expect("size checked above");
        self.draw_header(&mut frame, width);
        self.draw_canvas(&mut frame, geometry.canvas);
        self.draw_menu(&mut frame, geometry.menu);
        self.draw_footer(&mut frame, geometry, width);
        if let Some(modal) = self.modal.as_ref() {
            draw_modal(&mut frame, modal, width, height);
        }
        frame
    }

    fn draw_header(&self, frame: &mut Frame, width: u16) {
        let line = Style::new(Color::DarkGrey, Color::Reset);
        for x in 0..width {
            frame.put(x, 0, '─', line);
        }
        let title = clip_text_cells(BUILD_ID, width.saturating_sub(4) as usize);
        let start = width.saturating_sub(text_cell_width(&title) as u16) / 2;
        if start > 0 {
            frame.put(start - 1, 0, ' ', Style::default());
        }
        frame.put_str(
            start,
            0,
            &title,
            Style::new(Color::Cyan, Color::Reset).bold(),
        );
        let end = start.saturating_add(text_cell_width(&title) as u16);
        if end < width {
            frame.put(end, 0, ' ', Style::default());
        }
    }

    fn draw_canvas(&mut self, frame: &mut Frame, canvas: Rect) {
        let border = Style::new(Color::DarkGrey, Color::Reset);
        for y in canvas.y..=canvas.bottom() {
            frame.put_i32(canvas.right() + 1, y, '│', border);
        }

        for edge in &self.graph.edges {
            self.draw_edge(frame, canvas, *edge);
        }

        let mut node_hits = Vec::new();
        for node in &self.graph.nodes {
            let screen_rect = node
                .rect
                .translated(canvas.x + self.camera_x, canvas.y + self.camera_y);
            if !rects_intersect(screen_rect, canvas) {
                continue;
            }
            draw_node(
                frame,
                canvas,
                node,
                screen_rect,
                node.selection == self.selection,
            );
            node_hits.push(HitRegion {
                rect: screen_rect,
                target: HitTarget::Selection(node.selection.clone()),
            });
        }
        self.hits.extend(node_hits);
    }

    fn draw_edge(&self, frame: &mut Frame, canvas: Rect, edge: LayoutEdge) {
        let parent = self.graph.nodes[edge.parent]
            .rect
            .translated(canvas.x + self.camera_x, canvas.y + self.camera_y);
        let child = self.graph.nodes[edge.child]
            .rect
            .translated(canvas.x + self.camera_x, canvas.y + self.camera_y);
        let start_x = parent.right() + 1;
        let start_y = parent.center_y();
        let end_x = child.x - 1;
        let end_y = child.center_y();
        let elbow_x = (start_x + end_x) / 2;
        let style = Style::new(Color::DarkGrey, Color::Reset).dim();

        draw_canvas_hline(frame, canvas, start_x, start_y, elbow_x, '─', style);
        draw_canvas_vline(frame, canvas, elbow_x, start_y, end_y, '│', style);
        draw_canvas_hline(frame, canvas, elbow_x, end_y, end_x, '─', style);
        put_canvas(frame, canvas, elbow_x, start_y, '┼', style);
        put_canvas(frame, canvas, elbow_x, end_y, '┼', style);
        put_canvas(frame, canvas, end_x, end_y, '▶', style);
    }

    fn draw_menu(&mut self, frame: &mut Frame, menu: Rect) {
        let background = Style::new(Color::Grey, Color::Black);
        for y in menu.y..=menu.bottom() {
            for x in menu.x..=menu.right() {
                frame.put_i32(x, y, ' ', background);
            }
        }

        let mut row = menu.y;
        let mut menu_hits = Vec::new();
        for section in MenuSection::ORDER {
            if row > menu.bottom() {
                break;
            }
            let active = self.menu.section == section;
            let header_style = if active {
                Style::new(Color::Black, Color::White).bold()
            } else {
                Style::new(Color::Cyan, Color::Black).bold()
            };
            fill_screen_row(frame, menu, row, ' ', header_style);
            let title = format!(" ☰ {} ", section.title());
            frame.put_str_i32(menu.x + 1, row, &title, header_style);
            menu_hits.push(HitRegion {
                rect: Rect {
                    x: menu.x,
                    y: row,
                    width: menu.width,
                    height: 1,
                },
                target: HitTarget::Section(section),
            });
            row += 1;

            for (index, entry) in entries_for(section, &self.selection).into_iter().enumerate() {
                if row > menu.bottom() {
                    break;
                }
                let style = if active {
                    Style::new(Color::White, Color::Black)
                } else {
                    Style::new(Color::DarkGrey, Color::Black).dim()
                };
                fill_screen_row(frame, menu, row, ' ', style);
                let label = self.menu_label(entry.command, entry.label);
                let text = format!(" {index} {label}");
                frame.put_str_i32(
                    menu.x + 1,
                    row,
                    &clip_text_cells(&text, menu.width.saturating_sub(2) as usize),
                    style,
                );
                menu_hits.push(HitRegion {
                    rect: Rect {
                        x: menu.x,
                        y: row,
                        width: menu.width,
                        height: 1,
                    },
                    target: HitTarget::Command(entry.command),
                });
                row += 1;
            }
            row += 1;
        }
        self.hits.extend(menu_hits);
    }

    fn menu_label(&self, command: Command, base: &'static str) -> String {
        match command {
            Command::CycleSpacing => format!("{base} {}", self.spacing + 1),
            Command::ToggleValues => {
                format!("{base} {}", if self.show_values { "on" } else { "off" })
            }
            _ => base.to_owned(),
        }
    }

    fn draw_footer(&self, frame: &mut Frame, geometry: Geometry, width: u16) {
        let selection = self.selection_summary();
        fill_row(frame, geometry.selection_row, width, Style::new(Color::Black, Color::Grey));
        frame.put_str(
            1,
            geometry.selection_row,
            &clip_text_cells(&selection, width.saturating_sub(2) as usize),
            Style::new(Color::Black, Color::Grey).bold(),
        );

        fill_row(frame, geometry.status_row, width, Style::new(Color::White, Color::Black));
        let status = format!(
            "{}  ・  Tab menu  0..9 action  Enter default  Home center  Esc exit",
            self.status
        );
        frame.put_str(
            1,
            geometry.status_row,
            &clip_text_cells(&status, width.saturating_sub(2) as usize),
            Style::new(Color::White, Color::Black),
        );
    }

    fn selection_summary(&self) -> String {
        match &self.selection {
            Selection::Database => format!(
                "RAM database ・ {} tables ・ {} visible rows ・ closes without persistence",
                self.snapshot.tables.len(),
                self.snapshot.total_rows()
            ),
            Selection::Table { table } => self
                .snapshot
                .table(table)
                .map(|snapshot| {
                    format!(
                        "table {table} ・ {}{} rows ・ &[u8] → &[u8]",
                        snapshot.rows.len(),
                        if snapshot.truncated { "+" } else { "" }
                    )
                })
                .unwrap_or_else(|| format!("table {table}")),
            Selection::Row { table, key } => self
                .snapshot
                .table(table)
                .and_then(|snapshot| {
                    snapshot
                        .rows
                        .iter()
                        .find(|row| row.key.as_slice() == key.as_slice())
                })
                .map(|row| {
                    format!(
                        "{table} ・ key {} ({} B) ・ value {} ({} B)",
                        display_bytes(&row.key),
                        row.key.len(),
                        display_bytes(&row.value),
                        row.value.len()
                    )
                })
                .unwrap_or_else(|| self.selection.label()),
        }
    }
}

fn is_action_key(key: KeyEvent) -> bool {
    matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
}

fn geometry(width: u16, height: u16) -> Option<Geometry> {
    if width < MIN_WIDTH || height < MIN_HEIGHT {
        return None;
    }
    let menu_width = MENU_WIDTH.min(width.saturating_sub(40));
    let menu_x = width.saturating_sub(menu_width);
    Some(Geometry {
        canvas: Rect {
            x: 1,
            y: 1,
            width: menu_x.saturating_sub(2),
            height: height.saturating_sub(3),
        },
        menu: Rect {
            x: menu_x as i32,
            y: 1,
            width: menu_width,
            height: height.saturating_sub(3),
        },
        selection_row: height - 2,
        status_row: height - 1,
    })
}

fn rects_intersect(left: Rect, right: Rect) -> bool {
    left.x <= right.right()
        && left.right() >= right.x
        && left.y <= right.bottom()
        && left.bottom() >= right.y
}

fn draw_node(
    frame: &mut Frame,
    canvas: Rect,
    node: &LayoutNode,
    rect: Rect,
    selected: bool,
) {
    let base = match node.kind {
        NodeKind::Database => Color::Cyan,
        NodeKind::Table => Color::Yellow,
        NodeKind::Row => Color::Green,
    };
    let border = if selected {
        Style::new(Color::Black, Color::White).bold()
    } else {
        Style::new(base, Color::Reset).bold()
    };
    let content = if selected {
        Style::new(Color::Black, Color::White)
    } else {
        Style::new(Color::White, Color::Reset)
    };

    for x in rect.x..=rect.right() {
        put_canvas(frame, canvas, x, rect.y, '─', border);
        put_canvas(frame, canvas, x, rect.bottom(), '─', border);
    }
    for y in rect.y..=rect.bottom() {
        put_canvas(frame, canvas, rect.x, y, '│', border);
        put_canvas(frame, canvas, rect.right(), y, '│', border);
    }
    put_canvas(frame, canvas, rect.x, rect.y, '┌', border);
    put_canvas(frame, canvas, rect.right(), rect.y, '┐', border);
    put_canvas(frame, canvas, rect.x, rect.bottom(), '└', border);
    put_canvas(frame, canvas, rect.right(), rect.bottom(), '┘', border);

    let title = format!(" {} ", clip_text_cells(&node.title, rect.width.saturating_sub(4) as usize));
    put_canvas_str(frame, canvas, rect.x + 2, rect.y, &title, border);
    let detail = clip_text_cells(&node.detail, rect.width.saturating_sub(4) as usize);
    put_canvas_str(frame, canvas, rect.x + 2, rect.y + 1, &detail, content);
}

fn put_canvas(frame: &mut Frame, canvas: Rect, x: i32, y: i32, ch: char, style: Style) {
    if canvas.contains(x, y) {
        frame.put_i32(x, y, ch, style);
    }
}

fn put_canvas_str(
    frame: &mut Frame,
    canvas: Rect,
    x: i32,
    y: i32,
    text: &str,
    style: Style,
) {
    if y < canvas.y || y > canvas.bottom() {
        return;
    }
    let mut column = x;
    for ch in text.chars() {
        let width = terminal_cell_width(ch) as i32;
        if column > canvas.right() {
            break;
        }
        if column >= canvas.x && column + width - 1 <= canvas.right() {
            frame.put_display_char(column as u16, y as u16, ch, style);
        }
        column += width;
    }
}

fn draw_canvas_hline(
    frame: &mut Frame,
    canvas: Rect,
    start_x: i32,
    y: i32,
    end_x: i32,
    ch: char,
    style: Style,
) {
    let (start, end) = if start_x <= end_x {
        (start_x, end_x)
    } else {
        (end_x, start_x)
    };
    for x in start..=end {
        put_canvas(frame, canvas, x, y, ch, style);
    }
}

fn draw_canvas_vline(
    frame: &mut Frame,
    canvas: Rect,
    x: i32,
    start_y: i32,
    end_y: i32,
    ch: char,
    style: Style,
) {
    let (start, end) = if start_y <= end_y {
        (start_y, end_y)
    } else {
        (end_y, start_y)
    };
    for y in start..=end {
        put_canvas(frame, canvas, x, y, ch, style);
    }
}

fn fill_screen_row(frame: &mut Frame, rect: Rect, y: i32, ch: char, style: Style) {
    for x in rect.x..=rect.right() {
        frame.put_i32(x, y, ch, style);
    }
}

fn fill_row(frame: &mut Frame, y: u16, width: u16, style: Style) {
    for x in 0..width {
        frame.put(x, y, ' ', style);
    }
}

fn draw_modal(frame: &mut Frame, modal: &Modal, width: u16, height: u16) {
    let modal_height = match modal {
        Modal::Help => 18_u16.min(height.saturating_sub(4)),
        _ => 8_u16.min(height.saturating_sub(4)),
    };
    let modal_width = 72_u16.min(width.saturating_sub(6)).max(36);
    let x = width.saturating_sub(modal_width) / 2;
    let y = height.saturating_sub(modal_height) / 2;
    let rect = Rect {
        x: x as i32,
        y: y as i32,
        width: modal_width,
        height: modal_height,
    };
    let background = Style::new(Color::White, Color::DarkBlue);
    for row in rect.y..=rect.bottom() {
        for column in rect.x..=rect.right() {
            frame.put_i32(column, row, ' ', background);
        }
    }
    draw_modal_box(frame, rect, background);

    match modal {
        Modal::Input(input) => {
            draw_modal_title(frame, rect, &input.title);
            frame.put_str_i32(
                rect.x + 3,
                rect.y + 2,
                &clip_text_cells(&input.prompt, rect.width.saturating_sub(6) as usize),
                background.bold(),
            );
            draw_input_line(frame, rect, input);
            frame.put_str_i32(
                rect.x + 3,
                rect.bottom() - 1,
                "Enter accept ・ Esc cancel ・ prefix bytes with hex:",
                background.dim(),
            );
        }
        Modal::Confirm(confirm) => {
            draw_modal_title(frame, rect, &confirm.title);
            frame.put_str_i32(
                rect.x + 3,
                rect.y + 2,
                &clip_text_cells(&confirm.question, rect.width.saturating_sub(6) as usize),
                background.bold(),
            );
            frame.put_str_i32(
                rect.x + 3,
                rect.y + 4,
                "[Y] confirm     [N] cancel",
                Style::new(Color::Yellow, Color::DarkBlue).bold(),
            );
        }
        Modal::Help => {
            draw_modal_title(frame, rect, "Help");
            let lines = [
                "tredb is an in-process redb explorer. Nothing is written to disk.",
                "",
                "LMB select ・ MMB drag pan ・ wheel vertical pan",
                "W/A/S/D or arrows pan ・ Home centers",
                "Tab / Shift+Tab cycles menu sections",
                "0..9 executes the local menu item",
                "Enter runs the context default ・ Delete removes selection",
                "R refreshes the owned snapshot ・ Ctrl-Q or Esc exits",
                "",
                "Text is UTF-8. Use hex:00 ff 7a for arbitrary bytes.",
                "Every user table is &[u8] → &[u8].",
                "",
                "Enter, Esc, or Q closes this help.",
            ];
            for (index, line) in lines.iter().enumerate() {
                if rect.y + 2 + index as i32 >= rect.bottom() {
                    break;
                }
                frame.put_str_i32(
                    rect.x + 3,
                    rect.y + 2 + index as i32,
                    &clip_text_cells(line, rect.width.saturating_sub(6) as usize),
                    background,
                );
            }
        }
    }
}

fn draw_modal_box(frame: &mut Frame, rect: Rect, style: Style) {
    for x in rect.x..=rect.right() {
        frame.put_i32(x, rect.y, '─', style.bold());
        frame.put_i32(x, rect.bottom(), '─', style.bold());
    }
    for y in rect.y..=rect.bottom() {
        frame.put_i32(rect.x, y, '│', style.bold());
        frame.put_i32(rect.right(), y, '│', style.bold());
    }
    frame.put_i32(rect.x, rect.y, '┌', style.bold());
    frame.put_i32(rect.right(), rect.y, '┐', style.bold());
    frame.put_i32(rect.x, rect.bottom(), '└', style.bold());
    frame.put_i32(rect.right(), rect.bottom(), '┘', style.bold());
}

fn draw_modal_title(frame: &mut Frame, rect: Rect, title: &str) {
    let title = format!(" {} ", clip_text_cells(title, rect.width.saturating_sub(6) as usize));
    frame.put_str_i32(
        rect.x + 3,
        rect.y,
        &title,
        Style::new(Color::Yellow, Color::DarkBlue).bold(),
    );
}

fn draw_input_line(frame: &mut Frame, rect: Rect, input: &InputModal) {
    let line_x = rect.x + 3;
    let line_y = rect.y + 4;
    let available = rect.width.saturating_sub(6) as usize;
    let total_chars = input.text.chars().count();
    let start = input.cursor.saturating_sub(available / 2);
    let end = (start + available).min(total_chars);
    let chars: Vec<char> = input.text.chars().collect();
    let normal = Style::new(Color::White, Color::Black);
    let cursor = Style::new(Color::Black, Color::White).bold();

    for offset in 0..available {
        frame.put_i32(line_x + offset as i32, line_y, ' ', normal);
    }

    let mut column = line_x;
    for (index, ch) in chars[start..end].iter().enumerate() {
        let actual = start + index;
        let style = if actual == input.cursor { cursor } else { normal };
        frame.put_display_char(column as u16, line_y as u16, *ch, style);
        column += terminal_cell_width(*ch) as i32;
        if column >= rect.right() - 2 {
            break;
        }
    }
    if input.cursor >= end && column < rect.right() - 2 {
        frame.put_i32(column, line_y, ' ', cursor);
    }
}
