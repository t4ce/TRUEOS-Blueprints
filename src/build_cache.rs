//! Content-based freshness for complete Blueprint artifacts.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: u32 = 1;
const IGNORED_DIRECTORIES: &[&str] = &["target", "dist", ".git", "node_modules", "__pycache__"];
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct FileState {
    length: u64,
    modified: (i64, u32),
    changed: (i64, u32),
    device: u64,
    inode: u64,
    mode: u32,
}

impl FileState {
    fn read(metadata: &Metadata) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Self {
                length: metadata.len(),
                modified: (metadata.mtime(), metadata.mtime_nsec() as u32),
                changed: (metadata.ctime(), metadata.ctime_nsec() as u32),
                device: metadata.dev(),
                inode: metadata.ino(),
                mode: metadata.mode(),
            }
        }
        #[cfg(not(unix))]
        {
            let modified = timestamp(metadata.modified().unwrap_or(UNIX_EPOCH));
            Self {
                length: metadata.len(),
                modified,
                changed: modified,
                device: 0,
                inode: 0,
                mode: u32::from(metadata.permissions().readonly()),
            }
        }
    }

    fn existed_at(&self, started: (i64, u32)) -> bool {
        self.modified <= started && self.changed <= started
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Entry {
    kind: String,
    digest: String,
    state: Option<FileState>,
}

#[derive(Clone, Deserialize, Serialize)]
struct Output {
    path: PathBuf,
    digest: String,
    state: FileState,
}

#[derive(Clone, Deserialize, Serialize)]
struct Stamp {
    version: u32,
    key_hash: String,
    environment_hash: String,
    roots: Vec<PathBuf>,
    input_digest: String,
    inputs: BTreeMap<String, Entry>,
    outputs: Vec<Output>,
}

struct Snapshot {
    digest: String,
    entries: BTreeMap<String, Entry>,
    stable: bool,
}

struct Watched {
    snapshot: Snapshot,
    started: (i64, u32),
    valid: bool,
}

pub(crate) struct BuildCache {
    stamp_path: PathBuf,
    key_hash: String,
    environment_hash: String,
    base_roots: Vec<PathBuf>,
    roots: Vec<PathBuf>,
    previous: Option<Stamp>,
    memo: BTreeMap<String, Entry>,
    watched: Option<Watched>,
}

impl BuildCache {
    pub(crate) fn new(
        stamp: PathBuf,
        key: String,
        roots: Vec<PathBuf>,
        environment: String,
    ) -> Result<Self, String> {
        // Store only digests of caller-provided key/environment strings. The
        // latter may contain credentials inherited by native build scripts.
        let key_hash = hash_bytes(key.as_bytes());
        let environment_hash = hash_bytes(environment.as_bytes());
        let previous = fs::read(&stamp)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Stamp>(&bytes).ok())
            .filter(|stamp| stamp.version == SCHEMA_VERSION);
        let base_roots = absolute_roots(roots)?;
        let mut roots = base_roots.clone();
        if let Some(previous) = &previous
            && previous.key_hash == key_hash
            && previous.environment_hash == environment_hash
        {
            roots.extend(previous.roots.iter().cloned());
            deduplicate(&mut roots);
        }
        let memo = previous
            .as_ref()
            .map(|stamp| stamp.inputs.clone())
            .unwrap_or_default();
        Ok(Self {
            stamp_path: stamp,
            key_hash,
            environment_hash,
            base_roots,
            roots,
            previous,
            memo,
            watched: None,
        })
    }

    pub(crate) fn is_fresh(&mut self) -> Result<bool, String> {
        let Some(previous) = &self.previous else {
            return Ok(false);
        };
        if previous.key_hash != self.key_hash
            || previous.environment_hash != self.environment_hash
            || previous.outputs.is_empty()
        {
            return Ok(false);
        }
        let snapshot = snapshot(&self.roots, &self.memo)?;
        let fresh_inputs = snapshot.stable && snapshot.digest == previous.input_digest;
        self.memo = snapshot.entries;
        if !fresh_inputs {
            return Ok(false);
        }
        for output in &previous.outputs {
            let Some(current) = read_output(&output.path, Some(output))? else {
                return Ok(false);
            };
            if current.digest != output.digest {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Start before staging, then add resolved dependencies before compiling.
    /// Previously watched inputs must remain unchanged between those phases.
    pub(crate) fn watch(&mut self, roots: Vec<PathBuf>) -> Result<(), String> {
        let mut valid = true;
        let previous_roots = self.roots.clone();
        if let Some(watched) = &self.watched {
            let current = snapshot(&self.roots, &self.memo)?;
            valid = watched.valid && unchanged_since_watch(&watched.snapshot, &current);
            self.memo = current.entries;
        }
        self.roots.extend(absolute_roots(roots)?);
        deduplicate(&mut self.roots);
        let started = timestamp(SystemTime::now());
        let captured = snapshot(&self.roots, &self.memo)?;
        valid &= captured.stable;
        if let Some(watched) = &self.watched {
            // Adding new roots must not reset the baseline of an earlier
            // source watch, even if an edit happens during dependency scanning.
            let rechecked = snapshot(&previous_roots, &captured.entries)?;
            valid &= unchanged_since_watch(&watched.snapshot, &rechecked);
        }
        self.memo = captured.entries.clone();
        self.watched = Some(Watched {
            snapshot: captured,
            started,
            valid,
        });
        Ok(())
    }

    /// Remove a previous success before a new build can replace its outputs.
    pub(crate) fn invalidate(&mut self) -> Result<(), String> {
        match fs::remove_file(&self.stamp_path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_string(error)),
        }
        self.previous = None;
        if self.watched.is_none() {
            // A changed manifest may have removed dependencies from the graph.
            // Keep prior roots for freshness checks, then rediscover the new
            // graph when starting a replacement build.
            self.roots = self.base_roots.clone();
        }
        Ok(())
    }

    /// Return false when edits during a successful build prevent safe reuse.
    pub(crate) fn record(
        &mut self,
        outputs: Vec<PathBuf>,
        extra_inputs: Vec<PathBuf>,
    ) -> Result<bool, String> {
        let watched = self
            .watched
            .as_ref()
            .ok_or("Blueprint cache inputs must be watched before building")?;
        if !watched.valid || outputs.is_empty() {
            return Ok(false);
        }
        let known = snapshot(&self.roots, &self.memo)?;
        if !unchanged_since_watch(&watched.snapshot, &known) {
            return Ok(false);
        }
        let mut roots = self.roots.clone();
        roots.extend(absolute_roots(extra_inputs)?);
        deduplicate(&mut roots);
        let complete = snapshot(&roots, &known.entries)?;
        if !complete.stable {
            return Ok(false);
        }
        // A dependency first learned from Cargo's dep-info must already have
        // existed at the final precompile watch. ctime catches edits whose
        // mtime was preserved or deliberately backdated.
        for (path, entry) in &complete.entries {
            if !known.entries.contains_key(path)
                && entry.kind != "directory"
                && let Some(state) = &entry.state
                && !state.existed_at(watched.started)
            {
                return Ok(false);
            }
        }
        let mut recorded_outputs = Vec::new();
        for path in absolute_roots(outputs)? {
            let Some(output) = read_output(&path, None)? else {
                return Ok(false);
            };
            recorded_outputs.push(output);
        }
        // Hashing large outputs can take time; validate the known inputs once
        // more before publishing the success stamp.
        let known_verified = snapshot(&self.roots, &complete.entries)?;
        if !unchanged_since_watch(&watched.snapshot, &known_verified) {
            return Ok(false);
        }
        let verified = snapshot(&roots, &complete.entries)?;
        if !verified.stable || verified.digest != complete.digest {
            return Ok(false);
        }
        let stamp = Stamp {
            version: SCHEMA_VERSION,
            key_hash: self.key_hash.clone(),
            environment_hash: self.environment_hash.clone(),
            roots,
            input_digest: verified.digest,
            inputs: verified.entries,
            outputs: recorded_outputs,
        };
        write_stamp(&self.stamp_path, &stamp)?;
        self.memo = stamp.inputs.clone();
        self.previous = Some(stamp);
        Ok(true)
    }
}

fn absolute_roots(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>, String> {
    let cwd = std::env::current_dir().map_err(io_string)?;
    let mut roots = paths
        .into_iter()
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            }
        })
        .collect::<Vec<_>>();
    deduplicate(&mut roots);
    Ok(roots)
}

