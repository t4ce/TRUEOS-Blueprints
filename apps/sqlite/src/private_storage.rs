//! Storage for the explicitly delegated native Codex dependency.
use std::path::PathBuf;

pub const ENDPOINT: &str = "http://127.0.0.1:42116";
pub const DIRECTORY: &str = "/apps/codex/.codex/sqlite-service/codex";

pub fn resolve(
    instance: &str,
    override_directory: Option<PathBuf>,
    trusted_directory: Option<&str>,
    trusted_endpoint: Option<&str>,
    native: bool,
) -> Result<PathBuf, String> {
    if !native || instance != "codex" && trusted_directory.is_none() && trusted_endpoint.is_none() {
        return Ok(override_directory
            .unwrap_or_else(|| PathBuf::from("/common/sqlite-service"))
            .join(instance));
    }
    if instance != "codex"
        || trusted_directory != Some(DIRECTORY)
        || trusted_endpoint != Some(ENDPOINT)
    {
        return Err(
            "native Codex SQLite requires its kernel-delegated private directory and endpoint"
                .into(),
        );
    }
    if override_directory.is_some() {
        return Err("--data-dir cannot override the delegated Codex SQLite directory".into());
    }
    Ok(PathBuf::from(DIRECTORY))
}

#[cfg(target_os = "trueos")]
pub async fn verify(instance: &str, directory: &std::path::Path) -> Result<(), String> {
    if instance != "codex" {
        return Ok(());
    }
    if directory != std::path::Path::new(DIRECTORY) {
        return Err("unexpected native Codex SQLite directory".into());
    }
    trueos::async_fs::assert_private_path(DIRECTORY.as_bytes())
        .await
        .map_err(|code| {
            format!("Codex SQLite storage is not delegated private storage: TRUEOSFS {code}")
        })?;
    // Trusted kernel preparation moves the whole legacy directory without
    // overwriting a destination. Never silently replace existing databases by
    // starting an empty service when that preservation step has not completed.
    // `/common` is the rooted shared export. Walking `/apps` first would
    // instead probe an app-relative directory under this container's scope.
    let legacy = "/common/sqlite-service/codex";
    let mut prefix = String::new();
    for part in legacy.trim_start_matches('/').split('/') {
        prefix.push('/');
        prefix.push_str(part);
        match trueos::async_fs::metadata(prefix.as_bytes()).await {
            Err(trueos::async_fs::ERR_NOT_FOUND) => return Ok(()),
            Err(code) => {
                return Err(format!(
                    "cannot verify legacy Codex SQLite preservation at {prefix}: TRUEOSFS {code}"
                ));
            }
            Ok(metadata) if metadata.kind != trueos::async_fs::NodeKind::Directory => {
                return Err(format!(
                    "legacy Codex SQLite path is not a directory: {prefix}"
                ));
            }
            Ok(_) => {}
        }
    }
    Err(format!(
        "legacy Codex SQLite storage remains at {legacy}; trusted private migration must complete before service startup"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_codex_cannot_start_public_or_redirected_storage() {
        assert!(resolve("codex", None, None, None, true).is_err());
        assert!(
            resolve(
                "codex",
                None,
                Some(DIRECTORY),
                Some("http://127.0.0.1:12345"),
                true
            )
            .is_err()
        );
        assert!(
            resolve(
                "codex",
                Some(PathBuf::from(DIRECTORY)),
                Some(DIRECTORY),
                Some(ENDPOINT),
                true
            )
            .is_err()
        );
        assert!(
            resolve(
                "codex",
                None,
                Some("/common/sqlite-service/codex"),
                Some(ENDPOINT),
                true
            )
            .is_err()
        );
        assert!(resolve("other", None, Some(DIRECTORY), Some(ENDPOINT), true).is_err());
    }
    #[test]
    fn exact_native_delegate_and_ordinary_host_instances_keep_their_paths() {
        assert_eq!(
            resolve("codex", None, Some(DIRECTORY), Some(ENDPOINT), true).unwrap(),
            PathBuf::from(DIRECTORY)
        );
        assert_eq!(
            resolve(
                "codex",
                Some(PathBuf::from("/tmp/fixture")),
                None,
                None,
                false
            )
            .unwrap(),
            PathBuf::from("/tmp/fixture/codex")
        );
        assert_eq!(
            resolve("other", None, None, None, true).unwrap(),
            PathBuf::from("/common/sqlite-service/other")
        );
    }
}
