//! Versioned editor actions carried by the native IDE routing endpoint.
//! The broker routes these payloads; Edit owns their meaning and source state.

use alloc::{string::{String, ToString}, vec::Vec};
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;
pub const MAX_WIRE_BYTES: usize = 64 * 1024;
pub const REGISTER: &str = "vFile:ide/register";
pub const DISCOVER: &str = "vFile:ide/discover";
pub const REQUEST: &str = "vFile:ide/request";
pub const POLL: &str = "vFile:ide/poll";
pub const REPLY: &str = "vFile:ide/reply";
pub const CANCEL: &str = "vFile:ide/cancel";
pub const UNREGISTER: &str = "vFile:ide/unregister";

/// One-shot Codex startup directive. The endpoint identifies an Edit owner
/// incarnation, so a closed or switched workspace cannot silently be replaced.
pub fn codex_start_script(endpoint: u64) -> Result<String, String> {
    if endpoint == 0 { return Err("IDE endpoint must be nonzero".into()); }
    Ok(format!("ide-endpoint {endpoint}\n"))
}

pub fn codex_start_endpoint(script: &str) -> Result<Option<u64>, String> {
    let mut endpoint = None;
    for line in script.lines() {
        let Some(value) = line.strip_prefix("ide-endpoint ") else {
            if line == "ide-endpoint" { return Err("ide-endpoint requires a value".into()); }
            continue;
        };
        if endpoint.is_some() || value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
            return Err("Expected one numeric ide-endpoint directive".into());
        }
        let id = value.parse::<u64>().map_err(|_| "Invalid IDE endpoint")?;
        if id == 0 { return Err("IDE endpoint must be nonzero".into()); }
        endpoint = Some(id);
    }
    Ok(endpoint)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Registration {
    pub version: u32,
    pub project: String,
    pub database: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Endpoint {
    pub endpoint: u64,
    pub project: String,
    pub database: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClientRequest {
    pub endpoint: u64,
    pub request_id: u64,
    pub command: IdeRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OwnerRequest {
    pub endpoint: u64,
    pub client_token: u64,
    pub request_id: u64,
    pub command: IdeRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OwnerReply {
    pub endpoint: u64,
    pub client_token: u64,
    pub request_id: u64,
    pub reply: IdeResponse,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum IdeRequest {
    Context {},
    /// Read immutable platform documentation independently of project files.
    ApiReference { topic: String },
    ReadFile {
        path: String,
    },
    /// The expected text is compared with the live editor buffer, including
    /// unsaved human edits. `None` means the file must not already exist.
    WriteFile {
        path: String,
        expected_revision_id: i64,
        expected_text: Option<String>,
        text: String,
    },
    Compile {
        expected_revision_id: i64,
    },
    BuildStatus {},
    CancelBuild {},
    Run {},
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum IdeResponse {
    Context { context: WorkspaceContext },
    ApiReference { reference: ApiReferenceDocument },
    File { file: FileContents },
    Saved { revision_id: i64 },
    Build { build: BuildState },
    Started { build_id: u64 },
    LaunchQueued { archive: String },
    Cancelled {},
    Error { code: String, message: String },
}

impl IdeResponse {
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Error {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceContext {
    pub project: String,
    pub database: String,
    pub compiler: CompilerBackend,
    pub entry: String,
    pub revision_id: i64,
    pub active_file: Option<String>,
    pub open_files: Vec<String>,
    pub selected_text: String,
    pub build: BuildState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_reference: Option<PlatformApiContext>,
}

/// Compact API index included in IDE context; full topics are fetched on demand.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlatformApiContext {
    pub api_id: String,
    pub source_sha256: String,
    pub index: String,
    pub topics: Vec<PlatformApiTopic>,
}

impl PlatformApiContext {
    /// Read-only platform reference metadata for an IDE context attachment.
    pub fn render_summary(&self) -> String {
        let mut text = format!("\n## TRUEOS platform API reference (read-only)\nAPI version: {}\nSource fingerprint: {}\nRetrieve a full topic with the IDE api_reference operation; documentation is independent of the project compiler and is not a runtime capability grant.\n\n{}\nAvailable topics:\n", self.api_id, self.source_sha256, self.index);
        for topic in &self.topics {
            text.push_str(&format!("- {}: {} ({} bytes)\n", topic.name, topic.title, topic.bytes));
        }
        text
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlatformApiTopic {
    pub name: String,
    pub title: String,
    pub bytes: usize,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiReferenceDocument {
    pub api_id: String,
    pub topic: String,
    pub sha256: String,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileContents {
    pub path: String,
    pub text: String,
    pub revision_id: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum BuildState {
    #[default]
    Idle,
    Running {
        build_id: u64,
        revision_id: i64,
    },
    Succeeded {
        build_id: u64,
        revision_id: i64,
        artifact: BuildArtifact,
    },
    Failed {
        build_id: u64,
        revision_id: i64,
        message: String,
    },
    Cancelled {
        build_id: u64,
        revision_id: i64,
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompilerBackend {
    OdinSon,
    RustcMin,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotFile {
    pub path: String,
    pub bytes: Vec<u8>,
    pub mode: u32,
}

/// A transient immutable input file consumed by a disposable compiler VM.
/// Compilers never open the live project database.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildRequest {
    pub version: u32,
    pub workspace_epoch: u64,
    pub build_id: u64,
    pub project: String,
    pub revision_id: i64,
    pub backend: CompilerBackend,
    pub entry: String,
    pub files: Vec<SnapshotFile>,
    pub output_path: String,
    pub result_path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildArtifact {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

/// Success is explicit and tied to one input; VM exit and old files do not
/// establish successful compilation. Human stdout is a separate stream.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildResult {
    pub version: u32,
    pub workspace_epoch: u64,
    pub build_id: u64,
    pub project: String,
    pub revision_id: i64,
    pub success: bool,
    pub artifact: Option<BuildArtifact>,
    pub diagnostics: String,
}

pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_WIRE_BYTES {
        return Err("IDE message exceeds 64 KiB; use smaller source changes".into());
    }
    Ok(bytes)
}

pub fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    if bytes.len() > MAX_WIRE_BYTES {
        return Err("IDE message exceeds 64 KiB".into());
    }
    serde_json::from_slice(bytes).map_err(|error| error.to_string())
}

#[cfg(test)]
mod launch_tests {
    use super::*;

    #[test]
    fn older_workspace_context_does_not_require_platform_reference() {
        let context: WorkspaceContext = serde_json::from_str(r#"{
            "project":"demo", "database":"projects.redb", "compiler":"odin_son",
            "entry":"main.odin", "revision_id":1, "active_file":null,
            "open_files":[], "selected_text":"", "build":{"state":"idle"}
        }"#).unwrap();
        assert!(context.api_reference.is_none());
        assert!(!serde_json::to_string(&context).unwrap().contains("api_reference"));
    }

    #[test]
    fn codex_startup_keeps_the_exact_workspace_endpoint() {
        assert_eq!(codex_start_endpoint(&codex_start_script(42).unwrap()).unwrap(), Some(42));
        assert_eq!(codex_start_endpoint("fs-scope trueosfs\nide-endpoint 18446744073709551615\n").unwrap(), Some(u64::MAX));
        assert_eq!(codex_start_endpoint("# ordinary Codex launch\n").unwrap(), None);
        assert!(codex_start_script(0).is_err());
        for script in ["ide-endpoint", "ide-endpoint ", "ide-endpoint 0", "ide-endpoint -1",
            "ide-endpoint 1x", "ide-endpoint 1 2", "ide-endpoint 18446744073709551616",
            "ide-endpoint 1\nide-endpoint 2\n", "ide-endpoint 1\0"] {
            assert!(codex_start_endpoint(script).is_err(), "{script:?}");
        }
    }
}
