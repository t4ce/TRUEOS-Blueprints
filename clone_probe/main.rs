#![no_std]
#![no_main]

use core::format_args;
use trueos::compat::vfetch;
use trueos::{bp_error, bp_info, vsys};

const EMPTY_BLOB_OID_HEX: &[u8; 40] = b"e69de29bb2d1d6434b8b29ae775ad8c2e48c5391";
const EMPTY_BLOB_OID_RAW: [u8; 20] = [
    0xe6, 0x9d, 0xe2, 0x9b, 0xb2, 0xd1, 0xd6, 0x43, 0x4b, 0x8b, 0x29, 0xae, 0x77, 0x5a, 0xd8, 0xc2,
    0xe4, 0x8c, 0x53, 0x91,
];
const URL: &[u8] = b"https://codeload.github.com/GitoxideLabs/gitoxide/tar.gz/refs/heads/main";
const OUT_PATH: &[u8] = b"clone_probe/gitoxide-main.tar.gz";
const WAIT_SLICE_MS: u64 = 250;
const WAIT_BUDGET_MS: u64 = 45_000;

#[unsafe(no_mangle)]
pub extern "C" fn main() {
    bp_info!("clone_probe: start");

    if let Err(err) = prove_git_oid_literal() {
        bp_error!("clone_probe: git oid probe failed: {}", err);
        return;
    }

    match clone_archive_probe() {
        Ok(()) => bp_info!("clone_probe: done"),
        Err(err) => bp_error!("clone_probe: failed: {}", err),
    }
}

fn prove_git_oid_literal() -> Result<(), &'static str> {
    let mut empty_blob = [0u8; EMPTY_BLOB_OID_RAW.len()];
    decode_hex_oid(EMPTY_BLOB_OID_HEX, &mut empty_blob)?;

    if empty_blob != EMPTY_BLOB_OID_RAW {
        return Err("empty blob oid mismatch");
    }

    bp_info!(
        "clone_probe: git oid ok len={} hex={}",
        empty_blob.len(),
        core::str::from_utf8(EMPTY_BLOB_OID_HEX).unwrap_or("<bad-oid>")
    );
    Ok(())
}

fn decode_hex_oid(input: &[u8], output: &mut [u8; 20]) -> Result<(), &'static str> {
    if input.len() != output.len() * 2 {
        return Err("empty blob oid length");
    }

    let mut index = 0usize;
    while index < output.len() {
        let hi = hex_nibble(input[index * 2])?;
        let lo = hex_nibble(input[index * 2 + 1])?;
        output[index] = (hi << 4) | lo;
        index += 1;
    }

    Ok(())
}

fn hex_nibble(byte: u8) -> Result<u8, &'static str> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err("empty blob oid digit"),
    }
}

fn clone_archive_probe() -> Result<(), &'static str> {
    bp_info!(
        "clone_probe: fetch start url={} out={}",
        core::str::from_utf8(URL).unwrap_or("<bad-url>"),
        core::str::from_utf8(OUT_PATH).unwrap_or("<bad-path>")
    );

    let prewarm = vfetch::prewarm_url(URL);
    bp_info!("clone_probe: prewarm rc={}", prewarm);

    let op_id = vfetch::fetch_to_file(URL, OUT_PATH).map_err(|_| "fetch start")?;
    bp_info!("clone_probe: fetch op_id={}", op_id);

    let mut waited = 0u64;
    loop {
        let rc = vfetch::fetch_wait(op_id, WAIT_SLICE_MS);
        if rc == 0 {
            bp_info!(
                "clone_probe: fetch ok path={} waited_ms={}",
                core::str::from_utf8(OUT_PATH).unwrap_or("<bad-path>"),
                waited
            );
            return Ok(());
        }
        if rc < 0 {
            let _ = vfetch::fetch_discard(op_id);
            bp_error!("clone_probe: fetch error rc={}", rc);
            return Err("fetch failed");
        }

        waited = waited.saturating_add(WAIT_SLICE_MS);
        if waited >= WAIT_BUDGET_MS {
            let latest = vfetch::fetch_result(op_id);
            let _ = vfetch::fetch_discard(op_id);
            bp_error!("clone_probe: fetch timeout latest_rc={}", latest);
            return Err("fetch timeout");
        }

        vsys::poll_once();
    }
}
