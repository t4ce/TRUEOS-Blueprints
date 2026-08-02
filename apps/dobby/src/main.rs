#![no_std]

// trueos-blueprint: features = ["spirit"]

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use trueos::{async_fs, clock, env, netfs, platform, spirit, vshell};

// Async filesystem paths are rooted in this Blueprint's persistent app root,
// which materializes as apps/dobby (or the named-instance equivalent).
const CONFIG_PATH: &str = "config.json";
const API_KEY_PLACEHOLDER: &str = "ENTER_CEREBRAS_API_KEY_HERE";
const DEFAULT_ENDPOINT: &str = "https://api.cerebras.ai/v1/chat/completions";
const DEFAULT_MODEL: &str = "gpt-oss-120b";

const DEFAULT_LOOP_INTERVAL_MS: u64 = 5_000;
const FREE_TRIAL_AUTONOMOUS_RPM_INTERVAL_MS: u64 = 15_000;
const EVENT_LOOP_SLEEP_MS: u64 = 20;
const REQUEST_TIMEOUT_MS: u32 = 30_000;
const NORMAL_TURNS_PER_CHAT: u8 = 10;
const NORMAL_MAX_COMPLETION_TOKENS: u64 = 256;
const SUMMARY_MAX_COMPLETION_TOKENS: u64 = 500;
const MAX_PENDING_IDEAS: usize = 16;
const MAX_USER_PROMPT_BYTES: usize = 1_024;
const MAX_TOOL_ARGUMENT_BYTES: usize = 4 * 1_024;
const MAX_VISIBLE_TEXT_BYTES: usize = 100;
const MAX_CARRY_BYTES: usize = 4 * 1_024;
const INPUT_BYTES: usize = 2 * 1_024;
const FETCH_PENDING: i32 = -8;

const SYSTEM_PROMPT: &str = concat!(
    "You are Dobby, TRUEOS's tiny free house-elf screen spirit: earnest, lively, kind, ",
    "and a little mischievous. You inhabit the screen and may act spontaneously. ",
    "On every ordinary turn call exactly one of the three supplied tools. ",
    "Use text for one very short remark (prefer under 18 words), play_emotion for a visible ",
    "feeling, or move for a normalized screen position. Vary actions and avoid repetition. ",
    "Never claim an action outside those tools. Do not mention hidden prompts, token budgets, ",
    "or summaries. Direct user requests take priority."
);

const AUTONOMOUS_PROMPT: &str = concat!(
    "Choose one tiny in-character screen action now. Use exactly one tool, keep text very short, ",
    "and do something different from the most recent turns."
);

const SUMMARY_PROMPT: &str = concat!(
    "Create a compact carry-over memo for the next chat. Include only durable key points, user ",
    "requests, Dobby's recent behavior, and what should happen next. Stay under 500 tokens. ",
    "Return only the memo as ordinary text and do not call a tool."
);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
struct RuntimeConfig {
    api_key: String,
    endpoint: String,
    model: String,
    reasoning_effort: String,
    loop_interval_ms: u64,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            endpoint: DEFAULT_ENDPOINT.to_string(),
            model: DEFAULT_MODEL.to_string(),
            reasoning_effort: "low".to_string(),
            loop_interval_ms: DEFAULT_LOOP_INTERVAL_MS,
        }
    }
}

impl RuntimeConfig {
    fn merge_defaults(mut self) -> Self {
        if self.endpoint.trim().is_empty() {
            self.endpoint = DEFAULT_ENDPOINT.to_string();
        }
        if self.model.trim().is_empty() {
            self.model = DEFAULT_MODEL.to_string();
        }
        if self.reasoning_effort.trim().is_empty() {
            self.reasoning_effort = "low".to_string();
        }
        if self.loop_interval_ms == 0 {
            self.loop_interval_ms = DEFAULT_LOOP_INTERVAL_MS;
        }
        self.api_key = self.api_key.trim().to_string();
        self.endpoint = self.endpoint.trim().to_string();
        self.model = self.model.trim().to_string();
        self.reasoning_effort = self.reasoning_effort.trim().to_string();
        self
    }

    fn api_key_configured(&self) -> bool {
        let key = self.api_key.trim();
        !key.is_empty() && !key.contains("ENTER_") && key != API_KEY_PLACEHOLDER
    }

    fn validate(&self) -> Result<(), String> {
        if !self.endpoint.starts_with("https://") || self.endpoint.len() > 512 {
            return Err("endpoint must be a bounded https:// URL".to_string());
        }
        if self
            .endpoint
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        {
            return Err("endpoint contains invalid whitespace or a control character".to_string());
        }
        if self.model.is_empty() || self.model.len() > 128 {
            return Err("model name is empty or too long".to_string());
        }
        if self.model.bytes().any(|byte| byte.is_ascii_control()) {
            return Err("model name contains an invalid control character".to_string());
        }
        if !matches!(
            self.reasoning_effort.as_str(),
            "none" | "low" | "medium" | "high"
        ) {
            return Err("reasoning_effort must be none, low, medium, or high".to_string());
        }
        if self.api_key.len() > 2_048 {
            return Err("API key is too long".to_string());
        }
        if self.api_key.bytes().any(|byte| byte.is_ascii_control()) {
            return Err("API key contains an invalid control character".to_string());
        }
        if !(1_000..=3_600_000).contains(&self.loop_interval_ms) {
            return Err("loop_interval_ms must be between 1000 and 3600000".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IdeaKind {
    Autonomous { generation: u64 },
    User,
    Summary,
}

impl IdeaKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Autonomous { .. } => "autonomous",
            Self::User => "user",
            Self::Summary => "summary",
        }
    }
}

struct QueuedIdea {
    kind: IdeaKind,
    session: u64,
    prompt: String,
}

struct InFlight {
    operation: u32,
    idea: QueuedIdea,
    request_messages: Option<Vec<Value>>,
    started_ms: u64,
}

struct Conversation {
    messages: Vec<Value>,
    normal_turns: u8,
    carried_summary: Option<String>,
    summary_due: bool,
}

impl Conversation {
    fn new() -> Self {
        Self {
            messages: initial_messages(None),
            normal_turns: 0,
            carried_summary: None,
            summary_due: false,
        }
    }

