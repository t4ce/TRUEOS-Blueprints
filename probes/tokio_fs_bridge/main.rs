//! Exercises handle-preserving std/Tokio conversion and the blocking reader API.
use std::io::{Read, Seek, SeekFrom, Write};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

const PATH: &str = "/common/tokio-fs-bridge.txt";
const MEMORY: &str = "/common/tokio-fs-bridge-memory.txt";
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    phase("std_write").await;
    std::fs::write(PATH, b"prefix:suffix\n")?;
    phase("std_open").await;
    let mut standard = std::fs::File::open(PATH)?;
    let mut prefix = [0; 7];
    Read::read_exact(&mut standard, &mut prefix)?;
    assert_eq!(&prefix, b"prefix:");
    phase("from_std").await;
    let mut file = tokio::fs::File::from_std(standard);
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).await?;
    assert_eq!(tail, b"suffix\n");
    assert_eq!(file.metadata().await?.len(), 14);
    let mut standard = file.into_std().await;
    Seek::seek(&mut standard, SeekFrom::Start(7))?;
    let mut reader: Box<dyn Read + Send> = Box::new(standard);
    let mut tail = String::new();
    Read::read_to_string(&mut reader, &mut tail)?;
    assert_eq!(tail, "suffix\n");
    drop(reader);

    // Seeking an append handle must not turn an append into an overwrite.
    let standard = std::fs::OpenOptions::new()
        .read(true)
        .append(true)
        .open(PATH)?;
    let mut file = tokio::fs::File::from_std(standard);
    AsyncSeekExt::seek(&mut file, SeekFrom::Start(0)).await?;
    file.write_all(b"appended\n").await?;
    file.flush().await?;
    file.sync_all().await?;
    drop(file);
    assert_eq!(std::fs::read(PATH)?, b"prefix:suffix\nappended\n");

    // The original descriptor's read-only access must still be enforced.
    let mut file = tokio::fs::File::from_std(std::fs::File::open(PATH)?);
    let write = file.write_all(b"must not write").await;
    let flush = file.flush().await;
    assert!(write.is_err() || flush.is_err());
    drop(file);
    assert_eq!(std::fs::read(PATH)?, b"prefix:suffix\nappended\n");

    // Snapshot files returned by into_std also implement owned blocking I/O.
    tokio::fs::write(MEMORY, b"memory\n").await?;
    let mut standard = tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(MEMORY)
        .await?
        .into_std()
        .await;
    Seek::seek(&mut standard, SeekFrom::Start(100))?;
    assert_eq!(Read::read(&mut standard, &mut [0u8; 1])?, 0);
    Seek::seek(&mut standard, SeekFrom::Start(0))?;
    Write::write_all(&mut standard, b"MEMORY\n")?;
    Write::flush(&mut standard)?;
    let mut reader: Box<dyn Read + Send> = Box::new(standard);
    let mut contents = Vec::new();
    // The cursor remains at the end after a write.
    Read::read_to_end(&mut reader, &mut contents)?;
    assert!(contents.is_empty());
    drop(reader);
    assert_eq!(tokio::fs::read(MEMORY).await?, b"MEMORY\n");
    Ok(())
}
async fn phase(stage: &str) {
    let report = format!("{{\"result\":\"RUNNING\",\"stage\":\"{stage}\"}}");
    let _ = trueos::async_fs::write_file(b"/common/tokio-fs-bridge-report.json", report.as_bytes())
        .await;
}
fn main() {
    let _ = trueos::async_fs::block_on(trueos::async_fs::write_file(
        b"/common/tokio-fs-bridge-report.json",
        br#"{"result":"START"}"#,
    ));
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = tokio::task::LocalSet::new().block_on(&rt, run());
    let report = match result {
        Ok(()) => String::from(
            "{\"result\":\"PASS\",\"cursor\":true,\"append\":true,\"read_only\":true,\"owned_blocking_io\":true}",
        ),
        Err(e) => format!("FAIL {e}"),
    };
    let _ = trueos::async_fs::block_on(trueos::async_fs::write_file(
        b"/common/tokio-fs-bridge-report.json",
        report.as_bytes(),
    ));
    trueos::logl::log(trueos::logl::level::INFO, report.as_str());
}
