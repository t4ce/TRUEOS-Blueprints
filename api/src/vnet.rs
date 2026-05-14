extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::vcabi;

const FETCH_PENDING: i32 = -8;
const NET_ERR_BAD_URL: i32 = -10;
const NET_ERR_TIMEOUT: i32 = -11;
const NET_ERR_HTTP: i32 = -12;
const NET_ERR_TLS: i32 = -13;
const FS_ERR_TIMEOUT: i32 = -14;

pub fn fetch_bytes(url: &str, timeout_ms: u64) -> Result<Vec<u8>, String> {
    let op_id = unsafe { vcabi::trueos_cabi_net_fetch_bytes_start(url.as_ptr(), url.len()) };
    if op_id == 0 {
        return Err(String::from("fetch start failed"));
    }

    let wait_rc = unsafe { vcabi::trueos_cabi_net_fetch_bytes_wait(op_id, timeout_ms) };
    if wait_rc != 0 {
        discard_fetch(op_id);
        return Err(format!("{} ({})", fetch_code_name(wait_rc), wait_rc));
    }

    let len = unsafe { vcabi::trueos_cabi_net_fetch_bytes_result_len(op_id) };
    if len < 0 {
        discard_fetch(op_id);
        return Err(format!("{} ({})", fetch_code_name(len as i32), len));
    }

    let mut body = Vec::new();
    body.resize(len as usize, 0);
    let got =
        unsafe { vcabi::trueos_cabi_net_fetch_bytes_read(op_id, body.as_mut_ptr(), body.len()) };
    discard_fetch(op_id);
    if got < 0 {
        return Err(format!("{} ({})", fetch_code_name(got as i32), got));
    }

    body.truncate(got as usize);
    Ok(body)
}

pub fn fetch_text(url: &str, timeout_ms: u64) -> Result<String, String> {
    let body = fetch_bytes(url, timeout_ms)?;
    String::from_utf8(body).map_err(|_| String::from("bad utf8"))
}

fn discard_fetch(op_id: u32) {
    if op_id != 0 {
        let _ = unsafe { vcabi::trueos_cabi_net_fetch_bytes_discard(op_id) };
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
