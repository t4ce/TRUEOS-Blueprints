#![no_std]

// trueos-blueprint: features = ["lumen"]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use trueos::{lumen, platform, replication, vshell};

const CHECKPOINT_VERSION: u64 = 1;
const POLL_MS: u64 = 10;
const MAX_WAIT_POLLS: usize = 18_000;
const INPUT_BYTES: usize = 4096;
const SPINNER_CADENCE_POLLS: usize = 4;
const SPINNER_MAX_SHIFT_CELLS: usize = 5;
const SPINNER_FRAMES: &[&str] = &["⢈", "⡈", "⡐", "⡠", "⣀", "⢄", "⢂", "⢁", "⡁"];

const SYSTEM_PROMPT: &str = concat!(
    "You are Lilly, a concise helpful assistant. ",
    "Default to a natural-language reply without a tool. Call play_emotion only when the current ",
    "user explicitly asks Lilly or Spirit to play, show, or express an emotion. Never call it for ",
    "a greeting, an ordinary question, or merely emotional wording. The Spirit action is real. ",
    "List of tools: [{\"name\":\"play_emotion\",\"description\":\"Play one explicitly requested ",
    "emotional idea through Lilly.\",\"parameters\":{\"type\":\"object\",",
    "\"properties\":{\"idea\":{\"type\":\"string\",\"enum\":[\"anger\",\"disgust\",",
    "\"fear\",\"joy\",\"sadness\",\"surprise\"]}},\"required\":[\"idea\"],",
    "\"additionalProperties\":false}}]. ",
    "Use at most one tool call and otherwise answer in plain text."
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
    let mut spinner = ProgressSpinner::start("lumen-bp: opening prefilled template");
    if let Err(error) = lumen::open_template(SYSTEM_PROMPT) {
        vshell::linef(format_args!(
            "lumen-bp: template open failed error={error:?}"
        ));
        return;
    }
    let ready = match wait_for_phase_with_spinner(lumen::LUMEN_PHASE_READY, &mut spinner) {
        Ok(status) => status,
        Err(error) => {
            vshell::linef(format_args!(
                "lumen-bp: template prefill failed error={error}"
            ));
            return;
        }
    };
    vshell::linef(format_args!(
        "lumen-bp: template ready prefix_tokens={} ownership=blueprint-policy+logical-state/kernel-model+igc+guc",
        ready.position
    ));
    vshell::line(
        "lumen-bp: replicate/pause this template now, or type a prompt for direct ABI bring-up",
    );

    let mut state = LogicalState::new();
    let mut input = [0u8; INPUT_BYTES];
    loop {
        if let Some(prepare) = replication::poll_prepare_pause() {
            if !prepare_pause(prepare, &mut state) {
                return;
            }
            continue;
        }

        let read = vshell::read(&mut input);
        if read == 0 {
            platform::poll_once();
            platform::sleep_ms(16);
            continue;
        }
        let prompt = trim_ascii(&input[..read]);
        if prompt == b"quit" {
            let _ = lumen::close();
            return;
        }
        let Ok(prompt) = core::str::from_utf8(prompt) else {
            vshell::line("lumen-bp: prompt must be UTF-8");
            continue;
        };
        if prompt.is_empty() {
            continue;
        }
        if let Err(error) = run_prompt(&mut state, prompt) {
            vshell::linef(format_args!("lumen-bp: prompt failed error={error}"));
            return;
        }
    }
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
    let tail_len = (status.reply_tail_len as usize).min(state.reply_tail.len());
    state.reply_tail = status.reply_tail;
    state.reply_tail_len = tail_len;
    let raw = lumen::take_reply(status).map_err(|error| alloc::format!("read {error:?}"))?;
    let response_turn = state.turns.saturating_add(1);
    let tool_authorized = emotion_tool_requested(prompt);
    log_raw_reply(response_turn, raw.as_slice(), tool_authorized);
    let raw = String::from_utf8_lossy(&raw);
    let adapted = adapt_tool_reply(raw.as_ref(), tool_authorized);
    match adapted.tool {
        ToolDisposition::None => {
            emit_text_reply(response_turn, adapted.text.as_str());
        }
        ToolDisposition::Executed => {
            if adapted.text.is_empty() {
                vshell::line("lumen-bp: tool objective handed to Spirit");
            } else {
                emit_text_reply(response_turn, adapted.text.as_str());
            }
        }
        ToolDisposition::Rejected => {
            vshell::line("lumen-bp: suppressed tool objective without explicit user intent");
            if adapted.text.is_empty() {
                emit_text_reply(response_turn, rejected_tool_fallback(prompt));
            } else {
                emit_text_reply(response_turn, adapted.text.as_str());
            }
        }
        ToolDisposition::Failed => {
            if adapted.text.is_empty() {
                emit_text_reply(
                    response_turn,
                    "Spirit could not accept that emotion right now.",
                );
            } else {
                emit_text_reply(response_turn, adapted.text.as_str());
            }
        }
    }
    state.turns = response_turn;
    Ok(())
}

