use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[cfg(any(target_os = "trueos", test))]
mod animation;
#[cfg(any(target_os = "trueos", test))]
mod autobs;
#[cfg(test)]
#[path = "greeting/floor.rs"]
mod floor;
#[cfg(test)]
#[path = "greeting/floor_material.rs"]
mod floor_material;
#[cfg(target_os = "trueos")]
mod greeting;
#[cfg(any(target_os = "trueos", test))]
mod greeting_motion;
#[cfg(any(target_os = "trueos", test))]
mod hair;
#[cfg(any(target_os = "trueos", test))]
mod hair_body;
#[cfg(any(target_os = "trueos", test))]
mod hair_guides;
#[cfg(any(target_os = "trueos", test))]
mod hair_ribbon;
#[cfg(any(target_os = "trueos", test))]
mod idle_wind;
#[cfg(any(target_os = "trueos", test))]
mod light;
#[cfg(any(target_os = "trueos", test))]
mod mesh;
#[cfg(target_os = "trueos")]
mod preview;

const HUMANOID_FILE: &str = "AvatarSample_B.glb";
const HUMANOID_GLB: &[u8] = include_bytes!("../assets/AvatarSample_B.glb");
#[cfg(target_os = "trueos")]
const GREETING_FILE: &str = "AvatarSample_B_Greeting.glb";

#[derive(Deserialize)]
struct Artifact {
    name: String,
    size: usize,
    sha256: String,
}
#[derive(Deserialize)]
struct Model {
    id: String,
    repo: String,
    revision: String,
    files: Vec<Artifact>,
}

fn catalog() -> Result<Vec<Model>> {
    Ok(serde_json::from_str(include_str!("../models.json"))?)
}
fn verify(bytes: &[u8], file: &Artifact) -> Result<()> {
    if bytes.len() != file.size {
        bail!(
            "{}: expected {} bytes, got {}",
            file.name,
            file.size,
            bytes.len()
        );
    }
    if format!("{:x}", Sha256::digest(bytes)) != file.sha256 {
        bail!("{}: SHA256 mismatch", file.name);
    }
    Ok(())
}
fn directory(root: &Path, model: &Model) -> PathBuf {
    root.join(&model.id).join(&model.revision)
}
fn ready(root: &Path, model: &Model) -> Result<()> {
    let dir = directory(root, model);
    // A completion marker is published last; partial bundles are never ready.
    let marker = fs::read_to_string(dir.join("ready"))?;
    if marker != model.revision {
        bail!("completion marker mismatch");
    }
    for file in &model.files {
        verify(&fs::read(dir.join(&file.name))?, file)?;
    }
    Ok(())
}
async fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    #[cfg(target_os = "trueos")]
    {
        return trueos::async_fs::write_file(path.to_string_lossy().as_bytes(), bytes)
            .await
            .map_err(|code| anyhow::anyhow!("TRUEOSFS publish: {code}"));
    }
    #[cfg(not(target_os = "trueos"))]
    {
        // TRUEOSFS publishes whole-file records; do not depend on replace-rename.
        let mut file = fs::File::create(path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    }
}

fn lock_setup(root: &Path) -> Result<fs::File> {
    fs::create_dir_all(root)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("setup.lock"))?;
    lock.try_lock()
        .context("another emomodel setup owns the lock")?;
    Ok(lock)
}

async fn install_humanoid(root: &Path) -> Result<PathBuf> {
    let path = root.join(HUMANOID_FILE);
    if fs::metadata(&path).is_ok_and(|meta| meta.len() == HUMANOID_GLB.len() as u64)
        && fs::read(&path).is_ok_and(|bytes| bytes == HUMANOID_GLB)
    {
        return Ok(path);
    }

    #[cfg(target_os = "trueos")]
    write(&path, HUMANOID_GLB).await?;
    #[cfg(not(target_os = "trueos"))]
    {
        let staging = root.join(".AvatarSample_B.glb.tmp");
        write(&staging, HUMANOID_GLB).await?;
        fs::rename(staging, &path)?;
    }
    Ok(path)
}

