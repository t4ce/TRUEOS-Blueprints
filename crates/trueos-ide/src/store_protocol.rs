//! Source-store operations. The kernel checks project leases before mutations.
use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};
use crate::{LoadedSource, ProjectBuildConfig, ProjectSummary, RevisionSummary, SourceFileInput};

pub const REQUEST: &str = "vFile:ide/store";
pub const REPLY: &str = "vFile:ide/store-reply";
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub database: String,
    pub command: Command,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    List {},
    Get { project: String, revision: Option<u64> },
    GetConfig { project: String },
    Create { project: String, files: Vec<SourceFileInput> },
    Save { project: String, expected_revision_id: i64, files: Vec<SourceFileInput> },
    SetConfig { project: String, config: ProjectBuildConfig },
}

impl Command {
    pub fn write_project(&self) -> Option<&str> {
        match self {
            Self::Create { project, .. } | Self::Save { project, .. } | Self::SetConfig { project, .. } => Some(project),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Reply {
    Projects { projects: Vec<ProjectSummary> },
    Source { source: Option<LoadedSource> },
    Config { config: Option<ProjectBuildConfig> },
    Saved { revision: RevisionSummary },
    Done {},
    Error { code: String, message: String },
}
