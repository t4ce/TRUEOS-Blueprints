//! Build identity, immutable input staging, and explicit result verification.
//! VM spawning and UI polling remain the editor's responsibility.

use alloc::{collections::BTreeSet, string::{String, ToString}, vec::Vec};
use core::fmt;
use crate::LoadedSource;
use crate::protocol::*;
use sha2::{Digest, Sha256};
use typed_path::Utf8UnixPath as WirePath;
#[cfg(feature = "std")]
use std::{fs, io, path::{Path, PathBuf}};

pub const BUILD_ROOT: &str = "/apps/common/ide/builds";
pub const MAX_BUILD_INPUT_BYTES: usize = 16 * 1024 * 1024;

/// Storage needed by compiler staging and verification, without a process or DB.
///
/// create_dir reserves a fresh workspace exclusively: existing or previously
/// used directories must fail. A native backend may implement this with an
/// atomic create-only reservation record after refusing existing directories.
/// Plain exists()+create_dir_all() is not an exclusive reservation.
pub trait BuildStorage {
    type Error: fmt::Display;
    fn create_dir_all(&mut self, path: &str) -> Result<(), Self::Error>;
    fn create_dir(&mut self, path: &str) -> Result<(), Self::Error>;
    fn is_already_exists(&self, error: &Self::Error) -> bool;
    fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), Self::Error>;
    fn len(&mut self, path: &str) -> Result<u64, Self::Error>;
    fn read(&mut self, path: &str) -> Result<Vec<u8>, Self::Error>;
}

#[derive(Debug)]
pub enum BuildError<E> {
    Storage(E),
    StorageContext { source: E, message: String },
    Invalid(String),
    Busy,
}
impl<E: fmt::Display> fmt::Display for BuildError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(error) => error.fmt(f),
            Self::StorageContext { message, .. } | Self::Invalid(message) => f.write_str(message),
            Self::Busy => f.write_str("a compiler job is already active"),
        }
    }
}
impl<E: core::error::Error + 'static> core::error::Error for BuildError<E> {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Storage(source) | Self::StorageContext { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidRequest(String);
impl fmt::Display for InvalidRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str(&self.0) }
}
impl core::error::Error for InvalidRequest {}

#[cfg(feature = "std")]
pub type RequestPath = PathBuf;
#[cfg(not(feature = "std"))]
pub type RequestPath = String;

pub struct PreparedBuild {
    pub request: BuildRequest,
    pub request_path: RequestPath,
}

pub struct BuildTracker {
    epoch: u64,
    owner: Option<u64>,
    next_id: u64,
    pub state: BuildState,
    active: Option<BuildRequest>,
    directory: Option<String>,
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

    pub fn active(&self) -> Option<&BuildRequest> { self.active.as_ref() }

    #[cfg(feature = "std")]
    pub fn prepare(
        &mut self, root: &Path, source: &LoadedSource, backend: CompilerBackend, entry: &str,
    ) -> io::Result<PreparedBuild> {
        let root = root.to_str().ok_or_else(|| invalid("compiler build root must be UTF-8"))?;
        self.prepare_with_storage(&mut HostStorage, root, source, backend, entry).map_err(host_error)
    }