fn deduplicate(paths: &mut Vec<PathBuf>) {
    paths.sort();
    paths.dedup();
}

fn unchanged_since_watch(before: &Snapshot, after: &Snapshot) -> bool {
    before.stable
        && after.stable
        && before.digest == after.digest
        && before.entries.iter().all(|(path, entry)| {
            // ctime/inode checks also catch edits whose original bytes and
            // mtime were restored before the build completed. Directory
            // timestamps can change when an ignored target/dist is created.
            entry.kind == "directory"
                || after
                    .entries
                    .get(path)
                    .is_some_and(|current| current.state == entry.state)
        })
}

fn snapshot(roots: &[PathBuf], memo: &BTreeMap<String, Entry>) -> Result<Snapshot, String> {
    let mut walker = Walker {
        entries: BTreeMap::new(),
        visited_directories: BTreeSet::new(),
        memo,
        stable: true,
    };
    for root in roots {
        walker.visit(root, true).map_err(|error| {
            format!(
                "cannot inspect Blueprint cache input {}: {error}",
                root.display()
            )
        })?;
    }
    let mut hash = Sha256::new();
    for (path, entry) in &walker.entries {
        hash_field(&mut hash, path.as_bytes());
        hash_field(&mut hash, entry.kind.as_bytes());
        hash_field(&mut hash, entry.digest.as_bytes());
        // File permissions can change which scripts or tools are executable.
        hash.update(
            entry
                .state
                .as_ref()
                .map_or(0, |state| state.mode)
                .to_le_bytes(),
        );
    }
    Ok(Snapshot {
        digest: format!("{:x}", hash.finalize()),
        entries: walker.entries,
        stable: walker.stable,
    })
}

