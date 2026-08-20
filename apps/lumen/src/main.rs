// trueos-blueprint: features = ["lumen"]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use std::io::{self, Write};
use std::time::Duration;

use crossterm::{
    cursor::{MoveTo, MoveToColumn, Show},
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    execute, queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use trueos::{
    logl::{self, level},
    lumen, platform, replication, vshell,
};

const LOG_TARGET: &str = "lumen";
const CHECKPOINT_VERSION: u64 = 1;
const POLL_MS: u64 = 10;
const EVENT_POLL_MS: u64 = 50;
const MAX_WAIT_POLLS: usize = 18_000;
const SPINNER_CADENCE_POLLS: usize = 4;
const SPINNER_MAX_SHIFT_CELLS: usize = 5;
const SPINNER_FRAMES: &[&str] = &["⢈", "⡈", "⡐", "⡠", "⣀", "⢄", "⢂", "⢁", "⡁"];

const SYSTEM_PROMPT: &str = concat!(
    "You are Lilly, a concise helpful assistant. ",
    "Respond with exactly one tool call. Use text for every ordinary answer, ",
    "play_emotion only when an emotion should be shown, and move only when movement is requested. ",
    "The Spirit actions are real. List of tools: ",
    "[{\"name\":\"text\",\"parameters\":{\"type\":\"object\",",
    "\"properties\":{\"text\":{\"type\":\"string\"}},\"required\":[\"text\"]}},",
    "{\"name\":\"play_emotion\",\"parameters\":{\"type\":\"object\",",
    "\"properties\":{\"idea\":{\"type\":\"string\",\"enum\":[\"anger\",\"disgust\",",
    "\"fear\",\"joy\",\"sadness\",\"surprise\"]}},\"required\":[\"idea\"],",
    "\"additionalProperties\":false}},",
    "{\"name\":\"move\",\"parameters\":{\"type\":\"object\",",
    "\"properties\":{\"x\":{\"type\":\"number\",\"minimum\":0,\"maximum\":1},",
    "\"y\":{\"type\":\"number\",\"minimum\":0,\"maximum\":1}},",
    "\"required\":[\"x\",\"y\"],\"additionalProperties\":false}}]."
);

struct LogicalState {
    turns: u64,
    reply_tail: [u32; 2],
    reply_tail_len: usize,
    model_checkpoint: Vec<u8>,
}

impl LogicalState {
    const fn new() -> Self {
        Self {
            turns: 0,
            reply_tail: [0; 2],
            reply_tail_len: 0,
            model_checkpoint: Vec::new(),
        }
    }
}

fn main() {
    let lease = match vshell::terminal_initial_lease() {
        Ok(lease) => lease,
        Err(error) => {
            diag(
                level::ERROR,
                format_args!("initial terminal lease unavailable: {error}"),
            );
            let _ = vshell::report_exit_reason("lumen terminal lease unavailable");
            let _ = vshell::shutdown_current_blueprint("lumen terminal lease unavailable");
            return;
        }
    };

    if let Err(error) = run_lumen(lease) {
        diag(
            level::ERROR,
            format_args!("terminal session failed: {error}"),
        );
        let _ = lumen::close();
        let _ = vshell::report_exit_reason("lumen terminal session failed");
        let _ = vshell::shutdown_current_blueprint("lumen terminal session failed");
    }
}

fn run_lumen(mut lease: vshell::TerminalLease) -> Result<(), String> {
    let mut state = LogicalState::new();
    let mut editor = InputEditor::new();
    let mut initialized = false;

    loop {
        let action = match run_terminal_session(&lease, &mut state, &mut editor, &mut initialized) {
            Ok(action) => action,
            Err(error) => {
                let _ = lease.release_to_shell();
                return Err(error);
            }
        };

        let ticket = lease
            .release_to_shell()
            .map_err(|error| format!("terminal lease release failed: {error}"))?;

        match action {
            SessionExit::ReturnToShell => {
                lease = wait_for_reentry(ticket, &mut state)?;
            }
            SessionExit::Quit => {
                let _ = lumen::close();
                let _ = vshell::report_exit_reason("lumen user exit");
                let _ = vshell::shutdown_current_blueprint("lumen user exit");
                return Ok(());
            }
        }
    }
}

fn run_terminal_session(
    lease: &vshell::TerminalLease,
    state: &mut LogicalState,
    editor: &mut InputEditor,
    initialized: &mut bool,
) -> Result<SessionExit, String> {
    let mut terminal =
        TerminalGuard::enter().map_err(|error| format!("terminal setup failed: {error}"))?;
    draw_session_header(state.turns, *initialized)
        .map_err(|error| format!("terminal header failed: {error}"))?;
    lease
        .acknowledge_ready()
        .map_err(|error| format!("terminal ready acknowledgement failed: {error}"))?;

    if !*initialized {
        initialize_lumen()?;
        *initialized = true;
    } else {
        vshell::linef(format_args!(
            "lumen-bp: terminal session resumed turns={}",
            state.turns
        ));
    }

    redraw_prompt(editor).map_err(|error| format!("prompt render failed: {error}"))?;

    loop {
        if let Some(prepare) = replication::poll_prepare_pause() {
            clear_prompt_for_output().map_err(|error| format!("prompt clear failed: {error}"))?;
            if !prepare_pause(prepare, state) {
                terminal
                    .exit()
                    .map_err(|error| format!("terminal restore failed: {error}"))?;
                return Ok(SessionExit::Quit);
            }
            redraw_prompt(editor).map_err(|error| format!("prompt render failed: {error}"))?;
        }

        if !event::poll(Duration::from_millis(EVENT_POLL_MS))
            .map_err(|error| format!("crossterm poll failed: {error}"))?
        {
            continue;
        }

        match event::read().map_err(|error| format!("crossterm read failed: {error}"))? {
            Event::Key(key) if key_is_active(key) => {
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(key.code, KeyCode::Char('q') | KeyCode::Char('c'))
                {
                    terminal
                        .exit()
                        .map_err(|error| format!("terminal restore failed: {error}"))?;
                    return Ok(SessionExit::Quit);
                }

                match key.code {
                    KeyCode::Esc | KeyCode::F(10) => {
                        terminal
                            .exit()
                            .map_err(|error| format!("terminal restore failed: {error}"))?;
                        return Ok(SessionExit::ReturnToShell);
                    }
                    KeyCode::Enter => {
                        let prompt = editor.take_submission();
                        finish_submitted_line(prompt.as_str())
                            .map_err(|error| format!("prompt finish failed: {error}"))?;
                        if prompt.is_empty() {
                            redraw_prompt(editor)
                                .map_err(|error| format!("prompt render failed: {error}"))?;
                            continue;
                        }
                        if matches!(prompt.as_str(), "quit" | ":quit" | ".quit" | ":q") {
                            terminal
                                .exit()
                                .map_err(|error| format!("terminal restore failed: {error}"))?;
                            return Ok(SessionExit::Quit);
                        }
                        if let Err(error) = run_prompt(state, prompt.as_str()) {
                            vshell::linef(format_args!("lumen-bp: prompt failed error={error}"));
                            terminal.exit().map_err(|restore| {
                                format!("{error}; terminal restore failed: {restore}")
                            })?;
                            return Err(error);
                        }
                        redraw_prompt(editor)
                            .map_err(|error| format!("prompt render failed: {error}"))?;
                    }
                    KeyCode::Backspace => {
                        editor.backspace();
                        redraw_prompt(editor)
                            .map_err(|error| format!("prompt render failed: {error}"))?;
                    }
                    KeyCode::Delete => {
                        editor.delete();
                        redraw_prompt(editor)
                            .map_err(|error| format!("prompt render failed: {error}"))?;
                    }
                    KeyCode::Left => {
                        editor.move_left();
                        redraw_prompt(editor)
                            .map_err(|error| format!("prompt render failed: {error}"))?;
                    }
                    KeyCode::Right => {
                        editor.move_right();
                        redraw_prompt(editor)
                            .map_err(|error| format!("prompt render failed: {error}"))?;
                    }
                    KeyCode::Home => {
                        editor.move_home();
                        redraw_prompt(editor)
                            .map_err(|error| format!("prompt render failed: {error}"))?;
                    }
                    KeyCode::End => {
                        editor.move_end();
                        redraw_prompt(editor)
                            .map_err(|error| format!("prompt render failed: {error}"))?;
                    }
                    KeyCode::Up => {
                        editor.history_up();
                        redraw_prompt(editor)
                            .map_err(|error| format!("prompt render failed: {error}"))?;
                    }
                    KeyCode::Down => {
                        editor.history_down();
                        redraw_prompt(editor)
                            .map_err(|error| format!("prompt render failed: {error}"))?;
                    }
                    KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        editor.clear();
                        redraw_prompt(editor)
                            .map_err(|error| format!("prompt render failed: {error}"))?;
                    }
                    KeyCode::Tab => {
                        editor.insert_text("    ");
                        redraw_prompt(editor)
                            .map_err(|error| format!("prompt render failed: {error}"))?;
                    }
                    KeyCode::Char(ch)
                        if !key.modifiers.contains(KeyModifiers::CONTROL)
                            && !key.modifiers.contains(KeyModifiers::ALT) =>
                    {
                        editor.insert_char(ch);
                        redraw_prompt(editor)
                            .map_err(|error| format!("prompt render failed: {error}"))?;
                    }
                    _ => {}
                }
            }
            Event::Paste(text) => {
                editor.insert_text(text.as_str());
                redraw_prompt(editor).map_err(|error| format!("prompt render failed: {error}"))?;
            }
            Event::Resize(_, _) => {
                redraw_prompt(editor).map_err(|error| format!("prompt render failed: {error}"))?;
            }
            _ => {}
        }
    }
}

