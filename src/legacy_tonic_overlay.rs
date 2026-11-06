//! Stage the original Tonic 0.12.3 release for the TRUEOS std runtime.
//!
//! Tokio's current platform fork retains std I/O, listener conversion and Unix
//! socket interfaces. The older no_std adaptations for Tonic 0.14 must not be
//! applied to this source or used to change its prost/body/axum contracts.
//! The one channel adaptation owns a cloned HTTP/2 request handle inside its
//! boxed async block. Hyper's edition-2024 opaque future captures the mutable
//! handle borrow, so creating that future from `&mut self` outside the block
//! cannot satisfy Tonic's `BoxFuture<'static, _>` contract.

use std::path::{Path, PathBuf};

const CONNECTION: &str = "src/transport/channel/service/connection.rs";
const BORROWED_REQUEST: &str = "        let fut = self.inner.send_request(req);\n\n        Box::pin(async move { fut.await.map_err(Into::into).map(|res| res.map(boxed)) })";
const OWNED_REQUEST: &str = "        let mut inner = self.inner.clone();\n\n        Box::pin(async move { inner.send_request(req).await.map_err(Into::into).map(|res| res.map(boxed)) })";

pub(crate) fn stage(work_dir: &Path, source: &Path) -> Result<PathBuf, String> {
    let manifest = source.join("Cargo.toml");
    if crate::package_name(&manifest)? != "tonic"
        || crate::package_version(&manifest)?.as_deref() != Some("0.12.3")
    {
        return Err(format!(
            "legacy Tonic staging requires original tonic 0.12.3 source: {}",
            source.display()
        ));
    }
    let connection = source.join(CONNECTION);
    let original = std::fs::read_to_string(&connection)
        .map_err(|error| format!("read {}: {error}", connection.display()))?;
    let occurrences = original.matches(BORROWED_REQUEST).count();
    if occurrences != 1 {
        return Err(format!(
            "legacy Tonic ownership patch expected exactly one original request future in {}; found {occurrences}",
            connection.display()
        ));
    }
    let patched = original.replacen(BORROWED_REQUEST, OWNED_REQUEST, 1);
    let staged = work_dir
        .join("source-overlay-crates")
        .join("tonic-0.12.3-trueos");
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    std::fs::write(staged.join(CONNECTION), patched)
        .map_err(|error| format!("write {}: {error}", staged.join(CONNECTION).display()))?;
    Ok(staged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn fixture(version: &str) -> (PathBuf, PathBuf) {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "trueos-tonic-staging-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let source = root.join("source");
        fs::create_dir_all(source.join("src")).unwrap();
        fs::write(
            source.join("Cargo.toml"),
            format!("[package]\nname = \"tonic\"\nversion = \"{version}\"\n\n[features]\ndefault = [\"transport\"]\ntransport = []\n"),
        )
        .unwrap();
        fs::write(source.join("src/lib.rs"), "// original source\n").unwrap();
        fs::create_dir_all(source.join(CONNECTION).parent().unwrap()).unwrap();
        fs::write(
            source.join(CONNECTION),
            format!("// unchanged prefix\n{BORROWED_REQUEST}\n// unchanged suffix\n"),
        )
        .unwrap();
        (root, source)
    }

    #[test]
    fn changes_only_the_request_future_and_preserves_the_selected_release() {
        let (root, source) = fixture("0.12.3");
        let staged = stage(&root.join("work"), &source).unwrap();
        for file in ["Cargo.toml", "src/lib.rs"] {
            assert_eq!(
                fs::read(staged.join(file)).unwrap(),
                fs::read(source.join(file)).unwrap(),
                "staging must not relabel or rewrite {file}"
            );
        }
        let original = fs::read_to_string(source.join(CONNECTION)).unwrap();
        let staged_connection = fs::read_to_string(staged.join(CONNECTION)).unwrap();
        assert_eq!(
            staged_connection,
            format!("// unchanged prefix\n{OWNED_REQUEST}\n// unchanged suffix\n")
        );
        assert_eq!(
            original,
            format!("// unchanged prefix\n{BORROWED_REQUEST}\n// unchanged suffix\n"),
            "the registry source must remain unchanged"
        );
        assert!(!staged_connection.contains("let fut = self.inner.send_request(req)"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_or_ambiguous_original_template_is_refused_before_resetting_stage() {
        let (root, source) = fixture("0.12.3");
        let work = root.join("work");
        let existing = work.join("source-overlay-crates/tonic-0.12.3-trueos");
        fs::create_dir_all(&existing).unwrap();
        fs::write(existing.join("retained"), "existing stage").unwrap();
        for (contents, count) in [
            (OWNED_REQUEST.to_owned(), 0),
            (format!("{BORROWED_REQUEST}\n{BORROWED_REQUEST}"), 2),
        ] {
            fs::write(source.join(CONNECTION), contents).unwrap();
            let error = stage(&work, &source).unwrap_err();
            assert!(error.contains(CONNECTION), "{error}");
            assert!(error.contains(&format!("found {count}")), "{error}");
            assert_eq!(
                fs::read_to_string(existing.join("retained")).unwrap(),
                "existing stage"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn another_release_is_refused_before_resetting_the_stage() {
        let (root, source) = fixture("0.14.6");
        let work = root.join("work");
        let existing = work.join("source-overlay-crates/tonic-0.12.3-trueos");
        fs::create_dir_all(&existing).unwrap();
        fs::write(existing.join("retained"), "existing stage").unwrap();
        assert!(stage(&work, &source).is_err());
        assert_eq!(
            fs::read_to_string(existing.join("retained")).unwrap(),
            "existing stage"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
