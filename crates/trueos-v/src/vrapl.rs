extern crate alloc;

use alloc::{string::String, vec, vec::Vec};

pub const MAX_HISTORY_BYTES: usize = 5 * 1024 * 1024;

type RaplReadFn = unsafe extern "C" fn(offset: usize, out_ptr: *mut u8, out_cap: usize) -> isize;

unsafe extern "C" {
    fn trueos_vlayer_rapl_snapshot_read(offset: usize, out_ptr: *mut u8, out_cap: usize) -> isize;
    fn trueos_vlayer_rapl_history_read(offset: usize, out_ptr: *mut u8, out_cap: usize) -> isize;
}

#[inline]
pub fn snapshot_bytes() -> Result<Vec<u8>, i32> {
    read_all(trueos_vlayer_rapl_snapshot_read, usize::MAX)
}

#[inline]
pub fn snapshot_len() -> Result<usize, i32> {
    read_len(trueos_vlayer_rapl_snapshot_read)
}

#[inline]
pub fn snapshot_text() -> Result<String, i32> {
    String::from_utf8(snapshot_bytes()?).map_err(|_| -1)
}

#[inline]
pub fn history_bytes(max_bytes: usize) -> Result<Vec<u8>, i32> {
    read_all(
        trueos_vlayer_rapl_history_read,
        max_bytes.min(MAX_HISTORY_BYTES),
    )
}

#[inline]
pub fn history_tail_bytes(max_bytes: usize) -> Result<Vec<u8>, i32> {
    let len = read_len(trueos_vlayer_rapl_history_read)?;
    let n = len.min(max_bytes.min(MAX_HISTORY_BYTES));
    read_at(trueos_vlayer_rapl_history_read, len.saturating_sub(n), n)
}

#[inline]
pub fn history_len() -> Result<usize, i32> {
    read_len(trueos_vlayer_rapl_history_read).map(|len| len.min(MAX_HISTORY_BYTES))
}

#[inline]
pub fn history_text(max_bytes: usize) -> Result<String, i32> {
    String::from_utf8(history_bytes(max_bytes)?).map_err(|_| -1)
}

#[inline]
pub fn history_tail_text(max_bytes: usize) -> Result<String, i32> {
    String::from_utf8(history_tail_bytes(max_bytes)?).map_err(|_| -1)
}

fn read_all(read_fn: RaplReadFn, max_bytes: usize) -> Result<Vec<u8>, i32> {
    if max_bytes == 0 {
        return Ok(Vec::new());
    }

    let len = read_len(read_fn)?;
    let len = len.min(max_bytes);
    let mut bytes = vec![0u8; len];
    if len == 0 {
        return Ok(bytes);
    }

    let got = unsafe { read_fn(0, bytes.as_mut_ptr(), bytes.len()) };
    if got < 0 {
        return Err(got as i32);
    }

    bytes.truncate((got as usize).min(len));
    Ok(bytes)
}

fn read_at(read_fn: RaplReadFn, offset: usize, len: usize) -> Result<Vec<u8>, i32> {
    let mut bytes = vec![0u8; len];
    if len == 0 {
        return Ok(bytes);
    }

    let got = unsafe { read_fn(offset, bytes.as_mut_ptr(), bytes.len()) };
    if got < 0 {
        return Err(got as i32);
    }

    bytes.truncate((got as usize).min(len));
    Ok(bytes)
}

fn read_len(read_fn: RaplReadFn) -> Result<usize, i32> {
    let len = unsafe { read_fn(0, core::ptr::null_mut(), 0) };
    if len < 0 {
        return Err(len as i32);
    }
    Ok(len as usize)
}
