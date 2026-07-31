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
        Ok(text) => vshell::linef(format_args!(
            "lumen-bp: raw-reply turn={} bytes={} utf8=1 text={:?}",
            turn,
            raw.len(),
            text,
        )),
        Err(error) => vshell::linef(format_args!(
            "lumen-bp: raw-reply turn={} bytes={} utf8=0 valid_up_to={} error_len={:?} raw_bytes={:?}",
            turn,
            raw.len(),
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
