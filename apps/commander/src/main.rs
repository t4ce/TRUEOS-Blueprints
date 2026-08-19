use std::io::{self, Write};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{
        self, DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    },
    execute, queue,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};

use trueos::{
    hid::{
        InputCombo, InputComboSourceKind, KeyboardControlCommand, MouseMotionCommand, VCursor,
        VKeyboard, KEYBOARD_CONTROL_OPCODE_STROKE, MOUSE_MOTION_FLAG_CLEAR_QUEUE,
        MOUSE_MOTION_OPCODE_TELEPORT,
    },
    logl::{self, level},
    ui4_scene::output_dimensions,
    vshell,
};

const LOG_TARGET: &str = "commander";
const COMMANDER_EXIT_CHAR: char = ']';

// USB HID keyboard usages expected by KeyboardControlCommand::key_code.
const HID_ENTER: u16 = 0x28;
const HID_ESCAPE: u16 = 0x29;
const HID_BACKSPACE: u16 = 0x2a;
const HID_TAB: u16 = 0x2b;
const HID_F1: u16 = 0x3a;
const HID_INSERT: u16 = 0x49;
const HID_HOME: u16 = 0x4a;
const HID_PAGE_UP: u16 = 0x4b;
const HID_DELETE: u16 = 0x4c;
const HID_END: u16 = 0x4d;
const HID_PAGE_DOWN: u16 = 0x4e;
const HID_RIGHT: u16 = 0x4f;
const HID_LEFT: u16 = 0x50;
const HID_DOWN: u16 = 0x51;
const HID_UP: u16 = 0x52;

// HID modifier-byte bits.
const HID_MOD_LCTRL: u8 = 1 << 0;
const HID_MOD_LSHIFT: u8 = 1 << 1;
const HID_MOD_LALT: u8 = 1 << 2;

// TRUEOS/UI4 pointer button mask.
const BUTTON_LEFT: u32 = 1 << 0;
const BUTTON_RIGHT: u32 = 1 << 1;
const BUTTON_MIDDLE: u32 = 1 << 2;

type CommanderResult<T> = Result<T, String>;

fn main() {
    let lease = match vshell::terminal_initial_lease() {
        Ok(lease) => lease,
        Err(error) => {
            diag(
                level::ERROR,
                format_args!("terminal lease unavailable: {error}"),
            );
            let _ = vshell::report_exit_reason("commander terminal lease unavailable");
            let _ = vshell::shutdown_current_blueprint("commander terminal lease unavailable");
            return;
        }
    };

    let result = run_commander(&lease);

    match &result {
        Ok(()) => {
            diag(level::INFO, "control session ended by user");
            let _ = vshell::report_exit_reason("commander user exit");
        }
        Err(error) => {
            diag(level::ERROR, format_args!("session failed: {error}"));
            let _ = vshell::report_exit_reason("commander session failed");
        }
    }

    // run_commander() owns the Crossterm guard and VLayer devices.
    // They are completely restored/released before we return the terminal lease.
    match lease.release_to_shell() {
        Ok(_ticket) => {
            let reason = if result.is_ok() {
                "commander terminated"
            } else {
                "commander terminated after session error"
            };
            let _ = vshell::shutdown_current_blueprint(reason);
        }
        Err(error) => {
            diag(
                level::ERROR,
                format_args!("terminal lease release failed: {error}"),
            );
            let _ = vshell::shutdown_current_blueprint("commander terminal release failed");
        }
    }
}

