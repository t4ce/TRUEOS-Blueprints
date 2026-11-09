//! Preserve Reqwest 0.12.28 TCP/TLS while refusing unavailable native UDS.
//! TrueOS is a Unix target, but its native TCP provider is not a Unix file
//! descriptor provider. Retaining the optional UDS connector also retains the
//! host std/Mio socket operations even when every request uses TCP.
use std::path::{Path, PathBuf};

const NATIVE: &str = "target_os = \"trueos\"";
const LOCAL: &str = "#[cfg(any(unix, target_os = \"windows\"))]";
const BUILD: &str = "        let config = self.config;\n";

fn replace_exact(text: &mut String, old: &str, new: &str, count: usize) -> Result<(), String> {
    let found = text.matches(old).count();
    if found != count {
        return Err(format!("pinned native Reqwest boundary drifted: expected {count} occurrences of {old:?}, found {found}"));
    }
    *text = text.replace(old, new);
    Ok(())
}

pub(crate) fn stage(work: &Path, source: &Path) -> Result<PathBuf, String> {
    if crate::package_name(&source.join("Cargo.toml"))? != "reqwest"
        || crate::package_version(&source.join("Cargo.toml"))?.as_deref() != Some("0.12.28")
    {
        return Err("native legacy Reqwest requires original reqwest 0.12.28".into());
    }
    let mut connect =
        std::fs::read_to_string(source.join("src/connect.rs")).map_err(|e| e.to_string())?;
    replace_exact(
        &mut connect,
        LOCAL,
        "#[cfg(any(all(unix, not(target_os = \"trueos\")), target_os = \"windows\"))]",
        3,
    )?;
    replace_exact(
        &mut connect,
        "#[cfg(unix)]\nimpl",
        "#[cfg(all(unix, not(target_os = \"trueos\")))]\nimpl",
        7,
    )?;
    replace_exact(
        &mut connect,
        "#[cfg(unix)]\n    impl",
        "#[cfg(all(unix, not(target_os = \"trueos\")))]\n    impl",
        4,
    )?;
    let mut client = std::fs::read_to_string(source.join("src/async_impl/client.rs"))
        .map_err(|e| e.to_string())?;
    replace_exact(
        &mut client,
        BUILD,
        &format!(
            r#"{BUILD}
        #[cfg({NATIVE})]
        if config.unix_socket.is_some() {{
            return Err(crate::error::builder(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "native Reqwest does not support Unix-domain sockets; use an HTTP(S) TCP endpoint",
            )));
        }}
"#
        ),
        1,
    )?;
    let staged = work.join("source-overlay-crates/reqwest-0.12.28");
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    std::fs::write(staged.join("src/connect.rs"), connect).map_err(|e| e.to_string())?;
    std::fs::write(staged.join("src/async_impl/client.rs"), client).map_err(|e| e.to_string())?;
    Ok(staged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_release_preserves_public_uds_api_and_refuses_drift() {
        let source = crate::find_cargo_registry_crate("reqwest-0.12.28").unwrap();
        let work =
            std::env::temp_dir().join(format!("trueos-reqwest-stage-{}", std::process::id()));
        let staged = stage(&work, &source).unwrap();
        assert_eq!(
            std::fs::read(staged.join("Cargo.toml")).unwrap(),
            std::fs::read(source.join("Cargo.toml")).unwrap()
        );
        let client = std::fs::read_to_string(staged.join("src/async_impl/client.rs")).unwrap();
        assert!(client.contains("pub fn unix_socket(mut self, path: impl UnixSocketProvider)"));
        assert!(client.contains("std::io::ErrorKind::Unsupported"));
        assert!(client.contains("if config.unix_socket.is_some()"));
        let mut text = "unchanged".to_string();
        assert!(replace_exact(&mut text, "missing", "replacement", 1).is_err());
        assert_eq!(text, "unchanged");
        std::fs::remove_dir_all(work).unwrap();
    }
}
