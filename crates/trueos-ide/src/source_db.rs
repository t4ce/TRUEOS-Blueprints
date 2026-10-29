//! Immutable source revisions, persisted in a crash-safe redb store.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const DEFAULT_DATABASE_PATH: &str = "rustc-sources.redb";
/// Shared source image for the IDE and other coordinated development tools.
pub const IDE_DATABASE_PATH: &str = "/apps/common/ide/projects.redb";
pub const DEFAULT_FILE_MODE: u32 = 0o100644;

const SCHEMA_VERSION: u64 = 1;
const MAX_PROJECT_NAME_BYTES: usize = 128;
const MAX_SOURCE_LABEL_BYTES: usize = 512;
const MAX_SOURCE_PATH_BYTES: usize = 4096;
const META: TableDefinition<&str, u64> = TableDefinition::new("meta");
const PROJECTS: TableDefinition<&str, &[u8]> = TableDefinition::new("projects");
const REVISIONS: TableDefinition<u64, &[u8]> = TableDefinition::new("revisions");
const GIT_REVISIONS: TableDefinition<&str, u64> = TableDefinition::new("git_revisions");
const FILES: TableDefinition<&str, &[u8]> = TableDefinition::new("files");
const BLOBS: TableDefinition<&str, &[u8]> = TableDefinition::new("blobs");
const SCHEMA: &str = "schema_version";
const NEXT_PROJECT: &str = "next_project_id";
const NEXT_REVISION: &str = "next_revision_id";

#[derive(Debug)]
pub enum SourceDbError {
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    Redb(String),
    Json(serde_json::Error),
    InvalidProjectName(String),
    InvalidSourceLabel(String),
    InvalidSourcePath(String),
    DuplicateSourcePath(String),
    EmptySourceSet,
    InvalidGitRepository(String),
    InvalidGitCommit(String),
    Integrity(String),
    UnsupportedSchemaVersion(u64),
    NumericRange(&'static str),
    StaleRevision {
        expected: i64,
        actual: Option<i64>,
    },
}
impl fmt::Display for SourceDbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                operation,
                path,
                source,
            } => write!(f, "{operation} {}: {source}", path.display()),
            Self::Redb(e) => write!(f, "redb: {e}"),
            Self::Json(e) => write!(f, "source database encoding: {e}"),
            Self::InvalidProjectName(e) => write!(f, "invalid project name: {e}"),
            Self::InvalidSourceLabel(e) => write!(f, "invalid source label: {e}"),
            Self::InvalidSourcePath(e) => write!(f, "invalid source path `{e}`"),
            Self::DuplicateSourcePath(e) => write!(f, "duplicate source path `{e}`"),
            Self::EmptySourceSet => write!(f, "source set contains no files"),
            Self::InvalidGitRepository(e) => write!(f, "invalid Git repository: {e}"),
            Self::InvalidGitCommit(e) => write!(f, "invalid Git commit: {e}"),
            Self::Integrity(e) => write!(f, "source database integrity: {e}"),
            Self::UnsupportedSchemaVersion(v) => {
                write!(f, "unsupported source database schema version {v}")
            }
            Self::NumericRange(e) => write!(f, "{e} is outside the supported range"),
            Self::StaleRevision { expected, actual } => write!(
                f,
                "stale source revision {expected}; current revision is {actual:?}"
            ),
        }
    }
}
impl std::error::Error for SourceDbError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}
impl From<serde_json::Error> for SourceDbError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
pub type Result<T> = std::result::Result<T, SourceDbError>;