fn run_commander(lease: &vshell::TerminalLease) -> CommanderResult<()> {
    let _terminal = TerminalGuard::enter()
        .map_err(|error| format!("terminal setup failed: {error}"))?;

    let (cols, rows) =
        terminal::size().map_err(|error| format!("terminal size failed: {error}"))?;
    let mut geometry = TerminalGeometry::new(cols, rows);

    let (output_width, output_height) = output_dimensions()
        .map_err(|error| format!("TRUEOS output dimensions unavailable: {error:?}"))?;

    let mut remote = RemoteCommander::new(output_width, output_height)?;

    draw_panel(&geometry, &remote)
        .map_err(|error| format!("first commander frame failed: {error}"))?;

    // The remote keyboard/cursor, Remote InputCombo, raw mode, alternate screen,
    // mouse tracking and first frame all exist before this exact lease is ready.
    lease
        .acknowledge_ready()
        .map_err(|error| format!("terminal ready acknowledgement failed: {error}"))?;

    diag(
        level::INFO,
        format_args!(
            "ready combo={} keyboard_slot={} cursor_slot={} terminal={}x{} output={}x{}",
            remote.combo_id(),
            remote.keyboard_slot(),
            remote.cursor_slot(),
            geometry.cols,
            geometry.rows,
            remote.output_width,
            remote.output_height,
        ),
    );

    // Narrow downstream proof: this does not depend on terminal RX or Crossterm.
    // If the TRUEOS cursor moves to the upper-left quarter, VLayer VMCall ->
    // mouse-control -> vcursor ring -> UI4 is working before any user input.
    remote.startup_cursor_self_test()?;

    let mut input_event_seq = 0u64;

    loop {
        let terminal_event =
            event::read().map_err(|error| format!("crossterm read failed: {error}"))?;

        match terminal_event {
            Event::Key(key) => {
                input_event_seq = input_event_seq.wrapping_add(1);
                diag(
                    level::INFO,
                    format_args!(
                        "rx seq={} event=key code={:?} kind={:?} modifiers={:?}",
                        input_event_seq, key.code, key.kind, key.modifiers
                    ),
                );
                if commander_exit_key(key) {
                    diag(level::INFO, "rx commander-exit");
                    break;
                }
                remote.forward_key(key)?;
                diag(
                    level::INFO,
                    format_args!("submit seq={} sink=keyboard result=ok", input_event_seq),
                );
            }
            Event::Mouse(mouse) => {
                input_event_seq = input_event_seq.wrapping_add(1);
                diag(
                    level::INFO,
                    format_args!(
                        "rx seq={} event=mouse kind={:?} col={} row={} modifiers={:?}",
                        input_event_seq,
                        mouse.kind,
                        mouse.column,
                        mouse.row,
                        mouse.modifiers
                    ),
                );
                remote.forward_mouse(mouse, geometry)?;
                diag(
                    level::INFO,
                    format_args!("submit seq={} sink=cursor result=ok", input_event_seq),
                );
            }
            Event::Resize(cols, rows) => {
                geometry = TerminalGeometry::new(cols, rows);
                diag(
                    level::INFO,
                    format_args!("rx event=resize cols={} rows={}", cols, rows),
                );

                if let Ok((width, height)) = output_dimensions() {
                    remote.set_output_dimensions(width, height);
                }

                draw_panel(&geometry, &remote)
                    .map_err(|error| format!("commander resize frame failed: {error}"))?;
            }
            Event::FocusLost => {
                // Never retain a remote button if the controlling terminal loses focus.
                remote.release_all_buttons()?;
            }
            Event::FocusGained => {}
            _ => {}
        }
    }

    remote.release_all_buttons()?;
    Ok(())
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;

        let mut out = io::stdout();
        if let Err(error) = execute!(
            &mut out,
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableFocusChange,
            Hide,
            Clear(ClearType::All),
            MoveTo(0, 0)
        ) {
            let _ = terminal::disable_raw_mode();
            return Err(error);
        }

        // Commander wants pointer motion even when no button is held.
        // 1003 = any-event tracking, 1006 = SGR coordinates.
        if let Err(error) = out.write_all(b"\x1b[?1003h\x1b[?1006h") {
            restore_terminal_output(&mut out);
            let _ = terminal::disable_raw_mode();
            return Err(error);
        }
        out.flush()?;

        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut out = io::stdout();

        let _ = out.write_all(b"\x1b[?1003l");
        restore_terminal_output(&mut out);
        let _ = terminal::disable_raw_mode();
    }
}

