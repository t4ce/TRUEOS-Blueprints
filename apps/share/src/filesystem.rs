//! A small Tokio backend, without LocalFs's Unix-specific desktop dependencies.
//! TRUEOS routes these standard file operations through its Blueprint ABI.
use std::io::{self, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use bytes::{Buf, Bytes};
use dav_server::davpath::DavPath;
use dav_server::fs::{
    DavDirEntry, DavFile, DavFileSystem, DavMetaData, FsError, FsFuture, FsResult, FsStream,
    OpenOptions, ReadDirMeta,
};
use futures_util::stream;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
type Metadata = fs::Metadata;
#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
type Metadata = std::fs::Metadata;

// The current TRUEOS Tokio file bridge retains whole files in memory. This
// per-file limit is 4 GiB; HTTP reads/writes themselves use chunks.
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

fn error(error: io::Error) -> FsError {
    match error.kind() {
        io::ErrorKind::NotFound => FsError::NotFound,
        io::ErrorKind::AlreadyExists | io::ErrorKind::DirectoryNotEmpty => FsError::Exists,
        io::ErrorKind::PermissionDenied
        | io::ErrorKind::NotADirectory
        | io::ErrorKind::IsADirectory => FsError::Forbidden,
        io::ErrorKind::StorageFull => FsError::InsufficientStorage,
        io::ErrorKind::FileTooLarge => FsError::TooLarge,
        io::ErrorKind::Unsupported => FsError::NotImplemented,
        _ => FsError::GeneralFailure,
    }
}

#[derive(Clone)]
pub struct ShareFs {
    root: PathBuf,
}

impl ShareFs {
    pub async fn new(root: &Path) -> io::Result<Self> {
        fs::create_dir_all(root).await?;
        let root = fs::canonicalize(root).await?;
        Ok(Self { root })
    }

    /// Only normal relative components are accepted, and symlinks are never
    /// followed. A missing final component is allowed for PUT/MKCOL/MOVE/COPY.
    async fn resolve(&self, path: &DavPath, allow_missing: bool) -> FsResult<PathBuf> {
        let mut result = self.root.clone();
        let components: Vec<_> = path.as_rel_ospath().components().collect();
        for (index, component) in components.iter().enumerate() {
            let Component::Normal(name) = component else {
                return Err(FsError::Forbidden);
            };
            result.push(name);
            match fs::symlink_metadata(&result).await {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(FsError::Forbidden);
                }
                Ok(_) => {}
                Err(err)
                    if allow_missing
                        && index + 1 == components.len()
                        && err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(error(err)),
            }
        }
        Ok(result)
    }

    async fn mutable_path(&self, path: &DavPath, allow_missing: bool) -> FsResult<PathBuf> {
        let resolved = self.resolve(path, allow_missing).await?;
        if resolved == self.root {
            return Err(FsError::Forbidden);
        }
        Ok(resolved)
    }
}

impl DavFileSystem for ShareFs {
    fn open<'a>(
        &'a self,
        path: &'a DavPath,
        options: OpenOptions,
    ) -> FsFuture<'a, Box<dyn DavFile>> {
        Box::pin(async move {
            if options.size.is_some_and(|size| size > MAX_FILE_BYTES) {
                return Err(FsError::TooLarge);
            }
            let target = self
                .mutable_path(path, options.create || options.create_new)
                .await?;
            if let Ok(metadata) = fs::metadata(&target).await {
                if !metadata.is_file() {
                    return Err(FsError::Forbidden);
                }
                if metadata.len() > MAX_FILE_BYTES {
                    return Err(FsError::TooLarge);
                }
            }
            let mut file = fs::OpenOptions::new()
                .read(options.read)
                .write(options.write)
                .append(options.append)
                .truncate(options.truncate)
                .create(options.create)
                .create_new(options.create_new)
                .open(target)
                .await
                .map_err(error)?;
            let position = if options.append {
                file.seek(SeekFrom::End(0)).await.map_err(error)?
            } else {
                0
            };
            Ok(Box::new(ShareFile {
                file,
                position,
                writable: options.write || options.append,
            }) as Box<dyn DavFile>)
        })
    }

    fn read_dir<'a>(
        &'a self,
        path: &'a DavPath,
        _: ReadDirMeta,
    ) -> FsFuture<'a, FsStream<Box<dyn DavDirEntry>>> {
        Box::pin(async move {
            let directory = fs::read_dir(self.resolve(path, false).await?)
                .await
                .map_err(error)?;
            let entries = stream::try_unfold(directory, |mut directory| async move {
                loop {
                    let Some(entry) = directory.next_entry().await.map_err(error)? else {
                        return Ok(None);
                    };
                    let metadata = fs::symlink_metadata(entry.path()).await.map_err(error)?;
                    if metadata.file_type().is_symlink()
                        || (!metadata.is_file() && !metadata.is_dir())
                    {
                        continue;
                    }
                    let entry = ShareEntry {
                        name: entry.file_name().as_encoded_bytes().to_vec(),
                        metadata: ShareMetadata(metadata),
                    };
                    return Ok(Some((Box::new(entry) as Box<dyn DavDirEntry>, directory)));
                }
            });
            Ok(Box::pin(entries) as FsStream<Box<dyn DavDirEntry>>)
        })
    }

    fn metadata<'a>(&'a self, path: &'a DavPath) -> FsFuture<'a, Box<dyn DavMetaData>> {
        Box::pin(async move {
            let metadata = fs::metadata(self.resolve(path, false).await?)
                .await
                .map_err(error)?;
            Ok(Box::new(ShareMetadata(metadata)) as Box<dyn DavMetaData>)
        })
    }

    fn create_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<'a, ()> {
        Box::pin(async move {
            fs::create_dir(self.mutable_path(path, true).await?)
                .await
                .map_err(error)
        })
    }

    fn remove_dir<'a>(&'a self, path: &'a DavPath) -> FsFuture<'a, ()> {
        Box::pin(async move {
            fs::remove_dir(self.mutable_path(path, false).await?)
                .await
                .map_err(error)
        })
    }

    fn remove_file<'a>(&'a self, path: &'a DavPath) -> FsFuture<'a, ()> {
        Box::pin(async move {
            fs::remove_file(self.mutable_path(path, false).await?)
                .await
                .map_err(error)
        })
    }

    fn rename<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<'a, ()> {
        Box::pin(async move {
            let source = self.mutable_path(from, false).await?;
            let destination = self.mutable_path(to, true).await?;
            fs::rename(source, destination).await.map_err(error)
        })
    }

    fn copy<'a>(&'a self, from: &'a DavPath, to: &'a DavPath) -> FsFuture<'a, ()> {
        Box::pin(async move {
            let source = self.resolve(from, false).await?;
            if fs::metadata(&source).await.map_err(error)?.len() > MAX_FILE_BYTES {
                return Err(FsError::TooLarge);
            }
            let destination = self.mutable_path(to, true).await?;
            fs::copy(source, destination).await.map_err(error)?;
            Ok(())
        })
    }
}