#[derive(Clone, Debug, Eq, PartialEq)]
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
#[derive(Clone, Debug, Eq, PartialEq)]
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevisionSummary {
    pub id: i64,
    pub project: String,
    pub revision: u64,
    pub origin: SourceOrigin,
    pub file_count: u64,
    pub source_bytes: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectSummary {
    pub id: i64,
    pub name: String,
    pub revision_count: u64,
    pub latest_revision: Option<u64>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
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

#[derive(Serialize, Deserialize)]
struct Project {
    id: u64,
    next_revision: u64,
}
#[derive(Serialize, Deserialize)]
struct Revision {
    project: String,
    revision: u64,
    origin: SourceOrigin,
    file_count: u64,
    source_bytes: u64,
}
#[derive(Serialize, Deserialize)]
struct FileLink {
    mode: u32,
    digest: String,
}

pub struct SourceDatabase {
    path: PathBuf,
    database: Database,
}
impl SourceDatabase {
    pub fn open_default() -> Result<Self> {
        Self::open(DEFAULT_DATABASE_PATH)
    }
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let database = r(Database::create(&path))?;
        let result = Self { path, database };
        result.initialize()?;
        Ok(result)
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn verify(&self) -> Result<()> {
        let read = r(self.database.begin_read())?;
        let meta = r(read.open_table(META))?;
        if meta_value(&meta, SCHEMA)? != SCHEMA_VERSION {
            return Err(SourceDbError::Integrity(String::from(
                "schema version mismatch",
            )));
        }
        let projects = r(read.open_table(PROJECTS))?;
        let revisions = r(read.open_table(REVISIONS))?;
        let files = r(read.open_table(FILES))?;
        let blobs = r(read.open_table(BLOBS))?;
        let git = r(read.open_table(GIT_REVISIONS))?;
        let mut known = BTreeMap::new();
        for row in r(projects.iter())? {
            let (name, value) = r(row)?;
            let p: Project = decode(value.value())?;
            if p.id == 0 || p.next_revision == 0 {
                return bad("invalid stored project");
            }
            known.insert(name.value().to_owned(), p);
        }
        let mut indexed = BTreeSet::new();
        for row in r(revisions.iter())? {
            let (id, value) = r(row)?;
            let v: Revision = decode(value.value())?;
            let project = known.get(&v.project).ok_or_else(|| {
                SourceDbError::Integrity(format!("revision {} has no project", id.value()))
            })?;
            if v.revision == 0 || v.revision >= project.next_revision {
                return bad("invalid stored revision number");
            }
            let (count, bytes) = read_files(&files, &blobs, id.value())?
                .into_iter()
                .try_fold((0u64, 0u64), |(n, total), f| {
                    Ok::<_, SourceDbError>((
                        n.checked_add(1)
                            .ok_or(SourceDbError::NumericRange("file count"))?,
                        total
                            .checked_add(
                                u64::try_from(f.bytes.len())
                                    .map_err(|_| SourceDbError::NumericRange("source bytes"))?,
                            )
                            .ok_or(SourceDbError::NumericRange("source bytes"))?,
                    ))
                })?;
            if count != v.file_count || bytes != v.source_bytes {
                return bad("revision file summary does not match its path map");
            }
            if let SourceOrigin::Git { repository, commit } = &v.origin {
                let key = git_key(repository, commit);
                if r(git.get(key.as_str()))?.map(|x| x.value()) != Some(id.value()) {
                    return bad("Git revision is missing its identity index");
                }
                indexed.insert(key);
            }
        }
        for row in r(git.iter())? {
            let (key, id) = r(row)?;
            if !indexed.contains(key.value()) || r(revisions.get(id.value()))?.is_none() {
                return bad("stale Git identity index");
            }
        }
        for row in r(blobs.iter())? {
            let (digest, bytes) = r(row)?;
            if !is_digest(digest.value()) || sha256(bytes.value()) != digest.value() {
                return bad("stored source blob failed digest verification");
            }
        }
        Ok(())
    }
    pub fn find_git(&self, repository: &str, commit: &str) -> Result<Option<RevisionSummary>> {
        let repository = canonicalize_git_repository(repository)?;
        let commit = canonicalize_git_commit(commit)?;
        self.find_git_canonical(&repository, &commit)
    }
    pub fn import_git(
        &mut self,
        project: &str,
        repository: &str,
        commit: &str,
        files: Vec<SourceFileInput>,
    ) -> Result<ImportResult> {
        let project = normalize_project(project)?;
        let repository = canonicalize_git_repository(repository)?;
        let commit = canonicalize_git_commit(commit)?;
        if let Some(revision) = self.find_git_canonical(&repository, &commit)? {
            return Ok(ImportResult {
                disposition: ImportDisposition::AlreadyPresent,
                revision,
            });
        }
        let files = normalize_files(files)?;
        let write = r(self.database.begin_write())?;
        let key = git_key(&repository, &commit);
        {
            let index = r(write.open_table(GIT_REVISIONS))?;
            if let Some(id) = r(index.get(key.as_str()))? {
                return Ok(ImportResult {
                    disposition: ImportDisposition::AlreadyPresent,
                    revision: self.summary_in_write(&write, id.value())?,
                });
            }
        }
        let revision = self.insert(
            &write,
            &project,
            SourceOrigin::Git { repository, commit },
            &files,
        )?;
        let mut index = r(write.open_table(GIT_REVISIONS))?;
        r(index.insert(key.as_str(), revision.id as u64))?;
        drop(index);
        r(write.commit())?;
        Ok(ImportResult {
            disposition: ImportDisposition::Inserted,
            revision,
        })
    }
    pub fn import_local(
        &mut self,
        project: &str,
        label: Option<&str>,
        files: Vec<SourceFileInput>,
    ) -> Result<ImportResult> {
        let project = normalize_project(project)?;
        let label = normalize_label(label)?;
        let files = normalize_files(files)?;
        let write = r(self.database.begin_write())?;
        let revision = self.insert(&write, &project, SourceOrigin::Local { label }, &files)?;
        r(write.commit())?;
        Ok(ImportResult {
            disposition: ImportDisposition::Inserted,
            revision,
        })
    }
    /// Atomically save a complete new project snapshot if its latest revision
    /// still has the supplied globally unique ID. The old revision remains
    /// available, including when the new snapshot changes or removes files.
    pub fn save_files(
        &mut self,
        project: &str,
        expected_revision_id: i64,
        files: Vec<SourceFileInput>,
    ) -> Result<RevisionSummary> {
        let project = normalize_project(project)?;
        let files = normalize_files(files)?;
        let write = r(self.database.begin_write())?;
        let mut latest: Option<(u64, u64)> = None;
        {
            let revisions = r(write.open_table(REVISIONS))?;
            for row in r(revisions.iter())? {
                let (id, value) = r(row)?;
                let revision: Revision = decode(value.value())?;
                if revision.project == project
                    && latest.is_none_or(|(_, number)| revision.revision > number)
                {
                    latest = Some((id.value(), revision.revision));
                }
            }
        }
        let actual = latest
            .map(|(id, _)| as_i64(id, "revision id"))
            .transpose()?;
        if actual != Some(expected_revision_id) {
            return Err(SourceDbError::StaleRevision {
                expected: expected_revision_id,
                actual,
            });
        }
        let revision = self.insert(
            &write,
            &project,
            SourceOrigin::Local { label: None },
            &files,
        )?;
        r(write.commit())?;
        Ok(revision)
    }
    pub fn list_projects(&self) -> Result<Vec<ProjectSummary>> {
        let read = r(self.database.begin_read())?;
        let projects = r(read.open_table(PROJECTS))?;
        let revisions = r(read.open_table(REVISIONS))?;
        let mut counts: BTreeMap<String, (u64, u64)> = BTreeMap::new();
        for row in r(revisions.iter())? {
            let (_, value) = r(row)?;
            let v: Revision = decode(value.value())?;
            let x = counts.entry(v.project).or_default();
            x.0 += 1;
            x.1 = x.1.max(v.revision);
        }
        let mut result = Vec::new();
        for row in r(projects.iter())? {
            let (name, value) = r(row)?;
            let p: Project = decode(value.value())?;
            let (count, latest) = counts.remove(name.value()).unwrap_or_default();
            result.push(ProjectSummary {
                id: as_i64(p.id, "project id")?,
                name: name.value().to_owned(),
                revision_count: count,
                latest_revision: (latest != 0).then_some(latest),
            });
        }
        result.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(result)
    }
    pub fn list_revisions(&self, project: &str) -> Result<Vec<RevisionSummary>> {
        let project = normalize_project(project)?;
        let read = r(self.database.begin_read())?;
        let table = r(read.open_table(REVISIONS))?;
        let mut result = Vec::new();
        for row in r(table.iter())? {
            let (id, value) = r(row)?;
            let v: Revision = decode(value.value())?;
            if v.project == project {
                result.push(summary(id.value(), v)?);
            }
        }
        result.sort_by_key(|x| x.revision);
        Ok(result)
    }
    pub fn get(&self, project: &str, wanted: Option<u64>) -> Result<Option<LoadedSource>> {
        let project = normalize_project(project)?;
        let read = r(self.database.begin_read())?;
        let table = r(read.open_table(REVISIONS))?;
        let mut selected = None;
        for row in r(table.iter())? {
            let (id, value) = r(row)?;
            let v: Revision = decode(value.value())?;
            if v.project == project
                && wanted.is_none_or(|n| n == v.revision)
                && selected
                    .as_ref()
                    .is_none_or(|(_, old): &(u64, Revision)| v.revision > old.revision)
            {
                selected = Some((id.value(), v));
            }
        }
        selected.map(|(id, v)| self.load(&read, id, v)).transpose()
    }
    pub fn get_by_id(&self, id: i64) -> Result<Option<LoadedSource>> {
        let id = u64::try_from(id).map_err(|_| SourceDbError::NumericRange("revision id"))?;
        let read = r(self.database.begin_read())?;
        let table = r(read.open_table(REVISIONS))?;
        r(table.get(id))?
            .map(|x| self.load(&read, id, decode(x.value())?))
            .transpose()
    }
    pub fn delete_revision(&mut self, project: &str, number: u64) -> Result<bool> {
        let project = normalize_project(project)?;
        let write = r(self.database.begin_write())?;
        let table = r(write.open_table(REVISIONS))?;
        let mut target = None;
        for row in r(table.iter())? {
            let (id, value) = r(row)?;
            let v: Revision = decode(value.value())?;
            if v.project == project && v.revision == number {
                target = Some((id.value(), v));
                break;
            }
        }
        drop(table);
        let Some((id, v)) = target else {
            return Ok(false);
        };
        self.remove(&write, id, &v)?;
        r(write.commit())?;
        Ok(true)
    }
    pub fn delete_project(&mut self, project: &str) -> Result<bool> {
        let project = normalize_project(project)?;
        let write = r(self.database.begin_write())?;
        {
            let projects = r(write.open_table(PROJECTS))?;
            if r(projects.get(project.as_str()))?.is_none() {
                return Ok(false);
            }
        }
        let table = r(write.open_table(REVISIONS))?;
        let mut remove = Vec::new();
        for row in r(table.iter())? {
            let (id, value) = r(row)?;
            let v: Revision = decode(value.value())?;
            if v.project == project {
                remove.push((id.value(), v));
            }
        }
        drop(table);
        for (id, v) in remove {
            self.remove(&write, id, &v)?;
        }
        let mut projects = r(write.open_table(PROJECTS))?;
        r(projects.remove(project.as_str()))?;
        drop(projects);
        self.prune(&write)?;
        r(write.commit())?;
        Ok(true)
    }
    fn initialize(&self) -> Result<()> {
        let write = r(self.database.begin_write())?;
        let mut meta = r(write.open_table(META))?;
        let existing_schema = r(meta.get(SCHEMA))?.map(|value| value.value());
        match existing_schema {
            Some(v) if v > SCHEMA_VERSION => {
                return Err(SourceDbError::UnsupportedSchemaVersion(v));
            }
            Some(v) if v != SCHEMA_VERSION => return bad("unsupported source database schema"),
            Some(_) => {}
            None => {
                r(meta.insert(SCHEMA, SCHEMA_VERSION))?;
                r(meta.insert(NEXT_PROJECT, 1))?;
                r(meta.insert(NEXT_REVISION, 1))?;
            }
        }
        drop(meta);
        drop(r(write.open_table(PROJECTS))?);
        drop(r(write.open_table(REVISIONS))?);
        drop(r(write.open_table(GIT_REVISIONS))?);
        drop(r(write.open_table(FILES))?);
        drop(r(write.open_table(BLOBS))?);
        r(write.commit())?;
        Ok(())
    }
    fn find_git_canonical(
        &self,
        repository: &str,
        commit: &str,
    ) -> Result<Option<RevisionSummary>> {
        let read = r(self.database.begin_read())?;
        let index = r(read.open_table(GIT_REVISIONS))?;
        let key = git_key(repository, commit);
        let Some(id) = r(index.get(key.as_str()))?.map(|x| x.value()) else {
            return Ok(None);
        };
        let table = r(read.open_table(REVISIONS))?;
        let value = r(table.get(id))?.ok_or_else(|| {
            SourceDbError::Integrity(String::from(
                "Git identity index points to missing revision",
            ))
        })?;
        Ok(Some(summary(id, decode(value.value())?)?))
    }
    fn summary_in_write(&self, write: &redb::WriteTransaction, id: u64) -> Result<RevisionSummary> {
        let table = r(write.open_table(REVISIONS))?;
        let v = r(table.get(id))?.ok_or_else(|| {
            SourceDbError::Integrity(String::from(
                "Git identity index points to missing revision",
            ))
        })?;
        summary(id, decode(v.value())?)
    }
    fn insert(
        &self,
        write: &redb::WriteTransaction,
        project: &str,
        origin: SourceOrigin,
        files: &[SourceFileInput],
    ) -> Result<RevisionSummary> {
        let (project_id, revision) = self.next_project_revision(write, project)?;
        let _ = project_id;
        let id = self.next_id(write, NEXT_REVISION, "revision id")?;
        let bytes = files.iter().try_fold(0u64, |n, f| {
            n.checked_add(
                u64::try_from(f.bytes.len())
                    .map_err(|_| SourceDbError::NumericRange("source bytes"))?,
            )
            .ok_or(SourceDbError::NumericRange("source bytes"))
        })?;
        {
            let mut blobs = r(write.open_table(BLOBS))?;
            let mut links = r(write.open_table(FILES))?;
            for file in files {
                let digest = sha256(&file.bytes);
                let existing = r(blobs.get(digest.as_str()))?.map(|value| value.value().to_vec());
                match existing {
                    Some(old) if old != file.bytes => return bad("source blob digest collision"),
                    Some(_) => {}
                    None => {
                        r(blobs.insert(digest.as_str(), file.bytes.as_slice()))?;
                    }
                };
                let key = file_key(id, &file.path);
                let link = serde_json::to_vec(&FileLink {
                    mode: file.mode,
                    digest,
                })?;
                r(links.insert(key.as_str(), link.as_slice()))?;
            }
        }
        let v = Revision {
            project: project.to_owned(),
            revision,
            origin,
            file_count: u64::try_from(files.len())
                .map_err(|_| SourceDbError::NumericRange("file count"))?,
            source_bytes: bytes,
        };
        let encoded = serde_json::to_vec(&v)?;
        let mut revisions = r(write.open_table(REVISIONS))?;
        r(revisions.insert(id, encoded.as_slice()))?;
        summary(id, v)
    }
    fn next_project_revision(
        &self,
        write: &redb::WriteTransaction,
        name: &str,
    ) -> Result<(u64, u64)> {
        let mut table = r(write.open_table(PROJECTS))?;
        let mut p = match r(table.get(name))? {
            Some(v) => decode(v.value())?,
            None => Project {
                id: self.next_id(write, NEXT_PROJECT, "project id")?,
                next_revision: 1,
            },
        };
        if p.next_revision == 0 {
            return bad("project revision counter is invalid");
        }
        let revision = p.next_revision;
        p.next_revision = p
            .next_revision
            .checked_add(1)
            .ok_or(SourceDbError::NumericRange("revision"))?;
        let encoded = serde_json::to_vec(&p)?;
        r(table.insert(name, encoded.as_slice()))?;
        Ok((p.id, revision))
    }
    fn next_id(
        &self,
        write: &redb::WriteTransaction,
        key: &'static str,
        field: &'static str,
    ) -> Result<u64> {
        let mut meta = r(write.open_table(META))?;
        let id = meta_value(&meta, key)?;
        if id == 0 {
            return bad("identifier counter is invalid");
        }
        r(meta.insert(
            key,
            id.checked_add(1)
                .ok_or(SourceDbError::NumericRange(field))?,
        ))?;
        Ok(id)
    }
    fn load(&self, read: &redb::ReadTransaction, id: u64, v: Revision) -> Result<LoadedSource> {
        let summary = summary(id, v)?;
        let files = r(read.open_table(FILES))?;
        let blobs = r(read.open_table(BLOBS))?;
        let files = read_files(&files, &blobs, id)?;
        let bytes = files.iter().try_fold(0u64, |n, f| {
            n.checked_add(
                u64::try_from(f.bytes.len())
                    .map_err(|_| SourceDbError::NumericRange("source bytes"))?,
            )
            .ok_or(SourceDbError::NumericRange("source bytes"))
        })?;
        if u64::try_from(files.len()).ok() != Some(summary.file_count)
            || bytes != summary.source_bytes
        {
            return bad("revision file summary does not match its path map");
        }
        Ok(LoadedSource {
            revision: summary,
            files,
        })
    }
    fn remove(&self, write: &redb::WriteTransaction, id: u64, v: &Revision) -> Result<()> {
        let prefix = file_prefix(id);
        let links = r(write.open_table(FILES))?;
        let keys = keys_with_prefix(&links, &prefix)?;
        drop(links);
        let mut links = r(write.open_table(FILES))?;
        for key in keys {
            r(links.remove(key.as_str()))?;
        }
        drop(links);
        let mut revisions = r(write.open_table(REVISIONS))?;
        r(revisions.remove(id))?;
        drop(revisions);
        if let SourceOrigin::Git { repository, commit } = &v.origin {
            let mut index = r(write.open_table(GIT_REVISIONS))?;
            let key = git_key(repository, commit);
            r(index.remove(key.as_str()))?;
        }
        self.prune(write)
    }
    fn prune(&self, write: &redb::WriteTransaction) -> Result<()> {
        let links = r(write.open_table(FILES))?;
        let mut live = BTreeSet::new();
        for row in r(links.iter())? {
            let (_, v) = r(row)?;
            let link: FileLink = decode(v.value())?;
            live.insert(link.digest);
        }
        drop(links);
        let blobs = r(write.open_table(BLOBS))?;
        let mut stale = Vec::new();
        for row in r(blobs.iter())? {
            let (k, _) = r(row)?;
            if !live.contains(k.value()) {
                stale.push(k.value().to_owned());
            }
        }
        drop(blobs);
        let mut blobs = r(write.open_table(BLOBS))?;
        for digest in stale {
            r(blobs.remove(digest.as_str()))?;
        }
        Ok(())
    }
}

pub fn canonicalize_git_repository(repository: &str) -> Result<String> {
    let repository = repository.trim();
    if repository.is_empty() || repository.bytes().any(|b| b.is_ascii_whitespace()) {
        return Err(SourceDbError::InvalidGitRepository(String::from(
            "URL is empty or contains whitespace",
        )));
    }
    if repository.contains('?') || repository.contains('#') {
        return Err(SourceDbError::InvalidGitRepository(String::from(
            "repository URL must not contain a query or fragment",
        )));
    }
    let (scheme, rest) = repository.split_once("://").ok_or_else(|| {
        SourceDbError::InvalidGitRepository(String::from("expected an HTTPS URL"))
    })?;
    if !scheme.eq_ignore_ascii_case("https") {
        return Err(SourceDbError::InvalidGitRepository(String::from(
            "only HTTPS repository URLs are supported",
        )));
    }
    let (host, path) = rest.split_once('/').ok_or_else(|| {
        SourceDbError::InvalidGitRepository(String::from("repository path is missing"))
    })?;
    if host.is_empty() || host.contains('@') || host.contains('\\') {
        return Err(SourceDbError::InvalidGitRepository(String::from(
            "repository host is invalid",
        )));
    }
    let mut parts: Vec<&str> = Vec::new();
    for x in path.split('/') {
        if x.is_empty() || x == "." {
            continue;
        }
        if x == ".." || x.contains('\\') {
            return Err(SourceDbError::InvalidGitRepository(String::from(
                "repository path is invalid",
            )));
        }
        parts.push(x);
    }
    let last = parts.last_mut().ok_or_else(|| {
        SourceDbError::InvalidGitRepository(String::from("repository path is missing"))
    })?;
    if last.to_ascii_lowercase().ends_with(".git") {
        *last = &last[..last.len() - 4];
    }
    if last.is_empty() {
        return Err(SourceDbError::InvalidGitRepository(String::from(
            "repository name is missing",
        )));
    }
    Ok(format!(
        "https://{}/{}",
        host.to_ascii_lowercase(),
        parts.join("/")
    ))
}
pub fn canonicalize_git_commit(commit: &str) -> Result<String> {
    let commit = commit.trim();
    if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(SourceDbError::InvalidGitCommit(String::from(
            "expected a full 40- or 64-character hexadecimal object ID",
        )));
    }
    Ok(commit.to_ascii_lowercase())
}
fn normalize_project(value: &str) -> Result<String> {
    let v = value.trim();
    if v.is_empty() || v.len() > MAX_PROJECT_NAME_BYTES || v.chars().any(char::is_control) {
        return Err(SourceDbError::InvalidProjectName(format!(
            "expected 1-{MAX_PROJECT_NAME_BYTES} bytes without control characters"
        )));
    }
    Ok(v.to_owned())
}
fn normalize_label(value: Option<&str>) -> Result<Option<String>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > MAX_SOURCE_LABEL_BYTES || value.chars().any(char::is_control) {
        return Err(SourceDbError::InvalidSourceLabel(format!(
            "expected at most {MAX_SOURCE_LABEL_BYTES} bytes without control characters"
        )));
    }
    Ok(Some(value.to_owned()))
}
fn normalize_files(mut files: Vec<SourceFileInput>) -> Result<Vec<SourceFileInput>> {
    if files.is_empty() {
        return Err(SourceDbError::EmptySourceSet);
    }
    for file in &mut files {
        file.path = normalize_path(&file.path)?;
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    for pair in files.windows(2) {
        if pair[0].path == pair[1].path {
            return Err(SourceDbError::DuplicateSourcePath(pair[0].path.clone()));
        }
    }
    Ok(files)
}
fn normalize_path(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > MAX_SOURCE_PATH_BYTES
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || value
            .split('/')
            .any(|x| x.is_empty() || x == "." || x == "..")
    {
        return Err(SourceDbError::InvalidSourcePath(value.to_owned()));
    }
    Ok(value.to_owned())
}
fn r<T, E: fmt::Display>(v: std::result::Result<T, E>) -> Result<T> {
    v.map_err(|e| SourceDbError::Redb(e.to_string()))
}
fn bad<T>(message: &str) -> Result<T> {
    Err(SourceDbError::Integrity(message.to_owned()))
}
fn decode<T: for<'a> Deserialize<'a>>(bytes: &[u8]) -> Result<T> {
    Ok(serde_json::from_slice(bytes)?)
}
fn meta_value(table: &impl ReadableTable<&'static str, u64>, key: &'static str) -> Result<u64> {
    r(table.get(key))?
        .map(|v| v.value())
        .ok_or_else(|| SourceDbError::Integrity(format!("missing metadata key {key}")))
}
fn summary(id: u64, v: Revision) -> Result<RevisionSummary> {
    Ok(RevisionSummary {
        id: as_i64(id, "revision id")?,
        project: v.project,
        revision: v.revision,
        origin: v.origin,
        file_count: v.file_count,
        source_bytes: v.source_bytes,
    })
}
fn as_i64(n: u64, field: &'static str) -> Result<i64> {
    i64::try_from(n).map_err(|_| SourceDbError::NumericRange(field))
}
fn file_prefix(id: u64) -> String {
    format!("{id:016x}\0")
}
fn file_key(id: u64, path: &str) -> String {
    format!("{}{path}", file_prefix(id))
}
fn git_key(repo: &str, commit: &str) -> String {
    format!("{repo}\0{commit}")
}
fn keys_with_prefix(
    table: &impl ReadableTable<&'static str, &'static [u8]>,
    prefix: &str,
) -> Result<Vec<String>> {
    let mut keys = Vec::new();
    for row in r(table.range(prefix..))? {
        let (key, _) = r(row)?;
        if !key.value().starts_with(prefix) {
            break;
        }
        keys.push(key.value().to_owned());
    }
    Ok(keys)
}
fn read_files(
    files: &impl ReadableTable<&'static str, &'static [u8]>,
    blobs: &impl ReadableTable<&'static str, &'static [u8]>,
    id: u64,
) -> Result<Vec<SourceFile>> {
    let prefix = file_prefix(id);
    let mut result = Vec::new();
    for row in r(files.range(prefix.as_str()..))? {
        let (key, value) = r(row)?;
        let Some(path) = key.value().strip_prefix(&prefix) else {
            break;
        };
        let path = normalize_path(path).map_err(|_| {
            SourceDbError::Integrity(format!("stored source path `{path}` is invalid"))
        })?;
        let link: FileLink = decode(value.value())?;
        if !is_digest(&link.digest) {
            return bad("stored source digest is invalid");
        }
        let bytes = r(blobs.get(link.digest.as_str()))?.ok_or_else(|| {
            SourceDbError::Integrity(format!("source file `{path}` references a missing blob"))
        })?;
        if sha256(bytes.value()) != link.digest {
            return bad("stored source blob failed digest verification");
        }
        result.push(SourceFile {
            path,
            bytes: bytes.value().to_vec(),
            mode: link.mode,
        });
    }
    Ok(result)
}
fn sha256(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for b in Sha256::digest(bytes) {
        out.push(H[usize::from(b >> 4)] as char);
        out.push(H[usize::from(b & 15)] as char);
    }
    out
}
fn is_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use redb::ReadableTableMetadata;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "trueos-rustc-redb-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn path(&self) -> PathBuf {
            self.0.join("sources.redb")
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn file(p: &str, b: &[u8]) -> SourceFileInput {
        SourceFileInput::new(p, b)
    }
    fn commit(b: u8) -> String {
        format!("{b:02x}").repeat(20)
    }

    fn blob_count(db: &SourceDatabase) -> u64 {
        let read = db.database.begin_read().unwrap();
        read.open_table(BLOBS).unwrap().len().unwrap()
    }

    #[test]
    fn editor_save_is_persistent_and_rejects_stale_snapshots() {
        let t = Temp::new();
        let mut db = SourceDatabase::open(t.path()).unwrap();
        let first = db
            .import_local(
                "app",
                None,
                vec![file("src/main.rs", b"old"), file("asset", b"binary")],
            )
            .unwrap()
            .revision;
        let snapshot = db.get("app", None).unwrap().unwrap();
        let saved = db
            .save_files(
                "app",
                snapshot.revision.id,
                vec![file("src/main.rs", b"new")],
            )
            .unwrap();
        assert!(
            matches!(db.save_files("app", snapshot.revision.id, vec![file("src/main.rs", b"lost")]), Err(SourceDbError::StaleRevision { expected, actual: Some(actual) }) if expected == first.id && actual == saved.id)
        );
        assert_eq!(
            db.get_by_id(first.id)
                .unwrap()
                .unwrap()
                .file("asset")
                .unwrap()
                .bytes,
            b"binary"
        );
        drop(db);
        let db = SourceDatabase::open(t.path()).unwrap();
        let current = db.get("app", None).unwrap().unwrap();
        assert_eq!(current.revision.id, saved.id);
        assert_eq!(current.file("src/main.rs").unwrap().bytes, b"new");
        assert!(current.file("asset").is_none());
        db.verify().unwrap();
    }

    #[test]
    fn editor_save_rejects_deleted_and_recreated_project() {
        let t = Temp::new();
        let mut db = SourceDatabase::open(t.path()).unwrap();
        let first = db
            .import_local("app", None, vec![file("main.rs", b"old")])
            .unwrap()
            .revision;
        db.delete_project("app").unwrap();
        assert!(matches!(
            db.save_files("app", first.id, vec![file("main.rs", b"old")]),
            Err(SourceDbError::StaleRevision { actual: None, .. })
        ));
        let replacement = db
            .import_local("app", None, vec![file("main.rs", b"replacement")])
            .unwrap()
            .revision;
        assert_eq!(replacement.revision, first.revision);
        assert!(
            matches!(db.save_files("app", first.id, vec![file("main.rs", b"old")]), Err(SourceDbError::StaleRevision { actual: Some(id), .. }) if id == replacement.id)
        );
    }

    #[test]
    fn canonicalizes_git_identity_and_requires_exact_commit() {
        assert_eq!(
            canonicalize_git_repository(" HTTPS://GitHub.COM/TrueOS/Example.GIT/ ").unwrap(),
            "https://github.com/TrueOS/Example"
        );
        assert_eq!(
            canonicalize_git_commit(commit(0xab).to_ascii_uppercase().as_str()).unwrap(),
            commit(0xab)
        );
        assert!(canonicalize_git_repository("http://github.com/a/b").is_err());
        assert!(canonicalize_git_commit("deadbeef").is_err());
    }

    #[test]
    fn git_is_idempotent() {
        let t = Temp::new();
        let mut db = SourceDatabase::open(t.path()).unwrap();
        let first = db
            .import_git(
                "demo",
                "https://github.com/TRUEOS/demo.git",
                &commit(1),
                vec![
                    file("Cargo.toml", b"[package]\nname='demo'\n"),
                    SourceFileInput::with_mode("src/main.rs", b"one", 0o100755),
                ],
            )
            .unwrap();
        let second = db
            .import_git(
                "ignored",
                "HTTPS://GITHUB.COM/TRUEOS/demo/",
                &commit(1),
                vec![file("changed", b"two")],
            )
            .unwrap();
        assert_eq!(second.disposition, ImportDisposition::AlreadyPresent);
        assert_eq!(first.revision.id, second.revision.id);
        assert_eq!(
            db.get_by_id(first.revision.id)
                .unwrap()
                .unwrap()
                .file("src/main.rs")
                .unwrap()
                .bytes,
            b"one"
        );
        assert_eq!(
            db.get_by_id(first.revision.id)
                .unwrap()
                .unwrap()
                .file("src/main.rs")
                .unwrap()
                .mode,
            0o100755
        );
    }
    #[test]
    fn local_revisions_deduplicate_blobs_and_keep_path_maps() {
        let t = Temp::new();
        let mut db = SourceDatabase::open(t.path()).unwrap();
        let common = b"same exact bytes";
        let first = db
            .import_local(
                "app",
                Some("first.7z"),
                vec![file("src/main.rs", common), file("Cargo.toml", b"one")],
            )
            .unwrap();
        let second = db
            .import_local(
                "app",
                Some("second.7z"),
                vec![file("src/lib.rs", common), file("Cargo.toml", b"two")],
            )
            .unwrap();
        assert_eq!(first.revision.revision, 1);
        assert_eq!(second.revision.revision, 2);
        assert_eq!(blob_count(&db), 3);
        assert_eq!(
            db.get("app", Some(1))
                .unwrap()
                .unwrap()
                .file("src/main.rs")
                .unwrap()
                .bytes,
            common
        );
        assert_eq!(
            db.get("app", None)
                .unwrap()
                .unwrap()
                .file("src/lib.rs")
                .unwrap()
                .bytes,
            common
        );
    }
    #[test]
    fn deletion_prunes_orphans_and_does_not_reuse_revision_numbers() {
        let t = Temp::new();
        let mut db = SourceDatabase::open(t.path()).unwrap();
        db.import_local("app", None, vec![file("one", b"shared")])
            .unwrap();
        db.import_local("app", None, vec![file("two", b"shared")])
            .unwrap();
        assert!(db.delete_revision("app", 1).unwrap());
        assert_eq!(blob_count(&db), 1);
        let third = db
            .import_local("app", None, vec![file("three", b"new")])
            .unwrap();
        assert_eq!(third.revision.revision, 3);
        assert!(db.delete_project("app").unwrap());
        assert_eq!(blob_count(&db), 0);
        db.verify().unwrap();
    }
    #[test]
    fn persists_reopens_and_rejects_corrupt_images() {
        let t = Temp::new();
        let path = t.path();
        {
            let mut db = SourceDatabase::open(&path).unwrap();
            db.import_local("ok", Some("input.7z"), vec![file("src/main.rs", b"ok")])
                .unwrap();
            db.verify().unwrap();
        }
        assert_eq!(
            SourceDatabase::open(&path)
                .unwrap()
                .get("ok", None)
                .unwrap()
                .unwrap()
                .files[0]
                .bytes,
            b"ok"
        );
        std::fs::write(&path, b"not redb").unwrap();
        assert!(SourceDatabase::open(&path).is_err());
    }

    #[test]
    fn rejects_unsafe_or_duplicate_paths_without_persisting() {
        let t = Temp::new();
        let path = t.path();
        let mut db = SourceDatabase::open(&path).unwrap();
        assert!(
            db.import_local("bad", None, vec![file("../escape", b"x")])
                .is_err()
        );
        assert!(
            db.import_local(
                "bad",
                None,
                vec![file("src/main.rs", b"a"), file("src/main.rs", b"b")],
            )
            .is_err()
        );
        assert!(db.list_projects().unwrap().is_empty());
        assert_eq!(blob_count(&db), 0);
        assert!(path.exists());
        db.verify().unwrap();
    }
}
