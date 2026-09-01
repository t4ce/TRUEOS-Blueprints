#![no_std]

extern crate alloc;

mod terminal;

use alloc::{collections::VecDeque, string::String, vec::Vec};

use terminal::{MouseButton, Terminal, TerminalColor};
use trueos::clock;
use trueos::input::{self, TrueosKeyboardOutputEvent};
use trueos::logl::{self, level};
use trueos::ui4_scene::{
    CursorStep, Damage, Error as UiError, Font, Frame, MenuEntry, POINTER_BUTTON_MIDDLE,
    POINTER_BUTTON_PRIMARY, POINTER_BUTTON_SECONDARY, PointerEvent, SceneTextRow,
    Shell2FontScaleStep, SpriteCorner, SpriteQuad, rgba, shell2_font_scale_steps,
};
use trueos::vshell::{
    SHELL2_FRONTEND_DIRECT_HANDOFF, SHELL2_FRONTEND_READ_DROPPED, Shell2Frontend,
    Shell2FrontendError, TerminalLease, TerminalParkingTicket, TerminalReentry,
};
use trueos::vsys;

const CHARACTERS_PER_ROW_SOFT_CAP: usize = 120;
const DEFAULT_ROW_HEIGHT_PX: u32 = 26;
const DEFAULT_FONT_PIXELS: u32 = 24;
/// JuliaMono's fixed advance is 600 font units per 1000-unit em. Keep the
/// ratio fractional at every scale step: rounding it per cell makes long
/// words drift over the whitespace which follows them.
const MONO_GLYPH_ADVANCE_NUMERATOR: u32 = 3;
const MONO_GLYPH_ADVANCE_DENOMINATOR: u32 = 5;

const FRAME_X: i32 = 0;
const FRAME_Y: i32 = 0;
const FRAME_WIDTH: u32 =
    CHARACTERS_PER_ROW_SOFT_CAP as u32 * DEFAULT_FONT_PIXELS * MONO_GLYPH_ADVANCE_NUMERATOR
        / MONO_GLYPH_ADVANCE_DENOMINATOR
        + FRAME_PADDING_PX * 2;
const FRAME_HEIGHT: u32 = 576;
const FRAME_PADDING_PX: u32 = 12;

#[derive(Clone, Copy, Debug, PartialEq)]
struct TerminalMetrics {
    font_pixels: f32,
    glyph_advance_px: f32,
    row_height_px: u32,
}

impl TerminalMetrics {
    fn from_font_step(step: Shell2FontScaleStep) -> Self {
        let pixels = step.effective_pixels.max(1);
        let scaled = |value: u32| {
            value
                .saturating_mul(pixels)
                .saturating_add(DEFAULT_FONT_PIXELS / 2)
                / DEFAULT_FONT_PIXELS
        };
        Self {
            font_pixels: pixels as f32,
            glyph_advance_px: pixels as f32 * MONO_GLYPH_ADVANCE_NUMERATOR as f32
                / MONO_GLYPH_ADVANCE_DENOMINATOR as f32,
            row_height_px: scaled(DEFAULT_ROW_HEIGHT_PX).max(1),
        }
    }
}

impl Default for TerminalMetrics {
    fn default() -> Self {
        Self {
            font_pixels: DEFAULT_FONT_PIXELS as f32,
            glyph_advance_px: DEFAULT_FONT_PIXELS as f32 * MONO_GLYPH_ADVANCE_NUMERATOR as f32
                / MONO_GLYPH_ADVANCE_DENOMINATOR as f32,
            row_height_px: DEFAULT_ROW_HEIGHT_PX,
        }
    }
}

struct FontScaleState {
    steps: Vec<Shell2FontScaleStep>,
    selected: usize,
    applied: usize,
}

impl FontScaleState {
    fn load() -> Result<Self, UiError> {
        let steps = shell2_font_scale_steps()?;
        if steps.is_empty()
            || !steps
                .windows(2)
                .all(|pair| pair[0].effective_pixels < pair[1].effective_pixels)
        {
            return Err(UiError::Invalid);
        }
        let selected = steps
            .iter()
            .position(|step| step.effective_pixels == DEFAULT_FONT_PIXELS)
            .ok_or(UiError::Invalid)?;
        Ok(Self {
            steps,
            selected,
            applied: selected,
        })
    }

    fn current(&self) -> Shell2FontScaleStep {
        self.steps[self.selected]
    }

    fn pending(&self) -> Option<Shell2FontScaleStep> {
        (self.selected != self.applied).then(|| self.current())
    }

    fn mark_applied(&mut self) {
        self.applied = self.selected;
    }

    fn larger(&mut self) {
        self.selected = self
            .selected
            .saturating_add(1)
            .min(self.steps.len().saturating_sub(1));
    }

    fn smaller(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }
}

fn font_step_larger(state: &mut FontScaleState) {
    state.larger();
}

fn font_step_smaller(state: &mut FontScaleState) {
    state.smaller();
}

const BACKGROUND: u32 = rgba(0, 0, 0, 191);
const FOREGROUND: u32 = rgba(255, 255, 255, 255);
/// The immediate frame-stamp ABI accepts at most this many colour layers, with
/// at most this many positioned runs in each layer. These are submission
/// bounds, not retained Shell2 state: every terminal repaint rebuilds them.
const DIRECT_STAMP_MAX_LAYERS: usize = 64;
const DIRECT_STAMP_MAX_RUNS_PER_LAYER: usize = 64;
const DIRECT_STAMP_MAX_CHARS_PER_RUN: usize = 256;
const DIRECT_SOLID_MAX_QUADS: usize = 8_192;
const POLL_INTERVAL_MS: u64 = 5;
const SHELL_OUTPUT_BATCH_CAP: usize = 8 * 1024;
const SHELL_ATTACH_RETRIES: usize = 1_000;
const APP_CURSOR_OUTLINE_STROKE_PX: u32 = 3;
const APP_CURSOR_CELL_WIDTH_SUBPX: u32 =
    (DEFAULT_FONT_PIXELS * MONO_GLYPH_ADVANCE_NUMERATOR * 1_024
        + MONO_GLYPH_ADVANCE_DENOMINATOR / 2)
        / MONO_GLYPH_ADVANCE_DENOMINATOR;