fn initialize_lumen() -> Result<(), String> {
    let mut spinner = ProgressSpinner::start("lumen-bp: opening prefilled template");
    lumen::open_template(SYSTEM_PROMPT)
        .map_err(|error| format!("template open failed: {error:?}"))?;
    let ready = wait_for_phase_with_spinner(lumen::LUMEN_PHASE_READY, &mut spinner)?;
    clear_prompt_for_output().map_err(|error| format!("template status clear failed: {error}"))?;
    vshell::linef(format_args!(
        "lumen-bp: template ready prefix_tokens={} ownership=blueprint-policy+logical-state/kernel-model+igc+guc",
        ready.position
    ));
    vshell::line(
        "lumen-bp: type a prompt · Esc/F10 returns to Shell2 · vmx_tui resumes this session",
    );
    Ok(())
}

fn wait_for_reentry(
    ticket: vshell::TerminalParkingTicket,
    state: &mut LogicalState,
) -> Result<vshell::TerminalLease, String> {
    loop {
        if let Some(prepare) = replication::poll_prepare_pause()
            && !prepare_pause(prepare, state)
        {
            return Err(String::from(
                "Lumen restore failed while terminal was parked",
            ));
        }

        match ticket
            .poll_reentry()
            .map_err(|error| format!("terminal reentry poll failed: {error}"))?
        {
            vshell::TerminalReentry::Pending => {
                platform::poll_once();
                platform::sleep_ms(POLL_MS);
            }
            vshell::TerminalReentry::Ready(lease) => return Ok(lease),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SessionExit {
    ReturnToShell,
    Quit,
}

struct TerminalGuard {
    active: bool,
}

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let mut out = io::stdout();
        if let Err(error) = execute!(
            &mut out,
            EnterAlternateScreen,
            EnableBracketedPaste,
            Clear(ClearType::All),
            MoveTo(0, 0),
            Show
        ) {
            let _ = execute!(
                &mut out,
                DisableBracketedPaste,
                ResetColor,
                Show,
                LeaveAlternateScreen
            );
            let _ = terminal::disable_raw_mode();
            return Err(error);
        }
        if let Err(error) = out.flush() {
            let _ = execute!(
                &mut out,
                DisableBracketedPaste,
                ResetColor,
                Show,
                LeaveAlternateScreen
            );
            let _ = terminal::disable_raw_mode();
            return Err(error);
        }
        Ok(Self { active: true })
    }

    fn exit(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        self.active = false;

        let mut first_error = None;
        let mut out = io::stdout();
        if let Err(error) = execute!(
            &mut out,
            DisableBracketedPaste,
            ResetColor,
            Show,
            LeaveAlternateScreen
        ) {
            first_error = Some(error);
        }
        if let Err(error) = out.flush()
            && first_error.is_none()
        {
            first_error = Some(error);
        }
        if let Err(error) = terminal::disable_raw_mode()
            && first_error.is_none()
        {
            first_error = Some(error);
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.exit();
    }
}

struct InputEditor {
    text: String,
    cursor: usize,
    history: Vec<String>,
    history_index: Option<usize>,
}

impl InputEditor {
    fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_index: None,
        }
    }

    fn insert_char(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }
        self.text.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        self.history_index = None;
    }

    fn insert_text(&mut self, text: &str) {
        for ch in text.chars() {
            match ch {
                '\r' | '\n' => self.insert_char(' '),
                '\t' => {
                    for _ in 0..4 {
                        self.insert_char(' ');
                    }
                }
                _ => self.insert_char(ch),
            }
        }
    }

    fn backspace(&mut self) {
        let Some((start, _)) = self.text[..self.cursor].char_indices().next_back() else {
            return;
        };
        self.text.drain(start..self.cursor);
        self.cursor = start;
        self.history_index = None;
    }

    fn delete(&mut self) {
        let Some(ch) = self.text[self.cursor..].chars().next() else {
            return;
        };
        let end = self.cursor + ch.len_utf8();
        self.text.drain(self.cursor..end);
        self.history_index = None;
    }

    fn move_left(&mut self) {
        if let Some((start, _)) = self.text[..self.cursor].char_indices().next_back() {
            self.cursor = start;
        }
    }

    fn move_right(&mut self) {
        if let Some(ch) = self.text[self.cursor..].chars().next() {
            self.cursor += ch.len_utf8();
        }
    }

    fn move_home(&mut self) {
        self.cursor = 0;
    }

    fn move_end(&mut self) {
        self.cursor = self.text.len();
    }

    fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.history_index = None;
    }

    fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let index = self
            .history_index
            .and_then(|index| index.checked_sub(1))
            .unwrap_or(self.history.len() - 1);
        self.history_index = Some(index);
        self.text.clone_from(&self.history[index]);
        self.cursor = self.text.len();
    }

    fn history_down(&mut self) {
        let Some(index) = self.history_index else {
            return;
        };
        if index + 1 < self.history.len() {
            let next = index + 1;
            self.history_index = Some(next);
            self.text.clone_from(&self.history[next]);
            self.cursor = self.text.len();
        } else {
            self.clear();
        }
    }

    fn take_submission(&mut self) -> String {
        let raw = core::mem::take(&mut self.text);
        self.cursor = 0;
        self.history_index = None;
        let prompt = raw.trim().to_string();
        if !prompt.is_empty()
            && self
                .history
                .last()
                .is_none_or(|previous| previous != &prompt)
        {
            self.history.push(prompt.clone());
        }
        prompt
    }

    fn visible_window(&self, width: usize) -> (String, usize) {
        let width = width.max(1);
        let chars: Vec<char> = self.text.chars().collect();
        let cursor_chars = self.text[..self.cursor].chars().count();
        let start = cursor_chars.saturating_sub(width.saturating_sub(1));
        let end = (start + width).min(chars.len());
        let visible = chars[start..end].iter().collect();
        (visible, cursor_chars.saturating_sub(start))
    }
}

