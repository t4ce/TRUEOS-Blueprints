#![no_std]

extern crate alloc;

mod terminal;

use alloc::{string::String, vec::Vec};

use terminal::{ForegroundColor, MouseButton, Terminal};
use trueos::input::{self, TrueosKeyboardOutputEvent};
use trueos::logl::{self, level};
use trueos::ui4_scene::{
    Damage, Error as UiError, Font, Frame, POINTER_BUTTON_MIDDLE, POINTER_BUTTON_PRIMARY,
    POINTER_BUTTON_SECONDARY, PointerEvent, SceneTextRow, rgba,
};
use trueos::vshell::{
    SHELL2_FRONTEND_DIRECT_HANDOFF, SHELL2_FRONTEND_READ_DROPPED, Shell2Frontend,
    Shell2FrontendError, TerminalLease, TerminalParkingTicket, TerminalReentry,
};
use trueos::vsys;

const CHARACTERS_PER_ROW_SOFT_CAP: usize = 120;
const DEFAULT_ROW_HEIGHT_PX: u32 = 26;
const DEFAULT_FONT_PIXELS: f32 = 24.0;
const DEFAULT_MONO_GLYPH_ADVANCE_PX: u32 = 12;
const DEFAULT_ZOOM_PERCENT: u16 = 100;

const FRAME_X: i32 = 0;
const FRAME_Y: i32 = 0;
const FRAME_WIDTH: u32 =
    CHARACTERS_PER_ROW_SOFT_CAP as u32 * DEFAULT_MONO_GLYPH_ADVANCE_PX + FRAME_PADDING_PX * 2;
const FRAME_HEIGHT: u32 = 576;
const FRAME_PADDING_PX: u32 = 12;

#[derive(Clone, Copy, Debug, PartialEq)]
struct TerminalMetrics {
    zoom_percent: u16,
    font_pixels: f32,
    glyph_advance_px: u32,
    row_height_px: u32,
}

impl TerminalMetrics {
    fn from_zoom_percent(percent: u16) -> Self {
        let percent = percent.clamp(50, 200);
        let scaled = |value: u32| value.saturating_mul(u32::from(percent)).saturating_add(50) / 100;
        Self {
            zoom_percent: percent,
            font_pixels: DEFAULT_FONT_PIXELS * f32::from(percent) / 100.0,
            glyph_advance_px: scaled(DEFAULT_MONO_GLYPH_ADVANCE_PX).max(1),
            row_height_px: scaled(DEFAULT_ROW_HEIGHT_PX).max(1),
        }
    }
}

impl Default for TerminalMetrics {
    fn default() -> Self {
        Self::from_zoom_percent(DEFAULT_ZOOM_PERCENT)
    }
}

const BACKGROUND: u32 = rgba(0, 0, 0, 191);
const FOREGROUND: u32 = rgba(255, 255, 255, 255);
const POLL_INTERVAL_MS: u64 = 5;
const SHELL_OUTPUT_BATCH_CAP: usize = 8 * 1024;
const SHELL_ATTACH_RETRIES: usize = 1_000;
const HID_MODIFIER_LEFT_CONTROL: u8 = 1 << 0;
const HID_MODIFIER_RIGHT_CONTROL: u8 = 1 << 4;
const HID_MODIFIER_CONTROL_MASK: u8 = HID_MODIFIER_LEFT_CONTROL | HID_MODIFIER_RIGHT_CONTROL;
const MATRIX_STATUS_ROW: usize = 1;
const MATRIX_CLICK_PREFIX_BYTE: u8 = 0xff;
const MATRIX_CLICK_SUFFIX_BYTE: u8 = 0x00;
const RETURN_TO_PARENT_BYTE: u8 = 0x1c;
const TERMINAL_RESET: &[u8] = b"\x1b[?1049l\x1b[0m\x1b[2J\x1b[H";

enum InvokingTerminal {
    Unavailable,
    Active(TerminalLease),
    Parked(TerminalParkingTicket),
}

impl InvokingTerminal {
    fn claim() -> Self {
        let Ok(lease) = trueos::vshell::terminal_initial_lease() else {
            return Self::Unavailable;
        };
        let _ = trueos::vshell::attached_write(TERMINAL_RESET);
        let _ = trueos::vshell::attached_write(
            b"shell: entered UI4 Shell2 session (Ctrl+\\ returns to parent Matrix)\r\n",
        );
        if lease.acknowledge_ready().is_err() {
            let _ = lease.release_to_shell();
            return Self::Unavailable;
        }
        Self::Active(lease)
    }

    const fn is_active(&self) -> bool {
        matches!(self, Self::Active(_))
    }

    fn park(&mut self) {
        let current = core::mem::replace(self, Self::Unavailable);
        if let Self::Active(lease) = current {
            match lease.release_to_shell() {
                Ok(ticket) => *self = Self::Parked(ticket),
                Err(error) => logl::log(
                    level::ERROR,
                    format_args!("shell: terminal lease release failed: {error}"),
                ),
            }
        } else {
            *self = current;
        }
    }

