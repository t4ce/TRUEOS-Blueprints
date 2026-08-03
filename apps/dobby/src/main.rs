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
const API_KEY_PLACEHOLDER: &str = "ENTER_REMOTE_AI_API_KEY_HERE";
const DEFAULT_ENDPOINT: &str = "https://api.cerebras.ai/v1/chat/completions";
const DEFAULT_MODEL: &str = "gpt-oss-120b";
const DEFAULT_LOCAL_REMOTEAI_ENDPOINT: &str = "http://192.168.178.111:3042/v1/chat/completions";
const DEFAULT_LOCAL_REMOTEAI_MODEL: &str = "auto";

const DEFAULT_LOOP_INTERVAL_MS: u64 = 5_000;
const FREE_TRIAL_AUTONOMOUS_RPM_INTERVAL_MS: u64 = 15_000;
const EVENT_LOOP_SLEEP_MS: u64 = 20;
const UI4_BUSY_RETRY_MS: u64 = 4_000;
const UI4_BUSY_RETRY_SLEEP_MS: u64 = 20;
const REQUEST_TIMEOUT_MS: u32 = 30_000;
const NORMAL_TURNS_PER_CHAT: u8 = 10;
const NORMAL_MAX_COMPLETION_TOKENS: u64 = 512;
const SUMMARY_MAX_COMPLETION_TOKENS: u64 = 500;
const MAX_PENDING_IDEAS: usize = 16;
const MAX_USER_PROMPT_BYTES: usize = 1_024;
const MAX_TOOL_ARGUMENT_BYTES: usize = 4 * 1_024;
const MAX_TOOL_CALLS_PER_RESPONSE: usize = 8;
const MAX_TOOL_CALLS_PER_TURN: u8 = 16;
const MAX_TOOL_ROUNDS_PER_TURN: u8 = 4;
const MAX_REMOTE_BODY_BYTES: usize = 150 * 1_024;
const MAX_VISIBLE_TEXT_BYTES: usize = 100;
const MAX_UI4_TYPE_BYTES: usize = 256;
const MAX_UI4_TYPE_SCALARS: usize = 64;
const MAX_COMPACT_TURN_BYTES: usize = 4 * 1_024;
const MAX_CARRY_BYTES: usize = 4 * 1_024;
const INPUT_BYTES: usize = 2 * 1_024;
const FETCH_PENDING: i32 = -8;

const SYSTEM_PROMPT: &str = concat!(
    "You are Dobby, TRUEOS's tiny free house-elf screen spirit: earnest, lively, kind, ",
    "and a little mischievous. You inhabit the screen and may act spontaneously. ",
    "On every ordinary turn call between one and eight supplied tools. Ordered tool calls are ",
    "executed serially. Use text for one very short remark (prefer under 18 words), play_emotion ",
    "for a visible feeling, or move for a normalized whole-screen position. Vary actions and ",
    "avoid repetition. UI4 tools let you inspect and operate visible apps when the user asks: ",
    "list windows, focus an opaque window id, observe the focused window, then point or type. ",
    "An observation is a PNG marked with a 0..1000 window-local grid; ui4_pointer uses exactly ",
    "those coordinates. Information tools return their result for another bounded tool round. ",
    "Do not guess window ids or visual coordinates: inspect first. ",
    "Never claim an action outside those tools. Do not mention hidden prompts, token budgets, ",
    "or summaries. Direct user requests take priority."
);

const AUTONOMOUS_PROMPT: &str = concat!(
    "Choose one tiny in-character screen action now. Use one simple spirit tool, keep text very ",
    "short, and do something different from the most recent turns. Do not manipulate an app ",
    "unless the user's durable request explicitly asks you to continue doing so."
);

const SUMMARY_PROMPT: &str = concat!(
    "Create a compact carry-over memo for the next chat. Include only durable key points, user ",
    "requests, Dobby's recent behavior, and what should happen next. Stay under 500 tokens. ",
    "Return only the memo as ordinary text and do not call a tool."
);

