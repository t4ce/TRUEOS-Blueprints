//! Native std TCP has no Unix-domain descriptor transport.
use std::path::{Path, PathBuf};

pub(crate) fn stage(work: &Path, source: &Path) -> Result<PathBuf, String> {
    let staged = work.join("source-overlays/tiny_http-0.12.0-native-tcp");
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    for relative in ["src/lib.rs", "src/connection.rs"] {
        let path = staged.join(relative);
        let source = std::fs::read_to_string(&path).map_err(crate::io_string)?;
        if !source.contains("#[cfg(unix)]") {
            return Err(format!(
                "{}: expected pinned tiny_http Unix variants",
                path.display()
            ));
        }
        let source = source
            .replace(
                "#[cfg(unix)]",
                "#[cfg(all(unix, not(target_os = \"trueos\")))]",
            )
            .replace(
                "#[cfg(not(unix))]",
                "#[cfg(any(target_os = \"trueos\", not(unix)))]",
            );
        std::fs::write(path, source).map_err(crate::io_string)?;
    }
    Ok(staged)
}