struct Walker<'a> {
    entries: BTreeMap<String, Entry>,
    visited_directories: BTreeSet<PathBuf>,
    memo: &'a BTreeMap<String, Entry>,
    stable: bool,
}

impl Walker<'_> {
    fn visit(&mut self, path: &Path, explicit: bool) -> io::Result<()> {
        // Generated directories may themselves be symlinks to a shared cache.
        if !explicit
            && path
                .file_name()
                .is_some_and(|name| IGNORED_DIRECTORIES.iter().any(|ignored| name == *ignored))
        {
            return Ok(());
        }
        let key = path_key(path);
        if self.entries.contains_key(&key) {
            return Ok(());
        }
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.entries
                    .insert(key, entry("missing", String::new(), None));
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        let state = FileState::read(&metadata);
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(path)?;
            self.entries.insert(
                key.clone(),
                entry(
                    "symlink",
                    hash_bytes(&path_bytes(&target)),
                    Some(state.clone()),
                ),
            );
            match fs::canonicalize(path) {
                Ok(target) => self.visit(&target, true)?,
                Err(error) => {
                    // Broken links and link cycles are stable identities, and
                    // become different snapshots when a target appears.
                    self.entries.insert(
                        format!("{key}:unresolved-target"),
                        entry("unresolved", format!("{:?}", error.kind()), None),
                    );
                }
            }
            self.verify_state(path, &state);
        } else if metadata.is_dir() {
            if !explicit
                && path
                    .file_name()
                    .is_some_and(|name| IGNORED_DIRECTORIES.iter().any(|ignored| name == *ignored))
            {
                return Ok(());
            }
            self.entries
                .insert(key, entry("directory", String::new(), Some(state.clone())));
            let canonical = fs::canonicalize(path)?;
            if self.visited_directories.insert(canonical) {
                let mut children = fs::read_dir(path)?
                    .map(|entry| entry.map(|entry| entry.path()))
                    .collect::<io::Result<Vec<_>>>()?;
                children.sort();
                for child in children {
                    self.visit(&child, false)?;
                }
            }
            self.verify_state(path, &state);
        } else if metadata.is_file() {
            let cached = self
                .memo
                .get(&key)
                .filter(|entry| entry.kind == "file" && entry.state.as_ref() == Some(&state));
            let digest = if let Some(cached) = cached {
                cached.digest.clone()
            } else {
                hash_file(path)?
            };
            self.entries
                .insert(key, entry("file", digest, Some(state.clone())));
            self.verify_state(path, &state);
        } else {
            self.entries
                .insert(key, entry("special", String::new(), Some(state)));
        }
        Ok(())
    }

    fn verify_state(&mut self, path: &Path, before: &FileState) {
        self.stable &= fs::symlink_metadata(path)
            .map(|metadata| FileState::read(&metadata) == *before)
            .unwrap_or(false);
    }
}