fn draw_session_header(turns: u64, resumed: bool) -> io::Result<()> {
    let mut out = io::stdout();
    execute!(
        &mut out,
        Clear(ClearType::All),
        MoveTo(0, 0),
        SetForegroundColor(Color::Cyan),
        Print("TRUEOS LUMEN"),
        ResetColor,
        Print("\r\n"),
        Print("Direct Crossterm terminal session\r\n"),
        Print("Esc/F10: return to Shell2 · vmx_tui: resume · Ctrl-Q/Ctrl-C: quit\r\n"),
        SetForegroundColor(Color::DarkGrey),
        Print(format!(
            "session={} turns={} input=Mio/Crossterm terminal handoff\r\n\r\n",
            if resumed { "resumed" } else { "initial" },
            turns
        )),
        ResetColor
    )?;
    out.flush()
}

fn redraw_prompt(editor: &InputEditor) -> io::Result<()> {
    const PREFIX: &str = "lumen> ";
    let width = terminal::size()
        .map(|(columns, _)| usize::from(columns).saturating_sub(PREFIX.len()))
        .unwrap_or(73)
        .max(1);
    let (visible, cursor) = editor.visible_window(width);
    let column = PREFIX
        .chars()
        .count()
        .saturating_add(cursor)
        .min(u16::MAX as usize) as u16;

    let mut out = io::stdout();
    queue!(
        &mut out,
        MoveToColumn(0),
        Clear(ClearType::CurrentLine),
        SetForegroundColor(Color::Yellow),
        Print(PREFIX),
        ResetColor,
        Print(visible),
        MoveToColumn(column)
    )?;
    out.flush()
}