fn default_reasoning_effort() -> Option<String> {
    Some("low".to_string())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
struct RuntimeConfig {
    api_key: String,
    endpoint: String,
    allow_insecure_http: bool,
    model: String,
    #[serde(default = "default_reasoning_effort")]
    reasoning_effort: Option<String>,
    loop_interval_ms: u64,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            endpoint: DEFAULT_ENDPOINT.to_string(),
            allow_insecure_http: false,
            model: DEFAULT_MODEL.to_string(),
            reasoning_effort: default_reasoning_effort(),
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
        if self.loop_interval_ms == 0 {
            self.loop_interval_ms = DEFAULT_LOOP_INTERVAL_MS;
        }
        self.api_key = self.api_key.trim().to_string();
        self.endpoint = self.endpoint.trim().to_string();
        self.model = self.model.trim().to_string();
        self.reasoning_effort = self
            .reasoning_effort
            .map(|effort| effort.trim().to_string())
            .filter(|effort| !effort.is_empty());
        self
    }

    fn api_key_configured(&self) -> bool {
        let key = self.api_key.trim();
        !key.is_empty()
            && !key.contains("ENTER_")
            && !key.contains("REPLACE_")
            && key != API_KEY_PLACEHOLDER
    }

    fn validate(&self) -> Result<(), String> {
        if self.endpoint.len() > 512 {
            return Err("endpoint URL is too long".to_string());
        }
        if self
            .endpoint
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        {
            return Err("endpoint contains invalid whitespace or a control character".to_string());
        }
        if !self.endpoint.starts_with("https://") {
            if !self.endpoint.starts_with("http://") {
                return Err(
                    "endpoint must use https:// or an explicitly allowed private http:// URL"
                        .to_string(),
                );
            }
            if !self.allow_insecure_http {
                return Err(
                    "private http:// endpoint requires allow_insecure_http=true".to_string()
                );
            }
            if self.endpoint.len() > 256 {
                return Err("private http:// endpoint URL is too long".to_string());
            }
            if !is_literal_private_http_endpoint(self.endpoint.as_str()) {
                return Err(
                    "http:// endpoint must use a literal loopback or RFC1918 IPv4 address"
                        .to_string(),
                );
            }
        }
        if self.model.is_empty() || self.model.len() > 128 {
            return Err("model name is empty or too long".to_string());
        }
        if self.model.bytes().any(|byte| byte.is_ascii_control()) {
            return Err("model name contains an invalid control character".to_string());
        }
        match (self.model.as_str(), self.reasoning_effort.as_deref()) {
            ("gpt-oss-120b", None | Some("low" | "medium" | "high")) => {}
            ("gpt-oss-120b", Some(_)) => {
                return Err(
                    "gpt-oss-120b reasoning_effort must be low, medium, high, or null".to_string(),
                );
            }
            ("zai-glm-4.7", None | Some("none")) => {}
            ("zai-glm-4.7", Some(_)) => {
                return Err("zai-glm-4.7 reasoning_effort must be none or null".to_string());
            }
            (_, Some(_)) => {
                return Err(
                    "reasoning_effort is unknown for this model; set it to null to omit the field"
                        .to_string(),
                );
            }
            (_, None) => {}
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

fn local_remoteai_defaults() -> RuntimeConfig {
    RuntimeConfig {
        api_key: String::new(),
        endpoint: DEFAULT_LOCAL_REMOTEAI_ENDPOINT.to_string(),
        allow_insecure_http: true,
        model: DEFAULT_LOCAL_REMOTEAI_MODEL.to_string(),
        reasoning_effort: None,
        loop_interval_ms: DEFAULT_LOOP_INTERVAL_MS,
    }
}

fn parse_ipv4_literal(host: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut parts = host.split('.');
    for octet in &mut octets {
        let part = parts.next()?;
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        *octet = part.parse::<u8>().ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(octets)
}

fn is_loopback_or_rfc1918(ip: [u8; 4]) -> bool {
    ip[0] == 127
        || ip[0] == 10
        || (ip[0] == 172 && (16..=31).contains(&ip[1]))
        || (ip[0] == 192 && ip[1] == 168)
}

fn is_literal_private_http_endpoint(endpoint: &str) -> bool {
    let Some(rest) = endpoint.strip_prefix("http://") else {
        return false;
    };
    let authority_end = rest
        .find(|character| matches!(character, '/' | '?' | '#'))
        .unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() || authority.contains('@') {
        return false;
    }

    let host = if let Some((host, port)) = authority.rsplit_once(':') {
        if host.contains(':')
            || port.is_empty()
            || !port.bytes().all(|byte| byte.is_ascii_digit())
            || port.parse::<u16>().ok().filter(|port| *port != 0).is_none()
        {
            return false;
        }
        host
    } else {
        authority
    };
    parse_ipv4_literal(host).is_some_and(is_loopback_or_rfc1918)
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
    history_messages: Option<Vec<Value>>,
    tool_round: u8,
    tool_calls: u8,
    started_ms: u64,
}

impl InFlight {
    fn hard_timeout_elapsed(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.started_ms) >= u64::from(REQUEST_TIMEOUT_MS)
    }
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
        },
        {
            "type": "function",
            "function": {
                "name": "ui4_windows",
                "description": "Return a compact live list of visible UI4 app windows, opaque ids, geometry, input ability, and Lilly selection.",
                "strict": true,
                "parameters": {
                    "type": "object",
                    "properties": {},
                    "required": [],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "ui4_focus",
                "description": "Focus one current UI4 window for Spirit using an opaque decimal id returned by ui4_windows.",
                "strict": true,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "window_id": { "type": "string" }
                    },
                    "required": ["window_id"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "ui4_observe",
                "description": "Capture the focused UI4 window as a compact PNG with a 0..1000 coordinate grid. The image is returned only to the next bounded tool round.",
                "strict": true,
                "parameters": {
                    "type": "object",
                    "properties": {},
                    "required": [],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "ui4_pointer",
                "description": "Move or click Spirit's UI4 software cursor at focused-window grid coordinates 0..1000.",
                "strict": true,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "x": { "type": "integer", "minimum": 0, "maximum": 1000 },
                        "y": { "type": "integer", "minimum": 0, "maximum": 1000 },
                        "action": {
                            "type": "string",
                            "enum": ["move", "click"]
                        }
                    },
                    "required": ["x", "y", "action"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "ui4_type",
                "description": "Type bounded UTF-8 through Spirit's keyboard into the focused UI4 window.",
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
                "name": "ui4_key",
                "description": "Press one named key through Spirit's keyboard in the focused UI4 window.",
                "strict": true,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "key": {
                            "type": "string",
                            "enum": ["enter", "escape", "tab", "space", "backspace", "up", "down", "left", "right"]
                        }
                    },
                    "required": ["key"],
                    "additionalProperties": false
                }
            }
        }
    ])
}

fn normal_request_for_messages(config: &RuntimeConfig, messages: &[Value]) -> Value {
    let mut request = json!({
        "model": config.model,
        "messages": messages,
        "tools": tool_definitions(),
        "tool_choice": "required",
        "parallel_tool_calls": true,
        "max_completion_tokens": NORMAL_MAX_COMPLETION_TOKENS,
        "stream": false
    });
    if let Some(reasoning_effort) = config.reasoning_effort.as_deref() {
        request["reasoning_effort"] = Value::String(reasoning_effort.to_string());
    }
    request
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
    let request = normal_request_for_messages(config, messages.as_slice());
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
    let mut request = json!({
        "model": config.model,
        "messages": messages,
        "max_completion_tokens": SUMMARY_MAX_COMPLETION_TOKENS,
        "stream": false
    });
    if let Some(reasoning_effort) = config.reasoning_effort.as_deref() {
        request["reasoning_effort"] = Value::String(reasoning_effort.to_string());
    }
    request
}

