//! Pinned native providers for Codex's RNG and immutable Git file reads.
use std::path::{Path, PathBuf};

pub(crate) fn stage_rand(work: &Path, source: &Path) -> Result<PathBuf, String> {
    validate(source, "rand", "0.8.6")?;
    let path = source.join("src/rngs/adapter/reseeding.rs");
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let original = "all(unix, not(target_os = \"emscripten\"))";
    if text.matches(original).count() != 2 {
        return Err("pinned rand fork-provider selectors drifted".into());
    }
    let text = text.replace(
        original,
        "all(unix, not(any(target_os = \"emscripten\", target_os = \"trueos\")))",
    );
    let staged = prepare(work, source, "rand-0.8.6-trueos")?;
    std::fs::write(staged.join("src/rngs/adapter/reseeding.rs"), text)
        .map_err(|e| e.to_string())?;
    Ok(staged)
}

pub(crate) fn stage_memmap(work: &Path, source: &Path) -> Result<PathBuf, String> {
    validate(source, "memmap2", "0.9.11")?;
    let mut text = std::fs::read_to_string(source.join("src/lib.rs")).map_err(|e| e.to_string())?;
    replace(
        &mut text,
        "#[cfg_attr(unix, path = \"unix.rs\")]",
        "#[cfg_attr(target_os = \"trueos\", path = \"trueos.rs\")]\n#[cfg_attr(all(unix, not(target_os = \"trueos\")), path = \"unix.rs\")]",
        1,
    )?;
    // A borrowed native File stays a File; its opaque capability is never an FD.
    replace(
        &mut text,
        "#[cfg(not(any(unix, windows)))]",
        "#[cfg(any(target_os = \"trueos\", not(any(unix, windows))))]",
        3,
    )?;
    replace(
        &mut text,
        "#[cfg(unix)]\nuse std::os::unix::io::{AsRawFd, RawFd};",
        "#[cfg(all(unix, not(target_os = \"trueos\")))]\nuse std::os::unix::io::{AsRawFd, RawFd};",
        1,
    )?;
    replace(
        &mut text,
        "#[cfg(unix)]\npub struct MmapRawDescriptor",
        "#[cfg(all(unix, not(target_os = \"trueos\")))]\npub struct MmapRawDescriptor",
        1,
    )?;
    replace(
        &mut text,
        "#[cfg(unix)]\nimpl MmapAsRawDesc for RawFd",
        "#[cfg(all(unix, not(target_os = \"trueos\")))]\nimpl MmapAsRawDesc for RawFd",
        1,
    )?;
    replace(
        &mut text,
        "#[cfg(unix)]\nimpl<T> MmapAsRawDesc for &T",
        "#[cfg(all(unix, not(target_os = \"trueos\")))]\nimpl<T> MmapAsRawDesc for &T",
        1,
    )?;
    let staged = prepare(work, source, "memmap2-0.9.11-trueos")?;
    std::fs::write(staged.join("src/lib.rs"), text).map_err(|e| e.to_string())?;
    std::fs::write(
        staged.join("src/trueos.rs"),
        include_str!("memmap2_trueos.rs"),
    )
    .map_err(|e| e.to_string())?;
    Ok(staged)
}
fn replace(text: &mut String, before: &str, after: &str, expected: usize) -> Result<(), String> {
    if text.matches(before).count() != expected {
        return Err(format!("pinned memmap2 boundary drifted: {before}"));
    }
    *text = text.replace(before, after);
    Ok(())
}
fn validate(source: &Path, name: &str, version: &str) -> Result<(), String> {
    let manifest = source.join("Cargo.toml");
    if crate::package_name(&manifest)? != name
        || crate::package_version(&manifest)?.as_deref() != Some(version)
    {
        return Err(format!(
            "native Codex data provider requires {name} {version}"
        ));
    }
    Ok(())
}
fn prepare(work: &Path, source: &Path, name: &str) -> Result<PathBuf, String> {
    let staged = work.join("source-overlay-crates").join(name);
    crate::reset_dir(&staged)?;
    crate::copy_app_tree(source, &staged)?;
    Ok(staged)
}