fn clear_prompt_for_output() -> io::Result<()> {
    let mut out = io::stdout();
    queue!(&mut out, MoveToColumn(0), Clear(ClearType::CurrentLine))?;
    out.flush()
}

fn finish_submitted_line(text: &str) -> io::Result<()> {
    let mut out = io::stdout();
    queue!(
        &mut out,
        MoveToColumn(0),
        Clear(ClearType::CurrentLine),
        SetForegroundColor(Color::Yellow),
        Print("lumen> "),
        ResetColor,
        Print(text),
        Print("\r\n")
    )?;
    out.flush()
}

fn key_is_active(key: KeyEvent) -> bool {
    matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
}

fn diag(message_level: u8, message: impl logl::IntoLogMessage) {
    let _ = logl::log_record(message_level, LOG_TARGET, message);
}

struct ProgressSpinner {
    label: &'static str,
    frame: usize,
    cadence: usize,
    shift_cells: usize,
}

impl ProgressSpinner {
    fn start(label: &'static str) -> Self {
        let spinner = Self {
            label,
            frame: 0,
            cadence: 0,
            shift_cells: 0,
        };
        spinner.draw();
        spinner
    }

    fn tick(&mut self) {
        self.cadence = self.cadence.saturating_add(1);
        if self.cadence < SPINNER_CADENCE_POLLS {
            return;
        }
        self.cadence = 0;
        self.frame += 1;
        if self.frame == SPINNER_FRAMES.len() {
            self.frame = 0;
            self.shift_cells = (self.shift_cells + 1) % SPINNER_MAX_SHIFT_CELLS.saturating_add(1);
        }
        self.draw();
    }

