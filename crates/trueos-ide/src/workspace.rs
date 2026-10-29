//! Build identity, immutable input staging, and explicit result verification.
//! VM spawning and UI polling remain the editor's responsibility.

use crate::LoadedSource;
use crate::protocol::*;
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub const BUILD_ROOT: &str = "/apps/common/ide/builds";
pub const MAX_BUILD_INPUT_BYTES: usize = 16 * 1024 * 1024;

pub struct PreparedBuild {
    pub request: BuildRequest,
    pub request_path: PathBuf,
}

pub struct BuildTracker {
    epoch: u64,
    owner: Option<u64>,
    next_id: u64,
    pub state: BuildState,
    active: Option<BuildRequest>,
    directory: Option<PathBuf>,
}

impl BuildTracker {
    pub fn new(epoch: u64) -> Self {
        Self {
            epoch,
            owner: None,
            next_id: 1,
            state: BuildState::Idle,
            active: None,
            directory: None,
        }
    }

    /// A broker endpoint is unique among live owners. This keeps concurrent
    /// owners apart even on filesystems without exclusive directory creation.
    pub fn new_owned(epoch: u64, endpoint: u64) -> Self {
        let mut tracker = Self::new(epoch);
        tracker.owner = Some(endpoint);
        tracker
    }

    pub fn active(&self) -> Option<&BuildRequest> {
        self.active.as_ref()
    }

