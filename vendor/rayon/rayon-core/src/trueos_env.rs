//! Minimal TRUEOS environment access for Rayon thread-count configuration.
//!
//! Keep this local instead of depending on the full `v` crate: `v` exposes
//! hashbrown 0.17 collections, while hashbrown 0.17's optional Rayon support
//! depends back on Rayon. That creates a TRUEOS-only Cargo package cycle.

use std::string::String;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VarError {
    NotPresent,
    NotUnicode,
}

unsafe extern "C" {
    fn trueos_cabi_env_var(
        key_ptr: *const u8,
        key_len: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
}

pub(super) fn var<K: AsRef<str>>(key: K) -> Result<String, VarError> {
    let key = key.as_ref();
    let len = unsafe {
        trueos_cabi_env_var(key.as_ptr(), key.len(), core::ptr::null_mut(), 0)
    };
    if len < 0 {
        return Err(VarError::NotPresent);
    }

    let mut bytes = vec![0u8; len as usize];
    let got = unsafe {
        trueos_cabi_env_var(key.as_ptr(), key.len(), bytes.as_mut_ptr(), bytes.len())
    };
    if got < 0 {
        return Err(VarError::NotPresent);
    }
    bytes.truncate(got as usize);
    String::from_utf8(bytes).map_err(|_| VarError::NotUnicode)
}