    fn draw(&self) {
        let mut line = String::with_capacity(
            self.label
                .len()
                .saturating_add(self.shift_cells)
                .saturating_add(1)
                .saturating_add(SPINNER_FRAMES[self.frame].len()),
        );
        line.push_str(self.label);
        for _ in 0..=self.shift_cells {
            line.push(' ');
        }
        line.push_str(SPINNER_FRAMES[self.frame]);
        vshell::progress_line(line.as_str());
    }
}

fn prepare_pause(prepare: replication::PreparePause, state: &mut LogicalState) -> bool {
    vshell::linef(format_args!(
        "lumen-bp: PreparePause operation={} reason={:?}; exporting logical LFM state",
        prepare.operation(),
        prepare.reason
    ));
    if let Err(error) = lumen::request_checkpoint() {
        vshell::linef(format_args!(
            "lumen-bp: checkpoint request failed error={error:?}; not Ready"
        ));
        return true;
    }
    let checkpoint = match wait_for_phase(lumen::LUMEN_PHASE_CHECKPOINT_READY) {
        Ok(status) => match lumen::read_checkpoint(status) {
            Ok(checkpoint) => checkpoint,
            Err(error) => {
                vshell::linef(format_args!(
                    "lumen-bp: checkpoint read failed error={error:?}; not Ready"
                ));
                return true;
            }
        },
        Err(error) => {
            vshell::linef(format_args!(
                "lumen-bp: checkpoint failed error={error}; not Ready"
            ));
            return true;
        }
    };
    state.model_checkpoint = checkpoint;
    if let Err(error) = lumen::close() {
        vshell::linef(format_args!(
            "lumen-bp: capability release failed error={error:?}; not Ready"
        ));
        return true;
    }
    vshell::linef(format_args!(
        "lumen-bp: Ready logical_bytes={} turns={} capability_released=1",
        state.model_checkpoint.len(),
        state.turns
    ));

    let resume = match replication::ready(prepare, CHECKPOINT_VERSION) {
        Ok(resume) => resume,
        Err(error) => {
            vshell::linef(format_args!("lumen-bp: Ready rejected error={error:?}"));
            return true;
        }
    };
    vshell::linef(format_args!(
        "lumen-bp: Resume instance={} lineage={} generation={} clone={}; reacquiring Lumen",
        resume.instance_guid(),
        resume.lineage_guid(),
        resume.generation,
        resume.is_clone
    ));
    if let Err(error) = lumen::restore(&state.model_checkpoint) {
        vshell::linef(format_args!(
            "lumen-bp: restore upload failed error={error:?}"
        ));
        return false;
    }
    match wait_for_phase(lumen::LUMEN_PHASE_READY) {
        Ok(status) => {
            vshell::linef(format_args!(
                "lumen-bp: private session ready position={} baseline_replayed=0",
                status.position
            ));
            true
        }
        Err(error) => {
            vshell::linef(format_args!("lumen-bp: restore failed error={error}"));
            false
        }
    }
}

