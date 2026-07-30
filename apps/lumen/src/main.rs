#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use trueos::{lumen, platform, replication, vshell};

const CHECKPOINT_VERSION: u64 = 1;
const POLL_MS: u64 = 10;
const MAX_WAIT_POLLS: usize = 18_000;
const INPUT_BYTES: usize = 4096;

const SYSTEM_PROMPT: &str = concat!(
    "You are Lilly, a concise helpful assistant. ",
    "You can directly ask Spirit to play an emotion; this is a real action, not a hypothetical ",
    "external tool. List of tools: [{\"name\":\"play_emotion\",\"description\":\"Play one fitting ",
    "emotional idea through Lilly when it adds meaning.\",\"parameters\":{\"type\":\"object\",",
    "\"properties\":{\"idea\":{\"type\":\"string\",\"enum\":[\"anger\",\"disgust\",",
    "\"fear\",\"joy\",\"sadness\",\"surprise\"]}},\"required\":[\"idea\"],",
    "\"additionalProperties\":false}}]. ",
    "Invoke it immediately when requested. Use at most one tool call per reply and continue with ",
    "the natural-language answer."
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
    vshell::line("lumen-bp: opening prefilled template");
    if let Err(error) = lumen::open_template(SYSTEM_PROMPT) {
        vshell::linef(format_args!(
            "lumen-bp: template open failed error={error:?}"
        ));
        return;
    }
    let ready = match wait_for_phase(lumen::LUMEN_PHASE_READY) {
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
    let status = wait_for_phase(lumen::LUMEN_PHASE_REPLY_READY)?;
    let tail_len = (status.reply_tail_len as usize).min(state.reply_tail.len());
    state.reply_tail = status.reply_tail;
    state.reply_tail_len = tail_len;
    let raw = lumen::take_reply(status).map_err(|error| alloc::format!("read {error:?}"))?;
    let raw = String::from_utf8_lossy(&raw);
    let adapted = adapt_tool_reply(raw.as_ref());
    if adapted.tool_only {
        vshell::line("lumen-bp: tool objective handed to Spirit");
    } else {
        vshell::linef(format_args!("lumen: {}", adapted.text));
    }
    state.turns = state.turns.saturating_add(1);
    Ok(())
}

fn wait_for_phase(expected: u32) -> Result<lumen::TrueosLumenStatus, String> {
    for _ in 0..MAX_WAIT_POLLS {
        let status = lumen::status().map_err(|error| alloc::format!("status {error:?}"))?;
        if status.phase == expected {
            return Ok(status);
        }
        if status.phase == lumen::LUMEN_PHASE_ERROR {
            return Err(alloc::format!("kernel error={}", status.error));
        }
        platform::poll_once();
        platform::sleep_ms(POLL_MS);
    }
    Err(String::from("timeout"))
}

struct AdaptedReply {
    text: String,
    tool_only: bool,
}

fn adapt_tool_reply(raw: &str) -> AdaptedReply {
    const START: &str = "<|tool_call_start|>";
    const END: &str = "<|tool_call_end|>";
    let Some(start) = raw.find(START) else {
        return AdaptedReply {
            text: raw.trim().to_string(),
            tool_only: false,
        };
    };
    let payload_start = start + START.len();
    let Some(relative_end) = raw[payload_start..].find(END) else {
        return AdaptedReply {
            text: remove_span(raw, start, raw.len()),
            tool_only: false,
        };
    };
    let payload_end = payload_start + relative_end;
    let span_end = payload_end + END.len();
    let Some(idea) = parse_emotion_call(&raw[payload_start..payload_end]) else {
        return AdaptedReply {
            text: remove_span(raw, start, span_end),
            tool_only: false,
        };
    };
    let text = remove_span(raw, start, span_end);
    let accepted = lumen::play_emotion(idea).is_ok();
    vshell::linef(format_args!(
        "lumen-bp: Spirit emotion idea={} accepted={}",
        idea, accepted as u8
    ));
    AdaptedReply {
        tool_only: text.is_empty(),
        text,
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