    pub fn prepare(
        &mut self,
        root: &Path,
        source: &LoadedSource,
        backend: CompilerBackend,
        entry: &str,
    ) -> io::Result<PreparedBuild> {
        if self.active.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "a compiler job is already active",
            ));
        }
        if !source.files.iter().any(|file| file.path == entry) {
            return Err(invalid(
                "compiler entry does not exist in the project revision",
            ));
        }
        let build_id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| invalid("build ID exhausted"))?;
        if self.directory.is_none() {
            fs::create_dir_all(root)?;
            loop {
                let name = match self.owner {
                    Some(endpoint) => format!("{}-{endpoint}", self.epoch),
                    None => self.epoch.to_string(),
                };
                let directory = root.join(name);
                match fs::create_dir(&directory) {
                    Ok(()) => {
                        self.directory = Some(directory);
                        break;
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                        self.epoch = self
                            .epoch
                            .checked_add(1)
                            .ok_or_else(|| invalid("workspace epoch exhausted"))?;
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        let session = self.directory.as_ref().unwrap();
        if session.parent() != Some(root) {
            return Err(invalid("build root changed during workspace session"));
        }
        let directory = session.join(build_id.to_string());
        // Never reuse a persisted build directory, even if an epoch was reused
        // after reboot. Old results must not become a new job's result.
        fs::create_dir(&directory)?;
        let digest = sha256(source.revision.project.as_bytes());
        let name: String = source
            .revision
            .project
            .chars()
            .take(48)
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '_' | '-') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let output = directory.join(format!("{name}-{}.bp", &digest[..8]));
        let request = BuildRequest {
            version: VERSION,
            workspace_epoch: self.epoch,
            build_id,
            project: source.revision.project.clone(),
            revision_id: source.revision.id,
            backend,
            entry: entry.into(),
            files: source
                .files
                .iter()
                .map(|file| SnapshotFile {
                    path: file.path.clone(),
                    bytes: file.bytes.clone(),
                    mode: file.mode,
                })
                .collect(),
            output_path: output.to_string_lossy().into_owned(),
            result_path: directory.join("result.json").to_string_lossy().into_owned(),
        };
        validate_request(&request)?;
        let bytes = serde_json::to_vec(&request).map_err(io::Error::other)?;
        if bytes.len() > MAX_BUILD_INPUT_BYTES {
            return Err(invalid("compiler snapshot exceeds 16 MiB"));
        }
        let request_path = directory.join("request.json");
        fs::write(&request_path, bytes)?;
        self.state = BuildState::Running {
            build_id,
            revision_id: request.revision_id,
        };
        self.active = Some(request.clone());
        Ok(PreparedBuild {
            request,
            request_path,
        })
    }

    /// Called only after the compiler has exited and its stdout is drained.
    /// The caller installs a returned artifact through AppDB's existing
    /// validation boundary before presenting the build as ready to run.
    pub fn finish(&mut self) -> io::Result<BuildArtifact> {
        let request = self
            .active
            .take()
            .ok_or_else(|| invalid("no active compiler job"))?;
        let result = read_result(&request);
        match &result {
            Ok(artifact) => {
                self.state = BuildState::Succeeded {
                    build_id: request.build_id,
                    revision_id: request.revision_id,
                    artifact: artifact.clone(),
                }
            }
            Err(error) => {
                self.state = BuildState::Failed {
                    build_id: request.build_id,
                    revision_id: request.revision_id,
                    message: error.to_string(),
                }
            }
        }
        result
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        if let Some(request) = self.active.take() {
            self.state = BuildState::Failed {
                build_id: request.build_id,
                revision_id: request.revision_id,
                message: message.into(),
            };
        } else if let BuildState::Succeeded {
            build_id,
            revision_id,
            ..
        } = self.state
        {
            self.state = BuildState::Failed {
                build_id,
                revision_id,
                message: message.into(),
            };
        }
    }

    /// Mark cancellation before asking the hypervisor to terminate the child.
    /// A late result can then never complete or install this job.
    pub fn cancel(&mut self) -> bool {
        let Some(request) = self.active.take() else {
            return false;
        };
        self.state = BuildState::Cancelled {
            build_id: request.build_id,
            revision_id: request.revision_id,
        };
        true
    }
}

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn validate_request(request: &BuildRequest) -> io::Result<()> {
    if request.version != VERSION
        || request.workspace_epoch == 0
        || request.build_id == 0
        || request.revision_id <= 0
        || request.project.is_empty()
        || request.files.is_empty()
        || request.output_path.is_empty()
        || request.result_path.is_empty()
        || request.output_path == request.result_path
    {
        return Err(invalid("invalid compiler build request identity"));
    }
    let mut paths = std::collections::BTreeSet::new();
    for file in &request.files {
        if !valid_source_path(&file.path) || !paths.insert(file.path.as_str()) {
            return Err(invalid("invalid or duplicate compiler source path"));
        }
    }
    if !paths.contains(request.entry.as_str()) {
        return Err(invalid("missing compiler entry"));
    }
    let output = Path::new(&request.output_path);
    let result = Path::new(&request.result_path);
    if output.extension().is_none_or(|ext| ext != "bp")
        || output.parent() != result.parent()
        || result.file_name().is_none_or(|name| name != "result.json")
    {
        return Err(invalid(
            "compiler output and result must share the build directory",
        ));
    }
    Ok(())
}

pub fn valid_source_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(['\\', '\0', ':'])
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

pub fn read_result(request: &BuildRequest) -> io::Result<BuildArtifact> {
    let length = fs::metadata(&request.result_path)?.len();
    if length > MAX_WIRE_BYTES as u64 {
        return Err(invalid("compiler result exceeds 64 KiB"));
    }
    let result: BuildResult =
        serde_json::from_slice(&fs::read(&request.result_path)?).map_err(io::Error::other)?;
    if result.version != VERSION
        || result.workspace_epoch != request.workspace_epoch
        || result.build_id != request.build_id
        || result.project != request.project
        || result.revision_id != request.revision_id
    {
        return Err(invalid(
            "compiler result does not match the submitted snapshot",
        ));
    }
    if !result.success {
        return Err(io::Error::other(if result.diagnostics.is_empty() {
            "compiler failed without diagnostics".into()
        } else {
            result.diagnostics
        }));
    }
    let artifact = result
        .artifact
        .ok_or_else(|| invalid("successful compiler result has no artifact"))?;
    if artifact.path != request.output_path || artifact.bytes == 0 {
        return Err(invalid("compiler result names an unexpected artifact"));
    }
    if fs::metadata(&artifact.path)?.len() != artifact.bytes {
        return Err(invalid("compiled artifact size does not match result"));
    }
    let bytes = fs::read(&artifact.path)?;
    if sha256(&bytes) != artifact.sha256 {
        return Err(invalid("compiled artifact hash does not match result"));
    }
    Ok(artifact)
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SourceDatabase, SourceFileInput};
    fn fixture() -> (PathBuf, LoadedSource) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "trueos-ide-build-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let mut db = SourceDatabase::open(root.join("sources.redb")).unwrap();
        db.import_local(
            "demo",
            None,
            vec![SourceFileInput::new("main.rs", b"source".to_vec())],
        )
        .unwrap();
        (root, db.get("demo", None).unwrap().unwrap())
    }
    fn result(request: &BuildRequest) -> BuildResult {
        fs::write(&request.output_path, b"verified artifact").unwrap();
        BuildResult {
            version: VERSION,
            workspace_epoch: request.workspace_epoch,
            build_id: request.build_id,
            project: request.project.clone(),
            revision_id: request.revision_id,
            success: true,
            artifact: Some(BuildArtifact {
                path: request.output_path.clone(),
                bytes: 17,
                sha256: sha256(b"verified artifact"),
            }),
            diagnostics: String::new(),
        }
    }
    #[test]
    fn explicit_matching_result_and_hash_are_required() {
        let (root, source) = fixture();
        let mut tracker = BuildTracker::new(7);
        let prepared = tracker
            .prepare(
                &root.join("builds"),
                &source,
                CompilerBackend::RustcMin,
                "main.rs",
            )
            .unwrap();
        let mut completion = result(&prepared.request);
        completion.revision_id += 1;
        fs::write(
            &prepared.request.result_path,
            serde_json::to_vec(&completion).unwrap(),
        )
        .unwrap();
        assert!(tracker.finish().is_err());
        assert!(matches!(tracker.state, BuildState::Failed { .. }));
        let prepared = tracker
            .prepare(
                &root.join("builds"),
                &source,
                CompilerBackend::RustcMin,
                "main.rs",
            )
            .unwrap();
        let completion = result(&prepared.request);
        fs::write(
            &prepared.request.result_path,
            serde_json::to_vec(&completion).unwrap(),
        )
        .unwrap();
        assert!(tracker.finish().is_ok());
        assert!(matches!(tracker.state, BuildState::Succeeded { .. }));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cancellation_discards_late_result_and_second_active_build_is_rejected() {
        let (root, source) = fixture();
        let mut tracker = BuildTracker::new(8);
        let prepared = tracker
            .prepare(
                &root.join("builds"),
                &source,
                CompilerBackend::RustcMin,
                "main.rs",
            )
            .unwrap();
        assert!(
            tracker
                .prepare(
                    &root.join("builds"),
                    &source,
                    CompilerBackend::RustcMin,
                    "main.rs"
                )
                .is_err()
        );
        let completion = result(&prepared.request);
        fs::write(
            &prepared.request.result_path,
            serde_json::to_vec(&completion).unwrap(),
        )
        .unwrap();
        assert!(tracker.cancel());
        assert!(tracker.finish().is_err());
        assert!(matches!(tracker.state, BuildState::Cancelled { .. }));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn changed_artifact_is_rejected_and_same_clock_sessions_reserve_distinct_directories() {
        let (root, source) = fixture();
        let builds = root.join("builds");
        let mut tracker = BuildTracker::new(9);
        let prepared = tracker
            .prepare(&builds, &source, CompilerBackend::RustcMin, "main.rs")
            .unwrap();
        let completion = result(&prepared.request);
        fs::write(
            &prepared.request.result_path,
            serde_json::to_vec(&completion).unwrap(),
        )
        .unwrap();
        fs::write(&prepared.request.output_path, b"tampered artifact").unwrap();
        assert!(tracker.finish().is_err());
        let second = BuildTracker::new(9)
            .prepare(&builds, &source, CompilerBackend::RustcMin, "main.rs")
            .unwrap();
        assert_ne!(
            prepared.request.workspace_epoch,
            second.request.workspace_epoch
        );
        assert_ne!(prepared.request.output_path, second.request.output_path);
        assert!(Path::new(&prepared.request.result_path).is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn live_owner_ids_separate_paths_before_any_filesystem_reservation() {
        let (root, source) = fixture();
        let builds = root.join("builds");
        let first = BuildTracker::new_owned(10, 21)
            .prepare(&builds, &source, CompilerBackend::RustcMin, "main.rs")
            .unwrap();
        let second = BuildTracker::new_owned(10, 22)
            .prepare(&builds, &source, CompilerBackend::RustcMin, "main.rs")
            .unwrap();
        assert_eq!(
            first.request.workspace_epoch,
            second.request.workspace_epoch
        );
        assert_eq!(first.request_path, builds.join("10-21/1/request.json"));
        assert_eq!(second.request_path, builds.join("10-22/1/request.json"));
        // A reboot may reuse an endpoint; retained directories still force a
        // fresh epoch instead of admitting an old result.
        let restarted = BuildTracker::new_owned(10, 21)
            .prepare(&builds, &source, CompilerBackend::RustcMin, "main.rs")
            .unwrap();
        assert_eq!(restarted.request_path, builds.join("11-21/1/request.json"));
        fs::remove_dir_all(root).unwrap();
    }
}