fn run_prompt(state: &mut LogicalState, prompt: &str) -> Result<(), String> {
    lumen::submit_prompt(
        state.turns,
        &state.reply_tail[..state.reply_tail_len],
        prompt,
    )
    .map_err(|error| alloc::format!("submit {error:?}"))?;
    let mut spinner = ProgressSpinner::start("lumen-bp: reasoning");
    let status = wait_for_phase_with_spinner(lumen::LUMEN_PHASE_REPLY_READY, &mut spinner)?;
    clear_prompt_for_output().map_err(|error| format!("reasoning status clear failed: {error}"))?;
    let tail_len = (status.reply_tail_len as usize).min(state.reply_tail.len());
    state.reply_tail = status.reply_tail;
    state.reply_tail_len = tail_len;
    let raw = lumen::take_reply(status).map_err(|error| alloc::format!("read {error:?}"))?;
    let response_turn = state.turns.saturating_add(1);
    log_raw_reply(response_turn, raw.as_slice());
    let raw = String::from_utf8_lossy(&raw);
    let adapted = adapt_tool_reply(raw.as_ref());
    match adapted.tool {
        ToolDisposition::None => {
            emit_text_reply(response_turn, adapted.text.as_str());
        }
        ToolDisposition::Text(text) => {
            emit_text_reply(response_turn, text.as_str());
        }
        ToolDisposition::Executed => {
            if adapted.text.is_empty() {
                vshell::line("lumen-bp: tool objective handed to Spirit");
            } else {
                emit_text_reply(response_turn, adapted.text.as_str());
            }
        }
        ToolDisposition::Rejected => {
            vshell::line("lumen-bp: rejected malformed or unknown tool objective");
            if adapted.text.is_empty() {
                emit_text_reply(response_turn, rejected_tool_fallback());
            } else {
                emit_text_reply(response_turn, adapted.text.as_str());
            }
        }
        ToolDisposition::Failed => {
            if adapted.text.is_empty() {
                emit_text_reply(
                    response_turn,
                    "Spirit could not accept that action right now.",
                );
            } else {
                emit_text_reply(response_turn, adapted.text.as_str());
            }
        }
    }
    state.turns = response_turn;
    Ok(())
}

fn log_raw_reply(turn: u64, raw: &[u8]) {
    match core::str::from_utf8(raw) {
        Ok(text) => diag(
            level::INFO,
            format_args!(
                "raw-reply turn={} bytes={} utf8=1 text={:?}",
                turn,
                raw.len(),
                text,
            ),
        ),
        Err(error) => diag(
            level::INFO,
            format_args!(
                "raw-reply turn={} bytes={} utf8=0 valid_up_to={} error_len={:?} raw_bytes={:?}",
                turn,
                raw.len(),
                error.valid_up_to(),
                error.error_len(),
                raw,
            ),
        ),
    }
}

fn emit_text_reply(turn: u64, text: &str) {
    if let Err(error) = lumen::present_reply(turn, text) {
        vshell::linef(format_args!(
            "lumen-bp: Spirit response handoff failed error={error:?}"
        ));
    }
    vshell::linef(format_args!("lumen: {text}"));
}

