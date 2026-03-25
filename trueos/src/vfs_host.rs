extern crate alloc;

use alloc::{string::String, vec::Vec};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

struct PendingWrite {
    path: PathBuf,
    bytes: Vec<u8>,
}

static NEXT_HANDLE: AtomicU32 = AtomicU32::new(1);
static PENDING_WRITES: OnceLock<Mutex<BTreeMap<u32, PendingWrite>>> = OnceLock::new();

fn pending_writes() -> &'static Mutex<BTreeMap<u32, PendingWrite>> {
    PENDING_WRITES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn host_path(path: &[u8]) -> Result<PathBuf, i32> {
    let text = core::str::from_utf8(path).map_err(|_| -1)?;
    Ok(PathBuf::from(text))
}

#[inline]
pub fn read_file(path: &[u8]) -> Result<Vec<u8>, i32> {
    let path = host_path(path)?;
    std::fs::read(path).map_err(|_| -1)
}

#[inline]
pub fn read_file_utf8(path: &[u8]) -> Result<String, i32> {
    let bytes = read_file(path)?;
    String::from_utf8(bytes).map_err(|_| -1)
}

#[inline]
pub fn write_begin(path: &[u8], total_len: u64) -> Result<u32, i32> {
    let path = host_path(path)?;
    let capacity = usize::try_from(total_len).map_err(|_| -1)?;
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed).max(1);
    pending_writes().lock().unwrap().insert(
        handle,
        PendingWrite {
            path,
            bytes: Vec::with_capacity(capacity),
        },
    );
    Ok(handle)
}

#[inline]
pub fn write_chunk(handle: u32, data: &[u8]) -> Result<(), i32> {
    let mut pending = pending_writes().lock().unwrap();
    let Some(file) = pending.get_mut(&handle) else {
        return Err(-1);
    };
    file.bytes.extend_from_slice(data);
    Ok(())
}

#[inline]
pub fn write_finish(handle: u32) -> Result<(), i32> {
    let pending = pending_writes().lock().unwrap().remove(&handle).ok_or(-1)?;
    if let Some(parent) = pending.path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| -1)?;
    }
    std::fs::write(pending.path, pending.bytes).map_err(|_| -1)
}

#[inline]
pub fn write_abort(handle: u32) -> Result<(), i32> {
    pending_writes().lock().unwrap().remove(&handle).ok_or(-1)?;
    Ok(())
}

#[inline]
pub fn remove(path: &[u8]) -> Result<(), i32> {
    let path = host_path(path)?;
    std::fs::remove_file(&path)
        .or_else(|_| std::fs::remove_dir_all(&path))
        .map_err(|_| -1)
}

#[inline]
pub fn trueosfs_primary_html_tree(_max_entries: u32) -> Result<Vec<u8>, i32> {
    Err(-1)
}