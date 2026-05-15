#![no_std]
#![allow(non_snake_case)]

extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use v::vnetfs;

const NET_ERR_BAD_URL: i32 = -10;

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
    vnetfs::fetch_bytes(url.as_bytes()).unwrap_or(0)
}

pub fn pollFlagSVGFetch(op_id: u32) -> i32 {
    if op_id == 0 {
        return 0;
    }
    vnetfs::fetch_bytes_result_len(op_id)
        .map(|len| len.min(i32::MAX as usize) as i32)
        .unwrap_or_else(|rc| rc)
}

pub fn readFlagSVGFetch(op_id: u32) -> String {
    if op_id == 0 {
        return String::new();
    }
    let Ok(len) = vnetfs::fetch_bytes_result_len(op_id) else {
        return String::new();
    };
    if len == 0 {
        return String::new();
    }

    let Ok(mut out) = vnetfs::fetch_bytes_read(op_id) else {
        return String::new();
    };
    out.truncate(len);
    String::from_utf8_lossy(out.as_slice()).into_owned()
}

pub fn discardFlagSVGFetch(op_id: u32) {
    if op_id != 0 {
        let _ = vnetfs::fetch_bytes_discard(op_id);
    }
}

pub fn getFlagSVG(countrycode: &str) -> String {
    let op = startFlagSVGFetch(countrycode);
    if op == 0 {
        return String::new();
    }

    let wait_rc = vnetfs::fetch_bytes_wait(op, 30_000);
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