    pub fn prepare_with_storage<S: BuildStorage>(
        &mut self, storage: &mut S, root: &str, source: &LoadedSource,
        backend: CompilerBackend, entry: &str,
    ) -> Result<PreparedBuild, BuildError<S::Error>> {
        if self.active.is_some() { return Err(BuildError::Busy); }
        if !source.files.iter().any(|file| file.path == entry) {
            return Err(BuildError::Invalid("compiler entry does not exist in the project revision".into()));
        }
        let build_id = self.next_id;
        self.next_id = self.next_id.checked_add(1)
            .ok_or_else(|| BuildError::Invalid("build ID exhausted".into()))?;
        if self.directory.is_none() {
            storage.create_dir_all(root).map_err(BuildError::Storage)?;
            loop {
                let name = match self.owner {
                    Some(endpoint) => format!("{}-{endpoint}", self.epoch),
                    None => self.epoch.to_string(),
                };
                let directory = WirePath::new(root).join(name).into_string();
                match storage.create_dir(&directory) {
                    Ok(()) => { self.directory = Some(directory); break; }
                    Err(error) if storage.is_already_exists(&error) => {
                        self.epoch = self.epoch.checked_add(1)
                            .ok_or_else(|| BuildError::Invalid("workspace epoch exhausted".into()))?;
                    }
                    Err(error) => return Err(BuildError::Storage(error)),
                }
            }
        }
        let session = WirePath::new(self.directory.as_ref().unwrap());
        if session.parent() != Some(WirePath::new(root)) {
            return Err(BuildError::Invalid("build root changed during workspace session".into()));
        }
        let directory = session.join(build_id.to_string());
        // Never reuse a persisted build directory. A late result must not be
        // admitted as the completion of a later immutable input snapshot.
        storage.create_dir(directory.as_str()).map_err(BuildError::Storage)?;
        let digest = sha256(source.revision.project.as_bytes());
        let name: String = source.revision.project.chars().take(48)
            .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '_' | '-') { c } else { '_' })
            .collect();
        let request = BuildRequest {
            version: VERSION,
            workspace_epoch: self.epoch,
            build_id,
            project: source.revision.project.clone(),
            revision_id: source.revision.id,
            backend,
            entry: entry.into(),
            files: source.files.iter().map(|file| SnapshotFile {
                path: file.path.clone(), bytes: file.bytes.clone(), mode: file.mode,
            }).collect(),
            output_path: directory.join(format!("{name}-{}.bp", &digest[..8])).into_string(),
            result_path: directory.join("result.json").into_string(),
        };
        validate_request_data(&request).map_err(|error| BuildError::Invalid(error.0))?;
        let bytes = serde_json::to_vec(&request)
            .map_err(|error| BuildError::Invalid(error.to_string()))?;
        if bytes.len() > MAX_BUILD_INPUT_BYTES {
            return Err(BuildError::Invalid("compiler snapshot exceeds 16 MiB".into()));
        }
        let request_path = directory.join("request.json").into_string();
        storage.write(&request_path, &bytes).map_err(BuildError::Storage)?;
        self.state = BuildState::Running { build_id, revision_id: request.revision_id };
        self.active = Some(request.clone());
        Ok(PreparedBuild { request, request_path: request_path.into() })
    }

    /// Called only after the compiler has exited and its stdout is drained.
    /// The caller must still install the artifact through AppDB's validation.
    #[cfg(feature = "std")]
    pub fn finish(&mut self) -> io::Result<BuildArtifact> {
        self.finish_with_storage(&mut HostStorage).map_err(host_error)
    }

    pub fn finish_with_storage<S: BuildStorage>(
        &mut self, storage: &mut S,
    ) -> Result<BuildArtifact, BuildError<S::Error>> {
        let request = self.active.take()
            .ok_or_else(|| BuildError::Invalid("no active compiler job".into()))?;
        let result = read_result_with_storage(storage, &request);
        self.state = match &result {
            Ok(artifact) => BuildState::Succeeded {
                build_id: request.build_id, revision_id: request.revision_id, artifact: artifact.clone(),
            },
            Err(error) => BuildState::Failed {
                build_id: request.build_id, revision_id: request.revision_id, message: error.to_string(),
            },
        };
        result
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        if let Some(request) = self.active.take() {
            self.state = BuildState::Failed {
                build_id: request.build_id, revision_id: request.revision_id, message: message.into(),
            };
        } else if let BuildState::Succeeded { build_id, revision_id, .. } = self.state {
            self.state = BuildState::Failed { build_id, revision_id, message: message.into() };
        }
    }

    /// Mark cancellation before asking the hypervisor to terminate the child.
    /// A late result can then never complete or install this job.
    pub fn cancel(&mut self) -> bool {
        let Some(request) = self.active.take() else { return false; };
        self.state = BuildState::Cancelled { build_id: request.build_id, revision_id: request.revision_id };
        true
    }
}

pub fn sha256(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }

#[cfg(feature = "std")]
pub fn validate_request(request: &BuildRequest) -> io::Result<()> {
    validate_request_data(request).map_err(|error| invalid(&error.0))
}
#[cfg(not(feature = "std"))]
pub fn validate_request(request: &BuildRequest) -> Result<(), InvalidRequest> {
    validate_request_data(request)
}

fn validate_request_data(request: &BuildRequest) -> Result<(), InvalidRequest> {
    if request.version != VERSION || request.workspace_epoch == 0 || request.build_id == 0
        || request.revision_id <= 0 || request.project.is_empty() || request.files.is_empty()
        || request.output_path.is_empty() || request.result_path.is_empty()
        || request.output_path == request.result_path
    {
        return Err(InvalidRequest("invalid compiler build request identity".into()));
    }
    let mut paths = BTreeSet::new();
    for file in &request.files {
        if !valid_source_path(&file.path) || !paths.insert(file.path.as_str()) {
            return Err(InvalidRequest("invalid or duplicate compiler source path".into()));
        }
    }
    if !paths.contains(request.entry.as_str()) { return Err(InvalidRequest("missing compiler entry".into())); }
    let output = WirePath::new(&request.output_path);
    let result = WirePath::new(&request.result_path);
    if output.extension().is_none_or(|ext| ext != "bp") || output.parent() != result.parent()
        || result.file_name().is_none_or(|name| name != "result.json")
    {
        return Err(InvalidRequest("compiler output and result must share the build directory".into()));
    }
    Ok(())
}

