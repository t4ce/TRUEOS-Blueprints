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
    let identity = file.metadata().await?;
    let id = identity.file_id().expect("committed file identity");
    let revision = identity.revision().expect("committed file revision");
    assert_eq!(identity.dev(), u64::from(identity.root_id()));
    assert_eq!(identity.ino(), id);
    assert_eq!(identity.ctime() as u64 * 1_000_000_000 + identity.ctime_nsec() as u64, revision);
    assert!(identity.is_file());
    assert!(!identity.is_symlink());
    assert_eq!(identity.created().unwrap_err().kind(), std::io::ErrorKind::Unsupported);
    assert_eq!(identity.modified().unwrap_err().kind(), std::io::ErrorKind::Unsupported);
    let path_identity = tokio::fs::metadata(PATH).await?;
    assert_eq!(path_identity.file_id(), Some(id));
    assert_eq!(path_identity.root_id(), identity.root_id());
    assert_eq!(path_identity.revision(), Some(revision));
    #[cfg(target_os = "trueos")]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(PATH)?;
        assert_eq!(metadata.ino(), id);
        assert_eq!(metadata.dev(), u64::from(identity.root_id()));
        assert_eq!(metadata.ctime(), identity.ctime());
        assert_eq!(metadata.ctime_nsec(), identity.ctime_nsec());
    }
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
    assert_eq!(tokio::fs::metadata(PATH).await?.file_id(), Some(id));

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
    phase("metadata_identity").await;
    let memory_before = tokio::fs::metadata(MEMORY).await?;
    let memory_id = memory_before.file_id().unwrap();
    tokio::fs::write(MEMORY, b"changed").await?;
    let memory_after = tokio::fs::metadata(MEMORY).await?;
    assert_eq!(memory_before.len(), memory_after.len());
    assert_ne!(memory_before.revision(), memory_after.revision());
    assert_eq!(tokio::fs::metadata(MEMORY).await?.file_id(), Some(memory_id));
    tokio::fs::remove_file(MEMORY).await?;
    tokio::fs::write(MEMORY, b"recreated").await?;
    assert_ne!(tokio::fs::metadata(MEMORY).await?.file_id(), Some(memory_id));
    let directory = tokio::fs::metadata("/common").await?;
    assert!(directory.is_dir());
    assert!(!directory.is_symlink());
    phase("permission_copy").await;
    let before_permissions = tokio::fs::metadata(MEMORY).await?;
    let permissions: std::fs::Permissions = before_permissions.permissions();
    tokio::fs::set_permissions(MEMORY, permissions.clone()).await?;
    let snapshot = tokio::fs::File::open(MEMORY).await?;
    snapshot.set_permissions(permissions.clone()).await?;
    drop(snapshot);
    let descriptor = tokio::fs::File::from_std(std::fs::File::open(MEMORY)?);
    descriptor.set_permissions(permissions).await?;
    drop(descriptor);
    let after_permissions = tokio::fs::metadata(MEMORY).await?;
    assert_eq!(before_permissions.file_id(), after_permissions.file_id());
    assert_eq!(before_permissions.revision(), after_permissions.revision());
    assert_eq!(tokio::fs::read(MEMORY).await?, b"recreated");
    let error = tokio::fs::set_permissions(
        "/common/tokio-fs-bridge-missing-dir/file",
        after_permissions.permissions(),
    ).await.unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);

    // Tokio 1.54 exposes timestamp setters. Native snapshot handles must report
    // the broker's missing capability rather than claiming a successful write.
    phase("timestamp_boundary").await;
    let before_timestamps = tokio::fs::metadata(MEMORY).await?;
    let timestamp = std::time::SystemTime::UNIX_EPOCH
        + std::time::Duration::from_secs(1_700_000_000);
    let snapshot = tokio::fs::File::open(MEMORY).await?;
    let error = snapshot.set_times(
        tokio::fs::FileTimes::new().set_accessed(timestamp).set_modified(timestamp),
    ).await.unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
    assert_eq!(error.raw_os_error(), None);
    let error = snapshot.set_modified(timestamp).await.unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
    drop(snapshot);

    #[cfg(target_os = "trueos")]
    {
        // Descriptor-backed files retain std's result, including its OS errno.
        // The current futimens shim returns ENOSYS. Comparing the standard call
        // avoids mistaking the native snapshot's constructed error for routing.
        phase("descriptor_timestamp_forwarding").await;
        let standard = std::fs::File::open(MEMORY)?;
        let expected = standard.set_times(
            std::fs::FileTimes::new().set_modified(timestamp),
        ).unwrap_err();
        assert!(expected.raw_os_error().is_some());
        let descriptor = tokio::fs::File::from_std(standard);
        let actual = descriptor.set_times(
            tokio::fs::FileTimes::new().set_modified(timestamp),
        ).await.unwrap_err();
        assert_eq!(actual.kind(), expected.kind());
        assert_eq!(actual.raw_os_error(), expected.raw_os_error());
        drop(descriptor);
    }

    let after_timestamps = tokio::fs::metadata(MEMORY).await?;
    assert_eq!(before_timestamps.file_id(), after_timestamps.file_id());
    assert_eq!(before_timestamps.revision(), after_timestamps.revision());
    assert_eq!(tokio::fs::read(MEMORY).await?, b"recreated");
    Ok(())
}
async fn phase(stage: &str) {
    let report = format!("{{\"result\":\"RUNNING\",\"stage\":\"{stage}\"}}");
    let _ = trueos::async_fs::write_file(b"/common/tokio-fs-bridge-report.json", report.as_bytes())
        .await;
}
fn main() {
    let artifact_sha256 = std::env::var("TRUEOS_APP_SHA256")
        .unwrap_or_else(|_| String::from("unavailable"));
    let instance = std::env::var("TRUEOS_APP_INSTANCE")
        .unwrap_or_else(|_| String::from("unavailable"));
    let archive = std::env::var("TRUEOS_APP_ARCHIVE")
        .unwrap_or_else(|_| String::from("unavailable"));
    trueos::logl::log(
        trueos::logl::level::INFO,
        format_args!("tokio_fs_bridge: identity artifact_sha256={artifact_sha256} instance={instance} archive={archive}"),
    );
    let _ = trueos::async_fs::block_on(trueos::async_fs::write_file(
        b"/common/tokio-fs-bridge-report.json",
        br#"{"result":"START"}"#,
    ));
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = tokio::task::LocalSet::new().block_on(&rt, run());
    let artifact_sha256_report = if artifact_sha256.len() == 64
        && artifact_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        artifact_sha256.as_str()
    } else {
        "unavailable"
    };
    let instance_report = if !instance.is_empty()
        && instance.bytes().all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
    {
        instance.as_str()
    } else {
        "unavailable"
    };
    let report = match result {
        Ok(()) => format!(
            "{{\"result\":\"PASS\",\"cursor\":true,\"append\":true,\"read_only\":true,\"owned_blocking_io\":true,\"metadata_identity\":true,\"std_tokio_identity\":true,\"same_length_revision\":true,\"permission_copy\":true,\"timestamp_boundary\":true,\"descriptor_timestamp_forwarding\":true,\"artifact_sha256\":\"{artifact_sha256_report}\",\"instance\":\"{instance_report}\"}}",
        ),
        Err(e) => format!("FAIL {e}"),
    };
    let _ = trueos::async_fs::block_on(trueos::async_fs::write_file(
        b"/common/tokio-fs-bridge-report.json",
        report.as_bytes(),
    ));
    trueos::logl::log(trueos::logl::level::INFO, report.as_str());
}