const APP_CURSOR_CELL_HEIGHT_SUBPX: u32 = DEFAULT_ROW_HEIGHT_PX * 1_024;
const SHELL_RENDER_TRACE_FIRST: u64 = 16;
const SHELL_RENDER_TRACE_EVERY: u64 = 128;
const SHELL_RENDER_TRACE_PENDING_CAP: usize = 32;
const HID_MODIFIER_LEFT_CONTROL: u8 = 1 << 0;
const HID_MODIFIER_RIGHT_CONTROL: u8 = 1 << 4;
const HID_MODIFIER_CONTROL_MASK: u8 = HID_MODIFIER_LEFT_CONTROL | HID_MODIFIER_RIGHT_CONTROL;
const MATRIX_STATUS_ROW: usize = 1;
const MATRIX_CLICK_PREFIX_BYTE: u8 = 0xff;
const MATRIX_CLICK_SUFFIX_BYTE: u8 = 0x00;
const RETURN_TO_PARENT_BYTE: u8 = 0x1c;
const TERMINAL_RESET: &[u8] = b"\x1b[?1049l\x1b[0m\x1b[2J\x1b[H";

/// One visible run in the current terminal frame. It is built and consumed
/// inside one presentation call; Shell2 retains no glyph, sprite, or font
/// result between frames.
struct DirectTextRun {
    color_rgba: u32,
    text: String,
    x: f32,
    y: f32,
}

struct DirectStampBudget {
    remaining_layers: usize,
}

#[derive(Clone, Copy, Debug, Default)]
struct RenderTiming {
    started_ns: u64,
    total_us: u64,
    begin_us: u64,
    solid_build_us: u64,
    solid_submit_us: u64,
    text_build_us: u64,
    stamp_submit_us: u64,
    publish_wait_us: u64,
    busy_polls: u64,
    cells: usize,
    solid_quads: usize,
    text_runs: usize,
    glyphs: usize,
    stamp_layers: usize,
}

#[derive(Clone, Copy, Debug)]
struct PendingInputTrace {
    sample: u64,
    scalar: u32,
    hid_t_ms: u64,
    received_ns: u64,
    submit_done_ns: u64,
    submit_us: u64,
}

#[derive(Clone, Copy, Debug)]
struct PendingRenderTrace {
    first_sample: u64,
    last_sample: u64,
    input_count: usize,
    first_scalar: u32,
    uniform_scalar: bool,
    first_hid_t_ms: u64,
    first_received_ns: u64,
    last_submit_done_ns: u64,
    submit_us: u64,
    output_seen_ns: u64,
    output_bytes: usize,
    terminal_feed_us: u64,
}

#[derive(Debug, Default)]
struct ShellRenderTracer {
    inputs_seen: u64,
    pending: VecDeque<PendingInputTrace>,
    ready: Option<PendingRenderTrace>,
    dropped_pending: u64,
}

impl ShellRenderTracer {
    fn begin_input(
        &mut self,
        event: TrueosKeyboardOutputEvent,
        bytes: &[u8],
    ) -> Option<PendingInputTrace> {
        self.inputs_seen = self.inputs_seen.saturating_add(1);
        let sample = self.inputs_seen;
        if sample > SHELL_RENDER_TRACE_FIRST && !sample.is_multiple_of(SHELL_RENDER_TRACE_EVERY) {
            return None;
        }
        let scalar = core::str::from_utf8(bytes)
            .ok()
            .and_then(|text| {
                let mut chars = text.chars();
                let scalar = chars.next()?;
                chars.next().is_none().then_some(scalar as u32)
            })
            .unwrap_or(0);
        Some(PendingInputTrace {
            sample,
            scalar,
            hid_t_ms: u64::from(event.t_ms),
            received_ns: clock::monotonic_nanos(),
            submit_done_ns: 0,
            submit_us: 0,
        })
    }

    fn finish_input(&mut self, mut trace: PendingInputTrace, submit_done_ns: u64) {
        trace.submit_done_ns = submit_done_ns;
        trace.submit_us = nanos_to_micros(submit_done_ns.saturating_sub(trace.received_ns));
        if self.pending.len() >= SHELL_RENDER_TRACE_PENDING_CAP {
            let _ = self.pending.pop_front();
            self.dropped_pending = self.dropped_pending.saturating_add(1);
        }
        self.pending.push_back(trace);
    }

    fn note_shell_output(&mut self, output_seen_ns: u64, output_bytes: usize, feed_us: u64) {
        let Some(first) = self.pending.pop_front() else {
            return;
        };
        let batch = self.ready.get_or_insert(PendingRenderTrace {
            first_sample: first.sample,
            last_sample: first.sample,
            input_count: 0,
            first_scalar: first.scalar,
            uniform_scalar: true,
            first_hid_t_ms: first.hid_t_ms,
            first_received_ns: first.received_ns,
            last_submit_done_ns: first.submit_done_ns,
            submit_us: 0,
            output_seen_ns,
            output_bytes: 0,
            terminal_feed_us: 0,
        });
        Self::merge_input(batch, first);
        while let Some(trace) = self.pending.pop_front() {
            Self::merge_input(batch, trace);
        }
        batch.output_bytes = batch.output_bytes.saturating_add(output_bytes);
        batch.terminal_feed_us = batch.terminal_feed_us.saturating_add(feed_us);
    }

    fn merge_input(batch: &mut PendingRenderTrace, trace: PendingInputTrace) {
        batch.last_sample = trace.sample;
        batch.input_count = batch.input_count.saturating_add(1);
        batch.uniform_scalar &= trace.scalar == batch.first_scalar;
        batch.last_submit_done_ns = batch.last_submit_done_ns.max(trace.submit_done_ns);
        batch.submit_us = batch.submit_us.saturating_add(trace.submit_us);
    }