    fn poll_reentry(&mut self, frontend: &mut Shell2Frontend, terminal: &Terminal) {
        let current = core::mem::replace(self, Self::Unavailable);
        let Self::Parked(ticket) = current else {
            *self = current;
            return;
        };
        match ticket.poll_reentry() {
            Ok(TerminalReentry::Pending) => *self = Self::Parked(ticket),
            Ok(TerminalReentry::Ready(lease)) => {
                let (cols, rows) = terminal.dimensions();
                if let Err(error) = frontend.resize(cols as u32, rows as u32) {
                    logl::log(
                        level::ERROR,
                        format_args!("shell: terminal reentry repaint failed: {error:?}"),
                    );
                    let _ = lease.release_to_shell();
                    return;
                }
                let _ = trueos::vshell::attached_write(TERMINAL_RESET);
                if lease.acknowledge_ready().is_ok() {
                    *self = Self::Active(lease);
                } else {
                    let _ = lease.release_to_shell();
                }
            }
            Err(error) => logl::log(
                level::ERROR,
                format_args!("shell: terminal reentry failed: {error}"),
            ),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MatrixSlotHover {
    start_col: usize,
    end_col: usize,
    command: String,
}

fn foreground_rgba(foreground: ForegroundColor) -> u32 {
    match foreground {
        ForegroundColor::Default => FOREGROUND,
        ForegroundColor::Rgb { red, green, blue } => rgba(red, green, blue, 255),
        ForegroundColor::Indexed(index) => ansi_indexed_rgba(index),
    }
}

fn ansi_indexed_rgba(index: u8) -> u32 {
    const ANSI: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 49, 49),
        (13, 188, 121),
        (229, 229, 16),
        (36, 114, 200),
        (188, 63, 188),
        (17, 168, 205),
        (229, 229, 229),
        (102, 102, 102),
        (241, 76, 76),
        (35, 209, 139),
        (245, 245, 67),
        (59, 142, 234),
        (214, 112, 214),
        (41, 184, 219),
        (255, 255, 255),
    ];
    const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

    let (red, green, blue) = match index {
        0..=15 => ANSI[index as usize],
        16..=231 => {
            let cube = index - 16;
            (
                CUBE_LEVELS[(cube / 36) as usize],
                CUBE_LEVELS[((cube / 6) % 6) as usize],
                CUBE_LEVELS[(cube % 6) as usize],
            )
        }
        232..=255 => {
            let gray = 8 + (index - 232) * 10;
            (gray, gray, gray)
        }
    };
    rgba(red, green, blue, 255)
}

#[derive(Debug, Default)]
struct KeyboardInputState {
    suppressed_text: Option<SuppressedText>,
    text_burst: Option<PendingTextBurst>,
}

#[derive(Debug)]
struct PendingTextBurst {
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    device_seq: u32,
    next_seq: u32,
    scalars: usize,
    bytes: Vec<u8>,
}

impl PendingTextBurst {
    fn start(event: TrueosKeyboardOutputEvent, text: &[u8]) -> Self {
        let mut bytes = Vec::with_capacity(input::KEYBOARD_TEXT_BURST_MAX_SCALARS * 4);
        bytes.extend_from_slice(text);
        Self {
            controller_id: event.controller_id,
            slot_id: event.slot_id,
            ep_target: event.ep_target,
            device_seq: event.device_seq,
            next_seq: event.seq.wrapping_add(1),
            scalars: 1,
            bytes,
        }
    }

    fn accepts(&self, event: TrueosKeyboardOutputEvent) -> bool {
        self.controller_id == event.controller_id
            && self.slot_id == event.slot_id
            && self.ep_target == event.ep_target
            && self.device_seq == event.device_seq
            && self.next_seq == event.seq
    }

