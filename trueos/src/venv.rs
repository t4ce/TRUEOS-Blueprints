extern crate alloc;

use alloc::{string::String, vec, vec::Vec};

use crate::vcabi;

#[inline]
pub fn var_bytes(key: &[u8]) -> Result<Vec<u8>, i32> {
    let len = unsafe { vcabi::trueos_cabi_env_var(key.as_ptr(), key.len(), core::ptr::null_mut(), 0) };
    if len < 0 {
        return Err(len as i32);
    }

    let mut bytes = vec![0u8; len as usize];
    let got = unsafe {
        vcabi::trueos_cabi_env_var(key.as_ptr(), key.len(), bytes.as_mut_ptr(), bytes.len())
    };
    if got < 0 {
        return Err(got as i32);
    }
    bytes.truncate(got as usize);
    Ok(bytes)
}

#[inline]
pub fn var(key: &str) -> Result<String, i32> {
    let bytes = var_bytes(key.as_bytes())?;
    String::from_utf8(bytes).map_err(|_| -1)
}
