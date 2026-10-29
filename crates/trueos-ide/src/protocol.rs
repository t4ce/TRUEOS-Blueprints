//! Versioned editor actions carried by the native IDE routing endpoint.
//! The broker routes these payloads; Edit owns their meaning and source state.

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
