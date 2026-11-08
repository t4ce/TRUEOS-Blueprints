//! Narrow pinned adapters for launch identity metadata.
use std::path::{Path, PathBuf};

pub(crate) fn stage_dirs_sys(work_dir: &Path, source: &Path) -> Result<PathBuf, String> {
    let manifest = source.join("Cargo.toml");
    if crate::package_name(&manifest)? != "dirs-sys"
        || crate::package_version(&manifest)?.as_deref() != Some("0.5.0")
    {
        return Err("native home provider requires pinned dirs-sys 0.5.0".into());
    }
    let path = source.join("src/lib.rs");
    let text = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let selector = "#[cfg(all(unix, not(target_os = \"redox\")))]";
    if text.matches(selector).count() != 3 {
        return Err("pinned dirs-sys Unix home selector drifted".into());
    }
    let selected = text.replace(
        selector,
        "#[cfg(all(unix, not(any(target_os = \"redox\", target_os = \"trueos\"))))]",
    );
    let selected = format!(
        "{selected}\n#[cfg(target_os = \"trueos\")]\nmod trueos;\n#[cfg(target_os = \"trueos\")]\npub use trueos::home_dir;\n"
    );
    let staged = work_dir.join("source-overlay-crates/dirs-sys-0.5.0-trueos");
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    std::fs::write(staged.join("src/lib.rs"), selected).map_err(|error| error.to_string())?;
    std::fs::write(
        staged.join("src/trueos.rs"),
        include_str!("dirs_sys_trueos.rs"),
    )
    .map_err(|error| error.to_string())?;
    Ok(staged)
}
