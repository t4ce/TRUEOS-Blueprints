use trueos::{
    logl::{self, level},
    platform::{
        format,
        io::SeekFrom,
        path::Path,
        thread,
        Vec,
    },
    t,
};

const PROBE_PATH: &str = "blueprint-tokio-fs-probe.txt";
const PROBE_DIR: &str = "blueprint-tokio-fs-dir";
const PROBE_NESTED_PATH: &str = "blueprint-tokio-fs-dir/nested/probe.txt";
const PROBE_BYTES: &[u8] = b"TRUEOS blueprint tokio::fs probe\n";
const PROBE_REWRITE_BYTES: &[u8] = b"TRUEOS blueprint tokio::fs handle rewrite\n";

fn main() {
    logl::log(level::INFO, format_args!("tokio_fs: start"));

    if let Err(stage) = probe_runtime_bootstrap_surfaces() {
        logl::log(level::ERROR, format_args!("tokio_fs: bootstrap failed stage={}", stage));
        return;
    }

    logl::log(level::INFO, format_args!("tokio_fs: stage runtime.current_thread.build"));
    let runtime = match t::runtime::current_thread().build() {
        Ok(rt) => rt,
        Err(err) => {
            logl::log(level::ERROR, format_args!("tokio_fs: runtime build failed: {}", err));
            return;
        }
    };
    logl::log(level::INFO, format_args!("tokio_fs: success runtime.current_thread.build"));

    runtime.block_on(async {
        match run_probe().await {
            Ok(()) => logl::log(level::INFO, format_args!("tokio_fs: done")),
            Err(stage) => logl::log(level::ERROR, format_args!("tokio_fs: failed stage={}", stage)),
        }
    });
}

fn probe_runtime_bootstrap_surfaces() -> Result<(), &'static str> {
    logl::log(level::INFO, format_args!("tokio_fs: stage thread.current.id"));
    let thread_id = thread::current().id();
    logl::log(level::INFO, format_args!("tokio_fs: success thread.current.id id={:?}", thread_id));

    logl::log(level::INFO, format_args!("tokio_fs: stage thread.yield_now"));
    thread::yield_now();
    logl::log(level::INFO, format_args!("tokio_fs: success thread.yield_now"));

    logl::log(level::INFO, format_args!("tokio_fs: stage runtime.current_thread.builder_new_plain"));
    let mut builder = t::tokio::runtime::Builder::new_current_thread();
    logl::log(level::INFO, format_args!("tokio_fs: success runtime.current_thread.builder_new_plain"));

    logl::log(level::INFO, format_args!("tokio_fs: stage runtime.current_thread.builder_build_plain"));
    let runtime = builder
        .build()
        .map_err(|_| "runtime.current_thread.builder_build_plain")?;
    logl::log(level::INFO, format_args!("tokio_fs: success runtime.current_thread.build_plain"));

    logl::log(level::INFO, format_args!("tokio_fs: stage runtime.current_thread.drop_plain"));
    drop(runtime);
    logl::log(level::INFO, format_args!("tokio_fs: success runtime.current_thread.drop_plain"));

    logl::log(level::INFO, format_args!("tokio_fs: stage runtime.current_thread.build_time"));
    let runtime = t::runtime::current_thread()
        .build()
        .map_err(|_| "runtime.current_thread.build_time")?;
    logl::log(level::INFO, format_args!("tokio_fs: success runtime.current_thread.build_time"));

    logl::log(level::INFO, format_args!("tokio_fs: stage runtime.current_thread.drop_time"));
    drop(runtime);
    logl::log(level::INFO, format_args!("tokio_fs: success runtime.current_thread.drop_time"));

    Ok(())
}