fn restore_terminal_output(out: &mut io::Stdout) {
    let _ = execute!(
        out,
        ResetColor,
        SetAttribute(Attribute::Reset),
        DisableFocusChange,
        DisableMouseCapture,
        Show,
        LeaveAlternateScreen
    );
    let _ = out.flush();
}

#[derive(Clone, Copy)]
struct TerminalGeometry {
    cols: u16,
    rows: u16,
}

impl TerminalGeometry {
    fn new(cols: u16, rows: u16) -> Self {
        Self {
            cols: cols.max(1),
            rows: rows.max(1),
        }
    }

    fn map_pointer(self, column: u16, row: u16, width: u32, height: u32) -> (i32, i32) {
        (
            scale_axis(column, self.cols, width),
            scale_axis(row, self.rows, height),
        )
    }
}

fn scale_axis(cell: u16, cells: u16, pixels: u32) -> i32 {
    if cells <= 1 || pixels <= 1 {
        return 0;
    }

    let cell = u64::from(cell.min(cells - 1));
    let cell_max = u64::from(cells - 1);
    let pixel_max = u64::from(pixels - 1);

    ((cell * pixel_max) / cell_max).min(i32::MAX as u64) as i32
}

struct RemoteCommander {
    combo: Option<InputCombo>,
    keyboard: VKeyboard,
    cursor: VCursor,
    output_width: u32,
    output_height: u32,
    x: i32,
    y: i32,
    buttons_down: u32,
}

impl RemoteCommander {
    fn new(output_width: u32, output_height: u32) -> CommanderResult<Self> {
        let keyboard = VKeyboard::request("commander-remote-keyboard")
            .map_err(|error| format!("remote keyboard request failed: {error}"))?;

        let cursor = VCursor::request("commander-remote-cursor")
            .map_err(|error| format!("remote cursor request failed: {error}"))?;

        let combo = InputCombo::request(
            "commander-terminal-control",
            InputComboSourceKind::Remote,
            None,
        )
        .map_err(|error| format!("remote input combo request failed: {error}"))?;

        if let Err(error) = combo.bind_keyboard(&keyboard) {
            let _ = combo.remove();
            return Err(format!("remote keyboard bind failed: {error}"));
        }

        if let Err(error) = combo.bind_cursor(&cursor) {
            let _ = combo.remove();
            return Err(format!("remote cursor bind failed: {error}"));
        }

        let x = (output_width / 2).min(i32::MAX as u32) as i32;
        let y = (output_height / 2).min(i32::MAX as u32) as i32;

        Ok(Self {
            combo: Some(combo),
            keyboard,
            cursor,
            output_width: output_width.max(1),
            output_height: output_height.max(1),
            x,
            y,
            buttons_down: 0,
        })
    }

    fn combo_id(&self) -> u32 {
        self.combo.as_ref().map_or(0, InputCombo::id)
    }

    fn keyboard_slot(&self) -> u32 {
        self.keyboard.slot_id()
    }

    fn cursor_slot(&self) -> u32 {
        self.cursor.slot_id()
    }

    fn set_output_dimensions(&mut self, width: u32, height: u32) {
        self.output_width = width.max(1);
        self.output_height = height.max(1);
        self.x = self
            .x
            .clamp(0, self.output_width.saturating_sub(1).min(i32::MAX as u32) as i32);
        self.y = self
            .y
            .clamp(0, self.output_height.saturating_sub(1).min(i32::MAX as u32) as i32);
    }