fn status(root: &Path, models: &[Model]) {
    let active = fs::read_to_string(root.join("active.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
    for model in models {
        let state = match ready(root, model) {
            Ok(()) => "ready".to_owned(),
            Err(error) if directory(root, model).exists() => {
                format!("incomplete/invalid ({error})")
            }
            Err(_) => "absent".to_owned(),
        };
        let selected = active
            .as_ref()
            .is_some_and(|v| v["id"] == model.id && v["revision"] == model.revision);
        println!(
            "{}: {}{} — {} bytes — {}",
            model.id,
            state,
            if selected { " [selected]" } else { "" },
            model.files.iter().map(|f| f.size).sum::<usize>(),
            directory(root, model).display()
        );
    }
}

#[cfg(target_os = "trueos")]
async fn fetch(url: &str, limit: usize, _client: &reqwest::Client) -> Result<Vec<u8>> {
    use trueos::netfs;
    struct Download(u32);
    impl Drop for Download {
        fn drop(&mut self) {
            let _ = netfs::fetch_bytes_discard(self.0);
        }
    }
    let download = Download(
        {
            unsafe extern "C" {
                fn trueos_cabi_net_fetch_bytes_start_with_options_v1(
                    url: *const u8,
                    len: usize,
                    timeout: u32,
                    max: u64,
                ) -> u32;
            }
            let id = unsafe {
                trueos_cabi_net_fetch_bytes_start_with_options_v1(
                    url.as_ptr(),
                    url.len(),
                    90_000,
                    limit.max(1024 * 1024) as u64,
                )
            };
            if id == 0 { Err(-4) } else { Ok(id) }
        }
        .map_err(|code| anyhow::anyhow!("native fetch start: {code}"))?,
    );
    let start = Instant::now();
    loop {
        match netfs::fetch_bytes_result_len(download.0) {
            Ok(len) => {
                if len > limit {
                    bail!("native body exceeds expected size");
                }
                return netfs::fetch_bytes_read(download.0)
                    .map_err(|code| anyhow::anyhow!("native fetch read: {code}"));
            }
            Err(-8) if start.elapsed() < Duration::from_secs(95) => {
                std::thread::sleep(Duration::from_millis(25))
            }
            Err(code) => bail!("native fetch failed: {code}; elapsed {:?}", start.elapsed()),
        }
    }
}

#[cfg(not(target_os = "trueos"))]
async fn fetch(url: &str, limit: usize, client: &reqwest::Client) -> Result<Vec<u8>> {
    let mut response = client.get(url).send().await?.error_for_status()?;
    eprintln!("emomodel: response host / {url}: {}", response.status());
    let mut bytes = Vec::with_capacity(limit);
    let mut reported = 0;
    while let Some(chunk) = response.chunk().await? {
        if bytes.len().saturating_add(chunk.len()) > limit {
            bail!("oversized {}", url);
        }
        bytes.extend_from_slice(&chunk);
        if bytes.len() / (1024 * 1024) > reported {
            reported = bytes.len() / (1024 * 1024);
            eprintln!("emomodel: {}: {} / {} bytes", url, bytes.len(), limit);
        }
    }
    Ok(bytes)
}

async fn install(root: &Path, model: &Model, client: &reqwest::Client) -> Result<()> {
    if ready(root, model).is_ok() {
        return Ok(());
    }
    let dir = directory(root, model);
    fs::create_dir_all(&dir)?;
    for artifact in &model.files {
        if fs::read(dir.join(&artifact.name)).is_ok_and(|b| verify(&b, artifact).is_ok()) {
            continue;
        }
        let url = format!(
            "https://huggingface.co/{}/resolve/{}/{}?download=true",
            model.repo, model.revision, artifact.name
        );
        eprintln!("emomodel: fetching {} / {}", model.id, artifact.name);
        let bytes = fetch(&url, artifact.size, client).await?;
        eprintln!("emomodel: verifying {}", artifact.name);
        verify(&bytes, artifact)?;
        eprintln!("emomodel: writing {}", artifact.name);
        write(&dir.join(&artifact.name), &bytes).await?;
    }
    write(&dir.join("ready"), model.revision.as_bytes()).await?;
    ready(root, model)
}

async fn run() -> Result<()> {
    let mut root = PathBuf::from("/apps/emomodel");
    let mut positional = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => root = args.next().context("--root requires a directory")?.into(),
            "--help" | "-h" => {
                println!(
                    "emomodel [--root PATH] [show | greeting | base-color | status | setup [auto|8m|32m|4m] | mesh]\n\nBare launch on TRUEOS opens the body lighting study, then plays Nod and includes Greeting when installed locally.\nshow opens the static humanoid mesh in UI4.\ngreeting requires AvatarSample_B_Greeting.glb and runs the lighting study and both clips at a 45 Hz target.\nbase-color shows the animated avatar with the original 4096-pixel face and body color maps.\nHost launch defaults to status.\nmesh installs the embedded AvatarSample_B.glb offline.\nsetup also installs the mesh, then prepares the requested model bundle.\nsetup auto tries 8m, then 32m, then 4m.\n32m is the larger alternative; 4m is the smaller fallback, not claimed equally accurate.\nDownloads pinned weights, tokenizer, and config to /apps/emomodel.\nactive.json identifies the selected complete bundle; no inference service is started."
                );
                return Ok(());
            }
            _ if arg.starts_with('-') => bail!("unknown option {arg}"),
            _ => positional.push(arg),
        }
    }
    let default_command = if cfg!(target_os = "trueos") {
        "show"
    } else {
        "status"
    };
    match positional
        .first()
        .map(String::as_str)
        .unwrap_or(default_command)
    {
        command @ ("show" | "greeting" | "base-color") if positional.len() <= 1 => {
            #[cfg(target_os = "trueos")]
            {
                if command == "show" && !positional.is_empty() {
                    return preview::show(HUMANOID_GLB);
                }
                let clip = root.join(GREETING_FILE);
                let data = if command == "greeting" || clip.is_file() {
                    Some(
                        fs::read(&clip)
                            .with_context(|| format!("read Greeting from {}", clip.display()))?,
                    )
                } else {
                    None
                };
                let motions = positional.is_empty().then(|| root.join("motions"));
                return greeting::show(
                    HUMANOID_GLB,
                    data.as_deref(),
                    motions.as_deref(),
                    command == "base-color",
                )
                .await;
            }
            #[cfg(not(target_os = "trueos"))]
            bail!("{command} requires TRUEOS UI4");
        }
        "status" if positional.len() <= 1 => {
            status(&root, &catalog()?);
            return Ok(());
        }
        "mesh" if positional.len() == 1 => {
            let _lock = lock_setup(&root)?;
            let path = install_humanoid(&root).await?;
            println!("emomodel: mesh ready at {}", path.display());
            return Ok(());
        }
        "setup" if positional.len() <= 2 => (),
        _ => bail!("use show, greeting, base-color, status, setup [auto|8m|32m|4m], or mesh"),
    }
    let models = catalog()?;
    let choice = positional.get(1).map(String::as_str).unwrap_or("auto");
    if choice != "auto" && !models.iter().any(|m| m.id == choice) {
        bail!("unknown model {choice}");
    }
    let _lock = lock_setup(&root)?;
    install_humanoid(&root).await?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(300))
        .https_only(true)
        .build()?;
    // Prefer a complete installed bundle before going to the network in auto mode.
    let candidates: Vec<_> = if choice == "auto" {
        let existing = models.iter().find(|m| ready(&root, m).is_ok());
        if let Some(model) = existing {
            vec![model]
        } else {
            models.iter().collect()
        }
    } else {
        models.iter().filter(|m| m.id == choice).collect()
    };
    let start = Instant::now();
    let mut failures = Vec::new();
    for model in candidates {
        match install(&root, model, &client).await {
            Ok(()) => {
                let manifest = serde_json::json!({"id": model.id, "repo": model.repo, "revision": model.revision,
                    "directory": directory(&root, model), "files": ["model.safetensors", "tokenizer.json", "config.json"]});
                write(
                    &root.join("active.json"),
                    &serde_json::to_vec_pretty(&manifest)?,
                )
                .await?;
                println!(
                    "emomodel: ready {} in {:.3}s; model directory: {}",
                    model.id,
                    start.elapsed().as_secs_f64(),
                    directory(&root, model).display()
                );
                status(&root, &models);
                return Ok(());
            }
            Err(error) => {
                eprintln!("emomodel: {} failed: {error:#}", model.id);
                failures.push(format!("{}: {error:#}", model.id));
            }
        }
    }
    bail!("no model installed: {}", failures.join("; "))
}

fn main() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn artifact_integrity_rejects_truncation_and_same_size_corruption() {
        let file = Artifact {
            name: "fixture".into(),
            size: 3,
            sha256: format!("{:x}", Sha256::digest(b"abc")),
        };
        assert!(verify(b"abc", &file).is_ok());
        assert!(verify(b"ab", &file).is_err());
        assert!(verify(b"abd", &file).is_err());
    }
    #[test]
    fn pinned_catalog_has_three_complete_bundles() {
        let models = catalog().unwrap();
        assert_eq!(models.len(), 3);
        for model in models {
            assert_eq!(model.revision.len(), 40);
            for name in ["model.safetensors", "tokenizer.json", "config.json"] {
                assert!(
                    model
                        .files
                        .iter()
                        .any(|f| f.name == name && f.sha256.len() == 64 && f.size > 0)
                );
            }
        }
    }
}
