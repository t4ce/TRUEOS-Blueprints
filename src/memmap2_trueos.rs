//! Owned private file snapshots on TRUEOS, backed by the native File provider.
//! Git uses private read-only maps. Shared writable, executable and foreign-FD
//! mapping semantics require an actual VM mapping provider and are unsupported.
use std::{fs::File, io, os::unix::fs::FileExt};

pub struct MmapInner {
    bytes: Vec<u8>,
}
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "TRUEOS supports owned private file snapshots, not Unix mappings",
    )
}
fn options(populate: bool, no_reserve: bool) -> io::Result<()> {
    if populate || no_reserve {
        Err(unsupported())
    } else {
        Ok(())
    }
}
impl MmapInner {
    fn snapshot(
        len: usize,
        file: &File,
        offset: u64,
        populate: bool,
        no_reserve: bool,
    ) -> io::Result<Self> {
        options(populate, no_reserve)?;
        if len > isize::MAX as usize || offset.checked_add(len as u64).is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native snapshot range overflows",
            ));
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(len).map_err(|_| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                "native snapshot allocation failed",
            )
        })?;
        bytes.resize(len, 0);
        let mut read = 0;
        while read < len {
            match file.read_at(&mut bytes[read..], offset + read as u64) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "native snapshot extends beyond the file",
                    ));
                }
                Ok(count) => read += count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(Self { bytes })
    }
    pub fn map(_: usize, _: &File, _: u64, _: bool, _: bool) -> io::Result<Self> {
        Err(unsupported())
    }
    pub fn map_exec(_: usize, _: &File, _: u64, _: bool, _: bool) -> io::Result<Self> {
        Err(unsupported())
    }
    pub fn map_mut(_: usize, _: &File, _: u64, _: bool, _: bool) -> io::Result<Self> {
        Err(unsupported())
    }
    pub fn map_copy(
        len: usize,
        file: &File,
        offset: u64,
        populate: bool,
        no_reserve: bool,
    ) -> io::Result<Self> {
        Self::snapshot(len, file, offset, populate, no_reserve)
    }
    pub fn map_copy_read_only(
        len: usize,
        file: &File,
        offset: u64,
        populate: bool,
        no_reserve: bool,
    ) -> io::Result<Self> {
        Self::snapshot(len, file, offset, populate, no_reserve)
    }
    pub fn map_anon(
        len: usize,
        stack: bool,
        populate: bool,
        huge: Option<u8>,
        no_reserve: bool,
    ) -> io::Result<Self> {
        if stack || populate || huge.is_some() || no_reserve {
            return Err(unsupported());
        }
        if len > isize::MAX as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native buffer length overflows",
            ));
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(len).map_err(|_| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                "native buffer allocation failed",
            )
        })?;
        bytes.resize(len, 0);
        Ok(Self { bytes })
    }
    fn range(&self, offset: usize, len: usize) -> io::Result<()> {
        if offset
            .checked_add(len)
            .is_some_and(|end| end <= self.bytes.len())
        {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native buffer range is out of bounds",
            ))
        }
    }
    // Private/anonymous buffers have no file writeback. Validate the requested
    // range rather than pretending a shared writable mapping was flushed.
    pub fn flush(&self, offset: usize, len: usize) -> io::Result<()> {
        self.range(offset, len)
    }
    pub fn flush_async(&self, offset: usize, len: usize) -> io::Result<()> {
        self.range(offset, len)
    }
    pub fn make_read_only(&mut self) -> io::Result<()> {
        Ok(())
    }
    pub fn make_mut(&mut self) -> io::Result<()> {
        Ok(())
    }
    pub fn make_exec(&mut self) -> io::Result<()> {
        Err(unsupported())
    }
    pub unsafe fn advise(&self, _: libc::c_int, _: usize, _: usize) -> io::Result<()> {
        Err(unsupported())
    }
    pub fn lock(&self) -> io::Result<()> {
        Err(unsupported())
    }
    pub fn unlock(&self) -> io::Result<()> {
        Err(unsupported())
    }
    #[cfg(target_os = "linux")]
    pub fn remap(&mut self, _: usize, _: crate::RemapOptions) -> io::Result<()> {
        Err(unsupported())
    }
    pub fn ptr(&self) -> *const u8 {
        self.bytes.as_ptr()
    }
    pub fn mut_ptr(&mut self) -> *mut u8 {
        self.bytes.as_mut_ptr()
    }
    pub fn len(&self) -> usize {
        self.bytes.len()
    }
}
pub fn file_len(file: &File) -> io::Result<u64> {
    Ok(file.metadata()?.len())
}