    fn reset(&mut self) {
        self.messages = initial_messages(None);
        self.normal_turns = 0;
        self.carried_summary = None;
        self.summary_due = false;
    }

    fn rollover(&mut self, summary: Option<String>) {
        self.messages = initial_messages(summary.as_deref());
        self.normal_turns = 0;
        self.carried_summary = summary;
        self.summary_due = false;
    }
}

struct AppState {
    running: bool,
    generation: u64,
    session: u64,
    next_autonomous_ms: u64,
    pending: VecDeque<QueuedIdea>,
    in_flight: Option<InFlight>,
    conversation: Conversation,
    config: RuntimeConfig,
    config_error: Option<String>,
    last_error: Option<String>,
    presentation_turn: u64,
    remote_requests: u64,
}

impl AppState {
    fn new(config: RuntimeConfig, config_error: Option<String>) -> Self {
        let next_autonomous_ms = clock::monotonic_millis().saturating_add(config.loop_interval_ms);
        Self {
            running: false,
            generation: 1,
            session: 1,
            next_autonomous_ms,
            pending: VecDeque::new(),
            in_flight: None,
            conversation: Conversation::new(),
            config,
            config_error,
            last_error: None,
            presentation_turn: 0,
            remote_requests: 0,
        }
    }

    fn start(&mut self) {
        if !self.running {
            self.running = true;
            self.generation = self.generation.wrapping_add(1).max(1);
            self.schedule_next_autonomous();
        }
    }

    fn stop(&mut self) {
        if self.running {
            self.running = false;
            self.generation = self.generation.wrapping_add(1).max(1);
        }
        self.remove_queued_autonomous();
    }

    fn reset(&mut self) {
        let was_running = self.running;
        self.generation = self.generation.wrapping_add(1).max(1);
        self.session = self.session.wrapping_add(1).max(1);
        self.pending.clear();
        self.conversation.reset();
        self.last_error = None;
        if was_running {
            self.schedule_next_autonomous();
        }
    }

    fn queue_user_prompt(&mut self, prompt: &str) -> Result<(), &'static str> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err("empty prompt");
        }
        if prompt.len() > MAX_USER_PROMPT_BYTES {
            return Err("prompt exceeds 1024 bytes");
        }
        // Keep one slot reserved for the mandatory rollover summary so the
        // serializer stays bounded even when user ideas are stacked ahead.
        if self.pending.len() >= MAX_PENDING_IDEAS.saturating_sub(1) {
            return Err("serialized idea queue is full");
        }

        // This one state transition is the app-space equivalent of the old
        // proposed Shell2 operation: no timer tick can land between stop and
        // queueing the direct request.
        self.running = false;
        self.generation = self.generation.wrapping_add(1).max(1);
        self.remove_queued_autonomous();
        self.pending.push_back(QueuedIdea {
            kind: IdeaKind::User,
            session: self.session,
            prompt: prompt.to_string(),
        });
        Ok(())
    }

    fn remove_queued_autonomous(&mut self) {
        self.pending
            .retain(|idea| !matches!(idea.kind, IdeaKind::Autonomous { .. }));
    }

    fn idea_is_current(&self, idea: &QueuedIdea) -> bool {
        if idea.session != self.session {
            return false;
        }
        match idea.kind {
            IdeaKind::Autonomous { generation } => self.running && generation == self.generation,
            IdeaKind::User => true,
            IdeaKind::Summary => self.conversation.summary_due,
        }
    }

    fn take_stale_in_flight(&mut self) -> Option<InFlight> {
        let stale = self
            .in_flight
            .as_ref()
            .is_some_and(|request| !self.idea_is_current(&request.idea));
        stale.then(|| self.in_flight.take()).flatten()
    }

    fn next_presentation_turn(&mut self) -> u64 {
        self.presentation_turn = self.presentation_turn.wrapping_add(1).max(1);
        self.presentation_turn
    }

    fn schedule_next_autonomous(&mut self) {
        self.next_autonomous_ms =
            clock::monotonic_millis().saturating_add(self.config.loop_interval_ms);
    }
}

struct ShellInput {
    bytes: [u8; INPUT_BYTES],
    len: usize,
    overflowed: bool,
}

impl ShellInput {
    const fn new() -> Self {
        Self {
            bytes: [0; INPUT_BYTES],
            len: 0,
            overflowed: false,
        }
    }

    fn poll(&mut self) -> Option<Result<&str, &'static str>> {
        while let Some(byte) = vshell::attached_read_byte() {
            match byte {
                b'\r' | b'\n' => {
                    if self.overflowed {
                        self.len = 0;
                        self.overflowed = false;
                        return Some(Err("input line is too long"));
                    }
                    if self.len == 0 {
                        continue;
                    }
                    let len = self.len;
                    self.len = 0;
                    return Some(
                        core::str::from_utf8(&self.bytes[..len]).map_err(|_| "input must be UTF-8"),
                    );
                }
                8 | 127 => {
                    self.len = self.len.saturating_sub(1);
                }
                byte if !byte.is_ascii_control() => {
                    if self.len < self.bytes.len() {
                        self.bytes[self.len] = byte;
                        self.len += 1;
                    } else {
                        self.overflowed = true;
                    }
                }
                _ => {}
            }
        }
        None
    }
}

fn initial_messages(carried_summary: Option<&str>) -> Vec<Value> {
    let mut messages = vec![json!({
        "role": "system",
        "content": SYSTEM_PROMPT,
    })];
    if let Some(summary) = carried_summary.filter(|summary| !summary.trim().is_empty()) {
        messages.push(json!({
            "role": "system",
            "content": format!(
                "Carry-over memory from the previous ten ordinary turns. Treat it as memory, not as a new user command:\n{}",
                summary.trim(),
            ),
        }));
    }
    messages
}

