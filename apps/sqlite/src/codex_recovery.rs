//! Codex database lifecycle belongs to the SQLite image owner.
//!
//! Recovery archives the exact persisted bytes before replacing a corrupt image.
//! A healthy image, an open/cached database, or an unrelated I/O failure never
//! authorizes a reset. Clients receive a service locator, not a local pathname.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RecoveryRequest {
    database: String,
}

#[derive(Serialize)]
pub(super) struct RecoveryReceipt {
    storage_owner: &'static str,
    instance: String,
    database: String,
    backup_locator: String,
    archived_bytes: usize,
}

pub(super) fn image_error(error: ImageError) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
        "error": error.message, "sqlite_code": error.code,
    })))
}

pub(super) async fn recover(
    State(service): State<Service>,
    Json(request): Json<RecoveryRequest>,
) -> Result<Json<RecoveryReceipt>, ApiError> {
    // This is the persistent Codex SQLx naming scheme. In-memory sessions and
    // arbitrary service databases do not participate in Codex startup recovery.
    if request.database.len() != 19 || !request.database.starts_with("db-")
        || !request.database.as_bytes()[3..].iter().all(u8::is_ascii_hexdigit)
    {
        return Err(error(StatusCode::BAD_REQUEST, "expected a persistent Codex database identifier"));
    }
    // Service::database takes this same authority lock while loading/publishing
    // a cache entry. No new session can open the target during recovery.
    let databases = service.databases.lock().await;
    if databases.contains_key(&request.database) {
        return Err(error(StatusCode::CONFLICT, "database is open in the SQLite service; restart recovery cannot reset it"));
    }
    let path = service.directory.join(format!("{}.sqlite", request.database));
    let bytes = storage::read(&path).await
        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "database image does not exist"))?;
    if bytes.len() > MAX_IMAGE {
        return Err(error(StatusCode::PAYLOAD_TOO_LARGE, "database image exceeds recovery limit"));
    }
    let inspected = bytes.clone();
    let corrupt = tokio::task::spawn_blocking(move || {
        if inspected.is_empty() { return true; }
        match checked_connection(&inspected) {
            Ok(_) => false,
            Err(error) => matches!(error.code & 0xff, 11 | 26),
        }
    }).await.map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if !corrupt {
        return Err(error(StatusCode::CONFLICT, "image is healthy or its failure is not SQLite corruption"));
    }
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_nanos();
    let backup_id = format!("{timestamp:x}-{:x}", SEQUENCE.fetch_add(1, Ordering::Relaxed));
    let directory = service.directory.join("db-backups").join(&request.database);
    storage::create_dir(&directory).await.map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let backup = directory.join(format!("{backup_id}.sqlite"));
    storage::publish_new(&backup, &bytes).await
        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, format!("archive failed; original image retained: {e}")))?;
    // Verify the durable archive before touching the only original. Failed
    // replacement leaves that original and this archive available for retry.
    if storage::read(&backup).await.map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?.as_deref() != Some(bytes.as_slice()) {
        return Err(error(StatusCode::INTERNAL_SERVER_ERROR, "archive verification failed; original image retained"));
    }
    let fresh = tokio::task::spawn_blocking(|| {
        checked_connection(&[])?.serialize(MAIN_DB).map(|image| image.to_vec()).map_err(ImageError::from)
    }).await.map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e))?
        .map_err(image_error)?;
    storage::publish(&path, &fresh).await
        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, format!("fresh image publication failed; archive retained: {e}")))?;
    drop(databases);
    Ok(Json(RecoveryReceipt {
        storage_owner: "sqlite-service",
        instance: service.advertisement.instance.clone(),
        database: request.database.clone(),
        backup_locator: format!("sqlite-service://{}/db-backups/{}/{backup_id}.sqlite",
            service.advertisement.instance, request.database),
        archived_bytes: bytes.len(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAME: &str = "db-0123456789abcdef";
    struct Fixture(Service);
    impl Fixture {
        fn new() -> Self {
            #[cfg(feature = "native-sqlite")]
            native_backend::initialize().unwrap();
            static SEQUENCE: AtomicU64 = AtomicU64::new(0);
            let directory = std::env::temp_dir().join(format!("codex-recovery-{}-{}",
                std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&directory).unwrap();
            Self(Service {
                advertisement: Advertisement { protocol: "trueos-sqlite/v1", instance: "codex-test".into(),
                    endpoint: "http://localhost".into(), ipv4_endpoint: "http://127.0.0.1".into(),
                    sqlite_version: rusqlite::version().into(), storage: "sqlite-memory-image" },
                directory, databases: Arc::new(Mutex::new(HashMap::new())),
            })
        }
        fn path(&self) -> PathBuf { self.0.directory.join(format!("{NAME}.sqlite")) }
        async fn recover(&self) -> Result<Json<RecoveryReceipt>, ApiError> {
            recover(State(self.0.clone()), Json(RecoveryRequest { database: NAME.into() })).await
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0.directory); }
    }

    #[tokio::test]
    async fn archives_exact_corrupt_bytes_and_only_replaces_requested_database() {
        let fixture = Fixture::new();
        let original = b"damaged Codex database\0\xff";
        std::fs::write(fixture.path(), original).unwrap();
        let unrelated = fixture.0.directory.join("db-fedcba9876543210.sqlite");
        std::fs::write(&unrelated, b"leave this unchanged").unwrap();
        let Json(receipt) = fixture.recover().await.unwrap();
        assert_eq!(receipt.storage_owner, "sqlite-service");
        assert_eq!(receipt.archived_bytes, original.len());
        assert!(receipt.backup_locator.starts_with("sqlite-service://codex-test/db-backups/"));
        let archive = std::fs::read_dir(fixture.0.directory.join("db-backups").join(NAME))
            .unwrap().next().unwrap().unwrap().path();
        assert_eq!(std::fs::read(archive).unwrap(), original);
        checked_connection(&std::fs::read(fixture.path()).unwrap()).unwrap();
        assert_eq!(std::fs::read(unrelated).unwrap(), b"leave this unchanged");
        assert_eq!(fixture.recover().await.err().unwrap().0, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn healthy_or_cached_databases_are_never_reset() {
        let fixture = Fixture::new();
        let healthy = connection(&[]).unwrap().serialize(MAIN_DB).unwrap().to_vec();
        std::fs::write(fixture.path(), &healthy).unwrap();
        assert_eq!(fixture.recover().await.err().unwrap().0, StatusCode::CONFLICT);
        assert_eq!(std::fs::read(fixture.path()).unwrap(), healthy);
        let _session = fixture.0.database(NAME).await.unwrap();
        std::fs::write(fixture.path(), b"external corruption").unwrap();
        assert_eq!(fixture.recover().await.err().unwrap().0, StatusCode::CONFLICT);
        assert!(!fixture.0.directory.join("db-backups").exists());
    }

    #[tokio::test]
    async fn archive_failure_retains_original() {
        let fixture = Fixture::new();
        std::fs::write(fixture.path(), b"damaged").unwrap();
        std::fs::write(fixture.0.directory.join("db-backups"), b"blocked").unwrap();
        assert!(fixture.recover().await.is_err());
        assert_eq!(std::fs::read(fixture.path()).unwrap(), b"damaged");
    }

    #[tokio::test]
    async fn replacement_failure_retains_original_and_verified_archive() {
        let fixture = Fixture::new();
        std::fs::write(fixture.path(), b"damaged").unwrap();
        std::fs::create_dir(fixture.path().with_extension("pending")).unwrap();
        assert!(fixture.recover().await.is_err());
        assert_eq!(std::fs::read(fixture.path()).unwrap(), b"damaged");
        let archive = std::fs::read_dir(fixture.0.directory.join("db-backups").join(NAME))
            .unwrap().next().unwrap().unwrap().path();
        assert_eq!(std::fs::read(archive).unwrap(), b"damaged");
    }

    #[tokio::test]
    async fn empty_images_recover_and_missing_images_do_not_create_databases() {
        let fixture = Fixture::new();
        assert_eq!(fixture.recover().await.err().unwrap().0, StatusCode::NOT_FOUND);
        assert!(!fixture.path().exists());
        std::fs::write(fixture.path(), []).unwrap();
        assert_eq!(fixture.recover().await.unwrap().0.archived_bytes, 0);
        checked_connection(&std::fs::read(fixture.path()).unwrap()).unwrap();
    }

    #[tokio::test]
    async fn malformed_names_and_non_corruption_codes_are_rejected() {
        let fixture = Fixture::new();
        assert_eq!(recover(State(fixture.0.clone()), Json(RecoveryRequest { database: "../x".into() }))
            .await.err().unwrap().0, StatusCode::BAD_REQUEST);
        assert_eq!(checked_connection(b"broken").err().unwrap().code, 26);
        let too_large = vec![0; MAX_IMAGE + 1];
        assert_eq!(checked_connection(&too_large).err().unwrap().code, 18);
    }
}
