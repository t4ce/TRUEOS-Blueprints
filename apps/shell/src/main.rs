#![no_std]

extern crate alloc;

mod terminal;

use alloc::vec::Vec;

use terminal::Terminal;
use trueos::input::{self, TrueosKeyboardOutputEvent};
use trueos::logl::{self, level};
use trueos::ui4_scene::{Damage, Error as UiError, Font, Frame, SceneTextRow, rgba};
use trueos::vshell::{SHELL2_FRONTEND_READ_DROPPED, Shell2Frontend, Shell2FrontendError};
use trueos::vsys;

// The terminal intentionally has no font metrics protocol yet. Shell2 wraps at
// this row width, and UI4 positions one Inconsolata glyph in each logical cell.
const ROW_HEIGHT_PX: u32 = 20;
const CHARACTERS_PER_ROW_SOFT_CAP: usize = 100;

const FRAME_X: i32 = 0;
const FRAME_Y: i32 = 0;
const FRAME_WIDTH: u32 = 1_024;
const FRAME_HEIGHT: u32 = 576;
const FRAME_PADDING_PX: u32 = 12;
const FONT_PIXELS: f32 = 18.0;
const MONO_GLYPH_ADVANCE_PX: f32 = FONT_PIXELS * 0.5;
const TERMINAL_ROWS: usize = ((FRAME_HEIGHT - FRAME_PADDING_PX * 2) / ROW_HEIGHT_PX) as usize;
const TERMINAL_COLS: usize = CHARACTERS_PER_ROW_SOFT_CAP;

const BACKGROUND: u32 = rgba(0, 0, 0, 255);
const FOREGROUND: u32 = rgba(238, 238, 238, 255);
const POLL_INTERVAL_MS: u64 = 5;
const SHELL_OUTPUT_BATCH_CAP: usize = 8 * 1024;
const SHELL_ATTACH_RETRIES: usize = 1_000;
const HID_MODIFIER_LEFT_CONTROL: u8 = 1 << 0;
const HID_MODIFIER_RIGHT_CONTROL: u8 = 1 << 4;
const HID_MODIFIER_CONTROL_MASK: u8 = HID_MODIFIER_LEFT_CONTROL | HID_MODIFIER_RIGHT_CONTROL;

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
enum KeyboardInputError {
    Ui(UiError),
    Shell(Shell2FrontendError),
}

impl From<UiError> for KeyboardInputError {
    fn from(error: UiError) -> Self {
        Self::Ui(error)
    }
}

impl From<Shell2FrontendError> for KeyboardInputError {
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

    let mut frontend = match attach_shell_frontend() {
        Ok(frontend) => frontend,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("shell: local shell2 session attach failed: {error:?}"),
            );
            return;
        }
    };
    let mut terminal = Terminal::new(TERMINAL_COLS, TERMINAL_ROWS);
    let mut keyboard_input = KeyboardInputState::default();

    if let Err(error) = present_terminal(&mut frame, &terminal) {
        logl::log(
            level::ERROR,
            format_args!("shell: first UI4 terminal frame failed: {error:?}"),
        );
        return;
    }
    let _ = terminal.take_dirty();
    logl::log(
        level::INFO,
        format_args!(
            "shell: local shell2 session online cols={} rows={} font=Inconsolata",
            TERMINAL_COLS, TERMINAL_ROWS
        ),
    );

    loop {
        if let Err(error) = drain_shell_output(&mut frontend, &mut terminal) {
            logl::log(
                level::ERROR,
                format_args!("shell: local shell2 output failed: {error:?}"),
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

        if terminal.take_dirty()
            && let Err(error) = present_terminal(&mut frame, &terminal)
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

fn attach_shell_frontend() -> Result<Shell2Frontend, Shell2FrontendError> {
    for attempt in 0..SHELL_ATTACH_RETRIES {
        match Shell2Frontend::attach(TERMINAL_COLS as u32, TERMINAL_ROWS as u32) {
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
) -> Result<(), Shell2FrontendError> {
    let mut bytes = [0u8; SHELL_OUTPUT_BATCH_CAP];
    for _ in 0..32 {
        let read = frontend.read(&mut bytes)?;
        if read.epoch_changed || read.flags & SHELL2_FRONTEND_READ_DROPPED != 0 {
            terminal.reset();
        }
        if read.len != 0 {
            terminal.feed(&bytes[..read.len]);
        }
        if read.len < bytes.len() {
            break;
        }
    }
    Ok(())
}

fn drain_keyboard_input(
    frame: &mut Frame,
    state: &mut KeyboardInputState,
    frontend: &Shell2Frontend,
) -> Result<(), KeyboardInputError> {
    while let Some(event) = frame.take_keyboard_event()? {
        handle_keyboard_event(state, frontend, event)?;
    }
    Ok(())
}

fn handle_keyboard_event(
    state: &mut KeyboardInputState,
    frontend: &Shell2Frontend,
    event: TrueosKeyboardOutputEvent,
) -> Result<(), KeyboardInputError> {
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

fn present_terminal(frame: &mut Frame, terminal: &Terminal) -> Result<(), UiError> {
    let mut rendered = terminal.render_rows();
    for row in &mut rendered {
        while row.as_bytes().last().copied() == Some(b' ') {
            row.pop();
        }
    }

    retry_busy(|| frame.begin(BACKGROUND))?;

    // A retain call is one cached UI4 layer. Keep one stable layer per
    // terminal row so editing a glyph does not invalidate every other row.
    for (row, text) in rendered.iter().enumerate() {
        let empty = text.is_empty();
        let scene = [SceneTextRow {
            // UI4 rejects empty and coverage-free retained runs. A black dot
            // keeps an empty row's layer present without becoming visible.
            text: if empty { "." } else { text.as_str() },
            x: FRAME_PADDING_PX as f32,
            y: FRAME_PADDING_PX as f32 + row as f32 * ROW_HEIGHT_PX as f32,
            font_pixels: FONT_PIXELS,
        }];
        retry_busy(|| {
            frame.retain_text_scene(
                Font::Inconsolata,
                (FRAME_WIDTH, FRAME_HEIGHT),
                if empty { BACKGROUND } else { FOREGROUND },
                &scene,
            )
        })?;
    }

    // Inconsolata's advance is one half-em. Keeping the cursor as a single
    // retained glyph lets UI4 reuse its mask as it translates between cells.
    let cursor = terminal.cursor();
    let cursor_scene = [SceneTextRow {
        text: "_",
        x: FRAME_PADDING_PX as f32 + cursor.col as f32 * MONO_GLYPH_ADVANCE_PX,
        y: FRAME_PADDING_PX as f32 + cursor.row as f32 * ROW_HEIGHT_PX as f32,
        font_pixels: FONT_PIXELS,
    }];
    retry_busy(|| {
        frame.retain_text_scene(
            Font::Inconsolata,
            (FRAME_WIDTH, FRAME_HEIGHT),
            if cursor.visible {
                FOREGROUND
            } else {
                BACKGROUND
            },
            &cursor_scene,
        )
    })?;

    retry_busy(|| frame.publish(Damage::full(FRAME_WIDTH, FRAME_HEIGHT)))
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
