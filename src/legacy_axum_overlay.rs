//! Preserve the original Axum 0.7 / axum-core 0.4 package identities.
//!
//! The current TRUEOS runtime provides Rust std, including Tokio's standard
//! I/O and listener contracts. These releases need no no_std facade or API
//! migration. In particular, Axum 0.8 is not a compatible source replacement:
//! its route syntax and request-extraction traits differ from Axum 0.7.

use std::path::{Path, PathBuf};

/// Stage one exact upstream release without rewriting its source or manifest.
/// The caller supplies the discovered registry directory and installs the
/// returned directory as a version-specific Cargo source overlay.
pub(crate) fn stage(work_dir: &Path, source: &Path) -> Result<PathBuf, String> {
    let manifest = source.join("Cargo.toml");
    let name = crate::package_name(&manifest)?;
    let version = crate::package_version(&manifest)?;
    match (name.as_str(), version.as_deref()) {
        ("axum", Some("0.7.9")) | ("axum-core", Some("0.4.5")) => {}
        _ => {
            return Err(format!(
                "legacy Axum overlay requires original axum 0.7.9 or axum-core 0.4.5 source: {}",
                source.display()
            ));
        }
    }
    let staged = work_dir
        .join("source-overlay-crates")
        .join(format!("{name}-{}-trueos", version.unwrap()));
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    Ok(staged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let directory = std::env::temp_dir().join(format!(
                "trueos-legacy-axum-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&directory).unwrap();
            Self(directory)
        }

        fn source(&self, name: &str, version: &str) -> PathBuf {
            let source = self.0.join(format!("{name}-{version}"));
            fs::create_dir_all(source.join("src")).unwrap();
            fs::write(
                source.join("Cargo.toml"),
                format!("[package]\nname = {name:?}\nversion = {version:?}\n"),
            )
            .unwrap();
            source
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn exact_releases_keep_upstream_source_and_manifest_bytes() {
        let fixture = Fixture::new();
        for (name, version) in [("axum", "0.7.9"), ("axum-core", "0.4.5")] {
            let source = fixture.source(name, version);
            let legacy_api = b"// Original /:parameter routes and FromRequest contracts.\n";
            fs::write(source.join("src/lib.rs"), legacy_api).unwrap();
            let original_manifest = fs::read(source.join("Cargo.toml")).unwrap();
            let staged = stage(&fixture.0.join("work"), &source).unwrap();
            assert_eq!(fs::read(staged.join("src/lib.rs")).unwrap(), legacy_api);
            assert_eq!(
                fs::read(staged.join("Cargo.toml")).unwrap(),
                original_manifest
            );
            assert_eq!(
                fs::read(source.join("Cargo.toml")).unwrap(),
                original_manifest
            );

            fs::write(staged.join("stale"), b"previous overlay").unwrap();
            stage(&fixture.0.join("work"), &source).unwrap();
            assert!(!staged.join("stale").exists());
        }
    }

    #[test]
    fn a_newer_release_is_rejected_without_relabeling() {
        let fixture = Fixture::new();
        let source = fixture.source("axum", "0.8.9");
        let work = fixture.0.join("work");
        let error = stage(&work, &source).unwrap_err();
        assert!(error.contains("original axum 0.7.9 or axum-core 0.4.5"));
        assert!(!work.exists());
        assert_eq!(
            crate::package_version(&source.join("Cargo.toml"))
                .unwrap()
                .as_deref(),
            Some("0.8.9")
        );
    }
}
