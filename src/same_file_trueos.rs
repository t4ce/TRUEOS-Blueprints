use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::Path;

/// Retains the native object while comparing its root and file identities.
#[derive(Debug)]
pub struct Handle {
    file: File,
    root_id: u32,
    file_id: u64,
}

impl Eq for Handle {}
impl PartialEq for Handle {
    fn eq(&self, other: &Self) -> bool {
        (self.root_id, self.file_id) == (other.root_id, other.file_id)
    }
}
impl Hash for Handle {
    fn hash<H: Hasher>(&self, hasher: &mut H) {
        self.root_id.hash(hasher);
        self.file_id.hash(hasher);
    }
}

impl Handle {
    pub fn from_path<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        Self::from_file(File::open(path)?)
    }
    pub fn from_file(file: File) -> io::Result<Self> {
        let metadata = file.metadata()?;
        let file_id = metadata.file_id().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "native file metadata has no stable filesystem object identity",
            )
        })?;
        Ok(Self {
            file,
            root_id: metadata.root_id(),
            file_id,
        })
    }
    pub fn stdin() -> io::Result<Self> {
        unsupported_stream()
    }
    pub fn stdout() -> io::Result<Self> {
        unsupported_stream()
    }
    pub fn stderr() -> io::Result<Self> {
        unsupported_stream()
    }
    pub fn as_file(&self) -> &File {
        &self.file
    }
    pub fn as_file_mut(&mut self) -> &mut File {
        &mut self.file
    }
    pub fn dev(&self) -> u64 {
        u64::from(self.root_id)
    }
    pub fn ino(&self) -> u64 {
        self.file_id
    }
}

fn unsupported_stream<T>() -> io::Result<T> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native console streams have no filesystem object identity",
    ))
}
