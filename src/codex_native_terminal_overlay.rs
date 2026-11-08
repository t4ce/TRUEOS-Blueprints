//! Select the pinned terminal-size provider without Unix descriptor discovery.
use std::path::{Path, PathBuf};

pub(crate) fn stage(work: &Path, source: &Path) -> Result<PathBuf, String> {
    let manifest = source.join("Cargo.toml");
    if crate::package_name(&manifest)? != "terminal_size"
        || crate::package_version(&manifest)?.as_deref() != Some("0.4.3")
    {
        return Err("native terminal geometry requires terminal_size 0.4.3".into());
    }
    let mut text = std::fs::read_to_string(source.join("src/lib.rs"))
        .map_err(|error| error.to_string())?;
    if text.matches("#[cfg(unix)]").count() != 2
        || text.matches("#[cfg(not(any(unix, windows)))]").count() != 1
    {
        return Err("pinned terminal_size platform selectors drifted".into());
    }
    text = text
        .replace("#[cfg(unix)]", "#[cfg(all(unix, not(target_os = \"trueos\")))]")
        .replace(
            "#[cfg(not(any(unix, windows)))]",
            "#[cfg(not(any(unix, windows, target_os = \"trueos\")))]",
        );
    text.push_str("\n#[cfg(target_os = \"trueos\")]\nmod trueos;\n#[cfg(target_os = \"trueos\")]\npub use self::trueos::terminal_size;\n");
    let staged = work.join("source-overlay-crates/terminal_size-0.4.3-trueos");
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    std::fs::write(staged.join("src/lib.rs"), text).map_err(|error| error.to_string())?;
    std::fs::write(staged.join("src/trueos.rs"), include_str!("terminal_size_trueos.rs"))
        .map_err(|error| error.to_string())?;
    Ok(staged)
}
