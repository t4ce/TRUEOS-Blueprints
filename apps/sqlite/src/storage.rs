use std::path::Path;

pub async fn read(path: &Path) -> Result<Option<Vec<u8>>, String> {
    #[cfg(target_os = "trueos")]
    match trueos::async_fs::read_file(path.to_string_lossy().as_bytes()).await {
        Ok(bytes) => Ok(Some(bytes)),
        Err(trueos::async_fs::ERR_NOT_FOUND) => Ok(None),
        Err(code) => Err(format!("read {}: TRUEOSFS {code}", path.display())),
    }
    #[cfg(not(target_os = "trueos"))]
    match tokio::fs::read(path).await {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

pub async fn create_dir(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "trueos")]
    return trueos::async_fs::create_dir_all(path.to_string_lossy().as_bytes())
        .await
        .map_err(|code| format!("create {}: TRUEOSFS {code}", path.display()));
    #[cfg(not(target_os = "trueos"))]
    tokio::fs::create_dir_all(path)
        .await
        .map_err(|error| error.to_string())
}

/// Publish a complete SQLite image before acknowledging its transaction.
pub async fn publish(path: &Path, bytes: &[u8]) -> Result<(), String> {
    #[cfg(target_os = "trueos")]
    {
        // TRUEOSFS publishes whole-file records. Its rename API deliberately
        // refuses replacement, so use the same image write as the proven probe.
        trueos::async_fs::write_file(path.to_string_lossy().as_bytes(), bytes)
            .await
            .map_err(|code| format!("write image: TRUEOSFS {code}"))?;
    }
    #[cfg(not(target_os = "trueos"))]
    {
        let temporary = path.with_extension("pending");
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::File::create(&temporary)
            .await
            .map_err(|e| e.to_string())?;
        file.write_all(bytes).await.map_err(|e| e.to_string())?;
        file.sync_all().await.map_err(|e| e.to_string())?;
        drop(file);
        tokio::fs::rename(&temporary, path)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Preserve a recovery archive without ever replacing an existing backup.
pub async fn publish_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    #[cfg(target_os = "trueos")]
    return trueos::async_fs::write_file_typed_checked(
        path.to_string_lossy().as_bytes(), bytes,
        trueos::async_fs::ContentTypeId::BLOB, true, None,
    ).await.map_err(|code| format!("create archive: TRUEOSFS {code}"));
    #[cfg(not(target_os = "trueos"))]
    {
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new().write(true).create_new(true).open(path)
            .await.map_err(|e| e.to_string())?;
        file.write_all(bytes).await.map_err(|e| e.to_string())?;
        file.sync_all().await.map_err(|e| e.to_string())
    }
}
