extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

unsafe extern "C" {
    fn trueos_cabi_net_fetch_bytes_start(url_ptr: *const u8, url_len: usize) -> u32;
    fn trueos_cabi_net_fetch_bytes_wait(op_id: u32, timeout_ms: u64) -> i32;
    fn trueos_cabi_net_fetch_bytes_result_len(op_id: u32) -> isize;
    fn trueos_cabi_net_fetch_bytes_read(op_id: u32, out_ptr: *mut u8, out_cap: usize) -> isize;
    fn trueos_cabi_net_fetch_bytes_discard(op_id: u32) -> i32;
}

const FETCH_PENDING: i32 = -8;
const NET_ERR_BAD_URL: i32 = -10;
const NET_ERR_TIMEOUT: i32 = -11;
const NET_ERR_HTTP: i32 = -12;
const NET_ERR_TLS: i32 = -13;
const FS_ERR_TIMEOUT: i32 = -14;

pub fn fetch_text(url: &str, timeout_ms: u64) -> Result<String, String> {
    let op_id = unsafe { trueos_cabi_net_fetch_bytes_start(url.as_ptr(), url.len()) };
    if op_id == 0 {
        return Err(String::from("fetch start failed"));
    }

    let wait_rc = unsafe { trueos_cabi_net_fetch_bytes_wait(op_id, timeout_ms) };
    if wait_rc != 0 {
        discard_fetch(op_id);
        return Err(format!("{} ({})", fetch_code_name(wait_rc), wait_rc));
    }

    let len = unsafe { trueos_cabi_net_fetch_bytes_result_len(op_id) };
    if len < 0 {
        discard_fetch(op_id);
        return Err(format!("{} ({})", fetch_code_name(len as i32), len));
    }

    let mut body = Vec::new();
    body.resize(len as usize, 0);
    let got = unsafe { trueos_cabi_net_fetch_bytes_read(op_id, body.as_mut_ptr(), body.len()) };
    discard_fetch(op_id);
    if got < 0 {
        return Err(format!("{} ({})", fetch_code_name(got as i32), got));
    }

    body.truncate(got as usize);
    String::from_utf8(body).map_err(|_| String::from("bad utf8"))
}

fn discard_fetch(op_id: u32) {
    if op_id != 0 {
        let _ = unsafe { trueos_cabi_net_fetch_bytes_discard(op_id) };
    }
}

fn fetch_code_name(code: i32) -> &'static str {
    match code {
        0 => "OK",
        FETCH_PENDING => "FETCH_PENDING",
        NET_ERR_BAD_URL => "NET_ERR_BAD_URL",
        NET_ERR_TIMEOUT => "NET_ERR_TIMEOUT",
        NET_ERR_HTTP => "NET_ERR_HTTP",
        NET_ERR_TLS => "NET_ERR_TLS",
        FS_ERR_TIMEOUT => "FS_ERR_TIMEOUT",
        _ => "UNKNOWN",
    }
}