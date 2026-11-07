//! The same workspace logic runs with alloc/serde and without the source DB.
use std::collections::{BTreeMap, BTreeSet};
use trueos_ide::{LoadedSource, ProjectBuildConfig, RevisionSummary, SourceFile, SourceFileInput, SourceOrigin};
use trueos_ide::protocol::{BuildArtifact, BuildRequest, BuildResult, BuildState, CompilerBackend, ProjectKind, VERSION};
use trueos_ide::workspace::{BuildError, BuildStorage, BuildTracker, sha256};

#[derive(Debug, PartialEq, Eq)]
enum Error { AlreadyExists, Missing, Denied }
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{self:?}") }
}
impl std::error::Error for Error {}

#[derive(Default)]
struct MemoryStorage {
    directories: BTreeSet<String>,
    files: BTreeMap<String, Vec<u8>>,
    fail_write: bool,
    reported_lengths: BTreeMap<String, u64>,
}
impl BuildStorage for MemoryStorage {
    type Error = Error;
    fn create_dir_all(&mut self, path: &str) -> Result<(), Error> {
        self.directories.insert(path.into());
        Ok(())
    }
    fn create_dir(&mut self, path: &str) -> Result<(), Error> {
        if self.directories.insert(path.into()) { Ok(()) } else { Err(Error::AlreadyExists) }
    }
    fn is_already_exists(&self, error: &Error) -> bool { *error == Error::AlreadyExists }
    fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), Error> {
        if self.fail_write { return Err(Error::Denied); }
        self.files.insert(path.into(), bytes.to_vec());
        Ok(())
    }
    fn len(&mut self, path: &str) -> Result<u64, Error> {
        self.reported_lengths.get(path).copied()
            .or_else(|| self.files.get(path).map(|bytes| bytes.len() as u64)).ok_or(Error::Missing)
    }
    fn read(&mut self, path: &str) -> Result<Vec<u8>, Error> {
        self.files.get(path).cloned().ok_or(Error::Missing)
    }
}

fn fixture() -> LoadedSource {
    LoadedSource {
        revision: RevisionSummary { id: 3, project: "Rust λ demo".into(), revision: 2,
            origin: SourceOrigin::Local { label: None }, file_count: 1, source_bytes: 6 },
        files: vec![SourceFile { path: "src/main.rs".into(), bytes: b"source".to_vec(), mode: 0o100644 }],
    }
}
fn prepare(tracker: &mut BuildTracker, storage: &mut MemoryStorage) -> BuildRequest {
    tracker.prepare_with_storage(storage, "/builds", &fixture(), CompilerBackend::RustcMin, "src/main.rs").unwrap().request
}
fn complete(storage: &mut MemoryStorage, request: &BuildRequest) -> BuildResult {
    let bytes = b"verified artifact";
    storage.write(&request.output_path, bytes).unwrap();
    let result = BuildResult { version: VERSION, workspace_epoch: request.workspace_epoch,
        build_id: request.build_id, project: request.project.clone(), revision_id: request.revision_id,
        success: true, artifact: Some(BuildArtifact { path: request.output_path.clone(),
            bytes: bytes.len() as u64, sha256: sha256(bytes) }), diagnostics: String::new() };
    storage.write(&request.result_path, &serde_json::to_vec(&result).unwrap()).unwrap();
    result
}

#[test]
fn source_models_and_store_protocol_need_no_database_feature() {
    let source = fixture();
    let decoded: LoadedSource = serde_json::from_slice(&serde_json::to_vec(&source).unwrap()).unwrap();
    assert_eq!(decoded, source);
    assert_eq!(decoded.file("src/main.rs").unwrap().bytes, b"source");
    assert!(decoded.file("missing.rs").is_none());
    let input = SourceFileInput::new("main.rs", b"main".to_vec());
    assert_eq!(input.mode, trueos_ide::DEFAULT_FILE_MODE);
    let request = trueos_ide::store_protocol::Request {
        database: trueos_ide::IDE_DATABASE_PATH.into(),
        command: trueos_ide::store_protocol::Command::SetConfig {
            project: "demo".into(), config: ProjectBuildConfig { kind: ProjectKind::Native,
                compiler: Some(CompilerBackend::RustcMin), entry: "src/main.rs".into() },
        },
    };
    let bytes = serde_json::to_vec(&request).unwrap();
    let decoded: trueos_ide::store_protocol::Request = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded.command.write_project(), Some("demo"));
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes);
}

#[test]
fn stages_an_immutable_snapshot_and_checks_result_identity_and_bytes() {
    let mut storage = MemoryStorage::default();
    let mut tracker = BuildTracker::new_owned(7, 21);
    let mut source = fixture();
    let prepared = tracker.prepare_with_storage(&mut storage, "/builds", &source,
        CompilerBackend::RustcMin, "src/main.rs").unwrap();
    assert_eq!(prepared.request_path.to_string_lossy_or_str(), "/builds/7-21/1/request.json");
    source.files[0].bytes.fill(b'!');
    let staged: BuildRequest = serde_json::from_slice(storage.files.get("/builds/7-21/1/request.json").unwrap()).unwrap();
    assert_eq!(staged.files[0].bytes, b"source");
    assert_eq!(tracker.active().unwrap().files[0].bytes, b"source");
    assert!(matches!(tracker.prepare_with_storage(&mut storage, "/builds", &source,
        CompilerBackend::RustcMin, "src/main.rs"), Err(BuildError::Busy)));
    complete(&mut storage, &prepared.request);
    assert!(tracker.finish_with_storage(&mut storage).is_ok());
    assert!(matches!(tracker.state, BuildState::Succeeded { .. }));
}