pub fn valid_source_path(path: &str) -> bool {
    !path.is_empty() && !path.starts_with('/') && !path.contains(['\\', '\0', ':'])
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

#[cfg(feature = "std")]
pub fn read_result(request: &BuildRequest) -> io::Result<BuildArtifact> {
    read_result_with_storage(&mut HostStorage, request).map_err(host_error)
}

pub fn read_result_with_storage<S: BuildStorage>(
    storage: &mut S, request: &BuildRequest,
) -> Result<BuildArtifact, BuildError<S::Error>> {
    let length = storage.len(&request.result_path).map_err(|error| {
        let message = format!(
            "Compiler exited without an accessible build result at {}: {error}. Compilation did not complete; check the compiler output for a crash or I/O failure.",
            request.result_path,
        );
        BuildError::StorageContext { source: error, message }
    })?;
    if length > MAX_WIRE_BYTES as u64 {
        return Err(BuildError::Invalid("compiler result exceeds 64 KiB".into()));
    }
    let result_bytes = storage.read(&request.result_path).map_err(BuildError::Storage)?;
    if result_bytes.len() > MAX_WIRE_BYTES {
        return Err(BuildError::Invalid("compiler result exceeds 64 KiB".into()));
    }
    let result: BuildResult = serde_json::from_slice(&result_bytes)
        .map_err(|error| BuildError::Invalid(error.to_string()))?;
    if result.version != VERSION || result.workspace_epoch != request.workspace_epoch
        || result.build_id != request.build_id || result.project != request.project
        || result.revision_id != request.revision_id
    {
        return Err(BuildError::Invalid("compiler result does not match the submitted snapshot".into()));
    }
    if !result.success {
        return Err(BuildError::Invalid(if result.diagnostics.is_empty() {
            "compiler failed without diagnostics".into()
        } else { result.diagnostics }));
    }
    let artifact = result.artifact
        .ok_or_else(|| BuildError::Invalid("successful compiler result has no artifact".into()))?;
    if artifact.path != request.output_path || artifact.bytes == 0 {
        return Err(BuildError::Invalid("compiler result names an unexpected artifact".into()));
    }
    if storage.len(&artifact.path).map_err(BuildError::Storage)? != artifact.bytes {
        return Err(BuildError::Invalid("compiled artifact size does not match result".into()));
    }
    let bytes = storage.read(&artifact.path).map_err(BuildError::Storage)?;
    if bytes.len() as u64 != artifact.bytes {
        return Err(BuildError::Invalid("compiled artifact size does not match result".into()));
    }
    if sha256(&bytes) != artifact.sha256 {
        return Err(BuildError::Invalid("compiled artifact hash does not match result".into()));
    }
    Ok(artifact)
}

#[cfg(feature = "std")]
struct HostStorage;
#[cfg(feature = "std")]
impl BuildStorage for HostStorage {
    type Error = io::Error;
    fn create_dir_all(&mut self, path: &str) -> io::Result<()> { fs::create_dir_all(path) }
    fn create_dir(&mut self, path: &str) -> io::Result<()> { fs::create_dir(path) }
    fn is_already_exists(&self, error: &io::Error) -> bool { error.kind() == io::ErrorKind::AlreadyExists }
    fn write(&mut self, path: &str, bytes: &[u8]) -> io::Result<()> { fs::write(path, bytes) }
    fn len(&mut self, path: &str) -> io::Result<u64> { fs::metadata(path).map(|metadata| metadata.len()) }
    fn read(&mut self, path: &str) -> io::Result<Vec<u8>> { fs::read(path) }
}
#[cfg(feature = "std")]
fn host_error(error: BuildError<io::Error>) -> io::Error {
    match error {
        BuildError::Storage(error) => error,
        BuildError::StorageContext { source, message } => io::Error::new(source.kind(), message),
        BuildError::Invalid(message) => invalid(&message),
        BuildError::Busy => io::Error::new(io::ErrorKind::WouldBlock, "a compiler job is already active"),
    }
}
#[cfg(feature = "std")]
fn invalid(message: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, message) }

#[cfg(all(test, feature = "store"))]
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
