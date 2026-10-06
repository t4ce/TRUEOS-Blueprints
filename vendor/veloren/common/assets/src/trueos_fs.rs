//! Veloren's filesystem source backed by native asynchronous TRUEOSFS jobs.
//!
//! `Source` is synchronous. The API's cooperative `block_on` only waits for
//! kernel-owned jobs; it does not construct a Tokio runtime or perform storage
//! work on this Blueprint lane.

use std::{
    io,
    path::{Path, PathBuf},
};

use assets_manager::source::{DirEntry, FileContent, Source};
use trueos::async_fs::{self, NodeKind};

#[derive(Debug, Clone)]
pub struct FileSystem {
    root: PathBuf,
}

fn error(operation: &str, path: &Path, code: i32) -> io::Error {
    let kind = match code {
        async_fs::ERR_NOT_FOUND => io::ErrorKind::NotFound,
        async_fs::ERR_BAD_PARAM => io::ErrorKind::InvalidInput,
        async_fs::ERR_BAD_UTF8 => io::ErrorKind::InvalidData,
        async_fs::ERR_NO_MEMORY => io::ErrorKind::OutOfMemory,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(
        kind,
        format!(
            "TRUEOSFS {operation} {} failed (code {code})",
            path.display()
        ),
    )
}

pub(super) fn is_dir(path: &Path) -> bool {
    async_fs::block_on(async_fs::metadata(path.as_os_str().as_encoded_bytes()))
        .is_ok_and(|metadata| metadata.is_dir())
}

pub(super) fn list_children(path: &Path) -> io::Result<Vec<(PathBuf, bool)>> {
    let listing = async_fs::block_on(async_fs::list_dir(path.as_os_str().as_encoded_bytes()))
        .map_err(|code| error("list directory", path, code))?;
    if listing.truncated {
        return Err(io::Error::other(format!(
            "TRUEOSFS directory listing truncated: {}",
            path.display()
        )));
    }
    Ok(listing
        .entries
        .into_iter()
        .map(|entry| (path.join(entry.name), entry.kind == NodeKind::Directory))
        .collect())
}

impl FileSystem {
    pub fn new(path: impl AsRef<Path>) -> io::Result<Self> {
        let root = path.as_ref().to_owned();
        let metadata = async_fs::block_on(async_fs::metadata(root.as_os_str().as_encoded_bytes()))
            .map_err(|code| error("stat directory", &root, code))?;
        if !metadata.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                format!("asset root is not a directory: {}", root.display()),
            ));
        }
        // Validate readability and the listing contract before creating a cache.
        list_children(&root)?;
        Ok(Self { root })
    }

    pub fn path_of(&self, entry: DirEntry<'_>) -> PathBuf {
        let mut path = self.root.clone();
        path.extend(entry.id().split('.'));
        if let DirEntry::File(_, extension) = entry {
            path.set_extension(extension);
        }
        path
    }
}

impl Source for FileSystem {
    fn read(&self, id: &str, ext: &str) -> io::Result<FileContent<'_>> {
        let path = self.path_of(DirEntry::File(id, ext));
        async_fs::block_on(async_fs::read_file(path.as_os_str().as_encoded_bytes()))
            .map(FileContent::Buffer)
            .map_err(|code| error("read asset", &path, code))
    }

    fn read_dir(&self, id: &str, callback: &mut dyn FnMut(DirEntry)) -> io::Result<()> {
        // Asset IDs and extensions define membership, not content signatures:
        // manifests, VOX files and binary maps may all be stored as BLOBs.
        // Native listings already carry node kinds, avoiding a stat per child.
        for (path, is_dir) in list_children(&self.path_of(DirEntry::Directory(id)))? {
            let Some(filename) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let (name, extension) = filename.split_once('.').unwrap_or((filename, ""));
            if name.is_empty() {
                continue;
            }
            let entry_id = if id.is_empty() {
                name.to_owned()
            } else {
                format!("{id}.{name}")
            };
            if is_dir {
                callback(DirEntry::Directory(&entry_id));
            } else {
                callback(DirEntry::File(&entry_id, extension));
            }
        }
        Ok(())
    }

    fn exists(&self, entry: DirEntry) -> bool {
        let path = self.path_of(entry);
        async_fs::block_on(async_fs::exists(path.as_os_str().as_encoded_bytes())).unwrap_or(false)
    }
}
