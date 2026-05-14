extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

use crate::vcabi;

pub const FS_ERR_NO_SPACE: i32 = -3;
pub const FS_ERR_BAD_PARAM: i32 = -4;
pub const FS_ERR_NOT_FOUND: i32 = -8;
pub const FS_ERR_TIMEOUT: i32 = -14;
pub const NET_ERR_BAD_URL: i32 = -10;
pub const NET_ERR_TIMEOUT: i32 = -11;
pub const NET_ERR_HTTP: i32 = -12;
pub const NET_ERR_TLS: i32 = -13;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FetchBytesError {
    StartFailed,
    Pending,
    Code(i32),
    NoSpace { needed: usize, cap: usize },
}

#[inline]
pub fn fetch_bytes_start(url: &str) -> Result<u32, FetchBytesError> {
    let op_id = unsafe { vcabi::trueos_cabi_net_fetch_bytes_start(url.as_ptr(), url.len()) };
    if op_id == 0 {
        Err(FetchBytesError::StartFailed)
    } else {
        Ok(op_id)
    }
}

#[inline]
pub fn fetch_bytes_result_len(op_id: u32) -> Result<Option<usize>, FetchBytesError> {
    let rc = unsafe { vcabi::trueos_cabi_net_fetch_bytes_result_len(op_id) };
    if rc == FS_ERR_NOT_FOUND as isize {
        Ok(None)
    } else if rc < 0 {
        Err(FetchBytesError::Code(rc as i32))
    } else {
        Ok(Some(rc as usize))
    }
}

pub fn fetch_bytes_read(op_id: u32, len: usize) -> Result<Vec<u8>, FetchBytesError> {
    let mut out = vec![0u8; len];
    let rc = unsafe { vcabi::trueos_cabi_net_fetch_bytes_read(op_id, out.as_mut_ptr(), out.len()) };
    if rc == FS_ERR_NOT_FOUND as isize {
        return Err(FetchBytesError::Pending);
    }
    if rc < 0 {
        return Err(FetchBytesError::Code(rc as i32));
    }
    let read = rc as usize;
    if read > out.len() {
        return Err(FetchBytesError::NoSpace {
            needed: read,
            cap: out.len(),
        });
    }
    out.truncate(read);
    Ok(out)
}

#[inline]
pub fn fetch_bytes_discard(op_id: u32) {
    let _ = unsafe { vcabi::trueos_cabi_net_fetch_bytes_discard(op_id) };
}

pub fn code_name(code: i32) -> &'static str {
    match code {
        0 => "OK",
        FS_ERR_NO_SPACE => "FS_ERR_NO_SPACE",
        FS_ERR_BAD_PARAM => "FS_ERR_BAD_PARAM",
        FS_ERR_NOT_FOUND => "FS_ERR_NOT_FOUND",
        FS_ERR_TIMEOUT => "FS_ERR_TIMEOUT",
        NET_ERR_BAD_URL => "NET_ERR_BAD_URL",
        NET_ERR_TIMEOUT => "NET_ERR_TIMEOUT",
        NET_ERR_HTTP => "NET_ERR_HTTP",
        NET_ERR_TLS => "NET_ERR_TLS",
        _ => "UNKNOWN",
    }
}