#[derive(Clone, Debug)]
struct ShareMetadata(Metadata);

impl DavMetaData for ShareMetadata {
    fn len(&self) -> u64 {
        self.0.len()
    }
    fn is_dir(&self) -> bool {
        self.0.is_dir()
    }
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    fn modified(&self) -> FsResult<SystemTime> {
        self.0.modified().map_err(error)
    }
    // Native Tokio metadata currently contains type and size, no timestamps.
    // Omit timestamp/ETag properties rather than inventing stable identities.
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    fn modified(&self) -> FsResult<SystemTime> {
        Err(FsError::NotImplemented)
    }
}

struct ShareEntry {
    name: Vec<u8>,
    metadata: ShareMetadata,
}

impl DavDirEntry for ShareEntry {
    fn name(&self) -> Vec<u8> {
        self.name.clone()
    }
    fn metadata(&self) -> FsFuture<'_, Box<dyn DavMetaData>> {
        Box::pin(async { Ok(Box::new(self.metadata.clone()) as Box<dyn DavMetaData>) })
    }
}

#[derive(Debug)]
struct ShareFile {
    file: fs::File,
    position: u64,
    writable: bool,
}

impl ShareFile {
    async fn write(&mut self, bytes: &[u8]) -> FsResult<()> {
        let end = self
            .position
            .checked_add(bytes.len() as u64)
            .ok_or(FsError::TooLarge)?;
        if end > MAX_FILE_BYTES {
            return Err(FsError::TooLarge);
        }
        self.file.write_all(bytes).await.map_err(error)?;
        self.position = end;
        Ok(())
    }
}

impl DavFile for ShareFile {
    fn metadata(&mut self) -> FsFuture<'_, Box<dyn DavMetaData>> {
        Box::pin(async move {
            let metadata = self.file.metadata().await.map_err(error)?;
            Ok(Box::new(ShareMetadata(metadata)) as Box<dyn DavMetaData>)
        })
    }

    fn write_buf(&mut self, mut buf: Box<dyn Buf + Send>) -> FsFuture<'_, ()> {
        Box::pin(async move {
            while buf.has_remaining() {
                let chunk = buf.chunk();
                self.write(chunk).await?;
                let count = chunk.len();
                buf.advance(count);
            }
            Ok(())
        })
    }

    fn write_bytes(&mut self, bytes: Bytes) -> FsFuture<'_, ()> {
        Box::pin(async move { self.write(&bytes).await })
    }

    fn read_bytes(&mut self, count: usize) -> FsFuture<'_, Bytes> {
        Box::pin(async move {
            let mut bytes = vec![0; count.min(64 * 1024)];
            let count = self.file.read(&mut bytes).await.map_err(error)?;
            bytes.truncate(count);
            self.position += count as u64;
            Ok(Bytes::from(bytes))
        })
    }

    fn seek(&mut self, position: SeekFrom) -> FsFuture<'_, u64> {
        Box::pin(async move {
            let position = self.file.seek(position).await.map_err(error)?;
            self.position = position;
            Ok(position)
        })
    }

    fn flush(&mut self) -> FsFuture<'_, ()> {
        Box::pin(async move {
            self.file.flush().await.map_err(error)?;
            if self.writable {
                // sync_all commits the TRUEOS Tokio bridge's buffered file.
                self.file.sync_all().await.map_err(error)?;
            }
            Ok(())
        })
    }
}
