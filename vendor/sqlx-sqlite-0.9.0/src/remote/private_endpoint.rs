//! The kernel reserves this native Codex dependency before either app starts.
//! A configurable localhost service cannot substitute for the delegated service.
pub(super) const ENDPOINT: &str = "http://127.0.0.1:42116";

pub(super) fn select(
    instance: &str,
    configured: Option<&str>,
    trusted: Option<&str>,
) -> Result<Option<&'static str>, &'static str> {
    if instance != "codex" && trusted.is_none() {
        return Ok(None);
    }
    if instance != "codex" {
        return Err("the private Codex SQLite dependency cannot select another instance");
    }
    if trusted != Some(ENDPOINT) {
        return Err("Codex SQLite requires its kernel-provided private service endpoint");
    }
    if configured.is_some_and(|value| value != ENDPOINT) {
        return Err("TRUEOS_SQLITE_ENDPOINT cannot override the private Codex service");
    }
    Ok(Some(ENDPOINT))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codex_requires_native_delegation() {
        assert!(select("codex", None, None).is_err());
        assert!(select("codex", Some("http://localhost:42116"), None).is_err());
        assert!(select("codex", None, Some("http://127.0.0.1:42117")).is_err());
    }
    #[test]
    fn private_endpoint_cannot_be_redirected() {
        for endpoint in ["http://127.0.0.1:12345", "http://localhost:42116", "http://example.com:42116", "http://127.0.0.1:42116/public"] {
            assert!(select("codex", Some(endpoint), Some(ENDPOINT)).is_err());
        }
        assert!(select("other", None, Some(ENDPOINT)).is_err());
    }
    #[test]
    fn admitted_codex_and_other_explicit_instances_keep_their_contracts() {
        assert_eq!(select("codex", None, Some(ENDPOINT)), Ok(Some(ENDPOINT)));
        assert_eq!(select("codex", Some(ENDPOINT), Some(ENDPOINT)), Ok(Some(ENDPOINT)));
        assert_eq!(select("other", Some("http://localhost:12345"), None), Ok(None));
    }
}