fn entry(kind: &str, digest: String, state: Option<FileState>) -> Entry {
    Entry {
        kind: kind.to_string(),
        digest,
        state,
    }
}

fn read_output(path: &Path, cached: Option<&Output>) -> Result<Option<Output>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_string(error)),
    };
    let state = FileState::read(&metadata);
    let digest = match cached.filter(|cached| cached.state == state) {
        Some(cached) => cached.digest.clone(),
        None => hash_file(path).map_err(io_string)?,
    };
    if !fs::symlink_metadata(path)
        .map(|metadata| FileState::read(&metadata) == state)
        .unwrap_or(false)
    {
        return Ok(None);
    }
    Ok(Some(Output {
        path: path.to_path_buf(),
        digest,
        state,
    }))
}

fn hash_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => hash.update(&buffer[..count]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn hash_field(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

fn path_bytes(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        path.as_os_str().to_string_lossy().as_bytes().to_vec()
    }
}

fn path_key(path: &Path) -> String {
    let bytes = path_bytes(path);
    let mut key = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(key, "{byte:02x}");
    }
    key
}

fn timestamp(time: SystemTime) -> (i64, u32) {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => (duration.as_secs() as i64, duration.subsec_nanos()),
        Err(error) => {
            let duration = error.duration();
            if duration.subsec_nanos() == 0 {
                (-(duration.as_secs() as i64), 0)
            } else {
                (
                    -(duration.as_secs() as i64) - 1,
                    1_000_000_000 - duration.subsec_nanos(),
                )
            }
        }
    }
}