fn wait_for_phase(expected: u32) -> Result<lumen::TrueosLumenStatus, String> {
    wait_for_phase_inner(expected, None)
}

fn wait_for_phase_with_spinner(
    expected: u32,
    spinner: &mut ProgressSpinner,
) -> Result<lumen::TrueosLumenStatus, String> {
    wait_for_phase_inner(expected, Some(spinner))
}

fn wait_for_phase_inner(
    expected: u32,
    mut spinner: Option<&mut ProgressSpinner>,
) -> Result<lumen::TrueosLumenStatus, String> {
    for _ in 0..MAX_WAIT_POLLS {
        let status = lumen::status().map_err(|error| alloc::format!("status {error:?}"))?;
        if status.phase == expected {
            return Ok(status);
        }
        if status.phase == lumen::LUMEN_PHASE_ERROR {
            return Err(alloc::format!("kernel error={}", status.error));
        }
        if let Some(spinner) = spinner.as_deref_mut() {
            spinner.tick();
        }
        platform::poll_once();
        platform::sleep_ms(POLL_MS);
    }
    Err(String::from("timeout"))
}

struct AdaptedReply {
    text: String,
    tool: ToolDisposition,
}

enum ToolDisposition {
    None,
    Text(String),
    Executed,
    Rejected,
    Failed,
}

fn adapt_tool_reply(raw: &str) -> AdaptedReply {
    const START: &str = "<|tool_call_start|>";
    const END: &str = "<|tool_call_end|>";
    let Some(start) = raw.find(START) else {
        return AdaptedReply {
            text: raw.trim().to_string(),
            tool: ToolDisposition::None,
        };
    };
    let payload_start = start + START.len();
    let Some(relative_end) = raw[payload_start..].find(END) else {
        return AdaptedReply {
            text: remove_span(raw, start, raw.len()),
            tool: ToolDisposition::Rejected,
        };
    };
    let payload_end = payload_start + relative_end;
    let span_end = payload_end + END.len();
    let text = remove_span(raw, start, span_end);
    AdaptedReply {
        text,
        tool: route_tool_call(&raw[payload_start..payload_end]),
    }
}

struct ParsedToolCall<'a> {
    name: &'a str,
    arguments: &'a str,
}

fn route_tool_call(payload: &str) -> ToolDisposition {
    let Some(call) = parse_tool_call(payload) else {
        return ToolDisposition::Rejected;
    };
    match call.name {
        "text" => route_text(call.arguments),
        "play_emotion" => route_play_emotion(call.arguments),
        "move" => route_move(call.arguments),
        _ => ToolDisposition::Rejected,
    }
}

fn route_text(arguments: &str) -> ToolDisposition {
    let Some(text) = parse_string_argument(arguments, "text") else {
        return ToolDisposition::Rejected;
    };
    if text.trim().is_empty() {
        return ToolDisposition::Rejected;
    }
    ToolDisposition::Text(text)
}

fn route_play_emotion(arguments: &str) -> ToolDisposition {
    let Some(idea) = parse_emotion_idea(arguments) else {
        return ToolDisposition::Rejected;
    };
    let accepted = lumen::play_emotion(idea).is_ok();
    vshell::linef(format_args!(
        "lumen-bp: Spirit emotion idea={} accepted={}",
        idea, accepted as u8
    ));
    if accepted {
        ToolDisposition::Executed
    } else {
        ToolDisposition::Failed
    }
}

fn route_move(arguments: &str) -> ToolDisposition {
    let Some((x, y)) = parse_move_arguments(arguments) else {
        return ToolDisposition::Rejected;
    };
    let accepted = lumen::move_spirit(x, y).is_ok();
    vshell::linef(format_args!(
        "lumen-bp: Spirit move x={:.3} y={:.3} accepted={}",
        x, y, accepted as u8
    ));
    if accepted {
        ToolDisposition::Executed
    } else {
        ToolDisposition::Failed
    }
}

fn parse_tool_call(payload: &str) -> Option<ParsedToolCall<'_>> {
    let payload = payload.trim();
    let payload = payload
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(payload)
        .trim();
    let open = payload.find('(')?;
    let name = payload.get(..open)?.trim();
    if name.is_empty() {
        return None;
    }
    let arguments = payload.get(open + 1..)?.strip_suffix(')')?.trim();
    Some(ParsedToolCall { name, arguments })
}