    fn finish_present(&mut self, timing: RenderTiming) {
        let Some(trace) = self.ready.take() else {
            return;
        };
        let visible_ns = clock::monotonic_nanos();
        let hid_ns = trace.first_hid_t_ms.saturating_mul(1_000_000);
        let scalar = if trace.uniform_scalar {
            trace.first_scalar
        } else {
            0
        };
        let _ = logl::log_record(
            level::TRACE,
            "shell2-render-trace",
            format_args!(
                "samples={}..{} inputs={} scalar=U+{:04X} uniform={} hid_to_app_us={} submit_us={} submit_to_output_us={} output_bytes={} terminal_feed_us={} output_to_present_us={} begin_us={} solid_build_us={} solid_submit_us={} text_build_us={} stamp_submit_us={} publish_wait_us={} present_us={} input_to_visible_us={} busy_polls={} cells={} solid_quads={} text_runs={} glyphs={} stamp_layers={} dropped_pending={}",
                trace.first_sample,
                trace.last_sample,
                trace.input_count,
                scalar,
                trace.uniform_scalar as u8,
                nanos_to_micros(trace.first_received_ns.saturating_sub(hid_ns)),
                trace.submit_us,
                nanos_to_micros(
                    trace
                        .output_seen_ns
                        .saturating_sub(trace.last_submit_done_ns)
                ),
                trace.output_bytes,
                trace.terminal_feed_us,
                nanos_to_micros(timing.started_ns.saturating_sub(trace.output_seen_ns)),
                timing.begin_us,
                timing.solid_build_us,
                timing.solid_submit_us,
                timing.text_build_us,
                timing.stamp_submit_us,
                timing.publish_wait_us,
                timing.total_us,
                nanos_to_micros(visible_ns.saturating_sub(trace.first_received_ns)),
                timing.busy_polls,
                timing.cells,
                timing.solid_quads,
                timing.text_runs,
                timing.glyphs,
                timing.stamp_layers,
                self.dropped_pending,
            ),
        );
    }
}

#[inline]
fn nanos_to_micros(nanos: u64) -> u64 {
    nanos / 1_000
}

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

/// Cursor state belongs to this frame's pixels. It deliberately uses the
/// initial Shell2 cell geometry: resize and font-scale support can reconfigure
/// it in a later patch without changing the AppOwned cursor contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AppCursorCell {
    col: usize,
    row: usize,
}

fn foreground_rgba(foreground: TerminalColor) -> u32 {
    match foreground {
        TerminalColor::Default => FOREGROUND,
        TerminalColor::Rgb { red, green, blue } => rgba(red, green, blue, 255),
        TerminalColor::Indexed(index) => ansi_indexed_rgba(index),
    }
}

