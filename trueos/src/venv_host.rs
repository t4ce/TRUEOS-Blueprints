extern crate alloc;

use alloc::{string::String, vec::Vec};

#[inline]
pub fn var_bytes(key: &[u8]) -> Result<Vec<u8>, i32> {
    let key = core::str::from_utf8(key).map_err(|_| -1)?;
    let value = std::env::var(key).map_err(|_| -1)?;
    Ok(value.into_bytes())
}

#[inline]
pub fn var(key: &str) -> Result<String, i32> {
    std::env::var(key).map_err(|_| -1)
}