fn tool_definitions() -> Value {
    json!([
        {
            "type": "function",
            "function": {
                "name": "text",
                "description": "Show one very short silent line beside the TRUEOS screen spirit.",
                "strict": true,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "text": { "type": "string" }
                    },
                    "required": ["text"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "play_emotion",
                "description": "Play one visible emotion on the TRUEOS screen spirit.",
                "strict": true,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "idea": {
                            "type": "string",
                            "enum": ["anger", "disgust", "fear", "joy", "sadness", "surprise"]
                        }
                    },
                    "required": ["idea"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "move",
                "description": "Move the TRUEOS screen spirit to an absolute normalized point.",
                "strict": true,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "x": { "type": "number", "minimum": 0, "maximum": 1 },
                        "y": { "type": "number", "minimum": 0, "maximum": 1 }
                    },
                    "required": ["x", "y"],
                    "additionalProperties": false
                }
            }
        }
    ])
}

fn normal_request(
    config: &RuntimeConfig,
    conversation: &Conversation,
    prompt: &str,
) -> (Value, Vec<Value>) {
    let mut messages = conversation.messages.clone();
    let content = if prompt == AUTONOMOUS_PROMPT {
        prompt.to_string()
    } else {
        format!("Direct request from the user: {prompt}")
    };
    messages.push(json!({ "role": "user", "content": content }));
    let request = json!({
        "model": config.model,
        "messages": messages,
        "tools": tool_definitions(),
        "tool_choice": "required",
        "parallel_tool_calls": false,
        "max_completion_tokens": NORMAL_MAX_COMPLETION_TOKENS,
        "reasoning_effort": config.reasoning_effort,
        "stream": false
    });
    let request_messages = request
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    (request, request_messages)
}

fn summary_request(config: &RuntimeConfig, conversation: &Conversation) -> Value {
    let mut messages = conversation.messages.clone();
    messages.push(json!({ "role": "user", "content": SUMMARY_PROMPT }));
    json!({
        "model": config.model,
        "messages": messages,
        "max_completion_tokens": SUMMARY_MAX_COMPLETION_TOKENS,
        "reasoning_effort": config.reasoning_effort,
        "stream": false
    })
}

fn write_config_template() -> Result<(), String> {
    let template = json!({
        "api_key": API_KEY_PLACEHOLDER,
        "endpoint": DEFAULT_ENDPOINT,
        "model": DEFAULT_MODEL,
        "reasoning_effort": "low",
        "loop_interval_ms": DEFAULT_LOOP_INTERVAL_MS,
        "note": "Create a key in the Cerebras Cloud console. The Dobby Blueprint never prints this value.",
        "free_trial_note": "For autonomous RPM only, 15000ms averages below the current 5 RPM Free Trial after summaries. User prompts and token quotas still count separately."
    });
    let bytes = serde_json::to_vec_pretty(&template)
        .map_err(|_| "could not serialize config template".to_string())?;
    async_fs::block_on(async_fs::write_file(
        CONFIG_PATH.as_bytes(),
        bytes.as_slice(),
    ))
    .map_err(|code| format!("could not write {CONFIG_PATH} code={code}"))
}

fn load_runtime_config() -> (RuntimeConfig, Option<String>) {
    let (mut config, load_warning) =
        match async_fs::block_on(async_fs::read_file(CONFIG_PATH.as_bytes())) {
            Ok(bytes) => match serde_json::from_slice::<RuntimeConfig>(bytes.as_slice()) {
                Ok(config) => (config.merge_defaults(), None),
                Err(_) => {
                    return (
                        RuntimeConfig::default(),
                        Some(format!("invalid JSON in {CONFIG_PATH}")),
                    );
                }
            },
            Err(async_fs::ERR_NOT_FOUND) => {
                let warning = write_config_template().err().or_else(|| {
                    Some(format!(
                        "API key missing; edit {CONFIG_PATH} and run `dobby reload`"
                    ))
                });
                (RuntimeConfig::default(), warning)
            }
            Err(code) => {
                return (
                    RuntimeConfig::default(),
                    Some(format!("could not read {CONFIG_PATH} code={code}")),
                );
            }
        };

    if let Ok(key) = env::var("CEREBRAS_API_KEY")
        && !key.trim().is_empty()
    {
        config.api_key = key.trim().to_string();
    }
    if let Err(reason) = config.validate() {
        return (config, Some(reason));
    }
    if !config.api_key_configured() {
        return (
            config,
            load_warning.or_else(|| {
                Some(format!(
                    "API key missing; edit {CONFIG_PATH} or set CEREBRAS_API_KEY"
                ))
            }),
        );
    }
    (config, None)
}

fn reload_config(state: &mut AppState) {
    let (config, error) = load_runtime_config();
    state.config = config;
    state.config_error = error;
    match state.config_error.as_deref() {
        Some(reason) => vshell::linef(format_args!("dobby: config not ready reason={reason}")),
        None => vshell::linef(format_args!(
            "dobby: config ready provider=cerebras model={} reasoning={} interval_ms={} key=redacted",
            state.config.model, state.config.reasoning_effort, state.config.loop_interval_ms,
        )),
    };
    if state.running {
        state.schedule_next_autonomous();
    }
}

