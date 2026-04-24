use trueos::{bp_error, bp_info, vfetch, vsys};
use trueos_blueprint::tokio;

const URL: &[u8] = b"https://codeload.github.com/GitoxideLabs/gitoxide/tar.gz/refs/heads/main";
const OUT_PATH: &[u8] = b"clone_probe/gitoxide-main.tar.gz";
const WAIT_SLICE_MS: u64 = 250;
const WAIT_BUDGET_MS: u64 = 45_000;

fn main() {
    bp_info!("clone_probe: start");

    if let Err(err) = prove_gix_hash_layer() {
        bp_error!("clone_probe: gix-hash probe failed: {}", err);
        return;
    }

    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            bp_error!("clone_probe: runtime build failed: {}", err);
            return;
        }
    };

    rt.block_on(async {
        match clone_archive_probe().await {
            Ok(()) => bp_info!("clone_probe: done"),
            Err(err) => bp_error!("clone_probe: failed: {}", err),
        }
    });
}

fn prove_gix_hash_layer() -> Result<(), &'static str> {
    let empty_blob = gix::ObjectId::from_hex(b"e69de29bb2d1d6434b8b29ae775ad8c2e48c5391")
        .map_err(|_| "empty blob oid parse")?;

    if !empty_blob.is_empty_blob() {
        return Err("empty blob oid mismatch");
    }

    bp_info!(
        "clone_probe: gix-hash ok kind={:?} empty_blob={}",
        empty_blob.kind(),
        empty_blob
    );
    Ok(())
}

async fn clone_archive_probe() -> Result<(), &'static str> {
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
        tokio::task::yield_now().await;
    }
}
