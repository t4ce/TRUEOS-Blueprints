extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use v::vnetfs;

const FETCH_PENDING: i32 = -8;
const NET_ERR_BAD_URL: i32 = -10;
const NET_ERR_TIMEOUT: i32 = -11;
const NET_ERR_HTTP: i32 = -12;
const NET_ERR_TLS: i32 = -13;
const FS_ERR_TIMEOUT: i32 = -14;

pub fn fetch_bytes(url: &str, timeout_ms: u64) -> Result<Vec<u8>, String> {
    let op_id = vnetfs::fetch_bytes(url.as_bytes()).map_err(fetch_error_string)?;

    let wait_rc = vnetfs::fetch_bytes_wait(op_id, timeout_ms);
    if wait_rc != 0 {
        discard_fetch(op_id);
        return Err(format!("{} ({})", fetch_code_name(wait_rc), wait_rc));
    }

    let len = match vnetfs::fetch_bytes_result_len(op_id) {
        Ok(len) => len,
        Err(code) => {
            discard_fetch(op_id);
            return Err(fetch_error_string(code));
        }
    };
    if len == 0 {
        discard_fetch(op_id);
        return Ok(Vec::new());
    }

    let mut body = Vec::new();
    body.resize(len, 0);
    let got = vnetfs::fetch_bytes_read(op_id).map_err(fetch_error_string)?;
    discard_fetch(op_id);
    Ok(got)
}

pub fn fetch_text(url: &str, timeout_ms: u64) -> Result<String, String> {
    let body = fetch_bytes(url, timeout_ms)?;
    String::from_utf8(body).map_err(|_| String::from("bad utf8"))
}

fn discard_fetch(op_id: u32) {
    if op_id != 0 {
        let _ = vnetfs::fetch_bytes_discard(op_id);
    }
}

fn fetch_error_string(code: i32) -> String {
    format!("{} ({})", fetch_code_name(code), code)
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