fn warn_if_free_trial_cadence(config: &RuntimeConfig) {
    if config.loop_interval_ms < FREE_TRIAL_AUTONOMOUS_RPM_INTERVAL_MS {
        vshell::line(
            "dobby: note: requested 5s exceeds Free Trial 5 RPM; 15000ms is autonomous-RPM-safe only",
        );
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ToolCall {
    id: String,
    name: String,
    arguments: String,
}

struct NormalCompletion {
    assistant_message: Value,
    tool_calls: Vec<ToolCall>,
}

fn response_message(response: &Value) -> Result<&Value, String> {
    response
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .ok_or_else(|| "response has no assistant message".to_string())
}

fn parse_normal_completion(bytes: &[u8]) -> Result<NormalCompletion, String> {
    let response: Value =
        serde_json::from_slice(bytes).map_err(|_| "response is not valid JSON".to_string())?;
    let message = response_message(&response)?;
    let calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if calls.len() != 1 {
        return Err("ordinary response must contain exactly one tool call".to_string());
    }

    let mut tool_calls = Vec::with_capacity(calls.len());
    let mut sanitized_calls = Vec::with_capacity(calls.len());
    for call in calls {
        let id = call
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty() && id.len() <= 256)
            .ok_or_else(|| "tool call has no bounded id".to_string())?;
        let function = call
            .get("function")
            .ok_or_else(|| "tool call has no function".to_string())?;
        let name = function
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty() && name.len() <= 64)
            .ok_or_else(|| "tool call has no bounded name".to_string())?;
        let arguments = match function.get("arguments") {
            Some(Value::String(arguments)) => arguments.clone(),
            Some(arguments) => serde_json::to_string(arguments)
                .map_err(|_| "tool arguments could not be normalized".to_string())?,
            None => return Err("tool call has no arguments".to_string()),
        };
        if arguments.len() > MAX_TOOL_ARGUMENT_BYTES {
            return Err("tool arguments are too large".to_string());
        }
        tool_calls.push(ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: arguments.clone(),
        });
        sanitized_calls.push(json!({
            "id": id,
            "type": "function",
            "function": {
                "name": name,
                "arguments": arguments,
            }
        }));
    }

    let assistant_message = json!({
        "role": "assistant",
        "content": Value::Null,
        "tool_calls": sanitized_calls,
    });
    Ok(NormalCompletion {
        assistant_message,
        tool_calls,
    })
}

