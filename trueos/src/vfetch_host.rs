extern crate alloc;

use alloc::vec::Vec;

#[inline]
pub fn prewarm_url(_url: &[u8]) -> i32 {
    -1
}

#[inline]
pub fn fetch_to_file(_url: &[u8], _path: &[u8]) -> Result<u32, i32> {
    Err(-1)
}

#[inline]
pub fn fetch_bytes(_url: &[u8]) -> Result<u32, i32> {
    Err(-1)
}

#[inline]
pub fn fetch_post_json_to_file(
    _url: &[u8],
    _path: &[u8],
    _body: &[u8],
    _bearer: Option<&[u8]>,
) -> Result<u32, i32> {
    Err(-1)
}

#[inline]
pub fn fetch_result(_op_id: u32) -> i32 {
    -1
}

#[inline]
pub fn fetch_wait(_op_id: u32, _timeout_ms: u64) -> i32 {
    -1
}

#[inline]
pub fn fetch_discard(_op_id: u32) -> i32 {
    -1
}

#[inline]
pub fn fetch_bytes_wait(_op_id: u32, _timeout_ms: u64) -> i32 {
    -1
}

#[inline]
pub fn fetch_bytes_result_len(_op_id: u32) -> Result<usize, i32> {
    Err(-1)
}

#[inline]
pub fn fetch_bytes_read(_op_id: u32) -> Result<Vec<u8>, i32> {
    Err(-1)
}

#[inline]
pub fn fetch_bytes_discard(_op_id: u32) -> i32 {
    -1
}