fn log_raw_reply(turn: u64, raw: &[u8], tool_authorized: bool) {
    match core::str::from_utf8(raw) {
        Ok(text) => vshell::linef(format_args!(
            "lumen-bp: raw-reply turn={} bytes={} utf8=1 tool_authorized={} text={:?}",
            turn,
            raw.len(),
            tool_authorized as u8,
            text,
        )),
        Err(error) => vshell::linef(format_args!(
            "lumen-bp: raw-reply turn={} bytes={} utf8=0 tool_authorized={} valid_up_to={} error_len={:?} raw_bytes={:?}",
            turn,
            raw.len(),
            tool_authorized as u8,
            error.valid_up_to(),
            error.error_len(),
            raw,
        )),
    };
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
    Executed,
    Rejected,
    Failed,
}

fn adapt_tool_reply(raw: &str, tool_authorized: bool) -> AdaptedReply {
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
    let Some(idea) = parse_emotion_call(&raw[payload_start..payload_end]) else {
        return AdaptedReply {
            text: remove_span(raw, start, span_end),
            tool: ToolDisposition::Rejected,
        };
    };
    let text = remove_span(raw, start, span_end);
    if !tool_authorized {
        return AdaptedReply {
            text,
            tool: ToolDisposition::Rejected,
        };
    }
    let accepted = lumen::play_emotion(idea).is_ok();
    vshell::linef(format_args!(
        "lumen-bp: Spirit emotion idea={} accepted={}",
        idea, accepted as u8
    ));
    AdaptedReply {
        text,
        tool: if accepted {
            ToolDisposition::Executed
        } else {
            ToolDisposition::Failed
        },
    }
}

fn emotion_tool_requested(prompt: &str) -> bool {
    const ACTIONS: &[&str] = &[
        "animate", "display", "express", "invoke", "make", "perform", "play", "show", "trigger",
        "use",
    ];
    const TARGETS: &[&str] = &[
        "anger", "disgust", "emotion", "emotions", "fear", "happy", "joy", "lilly", "sad",
        "sadness", "spirit", "surprise",
    ];
    ACTIONS.iter().any(|word| contains_ascii_word(prompt, word))
        && TARGETS.iter().any(|word| contains_ascii_word(prompt, word))
}

fn contains_ascii_word(text: &str, expected: &str) -> bool {
    text.split(|ch: char| !ch.is_ascii_alphanumeric())
        .any(|word| word.eq_ignore_ascii_case(expected))
}

fn rejected_tool_fallback(prompt: &str) -> &'static str {
    const GREETINGS: &[&str] = &["hello", "hey", "hi"];
    if GREETINGS
        .iter()
        .any(|word| contains_ascii_word(prompt, word))
    {
        "Hello! How can I help you today?"
    } else {
        "I could not form a text reply for that turn; please try the request once more."
    }
}

fn parse_emotion_call(payload: &str) -> Option<&'static str> {
    let payload = payload.trim();
    let payload = payload
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(payload)
        .trim();
    let value = payload
        .strip_prefix("play_emotion")?
        .trim()
        .strip_prefix('(')?
        .strip_suffix(')')?
        .trim()
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
    use super::emotion_tool_requested;

    #[test]
    fn ordinary_language_does_not_authorize_spirit() {
        assert!(!emotion_tool_requested("hi"));
        assert!(!emotion_tool_requested("What is joy?"));
        assert!(!emotion_tool_requested("I feel sad today."));
    }

    #[test]
    fn explicit_emotion_actions_authorize_spirit() {
        assert!(emotion_tool_requested("Please play joy."));
        assert!(emotion_tool_requested("Ask Spirit to show an emotion."));
        assert!(emotion_tool_requested("Can Lilly express surprise?"));
    }
}
