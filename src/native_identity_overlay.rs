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

pub(crate) fn stage_gix_config_value(work_dir: &Path, source: &Path) -> Result<PathBuf, String> {
    let manifest = source.join("Cargo.toml");
    if crate::package_name(&manifest)? != "gix-config-value"
        || crate::package_version(&manifest)?.as_deref() != Some("0.21.0")
    {
        return Err("native named-home policy requires pinned gix-config-value 0.21.0".into());
    }
    let original =
        std::fs::read_to_string(source.join("src/path.rs")).map_err(|error| error.to_string())?;
    let boundary = "            target_os = \"android\",";
    let declaration = "    pub fn home_for_user(name: &str) -> Option<PathBuf> {";
    if original.matches(boundary).count() != 2 || original.matches(declaration).count() != 1 {
        return Err("pinned gix-config-value named-user home policy drifted".into());
    }
    // No native account-home directory exists. Plain ~ expansion uses the
    // separately supplied Context.home_dir; no user is mapped to a fake home.
    let selected = original.replace(boundary,
        "            target_os = \"trueos\",\n            target_os = \"android\",")
        .replace(declaration,
        "    #[cfg_attr(target_os = \"trueos\", allow(unused_variables))]\n    pub fn home_for_user(name: &str) -> Option<PathBuf> {")
        .replace("Returns `None` on Windows, Android, and WebAssembly targets other than Emscripten.",
        "Returns `None` on TRUEOS, Windows, Android, and WebAssembly targets other than Emscripten.");
    let staged = work_dir.join("source-overlay-crates/gix-config-value-0.21.0-trueos");
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    std::fs::write(staged.join("src/path.rs"), selected).map_err(|error| error.to_string())?;
    Ok(staged)
}
