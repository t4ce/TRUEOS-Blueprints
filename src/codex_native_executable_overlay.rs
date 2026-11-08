//! Native executable discovery never mistakes a data file for a Unix program.
use std::path::{Path, PathBuf};

pub(crate) fn stage(work: &Path, source: &Path) -> Result<PathBuf, String> {
    let manifest = source.join("Cargo.toml");
    if crate::package_name(&manifest)? != "which"
        || crate::package_version(&manifest)?.as_deref() != Some("8.0.0")
    {
        return Err("native executable discovery requires which 8.0.0".into());
    }
    let path = source.join("src/sys.rs");
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let selector = "    #[cfg(any(unix, target_os = \"wasi\", target_os = \"redox\"))]";
    if text.matches(selector).count() != 1 {
        return Err("pinned which executable-provider selector drifted".into());
    }
    let text = text.replace(selector, r#"    #[cfg(target_os = "trueos")]
    fn is_valid_executable(&self, _path: &Path) -> io::Result<bool> {
        // Native command sessions are resolved by Shell3; generic std process
        // execution has no file-executable capability. Record existence and
        // file type alone do not establish that a path can be executed.
        Err(io::Error::new(io::ErrorKind::Unsupported,
            "native file executable classification is unavailable; use Shell3 command sessions"))
    }

    #[cfg(all(not(target_os = "trueos"), any(unix, target_os = "wasi", target_os = "redox")))]"#);
    let staged = work.join("source-overlay-crates/which-8.0.0-trueos");
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    std::fs::write(staged.join("src/sys.rs"), text).map_err(|error| error.to_string())?;
    Ok(staged)
}