    fn startup_cursor_self_test(&mut self) -> CommanderResult<()> {
        let x = (self.output_width / 4).min(i32::MAX as u32) as i32;
        let y = (self.output_height / 4).min(i32::MAX as u32) as i32;
        diag(
            level::INFO,
            format_args!(
                "selftest sink=cursor action=teleport begin x={} y={} slot={}",
                x,
                y,
                self.cursor.slot_id()
            ),
        );
        self.cursor
            .submit(MouseMotionCommand {
                opcode: MOUSE_MOTION_OPCODE_TELEPORT,
                flags: MOUSE_MOTION_FLAG_CLEAR_QUEUE,
                x,
                y,
                ..MouseMotionCommand::default()
            })
            .map_err(|error| format!("startup cursor self-test submit failed: {error}"))?;
        self.x = x;
        self.y = y;
        diag(
            level::INFO,
            format_args!(
                "selftest sink=cursor action=teleport submit=ok x={} y={} slot={}",
                x,
                y,
                self.cursor.slot_id()
            ),
        );
        Ok(())
    }

    fn forward_key(&self, key: KeyEvent) -> CommanderResult<()> {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return Ok(());
        }

        let mut command = KeyboardControlCommand {
            opcode: KEYBOARD_CONTROL_OPCODE_STROKE,
            modifiers: hid_modifiers(key.modifiers),
            ..KeyboardControlCommand::default()
        };

        match key.code {
            KeyCode::Char(ch) => {
                command.codepoint = ch as u32;
            }
            KeyCode::Enter => command.key_code = HID_ENTER,
            KeyCode::Esc => command.key_code = HID_ESCAPE,
            KeyCode::Backspace => command.key_code = HID_BACKSPACE,
            KeyCode::Tab => command.key_code = HID_TAB,
            KeyCode::BackTab => {
                command.key_code = HID_TAB;
                command.modifiers |= HID_MOD_LSHIFT;
            }
            KeyCode::Insert => command.key_code = HID_INSERT,
            KeyCode::Delete => command.key_code = HID_DELETE,
            KeyCode::Home => command.key_code = HID_HOME,
            KeyCode::End => command.key_code = HID_END,
            KeyCode::PageUp => command.key_code = HID_PAGE_UP,
            KeyCode::PageDown => command.key_code = HID_PAGE_DOWN,
            KeyCode::Left => command.key_code = HID_LEFT,
            KeyCode::Right => command.key_code = HID_RIGHT,
            KeyCode::Up => command.key_code = HID_UP,
            KeyCode::Down => command.key_code = HID_DOWN,
            KeyCode::F(number) if (1..=12).contains(&number) => {
                command.key_code = HID_F1 + u16::from(number - 1);
            }
            _ => return Ok(()),
        }

        self.keyboard
            .submit(command)
            .map_err(|error| format!("remote keyboard submit failed: {error}"))
    }

    fn forward_mouse(
        &mut self,
        mouse: MouseEvent,
        geometry: TerminalGeometry,
    ) -> CommanderResult<()> {
        let (x, y) = geometry.map_pointer(
            mouse.column,
            mouse.row,
            self.output_width,
            self.output_height,
        );
        self.x = x;
        self.y = y;

        let before = self.buttons_down;
        let mut wheel = 0i16;

        match mouse.kind {
            MouseEventKind::Down(button) => {
                self.buttons_down |= mouse_button_mask(button);
            }
            MouseEventKind::Up(button) => {
                self.buttons_down &= !mouse_button_mask(button);
            }
            MouseEventKind::Drag(button) => {
                self.buttons_down |= mouse_button_mask(button);
            }
            MouseEventKind::Moved => {}
            MouseEventKind::ScrollUp => wheel = 1,
            MouseEventKind::ScrollDown => wheel = -1,
            MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight => return Ok(()),
        }

        let buttons_set = self.buttons_down & !before;
        let buttons_clear = before & !self.buttons_down;

        diag(
            level::INFO,
            format_args!(
                "translate event=mouse kind={:?} cell={},{} output={},{} buttons_set=0x{:x} buttons_clear=0x{:x} wheel={}",
                mouse.kind,
                mouse.column,
                mouse.row,
                x,
                y,
                buttons_set,
                buttons_clear,
                wheel
            ),
        );

        self.cursor
            .submit(MouseMotionCommand {
                opcode: MOUSE_MOTION_OPCODE_TELEPORT,
                x,
                y,
                buttons_set,
                buttons_clear,
                wheel,
                ..MouseMotionCommand::default()
            })
            .map_err(|error| format!("remote cursor submit failed: {error}"))
    }

    fn release_all_buttons(&mut self) -> CommanderResult<()> {
        if self.buttons_down == 0 {
            return Ok(());
        }

        let buttons_clear = self.buttons_down;
        self.buttons_down = 0;

        self.cursor
            .submit(MouseMotionCommand {
                opcode: MOUSE_MOTION_OPCODE_TELEPORT,
                x: self.x,
                y: self.y,
                buttons_clear,
                ..MouseMotionCommand::default()
            })
            .map_err(|error| format!("remote cursor button release failed: {error}"))
    }
}

