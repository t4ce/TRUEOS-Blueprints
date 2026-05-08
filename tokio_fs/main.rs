use std::io::SeekFrom;
use std::path::Path;

use trueos_blueprint::{bp_error, bp_info, fs, io, runtime, tokio};

const PROBE_PATH: &str = "blueprint-tokio-fs-probe.txt";
const PROBE_DIR: &str = "blueprint-tokio-fs-dir";
const PROBE_NESTED_PATH: &str = "blueprint-tokio-fs-dir/nested/probe.txt";
const PROBE_BYTES: &[u8] = b"TRUEOS blueprint tokio::fs probe\n";
const PROBE_REWRITE_BYTES: &[u8] = b"TRUEOS blueprint tokio::fs handle rewrite\n";

fn main() {
    bp_info!("tokio_fs: start");

    if let Err(stage) = probe_runtime_bootstrap_surfaces() {
        bp_error!("tokio_fs: bootstrap failed stage={}", stage);
        return;
    }

    bp_info!("tokio_fs: stage runtime.current_thread.build");
    let runtime = match runtime::current_thread().build() {
        Ok(rt) => rt,
        Err(err) => {
            bp_error!("tokio_fs: runtime build failed: {}", err);
            return;
        }
    };
    bp_info!("tokio_fs: success runtime.current_thread.build");

    runtime.block_on(async {
        match run_probe().await {
            Ok(()) => bp_info!("tokio_fs: done"),
            Err(stage) => bp_error!("tokio_fs: failed stage={}", stage),
        }
    });
}

fn probe_runtime_bootstrap_surfaces() -> Result<(), &'static str> {
    bp_info!("tokio_fs: stage thread.current.id");
    let thread_id = std::thread::current().id();
    bp_info!("tokio_fs: success thread.current.id id={:?}", thread_id);

    bp_info!("tokio_fs: stage thread.yield_now");
    std::thread::yield_now();
    bp_info!("tokio_fs: success thread.yield_now");

    bp_info!("tokio_fs: stage runtime.current_thread.builder_new_plain");
    let mut builder = tokio::runtime::Builder::new_current_thread();
    bp_info!("tokio_fs: success runtime.current_thread.builder_new_plain");

    bp_info!("tokio_fs: stage runtime.current_thread.builder_build_plain");
    let runtime = builder
        .build()
        .map_err(|_| "runtime.current_thread.builder_build_plain")?;
    bp_info!("tokio_fs: success runtime.current_thread.build_plain");

    bp_info!("tokio_fs: stage runtime.current_thread.drop_plain");
    drop(runtime);
    bp_info!("tokio_fs: success runtime.current_thread.drop_plain");

    bp_info!("tokio_fs: stage runtime.current_thread.build_time");
    let runtime = runtime::current_thread()
        .build()
        .map_err(|_| "runtime.current_thread.build_time")?;
    bp_info!("tokio_fs: success runtime.current_thread.build_time");

    bp_info!("tokio_fs: stage runtime.current_thread.drop_time");
    drop(runtime);
    bp_info!("tokio_fs: success runtime.current_thread.drop_time");

    Ok(())
}

