#![no_std]
#![allow(non_snake_case)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

unsafe extern "C" {
    fn trueos_cabi_fs_read_file(
        path_ptr: *const u8,
        path_len: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    fn trueos_cabi_net_fetch_start(
        url_ptr: *const u8,
        url_len: usize,
        path_ptr: *const u8,
        path_len: usize,
    ) -> u32;
    fn trueos_cabi_net_fetch_wait(op_id: u32, timeout_ms: u64) -> i32;
    fn trueos_cabi_net_fetch_result(op_id: u32) -> i32;
    fn trueos_cabi_net_fetch_discard(op_id: u32) -> i32;
}

fn read_file(path: &str) -> Result<Vec<u8>, i32> {
    let len =
        unsafe { trueos_cabi_fs_read_file(path.as_ptr(), path.len(), core::ptr::null_mut(), 0) };
    if len < 0 {
        return Err(len as i32);
    }
    let mut out = Vec::with_capacity(len as usize);
    out.resize(len as usize, 0);
    let got =
        unsafe { trueos_cabi_fs_read_file(path.as_ptr(), path.len(), out.as_mut_ptr(), out.len()) };
    if got < 0 {
        return Err(got as i32);
    }
    out.truncate(got as usize);
    Ok(out)
}

fn normalize_country_code(countrycode: &str) -> Option<String> {
    let trimmed = countrycode.trim();
    if trimmed.len() != 2 {
        return None;
    }
    if !trimmed.as_bytes().iter().all(u8::is_ascii_alphabetic) {
        return None;
    }
    Some(trimmed.to_ascii_lowercase())
}

fn flag_path(code: &str) -> String {
    format!("flags/{}.svg", code)
}

fn flag_url(code: &str) -> String {
    format!("https://flagcdn.com/{}.svg", code)
}

pub fn getCachedFlagSVG(countrycode: &str) -> String {
    let Some(code) = normalize_country_code(countrycode) else {
        return String::new();
    };
    match read_file(flag_path(code.as_str()).as_str()) {
        Ok(bytes) => String::from_utf8_lossy(bytes.as_slice()).into_owned(),
        Err(_) => String::new(),
    }
}

pub fn startFlagSVGFetch(countrycode: &str) -> u32 {
    let Some(code) = normalize_country_code(countrycode) else {
        return 0;
    };
    if !getCachedFlagSVG(code.as_str()).is_empty() {
        return 0;
    }

    let url = flag_url(code.as_str());
    let path = flag_path(code.as_str());
    unsafe { trueos_cabi_net_fetch_start(url.as_ptr(), url.len(), path.as_ptr(), path.len()) }
}

pub fn pollFlagSVGFetch(op_id: u32) -> i32 {
    if op_id == 0 {
        return 0;
    }
    unsafe { trueos_cabi_net_fetch_result(op_id) }
}

pub fn discardFlagSVGFetch(op_id: u32) {
    if op_id != 0 {
        let _ = unsafe { trueos_cabi_net_fetch_discard(op_id) };
    }
}

pub fn getFlagSVG(countrycode: &str) -> String {
    let cached = getCachedFlagSVG(countrycode);
    if !cached.is_empty() {
        return cached;
    }

    let op = startFlagSVGFetch(countrycode);
    if op == 0 {
        return String::new();
    }

    let wait_rc = unsafe { trueos_cabi_net_fetch_wait(op, 30_000) };
    if wait_rc != 0 {
        discardFlagSVGFetch(op);
        return String::new();
    }

    let result_rc = pollFlagSVGFetch(op);
    discardFlagSVGFetch(op);
    if result_rc != 0 {
        return String::new();
    }

    getCachedFlagSVG(countrycode)
}
