use super::FsStats;
use std::fs::{File, TryLockError};
use std::io::{self, Error, ErrorKind, Result};
use std::path::Path;

pub fn duplicate(file: &File) -> Result<File> {
    file.try_clone()
}
pub fn lock_shared(file: &File) -> Result<()> {
    file.lock_shared()
}
pub fn lock_exclusive(file: &File) -> Result<()> {
    file.lock()
}
pub fn unlock(file: &File) -> Result<()> {
    file.unlock()
}
pub fn lock_error() -> Error {
    Error::new(ErrorKind::WouldBlock, "native file lock is held")
}

fn lock_result(result: std::result::Result<(), TryLockError>) -> Result<()> {
    result.map_err(|error| match error {
        TryLockError::WouldBlock => lock_error(),
        TryLockError::Error(error) => error,
    })
}
pub fn try_lock_shared(file: &File) -> Result<()> {
    lock_result(file.try_lock_shared())
}
pub fn try_lock_exclusive(file: &File) -> Result<()> {
    lock_result(file.try_lock())
}

fn unavailable<T>() -> Result<T> {
    Err(io::Error::new(
        ErrorKind::Unsupported,
        "native filesystem does not expose physical allocation or free-space statistics",
    ))
}
// Logical file length cannot stand in for allocated blocks, reserved disk
// capacity or mount free space. Callers can choose their existing fallback.
pub fn allocated_size(_: &File) -> Result<u64> {
    unavailable()
}
pub fn allocate(_: &File, _: u64) -> Result<()> {
    unavailable()
}
pub fn statvfs(_: &Path) -> Result<FsStats> {
    unavailable()
}
