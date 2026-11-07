//! Owned source snapshots and project settings shared without a database runtime.

use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};

pub const DEFAULT_DATABASE_PATH: &str = "rustc-sources.redb";
/// Shared source image for the IDE and other coordinated development tools.
pub const IDE_DATABASE_PATH: &str = "/apps/common/ide/projects.redb";
pub const DEFAULT_FILE_MODE: u32 = 0o100644;


#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceFileInput {
    pub path: String,
    pub bytes: Vec<u8>,
    pub mode: u32,
}
impl SourceFileInput {
    pub fn new(path: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            path: path.into(),
            bytes: bytes.into(),
            mode: DEFAULT_FILE_MODE,
        }
    }
    pub fn with_mode(path: impl Into<String>, bytes: impl Into<Vec<u8>>, mode: u32) -> Self {
        Self {
            path: path.into(),
            bytes: bytes.into(),
            mode,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceFile {
    pub path: String,
    pub bytes: Vec<u8>,
    pub mode: u32,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SourceOrigin {
    Local { label: Option<String> },
    Git { repository: String, commit: String },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RevisionSummary {
    pub id: i64,
    pub project: String,
    pub revision: u64,
    pub origin: SourceOrigin,
    pub file_count: u64,
    pub source_bytes: u64,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub id: i64,
    pub name: String,
    pub revision_count: u64,
    pub latest_revision: Option<u64>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LoadedSource {
    pub revision: RevisionSummary,
    pub files: Vec<SourceFile>,
}
impl LoadedSource {
    pub fn file(&self, path: &str) -> Option<&SourceFile> {
        self.files
            .binary_search_by(|f| f.path.as_str().cmp(path))
            .ok()
            .map(|i| &self.files[i])
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportDisposition {
    Inserted,
    AlreadyPresent,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportResult {
    pub disposition: ImportDisposition,
    pub revision: RevisionSummary,
}

/// Build settings saved with a project, independent of its source revisions.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectBuildConfig {
    #[serde(default)]
    pub kind: crate::protocol::ProjectKind,
    pub compiler: Option<crate::protocol::CompilerBackend>,
    pub entry: String,
}