fn write_stamp(path: &Path, stamp: &Stamp) -> Result<(), String> {
    let bytes = serde_json::to_vec(stamp).map_err(|error| error.to_string())?;
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(io_string)?;
    let (temporary, mut file) = loop {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".blueprint-cache-{}-{sequence}.tmp",
            std::process::id()
        ));
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_string(error)),
        }
    };
    let result = (|| {
        file.write_all(&bytes)?;
        file.flush()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(io_string)
}

fn io_string(error: io::Error) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::FileTimes;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "blueprint-build-cache-test-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir_all(path.join("source")).unwrap();
            fs::write(path.join("source/main.rs"), b"first").unwrap();
            fs::write(path.join("output.bp"), b"packed").unwrap();
            Self(path)
        }

        fn cache(&self) -> BuildCache {
            self.cache_with("app:dev", "PRIVATE_TOKEN=not-for-the-stamp")
        }

        fn cache_with(&self, key: &str, environment: &str) -> BuildCache {
            BuildCache::new(
                self.0.join("cache.json"),
                key.to_string(),
                vec![self.0.join("source")],
                environment.to_string(),
            )
            .unwrap()
        }

        fn record(&self) {
            let mut cache = self.cache();
            cache.watch(Vec::new()).unwrap();
            assert!(
                cache
                    .record(vec![self.0.join("output.bp")], Vec::new())
                    .unwrap()
            );
        }

        fn fresh(&self) -> bool {
            self.cache().is_fresh().unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn unchanged_build_hits_and_timestamp_only_changes_keep_hit() {
        let fixture = Fixture::new();
        fixture.record();
        assert!(fixture.fresh());
        let source = fixture.0.join("source/main.rs");
        File::open(&source)
            .unwrap()
            .set_times(FileTimes::new().set_modified(UNIX_EPOCH))
            .unwrap();
        assert!(fixture.fresh());
        let stamp = fs::read_to_string(fixture.0.join("cache.json")).unwrap();
        assert!(!stamp.contains("PRIVATE_TOKEN"));
        assert!(!stamp.contains("not-for-the-stamp"));
    }

    #[cfg(unix)]
    #[test]
    fn generated_cache_symlinks_are_excluded_from_source_tracking() {
        let fixture = Fixture::new();
        let generated = fixture.0.join("shared-target");
        fs::create_dir(&generated).unwrap();
        fs::write(generated.join("output"), "old").unwrap();
        std::os::unix::fs::symlink(&generated, fixture.0.join("source/target")).unwrap();
        fixture.record();
        fs::write(generated.join("output"), "new").unwrap();
        assert!(fixture.fresh());
    }

    #[test]
    fn same_size_edits_additions_and_removals_invalidate() {
        let fixture = Fixture::new();
        fixture.record();
        fs::write(fixture.0.join("source/main.rs"), b"other").unwrap();
        assert!(!fixture.fresh());
        fixture.record();
        fs::write(fixture.0.join("source/asset.bin"), b"asset").unwrap();
        assert!(!fixture.fresh());
        fixture.record();
        fs::remove_file(fixture.0.join("source/asset.bin")).unwrap();
        assert!(!fixture.fresh());
    }

    #[test]
    fn missing_corrupt_output_and_invalid_stamp_are_misses() {
        let fixture = Fixture::new();
        fixture.record();
        fs::write(fixture.0.join("output.bp"), b"broken").unwrap();
        assert!(!fixture.fresh());
        fixture.record();
        fs::remove_file(fixture.0.join("output.bp")).unwrap();
        assert!(!fixture.fresh());
        fs::write(fixture.0.join("cache.json"), b"not json").unwrap();
        assert!(!fixture.fresh());
    }

    #[test]
    fn configuration_changes_and_failed_rebuilds_cannot_reuse_success() {
        let fixture = Fixture::new();
        fixture.record();
        assert!(
            !fixture
                .cache_with("app:release", "PRIVATE_TOKEN=not-for-the-stamp")
                .is_fresh()
                .unwrap()
        );
        assert!(
            !fixture
                .cache_with("app:dev", "different")
                .is_fresh()
                .unwrap()
        );
        let mut cache = fixture.cache();
        cache.invalidate().unwrap();
        assert!(!cache.is_fresh().unwrap());
        assert!(!fixture.fresh());
    }

    #[test]
    fn rejects_edits_during_build_and_between_preparation_watches() {
        let fixture = Fixture::new();
        let mut cache = fixture.cache();
        cache.watch(Vec::new()).unwrap();
        fs::write(fixture.0.join("source/main.rs"), b"other").unwrap();
        assert!(
            !cache
                .record(vec![fixture.0.join("output.bp")], Vec::new())
                .unwrap()
        );
        assert!(!fixture.0.join("cache.json").exists());
        let mut cache = fixture.cache();
        cache.watch(Vec::new()).unwrap();
        fs::write(fixture.0.join("source/main.rs"), b"third").unwrap();
        cache.watch(Vec::new()).unwrap();
        assert!(
            !cache
                .record(vec![fixture.0.join("output.bp")], Vec::new())
                .unwrap()
        );
    }

    #[test]
    fn records_preexisting_discovered_files_but_rejects_new_build_time_inputs() {
        let fixture = Fixture::new();
        let external = fixture.0.join("external.rs");
        fs::write(&external, b"preexisting").unwrap();
        let mut cache = fixture.cache();
        cache.watch(Vec::new()).unwrap();
        assert!(
            cache
                .record(vec![fixture.0.join("output.bp")], vec![external.clone()])
                .unwrap()
        );
        assert!(fixture.fresh());
        fs::write(&external, b"external edit").unwrap();
        assert!(!fixture.fresh());

        let mut cache = fixture.cache();
        cache.watch(Vec::new()).unwrap();
        let late = fixture.0.join("late.rs");
        fs::write(&late, b"created during build").unwrap();
        // Give the file a distinguishable timestamp even on filesystems with
        // coarse inode timestamp resolution.
        File::open(&late)
            .unwrap()
            .set_times(
                FileTimes::new()
                    .set_modified(SystemTime::now() + std::time::Duration::from_secs(1)),
            )
            .unwrap();
        assert!(
            !cache
                .record(vec![fixture.0.join("output.bp")], vec![late])
                .unwrap()
        );
    }

    #[test]
    fn newly_prepared_dependencies_can_be_added_before_compile() {
        let fixture = Fixture::new();
        let mut cache = fixture.cache();
        cache.watch(Vec::new()).unwrap();
        let prepared = fixture.0.join("prepared.rs");
        fs::write(&prepared, b"prepared sysroot").unwrap();
        cache.watch(vec![prepared.clone()]).unwrap();
        assert!(
            cache
                .record(vec![fixture.0.join("output.bp")], Vec::new())
                .unwrap()
        );
        assert!(fixture.fresh());
        fs::write(prepared, b"updated sysroot").unwrap();
        assert!(!fixture.fresh());
    }

    #[test]
    fn replacement_build_drops_dependencies_removed_from_the_graph() {
        let fixture = Fixture::new();
        let removed = fixture.0.join("removed-dependency.rs");
        fs::write(&removed, b"old dependency").unwrap();
        let mut cache = fixture.cache();
        cache.watch(vec![removed.clone()]).unwrap();
        assert!(
            cache
                .record(vec![fixture.0.join("output.bp")], Vec::new())
                .unwrap()
        );
        let mut cache = fixture.cache();
        cache.invalidate().unwrap();
        cache.watch(Vec::new()).unwrap();
        assert!(
            cache
                .record(vec![fixture.0.join("output.bp")], Vec::new())
                .unwrap()
        );
        fs::write(removed, b"unrelated edit").unwrap();
        assert!(fixture.fresh());
    }

    #[test]
    fn missing_explicit_inputs_and_required_sidecars_are_tracked() {
        let fixture = Fixture::new();
        let optional_input = fixture.0.join("optional-config");
        let sidecar = fixture.0.join("output.debug");
        fs::write(&sidecar, b"debug info").unwrap();
        let mut cache = fixture.cache();
        cache.watch(vec![optional_input.clone()]).unwrap();
        assert!(
            cache
                .record(
                    vec![fixture.0.join("output.bp"), sidecar.clone()],
                    Vec::new()
                )
                .unwrap()
        );
        assert!(fixture.fresh());
        fs::write(optional_input, b"new config").unwrap();
        assert!(!fixture.fresh());
        fs::remove_file(sidecar).unwrap();
        assert!(!fixture.fresh());
    }

    #[test]
    fn skips_generated_directories_but_tracks_explicit_files_inside_them() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.0.join("source/target")).unwrap();
        fs::write(fixture.0.join("source/target/ignored"), b"old").unwrap();
        fixture.record();
        fs::write(fixture.0.join("source/target/ignored"), b"new").unwrap();
        assert!(fixture.fresh());
        let explicit = fixture.0.join("source/target/explicit");
        fs::write(&explicit, b"watched").unwrap();
        let mut cache = fixture.cache();
        cache.watch(vec![explicit.clone()]).unwrap();
        assert!(
            cache
                .record(vec![fixture.0.join("output.bp")], Vec::new())
                .unwrap()
        );
        assert!(fixture.fresh());
        fs::write(explicit, b"changed").unwrap();
        assert!(!fixture.fresh());
    }

    #[cfg(unix)]
    #[test]
    fn follows_external_symlinks_and_handles_cycles_and_broken_links() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let external = fixture.0.join("external");
        fs::create_dir_all(&external).unwrap();
        fs::write(external.join("asset"), b"first").unwrap();
        symlink(&external, fixture.0.join("source/external")).unwrap();
        symlink(fixture.0.join("source"), fixture.0.join("source/cycle")).unwrap();
        symlink("missing", fixture.0.join("source/broken")).unwrap();
        fixture.record();
        assert!(fixture.fresh());
        fs::write(external.join("asset"), b"other").unwrap();
        assert!(!fixture.fresh());
        fixture.record();
        fs::write(fixture.0.join("source/missing"), b"appeared").unwrap();
        assert!(!fixture.fresh());
    }
}