impl Drop for RemoteCommander {
    fn drop(&mut self) {
        if let Some(combo) = self.combo.take() {
            let _ = combo.remove();
        }
    }
}

fn commander_exit_key(key: KeyEvent) -> bool {
    matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
        && key.code == KeyCode::Char(COMMANDER_EXIT_CHAR)
        && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn hid_modifiers(modifiers: KeyModifiers) -> u8 {
    let mut out = 0u8;

    if modifiers.contains(KeyModifiers::CONTROL) {
        out |= HID_MOD_LCTRL;
    }
    if modifiers.contains(KeyModifiers::SHIFT) {
        out |= HID_MOD_LSHIFT;
    }
    if modifiers.contains(KeyModifiers::ALT) {
        out |= HID_MOD_LALT;
    }

    out
}

fn mouse_button_mask(button: MouseButton) -> u32 {
    match button {
        MouseButton::Left => BUTTON_LEFT,
        MouseButton::Right => BUTTON_RIGHT,
        MouseButton::Middle => BUTTON_MIDDLE,
    }
}

fn draw_panel(geometry: &TerminalGeometry, remote: &RemoteCommander) -> io::Result<()> {
    let mut out = io::stdout();

    queue!(
        &mut out,
        Clear(ClearType::All),
        MoveTo(0, 0),
        SetForegroundColor(Color::Cyan),
        SetAttribute(Attribute::Bold),
        Print("TRUEOS COMMANDER"),
        SetAttribute(Attribute::Reset),
        ResetColor,
        Print("\r\n\r\n"),
        Print("This terminal is now a Remote VLayer keyboard + pointer.\r\n"),
        Print("Crossterm decodes the terminal control stream; Commander translates\r\n"),
        Print("those logical key/mouse events into the existing VLayer devices.\r\n\r\n"),
        SetForegroundColor(Color::DarkGrey),
        Print(format!(
            "terminal surface : {} x {} cells\r\n",
            geometry.cols, geometry.rows
        )),
        Print(format!(
            "TRUEOS output    : {} x {} pixels\r\n",
            remote.output_width, remote.output_height
        )),
        Print(format!("InputCombo       : {} (Remote)\r\n", remote.combo_id())),
        Print(format!("VKeyboard slot   : {}\r\n", remote.keyboard_slot())),
        Print(format!("VCursor slot     : {}\r\n", remote.cursor_slot())),
        ResetColor,
        Print("\r\n"),
        Print("Move/click/drag/wheel here to drive the TRUEOS virtual cursor.\r\n"),
        Print("Type here to drive the TRUEOS virtual keyboard.\r\n"),
        Print("Terminal cell coordinates are normalized across the TRUEOS output.\r\n\r\n"),
        SetForegroundColor(Color::Yellow),
        SetAttribute(Attribute::Bold),
        Print("Ctrl-]  release Commander and return to Shell2"),
        SetAttribute(Attribute::Reset),
        ResetColor,
    )?;

    out.flush()
}

fn diag(message_level: u8, message: impl logl::IntoLogMessage) {
    let _ = logl::log_record(message_level, LOG_TARGET, message);
}