// Keep the test source common to std PathBuf and no_std String request paths.
trait PathText { fn to_string_lossy_or_str(&self) -> String; }
impl PathText for String { fn to_string_lossy_or_str(&self) -> String { self.clone() } }
impl PathText for std::path::PathBuf {
    fn to_string_lossy_or_str(&self) -> String { self.to_string_lossy().into_owned() }
}

#[test]
fn owner_epoch_and_build_id_reservations_never_reuse_persisted_snapshots() {
    let mut storage = MemoryStorage::default();
    let mut first = BuildTracker::new_owned(10, 21);
    let a = prepare(&mut first, &mut storage);
    let b = prepare(&mut BuildTracker::new_owned(10, 22), &mut storage);
    let restarted = prepare(&mut BuildTracker::new_owned(10, 21), &mut storage);
    assert_eq!((a.workspace_epoch, b.workspace_epoch, restarted.workspace_epoch), (10, 10, 11));
    assert_ne!(a.result_path, b.result_path);
    assert_ne!(a.result_path, restarted.result_path);
    assert!(first.cancel());
    let next = prepare(&mut first, &mut storage);
    assert_eq!(next.build_id, 2);
    assert_ne!(next.result_path, a.result_path);
    complete(&mut storage, &a);
    assert!(first.finish_with_storage(&mut storage).unwrap_err().to_string().contains("without an accessible build result"));
}

#[test]
fn cancellation_and_failed_staging_do_not_admit_late_results() {
    let mut storage = MemoryStorage::default();
    let mut tracker = BuildTracker::new(8);
    let request = prepare(&mut tracker, &mut storage);
    assert!(tracker.cancel());
    complete(&mut storage, &request);
    assert!(tracker.finish_with_storage(&mut storage).is_err());
    assert!(matches!(tracker.state, BuildState::Cancelled { .. }));
    storage.fail_write = true;
    assert!(matches!(tracker.prepare_with_storage(&mut storage, "/builds", &fixture(),
        CompilerBackend::RustcMin, "src/main.rs"), Err(BuildError::Storage(Error::Denied))));
    assert!(tracker.active().is_none());
    storage.fail_write = false;
    let request = prepare(&mut tracker, &mut storage);
    assert_eq!(request.build_id, 3);
}

#[test]
fn stale_results_wrong_artifacts_and_compiler_failures_keep_diagnostics() {
    for change in 0..8 {
        let mut storage = MemoryStorage::default();
        let mut tracker = BuildTracker::new(9);
        let request = prepare(&mut tracker, &mut storage);
        let mut result = complete(&mut storage, &request);
        match change {
            0 => result.workspace_epoch += 1,
            1 => result.build_id += 1,
            2 => result.revision_id += 1,
            3 => result.project.push('!'),
            4 => result.artifact.as_mut().unwrap().path.push('!'),
            5 => result.artifact.as_mut().unwrap().bytes += 1,
            6 => { storage.files.get_mut(&request.output_path).unwrap()[0] ^= 1; },
            7 => { result.success = false; result.diagnostics = "compiler failed at main.rs:7: λ".into(); },
            _ => unreachable!(),
        }
        storage.write(&request.result_path, &serde_json::to_vec(&result).unwrap()).unwrap();
        let message = tracker.finish_with_storage(&mut storage).unwrap_err().to_string();
        if change == 7 { assert_eq!(message, result.diagnostics); }
        assert!(matches!(tracker.state, BuildState::Failed { .. }));
    }
}

#[test]
fn result_size_guards_check_actual_reads_and_request_paths_remain_validated() {
    let mut storage = MemoryStorage::default();
    let mut tracker = BuildTracker::new(11);
    let request = prepare(&mut tracker, &mut storage);
    complete(&mut storage, &request);
    storage.files.insert(request.result_path.clone(), vec![b' '; 65 * 1024]);
    storage.reported_lengths.insert(request.result_path.clone(), 1);
    assert_eq!(tracker.finish_with_storage(&mut storage).unwrap_err().to_string(), "compiler result exceeds 64 KiB");
    for path in ["/absolute.rs", "../escape.rs", "a/../b.rs", "a//b.rs", "a\\b.rs", "vFile:a", "bad\0.rs"] {
        let mut source = fixture(); source.files[0].path = path.into();
        assert!(BuildTracker::new(1).prepare_with_storage(&mut MemoryStorage::default(),
            "/builds", &source, CompilerBackend::RustcMin, path).is_err());
    }
    let mut storage = MemoryStorage::default();
    storage.directories.insert(format!("/builds/{}", u64::MAX));
    assert_eq!(BuildTracker::new(u64::MAX).prepare_with_storage(&mut storage, "/builds",
        &fixture(), CompilerBackend::RustcMin, "src/main.rs").err().unwrap().to_string(), "workspace epoch exhausted");
}