fn parse_emotion_idea(arguments: &str) -> Option<&'static str> {
    let value = arguments
        .strip_prefix("idea")?
        .trim()
        .strip_prefix('=')?
        .trim();
    let quote = value.as_bytes().first().copied()?;
    if !matches!(quote, b'\'' | b'"') || value.as_bytes().last().copied() != Some(quote) {
        return None;
    }
    match value.get(1..value.len().checked_sub(1)?)? {
        "anger" => Some("anger"),
        "disgust" => Some("disgust"),
        "fear" => Some("fear"),
        "joy" => Some("joy"),
        "sadness" => Some("sadness"),
        "surprise" => Some("surprise"),
        _ => None,
    }
}

fn parse_string_argument(arguments: &str, name: &str) -> Option<String> {
    let value = arguments
        .strip_prefix(name)?
        .trim()
        .strip_prefix('=')?
        .trim();
    let quote = value.as_bytes().first().copied()?;
    if !matches!(quote, b'\'' | b'"') || value.as_bytes().last().copied() != Some(quote) {
        return None;
    }
    let inner = value.get(1..value.len().checked_sub(1)?)?;
    let mut output = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        match chars.next()? {
            '\\' => output.push('\\'),
            '"' => output.push('"'),
            '\'' => output.push('\''),
            'n' => output.push('\n'),
            'r' => output.push('\r'),
            't' => output.push('\t'),
            _ => return None,
        }
    }
    Some(output)
}

fn parse_move_arguments(arguments: &str) -> Option<(f32, f32)> {
    let mut x = None;
    let mut y = None;
    for argument in arguments.split(',') {
        let (name, value) = argument.split_once('=')?;
        let value = value.trim().parse::<f32>().ok()?;
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return None;
        }
        match name.trim() {
            "x" if x.is_none() => x = Some(value),
            "y" if y.is_none() => y = Some(value),
            _ => return None,
        }
    }
    Some((x?, y?))
}

fn rejected_tool_fallback() -> &'static str {
    "I could not form a valid tool call or text reply for that turn; please try once more."
}

fn remove_span(raw: &str, start: usize, end: usize) -> String {
    let before = raw.get(..start).unwrap_or_default().trim_end();
    let after = raw.get(end..).unwrap_or_default().trim_start();
    let mut text = String::with_capacity(before.len() + after.len() + 1);
    text.push_str(before);
    if !before.is_empty() && !after.is_empty() {
        text.push(' ');
    }
    text.push_str(after);
    text
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::{parse_emotion_idea, parse_move_arguments, parse_string_argument, parse_tool_call};

    #[test]
    fn structured_tool_call_routes_by_registered_name() {
        let call = parse_tool_call("[play_emotion(idea='joy')]").unwrap();
        assert_eq!(call.name, "play_emotion");
        assert_eq!(parse_emotion_idea(call.arguments), Some("joy"));
    }

    #[test]
    fn malformed_or_out_of_schema_emotion_is_rejected() {
        assert!(parse_tool_call("play_emotion idea='joy'").is_none());
        assert_eq!(parse_emotion_idea("idea='calm'"), None);
        assert_eq!(parse_emotion_idea("idea=joy"), None);
    }

    #[test]
    fn text_argument_unescapes_model_function_syntax() {
        assert_eq!(
            parse_string_argument(r#"text=\"Hello, \\\"Lilly\\\"!\""#, "text").as_deref(),
            Some("Hello, \"Lilly\"!")
        );
        assert!(parse_string_argument("text=hello", "text").is_none());
    }

    #[test]
    fn move_arguments_are_named_bounded_and_order_independent() {
        assert_eq!(parse_move_arguments("x=0.25, y=1"), Some((0.25, 1.0)));
        assert_eq!(parse_move_arguments("y=0.75,x=0"), Some((0.0, 0.75)));
        assert!(parse_move_arguments("x=-0.1,y=0.5").is_none());
        assert!(parse_move_arguments("x=0.5,x=0.6,y=0.5").is_none());
    }
}