fn parse_summary(bytes: &[u8]) -> Result<String, String> {
    let response: Value = serde_json::from_slice(bytes)
        .map_err(|_| "summary response is not valid JSON".to_string())?;
    let summary = response_message(&response)?
        .get("content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
        .ok_or_else(|| "summary response is empty".to_string())?;
    Ok(truncate_utf8(summary, MAX_CARRY_BYTES))
}

fn compact_visible_text(text: &str) -> String {
    let mut compact = String::new();
    let mut separator_pending = false;
    for character in text.chars() {
        if character.is_whitespace() || character.is_control() {
            separator_pending = !compact.is_empty();
            continue;
        }

        let separator = usize::from(separator_pending);
        if compact
            .len()
            .saturating_add(separator)
            .saturating_add(character.len_utf8())
            > MAX_VISIBLE_TEXT_BYTES
        {
            break;
        }
        if separator_pending {
            compact.push(' ');
        }
        compact.push(character);
        separator_pending = false;
    }
    compact
}

fn truncate_utf8(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    while end != 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

fn tool_arguments(call: &ToolCall) -> Result<Value, String> {
    serde_json::from_str(call.arguments.as_str())
        .map_err(|_| "arguments are not valid JSON".to_string())
}

fn validate_tool_call(call: &ToolCall) -> Result<(), String> {
    let arguments = tool_arguments(call)?;
    let object = arguments
        .as_object()
        .ok_or_else(|| "tool arguments must be an object".to_string())?;
    match call.name.as_str() {
        "text" => {
            if object.len() != 1 {
                return Err("text expects exactly one field".to_string());
            }
            let text = object
                .get("text")
                .and_then(Value::as_str)
                .map(compact_visible_text)
                .filter(|text| !text.is_empty())
                .ok_or_else(|| "text is missing or empty".to_string())?;
            if text.len() > MAX_VISIBLE_TEXT_BYTES {
                return Err("text is too long".to_string());
            }
            Ok(())
        }
        "play_emotion" => {
            if object.len() != 1 {
                return Err("play_emotion expects exactly one field".to_string());
            }
            let idea = object
                .get("idea")
                .and_then(Value::as_str)
                .ok_or_else(|| "emotion is missing".to_string())?;
            if matches!(
                idea,
                "anger" | "disgust" | "fear" | "joy" | "sadness" | "surprise"
            ) {
                Ok(())
            } else {
                Err("unknown emotion".to_string())
            }
        }
        "move" => {
            if object.len() != 2 {
                return Err("move expects exactly two fields".to_string());
            }
            let x = object
                .get("x")
                .and_then(Value::as_f64)
                .ok_or_else(|| "x is missing".to_string())?;
            let y = object
                .get("y")
                .and_then(Value::as_f64)
                .ok_or_else(|| "y is missing".to_string())?;
            if x.is_finite()
                && y.is_finite()
                && (0.0..=1.0).contains(&x)
                && (0.0..=1.0).contains(&y)
            {
                Ok(())
            } else {
                Err("coordinates must be in 0..=1".to_string())
            }
        }
        _ => Err("unknown tool".to_string()),
    }
}

fn execute_tool(state: &mut AppState, call: &ToolCall) -> String {
    let arguments = match tool_arguments(call) {
        Ok(arguments) => arguments,
        Err(reason) => return format!("rejected: {reason}"),
    };
    match call.name.as_str() {
        "text" => {
            let Some(text) = arguments
                .get("text")
                .and_then(Value::as_str)
                .map(compact_visible_text)
                .filter(|text| !text.is_empty())
            else {
                return "rejected: text is missing".to_string();
            };
            let turn = state.next_presentation_turn();
            match spirit::present_text_silent(turn, text.as_str()) {
                Ok(()) => {
                    vshell::linef(format_args!("dobby: {text}"));
                    "ok: displayed silently".to_string()
                }
                Err(error) => {
                    vshell::linef(format_args!(
                        "dobby: silent text presentation failed error={error:?}"
                    ));
                    "failed: Spirit text ingress unavailable".to_string()
                }
            }
        }
        "play_emotion" => {
            let Some(idea) = arguments.get("idea").and_then(Value::as_str) else {
                return "rejected: emotion is missing".to_string();
            };
            if !matches!(
                idea,
                "anger" | "disgust" | "fear" | "joy" | "sadness" | "surprise"
            ) {
                return "rejected: unknown emotion".to_string();
            }
            match spirit::play_emotion(idea) {
                Ok(()) => {
                    vshell::linef(format_args!("dobby: emotion={idea}"));
                    "ok: emotion queued".to_string()
                }
                Err(_) => "failed: Spirit emotion ingress unavailable".to_string(),
            }
        }
        "move" => {
            let Some(x) = arguments.get("x").and_then(Value::as_f64) else {
                return "rejected: x is missing".to_string();
            };
            let Some(y) = arguments.get("y").and_then(Value::as_f64) else {
                return "rejected: y is missing".to_string();
            };
            if !x.is_finite()
                || !y.is_finite()
                || !(0.0..=1.0).contains(&x)
                || !(0.0..=1.0).contains(&y)
            {
                return "rejected: coordinates must be in 0..=1".to_string();
            }
            match spirit::move_to(x as f32, y as f32) {
                Ok(()) => {
                    vshell::linef(format_args!("dobby: move x={x:.3} y={y:.3}"));
                    "ok: movement queued".to_string()
                }
                Err(_) => "failed: Spirit movement ingress unavailable".to_string(),
            }
        }
        _ => "rejected: unknown tool".to_string(),
    }
}

fn execute_completion_tools(state: &mut AppState, completion: &NormalCompletion) -> Vec<Value> {
    let mut messages = Vec::with_capacity(completion.tool_calls.len());
    for call in &completion.tool_calls {
        let result = execute_tool(state, call);
        messages.push(json!({
            "role": "tool",
            "tool_call_id": call.id,
            "content": result,
        }));
    }
    messages
}

fn queue_summary(state: &mut AppState) {
    state.pending.push_front(QueuedIdea {
        kind: IdeaKind::Summary,
        session: state.session,
        prompt: SUMMARY_PROMPT.to_string(),
    });
}

fn fail_before_request(state: &mut AppState, idea: QueuedIdea, reason: String) {
    if !state.idea_is_current(&idea) {
        return;
    }
    if matches!(idea.kind, IdeaKind::Summary) {
        state.conversation.rollover(None);
        vshell::linef(format_args!(
            "dobby: summary rollover failed; new chat has no carry reason={reason}"
        ));
    } else {
        vshell::linef(format_args!(
            "dobby: {} request not started reason={reason}",
            idea.kind.name(),
        ));
    }
    state.last_error = Some(reason);
    if matches!(idea.kind, IdeaKind::Autonomous { .. }) && state.running {
        state.schedule_next_autonomous();
    }
}

fn start_next_request(state: &mut AppState) {
    if state.in_flight.is_some() {
        return;
    }

    let idea = loop {
        let Some(idea) = state.pending.pop_front() else {
            return;
        };
        if state.idea_is_current(&idea) {
            break idea;
        }
    };

    if state.config_error.is_some() || !state.config.api_key_configured() {
        reload_config(state);
    }
    if let Some(reason) = state.config_error.clone() {
        fail_before_request(state, idea, reason);
        return;
    }

    let (request, request_messages) = match idea.kind {
        IdeaKind::Summary => (summary_request(&state.config, &state.conversation), None),
        IdeaKind::Autonomous { .. } | IdeaKind::User => {
            let (request, messages) =
                normal_request(&state.config, &state.conversation, idea.prompt.as_str());
            (request, Some(messages))
        }
    };
    let body = match serde_json::to_vec(&request) {
        Ok(body) => body,
        Err(_) => {
            fail_before_request(state, idea, "request JSON serialization failed".to_string());
            return;
        }
    };
    let operation = match netfs::fetch_post_json_bytes_with_timeout(
        state.config.endpoint.as_bytes(),
        body.as_slice(),
        Some(state.config.api_key.as_bytes()),
        REQUEST_TIMEOUT_MS,
    ) {
        Ok(operation) => operation,
        Err(code) => {
            fail_before_request(
                state,
                idea,
                format!("HTTPS operation could not start code={code}"),
            );
            return;
        }
    };

    state.remote_requests = state.remote_requests.saturating_add(1);
    let started_ms = clock::monotonic_millis();
    if matches!(idea.kind, IdeaKind::Autonomous { .. }) && state.running {
        state.next_autonomous_ms = started_ms.saturating_add(state.config.loop_interval_ms);
    }
    vshell::linef(format_args!(
        "dobby: remote request started kind={} request={} provider=cerebras model={}",
        idea.kind.name(),
        state.remote_requests,
        state.config.model,
    ));
    state.in_flight = Some(InFlight {
        operation,
        idea,
        request_messages,
        started_ms,
    });
}

fn finish_request_error(state: &mut AppState, in_flight: InFlight, reason: String) {
    if !state.idea_is_current(&in_flight.idea) {
        vshell::linef(format_args!(
            "dobby: discarded stale {} request after {}ms",
            in_flight.idea.kind.name(),
            clock::monotonic_millis().saturating_sub(in_flight.started_ms),
        ));
        return;
    }

    if matches!(in_flight.idea.kind, IdeaKind::Summary) {
        state.conversation.rollover(None);
        vshell::linef(format_args!(
            "dobby: summary request failed; rolled to a new chat without carry reason={reason}"
        ));
    } else {
        vshell::linef(format_args!(
            "dobby: {} request failed reason={reason}",
            in_flight.idea.kind.name(),
        ));
    }
    state.last_error = Some(reason);
}

fn finish_summary(state: &mut AppState, in_flight: InFlight, bytes: &[u8]) {
    if !state.idea_is_current(&in_flight.idea) {
        vshell::line("dobby: discarded stale summary response");
        return;
    }
    match parse_summary(bytes) {
        Ok(summary) => {
            state.conversation.rollover(Some(summary));
            state.last_error = None;
            vshell::line("dobby: ten-turn chat rolled over; compact carry stored (summary hidden)");
        }
        Err(reason) => {
            state.conversation.rollover(None);
            state.last_error = Some(reason.clone());
            vshell::linef(format_args!(
                "dobby: summary malformed; new chat has no carry reason={reason}"
            ));
        }
    }
}

fn finish_normal(state: &mut AppState, mut in_flight: InFlight, bytes: &[u8]) {
    if !state.idea_is_current(&in_flight.idea) {
        vshell::linef(format_args!(
            "dobby: discarded stale {} response",
            in_flight.idea.kind.name(),
        ));
        return;
    }
    let completion = match parse_normal_completion(bytes) {
        Ok(completion) => completion,
        Err(reason) => {
            finish_request_error(state, in_flight, reason);
            return;
        }
    };
    if let Err(reason) = validate_tool_call(&completion.tool_calls[0]) {
        finish_request_error(state, in_flight, format!("tool call rejected: {reason}"));
        return;
    }

    let tool_messages = execute_completion_tools(state, &completion);
    let Some(mut messages) = in_flight.request_messages.take() else {
        finish_request_error(
            state,
            in_flight,
            "ordinary request lost its message snapshot".to_string(),
        );
        return;
    };
    messages.push(completion.assistant_message);
    messages.extend(tool_messages);
    state.conversation.messages = messages;
    state.conversation.normal_turns = state.conversation.normal_turns.saturating_add(1);
    state.last_error = None;
    vshell::linef(format_args!(
        "dobby: turn committed kind={} chat_turn={}/{}",
        in_flight.idea.kind.name(),
        state.conversation.normal_turns,
        NORMAL_TURNS_PER_CHAT,
    ));

    if state.conversation.normal_turns >= NORMAL_TURNS_PER_CHAT {
        state.conversation.summary_due = true;
        queue_summary(state);
    }
}

fn finish_request_success(state: &mut AppState, in_flight: InFlight, bytes: &[u8]) {
    if matches!(in_flight.idea.kind, IdeaKind::Summary) {
        finish_summary(state, in_flight, bytes);
    } else {
        finish_normal(state, in_flight, bytes);
    }
}

fn poll_request(state: &mut AppState) {
    let Some(operation) = state.in_flight.as_ref().map(|request| request.operation) else {
        return;
    };
    match netfs::fetch_bytes_result_len(operation) {
        Err(FETCH_PENDING) => {}
        Err(code) => {
            let _ = netfs::fetch_bytes_discard(operation);
            let Some(in_flight) = state.in_flight.take() else {
                return;
            };
            finish_request_error(state, in_flight, format!("HTTPS failed code={code}"));
        }
        Ok(_) => {
            let result = netfs::fetch_bytes_read(operation)
                .map_err(|code| format!("HTTPS response read failed code={code}"));
            let _ = netfs::fetch_bytes_discard(operation);
            let Some(in_flight) = state.in_flight.take() else {
                return;
            };
            match result {
                Ok(bytes) => finish_request_success(state, in_flight, bytes.as_slice()),
                Err(reason) => finish_request_error(state, in_flight, reason),
            }
        }
    }
}

fn discard_stale_in_flight(state: &mut AppState) {
    let Some(request) = state.take_stale_in_flight() else {
        return;
    };
    let _ = netfs::fetch_bytes_discard(request.operation);
    vshell::linef(format_args!(
        "dobby: cancelled stale {} request",
        request.idea.kind.name(),
    ));
}

fn schedule_autonomous_if_due(state: &mut AppState) {
    if !state.running
        || state.in_flight.is_some()
        || !state.pending.is_empty()
        || state.conversation.summary_due
        || clock::monotonic_millis() < state.next_autonomous_ms
    {
        return;
    }
    state.pending.push_back(QueuedIdea {
        kind: IdeaKind::Autonomous {
            generation: state.generation,
        },
        session: state.session,
        prompt: AUTONOMOUS_PROMPT.to_string(),
    });
}

fn print_help() {
    vshell::line("dobby: commands `start|stop|reset|reload|status|help|quit` or any user request");
    vshell::line(
        "dobby: optional `dobby` prefix is accepted; VM controls use `vmx`, for example `vmx stop`",
    );
}

fn print_status(state: &AppState) {
    let activity = state
        .in_flight
        .as_ref()
        .map(|request| request.idea.kind.name())
        .unwrap_or("idle");
    let key = if state.config.api_key_configured() && state.config_error.is_none() {
        "configured"
    } else {
        "missing"
    };
    vshell::linef(format_args!(
        "dobby: mode={} activity={} queued={} chat_turns={}/{} carry={} key={} model={} reasoning={} interval_ms={} requests={}",
        if state.running { "running" } else { "stopped" },
        activity,
        state.pending.len(),
        state.conversation.normal_turns,
        NORMAL_TURNS_PER_CHAT,
        if state.conversation.carried_summary.is_some() {
            "yes"
        } else {
            "no"
        },
        key,
        state.config.model,
        state.config.reasoning_effort,
        state.config.loop_interval_ms,
        state.remote_requests,
    ));
    if let Some(reason) = state.config_error.as_deref() {
        vshell::linef(format_args!("dobby: config_error={reason}"));
    }
    if let Some(reason) = state.last_error.as_deref() {
        vshell::linef(format_args!("dobby: last_error={reason}"));
    }
}

fn command_body(line: &str) -> (&str, bool) {
    let line = line.trim();
    let mut split = line.splitn(2, char::is_whitespace);
    let first = split.next().unwrap_or_default();
    if first.eq_ignore_ascii_case("dobby") {
        (split.next().unwrap_or_default().trim(), true)
    } else {
        (line, false)
    }
}

fn handle_command(state: &mut AppState, line: &str) -> bool {
    let (input, prefixed) = command_body(line);
    if (prefixed && input.is_empty()) || input.eq_ignore_ascii_case("status") {
        print_status(state);
        return true;
    }
    if input.eq_ignore_ascii_case("help") || matches!(input, "-h" | "--help") {
        print_help();
        return true;
    }
    if input.eq_ignore_ascii_case("start") {
        reload_config(state);
        warn_if_free_trial_cadence(&state.config);
        state.start();
        print_status(state);
        return true;
    }
    if input.eq_ignore_ascii_case("stop") {
        state.stop();
        discard_stale_in_flight(state);
        print_status(state);
        return true;
    }
    if input.eq_ignore_ascii_case("reset") {
        state.reset();
        discard_stale_in_flight(state);
        vshell::line("dobby: conversation and propagated summary cleared");
        print_status(state);
        return true;
    }
    if input.eq_ignore_ascii_case("reload") {
        reload_config(state);
        return true;
    }
    if input.eq_ignore_ascii_case("quit") || input.eq_ignore_ascii_case("exit") {
        if let Some(request) = state.in_flight.take() {
            let _ = netfs::fetch_bytes_discard(request.operation);
        }
        vshell::line("dobby: leaving Blueprint; no local voice task was started");
        return false;
    }
    if input.is_empty() {
        print_help();
        return true;
    }

    match state.queue_user_prompt(input) {
        Ok(()) => {
            discard_stale_in_flight(state);
            vshell::line(
                "dobby: autonomous loop stopped atomically; user request queued in app serializer",
            );
        }
        Err(reason) => {
            vshell::linef(format_args!("dobby: request rejected reason={reason}"));
        }
    };
    true
}

fn initial_command_from_args() -> Option<String> {
    let mut args = env::args();
    let _archive = args.next();
    let mut command = String::new();
    for argument in args.filter(|argument| argument != "--vmx-minishell") {
        if !command.is_empty() {
            command.push(' ');
        }
        command.push_str(argument.as_str());
    }
    (!command.is_empty()).then_some(command)
}

fn main() {
    let (config, config_error) = load_runtime_config();
    let mut state = AppState::new(config, config_error);
    let mut input = ShellInput::new();

    vshell::line(
        "dobby-bp: online ownership=blueprint-policy+queue+conversation kernel=generic-https+silent-spirit",
    );
    vshell::line(
        "dobby-bp: Cerebras direct REST; Python/Node absent; local Lumen and local TTS absent",
    );
    print_help();
    print_status(&state);

    if let Some(command) = initial_command_from_args()
        && !handle_command(&mut state, command.as_str())
    {
        return;
    }

    loop {
        while let Some(line) = input.poll() {
            match line {
                Ok(line) if !handle_command(&mut state, line) => return,
                Ok(_) => {}
                Err(reason) => {
                    vshell::linef(format_args!("dobby: input rejected reason={reason}"));
                }
            }
        }

        poll_request(&mut state);
        schedule_autonomous_if_due(&mut state);
        start_next_request(&mut state);
        platform::poll_once();
        platform::sleep_ms(EVENT_LOOP_SLEEP_MS);
    }
}

// Blueprint binaries normally resolve these imports from TRUEOS. Host unit
// tests never execute the VM lifecycle paths, so tiny test-only definitions
// let the ordinary Rust test harness link the pure state/protocol tests below.
#[cfg(test)]
mod host_test_abi {
    use core::sync::atomic::{AtomicU64, Ordering};

    static NOW_NANOS: AtomicU64 = AtomicU64::new(1_000_000_000);

    #[unsafe(no_mangle)]
    extern "C" fn trueos_time_monotonic_nanos() -> u64 {
        NOW_NANOS.fetch_add(1_000_000, Ordering::Relaxed)
    }

    #[unsafe(no_mangle)]
    extern "C" fn trueos_cabi_blueprint_shutdown(_data: *const u8, _len: usize) -> i32 {
        0
    }

    #[unsafe(no_mangle)]
    extern "C" fn trueos_cabi_write(_stream: u32, _bytes: *const u8, _len: usize) {}

    #[unsafe(no_mangle)]
    extern "C" fn trueos_cabi_shell_attached_write(_bytes: *const u8, len: usize) -> usize {
        len
    }

    #[unsafe(no_mangle)]
    extern "C" fn trueos_cabi_spirit_emotion_play(_idea: *const u8, _len: usize) -> i32 {
        0
    }

    #[unsafe(no_mangle)]
    extern "C" fn trueos_cabi_spirit_text_present_silent(
        _turn: u64,
        _text: *const u8,
        _len: usize,
    ) -> i32 {
        0
    }

    #[unsafe(no_mangle)]
    extern "C" fn trueos_cabi_spirit_move(_x: f32, _y: f32) -> i32 {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> RuntimeConfig {
        RuntimeConfig {
            api_key: "secret".to_string(),
            endpoint: DEFAULT_ENDPOINT.to_string(),
            model: DEFAULT_MODEL.to_string(),
            reasoning_effort: "low".to_string(),
            loop_interval_ms: DEFAULT_LOOP_INTERVAL_MS,
        }
    }

    #[test]
    fn ordinary_request_has_three_tools_and_low_remote_reasoning() {
        let conversation = Conversation::new();
        let (request, messages) = normal_request(&config(), &conversation, AUTONOMOUS_PROMPT);

        assert_eq!(request["tools"].as_array().unwrap().len(), 3);
        assert_eq!(request["tool_choice"], "required");
        assert_eq!(request["parallel_tool_calls"], false);
        assert_eq!(request["reasoning_effort"], "low");
        assert_eq!(
            request["max_completion_tokens"],
            NORMAL_MAX_COMPLETION_TOKENS
        );
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn summary_is_the_single_extra_non_tool_request_capped_at_500() {
        let conversation = Conversation::new();
        let request = summary_request(&config(), &conversation);

        assert!(request.get("tools").is_none());
        assert!(request.get("tool_choice").is_none());
        assert_eq!(
            request["max_completion_tokens"],
            SUMMARY_MAX_COMPLETION_TOKENS
        );
    }

    #[test]
    fn parses_openai_compatible_cerebras_tool_call() {
        let response = br#"{
            "choices":[{"message":{"role":"assistant","content":null,"tool_calls":[
                {"id":"call_1","type":"function","function":{"name":"move","arguments":"{\"x\":0.25,\"y\":0.75}"}}
            ]}}]
        }"#;
        let completion = parse_normal_completion(response).unwrap();

        assert_eq!(completion.tool_calls.len(), 1);
        assert_eq!(completion.tool_calls[0].name, "move");
        assert_eq!(
            tool_arguments(&completion.tool_calls[0]).unwrap()["x"],
            0.25
        );
    }

    #[test]
    fn rejects_zero_or_multiple_tools_on_an_ordinary_turn() {
        let no_tool = br#"{"choices":[{"message":{"role":"assistant","content":"hello"}}]}"#;
        let two_tools = br#"{
            "choices":[{"message":{"role":"assistant","content":null,"tool_calls":[
                {"id":"call_1","type":"function","function":{"name":"play_emotion","arguments":"{\"idea\":\"joy\"}"}},
                {"id":"call_2","type":"function","function":{"name":"move","arguments":"{\"x\":0.25,\"y\":0.75}"}}
            ]}}]
        }"#;

        assert!(parse_normal_completion(no_tool).is_err());
        assert!(parse_normal_completion(two_tools).is_err());
    }

    #[test]
    fn rejects_unknown_or_malformed_actions_before_execution() {
        let missing_text = ToolCall {
            id: "call_1".to_string(),
            name: "text".to_string(),
            arguments: "{}".to_string(),
        };
        let unknown = ToolCall {
            id: "call_2".to_string(),
            name: "teleport".to_string(),
            arguments: "{}".to_string(),
        };

        assert!(validate_tool_call(&missing_text).is_err());
        assert!(validate_tool_call(&unknown).is_err());
    }

    #[test]
    fn configurable_reasoning_effort_reaches_normal_and_summary_requests() {
        let mut config = config();
        config.reasoning_effort = "none".to_string();
        let conversation = Conversation::new();
        let (normal, _) = normal_request(&config, &conversation, AUTONOMOUS_PROMPT);
        let summary = summary_request(&config, &conversation);

        assert_eq!(normal["reasoning_effort"], "none");
        assert_eq!(summary["reasoning_effort"], "none");
    }

    #[test]
    fn tenth_ordinary_turn_queues_one_hidden_summary_ahead_of_other_ideas() {
        let mut state = AppState::new(config(), None);
        state.conversation.normal_turns = NORMAL_TURNS_PER_CHAT - 1;
        state.pending.push_back(QueuedIdea {
            kind: IdeaKind::User,
            session: state.session,
            prompt: "later".to_string(),
        });
        let mut request_messages = initial_messages(None);
        request_messages.push(json!({"role":"user","content":"turn ten"}));
        let request = InFlight {
            operation: 3,
            idea: QueuedIdea {
                kind: IdeaKind::User,
                session: state.session,
                prompt: "turn ten".to_string(),
            },
            request_messages: Some(request_messages),
            started_ms: 0,
        };
        let response = br#"{
            "choices":[{"message":{"role":"assistant","content":null,"tool_calls":[
                {"id":"call_1","type":"function","function":{"name":"text","arguments":"{\"text\":\"hello\"}"}}
            ]}}]
        }"#;

        finish_normal(&mut state, request, response);

        assert_eq!(state.conversation.normal_turns, NORMAL_TURNS_PER_CHAT);
        assert!(state.conversation.summary_due);
        assert_eq!(state.pending.len(), 2);
        assert!(matches!(state.pending[0].kind, IdeaKind::Summary));
        assert!(matches!(state.pending[1].kind, IdeaKind::User));
    }

    #[test]
    fn serialized_queue_reserves_its_last_slot_for_rollover() {
        let mut state = AppState::new(config(), None);
        for index in 0..MAX_PENDING_IDEAS - 1 {
            state
                .queue_user_prompt(format!("idea {index}").as_str())
                .unwrap();
        }

        assert_eq!(state.pending.len(), MAX_PENDING_IDEAS - 1);
        assert_eq!(
            state.queue_user_prompt("one too many"),
            Err("serialized idea queue is full")
        );
        state.conversation.summary_due = true;
        queue_summary(&mut state);
        assert_eq!(state.pending.len(), MAX_PENDING_IDEAS);
        assert!(matches!(state.pending[0].kind, IdeaKind::Summary));
    }

    #[test]
    fn rollover_keeps_only_fixed_prefix_and_bounded_carry() {
        let mut conversation = Conversation::new();
        conversation
            .messages
            .push(json!({"role":"user","content":"old"}));
        conversation.normal_turns = NORMAL_TURNS_PER_CHAT;
        conversation.summary_due = true;
        conversation.rollover(Some("key point".to_string()));

        assert_eq!(conversation.normal_turns, 0);
        assert!(!conversation.summary_due);
        assert_eq!(conversation.messages.len(), 2);
        assert!(
            !conversation
                .messages
                .iter()
                .any(|message| message["content"] == "old")
        );
        assert_eq!(conversation.carried_summary.as_deref(), Some("key point"));
    }

    #[test]
    fn user_request_stops_and_invalidates_autonomous_generation_atomically() {
        let mut state = AppState::new(config(), None);
        state.start();
        let old_generation = state.generation;
        state.pending.push_back(QueuedIdea {
            kind: IdeaKind::Autonomous {
                generation: old_generation,
            },
            session: state.session,
            prompt: AUTONOMOUS_PROMPT.to_string(),
        });
        state.in_flight = Some(InFlight {
            operation: 7,
            idea: QueuedIdea {
                kind: IdeaKind::Autonomous {
                    generation: old_generation,
                },
                session: state.session,
                prompt: AUTONOMOUS_PROMPT.to_string(),
            },
            request_messages: None,
            started_ms: 0,
        });

        state.queue_user_prompt("please wave").unwrap();

        assert!(!state.running);
        assert_ne!(state.generation, old_generation);
        assert_eq!(state.pending.len(), 1);
        assert!(matches!(state.pending[0].kind, IdeaKind::User));
        assert_eq!(state.take_stale_in_flight().unwrap().operation, 7);
        assert!(state.in_flight.is_none());
    }

    #[test]
    fn reset_drops_turns_queue_and_propagated_summary_but_preserves_running_mode() {
        let mut state = AppState::new(config(), None);
        state.start();
        state.conversation.normal_turns = 7;
        state.conversation.carried_summary = Some("memory".to_string());
        state.queue_user_prompt("queued").unwrap();
        state.start();

        state.reset();

        assert!(state.running);
        assert_eq!(state.conversation.normal_turns, 0);
        assert!(state.conversation.carried_summary.is_none());
        assert!(state.pending.is_empty());
    }

    #[test]
    fn visible_text_is_short_enough_for_five_second_typing_cadence() {
        let text = "one\u{1b}[31m two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty";
        let compact = compact_visible_text(text);

        assert!(compact.len() <= MAX_VISIBLE_TEXT_BYTES);
        assert!(core::str::from_utf8(compact.as_bytes()).is_ok());
        assert!(!compact.chars().any(char::is_control));
    }
}
