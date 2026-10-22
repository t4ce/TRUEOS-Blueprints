use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::source_db::{LoadedSource, SourceFileInput};

const STAGING_ROOT: &str = "rustc-source-staging";
const TRUEOSFS_DIRECTORY_MARKER: &str = ".keep";
const MAX_SOURCE_FILES: usize = 4096;
const MAX_SOURCE_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SOURCE_BYTES: u64 = 256 * 1024 * 1024;
static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub async fn read_archive(path: &str) -> Result<Vec<SourceFileInput>, String> {
    let staging = unique_staging_path("import");
    reset_staging(staging.as_path())?;
    let result = async {
        let staging_bytes = staging
            .to_str()
            .ok_or_else(|| String::from("staging path is not UTF-8"))?
            .as_bytes();
        let report = trueos::archive::unpack(path.as_bytes(), staging_bytes)
            .await
            .map_err(|code| format!("unpack {path}: archive error {code}"))?;
        let files = collect_files(staging.as_path())?;
        if files.len() as u32 != report.file_count {
            let paths = files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>()
                .join(",");
            return Err(format!(
                "archive report declared {} files but staging contains {} paths={}",
                report.file_count,
                files.len(),
                paths
            ));
        }
        Ok(files)
    }
    .await;
    let cleanup = remove_staging(staging.as_path());
    match (result, cleanup) {
        (Ok(files), Ok(())) => Ok(files),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

pub async fn write_archive(
    source: &LoadedSource,
    archive_path: &str,
) -> Result<trueos::archive::Report, String> {
    let staging = unique_staging_path("export");
    reset_staging(staging.as_path())?;
    let result = async {
        for file in &source.files {
            let path = safe_staging_path(staging.as_path(), file.path.as_str())?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| io_error("create export directory", parent, error))?;
            }
            fs::write(path.as_path(), file.bytes.as_slice())
                .map_err(|error| io_error("write export source", path.as_path(), error))?;
        }
        let staging_bytes = staging
            .to_str()
            .ok_or_else(|| String::from("staging path is not UTF-8"))?
            .as_bytes();
        trueos::archive::pack(staging_bytes, archive_path.as_bytes())
            .await
            .map_err(|code| format!("pack {archive_path}: archive error {code}"))
    }
    .await;
    let cleanup = remove_staging(staging.as_path());
    match (result, cleanup) {
        (Ok(report), Ok(())) => Ok(report),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

fn reset_staging(path: &Path) -> Result<(), String> {
    if !is_staging_path(path) {
        return Err(String::from("refused unexpected staging path"));
    }
    remove_staging(path)?;
    fs::create_dir_all(path).map_err(|error| io_error("create staging directory", path, error))
}

fn remove_staging(path: &Path) -> Result<(), String> {
    if !is_staging_path(path) {
        return Err(String::from("refused unexpected staging path"));
    }
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error("remove staging directory", path, error)),
    }
}

fn unique_staging_path(operation: &str) -> PathBuf {
    let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = trueos::clock::monotonic_nanos();
    Path::new(STAGING_ROOT).join(format!("{operation}-{nanos:016x}-{sequence:016x}"))
}

fn is_staging_path(path: &Path) -> bool {
    path.parent() == Some(Path::new(STAGING_ROOT))
        && path.file_name().is_some_and(|name| {
            name.to_str().is_some_and(|name| {
                ["import-", "export-"]
                    .into_iter()
                    .any(|prefix| name.starts_with(prefix) && name.len() > prefix.len())
            })
        })
}

fn collect_files(root: &Path) -> Result<Vec<SourceFileInput>, String> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    let mut total = 0u64;
    while let Some(directory) = pending.pop() {
        let mut entries = fs::read_dir(directory.as_path())
            .map_err(|error| io_error("read staging directory", directory.as_path(), error))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| io_error("read staging entry", directory.as_path(), error))?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let file_type = entry.file_type().map_err(|error| {
                io_error("inspect staging entry", entry.path().as_path(), error)
            })?;
            // TRUEOSFS represents directories with reserved, empty `.keep`
            // records. The kernel archive worker creates these records while
            // materializing the staging tree; they are filesystem metadata,
            // not members of the source archive. The archive packer applies
            // the same reservation when walking a TRUEOSFS directory.
            if file_type.is_file() && is_trueosfs_directory_marker(entry.file_name().as_os_str()) {
                continue;
            }
            if file_type.is_dir() {
                pending.push(entry.path());
                continue;
            }
            if !file_type.is_file() {
                return Err(format!(
                    "archive contains unsupported non-file entry {}",
                    entry.path().display()
                ));
            }
            if files.len() >= MAX_SOURCE_FILES {
                return Err(format!("source archive exceeds {MAX_SOURCE_FILES} files"));
            }
            let metadata = entry
                .metadata()
                .map_err(|error| io_error("inspect source file", entry.path().as_path(), error))?;
            if metadata.len() > MAX_SOURCE_FILE_BYTES {
                return Err(format!(
                    "source file {} is {} bytes; limit is {MAX_SOURCE_FILE_BYTES}",
                    entry.path().display(),
                    metadata.len()
                ));
            }
            total = total
                .checked_add(metadata.len())
                .ok_or_else(|| String::from("source archive byte count overflow"))?;
            if total > MAX_SOURCE_BYTES {
                return Err(format!(
                    "source archive is {total} bytes; limit is {MAX_SOURCE_BYTES}"
                ));
            }
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|_| String::from("staging entry escaped its root"))?
                .to_str()
                .ok_or_else(|| String::from("source archive path is not UTF-8"))?
                .replace('\\', "/");
            let bytes = fs::read(entry.path())
                .map_err(|error| io_error("read unpacked source", entry.path().as_path(), error))?;
            files.push(SourceFileInput::new(relative, bytes));
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    if files.is_empty() {
        return Err(String::from("source archive contains no files"));
    }
    Ok(files)
}

fn is_trueosfs_directory_marker(name: &OsStr) -> bool {
    name == OsStr::new(TRUEOSFS_DIRECTORY_MARKER)
}

fn safe_staging_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.ends_with('/')
        || relative.contains('\\')
        || relative
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return Err(format!("unsafe stored source path `{relative}`"));
    }
    Ok(root.join(relative))
}

fn io_error(operation: &str, path: &Path, error: io::Error) -> String {
    format!("{operation} {}: {error}", path.display())
}

#[cfg(test)]
mod tests {
    use super::{is_staging_path, is_trueosfs_directory_marker, safe_staging_path};
    use std::ffi::OsStr;
    use std::path::Path;

    #[test]
    fn export_paths_remain_under_staging() {
        let staging = Path::new("rustc-source-staging/export-1234");
        assert_eq!(
            safe_staging_path(staging, "src/main.rs").unwrap(),
            staging.join("src/main.rs")
        );
        for path in [
            "",
            "../secret",
            "/common/secret",
            "src//main.rs",
            "src\\main.rs",
        ] {
            assert!(safe_staging_path(staging, path).is_err());
        }
        assert!(is_staging_path(staging));
        assert!(!is_staging_path(Path::new("rustc-source-staging")));
        assert!(!is_staging_path(Path::new("other/export-1234")));
    }

    #[test]
    fn trueosfs_directory_markers_are_not_source_files() {
        assert!(is_trueosfs_directory_marker(OsStr::new(".keep")));
        assert!(!is_trueosfs_directory_marker(OsStr::new(".gitkeep")));
        assert!(!is_trueosfs_directory_marker(OsStr::new("keep")));
    }
}