    fn push(&mut self, event: TrueosKeyboardOutputEvent, text: &[u8]) -> bool {
        if self.scalars >= input::KEYBOARD_TEXT_BURST_MAX_SCALARS {
            return false;
        }
        self.bytes.extend_from_slice(text);
        self.scalars += 1;
        self.next_seq = event.seq.wrapping_add(1);
        true
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InputError {
    Ui(UiError),
    Shell(Shell2FrontendError),
}

impl From<UiError> for InputError {
    fn from(error: UiError) -> Self {
        Self::Ui(error)
    }
}

impl From<Shell2FrontendError> for InputError {
    fn from(error: Shell2FrontendError) -> Self {
        Self::Shell(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SuppressedText {
    t_ms: u32,
    device_seq: u32,
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    utf8: [u8; 4],
    utf8_len: u8,
}

impl SuppressedText {
    fn from_key(event: TrueosKeyboardOutputEvent) -> Option<Self> {
        (event.utf8_len != 0 && usize::from(event.utf8_len) <= event.utf8.len()).then_some(Self {
            t_ms: event.t_ms,
            device_seq: event.device_seq,
            controller_id: event.controller_id,
            slot_id: event.slot_id,
            ep_target: event.ep_target,
            utf8: event.utf8,
            utf8_len: event.utf8_len,
        })
    }

    fn matches(self, event: TrueosKeyboardOutputEvent) -> bool {
        self.t_ms == event.t_ms
            && self.device_seq == event.device_seq
            && self.controller_id == event.controller_id
            && self.slot_id == event.slot_id
            && self.ep_target == event.ep_target
            && self.utf8_len == event.utf8_len
            && self.utf8[..usize::from(self.utf8_len)] == event.utf8[..usize::from(event.utf8_len)]
    }
}

fn main() {
    let Ok(mut frame) = Frame::open_streaming(FRAME_X, FRAME_Y, FRAME_WIDTH, FRAME_HEIGHT) else {
        logl::log(
            level::ERROR,
            "shell: UI4 streaming frame reservation failed",
        );
        return;
    };

    let mut metrics = TerminalMetrics::default();
    let (cols, rows) = terminal_grid_size(frame.width(), frame.height(), metrics);
    let mut frontend = match attach_shell_frontend(cols, rows) {
        Ok(frontend) => frontend,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("shell: local shell2 session attach failed: {error:?}"),
            );
            return;
        }
    };
    let mut terminal = Terminal::new(cols, rows);
    let mut keyboard_input = KeyboardInputState::default();
    let mut matrix_slot_hover = None;

    if let Err(error) = present_terminal(&mut frame, &terminal, metrics, None) {
        logl::log(
            level::ERROR,
            format_args!("shell: first UI4 terminal frame failed: {error:?}"),
        );
        return;
    }
    let _ = terminal.take_dirty();
    let mut invoking_terminal = InvokingTerminal::claim();
    logl::log(
        level::INFO,
        format_args!(
            "shell: local shell2 session online cols={} rows={} font=Inconsolata",
            cols, rows
        ),
    );

    loop {
        invoking_terminal.poll_reentry(&mut frontend, &terminal);
        let resized = match drain_resize_events(&mut frame, &mut frontend, &mut terminal, metrics) {
            Ok(resized) => resized,
            Err(error) => {
                logl::log(
                    level::ERROR,
                    format_args!("shell: UI4 frame resize failed: {error:?}"),
                );
                return;
            }
        };

        if let Err(error) =
            drain_shell_output(&mut frontend, &mut terminal, invoking_terminal.is_active())
        {
            logl::log(
                level::ERROR,
                format_args!("shell: local shell2 output failed: {error:?}"),
            );
            return;
        }

        if let Err(error) = drain_invoking_terminal_input(&frontend, &mut invoking_terminal) {
            logl::log(
                level::ERROR,
                format_args!("shell: invoking terminal input failed: {error:?}"),
            );
            return;
        }

        if let Some(zoom_percent) = terminal.take_zoom_percent()
            && let Err(error) = apply_terminal_zoom(
                &mut frame,
                &mut frontend,
                &mut terminal,
                &mut metrics,
                zoom_percent,
            )
        {
            logl::log(
                level::ERROR,
                format_args!("shell: terminal zoom failed: {error:?}"),
            );
            return;
        }

        if let Err(error) = drain_keyboard_input(&mut frame, &mut keyboard_input, &frontend) {
            logl::log(
                level::ERROR,
                format_args!("shell: routed keyboard input failed: {error:?}"),
            );
            return;
        }

        let mut hover_changed = match drain_pointer_input(
            &mut frame,
            &terminal,
            &frontend,
            metrics,
            &mut matrix_slot_hover,
        ) {
            Ok(changed) => changed,
            Err(error) => {
                logl::log(
                    level::ERROR,
                    format_args!("shell: routed pointer input failed: {error:?}"),
                );
                return;
            }
        };

        if terminal.mouse_tracking_enabled() && matrix_slot_hover.take().is_some() {
            // A direct terminal owner gets unmodified pointer semantics.
            // Shell2's status strip is not live during that handoff.
            hover_changed = true;
        }

        if (terminal.take_dirty() || hover_changed)
            && let Err(error) =
                present_terminal(&mut frame, &terminal, metrics, matrix_slot_hover.as_ref())
        {
            logl::log(
                level::ERROR,
                format_args!("shell: UI4 terminal frame failed: {error:?}"),
            );
            return;
        }

        vsys::poll_once();
        vsys::sleep_ms(POLL_INTERVAL_MS);
    }
}

fn drain_resize_events(
    frame: &mut Frame,
    frontend: &mut Shell2Frontend,
    terminal: &mut Terminal,
    metrics: TerminalMetrics,
) -> Result<bool, InputError> {
    let mut resized = false;
    while let Some(resize) = frame.take_resize_event()? {
        if resize.width == frame.width() && resize.height == frame.height() {
            continue;
        }
        frame.resize(resize.width, resize.height)?;
        let (old_cols, old_rows) = terminal.dimensions();
        let (old_origin_x, old_origin_y) =
            centered_terminal_origin(resize.width, resize.height, old_cols, old_rows, metrics);
        present_terminal_at(frame, terminal, old_origin_x, old_origin_y, metrics, None)?;
        let (cols, rows) = terminal_grid_size(resize.width, resize.height, metrics);
        if terminal.dimensions() != (cols, rows) {
            terminal.resize(cols, rows);
            frontend.resize(cols as u32, rows as u32)?;
            // The centered old grid is already live. Wait for Shell2's fresh
            // replay rather than publishing the new, empty terminal model.
            let _ = terminal.take_dirty();
        }
        resized = true;
        logl::log(
            level::INFO,
            format_args!(
                "shell: resized {}x{} -> {}x{} cols={} rows={}",
                resize.old_width, resize.old_height, resize.width, resize.height, cols, rows
            ),
        );
    }
    Ok(resized)
}

fn terminal_grid_size(width: u32, height: u32, metrics: TerminalMetrics) -> (usize, usize) {
    let width = width.saturating_sub(FRAME_PADDING_PX * 2);
    let height = height.saturating_sub(FRAME_PADDING_PX * 2);
    (
        (width / metrics.glyph_advance_px).max(1) as usize,
        (height / metrics.row_height_px).max(1) as usize,
    )
}

fn centered_terminal_origin(
    width: u32,
    height: u32,
    cols: usize,
    rows: usize,
    metrics: TerminalMetrics,
) -> (u32, u32) {
    let content_width = u32::try_from(cols)
        .unwrap_or(u32::MAX)
        .saturating_mul(metrics.glyph_advance_px)
        .saturating_add(FRAME_PADDING_PX * 2);
    let content_height = u32::try_from(rows)
        .unwrap_or(u32::MAX)
        .saturating_mul(metrics.row_height_px)
        .saturating_add(FRAME_PADDING_PX * 2);
    (
        width.saturating_sub(content_width) / 2,
        height.saturating_sub(content_height) / 2,
    )
}

fn apply_terminal_zoom(
    frame: &mut Frame,
    frontend: &mut Shell2Frontend,
    terminal: &mut Terminal,
    metrics: &mut TerminalMetrics,
    zoom_percent: u16,
) -> Result<bool, InputError> {
    let next = TerminalMetrics::from_zoom_percent(zoom_percent);
    if *metrics == next {
        return Ok(false);
    }

    *metrics = next;
    let (cols, rows) = terminal_grid_size(frame.width(), frame.height(), next);
    terminal.resize(cols, rows);
    frontend.resize(cols as u32, rows as u32)?;
    logl::log(
        level::INFO,
        format_args!(
            "shell: terminal zoom={} font_pixels={} cell={}x{} grid={}x{}",
            next.zoom_percent,
            next.font_pixels,
            next.glyph_advance_px,
            next.row_height_px,
            cols,
            rows,
        ),
    );
    Ok(true)
}

fn attach_shell_frontend(cols: usize, rows: usize) -> Result<Shell2Frontend, Shell2FrontendError> {
    for attempt in 0..SHELL_ATTACH_RETRIES {
        match Shell2Frontend::attach(cols as u32, rows as u32) {
            Ok(frontend) => return Ok(frontend),
            Err(Shell2FrontendError(-3)) if attempt + 1 < SHELL_ATTACH_RETRIES => {
                vsys::poll_once();
                vsys::sleep_ms(POLL_INTERVAL_MS);
            }
            Err(error) => return Err(error),
        }
    }
    Err(Shell2FrontendError(-3))
}

fn drain_shell_output(
    frontend: &mut Shell2Frontend,
    terminal: &mut Terminal,
    mirror_to_invoking_terminal: bool,
) -> Result<(), Shell2FrontendError> {
    let mut bytes = [0u8; SHELL_OUTPUT_BATCH_CAP];
    for _ in 0..32 {
        let read = frontend.read(&mut bytes)?;
        if read.epoch_changed || read.flags & SHELL2_FRONTEND_READ_DROPPED != 0 {
            terminal.reset();
        }
        if read.len != 0 {
            if mirror_to_invoking_terminal {
                let _ = trueos::vshell::attached_write(&bytes[..read.len]);
            }
            terminal.feed(&bytes[..read.len]);
        }
        let responses = terminal.take_responses();
        if read.flags & SHELL2_FRONTEND_DIRECT_HANDOFF != 0 && !responses.is_empty() {
            submit_input(frontend, responses.as_slice())?;
        }
        if read.len < bytes.len() {
            break;
        }
    }
    Ok(())
}

fn drain_invoking_terminal_input(
    frontend: &Shell2Frontend,
    invoking_terminal: &mut InvokingTerminal,
) -> Result<(), Shell2FrontendError> {
    if !invoking_terminal.is_active() {
        return Ok(());
    }

    let mut bytes = [0u8; 1024];
    for _ in 0..32 {
        let len = trueos::vshell::attached_read_available(&mut bytes);
        if len == 0 {
            break;
        }
        if let Some(exit_at) = bytes[..len]
            .iter()
            .position(|byte| *byte == RETURN_TO_PARENT_BYTE)
        {
            if exit_at != 0 {
                submit_input(frontend, &bytes[..exit_at])?;
            }
            invoking_terminal.park();
            break;
        }
        submit_input(frontend, &bytes[..len])?;
        if len < bytes.len() {
            break;
        }
    }
    Ok(())
}

fn drain_keyboard_input(
    frame: &mut Frame,
    state: &mut KeyboardInputState,
    frontend: &Shell2Frontend,
) -> Result<(), InputError> {
    while let Some(event) = frame.take_keyboard_event()? {
        handle_keyboard_event(state, frontend, event)?;
    }
    Ok(())
}

fn handle_keyboard_event(
    state: &mut KeyboardInputState,
    frontend: &Shell2Frontend,
    event: TrueosKeyboardOutputEvent,
) -> Result<(), InputError> {
    let burst_member = event.flags & input::KEYBOARD_OUTPUT_FLAG_TEXT_BURST != 0;
    let burst_start = event.flags & input::KEYBOARD_OUTPUT_FLAG_TEXT_BURST_START != 0;
    let burst_end = event.flags & input::KEYBOARD_OUTPUT_FLAG_TEXT_BURST_END != 0;

    if event.flags & input::KEYBOARD_OUTPUT_FLAG_PRESS == 0 {
        reset_incomplete_text_burst(state, "non-press event inside burst");
        state.suppressed_text = None;
        return Ok(());
    }
    if !burst_member && (burst_start || burst_end) {
        reset_incomplete_text_burst(state, "boundary without burst membership");
        warn_text_burst_protocol(event, "boundary without burst membership");
        state.suppressed_text = None;
        return Ok(());
    }
    if burst_member {
        state.suppressed_text = None;
        if event.kind != input::KEYBOARD_OUTPUT_KIND_TEXT {
            reset_incomplete_text_burst(state, "non-text event inside burst");
            warn_text_burst_protocol(event, "non-text event inside burst");
            return Ok(());
        }
        let Some(text) = event_text(&event) else {
            reset_incomplete_text_burst(state, "invalid scalar inside burst");
            warn_text_burst_protocol(event, "invalid scalar inside burst");
            return Ok(());
        };
        handle_text_burst(state, frontend, event, text, burst_start, burst_end)?;
        return Ok(());
    }

    reset_incomplete_text_burst(state, "ordinary event before burst END");
    match event.kind {
        input::KEYBOARD_OUTPUT_KIND_TEXT => {
            if state
                .suppressed_text
                .is_some_and(|suppressed| suppressed.matches(event))
            {
                state.suppressed_text = None;
                return Ok(());
            }
            state.suppressed_text = None;
            let Some(text) = event_text(&event) else {
                return Ok(());
            };
            if let Some(control) = control_ascii(event, text) {
                submit_input(frontend, core::slice::from_ref(&control))?;
            } else {
                // An ordinary key transition stays one glyph-sized operation.
                submit_input(frontend, text)?;
            }
        }
        input::KEYBOARD_OUTPUT_KIND_KEY => {
            state.suppressed_text = None;
            if let Some(sequence) = named_key_sequence(event.key_code) {
                submit_input(frontend, sequence)?;
                // Some physical reports carry both the named key and its text
                // scalar. This state crosses polling calls so a ring snapshot
                // boundary cannot duplicate Enter/Tab/Space.
                state.suppressed_text = SuppressedText::from_key(event);
            }
        }
        _ => state.suppressed_text = None,
    }
    Ok(())
}

fn drain_pointer_input(
    frame: &mut Frame,
    terminal: &Terminal,
    frontend: &Shell2Frontend,
    metrics: TerminalMetrics,
    matrix_slot_hover: &mut Option<MatrixSlotHover>,
) -> Result<bool, InputError> {
    let initial_hover = matrix_slot_hover.clone();
    while let Some(event) = frame.take_pointer_event()? {
        let (col, row) = pointer_cell(event.local_x, event.local_y, terminal.dimensions(), metrics);

        let hovered_slot =
            pointer_matrix_cell(event.local_x, event.local_y, terminal.dimensions(), metrics)
                .and_then(|(col, row)| matrix_slot_at(terminal, col, row));
        if !terminal.mouse_tracking_enabled() {
            *matrix_slot_hover = hovered_slot.clone();
            if event.buttons_pressed & POINTER_BUTTON_PRIMARY != 0
                && let Some(slot) = hovered_slot
            {
                let mut submission = Vec::with_capacity(slot.command.len() + 2);
                submission.push(MATRIX_CLICK_PREFIX_BYTE);
                submission.extend_from_slice(slot.command.as_bytes());
                submission.push(MATRIX_CLICK_SUFFIX_BYTE);
                submit_input(frontend, submission.as_slice())?;
            }
        }

        for (mask, button) in mouse_buttons() {
            if event.buttons_pressed & mask != 0
                && let Some(sequence) = terminal.mouse_button(button, true, col, row)
            {
                submit_input(frontend, sequence.as_slice())?;
            }
        }
        for (mask, button) in mouse_buttons() {
            if event.buttons_released & mask != 0
                && let Some(sequence) = terminal.mouse_button(button, false, col, row)
            {
                submit_input(frontend, sequence.as_slice())?;
            }
        }

        if event.wheel != 0 {
            let steps = usize::from(event.wheel.unsigned_abs()).min(8);
            for _ in 0..steps {
                if let Some(sequence) = terminal.mouse_wheel(event.wheel > 0, col, row) {
                    submit_input(frontend, sequence.as_slice())?;
                }
            }
        }

        if pointer_event_is_motion(event)
            && let Some(sequence) = terminal.mouse_motion(held_mouse_button(event), col, row)
        {
            submit_input(frontend, sequence.as_slice())?;
        }
    }
    Ok(*matrix_slot_hover != initial_hover)
}

fn pointer_matrix_cell(
    local_x: i32,
    local_y: i32,
    dimensions: (usize, usize),
    metrics: TerminalMetrics,
) -> Option<(usize, usize)> {
    let x = local_x.checked_sub(FRAME_PADDING_PX as i32)?;
    let y = local_y.checked_sub(FRAME_PADDING_PX as i32)?;
    if x < 0 || y < 0 {
        return None;
    }
    let col = x as u32 / metrics.glyph_advance_px;
    let row = y as u32 / metrics.row_height_px;
    (col < dimensions.0 as u32 && row < dimensions.1 as u32).then_some((col as usize, row as usize))
}

fn matrix_slot_at(terminal: &Terminal, col: usize, row: usize) -> Option<MatrixSlotHover> {
    if row != MATRIX_STATUS_ROW {
        return None;
    }
    let (cols, rows) = terminal.dimensions();
    if col >= cols || row >= rows {
        return None;
    }
    let cells = &terminal.cells()[row * cols..(row + 1) * cols];
    let start_col = (0..=col)
        .rev()
        .find(|&candidate| cells[candidate].glyph == '§')?;
    if cells[start_col..col].iter().any(|cell| cell.glyph == ' ') {
        return None;
    }
    if start_col != 0 && cells[start_col - 1].glyph != ' ' {
        return None;
    }
    let end_col = cells[start_col..]
        .iter()
        .position(|cell| cell.glyph == ' ')
        .map_or(cols, |offset| start_col + offset);
    if col >= end_col {
        return None;
    }

    let mut command = String::with_capacity(end_col - start_col + 1);
    for cell in &cells[start_col..end_col] {
        command.push(cell.glyph);
    }
    Some(MatrixSlotHover {
        start_col,
        end_col,
        command,
    })
}

fn pointer_cell(
    local_x: i32,
    local_y: i32,
    dimensions: (usize, usize),
    metrics: TerminalMetrics,
) -> (usize, usize) {
    let x = local_x.saturating_sub(FRAME_PADDING_PX as i32).max(0) as u32;
    let y = local_y.saturating_sub(FRAME_PADDING_PX as i32).max(0) as u32;
    let col = ((x / metrics.glyph_advance_px) as usize).min(dimensions.0 - 1);
    let row = ((y / metrics.row_height_px) as usize).min(dimensions.1 - 1);
    (col, row)
}

fn mouse_buttons() -> [(u32, MouseButton); 3] {
    [
        (POINTER_BUTTON_PRIMARY, MouseButton::Left),
        (POINTER_BUTTON_MIDDLE, MouseButton::Middle),
        (POINTER_BUTTON_SECONDARY, MouseButton::Right),
    ]
}

fn held_mouse_button(event: PointerEvent) -> Option<MouseButton> {
    mouse_buttons()
        .into_iter()
        .find_map(|(mask, button)| (event.buttons_down & mask != 0).then_some(button))
}

fn pointer_event_is_motion(event: PointerEvent) -> bool {
    event.wheel == 0 && event.buttons_pressed == 0 && event.buttons_released == 0
}

fn handle_text_burst(
    state: &mut KeyboardInputState,
    frontend: &Shell2Frontend,
    event: TrueosKeyboardOutputEvent,
    text: &[u8],
    start: bool,
    end: bool,
) -> Result<(), Shell2FrontendError> {
    let valid_identity = event.controller_id == 0
        && event.slot_id != 0
        && event.ep_target == 0
        && event.device_seq != 0
        && event.flags & input::KEYBOARD_OUTPUT_FLAG_SYNTHETIC != 0;
    if !valid_identity {
        reset_incomplete_text_burst(state, "invalid burst identity");
        warn_text_burst_protocol(event, "invalid burst identity");
        return Ok(());
    }

    if start {
        reset_incomplete_text_burst(state, "new START before prior END");
        state.text_burst = Some(PendingTextBurst::start(event, text));
    } else {
        let accepts = state
            .text_burst
            .as_ref()
            .is_some_and(|burst| burst.accepts(event));
        if !accepts {
            reset_incomplete_text_burst(state, "non-contiguous burst member");
            warn_text_burst_protocol(event, "non-contiguous burst member");
            return Ok(());
        }
        let pushed = state
            .text_burst
            .as_mut()
            .is_some_and(|burst| burst.push(event, text));
        if !pushed {
            reset_incomplete_text_burst(state, "burst exceeds scalar cap");
            warn_text_burst_protocol(event, "burst exceeds scalar cap");
            return Ok(());
        }
    }

    if end {
        let Some(burst) = state.text_burst.take() else {
            warn_text_burst_protocol(event, "END without active burst");
            return Ok(());
        };
        submit_input(frontend, burst.bytes.as_slice())?;
    }
    Ok(())
}

fn reset_incomplete_text_burst(state: &mut KeyboardInputState, reason: &'static str) {
    let Some(burst) = state.text_burst.take() else {
        return;
    };
    logl::log(
        level::WARN,
        format_args!(
            "shell: discarded incomplete text burst id={} scalars={} reason={reason}",
            burst.device_seq, burst.scalars
        ),
    );
}

fn warn_text_burst_protocol(event: TrueosKeyboardOutputEvent, reason: &'static str) {
    logl::log(
        level::WARN,
        format_args!(
            "shell: rejected text burst event id={} seq={} flags=0x{:08x} reason={reason}",
            event.device_seq, event.seq, event.flags
        ),
    );
}

fn event_text(event: &TrueosKeyboardOutputEvent) -> Option<&[u8]> {
    let len = usize::from(event.utf8_len);
    if len == 0 || len > event.utf8.len() {
        return None;
    }
    let text = core::str::from_utf8(&event.utf8[..len]).ok()?;
    (text.chars().count() == 1).then_some(&event.utf8[..len])
}

fn control_ascii(event: TrueosKeyboardOutputEvent, text: &[u8]) -> Option<u8> {
    if event.modifiers & HID_MODIFIER_CONTROL_MASK == 0 || text.len() != 1 {
        return None;
    }
    match text[0] {
        b'a'..=b'z' => Some(text[0] - b'a' + 1),
        b'A'..=b'Z' => Some(text[0] - b'A' + 1),
        b'@' => Some(0x00),
        b'[' => Some(0x1b),
        b'\\' => Some(0x1c),
        b']' => Some(0x1d),
        b'^' => Some(0x1e),
        b'_' => Some(0x1f),
        b'?' => Some(0x7f),
        _ => None,
    }
}

fn submit_input(frontend: &Shell2Frontend, bytes: &[u8]) -> Result<(), Shell2FrontendError> {
    if bytes.is_empty() {
        return Ok(());
    }
    loop {
        match frontend.submit_input(bytes) {
            Ok(written) if written == bytes.len() => return Ok(()),
            Ok(_) => return Err(Shell2FrontendError(-3)),
            Err(Shell2FrontendError(-5)) => {
                vsys::poll_once();
                vsys::sleep_ms(1);
            }
            Err(error) => return Err(error),
        }
    }
}

fn named_key_sequence(key_code: u16) -> Option<&'static [u8]> {
    match key_code {
        input::KEYBOARD_KEY_BACKSPACE => Some(b"\x08"),
        input::KEYBOARD_KEY_TAB => Some(b"\t"),
        input::KEYBOARD_KEY_ENTER => Some(b"\r"),
        input::KEYBOARD_KEY_ESCAPE => Some(b"\x1b"),
        input::KEYBOARD_KEY_SPACE => Some(b" "),
        input::KEYBOARD_KEY_DELETE => Some(b"\x1b[3~"),
        input::KEYBOARD_KEY_INSERT => Some(b"\x1b[2~"),
        input::KEYBOARD_KEY_HOME => Some(b"\x1b[H"),
        input::KEYBOARD_KEY_END => Some(b"\x1b[F"),
        input::KEYBOARD_KEY_PAGE_UP => Some(b"\x1b[5~"),
        input::KEYBOARD_KEY_PAGE_DOWN => Some(b"\x1b[6~"),
        input::KEYBOARD_KEY_ARROW_UP => Some(b"\x1b[A"),
        input::KEYBOARD_KEY_ARROW_DOWN => Some(b"\x1b[B"),
        input::KEYBOARD_KEY_ARROW_LEFT => Some(b"\x1b[D"),
        input::KEYBOARD_KEY_ARROW_RIGHT => Some(b"\x1b[C"),
        input::KEYBOARD_KEY_START => Some("§".as_bytes()),
        input::KEYBOARD_KEY_F1 => Some(b"\x1bOP"),
        input::KEYBOARD_KEY_F2 => Some(b"\x1bOQ"),
        input::KEYBOARD_KEY_F3 => Some(b"\x1bOR"),
        input::KEYBOARD_KEY_F4 => Some(b"\x1bOS"),
        input::KEYBOARD_KEY_F5 => Some(b"\x1b[15~"),
        input::KEYBOARD_KEY_F6 => Some(b"\x1b[17~"),
        input::KEYBOARD_KEY_F7 => Some(b"\x1b[18~"),
        input::KEYBOARD_KEY_F8 => Some(b"\x1b[19~"),
        input::KEYBOARD_KEY_F9 => Some(b"\x1b[20~"),
        input::KEYBOARD_KEY_F10 => Some(b"\x1b[21~"),
        input::KEYBOARD_KEY_F11 => Some(b"\x1b[23~"),
        input::KEYBOARD_KEY_F12 => Some(b"\x1b[24~"),
        _ => None,
    }
}

fn present_terminal(
    frame: &mut Frame,
    terminal: &Terminal,
    metrics: TerminalMetrics,
    matrix_slot_hover: Option<&MatrixSlotHover>,
) -> Result<(), UiError> {
    present_terminal_at(frame, terminal, 0, 0, metrics, matrix_slot_hover)
}

fn present_terminal_at(
    frame: &mut Frame,
    terminal: &Terminal,
    origin_x: u32,
    origin_y: u32,
    metrics: TerminalMetrics,
    matrix_slot_hover: Option<&MatrixSlotHover>,
) -> Result<(), UiError> {
    retry_busy(|| frame.begin(BACKGROUND))?;

    let viewport = (frame.width(), frame.height());

    let (cols, rows) = terminal.dimensions();
    // `retain_text_scene` identifies a retained layer by its ordinal within a
    // frame. Keep that ordering fixed at the terminal-cell granularity rather
    // than emitting style runs: adding a blank cell or changing one cell's
    // colour must not move every following cell onto a different retained
    // layer. This is deliberately a bounded visible-grid cache. It has no
    // scrollback, offscreen canvas, or framebuffer state; UI4 owns only these
    // `cols * rows * 2` resident coverage layers while this frame is alive.
    for row in 0..rows {
        let cells = &terminal.cells()[row * cols..(row + 1) * cols];
        for (col, cell) in cells.iter().enumerate() {
            let x = origin_x as f32
                + FRAME_PADDING_PX as f32
                + col as f32 * metrics.glyph_advance_px as f32;
            let y = origin_y as f32
                + FRAME_PADDING_PX as f32
                + row as f32 * metrics.row_height_px as f32;

            retain_terminal_slot(
                frame,
                viewport,
                cell.glyph,
                x,
                y,
                metrics.font_pixels,
                foreground_rgba(cell.style.foreground),
            )?;
            // Keep an underline layer for every cell. An invisible layer keeps
            // its glyph mask warm, so toggling underline changes only colour.
            retain_terminal_slot(
                frame,
                viewport,
                '_',
                x,
                y,
                metrics.font_pixels,
                if cell.style.underline {
                    foreground_rgba(cell.style.foreground)
                } else {
                    transparent_rgba(foreground_rgba(cell.style.foreground))
                },
            )?;
        }
    }

    // Reserve exactly one hover layer. Its contents may change, but that
    // change cannot perturb any terminal-cell or cursor layer indices.
    let hover = matrix_slot_hover.filter(|hover| {
        matrix_slot_at(terminal, hover.start_col, MATRIX_STATUS_ROW)
            .is_some_and(|current| current == **hover)
    });
    if let Some(hover) = hover {
        let mut underline = String::with_capacity(hover.end_col - hover.start_col);
        for _ in hover.start_col..hover.end_col {
            underline.push('_');
        }
        let scene = [SceneTextRow {
            text: underline.as_str(),
            x: origin_x as f32
                + FRAME_PADDING_PX as f32
                + hover.start_col as f32 * metrics.glyph_advance_px as f32,
            y: origin_y as f32
                + FRAME_PADDING_PX as f32
                + MATRIX_STATUS_ROW as f32 * metrics.row_height_px as f32,
            font_pixels: metrics.font_pixels,
        }];
        let (cols, _) = terminal.dimensions();
        let foreground = terminal.cells()[MATRIX_STATUS_ROW * cols + hover.start_col]
            .style
            .foreground;
        retry_busy(|| {
            frame.retain_text_scene(
                Font::Inconsolata,
                viewport,
                foreground_rgba(foreground),
                &scene,
            )
        })?;
    } else {
        retain_terminal_slot(
            frame,
            viewport,
            '_',
            origin_x as f32 + FRAME_PADDING_PX as f32,
            origin_y as f32
                + FRAME_PADDING_PX as f32
                + MATRIX_STATUS_ROW as f32 * metrics.row_height_px as f32,
            metrics.font_pixels,
            transparent_rgba(FOREGROUND),
        )?;
    }

    // Inconsolata's advance is one half-em. Keeping the cursor as a single
    // retained glyph lets UI4 reuse its mask as it translates between cells.
    let cursor = terminal.cursor();
    let cursor_scene = [SceneTextRow {
        text: "_",
        x: origin_x as f32
            + FRAME_PADDING_PX as f32
            + cursor.col as f32 * metrics.glyph_advance_px as f32,
        y: origin_y as f32
            + FRAME_PADDING_PX as f32
            + cursor.row as f32 * metrics.row_height_px as f32,
        font_pixels: metrics.font_pixels,
    }];
    retry_busy(|| {
        frame.retain_text_scene(
            Font::Inconsolata,
            viewport,
            if cursor.visible {
                FOREGROUND
            } else {
                BACKGROUND
            },
            &cursor_scene,
        )
    })?;

    retry_busy(|| frame.publish(Damage::full(viewport.0, viewport.1)))
}

fn retain_terminal_slot(
    frame: &mut Frame,
    viewport: (u32, u32),
    glyph: char,
    x: f32,
    y: f32,
    font_pixels: f32,
    color_rgba: u32,
) -> Result<(), UiError> {
    let mut text = [0; 4];
    let scene = [SceneTextRow {
        text: glyph.encode_utf8(&mut text),
        x,
        y,
        font_pixels,
    }];
    retry_busy(|| frame.retain_text_scene(Font::Inconsolata, viewport, color_rgba, &scene))
}

const fn transparent_rgba(color_rgba: u32) -> u32 {
    let [red, green, blue, _] = color_rgba.to_le_bytes();
    rgba(red, green, blue, 0)
}

fn retry_busy(mut operation: impl FnMut() -> Result<(), UiError>) -> Result<(), UiError> {
    loop {
        match operation() {
            Ok(()) => return Ok(()),
            Err(UiError::Busy) => {
                vsys::poll_once();
                vsys::sleep_ms(1);
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_xterm_indexed_palette_to_rgba() {
        assert_eq!(ansi_indexed_rgba(1), rgba(205, 49, 49, 255));
        assert_eq!(ansi_indexed_rgba(16), rgba(0, 0, 0, 255));
        assert_eq!(ansi_indexed_rgba(196), rgba(255, 0, 0, 255));
        assert_eq!(ansi_indexed_rgba(255), rgba(238, 238, 238, 255));
    }

    #[test]
    fn keeps_default_and_rgb_foregrounds_opaque() {
        assert_eq!(foreground_rgba(ForegroundColor::Default), FOREGROUND);
        assert_eq!(
            foreground_rgba(ForegroundColor::Rgb {
                red: 1,
                green: 2,
                blue: 3,
            }),
            rgba(1, 2, 3, 255)
        );
    }
}