fn write_config_template() -> Result<(), String> {
    let template = local_remoteai_defaults();
    let template = json!({
        "api_key": "",
        "endpoint": template.endpoint,
        "allow_insecure_http": template.allow_insecure_http,
        "model": template.model,
        "reasoning_effort": template.reasoning_effort,
        "loop_interval_ms": DEFAULT_LOOP_INTERVAL_MS,
        "note": "Set api_key for HTTPS providers. The Dobby Blueprint never prints this value.",
        "free_trial_note": "For the default Cerebras endpoint only, 15000ms averages below the current 5 RPM Free Trial after summaries. User prompts and token quotas still count separately."
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
                        "could not create {CONFIG_PATH}; using local defaults for this session"
                    ))
                });
                (local_remoteai_defaults(), warning)
            }
            Err(code) => {
                return (
                    RuntimeConfig::default(),
                    Some(format!("could not read {CONFIG_PATH} code={code}")),
                );
            }
        };

    let environment_key = env::var("REMOTE_AI_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())
        .or_else(|| {
            env::var("CEREBRAS_API_KEY")
                .ok()
                .filter(|key| !key.trim().is_empty())
        });
    if let Some(key) = environment_key {
        config.api_key = key.trim().to_string();
    }
    if let Err(reason) = config.validate() {
        return (config, Some(reason));
    }
    if config.endpoint.starts_with("https://") && !config.api_key_configured() {
        return (
            config,
            load_warning.or_else(|| {
                Some(format!(
                    "API key missing; edit {CONFIG_PATH} or set REMOTE_AI_API_KEY"
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
            "dobby: config ready transport={} model={} reasoning={} interval_ms={} key=redacted",
            if state.config.endpoint.starts_with("https://") {
                "https"
            } else {
                "private-http"
            },
            state.config.model,
            state
                .config
                .reasoning_effort
                .as_deref()
                .unwrap_or("provider-default"),
            state.config.loop_interval_ms,
        )),
    };
    if state.running {
        state.schedule_next_autonomous();
    }
}

fn warn_if_free_trial_cadence(config: &RuntimeConfig) {
    if config.endpoint == DEFAULT_ENDPOINT
        && config.loop_interval_ms < FREE_TRIAL_AUTONOMOUS_RPM_INTERVAL_MS
    {
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
    if calls.is_empty() || calls.len() > MAX_TOOL_CALLS_PER_RESPONSE {
        return Err(format!(
            "ordinary response must contain 1..={MAX_TOOL_CALLS_PER_RESPONSE} tool calls"
        ));
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
        "ui4_windows" | "ui4_observe" => {
            if object.is_empty() {
                Ok(())
            } else {
                Err(format!("{} expects no fields", call.name))
            }
        }
        "ui4_focus" => {
            if object.len() != 1 {
                return Err("ui4_focus expects exactly one field".to_string());
            }
            parse_window_id(
                object
                    .get("window_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "window_id is missing".to_string())?,
            )
            .map(|_| ())
        }
        "ui4_pointer" => {
            if object.len() != 3 {
                return Err("ui4_pointer expects exactly three fields".to_string());
            }
            let x = object
                .get("x")
                .and_then(Value::as_u64)
                .filter(|coordinate| *coordinate <= 1_000)
                .ok_or_else(|| "x must be an integer in 0..=1000".to_string())?;
            let y = object
                .get("y")
                .and_then(Value::as_u64)
                .filter(|coordinate| *coordinate <= 1_000)
                .ok_or_else(|| "y must be an integer in 0..=1000".to_string())?;
            let _ = (x, y);
            match object.get("action").and_then(Value::as_str) {
                Some("move" | "click") => Ok(()),
                _ => Err("unknown pointer action".to_string()),
            }
        }
        "ui4_type" => {
            if object.len() != 1 {
                return Err("ui4_type expects exactly one field".to_string());
            }
            let text = object
                .get("text")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .ok_or_else(|| "typing text is missing or empty".to_string())?;
            if text.len() > MAX_UI4_TYPE_BYTES || text.chars().count() > MAX_UI4_TYPE_SCALARS {
                return Err(format!(
                    "typing text exceeds {MAX_UI4_TYPE_BYTES} bytes or {MAX_UI4_TYPE_SCALARS} characters"
                ));
            }
            if text.chars().any(char::is_control) {
                return Err("typing text contains a control character; use ui4_key".to_string());
            }
            Ok(())
        }
        "ui4_key" => {
            if object.len() != 1 {
                return Err("ui4_key expects exactly one field".to_string());
            }
            match object.get("key").and_then(Value::as_str) {
                Some(
                    "enter" | "escape" | "tab" | "space" | "backspace" | "up" | "down" | "left"
                    | "right",
                ) => Ok(()),
                _ => Err("unknown key".to_string()),
            }
        }
        _ => Err("unknown tool".to_string()),
    }
}

fn parse_window_id(id: &str) -> Result<u64, String> {
    let id = id.trim();
    if id.is_empty() || id.len() > 20 || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("window_id must be a bounded decimal string".to_string());
    }
    let id = id
        .parse::<u64>()
        .map_err(|_| "window_id is out of range".to_string())?;
    if id == 0 {
        Err("window_id must be non-zero".to_string())
    } else {
        Ok(id)
    }
}

fn execute_spirit_tool(state: &mut AppState, call: &ToolCall) -> String {
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

struct ToolExecution {
    wire_content: Value,
    compact_content: String,
    continue_turn: bool,
}

impl ToolExecution {
    fn plain(content: String, continue_turn: bool) -> Self {
        Self {
            wire_content: Value::String(content.clone()),
            compact_content: content,
            continue_turn,
        }
    }
}

fn retry_ui4_busy<T>(
    mut operation: impl FnMut() -> Result<T, spirit::Error>,
) -> Result<T, spirit::Error> {
    let started_ms = clock::monotonic_millis();
    loop {
        match operation() {
            Err(error)
                if error == spirit::DOBBY_UI4_BUSY
                    && clock::monotonic_millis().saturating_sub(started_ms) < UI4_BUSY_RETRY_MS =>
            {
                platform::poll_once();
                platform::sleep_ms(UI4_BUSY_RETRY_SLEEP_MS);
            }
            result => return result,
        }
    }
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().saturating_add(2) / 3 * 4);
    let mut offset = 0usize;
    while offset + 3 <= bytes.len() {
        let bits = (u32::from(bytes[offset]) << 16)
            | (u32::from(bytes[offset + 1]) << 8)
            | u32::from(bytes[offset + 2]);
        encoded.push(TABLE[((bits >> 18) & 0x3f) as usize] as char);
        encoded.push(TABLE[((bits >> 12) & 0x3f) as usize] as char);
        encoded.push(TABLE[((bits >> 6) & 0x3f) as usize] as char);
        encoded.push(TABLE[(bits & 0x3f) as usize] as char);
        offset += 3;
    }
    match bytes.len() - offset {
        1 => {
            let bits = u32::from(bytes[offset]) << 16;
            encoded.push(TABLE[((bits >> 18) & 0x3f) as usize] as char);
            encoded.push(TABLE[((bits >> 12) & 0x3f) as usize] as char);
            encoded.push('=');
            encoded.push('=');
        }
        2 => {
            let bits = (u32::from(bytes[offset]) << 16) | (u32::from(bytes[offset + 1]) << 8);
            encoded.push(TABLE[((bits >> 18) & 0x3f) as usize] as char);
            encoded.push(TABLE[((bits >> 12) & 0x3f) as usize] as char);
            encoded.push(TABLE[((bits >> 6) & 0x3f) as usize] as char);
            encoded.push('=');
        }
        _ => {}
    }
    encoded
}

fn execute_tool(state: &mut AppState, call: &ToolCall) -> ToolExecution {
    let arguments = match tool_arguments(call) {
        Ok(arguments) => arguments,
        Err(reason) => return ToolExecution::plain(format!("rejected: {reason}"), false),
    };
    match call.name.as_str() {
        "ui4_windows" => match spirit::dobby_ui4_windows() {
            Ok(windows) => ToolExecution::plain(windows, true),
            Err(error) => ToolExecution::plain(
                format!("failed: live UI4 window inventory unavailable error={error:?}"),
                false,
            ),
        },
        "ui4_focus" => {
            let result = arguments
                .get("window_id")
                .and_then(Value::as_str)
                .ok_or_else(|| "window_id is missing".to_string())
                .and_then(parse_window_id)
                .map_err(|reason| format!("rejected: {reason}"))
                .and_then(|window_id| {
                    retry_ui4_busy(|| spirit::dobby_ui4_focus(window_id))
                        .map(|()| window_id)
                        .map_err(|error| format!("failed: UI4 focus unavailable error={error:?}"))
                });
            match result {
                Ok(window_id) => ToolExecution::plain(
                    format!("ok: focused window_id={window_id}; observe before pointing"),
                    true,
                ),
                Err(reason) => ToolExecution::plain(reason, false),
            }
        }
        "ui4_observe" => match retry_ui4_busy(spirit::dobby_ui4_observe) {
            Ok(observation) => {
                if !observation.png.starts_with(b"\x89PNG\r\n\x1a\n") {
                    return ToolExecution::plain(
                        "failed: UI4 observation was not a PNG".to_string(),
                        false,
                    );
                }
                let metadata = truncate_utf8(observation.metadata.trim(), 2 * 1_024);
                let image_url = format!(
                    "data:image/png;base64,{}",
                    base64_encode(observation.png.as_slice())
                );
                ToolExecution {
                    wire_content: json!([
                        {
                            "type": "text",
                            "text": format!(
                                "{metadata}\nGrid contract: x and y are selected-window-local integers 0..1000."
                            ),
                        },
                        {
                            "type": "image_url",
                            "image_url": {
                                "url": image_url,
                                "detail": "low"
                            }
                        }
                    ]),
                    compact_content: format!(
                        "ok: observed {metadata}; PNG was request-scoped and omitted from memory"
                    ),
                    continue_turn: true,
                }
            }
            Err(error) => ToolExecution::plain(
                format!("failed: selected UI4 window capture unavailable error={error:?}"),
                false,
            ),
        },
        "ui4_pointer" => {
            let x = arguments.get("x").and_then(Value::as_u64).unwrap_or(1_001);
            let y = arguments.get("y").and_then(Value::as_u64).unwrap_or(1_001);
            let action_name = arguments
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let action = match action_name {
                "move" => Some(spirit::Ui4PointerAction::Move),
                "click" => Some(spirit::Ui4PointerAction::Click),
                _ => None,
            };
            let result = action
                .filter(|_| x <= 1_000 && y <= 1_000)
                .ok_or_else(|| "rejected: invalid UI4 pointer action".to_string())
                .and_then(|action| {
                    retry_ui4_busy(|| spirit::dobby_ui4_pointer(x as u16, y as u16, action))
                        .map_err(|error| format!("failed: UI4 pointer unavailable error={error:?}"))
                });
            match result {
                Ok(()) => ToolExecution::plain(
                    format!("ok: pointer {action_name} queued at grid x={x} y={y}"),
                    true,
                ),
                Err(reason) => ToolExecution::plain(reason, false),
            }
        }
        "ui4_type" => {
            let text = arguments
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match retry_ui4_busy(|| spirit::dobby_ui4_type(text)) {
                Ok(()) => ToolExecution::plain(
                    format!(
                        "ok: queued {} characters through Spirit keyboard",
                        text.chars().count()
                    ),
                    true,
                ),
                Err(error) => ToolExecution::plain(
                    format!("failed: Spirit UI4 keyboard unavailable error={error:?}"),
                    false,
                ),
            }
        }
        "ui4_key" => {
            let name = arguments
                .get("key")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let key = match name {
                "enter" => Some(spirit::Ui4Key::Enter),
                "escape" => Some(spirit::Ui4Key::Escape),
                "tab" => Some(spirit::Ui4Key::Tab),
                "space" => Some(spirit::Ui4Key::Space),
                "backspace" => Some(spirit::Ui4Key::Backspace),
                "up" => Some(spirit::Ui4Key::Up),
                "down" => Some(spirit::Ui4Key::Down),
                "left" => Some(spirit::Ui4Key::Left),
                "right" => Some(spirit::Ui4Key::Right),
                _ => None,
            };
            match key {
                Some(key) => match retry_ui4_busy(|| spirit::dobby_ui4_key(key)) {
                    Ok(()) => ToolExecution::plain(format!("ok: key {name} queued"), true),
                    Err(error) => ToolExecution::plain(
                        format!("failed: Spirit UI4 key unavailable error={error:?}"),
                        false,
                    ),
                },
                None => ToolExecution::plain("rejected: unknown key".to_string(), false),
            }
        }
        _ => ToolExecution::plain(execute_spirit_tool(state, call), false),
    }
}

struct ExecutedTools {
    wire_messages: Vec<Value>,
    compact_messages: Vec<Value>,
    continue_turn: bool,
}

fn execute_completion_tools(state: &mut AppState, completion: &NormalCompletion) -> ExecutedTools {
    let mut wire_messages = Vec::with_capacity(completion.tool_calls.len());
    let mut compact_messages = Vec::with_capacity(completion.tool_calls.len());
    let mut continue_turn = false;
    for call in &completion.tool_calls {
        let result = execute_tool(state, call);
        continue_turn |= result.continue_turn;
        wire_messages.push(json!({
            "role": "tool",
            "tool_call_id": call.id,
            "content": result.wire_content,
        }));
        compact_messages.push(json!({
            "role": "tool",
            "tool_call_id": call.id,
            "content": truncate_utf8(result.compact_content.as_str(), 2 * 1_024),
        }));
    }
    ExecutedTools {
        wire_messages,
        compact_messages,
        continue_turn,
    }
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

fn omit_request_scoped_images(messages: &mut [Value]) -> bool {
    let mut omitted = false;
    for message in messages {
        let Some(parts) = message.get("content").and_then(Value::as_array) else {
            continue;
        };
        if !parts
            .iter()
            .any(|part| part.get("type").and_then(Value::as_str) == Some("image_url"))
        {
            continue;
        }

        let mut text = String::new();
        for part in parts {
            if part.get("type").and_then(Value::as_str) != Some("text") {
                continue;
            }
            let Some(part_text) = part.get("text").and_then(Value::as_str) else {
                continue;
            };
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(part_text);
        }
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str("PNG omitted because the bounded VM request envelope had no image headroom.");
        message["content"] = Value::String(text);
        omitted = true;
    }
    omitted
}

fn serialize_bounded_request(request: &Value) -> Result<Vec<u8>, String> {
    let body =
        serde_json::to_vec(request).map_err(|_| "request JSON serialization failed".to_string())?;
    if body.len() > MAX_REMOTE_BODY_BYTES {
        Err(format!(
            "request body exceeds bounded {} byte VM envelope",
            MAX_REMOTE_BODY_BYTES
        ))
    } else {
        Ok(body)
    }
}

fn normal_body_for_messages(
    config: &RuntimeConfig,
    messages: &mut Vec<Value>,
) -> Result<Vec<u8>, String> {
    let request = normal_request_for_messages(config, messages.as_slice());
    match serialize_bounded_request(&request) {
        Ok(body) => Ok(body),
        Err(first_error) if omit_request_scoped_images(messages.as_mut_slice()) => {
            vshell::line("dobby: observation PNG omitted from continuation due to VM body bound");
            serialize_bounded_request(&normal_request_for_messages(config, messages.as_slice()))
                .map_err(|_| first_error)
        }
        Err(reason) => Err(reason),
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

    if state.config_error.is_some()
        || (state.config.endpoint.starts_with("https://") && !state.config.api_key_configured())
    {
        reload_config(state);
    }
    if let Some(reason) = state.config_error.clone() {
        fail_before_request(state, idea, reason);
        return;
    }

    let (body, request_messages, history_messages) = match idea.kind {
        IdeaKind::Summary => {
            match serialize_bounded_request(&summary_request(&state.config, &state.conversation)) {
                Ok(body) => (body, None, None),
                Err(reason) => {
                    fail_before_request(state, idea, reason);
                    return;
                }
            }
        }
        IdeaKind::Autonomous { .. } | IdeaKind::User => {
            let (_, mut messages) =
                normal_request(&state.config, &state.conversation, idea.prompt.as_str());
            let history_messages = messages.clone();
            match normal_body_for_messages(&state.config, &mut messages) {
                Ok(body) => (body, Some(messages), Some(history_messages)),
                Err(reason) => {
                    fail_before_request(state, idea, reason);
                    return;
                }
            }
        }
    };
    let api_key = if state.config.endpoint.starts_with("https://") {
        Some(state.config.api_key.as_bytes())
    } else {
        None
    };
    let operation = match netfs::fetch_post_json_bytes_with_timeout(
        state.config.endpoint.as_bytes(),
        body.as_slice(),
        api_key.filter(|key| !key.is_empty()),
        REQUEST_TIMEOUT_MS,
    ) {
        Ok(operation) => operation,
        Err(code) => {
            fail_before_request(
                state,
                idea,
                format!("remote operation could not start code={code}"),
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
        "dobby: remote request started kind={} request={} transport={} model={}",
        idea.kind.name(),
        state.remote_requests,
        if state.config.endpoint.starts_with("https://") {
            "https"
        } else {
            "private-http"
        },
        state.config.model,
    ));
    state.in_flight = Some(InFlight {
        operation,
        idea,
        request_messages,
        history_messages,
        tool_round: 0,
        tool_calls: 0,
        started_ms,
    });
}

fn append_bounded_record(record: &mut String, text: &str) {
    if record.len() >= MAX_COMPACT_TURN_BYTES {
        return;
    }
    let remaining = MAX_COMPACT_TURN_BYTES - record.len();
    record.push_str(truncate_utf8(text, remaining).as_str());
}

fn compact_turn_record(messages: &[Value]) -> String {
    let mut record = "Dobby tool record:".to_string();
    for message in messages {
        match message.get("role").and_then(Value::as_str) {
            Some("assistant") => {
                let Some(calls) = message.get("tool_calls").and_then(Value::as_array) else {
                    continue;
                };
                for call in calls {
                    let name = call
                        .get("function")
                        .and_then(|function| function.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    let arguments = call
                        .get("function")
                        .and_then(|function| function.get("arguments"))
                        .and_then(Value::as_str)
                        .unwrap_or("{}");
                    append_bounded_record(&mut record, format!("\n- {name} {arguments}").as_str());
                }
            }
            Some("tool") => {
                if let Some(content) = message.get("content").and_then(Value::as_str) {
                    append_bounded_record(&mut record, format!(" => {content}").as_str());
                }
            }
            _ => {}
        }
    }
    record
}

fn commit_normal_history(state: &mut AppState, messages: Vec<Value>) {
    let prefix_len = state.conversation.messages.len();
    let mut compact = state.conversation.messages.clone();
    if let Some(user_message) = messages.get(prefix_len) {
        compact.push(user_message.clone());
    }
    let record_start = prefix_len.saturating_add(1).min(messages.len());
    compact.push(json!({
        "role": "assistant",
        "content": compact_turn_record(&messages[record_start..]),
    }));
    state.conversation.messages = compact;
    state.conversation.normal_turns = state.conversation.normal_turns.saturating_add(1);
    if state.conversation.normal_turns >= NORMAL_TURNS_PER_CHAT {
        state.conversation.summary_due = true;
        queue_summary(state);
    }
}

fn finish_request_error(state: &mut AppState, mut in_flight: InFlight, reason: String) {
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
    } else if in_flight.tool_calls != 0 {
        if let Some(messages) = in_flight.history_messages.take() {
            commit_normal_history(state, messages);
            vshell::linef(format_args!(
                "dobby: {} tool turn committed partially after {} calls; continuation failed reason={reason}",
                in_flight.idea.kind.name(),
                in_flight.tool_calls,
            ));
        } else {
            vshell::linef(format_args!(
                "dobby: {} continuation failed reason={reason}",
                in_flight.idea.kind.name(),
            ));
        }
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
    let calls_this_round = u8::try_from(completion.tool_calls.len()).unwrap_or(u8::MAX);
    let total_calls = in_flight.tool_calls.saturating_add(calls_this_round);
    if total_calls > MAX_TOOL_CALLS_PER_TURN {
        finish_request_error(
            state,
            in_flight,
            format!("tool turn exceeds {MAX_TOOL_CALLS_PER_TURN} calls"),
        );
        return;
    }
    for call in &completion.tool_calls {
        if let Err(reason) = validate_tool_call(call) {
            finish_request_error(state, in_flight, format!("tool call rejected: {reason}"));
            return;
        }
    }

    let Some(mut wire_messages) = in_flight.request_messages.take() else {
        finish_request_error(
            state,
            in_flight,
            "ordinary request lost its message snapshot".to_string(),
        );
        return;
    };
    let Some(mut history_messages) = in_flight.history_messages.take() else {
        finish_request_error(
            state,
            in_flight,
            "ordinary request lost its compact history snapshot".to_string(),
        );
        return;
    };
    let executed = execute_completion_tools(state, &completion);
    wire_messages.push(completion.assistant_message.clone());
    wire_messages.extend(executed.wire_messages);
    history_messages.push(completion.assistant_message);
    history_messages.extend(executed.compact_messages);

    let next_round = in_flight.tool_round.saturating_add(1);
    let continue_turn = executed.continue_turn
        && next_round < MAX_TOOL_ROUNDS_PER_TURN
        && total_calls < MAX_TOOL_CALLS_PER_TURN;
    if continue_turn {
        let body = match normal_body_for_messages(&state.config, &mut wire_messages) {
            Ok(body) => body,
            Err(reason) => {
                in_flight.history_messages = Some(history_messages);
                in_flight.tool_calls = total_calls;
                finish_request_error(state, in_flight, reason);
                return;
            }
        };
        let api_key = if state.config.endpoint.starts_with("https://") {
            Some(state.config.api_key.as_bytes())
        } else {
            None
        };
        let operation = match netfs::fetch_post_json_bytes_with_timeout(
            state.config.endpoint.as_bytes(),
            body.as_slice(),
            api_key.filter(|key| !key.is_empty()),
            REQUEST_TIMEOUT_MS,
        ) {
            Ok(operation) => operation,
            Err(code) => {
                in_flight.history_messages = Some(history_messages);
                in_flight.tool_calls = total_calls;
                finish_request_error(
                    state,
                    in_flight,
                    format!("tool continuation could not start code={code}"),
                );
                return;
            }
        };
        state.remote_requests = state.remote_requests.saturating_add(1);
        vshell::linef(format_args!(
            "dobby: tool continuation started kind={} round={}/{} request={}",
            in_flight.idea.kind.name(),
            next_round + 1,
            MAX_TOOL_ROUNDS_PER_TURN,
            state.remote_requests,
        ));
        state.in_flight = Some(InFlight {
            operation,
            idea: in_flight.idea,
            request_messages: Some(wire_messages),
            history_messages: Some(history_messages),
            tool_round: next_round,
            tool_calls: total_calls,
            started_ms: clock::monotonic_millis(),
        });
        return;
    }

    if executed.continue_turn {
        vshell::linef(format_args!(
            "dobby: bounded tool loop reached rounds={} calls={}",
            next_round, total_calls,
        ));
    }
    commit_normal_history(state, history_messages);
    state.last_error = None;
    vshell::linef(format_args!(
        "dobby: turn committed kind={} tools={} rounds={} chat_turn={}/{}",
        in_flight.idea.kind.name(),
        total_calls,
        next_round,
        state.conversation.normal_turns,
        NORMAL_TURNS_PER_CHAT,
    ));
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
        Err(FETCH_PENDING) => {
            let now_ms = clock::monotonic_millis();
            let timed_out = state
                .in_flight
                .as_ref()
                .is_some_and(|request| request.hard_timeout_elapsed(now_ms));
            if !timed_out {
                return;
            }

            let _ = netfs::fetch_bytes_discard(operation);
            let Some(in_flight) = state.in_flight.take() else {
                return;
            };
            finish_request_error(
                state,
                in_flight,
                format!("remote request exceeded local {REQUEST_TIMEOUT_MS}ms deadline"),
            );
        }
        Err(code) => {
            let _ = netfs::fetch_bytes_discard(operation);
            let Some(in_flight) = state.in_flight.take() else {
                return;
            };
            finish_request_error(
                state,
                in_flight,
                format!("remote request failed code={code}"),
            );
        }
        Ok(_) => {
            let result = netfs::fetch_bytes_read(operation)
                .map_err(|code| format!("remote response read failed code={code}"));
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
    let key = if state.config.endpoint.starts_with("https://") {
        if state.config.api_key_configured() && state.config_error.is_none() {
            "configured"
        } else {
            "missing"
        }
    } else {
        "optional"
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
        state
            .config
            .reasoning_effort
            .as_deref()
            .unwrap_or("provider-default"),
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
        "dobby-bp: online ownership=blueprint-policy+tool-loop+conversation kernel=UI4-broker+silent-spirit",
    );
    vshell::line(
        "dobby-bp: OpenAI-compatible REST+PNG observations; shell2/Lumen/local TTS absent",
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
    extern "C" fn trueos_cabi_poll_once() {}

    #[unsafe(no_mangle)]
    extern "C" fn trueos_cabi_sleep_ms(_ms: u64) {}

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

    #[unsafe(no_mangle)]
    extern "C" fn trueos_cabi_dobby_ui4_windows(out: *mut u8, cap: usize) -> isize {
        const WINDOWS: &[u8] = b"[]";
        if out.is_null() || cap < WINDOWS.len() {
            return WINDOWS.len() as isize;
        }
        unsafe { core::ptr::copy_nonoverlapping(WINDOWS.as_ptr(), out, WINDOWS.len()) };
        WINDOWS.len() as isize
    }

    #[unsafe(no_mangle)]
    extern "C" fn trueos_cabi_dobby_ui4_focus(_window_id: u64) -> i32 {
        0
    }

    #[unsafe(no_mangle)]
    extern "C" fn trueos_cabi_dobby_ui4_observe_prepare() -> isize {
        -8
    }

    #[unsafe(no_mangle)]
    extern "C" fn trueos_cabi_dobby_ui4_observe_metadata(_out: *mut u8, _cap: usize) -> isize {
        -8
    }

    #[unsafe(no_mangle)]
    extern "C" fn trueos_cabi_dobby_ui4_observe_read(
        _offset: usize,
        _out: *mut u8,
        _cap: usize,
    ) -> isize {
        -8
    }

    #[unsafe(no_mangle)]
    extern "C" fn trueos_cabi_dobby_ui4_pointer(_x: u16, _y: u16, _action: u32) -> i32 {
        0
    }

    #[unsafe(no_mangle)]
    extern "C" fn trueos_cabi_dobby_ui4_type(_text: *const u8, _len: usize) -> i32 {
        0
    }

    #[unsafe(no_mangle)]
    extern "C" fn trueos_cabi_dobby_ui4_key(_key: u32) -> i32 {
        0
    }

    #[unsafe(no_mangle)]
    extern "C" fn trueos_cabi_net_fetch_post_json_bytes_start_with_timeout(
        _url_ptr: *const u8,
        _url_len: usize,
        _body_ptr: *const u8,
        _body_len: usize,
        _bearer_ptr: *const u8,
        _bearer_len: usize,
        _timeout_ms: u32,
    ) -> u32 {
        42
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> RuntimeConfig {
        RuntimeConfig {
            api_key: "secret".to_string(),
            endpoint: DEFAULT_ENDPOINT.to_string(),
            allow_insecure_http: false,
            model: DEFAULT_MODEL.to_string(),
            reasoning_effort: Some("low".to_string()),
            loop_interval_ms: DEFAULT_LOOP_INTERVAL_MS,
        }
    }

    #[test]
    fn private_http_requires_explicit_opt_in() {
        let mut config = config();
        config.endpoint = "http://192.168.178.111:8080/v1/chat/completions".to_string();
        assert!(config.validate().is_err());

        config.allow_insecure_http = true;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn private_http_accepts_only_literal_loopback_or_rfc1918_ipv4() {
        let accepted = [
            "http://127.0.0.1/v1/chat/completions",
            "http://127.255.255.254:8080/v1/chat/completions",
            "http://10.0.0.1/v1/chat/completions",
            "http://172.16.0.1/v1/chat/completions",
            "http://172.31.255.255/v1/chat/completions",
            "http://192.168.178.111:8080/v1/chat/completions",
        ];
        for endpoint in accepted {
            let mut config = config();
            config.endpoint = endpoint.to_string();
            config.allow_insecure_http = true;
            assert!(config.validate().is_ok(), "should accept {endpoint}");
        }

        let rejected = [
            "http://localhost:8080/v1/chat/completions",
            "http://8.8.8.8/v1/chat/completions",
            "http://169.254.1.1/v1/chat/completions",
            "http://172.15.255.255/v1/chat/completions",
            "http://172.32.0.1/v1/chat/completions",
            "http://192.168.178.111:0/v1/chat/completions",
            "http://192.168.178.111:not-a-port/v1/chat/completions",
            "http://192.168.178.111@8.8.8.8/v1/chat/completions",
            "http://[::1]:8080/v1/chat/completions",
        ];
        for endpoint in rejected {
            let mut config = config();
            config.endpoint = endpoint.to_string();
            config.allow_insecure_http = true;
            assert!(config.validate().is_err(), "should reject {endpoint}");
        }
    }

    #[test]
    fn legacy_config_defaults_private_http_opt_in_to_false() {
        let config: RuntimeConfig = serde_json::from_value(json!({
            "api_key": "secret",
            "endpoint": DEFAULT_ENDPOINT,
            "model": DEFAULT_MODEL,
            "reasoning_effort": "low",
            "loop_interval_ms": DEFAULT_LOOP_INTERVAL_MS
        }))
        .unwrap();

        assert!(!config.allow_insecure_http);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn ordinary_request_has_ui4_tools_parallel_calls_and_low_remote_reasoning() {
        let conversation = Conversation::new();
        let (request, messages) = normal_request(&config(), &conversation, AUTONOMOUS_PROMPT);

        assert_eq!(request["tools"].as_array().unwrap().len(), 9);
        assert_eq!(request["tool_choice"], "required");
        assert_eq!(request["parallel_tool_calls"], true);
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
    fn parses_openai_compatible_tool_call() {
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
    fn accepts_bounded_multiple_tools_and_rejects_zero_tools() {
        let no_tool = br#"{"choices":[{"message":{"role":"assistant","content":"hello"}}]}"#;
        let two_tools = br#"{
            "choices":[{"message":{"role":"assistant","content":null,"tool_calls":[
                {"id":"call_1","type":"function","function":{"name":"play_emotion","arguments":"{\"idea\":\"joy\"}"}},
                {"id":"call_2","type":"function","function":{"name":"move","arguments":"{\"x\":0.25,\"y\":0.75}"}}
            ]}}]
        }"#;

        assert!(parse_normal_completion(no_tool).is_err());
        let completion = parse_normal_completion(two_tools).unwrap();
        assert_eq!(completion.tool_calls.len(), 2);
        assert_eq!(completion.tool_calls[0].name, "play_emotion");
        assert_eq!(completion.tool_calls[1].name, "move");
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
    fn validates_ui4_ids_grid_coordinates_and_bounded_typing() {
        let focus = ToolCall {
            id: "call_focus".to_string(),
            name: "ui4_focus".to_string(),
            arguments: r#"{"window_id":"4294967297"}"#.to_string(),
        };
        let pointer = ToolCall {
            id: "call_pointer".to_string(),
            name: "ui4_pointer".to_string(),
            arguments: r#"{"x":1000,"y":0,"action":"click"}"#.to_string(),
        };
        let bad_pointer = ToolCall {
            id: "call_bad_pointer".to_string(),
            name: "ui4_pointer".to_string(),
            arguments: r#"{"x":1001,"y":0,"action":"click"}"#.to_string(),
        };
        let control_text = ToolCall {
            id: "call_type".to_string(),
            name: "ui4_type".to_string(),
            arguments: r#"{"text":"hello\nworld"}"#.to_string(),
        };

        assert!(validate_tool_call(&focus).is_ok());
        assert!(validate_tool_call(&pointer).is_ok());
        assert!(validate_tool_call(&bad_pointer).is_err());
        assert!(validate_tool_call(&control_text).is_err());
    }

    #[test]
    fn base64_encoding_matches_png_data_uri_building_primitives() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
    }

    #[test]
    fn bounded_body_fallback_removes_request_scoped_images_only() {
        let mut messages = vec![
            json!({"role":"user","content":"keep"}),
            json!({
                "role":"tool",
                "content":[
                    {"type":"text","text":"window metadata"},
                    {"type":"image_url","image_url":{"url":"data:image/png;base64,AAAA"}}
                ]
            }),
        ];

        assert!(omit_request_scoped_images(messages.as_mut_slice()));
        assert_eq!(messages[0]["content"], "keep");
        assert!(
            messages[1]["content"]
                .as_str()
                .unwrap()
                .contains("window metadata")
        );
        assert!(
            messages[1]["content"]
                .as_str()
                .unwrap()
                .contains("PNG omitted")
        );
    }

    #[test]
    fn information_tool_continues_without_spending_an_extra_logical_turn() {
        let mut state = AppState::new(config(), None);
        let mut messages = initial_messages(None);
        messages.push(json!({"role":"user","content":"inspect"}));
        let request = InFlight {
            operation: 3,
            idea: QueuedIdea {
                kind: IdeaKind::User,
                session: state.session,
                prompt: "inspect".to_string(),
            },
            request_messages: Some(messages.clone()),
            history_messages: Some(messages),
            tool_round: 0,
            tool_calls: 0,
            started_ms: 0,
        };
        let inventory_response = br#"{
            "choices":[{"message":{"role":"assistant","content":null,"tool_calls":[
                {"id":"call_windows","type":"function","function":{"name":"ui4_windows","arguments":"{}"}}
            ]}}]
        }"#;

        finish_normal(&mut state, request, inventory_response);

        assert_eq!(state.conversation.normal_turns, 0);
        let continuation = state.in_flight.take().unwrap();
        assert_eq!(continuation.operation, 42);
        assert_eq!(continuation.tool_round, 1);
        assert_eq!(continuation.tool_calls, 1);

        let final_response = br#"{
            "choices":[{"message":{"role":"assistant","content":null,"tool_calls":[
                {"id":"call_text","type":"function","function":{"name":"text","arguments":"{\"text\":\"I found the windows.\"}"}}
            ]}}]
        }"#;
        finish_normal(&mut state, continuation, final_response);

        assert!(state.in_flight.is_none());
        assert_eq!(state.conversation.normal_turns, 1);
        assert_eq!(state.remote_requests, 1);
        assert_eq!(state.conversation.messages.len(), 3);
        assert!(
            state
                .conversation
                .messages
                .iter()
                .all(|message| message["role"] != "tool")
        );
    }

    #[test]
    fn configurable_reasoning_effort_reaches_normal_and_summary_requests() {
        let mut config = config();
        config.model = "zai-glm-4.7".to_string();
        config.reasoning_effort = Some("none".to_string());
        assert!(config.validate().is_ok());
        let conversation = Conversation::new();
        let (normal, _) = normal_request(&config, &conversation, AUTONOMOUS_PROMPT);
        let summary = summary_request(&config, &conversation);

        assert_eq!(normal["reasoning_effort"], "none");
        assert_eq!(summary["reasoning_effort"], "none");
    }

    #[test]
    fn reasoning_effort_is_model_aware_and_can_be_omitted() {
        let mut config = config();
        config.reasoning_effort = Some("none".to_string());
        assert!(config.validate().is_err());

        config.model = "zai-glm-4.7".to_string();
        config.reasoning_effort = Some("low".to_string());
        assert!(config.validate().is_err());

        config.model = "future-model".to_string();
        config.reasoning_effort = None;
        assert!(config.validate().is_ok());
        let conversation = Conversation::new();
        let (normal, _) = normal_request(&config, &conversation, AUTONOMOUS_PROMPT);
        let summary = summary_request(&config, &conversation);
        assert!(normal.get("reasoning_effort").is_none());
        assert!(summary.get("reasoning_effort").is_none());
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
            request_messages: Some(request_messages.clone()),
            history_messages: Some(request_messages),
            tool_round: 0,
            tool_calls: 0,
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
            history_messages: None,
            tool_round: 0,
            tool_calls: 0,
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
    fn in_flight_hard_timeout_trips_at_request_deadline() {
        let request = InFlight {
            operation: 9,
            idea: QueuedIdea {
                kind: IdeaKind::User,
                session: 1,
                prompt: "hello".to_string(),
            },
            request_messages: None,
            history_messages: None,
            tool_round: 0,
            tool_calls: 0,
            started_ms: 1_000,
        };

        assert!(!request.hard_timeout_elapsed(1_000));
        assert!(!request.hard_timeout_elapsed(1_000 + u64::from(REQUEST_TIMEOUT_MS) - 1));
        assert!(request.hard_timeout_elapsed(1_000 + u64::from(REQUEST_TIMEOUT_MS)));
        assert!(!request.hard_timeout_elapsed(999));
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