fn background_rgba(background: TerminalColor) -> Option<u32> {
    match background {
        TerminalColor::Default => None,
        TerminalColor::Rgb { red, green, blue } => Some(rgba(red, green, blue, 255)),
        TerminalColor::Indexed(index) => Some(ansi_indexed_rgba(index)),
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
    if let Err(error) = frame.set_custom_cursor(true) {
        logl::log(
            level::ERROR,
            format_args!("shell: UI4 AppOwned cursor setup failed: {error:?}"),
        );
        return;
    }
    if let Err(error) = frame.set_cursor_step(Some(CursorStep {
        origin_x: FRAME_PADDING_PX,
        origin_y: FRAME_PADDING_PX,
        cell_width_subpx: APP_CURSOR_CELL_WIDTH_SUBPX,
        cell_height_subpx: APP_CURSOR_CELL_HEIGHT_SUBPX,
    })) {
        logl::log(
            level::ERROR,
            format_args!("shell: UI4 AppOwned cursor step setup failed: {error:?}"),
        );
        return;
    }

    let mut font_scale = match FontScaleState::load() {
        Ok(scale) => scale,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("shell: UI4 font scale ladder unavailable: {error:?}"),
            );
            return;
        }
    };
    let font_menu = [
        MenuEntry::new("Font +1", font_step_larger),
        MenuEntry::new("Font -1", font_step_smaller),
    ];
    if let Err(error) = frame.register_context_menu(&font_menu) {
        logl::log(
            level::ERROR,
            format_args!("shell: UI4 font context menu registration failed: {error:?}"),
        );
        return;
    }

    let mut metrics = TerminalMetrics::from_font_step(font_scale.current());
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
    let mut render_tracer = ShellRenderTracer::default();
    let mut matrix_slot_hover = None;
    let mut app_cursor = None;

    if let Err(error) = present_terminal(&mut frame, &terminal, metrics, None, app_cursor) {
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
            "shell: local shell2 session online cols={} rows={} font=JuliaMono fallback=Inconsolata cursor=app-cell-outline",
            cols, rows
        ),
    );

    loop {
        invoking_terminal.poll_reentry(&mut frontend, &terminal);
        let _resized = match drain_resize_events(
            &mut frame,
            &mut frontend,
            &mut terminal,
            metrics,
            app_cursor,
        ) {
            Ok(resized) => resized,
            Err(error) => {
                logl::log(
                    level::ERROR,
                    format_args!("shell: UI4 frame resize failed: {error:?}"),
                );
                return;
            }
        };

        if let Err(error) = drain_shell_output(
            &mut frontend,
            &mut terminal,
            invoking_terminal.is_active(),
            &mut render_tracer,
        ) {
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

        if let Err(error) = frame.pump_context_menu(&font_menu, &mut font_scale) {
            logl::log(
                level::ERROR,
                format_args!("shell: UI4 font context menu failed: {error:?}"),
            );
            return;
        }
        let mut font_scaled = false;
        if let Some(step) = font_scale.pending() {
            font_scaled = match apply_terminal_font_step(
                &mut frame,
                &mut frontend,
                &mut terminal,
                &mut metrics,
                step,
            ) {
                Ok(changed) => changed,
                Err(error) => {
                    logl::log(
                        level::ERROR,
                        format_args!("shell: terminal font scale failed: {error:?}"),
                    );
                    return;
                }
            };
            font_scale.mark_applied();
        }

        if let Err(error) = drain_keyboard_input(
            &mut frame,
            &mut keyboard_input,
            &frontend,
            &mut render_tracer,
        ) {
            logl::log(
                level::ERROR,
                format_args!("shell: routed keyboard input failed: {error:?}"),
            );
            return;
        }

        let pointer_changed = match drain_pointer_input(
            &mut frame,
            &terminal,
            &frontend,
            metrics,
            &mut matrix_slot_hover,
            &mut app_cursor,
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

        let mut hover_changed = pointer_changed.hover;
        if terminal.mouse_tracking_enabled() && matrix_slot_hover.take().is_some() {
            // A direct terminal owner gets unmodified pointer semantics.
            // Shell2's status strip is not live during that handoff.
            hover_changed = true;
        }

        if terminal.take_dirty() || hover_changed || pointer_changed.cursor || font_scaled {
            match present_terminal(
                &mut frame,
                &terminal,
                metrics,
                matrix_slot_hover.as_ref(),
                app_cursor,
            ) {
                Ok(timing) => render_tracer.finish_present(timing),
                Err(error) => {
                    logl::log(
                        level::ERROR,
                        format_args!("shell: UI4 terminal frame failed: {error:?}"),
                    );
                    return;
                }
            }
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
    app_cursor: Option<AppCursorCell>,
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
        present_terminal_at(
            frame,
            terminal,
            old_origin_x,
            old_origin_y,
            metrics,
            None,
            app_cursor,
        )?;
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
        ((width as f32 / metrics.glyph_advance_px) as usize).max(1),
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
    let content_width = cols as f32 * metrics.glyph_advance_px + (FRAME_PADDING_PX * 2) as f32;
    let content_height = u32::try_from(rows)
        .unwrap_or(u32::MAX)
        .saturating_mul(metrics.row_height_px)
        .saturating_add(FRAME_PADDING_PX * 2);
    (
        ((width as f32 - content_width).max(0.0) / 2.0) as u32,
        height.saturating_sub(content_height) / 2,
    )
}

fn apply_terminal_font_step(
    frame: &mut Frame,
    frontend: &mut Shell2Frontend,
    terminal: &mut Terminal,
    metrics: &mut TerminalMetrics,
    step: Shell2FontScaleStep,
) -> Result<bool, InputError> {
    let next = TerminalMetrics::from_font_step(step);
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
            "shell: font step effective_px={} native_tier_px={} residual_milli={} cell={:.2}x{} grid={}x{}",
            step.effective_pixels,
            step.native_tier_pixels,
            step.residual_milli,
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
    render_tracer: &mut ShellRenderTracer,
) -> Result<(), Shell2FrontendError> {
    let mut bytes = [0u8; SHELL_OUTPUT_BATCH_CAP];
    for _ in 0..32 {
        let read = frontend.read(&mut bytes)?;
        if read.epoch_changed || read.flags & SHELL2_FRONTEND_READ_DROPPED != 0 {
            terminal.reset();
        }
        if read.len != 0 {
            let output_seen_ns = clock::monotonic_nanos();
            if mirror_to_invoking_terminal {
                let _ = trueos::vshell::attached_write(&bytes[..read.len]);
            }
            terminal.feed(&bytes[..read.len]);
            render_tracer.note_shell_output(
                output_seen_ns,
                read.len,
                nanos_to_micros(clock::monotonic_nanos().saturating_sub(output_seen_ns)),
            );
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
    render_tracer: &mut ShellRenderTracer,
) -> Result<(), InputError> {
    while let Some(event) = frame.take_keyboard_event()? {
        handle_keyboard_event(state, frontend, event, render_tracer)?;
    }
    Ok(())
}

fn handle_keyboard_event(
    state: &mut KeyboardInputState,
    frontend: &Shell2Frontend,
    event: TrueosKeyboardOutputEvent,
    render_tracer: &mut ShellRenderTracer,
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
                submit_traced_input(
                    render_tracer,
                    frontend,
                    event,
                    core::slice::from_ref(&control),
                )?;
            } else {
                // An ordinary key transition stays one glyph-sized operation.
                submit_traced_input(render_tracer, frontend, event, text)?;
            }
        }
        input::KEYBOARD_OUTPUT_KIND_KEY => {
            state.suppressed_text = None;
            if let Some(sequence) = named_key_sequence(event.key_code) {
                submit_traced_input(render_tracer, frontend, event, sequence)?;
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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct PointerChanges {
    hover: bool,
    cursor: bool,
}

fn drain_pointer_input(
    frame: &mut Frame,
    terminal: &Terminal,
    frontend: &Shell2Frontend,
    metrics: TerminalMetrics,
    matrix_slot_hover: &mut Option<MatrixSlotHover>,
    app_cursor: &mut Option<AppCursorCell>,
) -> Result<PointerChanges, InputError> {
    let initial_hover = matrix_slot_hover.clone();
    let initial_cursor = *app_cursor;
    while let Some(event) = frame.take_pointer_event()? {
        // The AppOwned outline is deliberately just a software-mouse motion
        // affordance. Clicks, wheels, selections, and keyboard input do not
        // create or move it.
        if pointer_event_is_motion(event) {
            *app_cursor = Some(app_cursor_cell(event.local_x, event.local_y));
        }
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
    Ok(PointerChanges {
        hover: *matrix_slot_hover != initial_hover,
        cursor: *app_cursor != initial_cursor,
    })
}

fn app_cursor_cell(local_x: i32, local_y: i32) -> AppCursorCell {
    let metrics = TerminalMetrics::default();
    let x = local_x.saturating_sub(FRAME_PADDING_PX as i32).max(0) as u32;
    let y = local_y.saturating_sub(FRAME_PADDING_PX as i32).max(0) as u32;
    AppCursorCell {
        col: (x as f32 / metrics.glyph_advance_px) as usize,
        row: (y / metrics.row_height_px) as usize,
    }
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
    let col = (x as f32 / metrics.glyph_advance_px) as usize;
    let row = y as u32 / metrics.row_height_px;
    (col < dimensions.0 && row < dimensions.1 as u32).then_some((col, row as usize))
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
    let col = ((x as f32 / metrics.glyph_advance_px) as usize).min(dimensions.0 - 1);
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

fn submit_traced_input(
    render_tracer: &mut ShellRenderTracer,
    frontend: &Shell2Frontend,
    event: TrueosKeyboardOutputEvent,
    bytes: &[u8],
) -> Result<(), Shell2FrontendError> {
    let trace = render_tracer.begin_input(event, bytes);
    submit_input(frontend, bytes)?;
    if let Some(trace) = trace {
        render_tracer.finish_input(trace, clock::monotonic_nanos());
    }
    Ok(())
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
    app_cursor: Option<AppCursorCell>,
) -> Result<RenderTiming, UiError> {
    present_terminal_at(
        frame,
        terminal,
        0,
        0,
        metrics,
        matrix_slot_hover,
        app_cursor,
    )
}

fn present_terminal_at(
    frame: &mut Frame,
    terminal: &Terminal,
    origin_x: u32,
    origin_y: u32,
    metrics: TerminalMetrics,
    matrix_slot_hover: Option<&MatrixSlotHover>,
    app_cursor: Option<AppCursorCell>,
) -> Result<RenderTiming, UiError> {
    let started_ns = clock::monotonic_nanos();
    let mut busy_polls = 0u64;
    // Paint transient cell rectangles, then stamp the current glyphs into the
    // same leased frame. Sprite id zero is UI4's frame-owned white pixel, so
    // this owns no uploaded sprite, glyph ticket, or retained cache.
    let phase_started_ns = clock::monotonic_nanos();
    busy_polls = busy_polls.saturating_add(retry_busy_observed(|| {
        frame.begin_sprite_frame(BACKGROUND)
    })?);
    let begin_us = nanos_to_micros(clock::monotonic_nanos().saturating_sub(phase_started_ns));

    let phase_started_ns = clock::monotonic_nanos();
    let (cols, rows) = terminal.dimensions();
    let mut solid_quads = Vec::with_capacity(terminal.cells().len().min(DIRECT_SOLID_MAX_QUADS));
    for row in 0..rows {
        let cells = &terminal.cells()[row * cols..(row + 1) * cols];
        collect_background_quads(&mut solid_quads, cells, row, origin_x, origin_y, metrics);
    }
    for row in 0..rows {
        let cells = &terminal.cells()[row * cols..(row + 1) * cols];
        collect_underline_quads(&mut solid_quads, cells, row, origin_x, origin_y, metrics);
    }

    let hover = matrix_slot_hover.filter(|hover| {
        matrix_slot_at(terminal, hover.start_col, MATRIX_STATUS_ROW)
            .is_some_and(|current| current == **hover)
    });
    if let Some(hover) = hover {
        let foreground = terminal.cells()[MATRIX_STATUS_ROW * cols + hover.start_col]
            .style
            .foreground;
        push_underline_quad(
            &mut solid_quads,
            foreground_rgba(foreground),
            MATRIX_STATUS_ROW,
            hover.start_col,
            hover.end_col,
            origin_x,
            origin_y,
            metrics,
        );
    }

    let cursor = terminal.cursor();
    if cursor.visible {
        push_underline_quad(
            &mut solid_quads,
            FOREGROUND,
            cursor.row,
            cursor.col,
            cursor.col.saturating_add(1),
            origin_x,
            origin_y,
            metrics,
        );
    }
    if let Some(cursor) = app_cursor {
        push_app_cursor_outline(&mut solid_quads, cursor);
    }
    if solid_quads.len() == DIRECT_SOLID_MAX_QUADS {
        logl::log(
            level::IMPORTANT,
            format_args!(
                "shell: immediate solid scene reached its {} quad cap; clipping remaining cell decoration",
                DIRECT_SOLID_MAX_QUADS
            ),
        );
    }
    let solid_build_us = nanos_to_micros(clock::monotonic_nanos().saturating_sub(phase_started_ns));

    let phase_started_ns = clock::monotonic_nanos();
    busy_polls = busy_polls.saturating_add(retry_busy_observed(|| {
        frame.draw_sprite_quads(&solid_quads)
    })?);
    let solid_submit_us =
        nanos_to_micros(clock::monotonic_nanos().saturating_sub(phase_started_ns));

    let phase_started_ns = clock::monotonic_nanos();
    let mut text_runs = Vec::with_capacity(terminal.cells().len());
    for row in 0..rows {
        let cells = &terminal.cells()[row * cols..(row + 1) * cols];
        collect_glyph_runs(&mut text_runs, cells, row, origin_x, origin_y, metrics);
    }
    let glyphs = text_runs.iter().fold(0usize, |total, run| {
        total.saturating_add(run.text.chars().count())
    });
    let text_build_us = nanos_to_micros(clock::monotonic_nanos().saturating_sub(phase_started_ns));

    let mut stamp_budget = DirectStampBudget {
        remaining_layers: DIRECT_STAMP_MAX_LAYERS,
    };
    let phase_started_ns = clock::monotonic_nanos();
    busy_polls = busy_polls.saturating_add(stamp_direct_runs(
        frame,
        &text_runs,
        metrics.font_pixels,
        &mut stamp_budget,
    )?);
    let stamp_submit_us =
        nanos_to_micros(clock::monotonic_nanos().saturating_sub(phase_started_ns));

    let phase_started_ns = clock::monotonic_nanos();
    busy_polls = busy_polls.saturating_add(retry_busy_observed(|| {
        frame.publish(Damage::full(frame.width(), frame.height()))
    })?);
    let finished_ns = clock::monotonic_nanos();
    Ok(RenderTiming {
        started_ns,
        total_us: nanos_to_micros(finished_ns.saturating_sub(started_ns)),
        begin_us,
        solid_build_us,
        solid_submit_us,
        text_build_us,
        stamp_submit_us,
        publish_wait_us: nanos_to_micros(finished_ns.saturating_sub(phase_started_ns)),
        busy_polls,
        cells: terminal.cells().len(),
        solid_quads: solid_quads.len(),
        text_runs: text_runs.len(),
        glyphs,
        stamp_layers: DIRECT_STAMP_MAX_LAYERS.saturating_sub(stamp_budget.remaining_layers),
    })
}

fn collect_glyph_runs(
    runs: &mut Vec<DirectTextRun>,
    cells: &[terminal::Cell],
    row: usize,
    origin_x: u32,
    origin_y: u32,
    metrics: TerminalMetrics,
) {
    let mut text = String::new();
    let mut color = FOREGROUND;
    let mut start_col = 0;
    let mut next_col = 0;
    for (col, cell) in cells.iter().enumerate() {
        let next_color = foreground_rgba(cell.style.foreground);
        if cell.glyph == ' ' {
            push_direct_run(
                runs, &mut text, color, start_col, row, origin_x, origin_y, metrics,
            );
            next_col = col.saturating_add(1);
            continue;
        }
        if text.is_empty() {
            color = next_color;
            start_col = col;
        } else if color != next_color || col != next_col {
            push_direct_run(
                runs, &mut text, color, start_col, row, origin_x, origin_y, metrics,
            );
            color = next_color;
            start_col = col;
        }
        text.push(cell.glyph);
        next_col = col.saturating_add(1);
    }
    push_direct_run(
        runs, &mut text, color, start_col, row, origin_x, origin_y, metrics,
    );
}

fn collect_background_quads(
    quads: &mut Vec<SpriteQuad>,
    cells: &[terminal::Cell],
    row: usize,
    origin_x: u32,
    origin_y: u32,
    metrics: TerminalMetrics,
) {
    let mut color = None;
    let mut start_col = 0;
    for col in 0..=cells.len() {
        let next_color = cells
            .get(col)
            .and_then(|cell| background_rgba(cell.style.background));
        if next_color != color {
            if let Some(color_rgba) = color {
                push_cell_quad(
                    quads,
                    color_rgba,
                    row,
                    start_col,
                    col,
                    origin_x,
                    origin_y,
                    metrics,
                    0,
                    metrics.row_height_px,
                );
            }
            color = next_color;
            start_col = col;
        }
    }
}

fn collect_underline_quads(
    quads: &mut Vec<SpriteQuad>,
    cells: &[terminal::Cell],
    row: usize,
    origin_x: u32,
    origin_y: u32,
    metrics: TerminalMetrics,
) {
    let mut color = None;
    let mut start_col = 0;
    for col in 0..=cells.len() {
        let next_color = cells
            .get(col)
            .filter(|cell| cell.style.underline)
            .map(|cell| foreground_rgba(cell.style.foreground));
        if next_color != color {
            if let Some(color_rgba) = color {
                push_underline_quad(
                    quads, color_rgba, row, start_col, col, origin_x, origin_y, metrics,
                );
            }
            color = next_color;
            start_col = col;
        }
    }
}

fn push_underline_quad(
    quads: &mut Vec<SpriteQuad>,
    color_rgba: u32,
    row: usize,
    start_col: usize,
    end_col: usize,
    origin_x: u32,
    origin_y: u32,
    metrics: TerminalMetrics,
) {
    let thickness = (metrics.font_pixels as u32)
        .saturating_mul(3)
        .saturating_add(DEFAULT_FONT_PIXELS / 2)
        / DEFAULT_FONT_PIXELS;
    let thickness = thickness.clamp(1, metrics.row_height_px);
    // Pin both terminal and Matrix hover underlines to the actual cell edge,
    // independent of the glyph ascent at the active Shell2 font scale.
    let top = metrics.row_height_px.saturating_sub(thickness);
    push_cell_quad(
        quads, color_rgba, row, start_col, end_col, origin_x, origin_y, metrics, top, thickness,
    );
}

fn push_app_cursor_outline(quads: &mut Vec<SpriteQuad>, cursor: AppCursorCell) {
    if quads.len() > DIRECT_SOLID_MAX_QUADS.saturating_sub(4) {
        return;
    }
    let metrics = TerminalMetrics::default();
    let left = terminal_cell_x(0, cursor.col, metrics);
    let right = terminal_cell_x(0, cursor.col.saturating_add(1), metrics);
    let top = terminal_cell_y(0, cursor.row, metrics);
    let bottom = top + metrics.row_height_px as f32;
    let stroke = APP_CURSOR_OUTLINE_STROKE_PX as f32;

    push_solid_quad(quads, FOREGROUND, left, top, right, top + stroke);
    push_solid_quad(quads, FOREGROUND, left, bottom - stroke, right, bottom);
    push_solid_quad(
        quads,
        FOREGROUND,
        left,
        top + stroke,
        left + stroke,
        bottom - stroke,
    );
    push_solid_quad(
        quads,
        FOREGROUND,
        right - stroke,
        top + stroke,
        right,
        bottom - stroke,
    );
}

fn push_solid_quad(
    quads: &mut Vec<SpriteQuad>,
    color_rgba: u32,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
) {
    if left >= right || top >= bottom || quads.len() >= DIRECT_SOLID_MAX_QUADS {
        return;
    }
    quads.push(SpriteQuad {
        sprite_id: 0,
        c0: SpriteCorner {
            x: left,
            y: top,
            u: 0.0,
            v: 0.0,
        },
        c1: SpriteCorner {
            x: right,
            y: top,
            u: 1.0,
            v: 0.0,
        },
        c2: SpriteCorner {
            x: right,
            y: bottom,
            u: 1.0,
            v: 1.0,
        },
        c3: SpriteCorner {
            x: left,
            y: bottom,
            u: 0.0,
            v: 1.0,
        },
        color_rgba,
        source_over: false,
    });
}

#[allow(clippy::too_many_arguments)]
fn push_cell_quad(
    quads: &mut Vec<SpriteQuad>,
    color_rgba: u32,
    row: usize,
    start_col: usize,
    end_col: usize,
    origin_x: u32,
    origin_y: u32,
    metrics: TerminalMetrics,
    top_offset: u32,
    height: u32,
) {
    if start_col >= end_col || quads.len() >= DIRECT_SOLID_MAX_QUADS {
        return;
    }
    let left = terminal_cell_x(origin_x, start_col, metrics);
    let right = terminal_cell_x(origin_x, end_col, metrics);
    let top = terminal_cell_y(origin_y, row, metrics) + top_offset as f32;
    let bottom = top + height as f32;
    push_solid_quad(quads, color_rgba, left, top, right, bottom);
}

fn push_direct_run(
    runs: &mut Vec<DirectTextRun>,
    text: &mut String,
    color_rgba: u32,
    start_col: usize,
    row: usize,
    origin_x: u32,
    origin_y: u32,
    metrics: TerminalMetrics,
) {
    if text.is_empty() {
        return;
    }
    push_direct_text(
        runs,
        color_rgba,
        core::mem::take(text),
        terminal_cell_x(origin_x, start_col, metrics),
        terminal_cell_y(origin_y, row, metrics),
        metrics.glyph_advance_px,
    );
}

fn push_direct_text(
    runs: &mut Vec<DirectTextRun>,
    color_rgba: u32,
    text: String,
    x: f32,
    y: f32,
    glyph_advance_px: f32,
) {
    let mut chunk = String::new();
    let mut chunk_chars = 0usize;
    let mut chunk_start = 0usize;
    for glyph in text.chars() {
        if chunk_chars == DIRECT_STAMP_MAX_CHARS_PER_RUN {
            runs.push(DirectTextRun {
                color_rgba,
                text: core::mem::take(&mut chunk),
                x: x + chunk_start as f32 * glyph_advance_px,
                y,
            });
            chunk_start = chunk_start.saturating_add(chunk_chars);
            chunk_chars = 0;
        }
        chunk.push(glyph);
        chunk_chars = chunk_chars.saturating_add(1);
    }
    if !chunk.is_empty() {
        runs.push(DirectTextRun {
            color_rgba,
            text: chunk,
            x: x + chunk_start as f32 * glyph_advance_px,
            y,
        });
    }
}

fn terminal_cell_x(origin_x: u32, col: usize, metrics: TerminalMetrics) -> f32 {
    origin_x as f32 + FRAME_PADDING_PX as f32 + col as f32 * metrics.glyph_advance_px
}

fn terminal_cell_y(origin_y: u32, row: usize, metrics: TerminalMetrics) -> f32 {
    origin_y
        .saturating_add(FRAME_PADDING_PX)
        .saturating_add((row as u32).saturating_mul(metrics.row_height_px)) as f32
}

fn stamp_direct_runs(
    frame: &mut Frame,
    runs: &[DirectTextRun],
    font_pixels: f32,
    budget: &mut DirectStampBudget,
) -> Result<u64, UiError> {
    if runs.is_empty() {
        return Ok(0);
    }

    let mut busy_polls = 0u64;

    let mut colors = Vec::<(u32, usize)>::new();
    for run in runs {
        if let Some((_, count)) = colors
            .iter_mut()
            .find(|(color, _)| *color == run.color_rgba)
        {
            *count = count.saturating_add(1);
        } else {
            colors.push((run.color_rgba, 1));
        }
    }
    let layer_count = colors.iter().fold(0usize, |total, (_, runs)| {
        total.saturating_add(
            runs.saturating_add(DIRECT_STAMP_MAX_RUNS_PER_LAYER - 1)
                / DIRECT_STAMP_MAX_RUNS_PER_LAYER,
        )
    });
    if layer_count > budget.remaining_layers {
        // The immediate ABI has no per-run colour field. Preserve terminal
        // liveness under pathological true-colour output instead of growing a
        // guest cache or retaining old glyph results. A richer direct ABI can
        // remove this fidelity fallback without changing Shell2 state.
        logl::log(
            level::IMPORTANT,
            format_args!(
                "shell: direct text stamp needs {} colour layers (cap {}); presenting this frame monochrome",
                layer_count, budget.remaining_layers
            ),
        );
        return stamp_runs_one_colour(frame, runs, font_pixels, FOREGROUND, budget);
    }

    for (color_rgba, _) in colors {
        let mut rows = Vec::with_capacity(DIRECT_STAMP_MAX_RUNS_PER_LAYER);
        for run in runs.iter().filter(|run| run.color_rgba == color_rgba) {
            rows.push(SceneTextRow {
                text: run.text.as_str(),
                x: run.x,
                y: run.y,
                font_pixels,
            });
            if rows.len() == DIRECT_STAMP_MAX_RUNS_PER_LAYER {
                busy_polls = busy_polls.saturating_add(retry_busy_observed(|| {
                    frame.stamp_text_scene(
                        Font::JuliaMono,
                        (frame.width(), frame.height()),
                        color_rgba,
                        &rows,
                    )
                })?);
                rows.clear();
            }
        }
        if !rows.is_empty() {
            busy_polls = busy_polls.saturating_add(retry_busy_observed(|| {
                frame.stamp_text_scene(
                    Font::JuliaMono,
                    (frame.width(), frame.height()),
                    color_rgba,
                    &rows,
                )
            })?);
        }
    }
    budget.remaining_layers = budget.remaining_layers.saturating_sub(layer_count);
    Ok(busy_polls)
}

fn stamp_runs_one_colour(
    frame: &mut Frame,
    runs: &[DirectTextRun],
    font_pixels: f32,
    color_rgba: u32,
    budget: &mut DirectStampBudget,
) -> Result<u64, UiError> {
    let layer_count = runs.len().div_ceil(DIRECT_STAMP_MAX_RUNS_PER_LAYER);
    if layer_count > budget.remaining_layers {
        logl::log(
            level::IMPORTANT,
            format_args!(
                "shell: direct text stamp has no layer budget for {} runs; omitting them",
                runs.len(),
            ),
        );
        return Ok(0);
    }
    let mut busy_polls = 0u64;
    for chunk in runs.chunks(DIRECT_STAMP_MAX_RUNS_PER_LAYER) {
        let rows = chunk
            .iter()
            .map(|run| SceneTextRow {
                text: run.text.as_str(),
                x: run.x,
                y: run.y,
                font_pixels,
            })
            .collect::<Vec<_>>();
        busy_polls = busy_polls.saturating_add(retry_busy_observed(|| {
            frame.stamp_text_scene(
                Font::JuliaMono,
                (frame.width(), frame.height()),
                color_rgba,
                &rows,
            )
        })?);
    }
    budget.remaining_layers = budget.remaining_layers.saturating_sub(layer_count);
    Ok(busy_polls)
}

fn retry_busy(mut operation: impl FnMut() -> Result<(), UiError>) -> Result<(), UiError> {
    retry_busy_observed(&mut operation).map(|_| ())
}

fn retry_busy_observed(mut operation: impl FnMut() -> Result<(), UiError>) -> Result<u64, UiError> {
    let mut busy_polls = 0u64;
    loop {
        match operation() {
            Ok(()) => return Ok(busy_polls),
            Err(UiError::Busy) => {
                busy_polls = busy_polls.saturating_add(1);
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
        assert_eq!(foreground_rgba(TerminalColor::Default), FOREGROUND);
        assert_eq!(
            foreground_rgba(TerminalColor::Rgb {
                red: 1,
                green: 2,
                blue: 3,
            }),
            rgba(1, 2, 3, 255)
        );
    }

    #[test]
    fn keeps_default_background_transparent_to_the_frame_clear() {
        assert_eq!(background_rgba(TerminalColor::Default), None);
        assert_eq!(
            background_rgba(TerminalColor::Rgb {
                red: 12,
                green: 36,
                blue: 98,
            }),
            Some(rgba(12, 36, 98, 255))
        );
    }

    #[test]
    fn coalesces_adjacent_cell_backgrounds_into_solid_quads() {
        let blue = TerminalColor::Rgb {
            red: 12,
            green: 36,
            blue: 98,
        };
        let red = TerminalColor::Indexed(1);
        let cell = |background| terminal::Cell {
            glyph: ' ',
            style: terminal::CellStyle {
                foreground: TerminalColor::Default,
                background,
                underline: false,
            },
        };
        let cells = [
            cell(blue),
            cell(blue),
            cell(TerminalColor::Default),
            cell(red),
        ];
        let mut quads = Vec::new();
        collect_background_quads(&mut quads, &cells, 0, 0, 0, TerminalMetrics::default());

        assert_eq!(quads.len(), 2);
        assert_eq!(quads[0].sprite_id, 0);
        assert_eq!(quads[0].c0.x, 12.0);
        assert_eq!(quads[0].c1.x, 36.0);
        assert_eq!(quads[0].c0.y, 12.0);
        assert_eq!(quads[0].c3.y, 38.0);
        assert_eq!(quads[0].color_rgba, rgba(12, 36, 98, 255));
        assert_eq!(quads[1].c0.x, 48.0);
        assert_eq!(quads[1].c1.x, 60.0);
        assert_eq!(quads[1].color_rgba, ansi_indexed_rgba(1));
    }

    #[test]
    fn underline_is_pinned_to_the_cell_bottom_edge() {
        let metrics = TerminalMetrics::default();
        let mut quads = Vec::new();
        push_underline_quad(&mut quads, FOREGROUND, 1, 2, 3, 0, 0, metrics);

        assert_eq!(quads.len(), 1);
        let thickness = 3.0;
        let cell_top = terminal_cell_y(0, 1, metrics);
        assert_eq!(
            quads[0].c0.y,
            cell_top + metrics.row_height_px as f32 - thickness
        );
        assert_eq!(quads[0].c3.y, cell_top + metrics.row_height_px as f32);
    }

    #[test]
    fn app_cursor_snaps_pointer_events_to_the_initial_default_grid() {
        assert_eq!(
            app_cursor_cell(FRAME_PADDING_PX as i32 + 14, FRAME_PADDING_PX as i32 + 25),
            AppCursorCell { col: 0, row: 0 }
        );
        assert_eq!(
            app_cursor_cell(FRAME_PADDING_PX as i32 + 15, FRAME_PADDING_PX as i32 + 26),
            AppCursorCell { col: 1, row: 1 }
        );
    }

    #[test]
    fn app_cursor_draws_a_three_pixel_cell_outline() {
        let cursor = AppCursorCell { col: 2, row: 1 };
        let metrics = TerminalMetrics::default();
        let mut quads = Vec::new();
        push_app_cursor_outline(&mut quads, cursor);

        assert_eq!(quads.len(), 4);
        assert_eq!(quads[0].c0.x, terminal_cell_x(0, cursor.col, metrics));
        assert_eq!(quads[0].c0.y, terminal_cell_y(0, cursor.row, metrics));
        assert_eq!(quads[0].c3.y - quads[0].c0.y, 3.0);
        assert_eq!(
            quads[1].c3.y,
            terminal_cell_y(0, cursor.row, metrics) + metrics.row_height_px as f32
        );
        assert_eq!(quads[2].c1.x - quads[2].c0.x, 3.0);
        assert_eq!(quads[3].c1.x - quads[3].c0.x, 3.0);
    }
}
