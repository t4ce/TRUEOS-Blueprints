//! Load the pinned checkpoint before starting inference.
use crate::bundle::{MODEL_BYTES, MODEL_SHA256};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};

#[cfg(target_os = "trueos")]
const MODEL_PATH: &str = "userdata/models/english_2026-10/model.safetensors";
#[cfg(not(target_os = "trueos"))]
const MODEL_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/models/model.safetensors");
#[cfg(target_os = "trueos")]
const SEED_URL: &str = "http://192.168.178.111:8787/model.safetensors";

pub fn load() -> Result<Vec<u8>> {
    // Let Ubuntu runs use a local checkpoint outside the source tree.
    #[cfg(not(target_os = "trueos"))]
    let override_path = std::env::var_os("PTTS_MODEL_PATH");
    #[cfg(not(target_os = "trueos"))]
    let path = override_path
        .as_deref()
        .map_or(Path::new(MODEL_PATH), Path::new);
    #[cfg(target_os = "trueos")]
    let path = Path::new(MODEL_PATH);

    match File::open(path) {
        Ok(mut file) => {
            let bytes = read_local(&mut file)
                .with_context(|| format!("invalid local checkpoint {}", path.display()))?;
            crate::notice(format!("ptts: using local checkpoint {}", path.display()));
            Ok(bytes)
        }
        #[cfg(target_os = "trueos")]
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fetch_and_save(),
        Err(error) => Err(error).with_context(|| format!("open checkpoint {}", path.display())),
    }
}

fn read_local(file: &mut File) -> Result<Vec<u8>> {
    let size = file.metadata()?.len();
    ensure!(
        size == MODEL_BYTES as u64,
        "expected {MODEL_BYTES} bytes, got {size}"
    );
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(MODEL_BYTES)
        .context("allocate checkpoint buffer")?;
    bytes.resize(MODEL_BYTES, 0);
    file.read_exact(&mut bytes)?;
    ensure!(file.read(&mut [0])? == 0, "checkpoint grew while reading");
    verify(&bytes)?;
    Ok(bytes)
}

fn verify(bytes: &[u8]) -> Result<()> {
    ensure!(
        bytes.len() == MODEL_BYTES,
        "expected {MODEL_BYTES} bytes, got {}",
        bytes.len()
    );
    ensure!(
        format!("{:x}", Sha256::digest(bytes)) == MODEL_SHA256,
        "checkpoint SHA-256 mismatch"
    );
    Ok(())
}

#[cfg(target_os = "trueos")]
fn fetch_and_save() -> Result<Vec<u8>> {
    use trueos::{async_fs, netfs};

    struct Download(u32);
    impl Drop for Download {
        fn drop(&mut self) {
            let _ = netfs::fetch_bytes_discard(self.0);
        }
    }

    let parent = MODEL_PATH.rsplit_once('/').expect("checkpoint directory").0;
    async_fs::block_on(async_fs::create_dir_all(parent.as_bytes()))
        .map_err(|code| anyhow::anyhow!("create ptts checkpoint directory ({code})"))?;
    let url = std::env::var("PTTS_MODEL_URL").unwrap_or_else(|_| SEED_URL.to_owned());
    crate::notice(format!("ptts: checkpoint missing, fetching {url}"));
    const TIMEOUT_MS: u32 = 90_000;
    let download = Download(
        netfs::fetch_bytes_with_options(url.as_bytes(), TIMEOUT_MS, MODEL_BYTES)
            .map_err(|code| anyhow::anyhow!("start checkpoint fetch ({code})"))?,
    );
    let status = netfs::fetch_bytes_wait(download.0, u64::from(TIMEOUT_MS));
    ensure!(status == 0, "checkpoint fetch from {url} failed ({status})");
    let bytes = netfs::fetch_bytes_read_bounded(download.0, MODEL_BYTES)
        .map_err(|code| anyhow::anyhow!("read checkpoint download ({code})"))?;
    verify(&bytes).context("invalid downloaded checkpoint")?;
    async_fs::block_on(async_fs::write_file_typed_checked(
        MODEL_PATH.as_bytes(),
        &bytes,
        async_fs::ContentTypeId::BLOB,
        true,
        None,
    ))
    .map_err(|code| anyhow::anyhow!("save checkpoint at {MODEL_PATH} ({code})"))?;
    crate::notice(format!("ptts: saved verified checkpoint {MODEL_PATH}"));
    Ok(bytes)
}