async fn run_probe() -> Result<(), &'static str> {
    let _ = tokio::fs::remove_file(PROBE_PATH).await;
    let _ = tokio::fs::remove_file(PROBE_NESTED_PATH).await;

    bp_info!("tokio_fs: stage fs.write");
    fs::write(PROBE_PATH, PROBE_BYTES)
        .await
        .map_err(|_| "fs.write")?;
    bp_info!("tokio_fs: success fs.write");

    bp_info!("tokio_fs: stage fs.read");
    let bytes = fs::read(PROBE_PATH).await.map_err(|_| "fs.read")?;
    if bytes.as_slice() != PROBE_BYTES {
        return Err("fs.read.value");
    }
    bp_info!("tokio_fs: success fs.read len={}", bytes.len());

    bp_info!("tokio_fs: stage fs.read_to_string");
    let text = fs::read_to_string(PROBE_PATH)
        .await
        .map_err(|_| "fs.read_to_string")?;
    if text.as_bytes() != PROBE_BYTES {
        return Err("fs.read_to_string.value");
    }
    bp_info!("tokio_fs: success fs.read_to_string len={}", text.len());

    bp_info!("tokio_fs: stage fs.open_options.surface");
    let _options = fs::OpenOptions::new();
    bp_info!("tokio_fs: success fs.open_options.surface");

    bp_info!("tokio_fs: stage fs.open_options.file_write_flush");
    probe_file_write_flush().await?;
    bp_info!("tokio_fs: success fs.open_options.file_write_flush");

    bp_info!("tokio_fs: stage fs.file.read_to_end");
    probe_file_read_to_end().await?;
    bp_info!("tokio_fs: success fs.file.read_to_end");

    bp_info!("tokio_fs: stage fs.file.seek_rewrite_flush");
    probe_file_seek_rewrite_flush().await?;
    bp_info!("tokio_fs: success fs.file.seek_rewrite_flush");

    bp_info!("tokio_fs: stage fs.try_exists");
    let exists = fs::try_exists(PROBE_PATH)
        .await
        .map_err(|_| "fs.try_exists")?;
    if !exists {
        return Err("fs.try_exists.false");
    }
    bp_info!("tokio_fs: success fs.try_exists");

    bp_info!("tokio_fs: stage fs.stat.file");
    let file_stat = fs::stat(PROBE_PATH.as_bytes()).map_err(|_| "fs.stat.file")?;
    if file_stat.kind != fs::FsNodeKind::File || file_stat.len != PROBE_BYTES.len() as u64 {
        return Err("fs.stat.file.value");
    }
    bp_info!("tokio_fs: success fs.stat.file len={}", file_stat.len);

    bp_info!("tokio_fs: stage fs.create_dir_all.nested_write_read");
    probe_create_dir_all_nested_write_read().await?;
    bp_info!("tokio_fs: success fs.create_dir_all.nested_write_read");

    bp_info!("tokio_fs: stage fs.stat.dir");
    let dir_stat =
        fs::stat(format!("{}/nested", PROBE_DIR).as_bytes()).map_err(|_| "fs.stat.dir")?;
    if dir_stat.kind != fs::FsNodeKind::Directory || dir_stat.len != 0 {
        return Err("fs.stat.dir.value");
    }
    bp_info!("tokio_fs: success fs.stat.dir");

    bp_info!("tokio_fs: stage fs.canonicalize.trueos");
    let canonical = fs::canonicalize(format!("{}/./nested/../nested/probe.txt", PROBE_DIR))
        .await
        .map_err(|_| "fs.canonicalize.trueos")?;
    if canonical != Path::new("/").join(PROBE_NESTED_PATH) {
        return Err("fs.canonicalize.trueos.value");
    }
    bp_info!(
        "tokio_fs: success fs.canonicalize.trueos path={}",
        canonical.display()
    );

    bp_info!("tokio_fs: stage fs.remove_file");
    tokio::fs::remove_file(PROBE_PATH)
        .await
        .map_err(|_| "fs.remove_file")?;
    let _ = tokio::fs::remove_file(PROBE_NESTED_PATH).await;
    bp_info!("tokio_fs: success fs.remove_file");

    Ok(())
}

async fn probe_create_dir_all_nested_write_read() -> Result<(), &'static str> {
    fs::create_dir_all(format!("{}/nested", PROBE_DIR))
        .await
        .map_err(|_| "fs.create_dir_all")?;

    fs::write(PROBE_NESTED_PATH, PROBE_BYTES)
        .await
        .map_err(|_| "fs.nested.write")?;

    let exists = fs::try_exists(PROBE_NESTED_PATH)
        .await
        .map_err(|_| "fs.nested.try_exists")?;
    if !exists {
        return Err("fs.nested.try_exists.false");
    }

    let bytes = fs::read(PROBE_NESTED_PATH)
        .await
        .map_err(|_| "fs.nested.read")?;
    if bytes.as_slice() != PROBE_BYTES {
        return Err("fs.nested.read.value");
    }

    Ok(())
}

async fn probe_file_write_flush() -> Result<(), &'static str> {
    use io::AsyncWriteExt;

    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .truncate(true)
        .open(PROBE_PATH)
        .await
        .map_err(|_| "fs.open_options.open")?;

    file.write_all(PROBE_REWRITE_BYTES)
        .await
        .map_err(|_| "fs.file.write_all")?;
    file.flush().await.map_err(|_| "fs.file.flush")?;
    Ok(())
}

async fn probe_file_read_to_end() -> Result<(), &'static str> {
    use io::AsyncReadExt;

    let mut file = fs::File::open(PROBE_PATH)
        .await
        .map_err(|_| "fs.file.open")?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .await
        .map_err(|_| "fs.file.read_to_end")?;
    if bytes.as_slice() != PROBE_REWRITE_BYTES {
        return Err("fs.file.read_to_end.value");
    }
    Ok(())
}

async fn probe_file_seek_rewrite_flush() -> Result<(), &'static str> {
    use io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(PROBE_PATH)
        .await
        .map_err(|_| "fs.file.seek_open")?;

    file.seek(SeekFrom::Start(0))
        .await
        .map_err(|_| "fs.file.seek_start")?;
    file.write_all(PROBE_BYTES)
        .await
        .map_err(|_| "fs.file.seek_write_all")?;
    file.set_len(PROBE_BYTES.len() as u64)
        .await
        .map_err(|_| "fs.file.seek_set_len")?;
    file.flush().await.map_err(|_| "fs.file.seek_flush")?;
    drop(file);

    let mut file = fs::File::open(PROBE_PATH)
        .await
        .map_err(|_| "fs.file.seek_verify_open")?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .await
        .map_err(|_| "fs.file.seek_verify_read_to_end")?;
    if bytes.as_slice() != PROBE_BYTES {
        return Err("fs.file.seek_rewrite.value");
    }
    Ok(())
}