async fn run_probe() -> Result<(), &'static str> {
    let _ = t::tokio::fs::remove_file(PROBE_PATH).await;
    let _ = t::tokio::fs::remove_file(PROBE_NESTED_PATH).await;

    logl::log(level::INFO, format_args!("tokio_fs: stage fs.write"));
    t::fs::write(PROBE_PATH, PROBE_BYTES)
        .await
        .map_err(|_| "fs.write")?;
    logl::log(level::INFO, format_args!("tokio_fs: success fs.write"));

    logl::log(level::INFO, format_args!("tokio_fs: stage fs.read"));
    let bytes = t::fs::read(PROBE_PATH).await.map_err(|_| "fs.read")?;
    if bytes.as_slice() != PROBE_BYTES {
        return Err("fs.read.value");
    }
    logl::log(level::INFO, format_args!("tokio_fs: success fs.read len={}", bytes.len()));

    logl::log(level::INFO, format_args!("tokio_fs: stage fs.read_to_string"));
    let text = t::fs::read_to_string(PROBE_PATH)
        .await
        .map_err(|_| "fs.read_to_string")?;
    if text.as_bytes() != PROBE_BYTES {
        return Err("fs.read_to_string.value");
    }
    logl::log(level::INFO, format_args!("tokio_fs: success fs.read_to_string len={}", text.len()));

    logl::log(level::INFO, format_args!("tokio_fs: stage fs.open_options.surface"));
    let _options = t::fs::OpenOptions::new();
    logl::log(level::INFO, format_args!("tokio_fs: success fs.open_options.surface"));

    logl::log(level::INFO, format_args!("tokio_fs: stage fs.open_options.file_write_flush"));
    probe_file_write_flush().await?;
    logl::log(level::INFO, format_args!("tokio_fs: success fs.open_options.file_write_flush"));

    logl::log(level::INFO, format_args!("tokio_fs: stage fs.file.read_to_end"));
    probe_file_read_to_end().await?;
    logl::log(level::INFO, format_args!("tokio_fs: success fs.file.read_to_end"));

    logl::log(level::INFO, format_args!("tokio_fs: stage fs.file.seek_rewrite_flush"));
    probe_file_seek_rewrite_flush().await?;
    logl::log(level::INFO, format_args!("tokio_fs: success fs.file.seek_rewrite_flush"));

    logl::log(level::INFO, format_args!("tokio_fs: stage fs.try_exists"));
    let exists = t::fs::try_exists(PROBE_PATH)
        .await
        .map_err(|_| "fs.try_exists")?;
    if !exists {
        return Err("fs.try_exists.false");
    }
    logl::log(level::INFO, format_args!("tokio_fs: success fs.try_exists"));

    logl::log(level::INFO, format_args!("tokio_fs: stage fs.stat.file"));
    let file_stat = t::fs::stat(PROBE_PATH.as_bytes()).map_err(|_| "fs.stat.file")?;
    if file_stat.kind != t::fs::FsNodeKind::File || file_stat.len != PROBE_BYTES.len() as u64 {
        return Err("fs.stat.file.value");
    }
    logl::log(level::INFO, format_args!("tokio_fs: success fs.stat.file len={}", file_stat.len));

    logl::log(level::INFO, format_args!("tokio_fs: stage fs.create_dir_all.nested_write_read"));
    probe_create_dir_all_nested_write_read().await?;
    logl::log(level::INFO, format_args!("tokio_fs: success fs.create_dir_all.nested_write_read"));

    logl::log(level::INFO, format_args!("tokio_fs: stage fs.stat.dir"));
    let dir_stat =
        t::fs::stat(format!("{}/nested", PROBE_DIR).as_bytes()).map_err(|_| "fs.stat.dir")?;
    if dir_stat.kind != t::fs::FsNodeKind::Directory || dir_stat.len != 0 {
        return Err("fs.stat.dir.value");
    }
    logl::log(level::INFO, format_args!("tokio_fs: success fs.stat.dir"));

    logl::log(level::INFO, format_args!("tokio_fs: stage fs.canonicalize.trueos"));
    let canonical = t::fs::canonicalize(format!("{}/./nested/../nested/probe.txt", PROBE_DIR))
        .await
        .map_err(|_| "fs.canonicalize.trueos")?;
    if canonical != Path::new("/").join(PROBE_NESTED_PATH) {
        return Err("fs.canonicalize.trueos.value");
    }
    logl::log(
        level::INFO,
        format_args!("tokio_fs: success fs.canonicalize.trueos path={}", canonical.as_os_str()),
    );

    logl::log(level::INFO, format_args!("tokio_fs: stage fs.remove_file"));
    t::tokio::fs::remove_file(PROBE_PATH)
        .await
        .map_err(|_| "fs.remove_file")?;
    let _ = t::tokio::fs::remove_file(PROBE_NESTED_PATH).await;
    logl::log(level::INFO, format_args!("tokio_fs: success fs.remove_file"));

    Ok(())
}

async fn probe_create_dir_all_nested_write_read() -> Result<(), &'static str> {
    t::fs::create_dir_all(format!("{}/nested", PROBE_DIR))
        .await
        .map_err(|_| "fs.create_dir_all")?;

    t::fs::write(PROBE_NESTED_PATH, PROBE_BYTES)
        .await
        .map_err(|_| "fs.nested.write")?;

    let exists = t::fs::try_exists(PROBE_NESTED_PATH)
        .await
        .map_err(|_| "fs.nested.try_exists")?;
    if !exists {
        return Err("fs.nested.try_exists.false");
    }

    let bytes = t::fs::read(PROBE_NESTED_PATH)
        .await
        .map_err(|_| "fs.nested.read")?;
    if bytes.as_slice() != PROBE_BYTES {
        return Err("fs.nested.read.value");
    }

    Ok(())
}

async fn probe_file_write_flush() -> Result<(), &'static str> {
    use t::io::AsyncWriteExt;

    let mut file = t::fs::OpenOptions::new()
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
    use t::io::AsyncReadExt;

    let mut file = t::fs::File::open(PROBE_PATH)
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
    use t::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

    let mut file = t::fs::OpenOptions::new()
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

    let mut file = t::fs::File::open(PROBE_PATH)
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
