#![no_std]
#![allow(non_snake_case)]

extern crate alloc;

use alloc::{format, string::String, vec::Vec};

unsafe extern "C" {
    fn trueos_cabi_net_fetch_bytes_start(url_ptr: *const u8, url_len: usize) -> u32;
    fn trueos_cabi_net_fetch_bytes_wait(op_id: u32, timeout_ms: u64) -> i32;
    fn trueos_cabi_net_fetch_bytes_result_len(op_id: u32) -> isize;
    fn trueos_cabi_net_fetch_bytes_read(op_id: u32, out_ptr: *mut u8, out_cap: usize) -> isize;
    fn trueos_cabi_net_fetch_bytes_discard(op_id: u32) -> i32;
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

fn flag_url(code: &str) -> String {
    format!("https://flagcdn.com/{}.svg", code)
}

pub fn getCachedFlagSVG(countrycode: &str) -> String {
    let _ = countrycode;
    String::new()
}

pub fn startFlagSVGFetch(countrycode: &str) -> u32 {
    let Some(code) = normalize_country_code(countrycode) else {
        return 0;
    };
    let url = flag_url(code.as_str());
    unsafe { trueos_cabi_net_fetch_bytes_start(url.as_ptr(), url.len()) }
}

pub fn pollFlagSVGFetch(op_id: u32) -> i32 {
    if op_id == 0 {
        return 0;
    }
    let rc = unsafe { trueos_cabi_net_fetch_bytes_result_len(op_id) };
    if rc > i32::MAX as isize {
        i32::MAX
    } else if rc < i32::MIN as isize {
        i32::MIN
    } else {
        rc as i32
    }
}

pub fn readFlagSVGFetch(op_id: u32) -> String {
    if op_id == 0 {
        return String::new();
    }
    let len = unsafe { trueos_cabi_net_fetch_bytes_result_len(op_id) };
    if len <= 0 {
        return String::new();
    }

    let mut out = Vec::new();
    out.resize(len as usize, 0);
    let got = unsafe { trueos_cabi_net_fetch_bytes_read(op_id, out.as_mut_ptr(), out.len()) };
    if got <= 0 {
        return String::new();
    }
    out.truncate(got as usize);
    String::from_utf8_lossy(out.as_slice()).into_owned()
}

pub fn discardFlagSVGFetch(op_id: u32) {
    if op_id != 0 {
        let _ = unsafe { trueos_cabi_net_fetch_bytes_discard(op_id) };
    }
}

pub fn getFlagSVG(countrycode: &str) -> String {
    let op = startFlagSVGFetch(countrycode);
    if op == 0 {
        return String::new();
    }

    let wait_rc = unsafe { trueos_cabi_net_fetch_bytes_wait(op, 30_000) };
    if wait_rc != 0 {
        discardFlagSVGFetch(op);
        return String::new();
    }

    let len = pollFlagSVGFetch(op);
    if len <= 0 {
        discardFlagSVGFetch(op);
        return String::new();
    }

    readFlagSVGFetch(op)
}
